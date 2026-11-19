//! The metal kernel: VA's squares through CLEAN band-passes. TONE COWBELL,
//! TONE CYMBAL and METAL HAT (the bank), METAL JINGLE (the bank's squares
//! gated by burst particles); machines.rs:913–1197.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::dsp::filter::FilterMode;
use crate::dsp::{Unit, fast_sin_turns};
use crate::hw::BLOCK_SIZE;
use crate::in_place::{field_list, in_place_enum};

use super::super::laws::{BankParams, CowbellParams, JingleParams, MetalParams};
use super::super::types::{Hz, PulsePace, SampleRate, Seconds, Semis};
use super::{
    Blk, Decay, Hiss, Hit, Lcg, Octaves, Pulse, Square, Svf, SweepState, Swept, drive, octaves,
    per_sample, quiet, sweeping,
};

in_place_enum! {
    pub enum Metal {
        Cowbell(Cowbell) => rebuild_cowbell, init_cowbell;
        Bank(Bank) => rebuild_bank, init_bank;
        Jingle(Jingle) => rebuild_jingle, init_jingle;
    }
}

impl Metal {
    pub(super) fn init_in_place(
        slot: &mut MaybeUninit<Metal>,
        p: MetalParams,
        h: Hit,
    ) -> &mut Metal {
        // SAFETY: the `write` fills its whole payload; the bank's and the
        // jingle's `init_in_place` write every field.
        unsafe {
            match p {
                MetalParams::Cowbell(p) => {
                    Metal::init_cowbell(slot, |s| s.write(Cowbell::new(p, h)))
                }
                MetalParams::Bank(p) => Metal::init_bank(slot, |s| Bank::init_in_place(s, p, h)),
                MetalParams::Jingle(p) => {
                    Metal::init_jingle(slot, |s| Jingle::init_in_place(s, p, h))
                }
            }
        }
    }

    /// The next block; `TICKED`, the squares a sample at a time
    /// (`Square::add`, `Pulse::sweep`).
    pub(super) fn render<const TICKED: bool>(&mut self, out: &mut Blk) {
        match self {
            Metal::Cowbell(k) => k.render::<TICKED>(out),
            Metal::Bank(k) => k.render::<TICKED>(out),
            Metal::Jingle(k) => k.render::<TICKED>(out),
        }
    }

    /// `KernelState::pace`: the cowbell's pulses swept while BUMP moves
    /// them; the bank's squares always run.
    pub(super) fn pace(&self) -> PulsePace {
        match self {
            Metal::Cowbell(k) => k.pace(),
            Metal::Bank(_) | Metal::Jingle(_) => PulsePace::Runs,
        }
    }

    pub(super) fn finished(&self) -> bool {
        match self {
            Metal::Cowbell(k) => k.quiet && k.slow.spent() && k.fast.spent(),
            Metal::Bank(k) => k.quiet && k.amp.spent() && k.fast.spent(),
            Metal::Jingle(k) => {
                k.quiet && k.n as f32 >= k.len && k.env < super::FLOOR && k.head_amp.spent()
            }
        }
    }
}

/// `Cowbell`, machines.rs:1052: two of VA's pulses, a band-pass and SOFT.
pub struct Cowbell {
    p: [Pulse; 2],
    hz: [f32; 2],
    bp: Svf,
    slow: Decay,
    fast: Decay,
    drive: f32,
    damp: f32,
    bump: Decay,
    bump_oct: Octaves,
    vel: f32,
    quiet: bool,
}

impl Cowbell {
    fn new(p: CowbellParams, h: Hit) -> Cowbell {
        let f = p.pitch.get();
        Cowbell {
            p: [Pulse::new(Unit::ZERO, h.sr), Pulse::new(Unit::ZERO, h.sr)],
            hz: [f, f * p.interval.get()],
            bp: Svf::new(FilterMode::Bp12, p.tone, p.ring, h.sr),
            slow: Decay::t60(p.decay, h.sr),
            fast: Decay::t60(Seconds::new(0.05), h.sr),
            drive: p.drive.get(),
            damp: p.damp.get(),
            bump: Decay::tau(Seconds::new(0.006), h.sr),
            bump_oct: Octaves::of_unit(p.bump),
            vel: h.level.get(),
            quiet: false,
        }
    }

    /// Its next block's pulses: swept while BUMP still moves them.
    /// Squares (SHAPE 0) never floor (`MAX_INC` < ½).
    fn pace(&self) -> PulsePace {
        let sweep = sweeping(self.bump_oct, self.bump.fall());
        PulsePace::of(sweep == SweepState::Moving, false)
    }

