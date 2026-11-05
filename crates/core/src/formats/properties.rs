//! Java resource bundles (`.properties`), read as `java.util.Properties.load` does.
//!
//! Sisulizer stores them as `TJavaResourceSource` (the same class also covers `.java`
//! ListResourceBundles, which susi does not read). Localized files follow the Java naming scheme
//! `<body>_<iso>.<ext>` (`Messages.properties` → `Messages_de.properties`, `Messages_pt_BR.properties`).
//!
//! - Syntax: `key=value`, `key: value` or `key value`; a line ending in an odd number of
//!   backslashes continues on the next line; comment lines start with `#` or `!`; escapes `\t`,
//!   `\n`, `\r`, `\f`, `\uXXXX`, and `\x` for any other `x`.
//! - Row ids are the (unescaped) keys. Entries with an empty key or value are not scanned; the
//!   last of duplicate keys wins, as in Java.
//! - Notes: the comment lines directly above an entry (lines without letters or digits, such as
//!   `#-----`, are skipped).
//! - Building copies the original and replaces the values; comments, blank lines, order,
//!   indentation, separators and line endings are kept. Omitted entries (write method) are removed
//!   together with the comment lines directly above them; Java then falls back to the parent bundle.
//! - Encoding ([`PropertiesEncoding`]): Java up to 8 reads ISO-8859-1, Java 9+ UTF-8 (falling back
//!   to ISO-8859-1). By default localized files keep the style of the original: raw UTF-8 when the
//!   original contains UTF-8 text, otherwise ASCII with `\uXXXX` escapes, which every Java version reads.

use std::collections::HashMap;
use std::path::Path;

use super::{ScannedString, TextStyle};
use crate::error::{Error, Result};
use crate::xml::Attrs;

/// Encoding of localized `.properties` files, stored in the Sisulizer source attribute `encoding`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PropertiesEncoding {
    /// Like the original (no `encoding` attribute).
    #[default]
    Auto,
    /// UTF-8, non-ASCII characters written as they are (Java 9+).
    Utf8,
    /// ISO-8859-1 / Windows-1252: non-ASCII characters in keys and values as `\uXXXX`.
    Escaped,
}

impl PropertiesEncoding {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Utf8, Self::Escaped];

    pub fn from_attrs(a: &Attrs) -> Self {
        match a.get("encoding").map(str::trim) {
            None | Some("") => Self::Auto,
            Some(v) if v.eq_ignore_ascii_case("utf-8") || v.eq_ignore_ascii_case("utf8") => Self::Utf8,
            // Sisulizer writes `windows-1252`.
            Some(_) => Self::Escaped,
        }
    }

    pub fn store(self, a: &mut Attrs) {
        match self {
            Self::Auto => {
                a.remove("encoding");
            }
            Self::Utf8 => a.set("encoding", "utf-8"),
            Self::Escaped => a.set("encoding", "iso-8859-1"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => crate::i18n::t("Automatic (like the original)"),
            Self::Utf8 => "UTF-8",
            Self::Escaped => crate::i18n::t("ISO-8859-1 with Unicode escapes"),
        }
    }
}

/// A logical line of a properties file.
enum Logical<'a> {
    Blank(&'a str),
    /// A comment line as written, and its text after `#`/`!`.
    Comment(&'a str, &'a str),
    Entry(Entry<'a>),
}

struct Entry<'a> {
    indent: &'a str,
    /// Key and separator as written.
    key_raw: String,
    sep: String,
    key: String,
    value: String,
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\x0C')
}

fn odd_backslashes(s: &str) -> bool {
    s.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

fn line_error(no: usize, message: &str) -> Error {
    Error::Format(format!("properties line {no}: {message}"))
}

/// Natural lines; `\r\n`, `\r` and `\n` all end a line, as in Java.
fn natural_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n").split(['\n', '\r']).map(str::to_string).collect()
}

