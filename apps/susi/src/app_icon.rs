//! Application icon (`assets/app-icon`, rendered by `cargo xtask icons`): window and taskbar icon,
//! logo in the About dialog. The `.exe` icon is embedded by `build.rs`.

const PNG: &[u8] = include_bytes!("../../../assets/app-icon/susi-256.png");

pub fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(PNG).unwrap_or_default()
}

/// The icon as texture, uploaded once per context.
pub fn logo(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::unique("susi-app-icon");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex;
    }
    let icon = window_icon();
    let image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    let tex = ctx.load_texture("susi-app-icon", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}
