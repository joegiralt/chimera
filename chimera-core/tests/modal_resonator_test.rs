//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::block::ParamKind;
use chimera_core::dsp::modal::{
    BankModes, MODAL_SPECS, ModalEngine, ModalParams, RELEASE_T60, ResonatorMode, damp_for, reads,
};
use chimera_core::dsp::note_to_freq;
use chimera_core::hw::Cost;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{
    Rig, SR, assert_stable, clicks, fundamental_hz, goertzel, play_modal, play_modal_at,
    play_modal_bare, rms_diff, routes,
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
/// growth, no DC at the output; Bowed still sounding while it bows.
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
                        // Bowed's C2 sounds throughout (#206), not freed,
                        // unless the bow has no pressure or no motion.
                        let still =
                            v == 0.0 && (s.id == ModalParams::FORCE || s.id == ModalParams::SPEED);
                        if mode == ResonatorMode::Bowed && !still {
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

/// Per model, from every continuous setting at 0.5 and MODES at 32, note
/// 48 held 1 s and released 0.5 s, so a lifted bow's DAMP is heard: each
/// setting at its min, middle and max. A setting the
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
            let [lo, mid, hi] = [s.min, s.quantize((s.min + s.max) * 0.5), s.max].map(|v| {
                let mut p = base;
                p.set(s.id, v);
                play_modal(&p, 48, blocks, blocks / 2)
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
    play_voice_at(48, p, mods, blocks, plucks)
}

/// `play_voice` at `note`.
fn play_voice_at(
    note: u8,
    p: &ParamSnapshot,
    mods: &ModState,
    blocks: usize,
    plucks: &[usize],
) -> Vec<f32> {
    let mut rig = Rig::new(SR);
    let (note, vel) = (MidiNote::new(note).unwrap(), Velocity::new(100).unwrap());
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

/// LFO 1 on each macro a model reads, at full depth, moves its sound and
/// adds no click past a strike; Bowed's DAMP is routed through a release. Plucked again just after 1 s, where
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
    for mode in MODES {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        p.modal.modes = BankModes::M48;
        p.lfos[0].rate = 5.0;
        for id in MACROS.into_iter().filter(|&id| reads(mode, id)) {
            // Bowed's DAMP is its ring after the lift: heard through a release.
            if mode == ResonatorMode::Bowed && id == ModalParams::DAMP {
                let addr = ParamAddr::new(BlockRef::Modal, id);
                let [dry, wet] = [0, 127].map(|a| {
                    let mut rig = Rig::new(SR);
                    let mods = routes(addr, a);
                    let v = Velocity::new(100).unwrap();
                    rig.note_on(MidiNote::new(48).unwrap(), v, &p);
                    let mut out = Vec::new();
                    let mut block = [0.0; BLOCK_SIZE];
                    for b in 0..2 * second {
                        if b == second {
                            rig.note_off();
                        }
                        rig.render(&mut block, &p, &mods);
                        out.extend_from_slice(&block);
                    }
                    out
                });
                let d = rms_diff(&dry, &wet);
                assert!(d > 1e-3, "Bowed DAMP: the route changes nothing ({d})");
                assert!(clicks(&wet).is_empty(), "Bowed DAMP: {:?}", clicks(&wet));
                continue;
            }
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
    let over = ModalEngine::cost(&p16.modal).0 - ModalEngine::cost(&string.modal).0;
    assert_eq!(rig.held_model_extra(&string), Cost(over), "plays 16 modes");
}

/// BODY and the ensemble latch at note-on, and so does their bill: an edit
/// that turns them off under a sounding note bills them until it ends, and
/// one that turns them on bills them at once, from the params.
#[test]
fn body_and_ensemble_keep_their_bill() {
    let mods = ModState::new();
    let (note, vel) = (MidiNote::new(60).unwrap(), Velocity::new(100).unwrap());
    for mode in [ResonatorMode::String, ResonatorMode::Sympathetic] {
        let mut on = ParamSnapshot::for_engine(EngineType::Modal);
        (on.modal.mode, on.modal.body, on.modal.ens_mix) = (mode, 0.3, 0.5);
        let mut off = on.clone();
        (off.modal.body, off.modal.ens_mix) = (0.0, 0.0);
        let mut rig = Rig::new(SR);
        rig.note_on(note, vel, &on);
        let mut block = [0.0; BLOCK_SIZE];
        rig.render(&mut block, &off, &mods);
        assert_eq!(
            rig.held_model_extra(&off),
            ModalEngine::BODY + ModalEngine::ENSEMBLE,
            "{mode:?}"
        );
        assert_eq!(rig.held_model_extra(&on), Cost::ZERO, "{mode:?}");
        // Turned on mid-note: the note plays without them, and is billed
        // for them anyway.
        let mut rig = Rig::new(SR);
        rig.note_on(note, vel, &off);
        rig.render(&mut block, &on, &mods);
        assert_eq!(rig.held_model_extra(&on), Cost::ZERO, "{mode:?}");
        assert_eq!(
            ModalEngine::cost(&on.modal),
            ModalEngine::cost(&off.modal) + ModalEngine::BODY + ModalEngine::ENSEMBLE,
            "{mode:?}"
        );
    }
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

/// Bowed's lifted bow at the v1 ring, `RELEASE_T60`: silent within 0.5 s
/// of note-off at C2. At INIT's DAMP it rings about 14 s.
#[test]
fn a_released_bowed_c2_is_silent_within_half_a_second() {
    let second = SR as usize / BLOCK_SIZE;
    let p = ModalParams {
        mode: ResonatorMode::Bowed,
        damp: damp_for(RELEASE_T60),
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

/// A second of `p` at `note`, 0.25 s in: the fundamental's cents from
/// the note, and the measured f0. Below G1 the loop clamps near G1's
/// length, so it is measured, and in cents, from G1.
fn f0_cents(p: &ModalParams, note: u8) -> (f64, f64) {
    let out = play_modal(p, note, 3 * SR as usize / BLOCK_SIZE / 2, 0);
    let f0 = note_to_freq(note.max(31)) as f64;
    let f1 = fundamental_hz(&out[SR as usize / 4..SR as usize * 5 / 4], f0);
    (1200.0 * (f1 / f0).log2(), f1)
}

/// STRUCTURE 0 to 1 on STRING, A0 to C6: the fundamental holds within
/// 2 cents, the dispersion's delay at f0 taken off the line. G1 at 1 is
/// the line's longest chain; below it, the clamped loop holds too.
#[test]
fn dispersion_keeps_pitch() {
    for note in [21, 24, 30, 31, 36, 60, 84] {
        let at = |structure: f32| ModalParams {
            structure,
            bright: 0.0,
            damp: 1.0,
            ..Default::default()
        };
        let (c0, _) = f0_cents(&at(0.0), note);
        let (c1, _) = f0_cents(&at(1.0), note);
        assert!(
            (c1 - c0).abs() < 2.0,
            "note {note}: {c0:+.2} → {c1:+.2} cents"
        );
        if note >= 31 {
            assert!(c1.abs() < 2.0, "note {note}: {c1:+.2} cents at STRUCTURE 1");
        }
    }
}

/// BODY colours on the output: C2 and C3, at the INIT's 0.3 and at 1,
/// sound the note, not its octave (#10).
#[test]
fn body_does_not_transpose() {
    for note in [36, 48] {
        for body in [0.0, 0.3, 1.0] {
            let p = ModalParams {
                body,
                ..Default::default()
            };
            let (c, _) = f0_cents(&p, note);
            assert!(c.abs() < 2.0, "note {note} BODY {body}: {c:+.2} cents");
            // What repeats, not only the strongest line near the note.
            let out = play_modal(&p, note, 3 * SR as usize / BLOCK_SIZE, 0);
            let f = common::period_hz(&out[SR as usize / 4..]);
            let off = 1200.0 * (f / note_to_freq(note) as f64).log2();
            assert!(
                off.abs() < 2.0,
                "note {note} BODY {body}: repeats at {f} Hz"
            );
        }
    }
}

/// STRUCTURE stiffens STRING: at 1 its 8th partial sits more than 5
/// cents sharp of 8·f0, at 0 within 1 cent of it.
#[test]
fn dispersion_stretches_the_partials() {
    let cents = |structure: f32| {
        let p = ModalParams {
            structure,
            bright: 1.0,
            damp: 1.0,
            body: 0.0,
            ..Default::default()
        };
        let (_, f1) = f0_cents(&p, 48);
        let out = play_modal(&p, 48, 3 * SR as usize / BLOCK_SIZE / 2, 0);
        let s = &out[SR as usize / 4..SR as usize * 5 / 4];
        let h = 8.0 * f1;
        1200.0 * (fundamental_hz(s, h) / h).log2()
    };
    let (off, on) = (cents(0.0), cents(1.0));
    assert!(off.abs() < 1.0, "STRUCTURE 0: 8th partial {off:+.2} cents");
    assert!(on > 5.0, "STRUCTURE 1: 8th partial {on:+.2} cents");
}

/// The largest second difference past the first second: a step's size.
fn kink(out: &[f32]) -> f32 {
    out[SR as usize..]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0, f32::max)
}

/// A square LFO swings STRUCTURE end to end on G1, the longest chain.
/// Each step glides the chain's delay 2 samples a block (`DISP_SLEW`), so
/// at BRIGHT 1 nothing clicks past the strike; at BRIGHT 0, where a step
/// would show, the kinks stay within 3× a held STRUCTURE's (unglided,
/// 4.2×).
#[test]
fn a_structure_step_at_g1_does_not_click() {
    let second = SR as usize / BLOCK_SIZE;
    let addr = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
    for bright in [1.0, 0.0] {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
        p.modal.structure = 0.5;
        p.modal.bright = bright;
        p.modal.damp = 1.0;
        p.lfos[0].rate = 2.0;
        p.lfos[0].shape = chimera_core::dsp::lfo::LfoShape::Square as u8;
        let routed = play_voice_at(31, &p, &routes(addr, 127), 2 * second, &[]);
        let held = [0.0, 1.0].map(|s| {
            let mut h = p.clone();
            h.modal.structure = s;
            play_voice_at(31, &h, &ModState::new(), 2 * second, &[])
        });
        assert!(
            rms_diff(&routed, &held[0]) > 1e-3,
            "the route changes nothing"
        );
        let n = clicks(&routed)
            .into_iter()
            .filter(|&(i, _)| i / BLOCK_SIZE >= ATTACK_BLOCKS)
            .count();
        assert_eq!(n, 0, "BRIGHT {bright}: {n} clicks");
        let (k, base) = (kink(&routed), kink(&held[0]).max(kink(&held[1])));
        assert!(k <= 3.0 * base, "BRIGHT {bright}: kink {k}, held {base}");
    }
}

/// ENS on (DEPTH 1, MIX 0.5) against off (MIX 0), STRING note 48 at ENS
/// RATE 0.5 (≈ 0.77 Hz), 4 s: energy spreads to ±10 cents of f0, and the
/// level moves at the LFO's rate.
#[test]
fn ensemble_is_audible() {
    let blocks = 4 * SR as usize / BLOCK_SIZE;
    let [on, off] = [0.5, 0.0].map(|ens_mix| {
        let p = ModalParams {
            mode: ResonatorMode::String,
            damp: 1.0,
            bright: 0.7,
            ens_depth: 1.0,
            ens_rate: 0.5,
            ens_mix,
            ..Default::default()
        };
        play_modal(&p, 48, blocks, 0)
    });
    let f0 = note_to_freq(48);
    let sideband = |x: &[f32]| {
        let g = |cents: f32| goertzel(x, f0 * 2f32.powf(cents / 1200.0), SR);
        20.0 * (0.5 * (g(10.0) + g(-10.0)) / g(0.0)).log10()
    };
    let spread = sideband(&on) - sideband(&off);
    assert!(spread > 6.0, "sidebands up {spread:.1} dB");

    // The 10 ms RMS envelope over seconds 1..4, in dB, less its line.
    let wobble = |x: &[f32]| {
        let w = SR as usize / 100;
        let db: Vec<f64> = x[SR as usize..4 * SR as usize]
            .chunks(w)
            .map(|c| 20.0 * (common::rms(c) as f64).max(1e-9).log10())
            .collect();
        let n = db.len() as f64;
        let mx = (n - 1.0) / 2.0;
        let my = db.iter().sum::<f64>() / n;
        let sxy: f64 = db
            .iter()
            .enumerate()
            .map(|(i, y)| (i as f64 - mx) * (y - my))
            .sum();
        let sxx: f64 = (0..db.len()).map(|i| (i as f64 - mx).powi(2)).sum();
        let slope = sxy / sxx;
        let var = db
            .iter()
            .enumerate()
            .map(|(i, y)| (y - my - slope * (i as f64 - mx)).powi(2))
            .sum::<f64>()
            / n;
        var.sqrt()
    };
    let (w_on, w_off) = (wobble(&on), wobble(&off));
    assert!(
        w_on > 3.0 * w_off,
        "level moves {w_on:.3} dB vs {w_off:.3} dB"
    );
}

/// Review Focus 5: DEPTH 1, MIX 1 on G1's longest loop, at STRUCTURE 0 and
/// 1 (the chain takes some of the line), at 0.1 Hz and 6 Hz, 30 s: the
/// heads stay in the line, no clicks, bounded.
#[test]
fn ensemble_at_full_depth_on_g1_stays_in_the_line() {
    let blocks = 30 * SR as usize / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for structure in [0.0, 1.0] {
            for ens_rate in [0.0, 1.0] {
                scope.spawn(move || {
                    let p = ModalParams {
                        mode: ResonatorMode::String,
                        damp: 1.0,
                        structure,
                        ens_depth: 1.0,
                        ens_rate,
                        ens_mix: 1.0,
                        ..Default::default()
                    };
                    let out = play_modal(&p, 31, blocks, 0);
                    let label = format!("STRUCTURE {structure} ENS RATE {ens_rate}");
                    assert!(out.iter().all(|x| x.is_finite()), "{label}: finite");
                    let peak = out.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                    assert!(peak <= 1.5, "{label}: peak {peak}");
                    let c = clicks(&out);
                    assert!(c.is_empty(), "{label}: clicks {:?}", &c[..c.len().min(5)]);
                });
            }
        }
    });
}

/// The fundamental at MIX 0.5 is the dry note's as DEPTH leaves 0, and
/// stays near it at a moderate DEPTH: the heads read in phase with the
/// dry, not half a period back (an octave-up comb).
#[test]
fn ensemble_keeps_the_fundamental() {
    let blocks = 3 * SR as usize / BLOCK_SIZE;
    for note in [48, 31] {
        let h1 = |ens_depth: f32| {
            let p = ModalParams {
                mode: ResonatorMode::String,
                damp: 1.0,
                bright: 0.7,
                ens_depth,
                ens_rate: 0.5,
                ens_mix: 0.5,
                ..Default::default()
            };
            let out = play_modal(&p, note, blocks, 0);
            let s = &out[SR as usize / 2..5 * SR as usize / 2];
            20.0 * goertzel(s, note_to_freq(note), SR).log10()
        };
        let (zero, nudge, some) = (h1(0.0), h1(0.001), h1(0.35));
        assert!(
            (nudge - zero).abs() < 0.5,
            "{note}: DEPTH 0.001 {nudge:.1} dB vs {zero:.1}"
        );
        assert!(
            (some - zero).abs() < 3.0,
            "{note}: DEPTH 0.35 {some:.1} dB vs {zero:.1}"
        );
    }
}

/// A PITCH route retunes the line under the heads each block: at DEPTH 1,
/// either rate, no click past the attack.
#[test]
fn a_pitch_route_with_the_ensemble_on_does_not_click() {
    let second = SR as usize / BLOCK_SIZE;
    let pitch = ParamAddr::new(BlockRef::Pitch, chimera_core::params::PitchParams::PITCH);
    for (ens_rate, ens_mix) in [(0.0, 0.5), (1.0, 0.5), (0.0, 1.0), (1.0, 1.0)] {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
        p.modal.damp = 1.0;
        (p.modal.ens_depth, p.modal.ens_rate, p.modal.ens_mix) = (1.0, ens_rate, ens_mix);
        p.lfos[0].rate = 5.0;
        for note in [31, 48] {
            let out = play_voice_at(note, &p, &routes(pitch, 32), 2 * second, &[]);
            let n = clicks(&out)
                .into_iter()
                .filter(|&(i, _)| i / BLOCK_SIZE >= ATTACK_BLOCKS)
                .count();
            assert_eq!(
                n, 0,
                "note {note} RATE {ens_rate} MIX {ens_mix}: {n} clicks"
            );
        }
    }
}

/// Note 48's period, samples.
fn period_of(note: u8) -> f32 {
    SR as f32 / note_to_freq(note)
}

/// SYMP note 48 held on chord 1; at block 100 STRUCTURE crosses to chord
/// 3. The halo glides, doesn't click, and 25 ms on sits on chord 3.
#[test]
fn chord_change_glides() {
    use chimera_core::dsp::modal::{CHORDS, SymPool, fold};
    let mut p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure: 0.1,
        damp: 1.0,
        ..Default::default()
    };
    let mut pool = SymPool::boxed();
    let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
    e.note_on(48, 100, &p, SR, &mut pool);
    let to = CHORDS[3].map(|st| fold(period_of(48) * 2f32.powf(-st / 12.0)));
    let mut out = Vec::new();
    let mut block = [0.0; BLOCK_SIZE];
    for i in 0..200 {
        if i == 100 {
            p.structure = 0.3;
        }
        e.render(&mut block, &p, SR, &mut pool);
        out.extend_from_slice(&block);
        if i == 100 {
            // Gliding, not stepped: string 4 goes 9.99 to 13.99.
            let (a, b) = (fold(period_of(48) * 2f32.powf(-9.99 / 12.0)), to[4]);
            let got = e.halo_periods(&pool).expect("a halo")[4];
            assert!(got < a - 1e-3 && got > b + 1e-3, "{got}: {a} to {b}");
        }
        if i >= 100 + 19 {
            let got = e.halo_periods(&pool).expect("a halo");
            for (g, t) in got.iter().zip(&to) {
                assert!((g - t).abs() < 1e-3, "block {i}: {got:?} vs {to:?}");
            }
        }
    }
    let c = clicks(&out);
    assert!(c.is_empty(), "clicks {:?}", &c[..c.len().min(5)]);
}

/// Review Focus 2: on G1 every chord's halo fits the line. Each string
/// plays its interval folded up by the least octaves that fit (−12 is
/// unison), on a ring sized for its longest chord, and 2 s ring finite
/// and bounded. A clamped line would miss its period.
#[test]
fn every_chord_fits_the_line_at_g1() {
    use chimera_core::dsp::modal::{CHORDS, MAX_STRING_DELAY, SymPool};
    let note_period = period_of(31) as f64;
    for (k, chord) in CHORDS.iter().enumerate() {
        let p = ModalParams {
            mode: ResonatorMode::Sympathetic,
            structure: (k as f32 + 0.5) / 11.0,
            damp: 1.0,
            ..Default::default()
        };
        let mut pool = SymPool::boxed();
        let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
        e.note_on(31, 127, &p, SR, &mut pool);
        let mut block = [0.0; BLOCK_SIZE];
        let mut peak = 0.0_f32;
        for i in 0..2 * SR as usize / BLOCK_SIZE {
            e.render(&mut block, &p, SR, &mut pool);
            assert!(block.iter().all(|x| x.is_finite()), "chord {k}: finite");
            peak = block.iter().fold(peak, |m, x| m.max(x.abs()));
            if i > 0 {
                continue;
            }
            let lines = e.halo_lines(&pool).expect("a halo");
            let periods = e.halo_periods(&pool).expect("a halo");
            let folded = |st: f32| {
                let mut p = note_period * 2f64.powf(-st as f64 / 12.0);
                while (p - 0.5).floor() > (MAX_STRING_DELAY - 2) as f64 {
                    p /= 2.0;
                }
                p
            };
            for (s, (&(_, ring), &st)) in lines.iter().zip(chord).enumerate() {
                // The ring is sized at note-on for the longest line any chord
                // gives this string: no glide grows it.
                let longest = CHORDS
                    .iter()
                    .map(|c| (folded(c[s]) - 0.5).floor() as usize + 2)
                    .max()
                    .unwrap();
                assert_eq!(ring, longest, "chord {k} string {s}");
                let want = folded(st);
                let got = periods[s] as f64;
                assert!(
                    (got - want).abs() < 1e-3,
                    "chord {k} string {s}: {got} vs {want}"
                );
            }
            assert!(
                (periods[0] as f64 - note_period).abs() < 1e-3,
                "−12 is unison"
            );
        }
        assert!(peak <= 2.0, "chord {k}: peak {peak}");
    }
}

/// A chord step on a low note, halo loud: the glide's sharpest kink stays
/// within 2× the held chords' either side. A re-split each block (64
/// samples) gave 2.9× here, and ticked in the demo's click check.
#[test]
fn a_chord_glide_on_a_low_note_does_not_tick() {
    use chimera_core::dsp::modal::SymPool;
    let at = |k: usize| (k as f32 + 0.5) / 11.0;
    let mut p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure: at(0),
        damp: 0.97,
        bright: 0.5,
        couple: 0.5,
        halo: 0.5,
        ..Default::default()
    };
    let mut pool = SymPool::boxed();
    let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
    e.note_on(38, 100, &p, SR, &mut pool);
    let mut out = Vec::new();
    let mut block = [0.0; BLOCK_SIZE];
    for i in 0..210 {
        if i == 150 {
            p.structure = at(5);
        }
        e.render(&mut block, &p, SR, &mut pool);
        out.extend_from_slice(&block);
    }
    let kink = |b: std::ops::Range<usize>| {
        out[b.start * BLOCK_SIZE..b.end * BLOCK_SIZE]
            .windows(3)
            .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
            .fold(0.0, f32::max)
    };
    let (glide, held) = (kink(150..166), kink(110..150).max(kink(166..206)));
    assert!(glide <= 2.0 * held, "glide kink {glide}, held {held}");
    assert!(clicks(&out[8 * BLOCK_SIZE..]).is_empty());
}

/// A pitch change mid-note retunes the halo to where a note-on at that
/// pitch puts it.
#[test]
fn a_pitch_change_retunes_the_halo() {
    use chimera_core::dsp::modal::SymPool;
    let p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        ..Default::default()
    };
    let r = 2f32.powf(7.0 / 12.0);
    let halo = |before: bool| {
        let mut pool = SymPool::boxed();
        let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
        if before {
            e.set_pitch(r);
        }
        e.note_on(36, 100, &p, SR, &mut pool);
        e.set_pitch(r);
        let mut block = [0.0; BLOCK_SIZE];
        e.render(&mut block, &p, SR, &mut pool);
        e.halo_periods(&pool).expect("a halo")
    };
    let (at_note_on, moved) = (halo(true), halo(false));
    for (a, b) in at_note_on.iter().zip(&moved) {
        assert!((a - b).abs() < 1e-3, "{at_note_on:?} vs {moved:?}");
    }
}

/// A note's first block takes its modulated STRUCTURE whole: a route
/// (here the stored chord 0, rendered on chord 5) puts the halo on the
/// routed chord at once, not 20 ms on.
#[test]
fn a_routed_chord_snaps_on_the_first_block() {
    use chimera_core::dsp::modal::{CHORDS, SymPool, fold};
    let stored = ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure: 0.5 / 11.0,
        ..Default::default()
    };
    let routed = ModalParams {
        structure: 5.5 / 11.0,
        ..stored
    };
    let mut pool = SymPool::boxed();
    let mut e = Box::new(ModalEngine::new_in(&mut pool, stored.mode));
    e.note_on(48, 100, &stored, SR, &mut pool);
    let mut block = [0.0; BLOCK_SIZE];
    e.render(&mut block, &routed, SR, &mut pool);
    let want = CHORDS[5].map(|st| fold(period_of(48) * 2f32.powf(-st / 12.0)));
    let got = e.halo_periods(&pool).expect("a halo");
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-3, "{got:?} vs {want:?}");
    }
}

