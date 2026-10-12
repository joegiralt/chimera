//! The factory bank: eight Algo Sounds and an INIT per playable engine
//! (`EngineType::PLAYABLE`: no TAPE, ADR 0132), kept as the pool's own
//! bytes in `factory.bin` so NEW copies them, no decode. The authored
//! Sounds below check the blob (`GOLDEN_RECORD=1` rewrites it).
//!
//! Blob: a `u16` count, then per entry a `u16` length and that many bytes,
//! the padded name and the records `encode_sound` writes. All little-endian.

use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::project::EncodedSound;

pub const FACTORY_LEN: usize = 8;

pub static FACTORY_BLOB: &[u8] = include_bytes!("factory.bin");

/// Each entry's name and bytes (name first, as the pool holds them), in
/// slot order.
pub fn factory_entries() -> impl Iterator<Item = (SoundName, &'static [u8])> {
    const _: () = assert!(FACTORY_BLOB.len() >= 2, "factory.bin has a count");
    let count = usize::from(u16::from_le_bytes([FACTORY_BLOB[0], FACTORY_BLOB[1]]));
    let mut rest = &FACTORY_BLOB[2..];
    (0..count).map_while(move |_| {
        let len = usize::from(u16::from_le_bytes([*rest.first()?, *rest.get(1)?]));
        let bytes = rest.get(2..2 + len)?;
        rest = &rest[2 + len..];
        Some((EncodedSound::new(bytes).name(), bytes))
    })
}

/// Entry `i`, decoded: the factory Sounds, then INIT per engine.
pub(crate) fn new_sound(i: usize) -> Option<Sound> {
    let (_, bytes) = factory_entries().nth(i)?;
    let mut s = Sound::neutral(EngineType::Algo);
    EncodedSound::new(bytes)
        .decode_into(&mut s)
        .expect("the blob decodes");
    Some(s)
}

/// The factory Sound `i`, from the blob.
pub fn factory_sound(i: usize) -> Option<Sound> {
    if i < FACTORY_LEN { new_sound(i) } else { None }
}

/// A Sound as the pool holds it: padded name, then its records.
#[cfg(any(test, feature = "test-support"))]
pub fn encode_entry(s: &Sound) -> alloc::vec::Vec<u8> {
    use crate::storage::{RecordWriter, encode_sound};
    use chimera_hal::store::{ByteSink, StoreError};
    struct V(alloc::vec::Vec<u8>);
    impl ByteSink for V {
        fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
            self.0.extend_from_slice(b);
            Ok(())
        }
    }
    let mut v = V(s.name.padded().to_vec());
    encode_sound(s, &mut RecordWriter::over(&mut v)).expect("a Vec takes it all");
    v.0
}

#[cfg(any(test, feature = "test-support"))]
pub use authored::{Demo, authored_sound, demo_sound};

#[cfg(any(test, feature = "test-support"))]
mod authored {
    use super::*;
    use crate::addr::{BlockRef, ParamAddr};
    use crate::budget::FORMANT_N_UNLINKED;
    use crate::dsp::algo::algorithms::AlgoId;
    use crate::dsp::algo::kinds::{Kinds, OpKind};
    use crate::dsp::algo::params::{AlgoOpParams, AlgoParams, INIT_FRMT};
    use crate::dsp::algo::waves::WaveId;
    use crate::modulation::ModSource;

    /// FILTER_SPECS' top of range (params.rs): essentially out of the way, for
    /// the FM patches whose timbre is the operators', not the filter's.
    const OPEN: f32 = 20000.0;

