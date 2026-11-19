//! SMART EQ's block (ADR 0296): a lower role is cut where a higher role
//! plays, slowly, never stepped; FRONT and a quiet setting are untouched.

use chimera_engine::hw::MAX_PARTS;
use chimera_engine::instrument::Sides;
use chimera_engine::part_eq::PartEqs;
use chimera_engine::part_eq::dsp::smart::{SmartBlock, run};
use chimera_engine::part_eq::smart::{
    Amount, Band, CutDb, PartKind, Role, SmartEq, SmartState, Switch,
};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;
use core::f32::consts::PI;

const FS: u32 = 48_000;
type Buses = [[f32; BLOCK_SIZE]; MAX_PARTS];
/// A Part's signal at sample `n`, `None` while its bus is unwritten.
type Gen = Box<dyn Fn(usize) -> Option<f32>>;

const BASS: PartId = PartId::ALL[0];
const PAD: PartId = PartId::ALL[1];

fn sine(hz: f32, amp: f32) -> impl Fn(usize) -> f32 + Copy + 'static {
    move |n| amp * (2.0 * PI * hz * n as f32 / FS as f32).sin()
}

/// A FRONT bass at 100 Hz and a MID pad: 110 Hz under 2.5 kHz and 7 kHz.
fn clash() -> [Gen; MAX_PARTS] {
    let bass = sine(100.0, 0.3);
    let (a, b, c) = (sine(110.0, 0.2), sine(2_500.0, 0.05), sine(7_000.0, 0.05));
    core::array::from_fn(|p| -> Gen {
        match p {
            0 => Box::new(move |n| Some(bass(n))),
            1 => Box::new(move |n| Some(a(n) + b(n) + c(n))),
            _ => Box::new(|_| None),
        }
    })
}

fn roles(front: &[usize], back: &[usize]) -> [Role; MAX_PARTS] {
    core::array::from_fn(|p| {
        if front.contains(&p) {
            Role::Front
        } else if back.contains(&p) {
            Role::Back
        } else {
            Role::Mid
        }
    })
}

fn on(amount: u8) -> SmartEq {
    SmartEq::new(Switch::On, Amount::from_code(amount).unwrap())
}

/// One block from sample `start`: the buses in and out.
fn block(
    st: &mut SmartState,
    start: usize,
    gens: &[Gen; MAX_PARTS],
    roles: &[Role; MAX_PARTS],
    smart: SmartEq,
) -> (Buses, Buses) {
    block_kinds(st, start, gens, roles, &[PartKind::Tonal; MAX_PARTS], smart)
}

/// `block`, with each Part's kind (a DRUM Part keys the kick's dip).
fn block_kinds(
    st: &mut SmartState,
    start: usize,
    gens: &[Gen; MAX_PARTS],
    roles: &[Role; MAX_PARTS],
    kinds: &[PartKind; MAX_PARTS],
    smart: SmartEq,
) -> (Buses, Buses) {
    let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
    let mut written = [false; MAX_PARTS];
    for (p, g) in gens.iter().enumerate() {
        for (i, s) in buses[p].iter_mut().enumerate() {
            if let Some(v) = g(start + i) {
                *s = v;
                written[p] = true;
            }
        }
    }
    let input = buses;
    let mut sides: Sides = [None; MAX_PARTS];
    run(
        st,
        SmartBlock {
            buses: &mut buses,
            sides: &mut sides,
            written: &written,
            roles,
            kinds,
            smart,
            sample_rate: FS,
        },
    );
    (input, buses)
}

fn seconds(s: f32) -> usize {
    (s * FS as f32) as usize / BLOCK_SIZE
}

