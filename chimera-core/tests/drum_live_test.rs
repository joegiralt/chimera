// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM slice 2 (ADR 0269): a pad's continuous values follow its live
//! edits on a hit still ringing, glided at the KNOB coefficient as a
//! Part's knob; its choices and its machine's knobs latch at the hit.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex, MutexGuard};

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::Block;
use chimera_core::dsp::drum::sound::{EnvDecay, EnvPos, PadEnv, PadSound};
use chimera_core::dsp::drum::types::{Hz, Kit, Knob, KnobValue, Machine, PadId};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use chimera_midi::value::U7;
use common::{InstRig, SR};

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

/// CUTOFF's place, 0..1 on its octaves.
fn place(hz: f32) -> f32 {
    FilterParams::default()
        .spec(FilterParams::CUTOFF)
        .expect("CUTOFF's spec")
        .normalize(hz)
}

/// Part 1's Sound (DRUM INIT) and, on Part 2, ALGO INIT on LADDER at
/// 300 Hz: its knob CUTOFF is the reference glide.
fn sound(e: EngineType) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(e);
    if e == EngineType::Algo {
        p.filter.set_kind(FilterKind::Ladder);
        p.filter.cutoff = 300.0;
    }
    p
}

/// Pads 9 and 10 on TONE CYMBAL, DECAY 127, LADDER at 300 Hz; the rest
/// `Kit::DEFAULT`'s.
fn kit() -> Kit {
    let mut k = Kit::DEFAULT;
    for n in [9, 10] {
        let p = k.pad_mut(pad(n));
        p.set_machine(Machine::ToneCymbal.into());
        p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
        let f = p.sound_mut().filter_mut();
        f.set_kind(FilterKind::Ladder);
        f.set_cutoff(Hz::new(300.0));
    }
    k
}

/// The writer, every Part on `Kit::DEFAULT` but Part 1 on `kit`; Part 1
/// (channel 1) DRUM, Part 2 (channel 2) ALGO, the rest on channel 16.
fn rig(kit: &Kit) -> (MutexGuard<'static, KitWriter>, InstRig) {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for p in PartId::ALL {
        w.publish(p, &Kit::DEFAULT);
        let _ = w.take_hit(p);
    }
    w.publish(P1, kit);
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(i.min(15) as u8).unwrap();
    }
    for i in 2..r.shared.parts.len() {
        r.shared.parts[i].mix.channel = MidiChannel::new(15).unwrap();
    }
    *r.shared.parts[0].params = sound(EngineType::Drum);
    *r.shared.parts[1].params = sound(EngineType::Algo);
    (w, r)
}

/// The note that strikes pad `n` (TONAL OFF: the kit every octave).
fn key(n: u8) -> u8 {
    n - 1
}

/// The active voice sounding `p`'s hit.
fn voice_of(r: &InstRig, p: PadId) -> usize {
    (0..MAX_VOICES)
        .find(|&v| {
            let voice = r.inst.voice(v);
            voice.is_active()
                && matches!(voice.slot(), EngineSlot::Drum(d) if d.hit().is_some_and(|h| h.pad() == p))
        })
        .expect("a voice sounds the pad")
}

/// Part 2's ALGO voice.
fn algo_voice(r: &InstRig) -> usize {
    (0..MAX_VOICES)
        .find(|&v| {
            let voice = r.inst.voice(v);
            voice.is_active() && matches!(voice.slot(), EngineSlot::Algo(_))
        })
        .expect("Part 2's note sounds")
}

/// Pad `n`'s sound in `k`, edited.
fn edit(k: &mut Kit, n: u8, f: impl FnOnce(&mut PadSound)) {
    f(k.pad_mut(pad(n)).sound_mut());
}

