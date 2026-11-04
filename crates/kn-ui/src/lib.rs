//! The Kinetto editor window: top bar, slide strip, stage and inspector.

mod canvas;
#[doc(hidden)]
pub mod device;
mod export;
mod inspector;
mod sound;
mod stage;
mod start;
mod strip;

use canvas::VelloCanvas;
use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use kn_engine::storage::{self, Bundle, Recovery};
use kn_engine::{Command, Session};
use kn_geom::{Color, Rect};
use kn_model::{Element, Project};
use kn_render::RenderContext;
use kn_time::Tick;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Work is written to disk this long after the last edit.
const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);
const TOAST_TIME: Duration = Duration::from_secs(4);

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);

/// Something that would throw away unsaved work, waiting for the user's OK.
#[derive(Clone)]
enum Pending {
    New,
    Open(PathBuf),
}

pub struct KinettoApp {
    session: Session,
    render: RenderContext,
    canvas: Option<VelloCanvas>,
    recovery: Option<Recovery>,
    /// Work found from a previous run that closed without saving.
    restorable: Option<Bundle>,
    /// The "What are you making?" screen is showing.
    choosing_template: bool,
    pending: Option<Pending>,

    slide: usize,
    selected: Option<usize>,
    playing: bool,
    /// Project time when playback started, and the wall-clock moment it did.
    play_origin: Tick,
    play_started: Instant,
    /// The sound card; `None` when there is none, and preview is silent.
    player: Option<device::Player>,
    /// Soundtrack mixed for the given revision; playing it follows the
    /// sound card's clock.
    soundtrack: Option<(u64, Option<std::sync::Arc<kn_audio::Pcm>>)>,
    audio_cache: kn_audio::AudioCache,
    audio_clock: bool,
    /// A voice take in progress, and the slide it is for.
    recording: Option<(usize, device::Recorder)>,
    play_time: Tick,
    drag: Option<stage::Drag>,
    export: Option<export::ExportJob>,

    /// Last revision written to the file or the recovery copy.
    persisted: u64,
    toast: Option<(String, Instant)>,
    title: String,
}

