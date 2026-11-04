//! Music and voice controls in the slide panel, and adding sound files.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::icons::Icon;
use kn_engine::Command;
use kn_model::{Mood, Music, MusicSource};
use std::path::Path;

/// Dropped sounds longer than this become music; shorter ones a voice-over.
const MUSIC_MIN_SECONDS: f64 = 60.0;
/// Room after a voice before the slide moves on.
const VOICE_TAIL_MILLIS: i64 = 600;

impl KinettoApp {
    /// The inspector rows for music: mood or your own song, volume and placement.
    pub(crate) fn music_rows(&mut self, list: &mut adw::List<'_>) {
        let current = self.project().music.clone();
        let label = match current.as_ref().map(|m| &m.source) {
            None => "None",
            Some(MusicSource::Mood(m)) => m.name(),
            Some(MusicSource::File(_)) => "My music",
        };
        let mut choice = current.as_ref().map(|m| m.source.clone());
        let mut pick_file = false;
        let mut selected_source = false;
        list.row("Music", None, |ui| {
            adw::dropdown(ui, "music", label, |ui| {
                selected_source |= ui.selectable_value(&mut choice, None, "None").clicked();
                for mood in Mood::ALL {
                    selected_source |= ui.selectable_value(&mut choice, Some(MusicSource::Mood(mood)), mood.name()).on_hover_text(mood.description()).clicked();
                }
                if ui.selectable_label(matches!(choice, Some(MusicSource::File(_))), "My music…").clicked() {
                    pick_file = true;
                }
            });
        });
        if let Some(Music { source: MusicSource::Mood(mood), .. }) = &current {
            list.full(|ui| { ui.label(egui::RichText::new(mood.description()).size(12.0).color(crate::theme::palette(ui.ctx()).dim)); });
        }
        if pick_file && let Some((id, _)) = self.pick_sound() {
            choice = Some(MusicSource::File(id));
        }
        if selected_source || choice != current.as_ref().map(|m| m.source.clone()) {
            self.choose_soundtrack(choice);
        }

        if let Some(music) = current.clone() {
            list.full(|ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, at) in [
                        ("Start at this slide", self.slide_start(self.slide)),
                        ("Start at playhead", self.playhead()),
                        ("From video start", kn_time::Tick::ZERO),
                    ] {
                        if ui.button(label).clicked() {
                            let mut placed = music.clone();
                            placed.move_to((at.as_secs_f64() * 1000.0).round() as i64, self.project().total_duration());
                            self.run(Command::SetMusic { music: Some(placed) });
                            self.session.seal();
                        }
                    }
                });
                ui.label(egui::RichText::new("Music uses video time. Drag its clip freely in Timeline; hold Shift to snap.").size(12.0).color(crate::theme::palette(ui.ctx()).dim));
            });
            self.music_placement_rows(list, &music);
        }
        if let Some(music) = current {
            list.row("Music volume", Some(&format!("{:.0}%", music.volume * 100.0)), |ui| {
                let mut volume = f64::from(music.volume);
                let r = adw::slider(ui, &mut volume, 0.0..=1.0, 0.05, "Music volume");
                let volume = volume as f32;
                if r.changed() {
                    self.run(Command::SetMusic { music: Some(Music { volume, ..music.clone() }) });
                }
                if r.drag_stopped() {
                    self.session.seal();
                }
            });
        }
    }

    /// Sets the soundtrack (or none) as one undo step, placed from the current slide.
    pub(crate) fn choose_soundtrack(&mut self, source: Option<MusicSource>) {
        let current = self.project().music.as_ref();
        let start = (self.slide_start(self.slide).as_secs_f64() * 1000.0).round() as i64;
        let music = source.map(|source| {
            let mut music = Music::new(source, current.map_or(0.6, |m| m.volume));
            music.start_ms = start;
            music.looped = current.is_none_or(|m| m.looped);
            music
        });
        self.run(Command::SetMusic { music });
        self.session.seal();
    }

    /// Where the music sits in the video, matching the music bar on the
    /// timeline.
    fn music_placement_rows(&mut self, list: &mut adw::List<'_>, music: &Music) {
        let total = self.project().total_duration();
        let total_s = total.as_secs_f64();
        let (start, end) = music.span(total);
        let (start_s, end_s) = (start.as_secs_f64(), end.as_secs_f64());
        let clock = |s: f64| format!("{}:{:04.1}", (s / 60.0) as u64, s % 60.0);
        let mut changed = music.clone();
        let mut sealed = false;


        list.row("Music starts", Some(&format!("At {}", clock(start_s))), |ui| {
            let mut s = start_s;
            let r = adw::slider(ui, &mut s, 0.0..=(total_s - 0.5).max(0.0), 0.1, "Music starts at");
            if r.changed() {
                changed.move_to((s * 1000.0).round() as i64, total);
            }
            sealed |= r.drag_stopped();
        });
        let mut until_end = music.end_ms.is_none();
        let subtitle = if until_end { "Until the video ends".to_owned() } else { format!("At {}", clock(end_s)) };
        list.row("Music stops", Some(&subtitle), |ui| {
            if adw::switch(ui, &mut until_end, "Until the video ends").changed() {
                changed.end_ms = if until_end { None } else { Some(((total_s - 1.0).max(start_s + 0.5) * 1000.0) as i64) };
                sealed = true;
            }
        });
        if !until_end {
            list.row("Stop time", Some(&clock(end_s)), |ui| {
                let mut e = end_s;
                let r = adw::slider(ui, &mut e, (start_s + 0.5)..=total_s, 0.1, "Music stops at");
                if r.changed() { changed.end_ms = Some((e * 1000.0).round() as i64); }
                sealed |= r.drag_stopped();
            });
        }
        if let MusicSource::File(id) = &music.source {
            let song = self.audio_cache.get(&self.session.media, id).map_or(0.0, |pcm| pcm.seconds());
            if song > 1.0 {
                let offset = music.offset_ms as f64 / 1000.0;
                list.row("Song begins from", Some(&format!("{} into the song, to skip an intro", clock(offset))), |ui| {
                    let mut o = offset;
                    let r = adw::slider(ui, &mut o, 0.0..=(song - 0.5), 0.1, "Song begins from");
                    if r.changed() {
                        changed.offset_ms = (o * 1000.0).round() as i64;
                    }
                    sealed |= r.drag_stopped();
                });
            }
        }
        list.row("Repeat the song", Some("If it ends before the music stops"), |ui| {
            if adw::switch(ui, &mut changed.looped, "Repeat the song").changed() {
                sealed = true;
            }
        });

        if changed != *music {
            self.run(Command::SetMusic { music: Some(changed) });
        }
        if sealed {
            self.session.seal();
        }
    }

    /// The inspector rows for slide `index`'s voice: record, add a file, play or remove.
    pub(crate) fn voice_rows(&mut self, list: &mut adw::List<'_>, index: usize) {
        if let Some((slide, recorder)) = &self.recording {
            let (slide, secs, level) = (*slide, recorder.seconds(), recorder.level());
            if slide != index {
                list.row("Voice", Some(&format!("Recording for slide {}…", slide + 1)), |_| {});
                return;
            }
            let mut stop = false;
            list.row("Recording", Some(&format!("{}:{:02}", secs as u64 / 60, secs as u64 % 60)), |ui| {
                stop = adw::button(ui, Some(Icon::Stop), "Stop", Kind::Destructive).clicked();
                ui.add(egui::ProgressBar::new(level.min(1.0)).desired_width(70.0).desired_height(6.0).fill(crate::theme::palette(ui.ctx()).accent_bg));
            });
            if stop {
                self.finish_recording();
            }
            list.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            return;
        }
        let has_voice = self.project().slides[index].voice.is_some();
        let (mut record, mut from_file, mut remove) = (false, false, false);
        let subtitle = if has_voice { "This slide has a voice-over" } else { "Talk over this slide" };
        list.row("Voice", Some(subtitle), |ui| {
            if has_voice {
                remove = adw::icon_button(ui, Icon::Delete, "Remove the voice").clicked();
            } else {
                from_file = adw::icon_button(ui, Icon::Open, "Use a sound file").clicked();
                record = adw::button(ui, Some(Icon::Microphone), "Record", Kind::Normal).on_hover_text("Talk into your microphone, then press Stop").clicked();
            }
        });
        if remove {
            self.run(Command::SetVoice { index, voice: None, fit_millis: 0 });
            self.session.seal();
            self.notify_undo("Voice removed");
        }
        if record {
            self.stop_playback();
            match crate::device::Recorder::start() {
                Ok(recorder) => self.recording = Some((index, recorder)),
                Err(message) => self.report(crate::problem::Problem::new("Couldn't start recording", message)),
            }
        }
        if from_file && let Some((id, secs)) = self.pick_sound() {
            self.set_voice(index, id, secs);
        }
    }

    /// Stops a voice recording in progress and attaches it to its slide.
    pub(crate) fn finish_recording(&mut self) {
        let Some((index, recorder)) = self.recording.take() else { return };
        let take = match recorder.finish() {
            Ok(take) => take,
            Err(message) => return self.report(crate::problem::Problem::new("The recording didn't work", message)),
        };
        let seconds = take.seconds();
        match self.session.media.import_audio(take.to_wav()) {
            Ok(id) => self.set_voice(index, id, seconds),
            Err(_) => self.report(crate::problem::Problem::new("The recording is too long", "Recordings can be up to about 15 minutes. Record a shorter take, or split it across slides.")),
        }
    }

    /// Gives slide `index` the voice `id`, lengthening the slide to fit it.
    fn set_voice(&mut self, index: usize, id: String, seconds: f64) {
        let fit_millis = (seconds * 1000.0).ceil() as i64 + VOICE_TAIL_MILLIS;
        self.run(Command::SetVoice { index, voice: Some(id), fit_millis });
        self.session.seal();
    }

    /// Asks for a sound file and imports it; returns its id and length in seconds.
    fn pick_sound(&mut self) -> Option<(String, f64)> {
        let path = rfd::FileDialog::new().add_filter("Sound", kn_media::AUDIO_EXTENSIONS).pick_file()?;
        self.import_sound_file(&path)
    }

    /// Checks the file decodes, then adds it; returns its id and length.
    pub(crate) fn import_sound_file(&mut self, path: &Path) -> Option<(String, f64)> {
        use crate::problem::Problem;
        let too_big = std::fs::metadata(path).is_ok_and(|m| m.len() > kn_media::MAX_FILE_BYTES as u64);
        if too_big {
            self.report(Problem::too_big(path, 100));
            return None;
        }
        let name = path.file_name().map_or_else(|| "the sound".to_owned(), |n| format!("\u{201c}{}\u{201d}", n.to_string_lossy()));
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(err) => {
                self.report(Problem::read(path, &err));
                return None;
            }
        };
        let seconds = match kn_audio::decode(&bytes) {
            Ok(pcm) => pcm.seconds(),
            Err(err) => {
                self.report(Problem::sound(&name, &err));
                return None;
            }
        };
        match self.session.media.import_audio(bytes) {
            Ok(id) => Some((id, seconds)),
            Err(_) => {
                self.report(Problem::sound(&name, &kn_audio::AudioError::Unsupported));
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
            let music = match self.project().music.clone() {
                Some(m) => Music { source: MusicSource::File(id), offset_ms: 0, ..m },
                None => Music::new(MusicSource::File(id), volume),
            };
            self.run(Command::SetMusic { music: Some(music) });
            self.session.seal();
            self.notify("Added as background music.");
        } else {
            self.set_voice(self.slide, id, seconds);
            self.notify(format!("Added as the voice for slide {}.", self.slide + 1));
        }
    }
}
