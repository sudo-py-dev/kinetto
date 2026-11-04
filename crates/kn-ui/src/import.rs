//! Making a video from existing material: pasted text, Markdown files and
//! PowerPoint decks.

use crate::adw::{self, Kind};
use crate::problem::Problem;
use crate::{KinettoApp, Pending};
use kn_engine::Session;
use kn_geom::Aspect;
use kn_media::MediaLibrary;
use std::path::Path;

pub(crate) const IMPORT_EXTENSIONS: &[&str] = &["pptx", "md", "markdown", "txt"];

impl KinettoApp {
    pub(crate) fn pick_import(&mut self) {
        let picked = rfd::FileDialog::new().add_filter("PowerPoint, Markdown or text", IMPORT_EXTENSIONS).pick_file();
        if let Some(path) = picked {
            self.request(Pending::Import(path));
        }
    }

    /// Replaces the open video with one made from `path`.
    pub(crate) fn import_file(&mut self, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > 300 * 1024 * 1024);
        if too_big {
            return self.report(Problem::too_big(path, 300));
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => return self.report(Problem::read(path, &err)),
        };
        let mut media = MediaLibrary::new();
        let outline = match ext.as_str() {
            "pptx" => match kn_import::parse_pptx(bytes, &mut media) {
                Ok(o) => o,
                Err(err) => return self.report(Problem::import(path, &err)),
            },
            _ => {
                let text = String::from_utf8_lossy(&bytes);
                let base = path.parent().unwrap_or(Path::new("."));
                kn_import::parse_markdown(&text, base, &mut media)
            }
        };
        self.finish_import(outline, media, Some(path));
    }

    fn finish_import(&mut self, outline: kn_import::Outline, media: MediaLibrary, from: Option<&Path>) {
        let theme = kn_layout::theme("Paper");
        match kn_import::build(&outline, Aspect::Wide16x9, theme) {
            Ok(project) => {
                let count = project.slides.len();
                let mut session = Session::new(project);
                session.media = media;
                self.choosing_template = false;
                self.replace_session(session);
                let note = match outline.warnings.len() {
                    0 => String::new(),
                    1 => format!(" {}.", outline.warnings[0]),
                    n => format!(" {n} things couldn't be brought in."),
                };
                self.notify(format!("Made {count} slide{} — pick a look on the right.{note}", if count == 1 { "" } else { "s" }));
            }
            Err(err) => match from {
                Some(path) => self.report(Problem::import(path, &err)),
                None => self.report(Problem::new("Couldn't make slides", "There's no text to make slides from. Type or paste some words first.")),
            },
        }
    }

    /// The "Paste your text" dialog.
    pub(crate) fn paste_dialog(&mut self, ctx: &egui::Context) {
        let Some(text) = self.pasting.as_mut() else { return };
        let p = crate::theme::palette(ctx);
        let style = ctx.style_of(ctx.theme());
        let frame = egui::Frame::new()
            .fill(style.visuals.window_fill)
            .corner_radius(crate::theme::DIALOG_RADIUS)
            .inner_margin(egui::Margin::same(24))
            .shadow(style.visuals.window_shadow);
        let mut action = None;
        egui::Modal::new(egui::Id::unique("paste")).frame(frame).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(egui::RichText::new("Make slides from text").family(crate::theme::bold()).size(19.0));
            ui.label(egui::RichText::new("Put a blank line between slides. The first line of each is its title; lines starting with “-” become points.").color(p.dim));
            ui.add_space(12.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY)
                        .hint_text("My trip to Japan\nTwo weeks, five cities\n\nWhat we saw\n- Temples\n- Mountains\n- Ramen"),
                );
            });
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = !text.trim().is_empty();
                let make = ui.add_enabled_ui(ready, |ui| adw::button(ui, None, "Make slides", Kind::Suggested)).inner;
                if make.on_disabled_hover_text("Type or paste some text first").clicked() {
                    action = Some(true);
                }
                if adw::button(ui, None, "Cancel", Kind::Normal).clicked() {
                    action = Some(false);
                }
            });
        });
        match action {
            Some(true) => {
                let text = self.pasting.take().unwrap_or_default();
                self.request(Pending::FromText(text));
            }
            Some(false) => self.pasting = None,
            None => {}
        }
    }

    /// Text with Markdown headings is read as Markdown, otherwise as plain
    /// paragraphs.
    pub(crate) fn import_text(&mut self, text: &str) {
        let mut media = MediaLibrary::new();
        let looks_like_markdown = text.lines().any(|l| l.trim_start().starts_with('#') || l.trim_start().starts_with('>'));
        let outline = if looks_like_markdown { kn_import::parse_markdown(text, Path::new("."), &mut media) } else { kn_import::parse_plain(text) };
        self.finish_import(outline, media, None);
    }
}
