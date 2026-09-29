//! The VCA destination (filter-routing spec § 4, § Tests "VCA").

mod common;

use chimera_core::addr::ParamAddr;
use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, LfoForm};
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

/// `blocks` of note 60, key up at `off`; `(output, active after each block)`.
fn life(p: &ParamSnapshot, ms: &ModState, off: usize, blocks: usize) -> (Vec<f32>, Vec<bool>) {
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    let (mut out, mut alive) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for i in 0..blocks {
        if i == off {
            v.note_off();
        }
        v.render(&mut b, p, ms);
        out.extend_from_slice(&b);
        alive.push(v.is_active());
    }
    (out, alive)
}

/// An Algo Sound whose engine outlives any envelope here (RR 1, the slowest).
fn long() -> ParamSnapshot {
    let mut p = init(EngineType::Algo);
    p.algo.ops[0].rr = 1;
    p
}

fn with_env3(f: Func, fall: f32) -> ParamSnapshot {
    let mut p = long();
    let e = &mut p.envelopes[2];
    (e.env_type, e.func.fall) = (EnvType::B, fall);
    e.func.set_func(f);
    p
}

/// ENV 2 → VCA: the voice ends at the end of the block in which ENV 2 goes idle.
#[test]
fn env2_on_the_vca_ends_the_voice_when_it_idles() {
    let p = long();
    let (_, alive) = life(&p, &mods(&[(ModSource::Env2, VCA, 127)]), 20, 400);
    let (_, plain) = life(&p, &ModState::new(), 20, 400);
    let mut env = Envelope::new();
    env.note_on();
    // Ticked per sample, as the voice runs a slot routed to its VCA.
    let idle = (0..400)
        .find(|&b| {
            let mut g = [0.0f32; BLOCK_SIZE];
            env.run_block(
                &p.envelopes[1],
                &EnvMods::NONE,
                b < 20,
                SR,
                Some((&mut g, 1.0)),
            );
            env.is_idle()
        })
        .unwrap();
    assert!(alive[idle - 1] && !alive[idle], "ends at block {idle}");
    assert!(plain[idle], "the engine alone would have kept it");
}

/// No drone: a source that doesn't end holds the voice only while the key
/// is held; after key-up it ends through the 128-sample fade.
#[test]
fn nothing_drones_after_key_up() {
    let e3 = |f| (with_env3(f, 0.2), ModSource::Env3);
    for (p, s) in [
        e3(Func::Lfo(LfoForm::Free)),
        e3(Func::Env(EnvForm::Cycle)),
        (long(), ModSource::Lfo1),
    ] {
        let (out, alive) = life(&p, &mods(&[(s, VCA, 127)]), 30, 60);
        let gone = alive.iter().position(|a| !a).expect("the voice ends");
        // At once (gain 0 at key-up) or after the two-block fade.
        assert!((30..=33).contains(&gone), "{s:?}: ended at block {gone}");
        assert!(out[(gone + 1) * BLOCK_SIZE..].iter().all(|&x| x == 0.0));
    }
}

/// VEL holds at a non-zero gain: key-up ends the voice through the
/// two-block fade, a steady ramp to silence.
#[test]
fn a_sounding_voice_ends_through_the_fade() {
    let p = long();
    let (out, alive) = life(&p, &mods(&[(ModSource::Vel, VCA, 127)]), 30, 60);
    let (plain, _) = life(&p, &ModState::new(), 30, 60);
    assert_eq!(
        alive.iter().position(|a| !a),
        Some(32),
        "fades blocks 31-32"
    );
    let blk = |i: usize| i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE;
    let level = out[blk(29)].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    // The routed gain is steady, so out / plain is the fade's envelope.
    let held = out[blk(30)][0] / plain[blk(30)][0];
    let env: Vec<f32> = (blk(31).start..blk(32).end)
        .filter(|&i| plain[i].abs() > 1e-3)
        .map(|i| out[i] / plain[i] / held)
        .collect();
    assert!(env.windows(2).all(|w| w[1] < w[0]), "falls monotonically");
    assert!(env[0] < 1.0 && env[env.len() - 1] < 0.02, "{env:?}");
    let last = out[..blk(33).start]
        .iter()
        .rposition(|&x| x != 0.0)
        .unwrap();
    assert!(last >= blk(32).start, "sounds into the fade's last block");
    assert!(out[last].abs() < level / 128.0, "{} vs {level}", out[last]);
    assert!(out[blk(33).start..].iter().all(|&x| x == 0.0));
}

/// A voice that ended at gain 0 starts its next note as a fresh voice
/// does, with or without a VCA route.
#[test]
fn a_voice_ended_at_gain_0_is_fresh() {
    let p = long();
    let routed = mods(&[(ModSource::Env2, VCA, 127)]);
    let note = MidiNote::new(64).unwrap();
    let run = |v: &mut Voice, ms: &ModState| {
        v.note_on(note, Velocity::DEFAULT, &p);
        let mut out = Vec::new();
        let mut b = [0.0f32; BLOCK_SIZE];
        for _ in 0..8 {
            v.render(&mut b, &p, ms);
            out.extend_from_slice(&b);
        }
        out
    };
    for ms in [ModState::new(), routed.clone()] {
        // Note 60 until ENV 2 ends it, the engine still sounding (RR 1).
        let mut v = Voice::new(SR);
        v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p);
        let mut b = [0.0f32; BLOCK_SIZE];
        for i in 0..400 {
            if i == 20 {
                v.note_off();
            }
            v.render(&mut b, &p, &routed);
            if !v.is_active() {
                break;
            }
        }
        assert!(!v.is_active(), "ENV 2 ended it");
        let (got, want) = (run(&mut v, &ms), run(&mut Voice::new(SR), &ms));
        let diff = got
            .iter()
            .zip(&want)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            diff < 1e-4,
            "routed {}: max diff {diff}",
            ms.num_dests() > 0
        );
    }
}

