//! The welcome page ("What are you making?") and the look picker.

use crate::i18n::tr;
use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::Icon;
use crate::theme::{self, CARD_RADIUS, palette};
use egui::{Color32, RichText, Sense, Stroke, Vec2};
use kn_engine::{Command, Session};
use kn_layout::{Category, THEMES, Theme};

/// Narrowest a template card may be before the grid drops a column.
const CARD_MIN_WIDTH: f32 = 224.0;

impl KinettoApp {
    /// Shown in place of the editor when starting something new.
    pub(crate) fn welcome(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = palette(ui.ctx());
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        egui::ScrollArea::vertical()
            .id_salt("welcome-page")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                let margin = if ui.available_width() < 800.0 { 20.0 } else { 36.0 };
                let width = (ui.available_width() - margin * 2.0).clamp(240.0, 1600.0);
                ui.allocate_ui_with_layout(Vec2::new(width, 0.0), adw::column(), |ui| {
                    ui.set_width(width);
                    ui.label(RichText::new(tr("Start with a story.")).family(theme::bold()).size(28.0));
                    ui.label(RichText::new(tr("Create from your ideas, or make a designed template your own.")).color(p.dim));
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        if adw::button(ui, Some(Icon::Open), "Open project…", Kind::Normal).clicked() { self.pick_open(); }
                        if adw::button(ui, Some(Icon::Copy), "Import slides…", Kind::Flat).on_hover_text(tr("Import PowerPoint or Markdown")).clicked() { self.pick_import(); }
                    });
                    ui.add_space(20.0);
                    egui::Frame::new().fill(p.card.lerp_to_gamma(p.accent_bg, 0.035)).stroke(Stroke::new(1.0, p.border)).corner_radius(16).inner_margin(22).show(ui, |ui| {
                        ui.set_width((width - 44.0).max(200.0));
                        let intro = |ui: &mut egui::Ui| {
                            ui.label(RichText::new(tr("QUICK VIDEO")).family(theme::bold()).size(11.0).color(p.accent));
                            ui.add_space(6.0);
                            ui.label(RichText::new(tr("Turn your ideas into a video.")).family(theme::bold()).size(21.0));
                            ui.label(RichText::new(tr("Add words and pictures, then choose a look.")).color(p.dim));
                        };
                        // `trailing` lays the buttons out from the right edge, so
                        // they are added in reverse to keep the same reading order.
                        let mut actions = |ui: &mut egui::Ui, trailing: bool| {
                            let create = |ui: &mut egui::Ui| adw::button(ui, Some(Icon::Add), "Create quick video", Kind::Suggested).clicked();
                            let sample = |ui: &mut egui::Ui| adw::button(ui, Some(Icon::Sparkle), "Try a sample", Kind::Normal).clicked();
                            let (created, sampled) = if trailing {
                                let sampled = sample(ui);
                                (create(ui), sampled)
                            } else {
                                (create(ui), sample(ui))
                            };
                            if created { self.open_quick(); }
                            if sampled { self.open_quick_sample(ui.ctx()); }
                        };
                        if width >= 1000.0 {
                            ui.horizontal(|ui| {
                                let text_width = ui.available_width() - 340.0;
                                ui.allocate_ui_with_layout(Vec2::new(text_width, 0.0), adw::column(), |ui| {
                                    ui.set_width(text_width);
                                    intro(ui);
                                });
                                ui.with_layout(adw::trailing(), |ui| actions(ui, true));
                            });
                        } else {
                            intro(ui);
                            ui.add_space(12.0);
                            ui.horizontal_wrapped(|ui| actions(ui, false));
                        }
                    });
                    self.recent_projects(ui);
                    ui.add_space(24.0);
                    ui.label(RichText::new(tr("Templates")).family(theme::bold()).size(18.0));
                    ui.label(RichText::new(tr("Choose a starting point and make it yours.")).color(p.dim));
                    ui.add_space(12.0);
                    self.category_chips(ui);
                    ui.add_space(16.0);
                    let columns = ((width + 16.0) / (CARD_MIN_WIDTH + 16.0)).floor().clamp(1.0, 6.0) as usize;
                    let card_width = (width - 16.0 * (columns - 1) as f32) / columns as f32;
                    let card_size = Vec2::new(card_width, (card_width - 16.0) * 9.0 / 16.0 + 126.0);
                    let mut picked = None;
                    // "All" shows every category under its own heading.
                    let shown: Vec<Category> = match self.template_category {
                        Some(category) => vec![category],
                        None => Category::ALL.to_vec(),
                    };
                    for category in shown {
                        let members: Vec<usize> = (0..self.templates.len()).filter(|&i| self.templates[i].0.category() == category).collect();
                        if members.is_empty() { continue; }
                        if self.template_category.is_none() {
                            ui.label(RichText::new(tr(category.name())).family(theme::semibold()).size(15.0));
                            ui.add_space(8.0);
                        }
                        crate::adw::left_to_right(ui, |ui| egui::Grid::new(("templates", category.name())).spacing([16.0, 16.0]).show(ui, |ui| {
                            for (n, &i) in members.iter().enumerate() {
                                if self.template_card(ui, frame, i, card_size).clicked() { picked = Some(i); }
                                if n % columns == columns - 1 { ui.end_row(); }
                            }
                        }));
                        ui.add_space(24.0);
                    }
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

    /// "All" and one chip per category; the chosen one filters the templates.
    fn category_chips(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            let options = std::iter::once(None).chain(Category::ALL.into_iter().map(Some));
            for option in options {
                let label = option.map_or("All", Category::name);
                if adw::chip(ui, label, self.template_category == option).clicked() {
                    self.template_category = option;
                }
            }
        });
    }

    /// One starter template: its first slide, name, description and details.
    /// Clicking it opens the template in the editor.
    fn template_card(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize, card_size: Vec2) -> egui::Response {
        let p = palette(ui.ctx());
        let (rect, response) = ui.allocate_exact_size(card_size, Sense::click());
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
        let preview = egui::Rect::from_min_size(rect.min + Vec2::splat(8.0), Vec2::new(card_size.x - 16.0, (card_size.x - 16.0) * 9.0 / 16.0));
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
        // The card's words start at the reading edge.
        let rtl = crate::i18n::rtl();
        let text_x = if rtl { rect.right() - 14.0 } else { rect.left() + 14.0 };
        let (top, centre) = if rtl { (egui::Align2::RIGHT_TOP, egui::Align2::RIGHT_CENTER) } else { (egui::Align2::LEFT_TOP, egui::Align2::LEFT_CENTER) };
        ui.painter().text(egui::pos2(text_x, preview.bottom() + 12.0), top, tr(name), egui::FontId::new(14.5, theme::bold()), p.fg);
        let description = adw::wrapped(ui, tr(template.description()), &egui::FontId::proportional(12.5), card_size.x - 28.0);
        let mut job = egui::text::LayoutJob::simple(description, egui::FontId::proportional(12.5), p.dim, card_size.x - 28.0);
        job.halign = if rtl { egui::Align::RIGHT } else { egui::Align::LEFT };
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(egui::pos2(text_x, preview.bottom() + 32.0), galley, Color32::PLACEHOLDER);
        let format = match project.aspect { kn_geom::Aspect::Tall9x16 => "Portrait", kn_geom::Aspect::Square1x1 => "Square", _ => "Landscape" };
        let count = project.slides.len();
        let slides = if count == 1 { tr("1 slide").to_owned() } else { crate::trf!("{n} slides", n = count) };
        let details = crate::i18n::row(&[&slides, tr(format)], " · ");
        ui.painter().text(egui::pos2(text_x, rect.bottom() - 18.0), centre, details, egui::FontId::proportional(11.5), p.dim);
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Named looks for the current slide.
    pub(crate) fn look_picker(&mut self, ui: &mut egui::Ui) {
        let current = self.project().slides[self.slide].theme_name(self.project()).to_owned();
        let columns = ((ui.available_width() + 8.0) / 64.0).floor().clamp(1.0, 4.0) as usize;
        let cell_width = (ui.available_width() - 8.0 * (columns - 1) as f32) / columns as f32;
        crate::adw::left_to_right(ui, |ui| egui::Grid::new("slide-looks").spacing(Vec2::new(8.0, 10.0)).show(ui, |ui| {
            for (i, theme) in THEMES.iter().enumerate() {
                let response = ui.allocate_ui_with_layout(Vec2::new(cell_width, 62.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let response = swatch(ui, theme, theme.name == current).on_hover_text(crate::i18n::row(&[tr(theme.name), &crate::trf!("{name} transition", name = crate::i18n::tr(theme.transition.name()))], " · "));
                    ui.label(RichText::new(tr(theme.name)).size(11.0));
                    response
                }).inner;
                if response.clicked() && theme.name != current {
                    self.run(Command::ApplySlideTheme { index: self.slide, theme: theme.name.into() });
                    self.session.seal();
                }
                if (i + 1) % columns == 0 { ui.end_row(); }
            }
        }));
    }
}

/// A small tile showing a look's background and accent colours.
pub(crate) fn swatch(ui: &mut egui::Ui, theme: &Theme, selected: bool) -> egui::Response {
    let p = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(62.0), 42.0), Sense::click());
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
