//! Any supported sound file to 48 kHz stereo floats.

use crate::{AudioError, CHANNELS, Pcm, RATE};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::io::Cursor;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Longest sound accepted, so a hostile file can't fill memory.
const MAX_SECONDS: usize = 30 * 60;

pub fn decode(bytes: &[u8]) -> Result<Pcm, AudioError> {
    let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), source, FormatOptions::default(), MetadataOptions::default())
        .map_err(|_| AudioError::Unsupported)?;
    let track = format.default_track(TrackType::Audio).ok_or(AudioError::Unsupported)?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or(AudioError::Unsupported)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|_| AudioError::Unsupported)?;
    let track_id = track.id;

    let (mut rate, mut channels) = (0, 0);
    let mut interleaved: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(_) => break,
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(buf) => buf,
            // A damaged packet is skipped, like players do.
            Err(Error::DecodeError(_)) => continue,
            Err(_) => break,
        };
        rate = buf.spec().rate();
        channels = buf.spec().channels().count();
        chunk.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(&mut chunk);
        interleaved.extend_from_slice(&chunk);
        if rate > 0 && interleaved.len() / channels.max(1) / rate as usize > MAX_SECONDS {
            return Err(AudioError::TooLong);
        }
    }
    if interleaved.is_empty() || channels == 0 || rate == 0 {
        return Err(AudioError::Unsupported);
    }
    let stereo = to_stereo(&interleaved, channels);
    if rate == RATE {
        return Ok(Pcm { samples: stereo });
    }
    resample(&stereo, rate)
}

/// Mono is copied to both sides; extra channels beyond two are dropped.
fn to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        1 => samples.iter().flat_map(|&s| [s, s]).collect(),
        2 => samples.to_vec(),
        n => samples.chunks_exact(n).flat_map(|f| [f[0], f[1]]).collect(),
    }
}

fn resample(stereo: &[f32], from: u32) -> Result<Pcm, AudioError> {
    let frames = stereo.len() / CHANNELS;
    let mut resampler = Fft::<f32>::new(from as usize, RATE as usize, 1024, CHANNELS, FixedSync::Input)
        .map_err(|e| AudioError::Resample(e.to_string()))?;
    let input = InterleavedSlice::new(stereo, CHANNELS, frames).map_err(|e| AudioError::Resample(e.to_string()))?;
    let out = resampler.process_all(&input, frames, None).map_err(|e| AudioError::Resample(e.to_string()))?;
    Ok(Pcm { samples: out.take_data() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 16-bit PCM WAV with one channel.
    pub(crate) fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        samples.iter().for_each(|s| out.extend_from_slice(&s.to_le_bytes()));
        out
    }

    #[test]
    fn mono_wav_becomes_stereo_at_48k() {
        let tone: Vec<i16> = (0..44_100).map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16).collect();
        let pcm = decode(&wav(44_100, &tone)).unwrap();
        let seconds = pcm.frames() as f64 / f64::from(RATE);
        assert!((seconds - 1.0).abs() < 0.03, "got {seconds} s");
        let peak = pcm.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 8000.0 / 32768.0).abs() < 0.05, "peak {peak}");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"this is not a sound").is_err());
    }
}
