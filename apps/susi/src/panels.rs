//! Window layout: menu bar, toolbar, project tree, filter panel and status bar.

use egui::{Color32, RichText};
use susi_core::filter::SearchIn;
use susi_core::i18n::{Lang, t};
use susi_core::model::{Item, Node, NodeRef, Project};
use susi_core::status::{RowStatus, TranslationStatus};

use crate::actions::{Action, Scope};
use crate::app::{Nav, SusiApp};
use crate::data_ui::DataCmd;
use crate::shots::ShotCmd;

pub fn status_color(s: TranslationStatus) -> Color32 {
    let [r, g, b] = s.color();
    Color32::from_rgb(r, g, b)
}

pub fn search_in_label(s: SearchIn) -> &'static str {
    match s {
        SearchIn::All => t("All columns"),
        SearchIn::Original => t("Original"),
        SearchIn::Translation => t("Translation"),
        SearchIn::Id => t("Context"),
        SearchIn::Comment => t("Comment"),
    }
}

pub const SEARCH_IN: [SearchIn; 5] = [SearchIn::All, SearchIn::Original, SearchIn::Translation, SearchIn::Id, SearchIn::Comment];

pub fn show(app: &mut SusiApp, ui: &mut egui::Ui) {
    use crate::help::Area;
    egui::Panel::top("menu").show(ui, |ui| menu_bar(app, ui));
    egui::Panel::top("toolbar").show(ui, |ui| toolbar(app, ui));
    egui::Panel::bottom("statusbar").show(ui, |ui| status_bar(app, ui));
    // The part of the window under the mouse pointer is the context of F1.
    let mut area = Area::Window;
    // The default sizes only apply until the user drags a panel edge; the sizes are saved on exit.
    if app.show_bottom {
        let r = egui::Panel::bottom("bottom")
            .resizable(true)
            .default_size(app.panels.bottom)
            .min_size(110.0)
            .show(ui, |ui| fill(ui, |ui| crate::bottom::show(app, ui)));
        app.panels.bottom = r.response.rect.height();
        area = pointed(&r.response, Area::Output, area);
    }
    if app.show_tree {
        let r = egui::Panel::left("tree").resizable(true).default_size(app.panels.tree).min_size(140.0).show(ui, |ui| fill(ui, |ui| tree(app, ui)));
        app.panels.tree = r.response.rect.width();
        area = pointed(&r.response, Area::Tree, area);
    }
    if app.show_filter {
        let r =
            egui::Panel::right("filter").resizable(true).default_size(app.panels.filter).min_size(160.0).show(ui, |ui| fill(ui, |ui| filter_panel(app, ui)));
        app.panels.filter = r.response.rect.width();
        area = pointed(&r.response, Area::Filter, area);
    }
    let r = egui::CentralPanel::default().show(ui, |ui| crate::grid::show(app, ui));
    app.help_area = pointed(&r.response, Area::Sheet, area);
}

/// `area` if the mouse pointer is over the panel of `r`, otherwise `other`.
fn pointed(r: &egui::Response, area: crate::help::Area, other: crate::help::Area) -> crate::help::Area {
    if r.contains_pointer() { area } else { other }
}

/// Shows `add` in exactly the space the panel currently has, clipping what does not fit.
///
/// An egui panel takes the size of its content: a bottom tab with little content (an empty list,
/// a hint) would shrink the panel, a taller one (picture preview) grow it, so its height changed
/// with every tab switch. Here the panel keeps the size it was given or dragged to.
fn fill<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
    child.shrink_clip_rect(rect);
    let r = add(&mut child);
    ui.expand_to_include_rect(rect);
    r
}

/// Menu entry that queues `a` and closes the menu.
fn cmd(ui: &mut egui::Ui, acts: &mut Vec<Action>, text: &str, a: Action) {
    if ui.button(text).clicked() {
        acts.push(a);
        ui.close();
    }
}

fn cmd_key(ui: &mut egui::Ui, acts: &mut Vec<Action>, text: &str, key: &str, a: Action) {
    if ui.add(egui::Button::new(text).shortcut_text(key)).clicked() {
        acts.push(a);
        ui.close();
    }
}

