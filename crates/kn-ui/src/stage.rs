//! The stage: the big slide preview where items are picked, moved and resized.

use crate::KinettoApp;
use crate::canvas::{Frame, Surface};
use egui::{Pos2, Sense, Stroke, Vec2};
use kn_engine::Command;
use kn_geom::Rect;
use std::hash::{Hash, Hasher};

const HANDLE: f32 = 10.0;
/// The selection outline sits this far outside the item, so neither it nor
/// the corner handles cover the item's edges or its first and last letters.
const SELECTION_PAD: f32 = 6.0;
const ROTATION_OFFSET: f32 = 28.0;
const ROTATION_RADIUS: f32 = 11.0;
const MIN_SIZE: f64 = 24.0;

#[derive(Clone, Copy)]
pub(crate) struct Drag {
    kind: DragKind,
    start_frame: Rect,
    start_pointer: Pos2,
    rotation: f64,
    square: bool,
}

#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Rotate,
    /// Which corner is being pulled: (right?, bottom?).
    Resize(bool, bool),
}

/// Maps between slide units and screen points for the shown stage.
struct View {
    origin: Pos2,
    scale: f32,
}

impl View {
    fn to_screen(&self, r: Rect) -> egui::Rect {
        let min = self.origin + Vec2::new(r.x as f32, r.y as f32) * self.scale;
        egui::Rect::from_min_size(min, Vec2::new(r.w as f32, r.h as f32) * self.scale)
    }

    fn to_slide(&self, p: Pos2) -> (f64, f64) {
        let v = (p - self.origin) / self.scale;
        (f64::from(v.x), f64::from(v.y))
    }
}

