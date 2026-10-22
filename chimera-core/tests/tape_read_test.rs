//! TAPE's read (spec § 1, § 2 Reading a source): the descriptor, the
//! 12-bit decode, and the 4-point Hermite (Niemitalo 2001's x-form).

mod common;

use chimera_core::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, Pos, Shape, SourceView, ZONE_LAYOUT_LEN, hermite,
};
use chimera_core::storage::twelve::{pack12, unpack12};
use chimera_core::stream::units::Frames;
use common::XorShift;
use common::tape::{held_plain, sine};

fn desc(shape: Shape, count: u8, rate: u16) -> Descriptor {
    Descriptor::whole(shape, count, rate).expect("a descriptor")
}

/// A descriptor, `layout` bytes of zone table, then `s` packed.
fn held(d: Descriptor, layout: usize, s: &[i16]) -> Vec<u8> {
    let mut h = d.to_bytes().to_vec();
    h.resize(DESC_LEN + layout, 0xAA);
    let at = h.len();
    h.resize(at + 3 * s.len().div_ceil(2), 0);
    pack12(s, &mut h[at..]);
    h
}

fn random12(rng: &mut XorShift) -> i16 {
    ((rng.next_u32() & 0xFFF) as i16) << 4 >> 4
}

#[test]
fn decode_matches_unpack12() {
    let mut rng = XorShift(0x7A9E_0201);
    let s: Vec<i16> = (0..100_000).map(|_| random12(&mut rng)).collect();
    let h = held(desc(Shape::Plain, 1, 48_000), 0, &s);
    let v = SourceView::new(&h).unwrap();
    assert_eq!(v.frames(), 100_000);
    for (i, &si) in s.iter().enumerate() {
        let (even, odd) = unpack12(&h[DESC_LEN..], i >> 1);
        let want = if i & 1 == 0 { even } else { odd };
        assert_eq!(v.sample(i as u32), want as i32, "sample {i}");
        assert_eq!(want, si);
    }
}

/// A descriptor's bytes, unchecked: what a file may hold.
fn raw(shape: Shape, count: u8, rate: u16, head_frames: u32, heads: u8) -> [u8; DESC_LEN] {
    let mut b = [0; DESC_LEN];
    (b[0], b[1]) = (shape as u8, count);
    b[2..4].copy_from_slice(&rate.to_le_bytes());
    b[4..8].copy_from_slice(&head_frames.to_le_bytes());
    b[8] = heads;
    b
}

#[test]
fn descriptor_round_trips() {
    let ok = [
        (Shape::Plain, 1),
        (Shape::Transwave, 1),
        (Shape::Transwave, 32),
        (Shape::ZoneSet, 1),
        (Shape::ZoneSet, 24),
    ];
    for (shape, count) in ok {
        for rate in (8_000..=48_000).step_by(4_000) {
            let head_frames = if shape == Shape::ZoneSet {
                0x0102_0304
            } else {
                0
            };
            let d = Descriptor::new(shape, count, rate, Frames::new(head_frames), 0).unwrap();
            let b = d.to_bytes();
            assert_eq!(b[0], shape as u8);
            assert_eq!(b[2..4], rate.to_le_bytes());
            assert_eq!(b[4..8], head_frames.to_le_bytes());
            assert_eq!(b[8..], [0; 8]);
            assert_eq!(Descriptor::from_bytes(&b), Some(d));
        }
    }
    let good = desc(Shape::Plain, 1, 48_000).to_bytes();
    assert_eq!(Descriptor::from_bytes(&good[..15]), None, "short");
    for at in 9..16 {
        let mut b = good;
        b[at] = 1;
        assert_eq!(Descriptor::from_bytes(&b), None, "reserved byte {at}");
    }
    let bad = [
        (Shape::Plain, 0, 48_000, 0),
        (Shape::Plain, 2, 48_000, 0),
        (Shape::Transwave, 0, 48_000, 0),
        (Shape::Transwave, 33, 48_000, 0),
        (Shape::ZoneSet, 25, 48_000, 0),
        (Shape::Plain, 1, 0, 0),
        (Shape::Plain, 1, 48_001, 0),
        (Shape::Plain, 1, 3_999, 0),
        (Shape::ZoneSet, 1, u16::MAX, 0),
        (Shape::Plain, 1, 48_000, 1),
        (Shape::Transwave, 4, 48_000, 4_096),
    ];
    for (shape, count, rate, hf) in bad {
        let d = Descriptor::new(shape, count, rate, Frames::new(hf), 0);
        assert_eq!(d, None, "{shape:?} {count} {rate} {hf}");
        let b = raw(shape, count, rate, hf, 0);
        assert_eq!(Descriptor::from_bytes(&b), None, "{b:?}");
    }
    let mut b = good;
    b[0] = 3;
    assert_eq!(Descriptor::from_bytes(&b), None, "unknown shape");
    assert_eq!(Shape::from_code(2), Some(Shape::ZoneSet));
    assert_eq!(Shape::from_code(3), None);
}

