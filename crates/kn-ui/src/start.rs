//! The welcome page ("What are you making?") and the look picker.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::Icon;
use crate::theme::{self, CARD_RADIUS, palette};
use egui::{Color32, RichText, Sense, Stroke, Vec2};
use kn_engine::{Command, Session};
use kn_layout::{THEMES, Theme};

const CARD: Vec2 = Vec2::new(224.0, 244.0);
const PREVIEW_H: f32 = 126.0;

impl KinettoApp {
    /// Shown in place of the editor when starting something new.
    pub(crate) fn welcome(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = palette(ui.ctx());
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                let width = ui.available_width().min(1000.0);
                ui.allocate_ui_with_layout(Vec2::new(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(width);
                    ui.label(RichText::new("Start with a story.").family(theme::bold()).size(28.0));
                    ui.label(RichText::new("Create from your ideas, or make a designed template your own.").color(p.dim));
                    ui.add_space(20.0);
                    egui::Frame::new().fill(p.card.lerp_to_gamma(p.accent_bg, 0.035)).stroke(Stroke::new(1.0, p.border)).corner_radius(16).inner_margin(22).show(ui, |ui| {
                        ui.set_width((width - 44.0).max(200.0));
                        ui.label(RichText::new("QUICK VIDEO").family(theme::bold()).size(11.0).color(p.accent));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Your ideas. A video in minutes.").family(theme::bold()).size(21.0));
                        ui.label(RichText::new("Add words and pictures. Choose a look, movement and music. Keep creating in the editor.").color(p.dim));
                        ui.add_space(12.0);
                        ui.horizontal_wrapped(|ui| {
                            if adw::button(ui, Some(Icon::Add), "Create quick video", Kind::Suggested).clicked() { self.open_quick(); }
                            if adw::button(ui, Some(Icon::Play), "Try a sample", Kind::Normal).clicked() { self.open_quick_sample(ui.ctx()); }
                        });
                    });
                    self.recent_projects(ui);
                    ui.add_space(24.0);
                    ui.label(RichText::new("Designed starting points").family(theme::bold()).size(18.0));
                    ui.label(RichText::new("Editable artwork, thoughtful motion and a matching soundtrack. All included.").color(p.dim));
                    ui.add_space(12.0);
                    let columns = ((width + 16.0) / (CARD.x + 16.0)).floor().clamp(1.0, 4.0) as usize;
                    let mut picked = None;
                    egui::Grid::new("templates").spacing([16.0, 16.0]).show(ui, |ui| {
                        for i in 0..self.templates.len() {
                            if self.template_card(ui, frame, i).clicked() { picked = Some(i); }
                            if i % columns == columns - 1 { ui.end_row(); }
                        }
                    });
                    ui.add_space(22.0);
                    ui.horizontal_wrapped(|ui| {
                        if adw::button(ui, Some(Icon::Copy), "Import slides…", Kind::Normal).on_hover_text("Import PowerPoint or Markdown").clicked() { self.pick_import(); }
                        if adw::button(ui, Some(Icon::Open), "Open project…", Kind::Normal).clicked() { self.pick_open(); }
                    });
                    if let Some(i) = picked {
                        let project = self.templates[i].1.clone();
                        self.choosing_template = false;
                        self.replace_session(Session::from_bundle(kn_engine::storage::Bundle { project, media: self.template_media.clone() }, None));
                    }
                    ui.add_space(32.0);
                });
            });
        });
    }

    fn template_card(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize) -> egui::Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
        let (template, project) = (&self.templates[i].0, &self.templates[i].1);
        let name = template.name();
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        let hovered = response.hovered() || response.has_focus();
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
            let time = project.slides[0].rest_time();
            let media = &self.template_media;
            let id = canvas.paint(rs, Surface::Template(i), Frame { project, media, time, size: px, stamp: Some(0) }, &mut self.render);
            egui::Image::new((id, size)).corner_radius(6).paint_at(ui, shown);
        }
        let text_left = rect.left() + 14.0;
        ui.painter().text(egui::pos2(text_left, preview.bottom() + 12.0), egui::Align2::LEFT_TOP, name, egui::FontId::new(14.5, theme::bold()), p.fg);
        let galley = ui.painter().layout(template.description().into(), egui::FontId::proportional(12.5), p.dim, CARD.x - 28.0);
        ui.painter().galley(egui::pos2(text_left, preview.bottom() + 32.0), galley, Color32::PLACEHOLDER);
        let format = match project.aspect { kn_geom::Aspect::Tall9x16 => "Portrait", kn_geom::Aspect::Square1x1 => "Square", _ => "Landscape" };
        let music = project.music.as_ref().and_then(|music| match music.source { kn_model::MusicSource::Mood(mood) => Some(mood.name()), _ => None });
        let details = format!("{} {} · {format}{}", project.slides.len(), if project.slides.len() == 1 { "slide" } else { "slides" }, music.map_or(String::new(), |m| format!(" · {m}")));
        ui.painter().text(egui::pos2(text_left, rect.bottom() - 18.0), egui::Align2::LEFT_CENTER, details, egui::FontId::proportional(10.5), p.dim);
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Named looks for the current slide.
    pub(crate) fn look_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.project().slides[self.slide].theme_name(self.project()).to_owned();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            for theme in THEMES {
                let response = ui.allocate_ui_with_layout(Vec2::new(62.0, 64.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                    let response = swatch(ui, theme, theme.name == current).on_hover_text(format!("{} · {} transition", theme.name, theme.transition.name()));
                    ui.label(RichText::new(theme.name).size(11.0));
                    response
                }).inner;
                if response.clicked() && theme.name != current {
                    self.run(Command::ApplySlideTheme { index: self.slide, theme: theme.name.into() });
                    self.session.seal();
                }
            }
        });
    }
}

pub(crate) fn swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
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
