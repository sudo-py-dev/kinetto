//! The sound card: playing the soundtrack during preview (its position is
//! the preview's clock, so picture and sound stay together) and recording
//! voice from the microphone.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, Stream, StreamConfig, SupportedBufferSize};
use kn_audio::{CHANNELS, Pcm, RATE};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// What the output callback reads. Position is in soundtrack frames, kept
/// as a fixed-point number so a device at another rate can step finely.
struct Shared {
    pcm: Mutex<Option<Arc<Pcm>>>,
    /// Soundtrack frame × 2³² (so fractional steps add up exactly).
    position: AtomicU64,
    playing: AtomicBool,
    /// How far behind the callback the speakers are, in microseconds.
    latency_us: AtomicU64,
    /// Underruns since start; only the first is logged.
    underruns: AtomicU64,
}

/// Frames per device buffer: about 21 ms at 48 kHz (about 64 ms total
/// output latency on PipeWire). Big enough that a busy moment (shader
/// compiles, an export) doesn't starve the sound card; with every core
/// busy it ran without a single underrun. The latency is subtracted from
/// the clock.
const BUFFER_FRAMES: u32 = 1024;

const FIXED: f64 = 4_294_967_296.0;

pub struct Player {
    _stream: Stream,
    shared: Arc<Shared>,
}

impl Player {
    /// Opens the default output; `None` if there's no usable sound device,
    /// in which case preview runs silent on the wall clock.
    pub fn new() -> Option<Self> {
        let device = cpal::default_host().default_output_device()?;
        let supported = device.default_output_config().ok()?;
        let mut config = supported.config();
        if let SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
            config.buffer_size = BufferSize::Fixed(BUFFER_FRAMES.clamp(min, max));
        }
        let shared = Arc::new(Shared {
            pcm: Mutex::new(None),
            position: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            latency_us: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        });
        let open = |config: &StreamConfig| match supported.sample_format() {
            SampleFormat::F32 => output::<f32>(&device, config, Arc::clone(&shared)),
            SampleFormat::I16 => output::<i16>(&device, config, Arc::clone(&shared)),
            SampleFormat::U16 => output::<u16>(&device, config, Arc::clone(&shared)),
            other => {
                tracing::warn!("sound device uses {other:?} samples, which aren't supported");
                None
            }
        };
        // Some devices refuse a fixed size; fall back to their default.
        let stream = open(&config).or_else(|| open(&StreamConfig { buffer_size: BufferSize::Default, ..config }))?;
        stream.play().ok()?;
        Some(Player { _stream: stream, shared })
    }

    /// Starts `pcm` from `frame` (at 48 kHz).
    pub fn play(&self, pcm: Arc<Pcm>, frame: usize) {
        if let Ok(mut slot) = self.shared.pcm.lock() {
            *slot = Some(pcm);
        }
        self.shared.position.store((frame as f64 * FIXED) as u64, Ordering::Release);
        self.shared.playing.store(true, Ordering::Release);
    }

    pub fn stop(&self) {
        self.shared.playing.store(false, Ordering::Release);
    }

    /// Underruns since the device opened, and the current output latency.
    pub fn health(&self) -> (u64, std::time::Duration) {
        let latency = std::time::Duration::from_micros(self.shared.latency_us.load(Ordering::Relaxed));
        (self.shared.underruns.load(Ordering::Relaxed), latency)
    }

    /// Seconds into the soundtrack that the speakers have reached: what the
    /// callback has handed over, minus what is still queued in the device.
    pub fn position_secs(&self) -> f64 {
        let handed = self.shared.position.load(Ordering::Acquire) as f64 / FIXED / f64::from(RATE);
        let queued = self.shared.latency_us.load(Ordering::Relaxed) as f64 / 1e6;
        (handed - queued).max(0.0)
    }
}

fn output<T>(device: &cpal::Device, config: &StreamConfig, shared: Arc<Shared>) -> Option<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    // Soundtrack frames per device frame.
    let step = (f64::from(RATE) / f64::from(config.sample_rate) * FIXED) as u64;
    let latency = Arc::clone(&shared);
    let counter = Arc::clone(&shared);
    let callback = move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
        let ts = info.timestamp();
        if let Some(ahead) = ts.playback.checked_duration_since(ts.callback) {
            latency.latency_us.store(ahead.as_micros() as u64, Ordering::Relaxed);
        }
        let silence = T::from_sample(0.0);
        let pcm = shared.pcm.try_lock().ok().and_then(|p| p.clone());
        let (Some(pcm), true) = (pcm, shared.playing.load(Ordering::Acquire)) else {
            out.fill(silence);
            return;
        };
        let mut pos = shared.position.load(Ordering::Acquire);
        for frame in out.chunks_mut(channels) {
            let at = (pos as f64 / FIXED) as usize;
            let (l, r) = match pcm.samples.get(at * CHANNELS..at * CHANNELS + 2) {
                Some(&[l, r]) => (l, r),
                _ => (0.0, 0.0),
            };
            for (c, sample) in frame.iter_mut().enumerate() {
                *sample = T::from_sample(match (channels, c) {
                    (1, _) => (l + r) / 2.0,
                    (_, 0) => l,
                    (_, 1) => r,
                    _ => 0.0,
                });
            }
            pos += step;
        }
        shared.position.store(pos, Ordering::Release);
    };
    let errors = move |err| {
        // An occasional underrun is a click, not a failure: say so once.
        if counter.underruns.fetch_add(1, Ordering::Relaxed) == 0 {
            tracing::warn!("sound output glitch: {err} (further ones are not logged)");
        }
    };
    device.build_output_stream::<T, _, _>(*config, callback, errors, None).map_err(|e| tracing::warn!("can't open sound output: {e}")).ok()
}