fn menu_bar(app: &mut SusiApp, ui: &mut egui::Ui) {
    let mut acts = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        file_menu(app, ui, &mut acts);
        ui.menu_button(t("Edit"), |ui| {
            let langs = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
            copy_from_menu(ui, &mut acts, &langs, app.lang.as_deref(), Action::CopyIntoCell);
            cmd(ui, &mut acts, t("Clear translation"), Action::ClearTranslation);
            cmd_key(ui, &mut acts, t("Edit cell"), "F2", Action::EditCell);
            ui.separator();
            cmd(ui, &mut acts, t("Row properties / comment…"), Action::RowProperties);
        });
        ui.menu_button(t("Search"), |ui| {
            cmd_key(ui, &mut acts, t("Find…"), "Ctrl+F", Action::Find);
            cmd(ui, &mut acts, t("Reset filter"), Action::ResetFilter);
        });
        view_menu(app, ui);
        project_menu(ui, &mut acts);
        ui.menu_button(t("Validation"), |ui| {
            cmd(ui, &mut acts, t("Validate"), Action::Validate { missing: false });
            cmd(ui, &mut acts, t("Validate (including missing translations)"), Action::Validate { missing: true });
            cmd(ui, &mut acts, t("Clear validation results"), Action::ClearIssues);
        });
        column_menu(app, ui, &mut acts);
        row_menu(app, ui, &mut acts);
        ui.menu_button(t("Translation"), |ui| {
            cmd(ui, &mut acts, t("Translate with translation memory…"), Action::TmTranslate);
            cmd(ui, &mut acts, t("Save to translation memory"), Action::SaveToTm);
            cmd(ui, &mut acts, t("Translate duplicate strings"), Action::TranslateDuplicates);
            ui.separator();
            cmd(ui, &mut acts, t("Create pseudo translation…"), Action::FillPseudo);
            cmd(ui, &mut acts, t("Remove pseudo translations"), Action::RemovePseudo);
        });
        crate::workflow_ui::menu(app, ui, &mut acts);
        ui.menu_button(t("Tools"), |ui| {
            cmd(ui, &mut acts, t("Options…"), Action::Settings);
            ui.menu_button(t("User interface language"), |ui| ui_language_items(app, ui));
        });
        ui.menu_button(t("Help"), |ui| {
            if ui.button(t("User guide")).on_hover_text(t("F1 opens the help for the part of the window under the mouse pointer")).clicked() {
                acts.push(Action::UserGuide);
                ui.close();
            }
            ui.separator();
            cmd(ui, &mut acts, t("About Susi…"), Action::About);
        });
    });
    app.actions.extend(acts);
}

/// Radio entries for the user interface language ("auto" and every supported language by its native name).
pub fn ui_language_items(app: &mut SusiApp, ui: &mut egui::Ui) {
    if ui.radio(app.settings.ui_language == "auto", t("Automatic (system language)")).clicked() {
        app.set_ui_language("auto");
        ui.close();
    }
    for l in Lang::all() {
        if ui.radio(app.settings.ui_language == l.code(), l.name()).clicked() {
            app.set_ui_language(l.code());
            ui.close();
        }
    }
}

fn file_menu(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    ui.menu_button(t("File"), |ui| {
        cmd_key(ui, acts, t("New project…"), "Ctrl+N", Action::New);
        cmd_key(ui, acts, t("Open…"), "Ctrl+O", Action::Open);
        ui.menu_button(t("Recent projects"), |ui| {
            if app.settings.recent.is_empty() {
                ui.label(t("(none)"));
            }
            for p in &app.settings.recent {
                cmd(ui, acts, &p.display().to_string(), Action::OpenRecent(p.clone()));
            }
        });
        ui.separator();
        cmd_key(ui, acts, t("Save"), "Ctrl+S", Action::Save);
        cmd(ui, acts, t("Save as…"), Action::SaveAs);
        ui.separator();
        cmd(ui, acts, t("Export (Excel, CSV, XLIFF, TMX)…"), Action::Export);
        cmd(ui, acts, t("Import…"), Action::Import);
        ui.separator();
        cmd(ui, acts, t("Exit"), Action::Exit);
    });
}

