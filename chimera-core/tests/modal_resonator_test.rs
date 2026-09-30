//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::block::ParamKind;
use chimera_core::dsp::modal::{
    BankModes, MODAL_SPECS, ModalEngine, ModalParams, ResonatorMode, reads,
};
use chimera_core::dsp::note_to_freq;
use chimera_core::hw::Cost;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{
    Rig, SR, assert_stable, clicks, fundamental_hz, play_modal, play_modal_at, play_modal_bare,
    rms_diff, routes,
};

const MODES: [ResonatorMode; 4] = [
    ResonatorMode::String,
    ResonatorMode::Modal,
    ResonatorMode::Bowed,
    ResonatorMode::Sympathetic,
];

/// A bow's steady limit cycle wobbles about 0.2 % between seconds; a
/// runaway grows far past this. Its render is deterministic (no noise).
const BOW_MARGIN: f32 = 1.005;

/// Each model, each setting at its min and max, C2 held 30 s: bounded, no
/// growth, no DC at the output; Bowed still sounding.
#[test]
fn every_model_is_stable_at_every_extreme() {
    let sr = SR as usize;
    std::thread::scope(|scope| {
        for mode in MODES {
            scope.spawn(move || {
                let margin = match mode {
                    ResonatorMode::Bowed => BOW_MARGIN,
                    _ => 1.001,
                };
                for s in MODAL_SPECS.iter().filter(|s| s.id != ModalParams::MODE) {
                    for (note, v) in [(36, s.min), (36, s.max)] {
                        let mut p = ModalParams {
                            mode,
                            ..Default::default()
                        };
                        p.set(s.id, v);
                        let out = play_modal(&p, note, 30 * sr / BLOCK_SIZE, 0);
                        let label = format!("{mode:?} {note} {} = {v}", s.label);
                        assert_stable(&out, 4.0, margin, &label);
                        // Bowed's C2 sounds throughout (#206), not freed.
                        if mode == ResonatorMode::Bowed {
                            let last = &out[out.len() - sr..];
                            assert!(common::rms(last) > 1e-3, "{label}: silent");
                        }
                    }
                }
            });
        }
    });
}

/// STRING and the SYMP main string (halo bare), G1 to C7, DAMP longest and
/// BRIGHT brightest, BODY off (its interim half-delay comb kills the odd
/// partials until Task 8): the fundamental within ±2 cents, and at G1 and C3
/// partials 2 to 4 within ±5 cents of its multiples. Whole-sample tuning
/// fails the first; a DC blocker in the loop, the second.
#[test]
fn strings_are_in_tune() {
    let blocks = 3 * SR as usize / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for mode in [ResonatorMode::String, ResonatorMode::Sympathetic] {
            scope.spawn(move || {
                let p = ModalParams {
                    mode,
                    damp: 1.0,
                    bright: 1.0,
                    body: 0.0,
                    ..Default::default()
                };
                for n in 31..=96 {
                    let out = match mode {
                        ResonatorMode::Sympathetic => play_modal_bare(&p, n, 100, blocks, 0),
                        _ => play_modal_at(&p, n, 100, blocks, 0),
                    };
                    let f0 = note_to_freq(n) as f64;
                    let s = &out[SR as usize / 4..SR as usize * 5 / 4];
                    let f1 = fundamental_hz(s, f0);
                    let cents = 1200.0 * (f1 / f0).log2();
                    assert!(cents.abs() < 2.0, "{mode:?} note {n}: {cents:+.2} cents");
                    // Harmonic, not just f0: partials 2..4 on multiples of it.
                    if matches!(n, 31 | 48) {
                        for k in 2..=4 {
                            let h = k as f64 * f1;
                            let off = 1200.0 * (fundamental_hz(s, h) / h).log2();
                            assert!(off.abs() < 5.0, "{mode:?} note {n} h{k}: {off:+.2} cents");
                        }
                    }
                }
            });
        }
    });
}

/// Held until Task 9: today's LFO moves the heads under a sample a second.
const INAUDIBLE_UNTIL_T9: &[(ResonatorMode, chimera_core::block::ParamId)] =
    &[(ResonatorMode::String, ModalParams::ENS_RATE)];

