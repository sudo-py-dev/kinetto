//! Background music composed on the spot, so every video can have a
//! soundtrack that is free to use anywhere.
//!
//! Each mood is a small song rather than a loop: an intro, then verses,
//! choruses and a breakdown that cycle for as long as the video needs. The
//! chords move with smooth voice leading, a melody is written from a seeded
//! motif, and the parts are played by simple synth instruments (electric
//! piano, plucked strings, bells, strings, saw pads) over a groove that
//! suits the mood. The same mood always sounds the same.

use crate::{CHANNELS, Pcm, RATE};
use kn_model::Mood;
use std::f32::consts::TAU;

// ---------------------------------------------------------------- styles

#[derive(Clone, Copy, PartialEq)]
enum Keys {
    /// Warm FM electric piano.
    Piano,
    /// Plucked string (Karplus–Strong).
    Pluck,
    /// Soft bell / mallet.
    Bell,
    /// Slow bowed strings.
    Strings,
    /// Wide detuned saw pad.
    Pad,
}

#[derive(Clone, Copy, PartialEq)]
enum Kit {
    None,
    /// Dusty swung boom-bap with vinyl crackle.
    Lofi,
    /// Kick and snare backbeat with eighth hats.
    Pop,
    /// Four on the floor with open hats.
    Four,
    /// Big toms and slow hits.
    Cinematic,
    /// Brushed shaker and rim, no kick on most beats.
    Soft,
}

struct Style {
    bpm: f32,
    /// Tonic as semitones from A4.
    key: i32,
    minor: bool,
    /// Scale degrees (0 = tonic) for the verse and chorus, one per bar.
    verse: [usize; 4],
    chorus: [usize; 4],
    sevenths: bool,
    keys: Keys,
    lead: Keys,
    /// A pad under everything (in addition to `keys`).
    pad: f32,
    kit: Kit,
    /// 0 straight, ~0.6 heavy swing on sixteenths.
    swing: f32,
    /// Notes per bar the melody aims for.
    density: f32,
    /// How hard the music breathes with the kick (0 none).
    pump: f32,
    /// Arpeggio pattern on sixteenths, None for no arpeggio.
    arp: Option<Keys>,
    seed: u32,
}

fn style(mood: Mood) -> Style {
    use Keys::*;
    let base = Style {
        bpm: 100.0, key: -9, minor: false, verse: [0, 5, 3, 4], chorus: [3, 4, 0, 5], sevenths: false,
        keys: Piano, lead: Bell, pad: 0.0, kit: Kit::Pop, swing: 0.0, density: 4.0, pump: 0.0, arp: None, seed: 1,
    };
    match mood {
        // C major · I vi IV V / IV V I vi
        Mood::Calm => Style { bpm: 72.0, keys: Piano, lead: Piano, pad: 0.5, kit: Kit::Soft, density: 3.0, sevenths: true, seed: 11, ..base },
        // G major · I V vi IV
        Mood::Upbeat => Style {
            bpm: 116.0, key: -14, verse: [0, 4, 5, 3], chorus: [3, 0, 4, 5], keys: Pluck, lead: Bell, pad: 0.35,
            kit: Kit::Pop, density: 6.0, pump: 0.35, arp: Some(Pluck), seed: 23, ..base
        },
        // D minor · i VI III VII
        Mood::Epic => Style {
            bpm: 84.0, key: -7, minor: true, verse: [0, 5, 2, 6], chorus: [5, 6, 0, 0], keys: Strings, lead: Strings,
            pad: 0.6, kit: Kit::Cinematic, density: 2.5, arp: Some(Pluck), seed: 37, ..base
        },
        // E major · Imaj7 IVmaj7 vi7 IV
        Mood::Ambient => Style {
            bpm: 60.0, key: -5, verse: [0, 3, 5, 3], chorus: [5, 3, 0, 4], sevenths: true, keys: Pad, lead: Bell,
            pad: 0.8, kit: Kit::None, density: 1.5, seed: 41, ..base
        },
        // F major jazzy · ii7 V7 Imaj7 vi7
        Mood::Lofi => Style {
            bpm: 78.0, key: -16, verse: [1, 4, 0, 5], chorus: [3, 2, 1, 4], sevenths: true, keys: Piano, lead: Piano,
            pad: 0.25, kit: Kit::Lofi, swing: 0.55, density: 3.5, seed: 53, ..base
        },
        // A♭ major · I iii IV iv-ish
        Mood::Dreamy => Style {
            bpm: 66.0, key: -1, verse: [0, 2, 3, 3], chorus: [5, 3, 0, 4], sevenths: true, keys: Pad, lead: Bell,
            pad: 0.6, kit: Kit::Soft, density: 2.5, arp: Some(Bell), seed: 67, ..base
        },
        // C major bouncy · I IV V I
        Mood::Playful => Style {
            bpm: 128.0, verse: [0, 3, 4, 0], chorus: [3, 4, 2, 5], keys: Pluck, lead: Bell, pad: 0.15, kit: Kit::Pop,
            swing: 0.25, density: 7.0, arp: Some(Pluck), seed: 79, ..base
        },
        // D major · I V vi IV, steady
        Mood::Corporate => Style {
            bpm: 104.0, key: -7, verse: [0, 4, 5, 3], chorus: [5, 3, 0, 4], sevenths: true, keys: Piano, lead: Pluck,
            pad: 0.4, kit: Kit::Four, density: 4.0, pump: 0.25, arp: Some(Pluck), seed: 89, ..base
        },
        // A minor · i VI III VII
        Mood::Dance => Style {
            bpm: 124.0, key: -12, minor: true, verse: [0, 5, 2, 6], chorus: [5, 6, 0, 4], keys: Pad, lead: Pad,
            pad: 0.5, kit: Kit::Four, density: 5.0, pump: 0.7, arp: Some(Pluck), seed: 97, ..base
        },
    }
}

