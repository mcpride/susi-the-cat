//! Bottom area: output log, validation results, search, statistics, comments and text editor.

use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use susi_core::filter::RowFilter;
use susi_core::i18n::{t, trn};
use susi_core::model::RowRef;
use susi_core::status::TranslationStatus;
use susi_core::tm::TmMatch;
use susi_core::validate::Severity;

use crate::actions::Action;
use crate::app::{BottomTab, LogKind, Nav, SusiApp};
use crate::panels::{SEARCH_IN, search_in_label, status_color};

pub fn show(app: &mut SusiApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        for t in BottomTab::ALL {
            let label = match t {
                BottomTab::Validation if !app.issues.is_empty() => format!("{} ({})", t.label(), app.issues.len()),
                BottomTab::Search if !app.search.results.is_empty() => format!("{} ({})", t.label(), app.search.results.len()),
                _ => t.label().to_string(),
            };
            ui.selectable_value(&mut app.tab, t, label);
        }
    });
    ui.separator();
    match app.tab {
        BottomTab::Output => output(app, ui),
        BottomTab::Validation => validation(app, ui),
        BottomTab::Search => search(app, ui),
        BottomTab::Statistics => statistics(app, ui),
        BottomTab::Comments => comments(app, ui),
        BottomTab::Editor => editor(app, ui),
    }
}

fn log_color(k: LogKind) -> Option<Color32> {
    match k {
        LogKind::Info => None,
        LogKind::Warning => Some(Color32::from_rgb(220, 140, 0)),
        LogKind::Error => Some(Color32::from_rgb(220, 40, 40)),
    }
}

fn small_label(ui: &mut Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).selectable(false).truncate());
}

fn output(app: &mut SusiApp, ui: &mut Ui) {
    if ui.small_button(t("Clear")).clicked() {
        app.log.clear();
    }
    TableBuilder::new(ui)
        .id_salt("log")
        .striped(true)
        .stick_to_bottom(true)
        .auto_shrink(false)
        .column(Column::exact(70.0))
        .column(Column::exact(90.0))
        .column(Column::remainder().clip(true))
        .header(18.0, |mut h| {
            h.col(|ui| {
                ui.strong(t("Time"));
            });
            h.col(|ui| {
                ui.strong(t("Type"));
            });
            h.col(|ui| {
                ui.strong(t("Message"));
            });
        })
        .body(|body| {
            body.rows(18.0, app.log.len(), |mut row| {
                let e = &app.log[row.index()];
                row.col(|ui| small_label(ui, e.time.as_str()));
                row.col(|ui| {
                    let t = RichText::new(e.kind.label());
                    small_label(ui, if let Some(c) = log_color(e.kind) { t.color(c) } else { t });
                });
                row.col(|ui| small_label(ui, e.message.as_str()));
            });
        });
}

