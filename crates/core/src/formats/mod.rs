//! File format scanners and builders.

pub mod json;
pub mod mo;
pub mod po;
pub mod properties;
pub mod resx;
pub mod tsv;

use crate::model::{Row, WriteMethod};

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
