//! Designed, editable starters. The sample media travels with the project.
use super::*;
use kn_model::{Easing, Entrance, Mood, Music, MusicSource, ShapeKind, TextAlign};

fn words(
    text: &str,
    frame: Rect,
    size: f64,
    bold: bool,
    role: Role,
    th: &Theme,
    motion: Motion,
) -> Element {
    let color = if role == Role::Caption {
        th.muted
    } else {
        th.text
    };
    let mut element = Element::text(text, frame, size, bold, color).with_role(role);
    if let ElementKind::Text { align, .. } = &mut element.kind {
        *align = TextAlign::Start;
    }
    element.motion = motion;
    element.motion_duration_ms = if motion.text_only() { 1200 } else { 650 };
    element.easing = if motion.text_only() {
        Easing::Linear
    } else {
        Easing::EaseOut
    };
    element.start_ms = Some(if role == Role::Title {
        180
    } else if role == Role::Body {
        550
    } else {
        0
    });
    element
}

fn shape(frame: Rect, kind: ShapeKind, accent: u8, th: &Theme) -> Element {
    let mut element =
        Element::shape(frame, th.accents[accent as usize % 3]).with_role(Role::Card(accent));
    if let ElementKind::Shape {
        shape,
        corner_radius,
        ..
    } = &mut element.kind
    {
        *shape = kind;
        *corner_radius = 32.0;
    }
    element.motion = Motion::Pop;
    element.start_ms = Some(0);
    element
}

fn picture(index: usize, frame: Rect) -> Element {
    let mut element = Element::image(kn_media::sample_id(index), frame);
    if let ElementKind::Image { corner_radius, .. } = &mut element.kind {
        *corner_radius = 36.0;
    }
    element.start_ms = Some(0);
    element.motion_duration_ms = 900;
    element.photo_motion = match index % 3 {
        0 => kn_model::PhotoMotion::ZoomIn,
        1 => kn_model::PhotoMotion::PanLeft,
        _ => kn_model::PhotoMotion::ZoomOut,
    };
    element
}

fn page(elements: Vec<Element>, th: &Theme, transition: Transition, seconds: i64) -> Slide {
    Slide {
        theme: None,
        background: th.background,
        duration: Tick::from_secs(seconds),
        elements,
        transition,
        dark: false,
        voice: None,
        entrance: Entrance::Together,
    }
}

fn hero(title: &str, subtitle: &str, label: &str, art: usize, aspect: Aspect, th: &Theme) -> Slide {
    let (_, h) = aspect.canvas_size();
    let tall = aspect == Aspect::Tall9x16;
    let elements = if tall {
        vec![
            picture(art, Rect::new(130., 180., 1660., h * 0.42)),
            words(
                label,
                Rect::new(150., h * 0.52, 1500., 90.),
                52.,
                true,
                Role::Caption,
                th,
                Motion::Fade,
            ),
            words(
                title,
                Rect::new(150., h * 0.59, 1620., 540.),
                174.,
                true,
                Role::Title,
                th,
                Motion::LetterRise,
            ),
            words(
                subtitle,
                Rect::new(150., h * 0.79, 1500., 220.),
                62.,
                false,
                Role::Body,
                th,
                Motion::Fade,
            ),
        ]
    } else {
        vec![
            picture(art, Rect::new(1040., 120., 750., 800.)),
            words(
                label,
                Rect::new(130., 180., 830., 65.),
                30.,
                true,
                Role::Caption,
                th,
                Motion::Fade,
            ),
            words(
                title,
                Rect::new(130., 310., 840., 360.),
                112.,
                true,
                Role::Title,
                th,
                Motion::LetterRise,
            ),
            words(
                subtitle,
                Rect::new(130., 700., 810., 150.),
                40.,
                false,
                Role::Body,
                th,
                Motion::Fade,
            ),
            shape(
                Rect::new(130., 905., 180., 10.),
                ShapeKind::Rectangle,
                0,
                th,
            ),
        ]
    };
    page(elements, th, Transition::Fade, 5)
}

