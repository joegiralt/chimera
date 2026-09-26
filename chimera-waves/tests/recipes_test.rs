use chimera_waves::{
    MAX_HARMONIC, MIPS, RECIPES, Recipe, Shape, WAVE_LEN, emit_rust, mip_harmonics, render,
    spectrum,
};
use std::f64::consts::PI;

fn naive(r: &Recipe) -> Vec<f64> {
    match r.shape {
        Shape::Time(f) => (0..1024).map(|n| f(n as f64 / 1024.0)).collect(),
        Shape::Sines(_) => Vec::new(),
    }
}

fn mean(mip: &[i16; WAVE_LEN]) -> f64 {
    mip.iter().map(|&v| v as f64).sum::<f64>() / WAVE_LEN as f64 / 32767.0
}

#[test]
fn every_recipe_is_finite_within_unit_peak_and_full_scale() {
    for r in &RECIPES {
        assert!(
            naive(r)
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.0 + 1e-12),
            "{}",
            r.name
        );
        let mips = render(r);
        let peak = mips.iter().flatten().map(|v| v.unsigned_abs()).max();
        assert_eq!(peak, Some(32767), "{} is scaled to full scale", r.name);
        assert!(mips.iter().all(|m| m.iter().any(|&v| v != 0)), "{}", r.name);
    }
}

#[test]
fn only_the_tx_waves_keep_dc_and_the_rest_are_centred() {
    for r in &RECIPES {
        let tx = r.name.starts_with('W');
        assert_eq!(r.keeps_dc, tx, "{}", r.name);
        if !tx {
            for (m, mip) in render(r).iter().enumerate() {
                assert!(
                    mean(mip).abs() < 1e-4,
                    "{} mip {m}: DC {}",
                    r.name,
                    mean(mip)
                );
            }
        }
    }
}

/// W3, W4, W7 and W8 are half-wave shapes whose DC is part of the sound;
/// W1, W2, W5 and W6 have none.
#[test]
fn tx_dc_is_where_the_shape_puts_it() {
    let dc = |i: usize| spectrum(RECIPES[i].shape).dc;
    for i in [0, 1, 4, 5] {
        assert!(dc(i).abs() < 1e-9, "{}", RECIPES[i].name);
    }
    assert!((dc(2) - 1.0 / PI).abs() < 1e-6, "W3 is a half sine");
    for i in [3, 6, 7] {
        assert!(dc(i) > 0.1, "{}", RECIPES[i].name);
    }
}

#[test]
fn a_sine_has_only_its_fundamental() {
    let s = spectrum(RECIPES[0].shape);
    assert!((s.sin[1] - 1.0).abs() < 1e-9);
    for k in 2..=MAX_HARMONIC {
        assert!(
            s.sin[k].abs() < 1e-9 && s.cos[k].abs() < 1e-9,
            "harmonic {k}"
        );
    }
}

#[test]
fn a_saw_falls_as_one_over_k() {
    let s = spectrum(RECIPES[9].shape);
    for k in 1..=16 {
        let want = 2.0 / (PI * k as f64);
        let got = s.sin[k].hypot(s.cos[k]);
        assert!(
            (got - want).abs() < 1e-3,
            "harmonic {k}: {got} (want {want})"
        );
    }
}

#[test]
fn each_mip_keeps_half_the_harmonics_of_the_one_before() {
    let h: Vec<usize> = (0..MIPS).map(mip_harmonics).collect();
    assert_eq!(h, [127, 64, 32, 16, 8, 4, 2, 1]);
}

#[test]
fn the_emitted_source_declares_both_tables() {
    let src = emit_rust();
    assert!(src.contains("pub static WAVE_NAMES: [&str; 16]"));
    assert!(src.contains("pub static WAVES: [[[i16; 257]; 8]; 16]"));
}
