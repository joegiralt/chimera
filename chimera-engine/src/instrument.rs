//! The playable instrument (instrument-core spec § Audio path, § Threading):
//! what the audio thread reads from the UI, and the voice pool that renders
//! every Part into the three DAC pairs.

use crate::dsp::algo::engine::{FormantView, FormantViews};
use crate::dsp::clampf;
use core::cmp::Reverse;
use core::mem::{MaybeUninit, size_of};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use chimera_hal::BLOCK_SIZE;
use chimera_hal::card_bus::CardFault;
use chimera_hal::units::StreamCount;

use crate::dsp::Stereo;
use crate::dsp::ease::{Ease, ease_coeff, step_of};
use crate::dsp::engines::{EngineCtx, SlotKind};
use crate::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use crate::dsp::knobs::{Gliding, KnobDelta, KnobPublisher, Knobs, Pace};
use crate::dsp::lfo::LfoClock;
use crate::dsp::modal::{ResonatorMode, SYM_NOTE_ON_CLEAR_MAX, SymPool};
use crate::dsp::rev::RevBill;
use crate::dsp::rev::sync::Tempo;
use crate::dsp::tape_engine::{COST_STREAM, Heads, TapeEngine};
use crate::dsp::voice::{HighNote, Voice};
use crate::hw::{Cost, DAC_PAIRS, MAX_PARTS, MAX_VOICES, SampleBudget, VOICE_RAM_BUDGET};
use crate::in_place::{by_value, uninit_at};
use crate::modulation::ModState;
use crate::note_queue::{
    BendEvent, ChannelMode, ChannelModeEvent, Expr, ExprEvent, NoteEvent, NoteKind, SustainEvent,
};
use crate::orbit::RingIdx;
use crate::orbit::ring::Harmony;
use crate::orbit::runner::{OrbitRunner, OrbitShared};
use crate::params::{EngineType, ParamSnapshot};
use crate::part::{Held, LiveSnap, PartParams};
use crate::preset::{Part, Performance, Sound};
use crate::project::{Loads, PartId, Project, SourceId};
use crate::scope::ScopeWriter;
use crate::source_table::{AudioToken, GateView};
use crate::stream::admit::{Age, Bill, ReadRatio};
use crate::stream::port::{CtxBase, NoPorts, Port, VoicePorts};
use crate::stream::stats::StreamStats;
use crate::tracked::Tracked;
use crate::voice_alloc::{Allocator, VoiceIdx};
use crate::{MidiChannel, MidiNote, Velocity};

mod admission;
pub use admission::AdmitDoor;
use chimera_theory::{Note, snap};

/// Each Part's newest TAPE note's heads, for the strip (spec § 3): stored
/// once a block by the audio, read by the UI. Two words a Part, as the
/// chip has no 64-bit atomics: a read may pair one block's head with the
/// next's second, a frame the strip never holds. Held inverted, so
/// `Heads::NONE` is all zeros: no flash for its initial value.
pub struct PartHeads([[AtomicU32; 2]; MAX_PARTS]);

impl PartHeads {
    pub const fn new() -> Self {
        Self([const { [const { AtomicU32::new(0) }; 2] }; MAX_PARTS])
    }

    pub fn publish(&self, part: usize, h: Heads) {
        let w = &self.0[part];
        w[0].store(!(h.raw() as u32), Relaxed);
        w[1].store(!((h.raw() >> 32) as u32), Relaxed);
    }

    pub fn read(&self, part: usize) -> Heads {
        let w = &self.0[part];
        Heads::from_raw(!(u64::from(w[0].load(Relaxed)) | u64::from(w[1].load(Relaxed)) << 32))
    }
}

impl Default for PartHeads {
    fn default() -> Self {
        Self::new()
    }
}

/// The heads the strip draws. Global: at most one test per test binary
/// may play TAPE through an `Instrument`.
pub static HEADS: PartHeads = PartHeads::new();

/// The LFO page's playhead (ADR 0177): the UI names the Part it shows
/// (`watch`), and once a block the audio stores that Part's newest
/// voice's phase in each LFO slot, nothing while no LFO page is up. A word
/// a slot: the Part, a live bit and the phase in 24 bits.
pub struct LfoView {
    watch: AtomicU32,
    at: [AtomicU32; 3],
}

/// One slot's playhead as published.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfoAt {
    pub part: usize,
    /// The newest voice's phase, 0..1; `None` with no voice.
    pub phase: Option<f32>,
}

impl LfoView {
    const LIVE: u32 = 1 << 31;
    const TURN: f32 = (1 << 24) as f32;

    pub const fn new() -> Self {
        Self {
            watch: AtomicU32::new(0),
            at: [const { AtomicU32::new(0) }; 3],
        }
    }

    /// The UI, each frame: the Part whose LFO page is up, or none. Stored
    /// only when it changes; leaving clears the last playheads, so the
    /// next page up starts with none.
    pub fn watch(&self, part: Option<usize>) {
        let w = part.map_or(0, |p| p as u32 + 1);
        if self.watch.load(Relaxed) == w {
            return;
        }
        self.watch.store(w, Relaxed);
        if part.is_none() {
            self.at.iter().for_each(|w| w.store(0, Relaxed));
        }
    }

    /// The audio: the Part to publish, if any.
    pub fn watched(&self) -> Option<usize> {
        (self.watch.load(Relaxed) as usize).checked_sub(1)
    }

    /// Every slot's, for `part`: its newest voice's phases, or none.
    pub fn publish_all(&self, part: usize, phases: Option<[f32; 3]>) {
        for slot in 0..3 {
            let phase = phases.map(|p| p[slot]);
            self.publish(slot, LfoAt { part, phase });
        }
    }

    pub fn publish(&self, slot: usize, at: LfoAt) {
        let phase = at.phase.map_or(0, |p| {
            Self::LIVE | (crate::dsp::clampf(p, 0.0, 1.0) * Self::TURN) as u32 & 0xff_ffff
        });
        self.at[slot].store(phase | (at.part as u32) << 24, Relaxed);
    }

    pub fn read(&self, slot: usize) -> LfoAt {
        let w = self.at[slot].load(Relaxed);
        LfoAt {
            part: (w >> 24 & 0x7f) as usize,
            phase: (w & Self::LIVE != 0).then(|| (w & 0xff_ffff) as f32 / Self::TURN),
        }
    }
}

impl Default for LfoView {
    fn default() -> Self {
        Self::new()
    }
}

/// The playheads the LFO page draws.
pub static LFO_VIEW: LfoView = LfoView::new();

/// One Part as the audio thread sees it.
#[derive(Clone, Debug)]
pub struct PartAudio {
    /// The Sound, stamped as published: a write past the publisher (a
    /// test's, the bench's) moves the stamp, and the knobs read it whole.
    pub params: Tracked<ParamSnapshot>,
    pub mod_state: Tracked<ModState>,
    /// What changed in its knobs (ADR 0115): all the audio reads of them.
    pub knobs: KnobDelta,
    pub mix: PartParams,
    /// The Part's Sound loads: a change lands its knobs.
    pub loads: Loads,
    /// The pool's id for its Sound's source (spec § 4): what a TAPE
    /// note-on latches. `None` with no source, or none in the pool.
    pub source: Option<SourceId>,
}

/// The Performance state the audio needs (ADR 0021: through a triple buffer).
#[derive(Clone, Debug)]
pub struct AudioShared {
    pub parts: [PartAudio; MAX_PARTS],
    pub fx: FxParams,
    /// The load epoch this snapshot was published under (ADR 0046).
    pub epoch: u32,
    pub orbit: OrbitShared,
    /// SETTINGS > TUNING, cents (`MidiSettings::tune`), from SYSTEM.
    pub tune: i8,
}

impl Default for AudioShared {
    fn default() -> Self {
        Self::from_performance(&Performance::new())
    }
}

impl PartAudio {
    /// `p` as the audio reads it but `loads`, which `fill` sets (in here,
    /// or in `init_in_place`, it costs ~0.8 KB of flash). Left at `NONE`,
    /// the first publish moves it: one landing, before any note.
    /// `p` as published, with `knobs`, its delta: stamped as its Sound.
    fn of(p: &Part, knobs: KnobDelta) -> Self {
        let stamp = p.sound.stamp();
        Self {
            params: Tracked::new(p.sound.params.clone(), stamp),
            mod_state: Tracked::new(p.sound.mod_state.clone(), stamp),
            knobs,
            mix: p.mix,
            loads: Loads::NONE,
            source: None,
        }
    }

    /// Cycles/sample of a voice of this Part: its Sound's, and where a
    /// re-snap can move it (SNAP on, HELD RE-SNAP) the retune its glide
    /// runs each block.
    fn voice_cost(&self) -> Cost {
        let glide = match (self.mix.snap, self.mix.held) {
            (LiveSnap::Off, _) | (_, Held::Stay) => Cost::ZERO,
            (_, Held::Resnap) => Voice::glide_cost(&self.params, &self.mod_state),
        };
        Voice::cost(&self.params, &self.mod_state) + glide
    }

    /// `key` as this Part plays it live under `h` (spec § 5.2): snapped to
    /// its SNAP while ORBIT runs, else as played.
    fn live_note(&self, key: MidiNote, h: Option<Harmony>) -> MidiNote {
        let (Some(h), Some(to)) = (h, self.mix.snap.target()) else {
            return key;
        };
        Note::new(key.get())
            .and_then(|n| MidiNote::new(snap(n, h.target(to)).get()))
            .unwrap_or(key)
    }
}

/// A held key on a HELD RE-SNAP Part glides to its new note over this
/// (95 % of the way, as GLIDE TIME is, ADR 0065): quick enough to follow
/// the chord, slow enough not to click (spec § 5.2).
pub const RESNAP_GLIDE_MS: u32 = 40;

