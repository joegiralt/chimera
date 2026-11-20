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
            wash: Role::Back,
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
        let (input, out) = block(&mut st, k * BLOCK_SIZE, &gens, &r, on(64));
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
    // Two ranks (BACK under FRONT) go deeper; AMOUNT's curve goes on past
    // polite: DEEP (95) twice as deep in LOW, OVER (127) three and a half
    // times.
    let cut_at = |amount: u8, r: &[Role; MAX_PARTS]| {
        let mut st = PartEqs::REST.smart;
        for k in 0..seconds(8.0) {
            block(&mut st, k * BLOCK_SIZE, &gens, r, on(amount));
        }
        st.cut(PAD, Band::Low).get()
    };
    let polite = cut_at(64, &r);
    let b = cut_at(64, &roles(&[0], &[1]));
    assert!(
        b > polite && b <= 6.0 + 1e-3,
        "BACK {b} dB, MID {polite} dB"
    );
    let (deep, over) = (cut_at(95, &r), cut_at(127, &r));
    assert!((deep / polite - 2.0).abs() < 0.1, "DEEP {deep} of {polite}");
    assert!(
        (over / polite - 3.5).abs() < 0.15,
        "OVER {over} of {polite}"
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
        let (i, o) = block(&mut st, k * BLOCK_SIZE, &gens, &r, on(64));
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
            0 => on(64),
            1 => SmartEq::OFF,
            2 => on(0),
            _ => on(64),
        };
        block(&mut st, k * BLOCK_SIZE, &gens, &r, smart);
        for p in PartId::ALL {
            for b in Band::ALL {
                let db = 20.0 * (1.0 + st.dip(p, b)).log10();
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
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(64));
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
        block(&mut st, k * BLOCK_SIZE, &loud, &r, on(64));
    }
    for k in seconds(3.0)..seconds(25.0) {
        block(&mut st, k * BLOCK_SIZE, &fading, &r, on(64));
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
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(64));
    }
    let bad: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        let g = sine(110.0, 0.2);
        if p == 1 {
            Box::new(|_| Some(f32::NAN))
        } else {
            Box::new(move |n| Some(g(n)))
        }
    });
    block(&mut st, seconds(2.0) * BLOCK_SIZE, &bad, &r, on(64));
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
        block(&mut st, k * BLOCK_SIZE, &gens, &r, on(64));
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
                let noise =
                    (((n as u32).wrapping_mul(2_654_435_761) >> 8) as f32 / 8_388_608.0) - 1.0;
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
    let (dip, slow, _) = kicked(true, 64);
    let ms = |m: f32| (m * FS as f32 / 1000.0) as usize / BLOCK_SIZE;
    let beat = ms(500.0);
    // Settled bars: the fourth second on.
    for kick in (4..8).map(|s| s * 2 * beat) {
        let peak = dip[kick..kick + ms(30.0)]
            .iter()
            .fold(0.0f32, |m, &d| m.max(d));
        assert!(peak > 3.0, "{peak} dB within 30 ms of the kick");
        // Its first 10 ms already well in.
        assert!(
            dip[kick + ms(10.0)] > 0.5 * peak,
            "{} dB at 10 ms",
            dip[kick + ms(10.0)]
        );
        // Let go by the snare, and the snare does not dip it again.
        let after = dip[kick + ms(240.0)];
        assert!(after < 0.3 * peak, "{after} dB at 240 ms of {peak}");
        let snare = dip[kick + ms(260.0)..kick + ms(490.0)]
            .iter()
            .fold(0.0f32, |m, &d| m.max(d));
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
    let (dip, _, out) = kicked(true, 64);
    assert!(dip.iter().fold(0.0f32, |m, &d| m.max(d)) > 3.0);
    let skip = FS as usize;
    let (base, dipped) = (above_2k(&plain[skip..]), above_2k(&out[skip..]));
    let total = plain[skip..]
        .iter()
        .map(|&v| (v as f64).powi(2))
        .sum::<f64>()
        / (plain.len() - skip) as f64;
    let db = 10.0 * (dipped / total).log10();
    let plain_db = 10.0 * (base / total).log10();
    // The filters' own residue of a 55 Hz sine sits near −84 dB; a click
    // would lift it by tens of dB.
    assert!(
        db < plain_db + 3.0,
        "above 2 kHz {db:.1} dB under the bass, plain {plain_db:.1}"
    );
}

#[test]
fn the_kick_dip_moves_fast_but_smoothly_and_the_rest_stay_slow() {
    let (dip, _, _) = kicked(true, 64);
    let worst = dip
        .windows(2)
        .map(|w| (w[1] - w[0]).abs() / BLOCK_SIZE as f32)
        .fold(0.0f32, f32::max);
    // The fast LOW: under 0.05 dB a sample (a 4 dB dip over two blocks).
    assert!(worst > 0.001 && worst < 0.05, "{worst} dB a sample");
}

/// A FRONT DRUM Part's kick every `gap` samples (a 120 → 50 Hz sweep,
/// 60 ms) over a 55 Hz sine bass, MID: each block's kick dip on the bass.
fn kicks_every(gap: usize, secs: f32) -> Vec<f32> {
    kicks_at(gap, secs, 64)
}

