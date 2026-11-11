//! VOX's first cut, vowel first (kept as model work: the owner heard a
//! synth singing vowels, not a mouth; `vox` is the consonant-first
//! rebuild). Renders only as the `vox-before_*` hits; its groove and demo
//! stay buildable but are not rendered.
//!
//! Beatboxed drums. Voiced
//! hits are ALGO's FRMT operators on the VOWEL's F1 and F2; consonants are
//! ALGO's NOIS operators (noise ring-modulated up to a centre that follows
//! the VOWEL on its slot), so the noise goes through formants with no new
//! filter. Plosives, lip pops and tongue clicks are VA NOISE through CLEAN
//! or DRUM's two-pole modes. VOWEL glides inside every hit (VOWEL to END
//! VOWEL over GLIDE) and takes a modulation offset per hit or an LFO.
//!
//! The vowel path is ALGO's own table (Peterson & Barney 1952, adult male
//! averages, in `vowel.rs`): data, cited there. Nothing else is borrowed.

#![allow(dead_code)]

use super::machines::{Knobs, Machine, NoiseLayer, Swept};
use super::parts::*;
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::kinds::{Kinds, OpKind};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::{fast_sin_turns, note_to_freq};
use chimera_core::taps::TapRows;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

/// A band of the voice: FRMT (voiced, at the hit's pitch) or NOIS (the
/// consonant), its FRMT and BW codes and vowel slot (1–3 F1–F3, 4 OFF).
#[derive(Clone, Copy)]
pub struct Band {
    pub kind: OpKind,
    pub frmt: f32,
    pub bw: u8,
    pub slot: u8,
}

const fn frmt(frmt: f32, bw: u8, slot: u8) -> Band {
    Band {
        kind: OpKind::Formant,
        frmt,
        bw,
        slot,
    }
}

const fn nois(frmt: f32, bw: u8, slot: u8) -> Band {
    Band {
        kind: OpKind::Noise,
        frmt,
        bw,
        slot,
    }
}

/// ALGO's voice on A1 (every operator a carrier), each band's LEVEL, the
/// VOWEL and the pitch moved per block as a mod route would move them.
pub struct Formants {
    e: Box<AlgoEngine>,
    p: AlgoParams,
    live: AlgoLive,
    note: u8,
    n: usize,
    taps: TapRows,
}

impl Formants {
    pub fn new(bands: &[Band], hz: f32, seed: u32) -> Self {
        let w1 = OpKind::Table(WaveId::W1);
        let kinds = core::array::from_fn(|i| bands.get(i).map_or(w1, |b| b.kind));
        let ops = core::array::from_fn(|i| {
            let mut o = AlgoOpParams {
                coarse: 4, // ratio 1
                ar: 31,
                d1r: 0,
                d1l: 15,
                rr: 15,
                ..AlgoOpParams::default()
            };
            if let Some(b) = bands.get(i) {
                o.level = 99;
                o.frmt = b.frmt.round() as u8;
                o.bw = b.bw;
                o.vslot = b.slot;
            }
            o
        });
        let mut p = AlgoParams::new(AlgoId::A1, AlgoId::A1, 0, Kinds::new(kinds), ops);
        p.vowel = 50;
        let mut raw = Box::<AlgoEngine>::new_uninit();
        AlgoEngine::init_in_place(&mut raw);
        // SAFETY: `init_in_place` wrote every field of the engine.
        let mut e = unsafe { raw.assume_init() };
        e.seed(chimera_core::voice_alloc::VoiceIdx::new((seed % 8) as usize).unwrap());
        let mut note = 0u8;
        while note < 120 && note_to_freq(note + 1) <= hz {
            note += 1;
        }
        let mut live = AlgoLive::from_params(&p);
        for (i, b) in bands.iter().enumerate() {
            live.frmt[i] = b.frmt;
        }
        e.note_on(MidiNote::new(note).unwrap(), Velocity::MAX, &p, SAMPLE_RATE);
        Self {
            e,
            p,
            live,
            note,
            n: bands.len(),
            taps: TapRows::new([false; 2]),
        }
    }

