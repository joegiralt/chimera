//! Modal 2's resonators (spec 2026-09-29-modal-2-resonators § Tests).
mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::block::ParamKind;
use chimera_core::dsp::modal::{
    BankModes, MODAL_SPECS, ModalEngine, ModalParams, ResonatorMode, damp_for, reads,
};
use chimera_core::dsp::note_to_freq;
use chimera_core::hw::Cost;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::{
    Rig, SR, assert_stable_at, at_model_level, clicks, fundamental_hz, goertzel, octave_clear,
    play_modal, play_modal_at, play_modal_bare, rms_diff, routes,
};

const MODES: [ResonatorMode; 4] = [
    ResonatorMode::String,
    ResonatorMode::Modal,
    ResonatorMode::Bowed,
    ResonatorMode::Sympathetic,
];

/// A bow's steady limit cycle wobbles a little between seconds; a
/// runaway grows far past this. Measured over whole periods
/// (`assert_stable_at`). Its render is deterministic (no noise).
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
                        assert_stable_at(&out, note_to_freq(note), 4.0, margin, &label);
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
/// model reads changes the sound, over the note or its first 0.25 s; one it ignores changes no bit. The
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
                // Over the note, or its first quarter second: BRIGHT on a
                // string moves the tone, not the fundamental's ring.
                let q = SR as usize / 4;
                let moved = [(&lo, &mid), (&mid, &hi), (&lo, &hi)]
                    .iter()
                    .map(|(a, b)| rms_diff(a, b).max(rms_diff(&a[..q], &b[..q])))
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
    at_model_level(out, p.modal.mode)
}

/// LFO 1 on each macro a model reads, at full depth, moves its sound and
/// adds no click past a strike; Bowed's DAMP is routed through a release. Plucked again just after 1 s, where
/// the LFO is near +0.5: POS is heard at a pluck, and its ends null alike.
/// The bank, at 48 modes, flags nothing on DAMP or POS, held at either end
/// or routed. Its high STRUCTURE and BRIGHT drive the output tanh and flag
/// by themselves (https://github.com/joegiralt/chimera/issues/231), so there
/// it may flag no more than the macro held at either end. So may Bowed's
/// BRIGHT: open, its output is the bow's sawtooth, a step each period.
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
                    at_model_level(out, mode)
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
            // The bow's open output is its Helmholtz sawtooth, a step a
            // period: at BRIGHT 1 it flags held, as the bank's tanh does.
            let bow_bright = mode == ResonatorMode::Bowed && id == ModalParams::BRIGHT;
            let held = if mode == ResonatorMode::Modal || bow_bright {
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
            let hot = bow_bright
                || mode == ResonatorMode::Modal
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
    (at_model_level(out, p.modal.mode), rig)
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

/// The owner's sitar rule (ADR 0054): a released SYMP note's halo rings
/// out on its own decay, after the main string has died, and the voice
/// keeps it until then.
#[test]
fn a_released_halo_outlasts_its_main_string() {
    let second = SR as usize / BLOCK_SIZE;
    let p = ModalParams {
        mode: ResonatorMode::Sympathetic,
        damp: 0.5,
        ..Default::default()
    };
    let last_heard = |out: &[f32]| out.iter().rposition(|x| x.abs() > 1e-3).unwrap();
    let full = play_modal(&p, 60, second / 2, 4 * second);
    let bare = play_modal_bare(&p, 60, common::VEL, second / 2, 4 * second);
    let (f, b) = (last_heard(&full), last_heard(&bare));
    assert!(f > b + SR as usize / 20, "halo heard to {f}, main to {b}");
}

/// RMS in dB of `out`'s 0.1 s from `at` s.
fn db_at(out: &[f32], at: f32) -> f32 {
    let i = (at * SR as f32) as usize;
    20.0 * common::rms(&out[i..i + SR as usize / 10]).log10()
}

/// The owner's ruling: the halo gets no release. After note-off it decays
/// as it does held, within 10 %, over 1 to 2 s. The halo is the full
/// note less the bare one: the
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
        full.iter()
            .zip(&bare)
            .map(|(f, b)| f - b)
            .collect::<Vec<f32>>()
    };
    let held = halo(3 * second, 0);
    let released = halo(second / 2, 5 * second / 2);
    let slope = |x: &[f32]| db_at(x, 2.0) - db_at(x, 1.0);
    let (h, r) = (slope(&held), slope(&released));
    assert!(h < -5.0, "held halo falls {h} dB");
    assert!((r / h - 1.0).abs() < 0.1, "released {r} dB, held {h} dB");
}

