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
mod playback;
mod present;
mod problem;
mod quick;
mod recent;
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
/// How long a toast stays on screen.
const TOAST_TIME: Duration = Duration::from_secs(4);

/// Undo the last change.
const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
/// Redo what was undone.
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
/// Redo, the Windows way.
const REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
/// Save, asking for a name the first time.
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
/// Open a project file.
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
/// Start a new video.
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);

/// Something that would throw away unsaved work, waiting for the user's OK.
#[derive(Clone)]
pub(crate) enum Pending {
    /// Start a new video from the welcome page.
    New,
    /// Quit the app.
    Close,
    /// Open this project file.
    Open(PathBuf),
    /// Import this PowerPoint, Markdown or text file.
    Import(PathBuf),
    /// Make a quick video from these words and pictures.
    Quick(Box<quick::QuickRequest>),
}

pub struct KinettoApp {
    /// The open project, its undo history and file path.
    session: Session,
    /// Caches (decoded pictures, text layouts) shared by every drawing.
    render: RenderContext,
    /// The GPU slide renderer; `None` without a usable GPU.
    canvas: Option<VelloCanvas>,
    /// Where untitled work and crash copies are kept.
    recovery: Option<Recovery>,
    /// Work found from a previous run that closed without saving.
    restorable: Option<Bundle>,
    /// The "What are you making?" screen is showing.
    choosing_template: bool,
    /// An action waiting for the "Save changes?" answer.
    pending: Option<Pending>,
    /// The user agreed to close; the window closes this frame.
    close_confirmed: bool,
    /// The user chose Discard, so nothing is saved on the way out.
    discard_on_close: bool,

    /// Index of the slide being edited.
    slide: usize,
    /// Index of the selected item on that slide, if any.
    selected: Option<usize>,
    /// Preview playback: clock, speakers and soundtrack.
    playback: playback::Playback,
    /// Decoded sound files, shared by playback and waveforms.
    audio_cache: kn_audio::AudioCache,
    /// A voice take in progress, and the slide it is for.
    recording: Option<(usize, device::Recorder)>,
    /// An item being moved, resized or rotated on the stage.
    drag: Option<stage::Drag>,
    /// An export in progress, shown in a dialog.
    export: Option<export::ExportJob>,

