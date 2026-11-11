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

/// Size of picture thumbnails in the dialog, in points.
const THUMB: f32 = 72.0;
/// Comfortable reading width for the single-column wizard.
const WIZARD_WIDTH: f32 = 600.0;

/// What was chosen last time, so the next quick video starts the same way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickChoices {
    /// A look by name; `None` picks one at random.
    pub look: Option<String>,
    /// Wide, tall or square.
    pub aspect: Aspect,
    /// How items arrive and leave, and how slides change.
    pub style: Style,
    /// The generated music's mood; `None` for no music.
    pub music: Option<Mood>,
    /// Music volume, 0 to 1.
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
        self.look.is_none() || self.style.motion == Pick::Random || self.style.transition == Pick::Random || self.style.exit == Pick::Random || self.style.photo_motion == Pick::Random
    }
}

/// A set of pictures that become slides together.
#[derive(Clone, Default)]
struct PhotoGroup {
    /// Title for the group's slide; empty for none.
    title: String,
    /// Show the pictures together on one slide instead of one per slide.
    collage: bool,
}

/// What the user typed about one picture.
#[derive(Clone, Default)]
struct PhotoDetails {
    /// Shown under the picture.
    caption: String,
    /// Index of the [`PhotoGroup`] it belongs to.
    group: usize,
}

/// Work for the background photo loader.
enum PhotoJob {
    /// Load this file into group `n`.
    File(std::path::PathBuf, usize, egui::Context),
    /// Show a file picker, then load what is chosen into group `n`.
    Pick(usize, egui::Context),
}

/// What the photo loader sends back, one message per file.
enum PhotoResult {
    /// A picture, ready to show, for group `group`.
    Picture { info: MediaInfo, bytes: std::sync::Arc<[u8]>, thumb: kn_media::Decoded, group: usize },
    /// A text file's words, to add to the story.
    Text(String),
    /// A file that was left out, and why.
    Note(String),
    /// A job is finished.
    Done,
}

/// Loads one file on the loader thread: a picture with its thumbnail, or a
/// text file's words. Returns false when the dialog has gone away.
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
        let bytes = media.get(&info.id).cloned().ok_or_else(|| format!("{name} was left out: it couldn't be read back"))?;
        Ok(PhotoResult::Picture { info, bytes, thumb, group })
    })();
    results.send(result.unwrap_or_else(PhotoResult::Note)).is_ok()
}

/// Starts the background thread that decodes pictures, so big photos never
/// freeze the dialog. Returns where to send jobs and where results arrive.
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
    /// The story text: blank lines separate slides.
    text: String,
    /// Current wizard page; content stays owned by the dialog between pages.
    step: usize,
    /// The pictures added so far.
    media: MediaLibrary,
    /// Each picture's details and the thumbnail shown in the dialog.
    pictures: Vec<(MediaInfo, egui::TextureHandle)>,
    /// Files that couldn't be used, in plain words.
    notes: Vec<String>,
    /// Thumbnails of the free sample artwork, loaded when first shown.
    sample_thumbs: Vec<egui::TextureHandle>,
    /// The photo groups, in order.
    groups: Vec<PhotoGroup>,
    /// Caption and group for each picture, same order as `pictures`.
    details: Vec<PhotoDetails>,
    /// The group new pictures go into.
    active_group: usize,
    /// Stable disclosure identities, independent of group titles and positions.
    group_ids: Vec<u64>,
    next_group_id: u64,
    /// Sends work to the photo loader.
    jobs: std::sync::mpsc::Sender<PhotoJob>,
    /// Receives the loader's results.
    loaded: std::sync::mpsc::Receiver<PhotoResult>,
    /// Jobs sent but not finished; Create waits until this is zero.
    loading: usize,
}

/// Everything needed to make (or remake) a quick video.
#[derive(Clone)]
pub(crate) struct QuickRequest {
    /// The story text.
    text: String,
    /// The pictures.
    media: MediaLibrary,
    /// Each picture's size and id, in order.
    pictures: Vec<MediaInfo>,
    /// The photo groups.
    groups: Vec<PhotoGroup>,
    /// Caption and group for each picture.
    details: Vec<PhotoDetails>,
    /// Look, shape, motion and music.
    choices: QuickChoices,
}