fn view_menu(app: &mut SusiApp, ui: &mut egui::Ui) {
    ui.menu_button(t("View"), |ui| {
        if ui.radio(app.all_langs, t("All languages")).clicked() {
            app.all_langs = true;
            app.rows_stale = true;
        }
        if ui.radio(!app.all_langs, t("Single language")).clicked() {
            app.all_langs = false;
            app.rows_stale = true;
        }
        ui.menu_button(t("Additional languages"), |ui| extra_language_items(app, ui));
        ui.separator();
        ui.checkbox(&mut app.show_tree, t("Project tree"));
        ui.checkbox(&mut app.show_filter, t("Filter bar"));
        ui.checkbox(&mut app.show_bottom, t("Output area"));
        ui.separator();
        let mut changed = ui.checkbox(&mut app.settings.show_id_column, t("\"Context\" column")).changed();
        changed |= ui.checkbox(&mut app.settings.show_comment_column, t("\"Row comment\" column")).changed();
        changed |= ui.checkbox(&mut app.settings.show_date_column, t("\"Translated on\" column")).changed();
        if ui.checkbox(&mut app.settings.dark_mode, t("Dark theme")).changed() {
            app.apply_theme(ui.ctx());
            changed = true;
        }
        if changed {
            let _ = app.settings.save();
        }
        current_language_items(app, ui);
    });
}

/// Radio entries selecting the current language, after a separator (end of the menu "View").
fn current_language_items(app: &mut SusiApp, ui: &mut egui::Ui) {
    let ids = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    ui.separator();
    for id in ids {
        if ui.radio(app.lang.as_deref() == Some(id.as_str()), SusiApp::lang_label(&id)).clicked() {
            app.lang = Some(id);
            app.rows_stale = true;
            ui.close();
        }
    }
}

/// Checkboxes for the languages shown in the single-language view in addition to the selected one.
fn extra_language_items(app: &mut SusiApp, ui: &mut egui::Ui) {
    let ids = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
    if ids.is_empty() {
        ui.label(t("(none)"));
    }
    for id in ids {
        let mut on = app.settings.extra_langs.contains(&id);
        if ui.checkbox(&mut on, SusiApp::lang_label(&id)).on_hover_text(t("Also shown in the single-language view")).changed() {
            app.toggle_extra_lang(&id);
        }
    }
}

fn project_menu(ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    ui.menu_button(t("Project"), |ui| {
        cmd(ui, acts, t("Add source…"), Action::AddSource);
        cmd_key(ui, acts, t("Scan for changes"), "F5", Action::Scan);
        cmd_key(ui, acts, t("Build"), "F9", Action::Build);
        ui.separator();
        cmd(ui, acts, t("Languages…"), Action::Languages);
        cmd(ui, acts, t("Remove unused rows"), Action::RemoveUnused);
        cmd(ui, acts, t("Clear new status (all rows)"), Action::ClearNewIn(Scope::All));
        ui.separator();
        cmd(ui, acts, t("Screenshots…"), Action::Screenshot(ShotCmd::Manager));
        cmd(ui, acts, t("Import screenshot manifest…"), Action::Screenshot(ShotCmd::ImportManifest));
        ui.separator();
        cmd(ui, acts, t("Statistics"), Action::Statistics);
        cmd(ui, acts, t("Save report…"), Action::Report);
    });
}

fn column_menu(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    ui.menu_button(t("Column"), |ui| {
        let langs = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
        column_items(ui, acts, &langs, app.lang.as_deref());
    });
}

/// Commands for the visible rows of the language column `lang` (`None`: the current language), named
/// and ordered like the row commands; menu "Column" and the context menu of a column header.
/// `langs` are the project languages.
pub fn column_items(ui: &mut egui::Ui, acts: &mut Vec<Action>, langs: &[String], lang: Option<&str>) {
    let target = lang.map(str::to_string);
    ui.menu_button(t("Status"), |ui| status_buttons(ui, acts, |s| Action::SetStatusIn(Scope::View, target.clone(), s)));
    copy_from_menu(ui, acts, langs, lang, |from| Action::CopyInColumn { from, lang: target.clone() });
    cmd(ui, acts, t("Clear translation"), Action::ClearColumn(target.clone()));
}