crate::in_place::field_list!(AudioShared => AudioShared { parts, fx, epoch, orbit, tune });

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
                parts
                    .add(i)
                    .write(PartAudio::of(&perf.parts()[i], KnobDelta::default()));
            }
            addr_of_mut!((*p).fx).write(perf.fx);
            addr_of_mut!((*p).epoch).write(0);
            addr_of_mut!((*p).orbit).write(OrbitShared::default());
            addr_of_mut!((*p).tune).write(0);
            slot.assume_init_mut()
        }
    }

    /// Overwrite with `perf` (the UI's per-frame publish), tagged with
    /// the load `epoch`, one Part at a time in place: the stack holds one
    /// `PartAudio`, not the whole struct. The destructuring lists every
    /// field, so a new one fails to compile here. Every Part's source is
    /// `None`, which mutes TAPE: the tests' publish, never a shell's. No
    /// knob deltas: the knobs read a Sound whole when its stamp moves.
    #[cfg(any(test, feature = "test-support"))]
    pub fn update_from(&mut self, perf: &Performance, epoch: u32) {
        self.fill(perf, |_| None, epoch, |_, _| KnobDelta::default());
    }

    /// The shells' publish: `project`'s Performance as `update_from`, each
    /// Part's source the id its pool holds for its Sound's ref (pass 2's,
    /// `source_store`'s or `source_absent`'s), a lookup, never a load; and
    /// each Part's knob delta from `knobs`, the UI's side (ADR 0115).
    pub fn update_from_project(
        &mut self,
        project: &Project,
        epoch: u32,
        knobs: &mut KnobPublisher,
    ) {
        let pool = project.pool();
        self.fill(
            project.perf(),
            |p| pool.find_source(p.sound.source?.r),
            epoch,
            |i, p| knobs.publish(i, &p.sound, p.loads()),
        );
    }

    /// Part `i` given `sound`, `loads` its Sound loads, as a publish gives
    /// it (ADR 0115): its values stamped as `sound`, its knob delta from
    /// `knobs`. The bench's one-Part publish, which `fill` makes for each.
    pub fn publish_sound(
        &mut self,
        i: usize,
        sound: &Tracked<Sound>,
        loads: Loads,
        knobs: &mut KnobPublisher,
    ) {
        let d = &mut self.parts[i];
        let stamp = sound.stamp();
        d.params = Tracked::new(sound.params.clone(), stamp);
        d.mod_state = Tracked::new(sound.mod_state.clone(), stamp);
        (d.knobs, d.loads) = (knobs.publish(i, sound, loads), loads);
    }

    /// Every Part's values, each stamped as its Sound (equal stamps, equal
    /// values), with its knob delta.
    fn fill(
        &mut self,
        perf: &Performance,
        source: impl Fn(&Part) -> Option<SourceId>,
        epoch: u32,
        mut knobs: impl FnMut(usize, &Part) -> KnobDelta,
    ) {
        let Self {
            parts,
            fx,
            epoch: e,
            // `UiState::publish_orbit` writes it, in the same publish.
            orbit: _,
            // Likewise: the shells write it, from SYSTEM.
            tune: _,
        } = self;
        for (i, (d, p)) in parts.iter_mut().zip(perf.parts()).enumerate() {
            *d = PartAudio::of(p, knobs(i, p));
            d.source = source(p);
            d.loads = p.loads();
        }
        *fx = perf.fx;
        *e = epoch;
    }
}

/// Part index `q`, as a voice's `sounding` holds it.
fn part_id(q: u8) -> PartId {
    PartId::ALL[q as usize % MAX_PARTS]
}

/// A new Sound in `part` (its `loads` moved): its knobs read whole and
/// land. Out of line: once a Part, six copies otherwise.
#[inline(never)]
fn new_sound((knobs, seen): &mut (Knobs, Loads), part: &PartAudio) {
    if *seen != part.loads {
        *seen = part.loads;
        knobs.land();
    }
}

/// One block for each DAC pair, interleaved L, R.
pub type DacOut = [[f32; BLOCK_SIZE * 2]; DAC_PAIRS];
pub use crate::dsp::limiter::DacBlocks;

/// No pool behind a call: every TAPE note-on is refused. The tests'
/// `handle` and `render` play through it; the shells through the block's
/// view (`run_block`).
const NO_SOURCES: GateView<'static> = GateView::empty();

/// A note waiting for its voice's fade to end (#33 M7), and the source it
/// latched at its note-on (TAPE).
#[derive(Clone, Copy, Debug)]
struct Waiting {
    vel: Velocity,
    source: Option<SourceId>,
}

/// What admission paid for a streamed note (slice 1 § 5.5): its bill,
/// and the read ratio that covers, its cap (`StreamPort::ratio_max`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Paid {
    bill: Bill,
    ratio: ReadRatio,
}

/// A voice's card stream, as admission sees it.
#[derive(Clone, Copy, Debug)]
struct Streaming {
    /// Its note's; `None` off the card, or not admitted yet (its source
    /// moving at note-on). What the card is booked for, not whether the
    /// voice streams: that is its deck's (`TapeDeck::streams`).
    paid: Option<Paid>,
    /// The note it took fades out first on the same ring: billed until
    /// the fade ends.
    fading: Option<Bill>,
    age: Age,
    /// Its reach rose past what the card admits: it holds at its cap,
    /// counted once (`StreamStats::held_at_cap`).
    capped: bool,
    /// Its loop's bill over its TAPE model's, at note-on (ADR 0211): held
    /// in every block's re-bill.
    looped: Cost,
}

impl Streaming {
    const IDLE: Streaming = Streaming {
        paid: None,
        fading: None,
        age: Age::FRESH,
        capped: false,
        looped: Cost::ZERO,
    };

    /// The ring serves one stream at a time: counted once, fading or not.
    fn billed(&self) -> Option<Bill> {
        self.paid.map(|p| p.bill).max(self.fading)
    }
}

// ADR 0013/0014: the voice pool (and its small bookkeeping) lives in D2.
const _: () = assert!(size_of::<Instrument>() <= VOICE_RAM_BUDGET);

/// Constant-power pan: (left, right) gains for `pan` in -1..1. Centre is
/// -3 dB per side; hard left/right is unity on one side, exactly 0 on the
/// other. Out-of-range `pan` is clamped (NaN reads as centre).
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = if pan.is_nan() {
        0.0
    } else {
        clampf(pan, -1.0, 1.0)
    };
    let q = core::f32::consts::FRAC_PI_4;
    (libm::sinf((1.0 - pan) * q), libm::sinf((1.0 + pan) * q))
}

/// Each Part's last pan and its `pan_gains`, and its mix gains eased
/// (never snap). `pan_gains` is two libm `sinf`, soft-float f64 on this FPU
/// and most of what mixing cost, so the audio thread pays it only when a
/// pan moves.
#[derive(Clone, Copy, Debug, Default)]
pub struct MixState {
    pans: [Option<(u32, (f32, f32))>; MAX_PARTS],
    /// L and R (pan × LEVEL) and the three sends, per Part.
    gains: [[Ease; MIX_GAINS]; MAX_PARTS],
}

/// A Part's eased gains: L, R and the three sends.
const MIX_GAINS: usize = 2 + FX_SENDS;

impl MixState {
    fn pan_gains(&mut self, p: usize, pan: f32) -> (f32, f32) {
        match self.pans[p] {
            Some((bits, g)) if bits == pan.to_bits() => g,
            _ => {
                let g = pan_gains(pan);
                self.pans[p] = Some((pan.to_bits(), g));
                g
            }
        }
    }
}

/// Adds a moving gain's ramp past its start to side `SIDE` of `out`
/// (`S` sides, interleaved): `bus · step·(i + 1)`. The mix passes play
/// each gain at its block's start; only the gains that move pay for this.
#[inline(always)]
fn glide<const S: usize, const SIDE: usize>(bus: &[f32; BLOCK_SIZE], step: f32, out: &mut [f32]) {
    // `n` counts in whole floats, exactly `(i + 1) as f32`.
    let mut n = 0.0f32;
    for (o, &b) in out.as_chunks_mut::<S>().0.iter_mut().zip(bus) {
        n += 1.0;
        o[SIDE] += b * (step * n);
    }
}

/// Samples per step of the send and the pair passes, sized so their
/// accumulators stay in the FPU's 32 registers once LLVM unrolls the Parts
/// by four (at 4 the send pass spills).
const SEND_STEP: usize = 2;
const PAIR_STEP: usize = 8;
const _: () = assert!(BLOCK_SIZE.is_multiple_of(SEND_STEP) && BLOCK_SIZE.is_multiple_of(PAIR_STEP));

