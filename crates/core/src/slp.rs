//! Reading and writing Susi/Sisulizer project files (`.slp`).
//!
//! Format (observed in Sisulizer 4.0.3xx files): UTF-8 with BOM, CRLF line endings, two-space
//! indentation, `<?xml version="1.0" encoding="utf-8"?>`, empty elements written as `<x/>`,
//! `& < > "` as entities and no-break spaces as decimal references.
//!
//! Strings of rows and translations use Sisulizer's own line break coding: `#c` = CR, `#l` = LF,
//! `##` = `#` (observed: `Line one#c#lLine two`, `JPEG error ##%d`); older files also use `#n`
//! for a line break. Other `#x` sequences are kept literally.

use std::fmt::Write as _;
use std::path::Path;

use crate::error::{Error, Result, read_file, write_file};
use crate::model::{Item, Language, Node, Project, Row, Source, Translation};
use crate::xml::{self, Attrs, Element};

/// Element names that are always treated as row containers.
const CONTAINER_TAGS: &[&str] = &["node"];

pub fn load(path: &Path) -> Result<Project> {
    let bytes = read_file(path)?;
    let mut p = parse(&bytes)?;
    p.path = Some(path.to_path_buf());
    Ok(p)
}

pub fn parse(bytes: &[u8]) -> Result<Project> {
    let doc = xml::parse_bytes(bytes)?;
    if doc.root.name != "document" {
        return Err(Error::Format(format!("not a Susi/Sisulizer project (root element <{}>)", doc.root.name)));
    }
    let mut p = Project { attrs: doc.root.attrs.clone(), decl: doc.decl.clone(), ..Default::default() };
    for el in doc.root.elements() {
        match el.name.as_str() {
            "lang" => p.languages.push(Language { attrs: el.attrs.clone(), children: el.elements().cloned().collect() }),
            "source" => p.sources.push(Source { attrs: el.attrs.clone(), items: parse_items(el, 0)? }),
            _ if p.sources.is_empty() => p.pre.push(el.clone()),
            _ => p.post.push(el.clone()),
        }
    }
    Ok(p)
}

fn parse_items(el: &Element, depth: usize) -> Result<Vec<Item>> {
    if depth > 256 {
        return Err(Error::Format("project nesting too deep".into()));
    }
    let mut items = Vec::new();
    for c in el.elements() {
        if c.name == "row" {
            items.push(Item::Row(parse_row(c)));
        } else if is_container(c) {
            items.push(Item::Node(Node { tag: c.name.clone(), attrs: c.attrs.clone(), items: parse_items(c, depth + 1)? }));
        } else {
            items.push(Item::Other(c.clone()));
        }
    }
    Ok(items)
}

/// A child element is a row container if it is a `<node>` or contains rows somewhere below and has
/// no own text content (so nothing is lost by re-serialising it structurally).
fn is_container(e: &Element) -> bool {
    if !e.text().trim().is_empty() {
        return false;
    }
    CONTAINER_TAGS.contains(&e.name.as_str()) || contains_row(e, 0)
}

fn contains_row(e: &Element, depth: usize) -> bool {
    depth < 256 && e.elements().any(|c| c.name == "row" || contains_row(c, depth + 1))
}

fn parse_row(el: &Element) -> Row {
    let mut row = Row { attrs: decode_attrs(&el.attrs), ..Default::default() };
    let mut has_native_el = false;
    for c in el.elements() {
        match c.name.as_str() {
            "native" => {
                has_native_el = true;
                row.native = Some(c.text());
            }
            "lang" => row.translations.push(Translation { attrs: decode_attrs(&c.attrs), text: c.text() }),
            _ if !has_native_el && row.translations.is_empty() => row.pre_native.push(c.clone()),
            _ => row.extra.push(c.clone()),
        }
    }
    if !has_native_el && !el.has_element_children() {
        let t = el.text();
        if !t.is_empty() {
            row.native = Some(t);
            row.inline_native = true;
        }
    }
    row.raw = row.native.as_deref().is_some_and(|n| n.contains('\n')) || row.translations.iter().any(|t| t.text.contains('\n'));
    if !row.raw {
        if let Some(n) = row.native.as_mut() {
            *n = decode(n);
        }
        for t in &mut row.translations {
            t.text = decode(&t.text);
        }
    }
    row
}

fn decode_attrs(a: &Attrs) -> Attrs {
    Attrs(a.iter().map(|(k, v)| (k.to_string(), decode(v))).collect())
}

fn encoded_attrs(a: &Attrs) -> Attrs {
    Attrs(a.iter().map(|(k, v)| (k.to_string(), encode(v))).collect())
}

