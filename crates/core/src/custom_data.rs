//! Custom data (Sisulizer): pictures and binary data stored in rows.
//!
//! A picture row (`TPictureRow`) keeps the bytes of the original and of its translations as
//! upper-case hex; a binary row (`TBinaryRow`) only keeps a `thumbnail` of the original (first 8
//! and last 3 bytes) and its translations as hex. A translation can be linked to a file
//! (`filename` relative to the project, `filedate` as DOS time); a changed file is read again
//! (Sisulizer: *Row | Link to File*). In RESX sources every picture is a `TImageDataNode` and all
//! other binary data a `TBinaryDataNode`, each with one row named like the resource.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Result, read_file};
use crate::i18n::t;
use crate::model::{Item, Node, Project, Row, RowRef};
use crate::status::{RowStatus, TranslationStatus};

pub const PICTURE_ROW: &str = "TPictureRow";
pub const BINARY_ROW: &str = "TBinaryRow";
pub const PICTURE_NODE: &str = "TImageDataNode";
pub const BINARY_NODE: &str = "TBinaryDataNode";
/// Node classes of .NET custom data. XML and HTML data are kept when present but never created.
pub const DATA_NODES: [&str; 4] = [PICTURE_NODE, BINARY_NODE, "TXmlDataNode", "THtmlDataNode"];
/// Hex lines end in this column, whatever their indentation (Sisulizer).
const LINE_END: usize = 78;

/// Whether a node holds custom data (and is not a container of a file's rows).
pub fn is_data_node(n: &Node) -> bool {
    n.attrs.get("class").is_some_and(|c| DATA_NODES.contains(&c))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Picture,
    Binary,
}

impl DataKind {
    pub fn of_row_class(class: &str) -> Option<Self> {
        match class {
            PICTURE_ROW => Some(Self::Picture),
            BINARY_ROW => Some(Self::Binary),
            _ => None,
        }
    }

    /// Pictures are recognized by their signature, everything else is binary data.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        if PictureFormat::of(bytes).is_some() { Self::Picture } else { Self::Binary }
    }

    pub fn row_class(self) -> &'static str {
        match self {
            Self::Picture => PICTURE_ROW,
            Self::Binary => BINARY_ROW,
        }
    }

    pub fn node_class(self) -> &'static str {
        match self {
            Self::Picture => PICTURE_NODE,
            Self::Binary => BINARY_NODE,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Picture => "picture",
            Self::Binary => "binary",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Picture => t("Picture"),
            Self::Binary => t("Binary data"),
        }
    }
}

/// Picture formats Sisulizer localizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureFormat {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Ico,
    Cur,
}

impl PictureFormat {
    pub fn of(b: &[u8]) -> Option<Self> {
        if b.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if b.starts_with(b"BM") && b.len() >= 26 && matches!(u32_le(b, 14), Some(12 | 40 | 52 | 56 | 64 | 108 | 124)) {
            Some(Self::Bmp)
        } else if b.len() >= 22 && b[..2] == [0, 0] && b[3] == 0 && b[4..6] != [0, 0] {
            match b[2] {
                1 => Some(Self::Ico),
                2 => Some(Self::Cur),
                _ => None,
            }
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Cur => "CUR",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Ico => "ico",
            Self::Cur => "cur",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Ico | Self::Cur => "image/x-icon",
        }
    }
}

/// Format and size of a picture, read from its header (0 if the size is not found).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureInfo {
    pub format: PictureFormat,
    pub width: u32,
    pub height: u32,
}

