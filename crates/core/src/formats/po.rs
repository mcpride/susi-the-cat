//! GNU gettext PO/POT files (Sisulizer `TPoSource`).
//!
//! The original is a `.pot` (or a `.po` when no template exists); building writes a localized
//! `.po` per language and optionally compiles it to `.mo`.
//! Source options are stored as `<source>` attributes `scancomments`, `compile`,
//! `usevalueasoriginal`, `includeempty`, `byteorder` (`boLittleEndian`/`boBigEndian`),
//! `programminglanguage` and `pluralforms` (the dialog defaults to 3 rows per plural message).
//! The header fields `Last-Translator` and `Language-Team` are translatable; `Content-Type` and
//! `Content-Transfer-Encoding` are written by the tool.
//!
//! Susi design: the row id is the `msgid`, prefixed by
//! `msgctxt|` when a context exists; plural messages get one row per plural form with the suffix
//! `[n]` (form 0 shows `msgid`, the others `msgid_plural`). Header fields use the ids
//! `#Last-Translator` and `#Language-Team`.

use super::{ScannedString, TextStyle};
use crate::error::{Error, Result};
use crate::xml::Attrs;

/// Translatable header fields and the placeholders written when the original lacks them.
pub const HEADER_FIELDS: [(&str, &str); 2] = [("Last-Translator", "FULL NAME <EMAIL@ADDRESS>"), ("Language-Team", "LANGUAGE <LL@li.org>")];

/// PO source options ("PO Source | Options").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoOptions {
    /// Read the comments (`#.`, `# `, `#:`) into the row comments.
    pub scan_comments: bool,
    /// Compile the localized PO files to binary MO files.
    pub compile: bool,
    /// Use the `msgstr` value as original instead of the `msgid`.
    pub msgstr_as_original: bool,
    /// Write items with empty translations into MO files.
    pub include_empty: bool,
    /// Byte order of the MO files.
    pub big_endian: bool,
    /// Rows per plural message (1..=6).
    pub plural_rows: usize,
}

impl Default for PoOptions {
    fn default() -> Self {
        Self { scan_comments: true, compile: true, msgstr_as_original: false, include_empty: false, big_endian: false, plural_rows: 3 }
    }
}

impl PoOptions {
    pub const MAX_PLURAL_ROWS: usize = 6;

    pub fn from_attrs(a: &Attrs) -> Self {
        let d = Self::default();
        let flag = |name: &str, def: bool| a.get(name).map_or(def, |v| v == "1" || v.eq_ignore_ascii_case("true"));
        Self {
            scan_comments: flag("scancomments", d.scan_comments),
            compile: flag("compile", d.compile),
            msgstr_as_original: flag("usevalueasoriginal", d.msgstr_as_original),
            include_empty: flag("includeempty", d.include_empty),
            big_endian: a.get("byteorder").is_some_and(|v| v == "boBigEndian"),
            plural_rows: a.get("pluralforms").and_then(|v| v.parse().ok()).filter(|n| (1..=Self::MAX_PLURAL_ROWS).contains(n)).unwrap_or(d.plural_rows),
        }
    }

    pub fn store(&self, a: &mut Attrs) {
        let b = |v: bool| if v { "1" } else { "0" };
        a.set("scancomments", b(self.scan_comments));
        a.set("compile", b(self.compile));
        a.set("usevalueasoriginal", b(self.msgstr_as_original));
        a.set("includeempty", b(self.include_empty));
        a.set("byteorder", if self.big_endian { "boBigEndian" } else { "boLittleEndian" });
        a.set("pluralforms", self.plural_rows.clamp(1, Self::MAX_PLURAL_ROWS).to_string());
    }
}

/// One message of a PO file. String fields keep their quoted segments to preserve line wrapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// Comment lines (`#...` except obsolete `#~`), verbatim.
    pub comments: Vec<String>,
    pub msgctxt: Option<Vec<String>>,
    pub msgid: Option<Vec<String>>,
    pub msgid_plural: Option<Vec<String>>,
    /// `(index, segments)`; the index is `None` for a plain `msgstr`.
    pub msgstr: Vec<(Option<usize>, Vec<String>)>,
    /// Obsolete (`#~`) lines, verbatim. An entry with only these (or only comments) is kept as is.
    pub obsolete: Vec<String>,
}