    /// A block ending at band amplitudes `amp` (linear, 1 = LEVEL 99),
    /// VOWEL `vowel` (0–100, U O A E I), pitch `hz`; `shift` moves every
    /// band's FRMT (24 an octave).
    pub fn block(&mut self, out: &mut Blk, amp: &[f32], vowel: f32, hz: f32, shift: f32) {
        self.e
            .set_pitch(12.0 * (hz / note_to_freq(self.note)).log2());
        for i in 0..OPS.min(self.n) {
            let a = amp.get(i).copied().unwrap_or(0.0);
            // LEVEL's 0.75 dB a step, 99 at unity.
            self.live.level[i] = if a > 1e-4 {
                (99.0 + 20.0 * a.min(1.0).log10() / 0.75).max(0.0)
            } else {
                0.0
            };
            self.live.frmt[i] = (f32::from(self.p.ops[i].frmt) + shift).clamp(0.0, 168.0);
        }
        self.live.vowel = vowel.clamp(0.0, 100.0);
        self.e
            .render(out, &self.p, &self.live, SAMPLE_RATE, &mut self.taps);
    }
}

/// What moves VOWEL besides the knobs: an offset latched at the hit (a
/// velocity, random or step route) and an LFO running through it.
#[derive(Clone, Copy, Debug, Default)]
pub struct VowelMod {
    /// VOWEL units, added to VOWEL and END VOWEL.
    pub offset: f32,
    /// Hz, depth in VOWEL units, phase at the hit (turns).
    pub lfo: Option<(f32, f32, f32)>,
}

/// VOWEL to END VOWEL over GLIDE (k[4], k[6], k[5]), plus the mod.
struct Glide {
    start: f32,
    end: f32,
    g: Decay,
    m: VowelMod,
    t: f32,
}

impl Glide {
    fn new(k: &Knobs, m: VowelMod) -> Self {
        Self {
            start: 100.0 * k[4],
            end: 100.0 * k[6],
            g: Decay::tau(expo(k[5], 0.003, 0.25)),
            m,
            t: 0.0,
        }
    }

    /// The VOWEL at this block's end.
    fn next(&mut self) -> f32 {
        let g = self.g.v;
        for _ in 0..B {
            self.g.next();
        }
        self.t += B as f32 / FS;
        let lfo = self.m.lfo.map_or(0.0, |(hz, d, ph)| {
            d * fast_sin_turns((ph + hz * self.t).fract())
        });
        self.end + (self.start - self.end) * g + self.m.offset + lfo
    }
}

/// An attack and an exponential fall, per block: the bands' LEVEL laws.
struct Ar {
    att: f32,
    t: f32,
    d: Decay,
}

impl Ar {
    fn new(att: f32, t60: f32) -> Self {
        Self {
            att,
            t: 0.0,
            d: Decay::t60(t60),
        }
    }

    fn next(&mut self) -> f32 {
        for _ in 0..B {
            self.d.next();
        }
        self.t += B as f32 / FS;
        (self.t / self.att.max(1e-4)).min(1.0) * self.d.v
    }
}

// ---------------------------------------------------------------- kick

/// VOX KICK, "b" / "bm": a lip pop, then a voiced "oo" at the pitch as it
/// drops, over a swept sine for the weight.
pub struct Kick {
    v: Formants,
    body: Swept,
    amp: Decay,
    voice: Ar,
    pop: NoiseLayer,
    /// The push of air: a 0.3 ms rise into a 1.5 ms fall.
    puff: (Decay, Decay),
    glide: Glide,
    lvl: (f32, f32, f32),
    vel: f32,
}

