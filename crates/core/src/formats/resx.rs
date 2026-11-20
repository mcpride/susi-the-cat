//! .NET resource files (`.resx`) and their UWP/WinUI variant (`.resw`, same schema).
//!
//! Localizable items are `<data>` elements whose name does not start with `>>` (designer
//! metadata): strings (no `type`/`mimetype`), text file references (`ResXFileRef` to a
//! `System.String`, scanned as strings like Sisulizer does) and custom data, i.e. embedded byte
//! arrays (`bytearray.base64`: bitmaps, icons, `byte[]`) and file references to other files (see
//! [`crate::custom_data`]). Localized files are copies of the original holding the translated
//! items only: strings with their translation, text file references as strings and custom data
//! embedded as byte arrays (as Sisulizer builds them; Susi always sets the `mimetype`). `.resw`
//! files are localized in one directory per language (`Strings\en-US\Resources.resw` →
//! `Strings\de\Resources.resw`).

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::ScannedString;
use crate::custom_data::{Excluded, ScannedData};
use crate::error::{Error, Result};
use crate::xml::{self, Document, Element, Node};

const BYTE_ARRAY: &str = "application/x-microsoft.net.object.bytearray.base64";
const FILE_REF: &str = "System.Resources.ResXFileRef";

/// Kind of a `<data>` item.
enum Entry {
    String,
    /// `ResXFileRef` to a text file: path and encoding.
    TextRef {
        file: String,
        encoding: Option<String>,
    },
    /// Embedded byte array.
    Embedded,
    /// `ResXFileRef` to another file: path and the .NET type of its content.
    FileRef {
        file: String,
        type_name: String,
    },
    Other,
}

fn entry(e: &Element) -> Entry {
    if e.name != "data" || e.attr("name").is_none_or(|n| n.starts_with(">>")) {
        return Entry::Other;
    }
    if is_string_data(e) {
        return Entry::String;
    }
    if e.attr("mimetype") == Some(BYTE_ARRAY) {
        return Entry::Embedded;
    }
    if e.attr("mimetype").is_some() || !e.attr("type").is_some_and(|t| t.starts_with(FILE_REF)) {
        return Entry::Other;
    }
    // `path;type[;encoding]` – the type name contains commas but no semicolons.
    let value = e.child("value").map(Element::text).unwrap_or_default();
    let mut parts = value.splitn(3, ';').map(str::trim);
    let file = parts.next().unwrap_or_default().to_string();
    let type_name = parts.next().unwrap_or_default().to_string();
    let encoding = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
    match () {
        _ if file.is_empty() => Entry::Other,
        _ if type_name.starts_with("System.String") => Entry::TextRef { file, encoding },
        _ => Entry::FileRef { file, type_name },
    }
}

fn is_string_data(e: &Element) -> bool {
    if e.name != "data" {
        return false;
    }
    let Some(name) = e.attr("name") else {
        return false;
    };
    if name.starts_with(">>") || e.attr("mimetype").is_some() {
        return false;
    }
    match e.attr("type") {
        None => true,
        Some(t) => t.starts_with("System.String"),
    }
}

pub fn parse(bytes: &[u8]) -> Result<Document> {
    let doc = xml::parse_bytes(bytes)?;
    if doc.root.name != "root" {
        return Err(Error::Format(format!("not a .resx file (root element <{}>)", doc.root.name)));
    }
    Ok(doc)
}

fn scanned_string(e: &Element, text: String) -> ScannedString {
    let comment = e.child("comment").map(Element::text).filter(|c| !c.trim().is_empty());
    ScannedString { id: e.attr("name").unwrap_or("").to_string(), text, comment }
}

fn value(e: &Element) -> String {
    e.child("value").map(Element::text).unwrap_or_default()
}

/// The strings of a `.resx` (without file references and custom data; used for localized files).
pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let doc = parse(bytes)?;
    Ok(doc.root.elements().filter(|e| is_string_data(e)).map(|e| scanned_string(e, value(e))).collect())
}

/// Result of [`scan_all`].
#[derive(Debug, Default)]
pub struct Scan {
    /// Strings and text file references.
    pub strings: Vec<ScannedString>,
    /// Pictures and other binary data, embedded or from referenced files.
    pub data: Vec<ScannedData>,
    /// Referenced files that could not be read, invalid embedded data.
    pub errors: Vec<String>,
}

