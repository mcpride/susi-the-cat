//! Project operations: adding sources, scanning originals and building localized files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result, read_file, write_file};
use crate::formats::{self, ScannedString, select_output};
use crate::lang::{PatternContext, expand_pattern};
use crate::model::{Item, Node, NodeRef, Project, Row, Source, SourceKind, WriteMethod, now_stamp, stamp_of};
use crate::status::{RowStatus, TranslationStatus};

/// Result of a scan.
#[derive(Debug, Default, Clone)]
pub struct ScanReport {
    pub files: usize,
    pub new: usize,
    pub changed: usize,
    pub unused: usize,
    pub total: usize,
    /// Translations taken over from existing localized files (TSV sources).
    pub imported: usize,
    pub errors: Vec<String>,
}

impl ScanReport {
    fn add(&mut self, o: &ScanReport) {
        self.files += o.files;
        self.new += o.new;
        self.changed += o.changed;
        self.unused += o.unused;
        self.total += o.total;
        self.imported += o.imported;
        self.errors.extend(o.errors.iter().cloned());
    }

    pub fn summary(&self) -> String {
        let mut s = crate::tf!(
            "{files} scanned: {rows} rows, {new} new, {changed} changed, {unused} unused, {errors} errors",
            files = crate::i18n::trn(self.files, "{n} file", "{n} files"),
            rows = self.total,
            new = self.new,
            changed = self.changed,
            unused = self.unused,
            errors = self.errors.len()
        );
        if self.imported > 0 {
            s.push_str(", ");
            s.push_str(&crate::i18n::trn(self.imported, "{n} translation imported", "{n} translations imported"));
        }
        s
    }
}

/// Result of a build.
#[derive(Debug, Default, Clone)]
pub struct BuildReport {
    /// Files that were written.
    pub written: Vec<PathBuf>,
    /// Files whose content did not change (left untouched).
    pub unchanged: Vec<PathBuf>,
    pub errors: Vec<String>,
}

impl BuildReport {
    pub fn summary(&self) -> String {
        crate::tf!(
            "{files} written, {unchanged} unchanged, {errors} errors",
            files = crate::i18n::trn(self.written.len(), "{n} file", "{n} files"),
            unchanged = self.unchanged.len(),
            errors = self.errors.len()
        )
    }
}

fn native_path(name: &str) -> PathBuf {
    PathBuf::from(name.replace(['\\', '/'], std::path::MAIN_SEPARATOR_STR))
}

/// Directory a source name is relative to and the file name / wildcard part.
fn source_location(p: &Project, s: &Source) -> (PathBuf, String) {
    let full = p.base_dir().join(native_path(s.name()));
    let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
    let file = full.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    (dir, file)
}

/// Localized output file for `file` of source `si` and language `lang`.
pub fn output_path(p: &Project, si: usize, file: &Path, lang: &str) -> Option<PathBuf> {
    let s = p.sources.get(si)?;
    let (root, _) = source_location(p, s);
    let (dir, pattern) = s.output_pattern();
    let file_dir = file.parent().unwrap_or(&root);
    let out_dir = normalize(&file_dir.join(native_path(&dir)));
    let ctx = PatternContext { file, source_dir: file_dir, out_dir: &out_dir, lang };
    Some(normalize(&expand_pattern(&pattern, &ctx)))
}

/// Removes `.` and resolves `..` components lexically.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn walk_dir(dir: &Path, recursive: bool, pattern: &glob::Pattern, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    let opts = glob::MatchOptions { case_sensitive: false, require_literal_separator: true, require_literal_leading_dot: false };
    for path in entries {
        if path.is_dir() {
            if recursive && depth < 32 {
                walk_dir(&path, true, pattern, out, depth + 1);
            }
        } else if path.file_name().and_then(|f| f.to_str()).is_some_and(|f| pattern.matches_with(f, opts)) {
            out.push(path);
        }
    }
}

