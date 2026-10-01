use chimera_core::addr::Op;
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::animation::AnimatedValue;
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::nav::{Location, NavCtx, PageAt};
use chimera_core::ui::page::{PageId, PageKey, ValFmt};
use chimera_core::ui::settings::rows;

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
    PageKey::Part {
        def: def.id,
        op: Op::A,
    }
}

fn cx() -> NavCtx {
    NavCtx {
        engines: [EngineType::Algo; 6],
        dyn_rows: 0,
    }
}

/// The page key at `l`.
fn key(l: Location, op: Op) -> PageKey {
    let (c, at) = l.page(&cx()).unwrap();
    let def = c.def_at(at).unwrap();
    PageKey::from_location(l, def, op)
}

fn pages(node: u8, sub: u8) -> Location {
    Location::pages(PartId::ALL[0], PageAt::of(node, sub))
}

fn leaf(labels: &[&str]) -> Location {
    let mut path = Vec::new();
    for l in labels {
        path.push(rows(&path).iter().position(|r| r.label == *l).unwrap() as u8);
    }
    Location::settings_at(&path, 0)
}

#[test]
fn test_ui_starts_at_part_1_home() {
    let ui = chimera_core::ui::UiState::new();
    assert_eq!(ui.location(), Location::home(&cx()));
    assert_eq!(key(ui.location(), Op::A), part(&reg::ALGO_ALG));
}

#[test]
fn test_page_from_location_part_chain() {
    for (node, def) in [
        (0, &reg::ALGO_ALG),
        (1, &reg::ALGO_WAVE),
        (2, &reg::DRIVE),
        (3, &reg::FILTER),
        (4, &reg::FOLDER),
        (5, &reg::MOD_MATRIX),
    ] {
        assert_eq!(key(pages(node, 0), Op::A), part(def), "node {node}");
    }
    for (sub, def) in [
        (1, &reg::ENVELOPE),
        (2, &reg::ENV_2),
        (3, &reg::ENV_3),
        (4, &reg::ENV_SPEED),
        (5, &reg::LFO),
        (6, &reg::LFO_2),
        (7, &reg::LFO_3),
    ] {
        assert_eq!(key(pages(5, sub), Op::A), part(def), "sub-page {sub}");
    }
    // The operator selection is part of a Part page's identity.
    assert_ne!(key(pages(5, 7), Op::B), key(pages(5, 7), Op::A));
}

#[test]
#[cfg(debug_assertions)]
fn test_page_from_location_demo() {
    let demo = leaf(&["SYSTEM", "DEMO"]);
    assert_eq!(
        PageKey::from_location(demo, &reg::DEMO_SHAPES, Op::A),
        PageKey::Legacy(PageId::Demo(reg::DEMO_SHAPES.id))
    );
    assert_eq!(
        key(demo, Op::A),
        PageKey::Legacy(PageId::Demo(reg::DEMO_WAVES.id))
    );
}

/// Spec §5: a SETTINGS leaf gets its own page (it used to alias an engine
/// page). TUNING is not slot-bound yet; THEME is.
#[test]
fn test_settings_leaves_have_their_own_page() {
    assert_eq!(
        key(leaf(&["AUDIO ROUTING", "TUNING"]), Op::A),
        PageKey::Legacy(PageId::System(reg::SYS_TUNING.id))
    );
    assert!(matches!(
        key(leaf(&["PERSONALIZE", "THEME"]), Op::A),
        PageKey::Part { .. }
    ));
}

// -- BlockDef registry: format coverage --

#[test]
fn test_drive_block_formats_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    let def = &block_registry::DRIVE;
    assert_eq!(def.params[0].format(), ValFmt::Uni); // DRIVE
    assert_eq!(def.params[1].format(), ValFmt::Bi); // TONE
    assert_eq!(def.params[2].format(), ValFmt::Bi); // MIX
}

#[test]
fn test_filter_mode_is_named_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    use chimera_core::ui::view::{SlotCtx, view};
    let ctx = SlotCtx::read(
        &chimera_core::params::ParamSnapshot::default(),
        chimera_core::addr::Op::A,
    );
    assert!(matches!(
        view(&block_registry::FILTER, 3, &ctx).fmt(),
        ValFmt::Names(_)
    ));
}

#[test]
fn test_fmt_one_based() {
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.0, ValFmt::OneBased(15));
    assert_eq!(buf.as_str(), "1");
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 1.0, ValFmt::OneBased(15));
    assert_eq!(buf.as_str(), "16");
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 2.0, ValFmt::OneBased(255)); // clamps, no overflow
    assert_eq!(buf.as_str(), "256");
}

#[test]
fn test_fmt_names() {
    const NAMES: ValFmt = ValFmt::Names(&["P1", "P2", "P3"]);
    for (v, want) in [
        (0.0, "P1"),
        (0.5, "P2"),
        (0.74, "P2"),
        (1.0, "P3"),
        (1.5, "P3"),
    ] {
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, v, NAMES);
        assert_eq!(buf.as_str(), want, "{v}");
    }
    assert_eq!(NAMES.max_int(), 2);
    assert_eq!(NAMES.snap_points(), ValFmt::Int(2).snap_points());
    let mut buf = FmtBuf::new();
    fmt_val(&mut buf, 0.0, ValFmt::Names(&[]));
    assert_eq!(buf.as_str(), "", "no names: nothing shown, no panic");
}

// -- Pan shows as L / C / R (UI refresh spec § Principles) --

#[test]
fn test_fmt_pan_left_centre_right() {
    for (v, want) in [
        (0.0, "L64"),
        (0.25, "L32"),
        (0.5, "C"),
        (0.75, "R31"),
        (1.0, "R63"),
    ] {
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, v, ValFmt::Pan);
        assert_eq!(buf.as_str(), want, "{v}");
    }
}

#[test]
fn test_pan_snaps_and_bipolar_like_bi() {
    assert!(ValFmt::Pan.is_bipolar());
    assert_eq!(ValFmt::Pan.snap_points(), ValFmt::Bi.snap_points());
    assert!(!ValFmt::Pan.is_discrete());
}

#[test]
fn test_discrete_formats_are_choices() {
    assert!(ValFmt::Int(7).is_discrete());
    assert!(ValFmt::OneBased(15).is_discrete());
    assert!(ValFmt::Names(&["A"]).is_discrete());
    assert!(!ValFmt::Uni.is_discrete() && !ValFmt::Bi.is_discrete());
}

#[test]
fn test_part_and_out_pan_use_the_pan_format() {
    use chimera_core::params::OUT_SPECS;
    use chimera_core::part::PART_SPECS;
    assert_eq!(PART_SPECS[4].fmt, ValFmt::Pan);
    assert_eq!(OUT_SPECS[1].fmt, ValFmt::Pan);
}

#[test]
fn test_fmt_signed() {
    let mut buf = FmtBuf::new();
    for (v, want) in [(0.0, "-3"), (0.5, "0"), (1.0, "+3"), (4.0 / 6.0, "+1")] {
        buf.clear();
        fmt_val(&mut buf, v, ValFmt::Signed(3));
        assert_eq!(buf.as_str(), want);
    }
    assert!(ValFmt::Signed(3).is_discrete() && ValFmt::Signed(3).is_bipolar());
    assert_eq!(ValFmt::Signed(24).max_int(), 48);
}