impl PictureInfo {
    pub fn of(b: &[u8]) -> Option<Self> {
        let format = PictureFormat::of(b)?;
        let (width, height) = match format {
            PictureFormat::Png => (u32_be(b, 16), u32_be(b, 20)),
            PictureFormat::Gif => (u16_le(b, 6).map(u32::from), u16_le(b, 8).map(u32::from)),
            PictureFormat::Bmp => {
                if u32_le(b, 14) == Some(12) {
                    (u16_le(b, 18).map(u32::from), u16_le(b, 20).map(u32::from))
                } else {
                    (u32_le(b, 18).map(|w| (w as i32).unsigned_abs()), u32_le(b, 22).map(|h| (h as i32).unsigned_abs()))
                }
            }
            PictureFormat::Ico | PictureFormat::Cur => {
                let dim = |i: usize| b.get(i).map(|&v| if v == 0 { 256 } else { u32::from(v) });
                (dim(6), dim(7))
            }
            PictureFormat::Jpeg => jpeg_size(b).map_or((None, None), |(w, h)| (Some(w), Some(h))),
        };
        Some(Self { format, width: width.unwrap_or(0), height: height.unwrap_or(0) })
    }

    /// `PNG 16 × 8`.
    pub fn summary(&self) -> String {
        if self.width == 0 || self.height == 0 { self.format.name().to_string() } else { format!("{} {} × {}", self.format.name(), self.width, self.height) }
    }
}

fn u16_le(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_le(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn u32_be(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Size from the first start-of-frame marker.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        match marker {
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]);
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]);
                return Some((u32::from(w), u32::from(h)));
            }
            0xFF => i += 1,
            0x01 | 0xD0..=0xD9 => i += 2,
            _ => i += 2 + usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]])),
        }
    }
    None
}

/// Short description of data: `PNG 16 × 8`, `512 bytes`.
pub fn summary(bytes: &[u8]) -> String {
    match PictureInfo::of(bytes) {
        Some(i) => i.summary(),
        None => crate::i18n::trn(bytes.len(), "{n} byte", "{n} bytes"),
    }
}

/// Upper-case hex, as Sisulizer stores data.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    s
}

/// Bytes of hex text (white space ignored); `None` if the text is not hex.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    let value = |d: u8| char::from(d).to_digit(16).and_then(|v| u8::try_from(v).ok());
    digits.chunks(2).map(|p| Some(value(p[0])? << 4 | value(p[1])?)).collect()
}

/// Hex as element content the way Sisulizer writes it: one line break, lines indented by `indent`
/// spaces that end in column 78, then the indentation of the closing tag (`indent - 2`).
pub fn wrap_hex(hex: &str, indent: usize) -> String {
    let width = LINE_END.saturating_sub(indent).max(2) & !1;
    let mut s = String::with_capacity(hex.len() + hex.len() / width * (indent + 1) + indent + 2);
    for chunk in hex.as_bytes().chunks(width) {
        s.push('\n');
        s.push_str(&" ".repeat(indent));
        s.push_str(std::str::from_utf8(chunk).unwrap_or_default());
    }
    s.push('\n');
    s.push_str(&" ".repeat(indent.saturating_sub(2)));
    s
}

/// Fingerprint of binary data: up to 11 bytes in full, otherwise `first 8...last 3` bytes.
pub fn thumbnail(bytes: &[u8]) -> String {
    if bytes.len() <= 11 { to_hex(bytes) } else { format!("{}...{}", to_hex(&bytes[..8]), to_hex(&bytes[bytes.len() - 3..])) }
}

/// `filedate` of a linked file: its modification time as a DOS time (rounded up to even seconds),
/// local time `yyyymmddhhnnss`.
pub fn file_date(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() + u64::from(since.subsec_nanos() > 0);
    let even = UNIX_EPOCH + Duration::from_secs(secs + secs % 2);
    crate::model::stamp_of(even)
}

impl Row {
    /// Kind of custom data held by the row (`None` for text rows).
    pub fn data_kind(&self) -> Option<DataKind> {
        self.attrs.get("class").and_then(DataKind::of_row_class)
    }

    pub fn is_data(&self) -> bool {
        self.data_kind().is_some()
    }

    /// No text to translate, count, validate or exchange: custom data or other raw content.
    pub fn is_data_or_raw(&self) -> bool {
        self.raw || self.is_data()
    }

