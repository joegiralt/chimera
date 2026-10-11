//! VA's focus glyphs (ADR 0129): each oscillator's scope, MODE's operator
//! scope, and BLEND's crossfader with its ends named by MODE.
//!
//! `SCREEN_DUMP=<dir>` writes the VA page per MODE, a WAVE sweep and
//! BLEND per MODE, dark and light (`va_*_dark`, `va_*_light`).

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::va::osc::{Morph, Unit, wave_at};
use chimera_core::dsp::va::{MODE_NAMES, VA_SPECS, VaMode, VaParams};
use chimera_core::params::EngineType;
use chimera_core::ui::UiState;
use chimera_core::ui::components::{SCOPE_HALF, SCOPE_N, VA_MIX_N};
use chimera_core::ui::glyph::{
    FocusGlyph, Gauge, VA_MIX_PARAMS, VaGlyph, VaMix, VaOsc, blend_ends, va_mix_trace,
    va_wave_trace,
};
use chimera_core::ui::theme_settings::{Accent, Black, ThemeSettings, palette};
use chimera_core::ui::{draw, theme};
use chimera_hal::EncoderId;
use screen::*;

fn morph(wave: f32, shape: f32) -> Morph {
    Morph {
        wave: Unit::new(wave),
        shape: Unit::new(shape),
    }
}

fn wave(m: Morph) -> [i32; SCOPE_N] {
    va_wave_trace::<SCOPE_N>(m, SCOPE_HALF)
}

fn mix(mode: VaMode, amount: f32) -> [i32; VA_MIX_N] {
    let saw = morph(2.0 / 3.0, 0.0);
    va_mix_trace::<VA_MIX_N>(
        &VaMix {
            mode,
            amount,
            osc: [saw, saw],
        },
        SCOPE_HALF,
    )
}

#[test]
fn the_specs_carry_the_va_glyphs() {
    let glyph = |id: chimera_core::block::ParamId| VA_SPECS[usize::from(id.0)].glyph;
    assert_eq!(
        glyph(VaParams::WAVE1),
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::One))
    );
    assert_eq!(
        glyph(VaParams::WAVE2),
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::Two))
    );
    assert_eq!(glyph(VaParams::MODE), FocusGlyph::Va(VaGlyph::Mode));
    assert_eq!(glyph(VaParams::AMOUNT), FocusGlyph::Va(VaGlyph::Blend));
    let va = |id| ParamAddr::new(BlockRef::Va, id);
    assert_eq!(
        VaOsc::One.params(),
        [va(VaParams::WAVE1), va(VaParams::SHAPE1)]
    );
    assert_eq!(
        VaOsc::Two.params(),
        [va(VaParams::WAVE2), va(VaParams::SHAPE2)]
    );
    assert_eq!(
        VA_MIX_PARAMS[..2],
        [va(VaParams::MODE), va(VaParams::AMOUNT)]
    );
}

/// The scope is the oscillator's own naive wave: at WAVE 0 the sine,
/// its crest a quarter in at full height.
#[test]
fn the_scope_traces_the_oscillators_own_wave() {
    let sine = wave(morph(0.0, 0.0));
    assert_eq!(sine[(SCOPE_N - 1) / 4], SCOPE_HALF);
    assert_eq!(sine[3 * (SCOPE_N - 1) / 4], -SCOPE_HALF);
    assert_eq!(sine[0], sine[SCOPE_N - 1], "one whole period");
    // Every point is `wave_at`'s, scaled to the peak.
    for m in [morph(0.4, 0.3), morph(0.9, 0.8)] {
        let ys: Vec<f32> = (0..SCOPE_N)
            .map(|i| wave_at(i as f32 / (SCOPE_N - 1) as f32, m, 1e-6).0)
            .collect();
        let peak = ys.iter().fold(0.0f32, |p, y| p.max(y.abs()));
        let t = wave(m);
        for (i, y) in ys.iter().enumerate() {
            assert_eq!(
                t[i],
                (y / peak * SCOPE_HALF as f32).round() as i32,
                "{m:?} {i}"
            );
        }
    }
}

