//! Envelope A (filter-routing spec § Envelope A, § Tests "Envelope A").

use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::modulator::env_a::{ACoefs, EnvA, Stage};
use chimera_core::dsp::modulator::law::speed_ranges;
use chimera_core::dsp::modulator::{EnvSpeed, HoldPos};
use chimera_core::params::EnvParams;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn params(speed: EnvSpeed) -> EnvParams {
    EnvParams {
        speed,
        hold: 0.0,
        hold_pos: HoldPos::Off,
        ..EnvParams::default()
    }
}

/// One sample (a stage ends on the first tick past its time: up to one
/// sample late, plus float error), or 2 ppm on long stages (f32's limit).
fn close(samples: f64, secs: f64, sr: u32, what: &str) {
    let want = secs * sr as f64;
    assert!(
        (samples - want).abs() <= 1.05f64.max(want * 2e-6),
        "{what}: {samples} samples, want {want}"
    );
}

/// The manual's ranges at every SPEED, at both slider ends.
#[test]
fn slider_ends_give_the_manuals_times() {
    // (speed, H max, A min, A max, D/R min, D/R max), seconds.
    let table = [
        (EnvSpeed::Fast, 2.5, 0.2e-3, 1.5, 0.6e-3, 2.5),
        (EnvSpeed::Med, 10.0, 2e-3, 10.0, 3.5e-3, 10.0),
        (EnvSpeed::Slow, 60.0, 9.3e-3, 60.0, 30e-3, 60.0),
    ];
    for (s, h1, a0, a1, d0, d1) in table {
        let r = speed_ranges(s);
        let at = |range: chimera_core::dsp::modulator::law::Range, p| range.at(p) as f64;
        close(at(r.hold, 0.0) * SR as f64, 1e-6, SR, "H min");
        close(at(r.hold, 1.0) * SR as f64, h1, SR, "H max");
        close(at(r.attack, 0.0) * SR as f64, a0, SR, "A min");
        close(at(r.attack, 1.0) * SR as f64, a1, SR, "A max");
        close(at(r.dec_rel, 0.0) * SR as f64, d0, SR, "D min");
        close(at(r.dec_rel, 1.0) * SR as f64, d1, SR, "D max");
    }
}

/// Samples the running stage takes from `note_on` (A: 0 → 1) and a full
/// decay (1 → S = 0), by the closed form's placement.
#[test]
fn a_full_swing_takes_the_sliders_time() {
    for speed in [EnvSpeed::Fast, EnvSpeed::Med, EnvSpeed::Slow] {
        for pos in [0.0f32, 0.3, 0.7] {
            let r = speed_ranges(speed);
            let mut p = params(speed);
            (p.attack, p.decay, p.sustain) = (pos, pos, 0.0);
            let c = ACoefs::new(&p, 0.0, SR);
            let mut e = EnvA::new();
            e.note_on();
            close(
                e.stage_samples(&c) as f64,
                r.attack.at(pos) as f64,
                SR,
                "attack",
            );
            let n = e.stage_samples(&c);
            e.advance(&c, true, n);
            assert_eq!(e.stage(), Stage::Decay, "{speed:?} {pos}");
            close(
                e.stage_samples(&c) as f64,
                r.dec_rel.at(pos) as f64,
                SR,
                "decay",
            );
        }
    }
}

/// Short stages, tick by tick: the per-sample path agrees with the law.
#[test]
fn ticks_take_the_sliders_time() {
    let mut p = params(EnvSpeed::Med); // A 10 ms, D 300 ms
    p.sustain = 0.0;
    let c = ACoefs::new(&p, 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    let (mut attack, mut decay) = (0, 0);
    while e.stage() != Stage::Sustain {
        match e.stage() {
            Stage::Attack => attack += 1,
            _ => decay += 1,
        }
        e.tick(&c, true);
    }
    close(attack as f64, 0.010, SR, "attack");
    close(decay as f64, 0.299_18, SR, "decay");
}

/// Review Focus 2: at 44.1 kHz a MED 10 ms attack takes 441 samples.
#[test]
fn stage_times_follow_the_sample_rate() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, 44_100);
    let mut e = EnvA::new();
    e.note_on();
    close(
        e.stage_samples(&c) as f64,
        0.010_003,
        44_100,
        "attack at 44.1 kHz",
    );
}

