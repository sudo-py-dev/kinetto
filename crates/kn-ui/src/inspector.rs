//! The right-hand panel: settings for the selected item, or for the slide.

use crate::{KinettoApp, from_egui, to_egui};
use egui::RichText;
use kn_engine::Command;
use kn_geom::Aspect;
use kn_model::{Element, ElementKind, Motion, TextAlign, Transition};

impl KinettoApp {
    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            match self.selected {
                Some(index) => self.element_inspector(ui, index),
                None => self.slide_inspector(ui),
            }
        });
    }

    fn element_inspector(&mut self, ui: &mut egui::Ui, index: usize) {
        let before = self.project().slides[self.slide].elements[index].clone();
        let mut e = before.clone();
        let mut sealed = false;

        match &mut e.kind {
            ElementKind::Text { text, color, size, bold, align } => {
                ui.label(RichText::new("Text").heading());
                ui.add_space(4.0);
                let r = ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY));
                sealed |= r.lost_focus();
                ui.add_space(8.0);
                section(ui, "Size");
                let r = ui.add(egui::Slider::new(size, 16.0..=240.0).step_by(1.0).show_value(false));
                sealed |= r.drag_stopped();
                ui.horizontal(|ui| {
                    ui.toggle_value(bold, RichText::new("B").strong()).on_hover_text("Bold");
                    ui.separator();
                    ui.selectable_value(align, TextAlign::Start, "Left");
                    ui.selectable_value(align, TextAlign::Center, "Centre");
                    ui.selectable_value(align, TextAlign::End, "Right");
                });
                section(ui, "Colour");
                sealed |= color_button(ui, color);
            }
            ElementKind::Image { corner_radius, .. } => {
                ui.label(RichText::new("Picture").heading());
                ui.label(RichText::new("Pictures fill their box and are cropped, never stretched.").weak());
                section(ui, "Roundness");
                let r = ui.add(egui::Slider::new(corner_radius, 0.0..=400.0).show_value(false));
                sealed |= r.drag_stopped();
            }
            ElementKind::Shape { fill, corner_radius } => {
                ui.label(RichText::new("Shape").heading());
                section(ui, "Colour");
                sealed |= color_button(ui, fill);
                section(ui, "Roundness");
                let r = ui.add(egui::Slider::new(corner_radius, 0.0..=200.0).show_value(false));
                sealed |= r.drag_stopped();
            }
        }

        section(ui, "How it appears");
        ui.horizontal_wrapped(|ui| {
            for (motion, name) in [(Motion::None, "Just there"), (Motion::Fade, "Fade in"), (Motion::Rise, "Rise up"), (Motion::Pop, "Pop")] {
                ui.selectable_value(&mut e.motion, motion, name);
            }
        });

        if e != before {
            self.update_element(index, e);
        }
        if sealed {
            self.session.seal();
        }

        section(ui, "Arrange");
        let last = self.project().slides[self.slide].elements.len() - 1;
        ui.horizontal(|ui| {
            if ui.add_enabled(index < last, egui::Button::new("Bring forward")).clicked() {
                self.run(Command::ReorderElement { slide: self.slide, from: index, to: index + 1 });
                self.selected = Some(index + 1);
            }
            if ui.add_enabled(index > 0, egui::Button::new("Send back")).clicked() {
                self.run(Command::ReorderElement { slide: self.slide, from: index, to: index - 1 });
                self.selected = Some(index - 1);
            }
        });
        ui.add_space(16.0);
        if ui.button("Delete item").clicked() {
            self.run(Command::DeleteElement { slide: self.slide, index });
            self.session.seal();
            self.selected = None;
        }
    }

    fn update_element(&mut self, index: usize, element: Element) {
        let motion_changed = element.motion != self.project().slides[self.slide].elements[index].motion;
        self.run(Command::UpdateElement { slide: self.slide, index, element });
        if motion_changed {
            // A new entrance is a discrete choice: its own undo step, and
            // replay the slide so the user sees it.
            self.session.seal();
            self.playing = false;
            self.toggle_play();
        }
    }

    fn slide_inspector(&mut self, ui: &mut egui::Ui) {
        let index = self.slide;
        let slide = self.project().slides[index].clone();
        ui.label(RichText::new(format!("Slide {}", index + 1)).heading());
        ui.label(RichText::new("Click an item on the slide to change it.").weak());

        section(ui, "Look");
        self.look_picker(ui);
        let mut dark = slide.dark;
        if ui.checkbox(&mut dark, "Contrast slide").on_hover_text("Use the look's bold background, e.g. for a section break").changed() {
            self.run(Command::SetDark { index, dark });
            self.session.seal();
        }

        section(ui, "Background");
        let mut bg = slide.background;
        let sealed = color_button(ui, &mut bg);
        if bg != slide.background {
            self.run(Command::SetBackground { index, color: bg });
        }

        section(ui, "Length");
        let mut secs = slide.duration.as_secs_f64();
        let r = ui.add(egui::Slider::new(&mut secs, 1.0..=20.0).step_by(0.5).suffix(" s"));
        if r.changed() {
            self.run(Command::SetDuration { index, millis: (secs * 1000.0).round() as i64 });
        }
        if sealed || r.drag_stopped() {
            self.session.seal();
        }

        if index > 0 {
            section(ui, "Arrives with");
            let mut transition = slide.transition;
            ui.horizontal_wrapped(|ui| {
                for kind in Transition::ALL {
                    ui.selectable_value(&mut transition, kind, kind.name());
                }
            });
            if transition != slide.transition {
                self.run(Command::SetTransition { index, transition });
                self.session.seal();
                // Play from the slide before, so the change is visible.
                self.slide = index - 1;
                self.toggle_play();
                self.play_origin = (self.slide_start(index) - kn_time::Tick::from_millis(800)).max(kn_time::Tick::ZERO);
            }
        }

        section(ui, "Video shape");
        let mut aspect = self.project().aspect;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut aspect, Aspect::Wide16x9, "Wide 16:9");
            ui.selectable_value(&mut aspect, Aspect::Tall9x16, "Tall 9:16");
            ui.selectable_value(&mut aspect, Aspect::Square1x1, "Square");
        });
        if aspect != self.project().aspect {
            self.run(Command::SetAspect { aspect });
            self.session.seal();
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

/// Shows a colour button; returns true when the picker closes, which ends
/// the edit as one undo step.
fn color_button(ui: &mut egui::Ui, color: &mut kn_geom::Color) -> bool {
    let mut c = to_egui(*color);
    let r = egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque);
    *color = from_egui(c);
    r.lost_focus() || r.clicked_elsewhere()
}