fn joined(segs: &[String]) -> String {
    segs.concat()
}

impl Entry {
    pub fn context(&self) -> Option<String> {
        self.msgctxt.as_deref().map(joined)
    }

    pub fn id(&self) -> Option<String> {
        self.msgid.as_deref().map(joined)
    }

    pub fn plural(&self) -> Option<String> {
        self.msgid_plural.as_deref().map(joined)
    }

    /// `msgstr` (index 0 for plural messages) or form `i`.
    pub fn msgstr(&self, i: usize) -> String {
        self.msgstr.iter().find(|(idx, _)| idx.unwrap_or(0) == i).map(|(_, s)| joined(s)).unwrap_or_default()
    }

    pub fn is_header(&self) -> bool {
        self.msgctxt.is_none() && self.msgid.as_ref().is_some_and(|s| joined(s).is_empty())
    }

    pub fn is_message(&self) -> bool {
        self.msgid.is_some()
    }

    pub fn flags(&self) -> Vec<String> {
        self.comments
            .iter()
            .filter_map(|c| c.strip_prefix("#,"))
            .flat_map(|f| f.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>())
            .collect()
    }

    pub fn is_fuzzy(&self) -> bool {
        self.flags().iter().any(|f| f == "fuzzy")
    }

    /// Developer-facing comment text: extracted (`#.`), translator (`# `) and reference (`#:`) lines.
    fn comment_text(&self) -> Option<String> {
        let lines: Vec<&str> = self
            .comments
            .iter()
            .filter_map(|c| {
                if let Some(r) = c.strip_prefix("#.").or_else(|| c.strip_prefix("#:")) {
                    Some(r.trim())
                } else if c.starts_with("#,") || c.starts_with("#|") {
                    None
                } else {
                    c.strip_prefix('#').map(str::trim)
                }
            })
            .filter(|l| !l.is_empty())
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// Base row id: `msgid` or `msgctxt|msgid`.
    pub fn row_id(&self) -> String {
        let id = self.id().unwrap_or_default();
        match self.context() {
            Some(c) => format!("{c}|{id}"),
            None => id,
        }
    }
}

/// Row id of plural form `i` of a message with base id `base`.
pub fn plural_row_id(base: &str, i: usize) -> String {
    format!("{base}[{i}]")
}

/// Row id of a translatable header field.
pub fn header_row_id(field: &str) -> String {
    format!("#{field}")
}

// ---------------------------------------------------------------------------------------------
// Parsing

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Ctxt,
    Id,
    Plural,
    Str(usize),
}

struct Parser {
    entries: Vec<Entry>,
    cur: Entry,
    field: Field,
}

impl Parser {
    fn finish(&mut self) {
        let e = std::mem::take(&mut self.cur);
        if e != Entry::default() {
            self.entries.push(e);
        }
        self.field = Field::None;
    }

    /// Starts a new entry when the current one already has message keywords.
    fn boundary(&mut self) {
        if self.cur.msgid.is_some() || !self.cur.obsolete.is_empty() {
            self.finish();
        }
    }

    fn append(&mut self, seg: String) -> bool {
        let target = match self.field {
            Field::None => return false,
            Field::Ctxt => self.cur.msgctxt.as_mut(),
            Field::Id => self.cur.msgid.as_mut(),
            Field::Plural => self.cur.msgid_plural.as_mut(),
            Field::Str(i) => self.cur.msgstr.get_mut(i).map(|(_, s)| s),
        };
        match target {
            Some(t) => {
                t.push(seg);
                true
            }
            None => false,
        }
    }

    fn line(&mut self, line: &str, no: usize) -> Result<()> {
        let l = line.trim();
        let err = |m: &str| Error::Format(format!("PO line {no}: {m}"));
        if l.is_empty() {
            self.finish();
        } else if l.starts_with("#~") {
            if self.cur.msgid.is_some() {
                self.finish();
            }
            self.cur.obsolete.push(line.to_string());
        } else if l.starts_with('#') {
            self.boundary();
            self.cur.comments.push(l.to_string());
        } else if l.starts_with('"') {
            let seg = unquote(l).ok_or_else(|| err("invalid string"))?;
            if !self.append(seg) {
                return Err(err("string without keyword"));
            }
        } else {
            self.keyword(l).map_err(|m| err(&m))?;
        }
        Ok(())
    }

