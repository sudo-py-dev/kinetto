//! Music and voice controls in the slide panel, and adding sound files.

use crate::KinettoApp;
use kn_engine::Command;
use kn_model::{Mood, Music, MusicSource};
use std::path::Path;

/// Dropped sounds longer than this become music; shorter ones a voice-over.
const MUSIC_MIN_SECONDS: f64 = 60.0;
/// Room after a voice before the slide moves on.
const VOICE_TAIL_MILLIS: i64 = 600;

impl KinettoApp {
    pub(crate) fn music_controls(&mut self, ui: &mut egui::Ui) {
        let current = self.project().music.clone();
        let mut choice = current.as_ref().map(|m| m.source.clone());
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut choice, None, "None");
            for mood in Mood::ALL {
                ui.selectable_value(&mut choice, Some(MusicSource::Mood(mood)), mood.name());
            }
            let own = matches!(choice, Some(MusicSource::File(_)));
            if ui.selectable_label(own, "My music…").on_hover_text("Use your own song (MP3, WAV, FLAC, OGG, M4A)").clicked()
                && let Some(id) = self.pick_sound().map(|(id, _)| id)
            {
                choice = Some(MusicSource::File(id));
            }
        });
        if choice != current.as_ref().map(|m| m.source.clone()) {
            let volume = current.as_ref().map_or(0.6, |m| m.volume);
            self.run(Command::SetMusic { music: choice.map(|source| Music { source, volume }) });
            self.session.seal();
        }
        if let Some(music) = current {
            let mut volume = music.volume;
            let r = ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false).text("Volume"));
            if r.changed() {
                self.run(Command::SetMusic { music: Some(Music { volume, ..music }) });
            }
            if r.drag_stopped() {
                self.session.seal();
            }
            ui.label(egui::RichText::new("Music gets quieter by itself while someone speaks.").weak());
        }
    }

    pub(crate) fn voice_controls(&mut self, ui: &mut egui::Ui, index: usize) {
        if let Some((slide, recorder)) = &self.recording {
            if *slide != index {
                ui.label(format!("Recording for slide {}…", slide + 1));
                return;
            }
            let secs = recorder.seconds();
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::from_rgb(230, 60, 60), "\u{25cf}");
                ui.label(format!("Recording  {}:{:02}", secs as u64 / 60, secs as u64 % 60));
            });
            ui.add(egui::ProgressBar::new(recorder.level().min(1.0)).desired_height(6.0));
            if ui.button("Stop recording").clicked() {
                self.finish_recording();
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        let voice = self.project().slides[index].voice.clone();
        ui.horizontal_wrapped(|ui| match voice {
            Some(_) => {
                ui.label("This slide has a voice-over.");
                if ui.button("Remove").clicked() {
                    self.run(Command::SetVoice { index, voice: None, fit_millis: 0 });
                    self.session.seal();
                }
            }
            None => {
                if ui.button("Record voice").on_hover_text("Talk into your microphone; press Stop when you're done").clicked() {
                    self.stop_playback();
                    match crate::device::Recorder::start() {
                        Ok(recorder) => self.recording = Some((index, recorder)),
                        Err(message) => self.notify(message),
                    }
                }
                if ui.button("Add voice from a file…").clicked()
                    && let Some((id, secs)) = self.pick_sound()
                {
                    self.set_voice(index, id, secs);
                }
            }
        });
    }

    fn finish_recording(&mut self) {
        let Some((index, recorder)) = self.recording.take() else { return };
        let take = match recorder.finish() {
            Ok(take) => take,
            Err(message) => return self.notify(message),
        };
        let seconds = take.seconds();
        match self.session.media.import_audio(take.to_wav()) {
            Ok(id) => self.set_voice(index, id, seconds),
            Err(err) => self.notify(err.to_string()),
        }
    }

    fn set_voice(&mut self, index: usize, id: String, seconds: f64) {
        let fit_millis = (seconds * 1000.0).ceil() as i64 + VOICE_TAIL_MILLIS;
        self.run(Command::SetVoice { index, voice: Some(id), fit_millis });
        self.session.seal();
    }

    fn pick_sound(&mut self) -> Option<(String, f64)> {
        let path = rfd::FileDialog::new().add_filter("Sound", kn_media::AUDIO_EXTENSIONS).pick_file()?;
        self.import_sound_file(&path)
    }

    /// Checks the file decodes, then adds it; returns its id and length.
    pub(crate) fn import_sound_file(&mut self, path: &Path) -> Option<(String, f64)> {
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > kn_media::MAX_FILE_BYTES as u64);
        if too_big {
            self.notify("That sound file is over 100 MB.");
            return None;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                self.notify(format!("Couldn't read that file: {err}"));
                return None;
            }
        };
        let seconds = match kn_audio::decode(&bytes) {
            Ok(pcm) => pcm.seconds(),
            Err(err) => {
                self.notify(err.to_string());
                return None;
            }
        };
        match self.session.media.import_audio(bytes) {
            Ok(id) => Some((id, seconds)),
            Err(err) => {
                self.notify(err.to_string());
                None
            }
        }
    }

    /// A dropped sound: long ones become the music, short ones this
    /// slide's voice-over.
    pub(crate) fn add_dropped_sound(&mut self, path: &Path) {
        let Some((id, seconds)) = self.import_sound_file(path) else { return };
        if seconds >= MUSIC_MIN_SECONDS {
            let volume = self.project().music.as_ref().map_or(0.6, |m| m.volume);
            self.run(Command::SetMusic { music: Some(Music { source: MusicSource::File(id), volume }) });
            self.session.seal();
            self.notify("Added as background music.");
        } else {
            self.set_voice(self.slide, id, seconds);
            self.notify(format!("Added as the voice for slide {}.", self.slide + 1));
        }
    }
}
