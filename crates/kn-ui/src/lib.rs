//! The Kinetto editor window: top bar, slide strip, stage and inspector.

mod adw;
mod canvas;
#[doc(hidden)]
pub mod device;
mod export;
mod header;
mod icons;
mod import;
mod inspector;
mod present;
mod problem;
mod quick;
mod settings;
mod sound;
mod stage;
mod start;
mod strip;
mod theme;
mod timeline;
mod transitions;

use canvas::VelloCanvas;
use problem::{Fix, Problem};
use egui::{Key, KeyboardShortcut, Modifiers};
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
/// After a failed automatic save, wait this long before trying again.
const AUTOSAVE_RETRY: Duration = Duration::from_secs(10);
const TOAST_TIME: Duration = Duration::from_secs(4);

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);

/// Something that would throw away unsaved work, waiting for the user's OK.
#[derive(Clone)]
pub(crate) enum Pending {
    New,
    Open(PathBuf),
    Import(PathBuf),
    Quick(Box<quick::QuickRequest>),
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
    /// Soundtrack mixed for the given content stamp; playing it follows the
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
    /// Bumped whenever another project replaces the open one. Revisions
    /// restart at zero per project, so cached pictures key on both.
    generation: u64,
    toast: Option<Toast>,
    title: String,
    about: bool,
    /// The "Make a video in seconds" dialog, while it is open.
    quick: Option<quick::Quick>,
    /// Full-screen presenting, when on.
    presenting: Option<present::Present>,
    /// A failure of something the user asked for, shown as a dialog.
    problem: Option<Problem>,
    /// Automatic saving is failing: why, shown as a banner until it works.
    save_trouble: Option<String>,
    autosave_failed_at: Option<Instant>,
    /// The user has been told once that there's no sound output.
    told_silent: bool,
    settings: settings::Settings,
    /// A moment picked on the timeline to look at while editing; `None`
    /// shows the current slide at rest.
    scrub: Option<Tick>,
    /// Timeline drags in progress.
    timeline_drag: Option<timeline::TimelineDrag>,
    /// Waveform peaks of the current song, by media id.
    waveform: Option<(String, std::sync::Arc<Vec<f32>>)>,
    /// An export chosen from a menu, started once the menu has closed.
    export_request: Option<export::Output>,
    /// Starter projects shown on the welcome page.
    templates: Vec<(kn_layout::Template, Project)>,
}

