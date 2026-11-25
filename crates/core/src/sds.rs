//! Sisulizer desktop file (`<project>.sds`) next to the `.slp`: view state of the project (window
//! layout, current row, sheet columns, …) and the validations excluded by the user.
//!
//! Observed (Sisulizer 4.0.374, Validation › Exclude):
//!
//! ```xml
//! <desktop scan="1" scanned="…" lang="ar" pane="TValidationPane" …>
//!   <validator>
//!     <translation>
//!       <excludedvalidations>
//!         <item class="TFormatValidation" context="%d file[0]" native="%d file"/>
//!       </excludedvalidations>
//!     </translation>
//!   </validator>
//!   …
//! </desktop>
//! ```
//!
//! An item excludes one check (`class`) of one row (`context` + `native`) for all languages.
//! `<translation>` holds the exclusions of *Validate Translation*, `<original>` those of *Validate
//! Original* (the element names are in the Sisulizer binary next to `excludedvalidations`).
//!
//! Susi only interprets `<validator>/<original|translation>/<excludedvalidations>`; everything else
//! is kept as read. The file is written only when the exclusions changed (or the project was saved
//! under a new name with exclusions), so Sisulizer's view state stays untouched otherwise.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::{Result, read_file, write_file};
use crate::model::{Project, RowRef};
use crate::xml::{self, Element, Node};

/// Which validation an exclusion belongs to: *Validate Original* or *Validate Translation*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Original,
    Translation,
}

impl Scope {
    pub const ALL: [Scope; 2] = [Scope::Original, Scope::Translation];

    fn tag(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Translation => "translation",
        }
    }
}

/// One excluded validation: check `class` (Sisulizer class name, e.g. `TFormatValidation`) of the
/// row identified by `context` and its original text `native`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Exclusion {
    pub scope: Scope,
    pub class: String,
    pub context: String,
    pub native: String,
}

/// Contents of the `.sds` file that Susi needs, plus the rest of the document to write it back.
#[derive(Debug, Clone, Default)]
pub struct Desktop {
    /// The file as read (`None`: there was none).
    doc: Option<xml::Document>,
    exclusions: Vec<Exclusion>,
    dirty: bool,
}

/// Path of the desktop file belonging to the project file `slp`.
pub fn path(slp: &Path) -> PathBuf {
    slp.with_extension("sds")
}

impl Desktop {
    /// Reads `path`; a missing file gives an empty desktop.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::parse(&read_file(path)?)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let doc = xml::parse_bytes(bytes)?;
        let mut exclusions = Vec::new();
        if let Some(v) = doc.root.child("validator") {
            for scope in Scope::ALL {
                let items = v.child(scope.tag()).and_then(|s| s.child("excludedvalidations"));
                for item in items.into_iter().flat_map(|l| l.elements()).filter(|e| e.name == "item") {
                    let get = |k: &str| crate::slp::decode(item.attr(k).unwrap_or_default());
                    let class = get("class");
                    if !class.is_empty() {
                        exclusions.push(Exclusion { scope, class, context: get("context"), native: get("native") });
                    }
                }
            }
        }
        Ok(Self { doc: Some(doc), exclusions, dirty: false })
    }

    pub fn exclusions(&self) -> &[Exclusion] {
        &self.exclusions
    }

    /// Exclusions of one validation scope.
    pub fn exclusions_of(&self, scope: Scope) -> impl Iterator<Item = &Exclusion> {
        self.exclusions.iter().filter(move |e| e.scope == scope)
    }

    /// Whether the exclusions changed since loading or saving.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Adds `e` unless it is already excluded. Returns whether it was added.
    pub fn add(&mut self, e: Exclusion) -> bool {
        if self.exclusions.contains(&e) {
            return false;
        }
        self.exclusions.push(e);
        self.dirty = true;
        true
    }

    /// Removes `e`. Returns whether it was there.
    pub fn remove(&mut self, e: &Exclusion) -> bool {
        let before = self.exclusions.len();
        self.exclusions.retain(|x| x != e);
        let removed = self.exclusions.len() != before;
        self.dirty |= removed;
        removed
    }

    /// Removes all exclusions of `scope`. Returns how many there were.
    pub fn clear(&mut self, scope: Scope) -> usize {
        let before = self.exclusions.len();
        self.exclusions.retain(|x| x.scope != scope);
        let removed = before - self.exclusions.len();
        self.dirty |= removed > 0;
        removed
    }

    /// Writes the file if the exclusions changed, or if the project moved (`moved`) and has
    /// exclusions or a desktop file to take along.
    pub fn save(&mut self, path: &Path, moved: bool) -> Result<()> {
        if !self.dirty && !(moved && (self.doc.is_some() || !self.exclusions.is_empty())) {
            return Ok(());
        }
        let doc = self.doc.get_or_insert_with(|| xml::Document {
            bom: true,
            decl: Some("xml version=\"1.0\" encoding=\"utf-8\"".into()),
            root: Element::new("desktop"),
            crlf: true,
            ..Default::default()
        });
        set_exclusions(&mut doc.root, &self.exclusions);
        write_file(path, &to_bytes(doc))?;
        self.dirty = false;
        Ok(())
    }
}

