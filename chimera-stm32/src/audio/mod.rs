pub mod dma;
pub mod sai;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::audio_out::{Half, interleave};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::instrument::{DacOut, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::part::DacPair;
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::Writer;
use chimera_hal::{MidiNote, Velocity};

// SAFETY: never read or written before Task 15, which builds it in place
// under the audio interrupt's single-owner discipline.
#[used]
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();

static mut WORK: [f32; BLOCK_SIZE] = [0.0; BLOCK_SIZE];
static mut DAC: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
// `VOICE_READY` is set only after `VOICE` is fully built in place.
static mut VOICE: MaybeUninit<Voice> = MaybeUninit::uninit();
static mut VOICE_READY: bool = false;
static mut PARAMS: Option<*const ParamSnapshot> = None;
static mut MOD_STATE_PTR: Option<*const ModState> = None;
static DEFAULT_MOD_STATE: ModState = ModState::new();
static mut SCOPE_WRITER: Option<ScopeWriter> = None;

pub fn init_scope(w: Writer<ScopeFrame>) {
    // SAFETY: called once from `main` before the DMA interrupt is unmasked;
    // nothing else touches `SCOPE_WRITER` yet.
    unsafe { addr_of_mut!(SCOPE_WRITER).write(Some(ScopeWriter::new(w))) };
}

pub fn render_half(half: Half) {
    // SAFETY: only the DMA1 stream 0 interrupt and `prefill` (before that
    // interrupt is unmasked) call this, never concurrently; they are the only
    // users of these statics after init. `VOICE` is read only once
    // `VOICE_READY` says it is initialised.
    unsafe {
        let work = &mut *addr_of_mut!(WORK);
        let dac = &mut *addr_of_mut!(DAC);
        match (*addr_of_mut!(VOICE_READY), *addr_of_mut!(PARAMS)) {
            (true, Some(params)) => {
                let mod_state = match *addr_of_mut!(MOD_STATE_PTR) {
                    Some(p) => &*p,
                    None => &DEFAULT_MOD_STATE,
                };
                (*addr_of_mut!(VOICE))
                    .assume_init_mut()
                    .render(work, &*params, mod_state);
            }
            _ => work.fill(0.0),
        }
        if let Some(s) = (*addr_of_mut!(SCOPE_WRITER)).as_mut() {
            s.write(work);
        }
        for (i, &s) in work.iter().enumerate() {
            dac[0][2 * i] = s;
            dac[0][2 * i + 1] = s;
        }
        interleave(dac, DacPair::P1, dma::half_mut(DacPair::P1, half));
    }
}

pub fn prefill() {
    render_half(Half::First);
    render_half(Half::Second);
}

/// # Safety
/// `params_ptr` and `mod_ptr` must outlive the audio system.
pub unsafe fn init_voice(params_ptr: *const ParamSnapshot, mod_ptr: *const ModState) {
    // SAFETY: called once during single-threaded init before the ISR is active.
    unsafe {
        Voice::init_in_place(&mut *addr_of_mut!(VOICE), chimera_hal::SAMPLE_RATE);
        addr_of_mut!(VOICE_READY).write(true);
        addr_of_mut!(PARAMS).write(Some(params_ptr));
        addr_of_mut!(MOD_STATE_PTR).write(Some(mod_ptr));
    }
}

pub fn trigger_note(note: MidiNote, velocity: Velocity) {
    // SAFETY: called during init before the ISR is active; `VOICE` is read
    // only once `VOICE_READY` says it is initialised.
    unsafe {
        if let (true, Some(p)) = (*addr_of_mut!(VOICE_READY), *addr_of_mut!(PARAMS)) {
            (*addr_of_mut!(VOICE))
                .assume_init_mut()
                .note_on(note, velocity, &*p);
        }
    }
}