#[test]
fn a_front_bass_cuts_a_mid_pads_low_band_and_is_never_touched() {
    let mut st = PartEqs::REST.smart;
    let (gens, r) = (clash(), roles(&[0], &[]));
    for k in 0..seconds(4.0) {
        let (input, out) = block(&mut st, k * BLOCK_SIZE, &gens, &r, on(127));
        assert_eq!(
            input[BASS.index()].map(f32::to_bits),
            out[BASS.index()].map(f32::to_bits),
            "the FRONT bass is never written"
        );
    }
    let low = st.cut(PAD, Band::Low).get();
    // The pad sits ~3.5 dB under the bass there: 4 dB × (12 − 3.5) / 12.
    assert!(
        low > 2.0 && low <= 4.0 + 1e-3,
        "pad LOW cut {low} dB, one rank: up to 4 dB"
    );
    for b in [Band::HighMid, Band::High] {
        assert_eq!(st.cut(PAD, b), CutDb::ZERO, "nothing above plays in {b:?}");
    }
    for b in Band::ALL {
        assert_eq!(st.cut(BASS, b), CutDb::ZERO);
    }
    // Two ranks (BACK under FRONT) go deeper; half AMOUNT, half as deep.
    let mut back = PartEqs::REST.smart;
    let mut half = PartEqs::REST.smart;
    for k in 0..seconds(6.0) {
        block(
            &mut back,
            k * BLOCK_SIZE,
            &gens,
            &roles(&[0], &[1]),
            on(127),
        );
        block(&mut half, k * BLOCK_SIZE, &gens, &r, on(64));
    }
    let (b, h) = (
        back.cut(PAD, Band::Low).get(),
        half.cut(PAD, Band::Low).get(),
    );
    assert!(b > low && b <= 6.0 + 1e-3, "BACK {b} dB, MID {low} dB");
    assert!(
        (h / st.cut(PAD, Band::Low).get() - 64.0 / 127.0).abs() < 0.05,
        "half {h}"
    );
}

/// The amplitude of `hz` in `x` (a projection on its sine and cosine).
fn tone(x: &[f32], start: usize, hz: f32) -> f64 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate() {
        let w = 2.0 * std::f64::consts::PI * hz as f64 * (start + i) as f64 / FS as f64;
        s += *v as f64 * w.sin();
        c += *v as f64 * w.cos();
    }
    2.0 * (s * s + c * c).sqrt() / x.len() as f64
}

#[test]
fn the_pads_low_band_dips_its_110_hz_and_leaves_its_highs() {
    let mut st = PartEqs::REST.smart;
    let (gens, r) = (clash(), roles(&[0], &[]));
    let n = seconds(5.0);
    let (mut xi, mut xo) = (Vec::new(), Vec::new());
    for k in 0..n {
        let (i, o) = block(&mut st, k * BLOCK_SIZE, &gens, &r, on(127));
        if k >= n - seconds(1.0) {
            xi.extend_from_slice(&i[PAD.index()]);
            xo.extend_from_slice(&o[PAD.index()]);
        }
    }
    let start = (n - seconds(1.0)) * BLOCK_SIZE;
    let cut = st.cut(PAD, Band::Low).get() as f64;
    let db = |hz| 20.0 * (tone(&xo, start, hz) / tone(&xi, start, hz)).log10();
    let (lo, hi) = (db(110.0), db(7_000.0));
    assert!(
        lo < -0.7 * cut,
        "110 Hz down {lo:.2} dB for a {cut:.2} dB cut"
    );
    assert!(hi.abs() < 0.3, "7 kHz moved {hi:.2} dB");
}

#[test]
fn no_gain_moves_more_than_a_thousandth_of_a_db_a_sample() {
    const STEP_DB: f32 = 0.001;
    let mut st = PartEqs::REST.smart;
    let gens = clash();
    let r = roles(&[0], &[1]);
    let mut last = [[0.0f32; 4]; MAX_PARTS];
    let mut worst = 0.0f32;
    for k in 0..seconds(30.0) {
        // On full, off, on, AMOUNT jumping 127 ↔ 0: every jump a target's.
        let smart = match k / seconds(3.0) % 4 {
            0 => on(127),
            1 => SmartEq::OFF,
            2 => on(0),
            _ => on(127),
        };
        block(&mut st, k * BLOCK_SIZE, &gens, &r, smart);
        for p in PartId::ALL {
            for b in Band::ALL {
                let db = 20.0 * st.gain(p, b).log10();
                let step = (db - last[p.index()][b.index()]).abs() / BLOCK_SIZE as f32;
                worst = worst.max(step);
                last[p.index()][b.index()] = db;
            }
        }
    }
    assert!(worst > 0.0 && worst <= STEP_DB, "{worst} dB a sample");
}

