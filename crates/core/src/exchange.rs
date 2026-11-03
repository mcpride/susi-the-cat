//! Exchange with translators: Excel (`.xlsx`), delimited text (`.csv`/`.txt`) and XLIFF 1.2.
//!
//! Table layout (one header row, labels in the current UI language):
//! `Source | Context | Original [en] | German [de] | German [de] Status | German [de] Comment | … | Row status | Row comment`.
//! Import identifies columns by their header (English or German), rows by source + context (fallback: original text).

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result, read_file, write_file};
use crate::i18n::t;
use crate::lang;
use crate::model::{Project, Row, RowRef};
use crate::status::{RowStatus, TranslationStatus};
use crate::xml::{self, escape_attr, escape_text};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Target languages; empty = all project languages.
    pub langs: Vec<String>,
    /// Export a row if any target language has one of these statuses; empty = all.
    pub statuses: Vec<TranslationStatus>,
    pub row_statuses: Vec<RowStatus>,
    /// Restrict to these rows (e.g. the current view).
    pub rows: Option<Vec<RowRef>>,
    pub status_columns: bool,
    pub comment_columns: bool,
    pub row_status_column: bool,
    pub row_comment_column: bool,
    pub max_chars_column: bool,
    pub include_locked: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            langs: Vec::new(),
            statuses: Vec::new(),
            row_statuses: Vec::new(),
            rows: None,
            status_columns: true,
            comment_columns: true,
            row_status_column: false,
            row_comment_column: true,
            max_chars_column: false,
            include_locked: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Languages to import; empty = all languages found in the file.
    pub langs: Vec<String>,
    /// Overwrite existing, different translations.
    pub overwrite: bool,
    /// Status for imported texts when the file has no status (default: Übersetzt).
    pub status: TranslationStatus,
    /// Skip rows whose original in the file differs from the project's original.
    pub check_original: bool,
    /// Import translator comments and row comments.
    pub comments: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self { langs: Vec::new(), overwrite: true, status: TranslationStatus::Translated, check_original: true, comments: true }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportReport {
    pub updated: usize,
    pub unchanged: usize,
    pub kept_existing: usize,
    pub not_found: usize,
    pub original_changed: usize,
    pub unknown_languages: Vec<String>,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        let mut s = crate::tf!(
            "{updated} translations imported, {unchanged} unchanged, {kept} existing kept, {missing} rows not found, {changed} skipped because the original changed",
            updated = self.updated,
            unchanged = self.unchanged,
            kept = self.kept_existing,
            missing = self.not_found,
            changed = self.original_changed
        );
        if !self.unknown_languages.is_empty() {
            s.push_str(&crate::tf!("; unknown languages: {langs}", langs = self.unknown_languages.join(", ")));
        }
        s
    }
}

fn lang_header(id: &str) -> String {
    format!("{} [{id}]", lang::name(id))
}

fn target_langs(p: &Project, langs: &[String]) -> Vec<String> {
    if langs.is_empty() { p.language_ids() } else { langs.to_vec() }
}

/// Rows selected for export.
pub fn export_rows(p: &Project, opts: &ExportOptions) -> Vec<RowRef> {
    let langs = target_langs(p, &opts.langs);
    let refs = opts.rows.clone().unwrap_or_else(|| p.rows());
    refs.into_iter()
        .filter(|r| {
            let Some(row) = p.row(r) else {
                return false;
            };
            if row.excluded() || row.raw || row.original().is_empty() || (row.locked() && !opts.include_locked) {
                return false;
            }
            if !opts.row_statuses.is_empty() && !opts.row_statuses.contains(&row.status()) {
                return false;
            }
            let orig = p.original_lang(r);
            opts.statuses.is_empty() || langs.iter().filter(|l| l.as_str() != orig).any(|l| opts.statuses.contains(&row.tstatus(l)))
        })
        .collect()
}