/// `kicks_every` at AMOUNT `amount`.
fn kicks_at(gap: usize, secs: f32, amount: u8) -> Vec<f32> {
    let gens: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        match p {
            0 => Box::new(move |n| {
                let t = (n % gap) as f32 / FS as f32;
                let ph = 2.0 * PI * (50.0 * t + 70.0 * 0.03 * (1.0 - (-t / 0.03).exp()));
                Some(0.8 * (-t / 0.06).exp() * ph.sin())
            }),
            1 => Box::new(|n| Some(0.3 * (2.0 * PI * 55.0 * n as f32 / FS as f32).sin())),
            _ => Box::new(|_| None),
        }
    });
    let mut st = PartEqs::REST.smart;
    let r = roles(&[0], &[]);
    (0..seconds(secs))
        .map(|k| {
            block_kinds(&mut st, k * BLOCK_SIZE, &gens, &r, &DRUM_FIRST, on(amount));
            st.kick(PAD).get()
        })
        .collect()
}

#[test]
fn dense_kicks_hold_one_steady_dip_and_sparse_ones_each_let_go() {
    // 16ths at 170 BPM, 88 ms apart: closer than the dip lets go. Held,
    // not fluttering at 11 Hz (1.8 dB peak to peak before the hold).
    let dense = kicks_every(FS as usize * 60 / 170 / 4, 6.0);
    let tail = &dense[seconds(3.0)..];
    let (lo, hi) = tail
        .iter()
        .fold((f32::MAX, 0.0f32), |(l, h), &d| (l.min(d), h.max(d)));
    assert!(hi > 3.0, "the dip holds: {hi} dB");
    assert!(
        hi - lo < 0.5,
        "{} dB peak to peak under 16ths at 170 BPM",
        hi - lo
    );
    // Quarter notes at 120 BPM: each kick dips and is let go before the
    // next.
    let beat = FS as usize / 2;
    let sparse = kicks_every(beat, 6.0);
    for kick in 6..11 {
        let at = kick * beat / BLOCK_SIZE;
        let bar = &sparse[at..at + beat / BLOCK_SIZE];
        let peak = bar.iter().fold(0.0f32, |m, &d| m.max(d));
        let before_next = bar[bar.len() - 2];
        assert!(peak > 3.0, "{peak} dB");
        assert!(
            before_next < 0.05 * peak,
            "{before_next} dB left of {peak} before the next kick"
        );
    }
}

/// One block: Part 1 a FRONT sine at 100 Hz, Part 2 a MID pad at 110 Hz,
/// each with `side` beside its bus.
fn sided(st: &mut SmartState, start: usize, side: Option<&dyn Fn(usize) -> f32>) -> Sides {
    let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
    let mut sides: Sides = [None; MAX_PARTS];
    for p in 0..2 {
        let g = sine(100.0 + 10.0 * p as f32, 0.3);
        buses[p] = core::array::from_fn(|i| g(start + i));
        sides[p] = side.map(|f| core::array::from_fn(|i| f(start + i)));
    }
    let mut written = [false; MAX_PARTS];
    (written[0], written[1]) = (true, true);
    run(
        st,
        SmartBlock {
            buses: &mut buses,
            sides: &mut sides,
            written: &written,
            roles: &roles(&[0], &[]),
            kinds: &[PartKind::Tonal; MAX_PARTS],
            wash: Role::Back,
            smart: on(64),
            sample_rate: FS,
        },
    );
    sides
}

