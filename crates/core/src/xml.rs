//! A small, order-preserving XML DOM on top of `quick-xml`.
//!
//! Sisulizer projects carry many attributes and elements that Susi does not interpret. To keep
//! saved files faithful, every element keeps its attributes in document order and every unknown
//! child is preserved verbatim (including whitespace text nodes in RESX files).

use std::fmt::Write as _;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{Error, Result};

/// Maximum element nesting accepted from input (guards against hostile, deeply nested files).
const MAX_DEPTH: usize = 512;

/// Ordered attribute list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs(pub Vec<(String, String)>);

impl Attrs {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Sets an attribute, keeping its position if it already exists (appends otherwise).
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value;
        } else {
            self.0.push((key.to_string(), value));
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        let pos = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(pos).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
    CData(String),
    Comment(String),
    Pi(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Attrs,
    pub children: Vec<Node>,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), attrs: Attrs::new(), children: Vec::new() }
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key)
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| if let Node::Element(e) = c { Some(e) } else { None })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|c| if let Node::Element(e) = c { Some(e) } else { None })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elements_mut().find(|e| e.name == name)
    }

    /// Concatenated text and CDATA content of this element (not of descendants).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) | Node::CData(t) => s.push_str(t),
                _ => {}
            }
        }
        s
    }

    /// Replaces all text content with `text`, keeping child elements.
    pub fn set_text(&mut self, text: &str) {
        self.children.retain(|c| !matches!(c, Node::Text(_) | Node::CData(_)));
        self.children.insert(0, Node::Text(text.to_string()));
    }

    pub fn has_element_children(&self) -> bool {
        self.children.iter().any(|c| matches!(c, Node::Element(_)))
    }
}

/// A parsed XML document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub bom: bool,
    /// Raw XML declaration content, e.g. `xml version="1.0" encoding="utf-8"`.
    pub decl: Option<String>,
    /// Nodes between the declaration and the root element (comments, whitespace, PIs).
    pub prolog: Vec<Node>,
    pub root: Element,
    /// Whether the input used CRLF line endings.
    pub crlf: bool,
}

/// Decodes bytes to a string, honouring UTF-8 and UTF-16 byte-order marks.
pub fn decode_text(bytes: &[u8]) -> Result<(String, bool)> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok((String::from_utf8(rest.to_vec()).map_err(|e| Error::Format(format!("invalid UTF-8: {e}")))?, true));
    }
    if bytes.len() >= 2 && (bytes[..2] == [0xFF, 0xFE] || bytes[..2] == [0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> =
            bytes[2..].as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return Ok((String::from_utf16(&units).map_err(|e| Error::Format(format!("invalid UTF-16: {e}")))?, true));
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => Ok((s, false)),
        // Old files are sometimes Windows-1252/Latin-1 encoded.
        Err(e) => Ok((e.into_bytes().iter().map(|&b| char::from(b)).collect(), false)),
    }
}

pub fn parse_bytes(bytes: &[u8]) -> Result<Document> {
    let (text, bom) = decode_text(bytes)?;
    let mut doc = parse_str(&text)?;
    doc.bom = bom;
    Ok(doc)
}

