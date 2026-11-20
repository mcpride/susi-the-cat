//! Pictures and binary data (Sisulizer custom data) in the GUI: the editor tab of a data cell
//! (Sisulizer: *Picture Translation*), commands to load, link and save files, cell texts and the
//! refresh of linked files.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use egui::{RichText, Ui};
use susi_core::custom_data::{self, DataKind, PictureFormat, PictureInfo};
use susi_core::i18n::t;
use susi_core::model::{Project, Row, RowRef};
use susi_core::ops;

use crate::actions::Action;
use crate::app::SusiApp;

#[derive(Debug, Clone, PartialEq)]
pub enum DataCmd {
    /// Loads the translation of the selected cell from a file; `link` keeps it linked.
    Load { link: bool },
    /// Removes the file link of the selected cell (the data stays).
    Unlink,
    /// Saves the original or, with `translation`, the translation of the selected cell.
    SaveAs { translation: bool },
}

/// Textures of pictures and originals of binary data read from source files.
#[derive(Default)]
pub struct DataUi {
    textures: HashMap<u64, Option<egui::TextureHandle>>,
    originals: HashMap<RowRef, Option<Vec<u8>>>,
}

impl DataUi {
    /// Forgets cached data (another project, a scan).
    pub fn clear(&mut self) {
        self.textures.clear();
        self.originals.clear();
    }

    fn original(&mut self, p: &Project, r: &RowRef) -> Option<Vec<u8>> {
        self.originals.entry(r.clone()).or_insert_with(|| ops::data_original(p, r)).clone()
    }

    fn texture(&mut self, ctx: &egui::Context, bytes: &[u8]) -> Option<egui::TextureHandle> {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        let key = h.finish();
        self.textures
            .entry(key)
            .or_insert_with(|| {
                let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                Some(ctx.load_texture(format!("data-{key:x}"), egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::NEAREST))
            })
            .clone()
    }

    #[cfg(test)]
    pub fn texture_count(&self) -> usize {
        self.textures.values().flatten().count()
    }
}

/// Text of a data cell in the sheet: the original (`lang` = `None`) or the translation.
pub fn cell_text(row: &Row, lang: Option<&str>) -> String {
    let Some(l) = lang else {
        return match row.original_bytes() {
            Some(b) => custom_data::summary(&b),
            None => row.data_kind().map(DataKind::label).unwrap_or_default().to_string(),
        };
    };
    let text = row.translation_bytes(l).map(|b| custom_data::summary(&b)).unwrap_or_default();
    match row.translation(l).and_then(|tr| tr.linked_file()) {
        Some(_) => format!("🔗 {text}"),
        None => text,
    }
}