/// A note's first block takes its routed STRUCTURE whole on STRING too:
/// the chain and line are where a note-on at the routed value puts them,
/// not 65 ms on. A VEL route arrives as the block's modulated params.
#[test]
fn a_routed_stiffness_snaps_on_the_first_block() {
    use chimera_core::dsp::modal::SymPool;
    let stored = ModalParams {
        mode: ResonatorMode::String,
        structure: 0.0,
        ..Default::default()
    };
    let routed = ModalParams {
        structure: 1.0,
        ..stored
    };
    let line = |at_on: &ModalParams| {
        let mut pool = SymPool::boxed();
        let mut e = Box::new(ModalEngine::new_in(&mut pool, stored.mode));
        e.note_on(31, 100, at_on, SR, &mut pool);
        let mut block = [0.0; BLOCK_SIZE];
        e.render(&mut block, &routed, SR, &mut pool);
        e.string_line().expect("a string")
    };
    let (want, got) = (line(&routed), line(&stored));
    assert_eq!(want.0, 1.0);
    assert_eq!(got, want);
}

/// A pitch change mid-glide moves the glide's end, not where the strings
/// are: the periods run on continuously from the step before.
#[test]
fn a_pitch_change_mid_glide_does_not_jump() {
    use chimera_core::dsp::modal::SymPool;
    let mut p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        structure: 0.5 / 11.0,
        ..Default::default()
    };
    let mut pool = SymPool::boxed();
    let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
    e.note_on(48, 100, &p, SR, &mut pool);
    let mut block = [0.0; BLOCK_SIZE];
    e.render(&mut block, &p, SR, &mut pool);
    p.structure = 5.5 / 11.0;
    for _ in 0..7 {
        e.render(&mut block, &p, SR, &mut pool);
    }
    // Halfway through the glide: the step each block is about 1/15 of it.
    let (a, b) = (e.halo_periods(&pool).unwrap(), {
        e.render(&mut block, &p, SR, &mut pool);
        e.halo_periods(&pool).unwrap()
    });
    let step = |x: &[f32; 7], y: &[f32; 7]| {
        x.iter()
            .zip(y)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    };
    let held = step(&a, &b);
    // A semitone up: the glide's end moves 6 %, the strings one step.
    e.set_pitch(2f32.powf(1.0 / 12.0));
    e.render(&mut block, &p, SR, &mut pool);
    let c = e.halo_periods(&pool).unwrap();
    assert!(step(&b, &c) <= 1.5 * held, "{} vs {held}", step(&b, &c));
}