pub fn parse_str(text: &str) -> Result<Document> {
    let mut reader = Reader::from_str(text);
    let cfg = reader.config_mut();
    cfg.trim_text_start = false;
    cfg.trim_text_end = false;
    cfg.expand_empty_elements = false;
    cfg.check_end_names = true;

    let mut doc = Document { crlf: text.contains("\r\n"), ..Default::default() };
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    loop {
        let ev = reader.read_event().map_err(|e| Error::Xml(format!("{e} at byte {}", reader.buffer_position())))?;
        match ev {
            Event::Decl(d) => doc.decl = Some(AsRef::<str>::as_ref(&d).to_string()),
            Event::Start(s) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::Xml("element nesting too deep".into()));
                }
                stack.push(start_to_element(&s)?);
            }
            Event::Empty(s) => {
                let el = start_to_element(&s)?;
                push_node(&mut stack, &mut root, &mut doc.prolog, Node::Element(el))?;
            }
            Event::End(_) => {
                let el = stack.pop().ok_or_else(|| Error::Xml("unbalanced end tag".into()))?;
                push_node(&mut stack, &mut root, &mut doc.prolog, Node::Element(el))?;
            }
            Event::Text(t) => {
                let raw = t.xml10_content();
                let s = unescape(&raw)?;
                push_text(&mut stack, &mut doc.prolog, root.is_some(), s);
            }
            Event::GeneralRef(r) => {
                let name = r.into_inner();
                let s = resolve_entity(&name)?;
                push_text(&mut stack, &mut doc.prolog, root.is_some(), s);
            }
            Event::CData(c) => {
                if let Some(top) = stack.last_mut() {
                    top.children.push(Node::CData(c.xml10_content().into_owned()));
                }
            }
            Event::Comment(c) => {
                let n = Node::Comment(c.into_inner().into_owned());
                if let Some(top) = stack.last_mut() {
                    top.children.push(n);
                } else if root.is_none() {
                    doc.prolog.push(n);
                }
            }
            Event::PI(p) => {
                let n = Node::Pi(p.into_inner().into_owned());
                if let Some(top) = stack.last_mut() {
                    top.children.push(n);
                } else if root.is_none() {
                    doc.prolog.push(n);
                }
            }
            Event::DocType(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(Error::Xml("unexpected end of document (unclosed elements)".into()));
    }
    doc.root = root.ok_or_else(|| Error::Xml("document has no root element".into()))?;
    Ok(doc)
}

fn push_text(stack: &mut [Element], prolog: &mut Vec<Node>, root_done: bool, s: String) {
    if let Some(top) = stack.last_mut() {
        if let Some(Node::Text(prev)) = top.children.last_mut() {
            prev.push_str(&s);
        } else {
            top.children.push(Node::Text(s));
        }
    } else if !root_done && !s.trim().is_empty() {
        prolog.push(Node::Text(s));
    }
}