#[test]
fn hold_positions() {
    let mut p = params(EnvSpeed::Med);
    p.hold = 0.8; // about 0.40 s
    let hold = (speed_ranges(EnvSpeed::Med).hold.at(0.8) * SR as f32) as u32;
    let stages = |p: &EnvParams, key_for: usize| {
        let c = ACoefs::new(p, 0.0, SR);
        let mut e = EnvA::new();
        e.note_on();
        (0..2 * SR as usize)
            .map(|i| {
                e.tick(&c, i < key_for);
                e.stage()
            })
            .collect::<Vec<_>>()
    };
    // OFF: no hold stage, whatever H.
    p.hold_pos = HoldPos::Off;
    assert!(!stages(&p, 100_000).contains(&Stage::Hold));
    // AHDSR: holds 1 for H after the attack. The tick that ends the attack
    // already reports Hold (at level 1, the attack's last sample), then H's
    // `hold` samples follow.
    p.hold_pos = HoldPos::Ahdsr;
    let s = stages(&p, 100_000);
    assert_eq!(
        s.iter().filter(|&&x| x == Stage::Hold).count() as u32,
        hold + 1
    );
    // GATE EXT: a one-sample gate still plays attack and decay, and
    // releases H after the note-on.
    p.hold_pos = HoldPos::GateExt;
    let s = stages(&p, 1);
    assert!(s.contains(&Stage::Decay) && !s.contains(&Stage::Hold));
    let release = s.iter().position(|&x| x == Stage::Release).unwrap() as u32;
    assert!(
        (release as i64 - hold as i64).abs() <= 1,
        "{release} vs {hold}"
    );
}

/// The per-sample step is one multiply-add toward the target: attack
/// levels form a geometric series about 1.3.
#[test]
fn tick_is_a_geometric_step() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    let l: Vec<f32> = (0..4).map(|_| e.tick(&c, true)).collect();
    let r1 = (l[2] - 1.3) / (l[1] - 1.3);
    let r2 = (l[3] - 1.3) / (l[2] - 1.3);
    assert!((r1 - r2).abs() < 1e-4, "{r1} {r2}");
}

/// Envelope A from the spec's formulas in f64: the reference both f32
/// paths are held to (ADR 0036). Same stages, gate and settling as `EnvA`.
struct RefA {
    stage: Stage,
    level: f64,
    hold_left: u64,
    since_on: u64,
    ca: f64,
    cd: f64,
    cr: f64,
    sus: f64,
    hold: u64,
    hold_pos: HoldPos,
}

impl RefA {
    fn new(p: &EnvParams, sr: u32) -> Self {
        let fs = sr as f64;
        // The manual's ranges in seconds; a position p is min·(max/min)^p.
        let (h, a, dr) = match p.speed {
            EnvSpeed::Fast => ((1e-6, 2.5), (2e-4, 1.5), (6e-4, 2.5)),
            EnvSpeed::Med => ((1e-6, 10.0), (2e-3, 10.0), (3.5e-3, 10.0)),
            EnvSpeed::Slow => ((1e-6, 60.0), (9.3e-3, 60.0), (3e-2, 60.0)),
        };
        let at = |(lo, hi): (f64, f64), x: f32| lo * (hi / lo).powf(x as f64);
        // c = 1 − e^(−1/(τ·fs)), τ = time / ln(the stage's overshoot ratio).
        let c = |secs: f64, ln: f64| 1.0 - (-ln / (secs * fs)).exp();
        Self {
            stage: Stage::Idle,
            level: 0.0,
            hold_left: 0,
            since_on: u64::MAX,
            ca: c(at(a, p.attack), (1.3f64 / 0.3).ln()),
            cd: c(at(dr, p.decay), 101f64.ln()),
            cr: c(at(dr, p.release), 101f64.ln()),
            sus: p.sustain as f64,
            hold: (at(h, p.hold) * fs) as u64,
            hold_pos: p.hold_pos,
        }
    }

    fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.since_on = 0;
    }

    fn tick(&mut self, key: bool) -> f64 {
        let gate = key || (self.hold_pos == HoldPos::GateExt && self.since_on < self.hold);
        self.since_on = self.since_on.saturating_add(1);
        let running = matches!(
            self.stage,
            Stage::Attack | Stage::Hold | Stage::Decay | Stage::Sustain
        );
        if running && !gate {
            self.stage = Stage::Release;
        } else if (self.stage == Stage::Hold && self.hold_left == 0)
            || (self.stage == Stage::Sustain && self.level > self.sus)
        {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Decay && self.level <= self.sus {
            self.stage = Stage::Sustain;
        }
        let (c, t, end, rising) = match self.stage {
            Stage::Hold => {
                self.hold_left -= 1;
                return self.level;
            }
            Stage::Attack => (self.ca, 1.3, 1.0, true),
            Stage::Decay => (self.cd, self.sus - 0.01, self.sus, false),
            Stage::Release => (self.cr, -0.01, 0.0, false),
            _ => return self.level,
        };
        let l = self.level + c * (t - self.level);
        if (rising && l >= end) || (!rising && l <= end) {
            self.level = end;
            self.stage = match self.stage {
                Stage::Attack if self.hold_pos == HoldPos::Ahdsr && self.hold > 0 => {
                    self.hold_left = self.hold;
                    Stage::Hold
                }
                Stage::Attack => Stage::Decay,
                Stage::Decay => Stage::Sustain,
                _ => Stage::Idle,
            };
        } else {
            self.level = l;
        }
        self.level
    }
}