/// The bench: a soft key on Bowed at the defaults bowed nothing. The
/// force follows velocity from half FORCE, so velocity 20 sounds and
/// holds for 2 s.
#[test]
fn a_soft_bowed_note_sounds() {
    let p = ModalParams {
        mode: ResonatorMode::Bowed,
        ..Default::default()
    };
    let out = play_modal_at(&p, 48, 20, 2 * SR as usize / BLOCK_SIZE, 0);
    let sr = SR as usize;
    let held = common::rms(&out[sr..2 * sr]);
    assert!(held > 1e-2, "seconds 1–2: rms {held}");
    let last = common::peak(&out[out.len() - BLOCK_SIZE..]);
    assert!(last > 1e-3, "last block: peak {last}");
}

/// Bowed's DAMP is the ring after the lift: the held bow is the same bit
/// for bit; released, it falls about 30 dB in half DAMP's T60.
#[test]
fn bowed_damp_is_the_ring_after_the_lift() {
    let second = SR as usize / BLOCK_SIZE;
    let at = |damp: f32| {
        let p = ModalParams {
            mode: ResonatorMode::Bowed,
            damp,
            ..Default::default()
        };
        play_modal_at(&p, 48, 100, second, 2 * second)
    };
    let (a, b) = (at(0.3), at(0.6));
    let held = SR as usize;
    assert!(
        a[..held]
            .iter()
            .zip(&b[..held])
            .all(|(x, y)| x.to_bits() == y.to_bits()),
        "DAMP moved the held bow"
    );
    // DAMP's law (`params::t60`): 0.30 s and 1.82 s.
    let t60 = |damp: f32| 0.05 * 400f32.powf(damp);
    for (damp, out) in [(0.3, &a), (0.6, &b)] {
        let fall = db_at(out, 1.05) - db_at(out, 1.05 + t60(damp) / 2.0);
        assert!((fall - 30.0).abs() < 6.0, "DAMP {damp}: {fall} dB");
    }
}