    /// Bytes of the original (pictures only; binary rows keep a thumbnail).
    pub fn original_bytes(&self) -> Option<Vec<u8>> {
        match self.data_kind()? {
            DataKind::Picture => self.native.as_deref().and_then(from_hex).filter(|b| !b.is_empty()),
            DataKind::Binary => None,
        }
    }

    /// Bytes of the translation in `lang`.
    pub fn translation_bytes(&self, lang: &str) -> Option<Vec<u8>> {
        self.is_data().then(|| self.translation(lang)).flatten().and_then(|t| from_hex(&t.text)).filter(|b| !b.is_empty())
    }

    /// Bytes to write into a localized file: the translation if translated and the row is not locked.
    pub fn build_bytes(&self, lang: &str) -> Option<Vec<u8>> {
        self.build_text(lang)?;
        self.translation_bytes(lang)
    }

    /// Sets the translation of a data row; an empty slice removes it.
    pub fn set_translation_bytes(&mut self, lang: &str, bytes: &[u8], status: TranslationStatus) {
        self.normalize_data();
        self.set_translation(lang, &to_hex(bytes), status);
    }

    /// Stores the hex of a data row without line breaks, so that the writer wraps it for its
    /// position (inline content or `<native>`, like Sisulizer when a translation is added).
    pub fn normalize_data(&mut self) {
        if !self.is_data() {
            return;
        }
        let strip = |s: &mut String| {
            if from_hex(s).is_some() {
                s.retain(|c| !c.is_ascii_whitespace());
            }
        };
        if let Some(n) = self.native.as_mut() {
            strip(n);
        }
        for tr in &mut self.translations {
            strip(&mut tr.text);
        }
        self.raw = false;
    }

    /// Replaces the original data; returns whether it changed. Translations of a changed original
    /// are marked for review, as for text rows.
    pub fn update_data(&mut self, bytes: &[u8], stamp: &str) -> bool {
        let kind = DataKind::of_bytes(bytes);
        let changed = match (self.data_kind(), kind) {
            (Some(DataKind::Picture), DataKind::Picture) => self.original_bytes().as_deref() != Some(bytes),
            (Some(DataKind::Binary), DataKind::Binary) => self.attrs.get("thumbnail") != Some(thumbnail(bytes).as_str()),
            _ => true,
        };
        if !changed {
            if self.status() == RowStatus::Unused {
                self.set_status(RowStatus::InUse);
            }
            return false;
        }
        self.normalize_data();
        self.attrs.set("class", kind.row_class());
        match kind {
            DataKind::Picture => {
                self.attrs.remove("thumbnail");
                self.native = Some(to_hex(bytes));
            }
            DataKind::Binary => {
                self.native = None;
                self.attrs.set("thumbnail", thumbnail(bytes));
            }
        }
        self.inline_native = false;
        self.attrs.set("changed", stamp);
        if self.status() != RowStatus::New {
            self.set_status(RowStatus::Changed);
        }
        for tr in &mut self.translations {
            if !tr.text.is_empty() {
                tr.attrs.set("invalidated", "1");
                tr.set_status(TranslationStatus::ForReview);
            }
        }
        true
    }
}

impl crate::model::Translation {
    /// File the translation is linked to (`filename`, relative to the project).
    pub fn linked_file(&self) -> Option<&str> {
        self.attrs.get("filename").filter(|f| !f.is_empty())
    }

    pub fn set_link(&mut self, file: &str, date: &str) {
        self.attrs.set("filedate", date);
        self.attrs.set("filename", file);
    }

    pub fn remove_link(&mut self) {
        self.attrs.remove("filedate");
        self.attrs.remove("filename");
    }
}

/// Path of a linked file, resolved against the project directory.
pub fn resolve_link(p: &Project, file: &str) -> PathBuf {
    crate::ops::normalize(&crate::ops::base_abs(&p.base_dir()).join(crate::ops::native_path(file)))
}