fn parse(lines: &[String]) -> Result<Vec<Logical<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(line) = lines.get(i) {
        let trimmed = line.trim_start_matches(is_ws);
        if trimmed.is_empty() {
            out.push(Logical::Blank(line));
        } else if let Some(text) = trimmed.strip_prefix(['#', '!']) {
            out.push(Logical::Comment(line, text));
        } else {
            let first = i;
            let mut logical = trimmed.to_string();
            while odd_backslashes(&logical) {
                logical.pop();
                // A continuation at the end of the file is dropped.
                let Some(next) = lines.get(i + 1) else { break };
                i += 1;
                logical.push_str(next.trim_start_matches(is_ws));
            }
            let entry = split_entry(&logical).map_err(|m| line_error(first + 1, &m))?;
            out.push(Logical::Entry(Entry { indent: &line[..line.len() - trimmed.len()], ..entry }));
        }
        i += 1;
    }
    Ok(out)
}

/// Splits a logical line into key, separator and value.
fn split_entry(logical: &str) -> std::result::Result<Entry<'static>, String> {
    let mut key_end = logical.len();
    let mut escaped = false;
    for (i, c) in logical.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '=' || c == ':' || is_ws(c) {
            key_end = i;
            break;
        }
    }
    let (key_raw, rest) = logical.split_at(key_end);
    let after_ws = rest.trim_start_matches(is_ws);
    let value_raw = after_ws.strip_prefix(['=', ':']).unwrap_or(after_ws).trim_start_matches(is_ws);
    let sep = &rest[..rest.len() - value_raw.len()];
    Ok(Entry { indent: "", key_raw: key_raw.to_string(), sep: sep.to_string(), key: unescape(key_raw)?, value: unescape(value_raw)? })
}

fn unescape(s: &str) -> std::result::Result<String, String> {
    fn flush(out: &mut String, units: &mut Vec<u16>) {
        out.extend(char::decode_utf16(units.drain(..)).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)));
    }
    let mut out = String::with_capacity(s.len());
    let mut units = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            flush(&mut out, &mut units);
            out.push(c);
            continue;
        }
        let Some(e) = chars.next() else { break };
        if e == 'u' {
            let hex: String = chars.by_ref().take(4).collect();
            if hex.len() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                return Err(format!("malformed \\uXXXX escape: \\u{hex}"));
            }
            units.push(u16::from_str_radix(&hex, 16).map_err(|e| e.to_string())?);
            continue;
        }
        flush(&mut out, &mut units);
        out.push(match e {
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            'f' => '\x0C',
            other => other,
        });
    }
    flush(&mut out, &mut units);
    Ok(out)
}

fn push_unicode(out: &mut String, c: char) {
    let mut buf = [0u16; 2];
    for u in c.encode_utf16(&mut buf) {
        out.push_str(&format!("\\u{u:04X}"));
    }
}

/// Escapes a value for writing; `ascii` writes non-ASCII characters as `\uXXXX`.
fn escape_value(s: &str, ascii: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0C' => out.push_str("\\f"),
            // Leading blanks would be taken for the separator.
            ' ' if i == 0 => out.push_str("\\ "),
            c if c.is_control() || (ascii && !c.is_ascii()) => push_unicode(&mut out, c),
            c => out.push(c),
        }
    }
    out
}

/// Writes the non-ASCII characters of an already escaped text as `\uXXXX`.
fn ascii_only(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() { out.push(c) } else { push_unicode(&mut out, c) }
    }
    out
}

/// Comment lines worth a note (not just separators such as `#-----`).
fn note_line(text: &str) -> Option<&str> {
    let t = text.trim();
    t.chars().any(char::is_alphanumeric).then_some(t)
}

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let (text, _) = crate::xml::decode_text(bytes)?;
    let lines = natural_lines(&text);
    let mut out: Vec<ScannedString> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut comments: Vec<&str> = Vec::new();
    for logical in parse(&lines)? {
        let e = match logical {
            Logical::Blank(_) => {
                comments.clear();
                continue;
            }
            Logical::Comment(_, text) => {
                comments.extend(note_line(text));
                continue;
            }
            Logical::Entry(e) => e,
        };
        let note = std::mem::take(&mut comments).join("\n");
        if e.key.is_empty() || e.value.is_empty() {
            continue;
        }
        let s = ScannedString { id: e.key, text: e.value, comment: (!note.is_empty()).then_some(note) };
        match index.get(&s.id).and_then(|&i| out.get_mut(i)) {
            Some(slot) => {
                let comment = s.comment.or_else(|| slot.comment.take());
                *slot = ScannedString { comment, ..s };
            }
            None => {
                index.insert(s.id.clone(), out.len());
                out.push(s);
            }
        }
    }
    Ok(out)
}

