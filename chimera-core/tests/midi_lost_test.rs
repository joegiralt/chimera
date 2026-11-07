//! A source lost (DIN's sensing timed out, USB reset) says so: a toast
//! naming the port, as a load waits too.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::midi_in::{CtrlEvent, Port};
use chimera_core::project::PartId;
use chimera_core::ui::busy::ToastStep;
use chimera_hal::Ms;

fn toast_after(port: Port) -> ToastStep {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, PartId::ALL[0]);
    ui.hear_ccs(
        core::iter::once((port, CtrlEvent::SensingLost)),
        Ms(1_000),
        true,
    );
    ui.step_toast(0)
}

#[test]
fn lost_source_toasts_its_port() {
    for (port, text) in [(Port::Din, "DIN MIDI LOST"), (Port::Usb, "USB MIDI LOST")] {
        match toast_after(port) {
            ToastStep::Show(l) => assert_eq!(l.as_str(), text),
            s => panic!("no toast for {port:?}: {s:?}"),
        }
    }
}

/// Both lost in one pass: one toast at a time, the last heard (USB's ring
/// drains after DIN's).
#[test]
fn both_lost_in_a_pass_toasts_the_last() {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, PartId::ALL[0]);
    let both = [
        (Port::Din, CtrlEvent::SensingLost),
        (Port::Usb, CtrlEvent::SensingLost),
    ];
    ui.hear_ccs(both.into_iter(), Ms(1_000), true);
    match ui.step_toast(0) {
        ToastStep::Show(l) => assert_eq!(l.as_str(), "USB MIDI LOST"),
        s => panic!("no toast: {s:?}"),
    }
}
