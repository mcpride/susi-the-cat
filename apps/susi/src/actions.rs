//! Commands triggered from menus, toolbars, context menus and shortcuts.

use std::path::PathBuf;

use susi_core::exchange::{ExportOptions, ImportOptions, ScreenshotExport};
use susi_core::i18n::{t, trn};
use susi_core::model::{NodeRef, Project, RowRef};
use susi_core::sds::Scope as ValidationScope;
use susi_core::status::{RowStatus, TranslationStatus};
use susi_core::tm::{self, TranslateOptions, TranslationMemory};
use susi_core::validate::{self, Severity, ValidateOptions, ValidationKind};
use susi_core::{ops, pseudo, slp, stats};

use crate::app::{BottomTab, Nav, SusiApp};
use crate::dialogs::{
    ClearColumnDialog, CopyDialog, Dialog, ExcludedDialog, ExportDialog, ImportDialog, PseudoDialog, RowPropsDialog, SourcePropsDialog, TmDialog,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    New,
    Open,
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    Exit,
    Export,
    Import,
    AddSource,
    Scan,
    Build,
    ScanSource(usize),
    BuildSource(usize),
    Languages,
    SourceProperties(usize),
    RemoveSource(usize),
    RemoveUnused,
    Statistics,
    Report,
    TmTranslate,
    SaveToTm,
    TranslateDuplicates,
    FillPseudo,
    RemovePseudo,
    Validate {
        missing: bool,
    },
    ClearIssues,
    /// Excludes the check of the validation result with this index from future validations.
    ExcludeIssue(usize),
    /// Opens the list of excluded original or translation validations.
    ExcludedValidations(ValidationScope),
    Find,
    ResetFilter,
    /// Status of the selected cells (selected rows, current language).
    SetStatus(TranslationStatus),
    /// Status of the current cell only (the editors show one cell).
    SetCellStatus(TranslationStatus),
    /// Status of all rows of a scope in a language (`None`: the current language).
    SetStatusIn(Scope, Option<String>, TranslationStatus),
    /// Copies the original (`from` = `None`) or the translations in `from` into the visible rows of
    /// a language column (`lang` = `None`: the current language); opens a dialog.
    CopyInColumn {
        from: Option<String>,
        lang: Option<String>,
    },
    /// Clears the translations of the visible rows of a language column (`None`: the current
    /// language) after confirmation.
    ClearColumn(Option<String>),
    /// Toggles of the selected rows; the current row decides the new value for all.
    ToggleLocked,
    ToggleMarked,
    ToggleExcluded,
    SetLockedIn(Scope, bool),
    ClearNewIn(Scope),
    RowProperties,
    /// Copies the original (`None`) or the translation in a language into the selected cells
    /// (selected rows, current language).
    CopyIntoCell(Option<String>),
    /// Clears the selected cells, after confirmation if there are several.
    ClearTranslation,
    /// Clears the current cell only (the editors show one cell).
    ClearCell,
    EditCell,
    Nav(Nav),
    /// Selects all rows of the sheet (`Ctrl+A`).
    SelectAll,
    Settings,
    About,
    /// Help page for the current context (`F1`).
    Help,
    /// Start page of the user guide.
    UserGuide,
    /// Git workflow command.
    Workflow(crate::workflow_ui::Command),
    /// Screenshot command.
    Screenshot(crate::shots::ShotCmd),
    /// Picture or binary data of the selected cell.
    Data(crate::data_ui::DataCmd),
}

/// Rows an action applies to.
#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    /// All rows of the project.
    All,
    /// Rows of the current (filtered) view.
    View,
    /// Rows of a source or node.
    Node(NodeRef),
}

impl Action {
    /// Actions that discard the current project and need confirmation if it has unsaved changes.
    fn needs_guard(&self) -> bool {
        matches!(self, Action::New | Action::Open | Action::OpenRecent(_) | Action::Exit)
    }
}

impl SusiApp {
    pub fn run_actions(&mut self, ctx: &egui::Context) {
        for a in std::mem::take(&mut self.actions) {
            if a.needs_guard() && self.dirty && !self.skip_guard {
                self.dialog = Some(Dialog::Confirm { then: a });
                continue;
            }
            self.skip_guard = false;
            self.run(a, ctx);
        }
    }

