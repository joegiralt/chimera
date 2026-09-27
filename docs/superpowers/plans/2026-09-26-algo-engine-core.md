# Algorithmic Engine, Sub-project 1 (Core Engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Pizza, FM and VA engines with one six-operator phase-modulation engine (16 waves, 32 algorithms, A↔B morph, group pages) that plays six voices on the STM32H750 inside the measured cycle budget.

**Architecture:** A functional core in `chimera-core/src/dsp/algo/` (wave tables, TX81Z facts, the envelope, the algorithm tables, `plan`, morph and carrier normalisation, all pure and host-tested) and one imperative shell, `AlgoEngine`, that turns each block's parameters into inputs for a per-sample kernel. The wave tables are rendered from recipes in a host-only crate, `chimera-waves`, by `chimera-core`'s build script. The kernel is benched on the chip before anything depends on it. The old engines leave in four green steps: Algo joins, the chain tests move, the UI default moves, then the deletion.

**Tech Stack:** Rust 2024 (`no_std` core, `f32` only in the render path), a cargo build script, embedded-graphics UI, the DWT cycle counter on the chip, `just`.

**Spec:** `docs/superpowers/specs/2026-09-26-algo-engine-design.md` (rev 2, approved). The adversarial review behind rev 2 is summarised where a task depends on it.

## Global Constraints

- Functional core / imperative shell.
- Type-driven design (ADR 0012).
- `f32` only in the render path (no `f64`, no libm trig there).
- CLAUDE.md's rules: SAFETY comments; no allocation or blocking in audio; nothing snaps.
- Desktop/chip parity (ADR 0013).
- Never stage `docs/chimera-ui-ux-spec.md`. Stage files by name in every commit; never `git add -A` or `git add .`.
- The build/test command is `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`. Written `just check` below, it always means this full command.
- Commit messages are terse and plain, with no conventional prefixes (no `fix:`/`feat:`/`docs:`…), no Co-Authored-By and no Claude attribution.
- Code comments only where genuinely needed (SAFETY, a non-obvious why, or a `ponytail:` ceiling note).
- Run `cargo fmt --all` before each check.
- No mention of any other synth manufacturer's product names beyond Yamaha TX81Z/FS1R (code, comments, docs, ADRs and commit messages).

## Review Focus

Inputs the spec implies but does not test, most likely to bite first. Each has its test in the named task.

1. **An ENV route on an Algo Sound.** The Algo engine does not put the amp envelope on the VCA, and today the envelope only advances when it does, so the matrix's ENV row would be stuck at 0 for the default engine. A user routing ENV → CUTOFF expects a filter sweep. Task 6 keeps the envelope running as the mod source; test `env_source_moves_on_an_algo_sound`.
2. **Stored bytes out of range** (a future Sound file or SysEx load, or a stale pool slot). Every `u8`/`i8` field at its extreme must render finite and bounded, never index out of bounds. Task 5, test `extreme_parameter_bytes_render_finite_and_bounded`.
3. **Retriggering or stealing a sounding voice.** A new note on a voice that is still sounding must not click. Task 5, test `retrigger_on_a_sounding_voice_does_not_click`.
4. **Encoder spin on WAVE or ALG while holding a note** (a change every block). The duck-and-swap must keep up, never stick silent, and recover once the knob stops. Task 5, test `a_wave_change_every_block_recovers_when_it_stops`.
5. **The top of the keyboard with the highest ratio and transpose** (note 127, COARSE 63, FINE 15, TRANSPOSE +24). The increment must stay under Nyquist and the output finite. Task 5, test `the_highest_note_and_ratio_stay_finite`.

## Decisions this plan makes where the spec is silent

The executor must not re-decide these; the reviewer should check them against the spec.

| Decision | Value | Where |
|---|---|---|
| DETUNE curve | 1.5 cents per step (±4.5 cents at ±3) | Task 2, `tx::DETUNE_CENTS` |
| Envelope time scale | every 4 steps of effective rate doubles the speed; AR 31 ≈ 12 samples; a decay falls 96 dB in 92.8 s × 2^(−rate/4) | Task 2, `env.rs` |
| Release rate range | RR 1–15 as on the TX81Z; default 8 (about 0.15 s to silence) | Task 5 |
| Key scaling | key code = (note − 21) / 3, 0–31; added to the rate as `code >> (3 − RS)` | Task 2 |
| PM depth | a full-scale modulator at weight 1 swings its target ±4 cycles | Task 3, `kernel::PM_CYCLES` |
| FEEDBACK 0–7 | off, then 1/32 cycle doubling to 2 cycles | Task 2, `tx::FEEDBACK_CYCLES` |
| VELOCITY 0–7 | each step takes 4 LEVEL steps (3 dB) off at velocity 0, scaled by (1 − velocity) | Task 5 |
| TRANSPOSE | `i8`, −24..+24 semitones | Task 5 |
| Init patch | ALG A = ALG B = T1, MORPH 0, operator 1 on W1 at LEVEL 99, the rest at LEVEL 0 | Task 5 |
| `MAX_EDGES` | 15: six operators with the modulator numbered above its target have at most 15 links, however two algorithms combine (the spec's "at most 30" counts A and B separately) | Task 3 |
| `is_active` | a carrier in the blended graph with a live envelope **and a LEVEL above 0** (a silent carrier does not hold the voice) | Task 5 |
| VELOCITY and RATE SCALE pages | added as OSC sub-pages: the spec's "Done when" requires every sub-project-1 parameter on a page, and its UI table omits these two | Task 11 |
| ALGO page slot E | the voice's output volume, as the FM algorithm page had | Task 6 |
| Amp envelope | always runs, as the ENV mod source; after the deletion no engine puts it on the VCA, so its parameters are no longer modulatable (ADR 0010) | Tasks 6, 9 |
| Sanity gate | the Algo sanity gate and the `algo_init` golden come in Task 6, before Task 8 re-records the instrument goldens on Algo (ADR 0011: gate first) | Task 6 |
| Factory bank | eight Sounds built in code and stored into pool slots 01–08 (indices 0–7) when the UI starts | Task 10 |

## File structure

New:

| File | Responsibility |
|---|---|
| `chimera-waves/` | Host-only crate: wave recipes, their spectra, band-limited mips, the generated Rust source |
| `chimera-core/build.rs` | Writes `$OUT_DIR/waves.rs` from `chimera_waves::emit_rust()` |
| `chimera-core/src/dsp/algo/mod.rs` | Module list |
| `…/algo/waves.rs` | The generated tables, `WaveId`, the flash budget, the mip choice |
| `…/algo/math.rs` | `f32` `exp2`, `log2`, `inv_sqrt` |
| `…/algo/tx.rs` | TX81Z facts: ratios, FINE, LEVEL, D1L, FEEDBACK, DETUNE |
| `…/algo/env.rs` | The per-sample `f32` rate envelope |
| `…/algo/plan.rs` | `Edge`, `EvalPlan`, `blend` |
| `…/algo/kernel.rs` | The per-sample operator loop |
| `…/algo/algorithms.rs` | The 32 algorithms, `AlgoId`, `plan(a, b)` |
| `…/algo/morph.rs` | `Morph`, carrier normalisation, incoming modulation depth |
| `…/algo/params.rs` | `AlgoParams`, `AlgoOpParams`, their specs and `Block` impls |
| `…/algo/engine.rs` | `AlgoEngine`, `AlgoLive` |
| `chimera-core/src/factory.rs` | The eight factory Sounds |
| `chimera-core/src/ui/alg_layout.rs` | The computed algorithm-diagram layout |
| `docs/adr/0022…`, `0023…`, `0024…` | The three ADRs |

Deleted in Task 9: `dsp/pizza.rs`, `dsp/engine_fm.rs`, `dsp/envelope_fm.rs`, `dsp/fm_tables.rs`, `dsp/fm_waveform.rs`, `dsp/oscillator.rs`, `tests/fm_test.rs`, `tests/fm_viz_test.rs`, `tests/oscillator_test.rs`.

## Task order

1. Wave recipes and tables
2. `f32` math, TX81Z facts and the envelope
3. Kernel prototype and chip bench — **stop-and-report checkpoint**
4. The 32 algorithms, `plan`, morph and carrier normalisation
5. `AlgoEngine`
6. Algo joins the engine set; the Algo sanity gate
7. The chain tests move off Pizza
8. The UI default moves to Algo
9. Pizza, FM and VA are deleted
10. The factory bank
11. The group pages
12. Goldens and ADRs 0022–0024
13. The measured `COST` and the six-voice golden — **hardware step**

---

### Task 1: Wave recipes and the generated tables

The 16 starter waves (spec § Waves) are formulas in a host-only crate. `chimera-core`'s build script renders them into `$OUT_DIR/waves.rs`: 16 waves × 8 mips × 256 `i16`, 64 KB, each mip band-limited to one octave fewer harmonics than the one before. TX81Z W1–W8 keep their DC (only W3, W4, W7 and W8 have any); the classic waves are centred.

**Files:**
- Create: `chimera-waves/Cargo.toml`, `chimera-waves/src/lib.rs`, `chimera-waves/tests/recipes_test.rs`
- Create: `chimera-core/build.rs`, `chimera-core/src/dsp/algo/mod.rs`, `chimera-core/src/dsp/algo/waves.rs`, `chimera-core/tests/algo_waves_test.rs`
- Modify: `Cargo.toml` (workspace `members`, `default-members`)
- Modify: `chimera-core/Cargo.toml` (add `[build-dependencies]`)
- Modify: `chimera-core/src/dsp/mod.rs` (add `pub mod algo;`)
- Modify: `Justfile` (`check`, `test`, `clippy` include `chimera-waves`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `chimera_waves::{WAVE_LEN: usize = 256, MIPS: usize = 8, MAX_HARMONIC: usize = 127}`
  - `chimera_waves::Shape { Time(fn(f64) -> f64), Sines(fn(usize) -> f64) }`, `chimera_waves::Recipe { name: &'static str, shape: Shape, keeps_dc: bool }`, `chimera_waves::RECIPES: [Recipe; 16]`
  - `chimera_waves::Spectrum { dc: f64, cos: [f64; 128], sin: [f64; 128] }`, `spectrum(Shape) -> Spectrum`, `mip_harmonics(usize) -> usize`, `render_mip(&Spectrum, bool, usize) -> [f64; 256]`, `render(&Recipe) -> [[i16; 256]; 8]`, `emit_rust() -> String`
  - `chimera_core::dsp::algo::waves::{WAVE_LEN, MIPS, WAVE_COUNT: usize = 16, Table = [i16; 256], WAVE_FLASH_BUDGET: usize = 65_536, WAVES: [[Table; 8]; 16], WAVE_NAMES: [&str; 16]}`
  - `WaveId` (`Copy`, `Eq`) with consts `W1 W2 W3 W4 W5 W6 W7 W8 TRI SAW SQR P25 P12 TSAW RSQR SSAW` (0–15 in that order), `WaveId::clamped(u8) -> WaveId` (const), `get(self) -> u8` (const), `name(self) -> &'static str`, `table(self, mip: usize) -> &'static Table` (mip clamped to 7)

- [ ] **Step 1: Write the failing recipe tests**

Create `chimera-waves/tests/recipes_test.rs`:

```rust
use chimera_waves::{MAX_HARMONIC, MIPS, RECIPES, Recipe, Shape, WAVE_LEN, emit_rust, mip_harmonics, render, spectrum};
use std::f64::consts::PI;

fn naive(r: &Recipe) -> Vec<f64> {
    match r.shape {
        Shape::Time(f) => (0..1024).map(|n| f(n as f64 / 1024.0)).collect(),
        Shape::Sines(_) => Vec::new(),
    }
}

fn mean(mip: &[i16; WAVE_LEN]) -> f64 {
    mip.iter().map(|&v| v as f64).sum::<f64>() / WAVE_LEN as f64 / 32767.0
}

#[test]
fn every_recipe_is_finite_within_unit_peak_and_full_scale() {
    for r in &RECIPES {
        assert!(
            naive(r).iter().all(|v| v.is_finite() && v.abs() <= 1.0 + 1e-12),
            "{}",
            r.name
        );
        let mips = render(r);
        let peak = mips.iter().flatten().map(|v| v.unsigned_abs()).max();
        assert_eq!(peak, Some(32767), "{} is scaled to full scale", r.name);
        assert!(mips.iter().all(|m| m.iter().any(|&v| v != 0)), "{}", r.name);
    }
}

#[test]
fn only_the_tx_waves_keep_dc_and_the_rest_are_centred() {
    for r in &RECIPES {
        let tx = r.name.starts_with('W');
        assert_eq!(r.keeps_dc, tx, "{}", r.name);
        if !tx {
            for (m, mip) in render(r).iter().enumerate() {
                assert!(mean(mip).abs() < 1e-4, "{} mip {m}: DC {}", r.name, mean(mip));
            }
        }
    }
}

/// W3, W4, W7 and W8 are half-wave shapes whose DC is part of the sound;
/// W1, W2, W5 and W6 have none.
#[test]
fn tx_dc_is_where_the_shape_puts_it() {
    let dc = |i: usize| spectrum(RECIPES[i].shape).dc;
    for i in [0, 1, 4, 5] {
        assert!(dc(i).abs() < 1e-9, "{}", RECIPES[i].name);
    }
    assert!((dc(2) - 1.0 / PI).abs() < 1e-6, "W3 is a half sine");
    for i in [3, 6, 7] {
        assert!(dc(i) > 0.1, "{}", RECIPES[i].name);
    }
}

#[test]
fn a_sine_has_only_its_fundamental() {
    let s = spectrum(RECIPES[0].shape);
    assert!((s.sin[1] - 1.0).abs() < 1e-9);
    for k in 2..=MAX_HARMONIC {
        assert!(s.sin[k].abs() < 1e-9 && s.cos[k].abs() < 1e-9, "harmonic {k}");
    }
}

#[test]
fn a_saw_falls_as_one_over_k() {
    let s = spectrum(RECIPES[9].shape);
    for k in 1..=16 {
        let want = 2.0 / (PI * k as f64);
        let got = s.sin[k].hypot(s.cos[k]);
        assert!((got - want).abs() < 1e-3, "harmonic {k}: {got} (want {want})");
    }
}

#[test]
fn each_mip_keeps_half_the_harmonics_of_the_one_before() {
    let h: Vec<usize> = (0..MIPS).map(mip_harmonics).collect();
    assert_eq!(h, [127, 64, 32, 16, 8, 4, 2, 1]);
}

#[test]
fn the_emitted_source_declares_both_tables() {
    let src = emit_rust();
    assert!(src.contains("pub static WAVE_NAMES: [&str; 16]"));
    assert!(src.contains("pub static WAVES: [[[i16; 256]; 8]; 16]"));
}
```

- [ ] **Step 2: Add the crate skeleton and run the tests to see them fail**

`Cargo.toml` (workspace root): add `"chimera-waves"` to both `members` and `default-members`.

Create `chimera-waves/Cargo.toml`:

```toml
[package]
name = "chimera-waves"
version.workspace = true
edition.workspace = true
```

Create an empty `chimera-waves/src/lib.rs`.

Run: `cargo test -p chimera-waves`
Expected: FAIL to compile, "unresolved imports `chimera_waves::MAX_HARMONIC`" and the rest.

- [ ] **Step 3: Write the recipes**

`chimera-waves/src/lib.rs`:

```rust
//! Recipes for the Algo engine's wave tables (ADR 0023). Host-only:
//! `chimera-core`'s build script renders them into flash tables.

use std::f64::consts::{PI, TAU};
use std::fmt::Write;

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
/// Mip 0's top harmonic: the table's Nyquist (128) is left out.
pub const MAX_HARMONIC: usize = 127;
const ANALYSIS_LEN: usize = 4096;

#[derive(Clone, Copy)]
pub enum Shape {
    /// One period over phase `0.0..1.0`.
    Time(fn(f64) -> f64),
    /// Sine amplitude of harmonic `k` (1-based).
    Sines(fn(usize) -> f64),
}

#[derive(Clone, Copy)]
pub struct Recipe {
    pub name: &'static str,
    pub shape: Shape,
    pub keeps_dc: bool,
}

fn w2(t: f64) -> f64 {
    (TAU * t).sin().signum() * (1.0 - (TAU * t).cos().abs())
}

fn first_half(t: f64, f: fn(f64) -> f64) -> f64 {
    if t < 0.5 { f(2.0 * t) } else { 0.0 }
}

const fn tx(name: &'static str, f: fn(f64) -> f64) -> Recipe {
    Recipe { name, shape: Shape::Time(f), keeps_dc: true }
}

const fn classic(name: &'static str, shape: Shape) -> Recipe {
    Recipe { name, shape, keeps_dc: false }
}

pub const RECIPES: [Recipe; 16] = [
    tx("W1", |t| (TAU * t).sin()),
    tx("W2", w2),
    tx("W3", |t| (TAU * t).sin().max(0.0)),
    tx("W4", |t| w2(t).max(0.0)),
    tx("W5", |t| first_half(t, |u| (TAU * u).sin())),
    tx("W6", |t| first_half(t, w2)),
    tx("W7", |t| first_half(t, |u| (TAU * u).sin().abs())),
    tx("W8", |t| first_half(t, |u| w2(u).abs())),
    classic(
        "TRI",
        Shape::Time(|t| {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }),
    ),
    classic("SAW", Shape::Time(|t| 2.0 * ((t + 0.5) % 1.0) - 1.0)),
    classic("SQR", Shape::Time(|t| if t < 0.5 { 1.0 } else { -1.0 })),
    classic("P25", Shape::Time(|t| if t < 0.25 { 1.0 } else { -1.0 })),
    classic("P12", Shape::Time(|t| if t < 0.125 { 1.0 } else { -1.0 })),
    classic(
        "TSAW",
        Shape::Time(|t| {
            if t < 0.75 {
                -1.0 + t / 0.375
            } else {
                1.0 - (t - 0.75) / 0.125
            }
        }),
    ),
    classic(
        "RSQR",
        Shape::Time(|t| (4.0 * (TAU * t).sin()).tanh() / 4.0f64.tanh()),
    ),
    classic(
        "SSAW",
        Shape::Sines(|k| {
            let sign = if k % 2 == 1 { 1.0 } else { -1.0 };
            sign * 2.0 / (PI * k as f64) * (-(k as f64 - 1.0) / 12.0).exp()
        }),
    ),
];

/// One period's Fourier series: DC, then harmonics `1..=MAX_HARMONIC`.
pub struct Spectrum {
    pub dc: f64,
    pub cos: [f64; MAX_HARMONIC + 1],
    pub sin: [f64; MAX_HARMONIC + 1],
}

pub fn spectrum(shape: Shape) -> Spectrum {
    let mut s = Spectrum {
        dc: 0.0,
        cos: [0.0; MAX_HARMONIC + 1],
        sin: [0.0; MAX_HARMONIC + 1],
    };
    match shape {
        Shape::Sines(amp) => {
            for (k, b) in s.sin.iter_mut().enumerate().skip(1) {
                *b = amp(k);
            }
        }
        Shape::Time(f) => {
            // Sampled at step centres, so a jump on a step edge splits evenly.
            let t = |n: usize| (n as f64 + 0.5) / ANALYSIS_LEN as f64;
            let x: Vec<f64> = (0..ANALYSIS_LEN).map(|n| f(t(n))).collect();
            s.dc = x.iter().sum::<f64>() / ANALYSIS_LEN as f64;
            for k in 1..=MAX_HARMONIC {
                let (mut a, mut b) = (0.0, 0.0);
                for (n, v) in x.iter().enumerate() {
                    let ph = TAU * k as f64 * t(n);
                    a += v * ph.cos();
                    b += v * ph.sin();
                }
                s.cos[k] = 2.0 * a / ANALYSIS_LEN as f64;
                s.sin[k] = 2.0 * b / ANALYSIS_LEN as f64;
            }
        }
    }
    s
}

pub fn mip_harmonics(mip: usize) -> usize {
    (128 >> mip).min(MAX_HARMONIC)
}

pub fn render_mip(s: &Spectrum, keeps_dc: bool, mip: usize) -> [f64; WAVE_LEN] {
    core::array::from_fn(|n| {
        let mut v = if keeps_dc { s.dc } else { 0.0 };
        for k in 1..=mip_harmonics(mip) {
            let ph = TAU * (k * n) as f64 / WAVE_LEN as f64;
            v += s.cos[k] * ph.cos() + s.sin[k] * ph.sin();
        }
        v
    })
}

/// Every mip, scaled by one factor so the loudest sample of any mip is
/// full scale: the level does not change from mip to mip.
pub fn render(r: &Recipe) -> [[i16; WAVE_LEN]; MIPS] {
    let s = spectrum(r.shape);
    let mips: Vec<[f64; WAVE_LEN]> = (0..MIPS).map(|m| render_mip(&s, r.keeps_dc, m)).collect();
    let peak = mips.iter().flatten().fold(0.0f64, |p, v| p.max(v.abs()));
    core::array::from_fn(|m| {
        core::array::from_fn(|n| (mips[m][n] / peak * 32767.0).round() as i16)
    })
}

/// The Rust source `chimera-core` includes: `WAVE_NAMES` and `WAVES`.
pub fn emit_rust() -> String {
    let mut out = String::new();
    let names: Vec<String> = RECIPES.iter().map(|r| format!("{:?}", r.name)).collect();
    let _ = writeln!(
        out,
        "pub static WAVE_NAMES: [&str; {}] = [{}];",
        RECIPES.len(),
        names.join(", ")
    );
    let _ = writeln!(
        out,
        "pub static WAVES: [[[i16; {WAVE_LEN}]; {MIPS}]; {}] = [",
        RECIPES.len()
    );
    for r in &RECIPES {
        let _ = writeln!(out, "    [");
        for mip in render(r) {
            let row: Vec<String> = mip.iter().map(i16::to_string).collect();
            let _ = writeln!(out, "        [{}],", row.join(", "));
        }
        let _ = writeln!(out, "    ],");
    }
    let _ = writeln!(out, "];");
    out
}
```

- [ ] **Step 4: Run the recipe tests**

Run: `cargo test -p chimera-waves`
Expected: PASS, 7 tests.

- [ ] **Step 5: Write the failing table tests in the core**

Create `chimera-core/tests/algo_waves_test.rs`:

```rust
//! Spec § Waves: the generated tables are band-limited per mip, fit their
//! flash budget, and `WaveId` names them.

use chimera_core::dsp::algo::waves::{
    MIPS, WAVE_COUNT, WAVE_FLASH_BUDGET, WAVE_LEN, WAVES, Table, WaveId,
};
use std::f64::consts::TAU;

fn magnitude(t: &Table, k: usize) -> f64 {
    let (mut a, mut b) = (0.0, 0.0);
    for (n, &v) in t.iter().enumerate() {
        let ph = TAU * (k * n) as f64 / WAVE_LEN as f64;
        a += v as f64 * ph.cos();
        b += v as f64 * ph.sin();
    }
    a.hypot(b)
}

#[test]
fn every_mip_is_band_limited() {
    for w in 0..WAVE_COUNT as u8 {
        let id = WaveId::clamped(w);
        for mip in 0..MIPS {
            let keep = (128 >> mip).min(127);
            let t = id.table(mip);
            let peak = (1..=keep).map(|k| magnitude(t, k)).fold(0.0, f64::max);
            let above = (keep + 1..=WAVE_LEN / 2)
                .map(|k| magnitude(t, k))
                .fold(0.0, f64::max);
            assert!(
                above < peak * 1e-3,
                "{} mip {mip}: {:.1} dB above its band",
                id.name(),
                20.0 * (above / peak).log10()
            );
        }
    }
}

#[test]
fn the_tables_fit_their_flash_budget() {
    let bytes = core::mem::size_of_val(&WAVES);
    assert_eq!(bytes, WAVE_COUNT * MIPS * WAVE_LEN * 2);
    assert!(bytes <= WAVE_FLASH_BUDGET, "{bytes} B");
}

#[test]
fn wave_ids_name_their_tables() {
    assert_eq!(WaveId::W1.name(), "W1");
    assert_eq!(WaveId::SAW.name(), "SAW");
    assert_eq!(WaveId::SSAW.name(), "SSAW");
    assert_eq!(WaveId::clamped(200), WaveId::SSAW);
    assert_eq!(WaveId::clamped(3).get(), 3);
    assert!(core::ptr::eq(WaveId::W1.table(99), WaveId::W1.table(MIPS - 1)));
}
```

Run: `cargo test -p chimera-core --test algo_waves_test`
Expected: FAIL to compile, "could not find `algo` in `dsp`".

- [ ] **Step 6: Generate the tables in the build script**

`chimera-core/Cargo.toml`, after `[dev-dependencies]`:

```toml
[build-dependencies]
chimera-waves = { path = "../chimera-waves" }
```

Create `chimera-core/build.rs`:

```rust
fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("waves.rs"), chimera_waves::emit_rust()).expect("write waves.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
```

`chimera-core/src/dsp/mod.rs`: add `pub mod algo;` to the module list (alphabetical, before `pub mod chorus;`).

Create `chimera-core/src/dsp/algo/mod.rs`:

```rust
//! The algorithmic engine (spec 2026-09-26-algo-engine-design): six
//! phase-modulation operators on 32 algorithms, morphing between two.

pub mod waves;
```

Create `chimera-core/src/dsp/algo/waves.rs`:

```rust
//! The generated wave tables (ADR 0023).

include!(concat!(env!("OUT_DIR"), "/waves.rs"));

pub const WAVE_LEN: usize = 256;
pub const MIPS: usize = 8;
pub const WAVE_COUNT: usize = 16;
pub type Table = [i16; WAVE_LEN];

pub const WAVE_FLASH_BUDGET: usize = 64 * 1024;
const _: () = assert!(core::mem::size_of::<[[Table; MIPS]; WAVE_COUNT]>() <= WAVE_FLASH_BUDGET);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveId(u8);

impl WaveId {
    pub const W1: WaveId = WaveId(0);
    pub const W2: WaveId = WaveId(1);
    pub const W3: WaveId = WaveId(2);
    pub const W4: WaveId = WaveId(3);
    pub const W5: WaveId = WaveId(4);
    pub const W6: WaveId = WaveId(5);
    pub const W7: WaveId = WaveId(6);
    pub const W8: WaveId = WaveId(7);
    pub const TRI: WaveId = WaveId(8);
    pub const SAW: WaveId = WaveId(9);
    pub const SQR: WaveId = WaveId(10);
    pub const P25: WaveId = WaveId(11);
    pub const P12: WaveId = WaveId(12);
    pub const TSAW: WaveId = WaveId(13);
    pub const RSQR: WaveId = WaveId(14);
    pub const SSAW: WaveId = WaveId(15);

    pub const fn clamped(v: u8) -> Self {
        if (v as usize) < WAVE_COUNT {
            WaveId(v)
        } else {
            WaveId(WAVE_COUNT as u8 - 1)
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn name(self) -> &'static str {
        WAVE_NAMES[self.0 as usize]
    }

    pub fn table(self, mip: usize) -> &'static Table {
        &WAVES[self.0 as usize][mip.min(MIPS - 1)]
    }
}
```

- [ ] **Step 7: Run the table tests**

Run: `cargo test -p chimera-core --test algo_waves_test`
Expected: PASS, 3 tests (the worst mip sits near −88 dB above its band).

- [ ] **Step 8: Put the new crate in the check**

`Justfile`:
- In `check`, change the first line to `cargo test -p chimera-core -p chimera-hal -p chimera-waves` and the first clippy line to `cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop -p chimera-waves --all-targets -- -D warnings`.
- In `test`, change to `cargo test -p chimera-core -p chimera-hal -p chimera-waves`.
- In `clippy`, change the first line to `cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop -p chimera-waves --all-targets -- -D warnings`.

- [ ] **Step 9: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS. The firmware builds link the 64 KB of tables (dead-stripped until Task 3 uses them).

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Justfile chimera-waves chimera-core/Cargo.toml chimera-core/build.rs chimera-core/src/dsp/mod.rs chimera-core/src/dsp/algo/mod.rs chimera-core/src/dsp/algo/waves.rs chimera-core/tests/algo_waves_test.rs
git commit -m "Wave recipes and their band-limited tables"
```

---

### Task 2: `f32` math, TX81Z facts and the envelope

The pure operator core (spec § Voice model, § Rendering): the TX81Z coarse ratio table and FINE steps (a table of facts from the owner's manual), DETUNE, the 0.75 dB LEVEL table, D1L, FEEDBACK, and a per-sample `f32` rate envelope written fresh. Nothing here uses `f64` or libm (spec § Principles: soft-double `f64` caused #26). A source scan guards the whole `algo` module from here on.

The envelope counts each stage's length in samples when the stage starts, so the sample loop does one multiply-add, one decrement and one integer compare, and no float comparison.

**Files:**
- Create: `chimera-core/src/dsp/algo/math.rs`, `chimera-core/src/dsp/algo/tx.rs`, `chimera-core/src/dsp/algo/env.rs`
- Modify: `chimera-core/src/dsp/algo/mod.rs`
- Test: `chimera-core/tests/algo_tx_test.rs`, `chimera-core/tests/algo_env_test.rs`, `chimera-core/tests/algo_source_test.rs`

**Interfaces:**
- Consumes: `chimera_core::MidiNote` (`get() -> u8`, `A4`, `new(u8) -> Option<MidiNote>`).
- Produces:
  - `math::{exp2(f32) -> f32 (const fn), log2(f32) -> f32, inv_sqrt(f32) -> f32}`
  - `tx::{COARSE: [f32; 64], FINE_TOP: [f32; 64], COARSE_NAMES: [&str; 64], ratio(coarse: u8, fine: u8) -> f32, LEVEL_GAIN: [f32; 100], level_gain(f32) -> f32, d1l_level(u8) -> f32, FEEDBACK_CYCLES: [f32; 8], DETUNE_CENTS: f32 = 1.5, detune_factor(i8) -> f32}`
  - `env::{ENV_FLOOR: f32 = 1e-4, EnvRates { ar, d1r, d1l, d2r, rr, rs: u8 }, key_scale(u8, MidiNote) -> u8, effective(u8, u8) -> u8, effective_release(u8, u8) -> u8, attack_add(u8, f32) -> f32, decay_log2(u8, f32) -> f32}`
  - `env::EnvCoefs { attack_add, d1_log2, d1_level, d2_log2, rr_log2: f32 }`, `EnvCoefs::new(EnvRates, MidiNote, sample_rate: f32) -> EnvCoefs`
  - `env::Stage { Idle, Attack, Decay1, Decay2, Release }`
  - `env::OpEnv` (`Copy`) with `IDLE: OpEnv`, `note_on(&mut self, EnvCoefs)`, `note_off(&mut self)`, `set_coefs(&mut self, EnvCoefs)`, `stage() -> Stage`, `level() -> f32`, `is_idle() -> bool`, `step(&mut self) -> f32` (`#[inline(always)]`)

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/algo_tx_test.rs`:

```rust
//! Spec § Voice model: TX81Z ratios and FINE steps, the 0.75 dB LEVEL step
//! with level 0 silent, D1L, FEEDBACK and DETUNE; and the `f32` math.

use chimera_core::dsp::algo::math::{exp2, inv_sqrt, log2};
use chimera_core::dsp::algo::tx::{
    COARSE, COARSE_NAMES, FEEDBACK_CYCLES, FINE_TOP, LEVEL_GAIN, d1l_level, detune_factor,
    level_gain, ratio,
};

fn db(ratio: f32) -> f64 {
    20.0 * (ratio as f64).log10()
}

#[test]
fn the_math_is_close_to_std() {
    for i in 0..60_000 {
        let x = -30.0 + i as f32 * 0.001;
        let r = exp2(x) as f64 / 2f64.powf(x as f64);
        assert!((r - 1.0).abs() < 2e-6, "exp2({x})");
    }
    let mut x = 1e-3f32;
    while x < 1e5 {
        assert!((log2(x) as f64 - (x as f64).log2()).abs() < 3e-5, "log2({x})");
        x *= 1.01;
    }
    let mut x = 1.0f32;
    while x <= 6.0 {
        assert!((inv_sqrt(x) as f64 * (x as f64).sqrt() - 1.0).abs() < 1e-5, "{x}");
        x += 0.01;
    }
}

#[test]
fn exp2_is_exact_at_whole_octaves() {
    for i in -20..20 {
        assert_eq!(exp2(i as f32), 2f32.powi(i));
    }
}

#[test]
fn coarse_is_the_tx81z_ratio_table() {
    assert_eq!(COARSE[0], 0.50);
    assert_eq!(COARSE[4], 1.00);
    assert_eq!(COARSE[8], 2.00);
    assert_eq!(COARSE[13], 4.00);
    assert_eq!(COARSE[63], 25.95);
    assert!(COARSE.windows(2).all(|w| w[0] < w[1]));
    for (i, (c, top)) in COARSE.iter().zip(FINE_TOP).enumerate() {
        assert!(top > *c, "coarse {i}");
        assert_eq!(COARSE_NAMES[i].parse::<f32>().unwrap(), *c);
    }
}

#[test]
fn fine_steps_evenly_toward_the_top_and_eight_below_coarse_4() {
    assert_eq!(ratio(4, 0), 1.0);
    assert!((ratio(4, 15) - FINE_TOP[4]).abs() < 1e-6);
    assert!((ratio(4, 1) - ratio(4, 0) - (FINE_TOP[4] - 1.0) / 15.0).abs() < 1e-6);
    assert!((ratio(0, 7) - FINE_TOP[0]).abs() < 1e-6);
    assert_eq!(ratio(0, 15), ratio(0, 7));
    assert_eq!(ratio(200, 0), COARSE[63]);
}

#[test]
fn level_steps_are_0_75_db_99_is_unity_and_0_is_silent() {
    assert_eq!(LEVEL_GAIN[99], 1.0);
    assert_eq!(LEVEL_GAIN[0], 0.0);
    for l in 2..100 {
        let step = db(LEVEL_GAIN[l] / LEVEL_GAIN[l - 1]);
        assert!((step - 0.75).abs() < 1e-3, "level {l}: {step} dB");
    }
}

#[test]
fn a_fractional_level_lies_between_its_steps() {
    let g = level_gain(50.5);
    assert!(g > LEVEL_GAIN[50] && g < LEVEL_GAIN[51]);
    assert_eq!(level_gain(-3.0), 0.0);
    assert!((level_gain(120.0) - 1.0).abs() < 1e-6);
}

#[test]
fn d1l_steps_are_3_db_and_0_decays_to_silence() {
    assert_eq!(d1l_level(15), 1.0);
    assert_eq!(d1l_level(0), 0.0);
    for d in 2..16 {
        let step = db(d1l_level(d) / d1l_level(d - 1));
        assert!((step - 3.0).abs() < 1e-3, "d1l {d}: {step} dB");
    }
    assert_eq!(d1l_level(40), d1l_level(15));
}

#[test]
fn feedback_doubles_per_step() {
    assert_eq!(FEEDBACK_CYCLES[0], 0.0);
    for n in 2..8 {
        assert_eq!(FEEDBACK_CYCLES[n], 2.0 * FEEDBACK_CYCLES[n - 1]);
    }
}

#[test]
fn detune_is_small_and_symmetric() {
    let cents = |d: i8| 1200.0 * (detune_factor(d) as f64).log2();
    assert_eq!(detune_factor(0), 1.0);
    assert!((cents(3) - 4.5).abs() < 0.01);
    assert!((cents(-3) + 4.5).abs() < 0.01);
    assert_eq!(detune_factor(9), detune_factor(3));
}
```

Create `chimera-core/tests/algo_env_test.rs`:

```rust
//! Spec § Rendering: TX81Z-style rates stepped per sample in `f32`; the
//! fastest attack is at most 16 samples; stage times follow the rate.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv, Stage, key_scale};
use chimera_core::dsp::algo::tx::d1l_level;

const SR: f32 = 48_000.0;
const HOLD: EnvRates = EnvRates { ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 8, rs: 0 };

fn started(r: EnvRates, note: MidiNote) -> OpEnv {
    let mut e = OpEnv::IDLE;
    e.note_on(EnvCoefs::new(r, note, SR));
    e
}

/// Samples spent in `stage` from now, stepping until it changes.
fn samples_in(e: &mut OpEnv, stage: Stage) -> usize {
    let mut n = 0;
    while e.stage() == stage {
        e.step();
        n += 1;
        assert!(n < 50_000_000, "{stage:?} never ends");
    }
    n
}

fn attack(ar: u8) -> usize {
    samples_in(&mut started(EnvRates { ar, ..HOLD }, MidiNote::A4), Stage::Attack)
}

#[test]
fn the_fastest_attack_is_at_most_16_samples() {
    assert!(attack(31) <= 16, "{}", attack(31));
}

#[test]
fn attack_time_doubles_every_two_rate_steps() {
    for ar in [5u8, 11, 17, 23] {
        let r = attack(ar) as f32 / attack(ar + 2) as f32;
        assert!((r - 2.0).abs() < 0.1, "AR {ar}: {r}");
    }
}

#[test]
fn decay_time_doubles_every_two_rate_steps() {
    let decay = |d1r: u8| {
        let mut e = started(EnvRates { d1r, d1l: 0, ..HOLD }, MidiNote::A4);
        samples_in(&mut e, Stage::Attack);
        samples_in(&mut e, Stage::Decay1)
    };
    for d1r in [11u8, 13, 15, 17] {
        let r = decay(d1r) as f32 / decay(d1r + 2) as f32;
        assert!((r - 2.0).abs() < 0.1, "D1R {d1r}: {r}");
    }
}

#[test]
fn decay1_stops_at_d1l_and_d2r_zero_holds() {
    let mut e = started(EnvRates { d1r: 20, d1l: 10, ..HOLD }, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    samples_in(&mut e, Stage::Decay1);
    assert_eq!(e.level(), d1l_level(10));
    for _ in 0..48_000 {
        e.step();
    }
    assert_eq!((e.stage(), e.level()), (Stage::Decay2, d1l_level(10)));
}

#[test]
fn d1l_15_goes_straight_to_the_second_decay() {
    let mut e = started(EnvRates { d1r: 20, d2r: 20, ..HOLD }, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    assert_eq!(e.stage(), Stage::Decay2);
    for _ in 0..48_000 {
        e.step();
    }
    assert!(e.level() < 1.0);
}

#[test]
fn release_ends_idle_and_silent() {
    let mut e = started(HOLD, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    e.note_off();
    samples_in(&mut e, Stage::Release);
    assert!(e.is_idle());
    assert_eq!(e.step(), 0.0);
}

#[test]
fn attack_rate_zero_never_sounds() {
    let mut e = started(EnvRates { ar: 0, ..HOLD }, MidiNote::A4);
    for _ in 0..10_000 {
        assert_eq!(e.step(), 0.0);
    }
    assert_eq!(e.stage(), Stage::Attack);
}

#[test]
fn a_retrigger_attacks_from_the_current_level() {
    let mut e = started(HOLD, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    e.note_off();
    for _ in 0..2_000 {
        e.step();
    }
    let before = e.level();
    assert!(before > 0.0);
    e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, SR));
    assert!(e.step() >= before);
}

#[test]
fn rate_scaling_speeds_up_high_notes() {
    let top = MidiNote::new(108).unwrap();
    assert_eq!(key_scale(3, MidiNote::new(21).unwrap()), 0);
    assert!(key_scale(3, top) > key_scale(0, top));
    let slow = EnvRates { ar: 10, ..HOLD };
    let fast = EnvRates { rs: 3, ..slow };
    let t = |r| samples_in(&mut started(r, top), Stage::Attack);
    assert!(t(fast) < t(slow));
}

#[test]
fn new_rates_apply_to_the_running_stage() {
    let slow = EnvRates { d1r: 4, d1l: 0, ..HOLD };
    let mut e = started(slow, MidiNote::A4);
    samples_in(&mut e, Stage::Attack);
    for _ in 0..100 {
        e.step();
    }
    e.set_coefs(EnvCoefs::new(EnvRates { d1r: 30, ..slow }, MidiNote::A4, SR));
    assert!(samples_in(&mut e, Stage::Decay1) < 2_000);
}
```

Create `chimera-core/tests/algo_source_test.rs`:

```rust
//! Spec § Principles: `f32` only in the render path. The chip has no
//! double-precision FPU, so `f64` becomes software float (#26), and libm's
//! `f32` trig calls soft-double internally.

#[test]
fn the_algo_module_uses_no_f64_and_no_libm() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/dsp/algo");
    let mut files = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let src = std::fs::read_to_string(&path).unwrap();
        for bad in ["f64", "libm"] {
            assert!(!src.contains(bad), "{} mentions {bad}", path.display());
        }
        files += 1;
    }
    assert!(files >= 4, "{files} files scanned");
}
```

Run: `cargo test -p chimera-core --test algo_tx_test --test algo_env_test --test algo_source_test`
Expected: FAIL to compile, "could not find `math` in `algo`".

- [ ] **Step 2: Write `math.rs`**

```rust
//! `f32` approximations of the few transcendental functions the engine needs.

use core::f32::consts::LN_2;

/// `2^x`; relative error below 2e-6 for `x` in `-126..127`, exact at integers.
pub const fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 127.0);
    let t = x as i32;
    let xi = if (t as f32) > x { t - 1 } else { t };
    let f = x - xi as f32;
    let p = 1.0
        + f * (LN_2
            + f * (0.240_226_5
                + f * (0.055_504_11
                    + f * (0.009_618_129
                        + f * (0.001_333_355_8 + f * (0.000_154_035_3 + f * 0.000_015_252_734))))));
    f32::from_bits(((xi + 127) as u32) << 23) * p
}