/// Replaces the `<excludedvalidations>` lists below `<validator>`; empty lists (and the containers
/// left empty by that) are removed, everything else stays.
fn set_exclusions(root: &mut Element, exclusions: &[Exclusion]) {
    if root.child("validator").is_none() {
        if exclusions.is_empty() {
            return;
        }
        // Sisulizer writes the validator right after the languages.
        let pos = root.children.iter().rposition(|c| matches!(c, Node::Element(e) if e.name == "language")).map_or(0, |i| i + 1);
        root.children.insert(pos, Node::Element(Element::new("validator")));
    }
    let Some(validator) = root.child_mut("validator") else {
        return;
    };
    for scope in Scope::ALL {
        let items: Vec<Node> = exclusions
            .iter()
            .filter(|e| e.scope == scope)
            .map(|e| {
                let mut item = Element::new("item");
                item.attrs.set("class", e.class.as_str());
                item.attrs.set("context", crate::slp::encode(&e.context));
                item.attrs.set("native", crate::slp::encode(&e.native));
                Node::Element(item)
            })
            .collect();
        if validator.child(scope.tag()).is_none() {
            if items.is_empty() {
                continue;
            }
            // `<original>` before `<translation>`, like the strings in the Sisulizer binary.
            let el = Node::Element(Element::new(scope.tag()));
            match scope {
                Scope::Original => validator.children.insert(0, el),
                Scope::Translation => validator.children.push(el),
            }
        }
        let Some(s) = validator.child_mut(scope.tag()) else {
            continue;
        };
        s.children.retain(|c| !matches!(c, Node::Element(e) if e.name == "excludedvalidations"));
        if !items.is_empty() {
            let mut list = Element::new("excludedvalidations");
            list.children = items;
            s.children.push(Node::Element(list));
        }
    }
    let empty = |e: &Element| e.attrs.is_empty() && !e.has_element_children();
    validator.children.retain(|c| !matches!(c, Node::Element(e) if Scope::ALL.iter().any(|s| s.tag() == e.name) && empty(e)));
    if empty(validator) {
        root.children.retain(|c| !matches!(c, Node::Element(e) if e.name == "validator"));
    }
}

/// Written like Sisulizer: UTF-8 with BOM, CRLF, two-space indentation, `<x/>`.
fn to_bytes(doc: &xml::Document) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(&format!("<?{}?>\n", doc.decl.as_deref().unwrap_or("xml version=\"1.0\" encoding=\"utf-8\"")));
    crate::slp::write_generic(&mut s, 0, &doc.root);
    let s = s.replace('\n', "\r\n");
    let mut out = Vec::with_capacity(s.len() + 3);
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    out.extend_from_slice(s.as_bytes());
    out
}

