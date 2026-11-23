// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The factory controller maps (ADR 0323): one flat map an engine, named
//! after it (ALGO, MODAL, VA, TAPE, DRUM), written through the firmware's
//! own `ctl::save` from the param tables, so they can't drift. The cells
//! every engine reads come first, each on the same CC in every map; then
//! the engine's own cells. CCs are those a binding may take, less ADR
//! 0263's built-ins, the controls a player's gear sends by itself and the
//! CCs a factory Sound's own rows use; MIDI's undefined CCs go first.
//! Cells the built-in layer already plays (CUTOFF, RESONANCE, the sends)
//! are left out. Every row is absolute `Jump`, echo off.

use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::path::Path;

use chimera_core::ui::mod_grid::block_name;
use chimera_engine::addr::{BlockRef, Op, ParamAddr};
use chimera_engine::block::{Motion, ParamId};
use chimera_engine::dsp::modulator::{EnvSlot, LfoSlot};
use chimera_engine::dsp::tape_engine::machine::MACHINES;
use chimera_engine::midi_map::builtin::BUILT_IN;
use chimera_engine::midi_map::ctl::{self, ControllerMap, FileBuf, MAP_ROWS};
use chimera_engine::midi_map::{Binding, Feedback, Input, Mappable, Span};
use chimera_engine::name::SoundName;
use chimera_engine::params::{EngineType, FilterParams, OutParams};
use chimera_engine::storage::Card;
use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};
use chimera_hal::testkit::MemStore;

/// The engines with a factory map, in the pack's order.
pub const ENGINES: [EngineType; 5] = [
    EngineType::Algo,
    EngineType::Modal,
    EngineType::Va,
    EngineType::Tape,
    EngineType::Drum,
];

/// MIDI's undefined CCs: the first a factory map takes.
const UNDEFINED: [u8; 40] = [
    3, 9, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 85, 86, 87, 88, 89, 90, 102, 103,
    104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
];

/// Controls a player's gear sends by itself (the wheel, breath, foot,
/// pedals, portamento control), and CC 16-19, a factory Sound's own rows
/// (`midi_map::default_for`), which would shadow a map row on them.
const KEPT_OFF: [u8; 13] = [1, 2, 4, 16, 17, 18, 19, 65, 66, 67, 68, 69, 84];

/// The CCs a factory map's rows take, in order: undefined first, then the
/// rest a binding may take, less the built-ins and `KEPT_OFF`.
pub fn factory_ccs() -> Vec<u8> {
    let free = |n: &u8| Input::cc(*n).is_some() && !BUILT_IN.contains(n) && !KEPT_OFF.contains(n);
    let mut out: Vec<u8> = UNDEFINED.iter().copied().filter(free).collect();
    out.extend((0..=127u8).filter(|n| free(n) && !UNDEFINED.contains(n)));
    out
}

/// The cells the built-in layer plays on every Part (ADR 0263).
fn built_in(a: ParamAddr) -> bool {
    a == ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF)
        || a == ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE)
        || [OutParams::SEND1, OutParams::SEND2, OutParams::SEND3]
            .into_iter()
            .any(|p| a == ParamAddr::new(BlockRef::Out, p))
}

/// Block `b`'s mappable cells, in table order, the built-in layer's left
/// out.
fn cells_of(b: BlockRef) -> impl Iterator<Item = Mappable> {
    (0..=u8::MAX)
        .filter_map(move |p| Mappable::new(ParamAddr::new(b, ParamId(p))))
        .filter(|m| !built_in(m.addr()))
}