impl KinettoApp {
    pub(crate) fn stage(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = crate::theme::palette(ui.ctx());
        let (cw, ch) = self.project().aspect.canvas_size();
        // Room around the slide, and a status line beneath it.
        const STATUS: f32 = 28.0;
        let area = ui.max_rect().shrink2(Vec2::new(40.0, 32.0));
        let area = egui::Rect::from_min_max(area.min, area.max - Vec2::new(0.0, STATUS));
        let fit = (area.width() / cw as f32).min(area.height() / ch as f32).max(0.01);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let rect = egui::Rect::from_center_size(area.center(), size);
        self.status_line(ui, egui::pos2(rect.center().x, rect.bottom() + 12.0));
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let view = View { origin: rect.min, scale: fit };
        let time = if self.playback.playing { self.playback.time } else { self.edit_time() };

        let content = self.content_stamp();
        let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) else {
            no_gpu_page(ui, rect);
            return;
        };
        let stamp = (!self.playback.playing).then(|| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (content, time, size.x.to_bits(), size.y.to_bits()).hash(&mut h);
            h.finish()
        });
        let ppp = ui.ctx().pixels_per_point();
        let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
        let frame = Frame { project: self.session.project(), media: &self.session.media, time, size: px, stamp };
        let id = canvas.paint(rs, Surface::Stage, frame, &mut self.render);
        let shadow = egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(if ui.visuals().dark_mode { 90 } else { 40 }) };
        ui.painter().add(shadow.as_shape(rect, 8));
        egui::Image::new((id, size)).corner_radius(8).paint_at(ui, rect);
        ui.painter().rect_stroke(rect, 8, egui::Stroke::new(1.0, p.border), egui::StrokeKind::Outside);
        if !self.playback.playing && self.project().slides[self.slide].elements.is_empty() {
            empty_hint(ui, rect);
        }

        if self.playback.playing {
            return;
        }
        if !self.rotation_handle(ui, &view) {
            self.handle_pointer(ui, &response, &view);
        }
        self.handle_keys(ui);
        self.draw_selection(ui, &view);
    }

    /// An independent target keeps the handle usable outside the slide.
    fn rotation_handle(&mut self, ui: &mut egui::Ui, view: &View) -> bool {
        let Some(index) = self.selected else { return false };
        let element = &self.project().slides[self.slide].elements[index];
        if !matches!(element.kind, kn_model::ElementKind::Image { .. } | kn_model::ElementKind::Shape { .. }) { return false; }
        let rotation = element.rotation_radians();
        let r = self.selection_rect(view, index);
        let top = rotate_screen(r.center_top(), r.center(), rotation);
        let centre = rotation_handle_position(r, rotation);
        let target = egui::Rect::from_center_size(centre, Vec2::splat(ROTATION_RADIUS * 2.0 + 8.0));
        let response = ui.interact(target, ui.scope_id().with(("object-rotation", self.slide, index)), Sense::click_and_drag());
        let angle = self.project().slides[self.slide].elements[index].rotation;
        response.widget_info(|| egui::WidgetInfo::slider(true, angle, "Rotate selected object"));
        if response.drag_started() {
            self.session.seal();
            self.drag = Some(Drag {
                kind: DragKind::Rotate,
                start_frame: self.project().slides[self.slide].elements[index].frame,
                start_pointer: response.interact_pointer_pos().unwrap_or(centre) - response.drag_delta(),
                rotation,
                square: false,
            });
        }
        if response.dragged()
            && let (Some(drag), Some(pointer)) = (self.drag, response.interact_pointer_pos())
            && matches!(drag.kind, DragKind::Rotate)
        {
            let origin = view.to_screen(drag.start_frame).center();
            if pointer.distance(origin) > 4.0 {
                let mut element = self.project().slides[self.slide].elements[index].clone();
                element.rotation = dragged_rotation(drag.rotation, drag.start_pointer - origin, pointer - origin, ui.input(|i| i.modifiers.shift));
                self.run(Command::UpdateElement { slide: self.slide, index, element });
            }
        }
        if response.has_focus() {
            let step = ui.input(|i| {
                let direction = i.key_pressed(egui::Key::ArrowRight) as i8 - i.key_pressed(egui::Key::ArrowLeft) as i8;
                f64::from(direction) * if i.modifiers.shift { 15.0 } else { 1.0 }
            });
            if step != 0.0 {
                let mut element = self.project().slides[self.slide].elements[index].clone();
                element.rotation = normalized_rotation(element.rotation + step);
                self.run(Command::UpdateElement { slide: self.slide, index, element });
                self.session.seal();
            }
        }
        if response.drag_stopped() {
            self.drag = None;
            self.session.seal();
        }
        let p = crate::theme::palette(ui.ctx());
        let accent = p.accent_bg;
        ui.painter().line_segment([top, centre], Stroke::new(1.5, accent));
        ui.painter().circle(centre, ROTATION_RADIUS, p.card, Stroke::new(1.5, accent));
        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(centre, Vec2::splat(16.0)), crate::icons::Icon::Undo, p.fg);
        if response.hovered() || response.has_focus() || response.dragged() {
            ui.painter().circle_stroke(centre, ROTATION_RADIUS + 3.0, Stroke::new(2.0, accent));
        }
        if response.dragged() {
            let angle = self.project().slides[self.slide].elements[index].rotation;
            ui.painter().text(centre + Vec2::new(19.0, 0.0), egui::Align2::LEFT_CENTER, format!("{angle:.1}°"), egui::FontId::proportional(12.0), p.fg);
        }
        let handled = response.hovered() || response.dragged() || response.drag_stopped() || response.clicked();
        response.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to rotate. Hold Shift for 15° steps. Arrow keys adjust the angle.");
        handled
    }

    fn bounds(&mut self, index: usize) -> Rect {
        let element = &self.session.project().slides[self.slide].elements[index];
        kn_render::element_bounds(element, &mut self.render.text)
    }

    fn hit(&mut self, (x, y): (f64, f64)) -> Option<usize> {
        let count = self.project().slides[self.slide].elements.len();
        (0..count).rev().find(|&i| {
            let r = self.bounds(i);
            let angle = -self.project().slides[self.slide].elements[i].rotation_radians();
            let (x, y) = rotate_point((x, y), (r.x + r.w / 2.0, r.y + r.h / 2.0), angle);
            x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
        })
    }

    /// The corner handle under `p`, if the selected item has one there.
    fn handle_at(&mut self, view: &View, p: Pos2) -> Option<DragKind> {
        let index = self.selected?;
        let r = self.selection_rect(view, index);
        let angle = self.project().slides[self.slide].elements[index].rotation_radians();
        [(false, false), (true, false), (false, true), (true, true)].into_iter().find_map(|(right, bottom)| {
            let corner = Pos2::new(if right { r.max.x } else { r.min.x }, if bottom { r.max.y } else { r.min.y });
            let corner = rotate_screen(corner, r.center(), angle);
            (corner.distance(p) <= HANDLE).then_some(DragKind::Resize(right, bottom))
        })
    }

    fn handle_pointer(&mut self, ui: &egui::Ui, response: &egui::Response, view: &View) {
        if let Some(pos) = response.hover_pos() {
            let icon = match self.handle_at(view, pos) {
                Some(DragKind::Resize(r, b)) if r == b => egui::CursorIcon::ResizeNwSe,
                Some(_) => egui::CursorIcon::ResizeNeSw,
                None if self.hit(view.to_slide(pos)).is_some() => egui::CursorIcon::Grab,
                None => egui::CursorIcon::Default,
            };
            ui.ctx().set_cursor_icon(icon);
        }

        if response.drag_started()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let kind = self.handle_at(view, pos).or_else(|| {
                self.selected = self.hit(view.to_slide(pos));
                self.selected.map(|_| DragKind::Move)
            });
            self.drag = kind.zip(self.selected).map(|(kind, i)| Drag {
                kind,
                start_frame: self.project().slides[self.slide].elements[i].frame,
                start_pointer: pos,
                rotation: self.project().slides[self.slide].elements[i].rotation_radians(),
                square: matches!(self.project().slides[self.slide].elements[i].kind, kn_model::ElementKind::Shape { shape: kn_model::ShapeKind::Square, .. }),
            });
        } else if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selected = self.hit(view.to_slide(pos));
        }

        if let (Some(drag), Some(index), Some(pos)) = (self.drag, self.selected, response.interact_pointer_pos())
            && response.dragged()
        {
            let d = (pos - drag.start_pointer) / view.scale;
            let frame = drag_frame(drag, f64::from(d.x), f64::from(d.y));
            self.run(Command::SetFrame { slide: self.slide, index, frame });
        }
        if response.drag_stopped() {
            self.drag = None;
            self.session.seal();
        }
    }

    fn handle_keys(&mut self, ui: &egui::Ui) {
        let Some(index) = self.selected else { return };
        let rotation_id = ui.scope_id().with(("object-rotation", self.slide, index));
        if ui.memory(|memory| memory.has_focus(rotation_id)) { return; }
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (delete, escape, dx, dy, shift) = ui.input(|i| {
            let step = |neg, pos| f64::from(i.key_pressed(pos) as i8 - i.key_pressed(neg) as i8);
            (
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::Escape),
                step(egui::Key::ArrowLeft, egui::Key::ArrowRight),
                step(egui::Key::ArrowUp, egui::Key::ArrowDown),
                i.modifiers.shift,
            )
        });
        if delete {
            self.delete_selected(index);
        } else if escape {
            self.selected = None;
        } else if dx != 0.0 || dy != 0.0 {
            let step = if shift { 50.0 } else { 10.0 };
            let mut frame = self.project().slides[self.slide].elements[index].frame;
            frame.x += dx * step;
            frame.y += dy * step;
            self.run(Command::SetFrame { slide: self.slide, index, frame });
        }
    }

    /// The selected item's outline on screen, padded out from its bounds.
    fn selection_rect(&mut self, view: &View, index: usize) -> egui::Rect {
        view.to_screen(self.bounds(index)).expand(SELECTION_PAD)
    }

    fn draw_selection(&mut self, ui: &egui::Ui, view: &View) {
        let Some(index) = self.selected else { return };
        let r = self.selection_rect(view, index);
        let accent = crate::theme::palette(ui.ctx()).accent_bg;
        let painter = ui.painter();
        let angle = self.project().slides[self.slide].elements[index].rotation_radians();
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].map(|p| rotate_screen(p, r.center(), angle));
        painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(2.0, accent)));
        for corner in corners {
            let handle = egui::Rect::from_center_size(corner, Vec2::splat(HANDLE));
            painter.rect(handle, 2, egui::Color32::WHITE, Stroke::new(2.0, accent), egui::StrokeKind::Middle);
        }
    }
}

