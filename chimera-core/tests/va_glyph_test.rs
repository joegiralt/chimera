//! VA's focus glyphs (ADR 0129): each oscillator's scope, MODE's operator
//! scope, and BLEND's crossfader with its ends named by MODE.
//!
//! `SCREEN_DUMP=<dir>` writes the VA page per MODE, a WAVE sweep and
//! BLEND per MODE, dark and light (`va_*_dark`, `va_*_light`).

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::va::osc::{Morph, Unit, wave_at};
use chimera_core::dsp::va::{MODE_NAMES, VA_SPECS, VaMode, VaParams};
use chimera_core::params::EngineType;
use chimera_core::ui::UiState;
use chimera_core::ui::components::{self, Layout, SCOPE_HALF, SCOPE_N, VA_MIX_N};
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
    // SHAPE draws the wave it makes (#980): its oscillator's scope.
    assert_eq!(
        glyph(VaParams::SHAPE1),
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::One))
    );
    assert_eq!(
        glyph(VaParams::SHAPE2),
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
                // AM's top is RING's product (ADR 0176)
                let same = [VaMode::ALL[i], VaMode::ALL[j]] == [VaMode::Ring, VaMode::Am];
                if same && amount == 1.0 {
                    assert_eq!(traces[i], traces[j], "AM at 1 is RING");
                    continue;
                }
                assert_ne!(
                    traces[i], traces[j],
                    "{} vs {} at {amount}",
                    MODE_NAMES[i], MODE_NAMES[j]
                );
            }
        }
    }
}

