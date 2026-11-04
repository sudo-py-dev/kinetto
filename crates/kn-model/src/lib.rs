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

/// Default entrance duration, also the time reserved for slide transitions.
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

    /// Fit both effects into the available time, so short slides still settle.
    pub fn element_motion_secs(&self, index: usize) -> f64 {
        let Some(element) = self.elements.get(index) else { return ENTRANCE_SECS };
        let available = (self.duration.as_secs_f64() - self.element_start(index)).max(0.001);
        let effects = if element.motion != Motion::None && element.exit != Motion::None { 2.0 } else { 1.0 };
        let span = element.motion_secs().min(available / effects);
        if element.exit != Motion::None {
            span.min((self.duration.as_secs_f64() - ENTRANCE_SECS).max(0.001))
        } else { span }
    }

    /// The moment every item has arrived (capped to the slide's length):
    /// how the slide looks "at rest", for editing, thumbnails and PDFs.
    pub fn rest_time(&self) -> Tick {
        let last = self.elements.iter().enumerate().map(|(i, e)| self.element_start(i) + if e.motion == Motion::None { 0.0 } else { self.element_motion_secs(i) }).fold(0.0, f64::max);
        let rest = Tick::from_millis((last.max(ENTRANCE_SECS) * 1000.0).ceil() as i64);
        rest.min(self.duration - Tick(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Transition {
    #[default]
    Cut,
    Fade,
    /// Fades to black, then up into the new slide.
    Dip,
    Slide,
    SlideUp,
    /// The new slide is uncovered from left to right.
    Wipe,
    Zoom,
    /// Both slides are faces of a cube turning to the left.
    TurnLeft,
    TurnRight,
    /// The slide flips over like a card.
    Flip,
}

impl Transition {
    pub const ALL: [Transition; 10] = [
        Transition::Cut,
        Transition::Fade,
        Transition::Dip,
        Transition::Slide,
        Transition::SlideUp,
        Transition::Wipe,
        Transition::Zoom,
        Transition::TurnLeft,
        Transition::TurnRight,
        Transition::Flip,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Transition::Cut => "Cut",
            Transition::Fade => "Fade",
            Transition::Dip => "Dip to black",
            Transition::Slide => "Slide",
            Transition::SlideUp => "Slide up",
            Transition::Wipe => "Wipe",
            Transition::Zoom => "Zoom",
            Transition::TurnLeft => "3D left",
            Transition::TurnRight => "3D right",
            Transition::Flip => "3D flip",
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
    /// How it leaves at the end of the slide; `Motion::None` stays put.
    #[serde(default)]
    pub exit: Motion,
    /// Requested duration for each entrance and exit; playback fits short slides.
    #[serde(default = "default_motion_duration")]
    pub motion_duration_ms: u32,
    #[serde(default)]
    pub easing: Easing,
    /// Continuous photo movement, independent of entrance and exit.
    #[serde(default)]
    pub photo_motion: PhotoMotion,
    #[serde(default = "default_photo_amount")]
    pub photo_amount: f64,
}

fn default_photo_amount() -> f64 { 0.12 }

/// Camera movement inside a fixed photo frame; never exposes empty edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PhotoMotion {
    #[default]
    None,
    ZoomIn,
    ZoomOut,
    PanLeft,
    PanRight,
    Breathe,
}

impl PhotoMotion {
    pub const ALL: [Self; 6] = [Self::None, Self::ZoomIn, Self::ZoomOut, Self::PanLeft, Self::PanRight, Self::Breathe];
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "Still", Self::ZoomIn => "Zoom in", Self::ZoomOut => "Zoom out",
            Self::PanLeft => "Drift left", Self::PanRight => "Drift right", Self::Breathe => "Breathe",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::None => "Keep the photo still after its entrance.",
            Self::ZoomIn => "Move closer gradually to draw attention to the centre.",
            Self::ZoomOut => "Start close and slowly reveal the wider scene.",
            Self::PanLeft => "Glide left across the photo for a cinematic travel feel.",
            Self::PanRight => "Glide right across the photo for a cinematic travel feel.",
            Self::Breathe => "Gently zoom in and back out once over the slide.",
        }
    }
    /// Scale and horizontal offset as a fraction of the frame width.
    pub fn sample(self, progress: f64, amount: f64) -> (f64, f64) {
        let t = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
        let a = if amount.is_finite() { amount.clamp(0.0, 0.4) } else { 0.12 };
        let smooth = t * t * (3.0 - 2.0 * t);
        match self {
            Self::None => (1.0, 0.0),
            Self::ZoomIn => (1.0 + a * smooth, 0.0),
            Self::ZoomOut => (1.0 + a * (1.0 - smooth), 0.0),
            Self::PanLeft => (1.0 + a, a * (0.5 - smooth)),
            Self::PanRight => (1.0 + a, a * (smooth - 0.5)),
            Self::Breathe => (1.0 + a * (1.0 - (std::f64::consts::TAU * t).cos()) / 2.0, 0.0),
        }
    }
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
    Shape { fill: Color, corner_radius: f64, #[serde(default)] shape: ShapeKind },
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
            exit: Motion::None,
            motion_duration_ms: 600,
            easing: Easing::default(),
            photo_motion: PhotoMotion::None,
            photo_amount: 0.12,
        }
    }

    pub fn image(media: String, frame: Rect) -> Self {
        Element { frame, kind: ElementKind::Image { media, corner_radius: 0.0 }, motion: Motion::Fade, role: Role::Free, start_ms: None, exit: Motion::None, motion_duration_ms: 600, easing: Easing::default(), photo_motion: PhotoMotion::None, photo_amount: 0.12 }
    }

    pub fn shape(frame: Rect, fill: Color) -> Self {
        Element { frame, kind: ElementKind::Shape { fill, corner_radius: 24.0, shape: ShapeKind::Rectangle }, motion: Motion::Pop, role: Role::Free, start_ms: None, exit: Motion::None, motion_duration_ms: 600, easing: Easing::default(), photo_motion: PhotoMotion::None, photo_amount: 0.12 }
    }

    /// Bound imported timing values to keep playback finite and useful.
    pub fn motion_secs(&self) -> f64 { f64::from(self.motion_duration_ms.clamp(100, 10000)) / 1000.0 }

    pub fn with_role(self, role: Role) -> Self {
        Element { role, ..self }
    }
}