/// Non-empty values by key (used to import existing localized files).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(scan(bytes)?.into_iter().map(|s| (s.id, s.text)).collect())
}

/// Builds a localized file: `resolve(key, original)` returns the value to write or `None` to omit
/// the entry.
pub fn build(original: &[u8], encoding: PropertiesEncoding, resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let style = TextStyle::detect(original);
    let (text, _) = crate::xml::decode_text(original)?;
    let utf8 = std::str::from_utf8(original).is_ok();
    let raw = match encoding {
        PropertiesEncoding::Auto => utf8 && !original.is_ascii(),
        PropertiesEncoding::Utf8 => true,
        PropertiesEncoding::Escaped => false,
    };
    let lines = natural_lines(&text);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    // Comment lines at the end of `out` that belong to the next entry.
    let mut attached = 0;
    for logical in parse(&lines)? {
        match logical {
            Logical::Blank(line) => {
                out.push(line.to_string());
                attached = 0;
            }
            Logical::Comment(line, _) => {
                out.push(line.to_string());
                attached += 1;
            }
            Logical::Entry(e) => {
                match resolve(&e.key, &e.value) {
                    Some(value) => {
                        let key = if raw { e.key_raw } else { ascii_only(&e.key_raw) };
                        let sep = if e.sep.is_empty() && !value.is_empty() { "=".to_string() } else { e.sep };
                        out.push(format!("{}{key}{sep}{}", e.indent, escape_value(&value, !raw)));
                    }
                    None => out.truncate(out.len().saturating_sub(attached)),
                }
                attached = 0;
            }
        }
    }
    // A BOM would become part of the first key for ISO-8859-1 readers.
    let style = TextStyle { trailing_newline: false, bom: style.bom && raw, ..style };
    let bytes = style.apply(&out.join("\n"));
    if utf8 || raw {
        return Ok(bytes);
    }
    // Keep an ISO-8859-1 original (comments) in ISO-8859-1.
    Ok(String::from_utf8_lossy(&bytes).chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect())
}

