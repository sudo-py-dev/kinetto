//! Slide styles for the starter templates: big moving titles, collages,
//! polaroids, full-screen photos, quotes, numbers, a film-style shot and a
//! countdown, plus confetti and balloons to decorate any of them.
//!
//! Every builder places things as fractions of the canvas, so the same
//! slide works wide, tall or square. Decorations use `Role::Card`, so they
//! take the colours of whatever look is chosen later; words over photos use
//! `Role::Free` white, so they stay readable on any look.

use super::{page, picture, words};
use crate::Theme;
use kn_geom::{Aspect, Color, Rect};
use kn_model::{Element, ElementKind, Motion, PhotoMotion, Role, ShapeKind, Slide, TextAlign, Transition};

const WHITE: Color = Color::rgb(255, 255, 255);

/// Canvas width and height, and whether it is taller than wide.
fn canvas(aspect: Aspect) -> (f64, f64, bool) {
    let (w, h) = aspect.canvas_size();
    (w, h, h > w)
}

/// Text size that suits the canvas: `wide` on landscape, larger on tall.
fn size(aspect: Aspect, wide: f64) -> f64 {
    if canvas(aspect).2 { wide * 1.35 } else { wide }
}

/// Centres a text item's lines.
fn centred(mut e: Element) -> Element {
    if let ElementKind::Text { align, .. } = &mut e.kind {
        *align = TextAlign::Center;
    }
    e
}

/// Starts an item's entrance `ms` into the slide.
fn at(mut e: Element, ms: i64) -> Element {
    e.start_ms = Some(ms);
    e
}

/// Text in a fixed colour that a new look leaves alone (words over photos).
fn fixed_words(text: &str, frame: Rect, size: f64, bold: bool, color: Color, motion: Motion, th: &Theme) -> Element {
    let mut e = words(text, frame, size, bold, Role::Free, th, motion);
    if let ElementKind::Text { color: c, .. } = &mut e.kind {
        *c = color;
    }
    e
}

/// A plain shape in a fixed colour (bands, frames, shadows).
fn panel(frame: Rect, color: Color, radius: f64, motion: Motion) -> Element {
    let mut e = Element::shape(frame, color);
    if let ElementKind::Shape { corner_radius, .. } = &mut e.kind {
        *corner_radius = radius;
    }
    e.motion = motion;
    e.start_ms = Some(0);
    e
}

/// A shape in the look's accent colour `accent`, following later looks.
fn accent(frame: Rect, kind: ShapeKind, accent: u8, th: &Theme, motion: Motion) -> Element {
    let mut e = Element::shape(frame, th.accents[accent as usize % 3]).with_role(Role::Card(accent));
    if let ElementKind::Shape { shape, corner_radius, .. } = &mut e.kind {
        *shape = kind;
        *corner_radius = frame.w.min(frame.h) / 2.0;
    }
    e.motion = motion;
    e.start_ms = Some(0);
    e
}

/// A picture with square corners and its own photo motion.
fn photo(art: usize, frame: Rect, radius: f64, motion: PhotoMotion) -> Element {
    let mut e = picture(art, frame);
    if let ElementKind::Image { corner_radius, .. } = &mut e.kind {
        *corner_radius = radius;
    }
    e.photo_motion = motion;
    e
}

/// Small repeatable numbers in 0..1, so decorations look scattered but are
/// the same every time the template is made.
struct Scatter(u64);

