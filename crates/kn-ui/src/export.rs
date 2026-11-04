//! The "Make Video" flow: pick where to save, export in the background with
//! a progress bar, then offer to show the file.

use crate::KinettoApp;
use kn_export::{ExportError, ExportSettings};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

/// What to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// An MP4 (H.264 video with Opus sound).
    Video,
    /// An animated GIF.
    Gif,
    /// A PDF with one page per slide.
    Pdf,
}

impl Output {
    /// The file extension for this output.
    fn ext(self) -> &'static str {
        match self {
            Output::Video => "mp4",
            Output::Gif => "gif",
            Output::Pdf => "pdf",
        }
    }

    /// What to call this output in messages ("video", "GIF", "PDF").
    fn noun(self) -> &'static str {
        match self {
            Output::Video => "video",
            Output::Gif => "GIF",
            Output::Pdf => "PDF",
        }
    }
}

/// An export running on its own thread, with progress the dialog reads.
pub(crate) struct ExportJob {
    /// What is being made.
    output: Output,
    /// Where it is being written.
    out: PathBuf,
    /// Size, frame rate and quality used.
    settings: ExportSettings,
    /// Frames finished so far (written by the export thread).
    done: Arc<AtomicU64>,
    /// Frames in all.
    total: Arc<AtomicU64>,
    /// Set to ask the export thread to stop early.
    cancel: Arc<AtomicBool>,
    /// When it began, for the time-left estimate.
    started: Instant,
    /// The export thread, until it has been joined.
    thread: Option<JoinHandle<Result<(), ExportError>>>,
    /// Set once the thread has finished.
    outcome: Option<Result<(), ExportError>>,
    /// The progress the ring shows, easing toward the real value.
    shown: f32,
    /// When (in egui time) the export finished, for the check-mark animation.
    finished_at: Option<f64>,
}

impl KinettoApp {
    /// Asks where to save, then starts making an MP4.
    pub(crate) fn make_video(&mut self, frame: &eframe::Frame) {
        self.make(Output::Video, frame);
    }

