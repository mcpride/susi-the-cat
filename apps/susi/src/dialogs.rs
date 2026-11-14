//! Modal dialogs.

use std::path::PathBuf;

use susi_core::formats::po::PoOptions;
use susi_core::formats::properties::PropertiesEncoding;
use susi_core::i18n::{Lang, t};
use susi_core::lang;
use susi_core::model::{RowRef, WriteMethod};
use susi_core::pseudo::{PseudoCase, PseudoKind, PseudoOptions};
use susi_core::settings::Settings;
use susi_core::status::{RowStatus, TranslationStatus};

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
    pub status_columns: bool,
    pub comment_columns: bool,
    pub row_status_column: bool,
    pub max_chars_column: bool,
    pub include_locked: bool,
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

pub enum Dialog {
    /// Unsaved changes; `then` runs after saving or discarding.
    Confirm {
        then: Action,
    },
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
    Settings(Box<Settings>),
    About,
    WorkflowFetch(FetchDialog),
    WorkflowSave(SaveDialog),
    WorkflowPublish(PublishDialog),
    WorkflowToken(TokenDialog),
}

/// Shows the open dialog (if any).
pub fn show(app: &mut SusiApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else {
        return;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("susi-dialog")).show(ctx, |ui| {
        ui.set_min_width(460.0);
        close = match &mut d {
            Dialog::Confirm { then } => confirm(app, ui, then),
            Dialog::Languages { add, remove } => languages(app, ui, add, remove),
            Dialog::AddSource { files, original } => add_source(app, ui, files, original),
            Dialog::SourceProps(s) => source_props(app, ui, s),
            Dialog::RemoveSource(i) => remove_source(app, ui, *i),
            Dialog::RowProps(r) => row_props(app, ui, r),
            Dialog::Export(e) => export(app, ui, e),
            Dialog::Import(i) => import(app, ui, i),
            Dialog::TmTranslate(t) => tm_translate(app, ui, t),
            Dialog::Pseudo(p) => pseudo(app, ui, p),
            Dialog::Settings(s) => settings(app, ui, s),
            Dialog::About => about(ui),
            Dialog::WorkflowFetch(f) => workflow_ui::fetch_dialog(app, ui, f),
            Dialog::WorkflowSave(s) => workflow_ui::save_dialog(app, ui, s),
            Dialog::WorkflowPublish(p) => workflow_ui::publish_dialog(app, ui, p),
            Dialog::WorkflowToken(t) => workflow_ui::token_dialog(app, ui, t),
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
            ui.selectable_value(value, code.to_string(), format!("{name} ({code})"));
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
    ui.strong(t("Languages"));
    ui.horizontal_wrapped(|ui| {
        for (l, on) in d.langs.iter_mut() {
            ui.checkbox(on, SusiApp::lang_label(l));
        }
    });
    ui.separator();
    ui.checkbox(&mut d.only_view, susi_core::tf!("Only rows of the current view ({count})", count = app.rows.len()));
    ui.checkbox(&mut d.status_columns, t("Status columns"));
    ui.checkbox(&mut d.comment_columns, t("Comment columns"));
    ui.checkbox(&mut d.row_status_column, t("Row status column"));
    ui.checkbox(&mut d.max_chars_column, t("\"Max. characters\" column"));
    ui.checkbox(&mut d.include_locked, t("Include \"Do not translate\" rows"));
    match ok_cancel(ui, t("Export"), d.langs.iter().any(|(_, on)| *on)) {
        Some(true) => {
            app.export(d);
            true
        }
        Some(false) => true,
        None => false,
    }
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
    ui.label(t("Supported sources: .NET RESX, JSON, PO/POT (gettext, with MO compilation), TSV (i18n catalogs), Java properties, Android string resources. Exchange: Excel, CSV, Text, XLIFF, TMX."));
    ui.label(t("Command line: susi-cli (SlMake compatible), AI integration: susi-cli mcp"));
    ui.separator();
    ui.small(t("Sisulizer is a product of Sisulizer Ltd. & Co KG. Susi is an independent new development."));
    ui.button(t("Close")).clicked()
}
