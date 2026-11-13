//! The roll (#809, ADR 0232): a Sound rewritten inside musically safe
//! ranges, per engine, from a seed. Bank 2's (`bank2.x`), reached through
//! the UI's door.
//!
//! Musical by construction, as stock PreenFM3's randomizer is (its facts,
//! read from its SynthState.cpp; no code): ratios from small harmonic sets,
//! carriers loud and harmonic so the note keeps its pitch, envelopes by
//! family, mod depth bounded per amount, modulation from a few sensible
//! sources. Each dimension draws from its own stream, so turning one never
//! moves another's values. OUT LEVEL, PITCH, the sends and LFO 1 (the
//! wheel's vibrato) are never touched.

use super::{Amount, Randomize};
use crate::block::DiskCode;
use crate::dsp::algo::algorithms::{ALGO_COUNT, AlgoId};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::waves::WaveId;
use crate::dsp::modal::{ModalParams, ResonatorMode};
use crate::dsp::va::{VaMode, VaParams};
use crate::modulation::{FixedCol, ModSource, ModState};
use crate::params::{EngineType, EnvParams, FilterParams};
use crate::preset::Sound;

/// xorshift32, seeded per dimension through a 32-bit finaliser.
struct Rng(u32);

impl Rng {
    #[optimize(size)]
    fn new(seed: u8, dim: u32) -> Self {
        let mut z = u32::from(seed).wrapping_mul(0x9E37_79B9) ^ dim.wrapping_mul(0x85EB_CA6B);
        z ^= z >> 16;
        z = z.wrapping_mul(0x7FEB_352D);
        z ^= z >> 15;
        z = z.wrapping_mul(0x846C_A68B);
        z ^= z >> 16;
        Rng(z | 1)
    }

    #[optimize(size)]
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0..1.
    #[optimize(size)]
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    #[optimize(size)]
    fn range(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    /// `lo..=hi`.
    #[optimize(size)]
    fn int(&mut self, (lo, hi): (i32, i32)) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u32) as i32
    }

    #[optimize(size)]
    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.next() as usize % xs.len()]
    }

    #[optimize(size)]
    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// `lo..hi`, even in octaves.
    #[optimize(size)]
    fn log(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo * libm::powf(hi / lo, self.unit())
    }
}

/// Amount `a`'s row of `t` (1..=3); `None` at OFF.
#[optimize(size)]
fn row<T: Copy>(a: Amount, t: [T; 3]) -> Option<T> {
    a.get().checked_sub(1).map(|i| t[usize::from(i)])
}

/// `r` rolled into `s`: OSC, then ENV, TONE and MOD, each over what the
/// one before left.
#[inline(never)]
#[optimize(size)]
pub fn roll(s: &mut Sound, r: &Randomize) {
    let e = s.engine();
    let p = &mut s.params;
    let g = |dim| Rng::new(r.seed.get(), dim);
    if r.osc != Amount::OFF {
        let n = r.osc.get();
        match e {
            EngineType::Algo => algo_osc(&mut p.algo, n, &mut g(1)),
            EngineType::Va => va_osc(&mut p.va, n, &mut g(1)),
            EngineType::Modal => modal_osc(&mut p.modal, n, &mut g(1)),
            // A sample's own pitch and place: nothing safe to roll. DRUM's
            // knobs are its pads', not the Sound's: none to roll yet.
            EngineType::Tape | EngineType::Drum => {}
        }
    }
    if let Some(f) = row(r.env, [Family::Pluck, Family::Pad, Family::Any]) {
        let g = &mut g(2);
        match e {
            EngineType::Algo => algo_env(&mut p.algo, f, g),
            EngineType::Modal => {
                p.modal.damp = g.range(f.pick((0.25, 0.5), (0.6, 0.9), (0.25, 0.9)))
            }
            // ENV 1 into VCA is their amp (`Sound::plain_init`).
            EngineType::Va | EngineType::Tape => amp_env(&mut p.envelopes[0], f, g),
            // Its machines carry their own decays: no amp to roll yet.
            EngineType::Drum => {}
        }
    }
    if r.tone != Amount::OFF {
        let (n, g) = (r.tone.get(), &mut g(3));
        filter(&mut p.filter, n, e == EngineType::Modal, g);
        match e {
            EngineType::Algo => algo_tone(&mut p.algo, n, g),
            EngineType::Modal => modal_tone(&mut p.modal, n, g),
            EngineType::Va | EngineType::Tape | EngineType::Drum => {}
        }
    }
    if r.mods != Amount::OFF {
        let (n, g) = (r.mods.get(), &mut g(4));
        mods(&mut s.params, &mut s.mod_state, n, g);
    }
}

