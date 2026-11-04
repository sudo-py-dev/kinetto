//! Turns a project at a point in time into a vello [`Scene`]. Preview and
//! export both call [`render_frame`], so what you see is what you export.

use kn_geom::{Color, Rect};
use kn_media::MediaLibrary;
use kn_model::{Element, ElementKind, Motion, Project, Slide, TextAlign, Transition};
use kn_text::{Align, TextEngine, TextStyle};
use kn_time::Tick;
use std::collections::HashMap;
use vello::Scene;
use vello::kurbo::{Affine, RoundedRect, Vec2};
use vello::peniko::{self, Blob, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, Mix};


/// Transitions play over the start of the incoming slide.
pub const TRANSITION: f64 = 0.6;

/// Caches kept between frames: text layouts and decoded pictures.
#[derive(Default)]
pub struct RenderContext {
    pub text: TextEngine,
    /// `None` marks a picture that failed to decode, so it isn't retried
    /// every frame.
    images: HashMap<String, Option<ImageData>>,
}

impl RenderContext {
    pub fn new() -> Self {
        Self::default()
    }

    fn image(&mut self, media: &MediaLibrary, id: &str) -> Option<&ImageData> {
        self.images
            .entry(id.to_owned())
            .or_insert_with(|| match media.decode(id) {
                Ok(d) => Some(ImageData {
                    data: Blob::from(d.rgba),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::Alpha,
                    width: d.width,
                    height: d.height,
                }),
                Err(err) => {
                    tracing::warn!("picture {id} can't be shown: {err}");
                    None
                }
            })
            .as_ref()
    }
}

/// Draws the frame at `t` into `scene`, scaled from slide units by `scale`.
pub fn render_frame(project: &Project, media: &MediaLibrary, t: Tick, scale: f64, scene: &mut Scene, cx: &mut RenderContext) {
    // Vello 0.11 loses its persistent image atlas after a resource-free frame,
    // while still treating the images as uploaded. A shared transparent pixel
    // keeps the atlas alive during blank slides and animation boundaries.
    // Remove when https://github.com/linebender/vello/issues/1948 is fixed.
    static ATLAS_ANCHOR: std::sync::OnceLock<ImageData> = std::sync::OnceLock::new();
    let anchor = ATLAS_ANCHOR.get_or_init(|| ImageData {
        data: Blob::from(vec![0_u8; 4]),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: 1,
        height: 1,
    });
    scene.draw_image(anchor, Affine::IDENTITY);
    let Some((index, local)) = project.slide_at(t) else { return };
    let slide = &project.slides[index];
    let (w, h) = project.aspect.canvas_size();
    let page = vello::kurbo::Rect::new(0.0, 0.0, w, h);
    let root = Affine::scale(scale);
    let local = local.as_secs_f64();

    let p = (local / TRANSITION).clamp(0.0, 1.0);
    let incoming = match (index.checked_sub(1), slide.transition) {
        (Some(prev), kind) if kind != Transition::Cut && p < 1.0 => {
            let before = &project.slides[prev];
            if backdrop(kind) {
                scene.fill(Fill::NonZero, root, peniko::Color::BLACK, None, &page);
            }
            let (out, inc) = transition_layers(kind, ease_in_out_cubic(p), root, (w, h));
            draw_slide(scene, out, page, before, before.duration.as_secs_f64(), media, cx);
            inc
        }
        _ => Layer::at(root),
    };
    draw_slide(scene, incoming, page, slide, local, media, cx);
}

/// Where a whole slide is drawn and how it looks while changing.
#[derive(Clone, Copy, Debug)]
struct Layer {
    root: Affine,
    alpha: f64,
    /// How much of the slide's width shows, from the left.
    reveal: f64,
    /// Darkening of a face turned away from the viewer, 0 to 1.
    shade: f64,
}

