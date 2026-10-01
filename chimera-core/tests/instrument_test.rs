//! The instrument audio path (instrument-core spec § Audio path): voices per
//! part, mono bus, constant-power pan and level into the part's DAC pair,
//! sends into the FX bus, FX return into DAC pair 1.

mod common;
use chimera_core::project::PartId;
use common::{SR, peak};

use chimera_core::dsp::Stereo;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::chorus::ChorusParams;
use chimera_core::dsp::ease::{Ease, ease_coeff, step_of};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::limiter::OUTPUT_TRIM;
use chimera_core::dsp::ring::{first_reflection, size_step};
use chimera_core::hw::{DAC_PAIRS, MAX_PARTS, MAX_VOICES};
use chimera_core::instrument::{
    AudioShared, DacBlocks, DacOut, Instrument, MixState, mix_parts, pan_gains,
};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::part::{DacPair, PartMode};
use chimera_core::preset::{Performance, Sound};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::fnv1a;

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};

/// The budget a voice share of `voice_share` cycles/sample gives, the FX
/// bus's own cost folded in: the smallest `cpu_hz` whose `SampleBudget`
/// rounds up to at least `FxBus::COST + voice_share` (`for_cpu` truncates).
const fn budget_for(voice_share: u32) -> SampleBudget {
    SampleBudget::for_cpu(((FxBus::COST.0 + voice_share) as u64 * 480_000).div_ceil(7) as u32)
}

/// The voices' share before the FX diet (7,000 − 3,310). After the diet
/// nothing sheds on rev V, so the allocation, stealing and shedding tests
/// here keep the share they were written against, whatever the bus costs.
const VOICE_SHARE: u32 = 3_690;
const BUDGET: SampleBudget = budget_for(VOICE_SHARE);

/// `Voice::cost` of factory Sound `i`, its own params and mod routing —
/// exactly what the allocator prices it at.
fn factory_voice_cost(i: usize) -> u32 {
    let s = chimera_core::factory::factory_sound(i).unwrap();
    chimera_core::dsp::voice::Voice::cost(&s.params, &s.mod_state).0
}

/// Room for the MORPH PAD voices a full-pool SAW LEAD chord's patch edit
/// leaves (two shed) plus the SQR BASS note that steals in next: just
/// enough that the note reuses the dying slot it waits on (ADR 0027)
/// instead of stealing a held one. Derived from `Voice::cost`, not a fixed
/// margin, so a future change to the cost model can't silently flip which
/// slot the note lands on.
fn morph_pad_plus_sqr_bass_budget() -> SampleBudget {
    budget_for((MAX_VOICES as u32 - 2) * factory_voice_cost(6) + factory_voice_cost(5))
}

fn on(ch: u8, note: u8) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind: NoteKind::On(Velocity::DEFAULT),
    }
}

fn off(ch: u8, note: u8) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind: NoteKind::Off,
    }
}

struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: chimera_core::scope::ScopeWriter,
}

impl Rig {
    fn new() -> Self {
        Self {
            inst: Box::new(Instrument::new(SR, BUDGET)),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: common::scope_writer(),
        }
    }
    /// The chip's own budget, for tests that fill the pool rather than the
    /// pre-diet voice share.
    fn rev_v() -> Self {
        let mut rig = Self::new();
        *rig.inst = Instrument::new(SR, SampleBudget::for_cpu(CPU_HZ_REV_V));
        rig
    }
    /// Rev V, or with the master tape (where ALGO INIT plays seven since
    /// ADR 0060) a budget for eight: the pool full of INIT.
    fn full_pool() -> Self {
        let mut rig = Self::rev_v();
        if cfg!(feature = "master-tape") {
            let init = AudioShared::default();
            let p = &init.parts[0];
            let voice = chimera_core::dsp::voice::Voice::cost(&p.params, &p.mod_state).0;
            *rig.inst = Instrument::new(SR, budget_for(8 * voice));
        }
        rig
    }
    fn render(&mut self, shared: &AudioShared) -> &DacOut {
        self.inst
            .render(&mut self.fx, &mut self.dac, shared, &mut self.scope);
        self.dac.out()
    }
    /// The last block out, limited.
    fn out(&self) -> &DacOut {
        self.dac.out()
    }
}

/// Left and right halves of an interleaved pair.
fn lr(pair: &[f32; BLOCK_SIZE * 2]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    (
        core::array::from_fn(|i| pair[2 * i]),
        core::array::from_fn(|i| pair[2 * i + 1]),
    )
}

#[test]
fn pan_law_is_constant_power() {
    let (l, r) = pan_gains(0.0);
    assert!(
        (l - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-7 && l == r,
        "centre = -3 dB per side"
    );
    assert_eq!(pan_gains(-1.0), (1.0, 0.0), "hard left");
    assert_eq!(pan_gains(1.0), (0.0, 1.0), "hard right");
    assert_eq!(pan_gains(-3.0), pan_gains(-1.0), "clamped left");
    assert_eq!(pan_gains(2.5), pan_gains(1.0), "clamped right");
    assert_eq!(pan_gains(f32::NAN), pan_gains(0.0), "NaN is centre");
    for p in [-0.7f32, -0.2, 0.3, 0.9] {
        let (l, r) = pan_gains(p);
        assert!((l * l + r * r - 1.0).abs() < 1e-6, "pan {p}");
    }
}

/// The DAC pair gets the part's mono bus × pan gain × level, then × the
/// output trim, one limiter lookahead (a block) later.
#[test]
fn part_bus_is_panned_and_levelled_into_its_pair() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    shared.parts[0].mix.level = 0.5;
    shared.parts[0].mix.pan = -1.0;
    rig.inst.handle(on(0, 60), &shared);
    for _ in 0..4 {
        rig.render(&shared);
    }
    let bus = *rig.inst.part_bus(0);
    rig.render(&shared);
    let (l, r) = lr(&rig.out()[0]);
    assert!(peak(&bus) > 0.01);
    for i in 0..BLOCK_SIZE {
        assert_eq!(l[i], bus[i] * 0.5 * OUTPUT_TRIM, "sample {i}");
        assert_eq!(r[i], 0.0);
    }
    assert_eq!(
        peak(&rig.out()[1]) + peak(&rig.out()[2]),
        0.0,
        "other pairs silent"
    );
}