/// Review Focus 1. A CUTOFF turn reaches the ringing hit as Part 2's knob
/// CUTOFF reaches its voice, block for block on CUTOFF's place: it rises
/// every block, by the knob's own one-pole steps (no step held, none
/// jumped), and lands within 1 % of 6000 Hz by 300 ms.
#[test]
fn cutoff_turn_glides_on_the_ringing_hit() {
    let mut k = kit();
    let (mut w, mut r) = rig(&k);
    r.note_on(1, 60);
    r.note_on(0, key(9));
    r.render(10);
    let (v, a) = (voice_of(&r, pad(9)), algo_voice(&r));
    assert_eq!(r.inst.voice(v).played_filter().cutoff, 300.0);
    assert_eq!(r.inst.played(CUTOFF)[a], 300.0);
    edit(&mut k, 9, |s| s.filter_mut().set_cutoff(Hz::new(6000.0)));
    w.publish(P1, &k);
    r.shared.parts[1].params.filter.cutoff = 6000.0;
    let blocks = 300 * SR as usize / 1000 / BLOCK_SIZE;
    let (mut was, mut knob_was) = (place(300.0), place(300.0));
    let mut steps = Vec::new();
    for b in 0..blocks {
        r.render(1);
        let f = r.inst.voice(v).played_filter();
        assert_eq!(f.kind(), FilterKind::Ladder);
        let (now, knob) = (place(f.cutoff), place(r.inst.played(CUTOFF)[a]));
        assert!(
            (now - knob).abs() < 1e-4,
            "block {b}: hit at {now}, knob at {knob}"
        );
        let step = now - was;
        assert!(
            (step - (knob - knob_was)).abs() < 1e-4,
            "block {b}: step {step}"
        );
        if f.cutoff != 6000.0 {
            assert!(step > 0.0, "block {b}: held at {}", f.cutoff);
        }
        steps.push(step);
        (was, knob_was) = (now, knob);
    }
    // The one-pole's steps shrink block by block: a staircase (a snap, or
    // a step held then jumped) would put a step past the one before it.
    let first = steps[0];
    assert!(
        first < 0.5 * (place(6000.0) - place(300.0)),
        "snapped: {first}"
    );
    for (b, s) in steps.windows(2).enumerate() {
        assert!(
            s[1] <= s[0] + 1e-6,
            "block {}: {} after {}",
            b + 1,
            s[1],
            s[0]
        );
    }
    let f = r.inst.voice(v).played_filter();
    assert!((f.cutoff - 6000.0).abs() < 60.0, "{} at 300 ms", f.cutoff);
}

/// TYPE latches: SEM published mid-ring leaves the hit on LADDER; the
/// next hit plays SEM.
#[test]
fn type_change_waits_for_the_next_hit() {
    let mut k = kit();
    let (mut w, mut r) = rig(&k);
    r.note_on(0, key(9));
    r.render(4);
    let v = voice_of(&r, pad(9));
    edit(&mut k, 9, |s| s.filter_mut().set_kind(FilterKind::Sem));
    w.publish(P1, &k);
    r.render(20);
    assert_eq!(r.inst.voice(v).played_filter().kind(), FilterKind::Ladder);
    r.note_off(0, key(9));
    r.note_on(0, key(9));
    r.render(1);
    // The new hit: a fresh voice, or the old one re-struck.
    let kinds: Vec<_> = (0..MAX_VOICES)
        .filter(|&u| {
            let voice = r.inst.voice(u);
            voice.is_active()
                && matches!(voice.slot(), EngineSlot::Drum(d) if d.hit().is_some_and(|h| h.pad() == pad(9)))
        })
        .map(|u| (u == v, r.inst.voice(u).played_filter().kind()))
        .collect();
    assert!(
        kinds.contains(&(kinds.len() == 1, FilterKind::Sem)),
        "{kinds:?}"
    );
}

