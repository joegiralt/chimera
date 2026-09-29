//! The playable instrument (instrument-core spec § Audio path, § Threading):
//! what the audio thread reads from the UI, and the voice pool that renders
//! every Part into the three DAC pairs.

use core::cmp::Reverse;
use core::mem::{MaybeUninit, size_of};
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::Stereo;
use crate::dsp::engines::SlotKind;
use crate::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use crate::dsp::modal::{ResonatorMode, SYM_NOTE_ON_CLEAR_MAX, SymPool};
use crate::dsp::voice::Voice;
use crate::hw::{
    AXI_SRAM, Cost, DAC_PAIRS, FB_BYTES, MAX_PARTS, MAX_VOICES, STORE_RESERVE, SampleBudget,
    UI_RESERVE, VOICE_RAM_BUDGET,
};
use crate::in_place::{by_value, uninit_at};
use crate::modulation::ModState;
use crate::note_queue::{NoteEvent, NoteKind};
use crate::params::ParamSnapshot;
use crate::part::PartParams;
use crate::perf::load::AudioStats;
use crate::preset::{Part, Performance, SoundPool};
use crate::scope::{ScopeFrame, ScopeWriter};
use crate::sym_alloc::{Place, Restart};
use crate::triple::TripleBuffer;
use crate::voice_alloc::{Allocator, VoiceIdx};
use crate::{MidiChannel, MidiNote, Velocity};

/// Everything the port places in AXI SRAM (ADR 0014): framebuffer, UI,
/// Performance, SoundPool, the `AudioShared`, scope and `AudioStats` triple
/// buffers (the scope's writer besides), the FX bus, and the card's store.
pub const AXI_RESIDENT: usize = FB_BYTES
    + UI_RESERVE
    + STORE_RESERVE
    + size_of::<Performance>()
    + size_of::<SoundPool>()
    + size_of::<TripleBuffer<AudioShared>>()
    + size_of::<TripleBuffer<ScopeFrame>>()
    + size_of::<ScopeWriter>()
    + size_of::<TripleBuffer<AudioStats>>()
    + size_of::<FxBus>();
const _: () = assert!(AXI_RESIDENT <= AXI_SRAM);

/// One Part as the audio thread sees it.
#[derive(Clone, Debug)]
pub struct PartAudio {
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub mix: PartParams,
}

/// The Performance state the audio needs (ADR 0021: through a triple buffer).
#[derive(Clone, Debug)]
pub struct AudioShared {
    pub parts: [PartAudio; MAX_PARTS],
    pub fx: FxParams,
}

impl Default for AudioShared {
    fn default() -> Self {
        Self::from_performance(&Performance::new())
    }
}

impl PartAudio {
    fn of(p: &Part) -> Self {
        Self {
            params: p.sound.params.clone(),
            mod_state: p.sound.mod_state.clone(),
            mix: p.mix,
        }
    }
}

crate::in_place::field_list!(AudioShared => AudioShared { parts, fx });

impl AudioShared {
    pub fn from_performance(perf: &Performance) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, perf)) }
    }

    /// Build from `perf` in `slot`, one `PartAudio` at a time: the stack
    /// never holds the whole struct.
    pub fn init_in_place<'s>(slot: &'s mut MaybeUninit<Self>, perf: &Performance) -> &'s mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from a live `&mut MaybeUninit<Self>`, so it is
        // valid, aligned and unaliased. `field_list!` above fails to compile
        // if a field is added; each field is written once through a raw
        // pointer before `assume_init_mut`.
        unsafe {
            let parts = addr_of_mut!((*p).parts).cast::<PartAudio>();
            // The destination's length bounds the writes, not the source's.
            #[allow(clippy::needless_range_loop)]
            for i in 0..MAX_PARTS {
                parts.add(i).write(PartAudio::of(&perf.parts[i]));
            }
            addr_of_mut!((*p).fx).write(perf.fx);
            slot.assume_init_mut()
        }
    }

    /// Overwrite with `perf` (the UI's per-frame publish), one Part at a
    /// time in place: the stack holds one `PartAudio`, not the whole
    /// struct. The destructuring lists every field, so a new one fails to
    /// compile here.
    pub fn update_from(&mut self, perf: &Performance) {
        let Self { parts, fx } = self;
        for (d, p) in parts.iter_mut().zip(&perf.parts) {
            *d = PartAudio::of(p);
        }
        *fx = perf.fx;
    }
}