/// Stock's envelope families: ENV's 1, 2 and 3.
#[derive(Clone, Copy)]
enum Family {
    Pluck,
    Pad,
    Any,
}

impl Family {
    #[optimize(size)]
    fn pick<T>(self, pluck: T, pad: T, any: T) -> T {
        match self {
            Family::Pluck => pluck,
            Family::Pad => pad,
            Family::Any => any,
        }
    }
}

// ── ALGO ───────────────────────────────────────────────────────────────

/// Coarse indices (`tx::COARSE_NAMES`): 0.50 1.00 2.00 3.00 4.00 5.00.
const R05: u8 = 0;
const R1: u8 = 4;
const R2: u8 = 8;
const R3: u8 = 10;
const R4: u8 = 13;
const R5: u8 = 16;
/// The highest ratio WILD reaches: 8.00.
const R8: u8 = 25;

/// Which operators sound (carriers) and which modulate, on ALG A (and B
/// while MORPH blends it in).
#[optimize(size)]
fn roles(a: &AlgoParams) -> (u8, u8) {
    let (x, y) = a.algs();
    let sides: &[AlgoId] = if a.morph > 0 { &[x, y] } else { &[x] };
    sides.iter().fold((0, 0), |(c, m), s| {
        let alg = s.algorithm();
        let mods = (0..OPS)
            .filter(|&i| alg.mods[i] != 0)
            .fold(0, |m, i| m | 1 << i);
        (c | alg.carriers, m | mods)
    })
}

/// Stock's OPER: the algorithm, ratios and waves, carriers' levels.
#[optimize(size)]
fn algo_osc(a: &mut AlgoParams, n: u8, g: &mut Rng) {
    // Plain tables first, so any algorithm fits the formant budget.
    for op in 0..OPS {
        let _ = a.try_set_kind(op, OpKind::Table(WaveId::W1));
    }
    // PURE: the TX81Z's eight; RICH adds the first A rows; WILD any.
    let top = [8, 22, ALGO_COUNT as i32][usize::from(n - 1)];
    let alg = AlgoId::clamped(g.int((0, top - 1)) as u8);
    let other = AlgoId::clamped(g.int((0, top - 1)) as u8);
    let morph = n == 3 && g.chance(0.5);
    let _ = a.try_set_algs(alg, if morph { other } else { alg });
    a.morph = if morph { g.int((20, 107)) as u8 } else { 0 };
    let (carriers, modulators) = roles(a);
    let count = carriers.count_ones() as i32;
    // Carriers in octaves: the note keeps its pitch class.
    let carrier_ratios: &[u8] =
        [&[R1][..], &[R05, R1, R1, R2], &[R05, R1, R2, R2]][usize::from(n - 1)];
    let mod_ratios: &[u8] =
        [&[R05, R1, R2, R4][..], &[R05, R1, R2, R3, R4, R5], &[]][usize::from(n - 1)];
    for i in 0..OPS {
        let carrier = carriers & 1 << i != 0;
        // The TX81Z's waves: carriers keep a strong fundamental (W1-W4).
        let w = match (n, carrier) {
            (3, true) => g.int((0, 3)),
            (3, false) => g.int((0, 7)),
            (2, false) if g.chance(0.3) => g.int((1, 3)),
            _ => 0,
        };
        let _ = a.try_set_kind(
            i,
            OpKind::from_code(w as u8).unwrap_or(OpKind::Table(WaveId::W1)),
        );
        let op = &mut a.ops[i];
        op.fine = 0;
        op.detune = 0;
        if carrier {
            op.coarse = g.pick(carrier_ratios);
            // Each carrier past the first 1.5 dB down: N of them sum about
            // where one does.
            op.level = (99 - 2 * (count - 1) - g.int((0, 6))).max(80) as u8;
            if n > 1 {
                op.detune = g.int((-1, 1)) as i8;
            }
        } else {
            op.coarse = if mod_ratios.is_empty() {
                g.int((0, i32::from(R8))) as u8
            } else {
                g.pick(mod_ratios)
            };
            if n == 3 && g.chance(0.15) {
                op.fine = g.int((1, 3)) as u8;
            }
            if modulators & 1 << i == 0 {
                op.level = 0;
            }
        }
    }
    // A new modulator never keeps an old carrier's level: SOFT's depth,
    // which TONE replaces when it rolls.
    algo_tone(a, 1, g);
}