/// A cycling burst ends after the burst running at key-up; a one-shot
/// burst ends after its burst, key held or not.
#[test]
fn bursts_end_when_their_burst_does() {
    let len = |fall: f32| {
        (chimera_core::dsp::modulator::law::BURST_LEN.at(fall) * SR as f32) as usize / BLOCK_SIZE
    };
    let p = with_env3(Func::Burst(EnvForm::Cycle), 0.2);
    let (_, alive) = life(&p, &mods(&[(ModSource::Env3, VCA, 127)]), 30, 200);
    let gone = alive.iter().position(|a| !a).expect("ends");
    assert!(
        gone > 30 && gone <= 30 + len(0.2) + 3,
        "cycle burst: {gone}"
    );
    let p = with_env3(Func::Burst(EnvForm::Ad), 0.2);
    let (_, alive) = life(&p, &mods(&[(ModSource::Env3, VCA, 127)]), usize::MAX, 200);
    let gone = alive
        .iter()
        .position(|a| !a)
        .expect("ends with the key held");
    assert!(gone <= len(0.2) + 3, "AD burst: {gone}");
}

/// Under every configuration an inactive engine ends the voice: a fast
/// engine release ends it while ENV 1 (60 s release) still holds.
#[test]
fn an_inactive_engine_always_ends_the_voice() {
    let mut p = init(EngineType::Algo);
    p.algo.ops[0].rr = 15;
    p.envelopes[0].speed = chimera_core::dsp::modulator::EnvSpeed::Slow;
    p.envelopes[0].release = 1.0;
    let (_, alive) = life(&p, &mods(&[(ModSource::Env1, VCA, 127)]), 20, 400);
    let gone = alive
        .iter()
        .position(|a| !a)
        .expect("the engine's end ends the voice");
    let mut env = Envelope::new();
    env.note_on();
    for b in 0..=gone {
        let mut g = [0.0f32; BLOCK_SIZE];
        env.run_block(
            &p.envelopes[0],
            &EnvMods::NONE,
            b < 20,
            SR,
            Some((&mut g, 1.0)),
        );
    }
    assert!(env.holds(false), "ENV 1 still held it at block {gone}");
}

/// Review Focus 5: the Sound switches engine mid-note; the fade keeps the
/// old VCA routes, stays finite, and the held note restarts on Modal.
#[test]
fn an_engine_switch_fades_with_the_old_vca_routes() {
    let algo = long();
    let modal = init(EngineType::Modal);
    let routed = mods(&[(ModSource::Env2, VCA, 127)]);
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &algo);
    let mut b = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    for _ in 0..30 {
        v.render(&mut b, &algo, &routed);
        peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    for i in 0..10 {
        v.render(&mut b, &modal, &ModState::new());
        assert!(b.iter().all(|x| x.is_finite()), "block {i}");
        if i < 2 {
            assert!(
                b.iter().all(|x| x.abs() <= peak * 1.01),
                "the fade never gets louder"
            );
        }
    }
    assert!(v.is_active(), "the held note restarted on Modal");
}

/// A voice an ENV → VCA release holds is not idle: it keeps its slot for
/// as long as the release sounds, and a new note takes another voice.
#[test]
fn a_release_the_vca_holds_is_not_idle() {
    use chimera_core::MidiChannel;
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
    use chimera_core::instrument::{AudioShared, Instrument};
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    let ev = |note, kind| NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind,
    };
    let mut shared = AudioShared::default();
    shared.parts[0].params = long();
    shared.parts[0].mod_state = mods(&[(ModSource::Env2, VCA, 127)]);
    // Note 60 released at block 20; another note at `other`. Per block:
    // (part 0 sounds, voice 0 still holds note 60).
    let run = |other: Option<usize>| {
        let mut inst = Box::new(Instrument::new(SR, SampleBudget::for_cpu(CPU_HZ_REV_V)));
        let mut fx = Box::new(FxBus::new());
        let mut out = Box::new(chimera_core::instrument::DacBlocks::new());
        let mut scope = common::scope_writer();
        inst.handle(ev(60, NoteKind::On(Velocity::DEFAULT)), &shared);
        let mut log = Vec::new();
        for b in 0..400 {
            if b == 20 {
                inst.handle(ev(60, NoteKind::Off), &shared);
            }
            if Some(b) == other {
                inst.handle(ev(64, NoteKind::On(Velocity::DEFAULT)), &shared);
                assert_eq!(inst.allocator().slots()[1].note(), MidiNote::new(64));
            }
            inst.render(&mut fx, &mut out, &shared, &mut scope);
            let v0 = inst.allocator().slots()[0];
            log.push((
                inst.part_bus(0).iter().any(|&x| x != 0.0),
                !v0.is_free() && v0.note() == MidiNote::new(60),
            ));
        }
        log
    };
    let alone = run(None);
    // `last`: the block in which the release reaches 0 and the voice ends.
    let last = alone.iter().rposition(|&(s, _)| s).unwrap();
    assert!(
        last > 22 && last < 399,
        "the release sounds past block 22, then ends"
    );
    assert!(
        alone[..last].iter().all(|&(_, kept)| kept),
        "held while audible"
    );
    assert!(!alone[last].1, "freed once silent");
    let both = run(Some(22));
    assert!(
        both[..last].iter().all(|&(_, kept)| kept),
        "not cut off by note 64"
    );
}