fn cards(title: &str, items: [(&str, &str); 3], aspect: Aspect, th: &Theme) -> Slide {
    let (_, h) = aspect.canvas_size();
    let tall = aspect == Aspect::Tall9x16;
    let mut elements = vec![words(
        title,
        Rect::new(130., h * 0.12, 1640., 240.),
        if tall { 126. } else { 94. },
        true,
        Role::Title,
        th,
        Motion::LetterFade,
    )];
    for (i, (title, body)) in items.into_iter().enumerate() {
        let (x, y, w, height) = if tall {
            (130., h * 0.30 + i as f64 * 590., 1660., 510.)
        } else {
            (130. + i as f64 * 570., 400., 520., 440.)
        };
        let mut card = Element::shape(Rect::new(x, y, w, height), th.text.with_alpha(10));
        card.motion = Motion::Rise;
        card.start_ms = Some(100 + i as i64 * 160);
        elements.push(card);
        elements.push(shape(
            Rect::new(x + 40., y + 40., 64., 64.),
            [ShapeKind::Ellipse, ShapeKind::Diamond, ShapeKind::Hexagon][i],
            i as u8,
            th,
        ));
        elements.push(words(
            title,
            Rect::new(x + 40., y + 145., w - 80., 140.),
            if tall { 74. } else { 47. },
            true,
            Role::Title,
            th,
            Motion::Rise,
        ));
        elements.push(words(
            body,
            Rect::new(x + 40., y + 285., w - 80., height - 310.),
            if tall { 52. } else { 32. },
            false,
            Role::Body,
            th,
            Motion::Fade,
        ));
    }
    page(elements, th, Transition::Slide, 6)
}

fn closing(title: &str, body: &str, aspect: Aspect, th: &Theme) -> Slide {
    let (_, h) = aspect.canvas_size();
    let mut elements = vec![
        shape(
            Rect::new(1500., h * 0.1, 220., 220.),
            ShapeKind::Ellipse,
            1,
            th,
        ),
        shape(
            Rect::new(200., h * 0.76, 90., 90.),
            ShapeKind::Diamond,
            0,
            th,
        ),
        words(
            "YOUR NEXT CHAPTER",
            Rect::new(260., h * 0.24, 1400., 70.),
            36.,
            true,
            Role::Caption,
            th,
            Motion::Fade,
        ),
        words(
            title,
            Rect::new(260., h * 0.37, 1400., 400.),
            if h > 2000. { 160. } else { 108. },
            true,
            Role::Title,
            th,
            Motion::LetterRise,
        ),
        words(
            body,
            Rect::new(260., h * 0.69, 1400., 190.),
            if h > 2000. { 60. } else { 44. },
            false,
            Role::Body,
            th,
            Motion::Fade,
        ),
    ];
    for element in &mut elements {
        if let ElementKind::Text { align, .. } = &mut element.kind {
            *align = TextAlign::Center;
        }
    }
    page(elements, th, Transition::Fade, 5)
}

