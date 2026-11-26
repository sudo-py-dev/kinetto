//! A text box that lays out and edits its text with parley, so Hebrew and
//! mixed Hebrew and English read, edit and select correctly.
//!
//! egui's own `TextEdit` has no bidi: it shows the words of a Hebrew line in
//! left-to-right order, and its cursor maps characters to the wrong glyphs.
//! Here parley does the hard parts — paragraph direction, word order, line
//! breaking, where the cursor goes for a click or an arrow key, the selection
//! and IME composition — and egui only paints each character where parley
//! placed it. Parley is given the same font files egui draws with
//! ([`theme::text_font_files`]), so the measurements match.

use crate::i18n;
use crate::theme::{self, palette};
use egui::{Color32, Event, Id, Key, Margin, Rect, Response, Sense, Ui, Vec2, WidgetInfo, pos2, vec2};
use parley::{BaseDirection, FontContext, FontFamily, FontFamilyName, LayoutContext, LineHeight, PlainEditor, PositionedLayoutItem, StyleProperty};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;

/// How a [`text_field`] looks.
pub struct Field<'a> {
    /// Several lines, with Enter starting a new one; otherwise one line that
    /// scrolls sideways.
    pub multiline: bool,
    /// Lines of room shown even when there is less text.
    pub rows: usize,
    /// Dim text shown while the box is empty (already translated).
    pub hint: &'a str,
    /// What screen readers call the box (already translated); the hint when empty.
    pub label: &'a str,
    /// Text size in points.
    pub size: f32,
    /// Space between the box's edge and the text.
    pub margin: Margin,
    /// Draw the rounded box behind the text.
    pub frame: bool,
}

impl Default for Field<'_> {
    fn default() -> Self {
        Field { multiline: false, rows: 1, hint: "", label: "", size: 14.0, margin: Margin::symmetric(12, 8), frame: true }
    }
}

/// Parley's font and layout caches, made once per thread.
struct Fonts {
    fonts: FontContext,
    layouts: LayoutContext<()>,
    /// Family names of [`theme::text_font_files`], in egui's order.
    stack: Vec<String>,
}

impl Fonts {
    fn new() -> Self {
        let mut fonts = FontContext::new();
        let mut stack = Vec::new();
        for file in theme::text_font_files() {
            let blob = parley::fontique::Blob::new(std::sync::Arc::new(file.to_vec()));
            for (family, _) in fonts.collection.register_fonts(blob, None) {
                if let Some(name) = fonts.collection.family_name(family) {
                    let name = name.to_owned();
                    if !stack.contains(&name) { stack.push(name); }
                }
            }
        }
        Fonts { fonts, layouts: LayoutContext::new(), stack }
    }
}

/// One box's editor, kept between frames.
struct State {
    editor: PlainEditor<()>,
    /// The frame it was last shown in; boxes not shown for a while are dropped.
    seen: u64,
    /// How far a one-line box has scrolled to keep the cursor in view.
    scroll: f32,
    /// The settings its layout was made with, to redo it when they change.
    size: f32,
    width: Option<f32>,
    direction: BaseDirection,
}

thread_local! {
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
    static STATES: RefCell<HashMap<Id, State>> = RefCell::new(HashMap::new());
}

/// A text box editing `text`. Works like `egui::TextEdit`: the response
/// reports `changed`, `has_focus` and `lost_focus`, and `request_focus` on it
/// puts the cursor in the box.
pub fn text_field(ui: &mut Ui, text: &mut String, field: Field<'_>) -> Response {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let frame_nr = ui.ctx().cumulative_frame_nr();
    let width = ui.available_width().max(48.0);
    let inner_width = (width - field.margin.sum().x).max(16.0);
    let focused = ui.memory(|m| m.has_focus(id));

    FONTS.with(|cell| {
        let mut cell = cell.borrow_mut();
        let fonts = cell.get_or_insert_with(Fonts::new);
        STATES.with(|states| {
            let mut states = states.borrow_mut();
            // Forget boxes that have not been on screen for a minute or so.
            states.retain(|_, s| frame_nr.saturating_sub(s.seen) < 3600);
            let state = states.entry(id).or_insert_with(|| new_state(fonts, field.size));
            state.seen = frame_nr;
            show(ui, id, text, &field, fonts, state, focused, width, inner_width)
        })
    })
}

