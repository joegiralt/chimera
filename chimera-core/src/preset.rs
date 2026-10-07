use crate::addr::{BlockRead, BlockRef};
use crate::mod_path::ModDestRegistry;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, ParamSnapshot};
use crate::project::SourceRef;

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
    /// A TAPE Sound's source: a reference, never audio (ADR 0087).
    pub source: Option<SourceRef>,
}

impl Sound {
    // Out of line: inlined into its dozen callers it cost ~15 KB of flash.
    #[inline(never)]
    pub fn init(engine: EngineType) -> Self {
        // The default routes (spec § 2): ENV 1, LFO 1 and NOTE → FREQ, the
        // fixed CUTOFF column, at 0, NOTE at the kind's key default, on
        // every engine. The registry stays empty.
        let mut mod_state = ModState::with_sources(MAX_MOD_SOURCES);
        let key = crate::dsp::filter::FilterKind::default_for(engine).key_default();
        for (s, a) in [
            (ModSource::Env1, 0),
            (ModSource::Lfo1, 0),
            (ModSource::Note, key),
        ] {
            mod_state.set_fixed(s.index(), FixedCol::Freq, a);
        }
        Self {
            name: Self::init_name(),
            params: ParamSnapshot::for_engine(engine),
            mod_state,
            dest_registry: ModDestRegistry::new(),
            source: None,
        }
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
