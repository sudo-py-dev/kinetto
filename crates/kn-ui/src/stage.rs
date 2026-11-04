//! The stage: the big slide preview where items are picked, moved and resized.

use crate::KinettoApp;
use crate::canvas::{Frame, Surface};
use egui::{Pos2, Sense, Stroke, Vec2};
use kn_engine::Command;
use kn_geom::Rect;
use kn_model::ElementKind;
use std::hash::{Hash, Hasher};

const HANDLE: f32 = 10.0;
const MIN_SIZE: f64 = 24.0;

#[derive(Clone, Copy)]
pub(crate) struct Drag {
    kind: DragKind,
    start_frame: Rect,
    start_pointer: Pos2,
}

#[derive(Clone, Copy)]
enum DragKind {
    Move,
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
        let (cw, ch) = self.project().aspect.canvas_size();
        let avail = ui.available_size() - Vec2::splat(32.0);
        let fit = (avail.x / cw as f32).min(avail.y / ch as f32).max(0.01);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let rect = egui::Rect::from_center_size(ui.max_rect().center(), size);
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let view = View { origin: rect.min, scale: fit };
        let time = if self.playing { self.play_time } else { self.edit_time() };

        let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) else {
            ui.put(rect, egui::Label::new("This device can't start the GPU canvas."));
            return;
        };
        let stamp = (!self.playing).then(|| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.session.revision(), time, size.x.to_bits(), size.y.to_bits()).hash(&mut h);
            h.finish()
        });
        let ppp = ui.ctx().pixels_per_point();
        let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
        let id = canvas.paint(rs, Surface::Stage, Frame { project: self.session.project(), time, size: px, stamp }, &mut self.text);
        egui::Image::new((id, size)).corner_radius(10).paint_at(ui, rect);

        if self.playing {
            return;
        }
        self.handle_pointer(ui, &response, &view);
        self.handle_keys(ui);
        self.draw_selection(ui, &view);
    }

    fn bounds(&mut self, index: usize) -> Rect {
        let element = &self.session.project().slides[self.slide].elements[index];
        kn_render::element_bounds(element, &mut self.text)
    }

    fn hit(&mut self, (x, y): (f64, f64)) -> Option<usize> {
        let count = self.project().slides[self.slide].elements.len();
        (0..count).rev().find(|&i| {
            let r = self.bounds(i);
            x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
        })
    }

    /// The corner handle under `p`, if the selected item has one there.
    fn handle_at(&mut self, view: &View, p: Pos2) -> Option<DragKind> {
        let index = self.selected?;
        let r = view.to_screen(self.bounds(index));
        [(false, false), (true, false), (false, true), (true, true)].into_iter().find_map(|(right, bottom)| {
            let corner = Pos2::new(if right { r.max.x } else { r.min.x }, if bottom { r.max.y } else { r.min.y });
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
            self.run(Command::DeleteElement { slide: self.slide, index });
            self.session.seal();
            self.selected = None;
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

    fn draw_selection(&mut self, ui: &egui::Ui, view: &View) {
        let Some(index) = self.selected else { return };
        let r = view.to_screen(self.bounds(index));
        let accent = ui.visuals().selection.bg_fill;
        let painter = ui.painter();
        painter.rect_stroke(r, 2, Stroke::new(2.0, accent), egui::StrokeKind::Outside);
        for corner in [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()] {
            let handle = egui::Rect::from_center_size(corner, Vec2::splat(HANDLE));
            painter.rect(handle, 2, egui::Color32::WHITE, Stroke::new(2.0, accent), egui::StrokeKind::Middle);
        }
        let is_text = matches!(self.project().slides[self.slide].elements[index].kind, ElementKind::Text { .. });
        if is_text {
            painter.text(
                r.center_top() - Vec2::new(0.0, 8.0),
                egui::Align2::CENTER_BOTTOM,
                "Edit the words on the right →",
                egui::FontId::proportional(12.0),
                accent,
            );
        }
    }
}

/// The new frame after dragging by (dx, dy) slide units.
fn drag_frame(drag: Drag, dx: f64, dy: f64) -> Rect {
    let f = drag.start_frame;
    match drag.kind {
        DragKind::Move => Rect { x: f.x + dx, y: f.y + dy, ..f },
        DragKind::Resize(right, bottom) => {
            let (x0, x1) = if right { (f.x, (f.x + f.w + dx).max(f.x + MIN_SIZE)) } else { ((f.x + dx).min(f.x + f.w - MIN_SIZE), f.x + f.w) };
            let (y0, y1) = if bottom { (f.y, (f.y + f.h + dy).max(f.y + MIN_SIZE)) } else { ((f.y + dy).min(f.y + f.h - MIN_SIZE), f.y + f.h) };
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(kind: DragKind) -> Drag {
        Drag { kind, start_frame: Rect::new(100.0, 100.0, 200.0, 100.0), start_pointer: Pos2::ZERO }
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
    fn resize_never_inverts() {
        let r = drag_frame(drag(DragKind::Resize(true, true)), -500.0, -500.0);
        assert_eq!((r.w, r.h), (MIN_SIZE, MIN_SIZE));
    }
}
