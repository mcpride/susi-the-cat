//! User interface localisation (GUI and CLI), modelled on PhotoCraft's i18n.
//!
//! Texts in the code are English and double as lookup keys; per-language `.tsv` catalogs
//! (see [`catalog`]) map them to display text at run time. Missing entries fall back to English.
//! The MCP interface never translates: its threads keep the default language (English).
//!
//! ```ignore
//! use susi_core::i18n::{t, trn};
//! ui.label(t("Open project"));
//! let msg = susi_core::tf!("Project saved: {path}", path = p.display());
//! let files = trn(n, "{n} file", "{n} files");
//! ```
//!
//! Adding a language: create `i18n/<code>-*.tsv` catalogs and register them in [`LANGUAGES`].

pub mod catalog;

use std::cell::Cell;
use std::fmt::Display;
use std::sync::OnceLock;

pub use catalog::Catalog;

/// Environment variable that overrides the system language when the preference is `auto`.
pub const ENV_LANG: &str = "SUSI_LANG";

/// A supported user interface language.
pub struct LangInfo {
    /// Primary language subtag (`en`, `de`).
    pub code: &'static str,
    /// Native name shown in language pickers.
    pub name: &'static str,
    sources: &'static [&'static str],
    catalog: OnceLock<Catalog>,
}

impl LangInfo {
    fn catalog(&'static self) -> &'static Catalog {
        self.catalog.get_or_init(|| Catalog::from_sources(self.sources))
    }
}

/// Registry; English (index 0) is the source language and has no catalog.
pub static LANGUAGES: [LangInfo; 2] = [
    LangInfo { code: "en", name: "English", sources: &[], catalog: OnceLock::new() },
    LangInfo {
        code: "de",
        name: "Deutsch",
        sources: &[include_str!("i18n/de-core.tsv"), include_str!("i18n/de-cli.tsv"), include_str!("i18n/de-gui.tsv")],
        catalog: OnceLock::new(),
    },
];

/// Handle of an entry in [`LANGUAGES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lang(usize);

impl Lang {
    pub const EN: Lang = Lang(0);
    pub const DE: Lang = Lang(1);

    pub fn all() -> impl Iterator<Item = Lang> {
        (0..LANGUAGES.len()).map(Lang)
    }

    fn info(self) -> &'static LangInfo {
        &LANGUAGES[self.0]
    }

    pub fn code(self) -> &'static str {
        self.info().code
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Resolves a language tag (`de`, `de-AT`, `de_DE.UTF-8`) by its primary subtag.
    pub fn from_code(tag: &str) -> Option<Lang> {
        let primary = tag.trim().split(['-', '_', '.', '@']).next().unwrap_or("").to_ascii_lowercase();
        Self::all().find(|l| l.code() == primary)
    }

    /// Resolves a preference: `auto` (or empty) = system language, otherwise a code; unknown = English.
    pub fn from_pref(pref: &str) -> Lang {
        match pref.trim() {
            p if p.is_empty() || p.eq_ignore_ascii_case("auto") => system_lang(),
            code => Self::from_code(code).unwrap_or(Lang::EN),
        }
    }
}

/// First supported language among the given locale tags, English otherwise.
pub fn resolve_tags<S: AsRef<str>>(tags: impl IntoIterator<Item = S>) -> Lang {
    tags.into_iter().find_map(|t| Lang::from_code(t.as_ref())).unwrap_or(Lang::EN)
}

/// System language: [`ENV_LANG`] if set, otherwise the OS locale list.
pub fn system_lang() -> Lang {
    match std::env::var(ENV_LANG) {
        Ok(v) if !v.trim().is_empty() && !v.trim().eq_ignore_ascii_case("auto") => Lang::from_code(&v).unwrap_or(Lang::EN),
        _ => resolve_tags(sys_locale::get_locales()),
    }
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// Sets the language of the current thread (the UI thread / CLI main thread).
pub fn set_current(lang: Lang) {
    CURRENT.with(|c| c.set(lang));
}

pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// Translates `s` into the current language.
pub fn t(s: &str) -> &str {
    tr(current(), s)
}

/// Translates `s` into `lang`.
pub fn tr(lang: Lang, s: &str) -> &str {
    lang.info().catalog().get(s).unwrap_or(s)
}

/// Translates `s` with a disambiguating context (same English text, different translations).
pub fn tr_ctx<'a>(context: &str, s: &'a str) -> &'a str {
    current().info().catalog().get_ctx(context, s).unwrap_or(s)
}