/// `log2(x)` for `x > 0`; absolute error below 3e-5.
pub fn log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = ((bits >> 23) & 0xff) as i32 - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    e as f32 + s * (2.885_39 + s2 * (0.961_797_6 + s2 * (0.577_078 + s2 * 0.412_198_6)))
}

/// `1 / sqrt(x)` for `x >= 1`; relative error below 1e-5.
pub fn inv_sqrt(x: f32) -> f32 {
    let y = f32::from_bits(0x5f37_59df - (x.to_bits() >> 1));
    let y = y * (1.5 - 0.5 * x * y * y);
    y * (1.5 - 0.5 * x * y * y)
}
```

- [ ] **Step 3: Write `tx.rs`**

The two ratio tables are the TX81Z owner's manual's frequency-ratio chart (coarse ratios, and the ratio FINE 15 reaches from each). Before committing, check every entry against the manual's chart; the tests pin the structure, not each value.

```rust
//! TX81Z operator facts from the owner's manual, re-implemented (ADR 0023).

use crate::dsp::algo::math::exp2;

// 3.14 and 6.28 are TX81Z ratios, not approximations of pi and tau.
#[allow(clippy::approx_constant)]
pub static COARSE: [f32; 64] = [
    0.50, 0.71, 0.78, 0.87, 1.00, 1.41, 1.57, 1.73, 2.00, 2.82, 3.00, 3.14, 3.46, 4.00, 4.24, 4.71,
    5.00, 5.19, 5.65, 6.00, 6.28, 6.92, 7.00, 7.07, 7.85, 8.00, 8.48, 8.65, 9.00, 9.42, 9.89,
    10.00, 10.38, 10.99, 11.00, 11.30, 12.00, 12.11, 12.56, 12.72, 13.00, 13.84, 14.00, 14.10,
    14.13, 15.00, 15.55, 15.57, 15.70, 16.96, 17.27, 17.30, 18.37, 18.84, 19.03, 19.78, 20.41,
    20.76, 21.20, 21.98, 22.49, 23.55, 24.22, 25.95,
];

pub static FINE_TOP: [f32; 64] = [
    0.93, 1.32, 1.37, 1.62, 1.93, 2.73, 3.04, 3.35, 2.93, 4.14, 3.93, 4.61, 5.08, 4.93, 5.55, 6.18,
    5.93, 6.81, 6.96, 6.93, 7.75, 8.54, 7.93, 8.37, 9.32, 8.93, 9.78, 10.27, 9.93, 10.89, 11.19,
    10.93, 12.00, 12.46, 11.93, 12.60, 12.93, 13.73, 14.03, 14.01, 13.93, 15.46, 14.93, 15.42,
    15.60, 15.93, 16.83, 17.19, 17.17, 18.24, 18.74, 18.92, 19.65, 20.31, 20.65, 21.06, 21.88,
    22.38, 22.47, 23.45, 24.11, 25.02, 25.84, 27.57,
];

pub static COARSE_NAMES: [&str; 64] = [
    "0.50", "0.71", "0.78", "0.87", "1.00", "1.41", "1.57", "1.73", "2.00", "2.82", "3.00",
    "3.14", "3.46", "4.00", "4.24", "4.71", "5.00", "5.19", "5.65", "6.00", "6.28", "6.92",
    "7.00", "7.07", "7.85", "8.00", "8.48", "8.65", "9.00", "9.42", "9.89", "10.00", "10.38",
    "10.99", "11.00", "11.30", "12.00", "12.11", "12.56", "12.72", "13.00", "13.84", "14.00",
    "14.10", "14.13", "15.00", "15.55", "15.57", "15.70", "16.96", "17.27", "17.30", "18.37",
    "18.84", "19.03", "19.78", "20.41", "20.76", "21.20", "21.98", "22.49", "23.55", "24.22",
    "25.95",
];

/// FINE takes 16 even steps toward `FINE_TOP`, 8 below coarse index 4
/// (where FINE 8–15 hold the top).
pub fn ratio(coarse: u8, fine: u8) -> f32 {
    let c = (coarse as usize).min(63);
    let (steps, f) = if c < 4 { (7.0, fine.min(7)) } else { (15.0, fine.min(15)) };
    COARSE[c] + (FINE_TOP[c] - COARSE[c]) * f as f32 / steps
}

const LEVEL_STEP_OCT: f32 = 0.75 / 6.020_6;

pub static LEVEL_GAIN: [f32; 100] = {
    let mut g = [0.0; 100];
    let mut l = 1;
    while l < 100 {
        g[l] = exp2(-((99 - l) as f32) * LEVEL_STEP_OCT);
        l += 1;
    }
    g
};

/// A modulated LEVEL is fractional: linear between steps, so it never zippers.
pub fn level_gain(level: f32) -> f32 {
    let l = level.clamp(0.0, 99.0);
    let i = (l as usize).min(98);
    LEVEL_GAIN[i] + (LEVEL_GAIN[i + 1] - LEVEL_GAIN[i]) * (l - i as f32)
}

/// D1L 15 is full level; each step is 3 dB (four LEVEL steps) down; 0 is silent.
pub fn d1l_level(d1l: u8) -> f32 {
    match d1l.min(15) {
        0 => 0.0,
        d => LEVEL_GAIN[99 - 4 * (15 - d as usize)],
    }
}

/// Phase swing, in cycles, of a full-scale output fed back.
pub static FEEDBACK_CYCLES: [f32; 8] = [0.0, 1.0 / 32.0, 1.0 / 16.0, 1.0 / 8.0, 0.25, 0.5, 1.0, 2.0];

pub const DETUNE_CENTS: f32 = 1.5;

pub fn detune_factor(detune: i8) -> f32 {
    exp2(detune.clamp(-3, 3) as f32 * DETUNE_CENTS / 1200.0)
}
```

- [ ] **Step 4: Write `env.rs`**

```rust
//! The operator envelope, stepped per sample in `f32`. Every four steps of
//! effective rate doubles the speed; the scale puts AR 31 at about 12 samples.

use crate::MidiNote;
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::algo::tx::d1l_level;

const ATTACK_SECONDS: f32 = 11.6;
/// Time a decay takes to fall 96 dB (16 octaves) at effective rate 0.
const DECAY_SECONDS: f32 = 92.8;
const DECAY_OCTAVES: f32 = 16.0;
/// −80 dB: a decay or release below this is over.
pub const ENV_FLOOR: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvRates {
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rs: u8,
}

/// Key code 0–31 (four per octave from A0), shifted by rate scaling 0–3.
pub fn key_scale(rs: u8, note: MidiNote) -> u8 {
    let code = (note.get().saturating_sub(21) / 3).min(31);
    code >> (3 - rs.min(3))
}

/// Effective rate 0–63 of a 5-bit rate; rate 0 stays 0 (off or hold).
pub fn effective(rate: u8, ks: u8) -> u8 {
    if rate == 0 { 0 } else { (2 * rate.min(31) + ks).min(63) }
}

pub fn effective_release(rr: u8, ks: u8) -> u8 {
    (4 * rr.clamp(1, 15) + 2 + ks).min(63)
}

pub fn attack_add(r: u8, sample_rate: f32) -> f32 {
    if r == 0 {
        return 0.0;
    }
    1.0 / (ATTACK_SECONDS * exp2(-(r as f32) / 4.0) * sample_rate)
}

/// `log2` of the per-sample decay factor (0: holds). Kept as a log so a
/// stage's length is one division, exact even for the slowest rates.
pub fn decay_log2(r: u8, sample_rate: f32) -> f32 {
    if r == 0 {
        return 0.0;
    }
    -DECAY_OCTAVES / (DECAY_SECONDS * exp2(-(r as f32) / 4.0) * sample_rate)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvCoefs {
    pub attack_add: f32,
    pub d1_log2: f32,
    pub d1_level: f32,
    pub d2_log2: f32,
    pub rr_log2: f32,
}

impl EnvCoefs {
    pub fn new(r: EnvRates, note: MidiNote, sample_rate: f32) -> Self {
        let ks = key_scale(r.rs, note);
        Self {
            attack_add: attack_add(effective(r.ar, ks), sample_rate),
            d1_log2: decay_log2(effective(r.d1r, ks), sample_rate),
            d1_level: d1l_level(r.d1l),
            d2_log2: decay_log2(effective(r.d2r, ks), sample_rate),
            rr_log2: decay_log2(effective_release(r.rr, ks), sample_rate),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay1,
    Decay2,
    Release,
}

/// Each sample is `level * mul + add`. A stage's length is counted when it
/// starts, so the sample loop makes no float comparison.
#[derive(Clone, Copy, Debug)]
pub struct OpEnv {
    level: f32,
    mul: f32,
    add: f32,
    left: u32,
    stage: Stage,
    coefs: EnvCoefs,
}

impl OpEnv {
    pub const IDLE: OpEnv = OpEnv {
        level: 0.0,
        mul: 1.0,
        add: 0.0,
        left: u32::MAX,
        stage: Stage::Idle,
        coefs: EnvCoefs {
            attack_add: 0.0,
            d1_log2: 0.0,
            d1_level: 1.0,
            d2_log2: 0.0,
            rr_log2: 0.0,
        },
    };

    /// Attacks from the current level, so a retrigger never clicks.
    pub fn note_on(&mut self, coefs: EnvCoefs) {
        self.coefs = coefs;
        self.enter(Stage::Attack);
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.enter(Stage::Release);
        }
    }

    pub fn set_coefs(&mut self, coefs: EnvCoefs) {
        self.coefs = coefs;
        self.enter(self.stage);
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline(always)]
    pub fn step(&mut self) -> f32 {
        self.level = self.level * self.mul + self.add;
        self.left -= 1;
        if self.left == 0 {
            self.advance();
        }
        self.level
    }

    fn advance(&mut self) {
        if self.mul == 1.0 && self.add == 0.0 {
            self.left = u32::MAX; // idle or holding: never ends
        } else {
            self.next();
        }
    }

    fn next(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level = 1.0;
                self.enter(Stage::Decay1);
            }
            Stage::Decay1 => {
                self.level = self.coefs.d1_level;
                self.enter(Stage::Decay2);
            }
            Stage::Decay2 | Stage::Release => {
                self.level = 0.0;
                self.enter(Stage::Idle);
            }
            Stage::Idle => {}
        }
    }

    fn enter(&mut self, stage: Stage) {
        let c = self.coefs;
        self.stage = stage;
        match stage {
            Stage::Idle => self.set(1.0, 0.0, u32::MAX),
            Stage::Attack if c.attack_add > 0.0 => {
                let left = ((1.0 - self.level) / c.attack_add) as u32 + 1;
                self.set(1.0, c.attack_add, left);
            }
            Stage::Attack => self.set(1.0, 0.0, u32::MAX),
            Stage::Decay1 => self.decay(c.d1_log2, c.d1_level.max(ENV_FLOOR)),
            Stage::Decay2 => self.decay(c.d2_log2, ENV_FLOOR),
            Stage::Release => self.decay(c.rr_log2, ENV_FLOOR),
        }
    }

    fn decay(&mut self, log2_mul: f32, target: f32) {
        if self.level <= target {
            self.next();
        } else if log2_mul < 0.0 {
            let left = (log2(target / self.level) / log2_mul) as u32 + 1;
            self.set(exp2(log2_mul), 0.0, left);
        } else {
            self.set(1.0, 0.0, u32::MAX);
        }
    }

    fn set(&mut self, mul: f32, add: f32, left: u32) {
        (self.mul, self.add, self.left) = (mul, add, left);
    }
}
```

`chimera-core/src/dsp/algo/mod.rs`: the module list becomes

```rust
pub mod env;
pub mod math;
pub mod tx;
pub mod waves;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p chimera-core --test algo_tx_test --test algo_env_test --test algo_source_test`
Expected: PASS (AR 31 attacks in 11 samples).

- [ ] **Step 6: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/dsp/algo/mod.rs chimera-core/src/dsp/algo/math.rs chimera-core/src/dsp/algo/tx.rs chimera-core/src/dsp/algo/env.rs chimera-core/tests/algo_tx_test.rs chimera-core/tests/algo_env_test.rs chimera-core/tests/algo_source_test.rs
git commit -m "TX81Z operator facts, f32 math and the per-sample envelope"
```

---

### Task 3: Kernel prototype and chip bench (stop-and-report checkpoint)

This task retires the CPU risk before anything depends on the kernel (spec § Budget). It builds the evaluation plan's data shape, the per-sample kernel (6 PM operators, an edge list, feedback averaged over two samples, a crossfade between adjacent mips), the mip choice, and a worst-case measurement on the chip. Target: **≤ 350 cycles per voice per sample for the engine itself.**

Design points the tests pin:
- The kernel keeps each operator's latest output in place. A link whose source runs earlier in the order reads this sample; one that runs later reads the previous sample. No per-link flag is read in the loop.
- Per block, each operator's inputs are copied into a `Lane` in evaluation order so the sample loop walks them front to back; gains, carrier weights, link weights and the output scale ramp linearly across the block (spec § No snapping).
- Phases are `u32` (2^32 is one cycle). A modulation offset is built in 2^24 units and shifted up by 8, one convert and one shift.

**Files:**
- Create: `chimera-core/src/dsp/algo/plan.rs`, `chimera-core/src/dsp/algo/kernel.rs`
- Modify: `chimera-core/src/dsp/algo/waves.rs` (mip choice), `chimera-core/src/dsp/algo/mod.rs`
- Modify: `chimera-stm32/src/bench.rs` (kernel measurement and its row)
- Test: `chimera-core/tests/algo_kernel_test.rs`

**Interfaces:**
- Consumes: `waves::{Table, WaveId, WAVE_LEN, MIPS}`, `env::{OpEnv, EnvCoefs, EnvRates}`, `math::log2`, `tx::FEEDBACK_CYCLES`, `chimera_core::hw::{BLOCK_SIZE, SAMPLE_RATE}`.
- Produces:
  - `plan::{OPS: usize = 6, MAX_EDGES: usize = 15}`, `plan::Edge { src: u8, dst: u8, a: f32, b: f32 }`, `plan::blend(a: f32, b: f32, m: f32) -> f32`
  - `plan::EvalPlan { order: [u8; 6], edges: [Edge; 15], starts: [u8; 7], delayed: u16, carrier_a: [f32; 6], carrier_b: [f32; 6] }` (`Copy`, `PartialEq`), `EvalPlan::build(a_mods: &[u8; 6], a_carriers: u8, b_mods: &[u8; 6], b_carriers: u8) -> EvalPlan`, `EvalPlan::edge_count(&self) -> usize`
  - `kernel::{PM_CYCLES: f32 = 4.0, SAMPLE_SCALE: f32 = 1.0 / 32_767.0}`
  - `kernel::OpBlock { inc: u32, gain_from: f32, gain_to: f32, feedback: f32, lo: &'static Table, hi: &'static Table, xfade: f32 }` (`Copy`)
  - `kernel::KernelBlock<'a> { plan: &'a EvalPlan, ops: [OpBlock; 6], morph_from: f32, morph_to: f32, norm_from: f32, norm_to: f32 }` (`Copy`)
  - `kernel::Kernel` (`Copy`) with `const fn new() -> Kernel`, `reset(&mut self, op: usize)`, `render(&mut self, blk: &KernelBlock, env: &mut [OpEnv; 6], out: &mut [f32; BLOCK_SIZE])`
  - `waves::MIP0_TOP_HZ: f32` (187.5), `WaveId::mip_pair(self, bandwidth_hz: f32) -> (&'static Table, &'static Table, f32)`

- [ ] **Step 1: Write the failing kernel tests**

Create `chimera-core/tests/algo_kernel_test.rs`:

```rust
//! Spec § Rendering: six PM operators on an edge list, feedback averaged
//! over two samples, a crossfade between adjacent mips.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::{MIP0_TOP_HZ, WaveId};
use chimera_hal::BLOCK_SIZE;

const SR: f32 = 48_000.0;
const HOLD: EnvRates = EnvRates { ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 8, rs: 0 };
const NONE: [u8; OPS] = [0; OPS];

fn op(hz: f32, gain: f32, wave: WaveId) -> OpBlock {
    OpBlock {
        inc: (hz / SR * 4_294_967_296.0) as u32,
        gain_from: gain * SAMPLE_SCALE,
        gain_to: gain * SAMPLE_SCALE,
        feedback: 0.0,
        lo: wave.table(0),
        hi: wave.table(0),
        xfade: 0.0,
    }
}

fn silent() -> [OpBlock; OPS] {
    core::array::from_fn(|_| op(0.0, 0.0, WaveId::W1))
}

fn envs() -> [OpEnv; OPS] {
    core::array::from_fn(|_| {
        let mut e = OpEnv::IDLE;
        e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, SR));
        e
    })
}

fn render(plan: &EvalPlan, ops: [OpBlock; OPS], blocks: usize) -> Vec<f32> {
    let (mut k, mut env) = (Kernel::new(), envs());
    let blk = KernelBlock { plan, ops, morph_from: 0.0, morph_to: 0.0, norm_from: 1.0, norm_to: 1.0 };
    let mut out = Vec::new();
    let mut b = [0.0; BLOCK_SIZE];
    for _ in 0..blocks {
        k.render(&blk, &mut env, &mut b);
        out.extend_from_slice(&b);
    }
    out
}

fn hz(s: &[f32]) -> f64 {
    let ups: Vec<f64> = (1..s.len())
        .filter(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .map(|i| (i - 1) as f64 + (-s[i - 1] as f64) / ((s[i] - s[i - 1]) as f64))
        .collect();
    (ups.len() - 1) as f64 * SR as f64 / (ups[ups.len() - 1] - ups[0])
}

fn goertzel(s: &[f32], f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / SR as f64;
    let (mut s1, mut s2) = (0.0, 0.0);
    for &x in s {
        let s0 = x as f64 + 2.0 * w.cos() * s1 - s2;
        (s2, s1) = (s1, s0);
    }
    (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt()
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[test]
fn a_lone_carrier_plays_its_table_at_its_increment() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = op(480.0, 1.0, WaveId::W1);
    let out = render(&plan, ops, 200);
    let f = hz(&out[BLOCK_SIZE..]);
    assert!((f - 480.0).abs() < 0.01, "{f} Hz");
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((peak - 1.0).abs() < 1e-3, "{peak}");
}

#[test]
fn a_silent_modulator_changes_nothing_and_a_loud_one_changes_the_tone() {
    let mut mods = NONE;
    mods[1] = 0b1;
    let plan = EvalPlan::build(&mods, 1, &mods, 1);
    let mut ops = silent();
    ops[0] = op(440.0, 1.0, WaveId::W1);
    let alone = render(&EvalPlan::build(&NONE, 1, &NONE, 1), ops, 20);
    ops[1] = op(880.0, 0.0, WaveId::W1);
    assert!(same_bits(&render(&plan, ops, 20), &alone));
    ops[1] = op(880.0, 0.5, WaveId::W1);
    let fm = render(&plan, ops, 20);
    assert!(fm.iter().zip(&alone).any(|(a, b)| (a - b).abs() > 0.1));
}

#[test]
fn feedback_adds_harmonics_to_a_sine() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let harmonics = |fb: f32| {
        let mut ops = silent();
        ops[0] = OpBlock { feedback: fb, ..op(440.0, 1.0, WaveId::W1) };
        let out = render(&plan, ops, 100);
        (2..=8).map(|h| goertzel(&out[1024..], 440.0 * h as f64)).sum::<f64>()
    };
    assert!(harmonics(FEEDBACK_CYCLES[4]) > 10.0 * harmonics(0.0));
}

#[test]
fn the_crossfade_ends_are_its_two_mips() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let with = |lo: WaveId, hi: WaveId, xfade: f32| {
        let mut ops = silent();
        ops[0] = OpBlock { lo: lo.table(0), hi: hi.table(0), xfade, ..op(300.0, 1.0, WaveId::W1) };
        render(&plan, ops, 10)
    };
    assert!(same_bits(&with(WaveId::W1, WaveId::SAW, 0.0), &with(WaveId::W1, WaveId::W1, 0.0)));
    let top = with(WaveId::W1, WaveId::SAW, 1.0);
    let saw = with(WaveId::SAW, WaveId::SAW, 0.0);
    assert!(top.iter().zip(&saw).all(|(a, b)| (a - b).abs() < 1e-5));
}

#[test]
fn a_gain_change_ramps_across_the_block() {
    let plan = EvalPlan::build(&NONE, 1, &NONE, 1);
    let mut ops = silent();
    ops[0] = OpBlock { gain_from: 0.0, ..op(480.0, 1.0, WaveId::W1) };
    let out = render(&plan, ops, 1);
    let early = out[..8].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(early < 0.15, "{early}");
}

#[test]
fn a_link_that_runs_backwards_is_flagged_as_delayed() {
    let (mut a, mut b) = (NONE, NONE);
    a[1] = 0b01; // 2 → 1
    b[0] = 0b10; // 1 → 2: the union is a cycle
    let plan = EvalPlan::build(&a, 1, &b, 1);
    assert_eq!(plan.delayed.count_ones(), 1);
    let e = plan.delayed.trailing_zeros() as usize;
    assert_eq!((plan.edges[e].src, plan.edges[e].dst), (0, 1));
}

#[test]
fn links_from_higher_to_lower_operators_all_run_forward() {
    let mut seed = 0x2545_f491u32;
    for _ in 0..500 {
        let mut mods = [[0u8; OPS]; 2];
        for m in mods.iter_mut() {
            for (i, bits) in m.iter_mut().enumerate() {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *bits = (seed as u8) & ((1u8 << i) - 1);
            }
        }
        let plan = EvalPlan::build(&mods[0], 1, &mods[1], 1);
        assert_eq!(plan.order, [5, 4, 3, 2, 1, 0]);
        assert_eq!(plan.delayed, 0);
        assert!(plan.edge_count() <= 15);
    }
}

#[test]
fn the_mip_follows_the_bandwidth_and_crossfades() {
    let w = WaveId::SAW;
    let (lo, hi, x) = w.mip_pair(100.0);
    assert!(core::ptr::eq(lo, w.table(0)) && core::ptr::eq(hi, w.table(1)) && x == 0.0);
    let (lo, hi, x) = w.mip_pair(MIP0_TOP_HZ * 2f32.powf(2.5));
    assert!(core::ptr::eq(lo, w.table(2)) && core::ptr::eq(hi, w.table(3)));
    assert!((x - 0.5).abs() < 1e-3, "{x}");
    let (lo, hi, x) = w.mip_pair(1.0e6);
    assert!(core::ptr::eq(lo, w.table(7)) && core::ptr::eq(hi, w.table(7)) && x == 0.0);
}
```

Run: `cargo test -p chimera-core --test algo_kernel_test`
Expected: FAIL to compile, "could not find `kernel` in `algo`".

- [ ] **Step 2: Write `plan.rs`**

```rust
//! The union of two algorithms' links, each weighted at MORPH 0 and 1, in
//! an order where every link runs forward when it can (spec § Plan and morph).

pub const OPS: usize = 6;
pub const MAX_EDGES: usize = 15;

/// `src` modulates `dst` (0-based) with weight `a` in ALG A and `b` in ALG B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub src: u8,
    pub dst: u8,
    pub a: f32,
    pub b: f32,
}

impl Edge {
    const NONE: Edge = Edge { src: 0, dst: 0, a: 0.0, b: 0.0 };
}

/// The one blend of a link weight or carrier gain at MORPH `m`: exact at
/// `m == 0` and `m == 1` for weights of 0 and 1.
#[inline(always)]
pub fn blend(a: f32, b: f32, m: f32) -> f32 {
    a + m * (b - a)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvalPlan {
    pub order: [u8; OPS],
    /// Grouped by target in `order`; ascending source within a target.
    pub edges: [Edge; MAX_EDGES],
    /// Links into `order[k]` are `edges[starts[k]..starts[k + 1]]`.
    pub starts: [u8; OPS + 1],
    /// Bit `e`: link `e`'s source runs later, so it reads the previous sample.
    pub delayed: u16,
    pub carrier_a: [f32; OPS],
    pub carrier_b: [f32; OPS],
}

impl EvalPlan {
    /// `mods[i]` bit `j`: operator `i` modulates operator `j`; `carriers` bit `i`: heard.
    pub fn build(a_mods: &[u8; OPS], a_carriers: u8, b_mods: &[u8; OPS], b_carriers: u8) -> Self {
        let union: [u8; OPS] = core::array::from_fn(|i| a_mods[i] | b_mods[i]);
        let order = topo_order(&union);
        let mut pos = [0usize; OPS];
        for (k, &op) in order.iter().enumerate() {
            pos[op as usize] = k;
        }
        let mut edges = [Edge::NONE; MAX_EDGES];
        let mut starts = [0u8; OPS + 1];
        let mut delayed = 0u16;
        let mut n = 0;
        for (k, &dst) in order.iter().enumerate() {
            starts[k] = n as u8;
            let bit = 1u8 << dst;
            for src in 0..OPS {
                let (in_a, in_b) = (a_mods[src] & bit != 0, b_mods[src] & bit != 0);
                if !(in_a || in_b) {
                    continue;
                }
                if pos[src] > k {
                    delayed |= 1 << n;
                }
                edges[n] = Edge { src: src as u8, dst, a: weight(in_a), b: weight(in_b) };
                n += 1;
            }
        }
        starts[OPS] = n as u8;
        Self {
            order,
            edges,
            starts,
            delayed,
            carrier_a: core::array::from_fn(|i| weight(a_carriers & (1 << i) != 0)),
            carrier_b: core::array::from_fn(|i| weight(b_carriers & (1 << i) != 0)),
        }
    }

    pub fn edge_count(&self) -> usize {
        self.starts[OPS] as usize
    }
}

fn weight(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

/// Kahn's algorithm, highest-numbered ready operator first; a cycle is
/// broken at its highest operator.
fn topo_order(mods: &[u8; OPS]) -> [u8; OPS] {
    let mut indegree = [0u8; OPS];
    for m in mods {
        for (dst, d) in indegree.iter_mut().enumerate() {
            *d += (m >> dst) & 1;
        }
    }
    let mut order = [0u8; OPS];
    let mut placed = 0u8;
    for slot in order.iter_mut() {
        let free = |i: &usize| placed & (1 << i) == 0;
        let Some(i) = (0..OPS)
            .rev()
            .filter(free)
            .find(|&i| indegree[i] == 0)
            .or_else(|| (0..OPS).rev().find(free))
        else {
            break;
        };
        *slot = i as u8;
        placed |= 1 << i;
        for (dst, d) in indegree.iter_mut().enumerate() {
            if mods[i] & (1 << dst) != 0 {
                *d = d.saturating_sub(1);
            }
        }
    }
    order
}
```

- [ ] **Step 3: Add the mip choice to `waves.rs`**

At the top of `waves.rs`, after the module doc:

```rust
use crate::dsp::algo::math::log2;
use crate::hw::SAMPLE_RATE;
```

After `WAVE_FLASH_BUDGET`'s assertion:

```rust
/// Mip 0's 127 harmonics stay under Nyquist up to this fundamental; each
/// mip doubles it.
pub const MIP0_TOP_HZ: f32 = SAMPLE_RATE as f32 / 256.0;
```

