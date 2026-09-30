//! STEAL = GLIDE (#254, ADR 0065): a note that steals a sounding voice of
//! its own Part and model glides the ring to its pitch, as a Prophet's
//! glide, and strikes it anew on the way; CUT strikes at the new pitch.

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::{ResonatorMode, out_gain};
use chimera_core::dsp::note_to_freq;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot, Steal};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{SR, fundamental_hz};

fn ev(ch: u8, note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind,
    }
}

fn on(ch: u8, note: u8) -> NoteEvent {
    ev(ch, note, NoteKind::On(Velocity::DEFAULT))
}

fn off(ch: u8, note: u8) -> NoteEvent {
    ev(ch, note, NoteKind::Off)
}

/// A budget with room for `voices` voices of `cost` beside the whole FX bus.
fn budget(cost: u32) -> SampleBudget {
    SampleBudget::for_cpu(((FxBus::COST.0 + cost) as u64 * 480_000).div_ceil(7) as u32)
}

struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: chimera_core::scope::ScopeWriter,
}

impl Rig {
    /// Room for one voice of Part 1's Sound: every other note steals.
    fn one_voice(shared: &AudioShared) -> Self {
        let p = &shared.parts[0];
        let cost = Voice::cost(&p.params, &p.mod_state).0;
        Self {
            inst: Box::new(Instrument::new(SR, budget(cost))),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: common::scope_writer(),
        }
    }

    /// `blocks` blocks of Part `part`'s bus, and each block's glide ratio of
    /// the voice sounding it.
    fn run(
        &mut self,
        shared: &AudioShared,
        blocks: usize,
        out: &mut Vec<f32>,
        ratios: &mut Vec<f32>,
    ) {
        for _ in 0..blocks {
            self.inst
                .render(&mut self.fx, &mut self.dac, shared, &mut self.scope);
            out.extend_from_slice(self.inst.part_bus(0));
            let active = self.inst.active();
            let v = (0..MAX_VOICES).find(|&v| active[v]).unwrap_or(0);
            ratios.push(self.inst.slides()[v]);
        }
    }
}

fn part(engine: EngineType, mode: Option<ResonatorMode>, steal: Steal) -> AudioShared {
    let mut shared = AudioShared::default();
    let mut p = ParamSnapshot::for_engine(engine);
    if let Some(m) = mode {
        p.modal.mode = m;
    }
    p.pitch.steal = steal;
    shared.parts[0].params = p;
    shared
}

const MODELS: [(&str, EngineType, Option<ResonatorMode>); 5] = [
    ("STRING", EngineType::Modal, Some(ResonatorMode::String)),
    ("BANK", EngineType::Modal, Some(ResonatorMode::Modal)),
    ("BOWED", EngineType::Modal, Some(ResonatorMode::Bowed)),
    ("SYMP", EngineType::Modal, Some(ResonatorMode::Sympathetic)),
    ("ALGO", EngineType::Algo, None),
];

const C3: u8 = 48;
const G3: u8 = 55;
const HELD: usize = 375;

/// C3 played and released, then G3, which steals its voice: the bus from
/// a block before G3's note-on and each block's glide ratio.
fn c3_then_g3(shared: &AudioShared, after: usize) -> (Vec<f32>, Vec<f32>) {
    steal(shared, (C3, G3), after)
}

/// `from` played and released, then `to`, which steals its voice.
fn steal(shared: &AudioShared, (from, to): (u8, u8), after: usize) -> (Vec<f32>, Vec<f32>) {
    let mut rig = Rig::one_voice(shared);
    let (mut out, mut ratios) = (Vec::new(), Vec::new());
    rig.inst.handle(on(0, from), shared);
    rig.run(shared, HELD, &mut out, &mut ratios);
    rig.inst.handle(off(0, from), shared);
    rig.run(shared, 4, &mut out, &mut ratios);
    let sounding = rig.inst.active().iter().filter(|&&a| a).count();
    assert_eq!(sounding, 1, "C3 rings on");
    rig.inst.handle(on(0, to), shared);
    let (mut g3, mut r) = (Vec::new(), Vec::new());
    rig.run(shared, after, &mut g3, &mut r);
    let sounding = rig.inst.active().iter().filter(|&&a| a).count();
    assert_eq!(sounding, 1, "G3 took C3's voice");
    // The block before the steal leads, for the click check.
    let mut lead = out[out.len() - BLOCK_SIZE..].to_vec();
    lead.extend_from_slice(&g3);
    (lead, r)
}

fn cents(f: f64, f0: f64) -> f64 {
    1200.0 * (f / f0).log2()
}