/// The cells every engine reads, in play order: ENV 1-2 (both ENV TYPEs'
/// cells and LEVEL), LFO 1-3, the filter, the FX slot, the pitch, the out:
/// knobs only, a switch left to the hand.
pub fn common() -> Vec<Mappable> {
    use chimera_engine::dsp::lfo::LfoParams as L;
    use chimera_engine::params::EnvParams as E;
    let env = [
        E::ATTACK,
        E::DECAY,
        E::SUSTAIN,
        E::RELEASE,
        E::RISE,
        E::FALL,
        E::SHAPE,
        E::TIME,
        E::LEVEL,
    ];
    let lfo = [L::RATE, L::DEPTH, L::OFFSET, L::RISE, L::FALL];
    let at = |b: BlockRef, ps: &[ParamId]| -> Vec<Mappable> {
        ps.iter()
            .filter_map(|&p| Mappable::new(ParamAddr::new(b, p)))
            .collect()
    };
    let mut out = Vec::new();
    for s in [EnvSlot::Env1, EnvSlot::Env2] {
        out.extend(at(BlockRef::Env(s), &env));
    }
    for s in [LfoSlot::Lfo1, LfoSlot::Lfo2, LfoSlot::Lfo3] {
        out.extend(at(BlockRef::Lfo(s), &lfo));
    }
    // The filter's, pitch's and out's knobs: a switch (TYPE, MODE, STEAL)
    // is the hand's.
    for b in [
        BlockRef::Filter,
        BlockRef::VoiceFx,
        BlockRef::Pitch,
        BlockRef::Out,
    ] {
        out.extend(cells_of(b).filter(|m| {
            b == BlockRef::VoiceFx
                || m.addr()
                    .spec()
                    .is_some_and(|s| matches!(s.motion(), Motion::Continuous(_)))
        }));
    }
    out
}

/// What fills a map past its engine's cells: ENV 3's.
fn fill() -> Vec<Mappable> {
    use chimera_engine::params::EnvParams as E;
    [
        E::ATTACK,
        E::DECAY,
        E::SUSTAIN,
        E::RELEASE,
        E::RISE,
        E::FALL,
        E::SHAPE,
        E::TIME,
        E::LEVEL,
    ]
    .into_iter()
    .filter_map(|p| Mappable::new(ParamAddr::new(BlockRef::Env(EnvSlot::Env3), p)))
    .collect()
}

/// Engine `e`'s own cells, in table order (ALGO: its voice, then
/// operators A to F). DRUM's pads and kit are no `Mappable` cells: its
/// map's own rows are the master FX a DRUM Part plays into.
fn own(e: EngineType) -> Vec<Mappable> {
    match e {
        // The voice, then each operator cell across the six operators, so
        // every operator's LEVEL and COARSE come before any's later cells.
        EngineType::Algo => {
            let ops = [Op::A, Op::B, Op::C, Op::D, Op::E, Op::F];
            let mut out: Vec<Mappable> = cells_of(BlockRef::Algo).collect();
            for p in 0..=u8::MAX {
                out.extend(ops.iter().filter_map(|&o| {
                    Mappable::new(ParamAddr::new(BlockRef::AlgoOp(o), ParamId(p)))
                }));
            }
            out
        }
        EngineType::Modal => cells_of(BlockRef::Modal).collect(),
        EngineType::Va => cells_of(BlockRef::Va).collect(),
        EngineType::Tape => cells_of(BlockRef::TapeEngine).collect(),
        EngineType::Drum => [
            BlockRef::Chorus,
            BlockRef::Delay,
            BlockRef::Reverb,
            BlockRef::Comp,
        ]
        .into_iter()
        .flat_map(cells_of)
        .collect(),
    }
}

/// The factory map for `e`: the common cells on the first CCs, then its
/// own, then ENV 3's, up to `MAP_ROWS` rows or the CCs.
pub fn map(e: EngineType) -> ControllerMap {
    let ccs = factory_ccs();
    let mut m = ControllerMap::EMPTY;
    let cells = common().into_iter().chain(own(e)).chain(fill());
    for (t, &cc) in cells.zip(&ccs).take(MAP_ROWS) {
        let input = Input::cc(cc).expect("a learnable CC");
        m.push(Binding {
            input,
            target: t,
            span: Span::FULL,
            feedback: Feedback::Off,
        })
        .expect("90 rows at most");
    }
    m
}

