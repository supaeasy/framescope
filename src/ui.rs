//! Design-System „Nocturne“: Farb-Tokens, Schriften (Inter, Phosphor-Icons) und kleine Widgets.
//!
//! Die Werte stammen aus `docs/mockups/_ds/.../styles.css`. Der Akzent wird nur als Linie, Marke
//! oder Glühen eingesetzt (Abspielkopf, Loop-Bereich, KEY), nie als Fläche.

use eframe::egui::{
    self, epaint::Mesh, Color32, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect,
    Response, Sense, Stroke, Vec2,
};
use std::sync::Arc;

// --- Farben (Nocturne) ---------------------------------------------------------------------
pub const BG: Color32 = Color32::from_rgb(0x16, 0x18, 0x26);
pub const SURFACE: Color32 = Color32::from_rgb(0x23, 0x25, 0x32);
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xe9, 0xed);

pub const N200: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xf5);
pub const N300: Color32 = Color32::from_rgb(0xcf, 0xd3, 0xe5);
pub const N400: Color32 = Color32::from_rgb(0xb2, 0xb6, 0xca);
pub const N700: Color32 = Color32::from_rgb(0x59, 0x5d, 0x6c);
pub const N800: Color32 = Color32::from_rgb(0x3f, 0x42, 0x4d);

pub const ACCENT: Color32 = Color32::from_rgb(0x91, 0x84, 0xd9);
pub const A100: Color32 = Color32::from_rgb(0xf5, 0xf4, 0xff);
pub const A300: Color32 = Color32::from_rgb(0xd2, 0xce, 0xfd);
pub const A800: Color32 = Color32::from_rgb(0x42, 0x3a, 0x6a);
pub const A900: Color32 = Color32::from_rgb(0x2b, 0x27, 0x41);

/// Warnfarbe für Fehlermeldungen (kein Nocturne-Token; bewusst dezent).
pub const ERROR: Color32 = Color32::from_rgb(0xe8, 0x8a, 0x84);

const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Medium-Tabular.otf");
const FILL_FAMILY: &str = "phosphor-fill";

/// `color` mit Deckkraft `a` (0..=1).
pub fn alpha(color: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// Icon aus Phosphor (Linie oder gefüllt).
#[derive(Clone, Copy)]
pub enum Glyph {
    Regular(&'static str),
    Fill(&'static str),
}

impl Glyph {
    fn font(self, size: f32) -> FontId {
        match self {
            Self::Regular(_) => FontId::new(size, FontFamily::Proportional),
            Self::Fill(_) => FontId::new(size, FontFamily::Name(FILL_FAMILY.into())),
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Regular(s) | Self::Fill(s) => s,
        }
    }
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".into(), Arc::new(FontData::from_static(INTER)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, "inter".into());
        }
    }
    // Phosphor steht an zweiter Stelle der Proportional-Familie (Fallback für Icon-Codepoints).
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    egui_phosphor::add_font_bytes_as_family(
        &mut fonts,
        FILL_FAMILY,
        egui_phosphor::Variant::Fill.font_bytes(),
    );
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = A300;
    v.selection.bg_fill = alpha(ACCENT, 0.35);
    v.selection.stroke = Stroke::new(1.0, A300);
    v.window_stroke = Stroke::new(1.0, N700);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, N800);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

/// Vertikaler Verlauf: oben transparent, unten `color` mit Deckkraft `bottom_alpha`.
pub fn fade_up(painter: &egui::Painter, rect: Rect, color: Color32, bottom_alpha: f32) {
    let (clear, solid) = (Color32::TRANSPARENT, alpha(color, bottom_alpha));
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), clear);
    mesh.colored_vertex(rect.right_top(), clear);
    mesh.colored_vertex(rect.right_bottom(), solid);
    mesh.colored_vertex(rect.left_bottom(), solid);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

fn hover_fill(resp: &Response, active: bool) -> Option<Color32> {
    if resp.is_pointer_button_down_on() {
        Some(alpha(ACCENT, 0.18))
    } else if resp.hovered() {
        Some(alpha(ACCENT, 0.10))
    } else if active {
        Some(alpha(ACCENT, 0.12))
    } else {
        None
    }
}

/// Quadratischer Ghost-Icon-Button (36 px, Icon 18 px) wie `.btn-ghost.btn-icon`.
pub fn ghost_icon(
    ui: &mut egui::Ui,
    glyph: Glyph,
    color: Color32,
    active: bool,
    tip: &str,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    if let Some(fill) = hover_fill(&resp, active) {
        ui.painter().rect_filled(rect, 8.0, fill);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph.text(),
        glyph.font(18.0),
        color,
    );
    resp.on_hover_text(tip)
}

/// Ghost-Button mit Icon und Beschriftung (`.btn-ghost`) auf halbtransparentem Grund,
/// damit er über dem Video lesbar bleibt.
pub fn ghost_button(
    ui: &mut egui::Ui,
    glyph: Option<Glyph>,
    label: &str,
    active: bool,
    tip: &str,
) -> Response {
    let color = if active { A300 } else { ACCENT };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::proportional(14.0), color);
    let icon_only = label.is_empty();
    let icon_w = if glyph.is_some() && !icon_only {
        16.0 + 6.0
    } else {
        0.0
    };
    let size = if icon_only {
        Vec2::splat(32.0)
    } else {
        Vec2::new(galley.size().x + icon_w + 20.0, 32.0)
    };
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_filled(rect, 8.0, alpha(BG, 0.72));
    if let Some(fill) = hover_fill(&resp, active) {
        ui.painter().rect_filled(rect, 8.0, fill);
    }
    let mut x = rect.left() + 10.0;
    if let Some(g) = glyph {
        let cx = if icon_only { rect.center().x } else { x + 8.0 };
        ui.painter().text(
            Pos2::new(cx, rect.center().y),
            egui::Align2::CENTER_CENTER,
            g.text(),
            g.font(16.0),
            color,
        );
        x += icon_w;
    }
    ui.painter().galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    resp.on_hover_text(tip)
}

/// Kleines Label wie `.tag-accent` (z. B. KEY).
pub fn tag(ui: &mut egui::Ui, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(11.0), A100);
    let size = galley.size() + Vec2::new(20.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 6.0, A800);
    ui.painter()
        .galley(rect.min + Vec2::new(10.0, 3.0), galley, A100);
}

/// Lautstärke-Balken (64 × 18 px): 3 px Linie, Füllung bis zum Pegel.
/// Rückgabe: `true`, wenn der Wert geändert wurde.
pub fn volume_bar(ui: &mut egui::Ui, volume: &mut f32, muted: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(64.0, 18.0), Sense::click_and_drag());
    let mut changed = false;
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            *volume = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            changed = true;
        }
    }
    let line = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 3.0));
    ui.painter().rect_filled(line, 2.0, N800);
    let level = if muted { 0.0 } else { *volume };
    let filled = Rect::from_min_max(
        line.min,
        Pos2::new(line.left() + line.width() * level, line.max.y),
    );
    ui.painter().rect_filled(filled, 2.0, N300);
    if resp.hovered() || resp.dragged() {
        ui.painter()
            .circle_filled(Pos2::new(filled.right(), line.center().y), 5.0, TEXT);
    }
    changed
}
