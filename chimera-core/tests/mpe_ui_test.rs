//! MPE's UI side (#845, ADR 0252): an MPE Configuration Message sets a
//! zone, RPN 0 on a member channel its bend, a member channel's controls
//! never reach a Part's map, and MIDI CONFIG › MPE sets the zones by hand;
//! the publish carries them to the audio.

#![cfg(feature = "midi-map")]

mod common;
mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::instrument::AudioShared;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::mpe::{DEFAULT_ZONE_BEND, Zones};
use chimera_core::part::PartParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_hal::{EncoderId, Ms};
use chimera_midi::ctl::{Ctl, DataValue, Sel};
use chimera_midi::value::{U7, U14};
use screen::{Input, feed};

const BEND: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::BEND);

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn rpn(c: u8, n: u16, v: u8) -> CtrlEvent {
    CtrlEvent::Ctl(
        ch(c),
        Ctl::Data {
            sel: Sel::Rpn(U14::new(n).unwrap()),
            value: DataValue::Coarse(U7::new(v).unwrap()),
        },
    )
}

fn nrpn(c: u8, v: u8) -> CtrlEvent {
    let u7 = |v| U7::new(v).unwrap();
    CtrlEvent::Ctl(
        ch(c),
        Ctl::Data {
            sel: Sel::Nrpn(U14::from_parts(u7(10), u7(0))),
            value: DataValue::Coarse(u7(v)),
        },
    )
}

struct Rig {
    ui: Box<UiState>,
    now: u32,
}

impl Rig {
    fn new() -> Self {
        let mut ui = Box::new(screen::stack_ui());
        for p in PartId::ALL {
            screen::init_part(&mut ui, p);
        }
        Rig { ui, now: 1_000 }
    }

    fn pass(&mut self, ev: &[CtrlEvent]) {
        self.now += 4;
        self.ui.hear_ccs(ev.iter().copied(), Ms(self.now), true);
    }

    fn zones(&self) -> Zones {
        self.ui.midi().mpe
    }

    fn bend(&self, p: usize) -> f32 {
        let part = self.ui.project().read_part(PartId::ALL[p]);
        part.block(BlockRef::Part).unwrap().get(BEND.param)
    }
}

#[test]
fn a_configuration_message_sets_its_zone_and_the_publish_carries_it() {
    let mut r = Rig::new();
    assert_eq!(r.zones(), Zones::OFF, "MPE is off out of the box");
    r.pass(&[rpn(0, 6, 15)]);
    assert_eq!(r.zones().lower(), 15);
    let mut shared = Box::new(AudioShared::default());
    r.ui.publish_ui(&mut shared);
    assert_eq!(shared.mpe, r.zones());
    // The upper zone takes channels back; RPN 6 elsewhere is no message.
    r.pass(&[rpn(15, 6, 3), rpn(4, 6, 9)]);
    assert_eq!((r.zones().lower(), r.zones().upper()), (11, 3));
    r.pass(&[rpn(0, 6, 0)]);
    assert_eq!(
        (r.zones().lower(), r.zones().upper()),
        (0, 3),
        "0 turns it off"
    );
}

#[test]
fn rpn0_on_a_member_sets_its_zone_s_bend_and_on_the_master_the_part_s() {
    let mut r = Rig::new();
    r.pass(&[rpn(0, 6, 7)]);
    let part2 = r.bend(1);
    r.pass(&[rpn(1, 0, 24)]);
    assert_eq!(r.zones().lower_bend(), 24);
    assert_eq!(r.bend(1), part2, "Part 2 on channel 2 is not asked");
    r.pass(&[rpn(0, 0, 12)]);
    assert_eq!(r.bend(0), 12.0, "the master's is Part 1's BEND");
    assert_eq!(r.zones().lower_bend(), 24);
    // A new configuration resets the member bend to the spec's.
    r.pass(&[rpn(0, 6, 7)]);
    assert_eq!(r.zones().lower_bend(), DEFAULT_ZONE_BEND);
}

#[test]
fn a_member_channel_s_controls_never_reach_a_part_s_map() {
    let mut r = Rig::new();
    let rev = r.ui.project().rev();
    r.pass(&[rpn(0, 6, 15), nrpn(2, 0)]);
    r.pass(&[]);
    assert_eq!(r.ui.project().rev(), rev, "Part 3 on channel 3 holds");
    r.pass(&[rpn(0, 6, 0), nrpn(2, 0)]);
    for _ in 0..50 {
        r.pass(&[]);
    }
    assert_ne!(r.ui.project().rev(), rev, "MPE off, its NRPN moves it");
}

#[test]
fn midi_config_mpe_sets_the_zones_by_hand() {
    use chimera_core::ui::block_registry::SYS_MPE;
    let mut r = Rig::new();
    screen::to_leaf(&mut r.ui, &["MIDI CONFIG", "MPE"]);
    assert_eq!(r.ui.page_def().id, SYS_MPE.id);
    feed(&mut r.ui, Input::turn(EncoderId::A, 15));
    assert_eq!(r.zones().lower(), 15);
    feed(&mut r.ui, Input::turn(EncoderId::B, -24));
    assert_eq!(r.zones().lower_bend(), 24);
    feed(&mut r.ui, Input::turn(EncoderId::C, 4));
    assert_eq!(
        (r.zones().lower(), r.zones().upper()),
        (10, 4),
        "the zones never overlap"
    );
    feed(&mut r.ui, Input::turn(EncoderId::D, 48));
    assert_eq!(r.zones().upper_bend(), 96);
}