/// Bowed's POS and BRIGHT change the sound, not the pitch: within 2 cents
/// of POS 0 at BRIGHT 1, the old bow. A friction reading a second tap
/// bowed a second, shorter loop: 93 Hz for 65 at POS 0.3.
#[test]
fn bowed_pos_and_bright_keep_pitch() {
    let blocks = 3 * SR as usize / BLOCK_SIZE / 2;
    let render = |pos: f32, bright: f32| {
        let p = ModalParams {
            mode: ResonatorMode::Bowed,
            pos,
            bright,
            ..Default::default()
        };
        play_modal_at(&p, 48, 100, blocks, 0)
    };
    // Today's bow sounds an octave below its note (pinned by
    // `a_v1_bowed_patch_bows_as_before`): measured there.
    let f0 = note_to_freq(48) as f64 / 2.0;
    let span = |o: &[f32]| o[SR as usize / 2..SR as usize * 3 / 2].to_vec();
    let base = render(0.0, 1.0);
    let f_base = fundamental_hz(&span(&base), f0);
    for pos in [0.0, 0.3, 0.7] {
        for bright in [0.0, 1.0] {
            if pos == 0.0 && bright == 1.0 {
                continue;
            }
            let out = render(pos, bright);
            let f = fundamental_hz(&span(&out), f0);
            let cents = 1200.0 * (f / f_base).log2();
            assert!(
                cents.abs() < 2.0,
                "POS {pos} BRIGHT {bright}: {cents:+.2} cents"
            );
            let d = rms_diff(&out, &base);
            assert!(d > 1e-3, "POS {pos} BRIGHT {bright}: unchanged ({d})");
        }
    }
}