#[test]
fn off_or_amount_zero_never_touches_a_bus_and_rests() {
    for smart in [
        SmartEq::OFF,
        on(0),
        SmartEq::new(Switch::Off, Amount::from_code(127).unwrap()),
    ] {
        let mut st = PartEqs::REST.smart;
        let (gens, r) = (clash(), roles(&[0], &[]));
        for k in 0..seconds(1.0) {
            let (i, o) = block(&mut st, k * BLOCK_SIZE, &gens, &r, smart);
            assert_eq!(
                i.map(|b| b.map(f32::to_bits)),
                o.map(|b| b.map(f32::to_bits))
            );
        }
        assert_eq!(st, SmartState::REST);
        assert!(!st.live());
    }
}

#[test]
fn switched_off_it_lets_go_slowly_then_rests() {
    let mut st = PartEqs::REST.smart;
    let (gens, r) = (clash(), roles(&[0], &[]));
    let mut k = 0;
    while k < seconds(4.0) {
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(127));
        k += 1;
    }
    let was = st.cut(PAD, Band::Low).get();
    block(&mut st, k * BLOCK_SIZE, &gens, &r, SmartEq::OFF);
    assert!(st.cut(PAD, Band::Low).get() > 0.99 * was, "no snap");
    let mut t = 0;
    while st.live() {
        k += 1;
        t += 1;
        block(&mut st, k * BLOCK_SIZE, &gens, &r, SmartEq::OFF);
        assert!(t < seconds(20.0), "rests within 20 s");
    }
    assert!(
        t > seconds(5.0),
        "released over {} s",
        t as f32 * BLOCK_SIZE as f32 / FS as f32
    );
    assert_eq!(st, SmartState::REST);
    let (i, o) = block(&mut st, k * BLOCK_SIZE, &gens, &r, SmartEq::OFF);
    assert_eq!(
        i.map(|b| b.map(f32::to_bits)),
        o.map(|b| b.map(f32::to_bits))
    );
}

#[test]
fn silence_after_sound_leaves_no_subnormals() {
    let mut st = PartEqs::REST.smart;
    let r = roles(&[0], &[]);
    let loud = clash();
    // Sound, then the buses still written but decaying to nothing.
    let fading: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        let g = sine(110.0 * (p + 1) as f32, 0.3);
        let start = seconds(3.0) * BLOCK_SIZE;
        Box::new(move |n| Some(g(n) * (-((n.saturating_sub(start)) as f32) / 2_000.0).exp()))
    });
    for k in 0..seconds(3.0) {
        block(&mut st, k * BLOCK_SIZE, &loud, &r, on(127));
    }
    for k in seconds(3.0)..seconds(25.0) {
        block(&mut st, k * BLOCK_SIZE, &fading, &r, on(127));
        for v in st.memory() {
            assert!(
                v == 0.0 || v.abs() >= f32::MIN_POSITIVE,
                "subnormal {v:e} at block {k}"
            );
        }
    }
}

#[test]
fn a_nan_on_a_bus_rests_only_that_parts_filters() {
    let mut st = PartEqs::REST.smart;
    let r = roles(&[0], &[]);
    let gens = clash();
    for k in 0..seconds(2.0) {
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(127));
    }
    let bad: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        let g = sine(110.0, 0.2);
        if p == 1 {
            Box::new(|_| Some(f32::NAN))
        } else {
            Box::new(move |n| Some(g(n)))
        }
    });
    block(&mut st, seconds(2.0) * BLOCK_SIZE, &bad, &r, on(127));
    for v in st.memory() {
        assert!(v.is_finite());
    }
    for b in Band::ALL {
        assert!(st.cut(PAD, b).get().is_finite());
    }
}

