//! Tool logic of the MCP server, independent of the transport (unit-testable).
//!
//! Rows are addressed by `key` (context: node names and row id joined by `/`, as shown in the
//! GUI's "Kontext" column) and, if the key is ambiguous, the `source` index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use susi_core::exchange::{self, ExportOptions, ImportOptions};
use susi_core::filter::{DateField, RowFilter, SearchIn, parse_date};
use susi_core::model::{Project, RowRef, display_stamp};
use susi_core::status::{RowStatus, TranslationStatus};
use susi_core::tm::{self, TranslateOptions, TranslationMemory};
use susi_core::validate::{self, ValidateOptions, ValidationKind};
use susi_core::{lang, ops, pseudo, slp, stats};

pub type ToolResult = Result<Value, String>;

#[derive(Debug, Default)]
pub struct Session {
    pub project: Option<Project>,
    pub dirty: bool,
    pub tm: Option<TranslationMemory>,
    /// TM file; default from the user settings.
    pub tm_path: Option<PathBuf>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn parse_status(s: &str) -> Result<TranslationStatus, String> {
    TranslationStatus::from_keyword(s).ok_or_else(|| format!("unknown status '{s}' (use untranslated|best|auto|translated|review|complete)"))
}

fn parse_statuses(v: &[String]) -> Result<Vec<TranslationStatus>, String> {
    v.iter().map(|s| parse_status(s)).collect()
}

/// Row lookup table for one project state.
struct Keys {
    map: HashMap<String, Vec<RowRef>>,
}

impl Keys {
    fn new(p: &Project) -> Self {
        let mut map: HashMap<String, Vec<RowRef>> = HashMap::new();
        for r in p.rows() {
            map.entry(p.context(&r)).or_default().push(r);
        }
        Self { map }
    }

    fn find(&self, p: &Project, key: &str, source: Option<usize>) -> Result<RowRef, String> {
        let mut c: Vec<&RowRef> = self.map.get(key).map(|v| v.iter().collect()).unwrap_or_default();
        if c.is_empty() {
            c = self.map.values().flatten().filter(|r| p.row(r).is_some_and(|x| x.id() == key)).collect();
        }
        if let Some(s) = source {
            c.retain(|r| r.source == s);
        }
        match c.as_slice() {
            [r] => Ok((*r).clone()),
            [] => Err(format!("row '{key}' not found")),
            _ => Err(format!("row key '{key}' is ambiguous ({} matches) – pass 'source'", c.len())),
        }
    }
}

fn date_bound(s: Option<&str>, end: bool) -> Result<Option<String>, String> {
    s.map(|v| parse_date(v, end).ok_or_else(|| format!("invalid date '{v}' (use e.g. 2026-11-01, \"2026-11-01 14:30\", today, 7d)"))).transpose()
}

pub fn row_json(p: &Project, r: &RowRef, langs: &[String]) -> Value {
    let Some(row) = p.row(r) else {
        return Value::Null;
    };
    let orig = p.original_lang(r);
    let mut tr = serde_json::Map::new();
    for l in langs.iter().filter(|l| l.as_str() != orig) {
        let mut o = json!({ "text": row.text(l).unwrap_or(""), "status": row.tstatus(l).keyword() });
        if let Some(t) = row.translation(l) {
            if !t.comment().is_empty() {
                o["comment"] = json!(t.comment());
            }
            if t.attrs.get("invalidated") == Some("1") {
                o["invalidated"] = json!(true);
            }
            if let Some(d) = row.translation_date(l) {
                o["date"] = json!(display_stamp(d));
            }
        }
        tr.insert(l.clone(), o);
    }
    let mut v = json!({
        "source": r.source,
        "key": p.context(r),
        "original": row.original(),
        "original_lang": orig,
        "row_status": row.status().keyword(),
        "translations": tr,
    });
    if !row.note().is_empty() {
        v["comment"] = json!(row.note());
    }
    if row.max_chars() > 0 {
        v["max_chars"] = json!(row.max_chars());
    }
    if row.locked() {
        v["do_not_translate"] = json!(true);
    }
    for name in ["created", "changed"] {
        if let Some(d) = row.attrs.get(name).filter(|d| !d.is_empty()) {
            v[name] = json!(display_stamp(d));
        }
    }
    let ph = validate::placeholders(row.original());
    if !ph.is_empty() {
        v["placeholders"] = json!(ph);
    }
    v
}

#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct TranslationInput {
    /// Row key (context) as returned by list_rows.
    pub key: String,
    /// Source index, only needed if the key is ambiguous.
    pub source: Option<usize>,
    /// Target language id (e.g. "de").
    pub lang: String,
    /// Translated text. An empty string removes the translation.
    pub text: String,
    /// Status: untranslated|best|auto|translated|review|complete. Default: auto.
    pub status: Option<String>,
    /// Optional translator comment.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListRowsInput {
    /// Languages to include (default: all project languages).
    #[serde(default)]
    pub langs: Vec<String>,
    /// Only rows where any of `langs` has one of these statuses (untranslated|best|auto|translated|review|complete).
    #[serde(default)]
    pub status: Vec<String>,
    /// Only rows with these row statuses (new|unused|inuse|changed).
    #[serde(default)]
    pub row_status: Vec<String>,
    /// Text search in original, translations, key and comments (case-insensitive).
    pub text: Option<String>,
    /// Restrict to one source (index).
    pub source: Option<usize>,
    /// Only rows whose timestamp (see `date_field`) is at or after this local date/time:
    /// "2026-11-01", "2026-11-01 14:30", "today", "yesterday", "7d" (days ago), "12h" (hours ago).
    pub since: Option<String>,
    /// Only rows whose timestamp is at or before this local date/time (a date includes the whole day).
    pub until: Option<String>,
    /// Timestamp for since/until: any (default) | created (row added by a scan) | changed (original changed) | translated (translation of `langs` edited).
    pub date_field: Option<String>,
    #[serde(default)]
    pub offset: usize,
    /// Maximum rows to return (default 100, max 1000).
    pub limit: Option<usize>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    fn project(&self) -> Result<&Project, String> {
        self.project.as_ref().ok_or_else(|| "no project open – call open_project first".to_string())
    }

