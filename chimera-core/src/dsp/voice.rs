use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::addr::Blocks;
use crate::block::apply_offset;
use crate::dsp::algo::engine::AlgoLive;
use crate::dsp::drive::Drive;
use crate::dsp::engines::Engines;
use crate::dsp::envelope::{EnvMods, Envelope};
use crate::dsp::filter::SvfFilter;
use crate::dsp::lfo::Lfo;
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::wavefolder::Wavefolder;
use crate::hw::{Cost, MAX_VOICES, VOICE_RAM_BUDGET};
use crate::in_place::{by_value, uninit_at};
use crate::modulation::{MAX_MOD_SOURCES, ModSource, ModState, note_source};
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

// ADR 0013: the voice pool fits D2 SRAM beside the DMA buffers, on both targets.
const _: () = assert!(core::mem::size_of::<[Voice; MAX_VOICES]>() <= VOICE_RAM_BUDGET);

/// Complete voice signal chain:
/// [Engine] → [Drive] → [Filter] → [Wavefolder] → [VCA]
/// Modulators: three envelopes, three LFOs
pub struct Voice {
    engines: Engines,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    envs: [Envelope; 3],
    lfos: [Lfo; 3],
    active_engine: EngineType,
    active: bool,
    last_note: MidiNote,
    last_velocity: Velocity,
    /// Samples left of a fade-out (`kill`, an engine change); 0 when not
    /// fading. Never set on an inactive voice.
    fade: u16,
    /// What starts when the fade ends.
    after_fade: AfterFade,
    /// Key down since the last note-on.
    held: bool,
    /// The modulated settings of the last block rendered. A fade keeps
    /// them, so a new Sound never reaches the sound it fades out.
    played: ParamSnapshot,
    played_live: AlgoLive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFade {
    Idle,
    /// The held note, on the Sound's new engine; a key up cancels it.
    Restart,
    /// A note-on that came mid-fade: `last_note`.
    Note,
}

// `reset` overwrites fields in place without dropping them.
const _: () = assert!(!core::mem::needs_drop::<Voice>());

/// Writes every field of `*p` but `engines`. The pattern is exhaustive, so
/// a new field fails to build until it is written here.
macro_rules! write_chain {
    ($p:ident, { $($f:ident: $v:expr),* $(,)? }) => {{
        let _every_field = |v: &Voice| {
            let Voice { engines: _, $($f: _),* } = v;
        };
        $(addr_of_mut!((*$p).$f).write($v);)*
    }};
}

impl Default for Voice {
    fn default() -> Self {
        Self::new(chimera_hal::SAMPLE_RATE)
    }
}

impl Voice {
    /// The chain's floor: engine costs are bench per-voice minus this. The
    /// bench's `FLOOR` row (an Algo patch with every LEVEL at 0) measured 5 on
    /// 2026-09-27; 10 is kept, erring high.
    pub const CHAIN_COST: Cost = Cost(10); // measured 2026-09-26, bench, rev V at 480 MHz

    /// A `kill` ramps to silence over this many samples (ADR 0027).
    pub const FADE: u16 = 2 * BLOCK_SIZE as u16;

    /// Cycles/sample of a voice playing `p` under `mods`.
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        Engines::cost(p, mods) + Self::CHAIN_COST
    }