impl Quick {
    /// The dialog with `text` already typed in and one empty photo group.
    pub fn new(text: String) -> Self {
        let (jobs, loaded) = photo_loader();
        Self { text, step: 0, media: MediaLibrary::new(), pictures: Vec::new(), notes: Vec::new(), sample_thumbs: Vec::new(), groups: vec![PhotoGroup::default()], details: Vec::new(), active_group: 0, group_ids: vec![0], next_group_id: 1, jobs, loaded, loading: 0 }
    }

    /// Makes the sample artwork thumbnails the first time they are needed.
    fn load_samples(&mut self, ctx: &egui::Context) {
        if !self.sample_thumbs.is_empty() { return; }
        let media = kn_media::sample_library();
        for (i, (name, _)) in kn_media::SAMPLE_ART.iter().enumerate() {
            if let Ok(image) = media.thumbnail(&kn_media::sample_id(i), 224) {
                self.sample_thumbs.push(ctx.load_texture(*name, color_image(&image), egui::TextureOptions::LINEAR));
            }
        }
    }

    /// There is something to make, and no picture is still loading.
    fn ready(&self) -> bool {
        self.loading == 0 && (!self.text.trim().is_empty() || !self.pictures.is_empty())
    }

    /// Adds a picture file, or its text if it is a text file.
    pub fn add_file(&mut self, ctx: &egui::Context, path: &Path) {
        if self.jobs.send(PhotoJob::File(path.to_owned(), self.active_group, ctx.clone())).is_ok() { self.loading += 1; }
    }