#[test]
fn a_ringing_tail_keeps_its_mud_cut_and_lets_go_smoothly() {
    // A MID chord struck every 4 s and left to ring (τ 1.5 s, as MODAL's
    // long tails) under a steady FRONT bass: while it rings within 12 dB
    // of the bass it keeps its LOW cut whole (owner, 2026-11-19: tails
    // muddy the mix); it lets go only as the ring falls far under.
    let bass = sine(100.0, 0.3);
    let ring = sine(110.0, 0.25);
    let strike = 4.0 * FS as f32;
    let gens: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        match p {
            0 => Box::new(move |n| Some(bass(n))),
            1 => Box::new(move |n| {
                let t = (n as f32 % strike) / FS as f32;
                Some(ring(n) * (-t / 1.5).exp())
            }),
            _ => Box::new(|_| None),
        }
    });
    let r = roles(&[0], &[]);
    let mut st = PartEqs::REST.smart;
    let mut low = Vec::new();
    for k in 0..seconds(12.0) {
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(127));
        low.push(st.cut(PAD, Band::Low).get());
    }
    let at = |s: f32| low[seconds(s)];
    // The third strike (8 s): its cut built, then its ring 3 s later
    // (−17 dB from the strike, ~13 dB under the bass) still holds most.
    assert!(at(9.0) > 3.5, "{} dB a second into the ring", at(9.0));
    assert!(
        at(11.0) > 0.75 * at(9.0),
        "{} dB 3 s into the ring",
        at(11.0)
    );
}

