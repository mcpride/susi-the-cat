//! .NET resource files (`.resx`).
//!
//! Localizable items are `<data>` elements without a `type`/`mimetype` (i.e. strings) whose name
//! does not start with `>>` (designer metadata). Localized files are copies of the original with
//! the non-string items removed and the string values replaced.

use super::ScannedString;
use crate::error::{Error, Result};
use crate::xml::{self, Document, Element, Node};

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

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let doc = parse(bytes)?;
    Ok(doc
        .root
        .elements()
        .filter(|e| is_string_data(e))
        .map(|e| {
            let comment = e.child("comment").map(Element::text).filter(|c| !c.trim().is_empty());
            ScannedString { id: e.attr("name").unwrap_or("").to_string(), text: e.child("value").map(Element::text).unwrap_or_default(), comment }
        })
        .collect())
}

/// Builds a localized `.resx`. `resolve(name, original)` returns the value to write or `None` to
/// omit the item.
pub fn build(original: &[u8], resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let mut doc = parse(original)?;
    let old = std::mem::take(&mut doc.root.children);
    let mut out: Vec<Node> = Vec::with_capacity(old.len());
    for node in old {
        match node {
            Node::Element(mut e) if e.name == "data" || e.name == "metadata" => {
                if !is_string_data(&e) {
                    drop_trailing_ws(&mut out);
                    continue;
                }
                let name = e.attr("name").unwrap_or("").to_string();
                let orig = e.child("value").map(Element::text).unwrap_or_default();
                match resolve(&name, &orig) {
                    Some(text) => {
                        match e.child_mut("value") {
                            Some(v) => v.set_text(&text),
                            None => {
                                let mut v = Element::new("value");
                                v.set_text(&text);
                                e.children.push(Node::Element(v));
                            }
                        }
                        out.push(Node::Element(e));
                    }
                    None => drop_trailing_ws(&mut out),
                }
            }
            other => out.push(other),
        }
    }
    doc.root.children = out;
    Ok(xml::write_document(&doc))
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

    #[test]
    fn build_translated_only() {
        let out = build(SRC.as_bytes(), &|n, _| (n == "Hello").then(|| "Hallo & willkommen".to_string())).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("<value>Hallo &amp; willkommen</value>"), "{s}");
        assert!(!s.contains("Icon"));
        assert!(!s.contains("Bye"));
        assert!(s.contains("text/microsoft-resx"));
        assert!(s.contains("\r\n"));
        assert_eq!(scan(s.as_bytes()).unwrap().len(), 1);
    }
}