    #[inline(never)]
    fn render<const TICKED: bool>(&mut self, out: &mut Blk) {
        let mut x = [0.0; BLOCK_SIZE];
        // BUMP: up to an octave above, falling in 6 ms.
        octaves(&mut self.bump, self.bump_oct, &mut x);
        let (a, b) = (&mut [0.0; BLOCK_SIZE], &mut [0.0; BLOCK_SIZE]);
        self.p[0].sweep::<TICKED>(self.hz[0], &x, a);
        self.p[1].sweep::<TICKED>(self.hz[1], &x, b);
        for (s, (a, b)) in x.iter_mut().zip(a.iter().zip(b.iter())) {
            *s = 0.5 * (a + b);
        }
        self.bp.run(&mut x);
        for (o, v) in out.iter_mut().zip(&x) {
            let a = self.damp * self.fast.next() + (1.0 - self.damp) * self.slow.next();
            *o = 0.8 * self.vel * drive(1.6 * v * a, self.drive);
        }
        self.quiet = quiet(out);
    }
}

/// A band-pass of the bank and its weight.
struct Bp {
    f: Svf,
    w: f32,
}

/// `Metal`, machines.rs:921: six squares and band-passes; HAT and CYMBAL.
pub struct Bank {
    sq: [Square; 6],
    hz: [f32; 6],
    bps: [Option<Bp>; 3],
    hp: Svf,
    lp: Option<Svf>,
    amp: Decay,
    fast: Decay,
    hiss: Hiss,
    noise: f32,
    shimmer: f32,
    t: f32,
    sr: SampleRate,
    vel: f32,
    gain: f32,
    quiet: bool,
}

field_list!(Bank => Bank { sq, hz, bps, hp, lp, amp, fast, hiss, noise, shimmer, t, sr, vel, gain, quiet });

impl Bank {
    /// In place, a part at a time: 2 KB never crosses the stack.
    fn init_in_place(slot: &mut MaybeUninit<Bank>, p: BankParams, h: Hit) -> &mut Bank {
        let k = slot.as_mut_ptr();
        // SAFETY: `k` is valid and unaliased; every field (`field_list!`
        // above) is written once, the arrays an element at a time, before
        // `assume_init_mut`.
        unsafe {
            let sq = addr_of_mut!((*k).sq).cast::<Square>();
            for i in 0..6 {
                sq.add(i).write(Square::new(h.sr));
            }
            addr_of_mut!((*k).hz).write(p.hz.map(|f| f.get()));
            let bps = addr_of_mut!((*k).bps).cast::<Option<Bp>>();
            for (i, b) in p.bands.into_iter().enumerate() {
                bps.add(i).write(b.map(|b| Bp {
                    f: Svf::new(FilterMode::Bp12, b.hz, b.res, h.sr),
                    w: b.weight.get(),
                }));
            }
            addr_of_mut!((*k).hp).write(Svf::new(FilterMode::Hp24, p.hp, Unit::ZERO, h.sr));
            addr_of_mut!((*k).lp)
                .write(p.lp.map(|hz| Svf::new(FilterMode::Lp12, hz, Unit::ZERO, h.sr)));
            addr_of_mut!((*k).amp).write(Decay::t60(p.decay, h.sr));
            addr_of_mut!((*k).fast).write(Decay::t60(p.fast, h.sr));
            addr_of_mut!((*k).hiss).write(Hiss::new(h.seed));
            addr_of_mut!((*k).noise).write(p.noise.get());
            addr_of_mut!((*k).shimmer).write(p.shimmer.get());
            addr_of_mut!((*k).t).write(0.0);
            addr_of_mut!((*k).sr).write(h.sr);
            addr_of_mut!((*k).vel).write(h.level.get());
            addr_of_mut!((*k).gain).write(p.gain.get());
            addr_of_mut!((*k).quiet).write(false);
            slot.assume_init_mut()
        }
    }