    /// The sample rate is stored once (spec §3), not passed per call.
    pub fn new(sample_rate: u32) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the engines are built in place
        // and every other (small) field is written once before
        // `assume_init_mut`.
        unsafe {
            Engines::init_in_place(uninit_at(addr_of_mut!((*p).engines)), sample_rate);
            Self::init_chain(p);
            slot.assume_init_mut()
        }
    }

    /// Every field but `engines`, as `new` builds it: the one list that
    /// `init_in_place` and `reset` share.
    ///
    /// # Safety
    /// `p` must be valid for writes, aligned and unaliased.
    unsafe fn init_chain(p: *mut Self) {
        // SAFETY: the caller's guarantee; every field is written by value,
        // and none has drop glue (asserted above) to skip.
        unsafe {
            let played = ParamSnapshot::default();
            write_chain!(p, {
                drive: Drive::new(),
                filter: SvfFilter::new(),
                folder: Wavefolder::new(),
                envs: [Envelope::new(); 3],
                lfos: [Lfo::new(); 3],
                active_engine: EngineType::Algo,
                active: false,
                last_note: MidiNote::A4,
                last_velocity: Velocity::DEFAULT,
                fade: 0,
                after_fade: AfterFade::Idle,
                held: false,
                played_live: AlgoLive::from_params(&played.algo),
                played: played,
            });
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.engines.sample_rate()
    }

    /// On a fading voice, or one sounding another engine, the note waits
    /// for the fade-out and then starts clean. Returns whether it replaced
    /// a note still waiting, unheard.
    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) -> bool {
        let replaced = self.after_fade == AfterFade::Note;
        self.held = true;
        if self.fade > 0 || (self.active && params.engine() != self.active_engine) {
            self.last_note = note;
            self.last_velocity = velocity;
            self.after_fade = AfterFade::Note;
            self.fade_out();
            return replaced;
        }
        self.trigger(note, velocity, params);
        replaced
    }

    fn trigger(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) {
        if !self.active {
            self.filter.hold(); // a fresh note: no ramp from the last note's cutoff
        }
        // The engine left behind starts clean when it next plays.
        if params.engine() != self.active_engine {
            self.engines.reset(self.active_engine);
        }
        self.active_engine = params.engine();
        self.last_note = note;
        self.last_velocity = velocity;
        self.engines
            .note_on(self.active_engine, note, velocity, params);
        for e in &mut self.envs {
            e.note_on();
        }
        for (l, p) in self.lfos.iter_mut().zip(&params.lfos) {
            l.note_on(p);
        }
        self.active = true;
        // Until the first block renders, a fade has these to keep.
        self.played.clone_from(params);
        self.played_live = AlgoLive::from_params(&params.algo);
    }

    pub fn note_off(&mut self) {
        self.held = false;
        if self.after_fade == AfterFade::Restart {
            self.after_fade = AfterFade::Idle;
        }
        self.engines.note_off(self.active_engine);
    }

    /// Fade to silence over `FADE` samples, then go idle as a fresh voice.
    /// A second call mid-fade does not restart it. Returns whether a note
    /// that came mid-fade was dropped unheard.
    pub fn kill(&mut self) -> bool {
        let dropped = self.after_fade == AfterFade::Note;
        self.after_fade = AfterFade::Idle;
        self.fade_out();
        dropped
    }

    fn fade_out(&mut self) {
        if self.active && self.fade == 0 {
            self.fade = Self::FADE;
        }
    }

    /// The fade is over: back to fresh, then start a queued note.
    fn fade_ended(&mut self, params: &ParamSnapshot) {
        let (after, held) = (self.after_fade, self.held);
        let (note, velocity) = (self.last_note, self.last_velocity);
        self.reset();
        if after != AfterFade::Idle {
            // Both engines are clean now: `trigger` has none to reset.
            self.active_engine = params.engine();
            self.held = true;
            self.trigger(note, velocity, params);
            if !held {
                self.note_off();
            }
        }
    }

    /// Back to the state `new` builds, in place.
    fn reset(&mut self) {
        self.engines.reset(self.active_engine);
        // SAFETY: `self` is a valid, aligned, unaliased `Voice`.
        unsafe { Self::init_chain(self) }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        mod_state: &ModState,
    ) {
        let sample_rate = self.sample_rate();

        // The Sound changed engine: fade the old one out; a held note then
        // restarts on the new one.
        if self.active && self.fade == 0 && params.engine() != self.active_engine {
            if self.held {
                self.after_fade = AfterFade::Restart;
            }
            self.fade_out();
        }

        if !self.active {
            output.fill(0.0);
            return;
        }

        // The modulators run every block, fading or not, from the settings
        // the voice plays (spec § Signal flow 1).
        let src = if self.fade == 0 { params } else { &self.played };
        let key = self.held;
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        for (s, env) in EnvSlot::ALL.iter().zip(self.envs.iter_mut()) {
            mod_values[ModSource::of_env(*s).index()] = env.run_block(
                &src.envelopes[s.index()],
                &EnvMods::NONE,
                key,
                sample_rate,
                None,
            );
        }
        for (s, lfo) in LfoSlot::ALL.iter().zip(self.lfos.iter_mut()) {
            mod_values[ModSource::of_lfo(*s).index()] =
                lfo.run_block(&src.lfos[s.index()], sample_rate);
        }
        mod_values[ModSource::Vel.index()] = self.last_velocity.unit();
        mod_values[ModSource::Note.index()] = note_source(self.last_note);

        // A fading voice keeps the settings it last played.
        if self.fade == 0 {
            // Every routed destination gets its offset through its block's
            // spec (spec §4).
            let (m, live) = (&mut self.played, &mut self.played_live);
            m.clone_from(params);
            *live = AlgoLive::from_params(&params.algo);
            live.routed = mod_state.algo_levels_routed();
            for d in 0..mod_state.num_dests() {
                let off = mod_state.sum_for(d, &mod_values);
                if off != 0.0 {
                    let a = mod_state.dest(d);
                    if live.offset(a, off) {
                        continue;
                    }
                    // Modulatable addresses are always Sound blocks (`voice_reads`).
                    if let Some(blk) = m.block_mut(a.block) {
                        apply_offset(blk, a.param, off);
                    }
                }
            }
        }
        let (m, live) = (&self.played, &self.played_live);

        // 1. Engine → raw oscillator output
        self.engines.render(self.active_engine, output, m, live);

        // 2. Drive
        self.drive.process(output, &m.drive);

        // 3. Filter
        self.filter.process(output, &m.filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &m.folder);

        // 5. Volume. No engine puts an envelope on the VCA yet.
        let volume = m.out.volume;
        for sample in output.iter_mut() {
            *sample *= volume;
        }

        // Check if done
        self.active = self.engines.is_active(self.active_engine);

        if self.fade > 0 {
            for sample in output.iter_mut() {
                self.fade = self.fade.saturating_sub(1);
                *sample *= f32::from(self.fade) / f32::from(Self::FADE);
            }
            if self.fade == 0 || !self.active {
                self.fade_ended(params);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec § 3: a fresh note starts without a ramp, even on a voice
    /// whose engine went quiet on its own (inactive, never reset).
    #[test]
    fn a_note_after_silence_starts_without_a_ramp() {
        let p = ParamSnapshot::default();
        let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        let mut b = [0.0f32; BLOCK_SIZE];
        v.render(&mut b, &p, &ModState::new());
        assert!(v.filter.last_g().is_some());
        v.active = false; // its engine went quiet
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        assert!(v.filter.last_g().is_none());
        // A retrigger of a sounding voice keeps it.
        v.render(&mut b, &p, &ModState::new());
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        assert!(v.filter.last_g().is_some());
    }
}