/// A glide steal from C3 to G3, and back down, at INIT's GLIDE TIME: the
/// ring starts at the stolen note's pitch and moves one way only, lands
/// within ±2 cents (±5 on BOWED, as its tuning is gated) after three GLIDE
/// TIMEs, and nothing clicks.
#[test]
fn a_glide_steal_lands_on_the_new_note() {
    for (from, to) in [(C3, G3), (G3, C3)] {
        let f = note_to_freq(to) as f64;
        for (name, engine, mode) in MODELS {
            let shared = part(engine, mode, Steal::Glide);
            let t = shared.parts[0].params.pitch.glide_secs();
            let three = (3.0 * t * SR as f32 / BLOCK_SIZE as f32).ceil() as usize;
            let window = (0.25 * SR as f32 / BLOCK_SIZE as f32) as usize;
            let (out, ratios) = steal(&shared, (from, to), three + window);

            let start = 2f32.powf((from as f32 - to as f32) / 12.0);
            assert!(
                (ratios[0] / start - 1.0).abs() < 0.03,
                "{name} {from}→{to}: starts at {} of the note",
                ratios[0]
            );
            let one_way = |w: &[f32]| {
                if from < to {
                    w[1] >= w[0]
                } else {
                    w[1] <= w[0]
                }
            };
            assert!(
                ratios.windows(2).all(one_way),
                "{name} {from}→{to}: not monotonic {ratios:?}"
            );
            assert_eq!(
                ratios[three], 1.0,
                "{name} {from}→{to}: landed after 3 × TIME"
            );

            let heard = &out[BLOCK_SIZE + three * BLOCK_SIZE..];
            let c = cents(fundamental_hz(heard, f), f);
            let within = if mode == Some(ResonatorMode::Bowed) {
                5.0
            } else {
                2.0
            };
            assert!(c.abs() <= within, "{name} {from}→{to}: {c:+.2} cents");

            let g = mode.map_or(1.0, out_gain);
            let w: Vec<f32> = out.iter().map(|x| x / g).collect();
            let clicks = common::clicks(&w);
            assert!(
                clicks.is_empty(),
                "{name} {from}→{to}: {:?}",
                &clicks[..clicks.len().min(10)]
            );
        }
    }
}

/// The lag, samples, over `lags` at which `s` from `at` nearly repeats:
/// its period there.
fn local_period(s: &[f32], at: usize, lags: core::ops::RangeInclusive<usize>) -> usize {
    let n = *lags.end();
    let d = |l: usize| -> f64 {
        (0..n)
            .map(|i| (s[at + i] as f64 - s[at + i + l] as f64).powi(2))
            .sum()
    };
    lags.min_by(|&a, &b| d(a).total_cmp(&d(b))).unwrap()
}

/// A GLIDE TIME's third in, 63 % of the way, the ring sounds between the
/// notes, where the glide has it: the tail glides, it does not jump.
#[test]
fn a_glide_is_heard_between_the_notes() {
    let (c3, g3) = (SR as f32 / note_to_freq(C3), SR as f32 / note_to_freq(G3));
    let shared = part(EngineType::Modal, Some(ResonatorMode::String), Steal::Glide);
    let tau = shared.parts[0].params.pitch.glide_secs() / 3.0;
    let k = (tau * SR as f32 / BLOCK_SIZE as f32).round() as usize;
    let (out, ratios) = c3_then_g3(&shared, k + 20);
    let want = g3 / ratios[k];
    let share = (c3 / want).log2() / (c3 / g3).log2();
    assert!((share - 0.63).abs() < 0.05, "{share}");
    // Centred on block `k` (the bus leads by a block).
    let heard = local_period(&out, (k + 1) * BLOCK_SIZE - 420, 200..=420) as f32;
    assert!(
        (heard - want).abs() <= 3.0,
        "{heard} samples heard, {want} sounding"
    );
}

/// CUT: the steal strikes G3 at its own pitch, at every GLIDE TIME, and
/// never glides.
#[test]
fn a_cut_steal_does_not_glide() {
    for (name, engine, mode) in MODELS {
        let mut ratios_by_time = Vec::new();
        for time in [0.0, 0.66, 1.0] {
            let mut shared = part(engine, mode, Steal::Cut);
            shared.parts[0].params.pitch.glide_time = time;
            let (out, ratios) = c3_then_g3(&shared, 40);
            assert!(ratios.iter().all(|&r| r == 1.0), "{name}");
            ratios_by_time.push(common::fnv1a(&out));
        }
        assert!(
            ratios_by_time.windows(2).all(|w| w[0] == w[1]),
            "{name}: TIME moves a CUT steal"
        );
    }
}

/// A note of another Part that steals the voice fades it out and starts
/// clean, GLIDE or not: only a Part's own steal glides.
#[test]
fn another_parts_steal_cuts() {
    let mut shared = part(EngineType::Modal, Some(ResonatorMode::String), Steal::Glide);
    shared.parts[1].params = shared.parts[0].params.clone();
    shared.parts[1].mix.channel = MidiChannel::new(1).unwrap();
    let mut rig = Rig::one_voice(&shared);
    let (mut out, mut ratios) = (Vec::new(), Vec::new());
    rig.inst.handle(on(0, C3), &shared);
    rig.run(&shared, 40, &mut out, &mut ratios);
    rig.inst.handle(off(0, C3), &shared);
    rig.inst.handle(on(1, G3), &shared);
    ratios.clear();
    rig.run(&shared, 20, &mut out, &mut ratios);
    assert!(ratios.iter().all(|&r| r == 1.0), "{ratios:?}");
    let s = rig.inst.allocator().slots();
    assert!(
        s.iter().any(|s| s.part() == Some(1)),
        "Part 2 took the voice"
    );
}