/// The bench times `mix_parts` alone: it is exactly `render`'s steps 2–4.
#[test]
fn mix_parts_alone_is_renders_mix() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    shared.parts[0].mix.pan = 0.3;
    shared.parts[0].mix.sends = [0.2, 0.3, 0.4];
    shared.fx.delay.mix = 0.5;
    rig.inst.handle(on(0, 60), &shared);
    rig.render(&shared);
    let bus = *rig.inst.part_bus(0);
    assert!(peak(&bus) > 0.01);
    let mut buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
    buses[0] = bus;
    let mut written = [false; MAX_PARTS];
    written[0] = true;
    let mut fx = Box::new(FxBus::new());
    let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
    let mut out = Box::new(DacBlocks::new());
    *out.mix() = [[1.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    let scope = mix_parts(
        &buses,
        &written,
        &mut sends,
        &mut MixState::default(),
        &mut fx,
        &shared,
        SR,
        &mut out,
    );
    assert_eq!(scope, bus);
    assert_eq!(out.out(), rig.out());
}

/// `mix_parts` as first written: each Part added into zeroed buffers, then
/// the master section and the limiter; each gain eased as `MixState` eases
/// it.
fn mix_parts_reference(
    buses: &[[f32; BLOCK_SIZE]; MAX_PARTS],
    written: &[bool; MAX_PARTS],
    sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
    fx: &mut FxBus,
    shared: &AudioShared,
    dac: &mut DacBlocks,
    eases: &mut [[Ease; 5]; MAX_PARTS],
) -> [f32; BLOCK_SIZE] {
    let out = dac.mix();
    *out = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    *sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
    let mut scope = [0.0f32; BLOCK_SIZE];
    let k = ease_coeff(SR);
    let mut moving = Vec::new();
    for (p, part) in shared.parts.iter().enumerate() {
        let (gl, gr) = pan_gains(part.mix.pan);
        let [s0, s1, s2] = part.mix.sends;
        let to = [gl * part.mix.level, gr * part.mix.level, s0, s1, s2];
        if !written[p] {
            for (e, &t) in eases[p].iter_mut().zip(&to) {
                e.land(t);
            }
            continue;
        }
        let g: [(f32, f32); 5] = core::array::from_fn(|j| eases[p][j].step(to[j], k));
        let bus = &buses[p];
        let k = part.mix.output.index();
        for i in 0..BLOCK_SIZE {
            out[k][2 * i] += bus[i] * g[0].0;
            out[k][2 * i + 1] += bus[i] * g[1].0;
            scope[i] += bus[i];
        }
        for (send, &(from, _)) in sends.iter_mut().zip(&g[2..]) {
            for (s, &b) in send.iter_mut().zip(bus) {
                *s += b * from;
            }
        }
        if g.iter().any(|g| g.0 != g.1) {
            moving.push((p, k, g.map(|g| step_of(g, BLOCK_SIZE))));
        }
    }
    // Each moving gain's ramp past its start, after every Part's start.
    let n = |i: usize| (i + 1) as f32;
    for &(p, _, steps) in &moving {
        for (send, &step) in sends.iter_mut().zip(&steps[2..]) {
            if step != 0.0 {
                for (i, (s, &b)) in send.iter_mut().zip(&buses[p]).enumerate() {
                    *s += b * (step * n(i));
                }
            }
        }
    }
    let mut ret = Stereo::SILENT;
    fx.process(sends, &shared.fx, SR, &mut ret);
    for (i, (&l, &r)) in ret.l.iter().zip(&ret.r).enumerate() {
        out[0][2 * i] += l;
        out[0][2 * i + 1] += r;
    }
    for &(p, k, steps) in &moving {
        for (side, &step) in steps[..2].iter().enumerate() {
            if step != 0.0 {
                for (i, &b) in buses[p].iter().enumerate() {
                    out[k][2 * i + side] += b * (step * n(i));
                }
            }
        }
    }
    fx.master(out, &shared.fx, SR);
    fx.limit(dac, SR);
    scope
}

fn bits<const N: usize>(x: &[f32; N]) -> [u32; N] {
    x.map(f32::to_bits)
}

/// The fused mix is bit-for-bit the reference, every Part written or some,
/// every FX on, pans moving under one `MixState`, signed zeros on the buses.
#[test]
fn mix_parts_is_bit_identical_to_the_reference() {
    let mut x = 0x9e37_79b9u32;
    let mut rnd = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x as f32 / u32::MAX as f32
    };
    let mut shared = AudioShared::default();
    shared.fx.chorus.mode = 3;
    shared.fx.chorus.mix = 0.5;
    shared.fx.delay.mix = 0.5;
    shared.fx.reverb.mix = 0.5;
    let (mut fx, mut fx_ref) = (Box::new(FxBus::new()), Box::new(FxBus::new()));
    let mut pans = MixState::default();
    let mut eases = [[Ease::default(); 5]; MAX_PARTS];
    let (mut sends, mut sends_ref) = ([[0.0; BLOCK_SIZE]; FX_SENDS], [[0.0; BLOCK_SIZE]; FX_SENDS]);
    let mut out = Box::new(DacBlocks::new());
    *out.mix() = [[1.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    let mut out_ref = Box::new(DacBlocks::new());
    let pairs = [DacPair::P1, DacPair::P2, DacPair::P3];
    for block in 0..200 {
        for part in shared.parts.iter_mut() {
            // Pans hold for a few blocks, as the cache sees them.
            if block % 4 == 0 {
                part.mix.pan = rnd() * 2.4 - 1.2;
            }
            part.mix.level = rnd();
            part.mix.sends = [rnd(), rnd(), rnd()];
            part.mix.output = pairs[(rnd() * 3.0) as usize % 3];
        }
        let buses: [[f32; BLOCK_SIZE]; MAX_PARTS] = core::array::from_fn(|_| {
            core::array::from_fn(|_| match rnd() {
                r if r < 0.05 => -0.0,
                r if r < 0.1 => 0.0,
                _ => rnd() * 2.0 - 1.0,
            })
        });
        let written: [bool; MAX_PARTS] = match block % 3 {
            0 => [true; MAX_PARTS],
            1 => core::array::from_fn(|_| rnd() < 0.5),
            _ => [false; MAX_PARTS],
        };
        let scope = mix_parts(
            &buses, &written, &mut sends, &mut pans, &mut fx, &shared, SR, &mut out,
        );
        let scope_ref = mix_parts_reference(
            &buses,
            &written,
            &mut sends_ref,
            &mut fx_ref,
            &shared,
            &mut out_ref,
            &mut eases,
        );
        assert_eq!(bits(&scope), bits(&scope_ref), "block {block}: scope");
        for k in 0..DAC_PAIRS {
            assert_eq!(
                bits(&out.out()[k]),
                bits(&out_ref.out()[k]),
                "block {block}: pair {k}"
            );
        }
        for k in 0..FX_SENDS {
            assert_eq!(
                bits(&sends[k]),
                bits(&sends_ref[k]),
                "block {block}: send {k}"
            );
        }
    }
}

/// Send/return: a Part on pair 3, panned hard left, with a reverb send puts
/// no dry signal on pair 1 — pair 1 carries exactly the FX return, × the
/// output trim a limiter lookahead late, which is silent until its first
/// reflection.
#[test]
fn fx_send_puts_no_dry_signal_on_pair_1() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    shared.fx.reverb.mix = 0.5;
    shared.fx.reverb.time = 0.7;
    shared.parts[0].mix.output = DacPair::P3;
    shared.parts[0].mix.pan = -1.0;
    shared.parts[0].mix.sends[2] = 0.5;
    let mut fx = Box::new(FxBus::new());
    rig.inst.handle(on(0, 60), &shared);
    let mut last = Stereo::SILENT;
    for b in 0..120 {
        rig.render(&shared);
        let bus = *rig.inst.part_bus(0);
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], bus.map(|s| s * 0.5)];
        let mut ret = Stereo::SILENT;
        fx.process(&mut sends, &shared.fx, SR, &mut ret);
        let (l, r) = lr(&rig.out()[0]);
        assert_eq!(
            (l, r),
            (last.l, last.r),
            "block {b}: pair 1 is the return only"
        );
        last = Stereo {
            l: ret.l.map(|x| x * OUTPUT_TRIM),
            r: ret.r.map(|x| x * OUTPUT_TRIM),
        };
        let (l3, r3) = lr(&rig.out()[2]);
        assert_eq!(peak(&r3), 0.0, "block {b}: hard left");
        if b < first_reflection(size_step(shared.fx.reverb.size)) / BLOCK_SIZE {
            assert!(
                b < 2 || peak(&l3) > 0.01,
                "block {b}: the part sounds on pair 3"
            );
            assert_eq!(peak(&rig.out()[0]), 0.0, "block {b}: no dry on pair 1");
        }
    }
    assert!(peak(&rig.out()[0]) > 1e-4, "the wet return arrived");
}

