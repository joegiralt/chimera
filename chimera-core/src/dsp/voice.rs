use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, Blocks};
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
use crate::modulation::{
    MAX_MOD_SOURCES, ModRouting, ModSource, ModState, VCA, amount_scale, note_source,
};
use crate::params::{EngineType, EnvParams, ParamSnapshot};
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
    /// The ENV destinations' sums from the last block's matrix (spec § Signal flow 1).
    env_mods: [EnvMods; 3],
    /// The sources' values from the last block; the VCA's other routes
    /// ramp from them.
    mod_values: [f32; MAX_MOD_SOURCES],
    /// The routes into VCA this voice plays.
    vca: VcaRoutes,
    /// A note-on since the last block: its ENV destinations are recomputed
    /// with its own VEL and NOTE before the modulators run.
    retrigger: bool,
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

/// The routes into VCA a voice plays, kept from the last block before a
/// fade (spec § 4; a fading voice keeps its routes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct VcaRoutes {
    bits: u8,
    /// `amount / 127` per source.
    amount: [f32; MAX_MOD_SOURCES],
}

impl VcaRoutes {
    fn of(m: &ModState) -> Self {
        let mut r = Self::default();
        if let Some(d) = m.find(VCA) {
            r.bits = m.present(d);
            for (s, a) in r.amount.iter_mut().enumerate() {
                *a = amount_scale(m.amount(s, d));
            }
        }
        r
    }

