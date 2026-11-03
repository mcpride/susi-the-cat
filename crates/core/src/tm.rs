//! Translation memory stored as TMX 1.4 (replacement for Sisulizer's internal TM database).
//!
//! Matching follows the defaults of Sisulizer's TM ("Keep case", "Remove white spaces"):
//! leading/trailing white space is ignored when storing and looking up and re-applied from the
//! original on translation; an exact-case match is preferred, otherwise a case-insensitive match
//! is used and the case of the first character is adapted.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result, read_file, write_file};
use crate::lang;
use crate::model::{Project, RowRef};
use crate::status::TranslationStatus;
use crate::xml::{self, Element, Node, escape_attr, escape_text};

/// One translation unit: the same text in several languages.
#[derive(Debug, Clone, Default)]
pub struct TmUnit {
    /// Language tag (as written to TMX) → text.
    pub variants: BTreeMap<String, String>,
    pub created: String,
    pub changed: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TmMatch {
    pub source: String,
    pub text: String,
    /// 100 = exact match.
    pub score: u8,
}

#[derive(Debug, Clone, Default)]
pub struct TranslationMemory {
    pub path: Option<PathBuf>,
    pub units: Vec<TmUnit>,
    /// normalized language → lower-cased trimmed text → unit indices
    index: HashMap<String, HashMap<String, Vec<usize>>>,
    pub dirty: bool,
}

fn norm_lang(l: &str) -> String {
    lang::tag(l).to_ascii_lowercase()
}

fn primary(l: &str) -> &str {
    l.split(['-', '_']).next().unwrap_or(l)
}

fn key(text: &str) -> String {
    text.trim().to_lowercase()
}

fn tmx_stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Splits `s` into leading white space, core and trailing white space.
fn split_ws(s: &str) -> (&str, &str, &str) {
    let core = s.trim();
    let start = s.len() - s.trim_start().len();
    (&s[..start], core, &s[start + core.len()..])
}

/// Adapts the case of the first letter of `text` to that of `model`.
fn adapt_case(model: &str, text: &str) -> String {
    let (Some(m), Some(t)) = (model.chars().next(), text.chars().next()) else {
        return text.to_string();
    };
    let rest = &text[t.len_utf8()..];
    if m.is_uppercase() && t.is_lowercase() {
        t.to_uppercase().collect::<String>() + rest
    } else if m.is_lowercase() && t.is_uppercase() {
        t.to_lowercase().collect::<String>() + rest
    } else {
        text.to_string()
    }
}

/// Similarity 0..=100 based on the Levenshtein distance of characters.
pub fn similarity(a: &str, b: &str) -> u8 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let max = a.len().max(b.len());
    if max == 0 {
        return 100;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[b.len()];
    u8::try_from(100 - dist * 100 / max).unwrap_or(0)
}

impl TranslationMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Default TM location: `%APPDATA%\susi\susi-tm.tmx` (or `$HOME/.config/susi/susi-tm.tmx`).
    pub fn default_path() -> PathBuf {
        crate::settings::config_dir().join("susi-tm.tmx")
    }