    /// Asks where to save `output`, then starts making it in the background.
    pub(crate) fn make(&mut self, output: Output, frame: &eframe::Frame) {
        if self.export.is_some() {
            return;
        }
        self.stop_playback();
        let stem = self.session.path.as_ref().and_then(|p| p.file_stem()).map_or("My video".into(), |s| s.to_string_lossy().into_owned());
        let filter = match output {
            Output::Video => "MP4 video",
            Output::Gif => "Animated GIF",
            Output::Pdf => "PDF document",
        };
        let ext = output.ext();
        let Some(mut out) = rfd::FileDialog::new().add_filter(filter, &[ext]).set_file_name(format!("{stem}.{ext}")).save_file() else {
            return;
        };
        if out.extension().is_none() {
            out.set_extension(ext);
        }

        let settings = ExportSettings::full_hd(self.project().aspect);
        // Export on the editor's own GPU device rather than a second one.
        let gpu = frame.wgpu_render_state().map(|rs| kn_export::Gpu { device: rs.device.clone(), queue: rs.queue.clone() });
        let project = Arc::clone(self.session.project());
        let media = self.session.media.clone();
        let (done, total, cancel) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(1)), Arc::new(AtomicBool::new(false)));
        let thread = {
            let (done, total, cancel, out) = (Arc::clone(&done), Arc::clone(&total), Arc::clone(&cancel), out.clone());
            std::thread::Builder::new().name("kinetto-export".into()).spawn(move || {
                let report = |d, t| {
                    total.store(t, Ordering::Relaxed);
                    done.store(d, Ordering::Relaxed);
                };
                match output {
                    Output::Video => kn_export::export_mp4(&project, &media, &settings, gpu, &out, &cancel, report),
                    Output::Gif => kn_export::export_gif(&project, &media, gpu, &out, &cancel, report),
                    Output::Pdf => kn_export::export_pdf(&project, &media, gpu, &out, &cancel, report),
                }
            })
        };
        match thread {
            Ok(thread) => {
                self.export = Some(ExportJob { output, out, settings, done, total, cancel, started: Instant::now(), thread: Some(thread), outcome: None, shown: 0.0, finished_at: None });
            }
            Err(_) => self.report(crate::problem::Problem::new(
                format!("Couldn't start making the {}", output.noun()),
                "Your computer is low on memory or other resources. Close some programs, then try again.",
            )),
        }
    }

    /// The progress dialog while an export runs, and its result once done.
    pub(crate) fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(job) = self.export.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        if job.outcome.is_none() && job.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = job.thread.take().expect("checked above").join();
            job.outcome = Some(match result {
                Ok(r) => r,
                // The export thread panicked: report it like any failure.
                Err(_) => Err(ExportError::Io(std::io::Error::other("the export stopped unexpectedly"))),
            });
            job.finished_at = Some(now);
        }

        let (done, total) = (job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed).max(1));
        let target = if matches!(job.outcome, Some(Ok(()))) { 1.0 } else { (done as f32 / total as f32).clamp(0.0, 1.0) };
        // Ease the ring toward the real progress so it glides between reports.
        let motion = ctx.style_of(ctx.theme()).animation_time > 0.0;
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        job.shown = if motion { job.shown + (target - job.shown) * (1.0 - (-dt * 10.0).exp()) } else { target };
        if (target - job.shown).abs() < 0.001 {
            job.shown = target;
        }

        let mut close = false;
        let mut animating = (target - job.shown).abs() > 0.0;
        let p = crate::theme::palette(ctx);
        let style = ctx.style_of(ctx.theme());
        let dialog = egui::Frame::new()
            .fill(style.visuals.window_fill)
            .corner_radius(crate::theme::DIALOG_RADIUS)
            .inner_margin(egui::Margin { left: 28, right: 28, top: 28, bottom: 22 })
            .shadow(style.visuals.window_shadow);
        egui::Modal::new(egui::Id::unique("export")).frame(dialog).show(ctx, |ui| {
            ui.set_width(340.0);
            let name = job.out.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            ui.vertical_centered(|ui| {
                let ring = match &job.outcome {
                    None if done == 0 => Ring::Waiting(now),
                    None => Ring::Progress(job.shown),
                    Some(Ok(())) => {
                        let t = if motion { ((now - job.finished_at.unwrap_or(now)) / 0.7) as f32 } else { 1.0 };
                        animating |= t < 1.0;
                        Ring::Done(t.min(1.0))
                    }
                    Some(Err(_)) => Ring::Progress(job.shown),
                };
                if matches!(ring, Ring::Waiting(_)) {
                    animating = true;
                }
                progress_ring(ui, p, ring);
                ui.add_space(16.0);

                let title = match &job.outcome {
                    Some(Ok(())) => format!("Your {} is ready", job.output.noun()),
                    _ if job.cancel.load(Ordering::Relaxed) => "Stopping…".to_owned(),
                    _ => format!("Making your {}…", job.output.noun()),
                };
                ui.label(egui::RichText::new(title).family(crate::theme::bold()).size(19.0).color(p.fg));
                ui.add_space(2.0);
                ui.add(egui::Label::new(egui::RichText::new(name).size(13.0).color(p.dim)).truncate());
                ui.add_space(14.0);

                let s = job.settings;
                let mut chips = match job.output {
                    Output::Video => vec![format!("{} × {}", s.width, s.height), format!("{} fps", s.fps.num / s.fps.den.max(1))],
                    Output::Gif => vec![format!("{} px wide", kn_export::GIF_WIDTH)],
                    Output::Pdf => vec![format!("{total} slides")],
                };
                match &job.outcome {
                    None if done == 0 => chips.push("Getting ready".to_owned()),
                    None => {
                        if job.output == Output::Pdf {
                            chips[0] = format!("Slide {done} of {total}");
                        }
                        let left = job.started.elapsed().as_secs_f64() * (total - done) as f64 / done as f64;
                        chips.push(format!("About {} left", human_time(left)));
                    }
                    Some(_) => {
                        let took = job.finished_at.map_or(0.0, |f| f - now) + job.started.elapsed().as_secs_f64();
                        chips.push(format!("Took {}", human_time(took)));
                    }
                }
                chip_row(ui, p, &chips);
            });
            ui.add_space(22.0);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match &job.outcome {
                None => {
                    let stopping = job.cancel.load(Ordering::Relaxed);
                    ui.add_enabled_ui(!stopping, |ui| {
                        if crate::adw::button(ui, None, "Cancel", crate::adw::Kind::Normal).clicked() {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    });
                }
                Some(Ok(())) => {
                    if crate::adw::button(ui, None, "Done", crate::adw::Kind::Suggested).clicked() {
                        close = true;
                    }
                    if crate::adw::button(ui, Some(crate::icons::Icon::Open), "Show in folder", crate::adw::Kind::Normal).clicked() {
                        reveal(&job.out);
                    }
                }
                Some(Err(_)) => close = true,
            });
        });
        if close {
            let job = self.export.take().expect("dialog was open");
            if let Some(Err(err)) = &job.outcome
                && let Some(problem) = crate::problem::Problem::export(job.output, &job.out, err)
            {
                self.report(problem);
            }
        } else if animating {
            ctx.request_repaint();
        } else if job.outcome.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// What the progress ring shows.
