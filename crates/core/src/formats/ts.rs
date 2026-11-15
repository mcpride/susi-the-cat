//! Qt Linguist translation sources (`.ts`), as written by `lupdate` and compiled to `.qm` by
//! `lrelease`.
//!
//! The original is one TS file of the application (`i18n/app_en.ts`, or a target language file
//! such as `app_de.ts`, which then is its own localized file). Every `<message>` is a row: the row
//! id is `context|source` (`context|source|comment` with a disambiguation comment) or the message
//! `id` of ID-based translations (`qsTrId`). The note combines the disambiguation `<comment>` and
//! the developer comment `<extracomment>` (`//:`). Plural messages (`numerus="yes"`) get one row
//! per numerus form, `id[0]`, `id[1]`, …, as many as the project languages need (Qt's rules:
//! German 2, Polish 3, Japanese 1, Arabic 6). Messages marked `vanished` or `obsolete` and
//! messages without source text are skipped.
//!
//! The build copies the original, sets `language` and fills every `<translation>`: finished
//! translations without `type`, drafts (best guess, for review) and missing ones as
//! `type="unfinished"`; plural messages get exactly the numerus forms of the target language. The
//! write method does not apply: a TS file always lists all messages. Obsolete messages and
//! translator comments are kept only in the file of the original's own language. The output is
//! formatted the way lupdate writes it, so that lupdate and Susi can work on the same files.

use std::collections::HashSet;
use std::fmt::Write as _;

use super::po::plural_row_id;
use super::{ScannedString, TextStyle};
use crate::error::{Error, Result};
use crate::xml::{self, Document, Element, Node};

/// Number of numerus forms Qt uses for a language (the plural rules of Qt Linguist, unknown
/// languages: 2).
pub fn numerus_forms(lang: &str) -> usize {
    let primary = lang.split(['-', '_', '.']).next().unwrap_or("").to_ascii_lowercase();
    match primary.as_str() {
        "bi" | "bo" | "dz" | "fa" | "fj" | "gn" | "hu" | "id" | "ja" | "jv" | "ko" | "ms" | "my" | "na" | "om" | "su" | "th" | "tr" | "tt" | "vi" | "yo"
        | "za" | "zh" => 1,
        "be" | "bs" | "ca" | "cs" | "dv" | "ga" | "gv" | "hr" | "ik" | "iu" | "lt" | "lv" | "mi" | "mk" | "pl" | "ro" | "ru" | "sa" | "se" | "sk" | "sm"
        | "sr" | "tl" | "uk" => 3,
        "gd" | "mt" | "sl" => 4,
        "cy" => 5,
        "ar" => 6,
        _ => 2,
    }
}

fn parse(bytes: &[u8]) -> Result<(Document, TextStyle, bool)> {
    let style = TextStyle::detect(bytes);
    let (text, _) = xml::decode_text(bytes)?;
    // Line breaks are restored by the style; literal carriage returns in texts are `<byte>` elements.
    let text = text.replace("\r\n", "\n");
    let doc = xml::parse_str(&text)?;
    if doc.root.name != "TS" {
        return Err(Error::Format(format!("not a Qt Linguist TS file (root element <{}>)", doc.root.name)));
    }
    Ok((doc, style, text.contains("<!DOCTYPE TS")))
}

/// Text of an element; `<byte value="x1"/>` (characters lupdate does not write literally) are resolved.
fn text(e: &Element) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c {
            Node::Text(t) | Node::CData(t) => s.push_str(t),
            Node::Element(b) if b.name == "byte" => {
                let v = b.attr("value").unwrap_or("");
                let code = match v.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => v.parse().ok(),
                };
                s.extend(code.and_then(char::from_u32));
            }
            _ => {}
        }
    }
    s
}

fn child_text(e: &Element, name: &str) -> String {
    e.child(name).map(text).unwrap_or_default()
}

fn is_obsolete(m: &Element) -> bool {
    m.child("translation").and_then(|t| t.attr("type")).is_some_and(|t| t == "vanished" || t == "obsolete")
}