/// Plural-aware translation; `{n}` is replaced by `n`.
pub fn trn(n: usize, one: &str, other: &str) -> String {
    let idx = usize::from(n != 1);
    let forms = current().info().catalog().plural(&format!("{one}|{other}"));
    let s = match forms {
        Some(f) if !f.is_empty() => f[idx.min(f.len() - 1)].as_str(),
        _ if idx == 0 => one,
        _ => other,
    };
    s.replace("{n}", &n.to_string())
}

/// Replaces `{name}` placeholders in a (translated) template.
pub fn fmt(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut s = template.to_string();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), &v.to_string());
    }
    s
}

/// Whether `s` equals the English text or any of its translations (for parsing localized input).
pub fn matches_any(english: &str, s: &str) -> bool {
    let s = s.trim().to_lowercase();
    Lang::all().any(|l| tr(l, english).to_lowercase() == s)
}

/// Translates a template and fills named placeholders: `tf!("Saved {path}", path = p.display())`.
#[macro_export]
macro_rules! tf {
    ($s:literal $(, $k:ident = $v:expr)* $(,)?) => {
        $crate::i18n::fmt($crate::i18n::t($s), &[$((stringify!($k), &$v as &dyn ::std::fmt::Display)),*])
    };
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::path::{Path, PathBuf};

    use super::catalog::{parse_entries, placeholders};
    use super::*;

    fn catalogs() -> Vec<(Lang, &'static str)> {
        Lang::all().flat_map(|l| l.info().sources.iter().map(move |s| (l, *s))).collect()
    }

    #[test]
    fn catalogs_parse_without_errors_or_duplicates() {
        for (l, text) in catalogs() {
            let (entries, errors) = parse_entries(text);
            assert!(errors.is_empty(), "{}: {errors:?}", l.code());
            assert!(!entries.is_empty());
            let mut seen = BTreeSet::new();
            for e in entries {
                assert!(seen.insert((e.context.clone(), e.source.clone())), "{}: duplicate entry line {}: {:?}", l.code(), e.line, e.source);
            }
        }
    }

    #[test]
    fn duplicate_entries_across_files_are_rejected() {
        let mut seen: HashMap<(String, String), usize> = HashMap::new();
        for (l, text) in catalogs() {
            for e in parse_entries(text).0 {
                *seen.entry((format!("{}:{}", l.code(), e.context), e.source)).or_default() += 1;
            }
        }
        let dups: Vec<_> = seen.into_iter().filter(|(_, n)| *n > 1).map(|(k, _)| k).collect();
        assert!(dups.is_empty(), "{dups:?}");
    }

    #[test]
    fn placeholders_match() {
        for (l, text) in catalogs() {
            for e in parse_entries(text).0 {
                if e.context == "@plural" {
                    let src: Vec<&str> = e.source.split('|').collect();
                    let tr: Vec<&str> = e.translation.split('|').collect();
                    assert_eq!(src.len(), 2, "line {}", e.line);
                    assert_eq!(tr.len(), 2, "{}: line {}", l.code(), e.line);
                    for (a, b) in src.iter().zip(&tr) {
                        assert_eq!(placeholders(a), placeholders(b), "{}: line {}", l.code(), e.line);
                    }
                } else {
                    assert_eq!(placeholders(&e.source), placeholders(&e.translation), "{}: line {}: {:?}", l.code(), e.line, e.source);
                }
            }
        }
    }

    #[test]
    fn lookup_and_fallback() {
        let unknown = String::from("No such text in any catalog");
        set_current(Lang::DE);
        assert_eq!(t(&unknown), unknown);
        assert_eq!(tr(Lang::DE, "Not translated"), "Nicht übersetzt");
        assert_eq!(trn(1, "{n} file", "{n} files"), "1 Datei");
        assert_eq!(trn(3, "{n} file", "{n} files"), "3 Dateien");
        set_current(Lang::EN);
        assert_eq!(t("Not translated"), "Not translated");
        assert_eq!(trn(1, "{n} file", "{n} files"), "1 file");
        assert_eq!(fmt("{a} of {b}", &[("a", &1), ("b", &"two")]), "1 of two");
        assert!(matches_any("Not translated", " nicht ÜBERSETZT"));
    }

    #[test]
    fn resolves_languages() {
        assert_eq!(resolve_tags(["de-AT", "en-US"]), Lang::DE);
        assert_eq!(resolve_tags(["fr-FR", "de_DE.UTF-8"]), Lang::DE);
        assert_eq!(resolve_tags(["fr-FR", "ja"]), Lang::EN);
        assert_eq!(resolve_tags(Vec::<String>::new()), Lang::EN);
        assert_eq!(Lang::from_pref("de"), Lang::DE);
        assert_eq!(Lang::from_pref("xx"), Lang::EN);
        assert_eq!(Lang::EN.code(), "en");
    }

    /// Susi's catalogs are maintained with Susi: `i18n/susi-i18n.slp` must cover and reproduce them.
    #[test]
    fn catalogs_match_the_susi_project() {
        let hint =
            "update the project with `susi-cli scan crates/core/src/i18n/susi-i18n.slp`, translate and build it with Susi (README: \"Translating Susi\")";
        let tmp = tempfile::tempdir().unwrap();
        let mut catalogs = BTreeSet::new();
        for e in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n")).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".tsv") || name.ends_with(".slp") {
                std::fs::copy(e.path(), tmp.path().join(&name)).unwrap();
            }
            if name.ends_with(".tsv") {
                catalogs.insert(crate::ops::normalize(&tmp.path().join(&name)));
            }
        }
        let mut p = crate::slp::load(&tmp.path().join("susi-i18n.slp")).unwrap();
        let mut covered = BTreeSet::new();
        for (si, s) in p.sources.iter().enumerate() {
            for f in crate::ops::source_files(&p, si).unwrap() {
                covered.extend(p.language_ids().iter().filter(|l| *l != s.original()).filter_map(|l| crate::ops::output_path(&p, si, &f, l)));
                covered.insert(f);
            }
        }
        let missing: Vec<_> = catalogs.difference(&covered).collect();
        assert!(missing.is_empty(), "catalogs missing in susi-i18n.slp: {missing:?}; add them as sources");
        let rep = crate::ops::scan_project(&mut p, None).unwrap();
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        assert_eq!((rep.new, rep.changed, rep.unused, rep.imported), (0, 0, 0, 0), "catalogs differ from susi-i18n.slp; {hint}");
        let b = crate::ops::build_project(&p, &[], None);
        assert!(b.errors.is_empty(), "{:?}", b.errors);
        assert!(b.written.is_empty(), "building susi-i18n.slp would change {:?}; {hint}", b.written);
    }

    // ---- coverage: every translatable literal in the sources has a German entry and vice versa ----

    /// Parses a Rust string literal starting at `s[0] == '"'`.
    fn rust_string(s: &str) -> Option<String> {
        let mut out = String::new();
        let mut it = s.char_indices().skip(1).peekable();
        while let Some((_, c)) = it.next() {
            match c {
                '"' => return Some(out),
                '\\' => match it.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    'u' => {
                        let mut hex = String::new();
                        for (_, h) in it.by_ref() {
                            match h {
                                '{' => {}
                                '}' => break,
                                h => hex.push(h),
                            }
                        }
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    '\n' | '\r' => {
                        while it.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                            it.next();
                        }
                    }
                    _ => return None,
                },
                c => out.push(c),
            }
        }
        None
    }

    /// Literal arguments following `pos` (after skipping `skip_args` non-literal arguments).
    fn literals_at(src: &str, pos: usize, skip_args: usize, count: usize) -> Option<Vec<String>> {
        let mut rest = &src[pos..];
        for _ in 0..skip_args {
            rest = &rest[rest.find(',')? + 1..];
        }
        let mut out = Vec::new();
        for i in 0..count {
            rest = rest.trim_start();
            if i > 0 {
                rest = rest.strip_prefix(',')?.trim_start();
            }
            if !rest.starts_with('"') {
                return None;
            }
            let lit = rust_string(rest)?;
            let raw_len = rest[1..].char_indices().scan(false, |esc, (i, c)| {
                let end = !*esc && c == '"';
                *esc = !*esc && c == '\\';
                Some((i, end))
            });
            let end = raw_len.filter(|(_, e)| *e).map(|(i, _)| i).next()?;
            rest = &rest[end + 2..];
            out.push(lit);
        }
        Some(out)
    }

    fn used_keys(src: &str, keys: &mut BTreeSet<(String, String)>, file: &str) {
        let src: String = src.lines().map(|l| if l.trim_start().starts_with("//") { "" } else { l }).collect::<Vec<_>>().join("\n");
        // (token, non-literal args to skip, literal count, kind)
        let patterns: [(&str, usize, usize, &str); 5] =
            [("t(", 0, 1, ""), ("tf!(", 0, 1, ""), ("tr_ctx(", 0, 2, "ctx"), ("trn(", 1, 2, "@plural"), ("tr(", 1, 1, "")];
        for (tok, skip, count, kind) in patterns {
            for (pos, _) in src.match_indices(tok) {
                let prev = src[..pos].chars().next_back();
                if prev.is_some_and(|c| c.is_alphanumeric() || "_.\"".contains(c)) {
                    continue;
                }
                let Some(lits) = literals_at(&src, pos + tok.len(), skip, count) else {
                    continue;
                };
                let key = match kind {
                    "ctx" => (lits[0].clone(), lits[1].clone()),
                    "@plural" => ("@plural".to_string(), format!("{}|{}", lits[0], lits[1])),
                    _ => (String::new(), lits[0].clone()),
                };
                assert!(!key.1.is_empty(), "{file}: empty translatable literal");
                keys.insert(key);
            }
        }
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    #[test]
    fn literal_scanner() {
        let mut k = BTreeSet::new();
        used_keys(
            "a(t(\"A \\\"b\\\"\\n\")); tf!(\n  \"x {n}\", n = 1); print(\"no\"); trn(n, \"{n} a\", \"{n} b\"); tr_ctx(\"menu\", \"File\"); t(x);",
            &mut k,
            "x",
        );
        let want: BTreeSet<(String, String)> =
            [("", "A \"b\"\n"), ("", "x {n}"), ("@plural", "{n} a|{n} b"), ("menu", "File")].into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        assert_eq!(k, want);
    }

    #[test]
    fn german_catalog_covers_all_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut files = Vec::new();
        for d in ["crates/core/src", "crates/mcp/src", "apps/susi/src", "apps/susi-cli/src"] {
            rust_files(&root.join(d), &mut files);
        }
        let mut used = BTreeSet::new();
        for f in &files {
            used_keys(&std::fs::read_to_string(f).unwrap(), &mut used, &f.display().to_string());
        }
        let mut have = BTreeSet::new();
        for (l, text) in catalogs() {
            if l == Lang::DE {
                have.extend(parse_entries(text).0.into_iter().map(|e| (e.context, e.source)));
            }
        }
        let missing: Vec<_> = used.difference(&have).collect();
        let stale: Vec<_> = have.difference(&used).collect();
        assert!(missing.is_empty(), "{} texts without German translation:\n{missing:#?}", missing.len());
        assert!(stale.is_empty(), "{} unused catalog entries:\n{stale:#?}", stale.len());
    }
}