    #[inline(never)]
    fn render<const TICKED: bool>(&mut self, out: &mut Blk) {
        let mut n = [0.0; BLOCK_SIZE];
        self.hiss.block(&mut n);
        // SHIMMER: two slow LFOs detune the squares (±1.5 %) and sway the
        // band-passes' weights; once a block, so a square's pitch holds a
        // block and it renders a block at a time.
        self.t += BLOCK_SIZE as f32 / self.sr.hz();
        let l1 = fast_sin_turns(4.3 * self.t);
        let l2 = fast_sin_turns(6.1 * self.t + 0.3);
        let det = 0.015 * self.shimmer;
        let (mut x, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        for (j, (o, &hz)) in self.sq.iter_mut().zip(&self.hz).enumerate() {
            let d = if j % 2 == 0 { l1 } else { l2 };
            o.add::<TICKED>(hz * (1.0 + det * d), &mut x, &mut b);
        }
        for (s, v) in x.iter_mut().zip(&n) {
            *s = *s * (1.0 / 6.0) + self.noise * v;
        }
        let mut y = [0.0; BLOCK_SIZE];
        for (j, bp) in self.bps.iter_mut().enumerate() {
            let Some(Bp { f, w }) = bp else { continue };
            let mut b = x;
            f.run(&mut b);
            let sway = 1.0 + 0.5 * self.shimmer * if j % 2 == 0 { l1 } else { l2 };
            for (o, v) in y.iter_mut().zip(&b) {
                *o += *w * sway * v;
            }
        }
        self.hp.run(&mut y);
        if let Some(f) = &mut self.lp {
            f.run(&mut y);
        }
        for (o, v) in out.iter_mut().zip(&y) {
            let a = 0.8 * self.amp.next() + 0.2 * self.fast.next();
            *o = self.vel * self.gain * v * a;
        }
        self.quiet = quiet(out);
    }
}

/// `Jingle`, machines.rs:1105: the bank's squares, rung higher and
/// narrower, gated by burst particles (each a jingle pair colliding); HEAD
/// a frame drum's thump under them.
pub struct Jingle {
    sq: [Square; 6],
    hz: [f32; 6],
    bps: [Bp; 2],
    hp: Svf,
    rng: Lcg,
    hiss: Hiss,
    /// Particles a sample at the shake's peak, its length (samples) and
    /// the peak's place (0..1).
    density: f32,
    len: f32,
    peak: f32,
    /// The particles' envelope and its fall a sample.
    env: f32,
    env_k: f32,
    head: Swept,
    head_amp: Decay,
    head_level: f32,
    n: u32,
    vel: f32,
    quiet: bool,
}

field_list!(Jingle => Jingle { sq, hz, bps, hp, rng, hiss, density, len, peak, env, env_k, head, head_amp, head_level, n, vel, quiet });

impl Jingle {
    /// In place, a part at a time, as the bank.
    fn init_in_place(slot: &mut MaybeUninit<Jingle>, p: JingleParams, h: Hit) -> &mut Jingle {
        let fs = h.sr.hz();
        let k = slot.as_mut_ptr();
        // SAFETY: `k` is valid and unaliased; every field (`field_list!`
        // above) is written once, the arrays an element at a time, before
        // `assume_init_mut`.
        unsafe {
            let sq = addr_of_mut!((*k).sq).cast::<Square>();
            for i in 0..6 {
                sq.add(i).write(Square::new(h.sr));
            }
            addr_of_mut!((*k).hz).write(p.hz.map(|f| f.get()));
            let bps = addr_of_mut!((*k).bps).cast::<Bp>();
            bps.write(Bp {
                f: Svf::new(FilterMode::Bp12, p.tone, p.res, h.sr),
                w: 0.6,
            });
            bps.add(1).write(Bp {
                f: Svf::new(FilterMode::Bp12, p.top, p.res, h.sr),
                w: 0.4,
            });
            addr_of_mut!((*k).hp).write(Svf::new(
                FilterMode::Hp24,
                Hz::new(3000.0),
                Unit::ZERO,
                h.sr,
            ));
            addr_of_mut!((*k).rng).write(Lcg::new(h.seed, 0x7A3B));
            addr_of_mut!((*k).hiss).write(Hiss::new(h.seed));
            addr_of_mut!((*k).density).write(p.jingles.get() / fs);
            addr_of_mut!((*k).len).write(p.length.get() * fs);
            addr_of_mut!((*k).peak).write(p.shake.get());
            addr_of_mut!((*k).env).write(1.0);
            addr_of_mut!((*k).env_k).write(per_sample(p.particle, h.sr));
            addr_of_mut!((*k).head).write(Swept::new(
                p.head_pitch,
                Semis::new(3.0),
                Seconds::new(0.01),
                h.sr,
            ));
            addr_of_mut!((*k).head_amp).write(Decay::t60(Seconds::new(0.12), h.sr));
            addr_of_mut!((*k).head_level).write(p.head.get());
            addr_of_mut!((*k).n).write(0);
            addr_of_mut!((*k).vel).write(h.level.get());
            addr_of_mut!((*k).quiet).write(false);
            slot.assume_init_mut()
        }
    }

