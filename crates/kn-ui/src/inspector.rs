//! The right-hand panel: settings for the selected item, or for the slide,
//! laid out as preference groups the way GNOME settings are.

use crate::adw;
use crate::icons::Icon;
use crate::theme::{self, palette};
use crate::{KinettoApp, from_egui, to_egui};
use egui::RichText;
use kn_engine::Command;
use kn_geom::Aspect;
use kn_model::{ENTRANCE_SECS, Element, ElementKind, Entrance, Motion, TextAlign, Transition};

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
        let look = look_colors(&self.project().theme);
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
                        list.row("Colour", None, |ui| sealed |= color_button(ui, color, &look, "Text colour"));
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
                        list.row("Colour", None, |ui| sealed |= color_button(ui, fill, &look, "Shape colour"));
                        list.row("Rounded corners", None, |ui| {
                            sealed |= adw::slider(ui, corner_radius, 0.0..=200.0, 1.0, "Rounded corners").drag_stopped();
                        });
                    });
                });
            }
        }

        let slide = self.project().slides[self.slide].clone();
        adw::group(ui, "Animation", Some("How and when it arrives"), |ui| {
            adw::boxed_list(ui, |list| {
                list.full(|ui| {
                    adw::toggle_group(ui, &mut e.motion, &[(Motion::None, "None"), (Motion::Fade, "Fade"), (Motion::Rise, "Rise"), (Motion::Pop, "Pop")]);
                });
                let start = slide.element_start(index);
                let subtitle = if e.start_ms.is_some() { format!("At {start:.2} s") } else { format!("Automatically, at {start:.2} s") };
                list.row("Appears", Some(&subtitle), |ui| {
                    if e.start_ms.is_some() && adw::icon_button(ui, Icon::Undo, "Back to automatic").clicked() {
                        e.start_ms = None;
                        sealed = true;
                    }
                    let mut secs = start;
                    let max = (slide.duration.as_secs_f64() - ENTRANCE_SECS).max(0.05);
                    let r = adw::slider(ui, &mut secs, 0.0..=max, 0.05, "Appears at");
                    if r.changed() {
                        e.start_ms = Some((secs * 1000.0).round() as i64);
                    }
                    sealed |= r.drag_stopped();
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
        let look = look_colors(&self.project().theme);
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

        self.items_list(ui, index);

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
                list.row("Items appear", None, |ui| {
                    let mut entrance = slide.entrance;
                    ui.set_max_width(170.0);
                    if adw::toggle_group(ui, &mut entrance, &[(Entrance::OneByOne, "One by one"), (Entrance::Together, "All at once")]) {
                        self.run(Command::SetEntrance { index, entrance });
                        self.session.seal();
                        self.play_from(self.slide_start(index));
                    }
                });
                list.row("Background", None, |ui| {
                    let mut bg = slide.background;
                    let sealed = color_button(ui, &mut bg, &look, "Background colour");
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

impl KinettoApp {
    /// The slide's items, front-most first like a layers list: click to
    /// select, drag to change which is in front.
    fn items_list(&mut self, ui: &mut egui::Ui, slide: usize) {
        let elements: Vec<ElementKind> = self.project().slides[slide].elements.iter().map(|e| e.kind.clone()).collect();
        let pinned: Vec<bool> = self.project().slides[slide].elements.iter().map(|e| e.start_ms.is_some()).collect();
        let p = palette(ui.ctx());
        let mut pick = None;
        let mut reorder = None;
        adw::group(ui, "Items on this slide", Some("Front first. Drag to change the order."), |ui| {
            if elements.is_empty() {
                adw::boxed_list(ui, |list| list.row("No items yet", Some("Add text, a shape or a picture with + in the top bar"), |_| {}));
                return;
            }
            adw::boxed_list(ui, |list| {
                list.full(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut rows = Vec::new();
                    for i in (0..elements.len()).rev() {
                        let (icon, name) = crate::timeline::item_name(&elements[i]);
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click_and_drag());
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &name));
                        let dragging = response.dragged();
                        if response.hovered() || dragging {
                            ui.painter().rect_filled(rect, 6, p.button[0]);
                            ui.ctx().set_cursor_icon(if dragging { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                        }
                        let selected = self.selected == Some(i);
                        if selected {
                            ui.painter().rect_filled(rect, 6, p.accent_bg.gamma_multiply(0.18));
                        }
                        let fg = if selected { p.accent } else { p.fg };
                        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.left_center() + egui::vec2(14.0, 0.0), egui::Vec2::splat(16.0)), icon, fg);
                        ui.painter().text(rect.left_center() + egui::vec2(32.0, 0.0), egui::Align2::LEFT_CENTER, &name, egui::FontId::proportional(13.5), fg);
                        if pinned[i] {
                            ui.painter().text(rect.right_center() - egui::vec2(36.0, 0.0), egui::Align2::RIGHT_CENTER, "timed", egui::FontId::proportional(11.0), p.dim);
                        }
                        // Grip dots on the right say "this can be dragged".
                        for dy in [-4.0, 0.0, 4.0] {
                            for dx in [0.0, 4.0] {
                                ui.painter().circle_filled(rect.right_center() + egui::vec2(-16.0 + dx, dy), 1.3, p.dim);
                            }
                        }
                        if response.clicked() {
                            pick = Some(i);
                        }
                        rows.push((i, rect, response));
                    }
                    // While dragging, show where the item will land.
                    if let Some((from, _, r)) = rows.iter().find(|(_, _, r)| r.dragged() || r.drag_stopped())
                        && let Some(pos) = r.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
                    {
                        let target = rows.iter().find(|(_, rect, _)| pos.y < rect.bottom()).or(rows.last()).map(|(i, rect, _)| (*i, *rect));
                        if let Some((to, rect)) = target {
                            let y = if to < *from { rect.bottom() + 1.0 } else { rect.top() - 1.0 };
                            if r.dragged() && to != *from {
                                ui.painter().hline(rect.x_range(), y, egui::Stroke::new(2.5, p.accent_bg));
                            }
                            if r.drag_stopped() && to != *from {
                                reorder = Some((*from, to));
                            }
                        }
                    }
                });
            });
        });
        if let Some(i) = pick {
            self.selected = Some(i);
        }
        if let Some((from, to)) = reorder {
            self.run(Command::ReorderElement { slide, from, to });
            self.session.seal();
            self.selected = Some(to);
        }
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

/// A colour button with the look's own colours offered first; returns
/// true when the choice is finished, ending the edit as one undo step.
fn color_button(ui: &mut egui::Ui, color: &mut kn_geom::Color, look: &[egui::Color32], label: &str) -> bool {
    let mut c = to_egui(*color);
    let (_, finished) = adw::color_button(ui, &mut c, look, label);
    *color = from_egui(c);
    finished
}

/// The current look's colours, as offered first in colour palettes.
fn look_colors(theme: &str) -> Vec<egui::Color32> {
    let t = kn_layout::theme(theme);
    [t.background, t.text, t.muted, t.dark_background, t.accents[0], t.accents[1], t.accents[2]].into_iter().map(to_egui).collect()
}
