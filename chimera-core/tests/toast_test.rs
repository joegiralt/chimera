//! Leaving System: no BUSY before the card work; a short toast after it,
//! counted down by the frame clock, never a sleep.

use chimera_core::project::Line;
use chimera_core::storage::{Card, Exit, FileError, SyncError, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::{Toast, ToastStep, ToastTimer, toast_for};
use chimera_core::ui::chain::ChainId;
use chimera_core::ui::theme_settings::Bright;
use chimera_hal::store::{StoreError, Unsupported};
use chimera_hal::testkit::MemStore;

#[test]
fn toast_for_table() {
    let saved = Some(Toast {
        text: Line::new("SAVED"),
        ms: Toast::SAVED_MS,
    });
    let err = |text| {
        Some(Toast {
            text: Line::new(text),
            ms: Toast::ERROR_MS,
        })
    };
    assert_eq!(toast_for(&Ok(Exit::Wrote)), saved);
    assert_eq!(toast_for(&Ok(Exit::Loaded)), None);
    assert_eq!(toast_for(&Ok(Exit::Unchanged)), None);
    for e in [
        StoreError::NoCard,
        StoreError::Full,
        StoreError::Timeout,
        StoreError::Io,
        StoreError::Corrupt,
        StoreError::NotFound,
        StoreError::Unsupported(Unsupported::Exfat),
        StoreError::Unsupported(Unsupported::NoPartitionTable),
        StoreError::Unsupported(Unsupported::BadBootSector),
        StoreError::Unsupported(Unsupported::FatNotMirrored),
    ] {
        assert_eq!(
            toast_for(&Err(SyncError::Store(e))),
            err(e.message()),
            "{e:?}"
        );
    }
    for e in [
        FileError::Truncated,
        FileError::BadMagic,
        FileError::BadCrc,
        FileError::NeedsNewerFirmware,
        FileError::WrongKind,
        FileError::Bounds,
        FileError::BadName,
        FileError::Corrupt,
    ] {
        assert_eq!(
            toast_for(&Err(SyncError::File(e))),
            err(e.message()),
            "{e:?}"
        );
    }
}

#[test]
fn toast_expires_after_its_time() {
    let mut t = ToastTimer::new();
    assert_eq!(t.step(16), ToastStep::Idle);
    t.show(Toast {
        text: Line::new("SAVED"),
        ms: 600,
    });
    assert_eq!(t.step(0), ToastStep::Show(Line::new("SAVED")));
    assert_eq!(t.step(599), ToastStep::Show(Line::new("SAVED")));
    // The frame that runs out clears it once, so the page repaints.
    assert_eq!(t.step(1), ToastStep::Ended);
    assert_eq!(t.step(16), ToastStep::Idle);
}

/// The timer starts when the toast is shown: the first step spans the card
/// work that made it, however long that took, and doesn't count.
#[test]
fn a_slow_operation_does_not_eat_the_toast() {
    let mut t = ToastTimer::new();
    t.show(Toast {
        text: Line::new("CARD TIMEOUT"),
        ms: Toast::ERROR_MS,
    });
    assert_eq!(t.step(10_000), ToastStep::Show(Line::new("CARD TIMEOUT")));
    assert_eq!(
        t.step(Toast::ERROR_MS - 1),
        ToastStep::Show(Line::new("CARD TIMEOUT"))
    );
    assert_eq!(t.step(1), ToastStep::Ended);
}

#[test]
fn dismiss_ends_it_early_once() {
    let mut t = ToastTimer::new();
    t.dismiss();
    assert_eq!(
        t.step(0),
        ToastStep::Idle,
        "nothing shown, nothing to clear"
    );
    t.show(Toast {
        text: Line::new("SAVED"),
        ms: 600,
    });
    t.dismiss();
    assert_eq!(t.step(0), ToastStep::Ended);
    assert_eq!(t.step(0), ToastStep::Idle);
}

/// One frame of the shells' loop after input, without a display: the exit
/// step can't draw, so no BUSY lands on leaving System; what it leaves is
/// the toast the next render draws.
fn frame(
    ui: &mut UiState,
    sync: &mut SystemSync,
    card: &mut Card,
    s: &mut MemStore,
    cur: &mut SystemSettings,
) -> ToastStep {
    ui.sync_system(sync, card, s, cur);
    // The step after the card work spans it: a slow one must not count.
    ui.step_toast(5_000)
}

#[test]
fn leaving_system_toasts_saved_not_busy() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    let mut ui = UiState::new();
    ui.set_theme(cur.theme);

    ui.nav.chain_id = ChainId::System;
    assert_eq!(
        frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur),
        ToastStep::Idle
    );
    let mut theme = ui.theme();
    theme.bright = Bright::new(40);
    ui.set_theme(theme);
    ui.nav.chain_id = ChainId::Part(0);
    assert_eq!(
        frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur),
        ToastStep::Show(Line::new("SAVED"))
    );
    assert_eq!(cur.theme, theme, "the save took the UI's theme");

    // Back in and out with no change: nothing to do, no toast.
    ui.step_toast(Toast::SAVED_MS);
    ui.nav.chain_id = ChainId::System;
    frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur);
    ui.nav.chain_id = ChainId::Part(0);
    assert_eq!(
        frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur),
        ToastStep::Idle
    );
}

#[test]
fn a_loaded_theme_is_its_own_feedback() {
    let mut s = MemStore::new(1);
    let mut want = SystemSettings::DEFAULT;
    want.theme.bright = Bright::new(40);
    {
        let mut card = Card::new();
        let (mut sync, ..) = SystemSync::boot(&mut card, &mut s);
        sync.write(&mut card, &mut s, &want).unwrap();
    }
    // Booted with no card: untouched defaults, then the card goes in.
    s.eject();
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    s.insert();
    let mut ui = UiState::new();
    ui.nav.chain_id = ChainId::System;
    frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur);
    ui.nav.chain_id = ChainId::Part(0);
    assert_eq!(
        frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur),
        ToastStep::Idle
    );
    assert_eq!(ui.theme(), want.theme, "loaded and applied");
}

#[test]
fn a_failed_exit_toasts_its_message() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    s.eject();
    let mut ui = UiState::new();
    ui.nav.chain_id = ChainId::System;
    frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur);
    ui.nav.chain_id = ChainId::Part(0);
    assert_eq!(
        frame(&mut ui, &mut sync, &mut card, &mut s, &mut cur),
        ToastStep::Show(Line::new("NO CARD"))
    );
    assert_eq!(
        ui.step_toast(Toast::ERROR_MS - 1),
        ToastStep::Show(Line::new("NO CARD"))
    );
    assert_eq!(ui.step_toast(1), ToastStep::Ended);
}
