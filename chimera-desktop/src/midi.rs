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

    const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

    /// A UI with Part 1's CUTOFF bound to `input`, at mid-range, and a
    /// port feeding its ring.
    fn bound(input: chimera_core::midi_map::Input) -> Box<UiState> {
        use chimera_core::addr::Blocks;
        let mut ui = Box::new(UiState::new());
        let s = ui.project_mut().edit_part(PartId::ALL[0]).sound;
        let m = chimera_core::midi_map::Mappable::new(CUTOFF).unwrap();
        s.map.learn(input, m).unwrap();
        let spec = CUTOFF.spec().unwrap();
        let mut b = s.params.block_mut(CUTOFF.block).unwrap();
        b.set(CUTOFF.param, spec.denormalize(0.5));
        ui
    }

    fn cutoff(ui: &UiState) -> f32 {
        let p = ui.project().read_part(PartId::ALL[0]);
        p.block(CUTOFF.block).unwrap().get(CUTOFF.param)
    }

    /// A BCR2000 knob in relative-2 (binary offset: +1 = 41, -1 = 3F), byte
    /// by byte through the port, a UI pass each: CUTOFF turns detent by
    /// detent, each exactly the front panel's.
    #[test]
    fn a_bcr_relative_2_stream_turns_cutoff_detent_by_detent() {
        use chimera_core::midi_map::{CcKind, Input, LearnCc};
        use chimera_midi::rel::ByteEncoding;
        let input = Input::Cc(
            LearnCc::new(81).unwrap(),
            CcKind::Relative(ByteEncoding::BinaryOffset),
        );
        let mut ui = bound(input);
        let mut panel = bound(input);
        let ring = CtrlRing::new();
        let q = NoteQueue::new();
        let (notes, _audio) = q.split().unwrap();
        let mut port = Port::new(notes, &ring);
        let ch = ui.project().part(PartId::ALL[0]).mix.channel.get();
        let start = cutoff(&ui);
        let mut now = 1_000;
        for (byte, d) in [(0x41u8, 1i8); 12].into_iter().chain([(0x3F, -1); 20]) {
            port.bytes(&[0xB0 | ch, 81, byte]);
            now += 4;
            ui.hear_ccs(ring.drain(), Ms(now), true);
            panel.edit_addr(PartId::ALL[0], CUTOFF, d, false);
            assert_eq!(cutoff(&ui), cutoff(&panel), "{byte:#x}");
        }
        assert!(cutoff(&ui) < start, "12 up, 20 down");
    }

    /// An EC4 encoder in 14-bit absolute (CC 2 then CC 34, MSB first) swept
    /// across the range, a pair a UI pass: CUTOFF climbs smoothly, never
    /// back, no step past two messages' worth, and lands on the top.
    #[test]
    fn an_ec4_14_bit_stream_sweeps_cutoff_smoothly() {
        use chimera_core::midi_map::{Cc14Msb, Input};
        use chimera_midi::take::Takeover;
        let mut ui = bound(Input::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Jump));
        let ring = CtrlRing::new();
        let q = NoteQueue::new();
        let (notes, _audio) = q.split().unwrap();
        let mut port = Port::new(notes, &ring);
        let ch = ui.project().part(PartId::ALL[0]).mix.channel.get();
        let spec = CUTOFF.spec().unwrap();
        let place = |ui: &UiState| spec.normalize(cutoff(ui));
        let mut now = 1_000;
        let mut pass = |ui: &mut UiState, port: &mut Port, v: u16| {
            let (msb, lsb) = ((v >> 7) as u8, (v & 0x7F) as u8);
            port.bytes(&[0xB0 | ch, 2, msb, 34, lsb]);
            now += 4;
            ui.hear_ccs(ring.drain(), Ms(now), true);
        };
        // From the bottom, so the sweep starts where CUTOFF is.
        pass(&mut ui, &mut port, 0);
        for _ in 0..50 {
            pass(&mut ui, &mut port, 0);
        }
        assert_eq!(place(&ui), 0.0);
        let (step, mut was, mut most) = (64u16, 0.0f32, 0.0f32);
        for v in (step..=16383).step_by(usize::from(step)).chain([16383]) {
            pass(&mut ui, &mut port, v);
            let now = place(&ui);
            assert!(now >= was, "never back: {was} -> {now} at {v}");
            most = most.max(now - was);
            was = now;
        }
        for _ in 0..50 {
            pass(&mut ui, &mut port, 16383);
        }
        assert_eq!(place(&ui), 1.0, "lands on the top");
        let bound = 2.0 * f32::from(step) / 16383.0;
        assert!(most <= bound, "a pass moved {most}, bound {bound}");
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
