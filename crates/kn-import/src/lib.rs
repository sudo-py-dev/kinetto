//! Turning existing material into slides: pasted text, Markdown files and
//! PowerPoint decks all become an [`Outline`], which [`build`] lays out in
//! the chosen look with sensible timing.

mod markdown;
mod pptx;
mod style;

pub use markdown::parse_markdown;
pub use pptx::parse_pptx;
pub use style::{Dice, Pick, Style, animate};

use kn_geom::{Aspect, Color, Rect};
use kn_layout::{Layout, Theme, Words};
use kn_media::MediaLibrary;
use kn_model::{Element, Motion, Project, Role, Slide, Transition};
use kn_time::Tick;

/// A photo and its optional caption in an ordered collection.
#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub media: String,
    pub width: u32,
    pub height: u32,
    pub caption: String,
}

/// What one slide should say, before any layout.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    PhotoGroup { title: String, photos: Vec<Photo> },
    Title { title: String, subtitle: String },
    Text { title: String, body: String },
    Points { title: String, points: Vec<String> },
    Quote { quote: String, author: String },
    /// A picture already added to the media library.
    Picture { media: String, width: u32, height: u32, caption: String },
}

#[derive(Debug, Default)]
pub struct Outline {
    pub parts: Vec<Part>,
    /// Things that couldn't be brought in, in plain words.
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("this file couldn't be read as a presentation: {0}")]
    Damaged(String),
    #[error("there's nothing in it to make slides from")]
    Empty,
}

/// Plain text: blank lines separate slides; a slide's first line is its
/// title, and lines starting with "-", "*", "•" or "1." are points.
pub fn parse_plain(text: &str) -> Outline {
    let mut parts = Vec::new();
    for block in text.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
        let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
        let Some(first) = lines.next() else { continue };
        let rest: Vec<&str> = lines.collect();
        let title = strip_bullet(first).unwrap_or(first).to_owned();
        if rest.is_empty() {
            parts.push(if parts.is_empty() { Part::Title { title, subtitle: String::new() } } else { Part::Text { title, body: String::new() } });
        } else if rest.iter().all(|l| strip_bullet(l).is_some()) {
            parts.push(Part::Points { title, points: rest.iter().filter_map(|l| strip_bullet(l)).map(str::to_owned).collect() });
        } else if parts.is_empty() && rest.len() == 1 {
            parts.push(Part::Title { title, subtitle: rest[0].to_owned() });
        } else {
            parts.push(Part::Text { title, body: rest.join(" ") });
        }
    }
    Outline { parts, warnings: Vec::new() }
}

/// Typed or pasted text: read as Markdown when it has Markdown headings or
/// quotes, otherwise as plain paragraphs.
pub fn outline_from_text(text: &str, media: &mut MediaLibrary) -> Outline {
    let looks_like_markdown = text.lines().any(|l| l.trim_start().starts_with('#') || l.trim_start().starts_with('>'));
    if looks_like_markdown { parse_markdown(text, std::path::Path::new("."), media) } else { parse_plain(text) }
}

/// The text of a list item, if `line` is one.
pub(crate) fn strip_bullet(line: &str) -> Option<&str> {
    let line = line.trim_start();
    for marker in ["- ", "* ", "• ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(rest.trim());
        }
    }
    None
}

/// Slides last long enough to read their words: about 2.5 words a second,
/// plus a moment to take the slide in.
fn reading_time(words: &str) -> Tick {
    let count = words.split_whitespace().count() as f64;
    let secs = (1.8 + count / 2.5).clamp(3.0, 12.0);
    Tick::from_millis((secs * 2.0).round() as i64 * 500)
}

