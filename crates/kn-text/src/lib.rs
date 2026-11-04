//! Text layout with parley and drawing with vello. Layouts are cached, so
//! preview playback doesn't reshape the same text every frame.

use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, GenericFamily, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use std::collections::HashMap;
use vello::Scene;
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// Everything that changes a layout's shape. Sizes are in slide units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub bold: bool,
    pub align: Align,
    pub width: f32,
}

#[derive(PartialEq, Eq, Hash)]
struct Key {
    text: String,
    size: u32,
    width: u32,
    bold: bool,
    align: Align,
}

impl Key {
    fn new(text: &str, s: &TextStyle) -> Self {
        Key { text: text.to_owned(), size: s.size.to_bits(), width: s.width.to_bits(), bold: s.bold, align: s.align }
    }
}

/// Keeps a few hundred layouts; editing creates new keys, so clear when full.
const CACHE_LIMIT: usize = 512;

pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    cache: HashMap<Key, Layout<()>>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    pub fn new() -> Self {
        TextEngine { fonts: FontContext::new(), layouts: LayoutContext::new(), cache: HashMap::new() }
    }

    /// Width and height of `text` laid out in `style`.
    pub fn measure(&mut self, text: &str, style: &TextStyle) -> (f32, f32) {
        let layout = self.layout(text, style);
        (layout.width(), layout.height())
    }

    /// Draws `text` with its top-left corner at the origin of `transform`.
    pub fn draw(&mut self, scene: &mut Scene, transform: Affine, text: &str, style: &TextStyle, color: Color) {
        let layout = self.layout(text, style);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                let run = glyph_run.run();
                // vello takes raw i16 variation coordinates.
                let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                let glyphs = glyph_run.positioned_glyphs().map(|g| vello::Glyph { id: g.id, x: g.x, y: g.y });
                scene
                    .draw_glyphs(run.font())
                    .font_size(run.font_size())
                    .normalized_coords(&coords)
                    .transform(transform)
                    .brush(color)
                    .draw(Fill::NonZero, glyphs);
            }
        }
    }

    fn layout(&mut self, text: &str, style: &TextStyle) -> &Layout<()> {
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        let key = Key::new(text, style);
        if !self.cache.contains_key(&key) {
            let layout = self.build(text, style);
            self.cache.insert(Key::new(text, style), layout);
        }
        &self.cache[&key]
    }

    fn build(&mut self, text: &str, style: &TextStyle) -> Layout<()> {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::from(GenericFamily::SansSerif)));
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(1.2)));
        builder.push_default(StyleProperty::FontWeight(if style.bold { FontWeight::BOLD } else { FontWeight::NORMAL }));
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(style.width));
        let alignment = match style.align {
            Align::Start => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::End => Alignment::End,
        };
        // Lines were broken at the box width, so alignment is relative to the
        // box and centred text sits in the middle of its frame.
        layout.align(alignment, AlignmentOptions::default());
        layout
    }
}
