//! "Find an action": one box that reaches everything the editor can do by
//! typing a few letters, and the keyboard shortcuts window.

use crate::i18n::tr;
use crate::adw::{self, Kind};
use crate::icons::{self, Icon};
use crate::theme::{self, DIALOG_RADIUS, palette};
use crate::{KinettoApp, Pending, export, settings};
use egui::{Key, Modifiers, RichText, Sense, Vec2};
use kn_engine::Command;
use kn_geom::Aspect;

/// Most rows shown at once before the list scrolls.
const VISIBLE_ROWS: f32 = 8.0;
/// Height of one result row.
const ROW: f32 = 36.0;

/// The open "find an action" box.
#[derive(Default)]
pub(crate) struct Finder {
    /// What the user has typed.
    query: String,
    /// Which result Enter would run.
    highlighted: usize,
    /// The text field has been given the keyboard once.
    focused: bool,
}

/// Something the finder can do.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Action {
    AddText,
    AddPicture,
    AddShape(kn_model::ShapeKind),
    NewSlide(kn_layout::Layout),
    DuplicateSlide,
    DeleteSlide,
    GoToSlide(usize),
    DuplicateItem,
    DeleteItem,
    Look(&'static str),
    Shape(Aspect),
    View(settings::Layout),
    Play,
    Present,
    Export(export::Output),
    Undo,
    Redo,
    Save,
    SaveAs,
    Open,
    New,
    Import,
    Quick,
    Shortcuts,
    About,
}

/// One row of results.
struct Entry {
    icon: Icon,
    label: String,
    /// Its keyboard shortcut, if it has one.
    hint: Option<&'static str>,
    action: Action,
}

/// The shortcuts shown in the shortcuts window, by group.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    ("Video", &[
        ("Play or stop", "Space"),
        ("Present full screen", "F5"),
        ("Previous / next slide", "Page Up / Page Down"),
        ("Find an action", "Ctrl+K"),
    ]),
    ("Selected item", &[
        ("Move", "Arrow keys"),
        ("Move further", "Shift+Arrow keys"),
        ("Move without snapping", "Hold Alt while dragging"),
        ("Edit the words", "Enter or double-click"),
        ("Duplicate", "Ctrl+D"),
        ("Copy / cut / paste", "Ctrl+C / X / V"),
        ("Delete", "Delete"),
        ("Deselect", "Escape"),
    ]),
    ("File", &[
        ("Undo / redo", "Ctrl+Z / Ctrl+Shift+Z"),
        ("Save", "Ctrl+S"),
        ("Open", "Ctrl+O"),
        ("New video", "Ctrl+N"),
        ("Keyboard shortcuts", "F1"),
    ]),
];

/// Whether every word of `query` appears in `label`, ignoring case.
fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| label.contains(word))
}

impl KinettoApp {
    /// Opens the finder with an empty box.
    pub(crate) fn open_finder(&mut self) {
        self.stop_playback();
        self.finder = Some(Finder::default());
    }

