//! Anwendungsfenster: Eingabe, Zeichnen von Video, Controls und Overlay.

use crate::compare::{self, Compare};
use crate::decoder::Frame;
use crate::player::Player;
use crate::settings::Settings;
use crate::sync::SyncController;
use crate::timeline::LoopBand;
use crate::{export, timecode, timeline, ui, winutil};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Key, Modifiers, Rect, RichText, Sense, Stroke,
    TextureOptions, Vec2,
};
use egui_phosphor::regular as ph;
use std::path::PathBuf;
use std::sync::Arc;

/// Höhe der Control-Leiste unten (Verlauf, Timeline, Schaltflächen), in Punkten.
const CONTROLS_HEIGHT: f32 = 144.0;
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
    OpenB(PathBuf),
    Toast { text: String, error: bool },
    ExportDir(PathBuf),
}

pub struct PlayerApp {
    player: Option<Player>,
    compare: Option<Compare>,
    uploaded: Option<Arc<Frame>>,
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
    always_on_top: bool,
    /// Das Fenster wird gerade per Ziehen im Bild verschoben (nicht den Schieber bewegen).
    window_drag: bool,
    toast: Option<(String, bool, f64)>,
    msg_tx: Sender<Msg>,
    msg_rx: Receiver<Msg>,
    /// Entwickler-Benchmark: nach N Sekunden Statistik schreiben und beenden.
    bench: Option<(f64, f64)>,
    /// Bench-Statistik der Oberfläche: letzte Zeit, Anzahl später UI-Frames, größtes Intervall.
    ui_stats: (f64, u64, f64),
}