/// Context under which Sisulizer identifies row `r`: the row id, preceded by the names of the
/// nodes below the source's file node, joined by `.`. Verified for Gettext (`%d file[0]`); for
/// deeper structures it is a best guess, which [`context_matches`] makes up for when reading.
pub fn context(p: &Project, r: &RowRef) -> String {
    let id = p.row(r).map(|x| x.id().to_string()).unwrap_or_default();
    let names = p.node_names(r);
    let mut parts: Vec<&str> = names.iter().skip(1).map(String::as_str).collect();
    parts.push(&id);
    parts.join(".")
}

/// Whether `context` (as stored in an exclusion) identifies row `r`: the row id alone or with any
/// number of the innermost node names, joined by `.` or `/` (Susi's own context included).
pub fn context_matches(p: &Project, r: &RowRef, context: &str) -> bool {
    let Some(row) = p.row(r) else {
        return false;
    };
    let id = row.id();
    let Some(prefix) = context.strip_suffix(id) else {
        return false;
    };
    if prefix.is_empty() {
        return true;
    }
    let names = p.node_names(r);
    [".", "/"].iter().any(|sep| {
        (0..names.len()).any(|k| {
            let mut joined = names[k..].join(sep);
            joined.push_str(sep);
            joined == prefix
        })
    })
}

/// Index of the exclusions for fast lookups during validation.
pub(crate) struct Index<'a> {
    set: HashSet<(Scope, &'a str, &'a str)>,
    all: &'a [Exclusion],
}