/// FX diet spec § Bus: the chorus returns stereo on pair 1, and the mono
/// sum keeps it.
#[test]
fn the_chorus_returns_stereo_on_pair_1() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    shared.fx.chorus = ChorusParams {
        mode: 1,
        rate: 0.5,
        depth: 0.5,
        mix: 1.0,
    };
    shared.parts[0].mix.output = DacPair::P3;
    shared.parts[0].mix.sends = [1.0, 0.0, 0.0];
    rig.inst.handle(on(0, 60), &shared);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for _ in 0..100 {
        rig.render(&shared);
        let (a, b) = lr(&rig.out()[0]);
        l.extend(a);
        r.extend(b);
    }
    let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
    let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a + b) / 2.0).collect();
    assert!(rms(&l) > 1e-3, "the chorus returns");
    assert!(l != r, "the sides differ");
    assert!(rms(&mono) >= 0.5 * rms(&l));
}

/// Part routing by channel at dequeue; parts sharing a channel layer.
#[test]
fn notes_route_by_channel() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    rig.inst.handle(on(2, 60), &shared);
    rig.render(&shared);
    for p in 0..6 {
        assert_eq!(peak(rig.inst.part_bus(p)) > 0.0, p == 2, "part {p}");
    }
    shared.parts[4].mix.channel = MidiChannel::new(2).unwrap();
    rig.inst.handle(on(2, 64), &shared);
    rig.render(&shared);
    assert!(
        peak(rig.inst.part_bus(4)) > 0.0,
        "part 5 layered on channel 3"
    );
}

/// Rule 5: after note-off the voice keeps rendering its tail and is freed
/// only when its engine goes quiet.
#[test]
fn tails_ring_out_then_free_the_voice() {
    let mut rig = Rig::new();
    let shared = AudioShared::default(); // Algo init, RR 8
    rig.inst.handle(on(0, 60), &shared);
    for _ in 0..20 {
        rig.render(&shared);
    }
    rig.inst.handle(off(0, 60), &shared);
    rig.render(&shared);
    assert!(peak(rig.inst.part_bus(0)) > 0.0, "tail");
    assert_eq!(rig.inst.allocator().slots()[0].part(), Some(0));
    let mut blocks = 0;
    while !rig.inst.allocator().slots()[0].is_free() {
        rig.render(&shared);
        blocks += 1;
        assert!(blocks < 2_000, "voice never freed");
    }
    assert!(
        blocks > 50,
        "freed after {blocks} blocks: before the 0.3 s release ended"
    );
}

/// Six Mono Parts of the costliest factory Sound, MORPH KEYS, spend the
/// rev V budget with two voices still free: a Poly note on top would go over
/// it, and every sounding voice is mono, so nothing can be stolen.
#[test]
fn refused_notes_are_counted_and_silent() {
    let mut rig = Rig::rev_v();
    let mut perf = Performance::new();
    for p in PartId::ALL {
        *perf.edit(p).sound = chimera_core::factory::factory_sound(7).unwrap();
    }
    let mut shared = AudioShared::from_performance(&perf);
    for p in 0..MAX_PARTS {
        shared.parts[p].mix.mode = PartMode::Mono;
        rig.inst.handle(on(p as u8, 60), &shared);
    }
    assert_eq!(rig.inst.allocator().refused(), 0);
    assert!(rig.inst.allocator().slots().iter().any(|s| s.is_free()));
    shared.parts[0].mix.mode = PartMode::Poly;
    shared.parts[0].mix.channel = MidiChannel::new(9).unwrap();
    rig.inst.handle(on(9, 72), &shared);
    assert_eq!(rig.inst.allocator().refused(), 1);
}

/// A performance-level render for the new goldens: `blocks` blocks, notes
/// on at block 0 and off at `blocks / 2`, every DAC sample hashed.
fn render_perf(perf: &Performance, notes: &[(u8, u8)], blocks: usize) -> Vec<f32> {
    render_tap(perf, notes, blocks, |rig| rig.out())
}

/// `render_perf`, each block read from `tap` after `render`.
fn render_tap(
    perf: &Performance,
    notes: &[(u8, u8)],
    blocks: usize,
    tap: fn(&Rig) -> &DacOut,
) -> Vec<f32> {
    let shared = AudioShared::from_performance(perf);
    let mut rig = Rig::new();
    let mut all = Vec::new();
    for b in 0..blocks {
        for &(ch, n) in notes {
            if b == 0 {
                rig.inst.handle(on(ch, n), &shared);
            }
            if b == blocks / 2 {
                rig.inst.handle(off(ch, n), &shared);
            }
        }
        rig.render(&shared);
        for pair in tap(&rig) {
            all.extend_from_slice(pair);
        }
    }
    all
}

const CHORD4: [(u8, u8); 4] = [(0, 60), (0, 64), (0, 67), (0, 71)];

fn chord() -> Vec<f32> {
    render_perf(&Performance::new(), &CHORD4, 200)
}

/// Part 2 plays Modal out of pair 2.
fn two_parts() -> Vec<f32> {
    render_perf(&two_parts_perf(), &[(0, 60), (1, 67)], 200)
}

fn two_parts_perf() -> Performance {
    let mut perf = Performance::new();
    *perf.edit(PartId::ALL[1]).sound = Sound::init(EngineType::Modal);
    perf.edit(PartId::ALL[1]).mix.output = DacPair::P2;
    perf.edit(PartId::ALL[1]).mix.pan = 0.5;
    perf
}

/// Part 1 on a lone sine: the reverb scenes lock the FX, not INIT's
/// voicing (INIT through this reverb peaks at 1.55: #193).
fn sine_perf() -> Performance {
    let mut perf = Performance::new();
    perf.edit(PartId::ALL[0]).sound.params.algo = AlgoParams::single(WaveId::W1);
    perf
}

fn reverb_send(send: f32) -> Vec<f32> {
    render_perf(&reverb_perf(send), &[(0, 60)], 300)
}

fn reverb_perf(send: f32) -> Performance {
    let mut perf = sine_perf();
    perf.fx.reverb.mix = 0.5;
    perf.fx.reverb.time = 0.7;
    perf.edit(PartId::ALL[0]).mix.sends[2] = send;
    perf
}

/// ADR 0011's gate for `reverb_send_on`: finite, within ±1.0, and the
/// reverb audible over the dry render.
#[test]
fn reverb_send_on_passes_the_sanity_gate() {
    let (dry, wet) = (reverb_send(0.0), reverb_send(0.5));
    assert!(wet.iter().all(|s| s.is_finite()));
    assert!(peak(&wet) <= 1.0, "{}", peak(&wet));
    let diff: Vec<f32> = wet.iter().zip(&dry).map(|(a, b)| a - b).collect();
    assert!(peak(&diff) > 1e-3);
}

const CHORD6: [u8; 6] = [48, 55, 60, 64, 67, 72];
/// One note per voice: the pool full.
const CHORD_FULL: [u8; MAX_VOICES] = [48, 52, 55, 60, 64, 67, 72, 76];

fn factory(i: usize) -> Performance {
    let mut perf = Performance::new();
    *perf.edit(PartId::ALL[0]).sound =
        chimera_core::factory::factory_sound(i).expect("factory Sound");
    perf
}

