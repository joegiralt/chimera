use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::in_place::uninit_at;

// The reader's `front`, the writer's `back` and `latest` are always a
// permutation of 0..3. Both swaps are AcqRel on `latest`: the writer's
// Release publishes its slot, the reader's Release hands its old front back
// before the writer can reuse it. Only the reader clears FRESH, so its first
// Relaxed load is just a hint.
const INDEX: u8 = 0b011;
const FRESH: u8 = 0b100;
/// `latest` is stored XOR this: its first value, 1, stores as 0, so a
/// buffer of zeroed slots is all zero bytes, in .bss, not a flash copy
/// in .data.
const STORED: u8 = 1;

pub struct TripleBuffer<T> {
    slots: [UnsafeCell<T>; 3],
    latest: AtomicU8,
}

// `init_slots_in_place` writes each field: a new one fails to build here.
crate::in_place::field_list!(TripleBuffer<u8> => TripleBuffer { slots, latest });

impl<T> TripleBuffer<T> {
    pub const fn new(first: T, second: T, third: T) -> Self {
        Self {
            slots: [
                UnsafeCell::new(first),
                UnsafeCell::new(second),
                UnsafeCell::new(third),
            ],
            latest: AtomicU8::new(1 ^ STORED),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, mut init: impl FnMut() -> T) -> &mut Self {
        // SAFETY: `MaybeUninit::write` initialises the whole slot.
        unsafe {
            Self::init_slots_in_place(slot, |s| {
                s.write(init());
            })
        }
    }

    /// As `init_in_place`, but `init` builds each slot where it lies, so a
    /// large `T` never sits on the stack.
    ///
    /// # Safety
    /// `init` must initialise the whole slot it is given.
    pub unsafe fn init_slots_in_place(
        slot: &mut MaybeUninit<Self>,
        mut init: impl FnMut(&mut MaybeUninit<T>),
    ) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from a live `&mut MaybeUninit<Self>`, so it is
        // valid, aligned and unaliased. `UnsafeCell<T>` is `repr(transparent)`
        // over `T`, so each slot is a `T`; the caller's `init` initialises
        // each of the three, and `latest` (the last field, `field_list!`
        // above) is written once, before
        // `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).slots).cast::<T>();
            for i in 0..3 {
                init(uninit_at(slots.add(i)));
            }
            addr_of_mut!((*p).latest).write(AtomicU8::new(1 ^ STORED));
            slot.assume_init_mut()
        }
    }

    /// The first slot of a buffer not built yet, as scratch for one `T`
    /// until `init_slots_in_place` builds it.
    pub fn first_slot(slot: &mut MaybeUninit<Self>) -> &mut MaybeUninit<T> {
        // SAFETY: as in `init_slots_in_place`, the first slot is a `T` in
        // the live, unaliased `slot`, and is borrowed only as long as it.
        unsafe { uninit_at(addr_of_mut!((*slot.as_mut_ptr()).slots).cast::<T>()) }
    }
}

impl<T: 'static> TripleBuffer<T> {
    pub fn split(&'static mut self) -> (Writer<T>, Reader<T>) {
        let buf: &'static Self = self;
        (Writer { buf, back: 2 }, Reader { buf, front: 0 })
    }
}

pub struct Writer<T: 'static> {
    buf: &'static TripleBuffer<T>,
    back: usize,
}

pub struct Reader<T: 'static> {
    buf: &'static TripleBuffer<T>,
    front: usize,
}

// SAFETY: a `Writer` touches only its `back` slot, which neither `latest`
// nor the reader's `front` names, and the index handover is atomic. Moving
// it to another thread moves `T` values across threads, hence `T: Send`.
unsafe impl<T: Send> Send for Writer<T> {}
// SAFETY: a `Reader` touches only its `front` slot, which the writer never
// writes until the reader swaps it back out; `T: Send` as for `Writer`.
unsafe impl<T: Send> Send for Reader<T> {}

impl<T> Writer<T> {
    /// The reader has taken the last publish: a later one is all it misses.
    pub fn taken(&self) -> bool {
        (self.buf.latest.load(Ordering::Acquire) ^ STORED) & FRESH == 0
    }

    pub fn publish(&mut self, f: impl FnOnce(&mut T)) {
        // SAFETY: `back` is the writer's own slot (the permutation
        // invariant): no other reference to it exists while `f` runs.
        f(unsafe { &mut *self.buf.slots[self.back].get() });
        let prev = self
            .buf
            .latest
            .swap((self.back as u8 | FRESH) ^ STORED, Ordering::AcqRel);
        self.back = ((prev ^ STORED) & INDEX) as usize;
    }
}

impl<T> Reader<T> {
    pub fn read(&mut self) -> &T {
        if self.buf.latest.load(Ordering::Relaxed) & FRESH != 0 {
            let prev = self
                .buf
                .latest
                .swap(self.front as u8 ^ STORED, Ordering::AcqRel);
            self.front = ((prev ^ STORED) & INDEX) as usize;
        }
        // SAFETY: `front` is the reader's own slot; the writer never writes
        // it until a later `read` swaps it out, and that `read` needs
        // `&mut self`, which this returned borrow prevents.
        unsafe { &*self.buf.slots[self.front].get() }
    }
}