fn default_motion_duration() -> u32 { 600 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Easing { Linear, #[default] EaseOut, EaseInOut, EaseIn }
impl Easing {
    pub const ALL: [Self; 4] = [Self::Linear, Self::EaseOut, Self::EaseInOut, Self::EaseIn];
    pub fn name(self) -> &'static str { match self { Self::Linear => "Steady", Self::EaseOut => "Gentle finish", Self::EaseInOut => "Smooth", Self::EaseIn => "Build up" } }
    pub fn sample(self, p: f64) -> f64 {
        let p = p.clamp(0.0, 1.0);
        match self { Self::Linear => p, Self::EaseOut => 1.0-(1.0-p).powi(3), Self::EaseIn => p.powi(3), Self::EaseInOut => if p < 0.5 { 4.0*p*p*p } else { 1.0-(-2.0*p+2.0).powi(3)/2.0 } }
    }
}


/// Entrance and exit animations, named in plain words. Exits play the
/// entrance backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Motion {
    #[default]
    None,
    Fade,
    Rise,
    Pop,
    /// Comes down from above.
    Drop,
    FromLeft,
    FromRight,
    /// Shrinks into place from larger.
    Zoom,
    /// Turns towards the viewer around its upright middle.
    Flip,
    Spin,
    Typewriter,
    LetterReveal,
    LetterFade,
    LetterRise,
    LetterDrop,
    LetterPop,
    LetterSpin,
    LetterWave,
    WipeRight,
    WipeLeft,
    WipeUp,
    WipeDown,
    Bounce,
}

impl Motion {
    pub const ALL: [Motion; 23] = [
        Motion::None,
        Motion::Fade,
        Motion::Rise,
        Motion::Drop,
        Motion::FromLeft,
        Motion::FromRight,
        Motion::Pop,
        Motion::Zoom,
        Motion::Flip,
        Motion::Spin,
        Motion::Typewriter,
        Motion::LetterReveal,
        Motion::LetterFade,
        Motion::LetterRise,
        Motion::LetterDrop,
        Motion::LetterPop,
        Motion::LetterSpin,
        Motion::LetterWave,
        Motion::WipeRight,
        Motion::WipeLeft,
        Motion::WipeUp,
        Motion::WipeDown,
        Motion::Bounce,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Motion::None => "None",
            Motion::Fade => "Fade",
            Motion::Rise => "Rise",
            Motion::Drop => "Drop",
            Motion::FromLeft => "From left",
            Motion::FromRight => "From right",
            Motion::Pop => "Pop",
            Motion::Zoom => "Zoom",
            Motion::Flip => "3D flip",
            Motion::Spin => "Spin",
            Motion::Typewriter => "Typewriter",
            Motion::LetterReveal => "Draw letters",
            Motion::LetterFade => "Letter fade",
            Motion::LetterRise => "Letter rise",
            Motion::LetterDrop => "Letter drop",
            Motion::LetterPop => "Letter pop",
            Motion::LetterSpin => "Letter spin",
            Motion::LetterWave => "Letter wave",
            Motion::WipeRight => "Reveal right",
            Motion::WipeLeft => "Reveal left",
            Motion::WipeUp => "Reveal up",
            Motion::WipeDown => "Reveal down",
            Motion::Bounce => "Bounce",
        }
    }

    pub fn text_only(self) -> bool {
        matches!(self, Self::Typewriter | Self::LetterReveal | Self::LetterFade | Self::LetterRise | Self::LetterDrop | Self::LetterPop | Self::LetterSpin | Self::LetterWave)
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::None | Self::Fade => "Simple",
            Self::Rise | Self::Drop | Self::FromLeft | Self::FromRight => "Slide",
            Self::WipeRight | Self::WipeLeft | Self::WipeUp | Self::WipeDown => "Reveal",
            m if m.text_only() => "Letter by letter",
            _ => "Expressive",
        }
    }

    /// The same movement named for an item leaving.
    pub fn exit_name(self) -> &'static str {
        match self {
            Motion::None => "Stays",
            Motion::Fade => "Fade out",
            Motion::Rise => "Sink away",
            Motion::Drop => "Lift away",
            Motion::FromLeft => "Out to the left",
            Motion::FromRight => "Out to the right",
            Motion::Pop => "Shrink away",
            Motion::Zoom => "Grow and fade",
            Motion::Flip => "3D flip away",
            Motion::Spin => "Spin away",
            Motion::Typewriter => "Erase letters",
            Motion::LetterReveal => "Erase letter by letter",
            Motion::LetterFade => "Letters fade out",
            Motion::LetterRise => "Letters sink away",
            Motion::LetterDrop => "Letters lift away",
            Motion::LetterPop => "Letters shrink away",
            Motion::LetterSpin => "Letters spin away",
            Motion::LetterWave => "Letters wave away",
            Motion::WipeRight => "Conceal left",
            Motion::WipeLeft => "Conceal right",
            Motion::WipeUp => "Conceal down",
            Motion::WipeDown => "Conceal up",
            Motion::Bounce => "Bounce away",
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

/// Geometry shared by the picker, preview and export. Coordinates are normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ShapeKind {
    #[default]
    Rectangle, Ellipse, Triangle, Diamond, Pentagon, Hexagon, Octagon,
    Star, ArrowRight, ArrowLeft, ArrowUp, ArrowDown, Chevron, Plus, Heart, SpeechBubble, Trapezoid,
}

