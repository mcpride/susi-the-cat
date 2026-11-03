//! Tab-separated i18n catalogs (`context<TAB>source<TAB>translation`), the format of Susi's own
//! UI catalogs and PhotoCraft's (see [`crate::i18n::catalog`]). Sisulizer hasn't such a source type,
//! so projects with TSV sources use the Susi-specific class `TTsvSource`.
//!
//! The original file is a catalog whose second column holds the original texts. Its third column
//! may already contain translations (e.g. `de-core.tsv`: English originals, German translations);
//! scanning imports those of the project languages into empty cells (see `ops::scan_source`).
//! Building rewrites only the third column and keeps comments, blank lines, order and line
//! endings, so the localized file of one language may be the original file itself. Untranslated
//! entries keep an empty third column, which the catalog loader treats as "use the original".
//!
//! Row ids: the escaped source as written in the file, prefixed by `context|` when a context
//! exists. Plural entries (context `@plural`) stay one row whose forms are separated by `|`.
//! Notes: the comment block (`# ...`) heading the section of an entry and its context.

use std::path::Path;

use super::{ScannedString, TextStyle};
use crate::error::{Error, Result};
use crate::i18n::catalog::{RawLine, escape, split_line, unescape};

/// Row id of an entry (columns as written in the file).
pub fn row_id(context: &str, source: &str) -> String {
    match context.trim() {
        "" => source.to_string(),
        c => format!("{c}|{source}"),
    }
}

fn line_error(no: usize, message: &str) -> Error {
    Error::Format(format!("TSV line {no}: {message}"))
}

/// Entry lines with their 1-based line number.
fn entries(text: &str) -> Result<Vec<(usize, Option<RawLine<'_>>)>> {
    text.split('\n').enumerate().map(|(i, l)| split_line(l).map(|e| (i + 1, e)).map_err(|m| line_error(i + 1, &m))).collect()
}

fn original_text(no: usize, e: &RawLine<'_>) -> Result<String> {
    let s = unescape(e.source).map_err(|m| line_error(no, &m))?;
    if s.is_empty() {
        return Err(line_error(no, "empty source"));
    }
    Ok(s)
}

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let (text, _) = crate::xml::decode_text(bytes)?;
    let mut out = Vec::new();
    // Comment block heading the current section; a block followed by a blank line before any
    // entry (e.g. the file header) does not apply to the entries.
    let mut section: Vec<String> = Vec::new();
    let mut section_used = false;
    for (no, raw) in text.split('\n').enumerate().map(|(i, l)| (i + 1, l.strip_suffix('\r').unwrap_or(l))) {
        if let Some(c) = raw.strip_prefix('#') {
            if section_used {
                section.clear();
                section_used = false;
            }
            section.push(c.trim().to_string());
            continue;
        }
        let Some(e) = split_line(raw).map_err(|m| line_error(no, &m))? else {
            if !section_used {
                section.clear();
            }
            continue;
        };
        section_used = true;
        let mut notes: Vec<String> = section.iter().filter(|s| !s.is_empty()).cloned().collect();
        if !e.context.trim().is_empty() {
            notes.push(format!("Context: {}", e.context.trim()));
        }
        out.push(ScannedString { id: row_id(e.context, e.source), text: original_text(no, &e)?, comment: (!notes.is_empty()).then(|| notes.join("\n")) });
    }
    Ok(out)
}

/// Non-empty translations of the third column by row id (malformed lines are skipped).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let (text, _) = crate::xml::decode_text(bytes)?;
    Ok(text
        .split('\n')
        .filter_map(|l| split_line(l).ok().flatten())
        .filter_map(|e| unescape(e.translation).ok().filter(|t| !t.is_empty()).map(|t| (row_id(e.context, e.source), t)))
        .collect())
}

/// Builds a localized catalog: `resolve(row_id, original)` returns the translation to write,
/// `None` for an empty third column.
pub fn build(original: &[u8], resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let style = TextStyle::detect(original);
    let (text, _) = crate::xml::decode_text(original)?;
    let mut lines = Vec::new();
    for ((no, entry), raw) in entries(&text)?.into_iter().zip(text.split('\n')) {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match entry {
            None => lines.push(raw.to_string()),
            Some(e) => {
                let value = resolve(&row_id(e.context, e.source), &original_text(no, &e)?).unwrap_or_default();
                lines.push(format!("{}\t{}\t{}", e.context, e.source, escape(&value)));
            }
        }
    }
    let style = TextStyle { trailing_newline: false, ..style };
    Ok(style.apply(&lines.join("\n")))
}

