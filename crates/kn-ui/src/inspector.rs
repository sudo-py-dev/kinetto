//! The right-hand panel: settings for the selected item, or for the slide,
//! laid out as preference groups the way GNOME settings are.

use crate::adw;
use crate::icons::Icon;
use crate::theme::{self, palette};
use crate::{KinettoApp, from_egui, to_egui};
use egui::RichText;
use kn_engine::Command;
use kn_geom::Aspect;
use kn_model::{Element, ElementKind, Motion, TextAlign, Transition};

impl KinettoApp {
    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 16 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match self.selected {
                    Some(index) => self.element_inspector(ui, index),
                    None => self.slide_inspector(ui),
                }
            });
        });
    }

    fn element_inspector(&mut self, ui: &mut egui::Ui, index: usize) {
        let before = self.project().slides[self.slide].elements[index].clone();
        let mut e = before.clone();
        let mut sealed = false;

        let kind = match &e.kind {
            ElementKind::Text { .. } => "Text",
            ElementKind::Shape { .. } => "Shape",
            ElementKind::Image { .. } => "Picture",
        };
        title(ui, kind, "Changes apply to the selected item");

        match &mut e.kind {
            ElementKind::Text { text, color, size, bold, align } => {
                adw::group(ui, "Words", None, |ui| {
                    adw::boxed_list(ui, |list| {
                        list.full(|ui| {
                            let r = ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY).frame(egui::Frame::NONE).hint_text("Type your words"));
                            sealed |= r.lost_focus();
                        });
                        list.row("Size", None, |ui| {
                            sealed |= adw::slider(ui, size, 16.0..=240.0, 1.0, "Size").drag_stopped();
                        });
                        list.row("Bold", None, |ui| {
                            sealed |= adw::switch(ui, bold, "Bold").changed();
                        });
                        list.row("Colour", None, |ui| sealed |= color_button(ui, color));
                        list.full(|ui| {
                            sealed |= adw::toggle_group(ui, align, &[(TextAlign::Start, "Left"), (TextAlign::Center, "Centre"), (TextAlign::End, "Right")]);
                        });
                    });
                });
            }
            ElementKind::Image { corner_radius, .. } => {
                adw::group(ui, "Picture", Some("Pictures fill their box and are cropped, never stretched."), |ui| {
                    adw::boxed_list(ui, |list| {
                        list.row("Rounded corners", None, |ui| {
                            sealed |= adw::slider(ui, corner_radius, 0.0..=400.0, 1.0, "Rounded corners").drag_stopped();
                        });
                    });
                });
            }
            ElementKind::Shape { fill, corner_radius } => {
                adw::group(ui, "Shape", None, |ui| {
                    adw::boxed_list(ui, |list| {
                        list.row("Colour", None, |ui| sealed |= color_button(ui, fill));
                        list.row("Rounded corners", None, |ui| {
                            sealed |= adw::slider(ui, corner_radius, 0.0..=200.0, 1.0, "Rounded corners").drag_stopped();
                        });
                    });
                });
            }
        }

        adw::group(ui, "Animation", Some("How it arrives when the slide starts"), |ui| {
            adw::boxed_list(ui, |list| {
                list.full(|ui| {
                    adw::toggle_group(ui, &mut e.motion, &[(Motion::None, "None"), (Motion::Fade, "Fade"), (Motion::Rise, "Rise"), (Motion::Pop, "Pop")]);
                });
            });
        });

        if e != before {
            self.update_element(index, e);
        }
        if sealed {
            self.session.seal();
        }

        let last = self.project().slides[self.slide].elements.len() - 1;
        let mut action = None;
        adw::group(ui, "Arrange", None, |ui| {
            adw::boxed_list(ui, |list| {
                list.row("Layer", Some("Bring in front of or behind others"), |ui| {
                    if ui.add_enabled_ui(index < last, |ui| adw::icon_button(ui, Icon::Up, "Bring forward")).inner.clicked() {
                        action = Some(Arrange::Forward);
                    }
                    if ui.add_enabled_ui(index > 0, |ui| adw::icon_button(ui, Icon::Down, "Send back")).inner.clicked() {
                        action = Some(Arrange::Back);
                    }
                });
                list.row("Remove from slide", None, |ui| {
                    if adw::icon_button(ui, Icon::Delete, "Delete (Del)").clicked() {
                        action = Some(Arrange::Delete);
                    }
                });
            });
        });
        match action {
            Some(Arrange::Forward) => {
                self.run(Command::ReorderElement { slide: self.slide, from: index, to: index + 1 });
                self.selected = Some(index + 1);
            }
            Some(Arrange::Back) => {
                self.run(Command::ReorderElement { slide: self.slide, from: index, to: index - 1 });
                self.selected = Some(index - 1);
            }
            Some(Arrange::Delete) => self.delete_selected(index),
            None => {}
        }
    }

    pub(crate) fn delete_selected(&mut self, index: usize) {
        self.run(Command::DeleteElement { slide: self.slide, index });
        self.session.seal();
        self.selected = None;
        self.notify_undo("Item deleted");
    }

    fn update_element(&mut self, index: usize, element: Element) {
        let motion_changed = element.motion != self.project().slides[self.slide].elements[index].motion;
        self.run(Command::UpdateElement { slide: self.slide, index, element });
        if motion_changed {
            // A new entrance is a discrete choice: its own undo step, and
            // replay the slide so the user sees it.
            self.session.seal();
            self.play_from(self.slide_start(self.slide));
        }
    }

    fn slide_inspector(&mut self, ui: &mut egui::Ui) {
        let index = self.slide;
        let slide = self.project().slides[index].clone();
        title(ui, &format!("Slide {}", index + 1), "Click something on the slide to change it");

        adw::group(ui, "Look", Some("Colours and motion for the whole video"), |ui| {
            adw::boxed_list(ui, |list| {
                list.full(|ui| self.look_picker(ui));
                list.row("Contrast slide", Some("Bold background, for section breaks"), |ui| {
                    let mut dark = slide.dark;
                    if adw::switch(ui, &mut dark, "Contrast slide").changed() {
                        self.run(Command::SetDark { index, dark });
                        self.session.seal();
                    }
                });
            });
        });

        adw::group(ui, "Timing", None, |ui| {
            adw::boxed_list(ui, |list| {
                let secs = slide.duration.as_secs_f64();
                list.row("Length", Some(&format!("{secs:.1} seconds")), |ui| {
                    let mut value = secs;
                    let r = adw::slider(ui, &mut value, 1.0..=20.0, 0.5, "Length");
                    if r.changed() {
                        self.run(Command::SetDuration { index, millis: (value * 1000.0).round() as i64 });
                    }
                    if r.drag_stopped() {
                        self.session.seal();
                    }
                });
                if index > 0 {
                    list.row("Arrives with", None, |ui| {
                        let mut transition = slide.transition;
                        adw::dropdown(ui, "transition", transition.name(), |ui| {
                            for kind in Transition::ALL {
                                ui.selectable_value(&mut transition, kind, kind.name());
                            }
                        });
                        if transition != slide.transition {
                            self.run(Command::SetTransition { index, transition });
                            self.session.seal();
                            // Play from just before, so the change is visible.
                            self.slide = index - 1;
                            self.play_from((self.slide_start(index) - kn_time::Tick::from_millis(800)).max(kn_time::Tick::ZERO));
                        }
                    });
                }
                list.row("Background", None, |ui| {
                    let mut bg = slide.background;
                    let sealed = color_button(ui, &mut bg);
                    if bg != slide.background {
                        self.run(Command::SetBackground { index, color: bg });
                    }
                    if sealed {
                        self.session.seal();
                    }
                });
            });
        });

        adw::group(ui, "Sound", Some("Music gets quieter by itself while someone speaks"), |ui| {
            adw::boxed_list(ui, |list| {
                self.voice_rows(list, index);
                self.music_rows(list);
            });
        });

        adw::group(ui, "Video shape", Some("Items move to fit when you change it"), |ui| {
            adw::boxed_list(ui, |list| {
                list.full(|ui| {
                    let mut aspect = self.project().aspect;
                    if adw::toggle_group(ui, &mut aspect, &[(Aspect::Wide16x9, "Wide"), (Aspect::Tall9x16, "Tall"), (Aspect::Square1x1, "Square")]) {
                        self.run(Command::SetAspect { aspect });
                        self.session.seal();
                    }
                });
            });
        });
    }
}

enum Arrange {
    Forward,
    Back,
    Delete,
}

/// The panel's heading: what is being edited, and a hint.
fn title(ui: &mut egui::Ui, heading: &str, hint: &str) {
    ui.label(RichText::new(heading).family(theme::bold()).size(18.0));
    ui.label(RichText::new(hint).size(12.5).color(palette(ui.ctx()).dim));
    ui.add_space(12.0);
}

/// Shows a colour button; returns true when the picker closes, which ends
/// the edit as one undo step.
fn color_button(ui: &mut egui::Ui, color: &mut kn_geom::Color) -> bool {
    let mut c = to_egui(*color);
    let r = egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque);
    *color = from_egui(c);
    r.lost_focus() || r.clicked_elsewhere()
}
