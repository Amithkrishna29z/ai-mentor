//! Visual language for the whole app: palette, egui style, and the small
//! building blocks (cards, pills, stat tiles, section headers) every view uses.
//!
//! Colours come from a validated reference palette. The chart surface is
//! `#1a1a19` dark / `#fcfcfb` light — the surfaces that palette was validated
//! against — so the heatmap ramp and status hues keep their contrast guarantees.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    /// Window / panel plane, behind the cards.
    pub plane: Color32,
    /// Card and popup surface.
    pub surface: Color32,
    /// Slightly raised surface for inputs and inner blocks.
    pub surface_alt: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub good: Color32,
    pub warning: Color32,
    pub serious: Color32,
    pub critical: Color32,
    pub grid: Color32,
}

pub const DARK: Theme = Theme {
    dark: true,
    plane: rgb(0x0d0d0d),
    surface: rgb(0x1a1a19),
    surface_alt: rgb(0x242422),
    border: Color32::from_rgba_premultiplied(38, 38, 36, 255),
    text: rgb(0xffffff),
    text_weak: rgb(0xc3c2b7),
    text_muted: rgb(0x898781),
    accent: rgb(0x3987e5),
    accent_dim: rgb(0x184f95),
    good: rgb(0x0ca30c),
    warning: rgb(0xfab219),
    serious: rgb(0xec835a),
    critical: rgb(0xd03b3b),
    grid: rgb(0x2c2c2a),
};

pub const LIGHT: Theme = Theme {
    dark: false,
    plane: rgb(0xf9f9f7),
    surface: rgb(0xfcfcfb),
    surface_alt: rgb(0xf0efec),
    border: rgb(0xe1e0d9),
    text: rgb(0x0b0b0b),
    text_weak: rgb(0x52514e),
    text_muted: rgb(0x898781),
    accent: rgb(0x2a78d6),
    accent_dim: rgb(0xcde2fb),
    good: rgb(0x0ca30c),
    warning: rgb(0xfab219),
    serious: rgb(0xec835a),
    critical: rgb(0xd03b3b),
    grid: rgb(0xe1e0d9),
};

pub fn theme_for(dark: bool) -> Theme {
    if dark {
        DARK
    } else {
        LIGHT
    }
}

/// The theme matching the context's current mode.
pub fn current(ui: &egui::Ui) -> Theme {
    theme_for(ui.visuals().dark_mode)
}

/// Install the palette, typography and spacing on the egui context. Both modes
/// are styled, so switching theme is just a preference flip.
pub fn apply(ctx: &egui::Context, dark: bool) {
    ctx.set_style_of(egui::Theme::Dark, build_style(ctx, true));
    ctx.set_style_of(egui::Theme::Light, build_style(ctx, false));
    ctx.set_theme(if dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
}

fn build_style(ctx: &egui::Context, dark: bool) -> egui::Style {
    let t = theme_for(dark);
    let base = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let mut style = (*base).clone();

    style.text_styles = [
        (TextStyle::Heading, FontId::new(21.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(9.0, 8.0);
    s.button_padding = egui::vec2(11.0, 6.0);
    s.window_margin = Margin::same(14);
    s.menu_margin = Margin::same(8);
    s.indent = 18.0;
    s.interact_size.y = 26.0;
    s.slider_width = 160.0;
    s.combo_width = 180.0;
    s.scroll.bar_width = 9.0;

    let v = &mut style.visuals;
    v.dark_mode = dark;
    v.panel_fill = t.plane;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.surface_alt;
    v.faint_bg_color = t.surface_alt;
    v.code_bg_color = t.surface_alt;
    v.override_text_color = Some(t.text);
    v.weak_text_color = Some(t.text_muted);
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.critical;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.striped = true;
    v.button_frame = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.slider_trailing_fill = true;
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 140 } else { 40 }),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 120 } else { 30 }),
    };
    v.selection.bg_fill = t.accent.gamma_multiply(if dark { 0.45 } else { 0.30 });
    v.selection.stroke = Stroke::new(1.0, t.accent);

    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;

    w.noninteractive.bg_fill = t.surface;
    w.noninteractive.weak_bg_fill = t.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_weak);
    w.noninteractive.corner_radius = radius;
    w.noninteractive.expansion = 0.0;

    w.inactive.bg_fill = t.surface_alt;
    w.inactive.weak_bg_fill = t.surface_alt;
    w.inactive.bg_stroke = Stroke::new(1.0, t.border);
    w.inactive.fg_stroke = Stroke::new(1.0, t.text_weak);
    w.inactive.corner_radius = radius;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = blend(t.surface_alt, t.accent, if dark { 0.22 } else { 0.14 });
    w.hovered.weak_bg_fill = blend(t.surface_alt, t.accent, if dark { 0.22 } else { 0.14 });
    w.hovered.bg_stroke = Stroke::new(1.0, t.accent.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.0, t.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 1.0;

    w.active.bg_fill = t.accent;
    w.active.weak_bg_fill = t.accent;
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    w.active.corner_radius = radius;
    w.active.expansion = 1.0;

    w.open.bg_fill = t.surface_alt;
    w.open.weak_bg_fill = t.surface_alt;
    w.open.bg_stroke = Stroke::new(1.0, t.accent.gamma_multiply(0.6));
    w.open.fg_stroke = Stroke::new(1.0, t.text);
    w.open.corner_radius = radius;

    style
}