/// Per model, from every continuous setting at 0.5 and MODES at 32, note
/// 48 held 1 s: each setting at its min, middle and max. A setting the
/// model reads changes the sound; one it ignores changes no bit. The
/// middle, as POS's ends can null alike.
#[test]
fn live_knobs_move_dimmed_knobs_do_not() {
    let blocks = SR as usize / BLOCK_SIZE;
    for mode in MODES {
        let mut base = ModalParams {
            mode,
            modes: BankModes::M32,
            ..Default::default()
        };
        for s in MODAL_SPECS
            .iter()
            .filter(|s| s.kind == ParamKind::Continuous)
        {
            base.set(s.id, 0.5);
        }
        for s in MODAL_SPECS.iter().filter(|s| s.id != ModalParams::MODE) {
            if INAUDIBLE_UNTIL_T9.contains(&(mode, s.id)) {
                continue;
            }
            let [lo, mid, hi] = [s.min, s.quantize((s.min + s.max) * 0.5), s.max].map(|v| {
                let mut p = base;
                p.set(s.id, v);
                play_modal(&p, 48, blocks, 0)
            });
            let same =
                |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
            if reads(mode, s.id) {
                let moved = [(&lo, &mid), (&mid, &hi), (&lo, &hi)]
                    .iter()
                    .map(|(a, b)| rms_diff(a, b))
                    .fold(0.0, f32::max);
                assert!(moved > 1e-3, "{mode:?} {}: live but inaudible", s.label);
            } else {
                assert!(
                    same(&lo, &hi) && same(&lo, &mid),
                    "{mode:?} {}: dimmed but heard",
                    s.label
                );
            }
        }
    }
}

/// SYMP's STRUCTURE tunes the halo only: bare, it changes no bit of the
/// main string; with a halo, it changes the sound.
#[test]
fn symp_structure_tunes_only_the_halo() {
    let blocks = SR as usize / BLOCK_SIZE;
    let at = |structure: f32| ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure,
        ..Default::default()
    };
    let bare = [0.0, 1.0].map(|s| play_modal_bare(&at(s), 48, 100, blocks, 0));
    assert!(
        bare[0]
            .iter()
            .zip(&bare[1])
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "bare: STRUCTURE moved the main string"
    );
    let full = [0.0, 1.0].map(|s| play_modal(&at(s), 48, blocks, 0));
    assert!(
        rms_diff(&full[0], &full[1]) > 1e-3,
        "halo: STRUCTURE inaudible"
    );
}

const MACROS: [chimera_core::block::ParamId; 4] = [
    ModalParams::STRUCTURE,
    ModalParams::BRIGHT,
    ModalParams::DAMP,
    ModalParams::POS,
];

/// A strike's first blocks: the click detector skips them (F6).
const ATTACK_BLOCKS: usize = 8;

/// `p` through a voice: note 48 held `blocks`, plucked again at each of
/// `plucks`.
fn play_voice(p: &ParamSnapshot, mods: &ModState, blocks: usize, plucks: &[usize]) -> Vec<f32> {
    let mut rig = Rig::new(SR);
    let (note, vel) = (MidiNote::new(48).unwrap(), Velocity::new(100).unwrap());
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut block = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        if b == 0 || plucks.contains(&b) {
            rig.note_on(note, vel, p);
        }
        rig.render(&mut block, p, mods);
        out.extend_from_slice(&block);
    }
    out
}

/// LFO 1 on each macro, at full depth, moves every live model's sound
/// and adds no click past a strike. Plucked again just after 1 s, where
/// the LFO is near +0.5: POS is heard at a pluck, and its ends null alike.
/// The bank, at 48 modes, flags nothing on DAMP or POS, held at either end
/// or routed. Its high STRUCTURE and BRIGHT drive the output tanh and flag
/// by themselves (https://github.com/joegiralt/chimera/issues/231), so there
/// it may flag no more than the macro held at either end.
#[test]
fn macros_are_routable() {
    let second = SR as usize / BLOCK_SIZE;
    let pluck = second + second / 60;
    let strikes = [0, pluck];
    let flags = |out: &[f32]| -> Vec<(usize, f32)> {
        clicks(out)
            .into_iter()
            .filter(|&(i, _)| {
                let b = i / BLOCK_SIZE;
                !strikes.iter().any(|&s| (s..s + ATTACK_BLOCKS).contains(&b))
            })
            .collect()
    };
    for mode in [
        ResonatorMode::Modal,
        ResonatorMode::String,
        ResonatorMode::Sympathetic,
    ] {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        p.modal.modes = BankModes::M48;
        p.lfos[0].rate = 5.0;
        for id in MACROS {
            let addr = ParamAddr::new(BlockRef::Modal, id);
            let [dry, wet] =
                [0, 127].map(|a| play_voice(&p, &routes(addr, a), 2 * second, &[pluck]));
            let d = rms_diff(&dry, &wet);
            assert!(d > 1e-3, "{mode:?} {id:?}: the route changes nothing ({d})");
            let held = if mode == ResonatorMode::Modal {
                [0.0, 1.0]
                    .map(|v| {
                        let mut held = p.clone();
                        held.modal.set(id, v);
                        flags(&play_voice(&held, &ModState::new(), 2 * second, &[pluck])).len()
                    })
                    .into_iter()
                    .max()
                    .unwrap()
            } else {
                0
            };
            let hot = mode == ResonatorMode::Modal
                && (id == ModalParams::STRUCTURE || id == ModalParams::BRIGHT);
            assert!(hot || held == 0, "{mode:?} {id:?}: held, {held} clicks");
            let n = flags(&wet).len();
            assert!(
                n <= held,
                "{mode:?} {id:?}: routed, {n} clicks, {held} held"
            );
        }
    }
}

