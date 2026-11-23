//! ORBIT's hooks in the shell, its one seam: the runner on the audio
//! thread, and its view, the beat LED and its publish on the UI loop.
//! Without the `orbit` feature each is a no-op twin with the same calls,
//! and nothing of the runner links.

use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::note_queue::{NoteEvent, RealTime};
use chimera_core::orbit::door::OrbitPorts;
use chimera_core::source_table::GateView;
use chimera_core::ui::UiState;
use stm32h7xx_hal::gpio::{Output, PB1, PushPull};

/// The front panel's beat LED.
pub type BeatLed = PB1<Output<PushPull>>;

#[cfg(feature = "orbit")]
pub use on::*;

#[cfg(not(feature = "orbit"))]
pub use off::*;

#[cfg(feature = "orbit")]
mod on {
    use super::*;
    use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
    use chimera_core::triple::{Reader, Writer};

    use chimera_core::hw::SAMPLE_RATE;

    /// The rate ORBIT's runner counts in.
    pub const SAMPLE_RATE_NZ: core::num::NonZeroU32 = match core::num::NonZeroU32::new(SAMPLE_RATE)
    {
        Some(r) => r,
        None => panic!("a sample rate of 0"),
    };

    /// The runner and its view's writer.
    pub struct Runner {
        runner: OrbitRunner,
        view: Writer<OrbitView>,
    }

    /// The view's reader and the beat LED.
    pub struct Shell {
        view: Reader<OrbitView>,
        led: BeatLed,
    }

    /// The view's buffer split between the two, once.
    pub fn take(led: BeatLed) -> (Runner, Shell) {
        let (w, r) = crate::shared::take_orbit().expect("ORBIT view taken once");
        let runner = Runner {
            runner: OrbitRunner::new(SAMPLE_RATE_NZ),
            view: w,
        };
        (runner, Shell { view: r, led })
    }

    // The audio interrupt's calls: inline, so they compile in the binary,
    // in the fast profile (ADR 0108).
    impl Runner {
        #[inline]
        pub fn now(&self) -> u64 {
            self.runner.now()
        }

        /// The runner, for the load gate's step: a `bank2 erase` shuts it
        /// (#784).
        #[inline]
        pub fn runner(&mut self) -> Option<&mut OrbitRunner> {
            Some(&mut self.runner)
        }

        /// A live key, raw, before any snap: capture sees it.
        #[inline]
        pub fn live(&mut self, n: NoteEvent) {
            self.runner.live(n);
        }

        #[inline]
        pub fn realtime(&mut self, src: usize, rt: RealTime, now: u64) {
            self.runner.realtime(src, rt, now);
        }

        #[inline]
        pub fn block<P: OrbitPorts>(
            &mut self,
            inst: &mut Instrument,
            shared: &AudioShared,
            src: &GateView<'_>,
            ports: &mut P,
        ) {
            self.runner.block_ported(inst, shared, src, ports);
        }

        /// The runner's view, for the UI.
        #[inline]
        pub fn publish(&mut self) {
            let r = &self.runner;
            self.view.publish(|v| r.fill_view(v));
        }
    }

    impl Shell {
        /// The runner's latest view into the UI.
        pub fn see(&mut self, ui: &mut UiState) {
            ui.set_orbit_view(self.view.read());
        }

        /// The LED lit on the beat, off when ORBIT is stopped.
        pub fn beat(&mut self, ui: &UiState) {
            if ui.led() {
                self.led.set_high();
            } else {
                self.led.set_low();
            }
        }
    }

    /// ORBIT's settings into an audio publish.
    pub fn publish(ui: &UiState, b: &mut AudioShared) {
        ui.publish_orbit(&mut b.orbit);
    }
}

#[cfg(not(feature = "orbit"))]
mod off {
    use super::*;

    pub struct Runner;

    /// The beat LED, held low.
    pub struct Shell;

    pub fn take(mut led: BeatLed) -> (Runner, Shell) {
        led.set_low();
        (Runner, Shell)
    }

    impl Runner {
        pub fn now(&self) -> u64 {
            0
        }

        /// No ORBIT: nothing for the load gate to shut.
        #[inline(always)]
        pub fn runner(&mut self) -> Option<&mut chimera_core::orbit::runner::OrbitRunner> {
            None
        }

        pub fn live(&mut self, _: NoteEvent) {}

        /// Without ORBIT nothing follows a clock.
        pub fn realtime(&mut self, _: usize, _: RealTime, _: u64) {}

        #[inline(always)]
        pub fn block<P: OrbitPorts>(
            &mut self,
            _: &mut Instrument,
            _: &AudioShared,
            _: &GateView<'_>,
            _: &mut P,
        ) {
        }

        pub fn publish(&mut self) {}
    }

    impl Shell {
        pub fn see(&mut self, _: &mut UiState) {}

        pub fn beat(&mut self, _: &UiState) {}
    }

    pub fn publish(_: &UiState, _: &mut AudioShared) {}
}
