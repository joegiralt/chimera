use core::mem::MaybeUninit;

/// # Safety
/// `p` must be non-null, aligned, valid for writes for `'a` and not aliased.
pub(crate) unsafe fn uninit_at<'a, T>(p: *mut T) -> &'a mut MaybeUninit<T> {
    // SAFETY: `MaybeUninit<T>` has `T`'s layout; the caller guarantees the
    // pointer is valid, aligned and unaliased for `'a`.
    unsafe { &mut *p.cast::<MaybeUninit<T>>() }
}

/// # Safety
/// `init` must initialise every field of the slot it is given
/// before returning (as this crate's in-place constructors do), not merely
/// return some other `&mut T`.
pub(crate) unsafe fn by_value<T>(init: impl FnOnce(&mut MaybeUninit<T>) -> &mut T) -> T {
    let mut slot = MaybeUninit::uninit();
    init(&mut slot);
    // SAFETY: the caller guarantees `init` wrote every field of `slot`.
    unsafe { slot.assume_init() }
}

// Lists every field of a struct by name: adding, removing or renaming a
// field fails to compile here, a prompt to re-check the in-place
// constructor beside it. It does not check that constructor, nor field
// types.
macro_rules! field_list {
    ($ty:ty => $name:ident { $($field:ident),* $(,)? }) => {
        const _: fn(&$ty) = |v| {
            let $name { $($field: _),* } = v;
        };
    };
}
pub(crate) use field_list;

// A `#[repr(C, u8)]` enum whose variants are built where the enum lives,
// never on the stack and moved in. The Reference defines that repr as a
// `repr(C)` struct of a `u8` tag and a `repr(C)` union of the payloads; a
// private mirror of it gives each variant's tag and payload address. Per
// variant `V(P) => rebuild_v, init_v`:
// - `rebuild_v(&mut self, init)` turns the enum into `V`, in place;
// - `init_v(slot, init)` builds `V` in an uninitialised slot.
// Payloads must have no drop glue: a rebuild overwrites the old one.
//
// The grammar is narrow on purpose: at most one generic parameter with a
// single path bound (`<S: Store>`), and tuple variants of one payload each.
macro_rules! in_place_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $Name:ident $(<$G:ident: $Bound:path>)? {
            $( $Variant:ident($Payload:ty) => $rebuild:ident, $init:ident; )+
        }
    ) => {
        $(#[$meta])*
        #[allow(dead_code, reason = "variants are built through the mirror")]
        #[repr(C, u8)]
        $vis enum $Name $(<$G: $Bound>)? { $( $Variant($Payload), )+ }

        const _: () = {
            use core::mem::{align_of, needs_drop, size_of, ManuallyDrop, MaybeUninit};

            #[derive(Clone, Copy)]
            #[repr(u8)]
            enum __Tag { $( $Variant, )+ }

            #[allow(non_snake_case)]
            #[repr(C)]
            union __Payload $(<$G: $Bound>)? { $( $Variant: ManuallyDrop<$Payload>, )+ }

            #[repr(C)]
            struct __Mirror $(<$G: $Bound>)? {
                tag: __Tag,
                payload: __Payload $(<$G>)?,
            }

            // Held across a rebuild's `init`, forgotten once it returns. If
            // `init` unwinds, this drop panics during the unwind, which
            // aborts: the tag already names a payload never written.
            struct __AbortOnUnwind;
            impl Drop for __AbortOnUnwind {
                fn drop(&mut self) {
                    panic!("in_place_enum: a rebuild's init unwound");
                }
            }

            impl $(<$G: $Bound>)? $Name $(<$G>)? {
                const LAYOUT: () = {
                    $( assert!(!needs_drop::<$Payload>(), "a payload has drop glue"); )+
                    assert!(size_of::<__Mirror $(<$G>)?>() == size_of::<Self>());
                    assert!(align_of::<__Mirror $(<$G>)?>() == align_of::<Self>());
                    // The union rounds the largest payload up to the enum's
                    // align; the tag takes one align more. Never the sum.
                    let mut largest = 0;
                    $( if size_of::<$Payload>() > largest { largest = size_of::<$Payload>(); } )+
                    let align = align_of::<Self>();
                    assert!(size_of::<Self>() <= largest.next_multiple_of(align) + align);
                };

                fn mirror(this: *mut Self) -> *mut __Mirror $(<$G>)? {
                    this.cast()
                }

                $(
                    /// If `init` unwinds, the process aborts.
                    ///
                    /// # Safety
                    /// `init` must initialise every field of the slot it is given
                    /// before returning (as this crate's in-place constructors do), not merely
                    /// return some other `&mut T`.
                    #[allow(dead_code, reason = "a slot need not use every constructor")]
                    pub(crate) unsafe fn $rebuild(
                        &mut self,
                        init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload,
                    ) {
                        let () = Self::LAYOUT;
                        let bomb = __AbortOnUnwind;
                        // SAFETY: `self` is valid, aligned and unaliased; the
                        // payloads have no drop glue (`LAYOUT`), so the old
                        // variant may be overwritten, and `init` leaves a
                        // valid `$Variant` (the caller's contract) or unwinds
                        // into `bomb`, which aborts before anything sees it.
                        unsafe { Self::$init($crate::in_place::uninit_at(self), init) };
                        core::mem::forget(bomb);
                    }

                    /// # Safety
                    /// `init` must initialise every field of the slot it is given
                    /// before returning (as this crate's in-place constructors do), not merely
                    /// return some other `&mut T`.
                    #[allow(dead_code, reason = "a slot need not use every constructor")]
                    pub(crate) unsafe fn $init(
                        slot: &mut MaybeUninit<Self>,
                        init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload,
                    ) -> &mut Self {
                        let () = Self::LAYOUT;
                        let m = Self::mirror(slot.as_mut_ptr());
                        // SAFETY: `m` is `slot`, whose layout is the mirror's
                        // (`LAYOUT`); writing a field through a raw pointer
                        // reads nothing uninitialised, and the payload
                        // pointer is aligned, in bounds and unaliased while
                        // `slot` is borrowed. `ManuallyDrop<P>` is `P`'s layout.
                        unsafe {
                            (&raw mut (*m).tag).write(__Tag::$Variant);
                            let payload = (&raw mut (*m).payload.$Variant).cast::<$Payload>();
                            init($crate::in_place::uninit_at(payload));
                        }
                        // SAFETY: the tag is `$Variant`'s and `init` wrote
                        // every field of its payload (the caller's contract).
                        unsafe { slot.assume_init_mut() }
                    }
                )+

                /// The tag byte and the payload address, read through the mirror.
                #[cfg(test)]
                pub(crate) fn mirror_parts(&self) -> (u8, *const u8) {
                    let () = Self::LAYOUT;
                    let m = core::ptr::from_ref(self).cast::<__Mirror $(<$G>)?>();
                    // SAFETY: `self` is a valid enum with the mirror's layout
                    // (`LAYOUT`), so its tag is a valid `__Tag`; nothing is
                    // written.
                    unsafe { ((*m).tag as u8, (&raw const (*m).payload).cast::<u8>()) }
                }
            }
        };
    };
}
pub(crate) use in_place_enum;