    #[inline(never)]
    fn render<const TICKED: bool>(&mut self, out: &mut Blk) {
        let mut nz = [0.0; BLOCK_SIZE];
        self.hiss.block(&mut nz);
        // the squares' sum, a block at a time (their pitches hold)
        let (mut x, mut b) = ([0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE]);
        for (o, &hz) in self.sq.iter_mut().zip(&self.hz) {
            o.add::<TICKED>(hz, &mut x, &mut b);
        }
        for (i, s) in x.iter_mut().enumerate() {
            let u = self.n as f32 / self.len;
            let shape = if u < self.peak {
                0.4 + 0.6 * u / self.peak
            } else if u < 1.0 {
                let d = (1.0 - u) / (1.0 - self.peak);
                d * d
            } else {
                0.0
            };
            if self.rng.unit() < self.density * shape {
                self.env = (self.env + 0.3 + 0.5 * self.rng.unit()).min(1.5);
            }
            self.env *= self.env_k;
            *s = self.env * (*s * (1.0 / 6.0) + 0.3 * nz[i]);
            self.n = self.n.saturating_add(1);
        }
        let mut y = [0.0; BLOCK_SIZE];
        for Bp { f, w } in self.bps.iter_mut() {
            let mut b = x;
            f.run(&mut b);
            for (o, v) in y.iter_mut().zip(&b) {
                *o += *w * v;
            }
        }
        self.hp.run(&mut y);
        // The particles' noise is spent: its block takes HEAD's.
        let head = &mut nz;
        self.head.block(head, None);
        for ((o, v), t) in out.iter_mut().zip(&y).zip(head.iter()) {
            let h = self.head_level * t * self.head_amp.next();
            *o = self.vel * (0.5 * v + h);
        }
        self.quiet = quiet(out);
    }
}

#[cfg(test)]
mod tests {
    use chimera_midi::value::U7;

    use super::super::super::laws::{KernelParams, MetalParams, resolve};
    use super::super::super::types::{
        HitSeed, Knob, KnobSet, KnobValue, Machine, PulsePace, SampleRate,
    };
    use super::super::{Hit, octaves};
    use super::{BLOCK_SIZE, Cowbell, Unit};

    /// A COWBELL at BUMP `bump` (0..=127).
    fn bell(bump: u8) -> Cowbell {
        let mut k = KnobSet::defaults(Machine::ToneCowbell);
        k.set(Knob::H, KnobValue::new(U7::new(bump).expect("0..=127")));
        let KernelParams::Metal(MetalParams::Cowbell(p)) = resolve(Machine::ToneCowbell, &k) else {
            panic!("COWBELL resolves to its kernel");
        };
        let h = Hit {
            level: Unit::ONE,
            seed: HitSeed::new(1),
            sr: SampleRate::HW,
        };
        Cowbell::new(p, h)
    }

    /// ADR 0287: `pace` is swept before every block whose pulses render
    /// a sample at a time (`Pulse::sweep`'s `steady` fails), never
    /// floored, and once it runs it stays running: BUMP 0 from the start,
    /// BUMP 127 after ~110 ms, a few blocks past the last ticked one at
    /// most.
    #[test]
    fn pace_covers_every_ticked_block() {
        for bump in [0, 1, 64, 127] {
            let mut k = bell(bump);
            let (mut stopped, mut last_ticked) = (None, None);
            for b in 0..400 {
                let pace = k.pace();
                assert!(matches!(pace, PulsePace::Runs | PulsePace::Swept));
                let said = pace == PulsePace::Swept;
                let (mut d, mut m) = (k.bump, [0.0; BLOCK_SIZE]);
                octaves(&mut d, k.bump_oct, &mut m);
                let ticks =
                    k.p[0].steady(k.hz[0], &m).is_none() || k.p[1].steady(k.hz[1], &m).is_none();
                assert!(said || !ticks, "BUMP {bump}, block {b}");
                if ticks {
                    last_ticked = Some(b);
                }
                match stopped {
                    None if !said => stopped = Some(b),
                    Some(_) => assert!(!said, "BUMP {bump}, block {b}"),
                    None => {}
                }
                k.render::<false>(&mut [0.0; BLOCK_SIZE]);
            }
            let stopped = stopped.expect("BUMP falls in 6 ms");
            assert_eq!(stopped == 0, bump == 0, "BUMP {bump}");
            assert!(stopped < 90, "BUMP {bump}: {stopped}");
            assert!(stopped <= last_ticked.map_or(0, |t| t + 10), "BUMP {bump}");
        }
    }
}