/// Review Focus 3: MODES latches at note-on. A ringing 48-mode note keeps
/// its 48 modes and its bill until it ends; the next note plays 16.
#[test]
fn modes_change_keeps_sounding_notes_and_their_bill() {
    let mut p48 = ParamSnapshot::for_engine(EngineType::Modal);
    p48.modal.mode = ResonatorMode::Modal;
    p48.modal.modes = BankModes::M48;
    let mut p16 = p48.clone();
    p16.modal.modes = BankModes::M16;
    let mods = ModState::new();
    let (note, vel) = (MidiNote::new(60).unwrap(), Velocity::new(100).unwrap());
    let run = |change: bool| {
        let mut rig = Rig::new(SR);
        rig.note_on(note, vel, &p48);
        let mut bits = Vec::new();
        let mut block = [0.0; BLOCK_SIZE];
        for b in 0..100 {
            let p = if change && b >= 50 { &p16 } else { &p48 };
            rig.render(&mut block, p, &mods);
            if b >= 50 {
                bits.extend(block.map(f32::to_bits));
                assert!(rig.is_active(), "block {b}: the note ended");
                let want = if change {
                    Cost(ModalEngine::COST_MODE.0 * 32)
                } else {
                    Cost::ZERO
                };
                assert_eq!(rig.held_model_extra(p), want, "block {b}");
            }
        }
        (bits, rig)
    };
    let (unchanged, _) = run(false);
    let (changed, mut rig) = run(true);
    assert!(
        changed == unchanged,
        "a MODES edit reached the sounding note"
    );
    rig.note_on(note, vel, &p16);
    let mut block = [0.0; BLOCK_SIZE];
    rig.render(&mut block, &p16, &mods);
    assert_eq!(rig.held_model_extra(&p16), Cost::ZERO);
    // Billed over a STRING edit: exactly the 16-mode bank's cost.
    let mut string = p16.clone();
    string.modal.mode = ResonatorMode::String;
    let over = ModalEngine::cost(&p16.modal).0 - ModalEngine::COST_STRING.0;
    assert_eq!(rig.held_model_extra(&string), Cost(over), "plays 16 modes");
}

/// `p` through a voice: `note` at `vel`, held `on` blocks, then released
/// `off` blocks, `off_params` rendered from the note-off on.
fn play_released(
    p: &ParamSnapshot,
    off_params: &ParamSnapshot,
    (note, vel): (u8, u8),
    on: usize,
    off: usize,
) -> (Vec<f32>, Rig) {
    let mut rig = Rig::new(SR);
    let mods = ModState::new();
    rig.note_on(MidiNote::new(note).unwrap(), Velocity::new(vel).unwrap(), p);
    let mut out = Vec::with_capacity((on + off) * BLOCK_SIZE);
    let mut block = [0.0; BLOCK_SIZE];
    for b in 0..on + off {
        if b == on {
            rig.note_off();
        }
        rig.render(&mut block, if b < on { p } else { off_params }, &mods);
        out.extend_from_slice(&block);
    }
    (out, rig)
}

/// Samples each side of a note-off that `release_does_not_click` weighs.
const CUT_W: usize = 128;

/// #51: note-off on every model, the DC blocker in place, passes the click
/// detector from a block before it on (F6: the bank's strike is not the
/// subject), and does not cut: the level just after it keeps half the
/// level just before, which the detector's absolute threshold misses at a
/// voice's level. The instant buffer scaling this replaces kept 9 to 29 %
/// and stepped the ring's DC (Task 2's carry); a 5 ms ramp barely starts.
#[test]
fn release_does_not_click() {
    let half = SR as usize / BLOCK_SIZE / 2;
    let off = half * BLOCK_SIZE;
    for mode in MODES {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        let (out, _) = play_released(&p, &p, (60, 127), half, half);
        let release = &out[off - BLOCK_SIZE..];
        assert!(
            clicks(release).is_empty(),
            "{mode:?}: {:?}",
            clicks(release)
        );
        let kept = common::rms(&out[off..off + CUT_W]) / common::rms(&out[off - CUT_W..off]);
        assert!(kept > 0.5, "{mode:?}: note-off keeps {kept}");
    }
}