fn validation(app: &mut SusiApp, ui: &mut Ui) {
    if app.issues.is_empty() {
        ui.label(t("No validation results. Validation › Validate starts a validation."));
        return;
    }
    let mut clicked: Option<(RowRef, String)> = None;
    TableBuilder::new(ui)
        .id_salt("issues")
        .striped(true)
        .auto_shrink(false)
        .sense(egui::Sense::click())
        .column(Column::exact(70.0))
        .column(Column::initial(220.0).clip(true))
        .column(Column::exact(70.0))
        .column(Column::initial(260.0).clip(true))
        .column(Column::remainder().clip(true))
        .header(18.0, |mut h| {
            for title in [t("Type"), t("Check"), t("Language"), t("Context"), t("Message")] {
                h.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(18.0, app.issues.len(), |mut row| {
                let is = &app.issues[row.index()];
                row.set_selected(app.selected.as_ref() == Some(&is.row) && app.lang.as_deref() == Some(is.lang.as_str()));
                let (label, color) = match is.severity {
                    Severity::Error => (t("Error"), Color32::from_rgb(220, 40, 40)),
                    Severity::Warning => (t("Warning"), Color32::from_rgb(220, 140, 0)),
                };
                row.col(|ui| small_label(ui, RichText::new(label).color(color)));
                row.col(|ui| small_label(ui, is.kind.label()));
                row.col(|ui| small_label(ui, is.lang.as_str()));
                row.col(|ui| small_label(ui, format!("{}: {}", is.source, is.context)));
                row.col(|ui| small_label(ui, is.message.as_str()));
                if row.response().clicked() {
                    clicked = Some((is.row.clone(), is.lang.clone()));
                }
            });
        });
    if let Some((r, l)) = clicked {
        app.select(r);
        app.lang = Some(l);
    }
}

/// Clickable list of rows (search results, comments).
fn row_list(app: &SusiApp, ui: &mut Ui, id: &str, rows: &[RowRef], third: &str, third_text: impl Fn(&susi_core::Row) -> String) -> Option<RowRef> {
    let p = app.project.as_ref()?;
    let mut clicked = None;
    TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .auto_shrink(false)
        .sense(egui::Sense::click())
        .column(Column::initial(260.0).clip(true))
        .column(Column::initial(320.0).clip(true))
        .column(Column::remainder().clip(true))
        .header(18.0, |mut h| {
            for title in [t("Context"), t("Original"), third] {
                h.col(|ui| {
                    ui.strong(title);
                });
            }
        })
        .body(|body| {
            body.rows(18.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let Some(data) = p.row(r) else {
                    return;
                };
                row.set_selected(app.selected.as_ref() == Some(r));
                row.col(|ui| small_label(ui, p.context(r)));
                row.col(|ui| small_label(ui, data.original().replace('\n', " ¶ ")));
                row.col(|ui| small_label(ui, third_text(data).replace('\n', " ¶ ")));
                if row.response().clicked() {
                    clicked = Some(r.clone());
                }
            });
        });
    clicked
}

fn search(app: &mut SusiApp, ui: &mut Ui) {
    let mut run = false;
    ui.horizontal(|ui| {
        ui.label(t("Find what:"));
        let resp = ui.add(egui::TextEdit::singleline(&mut app.search.text).desired_width(260.0));
        if app.search.focus {
            resp.request_focus();
            app.search.focus = false;
        }
        run |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        egui::ComboBox::from_id_salt("search-in").selected_text(search_in_label(app.search.search_in)).show_ui(ui, |ui| {
            for s in SEARCH_IN {
                ui.selectable_value(&mut app.search.search_in, s, search_in_label(s));
            }
        });
        ui.checkbox(&mut app.search.case_sensitive, t("Match case"));
        run |= ui.button(t("Search")).clicked();
        if ui.button(t("Use as filter")).on_hover_text(t("Copy the search text to the filter bar")).clicked() {
            app.filter.text = app.search.text.clone();
            app.filter.search_in = app.search.search_in;
            app.filter.case_sensitive = app.search.case_sensitive;
            app.rows_stale = true;
        }
    });
    if run && let Some(p) = &app.project {
        let f = RowFilter {
            text: app.search.text.clone(),
            search_in: app.search.search_in,
            case_sensitive: app.search.case_sensitive,
            include_excluded: true,
            ..Default::default()
        };
        app.search.results = if app.search.text.is_empty() { Vec::new() } else { f.apply(p) };
        let msg = susi_core::tf!("Search \"{text}\": {hits}", text = app.search.text, hits = trn(app.search.results.len(), "{n} hit", "{n} hits"));
        app.info(msg);
    }
    let lang = app.lang.clone().unwrap_or_default();
    let rows = app.search.results.clone();
    if let Some(r) = row_list(app, ui, "search-results", &rows, t("Translation"), |row| row.text(&lang).unwrap_or("").to_string()) {
        app.select(r);
    }
}