/// Builds the exchange table.
pub fn export_table(p: &Project, opts: &ExportOptions) -> Table {
    let langs = target_langs(p, &opts.langs);
    let originals: Vec<String> = {
        let mut v: Vec<String> = p.sources.iter().map(|s| s.original().to_string()).collect();
        v.sort();
        v.dedup();
        v
    };
    let orig_header = if originals.len() == 1 { format!("Original [{}]", originals[0]) } else { "Original".to_string() };
    let mut headers = vec![t("Source").to_string(), t("Context").to_string(), orig_header];
    for l in &langs {
        let h = lang_header(l);
        headers.push(h.clone());
        if opts.status_columns {
            headers.push(format!("{h} Status"));
        }
        if opts.comment_columns {
            headers.push(format!("{h} {}", t("Comment")));
        }
    }
    if opts.row_status_column {
        headers.push(t("Row status").into());
    }
    if opts.row_comment_column {
        headers.push(t("Row comment").into());
    }
    if opts.max_chars_column {
        headers.push(t("Max. characters").into());
    }
    let mut rows = Vec::new();
    for r in export_rows(p, opts) {
        let Some(row) = p.row(&r) else {
            continue;
        };
        let orig = p.original_lang(&r);
        let mut rec = vec![p.sources.get(r.source).map(|s| s.name().to_string()).unwrap_or_default(), p.context(&r), row.original().to_string()];
        for l in &langs {
            let own = l.as_str() == orig;
            rec.push(if own { row.original().to_string() } else { row.text(l).unwrap_or("").to_string() });
            if opts.status_columns {
                rec.push(if own { String::new() } else { row.tstatus(l).label().to_string() });
            }
            if opts.comment_columns {
                rec.push(row.translation(l).map(|t| t.comment().to_string()).unwrap_or_default());
            }
        }
        if opts.row_status_column {
            rec.push(row.status().label().into());
        }
        if opts.row_comment_column {
            rec.push(row.note().into());
        }
        if opts.max_chars_column {
            rec.push(if row.max_chars() > 0 { row.max_chars().to_string() } else { String::new() });
        }
        rows.push(rec);
    }
    Table { headers, rows }
}

#[derive(Debug, Clone, PartialEq)]
enum Col {
    Source,
    Context,
    Original,
    Text(String),
    Status(String),
    Comment(String),
    RowComment,
    Ignore,
}

/// Resolves a language from a header like `Deutsch [de]`, `de`, `Deutsch` or `German`.
fn header_lang(h: &str) -> Option<String> {
    if let (Some(a), Some(b)) = (h.rfind('['), h.rfind(']'))
        && a < b
    {
        return Some(h[a + 1..b].trim().to_string());
    }
    let h = h.trim();
    lang::known_languages().map(|(id, _)| id).find(|id| lang::matches_name(id, h)).map(str::to_string)
}

fn classify(h: &str) -> Col {
    let t = h.trim();
    let lower = t.to_lowercase();
    match lower.as_str() {
        "quelle" | "source" | "datei" | "file" => return Col::Source,
        "kontext" | "context" | "id" | "schlüssel" | "key" => return Col::Context,
        "zeilenkommentar" | "row comment" | "kommentar" | "comment" => return Col::RowComment,
        _ => {}
    }
    if lower.starts_with("original") {
        return Col::Original;
    }
    if let Some(base) = t.strip_suffix(" Status").or_else(|| t.strip_suffix(" status")) {
        return header_lang(base).map_or(Col::Ignore, Col::Status);
    }
    if let Some(base) = t.strip_suffix(" Kommentar").or_else(|| t.strip_suffix(" Comment")).or_else(|| t.strip_suffix(" comment")) {
        return header_lang(base).map_or(Col::Ignore, Col::Comment);
    }
    header_lang(t).map_or(Col::Ignore, Col::Text)
}

fn parse_status(s: &str) -> Option<TranslationStatus> {
    let s = s.trim();
    TranslationStatus::from_label(s)
}

/// Excel and many editors turn CRLF into LF; restore the original's line break style.
fn fix_newlines(original: &str, text: &str) -> String {
    let text = text.replace("_x000D_", "\r");
    if original.contains("\r\n") && !text.contains("\r\n") { text.replace('\n', "\r\n") } else { text }
}

fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

struct RowIndex {
    by_key: HashMap<(String, String), RowRef>,
    by_context: HashMap<String, Vec<RowRef>>,
    by_original: HashMap<String, Vec<RowRef>>,
}

impl RowIndex {
    fn new(p: &Project) -> Self {
        let mut idx = RowIndex { by_key: HashMap::new(), by_context: HashMap::new(), by_original: HashMap::new() };
        for r in p.rows() {
            let ctx = p.context(&r);
            let src = p.sources.get(r.source).map(|s| s.name().to_lowercase()).unwrap_or_default();
            idx.by_key.entry((src, ctx.clone())).or_insert_with(|| r.clone());
            idx.by_context.entry(ctx).or_default().push(r.clone());
            if let Some(row) = p.row(&r) {
                idx.by_original.entry(row.original().replace("\r\n", "\n")).or_default().push(r.clone());
            }
        }
        idx
    }

