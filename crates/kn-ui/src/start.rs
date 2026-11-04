//! The start screen ("What are you making?") and the look picker.

use crate::KinettoApp;
use egui::{Color32, RichText, Sense, Vec2};
use kn_engine::{Command, Session};
use kn_layout::{THEMES, Template, Theme};

const CARD: Vec2 = Vec2::new(200.0, 120.0);

impl KinettoApp {
    pub(crate) fn start_screen(&mut self, ctx: &egui::Context) {
        if !self.choosing_template || self.restorable.is_some() {
            return;
        }
        let mut picked = None;
        let mut open = false;
        egui::Modal::new(egui::Id::unique("start")).show(ctx, |ui| {
            ui.heading("What are you making?");
            ui.label(RichText::new("Pick a starting point. You can change everything later.").weak());
            ui.add_space(12.0);
            egui::Grid::new("templates").spacing([12.0, 12.0]).show(ui, |ui| {
                for (i, template) in Template::ALL.into_iter().enumerate() {
                    if template_card(ui, template).clicked() {
                        picked = Some(template);
                    }
                    if i % 3 == 2 {
                        ui.end_row();
                    }
                }
            });
            ui.add_space(12.0);
            if ui.link("Open a project instead…").clicked() {
                open = true;
            }
        });
        if let Some(template) = picked {
            self.choosing_template = false;
            self.replace_session(Session::new(kn_layout::template(template)));
        }
        if open {
            self.choosing_template = false;
            self.pick_open();
        }
    }

    /// Swatches for every look; clicking one restyles the whole video.
    pub(crate) fn look_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.project().theme.clone();
        ui.horizontal_wrapped(|ui| {
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

fn template_card(ui: &mut egui::Ui, template: Template) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(rect, 10, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
    let inner = rect.shrink(12.0);
    painter.text(inner.left_top(), egui::Align2::LEFT_TOP, template.name(), egui::FontId::proportional(17.0), visuals.text_color());
    let galley = painter.layout(
        template.description().into(),
        egui::FontId::proportional(13.0),
        ui.visuals().weak_text_color(),
        inner.width(),
    );
    painter.galley(inner.left_top() + Vec2::new(0.0, 28.0), galley, Color32::PLACEHOLDER);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(52.0, 36.0), Sense::click());
    let painter = ui.painter();
    let c = |c: kn_geom::Color| Color32::from_rgb(c.r, c.g, c.b);
    let stroke = if selected {
        egui::Stroke::new(3.0, ui.visuals().selection.bg_fill)
    } else {
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    painter.rect(rect, 6, c(theme.background), stroke, egui::StrokeKind::Outside);
    for (i, accent) in theme.accents.iter().enumerate() {
        let centre = rect.left_center() + Vec2::new(12.0 + 14.0 * i as f32, 0.0);
        painter.circle_filled(centre, 5.0, c(*accent));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