/// Default localized file pattern (Java bundle names): a trailing locale in the file name is
/// replaced (`Messages_en.properties` → `Messages_<iso>.<ext>`), otherwise `<body>_<iso>.<ext>`.
pub fn default_pattern(file_name: &str) -> String {
    let file_name = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    let stem = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.match_indices('_')
        .map(|(i, _)| i)
        .find(|&i| crate::lang::is_known(&stem[i + 1..]))
        .map_or_else(|| "<body>_<iso>.<ext>".to_string(), |i| format!("{}_<iso>.<ext>", &stem[..i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "# Converter bundle\n\n#-------\n# Menu\nmenu.file=File\n  menu.exit = Exit\nabout\\ box : About \\\n    Converter...\nkey\\=with\\:colon\tValue\nempty=\nnovalue\nunicode=Caf\\u00e9 \\uD83D\\uDE00\n! last\ndup=first\ndup=second\n";

    #[test]
    fn scans_keys_values_and_notes() {
        let s = scan(SRC.as_bytes()).unwrap();
        let pairs: Vec<(&str, &str)> = s.iter().map(|x| (x.id.as_str(), x.text.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("menu.file", "File"),
                ("menu.exit", "Exit"),
                ("about box", "About Converter..."),
                ("key=with:colon", "Value"),
                ("unicode", "Café 😀"),
                ("dup", "second"),
            ]
        );
        assert_eq!(s[0].comment.as_deref(), Some("Menu"));
        assert_eq!(s[1].comment, None);
        assert_eq!(s[5].comment.as_deref(), Some("last"));
        assert!(scan(b"bad=\\u00G1\n").is_err());
        assert_eq!(scan(b"a=b\\").unwrap()[0].text, "b");
        assert_eq!(scan(b"a=x\\\\\nb=y\r\nc=z\rd").unwrap().len(), 3);
    }

    #[test]
    fn build_keeps_layout_and_escapes() {
        let crlf = SRC.replace('\n', "\r\n");
        let out = build(crlf.as_bytes(), PropertiesEncoding::Auto, &|k, orig| match k {
            "menu.file" => Some("Datei".into()),
            "menu.exit" => None,
            "about box" => Some(" Über\nKonverter".into()),
            "unicode" => Some("Kaffee ☕".into()),
            _ => Some(orig.to_string()),
        })
        .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("# Converter bundle\r\n\r\n#-------\r\n# Menu\r\nmenu.file=Datei\r\nabout\\ box : \\ \\u00DCber\\nKonverter\r\n"), "{out}");
        assert!(out.contains("key\\=with\\:colon\tValue\r\nempty=\r\nnovalue\r\nunicode=Kaffee \\u2615\r\n! last\r\ndup=first\r\ndup=second\r\n"), "{out}");
        assert!(out.ends_with("dup=second\r\n"));
        // Omitted entries take their comment lines with them; a value for a key without separator gets one.
        let out = build(b"# a\nA=x\n\n# b\nB\n", PropertiesEncoding::Auto, &|k, _| (k == "B").then(|| "y".into())).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "\n# b\nB=y\n");
    }

    #[test]
    fn build_encodings() {
        let resolve = |_: &str, _: &str| Some("Grüße".to_string());
        let utf8 = "\u{feff}# Größe\nk=Größe\n";
        assert_eq!(build(utf8.as_bytes(), PropertiesEncoding::Auto, &resolve).unwrap(), utf8.replace("=Größe", "=Grüße").as_bytes());
        assert_eq!(build(utf8.as_bytes(), PropertiesEncoding::Escaped, &resolve).unwrap(), "# Größe\nk=Gr\\u00FC\\u00DFe\n".as_bytes());
        assert_eq!(build(b"k=x\n", PropertiesEncoding::Utf8, &resolve).unwrap(), "k=Grüße\n".as_bytes());
        // ISO-8859-1 original: comments stay ISO-8859-1, keys and values become ASCII.
        let latin1 = b"# Gr\xF6\xDFe\nk\xE9=x\n";
        assert_eq!(scan(latin1).unwrap()[0].id, "ké");
        assert_eq!(build(latin1, PropertiesEncoding::Auto, &resolve).unwrap(), b"# Gr\xF6\xDFe\nk\\u00E9=Gr\\u00FC\\u00DFe\n");
    }

    #[test]
    fn encoding_attribute() {
        let mut a = Attrs::new();
        assert_eq!(PropertiesEncoding::from_attrs(&a), PropertiesEncoding::Auto);
        a.set("encoding", "windows-1252");
        assert_eq!(PropertiesEncoding::from_attrs(&a), PropertiesEncoding::Escaped);
        PropertiesEncoding::Utf8.store(&mut a);
        assert_eq!(PropertiesEncoding::from_attrs(&a), PropertiesEncoding::Utf8);
        PropertiesEncoding::Auto.store(&mut a);
        assert_eq!(a.get("encoding"), None);
    }

    #[test]
    fn default_patterns() {
        assert_eq!(default_pattern("Messages.properties"), "<body>_<iso>.<ext>");
        assert_eq!(default_pattern("res\\Messages_en.properties"), "Messages_<iso>.<ext>");
        assert_eq!(default_pattern("app_ui_pt_BR.properties"), "app_ui_<iso>.<ext>");
        assert_eq!(default_pattern("*.properties"), "<body>_<iso>.<ext>");
    }
}