    /// Loads a TMX file; a missing file yields an empty TM bound to `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let mut tm = if path.exists() { Self::from_tmx(&read_file(path)?)? } else { Self::new() };
        tm.path = Some(path.to_path_buf());
        Ok(tm)
    }

    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or_else(|| Error::Invalid("translation memory has no file name".into()))?;
        self.save_as(&path)
    }

    pub fn save_as(&mut self, path: &Path) -> Result<()> {
        write_file(path, self.to_tmx().as_bytes())?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Languages contained in the TM.
    pub fn languages(&self) -> Vec<String> {
        let mut v: Vec<String> = self.units.iter().flat_map(|u| u.variants.keys().cloned()).collect();
        v.sort();
        v.dedup();
        v
    }

    fn reindex(&mut self) {
        self.index.clear();
        for i in 0..self.units.len() {
            self.index_unit(i);
        }
    }

    fn index_unit(&mut self, i: usize) {
        let Some(u) = self.units.get(i) else {
            return;
        };
        let entries: Vec<(String, String)> = u.variants.iter().map(|(l, t)| (norm_lang(l), key(t))).collect();
        for (l, k) in entries {
            let v = self.index.entry(l).or_default().entry(k).or_default();
            if !v.contains(&i) {
                v.push(i);
            }
        }
    }

    /// Language key in the index matching `l` (exact tag, else same primary language).
    fn lang_keys(&self, l: &str) -> Vec<String> {
        let n = norm_lang(l);
        if self.index.contains_key(&n) {
            return vec![n];
        }
        let p = primary(&n).to_string();
        let mut v: Vec<String> = self.index.keys().filter(|k| primary(k) == p).cloned().collect();
        v.sort();
        v
    }

    fn variant<'a>(u: &'a TmUnit, l: &str) -> Option<&'a str> {
        let n = norm_lang(l);
        if let Some((_, t)) = u.variants.iter().find(|(k, _)| norm_lang(k) == n) {
            return Some(t);
        }
        let p = primary(&n);
        u.variants.iter().find(|(k, _)| primary(&norm_lang(k)) == p).map(|(_, t)| t.as_str())
    }

    /// Adds (or updates) a translation pair. Returns `true` if the TM changed.
    pub fn add(&mut self, src_lang: &str, src: &str, tgt_lang: &str, tgt: &str) -> bool {
        let (src_core, tgt_core) = (src.trim(), tgt.trim());
        if src_core.is_empty() || tgt_core.is_empty() {
            return false;
        }
        let candidates: Vec<usize> = self
            .lang_keys(src_lang)
            .iter()
            .filter_map(|l| self.index.get(l).and_then(|m| m.get(&key(src_core))))
            .flatten()
            .copied()
            .filter(|&i| self.units.get(i).and_then(|u| Self::variant(u, src_lang)) == Some(src_core))
            .collect();
        let tag = lang::tag(tgt_lang);
        for &i in &candidates {
            let Some(u) = self.units.get_mut(i) else {
                continue;
            };
            match u.variants.get(&tag) {
                Some(t) if t == tgt_core => return false,
                Some(_) => {}
                None => {
                    u.variants.insert(tag, tgt_core.to_string());
                    u.changed = tmx_stamp();
                    self.index_unit(i);
                    self.dirty = true;
                    return true;
                }
            }
        }
        // New unit (also when the same source already has a different translation: TMX allows several units).
        let mut variants = BTreeMap::new();
        variants.insert(lang::tag(src_lang), src_core.to_string());
        variants.insert(tag, tgt_core.to_string());
        let stamp = tmx_stamp();
        self.units.push(TmUnit { variants, created: stamp.clone(), changed: stamp });
        self.index_unit(self.units.len() - 1);
        self.dirty = true;
        true
    }

    /// Exact (white space and, as fallback, case insensitive) lookup. The returned text carries
    /// the original's leading/trailing white space.
    pub fn lookup(&self, src_lang: &str, src: &str, tgt_lang: &str) -> Option<TmMatch> {
        let (pre, core, post) = split_ws(src);
        if core.is_empty() {
            return None;
        }
        let mut best: Option<(bool, &str, &str)> = None;
        for l in self.lang_keys(src_lang) {
            let Some(ids) = self.index.get(&l).and_then(|m| m.get(&key(core))) else {
                continue;
            };
            for &i in ids.iter().rev() {
                let Some(u) = self.units.get(i) else {
                    continue;
                };
                let (Some(s), Some(t)) = (Self::variant(u, src_lang), Self::variant(u, tgt_lang)) else {
                    continue;
                };
                let exact = s == core;
                if best.is_none_or(|(e, ..)| exact && !e) {
                    best = Some((exact, s, t));
                }
            }
        }
        let (exact, s, t) = best?;
        let text = if exact { t.to_string() } else { adapt_case(core, t) };
        Some(TmMatch { source: s.to_string(), text: format!("{pre}{text}{post}"), score: 100 })
    }

    /// Fuzzy matches with a score of at least `min_score`, best first.
    pub fn fuzzy(&self, src_lang: &str, src: &str, tgt_lang: &str, min_score: u8, max: usize) -> Vec<TmMatch> {
        let (pre, core, post) = split_ws(src);
        let n = core.chars().count();
        if n == 0 {
            return Vec::new();
        }
        let tolerance = n * usize::from(100 - min_score.min(100)) / 100 + 1;
        let mut out: Vec<TmMatch> = Vec::new();
        for u in &self.units {
            let (Some(s), Some(t)) = (Self::variant(u, src_lang), Self::variant(u, tgt_lang)) else {
                continue;
            };
            if s.chars().count().abs_diff(n) > tolerance {
                continue;
            }
            let score = similarity(core, s);
            if score >= min_score && !out.iter().any(|m| m.source == s && m.text.trim() == t) {
                out.push(TmMatch { source: s.to_string(), text: format!("{pre}{t}{post}"), score });
            }
        }
        out.sort_by_key(|m| std::cmp::Reverse(m.score));
        out.truncate(max);
        out
    }

    /// Removes all units (and saves nothing).
    pub fn clear(&mut self) {
        self.units.clear();
        self.index.clear();
        self.dirty = true;
    }

    /// Removes the unit at `i`.
    pub fn remove(&mut self, i: usize) {
        if i < self.units.len() {
            self.units.remove(i);
            self.reindex();
            self.dirty = true;
        }
    }

    pub fn from_tmx(bytes: &[u8]) -> Result<Self> {
        let doc = xml::parse_bytes(bytes)?;
        if doc.root.name != "tmx" {
            return Err(Error::Format("not a TMX file (root element is not <tmx>)".into()));
        }
        let mut tm = Self::new();
        if let Some(body) = doc.root.child("body") {
            for tu in body.elements().filter(|e| e.name == "tu") {
                let mut u = TmUnit {
                    created: tu.attr("creationdate").unwrap_or_default().to_string(),
                    changed: tu.attr("changedate").unwrap_or_default().to_string(),
                    ..Default::default()
                };
                for tuv in tu.elements().filter(|e| e.name == "tuv") {
                    let Some(l) = tuv.attr("xml:lang").or_else(|| tuv.attr("lang")) else {
                        continue;
                    };
                    if let Some(seg) = tuv.child("seg") {
                        u.variants.insert(l.to_string(), deep_text(seg));
                    }
                }
                if u.variants.len() >= 2 {
                    tm.units.push(u);
                }
            }
        }
        tm.reindex();
        Ok(tm)
    }

    pub fn to_tmx(&self) -> String {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<tmx version=\"1.4\">\n");
        s.push_str(&format!(
            "  <header creationtool=\"susi\" creationtoolversion=\"{}\" datatype=\"plaintext\" segtype=\"sentence\" adminlang=\"en-US\" srclang=\"*all*\" o-tmf=\"susi\"/>\n  <body>\n",
            env!("CARGO_PKG_VERSION")
        ));
        for u in &self.units {
            s.push_str("    <tu");
            if !u.created.is_empty() {
                s.push_str(&format!(" creationdate=\"{}\"", escape_attr(&u.created)));
            }
            if !u.changed.is_empty() && u.changed != u.created {
                s.push_str(&format!(" changedate=\"{}\"", escape_attr(&u.changed)));
            }
            s.push_str(">\n");
            for (l, t) in &u.variants {
                s.push_str(&format!("      <tuv xml:lang=\"{}\"><seg>{}</seg></tuv>\n", escape_attr(l), escape_text(t)));
            }
            s.push_str("    </tu>\n");
        }
        s.push_str("  </body>\n</tmx>\n");
        s
    }
}

