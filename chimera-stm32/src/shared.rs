use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::instrument::AudioShared;
use chimera_core::preset::Performance;
use chimera_core::scope::{ScopeFrame, scope_buffer};
use chimera_core::triple::{Reader, TripleBuffer, Writer};
use chimera_core::ui::UiState;

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

// `UiState` is ~27 KB: it lives in AXI, not in `main`'s frame on the 128 KB
// DTCM stack (ADR 0020).
static mut UI: MaybeUninit<UiState> = MaybeUninit::uninit();
static UI_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_ui() -> Option<&'static mut UiState> {
    if UI_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `UI` ever made.
    Some(UiState::init_in_place(unsafe { &mut *addr_of_mut!(UI) }))
}

static mut AUDIO: MaybeUninit<TripleBuffer<AudioShared>> = MaybeUninit::uninit();
static AUDIO_TAKEN: AtomicBool = AtomicBool::new(false);

// Seeded from the UI's `Performance`, not `AudioShared::default`, whose
// temporary `Performance` puts an 8 KB frame on the stack.
pub fn take_audio(perf: &Performance) -> Option<(Writer<AudioShared>, Reader<AudioShared>)> {
    if AUDIO_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `AUDIO` ever made; it is built in place, one 3 KB copy
    // at a time.
    let slot = unsafe { &mut *addr_of_mut!(AUDIO) };
    Some(TripleBuffer::init_in_place(slot, || AudioShared::from_performance(perf)).split())
}
