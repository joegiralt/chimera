//! Switching on `SlotKind` (exclusive-state spec § 3): a change of engine
//! or of Modal MODE fades the Part's sounding voices and rebuilds each once
//! silent; idle voices switch at their next note; knobs never rebuild.
mod common;
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

fn render(v: &mut Voice, p: &ParamSnapshot) -> Block {
    let mut b = [0.0; BLOCK_SIZE];
    v.render(&mut b, p, &ModState::new());
    b
}

fn bits(b: &Block) -> [u32; BLOCK_SIZE] {
    b.map(f32::to_bits)
}

/// A voice sounding note `n` on `p`.
fn playing(n: u8, p: &ParamSnapshot) -> Voice {
    let mut v = Voice::new(SR);
    v.note_on(note(n), vel(), p);
    v
}

/// A fresh voice's first `blocks` blocks of note `n` on `p`.
fn fresh(n: u8, p: &ParamSnapshot, blocks: usize) -> Vec<Block> {
    let mut v = playing(n, p);
    (0..blocks).map(|_| render(&mut v, p)).collect()
}

/// `v`'s next `blocks` blocks on `p` equal a fresh voice's first ones.
fn assert_fresh(v: &mut Voice, n: u8, p: &ParamSnapshot, blocks: usize, what: &str) {
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
            let bound = s + a / Voice::FADE as f32;
            let step = max_step(&fade);
            assert!(step <= bound, "{what}: step {step} > {bound}");
            assert_eq!(*fade.last().unwrap(), 0.0, "{what}: the fade ends silent");
            assert_fresh(&mut v, 60, &to, 8, what);
        }
    }
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
    let mut shared = AudioShared::default();
    shared.parts[0].params = tri();
    shared.parts[1].params = string();
    for part in &mut shared.parts[..2] {
        part.mix.sends = [0.5; 3];
    }
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
            outs.push(dac);
        }
        (bus, outs)
    };
    let (bus_a, _) = run(false);
    let (bus_b, dac_b) = run(true);
    assert!(bus_a.iter().flatten().any(|&s| s != 0), "Part 2 sounds");
    for (b, (x, y)) in bus_a.iter().zip(&bus_b).enumerate() {
        assert_eq!(x, y, "Part 2's bus, block {b}");
    }
    assert!(
        dac_b[22].iter().flatten().any(|&s| s != 0.0),
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
