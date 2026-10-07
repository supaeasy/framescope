//! Timeline-Widget: Track, Position, Scrubbing per Maus.

use crate::ui::ACCENT;
use eframe::egui::{self, Color32, Rect, Sense, Vec2};

/// Zeichnet die Timeline. `pos` und Rückgabewert sind Anteile (0..=1);
/// ein Rückgabewert bedeutet: der Nutzer scrubbt gerade an diese Stelle.
pub fn show(ui: &mut egui::Ui, width: f32, pos: f32) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
    let p = ui.painter();
    p.rect_filled(track, 2.0, Color32::from_white_alpha(36));

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
