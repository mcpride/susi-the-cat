//! Pseudo translation (Sisulizer: Spalte | Pseudo-Übersetzung füllen / entfernen).
//!
//! Observed: pseudo languages are stored as `<lang id="ko" pseudo="1"><test type="ptEmulation"/></lang>`
//! and pseudo cells carry `test="1"`. The transformation rules follow the help topic
//! "Pseudo Translation" (emulation with diacritics/expand/enclose/case, replace, reduce).

use serde::{Deserialize, Serialize};

use crate::model::{Project, RowRef};
use crate::status::TranslationStatus;
use crate::xml::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PseudoKind {
    #[default]
    Emulation,
    Replace,
    Reduce,
    MaxTranslation,
}

impl PseudoKind {
    /// Value of `<test type="...">`.
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Emulation => "ptEmulation",
            Self::Replace => "ptReplace",
            Self::Reduce => "ptReduce",
            Self::MaxTranslation => "ptMaxTranslation",
        }
    }

    pub fn from_type_name(s: &str) -> Option<Self> {
        [Self::Emulation, Self::Replace, Self::Reduce, Self::MaxTranslation].into_iter().find(|k| k.type_name().eq_ignore_ascii_case(s))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Emulation => crate::i18n::t("Emulation"),
            Self::Replace => crate::i18n::t("Replace"),
            Self::Reduce => crate::i18n::t("Reduce"),
            Self::MaxTranslation => crate::i18n::t("Longest translation"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PseudoCase {
    #[default]
    NoChange,
    Alternate,
    Upper,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PseudoOptions {
    pub kind: PseudoKind,
    pub diacritics: bool,
    pub expand: bool,
    pub enclose: bool,
    pub case: PseudoCase,
    pub begin: char,
    pub end: char,
    pub expand_char: char,
    pub last_char: char,
    /// Expansion in percent for lengths 1–5, 6–10, 11–20, 21–30, 31–50, >50.
    pub expand_percent: [u32; 6],
    pub replace_char: char,
    /// Reduce: `None` = keep first character, `Some(s)` = replace by `s`.
    pub reduce_to: Option<String>,
}

impl Default for PseudoOptions {
    fn default() -> Self {
        Self {
            kind: PseudoKind::Emulation,
            diacritics: true,
            expand: true,
            enclose: true,
            case: PseudoCase::NoChange,
            begin: '{',
            end: '}',
            expand_char: '_',
            last_char: 'X',
            expand_percent: [100, 80, 60, 40, 30, 20],
            replace_char: '.',
            reduce_to: None,
        }
    }
}

fn accent(c: char) -> char {
    const FROM: &str = "aeiouyAEIOUYcCnNsSzZ";
    const TO: &str = "àéîöüýÀÉÎÖÜÝçÇñÑšŠžŽ";
    FROM.chars().position(|f| f == c).and_then(|i| TO.chars().nth(i)).unwrap_or(c)
}

/// Placeholders and escape sequences that must survive pseudo translation unchanged.
fn protected_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let end = match b[i] {
            b'{' => s[i..].find('}').map(|e| i + e + 1),
            b'%' if i + 1 < b.len() => {
                let rest = &s[i + 1..];
                let n = rest.find(|c: char| c.is_ascii_alphabetic() || c == '%').map(|e| e + 1).filter(|&e| e <= 8);
                n.map(|e| i + 1 + e)
            }
            b'<' => s[i..].find('>').map(|e| i + e + 1),
            b'&' => s[i..].find(';').filter(|&e| e <= 8).map(|e| i + e + 1),
            b'\\' if i + 1 < b.len() => Some(i + 2),
            _ => None,
        };
        match end {
            Some(e) if e <= b.len() && s.is_char_boundary(e) => {
                spans.push((i, e));
                i = e;
            }
            _ => i += s[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    spans
}

fn map_unprotected(s: &str, mut f: impl FnMut(char) -> String) -> String {
    let spans = protected_spans(s);
    let mut out = String::with_capacity(s.len() * 2);
    let mut pos = 0;
    for (a, e) in spans.into_iter().chain(std::iter::once((s.len(), s.len()))) {
        for c in s[pos..a].chars() {
            out.push_str(&f(c));
        }
        out.push_str(&s[a..e]);
        pos = e;
    }
    out
}

/// Pseudo-translates a single string.
pub fn pseudo_text(original: &str, opts: &PseudoOptions) -> String {
    match opts.kind {
        PseudoKind::Replace => map_unprotected(original, |c| if c.is_whitespace() { c.to_string() } else { opts.replace_char.to_string() }),
        PseudoKind::Reduce => match &opts.reduce_to {
            Some(s) => s.clone(),
            None => original.chars().next().map(String::from).unwrap_or_default(),
        },
        PseudoKind::Emulation | PseudoKind::MaxTranslation => emulate(original, opts),
    }
}

fn emulate(original: &str, opts: &PseudoOptions) -> String {
    let mut upper_next = true;
    let mut s = map_unprotected(original, |c| {
        let c = match opts.case {
            PseudoCase::NoChange => c,
            PseudoCase::Upper => c.to_uppercase().next().unwrap_or(c),
            PseudoCase::Alternate if c.is_alphabetic() => {
                let r = if upper_next { c.to_uppercase().next() } else { c.to_lowercase().next() };
                upper_next = !upper_next;
                r.unwrap_or(c)
            }
            PseudoCase::Alternate => c,
        };
        if opts.diacritics { accent(c).to_string() } else { c.to_string() }
    });
    if opts.expand {
        let n = original.chars().count();
        let pct = match n {
            0 => 0,
            1..=5 => opts.expand_percent[0],
            6..=10 => opts.expand_percent[1],
            11..=20 => opts.expand_percent[2],
            21..=30 => opts.expand_percent[3],
            31..=50 => opts.expand_percent[4],
            _ => opts.expand_percent[5],
        } as usize;
        let extra = (n * pct).div_ceil(100);
        if extra > 0 {
            s.extend(std::iter::repeat_n(opts.expand_char, extra - 1));
            s.push(opts.last_char);
        }
    }
    if opts.enclose {
        s = format!("{}{s}{}", opts.begin, opts.end);
    }
    s
}

/// Marks `lang` as pseudo language (`pseudo="1"` + `<test type>`). Adds the language if missing.
pub fn set_pseudo_language(p: &mut Project, lang: &str, kind: PseudoKind) {
    p.add_language(lang);
    if let Some(l) = p.languages.iter_mut().find(|l| l.id() == lang) {
        l.attrs.set("pseudo", "1");
        l.children.retain(|c| c.name != "test");
        let mut t = Element::new("test");
        t.attrs.set("type", kind.type_name());
        l.children.push(t);
    }
}

/// Fills pseudo translations for `lang`. Only empty cells and existing pseudo cells are written
/// unless `overwrite` is set. Returns the number of cells written.
pub fn fill_pseudo(p: &mut Project, lang: &str, opts: &PseudoOptions, rows: Option<&[RowRef]>, overwrite: bool) -> usize {
    let refs = rows.map(<[RowRef]>::to_vec).unwrap_or_else(|| p.rows());
    let langs = p.language_ids();
    let mut n = 0;
    for r in refs {
        if p.original_lang(&r) == lang {
            continue;
        }
        let Some(row) = p.row_mut(&r) else {
            continue;
        };
        if row.locked() || row.excluded() || row.raw || row.original().is_empty() {
            continue;
        }
        let is_pseudo_cell = row.translation(lang).is_some_and(|t| t.attrs.get("test") == Some("1"));
        if !overwrite && !is_pseudo_cell && row.tstatus(lang) != TranslationStatus::NotTranslated {
            continue;
        }
        let base = if opts.kind == PseudoKind::MaxTranslation {
            langs.iter().filter_map(|l| row.text(l)).chain(std::iter::once(row.original())).max_by_key(|t| t.chars().count()).unwrap_or("").to_string()
        } else {
            row.original().to_string()
        };
        let text = pseudo_text(&base, opts);
        row.set_translation(lang, &text, TranslationStatus::AutoTranslated);
        if let Some(t) = row.translation_mut(lang) {
            t.attrs.set("test", "1");
        }
        n += 1;
    }
    n
}

/// Removes all pseudo cells (`test="1"`) of `lang` (all languages if `None`).
pub fn remove_pseudo(p: &mut Project, lang: Option<&str>) -> usize {
    let mut n = 0;
    for r in p.rows() {
        if let Some(row) = p.row_mut(&r) {
            let before = row.translations.len();
            row.translations.retain(|t| !(t.attrs.get("test") == Some("1") && lang.is_none_or(|l| t.lang() == l)));
            n += before - row.translations.len();
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulation_keeps_placeholders() {
        let o = PseudoOptions::default();
        let s = pseudo_text("Hello {0}", &o);
        assert!(s.starts_with("{Héllö {0}"), "{s}");
        assert!(s.ends_with("X}"));
        let r = PseudoOptions { kind: PseudoKind::Replace, ..Default::default() };
        assert_eq!(pseudo_text("Hi %s you", &r), ".. %s ...");
        let d = PseudoOptions { kind: PseudoKind::Reduce, ..Default::default() };
        assert_eq!(pseudo_text("Hello", &d), "H");
    }
}
