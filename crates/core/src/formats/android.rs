//! Android string resources (`res/values/strings.xml` and other `values` files).
//!
//! Sisulizer stores a single values file as `TAndroidValuesSource` with the rows in a
//! `TAndroidValuesGroup` node named `string`. Localized files go to the BCP 47 resource directories
//! of Android 7.0+ (`res/values-b+de/strings.xml`, `res/values-b+pt+BR/strings.xml`); projects that
//! already use the legacy directories (`values-de`, `values-pt-rBR`) keep them (see
//! [`existing_pattern`]), because both forms for the same locale would be duplicate resources.
//!
//! - Localizable: `<string>` (also `<item type="string">`), the items of `<string-array>` and the
//!   quantities of `<plurals>`. Skipped: `translatable="false"`, references (`@string/other`,
//!   `?attr/x`), empty values and every other resource type (colors, dimensions, styles, ...).
//! - Row ids: the resource name; array items `name[0]`, `name[1]`, ...; plural quantities
//!   `name[one]`, `name[other]`, ...; resources with a `product` attribute (other than `default`)
//!   get `[product=x]` appended to the name.
//! - Plurals: target languages need other quantities than the original (Polish: `one`, `few`,
//!   `many`, `other`). The scan adds rows for the quantities the project languages use (CLDR
//!   rules for integers, [`plural_categories`]) with the original text of `other`; the build writes
//!   exactly the quantities of the target language.
//! - Texts are shown as Android displays them: XML entities and Android escapes (`\'`, `\"`, `\n`,
//!   `\t`, `\@`, `\?`, `\uXXXX`) resolved, double-quoted sections unquoted, whitespace collapsed.
//!   Values with styling or `<xliff:g>` placeholders keep their tags (and XML-escaped text, `&amp;`).
//!   Building escapes translations again, so translators never type Android escapes.
//! - Notes: the XML comments directly above a resource (a blank line separates them).
//! - Building copies the original with the translated values; non-localizable resources and
//!   omitted strings (write method) are removed together with their comments, so Android falls
//!   back to the default resources. Arrays and plurals are written as a whole when at least one
//!   of their items is written (missing items keep the original text).

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use super::ScannedString;
use crate::error::{Error, Result};
use crate::xml::{self, Document, Element, Node};

