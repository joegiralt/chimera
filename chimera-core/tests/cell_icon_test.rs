use chimera_core::ui::page::{CellIcon, PageId, PageLayout, ValFmt};

// ── Icon frame quantization ─────────────────────────────────────────

#[test]
fn test_frame_quantization_16_steps() {
    // 128 MIDI values / 8 = 16 frames (0-15)
    // Normalized 0.0 -> frame 0, 1.0 -> frame 15
    let frame_at = |val: f32| -> u8 { (val * 15.0) as u8 };

    assert_eq!(frame_at(0.0), 0);
    assert_eq!(frame_at(1.0), 15);
    assert_eq!(frame_at(0.5), 7);
    // Each frame spans ~8 MIDI values
    assert_eq!(frame_at(8.0 / 127.0), 0); // MIDI 8 -> still frame 0
    assert_eq!(frame_at(9.0 / 127.0), 1); // MIDI 9 -> frame 1
}

// ── Every page has valid icons ──────────────────────────────────────

#[test]
fn test_cell_grid_pages_have_icons() {
    let cell_pages = [
        PageId::Drive,
        PageId::Folder,
        PageId::EngineVa,
        PageId::EngineModal1,
        PageId::Mixer,
        PageId::Efx,
        PageId::GlobalEfx,
        PageId::DemoWaves,
        PageId::DemoShapes,
        PageId::DemoMotion,
    ];

    for page in &cell_pages {
        assert_eq!(page.layout(), PageLayout::CellGrid, "{:?} should be CellGrid", page);
        let icons = page.cell_icons();
        let labels = page.encoder_labels();

        // Every non-"--" label should have a non-None icon
        for (i, label) in labels.iter().enumerate() {
            if *label != "--" {
                assert_ne!(
                    icons[i],
                    CellIcon::None,
                    "{:?} encoder {} ({}) has no icon",
                    page, i, label
                );
            }
        }
    }
}

#[test]
fn test_big_viz_pages_dont_need_icons() {
    let viz_pages = [
        PageId::Filter,
        PageId::EnvAmp,
        PageId::EnvFilter,
        PageId::EnvAux,
        PageId::Vca,
        PageId::EngineFmA,
        PageId::Routing,
        PageId::Compressor,
    ];

    for page in &viz_pages {
        assert_eq!(page.layout(), PageLayout::BigViz, "{:?} should be BigViz", page);
    }
}

// ── Demo storybook covers all icon types ────────────────────────────

#[test]
fn test_demo_storybook_covers_all_icons() {
    let all_demo_icons: Vec<CellIcon> = [
        PageId::DemoWaves,
        PageId::DemoShapes,
        PageId::DemoMotion,
    ]
    .iter()
    .flat_map(|p| p.cell_icons().to_vec())
    .collect();

    // Every icon type (except None) should appear at least once in the demo
    let icon_types = [
        CellIcon::WaveClip,
        CellIcon::WaveShape,
        CellIcon::PulseWidth,
        CellIcon::WaveFold,
        CellIcon::ToneTilt,
        CellIcon::Symmetry,
        CellIcon::Arc,
        CellIcon::LevelBar,
        CellIcon::PanDot,
        CellIcon::DryWet,
        CellIcon::Cube,
        CellIcon::Stack,
        CellIcon::Ripple,
        CellIcon::Burst,
        CellIcon::Orbit,
        CellIcon::Scatter,
        CellIcon::Bounce,
        CellIcon::Breathe,
    ];

    for icon in &icon_types {
        assert!(
            all_demo_icons.contains(icon),
            "{:?} not present in demo storybook",
            icon
        );
    }
}

// ── ValFmt consistency ──────────────────────────────────────────────

#[test]
fn test_all_pages_have_6_labels() {
    let all_pages = [
        PageId::EngineFmA, PageId::EngineFmB, PageId::EngineFmC,
        PageId::EngineModal1, PageId::EngineVa,
        PageId::Drive, PageId::Filter, PageId::Folder,
        PageId::Vca, PageId::Efx, PageId::Mixer,
        PageId::Routing, PageId::Compressor, PageId::GlobalEfx,
        PageId::EnvAmp, PageId::EnvFilter, PageId::EnvAux,
        PageId::DemoWaves, PageId::DemoShapes, PageId::DemoMotion,
    ];

    for page in &all_pages {
        let labels = page.encoder_labels();
        assert_eq!(labels.len(), 6, "{:?} should have 6 labels", page);

        let fmts = page.val_formats();
        assert_eq!(fmts.len(), 6, "{:?} should have 6 val formats", page);
    }
}

