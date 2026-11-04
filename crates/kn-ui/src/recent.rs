//! Project history contains paths only; removing an entry never removes a file.
use crate::{KinettoApp, Pending, adw};
use crate::theme::{self, palette};
use std::path::Path;

impl KinettoApp {
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

    fn save_project_history(&mut self) {
        if let Err(err) = self.settings.try_save() {
            tracing::warn!("couldn't save recent projects: {err}");
            self.notify("Recent projects updated for this session, but could not be saved");
        }
    }

    pub(crate) fn recent_projects(&mut self, ui: &mut egui::Ui) {
        let p = palette(ui.ctx());
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Recent projects").family(theme::bold()).size(18.0));
            if !self.settings.recent_projects.is_empty() && ui.button("Clear list").on_hover_text("Remove history entries; your project files stay on disk").clicked() {
                self.settings.recent_projects.clear();
                self.save_project_history();
            }
        });
        if self.settings.recent_projects.is_empty() {
            ui.label(egui::RichText::new("Projects you open or save will appear here.").color(p.dim));
            return;
        }
        let mut open = None;
        let mut remove = None;
        let mut locate = false;
        egui::ScrollArea::vertical().id_salt("recent-projects").max_height(340.0).show(ui, |ui| {
        adw::boxed_list(ui, |list| {
            for path in self.settings.recent_projects.clone() {
                list.full(|ui| {
                    ui.push_id(&path, |ui| {
                        ui.horizontal(|ui| {
                            let name = path.file_stem().unwrap_or_default().to_string_lossy();
                            if ui.button(name.as_ref()).on_hover_text("Open project").clicked() { open = Some(path.clone()); }
                            if adw::icon_button(ui, crate::icons::Icon::Delete, "Remove from recent projects").on_hover_text("Remove from this list; keep the project file").clicked() { remove = Some(path.clone()); }
                        });
                        ui.add(egui::Label::new(egui::RichText::new(path.display().to_string()).size(12.0).color(p.dim)).wrap());
                        match path.try_exists() {
                            Ok(true) => {},
                            Ok(false) => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new("File moved or missing").size(12.0).color(p.dim));
                                    if ui.button("Locate project…").clicked() { locate = true; }
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
