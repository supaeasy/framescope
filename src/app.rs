//! Anwendungsfenster: Eingabe, Zeichnen von Video, Controls und Overlay.

use crate::player::Player;
use crate::settings::Settings;
use crate::sync::SyncController;
use crate::timeline::LoopBand;
use crate::{export, timecode, timeline, ui};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Key, Modifiers, Rect, RichText, Sense, Stroke,
    TextureOptions, Vec2,
};
use std::path::PathBuf;

/// Wie lange die Controls nach der letzten Mausbewegung sichtbar bleiben.
const CONTROLS_TIMEOUT: f64 = 2.5;
/// Anzeigedauer von Meldungen (Sekunden).
const TOAST_SECS: f64 = 4.0;
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mkv", "mov", "webm", "avi", "ts", "mts", "wmv", "flv",
];

/// Nachrichten von Hintergrund-Threads (Dialoge, Export) an die UI.
enum Msg {
    Open(PathBuf),
    Toast { text: String, error: bool },
    ExportDir(PathBuf),
}

pub struct PlayerApp {
    player: Option<Player>,
    settings: Settings,
    export_dir_changed: bool,
    volume: f32,
    muted: bool,
    hud: bool,
    help: bool,
    sync: SyncController,
    error: Option<String>,
    texture: Option<egui::TextureHandle>,
    last_activity: f64,
    fullscreen: bool,
    toast: Option<(String, bool, f64)>,
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,
    /// Entwickler-Benchmark: nach N Sekunden Statistik schreiben und beenden.
    bench: Option<(f64, f64)>,
}