    /// Takes in finished pictures from the loader (a few per frame).
    fn poll_photos(&mut self, ctx: &egui::Context) {
        // Keep a frame bounded even when many tiny files finish together.
        for _ in 0..4 {
            let Ok(result) = self.loaded.try_recv() else { break };
            match result {
                PhotoResult::Picture { info, bytes, thumb, group } => {
                    self.media.insert_shared(info.id.clone(), bytes);
                    self.push_picture(ctx, info, &thumb, group);
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

    /// Adds a picture from bytes (pasted or dropped without a file path).
    pub fn add_picture(&mut self, ctx: &egui::Context, bytes: Vec<u8>, name: &str) {
        let info = match self.media.import(bytes) {
            Ok(info) => info,
            Err(err) => return self.notes.push(format!("{name} was left out: {err}.")),
        };
        let thumb = match self.media.thumbnail(&info.id, (THUMB * 2.0) as u32) {
            Ok(t) => t,
            Err(err) => return self.notes.push(format!("{name} was left out: {err}.")),
        };
        self.push_picture(ctx, info, &thumb, self.active_group);
    }

    /// Lists a picture already in the library, in `group` (or the last
    /// group, if that one has been removed since).
    fn push_picture(&mut self, ctx: &egui::Context, info: MediaInfo, thumb: &kn_media::Decoded, group: usize) {
        let texture = ctx.load_texture(format!("quick-{}", info.id), color_image(thumb), egui::TextureOptions::LINEAR);
        self.pictures.push((info, texture));
        self.details.push(PhotoDetails { group: group.min(self.groups.len() - 1), ..Default::default() });
    }

    /// Adds sample artwork `index` to the active group, unless it is
    /// already there. Returns the picture's place in the list.
    fn add_sample(&mut self, ctx: &egui::Context, index: usize) -> Option<usize> {
        if self.has_sample(index) { return None; }
        let (name, bytes) = kn_media::SAMPLE_ART[index];
        let before = self.pictures.len();
        self.add_picture(ctx, bytes.to_vec(), name);
        (self.pictures.len() > before).then_some(before)
    }

    /// Sample artwork `index` is among the pictures.
    fn has_sample(&self, index: usize) -> bool {
        let id = kn_media::sample_id(index);
        self.pictures.iter().any(|(info, _)| info.id == id)
    }

    /// What to call group `index`: its title, or "Group 2" without one.
    fn group_name(&self, index: usize) -> String {
        let title = self.groups[index].title.trim();
        if title.is_empty() { format!("Group {}", index + 1) } else { title.to_owned() }
    }

    /// The nearest picture before (`later` false) or after `i` in the same group.
    fn neighbour(&self, i: usize, later: bool) -> Option<usize> {
        let group = self.details[i].group;
        let same = |j: &usize| self.details[*j].group == group;
        if later { (i + 1..self.details.len()).find(same) } else { (0..i).rev().find(same) }
    }

    /// Swaps two pictures' places in the story.
    fn swap_pictures(&mut self, a: usize, b: usize) {
        self.pictures.swap(a, b);
        self.details.swap(a, b);
    }
}

/// A decoded picture as an egui image.
fn color_image(image: &kn_media::Decoded) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba)
}

/// The ready-made story behind "Try a sample": its words, then the sample
/// artwork (by index in [`kn_media::SAMPLE_ART`]) with a caption for each.
const SAMPLE_STORY: &str = "Take the scenic route\nA few moments worth keeping\n\nA fresh perspective\n- Slow down\n- Look closer\n- Keep exploring";
const SAMPLE_GROUP: &str = "Moments outdoors";
const SAMPLE_PHOTOS: [(usize, &str); 3] = [(0, "A quieter kind of morning"), (5, "Salt air and open water"), (7, "Colour along the path")];

impl KinettoApp {
    /// Opens the quick video dialog, empty.
    pub(crate) fn open_quick(&mut self) {
        self.quick = Some(Quick::new(String::new()));
    }

    /// Opens the dialog filled with a sample story and a captioned collage,
    /// so one click on Create shows what a finished quick video looks like.
    pub(crate) fn open_quick_sample(&mut self, ctx: &egui::Context) {
        let mut quick = Quick::new(SAMPLE_STORY.into());
        quick.groups[0] = PhotoGroup { title: SAMPLE_GROUP.into(), collage: true };
        for (index, caption) in SAMPLE_PHOTOS {
            if let Some(i) = quick.add_sample(ctx, index) {
                quick.details[i].caption = caption.into();
            }
        }
        self.quick = Some(quick);
    }

    /// Asks the loader to show a picture picker for the active group.
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
        // The page scrolls inside the dialog; the steps above it and the
        // buttons below it always stay in view.
        let page_height = (ctx.content_rect().height() - CHROME).clamp(160.0, 520.0);
        egui::Modal::new(egui::Id::unique("quick")).frame(frame).show(ctx, |ui| {
            let Some(quick) = self.quick.as_mut() else { return };
            let width = (ctx.content_rect().width() - 80.0).clamp(240.0, WIZARD_WIDTH);
            ui.set_width(width);
            quick.load_samples(ctx);
            ui.label(RichText::new("Create a quick video").family(theme::bold()).size(22.0));
            ui.label(RichText::new("Three simple steps. Everything stays editable.").color(p.dim));
            ui.add_space(14.0);
            stepper(ui, &mut quick.step);
            ui.add_space(6.0);
            ui.separator();
            egui::ScrollArea::vertical().id_salt(("quick-step", quick.step))
                .max_height(page_height).min_scrolled_height(page_height).auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width((width - 16.0).max(200.0));
                    ui.add_space(8.0);
                    match quick.step {
                        0 => content(ui, quick, &mut add_pictures),
                        1 => choose_look(ui, &mut choices),
                        _ => choose_motion(ui, &mut choices),
                    }
                    ui.add_space(12.0);
                });
            ui.separator();
            ui.add_space(6.0);
            action = footer(ui, quick);
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

/// The wizard's pages, in order.
const STEPS: [&str; 3] = ["Content", "Style", "Motion & music"];
/// Height of everything in the dialog around the scrolling page: margins,
/// title, steps and buttons, and room to see the window behind.
const CHROME: f32 = 330.0;

/// The numbered steps across the top: the current one is filled, finished
/// ones show a tick, and any can be clicked to jump there.
fn stepper(ui: &mut egui::Ui, step: &mut usize) {
    let p = palette(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (index, label) in STEPS.iter().enumerate() {
            if index > 0 {
                let (line, _) = ui.allocate_exact_size(Vec2::new(22.0, 34.0), Sense::hover());
                let color = if index <= *step { p.accent_bg } else { p.border };
                ui.painter().hline(line.x_range(), line.center().y, Stroke::new(2.0, color));
            }
            let current = index == *step;
            let font = egui::FontId::new(13.5, if current { theme::semibold() } else { egui::FontFamily::Proportional });
            let galley = ui.painter().layout_no_wrap((*label).to_owned(), font, if index <= *step { p.fg } else { p.dim });
            let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0 + 8.0 + galley.size().x + 6.0, 34.0), Sense::click());
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, current, format!("Step {}: {label}", index + 1)));
            if response.hovered() && !current {
                ui.painter().rect_filled(rect.expand2(Vec2::new(4.0, 0.0)), theme::BUTTON_RADIUS, p.button[0]);
            }
            let centre = egui::pos2(rect.left() + 12.0, rect.center().y);
            if index < *step {
                // Done: a tick on a soft accent disc.
                ui.painter().circle_filled(centre, 12.0, p.accent_bg.gamma_multiply(0.22));
                let tick = [centre + Vec2::new(-4.5, 0.5), centre + Vec2::new(-1.5, 3.5), centre + Vec2::new(4.5, -3.5)];
                ui.painter().add(egui::Shape::line(tick.to_vec(), Stroke::new(1.8, p.accent)));
            } else {
                let (fill, ink) = if current { (p.accent_bg, p.accent_fg) } else { (p.button[0], p.dim) };
                ui.painter().circle_filled(centre, 12.0, fill);
                ui.painter().text(centre, egui::Align2::CENTER_CENTER, (index + 1).to_string(), egui::FontId::new(12.5, theme::semibold()), ink);
            }
            let color = if index <= *step { p.fg } else { p.dim };
            ui.painter().galley(egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0), galley, color);
            if response.has_focus() {
                ui.painter().rect_stroke(rect.expand(2.0), theme::BUTTON_RADIUS, Stroke::new(2.0, p.accent), egui::StrokeKind::Outside);
            }
            if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                *step = index;
            }
        }
    });
}

