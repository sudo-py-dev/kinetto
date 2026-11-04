//! How a new video moves, chosen before it is made: what each look does,
//! one animation for everything, or a different one on every slide.

use kn_model::{ElementKind, Motion, PhotoMotion, Project, Transition};
use serde::{Deserialize, Serialize};

/// A choice for the whole video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Pick<T> {
    /// What the look and each layout do.
    #[default]
    Look,
    /// Something different on every slide.
    Random,
    /// The same everywhere.
    Every(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// How items arrive.
    pub motion: Pick<Motion>,
    /// How one slide gives way to the next.
    pub transition: Pick<Transition>,
    /// How items leave at the end of each slide; `Look` leaves them in place.
    pub exit: Pick<Motion>,
    /// Continuous camera movement, separate from entrances and exits.
    pub photo_motion: Pick<PhotoMotion>,
    /// Camera travel as a percentage of the image frame.
    pub photo_amount_percent: u8,
}

impl Default for Style {
    fn default() -> Self {
        Self { motion: Pick::Look, transition: Pick::Look, exit: Pick::Look, photo_motion: Pick::Look, photo_amount_percent: 12 }
    }
}

/// A small, fast generator (SplitMix64); good enough to vary a video.
#[derive(Debug, Clone)]
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// A different seed every run.
    pub fn from_clock() -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Self(nanos)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// An index below `n` (which must not be zero).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// One of `options`, never the same as `last` when there is a choice.
    fn pick_new<T: Copy + PartialEq>(&mut self, options: &[T], last: Option<T>) -> T {
        loop {
            let t = options[self.below(options.len())];
            if options.len() < 2 || Some(t) != last {
                return t;
            }
        }
    }
}

/// Applies `style` to every slide of `project`. The first slide always
/// starts with a cut, and items keep their order of arrival.
pub fn animate(project: &mut Project, style: Style, dice: &mut Dice) {
    // Everything that moves, but not "None" or "Cut".
    let moving: Vec<_> = Motion::ALL[1..].iter().copied().filter(|m| !m.text_only()).collect();
    let changing = &Transition::ALL[1..];
    let (mut last_motion, mut last_transition, mut last_exit) = (None, None, None);
    let mut last_photo = None;
    for (i, slide) in project.slides.iter_mut().enumerate() {
        for element in &mut slide.elements {
            if !matches!(element.kind, ElementKind::Image { .. }) { continue; }
            let motion = match style.photo_motion {
                Pick::Look => continue,
                Pick::Every(motion) => motion,
                Pick::Random => dice.pick_new(&PhotoMotion::ALL[1..], last_photo),
            };
            last_photo = Some(motion);
            element.photo_motion = motion;
            element.photo_amount = f64::from(style.photo_amount_percent.min(40)) / 100.0;
        }
        match style.exit {
            Pick::Look => {}
            Pick::Random => {
                let m = dice.pick_new(&moving, last_exit);
                last_exit = Some(m);
                slide.elements.iter_mut().for_each(|e| apply_motion(e, m, true));
            }
            Pick::Every(m) => slide.elements.iter_mut().for_each(|e| apply_motion(e, m, true)),
        }
        match style.motion {
            Pick::Look => {}
            // One motion per slide keeps each slide calm while the video varies.
            Pick::Random => {
                let m = dice.pick_new(&moving, last_motion);
                last_motion = Some(m);
                slide.elements.iter_mut().for_each(|e| apply_motion(e, m, false));
            }
            Pick::Every(m) => slide.elements.iter_mut().for_each(|e| apply_motion(e, m, false)),
        }
        if i == 0 {
            slide.transition = Transition::Cut;
            continue;
        }
        match style.transition {
            Pick::Look => {}
            Pick::Random => {
                let t = dice.pick_new(changing, last_transition);
                last_transition = Some(t);
                slide.transition = t;
            }
            Pick::Every(t) => slide.transition = t,
        }
    }
}