    fn keyword(&mut self, l: &str) -> std::result::Result<(), String> {
        let (kw, rest) = l.split_once(|c: char| c.is_whitespace()).ok_or_else(|| format!("unexpected '{l}'"))?;
        let seg = unquote(rest.trim()).ok_or_else(|| "invalid string".to_string())?;
        match kw {
            "msgctxt" => {
                self.boundary();
                self.cur.msgctxt = Some(vec![seg]);
                self.field = Field::Ctxt;
            }
            "msgid" => {
                self.boundary();
                self.cur.msgid = Some(vec![seg]);
                self.field = Field::Id;
            }
            "msgid_plural" => {
                self.cur.msgid_plural = Some(vec![seg]);
                self.field = Field::Plural;
            }
            "msgstr" => self.push_str(None, seg),
            _ => {
                let idx = kw
                    .strip_prefix("msgstr[")
                    .and_then(|r| r.strip_suffix(']'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .ok_or_else(|| format!("unknown keyword '{kw}'"))?;
                self.push_str(Some(idx), seg);
            }
        }
        Ok(())
    }

    fn push_str(&mut self, idx: Option<usize>, seg: String) {
        self.cur.msgstr.push((idx, vec![seg]));
        self.field = Field::Str(self.cur.msgstr.len() - 1);
    }
}

/// Parses a PO/POT file.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    let (text, _) = crate::xml::decode_text(bytes)?;
    parse_str(&text)
}

pub fn parse_str(text: &str) -> Result<Vec<Entry>> {
    let mut p = Parser { entries: Vec::new(), cur: Entry::default(), field: Field::None };
    for (i, line) in text.lines().enumerate() {
        p.line(line, i + 1)?;
    }
    p.finish();
    Ok(p.entries)
}

/// Decodes a quoted C string (`"..."`).
fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            return None;
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            'x' => {
                let mut v = 0u32;
                while let Some(d) = chars.peek().and_then(|c| c.to_digit(16)) {
                    v = v.saturating_mul(16).saturating_add(d);
                    chars.next();
                }
                out.push(char::from_u32(v)?);
            }
            d @ '0'..='7' => {
                let mut v = d.to_digit(8)?;
                for _ in 0..2 {
                    match chars.peek().and_then(|c| c.to_digit(8)) {
                        Some(n) => {
                            v = v * 8 + n;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(char::from_u32(v)?);
            }
            other => out.push(other),
        }
    }
    Some(out)
}

/// Encodes a string as a quoted C string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------------------------
// Scanning

/// Header fields (`Key: value` lines of the header `msgstr`).
pub fn header_fields(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).filter(|(k, _)| !k.is_empty()).collect()
}

pub fn scan(bytes: &[u8], opts: &PoOptions) -> Result<Vec<ScannedString>> {
    let entries = parse(bytes)?;
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| e.is_message()) {
        if e.is_header() {
            let fields = header_fields(&e.msgstr(0));
            for (name, placeholder) in HEADER_FIELDS {
                let value = fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or(placeholder, |(_, v)| v.as_str());
                out.push(ScannedString { id: header_row_id(name), text: value.to_string(), comment: None });
            }
            continue;
        }
        let comment = if opts.scan_comments { e.comment_text() } else { None };
        let base = e.row_id();
        match e.plural() {
            None => {
                let text = original_text(e, 0, opts);
                out.push(ScannedString { id: base, text, comment });
            }
            Some(_) => {
                for i in 0..opts.plural_rows.max(1) {
                    out.push(ScannedString { id: plural_row_id(&base, i), text: original_text(e, i, opts), comment: comment.clone() });
                }
            }
        }
    }
    Ok(out)
}

/// Original text of (plural form `i` of) a message.
fn original_text(e: &Entry, i: usize, opts: &PoOptions) -> String {
    if opts.msgstr_as_original {
        let s = e.msgstr(i);
        if !s.is_empty() {
            return s;
        }
    }
    match (i, e.plural()) {
        (0, _) | (_, None) => e.id().unwrap_or_default(),
        (_, Some(p)) => p,
    }
}