/// Six voices of the factory SAW LEAD, which fits six (ADR 0026).
fn six_voice_chord() -> Vec<f32> {
    render_perf(&factory(4), &CHORD6.map(|n| (0, n)), 200)
}

#[test]
fn a_six_voice_saw_lead_chord_is_not_refused() {
    let shared = AudioShared::from_performance(&factory(4));
    let mut rig = Rig::new();
    for n in CHORD6 {
        rig.inst.handle(on(0, n), &shared);
    }
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert_eq!(a.refused(), 0);
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), 6);
}

/// ADR 0026: TX EPIANO fits five voices, so the sixth note steals the
/// oldest held one (voice_alloc rule 4) and the pool stays in budget.
#[test]
fn a_six_note_tx_epiano_chord_stays_in_budget() {
    let shared = AudioShared::from_performance(&factory(1));
    let mut rig = Rig::new();
    for n in CHORD6 {
        rig.inst.handle(on(0, n), &shared);
        assert!(rig.inst.allocator().sounding_cost() + FxBus::COST <= BUDGET.as_cost());
    }
    for _ in 0..4 {
        rig.render(&shared);
    }
    let a = rig.inst.allocator();
    assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());
    assert_eq!(a.refused(), 0);
    let mut held: Vec<u8> = a
        .slots()
        .iter()
        .filter_map(|s| s.note().filter(|_| s.held()).map(|n| n.get()))
        .collect();
    held.sort();
    assert_eq!(held, CHORD6[1..], "the oldest note, 48, was stolen");
}

#[test]
fn the_test_budget_keeps_the_pre_diet_voice_share() {
    assert_eq!(BUDGET.as_cost().0, FxBus::COST.0 + VOICE_SHARE);
}

/// ADR 0040: on rev V a light patch (SAW LEAD, 555) plays every voice of the
/// pool; the budget would allow ten.
#[test]
fn a_light_patch_plays_every_voice_on_rev_v() {
    let shared = AudioShared::from_performance(&factory(4));
    let mut rig = Rig::rev_v();
    for n in CHORD_FULL {
        rig.inst.handle(on(0, n), &shared);
    }
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert_eq!(a.refused(), 0);
    assert_eq!(
        a.slots().iter().filter(|s| !s.is_free()).count(),
        MAX_VOICES
    );
    assert!(a.sounding_cost() + FxBus::COST <= SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost());
}

/// Spec "Done when" and ADR 0040: on rev V the allocator grants the
/// costliest patch, A16 ∪ A17, six voices: a full-pool chord steals, so six
/// of its notes sound and none is refused.
#[test]
fn the_costliest_patch_plays_six_voices_on_rev_v() {
    use chimera_core::dsp::algo::algorithms::AlgoId;
    let mut shared = AudioShared::default();
    let a = &mut shared.parts[0].params.algo;
    (a.alg_a, a.alg_b, a.morph) = (AlgoId::A16.get(), AlgoId::A17.get(), 64);
    for op in a.ops.iter_mut() {
        (op.level, op.feedback) = (99, 7);
    }
    let mut rig = Rig::rev_v();
    for n in CHORD_FULL {
        rig.inst.handle(on(0, n), &shared);
    }
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert_eq!(a.refused(), 0);
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), 6);
    assert!(a.sounding_cost() + FxBus::COST <= SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost());
}

/// Recorded when the instrument path landed (plan Task 12). Re-record only
/// for an intended sound change (`common::golden`). Every row re-recorded
/// at Task 17 for ADR 0060's voice DC blocker; each comment names what came
/// before. The Algo rows re-recorded for the filter's C1 `saturate` (ADR
/// 0063), then every row for its INIT (OUT LEVEL 45/128, RR 5).
const GOLDENS: &[(&str, u64)] = &[
    ("poly_chord", 0xba9fbebb12bbbc4d), // ADR 0049 INIT, then the ADR 0050 output trim
    // Re-recorded: Modal 2 step A, then ADR 0058's gain, then Task 18's DAMP make-up,
    // then the free ring on release (ADR 0062).
    ("two_parts_two_pairs", 0xd461a92a9542074a),
    ("reverb_send_off", 0xb7ff733538f39db9), // ADR 0049 INIT, then the ADR 0050 output trim
    ("reverb_send_on", 0xa7009ff4d6324987),  // ADR 0049 INIT, then the ADR 0050 output trim
    ("six_voice_chord", 0xb3bb31dbc471cf01), // ADR 0049 INIT, then the ADR 0050 output trim
];

/// ADR 0050: everything before the limiter is the mix unchanged by it,
/// bit for bit. These are the goldens as ADR 0049's INIT recorded them,
/// before the output trim; every row re-recorded for ADR 0060's voice DC
/// blocker, and for ADR 0063's C1 `saturate` and INIT.
const PRE_LIMITER: &[(&str, u64)] = &[
    ("poly_chord", 0x9a7e4fc66c45f339),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up,
    // then the free ring on release (ADR 0062).
    ("two_parts_two_pairs", 0x5505c654fb526b21),
    ("reverb_send_off", 0x8180167de55cf955),
    ("reverb_send_on", 0x746ea6b464bbdb38),
    ("six_voice_chord", 0x5efd58ea3b91d3bd),
];

