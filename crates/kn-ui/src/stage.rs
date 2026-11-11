//! The stage: the big slide preview where items are picked, moved and resized.

use crate::i18n::tr;
use crate::KinettoApp;
use crate::canvas::{Frame, Surface};
use egui::{Pos2, Sense, Stroke, Vec2};
use kn_engine::Command;
use kn_geom::Rect;
use std::hash::{Hash, Hasher};

/// Size of the corner resize handles, in points.
const HANDLE: f32 = 10.0;
/// The selection outline sits this far outside the item, so neither it nor
/// the corner handles cover the item's edges or its first and last letters.
const SELECTION_PAD: f32 = 6.0;
/// How far above the item the rotation knob sits.
const ROTATION_OFFSET: f32 = 28.0;
/// Radius of the rotation knob.
const ROTATION_RADIUS: f32 = 11.0;
/// Smallest an item can be resized to, in slide units.
const MIN_SIZE: f64 = 24.0;
/// How close, in screen points, a moved item must come to a guide to snap.
const SNAP_DISTANCE: f32 = 6.0;
/// Height of the play controls under the slide in the Simple layout.
const TRANSPORT: f32 = 44.0;
/// How far a duplicate or a pasted copy lands from the original.
const COPY_OFFSET: f64 = 40.0;
/// Marks clipboard text as a copied Kinetto item.
const CLIP_PREFIX: &str = "kinetto-item:";
/// Longest pasted text that becomes a text item.
const PASTE_TEXT_LIMIT: usize = 2000;
/// Duplicate the selected item.
const DUPLICATE: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::D);

/// The lines a moved item is snapped to, in slide units, shown while dragging.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct Guides {
    /// A vertical line at this x.
    x: Option<f64>,
    /// A horizontal line at this y.
    y: Option<f64>,
}

/// An item being changed with the pointer on the stage.
#[derive(Clone, Copy)]
pub(crate) struct Drag {
    /// Moving, resizing or rotating.
    kind: DragKind,
    /// The item's frame when the drag began; changes are measured from it.
    start_frame: Rect,
    /// Where the pointer was when the drag began.
    start_pointer: Pos2,
    /// The item's rotation when the drag began, in radians.
    rotation: f64,
    /// Keep width and height equal while resizing (square shapes).
    square: bool,
}

/// What a stage drag does.
#[derive(Clone, Copy)]
enum DragKind {
    /// Drag the whole item.
    Move,
    /// Turn it with the rotation knob.
    Rotate,
    /// Which corner is being pulled: (right?, bottom?).
    Resize(bool, bool),
}

/// Maps between slide units and screen points for the shown stage.
struct View {
    /// Screen position of the slide's top-left corner.
    origin: Pos2,
    /// Screen points per slide unit.
    scale: f32,
}

impl View {
    /// A slide-unit rectangle in screen points.
    fn to_screen(&self, r: Rect) -> egui::Rect {
        let min = self.origin + Vec2::new(r.x as f32, r.y as f32) * self.scale;
        egui::Rect::from_min_size(min, Vec2::new(r.w as f32, r.h as f32) * self.scale)
    }

    /// A screen point in slide units.
    fn to_slide(&self, p: Pos2) -> (f64, f64) {
        let v = (p - self.origin) / self.scale;
        (f64::from(v.x), f64::from(v.y))
    }
}