#[test]
fn test_bipolar_params_use_bipolar_snaps() {
    // Every bipolar encoder should have 5 snap points centered on 0
    let snaps = ValFmt::Bi.snap_points();
    assert_eq!(snaps.len(), 5);

    // Center snap should be at MIDI 64 (normalized ~0.504)
    let center = snaps[2];
    assert!((center - 64.0 / 127.0).abs() < 0.01, "bipolar center should be at MIDI 64");
}

#[test]
fn test_unipolar_snaps_in_order() {
    let snaps = ValFmt::Uni.snap_points();
    for i in 1..snaps.len() {
        assert!(snaps[i] > snaps[i - 1], "snap points must be ascending");
    }
}

#[test]
fn test_bipolar_snaps_in_order() {
    let snaps = ValFmt::Bi.snap_points();
    for i in 1..snaps.len() {
        assert!(snaps[i] > snaps[i - 1], "snap points must be ascending");
    }
}

#[test]
fn test_bipolar_snaps_are_symmetric() {
    let snaps = ValFmt::Bi.snap_points();
    let center = snaps[2];
    // Distance from center to -44 should equal distance from center to +43
    let low_dist = center - snaps[1];
    let high_dist = snaps[3] - center;
    assert!(
        (low_dist - high_dist).abs() < 0.02,
        "bipolar snaps should be roughly symmetric: low={} high={}",
        low_dist, high_dist
    );
}

// ── Chain -> Page -> Icon consistency ───────────────────────────────

#[test]
fn test_drive_page_has_correct_setup() {
    let page = PageId::Drive;
    assert_eq!(page.layout(), PageLayout::CellGrid);

    let labels = page.encoder_labels();
    assert_eq!(labels[0], "DRIVE");
    assert_eq!(labels[1], "TONE");
    assert_eq!(labels[2], "MIX");
    assert_eq!(labels[3], "--");

    let fmts = page.val_formats();
    assert_eq!(fmts[0], ValFmt::Uni);  // DRIVE is unipolar
    assert_eq!(fmts[1], ValFmt::Bi);   // TONE is bipolar
    assert_eq!(fmts[2], ValFmt::Bi);   // MIX is bipolar

    let icons = page.cell_icons();
    assert_eq!(icons[0], CellIcon::WaveClip);
    assert_eq!(icons[1], CellIcon::ToneTilt);
    assert_eq!(icons[2], CellIcon::DryWet);
}

#[test]
fn test_folder_page_has_correct_setup() {
    let page = PageId::Folder;
    assert_eq!(page.layout(), PageLayout::CellGrid);

    let labels = page.encoder_labels();
    assert_eq!(labels[0], "FOLD");
    assert_eq!(labels[1], "SYM");
    assert_eq!(labels[2], "MIX");

    let fmts = page.val_formats();
    assert_eq!(fmts[0], ValFmt::Uni);  // FOLD
    assert_eq!(fmts[1], ValFmt::Bi);   // SYM bipolar
    assert_eq!(fmts[2], ValFmt::Bi);   // MIX bipolar

    let icons = page.cell_icons();
    assert_eq!(icons[0], CellIcon::WaveFold);
    assert_eq!(icons[1], CellIcon::Symmetry);
    assert_eq!(icons[2], CellIcon::DryWet);
}

// ── fold_wave function ──────────────────────────────────────────────

#[test]
fn test_fold_wave_identity_in_range() {
    use chimera_core::ui::cell::fold_wave;
    // Values in -1..1 should pass through unchanged
    assert!((fold_wave(0.0) - 0.0).abs() < 0.01);
    assert!((fold_wave(0.5) - 0.5).abs() < 0.01);
    assert!((fold_wave(-0.5) - (-0.5)).abs() < 0.01);
}

#[test]
fn test_fold_wave_reflects() {
    use chimera_core::ui::cell::fold_wave;
    // Values beyond 1 should reflect back
    let v = fold_wave(1.5);
    assert!((v - 0.5).abs() < 0.01, "1.5 should fold to 0.5, got {}", v);

    let v = fold_wave(2.0);
    assert!((v - 0.0).abs() < 0.01, "2.0 should fold to 0.0, got {}", v);

    let v = fold_wave(-1.5);
    assert!((v - (-0.5)).abs() < 0.01, "-1.5 should fold to -0.5, got {}", v);
}

#[test]
fn test_fold_wave_stays_bounded() {
    use chimera_core::ui::cell::fold_wave;
    // Any input should produce output in -1..1
    for i in -100..=100 {
        let input = i as f32 * 0.1;
        let output = fold_wave(input);
        assert!(
            output >= -1.0 && output <= 1.0,
            "fold_wave({}) = {} is out of range",
            input, output
        );
    }
}
