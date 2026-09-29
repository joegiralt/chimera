//! The factory bank: eight Algo Sounds.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::waves::WaveId;
use crate::modulation::ModSource;
use crate::params::EngineType;
use crate::preset::{NAME_LEN, Sound, SoundPool};

pub const FACTORY_LEN: usize = 8;

/// FILTER_SPECS' top of range (params.rs): essentially out of the way, for
/// the FM patches whose timbre is the operators', not the filter's.
const OPEN: f32 = 20000.0;

/// `env` is AR, D1R, D1L, D2R, RR.
fn op(wave: WaveId, coarse: u8, level: u8, env: [u8; 5]) -> AlgoOpParams {
    let [ar, d1r, d1l, d2r, rr] = env;
    AlgoOpParams {
        wave: wave.get(),
        coarse,
        level,
        ar,
        d1r,
        d1l,
        d2r,
        rr,
        ..AlgoOpParams::default()
    }
}

fn algo(a: AlgoId, b: AlgoId, morph: u8, ops: [AlgoOpParams; 6]) -> AlgoParams {
    AlgoParams {
        alg_a: a.get(),
        alg_b: b.get(),
        morph,
        transpose: 0,
        ops,
    }
}

fn named(name: &str, algo: AlgoParams) -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.name = [0; NAME_LEN];
    s.name[..name.len()].copy_from_slice(name.as_bytes());
    s.params.algo = algo;
    s
}