/// `level` (and `stage`, if given) at sample `n` match the reference at
/// n − 1, n or n + 1 within 1e-4: ADR 0036's tolerance, which supersedes
/// the spec's 1e-6 (f32 cannot hold it).
fn near(refs: &[(Stage, f64)], n: usize, stage: Option<Stage>, level: f32) -> bool {
    (n.saturating_sub(1)..=(n + 1).min(refs.len() - 1))
        .any(|m| stage.is_none_or(|s| s == refs[m].0) && (refs[m].1 - level as f64).abs() <= 1e-4)
}

/// The per-sample paths (`tick`, `fill`) and the per-block path
/// (`advance`) each match the f64 reference across every stage boundary.
#[test]
fn each_path_matches_an_f64_reference() {
    // (…, blocks the key is held)
    for (speed, hold_pos, hold, a, d, s, r, held) in [
        (EnvSpeed::Fast, HoldPos::Off, 0.0, 0.0, 0.3, 0.5, 0.3, 150), // attack ends mid-block
        (
            EnvSpeed::Med,
            HoldPos::Ahdsr,
            0.55,
            0.19,
            0.4,
            0.2,
            0.4,
            150,
        ), // a hold stage
        // GATE EXT: the key is up after one block, well inside H (760
        // samples), so the extended gate decides where the release starts,
        // mid-block.
        (EnvSpeed::Med, HoldPos::GateExt, 0.6, 0.1, 0.2, 0.6, 0.3, 1),
        (
            EnvSpeed::Fast,
            HoldPos::Ahdsr,
            0.0,
            0.05,
            0.05,
            0.0,
            0.05,
            150,
        ), // decay to 0, release to idle
    ] {
        let p = EnvParams {
            speed,
            hold_pos,
            hold,
            attack: a,
            decay: d,
            sustain: s,
            release: r,
            ..EnvParams::default()
        };
        const BLOCKS: usize = 300;
        let key = |b: usize| b < held;
        let mut reference = RefA::new(&p, SR);
        reference.note_on();
        let refs: Vec<(Stage, f64)> = (0..BLOCKS * BLOCK_SIZE)
            .map(|n| {
                let l = reference.tick(key(n / BLOCK_SIZE));
                (reference.stage, l)
            })
            .collect();
        let c = ACoefs::new(&p, 0.0, SR);
        let (mut ticked, mut filled, mut blocked) = (EnvA::new(), EnvA::new(), EnvA::new());
        for e in [&mut ticked, &mut filled, &mut blocked] {
            e.note_on();
        }
        for b in 0..BLOCKS {
            let mut buf = [0.0f32; BLOCK_SIZE];
            filled.fill(&c, key(b), &mut buf);
            blocked.advance(&c, key(b), BLOCK_SIZE as u32);
            for (i, &f) in buf.iter().enumerate() {
                let n = b * BLOCK_SIZE + i;
                let t = ticked.tick(&c, key(b));
                assert!(
                    near(&refs, n, Some(ticked.stage()), t),
                    "tick {speed:?} {hold_pos:?} {n}: {t} vs {:?}",
                    refs[n]
                );
                assert!(
                    near(&refs, n, None, f),
                    "fill {speed:?} {hold_pos:?} {n}: {f} vs {:?}",
                    refs[n]
                );
            }
            let n = (b + 1) * BLOCK_SIZE - 1;
            assert!(
                near(&refs, n, Some(blocked.stage()), blocked.level()),
                "advance {speed:?} {hold_pos:?} block {b}: {} vs {:?}",
                blocked.level(),
                refs[n]
            );
            assert!(
                near(&refs, n, Some(filled.stage()), filled.level()),
                "fill's stage, block {b}"
            );
        }
    }
}

