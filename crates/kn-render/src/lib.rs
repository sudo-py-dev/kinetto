//! Turns a project at a point in time into a vello [`Scene`]. Preview and
//! export both call [`render_frame`], so what you see is what you export.

use kn_geom::{Color, Rect};
use kn_media::MediaLibrary;
use kn_model::{Element, ElementKind, Motion, Project, TextAlign};
use kn_text::{Align, TextEngine, TextStyle};
use kn_time::Tick;
use std::collections::HashMap;
use vello::Scene;
use vello::kurbo::{Affine, RoundedRect, Vec2};
use vello::peniko::{self, Blob, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, Mix};

/// How long an entrance animation takes, and the gap between elements.
const ENTRANCE: f64 = 0.6;
const STAGGER: f64 = 0.15;

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

/// Time into a slide at which every entrance has finished.
pub fn rest_time(element_count: usize) -> Tick {
    let secs = element_count.saturating_sub(1) as f64 * STAGGER + ENTRANCE;
    Tick::from_millis((secs * 1000.0).ceil() as i64)
}

/// Draws the frame at `t` into `scene`, scaled from slide units by `scale`.
pub fn render_frame(project: &Project, media: &MediaLibrary, t: Tick, scale: f64, scene: &mut Scene, cx: &mut RenderContext) {
    let Some((index, local)) = project.slide_at(t) else { return };
    let slide = &project.slides[index];
    let (w, h) = project.aspect.canvas_size();
    let root = Affine::scale(scale);

    scene.fill(Fill::NonZero, root, color(slide.background), None, &vello::kurbo::Rect::new(0.0, 0.0, w, h));

    let local = local.as_secs_f64();
    for (i, element) in slide.elements.iter().enumerate() {
        let progress = ((local - i as f64 * STAGGER) / ENTRANCE).clamp(0.0, 1.0);
        draw_element(scene, root, element, ease_out_cubic(progress), media, cx);
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

fn draw_element(scene: &mut Scene, root: Affine, element: &Element, p: f64, media: &MediaLibrary, cx: &mut RenderContext) {
    let Pose { alpha, offset, scale } = pose(element.motion, p);
    if alpha <= 0.0 {
        return;
    }
    let r = element_bounds(element, &mut cx.text);
    let centre = Vec2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let transform = root
        * Affine::translate(centre + offset)
        * Affine::scale(scale)
        * Affine::translate(-centre);

    let bounds = rounded(r, 0.0);
    let fade = alpha < 1.0;
    if fade {
        scene.push_layer(Fill::NonZero, Mix::Normal, alpha as f32, transform, &bounds);
    }
    match &element.kind {
        ElementKind::Shape { fill, corner_radius } => {
            scene.fill(Fill::NonZero, transform, color(*fill), None, &rounded(r, *corner_radius));
        }
        ElementKind::Text { text: s, color: c, .. } => {
            cx.text.draw(scene, transform * Affine::translate((r.x, r.y)), s, &text_style(element), color(*c));
        }
        ElementKind::Image { media: id, corner_radius } => {
            let clip = rounded(r, *corner_radius);
            match cx.image(media, id) {
                Some(img) => {
                    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
                    let cover = (r.w / iw).max(r.h / ih);
                    let place = Affine::translate((r.x + r.w / 2.0, r.y + r.h / 2.0))
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
    if fade {
        scene.pop_layer();
    }
}

struct Pose {
    alpha: f64,
    offset: Vec2,
    scale: f64,
}

/// Where an element is during its entrance, `p` running from 0 to 1.
fn pose(motion: Motion, p: f64) -> Pose {
    let still = Pose { alpha: 1.0, offset: Vec2::ZERO, scale: 1.0 };
    match motion {
        Motion::None => still,
        Motion::Fade => Pose { alpha: p, ..still },
        Motion::Rise => Pose { alpha: p, offset: Vec2::new(0.0, (1.0 - p) * 80.0), ..still },
        Motion::Pop => Pose { alpha: p, scale: 0.8 + 0.2 * p, ..still },
    }
}

fn ease_out_cubic(p: f64) -> f64 {
    1.0 - (1.0 - p).powi(3)
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
    fn entrance_ends_at_rest() {
        for m in [Motion::None, Motion::Fade, Motion::Rise, Motion::Pop] {
            let p = pose(m, 1.0);
            assert_eq!((p.alpha, p.offset, p.scale), (1.0, Vec2::ZERO, 1.0), "{m:?}");
        }
    }

    #[test]
    fn renders_every_frame_without_panicking() {
        let project = Project::sample();
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