    /// Last revision written to the file or the recovery copy.
    persisted: u64,
    /// Bumped whenever another project replaces the open one. Revisions
    /// restart at zero per project, so cached pictures key on both.
    generation: u64,
    /// The message at the bottom of the window, if any.
    toast: Option<Toast>,
    /// The window title last sent, so it is only sent when it changes.
    title: String,
    /// The About window is open.
    about: bool,
    /// The "Make a video in seconds" dialog, while it is open.
    quick: Option<quick::Quick>,
    /// Full-screen presenting, when on.
    presenting: Option<present::Present>,
    /// A failure of something the user asked for, shown as a dialog.
    problem: Option<Problem>,
    /// Automatic saving is failing: why, shown as a banner until it works.
    save_trouble: Option<String>,
    /// When automatic saving last failed, to space out retries.
    autosave_failed_at: Option<Instant>,
    /// The user has been told once that there's no sound output.
    told_silent: bool,
    /// Preferences kept between runs (layout, recent files, quick choices).
    settings: settings::Settings,
    /// A moment picked on the timeline to look at while editing; `None`
    /// shows the current slide at rest.
    scrub: Option<Tick>,
    /// Timeline drags in progress.
    timeline_drag: Option<timeline::TimelineDrag>,
    /// Cached source waveforms; generated music is analysed off the UI thread.
    waveform: std::collections::HashMap<String, timeline::Waveform>,
    /// An export chosen from a menu, started once the menu has closed.
    export_request: Option<export::Output>,
    /// The welcome page's template filter; `None` shows every category.
    template_category: Option<kn_layout::Category>,
    /// Starter projects shown on the welcome page.
    templates: Vec<(kn_layout::Template, Project)>,
    /// Pictures used by the starter templates.
    template_media: kn_media::MediaLibrary,
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
            close_confirmed: false,
            discard_on_close: false,
            slide: 0,
            selected: None,
            playback: playback::Playback::new(),
            audio_cache: kn_audio::AudioCache::default(),
            recording: None,
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
            waveform: Default::default(),
            template_category: None,
            templates: kn_layout::Template::ALL.into_iter().map(|t| (t, kn_layout::template(t))).collect(),
            template_media: kn_media::sample_library(),
        }
    }

    /// Open a file passed by the desktop file manager at application startup.
    pub fn new_with_project(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut app = Self::new(cc);
        if let Some(path) = path { app.open_startup_project(path); }
        app
    }

    /// Opens `path` at start-up, skipping the welcome page; a failure is
    /// reported and the welcome page stays.
    fn open_startup_project(&mut self, path: PathBuf) {
        match storage::load(&path) {
            Ok(bundle) => {
                self.replace_session(Session::from_bundle(bundle, Some(path)));
                self.choosing_template = false;
                self.restorable = None;
            }
            Err(err) => self.report(Problem::open(&path, &err)),
        }
    }

    /// The open project.
    fn project(&self) -> &Project {
        self.session.project()
    }

    /// Identifies the open project's current content, for picture caches.
    fn content_stamp(&self) -> u64 {
        self.generation << 40 ^ self.session.revision()
    }

    /// When slide `index` starts in the video.
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

    /// Switches between the Simple and Timeline layouts and remembers it.
    fn set_layout(&mut self, layout: settings::Layout) {
        if self.settings.layout != layout {
            self.settings.layout = layout;
            self.settings.save();
        }
    }

    /// Runs an editing command; a refusal is shown as a toast.
    fn run(&mut self, command: Command) {
        if let Err(err) = self.session.execute(command) {
            self.notify(err.to_string());
        }
        self.clamp_selection();
    }

    /// Keeps the slide and item indices valid after slides or items go away.
    fn clamp_selection(&mut self) {
        self.slide = self.slide.min(self.project().slides.len() - 1);
        let count = self.project().slides[self.slide].elements.len();
        self.selected = self.selected.filter(|&i| i < count);
    }

    /// Shows slide `index` at rest, with nothing selected and playback stopped.
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

    /// Shows a short message at the bottom of the window.
    fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast { message: message.into(), undo: false, again: None, at: Instant::now() });
    }

    /// A toast for an edit that can be taken back with one click.
    fn notify_undo(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast { message: message.into(), undo: true, again: None, at: Instant::now() });
    }

    /// Play from the current slide, or stop (the Space key and play button).
    fn toggle_play(&mut self) {
        if self.playback.playing {
            self.stop_playback();
        } else {
            self.play_from(self.slide_start(self.slide));
        }
    }

    /// Plays from `origin`, with sound when the project has any.
    fn play_from(&mut self, origin: Tick) {
        self.selected = None;
        let stamp = self.content_stamp();
        let (project, media, cache) = (self.session.project(), &self.session.media, &mut self.audio_cache);
        let track = self.playback.soundtrack(stamp, || kn_audio::mix(project, media, cache));
        if matches!(self.playback.start(origin, track), playback::Started::NoSpeakers) && !self.told_silent {
            self.told_silent = true;
            self.notify("No speakers or headphones were found, so the preview is silent. Videos you make still include the sound.");
        }
    }

    /// Stops playback and its sound.
    fn stop_playback(&mut self) {
        self.playback.stop();
    }

    /// Moves playback on by one frame; follows into the next slide, and stops
    /// at the end of the video.
    fn advance_playback(&mut self, ctx: &egui::Context) {
        if !self.playback.playing {
            return;
        }
        let now = self.playback.tick();
        match self.project().slide_at(now) {
            Some((index, _)) => self.slide = index,
            None => self.stop_playback(),
        }
        ctx.request_repaint();
    }

    /// Adds a text box across the middle of the slide, in a colour that
    /// reads on its background.
    fn add_text(&mut self) {
        let (_, h) = self.project().aspect.canvas_size();
        let bg = self.project().slides[self.slide].background;
        let element = Element::text("Your text", Rect::new(360.0, h / 2.0 - 60.0, 1200.0, 120.0), 80.0, true, readable_on(bg));
        self.add_element(element);
    }

    /// New shapes take the look's next accent colour, and follow it later.
    fn add_shape(&mut self, kind: kn_model::ShapeKind) {
        let (w, h) = self.project().aspect.canvas_size();
        let theme = kn_layout::theme(self.project().slides[self.slide].theme_name(self.project()));
        let n = self.project().slides[self.slide].elements.len() % 3;
        let height = match kind {
            kn_model::ShapeKind::Square | kn_model::ShapeKind::Diamond => 400.0,
            kn_model::ShapeKind::Semicircle | kn_model::ShapeKind::Capsule => 200.0,
            _ => 300.0,
        };
        let mut shape = Element::shape(Rect::new(w / 2.0 - 200.0, h / 2.0 - height / 2.0, 400.0, height), theme.accents[n]).with_role(kn_model::Role::Card(n as u8));
        if let kn_model::ElementKind::Shape { shape: geometry, .. } = &mut shape.kind { *geometry = kind; }
        self.add_element(shape);
    }

    /// Asks for picture files and adds each to the current slide.
    fn pick_pictures(&mut self) {
        let files = rfd::FileDialog::new().add_filter("Pictures", PICTURE_EXTENSIONS).pick_files().unwrap_or_default();
        for path in files {
            self.add_picture_file(&path);
        }
    }

    /// Reads a picture file and adds it, refusing files over the size limit.
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
            } else if kn_media::AUDIO_EXTENSIONS.contains(&ext.as_str()) {
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
        let radius = if is_maximized(ctx) { 0 } else { theme::WINDOW_RADIUS };
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(ctx.content_rect(), radius, theme::palette(ctx).window);
    }

    /// A hairline around the window, as GNOME draws for client-side frames.
    fn window_outline(&self, ctx: &egui::Context) {
        if is_maximized(ctx) {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("outline")));
        let stroke = egui::Stroke::new(1.0, theme::palette(ctx).border);
        painter.rect_stroke(ctx.content_rect().shrink(0.5), theme::WINDOW_RADIUS, stroke, egui::StrokeKind::Inside);
    }

    /// Darkens the window with a hint while files are dragged over it.
    fn drop_hint(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
        painter.rect_filled(screen, 0, egui::Color32::from_black_alpha(160));
        painter.text(screen.center(), egui::Align2::CENTER_CENTER, "Drop pictures or sounds to add them", egui::FontId::proportional(28.0), egui::Color32::WHITE);
    }

    /// Adds `element` to the current slide as one undo step, and selects it.
    fn add_element(&mut self, element: Element) {
        self.stop_playback();
        self.run(Command::AddElement { slide: self.slide, element });
        self.session.seal();
        self.selected = Some(self.project().slides[self.slide].elements.len() - 1);
    }

    // ----- files -----

    /// Closing now would lose work: there are unsaved edits, or the video
    /// has never been saved to a file.
    fn has_unsaved_project(&self) -> bool {
        self.session.is_dirty() || (!self.choosing_template && self.session.path.is_none())
    }

    /// The window's close button: asks first when work would be lost.
    fn request_close(&mut self) {
        if self.close_confirmed { return; }
        if self.recording.is_some() { self.finish_recording(); }
        self.stop_playback();
        if self.has_unsaved_project() {
            self.pending = Some(Pending::Close);
            // Keep a safety copy while the save decision is pending.
            if self.session.path.is_none() { self.autosave(true); }
        } else {
            self.close_confirmed = true;
        }
    }

    /// Does `action` now, or first asks "Save changes?" if work would be lost.
    fn request(&mut self, action: Pending) {
        if self.has_unsaved_project() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    /// Carries out an action, once any unsaved work has been dealt with.
    fn perform(&mut self, action: Pending) {
        match action {
            Pending::New => {
                self.replace_session(Session::new(Project::sample()));
                self.choosing_template = true;
                self.persisted = self.session.revision();
            },
            Pending::Close => self.close_confirmed = true,
            Pending::Import(path) => self.import_file(&path),
            Pending::Quick(request) => self.quick_make(&request),
            Pending::Open(path) => match storage::load(&path) {
                Ok(bundle) => {
                    self.replace_session(Session::from_bundle(bundle, Some(path)));
                    self.choosing_template = false;
                    self.restorable = None;
                },
                Err(err) => self.report(Problem::open(&path, &err)),
            },
        }
    }

    /// Swaps in another project, clearing everything tied to the old one
    /// (selection, waveforms, playback, the safety copy).
    fn replace_session(&mut self, session: Session) {
        self.session = session;
        self.waveform.clear();
        if let Some(path) = self.session.path.clone() { self.remember_project(&path); }
        self.generation += 1;
        self.save_trouble = None;
        self.autosave_failed_at = None;
        self.persisted = if self.session.path.is_none() { u64::MAX } else { self.session.revision() };
        self.slide = 0;
        self.selected = None;
        self.stop_playback();
        if let Some(r) = &self.recovery {
            r.clear();
        }
    }

    /// Asks for a project file and opens it.
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

    /// Asks for a file name and saves there. Keeps the old name if the save
    /// fails or is cancelled. Returns whether it saved.
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

    /// Writes the project to its file and marks it saved; does nothing for
    /// an untitled project.
    fn write_file(&mut self) -> Result<(), storage::StorageError> {
        let Some(path) = self.session.path.clone() else { return Ok(()) };
        let revision = self.session.revision();
        storage::save(self.session.project(), &self.session.media, &path)?;
        self.session.mark_saved(revision);
        self.remember_project(&path);
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
        let idle = self.session.idle_since_edit().unwrap_or(Duration::MAX);
        if !force && (idle < AUTOSAVE_DELAY || self.drag.is_some()) {
            return;
        }
        if !force && self.autosave_failed_at.is_some_and(|t| t.elapsed() < AUTOSAVE_RETRY) {
            return;
        }
        let failure = if let Some(path) = self.session.path.clone() {
            self.write_file().err().map(|err| {
                let reason = storage_reason(&err);
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
                    let reason = storage_reason(&err);
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

    /// Shows the current problem, if any, with its fix button.
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

    /// Keyboard shortcuts: undo/redo (not while typing), save, open, new and
    /// Space to play.
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

    /// Keeps the window title as "Name • — Kinetto", the dot marking unsaved edits.
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
        let closing = matches!(action, Pending::Close);
        let choice = adw::alert(
            ctx,
            "unsaved",
            if closing { "Save before closing?" } else { "Save changes?" },
            if closing { "Save this video to a project file so you can open it again. Discard closes the app without saving." } else { "This video has changes that aren't saved to a file. If you don't save, they will be lost." },
            &[("Cancel", adw::Kind::Normal), ("Discard", adw::Kind::Destructive), (if closing { "Save and close" } else { "Save" }, adw::Kind::Suggested)],
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
                if closing {
                    self.discard_on_close = true;
                    if let Some(recovery) = &self.recovery { recovery.clear(); }
                }
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

    /// Shows the toast until it times out; its button undoes or reshuffles.
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
        let r = if is_maximized(ctx) { 0 } else { theme::WINDOW_RADIUS };
        let [nw, ne, sw, se] = corners.map(|c| if c { r } else { 0 });
        egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw, ne, sw, se })
    }
}

/// A short message at the bottom of the window.
struct Toast {
    /// What to say.
    message: String,
    /// Offers Undo for the edit just made.
    undo: bool,
    /// Offers to make a quick video again with fresh random choices.
    again: Option<Box<quick::QuickRequest>>,
    /// When it appeared, for timing it out.
    at: Instant,
}

impl eframe::App for KinettoApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.close_confirmed {
            if self.presenting.is_some() { self.stop_presenting(ctx); }
            self.request_close();
            if !self.close_confirmed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.request_repaint();
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.presenting.is_some() {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ui, |ui| self.present_view(ui, frame));
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::F5)) && !self.choosing_template && !dialog_open(&ctx) {
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
        self.problem_dialog(&ctx);
        self.quick_dialog(&ctx);
        self.about_dialog(&ctx);
        self.export_dialog(&ctx);
        self.dialogs(&ctx);
        self.toast(&ctx);
        self.update_title(&ctx);
        if !self.discard_on_close && !matches!(self.pending, Some(Pending::Close)) { self.autosave(false); }
        if self.close_confirmed { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
        if self.session.revision() != self.persisted {
            ctx.request_repaint_after(AUTOSAVE_DELAY);
        }
    }

    /// The window is transparent around its rounded corners.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    /// Finish shutdown cleanly and flush saves unless the user discarded them.
    fn on_exit(&mut self) {
        if self.recording.is_some() {
            self.finish_recording();
        }
        self.cancel_export_and_wait();
        if !self.discard_on_close { self.autosave(true); }
    }
}

/// Picture types that can be added.
pub(crate) const PICTURE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// The window fills the screen, so its corners are square.
fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false))
}

