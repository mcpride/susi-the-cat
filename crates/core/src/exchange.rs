//! Exchange with translators: Excel (`.xlsx`), delimited text (`.csv`/`.txt`) and XLIFF 1.2.
//!
//! Table layout (one header row, labels in a UI language, by default the current one):
//! `Source | Context | Original [en] | German [de] | German [de] Status | German [de] Comment | … | Row status | Row comment`.
//! Import identifies columns by their header (in any UI language), rows by source + context (fallback: original text).
//!
//! Screenshots linked to the exported rows (see [`crate::screenshots`]) are added as columns
//! `Screenshot`, `Screenshot 2`, … (one per screenshot of a row, as an Excel cell holds only one
//! link): in Excel by default as links to a second worksheet showing each image once with numbered
//! frames around the marked areas, otherwise as relative paths of images copied to
//! `<name>_screenshots/` (XLIFF: `<context-group>` per trans-unit). There, the frame of a marked
//! area is drawn into a copy of the image per area. Import ignores them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, read_file, write_file};
use crate::i18n::{Lang, t, tr};
use crate::lang;
use crate::model::{Project, Row, RowRef};
use crate::screenshots::{Area, Screenshot, ScreenshotStore};
use crate::status::{RowStatus, TranslationStatus};
use crate::xml::{self, escape_attr, escape_text};

/// How screenshots linked to the exported rows are included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotExport {
    /// `Embed` for Excel workbooks, `Link` for the other formats.
    #[default]
    Auto,
    None,
    /// Images copied next to the export (`<name>_screenshots/`), referenced by relative path; a
    /// marked area is framed in a copy of the image per area.
    Link,
    /// Excel: images on a second worksheet with numbered frames; other formats like `Link`.
    Embed,
}

impl ScreenshotExport {
    pub const ALL: [ScreenshotExport; 4] = [Self::Auto, Self::None, Self::Link, Self::Embed];

    /// Keyword of the CLI (`-screenshots:`) and MCP.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Link => "link",
            Self::Embed => "embed",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|m| m.keyword() == s)
    }

    /// `Some(embed)` if screenshots are exported to `format`.
    fn mode(self, format: ExchangeFormat) -> Option<bool> {
        match (self, format) {
            (Self::None, _) | (_, ExchangeFormat::Tmx) => None,
            (Self::Auto | Self::Embed, ExchangeFormat::Excel) => Some(true),
            _ => Some(false),
        }
    }
}

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
    /// The original texts (tables only; XLIFF always has them). Without them, the import finds
    /// rows by context only and cannot skip rows whose original has changed.
    pub original_column: bool,
    pub status_columns: bool,
    pub comment_columns: bool,
    pub row_status_column: bool,
    pub row_comment_column: bool,
    pub max_chars_column: bool,
    pub include_locked: bool,
    /// UI language of the column headers, status values and worksheet name of tables (Excel,
    /// CSV, text); `None` = the current UI language.
    pub label_lang: Option<Lang>,
    /// Screenshots of the exported rows (only with a store passed to [`export_file`]).
    pub screenshots: ScreenshotExport,
}

impl ExportOptions {
    fn label_lang(&self) -> Lang {
        self.label_lang.unwrap_or_else(crate::i18n::current)
    }
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            langs: Vec::new(),
            statuses: Vec::new(),
            row_statuses: Vec::new(),
            rows: None,
            original_column: true,
            status_columns: true,
            comment_columns: true,
            row_status_column: false,
            row_comment_column: true,
            max_chars_column: false,
            include_locked: false,
            label_lang: None,
            screenshots: ScreenshotExport::Auto,
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
    /// Rows of the file skipped because they are not translated ("Do not translate", excluded,
    /// custom data).
    pub not_translatable: usize,
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
        if self.not_translatable > 0 {
            s.push_str(&crate::tf!("; {skipped} rows skipped (\"Do not translate\" or excluded)", skipped = self.not_translatable));
        }
        if !self.unknown_languages.is_empty() {
            s.push_str(&crate::tf!("; unknown languages: {langs}", langs = self.unknown_languages.join(", ")));
        }
        s
    }
}

