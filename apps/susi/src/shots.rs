//! Screenshots in the GUI: the bottom tab (image of the selected row with its marked area), adding
//! (file dialog, clipboard, drag and drop), linking, marking areas and the screenshot manager.
//!
//! The screenshots are stored next to the project (`<project>.files/`) and saved with it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use susi_core::i18n::{t, trn};
use susi_core::model::RowRef;
use susi_core::screenshots::{Area, Link, ScreenshotIndex, ScreenshotStore};
use susi_core::tf;

use crate::actions::{Action, Scope};
use crate::app::{BottomTab, SusiApp};
use crate::dialogs::Dialog;

/// Screenshot commands (menus, bottom tab, manager).
#[derive(Debug, Clone, PartialEq)]
pub enum ShotCmd {
    /// Adds image files chosen in a file dialog and links them to the rows of the scope
    /// (`None`: the selected row).
    Add(Option<Scope>),
    /// Pastes the image of the clipboard and links it to the selected row.
    Paste,
    /// Links screenshot `id` to the selected row.
    Link(String),
    /// Removes the link of screenshot `id` to the selected row.
    Unlink(String),
    /// Sets (or clears) the area of the selected row on screenshot `id`.
    SetArea(String, Option<Area>),
    Remove(String),
    SetTitle(String, String),
    /// Removes orphaned links and unused image files.
    Prune,
    /// Reads changed origin files again.
    Refresh,
    /// Imports a manifest of a UI test run.
    ImportManifest,
    /// Opens the screenshot manager.
    Manager,
}

/// State of the screenshot views.
#[derive(Default)]
pub struct ShotUi {
    /// Image textures by image file name (the name changes with the content).
    textures: HashMap<String, egui::TextureHandle>,
    /// Image files that could not be read.
    failed: HashSet<String>,
    /// Screenshot shown in the bottom tab when the selected row has several.
    pub current: Option<String>,
    /// Marking an area: the next drag over the image defines it.
    pub marking: bool,
    drag: Option<(Pos2, Pos2)>,
    /// A paste was requested with the button: the next pasted image is added.
    pub paste_requested: bool,
}

#[cfg(test)]
impl ShotUi {
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
}

/// Color of marked areas.
const AREA: Color32 = Color32::from_rgb(230, 40, 40);

fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg"].contains(&e.to_ascii_lowercase().as_str()))
}