/// An edit to pad 10 leaves pad 9's ringing hit where it was.
#[test]
fn other_pads_hold() {
    let mut k = kit();
    let (mut w, mut r) = rig(&k);
    r.note_on(0, key(9));
    r.render(4);
    let v = voice_of(&r, pad(9));
    let before = r.inst.voice(v).played_filter();
    edit(&mut k, 10, |s| {
        s.filter_mut().set_cutoff(Hz::new(6000.0));
        s.filter_mut().set_kind(FilterKind::Sem);
    });
    w.publish(P1, &k);
    r.render(20);
    let after = r.inst.voice(v).played_filter();
    assert_eq!(
        (after.kind(), after.cutoff, after.resonance),
        (before.kind(), before.cutoff, before.resonance)
    );
}

/// The machine's knobs latch: a DECAY edit published mid-ring leaves the
/// hit's sound bit for bit. On PLAIN SINE (DECAY 127, no filter), whose
/// hit doesn't read its seed.
#[test]
fn machine_knobs_stay_latched() {
    let play = |edit: bool| {
        let mut k = Kit::DEFAULT;
        let p = k.pad_mut(pad(9));
        p.set_machine(Machine::PlainSine.into());
        p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
        let (mut w, mut r) = rig(&k);
        r.note_on(0, key(9));
        let mut out = Vec::new();
        for b in 0..200 {
            if edit && b == 10 {
                k.pad_mut(pad(9))
                    .knobs_mut()
                    .set(Knob::B, KnobValue::new(U7::new(5).unwrap()));
                w.publish(P1, &k);
            }
            r.render(1);
            out.extend_from_slice(r.inst.part_bus(0));
        }
        out
    };
    let (held, edited) = (play(false), play(true));
    assert!(held.iter().any(|x| x.abs() > 1e-3), "silent");
    assert_eq!(held, edited);
}

/// No publish, no read: `sound_if_newer` is `None` while the view's `seq`
/// stands, a publish that changes nothing included.
#[test]
fn no_publish_no_read() {
    let mut k = kit();
    let (mut w, _r) = rig(&k);
    let mut seen = KIT_VIEW.seq(P1);
    assert_eq!(KIT_VIEW.sound_if_newer(P1, pad(9), &mut seen), None);
    w.publish(P1, &k);
    assert_eq!(KIT_VIEW.sound_if_newer(P1, pad(9), &mut seen), None);
    edit(&mut k, 9, |s| s.filter_mut().set_cutoff(Hz::new(800.0)));
    w.publish(P1, &k);
    let s = KIT_VIEW
        .sound_if_newer(P1, pad(9), &mut seen)
        .expect("a publish");
    assert_eq!(s.filter().cutoff(), Hz::new(800.0));
    assert_eq!(KIT_VIEW.sound_if_newer(P1, pad(9), &mut seen), None);
}

/// An AMP DECAY turned down on a ringing hit ends it; untouched, it rings
/// on. PLAIN SINE (DECAY 127) under an AMP of D 120 on MED.
#[test]
fn amp_decay_follows() {
    let play = |edit: bool| {
        let mut k = Kit::DEFAULT;
        let p = k.pad_mut(pad(9));
        p.set_machine(Machine::PlainSine.into());
        p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
        let env = |d: u8| {
            PadEnv::new(
                EnvPos::ZERO,
                EnvPos::ZERO,
                EnvDecay::Time(EnvPos::new(U7::new(d).unwrap())),
                EnvSpeed::Med,
            )
        };
        *p.sound_mut().amp_mut().env_mut() = env(120);
        let (mut w, mut r) = rig(&k);
        r.note_on(0, key(9));
        r.render(5);
        if edit {
            *k.pad_mut(pad(9)).sound_mut().amp_mut().env_mut() = env(10);
            w.publish(P1, &k);
        }
        r.render(500 * SR as usize / 1000 / BLOCK_SIZE);
        (0..MAX_VOICES).any(|v| r.inst.voice(v).is_active())
    };
    assert!(play(false), "D 120 ended by 500 ms");
    assert!(!play(true), "D 10 still sounding at 500 ms");
}
