//! Theme und kleine UI-Helfer (Icons werden gezeichnet, damit sie nicht von Font-Glyphen abhängen).

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};

pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0c, 0x0e);
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(0x14, 0x15, 0x19, 0xe6);
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
pub const TEXT: Color32 = Color32::from_rgb(0xdd, 0xe0, 0xe6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x9a);
pub const KEY: Color32 = Color32::from_rgb(0xff, 0xb4, 0x2e);

pub fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = Color32::from_rgb(0x08, 0x09, 0x0a);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(40));
    v.widgets.hovered.weak_bg_fill = Color32::from_white_alpha(28);
    v.widgets.active.weak_bg_fill = Color32::from_white_alpha(48);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(10.0, 6.0);
    });
}

#[derive(Clone, Copy)]
pub enum Icon {
    Play,
    Pause,
    Speaker,
    SpeakerMuted,
}

/// Runder, dezenter Icon-Button.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.painter()
            .circle_filled(rect.center(), size / 2.0, Color32::from_white_alpha(30));
    }
    let c = rect.center();
    let r = size * 0.22;
    match icon {
        Icon::Play => {
            let pts = vec![
                Pos2::new(c.x - r * 0.7, c.y - r * 1.1),
                Pos2::new(c.x - r * 0.7, c.y + r * 1.1),
                Pos2::new(c.x + r * 1.1, c.y),
            ];
            ui.painter()
                .add(Shape::convex_polygon(pts, TEXT, Stroke::NONE));
        }
        Icon::Speaker | Icon::SpeakerMuted => {
            let r = size * 0.2;
            let body =
                Rect::from_center_size(Pos2::new(c.x - r * 1.1, c.y), Vec2::new(r * 0.9, r * 1.2));
            ui.painter().rect_filled(body, 1.0, TEXT);
            let cone = vec![
                Pos2::new(c.x - r * 0.7, c.y - r * 0.6),
                Pos2::new(c.x + r * 0.3, c.y - r * 1.4),
                Pos2::new(c.x + r * 0.3, c.y + r * 1.4),
                Pos2::new(c.x - r * 0.7, c.y + r * 0.6),
            ];
            ui.painter()
                .add(Shape::convex_polygon(cone, TEXT, Stroke::NONE));
            let stroke = Stroke::new(
                1.6,
                if matches!(icon, Icon::Speaker) {
                    TEXT
                } else {
                    TEXT_DIM
                },
            );
            if matches!(icon, Icon::Speaker) {
                for k in [1.0f32, 1.9] {
                    let arc: Vec<Pos2> = (-4..=4)
                        .map(|i| {
                            let a = i as f32 * 0.16;
                            Pos2::new(
                                c.x + r * 0.6 + r * k * a.cos() * 0.8,
                                c.y + r * k * a.sin() * 0.8,
                            )
                        })
                        .collect();
                    ui.painter().add(Shape::line(arc, stroke));
                }
            } else {
                let o = Pos2::new(c.x + r * 1.6, c.y);
                let d = r * 0.8;
                ui.painter()
                    .line_segment([o + Vec2::new(-d, -d), o + Vec2::new(d, d)], stroke);
                ui.painter()
                    .line_segment([o + Vec2::new(-d, d), o + Vec2::new(d, -d)], stroke);
            }
        }
        Icon::Pause => {
            for dx in [-r * 0.6, r * 0.6] {
                let bar =
                    Rect::from_center_size(Pos2::new(c.x + dx, c.y), Vec2::new(r * 0.65, r * 2.1));
                ui.painter().rect_filled(bar, 1.0, TEXT);
            }
        }
    }
    resp
}