    fn find(&self, source: Option<&str>, context: Option<&str>, original: Option<&str>) -> Vec<RowRef> {
        if let Some(ctx) = context.filter(|c| !c.is_empty()) {
            if let Some(src) = source.filter(|s| !s.is_empty()) {
                return self.by_key.get(&(src.to_lowercase(), ctx.to_string())).cloned().into_iter().collect();
            }
            return self.by_context.get(ctx).cloned().unwrap_or_default();
        }
        original.and_then(|o| self.by_original.get(&o.replace("\r\n", "\n"))).cloned().unwrap_or_default()
    }
}

/// One cell value to import.
struct Cell<'a> {
    lang: &'a str,
    text: &'a str,
    status: Option<TranslationStatus>,
    comment: Option<&'a str>,
}

fn import_cell(row: &mut Row, cell: &Cell<'_>, opts: &ImportOptions, rep: &mut ImportReport) {
    let text = fix_newlines(row.original(), cell.text);
    if let Some(c) = cell.comment.filter(|_| opts.comments)
        && (row.translation(cell.lang).is_some() || !text.is_empty())
        && let Some(t) = row.translation_mut(cell.lang)
    {
        t.set_comment(c);
    }
    if text.is_empty() {
        return;
    }
    let status = cell.status.filter(|s| *s != TranslationStatus::NotTranslated).unwrap_or(opts.status);
    match row.text(cell.lang) {
        Some(cur) if cur == text => {
            if cell.status.is_some() && row.tstatus(cell.lang) != status {
                row.set_tstatus(cell.lang, status);
                rep.updated += 1;
            } else {
                rep.unchanged += 1;
            }
            return;
        }
        Some(_) if !opts.overwrite => {
            rep.kept_existing += 1;
            return;
        }
        _ => {}
    }
    row.set_translation(cell.lang, &text, status);
    if let Some(c) = cell.comment.filter(|c| opts.comments && !c.is_empty())
        && let Some(t) = row.translation_mut(cell.lang)
    {
        t.set_comment(c);
    }
    rep.updated += 1;
}

