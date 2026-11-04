//! The slide strip: live thumbnails, slide actions and "New slide".

use crate::KinettoApp;
use crate::canvas::{Frame, Surface};
use egui::{RichText, Sense, Stroke, Vec2};
use kn_engine::Command;

const THUMB_WIDTH: f32 = 160.0;

impl KinettoApp {
    pub(crate) fn slide_strip(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.add_space(8.0);
        ui.label(RichText::new("Slides").strong());
        ui.add_space(8.0);

        let count = self.project().slides.len();
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            canvas.retain_thumbnails(rs, count);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for index in 0..count {
                self.thumbnail(ui, frame, index);
                ui.add_space(10.0);
            }
            ui.scope(|ui| {
                ui.set_width(THUMB_WIDTH);
                ui.menu_button("+ New slide", |ui| {
                    for layout in kn_layout::Layout::ALL {
                        if ui.button(layout.name()).clicked() {
                            self.run(Command::AddLayoutSlide { at: self.slide + 1, layout });
                            self.session.seal();
                            self.select_slide(self.slide + 1);
                        }
                    }
                });
            });
        });
    }

    fn thumbnail(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, index: usize) {
        let (cw, ch) = self.project().aspect.canvas_size();
        let size = Vec2::new(THUMB_WIDTH, THUMB_WIDTH * (ch / cw) as f32);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());

        let start = self.slide_start(index);
        let slide = &self.project().slides[index];
        let time = start + kn_render::rest_time(slide.elements.len()).min(slide.duration - kn_time::Tick(1));
        let secs = slide.duration.as_secs_f64();
        let ppp = ui.ctx().pixels_per_point();
        let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
        let stamp = Some(self.session.revision());

        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let (project, media) = (self.session.project(), &self.session.media);
            let id = canvas.paint(rs, Surface::Thumbnail(index), Frame { project, media, time, size: px, stamp }, &mut self.render);
            egui::Image::new((id, size)).corner_radius(6).paint_at(ui, rect);
        }

        let visuals = ui.visuals();
        let stroke = if index == self.slide {
            Stroke::new(3.0, visuals.selection.bg_fill)
        } else {
            Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color)
        };
        ui.painter().rect_stroke(rect, 6, stroke, egui::StrokeKind::Outside);
        ui.painter().text(
            rect.left_bottom() + Vec2::new(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!("{}   {secs:.1} s", index + 1),
            egui::FontId::proportional(11.0),
            visuals.weak_text_color(),
        );
        ui.add_space(12.0);

        if response.clicked() {
            self.select_slide(index);
        }
        let last = self.project().slides.len() - 1;
        response.context_menu(|ui| {
            if ui.button("Duplicate").clicked() {
                self.run(Command::DuplicateSlide { index });
                self.select_slide(index + 1);
            }
            if ui.add_enabled(index > 0, egui::Button::new("Move up")).clicked() {
                self.run(Command::MoveSlide { from: index, to: index - 1 });
                self.select_slide(index - 1);
            }
            if ui.add_enabled(index < last, egui::Button::new("Move down")).clicked() {
                self.run(Command::MoveSlide { from: index, to: index + 1 });
                self.select_slide(index + 1);
            }
            ui.separator();
            if ui.add_enabled(last > 0, egui::Button::new("Delete slide")).clicked() {
                self.run(Command::DeleteSlide { index });
                self.select_slide(index.saturating_sub(1));
            }
        });
    }
}
