//! Safe-Area-Ebenen (Title-/Action-Safe, YouTube-Vorgaben), die über das Bild gelegt werden.
//!
//! Die YouTube-Flächen stammen aus den offiziellen „Safe Zone Overlays“ (weiß = sichere Fläche)
//! und sind hier als Anteile der Bildgröße hinterlegt (1920×1080, 1080×1080, 1080×1920).

use crate::ui;
use eframe::egui::{self, pos2, Color32, FontId, Painter, Pos2, Rect, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// EBU R 95 / SRG: Action-Safe, 93 % der Bildbreite und -höhe.
    TvAction,
    /// EBU R 95 / SRG: Title-Safe, 90 % der Bildbreite und -höhe.
    TvTitle,
    YtHorizontal,
    YtSquare,
    YtVertical,
}

pub const ZONES: [Zone; 5] = [
    Zone::TvAction,
    Zone::TvTitle,
    Zone::YtHorizontal,
    Zone::YtSquare,
    Zone::YtVertical,
];

impl Zone {
    pub fn bit(self) -> u8 {
        1 << ZONES.iter().position(|z| *z == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            Zone::TvAction => "TV Action-Safe 93 % (EBU R 95)",
            Zone::TvTitle => "TV Title-Safe 90 % (EBU R 95)",
            Zone::YtHorizontal => "YouTube Horizontal 16:9",
            Zone::YtSquare => "YouTube Quadrat 1:1",
            Zone::YtVertical => "YouTube Vertikal 9:16",
        }
    }

    /// Kurzname für die Beschriftung im Bild.
    fn tag(self) -> &'static str {
        match self {
            Zone::TvAction => "Action-Safe 93 %",
            Zone::TvTitle => "Title-Safe 90 %",
            Zone::YtHorizontal => "YouTube 16:9",
            Zone::YtSquare => "YouTube 1:1",
            Zone::YtVertical => "YouTube 9:16",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Zone::TvAction => Color32::from_rgb(0xf2, 0xc9, 0x4c),
            Zone::TvTitle => Color32::from_rgb(0x56, 0xcc, 0xf2),
            Zone::YtHorizontal => Color32::from_rgb(0xeb, 0x57, 0x57),
            Zone::YtSquare => Color32::from_rgb(0x6f, 0xcf, 0x97),
            Zone::YtVertical => Color32::from_rgb(0xbb, 0x6b, 0xd9),
        }
    }

    /// Umriss der sicheren Fläche in Anteilen der Bildgröße (im Uhrzeigersinn).
    pub fn outline(self) -> Vec<(f32, f32)> {
        let px = |w: f32, h: f32, pts: &[(f32, f32)]| -> Vec<(f32, f32)> {
            pts.iter().map(|(x, y)| (x / w, y / h)).collect()
        };
        match self {
            Zone::TvAction => inset(0.035),
            Zone::TvTitle => inset(0.05),
            Zone::YtHorizontal => px(
                1920.0,
                1080.0,
                &[
                    (38.0, 183.0),
                    (496.0, 183.0),
                    (496.0, 38.0),
                    (1444.0, 38.0),
                    (1444.0, 133.0),
                    (1758.0, 133.0),
                    (1758.0, 693.0),
                    (38.0, 693.0),
                ],
            ),
            Zone::YtSquare => px(
                1080.0,
                1080.0,
                &[
                    (48.0, 48.0),
                    (534.0, 48.0),
                    (534.0, 105.0),
                    (979.0, 105.0),
                    (979.0, 690.0),
                    (48.0, 690.0),
                ],
            ),
            Zone::YtVertical => px(
                1080.0,
                1920.0,
                &[
                    (48.0, 288.0),
                    (888.0, 288.0),
                    (888.0, 1248.0),
                    (48.0, 1248.0),
                ],
            ),
        }
    }
}

fn inset(m: f32) -> Vec<(f32, f32)> {
    vec![(m, m), (1.0 - m, m), (1.0 - m, 1.0 - m), (m, 1.0 - m)]
}

/// Zeichnet alle in `mask` gewählten Ebenen auf das Bild `rect`.
pub fn draw(painter: &Painter, rect: Rect, mask: u8) {
    for (slot, zone) in ZONES
        .into_iter()
        .filter(|z| mask & z.bit() != 0)
        .enumerate()
    {
        let pts: Vec<Pos2> = zone
            .outline()
            .into_iter()
            .map(|(x, y)| {
                pos2(
                    rect.left() + x * rect.width(),
                    rect.top() + y * rect.height(),
                )
            })
            .collect();
        let mut closed = pts.clone();
        closed.push(pts[0]);
        // Dunkle Unterlage, damit die Linie auf weißem wie auf schwarzem Bild sichtbar bleibt.
        painter.add(egui::Shape::line(
            closed.clone(),
            Stroke::new(3.5, Color32::from_black_alpha(170)),
        ));
        painter.add(egui::Shape::line(closed, Stroke::new(1.5, zone.color())));
        let anchor = pts
            .iter()
            .copied()
            .min_by(|a, b| {
                (a.y, a.x)
                    .partial_cmp(&(b.y, b.x))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(pts[0]);
        let galley = painter.layout_no_wrap(
            zone.tag().to_owned(),
            FontId::proportional(11.0),
            zone.color(),
        );
        let tag = Rect::from_min_size(
            anchor + egui::vec2(4.0, 4.0 + slot as f32 * 20.0),
            galley.size() + egui::vec2(10.0, 4.0),
        );
        painter.rect_filled(tag, 4.0, ui::alpha(Color32::BLACK, 0.7));
        painter.galley(tag.min + egui::vec2(5.0, 2.0), galley, zone.color());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_unique() {
        let all = ZONES.iter().fold(0u8, |m, z| m | z.bit());
        assert_eq!(all.count_ones() as usize, ZONES.len());
    }

    #[test]
    fn outlines_are_inside_the_frame() {
        for z in ZONES {
            assert!(z.outline().len() >= 4);
            for (x, y) in z.outline() {
                assert!(
                    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                    "{z:?}"
                );
            }
        }
    }
}
