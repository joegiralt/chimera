//! Chimera's devices as the A/B sees them: a name, its knobs and a render
//! through the real DSP. Add one here when a comparison needs it.

use chimera_core::MidiNote;
use chimera_core::dsp::Stereo;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::waves::{WAVE_IDENTS, WaveId, mip_position};
use chimera_core::taps::TapRows;
use chimera_core::dsp::filter::{Filter, FilterMode, FilterMods, level_from_drive};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{KnobMemory, ReverbParams, ReverbType};
use chimera_core::params::FilterParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::mem::MaybeUninit;

/// One knob: a number, or one of `choices` (given by name, stored as its
/// index).
pub struct Knob {
    pub name: &'static str,
    pub default: f32,
    pub choices: &'static [&'static str],
}

const fn num(name: &'static str, default: f32) -> Knob {
    Knob {
        name,
        default,
        choices: &[],
    }
}

/// A device's knob values, in its `knobs` order.
pub type Values = Vec<f32>;

pub enum Device {
    /// A reverb TYPE through the real `FxBus`.
    Reverb(ReverbType),
    /// The SVF, CLEAN: MODE, FREQ, RES (DRIVE 0).
    CleanFilter,
    /// One ALGO operator alone as a carrier.
    AlgoOp,
}

/// A reverb's knobs, in the order its values take.
pub const REVERB_KNOBS: [&str; 10] = [
    "size", "time", "tone", "diffuse", "predelay", "smear", "rate", "shape", "damping", "density",
];

/// MODE names, then 2-pole/4-pole aliases a reference may use.
const FILTER_MODES: [&str; 13] = [
    "LP6", "LP12", "LP24", "BP12", "BP24", "HP24", "NOTCH", "PHASER", "LP1", "LP2", "LP4", "BP2",
    "BP4",
];

const FILTER_KNOBS: [Knob; 4] = [
    Knob {
        name: "mode",
        default: 1.0,
        choices: &FILTER_MODES,
    },
    num("freq", 1000.0),
    num("res", 0.0),
    num("drive", 0.0),
];

const OSC_KNOBS: [Knob; 3] = [
    Knob {
        name: "wave",
        default: 0.0,
        choices: &WAVE_IDENTS,
    },
    num("freq", 440.0),
    num("level", 0.5),
];

impl Device {
    /// Every device the tool knows, by name.
    pub fn all() -> Vec<(String, Device)> {
        let mut v: Vec<(String, Device)> = ReverbType::ALL
            .iter()
            .zip(ReverbType::NAMES)
            .map(|(&t, n)| (n.to_ascii_lowercase(), Device::Reverb(t)))
            .collect();
        v.push(("clean-filter".into(), Device::CleanFilter));
        v.push(("algo-op".into(), Device::AlgoOp));
        v
    }

    pub fn find(name: &str) -> Result<Device, String> {
        Self::all()
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, d)| d)
            .ok_or_else(|| {
                let names: Vec<String> = Self::all().into_iter().map(|(n, _)| n).collect();
                format!("no device `{name}` (have: {})", names.join(", "))
            })
    }

    pub fn knobs(&self) -> Vec<Knob> {
        match self {
            Device::Reverb(t) => {
                let start = start_params(*t);
                let v = reverb_values(&start);
                REVERB_KNOBS
                    .iter()
                    .zip(v)
                    .map(|(&n, d)| num(n, d))
                    .collect()
            }
            Device::CleanFilter => FILTER_KNOBS.into_iter().collect(),
            Device::AlgoOp => OSC_KNOBS.into_iter().collect(),
        }
    }

    /// Whether it plays from nothing (an oscillator) rather than process.
    pub fn is_source(&self) -> bool {
        matches!(self, Device::AlgoOp)
    }

    /// The defaults with `key=value` pairs applied; an unknown key is an
    /// error.
    pub fn values(&self, pairs: &[(String, String)]) -> Result<Values, String> {
        let knobs = self.knobs();
        let mut v: Values = knobs.iter().map(|k| k.default).collect();
        for (key, val) in pairs {
            let i = knobs
                .iter()
                .position(|k| k.name.eq_ignore_ascii_case(key))
                .ok_or_else(|| format!("no knob `{key}`"))?;
            let choices = knobs[i].choices;
            v[i] = match choices.iter().position(|c| c.eq_ignore_ascii_case(val)) {
                Some(c) => c as f32,
                None if !choices.is_empty() => {
                    return Err(format!("{key}={val}: one of {}", choices.join(", ")));
                }
                None => val
                    .parse()
                    .map_err(|_| format!("{key}={val}: not a number"))?,
            };
        }
        if let Device::CleanFilter = self
            && !FilterParams::default().set_mode(filter_mode(v[0] as usize))
        {
            return Err(format!(
                "mode {}: not an SVF mode",
                FILTER_MODES[v[0] as usize]
            ));
        }
        Ok(v)
    }

    /// Renders `len` samples at `crate::SR`: of `input` for an effect, from silence
    /// for a source.
    pub fn render(&self, v: &[f32], input: &[f32], len: usize) -> Vec<f32> {
        match self {
            Device::Reverb(t) => render_reverb(*t, v, input),
            Device::CleanFilter => render_filter(v, input),
            Device::AlgoOp => render_op(v, len),
        }
    }
}

fn start_params(t: ReverbType) -> ReverbParams {
    FxParams::default().reverb.with_type(t, &KnobMemory::new())
}

