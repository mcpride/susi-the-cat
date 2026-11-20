//! Custom data (pictures, binary data, text file references) of RESX sources, compared with what
//! Sisulizer 4.0.374 produced for the same files (`testdata/projects/custom-data/sisulizer/`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use susi_core::custom_data::{self, BINARY_NODE, DataKind, Excluded, PICTURE_NODE};
use susi_core::model::{Item, Project, Row};
use susi_core::{RowStatus, TranslationStatus, ops, slp, xml};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/custom-data")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// The fixture in a temporary directory, with a project of Sisulizer (`scan.slp` or
/// `translated.slp`) next to the original as `app.slp`.
fn workspace(sisulizer: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture(), dir.path());
    let slp = dir.path().join("app.slp");
    std::fs::copy(fixture().join("sisulizer").join(sisulizer), &slp).unwrap();
    (dir, slp)
}

/// Data nodes of the first source: name -> (node class, row).
fn data_nodes(p: &Project) -> BTreeMap<String, (String, Row)> {
    let mut out = BTreeMap::new();
    for item in &p.sources[0].items {
        if let Item::Node(n) = item
            && let Some(Item::Row(r)) = n.items.first()
        {
            out.insert(n.name().to_string(), (n.attrs.get("class").unwrap_or("").to_string(), r.clone()));
        }
    }
    out
}

fn texts(p: &Project) -> BTreeMap<String, String> {
    p.sources[0].items.iter().filter_map(|i| if let Item::Row(r) = i { Some((r.id().to_string(), r.original().to_string())) } else { None }).collect()
}

#[test]
fn scan_matches_sisulizer() {
    let (dir, _) = workspace("scan.slp");
    let mut p = Project::new();
    p.path = Some(dir.path().join("new.slp"));
    p.add_language("en");
    p.add_language("de");
    let (_, rep) = ops::add_source(&mut p, &dir.path().join("Strings.resx"), "en").unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    let sisulizer = slp::load(&fixture().join("sisulizer").join("scan.slp")).unwrap();

    assert_eq!(texts(&p), texts(&sisulizer), "strings and text file references");
    let (ours, theirs) = (data_nodes(&p), data_nodes(&sisulizer));
    assert_eq!(ours.keys().collect::<Vec<_>>(), theirs.keys().collect::<Vec<_>>());
    for (name, (class, theirs)) in &theirs {
        let (our_class, ours) = &ours[name];
        assert_eq!(our_class, class, "{name}");
        assert_eq!(ours.attrs.get("class"), theirs.attrs.get("class"), "{name}");
        assert_eq!(ours.attrs.get("thumbnail"), theirs.attrs.get("thumbnail"), "{name}");
        assert_eq!(ours.original_bytes(), theirs.original_bytes(), "{name}");
        assert_eq!(ours.status(), RowStatus::New);
    }
    assert_eq!(ours["Logo"].1.data_kind(), Some(DataKind::Picture));
    assert_eq!(ours["Blob"].1.data_kind(), Some(DataKind::Binary));
    // The original of binary data is read from the .resx (the project keeps a thumbnail only).
    let blob = p.rows().into_iter().find(|r| p.row(r).unwrap().id() == "FileBytes").unwrap();
    assert_eq!(ops::data_original(&p, &blob), Some((0..40).collect::<Vec<u8>>()));

    // Written like Sisulizer: hex inline, wrapped to column 78; binary rows as `<row …></row>`.
    let text = String::from_utf8(slp::to_bytes(&p)).unwrap();
    let theirs = std::fs::read_to_string(fixture().join("sisulizer").join("scan.slp")).unwrap();
    let logo = |s: &str| s[s.find("<row id=\"Logo\"").unwrap()..].split("</row>").next().unwrap().split_once('>').unwrap().1.to_string();
    assert_eq!(logo(&text), logo(&theirs));
    assert!(text.contains("thumbnail=\"0001020304050607...FDFEFF\"></row>"), "{text}");

    // A rescan without changes keeps everything.
    let again = ops::scan_project(&mut p, None).unwrap();
    assert_eq!((again.new, again.changed, again.unused), (0, 0, 0));
}

#[test]
fn build_matches_sisulizer() {
    let (dir, slp_path) = workspace("translated.slp");
    let p = slp::load(&slp_path).unwrap();
    let rep = ops::build_project(&p, &["de".into()], None);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    let entries = |path: &Path| -> BTreeMap<String, (Vec<(String, String)>, String)> {
        let doc = xml::parse_bytes(&std::fs::read(path).unwrap()).unwrap();
        doc.root
            .elements()
            .filter(|e| e.name == "data")
            .map(|e| {
                let attrs = e.attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
                (e.attr("name").unwrap().to_string(), (attrs, e.child("value").map(xml::Element::text).unwrap_or_default().replace("\r\n", "\n")))
            })
            .collect()
    };
    let ours = entries(&dir.path().join("Strings.de.resx"));
    let mut theirs = entries(&fixture().join("sisulizer").join("Strings.de.resx"));
    // Sisulizer forgets the mimetype of a former byte[] file reference; Susi writes it.
    theirs.get_mut("FileBytes").unwrap().0.push(("mimetype".into(), "application/x-microsoft.net.object.bytearray.base64".into()));
    assert_eq!(ours, theirs);
    assert_eq!(ours.keys().collect::<Vec<_>>(), ["Blob", "FileBytes", "FilePicture", "FileText", "Hello", "Logo"]);
}

