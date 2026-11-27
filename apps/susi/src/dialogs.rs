//! Modal dialogs.

use std::path::PathBuf;

use susi_core::custom_data::Excluded;
use susi_core::exchange::{ExchangeFormat, ScreenshotExport};
use susi_core::formats::po::PoOptions;
use susi_core::formats::properties::PropertiesEncoding;
use susi_core::i18n::{Lang, t, trn};
use susi_core::lang;
use susi_core::model::{RowRef, WriteMethod};
use susi_core::ops::CopyTargets;
use susi_core::pseudo::{PseudoCase, PseudoKind, PseudoOptions};
use susi_core::sds::{Exclusion, Scope};
use susi_core::settings::Settings;
use susi_core::status::{RowStatus, TranslationStatus};
use susi_core::validate;

use crate::actions::Action;
use crate::app::SusiApp;
use crate::workflow_ui::{self, FetchDialog, PublishDialog, SaveDialog, TokenDialog};

pub struct SourcePropsDialog {
    pub index: usize,
    pub name: String,
    pub kind: String,
    pub original: String,
    pub method: WriteMethod,
    pub dir: String,
    pub pattern: String,
    /// Options of a PO source.
    pub po: Option<PoOptions>,
    /// Encoding of a Java properties source.
    pub encoding: Option<PropertiesEncoding>,
    /// Android string resources (shows the Android placeholders).
    pub android: bool,
    /// Data types not scanned (RESX sources).
    pub excluded: Option<Excluded>,
}

pub struct RowPropsDialog {
    pub row: RowRef,
    pub note: String,
    pub max_chars: String,
    pub locked: bool,
    pub excluded: bool,
    pub marked: bool,
    pub status: RowStatus,
}

pub struct ExportDialog {
    pub path: PathBuf,
    pub langs: Vec<(String, bool)>,
    pub only_view: bool,
    pub original_column: bool,
    pub status_columns: bool,
    /// A translation comment column for every language.
    pub comment_columns: bool,
    pub row_status_column: bool,
    pub row_comment_column: bool,
    pub max_chars_column: bool,
    pub include_locked: bool,
    /// UI language of headers and status values (tables only): empty = user interface language.
    pub label_lang: String,
    /// Screenshots of the exported rows.
    pub screenshots: ScreenshotExport,
}

pub struct ImportDialog {
    pub path: PathBuf,
    pub overwrite: bool,
    pub check_original: bool,
    pub comments: bool,
    pub status: TranslationStatus,
}

pub struct TmDialog {
    pub overwrite: bool,
    pub fuzzy: bool,
    pub only_view: bool,
    pub only_lang: bool,
}

pub struct PseudoDialog {
    pub lang: String,
    pub options: PseudoOptions,
    pub overwrite: bool,
}

/// Copying the original or a language into the visible rows of a language column.
pub struct CopyDialog {
    /// Language copied from; `None` for the original.
    pub from: Option<String>,
    pub lang: String,
    /// Visible rows when the dialog was opened.
    pub rows: Vec<RowRef>,
    pub targets: CopyTargets,
    pub overwrite: bool,
}

/// Confirmation before clearing the translations of the visible rows of a language column.
pub struct ClearColumnDialog {
    pub lang: String,
    /// Visible rows with a translation in `lang`.
    pub rows: Vec<RowRef>,
}

pub enum Dialog {
    /// Unsaved changes; `then` runs after saving or discarding.
    Confirm {
        then: Action,
    },
    /// The project file was changed by another program: reload it?
    Reload,
    Languages {
        add: String,
        remove: Option<String>,
    },
    AddSource {
        files: Vec<PathBuf>,
        original: String,
    },
    SourceProps(SourcePropsDialog),
    RemoveSource(usize),
    RowProps(RowPropsDialog),
    Export(ExportDialog),
    Import(ImportDialog),
    TmTranslate(TmDialog),
    Pseudo(PseudoDialog),
    Copy(CopyDialog),
    ClearColumn(ClearColumnDialog),
    Settings(Box<Settings>),
    About,
    WorkflowFetch(FetchDialog),
    WorkflowSave(SaveDialog),
    WorkflowPublish(PublishDialog),
    WorkflowToken(TokenDialog),
    /// Screenshot manager.
    Screenshots,
    /// Excluded original or translation validations.
    Excluded(ExcludedDialog),
}

pub struct ExcludedDialog {
    pub scope: Scope,
    /// Index within the exclusions of `scope`.
    pub selected: Option<usize>,
}

/// Minimum width of the dialogs.
const DIALOG_WIDTH: f32 = 460.0;

