//! An `Instrument` with its FX bus, DAC blocks and scope: the suites that
//! play notes through the whole pool.

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::scope::ScopeWriter;
use chimera_core::{MidiChannel, MidiNote, Velocity};

use super::{SR, scope_writer};

pub struct InstRig {
    pub inst: Box<Instrument>,
    pub fx: Box<FxBus>,
    pub dac: Box<DacBlocks>,
    pub scope: ScopeWriter,
    /// What `note_on`, `note_off` and `render` play.
    pub shared: Box<AudioShared>,
}

fn event(ch: u8, note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind,
    }
}

impl InstRig {
    pub fn with_budget(budget: SampleBudget) -> Self {
        Self {
            inst: Box::new(Instrument::new(SR, budget)),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: scope_writer(),
            shared: Box::default(),
        }
    }

    pub fn note_on(&mut self, ch: u8, note: u8) {
        let ev = event(ch, note, NoteKind::On(Velocity::DEFAULT));
        self.inst.handle(ev, &self.shared);
    }

    pub fn note_off(&mut self, ch: u8, note: u8) {
        self.inst
            .handle(event(ch, note, NoteKind::Off), &self.shared);
    }

    /// `blocks` blocks of `shared`.
    pub fn render(&mut self, blocks: usize) {
        for _ in 0..blocks {
            self.inst
                .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
        }
    }

    /// One block of `shared`, given: the DAC pairs out.
    pub fn block(&mut self, shared: &AudioShared) -> &DacOut {
        self.inst
            .render(&mut self.fx, &mut self.dac, shared, &mut self.scope);
        self.dac.out()
    }

    /// The last block out, limited.
    pub fn out(&self) -> &DacOut {
        self.dac.out()
    }
}