// ---------------------------------------------------------------------------------------------
// Building

/// Inputs for building one localized PO file.
pub struct BuildParams<'a> {
    /// language id of the target language.
    pub lang: &'a str,
    pub options: PoOptions,
    /// `resolve(row_id, original)` returns the text to write, `None` for an empty `msgstr`.
    pub resolve: &'a dyn Fn(&str, &str) -> Option<String>,
    /// Whether the translation of a row is marked fuzzy (for review / best guess).
    pub fuzzy: &'a dyn Fn(&str) -> bool,
    /// `PO-Revision-Date` value; the original value is kept when `None`.
    pub revision_date: Option<String>,
}

/// Builds a localized PO file from the original PO/POT.
pub fn build(original: &[u8], params: &BuildParams<'_>) -> Result<Vec<u8>> {
    let style = TextStyle::detect(original);
    let entries = parse(original)?;
    let mut blocks = Vec::with_capacity(entries.len());
    for e in &entries {
        let mut lines = Vec::new();
        if !e.is_message() {
            lines.extend(e.comments.iter().cloned());
            lines.extend(e.obsolete.iter().cloned());
        } else if e.is_header() {
            write_header(e, params, &mut lines);
        } else {
            write_message(e, params, &mut lines);
        }
        blocks.push(lines.join("\n"));
    }
    let mut text = blocks.join("\n\n");
    text.push('\n');
    Ok(style.apply(&text))
}

/// Comment lines with the `#,` flags replaced by `flags` (fuzzy handled by the caller).
fn comments_with_flags(e: &Entry, fuzzy: bool) -> Vec<String> {
    let mut flags: Vec<String> = e.flags().into_iter().filter(|f| f != "fuzzy").collect();
    if fuzzy {
        flags.insert(0, "fuzzy".into());
    }
    let flag_line = (!flags.is_empty()).then(|| format!("#, {}", flags.join(", ")));
    let mut out = Vec::with_capacity(e.comments.len() + 1);
    let mut placed = false;
    for c in &e.comments {
        if c.starts_with("#,") {
            if let (false, Some(f)) = (placed, &flag_line) {
                out.push(f.clone());
            }
            placed = true;
        } else if c.starts_with("#|") && !placed {
            if let Some(f) = &flag_line {
                out.push(f.clone());
            }
            placed = true;
            out.push(c.clone());
        } else {
            out.push(c.clone());
        }
    }
    if let (false, Some(f)) = (placed, flag_line) {
        out.push(f);
    }
    out
}

fn push_segments(lines: &mut Vec<String>, keyword: &str, segs: &[String]) {
    match segs {
        [] => lines.push(format!("{keyword} \"\"")),
        [first, rest @ ..] => {
            lines.push(format!("{keyword} {}", quote(first)));
            lines.extend(rest.iter().map(|s| quote(s)));
        }
    }
}

/// Writes a value like gettext: multi-line values start with `""` and break after each `\n`.
fn push_value(lines: &mut Vec<String>, keyword: &str, text: &str) {
    let parts: Vec<&str> = text.split_inclusive('\n').collect();
    if parts.len() <= 1 {
        lines.push(format!("{keyword} {}", quote(text)));
    } else {
        lines.push(format!("{keyword} \"\""));
        lines.extend(parts.iter().map(|p| quote(p)));
    }
}

fn push_keys(e: &Entry, lines: &mut Vec<String>) {
    if let Some(c) = &e.msgctxt {
        push_segments(lines, "msgctxt", c);
    }
    push_segments(lines, "msgid", e.msgid.as_deref().unwrap_or_default());
    if let Some(p) = &e.msgid_plural {
        push_segments(lines, "msgid_plural", p);
    }
}