impl KinettoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let canvas = cc.wgpu_render_state.as_ref().and_then(|rs| match VelloCanvas::new(rs) {
            Ok(c) => Some(c),
            Err(err) => {
                tracing::error!("could not start the canvas renderer: {err}");
                None
            }
        });
        let recovery = Recovery::in_data_dir();
        let restorable = recovery.as_ref().and_then(Recovery::pending);
        Self {
            session: Session::new(Project::sample()),
            render: RenderContext::new(),
            canvas,
            recovery,
            restorable,
            choosing_template: true,
            pending: None,
            slide: 0,
            selected: None,
            playing: false,
            play_origin: Tick::ZERO,
            play_started: Instant::now(),
            player: device::Player::new(),
            soundtrack: None,
            audio_cache: kn_audio::AudioCache::default(),
            audio_clock: false,
            recording: None,
            play_time: Tick::ZERO,
            drag: None,
            export: None,
            persisted: 0,
            toast: None,
            title: String::new(),
        }
    }

    fn project(&self) -> &Project {
        self.session.project()
    }

    fn slide_start(&self, index: usize) -> Tick {
        self.project().slides[..index].iter().fold(Tick::ZERO, |acc, s| acc + s.duration)
    }

    /// The moment shown while editing: the current slide with every
    /// entrance finished.
    fn edit_time(&self) -> Tick {
        let slide = &self.project().slides[self.slide];
        let rest = kn_render::rest_time(slide.elements.len()).min(slide.duration - Tick(1));
        self.slide_start(self.slide) + rest
    }

    fn run(&mut self, command: Command) {
        if let Err(err) = self.session.execute(command) {
            self.notify(err.to_string());
        }
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        self.slide = self.slide.min(self.project().slides.len() - 1);
        let count = self.project().slides[self.slide].elements.len();
        self.selected = self.selected.filter(|&i| i < count);
    }

    fn select_slide(&mut self, index: usize) {
        self.slide = index;
        self.selected = None;
        self.stop_playback();
    }

    fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    fn toggle_play(&mut self) {
        if self.playing {
            self.stop_playback();
        } else {
            self.play_from(self.slide_start(self.slide));
        }
    }

    /// Plays from `origin`, with sound when the project has any.
    fn play_from(&mut self, origin: Tick) {
        self.playing = true;
        self.selected = None;
        self.play_origin = origin;
        self.play_started = Instant::now();
        self.play_time = origin;
        let revision = self.session.revision();
        if self.soundtrack.as_ref().is_none_or(|(r, _)| *r != revision) {
            let pcm = kn_audio::mix(self.session.project(), &self.session.media, &mut self.audio_cache);
            self.soundtrack = Some((revision, pcm.map(std::sync::Arc::new)));
        }
        let track = self.soundtrack.as_ref().and_then(|(_, p)| p.clone());
        self.audio_clock = match (&self.player, track) {
            (Some(player), Some(pcm)) => {
                player.play(pcm, (origin.as_secs_f64() * f64::from(kn_audio::RATE)) as usize);
                true
            }
            _ => false,
        };
    }

    fn stop_playback(&mut self) {
        self.playing = false;
        if let Some(player) = &self.player {
            player.stop();
        }
    }

    fn advance_playback(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }
        // With sound, the sound card is the clock so picture never drifts
        // from it; without, time is measured from a fixed start.
        let secs = match (&self.player, self.audio_clock) {
            (Some(player), true) => player.position_secs(),
            _ => self.play_origin.as_secs_f64() + self.play_started.elapsed().as_secs_f64(),
        };
        self.play_time = Tick((secs * kn_time::TICKS_PER_SECOND as f64) as i64);
        match self.project().slide_at(self.play_time) {
            Some((index, _)) => self.slide = index,
            None => self.stop_playback(),
        }
        ctx.request_repaint();
    }

    fn add_text(&mut self) {
        let (_, h) = self.project().aspect.canvas_size();
        let bg = self.project().slides[self.slide].background;
        let element = Element::text("Your text", Rect::new(360.0, h / 2.0 - 60.0, 1200.0, 120.0), 80.0, true, readable_on(bg));
        self.add_element(element);
    }

    /// New shapes take the look's next accent colour, and follow it later.
    fn add_shape(&mut self) {
        let (_, h) = self.project().aspect.canvas_size();
        let theme = kn_layout::theme(&self.project().theme);
        let n = self.project().slides[self.slide].elements.len() % 3;
        let shape = Element::shape(Rect::new(760.0, h / 2.0 - 150.0, 400.0, 300.0), theme.accents[n]).with_role(kn_model::Role::Card(n as u8));
        self.add_element(shape);
    }

    fn pick_pictures(&mut self) {
        let files = rfd::FileDialog::new().add_filter("Pictures", PICTURE_EXTENSIONS).pick_files().unwrap_or_default();
        for path in files {
            self.add_picture_file(&path);
        }
    }

    fn add_picture_file(&mut self, path: &Path) {
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > kn_media::MAX_FILE_BYTES as u64);
        if too_big {
            self.notify("That picture is over 100 MB.");
            return;
        }
        match std::fs::read(path) {
            Ok(bytes) => self.add_picture(bytes),
            Err(err) => self.notify(format!("Couldn't read that picture: {err}")),
        }
    }

    /// Adds a picture centred on the slide, as large as fits comfortably.
    fn add_picture(&mut self, bytes: Vec<u8>) {
        let info = match self.session.media.import(bytes) {
            Ok(info) => info,
            Err(err) => return self.notify(err.to_string()),
        };
        let (cw, ch) = self.project().aspect.canvas_size();
        let (iw, ih) = (f64::from(info.width), f64::from(info.height));
        let fit = (cw * 0.7 / iw).min(ch * 0.7 / ih);
        let (w, h) = (iw * fit, ih * fit);
        // Each extra picture on a slide lands a little lower-right.
        let nudge = 40.0 * self.project().slides[self.slide].elements.len() as f64 % 200.0;
        self.add_element(Element::image(info.id, Rect::new((cw - w) / 2.0 + nudge, (ch - h) / 2.0 + nudge, w, h)));
    }

    /// Pictures dropped on the window are added; a project file is opened.
    fn dropped_files(&mut self, ctx: &egui::Context) {
        let files = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        for file in files {
            let path = file.path();
            if path.extension().is_some_and(|e| e == storage::EXTENSION) {
                self.request(Pending::Open(path.to_path_buf()));
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| kn_media::AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())) {
                self.add_dropped_sound(path);
            } else if !path.as_os_str().is_empty() {
                self.add_picture_file(path);
            } else {
                // Some platforms hand over bytes without a path.
                match file.bytes() {
                    Ok(bytes) => self.add_picture(bytes),
                    Err(err) => self.notify(format!("Couldn't read that file: {err}")),
                }
            }
        }
    }

    fn drop_hint(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
        painter.rect_filled(screen, 0, egui::Color32::from_black_alpha(160));
        painter.text(screen.center(), egui::Align2::CENTER_CENTER, "Drop pictures or sounds to add them", egui::FontId::proportional(28.0), egui::Color32::WHITE);
    }

    fn add_element(&mut self, element: Element) {
        self.stop_playback();
        self.run(Command::AddElement { slide: self.slide, element });
        self.session.seal();
        self.selected = Some(self.project().slides[self.slide].elements.len() - 1);
    }

    // ----- files -----

    fn request(&mut self, action: Pending) {
        if self.session.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => self.choosing_template = true,
            Pending::Open(path) => match storage::load(&path) {
                Ok(bundle) => self.replace_session(Session::from_bundle(bundle, Some(path))),
                Err(err) => self.notify(format!("Couldn't open that file: {err}")),
            },
        }
    }

    fn replace_session(&mut self, session: Session) {
        self.session = session;
        self.persisted = self.session.revision();
        self.slide = 0;
        self.selected = None;
        self.stop_playback();
        if let Some(r) = &self.recovery {
            r.clear();
        }
    }

    fn pick_open(&mut self) {
        if let Some(path) = file_dialog().pick_file() {
            self.request(Pending::Open(path));
        }
    }

    /// Saves to the current file, asking for a name the first time.
    fn save(&mut self) -> bool {
        if self.session.path.is_none() {
            return self.save_as();
        }
        self.write_file()
    }

    fn save_as(&mut self) -> bool {
        let Some(mut path) = file_dialog().set_file_name("My video.kinetto").save_file() else { return false };
        if path.extension().is_none() {
            path.set_extension(storage::EXTENSION);
        }
        self.session.path = Some(path);
        self.write_file()
    }

    fn write_file(&mut self) -> bool {
        let Some(path) = self.session.path.clone() else { return false };
        let revision = self.session.revision();
        match storage::save(self.session.project(), &self.session.media, &path) {
            Ok(()) => {
                self.session.mark_saved(revision);
                self.persisted = revision;
                if let Some(r) = &self.recovery {
                    r.clear();
                }
                true
            }
            Err(err) => {
                self.notify(format!("Couldn't save: {err}"));
                false
            }
        }
    }

    /// Named projects save themselves; untitled ones go to the recovery copy.
    fn autosave(&mut self, force: bool) {
        let revision = self.session.revision();
        if revision == self.persisted {
            return;
        }
        let idle = self.session.idle_since_edit().unwrap_or_default();
        if !force && (idle < AUTOSAVE_DELAY || self.drag.is_some()) {
            return;
        }
        if self.session.path.is_some() {
            self.write_file();
        } else if let Some(r) = &self.recovery {
            match r.write(self.session.project(), &self.session.media) {
                Ok(()) => self.persisted = revision,
                Err(err) => tracing::warn!("recovery copy failed: {err}"),
            }
        }
    }

    // ----- frame -----

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let (undo, redo, save, open, new) = ctx.input_mut(|i| {
            (
                !typing && i.consume_shortcut(&UNDO),
                !typing && (i.consume_shortcut(&REDO) || i.consume_shortcut(&REDO_ALT)),
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&NEW),
            )
        });
        if undo && self.session.undo() {
            self.clamp_selection();
        }
        if redo && self.session.redo() {
            self.clamp_selection();
        }
        if save {
            self.save();
        }
        if open {
            self.pick_open();
        }
        if new {
            self.request(Pending::New);
        }
        if !typing && ctx.input(|i| i.key_pressed(Key::Space)) {
            self.toggle_play();
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .session
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Untitled".to_owned(), |s| s.to_string_lossy().into_owned());
        let dot = if self.session.is_dirty() && self.session.path.is_some() { " •" } else { "" };
        let title = format!("{name}{dot} — Kinetto");
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Kinetto").heading().strong());
            ui.separator();
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.request(Pending::New);
                }
                if ui.button("Open…").clicked() {
                    self.pick_open();
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                }
            });
            let undo = self.session.undo_label();
            if ui.add_enabled(undo.is_some(), egui::Button::new("Undo")).on_hover_text(undo.unwrap_or("Nothing to undo")).clicked() {
                self.session.undo();
                self.clamp_selection();
            }
            let redo = self.session.redo_label();
            if ui.add_enabled(redo.is_some(), egui::Button::new("Redo")).on_hover_text(redo.unwrap_or("Nothing to redo")).clicked() {
                self.session.redo();
                self.clamp_selection();
            }
            ui.separator();
            if ui.button("Add text").clicked() {
                self.add_text();
            }
            if ui.button("Add shape").clicked() {
                self.add_shape();
            }
            if ui.button("Add picture").on_hover_text("Or drop pictures onto the window").clicked() {
                self.pick_pictures();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let make = egui::Button::new(RichText::new("Make Video").strong()).fill(ui.visuals().selection.bg_fill);
                if ui.add(make).on_hover_text("Save your slides as an MP4 video").clicked() {
                    self.make_video(frame);
                }
                let label = if self.playing { "Stop" } else { "Play" };
                if ui.button(label).on_hover_text("Space").clicked() {
                    self.toggle_play();
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.restorable.is_some() {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("restore")).show(ctx, |ui| {
                ui.heading("Welcome back");
                ui.label("Kinetto closed before your last video was saved. Bring it back?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore my work").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Start fresh").clicked() {
                        choice = Some(false);
                    }
                });
            });
            match choice {
                Some(true) => {
                    let bundle = self.restorable.take().expect("checked above");
                    self.replace_session(Session::from_bundle(bundle, None));
                }
                Some(false) => {
                    self.restorable = None;
                    if let Some(r) = &self.recovery {
                        r.clear();
                    }
                }
                None => {}
            }
        }

        if let Some(action) = self.pending.clone() {
            let mut choice = None;
            egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
                ui.heading("Save your changes?");
                ui.label("Your current video has changes that aren't saved to a file.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some("save");
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some("discard");
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some("cancel");
                    }
                });
            });
            match choice {
                Some("save") => {
                    self.pending = None;
                    if self.save() {
                        self.perform(action);
                    }
                }
                Some("discard") => {
                    self.pending = None;
                    self.perform(action);
                }
                Some(_) => self.pending = None,
                None => {}
            }
        }
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some((message, at)) = &self.toast else { return };
        if at.elapsed() > TOAST_TIME {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -24.0])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(message.as_str()));
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