impl Layer {
    fn at(root: Affine) -> Self {
        Layer { root, alpha: 1.0, reveal: 1.0, shade: 0.0 }
    }

    fn at_alpha(root: Affine, alpha: f64) -> Self {
        Layer { alpha, ..Layer::at(root) }
    }
}

/// Transitions that leave parts of the frame uncovered, which show black.
fn backdrop(kind: Transition) -> bool {
    matches!(kind, Transition::Dip | Transition::TurnLeft | Transition::TurnRight | Transition::Flip)
}

/// Outgoing and incoming slide placement at transition progress `p`.
fn transition_layers(kind: Transition, p: f64, root: Affine, (width, height): (f64, f64)) -> (Layer, Layer) {
    let out = Layer::at(root);
    match kind {
        Transition::Cut => (out, Layer::at(root)),
        Transition::Fade => (out, Layer::at_alpha(root, p)),
        Transition::Dip => (Layer::at_alpha(root, (1.0 - 2.0 * p).max(0.0)), Layer::at_alpha(root, (2.0 * p - 1.0).max(0.0))),
        Transition::Slide => (Layer::at(root * Affine::translate((-p * width, 0.0))), Layer::at(root * Affine::translate(((1.0 - p) * width, 0.0)))),
        Transition::SlideUp => (Layer::at(root * Affine::translate((0.0, -p * height))), Layer::at(root * Affine::translate((0.0, (1.0 - p) * height)))),
        Transition::SlideRight => (Layer::at(root * Affine::translate((p * width, 0.0))), Layer::at(root * Affine::translate((-(1.0 - p) * width, 0.0)))),
        Transition::SlideDown => (Layer::at(root * Affine::translate((0.0, p * height))), Layer::at(root * Affine::translate((0.0, -(1.0 - p) * height)))),
        Transition::Cover => (out, Layer::at(root * Affine::translate(((1.0 - p) * width, 0.0)))),
        Transition::ZoomOut => {
            let centre = Vec2::new(width / 2.0, height / 2.0);
            let zoom = Affine::translate(centre) * Affine::scale(0.8 + 0.2 * p) * Affine::translate(-centre);
            (out, Layer::at_alpha(root * zoom, p))
        }
        Transition::Wipe => (out, Layer { reveal: p, ..Layer::at(root) }),
        Transition::Zoom => {
            let s = 1.15 - 0.15 * p;
            let centre = Vec2::new(width / 2.0, height / 2.0);
            let zoom = Affine::translate(centre) * Affine::scale(s) * Affine::translate(-centre);
            (out, Layer::at_alpha(root * zoom, p))
        }
        Transition::TurnLeft | Transition::TurnRight => {
            // Two faces of a cube seen square on: the leaving face narrows
            // as the arriving one widens, and the whole cube is scaled to
            // fit the frame. Faces turned away are shorter and darker,
            // which reads as depth.
            let angle = p * std::f64::consts::FRAC_PI_2;
            let (leaving, arriving) = (angle.cos(), angle.sin());
            let fit = 1.0 / (leaving + arriving);
            let cube = Affine::translate((0.0, height * (1.0 - fit) / 2.0)) * Affine::scale(fit);
            let face = |x: f64, across: f64| {
                let tall = 0.85 + 0.15 * across;
                Affine::translate((x, height / 2.0)) * Affine::scale_non_uniform(across.max(1e-6), tall) * Affine::translate((0.0, -height / 2.0))
            };
            let (out_x, in_x) = if kind == Transition::TurnLeft { (0.0, leaving * width) } else { (arriving * width, 0.0) };
            (
                Layer { shade: (1.0 - leaving) * 0.6, ..Layer::at(root * cube * face(out_x, leaving)) },
                Layer { shade: (1.0 - arriving) * 0.6, ..Layer::at(root * cube * face(in_x, arriving)) },
            )
        }
        Transition::Flip => {
            // A card turning over: the old side narrows to an edge, then
            // the new side opens out.
            let across = (p * std::f64::consts::PI).cos();
            let flip = |across: f64| {
                let tall = 0.9 + 0.1 * across;
                let centre = Vec2::new(width / 2.0, height / 2.0);
                Affine::translate(centre) * Affine::scale_non_uniform(across.max(1e-6), tall) * Affine::translate(-centre)
            };
            if across > 0.0 {
                (Layer { shade: (1.0 - across) * 0.6, ..Layer::at(root * flip(across)) }, Layer::at_alpha(root, 0.0))
            } else {
                (Layer::at_alpha(root, 0.0), Layer { shade: (1.0 + across) * 0.6, ..Layer::at(root * flip(-across)) })
            }
        }
    }
}

