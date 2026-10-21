//! WALK's footsteps glyph (ADR 0203): DIRECTION lays them, DRIFT scatters
//! them, FADE fades the last, TRON marks its tape's end; quantised inputs,
//! no frame term. `GLYPH_OUT=dir … -- --ignored` writes the owner's
//! variants as BMPs, 4× (each style, each pose).

use chimera_core::dsp::tape_engine::Direction;
use chimera_core::ui::tape_glyph::{
    Canvas, GlyphInputs, H, Kind, Machine, Palette, Pose, StepStyle, TapeGauge, W,
};

/// The glyph's set values for WALK in `d`, TRON `tron`, DRIFT `drift`.
fn set(d: Direction, tron: bool, drift: f32, fade: f32) -> [f32; 13] {
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
fn write_variants() {
    let dir = std::path::PathBuf::from(std::env::var("GLYPH_OUT").expect("GLYPH_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    let pal = Palette::THEME;
    let poses = [
        ("fwd", Direction::Forward, false, 0.0),
        ("reverse", Direction::Reverse, false, 0.0),
        ("loop", Direction::Loop, false, 0.0),
        ("ping", Direction::Ping, false, 0.0),
        ("fwd-drift", Direction::Forward, false, 1.0),
        ("loop-drift", Direction::Loop, false, 1.0),
        ("fwd-tron", Direction::Forward, true, 0.0),
        ("loop-tron", Direction::Loop, true, 0.0),
    ];
    for (k, style) in StepStyle::ALL.into_iter().enumerate() {
        for (slug, d, tron, drift) in poses {
            let c = canvas(&gauge(d, tron, drift, 0.3), style);
            let s = 4;
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
            let name = format!(
                "glyph_{}_{}{}.bmp",
                ["prints", "treads", "trail"][k],
                slug,
                ""
            );
            std::fs::write(dir.join(name), b).unwrap();
        }
    }
}
