//! The formant operators' stored parameters (spec § 2): their specs, INIT,
//! display, snaps and the Sound cap.

use chimera_core::addr::{BlockRef, Op};
use chimera_core::block::{Block, Change, Motion, ParamSpec, Smoothing, ValFmt};
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{OpKind, fits};
use chimera_core::dsp::algo::params::{
    ALGO_OP_SPECS, ALGO_SPECS, AlgoOpParams, AlgoParams, bw_hz, frmt_hz,
};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::vowel::{VSLOT_IDENTS, VowelSlot};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::factory::{Demo, demo_sound};
use chimera_core::ui::draw::text_width;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::theme;

#[test]
fn formant_params_follow_the_spec() {
    let p = AlgoOpParams::default();
    assert_eq!(
        (p.frmt, p.bw, p.skirt, p.key, p.res, p.vslot),
        (72, 72, 1, 0, 0, 0)
    );
    assert!((frmt_hz(72.0) - 800.0).abs() < 1e-3 && (frmt_hz(168.0) - 12_800.0).abs() < 0.1);
    assert!((bw_hz(72.0) - 160.0).abs() < 1e-3 && (bw_hz(216.0) - 10_240.0).abs() < 0.1);
    let ids: Vec<_> = ALGO_OP_SPECS[13..]
        .iter()
        .map(|s| (s.id.0, s.ident, s.max))
        .collect();
    assert_eq!(
        ids,
        [
            (13, "FRMT", 168.0),
            (14, "BW", 216.0),
            (15, "SKIRT", 3.0),
            (16, "KEY", 100.0),
            (17, "RES", 100.0),
            (18, "VSLOT", 4.0)
        ]
    );
    assert_eq!(
        (ALGO_SPECS[4].id.0, ALGO_SPECS[4].ident, ALGO_SPECS[4].max),
        (4, "VOWEL", 100.0)
    );
    assert!(ALGO_OP_SPECS[13..].iter().all(|s| !s.modulatable) && !ALGO_SPECS[4].modulatable);
}

#[test]
fn the_specs_declare_their_formats_and_motions() {
    let m = |s: &ParamSpec| s.motion();
    let knob = Motion::Continuous(Smoothing::KNOB);
    let note = Motion::Continuous(Smoothing::NoteOn);
    let s = &ALGO_OP_SPECS;
    assert_eq!(
        s[13].fmt,
        ValFmt::LogHz {
            base: 100,
            per_octave: 24
        }
    );
    assert_eq!(
        s[14].fmt,
        ValFmt::LogHz {
            base: 20,
            per_octave: 24
        }
    );
    assert_eq!(
        (s[15].fmt, s[16].fmt, s[17].fmt),
        (ValFmt::Int(3), ValFmt::Int(100), ValFmt::Int(100))
    );
    assert!(matches!(s[18].fmt, ValFmt::Names(n) if n == VSLOT_IDENTS));
    assert_eq!(ALGO_SPECS[4].fmt, ValFmt::Vowel);
    assert_eq!(
        (m(&s[13]), m(&s[14]), m(&ALGO_SPECS[4])),
        (knob, knob, knob)
    );
    assert_eq!((m(&s[16]), m(&s[17])), (note, note));
    assert_eq!(
        (m(&s[15]), m(&s[18])),
        (
            Motion::Stepped(Change::NoteOn),
            Motion::Stepped(Change::NoteOn)
        )
    );
}

#[test]
fn init_carries_vowel_a() {
    let p = AlgoParams::default();
    assert_eq!(p.ops.map(|o| o.frmt), [69, 83, 111, 123, 123, 123]);
    assert_eq!(p.vowel, 50);
}

#[test]
fn log_hz_and_vowel_read_as_the_spec_says() {
    let f = |v: f32, fmt| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, v, fmt);
        b.as_str().to_string()
    };
    let hz = ValFmt::LogHz {
        base: 100,
        per_octave: 24,
    };
    assert_eq!(
        (f(69.0, hz), f(83.0, hz), f(168.0, hz)),
        ("734 Hz".into(), "1.10 kHz".into(), "12.8 kHz".into())
    );
    assert_eq!(
        f(
            0.0,
            ValFmt::LogHz {
                base: 20,
                per_octave: 24
            }
        ),
        "20 Hz"
    );
    let v: Vec<_> = [0.0, 25.0, 50.0, 62.0, 63.0, 100.0]
        .map(|x| f(x, ValFmt::Vowel))
        .into();
    assert_eq!(v, ["U", "O", "A", "A", "E", "I"]);
}

