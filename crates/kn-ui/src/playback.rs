//! Preview playback: whether it runs, the clock it follows and the sound it
//! plays. The editor decides what to play; this keeps time.

use crate::device;
use kn_audio::Pcm;
use kn_time::Tick;
use std::sync::Arc;
use std::time::Instant;

/// Preview playback state. The app decides where to play from; this keeps
/// the time and drives the speakers.
pub(crate) struct Playback {
    /// Playback is running.
    pub(crate) playing: bool,
    /// The project time playback started from.
    pub(crate) origin: Tick,
    /// The wall-clock moment playback started (the clock without sound).
    pub(crate) started: Instant,
    /// The moment showing now, while playing.
    pub(crate) time: Tick,
    /// The sound card; `None` when there is none, and preview is silent.
    pub(crate) player: Option<device::Player>,
    /// True while the sound card is the clock (sound is playing).
    pub(crate) audio_clock: bool,
    /// Soundtrack mixed for the given content stamp.
    soundtrack: Option<(u64, Option<Arc<Pcm>>)>,
}

/// What starting playback found out.
pub(crate) enum Started {
    /// Playing, and the sound card is the clock.
    WithSound,
    /// Playing a project with no sound.
    Silent,
    /// There is sound to play but no speakers or headphones.
    NoSpeakers,
}

impl Playback {
    /// Stopped, with the default speakers opened if there are any.
    pub(crate) fn new() -> Self {
        Self {
            playing: false,
            origin: Tick::ZERO,
            started: Instant::now(),
            time: Tick::ZERO,
            player: device::Player::new(),
            audio_clock: false,
            soundtrack: None,
        }
    }

    /// The soundtrack for content `stamp`, mixed by `mix` only when the
    /// content changed since the last time.
    pub(crate) fn soundtrack(&mut self, stamp: u64, mix: impl FnOnce() -> Option<Pcm>) -> Option<Arc<Pcm>> {
        if self.soundtrack.as_ref().is_none_or(|(s, _)| *s != stamp) {
            self.soundtrack = Some((stamp, mix().map(Arc::new)));
        }
        self.soundtrack.as_ref().and_then(|(_, pcm)| pcm.clone())
    }

    /// Plays from `origin`, with `track` when there is one.
    pub(crate) fn start(&mut self, origin: Tick, track: Option<Arc<Pcm>>) -> Started {
        self.playing = true;
        self.origin = origin;
        self.started = Instant::now();
        self.time = origin;
        // Speakers may have been plugged in since start-up.
        if track.is_some() && self.player.is_none() {
            self.player = device::Player::new();
        }
        let (started, clock) = match (&self.player, track) {
            (Some(player), Some(pcm)) => {
                player.play(pcm, (origin.as_secs_f64() * f64::from(kn_audio::RATE)) as usize);
                (Started::WithSound, true)
            }
            (None, Some(_)) => (Started::NoSpeakers, false),
            (_, None) => (Started::Silent, false),
        };
        self.audio_clock = clock;
        started
    }

    /// Stops the picture and the sound.
    pub(crate) fn stop(&mut self) {
        self.playing = false;
        self.stop_sound();
    }

    /// Silences the speakers but leaves the picture's state alone.
    pub(crate) fn stop_sound(&self) {
        if let Some(player) = &self.player {
            player.stop();
        }
    }

    /// Moves `time` on to now. With sound, the sound card is the clock so
    /// picture never drifts from it; without, time runs from a fixed start.
    pub(crate) fn tick(&mut self) -> Tick {
        let secs = match (&self.player, self.audio_clock) {
            // Hold at the start until the first sound reaches the speakers,
            // rather than stepping back by the output latency.
            (Some(player), true) => player.position_secs().max(self.origin.as_secs_f64()),
            _ => self.origin.as_secs_f64() + self.started.elapsed().as_secs_f64(),
        };
        self.time = Tick((secs * kn_time::TICKS_PER_SECOND as f64) as i64);
        self.time
    }
}
