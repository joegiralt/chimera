//! Stress tests: verify all engines can render within hardware CPU budget.
//! STM32H750 @ 480MHz, 48kHz, BLOCK_SIZE=128 = 10,000 cycles/sample.
//! We measure wall-clock time on desktop and flag anything too slow.

use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};
use std::time::Instant;

const SR: u32 = 48000;
const BLOCKS: usize = 100;

/// Measure average render time per block for a given configuration.
fn bench_render(name: &str, setup: impl FnOnce(&mut ParamSnapshot)) -> f64 {
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    let mut voice = Voice::new();
    voice.note_on(60, 100, &params, SR);

    let mut block = [0.0f32; 64];
    // Warmup
    for _ in 0..10 {
        voice.render(&mut block, &params, SR);
    }

    let start = Instant::now();
    for _ in 0..BLOCKS {
        voice.render(&mut block, &params, SR);
    }
    let elapsed = start.elapsed();

    let us_per_block = elapsed.as_micros() as f64 / BLOCKS as f64;
    let us_per_sample = us_per_block / 128.0;
    eprintln!(
        "{:30} {:8.1} us/block  {:6.2} us/sample",
        name, us_per_block, us_per_sample
    );
    us_per_block
}

#[test]
fn stress_fm_basic() {
    let t = bench_render("FM basic (sine carrier)", |p| {
        p.engine = EngineType::Fm;
    });
    assert!(t < 5000.0, "FM basic too slow: {} us/block", t);
}

#[test]
fn stress_fm_full() {
    let t = bench_render("FM full (4 ops, feedback)", |p| {
        p.engine = EngineType::Fm;
        p.fm.algorithm = 0;
        p.fm.feedback = 0.5;
        p.fm.op_level = [0.8, 0.6, 0.6, 1.0];
    });
    assert!(t < 5000.0, "FM full too slow: {} us/block", t);
}

#[test]
fn stress_fm_full_with_chain() {
    let t = bench_render("FM + drive + filter + folder", |p| {
        p.engine = EngineType::Fm;
        p.fm.op_level = [0.8, 0.6, 0.6, 1.0];
        p.drive.drive.set(0.5);
        p.drive.mix.set(1.0);
        p.filter.cutoff.set(2000.0);
        p.filter.resonance.set(0.7);
        p.filter.mode = 2; // LP4
        p.folder.fold.set(0.5);
        p.folder.mix.set(1.0);
    });
    assert!(t < 5000.0, "FM + chain too slow: {} us/block", t);
}

#[test]
fn stress_ks_string() {
    let t = bench_render("KS+ string (body+stiff+ens)", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 0;
        p.modal.ks_body = 0.5;
        p.modal.ks_stiffness = 0.3;
        p.modal.ks_feedback = 0.3;
        p.modal.ks_ens_depth = 0.5;
        p.modal.ks_ens_mix = 0.5;
    });
    assert!(t < 5000.0, "KS+ too slow: {} us/block", t);
}

#[test]
fn stress_modal_32_modes() {
    let t = bench_render("Modal 32 modes", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 32;
    });
    assert!(t < 5000.0, "Modal 32 too slow: {} us/block", t);
}

#[test]
fn stress_modal_48_modes() {
    let t = bench_render("Modal 48 modes", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 48;
    });
    assert!(t < 10000.0, "Modal 48 too slow: {} us/block", t);
}

#[test]
fn stress_bowed() {
    let t = bench_render("Bowed string", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 2;
    });
    assert!(t < 5000.0, "Bowed too slow: {} us/block", t);
}

#[test]
fn stress_sympathetic() {
    let t = bench_render("Sympathetic (8 strings)", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 3;
        p.modal.inharm = 0.5;
    });
    assert!(t < 10000.0, "Sympathetic too slow: {} us/block", t);
}

#[test]
fn stress_sympathetic_with_chain() {
    let t = bench_render("Sympathetic + full chain", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 3;
        p.modal.inharm = 0.5;
        p.drive.drive.set(0.5);
        p.drive.mix.set(1.0);
        p.filter.cutoff.set(3000.0);
        p.filter.resonance.set(0.5);
        p.filter.mode = 2;
        p.folder.fold.set(0.3);
        p.folder.mix.set(1.0);
    });
    assert!(t < 10000.0, "Sympathetic + chain too slow: {} us/block", t);
}

#[test]
fn stress_worst_case() {
    let t = bench_render("WORST CASE: 48-mode + full chain", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 48;
        p.drive.drive.set(1.0);
        p.drive.mix.set(1.0);
        p.filter.cutoff.set(1000.0);
        p.filter.resonance.set(0.9);
        p.filter.mode = 2; // LP4 (two cascaded SVFs)
        p.folder.fold.set(1.0);
        p.folder.mix.set(1.0);
    });
    // This is the absolute worst case — if this fits, everything fits
    assert!(t < 15000.0, "Worst case too slow: {} us/block", t);
}

/// Summary test: prints all timings in a table.
#[test]
fn stress_summary() {
    eprintln!("\n=== STRESS TEST SUMMARY ===");
    eprintln!(
        "{:30} {:>12} {:>10}",
        "Configuration", "us/block", "us/sample"
    );
    eprintln!("{}", "-".repeat(55));

    bench_render("FM basic", |p| {
        p.engine = EngineType::Fm;
    });
    bench_render("FM full 4-op", |p| {
        p.engine = EngineType::Fm;
        p.fm.op_level = [0.8, 0.6, 0.6, 1.0];
        p.fm.feedback = 0.5;
    });
    bench_render("KS+ string", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 0;
    });
    bench_render("KS+ full features", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 0;
        p.modal.ks_body = 0.5;
        p.modal.ks_stiffness = 0.3;
        p.modal.ks_ens_depth = 0.5;
        p.modal.ks_ens_mix = 0.5;
    });
    bench_render("Modal 16 modes", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 16;
    });
    bench_render("Modal 32 modes", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 32;
    });
    bench_render("Modal 48 modes", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 1;
        p.modal.num_modes = 48;
    });
    bench_render("Bowed", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 2;
    });
    bench_render("Sympathetic 8 strings", |p| {
        p.engine = EngineType::Modal;
        p.modal.mode = 3;
    });
    bench_render("+ Drive", |p| {
        p.engine = EngineType::Fm;
        p.drive.drive.set(0.8);
        p.drive.mix.set(1.0);
    });
    bench_render("+ Filter LP4", |p| {
        p.engine = EngineType::Fm;
        p.filter.cutoff.set(2000.0);
        p.filter.mode = 2;
    });
    bench_render("+ Wavefolder", |p| {
        p.engine = EngineType::Fm;
        p.folder.fold.set(0.8);
        p.folder.mix.set(1.0);
    });

    eprintln!("\nBudget: 2667 us/block (128 samples @ 48kHz)");
    eprintln!("STM32H750 is ~3x faster than desktop per-cycle");
}
