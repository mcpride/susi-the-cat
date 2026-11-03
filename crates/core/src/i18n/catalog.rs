//! Parser for the `.tsv` translation catalogs.
//!
//! Each non-empty, non-comment line is `context<TAB>source<TAB>translation`:
//! * empty context: plain entry, looked up by [`super::t`] / [`crate::tf!`];
//! * `@plural`: source `one|other`, translation forms separated by `|` ([`super::trn`]);
//! * anything else: disambiguating context for [`super::tr_ctx`].
//!
//! Escapes in source and translation: `\n`, `\t`, `\r`, `\\`. Lines starting with `#` are comments.
//! An empty (or missing) translation column means "not translated yet": the entry is skipped and
//! the English text is shown. Susi itself can maintain these files (TSV sources, `formats::tsv`).

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub line: usize,
    pub context: String,
    pub source: String,
    pub translation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

pub fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(o) => return Err(format!("unknown escape '\\{o}'")),
            None => return Err("trailing backslash".into()),
        }
    }
    Ok(out)
}

/// Inverse of [`unescape`].
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Raw (still escaped) columns of an entry line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawLine<'a> {
    pub context: &'a str,
    pub source: &'a str,
    pub translation: &'a str,
}

/// Splits one line: `Ok(None)` for comments and blank lines; the translation column may be missing.
pub fn split_line(raw: &str) -> Result<Option<RawLine<'_>>, String> {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    if raw.trim().is_empty() || raw.starts_with('#') {
        return Ok(None);
    }
    let parts: Vec<&str> = raw.split('\t').collect();
    match parts[..] {
        [context, source] => Ok(Some(RawLine { context, source, translation: "" })),
        [context, source, translation] => Ok(Some(RawLine { context, source, translation })),
        _ => Err(format!("expected 3 tab-separated columns, found {}", parts.len())),
    }
}

/// Parses catalog text into translated entries; malformed lines are reported and skipped,
/// untranslated entries (empty translation) are skipped silently.
pub fn parse_entries(text: &str) -> (Vec<Entry>, Vec<ParseError>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let cols = match split_line(raw) {
            Ok(Some(c)) => c,
            Ok(None) => continue,
            Err(message) => {
                errors.push(ParseError { line, message });
                continue;
            }
        };
        let (source, translation) = match (unescape(cols.source), unescape(cols.translation)) {
            (Ok(s), Ok(t)) => (s, t),
            (Err(e), _) | (_, Err(e)) => {
                errors.push(ParseError { line, message: e });
                continue;
            }
        };
        if source.is_empty() {
            errors.push(ParseError { line, message: "empty source".into() });
            continue;
        }
        if translation.is_empty() {
            continue;
        }
        entries.push(Entry { line, context: cols.context.trim().to_string(), source, translation });
    }
    (entries, errors)
}

/// Lookup tables of one language.
#[derive(Debug, Default)]
pub struct Catalog {
    plain: HashMap<String, String>,
    contextual: HashMap<String, HashMap<String, String>>,
    plurals: HashMap<String, Vec<String>>,
}

impl Catalog {
    pub fn from_sources(sources: &[&str]) -> Self {
        let mut c = Catalog::default();
        for text in sources {
            for e in parse_entries(text).0 {
                c.insert(e);
            }
        }
        c
    }

    fn insert(&mut self, e: Entry) {
        match e.context.as_str() {
            "" => {
                self.plain.insert(e.source, e.translation);
            }
            "@plural" => {
                self.plurals.insert(e.source, e.translation.split('|').map(str::to_string).collect());
            }
            _ => {
                self.contextual.entry(e.context).or_default().insert(e.source, e.translation);
            }
        }
    }

    pub fn get(&self, source: &str) -> Option<&str> {
        self.plain.get(source).map(String::as_str)
    }

    pub fn get_ctx(&self, context: &str, source: &str) -> Option<&str> {
        self.contextual.get(context).and_then(|m| m.get(source)).map(String::as_str)
    }

    /// Plural forms for the key `one|other`.
    pub fn plural(&self, key: &str) -> Option<&[String]> {
        self.plurals.get(key).map(Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.plain.len() + self.plurals.len() + self.contextual.values().map(HashMap::len).sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `{name}` placeholders of a template, sorted (for consistency checks).
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('{') {
        let after = &rest[a + 1..];
        match after.find('}') {
            Some(b) if b > 0 && after[..b].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push(after[..b].to_string());
                rest = &after[b + 1..];
            }
            _ => rest = after,
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_escapes() {
        let (e, err) = parse_entries(
            "# comment\n\n\tOpen\tÖffnen\n@plural\t{n} file|{n} files\t{n} Datei|{n} Dateien\nmenu\tFile\tDatei\n\tA\\nB\tC\\tD\nbad line\n\tTodo\t\n\tTodo2\n",
        );
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].line, 7);
        assert_eq!(e.len(), 4);
        assert_eq!(escape("a\\b\nc\td\re"), "a\\\\b\\nc\\td\\re");
        assert_eq!(unescape(&escape("a\\b\nc\td\re")).as_deref(), Ok("a\\b\nc\td\re"));
        assert_eq!(e[3].source, "A\nB");
        assert_eq!(e[3].translation, "C\tD");
        let c = Catalog::from_sources(&["\tOpen\tÖffnen\n@plural\t{n} file|{n} files\t{n} Datei|{n} Dateien\nmenu\tFile\tDatei\n"]);
        assert_eq!(c.get("Open"), Some("Öffnen"));
        assert_eq!(c.get_ctx("menu", "File"), Some("Datei"));
        assert_eq!(c.get("File"), None);
        assert_eq!(c.plural("{n} file|{n} files").map(<[String]>::len), Some(2));
    }

    #[test]
    fn finds_placeholders() {
        assert_eq!(placeholders("{b} of {a} {} {x y} {0}"), vec!["0", "a", "b"]);
    }
}
