//! SUGGEST, the factory controller map (ADR 0226): a shared section and
//! one an engine, on MIDI's undefined CCs, written through the firmware's
//! own map codec from the param tables, so it can't drift. CC 20 is each
//! engine's main timbre knob, 21 its second, 29 its character; the
//! shared rows sit on 14, 15 and 85-88. None is a built-in CC (ADR 0263).
//! Every row is absolute, `Jump`: learn's own default.

use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::path::Path;

use chimera_core::ui::mod_grid::block_name;
use chimera_engine::addr::{BlockRef, Op, ParamAddr};
use chimera_engine::block::ParamId;
use chimera_engine::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_engine::dsp::lfo::LfoParams;
use chimera_engine::dsp::modal::ModalParams;
use chimera_engine::dsp::modulator::{EnvSlot, LfoSlot};
use chimera_engine::dsp::tape_engine::machine::MACHINES;
use chimera_engine::dsp::tape_engine::params::TapeEngineParams as T;
use chimera_engine::dsp::va::VaParams;
use chimera_engine::dsp::voice_fx::params::VoiceFxParams;
use chimera_engine::midi_map::ctl::{self, ControllerMap};
use chimera_engine::midi_map::{Input, Mappable, MidiMap};
use chimera_engine::name::SoundName;
use chimera_engine::params::{EngineType, EnvParams, PitchParams};
use chimera_engine::storage::Card;
use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};
use chimera_hal::testkit::MemStore;

pub const NAME: &str = "SUGGEST";

type Row = (u8, BlockRef, ParamId);

/// Every engine reads these.
pub const SHARED: [Row; 6] = [
    (14, BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::RATE),
    (15, BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH),
    // How far ENV 2 moves what it drives: the filter, in most Sounds.
    (85, BlockRef::Env(EnvSlot::Env2), EnvParams::LEVEL),
    (86, BlockRef::Pitch, PitchParams::GLIDE_TIME),
    (87, BlockRef::VoiceFx, VoiceFxParams::P1),
    (88, BlockRef::VoiceFx, VoiceFxParams::MIX),
];

const fn op(o: Op) -> BlockRef {
    BlockRef::AlgoOp(o)
}

/// Each engine's ten, CC 20-29 by role.
pub const ENGINES: [(EngineType, [Row; 10]); 4] = [
    (
        EngineType::Algo,
        [
            (20, BlockRef::Algo, AlgoParams::MORPH),
            (21, BlockRef::Algo, AlgoParams::VOWEL),
            // TX81Z's op 2-4: the modulators of its algorithms.
            (22, op(Op::B), AlgoOpParams::LEVEL),
            (23, op(Op::C), AlgoOpParams::LEVEL),
            (24, op(Op::D), AlgoOpParams::LEVEL),
            (25, op(Op::B), AlgoOpParams::COARSE),
            (26, op(Op::C), AlgoOpParams::COARSE),
            (27, op(Op::D), AlgoOpParams::COARSE),
            (28, op(Op::B), AlgoOpParams::DETUNE),
            (29, op(Op::D), AlgoOpParams::FEEDBACK),
        ],
    ),
    (
        EngineType::Modal,
        [
            (20, BlockRef::Modal, ModalParams::STRUCTURE),
            (21, BlockRef::Modal, ModalParams::BRIGHT),
            (22, BlockRef::Modal, ModalParams::POS),
            (23, BlockRef::Modal, ModalParams::EXCITE),
            (24, BlockRef::Modal, ModalParams::BODY),
            (25, BlockRef::Modal, ModalParams::COLOR),
            (26, BlockRef::Modal, ModalParams::BURST),
            (27, BlockRef::Modal, ModalParams::FORCE),
            (28, BlockRef::Modal, ModalParams::COUPLE),
            (29, BlockRef::Modal, ModalParams::HALO),
        ],
    ),
    (
        EngineType::Va,
        [
            (20, BlockRef::Va, VaParams::WAVE1),
            (21, BlockRef::Va, VaParams::WAVE2),
            (22, BlockRef::Va, VaParams::SHAPE1),
            (23, BlockRef::Va, VaParams::SHAPE2),
            (24, BlockRef::Va, VaParams::AMOUNT),
            (25, BlockRef::Va, VaParams::MODE),
            (26, BlockRef::Va, VaParams::SUB),
            (27, BlockRef::Va, VaParams::NOISE),
            (28, BlockRef::Va, VaParams::FINE2),
            (29, BlockRef::Va, VaParams::STAB),
        ],
    ),
    (
        EngineType::Tape,
        [
            (20, BlockRef::TapeEngine, T::POSITION),
            (21, BlockRef::TapeEngine, T::MOTION),
            (22, BlockRef::TapeEngine, T::SIZE),
            (23, BlockRef::TapeEngine, T::DRAG),
            (24, BlockRef::TapeEngine, T::DRIFT),
            (25, BlockRef::TapeEngine, T::LENGTH),
            (26, BlockRef::TapeEngine, T::GRAIN),
            (27, BlockRef::TapeEngine, T::SPRAY),
            (28, BlockRef::TapeEngine, T::TAPE),
            (29, BlockRef::TapeEngine, T::AGE),
        ],
    ),
];