fn is_numerus(m: &Element) -> bool {
    m.attr("numerus") == Some("yes")
}

/// Base row id: the message `id` or `context|source[|comment]`.
fn message_id(context: &str, m: &Element) -> String {
    if let Some(id) = m.attr("id").filter(|i| !i.is_empty()) {
        return id.to_string();
    }
    let mut id = format!("{context}|{}", child_text(m, "source"));
    let comment = child_text(m, "comment");
    if !comment.is_empty() {
        id.push('|');
        id.push_str(&comment);
    }
    id
}

/// All messages with the name of their context.
fn messages(root: &Element) -> Vec<(String, &Element)> {
    root.elements()
        .filter(|c| c.name == "context")
        .flat_map(|c| {
            let name = child_text(c, "name");
            c.elements().filter(|m| m.name == "message").map(move |m| (name.clone(), m))
        })
        .collect()
}

/// Scans a TS file. `langs` are the target languages: plural messages get rows for as many
/// numerus forms as the language with the most forms needs.
pub fn scan(bytes: &[u8], langs: &[String]) -> Result<Vec<ScannedString>> {
    let (doc, ..) = parse(bytes)?;
    let forms = langs.iter().map(|l| numerus_forms(l)).max().unwrap_or(2);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (context, m) in messages(&doc.root) {
        let source = child_text(m, "source");
        if is_obsolete(m) || source.is_empty() {
            continue;
        }
        let id = message_id(&context, m);
        let notes: Vec<String> = ["comment", "extracomment"].iter().map(|n| child_text(m, n).trim().to_string()).filter(|s| !s.is_empty()).collect();
        let comment = (!notes.is_empty()).then(|| notes.join("\n"));
        let ids = if is_numerus(m) { (0..forms).map(|k| plural_row_id(&id, k)).collect() } else { vec![id] };
        for id in ids {
            if seen.insert(id.clone()) {
                out.push(ScannedString { id, text: source.clone(), comment: comment.clone() });
            }
        }
    }
    Ok(out)
}

/// Finished translations of an existing TS file (taken over when the source is added).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    found(bytes, false)
}

/// Unfinished translations with text (taken over as best guesses).
pub fn drafts(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    found(bytes, true)
}

fn found(bytes: &[u8], unfinished: bool) -> Result<Vec<(String, String)>> {
    let (doc, ..) = parse(bytes)?;
    let mut out = Vec::new();
    for (context, m) in messages(&doc.root) {
        let Some(t) = m.child("translation") else { continue };
        if is_obsolete(m) || (t.attr("type") == Some("unfinished")) != unfinished {
            continue;
        }
        let id = message_id(&context, m);
        if is_numerus(m) {
            out.extend(t.elements().filter(|f| f.name == "numerusform").enumerate().map(|(k, f)| (plural_row_id(&id, k), text(f))));
        } else {
            out.push((id, text(t)));
        }
    }
    out.retain(|(_, t)| !t.is_empty());
    Ok(out)
}

/// A translation for the build: text and whether it is finished.
pub type Translation = Option<(String, bool)>;

/// Builds the TS file of `lang`. `own` is set when the localized file is the original itself: its
/// `language`, obsolete messages and translator comments are kept. `translation(id)` returns the
/// text of a row and whether it is finished (`None`: not translated).
pub fn build(original: &[u8], lang: &str, own: bool, translation: &dyn Fn(&str) -> Translation) -> Result<Vec<u8>> {
    let (mut doc, style, doctype) = parse(original)?;
    if !own {
        doc.root.attrs.set("language", crate::lang::iso(lang));
    }
    let forms = numerus_forms(lang);
    for ctx in doc.root.elements_mut().filter(|c| c.name == "context") {
        let name = child_text(ctx, "name");
        let old = std::mem::take(&mut ctx.children);
        let mut out = Vec::with_capacity(old.len());
        for node in old {
            match node {
                Node::Element(m) if m.name == "message" && is_obsolete(&m) && !own => drop_trailing_ws(&mut out),
                Node::Element(mut m) if m.name == "message" && !is_obsolete(&m) => {
                    let id = message_id(&name, &m);
                    fill(&mut m, &id, forms, own, translation);
                    out.push(Node::Element(m));
                }
                other => out.push(other),
            }
        }
        ctx.children = out;
    }
    // Contexts without messages are dropped, as lupdate does.
    retain_elements(&mut doc.root, |e| e.name != "context" || e.elements().any(|m| m.name == "message"));
    Ok(write(&doc, doctype, style))
}