impl ShapeKind {
    pub const ALL: [Self; 17] = [Self::Rectangle, Self::Ellipse, Self::Triangle, Self::Diamond,
        Self::Pentagon, Self::Hexagon, Self::Octagon, Self::Star, Self::ArrowRight,
        Self::ArrowLeft, Self::ArrowUp, Self::ArrowDown, Self::Chevron, Self::Plus,
        Self::Heart, Self::SpeechBubble, Self::Trapezoid];

    pub fn name(self) -> &'static str {
        match self {
            Self::Trapezoid => "Trapezoid", Self::Rectangle => "Rectangle", Self::Ellipse => "Ellipse", Self::Triangle => "Triangle",
            Self::Diamond => "Diamond", Self::Pentagon => "Pentagon", Self::Hexagon => "Hexagon",
            Self::Octagon => "Octagon", Self::Star => "Star", Self::ArrowRight => "Right arrow",
            Self::ArrowLeft => "Left arrow", Self::ArrowUp => "Up arrow", Self::ArrowDown => "Down arrow",
            Self::Chevron => "Chevron", Self::Plus => "Plus", Self::Heart => "Heart", Self::SpeechBubble => "Speech bubble",
        }
    }

    pub fn points(self) -> Vec<(f64, f64)> {
        use std::f64::consts::{PI, TAU};
        let polygon = |n: usize, star: bool| (0..n).map(|i| {
            let angle = -PI / 2.0 + i as f64 * TAU / n as f64;
            let radius = if star && i % 2 == 1 { 0.22 } else { 0.5 };
            (0.5 + radius * angle.cos(), 0.5 + radius * angle.sin())
        }).collect();
        match self {
            Self::Trapezoid => vec![(0.2,0.),(0.8,0.),(1.,1.),(0.,1.)],
            Self::Rectangle => vec![(0.,0.),(1.,0.),(1.,1.),(0.,1.)],
            Self::Ellipse => polygon(64, false), Self::Triangle => polygon(3, false),
            Self::Diamond => polygon(4, false), Self::Pentagon => polygon(5, false),
            Self::Hexagon => polygon(6, false), Self::Octagon => polygon(8, false),
            Self::Star => polygon(10, true),
            Self::ArrowRight => vec![(0.,0.28),(0.58,0.28),(0.58,0.),(1.,0.5),(0.58,1.),(0.58,0.72),(0.,0.72)],
            Self::ArrowLeft => Self::ArrowRight.points().into_iter().map(|(x,y)| (1.-x,y)).collect(),
            Self::ArrowUp => Self::ArrowRight.points().into_iter().map(|(x,y)| (y,1.-x)).collect(),
            Self::ArrowDown => Self::ArrowRight.points().into_iter().map(|(x,y)| (y,x)).collect(),
            Self::Chevron => vec![(0.,0.),(0.55,0.),(1.,0.5),(0.55,1.),(0.,1.),(0.45,0.5)],
            Self::Plus => vec![(0.33,0.),(0.67,0.),(0.67,0.33),(1.,0.33),(1.,0.67),(0.67,0.67),(0.67,1.),(0.33,1.),(0.33,0.67),(0.,0.67),(0.,0.33),(0.33,0.33)],
            Self::Heart => (0..64).map(|i| {
                let t = i as f64 * TAU / 64.;
                (0.5 + t.sin().powi(3) / 2., (13. - (13.*t.cos()-5.*(2.*t).cos()-2.*(3.*t).cos()-(4.*t).cos())) / 30.)
            }).collect(),
            Self::SpeechBubble => vec![(0.,0.),(1.,0.),(1.,0.75),(0.4,0.75),(0.15,1.),(0.15,0.75),(0.,0.75)],
        }
    }
}