/// WAVE slides: a step of WAVE (1/128) moves no point far, at any SHAPE,
/// across the segment edges too; between two waves the trace is a mix of
/// both, unlike either.
#[test]
fn the_scope_slides_continuously_as_wave_turns() {
    const BOUND: i32 = 4;
    for shape in [0.0, 0.5, 1.0] {
        let mut prev = wave(morph(0.0, shape));
        for k in 1..=128 {
            let t = wave(morph(k as f32 / 128.0, shape));
            let worst = (0..SCOPE_N).map(|i| (t[i] - prev[i]).abs()).max().unwrap();
            assert!(worst <= BOUND, "SHAPE {shape} WAVE {k}/128: {worst} px");
            prev = t;
        }
    }
    let (sine, tri) = (wave(morph(0.0, 0.0)), wave(morph(1.0 / 3.0, 0.0)));
    let between = wave(morph(1.0 / 6.0, 0.0));
    assert_ne!(between, sine);
    assert_ne!(between, tri);
}

/// MODE's glyph is the operator: each MODE draws its own picture.
#[test]
fn the_mode_glyph_differs_per_mode() {
    for amount in [0.5, 1.0] {
        let traces: Vec<_> = VaMode::ALL.iter().map(|&m| mix(m, amount)).collect();
        for i in 0..traces.len() {
            for j in i + 1..traces.len() {
                assert_ne!(
                    traces[i], traces[j],
                    "{} vs {} at {amount}",
                    MODE_NAMES[i], MODE_NAMES[j]
                );
            }
        }
    }
}

/// SYNC restarts OSC 2 on OSC 1's wrap: OSC 1's second period starts as
/// its first does; MIX, OSC 2 running on, does not.
#[test]
fn sync_resets_osc_2_on_osc_1s_wrap() {
    let half = (VA_MIX_N - 1) / 2;
    let sync = mix(VaMode::Sync, 1.0);
    assert_eq!(sync[..4], sync[half..half + 4]);
    let free = mix(VaMode::Mix, 1.0);
    assert_ne!(free[..4], free[half..half + 4]);
}

/// BITS at full AMOUNT is 2 bits: five levels at most.
#[test]
fn bits_crushes_to_a_few_levels() {
    let mut levels: Vec<i32> = mix(VaMode::Bits, 1.0).to_vec();
    levels.sort();
    levels.dedup();
    assert!(levels.len() <= 5, "{levels:?}");
    let mut clean: Vec<i32> = mix(VaMode::Bits, 0.0).to_vec();
    clean.sort();
    clean.dedup();
    assert!(clean.len() > 5, "{clean:?}");
}

#[test]
fn blend_ends_name_what_amount_does() {
    assert_eq!(blend_ends(VaMode::Mix), ["1", "2"]);
    assert_eq!(blend_ends(VaMode::Sync), ["1", "2"]);
    assert_eq!(blend_ends(VaMode::Ring), ["MIX", "RING"]);
    assert_eq!(blend_ends(VaMode::Xor), ["MIX", "XOR"]);
    assert_eq!(blend_ends(VaMode::Bits), ["CLEAN", "CRUSH"]);
}

fn label_w(s: &str) -> i32 {
    draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING)
}

/// BLEND and STABILITY fit their cells; each MODE's end words fit
/// under BLEND's track, apart.
#[test]
fn the_new_words_fit() {
    for l in ["BLEND", "STABILITY"] {
        assert!(label_w(l) <= theme::CELL_BAR_W, "{l}: {} px", label_w(l));
    }
    for m in VaMode::ALL {
        let [l, r] = blend_ends(m);
        let w = label_w(l) + theme::XF_ENDS_GAP + label_w(r);
        assert!(w <= theme::BLEND_W, "{m:?}: {w} px");
    }
}

/// Every value the VA glyphs' params show ends before their gauge.
#[test]
fn values_stay_clear_of_the_glyphs() {
    const GAP: i32 = 4;
    let right = |w: &str| theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, w, 0);
    let edge = theme::SCREEN_W - theme::MARGIN_X;
    for name in MODE_NAMES {
        assert!(right(name) + GAP <= edge - theme::VA_MIX_W, "{name}");
    }
    for v in 0..=127 {
        let v = v.to_string();
        assert!(right(&v) + GAP <= edge - theme::BLEND_W, "{v}");
        assert!(right(&v) + GAP <= theme::WAVE_X, "{v}");
    }
}