/// A raised S doesn't lift a decaying level: it sustains where it is.
#[test]
fn raising_s_mid_decay_holds_the_level() {
    let mut p = params(EnvSpeed::Med);
    p.sustain = 0.2;
    let c = ACoefs::new(&p, 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    while e.stage() != Stage::Decay {
        e.tick(&c, true);
    }
    for _ in 0..2000 {
        e.tick(&c, true); // decaying toward 0.2
    }
    let before = e.level();
    p.sustain = 0.9;
    let c = ACoefs::new(&p, 0.0, SR);
    let after = e.tick(&c, true);
    assert_eq!(e.stage(), Stage::Sustain);
    assert_eq!(after, before);
}

/// A release during the attack and a note-on during the release leave the
/// level where it is (RETRIG from the current level).
#[test]
fn release_and_retrigger_do_not_move_the_level() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    for _ in 0..200 {
        e.tick(&c, true);
    }
    let before = e.level();
    let after = e.tick(&c, false);
    assert_eq!(e.stage(), Stage::Release);
    assert!((after - before).abs() < 0.01, "{before} → {after}");
    for _ in 0..2000 {
        e.tick(&c, false);
    }
    let before = e.level();
    e.note_on();
    let after = e.tick(&c, true);
    assert_eq!(e.stage(), Stage::Attack);
    assert!((after - before).abs() < 0.01, "{before} → {after}");
}

/// LEVEL: unrouted the peak is 1; routed it is `clamp(Σ, 0, 1)`.
#[test]
fn level_sets_the_peak() {
    let p = EnvParams::default();
    for (level, want) in [(None, 1.0f32), (Some(0.25), 0.25)] {
        let m = EnvMods {
            level,
            ..EnvMods::NONE
        };
        let mut e = Envelope::new();
        e.note_on(&p);
        let mut peak = 0.0f32;
        for _ in 0..40 {
            peak = peak.max(e.run_block(&p, &m, true, SR, None));
        }
        // Block starts sample the peak within 0.01 (the decay has begun).
        assert!((peak - want).abs() < 0.01, "{level:?}: {peak}");
    }
}

/// On the VCA path a LEVEL change ramps across the block instead of
/// stepping (no zipper from an LFO → LEVEL route).
#[test]
fn a_level_change_ramps_on_the_vca_path() {
    let p = EnvParams {
        sustain: 0.7,
        ..EnvParams::default()
    };
    let full = EnvMods {
        level: Some(1.0),
        ..EnvMods::NONE
    };
    let half = EnvMods {
        level: Some(0.5),
        ..EnvMods::NONE
    };
    let mut e = Envelope::new();
    e.note_on(&p);
    for _ in 0..1000 {
        e.run_block(&p, &full, true, SR, None); // well into sustain at 0.7
    }
    let mut g = [0.0f32; BLOCK_SIZE];
    e.run_block(&p, &full, true, SR, Some((&mut g, 1.0)));
    let mut g = [0.0f32; BLOCK_SIZE];
    e.run_block(&p, &half, true, SR, Some((&mut g, 1.0)));
    let step = 0.7 * 0.5 / BLOCK_SIZE as f32;
    assert!(
        (g[BLOCK_SIZE - 1] - 0.35).abs() < 1e-4,
        "{}",
        g[BLOCK_SIZE - 1]
    );
    assert!((g[0] - (0.7 - step)).abs() < 1e-4, "{}", g[0]);
    assert!(
        g.windows(2).all(|w| (w[0] - w[1] - step).abs() < 1e-5),
        "an even ramp"
    );
}

/// TIME +100 % makes every stage 32 × shorter; −100 % 32 × longer.
#[test]
fn time_scales_every_stage() {
    let p = params(EnvSpeed::Med);
    let samples = |time: f32| {
        let c = ACoefs::new(&p, time, SR);
        let mut e = EnvA::new();
        e.note_on();
        e.stage_samples(&c) as f64
    };
    let base = samples(0.0);
    assert!((samples(1.0) - base / 32.0).abs() <= 1.0);
    assert!((samples(-1.0) - base * 32.0).abs() <= 32.0);
}

/// Review Focus 1: a TIME sum far past ±1 clamps, and stays finite.
#[test]
fn time_route_sums_clamp() {
    let p = params(EnvSpeed::Med);
    let mut e = EnvA::new();
    e.note_on();
    for (sum, like) in [(8.0f32, 1.0f32), (-8.0, -1.0), (1e9, 1.0)] {
        let (c, d) = (ACoefs::new(&p, sum, SR), ACoefs::new(&p, like, SR));
        assert_eq!(e.stage_samples(&c), e.stage_samples(&d), "{sum}");
    }
    let c = ACoefs::new(&p, f32::NAN, SR);
    e.advance(&c, true, 64);
    assert!(e.level().is_finite());
}