impl KinettoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
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
            generation: 0,
            toast: None,
            title: String::new(),
            about: false,
            quick: None,
            presenting: None,
            export_request: None,
            problem: None,
            save_trouble: None,
            autosave_failed_at: None,
            told_silent: false,
            settings: settings::Settings::load(),
            scrub: None,
            timeline_drag: None,
            waveform: None,
            templates: kn_layout::Template::ALL.into_iter().map(|t| (t, kn_layout::template(t))).collect(),
        }
    }

    fn project(&self) -> &Project {
        self.session.project()
    }

    /// Identifies the open project's current content, for picture caches.
    fn content_stamp(&self) -> u64 {
        self.generation << 40 ^ self.session.revision()
    }

    fn slide_start(&self, index: usize) -> Tick {
        self.project().slides[..index].iter().fold(Tick::ZERO, |acc, s| acc + s.duration)
    }

    /// The moment shown while editing: a time picked on the timeline, or
    /// the current slide with every entrance finished.
    fn edit_time(&self) -> Tick {
        if let Some(t) = self.scrub.filter(|t| self.project().slide_at(*t).is_some_and(|(i, _)| i == self.slide)) {
            return t;
        }
        let slide = &self.project().slides[self.slide];
        self.slide_start(self.slide) + slide.rest_time()
    }

    fn set_layout(&mut self, layout: settings::Layout) {
        if self.settings.layout != layout {
            self.settings.layout = layout;
            self.settings.save();
        }
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
        self.scrub = None;
        self.stop_playback();
    }

    /// Apply a transition and play from just before the boundary.
    fn choose_transition(&mut self, index: usize, transition: kn_model::Transition) {
        if self.project().slides[index].transition != transition {
            self.run(Command::SetTransition { index, transition });
            self.session.seal();
        }
        self.select_slide(index - 1);
        self.play_from((self.slide_start(index) - Tick::from_millis(800)).max(Tick::ZERO));
    }

    fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast { message: message.into(), undo: false, again: None, at: Instant::now() });
    }

    /// A toast for an edit that can be taken back with one click.
    fn notify_undo(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast { message: message.into(), undo: true, again: None, at: Instant::now() });
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
        let revision = self.content_stamp();
        if self.soundtrack.as_ref().is_none_or(|(r, _)| *r != revision) {
            let pcm = kn_audio::mix(self.session.project(), &self.session.media, &mut self.audio_cache);
            self.soundtrack = Some((revision, pcm.map(std::sync::Arc::new)));
        }
        let track = self.soundtrack.as_ref().and_then(|(_, p)| p.clone());
        // Speakers may have been plugged in since start-up.
        if track.is_some() && self.player.is_none() {
            self.player = device::Player::new();
            if self.player.is_none() && !self.told_silent {
                self.told_silent = true;
                self.notify("No speakers or headphones were found, so the preview is silent. Videos you make still include the sound.");
            }
        }
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
            // Hold at the start until the first sound reaches the speakers,
            // rather than stepping back by the output latency.
            (Some(player), true) => player.position_secs().max(self.play_origin.as_secs_f64()),
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
    fn add_shape(&mut self, kind: kn_model::ShapeKind) {
        let (w, h) = self.project().aspect.canvas_size();
        let theme = kn_layout::theme(&self.project().theme);
        let n = self.project().slides[self.slide].elements.len() % 3;
        let mut shape = Element::shape(Rect::new(w / 2.0 - 200.0, h / 2.0 - 150.0, 400.0, 300.0), theme.accents[n]).with_role(kn_model::Role::Card(n as u8));
        if let kn_model::ElementKind::Shape { shape: geometry, .. } = &mut shape.kind { *geometry = kind; }
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
            return self.report(Problem::too_big(path, 100));
        }
        let name = path.file_name().map_or_else(|| "the picture".to_owned(), |n| format!("\u{201c}{}\u{201d}", n.to_string_lossy()));
        match std::fs::read(path) {
            Ok(bytes) => self.add_picture(bytes, &name),
            Err(err) => self.report(Problem::read(path, &err)),
        }
    }

    /// Adds a picture centred on the slide, as large as fits comfortably.
    fn add_picture(&mut self, bytes: Vec<u8>, name: &str) {
        let info = match self.session.media.import(bytes) {
            Ok(info) => info,
            Err(err) => return self.report(Problem::picture(name, &err)),
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
        // While the quick dialog is open, what's dropped goes into it.
        if let Some(quick) = self.quick.as_mut() {
            for file in files {
                match (file.path(), file.bytes()) {
                    (path, _) if !path.as_os_str().is_empty() => quick.add_file(ctx, path),
                    (_, Ok(bytes)) => quick.add_picture(ctx, bytes, "the dropped file"),
                    _ => {}
                }
            }
            return;
        }
        for file in files {
            let path = file.path();
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            if ext == storage::EXTENSION {
                self.request(Pending::Open(path.to_path_buf()));
            } else if import::IMPORT_EXTENSIONS.contains(&ext.as_str()) {
                self.request(Pending::Import(path.to_path_buf()));
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| kn_media::AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())) {
                self.add_dropped_sound(path);
            } else if !path.as_os_str().is_empty() {
                self.add_picture_file(path);
            } else {
                // Some platforms hand over bytes without a path.
                match file.bytes() {
                    Ok(bytes) => self.add_picture(bytes, "the dropped file"),
                    Err(_) => self.report(Problem::new("Couldn't read the dropped file", "Try saving it to a folder first, then drag it from there.")),
                }
            }
        }
    }

    /// A solid, rounded backing for the whole window. The window itself is
    /// transparent (for the rounded corners), so without this anything
    /// drawn partly see-through would let other windows show through.
    fn window_backing(&self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
        let radius = if maximized { 0 } else { theme::WINDOW_RADIUS };
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(ctx.content_rect(), radius, theme::palette(ctx).window);
    }

    /// A hairline around the window, as GNOME draws for client-side frames.
    fn window_outline(&self, ctx: &egui::Context) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
        if maximized {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("outline")));
        let stroke = egui::Stroke::new(1.0, theme::palette(ctx).border);
        painter.rect_stroke(ctx.content_rect().shrink(0.5), theme::WINDOW_RADIUS, stroke, egui::StrokeKind::Inside);
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
            Pending::Import(path) => self.import_file(&path),
            Pending::Quick(request) => self.quick_make(&request),
            Pending::Open(path) => match storage::load(&path) {
                Ok(bundle) => self.replace_session(Session::from_bundle(bundle, Some(path))),
                Err(err) => self.report(Problem::open(&path, &err)),
            },
        }
    }

    fn replace_session(&mut self, session: Session) {
        self.session = session;
        self.generation += 1;
        self.save_trouble = None;
        self.autosave_failed_at = None;
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
        self.save_reporting()
    }

    fn save_as(&mut self) -> bool {
        let Some(mut path) = file_dialog().set_file_name("My video.kinetto").save_file() else { return false };
        if path.extension().is_none() {
            path.set_extension(storage::EXTENSION);
        }
        // Keep the old file name if the new place doesn't work out.
        let previous = self.session.path.replace(path);
        let saved = self.save_reporting();
        if !saved {
            self.session.path = previous;
        }
        saved
    }

    /// A save the user asked for: failures open a dialog.
    fn save_reporting(&mut self) -> bool {
        match self.write_file() {
            Ok(()) => true,
            Err(err) => {
                let path = self.session.path.clone().unwrap_or_default();
                self.report(Problem::save(&path, &err));
                false
            }
        }
    }

    fn write_file(&mut self) -> Result<(), storage::StorageError> {
        let Some(path) = self.session.path.clone() else { return Ok(()) };
        let revision = self.session.revision();
        storage::save(self.session.project(), &self.session.media, &path)?;
        self.session.mark_saved(revision);
        self.persisted = revision;
        self.save_trouble = None;
        self.autosave_failed_at = None;
        if let Some(r) = &self.recovery {
            r.clear();
        }
        Ok(())
    }

    /// Named projects save themselves; untitled ones go to the recovery
    /// copy. Failures show a banner rather than interrupting, and are
    /// retried every few seconds.
    fn autosave(&mut self, force: bool) {
        let revision = self.session.revision();
        if revision == self.persisted {
            return;
        }
        let idle = self.session.idle_since_edit().unwrap_or_default();
        if !force && (idle < AUTOSAVE_DELAY || self.drag.is_some()) {
            return;
        }
        if !force && self.autosave_failed_at.is_some_and(|t| t.elapsed() < AUTOSAVE_RETRY) {
            return;
        }
        let failure = if let Some(path) = self.session.path.clone() {
            self.write_file().err().map(|err| {
                let reason = match &err {
                    storage::StorageError::Io(io) => problem::io_reason(io),
                    other => other.to_string(),
                };
                let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                format!("Changes can't be saved to \u{201c}{name}\u{201d}. {reason}")
            })
        } else if let Some(r) = &self.recovery {
            match r.write(self.session.project(), &self.session.media) {
                Ok(()) => {
                    self.persisted = revision;
                    None
                }
                Err(err) => {
                    tracing::warn!("recovery copy failed: {err}");
                    let reason = match &err {
                        storage::StorageError::Io(io) => problem::io_reason(io),
                        other => other.to_string(),
                    };
                    Some(format!("Kinetto can't keep a safety copy of this video. {reason} Save it to a file to keep it safe."))
                }
            }
        } else {
            None
        };
        if let Some(message) = failure {
            self.save_trouble = Some(message);
            self.autosave_failed_at = Some(Instant::now());
        } else {
            self.save_trouble = None;
            self.autosave_failed_at = None;
        }
    }

    /// Shows a failure of something the user just asked for.
    fn report(&mut self, problem: Problem) {
        tracing::info!("{}: {}", problem.title, problem.body);
        self.problem = Some(problem);
    }

    fn problem_dialog(&mut self, ctx: &egui::Context) {
        let Some(problem) = self.problem.clone() else { return };
        let buttons: Vec<(&str, adw::Kind)> = match problem.fix {
            Some(fix) => vec![("Close", adw::Kind::Normal), (fix.label(), adw::Kind::Suggested)],
            None => vec![("OK", adw::Kind::Suggested)],
        };
        let Some(choice) = adw::alert(ctx, "problem", &problem.title, &problem.body, &buttons) else { return };
        self.problem = None;
        match (problem.fix, choice) {
            (Some(Fix::SaveAs), 1) => {
                self.save_as();
            }
            (Some(Fix::ExportAgain(output)), 1) => self.export_request = Some(output),
            (Some(Fix::Import), 1) => self.pick_import(),
            (Some(Fix::Open), 1) => self.pick_open(),
            _ => {}
        }
    }

    /// When saving keeps failing, says so above the editor until it works.
    fn save_banner(&mut self, ui: &mut egui::Ui) {
        let Some(message) = self.save_trouble.clone() else { return };
        if adw::banner(ui, &message, &[("Save as…", adw::Kind::Suggested)]).is_some() {
            self.save_as();
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

    /// "Save changes?" before something replaces the open video.
    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending.clone() else { return };
        let choice = adw::alert(
            ctx,
            "unsaved",
            "Save changes?",
            "This video has changes that aren't saved to a file. If you don't save, they will be lost.",
            &[("Cancel", adw::Kind::Normal), ("Discard", adw::Kind::Destructive), ("Save", adw::Kind::Suggested)],
        );
        match choice {
            Some(2) => {
                self.pending = None;
                if self.save() {
                    self.perform(action);
                }
            }
            Some(1) => {
                self.pending = None;
                self.perform(action);
            }
            Some(_) => self.pending = None,
            None => {}
        }
    }

    /// Offers to bring back work from a run that closed without saving.
    fn restore_banner(&mut self, ui: &mut egui::Ui) {
        if self.restorable.is_none() {
            return;
        }
        let choice = adw::banner(
            ui,
            "Kinetto closed before your last video was saved.",
            &[("Discard", adw::Kind::Flat), ("Restore", adw::Kind::Suggested)],
        );
        match choice {
            Some(1) => {
                let bundle = self.restorable.take().expect("checked above");
                self.choosing_template = false;
                self.replace_session(Session::from_bundle(bundle, None));
            }
            Some(_) => {
                self.restorable = None;
                if let Some(r) = &self.recovery {
                    r.clear();
                }
            }
            None => {}
        }
    }

    fn toast(&mut self, ctx: &egui::Context) {
        let Some(toast) = &self.toast else { return };
        if toast.at.elapsed() > TOAST_TIME {
            self.toast = None;
            return;
        }
        let action = if toast.again.is_some() { Some("Shuffle") } else { toast.undo.then_some("Undo") };
        if adw::toast(ctx, &toast.message, action) {
            let again = self.toast.take().and_then(|t| t.again);
            if let Some(request) = again {
                self.request(Pending::Quick(request));
            } else if self.session.undo() {
                self.clamp_selection();
            }
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    /// Frames for the panels: rounded at the window's outer corners unless
    /// the window fills the screen.
    fn panel_frame(&self, ctx: &egui::Context, fill: egui::Color32, corners: [bool; 4]) -> egui::Frame {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
        let r = if maximized { 0 } else { theme::WINDOW_RADIUS };
        let [nw, ne, sw, se] = corners.map(|c| if c { r } else { 0 });
        egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw, ne, sw, se })
    }
}

