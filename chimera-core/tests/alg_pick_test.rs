//! #565: picking ALG B shows B's diagram, not A's blended by MORPH.

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::theme;
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