/// A named golden case: a case name paired with its render function.
type GoldenCase = (&'static str, fn() -> Vec<f32>);

#[test]
fn instrument_goldens_match() {
    let cases: [GoldenCase; 5] = [
        ("poly_chord", chord),
        ("two_parts_two_pairs", two_parts),
        ("reverb_send_off", || reverb_send(0.0)),
        ("reverb_send_on", || reverb_send(0.5)),
        ("six_voice_chord", six_voice_chord),
    ];
    let got: Vec<_> = cases
        .into_iter()
        .map(|(name, render)| (name, fnv1a(&render())))
        .collect();
    common::golden::check(GOLDENS, &got);
}

#[test]
fn the_mix_before_the_limiter_is_mains() {
    fn pre(rig: &Rig) -> &DacOut {
        rig.dac.input()
    }
    let cases: [(&str, Vec<f32>); 5] = [
        (
            "poly_chord",
            render_tap(&Performance::new(), &CHORD4, 200, pre),
        ),
        (
            "two_parts_two_pairs",
            render_tap(&two_parts_perf(), &[(0, 60), (1, 67)], 200, pre),
        ),
        (
            "reverb_send_off",
            render_tap(&reverb_perf(0.0), &[(0, 60)], 300, pre),
        ),
        (
            "reverb_send_on",
            render_tap(&reverb_perf(0.5), &[(0, 60)], 300, pre),
        ),
        (
            "six_voice_chord",
            render_tap(&factory(4), &CHORD6.map(|n| (0, n)), 200, pre),
        ),
    ];
    let got: Vec<_> = cases.iter().map(|(name, v)| (*name, fnv1a(v))).collect();
    let failures = common::golden::mismatches(PRE_LIMITER, &got);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What the goldens lock is what the spec asks for.
#[test]
fn golden_scenes_do_what_they_say() {
    let frames = |v: &[f32], pair: usize| -> Vec<f32> {
        v.chunks(BLOCK_SIZE * 2 * DAC_PAIRS)
            .flat_map(|b| b[pair * BLOCK_SIZE * 2..][..BLOCK_SIZE * 2].to_vec())
            .collect()
    };
    // Four voices sound at once.
    let single = render_perf(&Performance::new(), &[(0, 60)], 200);
    assert!(peak(&chord()) > peak(&single));
    // Part 2 plays out of pair 2 only; pair 3 stays silent.
    let two = two_parts();
    assert!(peak(&frames(&two, 1)) > 0.01);
    assert_eq!(peak(&frames(&two, 2)), 0.0);
    // The send adds a reverb return (to pair 1) and nothing else changes
    // when it is 0: send off = no FX at all.
    let (dry, wet) = (reverb_send(0.0), reverb_send(0.5));
    assert_ne!(fnv1a(&dry), fnv1a(&wet));
    assert_eq!(
        fnv1a(&dry),
        fnv1a(&render_perf(&sine_perf(), &[(0, 60)], 300))
    );
}

/// Review Focus: a Part's channel changes while a key is held. The
/// note-off arrives on the channel the note-on came from and still
/// releases the voice (no stuck note).
#[test]
fn note_off_follows_the_note_on_channel() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    rig.inst.handle(on(0, 60), &shared);
    rig.render(&shared);
    shared.parts[0].mix.channel = MidiChannel::new(3).unwrap();
    rig.inst.handle(off(0, 60), &shared);
    assert!(!rig.inst.allocator().slots()[0].held());
}

/// Review Focus: switching a held chord that fills the pool to a costlier
/// Sound must not push it over the CPU budget; the voices over budget are
/// cut, and as many as the Modal default's cost allows keep sounding.
#[test]
fn sound_change_mid_chord_stays_in_budget() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::params::{EngineType, ParamSnapshot};
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V);
    let mut rig = Rig::rev_v();
    let mut shared = AudioShared::default();
    for n in 0..MAX_VOICES as u8 {
        rig.inst.handle(on(0, 60 + n), &shared);
    }
    rig.render(&shared);
    let fits = |p: &ParamSnapshot| {
        ((budget.as_cost().0 - FxBus::COST.0) / Voice::cost(p, &ModState::new()).0)
            .min(MAX_VOICES as u32) as usize
    };
    let held = fits(&shared.parts[0].params);
    assert_eq!(
        rig.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| !s.is_free())
            .count(),
        held
    );
    shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert!(a.sounding_cost() + FxBus::COST <= budget.as_cost());
    let expected = ((budget.as_cost().0 - FxBus::COST.0)
        / Voice::cost(
            &ParamSnapshot::for_engine(EngineType::Modal),
            &ModState::new(),
        )
        .0)
        .min(held as u32);
    assert_eq!(
        a.slots().iter().filter(|s| !s.is_free()).count(),
        expected as usize
    );
    assert!(a.slots().iter().all(|s| s.is_free()
        || s.cost()
            == Voice::cost(
                &ParamSnapshot::for_engine(EngineType::Modal),
                &ModState::new()
            )));
}

/// ADR 0040, 0049: eight held INIT notes all sound at rev V's budget;
/// seven with the master tape (ADR 0055), whose two cycles to spare ADR
/// 0060's DC blocker took.
#[test]
fn eight_init_voices_fit_rev_v() {
    let mut rig = Rig::rev_v();
    let shared = AudioShared::default();
    for n in 0..MAX_VOICES as u8 {
        rig.inst.handle(on(0, 60 + n), &shared);
    }
    for _ in 0..4 {
        rig.render(&shared);
    }
    let a = rig.inst.allocator();
    assert_eq!(MAX_VOICES, 8);
    let want = if cfg!(feature = "master-tape") { 7 } else { 8 };
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), want);
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    assert!(
        a.sounding_cost() + FxBus::COST <= budget,
        "{:?}",
        a.sounding_cost()
    );
}

/// Blocks after a lone note-off (note 60, default Sound) until the voice's
/// engine goes quiet and the allocator frees it.
fn blocks_until_free() -> usize {
    let mut rig = Rig::new();
    let shared = AudioShared::default();
    rig.inst.handle(on(0, 60), &shared);
    for _ in 0..20 {
        rig.render(&shared);
    }
    rig.inst.handle(off(0, 60), &shared);
    let mut blocks = 0;
    while !rig.inst.allocator().slots()[0].is_free() {
        rig.render(&shared);
        blocks += 1;
        assert!(blocks < 2_000, "voice never freed");
    }
    blocks
}

/// Controller item (a): a voice is freed only when the note it plays *now*
/// goes quiet — never on a report about the note it played before. Voice 0
/// takes a new note around the block where its old tail ends (stolen while
/// still releasing, or taken right as it is freed). The new note is a Modal
/// pluck on part 2, which rings out for many blocks even when released at
/// once: so it must survive being held for 20 blocks, and also being
/// released before it has rendered a single block (`held == 0`) — the case
/// where a stale "old note went quiet" report would free it unheard.
#[test]
fn stealing_a_releasing_voice_does_not_free_the_new_note() {
    let n = blocks_until_free();
    assert!(n > 2);
    let mut shared = AudioShared::default();
    shared.parts[1].params = ParamSnapshot::for_engine(EngineType::Modal);
    for (after_off, held) in [n - 2, n - 1, n, n + 1]
        .into_iter()
        .flat_map(|a| [(a, 0), (a, 20)])
    {
        let mut rig = Rig::full_pool();
        for k in 0..MAX_VOICES as u8 {
            rig.inst.handle(on(0, 60 + k), &shared); // the pool full, voice 0 oldest
        }
        for _ in 0..20 {
            rig.render(&shared);
        }
        rig.inst.handle(off(0, 60), &shared);
        for _ in 0..after_off {
            rig.render(&shared);
        }
        rig.inst.handle(on(1, 72), &shared); // voice 0: stolen, or free again
        let s = rig.inst.allocator().slots()[0];
        assert_eq!(
            (s.part(), s.note(), s.held()),
            (Some(1), MidiNote::new(72), true),
            "{after_off}: voice 0 took it"
        );
        for b in 0..held {
            rig.render(&shared);
            assert!(
                rig.inst.allocator().slots()[0].held(),
                "{after_off}: new note freed at block {b}"
            );
        }
        rig.inst.handle(off(1, 72), &shared);
        let mut blocks = 0;
        while !rig.inst.allocator().slots()[0].is_free() {
            rig.render(&shared);
            blocks += 1;
            assert!(blocks < 20_000, "voice never freed");
        }
        assert!(
            blocks > 50,
            "{after_off}/{held}: new note freed after {blocks} blocks, before it rang out"
        );
    }
}

/// Controller item (a), Mono: a Mono part retriggers its own releasing
/// voice around the block where the old tail ends — here with a Modal pluck,
/// which rings out even when released at once. The new note is not freed
/// early, whether held for 20 blocks or released before it rendered.
#[test]
fn retriggering_a_releasing_mono_voice_does_not_free_the_new_note() {
    let n = blocks_until_free();
    let mut init = AudioShared::default();
    init.parts[0].mix.mode = PartMode::Mono;
    let mut modal = init.clone();
    modal.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    for (after_off, held) in [n - 2, n - 1, n, n + 1]
        .into_iter()
        .flat_map(|a| [(a, 0), (a, 20)])
    {
        let mut rig = Rig::new();
        rig.inst.handle(on(0, 60), &init);
        for _ in 0..20 {
            rig.render(&init);
        }
        rig.inst.handle(off(0, 60), &init);
        for _ in 0..after_off {
            rig.render(&init);
        }
        rig.inst.handle(on(0, 62), &modal);
        for b in 0..held {
            rig.render(&modal);
            let s = rig
                .inst
                .allocator()
                .slots()
                .iter()
                .find(|s| !s.is_free())
                .copied();
            let s = s.unwrap_or_else(|| panic!("{after_off}: new note freed at block {b}"));
            assert_eq!((s.note(), s.held()), (MidiNote::new(62), true));
        }
        rig.inst.handle(off(0, 62), &modal);
        let mut blocks = 0;
        while rig.inst.allocator().slots().iter().any(|s| !s.is_free()) {
            rig.render(&modal);
            blocks += 1;
            assert!(blocks < 20_000, "voice never freed");
        }
        assert!(
            blocks > 50,
            "{after_off}/{held}: new note freed after {blocks} blocks, before it rang out"
        );
    }
}