/// The buttons under the page: Cancel on the left; Back and the main
/// action on the right. Returns `Some(true)` to make the video and
/// `Some(false)` to close without making one.
fn footer(ui: &mut egui::Ui, quick: &mut Quick) -> Option<bool> {
    let mut action = None;
    let last = STEPS.len() - 1;
    let shortcut = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 36.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        if adw::button(ui, None, "Cancel", Kind::Flat).clicked() {
            action = Some(false);
        }
        if quick.loading > 0 {
            ui.spinner();
            ui.label(RichText::new(format!("Loading photos… {} ready", quick.pictures.len())).size(12.5).color(palette(ui.ctx()).dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = quick.ready();
            if quick.step == last {
                let make = ui.add_enabled_ui(ready, |ui| adw::button(ui, Some(Icon::Video), "Create video", Kind::Suggested)).inner;
                let make = make.on_hover_text("Ctrl+Enter").on_disabled_hover_text(if quick.loading > 0 { "Waiting for your photos to load" } else { "Add some words or pictures first" });
                if make.clicked() || (ready && shortcut) { action = Some(true); }
            } else if adw::button(ui, None, "Continue", Kind::Suggested).clicked() {
                quick.step += 1;
            }
            if quick.step > 0 && adw::button(ui, None, "Back", Kind::Normal).clicked() {
                quick.step -= 1;
            }
        });
    });
    action
}

/// Adds the photo groups to the outline as slides, with their titles and captions.
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