/// `e`'s map's name: the engine's.
pub fn name(e: EngineType) -> SoundName {
    let n = match e {
        EngineType::Algo => "ALGO",
        EngineType::Modal => "MODAL",
        EngineType::Va => "VA",
        EngineType::Tape => "TAPE",
        EngineType::Drum => "DRUM",
    };
    SoundName::new(n).expect("a map name")
}

/// `e`'s card file stem: `put` and the factory pack use it.
pub fn stem(e: EngineType) -> String {
    let (s, l) = ctl::stem(&name(e));
    String::from_utf8(s[..l].to_vec()).expect("ASCII")
}

/// `e`'s map's file, as `ctl::save` writes it on a fresh card: side A.
pub fn file(e: EngineType) -> Vec<u8> {
    let mut s = MemStore::new(1);
    let mut buf = Box::new(FileBuf::new());
    Card::new()
        .run(&mut s, |s, r| {
            Ok::<_, StoreError>(ctl::save(s, r, &name(e), &mut buf, &map(e)))
        })
        .expect("a card")
        .result
        .expect("run")
        .expect("saved");
    struct Sink(Vec<u8>);
    impl ReadSink for Sink {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
            self.0.extend_from_slice(b);
            ControlFlow::Continue(())
        }
    }
    let vol = s.mount().expect("mounted");
    let f = FileName::new(Dir::Maps, stem(e).as_bytes(), b"A").expect("a name");
    let mut k = Sink(Vec::new());
    s.read(vol, f, &mut k).expect("side A");
    k.0
}

/// `<dir>/CHIMERA/MAPS/<stem>.A` for each engine, as the card holds them.
pub fn write(dir: &Path) -> Result<(), String> {
    let d = dir.join("CHIMERA/MAPS");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    for e in ENGINES {
        let p = d.join(format!("{}.A", stem(e)));
        std::fs::write(&p, file(e)).map_err(|e| e.to_string())?;
        println!("{}", p.display());
    }
    Ok(())
}

/// A cell as the guide names it.
pub fn cell(t: Mappable) -> String {
    let a = t.addr();
    let spec = a.spec().expect("a cell");
    match a.block {
        BlockRef::Algo | BlockRef::Modal | BlockRef::Va => spec.label.to_string(),
        // Each machine's label for it.
        BlockRef::TapeEngine => {
            let on: Vec<String> = MACHINES
                .iter()
                .filter_map(|m| {
                    let c = m.deck.iter().chain(m.cells).find(|c| c.id == a.param)?;
                    Some(format!("{} {}", m.page, c.label))
                })
                .collect();
            if on.is_empty() {
                spec.label.to_string()
            } else {
                format!("{} ({})", spec.label, on.join(", "))
            }
        }
        b => format!("{} {}", block_name(b), spec.label),
    }
}