/// Steps 2–4 of `render`: each written Part's bus, panned and levelled,
/// into its pair and, by its sends, into the FX sends, each gain eased
/// (`MixState`); then the FX bus
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
    mix: &mut MixState,
    fx: &mut FxBus,
    shared: &AudioShared,
    tempo: Tempo,
    sample_rate: u32,
    dac: &mut DacBlocks,
) -> [f32; BLOCK_SIZE] {
    // The written Parts in order, gains hoisted at their block's start; by
    // pair for the dry mix. The Parts whose gains move, with their pair and
    // steps.
    let mut src = [(&buses[0], [0.0f32; FX_SENDS]); MAX_PARTS];
    let mut n = 0;
    let mut dry = [[(&buses[0], 0.0f32, 0.0f32); MAX_PARTS]; DAC_PAIRS];
    let mut dry_n = [0usize; DAC_PAIRS];
    let mut moving = [(&buses[0], 0usize, [0.0f32; MIX_GAINS]); MAX_PARTS];
    let mut moving_n = 0;
    let k = ease_coeff(sample_rate);
    for (p, part) in shared.parts.iter().enumerate() {
        let (gl, gr) = mix.pan_gains(p, part.mix.pan);
        let [s0, s1, s2] = part.params.out.sends;
        let to = [gl * part.mix.level, gr * part.mix.level, s0, s1, s2];
        let eases = &mut mix.gains[p];
        // A part with no voices this block has a silent bus: nothing to
        // add, and its gains land unheard.
        if !written[p] {
            for (e, &t) in eases.iter_mut().zip(&to) {
                e.land(t);
            }
            continue;
        }
        let g: [(f32, f32); MIX_GAINS] = core::array::from_fn(|j| eases[j].step(to[j], k));
        let bus = &buses[p];
        src[n] = (bus, [g[2].0, g[3].0, g[4].0]);
        n += 1;
        let k = part.mix.output.index();
        dry[k][dry_n[k]] = (bus, g[0].0, g[1].0);
        dry_n[k] += 1;
        if g.iter().any(|g| g.0 != g.1) {
            moving[moving_n] = (bus, k, g.map(|g| step_of(g, BLOCK_SIZE)));
            moving_n += 1;
        }
    }
    let moving = &moving[..moving_n];

    let mut scope = [0.0f32; BLOCK_SIZE];
    // No Part written: every sum is its +0 start, as the scope's is (#793).
    if n == 0 && crate::dsp::may_skip() {
        *sends = [[0.0; BLOCK_SIZE]; FX_SENDS];
    } else {
        let [s0, s1, s2] = &mut *sends;
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
    }
    for &(bus, _, steps) in moving {
        for (send, &step) in sends.iter_mut().zip(&steps[2..]) {
            if step != 0.0 {
                glide::<1, 0>(bus, step, send);
            }
        }
    }

    // The FX bus once; its return lands on pair 1, L and R.
    let mut ret = Stereo::SILENT;
    fx.process(sends, &shared.fx, tempo, sample_rate, &mut ret);

    let (out, zero) = dac.mix_known();
    for (k, pair) in out.iter_mut().enumerate() {
        let parts = &dry[k][..dry_n[k]];
        // Nothing routed (pair 1 takes the FX return): +0s, already there
        // if this block's pair was +0 when it went out (#793).
        if k != 0 && parts.is_empty() {
            if zero[k] && crate::dsp::may_skip() {
                crate::dsp::skipped(crate::dsp::Skip::Pairs);
            } else {
                pair.fill(0.0);
                zero[k] = true;
            }
            continue;
        }
        zero[k] = false;
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
    for &(bus, k, [l, r, ..]) in moving {
        if l != 0.0 {
            glide::<2, 0>(bus, l, &mut out[k]);
        }
        if r != 0.0 {
            glide::<2, 1>(bus, r, &mut out[k]);
        }
    }
    // The master section, after every pair is summed; then the output stage.
    if fx.master(out, &shared.fx, sample_rate) {
        // Scaled: its +0 pairs no longer known.
        *zero = [false; DAC_PAIRS];
    }
    fx.limit(dac, &shared.fx, sample_rate);
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
    /// Who started each voice's note: a note-off releases only its own
    /// origin's voices, so ORBIT's rings and a played key never release
    /// one another.
    note_origin: [NoteOrigin; MAX_VOICES],
    /// The key that started each voice's note: what its note-off names.
    /// A snapped live note sounds another (the allocator's); a re-snap
    /// snaps this again, so repeated chord changes never drift.
    keys: [MidiNote; MAX_VOICES],
    /// The keys down on each voice, by MIDI note: two keys that snap to one
    /// pitch share a voice (a Poly re-strike), and it is let go when the
    /// last of them is up.
    held_keys: [NoteSet; MAX_VOICES],
    /// ORBIT's key and chord now; `None` while it is stopped (spec § 5.2).
    harmony: Option<Harmony>,
    /// ORBIT's clock: what the reverb's synced times follow.
    tempo: Tempo,
    /// A re-snap's note waiting on the clear budget (a SYMP voice's rings
    /// grow for its glide), served at the next block's start.
    resnap_to: [Option<MidiNote>; MAX_VOICES],
    /// The Part whose Sound each voice renders: its slot's Part, except
    /// while another Part's sound fades out ahead of a waiting note.
    sounding: [u8; MAX_VOICES],
    /// A note waiting for its voice's fade to end.
    waiting: [Option<Waiting>; MAX_VOICES],
    /// Each Part's mono bus from the last `render`: the sum of its voices.
    buses: [[f32; BLOCK_SIZE]; MAX_PARTS],
    sends: [[f32; BLOCK_SIZE]; FX_SENDS],
    mix: MixState,
    sample_rate: u32,
    /// Each Part's kind at the last `render`: a Part that has just become
    /// Sympathetic restarts its held notes through the pool.
    last_kind: [SlotKind; MAX_PARTS],
    /// This block's Sympathetic clear budget left, in bytes.
    clear_left: usize,
    /// A note waits on the clear budget: later Sympathetic notes queue
    /// behind it, until render step 0 serves the queue oldest first.
    clear_backlog: bool,
    /// Each Part's `voice_cost`, worked out once a block (`recost`).
    costs: [Cost; MAX_PARTS],
    /// The snapshot `costs` were worked out from this block, by address;
    /// 0 when not yet (each `render` ends the block).
    costed: usize,
    /// `costs` worked out since start: for the tests.
    costings: u32,
    /// Each MIDI channel's pitch-bend wheel, -8192..=8191.
    wheel: [i16; 16],
    /// Each MIDI channel's mod wheel and channel pressure, 0..=127: the
    /// WHEEL and PRESSURE sources' targets (`ease_bend` glides them).
    expr: [[u8; 2]; 16],
    /// Each voice's key's poly pressure, 0..=127: a struck key's only.
    poly: [u8; MAX_VOICES],
    /// The channels whose sustain pedal is down, a bit each.
    pedal: u16,
    /// A voice whose last key is up with its channel's pedal down: let go
    /// when the pedal is, or when a note takes the voice.
    sustained: [bool; MAX_VOICES],
    /// The KNOB glide's coefficient a block, for the bend (ADR 0112).
    bend_k: f32,
    /// Each Part's knobs, eased (ADR 0112), and the Part's loads they
    /// last saw.
    knobs: [(Knobs, Loads); MAX_PARTS],
    /// Each Part's FREE LFO phases (ADR 0135), stepped every block.
    lfo_clocks: [LfoClock; MAX_PARTS],
    /// How they move: glide, or land (`land_knobs`).
    knob_pace: Pace,
    /// The reverb's reserve from the budget this block, with GRAIN's
    /// (ADR 0202), and the reverb's tracker.
    rev_extra: Cost,
    rev_bill: RevBill,
    /// SETTINGS > TUNING in cents, eased a block at a time (never snap),
    /// and in semitones as the engines read it.
    tune_ease: Ease,
    tune: f32,
    /// Each voice's tuning offset (`tuning`, ADR 0240), eased at its
    /// glide's pace; nothing without `tuning`.
    tuned: crate::tuning::VoiceTunes,
    /// Each voice's stream: what admission counts.
    streaming: [Streaming; MAX_VOICES],
}

/// Who plays a note: the keys and MIDI, or one of ORBIT's rings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteOrigin {
    Live,
    Orbit(RingIdx),
}

/// The string-line bytes Sympathetic note-ons may clear in one block (spec
/// § 4.8): one worst-case note-on, so any one fits a fresh block. A
/// note-on past it waits a block, as a stolen note does: a chord of
/// worst-case slots starts a note a block, a typical one all at once.
pub const SYM_CLEAR_BUDGET: usize = SYM_NOTE_ON_CLEAR_MAX;

crate::in_place::field_list!(Instrument => Instrument { voices, sym, alloc, note_channel, note_origin, keys, held_keys, harmony, tempo, resnap_to, sounding, waiting, buses, sends, mix, sample_rate, last_kind, clear_left, clear_backlog, costs, costed, costings, wheel, expr, poly, pedal, sustained, bend_k, knobs, lfo_clocks, knob_pace, rev_extra, rev_bill, tune_ease, tune, tuned, streaming });

const SYMPATHETIC: SlotKind = SlotKind::Modal(ResonatorMode::Sympathetic);