/// All plural quantities in Android's order.
pub const QUANTITIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// Plural quantities a language uses for integers (CLDR), always including `other` (Android's
/// fallback). `None` for unknown languages.
pub fn plural_categories(lang: &str) -> Option<&'static [&'static str]> {
    let base = lang.split(['-', '_', '.', '+']).next().unwrap_or("").to_ascii_lowercase();
    Some(match base.as_str() {
        "ja" | "ko" | "zh" | "th" | "vi" | "id" | "in" | "ms" | "lo" | "km" | "my" | "jv" | "bo" => &["other"],
        "af" | "am" | "az" | "bg" | "bn" | "ca" | "da" | "de" | "el" | "en" | "es" | "et" | "eu" | "fa" | "fi" | "fil" | "fr" | "fy" | "gl" | "gu" | "hi"
        | "hu" | "hy" | "is" | "it" | "ka" | "kk" | "kn" | "ky" | "lb" | "mk" | "ml" | "mn" | "mr" | "nb" | "ne" | "nl" | "nn" | "no" | "pa" | "pt" | "si"
        | "sq" | "sv" | "sw" | "ta" | "te" | "tl" | "tr" | "ur" | "uz" | "zu" => &["one", "other"],
        "bs" | "cs" | "hr" | "lt" | "ro" | "sk" | "sr" => &["one", "few", "other"],
        "be" | "pl" | "ru" | "uk" => &["one", "few", "many", "other"],
        "sl" => &["one", "two", "few", "other"],
        "lv" => &["zero", "one", "other"],
        "he" | "iw" => &["one", "two", "other"],
        "ga" | "mt" => &["one", "two", "few", "many", "other"],
        "ar" | "cy" => &QUANTITIES,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Array,
    Plurals,
}

fn kind(e: &Element) -> Option<Kind> {
    match e.name.as_str() {
        "string" => Some(Kind::String),
        "item" if e.attr("type") == Some("string") => Some(Kind::String),
        "string-array" => Some(Kind::Array),
        "plurals" => Some(Kind::Plurals),
        _ => None,
    }
}

fn translatable(e: &Element) -> bool {
    e.attr("translatable").is_none_or(|v| !v.trim().eq_ignore_ascii_case("false"))
}

/// `@string/x`, `@android:string/x`, `?attr/x`: not text but a reference to another resource.
fn is_reference(e: &Element) -> bool {
    !e.has_element_children() && {
        let t = e.text();
        let t = t.trim_start();
        t.starts_with('@') || t.starts_with('?')
    }
}

/// Row id of a resource (without item suffix).
fn base_id(e: &Element) -> Option<String> {
    let name = e.attr("name").map(str::trim).filter(|n| !n.is_empty())?;
    Some(match e.attr("product").map(str::trim) {
        Some(p) if !p.is_empty() && p != "default" => format!("{name}[product={p}]"),
        _ => name.to_string(),
    })
}

fn array_items(e: &Element) -> impl Iterator<Item = &Element> {
    e.elements().filter(|c| c.name == "item")
}

fn plural_items(e: &Element) -> Vec<(&'static str, &Element)> {
    e.elements().filter(|c| c.name == "item").filter_map(|c| QUANTITIES.iter().find(|q| c.attr("quantity") == Some(q)).map(|q| (*q, c))).collect()
}

/// The item whose text stands in for quantities missing in the original.
fn fallback_item<'a>(items: &[(&'static str, &'a Element)]) -> Option<&'a Element> {
    items.iter().find(|(q, _)| *q == "other").or(items.last()).map(|(_, e)| *e)
}

pub fn parse(bytes: &[u8]) -> Result<Document> {
    let doc = xml::parse_bytes(bytes)?;
    if doc.root.name != "resources" {
        return Err(Error::Format(format!("not an Android resource file (root element <{}>)", doc.root.name)));
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Android text decoding and encoding

/// Whitespace that Android collapses (non-breaking spaces are kept).
fn collapsible(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}')
}

fn escape_xml_char(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        c => out.push(c),
    }
}

fn open_tag(e: &Element) -> String {
    let mut s = format!("<{}", e.name);
    for (k, v) in e.attrs.iter() {
        let _ = write!(s, " {k}=\"");
        for c in v.chars() {
            match c {
                '"' => s.push_str("&quot;"),
                c => escape_xml_char(&mut s, c),
            }
        }
        s.push('"');
    }
    s.push_str(if e.children.is_empty() { "/>" } else { ">" });
    s
}

/// Applies Android's string processing to the text of a resource value.
#[derive(Default)]
struct Decoder {
    out: String,
    /// Keep tags and XML-escape the text (value with child elements).
    markup: bool,
    quoted: bool,
    /// Collapsed whitespace waiting for the next character (dropped at the start and end).
    space: bool,
    started: bool,
    /// Tags after pending whitespace (written after the space once more text follows).
    deferred: String,
}

impl Decoder {
    fn text(&mut self, s: &str) {
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    let Some(e) = chars.next() else { break };
                    let ch = match e {
                        'n' => '\n',
                        't' => '\t',
                        'u' => {
                            let hex: String = chars.clone().take(4).take_while(char::is_ascii_hexdigit).collect();
                            for _ in 0..hex.len() {
                                chars.next();
                            }
                            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                Some(c) if hex.len() == 4 => c,
                                _ => continue,
                            }
                        }
                        other => other,
                    };
                    self.push(ch);
                }
                '"' => self.quoted = !self.quoted,
                c if !self.quoted && collapsible(c) => self.space = true,
                c => self.push(c),
            }
        }
    }

    fn push(&mut self, c: char) {
        if std::mem::take(&mut self.space) && self.started {
            self.out.push(' ');
        }
        self.out.push_str(&std::mem::take(&mut self.deferred));
        self.started = true;
        if self.markup {
            escape_xml_char(&mut self.out, c);
        } else {
            self.out.push(c);
        }
    }

    fn tag(&mut self, t: &str) {
        if self.space && self.started {
            self.deferred.push_str(t);
        } else {
            self.out.push_str(t);
        }
    }

    fn walk(&mut self, e: &Element) {
        for c in &e.children {
            match c {
                Node::Text(t) | Node::CData(t) => self.text(t),
                Node::Element(child) => {
                    self.tag(&open_tag(child));
                    if !child.children.is_empty() {
                        self.walk(child);
                        self.tag(&format!("</{}>", child.name));
                    }
                }
                Node::Comment(_) | Node::Pi(_) => {}
            }
        }
    }

    fn finish(mut self) -> String {
        self.out.push_str(&self.deferred);
        self.out
    }
}