/// Original files of a source (expands wildcards; skips localized outputs of other matches).
pub fn source_files(p: &Project, si: usize) -> Result<Vec<PathBuf>> {
    let s = p.sources.get(si).ok_or_else(|| Error::NotFound(format!("source #{si}")))?;
    let (dir, file) = source_location(p, s);
    if !s.is_wildcard() {
        return Ok(vec![normalize(&dir.join(file))]);
    }
    let pattern = glob::Pattern::new(&file).map_err(|e| Error::Invalid(crate::tf!("invalid wildcard '{file}': {error}", file = file, error = e)))?;
    let mut found = Vec::new();
    walk_dir(&dir, s.subdirs(), &pattern, &mut found, 0);
    let found: Vec<PathBuf> = found.iter().map(|f| normalize(f)).collect();
    let langs = p.language_ids();
    let outputs: std::collections::HashSet<PathBuf> =
        found.iter().flat_map(|f| langs.iter().filter_map(|l| output_path(p, si, f, l)).filter(|o| o != f).collect::<Vec<_>>()).collect();
    Ok(found.into_iter().filter(|f| !outputs.contains(f)).collect())
}

fn node_class(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Resx => "TNetResourceNode",
        SourceKind::Po => "TPoNode",
        SourceKind::Tsv => "TTsvNode",
        _ => "TJsonNode",
    }
}

/// Item path of the container holding the rows of `file` (`rel_name` relative to the source dir).
fn container_for(src: &mut Source, rel_name: &str, wildcard: bool, create: bool) -> Option<Vec<usize>> {
    let named = src.items.iter().position(|i| matches!(i, Item::Node(n) if n.name().eq_ignore_ascii_case(rel_name)));
    if let Some(i) = named {
        return Some(vec![i]);
    }
    if !wildcard {
        let has_direct_rows = src.items.iter().any(|i| matches!(i, Item::Row(_)));
        let nodes: Vec<usize> = src.items.iter().enumerate().filter(|(_, i)| matches!(i, Item::Node(_))).map(|(i, _)| i).collect();
        if has_direct_rows || (nodes.is_empty() && src.kind() == SourceKind::Resx) {
            return Some(Vec::new());
        }
        if nodes.len() == 1 {
            return nodes.first().map(|i| vec![*i]);
        }
    }
    if !create {
        return None;
    }
    let mut node = Node::new(node_class(src.kind()), rel_name);
    node.attrs.set("state", RowStatus::New.as_index().to_string());
    node.attrs.set("created", now_stamp());
    src.items.push(Item::Node(node));
    Some(vec![src.items.len() - 1])
}

fn rel_name(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('/', "\\"))
        .unwrap_or_else(|_| file.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
}

pub fn scan_file(src: &Source, bytes: &[u8]) -> Result<Vec<ScannedString>> {
    match src.kind() {
        SourceKind::Json => formats::json::scan(bytes),
        SourceKind::Resx => formats::resx::scan(bytes),
        SourceKind::Po => formats::po::scan(bytes, &src.po_options()),
        SourceKind::Tsv => formats::tsv::scan(bytes),
        SourceKind::Other => Err(Error::Unsupported(crate::i18n::t("source type not supported by susi").into())),
    }
}

/// Merges scanned strings into a container (rows directly inside `items`).
pub fn merge_rows(items: &mut Vec<Item>, scanned: &[ScannedString]) -> ScanReport {
    let mut rep = ScanReport { total: scanned.len(), ..Default::default() };
    let stamp = now_stamp();
    let mut seen = std::collections::HashSet::new();
    // Insert position for new rows: after the previously handled row.
    let mut cursor: Option<usize> = None;
    for s in scanned {
        if !seen.insert(s.id.as_str()) {
            continue;
        }
        let existing = items.iter().position(|i| matches!(i, Item::Row(r) if r.id() == s.id));
        match existing {
            Some(idx) => {
                if let Some(Item::Row(r)) = items.get_mut(idx)
                    && update_row(r, s, &stamp)
                {
                    rep.changed += 1;
                }
                cursor = Some(idx);
            }
            None => {
                let mut row = Row::new(&s.id, &s.text);
                row.set_status(RowStatus::New);
                row.attrs.set("created", stamp.clone());
                set_dev_comment(&mut row, s.comment.as_deref());
                let at = match cursor {
                    Some(c) => c + 1,
                    None => items.iter().position(|i| matches!(i, Item::Row(_) | Item::Node(_))).unwrap_or(items.len()),
                };
                items.insert(at, Item::Row(row));
                cursor = Some(at);
                rep.new += 1;
            }
        }
    }
    for item in items.iter_mut() {
        if let Item::Row(r) = item
            && !seen.contains(r.id())
            && r.status() != RowStatus::Unused
        {
            r.set_status(RowStatus::Unused);
            rep.unused += 1;
        }
    }
    rep
}

