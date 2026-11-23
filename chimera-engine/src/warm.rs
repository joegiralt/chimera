// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bench only (`bench-warm`, the bench-algo set): a hook around each
//! voice's render, through which the bench's WARM page renders a voice's
//! block twice, the second from a copy of the voice, and times both
//! (`chimera-stm32`'s `bench::warm_twice`, bank 2's). Measurement only:
//! the second render is the one that plays.

use crate::dsp::voice::Voice;
use core::sync::atomic::{AtomicPtr, Ordering::Relaxed};

/// What the bench runs in place of a voice's render: the voice and its
/// render.
pub type Hook = fn(&mut Voice, &mut dyn FnMut(&mut Voice));

static HOOK: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Sets the hook (`None`: renders run as they always do).
pub fn set_hook(h: Option<Hook>) {
    HOOK.store(h.map_or(core::ptr::null_mut(), |f| f as *mut ()), Relaxed);
}

/// `render` on `v`, or the hook on both while one is set.
#[inline(never)]
pub(crate) fn twice(v: &mut Voice, render: &mut dyn FnMut(&mut Voice)) {
    // Opaque: LTO, seeing the bench's one hook, would call it directly,
    // a branch from bank 1 into bank 2 the door check refuses.
    let p = core::hint::black_box(HOOK.load(Relaxed));
    if p.is_null() {
        render(v);
    } else {
        // SAFETY: only `set_hook` stores a non-null pointer, a `Hook`.
        let h: Hook = unsafe { core::mem::transmute(p) };
        h(v, render);
    }
}