fn lang_header(ui: Lang, id: &str) -> String {
    format!("{} [{id}]", lang::name_in(ui, id))
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
            if row.excluded() || row.is_data_or_raw() || row.original().is_empty() || (row.locked() && !opts.include_locked) {
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
    table_of(p, opts, &export_rows(p, opts), None)
}

/// The exchange table of `rows`, with a screenshot column if `shots` is given.
fn table_of(p: &Project, opts: &ExportOptions, rows: &[RowRef], shots: Option<&ShotPlan<'_>>) -> Table {
    let langs = target_langs(p, &opts.langs);
    let originals: Vec<String> = {
        let mut v: Vec<String> = p.sources.iter().map(|s| s.original().to_string()).collect();
        v.sort();
        v.dedup();
        v
    };
    let ui = opts.label_lang();
    let mut headers = vec![tr(ui, "Source").to_string(), tr(ui, "Context").to_string()];
    if opts.original_column {
        headers.push(if originals.len() == 1 { format!("Original [{}]", originals[0]) } else { "Original".to_string() });
    }
    for l in &langs {
        let h = lang_header(ui, l);
        headers.push(h.clone());
        if opts.status_columns {
            headers.push(format!("{h} Status"));
        }
        if opts.comment_columns {
            headers.push(format!("{h} {}", tr(ui, "Comment")));
        }
    }
    if opts.row_status_column {
        headers.push(tr(ui, "Row status").into());
    }
    if opts.row_comment_column {
        headers.push(tr(ui, "Row comment").into());
    }
    if opts.max_chars_column {
        headers.push(tr(ui, "Max. characters").into());
    }
    let shot_columns = shots.map_or(0, ShotPlan::columns);
    for n in 1..=shot_columns {
        let h = tr(ui, "Screenshot");
        headers.push(if n == 1 { h.into() } else { format!("{h} {n}") });
    }
    let mut out = Vec::new();
    for r in rows {
        let Some(row) = p.row(r) else {
            continue;
        };
        let orig = p.original_lang(r);
        let mut rec = vec![p.sources.get(r.source).map(|s| s.name().to_string()).unwrap_or_default(), p.context(r)];
        if opts.original_column {
            rec.push(row.original().to_string());
        }
        for l in &langs {
            let own = l.as_str() == orig;
            rec.push(if own { row.original().to_string() } else { row.text(l).unwrap_or("").to_string() });
            if opts.status_columns {
                rec.push(if own { String::new() } else { row.tstatus(l).label_in(ui).to_string() });
            }
            if opts.comment_columns {
                rec.push(row.translation(l).map(|t| t.comment().to_string()).unwrap_or_default());
            }
        }
        if opts.row_status_column {
            rec.push(row.status().label_in(ui).into());
        }
        if opts.row_comment_column {
            rec.push(row.note().into());
        }
        if opts.max_chars_column {
            rec.push(if row.max_chars() > 0 { row.max_chars().to_string() } else { String::new() });
        }
        if let Some(s) = shots {
            let mut cells: Vec<String> = s.entries(r).into_iter().map(|(text, _)| text).collect();
            cells.resize(shot_columns, String::new());
            rec.extend(cells);
        }
        out.push(rec);
    }
    Table { headers, rows: out }
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

/// `s` without `suffix` (ASCII case-insensitive).
fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let cut = s.len().checked_sub(suffix.len())?;
    (s.is_char_boundary(cut) && s[cut..].eq_ignore_ascii_case(suffix)).then(|| &s[..cut])
}

/// Column of a header written in any UI language (tables may come from another Susi user).
fn classify(h: &str) -> Col {
    let t = h.trim();
    let lower = t.to_lowercase();
    // Further names used in hand-made tables.
    let is = |english: &str, extra: &[&str]| crate::i18n::matches_any(english, t) || extra.contains(&lower.as_str());
    if is("Source", &["datei", "file"]) {
        return Col::Source;
    }
    if is("Context", &["id", "schlüssel", "key"]) {
        return Col::Context;
    }
    if is("Row comment", &[]) || is("Comment", &[]) {
        return Col::RowComment;
    }
    // Exported for information only (`Screenshot`, `Screenshot 2`, …); never mistaken for a
    // language column.
    let unnumbered = t.rsplit_once(' ').filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).map_or(t, |(base, _)| base.trim_end());
    if is("Screenshot", &["screenshots"]) || crate::i18n::matches_any("Screenshot", unnumbered) {
        return Col::Ignore;
    }
    if lower.starts_with("original") {
        return Col::Original;
    }
    if let Some(base) = strip_suffix_ci(t, " Status") {
        return header_lang(base).map_or(Col::Ignore, Col::Status);
    }
    if let Some(base) = Lang::all().find_map(|l| strip_suffix_ci(t, &format!(" {}", tr(l, "Comment")))) {
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

/// Whether an import may change `row`: not for rows marked "Do not translate", excluded rows and
/// custom data, as for the translation memory and copying.
fn importable(row: &Row) -> bool {
    !(row.locked() || row.excluded() || row.is_data_or_raw())
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
            if !importable(row) {
                rep.not_translatable += 1;
                continue;
            }
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

/// Writes the table as a workbook; `ui`: UI language of the worksheet name.
pub fn write_xlsx(t: &Table, path: &Path, ui: Lang) -> Result<()> {
    write_workbook(t, path, ui, None)
}

fn xe(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::Excel(e.to_string())
}

fn header_format() -> rust_xlsxwriter::Format {
    use rust_xlsxwriter::{Color, Format, FormatBorder};
    Format::new().set_bold().set_background_color(Color::RGB(0x00DD_E6F0)).set_border_bottom(FormatBorder::Thin)
}

/// Screenshots of an Excel export: each screenshot column of the table links to one screenshot of
/// the row (on the second worksheet or the copied image file).
struct Workbook<'a> {
    plan: &'a ShotPlan<'a>,
    /// Table rows (without header) in order.
    rows: &'a [RowRef],
    p: &'a Project,
}

/// Number of leading columns that identify the rows (source, context, original): they stay
/// visible when scrolling to the right.
fn key_columns(t: &Table) -> u16 {
    let n = t.headers.iter().take_while(|h| matches!(classify(h), Col::Source | Col::Context | Col::Original)).count();
    u16::try_from(n).unwrap_or(u16::MAX)
}

fn write_workbook(t: &Table, path: &Path, ui: Lang, shots: Option<&Workbook<'_>>) -> Result<()> {
    use rust_xlsxwriter::{Format, Url};
    let mut wb = rust_xlsxwriter::Workbook::new();
    let sheet = tr(ui, "Screenshots");
    let layout = match shots.filter(|s| s.plan.embed) {
        Some(s) => s.plan.layout()?,
        None => Vec::new(),
    };
    let ws = wb.add_worksheet();
    ws.set_name(tr(ui, "Translation")).map_err(xe)?;
    let head = header_format();
    let wrap = Format::new().set_text_wrap();
    // The screenshot columns are the last ones.
    let first_shot_col = shots.and_then(|s| t.headers.len().checked_sub(s.plan.columns())).unwrap_or(usize::MAX);
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
    for (ri, rec) in t.rows.iter().enumerate() {
        let r = u32::try_from(ri + 1).map_err(|_| Error::Excel(crate::i18n::t("too many rows").into()))?;
        for (ci, v) in rec.iter().enumerate() {
            if v.is_empty() {
                continue;
            }
            let c = u16::try_from(ci).map_err(|_| Error::Excel(crate::i18n::t("too many columns").into()))?;
            let link = shots.zip(ci.checked_sub(first_shot_col)).and_then(|(s, k)| {
                let entries = s.plan.entries(s.rows.get(ri)?);
                let (_, m) = entries.get(k)?;
                if s.plan.embed {
                    Some(format!("internal:'{}'!A{}", sheet.replace('\'', "''"), layout.get(m.shot)?.title_row + 1))
                } else {
                    Some(format!("file:///{}", s.plan.rel(m)?))
                }
            });
            if let Some(link) = link {
                ws.write_url_with_text(r, c, Url::new(link), v).map_err(xe)?;
            } else if v.contains('\n') {
                ws.write_string_with_format(r, c, v, &wrap).map_err(xe)?;
            } else {
                ws.write_string(r, c, v).map_err(xe)?;
            }
        }
    }
    ws.set_freeze_panes(1, key_columns(t)).map_err(xe)?;
    if !t.headers.is_empty() {
        let last_col = u16::try_from(t.headers.len() - 1).unwrap_or(u16::MAX);
        let last_row = u32::try_from(t.rows.len()).unwrap_or(u32::MAX);
        ws.autofilter(0, 0, last_row, last_col).map_err(xe)?;
    }
    if let Some(s) = shots.filter(|s| s.plan.embed) {
        let ws = wb.add_worksheet();
        ws.set_name(sheet).map_err(xe)?;
        s.write_sheet(ws, &layout, ui)?;
    }
    wb.save(path).map_err(xe)
}

// ---------------------------------------------------------------------------------------------
// Screenshots

/// A screenshot shown for an exported row.
#[derive(Debug, Clone)]
struct Mark {
    /// Position in [`ShotPlan::shots`].
    shot: usize,
    /// Number of the row on that screenshot (1-based).
    no: usize,
    row: RowRef,
    /// Marked area, clipped to the image.
    area: Option<Area>,
}

/// The screenshots (with an existing image) of the exported rows.
struct ShotPlan<'a> {
    store: &'a ScreenshotStore,
    /// Excel: on a second worksheet instead of copied files.
    embed: bool,
    /// Folder of the copied images next to the export (`<name>_screenshots`) and its name.
    dir: PathBuf,
    dir_name: String,
    /// In order of first use.
    shots: Vec<&'a Screenshot>,
    marks: HashMap<RowRef, Vec<Mark>>,
    /// Per entry of `shots`: its rows in export order.
    by_shot: Vec<Vec<Mark>>,
}

impl<'a> ShotPlan<'a> {
    /// `None` if none of `rows` has a screenshot.
    fn new(p: &Project, store: &'a ScreenshotStore, rows: &[RowRef], embed: bool, export: &Path) -> Option<Self> {
        let idx = store.index(p);
        let images = store.image_dir();
        let mut shots: Vec<&'a Screenshot> = Vec::new();
        let mut by_shot: Vec<Vec<Mark>> = Vec::new();
        let mut marks: HashMap<RowRef, Vec<Mark>> = HashMap::new();
        let mut pos: HashMap<&'a str, usize> = HashMap::new();
        for r in rows {
            for (s, l) in store.links_of(&idx, r) {
                if !images.join(&s.file).is_file() {
                    continue;
                }
                let k = *pos.entry(s.id.as_str()).or_insert_with(|| {
                    shots.push(s);
                    by_shot.push(Vec::new());
                    shots.len() - 1
                });
                let m = Mark { shot: k, no: by_shot[k].len() + 1, row: r.clone(), area: l.area.and_then(|a| a.clip(s.width, s.height)) };
                by_shot[k].push(m.clone());
                marks.entry(r.clone()).or_default().push(m);
            }
        }
        if shots.is_empty() {
            return None;
        }
        let stem = export.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "export".into());
        let dir_name = format!("{stem}_screenshots");
        Some(Self { store, embed, dir: export.with_file_name(&dir_name), dir_name, shots, marks, by_shot })
    }

    /// Name of the image copied to [`Self::dir`] for `m`: the screenshot itself, or for a marked
    /// area a copy with its frame drawn in (`<stem>.<x>-<y>-<w>-<h>.<ext>`; the stored file names
    /// `<id>.<hash>.<ext>` contain no further dot, so they cannot collide).
    fn file_name(&self, m: &Mark) -> Option<String> {
        let file = &self.shots.get(m.shot)?.file;
        Some(match m.area {
            None => file.clone(),
            Some(a) => {
                let area = format!("{}-{}-{}-{}", a.x, a.y, a.w, a.h);
                match file.rsplit_once('.') {
                    Some((stem, ext)) => format!("{stem}.{area}.{ext}"),
                    None => format!("{file}.{area}"),
                }
            }
        })
    }

    /// Reference of the image copied for `m`, relative to the export.
    fn rel(&self, m: &Mark) -> Option<String> {
        Some(format!("{}/{}", self.dir_name, self.file_name(m)?))
    }

    /// Screenshots of row `r` with the text of their column (without repetitions): one column each,
    /// as an Excel cell holds only one link.
    fn entries(&self, r: &RowRef) -> Vec<(String, &Mark)> {
        let mut v: Vec<(String, &Mark)> = Vec::new();
        for m in self.marks.get(r).into_iter().flatten() {
            let text = if self.embed { self.shots.get(m.shot).map(|s| format!("{} (#{})", s.label(), m.no)) } else { self.rel(m) };
            if let Some(text) = text.filter(|t| !v.iter().any(|(x, _)| x == t)) {
                v.push((text, m));
            }
        }
        v
    }

    /// Number of screenshot columns: the most screenshots of one row.
    fn columns(&self) -> usize {
        self.marks.keys().map(|r| self.entries(r).len()).max().unwrap_or(0)
    }

    /// Copies the images to [`Self::dir`]: per screenshot the image itself if a row shows it
    /// without area, and one framed copy per marked area.
    fn copy_images(&self) -> Result<()> {
        let from = self.store.image_dir();
        for (s, marks) in self.shots.iter().zip(&self.by_shot) {
            let path = from.join(&s.file);
            let bytes = read_file(&path)?;
            let mut done = HashSet::new();
            for m in marks.iter().filter(|m| done.insert(m.area)) {
                let Some(name) = self.file_name(m) else {
                    continue;
                };
                match m.area {
                    None => write_file(&self.dir.join(name), &bytes)?,
                    Some(a) => write_file(&self.dir.join(name), &draw_frame(&bytes, a, &path)?)?,
                }
            }
        }
        Ok(())
    }

    /// Rows and scaling of the images on the screenshot worksheet.
    fn layout(&self) -> Result<Vec<SheetImage>> {
        let from = self.store.image_dir();
        let mut out = Vec::new();
        let mut row = 0u32;
        for (s, marks) in self.shots.iter().zip(&self.by_shot) {
            let image = rust_xlsxwriter::Image::new_from_buffer(&read_file(&from.join(&s.file))?).map_err(xe)?;
            let dpi = |d: f64| if d > 0.0 { 96.0 / d } else { 1.0 };
            let (dx, dy) = (dpi(image.width_dpi()), dpi(image.height_dpi()));
            let scale = (SHEET_IMAGE_WIDTH / (image.width() * dx).max(1.0)).min(1.0);
            let image = image.set_scale_width(scale).set_scale_height(scale);
            let place = SheetImage { title_row: row, image, fx: dx * scale, fy: dy * scale };
            // title, image, table header, one row per mark, blank row
            row += 1 + image_rows(&place) + 1 + u32::try_from(marks.len()).unwrap_or(u32::MAX / 2) + 1;
            out.push(place);
        }
        Ok(out)
    }

    /// XLIFF `<context-group>`s of row `r`.
    fn xliff(&self, r: &RowRef) -> String {
        let mut s = String::new();
        for m in self.marks.get(r).into_iter().flatten() {
            let Some(rel) = self.rel(m) else {
                continue;
            };
            s.push_str("        <context-group purpose=\"information\">\n");
            s.push_str(&format!("          <context context-type=\"x-screenshot\">{}</context>\n", escape_text(&rel)));
            if let Some(a) = m.area {
                s.push_str(&format!("          <context context-type=\"x-screenshot-area\">{a}</context>\n"));
            }
            s.push_str("        </context-group>\n");
        }
        s
    }
}