fn push_node(stack: &mut [Element], root: &mut Option<Element>, _prolog: &mut [Node], node: Node) -> Result<()> {
    if let Some(top) = stack.last_mut() {
        top.children.push(node);
        return Ok(());
    }
    match node {
        Node::Element(e) => {
            if root.is_some() {
                return Err(Error::Xml("multiple root elements".into()));
            }
            *root = Some(e);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn start_to_element(s: &quick_xml::events::BytesStart<'_>) -> Result<Element> {
    let name = AsRef::<str>::as_ref(&s.name()).to_string();
    let mut attrs = Attrs::new();
    for a in s.attributes().with_checks(false) {
        let a = a.map_err(|e| Error::Xml(e.to_string()))?;
        let key = AsRef::<str>::as_ref(&a.key).to_string();
        let value = unescape(&a.value)?;
        attrs.0.push((key, value));
    }
    Ok(Element { name, attrs, children: Vec::new() })
}

fn resolve_entity(name: &str) -> Result<String> {
    Ok(match name {
        "lt" => "<".into(),
        "gt" => ">".into(),
        "amp" => "&".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        _ => {
            let num = name.strip_prefix('#').ok_or_else(|| Error::Xml(format!("unknown entity &{name};")))?;
            let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) { u32::from_str_radix(hex, 16) } else { num.parse::<u32>() }
                .map_err(|_| Error::Xml(format!("invalid character reference &{name};")))?;
            char::from_u32(code).ok_or_else(|| Error::Xml(format!("invalid character reference &{name};")))?.to_string()
        }
    })
}

/// Resolves the predefined XML entities and character references in `s`.
pub fn unescape(s: &str) -> Result<String> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let end = after.find(';').ok_or_else(|| Error::Xml("unterminated entity reference".into()))?;
        out.push_str(&resolve_entity(&after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Characters Sisulizer writes as numeric references (control characters and no-break space).
fn needs_char_ref(c: char) -> bool {
    (c < ' ' && c != '\u{0}') || c == '\u{A0}' || c == '\u{7F}'
}

/// Escapes element text the way Sisulizer does: `& < >` as entities, control characters and the
/// no-break space as decimal character references.
pub fn escape_text(s: &str) -> String {
    escape(s, false)
}

/// Escapes an attribute value (like [`escape_text`], plus `"`).
pub fn escape_attr(s: &str) -> String {
    escape(s, true)
}

fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\u{0}' => {}
            c if needs_char_ref(c) => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Escaping for generic (non-Sisulizer) documents such as RESX: keeps line breaks and tabs literal.
fn escape_plain(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#10;"),
            '\r' if attr => out.push_str("&#13;"),
            '\t' if attr => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            '\u{0}' => {}
            c if c < ' ' && c != '\n' && c != '\t' => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Serialises a generic document verbatim (whitespace text nodes are preserved, nothing is
/// re-indented). Used for RESX and other third-party XML files.
pub fn write_document(doc: &Document) -> Vec<u8> {
    let mut s = String::new();
    if let Some(decl) = &doc.decl {
        let _ = write!(s, "<?{decl}?>");
        s.push('\n');
    }
    for n in &doc.prolog {
        write_node(&mut s, n);
        if matches!(n, Node::Comment(_) | Node::Pi(_)) {
            s.push('\n');
        }
    }
    write_element(&mut s, &doc.root);
    s.push('\n');
    let s = if doc.crlf { s.replace("\r\n", "\n").replace('\n', "\r\n") } else { s };
    let mut out = Vec::with_capacity(s.len() + 3);
    if doc.bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    out.extend_from_slice(s.as_bytes());
    out
}

fn write_node(s: &mut String, n: &Node) {
    match n {
        Node::Element(e) => write_element(s, e),
        Node::Text(t) => s.push_str(&escape_plain(t, false)),
        Node::CData(t) => {
            let _ = write!(s, "<![CDATA[{t}]]>");
        }
        Node::Comment(t) => {
            let _ = write!(s, "<!--{t}-->");
        }
        Node::Pi(t) => {
            let _ = write!(s, "<?{t}?>");
        }
    }
}

fn write_element(s: &mut String, e: &Element) {
    s.push('<');
    s.push_str(&e.name);
    for (k, v) in e.attrs.iter() {
        let _ = write!(s, " {k}=\"{}\"", escape_plain(v, true));
    }
    if e.children.is_empty() {
        s.push_str(" />");
        return;
    }
    s.push('>');
    for c in &e.children {
        write_node(s, c);
    }
    let _ = write!(s, "</{}>", e.name);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_entities_and_order() {
        let src = "<?xml version=\"1.0\"?>\n<a z=\"1\" b=\"&lt;iso&gt;\"><b>x &amp; y&#160;z</b>\n  <c/></a>";
        let doc = parse_str(src).unwrap();
        assert_eq!(doc.root.attrs.0[0].0, "z");
        assert_eq!(doc.root.attr("b"), Some("<iso>"));
        assert_eq!(doc.root.child("b").unwrap().text(), "x & y\u{A0}z");
        let out = String::from_utf8(write_document(&doc)).unwrap();
        assert!(out.contains("b=\"&lt;iso&gt;\""));
        assert!(out.contains("<c />"));
    }

    #[test]
    fn escape_like_sisulizer() {
        assert_eq!(escape_attr("Auf \"Name\" <x>"), "Auf &quot;Name&quot; &lt;x&gt;");
        assert_eq!(escape_text("a\u{A0}b\r\nc"), "a&#160;b&#13;&#10;c");
    }

    #[test]
    fn rejects_garbage_without_panic() {
        assert!(parse_str("<a><b></a>").is_err());
        assert!(parse_str("").is_err());
        assert!(parse_str("<a>&bogus;</a>").is_err());
    }
}