/// Part 1 a DRUM kit, FRONT: a kick on every beat at 120 BPM (a 120 → 50
/// Hz sweep, 150 ms) and a snare on every off-beat (180 and 330 Hz with a
/// noise burst, 80 ms); Part 2 a sine bass at 55 Hz, MID.
fn kick_and_bass(snare: bool) -> [Gen; MAX_PARTS] {
    let beat = FS as usize / 2;
    core::array::from_fn(|p| -> Gen {
        match p {
            0 => Box::new(move |n| {
                let t = (n % beat) as f32 / FS as f32;
                // The sweep's phase: ∫ 50 + 70 e^(−t/0.03) dt.
                let ph = 2.0 * PI * (50.0 * t + 70.0 * 0.03 * (1.0 - (-t / 0.03).exp()));
                let kick = 0.8 * (-t / 0.15).exp() * ph.sin();
                let u = ((n + beat / 2) % beat) as f32 / FS as f32;
                let noise = (((n as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / 8_388_608.0) - 1.0;
                let tone = (2.0 * PI * 180.0 * u).sin() + (2.0 * PI * 330.0 * u).sin();
                let sn = 0.4 * (-u / 0.08).exp() * (0.5 * tone + noise);
                Some(kick + if snare { sn } else { 0.0 })
            }),
            1 => Box::new(|n| Some(0.3 * (2.0 * PI * 55.0 * n as f32 / FS as f32).sin())),
            _ => Box::new(|_| None),
        }
    })
}

const DRUM_FIRST: [PartKind; MAX_PARTS] = {
    let mut k = [PartKind::Tonal; MAX_PARTS];
    k[0] = PartKind::Drum;
    k
};

/// Each block's kick dip on the bass and its slow LOW cut, and the bass out.
fn kicked(snare: bool, amount: u8) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut st = PartEqs::REST.smart;
    let gens = kick_and_bass(snare);
    let r = roles(&[0], &[]);
    let (mut dip, mut slow, mut out) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..seconds(8.0) {
        let (_, o) = block_kinds(&mut st, k * BLOCK_SIZE, &gens, &r, &DRUM_FIRST, on(amount));
        dip.push(st.kick(PAD).get());
        slow.push(st.slow_cut(PAD, Band::Low).get());
        out.extend_from_slice(&o[PAD.index()]);
        // The dip is LOW's alone; above, only the slow rule's hair of a
        // cut where a 55 Hz sine's skirt meets the snare.
        for b in [Band::LowMid, Band::HighMid, Band::High] {
            assert!(st.cut(PAD, b).get() < 0.3, "{b:?} {:?}", st.cut(PAD, b));
        }
    }
    (dip, slow, out)
}

#[test]
fn a_front_kick_dips_the_bass_low_band_on_each_hit_and_lets_go() {
    let (dip, slow, _) = kicked(true, 127);
    let ms = |m: f32| (m * FS as f32 / 1000.0) as usize / BLOCK_SIZE;
    let beat = ms(500.0);
    // Settled bars: the fourth second on.
    for kick in (4..8).map(|s| s * 2 * beat) {
        let peak = dip[kick..kick + ms(30.0)].iter().fold(0.0f32, |m, &d| m.max(d));
        assert!(peak > 3.0, "{peak} dB within 30 ms of the kick");
        // Its first 10 ms already well in.
        assert!(dip[kick + ms(10.0)] > 0.5 * peak, "{} dB at 10 ms", dip[kick + ms(10.0)]);
        // Let go by the snare, and the snare does not dip it again.
        let after = dip[kick + ms(240.0)];
        assert!(after < 0.3 * peak, "{after} dB at 240 ms of {peak}");
        let snare = dip[kick + ms(260.0)..kick + ms(490.0)].iter().fold(0.0f32, |m, &d| m.max(d));
        assert!(snare < 0.3 * peak, "{snare} dB under the snare of {peak}");
    }
    // The kick never cuts the bass's LOW slowly: only its hits dip it.
    assert!(slow.iter().all(|&c| c == 0.0));
}

#[test]
fn the_kick_dip_never_clicks_on_a_sine_bass() {
    // The bass out, high-passed at 2 kHz (four TPT one-poles): a click or
    // a stepped gain shows there; a 55 Hz sine's gain moving smoothly
    // does not. Against the bass with SMART EQ off.
    fn above_2k(x: &[f32]) -> f64 {
        let g = (PI * 2_000.0 / FS as f32).tan();
        let g = g / (1.0 + g);
        let mut s = [0.0f32; 4];
        let mut e = 0.0f64;
        for &v in x {
            let mut y = v;
            for st in s.iter_mut() {
                let w = (y - *st) * g;
                let lp = w + *st;
                *st = lp + w;
                y -= lp;
            }
            e += (y as f64).powi(2);
        }
        e / x.len() as f64
    }
    let (_, _, plain) = kicked(true, 0);
    let (dip, _, out) = kicked(true, 127);
    assert!(dip.iter().fold(0.0f32, |m, &d| m.max(d)) > 3.0);
    let skip = FS as usize;
    let (base, dipped) = (above_2k(&plain[skip..]), above_2k(&out[skip..]));
    let total = plain[skip..].iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / (plain.len() - skip) as f64;
    let db = 10.0 * (dipped / total).log10();
    let plain_db = 10.0 * (base / total).log10();
    // The filters' own residue of a 55 Hz sine sits near −84 dB; a click
    // would lift it by tens of dB.
    assert!(db < plain_db + 3.0, "above 2 kHz {db:.1} dB under the bass, plain {plain_db:.1}");
}

#[test]
fn the_kick_dip_moves_fast_but_smoothly_and_the_rest_stay_slow() {
    let (dip, _, _) = kicked(true, 127);
    let worst = dip
        .windows(2)
        .map(|w| (w[1] - w[0]).abs() / BLOCK_SIZE as f32)
        .fold(0.0f32, f32::max);
    // The fast LOW: under 0.05 dB a sample (a 4 dB dip over two blocks).
    assert!(worst > 0.001 && worst < 0.05, "{worst} dB a sample");
}
