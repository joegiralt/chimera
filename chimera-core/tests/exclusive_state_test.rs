//! Switching on `SlotKind` (exclusive-state spec § 3): a change of engine
//! or of Modal MODE fades the Part's sounding voices and rebuilds each once
//! silent; idle voices switch at their next note; knobs never rebuild.
mod common;
use common::Rig;
use common::{SR, peak, scope_writer, tri};

use chimera_core::addr::{BlockRef, Blocks};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::{ModalParams, ResonatorMode};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, DAC_PAIRS, MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

type Block = [f32; BLOCK_SIZE];

const FADE_BLOCKS: usize = Voice::FADE as usize / BLOCK_SIZE;

fn modal(mode: ResonatorMode) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = mode;
    p
}

fn string() -> ParamSnapshot {
    modal(ResonatorMode::String)
}

fn sym() -> ParamSnapshot {
    modal(ResonatorMode::Sympathetic)
}

fn note(n: u8) -> MidiNote {
    MidiNote::new(n).unwrap()
}

fn vel() -> Velocity {
    Velocity::new(100).unwrap()
}

/// The largest |x[n] − x[n−1]|.
fn max_step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

fn render(v: &mut Rig, p: &ParamSnapshot) -> Block {
    let mut b = [0.0; BLOCK_SIZE];
    v.render(&mut b, p, &ModState::new());
    b
}

fn bits(b: &Block) -> [u32; BLOCK_SIZE] {
    b.map(f32::to_bits)
}

/// A voice sounding note `n` on `p`.
fn playing(n: u8, p: &ParamSnapshot) -> Rig {
    let mut v = Rig::new(SR);
    v.note_on(note(n), vel(), p);
    v
}

/// A fresh voice's first `blocks` blocks of note `n` on `p`.
fn fresh(n: u8, p: &ParamSnapshot, blocks: usize) -> Vec<Block> {
    let mut v = playing(n, p);
    (0..blocks).map(|_| render(&mut v, p)).collect()
}

/// `v`'s next `blocks` blocks on `p` equal a fresh voice's first ones.
fn assert_fresh(v: &mut Rig, n: u8, p: &ParamSnapshot, blocks: usize, what: &str) {
    for (i, want) in fresh(n, p, blocks).iter().enumerate() {
        assert_eq!(bits(&render(v, p)), bits(want), "{what}: block {i}");
    }
}

/// A held note whose Part switches: by the Sound alone, or with a new
/// note-on of the same key on the new Sound (a retrigger; before § 3 a
/// MODE edit rebuilt the ringing model there, unfaded).
#[test]
fn switch_never_clicks() {
    let cases = [
        (
            "Algo -> Modal",
            tri(),
            ParamSnapshot::for_engine(EngineType::Modal),
        ),
        ("String -> Sympathetic", string(), sym()),
    ];
    for (name, from, to) in cases {
        for retrigger in [false, true] {
            let what = &format!("{name}, retrigger {retrigger}");
            let mut v = playing(60, &from);
            let mut last = [0.0; BLOCK_SIZE];
            for _ in 0..8 {
                last = render(&mut v, &from);
            }
            let (s, a) = (max_step(&last), peak(&last));
            assert!(a > 0.0, "{what}: sounding before the switch");
            if retrigger {
                v.note_on(note(60), vel(), &to);
            }
            let mut fade = vec![last[BLOCK_SIZE - 1]];
            for _ in 0..FADE_BLOCKS {
                fade.extend_from_slice(&render(&mut v, &to));
            }
            // |Δ(x·g)| ≤ |Δx|·g + |x|·|Δg|, with g ≤ 1 and |Δg| = 1/FADE: the
            // signal's own slew plus the fade's slope. It assumes the fade
            // blocks' step and peak don't exceed block 8's.
            let bound = s + a / Voice::FADE as f32;
            let step = max_step(&fade);
            assert!(step <= bound, "{what}: step {step} > {bound}");
            assert_eq!(*fade.last().unwrap(), 0.0, "{what}: the fade ends silent");
            assert_fresh(&mut v, 60, &to, 8, what);
        }
    }
}

/// A MODE edit, then a new note on the ringing voice: it waits for the
/// fade, and nothing of the String note (its string, its noise) reaches
/// the Sympathetic one.
#[test]
fn a_mode_change_at_note_on_plays_like_a_fresh_voice() {
    let mut v = playing(57, &string());
    for _ in 0..40 {
        render(&mut v, &string());
    }
    let r = v.rebuilds();
    v.note_on(note(62), Velocity::new(110).unwrap(), &sym());
    assert_eq!(v.rebuilds(), r, "a sounding voice waits for its fade");
    for _ in 0..FADE_BLOCKS {
        render(&mut v, &sym());
    }
    assert_eq!(v.rebuilds(), r.wrapping_add(1));
    let mut f = Rig::new(SR);
    f.note_on(note(62), Velocity::new(110).unwrap(), &sym());
    for block in 0..20 {
        assert_eq!(
            bits(&render(&mut v, &sym())),
            bits(&render(&mut f, &sym())),
            "block {block}"
        );
    }
}