/// Colour of the frames around marked areas (Excel shapes and framed image copies).
const FRAME_RGB: [u8; 3] = [0xE0, 0x20, 0x20];
/// Line width of a frame drawn into an image copy, in pixels.
const FRAME_WIDTH: u32 = 3;

/// `bytes` (PNG or JPEG, read from `path`) with a frame around `a` drawn in, in the same format.
/// The frame lies outside the area where the image leaves room, otherwise along its inner edge.
fn draw_frame(bytes: &[u8], a: Area, path: &Path) -> Result<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, Pixel, Rgb, Rgba};
    let format = match crate::screenshots::image_kind(bytes) {
        Some("png") => ImageFormat::Png,
        Some(_) => ImageFormat::Jpeg,
        None => return Err(Error::Unsupported(crate::tf!("image format (PNG or JPEG expected): {path}", path = path.display()))),
    };
    let broken = |e: image::ImageError| Error::Format(format!("{}: {e}", path.display()));
    let decoded = image::load_from_memory_with_format(bytes, format).map_err(broken)?;
    fn frame<P: Pixel<Subpixel = u8>>(img: &mut image::ImageBuffer<P, Vec<u8>>, a: Area, color: P) {
        let (w, h) = img.dimensions();
        let (x0, y0) = (a.x.saturating_sub(FRAME_WIDTH), a.y.saturating_sub(FRAME_WIDTH));
        let x1 = a.x.saturating_add(a.w).saturating_add(FRAME_WIDTH).min(w);
        let y1 = a.y.saturating_add(a.h).saturating_add(FRAME_WIDTH).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                if x < x0 + FRAME_WIDTH || x + FRAME_WIDTH >= x1 || y < y0 + FRAME_WIDTH || y + FRAME_WIDTH >= y1 {
                    img.put_pixel(x, y, color);
                }
            }
        }
    }
    // Colour images in any case, so that the frame is red on grey screenshots too.
    let img = if format == ImageFormat::Png && decoded.color().has_alpha() {
        let mut img = decoded.into_rgba8();
        let [r, g, b] = FRAME_RGB;
        frame(&mut img, a, Rgba([r, g, b, 0xFF]));
        DynamicImage::ImageRgba8(img)
    } else {
        let mut img = decoded.into_rgb8();
        frame(&mut img, a, Rgb(FRAME_RGB));
        DynamicImage::ImageRgb8(img)
    };
    let mut out = std::io::Cursor::new(Vec::new());
    match format {
        // Better than the default quality (75): screenshots contain text.
        ImageFormat::Jpeg => img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)),
        _ => img.write_to(&mut out, format),
    }
    .map_err(broken)?;
    Ok(out.into_inner())
}