/// Scans an original `.resx`: strings, text file references (`xdtText` excludes them) and custom
/// data. Referenced files are relative to `dir`, the directory of the `.resx`.
pub fn scan_all(bytes: &[u8], dir: &Path, excluded: Excluded) -> Result<Scan> {
    let doc = parse(bytes)?;
    let mut out = Scan::default();
    for e in doc.root.elements() {
        let name = e.attr("name").unwrap_or("").to_string();
        let read = |file: &str| {
            let path = dir.join(crate::ops::native_path(file));
            std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))
        };
        match entry(e) {
            Entry::String => out.strings.push(scanned_string(e, value(e))),
            Entry::TextRef { file, encoding } if !excluded.text => match read(&file) {
                Ok(b) => out.strings.push(scanned_string(e, decode_text_file(&b, encoding.as_deref()))),
                Err(err) => out.errors.push(err),
            },
            Entry::Embedded => match BASE64.decode(value(e).bytes().filter(|b| !b.is_ascii_whitespace()).collect::<Vec<u8>>()) {
                Ok(bytes) => out.data.push(ScannedData { name, bytes }),
                Err(_) => out.errors.push(crate::tf!("{name}: invalid base64 data", name = name)),
            },
            Entry::FileRef { file, .. } => match read(&file) {
                Ok(bytes) => out.data.push(ScannedData { name, bytes }),
                Err(err) => out.errors.push(err),
            },
            _ => {}
        }
    }
    Ok(out)
}

/// Text of a referenced text file: BOM first, then the encoding named in the reference
/// (UTF-8 by default; unknown code pages are read as UTF-8 if valid, otherwise as Latin-1).
fn decode_text_file(b: &[u8], encoding: Option<&str>) -> String {
    let utf16 = |b: &[u8], be: bool| {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|&c| if be { u16::from_be_bytes(c) } else { u16::from_le_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, false);
    }
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, true);
    }
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        Some("utf-16" | "utf-16le" | "unicode") => utf16(b, false),
        Some("utf-16be" | "unicodefffe" | "bigendianunicode") => utf16(b, true),
        None | Some("utf-8" | "utf8") => String::from_utf8_lossy(b).into_owned(),
        Some(_) => std::str::from_utf8(b).map(str::to_string).unwrap_or_else(|_| b.iter().map(|&c| char::from(c)).collect()),
    }
}

/// Values of a localized `.resx`; `None` omits an item.
pub struct Values<'a> {
    /// String value of `(name, original)`.
    pub text: &'a dyn Fn(&str, &str) -> Option<String>,
    /// Text of a text file reference (written as a string).
    pub text_ref: &'a dyn Fn(&str) -> Option<String>,
    /// Bytes of custom data (written embedded).
    pub data: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Keep untranslated embedded data (write method *All*); file references are always dropped,
    /// their paths are relative to the original.
    pub keep_data: bool,
}

fn set_value(e: &mut Element, text: &str) {
    match e.child_mut("value") {
        Some(v) => v.set_text(text),
        None => {
            let mut v = Element::new("value");
            v.set_text(text);
            e.children.push(Node::Element(v));
        }
    }
}

/// Builds a localized `.resx`.
pub fn build(original: &[u8], v: &Values<'_>) -> Result<Vec<u8>> {
    let mut doc = parse(original)?;
    let old = std::mem::take(&mut doc.root.children);
    let mut out: Vec<Node> = Vec::with_capacity(old.len());
    for node in old {
        let Node::Element(mut e) = node else {
            out.push(node);
            continue;
        };
        if e.name != "data" && e.name != "metadata" {
            out.push(Node::Element(e));
            continue;
        }
        let name = e.attr("name").unwrap_or("").to_string();
        let keep = match entry(&e) {
            Entry::String => (v.text)(&name, &value(&e)).map(|text| set_value(&mut e, &text)).is_some(),
            Entry::TextRef { .. } => match (v.text_ref)(&name) {
                Some(text) => {
                    e.attrs.remove("type");
                    e.attrs.set("xml:space", "preserve");
                    set_value(&mut e, &text);
                    true
                }
                None => false,
            },
            Entry::Embedded => match (v.data)(&name) {
                Some(bytes) => {
                    set_value(&mut e, &BASE64.encode(bytes));
                    true
                }
                None => v.keep_data,
            },
            Entry::FileRef { type_name, .. } => match (v.data)(&name) {
                Some(bytes) => {
                    e.attrs.set("type", type_name);
                    e.attrs.set("mimetype", BYTE_ARRAY);
                    set_value(&mut e, &BASE64.encode(bytes));
                    true
                }
                None => false,
            },
            Entry::Other => false,
        };
        if keep {
            out.push(Node::Element(e));
        } else {
            drop_trailing_ws(&mut out);
        }
    }
    doc.root.children = out;
    Ok(xml::write_document(&doc))
}

