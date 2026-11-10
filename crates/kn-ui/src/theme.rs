//! A GNOME (libadwaita) look for egui: the Adwaita light and dark palettes,
//! the system UI font at real weights, rounded corners and roomy spacing.
//! Follows the system light/dark setting.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme, Vec2};
use std::sync::Arc;

/// Colours the custom widgets need beyond egui's own visuals.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Behind panels and sidebars' content.
    pub window: Color32,
    /// Behind the stage and content views.
    pub view: Color32,
    /// The header bar.
    pub header: Color32,
    /// The slide sidebar and timeline.
    pub sidebar: Color32,
    /// Boxed lists and cards.
    pub card: Color32,
    /// Normal text and icons.
    pub fg: Color32,
    /// Secondary text.
    pub dim: Color32,
    /// Thin outlines and separators.
    pub border: Color32,
    /// Fill of a normal button at rest, hovered and pressed.
    pub button: [Color32; 3],
    /// Fill of suggested buttons, switches and slider rails.
    pub accent_bg: Color32,
    /// Text on `accent_bg`.
    pub accent_fg: Color32,
    /// Accent used for text and thin strokes (readable on the window).
    pub accent: Color32,
    /// Fill of destructive buttons.
    pub destructive_bg: Color32,
    /// Destructive text (errors) on the window.
    pub destructive: Color32,
    /// Toast background.
    pub toast: Color32,
    /// Banner background (opaque).
    pub banner: Color32,
}

/// A colour from straight (not premultiplied) RGBA, usable in consts.
const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// Adwaita's light palette.
pub const LIGHT: Palette = Palette {
    window: Color32::from_rgb(0xfa, 0xfa, 0xfb),
    view: Color32::from_rgb(0xf5, 0xf5, 0xf7),
    header: Color32::from_rgb(0xff, 0xff, 0xff),
    sidebar: Color32::from_rgb(0xf5, 0xf5, 0xf7),
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
    // Opaque: the window is transparent around its rounded corners, so a
    // see-through fill would show the desktop behind.
    banner: Color32::from_rgb(0xdd, 0xe8, 0xf8),
};

/// Adwaita's dark palette.
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
    banner: Color32::from_rgb(0x26, 0x38, 0x50),
};

/// The palette for the theme egui is showing.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    if ctx.theme() == Theme::Dark { &DARK } else { &LIGHT }
}

/// Rounded corners used throughout, matching libadwaita.
pub const BUTTON_RADIUS: u8 = 8;
/// Cards and boxed lists.
pub const CARD_RADIUS: u8 = 12;
/// The window's outer corners.
pub const WINDOW_RADIUS: u8 = 12;
/// Dialogs.
pub const DIALOG_RADIUS: u8 = 18;

/// Families for the heavier weights of the UI font.
pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

/// The UI font at weight 600.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Installs the fonts and both light and dark styles, following the
/// system's light/dark setting.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::System);
    for (theme, p) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        ctx.set_style_of(theme, style(theme, p));
    }
}

/// egui's style for one theme: text sizes, spacing, corners and colours.
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
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border.gamma_multiply(0.75));
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
    // Fonts for scripts the UI font lacks (Hebrew, Arabic, CJK, …), tried
    // after it and before egui's own.
    let mut scripts = Vec::new();
    for (i, bytes) in script_fonts().into_iter().enumerate() {
        let name = format!("script-{i}");
        defs.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        scripts.push(name);
    }
    let with_fallback = |mut v: Vec<String>| {
        v.extend(scripts.iter().cloned());
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
    PAIRS
        .iter()
        .find_map(|(regular, bold)| Some((std::fs::read(regular).ok()?, Some(std::fs::read(bold).ok()?))))
        .or_else(fontconfig_ui_font)
}

/// Languages whose letters the UI font may not have, so text typed in them
/// shows real glyphs instead of empty boxes.
#[cfg(all(unix, not(target_os = "macos")))]
const SCRIPT_LANGS: &[&str] = &["he", "ar", "el", "ru", "hi", "th", "ja", "ko", "zh-cn"];

/// Fallback fonts covering additional scripts, each file once.
fn script_fonts() -> Vec<Vec<u8>> {
    let mut paths: Vec<String> = fontconfig_script_fonts();
    // Well-known files on systems without fontconfig.
    const KNOWN: &[&str] = &[
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "/System/Library/Fonts/ArialHB.ttc",
        "/System/Library/Fonts/GeezaPro.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ];
    if paths.is_empty() {
        paths.extend(KNOWN.iter().map(|p| p.to_string()));
    }
    let mut seen = std::collections::HashSet::new();
    paths.into_iter().filter(|p| seen.insert(p.clone())).filter_map(|p| std::fs::read(p).ok()).collect()
}

/// The file fontconfig picks for sans-serif text in each of
/// [`SCRIPT_LANGS`], skipping the plain sans-serif choice (already the UI
/// font or covered by it).
#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_script_fonts() -> Vec<String> {
    let ask = |pattern: &str| -> Option<String> {
        let out = std::process::Command::new("fc-match").args(["--format=%{file}", pattern]).output().ok()?;
        let path = String::from_utf8(out.stdout).ok()?;
        (out.status.success() && !path.is_empty()).then_some(path)
    };
    let plain = ask("sans-serif");
    SCRIPT_LANGS.iter().filter_map(|lang| ask(&format!("sans-serif:lang={lang}"))).filter(|p| Some(p) != plain.as_ref()).collect()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn fontconfig_script_fonts() -> Vec<String> {
    Vec::new()
}

/// Asks fontconfig for the desktop's sans-serif font, for systems where none
/// of the usual files are installed. A separate bold file is used when there
/// is one; a variable font answers both questions with the same file.
#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_ui_font() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let ask = |pattern: &str| -> Option<String> {
        let out = std::process::Command::new("fc-match").args(["--format=%{file}", pattern]).output().ok()?;
        let path = String::from_utf8(out.stdout).ok()?;
        (out.status.success() && !path.is_empty()).then_some(path)
    };
    let regular = ask("sans-serif")?;
    let bold = ask("sans-serif:bold").filter(|b| *b != regular);
    let bytes = std::fs::read(&regular).ok()?;
    Some((bytes, bold.and_then(|b| std::fs::read(b).ok())))
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn fontconfig_ui_font() -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hebrew_and_other_scripts_have_glyphs_when_the_system_has_a_font() {
        if script_fonts().is_empty() {
            return; // No fallback fonts on this machine to test with.
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts());
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        for family in [FontFamily::Proportional, semibold(), bold()] {
            let font = FontId::new(14.0, family.clone());
            assert!(ctx.fonts_mut(|f| f.has_glyphs(&font, "שלום עולם")), "{family:?} lacks Hebrew");
        }
    }
}