/// Bowed's lifted bow at the v1 release's 0.12 s ring: silent within
/// 0.5 s of note-off at C2. At INIT's DAMP it rings about 14 s.
#[test]
fn a_released_bowed_c2_is_silent_within_half_a_second() {
    let second = SR as usize / BLOCK_SIZE;
    let p = ModalParams {
        mode: ResonatorMode::Bowed,
        damp: damp_for(0.12),
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

/// The owner's UAT (2026-09-30): the halo glides as a Prophet's glide
/// does. SYMP note 48 held on chord 1; at block 100 STRUCTURE crosses to
/// chord 3, string 4 from 9.99 to 13.99 semitones. Its pitch moves one
/// way only, reaches 90 % of the interval 150 to 250 ms on, lands, and
/// nothing clicks.
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
    let from = fold(period_of(48) * 2f32.powf(-9.99 / 12.0));
    let mut out = Vec::new();
    let mut block = [0.0; BLOCK_SIZE];
    let mut path = Vec::new();
    for i in 0..900 {
        if i == 100 {
            p.structure = 0.3;
        }
        e.render(&mut block, &p, SR, &mut pool);
        out.extend_from_slice(&block);
        if i >= 100 {
            path.push(e.halo_periods(&pool).expect("a halo")[4]);
        }
    }
    assert!(path.windows(2).all(|w| w[1] <= w[0]), "one way");
    let share = |x: f32| (x / from).log2() / (to[4] / from).log2();
    let at90 = path.iter().position(|&x| share(x) >= 0.9).expect("90 %");
    let ms = (at90 * BLOCK_SIZE) as f32 * 1000.0 / SR as f32;
    assert!((150.0..=250.0).contains(&ms), "90 % at {ms} ms");
    let got = e.halo_periods(&pool).expect("a halo");
    for (g, t) in got.iter().zip(&to) {
        assert!((g - t).abs() < 1e-3, "{got:?} vs {to:?}");
    }
    let c = clicks(&out);
    assert!(c.is_empty(), "clicks {:?}", &c[..c.len().min(5)]);
}

/// Low, a halo string's interval folds up an octave in one chord and not
/// the next. Every step from chord to chord, on C1 and G1, moves each
/// string by the table's interval where that fits the line, else the other
/// way round the octave: never the long way round the fold.
#[test]
fn a_chord_glide_never_crosses_an_octave() {
    use chimera_core::dsp::modal::{CHORD_COUNT, CHORDS, SymPool, fold};
    for note in [24u8, 31] {
        for k in 0..CHORD_COUNT - 1 {
            let at = |k: usize| (k as f32 + 0.5) / CHORD_COUNT as f32;
            let mut p = ModalParams {
                mode: ResonatorMode::Sympathetic,
                structure: at(k),
                ..Default::default()
            };
            let mut pool = SymPool::boxed();
            let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
            e.note_on(note, 100, &p, SR, &mut pool);
            let mut block = [0.0; BLOCK_SIZE];
            e.render(&mut block, &p, SR, &mut pool);
            let before = e.halo_periods(&pool).unwrap();
            p.structure = at(k + 1);
            for _ in 0..2 * SR as usize / BLOCK_SIZE {
                e.render(&mut block, &p, SR, &mut pool);
            }
            let after = e.halo_periods(&pool).unwrap();
            for s in 0..7 {
                let moved = 12.0 * (before[s] / after[s]).log2();
                let want = CHORDS[k + 1][s] - CHORDS[k][s];
                let direct = before[s] * 2f32.powf(-want / 12.0);
                let want = if fold(direct) == direct {
                    want
                } else {
                    want - 12.0 * want.signum()
                };
                assert!(
                    (moved - want).abs() < 0.01,
                    "note {note}, chord {k} to {}, string {s}: moved {moved}, table {want}",
                    k + 1
                );
            }
        }
    }
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
/// within 2× the held chords' either side. The 20 ms linear glide's
/// re-split each block gave 2.9× here, and ticked in the demo's click
/// check; the 80 ms one-pole's steps, a quarter of its largest, do not.
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
/// routed chord at once, not glided.
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
/// holds for 2 s. At INIT's POS 0, the bow at the bridge, a light bow may
/// play a surface sound, as a real one does: only its level is held. On a
/// v1 patch's bow it plays its note.
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
    // A v1 patch's bow, soft: as loud, and at its note.
    let out = play_modal_at(&v1_bowed(), 48, 20, 2 * SR as usize / BLOCK_SIZE, 0);
    let s = &out[sr..2 * sr];
    let v1 = common::rms(s);
    assert!(v1 > 1e-2, "v1, seconds 1–2: rms {v1}");
    let f0 = note_to_freq(48);
    let c = cents(fundamental_hz(s, f0 as f64), f0);
    assert!(c.abs() < BOW_CENTS, "v1: {c:+.2} cents");
    assert!(octave_clear(s, f0), "v1: an octave low");
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
    let step = common::step;
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

/// The v1 Bowed patch's macros (`translate_v1`): the bow an eighth of the
/// string from the bridge, a moderately lossy bridge.
fn v1_bowed() -> ModalParams {
    ModalParams {
        mode: ResonatorMode::Bowed,
        force: 0.5,
        speed: 0.5,
        pos: 0.15,
        bright: 0.5,
        damp: damp_for(0.5),
        ..Default::default()
    }
}

/// Bowed's tuning gate, cents: a bow's stick-slip moves its pitch a few
/// cents, as a real one's does (controller ruling, spec § 2 BOWED).
const BOW_CENTS: f64 = 5.0;

/// `f` in cents from `f0`.
fn cents(f: f64, f0: f32) -> f64 {
    1200.0 * (f / f0 as f64).log2()
}

/// Seconds 0.5 to 1.5.
fn steady(out: &[f32]) -> &[f32] {
    &out[SR as usize / 2..SR as usize * 3 / 2]
}

/// #240: G1 to C7 within `BOW_CENTS` of its note, not an octave low; at
/// G1 and C3 partials 2 to 4 on multiples of it.
#[test]
fn bowed_is_in_tune() {
    let blocks = 2 * SR as usize / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for notes in [31..=52, 53..=74, 75..=96] {
            scope.spawn(move || {
                for n in notes {
                    let out = play_modal_at(&v1_bowed(), n, 100, blocks, 0);
                    let s = steady(&out);
                    let f0 = note_to_freq(n);
                    let f1 = fundamental_hz(s, f0 as f64);
                    let c = cents(f1, f0);
                    assert!(c.abs() < BOW_CENTS, "note {n}: {c:+.2} cents");
                    assert!(octave_clear(s, f0), "note {n}: an octave low");
                    if matches!(n, 31 | 48) {
                        for k in 2..=4 {
                            let h = k as f64 * f1;
                            let off = 1200.0 * (fundamental_hz(s, h) / h).log2();
                            assert!(off.abs() < 5.0, "note {n} h{k}: {off:+.2} cents");
                        }
                    }
                }
            });
        }
    });
}

