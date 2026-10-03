//! The ALGO operator envelope graph (`viz::op_env`, #311) against the
//! envelope it pictures.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv, Stage};
use chimera_core::ui::{draw, theme, viz};

const SR: f32 = 48_000.0;

fn rates(ar: u8, d1r: u8, d1l: u8, d2r: u8, rr: u8) -> EnvRates {
    EnvRates {
        ar,
        d1r,
        d1l,
        d2r,
        rr,
        rs: 0,
    }
}

fn op_env(ar: u8, d1r: u8, d1l: u8, d2r: u8) -> ([f32; 4], [f32; 5]) {
    viz::op_env(rates(ar, d1r, d1l, d2r, 5))
}

/// A level as a graph height: the dB axis `op_env` draws on.
fn height(level: f32) -> f32 {
    if level > 0.0 {
        (1.0 + level.log2() / viz::OP_LEVEL_OCTAVES).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Step `e` while it is in `stage`, at most `max` seconds.
fn run_while(e: &mut OpEnv, stage: Stage, max: f32) {
    for _ in 0..(max * SR) as usize {
        if e.stage() != stage {
            break;
        }
        e.step();
    }
}

/// The breakpoints a stepped `OpEnv` reaches: the peak, the knee where D1
/// ends (or where it holds), D2 after `OP_D2_SECONDS`, then, released
/// there, the seconds the release takes to reach the graph's floor.
fn stepped(r: EnvRates) -> ([f32; 5], f32) {
    // Key code 0: no key scaling, as the graph.
    let note = MidiNote::new(21).unwrap();
    let mut e = OpEnv::IDLE;
    e.note_on(EnvCoefs::new(r, note, SR));
    run_while(&mut e, Stage::Attack, 30.0);
    let peak = e.level();
    run_while(&mut e, Stage::Decay1, 30.0);
    let knee = e.level();
    for _ in 0..(viz::OP_D2_SECONDS * SR) as usize {
        e.step();
    }
    let end = e.level();
    e.note_off();
    let floor = (-viz::OP_LEVEL_OCTAVES).exp2();
    let mut n = 0;
    while e.level() > floor && n < (30.0 * SR) as usize {
        e.step();
        n += 1;
    }
    (
        [0.0, height(peak), height(knee), height(end), 0.0],
        n as f32 / SR,
    )
}

/// The graph's breakpoints are where the DSP's envelope goes, edge cases
/// included: AR 0 never rises, D1R 0 holds at full, D1L 0 falls silent,
/// D1L 15 meets D1 at once so D2 runs even at D1R 0 (INIT), RR 15.
#[test]
fn op_env_breakpoints_follow_the_dsp() {
    let cases = [
        ("INIT, D2R turned", rates(31, 0, 15, 8, 5)),
        ("AR 0", rates(0, 12, 8, 4, 5)),
        ("D1R 0 holds", rates(31, 0, 8, 8, 5)),
        ("D1L 0", rates(31, 12, 0, 8, 5)),
        ("D1L 15, D1R 0, D2R 0", rates(31, 0, 15, 0, 5)),
        ("RR 15", rates(31, 12, 8, 8, 15)),
        ("mid", rates(20, 14, 10, 6, 7)),
    ];
    for (name, r) in cases {
        let (_, graph) = viz::op_env(r);
        let (dsp, _) = stepped(r);
        for (i, (g, d)) in graph.iter().zip(dsp).enumerate() {
            assert!(
                (g - d).abs() < 0.02,
                "{name}: breakpoint {i} graph {graph:?} dsp {dsp:?}"
            );
        }
    }
}

/// The R segment's share of the plot follows the DSP's release time: RR 15
/// (2 ms) sits at its least width, RR 1 (seconds) far wider.
#[test]
fn op_env_release_width_follows_the_dsp() {
    let (fast_w, _) = viz::op_env(rates(31, 12, 8, 0, 15));
    let (slow_w, _) = viz::op_env(rates(31, 12, 8, 0, 1));
    let (_, fast_t) = stepped(rates(31, 12, 8, 0, 15));
    let (_, slow_t) = stepped(rates(31, 12, 8, 0, 1));
    assert!(fast_t < 0.01 && slow_t > 1.0, "{fast_t} s, {slow_t} s");
    assert!(slow_w[3] > 3.0 * fast_w[3], "{fast_w:?} {slow_w:?}");
}

/// The widths fill the plot and follow the rates: AR 31 all but vertical,
/// D1L 15 a flat D1, D2R 0 a flat D2.
#[test]
fn op_env_follows_the_rates() {
    let (w, h) = op_env(31, 12, 15, 0);
    assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{w:?}");
    assert!(w[0] < 0.05, "AR 31 attacks at once: {w:?}");
    assert_eq!(h[1], h[2], "D1L 15: D1 stays at full");
    assert_eq!(h[2], h[3], "D2R 0 holds: {h:?}");
    assert_eq!((h[0], h[1], h[4]), (0.0, 1.0, 0.0));

    let (slow, _) = op_env(8, 12, 15, 0);
    assert!(slow[0] > 4.0 * w[0], "a slower attack is wider");
    let (_, knee) = op_env(31, 12, 8, 0);
    assert!(
        knee[2] > 0.0 && knee[2] < 1.0,
        "D1L sets the knee: {knee:?}"
    );
    let (_, sinking) = op_env(31, 12, 8, 10);
    assert!(sinking[3] < knee[3], "D2R sinks D2");
    let (d1, _) = op_env(31, 12, 8, 0);
    let (d1_fast, _) = op_env(31, 24, 8, 0);
    assert!(d1_fast[1] < d1[1], "a faster D1R is shorter");
}

/// Every stage but A has room for its label, however fast the rates.
#[test]
fn op_env_stages_have_room_for_labels() {
    let plot = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
    let label = |s: &str| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING) as f32;
    for r in [1, 16, 31] {
        let (widths, _) = op_env(31, r, 15, r);
        for (w, s) in widths.iter().zip(viz::OP_ENV_LABELS).skip(1) {
            let room = w * plot;
            assert!(room >= label(s), "rate {r}: {s} is {room} px");
        }
    }
}
