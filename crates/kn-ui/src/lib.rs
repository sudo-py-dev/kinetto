//! The Kinetto editor window: top bar, slide strip, stage and inspector.

mod adw;
mod canvas;
#[doc(hidden)]
pub mod device;
mod export;
mod header;
mod icons;
mod inspector;
mod sound;
mod stage;
mod start;
mod strip;
mod theme;

use canvas::VelloCanvas;
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
    toast: Option<Toast>,
    title: String,
    about: bool,
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
            toast: None,
            title: String::new(),
            about: false,
            templates: kn_layout::Template::ALL.into_iter().map(|t| (t, kn_layout::template(t))).collect(),
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
        self.toast = Some(Toast { message: message.into(), undo: false, at: Instant::now() });
    }

    /// A toast for an edit that can be taken back with one click.
    fn notify_undo(&mut self, message: impl Into<String>) {
        self.toast = Some(Toast { message: message.into(), undo: true, at: Instant::now() });
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
        let action = toast.undo.then_some("Undo");
        if adw::toast(ctx, &toast.message, action) {
            self.toast = None;
            if self.session.undo() {
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
    at: Instant,
}

impl eframe::App for KinettoApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.resize_edges(&ctx);
        self.shortcuts(&ctx);
        self.dropped_files(&ctx);
        self.advance_playback(&ctx);
        let p = theme::palette(&ctx);

        egui::Panel::top("header")
            .exact_size(header::HEIGHT)
            .frame(self.panel_frame(&ctx, p.header, [true, true, false, false]))
            .show(ui, |ui| self.header_bar(ui, frame));
        if self.restorable.is_some() {
            egui::Panel::top("banner").frame(egui::Frame::new()).show_separator_line(false).show(ui, |ui| self.restore_banner(ui));
        }
        if self.choosing_template {
            egui::CentralPanel::default()
                .frame(self.panel_frame(&ctx, p.view, [false, false, true, true]))
                .show(ui, |ui| self.welcome(ui, frame));
        } else {
            egui::Panel::left("slides")
                .resizable(false)
                .exact_size(212.0)
                .frame(self.panel_frame(&ctx, p.sidebar, [false, false, true, false]))
                .show(ui, |ui| self.slide_strip(ui, frame));
            egui::Panel::right("inspector")
                .resizable(false)
                .exact_size(328.0)
                .frame(self.panel_frame(&ctx, p.window, [false, false, false, true]))
                .show(ui, |ui| self.inspector(ui));
            egui::CentralPanel::default().frame(egui::Frame::new().fill(p.view)).show(ui, |ui| self.stage(ui, frame));
        }
        self.window_outline(&ctx);

        self.drop_hint(&ctx);
        self.dialogs(&ctx);
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

#[cfg(test)]
mod screens {
    //! Renders the real window to PNGs for a visual check. Runs only when
    //! `KINETTO_SCREENS` names an output folder.

    use super::KinettoApp;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn render_screens() {
        let Some(dir) = std::env::var_os("KINETTO_SCREENS").map(std::path::PathBuf::from) else { return };
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let mut h = Harness::builder()
                .with_size([1280.0, 800.0])
                .with_theme(theme)
                .wgpu()
                .build_eframe(|cc| KinettoApp::new(cc));
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("welcome-{name}.png"))).unwrap();

            h.get_by_label("Product promo").click();
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("editor-{name}.png"))).unwrap();

            h.state_mut().selected = Some(1);
            h.run_steps(4);
            h.render().unwrap().save(dir.join(format!("item-{name}.png"))).unwrap();
        }
    }
}
