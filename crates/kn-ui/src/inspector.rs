//! The right-hand panel: settings for the selected item, or for the slide,
//! laid out as preference groups the way GNOME settings are.

use crate::adw;
use crate::icons::Icon;
use crate::theme::{self, palette};
use crate::{KinettoApp, from_egui, to_egui};
use egui::RichText;
use kn_engine::Command;
use kn_geom::Aspect;
use kn_model::{ Element, ElementKind, Entrance, Motion, TextAlign, Transition};

/// Tallest the "Items on this slide" list grows (about eight rows) before
/// it scrolls inside itself.
pub(crate) const ITEMS_LIST_HEIGHT: f32 = 8.0 * 34.0;

/// Tallest the "Words" box grows before it scrolls inside itself.
pub(crate) const WORDS_HEIGHT: f32 = 220.0;

impl KinettoApp {
    /// The right-hand panel: the selected item's settings, or the slide's.
    pub(crate) fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 18, bottom: 24 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match self.selected {
                    Some(index) => self.element_inspector(ui, index),
                    None => self.slide_inspector(ui),
                }
            });
        });
    }

    /// Settings for item `index` on the current slide. Edits go to a copy
    /// that is applied as one command when it differs.
    fn element_inspector(&mut self, ui: &mut egui::Ui, index: usize) {
        let before = self.project().slides[self.slide].elements[index].clone();
        let look = look_colors(self.project().slides[self.slide].theme_name(self.project()));
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
                            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                            egui::ScrollArea::vertical().id_salt("words").max_height(WORDS_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                                let r = ui.add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY).frame(egui::Frame::NONE).hint_text("Type your words"));
                                sealed |= r.lost_focus();
                            });
                        });
                        list.row("Size", Some(&format!("{size:.0} px")), |ui| {
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
                adw::group(ui, "Appearance", None, |ui| {
                    adw::boxed_list(ui, |list| {
                        list.row("Corner radius", Some(&format!("{corner_radius:.0} px")), |ui| {
                            sealed |= adw::slider(ui, corner_radius, 0.0..=400.0, 1.0, "Rounded corners").drag_stopped();
                        });
                        list.full(|ui| sealed |= rotation_control(ui, &mut e.rotation, "Image rotation"));
                    });
                });
            }
            ElementKind::Shape { fill, corner_radius, shape } => {
                adw::group(ui, "Appearance", None, |ui| {
                    adw::boxed_list(ui, |list| {
                        list.row("Type", None, |ui| {
                            let before = *shape;
                            adw::dropdown(ui, "shape-type", shape.name(), |ui| {
                                for kind in kn_model::ShapeKind::ALL { ui.selectable_value(shape, kind, kind.name()); }
                            });
                            if before != *shape {
                                let height = match *shape {
                                    kn_model::ShapeKind::Square | kn_model::ShapeKind::Diamond => Some(e.frame.w),
                                    kn_model::ShapeKind::Semicircle | kn_model::ShapeKind::Capsule => Some(e.frame.w / 2.0),
                                    _ => None,
                                };
                                if let Some(height) = height {
                                    e.frame.y += (e.frame.h - height) / 2.0;
                                    e.frame.h = height;
                                }
                                sealed = true;
                            }
                        });
                        list.row("Colour", None, |ui| sealed |= color_button(ui, fill, &look, "Shape colour"));
                        if *shape == kn_model::ShapeKind::Rectangle {
                        list.row("Corner radius", Some(&format!("{corner_radius:.0} px")), |ui| {
                            sealed |= adw::slider(ui, corner_radius, 0.0..=200.0, 1.0, "Rounded corners").drag_stopped();
                        });
                        }
                        list.full(|ui| sealed |= rotation_control(ui, &mut e.rotation, "Shape rotation"));
                    });
                });
            }
        }

        if matches!(e.kind, ElementKind::Image { .. }) {
            adw::group(ui, "Photo motion", Some("Moves the photo inside its frame"), |ui| {
                adw::boxed_list(ui, |list| {
                    list.row("Movement", None, |ui| {
                        let previous = e.photo_motion;
                        adw::dropdown(ui, "photo-motion", e.photo_motion.name(), |ui| {
                            for option in kn_model::PhotoMotion::ALL {
                                ui.selectable_value(&mut e.photo_motion, option, option.name()).on_hover_text(option.description());
                            }
                        });
                        sealed |= previous != e.photo_motion;
                    });
                    if e.photo_motion != kn_model::PhotoMotion::None {
                        list.row("Zoom amount", Some(&format!("{:.0}%", e.photo_amount * 100.0)), |ui| {
                            let mut percent = e.photo_amount * 100.0;
                            let r = adw::slider(ui, &mut percent, 2.0..=40.0, 1.0, "Motion amount");
                            if r.changed() { e.photo_amount = percent / 100.0; }
                            sealed |= r.drag_stopped();
                        });
                    }
                    list.row("Preset", None, |ui| {
                        let presets = [
                            ("Travel drift", kn_model::PhotoMotion::PanLeft, 0.18, "A slow pan for landscapes and travel journals."),
                            ("Elegant reveal", kn_model::PhotoMotion::ZoomOut, 0.10, "A restrained reveal for invitations and wedding photos."),
                            ("Product focus", kn_model::PhotoMotion::ZoomIn, 0.16, "A closer look at a product or interior detail."),
                        ];
                        let current = presets.iter().find(|(_, motion, amount, _)| e.photo_motion == *motion && (e.photo_amount - amount).abs() < 0.001).map_or("Custom", |p| p.0);
                        let response = adw::dropdown(ui, "photo-preset", current, |ui| {
                            for (label, motion, amount, hint) in presets {
                                if ui.selectable_label(current == label, label).on_hover_text(hint).clicked() {
                                    e.photo_motion = motion;
                                    e.photo_amount = amount;
                                    sealed = true;
                                }
                            }
                        });
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Photo preset"));
                    });
                });
            });
        }

        let slide = self.project().slides[self.slide].clone();
        let is_text = matches!(e.kind, ElementKind::Text { .. });
        let mut replay = false;
        adw::group(ui, if is_text { "Text animation" } else { "Animation" }, None, |ui| {
            adw::boxed_list(ui, |list| {
                list.row("Entrance", None, |ui| {
                    let previous = e.motion;
                    motion_picker(ui, "motion", &mut e.motion, is_text, false);
                    if previous != e.motion {
                        if e.motion.text_only() && !previous.text_only() {
                            e.motion_duration_ms = 1800;
                            e.easing = kn_model::Easing::Linear;
                        }
                        sealed = true;
                    }
                });
                list.row("Exit", None, |ui| {
                    let previous = e.exit;
                    motion_picker(ui, "exit", &mut e.exit, is_text, true);
                    sealed |= previous != e.exit;
                });
                list.row("Duration", Some(&format!("{:.1} s each", e.motion_secs())), |ui| {
                    let mut seconds = e.motion_secs();
                    let response = adw::slider(ui, &mut seconds, 0.1..=5.0, 0.1, "Animation duration");
                    if response.changed() { e.motion_duration_ms = (seconds * 1000.0).round() as u32; }
                    sealed |= response.drag_stopped();
                });
                list.row("Pace", None, |ui| {
                    let previous = e.easing;
                    adw::dropdown(ui, "animation-easing", e.easing.name(), |ui| {
                        for easing in kn_model::Easing::ALL { ui.selectable_value(&mut e.easing, easing, easing.name()); }
                    });
                    sealed |= previous != e.easing;
                });
                let start = slide.element_start(index);
                let subtitle = if e.start_ms.is_some() { format!("At {start:.2} s") } else { format!("Auto · {start:.2} s") };
                list.row("Start time", Some(&subtitle), |ui| {
                    let mut secs = start;
                    let max = (slide.duration.as_secs_f64() - e.motion_secs()).max(0.0);
                    let r = adw::slider(ui, &mut secs, 0.0..=max, 0.05, "Appears at");
                    if r.changed() {
                        e.start_ms = Some((secs * 1000.0).round() as i64);
                    }
                    sealed |= r.drag_stopped();
                });
                if e.start_ms.is_some() {
                    list.row("Start timing", None, |ui| {
                        if adw::button(ui, Some(Icon::Undo), "Automatic", adw::Kind::Normal).on_hover_text("Back to automatic").clicked() {
                            e.start_ms = None;
                            sealed = true;
                        }
                    });
                }
                list.row("Preview", None, |ui| {
                    replay = adw::button(ui, Some(Icon::Play), "Replay", adw::Kind::Normal).clicked();
                });
            });
        });

        if e != before {
            self.update_element(index, e);
        }
        if sealed {
            self.session.seal();
        }

        if replay {
            self.play_from(self.slide_start(self.slide));
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

    /// Removes item `index` from the current slide, with an Undo toast.
    pub(crate) fn delete_selected(&mut self, index: usize) {
        self.run(Command::DeleteElement { slide: self.slide, index });
        self.session.seal();
        self.selected = None;
        self.notify_undo("Item deleted");
    }

    /// Applies an edited copy of item `index` if anything changed.
    fn update_element(&mut self, index: usize, element: Element) {
        let before = &self.project().slides[self.slide].elements[index];
        let motion_changed = element.motion != before.motion || element.exit != before.exit;
        self.run(Command::UpdateElement { slide: self.slide, index, element });
        if motion_changed {
            // Keep the inspector open for fine-tuning; Replay is explicit.
            self.session.seal();
        }
    }

    /// Settings for the current slide when no item is selected: look, items,
    /// timing, transition, sound and video shape.
    fn slide_inspector(&mut self, ui: &mut egui::Ui) {
        let index = self.slide;
        let slide = self.project().slides[index].clone();
        let look = look_colors(self.project().slides[self.slide].theme_name(self.project()));
        title(ui, &format!("Slide {}", index + 1), "Select an item on the slide to edit it");

        adw::group(ui, "Look", Some("Colours and style for this slide"), |ui| {
            adw::boxed_list(ui, |list| {
                list.full(|ui| self.look_picker(ui));
                list.full(|ui| {
                    if adw::wide_button(ui, "Apply to all slides", adw::Kind::Normal, ui.available_width()).on_hover_text("Copy this look and contrast setting to every slide. Undo restores each slide's previous look.").clicked() {
                        self.run(Command::ApplySlideLookToAll { index });
                        self.session.seal();
                    }
                });
                list.row("Dark background", Some("For section breaks"), |ui| {
                    let mut dark = slide.dark;
                    if adw::switch(ui, &mut dark, "Dark background").changed() {
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
                list.row("Duration", Some(&format!("{secs:.1} seconds")), |ui| {
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
                    list.row("Transition", None, |ui| {
                        let mut transition = slide.transition;
                        adw::dropdown(ui, "transition", transition.name(), |ui| {
                            for kind in Transition::ALL {
                                ui.selectable_value(&mut transition, kind, kind.name());
                            }
                        });
                        if transition != slide.transition {
                            self.choose_transition(index, transition);
                        }
                    });
                }
                list.full(|ui| {
                    ui.label("Items appear");
                    ui.add_space(8.0);
                    let mut entrance = slide.entrance;
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
        // Just what each row shows: names are short, so text bodies aren't
        // copied every frame.
        let items: Vec<(Icon, String, Option<f64>)> = self.project().slides[slide].elements.iter().map(|e| {
            let (icon, name) = crate::timeline::item_name(&e.kind);
            (icon, name, e.start_ms.map(|ms| ms as f64 / 1000.0))
        }).collect();
        let p = palette(ui.ctx());
        let mut pick = None;
        let mut reorder = None;
        let about = match items.len() {
            0..=1 => "Front first. Drag to change the order.".to_owned(),
            n => format!("{n} items, front first. Drag to change the order."),
        };
        adw::group(ui, "Items on this slide", Some(&about), |ui| {
            if items.is_empty() {
                adw::boxed_list(ui, |list| list.row("No items yet", Some("Add text, a shape or a picture with + in the top bar"), |_| {}));
                return;
            }
            adw::boxed_list(ui, |list| {
                list.full(|ui| {
                    // A long list scrolls inside itself, so the settings below
                    // stay a short scroll away. A solid bar shows there is more.
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::vertical().id_salt(("items-list", slide)).max_height(ITEMS_LIST_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let mut rows = Vec::new();
                    for (i, (icon, name, start)) in items.iter().enumerate().rev() {
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click_and_drag());
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, name));
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
                        crate::icons::paint(ui.painter(), egui::Rect::from_center_size(rect.left_center() + egui::vec2(14.0, 0.0), egui::Vec2::splat(16.0)), *icon, fg);
                        ui.painter().text(rect.left_center() + egui::vec2(32.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.5), fg);
                        // When the item appears, if it has its own time.
                        if let Some(secs) = start {
                            ui.painter().text(rect.right_center() - egui::vec2(36.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{secs:.1} s"), egui::FontId::proportional(11.5), p.dim);
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
                        // Held near the top or bottom: scroll to reach the rest.
                        let visible = ui.clip_rect();
                        if r.dragged() {
                            let push = if pos.y < visible.top() + 24.0 { 8.0 } else if pos.y > visible.bottom() - 24.0 { -8.0 } else { 0.0 };
                            if push != 0.0 {
                                ui.scroll_with_delta(egui::vec2(0.0, push));
                                ui.ctx().request_repaint();
                            }
                        }
                    }
                    });
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

/// What the item's arrange buttons can do.
enum Arrange {
    /// One step nearer the front.
    Forward,
    /// One step further back.
    Back,
    /// Remove the item.
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

/// Category menus keep a large motion library readable in the narrow inspector.
fn motion_picker(ui: &mut egui::Ui, id: &str, motion: &mut Motion, is_text: bool, exit: bool) {
    let label = if exit { motion.exit_name() } else { motion.name() };
    let response = adw::dropdown(ui, id, label, |ui| {
        for category in ["Simple", "Letter by letter", "Slide", "Reveal", "Expressive"] {
            if category == "Letter by letter" && !is_text { continue; }
            ui.menu_button(category, |ui| {
                for option in Motion::ALL.into_iter().filter(|m| m.category() == category) {
                    let label = if exit { option.exit_name() } else { option.name() };
                    if ui.selectable_value(motion, option, label).clicked() { ui.close(); }
                }
            });
        }
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, if exit { "Exit animation" } else { "Entrance animation" }));
}

/// Shared precise rotation control for images and shapes.
fn rotation_control(ui: &mut egui::Ui, rotation: &mut f64, label: &str) -> bool {
    let mut sealed = false;
    ui.horizontal(|ui| {
        ui.label("Rotation");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(*rotation != 0.0, |ui| {
                if adw::icon_button(ui, Icon::Undo, "Reset rotation").clicked() {
                    *rotation = 0.0;
                    sealed = true;
                }
            });
        });
    });
    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(60.0);
    let response = ui.add(egui::Slider::new(rotation, -180.0..=180.0).suffix("°").max_decimals(1));
    response.widget_info(|| egui::WidgetInfo::slider(true, *rotation, label));
    sealed |= response.drag_stopped() || response.lost_focus() || (response.changed() && !response.dragged());
    sealed
}