fn set_dev_comment(row: &mut Row, comment: Option<&str>) {
    match comment {
        Some(c) => {
            row.attrs.set("note", c);
            row.attrs.set("notereadonly", "1");
        }
        None if row.attrs.get("notereadonly").is_some() => {
            row.attrs.remove("note");
            row.attrs.remove("notereadonly");
        }
        None => {}
    }
}

/// Updates an existing row from a scanned string. Returns whether the original text changed.
fn update_row(r: &mut Row, s: &ScannedString, stamp: &str) -> bool {
    if s.comment.is_some() || r.attrs.get("notereadonly").is_some() {
        set_dev_comment(r, s.comment.as_deref());
    }
    let changed = r.native.as_deref() != Some(s.text.as_str());
    if changed {
        r.native = Some(s.text.clone());
        r.inline_native = false;
        r.attrs.set("changed", stamp);
        if r.status() != RowStatus::New {
            r.set_status(RowStatus::Changed);
        }
        // Existing translations no longer match the new original: mark them for review.
        for t in &mut r.translations {
            if !t.text.is_empty() {
                t.attrs.set("invalidated", "1");
                t.set_status(TranslationStatus::ForReview);
            }
        }
    } else if r.status() == RowStatus::Unused {
        r.set_status(RowStatus::InUse);
    }
    changed
}

