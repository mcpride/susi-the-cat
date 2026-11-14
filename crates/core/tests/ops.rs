//! Scan/build against copies of the project fixtures in `testdata/projects`.

use std::path::{Path, PathBuf};

use susi_core::{ops, slp};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let t = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

/// Copies all JSON projects: `demo-ui` references a source of `demo-widget`.
fn projects_copy() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["demo-ui", "demo-settings", "demo-widget"] {
        copy_dir(&testdata().join("projects").join(name), &tmp.path().join(name));
    }
    tmp
}

#[test]
fn json_project_scan_and_build() {
    let tmp = projects_copy();
    let root = tmp.path().join("demo-ui");
    let mut p = slp::load(&root.join("susi/demo.slp")).unwrap();
    let before = slp::to_bytes(&p);

    let rep = ops::scan_project(&mut p, None).unwrap();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!((rep.files, rep.new, rep.changed, rep.unused), (2, 0, 0, 0));
    assert_eq!(rep.total, 210);

    let i18n = root.join("src/assets/i18n");
    std::fs::remove_file(i18n.join("de_v5.json")).unwrap();
    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.written.len() + b.unchanged.len(), 32, "16 languages x 2 sources");
    assert!(b.written.contains(&ops::normalize(&i18n.join("de_v5.json"))));

    let de = std::fs::read(i18n.join("de_v5.json")).unwrap();
    assert!(de.starts_with(b"{\r\n  \""), "style of the original must be kept");
    let v: serde_json::Value = serde_json::from_slice(&de).unwrap();
    let r = p.find_row("sis.json/RAVUBI.ETOJA").unwrap();
    let expected = p.row(&r).unwrap().text("de").unwrap();
    assert_eq!(v["RAVUBI"]["ETOJA"], expected);

    // Scanning an unchanged source must not alter rows (only scan counters and the file date,
    // which differs because of the checkout).
    for (src, orig) in p.sources.iter_mut().zip(slp::parse(&before).unwrap().sources) {
        src.attrs.set("date", orig.attrs.get("date").unwrap().to_string());
    }
    let after = String::from_utf8(slp::to_bytes(&p)).unwrap();
    let strip = |s: &str| s.lines().skip(2).collect::<Vec<_>>().join("\n");
    assert_eq!(strip(&after), strip(&String::from_utf8(before).unwrap()));
}

/// Scanning must find exactly the differences Sisulizer finds: `demo-ui` is in sync with its sources,
/// `demo-settings` lacks two keys added to its source after the last scan, and `demo-widget` is an old
/// project whose source has been rewritten since.
#[test]
fn json_projects_scan_like_sisulizer() {
    let tmp = projects_copy();
    for (name, expected) in [("demo-ui", (2, 0, 0, 0)), ("demo-settings", (1, 2, 0, 0)), ("demo-widget", (1, 22, 0, 4))] {
        let mut p = slp::load(&tmp.path().join(name).join("susi/demo.slp")).unwrap();
        let rep = ops::scan_project(&mut p, None).unwrap();
        assert!(rep.errors.is_empty(), "{name}: {:?}", rep.errors);
        assert_eq!((rep.files, rep.new, rep.changed, rep.unused), expected, "{name}");
        let b = ops::build_project(&p, &[], None);
        assert!(b.errors.is_empty(), "{name}: {:?}", b.errors);
    }
}

#[test]
fn resx_build_matches_sisulizer_output() {
    let dir = testdata().join("projects/wpf-resources");
    let tmp = tempfile::tempdir().unwrap();
    for f in ["Resources.slp", "Resources.resx"] {
        std::fs::copy(dir.join(f), tmp.path().join(f)).unwrap();
    }
    let p = slp::load(&tmp.path().join("Resources.slp")).unwrap();
    let b = ops::build_project(&p, &["de".to_string()], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let ours = std::fs::read(tmp.path().join("Resources.de.resx")).unwrap();
    let theirs = std::fs::read(dir.join("Resources.de.resx")).unwrap();
    let a = susi_core::formats::resx::scan(&ours).unwrap();
    let b = susi_core::formats::resx::scan(&theirs).unwrap();
    assert_eq!(a, b);
    let s = String::from_utf8_lossy(&ours);
    let t = String::from_utf8_lossy(&theirs);
    assert_eq!(s.matches("<data ").count(), t.matches("<data ").count(), "non-string data must be removed");
}

#[test]
fn add_source_creates_node_and_rows() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("i18n")).unwrap();
    std::fs::write(tmp.path().join("i18n/en.json"), "{\n  \"A\": {\n    \"B\": \"Hello\"\n  }\n}\n").unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("prj/test.slp"));
    p.add_language("de");
    let (si, rep) = ops::add_source(&mut p, &tmp.path().join("i18n/en.json"), "en").unwrap();
    assert_eq!(rep.new, 1);
    assert_eq!(p.sources[si].name(), "..\\i18n\\en.json");
    let r = p.find_row("en.json/A.B").unwrap();
    p.row_mut(&r).unwrap().set_translation("de", "Hallo", susi_core::TranslationStatus::Translated);
    p.sources[si].set_output_pattern(".", "<iso>.<ext>");
    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let out = std::fs::read_to_string(tmp.path().join("i18n/de.json")).unwrap();
    assert_eq!(out, "{\n  \"A\": {\n    \"B\": \"Hallo\"\n  }\n}\n");
    slp::save(&mut p, &tmp.path().join("prj/test.slp")).unwrap();
    let again = slp::load(&tmp.path().join("prj/test.slp")).unwrap();
    assert_eq!(again.rows().len(), 1);
}

