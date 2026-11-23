//! Susi the CAT – computer-assisted translation GUI tool (inspired by Sisulizer).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod app_icon;
mod bottom;
mod data_ui;
mod dialogs;
mod grid;
mod list;
mod panels;
mod shots;
#[cfg(test)]
mod tests;
mod window;
mod workflow_ui;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let project = std::env::args_os().nth(1).map(PathBuf::from);
    let state_file = window::state_file();
    // Without a saved geometry the default size may not fit the monitor.
    let first_start = !state_file.exists();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Susi the CAT")
            .with_inner_size(window::DEFAULT_SIZE)
            .with_min_inner_size(window::MIN_SIZE)
            .with_icon(app_icon::window_icon()),
        persistence_path: Some(state_file),
        ..Default::default()
    };
    eframe::run_native(
        "Susi the CAT",
        options,
        Box::new(move |cc| {
            let mut app = app::SusiApp::new(&cc.egui_ctx, project);
            app.fit_window = first_start;
            if let Some(storage) = cc.storage {
                app.panels = window::PanelSizes::load(storage);
            }
            Ok(Box::new(app))
        }),
    )
}
