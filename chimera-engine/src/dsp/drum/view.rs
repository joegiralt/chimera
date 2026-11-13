//! The kits as the audio reads them (ADR 0265): each Part's kit and its
//! TONAL (ADR 0270), published by the UI, and back to the UI each Part's
//! live hits and sounding pads; and each Part's hit seed, the audio's own. A static of atomics only: the audio never touches the UI's
//! `Kits`, and any bytes decode to a pad (`Pad::from_bytes`), so the
//! chip's NOLOAD ITCM needs no build before the first publish.
//!
//! A Part's kit is a two-slot seqlock: a publish writes the slot the audio
//! isn't reading, then flips. The audio's read never waits. On the chip
//! the UI can't run inside the audio's interrupt, so a read is never
//! overtaken; on the desktop a lapped read (two publishes during one pad's
//! 70 loads) reads again, at most twice more, and any bytes it ends with
//! are still a valid pad.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering, fence};

use crate::hw::MAX_PARTS;
use crate::project::{Kits, PartId};

use crate::MidiNote;

use super::sound::{KitBill, PAD_SOUND_BYTES, PadSound, kit_bill};
use super::types::{DrumHit, HitSeed, Kit, KitNote, PAD_BYTES, Pad, PadId, PadSet, Tonal};

/// A pad in the view: its 13 bytes (`Pad::to_bytes`), then its sound's 57
/// (`PadSound::to_bytes`).
const VIEW_BYTES: usize = PAD_BYTES + PAD_SOUND_BYTES;

/// One Part's kit, selected pad and live hits.
struct PartView {
    /// Twice a publish: odd while one is written. Slot `seq / 2 % 2` is the
    /// published one; a publish writes the other.
    seq: AtomicU32,
    /// The kit's TONAL's code (`Tonal::code`): what C1 up plays.
    tonal: AtomicU8,
    /// The last live hit's pad index + 1, 0 for none: the audio's, taken
    /// by the UI.
    hit: AtomicU8,
    /// Every pad a live hit struck since the UI's last take (`PadSet`'s
    /// bits): a UI frame's hits all light, not just its last.
    struck: AtomicU16,
    /// The pads its voices sound (`PadSet`'s bits): the audio's, each block.
    sounding: AtomicU16,
    /// The last hit's `HitSeed`: the audio's alone, each hit the next.
    seed: AtomicU32,
    /// The kit's `kit_bill` (`KitBill::to_bits`): the UI's, stored each
    /// publish.
    bill: AtomicU32,
    slots: [[[AtomicU8; VIEW_BYTES]; PadId::ALL.len()]; 2],
}

/// Every Part's `PartView` (10,200 B).
pub struct KitView {
    parts: [PartView; MAX_PARTS],
}

const _: () = assert!(core::mem::size_of::<KitView>() == 10_200);

// SAFETY: `.itcm` is the board's NOLOAD ITCM (chimera-stm32's build.rs,
// ADR 0266), CPU-only and never DMA's; it holds garbage at boot, which
// every atomic here accepts as a value. On the desktop and in the tests
// the static is ordinary zeroed memory.
#[cfg_attr(target_os = "none", unsafe(link_section = ".itcm"))]
pub static KIT_VIEW: KitView = KitView::new();

/// `KitWriter::take`'s once.
static WRITER_TAKEN: AtomicBool = AtomicBool::new(false);

const fn slot(seq: u32) -> usize {
    (seq >> 1 & 1) as usize
}

impl KitView {
    const fn new() -> KitView {
        KitView {
            parts: [const {
                PartView {
                    seq: AtomicU32::new(0),
                    tonal: AtomicU8::new(0),
                    hit: AtomicU8::new(0),
                    struck: AtomicU16::new(0),
                    sounding: AtomicU16::new(0),
                    seed: AtomicU32::new(0),
                    bill: AtomicU32::new(0),
                    slots: [const {
                        [const { [const { AtomicU8::new(0) }; VIEW_BYTES] }; PadId::ALL.len()]
                    }; 2],
                }
            }; MAX_PARTS],
        }
    }

    /// Audio: what `note` strikes on `part`'s kit, as its TONAL says
    /// (`KitNote::new`); a code past the pads (garbage at boot) is OFF.
    pub fn hit(&self, part: PartId, note: MidiNote) -> DrumHit {
        let v = &self.parts[part.index()];
        let tonal = Tonal::from_code(v.tonal.load(Ordering::Relaxed)).unwrap_or(Tonal::Off);
        let (pad, pitch) = KitNote::new(note, tonal).pad();
        let mut b = [0; VIEW_BYTES];
        for _ in 0..3 {
            let s = v.seq.load(Ordering::Acquire);
            for (o, a) in b.iter_mut().zip(&v.slots[slot(s)][pad.index()]) {
                *o = a.load(Ordering::Relaxed);
            }
            fence(Ordering::Acquire);
            // Its slot is written again from `(s & !1) + 3` on.
            if v.seq.load(Ordering::Relaxed).wrapping_sub(s & !1) < 3 {
                break;
            }
        }
        let d = Kit::default_pad(pad);
        let sound = PadSound::from_bytes(core::array::from_fn(|i| b[PAD_BYTES + i]), d.sound());
        let p = Pad::from_bytes(core::array::from_fn(|i| b[i]), &d).with_sound(sound);
        DrumHit::new(pad, p, pitch)
    }