fn apply_motion(element: &mut kn_model::Element, motion: Motion, exit: bool) {
    let motion = if motion.text_only() && !matches!(element.kind, kn_model::ElementKind::Text { .. }) { Motion::Fade } else { motion };
    if exit { element.exit = motion; } else { element.motion = motion; }
    if motion.text_only() {
        element.motion_duration_ms = 1800;
        element.easing = kn_model::Easing::Linear;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        let theme = kn_layout::theme("Paper");
        let outline = crate::parse_plain("Title\nSub\n\nA\nB\n\nC\n- x\n- y\n- z\n\nD\n\nE\n\nF");
        crate::build(&outline, kn_geom::Aspect::Wide16x9, theme).unwrap()
    }

    #[test]
    fn camera_choices_apply_only_to_photos_and_clamp_intensity() {
        let mut p = project();
        let text = p.slides[0].elements.clone();
        for _ in 0..4 {
            p.slides[0].elements.push(kn_model::Element::image("photo".into(), kn_geom::Rect::new(0., 0., 100., 100.)));
        }
        animate(&mut p, Style { photo_motion: Pick::Every(PhotoMotion::ZoomOut), photo_amount_percent: 24, ..Style::default() }, &mut Dice::new(7));
        assert_eq!(&p.slides[0].elements[..text.len()], text.as_slice());
        for e in &p.slides[0].elements[text.len()..] {
            assert_eq!(e.photo_motion, PhotoMotion::ZoomOut);
            assert_eq!(e.photo_amount, 0.24);
        }
        animate(&mut p, Style { photo_motion: Pick::Random, photo_amount_percent: 255, ..Style::default() }, &mut Dice::new(7));
        let photos = &p.slides[0].elements[text.len()..];
        assert!(photos.windows(2).all(|w| w[0].photo_motion != w[1].photo_motion));
        assert!(photos.iter().all(|e| e.photo_motion != PhotoMotion::None && e.photo_amount == 0.4));
    }

    #[test]
    fn the_look_is_left_alone() {
        let before = project();
        let mut after = before.clone();
        animate(&mut after, Style::default(), &mut Dice::new(1));
        assert_eq!(before, after);
    }

    #[test]
    fn letter_effects_apply_to_text_with_readable_timing() {
        let mut p = project();
        animate(&mut p, Style { motion: Pick::Every(Motion::LetterReveal), ..Style::default() }, &mut Dice::new(1));
        for element in p.slides.iter().flat_map(|s| &s.elements) {
            if matches!(element.kind, kn_model::ElementKind::Text { .. }) {
                assert_eq!(element.motion, Motion::LetterReveal);
                assert_eq!(element.motion_duration_ms, 1800);
            } else { assert_eq!(element.motion, Motion::Fade); }
        }
    }

    #[test]
    fn one_choice_goes_everywhere_but_the_first_cut() {
        let mut p = project();
        animate(&mut p, Style { motion: Pick::Every(Motion::Pop), transition: Pick::Every(Transition::Zoom), exit: Pick::Every(Motion::Fade), ..Style::default() }, &mut Dice::new(1));
        assert!(p.slides.iter().flat_map(|s| &s.elements).all(|e| e.exit == Motion::Fade));
        assert!(p.slides.iter().flat_map(|s| &s.elements).all(|e| e.motion == Motion::Pop));
        assert_eq!(p.slides[0].transition, Transition::Cut);
        assert!(p.slides[1..].iter().all(|s| s.transition == Transition::Zoom));
    }

    #[test]
    fn random_moves_and_never_repeats_back_to_back() {
        let mut p = project();
        animate(&mut p, Style { motion: Pick::Random, transition: Pick::Random, exit: Pick::Look, ..Style::default() }, &mut Dice::new(7));
        for pair in p.slides[1..].windows(2) {
            assert_ne!(pair[0].transition, pair[1].transition);
            assert_ne!(pair[0].transition, Transition::Cut);
        }
        for s in &p.slides {
            let first = s.elements[0].motion;
            assert_ne!(first, Motion::None);
            assert!(s.elements.iter().all(|e| e.motion == first));
        }
    }
}