/// Widest screenshot on the screenshot worksheet, in pixels.
const SHEET_IMAGE_WIDTH: f64 = 900.0;
/// Default row height of a worksheet in pixels.
const SHEET_ROW_HEIGHT: f64 = 20.0;

/// Position of one screenshot on the screenshot worksheet.
struct SheetImage {
    title_row: u32,
    image: rust_xlsxwriter::Image,
    /// Image pixels → displayed pixels (scaling and DPI).
    fx: f64,
    fy: f64,
}

impl Workbook<'_> {
    /// Per screenshot: title, image with a numbered frame per marked area, then a table
    /// `No. | Context | Original` of the rows shown.
    fn write_sheet(&self, ws: &mut rust_xlsxwriter::Worksheet, layout: &[SheetImage], ui: Lang) -> Result<()> {
        use rust_xlsxwriter::{Color, Format, Shape, ShapeFont, ShapeFormat, ShapeLine};
        let [r, g, b] = FRAME_RGB;
        let red = Color::RGB(u32::from_be_bytes([0, r, g, b]));
        let frame = ShapeFormat::new().set_no_fill().set_line(&ShapeLine::new().set_color(red).set_width(2.0));
        let font = ShapeFont::new().set_bold().set_color(red).set_size(9);
        let (head, bold, wrap) = (header_format(), Format::new().set_bold().set_font_size(12), Format::new().set_text_wrap());
        ws.set_column_width(0, 8).map_err(xe)?;
        ws.set_column_width(1, 40).map_err(xe)?;
        ws.set_column_width(2, 60).map_err(xe)?;
        for (k, (shot, place)) in self.plan.shots.iter().zip(layout).enumerate() {
            let title = if shot.title.is_empty() { shot.id.clone() } else { format!("{} ({})", shot.title, shot.id) };
            ws.write_string_with_format(place.title_row, 0, title, &bold).map_err(xe)?;
            let top = place.title_row + 1;
            ws.insert_image(top, 0, &place.image).map_err(xe)?;
            let marks = &self.plan.by_shot[k];
            for m in marks {
                let Some(a) = m.area else {
                    continue;
                };
                let px = |v: u32, f: f64| (f64::from(v) * f).round() as u32;
                let shape = Shape::textbox()
                    .set_text(m.no.to_string())
                    .set_width(px(a.w, place.fx).max(6))
                    .set_height(px(a.h, place.fy).max(6))
                    .set_format(&frame)
                    .set_font(&font);
                ws.insert_shape_with_offset(top, 0, &shape, px(a.x, place.fx), px(a.y, place.fy)).map_err(xe)?;
            }
            let mut r = top + image_rows(place);
            for (c, h) in [tr(ui, "No."), tr(ui, "Context"), "Original"].into_iter().enumerate() {
                ws.write_string_with_format(r, c as u16, h, &head).map_err(xe)?;
            }
            for m in marks {
                r += 1;
                let original = self.p.row(&m.row).map(|x| x.original().to_string()).unwrap_or_default();
                ws.write_number(r, 0, m.no as f64).map_err(xe)?;
                ws.write_string(r, 1, self.p.context(&m.row)).map_err(xe)?;
                ws.write_string_with_format(r, 2, original, &wrap).map_err(xe)?;
            }
        }
        Ok(())
    }
}