/// Review Focus (stuck notes): one Part holds the same key from two
/// channels — C4 from channel 1, then the Part moves to channel 4 and C4
/// comes again. The second re-strikes the key's voice (ADR 0062), which
/// channel 4's note-off releases, in either order; channel 1's releases
/// nothing, and the voice rings out and is freed.
#[test]
fn same_note_from_two_channels_releases_both_voices() {
    for ch4_first in [true, false] {
        let mut rig = Rig::new();
        let mut shared = AudioShared::default();
        rig.inst.handle(on(0, 60), &shared); // voice 0
        rig.render(&shared);
        shared.parts[0].mix.channel = MidiChannel::new(3).unwrap();
        rig.inst.handle(on(3, 60), &shared); // voice 0, re-struck
        rig.render(&shared);
        let part0 = rig
            .inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| s.part() == Some(0));
        assert_eq!(part0.count(), 1, "one voice");
        let (first, second) = if ch4_first { (3, 0) } else { (0, 3) };
        rig.inst.handle(off(first, 60), &shared);
        let held = rig.inst.allocator().slots()[0].held();
        assert_eq!(held, !ch4_first, "channel {first}'s note-off");
        rig.inst.handle(off(second, 60), &shared);
        assert!(!rig.inst.allocator().slots()[0].held());
        let mut blocks = 0;
        while rig.inst.allocator().slots().iter().any(|s| !s.is_free()) {
            rig.render(&shared);
            blocks += 1;
            assert!(
                blocks < 3_000,
                "stuck: {:?}",
                rig.inst.allocator().slots().map(|s| (s.part(), s.held()))
            );
        }
    }
}

/// Six SAW LEAD notes, then the chord's Sound becomes TX EPIANO, which fits
/// five (ADR 0026): one voice is shed. `release` notes are let go first.
fn shed_one(release: &[u8]) -> (Rig, AudioShared) {
    let light = AudioShared::from_performance(&factory(4));
    let heavy = AudioShared::from_performance(&factory(1));
    let mut rig = Rig::new();
    for n in CHORD6 {
        rig.inst.handle(on(0, n), &light);
    }
    for _ in 0..20 {
        rig.render(&light);
    }
    for &n in release {
        rig.inst.handle(off(0, n), &light);
    }
    rig.render(&light);
    assert_eq!(
        rig.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| !s.is_free())
            .count(),
        6
    );
    rig.render(&heavy);
    (rig, heavy)
}

fn dying(rig: &Rig) -> Vec<(usize, u8)> {
    let slots = rig.inst.allocator().slots();
    (0..slots.len())
        .filter(|&v| slots[v].dying())
        .map(|v| (v, slots[v].note().unwrap().get()))
        .collect()
}

/// ADR 0027: a recost over budget takes a released tail, the oldest, before
/// any held note; the dying voice is not reallocated, and frees when the
/// fade ends, back within budget.
#[test]
fn a_patch_edit_over_budget_fades_a_tail_first() {
    let (mut rig, heavy) = shed_one(&[64, 55]);
    let d = dying(&rig);
    assert_eq!(
        d.iter().map(|x| x.1).collect::<Vec<_>>(),
        [55],
        "the oldest tail"
    );
    let v = d[0].0;
    rig.inst.handle(on(0, 90), &heavy);
    let s = rig.inst.allocator().slots()[v];
    assert!(
        s.dying() && s.note() == MidiNote::new(55),
        "reallocated mid-fade"
    );
    rig.render(&heavy); // the fade's second block
    let a = rig.inst.allocator();
    assert!(a.slots()[v].is_free());
    assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());
}

/// With only held notes, the newest is shed. It keeps its slot, not
/// reallocated, through the two-block fade (the block it was shed in and
/// the next), then frees.
#[test]
fn a_patch_edit_over_budget_fades_the_newest_held_note() {
    let (mut rig, heavy) = shed_one(&[]);
    let d = dying(&rig);
    assert_eq!(d.iter().map(|x| x.1).collect::<Vec<_>>(), [72]);
    rig.inst.handle(off(0, 48), &heavy); // a tail to steal
    rig.inst.handle(on(0, 90), &heavy);
    assert_eq!(dying(&rig), d, "reallocated mid-fade");
    rig.render(&heavy);
    assert!(dying(&rig).is_empty());
    let a = rig.inst.allocator();
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), 5);
    assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());
}

/// The slots holding `note`, and whether each is dying.
fn slots_of(rig: &Rig, note: u8) -> Vec<(usize, bool)> {
    let slots = rig.inst.allocator().slots();
    (0..slots.len())
        .filter(|&v| slots[v].note().map(|n| n.get()) == Some(note))
        .map(|v| (v, slots[v].dying()))
        .collect()
}

/// #33 M1: a note-on in the same block as a patch edit is judged against
/// the new patch's costs, so it is not admitted and then shed at once. The
/// edit sheds the newest held note, 67, as `render` would; the note-on then
/// steals the oldest held note, 48 (voice_alloc rule 4), as it would a
/// block later.
#[test]
fn a_note_on_with_a_patch_edit_is_judged_at_the_new_cost() {
    let light = AudioShared::from_performance(&factory(4)); // SAW LEAD: 6
    let heavy = AudioShared::from_performance(&factory(6)); // MORPH PAD: 4
    let mut rig = Rig::new();
    for n in &CHORD6[..5] {
        rig.inst.handle(on(0, *n), &light);
    }
    for _ in 0..4 {
        rig.render(&light);
    }
    rig.inst.handle(on(0, 90), &heavy);
    for _ in 0..4 {
        rig.render(&heavy);
    }
    let a = rig.inst.allocator();
    assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());
    assert_eq!(slots_of(&rig, 90).len(), 1, "the new note plays");
    let (v, dying) = slots_of(&rig, 90)[0];
    assert!(!dying && rig.inst.allocator().slots()[v].held());
    assert!(peak(rig.inst.part_bus(0)) > 0.0);
    assert_eq!(rig.inst.allocator().refused(), 0);
    assert!(slots_of(&rig, 48).is_empty() && slots_of(&rig, 67).is_empty());
    for n in [55, 60, 64] {
        assert_eq!(slots_of(&rig, n).len(), 1, "{n} still held");
    }
}

