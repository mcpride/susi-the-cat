//! Tool logic of the MCP server, independent of the transport (unit-testable).
//!
//! Rows are addressed by `key` (context: node names and row id joined by `/`, as shown in the
//! GUI's "Kontext" column) and, if the key is ambiguous, the `source` index.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use susi_core::custom_data::{self, DataKind, PictureInfo};
use susi_core::exchange::{self, ExportOptions, ImportOptions, ScreenshotExport};
use susi_core::filter::{DateField, RowFilter, SearchIn, parse_date};
use susi_core::keys::{KeyError, RowKeys};
use susi_core::model::{Project, RowRef, display_stamp};
use susi_core::screenshots::{Area, Link, ScreenshotIndex, ScreenshotStore};
use susi_core::status::{RowStatus, TranslationStatus};
use susi_core::tm::{self, TranslateOptions, TranslationMemory};
use susi_core::validate::{self, ValidateOptions, ValidationKind};
use susi_core::{lang, ops, pseudo, slp, stats};
use susi_workflow::{FetchOptions, StatusOptions, Workflow};

pub type ToolResult = Result<Value, String>;

#[derive(Debug, Default)]
pub struct Session {
    pub project: Option<Project>,
    pub dirty: bool,
    pub tm: Option<TranslationMemory>,
    /// TM file; default from the user settings.
    pub tm_path: Option<PathBuf>,
    /// Screenshots of the project (`<project>.files/`), saved with it.
    pub shots: Option<ScreenshotStore>,
    /// Why the screenshots could not be loaded.
    pub shots_error: Option<String>,
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

/// Row lookup table for one project state: context first, then the plain row id (see [`RowKeys`]).
struct Keys(RowKeys);

impl Keys {
    fn new(p: &Project) -> Self {
        Self(RowKeys::new(p))
    }