impl KinettoApp {
    /// "Slide 2 of 5 · 0:03 / 0:14" under the slide.
    fn status_line(&self, ui: &egui::Ui, at: egui::Pos2) {
        let p = crate::theme::palette(ui.ctx());
        let total = self.project().total_duration().as_secs_f64();
        let now = if self.playback.playing { self.playback.time } else { self.slide_start(self.slide) }.as_secs_f64();
        let clock = |s: f64| format!("{}:{:02}", s as u64 / 60, s as u64 % 60);
        let text = format!("Slide {} of {}   ·   {} / {}", self.slide + 1, self.project().slides.len(), clock(now), clock(total));
        ui.painter().text(at, egui::Align2::CENTER_TOP, text, egui::FontId::proportional(12.5), p.dim);
    }
}

/// Shown instead of the slide when the graphics card can't be used.
fn no_gpu_page(ui: &egui::Ui, rect: egui::Rect) {
    let p = crate::theme::palette(ui.ctx());
    ui.painter().rect_filled(rect, 8, p.button[0]);
    let lines = [
        ("Kinetto can't draw on this computer", 18.0, crate::theme::bold(), p.fg),
        ("It needs a graphics card that supports Vulkan, Metal or DirectX 12.", 13.5, egui::FontFamily::Proportional, p.dim),
        ("Updating your graphics drivers usually fixes this; then restart Kinetto.", 13.5, egui::FontFamily::Proportional, p.dim),
    ];
    for (i, (text, size, family, color)) in lines.into_iter().enumerate() {
        let y = rect.center().y - 28.0 + i as f32 * 26.0;
        ui.painter().text(egui::pos2(rect.center().x, y), egui::Align2::CENTER_CENTER, text, egui::FontId::new(size, family), color);
    }
}