/// #33 M7: with the pool full only because voices are fading out, a note
/// that fits the budget takes a dying slot and plays once the fade ends; no
/// held note is stolen and none is refused.
#[test]
fn a_note_on_waits_out_a_fade_before_stealing_a_held_note() {
    let perf = |lead: usize| {
        let mut p = factory(lead);
        *p.edit(PartId::ALL[1]).sound = chimera_core::factory::factory_sound(5).unwrap(); // SQR BASS
        AudioShared::from_performance(&p)
    };
    let (light, heavy) = (perf(4), perf(6)); // SAW LEAD fills the pool, MORPH PAD two short
    let mut rig = Rig::new();
    *rig.inst = Instrument::new(SR, morph_pad_plus_sqr_bass_budget());
    for n in CHORD_FULL {
        rig.inst.handle(on(0, n), &light);
    }
    for _ in 0..4 {
        rig.render(&light);
    }
    rig.render(&heavy); // two are shed
    let dying: Vec<usize> = (0..MAX_VOICES)
        .filter(|&v| rig.inst.allocator().slots()[v].dying())
        .collect();
    assert_eq!(dying.len(), 2);
    rig.inst.handle(on(1, 40), &heavy);
    let a = rig.inst.allocator();
    assert_eq!(a.refused(), 0);
    let held: Vec<_> = a
        .slots()
        .iter()
        .filter(|s| s.part() == Some(0) && s.held() && !s.dying())
        .collect();
    assert_eq!(held.len(), MAX_VOICES - 2, "no held note stolen");
    let (v, d) = slots_of(&rig, 40)[0];
    assert!(dying.contains(&v) && !d);
    rig.render(&heavy); // the fade's second block: bass still silent
    assert_eq!(peak(rig.inst.part_bus(1)), 0.0);
    rig.render(&heavy);
    assert!(
        peak(rig.inst.part_bus(1)) > 0.0,
        "the bass plays after the fade"
    );
    assert!(rig.inst.allocator().slots()[v].held());
}

/// ADR 0027: a patch edit that sheds a note still waiting out a fade drops
/// it unheard, and counts it as refused.
#[test]
fn a_shed_waiting_note_counts_as_refused() {
    let perf = |lead: usize, other: usize| {
        let mut p = factory(lead);
        *p.edit(PartId::ALL[1]).sound = chimera_core::factory::factory_sound(other).unwrap();
        AudioShared::from_performance(&p)
    };
    let mut rig = Rig::new();
    *rig.inst = Instrument::new(SR, morph_pad_plus_sqr_bass_budget());
    let light = perf(4, 5);
    for n in CHORD_FULL {
        rig.inst.handle(on(0, n), &light);
    }
    for _ in 0..4 {
        rig.render(&light);
    }
    let heavy = perf(6, 5);
    rig.render(&heavy); // two shed
    rig.inst.handle(on(1, 40), &heavy); // waits on a dying slot
    assert_eq!(slots_of(&rig, 40).len(), 1);
    rig.render(&perf(6, 7)); // part 2 now MORPH KEYS: the waiting note goes
    assert_eq!(rig.inst.allocator().refused(), 1);
    for _ in 0..4 {
        rig.render(&perf(6, 7));
    }
    assert!(slots_of(&rig, 40).is_empty());
    assert_eq!(peak(rig.inst.part_bus(1)), 0.0);
}

/// #33: a steal from another Part on the same engine fades the old sound
/// out on its own Part's bus, then starts the new note clean.
#[test]
fn a_steal_from_another_part_fades_on_the_old_bus() {
    let mut p = factory(4); // SAW LEAD on both Parts
    *p.edit(PartId::ALL[1]).sound = chimera_core::factory::factory_sound(4).unwrap();
    let shared = AudioShared::from_performance(&p);
    let mut rig = Rig::new();
    for n in CHORD6 {
        rig.inst.handle(on(0, n), &shared);
    }
    for _ in 0..4 {
        rig.render(&shared);
    }
    rig.inst.handle(off(0, 48), &shared);
    rig.render(&shared);
    rig.inst.handle(on(1, 40), &shared); // steals the tail of 48
    let (v, _) = slots_of(&rig, 40)[0];
    for _ in 0..2 {
        rig.render(&shared);
        assert_eq!(peak(rig.inst.part_bus(1)), 0.0, "fading on part 1's bus");
    }
    rig.render(&shared);
    assert!(peak(rig.inst.part_bus(1)) > 0.0, "then the new note plays");
    assert!(rig.inst.allocator().slots()[v].held());
}

/// Part 0 holds a six-note SAW LEAD chord, filling the pool; Parts 1 and 2
/// play SAW LEAD too, Part 1 in `mode`.
fn full_pool(mode: PartMode) -> (Rig, AudioShared) {
    let mut p = factory(4);
    for q in [1, 2] {
        *p.edit(PartId::ALL[q]).sound = chimera_core::factory::factory_sound(4).unwrap();
    }
    p.edit(PartId::ALL[1]).mix.mode = mode;
    let shared = AudioShared::from_performance(&p);
    let mut rig = Rig::new();
    for n in CHORD6 {
        rig.inst.handle(on(0, n), &shared);
    }
    for _ in 0..4 {
        rig.render(&shared);
    }
    (rig, shared)
}

/// A Mono Part that retriggers its own note still waiting out another
/// Part's fade replaces it unheard: counted as refused.
#[test]
fn a_mono_retrigger_of_a_waiting_note_counts_it_as_refused() {
    let (mut rig, shared) = full_pool(PartMode::Mono);
    rig.inst.handle(on(1, 40), &shared); // steals 48, waits
    let (v, _) = slots_of(&rig, 40)[0];
    rig.inst.handle(on(1, 41), &shared); // same voice, before 40 sounds
    assert_eq!(slots_of(&rig, 41), [(v, false)]);
    assert_eq!(rig.inst.allocator().refused(), 1);
    for _ in 0..3 {
        rig.render(&shared);
    }
    assert!(peak(rig.inst.part_bus(1)) > 0.0, "41 plays");
}

/// A second Part that steals a slot whose note is still waiting (released
/// before it sounded) drops that note: counted as refused.
#[test]
fn a_steal_of_a_waiting_note_counts_it_as_refused() {
    let (mut rig, shared) = full_pool(PartMode::Poly);
    rig.inst.handle(on(1, 90), &shared); // steals 48, waits
    let (v, _) = slots_of(&rig, 90)[0];
    rig.inst.handle(off(1, 90), &shared); // a short note, still unheard
    rig.inst.handle(on(2, 70), &shared); // takes the released slot
    assert_eq!(slots_of(&rig, 70), [(v, false)]);
    assert_eq!(rig.inst.allocator().refused(), 1);
    for _ in 0..3 {
        rig.render(&shared);
    }
    assert_eq!(peak(rig.inst.part_bus(1)), 0.0, "90 never sounds");
    assert!(peak(rig.inst.part_bus(2)) > 0.0, "70 plays");
}

/// FX diet spec § Tape: on pair 1 only, after its sum. With the tape up,
/// pair 1 changes and pairs 2 and 3 stay bit-identical. Without
/// `master-tape` (ADR 0055) the tape is off the chain: every pair stays
/// bit-identical.
#[test]
fn the_tape_is_on_pair_1_only() {
    let render = |mix: f32| {
        let mut shared = AudioShared::default();
        shared.parts[0].mix.output = DacPair::P1;
        shared.parts[1].mix.output = DacPair::P2;
        (shared.fx.tape.drive, shared.fx.tape.mix) = (1.0, mix);
        let buses: [[f32; BLOCK_SIZE]; MAX_PARTS] = core::array::from_fn(|p| {
            core::array::from_fn(|i| 0.5 * ((i + 7 * p) as f32 * 0.37).sin())
        });
        let written = [true, true, false, false, false, false];
        let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
        let mut pans = MixState::default();
        let mut fx = Box::new(FxBus::new());
        let mut out = Box::new(DacBlocks::new());
        let mut blocks = Vec::new();
        for _ in 0..32 {
            mix_parts(
                &buses, &written, &mut sends, &mut pans, &mut fx, &shared, SR, &mut out,
            );
            blocks.push(*out.out());
        }
        blocks
    };
    let (off, on) = (render(0.0), render(1.0));
    assert!(
        off.iter()
            .zip(&on)
            .all(|(a, b)| a[1] == b[1] && a[2] == b[2])
    );
    #[cfg(feature = "master-tape")]
    assert!(off.iter().zip(&on).any(|(a, b)| a[0] != b[0]));
    #[cfg(not(feature = "master-tape"))]
    assert!(off.iter().zip(&on).all(|(a, b)| a[0] == b[0]));
}

