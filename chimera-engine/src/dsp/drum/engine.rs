//! DRUM's engine (ADR 0265): a voice slot playing one hit. A hit only
//! starts from a `DrumHit` (`strike`); a plain note never starts one.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_midi::value::U7;

use crate::Velocity;
use crate::block::{GlideMs, apply_offset};
use crate::dsp::knobs::coeff;
use crate::dsp::lfo::{Lfo, LfoParams};
use crate::dsp::modal::ModalEngine;
use crate::dsp::modulator::env_a::{ACoefs, EnvA, Stage};
use crate::hw::BLOCK_SIZE;
use crate::params::{EnvParams, ParamSnapshot};
use crate::project::PartId;

use super::kernel::KernelState;
use super::laws::resolve;
use super::sound::{
    AmpMode, EnvDecay, PadAmp, PadRoutes, PadSound, Places, RouteDest, RouteOffsets, RouteValues,
};
use super::types::{DrumHit, HitSeed, Pan, SampleRate};
use super::view::KIT_VIEW;

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

/// A hit's AMP envelope (ADR 0269), latched at the strike: Envelope A's
/// stages on the pad's AHD. ONE-SHOT runs with its key held for good, so
/// it ends where the decay reaches 0. GATED holds at full until note-off,
/// then releases over the decay. An infinite decay holds either way.
#[derive(Clone, Copy, Debug)]
struct AmpEnv {
    coefs: ACoefs,
    env: EnvA,
    /// Held: GATED until note-off, else always.
    key: bool,
    /// Note-off lets go of the key: GATED with a finite decay.
    gated: bool,
}

impl AmpEnv {
    /// `amp`'s envelope at `sr`, struck; `None` when OPEN (a constant 1).
    fn strike(amp: &PadAmp, sr: SampleRate) -> Option<AmpEnv> {
        let e = amp.env();
        if e.is_open() {
            return None;
        }
        let gated = amp.mode() == AmpMode::Gated && e.decay() != EnvDecay::Infinite;
        let mut env = EnvA::new();
        env.note_on();
        Some(AmpEnv {
            coefs: AmpEnv::coefs(e.env_params(), gated, sr),
            env,
            key: true,
            gated,
        })
    }

    /// `p`'s stages; GATED: sustain at full, the decay its release. Out
    /// of line: the strike's and `follow`'s one copy.
    #[inline(never)]
    fn coefs(p: EnvParams, gated: bool, sr: SampleRate) -> ACoefs {
        let p = if gated {
            EnvParams { sustain: 1.0, ..p }
        } else {
            p
        };
        ACoefs::new(&p, 0.0, sr.get())
    }

    /// Done: released to idle, or decayed to a sustain of 0.
    fn ended(&self) -> bool {
        match self.env.stage() {
            Stage::Idle => true,
            Stage::Sustain => self.env.level() == 0.0,
            _ => false,
        }
    }
}

/// A hit's mod envelope and routes (ADR 0269), latched at the strike;
/// the pad LFO's settings, its state the engine's (`DrumEngine::lfo`).
#[derive(Clone, Copy, Debug)]
struct PadMods {
    routes: PadRoutes,
    coefs: ACoefs,
    /// The mod envelope: AHD, its key held for good.
    env: EnvA,
    lfo: LfoParams,
    /// Each route's a (`PadRoutes::amounts`), glided by `follow`.
    amounts: [f32; 4],
    /// LFO SPEED's and DEPTH's sums, last block's (ADR 0250's DEPTH).
    lfo_off: (f32, f32),
    /// A route moves VOLUME.
    vol_routed: bool,
    /// VOLUME's gain, last block's and this one's; `None` before the
    /// first block, or with no VOLUME route.
    vol: Option<(f32, f32)>,
}

impl PadMods {
    /// `s`'s, struck; `None` with no route (the pad moves nothing).
    fn strike(s: &PadSound, sr: SampleRate) -> Option<PadMods> {
        let routes = *s.routes();
        if routes.is_off() {
            return None;
        }
        let mut env = EnvA::new();
        env.note_on();
        Some(PadMods {
            routes,
            coefs: ACoefs::new(&s.env().env_params(), 0.0, sr.get()),
            env,
            lfo: s.lfo().lfo_params(),
            amounts: routes.amounts(),
            lfo_off: (0.0, 0.0),
            vol_routed: routes.moves(RouteDest::Volume),
            vol: None,
        })
    }
}