In `impl WaveId`, after `table`:

```rust
    /// The two mips either side of `bandwidth_hz` and the crossfade between
    /// them, so a change of mip never steps the sound.
    pub fn mip_pair(self, bandwidth_hz: f32) -> (&'static Table, &'static Table, f32) {
        let top = (MIPS - 1) as f32;
        let m = if bandwidth_hz > MIP0_TOP_HZ {
            log2(bandwidth_hz / MIP0_TOP_HZ).min(top)
        } else {
            0.0
        };
        let lo = m as usize;
        (self.table(lo), self.table(lo + 1), m - lo as f32)
    }
```

- [ ] **Step 4: Write `kernel.rs`**

```rust
//! The per-sample loop: six phase-modulation operators on an `EvalPlan`.

use crate::dsp::algo::env::OpEnv;
use crate::dsp::algo::plan::{EvalPlan, MAX_EDGES, OPS, blend};
use crate::dsp::algo::waves::{Table, WAVE_LEN};
use crate::hw::BLOCK_SIZE;

/// Phase swing, in cycles, of a full-scale modulator at weight 1.
pub const PM_CYCLES: f32 = 4.0;
const PHASE_UNITS: f32 = 16_777_216.0;
const PM_SCALE: f32 = PM_CYCLES * PHASE_UNITS;
pub const SAMPLE_SCALE: f32 = 1.0 / 32_767.0;

#[derive(Clone, Copy, Debug)]
pub struct OpBlock {
    /// 2^32 is one cycle.
    pub inc: u32,
    /// `SAMPLE_SCALE` included.
    pub gain_from: f32,
    pub gain_to: f32,
    /// `FEEDBACK_CYCLES` of the operator's setting.
    pub feedback: f32,
    pub lo: &'static Table,
    pub hi: &'static Table,
    pub xfade: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct KernelBlock<'a> {
    pub plan: &'a EvalPlan,
    pub ops: [OpBlock; OPS],
    pub morph_from: f32,
    pub morph_to: f32,
    pub norm_from: f32,
    pub norm_to: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Kernel {
    phase: [u32; OPS],
    /// Each operator's latest output (eight slots: an index masked with 7
    /// needs no bounds check).
    out: [f32; 8],
    /// The output before that, for feedback averaged over two samples.
    hist: [f32; OPS],
}

#[derive(Clone, Copy)]
struct Lane {
    op: usize,
    phase: u32,
    inc: u32,
    gain: f32,
    dgain: f32,
    carrier: f32,
    dcarrier: f32,
    feedback: f32,
    hist: f32,
    xfade: f32,
    lo: &'static Table,
    hi: &'static Table,
    edges: (usize, usize),
    env: OpEnv,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

impl Kernel {
    pub const fn new() -> Self {
        Self { phase: [0; OPS], out: [0.0; 8], hist: [0.0; OPS] }
    }

    pub fn reset(&mut self, op: usize) {
        self.phase[op] = 0;
        self.out[op] = 0.0;
        self.hist[op] = 0.0;
    }

    pub fn render(&mut self, blk: &KernelBlock, env: &mut [OpEnv; OPS], out: &mut [f32; BLOCK_SIZE]) {
        const STEP: f32 = 1.0 / BLOCK_SIZE as f32;
        let plan = blk.plan;
        let dm = (blk.morph_to - blk.morph_from) * STEP;
        let mut w = [0.0f32; MAX_EDGES];
        let mut dw = [0.0f32; MAX_EDGES];
        let mut src = [0u8; MAX_EDGES];
        for e in 0..plan.edge_count() {
            let edge = plan.edges[e];
            w[e] = blend(edge.a, edge.b, blk.morph_from) * PM_SCALE;
            dw[e] = (edge.b - edge.a) * dm * PM_SCALE;
            src[e] = edge.src;
        }
        let mut lanes: [Lane; OPS] = core::array::from_fn(|k| {
            let op = plan.order[k] as usize % OPS;
            let o = &blk.ops[op];
            Lane {
                op,
                phase: self.phase[op],
                inc: o.inc,
                gain: o.gain_from,
                dgain: (o.gain_to - o.gain_from) * STEP,
                carrier: blend(plan.carrier_a[op], plan.carrier_b[op], blk.morph_from),
                dcarrier: (plan.carrier_b[op] - plan.carrier_a[op]) * dm,
                feedback: o.feedback * 0.5 * PHASE_UNITS,
                hist: self.hist[op],
                xfade: o.xfade,
                lo: o.lo,
                hi: o.hi,
                edges: (plan.starts[k] as usize, plan.starts[k + 1] as usize),
                env: env[op],
            }
        });
        let mut norm = blk.norm_from;
        let dnorm = (blk.norm_to - blk.norm_from) * STEP;
        for s in out.iter_mut() {
            let mut acc = 0.0f32;
            for l in lanes.iter_mut() {
                let prev = self.out[l.op & 7];
                let mut pm = l.feedback * (prev + l.hist);
                for e in l.edges.0..l.edges.1.min(MAX_EDGES) {
                    pm += w[e] * self.out[src[e] as usize & 7];
                    w[e] += dw[e];
                }
                l.phase = l.phase.wrapping_add(l.inc);
                let p = l.phase.wrapping_add((pm as i32 as u32) << 8);
                let y = read(l.lo, l.hi, l.xfade, p) * l.env.step() * l.gain;
                l.gain += l.dgain;
                l.hist = prev;
                self.out[l.op & 7] = y;
                acc += l.carrier * y;
                l.carrier += l.dcarrier;
            }
            *s = acc * norm;
            norm += dnorm;
        }
        for l in &lanes {
            self.phase[l.op] = l.phase;
            self.hist[l.op] = l.hist;
            env[l.op] = l.env;
        }
    }
}

#[inline(always)]
fn read(lo: &Table, hi: &Table, xfade: f32, p: u32) -> f32 {
    let i = (p >> 24) as usize;
    let j = (i + 1) & (WAVE_LEN - 1);
    let f = (p & 0x00ff_ffff) as f32 * (1.0 / PHASE_UNITS);
    let (l0, l1) = (lo[i] as f32, lo[j] as f32);
    let (h0, h1) = (hi[i] as f32, hi[j] as f32);
    let a = l0 + (l1 - l0) * f;
    a + (h0 + (h1 - h0) * f - a) * xfade
}
```

`chimera-core/src/dsp/algo/mod.rs` module list becomes `env, kernel, math, plan, tx, waves`.

- [ ] **Step 5: Run the kernel tests**

Run: `cargo test -p chimera-core --test algo_kernel_test --test algo_source_test`
Expected: PASS.

- [ ] **Step 6: Add the kernel measurement to the bench**

`chimera-stm32/src/bench.rs`. Add imports:

```rust
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::tx::FEEDBACK_CYCLES;
use chimera_core::dsp::algo::waves::WaveId;
```

Add the function (after `impl Rig`):

```rust
/// Spec § Budget worst case, one kernel per voice: six operators, all with
/// feedback, six distinct waves (the D-cache worst case), mip crossfades,
/// MORPH moving through 0.5 between A14 and A22 (the masks are written
/// here, before the algorithm tables exist).
#[inline(never)]
fn time_kernel() -> u32 {
    const A14: [u8; OPS] = [0, 0, 0b11, 0b11, 0b100, 0b1000];
    const A22: [u8; OPS] = [0, 0b1, 0b1, 0b10, 0b100, 0b1_1000];
    const WAVES: [WaveId; OPS] = [
        WaveId::W2,
        WaveId::SAW,
        WaveId::SQR,
        WaveId::P25,
        WaveId::TRI,
        WaveId::W7,
    ];
    let plan = EvalPlan::build(&A14, 0b11, &A22, 0b1);
    let rates = EnvRates { ar: 31, d1r: 0, d1l: 15, d2r: 0, rr: 8, rs: 0 };
    let mut kernels = [Kernel::new(); MAX_VOICES];
    let mut envs = [[OpEnv::IDLE; OPS]; MAX_VOICES];
    for (v, env) in envs.iter_mut().enumerate() {
        let note = MidiNote::new(48 + 5 * v as u8).unwrap_or(MidiNote::A4);
        for e in env.iter_mut() {
            e.note_on(EnvCoefs::new(rates, note, SAMPLE_RATE as f32));
        }
    }
    let blocks: [KernelBlock; MAX_VOICES] = core::array::from_fn(|v| KernelBlock {
        plan: &plan,
        ops: core::array::from_fn(|i| {
            let wave = WAVES[(i + v) % OPS];
            OpBlock {
                inc: (i as u32 + 1) * (11_600_000 + 2_000_000 * v as u32),
                gain_from: 0.9 * SAMPLE_SCALE,
                gain_to: 0.8 * SAMPLE_SCALE,
                feedback: FEEDBACK_CYCLES[7],
                lo: wave.table(2),
                hi: wave.table(3),
                xfade: 0.5,
            }
        }),
        morph_from: 0.45,
        morph_to: 0.55,
        norm_from: 0.7,
        norm_to: 0.7,
    });
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut run = |kernels: &mut [Kernel; MAX_VOICES], envs: &mut [[OpEnv; OPS]; MAX_VOICES]| {
        for ((k, env), blk) in kernels.iter_mut().zip(envs.iter_mut()).zip(&blocks) {
            k.render(blk, env, &mut out);
        }
    };
    for _ in 0..WARM_BLOCKS {
        run(&mut kernels, &mut envs);
    }
    let start = DWT::cycle_count();
    for _ in 0..TIMED_BLOCKS {
        run(&mut kernels, &mut envs);
    }
    let cycles = DWT::cycle_count().wrapping_sub(start);
    core::hint::black_box(&out);
    cycles / (TIMED_BLOCKS * BLOCK_SIZE as u32 * MAX_VOICES as u32)
}
```

In `run`, after the `fx` measurement: `let kernel = time_kernel();` and pass it to `show`: `show(display, clocks, &voices, kernel, &fx);`.

In `show`, add the parameter `kernel: u32` after `voices`, and replace the FX row's first line `let y = 58 + ENGINES as i32 * 30;` with:

```rust
    let y = 58 + ENGINES as i32 * 30;
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    let y = y + 30;
```