/// A modal dialog is showing, so F5 waits until it closes.
fn dialog_open(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.top_modal_layer().is_some())
}

/// Why a file couldn't be written, in words for people.
fn storage_reason(err: &storage::StorageError) -> String {
    match err {
        storage::StorageError::Io(io) => problem::io_reason(io),
        other => other.to_string(),
    }
}

/// A file dialog filtered to Kinetto project files.
fn file_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Kinetto video", &[storage::EXTENSION])
}

/// Black or white, whichever reads better on `bg`.
fn readable_on(bg: Color) -> Color {
    let luma = 0.299 * f64::from(bg.r) + 0.587 * f64::from(bg.g) + 0.114 * f64::from(bg.b);
    if luma > 150.0 { Color::rgb(24, 24, 32) } else { Color::rgb(250, 247, 242) }
}

/// Our colour as an egui colour.
fn to_egui(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// An egui colour as our colour.
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

    /// Shows `session` in the editor, away from the user's real safety copy
    /// and settings; `layout` picks Simple or Timeline when given.
    fn open_editor(h: &mut Harness<'_, KinettoApp>, session: kn_engine::Session, layout: Option<super::settings::Layout>) {
        let app = h.state_mut();
        app.session = session;
        app.recovery = None;
        app.restorable = None;
        app.choosing_template = false;
        if let Some(layout) = layout {
            app.settings.layout = layout;
        }
    }

    #[test]
    fn soundtrack_selection_and_placement_follow_slide_and_scrubbed_playhead() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = Harness::builder().with_size([1280.0, 1100.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
            h.state_mut().slide = 1;
            h.state_mut().selected = None;
            h.state_mut().choose_soundtrack(Some(kn_model::MusicSource::Mood(kn_model::Mood::Calm)));
            assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
            assert_eq!(h.state().project().music.as_ref().unwrap().offset_ms, 0);
            h.state_mut().scrub = Some(kn_time::Tick::from_millis(4321));
            h.run_steps(4);
            h.get_by_label("Start at playhead").scroll_to_me(); h.run_steps(4);
            h.get_by_label("Start at playhead").click(); h.run_steps(4);
            assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 4321);
            h.get_by_label("From video start").click(); h.run_steps(4);
            assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 0);
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 4321);
            h.state_mut().choose_soundtrack(Some(kn_model::MusicSource::Mood(kn_model::Mood::Calm)));
            assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
        }
    }

    #[test]
    fn timeline_music_volume_and_removal_support_undo_and_show_audio_peaks() {
        let screenshots = std::env::var_os("KINETTO_AUDIO_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1440.0, 1000.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            let mut media = kn_media::MediaLibrary::new();
            let pcm = kn_audio::generate(kn_model::Mood::Upbeat, kn_audio::RATE as usize * 3);
            let id = media.import_audio(pcm.to_wav()).unwrap();
            let mut project = kn_model::Project::sample();
            project.slides[0].elements.truncate(1);
            project.music = Some(kn_model::Music::new(kn_model::MusicSource::File(id.clone()), 0.6));
            project.slides[0].voice = Some(id);
            h.state_mut().session = kn_engine::Session::from_bundle(storage::Bundle { project: project.clone(), media }, None);
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().choosing_template = false;
            h.state_mut().settings.layout = super::settings::Layout::Timeline;
            h.state_mut().settings.timeline_zoom = Some(100.0);
            h.run_steps(4);
            assert_eq!(h.state().waveform.len(), 1, "Music and voice share the source peak cache");
            h.get_by_role_and_label(egui::Role::Slider, "Timeline music volume").set_value_accesskit(25.0);
            h.run_steps(4);
            assert_eq!(h.state().project().music.as_ref().unwrap().volume, 0.25);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("audio-timeline-{name}.png"))).unwrap(); }
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project(), &project);
            h.run_steps(4);
            h.get_by_label("Remove music").click();
            h.run_steps(4);
            assert!(h.state().project().music.is_none());
            assert!(h.state().project().slides[0].voice.is_some());
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project(), &project);
        }
    }

    #[test]
    fn full_length_music_drags_freely_and_undoes_in_one_step() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = Harness::builder().with_size([1280.0, 900.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
            let mut project = kn_model::Project::sample();
            project.music = Some(kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Calm), 0.4));
            open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Timeline));
            h.state_mut().settings.timeline_zoom = Some(60.0);
            h.run_steps(4);
            let origin = h.get_by_label("Move music clip").rect().center();
            h.drag_at(origin); h.run_steps(2);
            let destination = origin + egui::vec2(140.7, 0.0);
            h.hover_at(destination); h.run_steps(4);
            h.drop_at(destination); h.run_steps(4);
            let music = h.state().project().music.as_ref().unwrap();
            assert!((2343..=2346).contains(&music.start_ms), "{music:?}");
            assert_eq!(music.end_ms, None);
            assert_eq!(music.offset_ms, 0);
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project(), &project);
            assert!(h.state_mut().session.redo());
            assert!((2343..=2346).contains(&h.state().project().music.as_ref().unwrap().start_ms));
        }
    }

    #[test]
    fn trimmed_music_restores_length_when_dragging_back_and_shift_snaps() {
        let mut h = Harness::builder().with_size([1280.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        let mut music = kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Calm), 0.4);
        music.end_ms = Some(4000); music.offset_ms = 700;
        project.music = Some(music);
        open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Timeline));
        h.state_mut().settings.timeline_zoom = Some(60.0);
        h.run_steps(4);
        let origin = h.get_by_label("Move music clip").rect().center();
        h.drag_at(origin); h.run_steps(2);
        h.hover_at(origin + egui::vec2(360.0, 0.0)); h.run_steps(4);
        let m = h.state().project().music.as_ref().unwrap();
        assert_eq!((m.start_ms, m.end_ms), (6000, Some(7000)));
        let back = origin + egui::vec2(75.0, 0.0);
        h.hover_at(back); h.run_steps(4);
        let m = h.state().project().music.as_ref().unwrap();
        assert_eq!((m.start_ms, m.end_ms, m.offset_ms), (1250, Some(5250), 700));
        h.drop_at(back); h.run_steps(4);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project);
        h.run_steps(4);
        let origin = h.get_by_label("Move music clip").rect().center();
        h.drag_at(origin); h.run_steps(2);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
        let near_boundary = origin + egui::vec2(176.7, 0.0);
        h.hover_at(near_boundary); h.run_steps(4);
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
        h.drop_at(near_boundary); h.run_steps(4);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project);
    }

    #[test]
    fn ruler_click_and_outside_release_never_leave_scrubbing_attached() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = Harness::builder().with_size([1280.0, 900.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Timeline));
            h.run_steps(4);
            let ruler = h.get_by_label("Timeline ruler").rect();
            let at = egui::pos2(ruler.left() + 100.0, ruler.center().y);
            h.drag_at(at); h.run_steps(2); h.drop_at(at); h.run_steps(4);
            assert!(h.state().timeline_drag.is_none());
            let selected = h.state().scrub;
            assert!(selected.is_some());
            h.hover_at(at + egui::vec2(100.0, 0.0)); h.run_steps(4);
            assert_eq!(h.state().scrub, selected);
            h.drag_at(at); h.run_steps(2);
            h.hover_at(at + egui::vec2(80.0, 0.0)); h.run_steps(4);
            assert!(matches!(h.state().timeline_drag, Some(super::timeline::TimelineDrag::Scrub)));
            h.drop_at(egui::pos2(20.0, 20.0)); h.run_steps(4);
            assert!(h.state().timeline_drag.is_none());
            let released = h.state().scrub;
            h.hover_at(at + egui::vec2(180.0, 0.0)); h.run_steps(4);
            assert_eq!(h.state().scrub, released);
            h.drag_at(at); h.run_steps(2);
            h.hover_at(at + egui::vec2(90.0, 0.0)); h.run_steps(4);
            h.event(egui::Event::WindowFocused(false)); h.run_steps(4);
            assert!(h.state().timeline_drag.is_none());
        }
    }

    #[test]
    fn playback_pages_the_timeline_without_continuously_chasing_the_head() {
        let mut h = Harness::builder().with_size([1280.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides = vec![project.slides[0].clone(); 30];
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Timeline));
        h.state_mut().settings.timeline_zoom = Some(60.0);
        h.run_steps(4);
        let initial = h.get_by_label("Timeline ruler").rect().left();
        h.state_mut().playback.playing = true;
        h.state_mut().playback.audio_clock = false;
        h.state_mut().playback.origin = kn_time::Tick::from_secs(25);
        h.state_mut().playback.started = std::time::Instant::now();
        h.run_steps(8);
        let paged = h.get_by_label("Timeline ruler").rect().left();
        assert!(paged < initial - 500.0, "timeline did not page: {initial} -> {paged}");
        h.run_steps(8);
        let settled = h.get_by_label("Timeline ruler").rect().left();
        assert!((settled - paged).abs() < 2.0, "timeline kept chasing the head");
        h.state_mut().playback.origin = kn_time::Tick::ZERO;
        h.state_mut().playback.started = std::time::Instant::now();
        h.run_steps(8);
        let restarted = h.get_by_label("Timeline ruler").rect().left();
        assert!((restarted - initial).abs() < 2.0, "restart did not return to the first page");
        h.state_mut().stop_playback();
    }

    #[test]
    fn desktop_project_open_loads_the_file_and_reports_missing_files() {
        let dir = std::env::temp_dir().join(format!("kinetto-desktop-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("My video with spaces.kinetto");
        let project = kn_layout::template(kn_layout::Template::Travel);
        storage::save(&project, &kn_media::sample_library(), &path).unwrap();
        let mut h = Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().open_startup_project(path.clone());
        assert_eq!(h.state().project(), &project);
        assert_eq!(h.state().session.path.as_ref(), Some(&path));
        assert_eq!(h.state().settings.recent_projects.first(), Some(&path.canonicalize().unwrap()));
        assert!(!h.state().choosing_template);
        assert!(!h.state().session.is_dirty());
        h.state_mut().open_startup_project(dir.join("missing.kinetto"));
        assert!(h.state().problem.is_some());
        assert_eq!(h.state().project(), &project);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recent_project_row_opens_the_editor_from_welcome() {
        let dir = std::env::temp_dir().join(format!("kinetto-recent-row-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("My recent video with a longer project name.kinetto");
        let project = kn_layout::template(kn_layout::Template::Travel);
        storage::save(&project, &kn_media::sample_library(), &path).unwrap();
        let path = path.canonicalize().unwrap();
        let screenshots = std::env::var_os("KINETTO_WELCOME_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1440.0, 960.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().settings = super::settings::Settings::default();
            h.state_mut().settings.recent_projects = vec![path.clone()];
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-recent-{name}.png"))).unwrap(); }
            h.set_size(egui::vec2(760.0, 850.0));
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-narrow-{name}.png"))).unwrap(); }
            h.get_by_label("Open My recent video with a longer project name").click();
            h.run_steps(4);
            assert!(!h.state().choosing_template);
            assert!(h.state().pending.is_none());
            assert_eq!(h.state().project(), &project);
            assert_eq!(h.state().session.path.as_ref(), Some(&path));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recent_project_failures_preserve_the_open_project_and_history() {
        let dir = std::env::temp_dir().join(format!("kinetto-recent-errors-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.kinetto");
        std::fs::write(&broken, b"not a project").unwrap();
        let missing = dir.join("missing.kinetto");
        let mut h = Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().settings.recent_projects = vec![missing.clone(), broken.clone()];
        let before = h.state().project().clone();
        for path in [missing, broken] {
            h.state_mut().perform(super::Pending::Open(path));
            assert!(h.state().problem.is_some());
            assert_eq!(h.state().project(), &before);
            assert_eq!(h.state().settings.recent_projects.len(), 2);
        }
        h.state_mut().problem = None;
        h.run_steps(4);
        assert!(h.query_by_label("File moved or missing").is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sidebar_animation_changes_only_the_following_slide_and_can_be_undone() {
        let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides.push(project.slides[0].clone());
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
        h.state_mut().playback.player = None;
        h.state_mut().told_silent = true;
        h.state_mut().selected = Some(1);
        h.run_steps(4);

        h.get_by_label("Animation between slides 1 and 2").click();
        h.run_steps(4);
        h.get_by_label("3D left").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("3D left").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
        assert_eq!(h.state().project().slides[0].transition, kn_model::Transition::Cut);
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
        assert!(h.state().playback.playing);
        assert!(h.state().selected.is_none());
        h.state_mut().stop_playback();

        h.get_by_label("Animation between slides 2 and 3").click();
        h.run_steps(4);
        h.get_by_label("Wipe").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Wipe").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Wipe);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
        h.state_mut().stop_playback();
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
        assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
        h.get_by_label("Animation between slides 2 and 3").click();
        h.run_steps(4);
        h.get_by_label("Spin").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Spin").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Spin);
        h.state_mut().stop_playback();
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
    }

    #[test]
    fn closing_an_untitled_video_requires_a_decision_and_keeps_a_safety_copy() {
        let directory = std::env::temp_dir().join(format!("kinetto-close-untitled-{}",std::process::id()));
        let screenshots = std::env::var_os("KINETTO_CLOSE_SCREENS").map(std::path::PathBuf::from);
        let builder = Harness::builder().with_size([1100.0,800.0]);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        let bundle = storage::Bundle { project:kn_layout::template(kn_layout::Template::Promo),media:kn_media::sample_library() };
        h.state_mut().replace_session(kn_engine::Session::from_bundle(bundle,None));
        h.state_mut().recovery = Some(storage::Recovery::at(directory.clone()));
        h.state_mut().choosing_template = false;
        assert!(!h.state().session.is_dirty());
        h.run_steps(4);
        let recovery = storage::Recovery::at(directory.clone()).pending().unwrap();
        for id in recovery.project.media_ids() { assert!(recovery.media.get(id).is_some()); }
        h.get_by_label("Close").click();
        h.run_steps(4);
        assert!(matches!(h.state().pending,Some(super::Pending::Close)));
        assert!(!h.state().close_confirmed);
        if let Some(directory) = &screenshots {
            std::fs::create_dir_all(directory).unwrap();
            h.render().unwrap().save(directory.join("save-before-closing.png")).unwrap();
        }
        h.get_by_label("Cancel").click();
        h.run_steps(4);
        assert!(h.state().pending.is_none());
        assert!(!h.state().close_confirmed);
        h.input_mut().viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
        h.run_steps(1);
        assert!(h.output().viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
        h.input_mut().viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.clear();
        h.run_steps(4);
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(h.state().pending.is_none());
        assert!(!h.state().close_confirmed);
        h.get_by_label("Close").click();
        h.run_steps(4);
        h.get_by_label("Discard").click();
        h.run_steps(4);
        assert!(h.state().close_confirmed && h.state().discard_on_close);
        eframe::App::on_exit(h.state_mut());
        assert!(storage::Recovery::at(directory.clone()).pending().is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn save_and_close_waits_for_success_and_discard_preserves_the_saved_file() {
        let directory = std::env::temp_dir().join(format!("kinetto-close-named-{}",std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for (case,save,failure) in [("save",true,false),("discard",false,false),("failure",true,true)] {
            let path = directory.join(format!("{case}.kinetto"));
            let mut h = Harness::builder().with_size([1100.0,800.0]).build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().session = kn_engine::Session::new(kn_model::Project::sample());
            h.state_mut().choosing_template = false;
            h.state_mut().session.path = Some(path.clone());
            h.state_mut().write_file().unwrap();
            let original = h.state().project().clone();
            h.state_mut().session.execute(kn_engine::Command::SetDuration { index:0,millis:8000 }).unwrap();
            if failure { h.state_mut().session.path = Some(directory.join("missing").join("video.kinetto")); }
            h.state_mut().request_close();
            h.run_steps(4);
            assert!(!h.state().close_confirmed);
            h.get_by_label(if save { "Save and close" } else { "Discard" }).click();
            h.run_steps(4);
            assert_eq!(h.state().close_confirmed,!failure);
            if failure {
                assert!(h.state().problem.is_some());
                assert!(h.state().session.is_dirty());
            } else {
                eframe::App::on_exit(h.state_mut());
                let saved = storage::load(&path).unwrap();
                assert_eq!(saved.project.slides[0].duration,if save { kn_time::Tick::from_millis(8000) } else { original.slides[0].duration });
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn picture_slides_render_in_timeline() {
        let Some(dir) = std::env::var_os("KINETTO_PICTURE_REVIEW").map(std::path::PathBuf::from) else { return };
        let mut h = Harness::builder().with_size([1280.0,800.0]).wgpu().build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.run_steps(5);
        let mut media = kn_media::sample_library();
        let mut outline = kn_import::outline_from_text("example\n\nexample\n1\n\nexample\n2",&mut media);
        for i in 0..12 { outline.parts.push(kn_import::Part::Picture { media:kn_media::sample_id(i%3),width:1600,height:1200,caption:String::new() }); }
        let bundle = storage::Bundle { project:kn_import::build(&outline,kn_geom::Aspect::Wide16x9,kn_layout::theme("Forest")).unwrap(),media };
        h.state_mut().replace_session(kn_engine::Session::from_bundle(bundle,None));
        h.state_mut().choosing_template = false;
        h.state_mut().settings.layout = super::settings::Layout::Timeline;
        h.state_mut().slide = 4;
        let time = h.state().slide_start(4)+kn_time::Tick::from_millis(600);
        h.state_mut().scrub = Some(time);
        h.run_steps(5);
        // Scrub over an entrance's background-only frame, then return to the picture.
        h.render().unwrap();
        h.state_mut().slide = 0;
        h.state_mut().scrub = Some(kn_time::Tick::ZERO);
        h.run_steps(4);
        h.render().unwrap();
        h.state_mut().slide = 4;
        h.state_mut().scrub = Some(time);
        h.run_steps(4);
        std::fs::create_dir_all(&dir).unwrap();
        let image = h.render().unwrap();
        let bg = h.state().project().slides[4].background;
        let visible = (180..750).flat_map(|x| (180..400).map(move |y| (x,y))).filter(|&(x,y)| {
            let pixel = image.get_pixel(x,y);
            pixel[0].abs_diff(bg.r)>12 || pixel[1].abs_diff(bg.g)>12 || pixel[2].abs_diff(bg.b)>12
        }).count();
        assert!(visible > 20000,"picture vanished after scrubbing: {visible} visible pixels");
        image.save(dir.join("timeline-picture.png")).unwrap();
    }

    #[test]
    fn quick_creation_and_templates_include_their_media_and_sound() {
        let screenshots = std::env::var_os("KINETTO_CREATION_SCREENS").map(std::path::PathBuf::from);
        for (theme,name) in [(egui::Theme::Light,"light"),(egui::Theme::Dark,"dark")] {
            let builder = Harness::builder().with_size([1280.0,960.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().settings = super::settings::Settings::default();
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-{name}.png"))).unwrap(); }
            h.get_by_label("Try a sample").click();
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("quick-{name}.png"))).unwrap(); }
            h.get_by_label("Add sample: Studio garden").click();
            h.run_steps(4);
            h.set_size(egui::vec2(760.0,850.0));
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("quick-narrow-{name}.png"))).unwrap(); }
            h.get_by_label("Create video").click();
            h.run_steps(4);
            assert!(!h.state().choosing_template);
            let project = h.state().project();
            assert!(project.music.is_some());
            assert!(project.media_ids().count() >= 2);
            for id in project.media_ids() { assert!(h.state().session.media.decode(id).is_ok()); }
            h.set_size(egui::vec2(1280.0,960.0));
            h.state_mut().choosing_template = true;
            h.run_steps(4);
            h.get_by_role_and_label(egui::Role::RadioButton, "Business").click();
            h.run_steps(4);
            h.get_by_label("Product promo").click();
            h.run_steps(4);
            assert!(!h.state().choosing_template);
            assert!(h.state().project().music.is_some());
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("template-editor-{name}.png"))).unwrap(); }
            let path = std::env::temp_dir().join(format!("kinetto-template-{}-{name}.kinetto",std::process::id()));
            storage::save(h.state().project(),&h.state().session.media,&path).unwrap();
            let loaded = storage::load(&path).unwrap();
            for id in loaded.project.media_ids() { assert!(loaded.media.decode(id).is_ok()); }
            assert_eq!(&loaded.project,h.state().project());
            std::fs::remove_file(path).unwrap();
            h.get_by_label("Quick video").click();
            h.run_steps(4);
            assert!(h.state().quick.is_some());
            h.get_by_label("Cancel").click();
            h.run_steps(4);
            assert!(h.state().quick.is_none());
            assert_eq!(&loaded.project,h.state().project());
        }
    }

    #[test]
    fn slide_look_picker_changes_one_slide_then_applies_to_all() {
        let screenshots = std::env::var_os("KINETTO_SLIDE_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            let project = kn_model::Project::sample();
            let second = project.slides[1].clone();
            open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
            h.state_mut().selected = None;
            h.run_steps(4);
            h.get_by_role_and_label(egui::Role::RadioButton, "Ocean").click();
            h.run_steps(4);
            assert_eq!(h.state().project().slides[0].theme.as_deref(), Some("Ocean"));
            assert_eq!(h.state().project().slides[1], second);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("slide-inspector-{name}.png"))).unwrap(); }
            h.get_by_label("Apply to all slides").click();
            h.run_steps(4);
            assert_eq!(h.state().project().theme, "Ocean");
            assert_eq!(h.state().project().slides[1].background, kn_layout::theme("Ocean").background);
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().slides[1], second);
        }
    }

    #[test]
    fn photo_motion_presets_apply_and_undo_in_both_themes() {
        let screenshots = std::env::var_os("KINETTO_PICTURE_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            let mut project = kn_model::Project::sample();
            project.slides[0].elements = vec![kn_model::Element::image("test-photo".into(), kn_geom::Rect::new(100., 100., 600., 600.))];
            open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
            h.state_mut().selected = Some(0);
            h.run_steps(4);
            h.get_by_label("Photo preset").scroll_to_me();
            h.run_steps(4);
            h.get_by_label("Photo preset").click();
            h.run_steps(4);
            h.get_by_label("Product focus").click();
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("picture-inspector-{name}.png"))).unwrap(); }
            let element = &h.state().project().slides[0].elements[0];
            assert_eq!(element.photo_motion, kn_model::PhotoMotion::ZoomIn);
            assert_eq!(element.photo_amount, 0.16);
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().slides[0].elements[0].photo_motion, kn_model::PhotoMotion::None);
        }
    }

    #[test]
    fn text_motion_picker_updates_timing_and_undo_in_both_themes() {
        let screenshots = std::env::var_os("KINETTO_MOTION_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
            h.state_mut().selected = Some(1);
            h.run_steps(4);
            h.get_by_label("Entrance animation").scroll_to_me();
            h.run_steps(4);
            h.get_by_label("Entrance animation").click();
            h.run_steps(4);
            h.get_by_label("Letter by letter").click();
            h.run_steps(4);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("text-motion-menu-{name}.png"))).unwrap(); }
            h.get_by_label("Draw letters").click();
            h.run_steps(4);
            let element = &h.state().project().slides[0].elements[1];
            assert_eq!(element.motion, kn_model::Motion::LetterReveal);
            assert_eq!(element.motion_duration_ms, 1800);
            assert_eq!(element.easing, kn_model::Easing::Linear);
            assert!(!h.state().playback.playing);
            assert_eq!(h.state().selected, Some(1));
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("text-motion-{name}.png"))).unwrap(); }
            h.get_by_label("Replay").scroll_to_me();
            h.run_steps(4);
            h.get_by_label("Replay").click();
            h.run_steps(4);
            assert!(h.state().playback.playing);
            h.state_mut().stop_playback();
            assert!(h.state_mut().session.undo());
            let element = &h.state().project().slides[0].elements[1];
            assert_eq!(element.motion, kn_model::Motion::Rise);
            assert_eq!(element.motion_duration_ms, 600);
        }
    }

    #[test]
    fn shape_gallery_inserts_selected_geometry_and_supports_undo() {
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            let mut h = Harness::builder().with_size([900.0, 700.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), None);
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
    fn square_rotation_preserves_geometry_and_undo() {
        let screenshots = std::env::var_os("KINETTO_SHAPE_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
            h.state_mut().add_shape(kn_model::ShapeKind::Square);
            h.run_steps(4);
            let index = h.state().selected.unwrap();
            let before = h.state().project().slides[0].elements[index].clone();
            assert_eq!(before.frame.w, before.frame.h);
            h.get_by_role_and_label(egui::Role::Slider, "Shape rotation").set_value_accesskit(37.5);
            h.run_steps(4);
            let rotated = &h.state().project().slides[0].elements[index];
            assert_eq!(rotated.rotation, 37.5);
            assert_eq!(rotated.frame, before.frame);
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("rotated-square-{name}.png"))).unwrap(); }
            assert!(h.state_mut().session.undo());
            assert_eq!(h.state().project().slides[0].elements[index], before);
            h.run_steps(4);
            h.get_by_role_and_label(egui::Role::Slider, "Shape rotation").set_value_accesskit(37.5);
            h.run_steps(4);
            h.get_by_label("Reset rotation").click();
            h.run_steps(4);
            assert_eq!(h.state().project().slides[0].elements[index].rotation, 0.);
        }
    }

    #[test]
    fn timeline_switches_between_slides_with_different_item_counts() {
        let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides[1].elements.clear();
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Timeline));
        h.run_steps(4);
        for (label, index) in [("Slide 2", 1), ("Slide 1", 0), ("Slide 2", 1)] {
            h.get_by_label(label).click();
            h.run_steps(4);
            assert_eq!(h.state().slide, index);
        }
    }

    #[test]
    fn problem_dialog_closes_with_escape_or_enter_and_keeps_the_slide() {
        let mut h = Harness::builder().with_size([1100.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().selected = Some(1);
        let before = h.state().project().clone();
        for key in [egui::Key::Escape, egui::Key::Enter] {
            h.state_mut().report(Problem::new("Something went wrong", "Try again."));
            h.run_steps(4);
            h.key_press(egui::Key::Delete);
            h.run_steps(2);
            assert!(h.state().problem.is_some(), "keys behind a dialog must not reach the editor");
            h.key_press(key);
            h.run_steps(4);
            assert!(h.state().problem.is_none(), "{key:?} should close the dialog");
            assert_eq!(h.state().selected, Some(1), "{key:?} must not also deselect the item");
        }
        assert_eq!(h.state().project(), &before);
    }

    #[test]
    fn focused_slider_arrow_keys_change_the_value_without_moving_the_item() {
        let mut h = Harness::builder().with_size([1100.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().selected = Some(1);
        h.run_steps(4);
        let before = h.state().project().slides[0].elements[1].clone();
        h.get_by_role_and_label(egui::Role::Slider, "Size").focus();
        h.run_steps(2);
        // Pressed twice: focus must stay on the slider, not move to a neighbour.
        for _ in 0..2 {
            h.key_press(egui::Key::ArrowRight);
            h.run_steps(4);
        }
        let after = &h.state().project().slides[0].elements[1];
        assert_eq!(after.frame, before.frame);
        let size = |e: &kn_model::Element| match e.kind { kn_model::ElementKind::Text { size, .. } => size, _ => unreachable!() };
        assert_eq!(size(after), size(&before) + 2.0);
    }

    #[test]
    fn focused_rotation_knob_keeps_the_arrow_keys() {
        // The stage (and its knob) needs the GPU canvas.
        if kn_export::Gpu::create().is_err() {
            eprintln!("no GPU; skipping");
            return;
        }
        let mut h = Harness::builder().with_size([1100.0, 900.0]).wgpu().build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().add_shape(kn_model::ShapeKind::Square);
        h.run_steps(4);
        let index = h.state().selected.unwrap();
        h.get_by_label("Rotate selected object").focus();
        h.run_steps(2);
        // Each press turns the shape; focus must not wander to the sidebar.
        for _ in 0..3 {
            h.key_press(egui::Key::ArrowLeft);
            h.run_steps(4);
        }
        assert_eq!(h.state().project().slides[0].elements[index].rotation, -3.0);
    }

    #[test]
    fn template_categories_filter_the_welcome_page_and_open_a_template() {
        let screenshots = std::env::var_os("KINETTO_TEMPLATE_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1440.0, 1000.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.run_steps(4);
            // "All" groups every template under its category's heading.
            // Once as a chip, once as the heading over its templates.
            assert_eq!(h.query_all_by_label("Memories & collages").count(), 2);
            assert!(h.query_by_label("Product promo").is_some());
            h.get_by_role_and_label(egui::Role::RadioButton, "Celebrations").click();
            h.run_steps(4);
            assert_eq!(h.state().template_category, Some(kn_layout::Category::Celebrations));
            assert!(h.query_by_label("Product promo").is_none());
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("templates-celebrations-{name}.png"))).unwrap(); }
            h.get_by_label("Birthday").click();
            h.run_steps(4);
            assert!(!h.state().choosing_template);
            assert_eq!(h.state().project(), &kn_layout::template(kn_layout::Template::Birthday));
            for id in h.state().project().media_ids() { assert!(h.state().session.media.decode(id).is_ok()); }
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("birthday-editor-{name}.png"))).unwrap(); }
        }
    }

    #[test]
    fn long_lists_scroll_inside_themselves_so_what_follows_stays_close() {
        let screenshots = std::env::var_os("KINETTO_SCROLL_SCREENS").map(std::path::PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([1200.0, 900.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            // Recent projects: twenty of them still leave the templates close.
            let recent: Vec<std::path::PathBuf> = (0..20).map(|i| std::env::temp_dir().join(format!("kinetto-missing-{i}.kinetto"))).collect();
            h.state_mut().settings.recent_projects = recent;
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.run_steps(4);
            let gap = h.get_by_label("Designed starting points").rect().top() - h.get_by_label("Recent projects").rect().top();
            assert!(gap < super::recent::RECENT_HEIGHT + 140.0, "recent list grew to {gap}");

            // Thirty items on a slide.
            let mut project = kn_model::Project::sample();
            let item = |i: usize| kn_model::Element::text(&format!("Item {i}"), kn_geom::Rect::new(100.0, 100.0, 400.0, 80.0), 40.0, false, kn_geom::Color::rgb(0, 0, 0));
            project.slides[0].elements = (0..30).map(item).collect();
            open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
            h.state_mut().selected = None;
            h.run_steps(4);
            let gap = h.get_by_label("Timing").rect().top() - h.get_by_label("Items on this slide").rect().top();
            assert!(gap < super::inspector::ITEMS_LIST_HEIGHT + 160.0, "items list grew to {gap}");
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("items-scroll-{name}.png"))).unwrap(); }
            // The last row starts out of view, below the list's bottom edge...
            let timing = h.get_by_label("Timing").rect().top();
            assert!(h.get_by_label("Item 0").rect().top() > timing, "Item 0 should start scrolled out of view");
            // Turning the mouse wheel over the list scrolls the list first; the
            // panel stays put until the list reaches its end.
            let wheel = |h: &mut Harness<'_, KinettoApp>| {
                h.event(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -150.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE });
                h.run_steps(12);
            };
            h.hover_at(h.get_by_label("Item 26").rect().center());
            h.run_steps(2);
            let top_row = h.get_by_label("Item 29").rect().top();
            wheel(&mut h);
            assert!(h.get_by_label("Item 29").rect().top() < top_row - 100.0, "the list scrolled");
            assert_eq!(h.get_by_label("Timing").rect().top(), timing, "the panel stayed put");
            // Scrolling on brings the last row into view inside the list.
            for _ in 0..12 {
                if h.get_by_label("Item 0").rect().bottom() < h.get_by_label("Timing").rect().top() { break; }
                wheel(&mut h);
            }
            assert!(h.get_by_label("Item 0").rect().bottom() < h.get_by_label("Timing").rect().top(), "Item 0 scrolled into view");
            h.get_by_label("Item 0").click();
            h.run_steps(4);
            assert_eq!(h.state().selected, Some(0));

            // A long text scrolls inside the Words box.
            let long = (0..60).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
            h.state_mut().session.execute(kn_engine::Command::UpdateElement { slide: 0, index: 0, element: kn_model::Element::text(&long, kn_geom::Rect::new(100.0, 100.0, 400.0, 80.0), 40.0, false, kn_geom::Color::rgb(0, 0, 0)) }).unwrap();
            h.run_steps(4);
            let gap = h.get_by_role_and_label(egui::Role::Slider, "Size").rect().top() - h.get_by_label("Words").rect().top();
            assert!(gap < super::inspector::WORDS_HEIGHT + 120.0, "words box grew to {gap}");
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
            // Let the page settle without the banner before clicking.
            h.run_steps(4);

            h.get_by_role_and_label(egui::Role::RadioButton, "Business").click();
            h.run_steps(4);
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
            h.get_by_label("Create video").click();
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
