//! The proof the UI loop is on an errand: long work (card work, SYSTEM's
//! sync, a sweep's ask, bank 2's install) takes `&Errand`, so the board
//! can't run it outside the scope that puts its USB guard's service test
//! to sleep (ADR 0222). Zero-sized: no code.
//!
//! Only the board mints one: `run` is `unsafe`, so a bare call fails to
//! build.
//!
//! ```compile_fail,E0133
//! chimera_core::errand::Errand::run(|_| ());
//! ```

pub struct Errand(());

impl Errand {
    /// `f` with the proof.
    ///
    /// # Safety
    ///
    /// No USB guard's service test can trip over `f`: the board's errand
    /// scope has put it to sleep, no guard runs (no port up, or a build
    /// without one), or `f` is short (`UiState::errand_due` false).
    /// Broken, a long `f` trips a false storm: USB MIDI held off a lap.
    #[inline(always)]
    pub unsafe fn run<R>(f: impl FnOnce(&Errand) -> R) -> R {
        f(&Errand(()))
    }

    /// Host code (tests, the desktop) has no guard to sleep.
    #[cfg(not(target_os = "none"))]
    pub const HOST: &Errand = &Errand(());
}