/// FX diet spec § Master comp: one gain for every pair. A quiet Part on
/// pair 2 is ducked by a loud Part on pair 1.
#[test]
fn the_master_comp_ducks_pair_2_with_pair_1() {
    let render = |ratio: u8| {
        let mut shared = AudioShared::default();
        shared.parts[0].mix.output = DacPair::P1;
        shared.parts[1].mix.output = DacPair::P2;
        (shared.fx.comp.thresh, shared.fx.comp.ratio) = (0.25, ratio);
        let buses: [[f32; BLOCK_SIZE]; MAX_PARTS] = core::array::from_fn(|p| match p {
            0 => [0.9; BLOCK_SIZE],
            1 => core::array::from_fn(|i| 0.01 * (i as f32 * 0.3).sin()),
            _ => [0.0; BLOCK_SIZE],
        });
        let written = [true, true, false, false, false, false];
        let mut sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
        let mut pans = MixState::default();
        let mut fx = Box::new(FxBus::new());
        let mut out = Box::new(DacBlocks::new());
        let mut energy = 0.0f32;
        for b in 0..128 {
            mix_parts(
                &buses, &written, &mut sends, &mut pans, &mut fx, &shared, SR, &mut out,
            );
            if b >= 64 {
                energy += out.out()[1].iter().map(|s| s * s).sum::<f32>();
            }
        }
        (energy, fx.master_gr_db())
    };
    let ((open, _), (ducked, gr)) = (render(0), render(7));
    let db = 10.0 * (ducked / open).log10();
    assert!(db < -15.0, "{db} dB");
    assert!(gr > 15.0, "{gr}");
}

/// #183: Sympathetic tails ringing when the Part switches to String fade
/// out on their own model (exclusive-state § 3) and cost Sympathetic until
/// the fade ends, so String notes played mid-fade are admitted against
/// that, not against String's price.
#[test]
fn a_mode_switch_bills_sounding_tails_at_their_own_model() {
    use chimera_core::dsp::modal::ResonatorMode;
    let voice = |mode| {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        chimera_core::dsp::voice::Voice::cost(&p, &ModState::new())
    };
    let (sym, string) = (
        voice(ResonatorMode::Sympathetic),
        voice(ResonatorMode::String),
    );
    let mut shared = AudioShared::default();
    shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    shared.parts[0].params.modal.mode = ResonatorMode::Sympathetic;
    let mut rig = Rig::rev_v();
    let tails = [48, 52, 55];
    for n in tails {
        rig.inst.handle(on(0, n), &shared);
    }
    rig.render(&shared);
    for n in tails {
        rig.inst.handle(off(0, n), &shared);
    }
    rig.render(&shared);
    let tail_voices: Vec<usize> = (0..MAX_VOICES)
        .filter(|&v| {
            rig.inst.allocator().slots()[v]
                .note()
                .is_some_and(|n| tails.contains(&n.get()))
        })
        .collect();
    assert_eq!(tail_voices.len(), tails.len());
    shared.parts[0].params.modal.mode = ResonatorMode::String;
    // The switch's first block: the tails are one block into their fade.
    rig.render(&shared);
    // Five fill the pool beside the tails: the budget alone decides.
    for n in 60..65 {
        rig.inst.handle(on(0, n), &shared);
    }
    // What the next block's voices really cost: a tail's voice, stolen or
    // not, fades out on its own model, and a note it took waits for that.
    let slots = rig.inst.allocator().slots();
    assert!(
        tail_voices.iter().all(|&v| !slots[v].is_free()),
        "the tails still fade"
    );
    let ringing = tail_voices.len();
    let strings = slots.iter().filter(|s| !s.is_free()).count() - ringing;
    let real = sym.0 * ringing as u32 + string.0 * strings as u32;
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    assert!(
        real + FxBus::COST.0 <= budget,
        "{ringing} SYM tails and {strings} STR notes cost {real} + FX over {budget}"
    );
}

fn loud(note: u8) -> NoteEvent {
    NoteEvent {
        kind: NoteKind::On(Velocity::new(127).unwrap()),
        ..on(0, note)
    }
}

/// A STRING Part, rendered `blocks` blocks: Part 1's bus.
fn string_blocks(rig: &mut Rig, shared: &AudioShared, blocks: usize, out: &mut Vec<f32>) {
    for _ in 0..blocks {
        rig.render(shared);
        out.extend_from_slice(rig.inst.part_bus(0));
    }
}

fn string_part() -> AudioShared {
    let mut shared = AudioShared::default();
    shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    shared.parts[0].params.modal.mode = chimera_core::dsp::modal::ResonatorMode::String;
    shared
}

/// The owner's UAT (2026-09-30): three strikes of C3 on a STRING Part,
/// held or released between, ring one voice, as one guitar string
/// re-plucked; and neither a re-strike nor a steal of a loud C2 tail (by
/// a D2, as smooth a pluck) clicks.
#[test]
fn a_restrike_reuses_its_string_and_nothing_clicks() {
    let shared = string_part();
    let mut rig = Rig::full_pool();
    for held in [true, false, true] {
        rig.inst.handle(loud(48), &shared);
        rig.render(&shared);
        if !held {
            rig.inst.handle(off(0, 48), &shared);
        }
        rig.render(&shared);
    }
    assert_eq!(slots_of(&rig, 48).len(), 1, "one voice");
    assert_eq!(rig.inst.sounding(), 1);

    // A loud C2, re-struck ringing; then the pool filled and the C2's
    // tail, the oldest, stolen.
    let mut rig = Rig::full_pool();
    let mut out = Vec::new();
    rig.inst.handle(loud(36), &shared);
    string_blocks(&mut rig, &shared, 40, &mut out);
    rig.inst.handle(off(0, 36), &shared);
    string_blocks(&mut rig, &shared, 20, &mut out);
    rig.inst.handle(loud(36), &shared);
    string_blocks(&mut rig, &shared, 40, &mut out);
    rig.inst.handle(off(0, 36), &shared);
    // Part 2's ALGO INIT fills the rest, on its own bus.
    for n in 0..MAX_VOICES as u8 - 1 {
        rig.inst.handle(on(1, 60 + n), &shared);
    }
    string_blocks(&mut rig, &shared, 20, &mut out);
    assert_eq!(slots_of(&rig, 36).len(), 1);
    rig.inst.handle(loud(38), &shared);
    string_blocks(&mut rig, &shared, 20, &mut out);
    assert!(slots_of(&rig, 36).is_empty(), "the C2 stolen");
    // At the model's level, as the models' click checks are set; a block
    // before each event to 8 after.
    let g = chimera_core::dsp::modal::out_gain(chimera_core::dsp::modal::ResonatorMode::String);
    for (what, at) in [("re-strike", 60), ("steal", 120)] {
        let w: Vec<f32> = out[(at - 1) * BLOCK_SIZE..(at + 8) * BLOCK_SIZE]
            .iter()
            .map(|x| x / g)
            .collect();
        let c = common::clicks(&w);
        assert!(c.is_empty(), "{what}: {:?}", &c[..c.len().min(20)]);
    }
}
