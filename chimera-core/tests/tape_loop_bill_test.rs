//! ADR 0211: LOOP, TRON and PING-PONG bill their loop by the output samples
//! a pass takes, each band at or over the unit's reading (TAPE COLD bench,
//! 2026-10-22), and admission books it at note-on and holds it.

mod common;
use common::InstRig;
use common::tape::{OneSource, held_plain, sine};

use chimera_core::dsp::tape_engine::heads::loop_extra;
use chimera_core::dsp::tape_engine::{COST_LOOP, COST_PING, COST_TRON, TapeMachine, Way};
use chimera_core::hw::{Cost, SampleBudget};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::stream::units::SourceRate;
use chimera_core::{MidiNote, Velocity};

/// The chain's 36 and TAPE's and AGE's 40 at ½, as the bench's rows ran.
const ROW: u32 = 76;

fn rate(hz: u16) -> SourceRate {
    SourceRate::new(hz).unwrap()
}

/// Each band's bill over the model's, a fade summing to 1 and an
/// equal-power one: a pass every 4, 16, 60 and 256 samples at `inc` 16.
#[test]
fn every_band_bills_its_pass_length() {
    let r = rate(24_000);
    let at = |w, spw: u32, fade| loop_extra(w, 16 * spw, fade, r, 16.0).0;
    let power = 481; // 20 ms at 24 kHz, and a frame
    for (w, tiny) in [(Way::Loop, 300), (Way::Tron, 440), (Way::Ping, 80)] {
        assert_eq!(at(w, 4, 0), tiny, "{w:?} under 16");
    }
    for (spw, sum, pow) in [
        (16, 80, 740),
        (59, 80, 740),
        (60, 20, 200),
        (255, 20, 200),
        (256, 0, 50),
        (1_023, 0, 50),
        (1_024, 0, 20),
        (4_096, 0, 10),
        (30_000, 0, 10),
    ] {
        for w in [Way::Loop, Way::Tron] {
            assert_eq!(at(w, spw, 0), sum, "{w:?} {spw} summing");
            assert_eq!(at(w, spw, power), pow, "{w:?} {spw} equal-power");
        }
        assert_eq!(at(Way::Ping, spw, 0), 0, "PING {spw}");
    }
    for w in [Way::One, Way::Scrub, Way::Wave, Way::Stretch] {
        assert_eq!(at(w, 4, power), 0, "{w:?} pays no loop");
    }
}

/// Every TAPE COLD row the unit read is billed at or over it.
#[test]
fn every_band_holds_the_unit_s_reading() {
    let row = |model: Cost, w, frames, fade, hz, inc| {
        model.0 + loop_extra(w, frames, fade, rate(hz), inc).0 + ROW
    };
    for (name, billed, read) in [
        // A 64-frame source at 24 kHz, `inc` 16: a pass every 4 samples.
        (
            "LOOP 64",
            row(COST_LOOP, Way::Loop, 64, 32, 24_000, 16.0),
            859,
        ),
        (
            "REV 64",
            row(COST_LOOP, Way::Loop, 64, 32, 24_000, 16.0),
            892,
        ),
        (
            "PING 64",
            row(COST_PING, Way::Ping, 64, 0, 24_000, 16.0),
            643,
        ),
        (
            "TRON",
            row(COST_TRON, Way::Tron, 64, 32, 24_000, 16.0),
            1_035,
        ),
        // The shortest legal loop, 256 frames at 8 kHz: every 16 samples.
        (
            "LOOP 256",
            row(COST_LOOP, Way::Loop, 256, 128, 8_000, 16.0),
            664,
        ),
        (
            "REV 256",
            row(COST_LOOP, Way::Loop, 256, 128, 8_000, 16.0),
            667,
        ),
        (
            "PING 256",
            row(COST_PING, Way::Ping, 256, 0, 8_000, 16.0),
            556,
        ),
        // SIZE 0.55 of 0.5 s at 24 kHz, XFADE ½: 999 frames, a 20.8 ms
        // equal-power fade, a pass every 62 samples.
        (
            "LOOP 40",
            row(COST_LOOP, Way::Loop, 999, 499, 24_000, 16.0),
            775,
        ),
    ] {
        assert!(billed >= read, "{name}: {billed} < {read}");
    }
}

/// A LOOP note on a 64-frame source at the top of the keyboard books its
/// tiny band at note-on and holds it every block; one played at its root
/// on a long source books the longest band's (its fade 20 ms or more,
/// equal-power).
#[test]
fn admission_books_the_loop_and_holds_it() {
    let tiny = held_plain(&sine(24_000, 16.0, 64), 24_000);
    let long = held_plain(&sine(24_000, 100.0, 24_000), 24_000);
    for (held, note, extra) in [(&tiny, 120, 300), (&long, 60, 10)] {
        let mut src = OneSource::new(held);
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
        let mut p: ParamSnapshot = common::init_params(EngineType::Tape);
        p.tape.machine = TapeMachine::Walk;
        p.tape.set_way(Way::Loop);
        (p.tape.size, p.tape.xfade) = (0.5, 0.5);
        *rig.shared.parts[0].params = p.clone();
        rig.shared.parts[0].source = Some(src.id);
        let ch = rig.shared.parts[0].mix.channel;
        let ev = NoteEvent {
            channel: ch,
            note: MidiNote::new(note).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        rig.inst.handle_with(ev, &rig.shared, &src.view());
        let one = rig.inst.allocator().sounding_cost();
        let model = chimera_core::dsp::voice::Voice::cost(&p, &rig.shared.parts[0].mod_state);
        assert!(
            one.0 >= model.0 + extra,
            "note {note}: booked {one:?}, model {model:?} + {extra}"
        );
        for _ in 0..8 {
            let view = src.view();
            rig.inst.render_with(
                &mut rig.fx,
                &mut rig.dac,
                &rig.shared,
                &mut rig.scope,
                &view,
            );
        }
        let held_on = rig.inst.allocator().sounding_cost();
        assert!(
            held_on.0 >= model.0 + extra,
            "note {note}: held {held_on:?}"
        );
    }
}
