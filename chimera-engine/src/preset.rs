use crate::addr::{BlockRead, BlockRef};
use crate::mod_path::ModDestRegistry;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, ParamSnapshot};
use crate::project::SourceRef;
use crate::storage::{SourceHead, TapeSource, ZoneTune};

pub use crate::project::{Part, PartEdit, Performance, part_block, part_block_mut};

/// The project pool's slots (ADR 0087): its bytes, not this, are the budget.
pub const MAX_SOUNDS: usize = 128;

#[derive(Clone, Debug)]
#[repr(C)]
pub struct Sound {
    pub name: SoundName,
    /// Carries the engine: `engine()` reads it from here, the one place.
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
    /// A TAPE Sound's source: a reference, never audio (ADR 0087), and a
    /// zone set's tuning, copied at pick (spec § 8).
    pub source: Option<TapeSource>,
}

impl Sound {
    // Out of line: inlined into its dozen callers it cost ~15 KB of flash.
    #[inline(never)]
    pub fn init(engine: EngineType) -> Self {
        // The default routes (spec § 2): ENV 1, LFO 1 and NOTE → FREQ, the
        // fixed CUTOFF column, at 0, on every engine (a kind that wants
        // keytracking on a new Sound sets NOTE's here). The registry stays
        // empty. A TAPE loop has no end: ENV 1 into VCA ends its note.
        let mut mod_state = ModState::with_sources(MAX_MOD_SOURCES);
        for s in [ModSource::Env1, ModSource::Lfo1, ModSource::Note] {
            mod_state.set_fixed(s.index(), FixedCol::Freq, 0);
        }
        if engine == EngineType::Tape {
            mod_state.set_fixed(ModSource::Env1.index(), FixedCol::Vca, 127);
        }
        Self {
            name: Self::init_name(),
            params: ParamSnapshot::for_engine(engine),
            mod_state,
            dest_registry: ModDestRegistry::new(),
            source: None,
        }
    }

    /// SOURCE picked (spec § 1): the head's ref, and its root and period
    /// copied into ROOT and PERIOD, with a zone set's `tune`. The voice
    /// reads them from here, never from the file.
    pub fn pick_source(&mut self, head: &SourceHead, tune: Option<ZoneTune>) {
        self.source = Some(TapeSource { r: head.sref, tune });
        self.params.tape.root = f32::from(head.root);
        self.params.tape.period = head.period_q16 as f32 / 65_536.0;
    }

    /// Its source's ref, if it has one.
    pub fn source_ref(&self) -> Option<SourceRef> {
        self.source.map(|s| s.r)
    }

    /// The engine this Sound plays, and so the chain the UI shows for it.
    pub fn engine(&self) -> EngineType {
        self.params.engine()
    }

    /// An init or neutral Sound's name.
    pub fn init_name() -> SoundName {
        SoundName::new("INIT").expect("a valid name")
    }

    /// Same name, engine, voice-block params (by bits), routes, taps and
    /// registry: what a card round trip must keep. Unused slots don't count.
    pub fn bits_eq(&self, o: &Sound) -> bool {
        self.name == o.name
            && self.engine() == o.engine()
            && BlockRef::ALL
                .into_iter()
                .all(|b| match (self.params.block(b), o.params.block(b)) {
                    (Some(x), Some(y)) => x
                        .specs()
                        .iter()
                        .all(|s| x.get(s.id).to_bits() == y.get(s.id).to_bits()),
                    (x, y) => x.is_none() && y.is_none(),
                })
            && self.params.taps == o.params.taps
            && self.mod_state.bits_eq(&o.mod_state)
            && self.dest_registry.bits_eq(&o.dest_registry)
            && self.source == o.source
    }
}
