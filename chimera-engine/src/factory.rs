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
pub use authored::authored_sound;

#[cfg(any(test, feature = "test-support"))]
mod authored {
    use super::*;
    use crate::addr::{BlockRef, ParamAddr};
    use crate::dsp::algo::algorithms::AlgoId;
    use crate::dsp::algo::kinds::{Kinds, OpKind};
    use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
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

    fn algo(a: AlgoId, b: AlgoId, morph: u8, kinds: Kinds, ops: [AlgoOpParams; 6]) -> AlgoParams {
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
}
