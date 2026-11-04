//! libadwaita-style widgets for egui: flat and suggested buttons, a switch,
//! toggle groups, preference groups with boxed lists, alert dialogs and
//! toasts. Each reports its role and label for screen readers.

use crate::icons::{self, Icon};
use crate::theme::{self, BUTTON_RADIUS, CARD_RADIUS, DIALOG_RADIUS, palette};
use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, Response, RichText, Role, Sense, Stroke, Ui, Vec2, WidgetInfo};

/// The kind of button, which sets its colours (as libadwaita's style classes do).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The standard grey button.
    Normal,
    /// No background until hovered: header bars and toolbars.
    Flat,
    /// The one main action of a view, in the accent colour.
    Suggested,
    /// Deletes or discards something.
    Destructive,
}

/// Background and text colour for a button of `kind` in its current
/// state (resting, hovered or pressed).
fn fills(ui: &Ui, kind: Kind, response: &Response) -> (Color32, Color32) {
    let p = palette(ui.ctx());
    let state = if response.is_pointer_button_down_on() { 2 } else if response.hovered() { 1 } else { 0 };
    match kind {
        Kind::Normal => (p.button[state], p.fg),
        Kind::Flat => (if state == 0 { Color32::TRANSPARENT } else { p.button[state - 1] }, p.fg),
        Kind::Suggested => (lighten(p.accent_bg, state), p.accent_fg),
        Kind::Destructive => (lighten(p.destructive_bg, state), Color32::WHITE),
    }
}

/// Hover brightens, press darkens, as Adwaita does for coloured buttons.
fn lighten(c: Color32, state: usize) -> Color32 {
    match state {
        1 => c.lerp_to_gamma(Color32::WHITE, 0.08),
        2 => c.lerp_to_gamma(Color32::BLACK, 0.12),
        _ => c,
    }
}

/// A button with an optional icon before its label.
pub fn button(ui: &mut Ui, icon: Option<Icon>, label: &str, kind: Kind) -> Response {
    let font = egui::FontId::new(14.0, if kind == Kind::Suggested || kind == Kind::Destructive { theme::bold() } else { egui::FontFamily::Proportional });
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let icon_w = if icon.is_some() { if label.is_empty() { 16.0 } else { 22.0 } } else { 0.0 };
    let pad = if label.is_empty() { 9.0 } else { 14.0 };
    let size = Vec2::new((galley.size().x + icon_w + pad * 2.0).max(34.0), 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let (bg, fg) = fills(ui, kind, &response);
        let fg = if enabled { fg } else { fg.gamma_multiply(0.45) };
        ui.painter().rect_filled(rect, BUTTON_RADIUS, bg);
        let mut x = rect.center().x - (galley.size().x + icon_w) / 2.0;
        if let Some(icon) = icon {
            let r = egui::Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
            icons::paint(ui.painter(), r, icon, fg);
            x += icon_w;
        }
        ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
        if response.has_focus() {
            ui.painter().rect_stroke(rect.expand(2.0), BUTTON_RADIUS + 2, Stroke::new(2.0, palette(ui.ctx()).accent.gamma_multiply(0.6)), egui::StrokeKind::Outside);
        }
    }
    response
}

/// A button filling `width`, for dialog button rows.
pub fn wide_button(ui: &mut Ui, label: &str, kind: Kind, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    let (bg, fg) = fills(ui, kind, &response);
    let fg = if enabled { fg } else { fg.gamma_multiply(0.45) };
    ui.painter().rect_filled(rect, BUTTON_RADIUS, bg);
    let family = if matches!(kind, Kind::Suggested | Kind::Destructive) { theme::bold() } else { egui::FontFamily::Proportional };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::new(14.0, family), fg);
    focus_ring(ui, rect, &response, BUTTON_RADIUS);
    response
}

/// An icon-only flat button; `tooltip` doubles as its accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, tooltip));
    if ui.is_rect_visible(rect) {
        let (bg, fg) = fills(ui, Kind::Flat, &response);
        ui.painter().rect_filled(rect, BUTTON_RADIUS, bg);
        icons::paint(ui.painter(), rect, icon, if enabled { fg } else { fg.gamma_multiply(0.4) });
    }
    focus_ring(ui, rect, &response, BUTTON_RADIUS);
    response.on_hover_text(tooltip)
}