/// Microphone capture until [`Recorder::finish`].
pub struct Recorder {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    /// Loudest recent sample, for a level meter (f32 bits).
    level: Arc<AtomicU64>,
    rate: u32,
    channels: usize,
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("No microphone was found. Plug one in, or choose one in your system's sound settings, then try again.")?;
        let supported = device
            .default_input_config()
            .map_err(|_| "The microphone can't be used right now. Close other apps that may be using it, check your system's privacy settings, then try again.")?;
        let config = supported.config();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let level = Arc::new(AtomicU64::new(0));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => input::<f32>(&device, &config, Arc::clone(&samples), Arc::clone(&level)),
            SampleFormat::I16 => input::<i16>(&device, &config, Arc::clone(&samples), Arc::clone(&level)),
            SampleFormat::U16 => input::<u16>(&device, &config, Arc::clone(&samples), Arc::clone(&level)),
            other => Err(format!("This microphone sends sound in a form Kinetto can't read ({other:?}). Try another microphone.")),
        }?;
        stream.play().map_err(|_| "The microphone couldn't start. Close other apps that may be using it, then try again.")?;
        Ok(Recorder { _stream: stream, samples, level, rate: config.sample_rate, channels: usize::from(config.channels) })
    }

    pub fn seconds(&self) -> f64 {
        let len = self.samples.lock().map_or(0, |s| s.len());
        len as f64 / self.channels.max(1) as f64 / f64::from(self.rate)
    }

    /// 0 to 1, decaying, for drawing a level meter.
    pub fn level(&self) -> f32 {
        f64::from_bits(self.level.load(Ordering::Relaxed)) as f32
    }

    /// Stops and returns the take at 48 kHz stereo, with silence trimmed
    /// from both ends.
    pub fn finish(self) -> Result<Pcm, String> {
        let raw = std::mem::take(&mut *self.samples.lock().map_err(|_| "The recording was lost.")?);
        let pcm = kn_audio::from_interleaved(&raw, self.rate, self.channels)
            .map_err(|_| "Nothing was recorded. Check the microphone is switched on and not muted, then try again.".to_owned())?;
        Ok(trim_silence(pcm))
    }
}

fn input<T>(device: &cpal::Device, config: &StreamConfig, samples: Arc<Mutex<Vec<f32>>>, level: Arc<AtomicU64>) -> Result<Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut meter = 0.0f64;
    let callback = move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mut peak = 0.0f32;
        if let Ok(mut buf) = samples.lock() {
            for &s in data {
                let v: f32 = s.to_sample();
                peak = peak.max(v.abs());
                buf.push(v);
            }
        }
        meter = f64::from(peak).max(meter * 0.85);
        level.store(meter.to_bits(), Ordering::Relaxed);
    };
    let errors = |err| tracing::warn!("microphone error: {err}");
    device
        .build_input_stream::<T, _, _>(*config, callback, errors, None)
        .map_err(|_| "The microphone couldn't start. Close other apps that may be using it, then try again.".to_owned())
}

/// Drops quiet lead-in and tail, keeping a short pad so words aren't cut.
fn trim_silence(pcm: Pcm) -> Pcm {
    const THRESHOLD: f32 = 0.02;
    let pad = RATE as usize / 10 * CHANNELS;
    let first = pcm.samples.iter().position(|s| s.abs() > THRESHOLD);
    let last = pcm.samples.iter().rposition(|s| s.abs() > THRESHOLD);
    match (first, last) {
        (Some(a), Some(b)) => {
            let start = a.saturating_sub(pad) / CHANNELS * CHANNELS;
            let end = ((b + pad) / CHANNELS * CHANNELS + CHANNELS).min(pcm.samples.len());
            Pcm { samples: pcm.samples[start..end].to_vec() }
        }
        _ => pcm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_keeps_the_words_and_a_little_air() {
        let mut samples = vec![0.0; RATE as usize * CHANNELS];
        samples.extend(std::iter::repeat_n(0.5, RATE as usize / 2 * CHANNELS));
        samples.extend(vec![0.0; RATE as usize * CHANNELS]);
        let trimmed = trim_silence(Pcm { samples });
        let secs = trimmed.seconds();
        assert!((0.65..0.75).contains(&secs), "{secs}");
        assert_eq!(trimmed.samples.len() % CHANNELS, 0);
    }
}
