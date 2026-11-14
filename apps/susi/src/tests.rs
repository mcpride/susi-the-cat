//! Headless rendering tests: run frames of the full UI against a real project.

use std::path::Path;

use susi_core::i18n::Lang;
use susi_core::settings::Settings;
use susi_core::slp;

use crate::actions::Action;
use crate::app::{BottomTab, SusiApp};
use crate::dialogs::{Dialog, ExportDialog, ImportDialog, PseudoDialog, TmDialog};
use crate::workflow_ui::{FetchDialog, PublishDialog, SaveDialog, TokenDialog};

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
fn sheet_sorting_cycles_and_orders() {
    use crate::app::{SheetSort, SortColumn};
    use susi_core::status::TranslationStatus;
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &std::env::temp_dir(), "en");
    app.refresh_rows();
    let unsorted = app.rows.clone();
    let p = app.project.as_ref().unwrap();
    let mut contexts: Vec<String> = unsorted.iter().map(|r| p.context(r).to_lowercase()).collect();
    contexts.sort();

    let mut states = Vec::new();
    for _ in 0..3 {
        app.sort = SheetSort::next(app.sort, SortColumn::Context);
        app.refresh_rows();
        states.push(app.sort.map(|s| s.descending));
        let p = app.project.as_ref().unwrap();
        let got: Vec<String> = app.rows.iter().map(|r| p.context(r).to_lowercase()).collect();
        match app.sort {
            Some(s) if s.descending => assert_eq!(got, contexts.iter().rev().cloned().collect::<Vec<_>>()),
            Some(_) => assert_eq!(got, contexts),
            None => assert_eq!(app.rows, unsorted),
        }
    }
    assert_eq!(states, [Some(false), Some(true), None]);

    // Date: newest first, then oldest first; undated rows stay at the end. Switching columns restarts the cycle.
    app.all_langs = false;
    let lang = app.lang.clone().unwrap();
    let p = app.project.as_mut().unwrap();
    for (r, stamp) in unsorted.iter().zip(["20260101000000", "20260301000000", "20260201000000"]) {
        if p.original_lang(r) == lang {
            continue;
        }
        let row = p.row_mut(r).unwrap();
        row.set_translation(&lang, "x", TranslationStatus::Translated);
        row.translation_mut(&lang).unwrap().attrs.set("date", stamp);
    }
    for r in unsorted.iter().skip(3) {
        p.row_mut(r).unwrap().set_translation(&lang, "", TranslationStatus::NotTranslated);
    }
    app.sort = SheetSort::next(app.sort, SortColumn::Context);
    assert_eq!(SheetSort::next(app.sort, SortColumn::Date), Some(SheetSort { column: SortColumn::Date, descending: true }));
    for (descending, expected) in [(true, [1, 2, 0]), (false, [0, 2, 1])] {
        app.sort = Some(SheetSort { column: SortColumn::Date, descending });
        app.refresh_rows();
        let p = app.project.as_ref().unwrap();
        let dated: Vec<&susi_core::model::RowRef> = app.rows.iter().filter(|r| p.row(r).unwrap().translation_date(&lang).is_some()).collect();
        let want: Vec<&susi_core::model::RowRef> = expected.iter().map(|&i| &unsorted[i]).collect();
        assert_eq!(dated, want);
        assert!(app.rows[..3].iter().all(|r| p.row(r).unwrap().translation_date(&lang).is_some()));
    }
    assert_eq!(SheetSort::next(app.sort, SortColumn::Date), None);
}

/// The single-language view shows the selected language plus the additional languages, in project order.
#[test]
fn single_language_view_adds_extra_languages() {
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &std::env::temp_dir(), "en");
    let ids = app.project.as_ref().unwrap().language_ids();
    assert!(ids.len() >= 3, "fixture needs at least three languages: {ids:?}");
    assert_eq!(app.visible_langs(), ids);

    app.all_langs = false;
    app.settings.extra_langs = vec![ids[1].clone(), ids[0].clone(), "xx".into()];
    app.lang = Some(ids[2].clone());
    assert_eq!(app.visible_langs(), ids[..3]);
    app.lang = Some(ids[0].clone());
    assert_eq!(app.visible_langs(), ids[..2]);
    run(&ctx, &mut app, 2);

    app.all_langs = true;
    assert_eq!(app.visible_langs(), ids);
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
        Dialog::WorkflowFetch(FetchDialog {
            save_first: true,
            pending: true,
            advanced: true,
            branch: "main".into(),
            commit: String::new(),
            default_branch: "main".into(),
            branches: vec!["main".into(), "release".into()],
        }),
        Dialog::WorkflowSave(SaveDialog {
            message: "Translations".into(),
            prefix: "fix".into(),
            ci_skip: true,
            build: true,
            branch: String::new(),
            new_branch: true,
            files: vec!["l10n/app.slp".into()],
            advanced: true,
        }),
        Dialog::WorkflowPublish(PublishDialog {
            title: "Translations".into(),
            description: "Please review".into(),
            reviewers: "anna; ben".into(),
            target: "main".into(),
            squash: true,
            remove_source_branch: true,
            advanced: true,
            save: None,
            build: true,
            branches: vec!["main".into()],
        }),
        Dialog::WorkflowToken(TokenDialog { server: "https://gitlab.example.com (group/app)".into(), token: String::new(), has_token: true }),
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
        include_str!("workflow_ui.rs"),
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
/// Turns the mouse wheel horizontally over `pos` for a few frames.
fn wheel_left(ctx: &egui::Context, app: &mut SusiApp, pos: egui::Pos2) {
    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-300.0, 0.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::NONE,
    };
    for _ in 0..10 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 860.0))),
            events: vec![egui::Event::PointerMoved(pos), wheel.clone()],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
    }
    run(ctx, app, 5);
}

/// The sheet with all languages is wider than the window and must scroll horizontally.
#[test]
fn sheet_scrolls_horizontally() {
    let tmp = std::env::temp_dir().join(format!("susi-gui-hscroll-{}", std::process::id()));
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &tmp, "en");
    app.all_langs = true;
    run(&ctx, &mut app, 2);
    wheel_left(&ctx, &mut app, egui::pos2(700.0, 400.0));
    assert!(app.grid_offset.x > 100.0, "sheet did not scroll horizontally: {:?}", app.grid_offset);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Bottom panel lists fill the view by default and scroll horizontally once a column is widened.
#[test]
fn bottom_list_scrolls_horizontally() {
    let tmp = std::env::temp_dir().join(format!("susi-gui-list-hscroll-{}", std::process::id()));
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &tmp, "en");
    app.tab = BottomTab::Output;
    let pos = egui::pos2(700.0, 760.0);
    run(&ctx, &mut app, 2);
    wheel_left(&ctx, &mut app, pos);
    assert_eq!(app.list_offset.x, 0.0, "list wider than the view by default");
    ctx.data_mut(|d| d.insert_temp(crate::list::widths_id("log"), vec![None, None, Some(3000.0_f32)]));
    run(&ctx, &mut app, 2);
    wheel_left(&ctx, &mut app, pos);
    assert!(app.list_offset.x > 100.0, "list did not scroll horizontally: {:?}", app.list_offset);
    let _ = std::fs::remove_dir_all(&tmp);
}