/// The small round window buttons (minimise, maximise, close).
pub fn window_button(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let p = palette(ui.ctx());
    let state = if response.is_pointer_button_down_on() { 2 } else if response.hovered() { 1 } else { 0 };
    let circle = egui::Rect::from_center_size(rect.center(), Vec2::splat(24.0));
    ui.painter().rect_filled(circle, 12, p.button[state]);
    icons::paint(ui.painter(), circle.shrink(5.0), icon, p.fg);
    focus_ring(ui, circle, &response, 12);
    response.on_hover_text(label)
}

/// An on/off switch that takes effect immediately.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(46.0, 26.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let value = *on;
    response.widget_info(|| WidgetInfo::selected(Role::Switch, ui.is_enabled(), value, label));
    let p = palette(ui.ctx());
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = p.button[1].lerp_to_gamma(p.accent_bg, t);
    ui.painter().rect_filled(rect, 13, if response.hovered() { lighten(track, 1) } else { track });
    let x = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 10.0, Color32::WHITE);
    ui.painter().circle_stroke(egui::pos2(x, rect.center().y), 10.0, Stroke::new(1.0, Color32::from_black_alpha(25)));
    focus_ring(ui, rect, &response, 13);
    response
}

/// A GtkScale-like slider: thin rail, accent fill, white knob. Arrow keys
/// step it when focused. Report changes via `changed()`, and the end of a
/// drag via `drag_stopped()`.
pub fn slider(ui: &mut Ui, value: &mut f64, range: std::ops::RangeInclusive<f64>, step: f64, label: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(ui.available_width().clamp(40.0, 150.0), 26.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let rail = rect.shrink2(Vec2::new(10.0, 0.0));
    let snap = |v: f64| ((v / step).round() * step).clamp(lo, hi);
    let before = *value;
    if let Some(pos) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked())
    {
        let t = f64::from(((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0));
        *value = snap(lo + t * (hi - lo));
    }
    if response.has_focus() {
        // Keep Left/Right on the slider instead of letting egui move focus
        // away, and consume them so nothing else reacts too.
        ui.memory_mut(|m| m.set_focus_lock_filter(response.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() }));
        let (left, right) = ui.input_mut(|i| {
            (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
        });
        if left || right {
            *value = snap(*value + if right { step } else { -step });
        }
    }
    // Screen readers and assistive tools can set or step the value too.
    let (set, up, down) = ui.input(|i| {
        use egui::accesskit::{Action, ActionData};
        let set = i.accesskit_action_requests(response.id, Action::SetValue).filter_map(|r| match r.data {
            Some(ActionData::NumericValue(v)) => Some(v),
            _ => None,
        }).last();
        (set, i.has_accesskit_action_request(response.id, Action::Increment), i.has_accesskit_action_request(response.id, Action::Decrement))
    });
    if let Some(v) = set {
        *value = snap(v);
    } else if up || down {
        *value = snap(*value + if up { step } else { -step });
    }
    if *value != before {
        response.mark_changed();
    }
    let shown = *value;
    response.widget_info(|| WidgetInfo::slider(ui.is_enabled(), shown, label));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        use egui::accesskit::Action;
        node.set_min_numeric_value(lo);
        node.set_max_numeric_value(hi);
        node.set_numeric_value_step(step);
        node.add_action(Action::SetValue);
        node.add_action(Action::Increment);
        node.add_action(Action::Decrement);
    });

    let p = palette(ui.ctx());
    let t = if hi > lo { ((*value - lo) / (hi - lo)).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let x = egui::lerp(rail.left()..=rail.right(), t);
    let y = rect.center().y;
    let track = egui::Rect::from_min_max(egui::pos2(rail.left(), y - 2.0), egui::pos2(rail.right(), y + 2.0));
    ui.painter().rect_filled(track, 2, p.button[1]);
    ui.painter().rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(x, track.max.y)), 2, p.accent_bg);
    let knob = egui::pos2(x, y);
    ui.painter().circle_filled(knob + Vec2::new(0.0, 1.0), 10.0, Color32::from_black_alpha(30));
    ui.painter().circle_filled(knob, 10.0, Color32::WHITE);
    ui.painter().circle_stroke(knob, 10.0, Stroke::new(if response.hovered() || response.dragged() { 1.5 } else { 1.0 }, Color32::from_black_alpha(if response.hovered() { 60 } else { 35 })));
    focus_ring(ui, rect, &response, BUTTON_RADIUS);
    response
}

