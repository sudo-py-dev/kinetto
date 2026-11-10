//! Symbolic icons drawn as vectors on a 16-unit grid, in the style of
//! GNOME's symbolic icon set, so they stay sharp and follow the text colour.

use egui::epaint::{CubicBezierShape, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2, vec2};

/// The symbolic icons the app uses, drawn as vector strokes so they stay
/// sharp at any scale and take any colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Add,
    Undo,
    Redo,
    Play,
    Stop,
    Menu,
    Close,
    Minimize,
    Maximize,
    Restore,
    Text,
    Shape,
    Picture,
    Microphone,
    Delete,
    More,
    /// A chevron pointing back.
    Left,
    Up,
    Down,
    Copy,
    Video,
    Open,
    /// One slide giving way to the next.
    Transition,
    /// Jump to the previous slide.
    Previous,
    /// Jump to the next slide.
    Next,
    /// A floppy disk: save.
    Save,
    /// A circled "i": about.
    Info,
    /// A keyboard: shortcuts.
    Keyboard,
    /// A screen with a play mark: present full screen.
    Present,
    /// A magnifying glass: find an action.
    Search,
    /// A clipboard: paste.
    Paste,
    /// Two stacked sheets with the upper one solid: bring to front.
    Front,
    /// Two stacked sheets with the lower one solid: send to back.
    Back,
}

/// A small symbolic chevron with rounded ends. Zero radians points right.
/// Kept separate so disclosure arrows can rotate with their reveal animation.
pub fn chevron(painter: &Painter, rect: Rect, angle: f32, color: Color32) {
    let size = 16.0_f32.min(rect.width()).min(rect.height());
    let k = size / 16.0;
    let rotation = egui::emath::Rot2::from_angle(angle);
    let point = |x: f32, y: f32| rect.center() + rotation * (vec2(x, y) * k);
    let points = vec![point(-2.5, -4.0), point(1.5, 0.0), point(-2.5, 4.0)];
    let stroke = Stroke::new(1.8 * k, color);
    painter.add(PathShape::line(points.clone(), stroke));
    // Rounded joins and caps keep the symbolic stroke soft at small sizes.
    for point in points { painter.circle_filled(point, stroke.width / 2.0, color); }
}

