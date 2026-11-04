//! The sidebar: a live thumbnail per slide, slide actions, and New slide.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::{self, Icon};
use crate::theme::{self, palette};
use egui::{RichText, Sense, Stroke, Vec2};
use kn_engine::Command;

/// Width of the slide thumbnails in the sidebar.
const THUMB_WIDTH: f32 = 180.0;

/// One drawn thumbnail, for working out drags across the whole list.
struct Thumb {
    /// The slide picture on screen.
    rect: egui::Rect,
    response: egui::Response,
    /// The rendered picture, for the copy that follows the pointer.
    texture: Option<egui::TextureId>,
}

/// Where slide `from` lands if dropped at height `y`, given every
/// thumbnail's rectangle: its index once the list is reordered, counting
/// the other slides whose middle is above `y`.
fn drop_target(rects: &[egui::Rect], from: usize, y: f32) -> usize {
    rects.iter().enumerate().filter(|(i, r)| *i != from && y > r.center().y).count()
}

impl KinettoApp {
    /// The Simple layout's sidebar: slide thumbnails with transition buttons
    /// between them and New slide at the bottom.
    pub(crate) fn slide_strip(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = palette(ui.ctx());
        let count = self.project().slides.len();
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            canvas.retain_thumbnails(rs, count);
        }
        egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 8 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Slides").family(theme::bold()).size(14.0));
                ui.label(RichText::new(count.to_string()).color(p.dim));
            });
        });
        let footer = 56.0;
        egui::ScrollArea::vertical().max_height(ui.available_height() - footer).auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 0)).show(ui, |ui| {
                let mut thumbs = Vec::with_capacity(count);
                for index in 0..count {
                    if index > 0 {
                        self.transition_button(ui, index);
                    }
                    thumbs.push(self.thumbnail(ui, frame, index));
                    // A thumbnail's menu can insert, remove or move a slide.
                    if self.project().slides.len() != count {
                        ui.ctx().request_discard("slide list changed");
                        self.slide_drag = None;
                        return;
                    }
                }
                self.drag_slides(ui, &thumbs);
            });
        });
        egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 10)).show(ui, |ui| {
            let response = ui.add_sized([THUMB_WIDTH, 34.0], |ui: &mut egui::Ui| adw::button(ui, Some(Icon::Add), "New slide", Kind::Normal));
            egui::Popup::menu(&response).width(THUMB_WIDTH).show(|ui| {
                ui.label(RichText::new("Layout").size(12.0).color(p.dim));
                for layout in kn_layout::Layout::ALL {
                    if ui.add(egui::Button::new(layout.name()).frame(false).min_size(Vec2::new(THUMB_WIDTH - 12.0, 30.0))).clicked() {
                        self.run(Command::AddLayoutSlide { at: self.slide + 1, layout });
                        self.session.seal();
                        self.select_slide(self.slide + 1);
                        ui.close();
                    }
                }
            });
        });
    }

    /// The animation belongs to the slide below this gap.
    fn transition_button(&mut self, ui: &mut egui::Ui, index: usize) {
        ui.push_id(("between-slides", self.generation, index), |ui| {
            ui.allocate_ui_with_layout(Vec2::new(THUMB_WIDTH, 34.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_width(THUMB_WIDTH);
                let current = self.project().slides[index].transition;
                let label = format!("Animation between slides {} and {}", index, index + 1);
                // A transition glyph, not a chevron: this is a choice of
                // animation, and "Slide" alone would read like a menu of slides.
                let response = adw::button(ui, Some(Icon::Transition), current.name(), Kind::Normal);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let response = response.on_hover_text(format!("Transition into slide {}: {}. Click to change.", index + 1, current.name()));
                egui::Popup::menu(&response).width(264.0).show(|ui| {
                    ui.label(RichText::new("Slide transition").family(theme::bold()).size(15.0));
                    ui.label(RichText::new(format!("Into slide {} · Hover to preview", index + 1)).size(12.0).color(palette(ui.ctx()).dim));
                    ui.add_space(6.0);
                    if let Some(transition) = crate::transitions::picker(ui, current) {
                        self.choose_transition(index, transition);
                        ui.close();
                    }
                });
            });
        });
    }

    /// Reorders slides by dragging their thumbnails: the grabbed slide dims
    /// and a copy follows the pointer, a line marks where it will land, and
    /// the list scrolls near its edges. Dropping is one undo step; Escape or
    /// letting go outside cancels nothing but the move itself.
    fn drag_slides(&mut self, ui: &egui::Ui, thumbs: &[Thumb]) {
        if let Some(from) = thumbs.iter().position(|t| t.response.drag_started()) {
            self.select_slide(from);
            self.slide_drag = Some(from);
        }
        let Some(from) = self.slide_drag.filter(|&from| from < thumbs.len()) else {
            self.slide_drag = None;
            return;
        };
        // Escape cancels. So does the window losing focus (say, alt-tab):
        // the button-up then never arrives, and the drag would stay stuck.
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || !i.focused || i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false)))) {
            self.slide_drag = None;
            return;
        }
        let released = thumbs[from].response.drag_stopped() || !ui.input(|i| i.pointer.primary_down());
        let Some(pointer) = ui.ctx().pointer_latest_pos() else {
            // Let go with the pointer outside the window: there is nowhere to
            // drop, so the move is cancelled rather than left waiting.
            if released {
                self.slide_drag = None;
            }
            return;
        };
        let rects: Vec<egui::Rect> = thumbs.iter().map(|t| t.rect).collect();
        let to = drop_target(&rects, from, pointer.y);
        if released {
            self.slide_drag = None;
            if to != from {
                self.run(Command::MoveSlide { from, to });
                self.session.seal();
                self.select_slide(to);
                self.notify_undo(format!("Moved slide {} to place {}", from + 1, to + 1));
            }
            return;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let p = palette(ui.ctx());
        // Where it will land: a line in the gap between two other slides.
        if to != from {
            let others: Vec<egui::Rect> = rects.iter().enumerate().filter(|(i, _)| *i != from).map(|(_, r)| *r).collect();
            // Just under the caption of the slide above, clear of the
            // transition button that follows it.
            let y = match to {
                0 => others[0].top() - 7.0,
                n => others[n - 1].bottom() + 25.0,
            };
            let x = others[0].x_range();
            ui.painter().line_segment([egui::pos2(x.min - 4.0, y), egui::pos2(x.max + 4.0, y)], Stroke::new(3.0, p.accent_bg));
            ui.painter().circle_filled(egui::pos2(x.min - 4.0, y), 5.0, p.accent_bg);
        }
        // A lifted copy of the slide follows the pointer, above everything.
        let grabbed = &thumbs[from];
        if let Some(texture) = grabbed.texture {
            // Smaller, and below-right of the pointer, so the line stays in view.
            let size = grabbed.rect.size() * 0.6;
            let ghost = egui::Rect::from_min_size(pointer + Vec2::new(16.0, 14.0), size);
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("slide-drag-ghost")));
            painter.rect_filled(ghost.translate(Vec2::new(0.0, 4.0)), 10, egui::Color32::from_black_alpha(50));
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.add(egui::epaint::RectShape::filled(ghost, 8, egui::Color32::WHITE).with_texture(texture, uv));
            painter.rect_stroke(ghost, 8, Stroke::new(2.0, p.accent_bg), egui::StrokeKind::Outside);
        }
        // Scroll when held near the top or bottom of the list.
        let visible = ui.clip_rect();
        let edge = 48.0;
        let push = if pointer.y < visible.top() + edge { 12.0 } else if pointer.y > visible.bottom() - edge { -12.0 } else { 0.0 };
        if push != 0.0 {
            ui.scroll_with_delta(Vec2::new(0.0, push));
        }
        ui.ctx().request_repaint();
    }

    /// Slide `index`'s thumbnail with its number and length: click to select
    /// it, drag to move it, right-click for the slide menu.
    fn thumbnail(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, index: usize) -> Thumb {
        let p = palette(ui.ctx());
        let (cw, ch) = self.project().aspect.canvas_size();
        // Tall slides would make a very long list; cap their height.
        let height = (THUMB_WIDTH * (ch / cw) as f32).min(THUMB_WIDTH * 1.1);
        let width = height * (cw / ch) as f32;
        let (row, response) = ui.allocate_exact_size(Vec2::new(THUMB_WIDTH, height + 26.0), Sense::click_and_drag());
        let rect = egui::Rect::from_min_size(egui::pos2(row.center().x - width / 2.0, row.top() + 4.0), Vec2::new(width, height - 8.0));
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, index == self.slide, format!("Slide {}", index + 1)));

        let start = self.slide_start(index);
        let slide = &self.project().slides[index];
        let time = start + slide.rest_time();
        let caption = format!("{}  ·  {:.1} s{}", index + 1, slide.duration.as_secs_f64(), if slide.voice.is_some() { "  ·  voice" } else { "" });
        let ppp = ui.ctx().pixels_per_point();
        let px = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let stamp = Some(self.content_stamp());
        let lifted = self.slide_drag == Some(index);
        let mut texture = None;
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let (project, media) = (self.session.project(), &self.session.media);
            let id = canvas.paint(rs, Surface::Thumbnail(index), Frame { project, media, time, size: px, stamp }, &mut self.render);
            // The slide being dragged stays in place, faded, until dropped.
            let tint = if lifted { egui::Color32::from_white_alpha(90) } else { egui::Color32::WHITE };
            egui::Image::new((id, rect.size())).corner_radius(8).tint(tint).paint_at(ui, rect);
            texture = Some(id);
        }

        let selected = index == self.slide;
        if selected {
            ui.painter().rect_stroke(rect.expand(3.0), 11, Stroke::new(2.5, p.accent_bg), egui::StrokeKind::Middle);
        } else {
            ui.painter().rect_stroke(rect, 8, Stroke::new(1.0, p.border), egui::StrokeKind::Outside);
        }
        let caption_color = if selected { p.accent } else { p.dim };
        ui.painter().text(egui::pos2(rect.left() + 2.0, rect.bottom() + 6.0), egui::Align2::LEFT_TOP, caption, egui::FontId::proportional(12.0), caption_color);

        if response.clicked() {
            self.select_slide(index);
        }
        // A visible ⋯ button opens the same menu as right-click.
        let more_rect = egui::Rect::from_min_size(egui::pos2(rect.right() - 30.0, rect.top() + 4.0), Vec2::splat(26.0));
        let more = ui.interact(more_rect, ui.scope_id().with(("slide-more", index)), Sense::click());
        if response.hovered() || more.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&more)) {
            ui.painter().rect_filled(more_rect, 13, p.toast.gamma_multiply(0.85));
            icons::paint(ui.painter(), more_rect.shrink(5.0), Icon::More, egui::Color32::WHITE);
        }
        egui::Popup::menu(&more).width(180.0).show(|ui| self.slide_menu(ui, index));
        egui::Popup::context_menu(&response).width(180.0).show(|ui| self.slide_menu(ui, index));
        let response = if self.slide_drag.is_none() {
            response.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Click to edit. Drag to move it.")
        } else {
            response
        };
        Thumb { rect, response, texture }
    }

    /// Duplicate, Move up, Move down and Delete slide for slide `index`.
    pub(crate) fn slide_menu(&mut self, ui: &mut egui::Ui, index: usize) {
        let last = self.project().slides.len() - 1;
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool| ui.add_enabled_ui(enabled, |ui| crate::header::menu_item(ui, icon, label, None)).inner;
        if item(ui, Icon::Copy, "Duplicate", true) {
            self.run(Command::DuplicateSlide { index });
            self.select_slide(index + 1);
        }
        if item(ui, Icon::Up, "Move up", index > 0) {
            self.run(Command::MoveSlide { from: index, to: index - 1 });
            self.select_slide(index - 1);
        }
        if item(ui, Icon::Down, "Move down", index < last) {
            self.run(Command::MoveSlide { from: index, to: index + 1 });
            self.select_slide(index + 1);
        }
        ui.separator();
        let delete = ui.add_enabled(last > 0, egui::Button::new(RichText::new("Delete slide").color(palette(ui.ctx()).destructive)).frame(false).min_size(Vec2::new(168.0, 30.0)));
        let delete = if last == 0 { delete.on_disabled_hover_text("A video needs at least one slide") } else { delete };
        if delete.clicked() {
            self.run(Command::DeleteSlide { index });
            self.session.seal();
            self.select_slide(index.saturating_sub(1));
            self.notify_undo(format!("Slide {} deleted", index + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_target_counts_the_other_slides_above_the_pointer() {
        let thumbs: Vec<egui::Rect> = (0..4).map(|i| egui::Rect::from_min_size(egui::pos2(0.0, i as f32 * 100.0), Vec2::new(100.0, 80.0))).collect();
        // Middles are at 40, 140, 240 and 340.
        assert_eq!(drop_target(&thumbs, 0, 10.0), 0);
        assert_eq!(drop_target(&thumbs, 0, 150.0), 1);
        assert_eq!(drop_target(&thumbs, 0, 999.0), 3);
        assert_eq!(drop_target(&thumbs, 3, 0.0), 0);
        assert_eq!(drop_target(&thumbs, 3, 345.0), 3, "over its own place it stays");
        assert_eq!(drop_target(&thumbs, 1, 145.0), 1, "its own middle doesn't count");
    }
}
