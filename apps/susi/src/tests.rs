//! Headless rendering tests: run frames of the full UI against a real project.

use std::path::Path;

use susi_core::i18n::Lang;
use susi_core::settings::Settings;
use susi_core::slp;

use susi_core::ops::CopyTargets;

use crate::actions::{Action, Scope};
use crate::app::{BottomTab, SusiApp};
use crate::dialogs::{ClearColumnDialog, CopyDialog, Dialog, ExportDialog, ImportDialog, PseudoDialog, TmDialog};
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

/// Column commands act on the visible rows of one language column, here not the current language.
#[test]
fn column_commands() {
    use susi_core::status::TranslationStatus;
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &std::env::temp_dir(), "en");
    run(&ctx, &mut app, 2);
    let current = app.lang.clone().unwrap();
    let lang = app.project.as_ref().unwrap().language_ids().into_iter().find(|l| *l != current).unwrap();
    let rows = app.rows.clone();
    let cells = |app: &SusiApp, l: &str| -> Vec<Option<String>> {
        let p = app.project.as_ref().unwrap();
        rows.iter().map(|r| p.row(r).unwrap().text(l).map(str::to_string)).collect()
    };
    let others = cells(&app, &current);
    assert!(cells(&app, &lang).iter().any(Option::is_some), "fixture needs translations in {lang}");

    // Clearing asks first.
    app.actions.push(Action::ClearColumn(Some(lang.clone())));
    run(&ctx, &mut app, 1);
    let Some(Dialog::ClearColumn(d)) = app.dialog.take() else { panic!("no confirmation") };
    assert!(!app.dirty);
    app.clear_column(&d);
    assert!(cells(&app, &lang).iter().all(Option::is_none));
    assert_eq!(cells(&app, &current), others);
    assert!(app.dirty);

    // Copying the original fills the empty cells; then nothing is left to copy.
    app.actions.push(Action::CopyInColumn { from: None, lang: Some(lang.clone()) });
    run(&ctx, &mut app, 1);
    let Some(Dialog::Copy(d)) = app.dialog.take() else { panic!("no copy dialog") };
    assert_eq!(d.targets.translated, 0);
    app.copy_in_column(&d);
    let p = app.project.as_ref().unwrap();
    for r in &rows {
        let row = p.row(r).unwrap();
        let copied = !row.locked() && !row.excluded() && !row.raw && !row.original().is_empty();
        assert_eq!(row.text(&lang), copied.then(|| row.original()), "{}", row.id());
    }
    app.actions.push(Action::CopyInColumn { from: None, lang: Some(lang.clone()) });
    run(&ctx, &mut app, 1);
    assert!(app.dialog.is_none());

    // Copying a language into the selected cell and, overwriting, into the current column.
    let editable = |row: &susi_core::model::Row| !row.locked() && !row.excluded() && !row.raw && row.text(&current).is_some();
    let p = app.project.as_ref().unwrap();
    let r0 = rows.iter().find(|r| p.row(r).is_some_and(|row| editable(row) && row.text(&current) != row.text(&lang))).unwrap().clone();
    app.select(r0.clone());
    app.lang = Some(lang.clone());
    app.actions.push(Action::CopyIntoCell(Some(current.clone())));
    run(&ctx, &mut app, 1);
    let row = app.project.as_ref().unwrap().row(&r0).unwrap();
    assert_eq!(row.text(&lang), row.text(&current));
    app.actions.push(Action::CopyInColumn { from: Some(current.clone()), lang: None });
    run(&ctx, &mut app, 1);
    let Some(Dialog::Copy(mut d)) = app.dialog.take() else { panic!("no copy dialog") };
    assert_eq!((d.from.as_deref(), d.lang.as_str()), (Some(current.as_str()), lang.as_str()));
    assert!(d.targets.translated > 0);
    d.overwrite = true;
    app.copy_in_column(&d);
    let p = app.project.as_ref().unwrap();
    assert!(rows.iter().map(|r| p.row(r).unwrap()).filter(|row| editable(row)).all(|row| row.text(&lang) == row.text(&current)));
    app.lang = Some(current.clone());

    app.actions.push(Action::SetStatusIn(Scope::View, Some(lang.clone()), TranslationStatus::Complete));
    run(&ctx, &mut app, 1);
    let p = app.project.as_ref().unwrap();
    assert!(rows.iter().map(|r| p.row(r).unwrap()).filter(|row| row.text(&lang).is_some()).all(|row| row.tstatus(&lang) == TranslationStatus::Complete));
    assert_eq!(cells(&app, &current), others);
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
        Dialog::Copy(CopyDialog { from: None, lang: "de".into(), rows: app.rows.clone(), targets: CopyTargets { empty: 2, translated: 5 }, overwrite: true }),
        Dialog::Copy(CopyDialog {
            from: Some("en".into()),
            lang: "de".into(),
            rows: app.rows.clone(),
            targets: CopyTargets { empty: 0, translated: 0 },
            overwrite: false,
        }),
        Dialog::ClearColumn(ClearColumnDialog { lang: "de".into(), rows: app.rows.clone() }),
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
            label_lang: "en".into(),
            screenshots: susi_core::exchange::ScreenshotExport::Embed,
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
            github: false,
        }),
        Dialog::WorkflowPublish(PublishDialog {
            title: "Translations".into(),
            description: String::new(),
            reviewers: "octo/translators".into(),
            target: "main".into(),
            squash: false,
            remove_source_branch: false,
            advanced: true,
            save: None,
            build: false,
            branches: vec!["main".into()],
            github: true,
        }),
        Dialog::WorkflowToken(TokenDialog { server: "https://gitlab.example.com (group/app)".into(), token: String::new(), has_token: true, github: false }),
        Dialog::WorkflowToken(TokenDialog { server: "https://github.com (octo/app)".into(), token: String::new(), has_token: false, github: true }),
        Dialog::Screenshots,
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

