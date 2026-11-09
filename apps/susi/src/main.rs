//! Susi the CAT – computer-assisted translation GUI tool (inspired by Sisulizer).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod app_icon;
mod bottom;
mod dialogs;
mod grid;
mod list;
mod panels;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let project = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Susi the CAT")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(app_icon::window_icon()),
        ..Default::default()
    };
    eframe::run_native("Susi the CAT", options, Box::new(move |cc| Ok(Box::new(app::SusiApp::new(&cc.egui_ctx, project)))))
}