impl PlayerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<PathBuf>,
        sync_on_start: bool,
    ) -> Self {
        let (msg_tx, msg_rx) = unbounded();
        let settings = Settings::load();
        let mut app = Self {
            player: None,
            volume: settings.volume,
            muted: settings.muted,
            hud: settings.hud,
            help: false,
            sync: SyncController::new({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            settings,
            export_dir_changed: false,
            error: None,
            texture: None,
            last_activity: 0.0,
            fullscreen: false,
            toast: None,
            msg_tx,
            msg_rx,
            bench: std::env::var("FRAMESCOPE_BENCH")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(|secs| (secs, -1.0)),
        };
        app.sync.set_enabled(sync_on_start, None);
        if let Some(path) = initial {
            app.open(&cc.egui_ctx, path);
        }
        app
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let wake_ctx = ctx.clone();
        self.error = None;
        self.texture = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} – FrameScope",
            path.file_name()
                .map_or_else(|| "Video".into(), |n| n.to_string_lossy())
        )));
        self.player = Some(Player::new(
            path,
            move || wake_ctx.request_repaint(),
            self.volume,
            self.muted,
        ));
    }

    /// Dateidialog in eigenem Thread, damit das Fenster nicht blockiert.
    fn open_dialog(&self, ctx: &egui::Context) {
        let (tx, ctx) = (self.msg_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("Video", VIDEO_EXTENSIONS)
                .add_filter("Alle Dateien", &["*"])
                .pick_file();
            if let Some(p) = picked {
                let _ = tx.send(Msg::Open(p));
                ctx.request_repaint();
            }
        });
    }

    /// Startet eine weitere Instanz; läuft Sync, startet sie ebenfalls synchronisiert.
    fn new_window(&self) {
        if let Ok(exe) = std::env::current_exe() {
            let mut cmd = std::process::Command::new(exe);
            if self.sync.enabled {
                cmd.arg("--sync");
            }
            let _ = cmd.spawn();
        }
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>, error: bool) {
        let now = ctx.input(|i| i.time);
        self.toast = Some((text.into(), error, now + TOAST_SECS));
    }

    fn toggle_sync(&mut self, ctx: &egui::Context) {
        if !self.sync.available() {
            self.toast(ctx, "Sync nicht verfügbar (keine freie Verbindung)", true);
            return;
        }
        let on = !self.sync.enabled;
        self.sync.set_enabled(on, self.player.as_ref());
        let text = if on { "Sync an" } else { "Sync aus" };
        self.toast(ctx, text, false);
    }

    /// Versatz so setzen, dass die aktuelle Position der des Sync-Partners entspricht.
    fn align_sync(&mut self, ctx: &egui::Context) {
        let Some(pos) = self.player.as_ref().map(Player::position) else {
            return;
        };
        if self.sync.align(pos) {
            let offset = self.sync.offset;
            self.toast(
                ctx,
                format!("Sync abgeglichen (Versatz {offset:+.3} s)"),
                false,
            );
        } else {
            self.toast(
                ctx,
                "Kein Sync-Partner gefunden (Sync bei beiden Fenstern einschalten)",
                true,
            );
        }
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    /// Exportiert den aktuellen Frame in Originalauflösung als PNG.
    /// Ohne Standardordner (oder mit `ask`) fragt ein Dialog nach dem Speicherort.
    fn export_png(&mut self, ctx: &egui::Context, ask: bool) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let Some(frame) = p.current.clone() else {
            return;
        };
        let number = p
            .frame_no()
            .unwrap_or_else(|| (frame.pts * p.fps()).round() as usize);
        let name = export::file_name(&p.path, number);
        let video_dir = p.path.parent().map(PathBuf::from);
        let default_dir = self.settings.export_dir.clone().filter(|d| d.is_dir());
        let (tx, ctx) = (self.msg_tx.clone(), ctx.clone());

        std::thread::spawn(move || {
            let (target, remember) = match (&default_dir, ask) {
                (Some(dir), false) => (Some(dir.join(&name)), false),
                _ => {
                    let mut dialog = rfd::FileDialog::new()
                        .set_file_name(&name)
                        .add_filter("PNG", &["png"]);
                    if let Some(d) = default_dir.as_ref().or(video_dir.as_ref()) {
                        dialog = dialog.set_directory(d);
                    }
                    (dialog.save_file(), true)
                }
            };
            let Some(path) = target else { return };
            let msg = match export::save_png(&path, frame.width, frame.height, frame.rgba.clone()) {
                Ok(()) => Msg::Toast {
                    text: format!(
                        "Frame {number} gespeichert ({}×{}): {}",
                        frame.width,
                        frame.height,
                        path.display()
                    ),
                    error: false,
                },
                Err(e) => Msg::Toast {
                    text: format!("{e:#}"),
                    error: true,
                },
            };
            if remember {
                if let Some(dir) = path.parent() {
                    let _ = tx.send(Msg::ExportDir(dir.to_path_buf()));
                }
            }
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    fn upload_texture(&mut self, ctx: &egui::Context) {
        let Some(frame) = self.player.as_ref().and_then(|p| p.current.clone()) else {
            return;
        };
        // RGBA ist opak (Alpha 255) → unmultiplied == premultiplied, direkter Cast ohne Pixelschleife.
        let pixels: Vec<Color32> = bytemuck::cast_slice(&frame.rgba).to_vec();
        let image = egui::ColorImage::new([frame.width, frame.height], pixels);
        match self.texture.as_mut() {
            Some(t) => t.set(image, TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture("video", image, TextureOptions::LINEAR)),
        }
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                Msg::Open(path) => self.open(ctx, path),
                Msg::Toast { text, error } => self.toast = Some((text, error, now + TOAST_SECS)),
                Msg::ExportDir(dir) => {
                    self.settings.export_dir = Some(dir);
                    self.export_dir_changed = true;
                }
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        #[derive(Default)]
        struct Keys {
            dropped: Option<PathBuf>,
            open: bool,
            new_win: bool,
            space: bool,
            key_back: usize,
            key_fwd: usize,
            back: usize,
            fwd: usize,
            mute: bool,
            vol_up: usize,
            vol_down: usize,
            loop_in: bool,
            loop_out: bool,
            loop_toggle: bool,
            loop_clear: bool,
            export: bool,
            export_as: bool,
            fullscreen: bool,
            escape: bool,
            hud: bool,
            help: bool,
            sync: bool,
            sync_align: bool,
        }
        // Zählt Tastendrücke (inkl. Wiederholungen) und verbraucht sie.
        fn presses(i: &mut egui::InputState, mods: Modifiers, key: Key) -> usize {
            let n = i
                .events
                .iter()
                .filter(|e| {
                    matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                        if *k == key && m.shift == mods.shift && !m.command && !m.alt)
                })
                .count();
            while i.consume_key(mods, key) {}
            n
        }
        let k = ctx.input_mut(|i| Keys {
            dropped: i
                .raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .next(),
            open: i.consume_key(Modifiers::COMMAND, Key::O),
            new_win: i.consume_key(Modifiers::COMMAND, Key::N),
            space: i.consume_key(Modifiers::NONE, Key::Space),
            key_back: presses(i, Modifiers::SHIFT, Key::ArrowLeft),
            key_fwd: presses(i, Modifiers::SHIFT, Key::ArrowRight),
            back: presses(i, Modifiers::NONE, Key::ArrowLeft),
            fwd: presses(i, Modifiers::NONE, Key::ArrowRight),
            mute: i.consume_key(Modifiers::NONE, Key::M),
            vol_up: presses(i, Modifiers::NONE, Key::ArrowUp),
            vol_down: presses(i, Modifiers::NONE, Key::ArrowDown),
            loop_in: i.consume_key(Modifiers::NONE, Key::I),
            loop_out: i.consume_key(Modifiers::NONE, Key::O),
            loop_toggle: i.consume_key(Modifiers::NONE, Key::L),
            loop_clear: i.consume_key(Modifiers::NONE, Key::X),
            export_as: i.consume_key(Modifiers::SHIFT, Key::S),
            export: i.consume_key(Modifiers::NONE, Key::S),
            fullscreen: i.consume_key(Modifiers::NONE, Key::F),
            escape: i.consume_key(Modifiers::NONE, Key::Escape),
            hud: i.consume_key(Modifiers::NONE, Key::H),
            help: i.consume_key(Modifiers::NONE, Key::F1),
            sync_align: i.consume_key(Modifiers::SHIFT, Key::Y),
            sync: i.consume_key(Modifiers::NONE, Key::Y),
        });
        if let Some(path) = k.dropped {
            self.open(ctx, path);
        }
        if k.open {
            self.open_dialog(ctx);
        }
        if k.new_win {
            self.new_window();
        }
        self.drain_messages(ctx);
        if k.sync {
            self.toggle_sync(ctx);
        }
        if k.sync_align {
            self.align_sync(ctx);
        }
        if k.hud {
            self.hud = !self.hud;
        }
        if k.help {
            self.help = !self.help;
        }
        if k.escape && self.help {
            self.help = false;
        } else if k.fullscreen {
            self.set_fullscreen(ctx, !self.fullscreen);
        } else if k.escape && self.fullscreen {
            self.set_fullscreen(ctx, false);
        }
        if k.export || k.export_as {
            self.export_png(ctx, k.export_as);
        }
        if k.mute {
            self.muted = !self.muted;
        }
        if k.vol_up != k.vol_down {
            let delta = (k.vol_up as f32 - k.vol_down as f32) * 0.05;
            self.volume = (self.volume + delta).clamp(0.0, 1.0);
            self.muted = false;
        }
        if let Some(p) = self.player.as_mut() {
            p.set_volume(self.volume);
            p.set_muted(self.muted);
            if k.space {
                p.toggle();
            }
            if k.loop_in {
                p.set_loop_in();
            }
            if k.loop_out {
                p.set_loop_out();
            }
            if k.loop_toggle {
                p.loop_on = !p.loop_on;
            }
            if k.loop_clear {
                p.clear_loop();
            }
            // Mehrere Drücke pro UI-Frame werden zusammengefasst (Netto-Schritte).
            let keys = k.key_fwd as isize - k.key_back as isize;
            if keys != 0 {
                p.step_key_n(keys);
            }
            let frames = k.fwd as isize - k.back as isize;
            if frames != 0 {
                p.step(frames);
            }
        }
    }

    fn draw_video(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, Color32::BLACK);
        // Doppelklick auf das Video: Vollbild umschalten.
        if ui
            .interact(full, egui::Id::new("video_area"), Sense::click())
            .double_clicked()
        {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
        let Some(tex) = &self.texture else { return };
        let size = tex.size_vec2();
        let scale = (full.width() / size.x).min(full.height() / size.y);
        let rect = Rect::from_center_size(full.center(), size * scale);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    }

    fn draw_placeholder(&self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        let p = ui.painter();
        let (title, hint) = match (&self.error, &self.player) {
            (Some(e), _) => ("Datei konnte nicht geöffnet werden".to_owned(), e.clone()),
            (None, Some(_)) => (String::new(), "Lade …".to_owned()),
            (None, None) => (
                "Video hierher ziehen".to_owned(),
                "oder Strg+O zum Öffnen · Strg+N für neues Fenster".to_owned(),
            ),
        };
        let c = full.center();
        let color = if self.error.is_some() {
            Color32::from_rgb(0xff, 0x7a, 0x70)
        } else {
            ui::TEXT
        };
        p.text(
            c - Vec2::new(0.0, 14.0),
            Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(22.0),
            color,
        );
        p.text(
            c + Vec2::new(0.0, 16.0),
            Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(14.0),
            ui::TEXT_DIM,
        );
    }

    /// Dezentes Menü oben rechts: Öffnen, neues Fenster, PNG, Vollbild.
    fn draw_menu(&mut self, ctx: &egui::Context) {
        let (mut open, mut new_win, mut png, mut full, mut help, mut sync) =
            (false, false, false, false, false, false);
        let has_frame = self.player.as_ref().is_some_and(|p| p.current.is_some());
        egui::Area::new(egui::Id::new("menu"))
            .anchor(Align2::RIGHT_TOP, [-14.0, 14.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::PANEL)
                    .corner_radius(10.0)
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
                    .inner_margin(egui::Margin::symmetric(6, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            open = menu_button(ui, "Öffnen", "Datei öffnen (Strg+O)", true);
                            new_win =
                                menu_button(ui, "Neues Fenster", "Weitere Instanz (Strg+N)", true);
                            png = menu_button(
                                ui,
                                "PNG",
                                "Aktuellen Frame als PNG speichern (S, Umschalt+S: Ordner wählen)",
                                has_frame,
                            );
                            full = menu_button(ui, "Vollbild", "Vollbild (F, Esc beendet)", true);
                            let sync_label = if self.sync.enabled {
                                format!("Sync · {}", self.sync.synced_peers())
                            } else {
                                "Sync".to_owned()
                            };
                            sync = menu_button(
                                ui,
                                &sync_label,
                                "Wiedergabe mit anderen FrameScope-Fenstern synchronisieren (Y, Umschalt+Y: Versatz abgleichen)",
                                true,
                            );
                            help = menu_button(ui, "?", "Tastenkürzel (F1)", true);
                        });
                    });
            });
        if open {
            self.open_dialog(ctx);
        }
        if new_win {
            self.new_window();
        }
        if png {
            self.export_png(ctx, false);
        }
        if full {
            self.set_fullscreen(ctx, !self.fullscreen);
        }
        if help {
            self.help = !self.help;
        }
        if sync {
            self.toggle_sync(ctx);
        }
    }

    /// Dauerhaftes Info-Overlay oben links (für Vergleiche und Screenshots).
    fn draw_hud(&self, ctx: &egui::Context) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let Some(cur) = &p.current else { return };
        let (frame_txt, tc_txt) = frame_texts(p);
        let key = is_key(p, cur.pts, cur.key);
        egui::Area::new(egui::Id::new("hud"))
            .anchor(Align2::LEFT_TOP, [14.0, 14.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(150))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(frame_txt).monospace().size(16.0));
                            ui.label(
                                RichText::new(tc_txt)
                                    .monospace()
                                    .size(16.0)
                                    .color(ui::TEXT_DIM),
                            );
                            if key {
                                ui.label(
                                    RichText::new(" KEY ")
                                        .monospace()
                                        .size(13.0)
                                        .color(Color32::BLACK)
                                        .background_color(ui::KEY),
                                );
                            }
                        });
                    });
            });
    }

    fn draw_help(&mut self, ctx: &egui::Context) {
        if !self.help {
            return;
        }
        const ROWS: &[(&str, &str)] = &[
            ("Leertaste", "Wiedergabe / Pause"),
            ("← / →", "Ein Frame zurück / vor"),
            ("Umschalt + ← / →", "Voriger / nächster Keyframe"),
            ("↑ / ↓  ·  M", "Lautstärke  ·  Stumm"),
            ("I  /  O", "Loop-Anfang / -Ende setzen"),
            ("L  ·  X", "Loop an/aus  ·  Marker löschen"),
            ("S  ·  Umschalt + S", "Frame als PNG  ·  Ordner wählen"),
            ("F  ·  Esc", "Vollbild  ·  beenden"),
            ("H", "Info-Overlay (Frame, Timecode, KEY)"),
            (
                "Y  ·  Umschalt + Y",
                "Sync mit anderen Fenstern  ·  Versatz abgleichen",
            ),
            ("Strg + O  ·  Strg + N", "Datei öffnen  ·  Neues Fenster"),
            ("F1", "Diese Hilfe"),
        ];
        let mut open = true;
        egui::Window::new("Tastenkürzel")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x14, 0x15, 0x19))
                    .corner_radius(12.0)
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(24)))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                egui::Grid::new("help_grid")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        for (keys, what) in ROWS {
                            ui.label(RichText::new(*keys).monospace().size(13.0).color(ui::KEY));
                            ui.label(RichText::new(*what).size(13.0));
                            ui.end_row();
                        }
                    });
            });
        self.help = open;
    }

    /// Entwickler-Benchmark (`FRAMESCOPE_BENCH=<sekunden>`): schreibt `framescope-bench.txt`
    /// neben die EXE und beendet das Programm.
    fn bench_tick(&mut self, ctx: &egui::Context, time: f64) {
        let Some((secs, start)) = self.bench.as_mut() else {
            return;
        };
        let Some(p) = self.player.as_ref() else {
            return;
        };
        if p.current.is_none() {
            return;
        }
        if *start < 0.0 {
            *start = time;
        }
        if time - *start >= *secs {
            let text = format!(
                "shown={} dropped={} wall={:.2}s position={:.2}s
",
                p.shown,
                p.dropped,
                time - *start,
                p.position()
            );
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::fs::write(exe.with_file_name("framescope-bench.txt"), text);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some((text, error, until)) = self.toast.clone() else {
            return;
        };
        if now >= until {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .anchor(Align2::LEFT_BOTTOM, [16.0, -118.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::PANEL)
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        let color = if error {
                            Color32::from_rgb(0xff, 0x7a, 0x70)
                        } else {
                            ui::TEXT
                        };
                        ui.label(RichText::new(text).size(13.0).color(color));
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn draw_controls(&mut self, ctx: &egui::Context) {
        let sync_txt = if self.sync.enabled {
            format!(
                " · Sync ({}){}",
                self.sync.synced_peers(),
                if self.sync.offset == 0.0 {
                    String::new()
                } else {
                    format!(" {:+.3}s", self.sync.offset)
                }
            )
        } else {
            String::new()
        };
        let Some(p) = self.player.as_mut() else {
            return;
        };
        let area = egui::Area::new(egui::Id::new("controls"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -14.0])
            .order(egui::Order::Foreground);
        area.show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 40.0).max(200.0);
            egui::Frame::new()
                .fill(ui::PANEL)
                .corner_radius(12.0)
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(18)))
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.set_width(width - 28.0);
                    let duration = p.duration();
                    let cur = p.current.as_ref().map(|f| (f.pts, f.key));
                    let frac = match cur {
                        Some((pts, _)) if duration > 0.0 => (pts / duration) as f32,
                        _ => 0.0,
                    };
                    ui.horizontal(|ui| {
                        let icon = if p.playing {
                            ui::Icon::Pause
                        } else {
                            ui::Icon::Play
                        };
                        if ui::icon_button(ui, icon, 34.0).clicked() {
                            p.toggle();
                        }
                        let (frame_txt, tc_txt) = frame_texts(p);
                        ui.label(RichText::new(frame_txt).monospace().size(15.0));
                        ui.label(
                            RichText::new(tc_txt)
                                .monospace()
                                .size(15.0)
                                .color(ui::TEXT_DIM),
                        );
                        if cur.is_some_and(|(pts, key)| is_key(p, pts, key)) {
                            ui.label(
                                RichText::new(" KEY ")
                                    .monospace()
                                    .size(12.0)
                                    .color(Color32::BLACK)
                                    .background_color(ui::KEY),
                            );
                        }
                        ui.add_space(6.0);
                        let loop_btn = ui
                            .selectable_label(
                                p.loop_on,
                                RichText::new("LOOP").monospace().size(12.0),
                            )
                            .on_hover_text(
                                "Loop an/aus (L) · I/O setzen Anfang/Ende · X löscht die Marker",
                            );
                        if loop_btn.clicked() {
                            p.loop_on = !p.loop_on;
                        }
                        if p.loop_in.is_some() || p.loop_out.is_some() {
                            let fmt =
                                |v: Option<usize>| v.map_or("·".to_owned(), |n| n.to_string());
                            let text = format!("{} → {}", fmt(p.loop_in), fmt(p.loop_out));
                            ui.label(
                                RichText::new(text)
                                    .monospace()
                                    .size(12.0)
                                    .color(ui::TEXT_DIM),
                            );
                        }
                        if p.has_audio() {
                            let icon = if self.muted || self.volume <= 0.0 {
                                ui::Icon::SpeakerMuted
                            } else {
                                ui::Icon::Speaker
                            };
                            if ui::icon_button(ui, icon, 30.0).clicked() {
                                self.muted = !self.muted;
                            }
                            ui.spacing_mut().slider_width = 80.0;
                            let mut vol = self.volume;
                            if ui
                                .add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false))
                                .changed()
                            {
                                self.volume = vol;
                                self.muted = false;
                            }
                        }
                        if let Some(i) = &p.info {
                            let dropped = if p.dropped > 0 {
                                format!(" · {} verworfen", p.dropped)
                            } else {
                                String::new()
                            };
                            let clock = if p.audio_is_master() {
                                " · Audio-Clock"
                            } else {
                                ""
                            };
                            let keys = p.index.as_ref().map_or(String::new(), |x| {
                                format!(" · {} Keyframes", x.key_count())
                            });
                            let text = format!(
                                "{}×{} · {} · {:.3} fps{keys}{clock}{dropped}{sync_txt}",
                                i.width, i.height, i.codec, i.fps
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(text)
                                            .monospace()
                                            .size(12.0)
                                            .color(ui::TEXT_DIM),
                                    );
                                },
                            );
                        }
                    });
                    let w = ui.available_width();
                    let band = p.loop_band().map(|(start, end)| LoopBand {
                        start,
                        end,
                        active: p.loop_on,
                    });
                    if let Some(f) = timeline::show(ui, w, frac, &p.key_fracs, band) {
                        p.seek_time(f64::from(f) * duration);
                    }
                });
        });
    }
}