#[test]
fn screenshots_of_rows() {
    use crate::shots::ShotCmd;
    use susi_core::screenshots::{Area, Link, ScreenshotStore, solid_png};
    let tmp = std::env::temp_dir().join(format!("susi-gui-shots-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let ctx = egui::Context::default();
    let mut app = app_with_fixture(&ctx, &tmp, "en");
    app.refresh_rows();
    let (first, second) = (app.rows[0].clone(), app.rows[1].clone());
    // A project without a file has no screenshots.
    app.paste_image(&egui::ColorImage::new([2, 2], vec![egui::Color32::RED; 4]));
    assert!(app.shots.is_none() && !app.dirty);

    // Give the project a file (it is never saved here) with a screenshot linked to the first row.
    let slp = tmp.join("demo.slp");
    let mut store = ScreenshotStore::new(&slp);
    store.add_bytes(&solid_png(320, 200, [200, 210, 220]), Some("main"), Some("Main window")).unwrap();
    let p = app.project.as_mut().unwrap();
    p.path = Some(slp.clone());
    store.link("main", [Link::to_row(p, &first, Area::parse("20,30,100,20"))]).unwrap();
    store.save().unwrap();
    app.load_screenshots();
    assert!(app.shot_index.has(&first) && !app.shot_index.has(&second));

    app.select(first.clone());
    app.tab = BottomTab::Screenshots;
    app.show_bottom = true;
    run(&ctx, &mut app, 3);
    assert_eq!(app.shot_ui.texture_count(), 1, "image shown");

    app.filter.screenshot = Some(true);
    app.refresh_rows();
    assert_eq!(app.rows, std::slice::from_ref(&first));
    app.filter.screenshot = Some(false);
    app.refresh_rows();
    assert!(!app.rows.contains(&first) && app.rows.contains(&second));
    app.filter = crate::app::FilterState::default();
    app.refresh_rows();

    // A marked area replaces the stored one.
    app.actions.push(Action::Screenshot(ShotCmd::SetArea("main".into(), Area::parse("1,2,30,10"))));
    run(&ctx, &mut app, 1);
    assert!(app.dirty);
    let shots = app.shots.as_ref().unwrap();
    assert_eq!(shots.links_of(&app.shot_index, &first)[0].1.area, Area::parse("1,2,30,10"));

    // Pasted images and added files are linked to the selected row.
    app.select(second.clone());
    app.paste_image(&egui::ColorImage::new([4, 3], vec![egui::Color32::BLUE; 12]));
    let file = tmp.join("dialog.png");
    std::fs::write(&file, solid_png(10, 10, [0, 0, 0])).unwrap();
    app.add_screenshot_files(&[file], std::slice::from_ref(&first));
    let shots = app.shots.as_ref().unwrap();
    assert_eq!(shots.len(), 3);
    assert_eq!((shots.links_of(&app.shot_index, &second).len(), shots.links_of(&app.shot_index, &first).len()), (1, 2));
    assert_eq!(app.tab, BottomTab::Screenshots);

    app.dialog = Some(Dialog::Screenshots);
    run(&ctx, &mut app, 2);
    assert!(app.dialog.is_some());
    // Removing the screenshot also removes its marker.
    app.actions.push(Action::Screenshot(ShotCmd::Remove("main".into())));
    run(&ctx, &mut app, 1);
    assert_eq!(app.shots.as_ref().unwrap().links_of(&app.shot_index, &first).len(), 1);
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
        include_str!("shots.rs"),
        include_str!("data_ui.rs"),
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

/// Pictures and binary data: cell texts, the editor tab with previews, loading, linking and
/// unlinking files, linked files read again on scan, the source option.
#[test]
fn custom_data_rows() {
    use crate::data_ui::{self, DataCmd};
    let tmp = std::env::temp_dir().join(format!("susi-gui-custom-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/custom-data"), &tmp);
    let slp_path = tmp.join("app.slp");
    std::fs::copy(tmp.join("sisulizer").join("translated.slp"), &slp_path).unwrap();
    let ctx = egui::Context::default();
    let settings = Settings { tm_path: Some(tmp.join("tm.tmx")), ui_language: "en".into(), ..Settings::default() };
    let mut app = SusiApp::with_settings(&ctx, settings, None);
    let mut project = slp::load(&slp_path).unwrap();
    project.path = None;
    app.set_project(project);
    // Linked files are relative to the project (set after set_project, which records recent files).
    app.project.as_mut().unwrap().path = Some(slp_path.clone());
    run(&ctx, &mut app, 2);
    let find = |app: &SusiApp, id: &str| {
        let p = app.project.as_ref().unwrap();
        p.rows().into_iter().find(|r| p.row(r).unwrap().id() == id).unwrap()
    };
    let (logo, icon, bytes) = (find(&app, "Logo"), find(&app, "AppIcon"), find(&app, "FileBytes"));
    let row = |app: &SusiApp, r: &susi_core::model::RowRef| app.project.as_ref().unwrap().row(r).unwrap().clone();
    assert_eq!(data_ui::cell_text(&row(&app, &logo), None), "PNG 16 × 8");
    assert_eq!(data_ui::cell_text(&row(&app, &logo), Some("de")), "🔗 PNG 16 × 8");
    assert_eq!(data_ui::cell_text(&row(&app, &bytes), None), "Binary data");

    // Editing a data cell opens the editor tab with both pictures; binary data as hex dump.
    app.select(logo.clone());
    app.lang = Some("de".into());
    app.actions.push(Action::EditCell);
    run(&ctx, &mut app, 2);
    assert!(app.cell_edit.is_none());
    assert!(app.show_bottom && app.tab == BottomTab::Editor);
    assert_eq!(app.data_ui.texture_count(), 2);
    app.select(bytes);
    run(&ctx, &mut app, 2);

    // Loading a linked file, removing the link; a changed linked file is read again on scan.
    app.select(icon.clone());
    data_ui::load_file(&mut app, &icon, "de", &tmp.join("de").join("logo.png"), true);
    assert!(app.dirty);
    assert_eq!(row(&app, &icon).translation("de").unwrap().linked_file(), Some("de\\logo.png"));
    app.actions.push(Action::Data(DataCmd::Unlink));
    run(&ctx, &mut app, 1);
    assert_eq!(row(&app, &icon).translation("de").unwrap().linked_file(), None);
    let new_logo = std::fs::read(tmp.join("de").join("file.png")).unwrap();
    std::fs::write(tmp.join("de").join("logo.png"), &new_logo).unwrap();
    app.actions.push(Action::Scan);
    run(&ctx, &mut app, 2);
    assert_eq!(row(&app, &logo).translation_bytes("de"), Some(new_logo));
    assert!(app.log.iter().any(|l| l.message.contains("Translation read from its changed linked file: Logo/Logo (de)")));

    // The source properties of a .resx offer the excluded data types.
    app.actions.push(Action::SourceProperties(0));
    run(&ctx, &mut app, 2);
    assert!(matches!(&app.dialog, Some(Dialog::SourceProps(d)) if d.excluded.is_some()));
    app.dialog = None;
    let _ = std::fs::remove_dir_all(&tmp);
}