fn draw_slide(
    scene: &mut Scene,
    layer: Layer,
    page: vello::kurbo::Rect,
    slide: &Slide,
    local: f64,
    media: &MediaLibrary,
    cx: &mut RenderContext,
) {
    if layer.alpha <= 0.0 || layer.reveal <= 0.0 {
        return;
    }
    let shown = vello::kurbo::Rect::new(page.x0, page.y0, page.x0 + page.width() * layer.reveal, page.y1);
    let fade = layer.alpha < 1.0 || layer.reveal < 1.0;
    if fade {
        scene.push_layer(Fill::NonZero, Mix::Normal, layer.alpha as f32, layer.root, &shown);
    }
    scene.fill(Fill::NonZero, layer.root, color(slide.background), None, &page);
    let duration = slide.duration.as_secs_f64();
    for (i, element) in slide.elements.iter().enumerate() {
        let span = slide.element_motion_secs(i);
        let progress = ((local - slide.element_start(i)) / span).clamp(0.0, 1.0);
        let (motion, p) = match element.exit {
            // Leaving plays the movement backwards over the slide's last moments.
            exit if exit != Motion::None && local > duration - span => (exit, element.easing.sample(((duration - local) / span).clamp(0.0, 1.0))),
            _ => (element.motion, element.easing.sample(progress)),
        };
        let photo_progress = ((local - slide.element_start(i)) / (duration - slide.element_start(i)).max(0.001)).clamp(0.0, 1.0);
        draw_element(scene, layer.root, element, motion, (p, photo_progress), media, cx);
    }
    if layer.shade > 0.0 {
        scene.fill(Fill::NonZero, layer.root, peniko::Color::BLACK.with_alpha(layer.shade as f32), None, &page);
    }
    if fade {
        scene.pop_layer();
    }
}

/// The area an element covers, in slide units. Text height comes from its
/// layout, so the box always fits the words.
pub fn element_bounds(element: &Element, text: &mut TextEngine) -> Rect {
    let r = element.frame;
    match &element.kind {
        ElementKind::Text { text: s, .. } => {
            let (_, h) = text.measure(s, &text_style(element));
            Rect { h: f64::from(h), ..r }
        }
        ElementKind::Shape { .. } | ElementKind::Image { .. } => r,
    }
}

fn text_style(element: &Element) -> TextStyle {
    let (size, bold, align) = match &element.kind {
        ElementKind::Text { size, bold, align, .. } => (*size, *bold, *align),
        ElementKind::Shape { .. } | ElementKind::Image { .. } => (0.0, false, TextAlign::Start),
    };
    let align = match align {
        TextAlign::Start => Align::Start,
        TextAlign::Center => Align::Center,
        TextAlign::End => Align::End,
    };
    TextStyle { size: size as f32, bold, align, width: element.frame.w as f32 }
}

