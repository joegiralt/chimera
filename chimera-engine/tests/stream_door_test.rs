//! Streamed loads' bank-1 side (ADR 0137): open, `heads_for` and
//! `load_kind` reach bank 2's placement through the door's `LoadDoor`;
//! closed, nothing is placed, so a source too big for the arena is
//! refused, while one that fits still loads whole. The host builds its own
//! door (`open_host`). One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::tape_engine::TapeMachine;
use chimera_engine::dsp::tape_engine::params::TapeEngineParams;
use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::project::arena::Cost;
use chimera_engine::project::{LoadKind, Refusal, load_kind};
use chimera_engine::storage::{SourceHead, SourceRef};
use chimera_engine::stream::stage::heads_for;

const RATE: u16 = 24_000;

fn head(frames: u32) -> SourceHead {
    SourceHead {
        sref: SourceRef {
            crc: 1,
            frames,
            rate: RATE,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("TAPE").unwrap(),
    }
}

#[test]
fn loads_place_heads_open_and_refuse_closed() {
    let p = TapeEngineParams {
        machine: TapeMachine::Walk,
        direction: Direction::Forward,
        ..TapeEngineParams::default()
    };
    let long = head(30 * RATE as u32);
    let short = head(RATE as u32);
    let none = Cost::of(0);
    let all = Cost::of(usize::MAX / 2);

    DOOR_CELL.close();
    assert_eq!(heads_for(long.sref, &p), Err(Refusal::Closed));
    assert_eq!(
        load_kind(&long, none, &p),
        LoadKind::Refused(Refusal::Closed)
    );
    assert_eq!(load_kind(&short, all, &p), LoadKind::Whole);

    DOOR_CELL.open_host();
    let heads = heads_for(long.sref, &p).expect("ONE streams");
    assert_eq!(heads.as_slice().len(), 1);
    assert_eq!(load_kind(&long, none, &p), LoadKind::Streamed(heads));
    assert_eq!(load_kind(&short, all, &p), LoadKind::Whole);
    DOOR_CELL.close();
}
