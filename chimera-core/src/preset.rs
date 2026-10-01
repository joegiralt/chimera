use crate::addr::{BlockRef, Blocks};
use crate::mod_path::ModDestRegistry;
use crate::modulation::{CUTOFF, CUTOFF_LABEL, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, ParamSnapshot};

pub use crate::project::{Part, PartEdit, Performance, part_block, part_block_mut};

pub const POOL_SIZE: usize = 32;

#[derive(Clone, Debug)]
#[repr(C)]
pub struct Sound {
    pub name: SoundName,
    /// Carries the engine: `engine()` reads it from here, the one place.
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub dest_registry: ModDestRegistry,
}

impl Sound {
    pub fn init(engine: EngineType) -> Self {
        // The default routes (spec § 2): ENV 1, LFO 1 and NOTE → CUTOFF at
        // 0, NOTE at the kind's key default, on every engine.
        let mut dest_registry = ModDestRegistry::new();
        let _ = dest_registry.add(CUTOFF, CUTOFF_LABEL); // an empty registry takes it
        let mut mod_state = ModState::from_registry(&dest_registry, MAX_MOD_SOURCES);
        let key = crate::dsp::filter::FilterKind::default().key_default();
        for (s, a) in [
            (ModSource::Env1, 0),
            (ModSource::Lfo1, 0),
            (ModSource::Note, key),
        ] {
            mod_state.set_route(s.index(), 0, a);
        }
        Self {
            name: Self::init_name(),
            params: ParamSnapshot::for_engine(engine),
            mod_state,
            dest_registry,
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

    /// Same name, engine, voice-block params (by bits), routes and
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
            && self.mod_state.bits_eq(&o.mod_state)
            && self.dest_registry.bits_eq(&o.dest_registry)
    }
}