/// FNV-1a over MODE's scope of two saws at AMOUNT `amount`.
fn trace_hash(mode: VaMode, amount: f32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for y in mix(mode, amount) {
        for b in y.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Every MODE's scope at BLEND 0.5 and 1, locked.
const MODE_GOLDENS: &[(&str, u64)] = &[
    ("mix_050", 0x9cc5eeb9241b78d5),
    ("mix_100", 0x8a8e3f6bb7477f78),
    ("sync_050", 0x9efcf5a7bbdf2a96),
    ("sync_100", 0x5a0b8f059dd06d9e),
    ("ring_050", 0x2b4d7a7ee457441e),
    ("ring_100", 0xaa65c3e6b8fd1365),
    ("xor_050", 0x5de78f0972e297c0),
    ("xor_100", 0x82b25a12dbe07bbe),
    ("bits_050", 0x39fcb39b11f54c7e),
    ("bits_100", 0xc67fbc52da2a4a08),
    ("fm_050", 0x53047951d090ece0),
    ("fm_100", 0x8f8eca18b59af852),
    ("loopfm_050", 0x809dbe700aabe2e8),
    ("loopfm_100", 0xb7a4016c646be97c),
    ("am_050", 0x02fb52e6dd09be3c),
    ("am_100", 0xaa65c3e6b8fd1365),
    ("max_050", 0x1831da0c4eb7b178),
    ("max_100", 0x884d5c6f119105ce),
    ("soft sync_050", 0xf3568a3904c84f59),
    ("soft sync_100", 0x1a37f470abe6e62d),
    ("phase_050", 0x6a9a5783df5824c8),
    ("phase_100", 0x3ae42e0f39f1e69b),
];

#[test]
fn mode_scopes_match_their_goldens() {
    let names: Vec<String> = VaMode::ALL
        .iter()
        .enumerate()
        .flat_map(|(k, _)| ["050", "100"].map(|a| format!("{}_{a}", MODE_NAMES[k].to_lowercase())))
        .collect();
    let got: Vec<(&str, u64)> = VaMode::ALL
        .iter()
        .flat_map(|&m| [trace_hash(m, 0.5), trace_hash(m, 1.0)])
        .zip(&names)
        .map(|(h, n)| (n.as_str(), h))
        .collect();
    common::golden::check(MODE_GOLDENS, &got);
}

/// FM's scope is OSC 1 read through zero: at BLEND 0 it is MIX's OSC 1
/// alone; at BLEND 1 it differs from it.
#[test]
fn fm_scope_reads_osc_1_through_osc_2() {
    assert_eq!(mix(VaMode::Fm, 0.0), mix(VaMode::Mix, 0.0));
    assert_ne!(mix(VaMode::Fm, 1.0), mix(VaMode::Mix, 0.0));
}

/// LOOPFM's scope draws both oscillators as loopback FM: BLEND moves it
/// (mix and loop together), and it is not MIX's.
#[test]
fn loopfm_scope_loops_with_blend() {
    assert_ne!(mix(VaMode::LoopFm, 0.0), mix(VaMode::LoopFm, 1.0));
    assert_ne!(mix(VaMode::LoopFm, 0.5), mix(VaMode::Mix, 0.5));
}

/// AM's scope is OSC 1 scaled by OSC 2 unipolarly: at BLEND 0 it is MIX's
/// OSC 1 alone; at BLEND 1 it differs from it.
#[test]
fn am_scope_scales_osc_1_by_osc_2() {
    assert_eq!(mix(VaMode::Am, 0.0), mix(VaMode::Mix, 0.0));
    assert_ne!(mix(VaMode::Am, 1.0), mix(VaMode::Mix, 0.0));
}

/// MAX's scope is the plain mix at BLEND 0 and rides above it at BLEND 1
/// (the larger of the two is never below their mean).
#[test]
fn max_scope_rises_from_the_mix() {
    let (lo, hi) = (mix(VaMode::Max, 0.0), mix(VaMode::Max, 1.0));
    assert_ne!(lo, hi);
    assert_eq!(lo, mix(VaMode::Mix, 0.5));
}

/// PHASE's scope is OSC 1 bent by OSC 2: at BLEND 0 it is MIX's OSC 1
/// alone; at BLEND 1 it differs from it.
#[test]
fn phase_scope_bends_osc_1() {
    assert_eq!(mix(VaMode::Phase, 0.0), mix(VaMode::Mix, 0.0));
    assert_ne!(mix(VaMode::Phase, 1.0), mix(VaMode::Mix, 0.0));
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

/// SOFT SYNC runs OSC 2 back over OSC 1's second period: at BLEND 1 the
/// second half is the first mirrored.
#[test]
fn soft_sync_mirrors_osc_2_on_osc_1s_wrap() {
    let t = mix(VaMode::SoftSync, 1.0);
    let n = t.len();
    for i in 0..n / 2 {
        assert!(
            (t[i] - t[n - 1 - i]).abs() <= 1,
            "{i}: {} {}",
            t[i],
            t[n - 1 - i]
        );
    }
    assert_ne!(t, mix(VaMode::Sync, 1.0));
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
    assert_eq!(blend_ends(VaMode::Fm), ["CLEAN", "DEEP"]);
    assert_eq!(blend_ends(VaMode::LoopFm), ["1", "LOOP"]);
    assert_eq!(blend_ends(VaMode::Am), ["CLEAN", "RING"]);
    assert_eq!(blend_ends(VaMode::SoftSync), ["1", "2"]);
    assert_eq!(blend_ends(VaMode::Phase), ["CLEAN", "WARP"]);
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

/// A MODE name too wide for the large numerals beside the scope reads in
/// the value font, and its ink stays clear of the scope's box.
#[test]
fn a_long_mode_name_reads_small_beside_the_scope() {
    const GAP: usize = components::GAUGE_GAP as usize;
    let gauge = Gauge::VaMix(VaMix::from_set([0.0; 6]));
    assert_eq!(components::layout("LOOPFM", &gauge), Layout::SmallBeside);
    assert_eq!(components::layout("MIX", &gauge), Layout::WithGauge);
    let mut both = Fb::new();
    components::focus_band(
        &mut both,
        "MODE",
        "LOOPFM",
        components::fit_gauge("LOOPFM", gauge),
        chimera_core::ui::components::Look::Live,
        None,
        theme::TextSize::Normal,
    );
    let mut scope = Fb::new();
    components::draw_gauge(&mut scope, gauge);
    let left = (theme::SCREEN_W - theme::MARGIN_X - theme::VA_MIX_W) as usize;
    let rows = theme::WAVE_Y as usize..theme::FOCUS_BOTTOM as usize;
    for invert in [false, true] {
        let p = palette(&ThemeSettings {
            invert,
            ..ThemeSettings::DEFAULT
        });
        let bg = p.map_raw(0);
        let ink = |fb: &Fb, x: usize, y: usize| p.map_raw(fb.px[y * W + x]) != bg;
        let mut text = 0;
        for y in rows.clone() {
            for x in 0..W {
                let t = ink(&both, x, y) && !ink(&scope, x, y);
                // the text's ink: never in the gap or over the scope
                assert!(!(t && x + GAP > left), "invert {invert}: ink at ({x}, {y})");
                text += usize::from(t);
            }
        }
        assert!(text > 100, "the word is drawn: {text}");
        let drew = rows
            .clone()
            .any(|y| (left..W).any(|x| ink(&both, x, y) && ink(&scope, x, y)));
        assert!(drew, "the scope is kept");
    }
}

/// Every value the VA glyphs' params show ends before their gauge.
#[test]
fn values_stay_clear_of_the_glyphs() {
    const GAP: i32 = 4;
    let right = |w: &str| theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, w, 0);
    let edge = theme::SCREEN_W - theme::MARGIN_X;
    for name in MODE_NAMES {
        // large beside the scope, or the value font beside it: never over it
        let gauge = Gauge::VaMix(VaMix::from_set([0.0; 6]));
        assert_ne!(components::layout(name, &gauge), Layout::NoGauge, "{name}");
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
        // SYNC and SOFT SYNC fade OSC 1 to OSC 2 as MIX does
        if !matches!(m, VaMode::Sync | VaMode::SoftSync) {
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

/// Every MODE word reads whole beside the scope: large, or small in
/// `SmallBeside`, never dropped to the no-scope layout.
#[test]
fn every_mode_word_fits_beside_the_scope() {
    let gauge = Gauge::VaMix(VaMix::from_set([0.0; 6]));
    for name in chimera_core::dsp::va::MODE_NAMES {
        let l = components::layout(name, &gauge);
        assert!(
            matches!(l, Layout::WithGauge | Layout::SmallBeside),
            "{name}: {l:?}"
        );
    }
    assert_eq!(components::layout("SOFT SYNC", &gauge), Layout::SmallBeside);
    assert_eq!(components::layout("PHASE", &gauge), Layout::SmallBeside);
}

// --- SHAPE's picture (#980) ---

/// FNV-1a over the scope's own box: clear of the hint line under it,
/// whose words follow the build's features.
fn band_hash(fb: &Fb) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let box_px = (theme::WAVE_Y..theme::WAVE_Y + theme::WAVE_H)
        .flat_map(|y| (theme::WAVE_X..theme::WAVE_X + theme::WAVE_W).map(move |x| (x, y)))
        .map(|(x, y)| fb.px[y as usize * W + x as usize]);
    for px in box_px {
        for b in px.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// WAVE 1's ticks at each wave, and between each pair.
const WAVES: [(&str, i8); 4] = [("sine", 0), ("tri", 42), ("saw", 85), ("pulse", 127)];
const BETWEEN: [(&str, i8); 3] = [("sine_tri", 21), ("tri_saw", 64), ("saw_pulse", 106)];
const SHAPES: [(&str, i8); 3] = [("000", 0), ("064", 64), ("127", 127)];

/// SHAPE touched on OSC 1's page: the scope at each WAVE and SHAPE 0, ½
/// and 1, and at each WAVE between two at SHAPE ½, locked.
const SHAPE_GOLDENS: &[(&str, u64)] = &[
    ("sine_000", 0x8cf18810ec3842d9),
    ("sine_064", 0x1428c67fa9926f89),
    ("sine_127", 0x4a21fccffc27b005),
    ("tri_000", 0x65701d6b45a1eb9b),
    ("tri_064", 0xd3cf3780a752d091),
    ("tri_127", 0xce8ff0102c991d37),
    ("saw_000", 0xb1f0d12ac0d35f7c),
    ("saw_064", 0xb54a65f5c09f3c0c),
    ("saw_127", 0x4ec6a504dc754def),
    ("pulse_000", 0xf4944560618be214),
    ("pulse_064", 0x803b9edc78c80cba),
    ("pulse_127", 0x5a0d843e2c26bd59),
    ("sine_tri_064", 0x3c8a24d89b44b2d9),
    ("tri_saw_064", 0x90ba6335de13e29e),
    ("saw_pulse_064", 0x4f700a1771eac29d),
];

/// VA at WAVE 1 `wave` ticks, on OSC 1's page with SHAPE at `shape`
/// ticks, touched.
fn shaped(wave: i8, shape: i8) -> Box<UiState> {
    let mut ui = Box::new(stack_ui());
    load_init(&mut ui, EngineType::Va);
    set(&mut ui, EncoderId::A, wave);
    tap(&mut ui, chimera_hal::ButtonId::Edit);
    assert_eq!(
        ui.page_def().id,
        chimera_core::ui::block_registry::VA_OSC1.id
    );
    set(&mut ui, EncoderId::C, shape);
    ui
}

/// SHAPE's band is its oscillator's wave as SHAPE makes it, per WAVE: a
/// picture of its own at every WAVE and SHAPE, the one WAVE's cell draws
/// at the same two values, from the knob's value alone.
#[test]
fn shape_draws_the_wave_it_makes() {
    let mut got = Vec::new();
    let mut seen = Vec::new();
    let cases = WAVES
        .iter()
        .flat_map(|&(w, wt)| SHAPES.map(|(s, st)| (format!("{w}_{s}"), wt, st)))
        .chain(BETWEEN.iter().map(|&(w, wt)| (format!("{w}_064"), wt, 64)));
    for (name, wave, shape) in cases {
        let mut ui = shaped(wave, shape);
        let fb = shoot(&ui, &format!("va_shape_{name}"));
        let b = band(&fb, theme::WAVE_W);
        // The scope, not the arc: the oscillator's own trace, drawn.
        let v = va(&ui);
        let t = va_wave_trace::<SCOPE_N>(morph(v.wave[0], v.shape[0]), SCOPE_HALF);
        let ink = b.iter().filter(|&&px| px != b[0]).count();
        assert!(ink >= SCOPE_N, "{name}: {ink} px of scope");
        assert!(t.iter().any(|&y| y != t[0]), "{name}");
        assert!(!seen.contains(&b), "{name}: a picture of its own");
        seen.push(b.clone());
        got.push((name.clone(), band_hash(&fb)));
        // WAVE's cell on VA's page draws the same picture.
        tap(&mut ui, chimera_hal::ButtonId::Seq);
        assert_eq!(
            ui.page_def().id,
            chimera_core::ui::block_registry::VA_HOME.id
        );
        feed(&mut ui, Input::turn(EncoderId::A, 1));
        feed(&mut ui, Input::turn(EncoderId::A, -1));
        settle(&mut ui);
        if !matches!(wave, 0 | 127) {
            assert_eq!(va(&ui).wave[0], v.wave[0], "{name}");
            assert!(
                band(&render_ui(&ui), theme::WAVE_W) == b,
                "{name}: WAVE's picture"
            );
        }
    }
    let got: Vec<(&str, u64)> = got.iter().map(|(n, h)| (n.as_str(), *h)).collect();
    common::golden::check(SHAPE_GOLDENS, &got);
}

/// OSC 2's SHAPE draws OSC 2's wave, and a turn of it redraws the band.
#[test]
fn shape_2_draws_oscillator_2() {
    let mut ui = Box::new(stack_ui());
    load_init(&mut ui, EngineType::Va);
    set(&mut ui, EncoderId::B, 127);
    tap(&mut ui, chimera_hal::ButtonId::Edit);
    tap(&mut ui, chimera_hal::ButtonId::Edit);
    assert_eq!(
        ui.page_def().id,
        chimera_core::ui::block_registry::VA_OSC2.id
    );
    set(&mut ui, EncoderId::C, 0);
    let wide = band(&render_ui(&ui), theme::WAVE_W);
    set(&mut ui, EncoderId::C, 100);
    let narrow = band(&render_ui(&ui), theme::WAVE_W);
    assert!(wide != narrow, "the pulse narrows");
    // WAVE 1 is the saw still: OSC 1's SHAPE at 0 draws another wave.
    assert!(band(&render_ui(&shaped(85, 0)), theme::WAVE_W) != wide);
}

/// A route into SHAPE moves the sound, never the picture: the cell is
/// the knob.
#[test]
fn shape_modulated_draws_the_knob() {
    let ui = shaped(85, 64);
    let g = |ui: &UiState| band(&render_ui(ui), theme::WAVE_W);
    let before = g(&ui);
    let mut ui = ui;
    for _ in 0..8 {
        ui.update(chimera_core::ui::animation::UiTick::for_test());
    }
    assert!(g(&ui) == before, "still: nothing animates it");
}