/// Imports a table produced by [`export_table`] (or edited by a translator).
pub fn import_table(p: &mut Project, t: &Table, opts: &ImportOptions) -> Result<ImportReport> {
    let cols: Vec<Col> = t.headers.iter().map(|h| classify(h)).collect();
    let pos = |c: &Col| cols.iter().position(|x| x == c);
    let (src_col, ctx_col, orig_col, note_col) = (pos(&Col::Source), pos(&Col::Context), pos(&Col::Original), pos(&Col::RowComment));
    if ctx_col.is_none() && orig_col.is_none() {
        return Err(Error::Format(crate::tf!("Column \"{context}\" or \"Original\" is missing", context = crate::i18n::t("Context"))));
    }
    let mut rep = ImportReport::default();
    let mut langs: Vec<String> = cols.iter().filter_map(|c| if let Col::Text(l) = c { Some(l.clone()) } else { None }).collect();
    langs.retain(|l| opts.langs.is_empty() || opts.langs.contains(l));
    rep.unknown_languages = langs.iter().filter(|l| !p.has_language(l)).cloned().collect();
    langs.retain(|l| p.has_language(l));
    let text_cols: Vec<(String, usize, Option<usize>, Option<usize>)> =
        langs.iter().filter_map(|l| Some((l.clone(), pos(&Col::Text(l.clone()))?, pos(&Col::Status(l.clone())), pos(&Col::Comment(l.clone()))))).collect();
    let idx = RowIndex::new(p);
    for rec in &t.rows {
        let get = |i: Option<usize>| i.and_then(|i| rec.get(i)).map(String::as_str);
        let found = idx.find(get(src_col), get(ctx_col), get(orig_col));
        if found.is_empty() {
            rep.not_found += 1;
            continue;
        }
        for r in found {
            let orig_lang = p.original_lang(&r).to_string();
            let Some(row) = p.row_mut(&r) else {
                continue;
            };
            if opts.check_original && get(orig_col).is_some_and(|o| !same_text(o, row.original())) {
                rep.original_changed += 1;
                continue;
            }
            if let Some(n) = get(note_col).filter(|_| opts.comments && row.attrs.get("notereadonly") != Some("1")) {
                row.set_note(n);
            }
            for (l, ti, si, ci) in text_cols.iter().filter(|c| c.0 != orig_lang) {
                let cell = Cell { lang: l, text: get(Some(*ti)).unwrap_or(""), status: get(*si).and_then(parse_status), comment: get(*ci) };
                import_cell(row, &cell, opts, &mut rep);
            }
        }
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------------------------
// Excel

pub fn write_xlsx(t: &Table, path: &Path) -> Result<()> {
    use rust_xlsxwriter::{Color, Format, FormatBorder, Workbook};
    let xe = |e: rust_xlsxwriter::XlsxError| Error::Excel(e.to_string());
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name(crate::i18n::t("Translation")).map_err(xe)?;
    let head = Format::new().set_bold().set_background_color(Color::RGB(0x00DD_E6F0)).set_border_bottom(FormatBorder::Thin);
    let wrap = Format::new().set_text_wrap();
    for (c, h) in t.headers.iter().enumerate() {
        let c = u16::try_from(c).map_err(|_| Error::Excel(crate::i18n::t("too many columns").into()))?;
        ws.write_string_with_format(0, c, h, &head).map_err(xe)?;
        let width = if h.ends_with(" Status") {
            16.0
        } else if c < 2 {
            24.0
        } else {
            40.0
        };
        ws.set_column_width(c, width).map_err(xe)?;
    }
    for (r, rec) in t.rows.iter().enumerate() {
        let r = u32::try_from(r + 1).map_err(|_| Error::Excel(crate::i18n::t("too many rows").into()))?;
        for (c, v) in rec.iter().enumerate() {
            if v.is_empty() {
                continue;
            }
            let c = u16::try_from(c).map_err(|_| Error::Excel(crate::i18n::t("too many columns").into()))?;
            if v.contains('\n') {
                ws.write_string_with_format(r, c, v, &wrap).map_err(xe)?;
            } else {
                ws.write_string(r, c, v).map_err(xe)?;
            }
        }
    }
    ws.set_freeze_panes(1, 3).map_err(xe)?;
    if !t.headers.is_empty() {
        let last_col = u16::try_from(t.headers.len() - 1).unwrap_or(u16::MAX);
        let last_row = u32::try_from(t.rows.len()).unwrap_or(u32::MAX);
        ws.autofilter(0, 0, last_row, last_col).map_err(xe)?;
    }
    wb.save(path).map_err(xe)
}

pub fn read_xlsx(path: &Path) -> Result<Table> {
    use calamine::{Data, Reader, open_workbook_auto};
    let mut wb = open_workbook_auto(path).map_err(|e| Error::Excel(format!("{}: {e}", path.display())))?;
    let name = wb.sheet_names().first().cloned().ok_or_else(|| Error::Excel(t("workbook has no worksheet").into()))?;
    let range = wb.worksheet_range(&name).map_err(|e| Error::Excel(e.to_string()))?;
    let mut rows = range.rows().map(|r| r.iter().map(|c| if matches!(c, Data::Empty) { String::new() } else { c.to_string() }).collect::<Vec<_>>());
    let headers = rows.next().unwrap_or_default();
    Ok(Table { headers, rows: rows.collect() })
}

// ---------------------------------------------------------------------------------------------
// Delimited text

/// UTF-8 (with BOM) delimited text, CRLF line ends, fields quoted when needed.
pub fn write_delimited(t: &Table, delim: char) -> Vec<u8> {
    let quote = |s: &str| {
        if s.contains(delim) || s.contains(['"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
    };
    let mut s = String::from("\u{FEFF}");
    for rec in std::iter::once(&t.headers).chain(t.rows.iter()) {
        let line: Vec<String> = rec.iter().map(|f| quote(f)).collect();
        s.push_str(&line.join(&delim.to_string()));
        s.push_str("\r\n");
    }
    s.into_bytes()
}

/// Parses delimited text; the delimiter is detected from the header line when `None`.
pub fn read_delimited(bytes: &[u8], delim: Option<char>) -> Result<Table> {
    let (text, _) = xml::decode_text(bytes)?;
    let first = text.lines().next().unwrap_or("");
    let delim = delim.unwrap_or_else(|| ['\t', ';', ','].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or('\t'));
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut rec: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == delim {
            rec.push(std::mem::take(&mut field));
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            rec.push(std::mem::take(&mut field));
            records.push(std::mem::take(&mut rec));
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !rec.is_empty() {
        rec.push(field);
        records.push(rec);
    }
    records.retain(|r| r.iter().any(|f| !f.is_empty()));
    let mut it = records.into_iter();
    Ok(Table { headers: it.next().unwrap_or_default(), rows: it.collect() })
}

// ---------------------------------------------------------------------------------------------
// XLIFF 1.2

fn xliff_state(s: TranslationStatus) -> &'static str {
    match s {
        TranslationStatus::NotTranslated => "needs-translation",
        TranslationStatus::BestGuess | TranslationStatus::AutoTranslated => "needs-review-translation",
        TranslationStatus::Translated => "translated",
        TranslationStatus::ForReview => "needs-review-translation",
        TranslationStatus::Complete => "final",
    }
}

fn status_from_xliff(state: Option<&str>, default: TranslationStatus) -> TranslationStatus {
    match state.unwrap_or("") {
        "final" | "signed-off" => TranslationStatus::Complete,
        "translated" => TranslationStatus::Translated,
        s if s.starts_with("needs-review") => TranslationStatus::ForReview,
        "needs-adaptation" | "needs-l10n" => TranslationStatus::BestGuess,
        _ => default,
    }
}

/// XLIFF 1.2 document for one target language (one `<file>` per source).
pub fn export_xliff(p: &Project, lang: &str, opts: &ExportOptions) -> String {
    let o = ExportOptions { langs: vec![lang.to_string()], ..opts.clone() };
    let rows = export_rows(p, &o);
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xliff version=\"1.2\" xmlns=\"urn:oasis:names:tc:xliff:document:1.2\">\n");
    for (si, src) in p.sources.iter().enumerate() {
        if src.original() == lang {
            continue;
        }
        let in_src: Vec<&RowRef> = rows.iter().filter(|r| r.source == si).collect();
        if in_src.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "  <file original=\"{}\" source-language=\"{}\" target-language=\"{}\" datatype=\"plaintext\" tool-id=\"susi\">\n    <header><tool tool-id=\"susi\" tool-name=\"Susi the CAT\" tool-version=\"{}\"/></header>\n    <body>\n",
            escape_attr(src.name()),
            escape_attr(&lang::tag(src.original())),
            escape_attr(&lang::tag(lang)),
            env!("CARGO_PKG_VERSION")
        ));
        for r in in_src {
            let Some(row) = p.row(r) else {
                continue;
            };
            let ctx = p.context(r);
            let max = if row.max_chars() > 0 { format!(" maxwidth=\"{}\" size-unit=\"char\"", row.max_chars()) } else { String::new() };
            s.push_str(&format!("      <trans-unit id=\"{}\" resname=\"{}\" xml:space=\"preserve\"{max}>\n", escape_attr(&ctx), escape_attr(row.id())));
            s.push_str(&format!("        <source>{}</source>\n", escape_text(row.original())));
            let st = row.tstatus(lang);
            s.push_str(&format!("        <target state=\"{}\">{}</target>\n", xliff_state(st), escape_text(row.text(lang).unwrap_or(""))));
            if !row.note().is_empty() {
                s.push_str(&format!("        <note from=\"developer\">{}</note>\n", escape_text(row.note())));
            }
            if let Some(c) = row.translation(lang).map(|t| t.comment()).filter(|c| !c.is_empty()) {
                s.push_str(&format!("        <note from=\"translator\">{}</note>\n", escape_text(c)));
            }
            s.push_str("      </trans-unit>\n");
        }
        s.push_str("    </body>\n  </file>\n");
    }
    s.push_str("</xliff>\n");
    s
}

fn xliff_inner_text(e: &xml::Element) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c {
            xml::Node::Text(t) | xml::Node::CData(t) => s.push_str(t),
            xml::Node::Element(x) => s.push_str(&xliff_inner_text(x)),
            _ => {}
        }
    }
    s
}