impl PlayerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial: Option<PathBuf>,
        initial_b: Option<PathBuf>,
        sync_on_start: bool,
    ) -> Self {
        let (msg_tx, msg_rx) = unbounded();
        let settings = Settings::load();
        let mut app = Self {
            player: None,
            compare: None,
            uploaded: None,
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
            always_on_top: false,
            window_drag: false,
            toast: None,
            msg_tx,
            msg_rx,
            ui_stats: (-1.0, 0, 0.0),
            bench: std::env::var("FRAMESCOPE_BENCH")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(|secs| (secs, -1.0)),
        };
        app.sync.set_enabled(sync_on_start, None);
        if let Some(path) = initial {
            app.open(&cc.egui_ctx, path);
            if let Some(b) = initial_b {
                app.open_b(&cc.egui_ctx, b);
            }
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
            true,
        ));
        if let Some(c) = self.compare.as_mut() {
            c.invalidate();
        }
    }

    /// Öffnet `path` als Video B im Vergleichsmodus.
    fn open_b(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.player.is_none() {
            self.toast(ctx, "Öffne zuerst Video A", true);
            return;
        }
        let wake_ctx = ctx.clone();
        let player = Player::new(path, move || wake_ctx.request_repaint(), 0.0, true, false);
        let mut new = Compare::new(player);
        if let Some(old) = self.compare.as_ref() {
            new.mode = old.mode;
            new.split = old.split;
        }
        let title = {
            let a = self
                .player
                .as_ref()
                .map_or_else(String::new, |p| file_label(&p.path));
            format!("{a} vs {} – FrameScope", file_label(&new.player.path))
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        self.compare = Some(new);
    }

    fn close_compare(&mut self, ctx: &egui::Context) {
        if self.compare.take().is_some() {
            self.toast(ctx, "Vergleich beendet", false);
        }
    }

    /// Dateidialog in eigenem Thread, damit das Fenster nicht blockiert.
    fn open_dialog(&self, ctx: &egui::Context, as_b: bool) {
        let (tx, ctx) = (self.msg_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .add_filter("Video", VIDEO_EXTENSIONS)
                .add_filter("Alle Dateien", &["*"])
                .pick_file();
            if let Some(p) = picked {
                let _ = tx.send(if as_b { Msg::OpenB(p) } else { Msg::Open(p) });
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

    /// Fenster immer im Vordergrund halten (oder zurück auf normal).
    fn set_always_on_top(&mut self, ctx: &egui::Context, on: bool) {
        self.always_on_top = on;
        let level = if on {
            egui::viewport::WindowLevel::AlwaysOnTop
        } else {
            egui::viewport::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    /// Alle offenen FrameScope-Fenster gleich groß und lückenlos im Raster anordnen.
    fn arrange_windows(&mut self, ctx: &egui::Context) {
        match winutil::arrange_windows() {
            Ok(1) => self.toast(
                ctx,
                "Nur ein Fenster geöffnet – füllt jetzt den Bildschirm",
                false,
            ),
            Ok(n) => self.toast(ctx, format!("{n} Fenster angeordnet"), false),
            Err(e) => self.toast(ctx, e, true),
        }
    }

    /// Fenster auf die Originalgröße des Videos setzen (1 Videopixel = 1 Bildschirmpixel).
    fn fit_window_to_video(&mut self, ctx: &egui::Context) {
        let Some((w, h)) = self
            .player
            .as_ref()
            .and_then(|p| p.info.as_ref())
            .map(|i| (i.width, i.height))
        else {
            self.toast(ctx, "Kein Video geladen", true);
            return;
        };
        if self.fullscreen {
            self.toast(ctx, "Erst Vollbild beenden (F)", true);
            return;
        }
        let (want_w, want_h) = (
            i32::try_from(w).unwrap_or(i32::MAX),
            i32::try_from(h).unwrap_or(i32::MAX),
        );
        match winutil::fit_own_window(want_w, want_h) {
            Ok((fw, fh)) if (fw, fh) == (want_w, want_h) => {
                self.toast(ctx, format!("Originalgröße: {fw}×{fh}"), false);
            }
            Ok((fw, fh)) => self.toast(
                ctx,
                format!("Video ({want_w}×{want_h}) größer als der Bildschirm – eingepasst auf {fw}×{fh}"),
                false,
            ),
            Err(e) => self.toast(ctx, e, true),
        }
    }

    fn is_maximized(ctx: &egui::Context) -> bool {
        ctx.input(|i| i.viewport().maximized.unwrap_or(false))
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

    /// Lädt neue Frames von A (und B) in Texturen; unveränderte Frames werden nicht erneut hochgeladen.
    fn upload_textures(&mut self, ctx: &egui::Context) {
        if let Some(frame) = self.player.as_ref().and_then(|p| p.current.clone()) {
            upload(
                ctx,
                &frame,
                &mut self.texture,
                &mut self.uploaded,
                "video_a",
            );
        }
        if let Some(c) = self.compare.as_mut() {
            if let Some(frame) = c.player.current.clone() {
                upload(ctx, &frame, &mut c.texture, &mut c.uploaded, "video_b");
            }
        }
    }

    /// Video B pollen und auf Video A ausrichten; Fehler beenden den Vergleich.
    fn update_compare(&mut self, ctx: &egui::Context) {
        let mut failure = None;
        if let (Some(c), Some(a)) = (self.compare.as_mut(), self.player.as_ref()) {
            c.player.poll();
            c.follow_master(a);
            failure = c.player.error.take();
        }
        if let Some(e) = failure {
            self.compare = None;
            self.toast(ctx, format!("Video B: {e}"), true);
        }
    }

    fn drain_messages(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                Msg::Open(path) => self.open(ctx, path),
                Msg::OpenB(path) => self.open_b(ctx, path),
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
            top: bool,
            arrange: bool,
            original: bool,
            compare_toggle: bool,
            compare_mode: bool,
            drop_to_b: bool,
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
        let compare_active = self.compare.is_some();
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
            compare_toggle: i.consume_key(Modifiers::COMMAND, Key::B),
            compare_mode: i.consume_key(Modifiers::NONE, Key::C),
            drop_to_b: i.modifiers.shift
                || (compare_active
                    && i.pointer
                        .hover_pos()
                        .is_some_and(|p| p.x > i.content_rect().center().x)),
            sync_align: i.consume_key(Modifiers::SHIFT, Key::Y),
            top: i.consume_key(Modifiers::NONE, Key::T),
            original: i.consume_key(Modifiers::NONE, Key::Num1),
            arrange: i.consume_key(Modifiers::NONE, Key::G),
            sync: i.consume_key(Modifiers::NONE, Key::Y),
        });
        if let Some(path) = k.dropped {
            if k.drop_to_b && self.player.is_some() {
                self.open_b(ctx, path);
            } else {
                self.open(ctx, path);
            }
        }
        if k.compare_toggle {
            if self.compare.is_some() {
                self.close_compare(ctx);
            } else {
                self.open_dialog(ctx, true);
            }
        }
        if k.compare_mode {
            if let Some(c) = self.compare.as_mut() {
                c.mode = c.mode.next();
            }
        }
        if k.open {
            self.open_dialog(ctx, false);
        }
        if k.new_win {
            self.new_window();
        }
        self.drain_messages(ctx);
        if k.original {
            self.fit_window_to_video(ctx);
        }
        if k.top {
            self.set_always_on_top(ctx, !self.always_on_top);
        }
        if k.arrange {
            self.arrange_windows(ctx);
        }
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
        ui.painter().rect_filled(full, 0.0, ui::BG);
        let resp = ui.interact(full, egui::Id::new("video_area"), Sense::click_and_drag());
        if !resp.dragged() {
            self.window_drag = false;
        }
        // Ziehen im Bild verschiebt das Fenster – im Schieber-/Überblenden-Modus nur am oberen
        // Rand (wie eine Titelleiste) oder mit gedrückter Alt-Taste, sonst bewegt es den Schieber.
        if resp.drag_started_by(egui::PointerButton::Primary) {
            let alt = ctx.input(|i| i.modifiers.alt);
            let in_top_strip = resp
                .interact_pointer_pos()
                .is_some_and(|p| p.y < full.top() + 44.0);
            let slider_mode = self
                .compare
                .as_ref()
                .is_some_and(|c| matches!(c.mode, compare::Mode::Wipe | compare::Mode::Blend));
            if alt || in_top_strip || !slider_mode {
                self.window_drag = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let split_drag = resp.dragged() && !self.window_drag;
        // Klick ins Bild: Wiedergabe/Pause. Doppelklick: Vollbild (der erste Klick hat schon
        // umgeschaltet, deshalb hier zurückschalten).
        if resp.double_clicked() {
            self.set_fullscreen(ctx, !self.fullscreen);
            if let Some(p) = self.player.as_mut() {
                p.toggle();
            }
        } else if resp.clicked() {
            if let Some(p) = self.player.as_mut() {
                p.toggle();
            }
        }
        let Some(tex_a) = self.texture.clone() else {
            return;
        };
        let painter = ui.painter().clone();
        let name_a = self
            .player
            .as_ref()
            .map_or_else(String::new, |p| file_label(&p.path));

        let Some(c) = self.compare.as_mut() else {
            let rect = compare::fit(full, tex_a.size_vec2());
            painter.image(tex_a.id(), rect, compare::FULL_UV, Color32::WHITE);
            return;
        };
        let tex_b = c.texture.clone();
        let name_b = file_label(&c.player.path);
        let pointer_x = resp.interact_pointer_pos().map(|p| p.x);

        match c.mode {
            compare::Mode::Wipe => {
                let rect = compare::fit(full, tex_a.size_vec2());
                painter.image(tex_a.id(), rect, compare::FULL_UV, Color32::WHITE);
                if split_drag {
                    if let Some(x) = pointer_x {
                        c.split = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    }
                }
                let x = rect.left() + c.split * rect.width();
                if let Some(tb) = &tex_b {
                    if let Some((part, uv)) = compare::right_part(rect, x) {
                        painter.image(tb.id(), part, uv, Color32::WHITE);
                    }
                }
                // Schieber: Linie mit Griff.
                let near = ctx
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|p| (p.x - x).abs() < 14.0 && rect.contains(p));
                if near || split_drag {
                    ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(3.0, Color32::from_black_alpha(120)),
                );
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1.5, Color32::WHITE),
                );
                let mid = egui::pos2(x, rect.center().y);
                painter.circle_filled(mid, 15.0, Color32::from_black_alpha(170));
                painter.circle_stroke(mid, 15.0, Stroke::new(1.5, Color32::WHITE));
                for dir in [-1.0f32, 1.0] {
                    let tip = mid + Vec2::new(dir * 9.0, 0.0);
                    let base = mid + Vec2::new(dir * 3.0, 0.0);
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, base + Vec2::new(0.0, -5.0), base + Vec2::new(0.0, 5.0)],
                        Color32::WHITE,
                        Stroke::NONE,
                    ));
                }
                chip(
                    &painter,
                    rect.left_top() + Vec2::new(10.0, 58.0),
                    Align2::LEFT_TOP,
                    &format!("A  {name_a}"),
                );
                chip(
                    &painter,
                    rect.right_top() + Vec2::new(-10.0, 58.0),
                    Align2::RIGHT_TOP,
                    &format!("B  {name_b}"),
                );
            }
            compare::Mode::SideBySide => {
                let mid = full.center().x;
                let left = Rect::from_min_max(full.min, egui::pos2(mid - 1.0, full.max.y));
                let right = Rect::from_min_max(egui::pos2(mid + 1.0, full.min.y), full.max);
                let rect_a = compare::fit(left, tex_a.size_vec2());
                painter.image(tex_a.id(), rect_a, compare::FULL_UV, Color32::WHITE);
                chip(
                    &painter,
                    rect_a.left_top() + Vec2::new(10.0, 58.0),
                    Align2::LEFT_TOP,
                    &format!("A  {name_a}"),
                );
                if let Some(tb) = &tex_b {
                    let rect_b = compare::fit(right, tb.size_vec2());
                    painter.image(tb.id(), rect_b, compare::FULL_UV, Color32::WHITE);
                    chip(
                        &painter,
                        rect_b.left_top() + Vec2::new(10.0, 58.0),
                        Align2::LEFT_TOP,
                        &format!("B  {name_b}"),
                    );
                }
                painter.line_segment(
                    [egui::pos2(mid, full.top()), egui::pos2(mid, full.bottom())],
                    Stroke::new(2.0, Color32::from_gray(60)),
                );
            }
            compare::Mode::Blend => {
                let rect = compare::fit(full, tex_a.size_vec2());
                painter.image(tex_a.id(), rect, compare::FULL_UV, Color32::WHITE);
                if split_drag {
                    if let Some(x) = pointer_x {
                        c.split = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                    }
                }
                if let Some(tb) = &tex_b {
                    painter.image(tb.id(), rect, compare::FULL_UV, compare::fade(c.split));
                }
                chip(
                    &painter,
                    rect.left_top() + Vec2::new(10.0, 58.0),
                    Align2::LEFT_TOP,
                    &format!("A {name_a}  ↔  B {name_b}  ({:.0} % B)", c.split * 100.0),
                );
            }
        }
    }

    fn draw_placeholder(&self, ui: &mut egui::Ui) {
        let full = ui.max_rect();
        if ui
            .interact(full, egui::Id::new("placeholder_area"), Sense::drag())
            .drag_started()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
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
            ui::ERROR
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
            ui::N400,
        );
    }

    /// Dünner Rand um das rahmenlose Fenster und unsichtbare Griffe zum Ändern der Größe.
    fn draw_window_frame(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let full = ui.max_rect();
        ui.painter().rect_stroke(
            full,
            0.0,
            Stroke::new(1.0, ui::alpha(ui::N700, 0.7)),
            egui::StrokeKind::Inside,
        );
        if self.fullscreen || Self::is_maximized(ctx) {
            return;
        }
        use egui::viewport::ResizeDirection as Dir;
        const EDGE: f32 = 5.0;
        const CORNER: f32 = 12.0;
        let (l, t, r, b) = (full.left(), full.top(), full.right(), full.bottom());
        let grips: [(&str, Rect, Dir, CursorIcon); 8] = [
            (
                "n",
                Rect::from_min_max(egui::pos2(l + CORNER, t), egui::pos2(r - CORNER, t + EDGE)),
                Dir::North,
                CursorIcon::ResizeVertical,
            ),
            (
                "s",
                Rect::from_min_max(egui::pos2(l + CORNER, b - EDGE), egui::pos2(r - CORNER, b)),
                Dir::South,
                CursorIcon::ResizeVertical,
            ),
            (
                "w",
                Rect::from_min_max(egui::pos2(l, t + CORNER), egui::pos2(l + EDGE, b - CORNER)),
                Dir::West,
                CursorIcon::ResizeHorizontal,
            ),
            (
                "e",
                Rect::from_min_max(egui::pos2(r - EDGE, t + CORNER), egui::pos2(r, b - CORNER)),
                Dir::East,
                CursorIcon::ResizeHorizontal,
            ),
            (
                "nw",
                Rect::from_min_size(egui::pos2(l, t), Vec2::splat(CORNER)),
                Dir::NorthWest,
                CursorIcon::ResizeNwSe,
            ),
            (
                "ne",
                Rect::from_min_size(egui::pos2(r - CORNER, t), Vec2::splat(CORNER)),
                Dir::NorthEast,
                CursorIcon::ResizeNeSw,
            ),
            (
                "sw",
                Rect::from_min_size(egui::pos2(l, b - CORNER), Vec2::splat(CORNER)),
                Dir::SouthWest,
                CursorIcon::ResizeNeSw,
            ),
            (
                "se",
                Rect::from_min_size(egui::pos2(r - CORNER, b - CORNER), Vec2::splat(CORNER)),
                Dir::SouthEast,
                CursorIcon::ResizeNwSe,
            ),
        ];
        for (name, rect, dir, cursor) in grips {
            let resp = ui.interact(rect, egui::Id::new(("resize_grip", name)), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ctx.set_cursor_icon(cursor);
            }
            if resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            }
        }
    }

    /// Schaltflächen oben rechts: Datei, Fenster, Vergleich, Sync, Vordergrund, Fenstersteuerung.
    fn draw_menu(&mut self, ctx: &egui::Context) {
        let (mut open, mut new_win, mut help, mut sync) = (false, false, false, false);
        let (mut cmp_toggle, mut cmp_mode) = (false, false);
        let (mut pin, mut arrange, mut original) = (false, false, false);
        let (mut minimize, mut maximize, mut close) = (false, false, false);
        let compare_label = self.compare.as_ref().map(|c| c.mode.label());
        let has_a = self.player.is_some();
        let maximized = Self::is_maximized(ctx);
        // In schmalen Fenstern nur Icons (der Tooltip nennt die Funktion).
        let compact = ctx.content_rect().width() < 980.0;
        let label = |text: &str| {
            if compact {
                String::new()
            } else {
                text.to_owned()
            }
        };
        let sync_label = if self.sync.enabled {
            label(&format!("Sync · {}", self.sync.synced_peers()))
        } else {
            label("Sync")
        };
        egui::Area::new(egui::Id::new("menu"))
            .anchor(Align2::RIGHT_TOP, [-14.0, 14.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    open = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::FOLDER_OPEN)), "", false, "Datei öffnen (Strg+O)").clicked();
                    new_win = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::BROWSERS)), "", false, "Neues Fenster (Strg+N)").clicked();
                    arrange = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::SQUARES_FOUR)), "", false, "Alle FrameScope-Fenster gleich groß und lückenlos anordnen (G)").clicked();
                    if has_a {
                        original = ui::ghost_button(
                            ui,
                            Some(ui::Glyph::Regular(ph::FRAME_CORNERS)),
                            "",
                            false,
                            "Fenster auf Originalgröße des Videos (1): 1 Videopixel = 1 Bildschirmpixel",
                        )
                        .clicked();
                    }
                    pin = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::PUSH_PIN)), "", self.always_on_top, "Immer im Vordergrund (T)").clicked();
                    help = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::QUESTION)), "", self.help, "Tastenkürzel (F1)").clicked();
                    if let Some(mode) = compare_label {
                        cmp_mode = ui::ghost_button(
                            ui,
                            Some(ui::Glyph::Regular(ph::ARROWS_LEFT_RIGHT)),
                            &label(&format!("Modus: {mode}")),
                            false,
                            &format!("Vergleichsmodus wechseln (C) – aktuell: {mode}"),
                        )
                        .clicked();
                    }
                    if has_a {
                        cmp_toggle = ui::ghost_button(
                            ui,
                            Some(ui::Glyph::Regular(ph::COLUMNS)),
                            &label(if compare_label.is_some() { "Vergleich beenden" } else { "Vergleichen…" }),
                            compare_label.is_some(),
                            "Zweites Video B zum Vergleich öffnen / beenden (Strg+B)",
                        )
                        .clicked();
                    }
                    sync = ui::ghost_button(
                        ui,
                        Some(ui::Glyph::Regular(ph::LINK)),
                        &sync_label,
                        self.sync.enabled,
                        "Wiedergabe mit anderen FrameScope-Fenstern synchronisieren (Y, Umschalt+Y: Versatz abgleichen)",
                    )
                    .clicked();
                    ui.add_space(8.0);
                    // Fenstersteuerung (das Fenster hat keine Titelleiste).
                    minimize = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::MINUS)), "", false, "Minimieren").clicked();
                    let max_glyph = if maximized { ph::COPY } else { ph::SQUARE };
                    maximize = ui::ghost_button(ui, Some(ui::Glyph::Regular(max_glyph)), "", false, if maximized { "Wiederherstellen" } else { "Maximieren" }).clicked();
                    close = ui::ghost_button(ui, Some(ui::Glyph::Regular(ph::X)), "", false, "Schließen").clicked();
                });
            });
        if open {
            self.open_dialog(ctx, false);
        }
        if new_win {
            self.new_window();
        }
        if arrange {
            self.arrange_windows(ctx);
        }
        if original {
            self.fit_window_to_video(ctx);
        }
        if pin {
            self.set_always_on_top(ctx, !self.always_on_top);
        }
        if help {
            self.help = !self.help;
        }
        if sync {
            self.toggle_sync(ctx);
        }
        if cmp_toggle {
            if self.compare.is_some() {
                self.close_compare(ctx);
            } else {
                self.open_dialog(ctx, true);
            }
        }
        if cmp_mode {
            if let Some(c) = self.compare.as_mut() {
                c.mode = c.mode.next();
            }
        }
        if minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if maximize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Dauerhaftes Info-Overlay oben links (für Vergleiche und Screenshots).
    fn draw_hud(&self, ctx: &egui::Context) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let Some(cur) = &p.current else { return };
        let (frame_txt, _, tc_txt) = frame_parts(p);
        let key = is_key(p, cur.pts, cur.key);
        egui::Area::new(egui::Id::new("hud"))
            .anchor(Align2::LEFT_TOP, [14.0, 14.0])
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::alpha(ui::BG, 0.78))
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(RichText::new(frame_txt).size(12.0));
                            ui.label(RichText::new(tc_txt).size(12.0).color(ui::N300));
                            if key {
                                ui::tag(ui, "KEY");
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
            ("Klick ins Bild", "Wiedergabe / Pause"),
            (
                "Ziehen im Bild",
                "Fenster verschieben (Alt + Ziehen: immer)",
            ),
            ("1", "Fenster auf Originalgröße des Videos"),
            ("T  ·  G", "Immer im Vordergrund  ·  Fenster anordnen"),
            ("Strg + B  ·  C", "Vergleich mit Video B  ·  Modus wechseln"),
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
                    .fill(ui::SURFACE)
                    .corner_radius(8.0)
                    .stroke(Stroke::new(1.0, ui::N700))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    })
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                egui::Grid::new("help_grid")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        for (keys, what) in ROWS {
                            ui.label(RichText::new(*keys).size(13.0).color(ui::A300));
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
        if p.playing {
            let (last, late, max) = &mut self.ui_stats;
            if *last >= 0.0 {
                let dt = time - *last;
                *max = max.max(dt);
                if dt > 1.5 / p.fps() {
                    *late += 1;
                }
            }
            *last = time;
        }
        if time - *start >= *secs {
            let (_, ui_late, ui_max) = self.ui_stats;
            let text = format!(
                "shown={} dropped={} starved={} seeks={} ui_late={} ui_max_ms={:.1} wall={:.2}s position={:.2}s
",
                p.shown,
                p.dropped,
                p.starved,
                p.hard_seeks,
                ui_late,
                ui_max * 1000.0,
                time - *start,
                p.position()
            );
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::fs::write(
                    exe.with_file_name(format!("framescope-bench-{}.txt", std::process::id())),
                    text,
                );
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
            .anchor(Align2::LEFT_BOTTOM, [16.0, -(CONTROLS_HEIGHT + 8.0)])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::SURFACE)
                    .corner_radius(8.0)
                    .stroke(Stroke::new(1.0, ui::N700))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        let color = if error { ui::ERROR } else { ui::TEXT };
                        ui.label(RichText::new(text).size(13.0).color(color));
                    });
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    /// Control-Leiste unten: Verlauf über dem Video, Timeline, Schaltflächen, Zähler.
    fn draw_controls(&mut self, ctx: &egui::Context) {
        let b_txt = self
            .compare
            .as_ref()
            .map(|c| format!("B {}", frame_texts(&c.player).0));
        let Some(p) = self.player.as_mut() else {
            return;
        };

        const PAD_X: f32 = 18.0;
        const PAD_BOTTOM: f32 = 14.0;
        const ROW: f32 = 36.0;
        const TIMELINE: f32 = 22.0;
        const GAP: f32 = 8.0;
        let screen = ctx.content_rect();
        let bar = Rect::from_min_max(
            egui::pos2(screen.left(), screen.bottom() - CONTROLS_HEIGHT),
            screen.right_bottom(),
        );
        // Verlauf liegt unter den Widgets und fängt keine Mauseingaben ab.
        let fade = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("controls_fade"),
        ));
        ui::fade_up(&fade, bar, ui::BG, 0.94);

        let inner_w = screen.width() - 2.0 * PAD_X;
        let top_left = egui::pos2(
            screen.left() + PAD_X,
            screen.bottom() - PAD_BOTTOM - ROW - GAP - TIMELINE,
        );
        let (mut want_png, mut want_fullscreen) = (false, false);
        let fullscreen = self.fullscreen;
        let info_tip = p.info.as_ref().map(|i| {
            let keys = p
                .index
                .as_ref()
                .map_or(String::new(), |x| format!(" · {} Keyframes", x.key_count()));
            let clock = if p.audio_is_master() {
                " · Audio-Clock"
            } else {
                ""
            };
            format!(
                "{}×{} · {} · {:.3} fps{keys}{clock}",
                i.width, i.height, i.codec, i.fps
            )
        });

        egui::Area::new(egui::Id::new("controls"))
            .fixed_pos(top_left)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(inner_w);
                ui.spacing_mut().item_spacing = egui::vec2(0.0, GAP);
                let duration = p.duration();
                let cur = p.current.as_ref().map(|f| (f.pts, f.key));
                let frac = match cur {
                    Some((pts, _)) if duration > 0.0 => (pts / duration) as f32,
                    _ => 0.0,
                };

                let band = p.loop_band().map(|(start, end)| LoopBand {
                    start,
                    end,
                    active: p.loop_on,
                });
                if let Some(f) = timeline::show(ui, inner_w, frac, &p.key_fracs, band) {
                    p.seek_time(f64::from(f) * duration);
                }

                ui.allocate_ui_with_layout(
                    egui::vec2(inner_w, ROW),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let play = if p.playing {
                            ui::Glyph::Fill(egui_phosphor::fill::PAUSE)
                        } else {
                            ui::Glyph::Fill(egui_phosphor::fill::PLAY)
                        };
                        if ui::ghost_icon(
                            ui,
                            play,
                            ui::ACCENT,
                            false,
                            "Wiedergabe / Pause (Leertaste)",
                        )
                        .clicked()
                        {
                            p.toggle();
                        }
                        if ui::ghost_icon(
                            ui,
                            ui::Glyph::Regular(ph::SKIP_BACK),
                            ui::ACCENT,
                            false,
                            "Voriger Keyframe (Umschalt+←)",
                        )
                        .clicked()
                        {
                            p.step_key_n(-1);
                        }
                        if ui::ghost_icon(
                            ui,
                            ui::Glyph::Regular(ph::CARET_LEFT),
                            ui::ACCENT,
                            false,
                            "Ein Frame zurück (←)",
                        )
                        .clicked()
                        {
                            p.step(-1);
                        }
                        if ui::ghost_icon(
                            ui,
                            ui::Glyph::Regular(ph::CARET_RIGHT),
                            ui::ACCENT,
                            false,
                            "Ein Frame vor (→)",
                        )
                        .clicked()
                        {
                            p.step(1);
                        }
                        if ui::ghost_icon(
                            ui,
                            ui::Glyph::Regular(ph::SKIP_FORWARD),
                            ui::ACCENT,
                            false,
                            "Nächster Keyframe (Umschalt+→)",
                        )
                        .clicked()
                        {
                            p.step_key_n(1);
                        }
                        ui.add_space(8.0);

                        // Zähler: „F 3240 / 7200“, Timecode, KEY – mit Tabellenziffern.
                        let (frame_txt, total_txt, tc_txt) = frame_parts(p);
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let counter = ui.add(
                            egui::Label::new(RichText::new(frame_txt).size(13.0))
                                .sense(Sense::hover()),
                        );
                        ui.label(RichText::new(total_txt).size(13.0).color(ui::N400));
                        ui.add_space(6.0);
                        ui.label(RichText::new(tc_txt).size(13.0).color(ui::N300));
                        if let Some(tip) = &info_tip {
                            counter.on_hover_text(tip);
                        }
                        ui.add_space(4.0);
                        if cur.is_some_and(|(pts, key)| is_key(p, pts, key)) {
                            ui::tag(ui, "KEY");
                        }
                        if let Some(b) = &b_txt {
                            ui.add_space(6.0);
                            ui.label(RichText::new(b).size(12.0).color(ui::N400));
                        }
                        if p.dropped > 0 || p.starved > 0 {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} verworfen · {}× Decoder zu langsam",
                                    p.dropped, p.starved
                                ))
                                .size(11.0)
                                .color(ui::ERROR),
                            )
                            .on_hover_text(
                                "verworfen: Anzeige kam nicht hinterher (Oberfläche/Grafik) ·                                  Decoder zu langsam: der nächste Frame war nicht rechtzeitig dekodiert",
                            );
                        }

                        // Rechte Gruppe (von rechts nach links).
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let fs = if fullscreen {
                                ui::Glyph::Regular(ph::CORNERS_IN)
                            } else {
                                ui::Glyph::Regular(ph::CORNERS_OUT)
                            };
                            if ui::ghost_icon(ui, fs, ui::ACCENT, false, "Vollbild (F)").clicked() {
                                want_fullscreen = true;
                            }
                            if p.has_audio() {
                                let mut vol = self.volume;
                                if ui::volume_bar(ui, &mut vol, self.muted) {
                                    self.volume = vol;
                                    self.muted = false;
                                }
                                let silent = self.muted || self.volume <= 0.0;
                                let g = if silent {
                                    ph::SPEAKER_X
                                } else {
                                    ph::SPEAKER_HIGH
                                };
                                if ui::ghost_icon(
                                    ui,
                                    ui::Glyph::Regular(g),
                                    ui::ACCENT,
                                    false,
                                    "Stumm (M) · Lautstärke ↑/↓",
                                )
                                .clicked()
                                {
                                    self.muted = !self.muted;
                                }
                            }
                            if ui::ghost_icon(
                                ui,
                                ui::Glyph::Regular(ph::CAMERA),
                                ui::ACCENT,
                                false,
                                "Frame als PNG speichern (S, Umschalt+S: Ordner wählen)",
                            )
                            .clicked()
                            {
                                want_png = true;
                            }
                            let loop_color = if p.loop_on { ui::A300 } else { ui::ACCENT };
                            if ui::ghost_icon(
                                ui,
                                ui::Glyph::Regular(ph::REPEAT),
                                loop_color,
                                p.loop_on,
                                "Loop an/aus (L) · I/O setzen Anfang/Ende · X löscht die Marker",
                            )
                            .clicked()
                            {
                                p.loop_on = !p.loop_on;
                            }
                            if p.loop_in.is_some() || p.loop_out.is_some() {
                                let fmt =
                                    |v: Option<usize>| v.map_or(String::new(), |n| n.to_string());
                                let text = format!("Loop {}–{}", fmt(p.loop_in), fmt(p.loop_out));
                                ui.add_space(6.0);
                                ui.label(RichText::new(text).size(12.0).color(ui::N400));
                            }
                        });
                    },
                );
            });
        if want_png {
            self.export_png(ctx, false);
        }
        if want_fullscreen {
            self.set_fullscreen(ctx, !fullscreen);
        }
    }
}