    fn project_mut(&mut self) -> Result<&mut Project, String> {
        self.project.as_mut().ok_or_else(|| "no project open – call open_project first".to_string())
    }

    fn tm_mut(&mut self) -> Result<&mut TranslationMemory, String> {
        if self.tm.is_none() {
            let path = self.tm_path.clone().unwrap_or_else(|| susi_core::settings::Settings::load().tm_path());
            self.tm = Some(TranslationMemory::open(&path).map_err(err)?);
        }
        self.tm.as_mut().ok_or_else(|| "translation memory unavailable".to_string())
    }

    pub fn open(&mut self, path: &Path) -> ToolResult {
        let p = slp::load(path).map_err(err)?;
        self.project = Some(p);
        self.dirty = false;
        self.info()
    }

    pub fn create(&mut self, path: &Path, langs: &[String]) -> ToolResult {
        let mut p = Project::new();
        for l in langs {
            p.add_language(l);
        }
        p.path = Some(path.to_path_buf());
        slp::save(&mut p, path).map_err(err)?;
        self.project = Some(p);
        self.dirty = false;
        self.info()
    }

    pub fn save(&mut self, path: Option<&Path>) -> ToolResult {
        let p = self.project_mut()?;
        let target = path.map(Path::to_path_buf).or_else(|| p.path.clone()).ok_or("project has no file name – pass 'path'")?;
        p.touch();
        slp::save(p, &target).map_err(err)?;
        p.path = Some(target.clone());
        self.dirty = false;
        if let Some(tm) = self.tm.as_mut().filter(|t| t.dirty) {
            tm.save().map_err(err)?;
        }
        Ok(json!({ "saved": target }))
    }