- [ ] **Step 7: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS, including `cargo build … --features bench`, its clippy and `just stack-check` (the bench frame is about 3 KB).

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/dsp/algo/mod.rs chimera-core/src/dsp/algo/plan.rs chimera-core/src/dsp/algo/kernel.rs chimera-core/src/dsp/algo/waves.rs chimera-core/tests/algo_kernel_test.rs chimera-stm32/src/bench.rs
git commit -m "Algo kernel prototype and its worst-case bench"
```

- [ ] **Step 9: STOP. Ask the user to run the chip bench, and wait**

Send the user exactly this, then wait for the reply. Do not start Task 4 before it.

> The kernel prototype is committed. Please measure it on the chip:
> 1. Put the synth into DFU mode, as for `just flash`.
> 2. From the repo root, run `just flash-bench`.
> 3. About 10 seconds after the reset the bench screen appears and stays for 30 seconds. Read:
>    - the first line, `BENCH REV <V or Y> <MHz> MHZ`;
>    - the line `KERNEL /VOICE <n> (350)`: the kernel's cycles per voice per sample;
>    - the `PIZZA /VOICE` and `FX` lines, to compare with the last run (about 244 and 3,200).
> 4. Reply with the numbers, then run `just flash` to put the normal firmware back.

Then decide on `n`:
- **n ≤ 350:** the risk is retired; continue to Task 4.
- **n > 350:** apply the spec's fallbacks in order (Appendix A), committing each (`Kernel specialised per block`, `Single mip for high carriers`), and repeat Step 9 after each:
  1. per-block kernel specialisation for the plan's shape;
  2. no mip crossfade for carriers above a pitch threshold;
  3. five voices: no code; `AlgoEngine::COST` measured in Task 13 makes the allocator refuse a sixth. Take this only with the user's agreement.

  Report each fallback taken and the new `n`.

---

### Task 4: The 32 algorithms, `plan`, morph and carrier normalisation

The algorithm tables are data taken from the spec's routings (T1–T8 are the TX81Z's eight on operators 1–4 with a 6→5 pair; A1–A24 by carrier count). Every table algorithm has its modulators numbered above their targets, so the union of any two is ordered 6→1 with no link running backwards: one plan serves the whole morph range, and each end renders exactly as that algorithm alone.

**Files:**
- Create: `chimera-core/src/dsp/algo/algorithms.rs`, `chimera-core/src/dsp/algo/morph.rs`
- Modify: `chimera-core/src/dsp/algo/mod.rs`
- Test: `chimera-core/tests/algo_algorithms_test.rs`, `chimera-core/tests/algo_morph_test.rs`

**Interfaces:**
- Consumes: `plan::{EvalPlan, OPS, blend}`, `math::inv_sqrt`.
- Produces:
  - `algorithms::Algorithm { name: &'static str, mods: [u8; 6], carriers: u8 }`, `algorithms::{ALGO_COUNT: usize = 32, ALGORITHMS: [Algorithm; 32], ALGO_NAMES: [&str; 32]}`
  - `algorithms::AlgoId` (`Copy`, `Eq`) with consts `T1`–`T8` (0–7) and `A1`–`A24` (8–31), `AlgoId::clamped(u8) -> AlgoId` (const), `get(self) -> u8` (const), `algorithm(self) -> &'static Algorithm`
  - `algorithms::plan(a: AlgoId, b: AlgoId) -> EvalPlan`
  - `morph::Morph` (`Copy`, `PartialEq`, `PartialOrd`) with `A`, `B`, `STORED_MAX: f32 = 127.0`, `from_param(f32) -> Morph` (0–127, clamped), `get(self) -> f32`
  - `morph::{carrier_sum(&EvalPlan, Morph) -> f32, carrier_norm(&EvalPlan, Morph) -> f32, incoming(&EvalPlan, Morph, op: usize, gain: &[f32; 6]) -> f32}`

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/algo_algorithms_test.rs`:

```rust
//! Spec § Algorithms and § Testing: every algorithm has operator 1 as a
//! carrier, no self-links, modulators numbered above their targets; T1–T8
//! are the TX81Z's routings; every pair plans forward.

use chimera_core::dsp::algo::algorithms::{ALGO_COUNT, ALGO_NAMES, ALGORITHMS, AlgoId, plan};
use chimera_core::dsp::algo::plan::OPS;

/// (modulator, target) links and carriers.
type Routing = (&'static [(u8, u8)], &'static [u8]);

/// The TX81Z's eight algorithms on operators 1–4 (owner's manual; ADR 0018
/// for 4).
const TX81Z: [Routing; 8] = [
    (&[(4, 3), (3, 2), (2, 1)], &[1]),
    (&[(3, 2), (4, 2), (2, 1)], &[1]),
    (&[(3, 2), (2, 1), (4, 1)], &[1]),
    (&[(4, 3), (3, 1), (2, 1)], &[1]),
    (&[(2, 1), (4, 3)], &[1, 3]),
    (&[(4, 1), (4, 2), (4, 3)], &[1, 2, 3]),
    (&[(4, 3)], &[1, 2, 3]),
    (&[], &[1, 2, 3, 4]),
];

/// Links and carriers per algorithm, counted from the spec's tables.
const LINKS: [u32; ALGO_COUNT] = [
    4, 4, 4, 4, 3, 4, 2, 1, 0, 1, 2, 2, 4, 2, 3, 3, 3, 4, 5, 4, 4, 6, 4, 8, 5, 5, 6, 5, 5, 6, 8, 7,
];
const CARRIERS: [u32; ALGO_COUNT] = [
    2, 2, 2, 2, 3, 4, 4, 5, 6, 5, 5, 4, 5, 4, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1,
];


#[test]
fn every_algorithm_follows_the_convention() {
    for (i, alg) in ALGORITHMS.iter().enumerate() {
        assert!(alg.carriers & 1 != 0, "{}: operator 1 is a carrier", alg.name);
        for (op, m) in alg.mods.iter().enumerate() {
            assert_eq!(m >> op, 0, "{}: operator {} links to itself or up", alg.name, op + 1);
            let heard = alg.carriers & (1 << op) != 0 || *m != 0;
            assert!(heard, "{}: operator {} heard", alg.name, op + 1);
        }
        let links: u32 = alg.mods.iter().map(|m| m.count_ones()).sum();
        assert_eq!(links, LINKS[i], "{} links", alg.name);
        assert_eq!(alg.carriers.count_ones(), CARRIERS[i], "{} carriers", alg.name);
        assert_eq!(ALGO_NAMES[i], alg.name);
    }
}

#[test]
fn names_are_t1_to_t8_then_a1_to_a24() {
    for (i, name) in ALGO_NAMES.iter().enumerate() {
        let want = if i < 8 { format!("T{}", i + 1) } else { format!("A{}", i - 7) };
        assert_eq!(*name, want);
    }
}

#[test]
fn t1_to_t8_are_the_tx81z_routings_with_a_6_to_5_pair() {
    for (t, (links, carriers)) in TX81Z.iter().enumerate() {
        let alg = &ALGORITHMS[t];
        let mut mods = [0u8; OPS];
        for &(m, target) in *links {
            mods[m as usize - 1] |= 1 << (target - 1);
        }
        let carriers = carriers.iter().fold(0u8, |c, &op| c | 1 << (op - 1));
        let low: Vec<u8> = alg.mods[..4].iter().map(|m| m & 0b1111).collect();
        assert_eq!(low, mods[..4], "{} links", alg.name);
        assert_eq!(alg.carriers & 0b1111, carriers, "{} carriers", alg.name);
        assert_eq!((alg.mods[4], alg.mods[5]), (0, 0b1_0000), "{}: 6 → 5", alg.name);
        assert_eq!(alg.carriers >> 4, 0b01, "{}: 5 heard, 6 not", alg.name);
    }
}

/// ADR 0018: in T4, operator 3 modulates operator 1, never operator 2.
#[test]
fn t4_operator_2_is_not_modulated_by_operator_3() {
    let t4 = AlgoId::T4.algorithm();
    assert_eq!(t4.mods[2] & 0b10, 0);
    assert_ne!(t4.mods[2] & 0b01, 0);
}

#[test]
fn every_pair_plans_forward_with_both_algorithms_links() {
    for a in 0..ALGO_COUNT as u8 {
        for b in 0..ALGO_COUNT as u8 {
            let (ia, ib) = (AlgoId::clamped(a), AlgoId::clamped(b));
            let p = plan(ia, ib);
            assert_eq!(p.order, [5, 4, 3, 2, 1, 0]);
            assert_eq!(p.delayed, 0);
            for e in &p.edges[..p.edge_count()] {
                let bit = 1 << e.dst;
                assert_eq!(e.a, (ia.algorithm().mods[e.src as usize] & bit != 0) as u8 as f32);
                assert_eq!(e.b, (ib.algorithm().mods[e.src as usize] & bit != 0) as u8 as f32);
            }
            let union: u32 = (0..OPS)
                .map(|i| (ia.algorithm().mods[i] | ib.algorithm().mods[i]).count_ones())
                .sum();
            assert_eq!(p.edge_count() as u32, union);
        }
    }
}

#[test]
fn ids_clamp_and_name_their_algorithm() {
    assert_eq!(AlgoId::clamped(99), AlgoId::A24);
    assert_eq!(AlgoId::A14.algorithm().name, "A14");
    assert_eq!(AlgoId::T1.get(), 0);
    assert_eq!(AlgoId::A1.get(), 8);
}
```

Create `chimera-core/tests/algo_morph_test.rs`:

```rust
//! Spec § Plan and morph: carrier gains blend with MORPH; the output scale
//! is 1 / sqrt(max(1, sum)); the mip bound counts blended link depth.

use chimera_core::dsp::algo::algorithms::{AlgoId, plan};
use chimera_core::dsp::algo::morph::{Morph, carrier_norm, carrier_sum, incoming};

#[test]
fn morph_maps_the_stored_range_and_clamps() {
    assert_eq!(Morph::from_param(0.0), Morph::A);
    assert_eq!(Morph::from_param(127.0), Morph::B);
    assert_eq!(Morph::from_param(-5.0), Morph::A);
    assert_eq!(Morph::from_param(300.0), Morph::B);
    assert!((Morph::from_param(63.5).get() - 0.5).abs() < 1e-6);
}

#[test]
fn carrier_gains_blend_between_the_ends() {
    let p = plan(AlgoId::T1, AlgoId::A1);
    assert_eq!(carrier_sum(&p, Morph::A), 2.0);
    assert_eq!(carrier_sum(&p, Morph::B), 6.0);
    assert!((carrier_sum(&p, Morph::from_param(63.5)) - 4.0).abs() < 1e-5);
}

#[test]
fn the_output_scale_is_one_over_root_carriers_and_never_boosts() {
    let a1 = plan(AlgoId::A1, AlgoId::A1);
    assert!((carrier_norm(&a1, Morph::A) - 1.0 / 6f32.sqrt()).abs() < 1e-5);
    assert_eq!(carrier_norm(&plan(AlgoId::A17, AlgoId::A17), Morph::A), 1.0);
    let sweep = plan(AlgoId::A17, AlgoId::A1);
    let mut last = carrier_norm(&sweep, Morph::A);
    for i in 1..=127 {
        let n = carrier_norm(&sweep, Morph::from_param(i as f32));
        assert!(n <= last && last - n < 0.02, "step {i}: {last} → {n}");
        last = n;
    }
}

#[test]
fn incoming_depth_sums_blended_links_times_source_gain() {
    let p = plan(AlgoId::A18, AlgoId::A1);
    let gain = [1.0, 0.5, 0.5, 0.5, 0.5, 0.5];
    assert_eq!(incoming(&p, Morph::A, 0, &gain), 2.5);
    assert_eq!(incoming(&p, Morph::B, 0, &gain), 0.0);
    assert!((incoming(&p, Morph::from_param(63.5), 0, &gain) - 1.25).abs() < 1e-5);
    assert_eq!(incoming(&p, Morph::A, 3, &gain), 0.0);
}
```

Run: `cargo test -p chimera-core --test algo_algorithms_test --test algo_morph_test`
Expected: FAIL to compile, "could not find `algorithms` in `algo`".

- [ ] **Step 2: Write `algorithms.rs`**

```rust
//! The 32 algorithms (spec § Algorithms), as data. Operators are 1–6 in the
//! tables; higher numbers modulate lower ones.

use crate::dsp::algo::plan::{EvalPlan, OPS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    pub name: &'static str,
    /// `mods[i]` bit `j`: operator `i + 1` modulates operator `j + 1`.
    pub mods: [u8; OPS],
    pub carriers: u8,
}

const fn alg(name: &'static str, links: &[(u8, u8)], carriers: &[u8]) -> Algorithm {
    let mut mods = [0u8; OPS];
    let mut i = 0;
    while i < links.len() {
        mods[links[i].0 as usize - 1] |= 1 << (links[i].1 - 1);
        i += 1;
    }
    let mut mask = 0u8;
    let mut i = 0;
    while i < carriers.len() {
        mask |= 1 << (carriers[i] - 1);
        i += 1;
    }
    Algorithm { name, mods, carriers: mask }
}

pub const ALGO_COUNT: usize = 32;

pub static ALGORITHMS: [Algorithm; ALGO_COUNT] = [
    alg("T1", &[(4, 3), (3, 2), (2, 1), (6, 5)], &[1, 5]),
    alg("T2", &[(4, 2), (3, 2), (2, 1), (6, 5)], &[1, 5]),
    alg("T3", &[(3, 2), (2, 1), (4, 1), (6, 5)], &[1, 5]),
    alg("T4", &[(4, 3), (3, 1), (2, 1), (6, 5)], &[1, 5]),
    alg("T5", &[(4, 3), (2, 1), (6, 5)], &[1, 3, 5]),
    alg("T6", &[(4, 1), (4, 2), (4, 3), (6, 5)], &[1, 2, 3, 5]),
    alg("T7", &[(4, 3), (6, 5)], &[1, 2, 3, 5]),
    alg("T8", &[(6, 5)], &[1, 2, 3, 4, 5]),
    alg("A1", &[], &[1, 2, 3, 4, 5, 6]),
    alg("A2", &[(6, 1)], &[1, 2, 3, 4, 5]),
    alg("A3", &[(6, 1), (6, 2)], &[1, 2, 3, 4, 5]),
    alg("A4", &[(6, 1), (5, 2)], &[1, 2, 3, 4]),
    alg("A5", &[(6, 1), (6, 2), (6, 3), (6, 4)], &[1, 2, 3, 4, 5]),
    alg("A6", &[(6, 5), (5, 1)], &[1, 2, 3, 4]),
    alg("A7", &[(4, 1), (5, 2), (6, 3)], &[1, 2, 3]),
    alg("A8", &[(4, 1), (5, 1), (6, 1)], &[1, 2, 3]),
    alg("A9", &[(6, 5), (5, 4), (4, 1)], &[1, 2, 3]),
    alg("A10", &[(6, 4), (6, 5), (4, 1), (5, 2)], &[1, 2, 3]),
    alg("A11", &[(5, 4), (6, 4), (4, 1), (4, 2), (4, 3)], &[1, 2, 3]),
    alg("A12", &[(5, 3), (3, 1), (6, 4), (4, 2)], &[1, 2]),
    alg("A13", &[(3, 1), (4, 1), (5, 2), (6, 2)], &[1, 2]),
    alg("A14", &[(3, 1), (3, 2), (4, 1), (4, 2), (5, 3), (6, 4)], &[1, 2]),
    alg("A15", &[(6, 5), (5, 3), (3, 1), (4, 2)], &[1, 2]),
    alg(
        "A16",
        &[(3, 1), (3, 2), (4, 1), (4, 2), (5, 1), (5, 2), (6, 1), (6, 2)],
        &[1, 2],
    ),
    alg("A17", &[(6, 5), (5, 4), (4, 3), (3, 2), (2, 1)], &[1]),
    alg("A18", &[(2, 1), (3, 1), (4, 1), (5, 1), (6, 1)], &[1]),
    alg("A19", &[(4, 2), (4, 3), (2, 1), (3, 1), (6, 5), (5, 1)], &[1]),
    alg("A20", &[(3, 2), (2, 1), (5, 4), (4, 1), (6, 1)], &[1]),
    alg("A21", &[(6, 5), (5, 4), (4, 1), (3, 2), (2, 1)], &[1]),
    alg("A22", &[(6, 4), (6, 5), (4, 2), (5, 3), (2, 1), (3, 1)], &[1]),
    alg(
        "A23",
        &[(6, 2), (6, 3), (6, 4), (6, 5), (2, 1), (3, 1), (4, 1), (5, 1)],
        &[1],
    ),
    alg("A24", &[(6, 5), (5, 2), (5, 3), (5, 4), (2, 1), (3, 1), (4, 1)], &[1]),
];

pub static ALGO_NAMES: [&str; ALGO_COUNT] = {
    let mut n = [""; ALGO_COUNT];
    let mut i = 0;
    while i < ALGO_COUNT {
        n[i] = ALGORITHMS[i].name;
        i += 1;
    }
    n
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoId(u8);

impl AlgoId {
    pub const T1: AlgoId = AlgoId(0);
    pub const T2: AlgoId = AlgoId(1);
    pub const T3: AlgoId = AlgoId(2);
    pub const T4: AlgoId = AlgoId(3);
    pub const T5: AlgoId = AlgoId(4);
    pub const T6: AlgoId = AlgoId(5);
    pub const T7: AlgoId = AlgoId(6);
    pub const T8: AlgoId = AlgoId(7);
    pub const A1: AlgoId = AlgoId(8);
    pub const A2: AlgoId = AlgoId(9);
    pub const A3: AlgoId = AlgoId(10);
    pub const A4: AlgoId = AlgoId(11);
    pub const A5: AlgoId = AlgoId(12);
    pub const A6: AlgoId = AlgoId(13);
    pub const A7: AlgoId = AlgoId(14);
    pub const A8: AlgoId = AlgoId(15);
    pub const A9: AlgoId = AlgoId(16);
    pub const A10: AlgoId = AlgoId(17);
    pub const A11: AlgoId = AlgoId(18);
    pub const A12: AlgoId = AlgoId(19);
    pub const A13: AlgoId = AlgoId(20);
    pub const A14: AlgoId = AlgoId(21);
    pub const A15: AlgoId = AlgoId(22);
    pub const A16: AlgoId = AlgoId(23);
    pub const A17: AlgoId = AlgoId(24);
    pub const A18: AlgoId = AlgoId(25);
    pub const A19: AlgoId = AlgoId(26);
    pub const A20: AlgoId = AlgoId(27);
    pub const A21: AlgoId = AlgoId(28);
    pub const A22: AlgoId = AlgoId(29);
    pub const A23: AlgoId = AlgoId(30);
    pub const A24: AlgoId = AlgoId(31);

    pub const fn clamped(v: u8) -> Self {
        if (v as usize) < ALGO_COUNT {
            AlgoId(v)
        } else {
            AlgoId(ALGO_COUNT as u8 - 1)
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn algorithm(self) -> &'static Algorithm {
        &ALGORITHMS[self.0 as usize]
    }
}

pub fn plan(a: AlgoId, b: AlgoId) -> EvalPlan {
    let (a, b) = (a.algorithm(), b.algorithm());
    EvalPlan::build(&a.mods, a.carriers, &b.mods, b.carriers)
}
```

- [ ] **Step 3: Write `morph.rs`**

```rust
//! MORPH between ALG A and ALG B (ADR 0024).

use crate::dsp::algo::math::inv_sqrt;
use crate::dsp::algo::plan::{EvalPlan, OPS, blend};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Morph(f32);

impl Morph {
    pub const A: Morph = Morph(0.0);
    pub const B: Morph = Morph(1.0);
    pub const STORED_MAX: f32 = 127.0;

    /// A stored MORPH, or a modulated one (fractional, maybe out of range).
    pub fn from_param(v: f32) -> Self {
        Morph((v / Self::STORED_MAX).clamp(0.0, 1.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }
}

pub fn carrier_sum(plan: &EvalPlan, m: Morph) -> f32 {
    (0..OPS)
        .map(|i| blend(plan.carrier_a[i], plan.carrier_b[i], m.get()))
        .sum()
}

/// Equal loudness for uncorrelated carriers, whatever their number.
pub fn carrier_norm(plan: &EvalPlan, m: Morph) -> f32 {
    let sum = carrier_sum(plan, m);
    if sum <= 1.0 { 1.0 } else { inv_sqrt(sum) }
}

/// The modulation depth into `op` the mip choice allows for.
pub fn incoming(plan: &EvalPlan, m: Morph, op: usize, gain: &[f32; OPS]) -> f32 {
    plan.edges[..plan.edge_count()]
        .iter()
        .filter(|e| e.dst as usize == op)
        .map(|e| blend(e.a, e.b, m.get()) * gain[e.src as usize])
        .sum()
}
```

`mod.rs` module list becomes `algorithms, env, kernel, math, morph, plan, tx, waves`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p chimera-core --test algo_algorithms_test --test algo_morph_test --test algo_source_test`
Expected: PASS.

- [ ] **Step 5: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/dsp/algo/mod.rs chimera-core/src/dsp/algo/algorithms.rs chimera-core/src/dsp/algo/morph.rs chimera-core/tests/algo_algorithms_test.rs chimera-core/tests/algo_morph_test.rs
git commit -m "The 32 algorithms, the morph plan and carrier normalisation"
```

---

### Task 5: `AlgoEngine`

The imperative shell. Per block (spec § Rendering) it refreshes the plan when ALG A or B changed, recomputes envelope coefficients when a rate changed, computes each operator's target gain (LEVEL, velocity, level modulation), increment (COARSE, FINE, DETUNE, transpose) and mip pair, and hands a `KernelBlock` to the kernel with every moving value ramped. A WAVE or ALG change while a note sounds ducks the affected output to zero over one block, swaps, and ramps back over the next (spec § No snapping). Parameters are stored as `u8`/`i8` (spec § Voice model, Storage).

**Files:**
- Create: `chimera-core/src/dsp/algo/params.rs`, `chimera-core/src/dsp/algo/engine.rs`
- Modify: `chimera-core/src/dsp/algo/mod.rs`
- Modify: `chimera-core/src/block.rs` (`ValFmt::Signed`), `chimera-core/src/ui/fmt.rs`
- Test: `chimera-core/tests/algo_engine_test.rs`; modify `chimera-core/tests/block_test.rs`, `chimera-core/tests/ui_test.rs`, `chimera-core/tests/in_place_test.rs`

**Interfaces:**
- Consumes: everything in `dsp::algo` so far; `crate::block::{Block, ParamId, ParamSpec, ValFmt}`; `crate::in_place::{by_value, field_list}`; `crate::hw::{BLOCK_SIZE, Cost}`; `crate::{MidiNote, Velocity}` (`Velocity::unit() -> f32`).
- Produces:
  - `ValFmt::Signed(u8)`: a discrete −N..=N shown with a sign.
  - `params::AlgoOpParams { wave, coarse, fine: u8, detune: i8, level, ar, d1r, d1l, d2r, rr, rate_scale, feedback, velocity: u8 }` (`Copy`, `Eq`, `Default`) with `ParamId` consts `WAVE`(0) `COARSE`(1) `FINE`(2) `DETUNE`(3) `LEVEL`(4) `AR`(5) `D1R`(6) `D1L`(7) `D2R`(8) `RR`(9) `RATE_SCALE`(10) `FEEDBACK`(11) `VELOCITY`(12), `rates(&self) -> EnvRates`; `params::ALGO_OP_SPECS: [ParamSpec; 13]` (only `LEVEL` modulatable)
  - `params::AlgoParams { alg_a: u8, alg_b: u8, morph: u8, transpose: i8, ops: [AlgoOpParams; 6] }` with consts `ALG_A`(0) `ALG_B`(1) `MORPH`(2) `TRANSPOSE`(3), `AlgoParams::single(WaveId) -> AlgoParams`; `params::ALGO_SPECS: [ParamSpec; 4]` (only `MORPH` modulatable); `Block` for both
  - `engine::AlgoLive { morph: f32, level: [f32; 6] }` (`Copy`, `PartialEq`), `AlgoLive::from_params(&AlgoParams) -> AlgoLive`
  - `engine::AlgoEngine` with `COST: Cost = Cost(560)` (estimate), `new() -> AlgoEngine`, `init_in_place(&mut MaybeUninit<AlgoEngine>) -> &mut AlgoEngine`, `note_on(&mut self, MidiNote, Velocity, &AlgoParams, sample_rate: u32)`, `note_off(&mut self)`, `render(&mut self, &mut [f32; BLOCK_SIZE], &AlgoParams, &AlgoLive, sample_rate: u32)`, `is_active(&self) -> bool`

- [ ] **Step 1: Write the failing engine tests**

Create `chimera-core/tests/algo_engine_test.rs`:

```rust
//! Spec § Rendering, § Plan and morph, § Testing: pitch, ends of the morph
//! bit-identical to each algorithm alone, equal loudness, nothing snaps.

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn render_with(
    p: &AlgoParams,
    note: u8,
    vel: u8,
    blocks: usize,
    mut change: impl FnMut(usize, &mut AlgoParams),
) -> Vec<f32> {
    let mut e = AlgoEngine::new();
    let mut p = *p;
    e.note_on(MidiNote::new(note).unwrap(), Velocity::new(vel).unwrap(), &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        change(b, &mut p);
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend_from_slice(&blk);
    }
    out
}

fn render(p: &AlgoParams, note: u8, blocks: usize) -> Vec<f32> {
    render_with(p, note, 100, blocks, |_, _| {})
}

/// Six sounding operators at distinct integer ratios, each on its own wave.
fn stack(a: AlgoId, b: AlgoId, morph: u8) -> AlgoParams {
    let mut p = AlgoParams { alg_a: a.get(), alg_b: b.get(), morph, ..AlgoParams::default() };
    for (i, o) in p.ops.iter_mut().enumerate() {
        *o = AlgoOpParams {
            level: 99,
            coarse: [4, 8, 10, 13, 16, 19][i],
            wave: i as u8,
            feedback: 3,
            ..AlgoOpParams::default()
        };
    }
    p
}

fn sines(p: AlgoParams, level: u8) -> AlgoParams {
    let mut p = p;
    for o in p.ops.iter_mut() {
        (o.wave, o.feedback, o.level) = (WaveId::W1.get(), 0, level);
    }
    p
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

fn rms_db(s: &[f32]) -> f64 {
    10.0 * (s.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / s.len() as f64).log10()
}

fn hz(s: &[f32]) -> f64 {
    let ups: Vec<f64> = (1..s.len())
        .filter(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .map(|i| (i - 1) as f64 + (-s[i - 1] as f64) / ((s[i] - s[i - 1]) as f64))
        .collect();
    (ups.len() - 1) as f64 * SR as f64 / (ups[ups.len() - 1] - ups[0])
}

fn same_bits(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

/// Largest sample-to-sample jump at block boundaries and elsewhere.
fn jumps(out: &[f32]) -> (f32, f32) {
    let (mut edge, mut inner) = (0.0f32, 0.0f32);
    for i in 1..out.len() {
        let j = (out[i] - out[i - 1]).abs();
        if i % BLOCK_SIZE == 0 { edge = edge.max(j) } else { inner = inner.max(j) }
    }
    (edge, inner)
}

#[test]
fn the_init_patch_plays_a4_within_a_cent() {
    let out = render(&AlgoParams::default(), 69, 750);
    let cents = 1200.0 * (hz(&out[4800..]) / 440.0).log2();
    assert!(cents.abs() < 1.0, "{cents} cents");
    assert!(peak(&out) <= 1.0);
}

#[test]
fn velocity_sensitivity_lowers_the_level() {
    let mut p = AlgoParams::default();
    p.ops[0].velocity = 7;
    let loud = rms_db(&render_with(&p, 60, 127, 50, |_, _| {})[640..]);
    let soft = rms_db(&render_with(&p, 60, 1, 50, |_, _| {})[640..]);
    let want = 7.0 * 3.0 * (1.0 - 1.0 / 127.0);
    assert!(((loud - soft) - want).abs() < 0.5, "{} dB", loud - soft);
}

#[test]
fn each_end_of_the_morph_is_bit_identical_to_its_algorithm_alone() {
    let a = render(&stack(AlgoId::A14, AlgoId::A22, 0), 60, 40);
    assert!(same_bits(&a, &render(&stack(AlgoId::A14, AlgoId::A14, 0), 60, 40)));
    let b = render(&stack(AlgoId::A14, AlgoId::A22, 127), 60, 40);
    assert!(same_bits(&b, &render(&stack(AlgoId::A22, AlgoId::A22, 0), 60, 40)));
    assert!(a.iter().zip(&b).any(|(x, y)| x != y));
}

#[test]
fn carrier_normalisation_keeps_a1_t1_and_a17_within_a_db() {
    let level: Vec<f64> = [AlgoId::A1, AlgoId::T1, AlgoId::A17]
        .iter()
        .map(|&a| rms_db(&render(&sines(stack(a, a, 0), 99), 57, 200)[3200..]))
        .collect();
    for l in &level {
        assert!((l - level[0]).abs() <= 1.0, "{level:?}");
    }
}

#[test]
fn a_morph_sweep_has_no_step_at_block_edges() {
    let base = sines(stack(AlgoId::T1, AlgoId::A17, 0), 80);
    let out = render_with(&base, 48, 100, 400, |b, p| p.morph = (b * 127 / 399) as u8);
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn a_level_change_ramps_instead_of_stepping() {
    let out = render_with(&AlgoParams::default(), 60, 100, 30, |b, p| {
        if b == 10 {
            p.ops[0].level = 40;
        }
    });
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn a_wave_change_ducks_swaps_and_returns() {
    let out = render_with(&AlgoParams::default(), 60, 100, 30, |b, p| {
        if b == 10 {
            p.ops[0].wave = WaveId::SQR.get();
        }
    });
    let blk = |b: usize| &out[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE];
    assert!(peak(&blk(10)[60..]) < 0.1 * peak(blk(9)), "ducked by the end of the block");
    let edge = 11 * BLOCK_SIZE;
    assert!((out[edge] - out[edge - 1]).abs() < 0.02, "the swap is silent");
    assert!(peak(blk(13)) > 0.5 * peak(blk(9)), "back up");
}

#[test]
fn an_algorithm_change_ducks_the_whole_output() {
    let p = stack(AlgoId::T1, AlgoId::T1, 0);
    let out = render_with(&p, 60, 100, 30, |b, p| {
        if b == 10 {
            (p.alg_a, p.alg_b) = (AlgoId::A1.get(), AlgoId::A1.get());
        }
    });
    let blk = |b: usize| &out[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE];
    assert!(peak(&blk(10)[60..]) < 0.1 * peak(blk(9)));
    assert!(peak(blk(13)) > 0.1);
}

#[test]
fn a_wave_change_every_block_recovers_when_it_stops() {
    let out = render_with(&AlgoParams::default(), 60, 100, 40, |b, p| {
        if b < 20 {
            p.ops[0].wave = (b % 16) as u8;
        }
    });
    assert!(out.iter().all(|x| x.is_finite()));
    let late = &out[24 * BLOCK_SIZE..];
    assert!(peak(late) > 0.5, "stuck silent: {}", peak(late));
}

#[test]
fn retrigger_on_a_sounding_voice_does_not_click() {
    let p = AlgoParams::default();
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p, SR);
    let mut out = Vec::new();
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..30 {
        if b == 15 {
            e.note_on(MidiNote::new(64).unwrap(), Velocity::DEFAULT, &p, SR);
        }
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        out.extend_from_slice(&blk);
    }
    let (edge, inner) = jumps(&out);
    assert!(edge <= inner * 1.05, "edge {edge}, inner {inner}");
}

#[test]
fn extreme_parameter_bytes_render_finite_and_bounded() {
    let op = AlgoOpParams {
        wave: 255,
        coarse: 255,
        fine: 255,
        detune: i8::MIN,
        level: 255,
        ar: 255,
        d1r: 255,
        d1l: 255,
        d2r: 255,
        rr: 255,
        rate_scale: 255,
        feedback: 255,
        velocity: 255,
    };
    let p = AlgoParams { alg_a: 255, alg_b: 255, morph: 255, transpose: i8::MAX, ops: [op; 6] };
    let out = render(&p, 127, 50);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 6f32.sqrt()));
}

#[test]
fn the_highest_note_and_ratio_stay_finite() {
    let mut p = AlgoParams { transpose: 24, ..AlgoParams::default() };
    (p.ops[0].coarse, p.ops[0].fine) = (63, 15);
    let out = render(&p, 127, 50);
    assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
}

#[test]
fn a_release_ends_the_voice_and_a_silent_carrier_does_not_hold_it() {
    let mut p = AlgoParams::default();
    p.ops[4].rr = 1; // operator 5 is a T1 carrier at LEVEL 0
    let mut e = AlgoEngine::new();
    e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR);
    let mut blk = [0.0; BLOCK_SIZE];
    for _ in 0..50 {
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
    }
    e.note_off();
    let mut n = 0;
    while e.is_active() {
        e.render(&mut blk, &p, &AlgoLive::from_params(&p), SR);
        n += 1;
        assert!(n < 400, "never ends");
    }
    assert!(peak(&blk) < 1e-3);
}

#[test]
fn single_puts_operator_1_alone_on_a_wave() {
    let p = AlgoParams::single(WaveId::SAW);
    assert_eq!(p.ops[0].wave, WaveId::SAW.get());
    assert_eq!(p.ops[0].level, 99);
    assert!(p.ops[1..].iter().all(|o| o.level == 0));
    assert_eq!(AlgoLive::from_params(&p).level[0], 99.0);
}
```

Run: `cargo test -p chimera-core --test algo_engine_test`
Expected: FAIL to compile, "could not find `engine` in `algo`".

- [ ] **Step 2: Add `ValFmt::Signed`**

`chimera-core/src/block.rs`, in `enum ValFmt` after `OneBased`:

```rust
    /// Discrete integer −N..=N shown with a sign (`-3`, `0`, `+3`).
    Signed(u8),
```

In `snap_points`, add the arm `ValFmt::Signed(_) => &[0.0, 0.5, 1.0],`. Change `is_bipolar` to `matches!(self, ValFmt::Bi | ValFmt::Pan | ValFmt::Signed(_))`, add `| ValFmt::Signed(_)` to `is_discrete`, and add the arm `ValFmt::Signed(n) => n.saturating_mul(2),` to `max_int`.

`chimera-core/src/ui/fmt.rs`, in `fmt_val` after the `OneBased` arm:

```rust
        ValFmt::Signed(n) => {
            let v = discrete(val, n.saturating_mul(2)) as i16 - n as i16;
            let _ = if v > 0 { write!(buf, "+{v}") } else { write!(buf, "{v}") };
        }
```

Add to `chimera-core/tests/ui_test.rs`:

```rust
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
```

- [ ] **Step 3: Write `params.rs`**

```rust
//! The Algo Sound's parameters, stored as bytes (spec § Voice model).

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::algo::algorithms::ALGO_NAMES;
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::tx::COARSE_NAMES;
use crate::dsp::algo::waves::{WAVE_NAMES, WaveId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoOpParams {
    pub wave: u8,
    pub coarse: u8,
    pub fine: u8,
    pub detune: i8,
    pub level: u8,
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rate_scale: u8,
    pub feedback: u8,
    pub velocity: u8,
}

impl Default for AlgoOpParams {
    fn default() -> Self {
        Self {
            wave: 0,
            coarse: 4,
            fine: 0,
            detune: 0,
            level: 0,
            ar: 31,
            d1r: 0,
            d1l: 15,
            d2r: 0,
            rr: 8,
            rate_scale: 0,
            feedback: 0,
            velocity: 0,
        }
    }
}

impl AlgoOpParams {
    pub const WAVE: ParamId = ParamId(0);
    pub const COARSE: ParamId = ParamId(1);
    pub const FINE: ParamId = ParamId(2);
    pub const DETUNE: ParamId = ParamId(3);
    pub const LEVEL: ParamId = ParamId(4);
    pub const AR: ParamId = ParamId(5);
    pub const D1R: ParamId = ParamId(6);
    pub const D1L: ParamId = ParamId(7);
    pub const D2R: ParamId = ParamId(8);
    pub const RR: ParamId = ParamId(9);
    pub const RATE_SCALE: ParamId = ParamId(10);
    pub const FEEDBACK: ParamId = ParamId(11);
    pub const VELOCITY: ParamId = ParamId(12);

    pub fn rates(&self) -> EnvRates {
        EnvRates {
            ar: self.ar,
            d1r: self.d1r,
            d1l: self.d1l,
            d2r: self.d2r,
            rr: self.rr,
            rs: self.rate_scale,
        }
    }
}

/// LEVEL is read every block and applied to the gain after conversion, so
/// it is the one operator destination (ADR 0010). FINE and FEEDBACK are
/// not: FINE's 104-cent steps would zipper.
pub static ALGO_OP_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "WAVE", ValFmt::Names(&WAVE_NAMES), 15.0, 0.0),
    ParamSpec::stepped(1, "CRSE", ValFmt::Names(&COARSE_NAMES), 0.0, 63.0, 4.0, false),
    ParamSpec::stepped(2, "FINE", ValFmt::Int(15), 0.0, 15.0, 0.0, false),
    ParamSpec::stepped(3, "DETUN", ValFmt::Signed(3), -3.0, 3.0, 0.0, false),
    ParamSpec::stepped(4, "LEVEL", ValFmt::Int(99), 0.0, 99.0, 0.0, true),
    ParamSpec::stepped(5, "AR", ValFmt::Int(31), 0.0, 31.0, 31.0, false),
    ParamSpec::stepped(6, "D1R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(7, "D1L", ValFmt::Int(15), 0.0, 15.0, 15.0, false),
    ParamSpec::stepped(8, "D2R", ValFmt::Int(31), 0.0, 31.0, 0.0, false),
    ParamSpec::stepped(9, "RR", ValFmt::OneBased(14), 1.0, 15.0, 8.0, false),
    ParamSpec::stepped(10, "RS", ValFmt::Int(3), 0.0, 3.0, 0.0, false),
    ParamSpec::stepped(11, "FDBK", ValFmt::Int(7), 0.0, 7.0, 0.0, false),
    ParamSpec::stepped(12, "VEL", ValFmt::Int(7), 0.0, 7.0, 0.0, false),
];

impl Block for AlgoOpParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVE => self.wave as f32,
            Self::COARSE => self.coarse as f32,
            Self::FINE => self.fine as f32,
            Self::DETUNE => self.detune as f32,
            Self::LEVEL => self.level as f32,
            Self::AR => self.ar as f32,
            Self::D1R => self.d1r as f32,
            Self::D1L => self.d1l as f32,
            Self::D2R => self.d2r as f32,
            Self::RR => self.rr as f32,
            Self::RATE_SCALE => self.rate_scale as f32,
            Self::FEEDBACK => self.feedback as f32,
            Self::VELOCITY => self.velocity as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::WAVE => self.wave = v as u8,
            Self::COARSE => self.coarse = v as u8,
            Self::FINE => self.fine = v as u8,
            Self::DETUNE => self.detune = v as i8,
            Self::LEVEL => self.level = v as u8,
            Self::AR => self.ar = v as u8,
            Self::D1R => self.d1r = v as u8,
            Self::D1L => self.d1l = v as u8,
            Self::D2R => self.d2r = v as u8,
            Self::RR => self.rr = v as u8,
            Self::RATE_SCALE => self.rate_scale = v as u8,
            Self::FEEDBACK => self.feedback = v as u8,
            Self::VELOCITY => self.velocity = v as u8,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoParams {
    pub alg_a: u8,
    pub alg_b: u8,
    pub morph: u8,
    pub transpose: i8,
    pub ops: [AlgoOpParams; OPS],
}

impl Default for AlgoParams {
    fn default() -> Self {
        Self::single(WaveId::W1)
    }
}

impl AlgoParams {
    pub const ALG_A: ParamId = ParamId(0);
    pub const ALG_B: ParamId = ParamId(1);
    pub const MORPH: ParamId = ParamId(2);
    pub const TRANSPOSE: ParamId = ParamId(3);

    /// Operator 1 alone at full level on `wave` (the one-operator Sounds).
    pub fn single(wave: WaveId) -> Self {
        let mut ops = [AlgoOpParams::default(); OPS];
        (ops[0].wave, ops[0].level) = (wave.get(), 99);
        Self { alg_a: 0, alg_b: 0, morph: 0, transpose: 0, ops }
    }
}

pub static ALGO_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(0, "ALG A", ValFmt::Names(&ALGO_NAMES), 31.0, 0.0),
    ParamSpec::choice(1, "ALG B", ValFmt::Names(&ALGO_NAMES), 31.0, 0.0),
    ParamSpec::stepped(2, "MORPH", ValFmt::Uni, 0.0, 127.0, 0.0, true),
    ParamSpec::stepped(3, "TRNSP", ValFmt::Signed(24), -24.0, 24.0, 0.0, false),
];

impl Block for AlgoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ALG_A => self.alg_a as f32,
            Self::ALG_B => self.alg_b as f32,
            Self::MORPH => self.morph as f32,
            Self::TRANSPOSE => self.transpose as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ALG_A => self.alg_a = v as u8,
            Self::ALG_B => self.alg_b = v as u8,
            Self::MORPH => self.morph = v as u8,
            Self::TRANSPOSE => self.transpose = v as i8,
            _ => {}
        }
    }
}
```

- [ ] **Step 4: Write `engine.rs`**

```rust
//! `AlgoEngine`: the imperative shell around the pure core.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::dsp::algo::algorithms::{AlgoId, plan};
use crate::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use crate::dsp::algo::kernel::{Kernel, KernelBlock, OpBlock, SAMPLE_SCALE};
use crate::dsp::algo::math::exp2;
use crate::dsp::algo::morph::{Morph, carrier_norm, incoming};
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::algo::plan::{EvalPlan, OPS, blend};
use crate::dsp::algo::tx::{FEEDBACK_CYCLES, detune_factor, level_gain, ratio};
use crate::dsp::algo::waves::WaveId;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::in_place::by_value;
use crate::{MidiNote, Velocity};

/// LEVEL steps (3 dB) one VELOCITY step takes off at velocity 0.
const VELOCITY_STEPS: f32 = 4.0;

/// The block's MORPH and LEVELs after modulation, unrounded, in the stored
/// ranges (ADR 0010's offset formula).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlgoLive {
    pub morph: f32,
    pub level: [f32; OPS],
}

impl AlgoLive {
    pub fn from_params(p: &AlgoParams) -> Self {
        Self {
            morph: p.morph as f32,
            level: core::array::from_fn(|i| p.ops[i].level as f32),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swap {
    Idle,
    /// `waves`: mask of operators whose WAVE changed.
    Ducking { alg: bool, waves: u8 },
    Rising,
}

pub struct AlgoEngine {
    kernel: Kernel,
    env: [OpEnv; OPS],
    plan: EvalPlan,
    plan_key: (AlgoId, AlgoId),
    waves: [WaveId; OPS],
    rates: [EnvRates; OPS],
    /// Gain, MORPH and output scale at the end of the last block: the next
    /// block's ramps start here.
    gain: [f32; OPS],
    morph: f32,
    norm: f32,
    swap: Swap,
    note: MidiNote,
    velocity: f32,
    active: bool,
}

crate::in_place::field_list!(AlgoEngine => AlgoEngine {
    kernel, env, plan, plan_key, waves, rates, gain, morph, norm, swap, note, velocity, active,
});

impl Default for AlgoEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AlgoEngine {
    pub const COST: Cost = Cost(560); // estimate

    pub fn new() -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(Self::init_in_place) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        let key = (AlgoId::T1, AlgoId::T1);
        let rates = AlgoParams::default().ops[0].rates();
        // SAFETY: `p` is valid and unaliased; every field is written once
        // before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).kernel).write(Kernel::new());
            addr_of_mut!((*p).env).write([OpEnv::IDLE; OPS]);
            addr_of_mut!((*p).plan).write(plan(key.0, key.1));
            addr_of_mut!((*p).plan_key).write(key);
            addr_of_mut!((*p).waves).write([WaveId::W1; OPS]);
            addr_of_mut!((*p).rates).write([rates; OPS]);
            addr_of_mut!((*p).gain).write([0.0; OPS]);
            addr_of_mut!((*p).morph).write(0.0);
            addr_of_mut!((*p).norm).write(1.0);
            addr_of_mut!((*p).swap).write(Swap::Idle);
            addr_of_mut!((*p).note).write(MidiNote::A4);
            addr_of_mut!((*p).velocity).write(1.0);
            addr_of_mut!((*p).active).write(false);
            slot.assume_init_mut()
        }
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, p: &AlgoParams, sample_rate: u32) {
        let live = AlgoLive::from_params(p);
        self.note = note;
        self.velocity = velocity.unit();
        self.adopt_alg(p);
        self.swap = Swap::Idle;
        let m = Morph::from_param(live.morph);
        self.morph = m.get();
        self.norm = carrier_norm(&self.plan, m);
        for i in 0..OPS {
            self.waves[i] = WaveId::clamped(p.ops[i].wave);
            self.rates[i] = p.ops[i].rates();
            // A sounding operator keeps its phase, so a retrigger never clicks.
            if self.env[i].is_idle() {
                self.kernel.reset(i);
            }
            self.env[i].note_on(EnvCoefs::new(self.rates[i], note, sample_rate as f32));
            self.gain[i] = self.target_gain(p, &live, i);
        }
        self.active = true;
    }

    pub fn note_off(&mut self) {
        for e in &mut self.env {
            e.note_off();
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(
        &mut self,
        out: &mut [f32; BLOCK_SIZE],
        p: &AlgoParams,
        live: &AlgoLive,
        sample_rate: u32,
    ) {
        if !self.active {
            out.fill(0.0);
            return;
        }
        let sr = sample_rate as f32;
        self.begin_swap(p);
        for i in 0..OPS {
            let r = p.ops[i].rates();
            if r != self.rates[i] {
                self.rates[i] = r;
                self.env[i].set_coefs(EnvCoefs::new(r, self.note, sr));
            }
        }
        let m = Morph::from_param(live.morph);
        let level: [f32; OPS] = core::array::from_fn(|i| level_gain(live.level[i]));
        let cycles =
            440.0 * exp2((self.note.get() as f32 + p.transpose as f32 - 69.0) / 12.0) / sr;
        let ops: [OpBlock; OPS] = core::array::from_fn(|i| {
            let o = &p.ops[i];
            let x = (cycles * ratio(o.coarse, o.fine) * detune_factor(o.detune)).min(0.5);
            let bandwidth = x * sr * (1.0 + incoming(&self.plan, m, i, &level));
            let (lo, hi, xfade) = self.waves[i].mip_pair(bandwidth);
            OpBlock {
                inc: (x * 4_294_967_296.0) as u32,
                gain_from: self.gain[i],
                gain_to: self.target_gain(p, live, i) * self.op_duck(i),
                feedback: FEEDBACK_CYCLES[o.feedback.min(7) as usize],
                lo,
                hi,
                xfade,
            }
        });
        let norm = carrier_norm(&self.plan, m) * self.alg_duck();
        let blk = KernelBlock {
            plan: &self.plan,
            ops,
            morph_from: self.morph,
            morph_to: m.get(),
            norm_from: self.norm,
            norm_to: norm,
        };
        self.kernel.render(&blk, &mut self.env, out);
        for (g, o) in self.gain.iter_mut().zip(&ops) {
            *g = o.gain_to;
        }
        (self.morph, self.norm) = (m.get(), norm);
        self.finish_swap(p);
        self.active = (0..OPS).any(|i| {
            blend(self.plan.carrier_a[i], self.plan.carrier_b[i], self.morph) > 0.0
                && live.level[i] > 0.0
                && !self.env[i].is_idle()
        });
    }

    fn target_gain(&self, p: &AlgoParams, live: &AlgoLive, i: usize) -> f32 {
        let vel = p.ops[i].velocity.min(7) as f32 * (1.0 - self.velocity) * VELOCITY_STEPS;
        level_gain(live.level[i] - vel) * SAMPLE_SCALE
    }

    fn adopt_alg(&mut self, p: &AlgoParams) {
        let key = (AlgoId::clamped(p.alg_a), AlgoId::clamped(p.alg_b));
        if key != self.plan_key {
            self.plan = plan(key.0, key.1);
            self.plan_key = key;
        }
    }

    fn begin_swap(&mut self, p: &AlgoParams) {
        if self.swap != Swap::Idle {
            return;
        }
        let alg = (AlgoId::clamped(p.alg_a), AlgoId::clamped(p.alg_b)) != self.plan_key;
        let waves = (0..OPS).fold(0u8, |m, i| {
            if WaveId::clamped(p.ops[i].wave) != self.waves[i] { m | 1 << i } else { m }
        });
        if alg || waves != 0 {
            self.swap = Swap::Ducking { alg, waves };
        }
    }

    fn finish_swap(&mut self, p: &AlgoParams) {
        self.swap = match self.swap {
            Swap::Ducking { alg, waves } => {
                if alg {
                    self.adopt_alg(p);
                }
                for i in 0..OPS {
                    if waves & (1 << i) != 0 {
                        self.waves[i] = WaveId::clamped(p.ops[i].wave);
                    }
                }
                Swap::Rising
            }
            Swap::Rising | Swap::Idle => Swap::Idle,
        };
    }

    fn op_duck(&self, i: usize) -> f32 {
        match self.swap {
            Swap::Ducking { waves, .. } if waves & (1 << i) != 0 => 0.0,
            _ => 1.0,
        }
    }

    fn alg_duck(&self) -> f32 {
        match self.swap {
            Swap::Ducking { alg: true, .. } => 0.0,
            _ => 1.0,
        }
    }
}
```

`mod.rs` module list becomes `algorithms, engine, env, kernel, math, morph, params, plan, tx, waves`.

- [ ] **Step 5: Run the engine tests**

Run: `cargo test -p chimera-core --test algo_engine_test --test algo_source_test --test ui_test`
Expected: PASS (A4 within 0.001 cents; A1, T1 and A17 all near −3.0 dB).

- [ ] **Step 6: Block conformity and the in-place constructor**

`chimera-core/tests/block_test.rs`, after `fm_conforms`:

```rust
#[test]
fn algo_conforms() {
    conforms("algo", chimera_core::dsp::algo::params::AlgoParams::default());
    conforms("algo_op", chimera_core::dsp::algo::params::AlgoOpParams::default());
}
```

`chimera-core/tests/in_place_test.rs`, at the end (the file already has `poisoned`):

```rust
/// Spec § Testing: `init_in_place` equals `new` for `AlgoEngine`.
#[test]
fn algo_engine_built_in_place_renders_like_new() {
    use chimera_core::dsp::algo::engine::{AlgoEngine, AlgoLive};
    use chimera_core::dsp::algo::params::AlgoParams;
    let p = AlgoParams::default();
    let live = AlgoLive::from_params(&p);
    let mut slot = poisoned::<AlgoEngine>();
    let engines = [&mut AlgoEngine::new(), AlgoEngine::init_in_place(&mut slot)];
    let outs: Vec<Vec<u32>> = engines
        .into_iter()
        .map(|e| {
            e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR);
            let mut out = Vec::new();
            let mut blk = [0.0f32; BLOCK_SIZE];
            for _ in 0..20 {
                e.render(&mut blk, &p, &live, SR);
                out.extend(blk.iter().map(|s| s.to_bits()));
            }
            out
        })
        .collect();
    assert_eq!(outs[0], outs[1]);
}
```

(`in_place_test.rs` imports `MidiNote`, `Velocity`, `BLOCK_SIZE` and defines `SR`; add any of these that are missing to its `use` lines.)

Run: `cargo test -p chimera-core --test block_test --test in_place_test`
Expected: PASS.

- [ ] **Step 7: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/block.rs chimera-core/src/ui/fmt.rs chimera-core/src/dsp/algo/mod.rs chimera-core/src/dsp/algo/params.rs chimera-core/src/dsp/algo/engine.rs chimera-core/tests/algo_engine_test.rs chimera-core/tests/block_test.rs chimera-core/tests/ui_test.rs chimera-core/tests/in_place_test.rs
git commit -m "AlgoEngine with ramped morph and level and ducked swaps"
```

---

### Task 6: Algo joins the engine set; the Algo sanity gate

The first of four green steps that replace the old engines (spec § Replacing the old engines). Algo is added **beside** Pizza, FM and VA, so every existing test and golden still passes; nothing is deleted yet. `Op` grows to A–F. The seven modulation destinations (MORPH and six LEVELs) reach the engine through `AlgoLive`, unrounded, so level modulation lands on the `f32` gain (spec § Voice model). The amp envelope keeps running as the ENV mod source even where it is off the VCA (Review Focus 1). The ADR 0011 sanity gate runs on the Algo init patch, then its `algo_init` golden is recorded; this comes before Task 8 re-records the instrument goldens on Algo.

**Files:**
- Modify: `chimera-core/src/addr.rs`, `chimera-core/src/params.rs`, `chimera-core/src/preset.rs`, `chimera-core/src/dsp/engines.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/dsp/algo/engine.rs`
- Modify: `chimera-core/src/ui/mod_grid.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/block_def.rs`, `chimera-core/src/ui/part_page.rs`, `chimera-core/src/ui/block_registry.rs`, `chimera-core/src/ui/chain.rs`, `chimera-core/src/ui/browser.rs`
- Modify: `chimera-stm32/src/bench.rs`
- Test (modify): `addr_test.rs`, `engines_test.rs`, `engine_source_test.rs`, `cost_test.rs`, `modulatable_test.rs`, `modulation_integration_test.rs`, `block_def_tests.rs`, `focus_test.rs`, `part_page_test.rs`, `browser_test.rs`, `sanity_test.rs`, `golden_test.rs`, `common/mod.rs`, `algo_engine_test.rs`, `screen_golden_test.rs` (all under `chimera-core/tests/`)

**Interfaces:**
- Consumes: `AlgoEngine`, `AlgoLive`, `AlgoParams`, `AlgoOpParams`, `ALGO_SPECS`, `ALGO_OP_SPECS` (Task 5).
- Produces:
  - `Op { A, B, C, D, E, F }`, `Op::ALL: [Op; 6]`, `nudged` clamps at F, `TryFrom<u8>` rejects 6 and above.
  - `BlockRef::Algo`, `BlockRef::AlgoOp(Op)`; `BlockRef::ALL: [BlockRef; 26]`.
  - `EngineType::Algo` (= 4, last), `EngineType::ALL: [EngineType; 5]`; `ChainType::Algo` (= 3, last), `ChainType::ALL: [ChainType; 4]`, label `"Algo"`.
  - `ParamSnapshot.algo: AlgoParams` (public field).
  - `AlgoLive::offset(&mut self, addr: ParamAddr, off: f32) -> bool`.
  - `Engines::render(&mut self, kind, out, p: &ParamSnapshot, live: &AlgoLive)` (new `live` argument).
  - `block_registry::{ALGO_WAVE (id 42, short "OSC"), ALGO_ALG (id 43, short "ALG"), ALGO_LEVEL (id 44, short "LVL"), ALGO_CHAIN}`; private helpers `op_row(ParamId) -> [ParamSlot; 6]` and `group(id, name, short, ParamId) -> BlockDef`.
  - `browser::INIT_TYPES == ChainType::ALL`.
  - Golden case `Case::AlgoInit` (`"algo_init"`).

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/addr_test.rs`:
- `op_rejects_out_of_range`: change `Op::try_from(4)` to `Op::try_from(6)` and `OpOutOfRange(4)` to `OpOutOfRange(6)`.
- `op_nudge_clamps`: replace the last two assertions with `assert_eq!(Op::C.nudged(127), Op::F);` and `assert_eq!(Op::F.nudged(-128), Op::A);`.
- `modulatable_addresses_are_exactly_the_spec_list`: replace the `for op in Op::ALL` loop with

```rust
    for op in [Op::A, Op::B, Op::C, Op::D] {
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::LEVEL));
        want.push(ParamAddr::new(BlockRef::FmOp(op), FmOpParams::FEEDBACK));
    }
    want.push(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH));
    for op in Op::ALL {
        want.push(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    }
```

and add `use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};`.

Append to `chimera-core/tests/algo_engine_test.rs`:

```rust
#[test]
fn live_values_take_offsets_by_the_adr_0010_formula_and_clamp() {
    use chimera_core::addr::{BlockRef, Op, ParamAddr};
    use chimera_core::params::FilterParams;
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let level = |op| ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL);
    let mut live = AlgoLive::from_params(&AlgoParams::default());
    assert!(live.offset(morph, 0.5));
    assert_eq!(live.morph, 63.5);
    assert!(live.offset(morph, 2.0));
    assert_eq!(live.morph, 127.0);
    assert!(live.offset(level(Op::F), -0.25));
    assert_eq!(live.level[5], 0.0);
    assert!(live.offset(level(Op::A), -0.25));
    assert_eq!(live.level[0], 99.0 - 24.75);
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    assert!(!live.offset(cutoff, 0.5));
}
```

`chimera-core/tests/engines_test.rs`: in `all_lists_every_engine_once_in_order` change `EngineType::Va as usize + 1` to `EngineType::Algo as usize + 1`; in `render`, change the call to `e.render(kind, &mut out, p, &AlgoLive::from_params(&p.algo));` (import `chimera_core::dsp::algo::engine::AlgoLive`); add the row `| Algo   | no             | a carrier's envelope   |` to the module doc table and this test:

```rust
#[test]
fn algo_row_no_amp_env_and_lives_until_its_carriers_release() {
    let kind = EngineType::Algo;
    assert!(!Engines::uses_amp_env(kind));
    let mut e = Engines::new(SR);
    let idle_env = Envelope::new(); // Algo activity ignores the amp envelope
    assert!(!e.is_active(kind, &idle_env));
    e.note_on(kind, MidiNote::A4, Velocity::DEFAULT, &params(kind));
    render(&mut e, kind, &params(kind), 1);
    assert!(e.is_active(kind, &idle_env));
    e.note_off(kind);
    render(&mut e, kind, &params(kind), 400);
    assert!(!e.is_active(kind, &idle_env));
}
```

`chimera-core/tests/modulation_integration_test.rs`, append:

```rust
/// Review Focus 1: the Algo engine keeps the amp envelope off the VCA, but
/// the envelope still drives the ENV mod source.
#[test]
fn env_source_moves_on_an_algo_sound() {
    use chimera_core::params::EngineType;
    let mut params = ParamSnapshot::for_engine(EngineType::Algo);
    params.filter.cutoff = 8000.0;
    params.filter.mode = 2;
    let mut registry = chimera_core::mod_path::ModDestRegistry::new();
    registry.add(CUTOFF, *b"FLTCUT\0\0").unwrap();
    let mut routed = ModState::from_registry(&registry, 2);
    routed.set_amount(0, 0, -100); // ENV → cutoff
    let render = |ms: &ModState| {
        let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
        voice.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &params);
        let mut out = [0.0f32; BLOCK_SIZE];
        let mut all = Vec::new();
        for _ in 0..16 {
            voice.render(&mut out, &params, ms);
            all.extend_from_slice(&out);
        }
        all
    };
    let (dry, wet) = (render(&ModState::new()), render(&routed));
    let diff: f32 = dry.iter().zip(&wet).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 0.01, "the ENV route changed nothing ({diff})");
}
```

`chimera-core/tests/block_def_tests.rs`, append:

```rust
#[test]
fn algo_chain_is_osc_alg_then_the_voice_chain() {
    let chain = &block_registry::ALGO_CHAIN;
    let shorts: Vec<&str> = chain.blocks.iter().map(|b| b.def.short).collect();
    assert_eq!(shorts, ["OSC", "ALG", "DRV", "FLT", "FLD", "MOD"]);
    assert_eq!(chain.active_def(0, 0).unwrap().name, "Wave");
    assert!(chain.blocks[0].sub_pages.iter().any(|d| d.name == "Level"));
    assert_eq!(chain.blocks[5].sub_pages.len(), 2);
}
```

`chimera-core/tests/part_page_test.rs`: in `fm_operator_page_follows_the_selection`, the selector now reaches operator F. Replace its first six lines after `let mut op = Op::A;` with

```rust
    part_page::apply_encoder(&reg::FM_OP, 0, 1, &mut p, &mut op); // selector
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&reg::FM_OP, 0, 9, &mut p, &mut op);
    assert_eq!(op, Op::F);
    part_page::apply_encoder(&reg::FM_OP, 0, -4, &mut p, &mut op);
    assert_eq!(op, Op::B);
```

and change `1.0 / 3.0` to `1.0 / 5.0`. Add (import `chimera_core::params::EngineType`):

```rust
#[test]
fn algo_pages_edit_every_operator_and_the_algorithm() {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    for slot in 0..6 {
        turn(&reg::ALGO_WAVE, slot, 1 + slot as i8, &mut p);
    }
    assert_eq!(p.algo.ops.map(|o| o.wave), [1, 2, 3, 4, 5, 6]);
    turn(&reg::ALGO_LEVEL, 5, 40, &mut p);
    assert_eq!(p.algo.ops[5].level, 40);
    turn(&reg::ALGO_LEVEL, 0, 5, &mut p);
    assert_eq!(p.algo.ops[0].level, 99, "clamped");
    turn(&reg::ALGO_ALG, 0, 21, &mut p);
    turn(&reg::ALGO_ALG, 1, 29, &mut p);
    turn(&reg::ALGO_ALG, 2, 64, &mut p);
    turn(&reg::ALGO_ALG, 3, -30, &mut p);
    assert_eq!(
        (p.algo.alg_a, p.algo.alg_b, p.algo.morph, p.algo.transpose),
        (21, 29, 64, -24)
    );
    snap(&reg::ALGO_ALG, 2, 1, &mut p); // MIX + turn snaps MORPH like any Uni value
    assert_eq!(p.algo.morph, 100);
}
```

`chimera-core/tests/common/mod.rs`: add `AlgoInit` to `Case` (doc: `/// The Algo init Sound: operator 1 on W1 at LEVEL 99, T1.`), to `Case::ALL` (append; the array becomes `[Case; 11]`), `name()` → `"algo_init"`, `setup()` → `Case::AlgoInit => (init_params(EngineType::Algo), ModState::new()),`, `init_params` → `EngineType::Algo => Sound::init(ChainType::Algo).params,`, and `expects_sound` → add `| EngineType::Algo` to the `true` arm.

`chimera-core/tests/sanity_test.rs`, append:

```rust
#[test]
fn algo_is_finite_bounded_audible() {
    assert_finite_bounded_audible(Case::AlgoInit);
}
#[test]
fn algo_is_silent_after_note_off() {
    assert_silent_after_note_off(Case::AlgoInit);
}
#[test]
fn algo_is_pitched() {
    assert_pitched(Case::AlgoInit);
}

/// Spec § Testing, the ADR 0011 gate: the init patch plays A4 at 440 Hz
/// within one cent through the whole voice.
#[test]
fn algo_plays_a4_within_a_cent() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::modulation::ModState;
    use chimera_core::{MidiNote, Velocity};
    let params = init_params(EngineType::Algo);
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(MidiNote::A4, Velocity::DEFAULT, &params);
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK_SIZE];
    for _ in 0..750 {
        voice.render(&mut block, &params, &ModState::new());
        out.extend_from_slice(&block);
    }
    let s = &out[4800..];
    let ups: Vec<f64> = (1..s.len())
        .filter(|&i| s[i - 1] < 0.0 && s[i] >= 0.0)
        .map(|i| (i - 1) as f64 + (-s[i - 1] as f64) / ((s[i] - s[i - 1]) as f64))
        .collect();
    let hz = (ups.len() - 1) as f64 * SR as f64 / (ups[ups.len() - 1] - ups[0]);
    let cents = 1200.0 * (hz / 440.0).log2();
    assert!(cents.abs() < 1.0, "{hz} Hz, {cents:+.3} cents");
}
```

Run: `cargo test -p chimera-core --test addr_test --test algo_engine_test --test engines_test --test sanity_test`
Expected: FAIL to compile, "no variant named `Algo` found for enum `BlockRef`".

- [ ] **Step 2: `Op` A–F and the Algo blocks (`addr.rs`)**

```rust
/// An operator. `TryFrom<u8>` rejects values above 5, so an out-of-range
/// operator (bad sound or SysEx data) is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    A,
    B,
    C,
    D,
    E,
    F,
}
```

`Op::ALL` becomes `[Op::A, Op::B, Op::C, Op::D, Op::E, Op::F]` (`[Op; 6]`); in `nudged`, `.clamp(0, 3)` becomes `.clamp(0, Op::ALL.len() as i16 - 1)` and its doc says "clamped at A and F".

`BlockRef`: add after `FmOp(Op)`:

```rust
    /// The Algo engine's voice-level parameters (`AlgoParams`).
    Algo,
    /// One Algo operator (`AlgoOpParams`).
    AlgoOp(Op),
```

`BlockRef::ALL` becomes `[BlockRef; 26]`, with `BlockRef::Algo` and `BlockRef::AlgoOp(Op::A)` … `BlockRef::AlgoOp(Op::F)` inserted after `BlockRef::FmOp(Op::D)`. In `specs`, add

```rust
            BlockRef::Algo => &crate::dsp::algo::params::ALGO_SPECS,
            BlockRef::AlgoOp(_) => &crate::dsp::algo::params::ALGO_OP_SPECS,
```

and in `voice_reads` add `| BlockRef::Algo | BlockRef::AlgoOp(_)` to the `true` arm.

- [ ] **Step 3: The engine, the Sound and the Part (`params.rs`, `preset.rs`)**

`params.rs`:
- `EngineType`: add `Algo = 4,` after `Va = 3,`; `ALL` becomes `[EngineType; 5]` with `EngineType::Algo` appended.
- `ParamSnapshot`: add the field `pub algo: crate::dsp::algo::params::AlgoParams,` after `fm`, and `algo: crate::dsp::algo::params::AlgoParams::default(),` in `Default`.
- `Blocks for ParamSnapshot`: in `block`, replace the `FmOp` arm and add the Algo arms:

```rust
            BlockRef::FmOp(op) => {
                return self.fm.operators.get(op.index()).map(|o| o as &dyn Block);
            }
            BlockRef::Algo => &self.algo,
            BlockRef::AlgoOp(op) => &self.algo.ops[op.index()],
```

and in `block_mut`:

```rust
            BlockRef::FmOp(op) => {
                return self
                    .fm
                    .operators
                    .get_mut(op.index())
                    .map(|o| o as &mut dyn Block);
            }
            BlockRef::Algo => &mut self.algo,
            BlockRef::AlgoOp(op) => &mut self.algo.ops[op.index()],
```

(FM has four operators; `FmOp(E)` and `FmOp(F)` resolve to nothing until FM is deleted in Task 9.)

`preset.rs`:
- `ChainType`: add `Algo = 3,`; `ALL` becomes `[ChainType::PizzaPoly, ChainType::Modal, ChainType::Fm, ChainType::Algo]` (`[ChainType; 4]`); `label` → `ChainType::Algo => "Algo"`; `engine` → `ChainType::Algo => EngineType::Algo`.
- `Blocks for PartEdit`: add `| BlockRef::Algo | BlockRef::AlgoOp(_)` to the Sound arm of both `block` and `block_mut`.

- [ ] **Step 4: `AlgoLive::offset` (`dsp/algo/engine.rs`)**

Add `use crate::addr::{BlockRef, ParamAddr};` and extend the params import to `use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};`. In `impl AlgoLive`:

```rust
    /// Takes a mod offset aimed at MORPH or a LEVEL (`false` for any other
    /// destination, which the voice applies to its blocks as before).
    pub fn offset(&mut self, addr: ParamAddr, off: f32) -> bool {
        let slot = match (addr.block, addr.param) {
            (BlockRef::Algo, AlgoParams::MORPH) => &mut self.morph,
            (BlockRef::AlgoOp(op), AlgoOpParams::LEVEL) => &mut self.level[op.index()],
            _ => return false,
        };
        if let Some(s) = addr.spec() {
            *slot = (*slot + off * (s.max - s.min)).clamp(s.min, s.max);
        }
        true
    }
```

- [ ] **Step 5: `Engines` and `Voice`**

`dsp/engines.rs`: add `use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};`; the struct gains `algo: AlgoEngine,` (after `modal`) and the `field_list!` gains `algo`. In `init_in_place`, after the Modal line:

```rust
            AlgoEngine::init_in_place(uninit_at(addr_of_mut!((*p).algo)));
```

and the SAFETY comment reads "`p` is valid and unaliased; Modal (40 KB) and Algo are built in place, Pizza and FM (about 1.1 KB) by value, each field once." Match arms:
- `note_on`: `EngineType::Algo => self.algo.note_on(note, vel, &p.algo, self.sample_rate),`
- `note_off`: `EngineType::Algo => self.algo.note_off(),`
- `render` takes `live: &AlgoLive` as its last argument: `EngineType::Algo => self.algo.render(out, &p.algo, live, self.sample_rate),`
- `cost`: `EngineType::Algo => AlgoEngine::COST,`
- `uses_amp_env`: `EngineType::Modal | EngineType::Algo => false,` (doc: "Modal's modes decay naturally and Algo's operators carry their own envelopes, so these only get the volume.")
- `is_active`: `EngineType::Algo => self.algo.is_active(),`

`dsp/voice.rs`: import `use crate::dsp::algo::engine::AlgoLive;`. Replace the modulated-copy loop and the engine call:

```rust
        let mut m = params.clone();
        let mut live = AlgoLive::from_params(&params.algo);
        for d in 0..mod_state.num_dests() {
            let off = mod_state.sum_for(d, &mod_values);
            if off != 0.0 {
                let a = mod_state.dest(d);
                if live.offset(a, off) {
                    continue;
                }
                // Modulatable addresses are always Sound blocks (`voice_reads`).
                if let Some(blk) = m.block_mut(a.block) {
                    apply_offset(blk, a.param, off);
                }
            }
        }

        // 1. Engine → raw oscillator output
        self.engines.render(self.active_engine, output, &m, &live);
```

Replace step 5 (the VCA) with:

```rust
        // 5. VCA. The amp envelope runs even off the VCA: it is the ENV mod source.
        let volume = m.out.volume;
        let on_vca = Engines::uses_amp_env(self.active_engine);
        for sample in output.iter_mut() {
            let env = self.amp_env.process(&m.envelopes[0], sample_rate);
            *sample *= if on_vca { env * volume } else { volume };
        }
```

Pizza and FM multiply by `env * volume` exactly as before and Modal by `volume`, so their goldens stay bit-identical.

- [ ] **Step 6: The UI knows Algo**

`ui/mod_grid.rs`, `block_tag`: replace the four `BlockRef::FmOp(Op::X)` arms with

```rust
        BlockRef::FmOp(op) | BlockRef::AlgoOp(op) => {
            ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][op.index()]
        }
        BlockRef::Algo => "ALG",
```

`ui/mod.rs`, `mod_label`: change `BlockRef::FmOp(op) => {` to `BlockRef::FmOp(op) | BlockRef::AlgoOp(op) => {`.

`ui/block_def.rs`, `ParamSlot::format`: `SlotBinding::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),`.

`ui/part_page.rs`, `read_values`: `SlotBinding::SelectOp => sel_op.index() as f32 / (Op::ALL.len() - 1) as f32,`.

`ui/block_registry.rs`: add `use crate::block::ParamId;` and `use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};`, then (after the FM pages):

```rust
// ---------------------------------------------------------------------------
// Algo engine: group pages, one parameter across operators 1–6 (spec § UI)
// ---------------------------------------------------------------------------

const fn op_row(id: ParamId) -> [ParamSlot; 6] {
    [
        ParamSlot::param(BlockRef::AlgoOp(Op::A), id).with_label("OP1"),
        ParamSlot::param(BlockRef::AlgoOp(Op::B), id).with_label("OP2"),
        ParamSlot::param(BlockRef::AlgoOp(Op::C), id).with_label("OP3"),
        ParamSlot::param(BlockRef::AlgoOp(Op::D), id).with_label("OP4"),
        ParamSlot::param(BlockRef::AlgoOp(Op::E), id).with_label("OP5"),
        ParamSlot::param(BlockRef::AlgoOp(Op::F), id).with_label("OP6"),
    ]
}

const fn group(id: u16, name: &'static str, short: &'static str, param: ParamId) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::None,
        params: op_row(param),
    }
}

/// The OSC node's home page; its short name labels the node on the map.
pub static ALGO_WAVE: BlockDef = group(42, "Wave", "OSC", AlgoOpParams::WAVE);
pub static ALGO_LEVEL: BlockDef = group(44, "Level", "LVL", AlgoOpParams::LEVEL);

pub static ALGO_ALG: BlockDef = BlockDef {
    id: 43,
    name: "Algorithm",
    short: "ALG",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Algo, AlgoParams::ALG_A),
        ParamSlot::param(BlockRef::Algo, AlgoParams::ALG_B),
        ParamSlot::param(BlockRef::Algo, AlgoParams::MORPH),
        ParamSlot::param(BlockRef::Algo, AlgoParams::TRANSPOSE),
        ParamSlot::param(BlockRef::Out, OutParams::VOLUME).with_label("VOL"),
        EMPTY,
    ],
};
```

and with the chain templates:

```rust
static ALGO_OSC_SUB_PAGES: [&BlockDef; 1] = [&ALGO_LEVEL];

static ALGO_BLOCKS: [ChainBlock; 6] = [
    ChainBlock {
        def: &ALGO_WAVE,
        sub_pages: &ALGO_OSC_SUB_PAGES,
    },
    ChainBlock {
        def: &ALGO_ALG,
        sub_pages: &[],
    },
    ChainBlock {
        def: &DRIVE,
        sub_pages: &[],
    },
    ChainBlock {
        def: &FILTER,
        sub_pages: &[],
    },
    ChainBlock {
        def: &FOLDER,
        sub_pages: &[],
    },
    ChainBlock {
        def: &MOD_MATRIX,
        sub_pages: &MOD_MATRIX_SUB_PAGES,
    },
];

pub static ALGO_CHAIN: ChainDef2 = ChainDef2 {
    name: "Algo",
    blocks: &ALGO_BLOCKS,
    mod_sources: &PART_MOD_SOURCES,
};
```

`ui/chain.rs`, `chain_def_for`: add `ChainType::Algo => &block_registry::ALGO_CHAIN,`.

`ui/browser.rs`: `pub const INIT_TYPES: [ChainType; ChainType::ALL.len()] = ChainType::ALL;` (doc: "The pool's slots, then one init Sound per chain type, in `ChainType::ALL` order.").

`chimera-stm32/src/bench.rs`, `name`: add `EngineType::Algo => "ALGO",`.

- [ ] **Step 7: Bring the other exhaustive tests along**

- `engine_source_test.rs`, `chain_type_names_its_engine`: add `assert_eq!(ChainType::Algo.engine(), EngineType::Algo);`.
- `cost_test.rs`, `voice_costs_are_the_bench_measurements`: add `assert_eq!(Voice::cost(EngineType::Algo), Cost(570));` and to its doc: "Algo is an estimate (engine ≤ 350 plus the chain) until Task 13 measures the worst case." `budget_capacity_per_engine` covers Algo unchanged (6 voices fit).
- `modulatable_test.rs`, `recipe`: add `BlockRef::Algo | BlockRef::AlgoOp(_) => EngineType::Algo,` to the engine choice, and to the second `match`:

```rust
        BlockRef::Algo | BlockRef::AlgoOp(_) => {
            // Every operator heard at ALG A, a chain at ALG B, MORPH halfway.
            p.algo.alg_a = AlgoId::A1.get();
            p.algo.alg_b = AlgoId::A17.get();
            p.algo.morph = 64;
            for (i, op) in p.algo.ops.iter_mut().enumerate() {
                (op.level, op.coarse) = (80, [4, 8, 10, 13, 16, 19][i]);
            }
        }
```

  (import `chimera_core::dsp::algo::algorithms::AlgoId`), and change `assert_eq!(checked, 25)` to `assert_eq!(checked, 32)`. This is ADR 0010's "every modulatable parameter audibly changes the output" for MORPH and the six LEVELs (spec § Testing, sanity gate).
- `focus_test.rs`, `every_page_id_fits_the_focus_table`: the array becomes `[&ChainDef2; 10]` with `&reg::ALGO_CHAIN` added.
- `browser_test.rs`, `init_rows_end_the_list_and_load`: the last row is now `ChainType::Algo`; its comment and `init_rows_name_their_chain_in_a_distinct_shade`'s say "four INIT rows".

- [ ] **Step 8: Run the core tests; the sanity gate must pass**