/// `file` as stored in `filename`: relative to the project directory (absolute on another drive).
pub fn link_name(p: &Project, file: &Path) -> String {
    let abs = crate::ops::normalize(&crate::ops::base_abs(file));
    let base = crate::ops::normalize(&crate::ops::base_abs(&p.base_dir()));
    crate::ops::relative_path(&base, &abs).unwrap_or(abs).to_string_lossy().replace('/', "\\")
}

/// Translations of data rows linked to files: row, language and resolved path.
pub fn linked_files(p: &Project) -> Vec<(RowRef, String, PathBuf)> {
    let mut out = Vec::new();
    for r in p.rows() {
        let Some(row) = p.row(&r).filter(|row| row.is_data()) else { continue };
        for tr in &row.translations {
            if let Some(f) = tr.linked_file() {
                out.push((r.clone(), tr.lang().to_string(), resolve_link(p, f)));
            }
        }
    }
    out
}

/// Number of picture rows, binary rows and linked translation files of the project.
pub fn counts(p: &Project) -> (usize, usize, usize) {
    let (mut pictures, mut binary) = (0, 0);
    for r in p.rows() {
        match p.row(&r).and_then(Row::data_kind) {
            Some(DataKind::Picture) => pictures += 1,
            Some(DataKind::Binary) => binary += 1,
            None => {}
        }
    }
    (pictures, binary, linked_files(p).len())
}

/// Loads the translation of a data row from `file`; `link` keeps it linked to the file.
pub fn load_translation_file(p: &mut Project, r: &RowRef, lang: &str, file: &Path, link: bool) -> Result<()> {
    let bytes = read_file(file)?;
    let name = link_name(p, file);
    let date = std::fs::metadata(file).and_then(|m| m.modified()).map(file_date).unwrap_or_default();
    let row = p.row_mut(r).ok_or_else(|| crate::Error::NotFound(t("row").into()))?;
    if !row.is_data() {
        return Err(crate::Error::Unsupported(t("the row contains no picture or binary data").into()));
    }
    let status = row.tstatus(lang).after_edit();
    row.set_translation_bytes(lang, &bytes, status);
    if let Some(tr) = row.translation_mut(lang) {
        if link { tr.set_link(&name, &date) } else { tr.remove_link() }
    }
    Ok(())
}

/// Removes the file link of a translation (the data stays). Returns whether there was one.
pub fn unlink_translation(p: &mut Project, r: &RowRef, lang: &str) -> bool {
    match p.row_mut(r).and_then(|row| row.translation_mut(lang)).filter(|tr| tr.linked_file().is_some()) {
        Some(tr) => {
            tr.remove_link();
            true
        }
        None => false,
    }
}

/// Result of [`refresh_linked`].
#[derive(Debug, Default)]
pub struct RefreshReport {
    /// Translations read again from their changed file.
    pub updated: Vec<(RowRef, String)>,
    /// Linked files that do not exist.
    pub missing: Vec<PathBuf>,
}

/// Reads linked files whose content differs from the translation again (Sisulizer compares the
/// file date; Susi compares the content because git sets file dates on checkout). Unchanged
/// files are left alone, so a checkout does not change the project.
pub fn refresh_linked(p: &mut Project) -> RefreshReport {
    let mut rep = RefreshReport::default();
    for (r, lang, path) in linked_files(p) {
        let Ok(bytes) = std::fs::read(&path) else {
            rep.missing.push(path);
            continue;
        };
        let date = std::fs::metadata(&path).and_then(|m| m.modified()).map(file_date).unwrap_or_default();
        let Some(row) = p.row_mut(&r) else { continue };
        if row.translation_bytes(&lang).as_deref() == Some(bytes.as_slice()) {
            continue;
        }
        let status = row.tstatus(&lang).after_edit();
        let name = row.translation(&lang).and_then(|tr| tr.linked_file()).unwrap_or_default().to_string();
        row.set_translation_bytes(&lang, &bytes, status);
        if let Some(tr) = row.translation_mut(&lang) {
            tr.set_link(&name, &date);
        }
        rep.updated.push((r, lang));
    }
    rep
}