impl<'a> Index<'a> {
    pub(crate) fn new(d: &'a Desktop) -> Self {
        Self { set: d.exclusions.iter().map(|e| (e.scope, e.class.as_str(), e.native.as_str())).collect(), all: &d.exclusions }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// Whether check `class` of row `r` (original text `native`) is excluded in `scope`.
    pub(crate) fn excludes(&self, p: &Project, r: &RowRef, scope: Scope, class: &str, native: &str) -> bool {
        self.set.contains(&(scope, class, native))
            && self.all.iter().any(|e| e.scope == scope && e.class == class && e.native == native && context_matches(p, r, &e.context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
        <desktop scan=\"1\" lang=\"ar\" row=\"%d file[0]\">\r\n  \
        <language id=\"de\" autoime=\"0\" autokeyboard=\"0\"/>\r\n  \
        <validator>\r\n    <original>\r\n      <type class=\"TCharacterWidthValidation\" validate=\"1\"/>\r\n    </original>\r\n    \
        <translation>\r\n      <excludedvalidations>\r\n        \
        <item class=\"TFormatValidation\" context=\"%d file[0]\" native=\"%d file\"/>\r\n      \
        </excludedvalidations>\r\n    </translation>\r\n  </validator>\r\n  \
        <sheet>\r\n    <columns>\r\n      <item class=\"TFlagsColumn\" width=\"38\"/>\r\n    </columns>\r\n  </sheet>\r\n\
        </desktop>\r\n";

    fn excl(scope: Scope, class: &str, context: &str, native: &str) -> Exclusion {
        Exclusion { scope, class: class.into(), context: context.into(), native: native.into() }
    }

    #[test]
    fn reads_sisulizer_exclusions() {
        let d = Desktop::parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(d.exclusions(), [excl(Scope::Translation, "TFormatValidation", "%d file[0]", "%d file")]);
        assert!(!d.is_dirty());
    }

    #[test]
    fn writes_back_unchanged_apart_from_the_exclusions() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("p.sds");
        let mut d = Desktop::parse(SAMPLE.as_bytes()).unwrap();
        // Nothing changed: nothing written.
        d.save(&file, false).unwrap();
        assert!(!file.exists());
        // Unchanged content round-trips byte for byte.
        d.dirty = true;
        d.save(&file, false).unwrap();
        assert_eq!(String::from_utf8(std::fs::read(&file).unwrap()).unwrap(), SAMPLE);

        assert!(d.add(excl(Scope::Original, "TSusiControlCharactersValidation", "A#B", "line\r\nbreak")));
        assert!(!d.add(excl(Scope::Original, "TSusiControlCharactersValidation", "A#B", "line\r\nbreak")));
        d.save(&file, false).unwrap();
        let text = String::from_utf8(std::fs::read(&file).unwrap()).unwrap();
        // Sisulizer's validation settings next to the list stay.
        assert!(
            text.contains(
                "  <validator>\r\n    <original>\r\n      <type class=\"TCharacterWidthValidation\" validate=\"1\"/>\r\n      \
                 <excludedvalidations>\r\n        \
                 <item class=\"TSusiControlCharactersValidation\" context=\"A##B\" native=\"line#c#lbreak\"/>\r\n      \
                 </excludedvalidations>\r\n    </original>\r\n    <translation>"
            ),
            "{text}"
        );
        assert!(text.contains("<item class=\"TFlagsColumn\" width=\"38\"/>"));
        let again = Desktop::load(&file).unwrap();
        assert_eq!(again.exclusions().len(), 2);
        assert_eq!(again.exclusions_of(Scope::Original).next().unwrap().native, "line\r\nbreak");

        // Removing everything removes the lists and the containers left empty, the rest stays.
        let mut d = again;
        assert_eq!(d.clear(Scope::Original), 1);
        assert!(d.remove(&excl(Scope::Translation, "TFormatValidation", "%d file[0]", "%d file")));
        d.save(&file, false).unwrap();
        let text = String::from_utf8(std::fs::read(&file).unwrap()).unwrap();
        assert!(
            text.contains(
                "  <validator>\r\n    <original>\r\n      <type class=\"TCharacterWidthValidation\" validate=\"1\"/>\r\n    </original>\r\n  </validator>\r\n"
            ),
            "{text}"
        );
        assert!(!text.contains("excludedvalidations") && !text.contains("<translation>"), "{text}");
        assert!(text.contains("<sheet>"));
    }

    #[test]
    fn new_file_only_with_exclusions() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("new.sds");
        let mut d = Desktop::load(&file).unwrap();
        d.save(&file, true).unwrap();
        assert!(!file.exists(), "no exclusions, no file");
        d.add(excl(Scope::Translation, "TMissingPeriodValidation", "OK", "OK."));
        d.save(&file, false).unwrap();
        let text = String::from_utf8(std::fs::read(&file).unwrap()).unwrap();
        assert!(text.starts_with("\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<desktop>\r\n  <validator>\r\n    <translation>"), "{text}");
    }

    #[test]
    fn contexts() {
        let slp = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<document>\n  <lang id=\"de\"/>\n  \
                   <source class=\"TVclSource\" name=\"a.exe\" original=\"en\">\n    \
                   <node class=\"TFormNode\" name=\"Form1\">\n      <node class=\"TComponentNode\" name=\"Button1\">\n        \
                   <row id=\"Caption\"><native>OK</native></row>\n      </node>\n    </node>\n    \
                   <node class=\"TPoNode\" name=\"messages.pot\">\n      <row id=\"%d file[0]\"><native>%d file</native></row>\n    </node>\n  \
                   </source>\n</document>\n";
        let p = crate::slp::parse(slp.as_bytes()).unwrap();
        let rows = p.rows();
        assert_eq!(context(&p, &rows[0]), "Button1.Caption");
        assert_eq!(context(&p, &rows[1]), "%d file[0]");
        for c in ["Caption", "Button1.Caption", "Form1.Button1.Caption", "Form1/Button1/Caption", "Button1/Caption"] {
            assert!(context_matches(&p, &rows[0], c), "{c}");
        }
        for c in ["Button2.Caption", "XCaption", "Form1.Caption.x", "1.Caption", ""] {
            assert!(!context_matches(&p, &rows[0], c), "{c}");
        }
        assert!(context_matches(&p, &rows[1], "%d file[0]"));
        assert!(context_matches(&p, &rows[1], &p.context(&rows[1])));
    }
}
