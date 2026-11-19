//! SMART EQ's block (ADR 0296): a lower role is cut where a higher role
//! plays, slowly, never stepped; FRONT and a quiet setting are untouched.

use chimera_engine::hw::MAX_PARTS;
use chimera_engine::instrument::Sides;
use chimera_engine::part_eq::PartEqs;
use chimera_engine::part_eq::dsp::smart::{SmartBlock, run};
use chimera_engine::part_eq::smart::{Amount, Band, CutDb, Role, SmartEq, SmartState, Switch};
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
