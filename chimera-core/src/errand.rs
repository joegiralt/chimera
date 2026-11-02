//! The proof the UI loop is on an errand: long work (card work, SYSTEM's
//! sync, a sweep's ask, bank 2's install) takes `&Errand`, so the board
//! can't run it outside the scope that puts its USB guard's service test
//! to sleep (ADR 0222). Zero-sized: no code.

pub struct Errand(());

impl Errand {
    /// `f` with the proof. In firmware the board's `errand` scope is the
    /// one caller, around which it sleeps the guard's service test.
    #[inline(always)]
    pub fn run<R>(f: impl FnOnce(&Errand) -> R) -> R {
        f(&Errand(()))
    }

    /// Host code (tests, the desktop) has no guard to sleep.
    #[cfg(not(target_os = "none"))]
    pub const HOST: &Errand = &Errand(());
}