    /// Everything that can be done right now, most used first.
    fn finder_entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut add = |icon, label: String, hint, action| out.push(Entry { icon, label: tr(&label).to_owned(), hint, action });
        if !self.choosing_template {
            add(Icon::Text, "Add text".into(), None, Action::AddText);
            add(Icon::Picture, "Add picture…".into(), None, Action::AddPicture);
            add(Icon::Play, if self.playback.playing { "Stop".into() } else { "Play from this slide".into() }, Some("Space"), Action::Play);
            add(Icon::Video, "Export video (MP4)…".into(), None, Action::Export(export::Output::Video));
            add(Icon::Present, "Present full screen".into(), Some("F5"), Action::Present);
            if self.selected.is_some() {
                add(Icon::Copy, "Duplicate selected item".into(), Some("Ctrl+D"), Action::DuplicateItem);
                add(Icon::Delete, "Delete selected item".into(), Some("Del"), Action::DeleteItem);
            }
            for layout in kn_layout::Layout::ALL {
                add(Icon::Add, crate::trf!("New slide: {name}", name = tr(layout.name())), None, Action::NewSlide(layout));
            }
            add(Icon::Copy, "Duplicate this slide".into(), None, Action::DuplicateSlide);
            if self.project().slides.len() > 1 {
                add(Icon::Delete, "Delete this slide".into(), None, Action::DeleteSlide);
            }
            if self.session.undo_label().is_some() {
                add(Icon::Undo, "Undo".into(), Some("Ctrl+Z"), Action::Undo);
            }
            if self.session.redo_label().is_some() {
                add(Icon::Redo, "Redo".into(), Some("Ctrl+Shift+Z"), Action::Redo);
            }
            for look in kn_layout::THEMES.iter() {
                add(Icon::Shape, crate::trf!("Look: {name}", name = tr(look.name)), None, Action::Look(look.name));
            }
            for (aspect, name) in [(Aspect::Wide16x9, "Wide (16:9)"), (Aspect::Tall9x16, "Tall (9:16)"), (Aspect::Square1x1, "Square (1:1)")] {
                if aspect != self.project().aspect {
                    add(Icon::Maximize, crate::trf!("Video shape: {name}", name = tr(name)), None, Action::Shape(aspect));
                }
            }
            for (layout, name) in [(settings::Layout::Simple, "Slides"), (settings::Layout::Timeline, "Timeline")] {
                if layout != self.settings.layout {
                    add(Icon::Transition, crate::trf!("Show the {name} view", name = tr(name)), None, Action::View(layout));
                }
            }
            for kind in kn_model::ShapeKind::ALL {
                add(Icon::Shape, crate::trf!("Add shape: {name}", name = tr(kind.name())), None, Action::AddShape(kind));
            }
            for index in 0..self.project().slides.len() {
                if index != self.slide {
                    add(Icon::Next, crate::trf!("Go to slide {n}", n = index + 1), None, Action::GoToSlide(index));
                }
            }
            add(Icon::Picture, "Export as GIF…".into(), None, Action::Export(export::Output::Gif));
            add(Icon::Text, "Export slides as PDF…".into(), None, Action::Export(export::Output::Pdf));
            add(Icon::Save, "Save".into(), Some("Ctrl+S"), Action::Save);
            add(Icon::Save, "Save as…".into(), None, Action::SaveAs);
        }
        add(Icon::Add, "New video".into(), Some("Ctrl+N"), Action::New);
        add(Icon::Open, "Open project…".into(), Some("Ctrl+O"), Action::Open);
        add(Icon::Copy, "Import PowerPoint or Markdown…".into(), None, Action::Import);
        add(Icon::Video, "Quick video…".into(), None, Action::Quick);
        add(Icon::Keyboard, "Keyboard shortcuts".into(), Some("F1"), Action::Shortcuts);
        add(Icon::Info, "About Kinetto".into(), None, Action::About);
        out
    }

    /// Does what a finder row says.
    fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::AddText => self.add_text(),
            Action::AddPicture => self.pick_pictures(),
            Action::AddShape(kind) => self.add_shape(kind),
            Action::NewSlide(layout) => {
                self.run(Command::AddLayoutSlide { at: self.slide + 1, layout });
                self.session.seal();
                self.select_slide(self.slide + 1);
            }
            Action::DuplicateSlide => {
                self.run(Command::DuplicateSlide { index: self.slide });
                self.session.seal();
                self.select_slide(self.slide + 1);
            }
            Action::DeleteSlide => {
                let index = self.slide;
                self.run(Command::DeleteSlide { index });
                self.session.seal();
                self.select_slide(index.saturating_sub(1));
                self.notify_undo(crate::trf!("Slide {n} deleted", n = index + 1));
            }
            Action::GoToSlide(index) => self.select_slide(index.min(self.project().slides.len() - 1)),
            Action::DuplicateItem => self.duplicate_selected(),
            Action::DeleteItem => {
                if let Some(index) = self.selected {
                    self.delete_selected(index);
                }
            }
            Action::Look(name) => {
                self.run(Command::ApplySlideTheme { index: self.slide, theme: name.into() });
                self.session.seal();
            }
            Action::Shape(aspect) => {
                self.run(Command::SetAspect { aspect });
                self.session.seal();
            }
            Action::View(layout) => self.set_layout(layout),
            Action::Play => self.toggle_play(),
            Action::Present => self.start_presenting(ctx),
            Action::Export(output) => self.open_export(output),
            Action::Undo => {
                self.session.undo();
                self.clamp_selection();
            }
            Action::Redo => {
                self.session.redo();
                self.clamp_selection();
            }
            Action::Save => {
                self.save();
            }
            Action::SaveAs => {
                self.save_as();
            }
            Action::Open => self.pick_open(),
            Action::New => self.request(Pending::New),
            Action::Import => self.pick_import(),
            Action::Quick => self.open_quick(),
            Action::Shortcuts => self.shortcuts_open = true,
            Action::About => self.about = true,
        }
    }

    /// The finder, while it is open: type to narrow, arrows to choose,
    /// Enter to run, Escape to close.
    pub(crate) fn finder_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut finder) = self.finder.take() else { return };
        let p = palette(ctx);
        let entries = self.finder_entries();
        let shown: Vec<&Entry> = entries.iter().filter(|e| matches(&e.label, &finder.query)).collect();
        // Keys are read before the text field can take them.
        let (up, down, enter) = ctx.input_mut(|i| {
            (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter))
        });
        let count = shown.len();
        if count > 0 {
            if down { finder.highlighted = (finder.highlighted + 1) % count; }
            if up { finder.highlighted = (finder.highlighted + count - 1) % count; }
        }
        finder.highlighted = finder.highlighted.min(count.saturating_sub(1));
        let mut chosen = (enter && count > 0).then(|| shown[finder.highlighted].action);

        let frame = egui::Frame::new()
            .fill(ctx.style_of(ctx.theme()).visuals.window_fill)
            .corner_radius(DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(12))
            .shadow(ctx.style_of(ctx.theme()).visuals.window_shadow);
        let width = (ctx.content_rect().width() - 48.0).min(480.0);
        let modal = egui::Modal::new(egui::Id::unique("finder")).frame(frame).show(ctx, |ui| crate::adw::reading_order(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 6.0;
            let field = ui.add(
                egui::TextEdit::singleline(&mut finder.query).align(crate::adw::text_align(false))
                    .hint_text(tr("What do you want to do?"))
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(12, 9))
                    .font(egui::FontId::proportional(15.0)),
            );
            field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find an action"));
            if !finder.focused || !field.has_focus() {
                field.request_focus();
                finder.focused = true;
            }
            if field.changed() {
                finder.highlighted = 0;
            }
            if shown.is_empty() {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(tr("Nothing matches. Try another word, like “slide” or “export”.")).color(p.dim)));
                ui.add_space(10.0);
                return;
            }
            egui::ScrollArea::vertical().max_height(ROW * VISIBLE_ROWS).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (n, entry) in shown.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuItem, true, &entry.label));
                    let highlighted = n == finder.highlighted;
                    if highlighted {
                        ui.painter().rect_filled(rect, theme::BUTTON_RADIUS, p.accent_bg.gamma_multiply(0.16));
                        if up || down { response.scroll_to_me(None); }
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, theme::BUTTON_RADIUS, p.button[0]);
                    }
                    icons::paint(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0)), entry.icon, p.fg);
                    ui.painter().text(egui::pos2(rect.left() + 40.0, rect.center().y), egui::Align2::LEFT_CENTER, &entry.label, egui::FontId::proportional(14.0), p.fg);
                    if let Some(hint) = entry.hint {
                        ui.painter().text(egui::pos2(rect.right() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, hint, egui::FontId::proportional(12.5), p.dim);
                    }
                    if response.clicked() {
                        chosen = Some(entry.action);
                    }
                }
            });
        }));
        if let Some(action) = chosen {
            self.run_action(ctx, action);
        } else if !modal.should_close() {
            self.finder = Some(finder);
        }
    }

    /// The keyboard shortcuts window, while it is open.
    pub(crate) fn shortcuts_dialog(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let p = palette(ctx);
        let frame = egui::Frame::new()
            .fill(ctx.style_of(ctx.theme()).visuals.window_fill)
            .corner_radius(DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(24))
            .shadow(ctx.style_of(ctx.theme()).visuals.window_shadow);
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("shortcuts")).frame(frame).show(ctx, |ui| crate::adw::reading_order(ui, |ui| {
            ui.set_width((ctx.content_rect().width() - 96.0).min(420.0));
            ui.label(RichText::new(tr("Keyboard shortcuts")).family(theme::bold()).size(19.0));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 220.0).max(160.0)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (group, rows) in SHORTCUTS {
                    ui.add_space(10.0);
                    ui.label(RichText::new(tr(group)).family(theme::semibold()).size(12.5).color(p.dim));
                    for (what, keys) in *rows {
                        ui.horizontal(|ui| {
                            ui.set_min_height(28.0);
                            ui.label(tr(what));
                            ui.with_layout(adw::trailing(), |ui| {
                                // Clear of the scroll bar.
                                ui.add_space(16.0);
                                ui.label(RichText::new(tr(keys)).color(p.dim));
                            });
                        });
                    }
                }
            });
            ui.add_space(16.0);
            close = adw::wide_button(ui, "Close", Kind::Normal, ui.available_width()).clicked();
        }));
        if close || modal.should_close() {
            self.shortcuts_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_typed_word_must_appear_in_any_order_and_case() {
        assert!(matches("New slide: Title", "slide new"));
        assert!(matches("Export as GIF…", "gif"));
        assert!(matches("Anything", ""));
        assert!(!matches("Add text", "slide"));
    }
}