impl Kick {
    pub fn new(k: Knobs, vel: f32, seed: u32, m: VowelMod) -> Self {
        let f0 = expo(k[0], 40.0, 110.0);
        let t60 = expo(k[1], 0.12, 1.6);
        Self {
            // "oo": F1 and F2 near U's; a low nasal hum under them.
            v: Formants::new(&[frmt(54.0, 40, 1), frmt(83.0, 52, 2)], f0, seed),
            body: Swept::new(f0, 30.0 * k[2], 0.018),
            amp: Decay::t60(t60),
            voice: Ar::new(0.004, 0.7 * t60),
            pop: NoiseLayer::new(seed, 0.004, FilterMode::Lp12, 2200.0, 0.2, 1.0),
            puff: (Decay::tau(0.0015), Decay::tau(0.0003)),
            glide: Glide::new(&k, m),
            lvl: (k[3], k[7], 1.0),
            vel,
        }
    }
}

impl Machine for Kick {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let hz = self.body.f0 * (self.body.oct * self.body.sw.v).exp2();
        let a = self.voice.next() * self.lvl.1;
        let mut f = [0.0; B];
        self.v
            .block(&mut f, &[a, 0.6 * a], self.glide.next(), hz, 0.0);
        let pop = self.pop.block();
        for (i, o) in out.iter_mut().enumerate() {
            let b = self.body.tick(1.0) * self.amp.next();
            // The pop: a burst of breath and one push of air.
            let p = self.lvl.0 * (0.6 * pop[i] + 1.1 * (self.puff.0.next() - self.puff.1.next()));
            *o = self.vel * soft(0.55 * b + 1.4 * f[i] + p);
        }
    }
}

fn soft(x: f32) -> f32 {
    x / (1.0 + x.abs() * 0.3)
}

// --------------------------------------------------------------- snare

/// VOX SNARE, "pf" / "k" / "psh": a plosive burst, then breath through
/// three NOIS bands (F2, F3 and an "s" over them) as the vowel glides
/// down, a short voiced "uh" under it.
pub struct Snare {
    v: Formants,
    plosive: NoiseLayer,
    breath: Ar,
    voice: Ar,
    glide: Glide,
    f0: f32,
    lvl: (f32, f32, f32),
    vel: f32,
}

impl Snare {
    pub fn new(k: Knobs, vel: f32, seed: u32, m: VowelMod) -> Self {
        let f0 = expo(k[0], 120.0, 320.0);
        let t60 = expo(k[1], 0.08, 0.7);
        Self {
            v: Formants::new(
                &[
                    nois(108.0, 150, 2),
                    nois(118.0, 160, 3),
                    nois(150.0, 185, 4),
                    frmt(69.0, 60, 1),
                    frmt(83.0, 70, 2),
                ],
                f0,
                seed,
            ),
            plosive: NoiseLayer::new(
                seed ^ 0x55,
                0.012,
                FilterMode::Bp12,
                expo(k[0], 900.0, 3200.0),
                0.4,
                1.0,
            ),
            breath: Ar::new(0.002, t60 * (0.4 + k[3])),
            voice: Ar::new(0.002, 0.12),
            glide: Glide::new(&k, m),
            f0,
            lvl: (k[2], k[3], k[7]),
            vel,
        }
    }
}

impl Machine for Snare {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let b = self.breath.next() * (0.3 + 0.7 * self.lvl.1);
        let a = self.voice.next() * self.lvl.2;
        let mut f = [0.0; B];
        self.v.block(
            &mut f,
            &[b, 0.8 * b, 0.6 * b, a, 0.7 * a],
            self.glide.next(),
            self.f0,
            0.0,
        );
        let p = self.plosive.block();
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.vel * soft(1.6 * f[i] + 1.2 * self.lvl.0 * p[i]);
        }
    }
}

// ----------------------------------------------------------------- hat

/// VOX HAT, "ts" / "t" / open "tsss": a "t" tick, then hiss through NOIS
/// bands on F2 and F3 high up (the vowel tilts "tsh" to "tss") and an air
/// band; DECAY runs closed to open.
pub struct Hat {
    v: Formants,
    tick: NoiseLayer,
    env: Ar,
    glide: Glide,
    shift: f32,
    lvl: (f32, f32),
    vel: f32,
}