/// Text content including inline elements (`<ph>`, `<bpt>`, ... contribute their text).
fn deep_text(e: &Element) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c {
            Node::Text(t) | Node::CData(t) => s.push_str(t),
            Node::Element(child) => s.push_str(&deep_text(child)),
            _ => {}
        }
    }
    s
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslateOptions {
    pub langs: Vec<String>,
    /// Also replace existing translations (default: only untranslated cells).
    pub overwrite: bool,
    /// Minimum fuzzy score; `None` disables fuzzy matches.
    pub fuzzy: Option<u8>,
}

impl Default for TranslateOptions {
    fn default() -> Self {
        Self { langs: Vec::new(), overwrite: false, fuzzy: Some(75) }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TranslateReport {
    pub exact: usize,
    pub fuzzy: usize,
    pub not_found: usize,
}

impl TranslateReport {
    pub fn summary(&self) -> String {
        crate::tf!("{exact} translated exactly, {fuzzy} as best guess, {none} without match", exact = self.exact, fuzzy = self.fuzzy, none = self.not_found)
    }
}

/// Translates rows from the TM. Exact matches get "Automatisch übersetzt", fuzzy matches "Vermutung".
pub fn translate_project(p: &mut Project, tm: &TranslationMemory, opts: &TranslateOptions, rows: Option<&[RowRef]>) -> TranslateReport {
    let mut rep = TranslateReport::default();
    let langs = if opts.langs.is_empty() { p.language_ids() } else { opts.langs.clone() };
    let refs = rows.map(<[RowRef]>::to_vec).unwrap_or_else(|| p.rows());
    for r in refs {
        let orig_lang = p.original_lang(&r).to_string();
        let Some(row) = p.row_mut(&r) else {
            continue;
        };
        if row.locked() || row.excluded() || row.original().trim().is_empty() || row.raw {
            continue;
        }
        for l in langs.iter().filter(|l| **l != orig_lang) {
            if !opts.overwrite && row.tstatus(l) != TranslationStatus::NotTranslated {
                continue;
            }
            if let Some(m) = tm.lookup(&orig_lang, row.original(), l) {
                row.set_translation(l, &m.text, TranslationStatus::AutoTranslated);
                rep.exact += 1;
            } else if let Some(m) = opts.fuzzy.and_then(|min| tm.fuzzy(&orig_lang, row.original(), l, min, 1).into_iter().next()) {
                row.set_translation(l, &m.text, TranslationStatus::BestGuess);
                rep.fuzzy += 1;
            } else {
                rep.not_found += 1;
            }
        }
    }
    rep
}

/// Adds translations with one of `statuses` (default: Translated, Complete) to the TM and marks them
/// `savedtotm="1"`. Returns the number of pairs added.
pub fn save_to_tm(p: &mut Project, tm: &mut TranslationMemory, langs: &[String], statuses: &[TranslationStatus], mark: bool) -> usize {
    let statuses: Vec<TranslationStatus> =
        if statuses.is_empty() { vec![TranslationStatus::Translated, TranslationStatus::Complete] } else { statuses.to_vec() };
    let langs = if langs.is_empty() { p.language_ids() } else { langs.to_vec() };
    let mut n = 0;
    for r in p.rows() {
        let orig_lang = p.original_lang(&r).to_string();
        let Some(row) = p.row_mut(&r) else {
            continue;
        };
        if row.raw || row.excluded() {
            continue;
        }
        let original = row.original().to_string();
        for l in langs.iter().filter(|l| **l != orig_lang) {
            if !statuses.contains(&row.tstatus(l)) {
                continue;
            }
            let Some(t) = row.translation_mut(l) else {
                continue;
            };
            if tm.add(&orig_lang, &original, l, &t.text) {
                n += 1;
            }
            if mark {
                t.attrs.set("savedtotm", "1");
            }
        }
    }
    n
}

/// Removes all `savedtotm` marks (Projekt | "Gespeichert-im-TM"-Status löschen).
pub fn clear_saved_to_tm(p: &mut Project) -> usize {
    let mut n = 0;
    for r in p.rows() {
        if let Some(row) = p.row_mut(&r) {
            for t in &mut row.translations {
                if t.attrs.remove("savedtotm").is_some() {
                    n += 1;
                }
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_case_and_whitespace() {
        let mut tm = TranslationMemory::new();
        assert!(tm.add("en", " Hello ", "de", "Hallo"));
        assert!(!tm.add("en", "Hello", "de", "Hallo"));
        assert_eq!(tm.lookup("en", "Hello", "de").unwrap().text, "Hallo");
        assert_eq!(tm.lookup("en", "  Hello:", "de"), None);
        assert_eq!(tm.lookup("en", " hello ", "de").unwrap().text, " hallo ");
        assert_eq!(tm.lookup("en-US", "Hello", "de-DE").unwrap().text, "Hallo");
    }

    #[test]
    fn fuzzy_and_tmx_roundtrip() {
        let mut tm = TranslationMemory::new();
        tm.add("en", "Open the file", "de", "Datei öffnen");
        tm.add("en", "A & B <c>", "fr", "A & B <c> fr");
        let m = tm.fuzzy("en", "Open the files", "de", 75, 3);
        assert_eq!(m.len(), 1);
        assert!(m[0].score >= 90);
        let tm2 = TranslationMemory::from_tmx(tm.to_tmx().as_bytes()).unwrap();
        assert_eq!(tm2.len(), 2);
        assert_eq!(tm2.lookup("en", "A & B <c>", "fr").unwrap().text, "A & B <c> fr");
    }

    #[test]
    fn similarity_bounds() {
        assert_eq!(similarity("abc", "abc"), 100);
        assert_eq!(similarity("", "abc"), 0);
        assert_eq!(similarity("abcd", "abce"), 75);
    }
}