fn new_state(fonts: &Fonts, size: f32) -> State {
    let mut editor = PlainEditor::new(size);
    let stack: Vec<FontFamilyName<'static>> = fonts.stack.iter().map(|name| FontFamilyName::Named(Cow::Owned(name.clone()))).collect();
    if !stack.is_empty() {
        editor.edit_styles().insert(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(stack))));
    }
    editor.edit_styles().insert(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.0)));
    State { editor, seen: 0, scroll: 0.0, size, width: None, direction: BaseDirection::Auto }
}

#[allow(clippy::too_many_arguments)]
fn show(ui: &mut Ui, id: Id, text: &mut String, field: &Field<'_>, fonts: &mut Fonts, state: &mut State, focused: bool, width: f32, inner_width: f32) -> Response {
    let p = palette(ui.ctx());
    let editor = &mut state.editor;

    // The caller's text wins when it changed from outside (undo, another item).
    if editor.text().to_string() != *text && !editor.is_composing() {
        editor.set_text(text);
    }
    // Text with no letters yet (empty, digits, punctuation) runs the way the
    // interface reads; otherwise its first letter decides.
    let direction = if text.chars().any(char::is_alphabetic) { BaseDirection::Auto } else if i18n::rtl() { BaseDirection::Rtl } else { BaseDirection::Ltr };
    if direction != state.direction {
        editor.set_base_direction(direction);
        state.direction = direction;
    }
    let wrap = field.multiline.then_some(inner_width);
    if wrap != state.width {
        editor.set_width(wrap);
        state.width = wrap;
    }
    if field.size != state.size {
        editor.edit_styles().insert(StyleProperty::FontSize(field.size));
        state.size = field.size;
    }
    editor.set_scale(1.0);

    // Room for the text, or for `rows` lines when it is shorter.
    let layout = editor.layout(&mut fonts.fonts, &mut fonts.layouts);
    let line_height = layout.lines().next().map_or(field.size * 1.25, |line| line.metrics().line_height);
    let text_height = if field.multiline { layout.height().max(line_height * field.rows as f32) } else { line_height };
    let size = vec2(width, text_height + field.margin.sum().y);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut response = ui.interact(rect, id, Sense::click_and_drag());
    let inner = rect - field.margin;

    // Where the layout's (0, 0) sits on screen. A one-line box keeps its text
    // at the edge where it starts reading and scrolls to keep the cursor in view.
    let layout_width = layout.width();
    let rtl = layout.is_rtl();
    let origin = |scroll: f32| {
        let x = if field.multiline {
            inner.left()
        } else if rtl {
            inner.right() - layout_width + scroll
        } else {
            inner.left() - scroll
        };
        pos2(x, inner.top())
    };

    // Pointer: click places the cursor, drag selects, double and triple
    // clicks select a word or a line, Shift-click extends.
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if let Some(pos) = response.interact_pointer_pos() {
        let at = pos - origin(state.scroll);
        let mut driver = state.editor.driver(&mut fonts.fonts, &mut fonts.layouts);
        let shift = ui.input(|i| i.modifiers.shift);
        if response.triple_clicked() {
            driver.select_line_at_point(at.x, at.y);
        } else if response.double_clicked() {
            driver.select_word_at_point(at.x, at.y);
        } else if response.drag_started() || response.clicked() {
            if shift { driver.shift_click_extension(at.x, at.y) } else { driver.move_to_point(at.x, at.y) }
        } else if response.dragged() {
            driver.extend_selection_to_point(at.x, at.y);
        }
        if !response.has_focus() {
            response.request_focus();
        }
    }

    // Keyboard, clipboard and IME while focused.
    if focused {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { horizontal_arrows: true, vertical_arrows: field.multiline, tab: false, escape: false }));
        let events = ui.input(|i| i.filtered_events(&egui::EventFilter { horizontal_arrows: true, vertical_arrows: field.multiline, tab: false, escape: true }));
        let mut copy = None;
        let mut leave = false;
        {
            let mut d = state.editor.driver(&mut fonts.fonts, &mut fonts.layouts);
            for event in events {
                match event {
                    Event::Text(t) => {
                        let t = if field.multiline { t } else { t.replace(['\n', '\r'], " ") };
                        if !t.is_empty() { d.insert_or_replace_selection(&t); }
                    }
                    Event::Paste(t) => {
                        let t = if field.multiline { t.replace("\r\n", "\n") } else { t.replace(['\n', '\r'], " ") };
                        d.insert_or_replace_selection(&t);
                    }
                    Event::Copy | Event::Cut => {
                        if let Some(selected) = d.editor.selected_text() {
                            copy = Some(selected.to_owned());
                            if matches!(event, Event::Cut) { d.delete_selection(); }
                        }
                    }
                    Event::Ime(egui::ImeEvent::Preedit { text: t, .. }) => {
                        if t.is_empty() { d.clear_compose() } else { d.set_compose(&t, None) }
                    }
                    Event::Ime(egui::ImeEvent::Commit(t)) => {
                        d.clear_compose();
                        if !t.is_empty() { d.insert_or_replace_selection(&t); }
                    }
                    Event::Key { key, pressed: true, modifiers: m, .. } => {
                        let (shift, word) = (m.shift, m.command || m.alt);
                        match key {
                            Key::ArrowLeft => match (shift, word) {
                                (false, false) => d.move_left(),
                                (true, false) => d.select_left(),
                                (false, true) => d.move_word_left(),
                                (true, true) => d.select_word_left(),
                            },
                            Key::ArrowRight => match (shift, word) {
                                (false, false) => d.move_right(),
                                (true, false) => d.select_right(),
                                (false, true) => d.move_word_right(),
                                (true, true) => d.select_word_right(),
                            },
                            Key::ArrowUp if field.multiline => if shift { d.select_up() } else { d.move_up() },
                            Key::ArrowDown if field.multiline => if shift { d.select_down() } else { d.move_down() },
                            Key::Home => match (shift, m.command) {
                                (false, false) => d.move_to_line_start(),
                                (true, false) => d.select_to_line_start(),
                                (false, true) => d.move_to_text_start(),
                                (true, true) => d.select_to_text_start(),
                            },
                            Key::End => match (shift, m.command) {
                                (false, false) => d.move_to_line_end(),
                                (true, false) => d.select_to_line_end(),
                                (false, true) => d.move_to_text_end(),
                                (true, true) => d.select_to_text_end(),
                            },
                            Key::Backspace => if word { d.backdelete_word() } else { d.backdelete() },
                            Key::Delete => if word { d.delete_word() } else { d.delete() },
                            Key::A if m.command => d.select_all(),
                            Key::Enter if field.multiline => d.insert_or_replace_selection("\n"),
                            Key::Enter | Key::Escape => leave = true,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(copied) = copy {
            ui.ctx().copy_text(copied);
        }
        if leave {
            response.surrender_focus();
        }
    }

    let edited = state.editor.text().to_string();
    if edited != *text {
        *text = edited;
        response.mark_changed();
    }

    // Keep the cursor of a one-line box in view.
    let layout = state.editor.layout(&mut fonts.fonts, &mut fonts.layouts);
    let layout_width = layout.width();
    let caret = state.editor.cursor_geometry(1.5);
    if !field.multiline {
        // `scroll` moves the text towards where it starts reading: left for
        // left-to-right text, right for right-to-left.
        if let Some(c) = caret {
            let x = origin(state.scroll).x + c.x0 as f32;
            let sign = if rtl { -1.0 } else { 1.0 };
            if x < inner.left() { state.scroll -= sign * (inner.left() - x); }
            if x > inner.right() { state.scroll += sign * (x - inner.right()); }
        }
        state.scroll = state.scroll.clamp(0.0, (layout_width - inner.width()).max(0.0));
    }
    let origin = origin(state.scroll);

    // Painting: the box, then the selection, the letters and the cursor.
    let focused = response.has_focus();
    if ui.is_rect_visible(rect) {
        if field.frame {
            let fill = ui.visuals().text_edit_bg_color();
            ui.painter().rect_filled(rect, theme::BUTTON_RADIUS, fill);
            if focused {
                ui.painter().rect_stroke(rect, theme::BUTTON_RADIUS, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
            }
        }
        let painter = ui.painter_at(inner.expand2(vec2(2.0, 1.0)));
        let to_rect = |b: parley::BoundingBox| Rect::from_min_max(origin + vec2(b.x0 as f32, b.y0 as f32), origin + vec2(b.x1 as f32, b.y1 as f32));
        if focused {
            for (b, _) in state.editor.selection_geometry() {
                painter.rect_filled(to_rect(b), 2.0, ui.visuals().selection.bg_fill);
            }
        }
        state.editor.refresh_layout(&mut fonts.fonts, &mut fonts.layouts);
        let layout = state.editor.try_layout().expect("just refreshed");
        let source = state.editor.raw_text();
        let font = egui::FontId::proportional(field.size);
        let compose = state.editor.raw_compose().clone();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                let baseline = run.baseline();
                let mut x = run.offset();
                for cluster in run.run().visual_clusters() {
                    let range = cluster.text_range();
                    let advance = cluster.advance();
                    let piece = &source[range.clone()];
                    if !piece.trim().is_empty() {
                        paint_piece(&painter, piece, &font, p.fg, origin + vec2(x, baseline));
                    }
                    if compose.as_ref().is_some_and(|c| c.start <= range.start && range.end <= c.end) {
                        let y = origin.y + baseline + 2.0;
                        painter.hline((origin.x + x)..=(origin.x + x + advance), y, egui::Stroke::new(1.0, p.fg));
                    }
                    x += advance;
                }
            }
        }
        if text.is_empty() && compose.is_none() && !field.hint.is_empty() {
            let hint = crate::adw::wrapped(ui, field.hint, &font, inner.width());
            let galley = ui.painter().layout(hint, font.clone(), ui.visuals().weak_text_color(), inner.width());
            let x = if i18n::rtl() { inner.right() - galley.size().x } else { inner.left() };
            painter.galley(pos2(x, inner.top()), galley, ui.visuals().weak_text_color());
        }
        if focused
            && let Some(c) = state.editor.cursor_geometry(2.0)
        {
            painter.rect_filled(to_rect(c), 0.0, ui.visuals().text_cursor.stroke.color);
        }
    }

    // The input method needs to know where the cursor is.
    if focused && let Some(c) = state.editor.cursor_geometry(2.0) {
        let cursor = Rect::from_min_max(origin + vec2(c.x0 as f32, c.y0 as f32), origin + vec2(c.x1 as f32, c.y1 as f32));
        ui.ctx().output_mut(|o| o.ime = Some(egui::output::IMEOutput { purpose: Default::default(), rect, cursor_rect: cursor, should_interrupt_composition: false }));
    }

    let role = if field.multiline { egui::Role::MultilineTextInput } else { egui::Role::TextInput };
    response.widget_info(|| {
        let mut info = WidgetInfo::text_edit(ui.is_enabled(), text.as_str(), text.as_str(), i18n::spoken(field.hint));
        info.role = role;
        let label = if field.label.is_empty() { field.hint } else { field.label };
        info.label = (!label.is_empty()).then(|| i18n::spoken(label));
        info
    });
    response
}

/// Draws one cluster (a letter, or a letter with its marks) with its baseline
/// at `at`, using egui's font so it looks like every other label.
fn paint_piece(painter: &egui::Painter, piece: &str, font: &egui::FontId, color: Color32, at: egui::Pos2) {
    let galley = painter.layout_no_wrap(piece.to_owned(), font.clone(), color);
    let Some(row) = galley.rows.first() else { return };
    let baseline = row.pos.y + row.row.glyphs.first().map_or(0.0, |g| g.pos.y);
    painter.galley(at - Vec2::new(0.0, baseline), galley, color);
}

/// The text of `text`'s clusters from left to right on screen, as parley
/// lays it out in a box of this kind.
#[cfg(test)]
fn visual_order(text: &str) -> String {
    FONTS.with(|cell| {
        let mut cell = cell.borrow_mut();
        let fonts = cell.get_or_insert_with(Fonts::new);
        let mut state = new_state(fonts, 14.0);
        state.editor.set_text(text);
        let layout = state.editor.layout(&mut fonts.fonts, &mut fonts.layouts);
        let mut out = String::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else { continue };
                for cluster in run.run().visual_clusters() {
                    out.push_str(&text[cluster.text_range()]);
                }
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn hebrew_words_are_laid_out_right_to_left_and_english_inside_stays_in_order() {
        // Left to right on screen: the last Hebrew word first.
        assert_eq!(visual_order("שלום עולם"), "םלוע םולש");
        // An English word inside Hebrew keeps its own order.
        assert_eq!(visual_order("שלום abc"), "abc םולש");
        assert_eq!(visual_order("Hello world"), "Hello world");
    }

    /// A box editing a string, in a Hebrew or English window.
    fn harness(multiline: bool) -> Harness<'static, String> {
        let mut h = Harness::builder().with_size([400.0, 200.0]).build_ui_state(
            move |ui, text: &mut String| {
                text_field(ui, text, Field { multiline, hint: "Type here", ..Default::default() });
            },
            String::new(),
        );
        theme::install(&h.ctx);
        h.run();
        h
    }

    fn press(h: &mut Harness<'_, String>, key: Key, modifiers: egui::Modifiers) {
        h.key_press_modifiers(modifiers, key);
        h.run();
    }

    #[test]
    fn typing_editing_and_cursor_keys_follow_the_reading_direction() {
        let mut h = harness(false);
        h.get_by_role(egui::Role::TextInput).focus();
        h.run();
        h.get_by_role(egui::Role::TextInput).type_text("שלום עולם");
        h.run();
        assert_eq!(h.state(), "שלום עולם");
        press(&mut h, Key::Backspace, egui::Modifiers::NONE);
        assert_eq!(h.state(), "שלום עול", "Backspace removes the last letter typed");
        // Home goes to where the line starts reading (its right edge here).
        press(&mut h, Key::Home, egui::Modifiers::NONE);
        h.get_by_role(egui::Role::TextInput).type_text("א");
        h.run();
        assert_eq!(h.state(), "אשלום עול");
        // In right-to-left text, the left arrow moves forward through the words.
        press(&mut h, Key::ArrowLeft, egui::Modifiers::NONE);
        h.get_by_role(egui::Role::TextInput).type_text("ב");
        h.run();
        assert_eq!(h.state(), "אשבלום עול");
        // Select all, then typing replaces everything.
        press(&mut h, Key::A, egui::Modifiers::COMMAND);
        h.get_by_role(egui::Role::TextInput).type_text("abc");
        h.run();
        assert_eq!(h.state(), "abc");
        press(&mut h, Key::ArrowLeft, egui::Modifiers::NONE);
        press(&mut h, Key::Delete, egui::Modifiers::NONE);
        assert_eq!(h.state(), "ab", "left-to-right text: Left goes back one letter");
    }

    #[test]
    fn clicking_puts_the_cursor_where_the_pointer_is() {
        let mut h = harness(false);
        *h.state_mut() = "שלום עולם".to_owned();
        h.run();
        let rect = h.get_by_role(egui::Role::TextInput).rect();
        // The text sits at the right edge; a click at its right end is its start.
        for (x, expected) in [(rect.right() - 13.0, "אשלום עולם"), (rect.left() + 20.0, "שלום עולםא")] {
            *h.state_mut() = "שלום עולם".to_owned();
            h.run();
            let at = egui::pos2(x, rect.center().y);
            h.event(egui::Event::PointerMoved(at));
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
            h.run();
            // (Clicking the empty space past the text's left end puts the cursor at its end.)
            h.get_by_role(egui::Role::TextInput).type_text("א");
            h.run();
            assert_eq!(h.state(), expected);
        }
    }

    /// With `KINETTO_SCREENS` set, shows a box in a Hebrew window with a
    /// selected word, and one with the cursor in mixed text.
    #[test]
    fn render_hebrew_selection_and_cursor() {
        let Some(dir) = std::env::var_os("KINETTO_SCREENS").map(std::path::PathBuf::from) else { return };
        crate::i18n::set(crate::i18n::Language::Hebrew);
        let mut h = Harness::builder().with_size([420.0, 150.0]).with_step_dt(1.0 / 60.0).wgpu().build_ui_state(
            |ui, texts: &mut (String, String, bool)| {
                crate::i18n::set(crate::i18n::Language::Hebrew);
                // The app's fonts (with Hebrew) take effect from the next frame.
                if !texts.2 {
                    theme::install(ui.ctx());
                    texts.2 = true;
                    return;
                }
                ui.add_space(8.0);
                text_field(ui, &mut texts.0, Field { label: "Title", ..Default::default() });
                ui.add_space(8.0);
                text_field(ui, &mut texts.1, Field { multiline: true, rows: 2, label: "Words", ..Default::default() });
            },
            ("שלום עולם ומה שלומך".to_owned(), "כתבו כאן Kinetto 2026\nשורה שנייה".to_owned(), false),
        );
        h.run();
        let fields = h.get_all_by_role(egui::Role::TextInput).map(|n| n.rect()).collect::<Vec<_>>();
        // Double-click the second word from the right to select it.
        let at = egui::pos2(fields[0].right() - 60.0, fields[0].center().y);
        h.event(egui::Event::PointerMoved(at));
        // Steps of 1/60 s (not the harness's default 1/4 s), so the two clicks
        // land within the double-click time.
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
        }
        h.step();
        h.run();
        assert_eq!(h.state().0, "שלום עולם ומה שלומך", "selecting changes nothing");
        h.render().unwrap().save(dir.join("text-field-he-selection.png")).unwrap();
        h.get_by_role(egui::Role::MultilineTextInput).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Home);
        h.run();
        for _ in 0..6 {
            h.key_press(Key::ArrowLeft);
            h.run();
        }
        h.render().unwrap().save(dir.join("text-field-he-cursor.png")).unwrap();
        crate::i18n::set(crate::i18n::Language::English);
    }

    #[test]
    fn double_click_selects_a_hebrew_word_and_typing_replaces_it() {
        let mut h = Harness::builder().with_size([400.0, 120.0]).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, text: &mut String| {
                text_field(ui, text, Field { label: "Title", ..Default::default() });
            },
            String::new(),
        );
        theme::install(&h.ctx);
        h.run();
        *h.state_mut() = "שלום עולם".to_owned();
        h.run();
        // The text sits at the right edge; its first word is the rightmost.
        let rect = h.get_by_role(egui::Role::TextInput).rect();
        let at = egui::pos2(rect.right() - 25.0, rect.center().y);
        h.event(egui::Event::PointerMoved(at));
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            }
        }
        h.step();
        h.run();
        h.get_by_role(egui::Role::TextInput).type_text("ערב");
        h.run();
        assert_eq!(h.state(), "ערב עולם", "the first word was selected and replaced");
    }

    #[test]
    fn several_lines_keep_enter_and_report_their_value() {
        let mut h = harness(true);
        h.get_by_role(egui::Role::MultilineTextInput).focus();
        h.run();
        h.get_by_role(egui::Role::MultilineTextInput).type_text("שורה");
        press(&mut h, Key::Enter, egui::Modifiers::NONE);
        h.get_by_role(egui::Role::MultilineTextInput).type_text("line");
        h.run();
        assert_eq!(h.state(), "שורה\nline");
        assert_eq!(h.get_by_role(egui::Role::MultilineTextInput).value().as_deref(), Some("שורה\nline"));
    }
}