/// The text of a resource value as Android shows it (tags kept for styled values).
fn decode(e: &Element) -> String {
    let mut d = Decoder { markup: e.has_element_children(), ..Default::default() };
    d.walk(e);
    d.finish()
}

enum Piece {
    Ch(char),
    Tag(String),
}

/// Writes text pieces as the XML source of a resource value with Android escaping; `xml` also
/// escapes `& < >` (not inside CDATA).
fn encode(pieces: &[Piece], xml: bool) -> String {
    let total = pieces.iter().filter(|p| matches!(p, Piece::Ch(_))).count();
    let mut out = String::new();
    let mut seen = 0;
    let mut prev = None;
    for p in pieces {
        let c = match p {
            Piece::Tag(t) => {
                out.push_str(t);
                continue;
            }
            Piece::Ch(c) => *c,
        };
        let first = seen == 0;
        let last = seen + 1 == total;
        seen += 1;
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '@' | '?' if first => {
                out.push('\\');
                out.push(c);
            }
            // Spaces Android would trim or collapse.
            ' ' if first || last || prev == Some(' ') => out.push_str("\\u0020"),
            c if c != ' ' && (collapsible(c) || c.is_control()) => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c if xml => escape_xml_char(&mut out, c),
            c => out.push(c),
        }
        prev = Some(c);
    }
    out
}