/// A short message at the bottom of the window.
struct Toast {
    message: String,
    /// Offers Undo for the edit just made.
    undo: bool,
    /// Offers to make a quick video again with fresh random choices.
    again: Option<Box<quick::QuickRequest>>,
    at: Instant,
}

impl eframe::App for KinettoApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.presenting.is_some() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ui, |ui| self.present_view(ui, frame));
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::F5)) && !self.choosing_template {
            self.start_presenting(&ctx);
        }
        self.resize_edges(&ctx);
        self.shortcuts(&ctx);
        self.dropped_files(&ctx);
        self.advance_playback(&ctx);
        let p = theme::palette(&ctx);
        self.window_backing(&ctx);

        egui::Panel::top("header")
            .exact_size(header::HEIGHT)
            .frame(self.panel_frame(&ctx, p.header, [true, true, false, false]))
            .show(ui, |ui| self.header_bar(ui, frame));
        if self.restorable.is_some() {
            egui::Panel::top("banner").frame(egui::Frame::new().fill(p.window)).show_separator_line(false).show(ui, |ui| self.restore_banner(ui));
        }
        if self.save_trouble.is_some() && !self.choosing_template {
            egui::Panel::top("save-banner").frame(egui::Frame::new().fill(p.window)).show_separator_line(false).show(ui, |ui| self.save_banner(ui));
        }
        if self.choosing_template {
            egui::CentralPanel::default()
                .frame(self.panel_frame(&ctx, p.view, [false, false, true, true]))
                .show(ui, |ui| self.welcome(ui, frame));
        } else {
            let timeline = self.settings.layout == settings::Layout::Timeline;
            egui::Panel::right("inspector")
                .resizable(false)
                .exact_size(328.0)
                .frame(self.panel_frame(&ctx, p.window, [false, false, false, true]))
                .show(ui, |ui| self.inspector(ui));
            if timeline {
                egui::Panel::bottom("timeline")
                    .resizable(true)
                    .default_size(timeline::DEFAULT_HEIGHT)
                    .min_size(150.0)
                    .max_size(420.0)
                    .frame(self.panel_frame(&ctx, p.sidebar, [false, false, true, false]))
                    .show(ui, |ui| self.timeline(ui, frame));
            } else {
                egui::Panel::left("slides")
                    .resizable(false)
                    .exact_size(212.0)
                    .frame(self.panel_frame(&ctx, p.sidebar, [false, false, true, false]))
                    .show(ui, |ui| self.slide_strip(ui, frame));
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.view)).show(ui, |ui| self.stage(ui, frame));
        }
        self.window_outline(&ctx);

        self.drop_hint(&ctx);
        self.dialogs(&ctx);
        self.problem_dialog(&ctx);
        self.quick_dialog(&ctx);
        self.about_dialog(&ctx);
        self.export_dialog(&ctx);
        self.toast(&ctx);
        self.update_title(&ctx);
        self.autosave(false);
        if self.session.revision() != self.persisted {
            ctx.request_repaint_after(AUTOSAVE_DELAY);
        }
    }

    /// The window is transparent around its rounded corners.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    /// Closing never loses work: a take being recorded is kept, a running
    /// export is stopped cleanly (no half-written file), and unsaved edits
    /// are written to the file or the safety copy.
    fn on_exit(&mut self) {
        if self.recording.is_some() {
            self.finish_recording();
        }
        self.cancel_export_and_wait();
        self.autosave(true);
    }
}

