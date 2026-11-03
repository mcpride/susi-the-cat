//! In-memory model of a Susi/Sisulizer project (`.slp`).
//!
//! The model is typed where Susi needs it (languages, sources, nodes, rows, translations) and keeps
//! everything else verbatim so that saving a project written by Sisulizer changes nothing Susi
//! did not touch.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::status::{RowStatus, TranslationStatus};
use crate::xml::{Attrs, Element};

/// Timestamp in `yyyymmddhhnnss` format (local time).
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y%m%d%H%M%S").to_string()
}

/// Formats a file modification time like [`now_stamp`].
pub fn stamp_of(time: std::time::SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = time.into();
    dt.format("%Y%m%d%H%M%S").to_string()
}

/// Turns `yyyymmddhhnnss` into `yyyy-mm-dd hh:nn:ss` for display.
pub fn display_stamp(s: &str) -> String {
    match chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S") {
        Ok(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => s.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct Project {
    /// Location of the `.slp` file (relative source paths are resolved against its directory).
    pub path: Option<PathBuf>,
    /// `<document>` attributes (created, version, date, scan, scanned, ...).
    pub attrs: Attrs,
    pub languages: Vec<Language>,
    pub sources: Vec<Source>,
    /// Unknown `<document>` children that appeared before the first source.
    pub pre: Vec<Element>,
    /// Unknown `<document>` children after the sources (excludedoriginals, dictionaries, ...).
    pub post: Vec<Element>,
    /// XML declaration as read (e.g. `xml version="1.0" encoding="UTF-8"`); default when `None`.
    pub decl: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Language {
    pub attrs: Attrs,
    pub children: Vec<Element>,
}

impl Language {
    pub fn new(id: &str) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("id", id);
        Self { attrs, children: Vec::new() }
    }

    pub fn id(&self) -> &str {
        self.attrs.get("id").unwrap_or("")
    }

    pub fn is_pseudo(&self) -> bool {
        self.attrs.get("pseudo").is_some_and(|v| v != "0" && !v.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceKind {
    Json,
    Resx,
    Po,
    /// Tab-separated i18n catalog (Susi extension, unknown to Sisulizer).
    Tsv,
    Other,
}

impl SourceKind {
    pub fn class_name(self) -> &'static str {
        match self {
            SourceKind::Json => "TJsonSource",
            SourceKind::Resx => "TNetResourceSource",
            SourceKind::Po => "TPoSource",
            SourceKind::Tsv => "TTsvSource",
            SourceKind::Other => "",
        }
    }

    pub fn from_class(class: &str) -> Self {
        match class {
            "TJsonSource" => SourceKind::Json,
            "TNetResourceSource" => SourceKind::Resx,
            "TPoSource" => SourceKind::Po,
            "TTsvSource" => SourceKind::Tsv,
            _ => SourceKind::Other,
        }
    }

    /// Detects the kind of a file from its extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "json" => Some(SourceKind::Json),
            "resx" => Some(SourceKind::Resx),
            "po" | "pot" => Some(SourceKind::Po),
            "tsv" => Some(SourceKind::Tsv),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Json => "JSON",
            SourceKind::Resx => ".NET RESX",
            SourceKind::Po => "PO (gettext)",
            SourceKind::Tsv => "TSV (i18n catalog)",
            SourceKind::Other => crate::i18n::t("Other"),
        }
    }
}

/// Items that localized resource files contain. 
/// Stored in the `writemethod` source attribute (`nwDifferent`, `nwTranslated`, `nwAll`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WriteMethod {
    /// Only items whose translation differs from the original.
    Different,
    /// Only items that have a translation.
    Translated,
    /// All items; untranslated ones use the original text.
    All,
}

impl WriteMethod {
    pub fn attr_value(self) -> &'static str {
        match self {
            WriteMethod::Different => "nwDifferent",
            WriteMethod::Translated => "nwTranslated",
            WriteMethod::All => "nwAll",
        }
    }

    pub fn from_attr(s: &str) -> Option<Self> {
        match s {
            "nwDifferent" => Some(WriteMethod::Different),
            "nwTranslated" => Some(WriteMethod::Translated),
            "nwAll" => Some(WriteMethod::All),
            _ => None,
        }
    }

    pub fn from_index(i: u32) -> Option<Self> {
        match i {
            0 => Some(WriteMethod::Different),
            1 => Some(WriteMethod::Translated),
            2 => Some(WriteMethod::All),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WriteMethod::Different => crate::i18n::tr_ctx("write method", "Different"),
            WriteMethod::Translated => crate::i18n::tr_ctx("write method", "Translated"),
            WriteMethod::All => crate::i18n::tr_ctx("write method", "All"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Source {
    pub attrs: Attrs,
    pub items: Vec<Item>,
}

impl Source {
    pub fn new(kind: SourceKind, name: &str, original: &str) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("class", kind.class_name());
        attrs.set("name", name);
        attrs.set("original", original);
        Self { attrs, items: Vec::new() }
    }

    pub fn class(&self) -> &str {
        self.attrs.get("class").unwrap_or("")
    }

    pub fn kind(&self) -> SourceKind {
        SourceKind::from_class(self.class())
    }

    /// Source file name (relative to the project file, may contain wildcards).
    pub fn name(&self) -> &str {
        self.attrs.get("name").unwrap_or("")
    }

    /// Original language id (e.g. `en`; may be a pseudo id such as `sis`).
    pub fn original(&self) -> &str {
        self.attrs.get("original").unwrap_or("")
    }

    pub fn is_wildcard(&self) -> bool {
        self.name().contains(['*', '?'])
    }

    pub fn subdirs(&self) -> bool {
        self.attrs.get("subdirs").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    pub fn write_method(&self) -> WriteMethod {
        if let Some(m) = self.attrs.get("writemethod").and_then(WriteMethod::from_attr) {
            return m;
        }
        // Observed in the admin projects: localized JSON files contain only translated keys
        // (untranslated languages produce `{}`); .NET resources are documented the same way.
        WriteMethod::Translated
    }

    pub fn set_write_method(&mut self, m: WriteMethod) {
        self.attrs.set("writemethod", m.attr_value());
    }

    /// Options of a PO source (defaults for other kinds).
    pub fn po_options(&self) -> crate::formats::po::PoOptions {
        crate::formats::po::PoOptions::from_attrs(&self.attrs)
    }

    pub fn set_po_options(&mut self, o: &crate::formats::po::PoOptions) {
        o.store(&mut self.attrs);
    }

    /// The `<outputfile>` element, if any.
    pub fn output_file(&self) -> Option<&Element> {
        self.items.iter().find_map(|i| match i {
            Item::Other(e) if e.name == "outputfile" => Some(e),
            _ => None,
        })
    }

    /// Output directory (relative to the source file directory) and localized file pattern.
    pub fn output_pattern(&self) -> (String, String) {
        let default_pattern = match self.kind() {
            SourceKind::Resx => "<body>.<net>.<ext>".to_string(),
            // gettext convention: one directory per language; templates (.pot) become .po.
            SourceKind::Po => "<iso>\\<body>.po".to_string(),
            SourceKind::Tsv => crate::formats::tsv::default_pattern(self.name()),
            _ => "<sl>\\<file>".to_string(),
        };
        match self.output_file() {
            Some(e) => (e.attr("dir").unwrap_or(".").to_string(), e.attr("localized").filter(|s| !s.is_empty()).map_or(default_pattern, str::to_string)),
            None => (".".to_string(), default_pattern),
        }
    }

    pub fn set_output_pattern(&mut self, dir: &str, localized: &str) {
        let pos = self.items.iter().position(|i| matches!(i, Item::Other(e) if e.name == "outputfile"));
        let mut el = match pos {
            Some(p) => match self.items.remove(p) {
                Item::Other(e) => e,
                _ => Element::new("outputfile"),
            },
            None => Element::new("outputfile"),
        };
        el.attrs.set("dir", dir);
        el.attrs.set("localized", localized);
        // Sisulizer writes <outputfile> after the option elements and before nodes and rows.
        let insert_at = pos.unwrap_or_else(|| self.items.iter().position(|i| !matches!(i, Item::Other(_))).unwrap_or(self.items.len()));
        self.items.insert(insert_at.min(self.items.len()), Item::Other(el));
    }

    pub fn row_count(&self) -> usize {
        count_rows(&self.items)
    }
}

fn count_rows(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Item::Row(_) => 1,
            Item::Node(n) => count_rows(&n.items),
            Item::Other(_) => 0,
        })
        .sum()
}

#[derive(Debug, Clone)]
pub enum Item {
    Node(Node),
    Row(Row),
    Other(Element),
}

/// A container element holding rows (`<node>` in JSON/RESX projects; `<component>`, `<item>`,
/// `<tag>`, ... in other source types).
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub tag: String,
    pub attrs: Attrs,
    pub items: Vec<Item>,
}

impl Node {
    pub fn new(class: &str, name: &str) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("class", class);
        attrs.set("name", name);
        Self { tag: "node".into(), attrs, items: Vec::new() }
    }

    pub fn name(&self) -> &str {
        self.attrs.get("name").or_else(|| self.attrs.get("class")).unwrap_or(&self.tag)
    }

    pub fn class(&self) -> &str {
        self.attrs.get("class").unwrap_or("")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Translation {
    pub attrs: Attrs,
    pub text: String,
}

impl Translation {
    pub fn lang(&self) -> &str {
        self.attrs.get("id").unwrap_or("")
    }

    pub fn status(&self) -> TranslationStatus {
        match self.attrs.get("status").and_then(|s| s.parse::<u32>().ok()).and_then(TranslationStatus::from_index) {
            Some(s) => s,
            None => TranslationStatus::Complete,
        }
    }

    pub fn set_status(&mut self, status: TranslationStatus) {
        if status == TranslationStatus::Complete {
            self.attrs.remove("status");
        } else {
            self.attrs.set("status", status.index().to_string());
        }
    }

    pub fn comment(&self) -> &str {
        self.attrs.get("comment").unwrap_or("")
    }

    pub fn set_comment(&mut self, c: &str) {
        if c.is_empty() {
            self.attrs.remove("comment");
        } else {
            self.attrs.set("comment", c);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Row {
    pub attrs: Attrs,
    /// Original text (`<native>` or inline content). `None` if the row has neither.
    pub native: Option<String>,
    /// The original text was stored as inline element content (`<row id="x">text</row>`).
    pub inline_native: bool,
    pub translations: Vec<Translation>,
    /// Unknown row children that precede `<native>` (e.g. `<prev>`), preserved verbatim.
    pub pre_native: Vec<Element>,
    /// Unknown row children, preserved verbatim.
    pub extra: Vec<Element>,
    /// Texts contain literal line breaks (pretty-printed binary data such as images) and are
    /// stored without Sisulizer's `#c`/`#l` coding.
    pub raw: bool,
}

impl Row {
    pub fn new(id: &str, native: &str) -> Self {
        let mut attrs = Attrs::new();
        attrs.set("id", id);
        Self { attrs, native: Some(native.to_string()), ..Default::default() }
    }

    pub fn id(&self) -> &str {
        self.attrs.get("id").unwrap_or("")
    }

    pub fn original(&self) -> &str {
        self.native.as_deref().unwrap_or("")
    }

    pub fn status(&self) -> RowStatus {
        self.attrs.get("state").and_then(|s| s.parse::<u32>().ok()).and_then(RowStatus::from_index).unwrap_or(RowStatus::InUse)
    }

    pub fn set_status(&mut self, s: RowStatus) {
        if s == RowStatus::InUse {
            self.attrs.remove("state");
        } else {
            self.attrs.set("state", s.as_index().to_string());
        }
    }

    fn flag(&self, name: &str) -> bool {
        self.attrs.get(name).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    fn set_flag(&mut self, name: &str, on: bool) {
        if on {
            self.attrs.set(name, "1");
        } else {
            self.attrs.remove(name);
        }
    }

    /// "Nicht übersetzen" (do not translate).
    pub fn locked(&self) -> bool {
        self.flag("locked")
    }

    pub fn set_locked(&mut self, on: bool) {
        self.set_flag("locked", on);
    }

    pub fn excluded(&self) -> bool {
        self.flag("excluded")
    }

    pub fn set_excluded(&mut self, on: bool) {
        self.set_flag("excluded", on);
    }

    pub fn marked(&self) -> bool {
        self.flag("marked")
    }

    pub fn set_marked(&mut self, on: bool) {
        self.set_flag("marked", on);
    }

    /// Row comment (`note`).
    pub fn note(&self) -> &str {
        self.attrs.get("note").unwrap_or("")
    }

    pub fn set_note(&mut self, note: &str) {
        if note.is_empty() {
            self.attrs.remove("note");
        } else {
            self.attrs.set("note", note);
        }
    }

    /// Maximum number of characters a translation may have (0 = unlimited).
    pub fn max_chars(&self) -> usize {
        self.attrs.get("chars").and_then(|s| s.parse().ok()).unwrap_or(0)
    }

    pub fn set_max_chars(&mut self, n: usize) {
        if n == 0 {
            self.attrs.remove("chars");
        } else {
            self.attrs.set("chars", n.to_string());
        }
    }

    pub fn translation(&self, lang: &str) -> Option<&Translation> {
        self.translations.iter().find(|t| t.lang() == lang)
    }

    pub fn translation_mut(&mut self, lang: &str) -> Option<&mut Translation> {
        self.translations.iter_mut().find(|t| t.lang() == lang)
    }

    /// Translated text, if a non-empty translation exists.
    pub fn text(&self, lang: &str) -> Option<&str> {
        self.translation(lang).map(|t| t.text.as_str()).filter(|t| !t.is_empty())
    }

    pub fn tstatus(&self, lang: &str) -> TranslationStatus {
        match self.translation(lang) {
            Some(t) if !t.text.is_empty() => t.status(),
            _ => TranslationStatus::NotTranslated,
        }
    }

    /// Text to use when building `lang`: the translation if translated and the row is not locked.
    pub fn build_text(&self, lang: &str) -> Option<&str> {
        if self.locked() {
            return None;
        }
        match self.translation(lang) {
            Some(t) if !t.text.is_empty() && t.status() != TranslationStatus::NotTranslated => Some(&t.text),
            _ => None,
        }
    }

    /// Sets (or clears, for an empty text) the translation for `lang`.
    pub fn set_translation(&mut self, lang: &str, text: &str, status: TranslationStatus) {
        if text.is_empty() {
            self.translations.retain(|t| t.lang() != lang);
            return;
        }
        let stamp = now_stamp();
        if let Some(t) = self.translation_mut(lang) {
            if t.text != text {
                t.text = text.to_string();
                t.attrs.remove("savedtotm");
                t.attrs.remove("invalidated");
            }
            t.attrs.set("date", stamp);
            t.set_status(status);
            return;
        }
        let mut attrs = Attrs::new();
        attrs.set("id", lang);
        attrs.set("date", stamp);
        let mut t = Translation { attrs, text: text.to_string() };
        t.set_status(status);
        self.translations.push(t);
    }

    /// Sets the status of an existing translation (no-op if there is none).
    pub fn set_tstatus(&mut self, lang: &str, status: TranslationStatus) -> bool {
        match self.translation_mut(lang) {
            Some(t) if !t.text.is_empty() => {
                t.set_status(status);
                true
            }
            _ => false,
        }
    }

    pub fn remove_translation(&mut self, lang: &str) {
        self.translations.retain(|t| t.lang() != lang);
    }
}

impl RowStatus {
    pub fn as_index(self) -> u32 {
        self as u32
    }
}

/// Address of a row: source index plus the item indices down to the row.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct RowRef {
    pub source: usize,
    pub path: Vec<usize>,
}

/// Address of a container (source or node).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct NodeRef {
    pub source: usize,
    pub path: Vec<usize>,
}

impl RowRef {
    /// Whether the row lies inside the container `node`.
    pub fn is_inside(&self, node: &NodeRef) -> bool {
        self.source == node.source && self.path.len() > node.path.len() && self.path.starts_with(&node.path)
    }
}

impl Project {
    pub fn new() -> Self {
        let mut attrs = Attrs::new();
        let stamp = now_stamp();
        attrs.set("created", stamp.clone());
        attrs.set("version", crate::SLP_VERSION);
        attrs.set("date", stamp);
        attrs.set("scan", "0");
        Self { attrs, ..Default::default() }
    }

    /// Directory against which relative source paths are resolved.
    pub fn base_dir(&self) -> PathBuf {
        self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn language_ids(&self) -> Vec<String> {
        self.languages.iter().map(|l| l.id().to_string()).collect()
    }

    pub fn has_language(&self, id: &str) -> bool {
        self.languages.iter().any(|l| l.id() == id)
    }

    pub fn add_language(&mut self, id: &str) -> bool {
        let id = id.trim();
        if id.is_empty() || self.has_language(id) {
            return false;
        }
        self.languages.push(Language::new(id));
        true
    }

    /// Removes a language and all its translations.
    pub fn remove_language(&mut self, id: &str) -> bool {
        let before = self.languages.len();
        self.languages.retain(|l| l.id() != id);
        if self.languages.len() == before {
            return false;
        }
        for r in self.rows() {
            if let Some(row) = self.row_mut(&r) {
                row.remove_translation(id);
            }
        }
        true
    }

    /// All rows in document order.
    pub fn rows(&self) -> Vec<RowRef> {
        let mut out = Vec::new();
        for (si, s) in self.sources.iter().enumerate() {
            collect_rows(&s.items, si, &mut Vec::new(), &mut out);
        }
        out
    }

    /// Rows inside a container (source or node).
    pub fn rows_in(&self, node: &NodeRef) -> Vec<RowRef> {
        let mut out = Vec::new();
        if let Some(items) = self.items_at(node) {
            let mut prefix = node.path.clone();
            collect_rows(items, node.source, &mut prefix, &mut out);
        }
        out
    }

    pub fn items_at(&self, node: &NodeRef) -> Option<&Vec<Item>> {
        let mut items = &self.sources.get(node.source)?.items;
        for &i in &node.path {
            match items.get(i)? {
                Item::Node(n) => items = &n.items,
                _ => return None,
            }
        }
        Some(items)
    }

    pub fn items_at_mut(&mut self, node: &NodeRef) -> Option<&mut Vec<Item>> {
        let mut items = &mut self.sources.get_mut(node.source)?.items;
        for &i in &node.path {
            match items.get_mut(i)? {
                Item::Node(n) => items = &mut n.items,
                _ => return None,
            }
        }
        Some(items)
    }

    pub fn node(&self, node: &NodeRef) -> Option<&Node> {
        let (last, parent) = node.path.split_last()?;
        let items = self.items_at(&NodeRef { source: node.source, path: parent.to_vec() })?;
        match items.get(*last)? {
            Item::Node(n) => Some(n),
            _ => None,
        }
    }

    pub fn row(&self, r: &RowRef) -> Option<&Row> {
        let (last, parent) = r.path.split_last()?;
        let items = self.items_at(&NodeRef { source: r.source, path: parent.to_vec() })?;
        match items.get(*last)? {
            Item::Row(row) => Some(row),
            _ => None,
        }
    }

    pub fn row_mut(&mut self, r: &RowRef) -> Option<&mut Row> {
        let (last, parent) = r.path.split_last()?;
        let items = self.items_at_mut(&NodeRef { source: r.source, path: parent.to_vec() })?;
        match items.get_mut(*last)? {
            Item::Row(row) => Some(row),
            _ => None,
        }
    }

    /// Names of the nodes between the source and the row.
    pub fn node_names(&self, r: &RowRef) -> Vec<String> {
        let mut names = Vec::new();
        let Some(source) = self.sources.get(r.source) else {
            return names;
        };
        let mut items = &source.items;
        for &i in r.path.iter().take(r.path.len().saturating_sub(1)) {
            match items.get(i) {
                Some(Item::Node(n)) => {
                    names.push(n.name().to_string());
                    items = &n.items;
                }
                _ => break,
            }
        }
        names
    }

    /// Context string identifying a row within its source: node names and row id joined by `/`.
    pub fn context(&self, r: &RowRef) -> String {
        let mut parts = self.node_names(r);
        parts.push(self.row(r).map(|x| x.id().to_string()).unwrap_or_default());
        parts.join("/")
    }

    /// Original language of the source containing `r`.
    pub fn original_lang(&self, r: &RowRef) -> &str {
        self.sources.get(r.source).map(Source::original).unwrap_or("")
    }

    /// Target languages (every project language except pseudo-original ids of sources).
    pub fn target_languages(&self) -> Vec<String> {
        self.languages.iter().map(|l| l.id().to_string()).collect()
    }

    /// Marks the document as saved now (updates `date`).
    pub fn touch(&mut self) {
        self.attrs.set("date", now_stamp());
    }

    /// Records a completed scan in the document attributes.
    pub fn record_scan(&mut self) {
        let n = self.attrs.get("scan").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        self.attrs.set("scan", (n + 1).to_string());
        self.attrs.set("scanned", now_stamp());
    }

    /// Finds a row by context string or plain id (first match).
    pub fn find_row(&self, context_or_id: &str) -> Option<RowRef> {
        let rows = self.rows();
        rows.iter().find(|r| self.context(r) == context_or_id).cloned().or_else(|| {
            let mut it = rows.iter().filter(|r| self.row(r).is_some_and(|x| x.id() == context_or_id));

            it.next().cloned()
        })
    }

    /// Removes rows whose row status is Unused. Returns how many were removed.
    pub fn remove_unused_rows(&mut self) -> usize {
        let mut n = 0;
        for s in &mut self.sources {
            n += remove_unused(&mut s.items);
        }
        n
    }
}

fn remove_unused(items: &mut Vec<Item>) -> usize {
    let before = items.len();
    items.retain(|i| !matches!(i, Item::Row(r) if r.status() == RowStatus::Unused));
    let mut n = before - items.len();
    for i in items.iter_mut() {
        if let Item::Node(node) = i {
            n += remove_unused(&mut node.items);
        }
    }
    n
}

fn collect_rows(items: &[Item], source: usize, prefix: &mut Vec<usize>, out: &mut Vec<RowRef>) {
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Row(_) => {
                let mut path = prefix.clone();
                path.push(i);
                out.push(RowRef { source, path });
            }
            Item::Node(n) => {
                prefix.push(i);
                collect_rows(&n.items, source, prefix, out);
                prefix.pop();
            }
            Item::Other(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_status_rules() {
        let mut r = Row::new("a", "Hello");
        assert_eq!(r.tstatus("de"), TranslationStatus::NotTranslated);
        r.set_translation("de", "Hallo", TranslationStatus::Translated);
        assert_eq!(r.tstatus("de"), TranslationStatus::Translated);
        assert_eq!(r.translation("de").unwrap().attrs.get("status"), Some("3"));
        r.set_tstatus("de", TranslationStatus::Complete);
        assert_eq!(r.translation("de").unwrap().attrs.get("status"), None);
        assert_eq!(r.tstatus("de"), TranslationStatus::Complete);
        r.set_locked(true);
        assert_eq!(r.build_text("de"), None);
        r.set_translation("de", "", TranslationStatus::Translated);
        assert!(r.translation("de").is_none());
    }

    #[test]
    fn row_status_default_in_use() {
        let mut r = Row::new("a", "x");
        assert_eq!(r.status(), RowStatus::InUse);
        r.set_status(RowStatus::New);
        assert_eq!(r.attrs.get("state"), Some("0"));
        r.set_status(RowStatus::InUse);
        assert_eq!(r.attrs.get("state"), None);
    }
}
