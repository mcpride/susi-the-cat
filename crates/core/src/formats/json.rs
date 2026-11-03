//! JSON source files (i18next / ngx-translate style key/value trees).
//!
//! Row ids are the dotted key paths of string leaves (e.g. `BUTTON.APPLY`); 
//! array elements use their index as path segment. Non-string values are not
//! localizable and are copied unchanged (write method All) or omitted.

use serde::Serialize;
use serde_json::{Map, Value};

use super::{ScannedString, TextStyle};
use crate::error::{Error, Result};

const MAX_DEPTH: usize = 256;

pub fn parse(bytes: &[u8]) -> Result<Value> {
    let (text, _) = crate::xml::decode_text(bytes)?;
    serde_json::from_str(&text).map_err(|e| Error::Json(e.to_string()))
}

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let v = parse(bytes)?;
    let mut out = Vec::new();
    walk(&v, "", &mut out, 0)?;
    Ok(out)
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") }
}

fn walk(v: &Value, path: &str, out: &mut Vec<ScannedString>, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Json("nesting too deep".into()));
    }
    match v {
        Value::String(s) => out.push(ScannedString { id: path.to_string(), text: s.clone(), comment: None }),
        Value::Object(m) => {
            for (k, child) in m {
                walk(child, &join(path, k), out, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for (i, child) in a.iter().enumerate() {
                walk(child, &join(path, &i.to_string()), out, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Detects the indentation unit of a pretty-printed JSON text (defaults to two spaces).
fn detect_indent(text: &str) -> String {
    for line in text.lines().skip(1) {
        let ws: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        if !ws.is_empty() {
            return ws;
        }
    }
    "  ".into()
}

/// Builds a localized JSON file. `resolve(id, original)` returns the text to write, or `None` to
/// omit the item. `keep_non_strings` copies numbers/booleans/null (write method All).
pub fn build(original: &[u8], resolve: &dyn Fn(&str, &str) -> Option<String>, keep_non_strings: bool) -> Result<Vec<u8>> {
    let style = TextStyle::detect(original);
    let (text, _) = crate::xml::decode_text(original)?;
    let v: Value = serde_json::from_str(&text).map_err(|e| Error::Json(e.to_string()))?;
    let out = match transform(&v, "", resolve, keep_non_strings, 0)? {
        Some(o) => o,
        None if v.is_array() => Value::Array(Vec::new()),
        None => Value::Object(Map::new()),
    };
    let indent = detect_indent(&text);
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    out.serialize(&mut ser).map_err(|e| Error::Json(e.to_string()))?;
    let s = String::from_utf8(buf).map_err(|e| Error::Json(e.to_string()))?;
    Ok(style.apply(&s))
}

fn transform(v: &Value, path: &str, resolve: &dyn Fn(&str, &str) -> Option<String>, keep: bool, depth: usize) -> Result<Option<Value>> {
    if depth > MAX_DEPTH {
        return Err(Error::Json("nesting too deep".into()));
    }
    Ok(match v {
        Value::String(s) => resolve(path, s).map(Value::String),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, child) in m {
                if let Some(c) = transform(child, &join(path, k), resolve, keep, depth + 1)? {
                    out.insert(k.clone(), c);
                }
            }
            if out.is_empty() && !m.is_empty() && depth > 0 { None } else { Some(Value::Object(out)) }
        }
        Value::Array(a) => {
            let mut out = Vec::new();
            for (i, child) in a.iter().enumerate() {
                match transform(child, &join(path, &i.to_string()), resolve, keep, depth + 1)? {
                    Some(c) => out.push(c),
                    // Arrays keep their positions: fall back to the original element.
                    None if keep => out.push(child.clone()),
                    None => {}
                }
            }
            if out.is_empty() && !a.is_empty() && depth > 0 { None } else { Some(Value::Array(out)) }
        }
        other if keep => Some(other.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &[u8] = b"{\r\n    \"BUTTON\": {\r\n        \"APPLY\": \"Apply\",\r\n        \"OK\": \"OK\"\r\n    },\r\n    \"count\": 3,\r\n    \"list\": [\"a\", \"b\"]\r\n}\r\n";

    #[test]
    fn scan_paths() {
        let s = scan(SRC).unwrap();
        let ids: Vec<_> = s.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["BUTTON.APPLY", "BUTTON.OK", "list.0", "list.1"]);
    }

    #[test]
    fn build_all_keeps_structure_and_style() {
        let out = build(SRC, &|id, orig| Some(if id == "BUTTON.APPLY" { "Übernehmen".into() } else { orig.into() }), true).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("{\r\n    \"BUTTON\": {\r\n        \"APPLY\": \"Übernehmen\""), "{s}");
        assert!(s.contains("\"count\": 3"));
        assert!(s.ends_with("}\r\n"));
    }

    #[test]
    fn build_translated_prunes() {
        let out = build(SRC, &|id, _| (id == "BUTTON.OK").then(|| "Gut".to_string()), false).unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v, serde_json::json!({"BUTTON": {"OK": "Gut"}}));
        let out = build(SRC, &|_, _| None, false).unwrap();
        assert_eq!(String::from_utf8(out).unwrap().trim(), "{}");
    }

    #[test]
    fn invalid_json_is_error() {
        assert!(scan(b"{").is_err());
    }
}