Run: `cargo test -p chimera-core`
Expected: PASS except `golden_test::goldens_match` and `goldens_match_through_the_instrument`, which report `algo_init: no golden recorded`, and `screen_golden_test::screen_goldens_match` for `engine_fm_op` (the selector's gauge now spans six operators) and `sound_browser` (a fourth INIT row). The four `sanity_test::algo_*` tests pass. If any sanity test fails, stop and fix the engine before recording anything (ADR 0011).

- [ ] **Step 9: Record `algo_init`, the only new golden**

Run: `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test goldens_match -- --nocapture`
Check that every printed row except `algo_init` equals its `GOLDENS` entry, then add only the `algo_init` row to `GOLDENS` (after `pizza_to_modal_switch`) with the comment `// Recorded after the Algo sanity gate (ADR 0011).`. Add `Case::AlgoInit` to `harness_is_deterministic`'s list.

Run: `cargo test -p chimera-core --test golden_test`
Expected: PASS (`algo_init` also matches through the Instrument: 570 cycles fit the budget).

- [ ] **Step 10: Re-record the two screens this task changes**

Run: `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture`
Paste the printed rows for `engine_fm_op` and `sound_browser` only; every other printed row must equal its entry. Look at the two screens: `SCREEN_DUMP=$PWD/target/screens cargo test -p chimera-core --test screen_golden_test -q`, then `magick target/screens/sound_browser.ppm /tmp/sound_browser.png` and view it (four INIT rows, the last tagged `OSC`).

- [ ] **Step 11: The AXI check**

Run: `cargo test -p chimera-core --test memory_budget_test -- --nocapture`
Expected: PASS. `AXI` grows by about 4.7 KB (about 84 bytes of `AlgoParams` in each of the 56 `ParamSnapshot` copies: 32 pool slots, 6 Parts, 3 × 6 in `AudioShared`) from 513,516 B, still under 524,288 B; the build's `AXI_RESIDENT` assertion enforces it. `block_test::snapshot_is_small` stays under 512 B. Note both numbers for the commit's review.

- [ ] **Step 12: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 13: Commit**

```bash
git add chimera-core/src/addr.rs chimera-core/src/params.rs chimera-core/src/preset.rs chimera-core/src/dsp/engines.rs chimera-core/src/dsp/voice.rs chimera-core/src/dsp/algo/engine.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/part_page.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/chain.rs chimera-core/src/ui/browser.rs chimera-stm32/src/bench.rs chimera-core/tests/addr_test.rs chimera-core/tests/algo_engine_test.rs chimera-core/tests/engines_test.rs chimera-core/tests/engine_source_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/modulatable_test.rs chimera-core/tests/modulation_integration_test.rs chimera-core/tests/block_def_tests.rs chimera-core/tests/focus_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/browser_test.rs chimera-core/tests/common/mod.rs chimera-core/tests/sanity_test.rs chimera-core/tests/golden_test.rs chimera-core/tests/screen_golden_test.rs
git commit -m "Algo joins the engine set, with its sanity gate and init golden"
```

---

### Task 7: The chain tests move off Pizza

Tests that used Pizza as "a sounding source through the voice chain" move to the Algo engine. The replacement for Pizza's default triangle is operator 1 alone on `TRI`, which has the same odd-harmonic spectrum. Pizza's CRUSH, used as "make the timbre harsher", becomes operator 2 modulating operator 1 at LEVEL 70 (T1); its measured worst sample jump is 0.035 against `click_free_test`'s 0.15 threshold. Tests of Pizza's own parameters stay until Task 9 deletes them. Every test keeps its intent; only its source changes.

**Files:**
- Modify (all under `chimera-core/tests/`): `click_free_test.rs`, `stress_test.rs`, `reverb_test.rs`, `property_test.rs`, `signal_chain_test.rs`, `chain_spectral_test.rs`, `live_param_test.rs`, `desktop_sim_test.rs`, `modulation_integration_test.rs`, `engine_switch_test.rs`, `modal_integration_test.rs`, `audio_shared_test.rs`, `part_test.rs`, `preset_test.rs`, `browser_test.rs`

**Interfaces:**
- Consumes: `ParamSnapshot::for_engine(EngineType::Algo)`, `AlgoParams::single(WaveId::TRI)`, `ChainType::Algo`.
- Produces: nothing new in the library. Each file below that needs it gets this local helper (with `use chimera_core::dsp::algo::params::AlgoParams;` and `use chimera_core::dsp::algo::waves::WaveId;`):

```rust
/// Pizza's old role: operator 1 alone on the triangle.
fn tri() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::TRI);
    p
}
```

- [ ] **Step 1: Move each test**

| File | Test | Change (intent kept) |
|---|---|---|
| `click_free_test.rs` | `test_no_clicks_fm_init`, `test_no_clicks_pizza_with_plate_reverb`, `test_no_clicks_pizza_with_fdn_reverb`, `test_no_clicks_fm_with_midiverb`, `test_no_clicks_odd_buffer_sizes`, `test_no_clicks_tiny_buffers`, `test_no_clicks_single_sample_buffers` | `*p = ParamSnapshot::for_engine(EngineType::Pizza);` → `*p = tri();`; the name strings read `"Algo triangle"`, `"Algo + plate reverb"`, and so on; renames: `test_no_clicks_fm_init` → `test_no_clicks_algo_triangle`, `test_no_clicks_pizza_with_plate_reverb` → `test_no_clicks_algo_with_plate_reverb`, `test_no_clicks_pizza_with_fdn_reverb` → `test_no_clicks_algo_with_fdn_reverb`, `test_no_clicks_fm_with_midiverb` → `test_no_clicks_algo_with_midiverb` |
| | `test_no_clicks_pizza_with_crush` | rename `test_no_clicks_algo_with_pm`; setup `*p = tri(); p.algo.ops[1].level = 70;` |
| `stress_test.rs` | `stress_pizza_basic`, `stress_pizza_crushed`, `stress_pizza_full_with_chain` | rename `stress_algo_basic`, `stress_algo_pm`, `stress_algo_full_with_chain`; `tri()`; `p.pizza.crush = x` → `p.algo.ops[1].level = 70`; drop `p.pizza.shape` |
| | `stress_summary` | its five Pizza entries use `tri()` (the "crushed" one also sets `p.algo.ops[1].level = 70`); their labels become "Algo basic", "Algo PM", "+ Drive", "+ Filter LP4", "+ Wavefolder" |
| | (new) `stress_algo_worst_case` | see Step 2 |
| `reverb_test.rs` | the two tests at lines 418 and 463 | `ParamSnapshot::for_engine(EngineType::Pizza)` → `tri()` |
| `signal_chain_test.rs` | `test_voice_produces_sound`, `test_voice_filter_shapes_sound` | `ParamSnapshot::default()` → `tri()` |
| | `test_voice_output_bounded` | `let mut params = tri(); params.algo.ops[1].level = 70;` in place of `ParamSnapshot::default()` and `params.pizza.crush = 0.7` |
| `chain_spectral_test.rs` | `test_voice_filter_sweep_audible` | `ParamSnapshot::default()` → `tri()`; the comment "Pizza produces harmonics by default" → "the triangle's odd harmonics" |
| `live_param_test.rs` | `test_filter_cutoff_sweep_mid_note`, `test_filter_resonance_mid_note`, `test_drive_amount_mid_note`, `test_folder_mid_note`, `test_volume_mid_note` | `*p = ParamSnapshot::for_engine(EngineType::Pizza);` → `*p = tri();` |
| | (new) `test_algo_wave_change_mid_note`, `test_algo_level_change_mid_note` | see Step 2 |
| `desktop_sim_test.rs` | `test_desktop_fm_produces_sound`, `test_desktop_filter_affects_output`, `test_desktop_drive_affects_output`, `test_desktop_engine_switch`, `test_desktop_mid_note_filter_sweep` | `ParamSnapshot::for_engine(EngineType::Pizza)` → `tri()`; rename the first `test_desktop_algo_produces_sound` |
| | `test_desktop_fm_modulation_works` | rename `test_desktop_algo_modulation_works`; `ui.params_mut().pizza.crush = 0.7;` → `ui.params_mut().algo.ops[1].level = 70;` |
| `modulation_integration_test.rs` | `voice_render_with_empty_mod_state`, `voice_render_with_mod_offset_changes_filter` | `ParamSnapshot::default()` → `tri()` |
| `engine_switch_test.rs` | `test_pizza_and_modal_produce_different_output` | rename `test_algo_and_modal_produce_different_output`; `render_voice(EngineType::Algo, 60, 16)`; locals `algo_buf`, `algo_rms` |
| | `test_engine_type_is_respected` | "Start with Pizza" → `ParamSnapshot::for_engine(EngineType::Algo)` |
| | `test_pizza_sustains_while_modal_decays` | rename `test_algo_sustains_while_modal_decays`; `render_voice(EngineType::Algo, 60, 64)` (the init patch holds at D1L 15 until note-off) |
| `modal_integration_test.rs` | `test_modal_different_from_pizza_through_voice` | rename `test_modal_different_from_algo_through_voice`; `render(EngineType::Algo)`; locals `algo`, `algo_max` |
| `audio_shared_test.rs` | `snapshot_copies_every_part_and_the_fx` | `load_init(ChainType::Fm)` → `load_init(ChainType::Algo)` |
| `part_test.rs` | `loading_a_sound_keeps_the_mix` | `Part::new(ChainType::PizzaPoly)` → `Part::new(ChainType::Modal)`; `load_init(ChainType::Fm)` and its assertion → `ChainType::Algo` |
| `preset_test.rs` | `patch_init_has_musically_useful_defaults`, `sound_pool_store_and_retrieve`, `part_starts_with_init_patch`, `part_load_from_pool_copies`, `part_edit_does_not_modify_pool`, `part_save_to_pool_overwrites`, and the Sound built at line 251 | `ChainType::PizzaPoly` → `ChainType::Algo` (Sound and pool mechanics; the UI tests move in Task 8) |
| `browser_test.rs` | `empty_slots_are_dimmed_and_saved_ones_bright`, `the_longest_sound_name_is_not_truncated` | `Sound::init(ChainType::Fm)` → `Sound::init(ChainType::Algo)` |
| `property_test.rs` | `prop_filter_cutoff_full_sweep`, `prop_filter_resonance_full_sweep`, `prop_drive_full_sweep`, `prop_folder_full_sweep`, `prop_volume_full_sweep` | `*p = ParamSnapshot::for_engine(EngineType::Pizza);` → `*p = tri();` |
| | `random_params` | add the Algo fields (Step 2); the Pizza lines stay until Task 9 |

- [ ] **Step 2: The new tests and the Algo fields in `random_params`**

`stress_test.rs` (import `chimera_core::dsp::algo::algorithms::AlgoId`):

```rust
#[test]
fn stress_algo_worst_case() {
    let t = bench_render("Algo worst case (6 ops, morph)", |p| {
        *p = ParamSnapshot::for_engine(EngineType::Algo);
        (p.algo.alg_a, p.algo.alg_b, p.algo.morph) = (AlgoId::A14.get(), AlgoId::A22.get(), 64);
        for (i, op) in p.algo.ops.iter_mut().enumerate() {
            (op.wave, op.coarse, op.level, op.feedback) = (i as u8, [4, 8, 10, 13, 16, 19][i], 99, 7);
        }
    });
    assert!(t < 5000.0, "Algo worst case too slow: {} us/block", t);
}
```

`live_param_test.rs` (it already has `render_with_param_change` and `harmonic_energy`):

```rust
#[test]
fn test_algo_wave_change_mid_note() {
    let (_, _, before, after) = render_with_param_change(
        |p| *p = ParamSnapshot::for_engine(EngineType::Algo),
        |p| p.algo.ops[0].wave = WaveId::SAW.get(),
        8,
    );
    assert!(
        after > before * 2.0,
        "a saw has more harmonics than a sine: before {before}, after {after}"
    );
}

#[test]
fn test_algo_level_change_mid_note() {
    let (before_rms, after_rms, _, _) = render_with_param_change(
        |p| *p = ParamSnapshot::for_engine(EngineType::Algo),
        |p| p.algo.ops[0].level = 60,
        8,
    );
    assert!(after_rms < before_rms * 0.1, "{before_rms} → {after_rms}");
}
```

(`render_with_param_change`'s third argument is the number of blocks before the change, as in the existing tests; the WAVE change ducks for one block and returns, well inside the measured window. LEVEL 60 is 29 dB down.)

`property_test.rs`, `random_params`, after the Modal block:

```rust
    // Algo params (release 8–15 so a note ends inside the note-off property's window)
    p.algo.alg_a = rng.u8(31);
    p.algo.alg_b = rng.u8(31);
    p.algo.morph = rng.u8(127);
    for op in p.algo.ops.iter_mut() {
        op.wave = rng.u8(15);
        op.coarse = rng.u8(63);
        op.fine = rng.u8(15);
        op.level = rng.u8(99);
        op.feedback = rng.u8(7);
        op.ar = 20 + rng.u8(11);
        op.d1r = rng.u8(31);
        op.d1l = rng.u8(15);
        op.d2r = rng.u8(31);
        op.rr = 8 + rng.u8(7);
    }
```

- [ ] **Step 3: Run the moved tests**

Run: `cargo test -p chimera-core --test click_free_test --test stress_test --test reverb_test --test property_test --test signal_chain_test --test chain_spectral_test --test live_param_test --test desktop_sim_test --test modulation_integration_test --test engine_switch_test --test modal_integration_test --test audio_shared_test --test part_test --test preset_test --test browser_test`
Expected: PASS.

Run: `rg -n "EngineType::Pizza|ChainType::PizzaPoly|ChainType::Fm" chimera-core/tests/click_free_test.rs chimera-core/tests/stress_test.rs chimera-core/tests/reverb_test.rs chimera-core/tests/signal_chain_test.rs chimera-core/tests/chain_spectral_test.rs chimera-core/tests/desktop_sim_test.rs chimera-core/tests/modulation_integration_test.rs chimera-core/tests/engine_switch_test.rs chimera-core/tests/modal_integration_test.rs chimera-core/tests/audio_shared_test.rs chimera-core/tests/part_test.rs`
Expected: no output.

- [ ] **Step 4: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/tests/click_free_test.rs chimera-core/tests/stress_test.rs chimera-core/tests/reverb_test.rs chimera-core/tests/property_test.rs chimera-core/tests/signal_chain_test.rs chimera-core/tests/chain_spectral_test.rs chimera-core/tests/live_param_test.rs chimera-core/tests/desktop_sim_test.rs chimera-core/tests/modulation_integration_test.rs chimera-core/tests/engine_switch_test.rs chimera-core/tests/modal_integration_test.rs chimera-core/tests/audio_shared_test.rs chimera-core/tests/part_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/browser_test.rs
git commit -m "Chain tests play the Algo triangle instead of Pizza"
```

---

### Task 8: The UI default moves to Algo

The default engine, chain and Part become Algo. The Algo chain has one more node than Pizza's (`OSC · ALG · DRV · FLT · FLD · MOD`), and its home page (WAVE) has no modulatable slot, so the UI tests that counted nodes or primed slot A of the home page now go to the node or page they mean. The instrument and screen goldens whose default Sound changes are re-recorded deliberately; the Algo sanity gate passed in Task 6.

**Files:**
- Modify: `chimera-core/src/params.rs` (`EngineType` default), `chimera-core/src/preset.rs` (`ChainType` default, `Performance::new`), `chimera-core/src/ui/chain.rs` (`ChainNav::new`), `chimera-core/src/dsp/voice.rs` (`init_in_place`)
- Test (modify): `screen/mod.rs`, `screen_golden_test.rs`, `cell_grid_test.rs`, `prime_status_test.rs`, `ui_routing_test.rs`, `focus_test.rs`, `header_map_test.rs`, `ui_test.rs`, `preset_test.rs`, `matrix_view_test.rs`, `all_pages_walk_test.rs`, `instrument_test.rs` (all under `chimera-core/tests/`)

**Interfaces:**
- Consumes: `ALGO_CHAIN`, `ALGO_WAVE`, `ALGO_ALG`, `ALGO_LEVEL` (Task 6).
- Produces: `EngineType::default() == Algo`, `ChainType::default() == Algo`, `Performance::new()` Parts on `ChainType::Algo`, `ChainNav::new().chain_type == ChainType::Algo`; test helper `screen::to_level_page(&mut UiState)`.

- [ ] **Step 1: Flip the defaults**

- `params.rs`: move `#[default]` from `Pizza` to `Algo`.
- `preset.rs`: move `#[default]` from `PizzaPoly` to `Algo`; in `Performance::new`, `..Part::new(ChainType::PizzaPoly)` → `..Part::new(ChainType::Algo)`.
- `ui/chain.rs`, `ChainNav::new`: `chain_type: ChainType::Algo,`.
- `dsp/voice.rs`, `init_in_place`: `addr_of_mut!((*p).active_engine).write(EngineType::Algo);`.

Run: `cargo test -p chimera-core 2>&1 | rg "^test .* FAILED|panicked" | head -60`
Expected: the UI and instrument tests listed below fail; nothing else.

- [ ] **Step 2: The screen harness and its cases**

`tests/screen/mod.rs`, add:

```rust
/// From Part 1's home (the OSC node), EDIT down to the LEVEL sub-page.
pub fn to_level_page(ui: &mut UiState) {
    use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_LEVEL};
    let subs = ALGO_CHAIN.blocks[0].sub_pages;
    let n = subs.iter().position(|d| d.id == ALGO_LEVEL.id).expect("LEVEL is an OSC sub-page") + 1;
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Edit));
    }
}
```

In `CASES`:
- rename `"engine_pizza"` to `"engine_algo"` (same input: encoder A +2, operator 1's WAVE to W3);
- `"bigviz_filter"`: `plus(ui, 2)` → `plus(ui, 3)`;
- `"bigviz_env"`: `plus(ui, 4)` → `plus(ui, 5)`;
- `"mod_matrix"`: the first `plus(ui, 2)` → `plus(ui, 3)`;
- `"sound_browser"`: `Sound::init(ChainType::Fm)` → `Sound::init(ChainType::Algo)`.

`screen_golden_test.rs`: rename the `engine_pizza` entry to `engine_algo`.

`cell_grid_test.rs`: every `"engine_pizza"` → `"engine_algo"`. In `focus_band_shows_the_last_touched_slot`, turn `EncoderId::C` by `5` (not `-5`) and build the expected band from the slot itself:

```rust
    let slot = &ui.nav.active_block_def().params[2];
    let v = ui.renderer.anim[2].current();
    let mut text = FmtBuf::new();
    fmt_val(&mut text, v, slot.format());
    let mut want = Fb::new();
    want.px.fill(fb.px[0]); // ground
    components::focus_band(&mut want, slot.label(), text.as_str(), v, slot.format().is_bipolar(), None);
```

In `a_turn_redraws_focus_and_cells_only`, turn `EncoderId::C` by `3` (WAVE 0 cannot go down).

- [ ] **Step 3: Priming tests go to a page that has something to prime**

`prime_status_test.rs` (import `screen::to_level_page` via the existing `use screen::*;`):
- `mix_plus_on_a_fresh_modulatable_param_reports_added`, `mix_plus_on_an_already_routed_param_reports_already_routed`, `an_encoder_turn_clears_the_status`, `a_page_change_clears_the_status`, `un_priming_also_clears_the_status`, `dirty_render_with_a_status_message_equals_full_render`, `focus_band_status_clearing_redraws_through_the_dirty_regions`: after `let mut ui = UiState::new();` add `to_level_page(&mut ui);` (slot A is operator 1's LEVEL; the comment "Pizza page: slot A is SHAPE" becomes "LEVEL page: slot A is operator 1's LEVEL").
- `mix_plus_on_a_non_modulatable_param_reports_not_modulatable`: `for _ in 0..4` → `for _ in 0..5` (MOD is node 5).
- `filter_page`: three `Plus` presses; its doc "Pizza → Drive → Filter, CUTOFF focused." becomes "OSC → ALG → DRV → FLT, CUTOFF focused.".

`ui_routing_test.rs`: the tests that primed Pizza's SHAPE, CRUSH and LEVEL now prime DRIVE, TONE and MIX on the Drive page (three modulatable slots, like before). Add

```rust
use chimera_core::params::DriveParams;

/// Plus ×2 from a Part's home reaches the Drive page (DRIVE, TONE, MIX).
fn to_drive(ui: &mut UiState) {
    for _ in 0..2 {
        press(ui, ButtonId::Plus);
    }
}
```

and change:
- `enter_matrix` / `leave_matrix`: from the Drive page, `for _ in 0..3` (DRV → FLT → FLD → MOD and back). Doc: "Plus ×3 from the Drive page reaches the MOD node; Minus ×3 returns."
- `set_first_amount`: `for _ in 0..5` and its doc "Plus ×5 to the MOD node".
- `priming_on_a_part_page_registers_its_address`:

```rust
    let mut ui = UiState::new();
    to_drive(&mut ui);
    prime_slot_0(&mut ui);
    assert_eq!(primed(&ui), [ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE)]);
    let reg = &ui.performance.parts[0].sound.dest_registry;
    assert_eq!(reg.get(0).unwrap().label_str(), "DRVDRIVE");
    assert_eq!(ui.mod_state().num_dests(), 1);
```

- `priming_a_non_modulatable_param_is_refused`: `for _ in 0..5`.
- `switching_part_rebuilds_the_matrix_for_that_part`: `let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);` replaces `shape` throughout; Part 1 primes with `to_drive(&mut ui); prime_slot_0(&mut ui); press(&mut ui, ButtonId::B1);` (back home) before `set_first_amount(&mut ui, 10)`; after `press(&mut ui, ButtonId::B2)` and its assertions, Part 2 primes with `to_drive(&mut ui);` then the existing encoder-B turn and MIX+Plus (TONE), then `press(&mut ui, ButtonId::B2);` (home) before `set_first_amount(&mut ui, 20)`.
- `priming_after_a_stale_cursor_does_not_inherit_a_phantom_amount`: `shape` → `drive` (DRIVE), `level` → `let mix = ParamAddr::new(BlockRef::Drive, DriveParams::MIX);`; `to_drive(&mut ui);` before the three `prime_slot` calls; after `press(&mut ui, ButtonId::B2);` add `to_drive(&mut ui);` before `prime_slot(&mut ui, EncoderId::A)`; the final assertions name `drive` and `mix`; comments say DRIVE, TONE, MIX.
- `un_priming_keeps_the_other_routes_own_amounts`: `shape` → `drive`, `level` → `mix`; `to_drive(&mut ui);` before the three `prime_slot` calls; comments say DRIVE (A), TONE (B), MIX (C).
- Drop the `use chimera_core::dsp::pizza::PizzaParams;` line once nothing uses it (the FM tests below it stay until Task 9).

`preset_test.rs`:
- `performance_has_six_parts_playing_sounds`: `ChainType::PizzaPoly` → `ChainType::Algo`.
- `priming_on_main_page_registers_focused_param`: three `Plus` presses ("node 3: Filter").
- `priming_on_pizza_lfo_sub_page_registers_nothing`: rename `priming_on_the_lfo_sub_page_registers_nothing`, `for _ in 0..5`, comment "(node 5, sub-page 2)".

`focus_test.rs`:
- `focus_is_remembered_per_page`: comments "Wave: C", "→ Algorithm", "Algorithm: B", "← Wave"; the assertions hold as written.
- `mixer_part_and_matrix_pages_use_the_same_mechanism`: `for _ in 0..5`, comment "Part 1 chain, Algo".
- `an_empty_slot_does_not_take_focus`: the empty slot is now the Algorithm page's F. Replace `load_init(&mut ui, ChainType::Fm);` and the next line with `feed(&mut ui, Input::press(ButtonId::Plus)); // Algorithm page` and `feed(&mut ui, Input::turn(EncoderId::C, 1)); // MORPH`; replace `feed(&mut ui, Input::turn(EncoderId::B, 3)); // slot b: empty` with `feed(&mut ui, Input::turn(EncoderId::F, 3)); // slot f: empty`; the doc says "(Algorithm page slot f)"; drop the now-unused `ChainType`/`load_init` imports.

`matrix_view_test.rs`, `an_empty_matrix_says_so`: `for _ in 0..5`.

`header_map_test.rs`:
- `header_names_context_and_page`: `("PART 1".into(), "WAVE".into())`; `nav.node = 3;` for `"FILTER"`.
- `header_dot_shows_only_while_sounding_and_stays_in_the_band`, `header_shows_audio_load_in_warning_colours`: the literal `"PIZZA"` → `"WAVE"`.
- `current_block_is_an_accent_pill_others_are_rings`: `nav.node = 3; // FLT of OSC ALG DRV FLT FLD MOD`, `let (pill, other) = (node_x(3, 6), node_x(0, 6));`.
- `sub_pages_hang_under_the_pill_with_the_current_one_lit`: `nav.node = 5; // MOD: MOD, ENV, LFO`, `let x = node_x(5, 6) - 8;`.

`ui_test.rs`:
- `test_chain_nav_starts_at_part0_engine`: `part(&reg::ALGO_WAVE)`.
- `test_page_from_nav_part_chain`: the node list becomes `[(0, &reg::ALGO_WAVE), (1, &reg::ALGO_ALG), (2, &reg::DRIVE), (3, &reg::FILTER), (4, &reg::FOLDER), (5, &reg::MOD_MATRIX)]` (the loop leaves `nav.node == 5`, so the sub-page assertions stay).

`all_pages_walk_test.rs`, `representative_pages_walk`: `Context::Part(ChainType::Fm)` → `Context::Part(ChainType::Algo)`.

`instrument_test.rs`: `tails_ring_out_then_free_the_voice`'s comment "Pizza, release 0.3 s" → "Algo init, RR 8"; in `retriggering_a_releasing_mono_voice_does_not_free_the_new_note` rename the local `pizza` to `init`.

- [ ] **Step 4: Run the UI tests**

Run: `cargo test -p chimera-core`
Expected: PASS except `screen_golden_test::screen_goldens_match` (`engine_algo`, `bigviz_filter`, `bigviz_env`, `mod_matrix`, `sound_browser`) and `instrument_test::instrument_goldens_match` (all four: the default Sound is now Algo).

- [ ] **Step 5: Re-record the goldens whose default Sound changed**

Run: `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture`
Paste the rows for `engine_algo`, `bigviz_filter`, `bigviz_env`, `mod_matrix` and `sound_browser`; every other printed row must equal its entry. Dump them (`SCREEN_DUMP=$PWD/target/screens cargo test -p chimera-core --test screen_golden_test -q`) and look at each: the map reads OSC ALG DRV FLT FLD MOD.

Run: `GOLDEN_RECORD=1 cargo test -p chimera-core --test instrument_test instrument_goldens_match -- --nocapture`
Paste the four rows; mark `poly_chord` and `reverb_send_*` with `// re-recorded: the default Sound is Algo` and `two_parts_two_pairs` with `// re-recorded: part 1 is Algo`.

- [ ] **Step 6: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/params.rs chimera-core/src/preset.rs chimera-core/src/ui/chain.rs chimera-core/src/dsp/voice.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs chimera-core/tests/cell_grid_test.rs chimera-core/tests/prime_status_test.rs chimera-core/tests/ui_routing_test.rs chimera-core/tests/focus_test.rs chimera-core/tests/header_map_test.rs chimera-core/tests/ui_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/matrix_view_test.rs chimera-core/tests/all_pages_walk_test.rs chimera-core/tests/instrument_test.rs
git commit -m "Algo is the default engine"
```

---

### Task 9: Pizza, FM and VA are deleted

`EngineType` and `ChainType` become `{Algo, Modal}` (spec § Replacing the old engines). The engines, their parameter blocks, pages, viz, goldens and tests go. None of the ported FM code survives. With no engine putting the amp envelope on the VCA, its parameters stop being modulation destinations (ADR 0010: a destination must audibly change the output on its own); the envelope still shapes the ENV source. The FM operator pages' `SelectOp` machinery stays, pointed at `AlgoOp`, for sub-project 4's per-operator pages.

**Files:**
- Delete: `chimera-core/src/dsp/pizza.rs`, `chimera-core/src/dsp/engine_fm.rs`, `chimera-core/src/dsp/envelope_fm.rs`, `chimera-core/src/dsp/fm_tables.rs`, `chimera-core/src/dsp/fm_waveform.rs`, `chimera-core/src/dsp/oscillator.rs`
- Delete: `chimera-core/tests/fm_test.rs`, `chimera-core/tests/fm_viz_test.rs`, `chimera-core/tests/oscillator_test.rs`
- Modify: `chimera-core/src/dsp/mod.rs`, `chimera-core/src/dsp/engines.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/params.rs`, `chimera-core/src/addr.rs`, `chimera-core/src/preset.rs`, `chimera-core/src/modulation.rs`
- Modify: `chimera-core/src/ui/block_def.rs`, `chimera-core/src/ui/block_registry.rs`, `chimera-core/src/ui/chain.rs`, `chimera-core/src/ui/mod_grid.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/page.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/viz.rs`
- Modify: `chimera-stm32/src/bench.rs`
- Test (modify): `golden_test.rs`, `common/mod.rs`, `sanity_test.rs`, `engines_test.rs`, `engine_source_test.rs`, `cost_test.rs`, `addr_test.rs`, `mod_registry_test.rs`, `modulation_test.rs`, `modulatable_test.rs`, `page_block_test.rs`, `part_page_test.rs`, `ui_routing_test.rs`, `prime_status_test.rs`, `focus_test.rs`, `block_def_tests.rs`, `browser_test.rs`, `preset_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`, `big_viz_test.rs`, `property_test.rs`, `live_param_test.rs`, `in_place_test.rs`, `instrument_test.rs`, `block_test.rs`

**Interfaces:**
- Consumes: everything from Tasks 5–8.
- Produces:
  - `EngineType { Algo = 0, Modal = 1 }` (default `Algo`), `EngineType::ALL: [EngineType; 2]`; `ChainType { Algo = 0, Modal = 1 }` (default `Algo`), `ChainType::ALL: [ChainType; 2]`.
  - `BlockRef::ALL: [BlockRef; 20]` = `Modal, Algo, AlgoOp(A..=F), Drive, Filter, Folder, AmpEnv, FilterEnv, AuxEnv, Lfo, Out, Chorus, Delay, Reverb, Part`; `BlockRef::AmpEnv.voice_reads() == false`.
  - `Engines::is_active(&self, kind: EngineType) -> bool` (the amp-envelope argument is gone).
  - `SlotBinding::SelectedOp` resolves to `BlockRef::AlgoOp(sel_op)`.
  - `VizType` without `AlgorithmDiagram` and `FmEnvelope` (Task 11 adds `AlgoDiagram`).

- [ ] **Step 1: Delete the engines and their tests**

```bash
git rm chimera-core/src/dsp/pizza.rs chimera-core/src/dsp/engine_fm.rs chimera-core/src/dsp/envelope_fm.rs chimera-core/src/dsp/fm_tables.rs chimera-core/src/dsp/fm_waveform.rs chimera-core/src/dsp/oscillator.rs chimera-core/tests/fm_test.rs chimera-core/tests/fm_viz_test.rs chimera-core/tests/oscillator_test.rs
```

`fm_test.rs`'s intents live on in Tasks 1–5: ratios, FINE and LEVEL (`algo_tx_test`), the envelope (`algo_env_test`), waves (`algo_waves_test`), operators and feedback (`algo_kernel_test`), the engine and its release (`algo_engine_test`), and ALG 4's routing (`algo_algorithms_test::t4_operator_2_is_not_modulated_by_operator_3`). `fm_viz_test.rs`'s intents (diagram in the band, no overlap, edges downward) return in Task 11's `alg_layout_test.rs`. `oscillator_test.rs` tested `SineOsc`, which only VA was to use.

- [ ] **Step 2: The library**

`dsp/mod.rs`: remove `pub mod engine_fm;`, `pub mod envelope_fm;`, `pub mod fm_tables;`, `pub mod fm_waveform;`, `pub mod oscillator;`, `pub mod pizza;`.

`dsp/engines.rs`:

```rust
//! Persistent engine instances with dispatch in one place (spec §3).
//!
//! Engines are never constructed in the audio interrupt: `ModalEngine` is
//! ~40 KB and the stack has no guard. Adding an engine = one field here plus
//! one arm in each exhaustive `match` below; the compiler lists them.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::modal::ModalEngine;
use crate::hw::Cost;
use crate::in_place::{by_value, uninit_at};
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

pub struct Engines {
    algo: AlgoEngine,
    modal: ModalEngine,
    sample_rate: u32,
}

crate::in_place::field_list!(Engines => Engines { algo, modal, sample_rate });

impl Engines {
    pub fn new(sample_rate: u32) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate)) }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; both engines are built in
        // place and the sample rate written once, before `assume_init_mut`.
        unsafe {
            AlgoEngine::init_in_place(uninit_at(addr_of_mut!((*p).algo)));
            ModalEngine::init_in_place(uninit_at(addr_of_mut!((*p).modal)));
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            slot.assume_init_mut()
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn note_on(&mut self, kind: EngineType, note: MidiNote, vel: Velocity, p: &ParamSnapshot) {
        match kind {
            EngineType::Algo => self.algo.note_on(note, vel, &p.algo, self.sample_rate),
            EngineType::Modal => {
                self.modal
                    .note_on(note.get(), vel.get(), &p.modal, self.sample_rate)
            }
        }
    }

    pub fn note_off(&mut self, kind: EngineType) {
        match kind {
            EngineType::Algo => self.algo.note_off(),
            EngineType::Modal => self.modal.note_off(),
        }
    }

    /// Render one block of raw engine output from (possibly modulated) params.
    pub fn render(
        &mut self,
        kind: EngineType,
        out: &mut [f32; BLOCK_SIZE],
        p: &ParamSnapshot,
        live: &AlgoLive,
    ) {
        match kind {
            EngineType::Algo => self.algo.render(out, &p.algo, live, self.sample_rate),
            EngineType::Modal => self.modal.render(out, &p.modal, self.sample_rate),
        }
    }

    /// Cycles/sample of one engine instance (ADR 0013).
    pub const fn cost(kind: EngineType) -> Cost {
        match kind {
            EngineType::Algo => AlgoEngine::COST,
            EngineType::Modal => ModalEngine::COST,
        }
    }

    /// VCA choice: does the amp envelope shape this engine's output? Modal's
    /// modes decay naturally and Algo's operators carry their own
    /// envelopes, so both only get the volume.
    pub fn uses_amp_env(kind: EngineType) -> bool {
        match kind {
            EngineType::Algo | EngineType::Modal => false,
        }
    }

    /// Voice lifetime: is this engine still sounding?
    pub fn is_active(&self, kind: EngineType) -> bool {
        match kind {
            EngineType::Algo => self.algo.is_active(),
            EngineType::Modal => self.modal.is_active(),
        }
    }
}
```

`dsp/voice.rs`: `self.active = self.engines.is_active(self.active_engine);`; the `CHAIN_COST` doc becomes "Measured with the silent VA placeholder, which went inactive after one block, so this is the chain's floor rather than its cost with a sounding engine; engine `COST`s are bench per-voice minus this."

`params.rs`: delete `FmOpParams`, `FM_OP_SPECS`, `FmParams`, `FM_SPECS` and their impls. `EngineType` becomes

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EngineType {
    #[default]
    Algo = 0,
    Modal = 1,
}

impl EngineType {
    /// Every engine. Tests iterate this; see `engines_test.rs` for the
    /// exhaustive-match guard that makes a new variant a compile error there.
    pub const ALL: [EngineType; 2] = [EngineType::Algo, EngineType::Modal];
}
```

`ParamSnapshot` loses `pizza` and `fm` (fields and `Default`); `Blocks for ParamSnapshot` loses the `Pizza`, `Fm` and `FmOp` arms.

`addr.rs`: `Op`'s doc starts "An operator of the Algo engine."; `BlockRef` loses `Pizza`, `Fm`, `FmOp`; `ALL` becomes the 20 listed under Interfaces; `specs` loses the three arms; `voice_reads`:

```rust
    /// Whether `Voice::render` reads this block from its modulated copy and
    /// hears it without a route of its own. The amp envelope only shapes the
    /// ENV source (no engine puts it on the VCA, ADR 0022); filter/aux
    /// envelopes are never read; the LFO is read unmodulated; FX run outside
    /// `Voice`.
    pub const fn voice_reads(self) -> bool {
        match self {
            BlockRef::Modal
            | BlockRef::Algo
            | BlockRef::AlgoOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::Out => true,
            BlockRef::AmpEnv
            | BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Part => false,
        }
    }
```

`preset.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChainType {
    #[default]
    Algo = 0,
    Modal = 1,
}

impl ChainType {
    pub const ALL: [ChainType; 2] = [ChainType::Algo, ChainType::Modal];

    /// Short display label for the chain type.
    pub fn label(self) -> &'static str {
        match self {
            ChainType::Algo => "Algo",
            ChainType::Modal => "Modal",
        }
    }

    /// The engine this chain plays (spec §6).
    pub const fn engine(self) -> EngineType {
        match self {
            ChainType::Algo => EngineType::Algo,
            ChainType::Modal => EngineType::Modal,
        }
    }
}
```

and `PartEdit`'s arms lose `Pizza`, `Fm`, `FmOp`.

`modulation.rs`: `use crate::dsp::algo::params::AlgoParams;` in place of the Pizza import, and `const UNUSED: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);`.

- [ ] **Step 3: The UI**

- `ui/block_def.rs`: `VizType` loses `AlgorithmDiagram` and `FmEnvelope`. `SlotBinding::SelectedOp`'s doc: "A param of the currently selected operator."; `SelectOp`'s: "The operator selector itself." In `ParamSlot::spec`, `SlotBinding::SelectedOp(id) => find_spec(BlockRef::AlgoOp(Op::A).specs(), id),` (comment: "All six operators share one spec table, so AlgoOp(Op::A) stands in."). In `slot_addr`, `SlotBinding::SelectedOp(id) => Some(ParamAddr::new(BlockRef::AlgoOp(sel_op), id)),`.
- `ui/block_registry.rs`: delete `PIZZA`, `VA`, `FM_ALG`, `FM_OP`, `FM_RATIO`, `FM_ENV1`–`FM_ENV4`, `PIZZA_BLOCK`, `PIZZA_POLY_BLOCKS`, `PIZZA_POLY_CHAIN`, `FM_SUB_PAGES`, `FM_MOD_MATRIX_SUB_PAGES`, `FM_BLOCKS`, `FM_CHAIN`, and the `PizzaParams`, `FmOpParams`, `FmParams` imports. Ids 1 and 4–7 and 23–26 are retired, not reused.
- `ui/chain.rs`: `chain_def_for` matches `ChainType::Algo => &block_registry::ALGO_CHAIN, ChainType::Modal => &block_registry::MODAL_PLUCK_CHAIN,`.
- `ui/mod_grid.rs`, `block_tag`: remove `BlockRef::Pizza => "PIZ"` and `BlockRef::Fm => "FM"`; the operator arm becomes `BlockRef::AlgoOp(op) => ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][op.index()],`.
- `ui/mod.rs`, `mod_label`: `BlockRef::AlgoOp(op) => {`; its doc "for FM operator params" → "for operator params".
- `ui/page.rs`: imports become `use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};` and `use crate::params::{DriveParams, EnvParams, FilterParams, FolderParams, OutParams};`, and

```rust
const DEMO_FM: [ParamAddr; 4] = [
    ParamAddr::new(BlockRef::Algo, AlgoParams::ALG_A),
    ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::FEEDBACK),
    ParamAddr::new(BlockRef::AlgoOp(Op::C), AlgoOpParams::FEEDBACK),
];
```

- `ui/renderer.rs`: delete the `VizType::FmEnvelope` arm of `draw_big_viz`, and the `VizType::AlgorithmDiagram` arms of `draw_band_viz` and `viz_inputs`.
- `ui/viz.rs`: delete the FM diagram (from the doc comment above `ALG_POS` through the end of `fm_algorithm`: `ALG_POS`, `ALG_EDGES`, `ALG_CARRIERS`, `ALG_STEP_X`, `ALG_STEP_Y`, `ALG_OP_R`, `alg_op_center`, `alg_edges`, `fm_algorithm`).
- `chimera-stm32/src/bench.rs`, `name`: two arms, `EngineType::Algo => "ALGO"` and `EngineType::Modal => "MODAL"`.

Run: `cargo build -p chimera-core && cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`
Expected: PASS (the library builds; tests come next).

- [ ] **Step 4: The tests**

| File | Test | Change (intent kept) |
|---|---|---|
| `common/mod.rs` | `Case` | drop `PizzaInit`, `PizzaLfoCutoff`, `FmInit`, `FmLfoCutoff`, `FmLfoOpALevel`, `FmInitPatchMod`, `VaInit`, `PizzaToModalSwitch` (and their `ALL`, `name`, `setup` arms); `Case::ALL: [Case; 3]` = `ModalInit, ModalLfoCutoff, AlgoInit`; `init_params` and `expects_sound` match `Algo` and `Modal` only; drop `OP_A_LEVEL` and the switch logic in both `render_case` functions (Task 12 brings a switch case back); drop the `FmOpParams` import |
| `golden_test.rs` | `GOLDENS` | drop the eight rows above |
| | `KNOWN_BROKEN` | keep `modal_init` and `modal_lfo_cutoff`; drop `pizza_to_modal_switch` and the four FM rows |
| | `goldens_match_through_the_instrument` | nothing is refused any more: drop `ISSUE_26` and the `refused_by_budget` branch; every case matches strictly; doc "part 1's mono bus through the voice pool matches every golden bit-for-bit" |
| | `fm_init_patch_has_no_prewire` | delete (the pre-wire it guarded went with FM; `Sound::init` building an empty `ModState` stays tested by `preset_test`) |
| | `harness_is_deterministic` | `[Case::AlgoInit, Case::ModalInit]` |
| | `modulated_cases_differ_from_unmodulated` | `[(Case::ModalLfoCutoff, Case::ModalInit)]` (Task 12 adds the Algo pairs) |
| `sanity_test.rs` | `pizza_*`, `fm_*`, `va_is_silent_placeholder` | delete (the Algo gate from Task 6 stays; Modal's is unchanged) |
| `engines_test.rs` | `pizza_row_amp_env_on_vca_and_lives_with_it`, `fm_row_amp_env_on_vca_and_lives_until_operators_idle`, `va_row_amp_env_on_vca_never_active_silent` | delete; the doc table keeps the Algo and Modal rows |
| | `modal_row_no_amp_env_and_lives_until_modes_decay`, `algo_row_no_amp_env_and_lives_until_its_carriers_release` | every `e.is_active(kind, &env)` becomes `e.is_active(kind)`; drop the envelopes they built and the `Envelope` import |
| | `all_lists_every_engine_once_in_order` | `EngineType::Modal as usize + 1` |
| `engine_source_test.rs` | `FM_ENGINE` const, `chain_type_names_its_engine` | `const ALGO_ENGINE: EngineType = ChainType::Algo.engine();`; assertions for `Algo` and `Modal` |
| `cost_test.rs` | both tests | only `Algo` (570) and `Modal` (380) rows; doc drops Pizza, FM and VA |
| `addr_test.rs` | `modulatable_addresses_are_exactly_the_spec_list` | drop Pizza's three, the four AmpEnv entries and the `FmOp` loop (17 remain); imports |
| | `block_mut_reaches_the_named_instance` | `p.block_mut(BlockRef::AlgoOp(Op::C)).unwrap().set(AlgoOpParams::LEVEL, 42.0); assert_eq!(p.algo.ops[2].level, 42);` |
| | `unknown_param_has_no_spec_and_is_not_modulatable` | `ParamAddr::new(BlockRef::Algo, ParamId(99))` |
| `mod_registry_test.rs` | `op_level` | `ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL)` |
| | `registry_refuses_non_modulatable` | the refused list: Modal EXCITE (note-on only), `(Algo, ALG_A)` (Enum), `(AlgoOp(A), WAVE)` (Enum), `(AlgoOp(A), AR)` (note-on only), `(AlgoOp(A), FEEDBACK)` (not a sub-project-1 destination), `(AmpEnv, ATTACK)` (off the VCA), Filter FM_AMOUNT (never read), FilterEnv ATTACK (never read), `(Algo, ParamId(99))` (no such param) |
| `modulation_test.rs` | `mod_state_offset_no_routes`, `mod_state_set_amount_ignores_out_of_range` | Pizza SHAPE → `ParamAddr::new(BlockRef::Drive, DriveParams::TONE)` |
| | `mod_state_sync_from_matrix` | `crush` → `let tone = ParamAddr::new(BlockRef::Drive, DriveParams::TONE);` |
| | `dest_and_sum_for_never_panic_out_of_range` | sentinel `ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH)` |
| | `matrix_dests_are_semantic` | `op_c` → `(AlgoOp(Op::C), AlgoOpParams::LEVEL)`; the unrouted one → `(AlgoOp(Op::D), AlgoOpParams::LEVEL)`; doc "(25)" counts become "(17)" |
| `modulatable_test.rs` | `recipe` | engine `Modal` for `BlockRef::Modal`, else `Algo`; the Algo arm from Task 6 stays; the other blocks get `p.algo = AlgoParams::single(WaveId::TRI);` before their own tweak; drop the FM arm; `checked == 17` |
| `page_block_test.rs` | `demo_pages_step_like_before` | `assert_eq!(p.algo.ops[2].feedback, 2);` |
| | `legacy_bindings_name_semantic_addresses` | `Some(ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK))` |
| `part_page_test.rs` | `pizza_page`, `fm_fixed_pages` | delete (the Algo pages test from Task 6 covers group pages) |
| | `fm_operator_page_follows_the_selection` | becomes `select_op_page_follows_the_selection` on a test-local page (Step 5) |
| | (new) `a_selected_op_slot_resolves_to_the_operator_selected_now` | Step 5; carries `ui_routing_test::selected_op_route_is_concrete`'s intent |
| `ui_routing_test.rs` | `selected_op_route_is_concrete` | delete (moved, above) |
| | `fm_operator_selector_updates_the_page_key` | delete: no chain page has a selector until sub-project 4; the page-key rule stays in `ui_test::test_page_from_nav_part_chain` |
| | `fm_matrix_rows_are_env_and_lfo` | rename `algo_matrix_rows_are_env_and_lfo`; scroll to `POOL_SIZE as i8` (the Algo INIT row); comment "(init) Algo" |
| `prime_status_test.rs` | `fm_env_page`, `the_status_shows_on_an_fm_envelope_page` | become `envelope_page` (Plus ×5 to MOD, EDIT to the ADSR sub-page, turn C) and `the_status_shows_on_the_envelope_page` (the status there is NOT MODULATABLE; any status must show on a BigViz page) |
| | `priming_past_matrix_capacity_on_the_fm_chain_reports_full` | Step 5 |
| `focus_test.rs` | `every_page_id_fits_the_focus_table` | drop `PIZZA_POLY_CHAIN` and `FM_CHAIN` (`[&ChainDef2; 8]`) |
| `block_def_tests.rs` | `pizza_poly_chain_has_5_blocks`, `fm_chain_has_5_blocks`, `fm_chain_resolves_sub_pages` | delete |
| | `chain_active_def_resolves` | `ALGO_CHAIN`: `(0, 0)` is `"Wave"`, `(3, 0)` is `"Filter"` |
| `browser_test.rs` | `init_rows_end_the_list_and_load` | the last row loads `ChainType::Modal`; comments "two INIT rows" |
| `preset_test.rs` | `browser_init_entries_set_chain_type` | the first INIT row (`POOL_SIZE`) is `ChainType::Algo`, the second (`POOL_SIZE + 1`) `ChainType::Modal`; drop the FM part |
| | `browser_load_keeps_part_mix` | row `POOL_SIZE + 1`, `ChainType::Modal` |
| | `priming_on_fm_ratio_slot_2_registers_nothing` | becomes `priming_a_wave_registers_nothing`: `UiState::new()` (WAVE page), turn C, prime, nothing registered |
| `screen/mod.rs`, `screen_golden_test.rs` | cases `engine_fm_alg`, `engine_fm_op`, `bigviz_fm_op_env` | delete (cases and rows) |
| `big_viz_test.rs` | `dirty_render_from_scratch_equals_full_render` | drop `"bigviz_fm_op_env"` |
| | `fm_envelope_is_reachable_and_lit` | delete |
| `property_test.rs` | `random_params` | drop the Pizza lines |
| | `prop_param_change_changes_output` | the last arm: `_ => params_b.algo.ops[0].level = 99 - params_a.algo.ops[0].level,` |
| | `prop_pizza_crush_full_sweep` | becomes `prop_algo_level_full_sweep` (Step 5) |
| `live_param_test.rs` | `test_pizza_shape_change_mid_note`, `test_pizza_crush_change_mid_note` | delete (Task 7's `test_algo_wave_change_mid_note` and `test_algo_level_change_mid_note` carry "an engine parameter changes the sound mid-note") |
| `in_place_test.rs` | `every_engine_and_every_effect` | part 0 `tri()` (the helper, as in Task 7), part 1 the Algo worst case of `stress_algo_worst_case`; the Modal parts stay |
| `instrument_test.rs` | `two_parts` doc | "Part 2 plays Modal out of pair 2." |
| `block_test.rs` | `fm_conforms`, `fm_settings_truncate_fractional_level` | delete |

- [ ] **Step 5: The rewritten tests**

`part_page_test.rs` (imports: `chimera_core::addr::{BlockRef, ParamAddr}`, `chimera_core::dsp::algo::params::AlgoOpParams`, `chimera_core::ui::block_def::{BlockDef, ParamSlot, VizType, slot_addr}`, `chimera_core::ui::page::PageLayout`):

```rust
/// The selector machinery sub-project 4's per-operator pages will use.
static OP_PAGE: BlockDef = BlockDef {
    id: 63,
    name: "Op",
    short: "OP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::select_op(),
        ParamSlot::selected_op(AlgoOpParams::LEVEL),
        ParamSlot::selected_op(AlgoOpParams::DETUNE),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

#[test]
fn select_op_page_follows_the_selection() {
    let mut p = ParamSnapshot::default();
    let mut op = Op::A;
    part_page::apply_encoder(&OP_PAGE, 0, 1, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&OP_PAGE, 0, 9, &mut p, &mut op);
    assert_eq!(op, Op::F);
    part_page::apply_encoder(&OP_PAGE, 0, -4, &mut p, &mut op);
    assert_eq!(op, Op::B);
    part_page::apply_encoder(&OP_PAGE, 1, 5, &mut p, &mut op);
    assert_eq!(p.algo.ops[1].level, 5);
    part_page::apply_encoder(&OP_PAGE, 2, -9, &mut p, &mut op);
    assert_eq!(p.algo.ops[1].detune, -3);
    assert_eq!(part_page::read_values(&OP_PAGE, &p, op)[0], 1.0 / 5.0);
    part_page::snap_encoder(&OP_PAGE, 1, 1, &mut p, op); // Int(99): the snap is the top
    assert_eq!(p.algo.ops[1].level, 99);
    part_page::snap_encoder(&OP_PAGE, 0, 1, &mut p, op); // selector: no snap
    assert_eq!(op, Op::B);
}

/// Review Focus 5 (spec §5): a route primed on a `SelectedOp` slot names
/// the operator selected at that moment.
#[test]
fn a_selected_op_slot_resolves_to_the_operator_selected_now() {
    let level = |op| Some(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    assert_eq!(slot_addr(&OP_PAGE, 1, Op::B), level(Op::B));
    assert_eq!(slot_addr(&OP_PAGE, 1, Op::F), level(Op::F));
    assert_eq!(slot_addr(&OP_PAGE, 0, Op::B), None);
}
```

`prime_status_test.rs`:

```rust
/// The Algo chain alone reaches 17 modulatable addresses (six LEVELs, MORPH,
/// VOL, Drive, Filter and Folder), one more than the matrix holds. Priming
/// them all through real input: exactly `MAX_MOD_DESTS` report ADDED, the
/// next distinct one reports MATRIX FULL, and the matrix holds every added one.
#[test]
fn priming_past_matrix_capacity_on_the_algo_chain_reports_full() {
    use chimera_core::modulation::MAX_MOD_DESTS;
    let mut ui = UiState::new();
    let mut seen = Vec::new();

    to_level_page(&mut ui);
    prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen); // six LEVELs
    feed(&mut ui, Input::press(ButtonId::Plus)); // Algorithm page
    prime_every_slot(&mut ui, &ALL_SLOTS[2..5], &mut seen); // MORPH, TRNSP (refused), VOL
    for _ in 0..3 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // Drive, Filter, Folder
        prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
    }

    let added = seen.iter().filter(|&&s| s == PrimeStatus::Added).count();
    assert_eq!(added, MAX_MOD_DESTS, "{seen:?}");
    let first_full = seen.iter().position(|&s| s == PrimeStatus::Full);
    let added_before = first_full.map(|i| {
        seen[..i]
            .iter()
            .filter(|&&s| s == PrimeStatus::Added)
            .count()
    });
    assert_eq!(
        added_before,
        Some(MAX_MOD_DESTS),
        "the 17th distinct address must report MATRIX FULL: {seen:?}"
    );
    assert_eq!(ui.matrix_state.num_dests, MAX_MOD_DESTS);
    assert_eq!(
        ui.performance.parts[0].sound.dest_registry.len(),
        MAX_MOD_DESTS
    );
}

/// The MOD node's ADSR sub-page (BigViz), slot C focused.
fn envelope_page() -> UiState {
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    settle(&mut ui);
    ui
}

#[test]
fn the_status_shows_on_the_envelope_page() {
    assert_status_line_shows(envelope_page(), "prime_status_envelope");
}
```

Delete `fm_env_page`, `the_status_shows_on_an_fm_envelope_page` and the `use chimera_core::preset::ChainType;` line.

`property_test.rs`:

```rust
#[test]
fn prop_algo_level_full_sweep() {
    verify_full_sweep(
        "Algo operator 1 LEVEL",
        |p| *p = ParamSnapshot::for_engine(EngineType::Algo),
        |p, v| p.algo.ops[0].level = (v * 99.0) as u8,
        16,
    );
}
```

`modulatable_test.rs` (imports: `AlgoId`, `AlgoParams`, `WaveId`):

```rust
/// A base sound in which `block` is audible: Modal's own engine, else Algo
/// (every operator heard at ALG A for its own parameters, the triangle for
/// the chain's).
fn recipe(block: BlockRef) -> ParamSnapshot {
    let engine = if block == BlockRef::Modal { EngineType::Modal } else { EngineType::Algo };
    let mut p = ParamSnapshot::for_engine(engine);
    p.lfo.rate = 5.0; // swings both ways within the render
    match block {
        BlockRef::Algo | BlockRef::AlgoOp(_) => {
            p.algo.alg_a = AlgoId::A1.get();
            p.algo.alg_b = AlgoId::A17.get();
            p.algo.morph = 64;
            for (i, op) in p.algo.ops.iter_mut().enumerate() {
                (op.level, op.coarse) = (80, [4, 8, 10, 13, 16, 19][i]);
            }
        }
        BlockRef::Modal => {}
        _ => p.algo = AlgoParams::single(WaveId::TRI),
    }
    match block {
        BlockRef::Drive => p.drive.drive = 0.5,
        BlockRef::Filter => p.filter.cutoff = 2000.0,
        BlockRef::Folder => p.folder.fold = 0.5,
        _ => {}
    }
    p
}
```

`in_place_test.rs`: add Task 7's `tri()` helper (and imports `AlgoId`, `AlgoParams`, `WaveId`), and in `every_engine_and_every_effect` replace the two lines that set parts 0 (Pizza) and 1 (FM) with:

```rust
    s.parts[0].params = tri();
    s.parts[1].params = ParamSnapshot::for_engine(EngineType::Algo);
    let a = &mut s.parts[1].params.algo;
    (a.alg_a, a.alg_b, a.morph) = (AlgoId::A14.get(), AlgoId::A22.get(), 64);
    for (i, op) in a.ops.iter_mut().enumerate() {
        (op.wave, op.coarse, op.level, op.feedback) = (i as u8, [4, 8, 10, 13, 16, 19][i], 99, 7);
    }
```

(Both Algo parts now play; the old FM part was refused by the budget. The test compares an in-place build with one built by value, so no golden moves.)

`block_def_tests.rs`:

```rust
#[test]
fn chain_active_def_resolves() {
    let chain = &block_registry::ALGO_CHAIN;
    assert_eq!(chain.active_def(0, 0).unwrap().name, "Wave");
    assert_eq!(chain.active_def(3, 0).unwrap().name, "Filter");
}
```

- [ ] **Step 5b: Nothing of the old engines is left**

Run: `rg -n "Pizza|pizza|PizzaPoly|FmOp|FmParams|FmEngine|fm_tables|envelope_fm|fm_waveform|EngineType::Fm|EngineType::Va|ChainType::Fm|oscillator::|SineOsc" chimera-core chimera-stm32 chimera-desktop`
Expected: no output.

- [ ] **Step 6: Run the tests and look at the memory**

Run: `cargo test -p chimera-core`
Expected: PASS. The goldens that remain (`modal_init`, `modal_lfo_cutoff`, `algo_init`, the instrument goldens and every screen that still exists) match unchanged: the deletion changes no sound and no remaining screen.

Run: `cargo test -p chimera-core --test memory_budget_test -- --nocapture`
Expected: PASS; AXI shrinks by about 6 KB (Pizza's and FM's ~112 bytes per `ParamSnapshot`, 56 copies) from Task 6's figure.

- [ ] **Step 7: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 8: Commit**

The deletions were staged by Step 1's `git rm`.

```bash
git add chimera-core/src/dsp/mod.rs chimera-core/src/dsp/engines.rs chimera-core/src/dsp/voice.rs chimera-core/src/params.rs chimera-core/src/addr.rs chimera-core/src/preset.rs chimera-core/src/modulation.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/chain.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/page.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/viz.rs chimera-stm32/src/bench.rs
git add chimera-core/tests/golden_test.rs chimera-core/tests/common/mod.rs chimera-core/tests/sanity_test.rs chimera-core/tests/engines_test.rs chimera-core/tests/engine_source_test.rs chimera-core/tests/cost_test.rs chimera-core/tests/addr_test.rs chimera-core/tests/mod_registry_test.rs chimera-core/tests/modulation_test.rs chimera-core/tests/modulatable_test.rs chimera-core/tests/page_block_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/ui_routing_test.rs chimera-core/tests/prime_status_test.rs chimera-core/tests/focus_test.rs chimera-core/tests/block_def_tests.rs chimera-core/tests/browser_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs chimera-core/tests/big_viz_test.rs chimera-core/tests/property_test.rs chimera-core/tests/live_param_test.rs chimera-core/tests/in_place_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/block_test.rs
git commit -m "Pizza, FM and VA are gone; Algo and Modal remain"
```

---

### Task 10: The factory bank

Eight Algo Sounds (spec § Replacing the old engines): four TX81Z-style patches (bass, e-piano, brass, bell), two one-operator Sounds in the old Pizza role (saw lead, square bass), and two morph showcases, one with an LFO sweeping MORPH through the matrix. They are built in code, not stored as data, and the UI stores them into pool slots 01–08 (indices 0–7) when it starts, so the browser lists them and both builds play them. There are no user patches yet, so nothing migrates.

**Files:**
- Create: `chimera-core/src/factory.rs`
- Modify: `chimera-core/src/lib.rs` (`pub mod factory;`), `chimera-core/src/ui/mod.rs` (`UiState::init_in_place`)
- Test: `chimera-core/tests/factory_test.rs`; modify `chimera-core/tests/screen_golden_test.rs` (`sound_browser` re-recorded)

**Interfaces:**
- Consumes: `Sound`, `SoundPool`, `ChainType::Algo`, `AlgoParams`, `AlgoOpParams`, `AlgoId`, `WaveId`, `ModDestRegistry`, `ModState`, `BlockRef::Algo`.
- Produces: `factory::FACTORY_LEN: usize = 8`, `factory::factory_sound(i: usize) -> Option<Sound>`, `factory::load_factory(&mut SoundPool)`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/factory_test.rs`:

```rust
//! Spec § Replacing the old engines: a bank of eight Algo Sounds that plays.

use chimera_core::dsp::voice::Voice;
use chimera_core::factory::{FACTORY_LEN, factory_sound, load_factory};
use chimera_core::preset::{ChainType, SoundPool};
use chimera_core::ui::UiState;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// One second held, then three seconds of release.
fn play(i: usize) -> (Vec<f32>, Vec<f32>) {
    let s = factory_sound(i).unwrap();
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &s.params);
    let (mut held, mut tail) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..750 {
        v.render(&mut b, &s.params, &s.mod_state);
        held.extend_from_slice(&b);
    }
    v.note_off();
    for _ in 0..2250 {
        v.render(&mut b, &s.params, &s.mod_state);
        tail.extend_from_slice(&b);
    }
    (held, tail)
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

#[test]
fn eight_algo_sounds_with_distinct_names() {
    let names: Vec<String> = (0..FACTORY_LEN)
        .map(|i| {
            let s = factory_sound(i).unwrap();
            assert_eq!(s.chain_type, ChainType::Algo);
            s.name_str().to_string()
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        assert!(!n.is_empty() && !names[..i].contains(n), "{n}");
    }
    assert!(factory_sound(FACTORY_LEN).is_none());
}

#[test]
fn every_factory_sound_is_audible_finite_bounded_and_ends() {
    for i in 0..FACTORY_LEN {
        let name = factory_sound(i).unwrap().name_str().to_string();
        let (held, tail) = play(i);
        assert!(held.iter().chain(&tail).all(|x| x.is_finite()), "{name}");
        assert!(peak(&held) > 1e-2 && peak(&held) <= 1.0, "{name}: peak {}", peak(&held));
        assert!(peak(&tail[tail.len() - 10 * BLOCK_SIZE..]) < 1e-4, "{name} rings on");
    }
}

#[test]
fn the_morph_showcases_morph() {
    for i in [6, 7] {
        let s = factory_sound(i).unwrap();
        assert_ne!(s.params.algo.alg_a, s.params.algo.alg_b, "{}", s.name_str());
    }
    let pad = factory_sound(6).unwrap();
    assert_eq!(pad.mod_state.num_dests(), 1);
    assert_ne!(pad.mod_state.amount(1, 0), 0, "the LFO moves MORPH");
}

#[test]
fn the_ui_starts_with_the_bank_in_the_pool() {
    let ui = UiState::new();
    for i in 0..FACTORY_LEN {
        let want = factory_sound(i).unwrap();
        assert_eq!(ui.pool.get(i).unwrap().name_str(), want.name_str());
    }
    assert!(ui.pool.get(FACTORY_LEN).is_none());
    let mut pool = SoundPool::new();
    load_factory(&mut pool);
    assert!(pool.get(FACTORY_LEN - 1).is_some() && pool.get(FACTORY_LEN).is_none());
}
```

Run: `cargo test -p chimera-core --test factory_test`
Expected: FAIL to compile, "could not find `factory` in `chimera_core`".

- [ ] **Step 2: Write `factory.rs`**

```rust
//! The factory bank: eight Algo Sounds.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::waves::WaveId;
use crate::mod_path::ModDestRegistry;
use crate::modulation::ModState;
use crate::preset::{ChainType, NAME_LEN, Sound, SoundPool};

pub const FACTORY_LEN: usize = 8;

/// `env` is AR, D1R, D1L, D2R, RR.
fn op(wave: WaveId, coarse: u8, level: u8, env: [u8; 5]) -> AlgoOpParams {
    let [ar, d1r, d1l, d2r, rr] = env;
    AlgoOpParams {
        wave: wave.get(),
        coarse,
        level,
        ar,
        d1r,
        d1l,
        d2r,
        rr,
        ..AlgoOpParams::default()
    }
}

fn algo(a: AlgoId, b: AlgoId, morph: u8, ops: [AlgoOpParams; 6]) -> AlgoParams {
    AlgoParams { alg_a: a.get(), alg_b: b.get(), morph, transpose: 0, ops }
}

fn named(name: &str, algo: AlgoParams) -> Sound {
    let mut s = Sound::init(ChainType::Algo);
    s.name = [0; NAME_LEN];
    s.name[..name.len()].copy_from_slice(name.as_bytes());
    s.params.algo = algo;
    s
}

pub fn factory_sound(i: usize) -> Option<Sound> {
    let off = AlgoOpParams::default();
    let w1 = WaveId::W1;
    let sound = match i {
        0 => named(
            "TX BASS",
            algo(AlgoId::T5, AlgoId::T5, 0, [
                AlgoOpParams { velocity: 2, ..op(w1, 0, 99, [31, 9, 12, 4, 9]) },
                AlgoOpParams { feedback: 5, velocity: 4, ..op(w1, 4, 76, [31, 14, 3, 6, 9]) },
                op(WaveId::W2, 4, 80, [31, 10, 10, 4, 9]),
                op(w1, 8, 60, [31, 16, 2, 6, 9]),
                off,
                off,
            ]),
        ),
        1 => named(
            "TX EPIANO",
            algo(AlgoId::T5, AlgoId::T5, 0, [
                AlgoOpParams { velocity: 3, ..op(w1, 4, 99, [31, 6, 9, 3, 8]) },
                AlgoOpParams { velocity: 5, ..op(w1, 42, 58, [31, 18, 0, 0, 8]) },
                AlgoOpParams { velocity: 3, ..op(w1, 4, 92, [31, 5, 10, 3, 8]) },
                AlgoOpParams { velocity: 5, ..op(w1, 4, 66, [31, 9, 5, 3, 8]) },
                off,
                off,
            ]),
        ),
        2 => named(
            "TX BRASS",
            algo(AlgoId::T3, AlgoId::T3, 0, [
                op(w1, 4, 99, [18, 5, 13, 2, 7]),
                op(w1, 4, 72, [16, 6, 11, 2, 7]),
                op(w1, 4, 64, [20, 4, 12, 2, 7]),
                AlgoOpParams { feedback: 6, ..op(w1, 4, 58, [14, 6, 10, 2, 7]) },
                off,
                off,
            ]),
        ),
        3 => named(
            "TX BELL",
            algo(AlgoId::T5, AlgoId::T5, 0, [
                op(w1, 4, 99, [31, 4, 0, 0, 5]),
                op(w1, 12, 72, [31, 6, 0, 0, 5]),
                op(w1, 11, 88, [31, 5, 0, 0, 5]),
                op(w1, 23, 66, [31, 7, 0, 0, 5]),
                off,
                off,
            ]),
        ),
        4 => {
            let mut s = named(
                "SAW LEAD",
                algo(AlgoId::A2, AlgoId::A2, 0, [
                    op(WaveId::SAW, 4, 99, [31, 0, 15, 0, 8]),
                    AlgoOpParams { detune: 3, ..op(WaveId::SAW, 4, 95, [31, 0, 15, 0, 8]) },
                    off,
                    off,
                    off,
                    off,
                ]),
            );
            (s.params.filter.cutoff, s.params.filter.resonance) = (6000.0, 0.3);
            s
        }
        5 => {
            let sqr = op(WaveId::SQR, 0, 99, [31, 8, 12, 0, 9]);
            let mut s = named("SQR BASS", algo(AlgoId::T1, AlgoId::T1, 0, [sqr, off, off, off, off, off]));
            (s.params.filter.cutoff, s.params.filter.resonance) = (900.0, 0.4);
            s.params.drive.drive = 0.3;
            s
        }
        6 => {
            const COARSE: [u8; 6] = [4, 8, 10, 13, 16, 19];
            const LEVEL: [u8; 6] = [88, 80, 76, 72, 70, 66];
            let ops = core::array::from_fn(|i| op(w1, COARSE[i], LEVEL[i], [12, 0, 15, 0, 5]));
            let mut s = named("MORPH PAD", algo(AlgoId::A1, AlgoId::A17, 40, ops));
            s.params.lfo.rate = 0.2;
            let mut reg = ModDestRegistry::new();
            if reg.add(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH), *b"ALGMORPH").is_ok() {
                s.mod_state = ModState::from_registry(&reg, 2);
                s.mod_state.set_amount(1, 0, 50); // LFO → MORPH
                s.dest_registry = reg;
            }
            s
        }
        7 => named(
            "MORPH KEYS",
            algo(AlgoId::T5, AlgoId::A12, 64, [
                AlgoOpParams { velocity: 3, ..op(w1, 4, 94, [31, 6, 9, 3, 8]) },
                op(WaveId::W2, 8, 70, [31, 10, 4, 3, 8]),
                op(w1, 4, 86, [31, 6, 9, 3, 8]),
                AlgoOpParams { feedback: 3, ..op(w1, 13, 64, [31, 8, 6, 3, 8]) },
                op(WaveId::W3, 4, 76, [31, 6, 9, 3, 8]),
                op(w1, 19, 60, [31, 9, 5, 3, 8]),
            ]),
        ),
        _ => return None,
    };
    Some(sound)
}

pub fn load_factory(pool: &mut SoundPool) {
    for i in 0..FACTORY_LEN {
        if let Some(s) = factory_sound(i) {
            pool.store(i, s);
        }
    }
}
```

`lib.rs`: add `pub mod factory;` (alphabetical, after `pub mod dsp;`).

`ui/mod.rs`, `UiState::init_in_place`: replace `SoundPool::init_in_place(uninit_at(addr_of_mut!((*p).pool)));` with

```rust
            let pool = SoundPool::init_in_place(uninit_at(addr_of_mut!((*p).pool)));
            crate::factory::load_factory(pool);
```

and add to its SAFETY comment: "the pool is built in place, then filled with the factory bank".

- [ ] **Step 3: Run the tests**

Run: `cargo test -p chimera-core --test factory_test`
Expected: PASS. If a Sound's held peak exceeds 1.0, lower its loudest carrier's LEVEL by 3 until it passes (and say so in the commit's review notes); the levels above were set from the carrier normalisation (`1/√carriers`) and the voice's 0.8 volume.

- [ ] **Step 4: The browser now shows the bank**

Run: `cargo test -p chimera-core --test screen_golden_test`
Expected: FAIL for `sound_browser` only (rows 03–08 show factory names). Re-record it with `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture`, paste that row only, and look at the dump.

Run: `cargo test -p chimera-core`
Expected: PASS (`memory_budget_test::ui_state_fits_the_ui_reserve` unchanged: the pool was already counted).

- [ ] **Step 5: Listen on the desktop**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just desktop`, open the browser (EDIT + B1), load each factory Sound and play it. Report to the user which ones sound wrong; do not re-voice beyond Step 3's level rule without them.

- [ ] **Step 6: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/factory.rs chimera-core/src/lib.rs chimera-core/src/ui/mod.rs chimera-core/tests/factory_test.rs chimera-core/tests/screen_golden_test.rs
git commit -m "Factory bank of eight Algo Sounds"
```

---

### Task 11: The group pages

The OSC node gains every group page (spec § UI): WAVE, COARSE, FINE, DETUNE (FINE's sub-page), LEVEL, VELOCITY, the five ENV stages, RATE SCALE and FEEDBACK, as one flat list of sub-pages, since sub-pages are one level deep. VELOCITY and RATE SCALE are added because "Done when" requires every sub-project-1 parameter on a page. The ALGO page's viz band shows the A and B diagrams blended by the lerped MORPH, laid out from the table. The map's branch list scrolls to keep the current sub-page on screen (existing behaviour, now tested for all 13 rows), and no map node overlaps another.

**Files:**
- Create: `chimera-core/src/ui/alg_layout.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod alg_layout;`), `chimera-core/src/ui/block_def.rs` (`VizType::AlgoDiagram`), `chimera-core/src/ui/block_registry.rs`, `chimera-core/src/ui/viz.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/focus.rs`
- Test: create `chimera-core/tests/alg_layout_test.rs`; modify `header_map_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`, `part_page_test.rs` (all under `chimera-core/tests/`)

**Interfaces:**
- Consumes: `Algorithm`, `AlgoId`, `plan::{OPS, blend}`, `theme`, `draw`, `region::quantize_values`.
- Produces:
  - `alg_layout::AlgLayout { pos: [(i32, i32); 6], r: i32 }`, `alg_layout::layout(&Algorithm) -> AlgLayout`, `alg_layout::blend(&AlgLayout, &AlgLayout, m: f32) -> AlgLayout`
  - `viz::algo_diagram(d, a: &Algorithm, b: &Algorithm, morph: f32)`
  - `VizType::AlgoDiagram`
  - pages `ALGO_COARSE` (45), `ALGO_FINE` (46), `ALGO_DETUNE` (47), `ALGO_VELOCITY` (48), `ALGO_AR` (49), `ALGO_D1R` (50), `ALGO_D1L` (51), `ALGO_D2R` (52), `ALGO_RR` (53), `ALGO_RATE_SCALE` (54), `ALGO_FEEDBACK` (55)
  - `focus::MAX_PAGES = 64`

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/alg_layout_test.rs`:

```rust
//! Spec § UI and § Testing: the computed diagram stays inside the viz band
//! for all 32 algorithms, with no overlapping nodes and every link pointing
//! down; a deep chain switches to the compact spacing.

use chimera_core::dsp::algo::algorithms::{ALGORITHMS, AlgoId};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::ui::alg_layout::{blend, layout};
use chimera_core::ui::theme;

#[test]
fn every_algorithm_fits_the_band_without_overlap_and_links_point_down() {
    for alg in ALGORITHMS.iter() {
        let l = layout(alg);
        for (op, &(x, y)) in l.pos.iter().enumerate() {
            assert!(
                y - l.r > theme::VIZ_BAND_TOP && y + l.r < theme::VIZ_BAND_BOTTOM,
                "{} op {} y {y}",
                alg.name,
                op + 1
            );
            assert!(
                x - l.r >= theme::VIZ_LEFT && x + l.r <= theme::VIZ_RIGHT,
                "{} op {} x {x}",
                alg.name,
                op + 1
            );
        }
        for i in 0..OPS {
            for j in i + 1..OPS {
                let (dx, dy) = (l.pos[i].0 - l.pos[j].0, l.pos[i].1 - l.pos[j].1);
                assert!(
                    dx * dx + dy * dy >= (2 * l.r + 1).pow(2),
                    "{}: operators {} and {} overlap",
                    alg.name,
                    i + 1,
                    j + 1
                );
            }
        }
        for (src, m) in alg.mods.iter().enumerate() {
            for dst in 0..OPS {
                if m & (1 << dst) != 0 {
                    assert!(l.pos[src].1 < l.pos[dst].1, "{}: {}→{}", alg.name, src + 1, dst + 1);
                }
            }
        }
    }
}

#[test]
fn a_six_deep_chain_is_compact_and_a1_is_one_row() {
    let a17 = layout(AlgoId::A17.algorithm());
    assert!(a17.r < layout(AlgoId::T1.algorithm()).r);
    let a1 = layout(AlgoId::A1.algorithm());
    assert!(a1.pos.iter().all(|p| p.1 == theme::VIZ_BAND_MID));
}

#[test]
fn the_blend_ends_are_the_two_layouts() {
    let (a, b) = (layout(AlgoId::T1.algorithm()), layout(AlgoId::A17.algorithm()));
    assert_eq!(blend(&a, &b, 0.0).pos, a.pos);
    assert_eq!(blend(&a, &b, 1.0).pos, b.pos);
    assert_eq!(blend(&a, &b, 0.5).r, b.r);
}
```

`chimera-core/tests/header_map_test.rs` (extend the screen imports to `use screen::{Fb, Input, feed, scope_fixture, settle};` and add `use chimera_core::ui::UiState; use chimera_core::ui::perf::PerfStats; use chimera_core::ui::block_registry::ALGO_CHAIN; use chimera_hal::ButtonId;`):

```rust
/// Spec § UI: whichever node is current, the Algo chain's map pill and
/// labels never overlap, and no sub-page label runs into the next node's.
#[test]
fn the_algo_map_has_no_overlapping_nodes() {
    use chimera_core::ui::draw::text_width;
    let n = ALGO_CHAIN.len();
    let half = |label: &str| text_width(&theme::FONT_LABEL, label, 0) / 2;
    for cur in 0..n {
        let ext: Vec<(i32, i32)> = (0..n)
            .map(|i| {
                let x = node_x(i, n);
                if i == cur {
                    (x - theme::PILL_W / 2, x + theme::PILL_W / 2)
                } else {
                    let h = half(ALGO_CHAIN.blocks[i].def.short);
                    (x - h, x + h)
                }
            })
            .collect();
        for w in ext.windows(2) {
            assert!(w[0].1 < w[1].0, "node {cur} current: {ext:?}");
        }
        if cur + 1 < n {
            let block = &ALGO_CHAIN.blocks[cur];
            let x = node_x(cur, n) - 8 + 6;
            for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                let end = x + text_width(&theme::FONT_LABEL, def.short, 0);
                assert!(end < ext[cur + 1].0, "{} runs into node {}", def.short, cur + 1);
            }
        }
    }
}

/// Spec § UI: EDIT reaches every OSC sub-page, and the map scrolls so the
/// current one is always drawn lit.
#[test]
fn every_osc_sub_page_is_reachable_and_lit_on_the_map() {
    let mut ui = UiState::new();
    let block = &ALGO_CHAIN.blocks[0];
    let x = node_x(0, ALGO_CHAIN.len()) - 8;
    for sub in 0..block.sub_page_count() {
        if sub > 0 {
            feed(&mut ui, Input::press(ButtonId::Edit));
        }
        assert_eq!(ui.nav.active_block_def().id, block.active_def(sub).id);
        settle(&mut ui);
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        let cy = if sub == 0 {
            theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2
        } else {
            theme::SCREEN_H - theme::BRANCH_LINE_HEIGHT / 2
        };
        assert_eq!(fb.at(x, cy), theme::ACCENT, "sub-page {sub} ({})", block.active_def(sub).short);
        assert_eq!(fb.oob, 0);
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.nav.sub_page, block.sub_page_count() - 1, "EDIT stops at the last");
}
```

`chimera-core/tests/part_page_test.rs`, append:

```rust
#[test]
fn the_osc_node_has_every_operator_parameter() {
    use chimera_core::addr::BlockRef;
    use chimera_core::dsp::algo::params::ALGO_OP_SPECS;
    use chimera_core::ui::block_def::SlotBinding;
    let block = &reg::ALGO_CHAIN.blocks[0];
    let mut edited = Vec::new();
    for sub in 0..block.sub_page_count() {
        let def = block.active_def(sub);
        for (i, slot) in def.params.iter().enumerate() {
            let SlotBinding::Param(a) = slot.binding else { panic!("{} slot {i}", def.name) };
            assert_eq!(a.block, BlockRef::AlgoOp(Op::ALL[i]), "{} slot {i}", def.name);
            edited.push(a.param);
        }
    }
    for s in &ALGO_OP_SPECS {
        assert!(edited.contains(&s.id), "{} has no page", s.label);
    }
    let names: Vec<&str> = (0..block.sub_page_count()).map(|s| block.active_def(s).short).collect();
    assert_eq!(
        names,
        ["OSC", "CRS", "FIN", "DET", "LVL", "VEL", "AR", "D1R", "D1L", "D2R", "RR", "RS", "FBK"]
    );
}
```

Run: `cargo test -p chimera-core --test alg_layout_test --test header_map_test --test part_page_test`
Expected: FAIL to compile, "could not find `alg_layout` in `ui`".

- [ ] **Step 2: The layout**

Create `chimera-core/src/ui/alg_layout.rs`:

```rust
//! The algorithm diagram's layout, computed from the table: rows by depth
//! (longest path to a carrier, carriers at the bottom), carriers in operator
//! order, each modulator over the mean of its targets, rows packed so no two
//! nodes overlap. A deep chain switches to a compact spacing.

use crate::dsp::algo::algorithms::Algorithm;
use crate::dsp::algo::plan::OPS;
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlgLayout {
    pub pos: [(i32, i32); OPS],
    pub r: i32,
}

const STEP_X: f32 = 30.0;
/// Node radius and row step; the compact pair fits six rows in the band.
const NORMAL: (i32, i32) = (7, 17);
const COMPACT: (i32, i32) = (5, 11);
const MAX_NORMAL_ROWS: usize = 4;

pub fn layout(alg: &Algorithm) -> AlgLayout {
    let mut depth = [0usize; OPS];
    for op in 0..OPS {
        if alg.carriers & (1 << op) == 0 {
            depth[op] = (0..op)
                .filter(|&t| alg.mods[op] & (1 << t) != 0)
                .map(|t| depth[t] + 1)
                .max()
                .unwrap_or(0);
        }
    }
    let rows = depth.iter().max().map_or(1, |d| d + 1);
    let (r, step_y) = if rows > MAX_NORMAL_ROWS { COMPACT } else { NORMAL };
    let mut x = [0.0f32; OPS];
    for row in 0..rows {
        let mut members = [(0.0f32, 0usize); OPS];
        let mut n = 0;
        for op in (0..OPS).filter(|&op| depth[op] == row) {
            let want = if row == 0 {
                (0..op).filter(|&c| depth[c] == 0).count() as f32
            } else {
                let (sum, count) = (0..op)
                    .filter(|&t| alg.mods[op] & (1 << t) != 0)
                    .fold((0.0, 0.0), |(s, c), t| (s + x[t], c + 1.0));
                sum / count
            };
            members[n] = (want, op);
            n += 1;
        }
        let row_members = &mut members[..n];
        row_members.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut placed = [0.0f32; OPS];
        for k in 0..n {
            placed[k] = if k == 0 {
                row_members[0].0
            } else {
                row_members[k].0.max(placed[k - 1] + 1.0)
            };
        }
        let want_sum: f32 = row_members.iter().map(|m| m.0).sum();
        let shift = (want_sum - placed[..n].iter().sum::<f32>()) / n as f32;
        for k in 0..n {
            x[row_members[k].1] = placed[k] + shift;
        }
    }
    let (lo, hi) = x.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let mid = (lo + hi) / 2.0;
    let bottom = theme::VIZ_BAND_MID + (rows as i32 - 1) * step_y / 2;
    AlgLayout {
        pos: core::array::from_fn(|op| {
            (
                theme::SCREEN_W / 2 + ((x[op] - mid) * STEP_X) as i32,
                bottom - depth[op] as i32 * step_y,
            )
        }),
        r,
    }
}

/// `a` moving to `b` as MORPH goes from 0 to 1.
pub fn blend(a: &AlgLayout, b: &AlgLayout, m: f32) -> AlgLayout {
    let lerp = |p: i32, q: i32| p + ((q - p) as f32 * m) as i32;
    AlgLayout {
        pos: core::array::from_fn(|i| (lerp(a.pos[i].0, b.pos[i].0), lerp(a.pos[i].1, b.pos[i].1))),
        r: a.r.min(b.r),
    }
}
```

`ui/mod.rs`: add `pub mod alg_layout;` to the module list.

- [ ] **Step 3: The diagram and its viz**

`ui/block_def.rs`, `VizType`: add

```rust
    /// ALG A's diagram moving to ALG B's with MORPH.
    AlgoDiagram,
```

`ui/viz.rs`, add (imports: `use crate::dsp::algo::algorithms::Algorithm; use crate::dsp::algo::plan::{OPS, blend}; use crate::ui::alg_layout;`):

```rust
/// The ALGO page: A's layout moving to B's with MORPH. A link is drawn in
/// MID once its blended weight reaches 0.5, FAINT below; an operator is a
/// filled carrier once its blended carrier gain reaches 0.5. Links stay 1 px
/// and grey, like the map's line.
pub fn algo_diagram<D>(d: &mut D, a: &Algorithm, b: &Algorithm, morph: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let on = |bit: bool| if bit { 1.0 } else { 0.0 };
    let l = alg_layout::blend(&alg_layout::layout(a), &alg_layout::layout(b), morph);
    for src in 0..OPS {
        for dst in 0..OPS {
            let bit = 1 << dst;
            let w = blend(on(a.mods[src] & bit != 0), on(b.mods[src] & bit != 0), morph);
            if w > 0.0 {
                let ((x0, y0), (x1, y1)) = (l.pos[src], l.pos[dst]);
                let c = if w >= 0.5 { theme::MID } else { theme::FAINT };
                draw::line(d, x0, y0, x1, y1, c, 1);
            }
        }
    }
    for (op, label) in ["1", "2", "3", "4", "5", "6"].into_iter().enumerate() {
        let (x, y) = l.pos[op];
        let c = blend(on(a.carriers & (1 << op) != 0), on(b.carriers & (1 << op) != 0), morph);
        if c >= 0.5 {
            draw::dot(d, x, y, l.r, theme::INK2);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x + 1, y + 4, theme::BG, 0);
        } else {
            draw::dot(d, x, y, l.r, theme::BG);
            draw::ring(d, x, y, l.r, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x + 1, y + 4, theme::MID, 0);
        }
    }
}
```

`ui/renderer.rs` (import `crate::dsp::algo::algorithms::AlgoId`), in `draw_band_viz`:

```rust
            VizType::AlgoDiagram => {
                let algo = &f.parts[f.active_part].sound.params.algo;
                viz::algo_diagram(
                    display,
                    AlgoId::clamped(algo.alg_a).algorithm(),
                    AlgoId::clamped(algo.alg_b).algorithm(),
                    self.anim[2].current(),
                );
            }
```

and in `viz_inputs`, the `CellGrid` match:

```rust
                VizType::AlgoDiagram => {
                    let algo = &f.parts[f.active_part].sound.params.algo;
                    let q = region::quantize_values(&self.anim);
                    ([0, 0, q[2], 0, 0, 0], (algo.alg_a as u32) << 8 | algo.alg_b as u32)
                }
```

(Slot c of the ALGO page is MORPH; its lerped display value drives the blend, so the diagram glides with the knob.)

- [ ] **Step 4: The pages**

`ui/block_registry.rs`: `ALGO_ALG`'s `viz` becomes `VizType::AlgoDiagram`. Add after `ALGO_LEVEL`:

```rust
pub static ALGO_COARSE: BlockDef = group(45, "Coarse", "CRS", AlgoOpParams::COARSE);
pub static ALGO_FINE: BlockDef = group(46, "Fine", "FIN", AlgoOpParams::FINE);
pub static ALGO_DETUNE: BlockDef = group(47, "Detune", "DET", AlgoOpParams::DETUNE);
pub static ALGO_VELOCITY: BlockDef = group(48, "Velocity", "VEL", AlgoOpParams::VELOCITY);
pub static ALGO_AR: BlockDef = group(49, "Env AR", "AR", AlgoOpParams::AR);
pub static ALGO_D1R: BlockDef = group(50, "Env D1R", "D1R", AlgoOpParams::D1R);
pub static ALGO_D1L: BlockDef = group(51, "Env D1L", "D1L", AlgoOpParams::D1L);
pub static ALGO_D2R: BlockDef = group(52, "Env D2R", "D2R", AlgoOpParams::D2R);
pub static ALGO_RR: BlockDef = group(53, "Env RR", "RR", AlgoOpParams::RR);
pub static ALGO_RATE_SCALE: BlockDef = group(54, "Rate Scale", "RS", AlgoOpParams::RATE_SCALE);
pub static ALGO_FEEDBACK: BlockDef = group(55, "Feedback", "FBK", AlgoOpParams::FEEDBACK);
```

and replace `ALGO_OSC_SUB_PAGES`:

```rust
/// WAVE is the OSC node's home; FINE's DETUNE and the five ENV stages sit
/// right after their group (sub-pages are one level deep).
static ALGO_OSC_SUB_PAGES: [&BlockDef; 12] = [
    &ALGO_COARSE,
    &ALGO_FINE,
    &ALGO_DETUNE,
    &ALGO_LEVEL,
    &ALGO_VELOCITY,
    &ALGO_AR,
    &ALGO_D1R,
    &ALGO_D1L,
    &ALGO_D2R,
    &ALGO_RR,
    &ALGO_RATE_SCALE,
    &ALGO_FEEDBACK,
];
```

`ui/focus.rs`: `pub const MAX_PAGES: usize = 64;` and its doc "ids are 0..=55 today (test-checked)". (`FocusMemory` grows by 16 bytes; `ui_state_fits_the_ui_reserve` allows it.)

- [ ] **Step 5: Screens for the new pages**

`tests/screen/mod.rs`, add to `CASES` (after `engine_algo`):

```rust
    ("algo_alg", |ui| {
        feed(ui, Input::press(ButtonId::Plus)); // Algorithm page
        feed(ui, Input::turn(EncoderId::B, 24)); // ALG B = A17
        feed(ui, Input::turn(EncoderId::C, 50)); // MORPH 50: the diagrams blend
    }),
    ("algo_level", |ui| {
        to_level_page(ui);
        feed(ui, Input::turn(EncoderId::B, 60)); // operator 2 LEVEL
    }),
    ("algo_osc_last", |ui| {
        for _ in 0..12 {
            feed(ui, Input::press(ButtonId::Edit)); // FEEDBACK, the last sub-page
        }
        feed(ui, Input::turn(EncoderId::D, 3));
    }),
```

Run: `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture`
Add the three new rows to `GOLDENS` in the same order as `CASES`; every existing row must print unchanged (the ALGO page appears in no earlier case). Dump the screens (`SCREEN_DUMP=$PWD/target/screens cargo test -p chimera-core --test screen_golden_test -q`), convert the three with `magick` and look: `algo_alg` shows six nodes midway between T1's and A17's layouts; `algo_osc_last` shows the branch list scrolled to FBK, lit, above the screen's bottom edge.

- [ ] **Step 6: Run everything**

Run: `cargo test -p chimera-core`
Expected: PASS, including `focus_test::every_page_id_fits_the_focus_table` (ids up to 55) and `all_pages_walk_test::representative_pages_walk`.

Run: `cargo test -p chimera-core --test all_pages_walk_test -- --ignored`
Expected: PASS (the exhaustive walk visits all 13 OSC sub-pages, dirty render equal to full render on every frame).

- [ ] **Step 7: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/ui/alg_layout.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/block_def.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/viz.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/focus.rs chimera-core/tests/alg_layout_test.rs chimera-core/tests/header_map_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "Algo group pages and the blended algorithm diagram"
```

---

### Task 12: Goldens and ADRs 0022–0024

The Algo goldens the spec lists (ADR 0011, recorded deliberately after the gate): the init patch (Task 6), a patch per T1–T8, a morph sweep, plus the two locks Pizza's goldens held for the chain: an LFO route to CUTOFF, and an engine switch to Modal mid-note. Each case passes the sanity checks before its golden is recorded. Then the three ADRs.

**Files:**
- Modify: `chimera-core/tests/common/mod.rs`, `chimera-core/tests/golden_test.rs`, `chimera-core/tests/sanity_test.rs`
- Create: `docs/adr/0022-one-algorithmic-engine.md`, `docs/adr/0023-waves-from-our-own-recipes.md`, `docs/adr/0024-morph-blends-link-weights.md`
- Modify: `docs/adr/README.md`, `docs/adr/0003-keep-faithful-tx81z-fm.md` (status line only)

**Interfaces:**
- Consumes: `AlgoId`, `WaveId`, `init_params`, `lfo_route`, `CUTOFF`, `MOD_LFO_RATE`.
- Produces: `Case::{AlgoLfoCutoff, AlgoTx(u8), AlgoMorphStatic, AlgoMorphSweep, AlgoToModalSwitch}`; test helpers `common::tx_patch(AlgoId) -> ParamSnapshot`, `common::morph_patch() -> ParamSnapshot`.

- [ ] **Step 1: The cases**

`tests/common/mod.rs` (imports: `chimera_core::dsp::algo::algorithms::AlgoId`, `chimera_core::dsp::algo::params::AlgoParams`, `chimera_core::dsp::algo::waves::WaveId`):

```rust
/// MORPH, the destination of the morph sweep.
pub const MORPH: ParamAddr = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);

static TX_NAMES: [&str; 8] = [
    "algo_t1", "algo_t2", "algo_t3", "algo_t4", "algo_t5", "algo_t6", "algo_t7", "algo_t8",
];

/// Four TX-style operators on T1–T8 (`t` 0–7); operators 5 and 6 silent.
pub fn tx_patch(alg: AlgoId) -> ParamSnapshot {
    let mut p = init_params(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b) = (alg.get(), alg.get());
    let ops = [
        (WaveId::W1, 4, 99, 0),
        (WaveId::W1, 8, 70, 0),
        (WaveId::W2, 4, 80, 0),
        (WaveId::W1, 13, 60, 4),
    ];
    for (o, (wave, coarse, level, feedback)) in p.algo.ops.iter_mut().zip(ops) {
        (o.wave, o.coarse, o.level, o.feedback) = (wave.get(), coarse, level, feedback);
        (o.d1r, o.d1l, o.d2r) = (6, 10, 2);
    }
    p
}

/// Six sines at ratios 1–6 between A1 and A17, MORPH halfway.
pub fn morph_patch() -> ParamSnapshot {
    let mut p = init_params(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b, p.algo.morph) = (AlgoId::A1.get(), AlgoId::A17.get(), 64);
    for (i, o) in p.algo.ops.iter_mut().enumerate() {
        (o.coarse, o.level) = ([4, 8, 10, 13, 16, 19][i], 80);
    }
    p
}
```

`Case` gains (with docs):

```rust
    /// Algo init with the LFO on filter cutoff: the chain lock Pizza's
    /// `pizza_lfo_cutoff` held.
    AlgoLfoCutoff,
    /// `tx_patch` on T1–T8 (0–7).
    AlgoTx(u8),
    /// `morph_patch` with no modulation.
    AlgoMorphStatic,
    /// `morph_patch` with the LFO sweeping MORPH.
    AlgoMorphSweep,
    /// Algo init; engine switched to Modal at block ON_BLOCKS / 2 (mid-note).
    AlgoToModalSwitch,
```

`Case::ALL` becomes `[Case; 15]`: `ModalInit, ModalLfoCutoff, AlgoInit, AlgoLfoCutoff, AlgoTx(0)` … `AlgoTx(7), AlgoMorphStatic, AlgoMorphSweep, AlgoToModalSwitch`. `name()`: `"algo_lfo_cutoff"`, `Case::AlgoTx(t) => TX_NAMES[t as usize % 8]`, `"algo_morph_static"`, `"algo_morph_sweep"`, `"algo_to_modal_switch"`. `setup()`:

```rust
        Case::AlgoLfoCutoff => with_lfo(EngineType::Algo, CUTOFF),
        Case::AlgoTx(t) => (tx_patch(AlgoId::clamped(t)), ModState::new()),
        Case::AlgoMorphStatic => (morph_patch(), ModState::new()),
        Case::AlgoMorphSweep => {
            let mut p = morph_patch();
            p.lfo.rate = MOD_LFO_RATE;
            (p, lfo_route(MORPH))
        }
        Case::AlgoToModalSwitch => (init_params(EngineType::Algo), ModState::new()),
```

In `render_case`, bring back the switch: before the loop `let switched = init_params(EngineType::Modal);`, and in it

```rust
        let p = if case == Case::AlgoToModalSwitch && b >= ON_BLOCKS / 2 {
            &switched
        } else {
            &params
        };
        voice.render(&mut block, p, &mod_state);
```

and in `render_case_through_instrument`, `let mut switched = shared.clone(); switched.parts[0].params = init_params(EngineType::Modal);` with `let s = if case == Case::AlgoToModalSwitch && b >= ON_BLOCKS / 2 { &switched } else { &shared };` used for `handle` and `render`, as the Pizza version did.

- [ ] **Step 2: Gate them**

`tests/sanity_test.rs`, append:

```rust
/// ADR 0011: every Algo golden case passes the gate before it is recorded.
#[test]
fn every_algo_case_is_finite_bounded_audible_and_ends() {
    let gated = Case::ALL
        .into_iter()
        .filter(|c| c.name().starts_with("algo_") && *c != Case::AlgoToModalSwitch);
    for case in gated {
        assert_finite_bounded_audible(case);
        assert_silent_after_note_off(case);
    }
}
```

Run: `cargo test -p chimera-core --test sanity_test`
Expected: PASS. A failure stops the task: fix the engine first, never the gate.

- [ ] **Step 3: Record them**

Run: `GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test goldens_match -- --nocapture`
The rows for `modal_init`, `modal_lfo_cutoff` and `algo_init` must print unchanged; add the twelve new rows to `GOLDENS` under the comment `// Recorded after the Algo sanity gate (ADR 0011).`. Add `("algo_to_modal_switch", "https://github.com/joegiralt/chimera/issues/10")` to `KNOWN_BROKEN` (Modal's half is #10's known-broken output).

In `golden_test.rs`, `modulated_cases_differ_from_unmodulated` gets `(Case::AlgoLfoCutoff, Case::AlgoInit)` and `(Case::AlgoMorphSweep, Case::AlgoMorphStatic)`, and add:

```rust
/// Spec § Testing: a patch per T1–T8, each a different algorithm.
#[test]
fn the_eight_tx_algorithms_render_differently() {
    let hashes: Vec<u64> = (0..8).map(|t| fnv1a(&render_case(Case::AlgoTx(t)))).collect();
    for (i, h) in hashes.iter().enumerate() {
        assert!(!hashes[..i].contains(h), "T{} renders like an earlier T", i + 1);
    }
}
```

Run: `cargo test -p chimera-core --test golden_test`
Expected: PASS, through the Instrument too (Algo voices fit the budget; the switch case matches strictly there, like `pizza_to_modal_switch` did).

- [ ] **Step 4: ADR 0022**

Create `docs/adr/0022-one-algorithmic-engine.md`:

```markdown
# 0022. One algorithmic six-operator engine replaces Pizza, FM and VA

- **Status:** Accepted (2026-09-26); supersedes [0003](0003-keep-faithful-tx81z-fm.md)
- **Deciders:** project owner

## Context
The chip bench measured the ported 4-op FM engine at 6,590 cycles per voice
per sample, over the whole 7,000-cycle budget, so every FM note was refused
(#26). Its envelope ran in `f64`, which the Cortex-M7 does in software (24
soft-double calls per sample), and its waves called libm's `sinf`, which is
soft-double inside. Pizza and the silent VA placeholder were two more engines
to maintain for roles a phase-modulation engine covers with one or two
operators. The ported code's licence was never verified.

## Decision
- One engine, Algo: six phase-modulation operators on 32 algorithms (the
  TX81Z's eight on operators 1–4 with a 6→5 pair, and 24 more), morphing
  between ALG A and ALG B. `EngineType` and `ChainType` are `{Algo, Modal}`.
- The render path is `f32` only, with no libm: a source scan fails the
  build's tests on `f64` or libm anywhere in `dsp/algo/`.
- Envelopes run per sample with TX81Z-style rates: every four steps of
  effective rate doubles the speed; AR 31 is about 12 samples.
- The engine does not use the amp envelope; the operator envelopes shape the
  sound, so release tails ring out. The amp envelope keeps running as the
  ENV mod source, and since no engine puts it on the VCA, its parameters are
  no longer modulation destinations (ADR 0010).
- Parameters are stored as bytes (`u8`/`i8`). MORPH and the six LEVELs are
  the destinations, applied unrounded to the gain and the blend.
- The budget: the engine itself gets 350 cycles per voice per sample; the
  voice chain about 220; `AlgoEngine::COST` is the chip bench's worst case
  (six operators with feedback, six waves, MORPH halfway between A14 and A22)
  minus `Voice::CHAIN_COST`.
- ADR 0003's rules, one by one: no libm in the render path — kept, and
  extended to `f64`; DC blocking — not added: only W3, W4, W7 and W8 carry
  DC, as on the TX81Z; waveform names from the ported code — replaced by the
  TX81Z's own W1–W8 and plain names for the classic waves (ADR 0023).

## Alternatives considered
- **Fix the FM engine's `f64` and keep it:** still four operators, one
  algorithm set, and code of unverified licence.
- **Keep Pizza and VA beside a new engine:** three engines where one covers
  their roles.
- **Per-block envelopes:** cheaper, but the TX81Z's fastest attack is about
  12 samples, and a 64-sample block would smear it.

## Consequences
The FM, Pizza and VA goldens are gone; Algo goldens were recorded after its
sanity gate. A factory bank of eight Algo Sounds fills the pool at start.
The Algo chain is `OSC · ALG · DRV · FLT · FLD · MOD`, with every operator
parameter on an OSC group page. Modulation modes, the full wave set, gang
edit and the TX character path are sub-projects 2–5.

## Sources
`docs/superpowers/specs/2026-09-26-algo-engine-design.md`;
`docs/superpowers/plans/2026-09-26-algo-engine-core.md`; #26; the chip
bench (`chimera-stm32/src/bench.rs`); TX81Z owner's manual.
```

- [ ] **Step 5: ADR 0023**

Create `docs/adr/0023-waves-from-our-own-recipes.md`:

```markdown
# 0023. The waves come from our own recipes

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The Algo engine needs band-limited wave tables in flash. The TX81Z's eight
waves are well described by simple piecewise-sine formulas; its ROM is not
ours to copy, and the ported FM code (the formulas and tables we had) is of
unverified licence.

## Decision
- A host-only crate, `chimera-waves`, holds each wave as a formula (or a
  harmonic series) and renders it: 256 `i16` samples per mip, 8 mips of
  constant length, each keeping half the harmonics of the one before
  (127 … 1), one scale per wave so no mip is louder than another.
  `chimera-core`'s build script writes the tables; a build script cannot use
  the crate it builds, hence the separate crate.
- Sub-project 1 has 16 waves: TX81Z W1–W8 from their shapes (sine, the
  quarter-sine W2, half-wave and doubled forms), and triangle, saw, square,
  25 % and 12 % pulse, trisaw, rounded square and soft saw.
- W3, W4, W7 and W8 keep their DC; every other wave is centred.
- The TX81Z's facts the engine uses (the 64 coarse ratios, the FINE targets,
  the 0.75 dB LEVEL step, D1L's 3 dB step) are re-entered from the owner's
  manual, and no code from the ported FM engine is carried over.
- The tables are 64 KB, checked against a flash budget at compile time.

## Alternatives considered
- **Dump the TX81Z ROM:** not ours; no.
- **Keep the ported wave code:** licence unverified, and it computed `sinf`
  per sample.
- **Halving mip lengths:** a quarter of the flash, but the indexing and
  interpolation differ per mip; 64 KB fits.

## Consequences
Below 187.5 Hz, mip 0 cannot hold every harmonic up to Nyquist; a low saw is
duller than an analogue one. Phase-modulation sidebands alias, as on the
TX81Z. Sub-project 3 grows the set to 64 waves.

## Sources
`chimera-waves/src/lib.rs`; `chimera-core/build.rs`; TX81Z owner's manual
(waveform chart, frequency-ratio chart).
```

- [ ] **Step 6: ADR 0024**

Create `docs/adr/0024-morph-blends-link-weights.md`:

```markdown
# 0024. MORPH blends link weights; one plan orders both algorithms

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
MORPH moves a voice from ALG A to ALG B. The operators must be evaluated in
an order where a modulator runs before its target, and each end of the morph
must sound exactly like its algorithm alone.

## Decision
- Each link's weight is `a + m (b − a)` (1 or 0 in each algorithm), and so is
  each operator's carrier gain; MORPH ramps per sample across each block.
- The output is `Σ gain · out / sqrt(max(1, Σ gain))`: equal loudness for
  uncorrelated carriers, smooth through the morph.
- The plan is the union of both algorithms' links, in a topological order
  (Kahn's, highest operator first). A link that runs backwards in that order
  reads its source's previous sample; the kernel needs no flag for it, as it
  keeps each operator's latest output in place.
- Every table algorithm has its modulators numbered above their targets, so
  the union of any two is ordered 6 → 1 with no link running backwards. One
  plan therefore serves the whole morph range, and at MORPH 0 and 1 the
  output is bit-identical to the algorithm alone (tested for A14 → A22).
- A change of ALG A or B while a note sounds ducks the output over one block,
  swaps the plan, and ramps back over the next.

## Alternatives considered
- **Switch between A's and B's own orders at the ends:** needless when one
  order suits both, and a switch mid-morph would delay a live link.
- **Crossfade two whole renders:** twice the cost.

## Consequences
A future user-defined algorithm that links upward will make some unions
cyclic; its backward links read one sample late, and the plan's `delayed`
mask records which.

## Sources
`chimera-core/src/dsp/algo/{plan,kernel,morph}.rs`;
`chimera-core/tests/algo_engine_test.rs`.
```

- [ ] **Step 7: The index and ADR 0003's status**

`docs/adr/README.md`: change 0003's status cell to `Superseded by 0022` and append:

```markdown
| [0022](0022-one-algorithmic-engine.md) | One algorithmic six-operator engine replaces Pizza, FM and VA | Accepted |
| [0023](0023-waves-from-our-own-recipes.md) | The waves come from our own recipes | Accepted |
| [0024](0024-morph-blends-link-weights.md) | MORPH blends link weights; one plan orders both algorithms | Accepted |
```

`docs/adr/0003-keep-faithful-tx81z-fm.md`: only the status line changes, to `- **Status:** Accepted (2026-09-23); superseded by [0022](0022-one-algorithmic-engine.md)`.

- [ ] **Step 8: Format and check**

Run: `cargo fmt --all && just check`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add chimera-core/tests/common/mod.rs chimera-core/tests/golden_test.rs chimera-core/tests/sanity_test.rs docs/adr/0022-one-algorithmic-engine.md docs/adr/0023-waves-from-our-own-recipes.md docs/adr/0024-morph-blends-link-weights.md docs/adr/README.md docs/adr/0003-keep-faithful-tx81z-fm.md
git commit -m "Algo goldens after the sanity gate; ADRs 0022 to 0024"
```

---

### Task 13: The measured `COST` and the six-voice golden (hardware step)

"Done when" (spec § Intent): the chip bench's worst-case Algo patch measures within the budget and that measurement is committed as `AlgoEngine::COST`; then the six-voice chord golden is recorded, so the allocator does not refuse it.

**Files:**
- Modify: `chimera-stm32/src/bench.rs`
- Modify: `chimera-core/src/dsp/algo/engine.rs` (`COST`)
- Test (modify): `chimera-core/tests/cost_test.rs`, `chimera-core/tests/instrument_test.rs`

**Interfaces:**
- Consumes: `AlgoId`, `WaveId`, `ParamSnapshot::for_engine(EngineType::Algo)`, `factory::factory_sound`.
- Produces: `AlgoEngine::COST` = the measurement; `bench::algo_worst_case() -> ParamSnapshot` (private).

- [ ] **Step 1: The worst-case row on the bench**

`chimera-stm32/src/bench.rs` (imports: `chimera_core::dsp::algo::algorithms::AlgoId`; `WaveId` is already imported):

```rust
/// Spec § Budget: the patch `AlgoEngine::COST` is measured on. Six audible
/// operators, all with feedback, six distinct waves, MORPH 0.5 between A14
/// and A22.
fn algo_worst_case() -> ParamSnapshot {
    const WAVES: [WaveId; OPS] = [
        WaveId::W2,
        WaveId::SAW,
        WaveId::SQR,
        WaveId::P25,
        WaveId::TRI,
        WaveId::W7,
    ];
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    (p.algo.alg_a, p.algo.alg_b, p.algo.morph) = (AlgoId::A14.get(), AlgoId::A22.get(), 64);
    for (i, op) in p.algo.ops.iter_mut().enumerate() {
        (op.wave, op.coarse, op.level, op.feedback) = (WAVES[i].get(), [4, 8, 10, 13, 16, 19][i], 99, 7);
    }
    p
}
```

In `run`, after the engine rows:

```rust
    let worst: [u32; MAX_VOICES] =
        core::array::from_fn(|i| rig.time(|s| s.parts[0].params = algo_worst_case(), i + 1));
```

and call `show(display, clocks, &voices, &worst, kernel, &fx);`. Replace `show` with:

```rust
fn show(
    display: &mut impl ChimeraDisplay,
    clocks: Clocks,
    voices: &[[u32; MAX_VOICES]; ENGINES],
    worst: &[u32; MAX_VOICES],
    kernel: u32,
    fx: &[u32; REVERB_TYPES],
) {
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let _ = write!(
        line,
        "BENCH REV {} {} MHZ",
        clocks.rev.label(),
        clocks.cpu_hz / 1_000_000
    );
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, 20, theme::INK);
    draw::text(display, &theme::FONT_LABEL, "CYCLES/SAMPLE, 1..6 VOICES", 4, 36, theme::MID);
    let rows = EngineType::ALL
        .iter()
        .map(|&e| name(e))
        .zip(voices.iter())
        .chain(core::iter::once(("ALGO WC", worst)));
    let mut y = 58;
    for (label, cycles) in rows {
        voice_row(display, &mut line, y, label, cycles);
        y += 30;
    }
    line.clear();
    let _ = write!(line, "KERNEL /VOICE {kernel} (350)");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    y += 30;
    draw::text(display, &theme::FONT_VALUE, "FX", 4, y, theme::INK);
    for (i, (label, c)) in ["PLATE", "FDN", "MV"].iter().zip(fx).enumerate() {
        line.clear();
        let _ = write!(line, "{label} {c}");
        let x = 4 + i as i32 * 2 * CELL_W;
        draw::text(display, &theme::FONT_LABEL, line.as_str(), x, y + 13, theme::INK2);
    }
    display.flush();
}

const CELL_W: i32 = 38;

/// `label`'s per-voice cost (six voices minus one, over five) and its six counts.
fn voice_row(
    display: &mut impl ChimeraDisplay,
    line: &mut FmtBuf,
    y: i32,
    label: &str,
    cycles: &[u32; MAX_VOICES],
) {
    let per_voice = cycles[MAX_VOICES - 1].saturating_sub(cycles[0]) / (MAX_VOICES as u32 - 1);
    line.clear();
    let _ = write!(line, "{label} /VOICE {per_voice}");
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
    // One cell per count: six five-digit counts overflow a `FmtBuf`.
    for (i, c) in cycles.iter().enumerate() {
        line.clear();
        let _ = write!(line, "{c}");
        draw::text(display, &theme::FONT_LABEL, line.as_str(), 4 + i as i32 * CELL_W, y + 13, theme::INK2);
    }
}
```

Run: `cargo fmt --all && just check`
Expected: PASS.

```bash
git add chimera-stm32/src/bench.rs
git commit -m "Worst-case Algo patch on the bench"
```

- [ ] **Step 2: STOP. Ask the user to run the chip bench, and wait**

Send the user exactly this, then wait:

> The worst-case Algo patch is on the bench. Please:
> 1. Put the synth into DFU mode and run `just flash-bench`.
> 2. On the bench screen read the first line (`BENCH REV … MHZ`), `ALGO WC /VOICE <N>` with its six counts, `KERNEL /VOICE <k> (350)`, and the `FX` line.
> 3. Reply with those numbers, then run `just flash`.

- [ ] **Step 3: Commit the measurement**

With the user's `N` (rev V, 480 MHz): `COST = N − 10` (`Voice::CHAIN_COST`), rounded up to the next 10, as for the other engines.

`dsp/algo/engine.rs`: `pub const COST: Cost = Cost(<COST>); // measured <date>, bench, rev V at 480 MHz` (the date the user ran it).

`tests/cost_test.rs`: the Algo row becomes `assert_eq!(Voice::cost(EngineType::Algo), Cost(<COST + 10>));`, its doc cites "`ALGO WC` <N>", and add:

```rust
/// Spec § Intent, "Done when": six worst-case Algo voices fit beside the FX bus.
#[test]
fn six_worst_case_algo_voices_fit_the_budget() {
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    let six = FxBus::COST.0 + MAX_VOICES as u32 * Voice::cost(EngineType::Algo).0;
    assert!(six <= budget.0, "{six} of {}", budget.0);
}
```

Run: `cargo test -p chimera-core --test cost_test`
Expected: PASS when `COST ≤ 623` (3,200 + 6 × (COST + 10) ≤ 7,000). If it fails, stop and report: six voices do not fit, and the user chooses between the fallbacks (Appendix A) and five voices (then this test asserts five and Step 4's chord has five notes).

- [ ] **Step 4: The six-voice golden**

`tests/instrument_test.rs`:

```rust
/// Six voices of the factory TX EPIANO: recorded after `AlgoEngine::COST`
/// was measured, so the allocator admits all six.
fn six_voice_chord() -> Vec<f32> {
    let mut perf = Performance::new();
    perf.parts[0].sound = chimera_core::factory::factory_sound(1).expect("TX EPIANO");
    render_perf(&perf, &[(0, 48), (0, 55), (0, 60), (0, 64), (0, 67), (0, 72)], 200)
}

#[test]
fn a_six_voice_algo_chord_is_not_refused() {
    let mut perf = Performance::new();
    perf.parts[0].sound = chimera_core::factory::factory_sound(1).expect("TX EPIANO");
    let shared = AudioShared::from_performance(&perf);
    let mut rig = Rig::new();
    for n in [48, 55, 60, 64, 67, 72] {
        rig.inst.handle(on(0, n), &shared);
    }
    rig.render(&shared);
    assert_eq!(rig.inst.allocator().refused(), 0);
    let held = rig.inst.allocator().slots().iter().filter(|s| !s.is_free()).count();
    assert_eq!(held, 6);
}
```

Add `("six_voice_chord", six_voice_chord)` to `instrument_goldens_match`'s cases (`[GoldenCase; 5]`), run `GOLDEN_RECORD=1 cargo test -p chimera-core --test instrument_test instrument_goldens_match -- --nocapture`, check the four existing rows print unchanged, and add the new row with `// recorded after AlgoEngine::COST was measured`.

Run: `cargo test -p chimera-core --test instrument_test`
Expected: PASS.

- [ ] **Step 5: Format, check and commit**

Run: `cargo fmt --all && just check`
Expected: PASS.

```bash
git add chimera-core/src/dsp/algo/engine.rs chimera-core/tests/cost_test.rs chimera-core/tests/instrument_test.rs
git commit -m "Measured Algo cost and the six-voice golden"
```

- [ ] **Step 6: STOP. The on-chip listening checks (the user runs them)**

Send the user exactly this and wait for the results:

> Please run the on-chip checks with the normal firmware (`just flash`):
> 1. **Loopback.** Connect DAC pair 1's output to your audio interface's input and record. Load factory Sound 02 (TX EPIANO) on Part 1 (EDIT + B1, scroll to slot 02, EDIT) and hold A4 (MIDI note 69) for 10 seconds. Check that the fundamental is 440 Hz within 1 cent, that no sample reaches full scale, and that there is no click at note-on, note-off, or when you turn operator 1's WAVE (OSC page, encoder A) and ALG A (Algorithm page, encoder A) while holding the note.
> 2. **Load under a chord.** Open System ▸ About ▸ AUDIO (MENU, PLUS ×4, EDIT). Play a six-note chord of TX EPIANO and hold it for 10 seconds. Read PEAK (the load; it must stay under 100 %) and OVER (it must not increase).
> 3. Reply with the pitch, the peak level, anything audible at the swaps, PEAK and OVER.

A failure in either check becomes a GitHub issue (joegiralt/chimera) with the recording's numbers, referenced in the handoff; it does not reopen the tasks above without the user.

---

## Appendix A: Fallbacks if the kernel misses 350 cycles

Apply in order, one at a time, re-running Task 3 Step 9 after each.

**Fallback 1: specialise the loop per block.** When nothing ramps this block (MORPH, gains and output scale all still, the usual case once a note settles), run a copy of the loop with the ramp updates compiled out. The bit-identity tests still hold: a ramp of zero adds nothing. In `kernel.rs`, replace the sample loop (from `for s in out.iter_mut()` to the end of that loop) with:

```rust
        let still = dm == 0.0 && dnorm == 0.0 && lanes.iter().all(|l| l.dgain == 0.0);
        if still {
            self.run::<false>(&mut lanes, &mut w, &dw, &src, norm, dnorm, out);
        } else {
            self.run::<true>(&mut lanes, &mut w, &dw, &src, norm, dnorm, out);
        }
```

and add:

```rust
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn run<const RAMP: bool>(
        &mut self,
        lanes: &mut [Lane; OPS],
        w: &mut [f32; MAX_EDGES],
        dw: &[f32; MAX_EDGES],
        src: &[u8; MAX_EDGES],
        mut norm: f32,
        dnorm: f32,
        out: &mut [f32; BLOCK_SIZE],
    ) {
        for s in out.iter_mut() {
            let mut acc = 0.0f32;
            for l in lanes.iter_mut() {
                let prev = self.out[l.op & 7];
                let mut pm = l.feedback * (prev + l.hist);
                for e in l.edges.0..l.edges.1.min(MAX_EDGES) {
                    pm += w[e] * self.out[src[e] as usize & 7];
                    if RAMP {
                        w[e] += dw[e];
                    }
                }
                l.phase = l.phase.wrapping_add(l.inc);
                let p = l.phase.wrapping_add((pm as i32 as u32) << 8);
                let y = read(l.lo, l.hi, l.xfade, p) * l.env.step() * l.gain;
                l.hist = prev;
                self.out[l.op & 7] = y;
                acc += l.carrier * y;
                if RAMP {
                    l.gain += l.dgain;
                    l.carrier += l.dcarrier;
                }
            }
            *s = acc * norm;
            if RAMP {
                norm += dnorm;
            }
        }
    }
```

In `bench.rs`'s `time_kernel`, measure both paths: keep the ramping block and add a second set with `morph_from = morph_to = 0.5` and `gain_from = gain_to = 0.8 * SAMPLE_SCALE`, shown as `KERNEL STILL /VOICE <n>`. The spec's worst case holds MORPH at 0.5, so the decision uses the still figure; the ramping one is the ceiling for a modulated MORPH or LEVEL. Commit: `Kernel specialised per block`.

**Fallback 2: one mip for high carriers.** Above a pitch where the missing crossfade is inaudible, a carrier reads one mip. In `kernel.rs`, add `single: bool` to `Lane`, set `single: core::ptr::eq(o.lo, o.hi),`, and read with

```rust
                let v = if l.single { read_one(l.lo, p) } else { read(l.lo, l.hi, l.xfade, p) };
                let y = v * l.env.step() * l.gain;
```

where

```rust
#[inline(always)]
fn read_one(t: &Table, p: u32) -> f32 {
    let i = (p >> 24) as usize;
    let (a, b) = (t[i] as f32, t[(i + 1) & (WAVE_LEN - 1)] as f32);
    a + (b - a) * ((p & 0x00ff_ffff) as f32 * (1.0 / PHASE_UNITS))
}
```

In `engine.rs` (after Task 5), after `mip_pair`: a carrier (`blend(carrier_a, carrier_b, m) > 0.0`) whose `x * sr` exceeds `SINGLE_MIP_ABOVE_HZ` (`const SINGLE_MIP_ABOVE_HZ: f32 = 1_000.0;`, a `ponytail:` note on it naming this fallback) takes `(lo, lo, 0.0)`. In the bench, give operators 1 and 2 (A14's carriers) `hi: lo`. Commit: `Single mip for high carriers`.

**Fallback 3: five voices.** No code. Task 13's measured `COST` makes the allocator refuse a sixth Algo voice; its tests then assert five.