fn bank() -> ParamSnapshot {
    let mut p = modal(ResonatorMode::Modal);
    p.modal.excite = 1.0;
    p
}

/// The bank's burst (its noise, its filter) is the bank's own: a Bank
/// note after another model plays like a fresh voice's first Bank note.
#[test]
fn a_bank_note_after_another_model_plays_like_the_first() {
    // A note on `p` over the ringing one, and its fade.
    let retrigger = |v: &mut Rig, n, vel, p: &ParamSnapshot| {
        v.note_on(note(n), Velocity::new(vel).unwrap(), p);
        for _ in 0..FADE_BLOCKS {
            render(v, p);
        }
    };
    let mut v = playing(57, &bank());
    for _ in 0..10 {
        render(&mut v, &bank());
    }
    retrigger(&mut v, 57, 100, &string());
    for _ in 0..10 {
        render(&mut v, &string());
    }
    retrigger(&mut v, 60, 90, &bank());
    let mut f = Rig::new(SR);
    f.note_on(note(60), Velocity::new(90).unwrap(), &bank());
    for block in 0..20 {
        let (a, b) = (render(&mut v, &bank()), render(&mut f, &bank()));
        assert_eq!(bits(&a), bits(&b), "block {block}");
    }
}

/// A MODE edit inside the bank's burst ends the burst with the bank: the
/// silent note after it goes idle like any other.
#[test]
fn a_mode_change_mid_burst_still_goes_idle() {
    let mut v = playing(57, &bank());
    render(&mut v, &bank());
    let mut silent = string();
    silent.modal.excite = 0.0;
    v.note_on(note(57), vel(), &silent);
    // The fade, then idle after 11 silent blocks (Modal's silence counter).
    for _ in 0..FADE_BLOCKS + 12 {
        render(&mut v, &silent);
    }
    assert!(!v.is_active());
}

#[test]
fn idle_voice_switches_in_the_same_block() {
    let mut v = playing(60, &tri());
    v.note_off();
    for _ in 0..10_000 {
        if !v.is_active() {
            break;
        }
        render(&mut v, &tri());
    }
    assert!(!v.is_active(), "the tri note ends");
    let r = v.rebuilds();
    let modal = ParamSnapshot::for_engine(EngineType::Modal);
    for p in [&modal, &tri(), &sym()] {
        for _ in 0..3 {
            assert_eq!(render(&mut v, p), [0.0; BLOCK_SIZE]);
        }
    }
    assert_eq!(v.rebuilds(), r, "an idle voice never rebuilds");
    v.note_on(note(60), vel(), &sym());
    assert_eq!(v.rebuilds(), r.wrapping_add(1));
    assert_fresh(&mut v, 60, &sym(), 1, "idle -> Sympathetic");
}

fn instrument() -> (Box<Instrument>, Box<FxBus>) {
    let inst = Instrument::new(SR, SampleBudget::for_cpu(CPU_HZ_REV_V));
    (Box::new(inst), Box::new(FxBus::new()))
}

fn event(channel: u8, n: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(channel).unwrap(),
        note: note(n),
        kind,
    }
}

