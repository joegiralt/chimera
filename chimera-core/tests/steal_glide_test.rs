// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! STEAL = GLIDE (#254, ADR 0065): a note that steals a sounding voice of
//! its own Part and model glides the ring to its pitch, as a Prophet's
//! glide, and strikes it anew on the way; CUT strikes at the new pitch.

mod common;

use chimera_core::dsp::modal::{ResonatorMode, out_gain};
use chimera_core::dsp::note_to_freq;
use chimera_core::hw::{MAX_VOICES, SampleBudget};
use chimera_core::instrument::AudioShared;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot, Steal};
use chimera_core::project::PartId;
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

use common::InstRig as Rig;

impl Rig {
    /// A budget with room for one voice of Part 1's Sound beside the bus
    /// and two Parts' knobs: a note of any Part steals.
    fn starved(shared: &AudioShared) -> Self {
        use chimera_core::dsp::fx_bus::FxBus;
        use chimera_core::dsp::knobs::Knobs;
        use chimera_core::dsp::lfo::LfoClock;
        let p = &shared.parts[0];
        let cost = chimera_core::dsp::voice::Voice::cost(&p.params, &p.mod_state).0;
        let reserved = FxBus::COST.0 + Knobs::RESERVE.0 + LfoClock::RESERVE.0;
        let need = (reserved + cost + 2 * Knobs::COST.0) as u64;
        Self::with_budget(SampleBudget::for_cpu(
            (need * 4_800_000).div_ceil(chimera_core::hw::AUDIO_BUDGET_PERCENT as u64) as u32,
        ))
    }

    /// Rev V's budget, Part 1 capped by its MAX VOICES (`part`, `cap`):
    /// a note past the cap steals its own. A Part's cap, not a starved
    /// budget: the clear room is a real chip's (ADR 0311).
    fn capped() -> Self {
        Self::with_budget(SampleBudget::for_cpu(chimera_core::hw::CPU_HZ_REV_V))
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
            self.block(shared);
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
    // The Parts carry INIT's wheel route: LFO 1 at its rest.
    p.lfos[0].depth = 0.0;
    *shared.parts[0].params = p;
    cap(&mut shared, 1);
    shared
}

/// Part 1's MAX VOICES at `n` (ADR 0281): a note past it steals its own.
fn cap(shared: &mut AudioShared, n: u8) {
    use chimera_core::voice_limit::{Asked, MaxVoices, Reserve, VoiceCap};
    let max = MaxVoices::Cap(VoiceCap::new(n).expect("a cap of 1 to 8"));
    shared.parts[0].mix.set_voices(Asked {
        max,
        reserve: Reserve::NONE,
    });
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
    let mut rig = Rig::capped();
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
    *shared.parts[1].params = (*shared.parts[0].params).clone();
    shared.parts[1].mix.channel = MidiChannel::new(1).unwrap();
    // Room for one STRING voice in all (STRING clears nothing to pace):
    // Part 2's note can only take Part 1's.
    let mut rig = Rig::starved(&shared);
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
        s.iter().any(|s| s.part() == Some(PartId::ALL[1])),
        "Part 2 took the voice"
    );
    assert_eq!(
        s.iter().filter(|s| !s.is_free()).count(),
        1,
        "one voice in all: Part 2's note stole Part 1's"
    );
}

