//! The soundtrack: background music and voice-overs mixed to one stereo
//! track, with music lowered under speech and the result evened out to a
//! standard loudness.

mod decode;
pub mod loudness;
mod music;

pub use decode::{decode, from_interleaved};
pub use music::generate;

use kn_media::MediaLibrary;
use kn_model::{MusicSource, Project};
use kn_time::Tick;
use std::collections::HashMap;
use std::sync::Arc;

pub const RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

/// Voice starts this long after its slide, so it doesn't clip the transition.
const VOICE_DELAY: f64 = 0.25;
/// Music under voice drops to this gain (about −12 dB).
const DUCKED: f32 = 0.25;
const DUCK_ATTACK: f32 = 0.15;
const DUCK_RELEASE: f32 = 0.6;
const MUSIC_FADE_IN: f64 = 0.5;
const MUSIC_FADE_OUT: f64 = 2.0;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("this sound file type isn't supported; use WAV, MP3, FLAC, OGG or M4A")]
    Unsupported,
    #[error("this sound is longer than 30 minutes")]
    TooLong,
    #[error("the sound couldn't be converted: {0}")]
    Resample(String),
}

/// Interleaved stereo at [`RATE`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pcm {
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / CHANNELS
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(RATE)
    }

    /// A 16-bit stereo WAV file, e.g. to save a recording.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * CHANNELS as u32 * 2).to_le_bytes());
        out.extend_from_slice(&(CHANNELS as u16 * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        out
    }
}

/// Decoded sounds, kept between mixes. Failed decodes are remembered so
/// they aren't retried.
#[derive(Default)]
pub struct AudioCache {
    sounds: HashMap<String, Option<Arc<Pcm>>>,
}

impl AudioCache {
    pub fn get(&mut self, media: &MediaLibrary, id: &str) -> Option<Arc<Pcm>> {
        self.sounds
            .entry(id.to_owned())
            .or_insert_with(|| {
                let bytes = media.get(id)?;
                match decode(bytes) {
                    Ok(pcm) => Some(Arc::new(pcm)),
                    Err(err) => {
                        tracing::warn!("sound {id} can't be played: {err}");
                        None
                    }
                }
            })
            .clone()
    }
}

fn frame_of(t: Tick) -> usize {
    (t.as_secs_f64() * f64::from(RATE)).round().max(0.0) as usize
}

/// The full soundtrack, as long as the video. `None` when the project has
/// no music and no voice.
pub fn mix(project: &Project, media: &MediaLibrary, cache: &mut AudioCache) -> Option<Pcm> {
    let frames = frame_of(project.total_duration());
    let has_voice = project.slides.iter().any(|s| s.voice.is_some());
    if frames == 0 || (project.music.is_none() && !has_voice) {
        return None;
    }

    // Voices, each starting just after its slide.
    let mut voice = vec![0.0f32; frames * CHANNELS];
    let mut start = Tick::ZERO;
    for slide in &project.slides {
        if let Some(pcm) = slide.voice.as_deref().and_then(|id| cache.get(media, id)) {
            let at = frame_of(start) + (VOICE_DELAY * f64::from(RATE)) as usize;
            let end = (at + pcm.frames()).min(frames);
            for (dst, src) in voice[at * CHANNELS..end * CHANNELS].iter_mut().zip(&pcm.samples) {
                *dst += src;
            }
        }
        start = start + slide.duration;
    }

    let mut out = voice.clone();
    if let Some(music) = &project.music {
        let (start, end) = music.span(project.total_duration());
        let (start, end) = (frame_of(start).min(frames), frame_of(end).min(frames));
        let offset = (music.offset_ms.max(0) as f64 / 1000.0 * f64::from(RATE)) as usize;
        let track = match &music.source {
            // Generated music is made as long as it needs to be.
            MusicSource::Mood(mood) => Some(Arc::new(generate(*mood, offset + (end - start)))),
            MusicSource::File(id) => cache.get(media, id),
        };
        if let Some(track) = track.filter(|t| t.frames() > 0) {
            let duck = duck_envelope(&voice, frames);
            let length = end.saturating_sub(start);
            // Fades stay inside short clips.
            let fade_in = ((MUSIC_FADE_IN * f64::from(RATE)) as usize).min(length / 3).max(1);
            let fade_out = ((MUSIC_FADE_OUT * f64::from(RATE)) as usize).min(length / 3).max(1);
            for i in start..end {
                let into = i - start;
                let mut src = offset + into;
                if src >= track.frames() {
                    if !music.looped {
                        break;
                    }
                    src %= track.frames();
                }
                let edge = (into as f32 / fade_in as f32).min(1.0) * ((end - i) as f32 / fade_out as f32).min(1.0);
                let gain = music.volume.clamp(0.0, 1.0) * duck[i] * edge;
                out[i * CHANNELS] += track.samples[src * CHANNELS] * gain;
                out[i * CHANNELS + 1] += track.samples[src * CHANNELS + 1] * gain;
            }
        }
    }
    loudness::normalise(&mut out);
    Some(Pcm { samples: out })
}

