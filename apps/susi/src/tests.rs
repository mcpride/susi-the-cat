//! Headless rendering tests: run frames of the full UI against a real project.

use std::path::Path;

use susi_core::i18n::Lang;
use susi_core::settings::Settings;
use susi_core::slp;

use crate::actions::Action;
use crate::app::{BottomTab, SusiApp};
use crate::dialogs::{Dialog, ExportDialog, ImportDialog, PseudoDialog, TmDialog};

fn run(ctx: &egui::Context, app: &mut SusiApp, frames: usize) {
    for _ in 0..frames {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 860.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
    }
}

fn app_with_fixture(ctx: &egui::Context, tmp: &Path, ui_language: &str) -> SusiApp {
    let slp_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/demo-ui/susi/demo.slp");
    let settings = Settings { tm_path: Some(tmp.join("tm.tmx")), ui_language: ui_language.into(), ..Settings::default() };
    let mut app = SusiApp::with_settings(ctx, settings, None);
    let mut project = slp::load(&slp_path).unwrap();
    // Without a path the project is never saved and the settings (recent files) are not written.
    project.path = None;
    app.set_project(project);
    app
}

#[test]
fn app_icon_decodes() {
    let icon = crate::app_icon::window_icon();
    assert_eq!((icon.width, icon.height, icon.rgba.len()), (256, 256, 256 * 256 * 4));
}

#[test]
fn renders_all_views_and_dialogs() {
    render_all_views_and_dialogs("en");
}

#[test]
fn renders_all_views_and_dialogs_in_german() {
    render_all_views_and_dialogs("de");
}

fn render_all_views_and_dialogs(ui_language: &str) {
    let tmp = std::env::temp_dir().join(format!("susi-gui-test-{ui_language}-{}", std::process::id()));
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &tmp, ui_language);
    run(&ctx, &mut app, 2);
    assert!(!app.rows.is_empty());
    eprintln!("rendered {} rows", app.rows.len());
    let first = app.rows[0].clone();
    app.select(first.clone());

    for all_langs in [true, false] {
        app.all_langs = all_langs;
        for tab in BottomTab::ALL {
            app.tab = tab;
            run(&ctx, &mut app, 2);
        }
    }

    app.actions.push(Action::Validate { missing: true });
    app.actions.push(Action::Statistics);
    app.actions.push(Action::RowProperties);
    run(&ctx, &mut app, 2);
    assert!(app.dialog.is_some(), "row properties dialog should be open");

    let lang = app.project.as_ref().and_then(|p| p.language_ids().first().cloned()).unwrap_or_default();
    app.start_cell_edit(first, lang);
    run(&ctx, &mut app, 2);
    app.commit_cell_edit();

    let dialogs = [
        Dialog::About,
        Dialog::Languages { add: String::new(), remove: None },
        Dialog::AddSource { files: vec![tmp.join("x.json")], original: "en".into() },
        Dialog::RemoveSource(0),
        Dialog::Confirm { then: Action::Exit },
        Dialog::TmTranslate(TmDialog { overwrite: false, fuzzy: true, only_view: false, only_lang: false }),
        Dialog::Pseudo(PseudoDialog { lang: "qps".into(), options: app.settings.pseudo.clone(), overwrite: false }),
        Dialog::Settings(Box::new(app.settings.clone())),
        Dialog::Export(ExportDialog {
            path: tmp.join("export.xlsx"),
            langs: vec![("de".into(), true)],
            only_view: false,
            status_columns: true,
            comment_columns: true,
            row_status_column: true,
            max_chars_column: true,
            include_locked: true,
        }),
        Dialog::Import(ImportDialog {
            path: tmp.join("import.xlsx"),
            overwrite: false,
            check_original: true,
            comments: true,
            status: susi_core::status::TranslationStatus::Translated,
        }),
    ];
    for d in dialogs {
        app.dialog = Some(d);
        run(&ctx, &mut app, 2);
        assert!(app.dialog.is_some(), "dialog closed without user interaction");
    }
    app.actions.push(Action::SourceProperties(0));
    run(&ctx, &mut app, 2);

    assert!(!app.dirty, "rendering alone must not modify the project");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Every symbol used in the UI sources must exist in egui's built-in fonts, otherwise it renders as an empty box.
#[test]
fn all_ui_glyphs_exist_in_default_fonts() {
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    let sources = [
        include_str!("panels.rs"),
        include_str!("grid.rs"),
        include_str!("bottom.rs"),
        include_str!("dialogs.rs"),
        include_str!("app.rs"),
        include_str!("actions.rs"),
    ];
    // Translations are shown in the same fonts.
    let i18n = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/core/src/i18n");
    let mut catalogs = Vec::new();
    for e in std::fs::read_dir(&i18n).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "tsv") {
            catalogs.push(std::fs::read_to_string(e.path()).unwrap());
        }
    }
    assert!(!catalogs.is_empty(), "no catalogs in {}", i18n.display());
    let font = egui::FontId::proportional(14.0);
    let mut missing: Vec<char> = sources
        .iter()
        .copied()
        .chain(catalogs.iter().map(String::as_str))
        .flat_map(str::chars)
        .filter(|c| !c.is_ascii() && !c.is_whitespace())
        .filter(|&c| !ctx.fonts_mut(|f| f.has_glyph(&font, c)))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    let list: Vec<String> = missing.iter().map(|c| format!("{c} U+{:04X}", u32::from(*c))).collect();
    assert!(missing.is_empty(), "glyphs missing in default fonts: {}", list.join(", "));
}

/// Changing the interface language takes effect immediately (texts are translated at render time).
#[test]
fn switches_ui_language() {
    let ctx = egui::Context::default();
    let settings = Settings { ui_language: "en".into(), ..Settings::default() };
    let mut app = SusiApp::with_settings(&ctx, settings, None);
    run(&ctx, &mut app, 1);
    assert_eq!(app.ui_lang, Lang::EN);
    assert_eq!(BottomTab::Output.label(), "Output");

    app.settings.ui_language = "de".into();
    app.apply_ui_language();
    run(&ctx, &mut app, 1);
    assert_eq!(app.ui_lang, Lang::DE);
    assert_eq!(BottomTab::Output.label(), "Ausgabe");
    assert_eq!(BottomTab::Statistics.label(), "Statistik");

    app.settings.ui_language = "en".into();
    app.apply_ui_language();
    run(&ctx, &mut app, 1);
    assert_eq!(BottomTab::Output.label(), "Output");
}
/// The sheet with all languages is wider than the window and must scroll horizontally.
#[test]
fn sheet_scrolls_horizontally() {
    let tmp = std::env::temp_dir().join(format!("susi-gui-hscroll-{}", std::process::id()));
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &tmp, "en");
    app.all_langs = true;
    run(&ctx, &mut app, 2);
    let pos = egui::pos2(700.0, 400.0);
    let wheel = |delta: egui::Vec2| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..10 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 860.0))),
            events: vec![egui::Event::PointerMoved(pos), wheel(egui::vec2(-300.0, 0.0))],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
    }
    run(&ctx, &mut app, 5);
    assert!(app.grid_offset.x > 100.0, "sheet did not scroll horizontally: {:?}", app.grid_offset);
    let _ = std::fs::remove_dir_all(&tmp);
}
