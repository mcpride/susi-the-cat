//! File format scanners and builders.

pub mod android;
pub mod arb;
pub mod json;
pub mod mo;
pub mod po;
pub mod properties;
pub mod resx;
pub mod ts;
pub mod tsv;
pub mod webext;

use std::path::Path;

use crate::model::{Row, WriteMethod};

/// Whether `s` reads as a locale: a known language id (`de`, `pt_BR`) or a known language with
/// region/script subtags (`de_DE`, `zh_Hant_TW`, `es_419`).
fn looks_like_locale(s: &str) -> bool {
    if crate::lang::is_known(s) {
        return true;
    }
    let mut parts = s.split(['_', '-']);
    let primary = parts.next().unwrap_or("");
    let subtags: Vec<&str> = parts.collect();
    let subtag = |p: &&str| match p.len() {
        2 | 4 => p.chars().all(|c| c.is_ascii_alphabetic()),
        3 => p.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    };
    matches!(primary.len(), 2 | 3) && crate::lang::is_known(primary) && !subtags.is_empty() && subtags.iter().all(subtag)
}

/// Default pattern of files named after their locale (Flutter ARB, Qt TS): the locale of the
/// original is replaced (`app_en.arb` → `app_<iso>.<ext>`, `app_de_DE.ts` → `app_<iso>.<ext>`,
/// `en.arb` → `<iso>.<ext>`), otherwise appended (`<body>_<iso>.<ext>`).
pub fn locale_suffix_pattern(file_name: &str) -> String {
    let base = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    let stem = Path::new(base).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if looks_like_locale(stem) {
        return "<iso>.<ext>".into();
    }
    stem.match_indices('_')
        .map(|(i, _)| i)
        .find(|&i| looks_like_locale(&stem[i + 1..]))
        .map_or_else(|| "<body>_<iso>.<ext>".to_string(), |i| format!("{}_<iso>.<ext>", &stem[..i]))
}

/// A localizable string found in an original file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedString {
    pub id: String,
    pub text: String,
    /// Developer comment from the file (RESX `<comment>`), if any.
    pub comment: Option<String>,
}

/// Decides what is written for one item into a localized file.
///
/// Returns `None` when the item must be omitted (write methods Different/Translated).
pub fn select_output(method: WriteMethod, row: Option<&Row>, lang: &str, original: &str) -> Option<String> {
    let translated = row.and_then(|r| r.build_text(lang));
    match method {
        WriteMethod::All => Some(translated.unwrap_or(original).to_string()),
        WriteMethod::Translated => translated.map(str::to_string),
        WriteMethod::Different => translated.filter(|t| *t != original).map(str::to_string),
    }
}

/// Line-ending / BOM conventions of an original text file, reproduced in localized files.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextStyle {
    pub bom: bool,
    pub crlf: bool,
    pub trailing_newline: bool,
}

impl TextStyle {
    pub fn detect(bytes: &[u8]) -> Self {
        let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let crlf = bytes.windows(2).any(|w| w == b"\r\n");
        let trailing_newline = bytes.ends_with(b"\n");
        Self { bom, crlf, trailing_newline }
    }

    pub fn apply(self, text: &str) -> Vec<u8> {
        let mut s = text.replace("\r\n", "\n");
        if self.trailing_newline && !s.ends_with('\n') {
            s.push('\n');
        }
        if self.crlf {
            s = s.replace('\n', "\r\n");
        }
        let mut out = Vec::with_capacity(s.len() + 3);
        if self.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(s.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_suffix_patterns() {
        assert_eq!(locale_suffix_pattern("lib\\l10n\\app_en.arb"), "app_<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("intl_pt_BR.arb"), "intl_<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("i18n/myapp_de_DE.ts"), "myapp_<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("app_ui_zh_Hant_TW.ts"), "app_ui_<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("l10n/en.arb"), "<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("es_419.ts"), "<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("strings.arb"), "<body>_<iso>.<ext>");
        assert_eq!(locale_suffix_pattern("app_core.ts"), "<body>_<iso>.<ext>");
    }
}