#[test]
fn a_side_takes_only_the_bands_that_cut_and_a_listened_part_s_none() {
    let mut st = PartEqs::REST.smart;
    let low = sine(105.0, 0.1);
    let mut last = [None; MAX_PARTS];
    for k in 0..seconds(4.0) {
        last = sided(&mut st, k * BLOCK_SIZE, Some(&low));
        // The FRONT Part is only listened to: its side is never written
        // and its side filters hold nothing.
        let front: [f32; BLOCK_SIZE] = core::array::from_fn(|i| low(k * BLOCK_SIZE + i));
        assert_eq!(last[0].unwrap().map(f32::to_bits), front.map(f32::to_bits));
        assert!(st.side_rests(BASS));
    }
    // The pad's LOW cuts: its side's 105 Hz is cut with it.
    assert!(st.cut(PAD, Band::Low).get() > 2.0);
    assert!(!st.side_rests(PAD));
    let start = (seconds(4.0) - 1) * BLOCK_SIZE;
    let dry: [f32; BLOCK_SIZE] = core::array::from_fn(|i| low(start + i));
    let (wet, dry) = (rms(&last[1].unwrap()), rms(&dry));
    assert!(
        wet < 0.85 * dry,
        "the side's low cut with the bus: {wet} of {dry}"
    );
    // No pad panned (no side this block): its filters rest.
    for k in seconds(4.0)..seconds(4.0) + 4 {
        sided(&mut st, k * BLOCK_SIZE, None);
    }
    assert!(st.side_rests(PAD));
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn a_part_hears_the_same_listened_to_or_cut() {
    // The pad cut by a FRONT bass heard decimated (the bass is only
    // listened to), against the same bass heard at the full rate (cut
    // itself, by a kit above it it never hears): the pad's cut agrees.
    let gens = clash();
    let mut thin = PartEqs::REST.smart;
    for k in 0..seconds(4.0) {
        block(&mut thin, k * BLOCK_SIZE, &gens, &roles(&[0], &[]), on(64));
    }
    // HIGH's level through half the rate: a 6.4 kHz tone on a FRONT Part
    // cuts a MID Part's HIGH as a FRONT 100 Hz tone cuts its LOW.
    let hi: [Gen; MAX_PARTS] = core::array::from_fn(|p| -> Gen {
        let (a, b) = (sine(6_400.0, 0.3), sine(6_000.0, 0.2));
        match p {
            0 => Box::new(move |n| Some(a(n))),
            1 => Box::new(move |n| Some(b(n))),
            _ => Box::new(|_| None),
        }
    });
    let mut high = PartEqs::REST.smart;
    for k in 0..seconds(4.0) {
        block(&mut high, k * BLOCK_SIZE, &hi, &roles(&[0], &[]), on(64));
    }
    let (lo, hi) = (
        thin.cut(PAD, Band::Low).get(),
        high.cut(PAD, Band::High).get(),
    );
    // LOW's reach is the mud's (full to 12 dB under), HIGH's the clear
    // bands' (3.5 dB under: 4 dB × 8.5 / 12).
    assert!(lo > 3.9, "{lo}");
    assert!((hi - 4.0 * 8.5 / 12.0).abs() < 0.35, "{hi}");
}

/// The real bands' response to a tone at `hz` under dips `d`, as the
/// block's own filters run them: its gain, real and imaginary.
fn measured(d: &[f32; 4], hz: f32) -> (f64, f64) {
    use chimera_engine::part_eq::dsp::smart::probe;
    // Settle, then project over a whole number of the tone's periods (at
    // least a tenth of a second): a part period biases the projection,
    // most at the low end.
    let period = FS as f64 / hz as f64;
    let span = ((0.1 * FS as f64 / period).ceil().max(4.0) * period).round() as usize;
    let settle = FS as usize / 8;
    let n = (settle + span).div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    let w = |i: usize| 2.0 * std::f64::consts::PI * hz as f64 * i as f64 / FS as f64;
    let mut x: Vec<f32> = (0..n).map(|i| w(i).sin() as f32).collect();
    probe(d, &mut x, FS);
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate().skip(n - span) {
        re += *v as f64 * w(i).sin();
        im += *v as f64 * w(i).cos();
    }
    let half = span as f64 / 2.0;
    (re / half, im / half)
}

fn measured_db(cuts: [f32; 4], hz: f32) -> f64 {
    use chimera_engine::part_eq::dsp::smart::dips;
    let (re, im) = measured(&dips(&cuts.map(CutDb::new)), hz);
    10.0 * (re * re + im * im).log10()
}

/// Prints `A[c][b] = Re B_b(f_c)` of the filters as they run, whose
/// inverse is `UNMIX`: `cargo test -p chimera-engine --test
/// smart_eq_dsp_test -- --ignored --nocapture unmix`.
#[test]
#[ignore]
fn print_the_unmix_matrix_s_source() {
    for c in Band::ALL {
        let row: Vec<String> = Band::ALL
            .iter()
            .map(|b| {
                let mut d = [0.0f32; 4];
                d[b.index()] = -0.5;
                let (re, im) = measured(&d, c.centre().get());
                format!("({:.5}, {:.5})", (re - 1.0) / -0.5, im / -0.5)
            })
            .collect();
        println!("[{}],", row.join(", "));
    }
}

#[test]
fn the_real_bands_cut_what_the_page_reads() {
    // The filters as they run (LOW and LOW MID state-variable, HIGH MID
    // and HIGH direct form), not their ideal: each band alone at 6 dB
    // reads −6 dB at its centre; all four at 6 dB is −6 dB across the
    // band, not the raw sum's −8.8 dB at 428 Hz.
    for b in Band::ALL {
        let mut cuts = [0.0; 4];
        cuts[b.index()] = 6.0;
        let at = measured_db(cuts, b.centre().get());
        assert!(
            (at + 6.0).abs() < 0.5,
            "{b:?} alone reads {at:.2} dB at its centre"
        );
    }
    for hz in [100.0, 200.0, 428.0, 800.0, 1_600.0, 3_200.0, 6_400.0] {
        let at = measured_db([6.0; 4], hz);
        assert!(
            at > -6.15 && at < -4.0,
            "all four at 6 dB: {at:.2} dB at {hz} Hz"
        );
    }
    // Unity, to the projection's own error.
    assert!(measured_db([0.0; 4], 428.0).abs() < 1e-3);
}

/// How the bands' sum sits against what was asked, over a set of cuts.
#[derive(Debug, Default)]
struct Fit {
    /// The most the sum is deeper than the deepest cut asked, dB.
    deeper: f64,
    /// The most it is over unity anywhere, dB, and where.
    over: f64,
    over_hz: f32,
    /// The most a readout (`heard`) is off the filters at its centre, dB.
    readout: f64,
    /// The most a band cut alone falls short of its asked depth, dB.
    short: f64,
}

#[test]
fn the_bands_sum_against_the_cuts_asked_on_a_fine_grid() {
    use chimera_engine::part_eq::dsp::smart::{dips, heard};
    // Each band's response as it runs, at 160 points from 20 Hz to 20 kHz
    // and at the four centres: the sum is linear in the dips, so every
    // combination of cuts follows. The cuts are AMOUNT's knots' (one and
    // two ranks), the kick's dip in LOW among them.
    let mut grid: Vec<f32> = (0..160)
        .map(|i| 20.0 * 1000f32.powf(i as f32 / 159.0))
        .collect();
    grid.extend(Band::ALL.map(|b| b.centre().get()));
    let bands: Vec<[(f64, f64); 4]> = grid
        .iter()
        .map(|&hz| {
            Band::ALL.map(|b| {
                let mut d = [0.0f32; 4];
                d[b.index()] = -0.5;
                let (re, im) = measured(&d, hz);
                ((re - 1.0) / -0.5, im / -0.5)
            })
        })
        .collect();
    let db = |d: &[f32; 4], at: usize| {
        let (mut re, mut im) = (1.0, 0.0);
        for (b, (r, i)) in bands[at].iter().enumerate() {
            re += d[b] as f64 * r;
            im += d[b] as f64 * i;
        }
        10.0 * (re * re + im * im).log10()
    };
    let fit = |low: &[f32], low_mid: &[f32], clear: &[f32]| {
        let mut f = Fit::default();
        for &a in low {
            for &b in low_mid {
                for &c in clear {
                    for &e in clear {
                        let cuts = [a, b, c, e];
                        let d = dips(&cuts.map(CutDb::new));
                        let most = cuts.iter().fold(0.0f32, |m, &c| m.max(c)) as f64;
                        for (at, &hz) in grid.iter().enumerate() {
                            let g = db(&d, at);
                            f.deeper = f.deeper.max(-g - most);
                            if g > f.over {
                                (f.over, f.over_hz) = (g, hz);
                            }
                        }
                        let read = heard(&d);
                        for band in Band::ALL {
                            let g = db(&d, 160 + band.index());
                            let r = read[band.index()].get() as f64;
                            f.readout = f.readout.max((g.min(0.0) + r).abs());
                            let alone = cuts.iter().filter(|c| **c > 0.0).count() == 1;
                            if alone && cuts[band.index()] > 0.0 {
                                f.short = f.short.max(g + cuts[band.index()] as f64);
                            }
                        }
                    }
                }
            }
        }
        f
    };
    let polite = fit(&[0.0, 4.0, 6.0], &[0.0, 4.0, 6.0], &[0.0, 4.0, 6.0]);
    let deep = fit(
        &[0.0, 8.0, 10.0, 12.0, 15.0],
        &[0.0, 8.0, 12.0],
        &[0.0, 4.0, 6.0],
    );
    let over = fit(
        &[0.0, 14.0, 18.0, 21.0, 27.0],
        &[0.0, 14.0, 21.0],
        &[0.0, 6.0, 9.0],
    );
    println!("polite {polite:?}\ndeep {deep:?}\nover {over:?}");
    // The measured bounds (ADR 0296 § 6), each with a little room.
    for (name, f, deeper, boost, readout, short) in [
        ("polite", &polite, DEEPER[0], BOOST[0], READOUT[0], SHORT[0]),
        ("deep", &deep, DEEPER[1], BOOST[1], READOUT[1], SHORT[1]),
        ("over", &over, DEEPER[2], BOOST[2], READOUT[2], SHORT[2]),
    ] {
        assert!(
            f.deeper < deeper,
            "{name}: {:.3} dB deeper than asked",
            f.deeper
        );
        assert!(
            f.over < boost,
            "{name}: {:.3} dB over unity at {} Hz",
            f.over,
            f.over_hz
        );
        assert!(
            f.readout < readout,
            "{name}: readout {:.3} dB off the filters",
            f.readout
        );
        assert!(
            f.short < short,
            "{name}: a band alone {:.3} dB short",
            f.short
        );
    }
    // Between the knots: every pair of bands, each from 0 to 21 dB by
    // 0.5 (the cuts move through every depth on their way).
    let mut pairs = Fit::default();
    let steps: Vec<f32> = (0..=42).map(|i| 0.5 * i as f32).collect();
    for x in Band::ALL {
        for y in Band::ALL {
            if x.index() >= y.index() {
                continue;
            }
            for &cx in &steps {
                for &cy in &steps {
                    let mut cuts = [0.0f32; 4];
                    (cuts[x.index()], cuts[y.index()]) = (cx, cy);
                    let d = dips(&cuts.map(CutDb::new));
                    for (at, &hz) in grid.iter().enumerate() {
                        let g = db(&d, at);
                        pairs.deeper = pairs.deeper.max(-g - cx.max(cy) as f64);
                        if g > pairs.over {
                            (pairs.over, pairs.over_hz) = (g, hz);
                        }
                    }
                }
            }
        }
    }
    println!("pairs {pairs:?}");
    assert!(pairs.deeper < PAIRS_DEEPER, "{:.3} dB deeper", pairs.deeper);
    assert!(
        pairs.over < PAIRS_BOOST,
        "{:.3} dB over at {} Hz",
        pairs.over,
        pairs.over_hz
    );
    // The lift is not only between LOW MID and HIGH MID: under LOW too.
    let mut cuts = [0.0f32; 4];
    cuts[Band::LowMid.index()] = 21.0;
    let d = dips(&cuts.map(CutDb::new));
    let under = (0..160)
        .filter(|&at| grid[at] < 60.0)
        .map(|at| db(&d, at))
        .fold(0.0, f64::max);
    println!("under 60 Hz, LOW MID at 21 dB: {under:.3} dB");
    assert!(under < PAIRS_BOOST);
    // No cut is no dip, exactly.
    assert_eq!(dips(&[CutDb::ZERO; 4]), [0.0; 4]);
}

/// Polite, DEEP, OVER: the fine grid's bounds. Measured: the sum 0.05,
/// 0.01, 0.003 dB deeper than asked; 0.27, 0.46, 0.56 dB over unity
/// between two bands (500 to 650 Hz), where a cut's neighbour is lifted
/// to hold its own centre; a readout within 0.001 dB of the filters; a
/// band cut alone 0.01, 0.14, 2.3 dB short (LOW asked 27 dB reaches 24.7:
/// its neighbours' quarter-turn parts hold it up).
const DEEPER: [f64; 3] = [0.1, 0.1, 0.1];
const BOOST: [f64; 3] = [0.3, 0.5, 0.6];
const READOUT: [f64; 3] = [0.01, 0.01, 0.01];
const SHORT: [f64; 3] = [0.05, 0.2, 2.4];
/// Any two bands at any depths to 21 dB by 0.5: measured 0.10 dB deeper
/// than the deeper asked, 0.56 dB over unity (at 1.76 kHz; 0.39 under
/// 60 Hz with LOW MID at 21 dB).
const PAIRS_DEEPER: f64 = 0.15;
const PAIRS_BOOST: f64 = 0.6;

#[test]
fn amount_s_curve_passes_its_knots_and_only_ever_deepens() {
    use chimera_engine::part_eq::dsp::smart::{KNOTS, caps_at};
    use chimera_engine::part_eq::smart::Caps;
    let at = |a: u8| caps_at(on(a));
    for (amount, caps) in KNOTS {
        let got = if amount.code() == 0 {
            Caps::NONE
        } else {
            at(amount.code())
        };
        for (g, w) in [
            (got.mud, caps.mud),
            (got.clear, caps.clear),
            (got.kick, caps.kick),
        ] {
            assert!(
                (g.get() - w.get()).abs() < 1e-4,
                "AMOUNT {}: {g:?} for {w:?}",
                amount.code()
            );
        }
    }
    // Polite at 64, DEEP at 95 (75 % of the slider), OVER at 127: BACK
    // under FRONT; one rank is 2/3.
    assert_eq!(at(64).mud, CutDb::new(6.0));
    assert_eq!(
        (at(95).mud, at(95).clear, at(95).kick),
        (CutDb::new(12.0), CutDb::new(6.0), CutDb::new(15.0))
    );
    assert_eq!((at(127).mud, at(127).kick), (CutDb::new(21.0), CutDb::MAX));
    // More AMOUNT is never a shallower cut, and never a jump.
    for a in 1..=127u8 {
        let (lo, hi) = (at(a - 1), at(a));
        let lo = if a == 1 { Caps::NONE } else { lo };
        for (l, h) in [(lo.mud, hi.mud), (lo.clear, hi.clear), (lo.kick, hi.kick)] {
            assert!(h.get() >= l.get() - 1e-5, "AMOUNT {a}");
            assert!(
                h.get() - l.get() < 0.5,
                "AMOUNT {a} jumps {} dB",
                h.get() - l.get()
            );
        }
    }
    assert_eq!(caps_at(SmartEq::OFF), Caps::NONE);
}

/// One block: Part 1 (`role`, a kit if `kit`) plays `lead`, and the
/// reverb returns `wash` on both sides (the right at half the left) in
/// role `wash_role`. The return out.
fn washed(
    st: &mut SmartState,
    start: usize,
    (lead, role, kit): (&dyn Fn(usize) -> f32, Role, bool),
    (wash, wash_role): (Option<&dyn Fn(usize) -> f32>, Role),
) -> chimera_engine::dsp::Stereo {
    use chimera_engine::dsp::Stereo;
    use chimera_engine::part_eq::dsp::smart::run_wash;
    use chimera_engine::part_eq::smart::Returned;
    let mut buses: Buses = [[0.0; BLOCK_SIZE]; MAX_PARTS];
    for (i, s) in buses[0].iter_mut().enumerate() {
        *s = lead(start + i);
    }
    let mut written = [false; MAX_PARTS];
    written[0] = true;
    let mut roles = [Role::Mid; MAX_PARTS];
    roles[0] = role;
    let mut kinds = [PartKind::Tonal; MAX_PARTS];
    if kit {
        kinds[0] = PartKind::Drum;
    }
    let mut sides: Sides = [None; MAX_PARTS];
    run(
        st,
        SmartBlock {
            buses: &mut buses,
            sides: &mut sides,
            written: &written,
            roles: &roles,
            kinds: &kinds,
            wash: wash_role,
            smart: on(64),
            sample_rate: FS,
        },
    );
    let mut wet = Stereo::SILENT;
    if let Some(w) = wash {
        for i in 0..BLOCK_SIZE {
            wet.l[i] = w(start + i);
            wet.r[i] = 0.5 * w(start + i);
        }
    }
    let reverb = if wash.is_some() {
        Returned::Sounding
    } else {
        Returned::Rests
    };
    run_wash(st, &mut wet, reverb, FS);
    wet
}

#[test]
fn the_reverb_s_return_yields_as_a_part_of_its_role_and_makes_nothing_yield() {
    // A FRONT lead at 440 Hz; the return (BACK) washes 400 Hz and 7 kHz.
    let lead = sine(440.0, 0.3);
    let (a, b) = (sine(400.0, 0.1), sine(7_000.0, 0.03));
    let wash = move |n: usize| a(n) + b(n);
    let mut st = PartEqs::REST.smart;
    let mut out = chimera_engine::dsp::Stereo::SILENT;
    for k in 0..seconds(5.0) {
        out = washed(
            &mut st,
            k * BLOCK_SIZE,
            (&lead, Role::Front, false),
            (Some(&wash), Role::Back),
        );
    }
    let cut = st.wash_cut(Band::LowMid).get();
    assert!(
        cut > 5.0 && cut <= 6.0 + 1e-3,
        "BACK under FRONT in LOW MID: {cut} dB"
    );
    assert_eq!(
        st.wash_cut(Band::High),
        CutDb::ZERO,
        "the lead plays nothing up there"
    );
    // Left and right take the same cut: the right stays half the left.
    let start = (seconds(5.0) - 1) * BLOCK_SIZE;
    let dry: [f32; BLOCK_SIZE] = core::array::from_fn(|i| wash(start + i));
    assert!(
        rms(&out.l) < 0.8 * rms(&dry),
        "{} of {}",
        rms(&out.l),
        rms(&dry)
    );
    for (l, r) in out.l.iter().zip(&out.r) {
        assert!((r - 0.5 * l).abs() < 1e-6);
    }
    // The lead is never cut, whatever the return's role or level.
    for b in Band::ALL {
        assert_eq!(st.cut(BASS, b), CutDb::ZERO);
    }
    // A FRONT return over a BACK Part: nothing yields to it, and it is
    // read, never written.
    let mut st = PartEqs::REST.smart;
    for k in 0..seconds(3.0) {
        let out = washed(
            &mut st,
            k * BLOCK_SIZE,
            (&lead, Role::Back, false),
            (Some(&wash), Role::Front),
        );
        let dry: [f32; BLOCK_SIZE] = core::array::from_fn(|i| wash(k * BLOCK_SIZE + i));
        assert_eq!(out.l.map(f32::to_bits), dry.map(f32::to_bits));
    }
    for b in Band::ALL {
        assert_eq!(
            st.cut(BASS, b),
            CutDb::ZERO,
            "a Part never yields to the return"
        );
        assert_eq!(st.wash_cut(b), CutDb::ZERO);
    }
}

#[test]
fn the_kick_dips_the_return_s_low_and_a_reverb_at_rest_is_left_alone() {
    // A FRONT kit's kick on the beat; the return rumbles at 70 Hz.
    let beat = FS as usize / 2;
    let kick = move |n: usize| {
        let t = (n % beat) as f32 / FS as f32;
        let ph = 2.0 * PI * (50.0 * t + 70.0 * 0.03 * (1.0 - (-t / 0.03).exp()));
        0.8 * (-t / 0.06).exp() * ph.sin()
    };
    let rumble = sine(70.0, 0.2);
    let mut st = PartEqs::REST.smart;
    let mut most = 0.0f32;
    let mut least = f32::MAX;
    for k in 0..seconds(6.0) {
        washed(
            &mut st,
            k * BLOCK_SIZE,
            (&kick, Role::Front, true),
            (Some(&rumble), Role::Back),
        );
        if k > seconds(4.0) {
            most = most.max(st.wash_cut(Band::Low).get());
            least = least.min(st.wash_cut(Band::Low).get());
        }
    }
    assert!(most > 4.0, "the kick dips the return's LOW: {most} dB");
    assert!(
        least < 0.1 * most,
        "and lets it go between kicks: {least} dB"
    );
    // The reverb at rest: nothing run, nothing written, its filters empty.
    let mut st = PartEqs::REST.smart;
    for k in 0..seconds(1.0) {
        let out = washed(
            &mut st,
            k * BLOCK_SIZE,
            (&kick, Role::Front, true),
            (None, Role::Back),
        );
        assert_eq!(out.l.map(f32::to_bits), [0u32; BLOCK_SIZE]);
    }
    for b in Band::ALL {
        assert_eq!(st.wash_cut(b), CutDb::ZERO);
    }
}

/// The kick dip's swing (most less least, dB) on the bass once settled,
/// under kicks `gap_ms` apart.
fn swing(gap_ms: f32, amount: u8) -> f32 {
    let dip = kicks_at((gap_ms * FS as f32 / 1000.0) as usize, 6.0, amount);
    let tail = &dip[seconds(3.0)..];
    let (lo, hi) = tail
        .iter()
        .fold((f32::MAX, 0.0f32), |(l, h), &d| (l.min(d), h.max(d)));
    hi - lo
}

#[test]
fn no_gap_between_kicks_is_a_cliff() {
    // The hold fades from kicks 100 ms apart to none at 220 ms: from 60 to
    // 400 ms no step of 10 ms moves the swing by 1 dB (a hard edge at
    // 150 ms was 5.6 dB one millisecond on).
    // At DEEP (AMOUNT 95, a 10 dB dip), where it matters most.
    let swings: Vec<f32> = (0..=34)
        .map(|i| swing(60.0 + 10.0 * i as f32, 95))
        .collect();
    for (i, w) in swings.windows(2).enumerate() {
        let at = 60 + 10 * i;
        assert!(
            (w[1] - w[0]).abs() < 1.0,
            "{at} ms: {} to {} dB",
            w[0],
            w[1]
        );
    }
    assert!(
        swings[0] < 0.5 && swings[4] < 0.5,
        "held to 100 ms: {swings:?}"
    );
    assert!(
        swings[34] > 3.0,
        "at 400 ms each kick dips and lets go: {}",
        swings[34]
    );
    // 32nds at 120 BPM, 62.5 ms apart: each kick lands on the last one's
    // body, the key never quiet between them; still one dip.
    let fast = swing(62.5, 95);
    assert!(fast < 0.5, "{fast} dB peak to peak under 32nds at 120 BPM");
}

#[test]
fn a_part_moved_to_front_is_billed_as_cut_until_its_cuts_let_go() {
    use chimera_engine::part_eq::PartEq;
    use chimera_engine::part_eq::bill::{CUT, LISTEN, SmartBill, SmartLoad, first_voice};
    use chimera_engine::part_eq::smart::{Bands, StillCut};
    // The pad, MID under a FRONT bass, is cut; then it is made FRONT too.
    // Its bands run on at the full rate until the cut is back: billed so.
    let gens = clash();
    let mut st = PartEqs::REST.smart;
    for k in 0..seconds(4.0) {
        block(&mut st, k * BLOCK_SIZE, &gens, &roles(&[0], &[]), on(127));
    }
    let bill = |st: &SmartState| {
        let b = SmartBill {
            load: SmartLoad::Runs,
            still: StillCut::of(st),
        };
        first_voice(PartEq::FLAT, Role::Front, PartKind::Tonal, b, PAD)
    };
    let both_front = roles(&[0, 1], &[]);
    let mut k = seconds(4.0);
    block(&mut st, k * BLOCK_SIZE, &gens, &both_front, on(127));
    assert!(st.cut(PAD, Band::Low).get() > 5.0);
    assert_eq!(bill(&st), CUT, "still cut: billed as cut");
    let mut blocks = 0;
    while bill(&st) == CUT {
        k += 1;
        blocks += 1;
        block(&mut st, k * BLOCK_SIZE, &gens, &both_front, on(127));
        assert!(blocks < seconds(30.0), "its cuts let go within 30 s");
    }
    assert!(
        blocks > seconds(5.0),
        "billed as cut for the seconds it takes"
    );
    assert_eq!(st.cut(PAD, Band::Low), CutDb::ZERO);
    assert_eq!(bill(&st), LISTEN);
    // The bass, FRONT all along, was never held over.
    assert_eq!(StillCut::of(&st).part(BASS), Bands::Listened);
}

/// A kit (Part 1, FRONT) playing `kit` over the sine bass, at DEEP: each
/// block's kick dip on the bass, and the blocks the kit's onsets fell on.
fn kit_over_bass(kit: impl Fn(f32) -> f32 + 'static, secs: f32) -> (Vec<f32>, Vec<usize>) {
    let gens: [Gen; MAX_PARTS] = [
        Box::new(move |n| Some(kit(n as f32 / FS as f32))),
        Box::new(|n| Some(0.3 * (2.0 * PI * 55.0 * n as f32 / FS as f32).sin())),
        Box::new(|_| None),
        Box::new(|_| None),
        Box::new(|_| None),
        Box::new(|_| None),
    ];
    let mut st = PartEqs::REST.smart;
    let r = roles(&[0], &[]);
    let (mut dip, mut onsets) = (Vec::new(), Vec::new());
    let mut was = st.since_kick(BASS);
    for k in 0..seconds(secs) {
        block_kinds(&mut st, k * BLOCK_SIZE, &gens, &r, &DRUM_FIRST, on(95));
        dip.push(st.kick(PAD).get());
        let since = st.since_kick(BASS);
        if since < was {
            onsets.push(k);
        }
        was = since;
    }
    (dip, onsets)
}

/// A kick `t` seconds in: a 120 to 50 Hz sweep, its body 150 ms.
fn kick_at(t: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let ph = 2.0 * PI * (50.0 * t + 70.0 * 0.03 * (1.0 - (-t / 0.03).exp()));
    (-t / 0.15).exp() * ph.sin()
}

#[test]
fn only_a_kick_is_an_onset() {
    let ms = |m: f32| (m * FS as f32 / 1000.0) as usize / BLOCK_SIZE;
    // White, from the sample's own number.
    let noise = |t: f32| {
        let n = (t * FS as f32 + 0.5) as u32;
        ((n.wrapping_mul(2_654_435_761) >> 8) as f32 / 8_388_608.0) - 1.0
    };
    // A kick every 500 ms, and a snare after each on the same Part: no
    // onset, and the dip lets go as it does with no snare. The snare's
    // LOW is 14.9, 8.9 and 5.4 dB under the kick's at 0.4, 0.8 and 1.2.
    let snare = move |t: f32, level: f32, after: f32| {
        let u = t % 0.5 - after;
        if u < 0.0 {
            return 0.0;
        }
        let tone = (2.0 * PI * 180.0 * u).sin() + (2.0 * PI * 330.0 * u).sin();
        level * (-u / 0.08).exp() * (0.5 * tone + noise(t))
    };
    let (plain, plain_on) = kit_over_bass(|t| 0.8 * kick_at(t % 0.5), 6.0);
    // Settled, from the second kick: twelve kicks, eleven onsets. (From
    // silence the first kick's key has no peak to be near yet.)
    let settled = |on: &[usize]| on.iter().filter(|&&k| k > ms(250.0)).count();
    assert_eq!(settled(&plain_on), 11, "{plain_on:?}");
    for (level, after) in [
        (0.4, 0.125),
        (0.8, 0.125),
        (1.2, 0.125),
        (0.8, 0.18),
        (1.2, 0.18),
    ] {
        let (with, with_on) = kit_over_bass(
            move |t| 0.8 * kick_at(t % 0.5) + snare(t, level, after),
            6.0,
        );
        assert_eq!(
            settled(&with_on),
            11,
            "a snare at {level}, {after} s on, is no onset: {with_on:?}"
        );
        // No hold: the dip lets go as with no snare, but for what the
        // snare's own LOW presses while it sounds (the level rule: real
        // low energy within 6 dB of the kit's peak counts, ADR 0296 § 4).
        // The quiet snare leaves it within 1 dB; the loud ones press up
        // to 4 dB more, and the dip is under 3 dB by 310 ms (228 ms
        // plain; a hold kept it over 3 dB to 408 ms).
        let more = if level < 0.5 { 1.0 } else { 4.0 };
        for bar in 8..11 {
            let kick = ms(500.0 * bar as f32);
            for at in ms(1000.0 * after + 5.0)..ms(480.0) {
                let (a, b) = (plain[kick + at], with[kick + at]);
                assert!(
                    b - a < more && a - b < 1.0,
                    "snare {level} at {after} s, bar {bar}, block {at}: {a} dB plain, {b} with"
                );
            }
            for at in ms(310.0)..ms(480.0) {
                assert!(
                    with[kick + at] < 3.0,
                    "snare {level} at {after} s: block {at}"
                );
            }
        }
    }
    // One tom is one onset, one ghost kick is one.
    // (Each a second after a kick at 0.)
    for (hz, level) in [(70.0, 0.7), (90.0, 0.7), (60.0, 1.2), (80.0, 1.2)] {
        let tom = move |t: f32| {
            let u = (t - 1.0).max(0.0);
            0.8 * kick_at(t) + level * (-u / 0.2).exp() * (2.0 * PI * hz * u).sin()
        };
        let (_, on) = kit_over_bass(tom, 3.0);
        assert_eq!(settled(&on), 1, "a {hz} Hz tom at {level}: {on:?}");
    }
    for level in [0.6, 0.4] {
        let (_, on) = kit_over_bass(move |t| 0.8 * kick_at(t) + level * kick_at(t - 1.0), 3.0);
        assert_eq!(settled(&on), 1, "a ghost kick at {level}: {on:?}");
    }
    // A held sub with tremolo (5.4 dB deep here) is one onset, not one a
    // cycle. Deeper, its swells are onsets, steadying a dip already held.
    for (sub, rate, deep) in [(50.0, 5.0, 0.3), (40.0, 8.0, 0.3), (60.0, 6.5, 0.3)] {
        let trem = move |t: f32| {
            let u = (t - 1.0).max(0.0);
            let swell = 1.0 + deep * (2.0 * PI * rate * u).sin();
            0.8 * kick_at(t) + 0.6 * swell * (2.0 * PI * sub * u).sin()
        };
        let (_, on) = kit_over_bass(trem, 5.0);
        assert!(
            settled(&on) <= 1,
            "a {sub} Hz sub, tremolo {rate} Hz: {on:?}"
        );
    }
}

#[test]
fn a_return_moved_to_front_still_runs_cut_until_its_cuts_let_go() {
    use chimera_engine::part_eq::smart::{Bands, StillCut};
    // The return is never heard through its cut bands (`Hears`): it is
    // its cuts not yet at rest that keep it billed as cut.
    let lead = |n: usize| 0.5 * (2.0 * PI * 300.0 * n as f32 / FS as f32).sin();
    let wash = |n: usize| 0.4 * (2.0 * PI * 330.0 * n as f32 / FS as f32).sin();
    let mut st = PartEqs::REST.smart;
    let mut k = 0;
    let mut run = |st: &mut SmartState, role: Role, blocks: usize| {
        for _ in 0..blocks {
            washed(
                st,
                k * BLOCK_SIZE,
                (&lead, Role::Front, false),
                (Some(&wash), role),
            );
            k += 1;
        }
    };
    run(&mut st, Role::Back, seconds(4.0));
    assert!(st.wash_cut(Band::LowMid).get() > 2.0);
    assert_eq!(StillCut::of(&st).wash(), Bands::Full);
    run(&mut st, Role::Front, seconds(1.0));
    assert!(st.wash_cut(Band::LowMid).get() > 0.5, "still letting go");
    assert_eq!(StillCut::of(&st).wash(), Bands::Full);
    run(&mut st, Role::Front, seconds(25.0));
    assert_eq!(st.wash_cut(Band::LowMid), CutDb::ZERO);
    assert_eq!(StillCut::of(&st).wash(), Bands::Listened);
    assert_eq!(StillCut::of(&st).part(BASS), Bands::Listened);
}