    fn find(&self, key: &str, source: Option<usize>) -> Result<RowRef, String> {
        self.0.find(key, source).map_err(|e| match e {
            KeyError::NotFound => format!("row '{key}' not found"),
            KeyError::Ambiguous(n) => format!("row key '{key}' is ambiguous ({n} matches) – pass 'source'"),
        })
    }
}

fn date_bound(s: Option<&str>, end: bool) -> Result<Option<String>, String> {
    s.map(|v| parse_date(v, end).ok_or_else(|| format!("invalid date '{v}' (use e.g. 2026-11-01, \"2026-11-01 14:30\", today, 7d)"))).transpose()
}

/// Picture or binary data: `{format: picture|binary, size, type, width, height}` (only what is known;
/// the original of binary data is not stored in the project).
fn data_json(kind: DataKind, bytes: Option<&[u8]>) -> Value {
    let mut v = json!({ "format": kind.keyword() });
    if let Some(b) = bytes {
        v["size"] = json!(b.len());
        if let Some(i) = PictureInfo::of(b) {
            v["type"] = json!(i.format.name());
            v["width"] = json!(i.width);
            v["height"] = json!(i.height);
        }
    }
    v
}

pub fn row_json(p: &Project, r: &RowRef, langs: &[String]) -> Value {
    let Some(row) = p.row(r) else {
        return Value::Null;
    };
    let orig = p.original_lang(r);
    let kind = row.data_kind();
    let mut tr = serde_json::Map::new();
    for l in langs.iter().filter(|l| l.as_str() != orig) {
        let mut o = match kind {
            // Pictures and binary data: a description instead of hex text.
            Some(k) => json!({ "data": data_json(k, row.translation_bytes(l).as_deref()), "status": row.tstatus(l).keyword() }),
            None => json!({ "text": row.text(l).unwrap_or(""), "status": row.tstatus(l).keyword() }),
        };
        if let Some(t) = row.translation(l) {
            if let Some(f) = t.linked_file().filter(|_| kind.is_some()) {
                o["file"] = json!(f);
            }
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
        "original": if kind.is_some() { Value::Null } else { json!(row.original()) },
        "original_lang": orig,
        "row_status": row.status().keyword(),
        "translations": tr,
    });
    if let Some(k) = kind {
        v["data"] = data_json(k, row.original_bytes().as_deref());
    }
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

/// Adds `screenshots: [{id, title, area}]` to a row object if the row has screenshots.
fn add_screenshots(v: &mut Value, shots: &ScreenshotStore, idx: &ScreenshotIndex, r: &RowRef) {
    let list: Vec<Value> = shots
        .links_of(idx, r)
        .iter()
        .map(|(s, l)| {
            let mut o = json!({ "id": s.id, "title": s.label() });
            if let Some(a) = l.area {
                o["area"] = json!(<[u32; 4]>::from(a));
            }
            o
        })
        .collect();
    if !list.is_empty() {
        v["screenshots"] = json!(list);
    }
}

/// Rows of a screenshot link.
#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ScreenshotKeyInput {
    /// Row key (context as returned by list_rows, or a row id); wildcards * and ? select several rows.
    pub key: String,
    /// Source index, only needed if the key is ambiguous.
    pub source: Option<usize>,
    /// Area of the text in the image in pixels: [x, y, width, height].
    pub area: Option<[u32; 4]>,
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
    /// Only rows with (true) or without (false) a linked screenshot.
    pub has_screenshot: Option<bool>,
    #[serde(default)]
    pub offset: usize,
    /// Maximum rows to return (default 100, max 1000).
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowFetchInput {
    /// Branch to check out (default: the repository's default branch).
    pub branch: Option<String>,
    /// Specific commit, tag or remote branch to check out read-only (overrides `branch`).
    pub commit: Option<String>,
    /// Discard unsaved changes of the open project (default false: refuse when there are unsaved changes).
    pub discard_changes: Option<bool>,
}

#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowSaveInput {
    /// Commit message without prefix (default: from the workflow configuration).
    pub message: Option<String>,
    /// Conventional commit type for semantic-release: fix | feat | chore | "" (default: configuration).
    pub prefix: Option<String>,
    /// Skip the CI pipeline (default: configuration).
    pub ci_skip: Option<bool>,
    /// Name of a new branch (default: generated from the configured pattern when on the default branch).
    pub branch: Option<String>,
    /// Build the localized files first (default: configuration).
    pub build: Option<bool>,
}

#[derive(Debug, Clone, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowPublishInput {
    /// Merge request title (default: configuration).
    pub title: Option<String>,
    /// Merge request description (default: configuration).
    pub description: Option<String>,
    /// Reviewer usernames (default: configuration).
    pub reviewers: Option<Vec<String>>,
    /// Target branch (default: configuration or default branch).
    pub target: Option<String>,
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
        (self.shots, self.shots_error) = match ScreenshotStore::open(path) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
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
        (self.shots, self.shots_error) = (Some(ScreenshotStore::new(path)), None);
        self.info()
    }

    pub fn save(&mut self, path: Option<&Path>) -> ToolResult {
        let p = self.project_mut()?;
        let target = path.map(Path::to_path_buf).or_else(|| p.path.clone()).ok_or("project has no file name – pass 'path'")?;
        p.touch();
        slp::save(p, &target).map_err(err)?;
        p.path = Some(target.clone());
        if let Some(shots) = self.shots.as_mut() {
            shots.relocate(&target).map_err(err)?;
            shots.save().map_err(err)?;
        }
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
        let mut v = json!({ "path": p.path, "dirty": self.dirty, "rows": st.rows, "words": st.words, "languages": langs, "sources": sources });
        if let Some(shots) = self.shots.as_ref().filter(|s| !s.is_empty()) {
            let idx = shots.index(p);
            v["screenshots"] =
                json!({ "count": shots.len(), "rows_with_screenshots": idx.rows().len(), "orphaned_links": idx.orphan_count(), "dir": shots.dir() });
        }
        if let Some(e) = &self.shots_error {
            v["screenshot_error"] = json!(e);
        }
        Ok(v)
    }

    /// Screenshot links resolved against the project (`None` without screenshots).
    fn shot_index(&self, p: &Project) -> Option<(&ScreenshotStore, ScreenshotIndex)> {
        self.shots.as_ref().filter(|s| !s.is_empty()).map(|s| (s, s.index(p)))
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
            has_screenshot: q.has_screenshot,
            ..Default::default()
        };
        let shots = self.shot_index(p);
        let f = RowFilter { screenshot_rows: std::sync::Arc::new(shots.as_ref().map(|(_, i)| i.rows()).unwrap_or_default()), ..f };
        let rows = f.apply(p);
        let langs = if q.langs.is_empty() { p.language_ids() } else { q.langs.clone() };
        let limit = q.limit.unwrap_or(100).clamp(1, 1000);
        let page: Vec<Value> = rows
            .iter()
            .skip(q.offset)
            .take(limit)
            .map(|r| {
                let mut v = row_json(p, r, &langs);
                if let Some((s, i)) = &shots {
                    add_screenshots(&mut v, s, i, r);
                }
                v
            })
            .collect();
        Ok(json!({ "total": rows.len(), "offset": q.offset, "returned": page.len(), "rows": page }))
    }

    pub fn get_row(&self, key: &str, source: Option<usize>) -> ToolResult {
        let p = self.project()?;
        let r = Keys::new(p).find(key, source)?;
        let mut v = row_json(p, &r, &p.language_ids());
        if let Some((s, i)) = self.shot_index(p) {
            add_screenshots(&mut v, s, &i, &r);
        }
        Ok(v)
    }

    /// Image of screenshot `id` (bytes, MIME type) and its rows with areas.
    pub fn screenshot(&self, id: &str) -> Result<(Vec<u8>, &'static str, Value), String> {
        let p = self.project()?;
        let shots = self.shots.as_ref().ok_or("the project has no screenshots")?;
        let s = shots.get(id).ok_or_else(|| format!("screenshot '{id}' not found"))?;
        let path = shots.image_dir().join(&s.file);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mime = if susi_core::screenshots::image_kind(&bytes) == Some("png") { "image/png" } else { "image/jpeg" };
        let idx = shots.index(p);
        let rows: Vec<Value> = idx
            .rows()
            .into_iter()
            .flat_map(|r| shots.links_of(&idx, &r).into_iter().filter(|(x, _)| x.id == s.id).map(move |(_, l)| (r.clone(), l.area)))
            .map(|(r, area)| {
                let mut o = json!({ "key": p.context(&r), "source": r.source, "original": p.row(&r).map(|x| x.original()).unwrap_or("") });
                if let Some(a) = area {
                    o["area"] = json!(<[u32; 4]>::from(a));
                }
                o
            })
            .collect();
        let info = json!({ "id": s.id, "title": s.title, "width": s.width, "height": s.height, "rows": rows, "orphaned_links": shots.orphans(&idx).len() });
        Ok((bytes, mime, info))
    }

    /// Adds an image file as screenshot (or replaces the image of `id`) and links it to rows.
    pub fn attach_screenshot(&mut self, path: &Path, keys: &[ScreenshotKeyInput], id: Option<&str>, title: Option<&str>) -> ToolResult {
        let p = self.project.as_ref().ok_or("no project open – call open_project first")?;
        let index = RowKeys::new(p);
        let mut links = Vec::new();
        let mut errors = Vec::new();
        for k in keys {
            let rows = index.select(&k.key, k.source);
            if rows.is_empty() {
                errors.push(format!("no row matches '{}'", k.key));
            }
            links.extend(rows.iter().map(|r| Link::to_row(p, r, k.area.map(Area::from).and_then(|a| (a.w > 0 && a.h > 0).then_some(a)))));
        }
        let shots = match (&self.shots_error, self.shots.as_mut()) {
            (Some(e), _) => return Err(format!("screenshots unavailable: {e}")),
            (None, None) => return Err("project has no file name – save it first".into()),
            (None, Some(s)) => s,
        };
        let id = shots.add_file(path, id, title).map_err(err)?;
        let linked = shots.link(&id, links).map_err(err)?;
        self.dirty = true;
        Ok(json!({ "id": id, "linked_rows": linked, "errors": errors }))
    }

    /// Removes the links of screenshot `id` to the given rows, or the screenshot without keys.
    pub fn detach_screenshot(&mut self, id: &str, keys: &[ScreenshotKeyInput]) -> ToolResult {
        let p = self.project.as_ref().ok_or("no project open – call open_project first")?;
        let shots = self.shots.as_mut().ok_or("the project has no screenshots")?;
        if shots.get(id).is_none() {
            return Err(format!("screenshot '{id}' not found"));
        }
        if keys.is_empty() {
            shots.remove(id);
            self.dirty = true;
            return Ok(json!({ "removed": id }));
        }
        let index = RowKeys::new(p);
        let n: usize = keys.iter().flat_map(|k| index.select(&k.key, k.source)).map(|r| shots.unlink_row(p, id, &r)).sum();
        self.dirty |= n > 0;
        Ok(json!({ "unlinked": n }))
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
                let r = keys.find(&it.key, it.source)?;
                if p.original_lang(&r) == it.lang {
                    return Err(format!("'{}' is the original language of this row", it.lang));
                }
                let status = it.status.as_deref().map(parse_status).transpose()?.unwrap_or(TranslationStatus::AutoTranslated);
                let row = p.row_mut(&r).ok_or("row vanished")?;
                if row.locked() {
                    return Err("row is marked 'do not translate'".into());
                }
                if row.is_data() {
                    return Err("the row holds a picture or binary data: use load_translation_file".into());
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
            match keys.find(k, source) {
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

    /// Copies the original (`from` = `None`) or the translations in `from` into `lang` for the rows
    /// of `keys_in` (all rows, or all rows of `source`, without keys).
    pub fn copy_translations(&mut self, from: Option<&str>, lang: &str, keys_in: &[String], source: Option<usize>, overwrite: bool) -> ToolResult {
        let p = self.project_mut()?;
        if let Some(l) = std::iter::once(lang).chain(from).find(|l| !p.has_language(l)) {
            return Err(format!("language '{l}' is not part of the project"));
        }
        if from == Some(lang) {
            return Err("from and lang must be different languages".into());
        }
        if let Some(si) = source.filter(|&si| si >= p.sources.len()) {
            return Err(format!("source {si} does not exist ({} sources)", p.sources.len()));
        }
        let mut errors = Vec::new();
        let rows: Vec<RowRef> = if keys_in.is_empty() {
            p.rows().into_iter().filter(|r| source.is_none_or(|si| r.source == si)).collect()
        } else {
            let keys = Keys::new(p);
            keys_in
                .iter()
                .filter_map(|k| match keys.find(k, source) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        errors.push(json!({ "key": k, "error": e }));
                        None
                    }
                })
                .collect()
        };
        let kept = if overwrite { 0 } else { ops::copy_targets(p, &rows, from, lang).translated };
        let copied = ops::copy_cells(p, &rows, from, lang, overwrite).len();
        self.dirty |= copied > 0;
        Ok(json!({ "copied": copied, "kept": kept, "errors": errors }))
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
        let r = Keys::new(p).find(key, source)?;
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

    /// Reads translations of pictures and binary data again whose linked file changed.
    fn refresh_linked(&mut self) -> Result<Value, String> {
        let p = self.project_mut()?;
        let rep = custom_data::refresh_linked(p);
        let updated: Vec<String> = rep.updated.iter().map(|(r, lang)| format!("{} ({lang})", p.context(r))).collect();
        self.dirty |= !updated.is_empty();
        Ok(json!({ "updated": updated, "missing": rep.missing }))
    }

    pub fn scan(&mut self, sources: Option<&[usize]>) -> ToolResult {
        let linked = self.refresh_linked()?;
        let p = self.project_mut()?;
        let rep = ops::scan_project(p, sources).map_err(err)?;
        self.dirty = true;
        Ok(json!({ "new": rep.new, "changed": rep.changed, "unused": rep.unused, "summary": rep.summary(), "linked_files": linked }))
    }

    pub fn build(&mut self, langs: &[String], sources: Option<&[usize]>) -> ToolResult {
        let linked = self.refresh_linked()?;
        let rep = ops::build_project(self.project()?, langs, sources);
        Ok(json!({ "written": rep.written, "unchanged": rep.unchanged.len(), "errors": rep.errors, "summary": rep.summary(), "linked_files": linked }))
    }

    /// Picture of a row (the original, or the translation in `lang`) with its description.
    pub fn picture(&self, key: &str, source: Option<usize>, lang: Option<&str>) -> Result<(Vec<u8>, &'static str, Value), String> {
        let p = self.project()?;
        let r = Keys::new(p).find(key, source)?;
        let row = p.row(&r).ok_or("row vanished")?;
        let bytes = match (row.data_kind(), lang) {
            (None, _) => return Err(format!("'{key}' is a text row, not a picture")),
            (Some(_), Some(l)) => row.translation_bytes(l).ok_or_else(|| format!("'{key}' has no translation in '{l}'"))?,
            (Some(DataKind::Binary), None) => return Err("the original of binary data is not stored in the project".into()),
            (Some(DataKind::Picture), None) => row.original_bytes().ok_or("the picture has no original data")?,
        };
        let info = PictureInfo::of(&bytes).ok_or_else(|| format!("the data is no supported picture ({})", custom_data::summary(&bytes)))?;
        Ok((bytes, info.format.mime(), row_json(p, &r, &p.language_ids())))
    }

    /// Sets the translation of a picture or binary row from a file, optionally linked to it.
    pub fn load_translation_file(&mut self, key: &str, source: Option<usize>, lang: &str, path: &Path, link: bool) -> ToolResult {
        let p = self.project_mut()?;
        if !p.has_language(lang) {
            return Err(format!("language '{lang}' is not part of the project"));
        }
        let r = Keys::new(p).find(key, source)?;
        custom_data::load_translation_file(p, &r, lang, path, link).map_err(err)?;
        self.dirty = true;
        let p = self.project()?;
        Ok(row_json(p, &r, &[lang.to_string()]))
    }

    pub fn validate(&self, langs: &[String], kinds: &[String]) -> ToolResult {
        let p = self.project()?;
        let mut opts = ValidateOptions { langs: langs.to_vec(), ..Default::default() };
        if !kinds.is_empty() {
            opts.kinds = kinds.iter().map(|k| ValidationKind::from_keyword(k).ok_or_else(|| format!("unknown validation '{k}'"))).collect::<Result<_, _>>()?;
        }
        let mut issues = validate::validate(p, &opts, None);
        if let Some(shots) = self.shots.as_ref().filter(|_| opts.kinds.contains(&ValidationKind::Screenshot)) {
            issues.extend(shots.issues(p, None));
        }
        let excluded = validate::remove_excluded(p, &mut issues);
        Ok(json!({ "count": issues.len(), "excluded": excluded, "issues": issues }))
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

    pub fn export(&self, path: &Path, langs: &[String], statuses: &[String], screenshots: Option<&str>) -> ToolResult {
        let screenshots = match screenshots {
            Some(m) => ScreenshotExport::from_keyword(m).ok_or_else(|| format!("unknown screenshots mode '{m}' (auto|none|link|embed)"))?,
            None => ScreenshotExport::Auto,
        };
        let opts = ExportOptions { langs: langs.to_vec(), statuses: parse_statuses(statuses)?, screenshots, ..Default::default() };
        let files = exchange::export_file(self.project()?, path, &opts, self.shots.as_ref()).map_err(err)?;
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

    fn workflow(&self) -> Result<Workflow, String> {
        let path = self.project()?.path.clone().ok_or("project has no file name")?;
        Workflow::detect(&path).map_err(err)?.ok_or_else(|| susi_workflow::Error::NotConfigured(path.display().to_string()).to_string())
    }

    pub fn workflow_status(&self, online: bool) -> ToolResult {
        let wf = self.workflow()?;
        let s = wf.status(self.project()?, StatusOptions { fetch: online, online }).map_err(err)?;
        let mut v = serde_json::to_value(&s).map_err(err)?;
        v["hint"] = json!(s.hint());
        v["unsaved_project_changes"] = json!(self.dirty);
        Ok(v)
    }

    pub fn workflow_fetch(&mut self, i: &WorkflowFetchInput) -> ToolResult {
        if self.dirty && !i.discard_changes.unwrap_or(false) {
            return Err("the open project has unsaved changes – call save_project and workflow_save first, or pass discard_changes=true".into());
        }
        let wf = self.workflow()?;
        let r = wf.fetch(&FetchOptions { branch: i.branch.clone(), commit: i.commit.clone() }).map_err(err)?;
        let path = self.project()?.path.clone().unwrap_or_default();
        self.project = Some(slp::load(&path).map_err(err)?);
        self.dirty = false;
        Ok(json!({ "fetched": r, "git": wf.take_log() }))
    }

    pub fn workflow_save(&mut self, i: &WorkflowSaveInput) -> ToolResult {
        let wf = self.workflow()?;
        let mut build_errors = Vec::new();
        if i.build.unwrap_or(wf.config().save.build) {
            let p = self.project()?;
            build_errors = ops::build_project(p, &p.language_ids(), None).errors;
        }
        if self.dirty {
            self.save(None)?;
        }
        let mut opts = wf.save_defaults();
        if let Some(m) = &i.message {
            opts.message.clone_from(m);
        }
        if let Some(p) = &i.prefix {
            opts.prefix.clone_from(p);
        }
        opts.ci_skip = i.ci_skip.unwrap_or(opts.ci_skip);
        opts.branch.clone_from(&i.branch);
        let r = wf.save(self.project()?, &opts).map_err(err)?;
        Ok(json!({ "saved": r, "build_errors": build_errors, "git": wf.take_log() }))
    }

    pub fn workflow_publish(&self, i: &WorkflowPublishInput) -> ToolResult {
        let wf = self.workflow()?;
        let mut opts = wf.publish_defaults();
        if let Some(v) = &i.title {
            opts.title.clone_from(v);
        }
        if let Some(v) = &i.description {
            opts.description.clone_from(v);
        }
        if let Some(v) = &i.reviewers {
            opts.reviewers.clone_from(v);
        }
        if let Some(v) = &i.target {
            opts.target.clone_from(v);
        }
        let forge = wf.forge().map_err(err)?;
        let r = wf.publish(forge.as_ref(), &opts).map_err(err)?;
        Ok(json!({ "published": r }))
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
        let e = s.workflow_status(false).unwrap_err();
        assert!(e.contains("not in a git working copy"), "{e}");
    }
}
