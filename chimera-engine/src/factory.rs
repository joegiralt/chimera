//! The factory bank: eight Algo Sounds, the four formant demos (CHOIR,
//! TALK BASS, BREATH, VOX), then an INIT per playable engine
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

pub const FACTORY_LEN: usize = 12;

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
#[cfg(any(test, feature = "test-support", feature = "demo-sounds"))]
pub use authored::{Demo, demo_sound};

#[cfg(any(test, feature = "test-support", feature = "demo-sounds"))]
// With `demo-sounds` alone only the demos are read.
#[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
mod authored {
    use super::*;
    use crate::addr::{BlockRef, ParamAddr};
    use crate::budget::FORMANT_N_UNLINKED;
    use crate::dsp::algo::algorithms::AlgoId;
    use crate::dsp::algo::kinds::{Kinds, OpKind};
    use crate::dsp::algo::params::{AlgoOpParams, AlgoParams, INIT_FRMT};
    use crate::dsp::algo::waves::WaveId;
    use crate::dsp::filter::FilterKind;
    use crate::dsp::lfo::LfoParams;
    use crate::dsp::modulator::{FuncParams, LfoForm, LfoType};
    use crate::modulation::{FixedCol, ModSource};
    use crate::params::{EnvParams, PitchParams};

    /// FILTER_SPECS' top of range (params.rs): essentially out of the way, for
    /// the FM patches whose timbre is the operators', not the filter's.
    const OPEN: f32 = 20000.0;

