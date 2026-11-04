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
    /// When the music begins in the video, in milliseconds.
    #[serde(default)]
    pub start_ms: i64,
    /// When it stops; `None` plays until the video ends.
    #[serde(default)]
    pub end_ms: Option<i64>,
    /// How far into the song it begins (skipping an intro), in milliseconds.
    #[serde(default)]
    pub offset_ms: i64,
    /// Repeat the song if it ends before the music should.
    #[serde(default = "yes")]
    pub looped: bool,
}

fn yes() -> bool {
    true
}

/// The shortest stretch of music that can be placed.
pub const MIN_MUSIC_MS: i64 = 500;

impl Music {
    /// Music across the whole video, from the start of the song.
    pub fn new(source: MusicSource, volume: f32) -> Self {
        Music { source, volume, start_ms: 0, end_ms: None, offset_ms: 0, looped: true }
    }

    /// Where the music plays in a video of length `total`, kept inside it
    /// and never shorter than [`MIN_MUSIC_MS`].
    pub fn span(&self, total: Tick) -> (Tick, Tick) {
        let total_ms = (total.as_secs_f64() * 1000.0).round() as i64;
        let start = self.start_ms.clamp(0, (total_ms - MIN_MUSIC_MS).max(0));
        let end = self.end_ms.unwrap_or(total_ms).clamp((start + MIN_MUSIC_MS).min(total_ms), total_ms);
        (Tick::from_millis(start), Tick::from_millis(end))
    }
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
    /// Whether items arrive one after another or together.
    #[serde(default)]
    pub entrance: Entrance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Entrance {
    #[default]
    OneByOne,
    Together,
}

/// How long an item's entrance animation takes, in seconds.
pub const ENTRANCE_SECS: f64 = 0.6;
/// The gap between items arriving one by one, in seconds.
pub const STAGGER_SECS: f64 = 0.15;

impl Slide {
    /// When item `i` starts arriving, in seconds into the slide: its own
    /// start time if set, otherwise from the slide's entrance style.
    pub fn element_start(&self, i: usize) -> f64 {
        match self.elements.get(i).and_then(|e| e.start_ms) {
            Some(ms) => ms.max(0) as f64 / 1000.0,
            None => match self.entrance {
                Entrance::OneByOne => i as f64 * STAGGER_SECS,
                Entrance::Together => 0.0,
            },
        }
    }

    /// The moment every item has arrived (capped to the slide's length):
    /// how the slide looks "at rest", for editing, thumbnails and PDFs.
    pub fn rest_time(&self) -> Tick {
        let last = (0..self.elements.len()).map(|i| self.element_start(i)).fold(0.0, f64::max);
        let rest = Tick::from_millis(((last + ENTRANCE_SECS) * 1000.0).ceil() as i64);
        rest.min(self.duration - Tick(1))
    }
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
    /// When it starts arriving, in milliseconds into the slide; `None`
    /// follows the slide's entrance style.
    #[serde(default)]
    pub start_ms: Option<i64>,
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
            start_ms: None,
        }
    }

    pub fn image(media: String, frame: Rect) -> Self {
        Element { frame, kind: ElementKind::Image { media, corner_radius: 0.0 }, motion: Motion::Fade, role: Role::Free, start_ms: None }
    }

    pub fn shape(frame: Rect, fill: Color) -> Self {
        Element { frame, kind: ElementKind::Shape { fill, corner_radius: 24.0 }, motion: Motion::Pop, role: Role::Free, start_ms: None }
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

impl Motion {
    pub const ALL: [Motion; 4] = [Motion::None, Motion::Fade, Motion::Rise, Motion::Pop];

    pub fn name(self) -> &'static str {
        match self {
            Motion::None => "None",
            Motion::Fade => "Fade",
            Motion::Rise => "Rise",
            Motion::Pop => "Pop",
        }
    }
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
                    entrance: Entrance::OneByOne,
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
                    entrance: Entrance::OneByOne,
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
            entrance: Entrance::OneByOne,
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
    fn entrance_timing() {
        let mut s = Project::sample().slides[1].clone();
        assert_eq!(s.element_start(2), 2.0 * STAGGER_SECS);
        s.entrance = Entrance::Together;
        assert_eq!(s.element_start(2), 0.0);
        s.elements[1].start_ms = Some(2500);
        assert_eq!(s.element_start(1), 2.5);
        assert_eq!(s.rest_time(), Tick::from_millis(3100));
        s.elements[1].start_ms = Some(60_000);
        assert_eq!(s.rest_time(), s.duration - Tick(1), "rest never passes the slide's end");
    }

    #[test]
    fn music_span_stays_inside_the_video() {
        let total = Tick::from_secs(10);
        let mut m = Music::new(MusicSource::Mood(Mood::Calm), 1.0);
        assert_eq!(m.span(total), (Tick::ZERO, total));
        m.start_ms = 3000;
        m.end_ms = Some(7000);
        assert_eq!(m.span(total), (Tick::from_secs(3), Tick::from_secs(7)));
        m.start_ms = 99_000;
        m.end_ms = Some(1);
        let (s, e) = m.span(total);
        assert_eq!((s, e), (Tick::from_millis(9500), total), "clamped inside, never shorter than the minimum");
        // Older files without the new fields still read.
        let old: Music = serde_json::from_str(r#"{"source":{"Mood":"Calm"},"volume":0.5}"#).unwrap();
        assert_eq!(old, Music::new(MusicSource::Mood(Mood::Calm), 0.5));
    }

    #[test]
    fn slide_lookup() {
        let p = Project::sample();
        assert_eq!(p.slide_at(Tick::from_secs(4)), Some((1, Tick::from_secs(1))));
        assert_eq!(p.slide_at(p.total_duration()), None);
    }
}
