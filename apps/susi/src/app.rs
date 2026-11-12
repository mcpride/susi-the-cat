//! Application state of the Susi desktop GUI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use susi_core::filter::{DateField, RowFilter, SearchIn, parse_date};
use susi_core::i18n::{Lang, t, trn};
use susi_core::model::{NodeRef, Project, RowRef};
use susi_core::settings::Settings;
use susi_core::stats::Statistics;
use susi_core::status::{RowStatus, TranslationStatus};
use susi_core::tm::{TmMatch, TranslationMemory};
use susi_core::validate::Issue;
use susi_core::{lang, slp};

use crate::actions::Action;
use crate::dialogs::Dialog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Info,
    Warning,
    Error,
}

impl LogKind {
    pub fn label(self) -> &'static str {
        match self {
            LogKind::Info => t("Information"),
            LogKind::Warning => t("Warning"),
            LogKind::Error => t("Error"),
        }
    }
}

pub struct LogEntry {
    pub time: String,
    pub kind: LogKind,
    pub message: String,
}

/// Tabs of the bottom area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Output,
    Validation,
    Search,
    Statistics,
    Comments,
    Editor,
}

impl BottomTab {
    pub const ALL: [BottomTab; 6] =
        [BottomTab::Output, BottomTab::Validation, BottomTab::Search, BottomTab::Statistics, BottomTab::Comments, BottomTab::Editor];

    pub fn label(self) -> &'static str {
        match self {
            BottomTab::Output => t("Output"),
            BottomTab::Validation => t("Validation results"),
            BottomTab::Search => t("Search results"),
            BottomTab::Statistics => t("Statistics"),
            BottomTab::Comments => t("Comments"),
            BottomTab::Editor => t("Text editor"),
        }
    }
}

/// Column the sheet can be sorted by (click on the column header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Context,
    Date,
}

/// Sort order of the sheet; no sort = document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetSort {
    pub column: SortColumn,
    pub descending: bool,
}

impl SheetSort {
    /// Next state after a click on `column`: unsorted, first direction, reverse, unsorted.
    /// The first direction is A–Z for the context and newest first for the date.
    pub fn next(cur: Option<SheetSort>, column: SortColumn) -> Option<SheetSort> {
        let first = column == SortColumn::Date;
        match cur {
            Some(s) if s.column == column && s.descending == first => Some(SheetSort { column, descending: !first }),
            Some(s) if s.column == column => None,
            _ => Some(SheetSort { column, descending: first }),
        }
    }

    /// Sorts `rows` (stable). Rows without a translation date of `lang` stay at the end.
    pub fn apply(self, p: &Project, rows: &mut Vec<RowRef>, lang: Option<&str>) {
        let key = |r: &RowRef| -> Option<(String, String)> {
            match self.column {
                SortColumn::Context => {
                    let ctx = p.context(r);
                    Some((ctx.to_lowercase(), ctx))
                }
                SortColumn::Date => p.row(r).and_then(|row| row.translation_date(lang?)).map(|d| (d.to_string(), String::new())),
            }
        };
        let mut keyed: Vec<_> = rows.drain(..).map(|r| (key(&r), r)).collect();
        keyed.sort_by(|(a, _), (b, _)| match (a, b) {
            (Some(a), Some(b)) if self.descending => b.cmp(a),
            (Some(a), Some(b)) => a.cmp(b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        rows.extend(keyed.into_iter().map(|(_, r)| r));
    }
}

/// Time period of the date filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DatePeriod {
    #[default]
    Any,
    Today,
    Last7Days,
    Last30Days,
    Range,
}

impl DatePeriod {
    pub const ALL: [DatePeriod; 5] = [DatePeriod::Any, DatePeriod::Today, DatePeriod::Last7Days, DatePeriod::Last30Days, DatePeriod::Range];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => t("Any time"),
            Self::Today => t("Today"),
            Self::Last7Days => t("Last 7 days"),
            Self::Last30Days => t("Last 30 days"),
            Self::Range => t("Period…"),
        }
    }
}

