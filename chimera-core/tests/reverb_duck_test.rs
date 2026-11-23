// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! REV › DUCK (ADR 0295): the wet return turned down under the reverb's
//! own input and back in the gaps. AMOUNT 0 is the reverb as before, bit
//! for bit (`reverb_dispatch_test`'s and the other reverb goldens hold
//! it, AMOUNT 0 their default); here, that a duck ducks, lets go over
//! RELEASE, and that its page and words are where they say.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{DUCK_RELEASE_DEFAULT, ReverbParams, ReverbType};
use chimera_core::preset::Sound;
use chimera_core::ui::block_registry::{EFX, EFX_DUCK};
use chimera_core::ui::{notes, theme};
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::rev::Noise;

/// A HALL fed a loud noise for 120 blocks, then nothing for 300: each
/// block's wet RMS.
fn run(duck: f32, release: f32) -> Vec<f32> {
    let mut rv = Box::new(Reverb::new());
    let mut n = Noise(0x0bad_cafe);
    let p = ReverbParams {
        kind: ReverbType::Hall,
        time: 0.8,
        duck,
        duck_release: release,
        ..Default::default()
    };
    (0..420)
        .map(|b| {
            let g = if b < 120 { 0.5 } else { 0.0 };
            let x: [f32; BLOCK_SIZE] = core::array::from_fn(|_| g * n.next());
            let mut wet = Stereo::SILENT;
            rv.process(&x, &p, 1.0, Tempo::DEFAULT, SR, &mut wet);
            (wet.l.iter().chain(&wet.r).map(|v| v * v).sum::<f32>() / (2 * BLOCK_SIZE) as f32)
                .sqrt()
        })
        .collect()
}

#[test]
fn amount_zero_and_default_release_is_the_reverb_as_it_was() {
    assert_eq!(ReverbParams::DEFAULT.duck, 0.0);
    let (a, b) = (run(0.0, DUCK_RELEASE_DEFAULT), run(0.0, 0.0));
    assert!(
        a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
        "RELEASE alone does nothing"
    );
}

#[test]
fn a_duck_turns_the_return_down_under_its_input_and_back_after() {
    let (dry, ducked) = (
        run(0.0, DUCK_RELEASE_DEFAULT),
        run(1.0, DUCK_RELEASE_DEFAULT),
    );
    // Under the input (the follower full): about MAX_DEPTH × the key's share down.
    let under = |v: &[f32]| v[60..120].iter().sum::<f32>();
    let ratio = under(&ducked) / under(&dry);
    assert!(ratio < 0.2, "ducked under the input: {ratio}");
    // RELEASE (300 ms, 225 blocks) after the input stops, the tail is back.
    let after = |v: &[f32]| v[350..420].iter().sum::<f32>();
    let back = after(&ducked) / after(&dry);
    assert!((back - 1.0).abs() < 1e-6, "back in the gap: {back}");
}

#[test]
fn a_longer_release_comes_back_slower() {
    let (fast, slow) = (run(1.0, 0.5), run(1.0, 1.0));
    let dry = run(0.0, 0.5);
    let at = |v: &[f32], b: usize| v[b] / dry[b];
    // 75 blocks (100 ms) after the input stops.
    assert!(
        at(&fast, 195) > at(&slow, 195),
        "{} vs {}",
        at(&fast, 195),
        at(&slow, 195)
    );
}

#[test]
fn duck_is_revs_page_with_amount_and_release() {
    assert_eq!((EFX_DUCK.name, EFX_DUCK.short), ("Reverb Duck", "DUCK"));
    let got: Vec<_> = (0..6).map(|i| EFX_DUCK.params[i].label()).collect();
    // Cell C: the return's SMART EQ role (ADR 0296), beside the duck.
    assert_eq!(got, ["AMOUNT", "RELEASE", "SMART ROLE", "--", "--", "--"]);
    let rev = |id| ParamAddr::new(BlockRef::Reverb, id);
    assert_eq!(
        rev(ReverbParams::DUCK).spec().map(|s| s.ident),
        Some("DUCK")
    );
    let s = Sound::init(chimera_core::params::EngineType::Algo);
    assert_eq!(
        notes::cell(rev(ReverbParams::DUCK), &s),
        Some(notes::DUCK_AMOUNT)
    );
    assert_eq!(
        notes::cell(rev(ReverbParams::DUCK_RELEASE), &s),
        Some(notes::DUCK_RELEASE)
    );
    for w in [notes::DUCK_AMOUNT, notes::DUCK_RELEASE] {
        let px = chimera_core::ui::draw::text_width(&theme::FONT_LABEL, w, 1);
        assert!(px <= theme::CONTENT_W, "{w}: {px}");
    }
    let _ = EFX.id;
}