/// Translations of an existing localized file (taken over when the source is added).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(scan(bytes)?.into_iter().map(|s| (s.id, s.text)).collect())
}

/// Whether a source name denotes a UWP/WinUI resource file.
pub fn is_resw(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".resw")
}

/// Default pattern of a `.resw`: a sibling language directory when the original lies in one
/// (`Strings\en-US\Resources.resw` → `<parent>\<net>\<file>`), otherwise a language directory next
/// to it (`<net>\<file>`).
pub fn resw_default_pattern(file_name: &str) -> String {
    let mut parts = file_name.rsplit(['\\', '/']);
    parts.next();
    match parts.next() {
        Some(dir) if crate::lang::is_known(dir) => "<parent>\\<net>\\<file>".into(),
        _ => "<net>\\<file>".into(),
    }
}

/// Removes the indentation text node in front of a dropped element.
fn drop_trailing_ws(out: &mut Vec<Node>) {
    if let Some(Node::Text(t)) = out.last()
        && t.trim().is_empty()
    {
        out.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<root>\r\n  <resheader name=\"resmimetype\">\r\n    <value>text/microsoft-resx</value>\r\n  </resheader>\r\n  <data name=\"Hello\" xml:space=\"preserve\">\r\n    <value>Hello &amp; welcome</value>\r\n    <comment>Greeting</comment>\r\n  </data>\r\n  <data name=\"Icon\" type=\"System.Drawing.Icon, System.Drawing\" mimetype=\"application/x-microsoft.net.object.bytearray.base64\">\r\n    <value>AAAA</value>\r\n  </data>\r\n  <data name=\"&gt;&gt;label1.Name\" xml:space=\"preserve\">\r\n    <value>label1</value>\r\n  </data>\r\n  <data name=\"Bye\" xml:space=\"preserve\">\r\n    <value>Bye</value>\r\n  </data>\r\n</root>";

    #[test]
    fn scan_strings_only() {
        let s = scan(SRC.as_bytes()).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, "Hello");
        assert_eq!(s[0].text, "Hello & welcome");
        assert_eq!(s[0].comment.as_deref(), Some("Greeting"));
    }

    fn strings<'a>(text: &'a dyn Fn(&str, &str) -> Option<String>) -> Values<'a> {
        Values { text, text_ref: &|_| None, data: &|_| None, keep_data: false }
    }

    #[test]
    fn build_translated_only() {
        let out = build(SRC.as_bytes(), &strings(&|n, _| (n == "Hello").then(|| "Hallo & willkommen".to_string()))).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("<value>Hallo &amp; willkommen</value>"), "{s}");
        assert!(!s.contains("Icon"));
        assert!(!s.contains("Bye"));
        assert!(s.contains("text/microsoft-resx"));
        assert!(s.contains("\r\n"));
        assert_eq!(scan(s.as_bytes()).unwrap().len(), 1);
        assert_eq!(translations(s.as_bytes()).unwrap(), [("Hello".to_string(), "Hallo & willkommen".to_string())]);
    }

    #[test]
    fn resw_patterns() {
        assert!(is_resw("Strings\\en-US\\Resources.resw") && !is_resw("Strings.resx"));
        assert_eq!(resw_default_pattern("Strings\\en-US\\Resources.resw"), "<parent>\\<net>\\<file>");
        assert_eq!(resw_default_pattern("App/Strings/en/Resources.resw"), "<parent>\\<net>\\<file>");
        assert_eq!(resw_default_pattern("Strings\\Resources.resw"), "<net>\\<file>");
        assert_eq!(resw_default_pattern("Resources.resw"), "<net>\\<file>");
    }
}