impl Hat {
    pub fn new(k: Knobs, vel: f32, seed: u32, m: VowelMod) -> Self {
        let bw = lin(k[3], 150.0, 200.0) as u8;
        Self {
            v: Formants::new(
                &[nois(136.0, bw, 2), nois(146.0, bw, 3), nois(160.0, 190, 4)],
                440.0,
                seed,
            ),
            tick: NoiseLayer::new(seed ^ 0x33, 0.003, FilterMode::Hp24, 4500.0, 0.2, 1.0),
            // Open swells in a little, as a held "sss" does.
            env: Ar::new(lin(k[1], 0.0005, 0.012), expo(k[1], 0.05, 1.4)),
            glide: Glide::new(&k, m),
            shift: 24.0 * (k[0] - 0.5),
            lvl: (k[2], k[7]),
            vel,
        }
    }
}

impl Machine for Hat {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let e = self.env.next();
        let mut f = [0.0; B];
        self.v.block(
            &mut f,
            &[e, 0.8 * e, self.lvl.1 * e],
            self.glide.next(),
            440.0,
            self.shift,
        );
        let t = self.tick.block();
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.vel * soft(1.5 * f[i] + 1.1 * self.lvl.0 * t[i]);
        }
    }
}

// --------------------------------------------------------------- click

/// VOX CLICK, "ka" / a tongue click: the tongue's pop rings two modes of
/// the mouth's cavity, a breath of "k" through a NOIS band, and a tiny
/// voiced vowel gliding fast.
pub struct Click {
    v: Formants,
    cavity: Modes<2>,
    struck: bool,
    asp: Ar,
    voice: Ar,
    glide: Glide,
    f0: f32,
    lvl: (f32, f32),
    vel: f32,
}

impl Click {
    pub fn new(k: Knobs, vel: f32, seed: u32, m: VowelMod) -> Self {
        let f0 = expo(k[0], 90.0, 360.0);
        let mut cavity = Modes::<2>::new();
        cavity.ratio = [1.0, 1.52];
        cavity.amp = [1.0, 0.45];
        let ring = lin(k[7], 0.008, 0.05);
        cavity.t60 = [ring, 0.7 * ring];
        cavity.set(expo(k[3], 600.0, 3600.0));
        Self {
            v: Formants::new(
                &[frmt(69.0, 50, 1), frmt(83.0, 60, 2), nois(118.0, 160, 3)],
                f0,
                seed,
            ),
            cavity,
            struck: false,
            asp: Ar::new(0.0005, 0.03),
            voice: Ar::new(0.003, expo(k[1], 0.04, 0.4)),
            glide: Glide::new(&k, m),
            f0,
            lvl: (k[2], k[7]),
            vel,
        }
    }
}

impl Machine for Click {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let a = self.voice.next();
        let s = self.asp.next() * (0.3 + 0.7 * self.lvl.0);
        let mut f = [0.0; B];
        self.v.block(
            &mut f,
            &[a, 0.7 * a, 0.5 * s],
            self.glide.next(),
            self.f0,
            0.0,
        );
        for (i, o) in out.iter_mut().enumerate() {
            let x = if self.struck { 0.0 } else { 1.0 };
            self.struck = true;
            let c = self.cavity.tick(x);
            *o = self.vel * soft(1.5 * f[i] + 0.9 * self.lvl.0 * c);
        }
    }
}

// ------------------------------------------------------------ renders

use super::machines::{Id, spec};
use super::{Out, Setting, add_at, knob_map, knobs, run_hit};

/// A first-cut hit (the old VOX ids' structs).
pub fn build_vox(id: Id, k: Knobs, vel: f32, seed: u32, m: VowelMod) -> Box<dyn Machine> {
    match id {
        Id::VoxKick => Box::new(Kick::new(k, vel, seed, m)),
        Id::VoxSnare => Box::new(Snare::new(k, vel, seed, m)),
        Id::VoxHat => Box::new(Hat::new(k, vel, seed, m)),
        Id::VoxClick => Box::new(Click::new(k, vel, seed, m)),
        _ => panic!("{id:?} has no first-cut VOX"),
    }
}