/// One block for each DAC pair, interleaved L, R.
pub type DacOut = [[f32; BLOCK_SIZE * 2]; DAC_PAIRS];

// ADR 0013/0014: the voice pool (and its small bookkeeping) lives in D2.
const _: () = assert!(size_of::<Instrument>() <= VOICE_RAM_BUDGET);

/// Constant-power pan: (left, right) gains for `pan` in -1..1. Centre is
/// -3 dB per side; hard left/right is unity on one side, exactly 0 on the
/// other. Out-of-range `pan` is clamped (NaN reads as centre).
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = if pan.is_nan() {
        0.0
    } else {
        pan.clamp(-1.0, 1.0)
    };
    let q = core::f32::consts::FRAC_PI_4;
    (libm::sinf((1.0 - pan) * q), libm::sinf((1.0 + pan) * q))
}

/// Each Part's last pan and its `pan_gains`. `pan_gains` is two libm
/// `sinf`, soft-float f64 on this FPU and most of what mixing cost, so the
/// audio thread pays it only when a pan moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct PanCache([Option<(u32, (f32, f32))>; MAX_PARTS]);

impl PanCache {
    fn gains(&mut self, p: usize, pan: f32) -> (f32, f32) {
        match self.0[p] {
            Some((bits, g)) if bits == pan.to_bits() => g,
            _ => {
                let g = pan_gains(pan);
                self.0[p] = Some((pan.to_bits(), g));
                g
            }
        }
    }
}

/// Samples per step of the send and the pair passes, sized so their
/// accumulators stay in the FPU's 32 registers once LLVM unrolls the Parts
/// by four (at 4 the send pass spills).
const SEND_STEP: usize = 2;
const PAIR_STEP: usize = 8;
const _: () = assert!(BLOCK_SIZE.is_multiple_of(SEND_STEP) && BLOCK_SIZE.is_multiple_of(PAIR_STEP));

/// Steps 2–4 of `render`: each written Part's bus, panned and levelled,
/// into its pair and, by its sends, into the FX sends; then the FX bus
/// once, its return on pair 1; then the master section (`FxBus::master`);
/// then the output stage (`FxBus::limit`, ADR 0050), which trims and
/// limits the block before this one.
/// Returns the scope block (the written buses summed). Separate so the
/// bench can time it without voices.
///
/// Each output sample is summed in registers from 0.0, Part by Part in
/// order (the return last), and stored once: the same additions in the
/// same order as adding each Part into zeroed buffers, a fraction of the
/// loads and stores.
#[allow(clippy::too_many_arguments)]
pub fn mix_parts(
    buses: &[[f32; BLOCK_SIZE]; MAX_PARTS],
    written: &[bool; MAX_PARTS],
    sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
    pans: &mut PanCache,
    fx: &mut FxBus,
    shared: &AudioShared,
    sample_rate: u32,
    out: &mut DacOut,
) -> [f32; BLOCK_SIZE] {
    // The written Parts in order, gains hoisted; by pair for the dry mix.
    let mut src = [(&buses[0], [0.0f32; FX_SENDS]); MAX_PARTS];
    let mut n = 0;
    let mut dry = [[(&buses[0], 0.0f32, 0.0f32); MAX_PARTS]; DAC_PAIRS];
    let mut dry_n = [0usize; DAC_PAIRS];
    for (p, part) in shared.parts.iter().enumerate() {
        // A part with no voices this block has a silent bus: nothing to add.
        if !written[p] {
            continue;
        }
        let bus = &buses[p];
        let (gl, gr) = pans.gains(p, part.mix.pan);
        let (gl, gr) = (gl * part.mix.level, gr * part.mix.level);
        src[n] = (bus, part.mix.sends);
        n += 1;
        let k = part.mix.output.index();
        dry[k][dry_n[k]] = (bus, gl, gr);
        dry_n[k] += 1;
    }

    let mut scope = [0.0f32; BLOCK_SIZE];
    let [s0, s1, s2] = sends;
    for c in 0..BLOCK_SIZE / SEND_STEP {
        let i = c * SEND_STEP;
        let mut a = [[0.0f32; SEND_STEP]; FX_SENDS + 1];
        for &(bus, amt) in &src[..n] {
            for k in 0..SEND_STEP {
                let b = bus[i + k];
                a[0][k] += b * amt[0];
                a[1][k] += b * amt[1];
                a[2][k] += b * amt[2];
                a[3][k] += b;
            }
        }
        s0[i..i + SEND_STEP].copy_from_slice(&a[0]);
        s1[i..i + SEND_STEP].copy_from_slice(&a[1]);
        s2[i..i + SEND_STEP].copy_from_slice(&a[2]);
        scope[i..i + SEND_STEP].copy_from_slice(&a[3]);
    }

    // The FX bus once; its return lands on pair 1, L and R.
    let mut ret = Stereo::SILENT;
    fx.process(sends, &shared.fx, sample_rate, &mut ret);

    for (k, pair) in out.iter_mut().enumerate() {
        let parts = &dry[k][..dry_n[k]];
        for c in 0..BLOCK_SIZE / PAIR_STEP {
            let i = c * PAIR_STEP;
            let mut a = [0.0f32; 2 * PAIR_STEP];
            for &(bus, gl, gr) in parts {
                for j in 0..PAIR_STEP {
                    a[2 * j] += bus[i + j] * gl;
                    a[2 * j + 1] += bus[i + j] * gr;
                }
            }
            if k == 0 {
                for j in 0..PAIR_STEP {
                    a[2 * j] += ret.l[i + j];
                    a[2 * j + 1] += ret.r[i + j];
                }
            }
            pair[2 * i..2 * (i + PAIR_STEP)].copy_from_slice(&a);
        }
    }
    // The master section, after every pair is summed; then the output stage.
    fx.master(out, &shared.fx, sample_rate);
    fx.limit(out, sample_rate);
    scope
}

