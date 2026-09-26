//! The instrument audio path (instrument-core spec § Audio path): voices per
//! part, mono bus, constant-power pan and level into the part's DAC pair,
//! sends into the FX bus, FX return into DAC pair 1.

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::instrument::{AudioShared, DacOut, Instrument, pan_gains};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::part::{DacPair, PartMode};
use chimera_core::preset::{ChainType, Performance};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::fnv1a;

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};

const BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);

const SR: u32 = chimera_hal::SAMPLE_RATE;

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
    out: DacOut,
    scope: chimera_core::scope::ScopeWriter,
}

impl Rig {
    fn new() -> Self {
        Self {
            inst: Box::new(Instrument::new(SR, BUDGET)),
            fx: Box::new(FxBus::new()),
            out: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS],
            scope: common::scope_writer(),
        }
    }
    fn render(&mut self, shared: &AudioShared) -> &DacOut {
        self.inst
            .render(&mut self.fx, &mut self.out, shared, &mut self.scope);
        &self.out
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
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

/// The DAC pair gets the part's mono bus × pan gain × level.
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
    let (l, r) = lr(&rig.out[0]);
    assert!(peak(&bus) > 0.01);
    for i in 0..BLOCK_SIZE {
        assert_eq!(l[i], bus[i] * 0.5, "sample {i}");
        assert_eq!(r[i], 0.0);
    }
    assert_eq!(
        peak(&rig.out[1]) + peak(&rig.out[2]),
        0.0,
        "other pairs silent"
    );
}

/// Send/return: a Part on pair 3, panned hard left, with a reverb send puts
/// no dry signal on pair 1 — pair 1 carries exactly the FX return (the same
/// on both sides), which is silent until the plate's first reflection.
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
    for b in 0..120 {
        rig.render(&shared);
        let bus = *rig.inst.part_bus(0);
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], bus.map(|s| s * 0.5)];
        let mut ret = [0.0f32; BLOCK_SIZE];
        fx.process(&mut sends, &shared.fx, SR, &mut ret);
        let (l, r) = lr(&rig.out[0]);
        assert_eq!((l, r), (ret, ret), "block {b}: pair 1 is the return only");
        let (l3, r3) = lr(&rig.out[2]);
        assert_eq!(peak(&r3), 0.0, "block {b}: hard left");
        if b < 3_411 / BLOCK_SIZE {
            assert!(
                b < 2 || peak(&l3) > 0.01,
                "block {b}: the part sounds on pair 3"
            );
            assert_eq!(peak(&rig.out[0]), 0.0, "block {b}: no dry on pair 1");
        }
    }
    assert!(peak(&rig.out[0]) > 1e-4, "the wet return arrived");
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
    let shared = AudioShared::default(); // Pizza, release 0.3 s
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

#[test]
fn refused_notes_are_counted_and_silent() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    for p in 0..6 {
        shared.parts[p].mix.mode = PartMode::Mono;
        rig.inst.handle(on(p as u8, 60), &shared);
    }
    shared.parts[0].mix.mode = PartMode::Poly;
    shared.parts[0].mix.channel = MidiChannel::new(9).unwrap();
    rig.inst.handle(on(9, 72), &shared);
    assert_eq!(rig.inst.allocator().refused(), 1);
}

/// A performance-level render for the new goldens: `blocks` blocks, notes
/// on at block 0 and off at `blocks / 2`, every DAC sample hashed.
fn render_perf(perf: &Performance, notes: &[(u8, u8)], blocks: usize) -> Vec<f32> {
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
        for pair in rig.render(&shared) {
            all.extend_from_slice(pair);
        }
    }
    all
}

fn chord() -> Vec<f32> {
    render_perf(
        &Performance::new(),
        &[(0, 60), (0, 64), (0, 67), (0, 71)],
        200,
    )
}

fn two_parts() -> Vec<f32> {
    let mut perf = Performance::new();
    perf.parts[1].load_init(ChainType::Fm);
    perf.parts[1].mix.output = DacPair::P2;
    perf.parts[1].mix.pan = 0.5;
    render_perf(&perf, &[(0, 60), (1, 67)], 200)
}

fn reverb_send(send: f32) -> Vec<f32> {
    let mut perf = Performance::new();
    perf.fx.reverb.mix = 0.5;
    perf.fx.reverb.time = 0.7;
    perf.parts[0].mix.sends[2] = send;
    render_perf(&perf, &[(0, 60)], 300)
}