    pub fn info(&self) -> ToolResult {
        let p = self.project()?;
        let sources: Vec<Value> = p
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (dir, file) = s.output_pattern();
                json!({ "index": i, "name": s.name(), "type": s.kind().label(), "original_lang": s.original(), "rows": s.row_count(), "output_dir": dir, "output_file": file, "write_method": format!("{:?}", s.write_method()) })
            })
            .collect();
        let st = stats::statistics(p, None);
        let langs: Vec<Value> = st.languages.iter().map(|l| json!({ "id": l.lang, "name": lang::name_en(&l.lang), "translated_percent": (l.percent() * 10.0).round() / 10.0, "untranslated": l.status[0] })).collect();
        Ok(json!({ "path": p.path, "dirty": self.dirty, "rows": st.rows, "words": st.words, "languages": langs, "sources": sources }))
    }

    pub fn list_rows(&self, q: &ListRowsInput) -> ToolResult {
        let p = self.project()?;
        let f = RowFilter {
            text: q.text.clone().unwrap_or_default(),
            search_in: SearchIn::All,
            langs: q.langs.clone(),
            statuses: parse_statuses(&q.status)?,
            row_statuses: q
                .row_status
                .iter()
                .map(|s| RowStatus::from_keyword(s).ok_or_else(|| format!("unknown row status '{s}'")))
                .collect::<Result<_, _>>()?,
            sources: q.source.into_iter().collect(),
            date_field: q
                .date_field
                .as_deref()
                .map(|s| DateField::from_keyword(s).ok_or_else(|| format!("unknown date_field '{s}' (any|created|changed|translated)")))
                .transpose()?
                .unwrap_or_default(),
            since: date_bound(q.since.as_deref(), false)?,
            until: date_bound(q.until.as_deref(), true)?,
            ..Default::default()
        };
        let rows = f.apply(p);
        let langs = if q.langs.is_empty() { p.language_ids() } else { q.langs.clone() };
        let limit = q.limit.unwrap_or(100).clamp(1, 1000);
        let page: Vec<Value> = rows.iter().skip(q.offset).take(limit).map(|r| row_json(p, r, &langs)).collect();
        Ok(json!({ "total": rows.len(), "offset": q.offset, "returned": page.len(), "rows": page }))
    }

    pub fn get_row(&self, key: &str, source: Option<usize>) -> ToolResult {
        let p = self.project()?;
        let r = Keys::new(p).find(p, key, source)?;
        Ok(row_json(p, &r, &p.language_ids()))
    }

    pub fn set_translations(&mut self, items: &[TranslationInput]) -> ToolResult {
        let p = self.project_mut()?;
        let keys = Keys::new(p);
        let mut ok = 0;
        let mut errors = Vec::new();
        for it in items {
            let res = (|| -> Result<(), String> {
                if !p.has_language(&it.lang) {
                    return Err(format!("language '{}' is not part of the project", it.lang));
                }
                let r = keys.find(p, &it.key, it.source)?;
                if p.original_lang(&r) == it.lang {
                    return Err(format!("'{}' is the original language of this row", it.lang));
                }
                let status = it.status.as_deref().map(parse_status).transpose()?.unwrap_or(TranslationStatus::AutoTranslated);
                let row = p.row_mut(&r).ok_or("row vanished")?;
                if row.locked() {
                    return Err("row is marked 'do not translate'".into());
                }
                row.set_translation(&it.lang, &it.text, status);
                if let (Some(c), Some(t)) = (&it.comment, row.translation_mut(&it.lang)) {
                    t.set_comment(c);
                }
                Ok(())
            })();
            match res {
                Ok(()) => ok += 1,
                Err(e) => errors.push(json!({ "key": it.key, "lang": it.lang, "error": e })),
            }
        }
        if ok > 0 {
            self.dirty = true;
        }
        Ok(json!({ "updated": ok, "errors": errors }))
    }

    pub fn set_status(&mut self, keys_in: &[String], source: Option<usize>, lang: &str, status: &str) -> ToolResult {
        let st = parse_status(status)?;
        let p = self.project_mut()?;
        let keys = Keys::new(p);
        let mut n = 0;
        let mut errors = Vec::new();
        for k in keys_in {
            match keys.find(p, k, source) {
                Ok(r) => {
                    if p.row_mut(&r).is_some_and(|row| row.set_tstatus(lang, st)) {
                        n += 1;
                    } else {
                        errors.push(json!({ "key": k, "error": "no translation to change" }));
                    }
                }
                Err(e) => errors.push(json!({ "key": k, "error": e })),
            }
        }
        self.dirty |= n > 0;
        Ok(json!({ "updated": n, "errors": errors }))
    }

    pub fn set_row_props(
        &mut self,
        key: &str,
        source: Option<usize>,
        comment: Option<&str>,
        do_not_translate: Option<bool>,
        max_chars: Option<usize>,
    ) -> ToolResult {
        let p = self.project_mut()?;
        let r = Keys::new(p).find(p, key, source)?;
        let row = p.row_mut(&r).ok_or("row vanished")?;
        if let Some(c) = comment {
            row.set_note(c);
        }
        if let Some(l) = do_not_translate {
            row.set_locked(l);
        }
        if let Some(m) = max_chars {
            row.set_max_chars(m);
        }
        let v = row_json(p, &r, &p.language_ids());
        self.dirty = true;
        Ok(v)
    }

    pub fn add_language(&mut self, id: &str) -> ToolResult {
        let added = self.project_mut()?.add_language(id);
        self.dirty |= added;
        Ok(json!({ "added": added, "name": lang::name_en(id) }))
    }

    pub fn add_source(&mut self, file: &Path, original_lang: &str) -> ToolResult {
        let p = self.project_mut()?;
        let (i, rep) = ops::add_source(p, file, original_lang).map_err(err)?;
        self.dirty = true;
        Ok(json!({ "source": i, "scan": rep.summary(), "new_rows": rep.new }))
    }

    pub fn scan(&mut self, sources: Option<&[usize]>) -> ToolResult {
        let p = self.project_mut()?;
        let rep = ops::scan_project(p, sources).map_err(err)?;
        self.dirty = true;
        Ok(json!({ "new": rep.new, "changed": rep.changed, "unused": rep.unused, "summary": rep.summary() }))
    }

    pub fn build(&self, langs: &[String], sources: Option<&[usize]>) -> ToolResult {
        let rep = ops::build_project(self.project()?, langs, sources);
        Ok(json!({ "written": rep.written, "unchanged": rep.unchanged.len(), "errors": rep.errors, "summary": rep.summary() }))
    }

    pub fn validate(&self, langs: &[String], kinds: &[String]) -> ToolResult {
        let p = self.project()?;
        let mut opts = ValidateOptions { langs: langs.to_vec(), ..Default::default() };
        if !kinds.is_empty() {
            opts.kinds = kinds.iter().map(|k| ValidationKind::from_keyword(k).ok_or_else(|| format!("unknown validation '{k}'"))).collect::<Result<_, _>>()?;
        }
        let issues = validate::validate(p, &opts, None);
        Ok(json!({ "count": issues.len(), "issues": issues }))
    }

    pub fn statistics(&self) -> ToolResult {
        let st = stats::statistics(self.project()?, None);
        serde_json::to_value(&st).map_err(err)
    }

    pub fn tm_translate(&mut self, langs: &[String], overwrite: bool, fuzzy: Option<u8>) -> ToolResult {
        self.tm_mut()?;
        let (Some(p), Some(tm)) = (self.project.as_mut(), self.tm.as_ref()) else {
            return Err("no project open".into());
        };
        let rep = tm::translate_project(p, tm, &TranslateOptions { langs: langs.to_vec(), overwrite, fuzzy }, None);
        self.dirty = true;
        Ok(json!({ "exact": rep.exact, "fuzzy": rep.fuzzy, "not_found": rep.not_found }))
    }

    pub fn tm_lookup(&mut self, text: &str, src_lang: &str, tgt_lang: &str, min_score: u8) -> ToolResult {
        let tm = self.tm_mut()?;
        let mut m: Vec<tm::TmMatch> = tm.lookup(src_lang, text, tgt_lang).into_iter().collect();
        if m.is_empty() {
            m = tm.fuzzy(src_lang, text, tgt_lang, min_score, 5);
        }
        Ok(json!({ "matches": m }))
    }

    pub fn tm_save(&mut self, langs: &[String]) -> ToolResult {
        self.tm_mut()?;
        let (Some(p), Some(tm)) = (self.project.as_mut(), self.tm.as_mut()) else {
            return Err("no project open".into());
        };
        let n = tm::save_to_tm(p, tm, langs, &[], true);
        tm.save().map_err(err)?;
        self.dirty = true;
        Ok(json!({ "added": n, "tm_size": tm.len(), "tm_path": tm.path }))
    }

    pub fn export(&self, path: &Path, langs: &[String], statuses: &[String]) -> ToolResult {
        let opts = ExportOptions { langs: langs.to_vec(), statuses: parse_statuses(statuses)?, ..Default::default() };
        let files = exchange::export_file(self.project()?, path, &opts).map_err(err)?;
        Ok(json!({ "files": files }))
    }

    pub fn import(&mut self, path: &Path, langs: &[String], overwrite: bool) -> ToolResult {
        let opts = ImportOptions { langs: langs.to_vec(), overwrite, ..Default::default() };
        let rep = exchange::import_file(self.project_mut()?, path, &opts).map_err(err)?;
        self.dirty |= rep.updated > 0;
        let mut v = serde_json::to_value(&rep).map_err(err)?;
        v["summary"] = json!(rep.summary());
        Ok(v)
    }

    pub fn fill_pseudo(&mut self, lang: &str) -> ToolResult {
        let p = self.project_mut()?;
        if !p.has_language(lang) {
            pseudo::set_pseudo_language(p, lang, pseudo::PseudoKind::Emulation);
        }
        let n = pseudo::fill_pseudo(p, lang, &pseudo::PseudoOptions::default(), None, false);
        self.dirty = true;
        Ok(json!({ "filled": n }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.json"), "{\n  \"menu\": { \"open\": \"Open {0}\", \"close\": \"Close\" }\n}").unwrap();
        (dir, PathBuf::new())
    }

    #[test]
    fn translate_via_session() {
        let (dir, _) = fixture();
        let slp_path = dir.path().join("app.slp");
        let mut s = Session::new();
        s.tm_path = Some(dir.path().join("tm.tmx"));
        s.create(&slp_path, &["en".into(), "de".into()]).unwrap();
        s.add_source(&dir.path().join("app.json"), "en").unwrap();
        let rows = s.list_rows(&ListRowsInput { status: vec!["untranslated".into()], langs: vec!["de".into()], ..Default::default() }).unwrap();
        assert_eq!(rows["total"], 2);
        assert_eq!(rows["rows"][0]["key"], "app.json/menu.open");
        assert_eq!(rows["rows"][0]["placeholders"][0], "{0}");
        let r = s
            .set_translations(&[
                TranslationInput { key: "menu.open".into(), lang: "de".into(), text: "{0} öffnen".into(), ..Default::default() },
                TranslationInput { key: "nope".into(), lang: "de".into(), text: "x".into(), ..Default::default() },
            ])
            .unwrap();
        assert_eq!(r["updated"], 1);
        assert_eq!(r["errors"].as_array().unwrap().len(), 1);
        assert_eq!(s.get_row("menu.open", None).unwrap()["translations"]["de"]["status"], "auto");
        s.set_status(&["menu.open".into()], None, "de", "complete").unwrap();
        s.save(None).unwrap();
        let built = s.build(&[], None).unwrap();
        assert!(built["errors"].as_array().unwrap().is_empty(), "{built}");
        let de = std::fs::read_to_string(dir.path().join("de").join("app.json")).unwrap();
        assert!(de.contains("{0} öffnen"), "{de}");
        let saved = s.tm_save(&[]).unwrap();
        assert_eq!(saved["added"], 1);
    }
}
