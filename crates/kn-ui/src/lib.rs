//! The Kinetto editor window: top bar, slide strip, stage and inspector.

mod adw;
mod canvas;
mod dialog;
#[doc(hidden)]
pub mod device;
mod export;
mod finder;
mod header;
pub mod i18n;
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
use egui::{Key, KeyboardShortcut, Modifiers};
use kn_engine::storage::{self, Bundle, Recovery};
use kn_engine::{Command, Session};
use kn_geom::{Color, Rect};
use kn_model::{Element, Project};
use kn_render::RenderContext;
use kn_time::Tick;
use problem::{Fix, Problem};
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
/// Find an action by typing.
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);

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
    /// A file dialog that is open.
    file_pick: Option<dialog::FilePick>,
    /// For waking the editor when a dialog closes.
    ctx: egui::Context,
    /// An item being moved, resized or rotated on the stage.
    drag: Option<stage::Drag>,
    /// A slide being dragged in the sidebar to a new place.
    slide_drag: Option<usize>,
    /// The project's revision when that drag began; any edit since cancels it.
    slide_drag_revision: u64,
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
    /// Guide lines the item being moved on the stage is snapped to.
    guides: stage::Guides,
    /// The item last copied, for pasting on any slide.
    clipboard: Option<Element>,
    /// Put the cursor in the inspector's Words box on the next frame.
    focus_words: bool,
    /// The "find an action" box, while it is open.
    finder: Option<finder::Finder>,
    /// The keyboard shortcuts window is open.
    shortcuts_open: bool,
    /// Which page of the slide settings is showing.
    inspector_tab: inspector::Tab,
    /// The export choices, while they are being made.
    export_sheet: Option<export::Output>,
    /// The fonts have been measured for how they draw right-to-left words.
    fonts_probed: bool,
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
            slide_drag: None,
            slide_drag_revision: 0,
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
            file_pick: None,
            ctx: cc.egui_ctx.clone(),
            settings: settings::Settings::load(),
            scrub: None,
            timeline_drag: None,
            waveform: Default::default(),
            template_category: None,
            templates: kn_layout::Template::ALL.into_iter().map(|t| (t, kn_layout::template(t))).collect(),
            template_media: kn_media::sample_library(),
            guides: stage::Guides::default(),
            clipboard: None,
            focus_words: false,
            finder: None,
            shortcuts_open: false,
            inspector_tab: inspector::Tab::default(),
            export_sheet: None,
            fonts_probed: false,
        }
    }

    /// Open a file passed by the desktop file manager at application startup.
    pub fn new_with_project(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut app = Self::new(cc);
        if let Some(path) = path { app.open_project(path); }
        app
    }

    /// Loads the project file at `path` into the editor. A failure is
    /// reported and whatever was showing stays.
    fn open_project(&mut self, path: PathBuf) {
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
        self.show_toast(message.into(), false);
    }

    /// A toast for an edit that can be taken back with one click.
    fn notify_undo(&mut self, message: impl Into<String>) {
        self.show_toast(message.into(), true);
    }

    /// Replaces whatever toast is showing with a new one.
    fn show_toast(&mut self, message: String, undo: bool) {
        self.toast = Some(Toast { message, undo, again: None, at: Instant::now() });
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
        let element = Element::text(i18n::content("Your text"), Rect::new(360.0, h / 2.0 - 60.0, 1200.0, 120.0), 80.0, true, readable_on(bg));
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
        self.ask_for_file(dialog::Purpose::Pictures, None);
    }

    /// Reads a picture file and adds it, refusing files over the size limit.
    fn add_picture_file(&mut self, path: &Path) {
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > kn_media::MAX_FILE_BYTES as u64);
        if too_big {
            return self.report(Problem::too_big(path, 100));
        }
        let name = path.file_name().map_or_else(|| crate::i18n::tr("the picture").to_owned(), |n| format!("\u{201c}{}\u{201d}", n.to_string_lossy()));
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
                    (_, Ok(bytes)) => quick.add_picture(ctx, bytes, crate::i18n::tr("the dropped file")),
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
                    Ok(bytes) => self.add_picture(bytes, crate::i18n::tr("the dropped file")),
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
        painter.text(screen.center(), egui::Align2::CENTER_CENTER, i18n::tr("Drop pictures or sounds to add them"), egui::FontId::proportional(28.0), egui::Color32::WHITE);
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
            Pending::Open(path) => self.open_project(path),
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
        self.clear_recovery();
    }

    /// Removes the safety copy, once the work is saved or given up.
    fn clear_recovery(&self) {
        if let Some(recovery) = &self.recovery {
            recovery.clear();
        }
    }

    /// Asks for a project file and opens it.
    fn pick_open(&mut self) {
        self.ask_for_file(dialog::Purpose::Open, None);
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
        self.clear_recovery();
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
                crate::trf!("Changes can't be saved to “{name}”. {reason}", name = name, reason = reason)
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
                    Some(crate::trf!("Kinetto can't keep a safety copy of this video. {reason} Save it to a file to keep it safe.", reason = reason))
                }
            }
        } else {
            None
        };
        self.autosave_failed_at = failure.is_some().then(Instant::now);
        self.save_trouble = failure;
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

    /// Keyboard shortcuts: undo/redo (not while typing), save, open, new,
    /// find an action, Space to play and Page Up/Down between slides.
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
        if ctx.input_mut(|i| i.consume_shortcut(&FIND)) && !dialog_open(ctx) {
            self.open_finder();
        }
        if ctx.input(|i| i.key_pressed(Key::F1)) && !dialog_open(ctx) {
            self.shortcuts_open = true;
        }
        if typing || self.choosing_template || dialog_open(ctx) {
            return;
        }
        let (space, back, forward) = ctx.input(|i| (i.key_pressed(Key::Space), i.key_pressed(Key::PageUp), i.key_pressed(Key::PageDown)));
        if space {
            self.toggle_play();
        }
        if back && self.slide > 0 {
            self.select_slide(self.slide - 1);
        }
        if forward && self.slide + 1 < self.project().slides.len() {
            self.select_slide(self.slide + 1);
        }
    }

    /// Keeps the window title as "Name • — Kinetto", the dot marking unsaved edits.
    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self
            .session
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| i18n::content("Untitled").to_owned(), |s| s.to_string_lossy().into_owned());
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
                    self.clear_recovery();
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
                self.clear_recovery();
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
        i18n::set(self.settings.language);
        if !self.fonts_probed {
            i18n::probe(&ctx);
            self.fonts_probed = true;
        }
        let rtl = i18n::rtl();
        self.poll_file_pick(Some(frame));
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
            .show(ui, |ui| adw::reading_order(ui, |ui| self.header_bar(ui)));
        if self.restorable.is_some() {
            egui::Panel::top("banner").frame(egui::Frame::new().fill(p.window)).show_separator_line(false).show(ui, |ui| adw::reading_order(ui, |ui| self.restore_banner(ui)));
        }
        if self.save_trouble.is_some() && !self.choosing_template {
            egui::Panel::top("save-banner").frame(egui::Frame::new().fill(p.window)).show_separator_line(false).show(ui, |ui| adw::reading_order(ui, |ui| self.save_banner(ui)));
        }
        if self.choosing_template {
            egui::CentralPanel::default()
                .frame(self.panel_frame(&ctx, p.view, [false, false, true, true]))
                .show(ui, |ui| adw::reading_order(ui, |ui| self.welcome(ui, frame)));
        } else {
            egui::Panel::top("editor-toolbar")
                .exact_size(50.0)
                .frame(egui::Frame::new().fill(p.window))
                .show(ui, |ui| adw::reading_order(ui, |ui| self.editor_toolbar(ui)));
            let timeline = self.settings.layout == settings::Layout::Timeline;
            // In a right-to-left language the side panels swap: slides where
            // reading starts (the right), settings at the far side.
            let (inspector, slides) = if rtl { (egui::Panel::left("inspector"), egui::Panel::right("slides")) } else { (egui::Panel::right("inspector"), egui::Panel::left("slides")) };
            inspector
                .resizable(false)
                .exact_size(328.0)
                .frame(self.panel_frame(&ctx, p.window, if rtl { [false, false, true, false] } else { [false, false, false, true] }))
                .show(ui, |ui| adw::reading_order(ui, |ui| self.inspector(ui)));
            if timeline {
                egui::Panel::bottom("timeline")
                    .resizable(true)
                    .default_size(timeline::DEFAULT_HEIGHT)
                    .min_size(150.0)
                    .max_size(420.0)
                    .frame(self.panel_frame(&ctx, p.sidebar, if rtl { [false, false, false, true] } else { [false, false, true, false] }))
                    .show(ui, |ui| self.timeline(ui, frame));
            } else {
                slides
                    .resizable(false)
                    .exact_size(196.0)
                    .frame(self.panel_frame(&ctx, p.sidebar, if rtl { [false, false, false, true] } else { [false, false, true, false] }))
                    .show(ui, |ui| self.slide_strip(ui, frame));
            }
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.view)).show(ui, |ui| self.stage(ui, frame));
        }
        self.window_outline(&ctx);

        self.drop_hint(&ctx);
        self.problem_dialog(&ctx);
        self.quick_dialog(&ctx);
        self.about_dialog(&ctx);
        self.finder_dialog(&ctx);
        self.shortcuts_dialog(&ctx);
        self.export_sheet_dialog(&ctx);
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
mod screens;