impl KinettoApp {
    /// The big slide in the middle: draws it, then handles selecting, dragging,
    /// resizing, rotating and keys for the selected item.
    pub(crate) fn stage(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = crate::theme::palette(ui.ctx());
        let (cw, ch) = self.project().aspect.canvas_size();
        // Room around the slide, and beneath it the play controls (Simple
        // layout) or a status line (the timeline has its own controls).
        const STATUS: f32 = 28.0;
        let simple = self.settings.layout == crate::settings::Layout::Simple;
        let padding = if ui.max_rect().width() < 560.0 { 24.0 } else { 40.0 };
        let area = ui.max_rect().shrink2(Vec2::new(padding, 24.0));
        let area = egui::Rect::from_min_max(area.min, area.max - Vec2::new(0.0, if simple { TRANSPORT } else { STATUS }));
        let fit = (area.width() / cw as f32).min(area.height() / ch as f32).max(0.01);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let rect = egui::Rect::from_center_size(area.center(), size);
        if simple {
            self.transport(ui, rect);
        } else {
            self.status_line(ui, egui::pos2(rect.center().x, rect.bottom() + 12.0));
        }
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let view = View { origin: rect.min, scale: fit };
        let time = if self.playback.playing { self.playback.time } else { self.edit_time() };

        // Copy, paste and duplicate work whatever is drawn below.
        if !self.playback.playing {
            self.clipboard_keys(ui);
        }
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
        let shadow = egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color: egui::Color32::from_black_alpha(if ui.visuals().dark_mode { 70 } else { 24 }) };
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
        // Right-click picks what is under the pointer, then offers its menu.
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.selected = self.hit(view.to_slide(pos));
        }
        egui::Popup::context_menu(&response).width(230.0).show(|ui| self.stage_menu(ui));
        self.handle_keys(ui);
        self.draw_guides(ui, &view);
        self.draw_selection(ui, &view);
    }

    /// Play controls under the slide: previous slide, play or stop, next
    /// slide, a bar to drag through the whole video, and the time.
    fn transport(&mut self, ui: &mut egui::Ui, slide: egui::Rect) {
        use crate::adw;
        use crate::icons::Icon;
        let p = crate::theme::palette(ui.ctx());
        let width = slide.width().clamp(300.0, 680.0).min(ui.max_rect().width() - 16.0);
        let bar = egui::Rect::from_center_size(egui::pos2(slide.center().x, slide.bottom() + 8.0 + TRANSPORT / 2.0), Vec2::new(width, 34.0));
        // Playback runs left to right in every language, like the timeline.
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt("transport").max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 2.0;
        let last = self.project().slides.len() - 1;
        if ui.add_enabled_ui(self.slide > 0, |ui| adw::icon_button(ui, Icon::Previous, "Previous slide  (Page Up)")).inner.clicked() {
            self.select_slide(self.slide - 1);
        }
        let (icon, tip) = if self.playback.playing { (Icon::Stop, "Stop  (Space)") } else { (Icon::Play, "Play from this slide  (Space)") };
        if adw::icon_button(ui, icon, tip).clicked() {
            self.toggle_play();
        }
        if ui.add_enabled_ui(self.slide < last, |ui| adw::icon_button(ui, Icon::Next, "Next slide  (Page Down)")).inner.clicked() {
            self.select_slide(self.slide + 1);
        }
        ui.add_space(8.0);

        let total = self.project().total_duration().as_secs_f64().max(0.001);
        let now = self.playhead().as_secs_f64();
        let clock = |s: f64| format!("{}:{:02}", s as u64 / 60, s as u64 % 60);
        let time = format!("{} / {}", clock(now), clock(total));
        let time = ui.painter().layout_no_wrap(time, egui::FontId::proportional(12.5), p.dim);
        let (r, response) = ui.allocate_exact_size(Vec2::new((ui.available_width() - time.size().x - 12.0).max(40.0), 28.0), Sense::click_and_drag());
        let rail = r.shrink2(Vec2::new(8.0, 0.0));
        if (response.dragged() || response.clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.scrub_to(f64::from(((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0)) * total);
        }
        response.widget_info(|| egui::WidgetInfo::slider(true, now, "Video position"));
        let y = r.center().y;
        let x_of = |secs: f64| egui::lerp(rail.left()..=rail.right(), (secs / total).clamp(0.0, 1.0) as f32);
        let track = egui::Rect::from_min_max(egui::pos2(rail.left(), y - 2.0), egui::pos2(rail.right(), y + 2.0));
        ui.painter().rect_filled(track, 2, p.button[1]);
        ui.painter().rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(x_of(now), track.max.y)), 2, p.accent_bg);
        // A small gap where each slide ends, so the bar reads as slides.
        let mut edge = 0.0;
        for s in &self.project().slides[..last] {
            edge += s.duration.as_secs_f64();
            ui.painter().vline(x_of(edge), (y - 3.0)..=(y + 3.0), Stroke::new(2.0, p.view));
        }
        let knob = egui::pos2(x_of(now), y);
        let active = response.hovered() || response.dragged();
        ui.painter().circle(knob, if active { 7.5 } else { 6.5 }, egui::Color32::WHITE, Stroke::new(1.0, egui::Color32::from_black_alpha(if active { 70 } else { 45 })));
        response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Drag to look through the video"));
        ui.add_space(10.0);
        let at = egui::pos2(ui.cursor().left(), y - time.size().y / 2.0);
        ui.painter().galley(at, time, p.dim);
    }

    /// The stage's right-click menu: actions for the item under the
    /// pointer, or ways to add something on empty space.
    fn stage_menu(&mut self, ui: &mut egui::Ui) {
        use crate::header::menu_item;
        use crate::icons::Icon;
        let item = |ui: &mut egui::Ui, icon: Icon, label: &str, shortcut: Option<&str>, enabled: bool| ui.add_enabled_ui(enabled, |ui| menu_item(ui, icon, label, shortcut)).inner;
        let ctx = ui.ctx().clone();
        if let Some(index) = self.selected {
            let last = self.project().slides[self.slide].elements.len() - 1;
            if item(ui, Icon::Copy, "Duplicate", Some("Ctrl+D"), true) { self.duplicate_selected(); }
            if item(ui, Icon::Copy, "Copy", Some("Ctrl+C"), true) { self.copy_selected(&ctx); }
            if item(ui, Icon::Paste, "Paste", Some("Ctrl+V"), true) { self.paste_from_menu(&ctx); }
            ui.separator();
            if item(ui, Icon::Front, "Bring to front", None, index < last) { self.restack(index, last); }
            if item(ui, Icon::Back, "Send to back", None, index > 0) { self.restack(index, 0); }
            ui.separator();
            if item(ui, Icon::Delete, "Delete", Some("Del"), true) { self.delete_selected(index); }
        } else {
            if item(ui, Icon::Paste, "Paste", Some("Ctrl+V"), true) { self.paste_from_menu(&ctx); }
            ui.separator();
            if item(ui, Icon::Text, "Add text", None, true) { self.add_text(); }
            if item(ui, Icon::Picture, "Add picture…", None, true) { self.pick_pictures(); }
        }
    }

    /// Moves item `from` to place `to` in the stacking order, as one undo step.
    fn restack(&mut self, from: usize, to: usize) {
        self.run(Command::ReorderElement { slide: self.slide, from, to });
        self.session.seal();
        self.selected = Some(to);
    }

    /// A copy of the selected item, a little down and to the right.
    pub(crate) fn duplicate_selected(&mut self) {
        let Some(index) = self.selected else { return };
        let mut copy = self.project().slides[self.slide].elements[index].clone();
        copy.frame.x += COPY_OFFSET;
        copy.frame.y += COPY_OFFSET;
        self.add_element(copy);
    }

    /// Copies the selected item, for pasting on any slide. It also goes to
    /// the system clipboard, so it survives into another Kinetto window.
    pub(crate) fn copy_selected(&mut self, ctx: &egui::Context) {
        let Some(index) = self.selected else { return };
        let element = self.project().slides[self.slide].elements[index].clone();
        if let Ok(json) = serde_json::to_string(&element) {
            ctx.copy_text(format!("{CLIP_PREFIX}{json}"));
        }
        self.clipboard = Some(element);
    }

    /// Paste chosen from a menu: the item copied here, or else whatever the
    /// system clipboard holds (it arrives as a paste event next frame).
    pub(crate) fn paste_from_menu(&mut self, ctx: &egui::Context) {
        match self.clipboard.clone() {
            Some(element) => self.paste_element(element),
            None => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
        }
    }

    /// Pasted clipboard text: a copied item comes back as that item, and
    /// any other words become a new text item.
    fn paste_text(&mut self, text: &str) {
        if let Some(json) = text.strip_prefix(CLIP_PREFIX) {
            match serde_json::from_str::<kn_model::Element>(json) {
                Ok(element) => self.paste_element(element),
                Err(_) => self.notify("That copied item can't be pasted here."),
            }
            return;
        }
        let words: String = text.trim().chars().take(PASTE_TEXT_LIMIT).collect();
        if words.is_empty() {
            return;
        }
        let (w, h) = self.project().aspect.canvas_size();
        let bg = self.project().slides[self.slide].background;
        let width = (w * 0.7).min(1400.0);
        let mut element = kn_model::Element::text(&words, Rect::new((w - width) / 2.0, h / 2.0 - 60.0, width, 120.0), 64.0, false, crate::readable_on(bg));
        // Long passages read better small.
        if let kn_model::ElementKind::Text { size, .. } = &mut element.kind
            && words.chars().count() > 80
        {
            *size = 40.0;
        }
        self.add_element(element);
    }

    /// Adds a copied item to the current slide, nudged aside when it would
    /// land exactly on top of another.
    fn paste_element(&mut self, mut element: kn_model::Element) {
        if let kn_model::ElementKind::Image { media, .. } = &element.kind
            && self.session.media.get(media).is_none()
        {
            return self.notify("That picture belongs to another video, so it can't be pasted here.");
        }
        while self.project().slides[self.slide].elements.iter().any(|e| e.frame == element.frame) {
            element.frame.x += COPY_OFFSET;
            element.frame.y += COPY_OFFSET;
        }
        self.add_element(element);
    }

    /// Copy, cut, paste and duplicate from the keyboard, unless the user is
    /// typing or a dialog is open.
    fn clipboard_keys(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        if ctx.egui_wants_keyboard_input() || crate::dialog_open(ctx) {
            return;
        }
        let (copy, cut, paste, duplicate) = ui.input_mut(|i| {
            let (mut copy, mut cut, mut paste) = (false, false, None);
            for event in &i.events {
                match event {
                    egui::Event::Copy => copy = true,
                    egui::Event::Cut => cut = true,
                    egui::Event::Paste(text) => paste = Some(text.clone()),
                    _ => {}
                }
            }
            (copy, cut, paste, i.consume_shortcut(&DUPLICATE))
        });
        if copy || cut {
            self.copy_selected(ctx);
        }
        if cut && let Some(index) = self.selected {
            self.run(Command::DeleteElement { slide: self.slide, index });
            self.session.seal();
            self.selected = None;
        }
        if let Some(text) = paste {
            self.paste_text(&text);
        }
        if duplicate {
            self.duplicate_selected();
        }
    }

    /// The guide lines the dragged item is snapped to, across the slide.
    fn draw_guides(&self, ui: &egui::Ui, view: &View) {
        if self.drag.is_none() {
            return;
        }
        let (cw, ch) = self.project().aspect.canvas_size();
        let slide = view.to_screen(Rect::new(0.0, 0.0, cw, ch));
        let stroke = Stroke::new(1.0, GUIDE);
        if let Some(x) = self.guides.x {
            ui.painter().vline(view.origin.x + x as f32 * view.scale, slide.y_range(), stroke);
        }
        if let Some(y) = self.guides.y {
            ui.painter().hline(slide.x_range(), view.origin.y + y as f32 * view.scale, stroke);
        }
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
            // Keep Left/Right here instead of letting egui move focus away.
            ui.memory_mut(|m| m.set_focus_lock_filter(response.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() }));
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
        response.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(tr("Drag to rotate. Hold Shift for 15° steps. Arrow keys adjust the angle."));
        handled
    }

    /// Item `index`'s area in slide units (text uses its laid-out height).
    fn bounds(&mut self, index: usize) -> Rect {
        let element = &self.session.project().slides[self.slide].elements[index];
        kn_render::element_bounds(element, &mut self.render.text)
    }

    /// The front-most item under a slide-unit point, allowing for rotation.
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

    /// Clicks select, and drags move or resize, the item under the pointer.
    /// A whole drag is one undo step.
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
            let mut frame = drag_frame(drag, f64::from(d.x), f64::from(d.y));
            self.guides = Guides::default();
            // A moved item snaps to the slide's edges and middle and to the
            // other items; holding Alt moves it freely.
            if matches!(drag.kind, DragKind::Move) && !ui.input(|i| i.modifiers.alt) {
                let current = self.project().slides[self.slide].elements[index].frame;
                let bounds = self.bounds(index);
                let moved = Rect::new(frame.x + bounds.x - current.x, frame.y + bounds.y - current.y, bounds.w, bounds.h);
                let count = self.project().slides[self.slide].elements.len();
                let others: Vec<Rect> = (0..count).filter(|&i| i != index).map(|i| self.bounds(i)).collect();
                let (dx, dy, guides) = snap_move(moved, self.project().aspect.canvas_size(), &others, f64::from(SNAP_DISTANCE / view.scale));
                frame.x += dx;
                frame.y += dy;
                self.guides = guides;
            }
            self.run(Command::SetFrame { slide: self.slide, index, frame });
        }
        if response.drag_stopped() {
            self.drag = None;
            self.guides = Guides::default();
            self.session.seal();
        }
        // Double-clicking words goes straight to typing them.
        if response.double_clicked() && self.selected_is_text() {
            self.focus_words = true;
        }
    }

    /// Keys for the selected item: Delete removes it, Escape deselects, arrows
    /// move it (Shift for bigger steps). Ignored while a widget has focus.
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
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.selected_is_text() && !crate::dialog_open(ui.ctx()) {
            self.focus_words = true;
        }
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

    /// The selected item is text, so its words can be typed.
    fn selected_is_text(&self) -> bool {
        self.selected.is_some_and(|i| matches!(self.project().slides[self.slide].elements[i].kind, kn_model::ElementKind::Text { .. }))
    }

    /// The selected item's outline on screen, padded out from its bounds.
    fn selection_rect(&mut self, view: &View, index: usize) -> egui::Rect {
        view.to_screen(self.bounds(index)).expand(SELECTION_PAD)
    }

    /// Draws the selected item's outline and corner handles, rotated with it.
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
        let now = self.playhead().as_secs_f64();
        let clock = |s: f64| format!("{}:{:02}", s as u64 / 60, s as u64 % 60);
        let text = crate::i18n::row(&[&crate::trf!("Slide {n} of {total}", n = self.slide + 1, total = self.project().slides.len()), &format!("{} / {}", clock(now), clock(total))], "   ·   ");
        ui.painter().text(at, egui::Align2::CENTER_TOP, text, egui::FontId::proportional(12.5), p.dim);
    }
}

