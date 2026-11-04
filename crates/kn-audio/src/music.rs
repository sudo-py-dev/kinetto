//! Background music made on the spot from simple synths, so every video can
//! have a soundtrack that is free to use anywhere. Each mood is a four-bar
//! chord loop with pads, an arpeggio, bass and (for some moods) drums.

use crate::{CHANNELS, Pcm, RATE};
use kn_model::Mood;
use std::f32::consts::TAU;

struct Style {
    bpm: f32,
    /// Chords as semitone offsets from A4 (440 Hz), one per bar.
    chords: [[i32; 3]; 4],
    pad: f32,
    arp: f32,
    bass: f32,
    drums: bool,
    /// Brightness of the pad: 0 is pure sine, 1 adds strong harmonics.
    bright: f32,
}

fn style(mood: Mood) -> Style {
    match mood {
        // C – Am – F – G
        Mood::Calm => Style { bpm: 72.0, chords: [[-9, -5, -2], [-12, -9, -5], [-16, -12, -9], [-14, -10, -7]], pad: 0.22, arp: 0.08, bass: 0.18, drums: false, bright: 0.15 },
        // G – D – Em – C
        Mood::Upbeat => Style { bpm: 116.0, chords: [[-14, -10, -7], [-7, -3, 0], [-5, -2, 2], [-9, -5, -2]], pad: 0.10, arp: 0.14, bass: 0.22, drums: true, bright: 0.35 },
        // Dm – B♭ – F – C
        Mood::Epic => Style { bpm: 84.0, chords: [[-7, -4, 0], [-11, -7, -4], [-16, -12, -9], [-9, -5, -2]], pad: 0.20, arp: 0.06, bass: 0.26, drums: true, bright: 0.6 },
    }
}

fn freq(semitones: i32) -> f32 {
    440.0 * 2f32.powf(semitones as f32 / 12.0)
}

/// A soft-edged tone: sine plus a few odd harmonics for brightness.
fn tone(phase: f32, bright: f32) -> f32 {
    let s = phase.sin();
    s + bright * ((3.0 * phase).sin() / 3.0 + (5.0 * phase).sin() / 5.0 * 0.6)
}

/// Fast attack, exponential fall: plucks and drums.
fn decay(t: f32, rate: f32) -> f32 {
    if t < 0.0 { 0.0 } else { (1.0 - (-t * 400.0).exp()) * (-t * rate).exp() }
}

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// `frames` of stereo music in `mood`. The same mood always sounds the same.
pub fn generate(mood: Mood, frames: usize) -> Pcm {
    let st = style(mood);
    let rate = RATE as f32;
    let beat = 60.0 / st.bpm;
    let bar = beat * 4.0;
    let mut noise = Noise(0x9e37_79b9);
    // One-pole high-pass state for the hi-hat: last output and last input.
    let (mut hat_hp, mut last_noise) = (0.0f32, 0.0f32);
    let mut out = vec![0.0f32; frames * CHANNELS];

    for (i, frame) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
        let t = i as f32 / rate;
        let bar_index = (t / bar) as usize;
        let chord = st.chords[bar_index % 4];
        let in_bar = t - bar_index as f32 * bar;
        // Pads swell in over each bar and ease out at its end.
        let swell = (in_bar / 0.8).min(1.0) * ((bar - in_bar) / 0.3).clamp(0.0, 1.0);
        let (mut left, mut right) = (0.0, 0.0);
        for (n, &note) in chord.iter().enumerate() {
            let f = freq(note);
            // A slight detune between sides makes the pad wide.
            left += tone(TAU * f * 0.998 * t, st.bright) * (1.0 - n as f32 * 0.1);
            right += tone(TAU * f * 1.002 * t, st.bright) * (1.0 - n as f32 * 0.1);
        }
        left *= st.pad * swell / 3.0;
        right *= st.pad * swell / 3.0;

        // Eighth-note arpeggio an octave up, panned gently.
        let eighth = beat / 2.0;
        let step = (t / eighth) as usize;
        let note = chord[[0, 1, 2, 1][step % 4]] + 12;
        let arp = (TAU * freq(note) * t).sin() * decay(t - step as f32 * eighth, 7.0) * st.arp;
        let pan = if step.is_multiple_of(2) { 0.65 } else { 0.35 };
        left += arp * (1.0 - pan) * 2.0;
        right += arp * pan * 2.0;

        // Bass on every beat, two octaves below the root.
        let beat_index = (t / beat) as usize;
        let since_beat = t - beat_index as f32 * beat;
        let bass = (TAU * freq(chord[0] - 24) * t).sin() * decay(since_beat, 2.5) * st.bass;
        left += bass;
        right += bass;

        if st.drums {
            // Kick: a falling sine on every beat.
            let kick_f = 50.0 + 90.0 * (-since_beat * 30.0).exp();
            let kick = (TAU * kick_f * since_beat).sin() * decay(since_beat, 9.0) * 0.5;
            // Hi-hat: high-passed noise on the off-beats.
            let n = noise.next();
            hat_hp = 0.6 * (hat_hp + n - last_noise);
            last_noise = n;
            let off = t - (beat_index as f32 + 0.5) * beat;
            let hat = hat_hp * decay(off, 45.0) * 0.08;
            // Clap on beats two and four.
            let clap = if beat_index % 2 == 1 { n * decay(since_beat, 18.0) * 0.12 } else { 0.0 };
            left += kick + hat * 0.8 + clap;
            right += kick + hat * 1.2 + clap;
        }
        frame[0] = left;
        frame[1] = right;
    }
    reverb(&mut out);
    Pcm { samples: out }
}

/// A small Schroeder reverb: four combs and two all-passes per side.
fn reverb(samples: &mut [f32]) {
    const COMBS: [usize; 4] = [1557, 1617, 1491, 1422];
    const ALLPASS: [usize; 2] = [225, 556];
    for side in 0..CHANNELS {
        let spread = side * 23;
        let mut combs: Vec<(Vec<f32>, usize)> = COMBS.iter().map(|&n| (vec![0.0; n + spread], 0)).collect();
        let mut passes: Vec<(Vec<f32>, usize)> = ALLPASS.iter().map(|&n| (vec![0.0; n + spread], 0)).collect();
        for s in samples.iter_mut().skip(side).step_by(CHANNELS) {
            let dry = *s;
            let mut wet = 0.0;
            for (buf, pos) in &mut combs {
                let y = buf[*pos];
                buf[*pos] = dry + y * 0.8;
                *pos = (*pos + 1) % buf.len();
                wet += y;
            }
            wet /= 4.0;
            for (buf, pos) in &mut passes {
                let y = buf[*pos];
                buf[*pos] = wet + y * 0.5;
                *pos = (*pos + 1) % buf.len();
                wet = y - wet * 0.5;
            }
            *s = dry * 0.82 + wet * 0.28;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mood_is_audible_finite_and_repeatable() {
        for mood in Mood::ALL {
            let a = generate(mood, RATE as usize * 2);
            assert_eq!(a.frames(), RATE as usize * 2);
            assert!(a.samples.iter().all(|s| s.is_finite()), "{mood:?}");
            let rms = (a.samples.iter().map(|s| s * s).sum::<f32>() / a.samples.len() as f32).sqrt();
            assert!(rms > 0.01, "{mood:?} is too quiet: {rms}");
            assert_eq!(a.samples, generate(mood, RATE as usize * 2).samples, "{mood:?} isn't repeatable");
        }
    }
}