fn draw_element(scene: &mut Scene, root: Affine, element: &Element, motion: Motion, (p, photo_progress): (f64, f64), media: &MediaLibrary, cx: &mut RenderContext) {
    let Pose { alpha, offset, scale, across, turn } = pose(motion, p);
    if alpha <= 0.0 || across <= 0.0 {
        return;
    }
    let r = element_bounds(element, &mut cx.text);
    let centre = Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let transform = root
        * Affine::translate(centre + offset)
        * Affine::rotate(turn)
        * Affine::scale_non_uniform(scale * across, scale)
        * Affine::translate(-centre);

    let bounds = rounded(r, 0.0);
    let fade = alpha < 1.0;
    if fade {
        scene.push_layer(Fill::NonZero, Mix::Normal, alpha as f32, transform, &bounds);
    }
    let reveal = matches!(motion, Motion::WipeRight | Motion::WipeLeft | Motion::WipeUp | Motion::WipeDown);
    if reveal {
        let clip = reveal_bounds(r, motion, p);
        scene.push_layer(Fill::NonZero, Mix::Normal, 1.0, transform, &clip);
    }
    match &element.kind {
        ElementKind::Shape { fill, corner_radius, shape } => {
            if *shape == kn_model::ShapeKind::Rectangle {
                scene.fill(Fill::NonZero, transform, color(*fill), None, &rounded(r, *corner_radius));
            } else {
                let mut path = vello::kurbo::BezPath::new();
                for (i, (x, y)) in shape.points().into_iter().enumerate() {
                    let point = (r.x + x * r.w, r.y + y * r.h);
                    if i == 0 { path.move_to(point); } else { path.line_to(point); }
                }
                path.close_path();
                scene.fill(Fill::NonZero, transform, color(*fill), None, &path);
            }
        }
        ElementKind::Text { text: s, color: c, .. } => {
            use kn_text::LetterEffect;
            let effect = match motion {
                Motion::Typewriter => Some(LetterEffect::Typewriter),
                Motion::LetterReveal => Some(LetterEffect::Reveal),
                Motion::LetterFade => Some(LetterEffect::Fade), Motion::LetterRise => Some(LetterEffect::Rise),
                Motion::LetterDrop => Some(LetterEffect::Drop), Motion::LetterPop => Some(LetterEffect::Pop),
                Motion::LetterSpin => Some(LetterEffect::Spin), Motion::LetterWave => Some(LetterEffect::Wave),
                _ => None,
            };
            let transform = transform * Affine::translate((r.x, r.y));
            if let Some(effect) = effect {
                cx.text.draw_letters(scene, transform, s, &text_style(element), color(*c), effect, p);
            } else {
                cx.text.draw(scene, transform, s, &text_style(element), color(*c));
            }
        }
        ElementKind::Image { media: id, corner_radius } => {
            let clip = rounded(r, *corner_radius);
            match cx.image(media, id) {
                Some(img) => {
                    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
                    let (zoom, drift) = element.photo_motion.sample(photo_progress, element.photo_amount);
                    let cover = (r.w / iw).max(r.h / ih) * zoom;
                    let place = Affine::translate((r.x + r.w / 2.0 + drift * r.w, r.y + r.h / 2.0))
                        * Affine::scale(cover)
                        * Affine::translate((-iw / 2.0, -ih / 2.0));
                    scene.push_clip_layer(Fill::NonZero, transform, &clip);
                    scene.draw_image(&ImageBrush::new(img.clone()), transform * place);
                    scene.pop_layer();
                }
                None => scene.fill(Fill::NonZero, transform, color(Color::rgb(200, 200, 205)), None, &clip),
            }
        }
    }
    if reveal { scene.pop_layer(); }
    if fade {
        scene.pop_layer();
    }
}

struct Pose {
    alpha: f64,
    offset: Vec2,
    scale: f64,
    /// Width as seen while turning about the upright middle, 0 to 1.
    across: f64,
    /// Rotation in radians.
    turn: f64,
}