/// Colour of the snapping guides: distinct from the blue selection.
const GUIDE: egui::Color32 = egui::Color32::from_rgb(0xf0, 0x3e, 0x9a);

/// The nearest of `targets` to any of an item's three `edges` (start,
/// middle, end) within `threshold`: how far to shift, and the line met.
fn snap_axis(edges: [f64; 3], targets: &[f64], threshold: f64) -> Option<(f64, f64)> {
    edges
        .iter()
        .flat_map(|edge| targets.iter().map(move |target| (target - edge, *target)))
        .filter(|(shift, _)| shift.abs() <= threshold)
        .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()))
}

/// Snaps a moved item's bounds to the slide's edges and middle and to the
/// edges and middles of the `others`. Returns the shift to apply in x and
/// y, and the guide lines to show.
fn snap_move(moved: Rect, (cw, ch): (f64, f64), others: &[Rect], threshold: f64) -> (f64, f64, Guides) {
    let mut xs = vec![0.0, cw / 2.0, cw];
    let mut ys = vec![0.0, ch / 2.0, ch];
    for r in others {
        xs.extend([r.x, r.x + r.w / 2.0, r.x + r.w]);
        ys.extend([r.y, r.y + r.h / 2.0, r.y + r.h]);
    }
    let x = snap_axis([moved.x, moved.x + moved.w / 2.0, moved.x + moved.w], &xs, threshold);
    let y = snap_axis([moved.y, moved.y + moved.h / 2.0, moved.y + moved.h], &ys, threshold);
    (x.map_or(0.0, |s| s.0), y.map_or(0.0, |s| s.0), Guides { x: x.map(|s| s.1), y: y.map(|s| s.1) })
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
        ui.painter().text(egui::pos2(rect.center().x, y), egui::Align2::CENTER_CENTER, tr(text), egui::FontId::new(size, family), color);
    }
}