const MAJOR: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [i32; 7] = [0, 2, 3, 5, 7, 8, 10];

impl Style {
    fn scale(&self) -> &'static [i32; 7] {
        if self.minor { &MINOR } else { &MAJOR }
    }
    /// The pitch of scale step `step` (any integer, octaves wrap).
    fn step(&self, step: i32) -> i32 {
        let (oct, deg) = (step.div_euclid(7), step.rem_euclid(7) as usize);
        self.key + oct * 12 + self.scale()[deg]
    }
    /// Scale steps of the chord on `degree`.
    fn chord_steps(&self, degree: usize) -> Vec<i32> {
        let d = degree as i32;
        let mut c = vec![d, d + 2, d + 4];
        if self.sevenths {
            c.push(d + 6);
        }
        c
    }
}

// ----------------------------------------------------------- arrangement

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Intro,
    Verse,
    Chorus,
    Break,
}

/// The song form after a four-bar intro, in bars. It repeats.
const FORM: [(Part, usize); 5] = [(Part::Verse, 8), (Part::Chorus, 8), (Part::Verse, 8), (Part::Chorus, 8), (Part::Break, 4)];
const INTRO_BARS: usize = 4;

/// The part playing in `bar`, how far into it, and its length.
fn part_at(bar: usize) -> (Part, usize, usize) {
    if bar < INTRO_BARS {
        return (Part::Intro, bar, INTRO_BARS);
    }
    let cycle: usize = FORM.iter().map(|p| p.1).sum();
    let mut b = (bar - INTRO_BARS) % cycle;
    for &(part, len) in &FORM {
        if b < len {
            return (part, b, len);
        }
        b -= len;
    }
    unreachable!()
}

// --------------------------------------------------------------- helpers

fn freq(semitones: i32) -> f32 {
    440.0 * 2f32.powf(semitones as f32 / 12.0)
}

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn unit(&mut self) -> f32 {
        self.next() as f32 / u32::MAX as f32
    }
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n.max(1)
    }
}

/// A fast sine of `x` radians (any range), within about 0.1% of the real
/// thing: plenty for synth voices, and many times quicker than `sin`.
fn sine(x: f32) -> f32 {
    let t = x * (1.0 / TAU);
    let t = t - (t + 0.5).floor();
    let y = 8.0 * t - 16.0 * t * t.abs();
    0.225 * (y * y.abs() - y) + y
}

/// Band-limited saw by polyBLEP.
fn saw(phase: f32, dt: f32) -> f32 {
    let mut v = 2.0 * phase - 1.0;
    if phase < dt {
        let t = phase / dt;
        v -= t + t - t * t - 1.0;
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        v -= t * t + t + t + 1.0;
    }
    v
}

/// Multiplier that makes a level fall by 1/e every `secs` seconds.
fn falloff(secs: f32) -> f32 {
    (-1.0 / (secs.max(1e-4) * RATE as f32)).exp()
}

/// An attack–decay–sustain–release envelope stepped once per sample, so
/// no `exp` runs in the inner loops.
struct Adsr {
    n: usize,
    attack: usize,
    held: usize,
    sustain: f32,
    decay: f32,
    decay_mul: f32,
    release: f32,
    release_mul: f32,
}

impl Adsr {
    /// A note held `len` seconds with attack `a`, decay time `d` towards
    /// sustain level `s`, and release time `r`.
    fn new(len: f32, a: f32, d: f32, s: f32, r: f32) -> Self {
        let rate = RATE as f32;
        Self {
            n: 0,
            attack: ((a * rate) as usize).max(1),
            held: (len * rate) as usize,
            sustain: s,
            decay: 1.0,
            decay_mul: falloff(d),
            release: 1.0,
            release_mul: falloff(r),
        }
    }
    fn next(&mut self) -> f32 {
        let level = if self.n < self.attack {
            self.n as f32 / self.attack as f32
        } else {
            self.decay *= self.decay_mul;
            self.sustain + (1.0 - self.sustain) * self.decay
        };
        if self.n >= self.held {
            self.release *= self.release_mul;
        }
        self.n += 1;
        level * self.release
    }
    /// The note has faded out completely.
    fn done(&self) -> bool {
        self.n > self.held && self.release < 1e-4
    }
}