/// What to do on an empty slide.
fn empty_hint(ui: &egui::Ui, rect: egui::Rect) {
    let color = egui::Color32::from_gray(128);
    ui.painter().text(rect.center() - Vec2::new(0.0, 12.0), egui::Align2::CENTER_CENTER, "This slide is empty", egui::FontId::new(18.0, crate::theme::bold()), color);
    ui.painter().text(rect.center() + Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, "Add text, a shape or a picture with +, or drop files here", egui::FontId::proportional(13.5), color);
}

fn rotation_handle_position(rect: egui::Rect, rotation: f64) -> Pos2 {
    rotate_screen(rect.center_top() - Vec2::new(0.0, ROTATION_OFFSET), rect.center(), rotation)
}

fn normalized_rotation(degrees: f64) -> f64 { (degrees + 180.0).rem_euclid(360.0) - 180.0 }

fn dragged_rotation(start: f64, from: Vec2, to: Vec2, snap: bool) -> f64 {
    let delta = f64::from(to.y).atan2(f64::from(to.x)) - f64::from(from.y).atan2(f64::from(from.x));
    let degrees = (start + delta).to_degrees();
    normalized_rotation(if snap { (degrees / 15.0).round() * 15.0 } else { degrees })
}

fn rotate_point((x, y): (f64, f64), (cx, cy): (f64, f64), angle: f64) -> (f64, f64) {
    let (sin, cos) = angle.sin_cos();
    (cx + cos * (x - cx) - sin * (y - cy), cy + sin * (x - cx) + cos * (y - cy))
}

fn rotate_screen(point: Pos2, centre: Pos2, angle: f64) -> Pos2 {
    let (x, y) = rotate_point((f64::from(point.x), f64::from(point.y)), (f64::from(centre.x), f64::from(centre.y)), angle);
    Pos2::new(x as f32, y as f32)
}

