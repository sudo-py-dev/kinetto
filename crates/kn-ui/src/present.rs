//! Present mode: the slides full screen, advanced by the presenter. Each
//! slide plays its entrance (and its sound) and then holds until the next
//! key press or click.

use crate::KinettoApp;
use crate::canvas::{Frame, Surface};
use egui::{Key, Vec2, ViewportCommand};
use kn_time::Tick;
use std::time::{Duration, Instant};

/// The pointer hides after this long without moving.
const CURSOR_HIDE: Duration = Duration::from_secs(2);

pub(crate) struct Present {
    slide: usize,
    started: Instant,
    last_pointer_move: Instant,
}

impl KinettoApp {
    pub(crate) fn start_presenting(&mut self, ctx: &egui::Context) {
        self.stop_playback();
        self.selected = None;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        self.presenting = Some(Present { slide: self.slide, started: Instant::now(), last_pointer_move: Instant::now() });
        self.play_slide_sound();
    }

    fn stop_presenting(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.presenting.take() {
            self.slide = p.slide;
        }
        self.stop_playback();
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
    }

    fn go_to(&mut self, ctx: &egui::Context, slide: Option<usize>) {
        match slide {
            Some(slide) if slide < self.project().slides.len() => {
                if let Some(p) = &mut self.presenting {
                    p.slide = slide;
                    p.started = Instant::now();
                }
                self.play_slide_sound();
            }
            _ => self.stop_presenting(ctx),
        }
    }

    /// Plays the soundtrack from the start of the current slide; it is
    /// stopped when the slide's time runs out.
    fn play_slide_sound(&mut self) {
        let Some(slide) = self.presenting.as_ref().map(|p| p.slide) else { return };
        let start = self.slide_start(slide);
        self.play_from(start);
        // Present mode keeps its own clock, so the editor's playback stays off.
        self.playing = false;
    }

    /// Draws the presentation and handles its keys and clicks.
    pub(crate) fn present_view(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        let Some(present) = self.presenting.as_mut() else { return };
        let (next, prev, exit, first, last) = ctx.input(|i| {
            let pressed = |keys: &[Key]| keys.iter().any(|k| i.key_pressed(*k));
            (
                pressed(&[Key::ArrowRight, Key::Space, Key::PageDown, Key::Enter, Key::ArrowDown]) || i.pointer.primary_clicked(),
                pressed(&[Key::ArrowLeft, Key::PageUp, Key::Backspace, Key::ArrowUp]) || i.pointer.secondary_clicked(),
                pressed(&[Key::Escape]),
                pressed(&[Key::Home]),
                pressed(&[Key::End]),
            )
        });
        if ctx.input(|i| i.pointer.delta() != Vec2::ZERO) {
            present.last_pointer_move = Instant::now();
        }
        let current = present.slide;
        let elapsed = present.started.elapsed();
        let hide_cursor = present.last_pointer_move.elapsed() > CURSOR_HIDE;

        let count = self.project().slides.len();
        if exit {
            return self.stop_presenting(&ctx);
        } else if next {
            return self.go_to(&ctx, Some(current + 1));
        } else if prev {
            return self.go_to(&ctx, current.checked_sub(1).or(Some(0)));
        } else if first {
            return self.go_to(&ctx, Some(0));
        } else if last {
            return self.go_to(&ctx, Some(count - 1));
        }

        // Play the slide, then hold on its last frame.
        let slide = &self.project().slides[current];
        let local = Tick((elapsed.as_secs_f64() * kn_time::TICKS_PER_SECOND as f64) as i64).min(slide.duration - Tick(1));
        if local >= slide.duration - Tick(1) {
            if let Some(player) = &self.player {
                player.stop();
            }
        } else {
            ctx.request_repaint();
        }
        if hide_cursor {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        } else {
            ctx.request_repaint_after(CURSOR_HIDE);
        }
        let time = self.slide_start(current) + local;

        let (cw, ch) = self.project().aspect.canvas_size();
        let area = ui.max_rect();
        let fit = (area.width() / cw as f32).min(area.height() / ch as f32);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let rect = egui::Rect::from_center_size(area.center(), size);
        ui.painter().rect_filled(area, 0, egui::Color32::BLACK);
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let ppp = ctx.pixels_per_point();
            let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
            let f = Frame { project: self.session.project(), media: &self.session.media, time, size: px, stamp: None };
            let id = canvas.paint(rs, Surface::Stage, f, &mut self.render);
            egui::Image::new((id, size)).paint_at(ui, rect);
        }
        if !hide_cursor {
            let hint = format!("{} of {}   ·   Right arrow or click for next   ·   Left arrow to go back   ·   Esc to finish", current + 1, count);
            ui.painter().text(
                area.center_bottom() - Vec2::new(0.0, 18.0),
                egui::Align2::CENTER_BOTTOM,
                hint,
                egui::FontId::proportional(13.0),
                egui::Color32::from_white_alpha(140),
            );
        }
    }
}