/// The first page: the story text, photo groups and sample artwork.
fn content(ui: &mut egui::Ui, quick: &mut Quick, add_pictures: &mut bool) {
    let p = palette(ui.ctx());
    heading(ui, "Your content", "Text is optional. Blank lines separate slides; add photo groups below.");
    ui.add(egui::TextEdit::multiline(&mut quick.text).desired_rows(6).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(12, 10))
        .hint_text("Give your story a title\nAdd a short introduction\n\nYour next slide\n- One clear idea\n- Another good thought"));
    ui.add_space(14.0);
    heading(ui, "Photo groups", "Organize photos into slides or collages. Expand a group to edit it.");
    let mut remove = None;
    let mut swap = None;
    let mut delete_group = None;
    for group_index in 0..quick.groups.len() {
        ui.push_id(("photo-group", quick.group_ids[group_index]), |ui| {
            egui::Frame::new().fill(p.card).corner_radius(10).inner_margin(12).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let count = quick.details.iter().filter(|d| d.group == group_index).count();
                let title = quick.group_name(group_index);
                let layout = if quick.groups[group_index].collage { "Collage" } else { "One per slide" };
                let disclosure = egui::CollapsingHeader::new(RichText::new(title).family(theme::semibold()))
                    .icon(adw::disclosure_icon).id_salt("contents").default_open(true).show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut quick.groups[group_index].title).desired_width(f32::INFINITY).hint_text("Group title (optional)"));
                    ui.add_space(2.0);
                    adw::toggle_group(ui, &mut quick.groups[group_index].collage, &[(false, "One photo per slide"), (true, "Collage on one slide")]);
                    if count == 0 { ui.label(RichText::new("Add photos to start this group").size(12.0).color(p.dim)); }
                    for i in 0..quick.pictures.len() {
                        if quick.details[i].group != group_index { continue; }
                        ui.push_id(("photo", i), |ui| {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let (_, texture) = &quick.pictures[i];
                                thumbnail(ui, texture);
                                ui.vertical(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut quick.details[i].caption).desired_width(ui.available_width()).hint_text("Photo title or caption"));
                                    // Every action on a photo is a visible button: order, group, remove.
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let (earlier, later) = (quick.neighbour(i, false), quick.neighbour(i, true));
                                        if ui.add_enabled_ui(earlier.is_some(), |ui| adw::icon_button(ui, Icon::MoveUp, "Move photo earlier")).inner.clicked() { swap = earlier.map(|j| (i, j)); }
                                        if ui.add_enabled_ui(later.is_some(), |ui| adw::icon_button(ui, Icon::MoveDown, "Move photo later")).inner.clicked() { swap = later.map(|j| (i, j)); }
                                        if quick.groups.len() > 1 {
                                            ui.add_space(6.0);
                                            adw::dropdown(ui, "move-group", "Move to group", |ui| {
                                                for target in 0..quick.groups.len() {
                                                    let label = quick.group_name(target);
                                                    ui.selectable_value(&mut quick.details[i].group, target, label);
                                                }
                                            });
                                        }
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if adw::icon_button(ui, Icon::Delete, "Remove photo").clicked() { remove = Some(i); }
                                        });
                                    });
                                });
                            });
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if adw::button(ui, Some(Icon::Picture), "Add pictures…", Kind::Normal).clicked() {
                            quick.active_group = group_index;
                            *add_pictures = true;
                        }
                        if quick.groups.len() > 1 {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_enabled_ui(quick.loading == 0, |ui| {
                                    if adw::button(ui, Some(Icon::Delete), "Remove group", Kind::Flat)
                                        .on_hover_text(if group_index == 0 { "Move its photos to the next group" } else { "Move its photos to the previous group" }).clicked() {
                                        delete_group = Some(group_index);
                                    }
                                });
                            });
                        }
                    });
                });
                if disclosure.body_returned.is_none() {
                    ui.label(RichText::new(format!("{count} {} · {layout}", if count == 1 { "photo" } else { "photos" })).size(12.0).color(p.dim));
                }
            });
        });
        ui.add_space(8.0);
    }
    if let Some((a, b)) = swap { quick.swap_pictures(a, b); }
    if let Some(i) = remove { drop(quick.pictures.remove(i)); quick.details.remove(i); }
    if let Some(i) = delete_group {
        quick.groups.remove(i);
        quick.group_ids.remove(i);
        for detail in &mut quick.details {
            if detail.group == i { detail.group = i.saturating_sub(1); }
            else if detail.group > i { detail.group -= 1; }
        }
        quick.active_group = if quick.active_group >= i { quick.active_group.saturating_sub(1) } else { quick.active_group };
    }
    if adw::button(ui, Some(Icon::Add), "Add photo group", Kind::Normal).clicked() {
        quick.groups.push(PhotoGroup::default());
        quick.group_ids.push(quick.next_group_id);
        quick.next_group_id += 1;
        quick.active_group = quick.groups.len() - 1;
    }
    ui.add_space(12.0);
    egui::CollapsingHeader::new("Free sample artwork").icon(adw::disclosure_icon).id_salt("quick-samples").default_open(false).show(ui, |ui| {
        if quick.groups.len() > 1 {
            adw::dropdown(ui, "artwork-target", &format!("Add artwork to {}", quick.group_name(quick.active_group)), |ui| {
                for i in 0..quick.groups.len() {
                    let label = quick.group_name(i);
                    ui.selectable_value(&mut quick.active_group, i, label);
                }
            });
        }
        ui.label(RichText::new("Included with Kinetto. Click one to add it; a tick marks those already in.").color(p.dim).size(12.0));
        let mut add = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            for (i, texture) in quick.sample_thumbs.iter().enumerate() {
                if sample_tile(ui, texture, kn_media::SAMPLE_ART[i].0, quick.has_sample(i)).clicked() { add = Some(i); }
            }
        });
        if let Some(i) = add { quick.add_sample(ui.ctx(), i); }
    });
    for note in &quick.notes { ui.label(RichText::new(note).size(12.5).color(p.destructive)); }
}