/// Problems of the translations of a data row in `langs` (validation check `data`): missing linked
/// files, picture translations of another format or size than the original, translations of a
/// picture that are no picture. Returns (language, message).
pub fn issues(p: &Project, row: &Row, langs: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let original = row.original_bytes().as_deref().and_then(PictureInfo::of);
    for tr in row.translations.iter().filter(|tr| langs.iter().any(|l| l == tr.lang())) {
        let lang = tr.lang().to_string();
        if let Some(f) = tr.linked_file()
            && !resolve_link(p, f).exists()
        {
            out.push((lang.clone(), crate::tf!("Linked file {file} not found.", file = f)));
        }
        let Some(bytes) = row.translation_bytes(&lang) else { continue };
        match (row.data_kind(), original, PictureInfo::of(&bytes)) {
            (Some(DataKind::Picture), _, None) => out.push((lang, crate::tf!("The translation is no supported picture ({data}).", data = summary(&bytes)))),
            (_, Some(o), Some(t)) if (o.format, o.width, o.height) != (t.format, t.width, t.height) => out.push((
                lang,
                crate::tf!("The translated picture ({translation}) differs from the original ({original}).", translation = t.summary(), original = o.summary()),
            )),
            _ => {}
        }
    }
    out
}

/// Custom data scanned from a resource: the resource name and its bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedData {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Data types excluded from scanning (`excludeddatatypes` of a source, e.g. `[xdtImage,xdtBinary]`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Excluded {
    pub images: bool,
    pub binary: bool,
    pub text: bool,
}

impl Excluded {
    pub fn parse(value: &str) -> Self {
        let has = |k: &str| value.trim_matches(['[', ']']).split(',').any(|v| v.trim().eq_ignore_ascii_case(k));
        Self { images: has("xdtImage"), binary: has("xdtBinary"), text: has("xdtText") }
    }

    /// Attribute value, `None` when nothing is excluded.
    pub fn value(&self) -> Option<String> {
        let v: Vec<&str> =
            [(self.binary, "xdtBinary"), (self.images, "xdtImage"), (self.text, "xdtText")].iter().filter(|(on, _)| *on).map(|(_, k)| *k).collect();
        (!v.is_empty()).then(|| format!("[{}]", v.join(",")))
    }

    pub fn excludes(&self, kind: DataKind) -> bool {
        match kind {
            DataKind::Picture => self.images,
            DataKind::Binary => self.binary,
        }
    }
}

impl crate::model::Source {
    /// Data types not scanned (`excludeddatatypes`; Sisulizer: *Source | Excluded types*).
    pub fn excluded_data(&self) -> Excluded {
        self.attrs.get("excludeddatatypes").map(Excluded::parse).unwrap_or_default()
    }

    pub fn set_excluded_data(&mut self, e: Excluded) {
        match e.value() {
            Some(v) => self.attrs.set("excludeddatatypes", v),
            None => {
                self.attrs.remove("excludeddatatypes");
            }
        }
    }
}

/// A new scanned data row, its attributes in Sisulizer's order (`state` and `created` before
/// `thumbnail`).
fn new_row(kind: DataKind, id: &str, bytes: &[u8], stamp: &str) -> Row {
    let mut r = Row::new(id, "");
    r.attrs.set("class", kind.row_class());
    r.set_status(RowStatus::New);
    r.attrs.set("created", stamp);
    match kind {
        DataKind::Picture => r.native = Some(to_hex(bytes)),
        DataKind::Binary => {
            r.native = None;
            r.attrs.set("thumbnail", thumbnail(bytes));
        }
    }
    r
}

