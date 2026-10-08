//! Timeline-Widget im Nocturne-Stil: dünne Linie, Keyframe-Marken, Loop-Band, Abspielkopf mit Glühen.

use crate::ui::{alpha, A900, ACCENT, N400, N700, N800, TEXT};
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
    // Sichtbar sind 18 px; zum Greifen ist die Fläche etwas höher.
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    let top = rect.center().y - 9.0;
    let track = Rect::from_min_size(
        egui::pos2(rect.left(), top + 8.0),
        Vec2::new(rect.width(), 3.0),
    );
    let p = ui.painter();
    p.rect_filled(track, 2.0, N800);

    if let Some(b) = band {
        let x0 = track.left() + track.width() * b.start.clamp(0.0, 1.0);
        let x1 = (track.left() + track.width() * b.end.clamp(0.0, 1.0)).max(x0 + 2.0);
        let (fill, edge) = if b.active {
            (A900, ACCENT)
        } else {
            (N800, N700)
        };
        let area = Rect::from_min_max(egui::pos2(x0, top + 4.0), egui::pos2(x1, top + 15.0));
        p.rect_filled(area, 2.0, fill);
        for x in [x0, x1 - 1.0] {
            p.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x, area.top()),
                    egui::pos2(x + 1.0, area.bottom()),
                ),
                0.0,
                edge,
            );
        }
    }

    let x = track.left() + track.width() * pos.clamp(0.0, 1.0);
    p.rect_filled(
        Rect::from_min_max(track.min, egui::pos2(x, track.max.y)),
        2.0,
        ACCENT,
    );

    // Keyframe-Marken: 1 × 5 px, höchstens eine pro Pixelspalte.
    let mut last_x = f32::NEG_INFINITY;
    for &f in key_fracs {
        let kx = (track.left() + track.width() * f).round();
        if kx - last_x >= 1.0 {
            p.rect_filled(
                Rect::from_min_max(egui::pos2(kx, top), egui::pos2(kx + 1.0, top + 5.0)),
                0.0,
                N400,
            );
            last_x = kx;
        }
    }

    // Abspielkopf: 12 px, mit 3 px Glühen in Akzentfarbe (40 %).
    let head = egui::pos2(x, top + 9.0);
    let hot = resp.hovered() || resp.dragged();
    p.circle_filled(head, if hot { 9.5 } else { 9.0 }, alpha(ACCENT, 0.40));
    p.circle_filled(head, 6.0, if hot { Color32::WHITE } else { TEXT });

    if resp.dragged() || resp.clicked() {
        let pointer = resp.interact_pointer_pos()?;
        return Some(((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0));
    }
    None
}