/// Lays `outline` out as a project in `theme`'s look.
pub fn build(outline: &Outline, aspect: Aspect, theme: &Theme) -> Result<Project, ImportError> {
    let mut slides = Vec::new();
    // A fn, not a closure, so each call may borrow words of its own lifetime.
    fn laid(layout: Layout, words: Words<'_>, text: &str, aspect: Aspect, theme: &Theme) -> Slide {
        let mut s = kn_layout::slide(layout, words, aspect, theme);
        s.duration = reading_time(text);
        s
    }
    for part in &outline.parts {
        match part {
            Part::Title { title, subtitle } => {
                let mut s = laid(Layout::Title, Words { title, body: subtitle, ..Words::default() }, &format!("{title} {subtitle}"), aspect, theme);
                if subtitle.is_empty() {
                    s.elements.retain(|e| e.role != Role::Caption);
                }
                slides.push(s);
            }
            Part::Text { title, body } => {
                let mut s = laid(Layout::TitleAndText, Words { title, body, ..Words::default() }, &format!("{title} {body}"), aspect, theme);
                if body.is_empty() {
                    s.elements.retain(|e| e.role != Role::Body);
                }
                slides.push(s);
            }
            // Three points fill the cards layout; other counts become a
            // bulleted text slide, or several card slides.
            Part::Points { title, points } => {
                let chunks: Vec<&[String]> = points.chunks(3).collect();
                for (i, chunk) in chunks.iter().enumerate() {
                    let title = if chunks.len() > 1 && i > 0 { format!("{title} (continued)") } else { title.clone() };
                    let words_text = format!("{title} {}", chunk.join(" "));
                    if let [a, b, c] = chunk {
                        slides.push(laid(Layout::List, Words { title: &title, points: [a, b, c], ..Words::default() }, &words_text, aspect, theme));
                    } else {
                        let body: Vec<String> = chunk.iter().map(|p| format!("•  {p}")).collect();
                        let body = body.join("\n");
                        slides.push(laid(Layout::TitleAndText, Words { title: &title, body: &body, ..Words::default() }, &words_text, aspect, theme));
                    }
                }
            }
            Part::Quote { quote, author } => {
                let quoted = format!("\u{201c}{}\u{201d}", quote.trim_matches(['"', '\u{201c}', '\u{201d}']));
                let mut s = laid(Layout::Quote, Words { title: &quoted, body: author, ..Words::default() }, quote, aspect, theme);
                if author.is_empty() {
                    s.elements.retain(|e| e.role != Role::Caption);
                }
                slides.push(s);
            }
            Part::PhotoGroup { title, photos } => {
                for chunk in photos.chunks(4) {
                    slides.push(photo_collage(aspect, theme, title, chunk));
                }
            }
            Part::Picture { media, width, height, caption } => slides.push(picture_slide(aspect, theme, media, *width, *height, caption)),
        }
    }
    if slides.is_empty() {
        return Err(ImportError::Empty);
    }
    slides[0].transition = Transition::Cut;
    let project = Project { aspect, slides, theme: theme.name.into(), music: None };
    Ok(kn_layout::recolour(&project, theme))
}

/// Up to four photos with a shared heading and individual captions.
fn photo_collage(aspect: Aspect, theme: &Theme, title: &str, photos: &[Photo]) -> Slide {
    let (w, h) = aspect.canvas_size();
    let mut slide = kn_layout::slide(Layout::Blank, Words::default(), aspect, theme);
    let top = if title.is_empty() { 90.0 } else { 220.0 };
    if !title.is_empty() {
        slide.elements.push(Element::text(title, Rect::new(100., 65., w - 200., 120.), 76., true, theme.text).with_role(Role::Title));
    }
    let columns = if photos.len() == 1 || aspect == Aspect::Tall9x16 && photos.len() == 2 { 1 } else { 2 };
    let rows = photos.len().div_ceil(columns).max(1);
    let cell_w = (w - 200. - 36. * (columns - 1) as f64) / columns as f64;
    let cell_h = (h - top - 90. - 36. * (rows - 1) as f64) / rows as f64;
    for (i, photo) in photos.iter().enumerate() {
        let x = 100. + (i % columns) as f64 * (cell_w + 36.);
        let y = top + (i / columns) as f64 * (cell_h + 36.);
        let caption_h = if photo.caption.is_empty() { 0. } else { 85. };
        let mut image = Element::image(photo.media.clone(), Rect::new(x, y, cell_w, cell_h - caption_h));
        image.photo_motion = kn_model::PhotoMotion::ZoomIn;
        slide.elements.push(image);
        if !photo.caption.is_empty() {
            slide.elements.push(Element::text(&photo.caption, Rect::new(x, y + cell_h - 65., cell_w, 65.), 42., false, theme.muted).with_role(Role::Caption));
        }
    }
    let words = format!("{title} {}", photos.iter().map(|p| p.caption.as_str()).collect::<Vec<_>>().join(" "));
    slide.duration = reading_time(&words).max(Tick::from_secs(5));
    slide
}

/// A picture as large as fits, with its caption underneath.
fn picture_slide(aspect: Aspect, theme: &Theme, media: &str, width: u32, height: u32, caption: &str) -> Slide {
    let (cw, ch) = aspect.canvas_size();
    let caption_room = if caption.is_empty() { 0.0 } else { 140.0 };
    let (iw, ih) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let fit = ((cw - 240.0) / iw).min((ch - 160.0 - caption_room) / ih);
    let (w, h) = (iw * fit, ih * fit);
    let top = (ch - h - caption_room) / 2.0;
    let mut elements = vec![Element { motion: Motion::Fade, ..Element::image(media.to_owned(), Rect::new((cw - w) / 2.0, top, w, h)) }];
    elements[0].kind = kn_model::ElementKind::Image { media: media.to_owned(), corner_radius: 16.0 };
    if !caption.is_empty() {
        let y = top + h + 40.0;
        elements.push(Element::text(caption, Rect::new(160.0, y, cw - 320.0, 70.0), 52.0, false, Color::rgb(0, 0, 0)).with_role(Role::Caption));
    }
    let mut slide = kn_layout::slide(Layout::Blank, Words::default(), aspect, theme);
    slide.elements = elements;
    slide.duration = reading_time(caption).max(Tick::from_secs(4));
    slide
}