/// POS moves the bow, and the tone, not the pitch. At POS 1 the bow point
/// nulls the 3rd harmonic: 10 dB under POS 0.15's.
#[test]
fn bowed_pos_moves_the_tone_not_the_pitch() {
    let blocks = 3 * SR as usize / BLOCK_SIZE / 2;
    for note in [31, 48, 84] {
        let f0 = note_to_freq(note);
        for bright in [0.0, 1.0] {
            let at = |pos: f32| {
                let p = ModalParams {
                    pos,
                    bright,
                    ..v1_bowed()
                };
                play_modal_at(&p, note, 100, blocks, 0)
            };
            for pos in [0.0, 0.15, 0.5, 1.0] {
                let out = at(pos);
                let s = steady(&out);
                let c = cents(fundamental_hz(s, f0 as f64), f0);
                assert!(
                    c.abs() < BOW_CENTS,
                    "{note} POS {pos} BRIGHT {bright}: {c:+.2} cents"
                );
                assert!(
                    octave_clear(s, f0),
                    "{note} POS {pos} BRIGHT {bright}: an octave low"
                );
            }
            if note == 48 {
                let h3 = |pos| {
                    let out = at(pos);
                    let s = steady(&out);
                    goertzel(s, 3.0 * f0, SR) / goertzel(s, f0, SR)
                };
                let db = 20.0 * (h3(1.0) / h3(0.15)).log10();
                assert!(
                    db <= -10.0,
                    "BRIGHT {bright}: 3rd harmonic {db:+.1} dB at POS 1"
                );
            }
        }
    }
}

/// BRIGHT is the output's low-pass on both taps, the loop's fixed at
/// INIT's: 0 against 1 takes 3 dB or more off harmonics 8 to 24, and
/// moves the fundamental under 2 cents. In the loop it sharpened the
/// stick-slip's corner, and BRIGHT 0 read 4.5 dB brighter.
#[test]
fn bowed_bright_is_heard() {
    let blocks = 3 * SR as usize / BLOCK_SIZE / 2;
    let f0 = note_to_freq(48);
    let at = |bright: f32| {
        let p = ModalParams {
            bright,
            ..v1_bowed()
        };
        play_modal_at(&p, 48, 100, blocks, 0)
    };
    let (dark, bright) = (at(0.0), at(1.0));
    let (dark, bright) = (steady(&dark), steady(&bright));
    let upper = |s: &[f32]| {
        (8..=24)
            .map(|k| goertzel(s, k as f32 * f0, SR).powi(2))
            .sum::<f32>()
    };
    let db = 10.0 * (upper(dark) / upper(bright)).log10();
    eprintln!("BRIGHT 0 against 1: harmonics 8–24 {db:+.2} dB");
    assert!(db <= -3.0, "harmonics 8–24: {db:+.2} dB");
    let (fd, fb) = (
        fundamental_hz(dark, f0 as f64),
        fundamental_hz(bright, f0 as f64),
    );
    let moved = 1200.0 * (fd / fb).log2();
    assert!(moved.abs() < 2.0, "the fundamental moves {moved:+.2} cents");
}

/// A square LFO swings POS end to end: the split glides a whole sample
/// one block in `BOW_SLEW` (8), so no sample steps further than the bow's
/// own corner does held at either end, and the pitch holds (a split that
/// jumps is caught there, +25 cents at G1).
#[test]
fn a_bowed_pos_sweep_does_not_click() {
    let second = SR as usize / BLOCK_SIZE;
    let addr = ParamAddr::new(BlockRef::Modal, ModalParams::POS);
    for note in [31, 96] {
        for force in [0.5, 1.0] {
            let mut p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal = ModalParams {
                force,
                ..v1_bowed()
            };
            p.lfos[0].rate = 2.0;
            p.lfos[0].shape = chimera_core::dsp::lfo::LfoShape::Square as u8;
            let routed = play_voice_at(note, &p, &routes(addr, 127), 2 * second, &[]);
            let held = [0.0, 1.0].map(|pos| {
                let mut h = p.clone();
                h.modal.pos = pos;
                play_voice_at(note, &h, &ModState::new(), 2 * second, &[])
            });
            let label = format!("{note} FORCE {force}");
            let d = rms_diff(&routed, &held[0]);
            assert!(d > 1e-3, "{label}: the route changes nothing ({d})");
            let (k, corner) = (
                common::step(&routed),
                common::step(&held[0]).max(common::step(&held[1])),
            );
            assert!(
                k <= 1.05 * corner,
                "{label}: steps {k}, the bow's corner {corner}"
            );
            let f0 = note_to_freq(note);
            let c = cents(fundamental_hz(steady(&routed), f0 as f64), f0);
            assert!(c.abs() < BOW_CENTS, "{label}: {c:+.2} cents");
        }
    }
}

