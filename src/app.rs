//! Anwendungsfenster: Eingabe, Zeichnen von Video, Controls und Overlay.

use crate::player::Player;
use crate::settings::Settings;
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
    error: Option<String>,
    texture: Option<egui::TextureHandle>,
    last_activity: f64,
    fullscreen: bool,
    toast: Option<(String, bool, f64)>,
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,
}

impl PlayerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let (msg_tx, msg_rx) = unbounded();
        let settings = Settings::load();
        let mut app = Self {
            player: None,
            volume: settings.volume,
            muted: settings.muted,
            settings,
            export_dir_changed: false,
            error: None,
            texture: None,
            last_activity: 0.0,
            fullscreen: false,
            toast: None,
            msg_tx,
            msg_rx,
        };
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

    fn new_window() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
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
        let image =
            egui::ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba);
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
        });
        if let Some(path) = k.dropped {
            self.open(ctx, path);
        }
        if k.open {
            self.open_dialog(ctx);
        }
        if k.new_win {
            Self::new_window();
        }
        self.drain_messages(ctx);
        if k.fullscreen {
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
        let (mut open, mut new_win, mut png, mut full) = (false, false, false, false);
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
                        });
                    });
            });
        if open {
            self.open_dialog(ctx);
        }
        if new_win {
            Self::new_window();
        }
        if png {
            self.export_png(ctx, false);
        }
        if full {
            self.set_fullscreen(ctx, !self.fullscreen);
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
                            let clock = if p.audio_is_master() {
                                " · Audio-Clock"
                            } else {
                                ""
                            };
                            let keys = p.index.as_ref().map_or(String::new(), |x| {
                                format!(" · {} Keyframes", x.key_count())
                            });
                            let text = format!(
                                "{}×{} · {} · {:.3} fps{keys}{clock}",
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
            if let Some(e) = p.error.take() {
                self.error = Some(e);
                self.player = None;
            }
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
        self.draw_toast(&ctx);

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
        if self.export_dir_changed {
            s.export_dir = self.settings.export_dir.clone();
        }
        s.save();
    }
}
