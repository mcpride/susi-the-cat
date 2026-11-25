//! Translation validation (Prüfen | Übersetzung prüfen).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;

use crate::i18n::t;
use crate::model::{Project, RowRef, Source};
use crate::sds::{Exclusion, Scope};
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
    ControlCharacters,
    IcuSyntax,
    IcuCases,
    /// Linked screenshot image missing (reported by `ScreenshotStore::issues`).
    Screenshot,
    /// Custom data: linked file missing, picture translation of another format or size.
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

impl ValidationKind {
    pub const ALL: [ValidationKind; 18] = [
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
        Self::ControlCharacters,
        Self::IcuSyntax,
        Self::IcuCases,
        Self::Screenshot,
        Self::Data,
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
            Self::ControlCharacters => t("Invalid control characters"),
            Self::IcuSyntax => t("Invalid ICU message"),
            Self::IcuCases => t("Incomplete ICU cases"),
            Self::Screenshot => t("Missing screenshot"),
            Self::Data => t("Custom data"),
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
            Self::ControlCharacters => "control",
            Self::IcuSyntax => "icu",
            Self::IcuCases => "icucases",
            Self::Screenshot => "screenshot",
            Self::Data => "data",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.keyword().eq_ignore_ascii_case(s.trim()))
    }

    pub fn severity(self) -> Severity {
        match self {
            Self::Format | Self::Tags | Self::CharacterLength | Self::AllSpaces | Self::ControlCharacters | Self::IcuSyntax => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// Class name of the check in Sisulizer, as stored in excluded validations (`.sds`). Checks
    /// that Sisulizer does not have get names of the form `TSusi…Validation`.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::MissingTranslation => "TMissingTranslationValidation",
            Self::Format => "TFormatValidation",
            Self::Tags => "THtmlTagValidation",
            Self::WhiteSpace => "TWhiteSpaceValidation",
            Self::NewLine => "TNewLineValidation",
            Self::MissingPeriod => "TMissingPeriodValidation",
            Self::MissingColon => "TMissingColonValidation",
            Self::MissingExclamation => "TMissingExclamationValidation",
            Self::MissingQuestion => "TMissingQuestionValidation",
            Self::CharacterLength => "TCharacterLengthValidation",
            Self::AllSpaces => "TAllSpacesTranslationValidation",
            Self::HotkeyCount => "THotkeyCountValidation",
            Self::DuplicateTranslation => "TDuplicateTranslationValidation",
            Self::ControlCharacters => "TSusiControlCharactersValidation",
            Self::IcuSyntax => "TSusiIcuSyntaxValidation",
            Self::IcuCases => "TSusiIcuCasesValidation",
            Self::Screenshot => "TSusiScreenshotValidation",
            Self::Data => "TSusiDataValidation",
        }
    }

    pub fn from_class_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.class_name().eq_ignore_ascii_case(s))
    }
}