#[derive(Clone, Copy)]
enum Ring {
    /// Nothing reported yet: a short arc spins (the egui time, in seconds).
    Waiting(f64),
    /// This much done, 0 to 1.
    Progress(f32),
    /// Finished: the ring fills and a check mark draws in (0 to 1 through the animation).
    Done(f32),
}

/// Draws the round progress indicator, centred in the row.
fn progress_ring(ui: &mut egui::Ui, p: &crate::theme::Palette, ring: Ring) {
    use egui::{Pos2, Shape, Stroke, vec2};
    use std::f32::consts::TAU;
    const SIZE: f32 = 84.0;
    const WIDTH: f32 = 7.0;
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(SIZE), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let c = rect.center();
    let r = SIZE / 2.0 - WIDTH / 2.0;
    let accent = p.accent_bg;
    let arc = |from: f32, sweep: f32, color| {
        if sweep <= 0.0 {
            return;
        }
        let steps = ((sweep / TAU) * 96.0).ceil().max(2.0) as usize;
        let at = |a: f32| c + vec2(a.cos(), a.sin()) * r;
        let pts: Vec<Pos2> = (0..=steps).map(|i| at(from + sweep * i as f32 / steps as f32)).collect();
        painter.add(Shape::line(pts, Stroke::new(WIDTH, color)));
        // Round caps.
        painter.circle_filled(at(from), WIDTH / 2.0, color);
        painter.circle_filled(at(from + sweep), WIDTH / 2.0, color);
    };
    let top = -TAU / 4.0;
    match ring {
        Ring::Waiting(time) => {
            painter.circle_stroke(c, r, Stroke::new(WIDTH, p.button[0]));
            let spin = (time as f32 * 1.6 * TAU) % TAU;
            let breathe = 0.18 + 0.1 * (time as f32 * 2.4).sin();
            arc(top + spin, breathe * TAU, accent);
        }
        Ring::Progress(f) => {
            painter.circle_stroke(c, r, Stroke::new(WIDTH, p.button[0]));
            arc(top, f * TAU, accent);
            let pct = format!("{:.0}", (f * 100.0).floor());
            let big = egui::FontId::new(24.0, crate::theme::bold());
            let small = egui::FontId::new(13.0, crate::theme::semibold());
            let n = painter.layout_no_wrap(pct, big, p.fg);
            let s = painter.layout_no_wrap("%".to_owned(), small, p.dim);
            let w = n.size().x + 1.0 + s.size().x;
            let left = c.x - w / 2.0;
            let base = c.y - n.size().y / 2.0;
            let s_y = base + n.size().y - s.size().y - 3.0;
            painter.galley(Pos2::new(left, base), n.clone(), p.fg);
            painter.galley(Pos2::new(left + n.size().x + 1.0, s_y), s, p.dim);
        }
        Ring::Done(t) => {
            // The disc swells out of the full ring with a little overshoot.
            let grow = ease_out_back((t / 0.55).min(1.0));
            painter.circle_stroke(c, r, Stroke::new(WIDTH, accent));
            painter.circle_filled(c, SIZE / 2.0 * grow.max(0.0), accent);
            // Then the check mark draws itself along its two strokes.
            let k = ((t - 0.3) / 0.7).clamp(0.0, 1.0);
            let k = 1.0 - (1.0 - k).powi(3);
            let u = SIZE / 2.0;
            let pts = [c + vec2(-0.36, 0.02) * u, c + vec2(-0.1, 0.28) * u, c + vec2(0.38, -0.24) * u];
            let (l1, l2) = (pts[0].distance(pts[1]), pts[1].distance(pts[2]));
            let reach = k * (l1 + l2);
            let mut line = vec![pts[0]];
            if reach <= l1 {
                line.push(pts[0].lerp(pts[1], reach / l1));
            } else {
                line.push(pts[1]);
                line.push(pts[1].lerp(pts[2], (reach - l1) / l2));
            }
            if k > 0.0 {
                let stroke = Stroke::new(5.5, p.accent_fg);
                painter.add(Shape::line(line.clone(), stroke));
                for q in [line[0], *line.last().expect("two points or more")] {
                    painter.circle_filled(q, 2.75, p.accent_fg);
                }
                if line.len() == 3 {
                    painter.circle_filled(line[1], 2.75, p.accent_fg);
                }
            }
        }
    }
}

