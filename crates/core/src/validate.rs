//! Translation validation (Prüfen | Übersetzung prüfen).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::i18n::t;
use crate::model::{Project, RowRef};
use crate::status::TranslationStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationKind {
    MissingTranslation,
    Format,
    Tags,
    WhiteSpace,
    NewLine,
    MissingPeriod,
    MissingColon,
    MissingExclamation,
    MissingQuestion,
    CharacterLength,
    AllSpaces,
    HotkeyCount,
    DuplicateTranslation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

impl ValidationKind {
    pub const ALL: [ValidationKind; 13] = [
        Self::MissingTranslation,
        Self::Format,
        Self::Tags,
        Self::WhiteSpace,
        Self::NewLine,
        Self::MissingPeriod,
        Self::MissingColon,
        Self::MissingExclamation,
        Self::MissingQuestion,
        Self::CharacterLength,
        Self::AllSpaces,
        Self::HotkeyCount,
        Self::DuplicateTranslation,
    ];

    /// Label in the current UI language (validation names).
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingTranslation => t("Missing translation"),
            Self::Format => t("Inconsistent format string"),
            Self::Tags => t("Inconsistent tags"),
            Self::WhiteSpace => t("Invalid number of spaces"),
            Self::NewLine => t("Invalid number of line breaks"),
            Self::MissingPeriod => t("Missing period"),
            Self::MissingColon => t("Missing colon"),
            Self::MissingExclamation => t("Missing exclamation mark"),
            Self::MissingQuestion => t("Missing question mark"),
            Self::CharacterLength => t("Length in characters"),
            Self::AllSpaces => t("Translation consists of spaces only"),
            Self::HotkeyCount => t("Invalid number of hotkeys"),
            Self::DuplicateTranslation => t("Duplicate translation"),
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::MissingTranslation => "missing",
            Self::Format => "format",
            Self::Tags => "tags",
            Self::WhiteSpace => "whitespace",
            Self::NewLine => "newline",
            Self::MissingPeriod => "period",
            Self::MissingColon => "colon",
            Self::MissingExclamation => "exclamation",
            Self::MissingQuestion => "question",
            Self::CharacterLength => "length",
            Self::AllSpaces => "allspaces",
            Self::HotkeyCount => "hotkey",
            Self::DuplicateTranslation => "duplicate",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.keyword().eq_ignore_ascii_case(s.trim()))
    }

    pub fn severity(self) -> Severity {
        match self {
            Self::Format | Self::Tags | Self::CharacterLength | Self::AllSpaces => Severity::Error,
            _ => Severity::Warning,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    #[serde(skip)]
    pub row: RowRef,
    pub source: String,
    pub context: String,
    pub lang: String,
    pub kind: ValidationKind,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ValidateOptions {
    pub langs: Vec<String>,
    /// Checks to run; empty = all except `MissingTranslation`.
    pub kinds: Vec<ValidationKind>,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self { langs: Vec::new(), kinds: ValidationKind::ALL.into_iter().filter(|k| *k != ValidationKind::MissingTranslation).collect() }
    }
}

/// Format placeholders: .NET `{0}`/`{0:N2}`/`{name}`, ICU/i18next `{{x}}`, printf `%s`/`%1$d`, `%%` excluded.
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (pos, c) = chars[i];
        if c == '{' {
            if chars.get(i + 1).is_some_and(|(_, n)| *n == '{') {
                if let Some(e) = s[pos..].find("}}") {
                    out.push(s[pos..pos + e + 2].split_whitespace().collect());
                    i = chars.iter().position(|(p, _)| *p >= pos + e + 2).unwrap_or(chars.len());
                    continue;
                }
            } else if let Some(e) = s[pos..].find('}') {
                let inner = &s[pos + 1..pos + e];
                if !inner.is_empty() && !inner.contains('{') && !inner.contains(char::is_whitespace) {
                    out.push(format!("{{{inner}}}"));
                    i = chars.iter().position(|(p, _)| *p > pos + e).unwrap_or(chars.len());
                    continue;
                }
            }
        } else if c == '%' {
            let rest = &s[pos + 1..];
            if rest.starts_with('%') {
                i += 2;
                continue;
            }
            let spec_len = rest.find(|ch: char| !(ch.is_ascii_digit() || "$-+#.lhz".contains(ch))).unwrap_or(rest.len());
            if let Some(conv) = rest[spec_len..].chars().next().filter(|ch| "sdifuxXcpeEgGo@".contains(*ch)) {
                out.push(format!("%{}{}", &rest[..spec_len], conv));
                i += 1 + spec_len + 1;
                continue;
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// Markup tags (`<b>`, `</b>`, `<br/>`) sorted.
pub fn tags(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find('<') {
        let after = &rest[a + 1..];
        let Some(e) = after.find('>') else {
            break;
        };
        let inner = &after[..e];
        let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if !name.is_empty() && !inner.contains('<') {
            let closing = if inner.starts_with('/') { "/" } else { "" };
            out.push(format!("<{closing}{}>", name.to_ascii_lowercase()));
        }
        rest = &after[e + 1..];
    }
    out.sort();
    out
}

fn lead_trail(s: &str) -> (usize, usize) {
    let lead = s.chars().take_while(|c| *c == ' ' || *c == '\t' || *c == '\u{3000}').count();
    let trail = s.chars().rev().take_while(|c| *c == ' ' || *c == '\t' || *c == '\u{3000}').count();
    (lead, trail)
}

fn newlines(s: &str) -> usize {
    s.replace("\r\n", "\n").matches(['\n', '\r']).count()
}

fn hotkeys(s: &str) -> usize {
    let b: Vec<char> = s.chars().collect();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == '&' {
            match b.get(i + 1) {
                Some('&') => i += 1,
                Some(c) if c.is_alphanumeric() => n += 1,
                _ => {}
            }
        }
        i += 1;
    }
    n
}

/// End punctuation equivalents (incl. full-width, Arabic, Greek/Spanish forms).
fn ends_with_any(s: &str, set: &[char]) -> bool {
    s.trim_end().chars().last().is_some_and(|c| set.contains(&c))
}

fn check_pair(orig: &str, tr: &str, max_chars: usize, kinds: &[ValidationKind]) -> Vec<(ValidationKind, String)> {
    let mut out = Vec::new();
    let on = |k: ValidationKind| kinds.contains(&k);
    if on(ValidationKind::AllSpaces) && tr.trim().is_empty() && !orig.trim().is_empty() {
        out.push((ValidationKind::AllSpaces, t("The translation contains only spaces.").into()));
    }
    if on(ValidationKind::Format) {
        let (a, b) = (placeholders(orig), placeholders(tr));
        if a != b {
            out.push((
                ValidationKind::Format,
                crate::tf!("Placeholders in the original {a}, in the translation {b}.", a = format!("{a:?}"), b = format!("{b:?}")),
            ));
        }
    }
    if on(ValidationKind::Tags) {
        let (a, b) = (tags(orig), tags(tr));
        if a != b {
            out.push((ValidationKind::Tags, crate::tf!("Tags in the original {a}, in the translation {b}.", a = format!("{a:?}"), b = format!("{b:?}"))));
        }
    }
    if on(ValidationKind::WhiteSpace) {
        let (a, b) = (lead_trail(orig), lead_trail(tr));
        if a != b {
            out.push((
                ValidationKind::WhiteSpace,
                crate::tf!("Leading/trailing spaces: original {a1}/{a2}, translation {b1}/{b2}.", a1 = a.0, a2 = a.1, b1 = b.0, b2 = b.1),
            ));
        }
    }
    if on(ValidationKind::NewLine) {
        let (a, b) = (newlines(orig), newlines(tr));
        if a != b {
            out.push((ValidationKind::NewLine, crate::tf!("Line breaks: original {a}, translation {b}.", a = a, b = b)));
        }
    }
    let punct: [(ValidationKind, char, &[char]); 4] = [
        (ValidationKind::MissingPeriod, '.', &['.', '。', '．', '।', '…']),
        (ValidationKind::MissingColon, ':', &[':', '：']),
        (ValidationKind::MissingExclamation, '!', &['!', '！', '¡']),
        (ValidationKind::MissingQuestion, '?', &['?', '？', ';', '؟', '¿']),
    ];
    for (k, c, set) in punct {
        if on(k) && orig.trim_end().ends_with(c) && !orig.trim_end().ends_with("...") && !ends_with_any(tr, set) {
            out.push((k, crate::tf!("The original ends with '{c}', the translation does not.", c = c)));
        }
    }
    if on(ValidationKind::CharacterLength) && max_chars > 0 && tr.chars().count() > max_chars {
        out.push((
            ValidationKind::CharacterLength,
            crate::tf!("The translation has {n} characters, {max} are allowed.", n = tr.chars().count(), max = max_chars),
        ));
    }
    if on(ValidationKind::HotkeyCount) {
        let (a, b) = (hotkeys(orig), hotkeys(tr));
        if a != b {
            out.push((ValidationKind::HotkeyCount, crate::tf!("Hotkeys (&): original {a}, translation {b}.", a = a, b = b)));
        }
    }
    out
}

/// Validates translations of the given rows (all rows if `None`).
pub fn validate(p: &Project, opts: &ValidateOptions, rows: Option<&[RowRef]>) -> Vec<Issue> {
    let kinds: Vec<ValidationKind> = if opts.kinds.is_empty() { ValidateOptions::default().kinds } else { opts.kinds.clone() };
    let langs = if opts.langs.is_empty() { p.language_ids() } else { opts.langs.clone() };
    let refs = rows.map(<[RowRef]>::to_vec).unwrap_or_else(|| p.rows());
    let mut issues = Vec::new();
    // (lang, original) → translations seen, for duplicate translation detection
    let mut seen: HashMap<(String, String), BTreeMap<String, RowRef>> = HashMap::new();
    for r in &refs {
        let Some(row) = p.row(r) else {
            continue;
        };
        if row.locked() || row.excluded() || row.raw || row.original().is_empty() {
            continue;
        }
        let orig_lang = p.original_lang(r);
        let push = |issues: &mut Vec<Issue>, lang: &str, kind: ValidationKind, message: String| {
            issues.push(Issue {
                row: r.clone(),
                source: p.sources.get(r.source).map(|s| s.name().to_string()).unwrap_or_default(),
                context: p.context(r),
                lang: lang.to_string(),
                kind,
                severity: kind.severity(),
                message,
            });
        };
        for l in langs.iter().filter(|l| l.as_str() != orig_lang) {
            if p.languages.iter().any(|x| x.id() == l && x.is_pseudo()) {
                continue;
            }
            let Some(tr) = row.text(l).filter(|_| row.tstatus(l) != TranslationStatus::NotTranslated) else {
                if kinds.contains(&ValidationKind::MissingTranslation) {
                    push(&mut issues, l, ValidationKind::MissingTranslation, t("Not translated.").into());
                }
                continue;
            };
            for (k, m) in check_pair(row.original(), tr, row.max_chars(), &kinds) {
                push(&mut issues, l, k, m);
            }
            if kinds.contains(&ValidationKind::DuplicateTranslation) {
                seen.entry((l.clone(), row.original().to_string())).or_default().entry(tr.to_string()).or_insert_with(|| r.clone());
            }
        }
    }
    let mut dups: Vec<_> = seen.into_iter().filter(|(_, v)| v.len() > 1).collect();
    dups.sort_by(|a, b| a.0.cmp(&b.0));
    for ((lang, original), variants) in dups {
        let list: Vec<&String> = variants.keys().collect();
        for r in variants.values() {
            issues.push(Issue {
                row: r.clone(),
                source: p.sources.get(r.source).map(|s| s.name().to_string()).unwrap_or_default(),
                context: p.context(r),
                lang: lang.clone(),
                kind: ValidationKind::DuplicateTranslation,
                severity: Severity::Warning,
                message: crate::tf!("\"{original}\" has been translated differently: {list}", original = original, list = format!("{list:?}")),
            });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_extraction() {
        assert_eq!(placeholders("{0} of {1:N2} {{name}} %s %1$d 100%%"), vec!["%1$d", "%s", "{0}", "{1:N2}", "{{name}}"]);
        assert!(placeholders("{ not one }").is_empty());
    }

    #[test]
    fn pair_checks() {
        let all = ValidateOptions::default().kinds;
        let k = |o: &str, t: &str| check_pair(o, t, 0, &all).into_iter().map(|x| x.0).collect::<Vec<_>>();
        assert!(k("Hello {0}.", "Hallo {0}.").is_empty());
        assert_eq!(k("Hello {0}", "Hallo"), vec![ValidationKind::Format]);
        assert_eq!(k(" Name:", "Name:"), vec![ValidationKind::WhiteSpace]);
        assert_eq!(k("Save?", "Speichern"), vec![ValidationKind::MissingQuestion]);
        assert_eq!(k("a\nb", "ab"), vec![ValidationKind::NewLine]);
        assert_eq!(k("<b>x</b>", "x"), vec![ValidationKind::Tags]);
        assert_eq!(k("&File", "Datei"), vec![ValidationKind::HotkeyCount]);
        assert_eq!(check_pair("abc", "abcd", 3, &all)[0].0, ValidationKind::CharacterLength);
    }
}