/// Draws the keyboard-focus outline around `rect` when it has focus.
fn focus_ring(ui: &Ui, rect: egui::Rect, response: &Response, radius: u8) {
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), radius, Stroke::new(2.0, palette(ui.ctx()).accent), egui::StrokeKind::Outside);
    }
}

/// GNOME's standard palette: light, normal and dark shades per hue, then
/// neutrals.
const PALETTE: [[u32; 3]; 7] = [
    [0x99c1f1, 0x3584e4, 0x1a5fb4],
    [0x8ff0a4, 0x33d17a, 0x26a269],
    [0xf9f06b, 0xf6d32d, 0xe5a50a],
    [0xffbe6f, 0xff7800, 0xc64600],
    [0xf66151, 0xe01b24, 0xa51d2d],
    [0xdc8add, 0x9141ac, 0x613583],
    [0xcdab8f, 0x986a44, 0x63452c],
];
/// GNOME's neutral swatches, white to black.
const NEUTRALS: [u32; 6] = [0xffffff, 0xdeddda, 0x9a9996, 0x5e5c64, 0x241f31, 0x000000];

/// A colour from a `0xRRGGBB` number.
fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// "#3584e4" or "#38e" to a colour.
fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        6 => Some(hex(v)),
        3 => Some(Color32::from_rgb(((v >> 8) & 0xf) as u8 * 17, ((v >> 4) & 0xf) as u8 * 17, (v & 0xf) as u8 * 17)),
        _ => None,
    }
}

/// A colour button opening a palette: the look's own colours, GNOME's
/// palette and a hex field. Returns (changed, finished); finished ends the
/// edit as one undo step.
pub fn color_button(ui: &mut Ui, color: &mut Color32, look: &[Color32], label: &str) -> (bool, bool) {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("{label}: choose a colour")));
    ui.painter().rect(rect.shrink(2.0), BUTTON_RADIUS, *color, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    if response.hovered() {
        ui.painter().rect_stroke(rect, BUTTON_RADIUS + 2, Stroke::new(2.0, p.accent_bg.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    }
    let (mut changed, mut finished) = (false, false);
    let hex_id = response.id.with("hex");
    egui::Popup::from_toggle_button_response(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(248.0)
        .show(|ui| {
            let mut pick = |ui: &mut Ui, c: Color32| {
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                let selected = c == *color;
                ui.painter().rect(r.shrink(1.0), 6, c, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
                if selected || resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.0), 8, Stroke::new(2.0, if selected { p.accent_bg } else { p.border }), egui::StrokeKind::Outside);
                }
                let [r8, g8, b8, _] = c.to_array();
                let resp = resp.on_hover_text(format!("#{r8:02x}{g8:02x}{b8:02x}"));
                if resp.clicked() {
                    *color = c;
                    changed = true;
                    finished = true;
                    ui.close();
                }
            };
            ui.spacing_mut().item_spacing = Vec2::splat(6.0);
            if !look.is_empty() {
                ui.label(RichText::new("This look").size(12.0).color(p.dim));
                ui.horizontal_wrapped(|ui| look.iter().for_each(|c| pick(ui, *c)));
                ui.add_space(4.0);
            }
            ui.label(RichText::new("Colours").size(12.0).color(p.dim));
            for shade in 0..3 {
                ui.horizontal(|ui| PALETTE.iter().for_each(|hue| pick(ui, hex(hue[shade]))));
            }
            ui.horizontal(|ui| NEUTRALS.iter().for_each(|c| pick(ui, hex(*c))));
            ui.add_space(4.0);
            ui.label(RichText::new("Exact colour").size(12.0).color(p.dim));
            let [r8, g8, b8, _] = color.to_array();
            let mut text = ui.data_mut(|d| d.get_temp::<String>(hex_id)).unwrap_or_else(|| format!("#{r8:02x}{g8:02x}{b8:02x}"));
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(120.0).hint_text("#3584e4"));
            match parse_hex(&text) {
                Some(c) if field.changed() => {
                    *color = c;
                    changed = true;
                }
                None if !text.trim().is_empty() => {
                    ui.label(RichText::new("Type a colour like #3584e4").size(12.0).color(p.destructive));
                }
                _ => {}
            }
            finished |= field.lost_focus();
            ui.data_mut(|d| d.insert_temp(hex_id, text));
        });
    if !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response)) {
        ui.data_mut(|d| d.remove::<String>(hex_id));
    }
    (changed, finished)
}