#[test]
fn po_source_scan_build_and_mo() {
    let tmp = tempfile::tempdir().unwrap();
    let pot = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=CHARSET\\n\"\n\n#. Button\nmsgid \"Save\"\nmsgstr \"\"\n\nmsgid \"Quit\"\nmsgstr \"\"\n\nmsgid \"One file\"\nmsgid_plural \"%d files\"\nmsgstr[0] \"\"\nmsgstr[1] \"\"\n";
    std::fs::create_dir_all(tmp.path().join("locale")).unwrap();
    std::fs::write(tmp.path().join("locale/messages.pot"), pot).unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("app.slp"));
    p.add_language("de");
    let (si, rep) = ops::add_source(&mut p, &tmp.path().join("locale/messages.pot"), "en").unwrap();
    assert_eq!(p.sources[si].class(), "TPoSource");
    // 2 header fields + 2 messages + 3 plural rows (default).
    assert_eq!(rep.new, 7, "{:?}", rep.errors);
    let save = p.find_row("messages.pot/Save").unwrap();
    assert_eq!(p.row(&save).unwrap().note(), "Button");
    for (id, text) in [("Save", "Speichern"), ("One file[0]", "Eine Datei"), ("One file[1]", "%d Dateien")] {
        let r = p.find_row(&format!("messages.pot/{id}")).unwrap();
        p.row_mut(&r).unwrap().set_translation("de", text, susi_core::TranslationStatus::Translated);
    }
    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let de = tmp.path().join("locale/de/messages.po");
    assert_eq!(b.written, [ops::normalize(&de), ops::normalize(&de.with_extension("mo"))]);
    let po = std::fs::read_to_string(&de).unwrap();
    assert!(po.contains("msgid \"Save\"\nmsgstr \"Speichern\""), "{po}");
    assert!(po.contains("msgid \"Quit\"\nmsgstr \"\""), "{po}");
    assert!(po.contains("msgstr[1] \"%d Dateien\""), "{po}");
    assert!(po.contains("\"Language: de\\n\""), "{po}");
    let mo = std::fs::read(de.with_extension("mo")).unwrap();
    assert_eq!(mo[..4], 0x9504_12de_u32.to_le_bytes());
    assert_eq!(u32::from_le_bytes([mo[8], mo[9], mo[10], mo[11]]), 3, "header, Save and the plural message");
    // A second build changes nothing (deterministic revision date).
    let b = ops::build_project(&p, &[], None);
    assert_eq!((b.written.len(), b.unchanged.len()), (0, 2));
}

