#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod decoder;
mod export;
mod index;
mod player;
mod settings;
mod timecode;
mod timeline;
mod ui;

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);

    let mut args = std::env::args_os().skip(1);
    if args.next().is_some_and(|a| a == "--audio-selftest") {
        let path = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Datei fehlt"))?;
        return audio::selftest(&path);
    }

    // Optionales Kommandozeilenargument: Videodatei.
    let initial: Option<PathBuf> = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FrameScope")
            .with_icon(window_icon())
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

/// Fenstericon (in der EXE eingebettet).
fn window_icon() -> eframe::egui::IconData {
    let bytes = include_bytes!("../assets/icon-256.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            eframe::egui::IconData {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            }
        }
        Err(_) => eframe::egui::IconData::default(),
    }
}
