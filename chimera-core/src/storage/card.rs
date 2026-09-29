//! The card: mounted per operation, a swap seen by volume serial and label.

use chimera_hal::store::{Store, StoreError, Unsupported, VolumeId};

/// A card fault. No NO CARD variant: `Failed(NoCard)` can't be built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardError {
    Unsupported(Unsupported),
    Full,
    Timeout,
    VolumeChanged(VolumeId),
    Io,
}

impl CardError {
    /// `None` for NO CARD and for file errors (`NotFound`, `Corrupt`).
    pub fn from_store(e: StoreError) -> Option<CardError> {
        match e {
            StoreError::NoCard | StoreError::NotFound | StoreError::Corrupt => None,
            StoreError::Unsupported(u) => Some(CardError::Unsupported(u)),
            StoreError::Full => Some(CardError::Full),
            StoreError::Timeout => Some(CardError::Timeout),
            StoreError::VolumeChanged(v) => Some(CardError::VolumeChanged(v)),
            StoreError::Io => Some(CardError::Io),
        }
    }
}

/// For `StoreError::message`.
impl From<CardError> for StoreError {
    fn from(e: CardError) -> StoreError {
        match e {
            CardError::Unsupported(u) => StoreError::Unsupported(u),
            CardError::Full => StoreError::Full,
            CardError::Timeout => StoreError::Timeout,
            CardError::VolumeChanged(v) => StoreError::VolumeChanged(v),
            CardError::Io => StoreError::Io,
        }
    }
}

/// Proof of a mount, lent as `&Ready` by `Card::run` for one operation.
///
/// Only this module builds one:
/// ```compile_fail,E0451
/// use chimera_core::storage::Ready;
/// use chimera_hal::store::VolumeId;
/// let _ = Ready { vol: VolumeId { serial: 1, label: [b' '; 11] } };
/// ```
///
/// It can't be copied out of the loan:
/// ```compile_fail,E0507
/// use chimera_core::storage::Card;
/// use chimera_hal::{store::StoreError, testkit::MemStore};
/// let (mut card, mut s) = (Card::new(), MemStore::new(1));
/// let _ = card.run(&mut s, |_, r| {
///     let owned = *r;
///     Ok::<_, StoreError>(())
/// });
/// ```
///
/// And the loan can't leave `run` (a lifetime error, which has no stable code):
/// ```compile_fail
/// use chimera_core::storage::Card;
/// use chimera_hal::{store::StoreError, testkit::MemStore};
/// let (mut card, mut s) = (Card::new(), MemStore::new(1));
/// let _ = card.run(&mut s, |_, r| Ok::<_, StoreError>(r));
/// ```
#[derive(Debug)]
pub struct Ready {
    vol: VolumeId,
}

impl Ready {
    pub fn volume(&self) -> VolumeId {
        self.vol
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Card {
    Absent,
    Ready(VolumeId),
    Failed {
        err: CardError,
        last: Option<VolumeId>,
    },
}

/// What a mount found, so callers drop what belonged to another card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardEvent {
    /// No card was known.
    Mounted,
    Same,
    Swapped {
        old: VolumeId,
    },
}

/// The store error inside an operation's error, if there is one.
pub trait CardFault {
    fn store_error(&self) -> Option<StoreError>;
}

impl CardFault for StoreError {
    fn store_error(&self) -> Option<StoreError> {
        Some(*self)
    }
}

impl Card {
    pub const fn new() -> Card {
        Card::Absent
    }

    fn last(self) -> Option<VolumeId> {
        match self {
            Card::Absent => None,
            Card::Ready(v) => Some(v),
            Card::Failed { last, .. } => last,
        }
    }

    /// Mounts, lends `&Ready` to `op`, and records any store error. The
    /// `Ready` lives on this stack frame, so it can't outlive the mount.
    pub fn run<S: Store, R, E: CardFault + From<StoreError>>(
        &mut self,
        store: &mut S,
        op: impl FnOnce(&mut S, &Ready) -> Result<R, E>,
    ) -> Result<(R, CardEvent), E> {
        let vol = match store.mount() {
            Ok(v) => v,
            Err(e) => {
                *self = after_error(*self, e);
                return Err(e.into());
            }
        };
        let (card, event) = mounted(*self, vol);
        *self = card;
        let ready = Ready { vol };
        match op(store, &ready) {
            Ok(r) => Ok((r, event)),
            Err(e) => {
                if let Some(se) = e.store_error() {
                    *self = after_error(*self, se);
                }
                Err(e)
            }
        }
    }
}

impl Default for Card {
    fn default() -> Card {
        Card::new()
    }
}

fn mounted(card: Card, vol: VolumeId) -> (Card, CardEvent) {
    let event = match card.last() {
        None => CardEvent::Mounted,
        Some(old) if old == vol => CardEvent::Same,
        Some(old) => CardEvent::Swapped { old },
    };
    (Card::Ready(vol), event)
}

/// The card after a mount: an event on success, none on failure.
pub fn after_mount(card: Card, r: Result<VolumeId, StoreError>) -> (Card, Option<CardEvent>) {
    match r {
        Ok(vol) => {
            let (card, event) = mounted(card, vol);
            (card, Some(event))
        }
        Err(e) => (after_error(card, e), None),
    }
}

/// NO CARD is `Absent`; a card fault is `Failed`, keeping the last volume; a
/// file error leaves the card alone.
pub fn after_error(card: Card, e: StoreError) -> Card {
    if e == StoreError::NoCard {
        return Card::Absent;
    }
    match CardError::from_store(e) {
        Some(err) => Card::Failed {
            err,
            last: card.last(),
        },
        None => card,
    }
}