#[test]
fn tsv_catalog_is_its_own_localized_file() {
    let tmp = tempfile::tempdir().unwrap();
    let i18n = tmp.path().join("i18n");
    std::fs::create_dir_all(&i18n).unwrap();
    let de = "# German catalog\n\n# Menus\n\tOpen\tÖffnen\nmenu\tFile\tDatei\n\tQuit\t\n@plural\t{n} file|{n} files\t{n} Datei|{n} Dateien\n";
    std::fs::write(i18n.join("de-app.tsv"), de).unwrap();
    std::fs::write(i18n.join("fr-app.tsv"), "\tOpen\tOuvrir\n\tUnknown\tInconnu\n").unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("app.slp"));
    for l in ["en", "de", "fr"] {
        p.add_language(l);
    }
    let (si, rep) = ops::add_source(&mut p, &i18n.join("de-app.tsv"), "en").unwrap();
    assert_eq!(p.sources[si].class(), "TTsvSource");
    assert_eq!(p.sources[si].output_pattern().1, "<iso>-app.<ext>");
    assert_eq!((rep.new, rep.imported), (4, 4), "3 German + 1 French translation: {:?}", rep.errors);
    let file = p.find_row("de-app.tsv/menu|File").unwrap();
    assert_eq!(p.row(&file).unwrap().note(), "Menus\nContext: menu");
    assert_eq!(p.row(&file).unwrap().text("de"), Some("Datei"));

    // The German catalog is rebuilt in place and unchanged; the French one is completed.
    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!((b.written, b.unchanged), (vec![ops::normalize(&i18n.join("fr-app.tsv"))], vec![ops::normalize(&i18n.join("de-app.tsv"))]));
    let fr = std::fs::read_to_string(i18n.join("fr-app.tsv")).unwrap();
    assert_eq!(fr, "# German catalog\n\n# Menus\n\tOpen\tOuvrir\nmenu\tFile\t\n\tQuit\t\n@plural\t{n} file|{n} files\t\n");

    let quit = p.find_row("de-app.tsv/Quit").unwrap();
    p.row_mut(&quit).unwrap().set_translation("de", "Beenden", susi_core::TranslationStatus::Translated);
    let b = ops::build_project(&p, &["de".to_string()], None);
    assert!(b.errors.is_empty() && b.written.len() == 1, "{b:?}");
    assert_eq!(std::fs::read_to_string(i18n.join("de-app.tsv")).unwrap(), de.replace("\tQuit\t\n", "\tQuit\tBeenden\n"));
    // Rescanning finds no changes and imports nothing new.
    let rep = ops::scan_project(&mut p, None).unwrap();
    assert_eq!((rep.new, rep.changed, rep.unused, rep.imported), (0, 0, 0, 0), "{:?}", rep.errors);
}

#[test]
fn java_properties_import_and_build() {
    let tmp = tempfile::tempdir().unwrap();
    let res = tmp.path().join("res");
    std::fs::create_dir_all(&res).unwrap();
    let en = "# Main window\r\nwindow.title=Converter\r\n\r\n# Buttons\r\nbutton.ok = OK\r\nbutton.quit = Quit\r\nmessage=Converted {0} files\\n\\\r\n    successfully.\r\n";
    std::fs::write(res.join("Converter.properties"), en).unwrap();
    std::fs::write(res.join("Converter_de.properties"), "window.title=Konverter\nbutton.quit=Beenden\nobsolete=Alt\n").unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("app.slp"));
    p.add_language("de");
    p.add_language("pt-BR");
    let (si, rep) = ops::add_source(&mut p, &res.join("Converter.properties"), "en").unwrap();
    assert_eq!(p.sources[si].class(), "TJavaResourceSource");
    assert_eq!(p.sources[si].output_pattern().1, "<body>_<iso>.<ext>");
    assert_eq!((rep.new, rep.imported), (4, 2), "{:?}", rep.errors);
    // Rows are stored directly in the source, as Sisulizer does.
    assert!(p.sources[si].items.iter().all(|i| !matches!(i, susi_core::Item::Node(_))));
    let msg = p.find_row("message").unwrap();
    assert_eq!(p.row(&msg).unwrap().native.as_deref(), Some("Converted {0} files\nsuccessfully."));
    assert_eq!(p.row(&p.find_row("button.ok").unwrap()).unwrap().note(), "Buttons");
    p.row_mut(&msg).unwrap().set_translation("de", "{0} Dateien\nerfolgreich konvertiert.", susi_core::TranslationStatus::Translated);
    p.row_mut(&msg).unwrap().set_translation("pt-BR", "{0} arquivos convertidos", susi_core::TranslationStatus::Translated);

    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let de = std::fs::read_to_string(res.join("Converter_de.properties")).unwrap();
    assert_eq!(de, "# Main window\r\nwindow.title=Konverter\r\n\r\nbutton.quit = Beenden\r\nmessage={0} Dateien\\nerfolgreich konvertiert.\r\n");
    let pt = std::fs::read_to_string(res.join("Converter_pt_BR.properties")).unwrap();
    assert_eq!(pt, "\r\nmessage={0} arquivos convertidos\r\n");
    // Rescanning does not import again and finds no changes; the localized files are no sources.
    let rep = ops::scan_project(&mut p, None).unwrap();
    assert_eq!((rep.new, rep.changed, rep.unused, rep.imported), (0, 0, 0, 0), "{:?}", rep.errors);
}