fn find_lang(p: &Project, tag: &str) -> Option<String> {
    let ids = p.language_ids();
    ids.iter()
        .find(|id| lang::tag(id).eq_ignore_ascii_case(tag) || id.eq_ignore_ascii_case(tag))
        .or_else(|| ids.iter().find(|id| id.split('-').next() == tag.split(['-', '_']).next()))
        .cloned()
}

/// Imports an XLIFF 1.2 file (as produced by [`export_xliff`] or a CAT tool).
pub fn import_xliff(p: &mut Project, bytes: &[u8], opts: &ImportOptions) -> Result<ImportReport> {
    let doc = xml::parse_bytes(bytes)?;
    if doc.root.name != "xliff" {
        return Err(Error::Format(t("not an XLIFF file").into()));
    }
    let idx = RowIndex::new(p);
    let mut rep = ImportReport::default();
    for file in doc.root.elements().filter(|e| e.name == "file") {
        let tag = file.attr("target-language").unwrap_or("");
        let Some(lang) = find_lang(p, tag) else {
            rep.unknown_languages.push(tag.to_string());
            continue;
        };
        if !opts.langs.is_empty() && !opts.langs.contains(&lang) {
            continue;
        }
        let original = file.attr("original");
        let Some(body) = file.child("body") else {
            continue;
        };
        let mut units = Vec::new();
        collect_units(body, &mut units);
        for tu in units {
            let source = tu.child("source").map(xliff_inner_text);
            let found = idx.find(original, tu.attr("id"), source.as_deref());
            let found = if found.is_empty() { idx.find(None, tu.attr("id"), source.as_deref()) } else { found };
            if found.is_empty() {
                rep.not_found += 1;
                continue;
            }
            let Some(target) = tu.child("target") else {
                continue;
            };
            let text = xliff_inner_text(target);
            let status = status_from_xliff(target.attr("state"), opts.status);
            let note = tu.elements().find(|e| e.name == "note" && e.attr("from") == Some("translator")).map(xliff_inner_text);
            for r in found {
                if p.original_lang(&r) == lang {
                    continue;
                }
                let Some(row) = p.row_mut(&r) else {
                    continue;
                };
                if opts.check_original && source.as_deref().is_some_and(|s| !same_text(s, row.original())) {
                    rep.original_changed += 1;
                    continue;
                }
                let cell = Cell { lang: &lang, text: &text, status: Some(status), comment: note.as_deref() };
                import_cell(row, &cell, opts, &mut rep);
            }
        }
    }
    Ok(rep)
}

