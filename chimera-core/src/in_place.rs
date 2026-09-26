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
