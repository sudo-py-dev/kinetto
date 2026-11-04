//! "Make a video in seconds": words and pictures in, with the look, the
//! shape and how everything moves chosen up front, then one click.

use crate::adw::{self, Kind};
use crate::icons::Icon;
use crate::theme::{self, palette};
use crate::{KinettoApp, Pending, PICTURE_EXTENSIONS};
use egui::{Color32, RichText, Sense, Stroke, Vec2};
use kn_geom::Aspect;
use kn_import::{Dice, Part, Pick, Style};
use kn_layout::THEMES;
use kn_media::{MediaInfo, MediaLibrary};
use kn_model::{Motion, Transition};
use serde::{Deserialize, Serialize};
use std::path::Path;

const THUMB: f32 = 72.0;
/// Width of each of the dialog's two columns, and the gap between them.
const COLUMN: f32 = 420.0;
const GUTTER: f32 = 28.0;

/// What was chosen last time, so the next quick video starts the same way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickChoices {
    /// A look by name; `None` picks one at random.
    pub look: Option<String>,
    pub aspect: Aspect,
    pub style: Style,
}

impl Default for QuickChoices {
    fn default() -> Self {
        Self { look: Some(THEMES[0].name.into()), aspect: Aspect::Wide16x9, style: Style::default() }
    }
}

impl QuickChoices {
    /// Something will come out differently if it is made again.
    pub fn has_chance(&self) -> bool {
        self.look.is_none() || self.style.motion == Pick::Random || self.style.transition == Pick::Random || self.style.exit == Pick::Random
    }
}

/// The dialog while it is open.
pub(crate) struct Quick {
    text: String,
    media: MediaLibrary,
    pictures: Vec<(MediaInfo, egui::TextureHandle)>,
    /// Files that couldn't be used, in plain words.
    notes: Vec<String>,
}

/// Everything needed to make (or remake) a quick video.
#[derive(Clone)]
pub(crate) struct QuickRequest {
    text: String,
    media: MediaLibrary,
    pictures: Vec<MediaInfo>,
    choices: QuickChoices,
}

impl Quick {
    pub fn new(text: String) -> Self {
        Self { text, media: MediaLibrary::new(), pictures: Vec::new(), notes: Vec::new() }
    }

    fn ready(&self) -> bool {
        !self.text.trim().is_empty() || !self.pictures.is_empty()
    }

    /// Adds a picture file, or its text if it is a text file.
    pub fn add_file(&mut self, ctx: &egui::Context, path: &Path) {
        let name = path.file_name().map_or_else(|| "a file".to_owned(), |n| format!("\u{201c}{}\u{201d}", n.to_string_lossy()));
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        if std::fs::metadata(path).is_ok_and(|m| m.len() > kn_media::MAX_FILE_BYTES as u64) {
            return self.notes.push(format!("{name} is bigger than 100 MB."));
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => return self.notes.push(format!("{name} couldn't be read. {}", crate::problem::io_reason(&err))),
        };
        if matches!(ext.as_str(), "txt" | "md" | "markdown") {
            if !self.text.trim().is_empty() {
                self.text.push_str("\n\n");
            }
            self.text.push_str(String::from_utf8_lossy(&bytes).trim());
        } else {
            self.add_picture(ctx, bytes, &name);
        }
    }

    pub fn add_picture(&mut self, ctx: &egui::Context, bytes: Vec<u8>, name: &str) {
        let info = match self.media.import(bytes) {
            Ok(info) => info,
            Err(err) => return self.notes.push(format!("{name} was left out: {err}.")),
        };
        let thumb = match self.media.thumbnail(&info.id, (THUMB * 2.0) as u32) {
            Ok(t) => t,
            Err(err) => return self.notes.push(format!("{name} was left out: {err}.")),
        };
        let image = egui::ColorImage::from_rgba_unmultiplied([thumb.width as usize, thumb.height as usize], &thumb.rgba);
        let texture = ctx.load_texture(format!("quick-{}", info.id), image, egui::TextureOptions::LINEAR);
        self.pictures.push((info, texture));
    }
}

impl KinettoApp {
    pub(crate) fn open_quick(&mut self) {
        self.quick = Some(Quick::new(String::new()));
    }

    fn pick_quick_pictures(&mut self, ctx: &egui::Context) {
        let files = rfd::FileDialog::new().add_filter("Pictures", PICTURE_EXTENSIONS).pick_files().unwrap_or_default();
        if let Some(quick) = self.quick.as_mut() {
            for path in files {
                quick.add_file(ctx, &path);
            }
        }
    }