/// Submenu "Copy from": the original (`None`) and, after a separator, the project languages other
/// than `target`.
fn copy_from_menu(ui: &mut egui::Ui, acts: &mut Vec<Action>, langs: &[String], target: Option<&str>, action: impl Fn(Option<String>) -> Action) {
    ui.menu_button(t("Copy from"), |ui| {
        cmd(ui, acts, t("Original"), action(None));
        let sources: Vec<&String> = langs.iter().filter(|l| Some(l.as_str()) != target).collect();
        if !sources.is_empty() {
            ui.separator();
        }
        for l in sources {
            cmd(ui, acts, &SusiApp::lang_label(l), action(Some(l.clone())));
        }
    });
}

/// One entry per translation status, with its color.
fn status_buttons(ui: &mut egui::Ui, acts: &mut Vec<Action>, action: impl Fn(TranslationStatus) -> Action) {
    for s in TranslationStatus::ALL {
        if ui.button(RichText::new(format!("■ {}", s.label())).color(status_color(s))).clicked() {
            acts.push(action(s));
            ui.close();
        }
    }
}

pub fn row_menu(app: &SusiApp, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
    ui.menu_button(t("Row"), |ui| {
        let langs = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
        let data_row = app.selected.as_ref().and_then(|r| app.project.as_ref()?.row(r)).is_some_and(susi_core::Row::is_data);
        row_menu_items(ui, acts, &langs, app.lang.as_deref(), data_row);
    });
}

/// Row commands (menu "Row" and the grid's context menu) for the cell of `lang` (the current
/// language) in the selected row; `langs` are the project languages, `data_row`: the row holds a
/// picture or binary data.
pub fn row_menu_items(ui: &mut egui::Ui, acts: &mut Vec<Action>, langs: &[String], lang: Option<&str>, data_row: bool) {
    ui.menu_button(t("Status"), |ui| status_buttons(ui, acts, Action::SetStatus));
    copy_from_menu(ui, acts, langs, lang, Action::CopyIntoCell);
    cmd(ui, acts, t("Clear translation"), Action::ClearTranslation);
    if data_row {
        ui.menu_button(t("Picture or binary data"), |ui| {
            cmd(ui, acts, t("Load translation from file…"), Action::Data(DataCmd::Load { link: false }));
            cmd(ui, acts, t("Link translation to file…"), Action::Data(DataCmd::Load { link: true }));
            cmd(ui, acts, t("Remove file link"), Action::Data(DataCmd::Unlink));
            ui.separator();
            cmd(ui, acts, t("Save original as…"), Action::Data(DataCmd::SaveAs { translation: false }));
            cmd(ui, acts, t("Save translation as…"), Action::Data(DataCmd::SaveAs { translation: true }));
        });
    }
    ui.separator();
    cmd(ui, acts, t("Toggle do not translate"), Action::ToggleLocked);
    cmd(ui, acts, t("Toggle mark"), Action::ToggleMarked);
    cmd(ui, acts, t("Toggle excluded"), Action::ToggleExcluded);
    ui.separator();
    ui.menu_button(t("Screenshot"), |ui| {
        cmd(ui, acts, t("Add for this row…"), Action::Screenshot(ShotCmd::Add(None)));
        cmd(ui, acts, t("Add for all rows of the view…"), Action::Screenshot(ShotCmd::Add(Some(Scope::View))));
        cmd(ui, acts, t("Paste from the clipboard"), Action::Screenshot(ShotCmd::Paste));
    });
    ui.separator();
    cmd(ui, acts, t("First row"), Action::Nav(Nav::First));
    cmd(ui, acts, t("Previous row"), Action::Nav(Nav::Prev));
    cmd(ui, acts, t("Next row"), Action::Nav(Nav::Next));
    cmd(ui, acts, t("Last row"), Action::Nav(Nav::Last));
    ui.separator();
    cmd(ui, acts, t("Properties…"), Action::RowProperties);
}