/// Adds a picture's bytes to `media`, noting a warning if it isn't usable.
pub(crate) fn add_picture(media: &mut MediaLibrary, bytes: Vec<u8>, caption: String, warnings: &mut Vec<String>, name: &str) -> Option<Part> {
    match media.import(bytes) {
        Ok(info) => Some(Part::Picture { media: info.id, width: info.width, height: info.height, caption }),
        Err(err) => {
            warnings.push(format!("Picture {name} was left out: {err}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_becomes_slides() {
        let o = parse_plain("My talk\nWhy it matters\n\nThree things\n- one\n- two\n- three\n\nThe end");
        assert_eq!(
            o.parts,
            vec![
                Part::Title { title: "My talk".into(), subtitle: "Why it matters".into() },
                Part::Points { title: "Three things".into(), points: vec!["one".into(), "two".into(), "three".into()] },
                Part::Text { title: "The end".into(), body: String::new() },
            ]
        );
    }

    #[test]
    fn bullets_are_recognised() {
        assert_eq!(strip_bullet("- a"), Some("a"));
        assert_eq!(strip_bullet("12. b"), Some("b"));
        assert_eq!(strip_bullet("3) c"), Some("c"));
        assert_eq!(strip_bullet("2026 was great"), None);
        assert_eq!(strip_bullet("-dash"), None);
    }

    #[test]
    fn point_counts_choose_layouts_and_slides_fit() {
        let theme = kn_layout::theme("Paper");
        let points = |n: usize| Part::Points { title: "T".into(), points: (0..n).map(|i| format!("p{i}")).collect() };
        let outline = Outline { parts: vec![points(2), points(3), points(7)], warnings: vec![] };
        let p = build(&outline, Aspect::Wide16x9, theme).unwrap();
        // 2 → text; 3 → cards; 7 → cards, cards, text.
        assert_eq!(p.slides.len(), 5);
        let cards = |s: &Slide| s.elements.iter().filter(|e| matches!(e.role, Role::Card(_))).count();
        assert_eq!(p.slides.iter().map(cards).collect::<Vec<_>>(), vec![0, 3, 3, 3, 0]);
        let (w, h) = Aspect::Wide16x9.canvas_size();
        for e in p.slides.iter().flat_map(|s| &s.elements) {
            assert!(e.frame.x >= 0.0 && e.frame.x + e.frame.w <= w && e.frame.y >= 0.0 && e.frame.y <= h, "{:?}", e.frame);
        }
    }

    #[test]
    fn reading_time_grows_with_words_within_limits() {
        assert_eq!(reading_time("Hi"), Tick::from_secs(3));
        assert!(reading_time(&"word ".repeat(20)) > Tick::from_secs(6));
        assert_eq!(reading_time(&"word ".repeat(500)), Tick::from_secs(12));
    }

    #[test]
    fn nothing_to_import_is_an_error() {
        assert!(matches!(build(&Outline::default(), Aspect::Wide16x9, kn_layout::theme("Paper")), Err(ImportError::Empty)));
    }
}

#[cfg(test)]
mod collage_tests {
    use super::*;

    #[test]
    fn collages_paginate_and_fit_every_video_shape() {
        for aspect in [Aspect::Wide16x9, Aspect::Tall9x16, Aspect::Square1x1] {
            let photos: Vec<_> = (0..9).map(|i| Photo { media: format!("photo-{i}"), width: 800, height: 600, caption: format!("Caption {i}") }).collect();
            let outline = Outline { parts: vec![Part::PhotoGroup { title: "Our trip".into(), photos }], warnings: vec![] };
            let project = build(&outline, aspect, kn_layout::theme("Paper")).unwrap();
            assert_eq!(project.slides.len(), 3);
            let (w, h) = aspect.canvas_size();
            let mut images = 0;
            for slide in &project.slides {
                assert!(slide.elements.iter().any(|e| matches!(&e.kind, kn_model::ElementKind::Text { text, .. } if text == "Our trip")));
                for element in &slide.elements {
                    let r = element.frame;
                    assert!(r.w > 0. && r.h > 0. && r.x >= 0. && r.y >= 0. && r.x + r.w <= w && r.y + r.h <= h);
                    if matches!(element.kind, kn_model::ElementKind::Image { .. }) { images += 1; }
                }
            }
            assert_eq!(images, 9);
        }
    }
}
