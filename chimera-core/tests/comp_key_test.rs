//! The master compressor's KEY (ADR 0303): FULL is the compressor as it
//! was; LOW, HIGH and HIT filter the detector and never the audio; PART n
//! keys from a Part's bus and passes that Part unducked.

mod common;
use chimera_core::comp_key::kern::sections_of;
use chimera_core::comp_key::{
    Biquad, CompKey, KEY_NAMES, KEYS, PASS_FADE_S, Passing, Ranged, Source, bill,
};
use chimera_core::dsp::comp::{CompParams, Ratio};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::hw::{Cost, DAC_PAIRS, MAX_PARTS};
use chimera_core::instrument::{AudioShared, DacBlocks, MixState, mix_parts};
use chimera_core::part::DacPair;
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use std::f64::consts::PI;

type Out = [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS];

/// PART `n + 1`.
fn part(n: usize) -> CompKey {
    PartId::ALL.map(CompKey::Part)[n]
}

/// How much of the passing Part is put back, 0 with nobody passing.
fn weight(bus: &FxBus) -> f32 {
    match bus.comp_key().passing() {
        Passing::Nobody => 0.0,
        Passing::Part { weight, .. } => weight.get(),
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

fn sine(hz: f32, amp: f32, i: usize) -> f32 {
    amp * (2.0 * core::f32::consts::PI * hz * i as f32 / SR as f32).sin()
}

fn noise(n: usize, amp: f32, seed: u32) -> Vec<f32> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
        })
        .collect()
}

/// The compressor's settings with `key`.
fn fx(comp: CompParams, key: CompKey) -> FxParams {
    FxParams {
        comp: CompParams { key, ..comp },
        ..FxParams::OFF
    }
}

/// `ch` (pair-major, L then R) through the bus's master section, the
/// settings `p(block)`: the same layout back.
fn master(
    bus: &mut FxBus,
    ch: &[Vec<f32>; 2 * DAC_PAIRS],
    p: impl Fn(usize) -> FxParams,
) -> [Vec<f32>; 2 * DAC_PAIRS] {
    let n = ch[0].len();
    let mut got: [Vec<f32>; 2 * DAC_PAIRS] = Default::default();
    for b in 0..n / BLOCK_SIZE {
        let mut out: Out = core::array::from_fn(|k| {
            core::array::from_fn(|i| ch[2 * k + i % 2][b * BLOCK_SIZE + i / 2])
        });
        bus.master(&mut out, &p(b), SR);
        for (c, g) in got.iter_mut().enumerate() {
            g.extend((0..BLOCK_SIZE).map(|i| out[c / 2][2 * i + c % 2]));
        }
    }
    got
}

/// `x` on pair 1 left, silence elsewhere.
fn pair1(x: Vec<f32>) -> [Vec<f32>; 2 * DAC_PAIRS] {
    let n = x.len();
    let mut ch: [Vec<f32>; 2 * DAC_PAIRS] = core::array::from_fn(|_| vec![0.0; n]);
    ch[0] = x;
    ch
}

/// The codes are frozen and the names read as the owner approved them.
#[test]
fn the_keys_are_full_the_ranges_then_the_parts() {
    assert_eq!(
        KEY_NAMES,
        [
            "FULL", "LOW", "HIGH", "HIT", "PART 1", "PART 2", "PART 3", "PART 4", "PART 5",
            "PART 6"
        ]
    );
    let idents: Vec<&str> = CompKey::ALL.iter().map(|k| k.ident()).collect();
    assert_eq!(
        idents,
        [
            "FULL", "LOW", "HIGH", "HIT", "PART1", "PART2", "PART3", "PART4", "PART5", "PART6"
        ]
    );
    for (c, k) in CompKey::ALL.into_iter().enumerate() {
        assert_eq!(usize::from(k.code()), c);
        assert_eq!(CompKey::from_code(c as u8), Some(k));
        assert_eq!(CompKey::from_f32(c as f32), k);
    }
    for c in KEYS as u8..=u8::MAX {
        assert_eq!(CompKey::from_code(c), None, "{c}");
    }
    assert_eq!(CompKey::from_f32(f32::NAN), CompKey::Full);
    assert_eq!(CompKey::from_f32(-3.0), CompKey::Full);
    assert_eq!(CompKey::from_f32(99.0), part(5));
    assert_eq!(CompParams::default().key, CompKey::Full);
    // In the padding: SMART EQ's test holds the size too.
    assert_eq!(size_of::<CompParams>(), 32);
    assert_eq!(size_of::<CompKey>(), 2);
}