#[cfg(test)]
mod shape_tests {
    use super::*;
    #[test]
    fn old_shapes_default_to_rectangle() {
        let shape: ElementKind = serde_json::from_value(serde_json::json!({"Shape": {"fill": serde_json::to_value(Color::rgb(1,2,3)).unwrap(), "corner_radius": 24.0}})).unwrap();
        assert!(matches!(shape, ElementKind::Shape { shape: ShapeKind::Rectangle, .. }));
    }
    #[test]
    fn every_shape_round_trips_and_has_finite_outline() {
        for kind in ShapeKind::ALL {
            let encoded = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<ShapeKind>(&encoded).unwrap(), kind);
            let points = kind.points();
            assert!(points.len() >= 3);
            assert!(points.iter().all(|(x,y)| x.is_finite() && y.is_finite() && (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y)));
        }
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;
    #[test]
    fn legacy_elements_keep_original_timing() {
        let e = Element::text("Hello", Rect::new(0.,0.,400.,100.), 60., false, Color::rgb(0,0,0));
        let mut value = serde_json::to_value(&e).unwrap();
        value.as_object_mut().unwrap().remove("motion_duration_ms");
        value.as_object_mut().unwrap().remove("easing");
        let old: Element = serde_json::from_value(value).unwrap();
        assert_eq!(old.motion_secs(), ENTRANCE_SECS);
        assert_eq!(old.easing, Easing::EaseOut);
    }
    #[test]
    fn easing_is_monotonic_with_exact_endpoints() {
        for easing in Easing::ALL {
            assert_eq!(easing.sample(0.0), 0.0);
            assert_eq!(easing.sample(1.0), 1.0);
            let mut before = 0.0;
            for i in 0..=100 {
                let now = easing.sample(i as f64/100.0);
                assert!(now >= before && now <= 1.0);
                before = now;
            }
        }
    }
    #[test]
    fn longer_motion_updates_rest_time_and_fits_short_slides() {
        let mut project = Project::sample();
        let slide = &mut project.slides[0];
        slide.elements.truncate(1);
        slide.entrance = Entrance::Together;
        slide.elements[0].motion_duration_ms = 1800;
        assert_eq!(slide.rest_time(), Tick::from_millis(1800));
        slide.elements[0].exit = Motion::Fade;
        slide.duration = Tick::from_secs(1);
        assert_eq!(slide.element_motion_secs(0), 0.4);
        assert_eq!(slide.rest_time(), Tick::from_millis(600));
        for motion in Motion::ALL {
            let encoded = serde_json::to_string(&motion).unwrap();
            assert_eq!(serde_json::from_str::<Motion>(&encoded).unwrap(), motion);
        }
    }
}

