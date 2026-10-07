//! Anwendungszustand: Player-Uhr, Event-Verarbeitung, Eingabe und Zeichnen.

use crate::decoder::{Command, DecoderHandle, Event, Frame, VideoInfo};
use crate::{timeline, ui};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Key, Modifiers, Rect, Stroke, TextureOptions, Vec2,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Wie lange die Controls nach der letzten Mausbewegung sichtbar bleiben.
const CONTROLS_TIMEOUT: f64 = 2.5;

struct Player {
    handle: DecoderHandle,
    info: Option<VideoInfo>,
    serial: u64,
    playing: bool,
    /// Position zum Zeitpunkt `base_time` (die Uhr läuft nur bei `playing`).
    base_pos: f64,
    base_time: Instant,
    /// Nach einem Seek: erster ankommender Frame wird sofort angezeigt.
    awaiting_frame: bool,
    pending: Option<Arc<Frame>>,
    current_pts: f64,
    current_key: bool,
    eof: bool,
}

impl Player {
    fn position(&self) -> f64 {
        if self.playing && !self.awaiting_frame {
            self.base_pos + self.base_time.elapsed().as_secs_f64()
        } else {
            self.base_pos
        }
    }

    fn duration(&self) -> f64 {
        self.info.as_ref().map_or(0.0, |i| i.duration)
    }

    fn seek(&mut self, time: f64) {
        let time = time.clamp(0.0, self.duration().max(0.0));
        self.serial += 1;
        self.base_pos = time;
        self.base_time = Instant::now();
        self.awaiting_frame = true;
        self.pending = None;
        self.eof = false;
        let _ = self.handle.cmd.send(Command::Seek {
            serial: self.serial,
            time,
        });
    }

    fn set_playing(&mut self, play: bool) {
        if play == self.playing {
            return;
        }
        if play && self.eof && self.pending.is_none() {
            // Am Ende: von vorn beginnen.
            self.seek(0.0);
        }
        self.base_pos = self.position();
        self.base_time = Instant::now();
        self.playing = play;
    }

    fn toggle(&mut self) {
        self.set_playing(!self.playing);
    }
}

pub struct PlayerApp {
    player: Option<Player>,
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
        let handle = DecoderHandle::spawn(path.clone(), move || wake_ctx.request_repaint());
        self.error = None;
        self.texture = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} – FrameScope",
            path.file_name()
                .map_or_else(|| "Video".into(), |n| n.to_string_lossy())
        )));
        self.player = Some(Player {
            handle,
            info: None,
            serial: 0,
            playing: true,
            base_pos: 0.0,
            base_time: Instant::now(),
            awaiting_frame: true,
            pending: None,
            current_pts: 0.0,
            current_key: false,
            eof: false,
        });
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

    /// Verarbeitet Decoder-Events und wählt den Frame passend zur Uhr.
    fn pump_events(&mut self, ctx: &egui::Context) {
        let mut error = None;
        let mut to_show: Option<Arc<Frame>> = None;
        if let Some(p) = self.player.as_mut() {
            loop {
                let candidate = match p.pending.take() {
                    Some(f) => f,
                    None => match p.handle.events.try_recv() {
                        Ok(Event::Opened(info)) => {
                            p.info = Some(info);
                            continue;
                        }
                        Ok(Event::Frame(f)) => f,
                        Ok(Event::Eof(s)) => {
                            if s == p.serial {
                                p.eof = true;
                            }
                            continue;
                        }
                        Ok(Event::Error(e)) => {
                            error = Some(e);
                            break;
                        }
                        Err(_) => break,
                    },
                };
                if candidate.serial != p.serial {
                    continue; // veraltet (vor einem Seek dekodiert)
                }
                if p.awaiting_frame {
                    p.awaiting_frame = false;
                    p.base_pos = candidate.pts;
                    p.base_time = Instant::now();
                    to_show = Some(candidate);
                } else if candidate.pts <= p.position() {
                    to_show = Some(candidate);
                } else {
                    p.pending = Some(candidate);
                    break;
                }
            }
            if let Some(f) = &to_show {
                p.current_pts = f.pts;
                p.current_key = f.key;
            }
            // Ende erreicht und letzter Frame angezeigt → stoppen.
            if p.eof && p.pending.is_none() && p.playing {
                p.base_pos = p.current_pts;
                p.playing = false;
            }
        }
        if let Some(e) = error {
            self.error = Some(e);
            self.player = None;
        }
        if let Some(f) = to_show {
            let image = egui::ColorImage::from_rgba_unmultiplied([f.width, f.height], &f.rgba);
            match self.texture.as_mut() {
                Some(t) => t.set(image, TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture("video", image, TextureOptions::LINEAR))
                }
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let (dropped, open, new_win, space) = ctx.input_mut(|i| {
            (
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .next(),
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::N),
                i.consume_key(Modifiers::NONE, Key::Space),
            )
        });
        if let Some(path) = dropped {
            self.open(ctx, path);
        }
        if open {
            self.open_dialog(ctx);
        }
        if new_win {
            Self::new_window();
        }
        if space {
            if let Some(p) = self.player.as_mut() {
                p.toggle();
            }
        }
        while let Ok(path) = self.open_rx.try_recv() {
            self.open(ctx, path);
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
        let err = self.error.is_some();
        p.text(
            c - Vec2::new(0.0, 14.0),
            Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(22.0),
            if err {
                Color32::from_rgb(0xff, 0x7a, 0x70)
            } else {
                ui::TEXT
            },
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
                    let pos = p
                        .position()
                        .min(if duration > 0.0 { duration } else { f64::MAX });
                    let frac = if duration > 0.0 {
                        (pos / duration) as f32
                    } else {
                        0.0
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
                        let time = format!("{} / {}", ui::fmt_time(pos), ui::fmt_time(duration));
                        ui.label(egui::RichText::new(time).monospace().size(14.0));
                        if p.current_key {
                            ui.label(
                                egui::RichText::new("KEY")
                                    .monospace()
                                    .size(12.0)
                                    .color(ui::KEY),
                            );
                        }
                        if let Some(i) = &p.info {
                            let text = format!(
                                "{}×{} · {} · {:.3} fps",
                                i.width, i.height, i.codec, i.fps
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(text)
                                            .monospace()
                                            .size(12.0)
                                            .color(ui::TEXT_DIM),
                                    );
                                },
                            );
                        }
                    });
                    let w = ui.available_width();
                    if let Some(f) = timeline::show(ui, w, frac) {
                        p.seek(f64::from(f) * duration);
                    }
                });
        });
    }
}

impl eframe::App for PlayerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_input(&ctx);
        self.pump_events(&ctx);

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
