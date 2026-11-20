//! The delay under SYNC on ORBIT's clock (ADR 0302), through
//! `OrbitRunner` and `Instrument` as the reverb's is
//! (`reverb_sync_test`'s rig). Its own binary: it renders, so it
//! publishes the global clock.

mod common;

use chimera_core::dsp::delay::sync::NOTES;
use chimera_core::dsp::delay::{DelayNote, DelayParams, DelaySync};
use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::hw::SampleBudget;
use chimera_core::meter::{delay_fold, rev_clock};
use chimera_core::note_queue::RealTime;
use chimera_core::orbit::runner::OrbitRunner;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::inst_rig::InstRig;
use common::rev::blocks;

/// ORBIT's runner handing an `Instrument` its tempo a block at a time,
/// fed a MIDI clock at `bpm` whose ticks land ±`jit_ms` off their beat.
struct Clocked {
    r: InstRig,
    runner: Box<OrbitRunner>,
    next: f64,
    due: f64,
    seed: u32,
}

impl Clocked {
    fn new(delay: DelayParams) -> Self {
        let mut r = InstRig::with_budget(SampleBudget::for_cpu(480_000_000));
        r.shared.fx.delay = delay;
        let sr = core::num::NonZeroU32::new(SR).unwrap();
        Clocked {
            r,
            runner: Box::new(OrbitRunner::new(sr)),
            next: 0.0,
            due: 0.0,
            seed: 0x2545_f491,
        }
    }

    fn block(&mut self, bpm: Option<f32>, jit_ms: f32) {
        let end = (self.runner.now() + BLOCK_SIZE as u64) as f64;
        while let Some(b) = bpm
            && self.due < end
        {
            let now = self.runner.now();
            self.runner.realtime(0, RealTime::Clock, now);
            self.next += SR as f64 * 60.0 / (b as f64 * 24.0);
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            let j = (self.seed as f64 / u32::MAX as f64 * 2.0 - 1.0) * jit_ms as f64;
            self.due = self.next + j * SR as f64 / 1000.0;
        }
        if bpm.is_none() {
            (self.next, self.due) = (end, end);
        }
        self.runner.block(&mut self.r.inst, &self.r.shared);
        self.r.render(1);
    }
}

fn synced(n: DelayNote) -> DelayParams {
    DelayParams::DEFAULT
        .with(DelayParams::SYNC, DelaySync::Sync.value())
        .with(DelayParams::NOTE, n.value())
}

/// A clock jittering ±0.5 ms a tick moves the synced delay's head once at
/// most, when the settled average is taken (a glide, or a crossfade where
/// the tempo is far from the 120 BPM it started on), and then never
/// again: no glide, no crossfade, the head on 1/8D of the tempo held. With
/// no clock the project's 120 BPM plays, never the knob's ms; and the
/// fold the cells read is the engine's (1/4 at 97 BPM, halved).
#[test]
fn a_jittering_clock_never_moves_the_synced_delay() {
    for bpm in [120.0, 128.0, 97.0] {
        let mut free_ms = synced(DelayNote::DEFAULT);
        free_ms.time_ms = 20.0;
        let mut c = Clocked::new(free_ms);
        let mut seen = vec![];
        for b in 0..blocks(9.0) {
            c.block((b >= blocks(1.0)).then_some(bpm), 0.5);
            seen.push((c.r.inst.tempo(), c.r.fx.delay().head()));
        }
        // Before the clock: the project's tempo.
        assert_eq!(seen[blocks(0.9)], (Tempo(120.0), (18_000.0, 0)));
        let moves: Vec<usize> = (1..seen.len()).filter(|&b| seen[b].0 != seen[b - 1].0).collect();
        assert!(moves.len() <= 1, "{bpm}: {moves:?}");
        // The one move over (a 20 ms crossfade, or a glide), the head
        // stays put.
        let from = moves.last().copied().unwrap_or(0) + blocks(0.4);
        let (t, head) = seen[from];
        assert!((t.0 - bpm).abs() <= 0.5, "{bpm}: {t:?}");
        let ms = NOTES[DelayNote::DEFAULT.index()].ms(t);
        assert_eq!(head, (ms * SR as f32 / 1000.0, 0), "{bpm}: at {t:?}");
        assert!(
            seen[from..].iter().all(|s| s.1 == head),
            "{bpm}: the head moved after the clock settled"
        );
        assert_eq!(delay_fold(), FoldState(0));
    }
    let mut c = Clocked::new(synced(DelayNote::new(DelayNote::COUNT - 1)));
    for b in 0..blocks(4.0) {
        c.block((b >= blocks(1.0)).then_some(97.0), 0.0);
    }
    assert_eq!(c.r.fx.delay().fold(), FoldState(1));
    assert_eq!(delay_fold(), FoldState(1));
    assert_eq!(rev_clock().0, c.r.inst.tempo());
}
