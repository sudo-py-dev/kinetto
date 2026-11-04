//! The document model: a project is an ordered list of slides, each holding
//! elements. Pure data, serializable, no UI or GPU types.

use kn_geom::{Aspect, Color, Rect};
use kn_time::Tick;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "kinetto.project";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub aspect: Aspect,
    pub slides: Vec<Slide>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    pub background: Color,
    pub duration: Tick,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub frame: Rect,
    pub kind: ElementKind,
    pub motion: Motion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementKind {
    Shape { fill: Color, corner_radius: f64 },
    Text { text: String, color: Color, size: f64, bold: bool, align: TextAlign },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

impl Element {
    pub fn text(text: &str, frame: Rect, size: f64, bold: bool, color: Color) -> Self {
        Element {
            frame,
            kind: ElementKind::Text { text: text.into(), color, size, bold, align: TextAlign::Center },
            motion: Motion::Rise,
        }
    }

    pub fn shape(frame: Rect, fill: Color) -> Self {
        Element { frame, kind: ElementKind::Shape { fill, corner_radius: 24.0 }, motion: Motion::Pop }
    }
}

/// Entrance animations, named in plain words. Durations come from the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Motion {
    #[default]
    None,
    Fade,
    Rise,
    Pop,
}

impl Project {
    pub fn total_duration(&self) -> Tick {
        self.slides.iter().fold(Tick::ZERO, |acc, s| acc + s.duration)
    }

    /// The slide showing at time `t` and the time local to that slide.
    pub fn slide_at(&self, t: Tick) -> Option<(usize, Tick)> {
        let mut start = Tick::ZERO;
        for (i, slide) in self.slides.iter().enumerate() {
            let end = start + slide.duration;
            if t < end {
                return Some((i, t - start));
            }
            start = end;
        }
        None
    }

    /// A small starter deck so a new project is never empty.
    pub fn sample() -> Self {
        let accent = Color::rgb(255, 94, 87);
        let ink = Color::rgb(24, 24, 32);
        let paper = Color::rgb(250, 247, 242);
        let point = |y, fill, label: &str| {
            [
                Element::shape(Rect::new(360.0, y, 1200.0, 160.0), fill),
                Element { motion: Motion::Fade, ..Element::text(label, Rect::new(360.0, y + 50.0, 1200.0, 60.0), 56.0, true, ink) },
            ]
        };
        Project {
            aspect: Aspect::Wide16x9,
            slides: vec![
                Slide {
                    background: paper,
                    duration: Tick::from_secs(3),
                    elements: vec![
                        Element::shape(Rect::new(760.0, 260.0, 400.0, 220.0), accent),
                        Element::text("Welcome to Kinetto", Rect::new(160.0, 560.0, 1600.0, 120.0), 110.0, true, ink),
                        Element::text("Slides in, video out.", Rect::new(160.0, 720.0, 1600.0, 70.0), 56.0, false, Color::rgb(110, 110, 120)),
                    ],
                },
                Slide {
                    background: ink,
                    duration: Tick::from_secs(4),
                    elements: [
                        vec![Element::text("Three easy steps", Rect::new(160.0, 90.0, 1600.0, 110.0), 90.0, true, paper)],
                        point(290.0, accent, "Type your story").to_vec(),
                        point(510.0, Color::rgb(87, 199, 255), "Pick a look").to_vec(),
                        point(730.0, Color::rgb(255, 214, 87), "Press Make Video").to_vec(),
                    ]
                    .concat(),
                },
            ],
        }
    }

    pub fn blank_slide(&self) -> Slide {
        Slide { background: Color::rgb(250, 247, 242), duration: Tick::from_secs(3), elements: Vec::new() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("not a Kinetto project")]
    WrongFormat,
    #[error("project was made by a newer Kinetto (schema {0}); please update")]
    TooNew(u32),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Serialize, Deserialize)]
struct Envelope<P> {
    format: String,
    schema_version: u32,
    project: P,
}

pub fn to_json(project: &Project) -> serde_json::Result<String> {
    serde_json::to_string(&Envelope { format: FORMAT.into(), schema_version: SCHEMA_VERSION, project })
}

pub fn from_json(json: &str) -> Result<Project, LoadError> {
    let env: Envelope<serde_json::Value> = serde_json::from_str(json)?;
    if env.format != FORMAT {
        return Err(LoadError::WrongFormat);
    }
    if env.schema_version > SCHEMA_VERSION {
        return Err(LoadError::TooNew(env.schema_version));
    }
    // Migrations from older schema versions run here, in order.
    Ok(serde_json::from_value(env.project)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let p = Project::sample();
        assert_eq!(from_json(&to_json(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn refuses_newer_schema() {
        let json = r#"{"format":"kinetto.project","schema_version":999,"project":{}}"#;
        assert!(matches!(from_json(json), Err(LoadError::TooNew(999))));
    }

    #[test]
    fn slide_lookup() {
        let p = Project::sample();
        assert_eq!(p.slide_at(Tick::from_secs(4)), Some((1, Tick::from_secs(1))));
        assert_eq!(p.slide_at(p.total_duration()), None);
    }
}
