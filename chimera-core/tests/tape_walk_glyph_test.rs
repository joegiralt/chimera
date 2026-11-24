// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! WALK's footsteps glyph (ADR 0203): DIRECTION lays them, DRIFT scatters
//! them, FADE fades the last, TRON marks its tape's end; quantised inputs,
//! no frame term. `GLYPH_OUT=dir cargo test -p chimera-core --features
//! walk-styles --test tape_walk_glyph_test -- --ignored` writes the
//! variants as BMPs at 1× and 4× (each style, each pose;
//! `GLYPH_STYLES=straight,prints` picks styles).

use chimera_core::dsp::tape_engine::Direction;
use chimera_core::ui::tape_glyph::{
    Canvas, GlyphInputs, Kind, Machine, Pose, StepStyle, TapeGauge,
};

/// The glyph's set values for WALK in `d`, TRON `tron`, DRIFT `drift`.
/// STRETCH's LENGTH and TYPE (ADR 0204) at their rest.
fn set(d: Direction, tron: bool, drift: f32, fade: f32) -> [f32; 15] {
    [
        0.0,
        0.0,
        1.0,
        0.15,
        0.0,
        0.75,
        fade,
        0.0,
        0.0,
        0.2,
        d as u8 as f32 / 3.0,
        f32::from(u8::from(tron)),
        drift,
        0.5,
        0.0,
    ]
}

fn gauge(d: Direction, tron: bool, drift: f32, fade: f32) -> TapeGauge {
    TapeGauge::of(&GlyphInputs::from_set(
        set(d, tron, drift, fade),
        Pose::Live,
    ))
}

fn canvas(g: &TapeGauge, style: StepStyle) -> Canvas {
    let mut c = Canvas::new();
    chimera_engine::tape_glyph::draw::render_style(g, &mut c, style);
    c
}

#[test]
fn walk_draws_footsteps_for_every_path() {
    for d in Direction::ALL {
        for tron in [false, true] {
            let g = gauge(d, tron, 0.0, 0.0);
            assert!(matches!(g.machine, Machine::Walk(_)), "{d:?}");
            let lit = canvas(&g, StepStyle::DEFAULT)
                .0
                .iter()
                .filter(|&&i| i != chimera_core::ui::tape_glyph::Ink::Bg)
                .count();
            assert!(lit > 40, "{d:?} {tron}: {lit}");
        }
    }
    assert_eq!(gauge(Direction::Forward, true, 0.0, 0.0).kind(), Kind::Tron);
    assert_eq!(gauge(Direction::Ping, false, 0.0, 0.0).kind(), Kind::Ping);
}

#[test]
fn each_input_moves_the_picture_and_its_key() {
    let base = gauge(Direction::Forward, false, 0.0, 0.0);
    let pic = |g: &TapeGauge| canvas(g, StepStyle::DEFAULT).0;
    for (what, g) in [
        ("REVERSE", gauge(Direction::Reverse, false, 0.0, 0.0)),
        ("LOOP", gauge(Direction::Loop, false, 0.0, 0.0)),
        ("PING", gauge(Direction::Ping, false, 0.0, 0.0)),
        ("TRON", gauge(Direction::Forward, true, 0.0, 0.0)),
        ("DRIFT", gauge(Direction::Forward, false, 1.0, 0.0)),
        ("FADE", gauge(Direction::Forward, false, 0.0, 0.6)),
    ] {
        assert_ne!(g.key(), base.key(), "{what}");
        assert!(pic(&g) != pic(&base), "{what}");
    }
    // Quantised: a move under a step leaves the key and the picture.
    let a = gauge(Direction::Loop, false, 0.5, 0.0);
    let b = gauge(Direction::Loop, false, 0.5 + 0.01, 0.0);
    assert_eq!(a.key(), b.key());
    assert!(pic(&a) == pic(&b));
    // No frame term: the same inputs draw alike, every time.
    assert!(pic(&a) == pic(&gauge(Direction::Loop, false, 0.5, 0.0)));
}