/// Shows the open dialog (if any).
pub fn show(app: &mut SusiApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else {
        return;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("susi-dialog")).show(ctx, |ui| {
        ui.set_min_width(DIALOG_WIDTH);
        close = match &mut d {
            Dialog::Confirm { then } => confirm(app, ui, then),
            Dialog::Reload => reload(app, ui),
            Dialog::Languages { add, remove } => languages(app, ui, add, remove),
            Dialog::AddSource { files, original } => add_source(app, ui, files, original),
            Dialog::SourceProps(s) => source_props(app, ui, s),
            Dialog::RemoveSource(i) => remove_source(app, ui, *i),
            Dialog::RowProps(r) => row_props(app, ui, r),
            Dialog::Export(e) => export(app, ui, e),
            Dialog::Import(i) => import(app, ui, i),
            Dialog::TmTranslate(t) => tm_translate(app, ui, t),
            Dialog::Pseudo(p) => pseudo(app, ui, p),
            Dialog::Copy(c) => copy_cells(app, ui, c),
            Dialog::ClearColumn(c) => clear_column(app, ui, c),
            Dialog::Settings(s) => settings(app, ui, s),
            Dialog::About => about(ui),
            Dialog::WorkflowFetch(f) => workflow_ui::fetch_dialog(app, ui, f),
            Dialog::WorkflowSave(s) => workflow_ui::save_dialog(app, ui, s),
            Dialog::WorkflowPublish(p) => workflow_ui::publish_dialog(app, ui, p),
            Dialog::WorkflowToken(t) => workflow_ui::token_dialog(app, ui, t),
            Dialog::Screenshots => crate::shots::manager(app, ui),
            Dialog::Excluded(e) => excluded(app, ui, e),
        };
    });
    if modal.should_close() {
        close = true;
    }
    if !close && app.dialog.is_none() {
        app.dialog = Some(d);
    }
}

/// OK/Cancel row. Returns `Some(true)` for OK, `Some(false)` for cancel.
pub fn ok_cancel(ui: &mut egui::Ui, ok: &str, enabled: bool) -> Option<bool> {
    ui.separator();
    ui.horizontal(|ui| {
        if ui.add_enabled(enabled, egui::Button::new(ok)).clicked() {
            return Some(true);
        }
        if ui.button(t("Cancel")).clicked() {
            return Some(false);
        }
        None
    })
    .inner
}

fn lang_combo(ui: &mut egui::Ui, id: &str, value: &mut String) {
    egui::ComboBox::from_id_salt(id).width(260.0).selected_text(SusiApp::lang_label(value)).show_ui(ui, |ui| {
        for (code, name) in lang::known_languages() {
            ui.selectable_value(value, code.to_string(), format!("{name} [{code}]"));
        }
    });
}

fn confirm(app: &mut SusiApp, ui: &mut egui::Ui, then: &Action) -> bool {
    ui.heading(t("Unsaved changes"));
    ui.label(t("The project has been modified. Do you want to save the changes?"));
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(t("Save")).clicked() {
            if app.save() {
                app.skip_guard = true;
                app.actions.push(then.clone());
            }
            return true;
        }
        if ui.button(t("Don't save")).clicked() {
            app.skip_guard = true;
            app.actions.push(then.clone());
            return true;
        }
        ui.button(t("Cancel")).clicked()
    })
    .inner
}

/// The project file was changed by another program (Sisulizer, susi-cli, an AI agent, git).
/// Closing the dialog keeps the open project; it is asked again when the file changes again.
fn reload(app: &mut SusiApp, ui: &mut egui::Ui) -> bool {
    let Some(path) = app.path().map(std::path::Path::to_path_buf) else {
        return true;
    };
    let unsaved = app.has_unsaved_changes();
    ui.heading(t("Project changed"));
    ui.label(susi_core::tf!("The project file {path} has been changed by another program. Do you want to reload it?", path = path.display()));
    if unsaved {
        ui.colored_label(ui.visuals().error_fg_color, t("The project has unsaved changes. They will be lost when it is reloaded."));
    }
    let backup = path.with_extension("~slp");
    let backup = backup.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    ui.small(susi_core::tf!(
        "If the open version is kept, saving it overwrites the changes made by the other program (the replaced file is kept as {backup}).",
        backup = backup
    ));
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(if unsaved { t("Discard changes and reload") } else { t("Reload") }).clicked() {
            app.reload();
            return true;
        }
        ui.button(t("Don't reload")).clicked()
    })
    .inner
}