/// A hit following its pad's live edits (ADR 0269): the view's publishes
/// seen, and its continuous values gliding to the pad's.
#[derive(Clone, Copy, Debug)]
struct Follow {
    /// `KitView::sound_if_newer`'s.
    seen: u32,
    sr: SampleRate,
    /// The KNOB coefficient at `sr`, worked out at the first glide.
    k: Option<f32>,
    /// Where the hit goes: its sound, the pad's live values on its
    /// choices (`PadSound::live_target`).
    target: PadSound,
    /// Where the places are and where they go, while they glide.
    glide: Option<(Places, Places)>,
}

/// The voice's drum: idle, or one hit, its kernel in place.
pub struct DrumEngine {
    /// What was struck, while a hit sounds: `kernel`, `pan`, `amp` and
    /// `mods` are its.
    hit: Option<DrumHit>,
    kernel: KernelState,
    pan: Option<MidSide>,
    /// Its AMP envelope, unless OPEN.
    amp: Option<AmpEnv>,
    /// Its mod envelope and routes, if a route moves something.
    mods: Option<PadMods>,
    /// The pad LFO, the voice's: FREE runs on from hit to hit, TRIG
    /// restarts at its PHASE.
    lfo: Lfo,
    follow: Follow,
    /// The hit's velocity, latched at the strike.
    vel: Velocity,
    /// Its VEL gain (`PadAmp::vel_gain`), last block's.
    vel_g: f32,
}