    /// Audio: what a hit on `part`'s kit is billed (`kit_bill`), as last
    /// published: `KitBill::NONE` from `KitWriter::take` on.
    pub fn bill(&self, part: PartId) -> KitBill {
        KitBill::from_bits(self.parts[part.index()].bill.load(Ordering::Relaxed))
    }

    /// Audio: `part`'s next hit's seed (`HitSeed::next` of its last).
    pub fn next_seed(&self, part: PartId) -> HitSeed {
        let s = &self.parts[part.index()].seed;
        let next = HitSeed::new(s.load(Ordering::Relaxed)).next();
        s.store(next.get(), Ordering::Relaxed);
        next
    }

    /// Audio: a live note struck `pad` on `part`, for the UI's pad to follow.
    pub fn struck(&self, part: PartId, pad: PadId) {
        let v = &self.parts[part.index()];
        v.struck
            .fetch_or(PadSet::EMPTY.with(pad).bits(), Ordering::Relaxed);
        v.hit.store(pad.index() as u8 + 1, Ordering::Relaxed);
    }

    /// Audio: the pads each Part's voices sound this block.
    pub fn sounding(&self, pads: &[PadSet; MAX_PARTS]) {
        for (v, s) in self.parts.iter().zip(pads) {
            v.sounding.store(s.bits(), Ordering::Relaxed);
        }
    }
}

/// The UI's end of `KIT_VIEW`: the one publisher, and the one taker of hits.
pub struct KitWriter {
    _once: (),
}

impl KitWriter {
    /// The writer, once: every TONAL OFF, no hit.
    pub fn take() -> Option<KitWriter> {
        if WRITER_TAKEN.swap(true, Ordering::AcqRel) {
            return None;
        }
        for v in &KIT_VIEW.parts {
            v.tonal.store(0, Ordering::Relaxed);
            v.hit.store(0, Ordering::Relaxed);
            v.struck.store(0, Ordering::Relaxed);
            v.sounding.store(0, Ordering::Relaxed);
            v.bill.store(0, Ordering::Relaxed);
        }
        Some(KitWriter { _once: () })
    }

    /// `kit` as `part`'s, if it changed: one publish an edit. TONAL and
    /// the bill are stored apart from the pads' slots, each publish: a hit
    /// may play its first block on the last bill.
    pub fn publish(&mut self, part: PartId, kit: &Kit) {
        let v = &KIT_VIEW.parts[part.index()];
        v.tonal.store(kit.tonal().code(), Ordering::Relaxed);
        v.bill.store(kit_bill(kit).to_bits(), Ordering::Relaxed);
        let b = PadId::ALL.map(|p| {
            let (pad, mut b) = (kit.pad(p), [0; VIEW_BYTES]);
            b[..PAD_BYTES].copy_from_slice(&pad.to_bytes());
            b[PAD_BYTES..].copy_from_slice(&pad.sound().to_bytes());
            b
        });
        // Only this writer stores it; odd is a garbage boot value.
        let s = v.seq.load(Ordering::Relaxed) & !1;
        let bytes = |slot: usize| b.iter().flatten().zip(v.slots[slot].iter().flatten());
        if bytes(slot(s)).all(|(x, a)| *x == a.load(Ordering::Relaxed)) {
            return;
        }
        v.seq.store(s.wrapping_add(1), Ordering::Release);
        fence(Ordering::Release);
        for (x, a) in bytes(slot(s) ^ 1) {
            a.store(*x, Ordering::Relaxed);
        }
        v.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Every Part's kit (`publish` each): the UI's, once a loop. On change
    /// only: private issue #953
    pub fn publish_all(&mut self, kits: &Kits) {
        for p in PartId::ALL {
            self.publish(p, kits.kit(p));
        }
    }

    /// The pads `part`'s live hits struck since the last take.
    pub fn take_hit(&mut self, part: PartId) -> Option<Struck> {
        let v = &KIT_VIEW.parts[part.index()];
        let last = PadId::new(v.hit.swap(0, Ordering::Relaxed).wrapping_sub(1))?;
        let pads = PadSet::from_bits(v.struck.swap(0, Ordering::Relaxed)).with(last);
        Some(Struck { last, pads })
    }

    /// The pads `part`'s voices sounded in the audio's last block.
    pub fn sounding(&self, part: PartId) -> PadSet {
        PadSet::from_bits(
            KIT_VIEW.parts[part.index()]
                .sounding
                .load(Ordering::Relaxed),
        )
    }
}

/// A Part's live hits since the UI's last take: the pads struck, and the
/// last of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Struck {
    last: PadId,
    /// `last` always among them.
    pads: PadSet,
}

impl Struck {
    /// The last pad struck: the one the UI selects.
    pub fn last(self) -> PadId {
        self.last
    }

    /// Every pad struck, in pad order.
    pub fn pads(self) -> PadSet {
        self.pads
    }
}