/// The shared voice pool and the per-block mix. The FX bus is passed to
/// `render` rather than owned: on hardware the pool is placed in D2 and the
/// FX bus in AXI (ADR 0014).
///
/// Audio-thread contract:
/// - The Instrument is the note queue's only consumer: the audio thread pops
///   every `NoteEvent` and feeds it to `handle` (the queue is single-producer,
///   single-consumer; nothing else may pop it).
/// - The `&AudioShared` passed to `handle` and `render` is the reader's
///   current buffer.
pub struct Instrument {
    voices: [Voice; MAX_VOICES],
    /// Sympathetic's sets, lent to the voices playing it (ADR 0054).
    sym: SymPool,
    alloc: Allocator,
    /// The MIDI channel each voice's note-on arrived on, so its note-off
    /// releases it even if the Part's channel changed meanwhile.
    note_channel: [MidiChannel; MAX_VOICES],
    /// The Part whose Sound each voice renders: its slot's Part, except
    /// while another Part's sound fades out ahead of a waiting note.
    sounding: [u8; MAX_VOICES],
    /// A note waiting for its voice's fade to end (#33 M7).
    waiting: [Option<Velocity>; MAX_VOICES],
    /// Each Part's mono bus from the last `render`: the sum of its voices.
    buses: [[f32; BLOCK_SIZE]; MAX_PARTS],
    sends: [[f32; BLOCK_SIZE]; FX_SENDS],
    pans: PanCache,
    sample_rate: u32,
    /// Each Part's kind at the last `render`: a Part that has just become
    /// Sympathetic restarts its held notes through the pool.
    last_kind: [SlotKind; MAX_PARTS],
    /// This block's Sympathetic clear budget left, in bytes.
    clear_left: usize,
    /// A note waits on the clear budget: later Sympathetic notes queue
    /// behind it, until render step 0 serves the queue oldest first.
    clear_backlog: bool,
}

/// The string-line bytes Sympathetic note-ons may clear in one block (spec
/// § 4.8): one worst-case note-on, so any one fits a fresh block. A
/// note-on past it waits a block, as a stolen note does: a chord of
/// worst-case slots starts a note a block, a typical one all at once.
pub const SYM_CLEAR_BUDGET: usize = SYM_NOTE_ON_CLEAR_MAX;

crate::in_place::field_list!(Instrument => Instrument { voices, sym, alloc, note_channel, sounding, waiting, buses, sends, pans, sample_rate, last_kind, clear_left, clear_backlog });

