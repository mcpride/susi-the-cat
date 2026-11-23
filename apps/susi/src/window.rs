//! Layout of the main window, restored from the last session (`window.ron` in the config directory):
//! eframe saves position, size and maximized state (and thereby the monitor), Susi the sizes of the
//! resizable panels. Without a saved geometry (first start) the window is fitted to the monitor and centered.

use std::path::PathBuf;

/// Sizes of the resizable panels (outer size in points): height of the bottom panel, widths of
/// project tree and filter panel. The panels keep these sizes whatever they show (see `panels::fill`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelSizes {
    pub bottom: f32,
    pub tree: f32,
    pub filter: f32,
}

impl Default for PanelSizes {
    fn default() -> Self {
        Self { bottom: 250.0, tree: 260.0, filter: 210.0 }
    }
}

const KEY_BOTTOM: &str = "panel_bottom";
const KEY_TREE: &str = "panel_tree";
const KEY_FILTER: &str = "panel_filter";

impl PanelSizes {
    /// Sizes of the last session; missing or unreadable values keep their default.
    pub fn load(storage: &dyn eframe::Storage) -> Self {
        let d = Self::default();
        let get = |key, default| eframe::get_value::<f32>(storage, key).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(default);
        Self { bottom: get(KEY_BOTTOM, d.bottom), tree: get(KEY_TREE, d.tree), filter: get(KEY_FILTER, d.filter) }
    }

    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, KEY_BOTTOM, &self.bottom);
        eframe::set_value(storage, KEY_TREE, &self.tree);
        eframe::set_value(storage, KEY_FILTER, &self.filter);
    }
}

/// Inner size on the first start, shrunk to the monitor if needed (points).
pub const DEFAULT_SIZE: egui::Vec2 = egui::vec2(1400.0, 860.0);
pub const MIN_SIZE: egui::Vec2 = egui::vec2(900.0, 560.0);
/// Room left for the taskbar: the monitor size includes it (points, Windows 11: 48).
const TASKBAR: f32 = 48.0;
/// Gap between the window frame and the edges of the usable area (points).
const MARGIN: f32 = 8.0;

/// File eframe stores the window geometry in.
pub fn state_file() -> PathBuf {
    susi_core::settings::config_dir().join("window.ron")
}

/// Shrinks the window to fit the monitor of the window, leaving room for the taskbar, and centers it.
///
/// Coordinates assume the window is on the primary monitor (origin 0,0), where Windows opens a
/// window without a position, like eframe's `centered` option does.
pub fn fit_to_monitor(ctx: &egui::Context) {
    let (monitor, inner, outer) = ctx.input(|i| {
        let v = i.viewport();
        (v.monitor_size, v.inner_rect, v.outer_rect)
    });
    let (Some(monitor), Some(inner), Some(outer)) = (monitor, inner, outer) else {
        return;
    };
    // Title bar and borders (on Windows including the invisible resize borders).
    let frame = outer.size() - inner.size();
    let usable = monitor - egui::vec2(0.0, TASKBAR);
    let size = inner.size().min(usable - frame - egui::Vec2::splat(2.0 * MARGIN)).max(MIN_SIZE);
    let pos = ((usable - size - frame) * 0.5).max(egui::Vec2::ZERO).to_pos2();
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
}
