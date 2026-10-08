//! A/B-Vergleich: ein zweites Video (B) läuft im selben Fenster auf der Uhr von Video A.

use crate::decoder::Frame;
use crate::player::Player;
use eframe::egui::{self, Color32, Rect};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Schieber über dem Bild: links A, rechts B.
    Wipe,
    SideBySide,
    /// B wird über A eingeblendet (Schieber = Deckkraft von B).
    Blend,
}

impl Mode {
    pub fn next(self) -> Self {
        match self {
            Self::Wipe => Self::SideBySide,
            Self::SideBySide => Self::Blend,
            Self::Blend => Self::Wipe,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wipe => "Schieber",
            Self::SideBySide => "Nebeneinander",
            Self::Blend => "Überblenden",
        }
    }
}

pub struct Compare {
    pub player: Player,
    pub texture: Option<egui::TextureHandle>,
    pub uploaded: Option<Arc<Frame>>,
    pub mode: Mode,
    /// Position des Schiebers (0..=1): Wipe = Trennlinie, Blend = Deckkraft von B.
    pub split: f32,
    /// Zuletzt an B weitergegebener Ereigniszähler von A (`u64::MAX` = noch nichts übertragen).
    seen_events: u64,
}

impl Compare {
    pub fn new(player: Player) -> Self {
        Self {
            player,
            texture: None,
            uploaded: None,
            mode: Mode::Wipe,
            split: 0.5,
            seen_events: u64::MAX,
        }
    }

    /// Erzwingt eine neue Ausrichtung an A (z. B. nach Öffnen eines anderen Videos A).
    pub fn invalidate(&mut self) {
        self.seen_events = u64::MAX;
    }

    /// B an A ausrichten: bei jeder Aktion von A hart, dazwischen läuft B auf der Uhr von A.
    pub fn follow_master(&mut self, a: &Player) {
        if a.events != self.seen_events && self.player.mirror(a.position(), a.playing) {
            self.seen_events = a.events;
        }
        self.player.external_clock = a.playing.then(|| a.position());
    }
}

/// Größtes Rechteck mit dem Seitenverhältnis von `size`, zentriert in `area`.
pub fn fit(area: Rect, size: egui::Vec2) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

/// Teil von `rect` rechts der Linie `x` samt passendem UV-Ausschnitt (für den Schieber).
pub fn right_part(rect: Rect, x: f32) -> Option<(Rect, Rect)> {
    let x = x.clamp(rect.left(), rect.right());
    if rect.right() - x < 0.5 {
        return None;
    }
    let frac = (x - rect.left()) / rect.width();
    let part = Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);
    let uv = Rect::from_min_max(egui::pos2(frac, 0.0), egui::pos2(1.0, 1.0));
    Some((part, uv))
}

pub const FULL_UV: Rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

pub fn fade(alpha: f32) -> Color32 {
    Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect_and_centers() {
        let area = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 500.0));
        let r = fit(area, egui::vec2(1920.0, 1080.0));
        assert!((r.height() - 500.0).abs() < 1e-3);
        assert!((r.width() - 888.888).abs() < 0.01);
        assert!((r.center().x - 500.0).abs() < 1e-3);
    }

    #[test]
    fn right_part_maps_uv() {
        let rect = Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(200.0, 100.0));
        let (part, uv) = right_part(rect, 150.0).expect("sichtbar");
        assert_eq!(part.left(), 150.0);
        assert_eq!(part.right(), 300.0);
        assert!((uv.left() - 0.25).abs() < 1e-6);
        assert!(right_part(rect, 300.0).is_none());
        // Links außerhalb: ganzes Bild.
        let (all, uv) = right_part(rect, 0.0).expect("sichtbar");
        assert_eq!(all, rect);
        assert_eq!(uv.left(), 0.0);
    }

    #[test]
    fn mode_cycles() {
        assert_eq!(Mode::Wipe.next().next().next(), Mode::Wipe);
    }
}
