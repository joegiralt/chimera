//! Whole scenes through the real `Instrument`, DAC pairs out (ADR 0109):
//! the loudness table, the limiter's tests and the A/B renders.
#![allow(dead_code)]

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::limiter::OUTPUT_GAIN;
use chimera_core::dsp::tape_engine::TapeModel;
use chimera_core::hw::{DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

use super::tape::{OneSource, held_plain};
use super::{SR, scope_writer};

/// One scene's output: every block's pairs, after the limiter, and the
/// pairs' peak before it (times the trim).
pub struct Run {
    /// Per pair, interleaved stereo.
    pub out: [Vec<f32>; DAC_PAIRS],
    /// Largest |sample| into the limiter, per block, times the trim.
    pub pre: Vec<f32>,
    /// Per block, the gain reduction the limiter took, dB over the rest
    /// gain (0 at rest; negative): the block out against the block in.
    pub gr: Vec<f32>,
    /// Per block, the energy of the block out and of the block it was
    /// (before the gain): `out[t]` is `in[t - 1]` scaled.
    pub e_out: Vec<f32>,
    pub e_in: Vec<f32>,
}

impl Run {
    /// Peak over every pair, from block `from`.
    pub fn peak(&self, from: usize) -> f32 {
        self.out
            .iter()
            .flat_map(|p| p[2 * BLOCK_SIZE * from..].iter())
            .fold(0.0, |m, x| f32::max(m, x.abs()))
    }

    /// RMS of the loudest pair (both sides), from block `from`.
    pub fn rms(&self, from: usize) -> f32 {
        self.out
            .iter()
            .map(|p| {
                let s = &p[2 * BLOCK_SIZE * from..];
                (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
            })
            .fold(0.0, f32::max)
    }

    /// Mono mix of pairs (their sum) for listening, interleaved stereo.
    pub fn mix(&self) -> Vec<f32> {
        let n = self.out[0].len();
        (0..n)
            .map(|i| self.out.iter().map(|p| p[i]).sum::<f32>())
            .collect()
    }
}

/// `notes` (part, note) at `vel`, held `hold` blocks then released, `total`
/// blocks in all. A TAPE Part reads `source` (held bytes).
pub fn run(
    perf: &Performance,
    notes: &[(usize, u8)],
    vel: u8,
    hold: usize,
    total: usize,
    source: Option<&[u8]>,
) -> Run {
    let mut shared = Box::new(AudioShared::from_performance(perf));
    let mut one = source.map(OneSource::new);
    if let Some(o) = &one {
        for p in shared.parts.iter_mut() {
            if p.params.engine() == EngineType::Tape {
                p.source = Some(o.id);
            }
        }
    }
    let mut inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(u32::MAX)));
    let mut fx = Box::new(FxBus::new());
    let mut dac = Box::new(DacBlocks::new());
    let mut scope = scope_writer();
    let ev = |part: usize, note: u8, kind| NoteEvent {
        channel: shared.parts[part].mix.channel,
        note: MidiNote::new(note).unwrap(),
        kind,
    };
    let mut r = Run {
        out: core::array::from_fn(|_| Vec::new()),
        pre: Vec::new(),
        gr: Vec::new(),
        e_out: Vec::new(),
        e_in: Vec::new(),
    };
    let mut prev_in = 0.0f32;
    for b in 0..total {
        let evs: Vec<NoteEvent> = if b == 0 {
            notes
                .iter()
                .map(|&(p, n)| ev(p, n, NoteKind::On(Velocity::new(vel).unwrap())))
                .collect()
        } else if b == hold {
            notes
                .iter()
                .map(|&(p, n)| ev(p, n, NoteKind::Off))
                .collect()
        } else {
            Vec::new()
        };
        match one.as_mut() {
            Some(o) => {
                let v = o.view();
                for e in evs {
                    inst.handle_with(e, &shared, &v);
                }
                inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &v);
            }
            None => {
                for e in evs {
                    inst.handle(e, &shared);
                }
                inst.render(&mut fx, &mut dac, &shared, &mut scope);
            }
        }
        let pre = dac
            .input()
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()));
        r.pre.push(pre * OUTPUT_GAIN);
        let energy =
            |x: &[[f32; 2 * BLOCK_SIZE]; DAC_PAIRS]| x.iter().flatten().map(|v| v * v).sum::<f32>();
        let e_out = energy(dac.out());
        r.gr.push(if prev_in > 1e-12 {
            db((e_out / prev_in).sqrt() / OUTPUT_GAIN)
        } else {
            0.0
        });
        r.e_out.push(e_out);
        r.e_in.push(prev_in);
        prev_in = energy(dac.input());
        for (k, p) in dac.out().iter().enumerate() {
            r.out[k].extend_from_slice(p);
        }
    }
    r
}

/// A TAPE factory-style source: factory Sound `slot` on C3, one second,
/// peak-normalised to 0.9 as the importer holds a source, at 48 kHz.
pub fn chimera_source(slot: usize) -> Vec<u8> {
    let mut perf = Performance::new();
    **perf.edit(PartId::ALL[0]).sound =
        chimera_core::factory::factory_sound(slot).expect("factory Sound");
    let r = run(&perf, &[(0, 48)], 100, 700, 750, None);
    let left: Vec<f32> = r.out[0][2 * BLOCK_SIZE * 2..]
        .chunks(2)
        .map(|f| f[0])
        .collect();
    let peak = left.iter().fold(1e-6f32, |m, x| m.max(x.abs()));
    let s: Vec<f32> = left.iter().map(|x| x * 0.9 / peak).collect();
    held_plain(&s, 48_000)
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

pub fn tape_model(perf: &mut Performance, part: usize, m: TapeModel) {
    perf.edit(PartId::ALL[part]).sound.params.tape.machine =
        chimera_core::dsp::tape_engine::TapeMachine::of(m);
}

pub fn init_perf(engine: EngineType) -> Performance {
    let mut perf = Performance::new();
    **perf.edit(PartId::ALL[0]).sound = Sound::init(engine);
    perf
}

pub fn ch(i: u8) -> MidiChannel {
    MidiChannel::clamped(i)
}