fn collect_units<'a>(e: &'a xml::Element, out: &mut Vec<&'a xml::Element>) {
    for c in e.elements() {
        if c.name == "trans-unit" {
            out.push(c);
        } else if c.name == "group" {
            collect_units(c, out);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// File level helpers

/// Exchange file types by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeFormat {
    Excel,
    Csv,
    Text,
    Xliff,
    Tmx,
}

impl ExchangeFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "xlsx" | "xlsm" | "xls" | "ods" => Some(Self::Excel),
            "csv" => Some(Self::Csv),
            "txt" | "tsv" | "tab" => Some(Self::Text),
            "xlf" | "xliff" => Some(Self::Xliff),
            "tmx" => Some(Self::Tmx),
            _ => None,
        }
    }
}

fn format_of(path: &Path) -> Result<ExchangeFormat> {
    ExchangeFormat::from_path(path).ok_or_else(|| Error::Unsupported(crate::tf!("unknown exchange format: {path}", path = path.display())))
}

/// Path for language `lang` when exporting XLIFF for several languages: `name.<lang>.xlf`.
pub fn per_language_path(path: &Path, lang: &str) -> std::path::PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("export");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("xlf");
    path.with_file_name(format!("{stem}.{lang}.{ext}"))
}

/// Exports to `path` (format by extension). Returns the written files.
pub fn export_file(p: &Project, path: &Path, opts: &ExportOptions) -> Result<Vec<std::path::PathBuf>> {
    match format_of(path)? {
        ExchangeFormat::Excel => {
            write_xlsx(&export_table(p, opts), path)?;
            Ok(vec![path.to_path_buf()])
        }
        ExchangeFormat::Csv => {
            write_file(path, &write_delimited(&export_table(p, opts), ';'))?;
            Ok(vec![path.to_path_buf()])
        }
        ExchangeFormat::Text => {
            write_file(path, &write_delimited(&export_table(p, opts), '\t'))?;
            Ok(vec![path.to_path_buf()])
        }
        ExchangeFormat::Xliff => {
            let langs = target_langs(p, &opts.langs);
            let mut out = Vec::new();
            for l in &langs {
                let target = if langs.len() == 1 { path.to_path_buf() } else { per_language_path(path, l) };
                write_file(&target, export_xliff(p, l, opts).as_bytes())?;
                out.push(target);
            }
            Ok(out)
        }
        ExchangeFormat::Tmx => {
            let mut copy = p.clone();
            let mut tm = crate::tm::TranslationMemory::new();
            let all: Vec<TranslationStatus> = TranslationStatus::ALL.into_iter().filter(|s| *s != TranslationStatus::NotTranslated).collect();
            let statuses = if opts.statuses.is_empty() { all } else { opts.statuses.clone() };
            crate::tm::save_to_tm(&mut copy, &mut tm, &opts.langs, &statuses, false);
            tm.save_as(path)?;
            Ok(vec![path.to_path_buf()])
        }
    }
}

