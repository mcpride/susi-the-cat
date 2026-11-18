//! Screenshots as context for translators: images linked to rows, optionally with the area where
//! the text appears.
//!
//! They are stored next to the project and never in the `.slp`: Sisulizer drops unknown content when
//! it saves a project, and embedded images would bloat the file (Sisulizer stores pictures as hex
//! dumps, about 2.5 times their size). `<stem>.files/screenshots.json` lists the screenshots and
//! their links; the images live in `<stem>.files/screenshots/`, named `<id>.<hash8>.<ext>` so that a
//! changed image gets a new name and an unchanged one is never copied again. Rows are addressed like
//! in exchange files: source name + context (see [`crate::keys`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result, read_file, write_file};
use crate::keys::{RowKeys, source_index};
use crate::model::{Project, RowRef, now_stamp};
use crate::validate::{Issue, ValidationKind};

/// Suffix of the folder next to the project: `app.slp` → `app.files/`.
pub const DIR_SUFFIX: &str = ".files";
/// Index file inside the folder.
pub const INDEX_FILE: &str = "screenshots.json";
/// Subfolder of the images.
pub const IMAGE_DIR: &str = "screenshots";
const VERSION: u32 = 1;

/// The folder holding the files that belong to project `slp` (screenshots, later binary files).
pub fn files_dir(slp: &Path) -> PathBuf {
    let stem = slp.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    slp.with_file_name(format!("{stem}{DIR_SUFFIX}"))
}

/// Rectangle in image pixels, written as `[x, y, w, h]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "[u32; 4]", into = "[u32; 4]")]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl From<[u32; 4]> for Area {
    fn from([x, y, w, h]: [u32; 4]) -> Self {
        Self { x, y, w, h }
    }
}

impl From<Area> for [u32; 4] {
    fn from(a: Area) -> Self {
        [a.x, a.y, a.w, a.h]
    }
}

impl Area {
    /// Parses `x,y,w,h` (also separated by spaces or `;`).
    pub fn parse(s: &str) -> Option<Self> {
        let v: Vec<u32> = s.split([',', ';', ' ']).filter(|p| !p.is_empty()).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
        let a: [u32; 4] = v.try_into().ok()?;
        let a = Area::from(a);
        (a.w > 0 && a.h > 0).then_some(a)
    }

    /// The area clipped to an image of `width` × `height`; `None` if nothing remains.
    pub fn clip(self, width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            return Some(self);
        }
        let (x, y) = (self.x.min(width), self.y.min(height));
        let (w, h) = (self.w.min(width - x), self.h.min(height - y));
        (w > 0 && h > 0).then_some(Self { x, y, w, h })
    }
}

impl std::fmt::Display for Area {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{},{},{},{}", self.x, self.y, self.w, self.h)
    }
}

/// Link of a screenshot to rows: source name (optional) + context or row id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Area>,
}

impl Link {
    /// Link to row `r` (source name + context).
    pub fn to_row(p: &Project, r: &RowRef, area: Option<Area>) -> Self {
        Self { source: p.sources.get(r.source).map(|s| s.name().to_string()), key: p.context(r), area }
    }

    fn same_target(&self, other: &Link) -> bool {
        let src = |l: &Link| l.source.as_deref().map(|s| s.replace('/', "\\").to_lowercase());
        self.key == other.key && src(self) == src(other)
    }