/// State of the filter panel on the right.
#[derive(Debug, Clone)]
pub struct FilterState {
    pub statuses: [bool; 6],
    pub row_statuses: [bool; 4],
    pub text: String,
    pub search_in: SearchIn,
    pub case_sensitive: bool,
    pub only_marked: bool,
    pub only_locked: bool,
    pub show_excluded: bool,
    pub date_period: DatePeriod,
    pub date_field: DateField,
    /// Bounds of [`DatePeriod::Range`] as typed (see [`parse_date`]); empty = open.
    pub date_from: String,
    pub date_to: String,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            statuses: [true; 6],
            row_statuses: [true; 4],
            text: String::new(),
            search_in: SearchIn::All,
            case_sensitive: false,
            only_marked: false,
            only_locked: false,
            show_excluded: false,
            date_period: DatePeriod::Any,
            date_field: DateField::Any,
            date_from: String::new(),
            date_to: String::new(),
        }
    }
}

impl FilterState {
    /// Whether any filter restricts the rows ("Special filters on!").
    pub fn is_active(&self) -> bool {
        self.statuses.iter().any(|b| !b)
            || self.row_statuses.iter().any(|b| !b)
            || !self.text.is_empty()
            || self.only_marked
            || self.only_locked
            || self.show_excluded
            || self.date_period != DatePeriod::Any
    }

    /// Timestamp bounds of the date filter; unparsable range bounds are ignored.
    pub fn date_bounds(&self) -> (Option<String>, Option<String>) {
        let bound = |s: &str, end| if s.trim().is_empty() { None } else { parse_date(s, end) };
        match self.date_period {
            DatePeriod::Any => (None, None),
            DatePeriod::Today => (parse_date("today", false), None),
            DatePeriod::Last7Days => (parse_date("6d", false), None),
            DatePeriod::Last30Days => (parse_date("29d", false), None),
            DatePeriod::Range => (bound(&self.date_from, false), bound(&self.date_to, true)),
        }
    }

    /// Core filter for the current view; `None` if no row can match (all boxes of a group unchecked).
    pub fn to_row_filter(&self, langs: Vec<String>, node: Option<NodeRef>) -> Option<RowFilter> {
        if self.statuses.iter().all(|b| !b) || self.row_statuses.iter().all(|b| !b) {
            return None;
        }
        let statuses = if self.statuses.iter().all(|b| *b) {
            Vec::new()
        } else {
            TranslationStatus::ALL.into_iter().zip(self.statuses).filter(|(_, on)| *on).map(|(s, _)| s).collect()
        };
        let row_statuses = if self.row_statuses.iter().all(|b| *b) {
            Vec::new()
        } else {
            RowStatus::ALL.into_iter().zip(self.row_statuses).filter(|(_, on)| *on).map(|(s, _)| s).collect()
        };
        let (since, until) = self.date_bounds();
        Some(RowFilter {
            text: self.text.clone(),
            case_sensitive: self.case_sensitive,
            search_in: self.search_in,
            langs,
            statuses,
            row_statuses,
            node,
            locked: self.only_locked.then_some(true),
            marked: self.only_marked.then_some(true),
            include_excluded: self.show_excluded,
            date_field: self.date_field,
            since,
            until,
            ..Default::default()
        })
    }
}

/// Inline editing of one grid cell.
pub struct CellEdit {
    pub row: RowRef,
    pub lang: String,
    pub text: String,
    pub focus: bool,
}

/// "Search results" tab.
#[derive(Default)]
pub struct SearchState {
    pub text: String,
    pub search_in: SearchIn,
    pub case_sensitive: bool,
    pub results: Vec<RowRef>,
    pub focus: bool,
}

pub struct SusiApp {
    pub project: Option<Project>,
    pub dirty: bool,
    pub settings: Settings,
    /// User interface language resolved from `settings.ui_language` (set as current language every frame).
    pub ui_lang: Lang,
    pub tm: Option<TranslationMemory>,
    /// Selected language (status column, single-language view, editor).
    pub lang: Option<String>,
    /// Show all languages side by side (otherwise only `lang`).
    pub all_langs: bool,
    /// Selected tree node; `None` = "All".
    pub tree: Option<NodeRef>,
    pub filter: FilterState,
    pub rows: Vec<RowRef>,
    pub rows_stale: bool,
    /// Sort order of the sheet (header click); `None` = document order.
    pub sort: Option<SheetSort>,
    pub selected: Option<RowRef>,
    pub scroll_to_selected: bool,
    pub cell_edit: Option<CellEdit>,
    pub log: Vec<LogEntry>,
    pub issues: Vec<Issue>,
    pub issue_cells: HashSet<(RowRef, String)>,
    pub search: SearchState,
    pub stats: Option<Statistics>,
    pub tab: BottomTab,
    pub dialog: Option<Dialog>,
    pub actions: Vec<Action>,
    /// Set while running an action the user already confirmed (unsaved changes).
    pub skip_guard: bool,
    pub allow_close: bool,
    pub show_tree: bool,
    pub show_filter: bool,
    pub show_bottom: bool,
    /// Cached TM suggestions of the text editor: (row, language, original) -> matches.
    pub suggestions: Option<((RowRef, String, String), Vec<TmMatch>)>,
    /// Column widths of the sheet, by column key (see `grid`).
    pub col_widths: HashMap<String, f32>,
    /// Scroll offset of the sheet body (last frame).
    pub grid_offset: egui::Vec2,
    /// Scroll offset of the list shown in the bottom panel (last frame).
    pub list_offset: egui::Vec2,
    /// Git workflow of the open project (configured working copy).
    pub workflow: Option<crate::workflow_ui::WorkflowUi>,
    /// Running git action.
    pub wf_job: Option<crate::workflow_ui::Job>,
    title: String,
}

