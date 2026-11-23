// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! AMP's BASE (ADR 0292): the stored VCA value is where the sum of the
//! routes into VCA starts. 0, every saved Sound's, is today's VCA bit for
//! bit (`vca_lpg_test`'s and the goldens' hashes hold it); with no route
//! it does nothing; it ducks or swells under PRESS, ramps across a block,
//! and holds a voice as the engine would with no route.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Own;
use chimera_core::modulation::{FixedCol, ModSource, ModState};
use chimera_core::params::{EngineType, OutParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::ui::{block_registry::AMP, notes, view};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::rig::Rig;

const SR: u32 = chimera_hal::SAMPLE_RATE;
const BASE: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::VCA);

/// `engine`'s INIT with only `routes` into VCA (source, amount) and BASE
/// at `base`.
fn sound(engine: EngineType, base: f32, routes: &[(ModSource, i8)]) -> (ParamSnapshot, ModState) {
    let s = Sound::init(engine);
    let mut m = s.mod_state.clone();
    for src in ModSource::ALL {
        m.clear_fixed(src.index(), FixedCol::Vca);
    }
    for &(src, a) in routes {
        m.set_fixed(src.index(), FixedCol::Vca, a);
    }
    let mut p = s.params;
    p.out.vca = base;
    (p, m)
}

/// `blocks` blocks of C3 at 100 under pressure `press`, the key held.
fn run(p: &ParamSnapshot, m: &ModState, press: f32, blocks: usize) -> Vec<f32> {
    let mut r = Rig::new(SR);
    r.note_on(MidiNote::new(48).unwrap(), Velocity::new(100).unwrap(), p);
    r.ease_own(
        Own {
            bend: 0.0,
            press,
            timbre: 0.0,
        },
        None,
    );
    let mut b = [0.0f32; BLOCK_SIZE];
    let mut out = Vec::new();
    for _ in 0..blocks {
        r.render(&mut b, p, m);
        out.extend_from_slice(&b);
    }
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn base_without_a_route_changes_nothing() {
    for e in [EngineType::Algo, EngineType::Modal, EngineType::Va] {
        let (p0, m) = sound(e, 0.0, &[]);
        let (p1, _) = sound(e, 0.7, &[]);
        let (a, b) = (run(&p0, &m, 0.0, 40), run(&p1, &m, 0.0, 40));
        assert!(a.iter().any(|&x| x != 0.0), "{e:?} sounds");
        assert!(
            a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()),
            "{e:?}: BASE with no route is the engine's own VCA"
        );
    }
}

/// BASE 127 and PRESS −64: full with no pressure, half with all of it.
#[test]
fn base_full_and_press_down_ducks() {
    let (p, m) = sound(EngineType::Algo, 1.0, &[(ModSource::Press, -64)]);
    let (free, pressed) = (run(&p, &m, 0.0, 40), run(&p, &m, 1.0, 40));
    let (a, b) = (
        rms(&free[20 * BLOCK_SIZE..]),
        rms(&pressed[20 * BLOCK_SIZE..]),
    );
    assert!(a > 0.01, "a held note sounds at BASE: {a}");
    let ratio = b / a;
    assert!(
        (ratio - (1.0 - 64.0 / 127.0)).abs() < 0.01,
        "ducked by half: {ratio}"
    );
}

/// BASE 64 and PRESS +63: half with no pressure, full with all of it.
#[test]
fn base_half_and_press_up_swells() {
    let (p, m) = sound(EngineType::Algo, 0.5, &[(ModSource::Press, 63)]);
    let (free, pressed) = (run(&p, &m, 0.0, 40), run(&p, &m, 1.0, 40));
    let (a, b) = (
        rms(&free[20 * BLOCK_SIZE..]),
        rms(&pressed[20 * BLOCK_SIZE..]),
    );
    let ratio = b / a;
    let want = (0.5 + 63.0 / 127.0) / 0.5;
    assert!((ratio - want).abs() < 0.02, "swelled ×{want}: {ratio}");
}

/// BASE 0 under PRESS alone is today's pressure-only VCA: silent at rest.
#[test]
fn base_zero_and_press_alone_is_silent_at_rest() {
    let (p, m) = sound(EngineType::Algo, 0.0, &[(ModSource::Press, 127)]);
    assert!(run(&p, &m, 0.0, 20).iter().all(|&x| x == 0.0));
}

/// A BASE turned mid-note ramps across the block: the gain goes from the
/// old value to the new one linearly, never in a step.
#[test]
fn a_base_change_ramps_across_the_block() {
    let (p0, m) = sound(EngineType::Algo, 0.2, &[(ModSource::Press, 0)]);
    let mut p1 = p0.clone();
    p1.out.vca = 1.0;
    let note = MidiNote::new(48).unwrap();
    let vel = Velocity::new(100).unwrap();
    let (mut steady, mut turned) = (Rig::new(SR), Rig::new(SR));
    steady.note_on(note, vel, &p0);
    turned.note_on(note, vel, &p0);
    let (mut a, mut b) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
    for _ in 0..30 {
        steady.render(&mut a, &p0, &m);
        turned.render(&mut b, &p0, &m);
    }
    steady.render(&mut a, &p0, &m);
    turned.render(&mut b, &p1, &m);
    let mut seen = 0;
    for i in 0..BLOCK_SIZE {
        if a[i].abs() < 1e-3 {
            continue;
        }
        let want = (0.2 + 0.8 * (i + 1) as f32 / BLOCK_SIZE as f32) / 0.2;
        assert!(
            (b[i] / a[i] - want).abs() < 1e-3,
            "sample {i}: {} vs {want}",
            b[i] / a[i]
        );
        seen += 1;
    }
    assert!(seen > BLOCK_SIZE / 2, "the block sounds");
}