    /// `env` is AR, D1R, D1L, D2R, RR.
    fn op(coarse: u8, level: u8, env: [u8; 5]) -> AlgoOpParams {
        let [ar, d1r, d1l, d2r, rr] = env;
        AlgoOpParams {
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

    /// The eight table Sounds sit on INIT's formants, so they stay sparse
    /// against the decode base.
    fn algo(
        a: AlgoId,
        b: AlgoId,
        morph: u8,
        kinds: Kinds,
        mut ops: [AlgoOpParams; 6],
    ) -> AlgoParams {
        for (o, f) in ops.iter_mut().zip(INIT_FRMT) {
            o.frmt = f;
        }
        AlgoParams::new(a, b, morph, kinds, ops)
    }

    const W1: OpKind = OpKind::Table(WaveId::W1);
    const W2: OpKind = OpKind::Table(WaveId::W2);
    const W3: OpKind = OpKind::Table(WaveId::W3);
    const SAW: OpKind = OpKind::Table(WaveId::SAW);
    const SQR: OpKind = OpKind::Table(WaveId::SQR);

    fn named(name: &str, algo: AlgoParams) -> Sound {
        let mut s = Sound::init(EngineType::Algo);
        s.name = SoundName::new(name).expect("a valid factory name");
        s.params.algo = algo;
        s
    }

    /// The Sounds as authored: what the blob must decode to.
    pub fn authored_sound(i: usize) -> Option<Sound> {
        // A silent operator, as the factory Sounds were written: RR 8, before
        // INIT's moved to 5 (ADR 0063).
        let off = AlgoOpParams {
            rr: 8,
            ..AlgoOpParams::default()
        };
        let sound = match i {
            0 => {
                let mut s = named(
                    "TX BASS",
                    algo(
                        AlgoId::T5,
                        AlgoId::T5,
                        0,
                        const { Kinds::new([W1, W1, W2, W1, W1, W1]) },
                        [
                            AlgoOpParams {
                                velocity: 2,
                                ..op(0, 99, [31, 9, 12, 4, 9])
                            },
                            AlgoOpParams {
                                feedback: 5,
                                velocity: 4,
                                ..op(4, 76, [31, 14, 3, 6, 9])
                            },
                            op(4, 80, [31, 10, 10, 4, 9]),
                            op(8, 60, [31, 16, 2, 6, 9]),
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
                        Kinds::TABLE,
                        [
                            AlgoOpParams {
                                velocity: 3,
                                ..op(4, 93, [31, 6, 9, 3, 8])
                            },
                            AlgoOpParams {
                                velocity: 5,
                                ..op(42, 58, [31, 18, 0, 0, 8])
                            },
                            AlgoOpParams {
                                velocity: 3,
                                ..op(4, 86, [31, 5, 10, 3, 8])
                            },
                            AlgoOpParams {
                                velocity: 5,
                                ..op(4, 66, [31, 9, 5, 3, 8])
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
                        Kinds::TABLE,
                        [
                            op(4, 99, [18, 5, 13, 2, 7]),
                            op(4, 72, [16, 6, 11, 2, 7]),
                            op(4, 64, [20, 4, 12, 2, 7]),
                            AlgoOpParams {
                                feedback: 6,
                                ..op(4, 58, [14, 6, 10, 2, 7])
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
                        Kinds::TABLE,
                        [
                            op(4, 93, [31, 4, 0, 0, 5]),
                            op(12, 72, [31, 6, 0, 0, 5]),
                            op(11, 82, [31, 5, 0, 0, 5]),
                            op(23, 66, [31, 7, 0, 0, 5]),
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
                        const { Kinds::new([SAW, SAW, W1, W1, W1, W1]) },
                        [
                            op(4, 90, [31, 0, 15, 0, 8]),
                            AlgoOpParams {
                                detune: 3,
                                ..op(4, 86, [31, 0, 15, 0, 8])
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
                let sqr = op(0, 95, [31, 8, 12, 0, 9]);
                let mut s = named(
                    "SQR BASS",
                    algo(
                        AlgoId::T1,
                        AlgoId::T1,
                        0,
                        const { Kinds::new([SQR, W1, W1, W1, W1, W1]) },
                        [sqr, off, off, off, off, off],
                    ),
                );
                (s.params.filter.cutoff, s.params.filter.resonance) = (900.0, 0.4);
                // DRIVE 0.3 trimmed for LEVEL 95's +0.01 dB: the clipper's input is unchanged.
                s.params.drive.drive = 0.299_496_26;
                // Its level from before INIT's moved to the median (ADR 0063).
                s.params.out.volume = 0.8;
                s
            }
            6 => {
                const COARSE: [u8; 6] = [4, 8, 10, 13, 16, 19];
                const LEVEL: [u8; 6] = [88, 80, 76, 72, 70, 66];
                const MORPH_BASE: u8 = 40;
                let ops = core::array::from_fn(|i| op(COARSE[i], LEVEL[i], [12, 0, 15, 0, 5]));
                let mut s = named(
                    "MORPH PAD",
                    algo(AlgoId::A1, AlgoId::A17, MORPH_BASE, Kinds::TABLE, ops),
                );
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
                        const { Kinds::new([W1, W2, W1, W1, W3, W1]) },
                        [
                            AlgoOpParams {
                                velocity: 3,
                                ..op(4, 94, [31, 6, 9, 3, 8])
                            },
                            op(8, 70, [31, 10, 4, 3, 8]),
                            op(4, 86, [31, 6, 9, 3, 8]),
                            AlgoOpParams {
                                feedback: 3,
                                ..op(13, 64, [31, 8, 6, 3, 8])
                            },
                            op(4, 76, [31, 6, 9, 3, 8]),
                            op(19, 60, [31, 9, 5, 3, 8]),
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

    /// The formant demo Sounds (spec § Demo clips), the voicing by
    /// `FORMANT_N_UNLINKED`: each is on A1, so no FRMT has a link into it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Demo {
        Choir,
        TalkBass,
        Breath,
    }

    const FRMT: OpKind = OpKind::Formant;
    const NOIS: OpKind = OpKind::Noise;
    const W1_: OpKind = OpKind::Table(WaveId::W1);

    // CHOIR: three FRMT and a NOIS, else two FRMT and two NOIS.
    const CHOIR_KINDS: Kinds = if FORMANT_N_UNLINKED >= 3 {
        Kinds::new([FRMT, FRMT, FRMT, NOIS, W1_, W1_])
    } else {
        Kinds::new([NOIS, FRMT, FRMT, NOIS, W1_, W1_])
    };
    const TALK_KINDS: Kinds = if FORMANT_N_UNLINKED >= 3 {
        Kinds::new([W1_, FRMT, FRMT, FRMT, W1_, W1_])
    } else {
        Kinds::new([W1_, FRMT, FRMT, NOIS, W1_, W1_])
    };
    const BREATH_KINDS: Kinds = Kinds::new([NOIS, NOIS, W1_, W1_, W1_, W1_]);

    /// `FRMT`, `BW` and `SLOT` are stored values (`frmt_hz`, `bw_hz`).
    fn band(level: u8, frmt: u8, bw: u8, slot: u8, env: [u8; 2]) -> AlgoOpParams {
        let [ar, rr] = env;
        AlgoOpParams {
            level,
            ar,
            rr,
            frmt,
            bw,
            vslot: slot,
            ..AlgoOpParams::default()
        }
    }

    // A's F1–F3 on the grid (INIT_FRMT), and the slot codes.
    const F1: u8 = 69;
    const F2: u8 = 83;
    const F3: u8 = 111;
    const S_AUTO: u8 = 0;
    const S_F1: u8 = 1;
    const S_F2: u8 = 2;
    const S_F3: u8 = 3;
    const S_OFF: u8 = 4;
    /// 113 Hz, 160 Hz and 3 kHz of width.
    const W_113: u8 = 60;
    const W_FMT: u8 = 72;
    const W_3K: u8 = 173;

    /// A named Algo Sound on A1 (all six carriers). Silent operators sit
    /// at LEVEL 0.
    fn demo(name: &str, kinds: Kinds, vowel: u8, ops: [AlgoOpParams; 4]) -> Sound {
        let off = AlgoOpParams::default();
        let [a, b, c, d] = ops;
        let mut p = AlgoParams::new(AlgoId::A1, AlgoId::A1, 0, kinds, [a, b, c, d, off, off]);
        p.vowel = vowel;
        named(name, p)
    }

    /// `s` with `src` routed into VOWEL by `amount`: VOWEL moves
    /// `amount / 127 · 100` at the source's full swing.
    fn vowel_route(mut s: Sound, src: ModSource, amount: i8) -> Sound {
        let vowel = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
        s.dest_registry
            .add(vowel, *b"ALGVOWEL")
            .expect("VOWEL is modulatable");
        let d = s.mod_state.push(vowel).expect("a free column");
        s.mod_state.set_amount(src.index(), d, amount);
        s
    }

    /// Operators 5 and 6 are unused. VOWEL moves: LFO 1 slowly across O–A
    /// on CHOIR, ENV 2 from U to A and back on TALK BASS (spec § Demo clips).
    pub fn demo_sound(d: Demo) -> Sound {
        let few = FORMANT_N_UNLINKED < 3;
        match d {
            Demo::Choir => {
                let pad = [14, 7];
                let ops = if few {
                    [
                        AlgoOpParams {
                            res: 70,
                            ..band(99, F1, W_113, S_F1, pad)
                        },
                        AlgoOpParams {
                            detune: -2,
                            ..band(99, F2, W_FMT, S_F2, pad)
                        },
                        AlgoOpParams {
                            detune: 2,
                            ..band(99, F2, W_FMT, S_F2, pad)
                        },
                        band(30, F3, W_3K, S_F3, pad),
                    ]
                } else {
                    [
                        band(99, F1, W_FMT, S_F1, pad),
                        AlgoOpParams {
                            detune: -2,
                            ..band(99, F2, W_FMT, S_F2, pad)
                        },
                        AlgoOpParams {
                            detune: 2,
                            ..band(99, F2, W_FMT, S_F2, pad)
                        },
                        band(30, F3, W_3K, S_F3, pad),
                    ]
                };
                // 37 ± 12.6: O (25) to A (50) and back, every 4 s.
                let mut s = vowel_route(demo("CHOIR", CHOIR_KINDS, 37, ops), ModSource::Lfo1, 16);
                s.params.lfos[0].rate = 0.25;
                s
            }
            Demo::TalkBass => {
                let hit = [31, 8];
                let (slots, last) = if few {
                    (
                        [S_F1, S_F2],
                        AlgoOpParams {
                            res: 70,
                            ..band(60, F3, W_3K, S_F3, hit)
                        },
                    )
                } else {
                    ([S_AUTO; 2], band(99, F3, W_FMT, S_AUTO, hit))
                };
                let body = AlgoOpParams {
                    coarse: 4,
                    level: 91,
                    ar: 31,
                    rr: 8,
                    ..AlgoOpParams::default()
                };
                let ops = [
                    body,
                    band(99, F1, W_FMT, slots[0], hit),
                    band(99, F2, W_FMT, slots[1], hit),
                    last,
                ];
                // ENV 2 to A (50.4) and down to U again under the held note:
                // "wow".
                let mut s = vowel_route(demo("TALK BASS", TALK_KINDS, 0, ops), ModSource::Env2, 64);
                s.params.envelopes[1].sustain = 0.0;
                s
            }
            Demo::Breath => {
                let slow = [10, 6];
                let ops = [
                    band(99, 86, 167, S_OFF, slow),
                    band(60, 135, 183, S_OFF, slow),
                    AlgoOpParams::default(),
                    AlgoOpParams::default(),
                ];
                demo("BREATH", BREATH_KINDS, 50, ops)
            }
        }
    }
}
