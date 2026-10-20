//! TAPE in bank 1 with `bank2` (ADR 0137): `off`'s surface over a
//! `TapeDeck` that bank 2's code plays, through the door's `TapeDoor`.
//! Closed, it is `off`'s silent engine. Open, the deck is built through the
//! door on first use, and every call that does work crosses it; what only
//! reads a field, or bank 1's cost table, doesn't. Nothing borrowed from
//! the door leaves `DOOR_CELL.with`.

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::bank2::DOOR_CELL;
use crate::hw::Cost;
use crate::modulation::ModState;
use crate::project::SourceId;
use crate::source_table::GateView;
use crate::stream::port::StreamPort;

use super::{Heads, TapeDeck, TapeEngineParams, TapeMachine, model_cost};

/// What bank 1 may call of the deck: one entry a method that does work,
/// all of them bank 2's (`TapeDeck::DOOR`).
#[repr(C)]
pub struct TapeDoor {
    init: fn(&mut MaybeUninit<TapeDeck>),
    seed: fn(&mut TapeDeck, u32),
    strip: fn(&TapeDeck) -> super::Strip,
    restore: fn(&mut TapeDeck, super::Strip),
    latch: fn(&mut TapeDeck, SourceId),
    unlatch: fn(&mut TapeDeck),
    start: fn(
        &mut TapeDeck,
        u8,
        &TapeEngineParams,
        &GateView<'_>,
        u32,
        Option<&mut StreamPort<'_, '_>>,
    ),
    refuses: fn(&TapeEngineParams, Option<SourceId>, &GateView<'_>) -> bool,
    play: fn(
        &mut TapeDeck,
        &mut [f32; BLOCK_SIZE],
        &TapeEngineParams,
        &GateView<'_>,
        u32,
        Option<&mut StreamPort<'_, '_>>,
    ),
    cost: fn(&TapeEngineParams, &ModState) -> Cost,
    playing_cost: fn(&TapeDeck) -> Option<Cost>,
    set_pitch: fn(&mut TapeDeck, f32),
    glide_to: fn(&mut TapeDeck, u8, (u32, f32)),
    release_by: fn(&mut TapeDeck, bool),
    hold: fn(&mut TapeDeck),
    heads: fn(&TapeDeck) -> Heads,
}

impl TapeDeck {
    /// The door's table. Its every entry is a `TapeDeck` item, so
    /// `bank2.x`'s `8TapeDeck` glob links it into bank 2.
    pub const DOOR: TapeDoor = TapeDoor {
        init: Self::door_init,
        seed: Self::seed,
        strip: Self::strip,
        restore: Self::restore,
        latch: Self::latch,
        unlatch: Self::unlatch,
        start: Self::start,
        refuses: Self::refuses,
        play: Self::play,
        cost: Self::cost,
        playing_cost: Self::playing_cost,
        set_pitch: Self::set_pitch,
        glide_to: Self::glide_to,
        release_by: Self::release_by,
        hold: Self::hold,
        heads: Self::heads,
    };

    fn door_init(slot: &mut MaybeUninit<TapeDeck>) {
        Self::init_in_place(slot);
    }
}

// A deck is built on its voice's first call through the door, in the audio
// block: eight voices' first notes in one block write eight decks, a few
// µs. Kept small so that stays so; never copied whole (no `Copy`).
const _: () = assert!(size_of::<TapeDeck>() <= 1024);

/// A voice's TAPE: the deck's room, and whether it is built.
pub struct TapeEngine {
    deck: MaybeUninit<TapeDeck>,
    /// `deck` is built: every field written through the door.
    live: bool,
    /// The voice's seed, for a deck built later.
    seed: u32,
}

/// The strip a rebuilt slot keeps: the deck's, if one ran.
#[derive(Clone, Copy)]
pub struct Strip {
    ran: Option<super::Strip>,
    seed: u32,
}

impl TapeEngine {
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(TapeEngine {
            deck: MaybeUninit::uninit(),
            live: false,
            seed: 0,
        })
    }

    /// `f` on the deck, built if it isn't; `None` closed.
    fn run<R>(&mut self, f: impl FnOnce(&TapeDoor, &mut TapeDeck) -> R) -> Option<R> {
        DOOR_CELL.with(|d| {
            let t = &d.tape;
            if !self.live {
                (t.init)(&mut self.deck);
                self.live = true;
                // SAFETY: `init` (`TapeDeck::init_in_place`) wrote every field.
                (t.seed)(unsafe { self.deck.assume_init_mut() }, self.seed);
            }
            // SAFETY: `live`: `init` wrote every field.
            f(t, unsafe { self.deck.assume_init_mut() })
        })
    }