/// Decodes Sisulizer's `#c` / `#l` / `##` coding.
pub fn decode(s: &str) -> String {
    if !s.contains('#') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '#' {
            out.push(c);
            continue;
        }
        match it.peek() {
            Some('#') => out.push('#'),
            Some('c') => out.push('\r'),
            Some('l') => out.push('\n'),
            // Older projects (2011) use `#n` for a line break.
            Some('n') => out.push_str("\r\n"),
            _ => {
                out.push('#');
                continue;
            }
        }
        it.next();
    }
    out
}

/// Encodes a string with Sisulizer's `#c` / `#l` / `##` coding.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '#' => out.push_str("##"),
            '\r' => out.push_str("#c"),
            '\n' => out.push_str("#l"),
            _ => out.push(c),
        }
    }
    out
}

fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\u{A0}' => out.push_str("&#160;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn escape_text(s: &str) -> String {
    escape(s, false)
}

fn escape_attr(s: &str) -> String {
    escape(s, true)
}

pub fn save(project: &mut Project, path: &Path) -> Result<()> {
    project.touch();
    let bytes = to_bytes(project);
    // Keeps the previous version as *.~slp.
    if path.exists() {
        let backup = path.with_extension("~slp");
        let _ = std::fs::copy(path, backup);
    }
    write_file(path, &bytes)?;
    project.path = Some(path.to_path_buf());
    Ok(())
}

pub fn to_bytes(p: &Project) -> Vec<u8> {
    let mut s = String::with_capacity(64 * 1024);
    let _ = writeln!(s, "<?{}?>", p.decl.as_deref().unwrap_or("xml version=\"1.0\" encoding=\"utf-8\""));
    open_tag(&mut s, 0, "document", &p.attrs);
    s.push_str(">\n");
    for l in &p.languages {
        if l.children.is_empty() {
            open_tag(&mut s, 1, "lang", &l.attrs);
            s.push_str("/>\n");
        } else {
            open_tag(&mut s, 1, "lang", &l.attrs);
            s.push_str(">\n");
            for c in &l.children {
                write_generic(&mut s, 2, c);
            }
            close_tag(&mut s, 1, "lang");
        }
    }
    for e in &p.pre {
        write_generic(&mut s, 1, e);
    }
    for src in &p.sources {
        open_tag(&mut s, 1, "source", &src.attrs);
        s.push_str(">\n");
        write_items(&mut s, 2, &src.items);
        close_tag(&mut s, 1, "source");
    }
    for e in &p.post {
        write_generic(&mut s, 1, e);
    }
    s.push_str("</document>\n");
    let s = s.replace('\n', "\r\n");
    let mut out = Vec::with_capacity(s.len() + 3);
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    out.extend_from_slice(s.as_bytes());
    out
}

fn indent(s: &mut String, level: usize) {
    for _ in 0..level {
        s.push_str("  ");
    }
}

fn open_tag(s: &mut String, level: usize, name: &str, attrs: &Attrs) {
    indent(s, level);
    s.push('<');
    s.push_str(name);
    for (k, v) in attrs.iter() {
        let _ = write!(s, " {k}=\"{}\"", escape_attr(v));
    }
}

fn close_tag(s: &mut String, level: usize, name: &str) {
    indent(s, level);
    let _ = writeln!(s, "</{name}>");
}

fn write_items(s: &mut String, level: usize, items: &[Item]) {
    for item in items {
        match item {
            Item::Row(r) => write_row(s, level, r),
            Item::Node(n) => {
                // Sisulizer never self-closes containers: an empty one is `<node ...>` + `</node>`.
                open_tag(s, level, &n.tag, &n.attrs);
                s.push_str(">\n");
                write_items(s, level + 1, &n.items);
                close_tag(s, level, &n.tag);
            }
            Item::Other(e) => write_generic(s, level, e),
        }
    }
}

fn write_row(s: &mut String, level: usize, r: &Row) {
    open_tag(s, level, "row", &encoded_attrs(&r.attrs));
    let code = |t: &str| if r.raw { t.to_string() } else { encode(t) };
    let native = r.native.as_deref().map(code);
    let native = native.as_deref();
    if r.translations.is_empty() && r.extra.is_empty() && r.pre_native.is_empty() {
        match native {
            None => {
                s.push_str("/>\n");
                return;
            }
            Some(n) if r.inline_native => {
                let _ = writeln!(s, ">{}</row>", escape_text(n));
                return;
            }
            _ => {}
        }
    }
    s.push_str(">\n");
    for e in &r.pre_native {
        write_generic(s, level + 1, e);
    }
    if let Some(n) = native {
        indent(s, level + 1);
        if n.is_empty() {
            s.push_str("<native/>\n");
        } else {
            let _ = writeln!(s, "<native>{}</native>", escape_text(n));
        }
    }
    for t in &r.translations {
        open_tag(s, level + 1, "lang", &encoded_attrs(&t.attrs));
        if t.text.is_empty() {
            s.push_str("/>\n");
        } else {
            let _ = writeln!(s, ">{}</lang>", escape_text(&code(&t.text)));
        }
    }
    for e in &r.extra {
        write_generic(s, level + 1, e);
    }
    close_tag(s, level, "row");
}