/// Loudness peaks, `per_second` of them for each second of `pcm`, for
/// drawing a waveform (0 to 1).
pub fn peaks(pcm: &Pcm, per_second: usize) -> Vec<f32> {
    let bucket = (RATE as usize / per_second.max(1)).max(1) * CHANNELS;
    let raw: Vec<f32> = pcm.samples.chunks(bucket).map(|c| c.iter().fold(0.0f32, |m, s| m.max(s.abs()))).collect();
    let loudest = raw.iter().copied().fold(0.0f32, f32::max).max(1e-6);
    raw.into_iter().map(|p| p / loudest).collect()
}

/// Per-frame music gain: [`DUCKED`] while someone speaks, 1 otherwise,
/// with smooth movement between the two.
fn duck_envelope(voice: &[f32], frames: usize) -> Vec<f32> {
    let window = RATE as usize / 20; // 50 ms
    let attack = 1.0 - (-1.0 / (DUCK_ATTACK * RATE as f32)).exp();
    let release = 1.0 - (-1.0 / (DUCK_RELEASE * RATE as f32)).exp();
    let mut gain = 1.0f32;
    let mut out = Vec::with_capacity(frames);
    let mut level = 0.0f32;
    for i in 0..frames {
        // Running mean of |voice| over the window.
        let add = voice[i * CHANNELS].abs();
        let drop = if i >= window { voice[(i - window) * CHANNELS].abs() } else { 0.0 };
        level += (add - drop) / window as f32;
        let speaking = level > 0.005;
        let target = if speaking { DUCKED } else { 1.0 };
        let speed = if target < gain { attack } else { release };
        gain += (target - gain) * speed;
        out.push(gain);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kn_model::{Mood, Music};

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
    }

    #[test]
    fn wav_round_trips() {
        let pcm = Pcm { samples: (0..9600).map(|i| (i as f32 * 0.01).sin() * 0.5).collect() };
        let back = decode(&pcm.to_wav()).unwrap();
        assert_eq!(back.frames(), pcm.frames());
        assert!(back.samples.iter().zip(&pcm.samples).all(|(a, b)| (a - b).abs() < 1e-3));
    }

    #[test]
    fn silent_projects_have_no_soundtrack() {
        assert!(mix(&Project::sample(), &MediaLibrary::new(), &mut AudioCache::default()).is_none());
    }

    #[test]
    fn music_fills_the_video_and_meets_loudness() {
        let mut p = Project::sample();
        p.music = Some(Music::new(MusicSource::Mood(Mood::Upbeat), 0.8));
        let out = mix(&p, &MediaLibrary::new(), &mut AudioCache::default()).unwrap();
        assert_eq!(out.frames(), frame_of(p.total_duration()));
        let l = loudness::integrated(&out.samples).unwrap();
        assert!((l - loudness::TARGET_LUFS).abs() < 1.0, "{l}");
        assert!(out.samples.iter().all(|s| s.abs() <= 0.9));
    }

    #[test]
    fn music_plays_only_where_it_is_placed() {
        // A one-second tone as the "song", placed from 2 s to 5 s, played once.
        let tone: Vec<i16> = (0..RATE as i32).map(|i| ((i as f32 * 0.07).sin() * 12000.0) as i16).collect();
        let mut media = MediaLibrary::new();
        let id = media.import_audio(super::decode::tests::wav(RATE, &tone)).unwrap();
        let mut p = Project::sample();
        let mut m = Music::new(MusicSource::File(id), 1.0);
        m.start_ms = 2000;
        m.end_ms = Some(5000);
        m.looped = false;
        p.music = Some(m.clone());
        let out = mix(&p, &media, &mut AudioCache::default()).unwrap();
        let at = |secs: f64| rms(&out.samples[frame_of(Tick::from_millis((secs * 1000.0) as i64)) * CHANNELS..][..4800]);
        assert!(at(1.0) < 1e-6, "silent before the music starts");
        assert!(at(2.5) > 0.01, "playing after it starts");
        assert!(at(3.5) < 1e-6, "silent once a one-off song has ended");

        m.looped = true;
        p.music = Some(m);
        let out = mix(&p, &media, &mut AudioCache::default()).unwrap();
        let at = |secs: f64| rms(&out.samples[frame_of(Tick::from_millis((secs * 1000.0) as i64)) * CHANNELS..][..4800]);
        assert!(at(3.5) > 0.01, "a looped song keeps going");
        assert!(at(5.5) < 1e-6, "silent after the music ends");
    }

    #[test]
    fn every_generated_track_respects_free_placement_and_silence() {
        for mood in Mood::ALL {
            let mut project = Project::sample();
            let mut music = Music::new(MusicSource::Mood(mood), 0.6);
            music.start_ms = 1234;
            music.end_ms = Some(4567);
            music.offset_ms = 789;
            project.music = Some(music);
            let out = mix(&project, &MediaLibrary::new(), &mut AudioCache::default()).unwrap();
            let at = |ms| frame_of(Tick::from_millis(ms)) * CHANNELS;
            assert!(out.samples[..at(1234)].iter().all(|s| *s == 0.0), "{mood:?}: before start");
            assert!(out.samples[at(4567)..].iter().all(|s| *s == 0.0), "{mood:?}: after end");
            assert!(rms(&out.samples[at(2000)..at(3000)]) > 0.01, "{mood:?}: not audible");
        }
    }

    #[test]
    fn music_ducks_under_voice() {
        // One second of a loud tone as the voice on slide 2.
        let tone: Vec<i16> = (0..RATE as i32).map(|i| ((i as f32 * 0.07).sin() * 12000.0) as i16).collect();
        let mut media = MediaLibrary::new();
        let id = media.import_audio(super::decode::tests::wav(RATE, &tone)).unwrap();
        let mut p = Project::sample();
        p.slides[1].voice = Some(id);
        p.music = Some(Music::new(MusicSource::Mood(Mood::Calm), 1.0));
        let voice_start = frame_of(p.slides[0].duration) + (VOICE_DELAY * f64::from(RATE)) as usize;

        let voice: Vec<f32> = {
            let mut v = vec![0.0; frame_of(p.total_duration()) * CHANNELS];
            let pcm = decode(media.get(p.slides[1].voice.as_ref().unwrap()).unwrap()).unwrap();
            v[voice_start * CHANNELS..voice_start * CHANNELS + pcm.samples.len()].copy_from_slice(&pcm.samples);
            v
        };
        let duck = duck_envelope(&voice, voice.len() / CHANNELS);
        let mid_voice = voice_start + RATE as usize / 2;
        assert!(duck[mid_voice] < 0.3, "music should be ducked during speech: {}", duck[mid_voice]);
        assert!(duck[voice_start / 2] > 0.99, "music should be full before speech");

        let out = mix(&p, &media, &mut AudioCache::default()).unwrap();
        assert!(rms(&out.samples) > 0.01);
    }
}
