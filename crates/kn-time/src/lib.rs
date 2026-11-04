//! Integer time base. All timing in Kinetto is in [`Tick`]s, never floats,
//! so frame boundaries never drift.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Sub};

/// Ticks per second. Divides evenly by 24, 25, 30, 50, 60, the NTSC rates
/// (24000/1001 etc.) and the 44.1 kHz and 48 kHz audio families.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Tick(pub i64);

impl Tick {
    pub const ZERO: Tick = Tick(0);

    pub const fn from_millis(ms: i64) -> Self {
        Tick(ms * (TICKS_PER_SECOND / 1000))
    }

    pub const fn from_secs(s: i64) -> Self {
        Tick(s * TICKS_PER_SECOND)
    }

    /// Lossy conversion for animation curves and display only.
    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / TICKS_PER_SECOND as f64
    }
}

impl Add for Tick {
    type Output = Tick;
    fn add(self, rhs: Tick) -> Tick {
        Tick(self.0 + rhs.0)
    }
}

impl Sub for Tick {
    type Output = Tick;
    fn sub(self, rhs: Tick) -> Tick {
        Tick(self.0 - rhs.0)
    }
}

/// A rational frame rate such as 30/1 or 30000/1001.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const FPS_30: FrameRate = FrameRate { num: 30, den: 1 };
    pub const FPS_60: FrameRate = FrameRate { num: 60, den: 1 };

    /// Exact duration of one frame.
    pub fn frame_duration(self) -> Tick {
        Tick(TICKS_PER_SECOND * self.den as i64 / self.num as i64)
    }

    /// Start time of frame `n`.
    pub fn frame_start(self, n: u64) -> Tick {
        Tick(n as i64 * self.frame_duration().0)
    }

    /// Number of whole frames needed to cover `duration` (rounded up).
    pub fn frames_in(self, duration: Tick) -> u64 {
        let d = self.frame_duration().0;
        ((duration.0 + d - 1) / d).max(0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_rates_divide_evenly() {
        for (num, den) in [(24, 1), (25, 1), (30, 1), (50, 1), (60, 1), (24000, 1001), (30000, 1001), (60000, 1001)] {
            assert_eq!(TICKS_PER_SECOND * den % num, 0, "{num}/{den}");
        }
        for rate in [44_100, 48_000, 88_200, 96_000] {
            assert_eq!(TICKS_PER_SECOND % rate, 0, "{rate} Hz");
        }
    }

    #[test]
    fn frames_round_up() {
        let r = FrameRate::FPS_30;
        assert_eq!(r.frames_in(Tick::from_secs(2)), 60);
        assert_eq!(r.frames_in(Tick::from_secs(2) + Tick(1)), 61);
        assert_eq!(r.frame_start(30), Tick::from_secs(1));
    }
}
