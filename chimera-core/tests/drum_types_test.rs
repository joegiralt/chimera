// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's type layer (ADR 0265): pads, knobs, machines, kit notes.

use chimera_core::MidiNote;
use chimera_core::block::DiskCode;
use chimera_core::dsp::drum::types::{
    ChokeGroup, DrumHit, HitSeed, Kernel, Kit, KitNote, Knob, KnobSet, KnobValue, Machine, Pad,
    PadId, PadSet, Pan, Tonal, Transpose,
};
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_midi::value::U7;

#[test]
fn pad_ids_wrap() {
    assert_eq!(PadId::ALL[11].next(), PadId::ALL[0]);
    assert_eq!(PadId::ALL[0].prev(), PadId::ALL[11]);
    assert_eq!(PadId::ALL[1].number(), 2);
    for (i, p) in PadId::ALL.iter().enumerate() {
        assert_eq!(p.index(), i);
        assert_eq!(p.next().prev(), *p);
    }
    assert_eq!(PadId::new(11), Some(PadId::ALL[11]));
    assert_eq!(PadId::new(12), None);
}

#[test]
fn pad_sets() {
    let p = |i| PadId::new(i).unwrap();
    assert_eq!(PadSet::EMPTY.iter().count(), 0);
    let s = PadSet::EMPTY.with(p(8)).with(p(1)).with(p(1));
    assert!(s.contains(p(1)) && s.contains(p(8)) && !s.contains(p(0)));
    assert_eq!(s.iter().collect::<Vec<_>>(), [p(1), p(8)], "in pad order");
    let u = s.union(PadSet::EMPTY.with(p(11)));
    assert_eq!(u.iter().collect::<Vec<_>>(), [p(1), p(8), p(11)]);
    assert_eq!(PadSet::from_bits(u.bits()), u);
    assert_eq!(
        PadSet::from_bits(u16::MAX).iter().count(),
        12,
        "bits past the pads dropped"
    );
}

/// ADR 0270's table: TONAL OFF repeats the kit every octave; TONAL on
/// plays its pad chromatically from C1 up, tuned at middle C.
#[test]
fn kit_notes() {
    let n = |n| MidiNote::new(n).unwrap();
    let pad = |i: u8| PadId::new(i - 1).unwrap();
    let st = |s| Transpose::new(s).unwrap();
    for (note, p) in [(0, 1), (11, 12), (12, 1), (13, 2), (60, 1), (127, 8)] {
        assert_eq!(
            KitNote::new(n(note), Tonal::Off),
            KitNote::Pad(pad(p)),
            "{note}"
        );
    }
    let t = Tonal::Pad(pad(3));
    for (note, want) in [
        (0, KitNote::Pad(pad(1))),
        (5, KitNote::Pad(pad(6))),
        (11, KitNote::Pad(pad(12))),
        (12, KitNote::Tonal(pad(3), st(-48))),
        (60, KitNote::Tonal(pad(3), Transpose::ZERO)),
        (72, KitNote::Tonal(pad(3), st(12))),
        (127, KitNote::Tonal(pad(3), st(67))),
    ] {
        assert_eq!(KitNote::new(n(note), t), want, "{note}");
    }
    // Total: every note on every TONAL is a pad.
    for tonal in [Tonal::Off].into_iter().chain(PadId::ALL.map(Tonal::Pad)) {
        for note in 0..=127 {
            let _ = KitNote::new(n(note), tonal);
        }
    }
}

#[test]
fn transpose_range() {
    assert_eq!(Transpose::new(67).map(Transpose::get), Some(67));
    assert_eq!(Transpose::new(-60).map(Transpose::get), Some(-60));
    assert_eq!(Transpose::new(68), None);
    assert_eq!(Transpose::new(-61), None);
    assert_eq!((Transpose::MIN.get(), Transpose::MAX.get()), (-60, 67));
}

#[test]
fn tonal_codes_are_stable() {
    assert_eq!(Tonal::Off.code(), 0);
    for p in PadId::ALL {
        assert_eq!(Tonal::Pad(p).code(), p.number());
        assert_eq!(Tonal::from_code(p.number()), Some(Tonal::Pad(p)));
    }
    assert_eq!(Tonal::from_code(0), Some(Tonal::Off));
    assert_eq!(Tonal::from_code(13), None);
    assert_eq!(Kit::DEFAULT.tonal(), Tonal::Off, "a kit repeats until set");
}