/// Where an element is during its entrance, `p` running from 0 to 1.
fn pose(motion: Motion, p: f64) -> Pose {
    let still = Pose { alpha: 1.0, offset: Vec2::ZERO, scale: 1.0, across: 1.0, turn: 0.0 };
    let to_go = 1.0 - p;
    match motion {
        Motion::None | Motion::LetterReveal | Motion::Typewriter | Motion::LetterFade | Motion::LetterRise | Motion::LetterDrop | Motion::LetterPop | Motion::LetterSpin | Motion::LetterWave | Motion::WipeRight | Motion::WipeLeft | Motion::WipeUp | Motion::WipeDown => still,
        Motion::Bounce => Pose { alpha: p, offset: Vec2::new(0.0, -(p * std::f64::consts::PI * 3.0).sin().abs() * to_go * 90.0), ..still },
        Motion::Fade => Pose { alpha: p, ..still },
        Motion::Rise => Pose { alpha: p, offset: Vec2::new(0.0, to_go * 80.0), ..still },
        Motion::Drop => Pose { alpha: p, offset: Vec2::new(0.0, -to_go * 80.0), ..still },
        Motion::FromLeft => Pose { alpha: p, offset: Vec2::new(-to_go * 200.0, 0.0), ..still },
        Motion::FromRight => Pose { alpha: p, offset: Vec2::new(to_go * 200.0, 0.0), ..still },
        Motion::Pop => Pose { alpha: p, scale: 0.8 + 0.2 * p, ..still },
        Motion::Zoom => Pose { alpha: p, scale: 1.0 + 0.5 * to_go, ..still },
        // Starts edge-on, so it seems to turn in from the side.
        Motion::Flip => Pose { alpha: p, across: (to_go * std::f64::consts::FRAC_PI_2).cos(), ..still },
        Motion::Spin => Pose { alpha: p, scale: 0.5 + 0.5 * p, turn: -to_go * std::f64::consts::PI, ..still },
    }
}

fn ease_in_out_cubic(p: f64) -> f64 {
    if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 }
}

fn rounded(r: Rect, radius: f64) -> RoundedRect {
    RoundedRect::new(r.x, r.y, r.x + r.w, r.y + r.h, radius)
}

fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_bounds_follow_layout() {
        let mut text = TextEngine::new();
        let mut e = Element::text("One line", Rect::new(0.0, 0.0, 1600.0, 1.0), 100.0, false, Color::rgb(0, 0, 0));
        let one = element_bounds(&e, &mut text).h;
        assert!(one > 100.0, "line height should exceed font size, got {one}");
        if let ElementKind::Text { text: t, .. } = &mut e.kind {
            *t = "One line\nTwo lines".into();
        }
        assert!(element_bounds(&e, &mut text).h > one * 1.8);
    }

    #[test]
    fn transitions_end_with_only_the_new_slide_in_place() {
        let off = |a: Affine| a.as_coeffs().iter().zip(Affine::IDENTITY.as_coeffs()).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        for kind in Transition::ALL {
            let (out, inc) = transition_layers(kind, 1.0, Affine::IDENTITY, (1920.0, 1080.0));
            assert!(off(inc.root) < 1e-9 && inc.alpha == 1.0 && inc.reveal == 1.0 && inc.shade < 1e-9, "{kind:?} ends at {inc:?}");
            // And every one starts with the old slide exactly in place.
            let (start, _) = transition_layers(kind, 0.0, Affine::IDENTITY, (1920.0, 1080.0));
            assert!(off(start.root) < 1e-9 && start.alpha == 1.0 && start.shade < 1e-9, "{kind:?} starts at {start:?}");
            let _ = out;
        }
        assert_eq!(ease_in_out_cubic(0.0), 0.0);
        assert_eq!(ease_in_out_cubic(1.0), 1.0);
    }

    #[test]
    fn entrance_ends_at_rest() {
        for m in Motion::ALL {
            let p = pose(m, 1.0);
            assert_eq!((p.alpha, p.offset, p.scale, p.across, p.turn), (1.0, Vec2::ZERO, 1.0, 1.0, 0.0), "{m:?}");
        }
    }

    #[test]
    fn renders_every_frame_without_panicking() {
        let mut project = Project::sample();
        // Every transition and movement, coming and going.
        for (i, slide) in project.slides.iter_mut().enumerate().skip(1) {
            slide.transition = Transition::ALL[i % Transition::ALL.len()];
        }
        for (i, e) in project.slides.iter_mut().flat_map(|s| &mut s.elements).enumerate() {
            e.motion = Motion::ALL[i % Motion::ALL.len()];
            e.exit = Motion::ALL[(i + 3) % Motion::ALL.len()];
        }
        let rate = kn_time::FrameRate::FPS_30;
        let mut cx = RenderContext::new();
        let media = MediaLibrary::new();
        for n in 0..rate.frames_in(project.total_duration()) {
            let mut scene = Scene::new();
            render_frame(&project, &media, rate.frame_start(n), 0.5, &mut scene, &mut cx);
        }
    }

    #[test]
    fn pictures_decode_once_and_missing_ones_draw_a_placeholder() {
        let mut media = MediaLibrary::new();
        let info = media.import(kn_media::test_png(8, 4)).unwrap();
        let mut project = Project::sample();
        let frame = Rect::new(0.0, 0.0, 400.0, 400.0);
        project.slides[0].elements = vec![Element::image(info.id.clone(), frame), Element::image("missing.png".into(), frame)];
        let mut cx = RenderContext::new();
        for _ in 0..3 {
            render_frame(&project, &media, Tick::from_secs(2), 1.0, &mut Scene::new(), &mut cx);
        }
        assert!(cx.images[&info.id].is_some());
        assert!(cx.images["missing.png"].is_none());
    }
}

