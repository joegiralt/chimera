//! The card: mounted per operation, a swap seen by volume serial and label.

use chimera_hal::store::{Store, StoreError, Unsupported, VolumeId};

/// A card fault. No NO CARD variant: `Failed(NoCard)` can't be built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardError {
    Unsupported(Unsupported),
    Timeout,
    VolumeChanged(VolumeId),
    Io,
}

impl CardError {
    /// `None` for NO CARD and for file-level conditions: `NotFound`,
    /// `Corrupt`, and `Full`, which leaves a healthy card a delete can make
    /// room on.
    pub fn from_store(e: StoreError) -> Option<CardError> {
        match e {
            StoreError::NoCard | StoreError::NotFound | StoreError::Corrupt | StoreError::Full => {
                None
            }
            StoreError::Unsupported(u) => Some(CardError::Unsupported(u)),
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
/// Nor cloned (a bare `r.clone()` would clone the reference):
/// ```compile_fail,E0599
/// use chimera_core::storage::Card;
/// use chimera_hal::{store::StoreError, testkit::MemStore};
/// let (mut card, mut s) = (Card::new(), MemStore::new(1));
/// let _ = card.run(&mut s, |_, r| {
///     let owned = (*r).clone();
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
    /// No card. The last volume is forgotten, so even the same card put back
    /// mounts as `Mounted` and everything cached is rebuilt: conservative.
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
    ///
    /// `Err` is a failed mount. Once mounted, the event comes back whatever
    /// `op` returns: a swap is news even when the op on the new card fails.
    pub fn run<S: Store, R, E: CardFault + From<StoreError>>(
        &mut self,
        store: &mut S,
        op: impl FnOnce(&mut S, &Ready) -> Result<R, E>,
    ) -> Result<Outcome<R, E>, E> {
        let vol = match store.mount() {
            Ok(v) => v,
            Err(e) => {
                *self = mount_failed(*self, e);
                return Err(e.into());
            }
        };
        let (card, event) = mounted(*self, vol);
        *self = card;
        let ready = Ready { vol };
        let result = op(store, &ready);
        if let Some(e) = result.as_ref().err().and_then(CardFault::store_error) {
            *self = after_error(*self, e);
        }
        Ok(Outcome { event, result })
    }
}

/// A mounted `Card::run`: what the mount found, and what `op` returned.
#[must_use]
#[derive(Debug, PartialEq)]
pub struct Outcome<R, E> {
    pub event: CardEvent,
    pub result: Result<R, E>,
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

/// A failed mount fails closed: anything but NO CARD is a card fault, and
/// one that names no card fault (a file error from `mount`) is `Io`. The
/// type-driven fix is a `MountError`: https://github.com/joegiralt/chimera/issues/196
fn mount_failed(card: Card, e: StoreError) -> Card {
    if e == StoreError::NoCard {
        return Card::Absent;
    }
    Card::Failed {
        err: CardError::from_store(e).unwrap_or(CardError::Io),
        last: card.last(),
    }
}

/// The card after a mount: an event on success, none on failure.
pub fn after_mount(card: Card, r: Result<VolumeId, StoreError>) -> (Card, Option<CardEvent>) {
    match r {
        Ok(vol) => {
            let (card, event) = mounted(card, vol);
            (card, Some(event))
        }
        Err(e) => (mount_failed(card, e), None),
    }
}

/// NO CARD is `Absent`; a card fault is `Failed`, keeping the last volume; a
/// file error or a full card leaves the card alone.
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
