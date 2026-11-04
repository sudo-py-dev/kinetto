//! File dialogs on their own thread. The editor keeps drawing while one is
//! open, so the click that closes the dialog can't land on a control (such
//! as an item in a still-open dropdown) in a half-finished frame.

use crate::export::Output;
use crate::{KinettoApp, Pending, PICTURE_EXTENSIONS};
use kn_model::MusicSource;
use std::path::PathBuf;
use std::sync::mpsc;

/// What the picked file is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Purpose {
    Music,
    /// The voice of this slide.
    Voice(usize),
    /// Pictures for the current slide.
    Pictures,
    /// A PowerPoint, Markdown or text file to make slides from.
    Import,
    /// A project to open.
    Open,
    /// Where to save a finished video, GIF or PDF.
    Export(Output),
}

impl Purpose {
    /// Whether the answer only makes sense for the project it was asked for.
    fn tied_to_project(self) -> bool {
        !matches!(self, Purpose::Import | Purpose::Open)
    }

    /// The dialog to show, and whether it saves rather than opens.
    fn dialog(self, save_name: Option<&str>) -> (rfd::FileDialog, Mode) {
        let d = rfd::FileDialog::new();
        match self {
            Purpose::Music | Purpose::Voice(_) => (d.add_filter("Sound", kn_media::AUDIO_EXTENSIONS), Mode::One),
            Purpose::Pictures => (d.add_filter("Pictures", PICTURE_EXTENSIONS), Mode::Many),
            Purpose::Import => (d.add_filter("PowerPoint, Markdown or text", crate::import::IMPORT_EXTENSIONS), Mode::One),
            Purpose::Open => (crate::file_dialog(), Mode::One),
            Purpose::Export(output) => {
                let filter = match output {
                    Output::Video => "MP4 video",
                    Output::Gif => "Animated GIF",
                    Output::Pdf => "PDF document",
                };
                let d = d.add_filter(filter, &[output.ext()]);
                (match save_name {
                    Some(name) => d.set_file_name(name),
                    None => d,
                }, Mode::Save)
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Mode {
    One,
    Many,
    Save,
}

/// A file dialog that is open.
pub(crate) struct FilePick {
    pub(crate) purpose: Purpose,
    /// The project it was asked for.
    pub(crate) generation: u64,
    /// The chosen files; empty when cancelled.
    pub(crate) answer: mpsc::Receiver<Vec<PathBuf>>,
}

impl KinettoApp {
    /// Opens the file dialog for `purpose` without blocking the editor; the
    /// answer is used by [`Self::poll_file_pick`]. Ignored while another
    /// dialog is open. `save_name` is the suggested name when saving.
    pub(crate) fn ask_for_file(&mut self, purpose: Purpose, save_name: Option<&str>) {
        if self.file_pick.is_some() {
            return;
        }
        let (dialog, mode) = purpose.dialog(save_name);
        let (send, answer) = mpsc::channel();
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new().name("kinetto-file-dialog".into()).spawn(move || {
            let paths = match mode {
                Mode::One => dialog.pick_file().into_iter().collect(),
                Mode::Many => dialog.pick_files().unwrap_or_default(),
                Mode::Save => dialog.save_file().into_iter().collect(),
            };
            let _ = send.send(paths);
            ctx.request_repaint();
        });
        if spawned.is_ok() {
            self.file_pick = Some(FilePick { purpose, generation: self.generation, answer });
        }
    }

    /// Uses the files picked in the open dialog, once it closes. Cancelling
    /// changes nothing. `frame` lets an export share the editor's GPU.
    pub(crate) fn poll_file_pick(&mut self, frame: Option<&eframe::Frame>) {
        let Some(pick) = &self.file_pick else { return };
        let paths = match pick.answer.try_recv() {
            Ok(paths) => paths,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let FilePick { purpose, generation, .. } = self.file_pick.take().expect("checked above");
        if paths.is_empty() || (purpose.tied_to_project() && generation != self.generation) {
            return;
        }
        match purpose {
            Purpose::Music => {
                if let Some((id, _)) = self.import_sound_file(&paths[0]) {
                    self.choose_soundtrack(Some(MusicSource::File(id)));
                }
            }
            Purpose::Voice(index) => {
                if index < self.project().slides.len()
                    && let Some((id, seconds)) = self.import_sound_file(&paths[0])
                {
                    self.set_voice(index, id, seconds);
                }
            }
            Purpose::Pictures => {
                for path in &paths {
                    self.add_picture_file(path);
                }
            }
            Purpose::Import => self.request(Pending::Import(paths[0].clone())),
            Purpose::Open => self.request(Pending::Open(paths[0].clone())),
            Purpose::Export(output) => self.start_export(output, paths[0].clone(), frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use kn_model::Mood;

    fn answered(purpose: Purpose, generation: u64, paths: Vec<PathBuf>) -> FilePick {
        let (send, answer) = mpsc::channel();
        send.send(paths).unwrap();
        FilePick { purpose, generation, answer }
    }

    #[test]
    fn cancelling_a_file_dialog_changes_nothing_and_picked_files_are_used() {
        let mut h = Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
        let app = h.state_mut();
        app.session = kn_engine::Session::new(kn_model::Project::sample());
        app.generation = 0;
        let before = app.project().clone();

        // Cancelled, for every kind of dialog: nothing changes.
        for purpose in [Purpose::Music, Purpose::Voice(0), Purpose::Pictures, Purpose::Import, Purpose::Open, Purpose::Export(Output::Video)] {
            app.file_pick = Some(answered(purpose, 0, Vec::new()));
            app.poll_file_pick(None);
            assert!(app.file_pick.is_none());
            assert_eq!(*app.project(), before, "cancelling {purpose:?} changed the project");
            assert!(app.export.is_none() && app.pending.is_none() && app.problem.is_none());
        }

        // Still open: nothing happens yet, and no second dialog opens.
        let (_keep, answer) = mpsc::channel();
        app.file_pick = Some(FilePick { purpose: Purpose::Music, generation: 0, answer });
        app.ask_for_file(Purpose::Pictures, None);
        app.poll_file_pick(None);
        assert_eq!(app.file_pick.as_ref().map(|p| p.purpose), Some(Purpose::Music));
        assert!(app.project().music.is_none());
        app.file_pick = None;

        // Picked: the sound becomes the music.
        let dir = std::env::temp_dir().join(format!("kinetto-file-pick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("song.wav");
        std::fs::write(&song, kn_audio::generate(Mood::Calm, kn_audio::RATE as usize).to_wav()).unwrap();
        app.file_pick = Some(answered(Purpose::Music, 0, vec![song.clone()]));
        app.poll_file_pick(None);
        assert!(matches!(app.project().music.as_ref().map(|m| &m.source), Some(MusicSource::File(_))));

        // An answer for a project that has since been replaced is ignored.
        app.file_pick = Some(answered(Purpose::Voice(0), 0, vec![song]));
        app.generation = 1;
        app.poll_file_pick(None);
        assert!(app.project().slides[0].voice.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
