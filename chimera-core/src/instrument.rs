//! The playable instrument (instrument-core spec § Audio path, § Threading):
//! what the audio thread reads from the UI, and the voice pool that renders
//! every Part into the three DAC pairs.

use core::mem::{MaybeUninit, size_of};
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::MidiChannel;
use crate::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use crate::dsp::voice::Voice;
use crate::hw::{
    AXI_SRAM, DAC_PAIRS, FB_BYTES, MAX_PARTS, MAX_VOICES, SampleBudget, UI_RESERVE,
    VOICE_RAM_BUDGET,
};
use crate::in_place::{by_value, uninit_at};
use crate::modulation::ModState;
use crate::note_queue::{NoteEvent, NoteKind};
use crate::params::ParamSnapshot;
use crate::part::PartParams;
use crate::perf::load::AudioStats;
use crate::preset::{Performance, SoundPool};
use crate::scope::{ScopeFrame, ScopeWriter};
use crate::triple::TripleBuffer;
use crate::voice_alloc::{Alloc, Allocator};

/// Everything the port places in AXI SRAM (ADR 0014): framebuffer, UI,
/// Performance, SoundPool, the `AudioShared`, scope and `AudioStats` triple
/// buffers (the scope's writer besides), and the FX bus.
pub const AXI_RESIDENT: usize = FB_BYTES
    + UI_RESERVE
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

/// The Performance state the audio needs, double-buffered by the platform
/// (one pointer swap per UI frame). The Sound names, pool and UI stay behind.
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

impl AudioShared {
    pub fn from_performance(perf: &Performance) -> Self {
        Self {
            parts: core::array::from_fn(|i| {
                let p = &perf.parts[i];
                PartAudio {
                    params: p.sound.params.clone(),
                    mod_state: p.sound.mod_state.clone(),
                    mix: p.mix,
                }
            }),
            fx: perf.fx,
        }
    }

    /// Overwrite with `perf` (the UI's per-frame refresh of the back
    /// buffer). Built through `from_performance` so there is exactly one
    /// place that lists `AudioShared`'s fields; the fresh copy is a stack
    /// temporary (~3 KB) that replaces `*self` in one move, never the heap.
    /// Callers must only ever run this on the back buffer — never on the
    /// copy the audio thread is currently reading.
    pub fn update_from(&mut self, perf: &Performance) {
        *self = Self::from_performance(perf);
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

/// The shared voice pool and the per-block mix. The FX bus is passed to
/// `render` rather than owned: on hardware the pool is placed in D2 and the
/// FX bus in AXI (ADR 0014).
///
/// Audio-thread contract:
/// - The Instrument is the note queue's only consumer: the audio thread pops
///   every `NoteEvent` and feeds it to `handle` (the queue is single-producer,
///   single-consumer; nothing else may pop it).
/// - The `&AudioShared` passed to `handle` and `render` is the front buffer.
///   The UI's `AudioShared::update_from` must only ever run on the back
///   buffer, never on the one borrowed here; the platform swaps them between
///   blocks.
pub struct Instrument {
    voices: [Voice; MAX_VOICES],
    alloc: Allocator,
    /// The MIDI channel each voice's note-on arrived on, so its note-off
    /// releases it even if the Part's channel changed meanwhile.
    note_channel: [MidiChannel; MAX_VOICES],
    /// Each Part's mono bus from the last `render`: the sum of its voices.
    buses: [[f32; BLOCK_SIZE]; MAX_PARTS],
    sends: [[f32; BLOCK_SIZE]; FX_SENDS],
    sample_rate: u32,
}

crate::in_place::field_list!(Instrument => Instrument { voices, alloc, note_channel, buses, sends, sample_rate });

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
        // SAFETY: `p` is valid and unaliased; the six voices are built in
        // place and the rest (the largest, `buses`, is 1.5 KB) written once
        // by value before `assume_init_mut`.
        unsafe {
            let voices = addr_of_mut!((*p).voices).cast::<Voice>();
            for v in 0..MAX_VOICES {
                Voice::init_in_place(uninit_at(voices.add(v)), sample_rate);
            }
            addr_of_mut!((*p).alloc).write(Allocator::new(budget));
            addr_of_mut!((*p).note_channel).write([MidiChannel::clamped(0); MAX_VOICES]);
            addr_of_mut!((*p).buses).write([[0.0; BLOCK_SIZE]; MAX_PARTS]);
            addr_of_mut!((*p).sends).write([[0.0; BLOCK_SIZE]; FX_SENDS]);
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            slot.assume_init_mut()
        }
    }

