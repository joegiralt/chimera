use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::dsp::algo::waves::{self, Waves};
use chimera_core::instrument::AudioShared;
#[cfg(feature = "orbit")]
use chimera_core::orbit::runner::OrbitView;
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

// The wave tables' zero-wait working copy, at the top of DTCM (dtcm.x).
#[unsafe(link_section = ".dtcm_waves")]
static mut WAVES_DTCM: MaybeUninit<Waves> = MaybeUninit::uninit();
static WAVES_TAKEN: AtomicBool = AtomicBool::new(false);

/// Copies the wave tables from flash into DTCM and reads them from there.
pub fn copy_waves() {
    if WAVES_TAKEN.swap(true, Ordering::AcqRel) {
        return;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `WAVES_DTCM` ever made.
    waves::copy_into(unsafe { &mut *addr_of_mut!(WAVES_DTCM) });
}

// `UiState` is ~27 KB: it lives in AXI, not in `main`'s frame on the
// ~63.75 KB DTCM stack (ADR 0025).
static mut UI: MaybeUninit<UiState> = MaybeUninit::uninit();
static UI_TAKEN: AtomicBool = AtomicBool::new(false);

/// Out of line for flash, not the stack: built in place, its frame is
/// 1 KB, but inlined into `synth` fat LTO reshuffles the UI loop's
/// inlining for 6.3 KB more `.text` (ADR 0089).
#[inline(never)]
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
    // reference to `AUDIO` ever made.
    let slot = unsafe { &mut *addr_of_mut!(AUDIO) };
    // SAFETY: `AudioShared::init_in_place` writes every field of each slot,
    // one Part at a time, so no whole `AudioShared` is ever on the stack.
    let buf = unsafe {
        TripleBuffer::init_slots_in_place(slot, |s| {
            AudioShared::init_in_place(s, perf);
        })
    };
    Some(buf.split())
}

// ORBIT's view, from the runner to the UI: AXI, beside `AUDIO`.
#[cfg(feature = "orbit")]
static mut ORBIT: MaybeUninit<TripleBuffer<OrbitView>> = MaybeUninit::uninit();
#[cfg(feature = "orbit")]
static ORBIT_TAKEN: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "orbit")]
pub fn take_orbit() -> Option<(Writer<OrbitView>, Reader<OrbitView>)> {
    if ORBIT_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `ORBIT` ever made.
    let slot = unsafe { &mut *addr_of_mut!(ORBIT) };
    Some(TripleBuffer::init_in_place(slot, OrbitView::default).split())
}