fn write_message(e: &Entry, params: &BuildParams<'_>, lines: &mut Vec<String>) {
    let base = e.row_id();
    let opts = &params.options;
    let (values, fuzzy) = match e.plural() {
        None => {
            let v = (params.resolve)(&base, &original_text(e, 0, opts));
            let fuzzy = v.is_some() && (params.fuzzy)(&base);
            (vec![v.unwrap_or_default()], fuzzy)
        }
        Some(_) => {
            let n = plural_forms(params.lang).0;
            let rows = opts.plural_rows.max(1);
            let ids: Vec<String> = (0..n).map(|i| plural_row_id(&base, i.min(rows - 1))).collect();
            let vals: Vec<Option<String>> = ids.iter().enumerate().map(|(i, id)| (params.resolve)(id, &original_text(e, i.min(rows - 1), opts))).collect();
            let fuzzy = vals.iter().zip(&ids).any(|(v, id)| v.is_some() && (params.fuzzy)(id));
            (vals.into_iter().map(Option::unwrap_or_default).collect(), fuzzy)
        }
    };
    lines.extend(comments_with_flags(e, fuzzy));
    push_keys(e, lines);
    if e.plural().is_some() {
        for (i, v) in values.iter().enumerate() {
            push_value(lines, &format!("msgstr[{i}]"), v);
        }
    } else {
        push_value(lines, "msgstr", values.first().map_or("", String::as_str));
    }
}

fn set_field(fields: &mut Vec<(String, String)>, name: &str, value: String) {
    match fields.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
        Some(f) => f.1 = value,
        None => fields.push((name.to_string(), value)),
    }
}

fn write_header(e: &Entry, params: &BuildParams<'_>, lines: &mut Vec<String>) {
    let mut fields = header_fields(&e.msgstr(0));
    if let Some(d) = &params.revision_date {
        set_field(&mut fields, "PO-Revision-Date", d.clone());
    }
    for (name, placeholder) in HEADER_FIELDS {
        let orig = fields.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or(placeholder.to_string(), |(_, v)| v.clone());
        let value = (params.resolve)(&header_row_id(name), &orig).unwrap_or(orig);
        set_field(&mut fields, name, value);
    }
    set_field(&mut fields, "Language", crate::lang::iso(params.lang));
    set_field(&mut fields, "Content-Type", "text/plain; charset=UTF-8".into());
    set_field(&mut fields, "Content-Transfer-Encoding", "8bit".into());
    let (n, expr) = plural_forms(params.lang);
    set_field(&mut fields, "Plural-Forms", format!("nplurals={n}; plural={expr};"));
    // The header of a template is fuzzy; a localized file must not be (msgfmt would skip it).
    lines.extend(comments_with_flags(e, false));
    push_keys(e, lines);
    lines.push("msgstr \"\"".into());
    lines.extend(fields.iter().map(|(k, v)| quote(&format!("{k}: {v}\n"))));
}

