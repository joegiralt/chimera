// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ADR 0091: an ALGO note-on reads its envelope coefficients from tables
//! built at compile time for `SAMPLE_RATE`, bit for bit what `exp2` and the
//! divisions give. Its own binary, and one test: `COMPUTED` is a
//! process-wide count.

mod common;

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{COMPUTED, EnvCoefs, EnvRates};
use chimera_core::hw::{CPU_HZ_REV_V, SAMPLE_RATE, SampleBudget};
use chimera_core::params::{EngineType, ParamSnapshot};
use core::sync::atomic::Ordering::Relaxed;

fn bits(c: EnvCoefs) -> [u32; 5] {
    [c.attack_add, c.d1_log2, c.d1_level, c.d2_log2, c.rr_log2].map(f32::to_bits)
}

#[test]
fn note_ons_read_tables_that_match_the_formula() {
    a_voice_start_reads_the_tables();
    the_tables_match_the_formula_bit_for_bit();
}

/// Every rate of every stage, at every key and rate scaling: the table
/// gives exactly what the formula does.
fn the_tables_match_the_formula_bit_for_bit() {
    let sr = SAMPLE_RATE as f32;
    let zero = EnvRates {
        ar: 0,
        d1r: 0,
        d1l: 0,
        d2r: 0,
        rr: 0,
        rs: 0,
    };
    for rs in 0..4 {
        for n in 0..128 {
            let note = MidiNote::new(n).unwrap();
            for v in 0..32 {
                for r in [
                    EnvRates { ar: v, rs, ..zero },
                    EnvRates { d1r: v, rs, ..zero },
                    EnvRates {
                        d1l: v % 16,
                        rs,
                        ..zero
                    },
                    EnvRates { d2r: v, rs, ..zero },
                    EnvRates {
                        rr: v % 16,
                        rs,
                        ..zero
                    },
                ] {
                    let (t, f) = (EnvCoefs::new(r, note, sr), EnvCoefs::compute(r, note, sr));
                    assert_eq!(bits(t), bits(f), "{r:?} at {n}");
                }
            }
        }
    }
}

/// Chords of the default ALGO Sound through the `Instrument`, struck,
/// rendered and released: no note-on works its coefficients out.
fn a_voice_start_reads_the_tables() {
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *r.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Algo);
    let before = COMPUTED.load(Relaxed);
    for round in 0..3 {
        for n in [36, 48, 60, 64, 67, 72, 84, 96] {
            r.note_on(0, n + round);
        }
        r.render(4);
        for n in [36, 48, 60, 64, 67, 72, 84, 96] {
            r.note_off(0, n + round);
        }
        r.render(4);
    }
    assert_eq!(COMPUTED.load(Relaxed), before);
}
