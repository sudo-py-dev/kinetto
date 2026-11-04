//! "Make Video": renders every frame with the same code as the preview,
//! encodes H.264 with OpenH264 and writes a web-ready MP4.

mod gpu;
mod still;
mod yuv;

pub use still::{GIF_WIDTH, export_gif, export_pdf};

pub use gpu::{Gpu, GpuError, Offscreen};

use kn_geom::Aspect;
use kn_media::MediaLibrary;
use kn_model::Project;
use kn_render::RenderContext;
use kn_time::FrameRate;
use muxide::api::{AudioCodec, MuxerBuilder, VideoCodec};
use openh264::OpenH264API;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameType, IntraFramePeriod, RateControlMode, UsageType, VuiConfig};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSettings {
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub bitrate: u32,
}

impl ExportSettings {
    /// Full-HD output in the project's shape: YouTube, Shorts/Reels, Square.
    pub fn full_hd(aspect: Aspect) -> Self {
        let (width, height) = match aspect {
            Aspect::Wide16x9 => (1920, 1080),
            Aspect::Tall9x16 => (1080, 1920),
            Aspect::Square1x1 => (1080, 1080),
        };
        // About 0.13 bits per pixel per frame: crisp for slides at 30 fps.
        let bitrate = (f64::from(width * height) * 30.0 * 0.13) as u32;
        ExportSettings { width, height, fps: FrameRate::FPS_30, bitrate }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Gpu(#[from] GpuError),
    #[error("the video encoder failed: {0}")]
    Encoder(#[from] openh264::Error),
    #[error("writing the video file failed: {0}")]
    Mux(#[from] muxide::api::MuxerError),
    #[error("writing the video file failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("the sound encoder failed: {0}")]
    Audio(&'static str),
    #[error("the video size must be even and at least 16 pixels")]
    BadSize,
    #[error("export was cancelled")]
    Cancelled,
}

fn encoder_threads() -> u16 {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(8) as u16)
}

impl ExportError {
    /// The file-system error behind this failure, if there is one (disk
    /// full, no permission, drive gone).
    pub fn io(&self) -> Option<&std::io::Error> {
        match self {
            ExportError::Io(e) | ExportError::Mux(muxide::api::MuxerError::Io(e)) => Some(e),
            _ => None,
        }
    }
}

/// One encoded frame in Annex B form.
pub struct EncodedFrame<'a> {
    pub index: u64,
    pub data: &'a [u8],
    pub keyframe: bool,
}

/// Renders and encodes every frame, handing each to `sink`. `progress` gets
/// (frames done, total frames). Stops early if `cancel` is set.
pub fn encode(
    project: &Project,
    media: &MediaLibrary,
    settings: &ExportSettings,
    gpu: Option<Gpu>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
    mut sink: impl FnMut(EncodedFrame<'_>) -> Result<(), ExportError>,
) -> Result<(), ExportError> {
    let (w, h) = (settings.width, settings.height);
    if w < 16 || h < 16 || !w.is_multiple_of(2) || !h.is_multiple_of(2) {
        return Err(ExportError::BadSize);
    }
    let mut gpu = match gpu {
        Some(gpu) => gpu::Offscreen::with_gpu(gpu, w, h)?,
        None => gpu::Offscreen::new(w, h)?,
    };
    let fps = f64::from(settings.fps.num) / f64::from(settings.fps.den);
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(settings.bitrate))
        .max_frame_rate(openh264::encoder::FrameRate::from_hz(fps as f32))
        // Buffer-based control keeps the bitrate without dropping frames;
        // the bitrate modes warn that they need frame skipping for that.
        .rate_control_mode(RateControlMode::Bufferbased)
        // The camera preset is about three times faster than the
        // screen-content one here, at the same quality for slides.
        .usage_type(UsageType::CameraVideoRealTime)
        // Every frame must come out, or video time would drift from audio.
        .skip_frames(false)
        .intra_frame_period(IntraFramePeriod::from_num_frames((fps * 2.0).round() as u32))
        // Frames are converted with BT.601 limited range; say so in the file.
        .vui(VuiConfig::bt601())
        .num_threads(encoder_threads());
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config)?;

    let total = settings.fps.frames_in(project.total_duration());
    // The GPU renders ahead on its own thread while this one converts and
    // encodes; buffers travel back for reuse so nothing is reallocated.
    std::thread::scope(|scope| {
        let (frames_tx, frames_rx) = mpsc::sync_channel::<Result<Vec<u8>, GpuError>>(3);
        let (spare_tx, spare_rx) = mpsc::channel::<Vec<u8>>();
        scope.spawn(move || {
            let mut cx = RenderContext::new();
            for index in 0..total {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let mut rgba = spare_rx.try_recv().unwrap_or_default();
                let frame = gpu.render(project, media, settings.fps.frame_start(index), &mut cx, &mut rgba).map(|()| rgba);
                let failed = frame.is_err();
                if frames_tx.send(frame).is_err() || failed {
                    break;
                }
            }
        });

        let mut i420 = yuv::I420::default();
        let mut bitstream = Vec::new();
        for index in 0..total {
            let Ok(rgba) = frames_rx.recv() else { return Err(ExportError::Cancelled) };
            let rgba = rgba?;
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            i420.convert(&rgba, w as usize, h as usize);
            let _ = spare_tx.send(rgba);
            let encoded = encoder.encode(&i420.slices())?;
            let keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
            bitstream.clear();
            encoded.write_vec(&mut bitstream);
            sink(EncodedFrame { index, data: &bitstream, keyframe })?;
            progress(index + 1, total);
        }
        Ok(())
    })
}

/// Writes the project to an MP4 at `out`. The file appears only when
/// complete; a cancelled or failed export leaves nothing behind.
pub fn export_mp4(
    project: &Project,
    media: &MediaLibrary,
    settings: &ExportSettings,
    gpu: Option<Gpu>,
    out: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<(), ExportError> {
    let tmp = out.with_extension("mp4.part");
    let result = (|| {
        let file = BufWriter::new(File::create(&tmp)?);
        let fps = f64::from(settings.fps.num) / f64::from(settings.fps.den);
        let soundtrack = kn_audio::mix(project, media, &mut kn_audio::AudioCache::default());
        let mut builder = MuxerBuilder::new(file).video(VideoCodec::H264, settings.width, settings.height, fps).with_fast_start(true);
        let mut audio = match &soundtrack {
            Some(pcm) => {
                builder = builder.audio(AudioCodec::Opus, kn_audio::RATE, kn_audio::CHANNELS as u16);
                Some(AudioTrack::new(pcm)?)
            }
            None => None,
        };
        let mut muxer = builder.build()?;
        encode(project, media, settings, gpu, cancel, progress, |frame| {
            let pts = frame.index as f64 / fps;
            muxer.write_video(pts, frame.data, frame.keyframe)?;
            // Keep sound packets interleaved with the pictures they belong to.
            if let Some(audio) = &mut audio {
                audio.write_until(&mut muxer, pts + 1.0 / fps)?;
            }
            Ok(())
        })?;
        if let Some(audio) = &mut audio {
            audio.write_until(&mut muxer, f64::INFINITY)?;
        }
        muxer.finish()?;
        fs::rename(&tmp, out)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Opus packets of 20 ms, handed to the muxer as the video catches up.
struct AudioTrack<'a> {
    pcm: &'a kn_audio::Pcm,
    encoder: opus_rs::OpusEncoder,
    next_frame: usize,
    packet: Vec<u8>,
    chunk: Vec<f32>,
}

/// Samples per channel in one Opus packet (20 ms at 48 kHz).
const OPUS_FRAME: usize = 960;

impl<'a> AudioTrack<'a> {
    fn new(pcm: &'a kn_audio::Pcm) -> Result<Self, ExportError> {
        let mut encoder = opus_rs::OpusEncoder::new(kn_audio::RATE as i32, kn_audio::CHANNELS, opus_rs::Application::Audio)
            .map_err(ExportError::Audio)?;
        encoder.bitrate_bps = 128_000;
        Ok(AudioTrack { pcm, encoder, next_frame: 0, packet: vec![0; 4000], chunk: vec![0.0; OPUS_FRAME * kn_audio::CHANNELS] })
    }

    fn write_until<W: std::io::Write>(&mut self, muxer: &mut muxide::api::Muxer<W>, until: f64) -> Result<(), ExportError> {
        let total = self.pcm.frames();
        while self.next_frame < total {
            let pts = self.next_frame as f64 / f64::from(kn_audio::RATE);
            if pts >= until {
                break;
            }
            // The last packet is padded with silence.
            let start = self.next_frame * kn_audio::CHANNELS;
            let end = (start + self.chunk.len()).min(self.pcm.samples.len());
            self.chunk.fill(0.0);
            self.chunk[..end - start].copy_from_slice(&self.pcm.samples[start..end]);
            let len = self.encoder.encode(&self.chunk, OPUS_FRAME, &mut self.packet).map_err(ExportError::Audio)?;
            muxer.write_audio(pts, &self.packet[..len])?;
            self.next_frame += OPUS_FRAME;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kn_time::Tick;
    use openh264::decoder::Decoder;
    use openh264::formats::YUVSource;

    fn tiny() -> ExportSettings {
        ExportSettings { width: 192, height: 108, fps: FrameRate { num: 10, den: 1 }, bitrate: 400_000 }
    }

    /// One device shared by every test, as the editor shares its own with
    /// export. `None` skips GPU tests on machines without one.
    fn test_gpu() -> Option<Gpu> {
        static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
        GPU.get_or_init(|| Gpu::create().ok()).clone()
    }

    fn gpu_available() -> bool {
        test_gpu().is_some()
    }

    #[test]
    fn encodes_every_frame_and_decodes_back_to_the_slide_colours() {
        if !gpu_available() {
            eprintln!("no GPU; skipping");
            return;
        }
        let project = Project::sample();
        let settings = tiny();
        let mut stream = Vec::new();
        let mut frames = 0;
        let mut first_key = None;
        encode(&project, &MediaLibrary::new(), &settings, test_gpu(), &AtomicBool::new(false), |_, _| {}, |f| {
            frames += 1;
            first_key.get_or_insert(f.keyframe);
            stream.extend_from_slice(f.data);
            Ok(())
        })
        .unwrap();
        assert_eq!(frames, settings.fps.frames_in(project.total_duration()));
        assert_eq!(first_key, Some(true), "video must start on a keyframe");

        // Decode the first frame and check a corner shows the slide background.
        let mut decoder = Decoder::new().unwrap();
        let first_frame_end = find_second_frame(&stream);
        let yuv = decoder.decode(&stream[..first_frame_end]).unwrap().expect("a decoded frame");
        let (w, h) = yuv.dimensions();
        assert_eq!((w, h), (192, 108));
        let mut rgb = vec![0; w * h * 3];
        yuv.write_rgb8(&mut rgb);
        let bg = project.slides[0].background;
        for (got, want) in rgb[..3].iter().zip([bg.r, bg.g, bg.b]) {
            assert!(got.abs_diff(want) <= 6, "corner pixel {:?} vs background {bg:?}", &rgb[..3]);
        }
    }

    /// Byte offset where the second frame's slice NAL begins.
    fn find_second_frame(stream: &[u8]) -> usize {
        let mut slices = 0;
        for i in 0..stream.len().saturating_sub(4) {
            if stream[i..i + 4] == [0, 0, 0, 1] {
                let nal_type = stream[i + 4] & 0x1f;
                if nal_type == 1 || nal_type == 5 {
                    slices += 1;
                    if slices == 2 {
                        return i;
                    }
                }
            }
        }
        stream.len()
    }

    #[test]
    fn writes_an_mp4_only_when_complete() {
        if !gpu_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kinetto-export-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let out = dir.join("talk.mp4");
        let mut project = Project::sample();
        project.slides.truncate(1);
        project.slides[0].duration = Tick::from_secs(1);
        export_mp4(&project, &MediaLibrary::new(), &tiny(), test_gpu(), &out, &AtomicBool::new(false), |_, _| {}).unwrap();
        let bytes = fs::read(&out).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp");
        // Fast start: the index comes before the media data.
        let moov = bytes.windows(4).position(|w| w == b"moov").unwrap();
        let mdat = bytes.windows(4).position(|w| w == b"mdat").unwrap();
        assert!(moov < mdat);

        // With music, the file gains an Opus sound track.
        let mut with_music = project.clone();
        with_music.music = Some(kn_model::Music { source: kn_model::MusicSource::Mood(kn_model::Mood::Calm), volume: 0.7 });
        let music_out = dir.join("music.mp4");
        export_mp4(&with_music, &MediaLibrary::new(), &tiny(), test_gpu(), &music_out, &AtomicBool::new(false), |_, _| {}).unwrap();
        let bytes = fs::read(&music_out).unwrap();
        assert!(bytes.windows(4).any(|w| w == b"Opus"), "no Opus track");
        assert!(bytes.windows(4).any(|w| w == b"dOps"), "no Opus config");
        fs::remove_file(&music_out).unwrap();

        let cancelled = dir.join("cancelled.mp4");
        let err = export_mp4(&project, &MediaLibrary::new(), &tiny(), test_gpu(), &cancelled, &AtomicBool::new(true), |_, _| {});
        assert!(matches!(err, Err(ExportError::Cancelled)));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "cancel must leave no files");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gif_and_pdf_exports_write_valid_files() {
        if !gpu_available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kinetto-still-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut project = Project::sample();
        project.slides[0].duration = Tick::from_secs(1);
        project.slides[1].duration = Tick::from_secs(1);
        let (gif, pdf) = (dir.join("a.gif"), dir.join("a.pdf"));
        export_gif(&project, &MediaLibrary::new(), test_gpu(), &gif, &AtomicBool::new(false), |_, _| {}).unwrap();
        export_pdf(&project, &MediaLibrary::new(), test_gpu(), &pdf, &AtomicBool::new(false), |_, _| {}).unwrap();

        let mut decoder = gif::DecodeOptions::new().read_info(File::open(&gif).unwrap()).unwrap();
        assert_eq!((decoder.width(), decoder.height()), (640, 360));
        let mut frames = 0;
        let mut centis = 0u32;
        while let Some(f) = decoder.read_next_frame().unwrap() {
            frames += 1;
            centis += u32::from(f.delay);
        }
        // Still stretches are merged, but the length is kept.
        assert!(frames < 24, "{frames} frames");
        assert!((190..=210).contains(&centis), "{centis} cs");

        let bytes = fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF") && String::from_utf8_lossy(&bytes).contains("/Count 2"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_odd_sizes() {
        let s = ExportSettings { width: 191, ..tiny() };
        let r = encode(&Project::sample(), &MediaLibrary::new(), &s, None, &AtomicBool::new(false), |_, _| {}, |_| Ok(()));
        assert!(matches!(r, Err(ExportError::BadSize)));
    }

    #[test]
    fn presets_match_the_shape() {
        assert_eq!(ExportSettings::full_hd(Aspect::Tall9x16).width, 1080);
        assert_eq!(ExportSettings::full_hd(Aspect::Tall9x16).height, 1920);
        assert_eq!(ExportSettings::full_hd(Aspect::Square1x1).height, 1080);
    }
}