/// gettext plural forms (`nplurals`, `plural` expression) of a language.
pub fn plural_forms(lang: &str) -> (usize, &'static str) {
    let base = lang.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    let iso = crate::lang::iso(lang);
    match base.as_str() {
        "ja" | "ko" | "zh" | "vi" | "th" | "id" | "ms" | "lo" | "km" | "my" => (1, "0"),
        "fr" | "oc" | "fil" | "tl" | "ln" | "hy" => (2, "(n > 1)"),
        "pt" if iso.eq_ignore_ascii_case("pt_BR") => (2, "(n > 1)"),
        "is" => (2, "(n%10 != 1 || n%100 == 11)"),
        "mk" => (2, "(n%10 == 1 && n%100 != 11 ? 0 : 1)"),
        "ru" | "uk" | "be" | "sr" | "hr" | "bs" => (3, "(n%10 == 1 && n%100 != 11 ? 0 : n%10 >= 2 && n%10 <= 4 && (n%100 < 10 || n%100 >= 20) ? 1 : 2)"),
        "pl" => (3, "(n == 1 ? 0 : n%10 >= 2 && n%10 <= 4 && (n%100 < 10 || n%100 >= 20) ? 1 : 2)"),
        "cs" | "sk" => (3, "(n == 1 ? 0 : n >= 2 && n <= 4 ? 1 : 2)"),
        "lt" => (3, "(n%10 == 1 && n%100 != 11 ? 0 : n%10 >= 2 && (n%100 < 10 || n%100 >= 20) ? 1 : 2)"),
        "lv" => (3, "(n%10 == 1 && n%100 != 11 ? 0 : n != 0 ? 1 : 2)"),
        "ro" => (3, "(n == 1 ? 0 : (n == 0 || (n%100 > 0 && n%100 < 20)) ? 1 : 2)"),
        "sl" => (4, "(n%100 == 1 ? 0 : n%100 == 2 ? 1 : n%100 == 3 || n%100 == 4 ? 2 : 3)"),
        "cy" => (4, "(n == 1 ? 0 : n == 2 ? 1 : (n != 8 && n != 11) ? 2 : 3)"),
        "ga" => (5, "(n == 1 ? 0 : n == 2 ? 1 : n < 7 ? 2 : n < 11 ? 3 : 4)"),
        "ar" => (6, "(n == 0 ? 0 : n == 1 ? 1 : n == 2 ? 2 : n%100 >= 3 && n%100 <= 10 ? 3 : n%100 >= 11 ? 4 : 5)"),
        _ => (2, "(n != 1)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POT: &str = "# SOME DESCRIPTIVE TITLE.\r\n#, fuzzy\r\nmsgid \"\"\r\nmsgstr \"\"\r\n\"Project-Id-Version: demo 1.0\\n\"\r\n\"PO-Revision-Date: YEAR-MO-DA HO:MI+ZONE\\n\"\r\n\"Content-Type: text/plain; charset=CHARSET\\n\"\r\n\r\n#. Greeting on the start page\r\n#: src/main.c:10\r\n#, c-format\r\nmsgid \"Hello %s\"\r\nmsgstr \"\"\r\n\r\nmsgctxt \"menu\"\r\nmsgid \"Open\"\r\nmsgstr \"\"\r\n\r\nmsgid \"\"\r\n\"Line one\\n\"\r\n\"Line \\\"two\\\"\"\r\nmsgstr \"\"\r\n\r\nmsgid \"One file\"\r\nmsgid_plural \"%d files\"\r\nmsgstr[0] \"\"\r\nmsgstr[1] \"\"\r\n\r\n#~ msgid \"Old\"\r\n#~ msgstr \"Alt\"\r\n";

    fn ids(v: &[ScannedString]) -> Vec<&str> {
        v.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn scan_messages_context_plural_header() {
        let opts = PoOptions { plural_rows: 2, ..Default::default() };
        let s = scan(POT.as_bytes(), &opts).unwrap();
        assert_eq!(ids(&s), ["#Last-Translator", "#Language-Team", "Hello %s", "menu|Open", "Line one\nLine \"two\"", "One file[0]", "One file[1]"]);
        assert_eq!(s[0].text, "FULL NAME <EMAIL@ADDRESS>");
        assert_eq!(s[2].comment.as_deref(), Some("Greeting on the start page\nsrc/main.c:10"));
        assert_eq!(s[6].text, "%d files");
        let s = scan(POT.as_bytes(), &PoOptions { scan_comments: false, ..Default::default() }).unwrap();
        assert_eq!(s[2].comment, None);
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn msgstr_as_original() {
        let po = "msgid \"BTN_OK\"\nmsgstr \"OK\"\n\nmsgid \"BTN_X\"\nmsgstr \"\"\n";
        let s = scan(po.as_bytes(), &PoOptions { msgstr_as_original: true, ..Default::default() }).unwrap();
        assert_eq!(s[0].text, "OK");
        assert_eq!(s[1].text, "BTN_X");
    }

    #[test]
    fn escapes_roundtrip() {
        let raw = "a\"b\\c\n\td\u{1}é";
        assert_eq!(unquote(&quote(raw)).unwrap(), raw);
        assert_eq!(unquote("\"\\101\\x42\"").unwrap(), "AB");
        assert!(unquote("\"a\"b\"").is_none());
    }

    #[test]
    fn invalid_file_is_error() {
        assert!(parse_str("msgid \"a\"\nmsgstr \"b\"\nfoo\n").is_err());
        assert!(parse_str("\"orphan\"\n").is_err());
    }

    #[test]
    fn build_localized_po() {
        let resolve = |id: &str, _orig: &str| match id {
            "Hello %s" => Some("Hallo %s".to_string()),
            "menu|Open" => Some("Öffnen".to_string()),
            "One file[0]" => Some("Eine Datei".to_string()),
            "One file[1]" => Some("%d Dateien".to_string()),
            "#Last-Translator" => Some("Anna <anna@example.com>".to_string()),
            _ => None,
        };
        let fuzzy = |id: &str| id == "menu|Open";
        let p =
            BuildParams { lang: "de", options: PoOptions::default(), resolve: &resolve, fuzzy: &fuzzy, revision_date: Some("2025-01-02 03:04+0100".into()) };
        let out = String::from_utf8(build(POT.as_bytes(), &p).unwrap()).unwrap();
        assert!(out.contains("\r\n") && !out.contains("\r\n\n"), "{out}");
        let text = out.replace("\r\n", "\n");
        assert!(text.starts_with("# SOME DESCRIPTIVE TITLE.\nmsgid \"\"\nmsgstr \"\"\n\"Project-Id-Version: demo 1.0\\n\"\n"), "{text}");
        for needle in [
            "\"PO-Revision-Date: 2025-01-02 03:04+0100\\n\"",
            "\"Content-Type: text/plain; charset=UTF-8\\n\"",
            "\"Last-Translator: Anna <anna@example.com>\\n\"",
            "\"Language-Team: LANGUAGE <LL@li.org>\\n\"",
            "\"Language: de\\n\"",
            "\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"",
            "#. Greeting on the start page\n#: src/main.c:10\n#, c-format\nmsgid \"Hello %s\"\nmsgstr \"Hallo %s\"",
            "#, fuzzy\nmsgctxt \"menu\"\nmsgid \"Open\"\nmsgstr \"Öffnen\"",
            "msgid \"\"\n\"Line one\\n\"\n\"Line \\\"two\\\"\"\nmsgstr \"\"\n",
            "msgstr[0] \"Eine Datei\"\nmsgstr[1] \"%d Dateien\"",
            "#~ msgid \"Old\"\n#~ msgstr \"Alt\"\n",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
        // The result parses and has the same messages.
        let entries = parse_str(&text).unwrap();
        assert_eq!(entries.iter().filter(|e| e.is_message()).count(), 5);
        assert!(!entries[0].is_fuzzy());
    }

    #[test]
    fn plural_forms_follow_target_language() {
        let po = "msgid \"One file\"\nmsgid_plural \"%d files\"\nmsgstr[0] \"\"\nmsgstr[1] \"\"\n";
        let resolve = |id: &str, _: &str| Some(id.to_string());
        let no = |_: &str| false;
        let p = BuildParams { lang: "pl", options: PoOptions { plural_rows: 2, ..Default::default() }, resolve: &resolve, fuzzy: &no, revision_date: None };
        let out = String::from_utf8(build(po.as_bytes(), &p).unwrap()).unwrap();
        // Polish needs 3 forms; with only 2 rows the last row is repeated.
        assert!(out.contains("msgstr[0] \"One file[0]\"\nmsgstr[1] \"One file[1]\"\nmsgstr[2] \"One file[1]\"\n"), "{out}");
        let p = BuildParams { lang: "ja", ..p };
        let out = String::from_utf8(build(po.as_bytes(), &p).unwrap()).unwrap();
        assert!(out.contains("msgstr[0] \"One file[0]\"\n") && !out.contains("msgstr[1]"), "{out}");
    }

    #[test]
    fn multiline_translation_is_wrapped_after_newlines() {
        let mut lines = Vec::new();
        push_value(&mut lines, "msgstr", "a\nb");
        assert_eq!(lines, ["msgstr \"\"", "\"a\\n\"", "\"b\""]);
        lines.clear();
        push_value(&mut lines, "msgstr", "only\n");
        assert_eq!(lines, ["msgstr \"only\\n\""]);
    }

    #[test]
    fn options_roundtrip_attrs() {
        let mut a = Attrs::new();
        assert_eq!(PoOptions::from_attrs(&a), PoOptions::default());
        let o = PoOptions { scan_comments: false, compile: false, msgstr_as_original: true, include_empty: true, big_endian: true, plural_rows: 6 };
        o.store(&mut a);
        assert_eq!(a.get("byteorder"), Some("boBigEndian"));
        assert_eq!(PoOptions::from_attrs(&a), o);
    }
}