/// Replaces the `<translation>` of a message.
fn fill(m: &mut Element, id: &str, forms: usize, own: bool, translation: &dyn Fn(&str) -> Translation) {
    let indent = indent_of(m, "translation").or_else(|| indent_of(m, "source")).unwrap_or_else(|| " ".repeat(8));
    let mut tr = Element::new("translation");
    let finished = if is_numerus(m) {
        let found: Vec<Translation> = (0..forms).map(|k| translation(&plural_row_id(id, k))).collect();
        for f in &found {
            let mut form = Element::new("numerusform");
            if let Some((t, _)) = f {
                form.children.push(Node::Text(t.clone()));
            }
            tr.children.push(Node::Text(format!("\n{indent}    ")));
            tr.children.push(Node::Element(form));
        }
        tr.children.push(Node::Text(format!("\n{indent}")));
        found.iter().all(|f| f.as_ref().is_some_and(|(_, done)| *done))
    } else {
        match translation(id) {
            Some((t, done)) => {
                tr.children.push(Node::Text(t));
                done
            }
            None => false,
        }
    };
    if !finished {
        tr.attrs.set("type", "unfinished");
    }
    // Translator comments belong to the language of the file; the old source text of a lupdate
    // guess is obsolete once the translation is finished.
    retain_elements(m, |e| !((e.name == "translatorcomment" && !own) || (e.name == "oldsource" && finished)));
    match m.children.iter().position(|n| matches!(n, Node::Element(e) if e.name == "translation")) {
        Some(i) => m.children[i] = Node::Element(tr),
        None => {
            let at = m.children.iter().rposition(|n| matches!(n, Node::Element(_))).map_or(0, |i| i + 1);
            m.children.splice(at..at, [Node::Text(format!("\n{indent}")), Node::Element(tr)]);
        }
    }
}

/// Indentation of the child element `name` (the whitespace after the last line break before it).
fn indent_of(m: &Element, name: &str) -> Option<String> {
    let i = m.children.iter().position(|n| matches!(n, Node::Element(e) if e.name == name))?;
    match m.children.get(i.checked_sub(1)?)? {
        Node::Text(t) if t.trim().is_empty() => t.rsplit('\n').next().map(str::to_string),
        _ => None,
    }
}

/// Keeps the child elements matching `keep`; dropped elements take their indentation with them.
fn retain_elements(e: &mut Element, keep: impl Fn(&Element) -> bool) {
    let old = std::mem::take(&mut e.children);
    for node in old {
        match node {
            Node::Element(c) if !keep(&c) => drop_trailing_ws(&mut e.children),
            other => e.children.push(other),
        }
    }
}

fn drop_trailing_ws(out: &mut Vec<Node>) {
    if let Some(Node::Text(t)) = out.last()
        && t.trim().is_empty()
    {
        out.pop();
    }
}

/// Elements lupdate writes with an end tag even when they are empty.
const TEXT_ELEMENTS: [&str; 10] =
    ["name", "source", "oldsource", "comment", "oldcomment", "extracomment", "translatorcomment", "translation", "numerusform", "lengthvariant"];

fn write(doc: &Document, doctype: bool, style: TextStyle) -> Vec<u8> {
    let mut s = String::new();
    let _ = writeln!(s, "<?{}?>", doc.decl.as_deref().unwrap_or("xml version=\"1.0\" encoding=\"utf-8\""));
    if doctype {
        s.push_str("<!DOCTYPE TS>\n");
    }
    for n in &doc.prolog {
        write_node(&mut s, n);
        if matches!(n, Node::Comment(_) | Node::Pi(_)) {
            s.push('\n');
        }
    }
    write_element(&mut s, &doc.root);
    s.push('\n');
    style.apply(&s)
}