/// Advance a 0..1 phase by `inc` and return it in radians.
fn turn(phase: &mut f32, inc: f32) -> f32 {
    *phase += inc;
    if *phase >= 1.0 {
        *phase -= 1.0;
    }
    *phase * TAU
}

// ------------------------------------------------------------ the mixer

struct Bus {
    dry: Vec<f32>,
    /// Sent to the reverb.
    wet: Vec<f32>,
    /// Instruments the kick pushes down.
    pumped: Vec<f32>,
    frames: usize,
}

impl Bus {
    fn new(frames: usize) -> Self {
        Self { dry: vec![0.0; frames * CHANNELS], wet: vec![0.0; frames * CHANNELS], pumped: vec![0.0; frames * CHANNELS], frames }
    }
}

struct Note {
    at: f32,
    len: f32,
    pitch: i32,
    vel: f32,
    pan: f32,
}

/// How long an instrument rings after its note ends.
fn tail(inst: Keys) -> f32 {
    match inst {
        Keys::Piano => 1.2,
        Keys::Pluck => 1.0,
        Keys::Bell => 2.0,
        Keys::Strings => 0.9,
        Keys::Pad => 1.2,
    }
}

fn play(bus: &mut Bus, inst: Keys, note: &Note, send: f32, pumped: bool, rng: &mut Rng) {
    let rate = RATE as f32;
    let start = (note.at * rate) as usize;
    if start >= bus.frames {
        return;
    }
    let end = (((note.at + note.len + tail(inst)) * rate) as usize).min(bus.frames);
    let f = freq(note.pitch);
    let (gl, gr) = ((1.0 - note.pan).sqrt() * note.vel, note.pan.sqrt() * note.vel);
    let target = if pumped { &mut bus.pumped } else { &mut bus.dry };

    // Per-instrument state.
    let mut phases = [rng.unit(), rng.unit(), rng.unit(), rng.unit()];
    let mut lp = [0.0f32; 2];
    let period = (rate / f).max(2.0) as usize;
    let mut ks: Vec<f32> = if inst == Keys::Pluck { (0..period).map(|_| rng.signed()).collect() } else { Vec::new() };
    let mut ks_pos = 0;
    let mut ks_last = 0.0;
    let held = (note.len * rate) as usize;
    let mut env = match inst {
        Keys::Piano => Adsr::new(note.len, 0.004, 0.9, 0.35, 0.35),
        Keys::Pluck => Adsr::new(note.len, 0.001, 10.0, 1.0, 0.12),
        Keys::Bell => Adsr::new(note.len.max(2.0), 0.003, 0.6, 0.0, 0.6),
        Keys::Strings => Adsr::new(note.len, 0.35, 1.0, 0.85, 0.6),
        Keys::Pad => Adsr::new(note.len, 0.6, 1.0, 0.85, 0.9),
    };
    // Fast decays inside a note: FM brightness, overtones.
    let (mut fast, fast_mul) = (1.0f32, falloff(if inst == Keys::Piano { 1.0 / 6.0 } else { 0.2 }));
    let (mut faster, faster_mul) = (1.0f32, falloff(if inst == Keys::Piano { 0.25 } else { 0.11 }));
    let inc = f / rate;
    let (vib_inc, trem_inc) = (5.2 / rate, 4.5 / rate);

    let range = start * CHANNELS..end * CHANNELS;
    let outs = target[range.clone()].as_chunks_mut::<CHANNELS>().0.iter_mut().zip(bus.wet[range].as_chunks_mut::<CHANNELS>().0);
    for (n, (out, wet)) in outs.enumerate() {
        let level = env.next();
        if env.done() {
            break;
        }
        fast *= fast_mul;
        faster *= faster_mul;
        let (l, r) = match inst {
            Keys::Piano => {
                // Two-operator FM: the bite fades into a mellow sine.
                let p = turn(&mut phases[0], inc);
                let p2 = turn(&mut phases[1], inc * 2.0);
                let trem = 1.0 + 0.06 * sine(turn(&mut phases[2], trem_inc));
                let m = sine(p) * (1.6 * fast + 0.25);
                let v = (sine(p + m) + 0.25 * sine(p2) * faster) * level;
                (v * trem, v * (2.0 - trem))
            }
            Keys::Pluck => {
                let y = ks[ks_pos];
                let next = ks[(ks_pos + 1) % period];
                let damp = if n < held { 0.996 } else { 0.95 };
                ks[ks_pos] = 0.5 * (y + next) * damp;
                ks_pos = (ks_pos + 1) % period;
                // Gentle lowpass softens the attack.
                ks_last += (y - ks_last) * 0.55;
                (ks_last * 0.9 * level, ks_last * 0.9 * level)
            }
            Keys::Bell => {
                let v = sine(turn(&mut phases[0], inc))
                    + 0.35 * sine(turn(&mut phases[1], inc * 2.756)) * fast
                    + 0.12 * sine(turn(&mut phases[2], inc * 5.404)) * faster;
                (v * level * 0.7, v * level * 0.7)
            }
            Keys::Strings | Keys::Pad => {
                let (detune, cut) = if inst == Keys::Strings { (0.004, 0.08) } else { (0.007, 0.05) };
                let depth = (n as f32 / (0.6 * rate)).min(1.0);
                let vib = 1.0 + 0.003 * sine(turn(&mut phases[3], vib_inc)) * depth;
                let mut v = [0.0f32; 2];
                for (k, ph) in phases.iter_mut().take(3).enumerate() {
                    let dt = inc * vib * (1.0 + detune * (k as f32 - 1.0));
                    *ph += dt;
                    if *ph >= 1.0 {
                        *ph -= 1.0;
                    }
                    let s = saw(*ph, dt);
                    match k {
                        0 => v[0] += s,
                        1 => {
                            v[0] += s * 0.5;
                            v[1] += s * 0.5;
                        }
                        _ => v[1] += s,
                    }
                }
                // Lowpass opens a little with the swell.
                let c = cut * (0.6 + 0.4 * level);
                for side in 0..2 {
                    lp[side] += (v[side] - lp[side]) * c;
                }
                (lp[0] * level * 0.45, lp[1] * level * 0.45)
            }
        };
        let (l, r) = (l * gl, r * gr);
        out[0] += l;
        out[1] += r;
        wet[0] += l * send;
        wet[1] += r * send;
    }
}

