//! A GNOME (libadwaita) look for egui: the Adwaita light and dark palettes,
//! the system UI font at real weights, rounded corners and roomy spacing.
//! Follows the system light/dark setting.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Vec2};
use std::sync::Arc;

/// Colours the custom widgets need beyond egui's own visuals.
#[derive(Clone, Copy)]
pub struct Palette {
    pub window: Color32,
    pub view: Color32,
    pub header: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub fg: Color32,
    pub dim: Color32,
    pub border: Color32,
    /// Fill of a normal button at rest, hovered and pressed.
    pub button: [Color32; 3],
    pub accent_bg: Color32,
    pub accent_fg: Color32,
    /// Accent used for text and thin strokes (readable on the window).
    pub accent: Color32,
    pub destructive_bg: Color32,
    pub destructive: Color32,
    pub toast: Color32,
    pub banner: Color32,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

pub const LIGHT: Palette = Palette {
    window: Color32::from_rgb(0xfa, 0xfa, 0xfb),
    view: Color32::from_rgb(0xff, 0xff, 0xff),
    header: Color32::from_rgb(0xff, 0xff, 0xff),
    sidebar: Color32::from_rgb(0xeb, 0xeb, 0xed),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    fg: Color32::from_rgb(0x2e, 0x2e, 0x33),
    dim: Color32::from_rgb(0x78, 0x78, 0x7d),
    border: rgba(0, 0, 6, 26),
    button: [rgba(0, 0, 6, 20), rgba(0, 0, 6, 31), rgba(0, 0, 6, 61)],
    accent_bg: Color32::from_rgb(0x35, 0x84, 0xe4),
    accent_fg: Color32::WHITE,
    accent: Color32::from_rgb(0x04, 0x61, 0xbe),
    destructive_bg: Color32::from_rgb(0xe0, 0x1b, 0x24),
    destructive: Color32::from_rgb(0xc3, 0x00, 0x00),
    toast: Color32::from_rgb(0x30, 0x30, 0x34),
    banner: rgba(0x35, 0x84, 0xe4, 38),
};

pub const DARK: Palette = Palette {
    window: Color32::from_rgb(0x22, 0x22, 0x26),
    view: Color32::from_rgb(0x1d, 0x1d, 0x20),
    header: Color32::from_rgb(0x2e, 0x2e, 0x32),
    sidebar: Color32::from_rgb(0x2e, 0x2e, 0x32),
    card: Color32::from_rgb(0x36, 0x36, 0x3a),
    fg: Color32::from_rgb(0xff, 0xff, 0xff),
    dim: Color32::from_rgb(0xa8, 0xa8, 0xad),
    border: rgba(255, 255, 255, 26),
    button: [rgba(255, 255, 255, 26), rgba(255, 255, 255, 38), rgba(255, 255, 255, 77)],
    accent_bg: Color32::from_rgb(0x35, 0x84, 0xe4),
    accent_fg: Color32::WHITE,
    accent: Color32::from_rgb(0x81, 0xd0, 0xff),
    destructive_bg: Color32::from_rgb(0xc0, 0x1c, 0x28),
    destructive: Color32::from_rgb(0xff, 0x93, 0x8c),
    toast: Color32::from_rgb(0x46, 0x46, 0x4b),
    banner: rgba(0x35, 0x84, 0xe4, 56),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == Theme::Dark { &DARK } else { &LIGHT }
}

/// Rounded corners used throughout, matching libadwaita.
pub const BUTTON_RADIUS: u8 = 8;
pub const CARD_RADIUS: u8 = 12;
pub const WINDOW_RADIUS: u8 = 12;
pub const DIALOG_RADIUS: u8 = 18;

/// Families for the heavier weights of the UI font.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::System);
    for (theme, p) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.set_style_of(theme, style(theme, p));
    }
}

