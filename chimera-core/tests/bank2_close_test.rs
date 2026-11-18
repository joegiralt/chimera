//! A bank-2 write releases TAPE's voices first; the other engines play on.
//! Only a gate build's `bank2 erase` closes a live door.
#![cfg(feature = "bank2-gate")]

mod common;
use common::tape::{OneSource, held_plain, sine};
use common::{InstRig, init_params};

use chimera_core::hw::SampleBudget;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::project::{LoadGate, LoadLink};
use chimera_core::source_table::GateView;
use chimera_core::{MidiNote, Velocity};

#[test]
fn closing_bank_two_releases_tape_and_leaves_algo_playing() {
    let mut src = OneSource::new(&held_plain(&sine(24_000, 100.0, 24_000), 24_000));
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    *rig.shared.parts[0].params = init_params(EngineType::Tape);
    rig.shared.parts[0].source = Some(src.id);
    *rig.shared.parts[1].params = init_params(EngineType::Algo);
    let ev = |ch, kind| NoteEvent {
        channel: ch,
        note: MidiNote::new(60).unwrap(),
        kind,
    };
    let on = NoteKind::On(Velocity::DEFAULT);
    let (c0, c1) = (
        rig.shared.parts[0].mix.channel,
        rig.shared.parts[1].mix.channel,
    );
    assert_ne!(c0, c1, "one channel a Part");
    let link = LoadLink::new();
    let mut gate = LoadGate::new();
    let mut block = |rig: &mut InstRig, src: &mut OneSource| {
        let view: GateView<'_> = src.view();
        gate.before_block(&link, &mut rig.inst, &rig.shared);
        rig.inst.render_with(
            &mut rig.fx,
            &mut rig.dac,
            &rig.shared,
            &mut rig.scope,
            &view,
        );
    };
    {
        let view = src.view();
        rig.inst.handle_with(ev(c0, on), &rig.shared, &view);
        rig.inst.handle_with(ev(c1, on), &rig.shared, &view);
    }
    for _ in 0..20 {
        block(&mut rig, &mut src);
    }
    let tape_voices = |rig: &InstRig| {
        let parts = rig.inst.sounding_parts();
        rig.inst
            .active()
            .iter()
            .zip(parts)
            .filter(|&(&a, p)| a && p == chimera_core::project::PartId::ALL[0])
            .count()
    };
    assert_eq!(tape_voices(&rig), 1, "TAPE sounds first");
    assert!(rig.inst.part_bus(0).iter().any(|x| x.abs() > 1e-4));

    // The audio runs a block each time the UI waits.
    let ok = {
        let mut ui = chimera_core::ui::UiState::new();
        let mut blocks = 0;
        ui.close_bank2(&link, || {
            block(&mut rig, &mut src);
            blocks += 1;
            blocks < 16
        })
    };
    assert!(ok, "the audio reported TAPE quiet");
    assert_eq!(tape_voices(&rig), 0, "no TAPE voice");
    block(&mut rig, &mut src);
    assert!(rig.inst.part_bus(0).iter().all(|&x| x == 0.0));
    assert!(
        rig.inst.part_bus(1).iter().any(|x| x.abs() > 1e-4),
        "ALGO plays on"
    );
    assert_eq!(rig.inst.sounding(), 1);

    // A note struck while shut dies; open, TAPE plays again.
    {
        let view = src.view();
        rig.inst.handle_with(ev(c0, on), &rig.shared, &view);
    }
    for _ in 0..6 {
        block(&mut rig, &mut src);
    }
    assert_eq!(tape_voices(&rig), 0, "shut stays shut");
    link.open_tape();
    {
        let view = src.view();
        rig.inst.handle_with(ev(c0, on), &rig.shared, &view);
    }
    block(&mut rig, &mut src);
    assert_eq!(tape_voices(&rig), 1, "open again");
}

#[test]
fn an_algo_note_stealing_a_fading_tape_voice_still_sounds() {
    let mut src = OneSource::new(&held_plain(&sine(24_000, 100.0, 24_000), 24_000));
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    *rig.shared.parts[0].params = init_params(EngineType::Tape);
    rig.shared.parts[0].source = Some(src.id);
    *rig.shared.parts[1].params = init_params(EngineType::Algo);
    let (c0, c1) = (
        rig.shared.parts[0].mix.channel,
        rig.shared.parts[1].mix.channel,
    );
    let link = LoadLink::new();
    let mut gate = LoadGate::new();
    let mut block = |rig: &mut InstRig, src: &mut OneSource| {
        let view: GateView<'_> = src.view();
        gate.before_block(&link, &mut rig.inst, &rig.shared);
        rig.inst.render_with(
            &mut rig.fx,
            &mut rig.dac,
            &rig.shared,
            &mut rig.scope,
            &view,
        );
    };
    let on = NoteKind::On(Velocity::DEFAULT);
    {
        let view = src.view();
        for n in 40..48 {
            let ev = NoteEvent {
                channel: c0,
                note: MidiNote::new(n).unwrap(),
                kind: on,
            };
            rig.inst.handle_with(ev, &rig.shared, &view);
        }
        // Every voice is TAPE's, so this steals one: the note waits on its fade.
        let ev = NoteEvent {
            channel: c1,
            note: MidiNote::new(60).unwrap(),
            kind: on,
        };
        rig.inst.handle_with(ev, &rig.shared, &view);
    }
    let mut ui = chimera_core::ui::UiState::new();
    let mut blocks = 0;
    let ok = ui.close_bank2(&link, || {
        block(&mut rig, &mut src);
        blocks += 1;
        blocks < 16
    });
    assert!(ok, "the audio reported TAPE quiet");
    for _ in 0..4 {
        block(&mut rig, &mut src);
    }
    assert!(rig.inst.part_bus(0).iter().all(|&x| x == 0.0));
    assert!(
        rig.inst.part_bus(1).iter().any(|x| x.abs() > 1e-4),
        "the stolen voice's ALGO note plays"
    );
}