/// FORCE and SPEED are read every block and eased: turned on a held bow,
/// the sound moves within 8 blocks (the loop's delay is about 6 at C3),
/// and without a click: no sample steps further than the bow's own corner
/// does, held from note-on at either setting.
#[test]
fn force_and_speed_move_a_held_bow() {
    use chimera_core::dsp::modal::SymPool;
    let second = SR as usize / BLOCK_SIZE;
    let turn = second / 2;
    let base = ModalParams {
        mode: ResonatorMode::Bowed,
        ..Default::default()
    };
    let play_from = |first: &ModalParams, late: &ModalParams| {
        let mut pool = SymPool::boxed();
        let mut e = Box::new(ModalEngine::new_in(&mut pool, base.mode));
        e.note_on(48, 100, first, SR, &mut pool);
        let mut out = Vec::new();
        let mut block = [0.0; BLOCK_SIZE];
        for b in 0..second {
            e.render(
                &mut block,
                if b < turn { first } else { late },
                SR,
                &mut pool,
            );
            out.extend_from_slice(&block);
        }
        out
    };
    let play = |late: &ModalParams| play_from(&base, late);
    // The largest step the desktop's output stage hears (`common::clicks`).
    let step = |x: &[f32]| {
        let soft = |x: f32| libm::tanhf(x * 0.4);
        x.windows(2)
            .map(|w| (soft(w[1]) - soft(w[0])).abs())
            .fold(0.0, f32::max)
    };
    let still = play(&base);
    let (at, few) = (turn * BLOCK_SIZE, (turn + 8) * BLOCK_SIZE);
    for (label, late) in [
        ("FORCE", ModalParams { force: 1.0, ..base }),
        ("SPEED", ModalParams { speed: 1.0, ..base }),
    ] {
        let moved = play(&late);
        assert!(
            still[..at]
                .iter()
                .zip(&moved[..at])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{label}: moved before the turn"
        );
        let d = rms_diff(&still[at..few], &moved[at..few]);
        assert!(d > 1e-3, "{label}: unheard within 8 blocks ({d})");
        let steady = play_from(&late, &late);
        let corner = step(&still[at..]).max(step(&steady[at..]));
        let turned = step(&moved[at - BLOCK_SIZE..]);
        assert!(
            turned <= corner * 1.05,
            "{label}: steps {turned}, the bow's corner {corner}"
        );
    }
}