crate::in_place::field_list!(DrumEngine => DrumEngine { hit, kernel, pan, amp, mods, lfo, follow, vel, vel_g });

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
            addr_of_mut!((*p).amp).write(None);
            addr_of_mut!((*p).mods).write(None);
            addr_of_mut!((*p).lfo).write(Lfo::new());
            addr_of_mut!((*p).follow).write(Follow {
                seen: 0,
                sr: SampleRate::HW,
                k: None,
                target: PadSound::DEFAULT,
                glide: None,
            });
            addr_of_mut!((*p).vel).write(Velocity::DEFAULT);
            addr_of_mut!((*p).vel_g).write(1.0);
            slot.assume_init_mut()
        }
    }

    /// `hit` at `vel`, cutting what played: its pad's machine and knobs,
    /// `hit.pitch()` semitones over the pad's PITCH, its sound latched.
    /// The kernel plays at full velocity: the pad's VEL is the hit's level
    /// by `vel` (`amp_block`), its only one.
    #[inline(never)]
    #[optimize(size)]
    pub fn strike(&mut self, hit: &DrumHit, vel: Velocity, seed: HitSeed, sr: SampleRate) {
        let pad = hit.params();
        let p = resolve(pad.machine(), pad.knobs()).transposed(hit.pitch());
        self.hit = Some(*hit);
        self.kernel.strike_in_place(p, Velocity::MAX, seed, sr);
        self.pan = MidSide::of(pad.pan());
        self.amp = AmpEnv::strike(pad.sound().amp(), sr);
        self.vel = vel;
        self.vel_g = pad.sound().amp().vel_gain(vel);
        self.mods = PadMods::strike(pad.sound(), sr);
        if let Some(m) = &self.mods {
            self.lfo.note_on(&m.lfo);
        }
        let k = self.follow.k.filter(|_| self.follow.sr == sr);
        self.follow = Follow {
            seen: self.follow.seen,
            sr,
            k,
            target: *pad.sound(),
            glide: None,
        };
    }

    /// The hit has seen `part`'s view up to `seq` (`KitView::seq`): it
    /// follows the publishes after it.
    pub fn watch(&mut self, seq: u32) {
        self.follow.seen = seq;
    }

    /// Once a block, before `overlay`: a publish since the hit's last
    /// moves where its continuous values go (`PadSound::live_target`),
    /// and they glide there at the KNOB coefficient on their places, as a
    /// Part's knob; the AMP and mod envelopes' stages are rebuilt only as
    /// they move. Nothing published, one load.
    #[inline(never)]
    #[optimize(size)]
    pub fn follow(&mut self, part: PartId) {
        let Some(hit) = &mut self.hit else {
            return;
        };
        let f = &mut self.follow;
        if let Some(live) = KIT_VIEW.sound_if_newer(part, hit.pad(), &mut f.seen) {
            let target = hit.params().sound().live_target(&live);
            if target != f.target {
                f.target = target;
                let at = f
                    .glide
                    .map_or_else(|| hit.params().sound().places(), |g| g.0);
                f.glide = Some((at, target.places()));
            }
        }
        let Some((was, to)) = f.glide else {
            return;
        };
        let sr = f.sr;
        let k = *f.k.get_or_insert_with(|| coeff(GlideMs::KNOB, sr.get()));
        let mut at = was;
        let s = hit.sound_mut();
        if at.ease(&to, k) {
            // Landed: the pad's values, bit for bit.
            *s = f.target;
            f.glide = None;
        } else {
            s.set_places(&at);
            f.glide = Some((at, to));
        }
        if let Some(a) = &mut self.amp
            && at.amp_env() != was.amp_env()
        {
            a.coefs = AmpEnv::coefs(s.amp_env_at(&at), a.gated, sr);
        }
        if let Some(m) = &mut self.mods {
            if at.env() != was.env() {
                m.coefs = AmpEnv::coefs(s.env_at(&at), false, sr);
            }
            if at.lfo() != was.lfo() {
                m.lfo = s.lfo_at(&at);
            }
            m.amounts = at.amounts();
            m.vol_routed = m.routes.moves_at(&m.amounts, |d| d == RouteDest::Volume);
        }
    }

    /// The hit's mod envelope and LFO, a block on, and its routes' sums:
    /// CUTOFF and RES for the voice's filter, VOLUME into `amp_block`,
    /// LFO SPEED and DEPTH kept for the next block. Each source's value is
    /// the block's end (the filter's ramp lands there), the LFO's its
    /// start (`Lfo::run_block`). Nothing to do, and zero, unrouted.
    pub fn mods_block(
        &mut self,
        part_lfos: [f32; 3],
        vel: Velocity,
        sr: SampleRate,
    ) -> RouteOffsets {
        let Some(m) = &mut self.mods else {
            return RouteOffsets::NONE;
        };
        let mut p = m.lfo;
        let (speed, depth) = m.lfo_off;
        if speed != 0.0 {
            apply_offset(&mut p, LfoParams::RATE, speed);
        }
        if depth != 0.0 {
            apply_offset(&mut p, LfoParams::DEPTH, depth);
        }
        let lfo = self.lfo.run_block(&p, sr.get());
        m.env.advance(&m.coefs, true, BLOCK_SIZE as u32);
        let [l1, l2, l3] = part_lfos;
        let v = RouteValues::new([m.env.level(), lfo, vel.unit(), l1, l2, l3]);
        let off = m.routes.offsets_at(&m.amounts, &v);
        m.lfo_off = (off.lfo_speed(), off.lfo_depth());
        // A VOLUME route glided to 0 ramps on to unity, then stops.
        if m.vol_routed || m.vol.is_some() {
            let g = off.volume().get();
            let routed = m.vol_routed;
            m.vol = Some((m.vol.map_or(g, |v| v.1), g)).filter(|&(a, b)| routed || a != b);
        }
        off
    }

    /// A route moves the hit's CUTOFF or RES: the filter takes its moving
    /// path, as under a Part route.
    pub fn moves_filter(&self) -> bool {
        self.mods
            .as_ref()
            .is_some_and(|m| m.routes.moves_at(&m.amounts, RouteDest::is_filter))
    }

    /// The hit's FILTER and AMP cells over `m` (`PadSound::overlay`).
    pub fn overlay(&self, m: &mut ParamSnapshot) {
        if let Some(h) = &self.hit {
            h.params().sound().overlay(m);
        }
    }

    /// The key is up: a GATED AMP releases; ONE-SHOT plays on.
    pub fn note_off(&mut self) {
        if let Some(a) = self.amp.as_mut().filter(|a| a.gated) {
            a.key = false;
        }
    }

    /// The AMP envelope as a per-sample gain on `out`; none while OPEN.
    /// A GATED key let go early still plays attack and hold first. Then
    /// VEL's gain on the hit's velocity times VOLUME's routes, ramped
    /// from last block's as a VCA source; a constant while VEL holds, and
    /// nothing at VEL 0 unrouted.
    pub fn amp_block(&mut self, out: &mut [f32; BLOCK_SIZE]) {
        if let Some(a) = &mut self.amp {
            for x in out.iter_mut() {
                let key = a.key || matches!(a.env.stage(), Stage::Attack | Stage::Hold);
                *x *= a.env.tick(&a.coefs, key);
            }
        }
        let g = self
            .hit
            .as_ref()
            .map_or(1.0, |h| h.params().sound().amp().vel_gain(self.vel));
        let was = core::mem::replace(&mut self.vel_g, g);
        let (vf, vt) = self.mods.as_ref().and_then(|m| m.vol).unwrap_or((1.0, 1.0));
        let (from, to) = (vf * was, vt * g);
        if from != 1.0 || to != 1.0 {
            let step = (to - from) / BLOCK_SIZE as f32;
            for (n, x) in out.iter_mut().enumerate() {
                *x *= from + step * n as f32;
            }
        }
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

    /// A hit sounds: its kernel rings and its AMP hasn't ended.
    pub fn is_active(&self) -> bool {
        self.hit.is_some()
            && !self.kernel.finished()
            && !self.amp.as_ref().is_some_and(AmpEnv::ended)
    }
}