fn languages(app: &mut SusiApp, ui: &mut egui::Ui, add: &mut String, remove: &mut Option<String>) -> bool {
    ui.heading(t("Languages"));
    let Some(p) = app.project.as_mut() else {
        return true;
    };
    let originals: Vec<String> = p.sources.iter().map(|s| s.original().to_string()).collect();
    if !originals.is_empty() {
        let list = dedup(originals).iter().map(|l| SusiApp::lang_label(l)).collect::<Vec<_>>().join(", ");
        ui.label(susi_core::tf!("Original language(s): {langs}", langs = list));
    }
    egui::Grid::new("langs").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong(t("ID"));
        ui.strong(t("Language"));
        ui.strong(t("Kind"));
        ui.label("");
        ui.end_row();
        for l in &p.languages {
            ui.label(l.id());
            ui.label(lang::name(l.id()));
            ui.label(if l.is_pseudo() { t("Pseudo") } else { t("Target") });
            if ui.small_button(t("Remove")).clicked() {
                *remove = Some(l.id().to_string());
            }
            ui.end_row();
        }
    });
    let mut changed = false;
    if let Some(r) = remove.clone() {
        ui.colored_label(egui::Color32::RED, susi_core::tf!("Remove language {lang} with all translations?", lang = SusiApp::lang_label(&r)));
        ui.horizontal(|ui| {
            if ui.button(t("Yes, remove")).clicked() {
                p.remove_language(&r);
                *remove = None;
                changed = true;
            }
            if ui.button(t("No")).clicked() {
                *remove = None;
            }
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(t("Add:"));
        lang_combo(ui, "add-lang", add);
        ui.label(t("ID:"));
        ui.add(egui::TextEdit::singleline(add).desired_width(70.0));
        if ui.add_enabled(!add.trim().is_empty(), egui::Button::new(t("Add"))).clicked() && p.add_language(add.trim()) {
            add.clear();
            changed = true;
        }
    });
    if changed {
        let ids = p.language_ids();
        if app.lang.as_ref().is_none_or(|l| !ids.contains(l)) {
            app.lang = ids.first().cloned();
        }
        app.structure_changed();
    }
    ui.separator();
    ui.button(t("Close")).clicked()
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

fn add_source(app: &mut SusiApp, ui: &mut egui::Ui, files: &[PathBuf], original: &mut String) -> bool {
    ui.heading(t("Add source"));
    for f in files.iter() {
        ui.label(format!("📄 {}", f.display()));
    }
    ui.horizontal(|ui| {
        ui.label(t("Original language:"));
        lang_combo(ui, "src-orig", original);
        ui.add(egui::TextEdit::singleline(original).desired_width(70.0));
    });
    ui.label(t("The files are stored relative to the project directory and scanned immediately."));
    match ok_cancel(ui, t("Add"), !original.trim().is_empty() && !files.is_empty()) {
        Some(true) => {
            app.add_sources(files, original.trim());
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn source_props(app: &mut SusiApp, ui: &mut egui::Ui, s: &mut SourcePropsDialog) -> bool {
    ui.heading(t("Source properties"));
    egui::Grid::new("srcprops").num_columns(2).show(ui, |ui| {
        ui.label(t("File:"));
        ui.label(&s.name);
        ui.end_row();
        ui.label(t("Type:"));
        ui.label(&s.kind);
        ui.end_row();
        ui.label(t("Original language:"));
        ui.add(egui::TextEdit::singleline(&mut s.original).desired_width(80.0));
        ui.end_row();
        ui.label(t("Localized files contain:"));
        egui::ComboBox::from_id_salt("method").selected_text(s.method.label()).show_ui(ui, |ui| {
            for m in [WriteMethod::Different, WriteMethod::Translated, WriteMethod::All] {
                ui.selectable_value(&mut s.method, m, m.label());
            }
        });
        ui.end_row();
        ui.label(t("Output directory:"));
        ui.text_edit_singleline(&mut s.dir);
        ui.end_row();
        ui.label(t("File pattern:"));
        ui.text_edit_singleline(&mut s.pattern);
        ui.end_row();
        if let Some(enc) = &mut s.encoding {
            ui.label(t("Encoding:"));
            egui::ComboBox::from_id_salt("encoding").selected_text(enc.label()).show_ui(ui, |ui| {
                for e in PropertiesEncoding::ALL {
                    ui.selectable_value(enc, e, e.label());
                }
            });
            ui.end_row();
        }
    });
    ui.small(t("Placeholders: <sl> language ID, <iso>, <net>, <file> file name, <body> name without extension, <ext> extension"));
    if s.android {
        ui.small(t("Android: <parent> resource directory, <android> BCP 47 qualifier for values-b+… (pt+BR), <androidr> legacy qualifier (pt-rBR)"));
    }
    if let Some(po) = &mut s.po {
        po_options(ui, po);
    }
    if let Some(e) = &mut s.excluded {
        ui.separator();
        ui.strong(t("Excluded data types"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut e.images, t("Pictures"));
            ui.checkbox(&mut e.binary, t("Binary data"));
            ui.checkbox(&mut e.text, t("Text files"));
        });
        ui.small(t("Takes effect on the next scan; rows no longer scanned become unused."));
    }
    match ok_cancel(ui, "OK", !s.original.trim().is_empty()) {
        Some(true) => {
            if let Some(src) = app.project.as_mut().and_then(|p| p.sources.get_mut(s.index)) {
                src.attrs.set("original", s.original.trim());
                src.set_write_method(s.method);
                src.set_output_pattern(s.dir.trim(), s.pattern.trim());
                if let Some(po) = &s.po {
                    src.set_po_options(po);
                }
                if let Some(enc) = s.encoding {
                    src.set_properties_encoding(enc);
                }
                if let Some(e) = s.excluded {
                    src.set_excluded_data(e);
                }
                app.touched();
            }
            true
        }
        Some(false) => true,
        None => false,
    }
}

/// PO source options ("PO Source | Options").
fn po_options(ui: &mut egui::Ui, po: &mut PoOptions) {
    ui.separator();
    ui.strong(t("PO options"));
    ui.checkbox(&mut po.scan_comments, t("Scan comments"));
    ui.checkbox(&mut po.msgstr_as_original, t("Use msgstr value as original"));
    ui.checkbox(&mut po.compile, t("Compile created PO files to binary MO files"));
    ui.add_enabled_ui(po.compile, |ui| {
        ui.checkbox(&mut po.include_empty, t("Include empty translations into MO files"));
        ui.horizontal(|ui| {
            ui.label(t("Byte order of MO files:"));
            ui.radio_value(&mut po.big_endian, false, t("Little endian"));
            ui.radio_value(&mut po.big_endian, true, t("Big endian"));
        });
    });
    ui.horizontal(|ui| {
        ui.label(t("Plural forms:"));
        ui.add(egui::DragValue::new(&mut po.plural_rows).range(1..=PoOptions::MAX_PLURAL_ROWS));
    });
    ui.small(t("Rows per plural message: 2 for Western and Asian languages, 3 for most languages, 4 for all but Arabic, 6 for Arabic."));
    ui.small(t("Scan options take effect at the next scan."));
}

fn remove_source(app: &mut SusiApp, ui: &mut egui::Ui, i: usize) -> bool {
    let name = app.project.as_ref().and_then(|p| p.sources.get(i)).map(|s| s.name().to_string()).unwrap_or_default();
    ui.heading(t("Remove source"));
    ui.label(susi_core::tf!("Remove source \"{name}\" with all rows and translations from the project?", name = name));
    match ok_cancel(ui, t("Remove"), true) {
        Some(true) => {
            if let Some(p) = app.project.as_mut().filter(|p| i < p.sources.len()) {
                p.sources.remove(i);
                app.tree = None;
                app.selected = None;
                app.cell_edit = None;
                app.set_issues(Vec::new());
                app.structure_changed();
                app.info(susi_core::tf!("Source removed: {name}", name = name));
            }
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn row_props(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut RowPropsDialog) -> bool {
    ui.heading(t("Row properties"));
    let (context, original) =
        app.project.as_ref().map(|p| (p.context(&d.row), p.row(&d.row).map(|r| r.original().to_string()).unwrap_or_default())).unwrap_or_default();
    egui::Grid::new("rowprops").num_columns(2).show(ui, |ui| {
        ui.label(t("Context:"));
        ui.label(&context);
        ui.end_row();
        ui.label(t("Original:"));
        ui.label(&original);
        ui.end_row();
        ui.label(t("Row status:"));
        egui::ComboBox::from_id_salt("rowstatus").selected_text(d.status.label()).show_ui(ui, |ui| {
            for s in RowStatus::ALL {
                ui.selectable_value(&mut d.status, s, s.label());
            }
        });
        ui.end_row();
        ui.label(t("Max. characters:"));
        ui.add(egui::TextEdit::singleline(&mut d.max_chars).desired_width(60.0).hint_text(t("unlimited")));
        ui.end_row();
    });
    ui.label(t("Row comment:"));
    ui.add(egui::TextEdit::multiline(&mut d.note).desired_rows(3).desired_width(f32::INFINITY));
    ui.checkbox(&mut d.locked, t("Do not translate"));
    ui.checkbox(&mut d.excluded, t("Excluded"));
    ui.checkbox(&mut d.marked, t("Marked"));
    let max_ok = d.max_chars.trim().is_empty() || d.max_chars.trim().parse::<usize>().is_ok();
    match ok_cancel(ui, "OK", max_ok) {
        Some(true) => {
            if let Some(r) = app.project.as_mut().and_then(|p| p.row_mut(&d.row)) {
                r.set_note(d.note.trim_end());
                r.set_max_chars(d.max_chars.trim().parse().unwrap_or(0));
                r.set_locked(d.locked);
                r.set_excluded(d.excluded);
                r.set_marked(d.marked);
                r.set_status(d.status);
                app.structure_changed();
            }
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn export(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut ExportDialog) -> bool {
    ui.heading(t("Export"));
    ui.label(susi_core::tf!("File: {path}", path = d.path.display()));
    ui.label(t("The format is determined by the file extension (xlsx, csv, txt, xlf, tmx)."));
    ui.separator();
    export_languages(ui, &mut d.langs);
    ui.separator();
    let table = matches!(ExchangeFormat::from_path(&d.path), Some(ExchangeFormat::Excel | ExchangeFormat::Csv | ExchangeFormat::Text));
    ui.checkbox(&mut d.only_view, susi_core::tf!("Only rows of the current view ({count})", count = app.rows.len()));
    ui.checkbox(&mut d.original_column, t("\"Original\" column"));
    // XLIFF always contains the original.
    if table && !d.original_column {
        ui.weak(t("Without the original, an import cannot recognize changed originals."));
    }
    ui.checkbox(&mut d.status_columns, t("Status columns"));
    ui.checkbox(&mut d.comment_columns, t("Translation comment columns"));
    ui.checkbox(&mut d.row_status_column, t("Row status column"));
    ui.checkbox(&mut d.row_comment_column, t("Row comment column"));
    ui.checkbox(&mut d.max_chars_column, t("\"Max. characters\" column"));
    ui.checkbox(&mut d.include_locked, t("Include \"Do not translate\" rows"));
    // XLIFF and TMX have no labels of their own.
    if table {
        ui.horizontal(|ui| {
            ui.label(t("Language of column headers and status values:"));
            label_language_combo(ui, &mut d.label_lang);
        });
    }
    if app.shots.as_ref().is_some_and(|s| !s.is_empty()) && ExchangeFormat::from_path(&d.path) != Some(ExchangeFormat::Tmx) {
        ui.horizontal(|ui| {
            ui.label(t("Screenshots:"));
            egui::ComboBox::from_id_salt("exp-shots").selected_text(screenshot_export_label(d.screenshots)).show_ui(ui, |ui| {
                for m in ScreenshotExport::ALL {
                    ui.selectable_value(&mut d.screenshots, m, screenshot_export_label(m));
                }
            });
        });
    }
    match ok_cancel(ui, t("Export"), d.langs.iter().any(|(_, on)| *on)) {
        Some(true) => {
            app.export(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

/// Languages to export: a checkbox above the list selects or clears all of them (partly selected:
/// indeterminate, a click selects all). The list is a scrollable grid with as many columns as fit
/// into the dialog.
fn export_languages(ui: &mut egui::Ui, langs: &mut [(String, bool)]) {
    let selected = langs.iter().filter(|(_, on)| *on).count();
    let mut all = !langs.is_empty() && selected == langs.len();
    let partly = selected > 0 && !all;
    ui.horizontal(|ui| {
        let toggle = egui::Checkbox::new(&mut all, egui::RichText::new(t("All languages")).strong()).indeterminate(partly);
        if ui.add_enabled(!langs.is_empty(), toggle).changed() {
            for (_, on) in langs.iter_mut() {
                *on = all;
            }
        }
        ui.weak(susi_core::tf!("{selected} of {count} selected", selected = selected, count = langs.len()));
    });
    let labels: Vec<String> = langs.iter().map(|(l, _)| SusiApp::lang_label(l)).collect();
    let frame = egui::Frame::group(ui.style());
    // The list fills the dialog without widening it. In the first (sizing) pass egui offers its
    // default area size, so the dialog would never become narrower than that.
    let outer = if ui.is_sizing_pass() { DIALOG_WIDTH } else { ui.available_width() };
    let spacing = ui.spacing();
    let width = outer - frame.total_margin().sum().x - spacing.scroll.allocated_width();
    let gap = spacing.item_spacing.x;
    let item = spacing.icon_width
        + spacing.icon_spacing
        + labels
            .iter()
            .map(|l| egui::WidgetText::from(l).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x)
            .fold(0.0, f32::max);
    let columns = grid_columns(width, item, gap, labels.len());
    frame.show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("exp-langs").max_height(180.0).show(ui, |ui| {
            ui.set_min_width(width);
            egui::Grid::new("exp-langs").num_columns(columns).min_col_width(item).show(ui, |ui| {
                for (i, ((_, on), label)) in langs.iter_mut().zip(labels).enumerate() {
                    ui.checkbox(on, label);
                    if i % columns == columns - 1 {
                        ui.end_row();
                    }
                }
            });
        });
    });
}

/// Columns of `item` width (`gap` between them) that fit into `width`: at least 1, at most `count`.
pub(crate) fn grid_columns(width: f32, item: f32, gap: f32, count: usize) -> usize {
    let fit = ((width + gap) / (item + gap).max(1.0)).floor();
    // `as` saturates; a negative width gives 0.
    (fit as usize).clamp(1, count.max(1))
}

fn screenshot_export_label(m: ScreenshotExport) -> &'static str {
    match m {
        ScreenshotExport::Auto => t("Automatic (embedded in Excel, otherwise as links)"),
        ScreenshotExport::None => t("None"),
        ScreenshotExport::Link => t("As links (images copied next to the export)"),
        ScreenshotExport::Embed => t("Embedded (Excel, second worksheet)"),
    }
}

/// Language of exported labels: empty (as the user interface) or a UI language code.
fn label_language_combo(ui: &mut egui::Ui, value: &mut String) {
    let as_ui = susi_core::tf!("As the user interface ({lang})", lang = susi_core::i18n::current().name());
    let text = Lang::all().find(|l| l.code() == value.as_str()).map_or_else(|| as_ui.clone(), |l| l.name().to_string());
    egui::ComboBox::from_id_salt("exp-label-lang").selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), as_ui);
        for l in Lang::all() {
            ui.selectable_value(value, l.code().to_string(), l.name());
        }
    });
}

fn status_combo(ui: &mut egui::Ui, id: &str, value: &mut TranslationStatus) {
    egui::ComboBox::from_id_salt(id).selected_text(value.label()).show_ui(ui, |ui| {
        for s in TranslationStatus::ALL {
            ui.selectable_value(value, s, s.label());
        }
    });
}

fn import(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut ImportDialog) -> bool {
    ui.heading(t("Import"));
    ui.label(susi_core::tf!("File: {path}", path = d.path.display()));
    ui.separator();
    ui.checkbox(&mut d.overwrite, t("Overwrite existing translations"));
    ui.checkbox(&mut d.check_original, t("Skip rows whose original has changed"));
    ui.checkbox(&mut d.comments, t("Import comments"));
    ui.horizontal(|ui| {
        ui.label(t("Status for imported texts without a status:"));
        status_combo(ui, "imp-status", &mut d.status);
    });
    match ok_cancel(ui, t("Import"), true) {
        Some(true) => {
            app.import(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn tm_translate(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut TmDialog) -> bool {
    ui.heading(t("Translate with translation memory"));
    ui.label(susi_core::tf!("Translation memory: {details}", details = app.settings.tm_path().display()));
    ui.separator();
    ui.checkbox(&mut d.overwrite, t("Replace existing translations"));
    ui.checkbox(&mut d.fuzzy, susi_core::tf!("Accept fuzzy matches from {percent} % as \"Best guess\"", percent = app.settings.fuzzy_threshold));
    ui.checkbox(&mut d.only_view, susi_core::tf!("Only rows of the current view ({count})", count = app.rows.len()));
    let lang = app.lang.clone().unwrap_or_default();
    ui.add_enabled(
        !lang.is_empty(),
        egui::Checkbox::new(&mut d.only_lang, susi_core::tf!("Only current language ({lang})", lang = SusiApp::lang_label(&lang))),
    );
    match ok_cancel(ui, t("Translate"), true) {
        Some(true) => {
            app.tm_translate(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn pseudo(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut PseudoDialog) -> bool {
    ui.heading(t("Pseudo translation"));
    ui.horizontal(|ui| {
        ui.label(t("Language:"));
        ui.add(egui::TextEdit::singleline(&mut d.lang).desired_width(80.0));
        ui.small(t("(created as pseudo language if necessary)"));
    });
    ui.horizontal(|ui| {
        ui.label(t("Kind:"));
        egui::ComboBox::from_id_salt("pseudo-kind").selected_text(d.options.kind.label()).show_ui(ui, |ui| {
            for k in [PseudoKind::Emulation, PseudoKind::Replace, PseudoKind::Reduce, PseudoKind::MaxTranslation] {
                ui.selectable_value(&mut d.options.kind, k, k.label());
            }
        });
    });
    ui.checkbox(&mut d.options.diacritics, t("Diacritics"));
    ui.checkbox(&mut d.options.expand, t("Expand text"));
    ui.checkbox(&mut d.options.enclose, t("Enclose in delimiters"));
    ui.horizontal(|ui| {
        ui.label(t("Case:"));
        ui.selectable_value(&mut d.options.case, PseudoCase::NoChange, t("No change"));
        ui.selectable_value(&mut d.options.case, PseudoCase::Alternate, t("Alternating"));
        ui.selectable_value(&mut d.options.case, PseudoCase::Upper, t("Upper case"));
    });
    ui.checkbox(&mut d.overwrite, t("Overwrite existing translations"));
    let sample = susi_core::pseudo::pseudo_text("Save changes to {0}?", &d.options);
    ui.label(susi_core::tf!("Sample: {sample}", sample = sample));
    match ok_cancel(ui, t("Create"), !d.lang.trim().is_empty()) {
        Some(true) => {
            app.fill_pseudo(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn copy_cells(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut CopyDialog) -> bool {
    let lang = SusiApp::lang_label(&d.lang);
    let title = match &d.from {
        None => {
            ui.heading(t("Copy original"));
            ui.label(susi_core::tf!("Copies the original text into the visible rows of the column {lang}.", lang = lang));
            t("Copy original")
        }
        Some(from) => {
            ui.heading(t("Copy language"));
            ui.label(susi_core::tf!(
                "Copies the translations of {from} into the visible rows of the column {lang}.",
                from = SusiApp::lang_label(from),
                lang = lang
            ));
            t("Copy language")
        }
    };
    ui.label(susi_core::tf!("Empty cells: {empty}, cells with a translation: {translated}", empty = d.targets.empty, translated = d.targets.translated));
    ui.add_enabled(d.targets.translated > 0, egui::Checkbox::new(&mut d.overwrite, t("Overwrite existing translations")));
    ui.small(t("Rows marked \"Do not translate\" and excluded rows are skipped."));
    match ok_cancel(ui, title, d.targets.count(d.overwrite) > 0) {
        Some(true) => {
            app.copy_in_column(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn clear_column(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut ClearColumnDialog) -> bool {
    ui.heading(t("Clear translations"));
    ui.label(susi_core::tf!(
        "Delete {count} in the visible rows of the column {lang}?",
        count = trn(d.rows.len(), "{n} translation", "{n} translations"),
        lang = SusiApp::lang_label(&d.lang)
    ));
    ui.label(t("Their comments are deleted as well. This cannot be undone."));
    match ok_cancel(ui, t("Clear translations"), true) {
        Some(true) => {
            app.clear_column(d);
            true
        }
        Some(false) => true,
        None => false,
    }
}

fn settings(app: &mut SusiApp, ui: &mut egui::Ui, s: &mut Settings) -> bool {
    ui.heading(t("Options"));
    let mut tm = s.tm_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
        ui.label(t("Translation memory (TMX):"));
        ui.horizontal(|ui| {
            let hint = susi_core::tm::TranslationMemory::default_path().display().to_string();
            if ui.add(egui::TextEdit::singleline(&mut tm).desired_width(300.0).hint_text(hint)).changed() {
                s.tm_path = (!tm.trim().is_empty()).then(|| PathBuf::from(tm.trim()));
            }
            if ui.button("…").clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("TMX", &["tmx"]).set_title(t("Translation memory")).save_file()
            {
                s.tm_path = Some(p);
            }
        });
        ui.end_row();
        ui.label(t("Minimum fuzzy match:"));
        ui.add(egui::Slider::new(&mut s.fuzzy_threshold, 50..=100).suffix(" %"));
        ui.end_row();
        ui.label(t("User interface language:"));
        ui_language_combo(ui, &mut s.ui_language);
        ui.end_row();
    });
    if s.ui_language != app.settings.ui_language {
        // Applies and saves the language immediately, independently of OK/Cancel.
        app.set_ui_language(&s.ui_language);
    }
    ui.checkbox(&mut s.auto_save_to_tm, t("Automatically save translated texts to the translation memory when saving"));
    ui.checkbox(&mut s.show_id_column, t("Show \"Context\" column"));
    ui.checkbox(&mut s.show_comment_column, t("Show \"Row comment\" column"));
    ui.checkbox(&mut s.show_date_column, t("Show \"Translated on\" column"));
    ui.checkbox(&mut s.dark_mode, t("Dark theme"));
    match ok_cancel(ui, "OK", true) {
        Some(true) => {
            if s.tm_path != app.settings.tm_path {
                app.tm = None;
            }
            app.settings = s.clone();
            app.apply_theme(ui.ctx());
            if let Err(e) = app.settings.save() {
                app.error(susi_core::tf!("Settings: {error}", error = e));
            }
            true
        }
        Some(false) => true,
        None => false,
    }
}

/// Interface language preference: "auto" or a language code (languages shown by their native name).
fn ui_language_combo(ui: &mut egui::Ui, pref: &mut String) {
    let text = match Lang::all().find(|l| l.code() == pref.as_str()) {
        Some(l) => l.name().to_string(),
        None => t("Automatic (system language)").to_string(),
    };
    egui::ComboBox::from_id_salt("ui-language").width(260.0).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(pref, "auto".to_string(), t("Automatic (system language)"));
        for l in Lang::all() {
            ui.selectable_value(pref, l.code().to_string(), l.name());
        }
    });
}

/// Excluded validations of one scope (like Sisulizer's *Excluded Translation Validations*): check,
/// context and original of each; *Remove* and *Remove all* bring the findings back.
fn excluded(app: &mut SusiApp, ui: &mut egui::Ui, d: &mut ExcludedDialog) -> bool {
    ui.heading(match d.scope {
        Scope::Original => t("Excluded original validations"),
        Scope::Translation => t("Excluded translation validations"),
    });
    let list: Vec<Exclusion> = app.project.as_ref().map(|p| p.desktop.exclusions_of(d.scope).cloned().collect()).unwrap_or_default();
    if d.selected.is_some_and(|i| i >= list.len()) {
        d.selected = None;
    }
    if list.is_empty() {
        ui.label(t("No excluded validations. Exclude a finding with the context menu of the validation results."));
    } else {
        egui::ScrollArea::both().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("excluded").num_columns(3).striped(true).show(ui, |ui| {
                ui.strong(t("Check"));
                ui.strong(t("Context"));
                ui.strong(t("Original"));
                ui.end_row();
                for (i, e) in list.iter().enumerate() {
                    let sel = d.selected == Some(i);
                    if ui.selectable_label(sel, validate::class_label(&e.class)).clicked() {
                        d.selected = Some(i);
                    }
                    if ui.selectable_label(sel, &e.context).clicked() {
                        d.selected = Some(i);
                    }
                    if ui.selectable_label(sel, e.native.replace('\n', " ¶ ").replace('\r', "")).clicked() {
                        d.selected = Some(i);
                    }
                    ui.end_row();
                }
            });
        });
    }
    ui.separator();
    let mut close = false;
    let mut changed = false;
    ui.horizontal(|ui| {
        let selected = d.selected.and_then(|i| list.get(i));
        if ui.add_enabled(selected.is_some(), egui::Button::new(t("Remove"))).clicked()
            && let (Some(e), Some(p)) = (selected, app.project.as_mut())
        {
            changed = p.desktop.remove(e);
            d.selected = None;
        }
        if ui.add_enabled(!list.is_empty(), egui::Button::new(t("Remove all"))).clicked()
            && let Some(p) = app.project.as_mut()
        {
            changed = p.desktop.clear(d.scope) > 0;
            d.selected = None;
        }
        close = ui.button(t("Close")).clicked();
    });
    if changed {
        app.touched();
        // The findings of removed exclusions come back: validate again if results are shown.
        if let Some(missing) = app.last_validation.filter(|_| !app.issues.is_empty()) {
            app.actions.push(Action::Validate { missing });
        }
    }
    close
}

fn about(ui: &mut egui::Ui) -> bool {
    let logo = crate::app_icon::logo(ui.ctx());
    ui.horizontal(|ui| {
        ui.add(egui::Image::from_texture(&logo).fit_to_exact_size(egui::vec2(64.0, 64.0)));
        ui.vertical(|ui| {
            ui.heading("Susi the CAT");
            ui.label(susi_core::tf!("Version {version}", version = env!("CARGO_PKG_VERSION")));
        });
    });
    ui.label(t("Localization tool for Susi/Sisulizer projects (.slp)."));
    ui.label(t("Supported sources: .NET RESX/RESW, JSON, PO/POT (gettext, with MO compilation), TSV (i18n catalogs), Java properties, Android string resources, Flutter ARB, WebExtension messages, Qt Linguist TS. Exchange: Excel, CSV, Text, XLIFF, TMX."));
    ui.label(t("Command line: susi-cli (SlMake compatible), AI integration: susi-cli mcp"));
    ui.separator();
    ui.small(t("Sisulizer is a product of Sisulizer Ltd. & Co KG. Susi is an independent new development."));
    ui.button(t("Close")).clicked()
}