/// Every operator on family `f`; modulators a little quicker, so the tone
/// closes as the note fades, as a real instrument's does.
#[optimize(size)]
fn algo_env(a: &mut AlgoParams, f: Family, g: &mut Rng) {
    let (carriers, _) = roles(a);
    let ar = f.pick((26, 31), (8, 14), (8, 31));
    let d1r = f.pick((10, 15), (3, 8), (0, 20));
    let d1l = f.pick((0, 6), (11, 15), (0, 15));
    let d2r = f.pick((6, 12), (0, 2), (0, 12));
    let rr = f.pick((6, 9), (3, 6), (3, 10));
    let base = (g.int(ar), g.int(d1r), g.int(d1l), g.int(d2r), g.int(rr));
    let rs = g.int((0, 2)) as u8;
    for (i, op) in a.ops.iter_mut().enumerate() {
        // Drawn for every operator: OSC's roles never shift ENV's stream.
        let quick = g.int((0, 4)) * i32::from(carriers & 1 << i == 0);
        op.ar = base.0 as u8;
        op.d1r = (base.1 + quick).min(31) as u8;
        op.d1l = base.2 as u8;
        op.d2r = (base.3 + quick).min(31) as u8;
        op.rr = base.4 as u8;
        op.rate_scale = rs;
    }
}

/// Stock's IM: the modulators' depth, their feedback and velocity.
#[optimize(size)]
fn algo_tone(a: &mut AlgoParams, n: u8, g: &mut Rng) {
    let (carriers, modulators) = roles(a);
    // Stock bounds the index per amount; here per modulator too, so five
    // into one carrier ring no harder than two (2.25 dB each past the first).
    let crowd = 3 * ((modulators & !carriers).count_ones() as i32 - 1).max(0);
    let (lo, hi) = [(45, 70), (55, 82), (58, 92)][usize::from(n - 1)];
    let level = (lo - crowd, hi - crowd);
    let fb = [(0, 0), (0, 2), (0, 4)][usize::from(n - 1)];
    for (i, op) in a.ops.iter_mut().enumerate() {
        // Drawn for every operator, so OSC's roles never shift the stream.
        let (l, v, f) = (g.int(level), g.int((0, (n + 1) as i32)), g.int(fb));
        // A carrier's feedback is noise in the note itself: modulators only.
        if modulators & 1 << i != 0 && carriers & 1 << i == 0 {
            op.level = l as u8;
            op.velocity = v as u8;
            op.feedback = f as u8;
        } else {
            op.feedback = 0;
            op.velocity = 0;
        }
    }
}

// ── VA ─────────────────────────────────────────────────────────────────

/// OSC 2's intervals in semitones: octaves, then fifths, then fourths.
const INTERVALS: [i8; 8] = [0, 12, -12, 7, 19, 24, 5, -5];

#[optimize(size)]
fn va_osc(v: &mut VaParams, n: u8, g: &mut Rng) {
    let i = usize::from(n - 1);
    v.mode = match n {
        1 => VaMode::Mix,
        2 => g.pick(&[VaMode::Mix, VaMode::Mix, VaMode::Sync, VaMode::SoftSync]),
        _ => VaMode::from_disk_code(g.int((0, 10)) as u8).unwrap_or_default(),
    };
    for w in &mut v.wave {
        *w = if n == 1 {
            g.pick(&[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0])
        } else {
            g.range([(0.0, 1.0), (0.2, 1.0), (0.0, 1.0)][i])
        };
    }
    v.coarse[0] = if n == 3 { g.pick(&[0, 0, -12, 12]) } else { 0 };
    v.coarse[1] = g.pick(&INTERVALS[..[3, 5, 8][i]]);
    v.fine[0] = 0.0;
    v.fine[1] = g.range([(3.0, 8.0), (0.0, 12.0), (-25.0, 25.0)][i]);
    v.amount = g.range([(0.35, 0.65), (0.2, 0.8), (0.0, 1.0)][i]);
    v.sub = g.range([(0.0, 0.0), (0.0, 0.4), (0.0, 0.7)][i]);
    for s in &mut v.shape {
        *s = g.range([(0.0, 0.0), (0.0, 0.4), (0.0, 0.8)][i]);
    }
    v.noise = g.range([(0.0, 0.0), (0.0, 0.08), (0.0, 0.25)][i]);
}

// ── MODAL ──────────────────────────────────────────────────────────────