#[cfg(test)]
mod photo_motion_tests {
    use super::*;

    #[test]
    fn old_projects_keep_photos_still_and_new_settings_round_trip() {
        let mut e = Element::image("photo".into(), Rect::new(0., 0., 400., 300.));
        let mut json = serde_json::to_value(&e).unwrap();
        json.as_object_mut().unwrap().remove("photo_motion");
        json.as_object_mut().unwrap().remove("photo_amount");
        let old: Element = serde_json::from_value(json).unwrap();
        assert_eq!(old.photo_motion, PhotoMotion::None);
        assert_eq!(old.photo_amount, 0.12);
        for motion in PhotoMotion::ALL {
            e.photo_motion = motion;
            e.photo_amount = 0.23;
            assert_eq!(serde_json::from_str::<Element>(&serde_json::to_string(&e).unwrap()).unwrap(), e);
        }
    }

    #[test]
    fn camera_stays_inside_photo_and_clamps_imported_values() {
        for motion in PhotoMotion::ALL {
            for amount in [-1., 0., 0.12, 0.4, 100., f64::NAN] {
                for i in -10..=110 {
                    let (scale, offset) = motion.sample(i as f64 / 100., amount);
                    assert!(scale.is_finite() && offset.is_finite());
                    assert!((1.0..=1.4).contains(&scale));
                    assert!(offset.abs() <= (scale - 1.0) / 2.0 + 1e-10);
                }
            }
        }
        assert_eq!(PhotoMotion::ZoomIn.sample(0., 0.2), (1., 0.));
        assert_eq!(PhotoMotion::ZoomOut.sample(1., 0.2), (1., 0.));
        assert_eq!(PhotoMotion::Breathe.sample(0.5, 0.2), (1.2, 0.));
    }
}