/// #206: Bowed G1 sounds in its first block and is not freed while bowed.
#[test]
fn bowed_low_notes_sound() {
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = ResonatorMode::Bowed;
    let mut rig = Rig::new(SR);
    let mods = ModState::new();
    rig.note_on(MidiNote::new(31).unwrap(), Velocity::new(100).unwrap(), &p);
    let mut block = [0.0; BLOCK_SIZE];
    let blocks = 2 * SR as usize / BLOCK_SIZE;
    for b in 0..blocks {
        rig.render(&mut block, &p, &mods);
        if b == 0 {
            assert!(common::peak(&block) > 1e-3, "silent first block");
        }
        assert!(rig.is_active(), "freed at block {b}");
    }
    assert!(common::peak(&block) > 1e-3, "silent last block");
}

/// Review Focus 1: a route pushing DAMP to its top during a release does
/// not hold the note: the release never gives the gain back, so the note
/// ends within 2 s, as soon as with DAMP left alone.
#[test]
fn a_released_note_ends_with_damp_at_its_top() {
    let blocks = SR as usize / BLOCK_SIZE;
    let mut p = ParamSnapshot::for_engine(EngineType::Modal);
    p.modal.mode = ResonatorMode::String;
    let mut top = p.clone();
    top.modal.damp = 1.0;
    // Blocks from note-off until the voice ends.
    let ends = |off: &ParamSnapshot| {
        let (out, rig) = play_released(&p, off, (48, 100), blocks / 2, 2 * blocks);
        assert!(!rig.is_active(), "still sounding 2 s after note-off");
        out.chunks(BLOCK_SIZE)
            .rposition(|b| b.iter().any(|&x| x != 0.0))
            .unwrap()
            - blocks / 2
    };
    let (alone, pushed) = (ends(&p), ends(&top));
    assert!(
        pushed <= alone + 2,
        "DAMP at its top: {pushed} blocks, {alone} left alone"
    );
}

/// The owner's sitar rule (ADR 0054): a released SYMP note's halo rings
/// out on its own decay, after the main string has died, and the voice
/// keeps it until then.
#[test]
fn a_released_halo_outlasts_its_main_string() {
    let second = SR as usize / BLOCK_SIZE;
    let p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        ..Default::default()
    };
    let last_heard = |out: &[f32]| out.iter().rposition(|x| x.abs() > 1e-3).unwrap();
    let full = play_modal(&p, 60, second / 2, 2 * second);
    let bare = play_modal_bare(&p, 60, common::VEL, second / 2, 2 * second);
    let (f, b) = (last_heard(&full), last_heard(&bare));
    assert!(f > b + SR as usize / 20, "halo heard to {f}, main to {b}");
}

/// RMS in dB of `out`'s 0.1 s from `at` s.
fn db_at(out: &[f32], at: f32) -> f32 {
    let i = (at * SR as f32) as usize;
    20.0 * common::rms(&out[i..i + SR as usize / 10]).log10()
}

/// The owner's ruling: the halo gets no release. After note-off it decays
/// as it does held, within 10 %, over 0.5 to 1.5 s; the main string alone
/// is silent by 0.5 s. The halo is the full note less the bare one: the
/// halo never drives the main string, and at these levels the tanh is
/// linear.
#[test]
fn a_released_halo_rings_on_its_held_decay() {
    let second = SR as usize / BLOCK_SIZE;
    // DAMP for a 2 s main string, so the halo falls about 15 dB a second.
    let p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        damp: 0.62,
        ..Default::default()
    };
    let halo = |on: usize, off: usize| {
        let full = play_modal(&p, 60, on, off);
        let bare = play_modal_bare(&p, 60, common::VEL, on, off);
        let d: Vec<f32> = full.iter().zip(&bare).map(|(f, b)| f - b).collect();
        (d, bare)
    };
    let (held, _) = halo(3 * second, 0);
    let (released, main) = halo(second / 2, 5 * second / 2);
    let slope = |x: &[f32]| db_at(x, 2.0) - db_at(x, 1.0);
    let (h, r) = (slope(&held), slope(&released));
    assert!(h < -5.0, "held halo falls {h} dB");
    assert!((r / h - 1.0).abs() < 0.1, "released {r} dB, held {h} dB");
    let tail = &main[SR as usize..];
    assert!(
        common::peak(tail) < 1e-3,
        "main string {}",
        common::peak(tail)
    );
}

/// Bowed's lifted bow: the ring decays at the strings' release, silent
/// within 0.5 s of note-off at C2.
#[test]
fn a_released_bowed_c2_is_silent_within_half_a_second() {
    let second = SR as usize / BLOCK_SIZE;
    let p = ModalParams {
        mode: ResonatorMode::Bowed,
        ..Default::default()
    };
    let out = play_modal(&p, 36, second, second);
    assert!(
        common::peak(&out[SR as usize - BLOCK_SIZE..SR as usize]) > 1e-2,
        "bowed"
    );
    let tail = &out[SR as usize * 3 / 2..];
    assert!(common::peak(tail) < 1e-3, "{}", common::peak(tail));
}