/// A bold section title with a dim line under it.
fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    // The same top space as `adw::group`, so both columns' titles line up.
    ui.add_space(4.0);
    ui.label(RichText::new(title).family(theme::bold()).size(14.0));
    ui.label(RichText::new(description).size(12.5).color(palette(ui.ctx()).dim));
    ui.add_space(4.0);
}

/// The look, the shape and how everything moves.
fn choose_look(ui: &mut egui::Ui, choices: &mut QuickChoices) {
    // Keep the essential choices visible; optional controls expand on demand.
    ui.spacing_mut().item_spacing.y = 4.0;
    adw::group(ui, "Choose a look", Some("Pick a colour palette for your video"), |ui| {
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
}

/// Motion and sound share one focused wizard page.
fn choose_motion(ui: &mut egui::Ui, choices: &mut QuickChoices) {
    heading(ui, "Set the mood", "Add movement and music, or keep the defaults and create your video.");
    disclosure(ui, "Animation", "Random gives every slide its own", false, |ui| {
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
    let photo_summary = match choices.style.photo_motion {
        Pick::Look => "Auto · follows the look",
        Pick::Random => "Random mix",
        Pick::Every(motion) => motion.name(),
    };
    disclosure(ui, "Photo motion", photo_summary, true, |ui| {
        ui.label(RichText::new("Smooth camera movement throughout each slide.").size(12.0).color(palette(ui.ctx()).dim));
        adw::boxed_list(ui, |list| {
            list.row("Camera effect", None, |ui| {
                let all: Vec<_> = kn_model::PhotoMotion::ALL.iter().map(|m| (*m, m.name())).collect();
                pick_dropdown(ui, "quick-photo-motion", &mut choices.style.photo_motion, "Auto", &all);
            });
            if !matches!(choices.style.photo_motion, Pick::Look | Pick::Every(kn_model::PhotoMotion::None)) {
                let percent = format!("{}%", choices.style.photo_amount_percent);
                list.row("Intensity", Some(&percent), |ui| {
                    let mut amount = f64::from(choices.style.photo_amount_percent);
                    if adw::slider(ui, &mut amount, 0.0..=40.0, 1.0, "Photo motion intensity").changed() {
                        choices.style.photo_amount_percent = amount.round() as u8;
                    }
                });
            }
        });
    });
    let soundtrack = choices.music.map_or("No music".to_owned(), |m| format!("{} · {:.0}%", m.name(), choices.music_volume * 100.0));
    disclosure(ui, "Soundtrack", &soundtrack, false, |ui| {
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

/// Optional settings use standard keyboard-accessible disclosures.
fn disclosure(ui: &mut egui::Ui, title: &str, summary: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    egui::Frame::new().fill(palette(ui.ctx()).card).corner_radius(10).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let disclosure = egui::CollapsingHeader::new(RichText::new(title).family(theme::semibold()))
            .icon(adw::disclosure_icon).id_salt(("quick-settings", title)).default_open(open).show(ui, add);
        if disclosure.body_returned.is_none() {
            ui.label(RichText::new(summary).size(12.0).color(palette(ui.ctx()).dim));
        }
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

/// Paints `texture` cropped to fill `rect`, with rounded corners and a hairline.
fn paint_cover(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect) {
    let size = texture.size_vec2();
    let fit = (rect.width() / size.x).max(rect.height() / size.y);
    let shown = size * fit;
    let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), Vec2::new(rect.width() / shown.x, rect.height() / shown.y));
    egui::Image::new(texture).uv(uv).corner_radius(8).paint_at(ui, rect);
    ui.painter().rect_stroke(rect, 8, Stroke::new(1.0, palette(ui.ctx()).border), egui::StrokeKind::Inside);
}

/// A picture in a photo group.
fn thumbnail(ui: &mut egui::Ui, texture: &egui::TextureHandle) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(THUMB), Sense::hover());
    paint_cover(ui, texture, rect);
}

/// One piece of sample artwork: click to add it. Once added it is dimmed
/// and ticked, and can't be added twice.
fn sample_tile(ui: &mut egui::Ui, texture: &egui::TextureHandle, name: &str, added: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(102.0, 76.5), if added { Sense::hover() } else { Sense::click() });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !added, format!("Add sample: {name}")));
    paint_cover(ui, texture, rect);
    if added {
        ui.painter().rect_filled(rect, 8, Color32::from_black_alpha(90));
        let badge = egui::Rect::from_center_size(rect.right_top() + Vec2::new(-15.0, 15.0), Vec2::splat(20.0));
        ui.painter().circle_filled(badge.center(), 10.0, p.accent_bg);
        crate::icons::paint(ui.painter(), badge.shrink(4.0), Icon::Check, p.accent_fg);
        return response.on_hover_text(format!("{name} is already added"));
    }
    if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(rect, 8, Stroke::new(2.0, p.accent_bg), egui::StrokeKind::Inside);
    }
    response.on_hover_text(format!("Add {name}")).on_hover_cursor(egui::CursorIcon::PointingHand)
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

/// A dropdown for how items arrive (or, with `exit`, leave): the look's
/// choice, random, or one motion for every slide.
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
    fn legacy_quick_settings_keep_camera_defaults() {
        let choices: QuickChoices = serde_json::from_str(r#"{"style":{"motion":"Look","transition":"Look","exit":"Look"}}"#).unwrap();
        assert_eq!(choices.style.photo_motion, Pick::Look);
        assert_eq!(choices.style.photo_amount_percent, 12);
    }

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
    fn photos_reorder_within_their_group_even_when_groups_interleave() {
        let ctx = egui::Context::default();
        let mut quick = Quick::new(String::new());
        quick.groups.push(PhotoGroup::default());
        // Group 0 holds pictures 0 and 2; picture 1 belongs to group 1.
        for (sample, group) in [(0, 0), (1, 1), (2, 0)] {
            quick.active_group = group;
            assert!(quick.add_sample(&ctx, sample).is_some());
        }
        assert_eq!(quick.add_sample(&ctx, 0), None, "a sample is only added once");
        assert_eq!((quick.neighbour(0, false), quick.neighbour(0, true)), (None, Some(2)));
        assert_eq!((quick.neighbour(1, false), quick.neighbour(1, true)), (None, None));
        let first = quick.pictures[0].0.id.clone();
        quick.swap_pictures(0, 2);
        assert_eq!(quick.pictures[2].0.id, first);
        assert_eq!(quick.details.iter().map(|d| d.group).collect::<Vec<_>>(), [0, 1, 0], "the other group keeps its place");
    }

    #[test]
    fn sample_story_fills_a_captioned_collage() {
        let ctx = egui::Context::default();
        let mut quick = Quick::new(SAMPLE_STORY.into());
        quick.groups[0] = PhotoGroup { title: SAMPLE_GROUP.into(), collage: true };
        for (index, caption) in SAMPLE_PHOTOS {
            let i = quick.add_sample(&ctx, index).expect("each sample picture is distinct");
            quick.details[i].caption = caption.into();
        }
        assert_eq!(quick.pictures.len(), SAMPLE_PHOTOS.len());
        assert_eq!(quick.group_name(0), SAMPLE_GROUP);
        let pictures: Vec<_> = quick.pictures.iter().map(|(info, _)| info.clone()).collect();
        let mut outline = kn_import::Outline::default();
        append_photo_groups(&mut outline, &pictures, &quick.groups, &quick.details);
        assert!(matches!(&outline.parts[..], [Part::PhotoGroup { photos, .. }] if photos.len() == 3 && photos.iter().all(|p| !p.caption.is_empty())));
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