/// Merges scanned custom data into a container: one data node with one row per resource.
/// Returns (new, changed, unused) counts. Nodes of other data classes with the same name (XML or
/// HTML data of Sisulizer) are left alone.
pub fn merge_data(items: &mut Vec<Item>, scanned: &[ScannedData], excluded: Excluded, stamp: &str) -> (usize, usize, usize) {
    let (mut new, mut changed, mut unused) = (0, 0, 0);
    let mut seen = std::collections::HashSet::new();
    for d in scanned {
        let kind = DataKind::of_bytes(&d.bytes);
        if d.bytes.is_empty() || excluded.excludes(kind) || !seen.insert(d.name.as_str()) {
            continue;
        }
        let existing = items.iter().position(|i| matches!(i, Item::Node(n) if is_data_node(n) && n.name() == d.name));
        let Some(idx) = existing else {
            let mut node = Node::new(kind.node_class(), &d.name);
            node.items.push(Item::Row(new_row(kind, &d.name, &d.bytes, stamp)));
            items.push(Item::Node(node));
            new += 1;
            continue;
        };
        let Some(Item::Node(node)) = items.get_mut(idx) else { continue };
        if !matches!(node.attrs.get("class"), Some(PICTURE_NODE | BINARY_NODE)) {
            continue;
        }
        node.attrs.set("class", kind.node_class());
        let row = node.items.iter_mut().find_map(|i| match i {
            Item::Row(r) if r.id() == d.name => Some(r),
            _ => None,
        });
        match row {
            Some(r) => changed += usize::from(r.update_data(&d.bytes, stamp)),
            None => {
                node.items.push(Item::Row(new_row(kind, &d.name, &d.bytes, stamp)));
                new += 1;
            }
        }
    }
    for item in items.iter_mut() {
        let Item::Node(n) = item else { continue };
        if !matches!(n.attrs.get("class"), Some(PICTURE_NODE | BINARY_NODE)) || seen.contains(n.name()) {
            continue;
        }
        for i in &mut n.items {
            if let Item::Row(r) = i
                && r.status() != RowStatus::Unused
            {
                r.set_status(RowStatus::Unused);
                unused += 1;
            }
        }
    }
    (new, changed, unused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_wrapping() {
        assert_eq!(to_hex(&[0, 0xAB, 0x10]), "00AB10");
        assert_eq!(from_hex(" 00ab\n  10 "), Some(vec![0, 0xAB, 0x10]));
        assert_eq!(from_hex("0U"), None);
        assert_eq!(from_hex("ABC"), None);
        let hex = "0".repeat(150);
        for (indent, width) in [(8, 70), (10, 68), (12, 66), (14, 64)] {
            let w = wrap_hex(&hex, indent);
            let lines: Vec<&str> = w.split('\n').collect();
            assert_eq!(lines[1].len(), indent + width, "indent {indent}");
            assert_eq!(*lines.last().unwrap(), " ".repeat(indent - 2));
            assert_eq!(from_hex(&w).unwrap().len(), 75);
        }
    }

    #[test]
    fn thumbnails() {
        assert_eq!(thumbnail(&[1, 2, 3, 4, 5]), "0102030405");
        assert_eq!(thumbnail(&(0..11).collect::<Vec<u8>>()), "000102030405060708090A");
        assert_eq!(thumbnail(&(0..12).collect::<Vec<u8>>()), "0001020304050607...090A0B");
    }

    #[test]
    fn picture_detection() {
        let png = [b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".as_slice(), &16u32.to_be_bytes(), &8u32.to_be_bytes()].concat();
        assert_eq!(PictureInfo::of(&png).map(|i| i.summary()).as_deref(), Some("PNG 16 × 8"));
        let gif = [b"GIF89a".as_slice(), &[3, 0, 2, 0]].concat();
        assert_eq!(PictureInfo::of(&gif).map(|i| (i.width, i.height)), Some((3, 2)));
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[14] = 40;
        bmp[18..22].copy_from_slice(&4i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-2i32).to_le_bytes());
        assert_eq!(PictureInfo::of(&bmp).map(|i| (i.format, i.width, i.height)), Some((PictureFormat::Bmp, 4, 2)));
        let mut ico = vec![0, 0, 1, 0, 1, 0, 16, 0];
        ico.resize(22, 0);
        assert_eq!(PictureInfo::of(&ico).map(|i| (i.format, i.width, i.height)), Some((PictureFormat::Ico, 16, 256)));
        ico[2] = 2;
        assert_eq!(PictureFormat::of(&ico), Some(PictureFormat::Cur));
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 5, 0, 7, 3, 0, 0, 0];
        assert_eq!(PictureInfo::of(&jpeg).map(|i| (i.width, i.height)), Some((7, 5)));
        assert_eq!(DataKind::of_bytes(b"BMx"), DataKind::Binary);
        assert_eq!(DataKind::of_bytes(&[0xEF, 0xBB, 0xBF, b'a']), DataKind::Binary);
        assert_eq!(summary(&[1, 2, 3]), "3 bytes");
    }

    #[test]
    fn dos_file_dates() {
        let at = |secs: u64, nanos: u32| file_date(UNIX_EPOCH + Duration::new(secs, nanos));
        let base = 1_700_000_000; // even
        assert_eq!(at(base, 0), crate::model::stamp_of(UNIX_EPOCH + Duration::from_secs(base)));
        assert_eq!(at(base - 1, 0), at(base, 0), "odd seconds round up");
        // 1 µs: SystemTime on Windows has a resolution of 100 ns.
        assert_eq!(at(base, 1_000), at(base + 2, 0), "fractions round up to the next even second");
    }

    #[test]
    fn excluded_data_types() {
        let e = Excluded::parse("[xdtImage,xdtBinary]");
        assert!(e.images && e.binary && !e.text);
        assert_eq!(e.value().as_deref(), Some("[xdtBinary,xdtImage]"));
        assert_eq!(Excluded::default().value(), None);
    }

    #[test]
    fn merging_data_nodes() {
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[1, 2, 3]].concat();
        let mut items = Vec::new();
        let scanned = vec![ScannedData { name: "Logo".into(), bytes: png.clone() }, ScannedData { name: "Blob".into(), bytes: vec![9; 20] }];
        assert_eq!(merge_data(&mut items, &scanned, Excluded::default(), "1"), (2, 0, 0));
        let row = |items: &[Item], i: usize| match &items[i] {
            Item::Node(n) => match &n.items[0] {
                Item::Row(r) => (n.attrs.get("class").unwrap_or("").to_string(), r.clone()),
                _ => panic!(),
            },
            _ => panic!(),
        };
        let (class, logo) = row(&items, 0);
        assert_eq!((class.as_str(), logo.original_bytes()), (PICTURE_NODE, Some(png.clone())));
        let (class, blob) = row(&items, 1);
        assert_eq!((class.as_str(), blob.attrs.get("thumbnail")), (BINARY_NODE, Some("0909090909090909...090909")));
        assert!(blob.native.is_none());

        // Unchanged data keeps the rows; a changed picture is marked, translations need review.
        if let Item::Node(n) = &mut items[0]
            && let Item::Row(r) = &mut n.items[0]
        {
            r.set_status(RowStatus::InUse);
            r.set_translation_bytes("de", &[1], TranslationStatus::Translated);
        }
        assert_eq!(merge_data(&mut items, &scanned, Excluded::default(), "2"), (0, 0, 0));
        let changed = vec![ScannedData { name: "Logo".into(), bytes: [png.as_slice(), &[4]].concat() }];
        assert_eq!(merge_data(&mut items, &changed, Excluded { binary: true, ..Default::default() }, "3"), (0, 1, 1));
        let (_, logo) = row(&items, 0);
        assert_eq!((logo.status(), logo.tstatus("de"), logo.attrs.get("changed")), (RowStatus::Changed, TranslationStatus::ForReview, Some("3")));
        assert_eq!(logo.translation_bytes("de"), Some(vec![1]));
        assert_eq!(row(&items, 1).1.status(), RowStatus::Unused);

        // Foreign data nodes of the same name stay untouched.
        let mut items = vec![Item::Node(Node::new("TXmlDataNode", "Logo"))];
        assert_eq!(merge_data(&mut items, &scanned[..1], Excluded::default(), "1"), (0, 0, 0));
        assert!(matches!(&items[0], Item::Node(n) if n.items.is_empty()));
    }
}