/// ALGO with BASE up rings out through its carriers after key up, as with
/// no route; under PRESS alone it ends at key up.
#[test]
fn base_holds_algo_through_its_tail() {
    let tail = |base: f32| {
        let (mut p, m) = sound(EngineType::Algo, base, &[(ModSource::Press, -64)]);
        p.envelopes[0].release = 0.5;
        let mut r = Rig::new(SR);
        r.note_on(MidiNote::new(48).unwrap(), Velocity::new(100).unwrap(), &p);
        let mut b = [0.0f32; BLOCK_SIZE];
        for _ in 0..20 {
            r.render(&mut b, &p, &m);
        }
        r.note_off();
        for _ in 0..10 {
            r.render(&mut b, &p, &m);
        }
        r.is_active()
    };
    assert!(tail(1.0), "BASE holds the carriers' release");
    assert!(!tail(0.0), "PRESS alone ends at key up");
}

/// VA never ends on its own: with BASE up its key holds it.
#[test]
fn base_holds_va_while_the_key_is_down() {
    let (p, m) = sound(EngineType::Va, 1.0, &[(ModSource::Press, -64)]);
    let mut r = Rig::new(SR);
    r.note_on(MidiNote::new(48).unwrap(), Velocity::new(100).unwrap(), &p);
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..20 {
        r.render(&mut b, &p, &m);
    }
    assert!(r.is_active());
    r.note_off();
    for _ in 0..10 {
        r.render(&mut b, &p, &m);
    }
    assert!(!r.is_active(), "key up ends it, through the fade");
}

/// Every factory Sound stores BASE 0: each plays as before.
#[test]
fn every_factory_sound_stores_base_zero() {
    for i in 0..chimera_core::factory::FACTORY_LEN {
        let s = chimera_core::factory::factory_sound(i).unwrap();
        assert_eq!(s.params.out.vca, 0.0, "factory {i}");
    }
    for e in EngineType::ALL {
        assert_eq!(Sound::init(e).params.out.vca, 0.0, "{e:?} INIT");
    }
}

/// AMP's E is BASE, the VCA destination's stored value: dimmed with its
/// note until a route into VCA exists.
#[test]
fn amp_e_is_base_dimmed_until_routed() {
    assert_eq!(AMP.params[4].label(), "BASE");
    assert_eq!(AMP.params[4].spec().map(|s| s.id), Some(OutParams::VCA));
    let mut s = Sound::init(EngineType::Algo);
    assert!(view::dimmed(BASE, &s));
    assert_eq!(notes::cell(BASE, &s), Some(notes::BASE_UNROUTED));
    s.mod_state
        .set_fixed(ModSource::Press.index(), FixedCol::Vca, -64);
    assert!(!view::dimmed(BASE, &s));
    assert_eq!(notes::cell(BASE, &s), None);
}

/// BASE glides as a knob does (ADR 0107): a Part's turn never steps it.
#[test]
fn base_is_a_knob() {
    use chimera_core::block::GlideMs;
    let spec = BASE.spec().unwrap();
    assert_eq!(spec.motion().knob(), Some(GlideMs::KNOB));
    assert!(spec.modulatable);
    assert_eq!(spec.ident, "VCA", "the destination's frozen identity");
}

/// The matrix's VCA column (ADR 0292): on a cell with no route, its words
/// say the routes are the amp, or with an ENV among them that they start
/// at BASE; a route's own cell keeps DELETE; another column its own hint.
#[test]
fn the_vca_column_says_what_its_routes_do() {
    use chimera_core::ui::mod_grid::{DELETE, MatrixState};
    let mut m = MatrixState::new();
    m.sel_col = FixedCol::Vca.index();
    m.sel_row = 0;
    let row_src = m.sel_source();
    assert_eq!(notes::matrix(&m), Some(&notes::VCA_REPLACES[..]));
    m.fixed[ModSource::Press.index()][FixedCol::Vca.index()] = -64;
    m.fixed_present[FixedCol::Vca.index()] |= 1 << ModSource::Press.index();
    assert_eq!(
        notes::matrix(&m),
        Some(&notes::VCA_REPLACES[..]),
        "PRESS is no ENV"
    );
    m.fixed_present[FixedCol::Vca.index()] |= 1 << ModSource::Env2.index();
    if row_src != ModSource::Env2.index() {
        assert_eq!(notes::matrix(&m), Some(&notes::VCA_BASE[..]));
    }
    m.fixed_present[FixedCol::Vca.index()] |= 1 << row_src;
    assert_eq!(notes::matrix(&m), None, "the route's own cell");
    assert_eq!(view::matrix_hint(&m), DELETE);
    m.sel_col = FixedCol::Freq.index();
    assert_eq!(notes::matrix(&m), None);
}

/// Each new word fits the band: a hint 216 px, a note the content width.
#[test]
fn the_words_fit() {
    use chimera_core::ui::{hint, theme};
    for h in [&notes::VCA_REPLACES[..], &notes::VCA_BASE[..]] {
        assert!(hint::width(h) <= 216, "{h:?}: {}", hint::width(h));
    }
    let w = chimera_core::ui::draw::text_width(&theme::FONT_LABEL, notes::BASE_UNROUTED, 1);
    assert!(w <= theme::CONTENT_W, "{w}");
}