/// Pretty-prints an element preserved verbatim from the input.
fn write_generic(s: &mut String, level: usize, e: &Element) {
    open_tag(s, level, &e.name, &e.attrs);
    let has_elements = e.has_element_children();
    let text = e.text();
    if !has_elements {
        if text.is_empty() {
            s.push_str("/>\n");
        } else if text.trim().is_empty() && text.starts_with('\n') {
            // `<x>` + line break + indentation + `</x>`: how Sisulizer writes empty containers.
            let _ = writeln!(s, ">{text}</{}>", e.name);
        } else {
            let _ = writeln!(s, ">{}</{}>", escape_text(&text), e.name);
        }
        return;
    }
    if !text.trim().is_empty() {
        // Mixed content: keep it on one line exactly as parsed.
        s.push('>');
        for c in &e.children {
            write_inline(s, c);
        }
        let _ = writeln!(s, "</{}>", e.name);
        return;
    }
    s.push_str(">\n");
    for c in e.elements() {
        write_generic(s, level + 1, c);
    }
    close_tag(s, level, &e.name);
}

fn write_inline(s: &mut String, n: &xml::Node) {
    match n {
        xml::Node::Text(t) | xml::Node::CData(t) => s.push_str(&escape_text(t)),
        xml::Node::Element(e) => {
            s.push('<');
            s.push_str(&e.name);
            for (k, v) in e.attrs.iter() {
                let _ = write!(s, " {k}=\"{}\"", escape_attr(v));
            }
            if e.children.is_empty() {
                s.push_str("/>");
            } else {
                s.push('>');
                for c in &e.children {
                    write_inline(s, c);
                }
                let _ = write!(s, "</{}>", e.name);
            }
        }
        xml::Node::Comment(c) => {
            let _ = write!(s, "<!--{c}-->");
        }
        xml::Node::Pi(p) => {
            let _ = write!(s, "<?{p}?>");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<document created=\"20190109123553\" version=\"4.0.374\" date=\"20250114093920\" scan=\"92\" scanned=\"20241115140911\">\r\n  <lang id=\"de\"/>\r\n  <lang id=\"en\"/>\r\n  <source class=\"TJsonSource\" name=\"..\\src\\sis.json\" original=\"sis\" date=\"20241011100136\">\r\n    <additionalfiles>\r\n      <file>..\\src\\sis.json</file>\r\n    </additionalfiles>\r\n    <outputfile dir=\".\" localized=\"&lt;iso&gt;_v5.&lt;ext&gt;\"/>\r\n    <node class=\"TJsonNode\" name=\"sis.json\" state=\"0\" created=\"20191010131008\">\r\n      <row id=\"BUTTON.APPLY\" state=\"0\" created=\"20221121083558\" note=\"Auf &quot;Name&quot;\">\r\n        <native>[Apply]</native>\r\n        <lang id=\"de\" date=\"20221121111839\">Übernehmen&#160;x</lang>\r\n        <lang id=\"en\" status=\"3\">Apply</lang>\r\n      </row>\r\n      <row id=\"CDS.DESCRIPTION\" locked=\"1\">[The view]</row>\r\n      <row id=\"EMPTY\"/>\r\n    </node>\r\n  </source>\r\n  <excludedoriginals>\r\n    <item>OK</item>\r\n  </excludedoriginals>\r\n</document>\r\n";

    #[test]
    fn roundtrip_is_byte_identical() {
        let p = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(p.languages.len(), 2);
        let rows = p.rows();
        assert_eq!(rows.len(), 3);
        let r0 = p.row(&rows[0]).unwrap();
        assert_eq!(r0.text("de"), Some("Übernehmen\u{A0}x"));
        assert_eq!(r0.note(), "Auf \"Name\"");
        assert_eq!(p.row(&rows[1]).unwrap().original(), "[The view]");
        assert!(p.row(&rows[1]).unwrap().locked());
        assert_eq!(p.context(&rows[0]), "sis.json/BUTTON.APPLY");
        assert_eq!(p.sources[0].output_pattern(), (".".to_string(), "<iso>_v5.<ext>".to_string()));
        let out = to_bytes(&p);
        assert_eq!(String::from_utf8(out).unwrap(), SAMPLE);
    }

    #[test]
    fn inline_row_becomes_native_when_translated() {
        let mut p = parse(SAMPLE.as_bytes()).unwrap();
        let rows = p.rows();
        p.row_mut(&rows[1]).unwrap().set_translation("de", "[Die Ansicht]", crate::status::TranslationStatus::Translated);
        let out = String::from_utf8(to_bytes(&p)).unwrap();
        assert!(out.contains("<native>[The view]</native>"));
    }

    #[test]
    fn rejects_non_project() {
        assert!(parse(b"<root/>").is_err());
        assert!(parse(b"garbage").is_err());
    }
}