/// FORCE, SPEED, BRIGHT and POS at their ends, G1 to the top note, 12 s:
/// bounded, no growth, no DC; with FORCE and SPEED at 1, released at
/// DAMP's ends, bounded; each sounding corner at G1 and C7 at its note.
#[test]
fn bowed_is_stable_and_in_tune_at_every_corner() {
    let sr = SR as usize;
    let blocks = 12 * sr / BLOCK_SIZE;
    std::thread::scope(|scope| {
        for note in [31, 96, 127] {
            scope.spawn(move || {
                for corner in 0..16 {
                    let end = |bit: usize| ((corner >> bit) & 1) as f32;
                    let p = ModalParams {
                        force: end(0),
                        speed: end(1),
                        bright: end(2),
                        pos: end(3),
                        ..v1_bowed()
                    };
                    let label =
                        format!("{note} F{} S{} B{} P{}", p.force, p.speed, p.bright, p.pos);
                    let out = play_modal(&p, note, blocks, 0);
                    assert_stable_at(&out, note_to_freq(note), 4.0, BOW_MARGIN, &label);
                    if p.force > 0.0 && p.speed > 0.0 && note != 127 {
                        let last = &out[out.len() - sr..];
                        assert!(common::rms(last) > 1e-3, "{label}: silent");
                        assert!(
                            octave_clear(last, note_to_freq(note)),
                            "{label}: an octave low"
                        );
                    }
                    if p.force == 1.0 && p.speed == 1.0 {
                        for damp in [0.0, 1.0] {
                            let q = ModalParams { damp, ..p };
                            let out = play_modal(&q, note, sr / BLOCK_SIZE, sr / BLOCK_SIZE);
                            assert!(
                                out.iter().all(|x| x.is_finite() && x.abs() <= 4.0),
                                "{label} DAMP {damp}: released, bounded"
                            );
                        }
                    }
                }
            });
        }
    });
}

/// No sub-harmonic at −20 dB: not f0/2 (`octave_clear`), nor f0/3 or 2f0/3
/// (period-tripling); in tune within `BOW_CENTS`, and sounding.
fn bows_clean(s: &[f32], note: u8) -> bool {
    let f0 = note_to_freq(note);
    let g = goertzel(s, f0, SR);
    let thirds = goertzel(s, f0 / 3.0, SR).max(goertzel(s, 2.0 * f0 / 3.0, SR));
    let c = cents(fundamental_hz(s, f0 as f64), f0);
    octave_clear(s, f0) && thirds < 0.1 * g && c.abs() < BOW_CENTS && common::rms(s) > 1e-3
}

