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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetterEffect { Typewriter, Reveal, Fade, Rise, Drop, Pop, Spin, Wave }

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

    /// Animate the final shaped glyphs: wrapping, alignment and ligatures stay stable.
    #[allow(clippy::too_many_arguments)] // Rendering inputs mirror draw(), plus the letter effect and its progress.
    pub fn draw_letters(&mut self, scene: &mut Scene, transform: Affine, text: &str, style: &TextStyle, color: Color, effect: LetterEffect, progress: f64) {
        if progress >= 1.0 { self.draw(scene, transform, text, style, color); return; }
        if progress <= 0.0 { return; }
        let layout = self.layout(text, style);
        let count: usize = layout.lines().flat_map(|line| line.items()).map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => run.positioned_glyphs().count(), _ => 0,
        }).sum();
        let mut index = 0;
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
                let run = glyph_run.run();
                let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                for g in glyph_run.positioned_glyphs() {
                    let p = letter_progress(index, count, progress, effect == LetterEffect::Typewriter);
                    index += 1;
                    if p <= 0.0 { continue; }
                    let q = 1.0 - (1.0-p).powi(3);
                    let remaining = 1.0-q;
                    let size = f64::from(run.font_size());
                    let (dy, scale, angle) = match effect {
                        LetterEffect::Rise => (remaining * size * 0.6, 1.0, 0.0),
                        LetterEffect::Drop => (-remaining * size * 0.6, 1.0, 0.0),
                        LetterEffect::Pop => (0.0, 0.3+0.7*q, 0.0),
                        LetterEffect::Spin => (0.0, 0.5+0.5*q, -remaining*1.2),
                        LetterEffect::Wave => ((p*std::f64::consts::TAU).sin()*remaining*size*0.5, 1.0, 0.0),
                        _ => (0.0, 1.0, 0.0),
                    };
                    let centre = (f64::from(g.x + g.advance/2.0), f64::from(g.y)-size*0.35);
                    let local = transform * Affine::translate((centre.0,centre.1+dy)) * Affine::rotate(angle) * Affine::scale(scale) * Affine::translate((-centre.0,-centre.1));
                    if effect == LetterEffect::Reveal {
                        let left = f64::from(g.x) - size * 0.15;
                        let right = f64::from(g.x + g.advance) + size * 0.2;
                        let clip = vello::kurbo::Rect::new(left, f64::from(g.y)-size*1.5, left+(right-left)*q, f64::from(g.y)+size*0.5);
                        scene.push_clip_layer(Fill::NonZero, local, &clip);
                    }
                    scene.draw_glyphs(run.font()).font_size(run.font_size()).normalized_coords(&coords)
                        .transform(local).brush(color.with_alpha(color.components[3] * q as f32))
                        .draw(Fill::NonZero, [vello::Glyph { id: g.id, x: g.x, y: g.y }].into_iter());
                    if effect == LetterEffect::Reveal { scene.pop_layer(); }
                }
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

/// Each letter gets a full settling interval, including the final letter.
fn letter_progress(index: usize, count: usize, progress: f64, discrete: bool) -> f64 {
    let progress = progress.clamp(0.0, 1.0);
    if discrete { return if (index as f64) < progress * count as f64 { 1.0 } else { 0.0 }; }
    let delay = index as f64 / count.max(1) as f64 * 0.65;
    ((progress-delay)/0.35).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staggered_letters_start_hidden_and_all_finish() {
        for count in [1,2,40,200] {
            for index in 0..count {
                for discrete in [false,true] {
                    assert_eq!(letter_progress(index,count,0.0,discrete),0.0);
                    assert_eq!(letter_progress(index,count,1.0,discrete),1.0);
                }
            }
        }
        assert!(letter_progress(0,10,0.3,false) > letter_progress(9,10,0.3,false));
    }
    #[test]
    fn letter_effects_preserve_unicode_layout() {
        let mut engine = TextEngine::new();
        let style = TextStyle { size: 40., width: 320., bold: false, align: Align::Center };
        for text in ["Hello office", "שלום עולם", "مرحبا بالعالم", "Cafe\u{301} 👩‍💻\nNew line"] {
            let before = engine.measure(text, &style);
            for effect in [LetterEffect::Typewriter, LetterEffect::Reveal, LetterEffect::Fade, LetterEffect::Rise, LetterEffect::Drop, LetterEffect::Pop, LetterEffect::Spin, LetterEffect::Wave] {
                for p in [0.0,0.1,0.5,0.9,1.0] {
                    engine.draw_letters(&mut Scene::new(), Affine::IDENTITY, text, &style, Color::BLACK, effect, p);
                }
            }
            assert_eq!(engine.measure(text, &style), before);
        }
    }
}
