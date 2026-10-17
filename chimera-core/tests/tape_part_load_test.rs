//! A project's TAPE Part loads as TAPE (ADR 0141): kept whole, saved back
//! whole, and while bank 2 isn't open, silent and reading NEEDS UPDATE.

mod common;
mod screen;

use chimera_core::bank2::{Bank2Status, NEEDS_UPDATE, NEEDS_UPDATE_CELL, Version, closed_text};
use chimera_core::block::Block;
use chimera_core::dsp::tape_engine::params::TapeEngineParams;
use chimera_core::params::EngineType;
use chimera_core::project::{Origin, PartFrom, PartId, PartSource, Project};
use chimera_core::ui::UiState;
use chimera_core::ui::components::header_fits;
use common::codec_util::{decode_project_lossy, encode_project};
use common::project::load;
use screen::*;

fn open() -> Bank2Status {
    Bank2Status::Open {
        version: Version::new("0.2.0 02e440e2"),
        seal: [1; 8],
    }
}

/// Part 2 TAPE, AGE turned: it loads as saved, not lossy, and a save
/// writes the file back byte for byte.
#[test]
fn a_tape_part_loads_as_tape_and_saves_back_whole() {
    let part = PartId::ALL[1];
    let (mut p, t) = Project::boxed();
    let src = PartSource {
        part,
        from: PartFrom::Init(EngineType::Tape),
    };
    load(&mut p, t, src).unwrap();
    p.edit_part(part)
        .sound
        .params
        .tape
        .set(TapeEngineParams::AGE, 0.4);
    let file = encode_project(&p);
    let (mut q, _) = Project::boxed();
    let lossy = decode_project_lossy(&file, &mut q).unwrap();
    assert!(!lossy, "nothing of a TAPE Part is dropped");
    let x = q.part(part);
    assert_eq!(x.sound.engine(), EngineType::Tape);
    assert!(x.sound.bits_eq(&p.part(part).sound));
    assert_eq!(x.origin(), Origin::Init(EngineType::Tape));
    assert_eq!(encode_project(&q), file, "the save writes it back whole");
}

/// The header's warning on `engine`'s home page, Part 1, under `bank2`.
fn warn(engine: EngineType, bank2: Option<Bank2Status>) -> Option<&'static str> {
    let mut ui = UiState::new();
    if let Some(s) = bank2 {
        ui.set_bank2(s);
    }
    load_init(&mut ui, engine);
    let h = ui.page_header();
    assert!(header_fits(&h), "{:?} {:?}", h.name.as_str(), h.warn);
    h.warn
}

#[test]
fn a_tape_part_reads_needs_update_while_bank2_is_closed() {
    use Bank2Status::{Empty, Unsealed};
    use EngineType::{Tape, Va};
    assert_eq!(warn(Tape, Some(Empty)), Some(NEEDS_UPDATE_CELL));
    assert_eq!(
        warn(Tape, Some(Unsealed)),
        Some(NEEDS_UPDATE_CELL),
        "a bank-2 build that was never sealed"
    );
    assert_eq!(warn(Tape, Some(open())), None);
    assert_eq!(warn(Tape, None), None, "no bank 2: TAPE plays from bank 1");
    assert_eq!(warn(Va, Some(Empty)), None, "VA plays from bank 1");
}

/// DECK's MODEL reads NEEDS UPDATE in the focus band, its last word in the
/// cell; open, its model.
#[test]
fn deck_reads_needs_update_closed() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::tape_engine::params::TapeEngineParams;
    let model = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);
    assert_eq!(
        closed_text(model, Some(Bank2Status::Empty)),
        Some(NEEDS_UPDATE)
    );
    assert_eq!(closed_text(model, Some(open())), None);
    assert_eq!(closed_text(model, None), None);
    let age = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::AGE);
    assert_eq!(closed_text(age, Some(Bank2Status::Empty)), None);
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        tape_deck(&mut ui, 0);
        settle(&mut ui);
        let h = ui.page_header();
        assert_eq!(h.name.as_str(), "DECK");
        (render_ui(&ui), h.warn)
    };
    let (none, empty, ok) = (
        shot(None),
        shot(Some(Bank2Status::Empty)),
        shot(Some(open())),
    );
    assert_eq!(
        none.0.hash(),
        ok.0.hash(),
        "open, DECK draws as it always did"
    );
    assert_eq!(empty.1, Some(NEEDS_UPDATE_CELL));
    assert!(first_cell_reads(&empty.0, NEEDS_UPDATE_CELL));
    assert!(first_cell_reads(&ok.0, "PLAY"));
}
