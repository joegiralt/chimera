//! A lone `Voice` with the sympathetic pool it borrows from and, for TAPE,
//! the one source its notes latch (by default the thread's second of saw,
//! `tape::saw_with_burst` at 24 kHz): the suites that drive one voice
//! directly. It places a Sympathetic note as the `Instrument` does: at its
//! note-on, and when a switch brings the voice onto Sympathetic.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};

use chimera_core::dsp::engines::{EngineCtx, SlotKind};
use chimera_core::dsp::knobs::{KnobDelta, Knobs, Pace};
use chimera_core::dsp::modal::{ResonatorMode, SymPool};
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::project::PartId;
use chimera_core::source_table::GateView;
use chimera_core::tracked::Tracked;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

use super::tape::{OneSource, held_plain, saw_with_burst};

thread_local! {
    /// A second of the saw with its burst at 24 kHz: what a `Rig::new`'s
    /// TAPE notes play. One a thread, as its table is leaked.
    static SAW: RefCell<OneSource> =
        RefCell::new(OneSource::new(&held_plain(&saw_with_burst(24_000, 1.0), 24_000)));
}

/// What a rig's TAPE notes play.
pub enum Src {
    /// The thread's saw.
    Saw,
    Own(OneSource),
    /// None: a TAPE note plays nothing.
    None,
}

const SYM: SlotKind = SlotKind::Modal(ResonatorMode::Sympathetic);

pub struct Rig {
    pub voice: Box<Voice>,
    pub pool: Box<SymPool>,
    /// What a TAPE note latches.
    pub src: Src,
    /// The Part the voice plays (VA's seed): Part 1 unless a test moves it.
    pub part: PartId,
    /// The knobs of the one-voice Part `render_glided` plays.
    knobs: Knobs,
    /// SETTINGS > TUNING in semitones, as the Instrument hands it.
    pub tune: f32,
}

/// `$body` on the rig's voice with a context: its source's view this
/// block, or none.
macro_rules! with_ctx {
    ($rig:expr, |$v:ident, $ctx:ident, $id:ident| $body:expr) => {{
        let Rig {
            voice,
            pool,
            src,
            part,
            tune,
            ..
        } = $rig;
        let (part, tune) = (*part, *tune);
        let $v = voice;
        let mut run = |$id: Option<chimera_core::project::SourceId>, view: &GateView<'_>| {
            let $ctx = &mut EngineCtx {
                sym: &mut **pool,
                streams: None,
                src: view,
                part,
                tune,
            };
            $body
        };
        match src {
            Src::Saw => SAW.with(|s| {
                let mut s = s.borrow_mut();
                let id = s.id;
                run(Some(id), &s.view())
            }),
            Src::Own(s) => {
                let id = s.id;
                run(Some(id), &s.view())
            }
            Src::None => run(None, &GateView::empty()),
        }
    }};
}

impl Rig {
    pub fn new(sample_rate: u32) -> Self {
        Self::at(sample_rate, VoiceIdx::ALL[0], Src::Saw)
    }

    /// A voice whose TAPE notes play `held`.
    pub fn with_source(sample_rate: u32, held: &[u8]) -> Self {
        Self::at(
            sample_rate,
            VoiceIdx::ALL[0],
            Src::Own(OneSource::new(held)),
        )
    }

    /// Voice `id` of the pool, playing `src`.
    pub fn at(sample_rate: u32, id: VoiceIdx, src: Src) -> Self {
        let mut raw = Box::<Voice>::new_uninit();
        Voice::init_in_place(&mut raw, sample_rate, id);
        Self {
            // SAFETY: `init_in_place` built a valid voice in the box.
            voice: unsafe { raw.assume_init() },
            pool: SymPool::boxed(),
            src,
            part: PartId::ALL[0],
            knobs: Knobs::UNPRIMED,
            tune: 0.0,
        }
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) -> bool {
        if SlotKind::of(params) == SYM {
            self.pool.place(VoiceIdx::ALL[0]);
        }
        with_ctx!(self, |v, ctx, id| v
            .note_on(note, velocity, params, id, ctx))
    }

    pub fn note_off(&mut self) {
        with_ctx!(self, |v, ctx, _id| v.note_off(ctx))
    }

    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], params: &ParamSnapshot, mods: &ModState) {
        if SlotKind::of(params) == SYM && self.voice.kind() != SYM {
            self.pool.place(VoiceIdx::ALL[0]);
        }
        with_ctx!(self, |v, ctx, _id| v.render(out, params, mods, ctx))
    }

    /// The one-voice Part's pitch bend, semitones, landed.
    pub fn set_bend(&mut self, semitones: f32) {
        self.knobs.ease_bend(semitones, None);
    }

    /// `render` as the voice of a one-voice Part: its knobs glide (ADR 0112).
    pub fn render_glided(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        mods: &ModState,
    ) {
        // Fresh each block, no publisher: read whole.
        let (p, m) = (Tracked::fresh(params.clone()), Tracked::fresh(mods.clone()));
        let pace = Some(Pace::Glide(self.voice.sample_rate()));
        self.knobs.ease((&p, &m), &KnobDelta::default(), pace);
        let knobs = self.knobs;
        with_ctx!(self, |v, ctx, _id| v
            .render_knobs(out, params, mods, &knobs, None, ctx))
    }
}

impl Deref for Rig {
    type Target = Voice;
    fn deref(&self) -> &Voice {
        &self.voice
    }
}

impl DerefMut for Rig {
    fn deref_mut(&mut self) -> &mut Voice {
        &mut self.voice
    }
}
