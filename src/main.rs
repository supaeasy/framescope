#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod decoder;
mod timeline;
mod ui;

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);

    // Optionales Kommandozeilenargument: Videodatei.
    let initial: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FrameScope")
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "FrameScope",
        options,
        Box::new(move |cc| {
            ui::apply_theme(&cc.egui_ctx);
            Ok(Box::new(app::PlayerApp::new(cc, initial)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("UI-Fehler: {e}"))
}
