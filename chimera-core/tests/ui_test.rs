use chimera_core::ui::animation::AnimatedValue;
use chimera_core::ui::chain::{ChainId, ChainNav};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::addr::Op;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::{PageId, PageKey, ValFmt};

// -- ValFmt --

#[test]
fn test_uni_snap_points() {
    let snaps = ValFmt::Uni.snap_points();
    assert_eq!(snaps.len(), 3);
    assert!((snaps[0] - 0.0).abs() < 0.001);
    assert!((snaps[1] - 100.0 / 127.0).abs() < 0.001);
    assert!((snaps[2] - 1.0).abs() < 0.001);
}

#[test]
fn test_bi_snap_points() {
    let snaps = ValFmt::Bi.snap_points();
    assert_eq!(snaps.len(), 5);
    assert!((snaps[0] - 0.0).abs() < 0.001);
    assert!((snaps[2] - 64.0 / 127.0).abs() < 0.001);
    assert!((snaps[4] - 1.0).abs() < 0.001);
}

#[test]
fn test_valfmt_is_bipolar() {
    assert!(!ValFmt::Uni.is_bipolar());
    assert!(ValFmt::Bi.is_bipolar());
    assert!(!ValFmt::Int(7).is_bipolar());
}

// -- fmt_val --

#[test]
fn test_fmt_unipolar_zero() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.0, ValFmt::Uni);
    assert_eq!(buf.as_str(), "0");
}

#[test]
fn test_fmt_unipolar_max() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 1.0, ValFmt::Uni);
    assert_eq!(buf.as_str(), "127");
}

#[test]
fn test_fmt_unipolar_mid_rounds() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.5, ValFmt::Uni);
    assert_eq!(buf.as_str(), "64");
}

#[test]
fn test_fmt_bipolar_center() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.5, ValFmt::Bi);
    assert_eq!(buf.as_str(), "0");
}

#[test]
fn test_fmt_bipolar_min() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.0, ValFmt::Bi);
    assert_eq!(buf.as_str(), "-64");
}

#[test]
fn test_fmt_bipolar_max() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 1.0, ValFmt::Bi);
    assert_eq!(buf.as_str(), "+63");
}

#[test]
fn test_fmt_bipolar_positive_has_plus() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.75, ValFmt::Bi);
    let s = buf.as_str();
    assert!(
        s.starts_with('+'),
        "positive bipolar should have + prefix: {}",
        s
    );
}

#[test]
fn test_fmt_int_zero() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.0, ValFmt::Int(7));
    assert_eq!(buf.as_str(), "0");
}

#[test]
fn test_fmt_int_max() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 1.0, ValFmt::Int(7));
    assert_eq!(buf.as_str(), "7");
}

#[test]
fn test_fmt_int_clamps() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 1.5, ValFmt::Int(7));
    assert_eq!(buf.as_str(), "7");
}

// -- Animation --

#[test]
fn test_animated_value_snap() {
    let mut av = AnimatedValue::new(0.0);
    av.snap(1.0);
    assert!((av.current() - 1.0).abs() < 0.001);
    assert!((av.target() - 1.0).abs() < 0.001);
    assert!(av.is_settled());
}

#[test]
fn test_animated_value_converges() {
    let mut av = AnimatedValue::new(0.0);
    av.set_target(1.0);
    assert!(!av.is_settled());

    // After enough updates, should converge
    for _ in 0..200 {
        av.update();
    }
    assert!(av.is_settled());
    assert!((av.current() - 1.0).abs() < 0.01);
}

#[test]
fn test_animated_value_moves_toward_target() {
    let mut av = AnimatedValue::new(0.0);
    av.set_target(1.0);
    av.update();
    assert!(av.current() > 0.0, "should move toward target");
    assert!(av.current() < 1.0, "should not overshoot");
}

// -- Chain navigation --

fn part(def: &BlockDef) -> PageKey {
    PageKey::Part { def: def.id, op: Op::A }
}

#[test]
fn test_chain_nav_starts_at_part0_engine() {
    let nav = ChainNav::new();
    assert_eq!(nav.chain_id, ChainId::Part(0));
    assert_eq!(nav.node, 0);
    assert_eq!(nav.sub_page, 0);
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::PIZZA));
}

#[test]
fn test_page_from_nav_part_chain() {
    let mut nav = ChainNav::new();
    for (node, def) in [(0, &reg::PIZZA), (1, &reg::DRIVE), (2, &reg::FILTER), (3, &reg::FOLDER), (4, &reg::MOD_MATRIX)] {
        nav.node = node;
        assert_eq!(PageKey::from_nav(&nav, Op::A), part(def), "node {node}");
    }
    nav.sub_page = 1;
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::ENVELOPE)); // Envelope at sub_page 1
    nav.sub_page = 2;
    assert_eq!(PageKey::from_nav(&nav, Op::A), part(&reg::LFO)); // LFO at sub_page 2
    // The operator selection is part of a Part page's identity.
    assert_ne!(PageKey::from_nav(&nav, Op::B), PageKey::from_nav(&nav, Op::A));
}

#[test]
fn test_page_from_nav_demo_chain() {
    let mut nav = ChainNav::new();
    nav.chain_id = ChainId::Demo;
    nav.node = 0;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoWaves));
    nav.node = 1;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoShapes));
    nav.node = 2;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::DemoMotion));
}

/// Spec §5: System gets its own page (it used to alias the Pizza page).
#[test]
fn test_system_chain_has_its_own_page() {
    let mut nav = ChainNav::new();
    nav.chain_id = ChainId::System;
    assert_eq!(PageKey::from_nav(&nav, Op::A), PageKey::Legacy(PageId::System));
}

// -- BlockDef registry: format coverage --

#[test]
fn test_drive_block_formats_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    let def = &block_registry::DRIVE;
    assert_eq!(def.params[0].format(), ValFmt::Uni); // DRIVE
    assert_eq!(def.params[1].format(), ValFmt::Bi);  // TONE
    assert_eq!(def.params[2].format(), ValFmt::Bi);  // MIX
}

#[test]
fn test_filter_env_amount_bipolar_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    let def = &block_registry::FILTER;
    assert_eq!(def.params[4].format(), ValFmt::Bi); // ENV amount
}

#[test]
fn test_mixer_pan_bipolar_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    let def = &block_registry::MIXER;
    assert_eq!(def.params[1].format(), ValFmt::Bi); // PAN
}
