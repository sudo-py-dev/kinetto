//! Integrated loudness per ITU-R BS.1770-4 (K-weighting, 400 ms blocks,
//! absolute and relative gates), so every video comes out equally loud.

use crate::{CHANNELS, RATE};

/// Target for online video; YouTube and most platforms sit near −14 to −16.
pub const TARGET_LUFS: f64 = -16.0;
/// Never let a sample peak above −1 dBFS.
const PEAK_LIMIT: f32 = 0.891;

/// A direct-form biquad.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The two K-weighting stages at 48 kHz, as published in BS.1770.
fn k_weighting() -> [Biquad; 2] {
    debug_assert_eq!(RATE, 48_000);
    [
        Biquad { b: [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85], a: [-1.690_659_293_182_41, 0.732_480_774_215_85], z: [0.0; 2] },
        Biquad { b: [1.0, -2.0, 1.0], a: [-1.990_047_454_833_98, 0.990_072_250_366_21], z: [0.0; 2] },
    ]
}

/// Loudness of interleaved stereo in LUFS; `None` for silence.
pub fn integrated(samples: &[f32]) -> Option<f64> {
    let block = (RATE as usize * 400) / 1000;
    let hop = block / 4;
    let mut filters = [k_weighting(), k_weighting()];
    // Squared, K-weighted signal summed over both channels, per frame.
    let power: Vec<f64> = samples
        .as_chunks::<CHANNELS>()
        .0
        .iter()
        .map(|frame| {
            frame.iter().zip(&mut filters).map(|(&s, [a, b])| b.run(a.run(f64::from(s))).powi(2)).sum()
        })
        .collect();
    if power.len() < block {
        return None;
    }
    let mut prefix = vec![0.0; power.len() + 1];
    for (i, p) in power.iter().enumerate() {
        prefix[i + 1] = prefix[i] + p;
    }
    let blocks: Vec<f64> = (0..=(power.len() - block) / hop).map(|i| (prefix[i * hop + block] - prefix[i * hop]) / block as f64).collect();
    let lufs = |mean: f64| -0.691 + 10.0 * mean.log10();
    let gated_mean = |threshold: f64| {
        let kept: Vec<f64> = blocks.iter().copied().filter(|&m| m > 0.0 && lufs(m) > threshold).collect();
        (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
    };
    let absolute = gated_mean(-70.0)?;
    let relative = gated_mean(lufs(absolute) - 10.0)?;
    Some(lufs(relative))
}

/// Scales to [`TARGET_LUFS`], backing off if that would push peaks past
/// −1 dBFS.
pub fn normalise(samples: &mut [f32]) {
    let Some(loudness) = integrated(samples) else { return };
    let gain = 10f64.powf((TARGET_LUFS - loudness) / 20.0) as f32;
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let gain = if peak * gain > PEAK_LIMIT { PEAK_LIMIT / peak } else { gain };
    samples.iter_mut().for_each(|s| *s *= gain);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        let n = (RATE as f32 * seconds) as usize;
        (0..n).flat_map(|i| {
            let s = amplitude * (std::f32::consts::TAU * freq * i as f32 / RATE as f32).sin();
            [s, s]
        })
        .collect()
    }

    #[test]
    fn ebu_3341_reference_sine_reads_minus_23() {
        // EBU Tech 3341 case 1: 1 kHz stereo sine at −23 dBFS per channel.
        let amplitude = 10f32.powf(-23.0 / 20.0);
        let l = integrated(&sine(1000.0, amplitude, 20.0)).unwrap();
        assert!((l + 23.0).abs() < 0.1, "{l}");
    }

    #[test]
    fn normalise_hits_target_and_silence_is_left_alone() {
        let mut quiet = sine(440.0, 0.05, 3.0);
        normalise(&mut quiet);
        let l = integrated(&quiet).unwrap();
        assert!((l - TARGET_LUFS).abs() < 0.2, "{l}");
        let mut silent = vec![0.0; 48_000];
        normalise(&mut silent);
        assert!(silent.iter().all(|&s| s == 0.0));
    }
}
