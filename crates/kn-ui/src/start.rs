//! The welcome page ("What are you making?") and the look picker.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::Icon;
use crate::theme::{self, CARD_RADIUS, palette};
use egui::{Color32, RichText, Sense, Stroke, Vec2};
use kn_engine::{Command, Session};
use kn_layout::{THEMES, Theme};

const CARD: Vec2 = Vec2::new(224.0, 212.0);
const PREVIEW_H: f32 = 126.0;

impl KinettoApp {
    /// Shown in place of the editor when starting something new.
    pub(crate) fn welcome(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = palette(ui.ctx());
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(48.0);
                ui.label(RichText::new("What are you making?").family(theme::bold()).size(28.0));
                ui.add_space(4.0);
                ui.label(RichText::new("Pick a starting point. You can change every word, colour and picture later.").color(p.dim));
                ui.add_space(28.0);

                let columns = ((ui.available_width() + 16.0) / (CARD.x + 16.0)).floor().clamp(1.0, 3.0) as usize;
                let width = columns as f32 * (CARD.x + 16.0) - 16.0;
                let mut picked = None;
                ui.allocate_ui(Vec2::new(width, 0.0), |ui| {
                    egui::Grid::new("templates").spacing([16.0, 16.0]).show(ui, |ui| {
                        for i in 0..self.templates.len() {
                            if self.template_card(ui, frame, i).clicked() {
                                picked = Some(i);
                            }
                            if i % columns == columns - 1 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.add_space(24.0);
                ui.label(RichText::new("Or start from something you already have").color(p.dim));
                ui.add_space(4.0);
                let row_width = 3.0 * 196.0 + 2.0 * 8.0;
                ui.allocate_ui(Vec2::new(row_width, 34.0), |ui| {
                    ui.horizontal(|ui| {
                        if adw::button(ui, Some(Icon::Text), "Paste your text…", Kind::Normal).clicked() {
                            self.pasting = Some(String::new());
                        }
                        if adw::button(ui, Some(Icon::Copy), "Import PowerPoint or Markdown…", Kind::Normal).clicked() {
                            self.pick_import();
                        }
                        if adw::button(ui, Some(Icon::Open), "Open a project…", Kind::Normal).clicked() {
                            self.pick_open();
                        }
                    });
                });
                ui.add_space(40.0);
                if let Some(i) = picked {
                    let project = self.templates[i].1.clone();
                    self.choosing_template = false;
                    self.replace_session(Session::new(project));
                }
            });
        });
    }

    fn template_card(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize) -> egui::Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
        let (template, project) = (&self.templates[i].0, &self.templates[i].1);
        let name = template.name();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let hovered = response.hovered();
        ui.painter().rect(
            rect,
            CARD_RADIUS,
            if hovered { p.card.lerp_to_gamma(p.accent_bg, 0.06) } else { p.card },
            Stroke::new(if hovered { 2.0 } else { 1.0 }, if hovered { p.accent_bg } else { p.border }),
            egui::StrokeKind::Inside,
        );

        // A live preview of the template's first slide, shown at rest.
        let preview = egui::Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::new(CARD.x - 16.0, PREVIEW_H));
        let (cw, ch) = project.aspect.canvas_size();
        let fit = (preview.width() / cw as f32).min(preview.height() / ch as f32);
        let size = Vec2::new(cw as f32 * fit, ch as f32 * fit);
        let shown = egui::Rect::from_center_size(preview.center(), size);
        ui.painter().rect_filled(preview, CARD_RADIUS - 4, p.button[0]);
        if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
            let ppp = ui.ctx().pixels_per_point();
            let px = [(size.x * ppp).round() as u32, (size.y * ppp).round() as u32];
            let time = project.slides[0].duration - kn_time::Tick::from_millis(10);
            let media = kn_media::MediaLibrary::new();
            let id = canvas.paint(rs, Surface::Template(i), Frame { project, media: &media, time, size: px, stamp: Some(0) }, &mut self.render);
            egui::Image::new((id, size)).corner_radius(6).paint_at(ui, shown);
        }
        let text_left = rect.left() + 14.0;
        ui.painter().text(egui::pos2(text_left, preview.bottom() + 12.0), egui::Align2::LEFT_TOP, name, egui::FontId::new(14.5, theme::bold()), p.fg);
        let galley = ui.painter().layout(template.description().into(), egui::FontId::proportional(12.5), p.dim, CARD.x - 28.0);
        ui.painter().galley(egui::pos2(text_left, preview.bottom() + 32.0), galley, Color32::PLACEHOLDER);
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Swatches for every look; clicking one restyles the whole video.
    pub(crate) fn look_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.project().theme.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            for theme in THEMES {
                let response = swatch(ui, theme, theme.name == current).on_hover_text(theme.name);
                if response.clicked() && theme.name != current {
                    self.run(Command::ApplyTheme { theme: theme.name.into() });
                    self.session.seal();
                }
            }
        });
    }
}

fn swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(62.0, 42.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, theme.name));
    let c = |c: kn_geom::Color| Color32::from_rgb(c.r, c.g, c.b);
    let inner = rect.shrink(3.0);
    ui.painter().rect(inner, 8, c(theme.background), Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
    for (i, accent) in theme.accents.iter().enumerate() {
        let centre = inner.left_center() + Vec2::new(14.0 + 14.0 * i as f32, 0.0);
        ui.painter().circle_filled(centre, 5.5, c(*accent));
    }
    if selected || response.hovered() {
        let color = if selected { p.accent_bg } else { p.border };
        ui.painter().rect_stroke(rect, 10, Stroke::new(2.0, color), egui::StrokeKind::Inside);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