fn statistics(app: &mut SusiApp, ui: &mut Ui) {
    if app.stats.is_none()
        && let Some(p) = &app.project
    {
        app.stats = Some(susi_core::stats::statistics(p, None));
    }
    let Some(s) = &app.stats else {
        ui.label(t("No project open"));
        return;
    };
    ui.label(susi_core::tf!(
        "{rows} rows, {words} words, {chars} characters, {locked} \"Do not translate\"",
        rows = s.rows,
        words = s.words,
        chars = s.characters,
        locked = s.locked
    ));
    ui.horizontal_wrapped(|ui| {
        for (k, v) in &s.row_status {
            ui.label(format!("{k}: {v}"));
            ui.separator();
        }
    });
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("stats").striped(true).num_columns(9).show(ui, |ui| {
            ui.strong(t("Language"));
            ui.strong(t("Progress"));
            for st in TranslationStatus::ALL {
                ui.label(RichText::new(st.label()).color(status_color(st)).strong());
            }
            ui.strong(t("Open words"));
            ui.end_row();
            for l in &s.languages {
                ui.label(format!("{} ({})", l.name, l.lang));
                ui.add(egui::ProgressBar::new((l.percent() / 100.0) as f32).desired_width(140.0).text(format!("{:.1} %", l.percent())));
                for n in l.status {
                    ui.label(n.to_string());
                }
                ui.label(l.words[0].to_string());
                ui.end_row();
            }
        });
    });
}