#[test]
fn odd_source_reads_one_silent_tail() {
    let mut x = vec![0.5; 64];
    x.push(0.25);
    let h = held_plain(&x, 48_000);
    assert_eq!(h.len(), DESC_LEN + 99);
    let v = SourceView::new(&h).unwrap();
    assert_eq!((v.frames(), v.sample(64), v.sample(65)), (66, 512, 0));
    let v64 = held_plain(&[0.5; 64], 48_000);
    assert_eq!(SourceView::new(&v64).unwrap().frames(), 64);
}

#[test]
fn hermite_exact_on_constant_and_ramp() {
    for k in 0..64 {
        let t = k as f32 / 64.0;
        for c in [-2048, -1, 0, 7, 2047] {
            assert_eq!(hermite([c; 4], t), c as f32 / 2048.0, "c {c} t {t}");
        }
        for (a, d) in [(-2048, 1), (0, 100), (-1000, 900), (2047, -1365)] {
            let y = [a, a + d, a + 2 * d, a + 3 * d];
            let want = (a as f32 + d as f32 + t * d as f32) / 2048.0;
            let got = hermite(y, t);
            assert!((got - want).abs() <= 1e-6, "{y:?} t {t}: {got} vs {want}");
        }
        // A quadratic exercises c2 and c3: t² through x = −1, 0, 1, 2.
        let sq = 2048.0 * hermite([1, 0, 1, 4], t);
        assert!((sq - t * t).abs() <= 1e-6, "t² at {t}: {sq}");
        let sq = hermite([500, 0, 500, 2000], t);
        assert!((sq - 500.0 * t * t / 2048.0).abs() <= 1e-6, "500 t² at {t}");
    }
    // Random taps and t against Catmull-Rom's matrix form, in f64.
    let mut rng = XorShift(0x7A9E_0204);
    for _ in 0..100_000 {
        let y = [(); 4].map(|_| random12(&mut rng) as i32);
        let t = rng.next_u32() as f64 / 4_294_967_296.0;
        let m = [
            [0.0, 2.0, 0.0, 0.0],
            [-1.0, 0.0, 1.0, 0.0],
            [2.0, -5.0, 4.0, -1.0],
            [-1.0, 3.0, -3.0, 1.0],
        ];
        let want: f64 = (0..4)
            .map(|p| t.powi(p as i32) * (0..4).map(|k| m[p][k] * y[k] as f64).sum::<f64>())
            .sum::<f64>()
            * 0.5
            / 2048.0;
        let got = hermite(y, t as f32) as f64;
        assert!((got - want).abs() <= 1e-6, "{y:?} t {t}: {got} vs {want}");
    }
}

