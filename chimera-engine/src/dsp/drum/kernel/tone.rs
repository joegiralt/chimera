//! The tone kernel: a swept sine body, a noise layer, drive. TONE KICK,
//! SNARE and TOM (machines.rs:693–853).

use core::mem::MaybeUninit;

use crate::dsp::filter::FilterMode;
use crate::dsp::voice_fx::fold;
use crate::dsp::{Unit, fast_sin_turns};
use crate::hw::BLOCK_SIZE;
use crate::in_place::in_place_enum;

use super::super::laws::{KickParams, SnareParams, TomParams, ToneParams};
use super::super::types::{Hz, Seconds};
use super::{Blk, Decay, Hit, NoiseLayer, Swept, drive, quiet};

in_place_enum! {
    pub enum Tone {
        Kick(Kick) => rebuild_kick, init_kick;
        Snare(Snare) => rebuild_snare, init_snare;
        Tom(Tom) => rebuild_tom, init_tom;
    }
}

impl Tone {
    pub(super) fn init_in_place(slot: &mut MaybeUninit<Tone>, p: ToneParams, h: Hit) -> &mut Tone {
        // SAFETY: each `write` fills its whole payload.
        unsafe {
            match p {
                ToneParams::Kick(p) => Tone::init_kick(slot, |s| s.write(Kick::new(p, h))),
                ToneParams::Snare(p) => Tone::init_snare(slot, |s| s.write(Snare::new(p, h))),
                ToneParams::Tom(p) => Tone::init_tom(slot, |s| s.write(Tom::new(p, h))),
            }
        }
    }

    pub(super) fn render(&mut self, out: &mut Blk) {
        match self {
            Tone::Kick(k) => k.render(out),
            Tone::Snare(k) => k.render(out),
            Tone::Tom(k) => k.render(out),
        }
    }

    pub(super) fn finished(&self) -> bool {
        match self {
            Tone::Kick(k) => k.quiet && k.amp.spent() && k.click.spent() && k.blip.spent(),
            Tone::Snare(k) => k.quiet && k.amp.spent() && k.amp_hi.spent() && k.snap.spent(),
            Tone::Tom(k) => k.quiet && k.slow.spent() && k.fast.spent() && k.noise.spent(),
        }
    }
}

/// `ToneKick`, machines.rs:693.
pub struct Kick {
    body: Swept,
    amp: Decay,
    click: NoiseLayer,
    blip: Decay,
    drive: f32,
    harm: f32,
    vel: f32,
    quiet: bool,
}

impl Kick {
    fn new(p: KickParams, h: Hit) -> Kick {
        let mut body = Swept::of(p.body, h.sr);
        // START: the body begins up to a quarter cycle in, a harder edge.
        body.ph = 0.25 * p.start.get();
        Kick {
            body,
            amp: Decay::t60(p.decay, h.sr),
            click: NoiseLayer::new(
                h.seed,
                Seconds::new(0.006),
                FilterMode::Hp24,
                Hz::new(2500.0),
                Unit::new(0.1),
                p.click,
                h.sr,
            ),
            blip: Decay::t60(Seconds::new(0.004), h.sr),
            drive: p.drive.get(),
            harm: p.harmonics.get(),
            vel: h.level.get(),
            quiet: false,
        }
    }

    #[inline(never)]
    fn render(&mut self, out: &mut Blk) {
        let mut c = [0.0; BLOCK_SIZE];
        self.click.block(&mut c);
        let blip = 0.5 * self.click.level.get();
        let (mut body, mut ph) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        self.body.block(&mut body, Some(&mut ph));
        for (i, o) in out.iter_mut().enumerate() {
            let b = body[i];
            // HARMONICS: the second and third partials folded in.
            let h = self.harm
                * (0.5 * fast_sin_turns(2.0 * ph[i]) + 0.25 * fast_sin_turns(3.0 * ph[i]));
            let x = 0.9 * (b + h) * self.amp.next() + c[i] + blip * self.blip.next();
            *o = 0.8 * self.vel * drive(x, self.drive);
        }
        self.quiet = quiet(out);
    }
}