fn tool(ui: &mut egui::Ui, acts: &mut Vec<Action>, text: &str, hint: &str, a: Action) {
    if ui.button(text).on_hover_text(hint).clicked() {
        acts.push(a);
    }
}

fn toolbar(app: &mut SusiApp, ui: &mut egui::Ui) {
    let mut acts = Vec::new();
    ui.horizontal(|ui| {
        tool(ui, &mut acts, "🗋", &format!("{} (Ctrl+N)", t("New project")), Action::New);
        tool(ui, &mut acts, "🗁", &format!("{} (Ctrl+O)", t("Open project")), Action::Open);
        tool(ui, &mut acts, "💾", &format!("{} (Ctrl+S)", t("Save")), Action::Save);
        ui.separator();
        tool(ui, &mut acts, &format!("➕ {}", t("Source")), t("Add source"), Action::AddSource);
        tool(ui, &mut acts, &format!("⟳ {}", t("Scan")), &format!("{} (F5)", t("Scan sources")), Action::Scan);
        tool(ui, &mut acts, &format!("🔨 {}", t("Build")), &format!("{} (F9)", t("Build localized files")), Action::Build);
        ui.separator();
        ui.label(t("Language:"));
        language_combo(app, ui);
        if ui.selectable_label(app.all_langs, t("All languages")).on_hover_text(t("Show all languages side by side or only the selected language")).clicked() {
            app.all_langs = !app.all_langs;
            app.rows_stale = true;
        }
        ui.separator();
        tool(ui, &mut acts, &format!("✔ {}", t("Validate")), t("Validate translations"), Action::Validate { missing: false });
        tool(ui, &mut acts, "🗄 TM", t("Translate with translation memory"), Action::TmTranslate);
        tool(ui, &mut acts, "📊", t("Statistics"), Action::Statistics);
        crate::workflow_ui::toolbar(app, ui, &mut acts);
    });
    app.actions.extend(acts);
}

fn language_combo(app: &mut SusiApp, ui: &mut egui::Ui) {
    let ids = app.project.as_ref().map(Project::language_ids).unwrap_or_default();
    let text = app.lang.as_deref().map(SusiApp::lang_label).unwrap_or_else(|| "–".into());
    egui::ComboBox::from_id_salt("toolbar-lang").width(190.0).selected_text(text).show_ui(ui, |ui| {
        for id in ids {
            if ui.selectable_label(app.lang.as_deref() == Some(id.as_str()), SusiApp::lang_label(&id)).clicked() {
                app.lang = Some(id);
                app.rows_stale = true;
            }
        }
    });
}

fn status_bar(app: &mut SusiApp, ui: &mut egui::Ui) {
    let mut acts = Vec::new();
    ui.horizontal(|ui| {
        crate::workflow_ui::status_bar(app, ui, &mut acts);
        let pos = app.selected_index().map(|i| i + 1).unwrap_or(0);
        ui.label(format!("{pos} / {}", app.rows.len()));
        ui.separator();
        if app.filter.is_active() || app.tree.is_some() {
            ui.colored_label(Color32::from_rgb(200, 40, 40), t("Filter active"));
            ui.separator();
        }
        if app.dirty {
            ui.label(t("Modified"));
            ui.separator();
        }
        let source = app.selected.as_ref().and_then(|r| app.project.as_ref()?.sources.get(r.source)).map(|s| s.name().to_string());
        if let Some(s) = source {
            ui.label(s);
        } else if let Some(p) = app.path() {
            ui.label(p.display().to_string());
        }
    });
    app.actions.extend(acts);
}

#[derive(Default)]
struct TreeOut {
    clicked: Option<Option<NodeRef>>,
    actions: Vec<Action>,
}