    pub fn allocator(&self) -> &Allocator {
        &self.alloc
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
                for (p, part) in shared.parts.iter().enumerate() {
                    if part.mix.channel != ev.channel {
                        continue;
                    }
                    let cost = Voice::cost(part.params.engine());
                    if let Alloc::Voice(v) =
                        self.alloc
                            .note_on(p as u8, part.mix.mode, ev.note, cost, FxBus::COST)
                    {
                        self.voices[v].note_on(ev.note, vel, &part.params);
                        self.note_channel[v] = ev.channel;
                    }
                }
            }
            NoteKind::Off => {
                // By voice index: every held voice this channel started on
                // this note (one per layered Part), and no other.
                for v in 0..MAX_VOICES {
                    let s = self.alloc.slots()[v];
                    if s.held() && s.note() == Some(ev.note) && self.note_channel[v] == ev.channel {
                        self.alloc.release(v);
                        self.voices[v].note_off();
                    }
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
        // A Sound that changed engine changes its voices' cost; cut the
        // newest voices if that went over the budget.
        for v in 0..MAX_VOICES {
            if let Some(p) = self.alloc.slots()[v].part() {
                self.alloc
                    .recost(v, Voice::cost(shared.parts[p as usize].params.engine()));
            }
        }
        // A hard cut, not a release: the slot is free at once and the voice
        // is no longer rendered, so its tail stops mid-block. The next
        // note-on on it re-triggers the engine from scratch.
        while let Some(v) = self.alloc.shed(FxBus::COST) {
            self.voices[v].note_off();
        }

        // 1. Voices into their part's mono bus. The first voice of a part is
        //    copied, not added, so a lone voice reaches the bus bit-for-bit.
        let mut written = [false; MAX_PARTS];
        for bus in self.buses.iter_mut() {
            bus.fill(0.0);
        }
        let mut block = [0.0f32; BLOCK_SIZE];
        for v in 0..MAX_VOICES {
            let Some(p) = self.alloc.slots()[v].part() else {
                continue;
            };
            let (p, part) = (p as usize, &shared.parts[p as usize]);
            self.voices[v].render(&mut block, &part.params, &part.mod_state);
            if written[p] {
                for (b, &s) in self.buses[p].iter_mut().zip(&block) {
                    *b += s;
                }
            } else {
                self.buses[p] = block;
                written[p] = true;
            }
            // 5. A released voice whose engine went quiet is free again.
            //    Read right after rendering the note the voice plays *now*,
            //    so a steal or retrigger since the last block is never freed
            //    by a report about the note it replaced.
            if !self.voices[v].is_active() {
                self.alloc.release_finished(v);
            }
        }

        // 2-3. Pan and level into the part's pair; sends into the FX bus.
        for pair in out.iter_mut() {
            pair.fill(0.0);
        }
        for send in self.sends.iter_mut() {
            send.fill(0.0);
        }
        let mut scope_block = [0.0f32; BLOCK_SIZE];
        for (p, part) in shared.parts.iter().enumerate() {
            // A part with no voices this block has a silent bus: nothing to add.
            if !written[p] {
                continue;
            }
            let bus = &self.buses[p];
            let (gl, gr) = pan_gains(part.mix.pan);
            let (gl, gr) = (gl * part.mix.level, gr * part.mix.level);
            let pair = &mut out[part.mix.output.index()];
            for i in 0..BLOCK_SIZE {
                pair[2 * i] += bus[i] * gl;
                pair[2 * i + 1] += bus[i] * gr;
                scope_block[i] += bus[i];
            }
            for (send, &amount) in self.sends.iter_mut().zip(&part.mix.sends) {
                for (s, &b) in send.iter_mut().zip(bus) {
                    *s += b * amount;
                }
            }
        }

        // 4. The FX bus once; its return lands on both sides of pair 1.
        let mut ret = [0.0f32; BLOCK_SIZE];
        fx.process(&mut self.sends, &shared.fx, self.sample_rate, &mut ret);
        for (i, &r) in ret.iter().enumerate() {
            out[0][2 * i] += r;
            out[0][2 * i + 1] += r;
        }

        // Oscilloscope: every part's bus, before pan and level.
        scope.write(&scope_block);
    }
}