fn menu_button(ui: &mut egui::Ui, label: &str, hint: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(13.0)).frame(false),
    )
    .on_hover_text(hint)
    .clicked()
}

/// Keyframe-Status des angezeigten Frames: bevorzugt aus dem Index, sonst Decoder-Flag.
fn is_key(p: &Player, pts: f64, decoder_flag: bool) -> bool {
    match &p.index {
        Some(idx) => idx.is_key(idx.frame_at(pts)),
        None => decoder_flag,
    }
}

/// Texte „F 42 / 600“ und Timecode des aktuellen Frames.
fn frame_texts(p: &Player) -> (String, String) {
    let Some(cur) = &p.current else {
        return ("F – / –".into(), "--:--:--:--".into());
    };
    match (&p.index, p.frame_no()) {
        (Some(idx), Some(n)) => (
            format!("F {n} / {}", idx.len()),
            timecode::format(cur.pts, idx.frame_in_second(n)),
        ),
        _ => ("F – / –".into(), timecode::format_nominal(cur.pts, p.fps())),
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_input(&ctx);
        if let Some(p) = self.player.as_mut() {
            p.poll();
            self.sync.update(Some(p));
            if let Some(e) = p.error.take() {
                self.error = Some(e);
                self.player = None;
            }
        }
        if self.player.is_none() {
            self.sync.update(None);
        }
        self.upload_texture(&ctx);

        let (time, moved) = ctx.input(|i| {
            (
                i.time,
                i.pointer.delta() != Vec2::ZERO || i.pointer.any_down(),
            )
        });
        if moved {
            self.last_activity = time;
        }
        let playing = self.player.as_ref().is_some_and(|p| p.playing);
        let controls_visible =
            !playing || self.player.is_none() || time - self.last_activity < CONTROLS_TIMEOUT;

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(ui, |ui| {
                if self.texture.is_some() {
                    self.draw_video(ui, &ctx);
                } else {
                    self.draw_placeholder(ui);
                }
            });
        if controls_visible {
            self.draw_menu(&ctx);
            self.draw_controls(&ctx);
        } else if self.texture.is_some() {
            ctx.set_cursor_icon(CursorIcon::None);
        }
        if self.hud {
            self.draw_hud(&ctx);
        }
        self.draw_help(&ctx);
        self.draw_toast(&ctx);
        self.bench_tick(&ctx, time);

        if self.sync.available() {
            // Lebenszeichen an andere Instanzen auch im Leerlauf.
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if playing {
            ctx.request_repaint();
        } else if controls_visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Frisch laden, damit parallel laufende Instanzen sich nicht gegenseitig überschreiben.
        let mut s = Settings::load();
        s.volume = self.volume;
        s.muted = self.muted;
        s.hud = self.hud;
        if self.export_dir_changed {
            s.export_dir = self.settings.export_dir.clone();
        }
        s.save();
    }
}
