//! Project history contains paths only; removing an entry never removes a file.
use crate::{KinettoApp, Pending, adw};
use crate::theme::{self, palette};
use std::path::Path;

/// Tallest the Recent projects list grows before it scrolls.
pub(crate) const RECENT_HEIGHT: f32 = 380.0;

impl KinettoApp {
    /// Puts `path` at the top of Recent projects and saves the list.
    pub(crate) fn remember_project(&mut self, path: &Path) {
        match self.settings.remember_project(path) {
            Ok(true) => self.save_project_history(),
            Ok(false) => {},
            Err(err) => {
                tracing::warn!("couldn't add recent project {}: {err}", path.display());
                self.notify("Project opened or saved, but could not add it to Recent projects");
            }
        }
    }

    /// Saves the settings after the recent list changed; failures become a toast.
    fn save_project_history(&mut self) {
        if let Err(err) = self.settings.try_save() {
            tracing::warn!("couldn't save recent projects: {err}");
            self.notify("Recent projects updated for this session, but could not be saved");
        }
    }

    /// The welcome page's Recent projects list: open, remove or clear.
    pub(crate) fn recent_projects(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Recent projects").family(theme::bold()).size(18.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.settings.recent_projects.is_empty() && adw::button(ui, None, "Clear list", adw::Kind::Flat).on_hover_text("Remove history entries; your project files stay on disk").clicked() {
                    self.settings.recent_projects.clear();
                    self.save_project_history();
                }
            });
        });
        ui.add_space(8.0);
        if self.settings.recent_projects.is_empty() {
            adw::boxed_list(ui, |list| list.full(|ui| {
                ui.label(egui::RichText::new("No recent projects yet").family(theme::semibold()));
                ui.label(egui::RichText::new("Open a project or create a video to get started.").size(12.5).color(p.dim));
            }));
            return;
        }
        let mut open = None;
        let mut remove = None;
        let mut locate = false;
        // About five projects show; a longer history scrolls inside the card
        // so the templates below stay close.
        egui::ScrollArea::vertical().id_salt("recent-projects").max_height(RECENT_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
        adw::boxed_list(ui, |list| {
            for path in self.settings.recent_projects.clone() {
                list.full(|ui| {
                    ui.push_id(&path, |ui| {
                        ui.horizontal(|ui| {
                            let name = path.file_stem().unwrap_or_default().to_string_lossy();
                            let width = (ui.available_width() - 42.0).max(100.0);
                            let response = ui.add_sized([width, 58.0], egui::Button::new("").frame_when_inactive(false));
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Open {name}")));
                            let rect = response.rect;
                            let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 22.0, rect.center().y), egui::Vec2::splat(20.0));
                            crate::icons::paint(ui.painter(), icon, crate::icons::Icon::Video, p.dim);
                            let left = rect.left() + 48.0;
                            let painter = ui.painter().with_clip_rect(rect.shrink(6.0));
                            let text = |value: String, font: egui::FontId, color| {
                                let mut job = egui::text::LayoutJob::simple(value, font, color, (rect.right() - left - 12.0).max(24.0));
                                job.wrap.max_rows = 1;
                                job.wrap.break_anywhere = true;
                                painter.layout_job(job)
                            };
                            let title = text(name.to_string(), egui::FontId::new(14.0, theme::semibold()), p.fg);
                            painter.galley(egui::pos2(left, rect.top() + 9.0), title, p.fg);
                            let folder = path.parent().unwrap_or(&path).display().to_string();
                            let location = text(folder, egui::FontId::proportional(12.0), p.dim);
                            painter.galley(egui::pos2(left, rect.top() + 32.0), location, p.dim);
                            if response.on_hover_text(path.display().to_string()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { open = Some(path.clone()); }
                            if adw::icon_button(ui, crate::icons::Icon::Close, "Remove from recent projects").on_hover_text("Remove from this list; keep the project file").clicked() { remove = Some(path.clone()); }
                        });
                        match path.try_exists() {
                            Ok(true) => {},
                            Ok(false) => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new("File moved or missing").size(12.0).color(p.dim));
                                    if adw::button(ui, Some(crate::icons::Icon::Open), "Locate project…", adw::Kind::Flat).clicked() { locate = true; }
                                });
                            }
                            Err(_) => { ui.label(egui::RichText::new("File location is currently unavailable").size(12.0).color(p.dim)); }
                        }
                    });
                });
            }
        });
        });
        if let Some(path) = remove {
            self.settings.recent_projects.retain(|p| p != &path);
            self.save_project_history();
        }
        if locate { self.pick_open(); }
        if let Some(path) = open { self.request(Pending::Open(path)); }
    }
}