impl Scatter {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Confetti: small stars, dots, diamonds and ribbons in the look's accents,
/// popping in one after another. `keep_clear` (fractions of the canvas)
/// stays empty for the words.
pub(super) fn confetti(aspect: Aspect, th: &Theme, seed: u64, count: usize, keep_clear: [f64; 4]) -> Vec<Element> {
    let (w, h, _) = canvas(aspect);
    let mut rnd = Scatter(seed);
    let kinds = [ShapeKind::Star, ShapeKind::Ellipse, ShapeKind::Diamond, ShapeKind::Rectangle, ShapeKind::Triangle];
    let [cx0, cy0, cx1, cy1] = keep_clear;
    let mut pieces = Vec::new();
    let mut tries = 0;
    while pieces.len() < count && tries < count * 20 {
        tries += 1;
        let (fx, fy) = (rnd.next(), rnd.next());
        if fx > cx0 && fx < cx1 && fy > cy0 && fy < cy1 {
            continue;
        }
        let kind = kinds[(rnd.next() * kinds.len() as f64) as usize % kinds.len()];
        let side = 22.0 + rnd.next() * 34.0;
        let (pw, ph) = if kind == ShapeKind::Rectangle { (side * 1.8, side * 0.55) } else { (side, side) };
        let mut piece = accent(Rect::new(fx * (w - pw), fy * (h - ph), pw, ph), kind, (rnd.next() * 3.0) as u8, th, Motion::Pop);
        if let ElementKind::Shape { corner_radius, .. } = &mut piece.kind {
            *corner_radius = if kind == ShapeKind::Rectangle { ph / 2.0 } else { 4.0 };
        }
        piece.rotation = (rnd.next() * 90.0 - 45.0).round();
        piece.start_ms = Some((rnd.next() * 900.0) as i64);
        piece.motion_duration_ms = 450;
        pieces.push(piece);
    }
    pieces
}

/// Balloons on strings rising in at the sides.
pub(super) fn balloons(aspect: Aspect, th: &Theme) -> Vec<Element> {
    let (w, h, tall) = canvas(aspect);
    let (bw, bh) = if tall { (230.0, 285.0) } else { (170.0, 210.0) };
    let spots: [(f64, f64); 4] = if tall {
        [(0.06, 0.05), (0.22, 0.02), (0.64, 0.03), (0.80, 0.06)]
    } else {
        [(0.03, 0.10), (0.11, 0.04), (0.80, 0.05), (0.89, 0.12)]
    };
    let mut items = Vec::new();
    for (i, (fx, fy)) in spots.into_iter().enumerate() {
        let (x, y) = (fx * w, fy * h);
        let mut string = panel(Rect::new(x + bw / 2.0 - 2.0, y + bh, 4.0, bh * 1.3), th.text.with_alpha(90), 2.0, Motion::Rise);
        string.start_ms = Some(150 * i as i64);
        items.push(string);
        let mut balloon = accent(Rect::new(x, y, bw, bh), ShapeKind::Ellipse, i as u8, th, Motion::Rise);
        balloon.start_ms = Some(150 * i as i64);
        balloon.motion_duration_ms = 900;
        items.push(balloon);
        let shine = panel(Rect::new(x + bw * 0.22, y + bh * 0.16, bw * 0.18, bh * 0.22), WHITE.with_alpha(90), bw * 0.09, Motion::Rise);
        items.push(at(shine, 150 * i as i64));
    }
    items
}

/// Huge words that arrive a line at a time; the kicker sits above. Lines
/// after the first take the look's accent colours.
pub(super) fn title_card(kicker: &str, lines: &[&str], aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let big = size(aspect, if lines.len() > 2 { 150.0 } else { 190.0 });
    let line_h = big * 1.18;
    let top = (h - line_h * lines.len() as f64) / 2.0 + if tall { 0.0 } else { 30.0 };
    let mut elements = vec![centred(words(kicker, Rect::new(160.0, top - size(aspect, 80.0), w - 320.0, size(aspect, 60.0)), size(aspect, 34.0), true, Role::Caption, th, Motion::Fade))];
    let motions = [Motion::LetterPop, Motion::LetterRise, Motion::LetterDrop, Motion::LetterWave];
    for (i, line) in lines.iter().enumerate() {
        let frame = Rect::new(80.0, top + i as f64 * line_h, w - 160.0, line_h);
        let text = if i == 0 {
            words(line, frame, big, true, Role::Title, th, motions[0])
        } else {
            fixed_words(line, frame, big, true, th.accents[(i - 1) % 3], motions[i % motions.len()], th)
        };
        elements.push(at(centred(text), 150 + i as i64 * 450));
    }
    let bar_w = if tall { 420.0 } else { 300.0 };
    let mut bar = accent(Rect::new((w - bar_w) / 2.0, top + line_h * lines.len() as f64 + 40.0, bar_w, 16.0), ShapeKind::Rectangle, 0, th, Motion::WipeRight);
    bar.start_ms = Some(400 + lines.len() as i64 * 450);
    elements.push(bar);
    page(elements, th, Transition::Zoom, 5)
}

/// A photo filling the screen, slowly moving, with words on a dark band.
pub(super) fn full_bleed(art: usize, kicker: &str, title: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let band = if tall { 0.30 } else { 0.38 };
    let mut elements = vec![
        photo(art, Rect::new(0.0, 0.0, w, h), 0.0, PhotoMotion::ZoomIn),
        panel(Rect::new(0.0, h * (1.0 - band), w, h * band), Color::rgb(0, 0, 0).with_alpha(120), 0.0, Motion::WipeUp),
    ];
    let left = if tall { 120.0 } else { 140.0 };
    elements.push(at(fixed_words(kicker, Rect::new(left, h * (1.0 - band) + size(aspect, 50.0), w - left * 2.0, size(aspect, 50.0)), size(aspect, 30.0), true, WHITE.with_alpha(210), Motion::Fade, th), 300));
    elements.push(at(fixed_words(title, Rect::new(left, h * (1.0 - band) + size(aspect, 110.0), w - left * 2.0, h * band - size(aspect, 130.0)), size(aspect, 96.0), true, WHITE, Motion::LetterRise, th), 500));
    page(elements, th, Transition::Fade, 5)
}

/// One big photo and two smaller ones, with the title on a coloured tag.
pub(super) fn mosaic(arts: [usize; 3], title: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let gap = 28.0;
    let m = 80.0;
    let frames = if tall {
        let big_h = (h - m * 2.0 - gap) * 0.58;
        let small_w = (w - m * 2.0 - gap) / 2.0;
        [
            Rect::new(m, m, w - m * 2.0, big_h),
            Rect::new(m, m + big_h + gap, small_w, h - m * 2.0 - big_h - gap),
            Rect::new(m + small_w + gap, m + big_h + gap, small_w, h - m * 2.0 - big_h - gap),
        ]
    } else {
        let big_w = (w - m * 2.0 - gap) * 0.62;
        let small_h = (h - m * 2.0 - gap) / 2.0;
        [
            Rect::new(m, m, big_w, h - m * 2.0),
            Rect::new(m + big_w + gap, m, w - m * 2.0 - big_w - gap, small_h),
            Rect::new(m + big_w + gap, m + small_h + gap, w - m * 2.0 - big_w - gap, small_h),
        ]
    };
    let moves = [PhotoMotion::ZoomIn, PhotoMotion::PanLeft, PhotoMotion::PanRight];
    let entrances = [Motion::Zoom, Motion::FromRight, Motion::FromRight];
    let mut elements = Vec::new();
    for i in 0..3 {
        let mut p = photo(arts[i], frames[i], 24.0, moves[i]);
        p.motion = entrances[i];
        elements.push(at(p, i as i64 * 220));
    }
    let tag_h = size(aspect, 120.0);
    let tag_w = (frames[0].w * 0.8).min(size(aspect, 760.0));
    let tag = Rect::new(frames[0].x + 40.0, frames[0].y + frames[0].h - tag_h - 40.0, tag_w, tag_h);
    let mut card = accent(tag, ShapeKind::Rectangle, 0, th, Motion::WipeRight);
    if let ElementKind::Shape { corner_radius, .. } = &mut card.kind {
        *corner_radius = 18.0;
    }
    elements.push(at(card, 650));
    let mut label = words(title, Rect::new(tag.x + 36.0, tag.y + tag_h * 0.22, tag_w - 72.0, tag_h * 0.7), size(aspect, 52.0), true, Role::OnCard, th, Motion::Fade);
    if let ElementKind::Text { color, .. } = &mut label.kind {
        *color = th.on_accent;
    }
    elements.push(at(label, 850));
    page(elements, th, Transition::Slide, 5)
}

/// A title and four photos in a grid (2 × 2, or a column of four on tall).
pub(super) fn grid(arts: [usize; 4], title: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let m = 90.0;
    let gap = 24.0;
    let title_h = size(aspect, 150.0);
    let mut elements = vec![words(title, Rect::new(m, m * 0.7, w - m * 2.0, title_h), size(aspect, 80.0), true, Role::Title, th, Motion::LetterFade)];
    let top = m * 0.7 + title_h + 10.0;
    let (cols, rows) = if tall { (1, 4) } else { (2, 2) };
    let cell_w = (w - m * 2.0 - gap * (cols - 1) as f64) / cols as f64;
    let cell_h = (h - top - m - gap * (rows - 1) as f64) / rows as f64;
    for (i, art) in arts.into_iter().enumerate() {
        let (c, r) = (i % cols, i / cols);
        let frame = Rect::new(m + c as f64 * (cell_w + gap), top + r as f64 * (cell_h + gap), cell_w, cell_h);
        let mut p = photo(art, frame, 20.0, [PhotoMotion::ZoomIn, PhotoMotion::PanRight, PhotoMotion::PanLeft, PhotoMotion::ZoomOut][i]);
        p.motion = Motion::Pop;
        elements.push(at(p, 250 + i as i64 * 180));
    }
    page(elements, th, Transition::Zoom, 6)
}

/// Three instant photos tossed onto the slide, each tilted, with a soft
/// shadow, and a title above.
pub(super) fn polaroids(arts: [usize; 3], title: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let card_w = if tall { w * 0.62 } else { w * 0.27 };
    let card_h = card_w * 1.18;
    let border = card_w * 0.06;
    let tilt = [-7.0, 5.0, -3.0];
    let centres: [(f64, f64); 3] = if tall {
        [(0.40, 0.33), (0.60, 0.56), (0.42, 0.79)]
    } else {
        [(0.24, 0.60), (0.50, 0.57), (0.76, 0.61)]
    };
    let title_y = if tall { 0.06 * h } else { 0.07 * h };
    let mut elements = vec![centred(words(title, Rect::new(120.0, title_y, w - 240.0, size(aspect, 130.0)), size(aspect, 78.0), true, Role::Title, th, Motion::LetterRise))];
    for i in 0..3 {
        let (cx, cy) = (centres[i].0 * w, centres[i].1 * h);
        let card = Rect::new(cx - card_w / 2.0, cy - card_h / 2.0, card_w, card_h);
        let start = 300 + i as i64 * 350;
        let mut shadow = panel(Rect::new(card.x + 14.0, card.y + 22.0, card_w, card_h), Color::rgb(0, 0, 0).with_alpha(45), 10.0, Motion::Drop);
        shadow.rotation = tilt[i];
        elements.push(at(shadow, start));
        let mut paper = panel(card, Color::rgb(253, 252, 248), 8.0, Motion::Drop);
        paper.rotation = tilt[i];
        elements.push(at(paper, start));
        // The photo sits in the card's upper part. Both turn about their own
        // centres, so move the photo's centre the way the card turns.
        let photo_h = card_w - border * 2.0;
        let offset = -(card_h / 2.0) + border + photo_h / 2.0;
        let angle = tilt[i].to_radians();
        let (px, py) = (cx - offset * angle.sin(), cy + offset * angle.cos());
        let mut p = photo(arts[i], Rect::new(px - photo_h / 2.0, py - photo_h / 2.0, photo_h, photo_h), 4.0, PhotoMotion::Breathe);
        p.rotation = tilt[i];
        p.motion = Motion::Drop;
        elements.push(at(p, start));
    }
    page(elements, th, Transition::Slide, 6)
}

/// A large quote with an oversized opening mark and who said it.
pub(super) fn quote(text: &str, author: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let m = if tall { 130.0 } else { 220.0 };
    let mark = fixed_words("\u{201c}", Rect::new(m - 20.0, h * 0.12, 400.0, size(aspect, 380.0)), size(aspect, 360.0), true, th.accents[0], Motion::Pop, th);
    let body_size = size(aspect, if text.chars().count() > 70 { 72.0 } else { 88.0 });
    // Roughly where the quote ends: its lines at about 0.5 em per letter.
    let per_line = ((w - m * 2.0) / (body_size * 0.5)).max(1.0);
    let lines = (text.chars().count() as f64 / per_line).ceil().max(1.0);
    let author_y = (h * 0.34 + lines * body_size * 1.25 + 60.0).min(h * 0.82);
    let elements = vec![
        mark,
        at(words(text, Rect::new(m, h * 0.34, w - m * 2.0, h * 0.42), body_size, true, Role::Title, th, Motion::LetterFade), 300),
        at(words(author, Rect::new(m, author_y + 40.0, w - m * 2.0, size(aspect, 70.0)), size(aspect, 40.0), false, Role::Caption, th, Motion::Fade), 1400),
        at(accent(Rect::new(m, author_y, 120.0, 10.0), ShapeKind::Rectangle, 1, th, Motion::WipeRight), 1300),
    ];
    let mut slide = page(elements, th, Transition::Fade, 6);
    if tall {
        slide.duration = kn_time::Tick::from_secs(7);
    }
    slide
}

/// A giant number with a line under it and three small facts below.
pub(super) fn big_number(number: &str, label: &str, facts: [(&str, &str); 3], aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, tall) = canvas(aspect);
    let num_size = size(aspect, 300.0);
    let mut elements = vec![
        at(centred(words(number, Rect::new(80.0, h * 0.08, w - 160.0, num_size * 1.2), num_size, true, Role::Title, th, Motion::LetterRise)), 100),
        at(centred(words(label, Rect::new(160.0, h * 0.08 + num_size * 1.2, w - 320.0, size(aspect, 80.0)), size(aspect, 46.0), false, Role::Caption, th, Motion::Fade)), 700),
    ];
    let top = if tall { h * 0.42 } else { h * 0.60 };
    for (i, (value, what)) in facts.into_iter().enumerate() {
        let frame = if tall {
            Rect::new(160.0, top + i as f64 * h * 0.17, w - 320.0, h * 0.15)
        } else {
            Rect::new(140.0 + i as f64 * (w - 280.0) / 3.0, top, (w - 280.0) / 3.0 - 30.0, h * 0.32)
        };
        let mut dot = accent(Rect::new(frame.x + frame.w / 2.0 - 14.0, frame.y, 28.0, 28.0), ShapeKind::Ellipse, i as u8, th, Motion::Pop);
        dot.start_ms = Some(1000 + i as i64 * 250);
        elements.push(dot);
        elements.push(at(centred(words(value, Rect::new(frame.x, frame.y + 50.0, frame.w, size(aspect, 90.0)), size(aspect, 72.0), true, Role::Title, th, Motion::Pop)), 1000 + i as i64 * 250));
        elements.push(at(centred(words(what, Rect::new(frame.x, frame.y + 50.0 + size(aspect, 95.0), frame.w, size(aspect, 80.0)), size(aspect, 34.0), false, Role::Body, th, Motion::Fade)), 1150 + i as i64 * 250));
    }
    page(elements, th, Transition::SlideUp, 6)
}

