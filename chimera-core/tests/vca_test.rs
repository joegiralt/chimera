//! The VCA destination (filter-routing spec § 4, § Tests "VCA").

use chimera_core::addr::ParamAddr;
use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{CUTOFF, MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

pub fn mods(routes: &[(ModSource, ParamAddr, i8)]) -> ModState {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        let _ = reg.add(a, *b"TEST\0\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amt) in routes {
        let d = ms.find(a).unwrap();
        ms.set_route(s.index(), d, amt);
    }
    ms
}

/// `blocks` blocks of note 60 at `vel`, key up at `off`; the output.
fn render(p: &ParamSnapshot, ms: &ModState, off: usize, blocks: usize, vel: u8) -> Vec<f32> {
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::new(vel).unwrap(), p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for i in 0..blocks {
        if i == off {
            v.note_off();
        }
        v.render(&mut b, p, ms);
        out.extend_from_slice(&b);
    }
    out
}

fn init(e: EngineType) -> ParamSnapshot {
    ParamSnapshot::for_engine(e)
}

/// No route: VEL changes nothing (the Algo/Modal pass-through).
#[test]
fn no_route_passes_through_and_vel_is_unread() {
    for e in EngineType::ALL {
        let (mut a, mut b) = (init(e), init(e));
        (a.out.vca_vel, b.out.vca_vel) = (0.0, 1.0);
        let ms = ModState::new();
        assert_eq!(
            render(&a, &ms, 50, 80, 100),
            render(&b, &ms, 50, 80, 100),
            "{e:?}"
        );
    }
}

/// ENV 2 → VCA at 100 %: the note follows ENV 2's contour (× velocity)
/// within 1 %, on Algo and Modal.
#[test]
fn env2_on_the_vca_follows_its_contour() {
    for e in EngineType::ALL {
        let p = init(e);
        let plain = render(&p, &ModState::new(), 50, 80, 100);
        let routed = render(&p, &mods(&[(ModSource::Env2, VCA, 127)]), 50, 80, 100);
        let mut env = Envelope::new();
        env.note_on();
        let v = 100.0 / 127.0;
        for blk in 0..80 {
            let mut g = [0.0f32; BLOCK_SIZE];
            env.run_block(
                &p.envelopes[1],
                &EnvMods::NONE,
                blk < 50,
                SR,
                Some((&mut g, 1.0)),
            );
            for (n, gn) in g.iter().enumerate() {
                let i = blk * BLOCK_SIZE + n;
                let want = plain[i] * gn.clamp(0.0, 1.0) * v;
                assert!(
                    (routed[i] - want).abs() <= 0.01 * plain[i].abs() + 1e-7,
                    "{e:?} sample {i}"
                );
            }
        }
    }
}

/// Halving ENV 2's amount halves the output exactly, with FOLD on: the
/// fold comes before the VCA.
#[test]
fn half_the_amount_is_half_the_level() {
    let mut p = init(EngineType::Algo);
    p.folder.fold = 0.6;
    let full = render(&p, &mods(&[(ModSource::Env2, VCA, 126)]), 50, 80, 100);
    let half = render(&p, &mods(&[(ModSource::Env2, VCA, 63)]), 50, 80, 100);
    for (i, (f, h)) in full.iter().zip(&half).enumerate() {
        assert_eq!(*f, 2.0 * h, "sample {i}");
    }
}

/// Two routes sum and clamp at 1: ENV 1 and ENV 2 at sustain (0.7 each).
#[test]
fn two_routes_sum_and_clamp() {
    let p = init(EngineType::Algo);
    let plain = render(&p, &ModState::new(), 90, 90, 127);
    let both = render(
        &p,
        &mods(&[(ModSource::Env1, VCA, 127), (ModSource::Env2, VCA, 127)]),
        90,
        90,
        127,
    );
    let late = 80 * BLOCK_SIZE; // both envelopes in sustain
    for i in late..late + BLOCK_SIZE {
        assert_eq!(
            both[i],
            plain[i] * 1.0,
            "sample {i}: the gain clamps at 1 (VEL 1 × vel 127)"
        );
    }
}

/// The SH-101 feel: ENV 1 on the VCA and the cutoff; ENV 1's decay moves both.
#[test]
fn one_envelope_moves_cutoff_and_level() {
    let mut p = init(EngineType::Algo);
    p.filter.cutoff = 800.0;
    // 800 Hz · 2^(10 · 0.2) = 3.2 kHz: the decay sweeps below the 20 kHz clamp.
    p.envelopes[0].sustain = 0.2;
    let with = |decay: f32, routes: &[(ModSource, ParamAddr, i8)]| {
        let mut q = p.clone();
        q.envelopes[0].decay = decay;
        render(&q, &mods(routes), 60, 60, 100)
    };
    let vca = [(ModSource::Env1, VCA, 127)];
    let cut = [(ModSource::Env1, CUTOFF, 127)];
    let both = [(ModSource::Env1, VCA, 127), (ModSource::Env1, CUTOFF, 127)];
    for r in [&vca[..], &cut[..], &both[..]] {
        assert_ne!(with(0.3, r), with(0.6, r), "{r:?}");
    }
    assert_ne!(with(0.3, &both), with(0.3, &vca));
    assert_ne!(with(0.3, &both), with(0.3, &cut));
}

/// AMP's VEL: at 0 two velocities give the same gain; at 100 % the gain
/// scales with velocity (the engine's own velocity divided out).
#[test]
fn vel_scales_the_vca() {
    for (vca_vel, same) in [(0.0f32, true), (1.0, false)] {
        let mut p = init(EngineType::Algo);
        p.out.vca_vel = vca_vel;
        let ms = mods(&[(ModSource::Env2, VCA, 127)]);
        // Over a whole block (a single sample may sit near a zero crossing).
        let ratio = |vel| {
            let (r, q) = (
                render(&p, &ms, 60, 60, vel),
                render(&p, &ModState::new(), 60, 60, vel),
            );
            let block = 40 * BLOCK_SIZE..41 * BLOCK_SIZE;
            let sum = |v: &[f32]| v[block.clone()].iter().map(|x| x.abs()).sum::<f32>();
            sum(&r) / sum(&q)
        };
        let (lo, hi) = (ratio(40), ratio(120));
        if same {
            assert!((lo - hi).abs() < 1e-6, "{lo} {hi}");
        } else {
            assert!((lo / hi - 40.0 / 120.0).abs() < 1e-3, "{lo} {hi}");
        }
    }
}

/// Review Focus 4: LFO 1 → VCA at −127 clamps at 0, never inverts.
#[test]
fn a_negative_vca_route_never_inverts() {
    let mut p = init(EngineType::Algo);
    p.lfos[0].rate = 8.0;
    let plain = render(&p, &ModState::new(), 90, 90, 100);
    let neg = render(&p, &mods(&[(ModSource::Lfo1, VCA, -127)]), 90, 90, 100);
    for (i, (a, b)) in plain.iter().zip(&neg).enumerate() {
        assert!(a * b >= 0.0, "sample {i}: {a} vs {b}");
    }
    assert!(
        neg.iter().any(|&x| x != 0.0),
        "the negative half of the LFO opens it"
    );
}