/// What to do on an empty slide.
fn empty_hint(ui: &egui::Ui, rect: egui::Rect) {
    let color = egui::Color32::from_gray(128);
    ui.painter().text(rect.center() - Vec2::new(0.0, 12.0), egui::Align2::CENTER_CENTER, tr("This slide is empty"), egui::FontId::new(18.0, crate::theme::bold()), color);
    ui.painter().text(rect.center() + Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, tr("Add text, a shape or a picture with +, or drop files here"), egui::FontId::proportional(13.5), color);
}

/// Centre of the rotation knob above `rect`, turned with the item.
fn rotation_handle_position(rect: egui::Rect, rotation: f64) -> Pos2 {
    rotate_screen(rect.center_top() - Vec2::new(0.0, ROTATION_OFFSET), rect.center(), rotation)
}

/// An angle in degrees brought into -180..180.
fn normalized_rotation(degrees: f64) -> f64 { (degrees + 180.0).rem_euclid(360.0) - 180.0 }

/// The rotation in degrees after dragging the knob from `from` to `to`
/// (both relative to the item's centre), starting from `start` radians;
/// `snap` rounds to 15° steps.
fn dragged_rotation(start: f64, from: Vec2, to: Vec2, snap: bool) -> f64 {
    let delta = f64::from(to.y).atan2(f64::from(to.x)) - f64::from(from.y).atan2(f64::from(from.x));
    let degrees = (start + delta).to_degrees();
    normalized_rotation(if snap { (degrees / 15.0).round() * 15.0 } else { degrees })
}