impl Instrument {
    /// Reserved from the voice budget for a block's burst of note-ons
    /// while ORBIT plays (ADR 0091): five voice starts, its five rings
    /// striking in one block, the most it sends; each costs the pick and a
    /// voice start, and the first the block's `recost`. Layered Parts on
    /// one channel, or a live chord in the same block, start more and run
    /// into the headroom. With ORBIT stopped nothing is reserved: a DIN
    /// chord comes a note a millisecond, spread over blocks. Re-measured
    /// with `OrbitRunner::COST`: their sum must cover ORBIT's cold first
    /// block (65,387 at 785a028a).
    pub const BURST: Cost = Cost(859); // 5 note-ons in one block, 54,810 (ORBIT's first block, every voice cold; its own work 10,577, under `OrbitRunner::COST`, so the pair covers the 65,387 block), measured 2026-10-05, bench ORBIT PHASES, rev V at 480 MHz, 785a028a; + ALGO's retrigger floor (#486), 3 instructions an operator, 90 × 1.46 × 1.1 = 145 by ADR 0056's count; / 64, rounded up

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
        // written once by value before `assume_init_mut`. `voices` and
        // `knobs` are arrays, so the `cast` to their element keeps their
        // layout and `add(i)` for `i` below their length (`MAX_VOICES`,
        // `MAX_PARTS`) stays inside the field; each element is written once.
        unsafe {
            let voices = addr_of_mut!((*p).voices).cast::<Voice>();
            for (v, id) in VoiceIdx::ALL.into_iter().enumerate() {
                Voice::init_in_place(uninit_at(voices.add(v)), sample_rate, id);
            }
            SymPool::init_in_place(uninit_at(addr_of_mut!((*p).sym)));
            addr_of_mut!((*p).alloc).write(Allocator::new(budget));
            addr_of_mut!((*p).note_channel).write([MidiChannel::clamped(0); MAX_VOICES]);
            addr_of_mut!((*p).note_origin).write([NoteOrigin::Live; MAX_VOICES]);
            addr_of_mut!((*p).keys).write([MidiNote::A4; MAX_VOICES]);
            addr_of_mut!((*p).held_keys).write([NoteSet::EMPTY; MAX_VOICES]);
            addr_of_mut!((*p).harmony).write(None);
            addr_of_mut!((*p).tempo).write(Tempo::DEFAULT);
            addr_of_mut!((*p).resnap_to).write([None; MAX_VOICES]);
            addr_of_mut!((*p).sounding).write([0; MAX_VOICES]);
            addr_of_mut!((*p).waiting).write([None; MAX_VOICES]);
            addr_of_mut!((*p).buses).write([[0.0; BLOCK_SIZE]; MAX_PARTS]);
            addr_of_mut!((*p).sends).write([[0.0; BLOCK_SIZE]; FX_SENDS]);
            addr_of_mut!((*p).mix).write(MixState::default());
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            // The default Sound's (`Performance::new`: Algo).
            addr_of_mut!((*p).last_kind).write([SlotKind::Algo; MAX_PARTS]);
            addr_of_mut!((*p).clear_left).write(SYM_CLEAR_BUDGET);
            addr_of_mut!((*p).clear_backlog).write(false);
            addr_of_mut!((*p).costs).write([Cost::ZERO; MAX_PARTS]);
            addr_of_mut!((*p).costed).write(0);
            addr_of_mut!((*p).costings).write(0);
            addr_of_mut!((*p).wheel).write([0; 16]);
            addr_of_mut!((*p).expr).write([[0; 2]; 16]);
            addr_of_mut!((*p).poly).write([0; MAX_VOICES]);
            addr_of_mut!((*p).pedal).write(0);
            addr_of_mut!((*p).sustained).write([false; MAX_VOICES]);
            addr_of_mut!((*p).bend_k).write(crate::dsp::knobs::coeff(
                crate::block::GlideMs::KNOB,
                sample_rate,
            ));
            // A Part at a time: the whole array by value is 5 KB of stack.
            let knobs = addr_of_mut!((*p).knobs).cast::<(Knobs, Loads)>();
            for i in 0..MAX_PARTS {
                knobs.add(i).write((Knobs::UNPRIMED, Loads::NONE));
            }
            addr_of_mut!((*p).lfo_clocks).write([LfoClock::ZERO; MAX_PARTS]);
            addr_of_mut!((*p).knob_pace).write(Pace::Glide(sample_rate));
            addr_of_mut!((*p).rev_extra).write(Cost::ZERO);
            addr_of_mut!((*p).rev_bill).write(RevBill::new());
            addr_of_mut!((*p).tune_ease).write(Ease::default());
            addr_of_mut!((*p).tune).write(0.0);
            addr_of_mut!((*p).tuned).write(crate::tuning::VoiceTunes::NEW);
            addr_of_mut!((*p).streaming).write([Streaming::IDLE; MAX_VOICES]);
            slot.assume_init_mut()
        }
    }

    pub fn allocator(&self) -> &Allocator {
        &self.alloc
    }

    /// Who started each voice's note, by voice index: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn origins(&self) -> &[NoteOrigin; MAX_VOICES] {
        &self.note_origin
    }

    /// Each voice's slot rebuilds (`Voice::rebuilds`): for the tests and
    /// the bench.
    pub fn rebuilds(&self) -> [u16; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].rebuilds())
    }

    /// Voices whose engine sounds: for the bench's per-voice figures.
    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// The Parts with a voice sounding (a released one counts until it
    /// ends).
    pub fn sounding_set(&self) -> crate::project::PartSet {
        (0..MAX_VOICES)
            .filter(|&v| self.voices[v].is_active())
            .filter_map(|v| crate::project::PartId::new(self.sounding[v]))
            .fold(crate::project::PartSet::EMPTY, |s, p| s.with(p))
    }

    /// The voice budget booked, FX bus and ORBIT included, in percent: for
    /// `stats`.
    pub fn cost_pct(&self) -> u8 {
        self.alloc.cost_pct(self.reserved())
    }

    /// Spent outside the pool each block, so a new voice must fit beside
    /// it: the FX bus, the reverb TYPE's extra, the knobs' reads, the LFO
    /// clocks (ADR 0135), and while
    /// ORBIT plays its own block and a burst of note-ons (ADR 0091). It plays exactly while it hands a harmony.
    fn reserved(&self) -> Cost {
        let orbit = match self.harmony {
            Some(_) => OrbitRunner::COST + Self::BURST,
            None => Cost::ZERO,
        };
        FxBus::COST + self.rev_extra + crate::budget::CONTROL_RESERVE + orbit
    }

    /// Voices that ring a pool set, at most `SYM_SLOTS`: for the bench,
    /// whose Sympathetic figures bill a ringing voice, not a bare one.
    pub fn ringing(&self) -> usize {
        self.voices.iter().filter(|v| v.rings()).count()
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

    /// Voice `v`: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn voice(&self, v: usize) -> &Voice {
        &self.voices[v]
    }

    /// Whether each voice rings a pool set: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rings(&self) -> [bool; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].rings())
    }

    /// Each voice's last LEVEL clip (`Voice::filter_clip`): for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn filter_clips(&self) -> [crate::meter::Clip; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].filter_clip())
    }

    /// Each voice's filter settings last block, routes applied: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played_filters(&self) -> [crate::params::FilterParams; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].played_filter())
    }

    /// Each voice's value of `addr` handed its engine last block, knobs and
    /// routes applied: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played(&self, addr: crate::addr::ParamAddr) -> [f32; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].played_value(addr))
    }

    /// No Part's knobs move: the bench times from here.
    pub fn knobs_settled(&self) -> bool {
        self.knobs.iter().all(|k| k.0.settled())
    }

    /// Part `part`'s knobs read so far, wrapping: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn knob_reads(&self, part: usize) -> u32 {
        self.knobs[part].0.reads()
    }

    /// Part `part`'s knob `addr` this block, its 0..1 place and whether
    /// it moves: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn knob(&self, part: usize, addr: crate::addr::ParamAddr) -> Option<(f32, bool)> {
        self.knobs[part].0.place(addr)
    }

    /// Each voice's MORPH last block, routes applied: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played_morphs(&self) -> [f32; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].played_morph())
    }

    /// Each voice's glide steal ratio, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slides(&self) -> [f32; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].slide())
    }

    /// Times the Parts were costed (`recost`): for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn costings(&self) -> u32 {
        self.costings
    }

    /// The Part whose Sound each voice renders: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn sounding_parts(&self) -> [u8; MAX_VOICES] {
        self.sounding
    }

    /// Whether each voice sounds: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn active(&self) -> [bool; MAX_VOICES] {
        core::array::from_fn(|v| self.voices[v].is_active())
    }

    /// No voice sounds.
    pub fn quiet(&self) -> bool {
        !self.voices.iter().any(Voice::is_active)
    }

    /// A project load (ADR 0046): every voice fades over `Voice::FADE` on
    /// the snapshot it plays, and every waiting note goes. Each slot is
    /// marked dying, so `render` step 5 frees it as it does a shed one: a
    /// later note-off finds nothing and a held key isn't retriggered.
    /// Nothing is counted unheard.
    pub fn kill_all(&mut self) {
        self.alloc.kill_all();
        for v in &mut self.voices {
            let _ = v.kill();
        }
        self.waiting = [None; MAX_VOICES];
        self.resnap_to = [None; MAX_VOICES];
        self.sustained = [false; MAX_VOICES];
        crate::played::clear_held();
    }

    /// Part `part`'s mono bus from the last `render` (before pan and level).
    pub fn part_bus(&self, part: usize) -> &[f32; BLOCK_SIZE] {
        &self.buses[part]
    }

    /// A played key or MIDI note, with no pool behind it (`NO_SOURCES`):
    /// tests only, so no shell mutes TAPE by calling it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn handle(&mut self, ev: NoteEvent, shared: &AudioShared) {
        self.handle_with(ev, shared, &NO_SOURCES);
    }

    /// A played key or MIDI note: `handle_from_with` `NoteOrigin::Live`.
    pub fn handle_with(&mut self, ev: NoteEvent, shared: &AudioShared, src: &GateView<'_>) {
        self.handle_from_with(ev, shared, NoteOrigin::Live, src);
    }

    /// `handle_from_with` with no pool behind it (`NO_SOURCES`): tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn handle_from(&mut self, ev: NoteEvent, shared: &AudioShared, from: NoteOrigin) {
        self.handle_from_with(ev, shared, from, &NO_SOURCES);
    }

    /// A note-on on Part `part` plays nothing (`TapeEngine::refuses`): a
    /// TAPE Part with no source, an absent one, one its model can't play
    /// through this block's `src`, or a streamed one it needs in RAM. The
    /// allocator is never asked.
    pub fn refuses(&self, part: usize, shared: &AudioShared, src: &GateView<'_>) -> bool {
        let a = &shared.parts[part % MAX_PARTS];
        a.params.engine() == EngineType::Tape && TapeEngine::refuses(&a.params.tape, a.source, src)
    }

    /// `f` on admission's door (`AdmitDoor`): bank 2's through the door
    /// with `bank2`, `None` closed; this build's own without; never
    /// without TAPE, where nothing streams.
    #[inline(always)]
    fn admission<R>(f: impl FnOnce(&AdmitDoor) -> R) -> Option<R> {
        #[cfg(feature = "bank2")]
        return crate::bank2::DOOR_CELL.with(|d| f(&d.admit));
        #[cfg(all(feature = "tape", not(feature = "bank2")))]
        return Some(f(&Self::ADMIT));
        #[cfg(not(feature = "tape"))]
        {
            let _ = f;
            None
        }
    }

    /// What a note on Part `p` asks of the card (`AdmitDoor::bill`); a
    /// Part that doesn't play TAPE asks nothing.
    fn stream_bill(
        &self,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        at: admission::BillAt,
    ) -> Option<Paid> {
        if shared.parts[p.index()].params.engine() != EngineType::Tape {
            return None;
        }
        Self::admission(|d| d.bill(self, p, shared, src, at)).flatten()
    }

    /// What a note on Part `p` bills for its loop over its TAPE model's
    /// (`AdmitDoor::loop_extra`, ADR 0211); nothing off TAPE.
    fn loop_bill(
        &self,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        at: admission::BillAt,
    ) -> Cost {
        if shared.parts[p.index()].params.engine() != EngineType::Tape {
            return Cost::ZERO;
        }
        Self::admission(|d| d.loop_extra(self, p, shared, src, at)).unwrap_or(Cost::ZERO)
    }

    /// Once a block, before the voices render: the streamed notes'
    /// re-bill (`AdmitDoor::refresh`).
    fn refresh_bills(
        &mut self,
        shared: &AudioShared,
        src: &GateView<'_>,
        stats: Option<&StreamStats>,
    ) {
        Self::admission(|d| d.refresh(self, shared, src, stats));
    }

    /// The voice a note-on books: a streamed note through admission
    /// (`AdmitDoor::pick`), any other the plain pick. Returns what the
    /// note is paid; `None`: refused, counted.
    fn pick_admitted(
        &mut self,
        at: admission::PickAt,
        costs: (Cost, bool),
        stream: Option<Paid>,
        ctx: (Option<&StreamStats>, CardFault),
    ) -> Option<(usize, Option<Paid>)> {
        if let Some(new) = stream
            && let Some(r) = Self::admission(|d| d.pick(self, at, costs, new, ctx))
        {
            return r;
        }
        let v = self.pick_one(at, costs.0);
        if v.is_none() {
            self.alloc.refuse();
        }
        v.map(|v| (v, None))
    }

    /// The allocator's pick for a note-on (`Allocator::pick_where`), among
    /// voices of its origin. Bank 1's: its filter is a `dyn Fn`, whose
    /// table bank 1 holds.
    fn pick_one(&mut self, at: admission::PickAt, cost: Cost) -> Option<usize> {
        let reserved = self.reserved();
        let origins = &self.note_origin;
        let q = at.part.index() as u8;
        self.alloc
            .pick_where(q, at.mode, at.note, cost, reserved, &|v| {
                origins[v] == at.origin
            })
    }

    /// Voice `v`'s port this call, capped at what it is paid, only while
    /// admission bills the voice; a note never admitted reads nothing. Its
    /// deck finds its own source's file (#594).
    fn port_of(&self, v: usize) -> Port {
        let st = &self.streaming[v];
        match st.billed() {
            Some(_) => Port::On(st.paid.map(|p| p.ratio)),
            None => Port::Off,
        }
    }

    /// Voices playing a streamed note now: the card's load.
    pub fn streaming(&self) -> Option<StreamCount> {
        let n = self.voices.iter().filter(|v| v.streams()).count();
        // At most `MAX_VOICES`, which `StreamCount::MAX` is.
        StreamCount::new(n as u8)
    }

    /// A block on voice `v`'s stream: its age (from its release, or its
    /// start while held; a shed voice the oldest of all), and the end of a
    /// fade it was billed for.
    #[cfg(feature = "tape")]
    fn age_stream(&mut self, v: usize) {
        let s = self.alloc.slots()[v];
        let st = &mut self.streaming[v];
        let level = self.voices[v].level();
        st.age = if s.dying() {
            Age::DYING
        } else {
            st.age.after_block(s.held(), level)
        };
        if !self.voices[v].fading() {
            st.fading = None;
        }
    }

    #[cfg(not(feature = "tape"))]
    #[inline(always)]
    fn age_stream(&mut self, _: usize) {}

    /// The voices' bills as admission counts them: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stream_bills(&self) -> [Option<Bill>; MAX_VOICES] {
        self.streaming.map(|s| s.billed())
    }

    /// The sources the voices read or will (spec § 4): each active voice's
    /// latched source, fading or not, the one a note queued behind its
    /// fade latched (`Voice::reads`), and a note waiting for another
    /// Part's fade. Stored once a block, it holds compaction off.
    pub fn source_mask(&self) -> u32 {
        let voices = self.voices.iter().flat_map(Voice::reads);
        let waiting = self.waiting.iter().map(|w| w.and_then(|w| w.source));
        voices
            .chain(waiting)
            .flatten()
            .fold(0, |m, id| m | id.bit())
    }

    /// Note-on: play on every Part listening on the channel, a live key
    /// snapped per Part while ORBIT runs. Note-off: release the voices this
    /// channel, `from` and the key started. A Poly re-strike takes back
    /// only `from`'s own voice of the note. A note-on judges against the
    /// Parts' costs worked out once for the block's snapshot (`recost`), so
    /// a burst, admitted or refused, costs them once.
    pub fn handle_from_with(
        &mut self,
        ev: NoteEvent,
        shared: &AudioShared,
        from: NoteOrigin,
        src: &GateView<'_>,
    ) {
        self.handle_streamed(ev, shared, from, src, &mut NoPorts);
    }

    /// `handle_from_with`, a streamed note starting through its voice's
    /// port from `ports` (slice 1 § 7.1).
    pub fn handle_streamed<P: VoicePorts>(
        &mut self,
        ev: NoteEvent,
        shared: &AudioShared,
        from: NoteOrigin,
        src: &GateView<'_>,
        ports: &mut P,
    ) {
        match ev.kind {
            NoteKind::On(vel) => {
                let live = from == NoteOrigin::Live;
                #[cfg(feature = "probe")]
                let t0 = crate::probe::now();
                // Judge the note against this block's patches, not the last.
                self.recost(shared);
                #[cfg(feature = "probe")]
                crate::probe::acc(0, crate::probe::now().wrapping_sub(t0));
                #[cfg(feature = "probe")]
                crate::probe::acc(3, 1);
                for (p, part) in shared.parts.iter().enumerate() {
                    if part.mix.channel != ev.channel || self.refuses(p, shared, src) {
                        continue;
                    }
                    let (q, mode) = (p as u8, part.mix.mode);
                    let Some(id) = PartId::new(q) else {
                        continue;
                    };
                    // ORBIT's notes are snapped in their ring.
                    let note = match from {
                        NoteOrigin::Live => part.live_note(ev.note, self.harmony),
                        NoteOrigin::Orbit(_) => ev.note,
                    };
                    #[cfg(feature = "probe")]
                    let t1 = crate::probe::now();
                    // ONE at RETRIG CUT: the note chokes the Part's others
                    // first, so its pick and admission take their room
                    // (shed voices go first) and nobody else's.
                    if part.params.engine() == EngineType::Tape
                        && crate::dsp::tape_engine::cuts(&part.params.tape)
                    {
                        self.cut_part(q);
                    }
                    // A Part's first voice brings its knobs (ADR 0112).
                    let first = !self.alloc.slots().iter().any(|s| s.part() == Some(q));
                    let knobs = if first { Knobs::COST } else { Cost::ZERO };
                    let mut cost = self.costs[p] + knobs + self.high_note(p, shared, note, first);
                    let at = admission::BillAt {
                        key: ev.note,
                        note,
                        source: part.source,
                    };
                    let stream = self.stream_bill(id, shared, src, at);
                    if stream.is_some() {
                        cost = cost + COST_STREAM;
                    }
                    let looped = self.loop_bill(id, shared, src, at);
                    cost = cost + looped;
                    let glides = part.params.pitch.steal_glide().is_some();
                    let pick = admission::PickAt {
                        part: id,
                        mode,
                        note,
                        origin: from,
                    };
                    let Some((v, stream)) = self.pick_admitted(
                        pick,
                        (cost, glides),
                        stream,
                        (ports.stats(), ports.fault()),
                    ) else {
                        continue;
                    };
                    // Sympathetic: a free slot is promised to it, else it
                    // plays bare (ADR 0054). Another kind's promise goes when
                    // the voice triggers it, or rests (`Voice::trigger`,
                    // `Voice::rest`).
                    if SlotKind::of(&part.params) == SYMPATHETIC {
                        self.sym.alloc_mut().place(VoiceIdx::ALL[v]);
                    }
                    // A key whose note this Part's voice already holds, from
                    // this origin and channel, joins its held keys.
                    let was = self.alloc.slots()[v];
                    let joins = was.held()
                        && !was.dying()
                        && was.part() == Some(q)
                        && was.note() == Some(note)
                        && self.note_origin[v] == from
                        && self.note_channel[v] == ev.channel;
                    let base = if joins {
                        self.held_keys[v]
                    } else {
                        NoteSet::EMPTY
                    };
                    self.held_keys[v] = base.with(ev.note);
                    self.alloc.book(v, q, mode, note, cost);
                    let waits = self.sounding[v] as usize != p && self.voices[v].is_active();
                    // The key's tuning: a glide eases to it at its own pace,
                    // a strike starts on it.
                    let to = crate::tuning::offset(p, note.get());
                    let start = match part.params.pitch.steal_glide() {
                        Some(tau) if self.voices[v].is_active() => {
                            let keep = crate::tuning::keep_for(tau, self.sample_rate);
                            self.tuned.glide(v, to, keep)
                        }
                        _ => self.tuned.strike(v, to),
                    };
                    // A voice taken from another Part fades out on that
                    // Part's Sound before the note starts: billed what it
                    // was (a held Modal's extra in) till then (ADR 0091). A
                    // shed one's fade was never billed (ADR 0027).
                    if waits && !was.dying() {
                        self.alloc.fade_from(v, was.cost());
                    }
                    // Sympathetic starts now only if the block's clear
                    // budget takes it and no note waits on it before; else
                    // it waits, as a steal does (spec § 4.8).
                    let deferred = !waits
                        && SlotKind::of(&part.params) == SYMPATHETIC
                        && !(self.voices[v].starts_now(&part.params, &self.sym)
                            && self.admit(v, &part.params, note));
                    #[cfg(feature = "probe")]
                    let t2 = crate::probe::now();
                    #[cfg(feature = "probe")]
                    crate::probe::acc(1, t2.wrapping_sub(t1));
                    let voice = &mut self.voices[v];
                    let waited = self.waiting[v].take().is_some();
                    let queued = if waits || deferred {
                        // Another Part's sound fades out on its own bus and
                        // settings first; a Sympathetic note past the clear
                        // budget waits a block.
                        self.waiting[v] = Some(Waiting {
                            vel,
                            source: part.source,
                        });
                        voice.kill()
                    } else {
                        self.sounding[v] = q;
                        let base = CtxBase {
                            sym: &mut self.sym,
                            src,
                            part: part_id(q),
                            tune: self.tune + start,
                        };
                        // Unbilled, no port (`port_of`).
                        let port = match stream {
                            Some(p) => Port::On(Some(p.ratio)),
                            None => Port::Off,
                        };
                        ports.ctx(base, VoiceIdx::ALL[v], port, |ctx| {
                            voice.note_on(note, vel, &part.params, part.source, ctx)
                        })
                    };
                    // The voice's old stream, if it fades out first on the
                    // same ring, counts until the fade ends.
                    #[cfg(feature = "tape")]
                    {
                        let old = self.streaming[v].billed();
                        self.streaming[v] = Streaming {
                            paid: stream,
                            fading: old.filter(|_| self.voices[v].fading()),
                            age: Age::FRESH,
                            capped: false,
                            looped,
                        };
                    }
                    #[cfg(not(feature = "tape"))]
                    let _ = looped;
                    #[cfg(feature = "probe")]
                    crate::probe::acc(2, crate::probe::now().wrapping_sub(t2));
                    // A note replaced before it sounded (ADR 0027).
                    if waited || queued {
                        self.alloc.dropped_unheard();
                    }
                    self.note_channel[v] = ev.channel;
                    self.note_origin[v] = from;
                    self.keys[v] = ev.note;
                    self.poly[v] = 0;
                    // A key down again, or a steal, takes the voice back
                    // from the pedal.
                    self.sustained[v] = false;
                    self.resnap_to[v] = None;
                    if live {
                        // The UI's screensaver: the note as this Part sounds it.
                        let cents = libm::roundf(part.params.pitch.semitones() * 100.0) as i16;
                        crate::played::note_on(
                            ev.channel.get(),
                            q,
                            ev.note.get(),
                            vel.get(),
                            note.get(),
                            cents,
                        );
                    }
                }
            }
            NoteKind::Off => {
                if from == NoteOrigin::Live {
                    crate::played::note_off(ev.channel.get(), ev.note.get());
                }
                // By voice index: every held voice this channel and `from`
                // hold the key on (one per layered Part), let go with the
                // last of its keys.
                #[cfg(feature = "probe")]
                crate::probe::acc(5, 1);
                for v in 0..MAX_VOICES {
                    let s = self.alloc.slots()[v];
                    if !(s.held()
                        && self.held_keys[v].contains(ev.note)
                        && self.note_channel[v] == ev.channel
                        && self.note_origin[v] == from)
                    {
                        continue;
                    }
                    self.held_keys[v] = self.held_keys[v].without(ev.note);
                    if let Some(left) = self.held_keys[v].lowest() {
                        // A key still down names it now, its pressure
                        // unheard yet.
                        self.keys[v] = left;
                        self.poly[v] = 0;
                    } else if from == NoteOrigin::Live && self.pedal_down(ev.channel) {
                        // The pedal holds it. ORBIT's rings are not the
                        // player's keys: they end on their own.
                        self.sustained[v] = true;
                    } else {
                        self.let_go(v);
                    }
                }
            }
        }
    }

    /// Part `part`'s pitch bend now, semitones: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn bend_of(&self, part: usize) -> f32 {
        self.knobs[part].0.bend()
    }

    fn pedal_down(&self, ch: MidiChannel) -> bool {
        self.pedal & (1 << ch.get()) != 0
    }

    /// Voice `v`'s note ends (its last key up, or the pedal).
    fn let_go(&mut self, v: usize) {
        self.alloc.release(v);
        self.resnap_to[v] = None;
        self.voices[v].note_off(&mut EngineCtx {
            sym: &mut self.sym,
            streams: None,
            src: &NO_SOURCES,
            part: part_id(self.sounding[v]),
            tune: self.tune,
        });
    }

    /// A channel's wheel moved: its Parts' pitch glides there
    /// (`ease_bend`), never steps.
    pub fn bend(&mut self, ev: BendEvent) {
        self.wheel[usize::from(ev.channel.get())] = ev.value.clamp(-8192, 8191);
    }

    /// A channel's mod wheel or pressure: its Parts' WHEEL and PRESSURE
    /// glide there (`ease_bend`). Poly pressure reaches the held voices its
    /// key struck on that channel, never ORBIT's.
    pub fn expr(&mut self, ev: ExprEvent) {
        let ch = usize::from(ev.channel.get()) & 15;
        match ev.expr {
            Expr::Wheel(v) => self.expr[ch][0] = v.get(),
            Expr::Pressure(v) => self.expr[ch][1] = v.get(),
            Expr::Poly(key, v) => {
                for i in 0..MAX_VOICES {
                    // A held key's voice: a releasing one of a re-struck
                    // key keeps its last value.
                    if self.alloc.slots()[i].held()
                        && self.note_channel[i] == ev.channel
                        && self.note_origin[i] == NoteOrigin::Live
                        && self.keys[i] == key
                    {
                        self.poly[i] = v.get();
                    }
                }
            }
        }
    }

    /// A channel's sustain pedal: down holds the keys it lifts; up lets
    /// them go.
    pub fn sustain(&mut self, ev: SustainEvent) {
        let bit = 1 << ev.channel.get();
        if ev.down {
            self.pedal |= bit;
            return;
        }
        self.pedal &= !bit;
        for v in 0..MAX_VOICES {
            if self.sustained[v] && self.note_channel[v] == ev.channel {
                self.sustained[v] = false;
                if self.alloc.slots()[v].held() && self.held_keys[v].is_empty() {
                    self.let_go(v);
                }
            }
        }
    }

    /// CC 120, 121, 123 on a channel's Parts.
    pub fn channel_mode(&mut self, ev: ChannelModeEvent) {
        let ch = ev.channel;
        match ev.mode {
            ChannelMode::ResetControllers => {
                self.bend(BendEvent {
                    channel: ch,
                    value: 0,
                });
                self.sustain(SustainEvent {
                    channel: ch,
                    down: false,
                });
                self.expr[usize::from(ch.get()) & 15] = [0; 2];
                for v in 0..MAX_VOICES {
                    if self.note_channel[v] == ch {
                        self.poly[v] = 0;
                    }
                }
            }
            ChannelMode::NotesOff => {
                crate::played::clear_channel(ch.get());
                for v in 0..MAX_VOICES {
                    if self.alloc.slots()[v].held()
                        && self.note_channel[v] == ch
                        && self.note_origin[v] == NoteOrigin::Live
                        && !self.held_keys[v].is_empty()
                    {
                        self.held_keys[v] = NoteSet::EMPTY;
                        if self.pedal_down(ch) {
                            self.sustained[v] = true;
                        } else {
                            self.let_go(v);
                        }
                    }
                }
            }
            ChannelMode::SoundOff => {
                crate::played::clear_channel(ch.get());
                for v in 0..MAX_VOICES {
                    if !self.alloc.slots()[v].is_free() && self.note_channel[v] == ch {
                        self.sound_off(v);
                    }
                }
            }
        }
    }

    /// Voice `v` fades out now, whatever holds it (CC 120's).
    fn sound_off(&mut self, v: usize) {
        self.held_keys[v] = NoteSet::EMPTY;
        self.sustained[v] = false;
        self.resnap_to[v] = None;
        self.alloc.kill(v);
        self.shed(v);
    }

    /// Whether `part`'s Sound is TAPE.
    #[cfg(feature = "bank2-gate")]
    fn part_is_tape(shared: &AudioShared, part: usize) -> bool {
        shared.parts[part % MAX_PARTS].params.engine() == EngineType::Tape
    }

    /// Whether voice `v` still plays a TAPE Part's Sound, or is booked to
    /// one (a TAPE note waiting on a fading voice).
    #[cfg(feature = "bank2-gate")]
    fn is_tape(&self, v: usize, shared: &AudioShared) -> bool {
        let slot = self.alloc.slots()[v];
        !slot.is_free()
            && (Self::part_is_tape(shared, self.sounding[v] as usize)
                || slot
                    .part()
                    .is_some_and(|q| Self::part_is_tape(shared, q as usize)))
    }

    /// CC 120 for TAPE's voices alone, on any channel: a bank-2 write is
    /// about to take TAPE's DSP away, so they fade now (not a release
    /// tail), and a key held down does not come back. The other Parts play.
    /// A TAPE sound fading under another Part's waiting note (a steal)
    /// only loses its DSP: the slot and the note stay, and start when it
    /// rests. Only a gate build's `bank2 erase` writes under an open door.
    #[cfg(feature = "bank2-gate")]
    pub fn sound_off_tape(&mut self, shared: &AudioShared) {
        for v in 0..MAX_VOICES {
            if !self.is_tape(v, shared) {
                continue;
            }
            let owner = self.alloc.slots()[v].part();
            if owner.is_some_and(|q| !Self::part_is_tape(shared, q as usize))
                && self.waiting[v].is_some()
            {
                let _ = self.voices[v].kill();
            } else {
                self.sound_off(v);
            }
        }
    }

    /// A TAPE voice still sounds, fading too.
    #[cfg(feature = "bank2-gate")]
    pub fn tape_sounding(&self, shared: &AudioShared) -> bool {
        (0..MAX_VOICES).any(|v| self.is_tape(v, shared))
    }

    /// Each Part's wheel reach in semitones, glided as a knob: a Part with
    /// no voice lands (nothing to zip).
    fn ease_bend(&mut self, shared: &AudioShared, playing: [bool; MAX_PARTS]) {
        let k = match self.knob_pace {
            Pace::Glide(_) => Some(self.bend_k),
            Pace::Land => None,
        };
        for ((knobs, part), playing) in self.knobs.iter_mut().zip(&shared.parts).zip(playing) {
            let w = self.wheel[usize::from(part.mix.channel.get())];
            let unit = if w > 0 {
                f32::from(w) / 8191.0
            } else {
                f32::from(w) / 8192.0
            };
            knobs
                .0
                .ease_bend(unit * f32::from(part.mix.bend), k.filter(|_| playing));
            let [wheel, press] = self.expr[usize::from(part.mix.channel.get()) & 15];
            knobs.0.ease_expr(
                [f32::from(wheel) / 127.0, f32::from(press) / 127.0],
                k.filter(|_| playing),
            );
        }
    }

    /// ORBIT's key and chord, handed on each block by its runner: live
    /// keys snap to it (`None`: ORBIT stopped, nothing snaps).
    pub fn set_harmony(&mut self, h: Option<Harmony>) {
        self.harmony = h;
    }

    /// ORBIT's tempo this block (`OrbitRunner::tempo`): what the reverb's
    /// synced times follow.
    pub fn set_tempo(&mut self, t: Tempo) {
        self.tempo = t;
    }

    /// The tempo the reverb follows.
    pub fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// Any harmony change (spec § 5.2): each held live voice on a HELD
    /// RE-SNAP Part glides to its key snapped to the harmony now, from the
    /// key, never from where it was. At most one
    /// retune a voice; a SYMP voice whose rings' growth doesn't fit the
    /// block's clear budget waits for the next block's.
    pub fn resnap(&mut self, shared: &AudioShared) {
        let Some(h) = self.harmony else {
            return;
        };
        for v in 0..MAX_VOICES {
            let s = self.alloc.slots()[v];
            let (Some(q), Some(now)) = (s.part(), s.note()) else {
                continue;
            };
            let part = &shared.parts[q as usize % MAX_PARTS];
            if !s.held()
                || s.dying()
                || self.note_origin[v] != NoteOrigin::Live
                || part.mix.held != Held::Resnap
                || part.mix.snap == LiveSnap::Off
            {
                continue;
            }
            let to = part.live_note(self.keys[v], Some(h));
            if to != now {
                self.alloc.renote(v, to);
                self.resnap_to[v] = Some(to);
            }
        }
        self.serve_resnaps();
    }

    /// The re-snaps the clear budget lets start: a SYMP voice's takes its
    /// rings' growth from it, unless a note waits on it first; any other
    /// writes nothing it bills. A voice let go meanwhile has none.
    fn serve_resnaps(&mut self) {
        let glide = RESNAP_GLIDE_MS * self.sample_rate / 1000;
        for v in 0..MAX_VOICES {
            let Some(to) = self.resnap_to[v] else {
                continue;
            };
            if !self.alloc.slots()[v].held() {
                self.resnap_to[v] = None;
                continue;
            }
            if self.voices[v].kind() == SYMPATHETIC {
                let bytes = self.voices[v].retune_clear(&self.sym, to);
                if self.clear_backlog || bytes > self.clear_left {
                    continue;
                }
                self.clear_left -= bytes;
            }
            let keep = crate::tuning::keep_for(RESNAP_GLIDE_MS as f32 / 3_000.0, self.sample_rate);
            let off = crate::tuning::offset(usize::from(self.sounding[v]), to.get());
            self.tuned.glide(v, off, keep);
            self.voices[v].retune(
                to,
                glide,
                &mut EngineCtx {
                    sym: &mut self.sym,
                    streams: None,
                    src: &NO_SOURCES,
                    part: part_id(self.sounding[v]),
                    tune: self.tune,
                },
            );
            self.resnap_to[v] = None;
        }
    }

    /// Starts the waiting notes on idle voices oldest first, by `Allocator`
    /// age, while the clear budget takes them: the first that doesn't fit
    /// queues the rest (`admit`).
    fn start_waiting<P: VoicePorts>(
        &mut self,
        shared: &AudioShared,
        src: &GateView<'_>,
        ports: &mut P,
    ) {
        let mut queue = [(u32::MAX, 0usize); MAX_VOICES];
        let mut n = 0;
        for v in 0..MAX_VOICES {
            if !self.voices[v].is_active() && self.waiting[v].is_some() {
                queue[n] = (self.alloc.slots()[v].age(), v);
                n += 1;
            }
        }
        queue[..n].sort_unstable();
        for &(_, v) in &queue[..n] {
            let s = self.alloc.slots()[v];
            if let (Some(w), Some(q), Some(note)) = (self.waiting[v], s.part(), s.note())
                && self.start(v, (w, q, note, s.held()), shared, src, ports)
            {
                self.waiting[v] = None;
            }
        }
    }

    /// Starts idle voice `v`'s waiting note `w` on Part `q`,
    /// released at once unless `held`, if the block's clear budget lets it:
    /// false if it must wait on. A Sympathetic note is placed as it starts
    /// (its Part may have become Sympathetic while it waited): it keeps its
    /// promise, takes a free slot, or plays bare.
    fn start<P: VoicePorts>(
        &mut self,
        v: usize,
        (w, q, note, held): (Waiting, u8, MidiNote, bool),
        shared: &AudioShared,
        src: &GateView<'_>,
        ports: &mut P,
    ) -> bool {
        let params = &shared.parts[q as usize % MAX_PARTS].params;
        if SlotKind::of(params) == SYMPATHETIC {
            self.sym.alloc_mut().place(VoiceIdx::ALL[v]);
        }
        if !self.admit(v, params, note) {
            return false;
        }
        self.alloc.fade_from(v, Cost::ZERO);
        #[cfg(feature = "tape")]
        {
            self.streaming[v].fading = None;
        }
        let port = self.port_of(v);
        let to = self
            .tuned
            .strike(v, crate::tuning::offset(usize::from(q), note.get()));
        let voice = &mut self.voices[v];
        let base = CtxBase {
            sym: &mut self.sym,
            src,
            part: part_id(q),
            tune: self.tune + to,
        };
        ports.ctx(base, VoiceIdx::ALL[v], port, |ctx| {
            let _ = voice.note_on(note, w.vel, params, w.source, ctx);
            if !held {
                voice.note_off(ctx);
            }
        });
        self.sounding[v] = q;
        true
    }

    /// Whether voice `v` may start `note` on `params` this block: anything
    /// but Sympathetic may; Sympathetic if no note waits on the clear
    /// budget before it and its clear fits what the block has left, which
    /// it then takes. One that doesn't fit starts the queue.
    fn admit(&mut self, v: usize, params: &ParamSnapshot, note: MidiNote) -> bool {
        if SlotKind::of(params) != SYMPATHETIC {
            return true;
        }
        if self.clear_backlog {
            return false;
        }
        let bytes = self.voices[v].sym_note_on_clear(&self.sym, note, params);
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
    /// and `render` step 5 frees it. Once a block: a later note-on, or the
    /// render, on the same snapshot reuses it (ADR 0091), so a burst of
    /// notes, admitted or refused, costs the Parts once.
    fn recost(&mut self, shared: &AudioShared) {
        let at = core::ptr::from_ref(shared) as usize;
        if self.costed == at {
            return;
        }
        #[cfg(feature = "probe")]
        let t0 = crate::probe::now();
        // Only a Part with a gliding knob pays the glide scans (ADR 0117).
        let glides: [Cost; MAX_PARTS] = core::array::from_fn(|p| {
            let g = self.knobs[p].0.gliding();
            if g == Gliding::default() {
                return Cost::ZERO;
            }
            let part = &shared.parts[p];
            Voice::glide_extra(&part.params, &part.mod_state, g)
        });
        let costs: [Cost; MAX_PARTS] = core::array::from_fn(|p| shared.parts[p].voice_cost());
        // A new note is judged with its Part's glide on it (ADR 0117).
        self.costs = core::array::from_fn(|p| costs[p] + glides[p]);
        (self.costed, self.costings) = (at, self.costings.wrapping_add(1));
        #[cfg(feature = "probe")]
        crate::probe::acc(8, crate::probe::now().wrapping_sub(t0));
        // A sounding Part's knobs and formant reach, once: on its first
        // voice (ADR 0112, ADR 0131).
        let mut knobs = [false; MAX_PARTS];
        let mut reach: [Option<HighNote>; MAX_PARTS] = [None; MAX_PARTS];
        for v in 0..MAX_VOICES {
            if let Some(p) = self.alloc.slots()[v].part() {
                let p = p as usize % MAX_PARTS;
                let part = &shared.parts[p];
                let first = !core::mem::replace(&mut knobs[p], true);
                if first {
                    reach[p] =
                        HighNote::of(&part.params, &part.mod_state, part.mix.bend, shared.tune);
                }
                let high = match (self.alloc.slots()[v].note(), reach[p]) {
                    (Some(n), Some(h)) => h.bill(n, self.voices[v].algo_slide(), first),
                    _ => Cost::ZERO,
                };
                let held = self.voices[v].held_model_extra(&part.params)
                    + self.voices[v].held_level_extra(&part.params, &part.mod_state)
                    + self.voices[v].held_ops_extra(&part.params, &part.mod_state)
                    + self.streaming[v].looped
                    + high;
                let k = if first { Knobs::COST } else { Cost::ZERO };
                self.alloc
                    .recost_with_glide(v, costs[p] + held + k, glides[p]);
            }
        }
        // Only the bus, the reverb's extra, the knobs' reads and the LFO
        // clocks: ORBIT's
        // reserve refuses new voices, it never fades out sounding ones
        // (ADR 0091).
        while let Some(v) = self
            .alloc
            .shed(FxBus::COST + self.rev_extra + crate::budget::CONTROL_RESERVE)
        {
            self.shed(v);
        }
    }

    /// A new formant note's reach on Part `p` (`HighNote::bill`, the
    /// Part's term on its `first` voice), its spans past C8 judged at a
    /// glide steal's start too: any of the Part's sounding voices may be
    /// taken, so its highest note and largest slide.
    #[inline(never)]
    fn high_note(&self, p: usize, shared: &AudioShared, note: MidiNote, first: bool) -> Cost {
        let part = &shared.parts[p];
        let (mut top, mut slide) = (note.get(), 1.0f32);
        if part.params.pitch.steal_glide().is_some() {
            for v in 0..MAX_VOICES {
                let s = self.alloc.slots()[v];
                if s.part() == Some(p as u8) {
                    top = top.max(s.note().map_or(0, MidiNote::get));
                    slide = slide.max(self.voices[v].algo_slide());
                }
            }
        }
        HighNote::of(&part.params, &part.mod_state, part.mix.bend, shared.tune)
            .map_or(Cost::ZERO, |h| {
                h.bill(MidiNote::new(top).unwrap_or(note), slide, first)
            })
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

    /// Every sounding voice of Part `q` fades out, as a shed one does:
    /// RETRIG CUT.
    fn cut_part(&mut self, q: u8) {
        for w in 0..MAX_VOICES {
            let s = self.alloc.slots()[w];
            if s.part() == Some(q) && !s.dying() && !s.is_free() {
                self.alloc.kill(w);
                self.shed(w);
            }
        }
    }

    /// A Part whose Sound has just become Sympathetic: its held, sounding
    /// notes restart on it, each fading first and waiting as another
    /// Part's steal does, so each restart's clear goes through the budget.
    /// Newest first, they are promised the free slots, so the last four
    /// played ring and the rest play bare (the owner's rule, 2026-09-29,
    /// spec § 4.5). Nothing is evicted.
    fn restart_switched(&mut self, shared: &AudioShared) {
        for (p, part) in shared.parts.iter().enumerate() {
            let kind = SlotKind::of(&part.params);
            let became = kind == SYMPATHETIC && self.last_kind[p] != SYMPATHETIC;
            self.last_kind[p] = kind;
            if !became {
                continue;
            }
            // A note already Sympathetic (a note-on since the switch) keeps
            // what it has, unless it fades toward its own restart (MODE
            // flipped away and back within the fade); a shed note stays
            // shed.
            let mut held = [(VoiceIdx::ALL[0], 0u32); MAX_VOICES];
            let mut n = 0;
            for (v, s) in self.alloc.slots().iter().enumerate() {
                let voice = &self.voices[v];
                if s.held()
                    && !s.dying()
                    && s.part() == Some(p as u8)
                    && self.sounding[v] as usize == p
                    && voice.is_active()
                    && (voice.kind() != SYMPATHETIC || voice.restarts())
                {
                    held[n] = (VoiceIdx::ALL[v], s.age());
                    n += 1;
                }
            }
            let held = &mut held[..n];
            held.sort_unstable_by_key(|&(_, age)| Reverse(age));
            for &(id, _) in held.iter() {
                let v = id.index();
                self.sym.alloc_mut().place(id);
                // Its note waits for the fade; a note that came mid-fade
                // gives way to it.
                let vel = self.voices[v].velocity();
                self.waiting[v].get_or_insert(Waiting { vel, source: None });
                let _ = self.voices[v].kill();
            }
        }
    }

    /// The Part knobs land instead of gliding: an offline render that
    /// drives a knob block by block (the factory tapes' MORPH sweep) plays
    /// it exactly.
    pub fn land_knobs(&mut self) {
        self.knob_pace = Pace::Land;
    }

    /// Each Part's knobs this block (ADR 0112, 0116): what changed eased
    /// while a voice plays the Part, landed while none does and on a new
    /// Sound.
    fn ease_knobs(&mut self, shared: &AudioShared) {
        let mut playing = [false; MAX_PARTS];
        let k = matches!(self.knob_pace, Pace::Glide(_)).then_some(self.bend_k);
        for v in 0..MAX_VOICES {
            if !self.alloc.slots()[v].is_free() {
                playing[self.sounding[v] as usize % MAX_PARTS] = true;
                self.voices[v].ease_press(f32::from(self.poly[v]) / 127.0, k);
            }
        }
        self.ease_bend(shared, playing);
        // Each Part takes its delta's moves (ADR 0116): a quiet one, none.
        let pace = self.knob_pace;
        for ((knobs, part), playing) in self.knobs.iter_mut().zip(&shared.parts).zip(playing) {
            new_sound(knobs, part);
            let k = &mut knobs.0;
            let sound = (&part.params, &part.mod_state);
            if !k.quiet(sound, &part.knobs) {
                k.ease(sound, &part.knobs, playing.then_some(pace));
            }
        }
    }

    /// One block as the shells run it (spec § 4): `notes` (the load gate,
    /// the drain, ORBIT) and the render read the pool through the block's
    /// view, then the sources the voices hold are stored, holding
    /// compaction off.
    pub fn run_block(
        &mut self,
        sources: &mut AudioToken,
        out: (&mut FxBus, &mut DacBlocks, &mut ScopeWriter),
        shared: &AudioShared,
        notes: impl FnOnce(&mut Self, &GateView<'_>),
    ) {
        self.run_block_ported(sources, out, shared, &mut NoPorts, |inst, reads, _| {
            notes(inst, reads)
        });
    }

    /// `run_block`, the notes and the render reaching each streamed
    /// voice's port through `ports` (slice 1 § 7.1).
    pub fn run_block_ported<P: VoicePorts>(
        &mut self,
        sources: &mut AudioToken,
        (fx, dac, scope): (&mut FxBus, &mut DacBlocks, &mut ScopeWriter),
        shared: &AudioShared,
        ports: &mut P,
        notes: impl FnOnce(&mut Self, &GateView<'_>, &mut P),
    ) {
        {
            let reads = sources.begin_block();
            notes(self, &reads, ports);
            self.render_streamed(fx, dac, shared, scope, &reads, ports);
        }
        sources.store_sounding(self.source_mask());
    }

    /// `render_with` with no pool behind it (`NO_SOURCES`): tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render(
        &mut self,
        fx: &mut FxBus,
        dac: &mut DacBlocks,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
    ) {
        self.render_with(fx, dac, shared, scope, &NO_SOURCES);
    }

    /// Render one block into the three DAC pairs, TAPE's voices reading
    /// through the block's `src`: `dac.out()` after.
    pub fn render_with(
        &mut self,
        fx: &mut FxBus,
        dac: &mut DacBlocks,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
        src: &GateView<'_>,
    ) {
        self.render_streamed(fx, dac, shared, scope, src, &mut NoPorts);
    }

    /// `render_with`, each streamed voice reading through its port from
    /// `ports` (slice 1 § 7.1).
    pub fn render_streamed<P: VoicePorts>(
        &mut self,
        fx: &mut FxBus,
        dac: &mut DacBlocks,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
        src: &GateView<'_>,
        ports: &mut P,
    ) {
        // The reverb TYPE's extra, and GRAIN's (ADR 0202).
        self.rev_extra = self.rev_bill.step(shared.fx.reverb.kind)
            + crate::dsp::delay::extra_cost(&shared.fx.delay);
        let (_, cents) = self
            .tune_ease
            .step(f32::from(shared.tune), ease_coeff(self.sample_rate));
        self.tune = cents / 100.0;
        self.recost(shared);
        self.restart_switched(shared);

        // 1. Voices into their part's mono bus. The first voice of a part is
        //    copied, not added, so a lone voice reaches the bus bit-for-bit.
        let mut written = [false; MAX_PARTS];
        let mut clip = [crate::meter::Clip::ZERO; MAX_PARTS];
        for bus in self.buses.iter_mut() {
            bus.fill(0.0);
        }
        let mut block = [0.0f32; BLOCK_SIZE];
        // 0. Notes that waited on the clear budget, their voices idle, start
        //    before the voices render: let in this block, one sounds in it.
        self.clear_backlog = false;
        self.refresh_bills(shared, src, ports.stats());
        self.start_waiting(shared, src, ports);
        self.serve_resnaps();
        #[cfg(feature = "probe")]
        let t0 = crate::probe::now();
        self.ease_knobs(shared);
        // 9: the knobs' ease, reads and glides (ADR 0115).
        #[cfg(feature = "probe")]
        crate::probe::acc(9, crate::probe::now().wrapping_sub(t0));
        for v in 0..MAX_VOICES {
            if self.alloc.slots()[v].is_free() {
                ports.drop_ring(VoiceIdx::ALL[v]);
                continue;
            }
            let p = self.sounding[v] as usize % MAX_PARTS;
            let part = &shared.parts[p];
            let port = self.port_of(v);
            // A glide's or re-snap's tuning eases at its pace.
            let off = self.tuned.step(v);
            let base = CtxBase {
                sym: &mut self.sym,
                src,
                part: PartId::ALL[p],
                tune: self.tune + off,
            };
            let voice = &mut self.voices[v];
            let (knobs, clock) = (&self.knobs[p].0, &self.lfo_clocks[p]);
            ports.ctx(base, VoiceIdx::ALL[v], port, |ctx| {
                #[cfg(feature = "bench-warm")]
                crate::warm::twice(voice, &mut |voice| {
                    voice.render_knobs(
                        &mut block,
                        &part.params,
                        &part.mod_state,
                        knobs,
                        Some(clock),
                        ctx,
                    )
                });
                #[cfg(not(feature = "bench-warm"))]
                voice.render_knobs(
                    &mut block,
                    &part.params,
                    &part.mod_state,
                    knobs,
                    Some(clock),
                    ctx,
                );
            });
            self.age_stream(v);
            // Killed, rebuilt or ended: its deck no longer tends the ring.
            if !self.voices[v].streams() {
                ports.drop_ring(VoiceIdx::ALL[v]);
            }
            clip[p] = clip[p].max(self.voices[v].filter_clip());
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
            //    A voice with a note waiting keeps it for step 6.
            if !self.voices[v].is_active()
                && (self.waiting[v].is_none() || self.alloc.slots()[v].note().is_none())
            {
                self.waiting[v] = None;
                self.alloc.release_finished(v);
                #[cfg(feature = "tape")]
                if self.alloc.slots()[v].is_free() {
                    self.streaming[v] = Streaming::IDLE;
                }
                self.voices[v].rest(&mut self.sym);
            }
        }
        // 6. Waiting notes whose voices went idle this block start, oldest
        //    first, behind any the budget still queues.
        self.start_waiting(shared, src, ports);
        // 7. Every Part's LFO clock steps a block, playing or not.
        self.step_lfo_clocks(shared);

        self.publish_heads();
        self.publish_lfo_view();
        crate::meter::publish_sounding_parts(self.sounding_set());
        self.clear_left = SYM_CLEAR_BUDGET;

        // 2-4.
        let scope_block = mix_parts(
            &self.buses,
            &written,
            &mut self.sends,
            &mut self.mix,
            fx,
            shared,
            self.tempo,
            self.sample_rate,
            dac,
        );
        // The MST page's GR meter and the synced reverb cells' clock:
        // relaxed stores, never block.
        crate::meter::MASTER_GR.publish(fx.master_gr_db());
        // Each Part's LEVEL LED: one atomic max, held until the UI takes it.
        for (m, c) in crate::meter::FILTER_CLIP.iter().zip(clip) {
            m.raise(c);
        }
        crate::meter::publish_rev_clock(self.tempo, fx.reverb().fold());

        // Oscilloscope: every part's bus, before pan and level.
        scope.write(&scope_block);
        // A key the readout holds that no voice does was lost (a dropped
        // note-off): let it go.
        if !self.alloc.slots().iter().any(|s| s.held() && !s.dying()) && crate::played::any_held() {
            crate::played::clear_held();
        }
        // The block is over: the next may carry other patches.
        self.costed = 0;
    }

    /// Each Part's newest sounding Algo voice's formant view (plan Ruling
    /// 8), not `sounding` where none does: the shells publish them once a
    /// block, and the OP page draws the edited Part's. Into `out`, the
    /// buffer's slot, and out of line: the audio interrupt stays small.
    #[inline(never)]
    pub fn formant_views_into(&self, out: &mut FormantViews) {
        let mut age = [None; MAX_PARTS];
        *out = [FormantView::NONE; MAX_PARTS];
        for (v, s) in self.alloc.slots().iter().enumerate() {
            if s.is_free() || !self.voices[v].is_active() {
                continue;
            }
            let p = self.sounding[v] as usize % MAX_PARTS;
            if let Some(f) = self.voices[v].formant_view()
                && age[p].is_none_or(|a| s.age() >= a)
            {
                age[p] = Some(s.age());
                out[p] = FormantView::live(f);
            }
        }
    }

    /// `formant_views_into` by value: the tests'.
    pub fn formant_views(&self) -> FormantViews {
        let mut out = [FormantView::NONE; MAX_PARTS];
        self.formant_views_into(&mut out);
        out
    }

    /// Every Part's FREE LFO clock a block on, at its stored RATEs (ADR
    /// 0135; the LFOs are not modulation targets). Never inlined: its
    /// count is `LfoClock::RESERVE`'s.
    #[inline(never)]
    fn step_lfo_clocks(&mut self, shared: &AudioShared) {
        let step = BLOCK_SIZE as f32 / self.sample_rate as f32;
        for (c, part) in self.lfo_clocks.iter_mut().zip(&shared.parts) {
            c.advance(&part.params.lfos, step);
        }
    }

    /// The LFO page's playheads (ADR 0177): the watched Part's newest
    /// sounding voice's phase in each slot. A load and three stores a
    /// block, none while no LFO page is up. Out of line: the audio
    /// interrupt stays small.
    #[inline(never)]
    fn publish_lfo_view(&self) {
        let Some(part) = LFO_VIEW.watched() else {
            return;
        };
        let mut newest = None;
        for (v, s) in self.alloc.slots().iter().enumerate() {
            if !s.is_free()
                && self.sounding[v] as usize % MAX_PARTS == part
                && self.voices[v].is_active()
                && newest.is_none_or(|(a, _)| s.age() >= a)
            {
                newest = Some((s.age(), v));
            }
        }
        LFO_VIEW.publish_all(part, newest.map(|(_, v)| self.voices[v].lfo_view_phases()));
    }

    /// The strip's heads (spec § 3): each Part's newest voice playing
    /// TAPE, none where no voice does. One store a Part a block.
    fn publish_heads(&self) {
        let mut newest = [(0, Heads::NONE); MAX_PARTS];
        for (v, s) in self.alloc.slots().iter().enumerate() {
            let h = self.voices[v].heads();
            let n = &mut newest[self.sounding[v] as usize % MAX_PARTS];
            if h != Heads::NONE && (n.1 == Heads::NONE || s.age() >= n.0) {
                *n = (s.age(), h);
            }
        }
        for (p, &(_, h)) in newest.iter().enumerate() {
            HEADS.publish(p, h);
        }
    }
}

/// A set of MIDI keys, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NoteSet(u128);

impl NoteSet {
    pub(crate) const EMPTY: NoteSet = NoteSet(0);

    pub(crate) const fn with(self, n: MidiNote) -> NoteSet {
        NoteSet(self.0 | 1 << n.get())
    }

    pub(crate) const fn without(self, n: MidiNote) -> NoteSet {
        NoteSet(self.0 & !(1 << n.get()))
    }

    pub(crate) const fn contains(self, n: MidiNote) -> bool {
        self.0 & 1 << n.get() != 0
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The lowest key in the set.
    pub(crate) fn lowest(self) -> Option<MidiNote> {
        (!self.is_empty()).then(|| MidiNote::new(self.0.trailing_zeros() as u8))?
    }
}