const SYMPATHETIC: SlotKind = SlotKind::Modal(ResonatorMode::Sympathetic);

impl Instrument {
    pub fn new(sample_rate: u32, budget: SampleBudget) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate, budget)) }
    }

    pub fn init_in_place(
        slot: &mut MaybeUninit<Self>,
        sample_rate: u32,
        budget: SampleBudget,
    ) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the voices and the pool are
        // built in place and the rest (the largest, `buses`, is 1.5 KB)
        // written once by value before `assume_init_mut`.
        unsafe {
            let voices = addr_of_mut!((*p).voices).cast::<Voice>();
            for (v, id) in VoiceIdx::ALL.into_iter().enumerate() {
                Voice::init_in_place(uninit_at(voices.add(v)), sample_rate, id);
            }
            SymPool::init_in_place(uninit_at(addr_of_mut!((*p).sym)));
            addr_of_mut!((*p).alloc).write(Allocator::new(budget));
            addr_of_mut!((*p).note_channel).write([MidiChannel::clamped(0); MAX_VOICES]);
            addr_of_mut!((*p).sounding).write([0; MAX_VOICES]);
            addr_of_mut!((*p).waiting).write([None; MAX_VOICES]);
            addr_of_mut!((*p).buses).write([[0.0; BLOCK_SIZE]; MAX_PARTS]);
            addr_of_mut!((*p).sends).write([[0.0; BLOCK_SIZE]; FX_SENDS]);
            addr_of_mut!((*p).pans).write(PanCache::default());
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            // The default Sound's (`Performance::new`: Algo).
            addr_of_mut!((*p).last_kind).write([SlotKind::Algo; MAX_PARTS]);
            addr_of_mut!((*p).clear_left).write(SYM_CLEAR_BUDGET);
            addr_of_mut!((*p).clear_backlog).write(false);
            slot.assume_init_mut()
        }
    }

    pub fn allocator(&self) -> &Allocator {
        &self.alloc
    }

    /// Each voice's slot rebuilds (`Voice::rebuilds`): for the tests and
    /// the bench.
    pub fn rebuilds(&self) -> [u16; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].rebuilds())
    }

    /// Voices whose engine sounds: for the bench, whose per-voice figures
    /// count only these (Sympathetic sounds at most `SYM_SLOTS`).
    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// The sympathetic pool's allocator: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn sym(&self) -> &crate::sym_alloc::SymAlloc {
        self.sym.alloc()
    }

    /// The kind each voice's slot holds: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slot_kinds(&self) -> [SlotKind; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].kind())
    }

    /// Whether each voice sounds: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn active(&self) -> [bool; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].is_active())
    }

    /// Part `part`'s mono bus from the last `render` (before pan and level).
    pub fn part_bus(&self, part: usize) -> &[f32; BLOCK_SIZE] {
        &self.buses[part]
    }

    /// Note-on: play on every Part listening on the channel. Note-off:
    /// release the voices this channel started.
    pub fn handle(&mut self, ev: NoteEvent, shared: &AudioShared) {
        match ev.kind {
            NoteKind::On(vel) => {
                // Judge the note against this block's patches, not the last.
                self.recost(shared);
                for (p, part) in shared.parts.iter().enumerate() {
                    if part.mix.channel != ev.channel {
                        continue;
                    }
                    let (q, mode) = (p as u8, part.mix.mode);
                    let cost = Voice::cost(&part.params, &part.mod_state);
                    let pick = self.alloc.pick(q, mode, cost, FxBus::COST);
                    // Sympathetic: the pool places it (the Rings rule, ADR
                    // 0054); a steal plays on the oldest slot's own voice.
                    let (v, wait) = if SlotKind::of(&part.params) == SYMPATHETIC {
                        let pick = pick.map(|v| VoiceIdx::ALL[v]);
                        let age = self.alloc.next_age();
                        let (v, steal, drops) = match self.sym.alloc_mut().place(pick, age) {
                            Place::On { voice, drops } => (Some(voice.index()), false, drops),
                            Place::Steal { voice, drops } => (Some(voice.index()), true, drops),
                            Place::Refused => (None, false, None),
                        };
                        // A restart waiting on that slot loses it to this note.
                        self.drop_waiting(drops);
                        // A voice whose slot is still another's, fading,
                        // waits for it as a steal does.
                        let awaits = v.is_some_and(|v| self.sym.alloc().awaits(VoiceIdx::ALL[v]));
                        (v, steal || awaits)
                    } else {
                        // A promise or claim of the voice's goes when it
                        // triggers this note, or rests (`Voice::trigger`,
                        // `Voice::rest`).
                        (pick, false)
                    };
                    let Some(v) = v else {
                        self.alloc.refuse();
                        continue;
                    };
                    // A Mono Part keeps one voice: the one it leaves fades.
                    if let Some(m) = self.alloc.book(v, q, mode, ev.note, cost) {
                        self.shed(m);
                    }
                    let waits =
                        wait || (self.sounding[v] as usize != p && self.voices[v].is_active());
                    // Sympathetic starts now only if the block's clear
                    // budget takes it and no note waits on it before; else
                    // it waits, as a steal does (spec § 4.8).
                    let deferred = !waits
                        && SlotKind::of(&part.params) == SYMPATHETIC
                        && !(self.voices[v].starts_now(&part.params)
                            && self.admit(v, &part.params));
                    let voice = &mut self.voices[v];
                    let waited = self.waiting[v].take().is_some();
                    let queued = if waits || deferred {
                        // Another note's sound fades out on its own bus and
                        // settings first: another Part's, or the oldest
                        // Sympathetic slot's, even this Part's own.
                        self.waiting[v] = Some(vel);
                        voice.kill()
                    } else {
                        self.sounding[v] = q;
                        voice.note_on(ev.note, vel, &part.params, &mut self.sym)
                    };
                    // A note replaced before it sounded (ADR 0027).
                    if waited || queued {
                        self.alloc.dropped_unheard();
                    }
                    self.note_channel[v] = ev.channel;
                }
            }
            NoteKind::Off => {
                // By voice index: every held voice this channel started on
                // this note (one per layered Part), and no other.
                for v in 0..MAX_VOICES {
                    let s = self.alloc.slots()[v];
                    if s.held() && s.note() == Some(ev.note) && self.note_channel[v] == ev.channel {
                        self.alloc.release(v);
                        self.voices[v].note_off(&mut self.sym);
                    }
                }
            }
        }
    }

    /// `w`'s waiting note, if any, lost its slot to a newer note: dropped
    /// unheard.
    fn drop_waiting(&mut self, w: Option<VoiceIdx>) {
        if let Some(w) = w
            && self.waiting[w.index()].take().is_some()
        {
            self.alloc.dropped_unheard();
        }
    }

    /// Starts idle voice `v`'s waiting note, velocity `vel` on Part `q`,
    /// released at once unless `held`, if the block's clear budget lets it:
    /// false if it must wait on. A Sympathetic note with no slot promised
    /// (its Part became Sympathetic while it waited on another Part's
    /// fade) is placed first, at its `age`, as `handle` places one: a
    /// steal moves it to the oldest slot's voice, to wait for that fade.
    fn start(
        &mut self,
        v: usize,
        (vel, q, note, held, age): (Velocity, u8, MidiNote, bool, u32),
        shared: &AudioShared,
    ) -> bool {
        let part = &shared.parts[q as usize % MAX_PARTS];
        let params = &part.params;
        let id = VoiceIdx::ALL[v];
        if SlotKind::of(params) == SYMPATHETIC && !self.sym.alloc().promised(id) {
            match self.sym.alloc_mut().place(Some(id), age) {
                Place::On { drops, .. } => self.drop_waiting(drops),
                Place::Steal { voice: u, drops } => {
                    self.drop_waiting(drops);
                    let (u, mode) = (u.index(), part.mix.mode);
                    let cost = Voice::cost(params, &part.mod_state);
                    self.alloc.release(v);
                    self.alloc.release_finished(v);
                    self.voices[v].rest(&mut self.sym);
                    if let Some(m) = self.alloc.book(u, q, mode, note, cost) {
                        self.shed(m);
                    }
                    if !held {
                        self.alloc.release(u);
                    }
                    let waited = self.waiting[u].replace(vel).is_some();
                    if waited | self.voices[u].kill() {
                        self.alloc.dropped_unheard();
                    }
                    self.note_channel[u] = self.note_channel[v];
                    return true;
                }
                Place::Refused => unreachable!("placed with a voice"),
            }
        }
        if !self.admit(v, params) {
            return false;
        }
        let voice = &mut self.voices[v];
        let _ = voice.note_on(note, vel, params, &mut self.sym);
        if !held {
            voice.note_off(&mut self.sym);
        }
        self.sounding[v] = q;
        true
    }

    /// Whether voice `v` may start a note on `params` this block: anything
    /// but Sympathetic may; Sympathetic if no note waits on the clear
    /// budget before it and its clear fits what the block has left, which
    /// it then takes. One that doesn't fit starts the queue.
    fn admit(&mut self, v: usize, params: &ParamSnapshot) -> bool {
        if SlotKind::of(params) != SYMPATHETIC {
            return true;
        }
        if self.clear_backlog {
            return false;
        }
        let bytes = self.voices[v].sym_note_on_clear(&self.sym);
        debug_assert!(bytes <= SYM_CLEAR_BUDGET);
        if bytes > self.clear_left {
            self.clear_backlog = true;
            return false;
        }
        self.clear_left -= bytes;
        true
    }

    /// A patch edit changes its voices' cost; fade voices out if that went
    /// over the budget (ADR 0027). The slot stays taken until the fade ends
    /// and `render` step 5 frees it.
    fn recost(&mut self, shared: &AudioShared) {
        let costs: [Cost; MAX_PARTS] = core::array::from_fn(|p| {
            Voice::cost(&shared.parts[p].params, &shared.parts[p].mod_state)
        });
        for v in 0..MAX_VOICES {
            if let Some(p) = self.alloc.slots()[v].part() {
                let p = p as usize % MAX_PARTS;
                let held = self.voices[v].held_model_extra(&shared.parts[p].params);
                self.alloc.recost(v, costs[p] + held);
            }
        }
        while let Some(v) = self.alloc.shed(FxBus::COST) {
            self.shed(v);
        }
    }

    /// Voice `v`, marked dying, fades out: a note waiting for it is dropped
    /// unheard (ADR 0027). Any slot promised or bound to it goes when it
    /// rests, freed.
    fn shed(&mut self, v: usize) {
        let queued = self.voices[v].kill();
        if self.waiting[v].take().is_some() || queued {
            self.alloc.dropped_unheard();
        }
    }

    /// A Part whose Sound has just become Sympathetic: its held, sounding
    /// notes, newest first, claim the pool's slots, so the last four
    /// played restart and the rest fade and stay silent until played
    /// again (the Rings rule, spec § 4.5). Each restart waits as a stolen
    /// note does.
    fn restart_switched(&mut self, shared: &AudioShared) {
        for (p, part) in shared.parts.iter().enumerate() {
            let kind = SlotKind::of(&part.params);
            let became = kind == SYMPATHETIC && self.last_kind[p] != SYMPATHETIC;
            self.last_kind[p] = kind;
            if !became {
                continue;
            }
            // A note already on a slot (a note-on since the switch) holds
            // it; a shed note stays shed.
            let mut held = [(VoiceIdx::ALL[0], 0u32); MAX_VOICES];
            let mut n = 0;
            for (v, s) in self.alloc.slots().iter().enumerate() {
                let voice = &self.voices[v];
                if s.held()
                    && !s.dying()
                    && s.part() == Some(p as u8)
                    && self.sounding[v] as usize == p
                    && voice.is_active()
                    && voice.kind() != SYMPATHETIC
                {
                    held[n] = (VoiceIdx::ALL[v], s.age());
                    n += 1;
                }
            }
            let held = &mut held[..n];
            held.sort_unstable_by_key(|&(_, age)| Reverse(age));
            for &(id, age) in held.iter() {
                let v = id.index();
                let dropped = match self.sym.alloc_mut().restart(id, age) {
                    Restart::Claimed { evict, drops } => {
                        // Its note waits for the fade, as a stolen one does.
                        let vel = self.voices[v].velocity();
                        self.waiting[v].get_or_insert(vel);
                        let _ = self.voices[v].kill();
                        let evicted = evict.is_some_and(|u| {
                            let u = u.index();
                            self.waiting[u].take().is_some() | self.voices[u].kill()
                        });
                        // Its holder already fades: only the waiter loses.
                        let lost = drops.is_some_and(|w| self.waiting[w.index()].take().is_some());
                        evicted || lost
                    }
                    Restart::Silent => {
                        // `restart` is Silent only for a voice no slot names.
                        let pool = self.sym.alloc();
                        debug_assert!(!pool.awaits(id) && !pool.promised(id));
                        self.waiting[v].take().is_some() | self.voices[v].kill()
                    }
                };
                if dropped {
                    self.alloc.dropped_unheard();
                }
            }
        }
    }

    /// Render one block into the three DAC pairs.
    pub fn render(
        &mut self,
        fx: &mut FxBus,
        out: &mut DacOut,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
    ) {
        self.recost(shared);
        self.restart_switched(shared);

        // 1. Voices into their part's mono bus. The first voice of a part is
        //    copied, not added, so a lone voice reaches the bus bit-for-bit.
        let mut written = [false; MAX_PARTS];
        for bus in self.buses.iter_mut() {
            bus.fill(0.0);
        }
        let mut block = [0.0f32; BLOCK_SIZE];
        // 0. Notes that waited on the clear budget, their voices idle, start
        //    oldest first before the voices render: let in this block, one
        //    sounds in it. The first that doesn't fit queues the rest again.
        let mut queue = [(u32::MAX, 0usize); MAX_VOICES];
        let mut n = 0;
        for v in 0..MAX_VOICES {
            let s = self.alloc.slots()[v];
            let idle = !self.voices[v].is_active();
            if idle && self.waiting[v].is_some() && !self.sym.alloc().awaits(VoiceIdx::ALL[v]) {
                queue[n] = (s.age(), v);
                n += 1;
            }
        }
        queue[..n].sort_unstable();
        self.clear_backlog = false;
        for &(_, v) in &queue[..n] {
            let s = self.alloc.slots()[v];
            if let (Some(vel), Some(q), Some(note)) = (self.waiting[v], s.part(), s.note())
                && self.start(v, (vel, q, note, s.held(), s.age()), shared)
            {
                self.waiting[v] = None;
            }
        }
        for v in 0..MAX_VOICES {
            if self.alloc.slots()[v].is_free() {
                continue;
            }
            let p = self.sounding[v] as usize % MAX_PARTS;
            let part = &shared.parts[p];
            self.voices[v].render(&mut block, &part.params, &part.mod_state, &mut self.sym);
            if written[p] {
                for (b, &s) in self.buses[p].iter_mut().zip(&block) {
                    *b += s;
                }
            } else {
                self.buses[p] = block;
                written[p] = true;
            }
            // 5. A released voice whose engine went quiet, or a shed one whose
            //    fade ended, is free again.
            //    Read right after rendering the note the voice plays *now*,
            //    so a steal or retrigger since the last block is never freed
            //    by a report about the note it replaced.
            if !self.voices[v].is_active() {
                let s = self.alloc.slots()[v];
                match (self.waiting[v].take(), s.part(), s.note()) {
                    // A note bound for a slot whose holder still fades: no
                    // rebuild spent waiting, it tries again after the next
                    // block. (A note of another kind waits at most until the
                    // claim's fade ends; its `trigger` cancels the claim.)
                    (Some(vel), Some(_), Some(_)) if self.sym.alloc().awaits(VoiceIdx::ALL[v]) => {
                        self.waiting[v] = Some(vel);
                    }
                    (Some(vel), Some(q), Some(note)) => {
                        // Past the block's clear budget: the next block.
                        if !self.start(v, (vel, q, note, s.held(), s.age()), shared) {
                            self.waiting[v] = Some(vel);
                        }
                    }
                    _ => {
                        self.alloc.release_finished(v);
                        self.voices[v].rest(&mut self.sym);
                    }
                }
            }
        }

        self.clear_left = SYM_CLEAR_BUDGET;

        // 2-4.
        let scope_block = mix_parts(
            &self.buses,
            &written,
            &mut self.sends,
            &mut self.pans,
            fx,
            shared,
            self.sample_rate,
            out,
        );
        // The MST page's GR meter: one relaxed store, never blocks.
        crate::meter::MASTER_GR.publish(fx.master_gr_db());

        // Oscilloscope: every part's bus, before pan and level.
        scope.write(&scope_block);
    }
}