#[test]
#[ignore]
#[cfg(feature = "walk-styles")]
fn write_variants() {
    use chimera_core::ui::tape_glyph::{H, Palette, W};
    let dir = std::path::PathBuf::from(std::env::var("GLYPH_OUT").expect("GLYPH_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    let pal = Palette::THEME;
    let (f, r, l, p) = (
        Direction::Forward,
        Direction::Reverse,
        Direction::Loop,
        Direction::Ping,
    );
    // (slug, DIRECTION, TRON, DRIFT, FADE, POSITION, LENGTH)
    let mut poses = vec![
        ("fwd".to_string(), f, false, 0.0, 0.3, 0.15, 0.75),
        ("reverse".into(), r, false, 0.0, 0.3, 0.15, 0.75),
        ("loop".into(), l, false, 0.0, 0.3, 0.15, 0.75),
        ("ping".into(), p, false, 0.0, 0.3, 0.15, 0.75),
        ("fwd-drift".into(), f, false, 1.0, 0.3, 0.15, 0.75),
        ("fwd-drift-half".into(), f, false, 0.5, 0.3, 0.15, 0.75),
        ("loop-drift".into(), l, false, 1.0, 0.3, 0.15, 0.75),
        ("fwd-tron".into(), f, true, 0.0, 0.3, 0.15, 0.75),
        ("reverse-tron".into(), r, true, 0.0, 0.3, 0.15, 0.75),
        ("loop-tron".into(), l, true, 0.0, 0.3, 0.15, 0.75),
        ("ping-tron".into(), p, true, 0.0, 0.3, 0.15, 0.75),
        ("fwd-fade0".into(), f, false, 0.0, 0.0, 0.15, 0.75),
        ("fwd-fade1".into(), f, false, 0.0, 0.9, 0.15, 0.75),
        ("fwd-short".into(), f, false, 0.0, 0.3, 0.15, 0.3),
        ("fwd-late".into(), f, false, 0.0, 0.3, 0.55, 0.75),
        ("loop-short".into(), l, false, 0.0, 0.3, 0.7, 0.3),
    ];
    // POSITION swept under a short window, a frame a step: what a turn
    // animates (the glyph has no frame term of its own).
    for (name, d) in [("fwd", f), ("reverse", r), ("loop", l), ("ping", p)] {
        for k in 0..5 {
            let pos = k as f32 / 4.0;
            poses.push((format!("{name}-sweep{k}"), d, false, 0.0, 0.3, pos, 0.35));
        }
    }
    let only = std::env::var("GLYPH_STYLES").unwrap_or_default();
    for style in StepStyle::ALL {
        let name = format!("{style:?}").to_lowercase();
        if !only.is_empty() && !only.split(',').any(|o| o == name) {
            continue;
        }
        for ((slug, d, tron, drift, fade, pos, len), s) in
            poses.iter().cloned().flat_map(|p| [(p.clone(), 1), (p, 4)])
        {
            let mut set = set(d, tron, drift, fade);
            set[3] = pos;
            set[5] = len;
            let g = TapeGauge::of(&GlyphInputs::from_set(set, Pose::Live));
            let c = canvas(&g, style);
            let (w, h) = (W * s, H * s);
            let row = (w * 3 + 3) & !3;
            let mut b = Vec::new();
            let size = 54 + row * h;
            b.extend_from_slice(b"BM");
            b.extend_from_slice(&(size as u32).to_le_bytes());
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&54u32.to_le_bytes());
            b.extend_from_slice(&40u32.to_le_bytes());
            b.extend_from_slice(&w.to_le_bytes());
            b.extend_from_slice(&h.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&24u16.to_le_bytes());
            b.extend_from_slice(&[0; 24]);
            for y in (0..h).rev() {
                let mut n = 0;
                for x in 0..w {
                    let ink = c.at(x / s, y / s).unwrap();
                    let v = pal.of(ink);
                    use embedded_graphics::pixelcolor::RgbColor;
                    let (r, g, bl) = (v.r() << 3, v.g() << 2, v.b() << 3);
                    b.extend_from_slice(&[bl, g, r]);
                    n += 3;
                }
                b.extend(std::iter::repeat_n(0u8, (row - n) as usize));
            }
            let file = format!("glyph_{name}_{slug}_{s}x.bmp");
            std::fs::write(dir.join(file), b).unwrap();
        }
    }
}
