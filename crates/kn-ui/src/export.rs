//! The "Make Video" flow: pick where to save, export in the background with
//! a progress bar, then offer to show the file.

use crate::KinettoApp;
use kn_export::{ExportError, ExportSettings};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

pub(crate) struct ExportJob {
    out: PathBuf,
    settings: ExportSettings,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    started: Instant,
    thread: Option<JoinHandle<Result<(), ExportError>>>,
    /// Set once the thread has finished.
    outcome: Option<Result<(), String>>,
}

impl KinettoApp {
    pub(crate) fn make_video(&mut self, frame: &eframe::Frame) {
        if self.export.is_some() {
            return;
        }
        self.stop_playback();
        let stem = self.session.path.as_ref().and_then(|p| p.file_stem()).map_or("My video".into(), |s| s.to_string_lossy().into_owned());
        let Some(mut out) = rfd::FileDialog::new().add_filter("MP4 video", &["mp4"]).set_file_name(format!("{stem}.mp4")).save_file() else {
            return;
        };
        if out.extension().is_none() {
            out.set_extension("mp4");
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
                kn_export::export_mp4(&project, &media, &settings, gpu, &out, &cancel, |d, t| {
                    total.store(t, Ordering::Relaxed);
                    done.store(d, Ordering::Relaxed);
                })
            })
        };
        match thread {
            Ok(thread) => {
                self.export = Some(ExportJob { out, settings, done, total, cancel, started: Instant::now(), thread: Some(thread), outcome: None });
            }
            Err(err) => self.notify(format!("Couldn't start the export: {err}")),
        }
    }

    pub(crate) fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(job) = self.export.as_mut() else { return };
        if job.outcome.is_none() && job.thread.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = job.thread.take().expect("checked above").join();
            job.outcome = Some(match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(ExportError::Cancelled)) => Err(String::new()),
                Ok(Err(err)) => Err(err.to_string()),
                Err(_) => Err("the export stopped unexpectedly".into()),
            });
        }

        let mut close = false;
        let p = crate::theme::palette(ctx);
        let style = ctx.style_of(ctx.theme());
        let dialog = egui::Frame::new()
            .fill(style.visuals.window_fill)
            .corner_radius(crate::theme::DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(24))
            .shadow(style.visuals.window_shadow);
        egui::Modal::new(egui::Id::unique("export")).frame(dialog).show(ctx, |ui| {
            ui.set_width(360.0);
            let heading = |ui: &mut egui::Ui, text: &str| ui.label(egui::RichText::new(text).family(crate::theme::bold()).size(19.0));
            match &job.outcome {
                None => {
                    heading(ui, "Making your video…");
                    ui.add_space(12.0);
                    let (done, total) = (job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed).max(1));
                    let fraction = done as f32 / total as f32;
                    ui.add(egui::ProgressBar::new(fraction).fill(p.accent_bg).desired_height(8.0).corner_radius(4));
                    let s = job.settings;
                    let mut detail = format!("{} × {}  ·  {} fps  ·  {:.0}%", s.width, s.height, s.fps.num / s.fps.den.max(1), fraction * 100.0);
                    if done > 0 {
                        let left = job.started.elapsed().as_secs_f64() * (total - done) as f64 / done as f64;
                        detail.push_str(&format!("  ·  about {} left", human_time(left)));
                    }
                    ui.label(egui::RichText::new(detail).size(12.5).color(p.dim));
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::adw::button(ui, None, "Cancel", crate::adw::Kind::Normal).clicked() {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                    });
                }
                Some(Ok(())) => {
                    heading(ui, "Your video is ready");
                    ui.add_space(4.0);
                    let name = job.out.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    ui.label(egui::RichText::new(name).color(p.dim));
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::adw::button(ui, None, "Done", crate::adw::Kind::Suggested).clicked() {
                            close = true;
                        }
                        if crate::adw::button(ui, None, "Show in folder", crate::adw::Kind::Normal).clicked() {
                            reveal(&job.out);
                        }
                    });
                }
                Some(Err(_)) => close = true,
            }
        });
        if close {
            let job = self.export.take().expect("dialog was open");
            if let Some(Err(message)) = job.outcome
                && !message.is_empty()
            {
                self.notify(format!("The video couldn't be made: {message}"));
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

fn human_time(secs: f64) -> String {
    if secs < 60.0 { format!("{} s", secs.ceil() as u64) } else { format!("{} min", (secs / 60.0).ceil() as u64) }
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