fn bass(bus: &mut Bus, note: &Note, round: bool) {
    let rate = RATE as f32;
    let start = (note.at * rate) as usize;
    if start >= bus.frames {
        return;
    }
    let end = (((note.at + note.len + 0.4) * rate) as usize).min(bus.frames);
    let f = freq(note.pitch);
    let (mut ph, mut lp) = (0.0f32, 0.0f32);
    let mut env = Adsr::new(note.len, 0.006, 0.4, 0.7, 0.06);
    let (mut bite, bite_mul) = (1.0f32, falloff(0.125));
    let dt = f / rate;
    for out in bus.pumped[start * CHANNELS..end * CHANNELS].as_chunks_mut::<CHANNELS>().0 {
        let level = env.next();
        bite *= bite_mul;
        let sub = sine(turn(&mut ph, dt));
        let v = if round {
            sub
        } else {
            lp += (saw(ph, dt) - lp) * (0.04 + 0.12 * bite);
            sub * 0.7 + lp * 0.6
        };
        let v = v * level * note.vel;
        out[0] += v;
        out[1] += v;
    }
}

// ------------------------------------------------------------- drums

#[derive(Clone, Copy)]
enum Hit {
    Kick,
    Snare,
    Rim,
    Hat,
    OpenHat,
    Shaker,
    Tom(f32),
    Crash,
}

struct Drums {
    noise: Rng,
}

impl Drums {
    fn hit(&mut self, bus: &mut Bus, hit: Hit, at: f32, vel: f32, lofi: bool) {
        let rate = RATE as f32;
        let start = (at.max(0.0) * rate) as usize;
        let dur = match hit {
            Hit::Kick => 0.45,
            Hit::Snare => 0.3,
            Hit::Rim => 0.08,
            Hit::Hat => 0.06,
            Hit::OpenHat => 0.35,
            Hit::Shaker => 0.09,
            Hit::Tom(_) => 0.7,
            Hit::Crash => 2.2,
        };
        let end = (start + (dur * rate) as usize).min(bus.frames);
        let (mut hp, mut last, mut bp, mut ph) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let pan = match hit {
            Hit::Hat | Hit::Shaker => 0.62,
            Hit::OpenHat => 0.4,
            Hit::Tom(p) => 0.35 + p * 0.3,
            _ => 0.5,
        };
        for i in start..end {
            let t = (i - start) as f32 / rate;
            let n = self.noise.signed();
            hp = 0.55 * (hp + n - last);
            last = n;
            let v = match hit {
                Hit::Kick => {
                    let f = 45.0 + 110.0 * (-t * 28.0).exp();
                    ph += f / rate;
                    let click = n * (-t * 300.0).exp() * 0.25;
                    (sine(TAU * ph) * (-t * if lofi { 9.0 } else { 7.0 }).exp() + click) * 0.95
                }
                Hit::Snare => {
                    bp += (n - bp) * 0.35;
                    let tone = sine(TAU * 185.0 * t) * (-t * 25.0).exp() * 0.5;
                    (bp * (-t * 14.0).exp() + tone) * 0.55
                }
                Hit::Rim => (sine(TAU * 820.0 * t) * 0.6 + hp * 0.5) * (-t * 60.0).exp() * 0.4,
                Hit::Hat => hp * (-t * 70.0).exp() * 0.32,
                Hit::OpenHat => hp * (-t * 9.0).exp() * 0.22,
                Hit::Shaker => hp * (t / 0.012).min(1.0) * (-t * 45.0).exp() * 0.22,
                Hit::Tom(p) => {
                    let f = (90.0 + 50.0 * p) * (1.0 + 0.5 * (-t * 20.0).exp());
                    ph += f / rate;
                    sine(TAU * ph) * (-t * 5.0).exp() * 0.8 + n * (-t * 40.0).exp() * 0.1
                }
                Hit::Crash => hp * (t / 0.003).min(1.0) * (-t * 2.2).exp() * 0.18,
            } * vel;
            bus.dry[i * CHANNELS] += v * (1.0 - pan) * 1.4;
            bus.dry[i * CHANNELS + 1] += v * pan * 1.4;
            let send = if matches!(hit, Hit::Snare | Hit::Tom(_) | Hit::Rim) { 0.25 } else { 0.05 };
            bus.wet[i * CHANNELS] += v * send;
            bus.wet[i * CHANNELS + 1] += v * send;
        }
    }
}

