use crate::addr::{BlockRead, BlockRef, ParamAddr};
use crate::midi_map::{MidiMap, default_for};
use crate::mod_path::ModDestRegistry;
use crate::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, ParamSnapshot, PitchParams};
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
    /// CC -> param mappings (MIDI map spec § 2). Empty is neutral.
    pub map: MidiMap,
}

impl Sound {
    // Out of line: inlined into its dozen callers it cost ~15 KB of flash.
    #[inline(never)]
    pub fn init(engine: EngineType) -> Self {
        let mut s = Self::plain_init(engine);
        s.map = default_for(engine);
        // A new Sound's 2040 or LADDER is a Prophet's voices, each its own
        // chip (ADR 0168); the factory Sounds, on `plain_init`, stay stable.
        s.params.filter.stability = crate::dsp::filter::stability::PROPHET;
        // The wheel's vibrato (spec § 9): LFO 1 DEPTH is the wheel, at rest
        // 0, and LFO 1 → PITCH at 2 is ±0.38 semitone at DEPTH 1.
        s.params.lfos[0].depth = 0.0;
        let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
        let lfo1 = ModSource::Lfo1.index();
        if s.dest_registry.add(pitch, *b"PITCH\0\0\0").is_ok()
            && let Some(d) = s.mod_state.push(pitch)
        {
            s.mod_state.set_amount(lfo1, d, 2);
        }
        s
    }

    /// INIT without the wheel or the map: the base the factory Sounds are
    /// authored on, with LFO 1 at DEPTH 1.
    #[inline(never)]
    pub fn plain_init(engine: EngineType) -> Self {
        // The default routes (spec § 2): ENV 1, LFO 1 and NOTE → FREQ, the
        // fixed CUTOFF column, at 0, on every engine (a kind that wants
        // keytracking on a new Sound sets NOTE's here). The registry stays
        // empty. A TAPE loop or a VA tone has no end: ENV 1 into VCA ends
        // its note. VA's INIT detunes OSC 2 and breathes a little (spec
        // § Voice and engine).
        let mut mod_state = ModState::with_sources(MAX_MOD_SOURCES);
        for s in [ModSource::Env1, ModSource::Lfo1, ModSource::Note] {
            mod_state.set_fixed(s.index(), FixedCol::Freq, 0);
        }
        if matches!(engine, EngineType::Tape | EngineType::Va) {
            mod_state.set_fixed(ModSource::Env1.index(), FixedCol::Vca, 127);
        }
        let mut params = ParamSnapshot::for_engine(engine);
        if engine == EngineType::Tape {
            // LOOP's LENGTH (`machine`): POSITION moves the loop at INIT.
            params.tape.size = crate::dsp::tape_engine::machine::LOOP_LENGTH_DEFAULT;
        }
        if engine == EngineType::Va {
            params.va.fine[1] = 6.0;
            params.va.stability = 0.7;
        }
        Self {
            name: Self::init_name(),
            params,
            mod_state,
            dest_registry: ModDestRegistry::new(),
            source: None,
            map: MidiMap::EMPTY,
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
            && self.map == o.map
    }
}