    fn has(&self, source: usize) -> bool {
        self.bits & (1 << source) != 0
    }
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
        Engines::cost(p, mods) + Self::CHAIN_COST + ModRouting::cost(p, mods)
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
                env_mods: [EnvMods::NONE; 3],
                mod_values: [0.0; MAX_MOD_SOURCES],
                vca: VcaRoutes::default(),
                retrigger: false,
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
            // An idle voice's ENV slots and last source values start as a
            // fresh voice's. Idle is silent: a release its VCA routes still
            // hold keeps the voice active (`vca_holds`).
            self.envs = [Envelope::new(); 3];
            self.mod_values = [0.0; MAX_MOD_SOURCES];
        }
        self.retrigger = true;
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

    /// Sounding: its engine is active and, with routes into VCA, one of
    /// them holds it (spec § 4).
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// A VCA source still holds the voice (spec § 4): an ENV slot per its
    /// TYPE and FORM, anything else while the key is held.
    fn vca_holds(&self) -> bool {
        let key = self.held;
        ModSource::ALL
            .iter()
            .filter(|s| self.vca.has(s.index()))
            .any(|s| match s.env_slot() {
                Some(e) => self.envs[e.index()].holds(key),
                None => key,
            })
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
        if self.fade == 0 {
            self.vca = VcaRoutes::of(mod_state);
        }
        let vca = self.vca;
        // The VCA's gain (spec § 4): 64 samples on the audio stack, zeroed
        // only with a route into VCA.
        let mut gain = (vca.bits != 0).then_some([0.0f32; BLOCK_SIZE]);
        if core::mem::take(&mut self.retrigger) {
            // The last block's ENV and LFO values; the new note's VEL and NOTE.
            let mut v = self.mod_values;
            v[ModSource::Vel.index()] = self.last_velocity.unit();
            v[ModSource::Note.index()] = note_source(self.last_note);
            let mut next = [EnvMods::NONE; 3];
            for d in 0..mod_state.num_dests() {
                if let BlockRef::Env(s) = mod_state.dest(d).block {
                    env_mod(&mut next[s.index()], mod_state, d, &v);
                }
            }
            self.env_mods = next;
        }
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        for (s, env) in EnvSlot::ALL.iter().zip(self.envs.iter_mut()) {
            let i = ModSource::of_env(*s).index();
            let feed = gain
                .as_mut()
                .filter(|_| vca.has(i))
                .map(|g| (g, vca.amount[i]));
            mod_values[i] = env.run_block(
                &src.envelopes[s.index()],
                &self.env_mods[s.index()],
                key,
                sample_rate,
                feed,
            );
        }
        for (s, lfo) in LfoSlot::ALL.iter().zip(self.lfos.iter_mut()) {
            mod_values[ModSource::of_lfo(*s).index()] =
                lfo.run_block(&src.lfos[s.index()], sample_rate);
        }
        mod_values[ModSource::Vel.index()] = self.last_velocity.unit();
        mod_values[ModSource::Note.index()] = note_source(self.last_note);
        // The other VCA sources ramp from their last block's value.
        for (s, &cur) in mod_values.iter().enumerate() {
            if let Some(gain) = gain.as_mut().filter(|_| vca.has(s))
                && ModSource::ALL[s].env_slot().is_none()
            {
                let a = vca.amount[s];
                let (from, step) = (
                    a * self.mod_values[s],
                    a * (cur - self.mod_values[s]) / BLOCK_SIZE as f32,
                );
                for (n, g) in gain.iter_mut().enumerate() {
                    *g += from + step * n as f32;
                }
            }
        }

        // A fading voice keeps the settings it last played.
        if self.fade == 0 {
            // Every routed destination gets its offset through its block's
            // spec (spec §4).
            let (m, live) = (&mut self.played, &mut self.played_live);
            m.clone_from(params);
            *live = AlgoLive::from_params(&params.algo);
            live.routed = mod_state.algo_levels_routed();
            let mut next = [EnvMods::NONE; 3];
            for d in 0..mod_state.num_dests() {
                let a = mod_state.dest(d);
                if a == VCA {
                    continue; // per sample, above
                }
                if let BlockRef::Env(s) = a.block {
                    // ENV destinations reach their slot next block.
                    env_mod(&mut next[s.index()], mod_state, d, &mod_values);
                    continue;
                }
                let off = mod_state.sum_for(d, &mod_values);
                if off != 0.0 {
                    if live.offset(a, off) {
                        continue;
                    }
                    // Modulatable addresses are always Sound blocks (`voice_reads`).
                    if let Some(blk) = m.block_mut(a.block) {
                        apply_offset(blk, a.param, off);
                    }
                }
            }
            self.env_mods = next;
        }
        self.mod_values = mod_values;
        let (m, live) = (&self.played, &self.played_live);

        // 1. Engine → raw oscillator output
        self.engines.render(self.active_engine, output, m, live);

        // 2. Drive
        self.drive.process(output, &m.drive);

        // 3. Filter
        self.filter.process(output, &m.filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &m.folder);

        // 5. The VCA, after the fold.
        let volume = m.out.volume;
        // The block's last routed gain, × AMP's VEL: whether the lifetime
        // check below ends the voice through a fade.
        let mut last_gain = 0.0;
        match &gain {
            None => {
                // No route: the engine decides (spec § 4). No wildcard, so a new
                // engine can't inherit the pass-through (VA gates: #148).
                match self.active_engine {
                    EngineType::Algo | EngineType::Modal => {
                        // Its own envelopes shape the sound: today's expression, bit for bit.
                        for sample in output.iter_mut() {
                            *sample *= volume;
                        }
                    }
                }
            }
            Some(gain) => {
                let vel = 1.0 - m.out.vca_vel + m.out.vca_vel * self.last_velocity.unit();
                let k = volume * vel;
                #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
                for (sample, g) in output.iter_mut().zip(gain) {
                    *sample *= k * g.max(0.0).min(1.0);
                }
                #[allow(clippy::manual_clamp)]
                let g = gain[BLOCK_SIZE - 1].max(0.0).min(1.0);
                last_gain = g * vel;
            }
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

        // Lifetime by the VCA's routes (spec § 4), once per block: a voice
        // no routed source holds ends, through the fade if it still sounds.
        if self.active && self.fade == 0 && self.vca.bits != 0 && !self.vca_holds() {
            if last_gain != 0.0 {
                self.after_fade = AfterFade::Idle;
                self.fade = Self::FADE;
            } else {
                // Silent, but the engine may still sound: back to fresh.
                self.reset();
            }
        }
    }
}

/// ENV destination `d`'s sum under `values`, into its slot's `n`.
fn env_mod(n: &mut EnvMods, mod_state: &ModState, d: usize, values: &[f32; MAX_MOD_SOURCES]) {
    let sum = mod_state.sum_for(d, values);
    match mod_state.dest(d).param {
        EnvParams::LEVEL if mod_state.present(d) != 0 => {
            #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
            let peak = sum.max(0.0).min(1.0);
            n.level = Some(peak);
        }
        EnvParams::TIME => n.time = sum,
        EnvParams::RISE => n.slides.rise = sum,
        EnvParams::FALL => n.slides.fall = sum,
        EnvParams::SHAPE => n.slides.shape = sum,
        _ => {}
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