/// One bar of `kit`. `energy` 1 is the verse, 2 the chorus; `fill` plays a
/// fill in the last beat. Returns the kick times for the pump.
#[allow(clippy::too_many_arguments)]
fn drum_bar(drums: &mut Drums, bus: &mut Bus, kit: Kit, bar_at: f32, beat: f32, swing: f32, energy: u8, fill: bool, crash: bool, kicks: &mut Vec<f32>) {
    let six = beat / 4.0;
    // Sixteenth position with swing pushed onto the off-sixteenths.
    let at = |s: usize| bar_at + s as f32 * six + if s % 2 == 1 { swing * six * 0.5 } else { 0.0 };
    let lofi = kit == Kit::Lofi;
    let mut kick = |d: &mut Drums, bus: &mut Bus, s: usize, v: f32| {
        d.hit(bus, Hit::Kick, at(s), v, lofi);
        kicks.push(at(s));
    };
    if crash && kit != Kit::Lofi && kit != Kit::Soft {
        drums.hit(bus, Hit::Crash, bar_at, 0.9, false);
    }
    let ghost = |s: usize| 0.75 + 0.25 * ((s * 7 + 3) % 5) as f32 / 4.0;
    match kit {
        Kit::None => {}
        Kit::Lofi => {
            for s in [0, 7, 10] {
                kick(drums, bus, s, 0.9);
            }
            for s in [4, 12] {
                drums.hit(bus, Hit::Snare, at(s), 0.8, true);
            }
            for s in (0..16).step_by(2) {
                drums.hit(bus, Hit::Hat, at(s), 0.6 * ghost(s), true);
            }
            if energy > 1 {
                for s in [3, 11, 15] {
                    drums.hit(bus, Hit::Hat, at(s), 0.35, true);
                }
            }
        }
        Kit::Pop => {
            for s in if energy > 1 { vec![0, 6, 8, 10] } else { vec![0, 8, 10] } {
                kick(drums, bus, s, 1.0);
            }
            for s in [4, 12] {
                drums.hit(bus, Hit::Snare, at(s), 0.9, false);
            }
            let step = if energy > 1 { 1 } else { 2 };
            for s in (0..16).step_by(step) {
                drums.hit(bus, Hit::Hat, at(s), if s % 2 == 0 { 0.7 } else { 0.4 } * ghost(s), false);
            }
        }
        Kit::Four => {
            for s in [0, 4, 8, 12] {
                kick(drums, bus, s, 1.0);
            }
            if energy > 1 {
                for s in [4, 12] {
                    drums.hit(bus, Hit::Snare, at(s), 0.7, false);
                }
            }
            for s in [2, 6, 10, 14] {
                drums.hit(bus, Hit::OpenHat, at(s), 0.8, false);
            }
            for s in (0..16).step_by(1) {
                if s % 4 != 2 {
                    drums.hit(bus, Hit::Hat, at(s), 0.35 * ghost(s), false);
                }
            }
        }
        Kit::Cinematic => {
            kick(drums, bus, 0, 1.0);
            if energy > 1 {
                kick(drums, bus, 10, 0.8);
                for s in [4, 12] {
                    drums.hit(bus, Hit::Snare, at(s), 0.8, false);
                }
                for s in [6, 7, 14] {
                    drums.hit(bus, Hit::Tom(0.0), at(s), 0.6, false);
                }
            } else {
                drums.hit(bus, Hit::Tom(0.0), at(8), 0.8, false);
            }
            for s in (0..16).step_by(2) {
                drums.hit(bus, Hit::Shaker, at(s), 0.5, false);
            }
        }
        Kit::Soft => {
            if energy > 1 {
                kick(drums, bus, 0, 0.6);
                kick(drums, bus, 10, 0.5);
            }
            drums.hit(bus, Hit::Rim, at(4), 0.6, false);
            drums.hit(bus, Hit::Rim, at(12), 0.6, false);
            for s in (0..16).step_by(2) {
                drums.hit(bus, Hit::Shaker, at(s), 0.5 * ghost(s), false);
            }
        }
    }
    if fill && kit != Kit::None {
        for (k, s) in (12..16).enumerate() {
            let hit = if matches!(kit, Kit::Cinematic | Kit::Pop) { Hit::Tom(1.0 - k as f32 / 3.0) } else { Hit::Snare };
            drums.hit(bus, hit, at(s), 0.5 + 0.1 * k as f32, lofi);
        }
    }
}

