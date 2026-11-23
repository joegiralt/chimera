// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #777, ADR 0211: STRETCH GRAIN's own-period search runs once a block at
//! most, so a chord struck on eight windows in one block stays inside it.
//! Judged by the searches counted against the unit's readings (bench STRIKE,
//! 2026-10-22, rev V at 480 MHz): eight GRAIN notes struck with their
//! source's PERIOD (no search) take 525,079 cycles of the block's 640,000,
//! and a search about 52,000.

mod common;
use common::InstRig;
use common::tape::{OneSource, held_plain, sine};

use chimera_core::dsp::tape_engine::stretch::period::searches;
use chimera_core::dsp::tape_engine::{StretchType, TapeMachine};
use chimera_core::hw::SampleBudget;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::{MidiNote, Velocity};

const BLOCK: u32 = 640_000;
const STRUCK_8: u32 = 525_079;
const SEARCH: u32 = 52_000;

#[test]
fn eight_windows_struck_in_one_block_stay_inside_it() {
    let mut src = OneSource::new(&held_plain(&sine(24_000, 109.0, 24_000), 24_000));
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    let mut p = common::init_params(EngineType::Tape);
    p.tape.machine = TapeMachine::Stretch;
    (p.tape.kind, p.tape.period) = (StretchType::Grain, 0.0);
    *rig.shared.parts[0].params = p;
    rig.shared.parts[0].source = Some(src.id);
    let ch = rig.shared.parts[0].mix.channel;
    let before = searches();
    // Eight notes, each on its own window (START moved between them), in
    // the block before one render.
    for k in 0..8u8 {
        rig.shared.parts[0].params.tape.start = 0.05 * f32::from(k);
        let ev = NoteEvent {
            channel: ch,
            note: MidiNote::new(60 + k).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        };
        rig.inst.handle_with(ev, &rig.shared, &src.view());
    }
    assert_eq!(rig.inst.sounding(), 8);
    let mut last = before;
    let mut per_block = Vec::new();
    for _ in 0..12 {
        let view = src.view();
        rig.inst.render_with(
            &mut rig.fx,
            &mut rig.dac,
            &rig.shared,
            &mut rig.scope,
            &view,
        );
        let now = searches();
        per_block.push(now - last);
        last = now;
    }
    // The strike's block and each after it: one search at most, inside
    // the block; and every window found its lock in the end.
    assert!(per_block.iter().all(|&n| n <= 1), "{per_block:?}");
    assert!(STRUCK_8 + SEARCH * per_block[0] <= BLOCK);
    assert_eq!(last - before, 8, "{per_block:?}");
    // Without the limit all eight would search in the strike's block.
    const { assert!(STRUCK_8 + SEARCH * 8 > BLOCK) };
}