/// A dropdown (GtkDropDown): the current choice and a small chevron.
pub fn dropdown<R>(ui: &mut Ui, id: &str, selected: &str, add: impl FnOnce(&mut Ui) -> R) -> Response {
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .width(ui.available_width().min(156.0))
        .icon(|ui, rect, visuals, _open| {
            icons::paint(ui.painter(), rect.expand(3.0), Icon::Down, visuals.fg_stroke.color);
        })
        .show_ui(ui, add).response
}

/// A row of mutually exclusive toggles in one rounded track; the chosen
/// one sits raised. Returns true when the choice changed.
pub fn toggle_group<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = palette(ui.ctx());
    let mut changed = false;
    let width = ui.available_width();
    let each = ((width - 6.0) / options.len() as f32).floor();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(each * options.len() as f32 + 6.0, 34.0), Sense::hover());
    ui.painter().rect_filled(rect, BUTTON_RADIUS + 2, p.button[0]);
    for (i, (option, label)) in options.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(rect.left() + 3.0 + each * i as f32, rect.top() + 3.0), Vec2::new(each, 28.0));
        let id = ui.scope_id().with(("toggle", i, label));
        let response = ui.interact(r, id, Sense::click());
        let selected = *value == *option;
        response.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, *label));
        if response.clicked() && !selected {
            *value = *option;
            changed = true;
        }
        if selected {
            let raised = if ui.ctx().theme() == egui::Theme::Dark { p.button[2] } else { Color32::WHITE };
            ui.painter().rect_filled(r, BUTTON_RADIUS, raised);
        } else if response.hovered() {
            ui.painter().rect_filled(r, BUTTON_RADIUS, p.button[0]);
        }
        focus_ring(ui, r, &response, BUTTON_RADIUS);
        let font = egui::FontId::new(13.5, if selected { theme::semibold() } else { egui::FontFamily::Proportional });
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, *label, font, p.fg);
    }
    changed
}

/// A titled preferences group, like AdwPreferencesGroup.
pub fn group(ui: &mut Ui, title: &str, description: Option<&str>, add: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).family(theme::bold()).size(14.0));
    if let Some(d) = description {
        ui.label(RichText::new(d).size(12.5).color(palette(ui.ctx()).dim));
    }
    ui.add_space(6.0);
    add(ui);
    ui.add_space(16.0);
}

/// A card holding rows separated by thin lines (AdwBoxedList).
pub fn boxed_list(ui: &mut Ui, add: impl FnOnce(&mut List<'_>)) {
    let p = palette(ui.ctx());
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CORNER)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_width(ui.available_width());
            let mut list = List { ui, first: true };
            add(&mut list);
        });
}

/// Corner rounding of boxed lists.
const CORNER: CornerRadius = CornerRadius::same(CARD_RADIUS);

/// The rows of a [`boxed_list`]; each row draws the line above itself.
pub struct List<'a> {
    /// The list's inside, where rows are laid out.
    ui: &'a mut Ui,
    /// No row has been added yet, so the next one needs no separator.
    first: bool,
}

impl List<'_> {
    /// The egui context, for rows that need the theme or time.
    pub fn ctx(&self) -> &egui::Context {
        self.ui.ctx()
    }

    /// Draws the thin line between rows, except above the first.
    fn separator(&mut self) {
        if !self.first {
            let rect = self.ui.available_rect_before_wrap();
            let y = rect.top();
            self.ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, palette(self.ui.ctx()).border));
        }
        self.first = false;
    }

    /// A row with a title (and optional subtitle) on the left and a control
    /// on the right.
    pub fn row(&mut self, title: &str, subtitle: Option<&str>, trailing: impl FnOnce(&mut Ui)) {
        self.separator();
        let p = palette(self.ui.ctx());
        Frame::new().inner_margin(Margin::symmetric(12, 10)).show(self.ui, |ui| {
            let width = ui.available_width();
            let control_width = (width * 0.55).clamp(100.0, 156.0);
            let label_width = (width - control_width - 12.0).max(48.0);
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 3.0);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(label_width, 34.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(label_width);
                    if subtitle.is_none() {
                        ui.add_space((34.0 - ui.text_style_height(&egui::TextStyle::Body)) * 0.5);
                    }
                    ui.label(title);
                    if let Some(s) = subtitle {
                        ui.label(RichText::new(s).size(12.0).color(p.dim));
                    }
                });
                ui.allocate_ui_with_layout(Vec2::new(control_width, 34.0), Layout::right_to_left(Align::Center), trailing);
            });
        });
    }

    /// A row whose content fills its width (text fields, swatches, sliders).
    pub fn full(&mut self, add: impl FnOnce(&mut Ui)) {
        self.separator();
        Frame::new().inner_margin(Margin::same(12)).show(self.ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    }
}