/// A glide steal, down or up, at TIME's slider 0 (1 ms), 0.3 (10 ms) and
/// INIT (150 ms), loses nothing it plays. Down, the loop lengthened faster
/// than it is written reads the ring's continuation, the old cycle, not
/// silence; either way the strike plucks the glide's longer line. Over each
/// window of the target's period of the first 40 blocks, it is no more than
/// 3.5 dB under the same steal at CUT (an instant glide up hears only the
/// pluck's youngest quarter of its line, −3.1 dB); it may be louder, as it adds to what
/// rings (SYMP's halo +3 dB, a bow on a ringing loop +6.5 dB in its first
/// window), but not by a burst's 9 dB. At TIME 0 no block is 20 dB under
/// CUT's. Reading a silent gap dipped 30 to 43 dB a block, with +13 dB
/// bursts; a pluck the old line's length down, 8 dB under, and the
/// target's length up, a pulse train 6 to 11 dB under.
#[test]
fn a_glide_steal_never_drops_out() {
    let cases = [
        ("STRING", ResonatorMode::String, (72, 36)),
        ("SYMP", ResonatorMode::Sympathetic, (72, 36)),
        ("BOWED", ResonatorMode::Bowed, (72, 36)),
        ("STRING", ResonatorMode::String, (G3, C3)),
        ("STRING", ResonatorMode::String, (36, 72)),
        ("SYMP", ResonatorMode::Sympathetic, (36, 72)),
        ("BOWED", ResonatorMode::Bowed, (36, 72)),
    ];
    let init = chimera_core::params::PitchParams::default().glide_time;
    for (name, mode, (from, to)) in cases {
        let period = SR as f32 / note_to_freq(to);
        let n = (period / BLOCK_SIZE as f32).ceil() as usize * BLOCK_SIZE;
        let (cut, _) = steal(
            &part(EngineType::Modal, Some(mode), Steal::Cut),
            (from, to),
            40,
        );
        for time in [0.0, 0.3, init] {
            let mut shared = part(EngineType::Modal, Some(mode), Steal::Glide);
            shared.parts[0].params.pitch.glide_time = time;
            let (out, _) = steal(&shared, (from, to), 40);
            let windows = out[BLOCK_SIZE..]
                .chunks_exact(n)
                .zip(cut[BLOCK_SIZE..].chunks_exact(n));
            for (k, (g, c)) in windows.enumerate() {
                let db = 20.0 * (common::rms(g) / common::rms(c)).log10();
                assert!(
                    (-3.5..=9.0).contains(&db),
                    "{name} {from}→{to} TIME {time}: window {k} {db:+.1} dB against CUT"
                );
            }
            if time == 0.0 {
                let blocks = out[BLOCK_SIZE..]
                    .as_chunks::<BLOCK_SIZE>()
                    .0
                    .iter()
                    .zip(cut[BLOCK_SIZE..].as_chunks::<BLOCK_SIZE>().0.iter());
                for (k, (g, c)) in blocks.enumerate() {
                    let db = 20.0 * (common::rms(g) / common::rms(c)).log10();
                    assert!(
                        db > -20.0,
                        "{name} {from}→{to}: block {k} {db:+.1} dB against CUT"
                    );
                }
            }
        }
    }
}

/// A fast glide down struck at EXCITE 0 adds no pluck: what sounds is the
/// old ring alone, carried round the lengthened loop. It never drops more
/// than 10 dB a block under the block before the steal; a zero-filled gap
/// dropped it 30 dB and more.
#[test]
fn a_fast_glide_down_carries_the_old_ring() {
    for (name, mode) in [
        ("STRING", ResonatorMode::String),
        ("SYMP", ResonatorMode::Sympathetic),
    ] {
        for time in [0.0, 0.3] {
            let mut shared = part(EngineType::Modal, Some(mode), Steal::Glide);
            shared.parts[0].params.pitch.glide_time = time;
            let mut rig = Rig::capped();
            let (mut out, mut ratios) = (Vec::new(), Vec::new());
            rig.inst.handle(on(0, 72), &shared);
            rig.run(&shared, 40, &mut out, &mut ratios);
            rig.inst.handle(off(0, 72), &shared);
            rig.run(&shared, 4, &mut out, &mut ratios);
            shared.parts[0].params.modal.excite = 0.0;
            rig.inst.handle(on(0, 36), &shared);
            let before = common::rms(&out[out.len() - BLOCK_SIZE..]);
            out.clear();
            rig.run(&shared, 40, &mut out, &mut ratios);
            for (k, b) in out.as_chunks::<BLOCK_SIZE>().0.iter().enumerate() {
                let db = 20.0 * (common::rms(b) / before).log10();
                assert!(db > -10.0, "{name} TIME {time}: block {k} {db:+.1} dB");
            }
        }
    }
}

/// Two SYMP glide steals of low notes, whose rings are whole, in one
/// block both glide: a glide clears nothing, so the block's clear budget,
/// one note-on's worst, takes both.
#[test]
fn two_symp_glide_steals_in_a_block_both_glide() {
    let mut shared = part(
        EngineType::Modal,
        Some(ResonatorMode::Sympathetic),
        Steal::Glide,
    );
    cap(&mut shared, 2);
    let mut rig = Rig::capped();
    let (mut out, mut ratios) = (Vec::new(), Vec::new());
    for n in [24, 31] {
        rig.inst.handle(on(0, n), &shared);
        rig.run(&shared, 2, &mut out, &mut ratios);
    }
    rig.run(&shared, 40, &mut out, &mut ratios);
    rig.inst.handle(off(0, 24), &shared);
    rig.inst.handle(off(0, 31), &shared);
    rig.run(&shared, 4, &mut out, &mut ratios);
    rig.inst.handle(on(0, 26), &shared);
    rig.inst.handle(on(0, 33), &shared);
    rig.run(&shared, 1, &mut out, &mut ratios);
    let (active, slides) = (rig.inst.active(), rig.inst.slides());
    let gliding = (0..MAX_VOICES)
        .filter(|&v| active[v] && slides[v] != 1.0)
        .count();
    assert_eq!(gliding, 2, "{slides:?}");
}