// ------------------------------------------------------------- melody

/// A two-bar motif: (sixteenth, length in sixteenths, scale-step offset).
fn motif(rng: &mut Rng, density: f32) -> Vec<(usize, usize, i32)> {
    let mut notes = Vec::new();
    let mut s = 0;
    let mut step = 0i32;
    let avg = (16.0 / density).clamp(1.0, 8.0);
    while s < 30 {
        let lens = [2usize, 2, 3, 4, 4, 6];
        let mut len = lens[rng.below(lens.len())];
        if (len as f32) < avg * 0.5 {
            len *= 2;
        }
        // Rest sometimes, to let it breathe.
        if rng.unit() > 0.18 || s == 0 {
            notes.push((s, len.min(32 - s), step));
        }
        s += len;
        let moves = [-2i32, -1, -1, 1, 1, 2, 0, 3, -3];
        step = (step + moves[rng.below(moves.len())]).clamp(-3, 6);
    }
    notes
}

/// Snap a scale step to the nearest chord tone (for strong beats).
fn to_chord(step: i32, chord: &[i32]) -> i32 {
    (step - 7..=step + 7)
        .filter(|s| chord.iter().any(|c| (s - c).rem_euclid(7) == 0))
        .min_by_key(|s| (s - step).abs())
        .unwrap_or(step)
}

/// Choose the voicing of `chord` (scale steps) closest to `prev`.
fn voice(st: &Style, chord: &[i32], prev: &[i32]) -> Vec<i32> {
    let center = if prev.is_empty() { st.key - 7 } else { prev.iter().sum::<i32>() / prev.len() as i32 };
    let mut out: Vec<i32> = chord
        .iter()
        .map(|&s| {
            let p = st.step(s);
            let oct = ((center - p) as f32 / 12.0).round() as i32;
            p + oct * 12
        })
        .collect();
    out.sort();
    out
}

// --------------------------------------------------------------- compose

