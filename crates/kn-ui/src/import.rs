//! Making a video from existing material: Markdown files and PowerPoint
//! decks. Typed text and pictures go through [`crate::quick`].

use crate::problem::Problem;
use crate::quick::QuickRequest;
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
        let theme = kn_layout::theme("Paper");
        self.finish_import(outline, media, Some(path), Aspect::Wide16x9, theme, kn_import::Style::default(), &mut kn_import::Dice::new(0), None);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish_import(
        &mut self,
        outline: kn_import::Outline,
        media: MediaLibrary,
        from: Option<&Path>,
        aspect: Aspect,
        theme: &kn_layout::Theme,
        style: kn_import::Style,
        dice: &mut kn_import::Dice,
        again: Option<Box<QuickRequest>>,
    ) {
        match kn_import::build(&outline, aspect, theme) {
            Ok(mut project) => {
                kn_import::animate(&mut project, style, dice);
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
                let s = if count == 1 { "" } else { "s" };
                let message = if again.is_some() {
                    format!("Made {count} slide{s} in the {} look.{note}", theme.name)
                } else {
                    format!("Made {count} slide{s} — pick a look on the right.{note}")
                };
                self.notify(message);
                if let Some(toast) = self.toast.as_mut() {
                    toast.again = again;
                }
            }
            Err(err) => match from {
                Some(path) => self.report(Problem::import(path, &err)),
                None => self.report(Problem::new("Couldn't make slides", "There's no text to make slides from. Type or paste some words first.")),
            },
        }
    }
}