/// The bow is robust across the instrument, not only at the tested
/// points. Seven notes, G1 to C7 × velocity 20 and 127 × FORCE 0.1, 0.5,
/// 1 × SPEED 0.1, 1 × POS 0 to 1 by quarters, 3 s held, clean over
/// 0.5–1.5 s and 2–3 s in 96 % of cases or more; and at INIT's POS,
/// FORCE and SPEED, velocity 20, 64 and 127, in every case. What fails is
/// C7's slow, heavy bow (SPEED 0.1, FORCE 0.5 at v127 or FORCE 1): it runs
/// 6 to 37 cents sharp, measured, beyond `unlocked`'s reach.
#[test]
fn bowed_plays_clean_across_the_instrument() {
    let sr = SR as usize;
    let blocks = 3 * sr / BLOCK_SIZE;
    let notes = [31, 36, 48, 60, 72, 84, 96];
    let clean = |p: &ModalParams, note: u8, vel: u8| {
        let out = play_modal_at(p, note, vel, blocks, 0);
        bows_clean(&out[sr / 2..sr * 3 / 2], note) && bows_clean(&out[2 * sr..3 * sr], note)
    };
    let (grid, defaults) = std::thread::scope(|scope| {
        let grid: Vec<_> = notes
            .iter()
            .map(|&note| {
                scope.spawn(move || {
                    let mut fails = Vec::new();
                    let mut n = 0;
                    for vel in [20, 127] {
                        for force in [0.1, 0.5, 1.0] {
                            for speed in [0.1, 1.0] {
                                for pos in [0.0, 0.25, 0.5, 0.75, 1.0] {
                                    let p = ModalParams {
                                        force,
                                        speed,
                                        pos,
                                        ..v1_bowed()
                                    };
                                    n += 1;
                                    if !clean(&p, note, vel) {
                                        fails.push(format!(
                                            "{note} v{vel} F{force} S{speed} P{pos}"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    (n, fails)
                })
            })
            .collect();
        let defaults: Vec<_> = notes
            .iter()
            .map(|&note| {
                scope.spawn(move || {
                    let p = ModalParams {
                        mode: ResonatorMode::Bowed,
                        ..Default::default()
                    };
                    [20, 64, 127]
                        .into_iter()
                        .filter(|&vel| !clean(&p, note, vel))
                        .map(|vel| format!("INIT {note} v{vel}"))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        (
            grid.into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>(),
            defaults
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect::<Vec<_>>(),
        )
    });
    let n: usize = grid.iter().map(|g| g.0).sum();
    let fails: Vec<_> = grid.into_iter().flat_map(|g| g.1).collect();
    let clean_pct = 100.0 * (n - fails.len()) as f64 / n as f64;
    assert!(
        clean_pct >= 96.0,
        "{clean_pct:.1} % clean of {n}: {fails:?}"
    );
    assert!(defaults.is_empty(), "the defaults: {defaults:?}");
}

/// FORCE and SPEED move the bow's tone, not only its level, at the v1
/// bow and INIT's POS, G1, C3 and C6, each at 0.25 against 1: the
/// harmonics' shares of the spectrum (summed |Δ| over 1 to 24) move
/// `FORCE_SHAPE` and `SPEED_SHAPE` or more, and FORCE, which sets the
/// friction's slope, moves harmonics 8 to 24 against the fundamental
/// `FORCE_DB` or more. Measured: FORCE's shares 0.030 at G1 to 0.25 at C3,
/// its harmonics 0.8 dB at G1, 2.3 at C3, 4.4 at C6; SPEED's shares 0.014
/// at G1 to 0.36 at C3.
#[test]
fn force_and_speed_move_the_bows_tone() {
    let sr = SR as usize;
    let mut worst = [(f32::MAX, String::new()), (f32::MAX, String::new())];
    let mut brighter = (f32::MAX, String::new());
    for note in [31, 48, 84] {
        let f0 = note_to_freq(note);
        for pos in [0.15, 0.0] {
            for (i, knob) in ["FORCE", "SPEED"].into_iter().enumerate() {
                let h = |v: f32| {
                    let mut p = ModalParams { pos, ..v1_bowed() };
                    match knob {
                        "FORCE" => p.force = v,
                        _ => p.speed = v,
                    }
                    let out = play_modal_at(&p, note, 100, 2 * sr / BLOCK_SIZE, 0);
                    (1..=24)
                        .map(|k| goertzel(&out[sr..2 * sr], k as f32 * f0, SR))
                        .collect::<Vec<_>>()
                };
                let (a, b) = (h(0.25), h(1.0));
                let share = |h: &[f32]| {
                    let sum: f32 = h.iter().sum();
                    h.iter().map(|x| x / sum).collect::<Vec<_>>()
                };
                let d: f32 = share(&a)
                    .iter()
                    .zip(share(&b))
                    .map(|(x, y)| (x - y).abs())
                    .sum();
                let label = format!("{knob} at {note} POS {pos}");
                if d < worst[i].0 {
                    worst[i] = (d, label.clone());
                }
                if knob == "FORCE" {
                    let upper =
                        |h: &[f32]| h[7..].iter().map(|x| x * x).sum::<f32>() / (h[0] * h[0]);
                    let db = (10.0 * (upper(&b) / upper(&a)).log10()).abs();
                    if db < brighter.0 {
                        brighter = (db, label);
                    }
                }
            }
        }
    }
    println!(
        "least: FORCE {:.4} ({}), SPEED {:.4} ({}); FORCE's harmonics {:.1} dB ({})",
        worst[0].0, worst[0].1, worst[1].0, worst[1].1, brighter.0, brighter.1
    );
    for ((d, label), floor) in worst.iter().zip([FORCE_SHAPE, SPEED_SHAPE]) {
        assert!(*d >= floor, "{label}: the shape moves {d:.4}");
    }
    assert!(
        brighter.0 >= FORCE_DB,
        "{}: FORCE moves harmonics 8–24 {:.1} dB",
        brighter.1,
        brighter.0
    );
}

/// `force_and_speed_move_the_bows_tone`'s floors, under its least.
const FORCE_SHAPE: f32 = 0.025;
const SPEED_SHAPE: f32 = 0.012;
const FORCE_DB: f32 = 0.6;

/// DAMP is the ring after the lift at every note: at C4 and C6 DAMP 1's
/// tail, 0.3–0.6 s after note-off, is 10 dB or more above DAMP 0.5's. The
/// lifted bow's gain rises to DAMP's past the bowed loss.
#[test]
fn bowed_damp_rings_at_every_note() {
    let (sr, second) = (SR as usize, SR as usize / BLOCK_SIZE);
    for note in [60, 84] {
        let tail = |damp: f32| {
            let p = ModalParams { damp, ..v1_bowed() };
            let out = play_modal_at(&p, note, 100, second, second);
            common::rms(&out[sr + sr * 3 / 10..sr + sr * 6 / 10])
        };
        let db = 20.0 * (tail(1.0) / tail(0.5)).log10();
        assert!(db >= 10.0, "{note}: DAMP 1 over 0.5, {db:+.1} dB");
    }
}

/// A note-off before the first block, or after exactly one, lifts the bow
/// for good: the note goes silent and frees its voice within the ring
/// (DAMP 0.5 s). A second note-off, at once or mid-ring, changes nothing,
/// and a retrigger, freed or mid-ring, bows again. A pending first-block
/// settle once re-armed a lifted bow, which played on forever.
#[test]
fn a_bow_lifted_before_it_plays_stays_lifted() {
    use chimera_core::dsp::modal::SymPool;
    let second = SR as usize / BLOCK_SIZE;
    let p = v1_bowed();
    let mut block = [0.0; BLOCK_SIZE];
    // `held` blocks, note-off, then 3 s, a second note-off at `again`.
    let lifted = |note: u8, held: usize, again: Option<usize>| {
        let mut pool = SymPool::boxed();
        let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
        let mut block = [0.0; BLOCK_SIZE];
        e.note_on(note, 100, &p, SR, &mut pool);
        for _ in 0..held {
            e.render(&mut block, &p, SR, &mut pool);
        }
        e.note_off(&mut pool);
        let mut out = Vec::new();
        for b in 0..3 * second {
            if again == Some(b) {
                e.note_off(&mut pool);
            }
            e.render(&mut block, &p, SR, &mut pool);
            out.extend_from_slice(&block);
        }
        let last = common::rms(&out[out.len() - SR as usize..]);
        assert!(
            last < 1e-4,
            "{note} held {held}, again {again:?}: rms {last} at 2–3 s"
        );
        assert!(
            !e.is_active(),
            "{note} held {held}, again {again:?}: never freed"
        );
        (e, pool)
    };
    for held in [0, 1] {
        for note in [48, 84] {
            lifted(note, held, None);
            lifted(note, held, Some(0));
            let (mut e, mut pool) = lifted(note, held, Some(second / 5));
            // A retrigger bows again: after the voice is freed, and mid-ring.
            e.note_on(note, 100, &p, SR, &mut pool);
            e.render(&mut block, &p, SR, &mut pool);
            e.note_off(&mut pool);
            for _ in 0..second / 5 {
                e.render(&mut block, &p, SR, &mut pool);
            }
            e.note_on(note, 100, &p, SR, &mut pool);
            let mut ring = Vec::new();
            for _ in 0..second {
                e.render(&mut block, &p, SR, &mut pool);
                ring.extend_from_slice(&block);
            }
            let r = common::rms(&ring[SR as usize / 2..]);
            assert!(r > 1e-2, "{note} held {held}: a retrigger is silent ({r})");
        }
    }
}

/// `x`'s amplitude at `hz`, Hann-windowed: one partial's level, its
/// neighbours a harmonic away leaking under −80 dB.
fn partial(x: &[f32], hz: f32) -> f64 {
    let (n, w) = (
        x.len() as f64,
        std::f64::consts::TAU * hz as f64 / SR as f64,
    );
    let (mut re, mut im) = (0.0, 0.0);
    for (i, &s) in x.iter().enumerate() {
        let h = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n).cos();
        re += s as f64 * h * (w * i as f64).cos();
        im += s as f64 * h * (w * i as f64).sin();
    }
    (re * re + im * im).sqrt() * 4.0 / n
}

/// DAMP's law (`params::t60`), seconds.
fn damp_t60(damp: f32) -> f32 {
    0.05 * 400f32.powf(damp)
}

/// ADR 0056: DAMP sets the fundamental's ring the same way at every
/// pitch. STRING, SYMP's main string and a lifted bow, G1 to C7, at
/// BRIGHT 0 and INIT's: the fundamental's T60 within 10 % of DAMP's law,
/// so DAMP 1 rings 20 s at C6; and nothing grows. The loop's low-pass
/// once took its loss at f0 on top of DAMP's, as f0³: C6 at DAMP 1 rang
/// 2.5 s, G6 0.8.
#[test]
fn damp_sets_the_ring_at_every_pitch() {
    let sr = SR as usize;
    std::thread::scope(|scope| {
        for mode in [
            ResonatorMode::String,
            ResonatorMode::Sympathetic,
            ResonatorMode::Bowed,
        ] {
            scope.spawn(move || {
                let mut bad = Vec::new();
                for note in [31u8, 48, 60, 72, 84, 91, 96] {
                    for bright in [0.0, 0.3] {
                        for damp in [0.5, 0.8, 1.0] {
                            let p = ModalParams {
                                mode,
                                damp,
                                bright,
                                ..Default::default()
                            };
                            let law = damp_t60(damp);
                            // A quarter of the ring: past it a high note, its peak
                            // the pluck's whole band, nears the silence rule.
                            let span = (law / 4.0).min(2.0);
                            // From the lift on a bow; else 0.2 s in.
                            let (on, from) = match mode {
                                ResonatorMode::Bowed => (sr / BLOCK_SIZE, 1.1),
                                _ => (0, 0.2),
                            };
                            let len = ((from + span + 0.2) * SR as f32) as usize;
                            let out = match mode {
                                ResonatorMode::Sympathetic => {
                                    play_modal_bare(&p, note, 100, len / BLOCK_SIZE, 0)
                                }
                                _ if on > 0 => {
                                    play_modal_at(&p, note, 100, on, len / BLOCK_SIZE - on)
                                }
                                _ => play_modal_at(&p, note, 100, len / BLOCK_SIZE, 0),
                            };
                            let f0 = note_to_freq(note);
                            let win = |t: f32| {
                                let i = (t * SR as f32) as usize;
                                &out[i..i + sr / 10]
                            };
                            let (a, b) = (partial(win(from), f0), partial(win(from + span), f0));
                            let fall = 20.0 * (a / b).log10();
                            let t60 = 60.0 * span / fall as f32;
                            let label = format!("{mode:?} {note} BRIGHT {bright} DAMP {damp}");
                            if !(fall > 0.0 && (t60 / law - 1.0).abs() < 0.1) {
                                let heard = out.iter().rposition(|&x| x != 0.0).unwrap_or(0);
                                bad.push(format!(
                                    "{label}: T60 {t60:.2} s, law {law:.2}; heard to {:.2} s",
                                    heard as f32 / SR as f32
                                ));
                            }
                            let (ra, rb) = (common::rms(win(from)), common::rms(win(from + span)));
                            if rb > ra {
                                bad.push(format!("{label}: grew {ra} → {rb}"));
                            }
                        }
                    }
                }
                assert!(bad.is_empty(), "{}", bad.join("\n"));
            });
        }
    });
}

/// ADR 0056: POS runs the pluck or strike from the end (β 0, where POS
/// 0 always was) to the middle (0.5). A pluck at β and at 1 − β is the
/// same, so POS 0 and 1 once rendered bit for bit alike. The 2nd partial
/// over the 1st falls from POS 0 to 0.5 to 1, to a null at the middle:
/// `cos πβ / cos πβ/2` on a pluck (`string::comb`), `cos² πβ` on a
/// strike (Rings' weights). A strike's weights are exact, so it falls at
/// every 1/8; a pluck's partials carry its noise's, a dB or two each
/// (the loop's period is not the line's), so its every step is
/// `string::comb`'s test.
#[test]
fn pos_runs_from_the_end_to_the_middle() {
    let sr = SR as usize;
    for mode in [
        ResonatorMode::String,
        ResonatorMode::Sympathetic,
        ResonatorMode::Modal,
    ] {
        let bank = mode == ResonatorMode::Modal;
        for note in [48u8, 60] {
            let f0 = note_to_freq(note);
            // The 2nd partial over the 1st, dB.
            let at = |pos: f32| {
                let p = ModalParams {
                    mode,
                    pos,
                    damp: 0.8,
                    // The bank's tanh linear; its modes harmonic, STRUCTURE's plateau.
                    bright: if bank { 0.3 } else { 1.0 },
                    structure: if bank { 0.27 } else { 0.0 },
                    body: 0.0,
                    ..Default::default()
                };
                let (vel, blocks) = (if bank { 30 } else { 100 }, sr / 2 / BLOCK_SIZE);
                let out = match mode {
                    ResonatorMode::Sympathetic => play_modal_bare(&p, note, vel, blocks, 0),
                    _ => play_modal_at(&p, note, vel, blocks, 0),
                };
                let s = &out[sr / 20..sr / 20 + sr / 5];
                20.0 * (partial(s, 2.0 * f0) / partial(s, f0)).log10()
            };
            let steps: Vec<f64> = (0..=8).map(|k| at(k as f32 / 8.0)).collect();
            let label = format!("{mode:?} {note}: {steps:.1?}");
            let (lo, mid, hi) = (steps[0], steps[4], steps[8]);
            assert!(lo > mid && mid > hi, "end, quarter, middle: {label}");
            assert!(hi < lo - 6.0, "the middle nulls the 2nd: {label}");
            if bank {
                for w in steps.windows(2) {
                    assert!(w[1] < w[0], "every step: {label}");
                }
            }
        }
    }
}

/// `out`'s mean over the whole periods of `hz` in the 0.1 s from `t` s,
/// the fractional ends weighted: a wave's own mean, no window edge's.
fn period_mean(out: &[f32], hz: f32, t: f64) -> f64 {
    let per = SR as f64 / hz as f64;
    let len = (0.1 * SR as f64 / per).ceil() * per;
    let a = t * SR as f64;
    let b = a + len;
    let (ia, ib) = (a as usize, b as usize);
    let sum: f64 = (ia..=ib)
        .map(|i| {
            let w = if i == ia {
                1.0 - (a - ia as f64)
            } else if i == ib {
                b - ib as f64
            } else {
                1.0
            };
            out[i] as f64 * w
        })
        .sum();
    sum / len
}

/// #248: a held bow does not drift below 10 Hz. Its attack settles the
/// string's static deflection at the loop's rate (12 periods); once
/// settled, from 1 s, every 0.1 s of the engine's output, over whole
/// periods, holds a mean at least 45 dB under its RMS (the one-loop bow's
/// worst, −49.2 dB at C6, ADR 0064), G1 to C7, at
/// SPEED and BRIGHT's corners. Measured over a window that cuts a
/// period, a bow's pulse wave reads as a −30 dB drift that isn't there.
#[test]
fn a_settled_bow_does_not_drift() {
    let sr = SR as usize;
    let mut worst = (f64::NEG_INFINITY, String::new());
    for note in [31u8, 36, 48, 60, 72, 84, 96] {
        for (speed, bright) in [(0.5, 0.3), (0.1, 0.0), (1.0, 0.0), (1.0, 1.0)] {
            let p = ModalParams {
                mode: ResonatorMode::Bowed,
                speed,
                bright,
                ..Default::default()
            };
            let out = play_modal_at(&p, note, 100, 3 * sr / BLOCK_SIZE, 0);
            let f0 = note_to_freq(note);
            let rms = common::rms(&out[sr..2 * sr]) as f64;
            for k in 0..15 {
                let m = period_mean(&out, f0, 1.0 + 0.1 * k as f64).abs();
                let db = 20.0 * (m / rms).log10();
                if db > worst.0 {
                    worst = (db, format!("{note} SPEED {speed} BRIGHT {bright}"));
                }
            }
        }
    }
    println!("worst: {:.1} dB at {}", worst.0, worst.1);
    assert!(worst.0 < -45.0, "{:.1} dB at {}", worst.0, worst.1);
}

/// The owner's UAT (2026-09-30): a released string rings on, as Rings'
/// does. STRING and SYMP at C3, released 0.3 s in: the fundamental's T60
/// from 0.1 s after the note-off is the held note's over the same span
/// within 10 %, and the 100 ms after the note-off are within 1 dB of the
/// held note's.
#[test]
fn a_released_string_rings_on_damp() {
    let sr = SR as usize;
    let (on, off) = (sr * 3 / 10 / BLOCK_SIZE, 3 * sr / BLOCK_SIZE);
    let cut = on * BLOCK_SIZE;
    let f0 = note_to_freq(48);
    for mode in [ResonatorMode::String, ResonatorMode::Sympathetic] {
        for damp in [0.5, ModalParams::default().damp] {
            let p = ModalParams {
                mode,
                damp,
                ..Default::default()
            };
            let held = play_modal(&p, 48, on + off, 0);
            let released = play_modal(&p, 48, on, off);
            // A quarter of the ring, at most a second.
            let span = (damp_t60(damp) / 4.0).min(1.0);
            let t60 = |out: &[f32]| {
                let at = |t: usize| partial(&out[t..t + sr / 10], f0);
                let a = cut + sr / 10;
                let b = a + (span * SR as f32) as usize;
                60.0 * span as f64 / (20.0 * (at(a) / at(b)).log10())
            };
            let (h, r) = (t60(&held), t60(&released));
            assert!(
                (r / h - 1.0).abs() < 0.1,
                "{mode:?} DAMP {damp}: released T60 {r:.2} s, held {h:.2} s"
            );
            let after = |out: &[f32]| common::rms(&out[cut..cut + sr / 10]);
            let drop = 20.0 * (after(&held) / after(&released)).log10();
            assert!(drop < 1.0, "{mode:?} DAMP {damp}: {drop:.2} dB down");
        }
    }
}

/// A note is freed only once it is 60 dB under its own peak: STRING C5 at
/// DAMP 1, released at once, still sounds 5 s on, and when it frees its
/// last blocks are under a thousandth of its peak.
#[test]
fn an_undamped_c5_frees_60_db_under_its_peak() {
    let p = ModalParams {
        mode: ResonatorMode::String,
        damp: 1.0,
        ..Default::default()
    };
    let mut pool = chimera_core::dsp::modal::SymPool::boxed();
    let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
    e.note_on(72, 100, &p, SR, &mut pool);
    e.note_off(&mut pool);
    let mut out = Vec::new();
    let mut block = [0.0; BLOCK_SIZE];
    while e.is_active() && out.len() < 120 * SR as usize {
        e.render(&mut block, &p, SR, &mut pool);
        out.extend_from_slice(&block);
    }
    let len = out.len() as f32 / SR as f32;
    assert!(len > 5.0, "freed at {len:.2} s");
    let peak = common::peak(&out);
    let last = common::peak(&out[out.len() - 11 * BLOCK_SIZE..]);
    assert!(last <= peak * 1e-3, "freed at {last} of a {peak} peak");
}

/// The owner's UAT (2026-09-30): a re-strike adds to what rings, as a
/// re-plucked string does, and clears nothing. C3 struck at v127, rung
/// 0.3 s (released on the strings, held on the bow), then struck again:
/// softly (v1) on the plucked and struck models, at v100 on a held bow.
/// The 0.1 s after the re-strike are within 1 dB of the note left alone.
#[test]
fn a_restrike_adds_to_the_ring() {
    let sr = SR as usize;
    let at = sr * 3 / 10 / BLOCK_SIZE;
    for mode in MODES {
        let p = ModalParams {
            mode,
            ..Default::default()
        };
        let bowed = mode == ResonatorMode::Bowed;
        let run = |restrike: bool| {
            let mut pool = chimera_core::dsp::modal::SymPool::boxed();
            let mut e = Box::new(ModalEngine::new_in(&mut pool, mode));
            e.note_on(48, if bowed { 100 } else { 127 }, &p, SR, &mut pool);
            if !bowed {
                e.note_off(&mut pool);
            }
            let mut out = Vec::new();
            let mut block = [0.0; BLOCK_SIZE];
            for b in 0..at + sr / 10 / BLOCK_SIZE {
                if b == at && restrike {
                    e.note_on(48, if bowed { 100 } else { 1 }, &p, SR, &mut pool);
                }
                e.render(&mut block, &p, SR, &mut pool);
                out.extend_from_slice(&block);
            }
            common::rms(&out[at * BLOCK_SIZE..])
        };
        let db = 20.0 * (run(true) / run(false)).log10();
        assert!(db.abs() < 1.0, "{mode:?}: {db:.2} dB");
    }
}

/// A resting halo follows a small pitch change where it is: after a chord
/// glide took a string round the fold to the octave above, a 0.1 %
/// PITCH move moves every string 0.1 %, not an octave down; a fifth down
/// takes every string whose line fits down a fifth.
#[test]
fn a_resting_halo_keeps_its_octave_under_a_pitch_move() {
    use chimera_core::dsp::modal::{CHORD_COUNT, SymPool};
    let at = |k: usize| (k as f32 + 0.5) / CHORD_COUNT as f32;
    for note in [24u8, 31] {
        for (from, to) in [(0, 1), (4, 5), (5, 6)] {
            let mut p = ModalParams {
                mode: ResonatorMode::Sympathetic,
                structure: at(from),
                ..Default::default()
            };
            let mut pool = SymPool::boxed();
            let mut e = Box::new(ModalEngine::new_in(&mut pool, p.mode));
            e.note_on(note, 100, &p, SR, &mut pool);
            let mut block = [0.0; BLOCK_SIZE];
            e.render(&mut block, &p, SR, &mut pool);
            p.structure = at(to);
            for _ in 0..2 * SR as usize / BLOCK_SIZE {
                e.render(&mut block, &p, SR, &mut pool);
            }
            // Then a fifth down, where every line still fits: it follows.
            for pitch in [1.001, 1.001 / 1.5] {
                let before = e.halo_periods(&pool).unwrap();
                let was = if pitch > 1.0 { 1.0 } else { 1.001 };
                e.set_pitch(pitch);
                e.render(&mut block, &p, SR, &mut pool);
                let after = e.halo_periods(&pool).unwrap();
                for s in 0..7 {
                    let r = after[s] / before[s] * pitch / was;
                    let fits = before[s] * was / pitch < 980.0;
                    assert!(
                        !fits || (r - 1.0).abs() < 1e-3,
                        "note {note}, chord {from} to {to}, string {s}, pitch {pitch}: × {r}"
                    );
                }
            }
        }
    }
}