/// `frames` of stereo music in `mood`. The same mood always sounds the same.
pub fn generate(mood: Mood, frames: usize) -> Pcm {
    let st = style(mood);
    let rate = RATE as f32;
    let beat = 60.0 / st.bpm;
    let bar = beat * 4.0;
    let six = beat / 4.0;
    let seconds = frames as f32 / rate;
    let bars = (seconds / bar).ceil() as usize + 1;

    let mut bus = Bus::new(frames);
    let mut rng = Rng(0x9e37_79b9 ^ st.seed.wrapping_mul(0x85eb_ca6b));
    let mut drums = Drums { noise: Rng(0x1234_5677 ^ st.seed) };
    let mut kicks = Vec::new();

    // Two motifs: one for verses, a higher one for choruses.
    let verse_motif = motif(&mut rng, st.density * 0.7);
    let chorus_motif = motif(&mut rng, st.density);
    let mut prev_voicing: Vec<i32> = Vec::new();

    for b in 0..bars {
        let at = b as f32 * bar;
        let (part, in_part, part_len) = part_at(b);
        let prog = if part == Part::Chorus { &st.chorus } else { &st.verse };
        let degree = prog[in_part % 4];
        let chord = st.chord_steps(degree);
        let voicing = voice(&st, &chord, &prev_voicing);
        prev_voicing = voicing.clone();
        let energy: u8 = match part {
            Part::Intro | Part::Break => 0,
            Part::Verse => 1,
            Part::Chorus => 2,
        };
        let last_bar = in_part + 1 == part_len;
        let lift = if energy == 2 { 1.0 } else { 0.8 };

        // Pad under everything.
        if st.pad > 0.0 {
            for (k, &p) in voicing.iter().enumerate() {
                let note = Note { at, len: bar, pitch: p, vel: st.pad * 0.16 * lift, pan: 0.3 + 0.4 * k as f32 / voicing.len() as f32 };
                play(&mut bus, if st.keys == Keys::Strings { Keys::Strings } else { Keys::Pad }, &note, 0.5, true, &mut rng);
            }
        }

        // Chords on the keys instrument, with a rhythm per part.
        if st.keys != Keys::Pad && st.keys != Keys::Strings {
            let hits: &[(usize, usize)] = match (st.keys, energy) {
                (Keys::Piano, 0) => &[(0, 16)],
                (Keys::Piano, _) if st.kit == Kit::Lofi => &[(0, 6), (7, 3), (10, 6)],
                (Keys::Piano, 1) => &[(0, 8), (8, 8)],
                (Keys::Piano, _) => &[(0, 6), (6, 4), (10, 6)],
                (_, 0) => &[(0, 8), (8, 8)],
                _ => &[(0, 3), (3, 3), (6, 2), (8, 3), (11, 3), (14, 2)],
            };
            for &(s, len) in hits {
                for (k, &p) in voicing.iter().enumerate() {
                    // A soft roll on the piano so chords aren't blocky.
                    let roll = if st.keys == Keys::Piano { k as f32 * 0.012 } else { 0.0 };
                    let pitch = if st.keys == Keys::Pluck { p + 12 } else { p };
                    let note = Note {
                        at: at + s as f32 * six + roll,
                        len: len as f32 * six,
                        pitch,
                        vel: 0.11 * lift * if s == 0 { 1.0 } else { 0.8 },
                        pan: 0.25 + 0.5 * k as f32 / voicing.len() as f32,
                    };
                    play(&mut bus, st.keys, &note, 0.35, true, &mut rng);
                }
            }
        }

        // Arpeggio up an octave in the busier parts.
        if let Some(inst) = st.arp
            && energy >= 1
            && (energy == 2 || st.keys == Keys::Pad || st.keys == Keys::Strings)
        {
            let pattern = [0usize, 1, 2, 1, 2, 3, 2, 1];
            let step = if st.bpm > 110.0 { 2 } else { 1 };
            for (n, s) in (0..16).step_by(step).enumerate() {
                let i = pattern[n % pattern.len()] % voicing.len();
                let p = voicing[i] + if inst == Keys::Bell { 24 } else { 12 };
                let swing = if s % 2 == 1 { st.swing * six * 0.5 } else { 0.0 };
                let note = Note { at: at + s as f32 * six + swing, len: six * 1.5, pitch: p, vel: 0.07, pan: if n % 2 == 0 { 0.3 } else { 0.7 } };
                play(&mut bus, inst, &note, 0.45, true, &mut rng);
            }
        }

        // Bass follows the root with a part-dependent rhythm.
        if energy >= 1 || part == Part::Break && in_part >= 2 {
            let root = st.step(degree as i32) - 24 + if st.key > -6 { -12 } else { 0 };
            let pattern: &[(usize, usize)] = match (st.kit, energy) {
                (Kit::Four, _) => &[(2, 2), (6, 2), (10, 2), (14, 2)],
                (Kit::Lofi, _) => &[(0, 6), (7, 3), (10, 5)],
                (Kit::None | Kit::Soft, _) => &[(0, 16)],
                (_, 1) => &[(0, 6), (8, 6)],
                _ => &[(0, 3), (3, 3), (6, 2), (8, 3), (11, 3), (14, 2)],
            };
            for &(s, len) in pattern {
                let note = Note { at: at + s as f32 * six, len: len as f32 * six * 0.9, pitch: root, vel: 0.32, pan: 0.5 };
                bass(&mut bus, &note, matches!(st.kit, Kit::None | Kit::Soft | Kit::Lofi));
            }
        }

        // Melody: the motif over each pair of bars, snapped to the chords.
        let sing = match part {
            Part::Chorus => Some((&chorus_motif, 7)),
            Part::Verse if in_part >= 4 => Some((&verse_motif, 0)),
            Part::Break => Some((&verse_motif, 7)),
            _ => None,
        };
        if let Some((m, lift_steps)) = sing {
            let half = in_part % 2;
            let phrase_end = in_part + 1 >= part_len && half == 1;
            for &(s, len, off) in m.iter().filter(|n| n.0 / 16 == half) {
                let s_in_bar = s % 16;
                let mut step = degree as i32 + off + lift_steps;
                if s_in_bar % 4 == 0 {
                    step = to_chord(step, &chord);
                }
                if phrase_end && s_in_bar >= 8 {
                    // Resolve home at the end of the phrase.
                    step = to_chord(step, &[0, 2, 4]);
                }
                let swing = if s % 2 == 1 { st.swing * six * 0.5 } else { 0.0 };
                let note = Note {
                    at: at + s_in_bar as f32 * six + swing,
                    len: len as f32 * six * 0.9,
                    pitch: st.step(step) + if st.lead == Keys::Bell { 12 } else { 0 },
                    vel: if st.lead == Keys::Pad || st.lead == Keys::Strings { 0.2 } else { 0.17 },
                    pan: 0.5,
                };
                play(&mut bus, st.lead, &note, 0.55, false, &mut rng);
            }
        }

        // Drums.
        if energy >= 1 {
            drum_bar(&mut drums, &mut bus, st.kit, at, beat, st.swing, energy, last_bar, in_part == 0 && energy == 2, &mut kicks);
        } else if part == Part::Intro && in_part == INTRO_BARS - 1 && st.kit != Kit::None {
            // A pickup into the first verse.
            for s in [12, 14] {
                drums.hit(&mut bus, Hit::Snare, at + s as f32 * six, 0.4, false);
            }
        }
    }

    finish(&st, bus, &kicks)
}

