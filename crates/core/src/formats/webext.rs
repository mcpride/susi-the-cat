//! Browser extension messages (Chrome/Firefox/Edge WebExtensions, `_locales/<lang>/messages.json`).
//!
//! Each top-level entry `{"message": "...", "description": "...", "placeholders": {...}}` is a
//! row (row id = key, text = `message`, note = `description`). Localized files
//! (`_locales/de/messages.json`) are copies of the original's entries with the translated
//! `message`; descriptions and placeholder definitions are kept, as every locale file needs the
//! placeholders its messages use (`$USER$`).

use std::path::Path;

use serde_json::{Map, Value};

use super::ScannedString;
use crate::error::{Error, Result};

/// Whether `path` is a WebExtension messages file: `messages.json` in a directory below `_locales`.
pub fn is_messages_file(path: &Path) -> bool {
    // Source names use `\` on every platform.
    let s = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let mut parts = s.rsplit('/');
    parts.next() == Some("messages.json") && parts.next().is_some_and(|l| !l.is_empty()) && parts.next() == Some("_locales")
}

fn object(bytes: &[u8]) -> Result<(Map<String, Value>, String, super::TextStyle)> {
    match super::json::read(bytes)? {
        (Value::Object(m), text, style) => Ok((m, text, style)),
        _ => Err(Error::Format("not a WebExtension messages file (no JSON object)".into())),
    }
}

fn message(v: &Value) -> Option<&str> {
    v.get("message").and_then(Value::as_str)
}

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let (m, ..) = object(bytes)?;
    Ok(m.iter()
        .filter_map(|(k, v)| {
            let comment = v.get("description").and_then(Value::as_str).map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
            Some(ScannedString { id: k.clone(), text: message(v)?.to_string(), comment })
        })
        .collect())
}

/// Translations of an existing localized file (taken over when the source is added).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(scan(bytes)?.into_iter().map(|s| (s.id, s.text)).collect())
}

/// Builds a localized messages file. `resolve(key, original)` returns the message to write or
/// `None` to omit the entry (the browser then falls back to the default locale).
pub fn build(original: &[u8], resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let (m, text, style) = object(original)?;
    let mut out = Map::new();
    for (k, v) in &m {
        let Some(t) = message(v).and_then(|orig| resolve(k, orig)) else { continue };
        let mut entry = v.clone();
        if let Some(e) = entry.as_object_mut() {
            e.insert("message".into(), Value::String(t));
        }
        out.insert(k.clone(), entry);
    }
    super::json::write_like(&Value::Object(out), &text, style)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "{\n    \"extName\": {\n        \"message\": \"Tab Saver\",\n        \"description\": \"Name of the extension.\"\n    },\n    \"greeting\": {\n        \"message\": \"Hello, $USER$!\",\n        \"placeholders\": {\n            \"user\": { \"content\": \"$1\", \"example\": \"Cira\" }\n        }\n    },\n    \"later\": { \"message\": \"Later\" }\n}\n";

    #[test]
    fn detects_messages_files() {
        assert!(is_messages_file(Path::new("ext/_locales/en/messages.json")));
        assert!(is_messages_file(Path::new("ext\\_locales\\en_GB\\Messages.json")));
        assert!(!is_messages_file(Path::new("ext/i18n/en/messages.json")));
        assert!(!is_messages_file(Path::new("_locales/en/strings.json")));
        assert!(!is_messages_file(Path::new("messages.json")));
    }

    #[test]
    fn scans_messages_and_descriptions() {
        let s = scan(SRC.as_bytes()).unwrap();
        let rows: Vec<(&str, &str, Option<&str>)> = s.iter().map(|x| (x.id.as_str(), x.text.as_str(), x.comment.as_deref())).collect();
        assert_eq!(rows, [("extName", "Tab Saver", Some("Name of the extension.")), ("greeting", "Hello, $USER$!", None), ("later", "Later", None)]);
    }

    #[test]
    fn build_keeps_descriptions_and_placeholders() {
        let out = build(SRC.as_bytes(), &|k, _| match k {
            "extName" => Some("Tab-Sicherer".into()),
            "greeting" => Some("Hallo, $USER$!".into()),
            _ => None,
        })
        .unwrap();
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "extName": { "message": "Tab-Sicherer", "description": "Name of the extension." },
                "greeting": { "message": "Hallo, $USER$!", "placeholders": { "user": { "content": "$1", "example": "Cira" } } }
            })
        );
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("{\n    \"extName\": {\n        \"message\""), "{s}");
        assert_eq!(translations(s.as_bytes()).unwrap().len(), 2);
    }
}