/// A section's gain at `hz`, dB, in f64.
fn gain_db(s: &Biquad, hz: f64) -> f64 {
    let w = 2.0 * PI * hz / SR as f64;
    let (b, a) = (s.b.map(f64::from), s.a.map(f64::from));
    let re = |k: [f64; 3]| k[0] + k[1] * w.cos() + k[2] * (2.0 * w).cos();
    let im = |k: [f64; 3]| -k[1] * w.sin() - k[2] * (2.0 * w).sin();
    let (n, d) = (b, [1.0, a[0], a[1]]);
    10.0 * ((re(n).powi(2) + im(n).powi(2)) / (re(d).powi(2) + im(d).powi(2))).log10()
}

/// BS.1770-4's K-weighting at 48 kHz, as the standard prints it: the
/// shelf, then the high-pass.
const K_SHELF: Biquad = Biquad {
    b: [1.535_124_9, -2.691_696_2, 1.198_392_8],
    a: [-1.690_659_3, 0.732_480_77],
};
const K_RLB: Biquad = Biquad {
    b: [1.0, -2.0, 1.0],
    a: [-1.990_047_5, 0.990_072_25],
};

/// The sections are the stated filters: LOW and HIGH second-order
/// Butterworth at 150 Hz (−3 dB at the corner, 12 dB an octave past it),
/// HIT the standard's K-weighting at 48 kHz, coefficient for coefficient.
#[test]
fn the_sections_are_the_stated_filters() {
    let butter = |hz: f64| -10.0 * (1.0 + (hz / 150.0).powi(4)).log10();
    let low = sections_of(Ranged::Low, SR);
    let high = sections_of(Ranged::High, SR);
    for hz in [20.0, 60.0, 150.0, 300.0, 600.0, 1_200.0, 3_000.0] {
        let (l, h) = (gain_db(&low[0], hz), gain_db(&high[0], 150.0 * 150.0 / hz));
        // The bilinear warp shows by 3 kHz: a quarter dB at −52.
        let tol = if hz > 1_000.0 { 0.3 } else { 0.1 };
        assert!((l - butter(hz)).abs() < tol, "LOW at {hz}: {l}");
        assert!(
            (h - butter(hz)).abs() < 0.1,
            "HIGH at {}: {h}",
            22_500.0 / hz
        );
    }
    assert!((gain_db(&low[0], 150.0) + 3.01).abs() < 0.05);
    assert!((gain_db(&high[0], 150.0) + 3.01).abs() < 0.05);
    let hit = sections_of(Ranged::Hit, SR);
    for (got, want) in hit.iter().zip([K_SHELF, K_RLB]) {
        for (g, w) in got.b.iter().chain(&got.a).zip(want.b.iter().chain(&want.a)) {
            assert!((g - w).abs() < 2e-6, "{got:?} against {want:?}");
        }
    }
    // The curve the standard draws: flat at 1 kHz but for its 0.7 dB, the
    // shelf's 4 dB above, the high-pass below.
    let k = |hz: f64| gain_db(&hit[0], hz) + gain_db(&hit[1], hz);
    assert!((k(1_000.0) - 0.69).abs() < 0.05, "{}", k(1_000.0));
    assert!((k(10_000.0) - 4.0).abs() < 0.1, "{}", k(10_000.0));
    assert!((k(100.0) + 1.0).abs() < 0.3, "{}", k(100.0));
    assert!(k(20.0) < -10.0, "{}", k(20.0));
    // Only the ranges have sections: the other keys name none.
    assert_eq!(CompKey::Full.source(), Source::Mix);
    assert_eq!(part(0).source(), Source::Part(PartId::ALL[0]));
    assert_eq!(CompKey::Hit.source(), Source::Ranged(Ranged::Hit));
}

/// What the detector reads of a −6 dBFS sine at `hz` under `key`, dB
/// against FULL's reading of it: from the reduction, at 2:1 over −40 dB
/// with the fastest attack and the slowest release. `None` under the
/// threshold (more than 34 dB down).
fn heard(key: CompKey, hz: f32) -> Option<f32> {
    let p = CompParams {
        thresh: 0.0,
        ratio: Ratio::ALL[2],
        attack: 0.0,
        release: 1.0,
        ..CompParams::default()
    };
    let n = 2 * SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let x: Vec<f32> = (0..n).map(|i| sine(hz, 0.5, i)).collect();
    let gr = |key: CompKey| {
        let mut bus = Box::new(FxBus::new());
        master(&mut bus, &pair1(x.clone()), |_| fx(p, key));
        bus.master_gr_db()
    };
    let (full, got) = (gr(CompKey::Full), gr(key));
    assert!(full > 16.0, "FULL reduces {full} dB at {hz} Hz");
    (got > 0.01).then(|| (got - full) / 0.5)
}

