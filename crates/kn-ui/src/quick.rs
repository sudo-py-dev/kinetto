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
use kn_model::{Mood, Motion, Music, MusicSource, Transition};
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
    pub music: Option<Mood>,
    pub music_volume: f64,
}

impl Default for QuickChoices {
    fn default() -> Self {
        Self { look: Some(THEMES[0].name.into()), aspect: Aspect::Wide16x9, style: Style::default(), music: Some(Mood::Calm), music_volume: 0.22 }
    }
}

impl QuickChoices {
    /// Something will come out differently if it is made again.
    pub fn has_chance(&self) -> bool {
        self.look.is_none() || self.style.motion == Pick::Random || self.style.transition == Pick::Random || self.style.exit == Pick::Random
    }
}

#[derive(Clone, Default)]
struct PhotoGroup {
    title: String,
    collage: bool,
}

#[derive(Clone, Default)]
struct PhotoDetails {
    caption: String,
    group: usize,
}

enum PhotoJob {
    File(std::path::PathBuf, usize, egui::Context),
    Pick(usize, egui::Context),
}

enum PhotoResult {
    Picture { info: MediaInfo, bytes: std::sync::Arc<[u8]>, thumb: kn_media::Decoded, group: usize },
    Text(String),
    Note(String),
    Done,
}