fn reveal_bounds(r: Rect, motion: Motion, p: f64) -> vello::kurbo::Rect {
    let p = p.clamp(0.0, 1.0);
    let (x0,y0,x1,y1) = match motion {
        Motion::WipeLeft => (r.x+r.w*(1.0-p), r.y, r.x+r.w, r.y+r.h),
        Motion::WipeUp => (r.x, r.y+r.h*(1.0-p), r.x+r.w, r.y+r.h),
        Motion::WipeDown => (r.x, r.y, r.x+r.w, r.y+r.h*p),
        _ => (r.x, r.y, r.x+r.w*p, r.y+r.h),
    };
    vello::kurbo::Rect::new(x0,y0,x1,y1)
}

#[cfg(test)]
mod motion_render_tests {
    use super::*;
    #[test]
    fn all_motion_styles_render_entrances_and_exits_with_unicode() {
        let mut project = Project::sample();
        project.slides.truncate(1);
        project.slides[0].elements = vec![Element::text("Modern office\nשלום Cafe\u{301}", Rect::new(200.,200.,1000.,200.), 72., true, Color::rgb(0,0,0))];
        project.slides[0].entrance = kn_model::Entrance::Together;
        let mut cx = RenderContext::new();
        let media = MediaLibrary::new();
        for motion in Motion::ALL {
            project.slides[0].elements[0].motion = motion;
            project.slides[0].elements[0].exit = motion;
            for ms in [0,1,150,300,590,600,1500,2400,2700,2999] {
                render_frame(&project,&media,Tick::from_millis(ms),0.5,&mut Scene::new(),&mut cx);
            }
        }
    }
    #[test]
    fn directional_reveal_bounds_start_empty_and_finish_at_full_frame() {
        let r = Rect::new(10.,20.,400.,100.);
        for motion in [Motion::WipeRight,Motion::WipeLeft,Motion::WipeUp,Motion::WipeDown] {
            let start = reveal_bounds(r,motion,0.0);
            assert_eq!(start.width()*start.height(),0.0);
            let end = reveal_bounds(r,motion,1.0);
            assert_eq!(end,vello::kurbo::Rect::new(10.,20.,410.,120.));
        }
    }
}