pub(super) fn build(template: Template) -> Project {
    let (name, aspect) = template.look();
    let th = theme(name);
    if template == Template::Blank {
        return Project {
            aspect,
            theme: name.into(),
            music: None,
            slides: vec![slide(
                Layout::Title,
                Words {
                    title: "Your title",
                    body: "Make it your own",
                    ..Words::default()
                },
                aspect,
                th,
            )],
        };
    }
    let (title, subtitle, label, art, points, ending, action, mood) = match template {
        Template::Tutorial => (
            "From idea\nto screen.",
            "A simple guide to making something worth sharing.",
            "THE CREATOR'S GUIDE",
            1,
            [
                ("Find your idea", "Start with one clear message."),
                ("Build the story", "Pair a few words with a strong visual."),
                ("Make it move", "Set the rhythm. Share your creation."),
            ],
            "Your story.\nReady to share.",
            "Replace these words with your own next step.",
            Mood::Calm,
        ),
        Template::Promo => (
            "Introducing\nForma.",
            "A little more focus. A lot more possibility.",
            "PRODUCT LAUNCH / 01",
            2,
            [
                ("Less friction", "A simpler way to start your day."),
                ("More focus", "Space for the work that matters."),
                ("Made for you", "Shape the experience around your ideas."),
            ],
            "Make room\nfor your next idea.",
            "Your product. Your invitation. Add your link here.",
            Mood::Upbeat,
        ),
        Template::Lesson => (
            "Big ideas.\nClear stories.",
            "Turn a complicated topic into a memorable lesson.",
            "A FRESH PERSPECTIVE",
            2,
            [
                ("Notice", "Look closely. What stands out?"),
                ("Connect", "Relate the idea to something familiar."),
                ("Try it", "Use one small action to make it stick."),
            ],
            "Keep your\ncuriosity moving.",
            "Add a question your audience can explore next.",
            Mood::Calm,
        ),
        Template::Social => (
            "Small rituals.\nBetter days.",
            "Three ways to bring a little calm into your routine.",
            "A MOMENT FOR YOU",
            1,
            [
                ("Make space", "Clear one small corner of your day."),
                ("Step outside", "Find a new perspective, close to home."),
                ("Start small", "One good habit is enough to begin."),
            ],
            "Save a little\nspace for yourself.",
            "Make this your own. Add your handle here.",
            Mood::Upbeat,
        ),
        Template::Story => (
            "Take the\nscenic route.",
            "A visual journal of places, pauses and new perspectives.",
            "FIELD NOTES / VOL. 01",
            0,
            [
                ("Slow down", "There is more here than a destination."),
                ("Look closer", "Keep the details that make a place yours."),
                (
                    "Bring it home",
                    "A small collection of moments to remember.",
                ),
            ],
            "Go somewhere\nthat stays with you.",
            "Replace the sample artwork with your own pictures.",
            Mood::Calm,
        ),
        Template::Portfolio => (
            "Work with\nperspective.",
            "A selection of ideas, experiments and considered details.",
            "SELECTED WORK / YOUR NAME",
            2,
            [
                ("The idea", "A clear point of view comes first."),
                ("The process", "Explore, refine and find the right balance."),
                ("The result", "Let the work tell its own story."),
            ],
            "Let's make\nsomething matter.",
            "Add your name, portfolio link or contact details.",
            Mood::Calm,
        ),
        Template::Event => (
            "Good people.\nGreat ideas.",
            "An invitation to connect, create and think a little bigger.",
            "YOU'RE INVITED",
            2,
            [
                ("Connect", "Meet people with a fresh point of view."),
                ("Create", "Make space for something unexpected."),
                ("Celebrate", "Leave with a story worth sharing."),
            ],
            "See you\nthere.",
            "Add your date, venue and registration link.",
            Mood::Epic,
        ),
        Template::Wedding => (
            "Together,\nalways.", "Join us for a day filled with love and laughter.", "SAVE THE DATE", 1,
            [("The ceremony", "Where our next chapter begins."), ("The celebration", "Dinner, dancing and our favourite people."), ("The details", "Add your date, location and RSVP.")],
            "With love,\nfrom us.", "We cannot wait to celebrate with you.", Mood::Dreamy,
        ),
        Template::Travel => (
            "Away we go.", "A few favourite places, a thousand little stories.", "TRAVEL DIARY", 0,
            [("First stop", "A new place to explore."), ("Hidden gems", "Take the road less travelled."), ("Golden hour", "Stay for one more sunset.")],
            "Until the\nnext adventure.", "Add your destination and favourite memory.", Mood::Lofi,
        ),
        Template::Family => (
            "Our favourite\nmoments.", "Little adventures. Big smiles. Memories together.", "THE FAMILY ALBUM", 1,
            [("Everyday joy", "The small things we love."), ("Big adventures", "Making new memories together."), ("Growing together", "A new chapter every day.")],
            "Made with\nlove.", "Add a date, a name or your own dedication.", Mood::Playful,
        ),
        Template::RealEstate => (
            "Welcome\nhome.", "Beautiful spaces, thoughtful details and room to grow.", "PROPERTY SHOWCASE", 2,
            [("Living spaces", "Light-filled rooms for everyday life."), ("The details", "Finishes that make a difference."), ("The location", "Everything you need, close to home.")],
            "See it\nfor yourself.", "Add your contact details and arrange a viewing.", Mood::Corporate,
        ),
        Template::Blank => unreachable!(),
    };
    let mut slides = vec![
        hero(title, subtitle, label, art, aspect, th),
        cards("A few things\nto inspire you.", points, aspect, th),
        hero(
            "A different\npoint of view.",
            "Every frame is yours to change. Add a picture, shape or a few carefully chosen words.",
            "MAKE IT YOUR OWN",
            (art + 1) % 3,
            aspect,
            th,
        ),
        closing(ending, action, aspect, th),
    ];
    slides[0].transition = Transition::Cut;
    Project {
        aspect,
        theme: name.into(),
        slides,
        music: Some(Music::new(MusicSource::Mood(mood), 0.22)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starters_have_real_media_sound_and_text_that_fits() {
        let media = kn_media::sample_library();
        let mut text_engine = kn_text::TextEngine::new();
        for template in Template::ALL {
            let project = build(template);
            if template != Template::Blank {
                assert!(project.music.is_some());
                assert!(
                    project
                        .slides
                        .iter()
                        .flat_map(|s| &s.elements)
                        .any(|e| e.motion.text_only())
                );
                assert!(
                    project
                        .slides
                        .iter()
                        .skip(1)
                        .any(|s| s.transition != Transition::Cut)
                );
                assert!(project.media_ids().next().is_some());
            }
            for id in project.media_ids() {
                assert!(media.decode(id).is_ok(), "{template:?}: {id}");
            }
            let (_, h) = project.aspect.canvas_size();
            for element in project.slides.iter().flat_map(|s| &s.elements) {
                if let ElementKind::Text {
                    text, size, bold, ..
                } = &element.kind
                {
                    let (_, height) = text_engine.measure(
                        text,
                        &kn_text::TextStyle {
                            size: *size as f32,
                            bold: *bold,
                            align: kn_text::Align::Start,
                            width: element.frame.w as f32,
                        },
                    );
                    assert!(
                        element.frame.y + f64::from(height) <= h,
                        "{template:?}: text outside canvas: {text}"
                    );
                }
            }
        }
    }
}