#[test]
fn shift_snaps_each_octave_and_each_vowel() {
    let frmt = &ALGO_OP_SPECS[13];
    assert_eq!(frmt.snap_target(72.0, 1), 96.0);
    assert_eq!(frmt.snap_target(72.0, -1), 48.0);
    assert_eq!(frmt.snap_target(73.0, -1), 72.0);
    assert_eq!(frmt.snap_target(0.0, -1), 0.0);
    assert_eq!(frmt.snap_target(168.0, 1), 168.0);
    assert_eq!(ALGO_OP_SPECS[14].snap_target(0.0, 1), 24.0);
    let vowel = &ALGO_SPECS[4];
    assert_eq!(vowel.snap_target(50.0, 1), 75.0);
    assert_eq!(vowel.snap_target(37.0, -1), 25.0);
    assert_eq!(vowel.snap_target(100.0, 1), 100.0);
}

#[test]
fn vowel_slots_have_their_frozen_codes() {
    let slots = [
        VowelSlot::Auto,
        VowelSlot::F1,
        VowelSlot::F2,
        VowelSlot::F3,
        VowelSlot::Off,
    ];
    for (c, s) in slots.into_iter().enumerate() {
        assert_eq!(
            (VowelSlot::from_code(c as u8), s.code()),
            (Some(s), c as u8)
        );
    }
    assert_eq!(VowelSlot::from_code(5), None);
    assert_eq!(VSLOT_IDENTS, ["AUTO", "F1", "F2", "F3", "OFF"]);
}

#[test]
fn the_new_params_read_and_write_through_the_views() {
    let mut p = AlgoParams::default();
    let mut v = p.op_mut(Op::C);
    for (id, val) in [(13, 99.0), (14, 150.0), (15, 3.0), (16, 40.0), (17, 70.0)] {
        v.set(chimera_core::block::ParamId(id), val);
        assert_eq!(v.get(chimera_core::block::ParamId(id)), val);
    }
    assert!(v.set_enum_code(AlgoOpParams::VSLOT, 3));
    assert!(!v.set_enum_code(AlgoOpParams::VSLOT, 5));
    assert_eq!(v.enum_ident(AlgoOpParams::VSLOT), Some("F3"));
    assert_eq!(p.ops[2].vslot, 3);
    p.set(AlgoParams::VOWEL, 10.0);
    assert_eq!(p.vowel, 10);
    assert_eq!(BlockRef::AlgoOp(Op::C).specs().len(), 19);
}

#[test]
fn every_new_label_fits_a_cell() {
    for s in ALGO_OP_SPECS[13..].iter().chain(&ALGO_SPECS[4..]) {
        let w = text_width(&theme::FONT_LABEL, s.label, theme::LABEL_TRACKING);
        assert!(w <= theme::CELL_BAR_W, "{} is {w} px", s.label);
    }
}

#[test]
fn skirt_0_bills_the_one_lane_term() {
    use chimera_core::dsp::algo::algorithms::AlgoId;
    let cost = |p: &AlgoParams| AlgoEngine::cost(p, &[false; OPS]).0;
    // T1 links 2 → 1; A1 links nothing.
    for (alg, linked) in [(AlgoId::T1, true), (AlgoId::A1, false)] {
        let mut p = AlgoParams::single(WaveId::W1);
        p.try_set_algs(alg, alg).unwrap();
        let table = cost(&p);
        p.try_set_kind(0, OpKind::Formant).unwrap();
        assert_eq!(p.linked().has(0), linked);
        let (one, two) = if linked {
            (
                AlgoEngine::COST_FRMT_ONE_LINKED,
                AlgoEngine::COST_FRMT_LINKED,
            )
        } else {
            (AlgoEngine::COST_FRMT_ONE, AlgoEngine::COST_FRMT)
        };
        let move_ = AlgoEngine::COST_FRMT_MOVE.0;
        p.ops[0].skirt = 0;
        assert_eq!(cost(&p), table - AlgoEngine::COST_OP.0 + one.0 + move_);
        p.ops[0].skirt = 1;
        assert_eq!(cost(&p), table - AlgoEngine::COST_OP.0 + two.0 + move_);
    }
}

#[test]
fn the_demo_sounds_fit_the_budget() {
    for d in [Demo::Choir, Demo::TalkBass, Demo::Breath] {
        let p = demo_sound(d).params.algo;
        assert!(fits(&p.kinds().all(), p.linked()));
    }
}