#[test]
fn linked_files_and_writing() {
    let (dir, slp_path) = workspace("translated.slp");
    let mut p = slp::load(&slp_path).unwrap();
    let rows = p.rows();
    let find = |p: &Project, id: &str| rows.iter().find(|r| p.row(r).unwrap().id() == id).unwrap().clone();
    let (logo, icon) = (find(&p, "Logo"), find(&p, "AppIcon"));
    assert_eq!(p.row(&logo).unwrap().translation("de").unwrap().linked_file(), Some("de\\logo.png"));
    assert_eq!(custom_data::linked_files(&p).len(), 3);

    // Unchanged linked files leave the project alone (whatever their date); a changed one is read.
    assert!(custom_data::refresh_linked(&mut p).updated.is_empty());
    let new_logo = std::fs::read(dir.path().join("de").join("file.png")).unwrap();
    std::fs::write(dir.path().join("de").join("logo.png"), &new_logo).unwrap();
    let rep = custom_data::refresh_linked(&mut p);
    assert_eq!(rep.updated, [(logo.clone(), "de".to_string())]);
    assert_eq!(p.row(&logo).unwrap().translation_bytes("de"), Some(new_logo.clone()));
    std::fs::remove_file(dir.path().join("de").join("data.bin")).unwrap();
    assert_eq!(custom_data::refresh_linked(&mut p).missing.len(), 1);
    let opts = susi_core::validate::ValidateOptions { kinds: vec![susi_core::validate::ValidationKind::Data], langs: Vec::new() };
    let messages: Vec<String> = susi_core::validate::validate(&p, &opts, None).into_iter().map(|i| format!("{}: {}", i.context, i.message)).collect();
    assert_eq!(
        messages,
        ["FileBytes/FileBytes: Linked file de\\data.bin not found.", "Logo/Logo: The translated picture (PNG 24 × 12) differs from the original (PNG 16 × 8).",]
    );

    // Loading a translation into an inline picture row moves the original into <native>.
    custom_data::load_translation_file(&mut p, &icon, "de", &dir.path().join("de").join("logo.png"), true).unwrap();
    let row = p.row(&icon).unwrap();
    assert_eq!((row.tstatus("de"), row.translation("de").unwrap().linked_file()), (TranslationStatus::Translated, Some("de\\logo.png")));
    slp::save(&mut p, &slp_path).unwrap();
    let text = std::fs::read_to_string(&slp_path).unwrap();
    let block = text[text.find("<row id=\"AppIcon\"").unwrap()..].split("</row>").next().unwrap().to_string();
    let lines: Vec<&str> = block.lines().collect();
    assert_eq!(lines[1].trim(), "<native>");
    assert_eq!(lines[2].len(), 78, "{block}");
    assert!(lines[2].starts_with("          000001000100"), "{block}");
    let again = slp::load(&slp_path).unwrap();
    assert_eq!(again.row(&icon).unwrap().translation_bytes("de"), Some(new_logo));
    assert_eq!(again.row(&icon).unwrap().original_bytes(), p.row(&icon).unwrap().original_bytes());
    assert!(custom_data::unlink_translation(&mut p, &icon, "de"));
}

#[test]
fn excluded_data_types() {
    let (dir, _) = workspace("scan.slp");
    let mut p = Project::new();
    p.path = Some(dir.path().join("new.slp"));
    p.add_language("en");
    ops::add_source(&mut p, &dir.path().join("Strings.resx"), "en").unwrap();
    p.sources[0].set_excluded_data(Excluded { images: true, text: true, ..Default::default() });
    let rep = ops::scan_project(&mut p, None).unwrap();
    assert_eq!(rep.unused, 6, "five pictures and the text file reference");
    assert_eq!(p.remove_unused_rows(), 6);
    let nodes = data_nodes(&p);
    assert_eq!(nodes.keys().collect::<Vec<_>>(), ["Blob", "FileBytes"]);
    assert!(nodes.values().all(|(class, _)| class == BINARY_NODE));
    assert!(!nodes.values().any(|(class, _)| class == PICTURE_NODE));
    assert_eq!(texts(&p).keys().collect::<Vec<_>>(), ["Hello"]);
    assert_eq!(p.sources[0].attrs.get("excludeddatatypes"), Some("[xdtImage,xdtText]"));
}