/// Turns a point about `(cx, cy)` by `angle` radians.
fn rotate_point((x, y): (f64, f64), (cx, cy): (f64, f64), angle: f64) -> (f64, f64) {
    let (sin, cos) = angle.sin_cos();
    (cx + cos * (x - cx) - sin * (y - cy), cy + sin * (x - cx) + cos * (y - cy))
}

/// Turns a screen point about `centre` by `angle` radians.
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
    fn moving_snaps_to_the_slide_middle_and_to_other_items_only_when_close() {
        let canvas = (1920.0, 1080.0);
        // Its middle is 4 units left of the slide's middle: pulled onto it.
        let (dx, dy, guides) = snap_move(Rect::new(856.0, 300.0, 200.0, 100.0), canvas, &[], 8.0);
        assert_eq!((dx, dy), (4.0, 0.0));
        assert_eq!(guides, Guides { x: Some(960.0), y: None });
        // Its top is 5 units below another item's bottom edge.
        let other = Rect::new(100.0, 100.0, 300.0, 195.0);
        let (dx, dy, guides) = snap_move(Rect::new(500.0, 300.0, 200.0, 100.0), canvas, &[other], 8.0);
        assert_eq!((dx, dy), (0.0, -5.0));
        assert_eq!(guides.y, Some(295.0));
        // Far from everything: left alone.
        let (dx, dy, guides) = snap_move(Rect::new(500.0, 420.0, 200.0, 100.0), canvas, &[], 8.0);
        assert_eq!((dx, dy, guides), (0.0, 0.0, Guides::default()));
    }

    #[test]
    fn pasting_adds_items_safely_and_refuses_what_it_cannot_use() {
        use egui_kittest::Harness;
        let mut app = Harness::builder().build_eframe(|cc| KinettoApp::new(cc)).into_state();
        let mut project = kn_model::Project::sample();
        let frame = Rect::new(100., 100., 200., 100.);
        project.slides[0].elements = vec![kn_model::Element::shape(frame, kn_geom::Color::rgb(50, 100, 200))];
        app.session = kn_engine::Session::new(project);
        app.recovery = None;
        let count = |app: &KinettoApp| app.project().slides[0].elements.len();

        // A copy landing exactly on another item is nudged until it is clear.
        let copy = app.project().slides[0].elements[0].clone();
        app.paste_element(copy.clone());
        app.paste_element(copy);
        let frames: Vec<Rect> = app.project().slides[0].elements.iter().map(|e| e.frame).collect();
        assert_eq!(frames[1], Rect::new(100. + COPY_OFFSET, 100. + COPY_OFFSET, 200., 100.));
        assert_eq!(frames[2], Rect::new(100. + 2. * COPY_OFFSET, 100. + 2. * COPY_OFFSET, 200., 100.));

        // A picture this video doesn't hold is refused, with a reason.
        app.paste_element(kn_model::Element::image("not-in-this-video".into(), frame));
        assert_eq!(count(&app), 3);
        assert!(app.toast.take().is_some());

        // A damaged copied item is refused; blank text does nothing.
        app.paste_text(&format!("{CLIP_PREFIX}{{not json"));
        assert!(app.toast.take().is_some());
        app.paste_text("  \n ");
        assert_eq!(count(&app), 3);
        assert!(app.toast.is_none());

        // Other words become a text item, cut to a sensible length.
        app.paste_text(&"x".repeat(PASTE_TEXT_LIMIT + 50));
        assert_eq!(count(&app), 4);
        let kn_model::ElementKind::Text { text, .. } = &app.project().slides[0].elements[3].kind else { panic!("pasted words should be text") };
        assert_eq!(text.chars().count(), PASTE_TEXT_LIMIT);
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
