//! Bindings behind bank 2's door (`MidiUiDoor`, ADR 0198, 0217), under
//! `bank2` on a host, where the table answers only while the door cell is
//! open. Closed, what MIDI in heard is drained and dropped: nothing moves,
//! nothing panics (spec § 13). Open again, it moves. One test: the door is
//! a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support", feature = "midi-map"))]

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::bank2::DOOR_CELL;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_hal::{EncoderId, Ms};
use chimera_midi::ctl::{Ctl, DataValue, Dir, FreeCc, Sel};
use chimera_midi::value::{U7, U14};
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

fn cutoff(ui: &UiState) -> f32 {
    let p = ui.project().read_part(P1);
    p.block(BlockRef::Filter).unwrap().get(CUTOFF.param)
}

#[test]
fn closed_drains_and_moves_nothing() {
    // Open before NEW is built, as the boot does.
    DOOR_CELL.open_host();
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    let ch = ui.project().part(P1).mix.channel;
    let u7 = |v| U7::new(v).unwrap();
    let cc74 = |v| {
        CtrlEvent::Ctl(
            ch,
            Ctl::Cc {
                cc: FreeCc::new(74).unwrap(),
                value: u7(v),
            },
        )
    };
    let every = [
        cc74(0),
        CtrlEvent::Ctl(
            ch,
            Ctl::Data {
                sel: Sel::Nrpn(U14::from_parts(u7(10), u7(0))),
                value: DataValue::Coarse(u7(0)),
            },
        ),
        CtrlEvent::Ctl(
            ch,
            Ctl::Data {
                sel: Sel::Rpn(U14::new(0).unwrap()),
                value: DataValue::Coarse(u7(12)),
            },
        ),
        CtrlEvent::Ctl(
            ch,
            Ctl::Step {
                sel: Sel::Rpn(U14::new(0).unwrap()),
                dir: Dir::Up,
            },
        ),
        CtrlEvent::Ctl(ch, Ctl::Pressure(u7(0))),
        CtrlEvent::SensingLost,
    ];
    let start = cutoff(&ui);
    assert_ne!(start, CUTOFF.spec().unwrap().min);

    DOOR_CELL.close();
    let rev = ui.project().rev();
    let mut now = 1_000;
    for _ in 0..50 {
        now += 4;
        ui.hear_ccs(every.into_iter(), Ms(now), true);
    }
    // A hand's turn closed: nothing to stop, nothing breaks.
    feed(&mut ui, Input::turn(EncoderId::F, 1));
    let rev2 = ui.project().rev();
    for _ in 0..20 {
        now += 4;
        ui.hear_ccs(every.into_iter(), Ms(now), true);
    }
    assert_eq!(ui.project().part(P1).mix.channel, ch);
    assert_eq!(ui.project().rev(), rev2, "closed, nothing heard is applied");
    assert!(rev2 >= rev);

    DOOR_CELL.open_host();
    for _ in 0..40 {
        now += 4;
        ui.hear_ccs(core::iter::once(cc74(0)), Ms(now), true);
    }
    assert_eq!(cutoff(&ui), CUTOFF.spec().unwrap().min, "open, CC 74 moves");
}