fn load_photo_file(path: &Path, group: usize, results: &std::sync::mpsc::SyncSender<PhotoResult>) -> bool {
    let result = (|| -> Result<PhotoResult, String> {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > kn_media::MAX_FILE_BYTES as u64 {
            return Err(format!("{name} is bigger than 100 MB."));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{name} couldn't be read: {e}"))?;
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
        if matches!(ext.as_str(), "txt" | "md" | "markdown") {
            return Ok(PhotoResult::Text(String::from_utf8_lossy(&bytes).trim().to_owned()));
        }
        let mut media = MediaLibrary::new();
        let info = media.import(bytes).map_err(|e| format!("{name} was left out: {e}"))?;
        let thumb = media.thumbnail(&info.id, (THUMB * 2.0) as u32).map_err(|e| format!("{name} was left out: {e}"))?;
        let bytes = media.get(&info.id).unwrap().clone();
        Ok(PhotoResult::Picture { info, bytes, thumb, group })
    })();
    results.send(result.unwrap_or_else(PhotoResult::Note)).is_ok()
}

fn photo_loader() -> (std::sync::mpsc::Sender<PhotoJob>, std::sync::mpsc::Receiver<PhotoResult>) {
    let (jobs, work) = std::sync::mpsc::channel();
    let (results, loaded) = std::sync::mpsc::sync_channel(2);
    std::thread::spawn(move || {
        while let Ok(job) = work.recv() {
            let (paths, group, ctx) = match job {
                PhotoJob::File(path, group, ctx) => (vec![path], group, ctx),
                PhotoJob::Pick(group, ctx) => (rfd::FileDialog::new().add_filter("Pictures", PICTURE_EXTENSIONS).pick_files().unwrap_or_default(), group, ctx),
            };
            for path in paths {
                if !load_photo_file(&path, group, &results) { return; }
                ctx.request_repaint();
            }
            if results.send(PhotoResult::Done).is_err() { return; }
            ctx.request_repaint();
        }
    });
    (jobs, loaded)
}

/// The dialog while it is open.
pub(crate) struct Quick {
    text: String,
    media: MediaLibrary,
    pictures: Vec<(MediaInfo, egui::TextureHandle)>,
    /// Files that couldn't be used, in plain words.
    notes: Vec<String>,
    sample_thumbs: Vec<egui::TextureHandle>,
    groups: Vec<PhotoGroup>,
    details: Vec<PhotoDetails>,
    active_group: usize,
    jobs: std::sync::mpsc::Sender<PhotoJob>,
    loaded: std::sync::mpsc::Receiver<PhotoResult>,
    loading: usize,
}

/// Everything needed to make (or remake) a quick video.
#[derive(Clone)]
pub(crate) struct QuickRequest {
    text: String,
    media: MediaLibrary,
    pictures: Vec<MediaInfo>,
    groups: Vec<PhotoGroup>,
    details: Vec<PhotoDetails>,
    choices: QuickChoices,
}

impl Quick {
    pub fn new(text: String) -> Self {
        let (jobs, loaded) = photo_loader();
        Self { text, media: MediaLibrary::new(), pictures: Vec::new(), notes: Vec::new(), sample_thumbs: Vec::new(), groups: vec![PhotoGroup::default()], details: Vec::new(), active_group: 0, jobs, loaded, loading: 0 }
    }

    fn load_samples(&mut self, ctx: &egui::Context) {
        if !self.sample_thumbs.is_empty() { return; }
        let media = kn_media::sample_library();
        for (i, (name, _)) in kn_media::SAMPLE_ART.iter().enumerate() {
            if let Ok(image) = media.thumbnail(&kn_media::sample_id(i), 224) {
                let image = egui::ColorImage::from_rgba_unmultiplied([image.width as usize,image.height as usize],&image.rgba);
                self.sample_thumbs.push(ctx.load_texture(*name,image,egui::TextureOptions::LINEAR));
            }
        }
    }

    fn ready(&self) -> bool {
        self.loading == 0 && (!self.text.trim().is_empty() || !self.pictures.is_empty())
    }

    /// Adds a picture file, or its text if it is a text file.
    pub fn add_file(&mut self, ctx: &egui::Context, path: &Path) {
        if self.jobs.send(PhotoJob::File(path.to_owned(), self.active_group, ctx.clone())).is_ok() { self.loading += 1; }
    }

    fn poll_photos(&mut self, ctx: &egui::Context) {
        // Keep a frame bounded even when many tiny files finish together.
        for _ in 0..4 {
            let Ok(result) = self.loaded.try_recv() else { break };
            match result {
                PhotoResult::Picture { info, bytes, thumb, group } => {
                    self.media.insert_shared(info.id.clone(), bytes);
                    let image = egui::ColorImage::from_rgba_unmultiplied([thumb.width as usize, thumb.height as usize], &thumb.rgba);
                    let texture = ctx.load_texture(format!("quick-{}", info.id), image, egui::TextureOptions::LINEAR);
                    self.pictures.push((info, texture));
                    self.details.push(PhotoDetails { group: group.min(self.groups.len() - 1), ..Default::default() });
                }
                PhotoResult::Text(text) => {
                    if !self.text.trim().is_empty() { self.text.push_str("\n\n"); }
                    self.text.push_str(&text);
                }
                PhotoResult::Note(note) => if !note.is_empty() { self.notes.push(note); },
                PhotoResult::Done => self.loading = self.loading.saturating_sub(1),
            }
        }
        if self.loading > 0 { ctx.request_repaint_after(std::time::Duration::from_millis(30)); }
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
        self.details.push(PhotoDetails { group: self.active_group, ..Default::default() });
    }
}

impl KinettoApp {
    pub(crate) fn open_quick(&mut self) {
        self.quick = Some(Quick::new(String::new()));
    }

    pub(crate) fn open_quick_sample(&mut self, ctx: &egui::Context) {
        let mut quick = Quick::new("Take the scenic route\nA few moments worth keeping\n\nA fresh perspective\n- Slow down\n- Look closer\n- Keep exploring".into());
        quick.add_picture(ctx, kn_media::SAMPLE_ART[0].1.to_vec(), kn_media::SAMPLE_ART[0].0);
        quick.groups[0].title = "Moments outdoors".into();
        quick.groups[0].collage = true;
        quick.details[0].caption = "A quieter kind of morning".into();
        self.quick = Some(quick);
    }

    fn pick_quick_pictures(&mut self, ctx: &egui::Context) {
        if let Some(quick) = self.quick.as_mut()
            && quick.jobs.send(PhotoJob::Pick(quick.active_group, ctx.clone())).is_ok() {
            quick.loading += 1;
        }
    }

    /// The "Make a video in seconds" dialog.
    pub(crate) fn quick_dialog(&mut self, ctx: &egui::Context) {
        if self.quick.is_none() {
            return;
        }
        if let Some(quick) = self.quick.as_mut() { quick.poll_photos(ctx); }
        let p = palette(ctx);
        let style = ctx.style_of(ctx.theme());
        let frame = egui::Frame::new()
            .fill(style.visuals.window_fill)
            .corner_radius(theme::DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(24))
            .shadow(style.visuals.window_shadow);
        let mut choices = self.settings.quick.clone();
        let (mut action, mut add_pictures) = (None, false);
        let max_height = (ctx.content_rect().height() - 220.0).max(200.0);
        egui::Modal::new(egui::Id::unique("quick")).frame(frame).show(ctx, |ui| {
            let Some(quick) = self.quick.as_mut() else { return };
            let width = (ctx.content_rect().width() - 80.0).clamp(280.0, 2.0 * COLUMN + GUTTER);
            ui.set_width(width);
            ui.set_max_height((ctx.content_rect().height() - 96.0).max(300.0));
            quick.load_samples(ctx);
            ui.label(RichText::new("Create a quick video").family(theme::bold()).size(22.0));
            ui.label(RichText::new("Your content, a considered look, and a little movement. Everything stays editable.").color(p.dim));
            ui.add_space(14.0);
            egui::ScrollArea::vertical().max_height(max_height).show(ui, |ui| {
                if width >= 780.0 {
                    let column_width = (width - GUTTER) / 2.0;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = GUTTER;
                        column(ui, column_width, |ui| content(ui, quick, &mut add_pictures));
                        column(ui, column_width - 12.0, |ui| choose(ui, &mut choices));
                    });
                } else {
                    content(ui, quick, &mut add_pictures);
                    ui.add_space(16.0);
                    choose(ui, &mut choices);
                }
            });
            ui.add_space(8.0);
            let shortcut = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
            if quick.loading > 0 {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Loading photos… {} ready", quick.pictures.len()));
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = quick.ready();
                let make = ui.add_enabled_ui(ready, |ui| adw::button(ui, Some(Icon::Video), "Create video", Kind::Suggested)).inner;
                let make = make.on_hover_text("Ctrl+Enter").on_disabled_hover_text(if quick.loading > 0 { "Wait for the photos to finish loading" } else { "Add some words or a picture first" });
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
                let request = QuickRequest { text: quick.text, media: quick.media, pictures, groups: quick.groups, details: quick.details, choices: self.settings.quick.clone() };
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
        append_photo_groups(&mut outline, &request.pictures, &request.groups, &request.details);
        let mut dice = Dice::from_clock();
        let theme = match request.choices.look.as_deref() {
            Some(name) => kn_layout::theme(name),
            None => &THEMES[dice.below(THEMES.len())],
        };
        let again = request.choices.has_chance().then(|| Box::new(request.clone()));
        let music = request.choices.music.map(|mood| Music::new(MusicSource::Mood(mood), request.choices.music_volume.clamp(0.0, 1.0) as f32));
        self.finish_import(outline, media, None, request.choices.aspect, theme, request.choices.style, &mut dice, music, again);
    }
}

fn append_photo_groups(outline: &mut kn_import::Outline, pictures: &[MediaInfo], groups: &[PhotoGroup], details: &[PhotoDetails]) {
    for (index, group) in groups.iter().enumerate() {
        let photos: Vec<_> = pictures.iter().zip(details).filter(|(_, d)| d.group == index).map(|(p, d)| kn_import::Photo {
            media: p.id.clone(), width: p.width, height: p.height, caption: d.caption.trim().to_owned(),
        }).collect();
        if photos.is_empty() { continue; }
        if group.collage {
            outline.parts.push(Part::PhotoGroup { title: group.title.trim().to_owned(), photos });
        } else {
            if !group.title.trim().is_empty() {
                outline.parts.push(Part::Title { title: group.title.trim().to_owned(), subtitle: String::new() });
            }
            outline.parts.extend(photos.into_iter().map(|p| Part::Picture { media: p.media, width: p.width, height: p.height, caption: p.caption }));
        }
    }
}

fn content(ui: &mut egui::Ui, quick: &mut Quick, add_pictures: &mut bool) {
    let p = palette(ui.ctx());
    heading(ui, "1. Your story", "Text is optional. Blank lines separate slides; add photo groups below.");
    ui.add(egui::TextEdit::multiline(&mut quick.text).desired_rows(4).desired_width(f32::INFINITY)
        .hint_text("Give your story a title\nAdd a short introduction\n\nYour next slide\n- One clear idea\n- Another good thought"));
    ui.add_space(14.0);
    heading(ui, "Photo groups", "Give a collection a title, or caption a single photo.");
    let mut remove = None;
    let mut delete_group = None;
    for group_index in 0..quick.groups.len() {
        ui.push_id(("photo-group", group_index), |ui| {
            egui::Frame::new().fill(p.card).corner_radius(10).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Group {}", group_index + 1)).family(theme::semibold()));
                    if quick.loading == 0 && quick.groups.len() > 1 && adw::icon_button(ui, Icon::Delete, "Remove group").on_hover_text("Move its photos to the previous group").clicked() { delete_group = Some(group_index); }
                });
                ui.add(egui::TextEdit::singleline(&mut quick.groups[group_index].title).desired_width(f32::INFINITY).hint_text("Group title (optional)"));
                adw::dropdown(ui, "group-layout", if quick.groups[group_index].collage { "Collage" } else { "One photo per slide" }, |ui| {
                    ui.selectable_value(&mut quick.groups[group_index].collage, false, "One photo per slide");
                    ui.selectable_value(&mut quick.groups[group_index].collage, true, "Collage");
                });
                let count = quick.details.iter().filter(|d| d.group == group_index).count();
                if count == 0 { ui.label(RichText::new("Add photos to start this group").size(12.0).color(p.dim)); }
                for i in 0..quick.pictures.len() {
                    if quick.details[i].group != group_index { continue; }
                    ui.push_id(("photo", i), |ui| {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let (_, texture) = &quick.pictures[i];
                            if thumbnail(ui, texture, i).clicked() { remove = Some(i); }
                            ui.vertical(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut quick.details[i].caption).desired_width(ui.available_width()).hint_text("Photo title or caption"));
                                ui.horizontal_wrapped(|ui| {
                                    if i > 0 && quick.details[i - 1].group == group_index && adw::icon_button(ui, Icon::Up, "Move photo earlier").clicked() {
                                        quick.pictures.swap(i, i - 1); quick.details.swap(i, i - 1);
                                    }
                                    if quick.groups.len() > 1 {
                                        adw::dropdown(ui, "move-group", "Move to group", |ui| {
                                            for (target, group) in quick.groups.iter().enumerate() {
                                                let label = if group.title.trim().is_empty() { format!("Group {}", target + 1) } else { group.title.clone() };
                                                ui.selectable_value(&mut quick.details[i].group, target, label);
                                            }
                                        });
                                    }
                                });
                            });
                        });
                    });
                }
                ui.add_space(6.0);
                if adw::button(ui, Some(Icon::Picture), "Add pictures…", Kind::Normal).clicked() {
                    quick.active_group = group_index;
                    *add_pictures = true;
                }
            });
        });
        ui.add_space(8.0);
    }
    if let Some(i) = remove { drop(quick.pictures.remove(i)); quick.details.remove(i); }
    if let Some(i) = delete_group {
        quick.groups.remove(i);
        for detail in &mut quick.details {
            if detail.group == i { detail.group = i.saturating_sub(1); }
            else if detail.group > i { detail.group -= 1; }
        }
        quick.active_group = if quick.active_group >= i { quick.active_group.saturating_sub(1) } else { quick.active_group };
    }
    if adw::button(ui, Some(Icon::Add), "Add photo group", Kind::Normal).clicked() {
        quick.groups.push(PhotoGroup::default());
        quick.active_group = quick.groups.len() - 1;
    }
    if quick.groups.len() > 1 {
        ui.label(RichText::new(format!("Sample artwork goes to Group {}", quick.active_group + 1)).size(12.0).color(p.dim));
    }
    ui.add_space(12.0);
    ui.label(RichText::new("Free sample artwork").family(theme::semibold()).size(12.0));
    ui.label(RichText::new("Included with Kinetto. Ready to use in your videos.").color(p.dim).size(12.0));
    let mut add = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        for (i, texture) in quick.sample_thumbs.iter().enumerate() {
            let (name, _) = kn_media::SAMPLE_ART[i];
            let added = quick.pictures.iter().any(|(info,_)| info.id == kn_media::sample_id(i));
            let response = ui.add_enabled(!added,egui::Button::image(egui::Image::new((texture.id(),Vec2::new(102.0,76.5))).corner_radius(6)).selected(added));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button,!added,format!("Add sample: {name}")));
            if response.on_hover_text(format!("Add {name}")).on_disabled_hover_text(format!("{name} is already added")).clicked() { add = Some(i); }
        }
    });
    if let Some(i) = add {
        let (name,bytes) = kn_media::SAMPLE_ART[i];
        if !quick.pictures.iter().any(|(info,_)| info.id == kn_media::sample_id(i)) { quick.add_picture(ui.ctx(),bytes.to_vec(),name); }
    }
    for note in &quick.notes { ui.label(RichText::new(note).size(12.5).color(p.destructive)); }
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
    adw::group(ui, "2. Make it yours", Some("Colour, format and movement"), |ui| {
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
    adw::group(ui, "Format", None, |ui| {
        adw::toggle_group(ui, &mut choices.aspect, &[(Aspect::Wide16x9, "Wide"), (Aspect::Tall9x16, "Tall"), (Aspect::Square1x1, "Square")]);
    });
    adw::group(ui, "Animation", Some("Random gives every slide its own"), |ui| {
        adw::boxed_list(ui, |list| {
            list.row("Items arrive", None, |ui| {
                pick_motion(ui, "quick-motion", &mut choices.style.motion, false);
            });
            list.row("Items leave", None, |ui| {
                pick_motion(ui, "quick-exit", &mut choices.style.exit, true);
            });
            list.row("Between slides", None, |ui| {
                let all: Vec<_> = Transition::ALL.iter().map(|t| (*t, t.name())).collect();
                pick_dropdown(ui, "quick-transition", &mut choices.style.transition, "Auto", &all);
            });
        });
    });
    adw::group(ui, "Soundtrack", Some("Original music, included and free to use"), |ui| {
        adw::boxed_list(ui, |list| {
            list.row("Music", None, |ui| {
                adw::dropdown(ui, "quick-music", choices.music.map_or("None", |m| m.name()), |ui| {
                    ui.selectable_value(&mut choices.music, None, "None");
                    for mood in Mood::ALL { ui.selectable_value(&mut choices.music, Some(mood), mood.name()).on_hover_text(mood.description()); }
                });
            });
            if choices.music.is_some() {
                let percent = format!("{:.0}%",choices.music_volume*100.0);
                list.row("Volume", Some(&percent), |ui| { adw::slider(ui, &mut choices.music_volume, 0.0..=1.0, 0.05, "Music volume"); });
            }
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

fn pick_motion(ui: &mut egui::Ui, id: &str, pick: &mut Pick<Motion>, exit: bool) {
    let auto = if exit { "They stay" } else { "Auto" };
    let selected = match pick { Pick::Look => auto, Pick::Random => "Random", Pick::Every(m) => if exit { m.exit_name() } else { m.name() } };
    adw::dropdown(ui, id, selected, |ui| {
        ui.selectable_value(pick, Pick::Look, auto);
        ui.selectable_value(pick, Pick::Random, "Random");
        ui.separator();
        for category in ["Simple", "Letter by letter", "Slide", "Reveal", "Expressive"] {
            ui.menu_button(category, |ui| {
                for motion in Motion::ALL.into_iter().filter(|m| m.category() == category) {
                    let label = if exit { motion.exit_name() } else { motion.name() };
                    if ui.selectable_value(pick, Pick::Every(motion), label).clicked() { ui.close(); }
                }
            });
        }
    });
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn background_photo_loader_preserves_group_and_recovers_from_bad_files() {
        let path = std::env::temp_dir().join(format!("kinetto-photo-loader-{}.png", std::process::id()));
        std::fs::write(&path, kn_media::test_png(240, 160)).unwrap();
        let (jobs, results) = photo_loader();
        jobs.send(PhotoJob::File(path.clone(), 2, egui::Context::default())).unwrap();
        let result = results.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        match result {
            PhotoResult::Picture { info, thumb, group, .. } => {
                assert_eq!(group, 2);
                assert_eq!((info.width, info.height), (240, 160));
                assert!(thumb.width <= 144 && thumb.height <= 144);
            }
            _ => panic!("expected a loaded photo"),
        }
        assert!(matches!(results.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), PhotoResult::Done));
        std::fs::remove_file(&path).unwrap();
        jobs.send(PhotoJob::File(path, 0, egui::Context::default())).unwrap();
        assert!(matches!(results.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), PhotoResult::Note(_)));
        assert!(matches!(results.recv_timeout(std::time::Duration::from_secs(5)).unwrap(), PhotoResult::Done));
        let mut quick = Quick::new("A story".into());
        assert!(quick.ready());
        quick.loading = 1;
        assert!(!quick.ready());
    }

    #[test]
    fn photo_groups_keep_captions_order_and_skip_empty_groups() {
        let library = kn_media::sample_library();
        let pictures: Vec<_> = (0..3).map(|i| {
            let image = library.decode(&kn_media::sample_id(i)).unwrap();
            MediaInfo { id: kn_media::sample_id(i), width: image.width, height: image.height }
        }).collect();
        let groups = vec![
            PhotoGroup { title: "Trip".into(), collage: false },
            PhotoGroup { title: "Details".into(), collage: true },
            PhotoGroup { title: "Empty".into(), collage: false },
        ];
        let details = vec![
            PhotoDetails { caption: "First morning".into(), group: 0 },
            PhotoDetails { caption: "Flowers".into(), group: 1 },
            PhotoDetails { caption: "Sunset".into(), group: 0 },
        ];
        let mut outline = kn_import::Outline::default();
        append_photo_groups(&mut outline, &pictures, &groups, &details);
        assert_eq!(outline.parts.len(), 4);
        assert!(matches!(&outline.parts[0], Part::Title { title, .. } if title == "Trip"));
        assert!(matches!(&outline.parts[1], Part::Picture { caption, .. } if caption == "First morning"));
        assert!(matches!(&outline.parts[2], Part::Picture { caption, .. } if caption == "Sunset"));
        assert!(matches!(&outline.parts[3], Part::PhotoGroup { title, photos } if title == "Details" && photos[0].caption == "Flowers"));
    }
}