fn tree(app: &mut SusiApp, ui: &mut egui::Ui) {
    ui.strong(t("Project"));
    ui.separator();
    let mut out = TreeOut::default();
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let Some(p) = app.project.as_ref() else {
            ui.label(t("No project open"));
            return;
        };
        let total: usize = p.sources.iter().map(|s| s.row_count()).sum();
        let root = ui.selectable_label(app.tree.is_none(), format!("🗀 {} ({total})", t("All")));
        if root.clicked() {
            out.clicked = Some(None);
        }
        root.context_menu(|ui| {
            cmd(ui, &mut out.actions, t("Add source…"), Action::AddSource);
            cmd(ui, &mut out.actions, t("Scan for changes"), Action::Scan);
            cmd(ui, &mut out.actions, t("Build"), Action::Build);
            ui.separator();
            scope_items(ui, &mut out.actions, &Scope::All);
        });
        for (si, s) in p.sources.iter().enumerate() {
            let node = NodeRef { source: si, path: Vec::new() };
            let label = format!("📄 {} ({})", s.name(), s.row_count());
            tree_node(ui, &node, label, &s.items, app.tree.as_ref(), &mut out);
        }
    });
    if let Some(sel) = out.clicked {
        app.tree = sel;
        app.rows_stale = true;
    }
    app.actions.extend(out.actions);
}

fn child_nodes(items: &[Item]) -> Vec<(usize, &Node)> {
    items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match it {
            Item::Node(n) => Some((i, n)),
            _ => None,
        })
        .collect()
}

fn tree_node(ui: &mut egui::Ui, node: &NodeRef, label: String, items: &[Item], selected: Option<&NodeRef>, out: &mut TreeOut) {
    let children = child_nodes(items);
    let is_sel = selected == Some(node);
    if children.is_empty() {
        let r = ui.horizontal(|ui| {
            ui.add_space(ui.spacing().indent);
            ui.selectable_label(is_sel, label)
        });
        tree_label_events(&r.inner, node, out);
        return;
    }
    let id = ui.make_persistent_id(("tree", node.source, &node.path));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, node.path.is_empty())
        .show_header(ui, |ui| {
            let r = ui.selectable_label(is_sel, label);
            tree_label_events(&r, node, out);
        })
        .body(|ui| {
            for (i, n) in children {
                let mut path = node.path.clone();
                path.push(i);
                let child = NodeRef { source: node.source, path };
                tree_node(ui, &child, n.name().to_string(), &n.items, selected, out);
            }
        });
}

fn tree_label_events(r: &egui::Response, node: &NodeRef, out: &mut TreeOut) {
    if r.clicked() {
        out.clicked = Some(Some(node.clone()));
    }
    r.context_menu(|ui| {
        if node.path.is_empty() {
            cmd(ui, &mut out.actions, t("Properties…"), Action::SourceProperties(node.source));
            cmd(ui, &mut out.actions, t("Scan for changes"), Action::ScanSource(node.source));
            cmd(ui, &mut out.actions, t("Build"), Action::BuildSource(node.source));
            ui.separator();
        }
        scope_items(ui, &mut out.actions, &Scope::Node(node.clone()));
        if node.path.is_empty() {
            ui.separator();
            cmd(ui, &mut out.actions, t("Remove source…"), Action::RemoveSource(node.source));
        }
    });
}

/// Context menu entries applying to all rows of a tree node.
fn scope_items(ui: &mut egui::Ui, acts: &mut Vec<Action>, scope: &Scope) {
    ui.menu_button(t("Status (current language)"), |ui| status_buttons(ui, acts, |s| Action::SetStatusIn(scope.clone(), None, s)));
    cmd(ui, acts, t("Do not translate"), Action::SetLockedIn(scope.clone(), true));
    cmd(ui, acts, t("Allow translation"), Action::SetLockedIn(scope.clone(), false));
    cmd(ui, acts, t("Clear new status"), Action::ClearNewIn(scope.clone()));
    cmd(ui, acts, t("Add screenshot to all rows…"), Action::Screenshot(ShotCmd::Add(Some(scope.clone()))));
    ui.separator();
    cmd(ui, acts, t("Statistics"), Action::Statistics);
    cmd(ui, acts, t("Validate"), Action::Validate { missing: false });
}