    /// Rows this link refers to (see [`RowKeys::select`]: wildcards allowed, an ambiguous row id
    /// resolves to none).
    fn resolve(&self, p: &Project, keys: &RowKeys) -> Vec<RowRef> {
        let source = match &self.source {
            Some(name) => match source_index(p, name) {
                Some(i) => Some(i),
                None => return Vec::new(),
            },
            None => None,
        };
        keys.select(&self.key, source)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screenshot {
    pub id: String,
    /// Image file name in the `screenshots/` folder.
    pub file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// `yyyymmddhhnnss`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    /// File the image was taken from (relative to the project folder, `/` separated), for refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// SHA-256 of the origin file when it was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_hash: Option<String>,
    #[serde(default)]
    pub links: Vec<Link>,
}

impl Screenshot {
    /// Title, or the id if there is none.
    pub fn label(&self) -> &str {
        if self.title.is_empty() { &self.id } else { &self.title }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    #[serde(default)]
    screenshots: Vec<Screenshot>,
}

/// Screenshot list of a UI test run: the schema of `screenshots.json`, with `file` relative to
/// the manifest and every field except `file` optional.
#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    screenshots: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    #[serde(default)]
    id: Option<String>,
    file: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PruneReport {
    /// Orphaned links removed.
    pub links: usize,
    /// Unused image files deleted on the next save.
    pub files: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RefreshReport {
    /// Screenshots whose origin file changed and was read again.
    pub updated: Vec<String>,
    pub unchanged: usize,
    /// Screenshots whose origin file no longer exists.
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ManifestReport {
    pub added: usize,
    pub replaced: usize,
    /// Screenshots missing in the manifest, removed with `prune`.
    pub removed: usize,
    pub links: usize,
}

/// File extension of a supported image (PNG or JPEG), detected from its signature.
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

/// Width and height in pixels of a PNG or JPEG image (read from its header, not decoded).
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image_kind(bytes)?;
    let img = rust_xlsxwriter::Image::new_from_buffer(bytes).ok()?;
    Some((img.width().round() as u32, img.height().round() as u32))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Readable id from a name: lowercase ASCII letters and digits, other characters become `-`.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            s.push(c.to_ascii_lowercase());
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-');
    if s.is_empty() { "screenshot".into() } else { s.into() }
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// Screenshots of one project.
#[derive(Debug, Clone)]
pub struct ScreenshotStore {
    dir: PathBuf,
    /// Project folder; origins are relative to it.
    base: PathBuf,
    shots: Vec<Screenshot>,
    dirty: bool,
    /// Image files replaced, removed or found unused since the last save; deleted by [`Self::save`]
    /// unless referenced again.
    obsolete: Vec<String>,
}

impl ScreenshotStore {
    /// Empty store for project `slp`.
    pub fn new(slp: &Path) -> Self {
        let base = slp.parent().map(Path::to_path_buf).unwrap_or_default();
        Self { dir: files_dir(slp), base, shots: Vec::new(), dirty: false, obsolete: Vec::new() }
    }

    /// Loads the screenshots of project `slp`; empty if there are none yet.
    pub fn open(slp: &Path) -> Result<Self> {
        let mut s = Self::new(slp);
        let index = s.index_path();
        if index.is_file() {
            let f: IndexFile = serde_json::from_slice(strip_bom(&read_file(&index)?)).map_err(|e| Error::Json(format!("{}: {e}", index.display())))?;
            if f.version > VERSION {
                return Err(Error::Unsupported(crate::tf!("{path}: screenshot index version {version}", path = index.display(), version = f.version)));
            }
            s.shots = f.screenshots;
            s.shots.sort_by(|a, b| a.id.cmp(&b.id));
        }
        Ok(s)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn index_path(&self) -> PathBuf {
        self.dir.join(INDEX_FILE)
    }

    pub fn image_dir(&self) -> PathBuf {
        self.dir.join(IMAGE_DIR)
    }

    /// All screenshots, sorted by id.
    pub fn screenshots(&self) -> &[Screenshot] {
        &self.shots
    }

    pub fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.shots.len()
    }

    /// Changed since loading or the last save.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn pos(&self, id: &str) -> Option<usize> {
        self.shots.iter().position(|s| s.id == id).or_else(|| {
            let id = slug(id);
            self.shots.iter().position(|s| s.id == id)
        })
    }

    pub fn get(&self, id: &str) -> Option<&Screenshot> {
        self.pos(id).map(|i| &self.shots[i])
    }

    /// Path of the image of screenshot `id`.
    pub fn image_path(&self, id: &str) -> Option<PathBuf> {
        self.get(id).map(|s| self.image_dir().join(&s.file))
    }

    /// Screenshots whose image file is missing.
    pub fn missing_files(&self) -> Vec<&Screenshot> {
        let dir = self.image_dir();
        self.shots.iter().filter(|s| !dir.join(&s.file).is_file()).collect()
    }

    /// Writes `screenshots.json` if anything changed and deletes image files no longer used.
    /// Without screenshots and without an index file nothing is created.
    pub fn save(&mut self) -> Result<()> {
        let index = self.index_path();
        if self.dirty && (!self.shots.is_empty() || index.exists()) {
            let f = IndexFile { version: VERSION, screenshots: self.shots.clone() };
            let mut json = serde_json::to_string_pretty(&f).map_err(|e| Error::Json(e.to_string()))?;
            json.push('\n');
            write_file(&index, json.as_bytes())?;
        }
        let used: HashSet<&str> = self.shots.iter().map(|s| s.file.as_str()).collect();
        for f in std::mem::take(&mut self.obsolete) {
            if !used.contains(f.as_str()) {
                let _ = std::fs::remove_file(self.image_dir().join(&f));
            }
        }
        self.dirty = false;
        Ok(())
    }

    /// Moves the store to project `slp` (Save As): copies the images to the new folder and rebases
    /// the origins. The index is written by the next [`Self::save`].
    pub fn relocate(&mut self, slp: &Path) -> Result<()> {
        let dir = files_dir(slp);
        if crate::ops::normalize(&dir) == crate::ops::normalize(&self.dir) {
            return Ok(());
        }
        let (old_images, new_images) = (self.image_dir(), dir.join(IMAGE_DIR));
        for s in &self.shots {
            let from = old_images.join(&s.file);
            if from.is_file() {
                write_file(&new_images.join(&s.file), &read_file(&from)?)?;
            }
        }
        let new_base = slp.parent().map(Path::to_path_buf).unwrap_or_default();
        for s in &mut self.shots {
            if let Some(o) = &s.origin {
                let abs = absolute(&self.base.join(o));
                s.origin = Some(origin_string(&new_base, &abs));
            }
        }
        self.dir = dir;
        self.base = new_base;
        self.obsolete.clear();
        self.dirty = true;
        Ok(())
    }

    /// Adds the image file `path`; `id` defaults to a unique id from the file name. An existing
    /// `id` gets the new image and keeps its links. The file is remembered as origin for
    /// [`Self::refresh`]. Returns the id.
    pub fn add_file(&mut self, path: &Path, id: Option<&str>, title: Option<&str>) -> Result<String> {
        let bytes = read_file(path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let origin = (origin_string(&self.base, &absolute(path)), sha256_hex(&bytes));
        self.put(&bytes, id, &name, title, Some(origin)).map_err(|e| match e {
            Error::Unsupported(_) => Error::Unsupported(crate::tf!("image format (PNG or JPEG expected): {path}", path = path.display())),
            e => e,
        })
    }

    /// Adds an image from memory (e.g. pasted from the clipboard). Returns the id.
    pub fn add_bytes(&mut self, bytes: &[u8], id: Option<&str>, title: Option<&str>) -> Result<String> {
        self.put(bytes, id, "screenshot", title, None)
    }

    fn put(&mut self, bytes: &[u8], id: Option<&str>, name: &str, title: Option<&str>, origin: Option<(String, String)>) -> Result<String> {
        let (Some(ext), Some((width, height))) = (image_kind(bytes), image_size(bytes)) else {
            return Err(Error::Unsupported(crate::i18n::t("image format (PNG or JPEG expected)").into()));
        };
        let hash = sha256_hex(bytes);
        let id = match id.filter(|i| !i.trim().is_empty()) {
            Some(i) => self.get(i).map_or_else(|| slug(i), |s| s.id.clone()),
            None => self.unique_id(&slug(name)),
        };
        let file = format!("{id}.{}.{ext}", &hash[..8]);
        let path = self.image_dir().join(&file);
        if !path.is_file() {
            write_file(&path, bytes)?;
        }
        let (origin, origin_hash) = origin.map_or((None, None), |(o, h)| (Some(o), Some(h)));
        match self.pos(&id) {
            Some(i) => {
                let s = &mut self.shots[i];
                let before = s.clone();
                if s.file != file {
                    self.obsolete.push(std::mem::replace(&mut s.file, file));
                }
                (s.width, s.height) = (width, height);
                if let Some(t) = title {
                    s.title = t.to_string();
                }
                if origin.is_some() {
                    (s.origin, s.origin_hash) = (origin, origin_hash);
                }
                self.dirty |= *s != before;
            }
            None => {
                let title = title.unwrap_or_default().to_string();
                let s = Screenshot { id: id.clone(), file, title, width, height, created: now_stamp(), origin, origin_hash, links: Vec::new() };
                let at = self.shots.partition_point(|x| x.id < id);
                self.shots.insert(at, s);
                self.dirty = true;
            }
        }
        Ok(id)
    }

    fn unique_id(&self, base: &str) -> String {
        let mut id = base.to_string();
        let mut n = 2;
        while self.pos(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }

    /// Removes screenshot `id` and its links; the image file is deleted by the next save.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(i) = self.pos(id) else {
            return false;
        };
        let s = self.shots.remove(i);
        self.obsolete.push(s.file);
        self.dirty = true;
        true
    }

    pub fn set_title(&mut self, id: &str, title: &str) -> bool {
        let Some(i) = self.pos(id) else {
            return false;
        };
        if self.shots[i].title != title {
            self.shots[i].title = title.to_string();
            self.dirty = true;
        }
        true
    }

    /// Adds links to screenshot `id`; a link to a target that is already linked updates its area
    /// (if given). Returns the number of new links.
    pub fn link(&mut self, id: &str, links: impl IntoIterator<Item = Link>) -> Result<usize> {
        let i = self.pos(id).ok_or_else(|| Error::NotFound(id.to_string()))?;
        let shot = &mut self.shots[i];
        let mut added = 0;
        for l in links {
            match shot.links.iter_mut().find(|x| x.same_target(&l)) {
                Some(x) => {
                    if l.area.is_some() && x.area != l.area {
                        x.area = l.area;
                        self.dirty = true;
                    }
                }
                None => {
                    shot.links.push(l);
                    added += 1;
                    self.dirty = true;
                }
            }
        }
        Ok(added)
    }

    /// Removes the links of screenshot `id` that refer to row `r`. Returns how many were removed.
    pub fn unlink_row(&mut self, p: &Project, id: &str, r: &RowRef) -> usize {
        let Some(i) = self.pos(id) else {
            return 0;
        };
        let keys = RowKeys::new(p);
        let before = self.shots[i].links.len();
        self.shots[i].links.retain(|l| !l.resolve(p, &keys).contains(r));
        let n = before - self.shots[i].links.len();
        self.dirty |= n > 0;
        n
    }

    /// Removes the links of screenshot `id` with the target of `link` (source + key).
    pub fn unlink(&mut self, id: &str, link: &Link) -> usize {
        let Some(i) = self.pos(id) else {
            return 0;
        };
        let before = self.shots[i].links.len();
        self.shots[i].links.retain(|l| !l.same_target(link));
        let n = before - self.shots[i].links.len();
        self.dirty |= n > 0;
        n
    }

    /// Sets the area of the link of screenshot `id` to row `r`, linking the row if necessary.
    pub fn set_area(&mut self, p: &Project, id: &str, r: &RowRef, area: Option<Area>) -> Result<()> {
        let i = self.pos(id).ok_or_else(|| Error::NotFound(id.to_string()))?;
        let keys = RowKeys::new(p);
        match self.shots[i].links.iter_mut().find(|l| l.resolve(p, &keys).contains(r)) {
            Some(l) if l.area == area => {}
            Some(l) => {
                l.area = area;
                self.dirty = true;
            }
            None => {
                self.shots[i].links.push(Link::to_row(p, r, area));
                self.dirty = true;
            }
        }
        Ok(())
    }

    /// Resolves the links against a project state.
    pub fn index(&self, p: &Project) -> ScreenshotIndex {
        ScreenshotIndex::new(p, self)
    }

    /// Screenshots linked to row `r` with the link (and its area).
    pub fn links_of<'a>(&'a self, idx: &ScreenshotIndex, r: &RowRef) -> Vec<(&'a Screenshot, &'a Link)> {
        idx.of_row(r).iter().filter_map(|&(s, l)| Some((self.shots.get(s)?, self.shots.get(s)?.links.get(l)?))).collect()
    }

    /// Links whose rows no longer exist.
    pub fn orphans<'a>(&'a self, idx: &ScreenshotIndex) -> Vec<(&'a Screenshot, &'a Link)> {
        idx.orphans.iter().filter_map(|&(s, l)| Some((self.shots.get(s)?, self.shots.get(s)?.links.get(l)?))).collect()
    }

    /// Removes orphaned links and schedules unused image files for deletion (next save).
    pub fn prune(&mut self, p: &Project) -> PruneReport {
        let idx = self.index(p);
        let orphans: HashSet<(usize, usize)> = idx.orphans.iter().copied().collect();
        let mut rep = PruneReport::default();
        for (si, s) in self.shots.iter_mut().enumerate() {
            let mut li = 0;
            s.links.retain(|_| {
                let keep = !orphans.contains(&(si, li));
                li += 1;
                keep
            });
        }
        rep.links = orphans.len();
        let used: HashSet<&str> = self.shots.iter().map(|s| s.file.as_str()).collect();
        let unused: Vec<String> = std::fs::read_dir(self.image_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|f| !used.contains(f.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        rep.files = unused.len();
        self.obsolete.extend(unused);
        self.dirty |= rep.links > 0 || rep.files > 0;
        rep
    }

    /// Reads origin files again whose content changed (all screenshots or only `id`); the links stay.
    pub fn refresh(&mut self, id: Option<&str>) -> Result<RefreshReport> {
        let mut rep = RefreshReport::default();
        let todo: Vec<(String, String, Option<String>)> = self
            .shots
            .iter()
            .filter(|s| id.is_none_or(|i| self.get(i).is_some_and(|x| x.id == s.id)))
            .filter_map(|s| Some((s.id.clone(), s.origin.clone()?, s.origin_hash.clone())))
            .collect();
        for (sid, origin, old) in todo {
            let path = self.base.join(&origin);
            if !path.is_file() {
                rep.missing.push(sid);
                continue;
            }
            let bytes = read_file(&path)?;
            let hash = sha256_hex(&bytes);
            if old.as_deref() == Some(hash.as_str()) {
                rep.unchanged += 1;
                continue;
            }
            self.put(&bytes, Some(&sid), &sid, None, Some((origin, hash)))?;
            rep.updated.push(sid);
        }
        Ok(rep)
    }

    /// Imports a manifest of a UI test run: every listed screenshot is added or replaced (image,
    /// title and links), so importing again is idempotent. With `prune`, screenshots missing in the
    /// manifest are removed. All images are read before anything changes.
    pub fn import_manifest(&mut self, path: &Path, prune: bool) -> Result<ManifestReport> {
        let m: Manifest = serde_json::from_slice(strip_bom(&read_file(path)?)).map_err(|e| Error::Json(format!("{}: {e}", path.display())))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut entries = Vec::new();
        for e in m.screenshots {
            let file = dir.join(&e.file);
            let bytes = read_file(&file)?;
            if image_size(&bytes).is_none() {
                return Err(Error::Unsupported(crate::tf!("image format (PNG or JPEG expected): {path}", path = file.display())));
            }
            let id = slug(e.id.as_deref().unwrap_or_else(|| Path::new(&e.file).file_stem().and_then(|s| s.to_str()).unwrap_or("screenshot")));
            entries.push((id, file, bytes, e.title, e.links));
        }
        let mut rep = ManifestReport::default();
        let mut seen = HashSet::new();
        for (id, file, bytes, title, links) in entries {
            if self.pos(&id).is_some() {
                rep.replaced += 1
            } else {
                rep.added += 1
            }
            let origin = (origin_string(&self.base, &absolute(&file)), sha256_hex(&bytes));
            let id = self.put(&bytes, Some(&id), &id, Some(title.as_deref().unwrap_or_default()), Some(origin))?;
            if let Some(i) = self.pos(&id) {
                let mut unique: Vec<Link> = Vec::new();
                for l in links {
                    match unique.iter_mut().find(|x| x.same_target(&l)) {
                        Some(x) => x.area = l.area.or(x.area),
                        None => unique.push(l),
                    }
                }
                rep.links += unique.len();
                if self.shots[i].links != unique {
                    self.shots[i].links = unique;
                    self.dirty = true;
                }
            }
            seen.insert(id);
        }
        if prune {
            let gone: Vec<String> = self.shots.iter().filter(|s| !seen.contains(&s.id)).map(|s| s.id.clone()).collect();
            rep.removed = gone.len();
            for id in gone {
                self.remove(&id);
            }
        }
        Ok(rep)
    }

    /// Validation issues: linked rows whose screenshot image is missing.
    pub fn issues(&self, p: &Project, rows: Option<&[RowRef]>) -> Vec<Issue> {
        let missing: HashSet<&str> = self.missing_files().into_iter().map(|s| s.id.as_str()).collect();
        if missing.is_empty() {
            return Vec::new();
        }
        let idx = self.index(p);
        let wanted: Option<HashSet<&RowRef>> = rows.map(|r| r.iter().collect());
        let mut refs: Vec<&RowRef> = idx.by_row.keys().filter(|r| wanted.as_ref().is_none_or(|w| w.contains(r))).collect();
        refs.sort();
        let mut issues = Vec::new();
        for r in refs {
            for (s, _) in self.links_of(&idx, r) {
                if missing.contains(s.id.as_str()) {
                    issues.push(Issue {
                        row: r.clone(),
                        source: p.sources.get(r.source).map(|x| x.name().to_string()).unwrap_or_default(),
                        context: p.context(r),
                        lang: p.original_lang(r).to_string(),
                        kind: ValidationKind::Screenshot,
                        severity: ValidationKind::Screenshot.severity(),
                        message: crate::tf!("Screenshot \"{title}\" is missing: {file}", title = s.label(), file = s.file),
                    });
                }
            }
        }
        issues
    }
}

fn absolute(p: &Path) -> PathBuf {
    let p = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map(|d| d.join(p)).unwrap_or_else(|_| p.to_path_buf()) };
    crate::ops::normalize(&p)
}

/// `target` relative to `base` (or absolute on another drive), `/` separated.
fn origin_string(base: &Path, target: &Path) -> String {
    let rel = crate::ops::relative_path(&absolute(base), target).unwrap_or_else(|| target.to_path_buf());
    rel.to_string_lossy().replace('\\', "/")
}

/// Links of a store resolved against a project state. Holds positions into the store: rebuild it
/// after changing the store or the project structure.
#[derive(Debug, Clone, Default)]
pub struct ScreenshotIndex {
    /// Row → (screenshot position, link position).
    by_row: HashMap<RowRef, Vec<(usize, usize)>>,
    orphans: Vec<(usize, usize)>,
}

impl ScreenshotIndex {
    pub fn new(p: &Project, store: &ScreenshotStore) -> Self {
        let keys = RowKeys::new(p);
        let mut idx = Self::default();
        for (si, s) in store.shots.iter().enumerate() {
            for (li, l) in s.links.iter().enumerate() {
                let rows = l.resolve(p, &keys);
                if rows.is_empty() {
                    idx.orphans.push((si, li));
                }
                for r in rows {
                    idx.by_row.entry(r).or_default().push((si, li));
                }
            }
        }
        idx
    }

    /// (screenshot position, link position) of the links of row `r`.
    pub fn of_row(&self, r: &RowRef) -> &[(usize, usize)] {
        self.by_row.get(r).map_or(&[], Vec::as_slice)
    }

    pub fn has(&self, r: &RowRef) -> bool {
        self.by_row.contains_key(r)
    }

    /// Rows with at least one screenshot.
    pub fn rows(&self) -> HashSet<RowRef> {
        self.by_row.keys().cloned().collect()
    }

    pub fn orphan_count(&self) -> usize {
        self.orphans.len()
    }
}

/// Encodes a single-colour RGB PNG (uncompressed). For placeholders and tests.
#[doc(hidden)]
pub fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    // every scanline: filter type 0, then the pixels
    let line: Vec<u8> = std::iter::once(0).chain(rgb.iter().copied().cycle().take(width as usize * 3)).collect();
    let raw = line.repeat(height as usize);
    // zlib stream with stored (uncompressed) deflate blocks
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if raw.is_empty() { vec![&[]] } else { raw.chunks(65_535).collect() };
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = u16::try_from(b.len()).unwrap_or(u16::MAX);
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Item, Node, Row, Source, SourceKind};

    fn project(dir: &Path) -> Project {
        let mut p = Project::new();
        p.path = Some(dir.join("app.slp"));
        p.add_language("de");
        let mut s = Source::new(SourceKind::Json, "src\\en.json", "en");
        let mut n = Node::new("TJsonNode", "LOGIN");
        n.items.push(Item::Row(Row::new("TITLE", "Login")));
        n.items.push(Item::Row(Row::new("USER", "User name")));
        s.items.push(Item::Node(n));
        p.sources.push(s);
        p
    }

    fn row(p: &Project, ctx: &str) -> RowRef {
        p.find_row(ctx).unwrap()
    }

    #[test]
    fn png_header_is_readable() {
        let png = solid_png(30, 20, [255, 0, 0]);
        assert_eq!((image_kind(&png), image_size(&png)), (Some("png"), Some((30, 20))));
        assert_eq!(image_kind(b"GIF89a"), None);
        assert_eq!(Area::parse("1, 2,3;4"), Some(Area { x: 1, y: 2, w: 3, h: 4 }));
        assert_eq!(Area::parse("1,2,0,4"), None);
        assert_eq!(Area { x: 25, y: 5, w: 10, h: 30 }.clip(30, 20), Some(Area { x: 25, y: 5, w: 5, h: 15 }));
        assert_eq!(slug("Login Dialog (de).png"), "login-dialog-de-png");
    }

    #[test]
    fn add_link_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let slp = p.path.clone().unwrap();
        let img = dir.path().join("shots").join("Login Dialog.png");
        write_file(&img, &solid_png(40, 30, [0, 0, 255])).unwrap();

        let mut st = ScreenshotStore::open(&slp).unwrap();
        assert!(st.is_empty());
        let id = st.add_file(&img, None, Some("Login")).unwrap();
        assert_eq!(id, "login-dialog");
        let title = row(&p, "LOGIN/TITLE");
        st.link(&id, [Link::to_row(&p, &title, Area::parse("1,2,10,5"))]).unwrap();
        // linking again only updates the area
        assert_eq!(st.link(&id, [Link::to_row(&p, &title, Area::parse("2,2,10,5"))]).unwrap(), 0);
        st.save().unwrap();
        assert!(!st.is_dirty());

        let st = ScreenshotStore::open(&slp).unwrap();
        let s = st.get("login-dialog").unwrap();
        assert_eq!((s.width, s.height, s.title.as_str(), s.origin.as_deref()), (40, 30, "Login", Some("shots/Login Dialog.png")));
        assert!(s.file.starts_with("login-dialog.") && st.image_path(&id).unwrap().is_file());
        let idx = st.index(&p);
        let links = st.links_of(&idx, &title);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].1.area, Area::parse("2,2,10,5"));
        assert!(!idx.has(&row(&p, "LOGIN/USER")));
        let json = std::fs::read_to_string(st.index_path()).unwrap();
        assert!(json.contains("\"key\": \"LOGIN/TITLE\"") && json.contains("\"area\": [\n"), "{json}");
    }

    #[test]
    fn same_image_is_not_copied_twice_changed_image_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let mut st = ScreenshotStore::new(p.path.as_ref().unwrap());
        let a = st.add_bytes(&solid_png(4, 4, [1, 2, 3]), Some("x"), None).unwrap();
        let first = st.get(&a).unwrap().file.clone();
        st.add_bytes(&solid_png(4, 4, [1, 2, 3]), Some("x"), None).unwrap();
        assert_eq!(st.get(&a).unwrap().file, first);
        // an id derived from a name never replaces an existing screenshot
        assert_eq!(st.add_bytes(&solid_png(4, 4, [1, 2, 3]), None, None).unwrap(), "screenshot");
        assert_eq!(st.add_bytes(&solid_png(4, 4, [1, 2, 3]), None, None).unwrap(), "screenshot-2");
        st.link("x", [Link::to_row(&p, &row(&p, "LOGIN/USER"), None)]).unwrap();
        st.save().unwrap();
        st.add_bytes(&solid_png(8, 4, [9, 9, 9]), Some("x"), None).unwrap();
        let second = st.get("x").unwrap().file.clone();
        assert_ne!(first, second);
        assert_eq!(st.get("x").unwrap().links.len(), 1, "links survive a new image");
        assert!(st.image_dir().join(&first).is_file(), "old file kept until saved");
        st.save().unwrap();
        assert!(!st.image_dir().join(&first).exists());
        assert!(st.image_dir().join(&second).is_file());
        assert!(matches!(st.add_bytes(b"GIF89a....", None, None), Err(Error::Unsupported(_))));
    }

    #[test]
    fn orphans_prune_and_unlink() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project(dir.path());
        let mut st = ScreenshotStore::new(p.path.as_ref().unwrap());
        let id = st.add_bytes(&solid_png(4, 4, [0, 0, 0]), Some("s"), None).unwrap();
        let (title, user) = (row(&p, "LOGIN/TITLE"), row(&p, "LOGIN/USER"));
        st.link(&id, [Link::to_row(&p, &title, None), Link::to_row(&p, &user, None)]).unwrap();
        st.link(&id, [Link { source: None, key: "TITLE".into(), area: None }]).unwrap();
        assert_eq!(st.index(&p).rows().len(), 2);
        assert_eq!(st.index(&p).of_row(&title).len(), 2, "context link and row id link");
        write_file(&st.image_dir().join("stray.png"), b"x").unwrap();

        // the row USER disappears (e.g. "Remove unused")
        if let Item::Node(n) = &mut p.sources[0].items[0] {
            n.items.remove(1);
        }
        let idx = st.index(&p);
        assert_eq!(st.orphans(&idx).len(), 1);
        assert_eq!(st.prune(&p), PruneReport { links: 1, files: 1 });
        st.save().unwrap();
        assert!(!st.image_dir().join("stray.png").exists());
        assert_eq!(st.get(&id).unwrap().links.len(), 2);

        assert_eq!(st.unlink_row(&p, &id, &title), 2);
        st.set_area(&p, &id, &title, Area::parse("0,0,2,2")).unwrap();
        assert_eq!(st.get(&id).unwrap().links, [Link { source: Some("src\\en.json".into()), key: "LOGIN/TITLE".into(), area: Area::parse("0,0,2,2") }]);
        assert_eq!(st.unlink(&id, &Link { source: Some("SRC/EN.JSON".into()), key: "LOGIN/TITLE".into(), area: None }), 1);
    }

    #[test]
    fn manifest_import_is_idempotent_and_refresh_reads_changed_origins() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let slp = p.path.clone().unwrap();
        let out = dir.path().join("ui-tests").join("out");
        write_file(&out.join("login.png"), &solid_png(20, 10, [1, 1, 1])).unwrap();
        write_file(&out.join("old.png"), &solid_png(2, 2, [1, 1, 1])).unwrap();
        let manifest = out.join("manifest.json");
        let json = r#"{ "screenshots": [
            { "id": "login", "file": "login.png", "title": "Login dialog",
              "links": [ { "key": "LOGIN/TITLE", "area": [1, 1, 5, 5] }, { "source": "src/en.json", "key": "USER" } ] } ] }"#;
        write_file(&manifest, json.as_bytes()).unwrap();
        let mut st = ScreenshotStore::open(&slp).unwrap();
        st.add_file(&out.join("old.png"), Some("old"), None).unwrap();

        let rep = st.import_manifest(&manifest, true).unwrap();
        assert_eq!(rep, ManifestReport { added: 1, replaced: 0, removed: 1, links: 2 });
        st.save().unwrap();
        assert_eq!(st.import_manifest(&manifest, true).unwrap(), ManifestReport { added: 0, replaced: 1, removed: 0, links: 2 });
        assert!(!st.is_dirty(), "importing the same manifest again changes nothing");
        let idx = st.index(&p);
        assert_eq!(idx.rows().len(), 2);
        assert_eq!(st.get("login").unwrap().origin.as_deref(), Some("ui-tests/out/login.png"));

        assert_eq!(st.refresh(None).unwrap(), RefreshReport { updated: vec![], unchanged: 1, missing: vec![] });
        write_file(&out.join("login.png"), &solid_png(30, 10, [2, 2, 2])).unwrap();
        assert_eq!(st.refresh(Some("login")).unwrap().updated, ["login"]);
        assert_eq!(st.get("login").unwrap().width, 30);
        assert_eq!(st.get("login").unwrap().links.len(), 2);
        std::fs::remove_file(out.join("login.png")).unwrap();
        assert_eq!(st.refresh(None).unwrap().missing, ["login"]);
    }

    #[test]
    fn save_as_copies_images_and_rebases_origins() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let img = dir.path().join("a.png");
        write_file(&img, &solid_png(3, 3, [5, 5, 5])).unwrap();
        let mut st = ScreenshotStore::new(p.path.as_ref().unwrap());
        st.add_file(&img, None, None).unwrap();
        st.save().unwrap();
        let new_slp = dir.path().join("copy").join("app2.slp");
        st.relocate(&new_slp).unwrap();
        st.save().unwrap();
        assert_eq!(st.dir(), dir.path().join("copy").join("app2.files"));
        let again = ScreenshotStore::open(&new_slp).unwrap();
        assert!(again.image_path("a").unwrap().is_file());
        assert_eq!(again.get("a").unwrap().origin.as_deref(), Some("../a.png"));
        assert!(ScreenshotStore::open(p.path.as_ref().unwrap()).unwrap().image_path("a").unwrap().is_file(), "original stays");
    }

    #[test]
    fn missing_image_is_a_validation_issue() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path());
        let mut st = ScreenshotStore::new(p.path.as_ref().unwrap());
        st.add_bytes(&solid_png(3, 3, [5, 5, 5]), Some("gone"), None).unwrap();
        st.link("gone", [Link::to_row(&p, &row(&p, "LOGIN/TITLE"), None)]).unwrap();
        assert!(st.issues(&p, None).is_empty());
        std::fs::remove_file(st.image_path("gone").unwrap()).unwrap();
        let issues = st.issues(&p, None);
        assert_eq!(issues.len(), 1);
        assert_eq!((issues[0].context.as_str(), issues[0].kind), ("LOGIN/TITLE", ValidationKind::Screenshot));
        assert!(st.issues(&p, Some(&[row(&p, "LOGIN/USER")])).is_empty());
        // without screenshots no folder is created
        let mut empty = ScreenshotStore::new(&dir.path().join("other.slp"));
        empty.save().unwrap();
        assert!(!dir.path().join("other.files").exists());
    }
}