    /// The "Make a video in seconds" dialog.
    pub(crate) fn quick_dialog(&mut self, ctx: &egui::Context) {
        if self.quick.is_none() {
            return;
        }
        let p = palette(ctx);
        let style = ctx.style_of(ctx.theme());
        let frame = egui::Frame::new()
            .fill(style.visuals.window_fill)
            .corner_radius(theme::DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(24))
            .shadow(style.visuals.window_shadow);
        let mut choices = self.settings.quick.clone();
        let (mut action, mut add_pictures) = (None, false);
        let max_height = (ctx.content_rect().height() - 180.0).max(240.0);
        egui::Modal::new(egui::Id::unique("quick")).frame(frame).show(ctx, |ui| {
            let Some(quick) = self.quick.as_mut() else { return };
            ui.set_width(2.0 * COLUMN + GUTTER);
            ui.label(RichText::new("Make a video in seconds").family(theme::bold()).size(19.0));
            ui.label(RichText::new("Add words, pictures or both, choose how it looks and moves, and it's made. You can change everything after.").color(p.dim));
            ui.add_space(14.0);
            // What goes in on the left, how it looks and moves on the right,
            // so every choice is in view at once.
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GUTTER;
                    column(ui, COLUMN, |ui| {
                        heading(ui, "Words", "A blank line starts a new slide. The first line is its title; lines starting with “-” become points.");
                        ui.add(
                            egui::TextEdit::multiline(&mut quick.text)
                                .desired_rows(8)
                                .desired_width(f32::INFINITY)
                                .hint_text("My trip to Japan\nTwo weeks, five cities\n\nWhat we saw\n- Temples\n- Mountains\n- Ramen"),
                        );
                        ui.add_space(14.0);
                        heading(ui, "Pictures", "Each gets a slide of its own, after the words. You can also drop them here.");
                        let mut remove = None;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                            for (i, (_, texture)) in quick.pictures.iter().enumerate() {
                                if thumbnail(ui, texture, i).clicked() {
                                    remove = Some(i);
                                }
                            }
                            let label = if quick.pictures.is_empty() { "Add pictures…" } else { "Add more…" };
                            if adw::button(ui, Some(Icon::Picture), label, Kind::Normal).clicked() {
                                add_pictures = true;
                            }
                        });
                        if let Some(i) = remove {
                            drop(quick.pictures.remove(i));
                        }
                        for note in &quick.notes {
                            ui.label(RichText::new(note).size(12.5).color(p.destructive));
                        }
                    });
                    // Room for the scroll bar beside the toggles.
                    column(ui, COLUMN - 12.0, |ui| choose(ui, &mut choices));
                });
            });
            ui.add_space(8.0);
            let shortcut = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = quick.ready();
                let make = ui.add_enabled_ui(ready, |ui| adw::button(ui, Some(Icon::Video), "Make video", Kind::Suggested)).inner;
                let make = make.on_hover_text("Ctrl+Enter").on_disabled_hover_text("Add some words or a picture first");
                if make.clicked() || (ready && shortcut) {
                    action = Some(true);
                }
                if adw::button(ui, None, "Cancel", Kind::Normal).clicked() {
                    action = Some(false);
                }
            });
        });
        if choices != self.settings.quick {
            self.settings.quick = choices;
            self.settings.save();
        }
        if add_pictures {
            self.pick_quick_pictures(ctx);
        }
        match action {
            Some(true) => {
                let Some(quick) = self.quick.take() else { return };
                let pictures = quick.pictures.into_iter().map(|(info, _)| info).collect();
                let request = QuickRequest { text: quick.text, media: quick.media, pictures, choices: self.settings.quick.clone() };
                self.request(Pending::Quick(Box::new(request)));
            }
            Some(false) => self.quick = None,
            None => {}
        }
    }

    /// Makes the video, replacing the open one.
    pub(crate) fn quick_make(&mut self, request: &QuickRequest) {
        let mut media = request.media.clone();
        let mut outline = if request.text.trim().is_empty() { kn_import::Outline::default() } else { kn_import::outline_from_text(&request.text, &mut media) };
        outline.parts.extend(request.pictures.iter().map(|p| Part::Picture { media: p.id.clone(), width: p.width, height: p.height, caption: String::new() }));
        let mut dice = Dice::from_clock();
        let theme = match request.choices.look.as_deref() {
            Some(name) => kn_layout::theme(name),
            None => &THEMES[dice.below(THEMES.len())],
        };
        let again = request.choices.has_chance().then(|| Box::new(request.clone()));
        self.finish_import(outline, media, None, request.choices.aspect, theme, request.choices.style, &mut dice, again);
    }
}