/// Pump, reverb, warmth and a gentle limiter.
fn finish(st: &Style, mut bus: Bus, kicks: &[f32]) -> Pcm {
    let rate = RATE as f32;
    let frames = bus.frames;
    // Sidechain-style pump: the kick briefly pushes the pumped bus down.
    let mut pump = vec![1.0f32; frames];
    if st.pump > 0.0 {
        let recover = 60.0 / st.bpm * 0.45;
        for &k in kicks {
            let start = (k * rate) as usize;
            let end = (start + (recover * rate) as usize).min(frames);
            for (i, g) in pump.iter_mut().enumerate().take(end).skip(start) {
                let t = (i - start) as f32 / rate / recover;
                *g = g.min(1.0 - st.pump * (1.0 - t).powi(2));
            }
        }
    } else if st.kit != Kit::None {
        // Even without pumping, keep the low end from fighting the kick.
        for &k in kicks {
            let start = (k * rate) as usize;
            let end = (start + (0.08 * rate) as usize).min(frames);
            for (i, g) in pump.iter_mut().enumerate().take(end).skip(start) {
                *g = g.min(0.75 + 0.25 * (i - start) as f32 / (0.08 * rate));
            }
        }
    }
    let mut out = bus.dry;
    for (i, frame) in out.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
        for (c, s) in frame.iter_mut().enumerate() {
            *s += bus.pumped[i * CHANNELS + c] * pump[i];
        }
    }

    let room = match st.kit {
        Kit::None => 0.6,
        Kit::Soft => 0.45,
        Kit::Cinematic => 0.5,
        _ => 0.3,
    };
    reverb(&mut bus.wet, if st.kit == Kit::None || st.keys == Keys::Pad { 0.88 } else { 0.82 });
    for (o, w) in out.iter_mut().zip(&bus.wet) {
        *o += w * room;
    }

    if st.kit == Kit::Lofi {
        // Tape warmth and vinyl crackle.
        let mut crackle = Rng(0xdead_beef);
        let mut lp = [0.0f32; 2];
        for frame in out.as_chunks_mut::<CHANNELS>().0 {
            let pop = if crackle.unit() > 0.9993 { crackle.signed() * 0.05 } else { 0.0 };
            let hiss = crackle.signed() * 0.002;
            for (c, s) in frame.iter_mut().enumerate() {
                lp[c] += (*s - lp[c]) * 0.35;
                *s = lp[c] + pop + hiss;
            }
        }
    }

    // Glue: a slow-ish peak follower then soft clip.
    let mut env = 0.0f32;
    for frame in out.as_chunks_mut::<CHANNELS>().0 {
        let peak = frame[0].abs().max(frame[1].abs());
        env = if peak > env { env + (peak - env) * 0.01 } else { env * 0.99995 };
        let gain = if env > 0.7 { 0.7 / env } else { 1.0 };
        for s in frame.iter_mut() {
            *s = (*s * gain).tanh();
        }
    }
    Pcm { samples: out }
}

/// A Schroeder reverb: four combs and two all-passes per side.
fn reverb(samples: &mut [f32], feedback: f32) {
    const COMBS: [usize; 4] = [1557, 1617, 1491, 1422];
    const ALLPASS: [usize; 2] = [225, 556];
    for side in 0..CHANNELS {
        let spread = side * 23;
        let mut combs: Vec<(Vec<f32>, usize, f32)> = COMBS.iter().map(|&n| (vec![0.0; n * 2 + spread], 0, 0.0)).collect();
        let mut passes: Vec<(Vec<f32>, usize)> = ALLPASS.iter().map(|&n| (vec![0.0; n + spread], 0)).collect();
        for s in samples.iter_mut().skip(side).step_by(CHANNELS) {
            let dry = *s;
            let mut wet = 0.0;
            for (buf, pos, damp) in &mut combs {
                let y = buf[*pos];
                // Damping darkens the tail like a real room.
                *damp += (y - *damp) * 0.6;
                buf[*pos] = dry + *damp * feedback;
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
            *s = wet;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|s| s * s).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    #[test]
    fn every_mood_is_audible_finite_and_repeatable() {
        for mood in Mood::ALL {
            let a = generate(mood, RATE as usize * 2);
            assert_eq!(a.frames(), RATE as usize * 2);
            assert!(a.samples.iter().all(|s| s.is_finite()), "{mood:?}");
            assert!(rms(&a.samples) > 0.01, "{mood:?} is too quiet: {}", rms(&a.samples));
            assert_eq!(a.samples, generate(mood, RATE as usize * 2).samples, "{mood:?} isn't repeatable");
        }
    }

    #[test]
    fn songs_change_over_time_instead_of_looping_one_bar() {
        // The chorus must not be a copy of the verse.
        for mood in Mood::ALL {
            let st = style(mood);
            let bar = 240.0 / st.bpm;
            let pcm = generate(mood, (bar * 24.0 * RATE as f32) as usize);
            let slice = |b: usize| {
                let s = (b as f32 * bar * RATE as f32) as usize * CHANNELS;
                &pcm.samples[s..s + (bar * RATE as f32) as usize * CHANNELS]
            };
            let (verse, chorus) = (slice(INTRO_BARS + 1), slice(INTRO_BARS + 9));
            let diff: Vec<f32> = verse.iter().zip(chorus).map(|(a, b)| a - b).collect();
            assert!(rms(&diff) > 0.02, "{mood:?} verse and chorus sound the same");
            assert!(pcm.samples.iter().all(|s| s.abs() <= 1.0), "{mood:?} clips");
        }
    }
}