/// The guide's tables (docs/guide/MIDI.md), from the maps.
pub fn tables() -> String {
    let ccs = factory_ccs();
    let common = common();
    let mut s = String::from("Every engine's map:\n\n| CC | Cell |\n|---|---|\n");
    for (t, cc) in common.iter().zip(&ccs) {
        let _ = writeln!(s, "| {cc} | {} |", cell(*t));
    }
    for e in ENGINES {
        let m = map(e);
        let _ = write!(
            s,
            "\n{} (`{}.A`, {} rows), after those:\n\n| CC | Cell |\n|---|---|\n",
            name(e).as_str(),
            stem(e),
            m.len()
        );
        for b in m.rows().skip(common.len()) {
            let Input::Cc(c, _) = b.input else { continue };
            let _ = writeln!(s, "| {} | {} |", c.get(), cell(b.target));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_engine::midi_map::CcKind;
    use chimera_engine::params::EnvParams;
    use chimera_midi::take::Takeover;

    const GUIDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide/MIDI.md");

    #[test]
    fn factory_ccs_are_free() {
        let c = factory_ccs();
        let set: std::collections::BTreeSet<_> = c.iter().collect();
        assert_eq!(set.len(), c.len(), "a CC twice");
        assert!(c.len() >= 85, "{} CCs", c.len());
        for n in &c {
            assert!(Input::cc(*n).is_some(), "CC {n}");
            assert!(!BUILT_IN.contains(n) && !KEPT_OFF.contains(n), "CC {n}");
        }
        assert_eq!(&c[..UNDEFINED.len()], &UNDEFINED[..]);
    }

    /// Each file, read back through the firmware's loader: every row a
    /// live cell of its engine, absolute `Jump` on a factory CC, no CC or
    /// cell twice, and the map as rich as the CCs and cells allow.
    #[test]
    fn each_map_loads_and_every_row_plays() {
        let ccs = factory_ccs();
        for e in ENGINES {
            let mut s = MemStore::new(2);
            let vol = s.mount().unwrap();
            s.make_dir(vol, Dir::Chimera).unwrap();
            s.make_dir(vol, Dir::Maps).unwrap();
            let f = FileName::new(Dir::Maps, stem(e).as_bytes(), b"A").unwrap();
            let bytes = file(e);
            s.write(vol, f, &mut |k| k.put(&bytes)).unwrap();
            let mut back = ControllerMap::EMPTY;
            let mut buf = Box::new(FileBuf::new());
            let lost = Card::new()
                .run(&mut s, |s, r| {
                    Ok::<_, StoreError>(ctl::load(s, r, &name(e), &mut buf, &mut back))
                })
                .unwrap()
                .result
                .unwrap()
                .unwrap();
            assert_eq!(lost, 0, "{e:?}");
            assert_eq!(back, map(e), "{e:?}");
            let want = (common().len() + own(e).len() + fill().len())
                .min(ccs.len())
                .min(MAP_ROWS);
            assert_eq!(back.len(), want, "{e:?}");
            let mut seen_cc = std::collections::BTreeSet::new();
            let mut seen_t = std::collections::BTreeSet::new();
            for b in back.rows() {
                assert!(b.target.live(e), "{e:?}: {:?}", b.target);
                let Input::Cc(cc, CcKind::Absolute(Takeover::Jump)) = b.input else {
                    panic!("{e:?}: {:?}", b.input);
                };
                assert!(ccs.contains(&cc.get()));
                assert!(seen_cc.insert(cc.get()), "{e:?}: CC {} twice", cc.get());
                assert!(
                    seen_t.insert(format!("{:?}", b.target)),
                    "{e:?}: a cell twice"
                );
                assert_eq!(b.feedback, Feedback::Off);
                assert!(!built_in(b.target.addr()));
            }
        }
    }

    #[test]
    fn common_cells_share_a_cc() {
        let n = common().len();
        let first = map(EngineType::Algo);
        for e in ENGINES {
            let m = map(e);
            for i in 0..n {
                assert_eq!(m.get(i), first.get(i), "{e:?} row {i}");
            }
        }
    }

    #[test]
    fn both_env_types_are_mapped() {
        for e in ENGINES {
            let m = map(e);
            for slot in [EnvSlot::Env1, EnvSlot::Env2] {
                for p in [
                    EnvParams::ATTACK,
                    EnvParams::DECAY,
                    EnvParams::SUSTAIN,
                    EnvParams::RELEASE,
                    EnvParams::RISE,
                    EnvParams::FALL,
                    EnvParams::SHAPE,
                    EnvParams::TIME,
                ] {
                    let t = Mappable::new(ParamAddr::new(BlockRef::Env(slot), p)).unwrap();
                    assert!(m.position(t).is_some(), "{e:?}: {slot:?} {p:?}");
                }
            }
        }
    }

    #[test]
    fn guide_tables_are_current() {
        let doc = std::fs::read_to_string(GUIDE).unwrap();
        assert!(
            doc.contains(&tables()),
            "docs/guide/MIDI.md's factory map tables are stale: paste `just engine-maps --tables`"
        );
    }
}