/// The detector's measured response is the stated filter's: within half
/// a dB of it wherever the level stays over the threshold.
#[test]
fn the_detector_hears_through_the_stated_filters() {
    let butter = |hz: f32| -10.0 * (1.0 + (hz / 150.0).powi(4)).log10();
    for hz in [40.0f32, 60.0, 150.0, 300.0, 600.0] {
        let low = heard(CompKey::Low, hz).unwrap();
        assert!((low - butter(hz)).abs() < 0.5, "LOW at {hz}: {low}");
        let mirror = 22_500.0 / hz;
        let high = heard(CompKey::High, mirror).unwrap();
        assert!((high - butter(hz)).abs() < 0.5, "HIGH at {mirror}: {high}");
    }
    // The design's own check: LOW hears 60 Hz as FULL does, and 3 kHz at
    // least 20 dB less; HIGH the mirror.
    assert!(heard(CompKey::Low, 60.0).unwrap().abs() < 1.0);
    assert!(heard(CompKey::Low, 3_000.0).is_none_or(|d| d < -20.0));
    assert!(heard(CompKey::High, 3_000.0).unwrap().abs() < 1.0);
    assert!(heard(CompKey::High, 20.0).is_none_or(|d| d < -20.0));
    let k = |hz: f64| gain_db(&K_SHELF, hz) + gain_db(&K_RLB, hz);
    for hz in [100.0f32, 1_000.0, 4_000.0, 10_000.0] {
        let hit = heard(CompKey::Hit, hz).unwrap();
        let want = k(f64::from(hz)) as f32;
        assert!((hit - want).abs() < 0.5, "HIT at {hz}: {hit}, want {want}");
    }
}

/// KEY never filters the audio: under every key each channel leaves as
/// it came times one gain, the same on every channel that sample; and
/// with nothing over the threshold every sample leaves as it came.
#[test]
fn the_audio_is_only_ever_scaled() {
    let n = 64 * BLOCK_SIZE;
    // A different spectrum on every channel; the first never near zero.
    let ch: [Vec<f32>; 2 * DAC_PAIRS] = core::array::from_fn(|c| {
        (0..n)
            .map(|i| match c {
                0 => 0.3 + sine(55.0, 0.1, i),
                1 => sine(3_100.0, 0.25, i),
                2 => sine(180.0, 0.2, i) + sine(7_000.0, 0.1, i),
                _ => noise(1, 0.2, (c * 7919 + i) as u32 + 1)[0],
            })
            .collect()
    });
    let squash = CompParams {
        thresh: 0.2,
        ratio: Ratio::ALL[6],
        ..CompParams::default()
    };
    let idle = CompParams {
        thresh: 1.0,
        ratio: Ratio::MAX,
        ..CompParams::default()
    };
    for key in CompKey::ALL {
        let mut bus = Box::new(FxBus::new());
        let got = master(&mut bus, &ch, |_| fx(squash, key));
        let mut least = 1.0f32;
        for i in 0..n {
            let g = got[0][i] / ch[0][i];
            least = least.min(g);
            for c in 1..2 * DAC_PAIRS {
                let want = ch[c][i] * g;
                assert!(
                    (got[c][i] - want).abs() < 1e-6,
                    "{key:?} channel {c} sample {i}: {} against {want}",
                    got[c][i]
                );
            }
        }
        // No Parts given: a PART key hears silence and reduces nothing.
        match key {
            CompKey::Part(_) => assert_eq!(least, 1.0, "{key:?}"),
            CompKey::Full | CompKey::Low | CompKey::High | CompKey::Hit => {
                assert!(least < 0.7, "{key:?} reduced to {least}")
            }
        }
        let mut bus = Box::new(FxBus::new());
        let got = master(&mut bus, &ch, |_| fx(idle, key));
        assert!(got == ch, "{key:?} under the threshold");
    }
}