#[test]
fn va_gauges_are_still_and_keyed_on_what_they_read() {
    let a = Gauge::VaWave(morph(0.5, 0.0));
    let b = Gauge::VaWave(morph(0.5, 0.5));
    assert!(!a.animates());
    assert_ne!(a.outside_key(), b.outside_key(), "SHAPE redraws the band");
    let m = |mode| Gauge::Blend { value: 0.5, mode };
    assert_ne!(m(VaMode::Ring).outside_key(), m(VaMode::Bits).outside_key());
    assert_eq!(
        Gauge::Arc {
            value: 0.5,
            bipolar: false
        }
        .outside_key(),
        0
    );
}

// --- The page, drawn ---

/// The owner's unit: TEAL, BLACK −2, dark or INVERT.
fn shoot(ui: &UiState, name: &str) -> Fb {
    let fb = render_ui(ui);
    assert_eq!(fb.oob, 0, "{name}");
    for invert in [false, true] {
        let p = palette(&ThemeSettings {
            invert,
            accent: Accent::Teal,
            black: Black::new(-2),
            ..ThemeSettings::DEFAULT
        });
        let mut shot = Fb::new();
        shot.px = fb.px.iter().map(|&px| p.map_raw(px)).collect();
        shot.dump(&format!("{name}_{}", if invert { "light" } else { "dark" }));
    }
    fb
}

fn set(ui: &mut UiState, e: EncoderId, ticks: i8) {
    feed(ui, Input::turn(e, -127));
    feed(ui, Input::turn(e, ticks));
    settle(ui);
}

fn va(ui: &UiState) -> VaParams {
    ui.params().va
}

/// The focus band's gauge box: what the glyph drew.
fn band(fb: &Fb, w: i32) -> Vec<u16> {
    let x0 = theme::SCREEN_W - theme::MARGIN_X - w;
    (theme::WAVE_Y..theme::FOCUS_BOTTOM)
        .flat_map(|y| (x0..x0 + w).map(move |x| fb.px[y as usize * W + x as usize]))
        .collect()
}

#[test]
fn the_va_page_draws_each_glyph() {
    let mut ui = UiState::new();
    load_init(&mut ui, EngineType::Va);
    // MODE per mode: its scope on the band, the word left of it.
    let mut seen = Vec::new();
    for (k, m) in VaMode::ALL.into_iter().enumerate() {
        set(&mut ui, EncoderId::C, k as i8);
        assert_eq!(va(&ui).mode, m);
        let fb = shoot(&ui, &format!("va_mode_{}", MODE_NAMES[k].to_lowercase()));
        let b = band(&fb, theme::VA_MIX_W);
        assert!(!seen.contains(&b), "{m:?}: a picture of its own");
        seen.push(b);
    }
    // BLEND per mode: the end words change with MODE.
    let mut seen = Vec::new();
    for (k, m) in VaMode::ALL.into_iter().enumerate() {
        set(&mut ui, EncoderId::C, k as i8);
        set(&mut ui, EncoderId::D, 32);
        assert_eq!(va(&ui).mode, m);
        let fb = shoot(&ui, &format!("va_blend_{}", MODE_NAMES[k].to_lowercase()));
        let b = band(&fb, theme::BLEND_W);
        if m != VaMode::Sync {
            assert!(!seen.contains(&b), "{m:?}: its own ends");
        }
        seen.push(b);
    }
    // WAVE 1 swept: sine, sine/tri, tri/saw, saw/pulse, pulse.
    set(&mut ui, EncoderId::C, 0);
    let mut seen = Vec::new();
    for ticks in [0i8, 21, 64, 107, 127] {
        set(&mut ui, EncoderId::A, ticks);
        let fb = shoot(&ui, &format!("va_wave1_{ticks:03}"));
        let b = band(&fb, theme::WAVE_W);
        assert!(!seen.contains(&b), "WAVE {ticks}");
        seen.push(b);
    }
}

/// SHAPE, on OSC 1's page, reshapes WAVE 1's scope on VA's.
#[test]
fn the_scope_reads_shape() {
    let mut ui = UiState::new();
    load_init(&mut ui, EngineType::Va);
    set(&mut ui, EncoderId::A, 64);
    let before = band(&render_ui(&ui), theme::WAVE_W);
    ui.project_mut()
        .edit_part(chimera_core::project::PartId::ALL[0])
        .sound
        .params
        .va
        .shape[0] = 0.8;
    settle(&mut ui);
    assert_ne!(band(&render_ui(&ui), theme::WAVE_W), before);
}
