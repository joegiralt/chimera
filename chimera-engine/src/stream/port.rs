//! What a streamed voice's cursor reaches in one block.

use chimera_hal::card_bus::CardBus;

use super::ring::{AudioSide, AudioTurn};
use super::stats::StreamStats;
use super::units::{AudioBlocks, Frames};
use crate::dsp::engines::EngineCtx;
use crate::dsp::modal::SymPool;
use crate::project::{PartId, SourceId};
use crate::source_table::GateView;
use crate::voice_alloc::VoiceIdx;

/// Built a block, a voice: it borrows the block's `AudioTurn`, so no
/// `Ready` outlives the block.
pub struct StreamPort<'a, 's> {
    pub turn: &'a mut AudioTurn<'s>,
    pub voice: VoiceIdx,
    /// The streamed file's frames.
    pub file_frames: Frames,
    pub now: AudioBlocks,
    pub bus: &'a CardBus,
    pub stats: &'a StreamStats,
    /// Pends the card IRQ (`DMA2_STR0`), after every new want, whoever
    /// owns the bus.
    pub pend: fn(),
    /// Cents a note started through this port can move past its PITCH and
    /// TUNING: its routes at full swing and the bend's range (#585). Its
    /// cursor plans for them.
    pub reach_cents: f32,
}

/// The card's side of every port this block.
pub struct StreamCard<'a> {
    /// A streamed source's frames; `None` for one that isn't.
    pub frames: &'a dyn Fn(SourceId) -> Option<Frames>,
    pub now: AudioBlocks,
    pub bus: &'a CardBus,
    pub stats: &'a StreamStats,
    pub pend: fn(),
}

/// The rings and the card, for the voices' ports (slice 1 § 7.1).
pub struct Streams<'a, 'r> {
    pub audio: &'a mut AudioSide<'r>,
    pub card: StreamCard<'a>,
}

/// What an engine call shares across voices: `EngineCtx` but the port.
pub struct CtxBase<'a> {
    pub sym: &'a mut SymPool,
    pub src: &'a GateView<'a>,
    pub part: PartId,
    pub tune: f32,
}

/// Each voice's port, a call at a time: `NoPorts` where no rings are
/// wired, `Streams` where they are.
pub trait VoicePorts {
    /// `f` on `base` with voice `v`'s port for `port`'s source, if it
    /// streams; a note it starts reaches `port`'s cents.
    fn ctx<R>(
        &mut self,
        base: CtxBase<'_>,
        v: VoiceIdx,
        port: (Option<SourceId>, f32),
        f: impl FnOnce(&mut EngineCtx<'_>) -> R,
    ) -> R;
}

/// No rings: a streamed source plays nothing.
pub struct NoPorts;

impl VoicePorts for NoPorts {
    #[inline(always)]
    fn ctx<R>(
        &mut self,
        base: CtxBase<'_>,
        _: VoiceIdx,
        _: (Option<SourceId>, f32),
        f: impl FnOnce(&mut EngineCtx<'_>) -> R,
    ) -> R {
        f(&mut EngineCtx {
            sym: base.sym,
            streams: None,
            src: base.src,
            part: base.part,
            tune: base.tune,
        })
    }
}

impl VoicePorts for Streams<'_, '_> {
    fn ctx<R>(
        &mut self,
        base: CtxBase<'_>,
        v: VoiceIdx,
        (source, reach_cents): (Option<SourceId>, f32),
        f: impl FnOnce(&mut EngineCtx<'_>) -> R,
    ) -> R {
        let c = &self.card;
        let Some(file_frames) = source.and_then(|id| (c.frames)(id)) else {
            return NoPorts.ctx(base, v, (None, 0.0), f);
        };
        let mut turn = self.audio.turn();
        let mut port = StreamPort {
            turn: &mut turn,
            voice: v,
            file_frames,
            now: c.now,
            bus: c.bus,
            stats: c.stats,
            pend: c.pend,
            reach_cents,
        };
        f(&mut EngineCtx {
            sym: base.sym,
            streams: Some(&mut port),
            src: base.src,
            part: base.part,
            tune: base.tune,
        })
    }
}
