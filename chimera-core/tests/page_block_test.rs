//! Page encoders, snap and display after moving onto `Block` specs.
//! Parity tests pin today's step sizes (plan § Encoder step audit).

use chimera_core::params::ParamSnapshot;
use chimera_core::ui::page::{PageId, ValFmt};

#[test]
fn pizza_encoder_steps_like_before() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.apply_encoder(0, 3, &mut p);
    assert_eq!(p.pizza.shape, 0.5 + 3.0 * (1.0 / 128.0));
    PageId::Pizza.apply_encoder(2, 127, &mut p);
    assert_eq!(p.pizza.level, 1.0);
}

/// Spec § Intended behavior changes: shift-snap now works on Pizza.
#[test]
fn pizza_snap_now_works() {
    let mut p = ParamSnapshot::default();
    PageId::Pizza.snap_encoder(1, 1, ValFmt::Uni, &mut p);
    assert_eq!(p.pizza.crush, 100.0 / 127.0);
}

#[test]
fn pizza_read_values() {
    let p = ParamSnapshot::default();
    assert_eq!(PageId::Pizza.read_values(&p), [0.5, 0.0, 0.8, 0.0, 0.0, 0.0]);
}
