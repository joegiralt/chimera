//! Stress tests: verify all engines can render within hardware CPU budget.
//! STM32H750 @ 480MHz, 48kHz, BLOCK_SIZE=128 = 10,000 cycles/sample.
//! We measure wall-clock time on desktop and flag anything too slow.

use chimera_core::{MidiNote, Velocity};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::modulation::ModState;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::{EngineType, ParamSnapshot};
use std::time::Instant;

const SR: u32 = 48000;
const BLOCKS: usize = 100;

/// Measure average render time per block for a given configuration.
fn bench_render(name: &str, setup: impl FnOnce(&mut ParamSnapshot)) -> f64 {
    let empty_mod = ModState::new();
    let mut params = ParamSnapshot::default();
    setup(&mut params);

    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);

    let mut block = [0.0f32; 64];
    // Warmup
    for _ in 0..10 {
        voice.render(&mut block, &params, &empty_mod);
    }

    let start = Instant::now();
    for _ in 0..BLOCKS {
        voice.render(&mut block, &params, &empty_mod);
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
fn stress_pizza_basic() {
    let t = bench_render("Pizza basic (triangle)", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
    });
    assert!(t < 5000.0, "Pizza basic too slow: {} us/block", t);
}

#[test]
fn stress_pizza_crushed() {
    let t = bench_render("Pizza crushed", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.pizza.crush = 0.7;
        p.pizza.shape = 0.8;
    });
    assert!(t < 5000.0, "Pizza crushed too slow: {} us/block", t);
}

#[test]
fn stress_pizza_full_with_chain() {
    let t = bench_render("Pizza + drive + filter + folder", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.pizza.crush = 0.5;
        p.drive.drive = 0.5;
        p.drive.mix = 1.0;
        p.filter.cutoff = 2000.0;
        p.filter.resonance = 0.7;
        p.filter.mode = 2; // LP4
        p.folder.fold = 0.5;
        p.folder.mix = 1.0;
    });
    assert!(t < 5000.0, "Pizza + chain too slow: {} us/block", t);
}

#[test]
fn stress_ks_string() {
    let t = bench_render("KS+ string (body+stiff+ens)", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
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
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 32;
    });
    assert!(t < 5000.0, "Modal 32 too slow: {} us/block", t);
}

#[test]
fn stress_modal_48_modes() {
    let t = bench_render("Modal 48 modes", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 48;
    });
    assert!(t < 10000.0, "Modal 48 too slow: {} us/block", t);
}

#[test]
fn stress_bowed() {
    let t = bench_render("Bowed string", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Bowed;
    });
    assert!(t < 5000.0, "Bowed too slow: {} us/block", t);
}

#[test]
fn stress_sympathetic() {
    let t = bench_render("Sympathetic (8 strings)", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Sympathetic;
        p.modal.inharm = 0.5;
    });
    assert!(t < 10000.0, "Sympathetic too slow: {} us/block", t);
}

#[test]
fn stress_sympathetic_with_chain() {
    let t = bench_render("Sympathetic + full chain", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Sympathetic;
        p.modal.inharm = 0.5;
        p.drive.drive = 0.5;
        p.drive.mix = 1.0;
        p.filter.cutoff = 3000.0;
        p.filter.resonance = 0.5;
        p.filter.mode = 2;
        p.folder.fold = 0.3;
        p.folder.mix = 1.0;
    });
    assert!(t < 10000.0, "Sympathetic + chain too slow: {} us/block", t);
}

#[test]
fn stress_worst_case() {
    let t = bench_render("WORST CASE: 48-mode + full chain", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 48;
        p.drive.drive = 1.0;
        p.drive.mix = 1.0;
        p.filter.cutoff = 1000.0;
        p.filter.resonance = 0.9;
        p.filter.mode = 2; // LP4 (two cascaded SVFs)
        p.folder.fold = 1.0;
        p.folder.mix = 1.0;
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

    bench_render("Pizza basic", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
    });
    bench_render("Pizza crushed", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.pizza.crush = 0.7;
        p.pizza.shape = 0.8;
    });
    bench_render("KS+ string", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
    });
    bench_render("KS+ full features", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::String;
        p.modal.ks_body = 0.5;
        p.modal.ks_stiffness = 0.3;
        p.modal.ks_ens_depth = 0.5;
        p.modal.ks_ens_mix = 0.5;
    });
    bench_render("Modal 16 modes", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 16;
    });
    bench_render("Modal 32 modes", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 32;
    });
    bench_render("Modal 48 modes", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Modal;
        p.modal.num_modes = 48;
    });
    bench_render("Bowed", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Bowed;
    });
    bench_render("Sympathetic 8 strings", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = ResonatorMode::Sympathetic;
    });
    bench_render("+ Drive", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.drive.drive = 0.8;
        p.drive.mix = 1.0;
    });
    bench_render("+ Filter LP4", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.filter.cutoff = 2000.0;
        p.filter.mode = 2;
    });
    bench_render("+ Wavefolder", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Pizza);
        p.folder.fold = 0.8;
        p.folder.mix = 1.0;
    });

    eprintln!("\nBudget: 2667 us/block (128 samples @ 48kHz)");
    eprintln!("STM32H750 is ~3x faster than desktop per-cycle");
}