/// A film-style shot: a moving photo between black bars, with spaced-out
/// capitals in the middle.
pub(super) fn cinematic(art: usize, kicker: &str, title: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, _) = canvas(aspect);
    let bar = h * 0.12;
    let black = Color::rgb(0, 0, 0);
    let elements = vec![
        photo(art, Rect::new(0.0, 0.0, w, h), 0.0, PhotoMotion::PanRight),
        panel(Rect::new(0.0, 0.0, w, h), black.with_alpha(70), 0.0, Motion::None),
        panel(Rect::new(0.0, 0.0, w, bar), black, 0.0, Motion::WipeDown),
        panel(Rect::new(0.0, h - bar, w, bar), black, 0.0, Motion::WipeUp),
        at(centred(fixed_words(kicker, Rect::new(120.0, h * 0.36, w - 240.0, size(aspect, 60.0)), size(aspect, 32.0), true, WHITE.with_alpha(200), Motion::Fade, th)), 600),
        at(centred(fixed_words(title, Rect::new(120.0, h * 0.36 + size(aspect, 70.0), w - 240.0, h * 0.4), size(aspect, 120.0), true, WHITE, Motion::LetterFade, th)), 900),
    ];
    let mut slide = page(elements, th, Transition::Dip, 6);
    slide.dark = true;
    slide
}