/// A centred dialog with a heading, body and a row of verb buttons (the
/// AdwAlertDialog layout). Returns the index of the button pressed.
///
/// Escape answers with the first button (the safe "Cancel" or "Close"), and
/// Enter with the suggested one, as GNOME dialogs do.
pub fn alert(ctx: &egui::Context, id: &str, heading: &str, body: &str, buttons: &[(&str, Kind)]) -> Option<usize> {
    let mut chosen = None;
    let p = palette(ctx);
    let frame = Frame::new()
        .fill(ctx.style_of(ctx.theme()).visuals.window_fill)
        .corner_radius(DIALOG_RADIUS)
        .inner_margin(Margin::same(24))
        .shadow(ctx.style_of(ctx.theme()).visuals.window_shadow);
    let modal = egui::Modal::new(egui::Id::unique(id)).frame(frame).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(heading).family(theme::bold()).size(19.0));
            ui.add_space(4.0);
            ui.label(RichText::new(body).color(p.dim));
        });
        ui.add_space(20.0);
        let each = (ui.available_width() - 8.0 * (buttons.len() as f32 - 1.0)) / buttons.len() as f32;
        ui.horizontal(|ui| {
            for (i, (label, kind)) in buttons.iter().enumerate() {
                if wide_button(ui, label, *kind, each).clicked() {
                    chosen = Some(i);
                }
            }
        });
    });
    if chosen.is_none() && modal.is_top_modal && !modal.any_popup_open {
        let (escape, enter) = ctx.input_mut(|i| {
            (i.consume_key(egui::Modifiers::NONE, egui::Key::Escape), i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        });
        if escape && !buttons.is_empty() {
            chosen = Some(0);
        } else if enter {
            chosen = buttons.iter().position(|(_, kind)| *kind == Kind::Suggested);
        }
    }
    chosen
}

/// The dark pill at the bottom of the window, with an optional action.
/// Returns true when the action was pressed.
pub fn toast(ctx: &egui::Context, message: &str, action: Option<&str>) -> bool {
    let p = palette(ctx);
    let mut pressed = false;
    egui::Area::new(egui::Id::unique("toast")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0]).show(ctx, |ui| {
        // Long messages wrap instead of running off a narrow window.
        let max = (ctx.content_rect().width() - 96.0).clamp(200.0, 560.0);
        Frame::new().fill(p.toast).corner_radius(20).inner_margin(Margin::symmetric(18, 8)).shadow(ctx.style_of(ctx.theme()).visuals.popup_shadow).show(ui, |ui| {
            ui.set_max_width(max);
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(message).color(Color32::WHITE)).wrap());
                if let Some(action) = action {
                    let r = ui.add(egui::Button::new(RichText::new(action).color(Color32::WHITE).family(theme::bold())).fill(Color32::from_white_alpha(28)).corner_radius(14));
                    pressed = r.clicked();
                }
            });
        });
    });
    pressed
}

/// A full-width strip under the header bar for a persistent message.
pub fn banner(ui: &mut Ui, message: &str, buttons: &[(&str, Kind)]) -> Option<usize> {
    let p = palette(ui.ctx());
    let mut chosen = None;
    Frame::new().fill(p.banner).inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Buttons keep their place on the right; the message wraps beside them.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            for (i, (label, kind)) in buttons.iter().enumerate().rev() {
                if button(ui, None, label, *kind).clicked() {
                    chosen = Some(i);
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(message).wrap());
            });
        });
    });
    chosen
}