fn image_rows(place: &SheetImage) -> u32 {
    let h = place.image.height() * place.fy;
    (h / SHEET_ROW_HEIGHT).ceil() as u32 + 1
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
    xliff_of(p, lang, opts, None)
}

fn xliff_of(p: &Project, lang: &str, opts: &ExportOptions, shots: Option<&ShotPlan<'_>>) -> String {
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
            if let Some(shots) = shots {
                s.push_str(&shots.xliff(r));
            }
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
                if !importable(row) {
                    rep.not_translatable += 1;
                    continue;
                }
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

/// Exports to `path` (format by extension). With `shots`, the screenshots of the exported rows are
/// included as set by [`ExportOptions::screenshots`]. Returns the written files (and the folder of
/// copied screenshots).
pub fn export_file(p: &Project, path: &Path, opts: &ExportOptions, shots: Option<&ScreenshotStore>) -> Result<Vec<PathBuf>> {
    let format = format_of(path)?;
    let rows = if format == ExchangeFormat::Tmx { Vec::new() } else { export_rows(p, opts) };
    let plan = shots.zip(opts.screenshots.mode(format)).and_then(|(st, embed)| ShotPlan::new(p, st, &rows, embed, path));
    let mut out = match format {
        ExchangeFormat::Excel => {
            let book = plan.as_ref().map(|plan| Workbook { plan, rows: &rows, p });
            write_workbook(&table_of(p, opts, &rows, plan.as_ref()), path, opts.label_lang(), book.as_ref())?;
            vec![path.to_path_buf()]
        }
        ExchangeFormat::Csv | ExchangeFormat::Text => {
            let delim = if format == ExchangeFormat::Csv { ';' } else { '\t' };
            write_file(path, &write_delimited(&table_of(p, opts, &rows, plan.as_ref()), delim))?;
            vec![path.to_path_buf()]
        }
        ExchangeFormat::Xliff => {
            let langs = target_langs(p, &opts.langs);
            let mut out = Vec::new();
            for l in &langs {
                let target = if langs.len() == 1 { path.to_path_buf() } else { per_language_path(path, l) };
                write_file(&target, xliff_of(p, l, opts, plan.as_ref()).as_bytes())?;
                out.push(target);
            }
            out
        }
        ExchangeFormat::Tmx => {
            let mut copy = p.clone();
            let mut tm = crate::tm::TranslationMemory::new();
            let all: Vec<TranslationStatus> = TranslationStatus::ALL.into_iter().filter(|s| *s != TranslationStatus::NotTranslated).collect();
            let statuses = if opts.statuses.is_empty() { all } else { opts.statuses.clone() };
            crate::tm::save_to_tm(&mut copy, &mut tm, &opts.langs, &statuses, false);
            tm.save_as(path)?;
            vec![path.to_path_buf()]
        }
    };
    if let Some(plan) = plan.as_ref().filter(|p| !p.embed) {
        plan.copy_images()?;
        out.push(plan.dir.clone());
    }
    Ok(out)
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

    /// Without the "Original" column the rows are found by context, and a changed original goes
    /// unnoticed. In Excel, the frozen columns end after the context.
    #[test]
    fn table_without_original_column() {
        let p = project();
        let with = export_table(&p, &ExportOptions::default());
        let mut t = export_table(&p, &ExportOptions { original_column: false, ..Default::default() });
        assert_eq!(t.headers[..3], ["Source", "Context", "German [de]"]);
        assert_eq!(t.rows[0][..3], ["a.json", "hello", "Hallo"]);
        assert_eq!((key_columns(&with), key_columns(&t)), (3, 2));
        let fr = t.headers.iter().position(|h| h == "French [fr]").unwrap();
        t.rows[1][fr] = "Ligne".into();
        let mut q = project();
        let r = q.find_row("multi").unwrap();
        q.row_mut(&r).unwrap().native = Some("Changed since the export".into());
        let rep = import_table(&mut q, &t, &ImportOptions::default()).unwrap();
        assert_eq!((rep.updated, rep.original_changed, rep.not_found), (1, 0, 0), "{rep:?}");
        assert_eq!(q.row(&r).unwrap().text("fr"), Some("Ligne"));
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

    /// As the translation memory, imports leave "Do not translate" and excluded rows unchanged.
    #[test]
    fn import_skips_rows_that_are_not_translated() {
        let p = project();
        let mut t = export_table(&p, &ExportOptions::default());
        let col = |h: &str| t.headers.iter().position(|x| x == h).unwrap();
        let (fr, comment, note) = (col("French [fr]"), col("French [fr] Comment"), col("Row comment"));
        for rec in &mut t.rows {
            (rec[fr], rec[comment], rec[note]) = ("Texte".into(), "Frage".into(), "Notiz".into());
        }
        let x = export_xliff(&p, "fr", &ExportOptions::default())
            .replace("<target state=\"needs-translation\"></target>", "<target state=\"final\">Texte</target>\n        <note from=\"translator\">Frage</note>");
        let marked = || {
            let mut q = project();
            let (hello, multi) = (q.find_row("hello").unwrap(), q.find_row("multi").unwrap());
            q.row_mut(&hello).unwrap().set_locked(true);
            q.row_mut(&multi).unwrap().set_excluded(true);
            q
        };
        let mut q = marked();
        let rep = import_table(&mut q, &t, &ImportOptions::default()).unwrap();
        assert_eq!((rep.updated, rep.not_translatable), (0, 2), "{rep:?}");
        assert!(rep.summary().ends_with("; 2 rows skipped (\"Do not translate\" or excluded)"), "{}", rep.summary());
        let rep = import_xliff(&mut q, x.as_bytes(), &ImportOptions::default()).unwrap();
        assert_eq!((rep.updated, rep.not_translatable), (0, 2), "{rep:?}");
        for id in ["hello", "multi"] {
            let row = q.row(&q.find_row(id).unwrap()).unwrap();
            assert_eq!((row.translation("fr").is_none(), row.note()), (true, ""), "{id}");
        }
        // Without the marks, the same files are imported.
        for xliff in [false, true] {
            let mut q = project();
            let opts = ImportOptions::default();
            let rep = if xliff { import_xliff(&mut q, x.as_bytes(), &opts) } else { import_table(&mut q, &t, &opts) }.unwrap();
            assert_eq!((rep.updated, rep.not_translatable), (2, 0), "xliff {xliff}: {rep:?}");
            assert!(!rep.summary().contains("excluded"), "{}", rep.summary());
            let row = q.row(&q.find_row("hello").unwrap()).unwrap();
            assert_eq!((row.text("fr"), row.translation("fr").map(|t| t.comment())), (Some("Texte"), Some("Frage")));
        }
    }

    #[test]
    fn xlsx_roundtrip() {
        let p = project();
        let t = export_table(&p, &ExportOptions::default());
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.xlsx");
        write_xlsx(&t, &f, Lang::EN).unwrap();
        let back = read_xlsx(&f).unwrap();
        assert_eq!(back.headers, t.headers);
        assert_eq!(back.rows[0][3], "Hallo");
        assert_eq!(back.rows[1][2].replace("\r\n", "\n"), "Line 1\nLine \"2\"; x");
    }

    #[test]
    fn labels_in_another_ui_language() {
        use calamine::Reader;
        crate::i18n::set_current(Lang::DE);
        let p = project();
        let opts = ExportOptions { row_status_column: true, ..Default::default() };
        let de = export_table(&p, &opts);
        assert_eq!(de.headers[..6], ["Quelle", "Kontext", "Original [en]", "Deutsch [de]", "Deutsch [de] Status", "Deutsch [de] Kommentar"]);
        assert_eq!(de.rows[0][4], "Übersetzt");
        let en = export_table(&p, &ExportOptions { label_lang: Some(Lang::EN), ..opts.clone() });
        assert_eq!(en.headers[..6], ["Source", "Context", "Original [en]", "German [de]", "German [de] Status", "German [de] Comment"]);
        let row_status = p.row(&p.find_row("hello").unwrap()).unwrap().status();
        assert_eq!((en.rows[0][4].as_str(), en.headers[9].as_str(), en.rows[0][9].as_str()), ("Translated", "Row status", row_status.label_in(Lang::EN)));
        assert_ne!(row_status.label_in(Lang::EN), row_status.label_in(Lang::DE));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("en.xlsx");
        export_file(&p, &f, &ExportOptions { label_lang: Some(Lang::EN), ..opts.clone() }, None).unwrap();
        assert_eq!(calamine::open_workbook_auto(&f).unwrap().sheet_names(), ["Translation"]);
        assert_eq!(read_xlsx(&f).unwrap().headers, en.headers);

        // The import recognizes the headers and status values of any UI language.
        crate::i18n::set_current(Lang::EN);
        let mut de = de;
        let col = |h: &str| de.headers.iter().position(|x| x == h).unwrap();
        let (text, status, comment) = (col("Französisch [fr]"), col("Französisch [fr] Status"), col("Französisch [fr] Kommentar"));
        de.rows[1][text] = "Ligne".into();
        de.rows[1][status] = TranslationStatus::Complete.label_in(Lang::DE).into();
        de.rows[1][comment] = "Bitte prüfen".into();
        de.rows[1][col("Zeilenkommentar")] = "Notiz".into();
        let mut q = project();
        let rep = import_table(&mut q, &de, &ImportOptions::default()).unwrap();
        assert_eq!((rep.updated, rep.not_found), (1, 0), "{rep:?}");
        let row = q.row(&q.find_row("multi").unwrap()).unwrap();
        assert_eq!((row.text("fr"), row.tstatus("fr"), row.note()), (Some("Ligne"), TranslationStatus::Complete, "Notiz"));
        assert_eq!(row.translation("fr").map(|t| t.comment()), Some("Bitte prüfen"));
    }

    /// Project with a screenshot linked to row `hello` (with area) and an orphaned link.
    fn with_screenshot(dir: &Path) -> (Project, ScreenshotStore) {
        use crate::screenshots::{Link, solid_png};
        let mut p = project();
        p.path = Some(dir.join("app.slp"));
        let mut st = ScreenshotStore::new(p.path.as_ref().unwrap());
        st.add_bytes(&solid_png(200, 100, [200, 200, 200]), Some("main"), Some("Main window")).unwrap();
        let hello = p.find_row("hello").unwrap();
        st.link("main", [Link::to_row(&p, &hello, Area::parse("10,10,50,20")), Link { source: None, key: "gone".into(), area: None }]).unwrap();
        (p, st)
    }

    fn zip_entry(file: &Path, name: &str) -> String {
        let mut z = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
        s
    }

    #[test]
    fn xlsx_embeds_screenshots_on_a_second_sheet() {
        use calamine::Reader;
        let dir = tempfile::tempdir().unwrap();
        let (p, st) = with_screenshot(dir.path());
        let f = dir.path().join("x.xlsx");
        let opts = ExportOptions { label_lang: Some(Lang::EN), ..Default::default() };
        assert_eq!(export_file(&p, &f, &opts, Some(&st)).unwrap(), std::slice::from_ref(&f), "embedded: no image folder");
        let mut wb = calamine::open_workbook_auto(&f).unwrap();
        assert_eq!(wb.sheet_names(), ["Translation", "Screenshots"]);
        let t = read_xlsx(&f).unwrap();
        assert_eq!(t.headers.last().map(String::as_str), Some("Screenshot"));
        assert_eq!((t.rows[0].last().unwrap().as_str(), t.rows[1].last().unwrap().as_str()), ("Main window (#1)", ""));
        let sheet = wb.worksheet_range("Screenshots").unwrap();
        let cells: Vec<String> = sheet.cells().map(|(_, _, c)| c.to_string()).collect();
        for want in ["Main window (main)", "No.", "Context", "Original", "1", "hello", "Hello"] {
            assert!(cells.iter().any(|c| c == want), "{want} missing in {cells:?}");
        }
        assert!(zip_entry(&f, "xl/worksheets/sheet1.xml").contains("location=\"'Screenshots'!A1\""));
        let drawing = zip_entry(&f, "xl/drawings/drawing1.xml");
        assert!(drawing.contains("<xdr:pic>") && drawing.contains("<xdr:sp ") && drawing.contains("<a:t>1</a:t>"), "{drawing}");

        // Re-import ignores the screenshot column and the second sheet.
        let mut q = project();
        let rep = import_file(&mut q, &f, &ImportOptions::default()).unwrap();
        assert_eq!((rep.not_found, rep.unknown_languages.len()), (0, 0), "{rep:?}");
        // Without a store or with `None` there is no screenshot column.
        let plain = ExportOptions { screenshots: ScreenshotExport::None, ..opts.clone() };
        export_file(&p, &f, &plain, Some(&st)).unwrap();
        assert!(!read_xlsx(&f).unwrap().headers.contains(&"Screenshot".to_string()));
        assert_eq!(calamine::open_workbook_auto(&f).unwrap().sheet_names().len(), 1);
    }

    /// An Excel cell holds only one link: each screenshot of a row gets a column of its own.
    #[test]
    fn several_screenshots_of_a_row_in_columns_of_their_own() {
        use crate::screenshots::{Link, solid_png};
        let dir = tempfile::tempdir().unwrap();
        let (p, mut st) = with_screenshot(dir.path());
        st.add_bytes(&solid_png(40, 30, [0, 0, 255]), Some("dialog"), Some("Dialog")).unwrap();
        let hello = p.find_row("hello").unwrap();
        st.link("dialog", [Link::to_row(&p, &hello, None)]).unwrap();
        let f = dir.path().join("x.xlsx");
        let main = st.get("main").unwrap().file.replace(".png", ".10-10-50-20.png");
        let dialog = st.get("dialog").unwrap().file.clone();
        // In the order of the store (by id).
        for (mode, want) in [
            (ScreenshotExport::Embed, ["Dialog (#1)".to_string(), "Main window (#1)".to_string()]),
            (ScreenshotExport::Link, [format!("x_screenshots/{dialog}"), format!("x_screenshots/{main}")]),
        ] {
            let opts = ExportOptions { label_lang: Some(Lang::EN), screenshots: mode, ..Default::default() };
            export_file(&p, &f, &opts, Some(&st)).unwrap();
            let t = read_xlsx(&f).unwrap();
            let n = t.headers.len();
            assert_eq!(t.headers[n - 2..], ["Screenshot", "Screenshot 2"], "{mode:?}");
            assert_eq!(t.rows[0][n - 2..], want, "{mode:?}");
            assert_eq!(t.rows[1][n - 2..], ["", ""], "{mode:?}");
            let sheet = zip_entry(&f, "xl/worksheets/sheet1.xml");
            assert_eq!(sheet.matches("<hyperlink ").count(), 2, "{mode:?}: {sheet}");
            let mut q = project();
            let rep = import_file(&mut q, &f, &ImportOptions::default()).unwrap();
            assert_eq!((rep.not_found, rep.unknown_languages.len()), (0, 0), "{mode:?}: {rep:?}");
        }
        assert_eq!(classify("Screenshot 12"), Col::Ignore);
    }

    #[test]
    fn csv_and_xliff_reference_copied_screenshots() {
        let dir = tempfile::tempdir().unwrap();
        let (p, st) = with_screenshot(dir.path());
        // The area of the link is framed in a copy of the image.
        let file = st.get("main").unwrap().file.replace(".png", ".10-10-50-20.png");
        let opts = ExportOptions { label_lang: Some(Lang::EN), langs: vec!["de".into()], ..Default::default() };

        let csv = dir.path().join("out").join("x.csv");
        let files = export_file(&p, &csv, &opts, Some(&st)).unwrap();
        let copies = dir.path().join("out").join("x_screenshots");
        assert_eq!(files, [csv.clone(), copies.clone()]);
        assert_eq!(std::fs::read_dir(&copies).unwrap().count(), 1, "no unframed copy without a row showing it");
        assert!(copies.join(&file).is_file());
        let t = read_delimited(&read_file(&csv).unwrap(), None).unwrap();
        assert_eq!(t.rows[0].last().unwrap(), &format!("x_screenshots/{file}"));
        let mut q = project();
        assert_eq!(import_file(&mut q, &csv, &ImportOptions::default()).unwrap().not_found, 0);

        let xlf = dir.path().join("y.xlf");
        export_file(&p, &xlf, &opts, Some(&st)).unwrap();
        let x = String::from_utf8(read_file(&xlf).unwrap()).unwrap();
        assert!(x.contains(&format!("<context context-type=\"x-screenshot\">y_screenshots/{file}</context>")), "{x}");
        assert!(x.contains("<context context-type=\"x-screenshot-area\">10,10,50,20</context>"));
        assert!(dir.path().join("y_screenshots").join(&file).is_file());
        let mut q = project();
        let rep = import_file(&mut q, &xlf, &ImportOptions::default()).unwrap();
        assert_eq!(rep.not_found, 0, "{rep:?}");
        assert_eq!(classify("Screenshot"), Col::Ignore);
    }

    /// Copied images: one framed copy per marked area (shared by links with the same area), the
    /// image itself for a link without area; PNG and JPEG keep their format.
    #[test]
    fn copied_screenshots_have_the_frames_drawn_in() {
        use crate::screenshots::Link;
        let dir = tempfile::tempdir().unwrap();
        let (mut p, mut st) = with_screenshot(dir.path());
        let mut s = Source::new(SourceKind::Json, "b.json", "en");
        for id in ["same", "other", "plain", "photo"] {
            s.items.push(Item::Row(Row::new(id, id)));
        }
        p.sources.push(s);
        let row = |p: &Project, id: &str| p.find_row(id).unwrap();
        let area = |s: &str| Area::parse(s);
        st.link(
            "main",
            [
                Link::to_row(&p, &row(&p, "same"), area("10,10,50,20")),
                Link::to_row(&p, &row(&p, "other"), area("0,60,200,40")),
                Link::to_row(&p, &row(&p, "plain"), None),
            ],
        )
        .unwrap();
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(80, 60, image::Rgb([0, 0, 255]))).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap();
        st.add_bytes(jpeg.get_ref(), Some("photo"), None).unwrap();
        st.link("photo", [Link::to_row(&p, &row(&p, "photo"), area("20,20,20,20"))]).unwrap();

        let csv = dir.path().join("x.csv");
        let opts = ExportOptions { label_lang: Some(Lang::EN), langs: vec!["de".into()], ..Default::default() };
        export_file(&p, &csv, &opts, Some(&st)).unwrap();
        let copies = dir.path().join("x_screenshots");
        let (main, photo) = (st.get("main").unwrap().file.clone(), st.get("photo").unwrap().file.clone());
        let named = |file: &str, a: &str| file.replace(".png", &format!(".{a}.png")).replace(".jpg", &format!(".{a}.jpg"));
        let t = read_delimited(&read_file(&csv).unwrap(), None).unwrap();
        let cells: Vec<&str> = t.rows.iter().map(|r| r.last().unwrap().as_str()).collect();
        let want =
            [named(&main, "10-10-50-20"), String::new(), named(&main, "10-10-50-20"), named(&main, "0-60-200-40"), main.clone(), named(&photo, "20-20-20-20")];
        assert_eq!(cells, want.iter().map(|f| if f.is_empty() { String::new() } else { format!("x_screenshots/{f}") }).collect::<Vec<_>>());
        let mut written: Vec<String> = std::fs::read_dir(&copies).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        written.sort();
        let mut expected: Vec<String> = want.into_iter().filter(|f| !f.is_empty()).collect();
        expected.sort();
        expected.dedup();
        assert_eq!(written, expected);

        let load = |f: &str| image::open(copies.join(f)).unwrap().to_rgb8();
        let (red, gray) = (image::Rgb(FRAME_RGB), image::Rgb([200, 200, 200]));
        // The unframed image is the original file.
        assert_eq!(read_file(&copies.join(&main)).unwrap(), read_file(&st.image_dir().join(&main)).unwrap());
        // Frame outside the area, the area itself unchanged.
        let img = load(&named(&main, "10-10-50-20"));
        assert_eq!((img.get_pixel(7, 7), img.get_pixel(62, 32), img.get_pixel(10, 10), img.get_pixel(6, 6)), (&red, &red, &gray, &gray));
        // At the edges of the image, the frame lies inside the area.
        let img = load(&named(&main, "0-60-200-40"));
        assert_eq!((img.get_pixel(0, 99), img.get_pixel(199, 80), img.get_pixel(100, 57), img.get_pixel(3, 63)), (&red, &red, &red, &gray));
        // JPEG stays JPEG (lossy: the colour only roughly).
        let jpg = read_file(&copies.join(named(&photo, "20-20-20-20"))).unwrap();
        assert_eq!(crate::screenshots::image_kind(&jpg), Some("jpg"));
        let px = image::load_from_memory(&jpg).unwrap().to_rgb8().get_pixel(18, 18).0;
        assert!(px[0] > 150 && px[2] < 100, "{px:?}");
    }
}
