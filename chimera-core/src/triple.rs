use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU8, Ordering};

// The reader's `front`, the writer's `back` and `latest` are always a
// permutation of 0..3. Both swaps are AcqRel on `latest`: the writer's
// Release publishes its slot, the reader's Release hands its old front back
// before the writer can reuse it. Only the reader clears FRESH, so its first
// Relaxed load is just a hint.
const INDEX: u8 = 0b011;
const FRESH: u8 = 0b100;

pub struct TripleBuffer<T> {
    slots: [UnsafeCell<T>; 3],
    latest: AtomicU8,
}

impl<T> TripleBuffer<T> {
    pub const fn new(first: T, second: T, third: T) -> Self {
        Self {
            slots: [
                UnsafeCell::new(first),
                UnsafeCell::new(second),
                UnsafeCell::new(third),
            ],
            latest: AtomicU8::new(1),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, mut init: impl FnMut() -> T) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from a live `&mut MaybeUninit<Self>`, so it is
        // valid, aligned and unaliased. Each of the three slots and `latest`
        // is written exactly once, through raw field pointers (no reference
        // to uninitialised memory is made), before `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).slots).cast::<UnsafeCell<T>>();
            for i in 0..3 {
                slots.add(i).write(UnsafeCell::new(init()));
            }
            addr_of_mut!((*p).latest).write(AtomicU8::new(1));
            slot.assume_init_mut()
        }
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
    pub fn publish(&mut self, f: impl FnOnce(&mut T)) {
        // SAFETY: `back` is the writer's own slot (the permutation
        // invariant): no other reference to it exists while `f` runs.
        f(unsafe { &mut *self.buf.slots[self.back].get() });
        let prev = self
            .buf
            .latest
            .swap(self.back as u8 | FRESH, Ordering::AcqRel);
        self.back = (prev & INDEX) as usize;
    }
}

impl<T> Reader<T> {
    pub fn read(&mut self) -> &T {
        if self.buf.latest.load(Ordering::Relaxed) & FRESH != 0 {
            let prev = self.buf.latest.swap(self.front as u8, Ordering::AcqRel);
            self.front = (prev & INDEX) as usize;
        }
        // SAFETY: `front` is the reader's own slot; the writer never writes
        // it until a later `read` swaps it out, and that `read` needs
        // `&mut self`, which this returned borrow prevents.
        unsafe { &*self.buf.slots[self.front].get() }
    }
}
