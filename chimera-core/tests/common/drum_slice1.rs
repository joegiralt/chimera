// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The DRUM slice-1 fixture, rendered (`golden_test`'s `DRUM_GOLDENS`).
//! Hit seeds are global and move from render to render: a binary renders
//! it once for a golden to hold.

use chimera_core::{MidiNote, Velocity};

/// The slice-1 fixture's render (`golden_test::drum_slice1_render`): Part
/// 1's mono bus and pair 1's L/R, with `edit` made to the project after its load.
pub fn render(edit: impl FnOnce(&mut chimera_core::project::Project)) -> (Vec<f32>, Vec<f32>) {
    use chimera_core::dsp::drum::view::KitWriter;
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::dsp::knobs::KnobPublisher;
    use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    use chimera_core::project::{PartId, Project};
    use chimera_hal::BLOCK_SIZE;
    let f = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/drum_slice1.prj"
    ))
    .unwrap();
    let (mut p, _) = Project::boxed();
    super::codec_util::decode_project(&f, &mut p).unwrap();
    edit(&mut p);
    let mut knobs = Box::new(KnobPublisher::new());
    let mut shared = Box::new(AudioShared::default());
    shared.update_from_project(&p, 0, &mut knobs);
    KitWriter::take()
        .expect("the kit writer, once a binary")
        .publish_all(p.kits());
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        // Rev V's; twice that at master-tape, whose bus leaves the kit short (#951).
        if cfg!(feature = "master-tape") {
            super::drum::PLAY_BUDGET
        } else {
            super::drum::REV_V_BUDGET
        },
    ));
    let mut fx = Box::new(FxBus::new());
    let mut dac = Box::new(DacBlocks::new());
    let mut scope = super::scope_writer();
    let channel = p.part(PartId::ALL[0]).mix.channel;
    // Pads 1, 2, 8, 9, 10 are notes 0, 1, 7, 8, 9.
    let hits = [(0, 100), (1, 40), (7, 127), (8, 90), (9, 64)];
    let blocks = 2 * chimera_hal::SAMPLE_RATE as usize / BLOCK_SIZE;
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut lr = Vec::with_capacity(2 * blocks * BLOCK_SIZE);
    for b in 0..blocks {
        if let Some(&(n, v)) = hits.get(b) {
            inst.handle(
                NoteEvent {
                    channel,
                    note: MidiNote::new(n).unwrap(),
                    kind: NoteKind::On(Velocity::new(v).unwrap()),
                },
                &shared,
            );
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        out.extend_from_slice(inst.part_bus(0));
        lr.extend_from_slice(&dac.out()[0]);
    }
    (out, lr)
}