#[cfg(test)]
mod tests {
    extern crate std;
    use core::mem::MaybeUninit;
    use std::boxed::Box;

    #[derive(Clone, Copy)]
    struct Wide<T>([T; 3], u16);

    in_place_enum! {
        enum Toy<T: Copy> {
            Small(u8) => rebuild_small, init_small;
            Wide(Wide<T>) => rebuild_wide, init_wide;
        }
    }

    #[test]
    fn slot_layout_matches_repr() {
        let mut raw = Box::<Toy<u64>>::new_uninit();
        // SAFETY: the box is valid for `size_of::<Toy<u64>>()` byte writes.
        unsafe {
            raw.as_mut_ptr()
                .cast::<u8>()
                .write_bytes(0xA5, size_of::<Toy<u64>>())
        };
        // SAFETY: the closure writes every field.
        unsafe { Toy::init_small(&mut raw, |p| p.write(3)) };
        // SAFETY: `init_small` built a valid `Toy` in the box.
        let mut slot = unsafe { raw.assume_init() };

        assert_eq!(slot.mirror_parts().0, 0);
        match &*slot {
            Toy::Small(p) => {
                assert_eq!(*p, 3);
                assert_eq!(p as *const u8, slot.mirror_parts().1)
            }
            _ => panic!(),
        }
        // SAFETY: the closure writes every field.
        unsafe { slot.rebuild_wide(|w| w.write(Wide([7; 3], 9))) };
        assert_eq!(slot.mirror_parts().0, 1);
        match &*slot {
            Toy::Wide(w) => {
                assert_eq!((w.0, w.1), ([7; 3], 9));
                assert_eq!(w as *const _ as *const u8, slot.mirror_parts().1)
            }
            _ => panic!(),
        }
        // SAFETY: the closure writes every field.
        unsafe { slot.rebuild_small(|p| p.write(5)) };
        assert!(matches!(*slot, Toy::Small(5)));
    }

    in_place_enum! {
        enum Odd {
            Bytes([u8; 9]) => rebuild_bytes, init_bytes;
            Word(u64) => rebuild_word, init_word;
        }
    }

    // A 9-byte payload beside an 8-aligned one: the union rounds up to 16,
    // so the enum is 24, more than the largest payload plus the tag's align.
    #[test]
    fn odd_payload_beside_aligned_one() {
        let mut raw = MaybeUninit::<Odd>::uninit();
        // SAFETY: the closure writes every field.
        let slot = unsafe { Odd::init_bytes(&mut raw, |p| p.write([1; 9])) };
        assert_eq!(slot.mirror_parts().0, 0);
        match &*slot {
            Odd::Bytes(p) => assert_eq!(p.as_ptr(), slot.mirror_parts().1),
            _ => panic!(),
        }
        // SAFETY: the closure writes every field.
        unsafe { slot.rebuild_word(|w| w.write(u64::MAX)) };
        assert_eq!(slot.mirror_parts().0, 1);
        match &*slot {
            Odd::Word(w) => {
                assert_eq!(*w, u64::MAX);
                assert_eq!(w as *const u64 as *const u8, slot.mirror_parts().1)
            }
            _ => panic!(),
        }
    }

    // A rebuild whose `init` unwinds would leave the tag naming a payload
    // never written, so it aborts instead. An abort can't be caught in
    // process: the test re-runs itself as a child and checks it died of
    // SIGABRT, not a panic's exit code.
    #[test]
    fn unwinding_init_aborts_the_rebuild() {
        const CHILD: &str = "CHIMERA_IN_PLACE_ABORT_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut slot = Box::new(Odd::Word(0));
            // SAFETY: the closure never returns, so writes nothing it owes.
            unsafe { slot.rebuild_bytes(|_| panic!("init unwinds")) };
            return;
        }
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "in_place::tests::unwinding_init_aborts_the_rebuild",
            ])
            .args(["--test-threads", "1"])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(6), "{status}");
        }
        #[cfg(not(unix))]
        assert!(!status.success() && status.code() != Some(101), "{status}");
    }
}
