use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::scope::{ScopeFrame, scope_buffer};
use chimera_core::triple::{Reader, TripleBuffer, Writer};

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();
static SCOPE_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_scope() -> Option<(Writer<ScopeFrame>, Reader<ScopeFrame>)> {
    if SCOPE_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `SCOPE` ever made.
    Some(unsafe { &mut *addr_of_mut!(SCOPE) }.split())
}