/// The magnitude of `x` at `f` cycles a sample under a 4-term
/// Blackman-Harris window (sidelobes −92 dB), so `f` needn't fit whole
/// cycles.
fn bin(x: &[f32], f: f64) -> f64 {
    use core::f64::consts::TAU;
    let n = x.len() as f64;
    let (mut re, mut im, mut sw) = (0.0, 0.0, 0.0);
    for (i, &s) in x.iter().enumerate() {
        let u = TAU * i as f64 / n;
        let h = 0.35875 - 0.48829 * u.cos() + 0.14128 * (2.0 * u).cos() - 0.01168 * (3.0 * u).cos();
        let w = TAU * f * i as f64;
        (re, im, sw) = (
            re + s as f64 * h * w.cos(),
            im + s as f64 * h * w.sin(),
            sw + h,
        );
    }
    2.0 * re.hypot(im) / sw
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

/// `f` cycles a sample, folded into [0, 0.5].
fn fold(f: f64) -> f64 {
    let g = f.rem_euclid(1.0);
    g.min(1.0 - g)
}

/// A 0.9 sine at `f` of the source's rate read at `inc`: its passband and
/// its images at `fs ∓ f`, in dB (the images relative to the signal).
fn response(f: f64, inc: f32) -> (f64, f64, f64) {
    let out_n = 16_384;
    let frames = (out_n as f32 * inc) as u32 + 8;
    let src = sine(48_000, (1.0 / f) as f32, frames);
    let h = held_plain(&src.iter().map(|x| 0.9 * x).collect::<Vec<_>>(), 48_000);
    let v = SourceView::new(&h).unwrap();
    let decoded: Vec<f32> = (0..frames).map(|i| v.sample(i) as f32 / 2048.0).collect();
    let (step, mut p) = (Pos::from_f32(inc), Pos(0));
    let out: Vec<f32> = (0..out_n)
        .map(|_| {
            let y = v.read(p);
            p = p + step;
            y
        })
        .collect();
    let inc = inc as f64;
    let a_out = bin(&out, f * inc);
    let image = |g: f64| db(bin(&out, fold(g * inc)) / a_out);
    (db(a_out / bin(&decoded, f)), image(1.0 - f), image(1.0 + f))
}

#[test]
fn hermite_response() {
    // Spec § 2's table: (signal in fs, passband dB, worst image dB rel).
    // An irrational inc samples every t, so it meets the kernel's whole
    // response; inc 0.5 samples only t = 0.5 ([−1, 9, 9, −1] / 16), which
    // is cleaner, so its bounds are tighter.
    for (f, pass, image, half_image) in [
        (0.05, 0.00, -69.0, -78.0),
        (0.1, -0.02, -50.0, -54.0),
        (0.2, -0.24, -29.0, -31.0),
    ] {
        let (_, below, above) = response(f, 0.618_033_9);
        let worst = below.max(above);
        assert!(worst <= image, "{f} fs at inc 0.618: image {worst:.1} dB");
        let (got_pass, below, above) = response(f, 0.5);
        assert!(
            (got_pass - pass).abs() <= 0.05,
            "{f} fs: passband {got_pass:.3} dB"
        );
        let worst = below.max(above);
        assert!(
            worst <= half_image,
            "{f} fs at inc 0.5: image {worst:.1} dB"
        );
    }
}

#[test]
fn pos_and_edges() {
    let p = Pos::from_f32(-0.5);
    assert_eq!((p.whole(), p.frac()), (u32::MAX, 0.5));
    assert_eq!(Pos::from_f32(-1e-10), Pos(0));
    assert_eq!((Pos(0) - Pos(1 << 32)).whole(), u32::MAX);
    assert_eq!(
        Pos::from_f32(2.25) + Pos::from_f32(0.75),
        Pos::from_f32(3.0)
    );
    let h = held_plain(&sine(48_000, 7.0, 70)[1..], 48_000);
    let v = SourceView::new(&h).unwrap();
    let last = v.frames() - 1;
    assert_eq!(v.read(Pos::from_f32(-5.0)), v.sample(0) as f32 / 2048.0);
    assert_eq!(
        v.read(Pos::from_f32((v.frames() + 3) as f32)),
        v.sample(last) as f32 / 2048.0
    );
    assert_eq!(v.sample(last + 100), v.sample(last));
}

#[test]
fn reads_stay_in_row_reader() {
    let mut rng = XorShift(0x7A9E_0206);
    let shapes = [
        (Shape::Plain, 1),
        (Shape::Transwave, 32),
        (Shape::ZoneSet, 24),
    ];
    for run in 0..200 {
        let (shape, max) = shapes[run % 3];
        let count = if max == 1 {
            1
        } else {
            1 + rng.below(max) as u8
        };
        let frames = 1 + rng.below(100_000);
        let s: Vec<i16> = (0..frames).map(|_| random12(&mut rng)).collect();
        let layout = if shape == Shape::ZoneSet {
            ZONE_LAYOUT_LEN * count as usize
        } else {
            0
        };
        let mut h = held(desc(shape, count, 24_000), layout, &s);
        let cut = rng.below(h.len() - DESC_LEN - layout + 1);
        h.truncate(h.len() - cut);
        let data = h.len() - DESC_LEN - layout;
        let Some(v) = SourceView::strict(&h) else {
            assert!(2 * (data / 3) < 64, "{data} B of samples refused");
            continue;
        };
        assert_eq!(v.frames() as usize, 2 * (data / 3));
        assert_eq!(v.desc(), desc(shape, count, 24_000));
        for _ in 0..50 {
            let p = Pos((rng.next_u32() as u64) << 32 | rng.next_u32() as u64);
            let near = Pos::from_f32(rng.below(v.frames() as usize + 4) as f32 - 2.0)
                + Pos(rng.next_u32() as u64);
            let max_log2 = 31 - v.frames().leading_zeros();
            let log2_n = rng.below(max_log2.min(11) as usize + 1) as u32;
            let base = rng.below((v.frames() - (1 << log2_n)) as usize + 1) as u32;
            for y in [v.read(p), v.read(near), v.read_cycle(base, log2_n, p)] {
                assert!(y.is_finite() && y.abs() <= 1.5, "{y}");
            }
        }
    }
    let h = held(desc(Shape::Transwave, 1, 24_000), 0, &[1; 64]);
    let v = SourceView::strict(&h).unwrap();
    let over = std::panic::catch_unwind(|| v.read_cycle(62, 2, Pos(0)));
    assert!(over.is_err(), "strict catches a frame past the row");
}

#[test]
fn zone_set_samples_start_after_layout() {
    let mut s = vec![100, -200, 300, -400, 500];
    s.resize(65, 0);
    let h = held(desc(Shape::ZoneSet, 3, 32_000), 27, &s);
    let v = SourceView::new(&h).unwrap();
    assert_eq!(v.frames(), 2 * ((h.len() - 16 - 27) / 3) as u32);
    assert_eq!(v.frames(), 66);
    let got: Vec<i32> = (0..5).map(|i| v.sample(i)).collect();
    assert_eq!(got, [100, -200, 300, -400, 500]);
    let short = &h[..16 + 26];
    assert!(SourceView::new(short).is_none(), "layout cut short");
}
