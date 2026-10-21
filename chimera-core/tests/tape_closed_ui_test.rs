//! TAPE's UI with bank 2 closed (ADR 0175), under `bank2` on a host, where
//! the UI's table answers only while the door cell is open: SOURCE reads
//! NEEDS UPDATE, its turn and MIX + PLUS queue nothing, a job queued while
//! open is dropped, and the strip and its key are empty. One test: the
//! door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod common;
mod screen;

use chimera_core::bank2::{DOOR_CELL, NEEDS_UPDATE};
use chimera_core::dsp::tape_engine::Heads;

use chimera_core::dsp::tape_engine::params::{TapeEngineParams, TapeMachine};
use chimera_core::params::EngineType;
use chimera_core::project::LoadLink;
use chimera_core::storage::{Card, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::settings::CardCx;
use chimera_core::ui::tape_strip::{Overview, draw_strip, key};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{OriginDimensions, Size};
use embedded_graphics::pixelcolor::Rgb565;
use screen::{Input, feed, load_init};

/// Counts the pixels drawn.
struct Count(usize);

impl OriginDimensions for Count {
    fn size(&self) -> Size {
        Size::new(320, 240)
    }
}

impl DrawTarget for Count {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        p: I,
    ) -> Result<(), Self::Error> {
        self.0 += p.into_iter().count();
        Ok(())
    }
}

fn band(ui: &UiState) -> String {
    let mut b = FmtBuf::new();
    ui.source_state().text(&mut b);
    b.as_str().into()
}

fn strip_pixels() -> usize {
    let mut c = Count(0);
    let p = TapeEngineParams::default();
    draw_strip(
        &mut c,
        &Overview::EMPTY,
        TapeMachine::Walk,
        &p,
        Heads::NONE,
        0,
        0,
    );
    c.0
}

#[test]
fn closed_tape_ui_says_so_and_does_nothing() {
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut s);
    let link = LoadLink::new();
    let mut ui = Box::new(UiState::new());
    DOOR_CELL.open_host();
    load_init(&mut ui, EngineType::Tape);
    assert_ne!(band(&ui), NEEDS_UPDATE);
    assert!(strip_pixels() > 0, "open, the empty strip's flat line");

    // Open, a turn with the card unlisted queues its listing.
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert!(ui.card_pending());

    DOOR_CELL.close();
    assert_eq!(band(&ui), NEEDS_UPDATE);
    // The job queued while open is dropped by the next card pass.
    let cx = CardCx {
        card: &mut card,
        store: &mut s,
        sync: &mut sync,
        settings: &mut set,
    };
    let _ = ui.card_work(cx, &link, |swap, _| {
        let _ = swap.settle(&link, || false);
    });
    assert!(!ui.card_pending(), "the queued job dropped");
    // Turning and MIX + PLUS queue nothing.
    let before = ui
        .project()
        .part(chimera_core::project::PartId::ALL[0])
        .sound
        .source;
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert!(!ui.card_pending() && !ui.source_loading());
    assert_eq!(
        ui.project()
            .part(chimera_core::project::PartId::ALL[0])
            .sound
            .source,
        before
    );
    // The strip and its key are empty.
    assert_eq!(strip_pixels(), 0);
    let p = TapeEngineParams::default();
    assert_eq!(key(&Overview::EMPTY, TapeMachine::Walk, &p, Heads::NONE), 0);
}
