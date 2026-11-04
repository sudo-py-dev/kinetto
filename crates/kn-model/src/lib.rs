//! The document model: a project is an ordered list of slides, each holding
//! elements. Pure data, serializable, no UI or GPU types.

use kn_geom::{Aspect, Color, Rect};
use kn_time::Tick;
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "kinetto.project";
/// 2: image elements; projects are saved as zip bundles with their media.
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub aspect: Aspect,
    pub slides: Vec<Slide>,
    /// Name of the look applied last; new slides and items follow it.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Background music for the whole video.
    #[serde(default)]
    pub music: Option<Music>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Music {
    pub source: MusicSource,
    /// 0 to 1; music is also lowered automatically under voice.
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MusicSource {
    /// Made by Kinetto, free to use anywhere.
    Mood(Mood),
    /// An audio file the user added.
    File(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mood {
    Calm,
    Upbeat,
    Epic,
}

impl Mood {
    pub const ALL: [Mood; 3] = [Mood::Calm, Mood::Upbeat, Mood::Epic];

    pub fn name(self) -> &'static str {
        match self {
            Mood::Calm => "Calm",
            Mood::Upbeat => "Upbeat",
            Mood::Epic => "Epic",
        }
    }
}

fn default_theme() -> String {
    "Paper".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    pub background: Color,
    pub duration: Tick,
    pub elements: Vec<Element>,
    /// How this slide replaces the one before it.
    #[serde(default)]
    pub transition: Transition,
    /// Uses the theme's contrasting background, for section breaks.
    #[serde(default)]
    pub dark: bool,
    /// Recorded or generated speech, starting with the slide.
    #[serde(default)]
    pub voice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Transition {
    #[default]
    Cut,
    Fade,
    Slide,
    Zoom,
}

impl Transition {
    pub const ALL: [Transition; 4] = [Transition::Cut, Transition::Fade, Transition::Slide, Transition::Zoom];

    pub fn name(self) -> &'static str {
        match self {
            Transition::Cut => "Cut",
            Transition::Fade => "Fade",
            Transition::Slide => "Slide",
            Transition::Zoom => "Zoom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub frame: Rect,
    pub kind: ElementKind,
    pub motion: Motion,
    /// What the item is for, so a new look can restyle it sensibly.
    #[serde(default)]
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Role {
    /// Placed by hand; a new look leaves its colours alone.
    #[default]
    Free,
    Title,
    Body,
    Caption,
    /// A coloured card using accent colour `n` of the theme.
    Card(u8),
    /// Text sitting on a card.
    OnCard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementKind {
    Shape { fill: Color, corner_radius: f64 },
    Text { text: String, color: Color, size: f64, bold: bool, align: TextAlign },
    /// A picture filling its frame, cropped rather than stretched.
    Image { media: String, corner_radius: f64 },
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
            role: Role::Free,
        }
    }

    pub fn image(media: String, frame: Rect) -> Self {
        Element { frame, kind: ElementKind::Image { media, corner_radius: 0.0 }, motion: Motion::Fade, role: Role::Free }
    }

    pub fn shape(frame: Rect, fill: Color) -> Self {
        Element { frame, kind: ElementKind::Shape { fill, corner_radius: 24.0 }, motion: Motion::Pop, role: Role::Free }
    }

    pub fn with_role(self, role: Role) -> Self {
        Element { role, ..self }
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
            theme: default_theme(),
            music: None,
            slides: vec![
                Slide {
                    background: paper,
                    duration: Tick::from_secs(3),
                    transition: Transition::Cut,
                    dark: false,
                    voice: None,
                    elements: vec![
                        Element::shape(Rect::new(760.0, 260.0, 400.0, 220.0), accent),
                        Element::text("Welcome to Kinetto", Rect::new(160.0, 560.0, 1600.0, 120.0), 110.0, true, ink),
                        Element::text("Slides in, video out.", Rect::new(160.0, 720.0, 1600.0, 70.0), 56.0, false, Color::rgb(110, 110, 120)),
                    ],
                },
                Slide {
                    background: ink,
                    duration: Tick::from_secs(4),
                    transition: Transition::Slide,
                    dark: true,
                    voice: None,
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

    /// Every picture and sound the project uses.
    pub fn media_ids(&self) -> impl Iterator<Item = &str> {
        let pictures = self.slides.iter().flat_map(|s| &s.elements).filter_map(|e| match &e.kind {
            ElementKind::Image { media, .. } => Some(media.as_str()),
            _ => None,
        });
        let voices = self.slides.iter().filter_map(|s| s.voice.as_deref());
        let music = self.music.iter().filter_map(|m| match &m.source {
            MusicSource::File(id) => Some(id.as_str()),
            MusicSource::Mood(_) => None,
        });
        pictures.chain(voices).chain(music)
    }

    pub fn blank_slide(&self) -> Slide {
        Slide {
            background: Color::rgb(250, 247, 242),
            duration: Tick::from_secs(3),
            elements: Vec::new(),
            transition: Transition::Fade,
            dark: false,
            voice: None,
        }
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