/// The first cut's settings (its knobs: PITCH DECAY … VOWEL GLIDE END
/// VOWEL and a last one per machine).
pub fn settings(id: Id) -> Vec<Setting> {
    match id {
        Id::VoxKick => vec![
            ("classic", &[50, 60, 70, 60, 0, 30, 0, 90], "\"b\""),
            ("mid", &[70, 40, 90, 100, 40, 40, 0, 110], "\"bmo\""),
            (
                "extreme",
                &[20, 110, 127, 127, 127, 90, 0, 127],
                "\"bwee-oo\"",
            ),
        ],
        Id::VoxSnare => vec![
            ("classic", &[60, 50, 90, 70, 100, 40, 10, 40], "\"psh\""),
            ("mid", &[100, 30, 120, 30, 75, 20, 40, 70], "\"k-uh\""),
            (
                "extreme",
                &[30, 110, 60, 127, 127, 100, 0, 20],
                "\"pfffshhh\"",
            ),
        ],
        Id::VoxHat => vec![
            ("classic", &[64, 20, 80, 50, 100, 40, 95, 60], "\"ts\""),
            ("mid", &[80, 5, 120, 30, 100, 20, 100, 40], "\"t\""),
            (
                "extreme",
                &[50, 110, 60, 110, 127, 110, 25, 90],
                "open \"tsssh\"",
            ),
        ],
        Id::VoxClick => vec![
            ("classic", &[60, 40, 90, 70, 60, 20, 10, 50], "\"ka\""),
            ("mid", &[80, 15, 127, 40, 25, 10, 25, 90], "\"tok\""),
            ("extreme", &[40, 100, 70, 100, 0, 90, 127, 30], "\"kwee\""),
        ],
        _ => Vec::new(),
    }
}
use serde_json::{Value, json};

/// ALGO's billed cycles for the bands a VOX machine runs (ADR 0131's
/// counted terms on A1, no links): COST_BASE 370, FRMT 220 + 40 moving
/// (VOWEL moves every block), NOIS 80. The glue (sine, pop, modes) on top
/// is the host bench's.
pub fn billed(id: Id) -> Option<Value> {
    let (frmt, nois) = match id {
        Id::VoxKick => (2, 0),
        Id::VoxSnare => (2, 3),
        Id::VoxHat => (0, 3),
        Id::VoxClick => (2, 1),
        _ => return None,
    };
    let c = 370 + frmt * 260 + nois * 80;
    Some(json!({
        "frmt": frmt, "nois": nois, "cycles": c,
        "ratio_vs_algo6": (c as f64 / 770.0 * 100.0).round() / 100.0,
        "terms": "COST_BASE 370 + FRMT (220 + 40 moving) each + NOIS 80 each; ALGO6 = 770",
    }))
}

pub fn run(out: &mut Out) -> Result<(), String> {
    groove(out)?;
    demo(out)
}

/// One VOX hit, its VOWEL moved by `m`.
fn hit_m(id: Id, set: usize, vel: f32, seed: u32, m: VowelMod) -> Vec<f32> {
    let k = knobs(id, settings(id)[set].1);
    run_hit(build_vox(id, k, vel, seed, m), 3.0)
}

/// A lane of the groove: pad, machine, setting, 64 steps, level.
type Lane = (&'static str, Id, usize, &'static str, f32);

