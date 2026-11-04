//! The sidebar: a live thumbnail per slide, slide actions, and New slide.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::{self, Icon};
use crate::theme::{self, palette};
use egui::{RichText, Sense, Stroke, Vec2};
use kn_engine::Command;
use kn_model::Transition;

const THUMB_WIDTH: f32 = 180.0;

impl KinettoApp {
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
                for index in 0..count {
                    if index > 0 {
                        self.transition_button(ui, index);
                    }
                    self.thumbnail(ui, frame, index);
                    // A thumbnail's menu can insert, remove or move a slide.
                    if self.project().slides.len() != count {
                        ui.ctx().request_discard("slide list changed");
                        break;
                    }
                }
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
                let response = adw::button(ui, Some(Icon::Down), "Animation", Kind::Normal);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let response = response.on_hover_text(format!("{label}: {}", current.name()));
                egui::Popup::menu(&response).width(180.0).show(|ui| {
                    ui.label(RichText::new(format!("Into slide {}", index + 1)).size(12.0).color(palette(ui.ctx()).dim));
                    for transition in Transition::ALL {
                        if ui.selectable_label(current == transition, transition.name()).clicked() {
                            self.choose_transition(index, transition);
                            ui.close();
                        }
                    }
                });
            });
        });
    }

    fn thumbnail(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, index: usize) {
        let p = palette(ui.ctx());
        let (cw, ch) = self.project().aspect.canvas_size();
        // Tall slides would make a very long list; cap their height.
        let height = (THUMB_WIDTH * (ch / cw) as f32).min(THUMB_WIDTH * 1.1);
        let width = height * (cw / ch) as f32;
        let (row, response) = ui.allocate_exact_size(Vec2::new(THUMB_WIDTH, height + 26.0), Sense::click());
        let rect = egui::Rect::from_min_size(egui::pos2(row.center().x - width / 2.0, row.top() + 4.0), Vec2::new(width, height - 8.0));
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, index == self.slide, format!("Slide {}", index + 1)));

        let start = self.slide_start(index);
        let slide = &self.project().slides[index];
        let time = start + slide.rest_time();
        let caption = format!("{}  ·  {:.1} s{}", index + 1, slide.duration.as_secs_f64(), if slide.voice.is_some() { "  ·  voice" } else { "" });
        let ppp = ui.ctx().pixels_per_point();
        let px = [(rect.width() * ppp).round() as u32, (rect.height() * ppp).round() as u32];
        let stamp = Some(self.content_stamp());
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let (project, media) = (self.session.project(), &self.session.media);
            let id = canvas.paint(rs, Surface::Thumbnail(index), Frame { project, media, time, size: px, stamp }, &mut self.render);
            egui::Image::new((id, rect.size())).corner_radius(8).paint_at(ui, rect);
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
    }

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