/// A kick under a pad: the pad alone holds FULL down, so the kick moves
/// its reduction little; LOW hears only the kick, so the mix dips on each
/// hit and comes back between them.
#[test]
fn low_pumps_where_full_holds() {
    let beat = SR as usize / 2;
    let n = 8 * beat / BLOCK_SIZE * BLOCK_SIZE;
    let x: Vec<f32> = (0..n)
        .map(|i| {
            let pad = sine(440.0, 0.2, i) + sine(660.0, 0.15, i) + sine(1_320.0, 0.15, i);
            let t = (i % beat) as f32 / SR as f32;
            let kick = 0.25 * (-t * 18.0).exp() * (2.0 * core::f32::consts::PI * 55.0 * t).sin();
            pad + kick
        })
        .collect();
    // The design's good setting: 6:1 over −24 dB, 3 ms in, 150 ms out.
    let p = CompParams {
        thresh: 0.4,
        ratio: Ratio::ALL[5],
        attack: 0.33,
        release: 0.6,
        ..CompParams::default()
    };
    // The reduction over the last two beats, a reading a block: its
    // deepest, and its shallowest.
    let swing = |key: CompKey| {
        let mut bus = Box::new(FxBus::new());
        let (mut most, mut least) = (0.0f32, f32::MAX);
        for b in 0..n / BLOCK_SIZE {
            let mut out: Out = [[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS];
            for i in 0..BLOCK_SIZE {
                out[0][2 * i] = x[b * BLOCK_SIZE + i];
                out[0][2 * i + 1] = x[b * BLOCK_SIZE + i];
            }
            bus.master(&mut out, &fx(p, key), SR);
            if b * BLOCK_SIZE >= 6 * beat {
                most = most.max(bus.master_gr_db());
                least = least.min(bus.master_gr_db());
            }
        }
        (most, least)
    };
    let (full, low) = (swing(CompKey::Full), swing(CompKey::Low));
    assert!(full.1 > 8.0, "FULL never lets go: {full:?}");
    assert!(full.0 - full.1 < 4.0, "FULL barely moves: {full:?}");
    assert!(low.1 < 1.0, "LOW comes back between kicks: {low:?}");
    assert!(low.0 - low.1 > 6.0, "LOW pumps: {low:?}");
    assert!(low.0 - low.1 > 2.0 * (full.0 - full.1));
}

/// An inf in the mix leaves no trace in a ranged detector: the reduction
/// comes back as FULL's does.
#[test]
fn an_inf_sample_recovers_under_a_range() {
    let n = 3 * SR as usize / BLOCK_SIZE * BLOCK_SIZE;
    let mut x: Vec<f32> = (0..n).map(|i| sine(80.0, 0.5, i)).collect();
    x[BLOCK_SIZE * 40 + 3] = f32::INFINITY;
    let p = CompParams {
        thresh: 0.0,
        ratio: Ratio::ALL[4],
        ..CompParams::default()
    };
    for key in [CompKey::Low, CompKey::High, CompKey::Hit] {
        let mut bus = Box::new(FxBus::new());
        let got = master(&mut bus, &pair1(x.clone()), |_| fx(p, key));
        let gr = bus.master_gr_db();
        assert!(gr.is_finite(), "{key:?}");
        assert!(got[0][n - BLOCK_SIZE..].iter().all(|s| s.is_finite()));
        let mut clean = Box::new(FxBus::new());
        let mut y = x.clone();
        y[BLOCK_SIZE * 40 + 3] = 0.0;
        master(&mut clean, &pair1(y), |_| fx(p, key));
        assert!(
            (gr - clean.master_gr_db()).abs() < 0.1,
            "{key:?}: {gr} against {}",
            clean.master_gr_db()
        );
    }
}

/// Every stored ratio keeps its meaning; MAX is appended, and takes
/// everything over THRESH off.
#[test]
fn max_ends_the_ratios() {
    let ratios = Ratio::ALL.map(Ratio::ratio);
    let (last, before) = ratios.split_last().unwrap();
    assert_eq!(before, [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 10.0, 20.0]);
    assert_eq!((*last, Ratio::MAX.ratio()), (f32::INFINITY, f32::INFINITY));
    assert_eq!((Ratio::UNITY.slope(), Ratio::MAX.slope()), (0.0, 1.0));
    assert_eq!(Ratio::MAX.ident(), "MAX");
    // A code past the table is no ratio; a knob's value past it is MAX.
    assert_eq!(Ratio::new(Ratio::MAX.code() + 1), None);
    assert_eq!(Ratio::from_f32(200.0), Ratio::MAX);
    assert_eq!(Ratio::from_f32(f32::NAN), Ratio::UNITY);
    let p = CompParams {
        thresh: 0.5,
        ratio: Ratio::MAX,
        ..CompParams::default()
    };
    assert_eq!(p.sanitised().ratio, Ratio::MAX);
    for level in [-30.0f32, -20.0, -12.0, 0.0] {
        let x = 10f32.powf(level / 20.0);
        let n = SR as usize / BLOCK_SIZE * BLOCK_SIZE;
        let mut bus = Box::new(FxBus::new());
        let out = master(&mut bus, &pair1(vec![x; n]), |_| fx(p, CompKey::Full));
        let got = db(out[0][n - 1] / x) + level;
        let want = level.min(-20.0);
        assert!((got - want).abs() < 0.05, "MAX at {level} dB: {got}");
    }
}

/// KEY's bill beside the bus: nothing at FULL, so every bill is as it
/// was.
#[test]
fn full_bills_nothing() {
    assert_eq!(bill::of(CompKey::Full), Cost::ZERO);
    assert_eq!(bill::of(CompKey::Low), bill::RANGE);
    assert_eq!(bill::of(CompKey::High), bill::RANGE);
    assert_eq!(bill::of(CompKey::Hit), bill::HIT);
    for k in PartId::ALL.map(CompKey::Part) {
        assert_eq!(bill::of(k), bill::PART);
    }
    assert!(bill::RANGE > Cost::ZERO && bill::HIT > bill::RANGE);
}

/// The Parts' buses through the mix, a block at a time.
struct Mix {
    shared: AudioShared,
    mix: MixState,
    fx: Box<FxBus>,
    dac: Box<DacBlocks>,
    sends: [[f32; BLOCK_SIZE]; FX_SENDS],
    block: usize,
}

impl Mix {
    /// Part 1 on `pair`, Part 2 on pair 1; the compressor `comp`.
    fn new(pair: DacPair, comp: CompParams) -> Mix {
        let mut shared = AudioShared::default();
        shared.parts[0].mix.output = pair;
        shared.parts[1].mix.output = DacPair::P1;
        (shared.parts[0].mix.level, shared.parts[0].mix.pan) = (0.6, -0.3);
        (shared.parts[1].mix.level, shared.parts[1].mix.pan) = (0.8, 0.2);
        shared.fx.comp = comp;
        Mix {
            shared,
            mix: MixState::default(),
            fx: Box::new(FxBus::new()),
            dac: Box::new(DacBlocks::new()),
            sends: [[0.0; BLOCK_SIZE]; FX_SENDS],
            block: 0,
        }
    }

    /// A block: Parts 1 and 2 play `key(i)` and `pad(i)` where written.
    /// The pairs as the mix summed them, after the compressor.
    fn step(
        &mut self,
        key: Option<&dyn Fn(usize) -> f32>,
        pad: Option<&dyn Fn(usize) -> f32>,
    ) -> Out {
        let i0 = self.block * BLOCK_SIZE;
        let mut buses = [[0.0f32; BLOCK_SIZE]; MAX_PARTS];
        let mut written = [false; MAX_PARTS];
        for (p, f) in [key, pad].into_iter().enumerate() {
            if let Some(f) = f {
                buses[p] = core::array::from_fn(|i| f(i0 + i));
                written[p] = true;
            }
        }
        mix_parts(
            &buses,
            &written,
            &mut self.sends,
            &mut self.mix,
            &mut self.fx,
            &self.shared,
            Tempo::DEFAULT,
            SR,
            &mut self.dac,
        );
        self.block += 1;
        *self.dac.input()
    }
}

/// RATIO MAX over −40 dB: everything the detector hears comes off.
const CRUSH: CompParams = CompParams {
    thresh: 0.0,
    ratio: Ratio::MAX,
    attack: 0.2,
    release: 0.5,
    ..CompParams::DEFAULT
};

fn kick(i: usize) -> f32 {
    sine(70.0, 0.5, i)
}

fn pad(i: usize) -> f32 {
    sine(330.0, 0.2, i) + sine(495.0, 0.15, i)
}

/// PART 1 as the key, on each output pair: Part 1 leaves at its own
/// level, sample for sample, while Part 2 ducks by what the static curve
/// asks of Part 1's bus (before its LEVEL: −6 dBFS over −40 dB at MAX is
/// 34 dB). MIX folds in: at 0.5 Part 2 keeps half of itself.
#[test]
fn the_key_part_plays_through_and_the_rest_ducks() {
    for pair in [DacPair::P1, DacPair::P2, DacPair::P3] {
        for mix in [1.0f32, 0.5] {
            let on = CompParams {
                key: part(0),
                mix,
                ..CRUSH
            };
            let mut all = Mix::new(pair, on);
            let mut key_alone = Mix::new(pair, CompParams::DEFAULT);
            let mut pad_alone = Mix::new(pair, CompParams::DEFAULT);
            let blocks = SR as usize / BLOCK_SIZE;
            let k = pair.index();
            for b in 0..blocks {
                let got = all.step(Some(&kick), Some(&pad));
                let key = key_alone.step(Some(&kick), None);
                let rest = pad_alone.step(None, Some(&pad));
                // Settled: past the engage and the pass's fade.
                if b < blocks / 2 {
                    continue;
                }
                let e = 1.0 + mix * (10f32.powf(-34.0 / 20.0) - 1.0);
                for i in 0..2 * BLOCK_SIZE {
                    // Part 2 alone is on pair 1 unless Part 1 shares it.
                    let ducked = got[0][i] - if k == 0 { key[0][i] } else { 0.0 };
                    // The reduction ripples a dB between the key's peaks.
                    let want = e * rest[0][i];
                    assert!(
                        (ducked - want).abs() < 0.15 * mix * want.abs() + 1e-6,
                        "{pair:?} MIX {mix} block {b} sample {i}: Part 2 {ducked}, want {}",
                        e * rest[0][i]
                    );
                    if k != 0 {
                        assert!(
                            (got[k][i] - key[k][i]).abs() < 1e-6,
                            "{pair:?} MIX {mix} block {b} sample {i}: Part 1 {} for {}",
                            got[k][i],
                            key[k][i]
                        );
                    }
                }
            }
            assert!((all.fx.master_gr_db() - 34.0).abs() < 1.5);
            assert_eq!(weight(&all.fx), 1.0);
        }
    }
}

/// On the pair it shares with the rest, the key Part's own level is
/// unchanged: taking the ducked rest away (its samples times the gain
/// the other channel shows) leaves Part 1 as it plays alone.
#[test]
fn the_key_part_is_whole_on_a_shared_pair() {
    let on = CompParams {
        key: part(0),
        ..CRUSH
    };
    let mut all = Mix::new(DacPair::P1, on);
    // Part 2 hard right: the left channel is Part 1 alone.
    all.shared.parts[1].mix.pan = 1.0;
    let mut key_alone = Mix::new(DacPair::P1, CompParams::DEFAULT);
    let blocks = SR as usize / BLOCK_SIZE;
    for b in 0..blocks {
        let got = all.step(Some(&kick), Some(&pad));
        let key = key_alone.step(Some(&kick), None);
        if b < blocks / 2 {
            continue;
        }
        for i in 0..BLOCK_SIZE {
            assert!((got[0][2 * i] - key[0][2 * i]).abs() < 1e-6, "block {b}");
        }
    }
}

/// The key's LEVEL never moves the pump: turned all the way down, Part 1
/// is silent and the rest still ducks 34 dB.
#[test]
fn the_key_is_heard_before_its_level() {
    let on = CompParams {
        key: part(0),
        ..CRUSH
    };
    let mut m = Mix::new(DacPair::P2, on);
    m.shared.parts[0].mix.level = 0.0;
    let mut out = [[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS];
    for _ in 0..SR as usize / BLOCK_SIZE {
        out = m.step(Some(&kick), Some(&pad));
    }
    assert!((m.fx.master_gr_db() - 34.0).abs() < 1.5);
    assert!(out[1].iter().all(|s| *s == 0.0));
}

/// A key with no voices reads as no level: the reduction lets go at REL
/// and the rest plays as with the compressor at rest.
#[test]
fn a_silent_key_releases() {
    let on = CompParams {
        key: part(0),
        ..CRUSH
    };
    let mut m = Mix::new(DacPair::P1, on);
    let mut rest = Mix::new(DacPair::P1, CompParams::DEFAULT);
    let half = SR as usize / BLOCK_SIZE / 2;
    for _ in 0..half {
        m.step(Some(&kick), Some(&pad));
        rest.step(None, Some(&pad));
    }
    assert!(m.fx.master_gr_db() > 32.0);
    // REL 0.5 is 100 ms: a second on, the reduction is gone.
    let (mut got, mut want) = (
        [[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS],
        [[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS],
    );
    for _ in 0..2 * half {
        got = m.step(None, Some(&pad));
        want = rest.step(None, Some(&pad));
    }
    assert!(m.fx.master_gr_db() < 0.01, "{}", m.fx.master_gr_db());
    for (g, w) in got[0].iter().zip(&want[0]) {
        assert!((g - w).abs() < 1e-4);
    }
    // And one that never plays never reduces.
    let mut never = Mix::new(
        DacPair::P1,
        CompParams {
            key: part(4),
            ..CRUSH
        },
    );
    for _ in 0..half {
        never.step(Some(&kick), Some(&pad));
    }
    assert_eq!(never.fx.master_gr_db(), 0.0);
}

/// The largest step between two samples.
fn step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
}

/// Switching KEY never clicks. Both Parts hold a steady level (DC, so
/// any jump shows whole): through every switch, some cut short mid-fade,
/// no sample steps by more than the gain's own attack moves it, where an
/// unfaded pass would step by a third of full scale.
#[test]
fn switching_key_never_clicks() {
    let seq = [
        CompKey::Full,
        part(0),
        CompKey::Low,
        part(1),
        part(0),
        CompKey::Hit,
        CompKey::High,
        CompKey::Full,
        part(0),
        CompKey::Full,
        part(1),
    ];
    let comp = CompParams {
        thresh: 0.0,
        ratio: Ratio::MAX,
        attack: 0.5,
        release: 0.5,
        ..CompParams::DEFAULT
    };
    for pair in [DacPair::P1, DacPair::P2] {
        // Dwells longer than the fade (7.5 blocks), and shorter.
        for dwell in [40usize, 5, 3] {
            let mut m = Mix::new(pair, comp);
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
            let mut key_pair = Vec::new();
            for b in 0..dwell * seq.len() * 2 + 60 {
                m.shared.fx.comp.key = seq[b / dwell % seq.len()];
                let out = m.step(Some(&|_| 0.5), Some(&|_| 0.4));
                // Past the engage's own fade in.
                if b >= 20 {
                    l.extend(out[0].iter().step_by(2));
                    r.extend(out[0].iter().skip(1).step_by(2));
                    key_pair.extend(out[pair.index()].iter().step_by(2));
                }
            }
            for (name, x) in [("left", &l), ("right", &r), ("the key's pair", &key_pair)] {
                assert!(
                    step(x) < 0.01,
                    "{pair:?} dwell {dwell}: {name} steps {}",
                    step(x)
                );
            }
        }
    }
}

/// The pass fades in over 10 ms, and out again before another Part's
/// begins.
#[test]
fn the_pass_fades_over_ten_ms() {
    let fade = (PASS_FADE_S * SR as f32) as usize;
    assert_eq!(fade, 480);
    let mut m = Mix::new(
        DacPair::P1,
        CompParams {
            key: part(0),
            ..CRUSH
        },
    );
    let mut blocks = 0;
    while weight(&m.fx) < 1.0 {
        m.step(Some(&kick), Some(&pad));
        blocks += 1;
        assert!(blocks < 100);
    }
    assert_eq!(blocks, fade.div_ceil(BLOCK_SIZE));
    m.shared.fx.comp.key = part(1);
    m.step(Some(&kick), Some(&pad));
    let w = weight(&m.fx);
    assert!(w < 1.0 && w > 0.8, "Part 1 fades out first: {w}");
    for _ in 0..2 * fade.div_ceil(BLOCK_SIZE) {
        m.step(Some(&kick), Some(&pad));
    }
    assert_eq!(weight(&m.fx), 1.0);
    // Back at FULL the pass fades out, and KEY then rests.
    m.shared.fx.comp.key = CompKey::Full;
    for _ in 0..fade.div_ceil(BLOCK_SIZE) + 1 {
        m.step(Some(&kick), Some(&pad));
    }
    assert_eq!(weight(&m.fx), 0.0);
    assert_eq!(*m.fx.comp_key(), chimera_core::comp_key::KeyState::REST);
}

/// The key Part's gains easing (its LEVEL and PAN moved mid-note) are
/// followed: on its own pair it still leaves as it plays alone.
#[test]
fn the_pass_follows_the_key_parts_moving_gains() {
    let on = CompParams {
        key: part(0),
        ..CRUSH
    };
    let mut all = Mix::new(DacPair::P2, on);
    let mut alone = Mix::new(DacPair::P2, CompParams::DEFAULT);
    let blocks = SR as usize / BLOCK_SIZE;
    for b in 0..blocks {
        if b == blocks / 2 {
            for m in [&mut all, &mut alone] {
                (m.shared.parts[0].mix.level, m.shared.parts[0].mix.pan) = (1.0, 0.7);
            }
        }
        let got = all.step(Some(&kick), Some(&pad));
        let key = alone.step(Some(&kick), None);
        if b < blocks / 4 {
            continue;
        }
        for i in 0..2 * BLOCK_SIZE {
            assert!(
                (got[1][i] - key[1][i]).abs() < 2e-6,
                "block {b} sample {i}: {} for {}",
                got[1][i],
                key[1][i]
            );
        }
    }
}

/// KEY's bill is reserved beside the bus: a pool that just holds four
/// voices under FULL admits three under HIT.
#[test]
fn a_full_pool_admits_fewer_voices_under_a_key() {
    use chimera_core::budget::CONTROL_RESERVE;
    use chimera_core::hw::SampleBudget;
    use common::InstRig;
    let costs = |rig: &InstRig| -> Vec<Cost> {
        let slots = rig.inst.allocator().slots();
        let mut c: Vec<Cost> = slots
            .iter()
            .filter(|s| !s.is_free())
            .map(|s| s.cost())
            .collect();
        c.sort();
        c
    };
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.note_on(0, 48);
    rig.note_on(0, 52);
    rig.render(2);
    let (next, first) = (costs(&rig)[0], costs(&rig)[1]);
    let count = |key: CompKey| {
        let reserved = FxBus::COST + CONTROL_RESERVE;
        let room = Cost(reserved.0 + first.0 + 3 * next.0 + bill::HIT.0 / 2);
        // The audio's 75 % share of `cpu` Hz is `cpu × 0.75 / 48 000` a sample.
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(room.0 * 64_000 + 63_999));
        rig.shared.fx.comp.key = key;
        for n in 0..6 {
            rig.note_on(0, 48 + n);
        }
        rig.render(12);
        costs(&rig).len()
    };
    assert_eq!(count(CompKey::Full), 4);
    assert_eq!(count(CompKey::Hit), 3);
}

/// A real Part as the key, its notes panned (a side beside its bus):
/// alone in the mix it leaves exactly as with the compressor at rest,
/// while the reduction it asks for is deep.
#[test]
fn a_panned_key_part_plays_through_whole() {
    use chimera_core::hw::SampleBudget;
    use chimera_core::params::{EngineType, ParamSnapshot};
    use chimera_core::stereo::VoicePan;
    use common::InstRig;
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let play = |comp: CompParams| {
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
        *rig.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
        rig.shared.parts[0].params.out.pan = VoicePan::new(0.6);
        rig.shared.parts[0].mix.output = DacPair::P2;
        rig.shared.fx.comp = comp;
        rig.render(1);
        rig.note_on(0, 48);
        let mut out = Vec::new();
        for _ in 0..200 {
            rig.render(1);
            out.extend_from_slice(&rig.out()[1]);
        }
        (out, rig.fx.master_gr_db())
    };
    let (dry, _) = play(CompParams::DEFAULT);
    let (keyed, gr) = play(CompParams {
        key: part(0),
        ..CRUSH
    });
    assert!(gr > 12.0, "the key asks for {gr} dB");
    let peak = dry.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.05, "the Part sounds: {peak}");
    // Past the engage and the pass's fade (30 blocks).
    let from = 2 * BLOCK_SIZE * 30;
    let l: f32 = dry[from..].iter().step_by(2).map(|s| s * s).sum();
    let r: f32 = dry[from..].iter().skip(1).step_by(2).map(|s| s * s).sum();
    assert!(r > 2.0 * l, "panned right: {l} {r}");
    for (i, (k, d)) in keyed[from..].iter().zip(&dry[from..]).enumerate() {
        assert!((k - d).abs() < 2e-6, "sample {i}: {k} for {d}");
    }
    // The same Part under FULL is squashed.
    let (full, _) = play(CRUSH);
    let fpeak = full[from..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(fpeak < 0.5 * peak);
}

/// After any sequence of keys, each held for any number of blocks, the
/// pass's weight is in 0..=1, nobody passing carries none, and a Part
/// that is not the key never gains weight.
#[test]
fn the_pass_weight_stays_in_range_under_any_key_sequence() {
    let mut rng = common::XorShift(0x5eed_c0de_1037);
    for _ in 0..40 {
        let mut m = Mix::new(DacPair::P1, CRUSH);
        let mut last: Option<(PartId, f32)> = None;
        for _ in 0..60 {
            let key = CompKey::ALL[rng.below(KEYS)];
            m.shared.fx.comp.key = key;
            for _ in 0..1 + rng.below(12) {
                m.step(Some(&kick), Some(&pad));
                match m.fx.comp_key().passing() {
                    Passing::Nobody => {
                        assert_eq!(weight(&m.fx), 0.0);
                        assert!(m.fx.comp_key().at_rest());
                        last = None;
                    }
                    Passing::Part { id, weight } => {
                        let w = weight.get();
                        assert!((0.0..=1.0).contains(&w), "{key:?}: {w}");
                        if let Some((was, w0)) = last
                            && was == id
                            && key.part() != Some(id)
                        {
                            assert!(w <= w0, "{id:?} is not the key, yet {w0} -> {w}");
                        }
                        last = Some((id, w));
                    }
                }
            }
        }
    }
}