    /// `f` on the deck if it is built; `None` closed.
    fn read<R>(&self, f: impl FnOnce(&TapeDoor, &TapeDeck) -> R) -> Option<R> {
        if !self.live {
            return None;
        }
        // SAFETY: `live`: `init` wrote every field.
        DOOR_CELL.with(|d| f(&d.tape, unsafe { self.deck.assume_init_ref() }))
    }

    pub fn seed(&mut self, s: u32) {
        self.seed = s;
        if self.live {
            self.run(|t, k| (t.seed)(k, s));
        }
    }

    pub fn strip(&self) -> Strip {
        Strip {
            ran: self.read(|t, k| (t.strip)(k)),
            seed: self.seed,
        }
    }

    pub fn restore(&mut self, s: Strip) {
        self.seed = s.seed;
        if let Some(r) = s.ran {
            self.run(|t, k| (t.restore)(k, r));
        }
    }

    pub fn latch(&mut self, id: SourceId) {
        self.run(|t, k| (t.latch)(k, id));
    }

    pub fn unlatch(&mut self) {
        self.run(|t, k| (t.unlatch)(k));
    }

    pub fn source(&self) -> Option<SourceId> {
        self.read(|_, k| k.src).flatten()
    }

    pub fn streams(&self) -> bool {
        self.read(|_, k| matches!(k.state, super::Note::Playing(super::Feed::Streamed(_))))
            .unwrap_or(false)
    }

    pub fn start(
        &mut self,
        note: u8,
        p: &TapeEngineParams,
        src: &GateView<'_>,
        sample_rate: u32,
        streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        self.run(|t, k| (t.start)(k, note, p, src, sample_rate, streams));
    }

    /// Closed, every note.
    pub fn refuses(p: &TapeEngineParams, id: Option<SourceId>, src: &GateView<'_>) -> bool {
        DOOR_CELL
            .with(|d| (d.tape.refuses)(p, id, src))
            .unwrap_or(true)
    }

    pub fn play(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &TapeEngineParams,
        src: &GateView<'_>,
        sample_rate: u32,
        streams: Option<&mut StreamPort<'_, '_>>,
    ) {
        if self
            .run(|t, k| (t.play)(k, out, p, src, sample_rate, streams))
            .is_none()
        {
            out.fill(0.0);
        }
    }

    pub fn cost(p: &TapeEngineParams, mods: &ModState) -> Cost {
        DOOR_CELL
            .with(|d| (d.tape.cost)(p, mods))
            .unwrap_or(Cost(0))
    }

    pub fn model_cost(m: TapeMachine) -> Cost {
        if DOOR_CELL.is_open() {
            model_cost(m)
        } else {
            Cost(0)
        }
    }

    pub fn playing_cost(&self) -> Option<Cost> {
        self.read(|t, k| (t.playing_cost)(k)).flatten()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.read(|_, k| k.slide()).unwrap_or(1.0)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn speed(&self) -> Option<(f32, f32)> {
        self.read(|_, k| k.speed()).flatten()
    }

    pub fn set_pitch(&mut self, cents: f32) {
        self.run(|t, k| (t.set_pitch)(k, cents));
    }

    pub fn glide_to(&mut self, note: u8, g: (u32, f32)) {
        self.run(|t, k| (t.glide_to)(k, note, g));
    }

    pub fn note_off(&mut self) {
        self.release_by(false);
    }

    pub fn release_by(&mut self, vca: bool) {
        self.run(|t, k| (t.release_by)(k, vca));
    }

    pub fn sustains(&self) -> bool {
        self.read(|_, k| !matches!(k.state, super::Note::Idle) && k.hold)
            .unwrap_or(false)
    }

    pub fn hold(&mut self) {
        self.run(|t, k| (t.hold)(k));
    }

    pub fn is_active(&self) -> bool {
        self.read(|_, k| !matches!(k.state, super::Note::Idle))
            .unwrap_or(false)
    }

    pub fn heads(&self) -> Heads {
        self.read(|t, k| (t.heads)(k)).unwrap_or(Heads::NONE)
    }
}