/// Recorded when the instrument path landed (plan Task 12). Re-record only
/// for an intended sound change:
///     GOLDEN_RECORD=1 cargo test -p chimera-core --test instrument_test -- --nocapture
const GOLDENS: &[(&str, u64)] = &[
    ("poly_chord", 0x9e1be15b748f4ab1),
    ("two_parts_two_pairs", 0xcfe8ed2b4c185e18),
    ("reverb_send_off", 0x25fa9f662d1acb99),
    ("reverb_send_on", 0x51da232bdad6e4d9), // re-recorded: FX returns wet-only
];

/// A named golden case: a case name paired with its render function.
type GoldenCase = (&'static str, fn() -> Vec<f32>);

#[test]
fn instrument_goldens_match() {
    let cases: [GoldenCase; 4] = [
        ("poly_chord", chord),
        ("two_parts_two_pairs", two_parts),
        ("reverb_send_off", || reverb_send(0.0)),
        ("reverb_send_on", || reverb_send(0.5)),
    ];
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for (name, render) in cases {
        let hash = fnv1a(&render());
        if record {
            println!("    (\"{name}\", 0x{hash:016x}),");
        } else if GOLDENS.iter().find(|g| g.0 == name).map(|g| g.1) != Some(hash) {
            failures.push(format!("{name}: 0x{hash:016x}"));
        }
    }
    assert!(
        failures.is_empty(),
        "instrument golden mismatch:\n{}",
        failures.join("\n")
    );
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
        fnv1a(&render_perf(&Performance::new(), &[(0, 60)], 300))
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

/// Review Focus: switching a held chord to a costlier Sound must not push
/// the pool over the CPU budget; the newest voices are cut.
#[test]
fn sound_change_mid_chord_stays_in_budget() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::params::{EngineType, ParamSnapshot};
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    for n in 0..6 {
        rig.inst.handle(on(0, 60 + n), &shared);
    }
    rig.render(&shared);
    assert_eq!(
        rig.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| !s.is_free())
            .count(),
        6
    );
    shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), 5);
    assert!(
        a.slots()
            .iter()
            .all(|s| s.is_free() || s.cost() == Voice::cost(EngineType::Modal))
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
        let mut rig = Rig::new();
        for k in 0..6 {
            rig.inst.handle(on(0, 60 + k), &shared); // voices 0..5, voice 0 oldest
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
    let mut pizza = AudioShared::default();
    pizza.parts[0].mix.mode = PartMode::Mono;
    let mut modal = pizza.clone();
    modal.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    for (after_off, held) in [n - 2, n - 1, n, n + 1]
        .into_iter()
        .flat_map(|a| [(a, 0), (a, 20)])
    {
        let mut rig = Rig::new();
        rig.inst.handle(on(0, 60), &pizza);
        for _ in 0..20 {
            rig.render(&pizza);
        }
        rig.inst.handle(off(0, 60), &pizza);
        for _ in 0..after_off {
            rig.render(&pizza);
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
/// comes again. Each note-off releases exactly the voice its channel
/// started, in either order, and both voices ring out and are freed.
#[test]
fn same_note_from_two_channels_releases_both_voices() {
    for ch4_first in [true, false] {
        let mut rig = Rig::new();
        let mut shared = AudioShared::default();
        rig.inst.handle(on(0, 60), &shared); // voice 0
        rig.render(&shared);
        shared.parts[0].mix.channel = MidiChannel::new(3).unwrap();
        rig.inst.handle(on(3, 60), &shared); // voice 1
        rig.render(&shared);
        let slots = rig.inst.allocator().slots();
        assert_eq!([slots[0].part(), slots[1].part()], [Some(0), Some(0)]);
        let (first, second) = if ch4_first { (3, 0) } else { (0, 3) };
        rig.inst.handle(off(first, 60), &shared);
        let held: Vec<bool> = rig.inst.allocator().slots()[..2]
            .iter()
            .map(|s| s.held())
            .collect();
        assert_eq!(
            held,
            if ch4_first {
                [true, false]
            } else {
                [false, true]
            },
            "only {first}'s voice released"
        );
        rig.inst.handle(off(second, 60), &shared);
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
