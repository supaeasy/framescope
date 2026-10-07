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

/// `hh:mm:ss.mmm` bzw. `mm:ss.mmm` für Zeiten unter einer Stunde.
pub fn fmt_time(secs: f64) -> String {
    let ms_total = (secs.max(0.0) * 1000.0).round() as u64;
    let (ms, s_total) = (ms_total % 1000, ms_total / 1000);
    let (s, m_total) = (s_total % 60, s_total / 60);
    let (m, h) = (m_total % 60, m_total / 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m:02}:{s:02}.{ms:03}")
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Play,
    Pause,
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