/// The model and where it is struck. The exciter always strikes.
#[optimize(size)]
fn modal_osc(m: &mut ModalParams, n: u8, g: &mut Rng) {
    let i = usize::from(n - 1);
    if n > 1 {
        m.mode = ResonatorMode::from_disk_code(g.int((0, 3)) as u8).unwrap_or(m.mode);
    }
    m.structure = g.range([(0.0, 0.3), (0.0, 0.5), (0.0, 0.6)][i]);
    // Struck near the end a string loses its fundamental: never past the middle.
    m.pos = g.range([(0.0, 0.3), (0.0, 0.4), (0.0, 0.5)][i]);
    m.excite = g.range((0.6, 1.0));
}

#[optimize(size)]
fn modal_tone(m: &mut ModalParams, n: u8, g: &mut Rng) {
    let i = usize::from(n - 1);
    m.bright = g.range([(0.2, 0.5), (0.2, 0.75), (0.05, 0.95)][i]);
    m.body = g.range([(0.0, 0.3), (0.0, 0.5), (0.0, 0.8)][i]);
    m.color = g.range((0.4, 1.0));
    m.burst = g.range((0.3, 1.0));
}

// ── Every engine ───────────────────────────────────────────────────────

/// ENV 1 as the amp: a pluck decays, a pad swells and holds.
#[optimize(size)]
fn amp_env(e: &mut EnvParams, f: Family, g: &mut Rng) {
    e.attack = g.range(f.pick((0.0, 0.12), (0.45, 0.7), (0.0, 0.7)));
    e.decay = g.range(f.pick((0.4, 0.62), (0.55, 0.75), (0.35, 0.8)));
    e.sustain = g.range(f.pick((0.0, 0.3), (0.6, 1.0), (0.2, 1.0)));
    e.release = g.range(f.pick((0.35, 0.6), (0.6, 0.8), (0.35, 0.8)));
    e.hold = 0.0;
}

/// CUTOFF and RESO: the TYPE and MODE stay the Sound's.
/// A struck resonator's tone is its BRIGHT: a dark CUTOFF on MODAL only
/// takes level, so its WILD opens no lower than BRIGHT's.
#[optimize(size)]
fn filter(f: &mut FilterParams, n: u8, modal: bool, g: &mut Rng) {
    let i = usize::from(n - 1);
    let c = if modal { i.min(1) } else { i };
    f.cutoff = g.log([(3_000.0, 14_000.0), (1_500.0, 16_000.0), (900.0, 18_000.0)][c]);
    f.resonance = g.range([(0.0, 0.2), (0.0, 0.4), (0.0, 0.65)][i]);
}

/// Stock's MODL: LFO 2 and 3 and ENV 2 into the fixed FREQ and Q columns.
/// MODAL's ensemble too.
#[optimize(size)]
fn mods(p: &mut crate::params::ParamSnapshot, m: &mut ModState, n: u8, g: &mut Rng) {
    let i = usize::from(n - 1);
    for lfo in &mut p.lfos[1..] {
        lfo.rate = g.log([(0.1, 2.0), (0.1, 6.0), (0.05, 12.0)][i]);
        lfo.shape = if n == 1 {
            g.int((0, 1)) as u8
        } else {
            g.int((0, 4)) as u8
        };
        lfo.depth = 1.0;
    }
    amp_env(&mut p.envelopes[1], Family::Any, g);
    let mut route = |s: ModSource, c: FixedCol, a: i32| match a {
        0 => m.clear_fixed(s.index(), c),
        a => m.set_fixed(s.index(), c, a as i8),
    };
    route(
        ModSource::Env2,
        FixedCol::Freq,
        // ENV 2 only ever opens the filter: closing a dark CUTOFF is silence.
        g.int([(10, 35), (0, 50), (0, 70)][i]),
    );
    route(
        ModSource::Lfo2,
        FixedCol::Freq,
        g.int([(0, 8), (0, 20), (-30, 30)][i]),
    );
    route(
        ModSource::Lfo3,
        FixedCol::Q,
        g.int([(0, 0), (0, 15), (0, 30)][i]),
    );
    if p.engine() == EngineType::Modal {
        p.modal.ens_depth = g.range((0.0, 0.25 * f32::from(n)));
        p.modal.ens_rate = g.range((0.1, 0.6));
        // The ensemble's detuned copies cancel the strike: a touch of it only.
        p.modal.ens_mix = g.range((0.0, 0.1 * f32::from(n)));
    }
}