#[test]
fn other_parts_are_untouched_by_a_switch() {
    // Part 1 is silent on the DAC and sends nothing; Part 2 only sends. So
    // the DAC carries the FX return alone, fed by Part 2 alone: a switch
    // that touched the FX would change it.
    let mut shared = AudioShared::default();
    shared.parts[0].params = tri();
    shared.parts[0].mix.level = 0.0;
    shared.parts[1].params = string();
    shared.parts[1].mix.level = 0.0;
    shared.parts[1].mix.sends = [0.5; 3];
    shared.fx.reverb.mix = 0.5;
    let mut switched = shared.clone();
    switched.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);

    let run = |switch: bool| {
        let (mut inst, mut fx) = instrument();
        let mut scope = scope_writer();
        let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
        let (mut bus, mut outs) = (Vec::new(), Vec::new());
        for b in 0..60 {
            let s = if switch && b >= 21 {
                &switched
            } else {
                &shared
            };
            if b == 0 {
                inst.handle(event(0, 60, NoteKind::On(vel())), s);
                inst.handle(event(1, 64, NoteKind::On(vel())), s);
            }
            if b == 20 {
                inst.handle(event(0, 60, NoteKind::Off), s);
                inst.handle(event(1, 64, NoteKind::Off), s);
            }
            inst.render(&mut fx, &mut dac, s, &mut scope);
            bus.push(bits(inst.part_bus(1)));
            outs.push(dac.map(|pair| pair.map(f32::to_bits)));
        }
        (bus, outs)
    };
    let (bus_a, dac_a) = run(false);
    let (bus_b, dac_b) = run(true);
    assert!(bus_a.iter().flatten().any(|&s| s != 0), "Part 2 sounds");
    for b in 0..60 {
        assert_eq!(bus_a[b], bus_b[b], "Part 2's bus, block {b}");
        assert_eq!(dac_a[b], dac_b[b], "the FX return, block {b}");
    }
    assert!(
        dac_b[22]
            .iter()
            .flatten()
            .any(|&s| f32::from_bits(s) != 0.0),
        "the reverb tail rings on past the switch"
    );
}

#[test]
fn model_switch_rebuilds_once() {
    let mut v = playing(60, &string());
    for _ in 0..8 {
        render(&mut v, &string());
    }
    let r = v.rebuilds();
    for _ in 0..FADE_BLOCKS {
        render(&mut v, &sym());
    }
    assert_fresh(&mut v, 60, &sym(), 8, "String -> Sympathetic");
    assert_eq!(v.rebuilds(), r.wrapping_add(1));
}

#[test]
fn knob_moves_never_rebuild() {
    let run = |moved: bool| {
        let mut p = sym();
        let mut v = playing(60, &p);
        let r = v.rebuilds();
        let mut out = Vec::new();
        for b in 0..60 {
            if moved {
                let delta = if b % 2 == 0 { 1 } else { -1 };
                for blk in BlockRef::ALL {
                    let Some(k) = p.block_mut(blk) else { continue };
                    for s in blk.specs() {
                        if (blk, s.id) != (BlockRef::Modal, ModalParams::MODE) {
                            k.nudge(s.id, delta);
                        }
                    }
                }
            }
            out.push(bits(&render(&mut v, &p)));
            assert!(v.is_active(), "moved {moved}: block {b}");
        }
        assert_eq!(v.rebuilds(), r, "moved {moved}");
        out
    };
    assert_ne!(run(true), run(false), "the knobs reach the ringing note");
}

#[test]
fn a_switch_back_mid_fade_restarts_on_the_sounds_kind() {
    let mut v = playing(60, &string());
    for _ in 0..8 {
        render(&mut v, &string());
    }
    let r = v.rebuilds();
    render(&mut v, &sym());
    for _ in 1..FADE_BLOCKS {
        render(&mut v, &string());
    }
    assert_fresh(&mut v, 60, &string(), 8, "String -> Sympathetic -> String");
    assert_eq!(v.rebuilds(), r.wrapping_add(1));
}

/// A fresh voice's first 8 sounding blocks of note `n` on `p`.
fn first_sounding(n: u8, p: &ParamSnapshot) -> Vec<Block> {
    let mut v = playing(n, p);
    let mut b = render(&mut v, p);
    for _ in 0..100 {
        if b.iter().any(|&s| s != 0.0) {
            break;
        }
        b = render(&mut v, p);
    }
    let mut out = vec![b];
    out.extend((1..8).map(|_| render(&mut v, p)));
    out
}

#[test]
fn a_steal_across_kinds_plays_the_new_kind_clean() {
    // Instrument: Part 2's note steals one of Part 1's voices.
    let mut shared = AudioShared::default();
    shared.parts[0].params = tri();
    shared.parts[1].params = sym();
    let (mut inst, mut fx) = instrument();
    let mut scope = scope_writer();
    let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
    for n in 0..MAX_VOICES as u8 {
        inst.handle(event(0, 60 + n, NoteKind::On(vel())), &shared);
    }
    inst.render(&mut fx, &mut dac, &shared, &mut scope);
    inst.handle(event(1, 50, NoteKind::On(vel())), &shared);
    let mut blocks = 0;
    loop {
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        if inst.part_bus(1).iter().any(|&s| s != 0.0) {
            break;
        }
        blocks += 1;
        assert!(blocks < 100, "Part 2 sounds");
    }
    for (i, want) in first_sounding(50, &sym()).iter().enumerate() {
        if i > 0 {
            inst.render(&mut fx, &mut dac, &shared, &mut scope);
        }
        assert_eq!(
            bits(inst.part_bus(1)),
            bits(want),
            "Part 2's bus, block {i}"
        );
    }

    // Voice: the same steal, and its rebuild bound.
    let mut v = playing(60, &tri());
    for _ in 0..8 {
        render(&mut v, &tri());
    }
    let r = v.rebuilds();
    v.kill();
    for _ in 0..FADE_BLOCKS + 1 {
        if !v.is_active() {
            break;
        }
        render(&mut v, &tri());
    }
    assert!(!v.is_active(), "the killed voice fades out");
    v.note_on(note(50), vel(), &sym());
    assert_eq!(v.rebuilds(), r.wrapping_add(2), "fade end, then the note");
    assert_fresh(&mut v, 50, &sym(), 8, "steal -> Sympathetic");
}