fn reverb_values(r: &ReverbParams) -> [f32; 10] {
    [
        r.size, r.time, r.tone, r.diffuse, r.predelay, r.smear, r.rate, r.shape, r.damping,
        r.density,
    ]
}

/// The reverb's return alone (MIX 1), mono (L+R)/2, after 0.5 s of silence
/// for the eases to land.
pub fn render_reverb(t: ReverbType, v: &[f32], x: &[f32]) -> Vec<f32> {
    let mut fx = FxParams {
        reverb: start_params(t),
        ..FxParams::default()
    };
    fx.reverb.mix = 1.0;
    let r = &mut fx.reverb;
    (r.size, r.time, r.tone, r.diffuse, r.predelay) = (v[0], v[1], v[2], v[3], v[4]);
    (r.smear, r.rate, r.shape, r.damping) = (v[5], v[6], v[7], v[8]);
    r.density = v.get(9).copied().unwrap_or(r.density);
    let mut slot = Box::new(MaybeUninit::<FxBus>::uninit());
    let bus = FxBus::init_in_place(&mut slot);
    let lead = (SAMPLE_RATE as usize / 2) / BLOCK_SIZE * BLOCK_SIZE;
    let n = x.len() + lead;
    let mut y = Vec::with_capacity(n + BLOCK_SIZE);
    let mut i = 0;
    while i < n {
        let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
        let [_, _, rev] = &mut sends;
        for (k, s) in rev.iter_mut().enumerate() {
            let j = i + k;
            if j >= lead && j - lead < x.len() {
                *s = x[j - lead];
            }
        }
        let mut ret = Stereo::SILENT;
        bus.process(&mut sends, &fx, Tempo::DEFAULT, SAMPLE_RATE, &mut ret);
        y.extend((0..BLOCK_SIZE).map(|k| 0.5 * (ret.l[k] + ret.r[k])));
        i += BLOCK_SIZE;
    }
    y[lead..lead + x.len()].to_vec()
}

fn filter_mode(i: usize) -> FilterMode {
    match FILTER_MODES[i] {
        "LP1" => FilterMode::Lp6,
        "LP2" => FilterMode::Lp12,
        "LP4" => FilterMode::Lp24,
        "BP2" => FilterMode::Bp12,
        "BP4" => FilterMode::Bp24,
        _ => FilterMode::ALL[i],
    }
}

fn render_filter(v: &[f32], x: &[f32]) -> Vec<f32> {
    let mut p = FilterParams::default();
    (p.cutoff, p.resonance, p.level) = (v[1], v[2], level_from_drive(v[3]));
    p.set_mode(filter_mode(v[0] as usize));
    let mut f = Filter::new();
    let mut y = x.to_vec();
    y.resize(x.len().next_multiple_of(BLOCK_SIZE), 0.0);
    for blk in y.chunks_exact_mut(BLOCK_SIZE) {
        if let Ok(blk) = <&mut [f32; BLOCK_SIZE]>::try_from(blk) {
            f.process(blk, &p, SAMPLE_RATE, &FilterMods::NONE);
        }
    }
    y.truncate(x.len());
    y
}

fn render_op(v: &[f32], len: usize) -> Vec<f32> {
    const NONE: [u8; OPS] = [0; OPS];
    const HOLD: EnvRates = EnvRates {
        ar: 31,
        d1r: 0,
        d1l: 15,
        d2r: 0,
        rr: 8,
        rs: 0,
    };
    let wave = WaveId::clamped(v[0] as u8);
    let (hz, level) = (v[1], v[2]);
    let pos = mip_position(hz);
    let lo = pos as usize;
    let silent = OpBlock {
        inc: 0,
        gain_from: 0.0,
        gain_to: 0.0,
        feedback: 0.0,
        lo: wave.table(0),
        hi: wave.table(0),
        xfade_from: 0.0,
        xfade_to: 0.0,
    };
    let mut ops = [silent; OPS];
    ops[0] = OpBlock {
        inc: (hz as f64 / crate::SR * 4_294_967_296.0) as u32,
        gain_from: level * SAMPLE_SCALE,
        gain_to: level * SAMPLE_SCALE,
        lo: wave.table(lo),
        hi: wave.table(lo + 1),
        xfade_from: pos - lo as f32,
        xfade_to: pos - lo as f32,
        ..silent
    };
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let blk = KernelBlock {
        plan: &plan,
        ops,
        morph_from: 0.0,
        morph_to: 0.0,
        norm_from: 1.0,
        norm_to: 1.0,
    };
    let mut env: [OpEnv; OPS] = core::array::from_fn(|_| {
        let mut e = OpEnv::IDLE;
        e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, crate::SR as f32));
        e
    });
    let mut k = Kernel::new();
    let mut out = Vec::with_capacity(len + BLOCK_SIZE);
    let mut b = [0.0; BLOCK_SIZE];
    while out.len() < len {
        k.render(&blk, &mut env, &mut b, &mut TapRows::new([false; 2]));
        out.extend_from_slice(&b);
    }
    out.truncate(len);
    out
}

/// The reverb TYPE named `name` (any case).
pub fn reverb_type(name: &str) -> Result<ReverbType, String> {
    ReverbType::NAMES
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| ReverbType::ALL[i])
        .ok_or_else(|| format!("no reverb TYPE `{name}`"))
}