/// A gentle overshoot easing for the finished disc.
fn ease_out_back(t: f32) -> f32 {
    let (c1, c3) = (1.4, 2.4);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// A centred row of small rounded labels ("1920 × 1080", "30 fps", …).
fn chip_row(ui: &mut egui::Ui, p: &crate::theme::Palette, chips: &[String]) {
    const GAP: f32 = 6.0;
    const PAD: f32 = 10.0;
    const HEIGHT: f32 = 24.0;
    let font = egui::FontId::new(12.5, crate::theme::semibold());
    let galleys: Vec<_> = chips.iter().map(|c| ui.painter().layout_no_wrap(c.clone(), font.clone(), p.dim)).collect();
    let width: f32 = galleys.iter().map(|g| g.size().x + PAD * 2.0).sum::<f32>() + GAP * galleys.len().saturating_sub(1) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT), egui::Sense::hover());
    let mut x = rect.center().x - width / 2.0;
    for (i, (g, text)) in galleys.into_iter().zip(chips).enumerate() {
        let chip = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(g.size().x + PAD * 2.0, HEIGHT));
        // Painted by hand, so tell screen readers what each chip says.
        let response = ui.interact(chip, ui.scope_id().with(("export-chip", i)), egui::Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
        ui.painter().rect_filled(chip, HEIGHT / 2.0, p.button[0]);
        let pos = chip.center() - g.size() / 2.0;
        ui.painter().galley(pos, g, p.dim);
        x = chip.right() + GAP;
    }
}

impl KinettoApp {
    /// Stops a running export and waits for it to clean up its file.
    pub(crate) fn cancel_export_and_wait(&mut self) {
        if let Some(mut job) = self.export.take() {
            job.cancel.store(true, Ordering::Relaxed);
            if let Some(thread) = job.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

/// Time left, rounded up: "45 s" under a minute, else "3 min".
fn human_time(secs: f64) -> String {
    if secs < 60.0 { format!("{} s", secs.ceil().max(1.0) as u64) } else { format!("{} min", (secs / 60.0).ceil() as u64) }
}

/// Opens the system file manager at `path`.
fn reveal(path: &Path) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(Path::new("."))).spawn()
    };
    if let Err(err) = result {
        tracing::warn!("could not open the file manager: {err}");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// A job whose "export thread" just waits until `finish` is set.
    fn job(output: Output, finish: &Arc<AtomicBool>) -> ExportJob {
        let finish = Arc::clone(finish);
        let thread = std::thread::spawn(move || {
            while !finish.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Ok(())
        });
        ExportJob {
            output,
            out: PathBuf::from("/tmp/My video.mp4"),
            settings: ExportSettings::full_hd(kn_model::Project::sample().aspect),
            done: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(120)),
            cancel: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            thread: Some(thread),
            outcome: None,
            shown: 0.0,
            finished_at: None,
        }
    }

    #[test]
    fn export_dialog_shows_progress_then_ready() {
        let screenshots = std::env::var_os("KINETTO_EXPORT_SCREENS").map(PathBuf::from);
        for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
            let builder = Harness::builder().with_size([720.0, 520.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            h.state_mut().recovery = None;
            h.state_mut().restorable = None;
            h.state_mut().choosing_template = false;
            let finish = Arc::new(AtomicBool::new(false));
            h.state_mut().export = Some(job(Output::Video, &finish));
            h.run_steps(4);
            h.get_by_label("Making your video…");
            h.get_by_label("Getting ready");
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("export-waiting-{name}.png"))).unwrap(); }

            h.state().export.as_ref().unwrap().done.store(51, Ordering::Relaxed);
            h.run_steps(60);
            assert!((h.state().export.as_ref().unwrap().shown - 51.0 / 120.0).abs() < 0.01, "The ring eases to the real progress");
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("export-progress-{name}.png"))).unwrap(); }

            finish.store(true, Ordering::Relaxed);
            while h.state().export.as_ref().unwrap().thread.as_ref().is_some_and(|t| !t.is_finished()) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            h.run_steps(60);
            h.get_by_label("Your video is ready");
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("export-ready-{name}.png"))).unwrap(); }
            h.get_by_label("Done").click();
            h.run_steps(4);
            assert!(h.state().export.is_none());
        }
    }
}