const GROOVE: [Lane; 6] = [
    (
        "KICK",
        Id::VoxKick,
        0,
        "X.....x...X.....X.....x.X..x....X.....x...X.....X.....x.X.x.x...",
        0.8,
    ),
    (
        "SNARE",
        Id::VoxSnare,
        0,
        "....X.......X.......X.......X..x....X.......X.......X.......X.xx",
        0.7,
    ),
    (
        "CLOSED",
        Id::VoxHat,
        0,
        "x.X.x.X.x.X.x...x.X.x.X.x.X.x...x.X.x.X.x.X.x...x.X.x.X.x.X.x...",
        0.55,
    ),
    (
        "OPEN",
        Id::VoxHat,
        2,
        "..............x...............x...............x.................",
        0.45,
    ),
    (
        "CLICK",
        Id::VoxClick,
        0,
        "..x.......x.x.....x...x...x..x....x.......x.x.....x.....x.x..x.x",
        0.6,
    ),
    (
        "TONGUE",
        Id::VoxClick,
        1,
        ".........x...............x.........x...........x.........x..x...",
        0.5,
    ),
];

const GROOVE_BPM: f32 = 96.0;
/// The swing: every second sixteenth at 60 % of the pair.
const SWING: f32 = 0.6;
/// Accent's route to VOWEL (an accented hit says a brighter vowel) and
/// the random route's depth, VOWEL units.
const ACCENT_VOWEL: f32 = 18.0;
const RANDOM_VOWEL: f32 = 22.0;

fn swung(i: usize, bpm: f32) -> usize {
    let step = 15.0 / bpm;
    let t = (i / 2) as f32 * 2.0 * step + if i % 2 == 1 { 2.0 * SWING * step } else { 0.0 };
    (t * FS) as usize
}

/// Four bars of beatbox, VOX only: swung, accents at full velocity (and
/// a brighter vowel), every hit a random vowel nudge; the open hat
/// choked by the next closed one.
fn groove(out: &mut Out) -> Result<(), String> {
    let mut rng = Lcg(0x5eed_b0c5);
    let closed: Vec<usize> = GROOVE[2]
        .3
        .char_indices()
        .filter(|(_, c)| *c != '.')
        .map(|(i, _)| i)
        .collect();
    let mut x = Vec::new();
    let mut pads = Vec::new();
    for (lane, id, set, steps, gain) in GROOVE {
        for (i, c) in steps.chars().enumerate() {
            let (vel, acc) = match c {
                'X' => (1.0, ACCENT_VOWEL),
                'x' => (0.7, 0.0),
                _ => continue,
            };
            let m = VowelMod {
                offset: acc + RANDOM_VOWEL * rng.next(),
                lfo: None,
            };
            let mut h = hit_m(id, set, vel, 700 + i as u32, m);
            if lane == "OPEN"
                && let Some(&j) = closed.iter().find(|&&j| j > i)
            {
                let cut = swung(j, GROOVE_BPM) - swung(i, GROOVE_BPM);
                let fade = (0.005 * FS) as usize;
                for (n, v) in h.iter_mut().enumerate().skip(cut) {
                    *v *= 1.0 - ((n - cut) as f32 / fade as f32).min(1.0);
                }
            }
            add_at(&mut x, swung(i, GROOVE_BPM), &h, gain);
        }
        let k = settings(id)[set].1;
        pads.push(json!({"pad": lane, "machine": spec(id).name, "setting": settings(id)[set].0, "knobs": knob_map(id, k), "level": gain, "steps": steps}));
    }
    x.resize(x.len().max(swung(64, GROOVE_BPM)), 0.0);
    out.write(
        "vox-groove_beatbox.wav",
        &x,
        json!({
            "kind": "vox-demo", "machine": "VOX KIT", "family": "VOX", "setting": "beatbox groove",
            "knobs": {}, "pads": pads, "bpm": GROOVE_BPM, "swing": SWING,
            "vowel_routes": {"accent_to_vowel": ACCENT_VOWEL, "random_to_vowel": RANDOM_VOWEL},
            "listen_for": "four bars of beatbox from the VOX machines alone, swung 60 %: does it read as a mouth? Accents (X) louder and brighter-vowelled; every hit's vowel nudged at random, so repeats don't sound looped; the open \"tsss\" choked by the next closed hat",
        }),
    )
}