    fn run(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::New => self.new_project(),
            Action::Open => {
                if let Some(p) = rfd::FileDialog::new().add_filter(t("Susi/Sisulizer project"), &["slp"]).set_title(t("Open project")).pick_file() {
                    self.open(&p);
                }
            }
            Action::OpenRecent(p) => self.open(&p),
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::Exit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::Settings => self.dialog = Some(Dialog::Settings(Box::new(self.settings.clone()))),
            Action::About => self.dialog = Some(Dialog::About),
            Action::Help => crate::help::open(self, ctx, crate::help::topic(self)),
            Action::UserGuide => crate::help::open(self, ctx, "index"),
            Action::Find => {
                self.show_bottom = true;
                self.tab = BottomTab::Search;
                self.search.focus = true;
            }
            Action::ResetFilter => {
                self.filter = crate::app::FilterState::default();
                self.tree = None;
                self.rows_stale = true;
            }
            Action::Nav(n) => self.navigate(n),
            Action::SelectAll => self.select_all(),
            Action::Workflow(c) => crate::workflow_ui::run(self, c, ctx),
            Action::Screenshot(c) => crate::shots::run(self, c, ctx),
            other => self.run_project_action(other),
        }
    }

    /// Actions that need an open project.
    fn run_project_action(&mut self, a: Action) {
        if self.project.is_none() {
            self.warn(t("No project open"));
            return;
        }
        match a {
            Action::Export => self.export_dialog(),
            Action::Import => self.import_dialog(),
            Action::AddSource => self.add_source_dialog(),
            Action::Scan => self.scan(None),
            Action::Build => self.build(None),
            Action::ScanSource(i) => self.scan(Some(vec![i])),
            Action::BuildSource(i) => self.build(Some(vec![i])),
            Action::Languages => self.dialog = Some(Dialog::Languages { add: String::new(), remove: None }),
            Action::SourceProperties(i) => self.source_props(i),
            Action::RemoveSource(i) => self.dialog = Some(Dialog::RemoveSource(i)),
            Action::RemoveUnused => self.remove_unused(),
            Action::Statistics => self.statistics(),
            Action::Report => self.report(),
            Action::Validate { missing } => self.validate(missing),
            Action::ClearIssues => self.set_issues(Vec::new()),
            Action::ExcludeIssue(i) => self.exclude_issue(i),
            Action::ExcludedValidations(scope) => self.dialog = Some(Dialog::Excluded(ExcludedDialog { scope, selected: None })),
            other => self.run_translation_action(other),
        }
    }

    fn run_translation_action(&mut self, a: Action) {
        match a {
            Action::TmTranslate => self.dialog = Some(Dialog::TmTranslate(TmDialog { overwrite: false, fuzzy: true, only_view: false, only_lang: false })),
            Action::SaveToTm => self.save_to_tm(),
            Action::TranslateDuplicates => self.translate_duplicates(),
            Action::FillPseudo => self.pseudo_dialog(),
            Action::RemovePseudo => {
                let n = self.project.as_mut().map(|p| pseudo::remove_pseudo(p, None)).unwrap_or(0);
                self.structure_changed();
                self.info(trn(n, "{n} pseudo translation removed", "{n} pseudo translations removed"));
            }
            other => self.run_row_action(other),
        }
    }

    fn run_row_action(&mut self, a: Action) {
        match a {
            Action::SetStatus(s) => self.set_status(s),
            Action::SetCellStatus(s) => self.set_cell_status(s),
            Action::SetStatusIn(scope, lang, s) => self.set_status_in(scope, lang, s),
            Action::CopyInColumn { from, lang } => self.copy_dialog(from, lang),
            Action::ClearColumn(lang) => self.clear_column_dialog(lang),
            Action::SetLockedIn(scope, on) => self.for_rows_in(scope, |r| r.set_locked(on)),
            Action::ClearNewIn(scope) => self.for_rows_in(scope, |r| {
                if r.status() == RowStatus::New {
                    r.set_status(RowStatus::InUse);
                }
            }),
            Action::ToggleLocked => self.toggle_rows(susi_core::Row::locked, susi_core::Row::set_locked),
            Action::ToggleMarked => self.toggle_rows(susi_core::Row::marked, susi_core::Row::set_marked),
            Action::ToggleExcluded => self.toggle_rows(susi_core::Row::excluded, susi_core::Row::set_excluded),
            Action::RowProperties => self.row_props_dialog(),
            Action::Data(c) => crate::data_ui::run(self, c),
            Action::CopyIntoCell(from) => self.copy_into_cell(from),
            Action::ClearTranslation => self.clear_translation(),
            Action::ClearCell => self.clear_cell(),
            Action::EditCell => {
                if let (Some(row), Some(lang)) = (self.selected.clone(), self.lang.clone()) {
                    self.start_cell_edit(row, lang);
                }
            }
            _ => {}
        }
    }

    fn new_project(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter(t("Susi/Sisulizer project"), &["slp"]).set_title(t("Create new project")).save_file() else {
            return;
        };
        let path = if path.extension().is_none() { path.with_extension("slp") } else { path };
        let mut p = Project::new();
        p.path = Some(path.clone());
        if let Err(e) = slp::save(&mut p, &path) {
            self.error(format!("{}: {e}", path.display()));
            return;
        }
        self.set_project(p);
        self.info(susi_core::tf!("New project created: {path}", path = path.display()));
        self.dialog = Some(Dialog::Languages { add: String::new(), remove: None });
    }

    fn navigate(&mut self, n: Nav) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        let cur = self.selected_index();
        let idx = match (n, cur) {
            (Nav::First, _) | (Nav::Next | Nav::Prev, None) => 0,
            (Nav::Last, _) => last,
            (Nav::Next, Some(i)) => (i + 1).min(last),
            (Nav::Prev, Some(i)) => i.saturating_sub(1),
        };
        let row = self.rows[idx].clone();
        self.select(row);
    }

    /// Sets a flag of the selected rows to the opposite of the current row's value.
    fn toggle_rows(&mut self, get: fn(&susi_core::Row) -> bool, set: fn(&mut susi_core::Row, bool)) {
        let Some(sel) = self.selected.clone() else {
            self.warn(t("No row selected"));
            return;
        };
        let rows = self.selected_rows();
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let Some(on) = p.row(&sel).map(|r| !get(r)) else {
            return;
        };
        for r in &rows {
            if let Some(row) = p.row_mut(r) {
                set(row, on);
            }
        }
        if rows.len() > 1 {
            self.info(trn(rows.len(), "{n} row changed", "{n} rows changed"));
        }
        self.touched();
    }

    pub(crate) fn rows_of(&self, scope: &Scope) -> Vec<RowRef> {
        match (scope, &self.project) {
            (Scope::All, Some(p)) => p.rows(),
            (Scope::Node(n), Some(p)) => p.rows_in(n),
            _ => self.rows.clone(),
        }
    }

    fn for_rows_in(&mut self, scope: Scope, mut f: impl FnMut(&mut susi_core::Row)) {
        let rows = self.rows_of(&scope);
        if let Some(p) = self.project.as_mut() {
            for r in &rows {
                if let Some(row) = p.row_mut(r) {
                    f(row);
                }
            }
        }
        self.info(trn(rows.len(), "{n} row changed", "{n} rows changed"));
        self.structure_changed();
    }

    fn set_status(&mut self, s: TranslationStatus) {
        let rows = self.selected_rows();
        let Some(lang) = self.lang.clone().filter(|_| rows.len() > 1) else {
            return self.set_cell_status(s);
        };
        self.commit_cell_edit();
        let n = self.set_status_of(&rows, lang, s);
        if n < rows.len() {
            self.warn(trn(rows.len() - n, "{n} selected cell contains no translation", "{n} selected cells contain no translation"));
        }
    }

    fn set_cell_status(&mut self, s: TranslationStatus) {
        self.commit_cell_edit();
        let (Some(sel), Some(lang)) = (self.selected.clone(), self.lang.clone()) else {
            self.warn(t("No row or language selected"));
            return;
        };
        let Some(row) = self.project.as_mut().and_then(|p| p.row_mut(&sel)) else {
            return;
        };
        if row.set_tstatus(&lang, s) {
            self.touched();
        } else {
            self.warn(t("The cell contains no translation; the status cannot be set"));
        }
    }

    /// The given language or else the current one; warns if there is none.
    fn lang_or_current(&mut self, lang: Option<String>) -> Option<String> {
        let lang = lang.or_else(|| self.lang.clone());
        if lang.is_none() {
            self.warn(t("No language selected"));
        }
        lang
    }

    fn set_status_in(&mut self, scope: Scope, lang: Option<String>, s: TranslationStatus) {
        let Some(lang) = self.lang_or_current(lang) else {
            return;
        };
        let rows = self.rows_of(&scope);
        self.set_status_of(&rows, lang, s);
    }

    /// Sets the status of the translations of `lang` in `rows`; returns how many were set.
    fn set_status_of(&mut self, rows: &[RowRef], lang: String, s: TranslationStatus) -> usize {
        let mut n = 0;
        if let Some(p) = self.project.as_mut() {
            for r in rows {
                if p.row_mut(r).is_some_and(|row| row.set_tstatus(&lang, s)) {
                    n += 1;
                }
            }
        }
        self.info(susi_core::tf!(
            "Status \"{status}\" set for {count} ({lang})",
            status = s.label(),
            count = trn(n, "{n} translation", "{n} translations"),
            lang = lang
        ));
        self.structure_changed();
        n
    }

    /// Copies the original (`from` = `None`) or the translation in `from` into the selected cells.
    /// Several cells are filled like a column: rows marked "Do not translate", excluded rows and
    /// rows without a text to copy are skipped.
    fn copy_into_cell(&mut self, from: Option<String>) {
        let (Some(sel), Some(lang)) = (self.selected.clone(), self.lang.clone()) else {
            self.warn(t("No row or language selected"));
            return;
        };
        let rows = self.selected_rows();
        if rows.len() > 1 {
            self.commit_cell_edit();
            let Some(p) = self.project.as_mut() else {
                return;
            };
            let done = ops::copy_cells(p, &rows, from.as_deref(), &lang, true);
            self.copied(&done, from.as_deref(), &lang);
            return;
        }
        let text = self.project.as_ref().and_then(|p| ops::copy_source(p, &sel, from.as_deref())).map(str::to_string);
        match (text, from) {
            (Some(text), _) => {
                self.cell_edit = None;
                self.write_translation(&sel, &lang, &text);
            }
            (None, Some(from)) => self.warn(susi_core::tf!("The row has no translation in {lang}", lang = from)),
            (None, None) => {}
        }
    }

    /// Opens the dialog copying the original or a language into the visible rows of a language column.
    fn copy_dialog(&mut self, from: Option<String>, lang: Option<String>) {
        let Some(lang) = self.lang_or_current(lang) else {
            return;
        };
        self.commit_cell_edit();
        let rows = self.rows.clone();
        let Some(p) = &self.project else {
            return;
        };
        let targets = ops::copy_targets(p, &rows, from.as_deref(), &lang);
        if targets.count(true) == 0 {
            self.info(susi_core::tf!("Nothing to copy in the column {lang}", lang = lang));
            return;
        }
        self.dialog = Some(Dialog::Copy(CopyDialog { from, lang, rows, targets, overwrite: false }));
    }

    pub fn copy_in_column(&mut self, d: &CopyDialog) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let done = ops::copy_cells(p, &d.rows, d.from.as_deref(), &d.lang, d.overwrite);
        self.copied(&done, d.from.as_deref(), &d.lang);
    }

    /// Bookkeeping and message after copying the original or `from` into the cells of `lang` in `done`.
    fn copied(&mut self, done: &[RowRef], from: Option<&str>, lang: &str) {
        self.cells_changed(done, lang);
        let count = trn(done.len(), "{n} cell", "{n} cells");
        self.info(match from {
            None => susi_core::tf!("Original copied into {count} ({lang})", count = count, lang = lang),
            Some(from) => susi_core::tf!("Translations of {from} copied into {count} ({lang})", from = from, count = count, lang = lang),
        });
    }

    /// Clears the selected cell at once, several selected cells after confirmation.
    fn clear_translation(&mut self) {
        let rows = self.selected_rows();
        let Some(lang) = self.lang.clone().filter(|_| rows.len() > 1) else {
            return self.clear_cell();
        };
        self.commit_cell_edit();
        let rows = self.project.as_ref().map(|p| ops::translated_rows(p, &rows, &lang)).unwrap_or_default();
        if rows.is_empty() {
            self.info(susi_core::tf!("The selected rows have no translations in {lang}", lang = lang));
            return;
        }
        self.dialog = Some(Dialog::ClearColumn(ClearColumnDialog { lang, rows, selection: true }));
    }

    fn clear_cell(&mut self) {
        if let (Some(row), Some(lang)) = (self.selected.clone(), self.lang.clone()) {
            self.cell_edit = None;
            self.write_translation(&row, &lang, "");
        }
    }

    /// Asks before clearing the translations of the visible rows of a language column.
    fn clear_column_dialog(&mut self, lang: Option<String>) {
        let Some(lang) = self.lang_or_current(lang) else {
            return;
        };
        self.commit_cell_edit();
        let rows = self.project.as_ref().map(|p| ops::translated_rows(p, &self.rows, &lang)).unwrap_or_default();
        if rows.is_empty() {
            self.info(susi_core::tf!("The column {lang} has no translations in the visible rows", lang = lang));
            return;
        }
        self.dialog = Some(Dialog::ClearColumn(ClearColumnDialog { lang, rows, selection: false }));
    }

    pub fn clear_column(&mut self, d: &ClearColumnDialog) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let done = ops::clear_translations(p, &d.rows, &d.lang);
        self.cells_changed(&done, &d.lang);
        self.info(susi_core::tf!("{count} deleted ({lang})", count = trn(done.len(), "{n} translation", "{n} translations"), lang = d.lang));
    }

    /// Bookkeeping after the cells of `lang` in `rows` were changed.
    fn cells_changed(&mut self, rows: &[RowRef], lang: &str) {
        if rows.is_empty() {
            return;
        }
        for r in rows {
            self.issue_cells.remove(&(r.clone(), lang.to_string()));
        }
        self.structure_changed();
    }

    fn row_props_dialog(&mut self) {
        let Some(sel) = self.selected.clone() else {
            self.warn(t("No row selected"));
            return;
        };
        if let Some(r) = self.project.as_ref().and_then(|p| p.row(&sel)) {
            self.dialog = Some(Dialog::RowProps(RowPropsDialog {
                note: r.note().to_string(),
                max_chars: if r.max_chars() == 0 { String::new() } else { r.max_chars().to_string() },
                locked: r.locked(),
                excluded: r.excluded(),
                marked: r.marked(),
                status: r.status(),
                row: sel,
            }));
        }
    }

    fn source_props(&mut self, i: usize) {
        let Some(s) = self.project.as_ref().and_then(|p| p.sources.get(i)) else {
            return;
        };
        let (dir, pattern) = s.output_pattern();
        self.dialog = Some(Dialog::SourceProps(SourcePropsDialog {
            index: i,
            name: s.name().to_string(),
            kind: s.kind().label().to_string(),
            original: s.original().to_string(),
            method: s.write_method(),
            dir,
            pattern,
            po: (s.kind() == susi_core::SourceKind::Po).then(|| s.po_options()),
            encoding: (s.kind() == susi_core::SourceKind::Properties).then(|| s.properties_encoding()),
            android: s.kind() == susi_core::SourceKind::Android,
            excluded: (s.kind() == susi_core::SourceKind::Resx).then(|| s.excluded_data()),
        }));
    }

    fn add_source_dialog(&mut self) {
        let Some(files) = rfd::FileDialog::new()
            .add_filter(
                t("Supported sources (JSON, .NET RESX/RESW, PO, TSV, Java, Android, ARB, Qt TS)"),
                &["json", "resx", "resw", "po", "pot", "tsv", "properties", "xml", "arb", "ts"],
            )
            .add_filter(t("JSON (also WebExtension messages.json)"), &["json"])
            .add_filter(t(".NET resources"), &["resx", "resw"])
            .add_filter("PO/POT (gettext)", &["po", "pot"])
            .add_filter(t("i18n catalogs (TSV)"), &["tsv"])
            .add_filter("Java properties", &["properties"])
            .add_filter(t("Android string resources"), &["xml"])
            .add_filter("Flutter ARB", &["arb"])
            .add_filter("Qt Linguist (TS)", &["ts"])
            .set_title(t("Add source"))
            .pick_files()
        else {
            return;
        };
        let original = self.project.as_ref().and_then(|p| p.sources.first()).map(|s| s.original().to_string()).unwrap_or_else(|| "en".into());
        self.dialog = Some(Dialog::AddSource { files, original });
    }

    pub fn add_sources(&mut self, files: &[PathBuf], original: &str) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let mut msgs = Vec::new();
        for f in files {
            msgs.push(match ops::add_source(p, f, original) {
                Ok((_, rep)) => Ok(susi_core::tf!("Source added: {path} – {summary}", path = f.display(), summary = rep.summary())),
                Err(e) => Err(format!("{}: {e}", f.display())),
            });
        }
        for m in msgs {
            match m {
                Ok(s) => self.info(s),
                Err(s) => self.error(s),
            }
        }
        self.structure_changed();
    }

    pub fn scan(&mut self, sources: Option<Vec<usize>>) {
        self.commit_cell_edit();
        crate::data_ui::refresh_linked(self);
        self.data_ui.clear();
        let Some(p) = self.project.as_mut() else {
            return;
        };
        match ops::scan_project(p, sources.as_deref()) {
            Ok(rep) => {
                let errors = rep.errors.clone();
                self.info(susi_core::tf!("Scan: {summary}", summary = rep.summary()));
                for e in errors {
                    self.error(e);
                }
            }
            Err(e) => self.error(susi_core::tf!("Scan failed: {error}", error = e)),
        }
        self.structure_changed();
    }

    pub fn build(&mut self, sources: Option<Vec<usize>>) {
        self.commit_cell_edit();
        crate::data_ui::refresh_linked(self);
        let Some(p) = self.project.as_ref() else {
            return;
        };
        let rep = ops::build_project(p, &p.language_ids(), sources.as_deref());
        for f in &rep.written {
            self.push_log(crate::app::LogKind::Info, susi_core::tf!("Written: {path}", path = f.display()));
        }
        for e in &rep.errors {
            self.error(e.clone());
        }
        self.info(susi_core::tf!("Build: {summary}", summary = rep.summary()));
    }

    fn remove_unused(&mut self) {
        let n = self.project.as_mut().map(Project::remove_unused_rows).unwrap_or(0);
        self.clear_selection();
        self.set_issues(Vec::new());
        self.structure_changed();
        self.info(trn(n, "{n} unused row removed", "{n} unused rows removed"));
    }

    pub fn statistics(&mut self) {
        if let Some(p) = &self.project {
            self.stats = Some(stats::statistics(p, None));
        }
        self.show_bottom = true;
        self.tab = BottomTab::Statistics;
    }

    fn report(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HTML", &["html"])
            .add_filter(t("Text"), &["txt"])
            .add_filter("JSON", &["json"])
            .set_title(t("Save report"))
            .save_file()
        else {
            return;
        };
        let Some(p) = &self.project else {
            return;
        };
        let title = p.path.as_ref().and_then(|x| x.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let s = stats::statistics(p, None);
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let content = match ext.as_str() {
            "json" => serde_json::to_string_pretty(&s).unwrap_or_default(),
            "txt" => s.to_text(),
            _ => s.to_html(&title),
        };
        match std::fs::write(&path, content) {
            Ok(()) => self.info(susi_core::tf!("Report saved: {path}", path = path.display())),
            Err(e) => self.error(format!("{}: {e}", path.display())),
        }
    }

    pub fn validate(&mut self, missing: bool) {
        self.commit_cell_edit();
        let Some(p) = &self.project else {
            return;
        };
        let mut opts = ValidateOptions::default();
        if missing {
            opts.kinds = ValidationKind::ALL.to_vec();
        }
        let mut issues = validate::validate(p, &opts, None);
        if let Some(shots) = &self.shots {
            issues.extend(shots.issues(p, None));
        }
        let excluded = validate::remove_excluded(p, &mut issues);
        let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
        let mut msg = susi_core::tf!(
            "Validation: {errors}, {warnings}",
            errors = trn(errors, "{n} error", "{n} errors"),
            warnings = trn(issues.len() - errors, "{n} warning", "{n} warnings")
        );
        if excluded > 0 {
            msg = format!("{msg} ({})", trn(excluded, "{n} excluded", "{n} excluded"));
        }
        self.set_issues(issues);
        self.last_validation = Some(missing);
        self.info(msg);
        self.show_bottom = true;
        self.tab = BottomTab::Validation;
    }

    /// Excludes the check of validation result `i` for its row (all languages) and removes the
    /// results hidden by that from the list.
    fn exclude_issue(&mut self, i: usize) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let Some(e) = self.issues.get(i).and_then(|is| is.exclusion(p)) else {
            return;
        };
        let msg = susi_core::tf!("Validation excluded: {check} – {context}", check = validate::class_label(&e.class), context = e.context);
        if !p.desktop.add(e) {
            return;
        }
        let mut issues = std::mem::take(&mut self.issues);
        validate::remove_excluded(p, &mut issues);
        self.set_issues(issues);
        self.touched();
        self.info(msg);
    }

    pub fn tm_translate(&mut self, d: &TmDialog) {
        self.commit_cell_edit();
        let fuzzy = d.fuzzy.then_some(self.settings.fuzzy_threshold);
        let langs: Vec<String> = if d.only_lang { self.lang.iter().cloned().collect() } else { Vec::new() };
        let rows = d.only_view.then(|| self.rows.clone());
        let Some(tm) = self.ensure_tm() else {
            return;
        };
        let tm = std::mem::take(tm);
        if let Some(p) = self.project.as_mut() {
            let rep = tm::translate_project(p, &tm, &TranslateOptions { langs, overwrite: d.overwrite, fuzzy }, rows.as_deref());
            self.info(susi_core::tf!("Translation memory: {details}", details = rep.summary()));
        }
        self.tm = Some(tm);
        self.structure_changed();
    }

    fn save_to_tm(&mut self) {
        self.commit_cell_edit();
        let Some(tm) = self.ensure_tm() else {
            return;
        };
        let mut tm = std::mem::take(tm);
        if let Some(p) = self.project.as_mut() {
            let n = tm::save_to_tm(p, &mut tm, &[], &[], true);
            self.suggestions = None;
            match tm.save() {
                Ok(()) => self.info(susi_core::tf!(
                    "{translations} saved to the translation memory ({entries})",
                    translations = trn(n, "{n} translation", "{n} translations"),
                    entries = trn(tm.len(), "{n} entry", "{n} entries")
                )),
                Err(e) => self.error(susi_core::tf!("Translation memory: {details}", details = e)),
            }
            self.touched();
        }
        self.tm = Some(tm);
    }

    fn translate_duplicates(&mut self) {
        self.commit_cell_edit();
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let mut memory = TranslationMemory::new();
        let usable = [TranslationStatus::Translated, TranslationStatus::Complete, TranslationStatus::ForReview, TranslationStatus::AutoTranslated];
        let langs = p.language_ids();
        tm::save_to_tm(p, &mut memory, &langs, &usable, false);
        let rep = tm::translate_project(p, &memory, &TranslateOptions { langs, overwrite: false, fuzzy: None }, None);
        self.info(trn(rep.exact, "{n} duplicate string translated", "{n} duplicate strings translated"));
        self.structure_changed();
    }

    fn pseudo_dialog(&mut self) {
        let lang = self.project.as_ref().and_then(|p| p.languages.iter().find(|l| l.is_pseudo()).map(|l| l.id().to_string())).unwrap_or_else(|| "qps".into());
        self.dialog = Some(Dialog::Pseudo(PseudoDialog { lang, options: self.settings.pseudo.clone(), overwrite: false }));
    }

    pub fn fill_pseudo(&mut self, d: &PseudoDialog) {
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let lang = d.lang.trim().to_string();
        let created = !p.has_language(&lang);
        if created {
            pseudo::set_pseudo_language(p, &lang, d.options.kind);
        }
        let n = pseudo::fill_pseudo(p, &lang, &d.options, None, d.overwrite);
        if created {
            self.info(susi_core::tf!("Pseudo language {lang} created", lang = lang));
        }
        let msg = trn(n, "{n} pseudo translation created for {lang}", "{n} pseudo translations created for {lang}");
        self.info(susi_core::i18n::fmt(&msg, &[("lang", &lang)]));
        self.settings.pseudo = d.options.clone();
        let _ = self.settings.save();
        self.structure_changed();
    }

    fn export_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(t("Excel workbook"), &["xlsx"])
            .add_filter(t("CSV (semicolon)"), &["csv"])
            .add_filter(t("Text (tab separated)"), &["txt"])
            .add_filter("XLIFF", &["xlf", "xliff"])
            .add_filter("TMX", &["tmx"])
            .set_title(t("Export"))
            .save_file()
        else {
            return;
        };
        let path = if path.extension().is_none() { path.with_extension("xlsx") } else { path };
        let langs = self.project.as_ref().map(Project::language_ids).unwrap_or_default().into_iter().map(|l| (l, true)).collect();
        let d = ExportOptions::default();
        self.dialog = Some(Dialog::Export(ExportDialog {
            path,
            langs,
            only_view: false,
            original_column: d.original_column,
            status_columns: d.status_columns,
            comment_columns: d.comment_columns,
            row_status_column: d.row_status_column,
            row_comment_column: d.row_comment_column,
            max_chars_column: d.max_chars_column,
            include_locked: d.include_locked,
            label_lang: self.settings.export_label_language.clone(),
            screenshots: ScreenshotExport::from_keyword(&self.settings.export_screenshots).unwrap_or_default(),
        }));
    }

    pub fn export(&mut self, d: &ExportDialog) {
        // Remembered: exports for the same translators usually need the same language.
        let shots = d.screenshots.keyword();
        if self.settings.export_label_language != d.label_lang || self.settings.export_screenshots != shots {
            self.settings.export_label_language.clone_from(&d.label_lang);
            self.settings.export_screenshots = shots.to_string();
            let _ = self.settings.save();
        }
        let Some(p) = &self.project else {
            return;
        };
        let opts = ExportOptions {
            langs: d.langs.iter().filter(|(_, on)| *on).map(|(l, _)| l.clone()).collect(),
            rows: d.only_view.then(|| self.rows.clone()),
            original_column: d.original_column,
            status_columns: d.status_columns,
            comment_columns: d.comment_columns,
            row_status_column: d.row_status_column,
            row_comment_column: d.row_comment_column,
            max_chars_column: d.max_chars_column,
            include_locked: d.include_locked,
            label_lang: susi_core::i18n::Lang::from_code(&d.label_lang),
            screenshots: d.screenshots,
            ..Default::default()
        };
        match susi_core::exchange::export_file(p, &d.path, &opts, self.shots.as_ref()) {
            Ok(files) => {
                for f in files {
                    self.info(susi_core::tf!("Exported: {path}", path = f.display()));
                }
            }
            Err(e) => self.error(susi_core::tf!("Export {path}: {error}", path = d.path.display(), error = e)),
        }
    }

    fn import_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(t("Exchange files"), &["xlsx", "xls", "ods", "csv", "txt", "xlf", "xliff", "tmx"])
            .set_title(t("Import"))
            .pick_file()
        else {
            return;
        };
        let d = ImportOptions::default();
        self.dialog =
            Some(Dialog::Import(ImportDialog { path, overwrite: d.overwrite, check_original: d.check_original, comments: d.comments, status: d.status }));
    }

    pub fn import(&mut self, d: &ImportDialog) {
        self.commit_cell_edit();
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let opts = ImportOptions { overwrite: d.overwrite, check_original: d.check_original, comments: d.comments, status: d.status, ..Default::default() };
        match susi_core::exchange::import_file(p, &d.path, &opts) {
            Ok(rep) => {
                self.info(susi_core::tf!("Import {path}: {details}", path = d.path.display(), details = rep.summary()));
                self.structure_changed();
            }
            Err(e) => self.error(susi_core::tf!("Import {path}: {details}", path = d.path.display(), details = e)),
        }
    }
}
