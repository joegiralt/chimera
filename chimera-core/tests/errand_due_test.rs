//! `errand_due` (ADR 0222): false only on a lap whose errand work never
//! touches the card, so the board runs it with the USB guard awake.

mod screen;

use chimera_core::project::LoadLink;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::settings::CardCx;
use chimera_hal::card_bus::CardFault;
use chimera_hal::store::{
    ByteSink, Dir, FileName, FileRuns, ReadSink, RunsError, Store, StoreError, VolumeId,
};
use chimera_hal::testkit::MemStore;
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, hold, tap, to_leaf};

/// `MemStore`, its every touch counted, its read fault set by hand.
struct Counted {
    s: MemStore,
    touches: u32,
    fault: CardFault,
}

impl Store for Counted {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.touches += 1;
        self.s.mount()
    }
    fn read_fault(&self) -> CardFault {
        self.fault
    }
    fn recover(&mut self) -> Result<VolumeId, StoreError> {
        self.touches += 1;
        self.fault = CardFault::Clear;
        self.s.mount()
    }
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.list(vol, dir, f)
    }
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.read(vol, file, sink)
    }
    fn read_at(
        &mut self,
        vol: VolumeId,
        file: FileName,
        offset: u32,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.read_at(vol, file, offset, sink)
    }
    fn runs(&mut self, vol: VolumeId, file: FileName) -> Result<FileRuns, RunsError> {
        self.touches += 1;
        self.s.runs(vol, file)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.touches += 1;
        self.s.write(vol, file, body)
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.delete(vol, file)
    }
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.make_dir(vol, dir)
    }
    fn seal(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.touches += 1;
        self.s.seal(vol, file)
    }
}

/// The board's lap, its errand's calls in its order.
struct Shell {
    s: Counted,
    card: Card,
    sync: SystemSync,
    set: SystemSettings,
    link: LoadLink,
}

impl Shell {
    fn new() -> Shell {
        let mut s = Counted {
            s: MemStore::new(1),
            touches: 0,
            fault: CardFault::Clear,
        };
        let mut card = Card::new();
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        Shell {
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
        }
    }

    /// The errand's work; whether it was due, and whether it touched the card.
    fn lap(&mut self, ui: &mut UiState) -> (bool, bool) {
        let due = ui.errand_due(&self.sync, self.s.read_fault());
        let before = self.s.touches;
        let e = chimera_core::errand::Errand::HOST;
        let _ = ui.take_dfu_synced(
            e,
            &mut self.sync,
            &mut self.card,
            &mut self.s,
            &mut self.set,
        );
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.s,
            sync: &mut self.sync,
            settings: &mut self.set,
        };
        let _ = ui.card_work(e, cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
        ui.sync_system(
            e,
            &mut self.sync,
            &mut self.card,
            &mut self.s,
            &mut self.set,
        );
        (due, self.s.touches != before)
    }
}

/// Through idle, SETTINGS in and out, a quick save, a DFU yes and a read
/// fault: no lap the errand skips touches the card, and each kind is due.
#[test]
fn a_lap_not_due_never_touches_the_card() {
    let mut ui = Box::new(UiState::new());
    let mut sh = Shell::new();
    let mut due_laps = 0;
    let mut step = |ui: &mut UiState, sh: &mut Shell, what: &str, want_due: bool| {
        let (due, touched) = sh.lap(ui);
        assert!(due || !touched, "{what}: not due, yet the card was touched");
        assert_eq!(due, want_due, "{what}");
        due_laps += due as u32;
        // The next lap is quiet again.
        let (due, touched) = sh.lap(ui);
        assert!(!due && !touched, "{what}: the lap after");
    };
    step(&mut ui, &mut sh, "idle", false);
    tap(&mut ui, ButtonId::Menu);
    step(&mut ui, &mut sh, "into SETTINGS", false);
    tap(&mut ui, ButtonId::Menu);
    assert!(!ui.in_settings());
    step(&mut ui, &mut sh, "out of SETTINGS: SYSTEM synced", true);
    hold(&mut ui, ButtonId::Menu);
    step(&mut ui, &mut sh, "quick save", true);
    to_leaf(&mut ui, &["SYSTEM"]);
    sh.lap(&mut ui);
    tap(&mut ui, ButtonId::Edit);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    tap(&mut ui, ButtonId::Edit);
    step(&mut ui, &mut sh, "DFU's yes", true);
    sh.s.fault = CardFault::Read;
    step(&mut ui, &mut sh, "a read fault", true);
    assert_eq!(due_laps, 4);
}