/// Keyframe-Status des angezeigten Frames: bevorzugt aus dem Index, sonst Decoder-Flag.
fn is_key(p: &Player, pts: f64, decoder_flag: bool) -> bool {
    match &p.index {
        Some(idx) => idx.is_key(idx.frame_at(pts)),
        None => decoder_flag,
    }
}

/// „F 42“, „/ 600“ und Timecode des aktuellen Frames (Teile einzeln, für die Zähler-Anzeige).
fn frame_parts(p: &Player) -> (String, String, String) {
    let Some(cur) = &p.current else {
        return ("F –".into(), "/ –".into(), "--:--:--:--".into());
    };
    match (&p.index, p.frame_no()) {
        (Some(idx), Some(n)) => (
            format!("F {n}"),
            format!("/ {}", idx.len()),
            timecode::format(cur.pts, idx.frame_in_second(n)),
        ),
        _ => (
            "F –".into(),
            "/ –".into(),
            timecode::format_nominal(cur.pts, p.fps()),
        ),
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
        self.update_compare(&ctx);
        self.upload_textures(&ctx);

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
            .frame(egui::Frame::new().fill(ui::BG))
            .show(ui, |ui| {
                if self.texture.is_some() {
                    self.draw_video(ui, &ctx);
                } else {
                    self.draw_placeholder(ui);
                }
                self.draw_window_frame(ui, &ctx);
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

/// Lädt `frame` als Textur hoch (nur, wenn es nicht schon die aktuelle Textur ist).
fn upload(
    ctx: &egui::Context,
    frame: &Arc<Frame>,
    tex: &mut Option<egui::TextureHandle>,
    uploaded: &mut Option<Arc<Frame>>,
    name: &str,
) {
    if tex.is_some() && uploaded.as_ref().is_some_and(|u| Arc::ptr_eq(u, frame)) {
        return;
    }
    // RGBA ist opak (Alpha 255) → unmultiplied == premultiplied, direkter Cast ohne Pixelschleife.
    let pixels: Vec<Color32> = bytemuck::cast_slice(&frame.rgba).to_vec();
    let image = egui::ColorImage::new([frame.width, frame.height], pixels);
    match tex.as_mut() {
        Some(t) => t.set(image, TextureOptions::LINEAR),
        None => *tex = Some(ctx.load_texture(name, image, TextureOptions::LINEAR)),
    }
    *uploaded = Some(frame.clone());
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(|| "Video".into(), |n| n.to_string_lossy().into_owned())
}

/// Kleine halbtransparente Beschriftung (A/B-Label im Vergleich).
fn chip(painter: &egui::Painter, pos: egui::Pos2, anchor: Align2, text: &str) {
    let galley =
        painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(12.0), ui::N200);
    let size = galley.size() + Vec2::new(16.0, 8.0);
    let rect = anchor.anchor_size(pos, size);
    painter.rect_filled(rect, 4.0, ui::alpha(ui::BG, 0.78));
    painter.galley(rect.min + Vec2::new(8.0, 4.0), galley, ui::N200);
}