fn comments(app: &mut SusiApp, ui: &mut Ui) {
    let lang = app.lang.clone().unwrap_or_default();
    let rows: Vec<RowRef> = match &app.project {
        Some(p) => app
            .rows
            .iter()
            .filter(|r| p.row(r).is_some_and(|row| !row.note().is_empty() || row.translation(&lang).is_some_and(|t| !t.comment().is_empty())))
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    if rows.is_empty() {
        ui.label(t("No comments in the current view."));
        return;
    }
    let text = |row: &susi_core::Row| {
        let tc = row.translation(&lang).map(|t| t.comment()).unwrap_or("");
        match (row.note().is_empty(), tc.is_empty()) {
            (false, false) => format!("{} | {lang}: {tc}", row.note()),
            (false, true) => row.note().to_string(),
            _ => format!("{lang}: {tc}"),
        }
    };
    if let Some(r) = row_list(app, ui, "comments", &rows, t("Comment"), text) {
        app.select(r);
    }
}

/// Editor buffers, reloaded from the project every frame and written back on change.
struct EditorBuf {
    row: RowRef,
    lang: String,
    original: String,
    text: String,
    note: String,
    comment: String,
    has_translation: bool,
    locked: bool,
    max_chars: usize,
}

fn editor_buf(app: &SusiApp) -> Option<EditorBuf> {
    let row = app.selected.clone()?;
    let lang = app.lang.clone()?;
    let r = app.project.as_ref()?.row(&row)?;
    let t = r.translation(&lang);
    Some(EditorBuf {
        original: r.original().to_string(),
        text: t.map(|t| t.text.clone()).unwrap_or_default(),
        note: r.note().to_string(),
        comment: t.map(|t| t.comment().to_string()).unwrap_or_default(),
        has_translation: t.is_some(),
        locked: r.locked(),
        max_chars: r.max_chars(),
        row,
        lang,
    })
}

fn editor(app: &mut SusiApp, ui: &mut Ui) {
    let Some(mut b) = editor_buf(app) else {
        ui.label(t("No row or language selected."));
        return;
    };
    let mut changed_text = false;
    let mut changed_note = false;
    let mut changed_comment = false;
    let mut take: Option<String> = None;
    let suggestions = suggestions(app, &b);
    ui.columns(3, |cols| {
        cols[0].label(
            RichText::new(susi_core::tf!("Original ({lang})", lang = app.project.as_ref().map(|p| p.original_lang(&b.row).to_string()).unwrap_or_default()))
                .strong(),
        );
        let mut orig = b.original.clone();
        cols[0].add(egui::TextEdit::multiline(&mut orig).desired_rows(3).desired_width(f32::INFINITY).interactive(false));
        cols[0].label(RichText::new(susi_core::tf!("Translation – {lang}", lang = SusiApp::lang_label(&b.lang))).strong());
        let edit = egui::TextEdit::multiline(&mut b.text).desired_rows(3).desired_width(f32::INFINITY);
        changed_text = cols[0].add_enabled(!b.locked, edit).changed();
        if b.max_chars > 0 {
            let n = b.text.chars().count();
            let color = if n > b.max_chars { Color32::RED } else { cols[0].visuals().weak_text_color() };
            cols[0].label(RichText::new(susi_core::tf!("{n} / {max} characters", n = n, max = b.max_chars)).color(color));
        }
        cols[1].label(RichText::new(t("Row comment")).strong());
        changed_note = cols[1].add(egui::TextEdit::multiline(&mut b.note).desired_rows(3).desired_width(f32::INFINITY)).changed();
        cols[1].label(RichText::new(t("Translation comment")).strong());
        changed_comment =
            cols[1].add_enabled(b.has_translation, egui::TextEdit::multiline(&mut b.comment).desired_rows(3).desired_width(f32::INFINITY)).changed();
        editor_side(app, &mut cols[2], &b, &suggestions, &mut take);
    });
    if changed_text {
        app.cell_edit = None;
        app.write_translation(&b.row, &b.lang, &b.text);
    }
    if let Some(t) = take {
        app.cell_edit = None;
        app.write_translation(&b.row, &b.lang, &t);
    }
    if changed_note && let Some(r) = app.project.as_mut().and_then(|p| p.row_mut(&b.row)) {
        r.set_note(&b.note);
        app.touched();
    }
    if changed_comment && let Some(t) = app.project.as_mut().and_then(|p| p.row_mut(&b.row)).and_then(|r| r.translation_mut(&b.lang)) {
        t.set_comment(&b.comment);
        app.touched();
    }
}

fn editor_side(app: &mut SusiApp, ui: &mut Ui, b: &EditorBuf, suggestions: &[TmMatch], take: &mut Option<String>) {
    ui.horizontal(|ui| {
        for (t, n) in [("⏮", Nav::First), ("◀", Nav::Prev), ("▶", Nav::Next), ("⏭", Nav::Last)] {
            if ui.button(t).clicked() {
                app.actions.push(Action::Nav(n));
            }
        }
    });
    let current = app.project.as_ref().and_then(|p| p.row(&b.row)).map(|r| r.tstatus(&b.lang));
    ui.horizontal_wrapped(|ui| {
        for s in TranslationStatus::ALL {
            let on = current == Some(s);
            if ui.selectable_label(on, RichText::new(format!("■ {}", s.label())).color(status_color(s))).clicked() {
                app.actions.push(Action::SetStatus(s));
            }
        }
    });
    if let Some(p) = &app.project
        && let Some(r) = p.row(&b.row)
    {
        egui::Grid::new("props").num_columns(2).show(ui, |ui| {
            ui.label(t("Row status:"));
            ui.label(r.status().label());
            ui.end_row();
            ui.label(t("Context:"));
            ui.label(p.context(&b.row));
            ui.end_row();
            ui.label(t("Source:"));
            ui.label(p.sources.get(b.row.source).map(|s| s.name()).unwrap_or(""));
            ui.end_row();
            ui.label(t("State:"));
            ui.label(if r.locked() {
                t("Do not translate")
            } else if r.excluded() {
                t("Excluded")
            } else {
                t("Translatable")
            });
            ui.end_row();
        });
    }
    if !suggestions.is_empty() {
        ui.separator();
        ui.label(RichText::new(t("Translation memory")).strong());
        for m in suggestions {
            ui.horizontal(|ui| {
                if ui.small_button(t("Apply")).clicked() {
                    *take = Some(m.text.clone());
                }
                ui.label(format!("{} % – {}", m.score, m.text.replace('\n', " ¶ ")));
            });
        }
    }
}

/// TM suggestions for the edited cell (cached per row, language and original text).
fn suggestions(app: &mut SusiApp, b: &EditorBuf) -> Vec<TmMatch> {
    let key = (b.row.clone(), b.lang.clone(), b.original.clone());
    if let Some((k, v)) = &app.suggestions
        && *k == key
    {
        return v.clone();
    }
    let src_lang = app.project.as_ref().map(|p| p.original_lang(&b.row).to_string()).unwrap_or_default();
    let min = app.settings.fuzzy_threshold;
    let v = match app.ensure_tm() {
        Some(tm) => tm.fuzzy(&src_lang, &b.original, &b.lang, min, 5),
        None => Vec::new(),
    };
    app.suggestions = Some((key, v.clone()));
    v
}