/// One number of a film-leader countdown: a big digit on an accent disc,
/// on a dark slide.
pub(super) fn countdown(n: u8, aspect: Aspect, th: &Theme) -> Slide {
    let (w, h, _) = canvas(aspect);
    let ring = w.min(h) * 0.55;
    let digit = ring * 0.62;
    // Accent 1: some looks use accent 0 for their dark background too.
    let mut disc = accent(Rect::new((w - ring) / 2.0, (h - ring) / 2.0, ring, ring), ShapeKind::Ellipse, 1, th, Motion::Zoom);
    disc.motion_duration_ms = 300;
    let mut number = centred(words(&n.to_string(), Rect::new(0.0, (h - digit * 1.2) / 2.0, w, digit * 1.2), digit, true, Role::OnCard, th, Motion::Zoom));
    number.motion_duration_ms = 300;
    number.start_ms = Some(0);
    if let ElementKind::Text { color, .. } = &mut number.kind {
        *color = th.on_accent;
    }
    let mut slide = page(vec![disc, number], th, Transition::Cut, 1);
    slide.dark = true;
    slide.background = th.dark_background;
    slide
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_repeats_and_stays_in_range() {
        let (mut a, mut b) = (Scatter(3), Scatter(3));
        for _ in 0..1000 {
            let x = a.next();
            assert_eq!(x, b.next());
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn decorations_stay_on_the_canvas_and_out_of_the_words() {
        let th = crate::theme("Candy");
        for aspect in [Aspect::Wide16x9, Aspect::Tall9x16, Aspect::Square1x1] {
            let (w, h) = aspect.canvas_size();
            let clear = [0.25, 0.3, 0.75, 0.7];
            let pieces = confetti(aspect, th, 9, 40, clear);
            assert_eq!(pieces.len(), 40);
            for p in pieces.iter().chain(&balloons(aspect, th)) {
                let f = p.frame;
                assert!(f.x >= 0.0 && f.y >= 0.0 && f.x + f.w <= w + 1.0 && f.y + f.h <= h + 1.0, "{aspect:?}: {f:?}");
            }
            for p in &pieces {
                let (fx, fy) = (p.frame.x / (w - p.frame.w), p.frame.y / (h - p.frame.h));
                assert!(!(fx > clear[0] && fx < clear[2] && fy > clear[1] && fy < clear[3]));
            }
        }
    }

    #[test]
    fn polaroid_photos_sit_inside_their_tilted_cards() {
        let th = crate::theme("Paper");
        for aspect in [Aspect::Wide16x9, Aspect::Tall9x16] {
            let slide = polaroids([0, 1, 2], "Title", aspect, th);
            // Title, then (shadow, card, photo) for each polaroid.
            for trio in slide.elements[1..].chunks(3) {
                let (card, photo) = (&trio[1], &trio[2]);
                assert_eq!(card.rotation, photo.rotation);
                let c = (card.frame.x + card.frame.w / 2.0, card.frame.y + card.frame.h / 2.0);
                let p = (photo.frame.x + photo.frame.w / 2.0, photo.frame.y + photo.frame.h / 2.0);
                // Unturn the photo's centre about the card's: it must be
                // straight above the centre, in the card's upper part.
                let a = -card.rotation.to_radians();
                let (dx, dy) = (p.0 - c.0, p.1 - c.1);
                let (ux, uy) = (dx * a.cos() - dy * a.sin(), dx * a.sin() + dy * a.cos());
                assert!(ux.abs() < 0.01, "{aspect:?}: photo off-centre by {ux}");
                assert!(uy < 0.0 && uy - photo.frame.h / 2.0 > -card.frame.h / 2.0);
            }
        }
    }
}
