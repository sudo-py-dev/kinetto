//! How a new video moves, chosen before it is made: what each look does,
//! one animation for everything, or a different one on every slide.

use kn_model::{Motion, Project, Transition};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// How items arrive.
    pub motion: Pick<Motion>,
    /// How one slide gives way to the next.
    pub transition: Pick<Transition>,
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
    const MOVING: [Motion; 3] = [Motion::Fade, Motion::Rise, Motion::Pop];
    const CHANGING: [Transition; 3] = [Transition::Fade, Transition::Slide, Transition::Zoom];
    let (mut last_motion, mut last_transition) = (None, None);
    for (i, slide) in project.slides.iter_mut().enumerate() {
        match style.motion {
            Pick::Look => {}
            // One motion per slide keeps each slide calm while the video varies.
            Pick::Random => {
                let m = dice.pick_new(&MOVING, last_motion);
                last_motion = Some(m);
                slide.elements.iter_mut().for_each(|e| e.motion = m);
            }
            Pick::Every(m) => slide.elements.iter_mut().for_each(|e| e.motion = m),
        }
        if i == 0 {
            slide.transition = Transition::Cut;
            continue;
        }
        match style.transition {
            Pick::Look => {}
            Pick::Random => {
                let t = dice.pick_new(&CHANGING, last_transition);
                last_transition = Some(t);
                slide.transition = t;
            }
            Pick::Every(t) => slide.transition = t,
        }
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
    fn the_look_is_left_alone() {
        let before = project();
        let mut after = before.clone();
        animate(&mut after, Style::default(), &mut Dice::new(1));
        assert_eq!(before, after);
    }

    #[test]
    fn one_choice_goes_everywhere_but_the_first_cut() {
        let mut p = project();
        animate(&mut p, Style { motion: Pick::Every(Motion::Pop), transition: Pick::Every(Transition::Zoom) }, &mut Dice::new(1));
        assert!(p.slides.iter().flat_map(|s| &s.elements).all(|e| e.motion == Motion::Pop));
        assert_eq!(p.slides[0].transition, Transition::Cut);
        assert!(p.slides[1..].iter().all(|s| s.transition == Transition::Zoom));
    }

    #[test]
    fn random_moves_and_never_repeats_back_to_back() {
        let mut p = project();
        animate(&mut p, Style { motion: Pick::Random, transition: Pick::Random }, &mut Dice::new(7));
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