/// The new frame after dragging by (dx, dy) slide units.
fn drag_frame(drag: Drag, dx: f64, dy: f64) -> Rect {
    let f = drag.start_frame;
    match drag.kind {
        DragKind::Move => Rect { x: f.x + dx, y: f.y + dy, ..f },
        DragKind::Rotate => f,
        DragKind::Resize(right, bottom) => {
            // Resize in the element's local axes; translate its centre so
            // the opposite corner stays fixed in slide coordinates.
            let (sin, cos) = drag.rotation.sin_cos();
            let (dx, dy) = (cos * dx + sin * dy, -sin * dx + cos * dy);
            let mut w = (f.w + if right { dx } else { -dx }).max(MIN_SIZE);
            let mut h = (f.h + if bottom { dy } else { -dy }).max(MIN_SIZE);
            if drag.square {
                let (dw, dh) = (w - f.w, h - f.h);
                let delta = if dw.abs() > dh.abs() { dw } else { dh };
                w = (f.w + delta).max(MIN_SIZE);
                h = w;
            }
            let local_x = (w - f.w) * if right { 0.5 } else { -0.5 };
            let local_y = (h - f.h) * if bottom { 0.5 } else { -0.5 };
            let centre_x = f.x + f.w / 2.0 + cos * local_x - sin * local_y;
            let centre_y = f.y + f.h / 2.0 + sin * local_x + cos * local_y;
            Rect::new(centre_x - w / 2.0, centre_y - h / 2.0, w, h)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(kind: DragKind) -> Drag {
        Drag { kind, start_frame: Rect::new(100.0, 100.0, 200.0, 100.0), start_pointer: Pos2::ZERO, rotation: 0.0, square: false }
    }

    #[test]
    fn rotation_handle_follows_the_top_edge() {
        let r = egui::Rect::from_min_size(egui::pos2(100., 100.), Vec2::new(200., 100.));
        assert_eq!(rotation_handle_position(r, 0.), egui::pos2(200., 72.));
        let p = rotation_handle_position(r, std::f64::consts::FRAC_PI_2);
        assert!(p.distance(egui::pos2(278., 150.)) < 0.001);
        assert_eq!(dragged_rotation(0., Vec2::new(0., -1.), Vec2::new(1., 0.), false), 90.);
        let angle = 37_f32.to_radians();
        let to = Vec2::new(angle.sin(), -angle.cos());
        assert!((dragged_rotation(0., Vec2::new(0., -1.), to, false) - 37.).abs() < 0.001);
        assert_eq!(dragged_rotation(0., Vec2::new(0., -1.), to, true), 30.);
    }

    #[test]
    fn dragging_rotation_handle_rotates_images_and_shapes_in_one_undo_step() {
        use egui_kittest::{Harness, kittest::Queryable};
        for image in [false, true] {
            let mut app = Harness::builder().build_eframe(|cc| KinettoApp::new(cc)).into_state();
            let frame = Rect::new(100., 100., 200., 100.);
            let element = if image { kn_model::Element::image("test".into(), frame) }
                else { kn_model::Element::shape(frame, kn_geom::Color::rgb(50, 100, 200)) };
            let mut project = kn_model::Project::sample();
            project.slides[0].elements = vec![element];
            app.session = kn_engine::Session::new(project);
            app.selected = Some(0);
            let mut h = Harness::builder().with_size([500., 400.]).build_ui_state(|ui, app: &mut KinettoApp| {
                let view = View { origin: Pos2::ZERO, scale: 1.0 };
                app.rotation_handle(ui, &view);
                app.handle_keys(ui);
            }, app);
            h.run_steps(4);
            let start = h.get_by_label("Rotate selected object").rect().center();
            h.drag_at(start);
            h.run_steps(2);
            h.hover_at(egui::pos2(278., 150.));
            h.run_steps(4);
            h.drop_at(egui::pos2(278., 150.));
            h.run_steps(4);
            let e = &h.state().project().slides[0].elements[0];
            assert!((e.rotation - 90.).abs() < 0.01, "got {}", e.rotation);
            assert_eq!(e.frame, frame);
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().slides[0].elements[0].rotation, 0.);
            assert!(!h.state_mut().session.undo());
            h.run_steps(4);
            h.get_by_label("Rotate selected object").focus();
            h.key_press(egui::Key::ArrowRight);
            h.run_steps(4);
            assert_eq!(h.state().project().slides[0].elements[0].rotation, 1.);
            assert_eq!(h.state().project().slides[0].elements[0].frame, frame);
        }
    }

    #[test]
    fn move_keeps_size() {
        assert_eq!(drag_frame(drag(DragKind::Move), 10.0, -5.0), Rect::new(110.0, 95.0, 200.0, 100.0));
    }

    #[test]
    fn resize_keeps_opposite_corner() {
        let r = drag_frame(drag(DragKind::Resize(false, false)), 50.0, 20.0);
        assert_eq!((r.x + r.w, r.y + r.h), (300.0, 200.0));
        assert_eq!((r.x, r.y), (150.0, 120.0));
    }

    #[test]
    fn rotated_resize_keeps_the_opposite_corner_fixed() {
        for angle in [45.0_f64, 90., -30.] {
            for (right, bottom) in [(true, true), (true, false), (false, true), (false, false)] {
                let mut drag = drag(DragKind::Resize(right, bottom));
                drag.rotation = angle.to_radians();
                let anchor = |r: Rect| rotate_point(
                    (if right { r.x } else { r.x + r.w }, if bottom { r.y } else { r.y + r.h }),
                    (r.x + r.w / 2., r.y + r.h / 2.), drag.rotation);
                let before = anchor(drag.start_frame);
                let after = anchor(drag_frame(drag, 50., 20.));
                assert!((before.0 - after.0).abs() < 1e-9 && (before.1 - after.1).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn square_stays_square_when_resized_at_45_degrees() {
        let mut drag = drag(DragKind::Resize(true, true));
        drag.start_frame.h = drag.start_frame.w;
        drag.square = true;
        drag.rotation = std::f64::consts::FRAC_PI_4;
        let r = drag_frame(drag, 60., -20.);
        assert_eq!(r.w, r.h);
    }

    #[test]
    fn resize_never_inverts() {
        let r = drag_frame(drag(DragKind::Resize(true, true)), -500.0, -500.0);
        assert_eq!((r.w, r.h), (MIN_SIZE, MIN_SIZE));
    }
}