pub(crate) const PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

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

#[cfg(test)]
mod screens {
    //! Renders the real window to PNGs for a visual check. Runs only when
    //! `KINETTO_SCREENS` names an output folder.

    use super::{KinettoApp, Problem, problem, storage};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn sidebar_animation_changes_only_the_following_slide_and_can_be_undone() {
        let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides.push(project.slides[0].clone());
        h.state_mut().session = kn_engine::Session::new(project);
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().choosing_template = false;
        h.state_mut().settings.layout = super::settings::Layout::Simple;
        h.state_mut().player = None;
        h.state_mut().told_silent = true;
        h.state_mut().selected = Some(1);
        h.run_steps(4);

        h.get_by_label("Animation between slides 1 and 2").click();
        h.run_steps(4);
        h.get_by_label("3D left").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
        assert_eq!(h.state().project().slides[0].transition, kn_model::Transition::Cut);
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
        assert!(h.state().playing);
        assert!(h.state().selected.is_none());
        h.state_mut().stop_playback();

        h.get_by_label("Animation between slides 2 and 3").click();
        h.run_steps(4);
        h.get_by_label("Wipe").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Wipe);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
        h.state_mut().stop_playback();
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
    }

    #[test]
    fn shape_gallery_inserts_selected_geometry_and_supports_undo() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = Harness::builder().with_size([900.0, 700.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().session = kn_engine::Session::new(kn_model::Project::sample());
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().choosing_template = false;
            h.state_mut().selected = None;
            h.run_steps(4);
            let before = h.state().project().slides[0].elements.len();
            h.get_by_label("Add text, a shape or a picture").click();
            h.run_steps(4);
            h.get_by_label("Star").click();
            h.run_steps(4);
            let elements = &h.state().project().slides[0].elements;
            assert_eq!(elements.len(), before + 1);
            assert!(matches!(elements.last().unwrap().kind, kn_model::ElementKind::Shape { shape: kn_model::ShapeKind::Star, .. }));
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().slides[0].elements.len(), before);
        }
    }

    #[test]
    fn timeline_switches_between_slides_with_different_item_counts() {
        let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides[1].elements.clear();
        h.state_mut().session = kn_engine::Session::new(project);
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().choosing_template = false;
        h.state_mut().settings.layout = super::settings::Layout::Timeline;
        h.run_steps(4);
        for (label, index) in [("Slide 2", 1), ("Slide 1", 0), ("Slide 2", 1)] {
            h.get_by_label(label).click();
            h.run_steps(4);
            assert_eq!(h.state().slide, index);
        }
    }

    #[test]
    fn render_screens() {
        let Some(dir) = std::env::var_os("KINETTO_SCREENS").map(std::path::PathBuf::from) else { return };
        // Keep the safety copy away from the real one in the user's data folder.
        let data = std::env::temp_dir().join(format!("kinetto-screens-{}", std::process::id()));
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let mut h = Harness::builder()
                .with_size([1280.0, 800.0])
                .with_theme(theme)
                .wgpu()
                .build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = Some(storage::Recovery::at(data.clone()));
            h.state_mut().restorable = None;
            // Not the user's saved preference: each screen sets its own.
            h.state_mut().settings = super::settings::Settings::default();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("welcome-{name}.png"))).unwrap();
            h.get_by_label("Main menu").click();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("menu-{name}.png"))).unwrap();
            h.key_press(egui::Key::Escape);
            h.run_steps(4);

            // With the restore banner showing, nothing inside the window may
            // be see-through (the window is transparent for its corners).
            h.state_mut().restorable = Some(storage::Bundle { project: kn_model::Project::sample(), media: kn_media::MediaLibrary::new() });
            h.run_steps(4);
            let img = h.render().unwrap();
            img.save(dir.join(format!("banner-{name}.png"))).unwrap();
            let (w, h_px) = img.dimensions();
            let corner = 14;
            let see_through = img
                .enumerate_pixels()
                .filter(|(x, y, _)| {
                    let near_x = *x < corner || *x >= w - corner;
                    let near_y = *y < corner || *y >= h_px - corner;
                    !(near_x && near_y)
                })
                .filter(|(_, _, px)| px.0[3] < 255)
                .count();
            assert_eq!(see_through, 0, "{see_through} see-through pixels in the {name} window");
            h.state_mut().restorable = None;

            h.get_by_label("Product promo").click();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("editor-{name}.png"))).unwrap();

            h.get_by_label("Animation between slides 1 and 2").click();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("transition-menu-{name}.png"))).unwrap();
            h.key_press(egui::Key::Escape);
            h.run_steps(4);

            h.state_mut().selected = Some(1);
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("item-{name}.png"))).unwrap();

            h.state_mut().settings.layout = super::settings::Layout::Timeline;
            h.state_mut().slide = 2;
            h.state_mut().selected = Some(3);
            // A real song, placed from 3 s to 14 s and starting 1.5 s in.
            let mut song = kn_audio::generate(kn_model::Mood::Upbeat, 48_000 * 10);
            kn_audio::loudness::normalise(&mut song.samples);
            let id = h.state_mut().session.media.import_audio(song.to_wav()).unwrap();
            let mut music = kn_model::Music::new(kn_model::MusicSource::File(id), 0.6);
            (music.start_ms, music.end_ms, music.offset_ms) = (3000, Some(14_000), 1500);
            h.state_mut().session.execute(kn_engine::Command::SetMusic { music: Some(music) }).unwrap();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("timeline-{name}.png"))).unwrap();
            h.state_mut().selected = None;
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("timeline-slide-{name}.png"))).unwrap();
            h.state_mut().slide = 1;
            h.state_mut().scrub = None;
            h.set_size(egui::vec2(1280.0, 960.0));
            h.run_steps(6);
            h.render().unwrap().save(dir.join(format!("timeline-music-{name}.png"))).unwrap();
            h.set_size(egui::vec2(1280.0, 800.0));
            h.state_mut().settings.layout = super::settings::Layout::Simple;

            let mut quick = super::quick::Quick::new("Our trip\nTwo weeks in Japan\n\nWhat we saw\n- Temples\n- Mountains\n- Ramen".into());
            for (pw, ph) in [(640, 480), (480, 640), (800, 800)] {
                quick.add_picture(&h.ctx, kn_media::test_png(pw, ph), "a test picture");
            }
            h.state_mut().quick = Some(quick);
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("paste-{name}.png"))).unwrap();
            h.set_size(egui::vec2(1280.0, 1100.0));
            h.state_mut().settings.quick.style.motion = kn_import::Pick::Random;
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("quick-tall-{name}.png"))).unwrap();
            h.set_size(egui::vec2(1280.0, 800.0));
            h.run_steps(2);
            h.get_by_label("Make video").click();
            h.run_steps(4);
            h.get_by_label("Discard").click();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("imported-{name}.png"))).unwrap();

            let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
            h.state_mut().save_trouble = Some(format!("Changes can't be saved to \u{201c}Talk.kinetto\u{201d}. {}", problem::io_reason(&full)));
            h.state_mut().report(Problem::save(std::path::Path::new("/home/me/Talk.kinetto"), &storage::StorageError::Io(full)));
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("problem-{name}.png"))).unwrap();
        }
        let _ = std::fs::remove_dir_all(data);
    }
}