#[test]
fn android_values_import_build_and_plurals() {
    let tmp = tempfile::tempdir().unwrap();
    let res = tmp.path().join("app").join("src").join("main").join("res");
    for d in ["values", "values-b+de"] {
        std::fs::create_dir_all(res.join(d)).unwrap();
    }
    let en = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<resources>\r\n    <!-- Title of the main screen -->\r\n    <string name=\"title\">Converter</string>\r\n    <string name=\"app_id\" translatable=\"false\">converter</string>\r\n    <string name=\"quit\">Don\\'t quit</string>\r\n    <dimen name=\"margin\">16dp</dimen>\r\n    <plurals name=\"files\">\r\n        <item quantity=\"one\">%d file</item>\r\n        <item quantity=\"other\">%d files</item>\r\n    </plurals>\r\n</resources>\r\n";
    std::fs::write(res.join("values").join("strings.xml"), en).unwrap();
    std::fs::write(
        res.join("values-b+de").join("strings.xml"),
        "<resources><string name=\"title\">Konverter</string><string name=\"obsolete\">Alt</string></resources>",
    )
    .unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("app.slp"));
    p.add_language("de");
    p.add_language("pl");
    let (si, rep) = ops::add_source(&mut p, &res.join("values").join("strings.xml"), "en").unwrap();
    assert_eq!(p.sources[si].class(), "TAndroidValuesSource");
    assert_eq!(p.sources[si].output_pattern().1, "<parent>\\values-b+<android>\\<file>");
    // title, quit and the plural quantities one/few/many/other (Polish needs few and many).
    assert_eq!((rep.new, rep.imported), (6, 1), "{:?}", rep.errors);
    // The rows are kept in the group node `string`, as Sisulizer does.
    assert!(matches!(&p.sources[si].items[1], susi_core::Item::Node(n) if n.name() == "string" && n.class() == "TAndroidValuesGroup"));
    let quit = p.find_row("quit").unwrap();
    assert_eq!(p.row(&quit).unwrap().original(), "Don't quit");
    assert_eq!(p.row(&p.find_row("title").unwrap()).unwrap().note(), "Title of the main screen");
    let set = |p: &mut susi_core::Project, id: &str, lang: &str, text: &str| {
        let r = p.find_row(id).unwrap();
        p.row_mut(&r).unwrap().set_translation(lang, text, susi_core::TranslationStatus::Translated);
    };
    set(&mut p, "quit", "de", "Nicht beenden");
    set(&mut p, "files[one]", "pl", "%d plik");
    set(&mut p, "files[few]", "pl", "%d pliki");
    set(&mut p, "files[many]", "pl", "%d plików");
    set(&mut p, "files[other]", "pl", "%d pliku");

    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let de = std::fs::read_to_string(res.join("values-b+de").join("strings.xml")).unwrap();
    assert_eq!(
        de,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<resources>\r\n    <!-- Title of the main screen -->\r\n    <string name=\"title\">Konverter</string>\r\n    <string name=\"quit\">Nicht beenden</string>\r\n</resources>\r\n"
    );
    let pl = std::fs::read_to_string(res.join("values-b+pl").join("strings.xml")).unwrap();
    assert!(pl.contains("<plurals name=\"files\">\r\n        <item quantity=\"one\">%d plik</item>\r\n        <item quantity=\"few\">%d pliki</item>\r\n        <item quantity=\"many\">%d plików</item>\r\n        <item quantity=\"other\">%d pliku</item>\r\n    </plurals>"), "{pl}");
    assert!(!pl.contains("title") && !pl.contains("dimen"), "{pl}");
    // Rescanning finds no changes; the localized files are no sources.
    let rep = ops::scan_project(&mut p, None).unwrap();
    assert_eq!((rep.new, rep.changed, rep.unused, rep.imported), (0, 0, 0, 0), "{:?}", rep.errors);
}

#[test]
fn android_keeps_legacy_resource_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let res = tmp.path().join("res");
    for d in ["values", "values-pt-rBR"] {
        std::fs::create_dir_all(res.join(d)).unwrap();
    }
    std::fs::write(res.join("values").join("strings.xml"), "<resources>\n    <string name=\"hello\">Hello</string>\n</resources>\n").unwrap();
    std::fs::write(res.join("values-pt-rBR").join("strings.xml"), "<resources>\n    <string name=\"hello\">Olá</string>\n</resources>\n").unwrap();
    let mut p = susi_core::Project::new();
    p.path = Some(tmp.path().join("app.slp"));
    p.add_language("pt-BR");
    p.add_language("zh");
    let (si, rep) = ops::add_source(&mut p, &res.join("values").join("strings.xml"), "en").unwrap();
    assert_eq!(p.sources[si].output_pattern().1, "<parent>\\values-<androidr>\\<file>");
    assert_eq!(rep.imported, 1, "{:?}", rep.errors);
    let hello = p.find_row("hello").unwrap();
    p.row_mut(&hello).unwrap().set_translation("zh", "你好", susi_core::TranslationStatus::Translated);
    let b = ops::build_project(&p, &[], None);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert!(res.join("values-zh-rCN").join("strings.xml").is_file());
    assert!(!res.join("values-b+pt+BR").exists());
}