fn style(theme: Theme, p: &Palette) -> egui::Style {
    let mut s = egui::Style { visuals: theme.default_visuals(), ..Default::default() };
    s.text_styles = [
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(15.0, bold())),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = Vec2::new(8.0, 8.0);
    sp.button_padding = Vec2::new(12.0, 7.0);
    sp.interact_size = Vec2::new(34.0, 34.0);
    sp.menu_margin = Margin::same(6);
    sp.window_margin = Margin::same(18);
    sp.slider_width = 150.0;
    sp.slider_rail_height = 4.0;
    sp.combo_height = 300.0;
    sp.icon_width = 18.0;
    sp.icon_width_inner = 10.0;

    let v = &mut s.visuals;
    v.override_text_color = Some(p.fg);
    v.weak_text_color = Some(p.dim);
    v.panel_fill = p.window;
    v.window_fill = if theme == Theme::Dark { p.card } else { p.view };
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(DIALOG_RADIUS);
    v.menu_corner_radius = CornerRadius::same(CARD_RADIUS);
    let shadow = |a| Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(a) };
    v.window_shadow = shadow(60);
    v.popup_shadow = shadow(40);
    v.extreme_bg_color = p.button[0];
    v.text_edit_bg_color = Some(p.button[0]);
    v.faint_bg_color = p.button[0];
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_bg.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(2.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.button_frame = true;

    let radius = CornerRadius::same(BUTTON_RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.fg);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [(&mut w.inactive, p.button[0]), (&mut w.hovered, p.button[1]), (&mut w.active, p.button[2]), (&mut w.open, p.button[1])] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.5, p.fg);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    // Slider rails and handles use the accent, like GtkScale.
    w.inactive.bg_fill = p.button[1];
    s
}

/// Uses the desktop's UI font when one is found, registered at regular,
/// semibold and bold weights; egui's built-in fonts stay as fallback.
fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut regular = Vec::new();
    let mut semi = Vec::new();
    let mut heavy = Vec::new();

    if let Some((bytes, bold_bytes)) = find_ui_font() {
        let bytes: Arc<[u8]> = bytes.into();
        let variable = bold_bytes.is_none();
        for (name, weight, list) in [("ui", 400.0, &mut regular), ("ui-semibold", 600.0, &mut semi), ("ui-bold", 700.0, &mut heavy)] {
            let data = match (&bold_bytes, weight > 400.0) {
                (Some(b), true) => FontData::from_owned(b.clone()),
                _ => FontData::from_owned(bytes.to_vec()),
            };
            let data = if variable {
                data.tweak(egui::FontTweak { coords: egui::epaint::text::VariationCoords::new([("wght", weight)]), ..Default::default() })
            } else {
                data
            };
            defs.font_data.insert(name.into(), Arc::new(data));
            list.push(name.to_owned());
        }
    }
    let with_fallback = |mut v: Vec<String>| {
        v.extend(fallback.iter().cloned());
        v
    };
    defs.families.insert(FontFamily::Proportional, with_fallback(regular));
    defs.families.insert(semibold(), with_fallback(semi));
    defs.families.insert(bold(), with_fallback(heavy));
    defs
}

/// The UI font and, for fonts without a weight axis, a separate bold file.
fn find_ui_font() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    // Variable fonts first: one file gives every weight.
    const VARIABLE: &[&str] = &[
        "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/truetype/adwaita-sans/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/opentype/cantarell/Cantarell-VF.otf",
        "/usr/share/fonts/cantarell/Cantarell-VF.otf",
        "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu[wdth,wght].ttf",
        "/System/Library/Fonts/SFNS.ttf",
    ];
    const PAIRS: &[(&str, &str)] = &[
        ("C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\segoeuib.ttf"),
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
    ];
    for path in VARIABLE {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((bytes, None));
        }
    }
    PAIRS.iter().find_map(|(regular, bold)| Some((std::fs::read(regular).ok()?, Some(std::fs::read(bold).ok()?))))
}