fn rows(r: &[Row]) -> MidiMap {
    let mut m = MidiMap::EMPTY;
    for &(cc, b, id) in r {
        let a = ParamAddr::new(b, id);
        let t = Mappable::new(a).unwrap_or_else(|| panic!("{a:?} is not mappable"));
        m.learn(Input::cc(cc).expect("a learnable CC"), t)
            .expect("16 rows at most");
    }
    m
}

/// The map, each section through `absorb` as a save would put it.
pub fn map() -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    m.absorb(&rows(&SHARED), EngineType::Algo)
        .expect("the shared rows fit");
    for (e, r) in &ENGINES {
        m.absorb(&rows(r), *e).expect("a layer of 16 at most");
    }
    m
}

pub fn name() -> SoundName {
    SoundName::new(NAME).expect("a map name")
}

/// The card file's stem: `put` and the factory pack use it.
pub fn stem() -> String {
    let (s, l) = ctl::stem(&name());
    String::from_utf8(s[..l].to_vec()).expect("ASCII")
}

/// The map's file, as `ctl::save` writes it on a fresh card: side A.
pub fn file() -> Vec<u8> {
    let mut s = MemStore::new(1);
    Card::new()
        .run(&mut s, |s, r| {
            Ok::<_, StoreError>(ctl::save(s, r, &name(), &map()))
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
    let f = FileName::new(Dir::Maps, stem().as_bytes(), b"A").expect("a name");
    let mut k = Sink(Vec::new());
    s.read(vol, f, &mut k).expect("side A");
    k.0
}

/// `<dir>/CHIMERA/MAPS/<stem>.A`, as the card holds it.
pub fn write(dir: &Path) -> Result<(), String> {
    let d = dir.join("CHIMERA/MAPS");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let p = d.join(format!("{}.A", stem()));
    std::fs::write(&p, file()).map_err(|e| e.to_string())?;
    println!("{}", p.display());
    Ok(())
}

fn cell(b: BlockRef, id: ParamId) -> String {
    let spec = ParamAddr::new(b, id).spec().expect("a cell");
    match b {
        BlockRef::Algo | BlockRef::Modal | BlockRef::Va => spec.label.to_string(),
        // Each machine's label for it.
        BlockRef::TapeEngine => {
            let on: Vec<String> = MACHINES
                .iter()
                .filter_map(|m| {
                    let c = m.deck.iter().chain(m.cells).find(|c| c.id == id)?;
                    Some(format!("{} {}", m.page, c.label))
                })
                .collect();
            if on.is_empty() {
                spec.label.to_string()
            } else {
                format!("{} ({})", spec.label, on.join(", "))
            }
        }
        _ => format!("{} {}", block_name(b), spec.label),
    }
}

/// The guide's tables (docs/guide/MIDI.md), from the rows.
pub fn tables() -> String {
    let mut s = String::from("| CC | Shared |\n|---|---|\n");
    for &(cc, b, id) in &SHARED {
        let _ = writeln!(s, "| {cc} | {} |", cell(b, id));
    }
    s.push_str("\n| CC | ALGO | MODAL | VA | TAPE |\n|---|---|---|---|---|\n");
    for i in 0..10 {
        let _ = write!(s, "| {} |", ENGINES[0].1[i].0);
        for (_, r) in &ENGINES {
            let _ = write!(s, " {} |", cell(r[i].1, r[i].2));
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_engine::midi_map::CcKind;
    use chimera_midi::take::Takeover;

    const GUIDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide/MIDI.md");

    /// ADR 0263's built-in layer (its own table) and MIDI's defined CCs.
    fn spoken(cc: u8) -> bool {
        let undefined = matches!(cc, 3 | 9 | 14 | 15 | 20..=31 | 85..=90 | 102..=119);
        !undefined || chimera_engine::midi_map::builtin::BUILT_IN.contains(&cc)
    }

    /// The file, read back through the firmware's loader: every row a
    /// live cell of its engine, on an undefined CC, absolute and `Jump`,
    /// and each engine's layer whole.
    #[test]
    fn the_file_loads_and_every_row_plays() {
        let mut s = MemStore::new(2);
        let vol = s.mount().unwrap();
        s.make_dir(vol, Dir::Chimera).unwrap();
        s.make_dir(vol, Dir::Maps).unwrap();
        let f = FileName::new(Dir::Maps, stem().as_bytes(), b"A").unwrap();
        let bytes = file();
        s.write(vol, f, &mut |k| k.put(&bytes)).unwrap();
        let mut back = ControllerMap::EMPTY;
        let lost = Card::new()
            .run(&mut s, |s, r| {
                Ok::<_, StoreError>(ctl::load(s, r, &name(), &mut back))
            })
            .unwrap()
            .result
            .unwrap()
            .unwrap();
        assert_eq!(lost, 0);
        assert_eq!(back, map());
        assert_eq!(back.section(None).len(), SHARED.len());
        // DRUM has no SUGGEST section: ADR 0266's flat maps give it its own.
        for e in ENGINES.map(|(e, _)| e) {
            let (layer, lost) = back.layer(e);
            assert_eq!((layer.len(), lost), (16, 0), "{e:?}");
            for b in layer.entries() {
                assert!(b.target.live(e), "{e:?}: {:?}", b.target);
                let Input::Cc(cc, CcKind::Absolute(Takeover::Jump)) = b.input else {
                    panic!("{e:?}: {:?}", b.input);
                };
                assert!(!spoken(cc.get()), "CC {} is spoken for", cc.get());
            }
            let ccs: std::collections::BTreeSet<_> = layer
                .entries()
                .iter()
                .map(|b| match b.input {
                    Input::Cc(c, _) => c.get(),
                    _ => 0,
                })
                .collect();
            assert_eq!(ccs.len(), 16, "{e:?}: a CC twice");
        }
    }

    #[test]
    fn same_cc_same_role() {
        for (_, r) in &ENGINES {
            let ccs: Vec<u8> = r.iter().map(|x| x.0).collect();
            assert_eq!(ccs, (20..30).collect::<Vec<_>>());
        }
    }

    #[test]
    fn guide_tables_are_current() {
        let doc = std::fs::read_to_string(GUIDE).unwrap();
        assert!(
            doc.contains(&format!("`{}.A`", stem())),
            "the guide names the file"
        );
        assert!(
            doc.contains(&tables()),
            "docs/guide/MIDI.md's SUGGEST tables are stale: paste `just suggest-map --tables`"
        );
    }
}