pub fn factory_sound(i: usize) -> Option<Sound> {
    let off = AlgoOpParams::default();
    let w1 = WaveId::W1;
    let sound = match i {
        0 => {
            let mut s = named(
                "TX BASS",
                algo(
                    AlgoId::T5,
                    AlgoId::T5,
                    0,
                    [
                        AlgoOpParams {
                            velocity: 2,
                            ..op(w1, 0, 99, [31, 9, 12, 4, 9])
                        },
                        AlgoOpParams {
                            feedback: 5,
                            velocity: 4,
                            ..op(w1, 4, 76, [31, 14, 3, 6, 9])
                        },
                        op(WaveId::W2, 4, 80, [31, 10, 10, 4, 9]),
                        op(w1, 8, 60, [31, 16, 2, 6, 9]),
                        off,
                        off,
                    ],
                ),
            );
            s.params.filter.cutoff = OPEN;
            s.params.out.volume = 0.470_479_88; // 0.8 × old / new norm (ADR 0049)
            s
        }
        1 => {
            let mut s = named(
                "TX EPIANO",
                algo(
                    AlgoId::T5,
                    AlgoId::T5,
                    0,
                    [
                        AlgoOpParams {
                            velocity: 3,
                            ..op(w1, 4, 93, [31, 6, 9, 3, 8])
                        },
                        AlgoOpParams {
                            velocity: 5,
                            ..op(w1, 42, 58, [31, 18, 0, 0, 8])
                        },
                        AlgoOpParams {
                            velocity: 3,
                            ..op(w1, 4, 86, [31, 5, 10, 3, 8])
                        },
                        AlgoOpParams {
                            velocity: 5,
                            ..op(w1, 4, 66, [31, 9, 5, 3, 8])
                        },
                        off,
                        off,
                    ],
                ),
            );
            s.params.filter.cutoff = OPEN;
            // Carriers 6 LEVELs (4.5 dB) down: the SVF sees the old level to
            // 0.27 dB; VOLUME takes the rest (ADR 0049).
            s.params.out.volume = 0.775_405_6;
            s
        }
        2 => {
            let mut s = named(
                "TX BRASS",
                algo(
                    AlgoId::T3,
                    AlgoId::T3,
                    0,
                    [
                        op(w1, 4, 99, [18, 5, 13, 2, 7]),
                        op(w1, 4, 72, [16, 6, 11, 2, 7]),
                        op(w1, 4, 64, [20, 4, 12, 2, 7]),
                        AlgoOpParams {
                            feedback: 6,
                            ..op(w1, 4, 58, [14, 6, 10, 2, 7])
                        },
                        off,
                        off,
                    ],
                ),
            );
            s.params.filter.cutoff = OPEN;
            s.params.out.volume = 0.565_685_33; // 0.8 × old / new norm (ADR 0049)
            s
        }
        3 => {
            let mut s = named(
                "TX BELL",
                algo(
                    AlgoId::T5,
                    AlgoId::T5,
                    0,
                    [
                        op(w1, 4, 93, [31, 4, 0, 0, 5]),
                        op(w1, 12, 72, [31, 6, 0, 0, 5]),
                        op(w1, 11, 82, [31, 5, 0, 0, 5]),
                        op(w1, 23, 66, [31, 7, 0, 0, 5]),
                        off,
                        off,
                    ],
                ),
            );
            s.params.filter.cutoff = OPEN;
            // Carriers 6 LEVELs down, as TX EPIANO's (ADR 0049).
            s.params.out.volume = 0.775_405_6;
            s
        }
        4 => {
            let mut s = named(
                "SAW LEAD",
                algo(
                    AlgoId::A2,
                    AlgoId::A2,
                    0,
                    [
                        op(WaveId::SAW, 4, 90, [31, 0, 15, 0, 8]),
                        AlgoOpParams {
                            detune: 3,
                            ..op(WaveId::SAW, 4, 86, [31, 0, 15, 0, 8])
                        },
                        off,
                        off,
                        off,
                        off,
                    ],
                ),
            );
            (s.params.filter.cutoff, s.params.filter.resonance) = (6000.0, 0.3);
            // Carriers 9 LEVELs (6.7 dB) down: the SVF sees the old level to
            // 0.24 dB; VOLUME takes the rest (ADR 0049).
            s.params.out.volume = 0.778_224_6;
            s
        }
        5 => {
            // LEVEL 95: -3.0 dB, T1's old 1 / √2, into DRIVE (ADR 0049).
            let sqr = op(WaveId::SQR, 0, 95, [31, 8, 12, 0, 9]);
            let mut s = named(
                "SQR BASS",
                algo(AlgoId::T1, AlgoId::T1, 0, [sqr, off, off, off, off, off]),
            );
            (s.params.filter.cutoff, s.params.filter.resonance) = (900.0, 0.4);
            // DRIVE 0.3 trimmed for LEVEL 95's +0.01 dB: the clipper's input is unchanged.
            s.params.drive.drive = 0.299_496_26;
            s
        }
        6 => {
            const COARSE: [u8; 6] = [4, 8, 10, 13, 16, 19];
            const LEVEL: [u8; 6] = [88, 80, 76, 72, 70, 66];
            const MORPH_BASE: u8 = 40;
            let ops = core::array::from_fn(|i| op(w1, COARSE[i], LEVEL[i], [12, 0, 15, 0, 5]));
            let mut s = named("MORPH PAD", algo(AlgoId::A1, AlgoId::A17, MORPH_BASE, ops));
            s.params.lfos[0].rate = 0.2;
            s.params.filter.cutoff = 4000.0;
            s.params.out.volume = 0.380_297_63; // at MORPH 40 (ADR 0049)
            let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
            if s.dest_registry.add(morph, *b"ALGMORPH").is_ok()
                && let Some(d) = s.mod_state.push(morph)
            {
                // Full LFO swing (± `depth`) must land inside MORPH's 0..=127
                // range from its base, or the sweep clips flat at an end.
                let headroom = MORPH_BASE.min(127 - MORPH_BASE) as f32;
                let amount = (headroom / s.params.lfos[0].depth) as i8;
                s.mod_state.set_amount(ModSource::Lfo1.index(), d, amount); // LFO 1 → MORPH
            }
            s
        }
        7 => {
            let mut s = named(
                "MORPH KEYS",
                algo(
                    AlgoId::T5,
                    AlgoId::A12,
                    64,
                    [
                        AlgoOpParams {
                            velocity: 3,
                            ..op(w1, 4, 94, [31, 6, 9, 3, 8])
                        },
                        op(WaveId::W2, 8, 70, [31, 10, 4, 3, 8]),
                        op(w1, 4, 86, [31, 6, 9, 3, 8]),
                        AlgoOpParams {
                            feedback: 3,
                            ..op(w1, 13, 64, [31, 8, 6, 3, 8])
                        },
                        op(WaveId::W3, 4, 76, [31, 6, 9, 3, 8]),
                        op(w1, 19, 60, [31, 9, 5, 3, 8]),
                    ],
                ),
            );
            s.params.filter.cutoff = OPEN;
            s.params.out.volume = 0.506_361_07; // 0.8 × old / new norm (ADR 0049)
            s
        }
        _ => return None,
    };
    Some(sound)
}

pub fn load_factory(pool: &mut SoundPool) {
    for i in 0..FACTORY_LEN {
        if let Some(s) = factory_sound(i) {
            pool.store(i, s);
        }
    }
}