/// Default localized file pattern: the language code in the file name is replaced by `<iso>`
/// (`de-core.tsv` → `<iso>-core.<ext>`, `app.en.tsv` → `app.<iso>.<ext>`), else `<iso>\<file>`.
pub fn default_pattern(file_name: &str) -> String {
    let file_name = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    let path = Path::new(file_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let seps: Vec<usize> = stem.char_indices().filter(|(_, c)| matches!(c, '-' | '_' | '.')).map(|(i, _)| i).collect();
    if crate::lang::is_known(stem) {
        return "<iso>.<ext>".into();
    }
    // Longest candidates first so that `pt_BR-core` is not read as `pt` + `_BR-core`.
    if let Some(&i) = seps.iter().rev().find(|&&i| crate::lang::is_known(&stem[..i])) {
        return format!("<iso>{}.<ext>", &stem[i..]);
    }
    if let Some(&i) = seps.iter().find(|&&i| crate::lang::is_known(&stem[i + 1..])) {
        return format!("{}<iso>.<ext>", &stem[..=i]);
    }
    "<iso>\\<file>".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = "# Header\n# second line\n\n# Menus\n\tOpen\tÖffnen\nmenu\tFile\t\n@plural\t{n} file|{n} files\t{n} Datei|{n} Dateien\n\tA\\nB\n# Other\n\tSave\tSpeichern\n";

    #[test]
    fn scans_ids_texts_and_notes() {
        let s = scan(CATALOG.as_bytes()).unwrap();
        let ids: Vec<&str> = s.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["Open", "menu|File", "@plural|{n} file|{n} files", "A\\nB", "Save"]);
        assert_eq!(s[3].text, "A\nB");
        assert_eq!(s[0].comment.as_deref(), Some("Menus"));
        assert_eq!(s[1].comment.as_deref(), Some("Menus\nContext: menu"));
        assert_eq!(s[4].comment.as_deref(), Some("Other"));
        assert!(scan(b"\tbad\tline\tx\n").is_err());
        assert!(scan(b"\t\tx\n").is_err());
    }

    #[test]
    fn reads_translations() {
        let t = translations(CATALOG.as_bytes()).unwrap();
        assert_eq!(
            t,
            [("Open".into(), "Öffnen".into()), ("@plural|{n} file|{n} files".into(), "{n} Datei|{n} Dateien".into()), ("Save".into(), "Speichern".into())]
        );
    }

    #[test]
    fn build_replaces_only_the_third_column() {
        let crlf = format!("\u{feff}{}", CATALOG.replace('\n', "\r\n"));
        let same = build(crlf.as_bytes(), &|id, _| translations(CATALOG.as_bytes()).unwrap().into_iter().find(|(k, _)| k == id).map(|(_, v)| v)).unwrap();
        let expected = crlf.replace("\tA\\nB\r\n", "\tA\\nB\t\r\n");
        assert_eq!(String::from_utf8(same).unwrap(), expected);
        let fr = build(CATALOG.as_bytes(), &|id, orig| (id != "Save").then(|| format!("{orig}\t!"))).unwrap();
        let fr = String::from_utf8(fr).unwrap();
        assert!(fr.starts_with("# Header\n# second line\n\n# Menus\n\tOpen\tOpen\\t!\nmenu\tFile\tFile\\t!\n"), "{fr}");
        assert!(fr.contains("\tA\\nB\tA\\nB\\t!\n# Other\n\tSave\t\n"), "{fr}");
        assert!(fr.ends_with('\n') && !fr.ends_with("\n\n"));
    }

    #[test]
    fn default_patterns() {
        assert_eq!(default_pattern("de-core.tsv"), "<iso>-core.<ext>");
        assert_eq!(default_pattern("i18n\\de-core.tsv"), "<iso>-core.<ext>");
        assert_eq!(default_pattern("pt_BR-core.tsv"), "<iso>-core.<ext>");
        assert_eq!(default_pattern("app.en.tsv"), "app.<iso>.<ext>");
        assert_eq!(default_pattern("strings_fr.tsv"), "strings_<iso>.<ext>");
        assert_eq!(default_pattern("en.tsv"), "<iso>.<ext>");
        assert_eq!(default_pattern("messages.tsv"), "<iso>\\<file>");
    }
}