/// The per-block bound (ADR 0051): a voice rebuilds at most three times
/// between two blocks, the drain before `Instrument::render` included.
/// Worst case, all on one voice: an idle trigger across kinds (1); a key
/// up; a steal by another Part, the pool full and this the oldest tail;
/// the fade ends at once, the engine silent in its first block (2); the
/// waiting note, across kinds again (3).
#[test]
fn a_voice_rebuilds_at_most_three_times_a_block() {
    let mut silent = tri();
    for o in &mut silent.algo.ops {
        o.level = 0; // quiet from its first block: the fade ends there
    }
    let mut shared = AudioShared::default();
    shared.parts[0].params = silent;
    shared.parts[1].params = sym();
    shared.parts[2].params = tri();
    let mut inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(u32::MAX)));
    let mut fx = Box::new(FxBus::new());
    let mut scope = scope_writer();
    let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
    // Seven held tri notes; the eighth voice ends idle on String.
    for n in 0..MAX_VOICES as u8 - 1 {
        inst.handle(event(2, 60 + n, NoteKind::On(vel())), &shared);
    }
    inst.handle(event(1, 40, NoteKind::On(vel())), &shared);
    inst.render(&mut fx, &mut dac, &shared, &mut scope);
    inst.handle(event(1, 40, NoteKind::Off), &shared);
    shared.parts[1].params = string();
    for _ in 0..FADE_BLOCKS + 1 {
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
    }
    let v = MAX_VOICES - 1;
    assert!(
        inst.allocator().slots()[v].is_free(),
        "the String voice is idle"
    );

    let before = inst.rebuilds();
    inst.handle(event(0, 30, NoteKind::On(vel())), &shared); // String -> Algo
    inst.handle(event(0, 30, NoteKind::Off), &shared);
    inst.handle(event(1, 50, NoteKind::On(vel())), &shared); // steals it
    inst.render(&mut fx, &mut dac, &shared, &mut scope); // fade end, Algo -> String
    let after = inst.rebuilds();
    let delta: Vec<u16> = (0..MAX_VOICES)
        .map(|i| after[i].wrapping_sub(before[i]))
        .collect();
    assert_eq!(delta[v], 3, "{delta:?}");
    assert_eq!(delta.iter().sum::<u16>(), 3, "{delta:?}");
    assert_eq!(inst.allocator().slots()[v].note(), Some(note(50)));
}

/// The same bound under a storm: random notes on four Parts whose Sounds
/// change kind at random, every voice's rebuilds counted per block.
#[test]
fn a_switch_storm_never_rebuilds_a_voice_more_than_three_times_a_block() {
    let mut silent = tri();
    for o in &mut silent.algo.ops {
        o.level = 0;
    }
    let sounds = [silent, tri(), string(), sym()];
    let mut rng = 0x2545_f491_u32;
    let mut next = |n: u32| {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng % n
    };
    let mut shared = AudioShared::default();
    let mut inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(u32::MAX)));
    let mut fx = Box::new(FxBus::new());
    let mut scope = scope_writer();
    let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
    let mut worst = 0;
    for _ in 0..3000 {
        let before = inst.rebuilds();
        for part in &mut shared.parts[..4] {
            if next(8) == 0 {
                part.params = sounds[next(4) as usize].clone();
            }
        }
        for _ in 0..next(6) {
            let (ch, n) = (next(4) as u8, 48 + next(12) as u8);
            let kind = if next(2) == 0 {
                NoteKind::On(vel())
            } else {
                NoteKind::Off
            };
            inst.handle(event(ch, n, kind), &shared);
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        let after = inst.rebuilds();
        for v in 0..MAX_VOICES {
            let d = after[v].wrapping_sub(before[v]);
            assert!(d <= 3, "voice {v}: {d} rebuilds in a block");
            worst = worst.max(d);
        }
    }
    assert!(worst >= 2, "the storm switches: worst {worst}");
}