impl SusiApp {
    pub fn new(ctx: &egui::Context, project: Option<PathBuf>) -> Self {
        Self::with_settings(ctx, Settings::load(), project)
    }

    pub fn with_settings(ctx: &egui::Context, settings: Settings, project: Option<PathBuf>) -> Self {
        let ui_lang = Lang::from_pref(&settings.ui_language);
        susi_core::i18n::set_current(ui_lang);
        let mut app = Self {
            project: None,
            dirty: false,
            settings,
            ui_lang,
            tm: None,
            lang: None,
            all_langs: true,
            tree: None,
            filter: FilterState::default(),
            rows: Vec::new(),
            rows_stale: false,
            sort: None,
            selected: None,
            scroll_to_selected: false,
            cell_edit: None,
            log: Vec::new(),
            issues: Vec::new(),
            issue_cells: HashSet::new(),
            search: SearchState::default(),
            stats: None,
            tab: BottomTab::Output,
            dialog: None,
            actions: Vec::new(),
            skip_guard: false,
            allow_close: false,
            show_tree: true,
            show_filter: true,
            show_bottom: true,
            suggestions: None,
            col_widths: HashMap::new(),
            grid_offset: egui::Vec2::ZERO,
            list_offset: egui::Vec2::ZERO,
            workflow: None,
            wf_job: None,
            title: String::new(),
        };
        app.apply_theme(ctx);
        app.info(susi_core::tf!("Susi the CAT {version} started", version = env!("CARGO_PKG_VERSION")));
        if let Some(p) = project {
            app.open(&p);
        }
        app
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        ctx.set_theme(if self.settings.dark_mode { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    }

    /// Re-resolves the user interface language from the settings and makes it current.
    pub fn apply_ui_language(&mut self) {
        self.ui_lang = Lang::from_pref(&self.settings.ui_language);
        susi_core::i18n::set_current(self.ui_lang);
        // Statistics contain translated labels.
        self.stats = None;
    }

    /// Changes the user interface language preference (`auto` or a language code), applies and saves it.
    pub fn set_ui_language(&mut self, pref: &str) {
        if self.settings.ui_language == pref {
            return;
        }
        self.settings.ui_language = pref.to_string();
        self.apply_ui_language();
        if let Err(e) = self.settings.save() {
            self.warn(susi_core::tf!("Settings could not be saved: {error}", error = e));
        }
    }

    pub fn push_log(&mut self, kind: LogKind, message: impl Into<String>) {
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        self.log.push(LogEntry { time, kind, message: message.into() });
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.push_log(LogKind::Info, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.push_log(LogKind::Warning, message);
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push_log(LogKind::Error, message);
        self.tab = BottomTab::Output;
        self.show_bottom = true;
    }

    pub fn path(&self) -> Option<&Path> {
        self.project.as_ref().and_then(|p| p.path.as_deref())
    }

    /// Records a modification of the project content.
    pub fn touched(&mut self) {
        self.dirty = true;
        self.stats = None;
    }

    /// Records a modification that may change which rows are visible.
    pub fn structure_changed(&mut self) {
        self.touched();
        self.rows_stale = true;
    }

    /// Replaces the current project (after loading or creating one).
    pub fn set_project(&mut self, p: Project) {
        let langs = p.language_ids();
        self.lang = self.lang.take().filter(|l| langs.contains(l)).or_else(|| langs.first().cloned());
        if let Some(path) = &p.path {
            self.settings.add_recent(path.clone());
            if let Err(e) = self.settings.save() {
                self.warn(susi_core::tf!("Settings could not be saved: {error}", error = e));
            }
        }
        self.project = Some(p);
        self.dirty = false;
        self.tree = None;
        self.selected = None;
        self.cell_edit = None;
        self.issues.clear();
        self.issue_cells.clear();
        self.search.results.clear();
        self.stats = None;
        self.rows_stale = true;
        crate::workflow_ui::detect(self);
    }

    pub fn open(&mut self, path: &Path) {
        match slp::load(path) {
            Ok(p) => {
                let rows = p.rows().len();
                self.set_project(p);
                self.info(susi_core::tf!("Project opened: {path} ({rows})", path = path.display(), rows = trn(rows, "{n} row", "{n} rows")));
            }
            Err(e) => self.error(format!("{}: {e}", path.display())),
        }
    }

    /// Saves to the project's path. Returns `false` if the project has no path or saving failed.
    pub fn save(&mut self) -> bool {
        self.commit_cell_edit();
        let Some(p) = self.project.as_mut() else {
            return false;
        };
        let Some(path) = p.path.clone() else {
            return self.save_as();
        };
        p.touch();
        match slp::save(p, &path) {
            Ok(()) => {
                self.dirty = false;
                self.info(susi_core::tf!("Project saved: {path}", path = path.display()));
                self.after_save();
                if self.wf_job.is_none() {
                    crate::workflow_ui::detect(self);
                    if self.wf_job.is_none() {
                        crate::workflow_ui::refresh(self, false);
                    }
                }
                true
            }
            Err(e) => {
                self.error(format!("{}: {e}", path.display()));
                false
            }
        }
    }

    pub fn save_as(&mut self) -> bool {
        let Some(path) = rfd::FileDialog::new().add_filter(t("Susi/Sisulizer project"), &["slp"]).set_title(t("Save project as")).save_file() else {
            return false;
        };
        let path = if path.extension().is_none() { path.with_extension("slp") } else { path };
        if let Some(p) = self.project.as_mut() {
            p.path = Some(path.clone());
        }
        self.settings.add_recent(path);
        self.save()
    }

    fn after_save(&mut self) {
        if !self.settings.auto_save_to_tm {
            return;
        }
        let Some(tm) = self.ensure_tm() else {
            return;
        };
        let mut tm = std::mem::take(tm);
        if let Some(p) = self.project.as_mut() {
            let n = susi_core::tm::save_to_tm(p, &mut tm, &[], &[], false);
            if n > 0 {
                match tm.save() {
                    Ok(()) => self.info(trn(
                        n,
                        "{n} translation automatically saved to the translation memory",
                        "{n} translations automatically saved to the translation memory",
                    )),
                    Err(e) => self.error(susi_core::tf!("Translation memory: {details}", details = e)),
                }
            }
        }
        self.tm = Some(tm);
    }

    /// Loads the translation memory on first use.
    pub fn ensure_tm(&mut self) -> Option<&mut TranslationMemory> {
        if self.tm.is_none() {
            self.suggestions = None;
            let path = self.settings.tm_path();
            match TranslationMemory::open(&path) {
                Ok(tm) => {
                    self.info(susi_core::tf!(
                        "Translation memory loaded: {path} ({entries})",
                        path = path.display(),
                        entries = trn(tm.len(), "{n} entry", "{n} entries")
                    ));
                    self.tm = Some(tm);
                }
                Err(e) => {
                    self.error(susi_core::tf!("Translation memory {path}: {error}", path = path.display(), error = e));
                    return None;
                }
            }
        }
        self.tm.as_mut()
    }

    /// Languages shown as grid columns.
    pub fn visible_langs(&self) -> Vec<String> {
        let Some(p) = &self.project else {
            return Vec::new();
        };
        if self.all_langs { p.language_ids() } else { self.lang.iter().cloned().collect() }
    }

    pub fn refresh_rows(&mut self) {
        self.rows_stale = false;
        let Some(p) = &self.project else {
            self.rows.clear();
            return;
        };
        let langs = if self.all_langs { Vec::new() } else { self.lang.iter().cloned().collect() };
        self.rows = match self.filter.to_row_filter(langs, self.tree.clone()) {
            Some(f) => f.apply(p),
            None => Vec::new(),
        };
        if let Some(s) = self.sort {
            s.apply(p, &mut self.rows, self.lang.as_deref());
        }
    }

    pub fn select(&mut self, row: RowRef) {
        if self.selected.as_ref() != Some(&row) {
            self.commit_cell_edit();
        }
        if !self.rows.contains(&row) {
            self.tree = None;
            self.filter = FilterState::default();
            self.refresh_rows();
        }
        self.selected = Some(row);
        self.scroll_to_selected = true;
    }

    pub fn selected_index(&self) -> Option<usize> {
        let sel = self.selected.as_ref()?;
        self.rows.iter().position(|r| r == sel)
    }

    /// Writes a translation, deriving the status: editing an untranslated, guessed or automatically
    /// translated text makes it "Translated"; otherwise the status is kept.
    pub fn write_translation(&mut self, row: &RowRef, lang: &str, text: &str) {
        let Some(r) = self.project.as_mut().and_then(|p| p.row_mut(row)) else {
            return;
        };
        if r.translation(lang).map(|t| t.text.as_str()).unwrap_or("") == text {
            return;
        }
        let status = match r.tstatus(lang) {
            TranslationStatus::NotTranslated | TranslationStatus::BestGuess | TranslationStatus::AutoTranslated => TranslationStatus::Translated,
            s => s,
        };
        r.set_translation(lang, text, status);
        self.issue_cells.remove(&(row.clone(), lang.to_string()));
        self.touched();
    }

    pub fn commit_cell_edit(&mut self) {
        if let Some(e) = self.cell_edit.take() {
            self.write_translation(&e.row, &e.lang, &e.text);
        }
    }

    pub fn start_cell_edit(&mut self, row: RowRef, lang: String) {
        self.commit_cell_edit();
        let Some(r) = self.project.as_ref().and_then(|p| p.row(&row)) else {
            return;
        };
        if r.locked() {
            self.warn(susi_core::tf!("Row \"{row}\" is marked as \"Do not translate\"", row = r.id()));
            return;
        }
        let text = r.translation(&lang).map(|t| t.text.clone()).unwrap_or_default();
        self.lang = Some(lang.clone());
        self.selected = Some(row.clone());
        self.cell_edit = Some(CellEdit { row, lang, text, focus: true });
    }

    pub fn set_issues(&mut self, issues: Vec<Issue>) {
        self.issue_cells = issues.iter().map(|i| (i.row.clone(), i.lang.clone())).collect();
        self.issues = issues;
    }

    fn window_title(&self) -> String {
        match self.path() {
            Some(p) => format!("Susi – {}{}", p.display(), if self.dirty { " *" } else { "" }),
            None => "Susi the CAT".to_string(),
        }
    }

    pub fn lang_label(id: &str) -> String {
        let name = lang::name(id);
        if name == id { id.to_string() } else { format!("{name} ({id})") }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.dialog = Some(Dialog::Confirm { then: Action::Exit });
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let shortcuts = [
            (KeyboardShortcut::new(Modifiers::COMMAND, Key::S), Action::Save),
            (KeyboardShortcut::new(Modifiers::COMMAND, Key::O), Action::Open),
            (KeyboardShortcut::new(Modifiers::COMMAND, Key::N), Action::New),
            (KeyboardShortcut::new(Modifiers::COMMAND, Key::F), Action::Find),
            (KeyboardShortcut::new(Modifiers::NONE, Key::F5), Action::Scan),
            (KeyboardShortcut::new(Modifiers::NONE, Key::F9), Action::Build),
        ];
        for (sc, action) in shortcuts {
            if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
                self.actions.push(action);
            }
        }
        if self.dialog.is_some() || self.cell_edit.is_some() || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let nav = [
            (Key::ArrowDown, Action::Nav(Nav::Next)),
            (Key::ArrowUp, Action::Nav(Nav::Prev)),
            (Key::Home, Action::Nav(Nav::First)),
            (Key::End, Action::Nav(Nav::Last)),
            (Key::F2, Action::EditCell),
            (Key::Enter, Action::EditCell),
        ];
        for (key, action) in nav {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                self.actions.push(action);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    First,
    Prev,
    Next,
    Last,
}

impl eframe::App for SusiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

impl SusiApp {
    /// Renders one frame and processes the queued actions.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        susi_core::i18n::set_current(self.ui_lang);
        let ctx = ui.ctx().clone();
        self.handle_close_request(&ctx);
        self.handle_shortcuts(&ctx);
        crate::workflow_ui::poll(self, &ctx);
        if self.rows_stale {
            self.refresh_rows();
        }
        crate::panels::show(self, ui);
        crate::dialogs::show(self, &ctx);
        self.run_actions(&ctx);
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}