/// Scans one source. Missing or unreadable files are reported, not fatal.
pub fn scan_source(p: &mut Project, si: usize) -> Result<ScanReport> {
    let files = source_files(p, si)?;
    let (root, _) = source_location(p, p.sources.get(si).ok_or_else(|| Error::NotFound(format!("source #{si}")))?);
    let mut rep = ScanReport::default();
    let wildcard = p.sources.get(si).is_some_and(Source::is_wildcard);
    let tsv = p.sources.get(si).is_some_and(|s| s.kind() == SourceKind::Tsv);
    let mut newest = None;
    for f in &files {
        let Some(src) = p.sources.get_mut(si) else { break };
        let scanned = match read_file(f).and_then(|b| scan_file(src, &b)) {
            Ok(s) => s,
            Err(e) => {
                rep.errors.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        if let Ok(m) = std::fs::metadata(f).and_then(|m| m.modified()) {
            newest = newest.max(Some(m));
        }
        let name = rel_name(&root, f);
        let Some(path) = container_for(src, &name, wildcard, true) else { continue };
        let node = NodeRef { source: si, path };
        let Some(items) = p.items_at_mut(&node) else { continue };
        let r = merge_rows(items, &scanned);
        let imported = if tsv { import_tsv_translations(p, si, f, &node) } else { 0 };
        rep.add(&ScanReport { files: 1, imported, ..r });
    }
    if let (false, Some(m), Some(src)) = (wildcard, newest, p.sources.get_mut(si)) {
        src.attrs.set("date", stamp_of(m));
    }
    Ok(rep)
}

/// Takes over the translations of the existing localized files of a TSV original (which may be the
/// original itself, e.g. `de-core.tsv`) into empty cells. Returns the number of imported cells.
fn import_tsv_translations(p: &mut Project, si: usize, file: &Path, node: &NodeRef) -> usize {
    let Some(src) = p.sources.get(si) else { return 0 };
    let original = src.original();
    let found: Vec<(String, HashMap<String, String>)> = p
        .languages
        .iter()
        .filter(|l| !l.is_pseudo() && l.id() != original)
        .filter_map(|l| {
            let bytes = std::fs::read(output_path(p, si, file, l.id())?).ok()?;
            Some((l.id().to_string(), formats::tsv::translations(&bytes).ok()?.into_iter().collect()))
        })
        .collect();
    let Some(items) = p.items_at_mut(node) else { return 0 };
    let mut n = 0;
    for item in items.iter_mut() {
        let Item::Row(r) = item else { continue };
        for (lang, tr) in &found {
            if let Some(t) = tr.get(r.id())
                && r.text(lang).is_none()
            {
                r.set_translation(lang, t, TranslationStatus::Translated);
                n += 1;
            }
        }
    }
    n
}

/// Scans the given sources (all when `None`).
pub fn scan_project(p: &mut Project, sources: Option<&[usize]>) -> Result<ScanReport> {
    let all: Vec<usize> = (0..p.sources.len()).collect();
    let mut rep = ScanReport::default();
    for &si in sources.unwrap_or(&all) {
        match scan_source(p, si) {
            Ok(r) => rep.add(&r),
            Err(e) => rep.errors.push(e.to_string()),
        }
    }
    p.record_scan();
    Ok(rep)
}

/// Adds a new source file (relative paths are stored relative to the project) and scans it.
pub fn add_source(p: &mut Project, file: &Path, original_lang: &str) -> Result<(usize, ScanReport)> {
    let name = file.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let kind = SourceKind::from_path(Path::new(&name))
        .ok_or_else(|| Error::Unsupported(crate::tf!("{file}: only .json, .resx, .po, .pot and .tsv are supported", file = file.display())))?;
    let base = p.base_dir();
    let abs = if file.is_absolute() { file.to_path_buf() } else { std::env::current_dir().map(|d| d.join(file)).unwrap_or_else(|_| file.to_path_buf()) };
    let rel = relative_path(&normalize(&base_abs(&base)), &normalize(&abs)).unwrap_or(abs);
    let rel = rel.to_string_lossy().replace('/', "\\");
    if p.sources.iter().any(|s| s.name().eq_ignore_ascii_case(&rel)) {
        return Err(Error::Invalid(crate::tf!("source '{source}' is already part of the project", source = rel)));
    }
    let mut src = Source::new(kind, &rel, original_lang);
    let (dir, pattern) = src.output_pattern();
    src.set_output_pattern(&dir, &pattern);
    p.sources.push(src);
    let si = p.sources.len() - 1;
    let rep = scan_source(p, si)?;
    Ok((si, rep))
}

fn base_abs(base: &Path) -> PathBuf {
    if base.is_absolute() { base.to_path_buf() } else { std::env::current_dir().map(|d| d.join(base)).unwrap_or_else(|_| base.to_path_buf()) }
}

/// `target` relative to `base` (both absolute and normalized); `None` on different roots.
pub fn relative_path(base: &Path, target: &Path) -> Option<PathBuf> {
    let b: Vec<_> = base.components().collect();
    let t: Vec<_> = target.components().collect();
    if b.first().map(|c| c.as_os_str().to_ascii_lowercase()) != t.first().map(|c| c.as_os_str().to_ascii_lowercase()) {
        return None;
    }
    let common = b.iter().zip(&t).take_while(|(x, y)| x.as_os_str().eq_ignore_ascii_case(y.as_os_str())).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in t.iter().skip(common) {
        out.push(c);
    }
    Some(out)
}

/// Rows of the container of `file`, by id.
fn rows_for_file<'a>(p: &'a Project, si: usize, file: &Path) -> HashMap<&'a str, &'a Row> {
    let mut map = HashMap::new();
    let Some(s) = p.sources.get(si) else { return map };
    let (root, _) = source_location(p, s);
    let mut tmp = s.clone();
    let Some(path) = container_for(&mut tmp, &rel_name(&root, file), s.is_wildcard(), false) else { return map };
    let items = p.items_at(&NodeRef { source: si, path });
    for item in items.into_iter().flatten() {
        if let Item::Row(r) = item {
            map.entry(r.id()).or_insert(r);
        }
    }
    map
}

/// Produces the localized content of one original file.
pub fn build_content(src: &Source, original: &[u8], rows: &HashMap<&str, &Row>, lang: &str) -> Result<Vec<u8>> {
    let method = src.write_method();
    let resolve = |id: &str, orig: &str| select_output(method, rows.get(id).copied(), lang, orig);
    match src.kind() {
        SourceKind::Json => formats::json::build(original, &resolve, method == WriteMethod::All),
        SourceKind::Resx => formats::resx::build(original, &resolve),
        SourceKind::Po => {
            let fuzzy = |id: &str| rows.get(id).is_some_and(|r| matches!(r.tstatus(lang), TranslationStatus::ForReview | TranslationStatus::BestGuess));
            let params =
                formats::po::BuildParams { lang, options: src.po_options(), resolve: &resolve, fuzzy: &fuzzy, revision_date: revision_date(rows, lang) };
            formats::po::build(original, &params)
        }
        SourceKind::Tsv => formats::tsv::build(original, &resolve),
        SourceKind::Other => Err(Error::Unsupported(crate::i18n::t("source type not supported by susi").into())),
    }
}

/// Builds localized files for `langs` (all project languages when empty) and the given sources.
pub fn build_project(p: &Project, langs: &[String], sources: Option<&[usize]>) -> BuildReport {
    let mut rep = BuildReport::default();
    let langs: Vec<String> = if langs.is_empty() { p.language_ids() } else { langs.to_vec() };
    let all: Vec<usize> = (0..p.sources.len()).collect();
    for &si in sources.unwrap_or(&all) {
        let Some(src) = p.sources.get(si) else { continue };
        let files = match source_files(p, si) {
            Ok(f) => f,
            Err(e) => {
                rep.errors.push(e.to_string());
                continue;
            }
        };
        for f in &files {
            let original = match read_file(f) {
                Ok(b) => b,
                Err(e) => {
                    rep.errors.push(e.to_string());
                    continue;
                }
            };
            let rows = rows_for_file(p, si, f);
            for lang in langs.iter().filter(|l| l.as_str() != src.original()) {
                if let Some(opts) = pseudo_options(p, lang) {
                    let owned = with_pseudo(&rows, lang, &opts);
                    let map: HashMap<&str, &Row> = owned.iter().map(|(k, r)| (k.as_str(), r)).collect();
                    build_one(p, si, src, f, &original, &map, lang, &mut rep);
                } else {
                    build_one(p, si, src, f, &original, &rows, lang, &mut rep);
                }
            }
        }
    }
    rep
}

/// Pseudo options for a pseudo language (`<lang pseudo="1"><test type="..."/>`).
fn pseudo_options(p: &Project, lang: &str) -> Option<crate::pseudo::PseudoOptions> {
    let l = p.languages.iter().find(|l| l.id() == lang && l.is_pseudo())?;
    let kind =
        l.children.iter().find(|c| c.name == "test").and_then(|t| t.attr("type")).and_then(crate::pseudo::PseudoKind::from_type_name).unwrap_or_default();
    Some(crate::pseudo::PseudoOptions { kind, ..Default::default() })
}

/// Copies of the rows where untranslated cells of `lang` are pseudo translated.
fn with_pseudo(rows: &HashMap<&str, &Row>, lang: &str, opts: &crate::pseudo::PseudoOptions) -> Vec<(String, Row)> {
    rows.iter()
        .map(|(k, r)| {
            let mut row = (*r).clone();
            if !row.locked() && row.build_text(lang).is_none() && !row.original().is_empty() {
                row.set_translation(lang, &crate::pseudo::pseudo_text(row.original(), opts), TranslationStatus::AutoTranslated);
            }
            ((*k).to_string(), row)
        })
        .collect()
}

/// Latest translation date of `lang` as PO `PO-Revision-Date` (`YYYY-MM-DD HH:MM+ZZZZ`).
fn revision_date(rows: &HashMap<&str, &Row>, lang: &str) -> Option<String> {
    let latest = rows.values().filter_map(|r| r.translation(lang)).filter(|t| !t.text.is_empty()).filter_map(|t| t.attrs.get("date")).max()?;
    let dt = chrono::NaiveDateTime::parse_from_str(latest, "%Y%m%d%H%M%S").ok()?;
    let local = dt.and_local_timezone(chrono::Local).earliest()?;
    Some(local.format("%Y-%m-%d %H:%M%z").to_string())
}

/// Writes one output file unless its content is unchanged.
fn write_output(out: PathBuf, bytes: &[u8], rep: &mut BuildReport) {
    if std::fs::read(&out).is_ok_and(|old| old == bytes) {
        rep.unchanged.push(out);
    } else if let Err(e) = write_file(&out, bytes) {
        rep.errors.push(e.to_string());
    } else {
        rep.written.push(out);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_one(p: &Project, si: usize, src: &Source, f: &Path, original: &[u8], rows: &HashMap<&str, &Row>, lang: &str, rep: &mut BuildReport) {
    let Some(out) = output_path(p, si, f, lang) else { return };
    // A TSV catalog may hold the translations of one language itself (e.g. `de-core.tsv`).
    if out == *f && src.kind() != SourceKind::Tsv {
        rep.errors.push(crate::tf!("{file}: output file equals the original file (check the output pattern)", file = out.display()));
        return;
    }
    let bytes = match build_content(src, original, rows, lang) {
        Ok(b) => b,
        Err(e) => {
            rep.errors.push(format!("{}: {e}", f.display()));
            return;
        }
    };
    let opts = src.po_options();
    let mo = (src.kind() == SourceKind::Po && opts.compile).then(|| formats::mo::compile(&bytes, opts.include_empty, opts.big_endian));
    write_output(out.clone(), &bytes, rep);
    match mo {
        Some(Ok(mo)) => write_output(out.with_extension("mo"), &mo, rep),
        Some(Err(e)) => rep.errors.push(format!("{}: {e}", out.display())),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(pairs: &[(&str, &str)]) -> Vec<ScannedString> {
        pairs.iter().map(|(i, t)| ScannedString { id: (*i).into(), text: (*t).into(), comment: None }).collect()
    }

    #[test]
    fn merge_marks_new_changed_unused() {
        let mut items = Vec::new();
        let r = merge_rows(&mut items, &scanned(&[("a", "A"), ("b", "B"), ("c", "C")]));
        assert_eq!((r.new, r.changed, r.unused), (3, 0, 0));
        for i in &mut items {
            if let Item::Row(row) = i {
                row.set_status(RowStatus::InUse);
                row.set_translation("de", "x", TranslationStatus::Translated);
            }
        }
        let r = merge_rows(&mut items, &scanned(&[("a", "A"), ("x", "X"), ("b", "B2")]));
        assert_eq!((r.new, r.changed, r.unused), (1, 1, 1));
        let ids: Vec<_> = items.iter().filter_map(|i| if let Item::Row(r) = i { Some(r.id().to_string()) } else { None }).collect();
        assert_eq!(ids, ["a", "x", "b", "c"]);
        let Some(Item::Row(b)) = items.get(2) else { panic!() };
        assert_eq!(b.status(), RowStatus::Changed);
        assert_eq!(b.tstatus("de"), TranslationStatus::ForReview);
        let Some(Item::Row(c)) = items.get(3) else { panic!() };
        assert_eq!(c.status(), RowStatus::Unused);
    }

    #[cfg(windows)]
    #[test]
    fn relative_paths() {
        let b = Path::new("C:\\p\\sisulizer");
        assert_eq!(relative_path(b, Path::new("C:\\p\\src\\a.json")).unwrap(), Path::new("..\\src\\a.json"));
        assert!(relative_path(b, Path::new("D:\\x.json")).is_none());
    }
}
