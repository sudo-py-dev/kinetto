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

/// Full-screen presenting (F5): one slide at a time, advanced by keys or clicks.
pub(crate) struct Present {
    /// The slide on screen.
    slide: usize,
    /// Temporarily cover the slide while speaking.
    blackout: bool,
    /// When that slide began, for its animations.
    started: Instant,
    /// Last mouse movement; the pointer hides after a moment of stillness.
    last_pointer_move: Instant,
}

impl KinettoApp {
    /// Goes full screen and presents from the current slide.
    pub(crate) fn start_presenting(&mut self, ctx: &egui::Context) {
        self.stop_playback();
        self.selected = None;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
        self.presenting = Some(Present { slide: self.slide, blackout: false, started: Instant::now(), last_pointer_move: Instant::now() });
        self.play_slide_sound();
    }

    /// Leaves full screen and returns to the editor at the slide shown.
    pub(crate) fn stop_presenting(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.presenting.take() {
            self.slide = p.slide;
        }
        self.stop_playback();
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
    }

    /// Shows `slide` from its start; with `None` or past the last slide, the
    /// show ends.
    fn go_to(&mut self, ctx: &egui::Context, slide: Option<usize>) {
        match slide {
            Some(slide) if slide < self.project().slides.len() => {
                if let Some(p) = &mut self.presenting {
                    p.slide = slide;
                    p.blackout = false;
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
        self.playback.playing = false;
    }

    /// Draws the presentation and handles its keys and clicks.
    pub(crate) fn present_view(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        let area = ui.max_rect();
        let controls = egui::Rect::from_center_size(area.center_bottom() - Vec2::new(0.0, 38.0), Vec2::new(440.0_f32.min(area.width()), 52.0));
        let Some(present) = self.presenting.as_mut() else { return };
        let over_controls = ctx.input(|i| i.pointer.interact_pos().is_some_and(|pos| controls.contains(pos)));
        // Space and Enter belong to a focused control button, if there is one.
        let control_focused = ctx.memory(|m| m.focused().is_some());
        let (next, prev, exit, first, last) = ctx.input(|i| {
            let pressed = |keys: &[Key]| keys.iter().any(|k| i.key_pressed(*k));
            (
                pressed(&[Key::ArrowRight, Key::PageDown, Key::ArrowDown]) || (!control_focused && pressed(&[Key::Space, Key::Enter])) || (i.pointer.primary_clicked() && !over_controls),
                pressed(&[Key::ArrowLeft, Key::PageUp, Key::Backspace, Key::ArrowUp]) || i.pointer.secondary_clicked(),
                pressed(&[Key::Escape]),
                pressed(&[Key::Home]),
                pressed(&[Key::End]),
            )
        });
        if ctx.input(|i| i.pointer.delta() != Vec2::ZERO) {
            present.last_pointer_move = Instant::now();
        }
        if ctx.input(|i| i.key_pressed(Key::B)) {
            present.blackout = !present.blackout;
            present.last_pointer_move = Instant::now();
        }
        let blackout = present.blackout;
        let current = present.slide;
        let elapsed = present.started.elapsed();
        let hide_cursor = present.last_pointer_move.elapsed() > CURSOR_HIDE && !over_controls;

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
            return self.go_to(&ctx, Some(count.saturating_sub(1)));
        }

        // Play the slide, then hold on its last frame.
        let slide = &self.project().slides[current];
        let local = Tick((elapsed.as_secs_f64() * kn_time::TICKS_PER_SECOND as f64) as i64).min(slide.duration - Tick(1));
        if local >= slide.duration - Tick(1) {
            self.playback.stop_sound();
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
        let fit = (area.width() / cw as f32).min(area.height() / ch as f32);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let rect = egui::Rect::from_center_size(area.center(), size);
        ui.painter().rect_filled(area, 0, egui::Color32::BLACK);
        if !blackout && let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let ppp = ctx.pixels_per_point();
            let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
            let f = Frame { project: self.session.project(), media: &self.session.media, time, size: px, stamp: None };
            let id = canvas.paint(rs, Surface::Stage, f, &mut self.render);
            egui::Image::new((id, size)).paint_at(ui, rect);
        }
        if !hide_cursor {
            let mut bar = ui.new_child(egui::UiBuilder::new().id_salt("present-controls").max_rect(controls));
            bar.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
            egui::Frame::new().fill(egui::Color32::from_black_alpha(225)).corner_radius(14).inner_margin(10).show(&mut bar, |ui| {
                ui.horizontal(|ui| {
                    if ui.add_enabled(current > 0, egui::Button::new("Previous")).clicked() {
                        self.go_to(&ctx, current.checked_sub(1));
                    }
                    ui.label(format!("{} / {}", current + 1, count));
                    if ui.button("Next").clicked() { self.go_to(&ctx, Some(current + 1)); }
                    if ui.button(if blackout { "Show slide" } else { "Blackout" }).on_hover_text("Cover the slide while speaking (B)").clicked()
                        && let Some(p) = &mut self.presenting { p.blackout = !p.blackout; }
                    if ui.button("Finish").on_hover_text("Return to the editor (Esc)").clicked() { self.stop_presenting(&ctx); }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn presentation_controls_navigate_cover_and_exit() {
        let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
        let ctx = h.ctx.clone();
        let app = h.state_mut();
        app.session = kn_engine::Session::new(kn_model::Project::sample());
        app.slide = 0;
        app.start_presenting(&ctx);
        h.run_steps(2);
        h.get_by_label("Blackout").click();
        h.run_steps(2);
        assert!(h.state().presenting.as_ref().unwrap().blackout);
        assert_eq!(h.state().presenting.as_ref().unwrap().slide, 0);
        h.get_by_label("Show slide").click();
        h.run_steps(2);
        assert!(!h.state().presenting.as_ref().unwrap().blackout);
        h.get_by_label("Next").click();
        h.run_steps(2);
        assert_eq!(h.state().presenting.as_ref().unwrap().slide, 1);
        h.get_by_label("Previous").click();
        h.run_steps(2);
        assert_eq!(h.state().presenting.as_ref().unwrap().slide, 0);
        h.get_by_label("Finish").click();
        h.run_steps(2);
        assert!(h.state().presenting.is_none());
    }
}
