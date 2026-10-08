#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod compare;
mod decoder;
mod export;
mod index;
mod player;
mod settings;
mod sync;
mod timecode;
mod timeline;
mod ui;
mod winutil;

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    ffmpeg_next::init()?;
    ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);

    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    if first
        .as_ref()
        .is_some_and(|a| a == "--version" || a == "-V")
    {
        // Dient auch als Smoke-Test: startet nur, wenn alle DLLs geladen werden konnten.
        println!("framescope {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if first.as_ref().is_some_and(|a| a == "--bench") {
        let path = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Datei fehlt"))?;
        return decoder::bench(&path);
    }
    if first.is_some_and(|a| a == "--audio-selftest") {
        let path = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Datei fehlt"))?;
        return audio::selftest(&path);
    }

    // Optionales Kommandozeilenargument: Videodatei.
    // Kommandozeile: [--sync] [Video A] [Video B → Vergleichsmodus]
    let mut sync = false;
    let mut initial: Option<PathBuf> = None;
    let mut initial_b: Option<PathBuf> = None;
    for arg in std::env::args_os().skip(1) {
        if arg == "--sync" {
            sync = true;
        } else if initial.is_none() {
            initial = Some(PathBuf::from(arg));
        } else if initial_b.is_none() {
            initial_b = Some(PathBuf::from(arg));
        }
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("FrameScope")
            .with_decorations(false)
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
            Ok(Box::new(app::PlayerApp::new(cc, initial, initial_b, sync)))
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
