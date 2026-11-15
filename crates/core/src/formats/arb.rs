//! Flutter/Dart Application Resource Bundles (`.arb`, e.g. `lib/l10n/app_en.arb`).
//!
//! An ARB file is a flat JSON object. Every string entry is a message (row id = key); values are
//! ICU MessageFormat strings, plurals and selects stay inside the message. `@key` objects hold the
//! message's metadata (`description` becomes the row's note), `@@` entries describe the file
//! (`@@locale`). Localized files (`app_de.arb`) contain the messages and the `@@` entries, with
//! `@@locale` set to the target language; the `@key` metadata stays in the template, which is
//! where Flutter's `gen-l10n` reads it.

use std::path::Path;

use serde_json::{Map, Value};

use super::ScannedString;
use crate::error::{Error, Result};

fn object(bytes: &[u8]) -> Result<(Map<String, Value>, String, super::TextStyle)> {
    match super::json::read(bytes)? {
        (Value::Object(m), text, style) => Ok((m, text, style)),
        _ => Err(Error::Format("not an ARB file (no JSON object)".into())),
    }
}

fn is_message(key: &str) -> bool {
    !key.starts_with('@')
}

pub fn scan(bytes: &[u8]) -> Result<Vec<ScannedString>> {
    let (m, ..) = object(bytes)?;
    Ok(m.iter()
        .filter(|(k, _)| is_message(k))
        .filter_map(|(k, v)| {
            let text = v.as_str()?;
            let comment = m
                .get(&format!("@{k}"))
                .and_then(|meta| meta.get("description"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string);
            Some(ScannedString { id: k.clone(), text: text.to_string(), comment })
        })
        .collect())
}

/// Translations of an existing localized file (taken over when the source is added).
pub fn translations(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    Ok(scan(bytes)?.into_iter().map(|s| (s.id, s.text)).collect())
}

/// Builds the localized file of `lang`. `resolve(key, original)` returns the message to write or
/// `None` to omit it.
pub fn build(original: &[u8], lang: &str, resolve: &dyn Fn(&str, &str) -> Option<String>) -> Result<Vec<u8>> {
    let (m, text, style) = object(original)?;
    let mut out = Map::new();
    for (k, v) in &m {
        if k == "@@locale" {
            out.insert(k.clone(), Value::String(crate::lang::iso(lang)));
        } else if k.starts_with("@@") {
            out.insert(k.clone(), v.clone());
        } else if let (true, Some(s)) = (is_message(k), v.as_str())
            && let Some(t) = resolve(k, s)
        {
            out.insert(k.clone(), Value::String(t));
        }
    }
    super::json::write_like(&Value::Object(out), &text, style)
}

/// Default localized file pattern: the locale suffix of the template is replaced
/// (`app_en.arb` → `app_<iso>.<ext>`, `en.arb` → `<iso>.<ext>`), otherwise appended.
pub fn default_pattern(file_name: &str) -> String {
    let base = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    let stem = Path::new(base).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if crate::lang::is_known(stem) { "<iso>.<ext>".into() } else { super::properties::default_pattern(file_name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "{\r\n  \"@@locale\": \"en\",\r\n  \"@@last_modified\": \"2026-11-01\",\r\n  \"helloWorld\": \"Hello World!\",\r\n  \"@helloWorld\": {\r\n    \"description\": \"The conventional newborn programmer greeting\"\r\n  },\r\n  \"nWombats\": \"{count, plural, =0{no wombats} one{1 wombat} other{{count} wombats}}\",\r\n  \"@nWombats\": {\r\n    \"description\": \"  \",\r\n    \"placeholders\": { \"count\": { \"type\": \"num\" } }\r\n  },\r\n  \"untranslated\": \"Later\"\r\n}\r\n";

    #[test]
    fn scans_messages_and_descriptions() {
        let s = scan(SRC.as_bytes()).unwrap();
        let rows: Vec<(&str, &str, Option<&str>)> = s.iter().map(|x| (x.id.as_str(), x.text.as_str(), x.comment.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("helloWorld", "Hello World!", Some("The conventional newborn programmer greeting")),
                ("nWombats", "{count, plural, =0{no wombats} one{1 wombat} other{{count} wombats}}", None),
                ("untranslated", "Later", None),
            ]
        );
        assert!(scan(b"[1]").is_err());
    }

    #[test]
    fn build_sets_locale_and_drops_metadata() {
        let out = build(SRC.as_bytes(), "pt-BR", &|k, _| match k {
            "helloWorld" => Some("Olá, mundo!".into()),
            "nWombats" => Some("{count, plural, =0{nenhum vombate} one{1 vombate} other{{count} vombates}}".into()),
            _ => None,
        })
        .unwrap();
        let s = String::from_utf8(out).unwrap();
        assert_eq!(
            s,
            "{\r\n  \"@@locale\": \"pt_BR\",\r\n  \"@@last_modified\": \"2026-11-01\",\r\n  \"helloWorld\": \"Olá, mundo!\",\r\n  \"nWombats\": \"{count, plural, =0{nenhum vombate} one{1 vombate} other{{count} vombates}}\"\r\n}\r\n"
        );
        assert_eq!(translations(s.as_bytes()).unwrap().len(), 2);
    }

    #[test]
    fn default_patterns() {
        assert_eq!(default_pattern("lib\\l10n\\app_en.arb"), "app_<iso>.<ext>");
        assert_eq!(default_pattern("intl_pt_BR.arb"), "intl_<iso>.<ext>");
        assert_eq!(default_pattern("l10n/en.arb"), "<iso>.<ext>");
        assert_eq!(default_pattern("strings.arb"), "<body>_<iso>.<ext>");
    }
}