/// "Date" section of the filter panel; returns whether the filter changed.
fn date_filter(ui: &mut egui::Ui, f: &mut crate::app::FilterState) -> bool {
    use crate::app::DatePeriod;
    use susi_core::filter::DateField;
    let mut changed = false;
    ui.strong(t("Date"));
    egui::ComboBox::from_id_salt("filter-period").selected_text(f.date_period.label()).show_ui(ui, |ui| {
        for p in DatePeriod::ALL {
            changed |= ui.selectable_value(&mut f.date_period, p, p.label()).changed();
        }
    });
    if f.date_period == DatePeriod::Any {
        return changed;
    }
    egui::ComboBox::from_id_salt("filter-datefield").selected_text(f.date_field.label()).show_ui(ui, |ui| {
        for d in DateField::ALL {
            changed |= ui.selectable_value(&mut f.date_field, d, d.label()).changed();
        }
    });
    if f.date_period == DatePeriod::Range {
        let hint = t("Date (YYYY-MM-DD) or 7d");
        for (label, text, end) in [(t("From"), &mut f.date_from, false), (t("To"), &mut f.date_to, true)] {
            ui.horizontal(|ui| {
                ui.label(label);
                let valid = text.trim().is_empty() || susi_core::filter::parse_date(text, end).is_some();
                let mut edit = egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY);
                if !valid {
                    edit = edit.text_color(Color32::from_rgb(220, 30, 30));
                }
                changed |= ui.add(edit).on_hover_text(t("e.g. 2026-11-01, 01.11.2026 14:30, today, yesterday, 7d, 12h")).changed();
            });
        }
    }
    changed
}

fn filter_panel(app: &mut SusiApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        let f = &mut app.filter;
        let mut changed = false;
        ui.strong(t("Translation status"));
        for (i, s) in TranslationStatus::ALL.into_iter().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut f.statuses[i], "").changed();
                ui.colored_label(status_color(s), "■");
                ui.label(s.label());
            });
        }
        ui.separator();
        ui.strong(t("Row status"));
        for (i, s) in RowStatus::ALL.into_iter().enumerate() {
            changed |= ui.checkbox(&mut f.row_statuses[i], s.label()).changed();
        }
        ui.separator();
        changed |= date_filter(ui, f);
        ui.separator();
        ui.strong(t("More"));
        changed |= ui.checkbox(&mut f.only_marked, t("Only marked rows")).changed();
        changed |= ui.checkbox(&mut f.only_locked, t("Only \"Do not translate\"")).changed();
        changed |= ui.checkbox(&mut f.show_excluded, t("Show excluded")).changed();
        let shot_label = |v: Option<bool>| match v {
            None => t("Any"),
            Some(true) => t("With screenshot"),
            Some(false) => t("Without screenshot"),
        };
        ui.horizontal(|ui| {
            ui.label(t("Screenshot:"));
            egui::ComboBox::from_id_salt("filter-shot").selected_text(shot_label(f.screenshot)).show_ui(ui, |ui| {
                for v in [None, Some(true), Some(false)] {
                    changed |= ui.selectable_value(&mut f.screenshot, v, shot_label(v)).changed();
                }
            });
        });
        ui.separator();
        ui.strong(t("Text"));
        changed |= ui.add(egui::TextEdit::singleline(&mut f.text).hint_text(t("Filter text")).desired_width(f32::INFINITY)).changed();
        egui::ComboBox::from_id_salt("filter-in").selected_text(search_in_label(f.search_in)).show_ui(ui, |ui| {
            for s in SEARCH_IN {
                changed |= ui.selectable_value(&mut f.search_in, s, search_in_label(s)).changed();
            }
        });
        changed |= ui.checkbox(&mut f.case_sensitive, t("Match case")).changed();
        ui.separator();
        if f.is_active() {
            ui.colored_label(Color32::from_rgb(220, 30, 30), RichText::new(t("Special filters on!")).strong());
            if ui.button(t("Reset filter")).clicked() {
                *f = crate::app::FilterState::default();
                changed = true;
            }
        }
        if changed {
            app.rows_stale = true;
        }
    });
}