    /// The FX slot as SOFT PRE at LEVEL ½: the old DRV at `drive`, `tone`,
    /// bit for bit.
    fn soft(p: &mut crate::params::ParamSnapshot, drive: f32, tone: f32) {
        use crate::dsp::voice_fx::params::FxType;
        p.fx.set_type(FxType::Soft);
        (p.fx.pots[0], p.fx.pots[1]) = (drive, tone);
    }

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
        let mut s = Sound::plain_init(EngineType::Algo);
        s.map = crate::midi_map::default_for(EngineType::Algo);
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
                soft(&mut s.params, 0.299_496_26, 0.5);
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
            8..=11 => demo_sound(Demo::ALL[i - 8]),
            _ => return None,
        };
        Some(sound)
    }

    /// The formant demo Sounds (spec § Demo clips), voiced against FS1R
    /// references: each is on A1, so no FRMT has a link into it, and below
    /// `FORMANT_N_UNLINKED` 3 the third FRMT is a NOIS band.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Demo {
        Choir,
        TalkBass,
        Breath,
        Vox,
    }

    impl Demo {
        /// In bank order: factory 8–11.
        pub const ALL: [Demo; 4] = [Demo::Choir, Demo::TalkBass, Demo::Breath, Demo::Vox];
    }

    const FRMT: OpKind = OpKind::Formant;
    const NOIS: OpKind = OpKind::Noise;
    const F3K: OpKind = if FORMANT_N_UNLINKED >= 3 { FRMT } else { NOIS };
    const W1_: OpKind = OpKind::Table(WaveId::W1);

    const CHOIR_KINDS: Kinds = Kinds::new([FRMT, FRMT, F3K, NOIS, NOIS, W1_]);
    const TALK_KINDS: Kinds = Kinds::new([W1_, FRMT, FRMT, F3K, NOIS, W1_]);
    const BREATH_KINDS: Kinds = Kinds::new([FRMT, FRMT, F3K, NOIS, W1_, W1_]);
    const VOX_KINDS: Kinds = Kinds::new([FRMT, FRMT, F3K, SQR, NOIS, W1_]);

    /// A formant or noise operator: stored LEVEL, FRMT, BW and SLOT, and
    /// AR, D1R, D1L, RR.
    fn band(level: u8, frmt: u8, bw: u8, slot: u8, env: [u8; 4]) -> AlgoOpParams {
        let [ar, d1r, d1l, rr] = env;
        AlgoOpParams {
            level,
            ar,
            d1r,
            d1l,
            rr,
            frmt,
            bw,
            vslot: slot,
            ..AlgoOpParams::default()
        }
    }

    // A's F1–F3 on the grid (INIT_FRMT), and the slot codes. FRMT is
    // `100·2^(v/24)` Hz, BW `20·2^(v/24)` Hz.
    const F1: u8 = 69;
    const F2: u8 = 83;
    const F3: u8 = 111;
    /// 3 kHz: the singer's formant over A's F3.
    const F3_HI: u8 = 118;
    const S_AUTO: u8 = 0;
    const S_F1: u8 = 1;
    const S_F2: u8 = 2;
    const S_F3: u8 = 3;
    const S_OFF: u8 = 4;

    /// A named Algo Sound on A1 (all six carriers) at `volume`.
    fn demo(name: &str, kinds: Kinds, vowel: u8, volume: f32, ops: [AlgoOpParams; 6]) -> Sound {
        let mut p = AlgoParams::new(AlgoId::A1, AlgoId::A1, 0, kinds, ops);
        p.vowel = vowel;
        let mut s = named(name, p);
        s.params.out.volume = volume;
        s
    }

    /// `src` routed into `a` by `amount`, as the matrix routes it.
    fn route(s: &mut Sound, a: ParamAddr, label: &[u8; 8], src: ModSource, amount: i8) {
        s.dest_registry
            .add(a, *label)
            .expect("modulatable, and room");
        let d = s.mod_state.find(a).or_else(|| s.mod_state.push(a));
        s.mod_state
            .set_amount(src.index(), d.expect("a free column"), amount);
    }

    const VOWEL: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
    const FINE: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::FINE);
    const PITCH: ParamAddr = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);

    fn op_addr(op: usize, p: crate::block::ParamId) -> ParamAddr {
        ParamAddr::new(BlockRef::AlgoOp(crate::addr::Op::ALL[op]), p)
    }

    /// A CLASSIC LFO: `shape` (SHAPE_NAMES' order), Hz, retriggered or free.
    fn lfo(shape: u8, rate: f32, retrig: bool, phase: f32) -> LfoParams {
        LfoParams {
            rate,
            shape,
            sync: u8::from(retrig),
            phase_offset: phase,
            ..LfoParams::default()
        }
    }

    /// FUNC's LFV: a slewed random walk at `rise` (RATE's position), each
    /// step at most `delta`.
    fn wander(rise: f32, delta: f32) -> LfoParams {
        LfoParams {
            lfo_type: LfoType::Func,
            func: FuncParams {
                lfo_form: LfoForm::Lfv,
                rise,
                fall: delta,
                shape: 0.5,
                ..FuncParams::LFO
            },
            ..LfoParams::default()
        }
    }

    /// ENV A at MED: attack, decay and release positions, and sustain.
    fn env(attack: f32, decay: f32, sustain: f32, release: f32) -> EnvParams {
        EnvParams {
            attack,
            decay,
            sustain,
            release,
            ..EnvParams::default()
        }
    }

    // ENV A's MED positions (`law::speed_ranges`): attack `2 ms·2^(12.29p)`,
    // decay and release `3.5 ms·2^(11.48p)`.
    const A_2MS: f32 = 0.0;
    const A_1S: f32 = 0.730;
    const D_80MS: f32 = 0.393;
    const D_150MS: f32 = 0.472;
    const D_300MS: f32 = 0.559;
    const D_400MS: f32 = 0.596;
    const D_1S: f32 = 0.711;
    // FUNC's RATE position (`law::B_RATE`, 0.05 Hz·2^(13.97p)): LFV holds
    // still for its first cycle, and each voice starts its own, so a slower
    // walk never moves under a short note.
    const R_1HZ: f32 = 0.309;

    /// The demo Sound `d`, as the bank holds it.
    pub fn demo_sound(d: Demo) -> Sound {
        let lvl = |o: usize| op_addr(o, AlgoOpParams::LEVEL);
        let bw = |o: usize| op_addr(o, AlgoOpParams::BW);
        let off = AlgoOpParams::default();
        match d {
            // FS-Choir and Spacy Aaah: three formants with a bright third,
            // a 5.5 Hz vibrato of about ±6 cents. Here each chord swells
            // open (ENV 2), drifts (LFO 1), and each note of it sings its
            // own vowel (NOTE); breath gusts over the top (LFV).
            Demo::Choir => {
                let pad = [12, 0, 15, 6];
                let ops = [
                    AlgoOpParams {
                        detune: -2,
                        key: 20,
                        ..band(99, F1, 60, S_F1, pad)
                    },
                    AlgoOpParams {
                        detune: 2,
                        key: 20,
                        ..band(95, F2, 72, S_F2, pad)
                    },
                    AlgoOpParams {
                        detune: 1,
                        skirt: 2,
                        ..band(78, F3_HI, 110, S_F3, pad)
                    },
                    // Breath over the singer's formant, and a soft band
                    // on F2 that follows the vowel.
                    band(60, 128, 173, S_OFF, [10, 0, 15, 6]),
                    band(66, F2, 110, S_F2, [10, 0, 15, 6]),
                    off,
                ];
                let mut s = demo("CHOIR", CHOIR_KINDS, 15, 1.0, ops);
                let p = &mut s.params;
                p.lfos[0] = lfo(1, 0.2, false, 0.0);
                p.lfos[1] = lfo(0, 5.4, false, 0.0);
                p.lfos[2] = wander(R_1HZ, 0.5);
                p.envelopes[1] = env(A_1S, D_1S, 1.0, D_1S);
                p.filter.cutoff = 7_000.0;
                p.filter.level = 0.6;
                p.out.sends = [0.35, 0.0, 0.3];
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Env2, 38);
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Lfo1, 10);
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Note, 100);
                route(&mut s, FINE, b"PTCHFINE", ModSource::Lfo2, 9);
                route(&mut s, FINE, b"PTCHFINE", ModSource::Lfo3, 3);
                route(&mut s, lvl(3), b"OP4LEVEL", ModSource::Lfo3, 13);
                route(&mut s, bw(1), b"OP2 BW\0\0", ModSource::Lfo3, 6);
                s
            }
            // FS-Choir's and Vox Tron's lines keep their formants high and
            // steady. Here each note says "wow": ENV 2 opens U to past A and
            // back, a sample and hold gives each note its own vowel, a noise
            // chiff starts it, and ENV 3 scoops its pitch; buzzy F1 (SKIRT 0,
            // FDBK) through a driven ladder.
            Demo::TalkBass => {
                let hit = [31, 0, 15, 8];
                let ops = [
                    AlgoOpParams {
                        coarse: 4,
                        level: 88,
                        ar: 31,
                        rr: 8,
                        ..AlgoOpParams::default()
                    },
                    AlgoOpParams {
                        skirt: 0,
                        feedback: 2,
                        key: 30,
                        ..band(99, F1, 72, S_AUTO, hit)
                    },
                    AlgoOpParams {
                        feedback: 1,
                        key: 30,
                        ..band(99, F2, 84, S_AUTO, hit)
                    },
                    AlgoOpParams {
                        skirt: 2,
                        key: 30,
                        res: 60,
                        ..band(85, F3, 96, S_AUTO, hit)
                    },
                    band(82, 123, 167, S_OFF, [31, 20, 0, 8]),
                    off,
                ];
                let mut s = demo("TALK BASS", TALK_KINDS, 0, 95.0 / 128.0, ops);
                let p = &mut s.params;
                p.lfos[0] = lfo(4, 0.5, true, 0.99);
                p.envelopes[0] = env(A_2MS, D_300MS, 0.3, D_150MS);
                p.envelopes[1] = env(A_2MS, D_400MS, 0.15, D_150MS);
                p.envelopes[2] = env(A_2MS, D_80MS, 0.0, D_80MS);
                p.filter.set_kind(FilterKind::Ladder);
                p.filter.cutoff = 1_800.0;
                p.filter.resonance = 0.35;
                p.filter.level = 0.65;
                soft(p, 0.35, 0.6);
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Env2, 90);
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Lfo1, 35);
                route(&mut s, PITCH, b"PITCH\0\0\0", ModSource::Env3, 5);
                s.mod_state
                    .set_fixed(ModSource::Env1.index(), FixedCol::Freq, 40);
                s
            }
            // FairyVoice's air: a pure "oo" whose F1 follows the note, a soft
            // second formant, and a shimmering formant near 3.6 kHz with a
            // breath band under it that follows velocity. Each note scoops
            // up into pitch and swells; the vowel and the breath only drift.
            Demo::Breath => {
                let swell = [9, 0, 15, 7];
                let ops = [
                    AlgoOpParams {
                        skirt: 2,
                        key: 50,
                        ..band(99, F1 - 6, 60, S_F1, swell)
                    },
                    AlgoOpParams {
                        key: 50,
                        ..band(80, 96, 84, S_F2, swell)
                    },
                    AlgoOpParams {
                        detune: 2,
                        ..band(68, 124, 118, S_OFF, swell)
                    },
                    AlgoOpParams {
                        velocity: 3,
                        ..band(82, 124, 150, S_OFF, swell)
                    },
                    off,
                    off,
                ];
                let mut s = demo("BREATH", BREATH_KINDS, 5, 1.0, ops);
                let p = &mut s.params;
                p.lfos[0] = lfo(0, 11.0, false, 0.0);
                p.lfos[2] = wander(R_1HZ, 0.4);
                p.envelopes[2] = env(A_2MS, D_80MS, 0.0, D_80MS);
                p.filter.cutoff = 12_000.0;
                p.filter.level = 0.62;
                p.out.sends = [0.25, 0.0, 0.35];
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Lfo3, 8);
                route(&mut s, lvl(2), b"OP3LEVEL", ModSource::Lfo1, 4);
                route(&mut s, lvl(3), b"OP4LEVEL", ModSource::Lfo3, 5);
                route(&mut s, PITCH, b"PITCH\0\0\0", ModSource::Env3, -10);
                s
            }
            // Vox Tron's and Beep VoX's lead: formants that follow the key,
            // buzzy (SKIRT 0, FDBK), a square under them. Each note blips
            // down in pitch (ENV 3) and closes from I toward O (ENV 2), as
            // Beep VoX's do; a 8 Hz sample and hold babbles the vowel, and
            // the line is driven into a dirty filter, with a delay.
            Demo::Vox => {
                let gate = [31, 0, 15, 9];
                let ops = [
                    AlgoOpParams {
                        skirt: 0,
                        key: 35,
                        ..band(99, F1, 72, S_F1, gate)
                    },
                    AlgoOpParams {
                        skirt: 0,
                        feedback: 3,
                        key: 35,
                        ..band(97, F2, 72, S_F2, gate)
                    },
                    AlgoOpParams {
                        key: 35,
                        ..band(78, F3_HI, 96, S_F3, gate)
                    },
                    AlgoOpParams {
                        coarse: 4,
                        level: 60,
                        ar: 31,
                        rr: 9,
                        ..AlgoOpParams::default()
                    },
                    band(76, 123, 167, S_OFF, [31, 20, 0, 9]),
                    off,
                ];
                let mut s = demo("VOX", VOX_KINDS, 30, 1.0, ops);
                let p = &mut s.params;
                p.lfos[0] = lfo(4, 8.0, false, 0.0);
                p.lfos[1] = lfo(0, 6.0, false, 0.0);
                p.envelopes[1] = env(A_2MS, D_300MS, 0.0, D_150MS);
                p.envelopes[2] = env(A_2MS, D_80MS, 0.0, D_80MS);
                p.filter.set_kind(FilterKind::Dirty);
                p.filter.cutoff = 2_500.0;
                p.filter.resonance = 0.35;
                soft(p, 0.45, 0.55);
                p.out.sends = [0.0, 0.3, 0.15];
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Env2, 75);
                route(&mut s, VOWEL, b"ALGVOWEL", ModSource::Lfo1, 20);
                route(&mut s, PITCH, b"PITCH\0\0\0", ModSource::Env3, 16);
                route(&mut s, FINE, b"PTCHFINE", ModSource::Lfo2, 9);
                s
            }
        }
    }
}