/// `ToneSnare`, machines.rs:744.
pub struct Snare {
    lo: Swept,
    hi: Swept,
    amp: Decay,
    amp_hi: Decay,
    snap: NoiseLayer,
    drive: f32,
    vel: f32,
    quiet: bool,
}

impl Snare {
    fn new(p: SnareParams, h: Hit) -> Snare {
        // Both heads' bump falls in 8 ms.
        let tau = Seconds::new(0.008);
        Snare {
            lo: Swept::new(p.pitch, p.bump, tau, h.sr),
            hi: Swept::new(Hz::new(p.pitch.get() * p.interval.get()), p.bump, tau, h.sr),
            amp: Decay::t60(p.decay, h.sr),
            amp_hi: Decay::t60(Seconds::new(0.6 * p.decay.get()), h.sr),
            snap: NoiseLayer::new(
                h.seed,
                p.snap_decay,
                FilterMode::Bp12,
                p.tone,
                Unit::ZERO,
                p.snap,
                h.sr,
            ),
            drive: p.drive.get(),
            vel: h.level.get(),
            quiet: false,
        }
    }

    #[inline(never)]
    fn render(&mut self, out: &mut Blk) {
        let mut n = [0.0; BLOCK_SIZE];
        self.snap.block(&mut n);
        let (mut lo, mut hi) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        self.lo.block(&mut lo, None);
        self.hi.block(&mut hi, None);
        for (i, o) in out.iter_mut().enumerate() {
            let b = 0.5 * lo[i] * self.amp.next() + 0.3 * hi[i] * self.amp_hi.next();
            let x = b + n[i];
            *o = self.vel
                * if self.drive > 0.0 {
                    0.8 * drive(x, self.drive)
                } else {
                    x
                };
        }
        self.quiet = quiet(out);
    }
}

/// `ToneTom`, machines.rs:797.
pub struct Tom {
    body: Swept,
    slow: Decay,
    fast: Decay,
    damp: f32,
    noise: NoiseLayer,
    drive: f32,
    hard: f32,
    vel: f32,
    quiet: bool,
}

impl Tom {
    fn new(p: TomParams, h: Hit) -> Tom {
        Tom {
            body: Swept::of(p.body, h.sr),
            slow: Decay::t60(p.decay, h.sr),
            fast: Decay::t60(Seconds::new(0.2 * p.decay.get()), h.sr),
            damp: p.damp.get(),
            noise: NoiseLayer::new(
                h.seed,
                Seconds::new(0.05),
                FilterMode::Lp12,
                p.noise_hz,
                Unit::ZERO,
                p.noise,
                h.sr,
            ),
            drive: p.drive.get(),
            hard: p.drive_type.get(),
            vel: h.level.get(),
            quiet: false,
        }
    }

    #[inline(never)]
    fn render(&mut self, out: &mut Blk) {
        let mut n = [0.0; BLOCK_SIZE];
        self.noise.block(&mut n);
        let mut body = [0.0; BLOCK_SIZE];
        self.body.block(&mut body, None);
        for (i, o) in out.iter_mut().enumerate() {
            let a = (1.0 - self.damp) * self.slow.next() + self.damp * self.fast.next();
            let x = 0.85 * body[i] * a + n[i];
            // DRIVE through SOFT; DRIVE TYPE leans it into FOLD's harder edge.
            *o = self.vel
                * if self.drive > 0.0 {
                    let s = 0.8 * drive(x, self.drive);
                    let f = 0.7
                        * fold::curve(
                            x * (1.0 + 2.0 * self.drive),
                            [0.3 + 0.7 * self.drive, 0.5, fold::UNITY_LEVEL],
                        );
                    s + self.hard * (f - s)
                } else {
                    x
                };
        }
        self.quiet = quiet(out);
    }
}