/// VOWEL as a mod destination: bar 1 dry, bar 2 a random value per hit
/// (latched at the hit), bars 3-4 an LFO running through the hits.
fn demo(out: &mut Out) -> Result<(), String> {
    const BPM: f32 = 120.0;
    const DEPTH: f32 = 45.0;
    const LFO_HZ: f32 = 0.5;
    let lanes: [(Id, usize, &str, f32); 2] = [
        (
            Id::VoxClick,
            0,
            "x.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xxx.xx",
            0.7,
        ),
        (
            Id::VoxKick,
            1,
            "X...X...X...X...X...X...X...X...X...X...X...X...X...X...X...X...",
            0.7,
        ),
    ];
    let mut rng = Lcg(0xb0ca1);
    let mut x = Vec::new();
    for (id, set, steps, gain) in lanes {
        for (i, c) in steps.chars().enumerate() {
            let vel = match c {
                'X' => 1.0,
                'x' => 0.75,
                _ => continue,
            };
            let t = i as f32 * 15.0 / BPM;
            let m = match i / 16 {
                0 => VowelMod::default(),
                1 => VowelMod {
                    offset: DEPTH * rng.next(),
                    lfo: None,
                },
                _ => VowelMod {
                    offset: 0.0,
                    lfo: Some((LFO_HZ, DEPTH, LFO_HZ * t)),
                },
            };
            let h = hit_m(id, set, vel, 300 + i as u32, m);
            add_at(&mut x, (t * FS) as usize, &h, gain);
        }
    }
    x.resize(x.len().max((4.0 * 4.0 * 60.0 / BPM * FS) as usize), 0.0);
    let pads: Vec<Value> = lanes
        .iter()
        .map(|(id, set, steps, gain)| json!({"machine": spec(*id).name, "setting": settings(*id)[*set].0, "knobs": knob_map(*id, settings(*id)[*set].1), "level": gain, "steps": steps}))
        .collect();
    out.write(
        "vox-demo_vowel-per-hit.wav",
        &x,
        json!({
            "kind": "vox-demo", "machine": "VOX CLICK + VOX KICK", "family": "VOX", "setting": "VOWEL modulated",
            "knobs": {}, "pads": pads, "bpm": BPM,
            "vowel_mod": {"bar 1": "none", "bar 2": format!("RANDOM per hit, latched, ±{DEPTH} VOWEL"), "bars 3-4": format!("LFO {LFO_HZ} Hz sine, ±{DEPTH} VOWEL, running inside each hit")},
            "listen_for": "the same two pads saying different things: bar 1 every hit alike; bar 2 each hit a random vowel; bars 3-4 the vowel sweeping slowly U..I across the hits (one LFO cycle over the two bars)",
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero crossings a second: where the energy sits, roughly.
    fn zcr(x: &[f32]) -> f32 {
        let n = x
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        n as f32 / (x.len() as f32 / FS)
    }

    #[test]
    fn vowel_moves_every_machine() {
        for id in [Id::VoxKick, Id::VoxSnare, Id::VoxHat, Id::VoxClick] {
            let at = |v: f32| {
                let mut k = knobs(id, settings(id)[0].1);
                (k[4], k[6]) = (v, v);
                // The pop, plosive, tick and tongue off: the bands alone.
                k[if id == Id::VoxKick { 3 } else { 2 }] = 0.0;
                zcr(&run_hit(build_vox(id, k, 1.0, 1, VowelMod::default()), 1.0))
            };
            let (u, i) = (at(0.0), at(1.0));
            assert!(i > 1.15 * u, "{id:?}: U {u} I {i}");
        }
    }

    #[test]
    fn a_mod_offset_moves_the_vowel() {
        let id = Id::VoxClick;
        let k = knobs(id, settings(id)[0].1);
        let h = |off: f32| {
            zcr(&run_hit(
                build_vox(
                    id,
                    k,
                    1.0,
                    1,
                    VowelMod {
                        offset: off,
                        lfo: None,
                    },
                ),
                1.0,
            ))
        };
        assert!(h(40.0) > h(-40.0) * 1.1);
    }
}
