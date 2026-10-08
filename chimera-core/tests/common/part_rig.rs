//! A Project played as the shells play it: published each block, two Parts.
#![allow(dead_code)]

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartFrom, PartId, PartSource, Project, TemplateCrc};
use chimera_core::scope::ScopeWriter;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::SAMPLE_RATE;

pub struct Rig {
    pub p: Box<Project>,
    t: TemplateCrc,
    pub shared: Box<AudioShared>,
    pub inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    scope: ScopeWriter,
}

impl Rig {
    /// Parts 1 and 2 playing `sounds`.
    pub fn new(sounds: [Sound; 2]) -> Self {
        let (mut p, t) = Project::boxed();
        for (part, s) in PartId::ALL.into_iter().zip(sounds) {
            *p.edit_part(part).sound = s;
        }
        let mut r = Self {
            p,
            t,
            shared: Box::new(AudioShared::default()),
            inst: Box::new(Instrument::new(
                SAMPLE_RATE,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            scope: super::scope_writer(),
        };
        r.publish();
        r
    }

    fn publish(&mut self) {
        self.shared.update_from_project(&self.p, 0);
    }

    /// Note 57 on (or off) on `part`'s channel.
    pub fn note(&mut self, part: usize, on: bool) {
        self.key(part, 57, on);
    }

    /// `key` on (or off) on `part`'s channel.
    pub fn key(&mut self, part: usize, key: u8, on: bool) {
        let kind = if on {
            NoteKind::On(Velocity::DEFAULT)
        } else {
            NoteKind::Off
        };
        let ev = NoteEvent {
            channel: MidiChannel::clamped(part as u8),
            note: MidiNote::new(key).unwrap(),
            kind,
        };
        self.inst.handle(ev, &self.shared);
    }

    /// One block, published first.
    pub fn render(&mut self) {
        self.publish();
        self.inst
            .render(&mut self.fx, &mut self.dac, &self.shared, &mut self.scope);
    }

    /// `f` of each busy voice's slot, voice order.
    fn busy<T>(&self, f: impl Fn(usize) -> T) -> Vec<T> {
        let busy = self.inst.allocator().slots().map(|s| !s.is_free());
        (0..busy.len()).filter(|&v| busy[v]).map(f).collect()
    }

    /// One block: the cutoffs the busy voices played.
    pub fn block(&mut self) -> Vec<f32> {
        self.render();
        let played = self.inst.played_filters();
        self.busy(|v| played[v].cutoff)
    }

    /// One block: the MORPHs the busy voices played.
    pub fn morph_block(&mut self) -> Vec<f32> {
        self.render();
        let played = self.inst.played_morphs();
        self.busy(|v| played[v])
    }

    pub fn sound(&mut self, part: usize) -> &mut Sound {
        self.p.edit_part(PartId::ALL[part]).sound
    }

    /// A Sound load into `part` (INIT on ALGO), REPLACE answered.
    pub fn load(&mut self, part: usize) {
        let src = PartSource {
            part: PartId::ALL[part],
            from: PartFrom::Init(EngineType::Algo),
        };
        super::project::load_anyway(&mut self.p, self.t, src).unwrap();
    }
}