impl eframe::App for KinettoApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.dropped_files(&ctx);
        self.advance_playback(&ctx);

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui, frame);
            ui.add_space(4.0);
        });
        egui::Panel::left("slides").resizable(false).exact_size(196.0).show(ui, |ui| self.slide_strip(ui, frame));
        egui::Panel::right("inspector").resizable(false).exact_size(280.0).show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default().show(ui, |ui| self.stage(ui, frame));

        self.drop_hint(&ctx);
        self.dialogs(&ctx);
        self.start_screen(&ctx);
        self.export_dialog(&ctx);
        self.toast(&ctx);
        self.update_title(&ctx);
        self.autosave(false);
        if self.session.revision() != self.persisted {
            ctx.request_repaint_after(AUTOSAVE_DELAY);
        }
    }

    fn on_exit(&mut self) {
        self.autosave(true);
    }
}

const PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

fn file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Kinetto video", &[storage::EXTENSION])
}

/// Black or white, whichever reads better on `bg`.
fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f64::from(bg.r) + 0.587 * f64::from(bg.g) + 0.114 * f64::from(bg.b);
    if luma > 150.0 { Color::rgb(24, 24, 32) } else { Color::rgb(250, 247, 242) }
}

fn to_egui(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

fn from_egui(c: egui::Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}
