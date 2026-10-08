//! Anwendungsfenster: Eingabe, Zeichnen von Video, Controls und Overlay.

use crate::player::Player;
use crate::{timecode, timeline, ui};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Key, Modifiers, Rect, RichText, Stroke, TextureOptions, Vec2,
};
use std::path::PathBuf;

/// Wie lange die Controls nach der letzten Mausbewegung sichtbar bleiben.
const CONTROLS_TIMEOUT: f64 = 2.5;

pub struct PlayerApp {
    player: Option<Player>,
    volume: f32,
    muted: bool,
    error: Option<String>,
    texture: Option<egui::TextureHandle>,
    last_activity: f64,
    open_tx: Sender<PathBuf>,
    open_rx: Receiver<PathBuf>,
}

impl PlayerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let (open_tx, open_rx) = unbounded();
        let mut app = Self {
            player: None,
            volume: 0.8,
            muted: false,
            error: None,
            texture: None,
            last_activity: 0.0,
            open_tx,
            open_rx,
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
        let (tx, ctx) = (self.open_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter(
                    "Video",
                    &[
                        "mp4", "m4v", "mkv", "mov", "webm", "avi", "ts", "mts", "wmv", "flv",
                    ],
                )
                .add_filter("Alle Dateien", &["*"])
                .pick_file();
            if let Some(p) = picked {
                let _ = tx.send(p);
                ctx.request_repaint();
            }
        });
    }

    fn new_window() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
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

    fn handle_input(&mut self, ctx: &egui::Context) {
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
        while let Ok(path) = self.open_rx.try_recv() {
            self.open(ctx, path);
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

    fn draw_video(&self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, Color32::BLACK);
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
                    if let Some(f) = timeline::show(ui, w, frac, &p.key_fracs) {
                        p.seek_time(f64::from(f) * duration);
                    }
                });
        });
    }
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
                    self.draw_video(ui);
                } else {
                    self.draw_placeholder(ui);
                }
            });
        if controls_visible {
            self.draw_controls(&ctx);
        } else if self.texture.is_some() {
            ctx.set_cursor_icon(CursorIcon::None);
        }

        if playing {
            ctx.request_repaint();
        } else if controls_visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}