/// Imports translations from `path` (format by extension).
pub fn import_file(p: &mut Project, path: &Path, opts: &ImportOptions) -> Result<ImportReport> {
    match format_of(path)? {
        ExchangeFormat::Excel => import_table(p, &read_xlsx(path)?, opts),
        ExchangeFormat::Csv | ExchangeFormat::Text => import_table(p, &read_delimited(&read_file(path)?, None)?, opts),
        ExchangeFormat::Xliff => import_xliff(p, &read_file(path)?, opts),
        ExchangeFormat::Tmx => {
            let tm = crate::tm::TranslationMemory::open(path)?;
            let t = crate::tm::TranslateOptions { langs: opts.langs.clone(), overwrite: opts.overwrite, fuzzy: None };
            let r = crate::tm::translate_project(p, &tm, &t, None);
            Ok(ImportReport { updated: r.exact, not_found: r.not_found, ..Default::default() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Item, Source, SourceKind};

    fn project() -> Project {
        let mut p = Project::new();
        p.add_language("de");
        p.add_language("fr");
        let mut s = Source::new(SourceKind::Json, "a.json", "en");
        let mut a = Row::new("hello", "Hello");
        a.set_translation("de", "Hallo", TranslationStatus::Translated);
        s.items.push(Item::Row(a));
        s.items.push(Item::Row(Row::new("multi", "Line 1\r\nLine \"2\"; x")));
        p.sources.push(s);
        p
    }

    #[test]
    fn table_roundtrip_csv() {
        let p = project();
        let t = export_table(&p, &ExportOptions::default());
        assert_eq!(t.headers[..4], ["Source", "Context", "Original [en]", "German [de]"]);
        let mut t2 = read_delimited(&write_delimited(&t, ';'), None).unwrap();
        assert_eq!(t2, t);
        // translator fills French for row 2 (Excel style LF line break)
        let fr = t2.headers.iter().position(|h| h == "French [fr]").unwrap();
        t2.rows[1][fr] = "Ligne 1\nLigne 2".into();
        let mut q = project();
        let rep = import_table(&mut q, &t2, &ImportOptions::default()).unwrap();
        assert_eq!(rep.updated, 1, "{rep:?}");
        let r = q.find_row("multi").unwrap();
        assert_eq!(q.row(&r).unwrap().text("fr"), Some("Ligne 1\r\nLigne 2"));
        assert_eq!(rep.unchanged, 1);
    }

    #[test]
    fn xliff_roundtrip() {
        let p = project();
        let x = export_xliff(&p, "de", &ExportOptions::default());
        let x = x.replace("<target state=\"needs-translation\"></target>", "<target state=\"final\">Zeile 1\r\nZeile \"2\"; x</target>");
        let mut q = project();
        let rep = import_xliff(&mut q, x.as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!(rep.updated, 1, "{rep:?}");
        let r = q.find_row("multi").unwrap();
        assert_eq!(q.row(&r).unwrap().tstatus("de"), TranslationStatus::Complete);
    }

    #[test]
    fn xlsx_roundtrip() {
        let p = project();
        let t = export_table(&p, &ExportOptions::default());
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.xlsx");
        write_xlsx(&t, &f).unwrap();
        let back = read_xlsx(&f).unwrap();
        assert_eq!(back.headers, t.headers);
        assert_eq!(back.rows[0][3], "Hallo");
        assert_eq!(back.rows[1][2].replace("\r\n", "\n"), "Line 1\nLine \"2\"; x");
    }
}
