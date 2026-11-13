//! DRUM's engine (ADR 0265): a voice slot playing one hit. A hit only
//! starts from a `DrumHit` (`strike`); a plain note never starts one.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_midi::value::U7;

use crate::Velocity;
use crate::dsp::modal::ModalEngine;
use crate::hw::BLOCK_SIZE;

use super::kernel::KernelState;
use super::laws::resolve;
use super::types::{DrumHit, HitSeed, Pan, SampleRate};

/// A pad's PAN off centre, as the gains of the voice's mid (its Part's
/// mono bus) and side: constant power, unity at centre, as the Part's own
/// PAN (`instrument::pan_gains`), its √2 the centre's 3 dB. Worked out at
/// the strike: two `sinf` a hit, not a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MidSide {
    mid: f32,
    side: f32,
}

impl MidSide {
    /// `None` at centre: the bus alone, bit for bit.
    pub fn of(pan: Pan) -> Option<MidSide> {
        if pan == Pan::CENTRE {
            return None;
        }
        let t = f32::from(pan.get()) / 64.0;
        let q = core::f32::consts::FRAC_PI_4;
        let k = core::f32::consts::SQRT_2;
        let (l, r) = (k * libm::sinf((1.0 - t) * q), k * libm::sinf((1.0 + t) * q));
        Some(MidSide {
            mid: (l + r) / 2.0,
            side: (r - l) / 2.0,
        })
    }

    /// `x` to its mid, in place, its side into `side` (written if empty):
    /// left is mid − side, right mid + side.
    pub fn split(self, x: &mut [f32; BLOCK_SIZE], side: &mut Option<[f32; BLOCK_SIZE]>) {
        let s = side.get_or_insert([0.0; BLOCK_SIZE]);
        for (x, s) in x.iter_mut().zip(s) {
            *s += *x * self.side;
            *x *= self.mid;
        }
    }
}

/// The voice's drum: idle, or one hit, its kernel in place.
pub struct DrumEngine {
    /// What was struck, while a hit sounds: `kernel` and `pan` are its.
    hit: Option<DrumHit>,
    kernel: KernelState,
    pan: Option<MidSide>,
}

crate::in_place::field_list!(DrumEngine => DrumEngine { hit, kernel, pan });

// A hit plays in a voice slot, which MODAL's engine sizes.
const _: () = assert!(size_of::<DrumEngine>() <= size_of::<ModalEngine>());

impl DrumEngine {
    /// Idle: no hit.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every field (`field_list!`
        // above) is written once, the kernel in place, before
        // `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).hit).write(None);
            KernelState::init_in_place(crate::in_place::uninit_at(addr_of_mut!((*p).kernel)));
            addr_of_mut!((*p).pan).write(None);
            slot.assume_init_mut()
        }
    }

    /// `hit` at `vel`, cutting what played: its pad's machine and knobs,
    /// `hit.pitch()` semitones over the pad's PITCH.
    #[inline(never)]
    #[optimize(size)]
    pub fn strike(&mut self, hit: DrumHit, vel: Velocity, seed: HitSeed, sr: SampleRate) {
        let pad = hit.params();
        let p = resolve(pad.machine(), pad.knobs()).transposed(hit.pitch());
        self.hit = Some(hit);
        self.kernel.strike_in_place(p, vel, seed, sr);
        self.pan = MidSide::of(pad.pan());
    }

    /// The hit sounding, if one is.
    pub fn hit(&self) -> Option<DrumHit> {
        self.hit
    }

    /// The hit's next block at its pad's LEVEL (k / 127, before the
    /// filter: the spec's MACHINE ─ LEVEL); silence once it has ended.
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE]) {
        match self.hit {
            Some(hit) if !self.kernel.finished() => {
                self.kernel.render(out);
                let level = hit.params().level();
                if level != U7::MAX {
                    let g = f32::from(level.get()) / 127.0;
                    out.iter_mut().for_each(|x| *x *= g);
                }
            }
            _ => out.fill(0.0),
        }
    }

    /// The hit's PAN, off centre.
    pub fn pan(&self) -> Option<MidSide> {
        self.hit.and(self.pan)
    }

    pub fn is_active(&self) -> bool {
        self.hit.is_some() && !self.kernel.finished()
    }
}