fn blend(base: Color32, over: Color32, amount: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - amount) + b as f32 * amount).round() as u8;
    Color32::from_rgb(
        mix(base.r(), over.r()),
        mix(base.g(), over.g()),
        mix(base.b(), over.b()),
    )
}

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

/// A surface card: the default container for any block of content.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let t = current(ui);
    egui::Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .show(ui, add)
        .inner
}

/// A card that leads with a title row.
pub fn titled_card<R>(
    ui: &mut egui::Ui,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    card(ui, |ui| {
        section_label(ui, title);
        ui.add_space(6.0);
        add(ui)
    })
}

/// Small upper-case section label — the quiet heading inside a card.
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    let t = current(ui);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.5)
            .color(t.text_muted)
            .strong(),
    );
}

/// A filled pill badge. The label always carries the meaning; colour supports it.
pub fn pill(ui: &mut egui::Ui, text: impl Into<String>, fill: Color32) -> egui::Response {
    let text: String = text.into();
    let t = current(ui);
    let fg = if t.dark && fill == t.warning {
        Color32::from_rgb(20, 20, 18)
    } else {
        Color32::WHITE
    };
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::new(11.0, FontFamily::Proportional),
        fg,
    );
    let padding = egui::vec2(8.0, 3.0);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(7), fill);
    ui.painter().galley(rect.min + padding, galley, fg);
    response
}

/// Outlined pill, for quiet metadata chips like "builds on".
pub fn chip(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let t = current(ui);
    let text: String = text.into();
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::new(11.0, FontFamily::Proportional),
        t.text_weak,
    );
    let padding = egui::vec2(8.0, 3.0);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + padding * 2.0, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(7), t.surface_alt);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0, t.border),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(rect.min + padding, galley, t.text_weak);
    response
}

/// Width of one dashboard stat tile, including its frame.
pub const TILE_SIZE: egui::Vec2 = egui::vec2(150.0, 66.0);

/// Label + big value, the dashboard stat tile. Fixed size, so a row of them
/// wraps predictably instead of overflowing the panel.
pub fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str, accent: Option<Color32>) {
    let t = current(ui);
    ui.allocate_ui(TILE_SIZE, |ui| {
        egui::Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(11))
            .show(ui, |ui| {
                ui.set_min_size(TILE_SIZE - egui::vec2(22.0, 22.0));
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label.to_uppercase())
                            .size(10.0)
                            .color(t.text_muted)
                            .strong(),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(value)
                            .size(21.0)
                            .color(accent.unwrap_or(t.text))
                            .strong(),
                    );
                });
            });
    });
}

/// A slim progress bar with its own label above it.
pub fn meter(ui: &mut egui::Ui, label: &str, fraction: f32, caption: &str) {
    let t = current(ui);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(t.text_weak).size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(caption).color(t.text).size(12.0).strong());
        });
    });
    ui.add_space(3.0);
    ui.add(
        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
            .desired_height(7.0)
            .corner_radius(CornerRadius::same(4))
            .fill(t.accent),
    );
}

/// Difficulty 1–5 is ordered magnitude, so it gets a one-hue ordinal ramp
/// (never the reserved status colours). The digit is always shown beside it.
pub fn difficulty_color(difficulty: i64, dark: bool) -> Color32 {
    if dark {
        match difficulty {
            1 => rgb(0x184f95),
            2 => rgb(0x256abf),
            3 => rgb(0x2a78d6),
            4 => rgb(0x3987e5),
            _ => rgb(0x6da7ec),
        }
    } else {
        match difficulty {
            1 => rgb(0x9ec5f4),
            2 => rgb(0x6da7ec),
            3 => rgb(0x3987e5),
            4 => rgb(0x2a78d6),
            _ => rgb(0x184f95),
        }
    }
}

/// Heatmap cell colour for `step` 0..=4, 0 meaning "no activity".
pub fn heat_color(step: u8, t: &Theme) -> Color32 {
    if t.dark {
        match step {
            0 => rgb(0x242422),
            1 => rgb(0x184f95),
            2 => rgb(0x256abf),
            3 => rgb(0x3987e5),
            _ => rgb(0x6da7ec),
        }
    } else {
        match step {
            0 => rgb(0xe8e7e1),
            1 => rgb(0x9ec5f4),
            2 => rgb(0x5598e7),
            3 => rgb(0x2a78d6),
            _ => rgb(0x184f95),
        }
    }
}
