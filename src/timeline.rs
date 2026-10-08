//! Timeline-Widget: Track, Keyframe-Marker, Position, Scrubbing per Maus.

use crate::ui::{ACCENT, KEY};
use eframe::egui::{self, Color32, Rect, Sense, Vec2};

/// Loop-Bereich auf der Timeline (Anteile 0..=1).
pub struct LoopBand {
    pub start: f32,
    pub end: f32,
    pub active: bool,
}

/// Zeichnet die Timeline. `pos` und `key_fracs` sind Anteile (0..=1); ein
/// Rückgabewert bedeutet: der Nutzer scrubbt gerade an diese Stelle.
pub fn show(
    ui: &mut egui::Ui,
    width: f32,
    pos: f32,
    key_fracs: &[f32],
    band: Option<LoopBand>,
) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
    let p = ui.painter();
    p.rect_filled(track, 2.0, Color32::from_white_alpha(36));

    if let Some(b) = band {
        let (x0, x1) = (
            track.left() + track.width() * b.start.clamp(0.0, 1.0),
            track.left() + track.width() * b.end.clamp(0.0, 1.0),
        );
        let color = if b.active {
            ACCENT
        } else {
            Color32::from_gray(150)
        };
        let area = Rect::from_min_max(
            egui::pos2(x0, rect.top() + 3.0),
            egui::pos2(x1.max(x0 + 2.0), rect.bottom() - 3.0),
        );
        p.rect_filled(area, 3.0, color.gamma_multiply(0.22));
        for x in [x0, x1] {
            let bar = Rect::from_min_max(
                egui::pos2(x - 1.0, area.top()),
                egui::pos2(x + 1.0, area.bottom()),
            );
            p.rect_filled(bar, 1.0, color);
        }
    }

    // Keyframe-Marker: höchstens einer pro Pixelspalte.
    let mut last_x = f32::NEG_INFINITY;
    for &f in key_fracs {
        let x = (track.left() + track.width() * f).round();
        if x - last_x >= 1.0 {
            let tick = Rect::from_min_max(
                egui::pos2(x, track.top() - 5.0),
                egui::pos2(x + 1.0, track.top() - 1.0),
            );
            p.rect_filled(tick, 0.0, KEY.gamma_multiply(0.85));
            last_x = x;
        }
    }

    let x = track.left() + track.width() * pos.clamp(0.0, 1.0);
    let filled = Rect::from_min_max(track.min, egui::pos2(x, track.max.y));
    p.rect_filled(filled, 2.0, ACCENT);
    let hot = resp.hovered() || resp.dragged();
    p.circle_filled(
        egui::pos2(x, track.center().y),
        if hot { 7.0 } else { 5.0 },
        Color32::WHITE,
    );

    if resp.dragged() || resp.clicked() {
        let pointer = resp.interact_pointer_pos()?;
        return Some(((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0));
    }
    None
}