#[test]
fn machine_codes_are_stable() {
    let codes: Vec<u8> = Machine::ALL.iter().map(|m| m.code()).collect();
    assert_eq!(codes, (1..=12).collect::<Vec<u8>>());
    for m in Machine::ALL {
        assert_eq!(m.disk_code(), m.code());
        assert_eq!(Machine::from_disk_code(m.code()), Some(m));
    }
    assert_eq!(Machine::from_disk_code(0), None);
    assert_eq!(Machine::from_disk_code(13), None);
}

#[test]
fn machine_kernels() {
    use Kernel::*;
    let k: Vec<Kernel> = Machine::ALL.iter().map(|m| m.kernel()).collect();
    assert_eq!(
        k,
        [
            Tone, Tone, Tone, Burst, Metal, Metal, Metal, Metal, Burst, Plain, Plain, Plain
        ]
    );
}

#[test]
fn choke_codes_round_trip() {
    for (i, c) in ChokeGroup::ALL.iter().enumerate() {
        assert_eq!(c.disk_code() as usize, i);
        assert_eq!(ChokeGroup::from_disk_code(i as u8), Some(*c));
    }
    assert_eq!(ChokeGroup::from_disk_code(5), None);
}

#[test]
fn knob_unit_ends() {
    assert_eq!(KnobValue::new(U7::ZERO).unit().get(), 0.0);
    assert_eq!(KnobValue::new(U7::MAX).unit().get(), 1.0);
}

#[test]
fn knob_groups() {
    assert_eq!(
        Knob::MAIN,
        [Knob::A, Knob::B, Knob::C, Knob::D, Knob::E, Knob::F]
    );
    assert_eq!(Knob::MORE, [Knob::G, Knob::H]);
}

#[test]
fn knob_set_is_per_knob() {
    let mut s = KnobSet::ZERO;
    let v = KnobValue::new(U7::new(90).unwrap());
    s.set(Knob::G, v);
    assert_eq!(s.get(Knob::G), v);
    for k in [
        Knob::A,
        Knob::B,
        Knob::C,
        Knob::D,
        Knob::E,
        Knob::F,
        Knob::H,
    ] {
        assert_eq!(s.get(k), KnobValue::new(U7::ZERO));
    }
}

#[test]
fn pan_clamps() {
    assert_eq!(Pan::clamped(-100).get(), -64);
    assert_eq!(Pan::clamped(100).get(), 63);
    assert_eq!(Pan::new(64), None);
    assert_eq!(Pan::new(-64).map(Pan::get), Some(-64));
    assert_eq!(Pan::CENTRE.get(), 0);
}

#[test]
fn kit_pads_edit_in_place() {
    let mut kit = Kit::DEFAULT;
    let p = PadId::ALL[3];
    kit.pad_mut(p).set_machine(Machine::MetalHat.into());
    kit.pad_mut(p).set_choke(ChokeGroup::C);
    assert_eq!(kit.pad(p).machine(), Machine::MetalHat.into());
    assert_eq!(kit.pad(p).choke(), ChokeGroup::C);
    assert_eq!(kit.pad(PadId::ALL[2]), Kit::DEFAULT.pad(PadId::ALL[2]));
}

#[test]
fn hit_carries_its_pad() {
    let pad = Pad::new(
        Machine::ToneSnare,
        KnobSet::ZERO,
        U7::MAX,
        Pan::CENTRE,
        ChokeGroup::Off,
        Retrig::Stack,
    );
    let hit = DrumHit::new(PadId::ALL[1], pad, Transpose::new(12).unwrap()).unwrap();
    assert_eq!(hit.pad(), PadId::ALL[1]);
    assert_eq!(hit.params(), pad);
    assert_eq!(hit.pitch().get(), 12);
}

#[test]
fn hit_seeds_step_as_an_lcg() {
    assert_eq!(HitSeed::new(0).next().get(), 1_013_904_223);
    assert_eq!(
        HitSeed::new(1).next().get(),
        1_664_525u32.wrapping_add(1_013_904_223)
    );
    let mut s = HitSeed::new(7);
    let mut seen = Vec::new();
    for _ in 0..64 {
        s = s.next();
        assert!(!seen.contains(&s), "a seed repeats within 64 hits");
        seen.push(s);
    }
}
