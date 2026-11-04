//! Looks, slide layouts and starter templates: the smart defaults that make
//! a first video look designed without any design work.

use kn_geom::{Aspect, Color, Rect};
use kn_model::{Element, ElementKind, Motion, Project, Role, Slide, Transition};
use kn_time::Tick;
use serde::{Deserialize, Serialize};

/// A named look: colours and motion that are applied together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub background: Color,
    pub text: Color,
    pub muted: Color,
    /// Background and text of "dark" (contrasting) slides.
    pub dark_background: Color,
    pub dark_text: Color,
    pub accents: [Color; 3],
    /// Text drawn on accent cards.
    pub on_accent: Color,
    pub transition: Transition,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

pub const THEMES: &[Theme] = &[
    Theme {
        name: "Paper",
        background: rgb(250, 247, 242),
        text: rgb(24, 24, 32),
        muted: rgb(110, 110, 120),
        dark_background: rgb(24, 24, 32),
        dark_text: rgb(250, 247, 242),
        accents: [rgb(255, 94, 87), rgb(87, 199, 255), rgb(255, 214, 87)],
        on_accent: rgb(24, 24, 32),
        transition: Transition::Slide,
    },
    Theme {
        name: "Midnight",
        background: rgb(14, 17, 34),
        text: rgb(240, 242, 255),
        muted: rgb(150, 158, 196),
        dark_background: rgb(98, 76, 255),
        dark_text: rgb(255, 255, 255),
        accents: [rgb(98, 76, 255), rgb(0, 214, 180), rgb(255, 107, 168)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Zoom,
    },
    Theme {
        name: "Ocean",
        background: rgb(236, 246, 250),
        text: rgb(10, 45, 70),
        muted: rgb(80, 115, 140),
        dark_background: rgb(10, 70, 110),
        dark_text: rgb(236, 246, 250),
        accents: [rgb(0, 128, 180), rgb(0, 180, 160), rgb(255, 170, 60)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Fade,
    },
    Theme {
        name: "Sunset",
        background: rgb(255, 243, 232),
        text: rgb(60, 20, 30),
        muted: rgb(140, 90, 90),
        dark_background: rgb(200, 60, 70),
        dark_text: rgb(255, 243, 232),
        accents: [rgb(240, 90, 60), rgb(250, 160, 50), rgb(170, 60, 120)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Slide,
    },
    Theme {
        name: "Forest",
        background: rgb(240, 244, 236),
        text: rgb(24, 48, 30),
        muted: rgb(90, 110, 90),
        dark_background: rgb(30, 70, 45),
        dark_text: rgb(240, 244, 236),
        accents: [rgb(60, 140, 80), rgb(200, 160, 60), rgb(120, 90, 60)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Fade,
    },
    Theme {
        name: "Mono",
        background: rgb(255, 255, 255),
        text: rgb(10, 10, 10),
        muted: rgb(120, 120, 120),
        dark_background: rgb(10, 10, 10),
        dark_text: rgb(255, 255, 255),
        accents: [rgb(10, 10, 10), rgb(90, 90, 90), rgb(180, 180, 180)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Cut,
    },
    Theme {
        name: "Candy",
        background: rgb(255, 240, 248),
        text: rgb(60, 20, 80),
        muted: rgb(140, 90, 150),
        dark_background: rgb(130, 50, 200),
        dark_text: rgb(255, 240, 248),
        accents: [rgb(255, 90, 170), rgb(130, 90, 255), rgb(255, 190, 60)],
        on_accent: rgb(255, 255, 255),
        transition: Transition::Zoom,
    },
    Theme {
        name: "Bold",
        background: rgb(255, 220, 0),
        text: rgb(0, 0, 0),
        muted: rgb(60, 50, 0),
        dark_background: rgb(0, 0, 0),
        dark_text: rgb(255, 220, 0),
        accents: [rgb(0, 0, 0), rgb(255, 60, 60), rgb(255, 255, 255)],
        on_accent: rgb(255, 220, 0),
        transition: Transition::Slide,
    },
];

/// The theme called `name`, or the first one if it doesn't exist.
pub fn theme(name: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.name == name).unwrap_or(&THEMES[0])
}

/// Switches the whole project to `theme`: colours and transitions.
pub fn apply_theme(project: &Project, theme: &Theme) -> Project {
    let mut base = project.clone();
    for slide in &mut base.slides { slide.theme = None; }
    let mut p = recolour(&base, theme);
    p.theme = theme.name.into();
    for slide in p.slides.iter_mut().skip(1) {
        slide.transition = theme.transition;
    }
    p
}

/// Recolours every slide and every item with a role; hand-placed items
/// keep their colours.
pub fn recolour(project: &Project, theme: &Theme) -> Project {
    let mut p = project.clone();
    for slide in &mut p.slides {
        let theme = slide.theme.as_deref().map(self::theme).unwrap_or(theme);
        recolour_slide(slide, theme);
    }
    p
}

/// Recolour one slide without changing its timing or other slides.
pub fn recolour_slide(slide: &mut Slide, theme: &Theme) {
        slide.background = if slide.dark { theme.dark_background } else { theme.background };
        let text = if slide.dark { theme.dark_text } else { theme.text };
        let muted = if slide.dark { theme.dark_text.with_alpha(200) } else { theme.muted };
        for element in &mut slide.elements {
            match (&mut element.kind, element.role) {
                (ElementKind::Text { color, .. }, Role::Title | Role::Body) => *color = text,
                (ElementKind::Text { color, .. }, Role::Caption) => *color = muted,
                (ElementKind::Text { color, .. }, Role::OnCard) => *color = theme.on_accent,
                (ElementKind::Text { color, .. }, Role::Accent(n)) => *color = theme.accents[usize::from(n) % 3],
                (ElementKind::Shape { fill, .. }, Role::Card(n)) => *fill = theme.accents[usize::from(n) % 3],
                _ => {}
            }
        }
}

/// Moves items when the video shape changes so each keeps its place
/// relative to the slide's height; sizes stay the same.
pub fn reflow(project: &Project, to: Aspect) -> Project {
    let (_, from_h) = project.aspect.canvas_size();
    let (_, to_h) = to.canvas_size();
    let mut p = project.clone();
    p.aspect = to;
    for element in p.slides.iter_mut().flat_map(|s| &mut s.elements) {
        let f = &mut element.frame;
        let centre = (f.y + f.h / 2.0) / from_h;
        f.y = (centre * to_h - f.h / 2.0).clamp(0.0, (to_h - f.h).max(0.0));
    }
    p
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    Title,
    TitleAndText,
    List,
    BigNumber,
    Quote,
    Blank,
}

impl Layout {
    pub const ALL: [Layout; 6] = [Layout::Title, Layout::TitleAndText, Layout::List, Layout::BigNumber, Layout::Quote, Layout::Blank];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Title => "Title",
            Layout::TitleAndText => "Title and text",
            Layout::List => "Three points",
            Layout::BigNumber => "Big number",
            Layout::Quote => "Quote",
            Layout::Blank => "Blank",
        }
    }
}

/// Words for a new slide; empty strings get friendly placeholders.
#[derive(Default, Clone, Copy)]
pub struct Words<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub points: [&'a str; 3],
}

const MARGIN: f64 = 160.0;
const WIDTH: f64 = 1920.0 - 2.0 * MARGIN;

fn or<'a>(s: &'a str, fallback: &'a str) -> &'a str {
    if s.is_empty() { fallback } else { s }
}

/// Rough width of a character in the bundled sans fonts, per unit of size.
const CHAR_WIDTH: f64 = 0.55;

/// Shrinks `size` (to no less than 70%) so `s` fits on `max_lines` lines,
/// and returns the size with the number of lines it will take.
fn fit(s: &str, size: f64, max_lines: f64) -> (f64, f64) {
    let chars = s.chars().count().max(1) as f64;
    let lines_at = |size: f64| (chars * CHAR_WIDTH * size / WIDTH).ceil().max(1.0);
    let fitted = (WIDTH * max_lines / (chars * CHAR_WIDTH)).clamp(size * 0.7, size);
    (fitted, lines_at(fitted))
}

fn text(s: &str, y: f64, size: f64, bold: bool, role: Role, motion: Motion) -> Element {
    let mut e = Element::text(s, Rect::new(MARGIN, y, WIDTH, size * 1.2), size, bold, Color::rgb(0, 0, 0)).with_role(role);
    e.motion = motion;
    e
}

/// A slide in `layout`, sized for `aspect` and coloured by `theme`.
pub fn slide(layout: Layout, words: Words<'_>, aspect: Aspect, theme: &Theme) -> Slide {
    let (_, h) = aspect.canvas_size();
    // Positions are fractions of a 16:9 slide; taller shapes keep content
    // grouped around the middle instead of spreading it to the edges.
    let span = h.min(1620.0);
    let at = |fraction: f64, size: f64| h / 2.0 + (fraction - 0.5) * span - size * 0.6;
    let mut dark = false;
    let elements = match layout {
        Layout::Title => {
            let title = or(words.title, "Your big idea");
            let (size, lines) = fit(title, 120.0, 2.0);
            // A two-line title grows upwards and downwards from the middle.
            let extra = (lines - 1.0) * size * 1.2;
            vec![
                Element::shape(Rect::new(1920.0 / 2.0 - 120.0, at(0.30, 16.0) - extra / 2.0, 240.0, 16.0), Color::rgb(0, 0, 0))
                    .with_role(Role::Card(0)),
                text(title, at(0.44, size) - extra / 2.0, size, true, Role::Title, Motion::Rise),
                text(or(words.body, "A short line that says why it matters"), at(0.60, 56.0) + extra / 2.0, 56.0, false, Role::Caption, Motion::Fade),
            ]
        }
        Layout::TitleAndText => {
            let title = or(words.title, "A clear heading");
            let (size, lines) = fit(title, 96.0, 2.0);
            let extra = (lines - 1.0) * size * 1.2;
            vec![
                text(title, at(0.18, size), size, true, Role::Title, Motion::Rise),
                text(
                    or(words.body, "Say one thing per slide. Keep it short, so people can read it while you talk."),
                    at(0.45, 64.0) + extra,
                    64.0,
                    false,
                    Role::Body,
                    Motion::Fade,
                ),
            ]
        }
        Layout::List => {
            let title = or(words.title, "Three things to know");
            let (size, _) = fit(title, 90.0, 1.0);
            let mut v = vec![text(title, at(0.13, size), size, true, Role::Title, Motion::Rise)];
            let fallback = ["First point", "Second point", "Third point"];
            for (i, (word, fallback)) in words.points.iter().zip(fallback).enumerate() {
                let y = at(0.36 + 0.21 * i as f64, 0.0) - 80.0;
                let card = Element::shape(Rect::new(360.0, y, 1200.0, 160.0), Color::rgb(0, 0, 0)).with_role(Role::Card(i as u8));
                let mut label = text(or(word, fallback), y + 47.0, 56.0, true, Role::OnCard, Motion::Fade);
                label.frame = Rect::new(400.0, y + 47.0, 1120.0, 67.0);
                v.extend([card, label]);
            }
            v
        }
        Layout::BigNumber => {
            let mut number = text(or(words.title, "3×"), at(0.42, 300.0), 300.0, true, Role::Title, Motion::Pop);
            number.role = Role::Title;
            vec![number, text(or(words.body, "faster than doing it by hand"), at(0.66, 64.0), 64.0, false, Role::Caption, Motion::Fade)]
        }
        Layout::Quote => {
            dark = true;
            let quote = or(words.title, "\u{201c}This made our video in five minutes.\u{201d}");
            let (size, lines) = fit(quote, 88.0, 3.0);
            let extra = (lines - 1.0) * size * 1.2;
            vec![
                text(quote, at(0.42, size) - extra / 2.0, size, true, Role::Title, Motion::Fade),
                text(or(words.body, "Someone who tried it"), at(0.64, 52.0) + extra / 2.0, 52.0, false, Role::Caption, Motion::Fade),
            ]
        }
        Layout::Blank => Vec::new(),
    };
    let slide = Slide { theme: None, background: Color::rgb(0, 0, 0), duration: Tick::from_secs(4), elements, transition: theme.transition, dark, voice: None, entrance: kn_model::Entrance::OneByOne };
    let one = Project { aspect, slides: vec![slide], theme: theme.name.into(), music: None };
    apply_theme(&one, theme).slides.remove(0)
}

/// Groups of starter templates on the welcome page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Birthdays, congratulations, parties, weddings and events.
    Celebrations,
    /// Photo collages, a year in review, travel and family.
    Memories,
    /// Film-style openings and big moving words.
    Titles,
    /// Launches, portfolios and property tours.
    Business,
    /// Guides and lessons.
    Learning,
    /// Tall posts and quote cards for sharing.
    Social,
    /// The blank canvas.
    Basics,
}

impl Category {
    /// In the order the welcome page shows them.
    pub const ALL: [Category; 7] = [Category::Celebrations, Category::Memories, Category::Titles, Category::Business, Category::Learning, Category::Social, Category::Basics];

    /// The chip and heading text on the welcome page.
    pub fn name(self) -> &'static str {
        match self {
            Category::Celebrations => "Celebrations",
            Category::Memories => "Memories & collages",
            Category::Titles => "Titles & intros",
            Category::Business => "Business",
            Category::Learning => "Learning",
            Category::Social => "Social",
            Category::Basics => "Basics",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Birthday,
    Congratulations,
    Party,
    Wedding,
    Event,
    Collage,
    YearInReview,
    Travel,
    Family,
    Story,
    MovieTitle,
    KineticTitle,
    Promo,
    Portfolio,
    RealEstate,
    Tutorial,
    Lesson,
    Social,
    Quote,
    Blank,
}

impl Template {
    /// Grouped by category, in the order the welcome page shows them.
    pub const ALL: [Template; 20] = [
        Template::Birthday, Template::Congratulations, Template::Party, Template::Wedding, Template::Event,
        Template::Collage, Template::YearInReview, Template::Travel, Template::Family, Template::Story,
        Template::MovieTitle, Template::KineticTitle,
        Template::Promo, Template::Portfolio, Template::RealEstate,
        Template::Tutorial, Template::Lesson,
        Template::Social, Template::Quote,
        Template::Blank,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Template::Birthday => "Birthday",
            Template::Congratulations => "Congratulations",
            Template::Party => "Party invitation",
            Template::Wedding => "Wedding",
            Template::Event => "Event invitation",
            Template::Collage => "Photo collage",
            Template::YearInReview => "Year in review",
            Template::Travel => "Travel diary",
            Template::Family => "Family memories",
            Template::Story => "Photo story",
            Template::MovieTitle => "Movie title",
            Template::KineticTitle => "Kinetic title",
            Template::Promo => "Product promo",
            Template::Portfolio => "Creative portfolio",
            Template::RealEstate => "Real estate",
            Template::Tutorial => "Tutorial",
            Template::Lesson => "Lesson",
            Template::Social => "Social post",
            Template::Quote => "Quote card",
            Template::Blank => "Blank",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Template::Birthday => "Balloons, confetti, a cake and your favourite photos",
            Template::Congratulations => "Big bold words and confetti for a well-earned win",
            Template::Party => "A night-out invitation with a cinematic city and a dance beat",
            Template::Wedding => "Polaroid memories, gentle reveals and dreamy music",
            Template::Event => "A cinematic invitation with a big-idea soundtrack",
            Template::Collage => "Mosaics, grids and tossed polaroids for a set of photos",
            Template::YearInReview => "Your year in numbers, photos and one quote to remember",
            Template::Travel => "Full-screen views and a photo mosaic with a relaxed beat",
            Template::Family => "Polaroids, a photo grid and a cheerful soundtrack",
            Template::Story => "A visual journal with a grid and a full-screen finish",
            Template::MovieTitle => "A 3-2-1 countdown, letterboxed shots and opening credits",
            Template::KineticTitle => "Huge words that pop, rise and drop to a dance beat",
            Template::Promo => "A bold launch, crisp feature cards and an upbeat finish",
            Template::Portfolio => "An editorial showcase for your work and process",
            Template::RealEstate => "A full-screen property tour with room highlights",
            Template::Tutorial => "An illustrated guide with clear steps and gentle motion",
            Template::Lesson => "Make a big idea approachable with clear visual steps",
            Template::Social => "A portrait story with warm visuals and an upbeat rhythm",
            Template::Quote => "A tall quote card to share, with a calm photo finish",
            Template::Blank => "A fresh canvas for your own ideas",
        }
    }

    /// Which welcome-page group the template belongs to.
    pub fn category(self) -> Category {
        match self {
            Template::Birthday | Template::Congratulations | Template::Party | Template::Wedding | Template::Event => Category::Celebrations,
            Template::Collage | Template::YearInReview | Template::Travel | Template::Family | Template::Story => Category::Memories,
            Template::MovieTitle | Template::KineticTitle => Category::Titles,
            Template::Promo | Template::Portfolio | Template::RealEstate => Category::Business,
            Template::Tutorial | Template::Lesson => Category::Learning,
            Template::Social | Template::Quote => Category::Social,
            Template::Blank => Category::Basics,
        }
    }

    fn look(self) -> (&'static str, Aspect) {
        match self {
            Template::Birthday => ("Candy", Aspect::Wide16x9),
            Template::Congratulations => ("Bold", Aspect::Wide16x9),
            Template::Party => ("Midnight", Aspect::Wide16x9),
            Template::Wedding => ("Paper", Aspect::Wide16x9),
            Template::Event => ("Midnight", Aspect::Wide16x9),
            Template::Collage => ("Paper", Aspect::Wide16x9),
            Template::YearInReview => ("Midnight", Aspect::Wide16x9),
            Template::Travel => ("Ocean", Aspect::Wide16x9),
            Template::Family => ("Sunset", Aspect::Square1x1),
            Template::Story => ("Forest", Aspect::Wide16x9),
            Template::MovieTitle => ("Midnight", Aspect::Wide16x9),
            Template::KineticTitle => ("Bold", Aspect::Wide16x9),
            Template::Promo => ("Midnight", Aspect::Wide16x9),
            Template::Portfolio => ("Mono", Aspect::Wide16x9),
            Template::RealEstate => ("Mono", Aspect::Wide16x9),
            Template::Tutorial => ("Paper", Aspect::Wide16x9),
            Template::Lesson => ("Ocean", Aspect::Wide16x9),
            Template::Social => ("Sunset", Aspect::Tall9x16),
            Template::Quote => ("Sunset", Aspect::Tall9x16),
            Template::Blank => ("Paper", Aspect::Wide16x9),
        }
    }
}

mod templates;

/// A designed starter, with bundled artwork and built-in music.
pub fn template(template: Template) -> Project { templates::build(template) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_builds_and_fits_its_canvas() {
        for t in Template::ALL {
            let p = template(t);
            assert!(!p.slides.is_empty(), "{t:?}");
            let (w, h) = p.aspect.canvas_size();
            for e in p.slides.iter().flat_map(|s| &s.elements) {
                let f = e.frame;
                assert!(f.x >= 0.0 && f.y >= 0.0 && f.x + f.w <= w && f.y + f.h <= h, "{t:?}: {f:?} outside {w}×{h}");
            }
        }
    }

    #[test]
    fn theme_recolours_roles_but_not_free_items() {
        let mut p = template(Template::Tutorial);
        p.slides[0].elements.push(Element::shape(Rect::new(0.0, 0.0, 10.0, 10.0), Color::rgb(1, 2, 3)));
        let midnight = theme("Midnight");
        let q = apply_theme(&p, midnight);
        assert_eq!(q.theme, "Midnight");
        assert_eq!(q.slides[0].background, midnight.background);
        let title = q.slides[0].elements.iter().find(|e| e.role == Role::Title).unwrap();
        assert!(matches!(title.kind, ElementKind::Text { color, .. } if color == midnight.text));
        let free = q.slides[0].elements.last().unwrap();
        assert!(matches!(free.kind, ElementKind::Shape { fill, .. } if fill == Color::rgb(1, 2, 3)));
    }

    #[test]
    fn reflow_keeps_relative_position_and_stays_inside() {
        let p = template(Template::Tutorial);
        let tall = reflow(&p, Aspect::Tall9x16);
        let (_, h) = Aspect::Tall9x16.canvas_size();
        let (a, b) = (&p.slides[0].elements[1].frame, &tall.slides[0].elements[1].frame);
        let rel = |f: &Rect, h: f64| (f.y + f.h / 2.0) / h;
        assert!((rel(a, 1080.0) - rel(b, h)).abs() < 1e-9);
        let square = reflow(&tall, Aspect::Square1x1);
        for e in square.slides.iter().flat_map(|s| &s.elements) {
            assert!(e.frame.y >= 0.0 && e.frame.y + e.frame.h <= 1920.0);
        }
    }

    #[test]
    fn long_titles_shrink_and_wrap_predictably() {
        assert_eq!(fit("Short", 120.0, 2.0), (120.0, 1.0));
        // Fits on two lines at full size: wraps but doesn't shrink.
        assert_eq!(fit("Meet your new favourite app, it is great", 120.0, 2.0), (120.0, 2.0));
        let (size, lines) = fit("Meet your new favourite app, it is great and it is very fast", 120.0, 2.0);
        assert!((84.0..120.0).contains(&size) && lines <= 2.0, "{size} {lines}");
    }

    #[test]
    fn unknown_theme_falls_back() {
        assert_eq!(theme("Nope").name, "Paper");
    }
}
