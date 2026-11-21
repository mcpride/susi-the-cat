//! End-to-end tests of the SlMake compatible CLI.

use std::path::{Path, PathBuf};

use susi_core::exchange;

struct Run {
    code: i32,
    out: String,
    err: String,
}

/// Runs with English output (the default `auto` would follow the OS locale).
fn run(args: &[&str]) -> Run {
    run_raw(&[args, &["-uilang:en"]].concat())
}

fn run_raw(args: &[&str]) -> Run {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = susi_cli::run_with(&args, &mut out, &mut err);
    Run { code, out: String::from_utf8_lossy(&out).into(), err: String::from_utf8_lossy(&err).into() }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

const RESX: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <data name="Greeting" xml:space="preserve">
    <value>Hello</value>
    <comment>Shown on start</comment>
  </data>
</root>
"#;

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("en.json"), r#"{"menu":{"open":"Open {0}","close":"Close"},"title":"Hello."}"#).unwrap();
    std::fs::write(dir.path().join("Strings.resx"), RESX).unwrap();
    let slp = dir.path().join("app.slp");
    let r = run(&["add", &s(&dir.path().join("en.json")), &s(&dir.path().join("*.resx")), "-origlang:en", "-lang:de;fr", &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    (dir, slp)
}

/// Fills the German column of an exported CSV with `f(original)` and writes it back.
fn translate_csv(path: &Path, f: impl Fn(&str) -> String) {
    let mut t = exchange::read_delimited(&std::fs::read(path).unwrap(), None).unwrap();
    let orig = t.headers.iter().position(|h| h.starts_with("Original")).unwrap();
    let de = t.headers.iter().position(|h| h.ends_with("[de]")).unwrap();
    for row in &mut t.rows {
        row[de] = f(&row[orig]);
    }
    std::fs::write(path, exchange::write_delimited(&t, ';')).unwrap();
}

#[test]
fn full_pipeline() {
    let (dir, slp) = fixture();
    let d = dir.path();

    let r = run(&["info", &s(&slp)]);
    assert_eq!(r.code, 0);
    assert!(r.out.contains("Strings.resx") && r.out.contains("en.json"), "{}", r.out);

    // Export for the translator, translate in "Excel", import.
    let csv = d.join("de.csv");
    let r = run(&["export", &s(&csv), "-lang:de", "-status:untranslated", &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    translate_csv(&csv, |o| match o {
        "Open {0}" => "Öffnen".into(),
        "Close" => "Schließen".into(),
        "Hello." => "Hallo.".into(),
        "Hello" => "Hallo".into(),
        _ => String::new(),
    });
    let r = run(&["import", &s(&csv), "-lang:de", &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("4 translations imported"), "{}", r.out);

    // The missing placeholder is a validation error: result code 2 with -w.
    let r = run(&["validate", "-lang:de", &s(&slp), "-w"]);
    assert_eq!(r.code, 2, "{}{}", r.out, r.err);
    assert!(r.err.contains("{0}") || r.err.contains("menu.open"), "{}", r.err);

    // The same as a JUnit report for GitLab CI (directories are created): one failed test.
    let junit = d.join("reports").join("susi-junit.xml");
    let junit_arg = format!("-junit:{}", s(&junit));
    let r = run(&["validate", "-lang:de", &junit_arg, &s(&slp)]);
    assert!(r.code == 1 && r.out.contains("Report created"), "{}{}", r.out, r.err);
    let xml = std::fs::read_to_string(&junit).unwrap();
    assert_eq!(xml.matches("<testcase ").count(), 1, "{xml}");
    assert!(xml.contains("<failure type=\"format\"") && xml.contains("menu") && xml.contains("[de]"), "{xml}");

    // Fix it via a second import (overwrite is the default).
    translate_csv(&csv, |o| if o == "Open {0}" { "{0} öffnen".into() } else { String::new() });
    let r = run(&["import", &s(&csv), "-lang:de", &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let r = run(&["validate", "-lang:de", &junit_arg, &s(&slp), "-w"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    // Without findings the report is written anyway, without test cases.
    let xml = std::fs::read_to_string(&junit).unwrap();
    assert!(xml.contains("tests=\"0\"") && !xml.contains("<testcase"), "{xml}");

    // Review export of today's translations; date options also restrict validate and report.
    let review = d.join("review.csv");
    let r = run(&["export", &s(&review), "-lang:de", "-since:today", "-datetype:translated", &s(&slp)]);
    assert!(r.code == 0 && r.out.contains("4 rows match the date filter"), "{}{}", r.out, r.err);
    assert_eq!(exchange::read_delimited(&std::fs::read(&review).unwrap(), None).unwrap().rows.len(), 4);
    let r = run(&["validate", "-all", "-until:yesterday", &s(&slp), "-w"]);
    assert!(r.code == 0 && r.out.contains("0 rows match the date filter"), "{}{}", r.out, r.err);
    let r = run(&["report", "-since:soon", &s(&slp)]);
    assert!(r.code == 1 && r.err.contains("-since"), "{}", r.err);

    // Build (SlMake "create") writes de\en.json and Strings.de.resx.
    let r = run(&["build", "-lang:de", &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let json = std::fs::read_to_string(d.join("de").join("en.json")).unwrap();
    assert!(json.contains("{0} öffnen") && json.contains("Schließen"), "{json}");
    let resx = std::fs::read_to_string(d.join("Strings.de.resx")).unwrap();
    assert!(resx.contains("<value>Hallo</value>"), "{resx}");

    // TM: save, then translate a second project from it.
    let tmx = d.join("tm.tmx");
    let r = run(&["savetotm", &format!("-tm:{}", s(&tmx)), &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let other = d.join("other.slp");
    let r = run(&["add", &s(&d.join("en.json")), "-lang:de", &s(&other)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let r = run(&["translate", &format!("--tm={}", s(&tmx)), &s(&other)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("3 translated exactly"), "{}", r.out);

    // Pseudo translation and report; several tasks in one call.
    let report = d.join("report.html");
    let r = run(&["fillpseudo", "-lang:ja", "report", &s(&report), &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(std::fs::read_to_string(&report).unwrap().contains("<html"));
    let p = susi_core::slp::load(&slp).unwrap();
    assert!(p.has_language("ja"));
    let r = run(&["removepseudo", "-lang:ja", "-q", &s(&slp)]);
    assert_eq!((r.code, r.out.as_str()), (0, ""));
}

#[test]
fn errors_and_result_codes() {
    let (dir, slp) = fixture();
    let r = run(&["export", &s(&dir.path().join("x.xlsx")), "-lang:xx", &s(&slp)]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("\"xx\""), "{}", r.err);

    let r = run(&["scan", &s(&dir.path().join("missing.slp"))]);
    assert_eq!(r.code, 1);
    let r = run(&["exchange", "a.sle", &s(&slp)]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("export"), "{}", r.err);

    // Untranslated rows are warnings only with -all: result code 1 with -w, 0 without.
    let r = run(&["validate", "-all", &s(&slp), "-w"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    let r = run(&["validate", "-all", &s(&slp)]);
    assert_eq!(r.code, 0);

    // In the JUnit report, warnings are failed tests only with -w (as for the result code).
    let junit = dir.path().join("junit.xml");
    let junit_arg = format!("--junit={}", s(&junit));
    let r = run(&["validate", "-all", &junit_arg, &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(!std::fs::read_to_string(&junit).unwrap().contains("<testcase"));
    let r = run(&["validate", "-all", &junit_arg, &s(&slp), "-w"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(std::fs::read_to_string(&junit).unwrap().contains("<failure type=\"missing\""));
    let r = run(&["validate", "-junit", &s(&slp)]);
    assert!(r.code == 1 && r.err.contains("-junit"), "{}", r.err);

    // -nosave keeps the project unchanged.
    let before = std::fs::read(&slp).unwrap();
    std::fs::write(dir.path().join("en.json"), r#"{"menu":{"open":"Open","close":"Close"}}"#).unwrap();
    let r = run(&["scan", "-nosave", &s(&slp)]);
    assert_eq!(r.code, 0);
    assert_eq!(std::fs::read(&slp).unwrap(), before);
    let r = run(&["scan", &s(&slp)]);
    assert!(r.out.contains("1 changed") && r.out.contains("1 unused"), "{}", r.out);

    let r = run(&["export", "-h"]);
    assert!(r.code == 0 && r.out.contains("-status"));
}

#[test]
fn ui_language() {
    let r = run_raw(&["-h", "-uilang:de"]);
    assert_eq!(r.code, 0);
    assert!(r.out.contains("Stiller Modus") && r.out.contains("-uilang:X"), "{}", r.out);
    let r = run_raw(&["-h", "-uilang:en"]);
    assert!(r.out.contains("Quiet mode") && r.out.contains("Options:"), "{}", r.out);

    // Parse errors are localized too, whatever the position of -uilang.
    let r = run_raw(&["-uilang:de", "export", "-bogus", "a.slp"]);
    assert_eq!(r.code, 1);
    assert!(r.err.starts_with("Fehler: Unbekannte Option -bogus"), "{}", r.err);
    let r = run_raw(&["scan", "missing.slp", "--uilang=en"]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("Error: Project file missing.slp not found"), "{}", r.err);
    let r = run_raw(&["info", "-uilang:xx", "a.slp"]);
    assert!(r.code == 1 && r.err.contains("-uilang"), "{}", r.err);
}

#[test]
fn real_project_info() {
    let slp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/demo-ui/susi/demo.slp");
    let before = std::fs::read(&slp).unwrap();
    let r = run(&["info", &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("Languages:"), "{}", r.out);
    assert_eq!(std::fs::read(&slp).unwrap(), before, "read-only task must not save");
}

#[test]
fn screenshots() {
    use susi_core::screenshots::{ScreenshotStore, solid_png};
    let (dir, slp) = fixture();
    let before = std::fs::read(&slp).unwrap();
    let ui = dir.path().join("ui-tests");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("menu.png"), solid_png(300, 200, [9, 9, 9])).unwrap();
    std::fs::write(ui.join("title.png"), solid_png(100, 50, [1, 1, 1])).unwrap();

    let r = run(&["screenshot", "add", &s(&ui.join("menu.png")), "-key:*/menu.*", "-title:Menu", "-area:10,10,80,20", &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("Screenshot added: menu") && r.out.contains("2 rows linked"), "{}", r.out);
    let manifest = ui.join("manifest.json");
    let json = r#"{"screenshots": [{"id": "start", "file": "title.png", "title": "Start",
        "links": [{"key": "title", "area": [1, 1, 50, 10]}, {"key": "nothing"}]}]}"#;
    std::fs::write(&manifest, json).unwrap();
    let r = run(&["screenshot", "import", &s(&manifest), &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    assert!(r.out.contains("1 screenshots added, 0 replaced, 0 removed, 2 links") && r.out.contains("1 link does not match any row"), "{}", r.out);
    assert_eq!(std::fs::read(&slp).unwrap(), before, "screenshots never change the project file");

    let r = run(&["screenshot", "list", &s(&slp)]);
    assert!(r.out.contains("menu  \"Menu\"  300 × 200  2 links") && r.out.contains("(no matching row)"), "{}", r.out);
    assert!(r.out.contains("2 screenshots, 1 orphaned link"), "{}", r.out);
    assert!(run(&["info", &s(&slp)]).out.contains("Screenshots: 2 screenshots, 1 orphaned link"));

    // Export with embedded screenshots; the re-import ignores them.
    let xlsx = dir.path().join("t.xlsx");
    let r = run(&["export", &s(&xlsx), "-lang:de", "-screenshots:embed", &s(&slp)]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let t = exchange::read_xlsx(&xlsx).unwrap();
    assert_eq!(t.headers.last().map(String::as_str), Some("Screenshot"));
    assert!(t.rows.iter().any(|r| r.last().is_some_and(|c| c == "Menu (#2)")), "{:?}", t.rows);
    let r = run(&["import", &s(&xlsx), "-lang:de", &s(&slp)]);
    assert_eq!((r.code, r.err.as_str()), (0, ""), "{}", r.out);
    let csv = dir.path().join("t.csv");
    assert_eq!(run(&["export", &s(&csv), "-lang:de", "-screenshots:link", &s(&slp)]).code, 0);
    assert_eq!(std::fs::read_dir(dir.path().join("t_screenshots")).unwrap().count(), 2);
    assert_ne!(run(&["export", &s(&csv), "-screenshots:pictures", &s(&slp)]).code, 0);

    let r = run(&["screenshot", "prune", &s(&slp)]);
    assert!(r.out.contains("1 orphaned links removed, 0 unused image files deleted"), "{}", r.out);
    std::fs::remove_file(ScreenshotStore::open(&slp).unwrap().image_path("menu").unwrap()).unwrap();
    let r = run(&["validate", "-check:screenshot", "-w", &s(&slp)]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.out.matches("Missing screenshot").count() == 2, "{}", r.out);
    let r = run(&["screenshot", "remove", "menu", "nope", &s(&slp)]);
    assert!(r.out.contains("Screenshot removed: menu") && r.err.contains("Screenshot not found: nope"), "{}{}", r.out, r.err);
    assert!(run(&["screenshot", "frobnicate", &s(&slp)]).err.contains("Unknown screenshot action"));
}

#[test]
fn copy_languages() {
    let (dir, slp) = fixture();
    let text = |key: &str, lang: &str| {
        let p = susi_core::slp::load(&slp).unwrap();
        p.rows().into_iter().find(|r| p.row(r).unwrap().id().ends_with(key)).and_then(|r| p.row(&r).unwrap().text(lang).map(str::to_string))
    };
    let r = run(&["copy", "-from:de", "-lang:fr", &s(&slp)]);
    assert!(r.code == 0 && r.out.contains("0 cells copied from German (de) to French (fr)"), "{}{}", r.out, r.err);

    let csv = dir.path().join("de.csv");
    assert_eq!(run(&["export", &s(&csv), "-lang:de", &s(&slp)]).code, 0);
    translate_csv(&csv, |o| format!("DE {o}"));
    assert_eq!(run(&["import", &s(&csv), "-lang:de", &s(&slp)]).code, 0);
    let r = run(&["copy", "-from:de", "-lang:fr", &s(&slp)]);
    assert!(r.code == 0 && r.out.contains("4 cells copied from German (de) to French (fr)"), "{}{}", r.out, r.err);
    assert_eq!(text("Greeting", "fr").as_deref(), Some("DE Hello"));

    // Existing translations are kept unless -overwrite is given; sources and the original as source.
    let r = run(&["copy", "-lang:fr", &s(&slp)]);
    assert!(r.out.contains("0 cells copied from the original") && r.out.contains("4 cells with a translation were kept"), "{}", r.out);
    let r = run(&["copy", "-lang:fr", "-overwrite", "-include:Strings.resx", &s(&slp)]);
    assert!(r.code == 0 && r.out.contains("1 cell copied from the original to French (fr)"), "{}{}", r.out, r.err);
    assert_eq!(text("Greeting", "fr").as_deref(), Some("Hello"));
    assert_eq!(text("title", "fr").as_deref(), Some("DE Hello."));

    let project = s(&slp);
    for (args, error) in [
        (["copy", "-from:de", "-overwrite"], "-lang missing"),
        (["copy", "-from:fr", "-lang:de;fr"], "must not contain the same language"),
        (["copy", "-from:xx", "-lang:fr"], "does not contain the language \"xx\""),
    ] {
        let r = run(&[&args[..], &[project.as_str()]].concat());
        assert!(r.code == 1 && r.err.contains(error), "{args:?}: {}{}", r.out, r.err);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Pictures and binary data of a RESX file (fixture with Sisulizer's projects).
#[test]
fn custom_data() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/custom-data"), d);
    let r = run(&["add", &s(&d.join("Strings.resx")), "-origlang:en", "-lang:de", &s(&d.join("new.slp"))]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);
    let r = run(&["info", &s(&d.join("new.slp"))]);
    assert!(r.out.contains("Pictures and binary data: 5 pictures, 2 binary rows, 0 linked files"), "{}", r.out);

    // Sisulizer's translated project: build, then a changed linked file is read again.
    let slp = d.join("app.slp");
    std::fs::copy(d.join("sisulizer").join("translated.slp"), &slp).unwrap();
    let r = run(&["create", "-lang:de", &s(&slp)]);
    assert!(r.code == 0 && r.out.contains("Strings.de.resx"), "{}{}", r.out, r.err);
    assert!(std::fs::read_to_string(d.join("Strings.de.resx")).unwrap().contains("<data name=\"Logo\""));
    std::fs::copy(d.join("de").join("file.png"), d.join("de").join("logo.png")).unwrap();
    std::fs::remove_file(d.join("de").join("data.bin")).unwrap();
    let r = run(&["create", "-lang:de", &s(&slp), "-w"]);
    assert_eq!(r.code, 1, "{}{}", r.out, r.err);
    assert!(r.out.contains("Translation read from its changed linked file: Logo/Logo (de)"), "{}", r.out);
    assert!(r.out.contains("Linked file") && r.out.contains("data.bin not found"), "{}", r.out);
    let r = run(&["validate", "-check:data", &s(&slp)]);
    assert!(r.out.contains("The translated picture (PNG 24 × 12) differs from the original (PNG 16 × 8)."), "{}{}", r.out, r.err);
}