fn write_node(s: &mut String, n: &Node) {
    match n {
        Node::Element(e) => write_element(s, e),
        Node::Text(t) => protect(s, t),
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
        let _ = write!(s, " {k}=\"");
        protect(s, v);
        s.push('"');
    }
    if e.children.is_empty() && !TEXT_ELEMENTS.contains(&e.name.as_str()) {
        s.push_str("/>");
        return;
    }
    s.push('>');
    for c in &e.children {
        write_node(s, c);
    }
    let _ = write!(s, "</{}>", e.name);
}

/// Escapes like lupdate: the five XML entities, and control characters and non-ASCII spaces as
/// `<byte value="xa0"/>`.
fn protect(s: &mut String, t: &str) {
    for c in t.chars() {
        match c {
            '"' => s.push_str("&quot;"),
            '&' => s.push_str("&amp;"),
            '>' => s.push_str("&gt;"),
            '<' => s.push_str("&lt;"),
            '\'' => s.push_str("&apos;"),
            '\n' | '\t' => s.push(c),
            c if (c as u32) < 0x20 || (c as u32 > 0x7F && c.is_whitespace()) => {
                let _ = write!(s, "<byte value=\"x{:x}\"/>", c as u32);
            }
            c => s.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE TS>
<TS version="2.1" language="de" sourcelanguage="en">
<context>
    <name>MainWindow</name>
    <message>
        <location filename="../mainwindow.cpp" line="12"/>
        <source>&amp;Open</source>
        <comment>verb</comment>
        <extracomment>File menu</extracomment>
        <translation>&amp;Öffnen</translation>
    </message>
    <message numerus="yes">
        <location filename="../mainwindow.cpp" line="20"/>
        <source>%n file(s)</source>
        <translation type="unfinished">
            <numerusform>%n Datei</numerusform>
            <numerusform></numerusform>
        </translation>
    </message>
    <message>
        <source>Don&apos;t save</source>
        <oldsource>Do not save</oldsource>
        <translatorcomment>checked</translatorcomment>
        <translation type="unfinished">Nicht speichern</translation>
    </message>
    <message>
        <source>Old</source>
        <translation type="vanished">Alt</translation>
    </message>
</context>
<context>
    <name>Gone</name>
    <message>
        <source>Gone</source>
        <translation type="obsolete">Weg</translation>
    </message>
</context>
<context>
    <name></name>
    <message id="app-title">
        <source>Tab<byte value="xa0"/>Saver</source>
        <translation type="unfinished"></translation>
    </message>
</context>
</TS>
"#;

    #[test]
    fn scans_messages_ids_notes_and_numerus_forms() {
        let s = scan(SRC.as_bytes(), &["de".into(), "pl".into()]).unwrap();
        let rows: Vec<(&str, &str, Option<&str>)> = s.iter().map(|x| (x.id.as_str(), x.text.as_str(), x.comment.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("MainWindow|&Open|verb", "&Open", Some("verb\nFile menu")),
                ("MainWindow|%n file(s)[0]", "%n file(s)", None),
                ("MainWindow|%n file(s)[1]", "%n file(s)", None),
                ("MainWindow|%n file(s)[2]", "%n file(s)", None),
                ("MainWindow|Don't save", "Don't save", None),
                ("app-title", "Tab\u{a0}Saver", None),
            ]
        );
        assert_eq!(scan(SRC.as_bytes(), &["ja".into()]).unwrap().iter().filter(|x| x.id.contains("file(s)")).count(), 1);
        assert!(scan(b"<resources/>", &[]).is_err());
    }

    #[test]
    fn finished_translations_and_drafts() {
        let own = |v: Vec<(String, String)>| v.into_iter().collect::<Vec<_>>();
        assert_eq!(own(translations(SRC.as_bytes()).unwrap()), [("MainWindow|&Open|verb".to_string(), "&Öffnen".to_string())]);
        let drafts = drafts(SRC.as_bytes()).unwrap();
        let ids: Vec<&str> = drafts.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["MainWindow|%n file(s)[0]", "MainWindow|Don't save"]);
    }

    const PL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE TS>
<TS version="2.1" language="pl" sourcelanguage="en">
<context>
    <name>MainWindow</name>
    <message>
        <location filename="../mainwindow.cpp" line="12"/>
        <source>&amp;Open</source>
        <comment>verb</comment>
        <extracomment>File menu</extracomment>
        <translation>&amp;Otwórz</translation>
    </message>
    <message numerus="yes">
        <location filename="../mainwindow.cpp" line="20"/>
        <source>%n file(s)</source>
        <translation type="unfinished">
            <numerusform>%n plik</numerusform>
            <numerusform>%n pliki</numerusform>
            <numerusform>%n plików</numerusform>
        </translation>
    </message>
    <message>
        <source>Don&apos;t save</source>
        <oldsource>Do not save</oldsource>
        <translation type="unfinished"></translation>
    </message>
</context>
<context>
    <name></name>
    <message id="app-title">
        <source>Tab<byte value="xa0"/>Saver</source>
        <translation>Tab<byte value="xa0"/>Saver</translation>
    </message>
</context>
</TS>
"#;

    #[test]
    fn build_writes_target_language_like_lupdate() {
        let tr = |id: &str| -> Translation {
            match id {
                "MainWindow|&Open|verb" => Some(("&Otwórz".into(), true)),
                "MainWindow|%n file(s)[0]" => Some(("%n plik".into(), true)),
                "MainWindow|%n file(s)[1]" => Some(("%n pliki".into(), true)),
                "MainWindow|%n file(s)[2]" => Some(("%n plików".into(), false)),
                "app-title" => Some(("Tab\u{a0}Saver".into(), true)),
                _ => None,
            }
        };
        let out = String::from_utf8(build(SRC.as_bytes(), "pl", false, &tr).unwrap()).unwrap();
        assert_eq!(out, PL);
        // CRLF originals stay CRLF.
        let crlf = build(SRC.replace('\n', "\r\n").as_bytes(), "pl", false, &tr).unwrap();
        assert_eq!(String::from_utf8(crlf).unwrap(), PL.replace('\n', "\r\n"));
    }

    #[test]
    fn build_of_the_own_language_keeps_obsolete_messages_and_translator_comments() {
        // The file keeps its language even when the project calls it differently (`de_DE` / `de`).
        let src = SRC.replace("language=\"de\"", "language=\"de_DE\"");
        let out = String::from_utf8(build(src.as_bytes(), "de", true, &|_| None).unwrap()).unwrap();
        assert!(out.contains("<TS version=\"2.1\" language=\"de_DE\" sourcelanguage=\"en\">"), "{out}");
        assert!(out.contains("<translation type=\"vanished\">Alt</translation>"), "{out}");
        assert!(out.contains("<name>Gone</name>") && out.contains("<translatorcomment>checked</translatorcomment>"), "{out}");
        // German has two numerus forms; nothing is translated, so everything is unfinished.
        assert!(
            out.contains(
                "<translation type=\"unfinished\">\n            <numerusform></numerusform>\n            <numerusform></numerusform>\n        </translation>"
            ),
            "{out}"
        );
        assert!(out.contains("<translation type=\"vanished\">Alt</translation>\n    </message>\n</context>\n<context>\n    <name>Gone</name>"), "{out}");
    }

    #[test]
    fn numerus_form_counts() {
        let n = |l: &str| numerus_forms(l);
        assert_eq!(
            (n("de"), n("pt-BR"), n("fr"), n("pl"), n("ru"), n("cs"), n("sl"), n("ar"), n("ja"), n("zh.tra"), n("xx")),
            (2, 2, 2, 3, 3, 3, 4, 6, 1, 1, 2)
        );
    }
}