/// Picture, or hex dump of other data; `missing` when there is no data.
fn preview(state: &mut DataUi, ui: &mut Ui, bytes: Option<&[u8]>, missing: &str) {
    let Some(b) = bytes else {
        ui.weak(missing);
        return;
    };
    ui.label(custom_data::summary(b));
    if PictureInfo::of(b).is_none() {
        let mut lines: Vec<String> = b
            .chunks(16)
            .take(16)
            .enumerate()
            .map(|(i, c)| format!("{:04X}  {}", i * 16, c.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")))
            .collect();
        if b.len() > 256 {
            lines.push("…".into());
        }
        ui.label(RichText::new(lines.join("\n")).monospace());
        return;
    }
    match state.texture(ui.ctx(), b) {
        Some(tex) => {
            let size = tex.size_vec2();
            // Small icons are enlarged (up to 8×), large pictures fit into the column.
            let k = ((ui.available_width() - 8.0).max(32.0) / size.x).min(220.0 / size.y).min(8.0);
            ui.image((tex.id(), size * k));
        }
        None => {
            ui.weak(t("The picture cannot be shown."));
        }
    }
}

/// Editor tab of a picture or binary cell.
pub fn editor(app: &mut SusiApp, ui: &mut Ui, r: &RowRef, lang: &str) {
    let Some(p) = app.project.as_ref() else { return };
    let Some(row) = p.row(r) else { return };
    let original = app.data_ui.original(p, r);
    let translation = row.translation_bytes(lang);
    let linked = row.translation(lang).and_then(|tr| tr.linked_file()).map(str::to_string);
    let orig_lang = p.original_lang(r).to_string();
    let editable = !row.locked() && orig_lang != lang;
    let missing_original = match row.attrs.get("thumbnail") {
        Some(th) => susi_core::tf!("Binary data, only a fingerprint is stored ({thumbnail})", thumbnail = th),
        None => t("No data").to_string(),
    };
    let mut acts = Vec::new();
    let mut button = |ui: &mut Ui, text: &str, enabled: bool, a: Action| {
        if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
            acts.push(a);
        }
    };
    ui.columns(2, |cols| {
        cols[0].label(RichText::new(susi_core::tf!("Original ({lang})", lang = orig_lang)).strong());
        preview(&mut app.data_ui, &mut cols[0], original.as_deref(), &missing_original);
        button(&mut cols[0], t("Save as…"), original.is_some(), Action::Data(DataCmd::SaveAs { translation: false }));

        cols[1].label(RichText::new(susi_core::tf!("Translation – {lang}", lang = SusiApp::lang_label(lang))).strong());
        preview(&mut app.data_ui, &mut cols[1], translation.as_deref(), t("Not translated."));
        if let Some(f) = &linked {
            cols[1].label(format!("🔗 {f}")).on_hover_text(t("Linked: a changed file is read again when the project is opened, scanned or built."));
        }
        cols[1].horizontal_wrapped(|ui| {
            button(ui, t("Load…"), editable, Action::Data(DataCmd::Load { link: false }));
            button(ui, t("Link to file…"), editable, Action::Data(DataCmd::Load { link: true }));
            button(ui, t("Remove file link"), linked.is_some(), Action::Data(DataCmd::Unlink));
            button(ui, t("Save as…"), translation.is_some(), Action::Data(DataCmd::SaveAs { translation: true }));
            button(ui, t("Clear translation"), editable && translation.is_some(), Action::ClearTranslation);
        });
    });
    app.actions.extend(acts);
}

/// Runs a command on the selected cell.
pub fn run(app: &mut SusiApp, cmd: DataCmd) {
    let (Some(r), Some(lang)) = (app.selected.clone(), app.lang.clone()) else {
        app.warn(t("No row or language selected"));
        return;
    };
    let Some(p) = app.project.as_ref() else { return };
    let Some(row) = p.row(&r).filter(|row| row.is_data()) else {
        app.warn(t("The row contains no picture or binary data"));
        return;
    };
    match cmd {
        DataCmd::Load { link } => {
            if p.original_lang(&r) == lang || row.locked() {
                app.warn(t("The cell cannot be translated"));
                return;
            }
            let title = if link { t("Link translation to file") } else { t("Load translation from file") };
            if let Some(file) = rfd::FileDialog::new().set_title(title).pick_file() {
                load_file(app, &r, &lang, &file, link);
            }
        }
        DataCmd::Unlink => {
            let removed = app.project.as_mut().is_some_and(|p| custom_data::unlink_translation(p, &r, &lang));
            if removed {
                app.touched();
                app.info(t("File link removed"));
            } else {
                app.warn(t("The translation is not linked to a file"));
            }
        }
        DataCmd::SaveAs { translation } => {
            let id = row.id().to_string();
            let bytes = if translation { row.translation_bytes(&lang) } else { app.data_ui.original(p, &r) };
            let Some(bytes) = bytes else {
                app.warn(t("No data to save"));
                return;
            };
            let ext = PictureFormat::of(&bytes).map_or("bin", PictureFormat::extension);
            let name = if translation { format!("{id}.{lang}.{ext}") } else { format!("{id}.{ext}") };
            if let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() {
                match std::fs::write(&path, &bytes) {
                    Ok(()) => app.info(susi_core::tf!("Saved: {path}", path = path.display())),
                    Err(e) => app.error(format!("{}: {e}", path.display())),
                }
            }
        }
    }
}

/// Loads the translation of a data cell from `file` (the file dialog's part of [`DataCmd::Load`]).
pub fn load_file(app: &mut SusiApp, r: &RowRef, lang: &str, file: &Path, link: bool) {
    let Some(p) = app.project.as_mut() else { return };
    match custom_data::load_translation_file(p, r, lang, file, link) {
        Ok(()) => {
            app.issue_cells.remove(&(r.clone(), lang.to_string()));
            app.touched();
            app.info(susi_core::tf!("Translation loaded from {file}", file = file.display()));
        }
        Err(e) => app.error(format!("{}: {e}", file.display())),
    }
}

/// Reads translations again whose linked file changed (on opening, scanning and building).
pub fn refresh_linked(app: &mut SusiApp) {
    let Some(p) = app.project.as_mut() else { return };
    let rep = custom_data::refresh_linked(p);
    let updated: Vec<(String, String)> = rep.updated.iter().map(|(r, l)| (p.context(r), l.clone())).collect();
    for (row, lang) in &updated {
        app.info(susi_core::tf!("Translation read from its changed linked file: {row} ({lang})", row = row, lang = lang));
    }
    for f in &rep.missing {
        app.warn(susi_core::tf!("Linked file {file} not found", file = f.display()));
    }
    if !updated.is_empty() {
        app.touched();
    }
}