/// `&` that does not start an entity or character reference becomes `&amp;`.
fn fix_ampersands(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.char_indices() {
        if c == '&' {
            let rest = &s[i + 1..];
            let len = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '#')).unwrap_or(rest.len());
            if len == 0 || !rest[len..].starts_with(';') {
                out.push_str("&amp;");
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn flatten(e: &Element, out: &mut Vec<Piece>) {
    for c in &e.children {
        match c {
            Node::Text(t) | Node::CData(t) => out.extend(t.chars().map(Piece::Ch)),
            Node::Element(child) => {
                out.push(Piece::Tag(open_tag(child)));
                if !child.children.is_empty() {
                    flatten(child, out);
                    out.push(Piece::Tag(format!("</{}>", child.name)));
                }
            }
            Node::Comment(_) | Node::Pi(_) => {}
        }
    }
}

/// Pieces of a translation of a styled value (`None` if it is not well-formed markup).
fn markup_pieces(t: &str) -> Option<Vec<Piece>> {
    let doc = xml::parse_str(&format!("<r>{}</r>", fix_ampersands(t))).ok()?;
    let mut out = Vec::new();
    flatten(&doc.root, &mut out);
    Some(out)
}

fn cdata_only(e: &Element) -> bool {
    e.children.iter().any(|c| matches!(c, Node::CData(_)))
        && e.children.iter().all(|c| matches!(c, Node::CData(_)) || matches!(c, Node::Text(t) if t.trim().is_empty()))
}

/// Replaces the value of a resource element with `text` (as shown to translators).
fn set_value(e: &mut Element, text: &str) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let markup = if e.has_element_children() { markup_pieces(&text) } else { None };
    let source = match markup {
        Some(pieces) => encode(&pieces, true),
        None => {
            let pieces: Vec<Piece> = text.chars().map(Piece::Ch).collect();
            if cdata_only(e) { format!("<![CDATA[{}]]>", encode(&pieces, false).replace("]]>", "]]]]><![CDATA[>")) } else { encode(&pieces, true) }
        }
    };
    e.children = match xml::parse_str(&format!("<r>{source}</r>")) {
        Ok(doc) => doc.root.children,
        Err(_) => vec![Node::Text(text)],
    };
}

// ---------------------------------------------------------------------------------------------
// Scan

/// A blank line between two nodes of the resources element.
fn blank_line(t: &str) -> bool {
    t.trim().is_empty() && t.matches('\n').count() >= 2
}

/// Comment lines worth a note (not just separators such as `=====`).
fn note_text(c: &str) -> Option<String> {
    let lines: Vec<&str> = c.lines().map(str::trim).filter(|l| l.chars().any(char::is_alphanumeric)).collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn join_notes(a: &str, b: &str) -> Option<String> {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => None,
        (false, true) => Some(a.to_string()),
        (true, false) => Some(b.to_string()),
        (false, false) => Some(format!("{a}\n{b}")),
    }
}

/// Scans a values file. `langs` are the target languages: plurals get rows for the quantities
/// they need beyond those of the original.
pub fn scan(bytes: &[u8], langs: &[String]) -> Result<Vec<ScannedString>> {
    let doc = parse(bytes)?;
    let extra: BTreeSet<&str> = langs.iter().filter_map(|l| plural_categories(l)).flatten().copied().collect();
    let mut out = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for node in &doc.root.children {
        match node {
            Node::Comment(c) => notes.extend(note_text(c)),
            Node::Text(t) if blank_line(t) => notes.clear(),
            Node::Element(e) => scan_element(e, &std::mem::take(&mut notes).join("\n"), &extra, &mut out),
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    out.retain(|s: &ScannedString| seen.insert(s.id.clone()));
    Ok(out)
}

fn scan_element(e: &Element, note: &str, extra: &BTreeSet<&str>, out: &mut Vec<ScannedString>) {
    let (Some(kind), Some(id)) = (kind(e), base_id(e)) else { return };
    if !translatable(e) {
        return;
    }
    let mut push = |id: String, item: &Element, extra_note: &str| {
        if is_reference(item) {
            return;
        }
        let text = decode(item);
        if !text.is_empty() {
            out.push(ScannedString { id, text, comment: join_notes(note, extra_note) });
        }
    };
    match kind {
        Kind::String => push(id, e, ""),
        Kind::Array => {
            for (i, item) in array_items(e).enumerate() {
                push(format!("{id}[{i}]"), item, "");
            }
        }
        Kind::Plurals => {
            let items = plural_items(e);
            let fallback = fallback_item(&items);
            for q in QUANTITIES {
                match items.iter().find(|(x, _)| *x == q) {
                    Some((_, item)) => push(format!("{id}[{q}]"), item, ""),
                    None if extra.contains(q) => {
                        if let Some(f) = fallback {
                            push(format!("{id}[{q}]"), f, &format!("Plural quantity \"{q}\" (not in the original, which shows the text of \"other\")"));
                        }
                    }
                    None => {}
                }
            }
        }
    }
}

/// Non-empty values by row id (used to import existing localized files).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(scan(bytes, &[])?.into_iter().map(|s| (s.id, s.text)).collect())
}

// ---------------------------------------------------------------------------------------------
// Build

/// Builds the localized values file for `lang`. `resolve(id, original)` returns the text to write
/// or `None` to omit the item.
pub fn build(original: &[u8], lang: &str, resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let mut doc = parse(original)?;
    let old = std::mem::take(&mut doc.root.children);
    let mut out: Vec<Node> = Vec::with_capacity(old.len());
    // Whitespace and comments since the last element or blank line (they belong to the next element).
    let mut pending: Vec<Node> = Vec::new();
    for node in old {
        match node {
            Node::Element(e) => match localize(e, lang, resolve) {
                Some(e) => {
                    out.append(&mut pending);
                    out.push(Node::Element(e));
                }
                None => pending.clear(),
            },
            Node::Text(t) if blank_line(&t) => {
                out.append(&mut pending);
                pending.push(Node::Text(t));
            }
            other => pending.push(other),
        }
    }
    out.append(&mut pending);
    doc.root.children = out;
    Ok(xml::write_document(&doc))
}

fn localize(mut e: Element, lang: &str, resolve: &dyn Fn(&str, &str) -> Option<String>) -> Option<Element> {
    let kind = kind(&e)?;
    let id = base_id(&e)?;
    if !translatable(&e) {
        return None;
    }
    let item_text = |item: &mut Element, id: &str| -> bool {
        if is_reference(item) {
            return false;
        }
        let orig = decode(item);
        match resolve(id, &orig).filter(|_| !orig.is_empty()) {
            Some(t) => {
                set_value(item, &t);
                true
            }
            None => false,
        }
    };
    match kind {
        Kind::String => item_text(&mut e, &id).then_some(e),
        Kind::Array => {
            let mut any = false;
            let items = e.children.iter_mut().filter_map(|c| match c {
                Node::Element(item) if item.name == "item" => Some(item),
                _ => None,
            });
            for (i, item) in items.enumerate() {
                any |= item_text(item, &format!("{id}[{i}]"));
            }
            any.then_some(e)
        }
        Kind::Plurals => {
            let items = plural_items(&e);
            let fallback = fallback_item(&items)?.clone();
            let wanted: Vec<&str> = match plural_categories(lang) {
                Some(cats) => QUANTITIES.iter().copied().filter(|q| cats.contains(q) || *q == "other").collect(),
                None => QUANTITIES.iter().copied().filter(|q| items.iter().any(|(x, _)| x == q)).collect(),
            };
            let mut new_items = Vec::new();
            let mut any = false;
            for q in wanted {
                let mut item = items.iter().find(|(x, _)| *x == q).map_or_else(|| fallback.clone(), |(_, el)| (*el).clone());
                item.attrs.set("quantity", q);
                any |= item_text(&mut item, &format!("{id}[{q}]"));
                new_items.push(item);
            }
            if !any {
                return None;
            }
            // Keep the indentation of the original items.
            let first = e.children.iter().position(|c| matches!(c, Node::Element(_)));
            let indent = first.and_then(|i| i.checked_sub(1)).and_then(|i| e.children.get(i)).cloned().filter(|n| matches!(n, Node::Text(_)));
            let closing = e.children.last().cloned().filter(|n| matches!(n, Node::Text(_)));
            e.children.clear();
            for item in new_items {
                e.children.extend(indent.clone());
                e.children.push(Node::Element(item));
            }
            e.children.extend(closing);
            Some(e)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Output patterns

/// Pattern placing the localized file in the resource directory `qualified` (with the other
/// qualifiers of the original's directory, e.g. `values-night` → `values-b+de-night`).
fn pattern_for(file_name: &str, qualified: &str) -> String {
    let mut parts = file_name.rsplit(['\\', '/']);
    parts.next();
    let dir = parts.next().unwrap_or("");
    let quals = dir.get(..6).filter(|p| p.eq_ignore_ascii_case("values")).map(|_| &dir[6..]).filter(|r| r.is_empty() || r.starts_with('-')).unwrap_or("");
    format!("<parent>\\{qualified}{quals}\\<file>")
}

/// Default localized file pattern: `<parent>\values-b+<android>\<file>`
/// (`res/values/strings.xml` → `res/values-b+de/strings.xml`).
pub fn default_pattern(file_name: &str) -> String {
    pattern_for(file_name, "values-b+<android>")
}

/// Legacy pattern `<parent>\values-<androidr>\<file>` when the resource directory next to the
/// original already holds localized copies in legacy directories (`values-de`, `values-pt-rBR`)
/// and none in BCP 47 directories (`values-b+de`).
pub fn existing_pattern(file: &Path) -> Option<String> {
    let name = file.file_name()?;
    let res = file.parent()?.parent()?;
    let mut legacy = false;
    for entry in std::fs::read_dir(res).ok()?.flatten() {
        let dir = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Some(quals) = dir.strip_prefix("values-") else { continue };
        if !entry.path().join(name).is_file() {
            continue;
        }
        let first = quals.split('-').next().unwrap_or("");
        if first.starts_with("b+") {
            return None;
        }
        // Two- or three-letter language codes (`car` is the UI mode qualifier).
        legacy |= matches!(first.len(), 2 | 3) && first != "car" && first.chars().all(|c| c.is_ascii_lowercase());
    }
    legacy.then(|| pattern_for(&file.to_string_lossy(), "values-<androidr>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<resources xmlns:xliff="urn:oasis:names:tc:xliff:document:1.2" xmlns:tools="http://schemas.android.com/tools">
    <!-- Copyright header -->

    <!-- Application name -->
    <string name="app_name">Converter</string>
    <string name="app_id" translatable="false">converter</string>
    <color name="accent">#FF0000</color>
    <!-- The action for submitting a form. This text is on a button that can fit 30 chars -->
    <string name="login_submit_button">Sign in</string>
    <string name="apostrophe">Don\'t \"stop\" &amp; go</string>
    <string name="quoted">"It's   spaced"</string>
    <string name="multi">First line\nSecond
        line</string>
    <string name="alias">@string/app_name</string>
    <string name="countdown">
      <xliff:g id="time" example="5 days">%1$s</xliff:g> until holiday
    </string>
    <string name="welcome">Welcome to <b>Android</b> &amp; more!</string>
    <string name="html"><![CDATA[<b>Bold</b> text]]></string>
    <string name="at">\@home</string>
    <string name="tablet" product="tablet">Tablet text</string>
    <string-array name="planets">
        <item>Mercury</item>
        <item>Venus</item>
    </string-array>
    <plurals name="songs">
        <!-- Always include %d in "one" -->
        <item quantity="one">%d song found.</item>
        <item quantity="other">%d songs found.</item>
    </plurals>
</resources>
"#;

    fn texts(s: &[ScannedString]) -> Vec<(&str, &str)> {
        s.iter().map(|x| (x.id.as_str(), x.text.as_str())).collect()
    }

    #[test]
    fn scan_strings_arrays_plurals() {
        let s = scan(SRC.as_bytes(), &[]).unwrap();
        assert_eq!(
            texts(&s),
            [
                ("app_name", "Converter"),
                ("login_submit_button", "Sign in"),
                ("apostrophe", "Don't \"stop\" & go"),
                ("quoted", "It's   spaced"),
                ("multi", "First line\nSecond line"),
                ("countdown", "<xliff:g id=\"time\" example=\"5 days\">%1$s</xliff:g> until holiday"),
                ("welcome", "Welcome to <b>Android</b> &amp; more!"),
                ("html", "<b>Bold</b> text"),
                ("at", "@home"),
                ("tablet[product=tablet]", "Tablet text"),
                ("planets[0]", "Mercury"),
                ("planets[1]", "Venus"),
                ("songs[one]", "%d song found."),
                ("songs[other]", "%d songs found."),
            ]
        );
        assert_eq!(s[0].comment.as_deref(), Some("Application name"));
        assert_eq!(s[1].comment.as_deref(), Some("The action for submitting a form. This text is on a button that can fit 30 chars"));
        assert_eq!(s[2].comment, None);
        assert!(scan(b"<root/>", &[]).is_err());
    }

    #[test]
    fn scan_adds_plural_quantities_of_target_languages() {
        let s = scan(SRC.as_bytes(), &["pl".into(), "ja".into()]).unwrap();
        let ids: Vec<&str> = s.iter().filter(|x| x.id.starts_with("songs")).map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["songs[one]", "songs[few]", "songs[many]", "songs[other]"]);
        let few = s.iter().find(|x| x.id == "songs[few]").unwrap();
        assert_eq!(few.text, "%d songs found.");
        assert!(few.comment.as_deref().unwrap().contains("\"few\""));
    }

    #[test]
    fn build_localized_file() {
        let resolve = |id: &str, _: &str| -> Option<String> {
            Some(
                match id {
                    "app_name" => "Konverter",
                    "apostrophe" => "Nicht \"stoppen\" & l'eau",
                    "quoted" => " zwei  Leerzeichen ",
                    "multi" => "Erste Zeile\nZweite Zeile",
                    "countdown" => "Noch <xliff:g id=\"time\" example=\"5 days\">%1$s</xliff:g> bis zum Urlaub",
                    "welcome" => "Willkommen bei <b>Android</b> & mehr!",
                    "html" => "<b>Fett</b> ]]> Text",
                    "at" => "@Haus",
                    "planets[1]" => "Venus (de)",
                    "songs[one]" => "%d Lied gefunden.",
                    _ => return None,
                }
                .to_string(),
            )
        };
        let out = String::from_utf8(build(SRC.as_bytes(), "de", &resolve).unwrap()).unwrap();
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources xmlns:xliff="), "{out}");
        assert!(out.contains("    <!-- Copyright header -->\n\n    <!-- Application name -->\n    <string name=\"app_name\">Konverter</string>\n"), "{out}");
        assert!(
            !out.contains("app_id") && !out.contains("accent") && !out.contains("login_submit") && !out.contains("alias") && !out.contains("tablet"),
            "{out}"
        );
        assert!(out.contains("<string name=\"apostrophe\">Nicht \\\"stoppen\\\" &amp; l\\'eau</string>"), "{out}");
        assert!(out.contains("<string name=\"quoted\">\\u0020zwei \\u0020Leerzeichen\\u0020</string>"), "{out}");
        assert!(out.contains("<string name=\"multi\">Erste Zeile\\nZweite Zeile</string>"), "{out}");
        assert!(out.contains("<string name=\"countdown\">Noch <xliff:g id=\"time\" example=\"5 days\">%1$s</xliff:g> bis zum Urlaub</string>"), "{out}");
        assert!(out.contains("<string name=\"welcome\">Willkommen bei <b>Android</b> &amp; mehr!</string>"), "{out}");
        assert!(out.contains("<string name=\"html\"><![CDATA[<b>Fett</b> ]]]]><![CDATA[> Text]]></string>"), "{out}");
        assert!(out.contains("<string name=\"at\">\\@Haus</string>"), "{out}");
        assert!(out.contains("<item>Mercury</item>\n        <item>Venus (de)</item>"), "{out}");
        assert!(out.contains("<plurals name=\"songs\">\n        <item quantity=\"one\">%d Lied gefunden.</item>\n        <item quantity=\"other\">%d songs found.</item>\n    </plurals>"), "{out}");
        assert!(out.ends_with("</plurals>\n</resources>\n"), "{out}");
        // The built file reads back to the translations.
        let back = scan(out.as_bytes(), &[]).unwrap();
        let get = |id: &str| back.iter().find(|s| s.id == id).map(|s| s.text.clone()).unwrap();
        for id in ["apostrophe", "quoted", "multi", "countdown", "at"] {
            let expected = resolve(id, "").unwrap();
            assert_eq!(get(id), expected, "{id}");
        }
        assert_eq!(get("welcome"), "Willkommen bei <b>Android</b> &amp; mehr!");
        assert_eq!(get("html"), "<b>Fett</b> ]]> Text");
    }

    #[test]
    fn build_plurals_for_target_language() {
        let src = "<resources>\n  <plurals name=\"n\">\n    <item quantity=\"one\">%d file</item>\n    <item quantity=\"other\">%d files</item>\n  </plurals>\n</resources>";
        let pl = |id: &str, _: &str| match id {
            "n[one]" => Some("%d plik".to_string()),
            "n[few]" => Some("%d pliki".to_string()),
            "n[many]" => Some("%d plików".to_string()),
            _ => None,
        };
        let out = String::from_utf8(build(src.as_bytes(), "pl", &pl).unwrap()).unwrap();
        assert_eq!(
            out,
            "<resources>\n  <plurals name=\"n\">\n    <item quantity=\"one\">%d plik</item>\n    <item quantity=\"few\">%d pliki</item>\n    <item quantity=\"many\">%d plików</item>\n    <item quantity=\"other\">%d files</item>\n  </plurals>\n</resources>\n"
        );
        let ja = |id: &str, _: &str| (id == "n[other]").then(|| "%d 個のファイル".to_string());
        let out = String::from_utf8(build(src.as_bytes(), "ja", &ja).unwrap()).unwrap();
        assert!(out.contains("<plurals name=\"n\">\n    <item quantity=\"other\">%d 個のファイル</item>\n  </plurals>"), "{out}");
        // Nothing translated: the whole plural is omitted.
        let out = String::from_utf8(build(src.as_bytes(), "pl", &|_, _| None).unwrap()).unwrap();
        assert_eq!(out, "<resources>\n</resources>\n");
    }

    #[test]
    fn markup_translation_that_is_not_xml_is_escaped() {
        let src = "<resources><string name=\"w\">Hi <b>you</b></string></resources>";
        let out = String::from_utf8(build(src.as_bytes(), "de", &|_, _| Some("a < b <i>".into())).unwrap()).unwrap();
        assert!(out.contains("<string name=\"w\">a &lt; b &lt;i&gt;</string>"), "{out}");
    }

    #[test]
    fn plural_rules() {
        assert_eq!(plural_categories("de"), Some(&["one", "other"][..]));
        assert_eq!(plural_categories("pt-BR"), Some(&["one", "other"][..]));
        assert_eq!(plural_categories("ru"), Some(&["one", "few", "many", "other"][..]));
        assert_eq!(plural_categories("zh.tra"), Some(&["other"][..]));
        assert_eq!(plural_categories("sis"), None);
    }

    #[test]
    fn patterns() {
        assert_eq!(default_pattern("res\\values\\strings.xml"), "<parent>\\values-b+<android>\\<file>");
        assert_eq!(default_pattern("app/src/main/res/values-night/arrays.xml"), "<parent>\\values-b+<android>-night\\<file>");
        assert_eq!(default_pattern("strings.xml"), "<parent>\\values-b+<android>\\<file>");
        let tmp = tempfile::tempdir().unwrap();
        let res = tmp.path().join("res");
        for d in ["values", "values-de", "values-night"] {
            std::fs::create_dir_all(res.join(d)).unwrap();
            std::fs::write(res.join(d).join("strings.xml"), "<resources/>").unwrap();
        }
        let file = res.join("values").join("strings.xml");
        assert_eq!(existing_pattern(&file).as_deref(), Some("<parent>\\values-<androidr>\\<file>"));
        std::fs::create_dir_all(res.join("values-b+fr")).unwrap();
        std::fs::write(res.join("values-b+fr").join("strings.xml"), "<resources/>").unwrap();
        assert_eq!(existing_pattern(&file), None);
    }
}
