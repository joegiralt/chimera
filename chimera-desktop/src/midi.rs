//! The desktop's midir input: port choice, a pure function so "no device"
//! and "loopback only" are host-tested without opening a real port; and
//! the port's bytes through a `DinPipe`, as the firmware's DIN.

use std::sync::atomic::{AtomicU32, Ordering};

use chimera_core::midi_in::{self, CtrlRing, DinSinks, PipeSlot};
use chimera_core::note_queue::NoteProducer;

/// The port's pipe as of its last bytes: `status`'s `midi` line.
pub static FAULTS: AtomicU32 = AtomicU32::new(0);
pub static DROPS: AtomicU32 = AtomicU32::new(0);

/// A MIDI port's bytes: its pipe, into its note queue and control ring.
pub struct Port<'q> {
    pipe: PipeSlot,
    notes: NoteProducer<'q>,
    ctrl: &'q CtrlRing,
}

impl<'q> Port<'q> {
    pub fn new(notes: NoteProducer<'q>, ctrl: &'q CtrlRing) -> Self {
        Self {
            pipe: PipeSlot::new(),
            notes,
            ctrl,
        }
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        let mut out = DinSinks {
            notes: &mut self.notes,
            ctrl: self.ctrl,
        };
        midi_in::feed(&mut self.pipe, bytes, &mut out);
        let s = self.pipe.stats();
        FAULTS.store(s.fault_total(), Ordering::Relaxed);
        DROPS.store(u32::from(s.refused), Ordering::Relaxed);
    }
}

pub fn pick_port(names: &[String], wanted: Option<&str>) -> Option<usize> {
    let lower = |s: &str| s.to_ascii_lowercase();
    match wanted {
        Some(w) => names.iter().position(|n| lower(n).contains(&lower(w))),
        None => names.iter().position(|n| !lower(n).contains("through")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Port, pick_port};
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::midi_in::CtrlRing;
    use chimera_core::note_queue::{InEvent, NoteKind, NoteQueue};
    use chimera_core::params::FilterParams;
    use chimera_core::project::PartId;
    use chimera_core::ui::UiState;
    use chimera_hal::Ms;

    /// The desktop's QA by bytes: a keyboard's note reaches the queue, and
    /// INIT's mapped CC 74 sweeps CUTOFF, through the port's pipe and ring
    /// as the midir callback and the UI loop run them.
    #[test]
    fn a_port_plays_notes_and_sweeps_cutoff() {
        const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
        let q = NoteQueue::new();
        let (notes, mut audio) = q.split().unwrap();
        let ring = CtrlRing::new();
        let mut port = Port::new(notes, &ring);
        let mut ui = Box::new(UiState::new());
        let p1 = PartId::ALL[0];
        let ch = ui.project().part(p1).mix.channel.get();
        let cutoff = |ui: &UiState| {
            ui.project()
                .read_part(p1)
                .block(CUTOFF.block)
                .unwrap()
                .get(CUTOFF.param)
        };
        let mut now = 1_000;
        let mut sweep_to = |ui: &mut UiState, port: &mut Port, v: u8| {
            port.bytes(&[0xB0 | ch, 74, v]);
            for _ in 0..100 {
                ui.hear_ccs(ring.drain(), Ms(now), true);
                now += 4;
            }
            cutoff(ui)
        };
        port.bytes(&[0x90 | ch, 60, 100]);
        match audio.pop() {
            Some(InEvent::Note(n)) => {
                assert_eq!((n.channel.get(), n.note.get()), (ch, 60));
                assert!(matches!(n.kind, NoteKind::On(_)));
            }
            e => panic!("{e:?}"),
        }
        let low = sweep_to(&mut ui, &mut port, 0);
        let high = sweep_to(&mut ui, &mut port, 127);
        assert!(high > low, "CC 74 sweeps CUTOFF: {low} -> {high}");
        assert_eq!(super::DROPS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_first_real_port_wins() {
        let n = names(&[
            "Midi Through:Midi Through Port-0 14:0",
            "KeyStep 37:KeyStep 37 MIDI 1 24:0",
        ]);
        assert_eq!(pick_port(&n, None), Some(1));
    }

    #[test]
    fn a_wanted_name_matches_case_insensitively() {
        let n = names(&["Arturia KeyStep", "nanoKEY2"]);
        assert_eq!(pick_port(&n, Some("NANOkey")), Some(1));
        assert_eq!(pick_port(&n, Some("launchpad")), None);
    }

    #[test]
    fn no_ports_or_only_loopback_picks_nothing() {
        assert_eq!(pick_port(&[], None), None);
        assert_eq!(
            pick_port(&names(&["Midi Through:Midi Through Port-0 14:0"]), None),
            None
        );
    }
}
