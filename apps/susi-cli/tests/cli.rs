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

    // Fix it via a second import (overwrite is the default).
    translate_csv(&csv, |o| if o == "Open {0}" { "{0} öffnen".into() } else { String::new() });
    let r = run(&["import", &s(&csv), "-lang:de", &s(&slp)]);
    assert_eq!(r.code, 0, "{}", r.err);
    let r = run(&["validate", "-lang:de", &s(&slp), "-w"]);
    assert_eq!(r.code, 0, "{}{}", r.out, r.err);

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
