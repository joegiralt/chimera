//! TAPE's read (spec § 1, § 2 Reading a source): the descriptor, the
//! 12-bit decode, and the 4-point Hermite (Niemitalo 2001's x-form).

mod common;

use chimera_core::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, Pos, Shape, SourceView, ZONE_LAYOUT_LEN, hermite,
};
use chimera_core::storage::twelve::{pack12, unpack12};
use common::XorShift;
use common::tape::{held_plain, sine};

fn desc(shape: Shape, count: u8, rate: u16) -> Descriptor {
    Descriptor {
        shape,
        count,
        rate,
        head_frames: 0,
    }
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
            let d = Descriptor {
                shape,
                count,
                rate,
                head_frames: 0x0102_0304,
            };
            let b = d.to_bytes();
            assert_eq!(b[0], shape as u8);
            assert_eq!(b[2..4], rate.to_le_bytes());
            assert_eq!(b[4..8], 0x0102_0304u32.to_le_bytes());
            assert_eq!(b[8..], [0; 8]);
            assert_eq!(Descriptor::from_bytes(&b), Some(d));
        }
    }
    let good = desc(Shape::Plain, 1, 48_000).to_bytes();
    assert_eq!(Descriptor::from_bytes(&good[..15]), None, "short");
    for at in 8..16 {
        let mut b = good;
        b[at] = 1;
        assert_eq!(Descriptor::from_bytes(&b), None, "reserved byte {at}");
    }
    let bad = [
        desc(Shape::Plain, 0, 48_000),
        desc(Shape::Plain, 2, 48_000),
        desc(Shape::Transwave, 0, 48_000),
        desc(Shape::Transwave, 33, 48_000),
        desc(Shape::ZoneSet, 25, 48_000),
        desc(Shape::Plain, 1, 0),
    ];
    for d in bad {
        assert_eq!(Descriptor::from_bytes(&d.to_bytes()), None, "{d:?}");
    }
    let mut b = good;
    b[0] = 3;
    assert_eq!(Descriptor::from_bytes(&b), None, "unknown shape");
    assert_eq!(Shape::from_code(2), Some(Shape::ZoneSet));
    assert_eq!(Shape::from_code(3), None);
}

#[test]
fn odd_source_reads_one_silent_tail() {
    let h = held_plain(&[0.5, -0.5, 0.25], 48_000);
    assert_eq!(h.len(), DESC_LEN + 6);
    let v = SourceView::new(&h).unwrap();
    assert_eq!((v.frames(), v.sample(2), v.sample(3)), (4, 512, 0));
    let v4 = held_plain(&[0.5, -0.5, 0.25, 0.1], 48_000);
    assert_eq!(SourceView::new(&v4).unwrap().frames(), 4);
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
    }
}

/// The magnitude of `x` at `f` cycles a sample, over whole cycles.
fn bin(x: &[f32], f: f64) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &s) in x.iter().enumerate() {
        let w = core::f64::consts::TAU * f * n as f64;
        (re, im) = (re + s as f64 * w.cos(), im + s as f64 * w.sin());
    }
    2.0 * re.hypot(im) / x.len() as f64
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

#[test]
fn hermite_response() {
    // (signal in fs, passband dB, worst image dB rel): spec § 2's table.
    for (f, pass, image) in [
        (0.05, 0.00, -69.0),
        (0.1, -0.02, -50.0),
        (0.2, -0.24, -29.0),
    ] {
        let src = sine(48_000, (1.0 / f) as f32, 4_100);
        let h = held_plain(&src.iter().map(|x| 0.9 * x).collect::<Vec<_>>(), 48_000);
        let v = SourceView::new(&h).unwrap();
        let (frames, out_n) = (4_000, 8_000);
        let decoded: Vec<f32> = (0..frames).map(|i| v.sample(i) as f32 / 2048.0).collect();
        let (step, mut p) = (Pos::from_f32(0.5), Pos::from_f32(0.0));
        let out: Vec<f32> = (0..out_n)
            .map(|_| {
                let y = v.read(p);
                p = p + step;
                y
            })
            .collect();
        let a_src = bin(&decoded, f);
        let a_out = bin(&out, f / 2.0);
        let a_img = bin(&out, (1.0 - f) / 2.0);
        let (got_pass, got_img) = (db(a_out / a_src), db(a_img / a_out));
        assert!(
            (got_pass - pass).abs() <= 0.5,
            "{f} fs: passband {got_pass:.3} dB"
        );
        assert!(got_img <= image, "{f} fs: image {got_img:.1} dB");
    }
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
            assert!(data < 3, "{data} B of samples refused");
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
    let h = held(desc(Shape::Transwave, 1, 24_000), 0, &[1, 2, 3, 4]);
    let v = SourceView::strict(&h).unwrap();
    let over = std::panic::catch_unwind(|| v.read_cycle(2, 2, Pos(0)));
    assert!(over.is_err(), "strict catches a frame past the row");
}

#[test]
fn zone_set_samples_start_after_layout() {
    let s = [100, -200, 300, -400, 500];
    let h = held(desc(Shape::ZoneSet, 3, 32_000), 27, &s);
    let v = SourceView::new(&h).unwrap();
    assert_eq!(v.frames(), 2 * ((h.len() - 16 - 27) / 3) as u32);
    assert_eq!(v.frames(), 6);
    let got: Vec<i32> = (0..5).map(|i| v.sample(i)).collect();
    assert_eq!(got, [100, -200, 300, -400, 500]);
    let short = &h[..16 + 26];
    assert!(SourceView::new(short).is_none(), "layout cut short");
}