/// Draws `icon` centred in `rect`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let size = 16.0f32.min(rect.width()).min(rect.height());
    let origin = rect.center() - Vec2::splat(size / 2.0);
    let k = size / 16.0;
    let p = |x: f32, y: f32| origin + vec2(x, y) * k;
    let stroke = Stroke::new(1.6 * k, color);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], stroke);
    };
    let poly = |pts: Vec<Pos2>, closed: bool| {
        let shape = if closed { PathShape::closed_line(pts, stroke) } else { PathShape::line(pts, stroke) };
        painter.add(shape);
    };
    let filled = |pts: Vec<Pos2>| {
        painter.add(PathShape::convex_polygon(pts, color, Stroke::NONE));
    };
    let rounded = |r: Rect, radius: f32| {
        painter.rect_stroke(r, radius * k, stroke, egui::StrokeKind::Middle);
    };

    match icon {
        Icon::Add => {
            line(p(8.0, 2.5), p(8.0, 13.5));
            line(p(2.5, 8.0), p(13.5, 8.0));
        }
        Icon::Undo | Icon::Redo => {
            // A hooked arrow; redo is the mirror image.
            let m = |x: f32, y: f32| if icon == Icon::Undo { p(x, y) } else { p(16.0 - x, y) };
            painter.add(CubicBezierShape::from_points_stroke(
                [m(3.5, 6.5), m(9.5, 6.5), m(13.5, 7.0), m(12.5, 12.5)],
                false,
                Color32::TRANSPARENT,
                PathStroke::new(1.6 * k, color),
            ));
            poly(vec![m(6.5, 3.5), m(3.5, 6.5), m(6.5, 9.5)], false);
        }
        Icon::Play => {
            filled(vec![p(4.5, 2.5), p(13.0, 8.0), p(4.5, 13.5)]);
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_min_max(p(3.5, 3.5), p(12.5, 12.5)), 2.0 * k, color);
        }
        Icon::Menu => {
            for y in [4.0, 8.0, 12.0] {
                line(p(2.5, y), p(13.5, y));
            }
        }
        Icon::Close => {
            line(p(4.0, 4.0), p(12.0, 12.0));
            line(p(12.0, 4.0), p(4.0, 12.0));
        }
        Icon::Minimize => line(p(4.0, 11.5), p(12.0, 11.5)),
        Icon::Maximize => rounded(Rect::from_min_max(p(4.0, 4.0), p(12.0, 12.0)), 1.5),
        Icon::Restore => {
            rounded(Rect::from_min_max(p(3.5, 6.0), p(10.0, 12.5)), 1.5);
            poly(vec![p(6.0, 3.5), p(12.5, 3.5), p(12.5, 10.0)], false);
        }
        Icon::Text => {
            line(p(3.0, 3.5), p(13.0, 3.5));
            line(p(8.0, 3.5), p(8.0, 13.0));
            line(p(6.0, 13.0), p(10.0, 13.0));
        }
        Icon::Shape => rounded(Rect::from_min_max(p(2.5, 3.5), p(13.5, 12.5)), 3.0),
        Icon::Picture => {
            rounded(Rect::from_min_max(p(2.0, 3.0), p(14.0, 13.0)), 2.0);
            poly(vec![p(4.0, 11.0), p(7.0, 7.5), p(9.5, 10.0), p(11.0, 8.5), p(12.5, 11.0)], false);
            painter.circle_filled(p(10.5, 5.8), 1.2 * k, color);
        }
        Icon::Microphone => {
            rounded(Rect::from_min_max(p(6.0, 1.8), p(10.0, 9.8)), 2.0);
            painter.add(CubicBezierShape::from_points_stroke(
                [p(3.5, 7.5), p(3.5, 12.5), p(12.5, 12.5), p(12.5, 7.5)],
                false,
                Color32::TRANSPARENT,
                PathStroke::new(1.6 * k, color),
            ));
            line(p(8.0, 11.8), p(8.0, 14.5));
        }
        Icon::Delete => {
            line(p(2.5, 4.0), p(13.5, 4.0));
            poly(vec![p(6.0, 4.0), p(6.5, 2.0), p(9.5, 2.0), p(10.0, 4.0)], false);
            poly(vec![p(4.0, 4.0), p(4.8, 14.0), p(11.2, 14.0), p(12.0, 4.0)], false);
        }
        Icon::More => {
            for x in [3.5, 8.0, 12.5] {
                painter.circle_filled(p(x, 8.0), 1.4 * k, color);
            }
        }
        Icon::Left => chevron(painter, rect, std::f32::consts::PI, color),
        Icon::Up => chevron(painter, rect, -std::f32::consts::FRAC_PI_2, color),
        Icon::Down => chevron(painter, rect, std::f32::consts::FRAC_PI_2, color),
        Icon::Copy => {
            rounded(Rect::from_min_max(p(5.5, 5.5), p(13.5, 13.5)), 1.5);
            poly(vec![p(3.0, 10.5), p(2.5, 10.5), p(2.5, 2.5), p(10.5, 2.5), p(10.5, 3.0)], false);
        }
        Icon::Open => {
            poly(vec![p(2.0, 13.0), p(2.0, 3.0), p(6.0, 3.0), p(7.5, 5.0), p(14.0, 5.0), p(14.0, 13.0)], true);
            line(p(2.0, 7.5), p(14.0, 7.5));
        }
        Icon::Video => {
            rounded(Rect::from_min_max(p(1.5, 4.0), p(10.5, 12.0)), 2.0);
            filled(vec![p(11.0, 8.0), p(14.5, 5.0), p(14.5, 11.0)]);
        }
        Icon::Transition => {
            rounded(Rect::from_min_max(p(1.5, 3.5), p(9.5, 10.5)), 0.8);
            painter.rect_filled(egui::Rect::from_min_max(p(6.5, 5.5), p(14.5, 12.5)), 0.8 * k, color);
        }
        Icon::Previous | Icon::Next => {
            // A bar and a triangle pointing at it; next is the mirror image.
            let m = |x: f32, y: f32| if icon == Icon::Previous { p(x, y) } else { p(16.0 - x, y) };
            line(m(4.0, 3.5), m(4.0, 12.5));
            filled(vec![m(12.5, 3.0), m(6.0, 8.0), m(12.5, 13.0)]);
        }
        Icon::Save => {
            poly(vec![p(2.5, 2.5), p(11.0, 2.5), p(13.5, 5.0), p(13.5, 13.5), p(2.5, 13.5)], true);
            poly(vec![p(5.0, 2.5), p(5.0, 6.0), p(10.0, 6.0), p(10.0, 2.5)], false);
            poly(vec![p(5.0, 13.5), p(5.0, 9.5), p(11.0, 9.5), p(11.0, 13.5)], false);
        }
        Icon::Info => {
            painter.circle_stroke(p(8.0, 8.0), 6.0 * k, stroke);
            line(p(8.0, 7.5), p(8.0, 11.5));
            painter.circle_filled(p(8.0, 5.0), 1.0 * k, color);
        }
        Icon::Keyboard => {
            rounded(Rect::from_min_max(p(1.5, 4.0), p(14.5, 12.0)), 2.0);
            for x in [4.5, 8.0, 11.5] {
                painter.circle_filled(p(x, 7.0), 0.9 * k, color);
            }
            line(p(5.0, 9.8), p(11.0, 9.8));
        }
        Icon::Present => {
            rounded(Rect::from_min_max(p(1.5, 2.5), p(14.5, 11.0)), 1.5);
            filled(vec![p(6.5, 4.5), p(10.5, 6.75), p(6.5, 9.0)]);
            line(p(5.0, 14.0), p(11.0, 14.0));
            line(p(8.0, 11.0), p(8.0, 14.0));
        }
        Icon::Search => {
            painter.circle_stroke(p(7.0, 7.0), 4.5 * k, stroke);
            line(p(10.3, 10.3), p(13.8, 13.8));
        }
        Icon::Paste => {
            poly(vec![p(5.5, 3.5), p(3.0, 3.5), p(3.0, 14.0), p(13.0, 14.0), p(13.0, 3.5), p(10.5, 3.5)], false);
            rounded(Rect::from_min_max(p(5.5, 2.0), p(10.5, 5.0)), 1.0);
        }
        Icon::Front | Icon::Back => {
            let (upper, lower) = (Rect::from_min_max(p(2.0, 2.0), p(10.5, 10.5)), Rect::from_min_max(p(5.5, 5.5), p(14.0, 14.0)));
            let (solid, outline) = if icon == Icon::Front { (upper, lower) } else { (lower, upper) };
            rounded(outline, 1.5);
            painter.rect_filled(solid, 1.5 * k, color);
        }
    }
}