/// Label of an excluded validation's check: the Susi label, or the Sisulizer class name without
/// `T…Validation` for checks only Sisulizer has (`TSpellingValidation` → `Spelling`).
pub fn class_label(class: &str) -> String {
    match ValidationKind::from_class_name(class) {
        Some(k) => k.label().to_string(),
        None => {
            let s = class.strip_prefix('T').unwrap_or(class);
            s.strip_suffix("Validation").filter(|x| !x.is_empty()).unwrap_or(s).to_string()
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

impl Issue {
    /// Validation that reports the issue: issues of the original carry the original language.
    pub fn scope(&self, p: &Project) -> Scope {
        if self.lang == p.original_lang(&self.row) { Scope::Original } else { Scope::Translation }
    }

    /// Exclusion that hides this issue (and the same check of the row in all other languages).
    pub fn exclusion(&self, p: &Project) -> Option<Exclusion> {
        let row = p.row(&self.row)?;
        Some(Exclusion {
            scope: self.scope(p),
            class: self.kind.class_name().to_string(),
            context: crate::sds::context(p, &self.row),
            native: row.original().to_string(),
        })
    }
}

/// Removes the issues excluded in the project's desktop file (see [`crate::sds`]) and returns
/// their number. Applies to the results of [`validate`] and of the screenshot check alike.
pub fn remove_excluded(p: &Project, issues: &mut Vec<Issue>) -> usize {
    let idx = crate::sds::Index::new(&p.desktop);
    if idx.is_empty() {
        return 0;
    }
    let before = issues.len();
    issues.retain(|i| {
        let native = p.row(&i.row).map(|r| r.original()).unwrap_or_default();
        !idx.excludes(p, &i.row, i.scope(p), i.kind.class_name(), native)
    });
    before - issues.len()
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

/// Format placeholders: .NET `{0}`/`{0:N2}`/`{name}`, ICU/i18next `{{x}}`, printf `%s`/`%1$d`
/// (`%%` excluded), Qt `%1`/`%L1`/`%n`, WebExtension `$name$` (case-insensitive).
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
            // Qt: `%1`..`%99`, `%L1` (localized number), `%n`/`%Ln` (count of a plural message).
            let qt = rest.strip_prefix('L').unwrap_or(rest);
            let digits = qt.chars().take_while(char::is_ascii_digit).take(2).count();
            let len = if digits > 0 { digits } else { usize::from(qt.starts_with('n')) };
            if len > 0 && !qt.starts_with('0') {
                let end = rest.len() - qt.len() + len;
                out.push(format!("%{}", &rest[..end]));
                i += 1 + end;
                continue;
            }
        } else if c == '$' {
            let rest = &s[pos + 1..];
            let len = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '@')).unwrap_or(rest.len());
            if len > 0 && rest[len..].starts_with('$') {
                out.push(format!("${}$", rest[..len].to_ascii_lowercase()));
                i += len + 2;
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

/// Abbreviations of the ASCII control characters U+0000–U+001F.
const C0_NAMES: [&str; 32] = [
    "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "HT", "LF", "VT", "FF", "CR", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN",
    "ETB", "CAN", "EM", "SUB", "ESC", "FS", "GS", "RS", "US",
];

/// Control characters (Unicode category Cc: ASCII U+0000–U+001F, DEL U+007F and C1 U+0080–U+009F)
/// except tab and line breaks, as `(1-based character position, character)`. They have no meaning
/// in UI texts and usually slip in through broken imports or encodings.
pub fn control_chars(s: &str) -> Vec<(usize, char)> {
    s.chars().enumerate().filter(|(_, c)| c.is_control() && !matches!(c, '\t' | '\n' | '\r')).map(|(i, c)| (i + 1, c)).collect()
}

/// `DEL (U+007F)`, `NUL (U+0000)`; C1 characters as plain `U+0085`.
fn control_char_name(c: char) -> String {
    let code = c as u32;
    match code {
        0..=0x1F => format!("{} (U+{code:04X})", C0_NAMES[code as usize]),
        0x7F => "DEL (U+007F)".into(),
        _ => format!("U+{code:04X}"),
    }
}

/// Lists the control characters of `s` (the first five) with their positions; `None` without any.
fn control_char_list(s: &str) -> Option<String> {
    const MAX: usize = 5;
    let found = control_chars(s);
    if found.is_empty() {
        return None;
    }
    let mut list: Vec<String> =
        found.iter().take(MAX).map(|(pos, c)| crate::tf!("{name} at position {pos}", name = control_char_name(*c), pos = pos)).collect();
    if found.len() > MAX {
        list.push("…".into());
    }
    Some(list.join(", "))
}

/// End punctuation equivalents (incl. full-width, Arabic, Greek/Spanish forms).
fn ends_with_any(s: &str, set: &[char]) -> bool {
    s.trim_end().chars().last().is_some_and(|c| set.contains(&c))
}

/// Checks a translation into `lang` against its original.
fn check_pair(orig: &str, tr: &str, lang: &str, max_chars: usize, kinds: &[ValidationKind]) -> Vec<(ValidationKind, String)> {
    let mut out = Vec::new();
    let on = |k: ValidationKind| kinds.contains(&k);
    if on(ValidationKind::AllSpaces) && tr.trim().is_empty() && !orig.trim().is_empty() {
        out.push((ValidationKind::AllSpaces, t("The translation contains only spaces.").into()));
    }
    // ICU messages (typed, plural or select arguments) are compared by their arguments: the
    // generic placeholder rules misread nested cases such as `other{{count} files}`.
    let icu = crate::icu::parse(orig).ok().filter(|a| crate::icu::is_complex(a)).map(|o| (o, crate::icu::parse(tr)));
    if on(ValidationKind::Format) {
        let pair = match &icu {
            Some((o, Ok(t))) => Some((crate::icu::signature_list(o), crate::icu::signature_list(t))),
            Some((_, Err(_))) => None,
            None => Some((placeholders(orig), placeholders(tr))),
        };
        if let Some((a, b)) = pair.filter(|(a, b)| a != b) {
            out.push((
                ValidationKind::Format,
                crate::tf!("Placeholders in the original {a}, in the translation {b}.", a = format!("{a:?}"), b = format!("{b:?}")),
            ));
        }
    }
    match &icu {
        Some((_, Err(e))) if on(ValidationKind::IcuSyntax) => {
            out.push((ValidationKind::IcuSyntax, crate::tf!("ICU syntax error in the translation: {error}.", error = e)));
        }
        Some((o, Ok(t))) if on(ValidationKind::IcuCases) => {
            for m in crate::icu::missing_cases(o, t, crate::formats::android::plural_categories(lang)) {
                let msg = match m {
                    crate::icu::Missing::Plural { arg, categories } => {
                        crate::tf!("{{arg}}: plural forms of {lang} missing: {cases}", arg = arg, lang = lang, cases = categories.join(", "))
                    }
                    crate::icu::Missing::Select { arg, cases } => {
                        crate::tf!("{{arg}}: cases of the original missing: {cases}", arg = arg, cases = cases.join(", "))
                    }
                };
                out.push((ValidationKind::IcuCases, msg));
            }
        }
        _ => {}
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
    if on(ValidationKind::ControlCharacters)
        && let Some(chars) = control_char_list(tr)
    {
        out.push((ValidationKind::ControlCharacters, crate::tf!("The translation contains control characters: {chars}.", chars = chars)));
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
        if row.is_data() {
            if kinds.contains(&ValidationKind::Data) && !row.locked() && !row.excluded() {
                for (lang, message) in crate::custom_data::issues(p, row, &langs) {
                    push(&mut issues, &lang, ValidationKind::Data, message);
                }
            }
            continue;
        }
        if row.locked() || row.excluded() || row.is_data_or_raw() || row.original().is_empty() {
            continue;
        }
        let orig_lang = p.original_lang(r);
        // The original is shared by all languages: reported once, under the original language.
        if kinds.contains(&ValidationKind::ControlCharacters)
            && let Some(chars) = control_char_list(row.original())
        {
            push(&mut issues, orig_lang, ValidationKind::ControlCharacters, crate::tf!("The original contains control characters: {chars}.", chars = chars));
        }
        if kinds.contains(&ValidationKind::IcuSyntax)
            && crate::icu::looks_like_icu(row.original())
            && let Err(e) = crate::icu::parse(row.original())
        {
            push(&mut issues, orig_lang, ValidationKind::IcuSyntax, crate::tf!("ICU syntax error in the original: {error}.", error = e));
        }
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
            for (k, m) in check_pair(row.original(), tr, l, row.max_chars(), &kinds) {
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

/// JUnit XML report of `issues` (e.g. for GitLab CI unit test reports): one failed test case per
/// issue (class name = source, name = language, context and check). Without issues the report
/// contains an empty test suite.
pub fn junit_xml(p: &Project, issues: &[&Issue]) -> String {
    let suite = p.path.as_deref().and_then(Path::file_name).map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "susi".into());
    let n = issues.len();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(out, "<testsuites name=\"susi-cli validate\" tests=\"{n}\" failures=\"{n}\" errors=\"0\">");
    let _ = write!(out, "  <testsuite name=\"{}\" tests=\"{n}\" failures=\"{n}\" errors=\"0\" skipped=\"0\"", junit_escape(&suite, true));
    if issues.is_empty() {
        out.push_str("/>\n</testsuites>\n");
        return out;
    }
    out.push_str(">\n");
    // GitLab keeps only the first of several test cases with the same name.
    let mut names = HashSet::new();
    for i in issues {
        let base = format!("[{}] {}: {}", i.lang, i.context, i.kind.label());
        let mut name = base.clone();
        for k in 2.. {
            if names.insert(name.clone()) {
                break;
            }
            name = format!("{base} ({k})");
        }
        let _ = write!(out, "    <testcase classname=\"{}\" name=\"{}\"", junit_escape(&i.source, true), junit_escape(&name, true));
        if let Some(file) = p.sources.get(i.row.source).and_then(|s| junit_file(p, s)) {
            let _ = write!(out, " file=\"{}\"", junit_escape(&file, true));
        }
        let severity = match i.severity {
            Severity::Error => t("Error"),
            Severity::Warning => t("Warning"),
        };
        let mut body = format!("{severity}: {}", i.message);
        if let Some(row) = p.row(&i.row) {
            let orig_lang = p.original_lang(&i.row);
            let _ = write!(body, "\n{}: {}", crate::tf!("Original ({lang})", lang = orig_lang), row.original());
            if let Some(text) = row.text(&i.lang).filter(|_| i.lang != orig_lang && i.kind != ValidationKind::MissingTranslation) {
                let _ = write!(body, "\n{}: {text}", crate::tf!("Translation – {lang}", lang = i.lang));
            }
        }
        let _ = writeln!(
            out,
            ">\n      <failure type=\"{}\" message=\"{}\">{}</failure>\n    </testcase>",
            i.kind.keyword(),
            junit_escape(&i.message, true),
            junit_escape(&body, false)
        );
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

/// Path of a source file for the JUnit `file` attribute: resolved against the project directory,
/// relative to the working directory where possible, with `/`; `None` for wildcard sources.
fn junit_file(p: &Project, s: &Source) -> Option<String> {
    let name = s.name();
    if name.is_empty() || name.contains(['*', '?']) {
        return None;
    }
    let path = crate::ops::normalize(&p.base_dir().join(name.replace('\\', "/")));
    let path = std::env::current_dir().ok().and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf)).unwrap_or(path);
    Some(path.to_string_lossy().replace('\\', "/"))
}

/// XML escaping for the JUnit report. Control characters (mostly not allowed in XML 1.0) and the
/// non-characters U+FFFE/U+FFFF are written visibly as `[U+007F]`.
fn junit_escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#10;"),
            '\t' if attr => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            '\n' | '\t' => out.push(c),
            c if c.is_control() || c == '\u{FFFE}' || c == '\u{FFFF}' => {
                let _ = write!(out, "[U+{:04X}]", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_extraction() {
        assert_eq!(placeholders("{0} of {1:N2} {{name}} %s %1$d 100%%"), vec!["%1$d", "%s", "{0}", "{1:N2}", "{{name}}"]);
        assert!(placeholders("{ not one }").is_empty());
        assert_eq!(placeholders("Hi $USER$, $Count$ tabs for $5 or $$"), vec!["$count$", "$user$"]);
        assert_eq!(placeholders("%1 of %L2 files, %n left (%Ln), 100% done, %0"), vec!["%1", "%L2", "%Ln", "%n"]);
        assert_eq!(placeholders("%1$s and %10"), vec!["%1$s", "%10"]);
    }

    #[test]
    fn pair_checks() {
        let all = ValidateOptions::default().kinds;
        let k = |o: &str, t: &str| check_pair(o, t, "de", 0, &all).into_iter().map(|x| x.0).collect::<Vec<_>>();
        assert!(k("Hello {0}.", "Hallo {0}.").is_empty());
        assert_eq!(k("Hello {0}", "Hallo"), vec![ValidationKind::Format]);
        assert_eq!(k(" Name:", "Name:"), vec![ValidationKind::WhiteSpace]);
        assert_eq!(k("Save?", "Speichern"), vec![ValidationKind::MissingQuestion]);
        assert_eq!(k("a\nb", "ab"), vec![ValidationKind::NewLine]);
        assert_eq!(k("<b>x</b>", "x"), vec![ValidationKind::Tags]);
        assert_eq!(k("&File", "Datei"), vec![ValidationKind::HotkeyCount]);
        assert_eq!(check_pair("abc", "abcd", "de", 3, &all)[0].0, ValidationKind::CharacterLength);
        assert_eq!(k("Save", "Speichern\u{7F}"), vec![ValidationKind::ControlCharacters]);
    }

    #[test]
    fn icu_messages() {
        let all = ValidateOptions::default().kinds;
        let check = |o: &str, t: &str, lang: &str| check_pair(o, t, lang, 0, &all);
        let kinds = |o: &str, t: &str, lang: &str| check(o, t, lang).into_iter().map(|x| x.0).collect::<Vec<_>>();
        let wombats = "{count, plural, =0{no wombats} one{1 wombat} other{{count} wombats}}";
        // Nested `{count}` is no i18next `{{…}}` placeholder: a correct translation passes.
        assert_eq!(kinds(wombats, "{count, plural, =0{keine Wombats} one{1 Wombat} other{{count} Wombats}}", "de"), vec![]);
        assert!(placeholders(wombats) != placeholders("{count, plural, =0{keine Wombats} one{1 Wombat} other{{count} Wombats}}"));
        // Translated keyword, renamed argument, broken braces.
        let e = check(wombats, "{count, plural, =0{keine} eins{1 Wombat} other{{count} Wombats}}", "de");
        assert_eq!(e.iter().map(|x| x.0).collect::<Vec<_>>(), vec![ValidationKind::IcuSyntax]);
        assert!(e[0].1.contains("'eins' is no plural category"), "{}", e[0].1);
        assert_eq!(kinds(wombats, "{anzahl, plural, one{1 Wombat} other{{anzahl} Wombats}}", "de"), vec![ValidationKind::Format]);
        assert_eq!(kinds(wombats, "{count, plural, one{1 Wombat} other{{count} Wombats}", "de"), vec![ValidationKind::IcuSyntax]);
        // Polish needs few and many; select cases of the original must stay.
        let m = check(wombats, "{count, plural, one{1 wombat} other{{count} wombatów}}", "pl");
        assert_eq!(m.iter().map(|x| x.0).collect::<Vec<_>>(), vec![ValidationKind::IcuCases]);
        assert_eq!(m[0].1, "{count}: plural forms of pl missing: few, many");
        let gender = "{g, select, male{his} female{her} other{their}} file";
        assert_eq!(check(gender, "{g, select, male{seine} other{ihre}} Datei", "de")[0].1, "{g}: cases of the original missing: female");
        // Not ICU: .NET format strings keep the generic placeholder check.
        assert_eq!(kinds("{0:N2} of {1}", "{0:N2} von", "de"), vec![ValidationKind::Format]);
    }

    #[test]
    fn control_characters() {
        // Tab and line breaks are legitimate; DEL, the other C0 characters and C1 are not.
        assert!(control_chars("a\tb\r\nc\n").is_empty());
        assert_eq!(control_chars("Öffnen\u{7F}"), vec![(7, '\u{7F}')]);
        assert_eq!(control_chars("\0a\u{1B}b\u{85}"), vec![(1, '\0'), (3, '\u{1B}'), (5, '\u{85}')]);
        assert_eq!(control_char_list("ab\u{7F}").as_deref(), Some("DEL (U+007F) at position 3"));
        assert_eq!(control_char_list("\u{0}\u{85}").as_deref(), Some("NUL (U+0000) at position 1, U+0085 at position 2"));
        assert!(control_char_list(&"\u{7F}".repeat(7)).unwrap().ends_with("at position 5, …"));
        assert_eq!(control_char_list("plain"), None);
    }

    /// Project with source `a.json` (original English, German translation) containing `rows`.
    fn project(rows: &str) -> Project {
        let slp = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<document>\n  <lang id=\"de\"/>\n  <lang id=\"en\"/>\n  \
             <source class=\"TJsonSource\" name=\"a.json\" original=\"en\">\n    <node class=\"TJsonNode\" name=\"a.json\">\n{rows}\n    \
             </node>\n  </source>\n</document>\n"
        );
        crate::slp::parse(slp.as_bytes()).unwrap()
    }

    const CONTROL_ROWS: &str = "<row id=\"OPEN\"><native>Open\u{7F}</native><lang id=\"de\" status=\"3\">Öffnen &amp; los\u{7F}</lang></row>\n\
                                <row id=\"SAVE\"><native>Save</native><lang id=\"de\" status=\"3\">Speichern</lang></row>";

    #[test]
    fn control_characters_in_original_and_translation() {
        let p = project(CONTROL_ROWS);
        let issues = validate(&p, &ValidateOptions::default(), None);
        let found: Vec<(&str, &str, ValidationKind, Severity)> = issues.iter().map(|i| (i.context.as_str(), i.lang.as_str(), i.kind, i.severity)).collect();
        assert_eq!(
            found,
            vec![
                ("a.json/OPEN", "en", ValidationKind::ControlCharacters, Severity::Error),
                ("a.json/OPEN", "de", ValidationKind::ControlCharacters, Severity::Error)
            ]
        );
        assert!(issues[0].message.contains("DEL (U+007F) at position 5"), "{}", issues[0].message);
        let only_format = ValidateOptions { kinds: vec![ValidationKind::Format], ..Default::default() };
        assert!(validate(&p, &only_format, None).is_empty());
        assert_eq!(ValidationKind::from_keyword("control"), Some(ValidationKind::ControlCharacters));
    }

    /// `testdata/slp/Gettext`: in `l10n.sds` Sisulizer excluded the format check of `%d file[0]`
    /// and of `%d result[0]` with the message context `android-app/search`.
    #[test]
    fn exclusions_of_the_sisulizer_desktop_file() {
        let slp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/slp/Gettext/l10n.slp");
        let p = crate::slp::load(&slp).unwrap();
        let format = |issues: &[Issue]| issues.iter().filter(|i| i.kind == ValidationKind::Format).map(|i| p.context(&i.row)).collect::<Vec<_>>();
        let excluded = |c: &String| c.ends_with("/%d file[0]") || c.ends_with("/android-app/search|%d result[0]");
        let mut issues = validate(&p, &ValidateOptions::default(), None);
        let all = format(&issues);
        for row in ["/%d file[0]", "/android-app/search|%d result[0]", "/web-app/search|%d result[0]"] {
            assert!(all.iter().any(|c| c.ends_with(row)), "{row}: {all:?}");
        }
        let removed = remove_excluded(&p, &mut issues);
        let kept = format(&issues);
        // Excluded for every language; other plural forms and the same text in another context stay.
        assert_eq!(removed, all.iter().filter(|c| excluded(c)).count());
        assert!(!kept.iter().any(excluded), "{kept:?}");
        assert!(kept.iter().any(|c| c.ends_with("/%d file[1]")), "{kept:?}");
        assert!(kept.iter().any(|c| c.ends_with("/web-app/search|%d result[0]")), "{kept:?}");

        // The exclusion Susi creates for an issue is the one Sisulizer wrote.
        let i = validate(&p, &ValidateOptions::default(), None)
            .into_iter()
            .find(|i| i.kind == ValidationKind::Format && i.context.ends_with("/%d file[0]"))
            .unwrap();
        assert_eq!(i.exclusion(&p).as_ref(), p.desktop.exclusions().first());
    }

    #[test]
    fn exclusions_by_scope_and_class() {
        let mut p = project(CONTROL_ROWS);
        let issues = validate(&p, &ValidateOptions::default(), None);
        assert_eq!(issues.len(), 2);
        // Excluding the original's issue keeps the translation's issue of the same check.
        let e = issues[0].exclusion(&p).unwrap();
        assert_eq!(
            (e.scope, e.class.as_str(), e.context.as_str(), e.native.as_str()),
            (Scope::Original, "TSusiControlCharactersValidation", "OPEN", "Open\u{7F}")
        );
        p.desktop.add(e);
        let mut again = validate(&p, &ValidateOptions::default(), None);
        assert_eq!(remove_excluded(&p, &mut again), 1);
        assert_eq!(again.iter().map(|i| i.lang.as_str()).collect::<Vec<_>>(), ["de"]);
        // A changed original is validated again.
        p.row_mut(&issues[0].row).unwrap().native = Some("Open\u{7F}\u{7F}".into());
        let mut changed = validate(&p, &ValidateOptions::default(), None);
        assert_eq!(remove_excluded(&p, &mut changed), 0);

        assert_eq!(class_label("TFormatValidation"), "Inconsistent format string");
        assert_eq!(class_label("TSpellingValidation"), "Spelling");
        assert!(ValidationKind::ALL.iter().all(|k| ValidationKind::from_class_name(k.class_name()) == Some(*k)));
    }

    #[test]
    fn junit_report() {
        let mut p = project(CONTROL_ROWS);
        p.path = Some(std::path::PathBuf::from("l10n").join("app.slp"));
        let issues = validate(&p, &ValidateOptions::default(), None);
        // The first issue twice: GitLab ignores repeated names, so the second one gets a suffix.
        let xml = junit_xml(&p, &[&issues[0], &issues[1], &issues[0]]);
        assert!(!xml.contains('\u{7F}'), "control characters must not reach the XML");
        let doc = crate::xml::parse_str(&xml).unwrap();
        assert_eq!((doc.root.name.as_str(), doc.root.attr("tests"), doc.root.attr("failures")), ("testsuites", Some("3"), Some("3")));
        let suite = doc.root.child("testsuite").unwrap();
        assert_eq!(suite.attr("name"), Some("app.slp"));
        let cases: Vec<_> = suite.elements().collect();
        let names: Vec<_> = cases.iter().map(|c| c.attr("name").unwrap()).collect();
        assert_eq!(
            names,
            [
                "[en] a.json/OPEN: Invalid control characters",
                "[de] a.json/OPEN: Invalid control characters",
                "[en] a.json/OPEN: Invalid control characters (2)"
            ]
        );
        assert_eq!((cases[1].attr("classname"), cases[1].attr("file")), (Some("a.json"), Some("l10n/a.json")));
        let failure = cases[1].child("failure").unwrap();
        assert_eq!(failure.attr("type"), Some("control"));
        assert_eq!(failure.attr("message"), Some(issues[1].message.as_str()));
        assert_eq!(
            failure.text(),
            "Error: The translation contains control characters: DEL (U+007F) at position 13.\nOriginal (en): Open[U+007F]\nTranslation – de: Öffnen & los[U+007F]"
        );
        // The original's issue shows no translation.
        assert!(!cases[0].child("failure").unwrap().text().contains("Translation"));

        // Without issues the report is still written, with an empty test suite.
        let doc = crate::xml::parse_str(&junit_xml(&p, &[])).unwrap();
        assert_eq!(doc.root.attr("tests"), Some("0"));
        assert_eq!(doc.root.child("testsuite").map(|s| s.elements().count()), Some(0));
    }
}
