// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! #565: picking ALG B shows B's diagram, not A's blended by MORPH.

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::{UiState, theme};
use chimera_hal::EncoderId;
use screen::*;

fn band(fb: &Fb) -> Vec<u16> {
    (theme::VIZ_BAND_TOP..theme::VIZ_BAND_BOTTOM)
        .flat_map(|y| (theme::VIZ_LEFT..theme::VIZ_RIGHT).map(move |x| (x, y)))
        .map(|(x, y)| fb.px[y as usize * W + x as usize])
        .collect()
}

/// ALG A = A1 + `a`, then ALG B = A1 + `b`, focus on ALG B, MORPH 0.
fn algo_page(a: i8, b: i8) -> Fb {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    if a != 0 {
        feed(&mut ui, Input::turn(EncoderId::A, a));
    }
    feed(&mut ui, Input::turn(EncoderId::B, b));
    settle(&mut ui);
    render_ui(&ui)
}

#[test]
fn the_diagram_follows_the_alg_b_cell() {
    let picked = algo_page(0, 16);
    picked.dump("alg_b_picked");
    // B focused at MORPH 0: the band is A17 whatever ALG A holds.
    assert!(band(&picked) == band(&algo_page(16, 16)));
    assert_ne!(band(&picked), band(&algo_page(0, 1)));
}

/// ALG B = A1 + `b` first, then ALG A = A1 + `a` (focus on ALG A), MORPH `m`.
fn pick_a(a: i8, b: i8, m: i8) -> Fb {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    feed(&mut ui, Input::turn(EncoderId::B, b));
    if m != 0 {
        feed(&mut ui, Input::turn(EncoderId::C, m));
    }
    feed(&mut ui, Input::turn(EncoderId::A, a));
    settle(&mut ui);
    render_ui(&ui)
}

#[test]
fn the_diagram_follows_the_alg_a_cell_at_any_morph() {
    for m in [0, 100] {
        let picked = pick_a(16, 1, m);
        // A17 shows whatever ALG B and MORPH hold.
        assert!(band(&picked) == band(&pick_a(16, 16, m)), "morph {m}");
        assert!(band(&picked) != band(&pick_a(2, 1, m)), "morph {m}");
    }
}

/// Turn `e` by `d` on a fresh UI after `steps`; each step is rendered with
/// `render_dirty` onto one framebuffer, then the band must equal a full
/// render's.
fn dirty_matches_full(ui: &mut UiState, steps: &[(EncoderId, i8)]) -> Vec<Vec<u16>> {
    let mut fb = Fb::new();
    let mut bands = vec![];
    let draw = |ui: &mut UiState, fb: &mut Fb| {
        ui.render_dirty_with_audio(
            fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            Some(&audio_fixture()),
            &scope_fixture(),
        );
    };
    draw(ui, &mut fb);
    for &(e, d) in steps {
        feed(ui, Input::turn(e, d));
        settle(ui);
        draw(ui, &mut fb);
        let full = render_ui(ui);
        assert!(band(&fb) == band(&full), "{e:?} {d}: dirty band is stale");
        bands.push(band(&full));
    }
    bands
}

fn init_ui() -> UiState {
    let mut ui = stack_ui();
    init_part(&mut ui, PartId::ALL[0]);
    ui
}

#[test]
fn dirty_redraw_follows_the_focus_between_morph_and_the_algs() {
    use EncoderId::{A, B, C};
    let mut ui = init_ui();
    // B to its last algorithm; MORPH focused; then focus moves with no
    // value changing (a turn into an end).
    let b = dirty_matches_full(
        &mut ui,
        &[(B, 100), (C, -1), (B, 1), (A, -1), (B, 1), (C, -1)],
    );
    // MORPH 0 blend = A1; ALG B focused = B alone; ALG A focused = A1 again;
    // the blend returns off ALG B.
    assert!(b[1] != b[2] && b[2] != b[3] && b[3] != b[4] && b[4] == b[2]);
    assert!(b[5] == b[1]);
}

#[test]
fn dirty_redraw_follows_a_held_refused_algorithm() {
    use EncoderId::B;
    use chimera_core::dsp::algo::algorithms::AlgoId;
    use chimera_core::dsp::algo::kinds::OpKind;
    use chimera_core::params::EngineType;
    use chimera_core::preset::Sound;
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..3 {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let mut ui = init_ui();
    let was = chimera_core::ui::nav::PartChain::of(&ui.project().part(PartId::ALL[0]).sound);
    **ui.project_mut().edit_part(PartId::ALL[0]).sound = s;
    ui.replaced_for_test(was);
    // B focused at A1; A2 refused and held, nothing written; A3 likewise.
    let b = dirty_matches_full(&mut ui, &[(B, -1), (B, 1), (B, 1)]);
    assert!(ui.held_choice().is_some(), "A3 is held");
    assert!(b[0] != b[1] && b[1] != b[2], "the held diagrams differ");
}