fn column(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        add(ui);
    });
}

fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(RichText::new(title).family(theme::bold()).size(14.0));
    ui.label(RichText::new(description).size(12.5).color(palette(ui.ctx()).dim));
    ui.add_space(4.0);
}

/// The look, the shape and how everything moves.
fn choose(ui: &mut egui::Ui, choices: &mut QuickChoices) {
    // Keep all three animation choices visible at the normal window height.
    ui.spacing_mut().item_spacing.y = 4.0;
    adw::group(ui, "Look", None, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(6.0);
            let surprise = dice_swatch(ui, choices.look.is_none()).on_hover_text("Surprise me: a different look each time");
            if surprise.clicked() {
                choices.look = None;
            }
            for theme in THEMES {
                let selected = choices.look.as_deref() == Some(theme.name);
                if crate::start::swatch(ui, theme, selected).on_hover_text(theme.name).clicked() {
                    choices.look = Some(theme.name.into());
                }
            }
        });
    });
    adw::group(ui, "Shape", None, |ui| {
        adw::toggle_group(ui, &mut choices.aspect, &[(Aspect::Wide16x9, "Wide"), (Aspect::Tall9x16, "Tall"), (Aspect::Square1x1, "Square")]);
    });
    adw::group(ui, "Animation", Some("Random gives every slide its own"), |ui| {
        adw::boxed_list(ui, |list| {
            list.row("Items arrive", None, |ui| {
                let mut all: Vec<_> = Motion::ALL[1..].iter().map(|m| (*m, m.name())).collect();
                all.push((Motion::None, "Still"));
                pick_dropdown(ui, "quick-motion", &mut choices.style.motion, "Auto", &all);
            });
            list.row("Items leave", None, |ui| {
                let all: Vec<_> = Motion::ALL[1..].iter().map(|m| (*m, m.exit_name())).collect();
                pick_dropdown(ui, "quick-exit", &mut choices.style.exit, "They stay", &all);
            });
            list.row("Between slides", None, |ui| {
                let all: Vec<_> = Transition::ALL.iter().map(|t| (*t, t.name())).collect();
                pick_dropdown(ui, "quick-transition", &mut choices.style.transition, "Auto", &all);
            });
        });
    });
}

/// "Auto", "Random", then every single choice.
fn pick_dropdown<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, pick: &mut Pick<T>, auto: &str, all: &[(T, &str)]) {
    let selected = match *pick {
        Pick::Look => auto,
        Pick::Random => "Random",
        Pick::Every(t) => all.iter().find(|(o, _)| *o == t).map_or(auto, |(_, name)| *name),
    };
    adw::dropdown(ui, id, selected, |ui| {
        ui.selectable_value(pick, Pick::Look, auto);
        ui.selectable_value(pick, Pick::Random, "Random");
        ui.separator();
        for (option, name) in all {
            ui.selectable_value(pick, Pick::Every(*option), *name);
        }
    });
}

/// A picture in the dialog; clicking it takes it out.
fn thumbnail(ui: &mut egui::Ui, texture: &egui::TextureHandle, i: usize) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(THUMB), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove picture {}", i + 1)));
    let size = texture.size_vec2();
    let fit = (THUMB / size.x).max(THUMB / size.y);
    let shown = size * fit;
    let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), Vec2::new(THUMB / shown.x, THUMB / shown.y));
    egui::Image::new(texture).uv(uv).corner_radius(8).paint_at(ui, rect);
    ui.painter().rect_stroke(rect, 8, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    if response.hovered() {
        ui.painter().rect_filled(rect, 8, Color32::from_black_alpha(110));
        crate::icons::paint(ui.painter(), rect, Icon::Close, Color32::WHITE);
    }
    response.on_hover_text("Remove").on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The "surprise me" choice beside the looks: a die.
fn dice_swatch(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(62.0, 42.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, "Surprise me"));
    let inner = rect.shrink(3.0);
    ui.painter().rect(inner, 8, p.button[0], Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    let die = egui::Rect::from_center_size(inner.center(), Vec2::splat(24.0));
    ui.painter().rect_stroke(die, 5, Stroke::new(1.6, p.fg), egui::StrokeKind::Inside);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (0.0, 0.0), (-1.0, 1.0), (1.0, 1.0)] {
        ui.painter().circle_filled(die.center() + Vec2::new(x, y) * 6.0, 2.0, p.fg);
    }
    if selected || response.hovered() {
        let color = if selected { p.accent_bg } else { p.border };
        ui.painter().rect_stroke(rect, 10, Stroke::new(2.0, color), egui::StrokeKind::Inside);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
