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
#[allow(unused_macros)]
macro_rules! in_place_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $Name:ident $(<$G:ident: $Bound:path>)? {
            $( $Variant:ident($Payload:ty) => $rebuild:ident, $init:ident; )+
        }
    ) => {
        $(#[$meta])*
        #[repr(C, u8)]
        $vis enum $Name $(<$G: $Bound>)? { $( $Variant($Payload), )+ }

        const _: () = {
            use core::mem::{align_of, needs_drop, size_of, ManuallyDrop, MaybeUninit};

            #[derive(Clone, Copy)]
            #[repr(u8)]
            enum Tag { $( $Variant, )+ }

            #[allow(non_snake_case)]
            #[repr(C)]
            union Payload $(<$G: $Bound>)? { $( $Variant: ManuallyDrop<$Payload>, )+ }

            #[repr(C)]
            struct Mirror $(<$G: $Bound>)? {
                tag: Tag,
                payload: Payload $(<$G>)?,
            }

            impl $(<$G: $Bound>)? $Name $(<$G>)? {
                const LAYOUT: () = {
                    $( assert!(!needs_drop::<$Payload>(), "a payload has drop glue"); )+
                    assert!(size_of::<Mirror $(<$G>)?>() == size_of::<Self>());
                    assert!(align_of::<Mirror $(<$G>)?>() == align_of::<Self>());
                    let mut largest = 0;
                    $( if size_of::<$Payload>() > largest { largest = size_of::<$Payload>(); } )+
                    assert!(size_of::<Self>() <= largest + align_of::<Self>());
                };

                fn mirror(this: *mut Self) -> *mut Mirror $(<$G>)? {
                    this.cast()
                }

                $(
                    /// # Safety
                    /// `init` must initialise every field of the slot it is given
                    /// before returning (as this crate's in-place constructors do), not merely
                    /// return some other `&mut T`. It must not unwind: the tag is
                    /// already written.
                    #[allow(dead_code)]
                    pub(crate) unsafe fn $rebuild(
                        &mut self,
                        init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload,
                    ) {
                        let () = Self::LAYOUT;
                        // SAFETY: `self` is valid, aligned and unaliased; the
                        // payloads have no drop glue (`LAYOUT`), so the old
                        // variant may be overwritten, and `init` leaves a
                        // valid `$Variant` (the caller's contract).
                        unsafe { Self::$init($crate::in_place::uninit_at(self), init) };
                    }

                    /// # Safety
                    /// `init` must initialise every field of the slot it is given
                    /// before returning (as this crate's in-place constructors do), not merely
                    /// return some other `&mut T`. It must not unwind: the tag is
                    /// already written.
                    #[allow(dead_code)]
                    pub(crate) unsafe fn $init(
                        slot: &mut MaybeUninit<Self>,
                        init: impl FnOnce(&mut MaybeUninit<$Payload>) -> &mut $Payload,
                    ) -> &mut Self {
                        let () = Self::LAYOUT;
                        // This builds the variant: say so to the dead-code lint.
                        let _: fn($Payload) -> Self = Self::$Variant;
                        let m = Self::mirror(slot.as_mut_ptr());
                        // SAFETY: `m` is `slot`, whose layout is the mirror's
                        // (`LAYOUT`); writing a field through a raw pointer
                        // reads nothing uninitialised, and the payload
                        // pointer is aligned, in bounds and unaliased while
                        // `slot` is borrowed. `ManuallyDrop<P>` is `P`'s layout.
                        unsafe {
                            (&raw mut (*m).tag).write(Tag::$Variant);
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
                    let m = core::ptr::from_ref(self).cast::<Mirror $(<$G>)?>();
                    // SAFETY: `self` is a valid enum with the mirror's layout
                    // (`LAYOUT`), so its tag is a valid `Tag`; nothing is
                    // written.
                    unsafe { ((*m).tag as u8, (&raw const (*m).payload).cast::<u8>()) }
                }
            }
        };
    };
}
#[allow(unused_imports)]
pub(crate) use in_place_enum;

#[cfg(test)]
mod tests {
    extern crate std;
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
}