/// PNG encoding of a pasted image.
fn png_of(img: &egui::ColorImage) -> Option<Vec<u8>> {
    let [w, h] = img.size;
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let buf = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buf).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Texture of an image file, loaded on first use.
fn texture(state: &mut ShotUi, ctx: &egui::Context, file: &str, path: &Path) -> Option<egui::TextureHandle> {
    if let Some(t) = state.textures.get(file) {
        return Some(t.clone());
    }
    if state.failed.contains(file) {
        return None;
    }
    let Some(img) = std::fs::read(path).ok().and_then(|b| image::load_from_memory(&b).ok()) else {
        state.failed.insert(file.to_string());
        return None;
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let tex = ctx.load_texture(format!("screenshot-{file}"), egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR);
    state.textures.insert(file.to_string(), tex.clone());
    Some(tex)
}

/// Small camera at the right of the context cell of a row with screenshots.
pub fn camera_marker(ui: &Ui) {
    let r = ui.max_rect();
    let color = Color32::from_rgb(40, 110, 200);
    let body = Rect::from_center_size(pos2(r.right() - 9.0, r.center().y + 1.0), vec2(10.0, 7.0));
    let p = ui.painter();
    p.rect_filled(Rect::from_min_size(body.min + vec2(2.0, -2.0), vec2(4.0, 2.0)), 0.5, color);
    p.rect_filled(body, 1.5, color);
    p.circle_filled(body.center(), 2.0, ui.visuals().extreme_bg_color);
}

impl SusiApp {
    /// Loads the screenshots of the open project (after opening, or saving under a new name).
    /// A project without a file has none.
    pub fn load_screenshots(&mut self) {
        self.shots = None;
        self.shot_ui = ShotUi::default();
        if let Some(path) = self.path().map(Path::to_path_buf) {
            match ScreenshotStore::open(&path) {
                Ok(s) => self.shots = Some(s),
                Err(e) => self.error(tf!("Screenshots: {error}", error = e)),
            }
        }
        self.rebuild_shot_index();
    }

    /// Resolves the screenshot links against the project (after changes of either).
    pub fn rebuild_shot_index(&mut self) {
        self.shot_index = match (&self.project, &self.shots) {
            (Some(p), Some(s)) => s.index(p),
            _ => ScreenshotIndex::default(),
        };
        if self.filter.screenshot.is_some() {
            self.rows_stale = true;
        }
    }

    /// Records a change of the screenshots; they are saved with the project.
    fn shots_changed(&mut self) {
        self.dirty = true;
        self.rebuild_shot_index();
    }

    /// Whether screenshots can be added; a project without a file must be saved first.
    fn can_add_screenshots(&mut self) -> bool {
        if self.shots.is_some() {
            return true;
        }
        self.warn(t("Save the project first to add screenshots."));
        false
    }

    fn target_rows(&self, scope: Option<&Scope>) -> Vec<RowRef> {
        match scope {
            Some(s) => self.rows_of(s),
            None => self.selected.iter().cloned().collect(),
        }
    }

    fn show_screenshot(&mut self, id: String) {
        self.shot_ui.current = Some(id);
        self.tab = BottomTab::Screenshots;
        self.show_bottom = true;
    }

    /// Adds image files and links them to `rows`.
    pub fn add_screenshot_files(&mut self, files: &[PathBuf], rows: &[RowRef]) {
        if !self.can_add_screenshots() {
            return;
        }
        let (mut added, mut errors) = (Vec::new(), Vec::new());
        if let (Some(p), Some(st)) = (&self.project, self.shots.as_mut()) {
            for f in files {
                match st.add_file(f, None, None) {
                    Ok(id) => {
                        if let Err(e) = st.link(&id, rows.iter().map(|r| Link::to_row(p, r, None))) {
                            errors.push(e.to_string());
                        }
                        added.push(id);
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
        }
        for e in errors {
            self.error(e);
        }
        if let Some(last) = added.last().cloned() {
            self.info(
                trn(rows.len(), "Screenshots added: {ids} (linked to {n} row)", "Screenshots added: {ids} (linked to {n} rows)")
                    .replace("{ids}", &added.join(", ")),
            );
            self.shots_changed();
            self.show_screenshot(last);
        }
    }

    /// Adds a pasted image and links it to the selected row.
    pub fn paste_image(&mut self, img: &egui::ColorImage) {
        if !self.can_add_screenshots() {
            return;
        }
        let Some(png) = png_of(img) else {
            self.error(t("The pasted image could not be converted."));
            return;
        };
        let rows = self.target_rows(None);
        let result = match (&self.project, self.shots.as_mut()) {
            (Some(p), Some(st)) => st.add_bytes(&png, None, None).and_then(|id| st.link(&id, rows.iter().map(|r| Link::to_row(p, r, None))).map(|_| id)),
            _ => return,
        };
        match result {
            Ok(id) => {
                self.info(tf!("Screenshot pasted: {id}", id = id));
                self.shots_changed();
                self.show_screenshot(id);
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Changes the store and the selected row: `f(project, store, row)`.
    fn with_selected(&mut self, f: impl FnOnce(&susi_core::Project, &mut ScreenshotStore, &RowRef) -> susi_core::Result<()>) {
        let result = match (&self.project, self.shots.as_mut(), &self.selected) {
            (Some(p), Some(st), Some(r)) => f(p, st, r),
            _ => return,
        };
        match result {
            Ok(()) => self.shots_changed(),
            Err(e) => self.error(e.to_string()),
        }
    }
}

/// Runs a screenshot command.
pub fn run(app: &mut SusiApp, cmd: ShotCmd, ctx: &egui::Context) {
    if app.project.is_none() {
        app.warn(t("No project open"));
        return;
    }
    match cmd {
        ShotCmd::Add(scope) => {
            if !app.can_add_screenshots() {
                return;
            }
            let rows = app.target_rows(scope.as_ref());
            if let Some(files) =
                rfd::FileDialog::new().add_filter(t("Images (PNG, JPEG)"), &["png", "jpg", "jpeg"]).set_title(t("Add screenshots")).pick_files()
            {
                app.add_screenshot_files(&files, &rows);
            }
        }
        ShotCmd::Paste => {
            if app.can_add_screenshots() {
                app.shot_ui.paste_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
        }
        ShotCmd::Link(id) => {
            app.with_selected(|p, st, r| st.link(&id, [Link::to_row(p, r, None)]).map(|_| ()));
            app.show_screenshot(id);
        }
        ShotCmd::Unlink(id) => app.with_selected(|p, st, r| {
            st.unlink_row(p, &id, r);
            Ok(())
        }),
        ShotCmd::SetArea(id, area) => app.with_selected(|p, st, r| st.set_area(p, &id, r, area)),
        ShotCmd::Remove(id) => {
            if app.shots.as_mut().is_some_and(|st| st.remove(&id)) {
                app.info(tf!("Screenshot removed: {id}", id = id));
                app.shots_changed();
            }
        }
        ShotCmd::SetTitle(id, title) => {
            if app.shots.as_mut().is_some_and(|st| st.set_title(&id, &title)) && app.shots.as_ref().is_some_and(ScreenshotStore::is_dirty) {
                app.dirty = true;
            }
        }
        ShotCmd::Prune => {
            let rep = match (&app.project, app.shots.as_mut()) {
                (Some(p), Some(st)) => st.prune(p),
                _ => return,
            };
            app.info(tf!("{links} orphaned links removed, {files} unused image files deleted", links = rep.links, files = rep.files));
            app.shots_changed();
        }
        ShotCmd::Refresh => {
            let Some(st) = app.shots.as_mut() else {
                return;
            };
            match st.refresh(None) {
                Ok(rep) => {
                    for id in &rep.missing {
                        app.warn(tf!("Origin file of screenshot {id} not found", id = id));
                    }
                    app.info(tf!("{updated} screenshots updated, {unchanged} unchanged", updated = rep.updated.len(), unchanged = rep.unchanged));
                    if !rep.updated.is_empty() {
                        app.shots_changed();
                    }
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        ShotCmd::ImportManifest => {
            if !app.can_add_screenshots() {
                return;
            }
            let Some(file) = rfd::FileDialog::new().add_filter(t("Screenshot manifest"), &["json"]).set_title(t("Import screenshot manifest")).pick_file()
            else {
                return;
            };
            let Some(st) = app.shots.as_mut() else {
                return;
            };
            match st.import_manifest(&file, false) {
                Ok(rep) => {
                    app.info(tf!(
                        "{added} screenshots added, {replaced} replaced, {removed} removed, {links} links",
                        added = rep.added,
                        replaced = rep.replaced,
                        removed = rep.removed,
                        links = rep.links
                    ));
                    app.shots_changed();
                }
                Err(e) => app.error(e.to_string()),
            }
        }
        ShotCmd::Manager => app.dialog = Some(Dialog::Screenshots),
    }
}

/// Dropped image files and pasted images (Ctrl+V while the screenshot tab is shown).
pub fn handle_input(app: &mut SusiApp, ctx: &egui::Context) {
    let (dropped, pasted) = ctx.input(|i| {
        let files: Vec<PathBuf> = i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| is_image(p)).collect();
        let image = i.events.iter().find_map(|e| if let egui::Event::PasteImage(img) = e { Some(img.clone()) } else { None });
        (files, image)
    });
    if !dropped.is_empty() && app.project.is_some() && app.dialog.is_none() {
        let rows = app.target_rows(None);
        app.add_screenshot_files(&dropped, &rows);
    }
    if let Some(img) = pasted {
        let tab_shown = app.show_bottom && app.tab == BottomTab::Screenshots && app.dialog.is_none();
        if app.shot_ui.paste_requested || tab_shown {
            app.shot_ui.paste_requested = false;
            app.paste_image(&img);
        }
    }
}

/// The "Screenshots" tab: screenshots of the selected row.
pub fn tab(app: &mut SusiApp, ui: &mut Ui) {
    let mut cmds: Vec<ShotCmd> = Vec::new();
    let Some(store) = app.shots.as_ref() else {
        ui.label(if app.project.is_some() { t("Save the project first to add screenshots.") } else { t("No project open") });
        return;
    };
    let links: Vec<(String, Option<Area>)> =
        app.selected.as_ref().map(|r| store.links_of(&app.shot_index, r).iter().map(|(s, l)| (s.id.clone(), l.area)).collect()).unwrap_or_default();
    let current = app.shot_ui.current.clone().filter(|c| links.iter().any(|(id, _)| id == c)).or_else(|| links.first().map(|(id, _)| id.clone()));
    let area = links.iter().find(|(id, _)| Some(id) == current.as_ref()).and_then(|(_, a)| *a);
    ui.horizontal_wrapped(|ui| {
        if ui.button(format!("➕ {}", t("Add…"))).on_hover_text(t("Add image files and link them to the selected row")).clicked() {
            cmds.push(ShotCmd::Add(None));
        }
        if ui.button(t("Paste")).on_hover_text(t("Paste an image from the clipboard (Ctrl+V)")).clicked() {
            cmds.push(ShotCmd::Paste);
        }
        let others: Vec<_> = store.screenshots().iter().filter(|s| !links.iter().any(|(id, _)| *id == s.id)).collect();
        if app.selected.is_some() && !others.is_empty() {
            egui::ComboBox::from_id_salt("shot-link").selected_text(t("Link existing…")).show_ui(ui, |ui| {
                for s in others {
                    if ui.selectable_label(false, s.label()).clicked() {
                        cmds.push(ShotCmd::Link(s.id.clone()));
                    }
                }
            });
        }
        if let Some(id) = &current {
            ui.separator();
            ui.toggle_value(&mut app.shot_ui.marking, t("Mark area")).on_hover_text(t("Drag a rectangle over the image where the text appears"));
            if area.is_some() && ui.button(t("Clear area")).clicked() {
                cmds.push(ShotCmd::SetArea(id.clone(), None));
            }
            if ui.button(t("Remove link")).on_hover_text(t("Remove the screenshot from the selected row")).clicked() {
                cmds.push(ShotCmd::Unlink(id.clone()));
            }
        }
        ui.separator();
        if ui.button(t("Manage…")).clicked() {
            cmds.push(ShotCmd::Manager);
        }
    });
    if links.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (id, _) in &links {
                let label = store.get(id).map_or(id.as_str(), |s| s.label());
                if ui.selectable_label(current.as_ref() == Some(id), label).clicked() {
                    app.shot_ui.current = Some(id.clone());
                }
            }
        });
    }
    ui.separator();
    match (&app.selected, current.as_ref().and_then(|id| store.get(id))) {
        (None, _) => {
            ui.label(t("Select a row to see its screenshots."));
        }
        (Some(_), None) => {
            ui.label(t("No screenshot is linked to the selected row. Add one, paste one (Ctrl+V) or drop image files here."));
        }
        (Some(_), Some(shot)) => {
            let path = store.image_dir().join(&shot.file);
            let Some(tex) = texture(&mut app.shot_ui, ui.ctx(), &shot.file, &path) else {
                ui.colored_label(AREA, tf!("Image file missing: {path}", path = path.display()));
                return push(app, cmds);
            };
            let [w, h] = tex.size();
            let avail = ui.available_size();
            let scale = (avail.x / w as f32).min(avail.y / h as f32).clamp(0.05, 1.0);
            let sense = if app.shot_ui.marking { Sense::drag() } else { Sense::hover() };
            let (rect, resp) = ui.allocate_exact_size(vec2(w as f32 * scale, h as f32 * scale), sense);
            let painter = ui.painter_at(rect);
            painter.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            let screen = |a: Area| Rect::from_min_size(rect.min + vec2(a.x as f32, a.y as f32) * scale, vec2(a.w as f32, a.h as f32) * scale);
            for l in shot.links.iter().filter_map(|l| l.area) {
                painter.rect_stroke(screen(l), 0.0, Stroke::new(1.0, AREA.gamma_multiply(0.45)), StrokeKind::Outside);
            }
            if let Some(a) = area {
                painter.rect_filled(screen(a), 0.0, AREA.gamma_multiply(0.12));
                painter.rect_stroke(screen(a), 0.0, Stroke::new(2.5, AREA), StrokeKind::Outside);
            }
            if app.shot_ui.marking {
                let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
                if let Some(pos) = resp.interact_pointer_pos() {
                    let pos = rect.clamp(pos);
                    app.shot_ui.drag = Some(match app.shot_ui.drag {
                        Some((from, _)) if !resp.drag_started() => (from, pos),
                        _ => (pos, pos),
                    });
                }
                if let Some((a, b)) = app.shot_ui.drag {
                    let r = Rect::from_two_pos(a, b);
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0, ui.visuals().selection.stroke.color), StrokeKind::Outside);
                    if resp.drag_stopped() {
                        let px = |v: f32| (v / scale).round().max(0.0) as u32;
                        let marked = Area { x: px(r.min.x - rect.min.x), y: px(r.min.y - rect.min.y), w: px(r.width()), h: px(r.height()) };
                        if marked.w > 1 && marked.h > 1 {
                            cmds.push(ShotCmd::SetArea(shot.id.clone(), marked.clip(shot.width, shot.height)));
                        }
                        app.shot_ui.drag = None;
                        app.shot_ui.marking = false;
                    }
                }
            }
        }
    }
    push(app, cmds);
}

fn push(app: &mut SusiApp, cmds: Vec<ShotCmd>) {
    app.actions.extend(cmds.into_iter().map(Action::Screenshot));
}

/// The screenshot manager: all screenshots with title, size and links. Returns whether to close.
pub fn manager(app: &mut SusiApp, ui: &mut Ui) -> bool {
    ui.heading(t("Screenshots"));
    let Some(store) = app.shots.as_ref() else {
        ui.label(t("Save the project first to add screenshots."));
        return ui.button(t("Close")).clicked();
    };
    ui.label(tf!("Folder: {path}", path = store.dir().display()));
    let orphans = store.orphans(&app.shot_index);
    let mut cmds = Vec::new();
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        egui::Grid::new("shot-manager").striped(true).num_columns(6).show(ui, |ui| {
            for h in ["", t("Id"), t("Title"), t("Size"), t("Links"), ""] {
                ui.strong(h);
            }
            ui.end_row();
            for s in store.screenshots() {
                match texture(&mut app.shot_ui, ui.ctx(), &s.file, &store.image_dir().join(&s.file)) {
                    Some(tex) => {
                        let [w, h] = tex.size();
                        let k = (64.0 / w as f32).min(40.0 / h as f32).min(1.0);
                        ui.image((tex.id(), vec2(w as f32 * k, h as f32 * k)));
                    }
                    None => {
                        ui.colored_label(AREA, "✖").on_hover_text(t("Image file missing"));
                    }
                }
                ui.label(&s.id);
                let mut title = s.title.clone();
                if ui.add(egui::TextEdit::singleline(&mut title).desired_width(180.0)).changed() {
                    cmds.push(ShotCmd::SetTitle(s.id.clone(), title));
                }
                ui.label(format!("{} × {}", s.width, s.height));
                let lost = orphans.iter().filter(|(x, _)| x.id == s.id).count();
                if lost > 0 {
                    ui.colored_label(AREA, format!("{} ({})", s.links.len(), trn(lost, "{n} orphaned link", "{n} orphaned links")));
                } else {
                    ui.label(s.links.len().to_string());
                }
                if ui.button("🗑").on_hover_text(t("Remove screenshot")).clicked() {
                    cmds.push(ShotCmd::Remove(s.id.clone()));
                }
                ui.end_row();
            }
        });
    });
    if store.is_empty() {
        ui.label(t("No screenshots yet."));
    }
    ui.separator();
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.button(t("Import manifest…")).on_hover_text(t("Screenshots of a UI test run (screenshots.json schema)")).clicked() {
            cmds.push(ShotCmd::ImportManifest);
        }
        if ui.button(t("Refresh")).on_hover_text(t("Read changed origin files again")).clicked() {
            cmds.push(ShotCmd::Refresh);
        }
        if ui.button(t("Clean up")).on_hover_text(t("Remove links to rows that no longer exist and unused image files")).clicked() {
            cmds.push(ShotCmd::Prune);
        }
        close = ui.button(t("Close")).clicked();
    });
    push(app, cmds);
    close
}
