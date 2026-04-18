use chimera_core::ui::animation::AnimatedValue;
use chimera_core::ui::chain::ChainNav;
use chimera_core::ui::fmt::{fmt_val, FmtBuf};
use chimera_core::ui::page::{PageId, PageLayout, ValFmt};

// ── ValFmt ──────────────────────────────────────────────────────────

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

// ── fmt_val ─────────────────────────────────────────────────────────

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
    assert!(s.starts_with('+'), "positive bipolar should have + prefix: {}", s);
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

// ── Animation ───────────────────────────────────────────────────────

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

// ── Chain navigation ────────────────────────────────────────────────

#[test]
fn test_chain_nav_starts_at_voice_engine() {
    let nav = ChainNav::new();
    assert_eq!(nav.chain, 0);
    assert_eq!(nav.node, 0);
    assert_eq!(nav.sub_page, 0);
    assert_eq!(PageId::from_nav(&nav), PageId::EngineFmA);
}

#[test]
fn test_page_from_nav_voice_chain() {
    let mut nav = ChainNav::new();
    nav.node = 0;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineFmA);
    nav.sub_page = 1;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineFmB);
    nav.sub_page = 2;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineFmC);
    nav.sub_page = 3;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineModal1);
    nav.sub_page = 4;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineModal2);
    nav.sub_page = 5;
    assert_eq!(PageId::from_nav(&nav), PageId::EngineVa);
    nav.sub_page = 0;
    nav.node = 1;
    assert_eq!(PageId::from_nav(&nav), PageId::Drive);
    nav.node = 2;
    assert_eq!(PageId::from_nav(&nav), PageId::Filter);
    nav.node = 3;
    assert_eq!(PageId::from_nav(&nav), PageId::Folder);
    nav.node = 4;
    assert_eq!(PageId::from_nav(&nav), PageId::Vca);
    nav.node = 5;
    assert_eq!(PageId::from_nav(&nav), PageId::Efx);
}

#[test]
fn test_page_from_nav_envelope_chain() {
    let mut nav = ChainNav::new();
    nav.chain = 2;
    nav.node = 0;
    assert_eq!(PageId::from_nav(&nav), PageId::EnvAmp);
    nav.node = 1;
    assert_eq!(PageId::from_nav(&nav), PageId::EnvFilter);
    nav.node = 2;
    assert_eq!(PageId::from_nav(&nav), PageId::EnvAux);
}

// ── Page layout ─────────────────────────────────────────────────────

#[test]
fn test_big_viz_pages() {
    assert_eq!(PageId::Filter.layout(), PageLayout::BigViz);
    assert_eq!(PageId::EnvAmp.layout(), PageLayout::BigViz);
    assert_eq!(PageId::EngineFmA.layout(), PageLayout::BigViz);
    assert_eq!(PageId::Compressor.layout(), PageLayout::BigViz);
}

#[test]
fn test_cell_grid_pages() {
    assert_eq!(PageId::Drive.layout(), PageLayout::CellGrid);
    assert_eq!(PageId::Folder.layout(), PageLayout::CellGrid);
    assert_eq!(PageId::Mixer.layout(), PageLayout::CellGrid);
    assert_eq!(PageId::EngineVa.layout(), PageLayout::CellGrid);
}

// ── Page val formats ────────────────────────────────────────────────

#[test]
fn test_drive_tone_and_mix_are_bipolar() {
    let fmts = PageId::Drive.val_formats();
    assert_eq!(fmts[0], ValFmt::Uni); // DRIVE
    assert_eq!(fmts[1], ValFmt::Bi);  // TONE
    assert_eq!(fmts[2], ValFmt::Bi);  // MIX
}

#[test]
fn test_filter_env_amount_is_bipolar() {
    let fmts = PageId::Filter.val_formats();
    assert_eq!(fmts[4], ValFmt::Bi); // ENV amount
}

#[test]
fn test_mixer_pan_is_bipolar() {
    let fmts = PageId::Mixer.val_formats();
    assert_eq!(fmts[1], ValFmt::Bi); // PAN
}

#[test]
fn test_folder_symmetry_is_bipolar() {
    let fmts = PageId::Folder.val_formats();
    assert_eq!(fmts[1], ValFmt::Bi); // SYM
}

// ── Encoder labels ──────────────────────────────────────────────────

#[test]
fn test_unused_encoder_slots_are_dashes() {
    let labels = PageId::Drive.encoder_labels();
    assert_eq!(labels[3], "--");
    assert_eq!(labels[4], "--");
    assert_eq!(labels[5], "--");
}

#[test]
fn test_filter_has_six_labels() {
    let labels = PageId::Filter.encoder_labels();
    for label in &labels {
        assert_ne!(*label, "--");
    }
}
