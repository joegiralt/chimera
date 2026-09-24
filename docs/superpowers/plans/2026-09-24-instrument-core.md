# Instrument Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the single-voice, single-track engine into a multitimbral instrument — a Performance of six Parts sharing a six-voice pool, a mixer, the shared FX bus and three stereo DAC pairs — playable in the desktop simulator and sized to fit the STM32H750 unchanged.

**Architecture:** New `hw.rs` holds the chip's limits; `const` assertions fail both builds when a layout stops fitting (voice pool in D2, FX bus + UI in AXI). Pure-logic pieces (`voice_alloc`, `note_queue`, `part`) are tested alone; `instrument.rs` composes them with the existing `Voice` and a new `FxBus`, and the desktop runs exactly that `Instrument` fed by a lock-free note queue and a double-buffered `AudioShared`. Existing goldens are re-checked bit-for-bit through each Part's mono bus.

**Tech Stack:** Rust 2024 (`no_std` core, `libm`), cpal/minifb desktop, cortex-m-rt firmware for `thumbv7em-none-eabihf`, `just`.

**Spec:** `docs/superpowers/specs/2026-09-24-instrument-core-design.md` (approved), with ADR 0013 (`docs/adr/0013-hardware-parity-budgets.md`) and ADRs 0007–0012. This plan adds ADR 0014 (Task 2).

## Global Constraints

- Rename: `Patch` → `Sound`, `Track` → `Part`, `Project` → `Performance`; `MixerState` is folded into `Part` and removed.
- `hw.rs`: `SAMPLE_RATE = 48_000` and `BLOCK_SIZE = 64` re-exported from `chimera-hal`; `MAX_VOICES = 6`; `MAX_PARTS = 6`; `DAC_PAIRS = 3`; `CPU_HZ = 480_000_000`; `CYCLES_PER_SAMPLE = CPU_HZ / SAMPLE_RATE` (10_000); `AUDIO_CYCLE_BUDGET = CYCLES_PER_SAMPLE * 70 / 100` (7_000); `AXI_SRAM = 512 * 1024`; `D2_SRAM = 288 * 1024`; `DTCM = 128 * 1024`.
- `const` size assertions fail the build on **both** targets; no `#[cfg]` escape for desktop.
- Every engine `COST` is marked `// estimate` until measured with the DWT cycle counter.
- Part defaults: part *n* listens on channel *n* (0-based), Poly, output P1, level 0.8, pan 0, sends 0.
- Note queue: fixed capacity 64 events, lock-free, no allocation; full queue → event dropped and counted.
- Existing goldens must match bit-for-bit through the new path (part 1's mono bus before pan and level). Goldens are re-recorded only for an intended sound change (ADR 0011).
- `just check` = core + hal tests, desktop build (+ its unit tests), firmware build + link.
- CLAUDE.md: no `unsafe` without `// SAFETY:`; no heap allocation, blocking or locks on the audio thread; no libc; bugs go to GitHub issues; decisions get ADRs; type-driven where it pays (ADR 0012); match surrounding style.
- Commit messages: conventional (`feat(core): …`, `refactor(core): …`, `test(core): …`, `build: …`, `docs: …`), ending with a blank line and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (or the model that writes the commit).

## Review Focus

1. **Loading a Sound into a Part (browser pool slot or init entry) while that Part is set to MIDI channel 10, level 0.3** — the Part keeps its channel and mix; only the Sound changes. Tests: Task 5 `loading_a_sound_keeps_the_mix`, `browser_load_keeps_part_mix`.
2. **Changing a Part's MIDI channel while a key is held** — the note-off (on the old channel) still releases the voice; no stuck note. Test: Task 12 `note_off_follows_the_note_on_channel`.
3. **Switching a held chord to a costlier Sound** (six FM voices → Modal) — the pool never exceeds the CPU budget; the newest voices are cut. Tests: Task 8 `recost_sheds_the_newest_voices_over_budget`, Task 12 `sound_change_mid_chord_stays_in_budget`.
4. **Switching a Part from Poly to Mono while a chord is held** — the chord's note-offs still release; new notes share one voice. Test: Task 8 `poly_to_mono_switch_releases_the_held_chord`.
5. **Two Parts set to the same MIDI channel** — both play (layer). Test: Task 12 `notes_route_by_channel`.

## Measured numbers (validated in a scratch worktree, `thumbv7em-none-eabihf` sizes)

| Item | Before | After |
|---|---|---|
| `Voice` | 67,832 B | 40,696 B |
| `[Voice; 6]` | 406,992 B | 244,176 B |
| `Instrument` (pool + allocator + buses) | — | 246,600 B |
| `FxBus` (chorus + delay + reverb) | 423,596 B | 253,612 B |
| `Reverb` / `TapeDelay` / `JunoChorus` | 215,180 / 192,016 / 16,400 B | 140,940 / 96,272 / 16,400 B |
| `ParamSnapshot` | 376 B | 316 B |
| `UiState` (incl. Performance + SoundPool) | 34,724 B | 32,576 B |

Region budgets (asserted):

| Region | Size | Contents | Used | Headroom |
|---|---|---|---|---|
| D2 SRAM1+2+3 @ 0x3000_0000 | 294,912 | 8,192 DMA/MIDI reserve (512 B used today) + `Instrument` | 246,600 of `VOICE_RAM_BUDGET` 286,720 | 40,120 |
| AXI @ 0x2400_0000 | 524,288 | framebuffer 153,600 + `UI_RESERVE` 65,536 + Performance 5,212 + SoundPool 26,496 + 2 × AudioShared 6,216 + FxBus 253,612 | 510,672 | 13,616 |
| DTCM | 131,072 | unused by this plan | 0 | — |

`UI_RESERVE` (64 KB) is sized from a release firmware's measured stack (`-Z emit-stack-sizes`): `main` frame 73,744 B, of which Performance + SoundPool are 31,708 B (counted separately), plus nested `UiState::new` 5,896 B and `Performance::new` 5,360 B ≈ 53 KB, plus interrupt frames. A *debug* firmware keeps `chimera-core` at opt-level 0 (`main` 73,568 B plus `SoundPool::new` 79,528 B nested) and is only link-checked.

Firmware after Task 14 (debug build): `.ram_d2` 247,112 B — `AUDIO_BUF` still at 0x3000_0000, the `Instrument` reservation at 0x3000_0200.

Chosen fix for the memory assertions (spec order): **(1) shrink Modal's string buffers** — 2,048 → 1,200 samples, the period of E1 (MIDI 28, 41.2 Hz) at 48 kHz. It suffices; `MAX_VOICES` stays 6 and no pool is needed. All ten existing goldens stay bit-identical (the golden cases play note 60; a note's output changes only if its period exceeds 1,199 samples, i.e. notes ≤ 27, which no golden or test plays). The FX bus, which the spec did not budget, gets the same rule (Task 3): reverb lines trimmed to their longest tap (bit-identical, locked by new FX goldens) and the tape delay's range capped at 500 ms.

New goldens (FNV-1a over every sample's bits), recorded by this plan:

| Test | Case | Hash |
|---|---|---|
| `fx_golden_test` (recorded **before** the trim) | chorus_both | `0x0ad6a4dbd5636552` |
| | delay_375ms | `0x4ed2b23c577884bf` |
| | delay_500ms | `0xc1f7798ede6627fd` |
| | reverb_plate | `0x543857e5bed9848d` |
| | reverb_fdn_max_size | `0xcd5bdb2e4f83a478` |
| | reverb_midiverb | `0xef55c35ea73dc552` |
| `instrument_test` | poly_chord | `0x9e1be15b748f4ab1` |
| | two_parts_two_pairs | `0xcfe8ed2b4c185e18` |
| | reverb_send_off | `0x25fa9f662d1acb99` |
| | reverb_send_on | `0xaee0d4aead340f8d` |

## Environment

- Core suite: `cargo test -p chimera-core` (baseline 367 passed, 0 failed, 2 ignored; `-p chimera-hal` adds 0).
- Firmware: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf`.
- Desktop: needs ALSA's pkg-config file. Where headers are missing, point `PKG_CONFIG_PATH` at a directory with an `alsa.pc` that links the runtime library; cargo (and so `just`) inherits it: `PKG_CONFIG_PATH=/path/to/pkgconfig cargo test -p chimera-desktop`.
- "Full check" below means all three: `cargo test -p chimera-core -p chimera-hal`, `cargo test -p chimera-desktop`, `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` (from Task 1 on: `just check`).

## File structure

| File | Responsibility | Task |
|---|---|---|
| `chimera-core/src/hw.rs` (new) | Chip limits, region budgets, `Cost` | 1, 2, 3, 11 |
| `Justfile` | `check` runs tests, desktop, firmware link | 1, 13 |
| `chimera-core/src/dsp/modal.rs` | `MAX_STRING_DELAY` = 1,200; `ModalEngine::COST` | 2, 9 |
| `docs/adr/0014-audio-memory-map.md` (new), `docs/adr/README.md` | Memory map decision | 2 |
| `chimera-core/src/dsp/fx_bus.rs` (new) | `FxParams`, `FxBus`, `FX_SENDS` | 3, 9 |
| `chimera-core/src/dsp/{chorus,delay,reverb}.rs` | `is_on()`, buffer trims, delay range | 3 |
| `chimera-core/src/preset.rs` | `Sound`, `Part`, `Performance`, `PartEdit` | 4, 5, 6, 7 |
| `chimera-core/src/part.rs` (new) | `PartParams` block, `PartMode`, `DacPair` | 5 |
| `chimera-hal/src/lib.rs` | `MidiChannel` newtype | 5 |
| `chimera-core/src/addr.rs` | `Blocks` trait, `BlockRef::Part` | 6, 7 |
| `chimera-core/src/params.rs` | FX leave `ParamSnapshot`; `impl Blocks` | 6, 7 |
| `chimera-core/src/ui/{page,part_page,mod,block_registry}.rs` | Pages resolve through `Blocks`; Mixer chain | 6, 7 |
| `chimera-core/src/voice_alloc.rs` (new) | `Allocator` | 8 |
| `chimera-core/src/dsp/{engines,voice,pizza,engine_fm}.rs` | Cost estimates | 9 |
| `chimera-core/src/note_queue.rs` (new) | SPSC `NoteQueue` | 10 |
| `chimera-core/src/instrument.rs` (new) | `AudioShared`, `Instrument`, AXI assertion | 11, 12 |
| `.cargo/config.toml` | Test-thread stack size | 12 |
| `chimera-desktop/src/{audio,main}.rs` | Desktop plays the `Instrument` | 6, 12, 13 |
| `chimera-stm32/{memory.x,build.rs,src/audio.rs}` | D2 mapping + pool reservation | 12, 14 |

---

### Task 1: `hw.rs` target limits, `Cost`, and `just check` that links the firmware

**Files:**
- Create: `chimera-core/src/hw.rs`
- Modify: `chimera-core/src/lib.rs` (module list), `Justfile` (`check` recipe)
- Test: `chimera-core/tests/hw_test.rs`

**Interfaces:**
- Consumes: `chimera_hal::{SAMPLE_RATE, BLOCK_SIZE}`.
- Produces: `chimera_core::hw::{SAMPLE_RATE, BLOCK_SIZE, MAX_VOICES, MAX_PARTS, DAC_PAIRS, CPU_HZ, CYCLES_PER_SAMPLE, AUDIO_CYCLE_BUDGET: Cost, AXI_SRAM, D2_SRAM, DTCM}`; `pub struct Cost(pub u32)` with `Cost::ZERO`, `Add`, `Sum`, `Ord`. `AUDIO_CYCLE_BUDGET` is a `Cost` (not the spec's `u32`) so budgets and costs cannot be mixed up with other integers (ADR 0012).

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/hw_test.rs`:

```rust
//! Target limits (ADR 0013): the constants both builds enforce.

use chimera_core::hw::{self, Cost};

#[test]
fn audio_timing_matches_the_chip() {
    assert_eq!(hw::SAMPLE_RATE, chimera_hal::SAMPLE_RATE);
    assert_eq!(hw::BLOCK_SIZE, chimera_hal::BLOCK_SIZE);
    assert_eq!(hw::CYCLES_PER_SAMPLE, 10_000);
    assert_eq!(hw::AUDIO_CYCLE_BUDGET, Cost(7_000));
}

#[test]
fn capacity_constants() {
    assert_eq!((hw::MAX_VOICES, hw::MAX_PARTS, hw::DAC_PAIRS), (6, 6, 3));
}

/// STM32H750 memory map (RM0433 §2.3): AXI 512 KB, D2 SRAM1+2+3 = 288 KB, DTCM 128 KB.
#[test]
fn memory_regions_match_the_h750() {
    assert_eq!(hw::AXI_SRAM, 524_288);
    assert_eq!(hw::D2_SRAM, 294_912);
    assert_eq!(hw::DTCM, 131_072);
}

#[test]
fn costs_add_and_compare() {
    assert_eq!(Cost(610) + Cost(1_210), Cost(1_820));
    assert_eq!([Cost(1), Cost(2), Cost(3)].into_iter().sum::<Cost>(), Cost(6));
    assert!(Cost(7_001) > hw::AUDIO_CYCLE_BUDGET);
    assert_eq!(Cost::ZERO, Cost(0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test hw_test`
Expected: FAIL to compile with `error[E0432]: unresolved import \`chimera_core::hw\``.

- [ ] **Step 3: Write minimal implementation**

Create `chimera-core/src/hw.rs`:

```rust
//! The STM32H750's limits as constants, shared by the desktop and firmware
//! builds (ADR 0013): a design that cannot run on the chip fails on both.

use core::iter::Sum;
use core::ops::Add;

pub use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

pub const MAX_VOICES: usize = 6;
pub const MAX_PARTS: usize = 6;
pub const DAC_PAIRS: usize = 3;

pub const CPU_HZ: u32 = 480_000_000;
pub const CYCLES_PER_SAMPLE: u32 = CPU_HZ / SAMPLE_RATE; // 10_000
/// 30% is left for UI, MIDI and interrupt overhead.
pub const AUDIO_CYCLE_BUDGET: Cost = Cost(CYCLES_PER_SAMPLE * 70 / 100); // 7_000

/// Memory regions, in bytes (STM32H750 map, RM0433 §2.3).
pub const AXI_SRAM: usize = 512 * 1024; // D1: framebuffer, UI, Performance, FX bus
pub const D2_SRAM: usize = 288 * 1024; // SRAM1+2+3 at 0x3000_0000: voices, DMA buffers
pub const DTCM: usize = 128 * 1024; // tables, audio stack

/// CPU cycles per sample (per voice for engines). Values are estimates
/// until measured on hardware with the DWT cycle counter (ADR 0013).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cost(pub u32);

impl Cost {
    pub const ZERO: Cost = Cost(0);
}

impl Add for Cost {
    type Output = Cost;

    fn add(self, rhs: Cost) -> Cost {
        Cost(self.0 + rhs.0)
    }
}

impl Sum for Cost {
    fn sum<I: Iterator<Item = Cost>>(iter: I) -> Cost {
        iter.fold(Cost::ZERO, Add::add)
    }
}
```

In `chimera-core/src/lib.rs`, add the module after `pub mod dsp;`:

```rust
pub mod dsp;
pub mod hw;
```

In `Justfile`, replace the `check` recipe:

```just
# Check everything compiles (desktop targets)
check:
    cargo check -p chimera-core -p chimera-hal -p chimera-desktop
```

with:

```just
# Everything must pass before a commit (ADR 0013): core + hal tests, desktop
# type-check, firmware build + link into its flash/RAM regions.
# The desktop needs ALSA's pkg-config file; point PKG_CONFIG_PATH at it if it
# is not installed system-wide (cargo inherits the variable).
check:
    cargo test -p chimera-core -p chimera-hal
    cargo check -p chimera-desktop
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf
```

(The spec says the `justfile` is missing; it exists as `Justfile` — CLAUDE.md's recipes are already there. Only `check` changes.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p chimera-core --test hw_test` — Expected: 4 passed.
Run: `PKG_CONFIG_PATH=… just check` — Expected: core suite 371 passed / 0 failed / 2 ignored, desktop `Finished`, firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/hw.rs chimera-core/src/lib.rs chimera-core/tests/hw_test.rs Justfile
git commit -m "feat(core): hw.rs target limits and Cost; just check builds firmware

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Modal strings sized to E1 so six voices fit D2 (+ ADR 0014)

**Files:**
- Modify: `chimera-core/src/hw.rs`, `chimera-core/src/dsp/modal.rs:280-303,782,797`, `chimera-core/src/dsp/voice.rs` (imports, const assertion)
- Create: `docs/adr/0014-audio-memory-map.md`; Modify: `docs/adr/README.md`
- Test: `chimera-core/tests/memory_budget_test.rs`

**Interfaces:**
- Consumes: `hw::{D2_SRAM, MAX_VOICES}` (Task 1).
- Produces: `hw::D2_DMA_RESERVE = 8 * 1024`, `hw::VOICE_RAM_BUDGET = D2_SRAM - D2_DMA_RESERVE` (286,720); `pub const chimera_core::dsp::modal::MAX_STRING_DELAY: usize = 1200` (was private `MAX_DELAY = 2048`).

Why 1,200: `MAX_STRING_DELAY` is only a ring-buffer length; `set_freq` clamps `delay_len` to `MAX_STRING_DELAY - 1` and every read is `% delay_len` (String/Sympathetic) or a ring read `delay_len` behind the write head (Bowed), so a note's output is unchanged whenever its period fits. E1 (MIDI 28) = 1,164 samples at 48 kHz fits; D#1 (1,234) does not. Six voices: 6 × 40,696 = 244,176 B (target) ≤ 286,720. Sympathetic strings are tuned at or above the main string, so they never need more.

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/memory_budget_test.rs`:

```rust
//! ADR 0013 memory budgets. The `const` assertions next to each type fail the
//! build on both targets; these repeat them at run time so a failure prints
//! the numbers.

use core::mem::size_of;

use chimera_core::dsp::modal::MAX_STRING_DELAY;
use chimera_core::dsp::note_to_freq;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw;

#[test]
fn voice_pool_fits_d2() {
    let size = size_of::<[Voice; hw::MAX_VOICES]>();
    eprintln!("[Voice; {}] = {size} B, budget {} B", hw::MAX_VOICES, hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "[Voice; {}] = {size} B", hw::MAX_VOICES);
}

/// ADR 0014: Modal's string buffers hold the period of E1 (MIDI 28, 41.2 Hz)
/// at 48 kHz exactly; lower notes clamp to the buffer.
#[test]
fn modal_strings_cover_e1_and_no_lower() {
    let period = |n: u8| (hw::SAMPLE_RATE as f32 / note_to_freq(n)) as usize;
    assert_eq!(period(28), 1164);
    assert!(period(28) <= MAX_STRING_DELAY - 1, "E1 must not clamp");
    assert!(period(27) > MAX_STRING_DELAY - 1, "buffer is larger than E1 needs");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test memory_budget_test`
Expected: FAIL to compile — `unresolved import chimera_core::dsp::modal::MAX_STRING_DELAY`, `cannot find value VOICE_RAM_BUDGET in module hw`.

- [ ] **Step 3: Add the budget and expose the buffer length (still 2,048)**

In `chimera-core/src/hw.rs`, after the `DTCM` line (leave one blank line between):

```rust

/// D2 kept free for audio DMA (3 SAI × 2 halves × 64 frames × 2 ch × 4 B =
/// 3 KB; today one 512 B buffer) and MIDI buffers.
pub const D2_DMA_RESERVE: usize = 8 * 1024;
/// `[Voice; MAX_VOICES]` lives in D2 beside the DMA buffers.
pub const VOICE_RAM_BUDGET: usize = D2_SRAM - D2_DMA_RESERVE; // 286_720
```

In `chimera-core/src/dsp/modal.rs`, rename the constant and its five uses (lines 280, 283, 293, 303, 782, 797):

```bash
sed -i 's/const MAX_DELAY: usize = 2048;/pub const MAX_STRING_DELAY: usize = 2048;/; s/\bMAX_DELAY\b/MAX_STRING_DELAY/g' chimera-core/src/dsp/modal.rs
```

- [ ] **Step 4: Run test to verify it fails on the numbers**

Run: `cargo test -p chimera-core --test memory_budget_test -- --nocapture`
Expected: 2 FAILED — `[Voice; 6] = 407472 B, budget 286720 B` (x86_64; 406,992 B on the target) and `buffer is larger than E1 needs`.

- [ ] **Step 5: Shrink the strings and assert the pool at compile time**

In `chimera-core/src/dsp/modal.rs` replace `pub const MAX_STRING_DELAY: usize = 2048;` with:

```rust
/// String delay-line length (ADR 0014): the period of E1 (MIDI 28, 41.2 Hz)
/// at 48 kHz is 1,164 samples, so E1 and above play at their exact period;
/// lower notes clamp to 1,199 samples (~40 Hz). Sized so six voices fit D2.
pub const MAX_STRING_DELAY: usize = 1200;
```

In `chimera-core/src/dsp/voice.rs`, add the import after `use crate::dsp::wavefolder::Wavefolder;`:

```rust
use crate::hw::{MAX_VOICES, VOICE_RAM_BUDGET};
```

and put this directly above `/// Complete voice signal chain:`:

```rust
// ADR 0013: the voice pool fits D2 SRAM beside the DMA buffers, on both targets.
const _: () = assert!(core::mem::size_of::<[Voice; MAX_VOICES]>() <= VOICE_RAM_BUDGET);

```

Create `docs/adr/0014-audio-memory-map.md` with exactly this content:

```markdown
# 0014. Voices in D2, FX bus in AXI; buffers sized to the range they serve

- **Status:** Accepted (2026-09-24)
- **Deciders:** project owner

## Context
ADR 0013 makes every audio-core layout fit the STM32H750 on both builds.
Measured on `thumbv7em-none-eabihf` before this change: `Voice` 67,832 B
(Modal's eight 2,048-sample string buffers are 65,664 B of it), so
`[Voice; 6]` = 406,992 B against 288 KB of D2 SRAM; the FX bus (chorus
16,400 + tape delay 192,016 + reverb 215,180) = 423,596 B, which the
instrument-core spec did not budget at all. AXI (512 KB) also holds the
150 KB framebuffer and `main`'s stack with the UI (73,744 B frame in a
release build).

## Decision
- **Voices in D2.** `Instrument` (the `[Voice; 6]` pool, allocator, part
  buses) is asserted `<= VOICE_RAM_BUDGET` = D2 (294,912) − 8 KB kept for
  DMA/MIDI buffers = 286,720 B. The firmware reserves it in
  `.ram_d2.voices` after the DMA buffer, so the linker proves it fits.
- **Modal strings sized to E1.** `MAX_STRING_DELAY` = 1,200 samples: E1
  (MIDI 28, 41.2 Hz, period 1,164 at 48 kHz) and above play at their exact
  period; lower notes clamp (~40 Hz floor; before: notes ≤ 18 clamped at
  2,047). `Voice` = 40,696 B, `Instrument` = 246,600 B.
- **FX bus in AXI**, asserted with everything else there: framebuffer
  153,600 + UI reserve 65,536 + Performance 5,212 + SoundPool 26,496 +
  2 × AudioShared 6,216 + FxBus 253,612 = 510,672 of 524,288 B.
  `Instrument::render` borrows the `FxBus` so the two can live in
  different regions.
- **FX buffers sized to their ranges.** Reverb lines cover their longest
  fixed tap (plate tank allpass 4,096 → 2,048, tank delay 8,192 → 4,800,
  FDN 2,048 → 1,152): output is bit-identical (locked by
  `fx_golden_test.rs`). The tape delay's range becomes 10–500 ms (was
  10–1,000) so its line is 24,064 samples (was 48,000): output is
  identical up to 500 ms.

## Alternatives considered
- **Lower `MAX_VOICES` to 4** — fits the old strings in D2 but loses two
  voices and does nothing for the FX bus.
- **Pool Modal's buffers** — only Modal voices need them, but the pool
  couples allocation to engine choice; more code for less than the 27 KB
  per voice the trim saves.
- **Voices in AXI, FX in D2** — the FX bus (≥ 333 KB even with the reverb
  trimmed and a 1 s delay) does not fit D2.
- **16-bit delay line, or one arena for the three reverbs** — change every
  delay/reverb output, or need a clear on type change inside the ISR.

## Consequences
Modal notes below E1 clamp; the delay tops out at 500 ms. AXI has
~13 KB headroom and D2 ~40 KB. The UI reserve is sized from a release
build's measured stack (`main` 73,744 B including UiState's Performance
and pool); a debug firmware (core at opt-level 0) needs far more stack and
is only link-checked. Before the port touches the pool, check that the D2
SRAM clocks (RCC_AHB2ENR SRAM1/2/3EN) are on; today only the 512 B DMA
buffer lives there.

## Sources
`chimera-core/tests/memory_budget_test.rs`, `fx_golden_test.rs`;
`docs/superpowers/plans/2026-09-24-instrument-core.md` (measurements);
RM0433 §2.3 memory map.
```

In `docs/adr/README.md`, add after the 0013 row:

```markdown
| [0014](0014-audio-memory-map.md) | Voices in D2, FX bus in AXI; buffers sized to the range they serve | Accepted |
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p chimera-core --test memory_budget_test -- --nocapture` — Expected: 2 passed, `[Voice; 6] = 244656 B, budget 286720 B`.
Run: `cargo test -p chimera-core` — Expected: 373 passed, 0 failed, 2 ignored; `golden_test` passes (all ten goldens bit-identical).
Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` — Expected: `Finished` (the const assertion holds on 32-bit too).
Optional proof the assertion bites: temporarily set `MAX_STRING_DELAY` back to 2048 and run `cargo build -p chimera-core` — Expected: `error[E0080]: evaluation panicked: assertion failed: core::mem::size_of::<[Voice; MAX_VOICES]>() <= VOICE_RAM_BUDGET`. Revert.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/hw.rs chimera-core/src/dsp/modal.rs chimera-core/src/dsp/voice.rs chimera-core/tests/memory_budget_test.rs docs/adr/0014-audio-memory-map.md docs/adr/README.md
git commit -m "feat(core): Modal strings sized to E1 so six voices fit D2

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: FX bus, with buffers trimmed to their ranges so it fits AXI

**Files:**
- Create: `chimera-core/src/dsp/fx_bus.rs`
- Modify: `chimera-core/src/dsp/mod.rs`, `chimera-core/src/hw.rs`, `chimera-core/src/dsp/chorus.rs`, `chimera-core/src/dsp/delay.rs`, `chimera-core/src/dsp/reverb.rs`
- Test: `chimera-core/tests/fx_golden_test.rs` (lock, written first), `chimera-core/tests/fx_bus_test.rs`, `chimera-core/tests/memory_budget_test.rs`

**Interfaces:**
- Consumes: `hw::{AXI_SRAM}` (Task 1).
- Produces: `hw::FX_BUS_BUDGET = 256 * 1024`; `chimera_core::dsp::fx_bus::{FX_SENDS = 3, FxParams { chorus: ChorusParams, delay: DelayParams, reverb: ReverbParams }, FxBus}`; `FxBus::new() -> Self`; `FxBus::process(&mut self, sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS], params: &FxParams, sample_rate: u32, ret: &mut [f32; BLOCK_SIZE])`; `ChorusParams::is_on(&self) -> bool`, `DelayParams::is_on`, `ReverbParams::is_on`. Send order everywhere: chorus, delay, reverb.

Semantics chosen (spec is silent): the effects are mono today, so each effect runs once on the sum of its sends and its processed output is the return (the effect's own `mix` param still sets its internal dry/wet, as today); an effect that is off returns nothing, so a send into it is silent. Stereo FX are DSP-quality work (out of scope).

- [ ] **Step 1: Lock today's FX output (before touching any buffer)**

Create `chimera-core/tests/fx_golden_test.rs`:

```rust
//! FX refactor lock (ADR 0011, ADR 0014): chorus, delay and reverb output
//! frozen bit-for-bit before their buffers were trimmed to what their ranges
//! need. Re-record only for an intended sound change:
//!
//!     GOLDEN_RECORD=1 cargo test -p chimera-core --test fx_golden_test -- --nocapture

mod common;

use chimera_core::dsp::chorus::{ChorusParams, JunoChorus};
use chimera_core::dsp::delay::{DelayParams, TapeDelay};
use chimera_core::dsp::reverb::{Reverb, ReverbParams};
use chimera_hal::BLOCK_SIZE;
use common::{fnv1a, SR};

/// 50 blocks of a 110 Hz saw at 0.5, then silence; 450 blocks in all so a
/// 500 ms delay repeats at least once.
const BURST_BLOCKS: usize = 50;
const FX_BLOCKS: usize = 450;

#[derive(Clone, Copy)]
enum Fx {
    Chorus(ChorusParams),
    Delay(DelayParams),
    Reverb(ReverbParams),
}

fn cases() -> [(&'static str, Fx); 6] {
    let reverb = |reverb_type: u8, size: f32| ReverbParams { reverb_type, time: 0.7, damping: 0.3, size, mix: 0.5 };
    [
        ("chorus_both", Fx::Chorus(ChorusParams { mode: 3, rate: 0.5, depth: 0.5, mix: 0.5 })),
        ("delay_375ms", Fx::Delay(DelayParams { feedback: 0.6, mix: 0.5, ..DelayParams::default() })),
        ("delay_500ms", Fx::Delay(DelayParams { time_ms: 500.0, feedback: 0.6, wow_flutter: 1.0, mix: 0.5, ..DelayParams::default() })),
        ("reverb_plate", Fx::Reverb(reverb(0, 0.5))),
        ("reverb_fdn_max_size", Fx::Reverb(reverb(1, 1.0))),
        ("reverb_midiverb", Fx::Reverb(reverb(2, 0.5))),
    ]
}

/// Block `b` of the input signal; `phase` carries the saw between blocks.
fn input(b: usize, phase: &mut f32) -> [f32; BLOCK_SIZE] {
    let mut block = [0.0f32; BLOCK_SIZE];
    if b < BURST_BLOCKS {
        for s in block.iter_mut() {
            *s = *phase - 0.5;
            *phase = (*phase + 110.0 / SR as f32).fract();
        }
    }
    block
}

fn render(fx: Fx) -> Vec<f32> {
    let mut chorus = Box::new(JunoChorus::new());
    let mut delay = Box::new(TapeDelay::new());
    let mut reverb = Box::new(Reverb::new());
    let mut out = Vec::with_capacity(FX_BLOCKS * BLOCK_SIZE);
    let mut phase = 0.0f32;
    for b in 0..FX_BLOCKS {
        let mut block = input(b, &mut phase);
        match fx {
            Fx::Chorus(p) => chorus.process(&mut block, &p, SR),
            Fx::Delay(p) => delay.process(&mut block, &p, SR),
            Fx::Reverb(p) => reverb.process(&mut block, &p),
        }
        out.extend_from_slice(&block);
    }
    out
}

const GOLDENS: &[(&str, u64)] = &[
    ("chorus_both", 0x0ad6a4dbd5636552),
    ("delay_375ms", 0x4ed2b23c577884bf),
    ("delay_500ms", 0xc1f7798ede6627fd),
    ("reverb_plate", 0x543857e5bed9848d),
    ("reverb_fdn_max_size", 0xcd5bdb2e4f83a478),
    ("reverb_midiverb", 0xef55c35ea73dc552),
];

#[test]
fn fx_goldens_match() {
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for (name, fx) in cases() {
        let hash = fnv1a(&render(fx));
        if record {
            println!("    (\"{name}\", 0x{hash:016x}),");
            continue;
        }
        match GOLDENS.iter().find(|g| g.0 == name) {
            Some(&(_, want)) if want == hash => {}
            Some(&(_, want)) => failures.push(format!("{name}: 0x{hash:016x} (want 0x{want:016x})")),
            None => failures.push(format!("{name}: no golden recorded")),
        }
    }
    assert!(failures.is_empty(), "fx golden mismatch:\n{}", failures.join("\n"));
}

/// A case whose effect is bypassed would lock only the dry input.
#[test]
fn fx_cases_are_not_dry() {
    let mut phase = 0.0f32;
    let dry: Vec<f32> = (0..FX_BLOCKS).flat_map(|b| input(b, &mut phase)).collect();
    for (name, fx) in cases() {
        assert_ne!(fnv1a(&render(fx)), fnv1a(&dry), "{name} is bypassed");
    }
}
```

- [ ] **Step 2: Run the lock on today's code**

Run: `cargo test -p chimera-core --test fx_golden_test`
Expected: 2 passed (the hashes were recorded from the unmodified FX; if they differ on your machine, STOP — the FX code is not what this plan was validated against).

- [ ] **Step 3: Write the failing FX bus tests**

Create `chimera-core/tests/fx_bus_test.rs`:

```rust
//! The shared FX bus (instrument-core spec § Audio path): each effect runs
//! once on the sum of the parts' sends; the return is the sum of the effects
//! that are on.

use chimera_core::dsp::fx_bus::{FxBus, FxParams, FX_SENDS};
use chimera_core::dsp::reverb::Reverb;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn burst() -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| if i % 16 == 0 { 0.5 } else { -0.1 })
}

/// Defaults match the old `ParamSnapshot` FX: everything off.
#[test]
fn defaults_are_all_off() {
    let p = FxParams::default();
    assert!(!p.chorus.is_on() && !p.delay.is_on() && !p.reverb.is_on());
    assert_eq!(p.reverb.mix, 0.0);
    assert_eq!(p.delay.time_ms, 375.0);
}

#[test]
fn effects_that_are_off_return_nothing() {
    let mut bus = Box::new(FxBus::new());
    let mut sends = [burst(); FX_SENDS];
    let mut ret = [1.0f32; BLOCK_SIZE];
    bus.process(&mut sends, &FxParams::default(), SR, &mut ret);
    assert!(ret.iter().all(|&s| s == 0.0));
}

/// With only the reverb on, the return is exactly the reverb of its send.
#[test]
fn return_is_the_processed_send() {
    let mut p = FxParams::default();
    p.reverb.mix = 0.5;
    p.reverb.time = 0.7;
    let mut bus = Box::new(FxBus::new());
    let mut alone = Box::new(Reverb::new());
    for b in 0..20 {
        let input = if b < 4 { burst() } else { [0.0; BLOCK_SIZE] };
        let mut sends = [[0.0; BLOCK_SIZE], [0.0; BLOCK_SIZE], input];
        let mut ret = [0.0f32; BLOCK_SIZE];
        bus.process(&mut sends, &p, SR, &mut ret);
        let mut want = input;
        alone.process(&mut want, &p.reverb);
        assert_eq!(ret, want, "block {b}");
    }
}
```

Append to `chimera-core/tests/memory_budget_test.rs`:

```rust

#[test]
fn fx_bus_fits_its_axi_share() {
    use chimera_core::dsp::fx_bus::FxBus;
    let size = size_of::<FxBus>();
    eprintln!("FxBus = {size} B, budget {} B", hw::FX_BUS_BUDGET);
    assert!(size <= hw::FX_BUS_BUDGET, "FxBus = {size} B");
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test fx_bus_test --test memory_budget_test`
Expected: FAIL to compile — `unresolved import chimera_core::dsp::fx_bus`, `cannot find value FX_BUS_BUDGET in module hw`.

- [ ] **Step 5: Add `FxBus`, `FxParams`, `is_on()` and the budget**

In `chimera-core/src/hw.rs`, after the `VOICE_RAM_BUDGET` line:

```rust
/// AXI share for the FX bus (ADR 0014). `instrument.rs` asserts the sum of
/// everything placed in AXI.
pub const FX_BUS_BUDGET: usize = 256 * 1024; // 262_144
```

In `chimera-core/src/dsp/mod.rs`, add after `pub mod delay;`:

```rust
pub mod fx_bus;
```

Create `chimera-core/src/dsp/fx_bus.rs`:

```rust
//! The shared FX bus (instrument-core spec § Audio path): chorus, delay and
//! reverb run once per block on the sum of every part's sends. The effects
//! are mono today, so the return is mono and lands on both sides of DAC
//! pair 1.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::chorus::{ChorusParams, JunoChorus};
use crate::dsp::delay::{DelayParams, TapeDelay};
use crate::dsp::reverb::{Reverb, ReverbParams};
use crate::hw::FX_BUS_BUDGET;

/// Sends per part, in this order: chorus, delay, reverb.
pub const FX_SENDS: usize = 3;

// ADR 0014: the FX bus lives in AXI SRAM beside the framebuffer and UI.
const _: () = assert!(core::mem::size_of::<FxBus>() <= FX_BUS_BUDGET);

/// Shared effect settings: one set per Performance, not per Sound.
#[derive(Clone, Copy, Debug)]
pub struct FxParams {
    pub chorus: ChorusParams,
    pub delay: DelayParams,
    pub reverb: ReverbParams,
}

impl Default for FxParams {
    /// The values `ParamSnapshot` carried before the FX moved out: all off.
    fn default() -> Self {
        Self {
            chorus: ChorusParams::default(),
            delay: DelayParams::default(),
            reverb: ReverbParams { reverb_type: 0, time: 0.5, damping: 0.3, size: 0.5, mix: 0.0 },
        }
    }
}

pub struct FxBus {
    chorus: JunoChorus,
    delay: TapeDelay,
    reverb: Reverb,
}

impl Default for FxBus {
    fn default() -> Self {
        Self::new()
    }
}

impl FxBus {
    pub fn new() -> Self {
        Self { chorus: JunoChorus::new(), delay: TapeDelay::new(), reverb: Reverb::new() }
    }

    /// Run each effect that is on over its send (processed in place) and
    /// write the sum of their outputs to `ret`. An effect that is off
    /// returns nothing, so a send into it is silent.
    pub fn process(
        &mut self,
        sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
        params: &FxParams,
        sample_rate: u32,
        ret: &mut [f32; BLOCK_SIZE],
    ) {
        ret.fill(0.0);
        let [chorus, delay, reverb] = sends;
        if params.chorus.is_on() {
            self.chorus.process(chorus, &params.chorus, sample_rate);
            add(ret, chorus);
        }
        if params.delay.is_on() {
            self.delay.process(delay, &params.delay, sample_rate);
            add(ret, delay);
        }
        if params.reverb.is_on() {
            self.reverb.process(reverb, &params.reverb);
            add(ret, reverb);
        }
    }
}

fn add(acc: &mut [f32; BLOCK_SIZE], x: &[f32; BLOCK_SIZE]) {
    for (a, &v) in acc.iter_mut().zip(x) {
        *a += v;
    }
}
```

`is_on()` — one source of truth for each effect's bypass. In `chimera-core/src/dsp/chorus.rs`, replace `impl ChorusParams {\n    pub const MODE` with:

```rust
impl ChorusParams {
    /// Off when the mode is off or the mix is below audibility; `process`
    /// passes the input through unchanged then.
    pub fn is_on(&self) -> bool {
        ChorusMode::from_u8(self.mode) != ChorusMode::Off && self.mix >= 0.001
    }

    pub const MODE
```

and at the top of `JunoChorus::process` replace

```rust
        let mode = ChorusMode::from_u8(params.mode);
        if mode == ChorusMode::Off || params.mix < 0.001 {
            return;
        }
```

with

```rust
        if !params.is_on() {
            return;
        }
        let mode = ChorusMode::from_u8(params.mode);
```

In `chimera-core/src/dsp/delay.rs`, replace `impl DelayParams {\n    pub const TIME_MS` with:

```rust
impl DelayParams {
    /// Off when the mix is below audibility; `process` passes the input
    /// through unchanged then.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    pub const TIME_MS
```

and in `TapeDelay::process` replace `if params.mix < 0.001 {` with `if !params.is_on() {`.

In `chimera-core/src/dsp/reverb.rs`, replace `impl ReverbParams {\n    pub const REVERB_TYPE` with:

```rust
impl ReverbParams {
    /// Off when the mix is below audibility; `process` passes the input
    /// through unchanged then.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    pub const REVERB_TYPE
```

and in `Reverb::process` replace `if params.mix < 0.001 {` with `if !params.is_on() {`.

- [ ] **Step 6: Run tests — the bus works, the budget fails**

Run: `cargo test -p chimera-core --test fx_bus_test`
Expected: FAIL to compile the library: `error[E0080]: evaluation panicked: assertion failed: core::mem::size_of::<FxBus>() <= FX_BUS_BUDGET` (the bus is 423,728 B on x86_64 / 423,596 B on the target).

- [ ] **Step 7: Trim the buffers to their ranges**

In `chimera-core/src/dsp/reverb.rs`:

Replace

```rust
    // Tank: 2 branches, each with 2 allpass + 1 delay
    ap_tank: [DelayLine<4096>; 4],
    del_tank: [DelayLine<8192>; 2],
```

with

```rust
    // Tank: 2 branches, each with 2 allpass + 1 delay
    ap_tank: [DelayLine<AP_TANK_LINE>; 4],
    del_tank: [DelayLine<DEL_TANK_LINE>; 2],
```

Replace

```rust
const AP_TANK_LENS: [usize; 4] = [1653, 2038, 1913, 1663];
const DEL_TANK_LENS: [usize; 2] = [3411, 4782];
```

with

```rust
const AP_TANK_LENS: [usize; 4] = [1653, 2038, 1913, 1663];
const DEL_TANK_LENS: [usize; 2] = [3411, 4782];
/// Line lengths cover the longest fixed tap (ADR 0014); a `DelayLine<N>`
/// with `N >= delay` reads exactly what a longer one would.
const AP_TANK_LINE: usize = 2048;
const DEL_TANK_LINE: usize = 4800;
const _: () = assert!(AP_TANK_LENS[1] <= AP_TANK_LINE && DEL_TANK_LENS[1] <= DEL_TANK_LINE);
```

Replace `    lines: [DelayLine<2048>; 4],` with `    lines: [DelayLine<FDN_LINE>; 4],`.

Replace `const FDN_LENS: [usize; 4] = [601, 773, 947, 1123];` with

```rust
const FDN_LENS: [usize; 4] = [601, 773, 947, 1123];
/// Covers the longest line at size 1.0 (`size_scale` = 1.0): 1,123 samples.
const FDN_LINE: usize = 1152;
```

Replace `                let len = len.max(2).min(2047);` with `                let len = len.max(2).min(FDN_LINE - 1);` (only reachable with `size` above the spec's 1.0 max).

In `chimera-core/src/dsp/delay.rs`, replace

```rust
/// Max delay: ~1 second at 48kHz
const MAX_DELAY_SAMPLES: usize = 48000;
```

with

```rust
/// 500 ms at 48 kHz plus headroom for the ±20-sample wow/flutter swing
/// (ADR 0014: the delay's range is 10..500 ms so the FX bus fits AXI).
const MAX_DELAY_SAMPLES: usize = 24_064;
```

replace `    /// Delay time in ms (10..1000)` with `    /// Delay time in ms (10..500)`, and in `DELAY_SPECS` replace

```rust
    ParamSpec::continuous(0, "TIME", ValFmt::Uni, 10.0, 1000.0, 375.0, 8.0, false),
```

with

```rust
    ParamSpec::continuous(0, "TIME", ValFmt::Uni, 10.0, 500.0, 375.0, 8.0, false),
```

- [ ] **Step 8: Run tests to verify they pass and the lock holds**

Run: `cargo test -p chimera-core --test fx_bus_test --test memory_budget_test --test fx_golden_test -- --nocapture`
Expected: all pass; `FxBus = 253744 B, budget 262144 B` (x86_64; 253,612 B on the target); `fx_goldens_match` ok — the trim is bit-identical, including `delay_500ms` at full wow/flutter.
Run: `just check` — Expected: 379 passed / 0 failed / 2 ignored, desktop and firmware `Finished`.

- [ ] **Step 9: Commit**

```bash
git add chimera-core/src/dsp/fx_bus.rs chimera-core/src/dsp/mod.rs chimera-core/src/hw.rs chimera-core/src/dsp/chorus.rs chimera-core/src/dsp/delay.rs chimera-core/src/dsp/reverb.rs chimera-core/tests/fx_golden_test.rs chimera-core/tests/fx_bus_test.rs chimera-core/tests/memory_budget_test.rs
git commit -m "feat(core): FX bus with buffers trimmed to their ranges so it fits AXI

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Rename `Patch` → `Sound`, `Track` → `Part`, `Project` → `Performance`

**Files (all edited by the script in Step 3):**
- `chimera-core/src/{preset,params,addr,block,mod_path}.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/ui/{mod,chain,renderer}.rs`
- `chimera-core/tests/{preset_test,ui_routing_test,binding_test,click_free_test,golden_test,fm_test,sanity_test,modulatable_test,engine_source_test}.rs`, `chimera-core/tests/common/mod.rs`
- `chimera-desktop/src/main.rs`, `chimera-stm32/src/main.rs`

**Interfaces:**
- Produces (pure rename, no behavior change): `preset::Sound` (was `Patch`), `preset::Part` (was `Track`, field `sound` was `patch`), `preset::Performance` (was `Project`, field `parts` was `tracks`), `UiState::performance` (was `project`), `UiState::active_part` (was `active_track`), `UiMode::SoundBrowser { part, cursor, scroll }` (was `PatchBrowser { track, … }`). Display strings (e.g. `"LOAD PATCH: B{}"`) are unchanged.

The mechanical rule:
1. In every `.rs` file under `chimera-core/src`, `chimera-core/tests`, `chimera-desktop/src`, `chimera-stm32/src` that contains one of the words: `PatchBrowser`→`SoundBrowser`, `Patch`→`Sound`, `Track`→`Part`, `Project`→`Performance`, `patch`→`sound`, `project`→`performance` (whole words; not `sub-project`).
2. `tracks`→`parts`, `track`→`part`, `*_track`→`*_part`, `track_*`→`part_*` **only** in the eight files where "track" means the Part (elsewhere it is English: "key track", "tracks how many").
3. The default name literal stays 16 bytes (`NAME_LEN`).

- [ ] **Step 1: Write the failing test**

In `chimera-core/tests/preset_test.rs`, replace

```rust
#[test]
fn project_has_six_tracks() {
    let project = Project::new();
    assert_eq!(project.tracks.len(), 6);
}
```

with

```rust
/// Spec § Vocabulary: a Performance holds MAX_PARTS Parts, each playing a Sound.
#[test]
fn performance_has_six_parts_playing_sounds() {
    let perf = Performance::new();
    assert_eq!(perf.parts.len(), chimera_core::hw::MAX_PARTS);
    let sound: &Sound = &perf.parts[0].sound;
    assert_eq!(sound.chain_type, ChainType::PizzaPoly);
    assert_eq!(&perf.name, b"New Performance\0");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test preset_test`
Expected: FAIL to compile — `cannot find type Performance`, `cannot find type Sound`.

- [ ] **Step 3: Run the rename**

```bash
set -euo pipefail
ALL=$(grep -rlP '\b(Patch|Track|Project|PatchBrowser|patch|project)\b' --include='*.rs' \
  chimera-core/src chimera-core/tests chimera-desktop/src chimera-stm32/src)
perl -pi -e 's/\bPatchBrowser\b/SoundBrowser/g; s/\bPatch\b/Sound/g; s/\bTrack\b/Part/g;
  s/\bProject\b/Performance/g; s/\bpatch\b/sound/g; s/(?<!-)\bproject\b/performance/g' $ALL
TRK="chimera-core/src/preset.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/chain.rs
  chimera-core/src/ui/renderer.rs chimera-core/tests/preset_test.rs chimera-core/tests/ui_routing_test.rs
  chimera-desktop/src/main.rs chimera-stm32/src/main.rs"
perl -pi -e 's/\btracks\b/parts/g; s/\btrack\b/part/g; s/_track\b/_part/g; s/\btrack_/part_/g' $TRK
perl -pi -e 's/\*b"New Performance\\0\\0\\0\\0\\0"/*b"New Performance\\0"/' chimera-core/src/preset.rs
```

(Run it under `bash`; zsh does not word-split `$ALL`.) Review `git diff`: 21 files, 198 lines changed; comments now read "sound browser", "active part", "FM init sound".

- [ ] **Step 4: Run tests to verify they pass**

Run: `just check`
Expected: 379 passed / 0 failed / 2 ignored (goldens unchanged); desktop and firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add -A chimera-core chimera-desktop chimera-stm32
git commit -m "refactor(core): rename Patch→Sound, Track→Part, Project→Performance

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Part mix settings — channel, mode, output, level, pan, sends

**Files:**
- Create: `chimera-core/src/part.rs`
- Modify: `chimera-hal/src/lib.rs` (`MidiChannel`), `chimera-core/src/lib.rs`, `chimera-core/src/preset.rs` (`Part`, `Performance`; delete `MixerState`), `chimera-core/src/ui/mod.rs` (`UiState::pool`, browser init entry)
- Test: `chimera-core/tests/part_test.rs`, `chimera-core/tests/block_test.rs`, `chimera-core/tests/preset_test.rs`

**Interfaces:**
- Consumes: `dsp::fx_bus::FX_SENDS` (Task 3), `hw::MAX_PARTS` (Task 1), `block::{Block, ParamId, ParamSpec, ValFmt}`.
- Produces:
  - `chimera_hal::MidiChannel` (re-exported as `chimera_core::MidiChannel`): `const fn new(u8) -> Option<Self>` (0..=15), `const fn clamped(u8) -> Self`, `const fn get(self) -> u8`.
  - `chimera_core::part::{PartMode { Mono = 0, Poly = 1 }, DacPair { P1 = 0, P2 = 1, P3 = 2 } (+ const fn index(self) -> usize), PartParams, PART_SPECS: [ParamSpec; 8]}`.
  - `PartParams { channel: MidiChannel, mode: PartMode, output: DacPair, level: f32, pan: f32, sends: [f32; FX_SENDS] }`, ids `CHANNEL 0, MODE 1, OUTPUT 2, LEVEL 3, PAN 4, SEND_CHORUS 5, SEND_DELAY 6, SEND_REVERB 7`; `fn for_part(index: usize) -> Self`; `impl Default` (= `for_part(0)`); `impl Block`. Nothing modulatable (ADR 0010: the mixer, not the voice, applies them — the spec's "modulation for free" for level/pan does not hold; see Deviations).
  - `Part { sound: Sound, loaded_from: Option<u8>, mix: PartParams }`, `Part::load_init(&mut self, ChainType)` (replaces the Sound only).
  - `Performance { name, parts: [Part; MAX_PARTS] }` — the `SoundPool` moves out to `UiState::pool` (spec: "SoundPool stays in the UI/Performance side").

The spec lists the six mix fields flat on `Part`; they are grouped in `Part::mix: PartParams` because the spec also makes `PartParams` the `Block` the pages bind to — one struct serves both.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/part_test.rs`:

```rust
//! A Part's mix settings (instrument-core spec § Data model).

use chimera_core::block::Block;
use chimera_core::hw::MAX_PARTS;
use chimera_core::part::{DacPair, PartMode, PartParams};
use chimera_core::preset::{ChainType, Part, Performance};
use chimera_core::MidiChannel;

#[test]
fn midi_channel_accepts_0_to_15_only() {
    assert_eq!(MidiChannel::new(0).map(MidiChannel::get), Some(0));
    assert_eq!(MidiChannel::new(15).map(MidiChannel::get), Some(15));
    assert_eq!(MidiChannel::new(16), None);
    assert_eq!(MidiChannel::clamped(200).get(), 15);
}

/// Part n listens on channel n (0-based), Poly, output P1, level 0.8,
/// centre pan, no sends.
#[test]
fn performance_defaults_per_part() {
    let perf = Performance::new();
    for (n, part) in perf.parts.iter().enumerate() {
        let m = &part.mix;
        assert_eq!(m.channel.get() as usize, n);
        assert_eq!((m.mode, m.output), (PartMode::Poly, DacPair::P1));
        assert_eq!((m.level, m.pan, m.sends), (0.8, 0.0, [0.0; 3]));
    }
    assert_eq!(perf.parts.len(), MAX_PARTS);
}

#[test]
fn enum_params_round_trip_through_the_block() {
    let mut m = PartParams::for_part(0);
    m.set(PartParams::MODE, 0.0);
    assert_eq!(m.mode, PartMode::Mono);
    m.set(PartParams::OUTPUT, 2.0);
    assert_eq!(m.output, DacPair::P3);
    assert_eq!(m.output.index(), 2);
    m.set(PartParams::CHANNEL, 99.0); // clamps to the spec's max
    assert_eq!(m.channel.get(), 15);
    m.nudge(PartParams::SEND_REVERB, 3);
    assert_eq!(m.sends[2], 3.0 / 128.0);
}

/// ADR 0010: level, pan and sends are applied by the mixer, not read by the
/// voice, so none of them is modulatable.
#[test]
fn no_part_param_is_modulatable() {
    assert!(PartParams::default().specs().iter().all(|s| !s.modulatable));
}

/// Loading a Sound replaces only the Sound: channel, mode and mix stay.
#[test]
fn loading_a_sound_keeps_the_mix() {
    let mut part = Part::new(ChainType::PizzaPoly);
    part.mix.channel = MidiChannel::new(9).unwrap();
    part.mix.level = 0.25;
    part.load_init(ChainType::Fm);
    assert_eq!(part.sound.chain_type, ChainType::Fm);
    assert_eq!((part.mix.channel.get(), part.mix.level), (9, 0.25));
    assert_eq!(part.loaded_from, None);
}
```

In `chimera-core/tests/block_test.rs`, add before `#[test]\nfn fx_conform() {`:

```rust
#[test]
fn part_conforms() {
    conforms("part", chimera_core::part::PartParams::default());
}

```

In `chimera-core/tests/preset_test.rs`: replace every `ui.performance.pool` with `ui.pool` (3 places):

```bash
sed -i 's/ui\.performance\.pool/ui.pool/g' chimera-core/tests/preset_test.rs
```

and add before `// ── Priming by slot address ──────────────────────────────────────`:

```rust
/// Loading from the browser replaces the Sound only: the Part keeps its
/// channel and mix (Review Focus: a load must not re-route MIDI).
#[test]
fn browser_load_keeps_part_mix() {
    let mut ui = UiState::new();
    ui.performance.parts[2].mix.channel = chimera_core::MidiChannel::new(9).unwrap();
    ui.performance.parts[2].mix.level = 0.3;
    open_browser(&mut ui, ButtonId::B3);
    ui.handle_input(&MockControls::new().encoder(EncoderId::Main, (POOL_SIZE + 2) as i8));
    ui.handle_input(&MockControls::new().button(ButtonId::Edit, ButtonState::Pressed));
    assert_eq!(ui.performance.parts[2].sound.chain_type, ChainType::Fm);
    assert_eq!(ui.performance.parts[2].mix.channel.get(), 9);
    assert_eq!(ui.performance.parts[2].mix.level, 0.3);
}

```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test part_test --test block_test --test preset_test`
Expected: FAIL to compile — `unresolved import chimera_core::part`, `unresolved import chimera_core::MidiChannel`, `no field mix on type Part`, `no method load_init`, `no field pool on type UiState`.

- [ ] **Step 3: Write minimal implementation**

In `chimera-hal/src/lib.rs`, insert before `/// Note-on velocity, 1..=127.`:

```rust
/// MIDI channel, 0..=15 (the low nibble of the status byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MidiChannel(u8);

impl MidiChannel {
    pub const fn new(c: u8) -> Option<Self> {
        if c <= 15 { Some(Self(c)) } else { None }
    }

    /// `c` limited to 15: for values already clamped by a param spec.
    pub const fn clamped(c: u8) -> Self {
        Self(if c > 15 { 15 } else { c })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

```

In `chimera-core/src/lib.rs`: change `pub use chimera_hal::{MidiNote, Velocity};` to `pub use chimera_hal::{MidiChannel, MidiNote, Velocity};` and add `pub mod part;` after `pub mod params;`.

Create `chimera-core/src/part.rs`:

```rust
//! A Part's mix settings (instrument-core spec § Data model): the MIDI
//! channel it listens on, Mono/Poly, the DAC pair it plays out of, level,
//! pan and FX sends. One `Block`, so it gets pages and snap like any other.

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::fx_bus::FX_SENDS;
use crate::MidiChannel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PartMode {
    /// One voice, retriggered by each note; never stolen.
    Mono = 0,
    /// Voices from the shared pool.
    Poly = 1,
}

impl PartMode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => PartMode::Mono,
            _ => PartMode::Poly,
        }
    }
}

/// One of the three stereo DAC outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DacPair {
    P1 = 0,
    P2 = 1,
    P3 = 2,
}

impl DacPair {
    pub const fn index(self) -> usize {
        self as usize
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => DacPair::P1,
            1 => DacPair::P2,
            _ => DacPair::P3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartParams {
    pub channel: MidiChannel,
    pub mode: PartMode,
    pub output: DacPair,
    /// 0..1
    pub level: f32,
    /// -1 (left) .. 1 (right)
    pub pan: f32,
    /// Chorus, delay, reverb (`FxBus` order), 0..1 each.
    pub sends: [f32; FX_SENDS],
}

impl PartParams {
    pub const CHANNEL: ParamId = ParamId(0);
    pub const MODE: ParamId = ParamId(1);
    pub const OUTPUT: ParamId = ParamId(2);
    pub const LEVEL: ParamId = ParamId(3);
    pub const PAN: ParamId = ParamId(4);
    pub const SEND_CHORUS: ParamId = ParamId(5);
    pub const SEND_DELAY: ParamId = ParamId(6);
    pub const SEND_REVERB: ParamId = ParamId(7);

    /// Part `index` (0-based) listens on channel `index`, Poly, output P1,
    /// level 0.8, centre pan, no sends.
    pub fn for_part(index: usize) -> Self {
        Self {
            channel: MidiChannel::clamped(index as u8),
            mode: PartMode::Poly,
            output: DacPair::P1,
            level: 0.8,
            pan: 0.0,
            sends: [0.0; FX_SENDS],
        }
    }
}

impl Default for PartParams {
    fn default() -> Self {
        Self::for_part(0)
    }
}

/// Nothing here is modulatable: the mixer applies these, not the voice (ADR 0010).
pub static PART_SPECS: [ParamSpec; 8] = [
    ParamSpec::choice(0, "CH", ValFmt::Int(15), 15.0, 0.0),
    ParamSpec::choice(1, "MODE", ValFmt::Int(1), 1.0, 1.0),
    ParamSpec::choice(2, "OUT", ValFmt::Int(2), 2.0, 0.0),
    ParamSpec::continuous(3, "LEVEL", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "PAN", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
    ParamSpec::continuous(5, "CHR", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "DLY", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(7, "REV", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for PartParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &PART_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CHANNEL => self.channel.get() as f32,
            Self::MODE => self.mode as u8 as f32,
            Self::OUTPUT => self.output as u8 as f32,
            Self::LEVEL => self.level,
            Self::PAN => self.pan,
            Self::SEND_CHORUS => self.sends[0],
            Self::SEND_DELAY => self.sends[1],
            Self::SEND_REVERB => self.sends[2],
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CHANNEL => self.channel = MidiChannel::clamped(v as u8),
            Self::MODE => self.mode = PartMode::from_u8(v as u8),
            Self::OUTPUT => self.output = DacPair::from_u8(v as u8),
            Self::LEVEL => self.level = v,
            Self::PAN => self.pan = v,
            Self::SEND_CHORUS => self.sends[0] = v,
            Self::SEND_DELAY => self.sends[1] = v,
            Self::SEND_REVERB => self.sends[2] = v,
            _ => {}
        }
    }
}
```

In `chimera-core/src/preset.rs`: add imports — `use crate::hw::MAX_PARTS;` before `use crate::mod_path::ModDestRegistry;` and `use crate::part::PartParams;` after `use crate::params::{EngineType, ParamSnapshot};`. Replace the `Part` struct and `Part::new`:

```rust
/// A slot playing one Sound, with its MIDI channel, mode, output and mix.
pub struct Part {
    pub sound: Sound,
    pub loaded_from: Option<u8>,
    pub mix: PartParams,
}

impl Part {
    /// An init Sound of `chain_type` with part 1's mix settings.
    pub fn new(chain_type: ChainType) -> Self {
        Self {
            sound: Sound::init(chain_type),
            loaded_from: None,
            mix: PartParams::default(),
        }
    }

    /// Replace the Sound with an init one; channel and mix stay.
    pub fn load_init(&mut self, chain_type: ChainType) {
        self.sound = Sound::init(chain_type);
        self.loaded_from = None;
    }
```

(keep `load_from_pool` and `save_to_pool` as they are), and replace everything from `pub struct MixerState {` to the end of the file with:

```rust
/// All Parts: the whole setup you play and save. The `SoundPool` is not
/// part of it (it stays on the UI side).
pub struct Performance {
    pub name: [u8; NAME_LEN],
    pub parts: [Part; MAX_PARTS],
}

impl Performance {
    pub fn new() -> Self {
        Self {
            name: *b"New Performance\0",
            parts: core::array::from_fn(|i| Part { mix: PartParams::for_part(i), ..Part::new(ChainType::PizzaPoly) }),
        }
    }
}
```

In `chimera-core/src/ui/mod.rs`:
- `sed -i 's/self\.performance\.pool/self.pool/g' chimera-core/src/ui/mod.rs` (4 places);
- `use crate::preset::{Performance, POOL_SIZE};` → `use crate::preset::{Performance, SoundPool, POOL_SIZE};`;
- in `pub struct UiState`, after `pub performance: Performance,` add:

```rust
    /// Saved Sounds that can be loaded into a Part (not part of the Performance).
    pub pool: SoundPool,
```

- in `UiState::new`'s `Self { … }`, after `performance,` add `pool: SoundPool::new(),`;
- in the browser's init-entry branch replace `self.performance.parts[sel_part] = crate::preset::Part::new(init_types[init_idx]);` with `self.performance.parts[sel_part].load_init(init_types[init_idx]);`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `just check` — Expected: 386 passed / 0 failed / 2 ignored; desktop and firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-hal/src/lib.rs chimera-core/src/lib.rs chimera-core/src/part.rs chimera-core/src/preset.rs chimera-core/src/ui/mod.rs chimera-core/tests/part_test.rs chimera-core/tests/block_test.rs chimera-core/tests/preset_test.rs
git commit -m "feat(core): Part mix settings (channel, mode, output, level, pan, sends)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: FX move from the Sound to the Performance; pages resolve through `Blocks`

**Files:**
- Modify: `chimera-core/src/addr.rs` (`Blocks` trait), `chimera-core/src/params.rs` (drop FX fields; `impl Blocks for ParamSnapshot`), `chimera-core/src/preset.rs` (`Performance::fx`, `Performance::edit`, `PartEdit`), `chimera-core/src/dsp/voice.rs`, `chimera-core/src/ui/page.rs`, `chimera-core/src/ui/part_page.rs`, `chimera-core/src/ui/mod.rs`, `chimera-desktop/src/audio.rs`, `chimera-desktop/src/main.rs`
- Test: `chimera-core/tests/page_block_test.rs`, `chimera-core/tests/addr_test.rs`, `chimera-core/tests/click_free_test.rs`, `chimera-core/tests/reverb_test.rs`

**Interfaces:**
- Consumes: `dsp::fx_bus::FxParams` (Task 3), `preset::Part` (Task 5).
- Produces:
  - `pub trait chimera_core::addr::Blocks { fn block(&self, b: BlockRef) -> Option<&dyn Block>; fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block>; }` — replaces the inherent `ParamSnapshot::block(_mut)` (which returned `&dyn Block`). `None` = the address is not held there.
  - `impl Blocks for ParamSnapshot` (Sound blocks; `Chorus`/`Delay`/`Reverb` → `None`).
  - `Performance { name, parts, fx: FxParams }`; `Performance::edit(&mut self, part: usize) -> PartEdit<'_>`; `pub struct PartEdit<'a> { pub part: &'a mut Part, pub fx: &'a mut FxParams }` with `impl Blocks` (FX from `fx`, the rest from the Part's Sound).
  - `PageId::{read_values(&self, &impl Blocks), apply_encoder(&self, usize, i8, &mut impl Blocks), snap_encoder(…)}`; `part_page::{read_values(&BlockDef, &impl Blocks, Op), apply_encoder(&BlockDef, usize, i8, &mut impl Blocks, &mut Op), snap_encoder(&BlockDef, usize, i8, &mut impl Blocks, Op)}` — tests passing `&mut ParamSnapshot` keep compiling.
  - Desktop (temporary, until Task 13): `DesktopAudio::update(&mut self, &ParamSnapshot, &ModState, &FxParams)`.

- [ ] **Step 1: Write the failing tests**

In `chimera-core/tests/page_block_test.rs`, change the import `use chimera_core::addr::{BlockRef, Op, ParamAddr};` to:

```rust
use chimera_core::addr::{BlockRef, Blocks, Op, ParamAddr};
use chimera_core::dsp::delay::DelayParams;
use chimera_core::preset::Performance;
```

and replace the whole `fn fx_encoders_step_like_before()` test with:

```rust
/// FX pages edit the Performance's shared FX (spec § Data model).
#[test]
fn fx_encoders_step_like_before() {
    let mut perf = Performance::new();
    PageId::Delay.apply_encoder(0, 2, &mut perf.edit(0));
    assert_eq!(perf.fx.delay.time_ms, 375.0 + 2.0 * 8.0);
    PageId::Chorus.apply_encoder(0, 5, &mut perf.edit(0));
    assert_eq!(perf.fx.chorus.mode, 3);
    PageId::MixReverb.apply_encoder(0, 5, &mut perf.edit(0));
    assert_eq!(perf.fx.reverb.reverb_type, 2);
    PageId::MixReverb.apply_encoder(4, -1, &mut perf.edit(0));
    assert_eq!(perf.fx.reverb.mix, 0.0);
    PageId::MixReverb.apply_encoder(4, 1, &mut perf.edit(0)); // Efx MIX slot, +1 off the floor
    assert_eq!(perf.fx.reverb.mix, 1.0 / 128.0);
    PageId::Delay.snap_encoder(5, 1, &mut perf.edit(0));
    assert_eq!(perf.fx.delay.mix, 100.0 / 127.0);
}

/// One FX set for every Part: an edit from part 1 is what part 4 sees.
#[test]
fn fx_are_shared_across_parts() {
    let mut perf = Performance::new();
    PageId::Delay.apply_encoder(0, 2, &mut perf.edit(0));
    let seen = perf.edit(3).block(BlockRef::Delay).map(|b| b.get(DelayParams::TIME_MS));
    assert_eq!(seen, Some(391.0));
}

/// A Sound carries no FX blocks any more.
#[test]
fn a_sound_has_no_fx_blocks() {
    let p = ParamSnapshot::default();
    for b in [BlockRef::Chorus, BlockRef::Delay, BlockRef::Reverb] {
        assert!(p.block(b).is_none(), "{b:?}");
    }
}
```

In `chimera-core/tests/addr_test.rs`, change the import to `use chimera_core::addr::{BlockRef, Blocks, Op, OpOutOfRange, ParamAddr};`, add `use chimera_core::preset::Performance;` directly after that `addr` import, replace `fn block_and_specs_agree` (and its doc comment) with:

```rust
/// `block()` hands out the instance whose spec table `BlockRef::specs`
/// names; a Part view resolves every address, a Sound all but the FX.
#[test]
fn block_and_specs_agree() {
    let mut perf = Performance::new();
    let part = perf.edit(0);
    for b in BlockRef::ALL {
        let blk = part.block(b).expect("a Part resolves every block");
        assert!(core::ptr::eq(blk.specs(), b.specs()), "{b:?}");
        let fx = matches!(b, BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb);
        assert_eq!(ParamSnapshot::default().block(b).is_some(), !fx, "{b:?}");
    }
}
```

and in the next test add `.unwrap()` after each `p.block_mut(…)` / `p.block(…)` call (four places: `FmOp(Op::C)`, `FilterEnv`, `Out` set, `Out` get), e.g. `p.block_mut(BlockRef::FmOp(Op::C)).unwrap().set(FmOpParams::LEVEL, 42.0);`.

The FX tests read reverb settings from `FxParams` instead of `ParamSnapshot` (verified to produce exactly the intended text):

```bash
perl -0pi -e '
  s/use chimera_core::dsp::reverb::Reverb;/use chimera_core::dsp::fx_bus::FxParams;\nuse chimera_core::dsp::reverb::{Reverb, ReverbParams};/;
  s/setup: impl FnOnce\(&mut ParamSnapshot\),/setup: impl FnOnce(&mut ParamSnapshot, &mut ReverbParams),/;
  s/    setup\(&mut params\);\n/    let mut rv = FxParams::default().reverb;\n    setup(&mut params, &mut rv);\n/;
  s/(params\.modal\.mode = ResonatorMode::Modal;\n)/$1    let rv = FxParams::default().reverb;\n/;
  s/reverb\.process\(&mut block, &params\.reverb\);/reverb.process(&mut block, &rv);/g;
  s/\|p\| \{(\n\s+\*p = ParamSnapshot::for_engine\(EngineType::Pizza\);\n\s+p\.reverb)/|p, rv| {$1/g;
  s/\|p\| \{/|p, _| {/g;
  s/p\.reverb\./rv./g;
' chimera-core/tests/click_free_test.rs
perl -0pi -e '
  s/let mut params = ParamSnapshot::for_engine\(EngineType::Pizza\);\n(\s+)params\.reverb\.mix = 0\.5;\n\s+params\.reverb\.time = 0\.7;\n/let params = ParamSnapshot::for_engine(EngineType::Pizza);\n$1let mut rv = chimera_core::dsp::fx_bus::FxParams::default().reverb;\n$1rv.mix = 0.5;\n$1rv.time = 0.7;\n/;
  s/let mut params = ParamSnapshot::for_engine\(EngineType::Pizza\);\n(\s+)params\.reverb\.reverb_type = rt;\n\s+params\.reverb\.mix = 0\.8;\n\s+params\.reverb\.time = 0\.6;\n/let params = ParamSnapshot::for_engine(EngineType::Pizza);\n$1let mut rv = chimera_core::dsp::fx_bus::FxParams::default().reverb;\n$1rv.reverb_type = rt;\n$1rv.mix = 0.8;\n$1rv.time = 0.6;\n/;
  s/reverb\.process\(&mut block, &params\.reverb\);/reverb.process(&mut block, &rv);/g;
' chimera-core/tests/reverb_test.rs
```

(The non-reverb `check_no_clicks` cases used to reset the reverb to the snapshot default, mix 0 = bypassed; `FxParams::default()` is the same.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test page_block_test --test addr_test`
Expected: FAIL to compile — `unresolved import chimera_core::addr::Blocks`, `no field fx on type Performance`, `no method named edit`, and `unwrap`/`is_some`/`is_none` not found on `&dyn Block` (the inherent `block()` still returns `&dyn Block`).

- [ ] **Step 3: Write the implementation**

`chimera-core/src/addr.rs`: change `use crate::block::{find_spec, ParamId, ParamSpec};` to `use crate::block::{find_spec, Block, ParamId, ParamSpec};`; change the doc comment above `Chorus,` to `/// Chorus, delay and reverb: the Performance's shared FX bus.`; append:

```rust

/// Resolves block addresses to values (spec § Data model). A Sound's
/// `ParamSnapshot` holds the voice blocks; a Part view (`PartEdit`) adds the
/// shared FX. `None`: the address is not held here.
pub trait Blocks {
    fn block(&self, b: BlockRef) -> Option<&dyn Block>;
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block>;
}
```

`chimera-core/src/params.rs`: `use crate::addr::BlockRef;` → `use crate::addr::{BlockRef, Blocks};`. Delete the three fields `pub reverb: …`, `pub delay: …`, `pub chorus: …` from `ParamSnapshot` and their three initialisers (`delay: …`, `chorus: …`, and the `reverb: crate::dsp::reverb::ReverbParams { … }` literal) from `impl Default for ParamSnapshot`. Replace the inherent `pub fn block(&self, …)` and `pub fn block_mut(&mut self, …)` together with their doc comment (keep `for_engine` and `engine` in the inherent impl, which now ends after `engine`) with:

```rust
/// The one exhaustive dispatch from a block address to a Sound's values
/// (spec §2). UI and modulation go through this; DSP reads fields. The FX
/// belong to the Performance, not the Sound.
impl Blocks for ParamSnapshot {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        Some(match b {
            BlockRef::Pizza => &self.pizza,
            BlockRef::Modal => &self.modal,
            BlockRef::Fm => &self.fm,
            BlockRef::FmOp(op) => &self.fm.operators[op.index()],
            BlockRef::Drive => &self.drive,
            BlockRef::Filter => &self.filter,
            BlockRef::Folder => &self.folder,
            BlockRef::AmpEnv => &self.envelopes[0],
            BlockRef::FilterEnv => &self.envelopes[1],
            BlockRef::AuxEnv => &self.envelopes[2],
            BlockRef::Lfo => &self.lfo,
            BlockRef::Out => &self.out,
            BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb => return None,
        })
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        Some(match b {
            BlockRef::Pizza => &mut self.pizza,
            BlockRef::Modal => &mut self.modal,
            BlockRef::Fm => &mut self.fm,
            BlockRef::FmOp(op) => &mut self.fm.operators[op.index()],
            BlockRef::Drive => &mut self.drive,
            BlockRef::Filter => &mut self.filter,
            BlockRef::Folder => &mut self.folder,
            BlockRef::AmpEnv => &mut self.envelopes[0],
            BlockRef::FilterEnv => &mut self.envelopes[1],
            BlockRef::AuxEnv => &mut self.envelopes[2],
            BlockRef::Lfo => &mut self.lfo,
            BlockRef::Out => &mut self.out,
            BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb => return None,
        })
    }
}
```

`chimera-core/src/dsp/voice.rs`: add `use crate::addr::Blocks;` above `use crate::block::apply_offset;`, and replace

```rust
                let a = mod_state.dest(d);
                apply_offset(m.block_mut(a.block), a.param, off);
```

with

```rust
                let a = mod_state.dest(d);
                // Modulatable addresses are always Sound blocks (`voice_reads`).
                if let Some(blk) = m.block_mut(a.block) {
                    apply_offset(blk, a.param, off);
                }
```

`chimera-core/src/preset.rs`: add imports above `use crate::hw::MAX_PARTS;`:

```rust
use crate::addr::{BlockRef, Blocks};
use crate::block::Block;
use crate::dsp::fx_bus::FxParams;
```

and replace the `Performance` struct, its doc comment and its impl with:

```rust
/// All Parts + FX: the whole setup you play and save. The `SoundPool` is
/// not part of it (it stays on the UI side).
pub struct Performance {
    pub name: [u8; NAME_LEN],
    pub parts: [Part; MAX_PARTS],
    /// Chorus, delay and reverb: shared by every Part, not per Sound.
    pub fx: FxParams,
}

impl Performance {
    pub fn new() -> Self {
        Self {
            name: *b"New Performance\0",
            parts: core::array::from_fn(|i| Part { mix: PartParams::for_part(i), ..Part::new(ChainType::PizzaPoly) }),
            fx: FxParams::default(),
        }
    }

    /// Part `part` as the pages edit it: its Sound plus the shared FX.
    pub fn edit(&mut self, part: usize) -> PartEdit<'_> {
        PartEdit { part: &mut self.parts[part], fx: &mut self.fx }
    }
}

/// One Part and the Performance's FX, borrowed together so a page can
/// address any block by `BlockRef`.
pub struct PartEdit<'a> {
    pub part: &'a mut Part,
    pub fx: &'a mut FxParams,
}

impl Blocks for PartEdit<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        match b {
            BlockRef::Chorus => Some(&self.fx.chorus),
            BlockRef::Delay => Some(&self.fx.delay),
            BlockRef::Reverb => Some(&self.fx.reverb),
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Out => self.part.sound.params.block(b),
        }
    }

    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        match b {
            BlockRef::Chorus => Some(&mut self.fx.chorus),
            BlockRef::Delay => Some(&mut self.fx.delay),
            BlockRef::Reverb => Some(&mut self.fx.reverb),
            BlockRef::Pizza
            | BlockRef::Modal
            | BlockRef::Fm
            | BlockRef::FmOp(_)
            | BlockRef::Drive
            | BlockRef::Filter
            | BlockRef::Folder
            | BlockRef::AmpEnv
            | BlockRef::FilterEnv
            | BlockRef::AuxEnv
            | BlockRef::Lfo
            | BlockRef::Out => self.part.sound.params.block_mut(b),
        }
    }
}
```

`chimera-core/src/ui/page.rs`: import `use crate::addr::{BlockRef, Blocks, Op, ParamAddr};`, drop `ParamSnapshot` from the `crate::params` import, and replace `read_values`, `apply_encoder`, `snap_encoder` of `impl PageId` with:

```rust
    /// Read 6 normalized (0..1) encoder values from params for this page.
    pub fn read_values(&self, params: &impl Blocks) -> [f32; 6] {
        core::array::from_fn(|i| match (self, i) {
            // Mixer bars for the unbound VOICES and PITCH slots.
            (PageId::Mixer, 2 | 4) => 0.5,
            _ => self
                .binding(i)
                .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
                .unwrap_or(0.0),
        })
    }

    /// Apply an encoder delta: `delta` ticks of the bound param's spec step.
    pub fn apply_encoder(&self, idx: usize, delta: i8, params: &mut impl Blocks) {
        if let Some(a) = self.binding(idx)
            && let Some(b) = params.block_mut(a.block)
        {
            b.nudge(a.param, delta);
        }
    }

    /// Shift+encoder: snap to the coarse points of the bound param's format.
    pub fn snap_encoder(&self, idx: usize, delta: i8, params: &mut impl Blocks) {
        if let Some(a) = self.binding(idx)
            && let Some(b) = params.block_mut(a.block)
        {
            b.snap(a.param, delta);
        }
    }
```

`chimera-core/src/ui/part_page.rs`: replace the imports `use crate::addr::Op;` + `use crate::params::ParamSnapshot;` with `use crate::addr::{Blocks, Op};` and the three functions with:

```rust
/// Normalized (0..1) display values of the six slots.
pub fn read_values(def: &BlockDef, params: &impl Blocks, sel_op: Op) -> [f32; 6] {
    core::array::from_fn(|i| match def.params[i].binding {
        SlotBinding::SelectOp => sel_op.index() as f32 / 3.0,
        _ => slot_addr(def, i, sel_op)
            .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
            .unwrap_or(0.0),
    })
}

/// One encoder turn on `slot`: steps the bound param, or the operator
/// selection for the `SelectOp` slot.
pub fn apply_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, sel_op: &mut Op) {
    if def.params.get(slot).is_some_and(|s| s.binding == SlotBinding::SelectOp) {
        *sel_op = sel_op.nudged(delta);
    } else if let Some(a) = slot_addr(def, slot, *sel_op)
        && let Some(b) = params.block_mut(a.block)
    {
        b.nudge(a.param, delta);
    }
}

/// Shift+encoder on `slot`: snap the bound param (the selector does not snap).
pub fn snap_encoder(def: &BlockDef, slot: usize, delta: i8, params: &mut impl Blocks, sel_op: Op) {
    if let Some(a) = slot_addr(def, slot, sel_op)
        && let Some(b) = params.block_mut(a.block)
    {
        b.snap(a.param, delta);
    }
}
```

`chimera-core/src/ui/mod.rs` — every page read/write goes through `self.performance.edit(part)`:
- import `use crate::addr::{BlockRef, Blocks, Op, ParamAddr};`
- in `new()`: `let performance = Performance::new();` → `let mut performance = Performance::new();`, and `…page_values(page, nav.active_block_def(), &performance.parts[0].sound.params, Op::A));` → `…page_values(page, nav.active_block_def(), &performance.edit(0), Op::A));`
- in `enter_page`: `page_values(self.page, self.nav.active_block_def(), self.params(), self.sel_op)` → `page_values(self.page, self.nav.active_block_def(), &self.performance.edit(self.active_part), self.sel_op)`
- in `handle_input`'s encoder loop: `let params = &mut self.performance.parts[at].sound.params;` → `let params = &mut self.performance.edit(at);`
- in `update()`, replace

```rust
        let at = self.active_part;
        let sound = &self.performance.parts[at].sound;

        // Read base param values
        let def = self.nav.active_block_def();
        let mut values = page_values(self.page, def, &sound.params, self.sel_op);
```

with

```rust
        let at = self.active_part;

        // Read base param values
        let def = self.nav.active_block_def();
        let mut values = page_values(self.page, def, &self.performance.edit(at), self.sel_op);
        let sound = &self.performance.parts[at].sound;
```

- `fn page_values(page: PageKey, def: &BlockDef, params: &ParamSnapshot, sel_op: Op)` → `fn page_values(page: PageKey, def: &BlockDef, params: &impl Blocks, sel_op: Op)`.

Desktop (keeps playing Part 1 with the shared FX until Task 13) — `chimera-desktop/src/audio.rs`: add `use chimera_core::dsp::fx_bus::FxParams;`; add `fx: FxParams,` to `struct AudioShared`; destructure `let AudioShared { params, mod_state, fx } = unsafe { &*current };`; use `&fx.chorus`, `&fx.delay`, `&fx.reverb` in the three FX calls; change `update` to:

```rust
    /// Push params, modulation routes and FX to the audio thread (lock-free swap).
    pub fn update(&mut self, params: &ParamSnapshot, mod_state: &ModState, fx: &FxParams) {
        let inactive = 1 - self.active_buf;
        let buf = &mut self.bufs[inactive];
        buf.params = params.clone();
        buf.mod_state = mod_state.clone();
        buf.fx = *fx;
        let ptr = buf as *mut AudioShared;
        self.shared.current.store(ptr, Ordering::Release);
        self.active_buf = inactive;
    }
```

`chimera-desktop/src/main.rs`: `audio.update(&sound.params, &sound.mod_state);` → `audio.update(&sound.params, &sound.mod_state, &ui.performance.fx);`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `just check` — Expected: 388 passed / 0 failed / 2 ignored (goldens unchanged: `Voice` only ever modulated Sound blocks); desktop and firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-core chimera-desktop
git commit -m "refactor(core): FX move from the Sound to the Performance; pages resolve through Blocks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Mixer chain edits the Part's mix settings and the shared FX

**Files:**
- Modify: `chimera-core/src/addr.rs` (`BlockRef::Part`), `chimera-core/src/params.rs`, `chimera-core/src/preset.rs` (`PartEdit`), `chimera-core/src/ui/block_registry.rs` (PART, SENDS, CHORUS, DELAY, EFX bindings; Mixer chain), `chimera-core/src/ui/page.rs` (drop the mis-bound legacy Mixer pages), `chimera-core/src/ui/mod.rs` (MIX + B<n> selects Part n)
- Test: `chimera-core/tests/mixer_page_test.rs` (new); update `page_block_test.rs`, `region_tests.rs`, `binding_test.rs`, `block_def_tests.rs`, `ui_routing_test.rs`, `addr_test.rs`

**Interfaces:**
- Consumes: `part::PartParams` + ids (Task 5), `Blocks`/`PartEdit` (Task 6).
- Produces: `BlockRef::Part` (in `BlockRef::ALL`, now 19 entries; `specs()` = `PART_SPECS`; `voice_reads()` = false); `ui::block_registry::PART` (id 27, replaces `CHANNEL`); `MIXER_CHANNEL_CHAIN` = `[PART, SENDS, CHORUS, DELAY, EFX]`; `PageId` loses `Mixer, Chorus, Delay, MixReverb, Master` (Mixer-chain pages are `PageKey::Part { def, op }`, slot-bound); navigating to `ChainId::Mixer(i)` sets `active_part = i`.

Resolved spec gaps: MIX + B6 stays the Demo chain, so Part 6's PART/SENDS pages are unreachable from the buttons in this sub-project (its defaults apply). The Chorus/Delay/Reverb pages move onto the Mixer chain (they were reachable only through the mis-bound Mixer nodes). `ValFmt::Int(15)` shows the channel 0-based (0–15) like the code; a 1–16 display is a later `ValFmt` change. The Sound's own `Out` volume is no longer on any mixer page (it was reachable only through the mis-bound Mixer/Master pages; the Demo "Shapes" page still binds it).

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/mixer_page_test.rs`:

```rust
//! Mixer chain (instrument-core spec § UI): MIX + B<n> opens Part n's PART
//! and SENDS pages and the shared FX pages, all bound through slot bindings.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::part::{DacPair, PartMode, PartParams};
use chimera_core::preset::Performance;
use chimera_core::ui::block_def::{BlockDef, SlotBinding};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::page::PageKey;
use chimera_core::ui::{part_page, UiState};
use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

struct MockControls {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl MockControls {
    fn new() -> Self {
        Self { buttons: Vec::new(), encoders: Vec::new() }
    }
    fn button(mut self, id: ButtonId, state: ButtonState) -> Self {
        self.buttons.push((id, state));
        self
    }
    fn encoder(mut self, id: EncoderId, delta: i8) -> Self {
        self.encoders.push((id, delta));
        self
    }
}

impl Controls for MockControls {
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
}

fn open_mixer(ui: &mut UiState, b: ButtonId) {
    ui.handle_input(&MockControls::new().button(ButtonId::Mix, ButtonState::Held).button(b, ButtonState::Pressed));
}

fn turn(ui: &mut UiState, enc: EncoderId, delta: i8) {
    ui.handle_input(&MockControls::new().encoder(enc, delta));
}

#[test]
fn mixer_chain_is_part_sends_and_fx() {
    let names: Vec<&str> = reg::MIXER_CHANNEL_CHAIN.blocks.iter().map(|b| b.def.name).collect();
    assert_eq!(names, ["Part", "Sends", "Chorus", "Delay", "Reverb"]);
}

/// Every slot on the Mixer chain is bound to a real spec: no Legacy slot is
/// left to edit the wrong block.
#[test]
fn every_mixer_slot_is_bound() {
    for block in reg::MIXER_CHANNEL_CHAIN.blocks {
        for (i, slot) in block.def.params.iter().enumerate() {
            match slot.binding {
                SlotBinding::Empty => {}
                SlotBinding::Param(a) => assert!(a.spec().is_some(), "{} slot {i}", block.def.name),
                other => panic!("{} slot {i}: {other:?}", block.def.name),
            }
        }
    }
}

#[test]
fn part_page_binds_channel_mode_output_level_pan() {
    let at = |i: usize| match reg::PART.params[i].binding {
        SlotBinding::Param(a) => a,
        other => panic!("slot {i}: {other:?}"),
    };
    let ids = [PartParams::CHANNEL, PartParams::MODE, PartParams::OUTPUT, PartParams::LEVEL, PartParams::PAN];
    for (i, id) in ids.into_iter().enumerate() {
        assert_eq!(at(i), ParamAddr::new(BlockRef::Part, id));
    }
}

/// MIX + B2 selects Part 2 and its encoders edit Part 2's mix settings.
#[test]
fn mix_b2_edits_part_2() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B2);
    assert_eq!(ui.active_part, 1);
    assert!(matches!(ui.page(), PageKey::Part { def: 27, .. }));
    turn(&mut ui, EncoderId::A, 3); // CH 1 → 4
    turn(&mut ui, EncoderId::B, -1); // Poly → Mono
    turn(&mut ui, EncoderId::C, 2); // P1 → P3
    turn(&mut ui, EncoderId::D, -8); // level
    let m = &ui.performance.parts[1].mix;
    assert_eq!((m.channel.get(), m.mode, m.output), (4, PartMode::Mono, DacPair::P3));
    assert_eq!(m.level, 0.8 - 8.0 / 128.0);
    assert_eq!(ui.performance.parts[0].mix, PartParams::for_part(0), "part 1 untouched");
}

#[test]
fn sends_page_edits_the_part_sends() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B3);
    ui.handle_input(&MockControls::new().button(ButtonId::Plus, ButtonState::Pressed)); // → SENDS
    turn(&mut ui, EncoderId::C, 64); // reverb send
    assert_eq!(ui.performance.parts[2].mix.sends, [0.0, 0.0, 0.5]);
}

fn turn_def(def: &BlockDef, slot: usize, delta: i8, perf: &mut Performance) {
    part_page::apply_encoder(def, slot, delta, &mut perf.edit(0), &mut Op::A);
}

/// FX pages keep the old encoder steps and edit the Performance's FX.
#[test]
fn fx_encoders_step_like_before() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    assert_eq!(perf.fx.delay.time_ms, 375.0 + 2.0 * 8.0);
    turn_def(&reg::CHORUS, 0, 5, &mut perf);
    assert_eq!(perf.fx.chorus.mode, 3);
    turn_def(&reg::EFX, 0, 5, &mut perf);
    assert_eq!(perf.fx.reverb.reverb_type, 2);
    turn_def(&reg::EFX, 4, -1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 0.0);
    turn_def(&reg::EFX, 4, 1, &mut perf);
    assert_eq!(perf.fx.reverb.mix, 1.0 / 128.0);
    part_page::snap_encoder(&reg::DELAY, 5, 1, &mut perf.edit(0), Op::A);
    assert_eq!(perf.fx.delay.mix, 100.0 / 127.0);
}

/// One FX set for every Part: an edit from part 1 is what part 4 shows.
#[test]
fn fx_are_shared_across_parts() {
    let mut perf = Performance::new();
    turn_def(&reg::DELAY, 0, 2, &mut perf);
    let shown = part_page::read_values(&reg::DELAY, &perf.edit(3), Op::A)[0];
    assert_eq!(shown, (391.0 - 10.0) / (500.0 - 10.0));
}

/// ADR 0010: mix settings are not modulatable, so MIX + Plus primes nothing.
#[test]
fn priming_a_part_param_is_refused() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B1);
    turn(&mut ui, EncoderId::D, 1); // focus LEVEL
    ui.handle_input(&MockControls::new().button(ButtonId::Mix, ButtonState::Held).button(ButtonId::Plus, ButtonState::Pressed));
    assert!(ui.performance.parts[0].sound.dest_registry.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test mixer_page_test`
Expected: FAIL to compile — `cannot find value PART in module reg`, `no variant … named Part found for enum BlockRef`.

- [ ] **Step 3: Write the implementation**

`chimera-core/src/addr.rs`:
- after `    Reverb,` in `enum BlockRef` add

```rust
    /// A Part's mix settings (`PartParams`): channel, mode, output, level,
    /// pan, sends.
    Part,
```

- `pub const ALL: [BlockRef; 18]` → `pub const ALL: [BlockRef; 19]`, adding `BlockRef::Part,` after `BlockRef::Reverb,`;
- in `specs()` add the arm `BlockRef::Part => &crate::part::PART_SPECS,`;
- in `voice_reads()` extend the `false` arm: `| BlockRef::Reverb\n            | BlockRef::Part => false,`.

`chimera-core/src/params.rs`: in both arms `BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb => return None,` add `| BlockRef::Part`, and change the impl's doc comment last sentence to `The FX\n/// and the mix settings belong to the Performance and Part, not the Sound.`

`chimera-core/src/preset.rs` (`impl Blocks for PartEdit`): after `BlockRef::Reverb => Some(&self.fx.reverb),` add `BlockRef::Part => Some(&self.part.mix),`; after `BlockRef::Reverb => Some(&mut self.fx.reverb),` add `BlockRef::Part => Some(&mut self.part.mix),`; doc comment of `PartEdit`: `/// One Part (Sound + mix settings) and the Performance's FX, borrowed\n/// together so a page can address any block by \`BlockRef\`.`

`chimera-core/src/ui/block_registry.rs` — imports become:

```rust
use crate::addr::{BlockRef, Op};
use crate::dsp::chorus::ChorusParams;
use crate::dsp::delay::DelayParams;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::ModalParams;
use crate::dsp::pizza::PizzaParams;
use crate::dsp::reverb::ReverbParams;
use crate::part::PartParams;
use crate::params::{DriveParams, EnvParams, FilterParams, FmOpParams, FmParams, FolderParams, OutParams};
```

`EFX` params:

```rust
    params: [
        ParamSlot::param(BlockRef::Reverb, ReverbParams::REVERB_TYPE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::TIME, CellIcon::Arc),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::DAMPING, CellIcon::Arc),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::SIZE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Reverb, ReverbParams::MIX, CellIcon::Arc),
        EMPTY,
    ],
```

`CHORUS` params:

```rust
    params: [
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MODE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::RATE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::DEPTH, CellIcon::Arc),
        ParamSlot::param(BlockRef::Chorus, ChorusParams::MIX, CellIcon::Arc),
        EMPTY,
        EMPTY,
    ],
```

`DELAY` params:

```rust
    params: [
        ParamSlot::param(BlockRef::Delay, DelayParams::TIME_MS, CellIcon::Arc),
        ParamSlot::param(BlockRef::Delay, DelayParams::FEEDBACK, CellIcon::Arc),
        ParamSlot::param(BlockRef::Delay, DelayParams::WOW_FLUTTER, CellIcon::Arc),
        ParamSlot::param(BlockRef::Delay, DelayParams::SATURATION, CellIcon::Arc),
        ParamSlot::param(BlockRef::Delay, DelayParams::TONE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Delay, DelayParams::MIX, CellIcon::Arc),
    ],
```

(labels and formats come from the specs and equal the old legacy ones.) Replace the whole `pub static CHANNEL: BlockDef = … ;` with:

```rust
/// A Part's MIDI channel, mode, output, level and pan (spec § UI).
pub static PART: BlockDef = BlockDef {
    id: 27,
    name: "Part",
    short: "PRT",
    layout: PageLayout::CellGrid,
    viz: VizType::MixerLevels,
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::CHANNEL, CellIcon::Arc),
        ParamSlot::param(BlockRef::Part, PartParams::MODE, CellIcon::Arc),
        ParamSlot::param(BlockRef::Part, PartParams::OUTPUT, CellIcon::Arc),
        ParamSlot::param(BlockRef::Part, PartParams::LEVEL, CellIcon::LevelBar),
        ParamSlot::param(BlockRef::Part, PartParams::PAN, CellIcon::PanDot),
        EMPTY,
    ],
};
```

`SENDS` params:

```rust
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::SEND_CHORUS, CellIcon::Arc),
        ParamSlot::param(BlockRef::Part, PartParams::SEND_DELAY, CellIcon::Arc),
        ParamSlot::param(BlockRef::Part, PartParams::SEND_REVERB, CellIcon::Arc),
        EMPTY,
        EMPTY,
        EMPTY,
    ],
```

and the chain:

```rust
/// MIX + B<n>: Part n's mix settings, then the shared FX (spec § UI).
static MIXER_CHANNEL_BLOCKS: [ChainBlock; 5] = [
    ChainBlock { def: &PART,   sub_pages: &[] },
    ChainBlock { def: &SENDS,  sub_pages: &[] },
    ChainBlock { def: &CHORUS, sub_pages: &[] },
    ChainBlock { def: &DELAY,  sub_pages: &[] },
    ChainBlock { def: &EFX,    sub_pages: &[] },
];
```

(`MIDI_CFG`, `EQ`, `MIXER`, `MASTER` and `MIX_CHAIN` stay defined, unreferenced by navigation, as before.)

`chimera-core/src/ui/page.rs`:
- imports: drop `use crate::dsp::chorus::ChorusParams;`, `use crate::dsp::delay::DelayParams;`, `use crate::dsp::reverb::ReverbParams;`;
- `enum PageId`: delete `Mixer, Chorus, Delay, MixReverb, Master,`; doc comment: `/// Pages still driven by \`PageId\`: System and Demo (spec §5). Part- and\n/// Mixer-chain pages are identified by \`PageKey::Part\` and driven by their\n/// \`BlockDef\` slot bindings (\`ui::part_page\`).`;
- `PageKey::Part` doc: `/// A slot-bound page (Part or Mixer chain) by \`BlockDef::id\` (defs like\n/// FILTER are shared across chains), with the FM operator selection so a\n/// selection change redraws the page.`; `PageKey::Legacy` doc: `/// System/Demo pages.`;
- `PageId::from_nav`: replace the `ChainId::Part(_) => return None,` arm and the whole `ChainId::Mixer(_) => match nav.node { … },` arm with `ChainId::Part(_) | ChainId::Mixer(_) => return None,` and its doc comment with `/// The legacy page at the current navigation position; \`None\` on a\n/// slot-bound Part or Mixer chain (see \`PageKey::from_nav\`).`;
- `binding`: delete the four arms `PageId::Mixer | PageId::Master => …`, `PageId::Chorus => …`, `PageId::Delay => …`, `PageId::MixReverb => …`;
- `read_values` loses the Mixer placeholder arm:

```rust
    pub fn read_values(&self, params: &impl Blocks) -> [f32; 6] {
        core::array::from_fn(|i| {
            self.binding(i)
                .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
                .unwrap_or(0.0)
        })
    }
```

- delete the consts `OUT_PAGE`, `CHORUS_PAGE`, `DELAY_PAGE`, `REVERB_PAGE`.

`chimera-core/src/ui/mod.rs`: replace

```rust
            // Update active_part when navigating to a Part
            if let ChainId::Part(i) = self.nav.chain_id {
```

with

```rust
            // B<n> and MIX + B<n> both select Part n for editing.
            if let ChainId::Part(i) | ChainId::Mixer(i) = self.nav.chain_id {
```

and the doc of `current_param_addr` with `/// The address the focused encoder edits, if its slot is bound. System\n/// and Demo slots are \`Legacy\`, so priming there does nothing; Mixer\n/// params are bound but not modulatable, so the registry refuses them.`

Existing tests that named the removed pages:
- `chimera-core/tests/region_tests.rs`: `sed -i 's/PageId::Mixer/PageId::DemoWaves/g'` (it only needs some legacy page);
- `chimera-core/tests/page_block_test.rs`: module doc `//! Legacy (\`PageId\`) pages — System, Demo — after moving onto`; delete `out_encoders_step_like_before`, `mixer_read_values_keep_placeholders`, and the two FX tests added in Task 6 (they now live in `mixer_page_test.rs`); drop the now-unused imports `DelayParams`, `Performance`, `OutParams`; in `legacy_bindings_name_semantic_addresses` delete the `PageId::Master.binding(0)` and `PageId::Mixer.binding(2)` asserts; in `every_legacy_binding_has_a_spec` delete the line `PageId::Mixer, PageId::Chorus, PageId::Delay, PageId::MixReverb, PageId::Master,`;
- `chimera-core/tests/binding_test.rs`: `&reg::CHANNEL` → `&reg::PART` in `block_def_ids_are_unique`;
- `chimera-core/tests/block_def_tests.rs`: `mixer_channel_strip_chain` becomes

```rust
#[test]
fn mixer_channel_strip_chain() {
    let chain = &block_registry::MIXER_CHANNEL_CHAIN;
    assert_eq!(chain.blocks[0].def.name, "Part");
    assert_eq!(chain.blocks[1].def.name, "Sends");
    assert_eq!(chain.len(), 5);
}
```

- `chimera-core/tests/ui_routing_test.rs`: doc of `priming_on_legacy_page_registers_nothing` → `/// Review Focus 1: priming on the Mixer (bound, not modulatable) or System\n/// (Legacy) chain must not register anything (it used to register\n/// \`Block{node,i}\`, which the voice read as a Pizza/Drive/Filter/Folder param).`
- `chimera-core/tests/addr_test.rs` (`block_and_specs_agree`): a Sound does not hold the Part's mix either:

```rust
/// `block()` hands out the instance whose spec table `BlockRef::specs`
/// names; a Part view resolves every address, a Sound all but the FX and
/// the Part's own mix settings.
#[test]
fn block_and_specs_agree() {
    let mut perf = Performance::new();
    let part = perf.edit(0);
    for b in BlockRef::ALL {
        let blk = part.block(b).expect("a Part resolves every block");
        assert!(core::ptr::eq(blk.specs(), b.specs()), "{b:?}");
        let not_in_sound = matches!(b, BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb | BlockRef::Part);
        assert_eq!(ParamSnapshot::default().block(b).is_some(), !not_in_sound, "{b:?}");
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `just check` — Expected: 392 passed / 0 failed / 2 ignored; desktop and firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-core
git commit -m "feat(ui): Mixer chain edits the Part's mix settings and the shared FX

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Voice allocator (round-robin, steal oldest, mono, CPU budget)

**Files:**
- Create: `chimera-core/src/voice_alloc.rs`
- Modify: `chimera-core/src/lib.rs`
- Test: `chimera-core/tests/voice_alloc_test.rs`

**Interfaces:**
- Consumes: `hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES}` (Task 1), `part::PartMode` (Task 5), `MidiNote`.
- Produces: `chimera_core::voice_alloc::{VoiceSlot, Alloc { Voice(usize), Refused }, Allocator}`:
  - `VoiceSlot` getters `part() -> Option<u8>`, `note() -> Option<MidiNote>`, `held() -> bool`, `is_free() -> bool`, `cost() -> Cost` (fields private, spec's `part/note/age/held` plus `mono` and `cost`).
  - `Allocator::new()`, `slots(&self) -> &[VoiceSlot; MAX_VOICES]`, `refused(&self) -> u32`, `sounding_cost(&self) -> Cost`,
  - `note_on(&mut self, part: u8, mode: PartMode, note: MidiNote, cost: Cost, reserved: Cost) -> Alloc`,
  - `note_off(&mut self, part: u8, note: MidiNote) -> Option<usize>`,
  - `release_finished(&mut self, voice: usize)` (frees only a released voice),
  - `recost(&mut self, voice: usize, cost: Cost)`, `shed(&mut self, reserved: Cost) -> Option<usize>`.

Deviation from the spec's signature: the spec passes `sounding_cost: Cost` into `note_on`. The allocator needs each voice's cost anyway (rule 4 must know what a steal frees), so it stores it per slot and sums it itself; the caller passes only `reserved` — the cost outside the pool (the FX bus, `FxBus::COST`). One source of truth, no drift. Rule 4 steals at most **one** voice (the oldest non-mono) and refuses without stealing if that does not make room. `recost`/`shed` cover Review Focus 3 (a Sound change mid-chord).

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/voice_alloc_test.rs`:

```rust
//! Digitone-style voice allocation (instrument-core spec § Voice allocation).

use chimera_core::hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES};
use chimera_core::part::PartMode::{self, Mono, Poly};
use chimera_core::voice_alloc::{Alloc, Allocator};
use chimera_core::MidiNote;

const FM: Cost = Cost(610);
const NONE: Cost = Cost::ZERO;

fn n(v: u8) -> MidiNote {
    MidiNote::new(v).unwrap()
}

fn on(a: &mut Allocator, part: u8, mode: PartMode, note: u8) -> Alloc {
    a.note_on(part, mode, n(note), FM, NONE)
}

fn voice(r: Alloc) -> usize {
    match r {
        Alloc::Voice(v) => v,
        Alloc::Refused => panic!("refused"),
    }
}

#[test]
fn poly_takes_free_voices_round_robin() {
    let mut a = Allocator::new();
    let got: Vec<usize> = (0..3).map(|i| voice(on(&mut a, 0, Poly, 60 + i))).collect();
    assert_eq!(got, [0, 1, 2]);
    a.note_off(0, n(60));
    a.release_finished(0);
    // The next notes continue after the last voice used, wrapping, instead
    // of reusing the just-freed voice 0.
    let got: Vec<usize> = (70..74).map(|i| voice(on(&mut a, 0, Poly, i))).collect();
    assert_eq!(got, [3, 4, 5, 0]);
}

#[test]
fn full_pool_steals_the_oldest_voice_across_parts() {
    let mut a = Allocator::new();
    for i in 0..MAX_VOICES as u8 {
        on(&mut a, i % 2, Poly, 60 + i); // parts 0 and 1 interleaved
    }
    let v = voice(on(&mut a, 2, Poly, 90)); // part 2, pool full
    assert_eq!(v, 0, "voice 0 (part 0, note 60) is the oldest");
    assert_eq!((a.slots()[0].part(), a.slots()[0].note()), (Some(2), Some(n(90))));
}

#[test]
fn mono_voices_are_never_stolen() {
    let mut a = Allocator::new();
    let mono = voice(on(&mut a, 0, Mono, 40)); // oldest voice, but mono
    for i in 1..MAX_VOICES as u8 {
        on(&mut a, 1, Poly, 60 + i);
    }
    let stolen = voice(on(&mut a, 2, Poly, 90));
    assert_ne!(stolen, mono);
    assert_eq!(a.slots()[mono].note(), Some(n(40)));
}

#[test]
fn refuses_when_every_voice_is_mono() {
    let mut a = Allocator::new();
    for p in 0..MAX_VOICES as u8 {
        on(&mut a, p, Mono, 60);
    }
    assert_eq!(on(&mut a, 0, Poly, 61), Alloc::Refused);
    assert_eq!(a.refused(), 1);
}

#[test]
fn mono_retrigger_reuses_its_voice() {
    let mut a = Allocator::new();
    let v = voice(on(&mut a, 3, Mono, 60));
    assert_eq!(voice(on(&mut a, 3, Mono, 64)), v);
    assert_eq!(a.slots()[v].note(), Some(n(64)));
    assert_eq!(a.slots().iter().filter(|s| s.part() == Some(3)).count(), 1);
    // Releasing the first note does nothing: the voice now plays 64.
    assert_eq!(a.note_off(3, n(60)), None);
    assert!(a.slots()[v].held());
}

#[test]
fn note_off_releases_only_the_matching_part_and_note() {
    let mut a = Allocator::new();
    let v0 = voice(on(&mut a, 0, Poly, 60));
    let v1 = voice(on(&mut a, 1, Poly, 60)); // same note, other part
    assert_eq!(a.note_off(1, n(60)), Some(v1));
    assert!(a.slots()[v0].held());
    assert!(!a.slots()[v1].held());
    assert_eq!(a.note_off(1, n(60)), None, "already released");
}

/// Rule 5: a released voice keeps its slot (its tail rings) until the
/// engine reports inactive; a held voice is never freed that way.
#[test]
fn tails_keep_the_voice_until_finished() {
    let mut a = Allocator::new();
    let v = voice(on(&mut a, 0, Poly, 60));
    a.release_finished(v); // still held: ignored
    assert_eq!(a.slots()[v].part(), Some(0));
    a.note_off(0, n(60));
    assert_eq!(a.slots()[v].part(), Some(0), "tail rings");
    a.release_finished(v);
    assert!(a.slots()[v].is_free());
}

/// Rule 4: over budget, steal one voice if that makes room, else refuse
/// without stealing.
#[test]
fn cpu_budget_steals_or_refuses() {
    let modal = Cost(1_210);
    let mut a = Allocator::new();
    let fx = Cost(1_000);
    // 4 Modal voices + FX = 5,840; a 5th would be 7,050 > 7,000.
    for i in 0..4 {
        assert!(matches!(a.note_on(0, Poly, n(60 + i), modal, fx), Alloc::Voice(_)));
    }
    assert_eq!(a.note_on(1, Poly, n(70), modal, fx), Alloc::Voice(0), "steals the oldest");
    assert_eq!(a.sounding_cost(), Cost(4 * 1_210));
    // Nothing to steal that frees enough: a voice costing more than the
    // whole budget is refused and nothing is stolen.
    let before: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note())).collect();
    assert_eq!(a.note_on(2, Poly, n(80), Cost(6_500), fx), Alloc::Refused);
    let after: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note())).collect();
    assert_eq!(before, after);
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Seeded property test: random notes on/off across parts and modes.
#[test]
fn random_play_keeps_the_pool_invariants() {
    const COSTS: [Cost; 3] = [Cost(610), Cost(710), Cost(1_210)];
    const FX: Cost = Cost(1_000);
    let (mut refusals, mut steals) = (0u32, 0u32);
    for seed in 1..=20u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let modes: [PartMode; 6] = core::array::from_fn(|_| if rng.below(3) == 0 { Mono } else { Poly });
        let mut a = Allocator::new();
        // Model: every held (part, note) and the voice playing it.
        let mut held: Vec<(u8, u8, usize)> = Vec::new();
        for step in 0..2_000 {
            let part = rng.below(6) as u8;
            let note = 48 + rng.below(12) as u8;
            let ctx = format!("seed {seed} step {step}");
            match rng.below(4) {
                0 | 1 => {
                    if held.iter().any(|h| h.0 == part && h.1 == note) {
                        continue;
                    }
                    let before: Vec<_> = a.slots().iter().map(|s| (s.part(), s.note(), s.held())).collect();
                    let cost = COSTS[part as usize % 3];
                    if let Alloc::Voice(v) = a.note_on(part, modes[part as usize], n(note), cost, FX) {
                        // The voice's previous held note, if any, was stolen or
                        // (same mono part) retriggered: it is no longer held.
                        held.retain(|h| h.2 != v);
                        held.push((part, note, v));
                        if let (Some(p), _, true) = before[v] {
                            assert!(modes[p as usize] == Poly || p == part, "{ctx}: mono voice of part {p} stolen");
                            steals += (p != part || modes[p as usize] == Poly) as u32;
                        }
                    }
                }
                2 => {
                    if let Some(i) = held.iter().position(|h| h.0 == part && h.1 == note) {
                        let (_, _, v) = held.remove(i);
                        assert_eq!(a.note_off(part, n(note)), Some(v), "{ctx}");
                    }
                }
                _ => {
                    let v = rng.below(MAX_VOICES as u64) as usize;
                    a.release_finished(v); // the engine went quiet
                }
            }
            // Every held note is still on its voice, held; nothing else is held.
            for &(p, nn, v) in &held {
                let s = &a.slots()[v];
                assert_eq!((s.part(), s.note(), s.held()), (Some(p), Some(n(nn)), true), "{ctx}");
            }
            assert_eq!(a.slots().iter().filter(|s| s.held()).count(), held.len(), "{ctx}");
            // A mono part plays at most one voice.
            for p in 0..6u8 {
                if modes[p as usize] == Mono {
                    assert!(a.slots().iter().filter(|s| s.part() == Some(p)).count() <= 1, "{ctx}");
                }
            }
            // The sounding total never exceeds the budget.
            assert!(a.sounding_cost() + FX <= AUDIO_CYCLE_BUDGET, "{ctx}");
        }
        refusals += a.refused();
    }
    // The random play reached both the steal and the refuse paths.
    assert!(steals > 0 && refusals > 0, "steals {steals}, refusals {refusals}");
}

/// A Sound change re-costs its sounding voices; over the budget, the newest
/// non-mono voices are shed until it fits.
#[test]
fn recost_sheds_the_newest_voices_over_budget() {
    let mut a = Allocator::new();
    let fx = Cost(600);
    for i in 0..6 {
        on(&mut a, 0, Poly, 60 + i); // 6 × 610 + 600 = 4,260
    }
    for v in 0..6 {
        a.recost(v, Cost(1_210)); // the part switched to Modal: 7,860
    }
    let mut shed = Vec::new();
    while let Some(v) = a.shed(fx) {
        shed.push(v);
    }
    assert_eq!(shed, [5], "newest first, only as many as needed");
    assert!(a.slots()[5].is_free());
    assert_eq!(a.sounding_cost() + fx, Cost(5 * 1_210 + 600));
}

/// Review Focus: a Part switched from Poly to Mono while a chord is held
/// still releases the chord (no stuck notes); new notes share one voice.
#[test]
fn poly_to_mono_switch_releases_the_held_chord() {
    let mut a = Allocator::new();
    let chord: Vec<usize> = (0..3).map(|i| voice(on(&mut a, 0, Poly, 60 + i))).collect();
    let m = voice(on(&mut a, 0, Mono, 72));
    assert!(!chord.contains(&m));
    assert_eq!(voice(on(&mut a, 0, Mono, 74)), m);
    for (i, &v) in chord.iter().enumerate() {
        assert_eq!(a.note_off(0, n(60 + i as u8)), Some(v));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test voice_alloc_test`
Expected: FAIL to compile — `unresolved import chimera_core::voice_alloc`.

- [ ] **Step 3: Write minimal implementation**

In `chimera-core/src/lib.rs` add `pub mod voice_alloc;` after `pub mod ui;`. Create `chimera-core/src/voice_alloc.rs`:

```rust
//! Digitone-style voice allocation (instrument-core spec § Voice
//! allocation): pure bookkeeping over the `MAX_VOICES` pool, no DSP.
//!
//! 1. A Mono part owns one voice while it sounds; a new note retriggers it.
//!    Mono voices are never stolen.
//! 2. A Poly part takes a free voice, round-robin.
//! 3. Pool full: steal the oldest non-mono voice from any part; refuse if
//!    every voice is mono.
//! 4. Over the CPU budget: steal one voice as in rule 3 if that makes room,
//!    else refuse (and steal nothing).
//! 5. Note-off releases the matching (part, note); the voice is free once its
//!    engine reports inactive (`release_finished`), so tails ring out.
//!
//! A Sound change re-costs sounding voices (`recost`); `shed` then cuts the
//! newest voices until the pool is back within the budget.

use crate::hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES};
use crate::part::PartMode;
use crate::MidiNote;

#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceSlot {
    part: Option<u8>,
    note: Option<MidiNote>,
    /// `Allocator::clock` at the last note-on: lower is older.
    age: u32,
    held: bool,
    mono: bool,
    cost: Cost,
}

impl VoiceSlot {
    pub fn part(&self) -> Option<u8> {
        self.part
    }

    pub fn note(&self) -> Option<MidiNote> {
        self.note
    }

    /// Key still down (no note-off yet).
    pub fn held(&self) -> bool {
        self.held
    }

    pub fn is_free(&self) -> bool {
        self.part.is_none()
    }

    /// Cycles/sample this voice costs now (0 when free).
    pub fn cost(&self) -> Cost {
        self.cost
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alloc {
    Voice(usize),
    Refused,
}

pub struct Allocator {
    slots: [VoiceSlot; MAX_VOICES],
    clock: u32,
    /// Next slot to try for a free voice.
    rr: usize,
    refused: u32,
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub fn new() -> Self {
        Self { slots: [VoiceSlot::default(); MAX_VOICES], clock: 0, rr: 0, refused: 0 }
    }

    pub fn slots(&self) -> &[VoiceSlot; MAX_VOICES] {
        &self.slots
    }

    /// Notes refused since start (debug counter).
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// Sum of the costs of every allocated voice.
    pub fn sounding_cost(&self) -> Cost {
        self.slots.iter().map(|s| s.cost).sum()
    }

    /// Allocate a voice for `note` on `part`. `cost` is the voice's
    /// cycles/sample; `reserved` is what is spent outside the pool (the FX
    /// bus). The pool's own sounding cost is tracked here, per slot.
    pub fn note_on(&mut self, part: u8, mode: PartMode, note: MidiNote, cost: Cost, reserved: Cost) -> Alloc {
        let v = match self.pick(part, mode, cost, reserved) {
            Some(v) => v,
            None => {
                self.refused = self.refused.wrapping_add(1);
                return Alloc::Refused;
            }
        };
        self.clock = self.clock.wrapping_add(1);
        self.rr = (v + 1) % MAX_VOICES;
        self.slots[v] = VoiceSlot {
            part: Some(part),
            note: Some(note),
            age: self.clock,
            held: true,
            mono: mode == PartMode::Mono,
            cost,
        };
        Alloc::Voice(v)
    }

    /// Release the held voice playing `note` on `part`, if any.
    pub fn note_off(&mut self, part: u8, note: MidiNote) -> Option<usize> {
        let v = self
            .slots
            .iter()
            .position(|s| s.held && s.part == Some(part) && s.note == Some(note))?;
        self.slots[v].held = false;
        Some(v)
    }

    /// The voice now costs `cost` (its Part's Sound changed engine).
    pub fn recost(&mut self, voice: usize, cost: Cost) {
        if let Some(s) = self.slots.get_mut(voice).filter(|s| !s.is_free()) {
            s.cost = cost;
        }
    }

    /// If the pool plus `reserved` is over the budget, free the newest
    /// voice — non-mono first — and return it for the caller to silence.
    /// Call until `None`.
    pub fn shed(&mut self, reserved: Cost) -> Option<usize> {
        if reserved + self.sounding_cost() <= AUDIO_CYCLE_BUDGET {
            return None;
        }
        let v = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free())
            .max_by_key(|&v| (!self.slots[v].mono, self.slots[v].age))?;
        self.slots[v] = VoiceSlot::default();
        Some(v)
    }

    /// The voice's engine went silent: free it if it was released.
    pub fn release_finished(&mut self, voice: usize) {
        if let Some(s) = self.slots.get_mut(voice)
            && !s.held
        {
            *s = VoiceSlot::default();
        }
    }

    fn pick(&self, part: u8, mode: PartMode, cost: Cost, reserved: Cost) -> Option<usize> {
        let fits = |freed: Cost| {
            let total = reserved.0 + self.sounding_cost().0 + cost.0;
            total.saturating_sub(freed.0) <= AUDIO_CYCLE_BUDGET.0
        };
        // Rule 1: a Mono part retriggers the voice it owns.
        if mode == PartMode::Mono
            && let Some(v) = self.slots.iter().position(|s| s.mono && s.part == Some(part))
        {
            return fits(self.slots[v].cost).then_some(v);
        }
        // Rule 2: a free voice, round-robin — if it fits the budget.
        let free = (0..MAX_VOICES).map(|i| (self.rr + i) % MAX_VOICES).find(|&v| self.slots[v].is_free());
        if let Some(v) = free
            && fits(Cost::ZERO)
        {
            return Some(v);
        }
        // Rules 3 and 4: steal the oldest non-mono voice if that makes room.
        let oldest = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free() && !self.slots[v].mono)
            .min_by_key(|&v| self.slots[v].age)?;
        fits(self.slots[oldest].cost).then_some(oldest)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p chimera-core --test voice_alloc_test` — Expected: 11 passed (the property test runs 20 seeds × 2,000 steps and asserts it reached both the steal and the refuse paths).
Run: `just check` — Expected: 403 passed / 0 failed / 2 ignored; desktop and firmware `Finished`.

(Validation note: without `self.rr = (v + 1) % MAX_VOICES;` the round-robin test fails with `left: [0, 3, 4, 5]`, `right: [3, 4, 5, 0]`.)

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/lib.rs chimera-core/src/voice_alloc.rs chimera-core/tests/voice_alloc_test.rs
git commit -m "feat(core): voice allocator (round-robin, steal oldest, mono, CPU budget)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Per-engine CPU cost estimates

**Files:**
- Modify: `chimera-core/src/dsp/pizza.rs`, `chimera-core/src/dsp/engine_fm.rs`, `chimera-core/src/dsp/modal.rs`, `chimera-core/src/dsp/engines.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/dsp/fx_bus.rs`
- Test: `chimera-core/tests/cost_test.rs`

**Interfaces:**
- Consumes: `hw::Cost` (Task 1).
- Produces: `PizzaOsc::COST = Cost(300)`, `FmEngine::COST = Cost(200)`, `ModalEngine::COST = Cost(800)`, private `VA_COST = Cost(300)` in `engines.rs`; `Engines::cost(EngineType) -> Cost` (const, exhaustive); `Voice::CHAIN_COST = Cost(410)`; `Voice::cost(EngineType) -> Cost` (const) = engine + chain; `FxBus::COST = Cost(600)`. All `// estimate`.

Numbers from `docs/chimera-synth-design.md` § CPU Budget: 4-op FM ~200, modal ~800, VA ~300; per-voice chain = drive 20 + filter 80 + filter FM 60 + folder 40 + VCA/amp env 50 + 3 envelopes 90 + 2 LFOs 40 + mod matrix 30 = 410, giving the table's totals FM 610, Modal 1,210, VA 710. Pizza has no row; it is costed like the VA oscillators (300). The FX bus has no row: chorus ~60 + tape delay ~350 (two `sinf` + `tanhf` per sample) + reverb ~150 + mixing ≈ 600, reserved whether or not an effect is on. With 7,000 cycles: six voices of every engine fit, Modal five (the design doc planned four).

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/cost_test.rs`:

```rust
//! CPU cost estimates (ADR 0013): cycles/sample per voice from the design
//! doc's budget table, until measured with the DWT cycle counter.

use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES};
use chimera_core::params::EngineType;

/// `docs/chimera-synth-design.md` § CPU Budget: FM ~610, Modal ~1,210 and
/// VA ~710 per voice including the chain (~410).
#[test]
fn voice_costs_follow_the_design_table() {
    assert_eq!(Voice::CHAIN_COST, Cost(410));
    assert_eq!(Voice::cost(EngineType::Fm), Cost(610));
    assert_eq!(Voice::cost(EngineType::Modal), Cost(1_210));
    assert_eq!(Voice::cost(EngineType::Va), Cost(710));
    assert_eq!(Voice::cost(EngineType::Pizza), Cost(710));
    for e in EngineType::ALL {
        assert_eq!(Voice::cost(e), Engines::cost(e) + Voice::CHAIN_COST, "{e:?}");
    }
}

/// What the budget allows with the FX bus running: six of every engine but
/// Modal, five Modal (the design doc planned for four).
#[test]
fn budget_capacity_per_engine() {
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * Voice::cost(e).0 <= AUDIO_CYCLE_BUDGET.0;
    for e in [EngineType::Pizza, EngineType::Fm, EngineType::Va] {
        assert!(fits(e, MAX_VOICES as u32), "{e:?}");
    }
    assert!(fits(EngineType::Modal, 5));
    assert!(!fits(EngineType::Modal, 6));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test cost_test`
Expected: FAIL to compile — `no associated function or constant named CHAIN_COST`, `… named cost found for struct Voice`, `… named COST found for struct FxBus`.

- [ ] **Step 3: Write minimal implementation**

`chimera-core/src/dsp/pizza.rs`: add `use crate::hw::Cost;` after `use crate::block::{Block, ParamId, ParamSpec, ValFmt};`, and at the top of `impl PizzaOsc {`:

```rust
    /// Not in the design doc's table; costed like the VA oscillator pair.
    pub const COST: Cost = Cost(300); // estimate

```

`chimera-core/src/dsp/engine_fm.rs`: add `use crate::hw::Cost;` after `use crate::dsp::fm_waveform;`, and at the top of `impl FmEngine {`:

```rust
    /// Design doc § CPU Budget: 4-op FM ~200 cycles/sample.
    pub const COST: Cost = Cost(200); // estimate

```

`chimera-core/src/dsp/modal.rs`: add `use crate::hw::Cost;` after `use crate::block::{Block, ParamId, ParamSpec, ValFmt};`, and at the top of `impl ModalEngine {`:

```rust
    /// Design doc § CPU Budget: physical modeling (modal, 8 modes) ~800.
    pub const COST: Cost = Cost(800); // estimate

```

`chimera-core/src/dsp/engines.rs`: add `use crate::hw::Cost;` after `use crate::dsp::pizza::PizzaOsc;`; above `pub struct Engines {`:

```rust
/// Design doc § CPU Budget: VA Polymod (2 osc + sync + PWM) ~300. The VA
/// engine is a silent placeholder; its budget is reserved now.
const VA_COST: Cost = Cost(300); // estimate

```

and above `/// VCA choice: does the amp envelope shape this engine's output?`:

```rust
    /// Cycles/sample of one engine instance (ADR 0013).
    pub const fn cost(kind: EngineType) -> Cost {
        match kind {
            EngineType::Pizza => PizzaOsc::COST,
            EngineType::Fm => FmEngine::COST,
            EngineType::Modal => ModalEngine::COST,
            EngineType::Va => VA_COST,
        }
    }

```

`chimera-core/src/dsp/voice.rs`: `use crate::hw::{MAX_VOICES, VOICE_RAM_BUDGET};` → `use crate::hw::{Cost, MAX_VOICES, VOICE_RAM_BUDGET};`; at the top of `impl Voice {`:

```rust
    /// Design doc § CPU Budget, everything but the engine: drive 20, filter
    /// 80, filter FM 60, folder 40, VCA + amp env 50, 3 envelopes 90,
    /// 2 LFOs 40, mod matrix 30.
    pub const CHAIN_COST: Cost = Cost(410); // estimate

    /// Cycles/sample of a voice playing `kind`.
    pub const fn cost(kind: EngineType) -> Cost {
        Cost(Engines::cost(kind).0 + Self::CHAIN_COST.0)
    }

```

`chimera-core/src/dsp/fx_bus.rs`: `use crate::hw::FX_BUS_BUDGET;` → `use crate::hw::{Cost, FX_BUS_BUDGET};`; at the top of `impl FxBus {`:

```rust
    /// Not in the design doc's table: chorus ~60, tape delay ~350 (two
    /// `sinf` + `tanhf` per sample), reverb ~150, plus mixing. Reserved
    /// from the voice budget whether or not an effect is on.
    pub const COST: Cost = Cost(600); // estimate

```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p chimera-core --test cost_test` — Expected: 2 passed.
Run: `just check` — Expected: 405 passed / 0 failed / 2 ignored; desktop and firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/dsp chimera-core/tests/cost_test.rs
git commit -m "feat(core): per-engine CPU cost estimates

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Lock-free SPSC note queue

**Files:**
- Create: `chimera-core/src/note_queue.rs`
- Modify: `chimera-core/src/lib.rs`
- Test: `chimera-core/tests/note_queue_test.rs`

**Interfaces:**
- Consumes: `MidiChannel` (Task 5), `MidiNote`, `Velocity`.
- Produces: `chimera_core::note_queue::{NOTE_QUEUE_LEN = 64, NoteKind { On(Velocity), Off }, NoteEvent { channel: MidiChannel, note: MidiNote, kind: NoteKind }, NoteQueue}`; `NoteQueue::new() -> Self` (`const fn`, usable in a `static`), `push(&self, NoteEvent) -> bool` (false = full, dropped, counted), `pop(&self) -> Option<NoteEvent>`, `dropped(&self) -> u32`. One producer thread, one consumer thread.

Each event packs into one `AtomicU32` slot (channel 4 bits, note 7, velocity 7 with 0 = off), so the queue needs no `unsafe` and no `UnsafeCell`; head/tail are free-running `AtomicU32` counters (Release on publish, Acquire on observe).

- [ ] **Step 1: Write the failing test**

Create `chimera-core/tests/note_queue_test.rs`:

```rust
//! Lock-free SPSC note queue from the UI/input thread to audio
//! (instrument-core spec § Threading).

use chimera_core::note_queue::{NoteEvent, NoteKind, NoteQueue, NOTE_QUEUE_LEN};
use chimera_core::{MidiChannel, MidiNote, Velocity};

fn ev(ch: u8, note: u8, vel: u8) -> NoteEvent {
    NoteEvent {
        channel: MidiChannel::new(ch).unwrap(),
        note: MidiNote::new(note).unwrap(),
        kind: match Velocity::new(vel) {
            Some(v) => NoteKind::On(v),
            None => NoteKind::Off,
        },
    }
}

#[test]
fn empty_queue_pops_nothing() {
    let q = NoteQueue::new();
    assert_eq!(q.pop(), None);
}

#[test]
fn events_come_out_in_order_and_intact() {
    let q = NoteQueue::new();
    let evs = [ev(0, 60, 100), ev(15, 127, 127), ev(9, 0, 1), ev(3, 64, 0)];
    for e in evs {
        assert!(q.push(e));
    }
    for e in evs {
        assert_eq!(q.pop(), Some(e));
    }
    assert_eq!(q.pop(), None);
}

#[test]
fn full_queue_drops_and_counts() {
    let q = NoteQueue::new();
    for i in 0..NOTE_QUEUE_LEN {
        assert!(q.push(ev(0, i as u8, 100)), "slot {i}");
    }
    assert!(!q.push(ev(0, 100, 100)));
    assert!(!q.push(ev(0, 101, 100)));
    assert_eq!(q.dropped(), 2);
    // The queued events are untouched by the drops.
    assert_eq!(q.pop(), Some(ev(0, 0, 100)));
    assert!(q.push(ev(0, 102, 100)), "room again after a pop");
}

#[test]
fn indices_wrap_around() {
    let q = NoteQueue::new();
    for i in 0..10 * NOTE_QUEUE_LEN {
        let e = ev((i % 16) as u8, (i % 128) as u8, (i % 128) as u8);
        assert!(q.push(e));
        assert_eq!(q.pop(), Some(e), "event {i}");
    }
    assert_eq!(q.dropped(), 0);
}

/// One producer thread, one consumer thread: every event arrives once, in order.
#[test]
fn producer_and_consumer_threads() {
    const N: usize = 20_000;
    let q = std::sync::Arc::new(NoteQueue::new());
    let producer = {
        let q = std::sync::Arc::clone(&q);
        std::thread::spawn(move || {
            for i in 0..N {
                while !q.push(ev(0, (i % 128) as u8, 1 + (i % 127) as u8)) {
                    std::thread::yield_now();
                }
            }
        })
    };
    let mut got = 0;
    while got < N {
        if let Some(e) = q.pop() {
            assert_eq!(e, ev(0, (got % 128) as u8, 1 + (got % 127) as u8));
            got += 1;
        }
    }
    producer.join().unwrap();
    assert_eq!(q.pop(), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chimera-core --test note_queue_test`
Expected: FAIL to compile — `unresolved import chimera_core::note_queue`.

- [ ] **Step 3: Write minimal implementation**

In `chimera-core/src/lib.rs` add `pub mod note_queue;` before `pub mod params;`. Create `chimera-core/src/note_queue.rs`:

```rust
//! Notes from the UI/input thread to the audio thread (instrument-core spec
//! § Threading): a fixed 64-event single-producer single-consumer ring.
//! Lock-free and allocation-free; each event is packed into one `AtomicU32`,
//! so there is no `unsafe`. A full queue drops the event and counts it.

use core::sync::atomic::{AtomicU32, Ordering};

use crate::{MidiChannel, MidiNote, Velocity};

pub const NOTE_QUEUE_LEN: usize = 64;
const _: () = assert!(NOTE_QUEUE_LEN.is_power_of_two());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    On(Velocity),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub channel: MidiChannel,
    pub note: MidiNote,
    pub kind: NoteKind,
}

impl NoteEvent {
    /// Bits 0..4 channel, 4..11 note, 11..18 velocity (0 = note-off).
    fn pack(self) -> u32 {
        let vel = match self.kind {
            NoteKind::On(v) => v.get(),
            NoteKind::Off => 0,
        };
        self.channel.get() as u32 | ((self.note.get() as u32) << 4) | ((vel as u32) << 11)
    }

    fn unpack(bits: u32) -> Option<Self> {
        Some(Self {
            channel: MidiChannel::new((bits & 0xF) as u8)?,
            note: MidiNote::new(((bits >> 4) & 0x7F) as u8)?,
            kind: match Velocity::new(((bits >> 11) & 0x7F) as u8) {
                Some(v) => NoteKind::On(v),
                None => NoteKind::Off,
            },
        })
    }
}

/// One producer (`push`) and one consumer (`pop`). Head and tail are
/// free-running counters; the slot is `counter % NOTE_QUEUE_LEN`.
pub struct NoteQueue {
    slots: [AtomicU32; NOTE_QUEUE_LEN],
    /// Next event to pop (written by the consumer only).
    head: AtomicU32,
    /// Next slot to push (written by the producer only).
    tail: AtomicU32,
    dropped: AtomicU32,
}

impl Default for NoteQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; NOTE_QUEUE_LEN],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// Producer side. `false`: the queue was full and the event was dropped.
    pub fn push(&self, ev: NoteEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) as usize >= NOTE_QUEUE_LEN {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.slots[tail as usize % NOTE_QUEUE_LEN].store(ev.pack(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side (the audio thread).
    pub fn pop(&self) -> Option<NoteEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let bits = self.slots[head as usize % NOTE_QUEUE_LEN].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        NoteEvent::unpack(bits)
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p chimera-core --test note_queue_test` — Expected: 5 passed.
Run: `just check` — Expected: 410 passed / 0 failed / 2 ignored; firmware `Finished` (`AtomicU32::fetch_add` is native on Cortex-M7).

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/lib.rs chimera-core/src/note_queue.rs chimera-core/tests/note_queue_test.rs
git commit -m "feat(core): lock-free SPSC note queue

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: `AudioShared` snapshot of the Performance; AXI budget asserted

**Files:**
- Create: `chimera-core/src/instrument.rs`
- Modify: `chimera-core/src/lib.rs`, `chimera-core/src/hw.rs`
- Test: `chimera-core/tests/audio_shared_test.rs`, `chimera-core/tests/memory_budget_test.rs`

**Interfaces:**
- Consumes: `Performance` (Tasks 5–6), `SoundPool`, `FxBus`/`FxParams` (Task 3), `PartParams` (Task 5).
- Produces: `hw::FB_BYTES = FB_SIZE * 2` (153,600), `hw::UI_RESERVE = 64 * 1024`; `chimera_core::instrument::{PartAudio { params: ParamSnapshot, mod_state: ModState, mix: PartParams }, AudioShared { parts: [PartAudio; MAX_PARTS], fx: FxParams }, AXI_RESIDENT: usize}`; `AudioShared::from_performance(&Performance) -> Self`, `AudioShared::update_from(&mut self, &Performance)` (in place, no allocation), `impl Default` (= from a new Performance).

The spec's `PartAudio { params, mod_state, mode, output, level, pan, sends }` + the channel it routes by (§ Threading) is `PartAudio { params, mod_state, mix: PartParams }` — the same fields, grouped as in Task 5. The AXI assertion is the spec's `Performance + SoundPool + FB + UI_RESERVE` plus the two `AudioShared` copies ("fits its region") and the FX bus (ADR 0014).

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/audio_shared_test.rs`:

```rust
//! What crosses from the UI to the audio thread (spec § Threading).

use chimera_core::instrument::AudioShared;
use chimera_core::part::PartMode;
use chimera_core::preset::{ChainType, Performance};

#[test]
fn snapshot_copies_every_part_and_the_fx() {
    let mut perf = Performance::new();
    perf.parts[4].load_init(ChainType::Fm);
    perf.parts[4].mix.mode = PartMode::Mono;
    perf.parts[4].mix.pan = -0.5;
    perf.fx.reverb.mix = 0.4;
    let shared = AudioShared::from_performance(&perf);
    assert_eq!(shared.parts[4].params.engine(), perf.parts[4].sound.params.engine());
    assert_eq!(shared.parts[4].mix, perf.parts[4].mix);
    assert_eq!(shared.fx.reverb.mix, 0.4);
    for (i, p) in shared.parts.iter().enumerate() {
        assert_eq!(p.mix.channel.get() as usize, i);
    }
}

/// The UI refreshes the back buffer in place each frame.
#[test]
fn update_from_overwrites_in_place() {
    let mut shared = AudioShared::default();
    let mut perf = Performance::new();
    perf.parts[0].sound.params.filter.cutoff = 440.0;
    perf.parts[0].mix.level = 0.1;
    shared.update_from(&perf);
    assert_eq!(shared.parts[0].params.filter.cutoff, 440.0);
    assert_eq!(shared.parts[0].mix.level, 0.1);
}
```

Append to `chimera-core/tests/memory_budget_test.rs`:

```rust

/// Spec § Hardware parity: Performance + SoundPool + framebuffer + UI
/// reserve (+ both AudioShared copies + the FX bus, ADR 0014) fit AXI.
#[test]
fn axi_residents_fit() {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::instrument::{AudioShared, AXI_RESIDENT};
    use chimera_core::preset::{Performance, SoundPool};
    let parts = [
        ("framebuffer", hw::FB_BYTES),
        ("UI reserve", hw::UI_RESERVE),
        ("Performance", size_of::<Performance>()),
        ("SoundPool", size_of::<SoundPool>()),
        ("AudioShared x2", 2 * size_of::<AudioShared>()),
        ("FxBus", size_of::<FxBus>()),
    ];
    for (name, size) in parts {
        eprintln!("{name:>15} {size:>7} B");
    }
    let total: usize = parts.iter().map(|p| p.1).sum();
    eprintln!("{:>15} {total:>7} B of {} B", "AXI", hw::AXI_SRAM);
    assert_eq!(total, AXI_RESIDENT);
    assert!(total <= hw::AXI_SRAM);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test audio_shared_test --test memory_budget_test`
Expected: FAIL to compile — `unresolved import chimera_core::instrument`, `cannot find value FB_BYTES / UI_RESERVE in module hw`.

- [ ] **Step 3: Write minimal implementation**

In `chimera-core/src/hw.rs`, above `/// AXI share for the FX bus (ADR 0014).`:

```rust
/// Framebuffer: 240 × 320 RGB565, one static in AXI (`chimera-stm32/src/display.rs`).
pub const FB_BYTES: usize = chimera_hal::FB_SIZE * 2; // 153_600
/// AXI kept for the UI besides the Performance and SoundPool: renderer and
/// navigation state, `main`'s stack temporaries and the interrupt stacks.
pub const UI_RESERVE: usize = 64 * 1024;
```

In `chimera-core/src/lib.rs` add `pub mod instrument;` after `pub mod hw;`. Create `chimera-core/src/instrument.rs`:

```rust
//! The playable instrument (instrument-core spec § Audio path, § Threading):
//! what the audio thread reads from the UI, and the voice pool that renders
//! every Part into the three DAC pairs.

use core::mem::size_of;

use crate::dsp::fx_bus::{FxBus, FxParams};
use crate::hw::{AXI_SRAM, FB_BYTES, MAX_PARTS, UI_RESERVE};
use crate::modulation::ModState;
use crate::params::ParamSnapshot;
use crate::part::PartParams;
use crate::preset::{Performance, SoundPool};

/// Everything the port places in AXI SRAM (ADR 0014): framebuffer, UI,
/// Performance, SoundPool, both `AudioShared` copies and the FX bus.
pub const AXI_RESIDENT: usize = FB_BYTES
    + UI_RESERVE
    + size_of::<Performance>()
    + size_of::<SoundPool>()
    + 2 * size_of::<AudioShared>()
    + size_of::<FxBus>();
const _: () = assert!(AXI_RESIDENT <= AXI_SRAM);

/// One Part as the audio thread sees it.
#[derive(Clone, Debug)]
pub struct PartAudio {
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub mix: PartParams,
}

/// The Performance state the audio needs, double-buffered by the platform
/// (one pointer swap per UI frame). The Sound names, pool and UI stay behind.
#[derive(Clone, Debug)]
pub struct AudioShared {
    pub parts: [PartAudio; MAX_PARTS],
    pub fx: FxParams,
}

impl Default for AudioShared {
    fn default() -> Self {
        Self::from_performance(&Performance::new())
    }
}

impl AudioShared {
    pub fn from_performance(perf: &Performance) -> Self {
        Self {
            parts: core::array::from_fn(|i| {
                let p = &perf.parts[i];
                PartAudio { params: p.sound.params.clone(), mod_state: p.sound.mod_state.clone(), mix: p.mix }
            }),
            fx: perf.fx,
        }
    }

    /// Overwrite with `perf` in place (the UI's per-frame copy into the back
    /// buffer; no allocation).
    pub fn update_from(&mut self, perf: &Performance) {
        for (dst, src) in self.parts.iter_mut().zip(&perf.parts) {
            dst.params.clone_from(&src.sound.params);
            dst.mod_state.clone_from(&src.sound.mod_state);
            dst.mix = src.mix;
        }
        self.fx = perf.fx;
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p chimera-core --test audio_shared_test --test memory_budget_test -- --nocapture` — Expected: all pass; on x86_64 the table prints framebuffer 153,600 / UI reserve 65,536 / Performance 5,312 / SoundPool 26,880 / AudioShared x2 6,368 / FxBus 253,744 = 511,440 of 524,288 (target: 510,672).
Run: `just check` — Expected: 413 passed / 0 failed / 2 ignored; desktop and firmware `Finished` (the AXI `const` assertion holds on the target).

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/lib.rs chimera-core/src/hw.rs chimera-core/src/instrument.rs chimera-core/tests/audio_shared_test.rs chimera-core/tests/memory_budget_test.rs
git commit -m "feat(core): AudioShared snapshot of the Performance; AXI budget asserted

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: `Instrument` renders the voice pool into three DAC pairs with the FX bus

**Files:**
- Modify: `chimera-core/src/instrument.rs` (append `Instrument`), `chimera-core/src/dsp/voice.rs` (scope write moves out), `chimera-desktop/src/audio.rs` and `chimera-stm32/src/audio.rs` (keep their scope fed until they switch), `.cargo/config.toml` (test-thread stack)
- Test: `chimera-core/tests/instrument_test.rs` (new), `chimera-core/tests/common/mod.rs`, `chimera-core/tests/golden_test.rs`, `chimera-core/tests/memory_budget_test.rs`

**Interfaces:**
- Consumes: `Voice`, `Voice::cost` (Task 9), `Allocator` (Task 8), `NoteEvent` (Task 10), `AudioShared` (Task 11), `FxBus` + `FxBus::COST` (Tasks 3, 9), `hw::{DAC_PAIRS, MAX_PARTS, MAX_VOICES, VOICE_RAM_BUDGET}`.
- Produces: `pub type DacOut = [[f32; BLOCK_SIZE * 2]; DAC_PAIRS]` (interleaved L, R per pair); `pub fn pan_gains(pan: f32) -> (f32, f32)`; `Instrument::new(sample_rate: u32) -> Self`, `handle(&mut self, ev: NoteEvent, shared: &AudioShared)`, `render(&mut self, fx: &mut FxBus, out: &mut DacOut, shared: &AudioShared)`, `part_bus(&self, part: usize) -> &[f32; BLOCK_SIZE]`, `allocator(&self) -> &Allocator`. `const` assertion `size_of::<Instrument>() <= VOICE_RAM_BUDGET`.

Deviation from the spec's sketch: `Instrument` does not own `fx: FxBus`; `render` borrows it. On hardware the pool goes to D2 and the FX bus to AXI (ADR 0014) — one struct cannot straddle two regions. Per block: recost/shed (Review Focus 3) → voices into their Part's mono bus (the first voice of a Part is **copied**, not added, so `-0.0` samples survive and a lone voice reaches the bus bit-for-bit) → free released voices whose engine went quiet → pan (constant-power, `sin((1∓pan)·π/4)` so hard left/right is exactly 0 on the other side) × level into the Part's pair, `bus × send[i]` into the FX inputs → FX once, return added to both sides of pair 1 → oscilloscope gets the sum of the buses (the scope write moves out of `Voice::render`, which would otherwise interleave six voices). Note-offs release the voices *their channel* started (Review Focus 2).

- [ ] **Step 1: Write the failing tests**

In `chimera-core/tests/common/mod.rs`, extend the imports:

```rust
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::instrument::{AudioShared, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
```

(replacing `use chimera_core::{MidiNote, Velocity};` and `use chimera_core::dsp::voice::Voice;`) and add above `/// FNV-1a 64 over the little-endian bytes of each sample's \`f32::to_bits\`.`:

```rust
/// The same harness through `Instrument` (instrument-core spec § Testing):
/// the case on part 1 (MIDI channel 1), other parts silent, sends 0.
/// Returns part 1's mono bus — the sum of its voices before pan and level.
pub fn render_case_through_instrument(case: Case) -> Vec<f32> {
    let (params, mod_state) = setup(case);
    let mut shared = AudioShared::default();
    shared.parts[0].params = params;
    shared.parts[0].mod_state = mod_state;
    let mut switched = shared.clone();
    switched.parts[0].params = init_params(EngineType::Modal);
    let mut inst = Box::new(Instrument::new(chimera_hal::SAMPLE_RATE));
    let mut fx = Box::new(FxBus::new());
    let mut dac = [[0.0f32; BLOCK_SIZE * 2]; DAC_PAIRS];
    let event = |kind| NoteEvent { channel: MidiChannel::new(0).unwrap(), note: MidiNote::new(NOTE).unwrap(), kind };
    let mut out = Vec::with_capacity(TOTAL_SAMPLES);
    for b in 0..ON_BLOCKS + OFF_BLOCKS {
        let s = if case == Case::PizzaToModalSwitch && b >= ON_BLOCKS / 2 { &switched } else { &shared };
        if b == 0 {
            inst.handle(event(NoteKind::On(Velocity::new(VEL).unwrap())), s);
        }
        if b == ON_BLOCKS {
            inst.handle(event(NoteKind::Off), s);
        }
        inst.render(&mut fx, &mut dac, s);
        out.extend_from_slice(inst.part_bus(0));
    }
    out
}

```

In `chimera-core/tests/golden_test.rs`, add above `#[test]\nfn known_broken_goldens_have_issues() {`:

```rust
/// Spec § Testing: part 1's mono bus through the new voice pool matches
/// every existing golden bit-for-bit.
#[test]
fn goldens_match_through_the_instrument() {
    let mut failures = Vec::new();
    for case in Case::ALL {
        let out = render_case_through_instrument(case);
        let (hash, sp) = (fnv1a(&out), spots(&out));
        let &(_, want_hash, want_spots) = GOLDENS.iter().find(|g| g.0 == case.name()).expect("recorded");
        if hash != want_hash || sp != want_spots {
            failures.push(format!("{}: hash 0x{hash:016x} (want 0x{want_hash:016x})", case.name()));
        }
    }
    assert!(failures.is_empty(), "instrument golden mismatch:\n{}", failures.join("\n"));
}

```

Append to `chimera-core/tests/memory_budget_test.rs`:

```rust

/// The whole pool with its bookkeeping (allocator, part buses, sends) fits D2.
#[test]
fn instrument_fits_d2() {
    let size = size_of::<chimera_core::instrument::Instrument>();
    eprintln!("Instrument = {size} B, budget {} B", hw::VOICE_RAM_BUDGET);
    assert!(size <= hw::VOICE_RAM_BUDGET, "Instrument = {size} B");
}
```

Create `chimera-core/tests/instrument_test.rs` (the new goldens are the values recorded when this task was validated; see Step 5):

```rust
//! The instrument audio path (instrument-core spec § Audio path): voices per
//! part, mono bus, constant-power pan and level into the part's DAC pair,
//! sends into the FX bus, FX return into DAC pair 1.

mod common;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::DAC_PAIRS;
use chimera_core::instrument::{pan_gains, AudioShared, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::part::{DacPair, PartMode};
use chimera_core::preset::{ChainType, Performance};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::fnv1a;

const SR: u32 = chimera_hal::SAMPLE_RATE;

fn on(ch: u8, note: u8) -> NoteEvent {
    NoteEvent { channel: MidiChannel::new(ch).unwrap(), note: MidiNote::new(note).unwrap(), kind: NoteKind::On(Velocity::DEFAULT) }
}

fn off(ch: u8, note: u8) -> NoteEvent {
    NoteEvent { channel: MidiChannel::new(ch).unwrap(), note: MidiNote::new(note).unwrap(), kind: NoteKind::Off }
}

struct Rig {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    out: DacOut,
}

impl Rig {
    fn new() -> Self {
        Self { inst: Box::new(Instrument::new(SR)), fx: Box::new(FxBus::new()), out: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS] }
    }
    fn render(&mut self, shared: &AudioShared) -> &DacOut {
        self.inst.render(&mut self.fx, &mut self.out, shared);
        &self.out
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// Left and right halves of an interleaved pair.
fn lr(pair: &[f32; BLOCK_SIZE * 2]) -> ([f32; BLOCK_SIZE], [f32; BLOCK_SIZE]) {
    (core::array::from_fn(|i| pair[2 * i]), core::array::from_fn(|i| pair[2 * i + 1]))
}

#[test]
fn pan_law_is_constant_power() {
    let (l, r) = pan_gains(0.0);
    assert!((l - core::f32::consts::FRAC_1_SQRT_2).abs() < 1e-7 && l == r, "centre = -3 dB per side");
    assert_eq!(pan_gains(-1.0), (1.0, 0.0), "hard left");
    assert_eq!(pan_gains(1.0), (0.0, 1.0), "hard right");
    for p in [-0.7f32, -0.2, 0.3, 0.9] {
        let (l, r) = pan_gains(p);
        assert!((l * l + r * r - 1.0).abs() < 1e-6, "pan {p}");
    }
}

/// The DAC pair gets the part's mono bus × pan gain × level.
#[test]
fn part_bus_is_panned_and_levelled_into_its_pair() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    shared.parts[0].mix.level = 0.5;
    shared.parts[0].mix.pan = -1.0;
    rig.inst.handle(on(0, 60), &shared);
    for _ in 0..4 {
        rig.render(&shared);
    }
    let bus = *rig.inst.part_bus(0);
    let (l, r) = lr(&rig.out[0]);
    assert!(peak(&bus) > 0.01);
    for i in 0..BLOCK_SIZE {
        assert_eq!(l[i], bus[i] * 0.5, "sample {i}");
        assert_eq!(r[i], 0.0);
    }
    assert_eq!(peak(&rig.out[1]) + peak(&rig.out[2]), 0.0, "other pairs silent");
}

/// Part routing by channel at dequeue; parts sharing a channel layer.
#[test]
fn notes_route_by_channel() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    rig.inst.handle(on(2, 60), &shared);
    rig.render(&shared);
    for p in 0..6 {
        assert_eq!(peak(rig.inst.part_bus(p)) > 0.0, p == 2, "part {p}");
    }
    shared.parts[4].mix.channel = MidiChannel::new(2).unwrap();
    rig.inst.handle(on(2, 64), &shared);
    rig.render(&shared);
    assert!(peak(rig.inst.part_bus(4)) > 0.0, "part 5 layered on channel 3");
}

/// Rule 5: after note-off the voice keeps rendering its tail and is freed
/// only when its engine goes quiet.
#[test]
fn tails_ring_out_then_free_the_voice() {
    let mut rig = Rig::new();
    let shared = AudioShared::default(); // Pizza, release 0.3 s
    rig.inst.handle(on(0, 60), &shared);
    for _ in 0..20 {
        rig.render(&shared);
    }
    rig.inst.handle(off(0, 60), &shared);
    rig.render(&shared);
    assert!(peak(rig.inst.part_bus(0)) > 0.0, "tail");
    assert_eq!(rig.inst.allocator().slots()[0].part(), Some(0));
    let mut blocks = 0;
    while !rig.inst.allocator().slots()[0].is_free() {
        rig.render(&shared);
        blocks += 1;
        assert!(blocks < 2_000, "voice never freed");
    }
    assert!(blocks > 50, "freed after {blocks} blocks: before the 0.3 s release ended");
}

#[test]
fn refused_notes_are_counted_and_silent() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    for p in 0..6 {
        shared.parts[p].mix.mode = PartMode::Mono;
        rig.inst.handle(on(p as u8, 60), &shared);
    }
    shared.parts[0].mix.mode = PartMode::Poly;
    shared.parts[0].mix.channel = MidiChannel::new(9).unwrap();
    rig.inst.handle(on(9, 72), &shared);
    assert_eq!(rig.inst.allocator().refused(), 1);
}

/// A performance-level render for the new goldens: `blocks` blocks, notes
/// on at block 0 and off at `blocks / 2`, every DAC sample hashed.
fn render_perf(perf: &Performance, notes: &[(u8, u8)], blocks: usize) -> Vec<f32> {
    let shared = AudioShared::from_performance(perf);
    let mut rig = Rig::new();
    let mut all = Vec::new();
    for b in 0..blocks {
        for &(ch, n) in notes {
            if b == 0 {
                rig.inst.handle(on(ch, n), &shared);
            }
            if b == blocks / 2 {
                rig.inst.handle(off(ch, n), &shared);
            }
        }
        for pair in rig.render(&shared) {
            all.extend_from_slice(pair);
        }
    }
    all
}

fn chord() -> Vec<f32> {
    render_perf(&Performance::new(), &[(0, 60), (0, 64), (0, 67), (0, 71)], 200)
}

fn two_parts() -> Vec<f32> {
    let mut perf = Performance::new();
    perf.parts[1].load_init(ChainType::Fm);
    perf.parts[1].mix.output = DacPair::P2;
    perf.parts[1].mix.pan = 0.5;
    render_perf(&perf, &[(0, 60), (1, 67)], 200)
}

fn reverb_send(send: f32) -> Vec<f32> {
    let mut perf = Performance::new();
    perf.fx.reverb.mix = 0.5;
    perf.fx.reverb.time = 0.7;
    perf.parts[0].mix.sends[2] = send;
    render_perf(&perf, &[(0, 60)], 300)
}

/// Recorded when the instrument path landed (plan Task 12). Re-record only
/// for an intended sound change:
///     GOLDEN_RECORD=1 cargo test -p chimera-core --test instrument_test -- --nocapture
const GOLDENS: &[(&str, u64)] = &[
    ("poly_chord", 0x9e1be15b748f4ab1),
    ("two_parts_two_pairs", 0xcfe8ed2b4c185e18),
    ("reverb_send_off", 0x25fa9f662d1acb99),
    ("reverb_send_on", 0xaee0d4aead340f8d),
];

#[test]
fn instrument_goldens_match() {
    let cases: [(&str, fn() -> Vec<f32>); 4] = [
        ("poly_chord", chord),
        ("two_parts_two_pairs", two_parts),
        ("reverb_send_off", || reverb_send(0.0)),
        ("reverb_send_on", || reverb_send(0.5)),
    ];
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for (name, render) in cases {
        let hash = fnv1a(&render());
        if record {
            println!("    (\"{name}\", 0x{hash:016x}),");
        } else if GOLDENS.iter().find(|g| g.0 == name).map(|g| g.1) != Some(hash) {
            failures.push(format!("{name}: 0x{hash:016x}"));
        }
    }
    assert!(failures.is_empty(), "instrument golden mismatch:\n{}", failures.join("\n"));
}

/// What the goldens lock is what the spec asks for.
#[test]
fn golden_scenes_do_what_they_say() {
    let frames = |v: &[f32], pair: usize| -> Vec<f32> {
        v.chunks(BLOCK_SIZE * 2 * DAC_PAIRS).flat_map(|b| b[pair * BLOCK_SIZE * 2..][..BLOCK_SIZE * 2].to_vec()).collect()
    };
    // Four voices sound at once.
    let single = render_perf(&Performance::new(), &[(0, 60)], 200);
    assert!(peak(&chord()) > peak(&single));
    // Part 2 plays out of pair 2 only; pair 3 stays silent.
    let two = two_parts();
    assert!(peak(&frames(&two, 1)) > 0.01);
    assert_eq!(peak(&frames(&two, 2)), 0.0);
    // The send adds a reverb return (to pair 1) and nothing else changes
    // when it is 0: send off = no FX at all.
    let (dry, wet) = (reverb_send(0.0), reverb_send(0.5));
    assert_ne!(fnv1a(&dry), fnv1a(&wet));
    assert_eq!(fnv1a(&dry), fnv1a(&render_perf(&Performance::new(), &[(0, 60)], 300)));
}

/// Review Focus: a Part's channel changes while a key is held. The
/// note-off arrives on the channel the note-on came from and still
/// releases the voice (no stuck note).
#[test]
fn note_off_follows_the_note_on_channel() {
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    rig.inst.handle(on(0, 60), &shared);
    rig.render(&shared);
    shared.parts[0].mix.channel = MidiChannel::new(3).unwrap();
    rig.inst.handle(off(0, 60), &shared);
    assert!(!rig.inst.allocator().slots()[0].held());
}

/// Review Focus: switching a held chord to a costlier Sound must not push
/// the pool over the CPU budget; the newest voices are cut.
#[test]
fn sound_change_mid_chord_stays_in_budget() {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::hw::AUDIO_CYCLE_BUDGET;
    use chimera_core::params::{EngineType, ParamSnapshot};
    let mut rig = Rig::new();
    let mut shared = AudioShared::default();
    for n in 0..6 {
        rig.inst.handle(on(0, 60 + n), &shared);
    }
    rig.render(&shared);
    assert_eq!(rig.inst.allocator().slots().iter().filter(|s| !s.is_free()).count(), 6);
    shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Modal);
    rig.render(&shared);
    let a = rig.inst.allocator();
    assert!(a.sounding_cost() + FxBus::COST <= AUDIO_CYCLE_BUDGET);
    assert_eq!(a.slots().iter().filter(|s| !s.is_free()).count(), 5);
    assert!(a.slots().iter().all(|s| s.is_free() || s.cost() == Voice::cost(EngineType::Modal)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test instrument_test --test golden_test`
Expected: FAIL to compile — `unresolved imports chimera_core::instrument::{pan_gains, DacOut, Instrument}`.

- [ ] **Step 3: Write the implementation**

In `chimera-core/src/instrument.rs`, replace the imports (from `use core::mem::size_of;` through `use crate::preset::{Performance, SoundPool};`) with:

```rust
use core::mem::size_of;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::fx_bus::{FxBus, FxParams, FX_SENDS};
use crate::dsp::voice::Voice;
use crate::hw::{AXI_SRAM, DAC_PAIRS, FB_BYTES, MAX_PARTS, MAX_VOICES, UI_RESERVE, VOICE_RAM_BUDGET};
use crate::modulation::ModState;
use crate::note_queue::{NoteEvent, NoteKind};
use crate::params::ParamSnapshot;
use crate::part::PartParams;
use crate::preset::{Performance, SoundPool};
use crate::voice_alloc::{Alloc, Allocator};
use crate::MidiChannel;
```

and append to the end of the file:

```rust

/// One block for each DAC pair, interleaved L, R.
pub type DacOut = [[f32; BLOCK_SIZE * 2]; DAC_PAIRS];

// ADR 0013/0014: the voice pool (and its small bookkeeping) lives in D2.
const _: () = assert!(size_of::<Instrument>() <= VOICE_RAM_BUDGET);

/// Constant-power pan: (left, right) gains for `pan` in -1..1. Centre is
/// -3 dB per side; hard left/right is unity on one side, exactly 0 on the other.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let q = core::f32::consts::FRAC_PI_4;
    (libm::sinf((1.0 - pan) * q), libm::sinf((1.0 + pan) * q))
}

/// The shared voice pool and the per-block mix. The FX bus is passed to
/// `render` rather than owned: on hardware the pool is placed in D2 and the
/// FX bus in AXI (ADR 0014).
pub struct Instrument {
    voices: [Voice; MAX_VOICES],
    alloc: Allocator,
    /// The MIDI channel each voice's note-on arrived on, so its note-off
    /// releases it even if the Part's channel changed meanwhile.
    note_channel: [MidiChannel; MAX_VOICES],
    /// Each Part's mono bus from the last `render`: the sum of its voices.
    buses: [[f32; BLOCK_SIZE]; MAX_PARTS],
    sends: [[f32; BLOCK_SIZE]; FX_SENDS],
    sample_rate: u32,
}

impl Instrument {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            voices: core::array::from_fn(|_| Voice::new(sample_rate)),
            alloc: Allocator::new(),
            note_channel: [MidiChannel::clamped(0); MAX_VOICES],
            buses: [[0.0; BLOCK_SIZE]; MAX_PARTS],
            sends: [[0.0; BLOCK_SIZE]; FX_SENDS],
            sample_rate,
        }
    }

    pub fn allocator(&self) -> &Allocator {
        &self.alloc
    }

    /// Part `part`'s mono bus from the last `render` (before pan and level).
    pub fn part_bus(&self, part: usize) -> &[f32; BLOCK_SIZE] {
        &self.buses[part]
    }

    /// Note-on: play on every Part listening on the channel. Note-off:
    /// release the voices this channel started.
    pub fn handle(&mut self, ev: NoteEvent, shared: &AudioShared) {
        match ev.kind {
            NoteKind::On(vel) => {
                for (p, part) in shared.parts.iter().enumerate() {
                    if part.mix.channel != ev.channel {
                        continue;
                    }
                    let cost = Voice::cost(part.params.engine());
                    if let Alloc::Voice(v) = self.alloc.note_on(p as u8, part.mix.mode, ev.note, cost, FxBus::COST) {
                        self.voices[v].note_on(ev.note, vel, &part.params);
                        self.note_channel[v] = ev.channel;
                    }
                }
            }
            NoteKind::Off => {
                for v in 0..MAX_VOICES {
                    let s = self.alloc.slots()[v];
                    if !(s.held() && s.note() == Some(ev.note) && self.note_channel[v] == ev.channel) {
                        continue;
                    }
                    if let Some(p) = s.part()
                        && self.alloc.note_off(p, ev.note) == Some(v)
                    {
                        self.voices[v].note_off();
                    }
                }
            }
        }
    }

    /// Render one block into the three DAC pairs.
    pub fn render(&mut self, fx: &mut FxBus, out: &mut DacOut, shared: &AudioShared) {
        // A Sound that changed engine changes its voices' cost; cut the
        // newest voices if that went over the budget.
        for v in 0..MAX_VOICES {
            if let Some(p) = self.alloc.slots()[v].part() {
                self.alloc.recost(v, Voice::cost(shared.parts[p as usize].params.engine()));
            }
        }
        while let Some(v) = self.alloc.shed(FxBus::COST) {
            self.voices[v].note_off();
        }

        // 1. Voices into their part's mono bus. The first voice of a part is
        //    copied, not added, so a lone voice reaches the bus bit-for-bit.
        let mut written = [false; MAX_PARTS];
        for bus in self.buses.iter_mut() {
            bus.fill(0.0);
        }
        let mut block = [0.0f32; BLOCK_SIZE];
        for v in 0..MAX_VOICES {
            let Some(p) = self.alloc.slots()[v].part() else { continue };
            let (p, part) = (p as usize, &shared.parts[p as usize]);
            self.voices[v].render(&mut block, &part.params, &part.mod_state);
            if written[p] {
                for (b, &s) in self.buses[p].iter_mut().zip(&block) {
                    *b += s;
                }
            } else {
                self.buses[p] = block;
                written[p] = true;
            }
            // 5. A released voice whose engine went quiet is free again.
            if !self.voices[v].is_active() {
                self.alloc.release_finished(v);
            }
        }

        // 2-3. Pan and level into the part's pair; sends into the FX bus.
        for pair in out.iter_mut() {
            pair.fill(0.0);
        }
        for send in self.sends.iter_mut() {
            send.fill(0.0);
        }
        let mut scope = [0.0f32; BLOCK_SIZE];
        for (p, part) in shared.parts.iter().enumerate() {
            let bus = &self.buses[p];
            let (gl, gr) = pan_gains(part.mix.pan);
            let (gl, gr) = (gl * part.mix.level, gr * part.mix.level);
            let pair = &mut out[part.mix.output.index()];
            for i in 0..BLOCK_SIZE {
                pair[2 * i] += bus[i] * gl;
                pair[2 * i + 1] += bus[i] * gr;
                scope[i] += bus[i];
            }
            for (send, &amount) in self.sends.iter_mut().zip(&part.mix.sends) {
                for (s, &b) in send.iter_mut().zip(bus) {
                    *s += b * amount;
                }
            }
        }

        // 4. The FX bus once; its return lands on both sides of pair 1.
        let mut ret = [0.0f32; BLOCK_SIZE];
        fx.process(&mut self.sends, &shared.fx, self.sample_rate, &mut ret);
        for (i, &r) in ret.iter().enumerate() {
            out[0][2 * i] += r;
            out[0][2 * i + 1] += r;
        }

        // Oscilloscope: every part's bus, before pan and level.
        crate::scope::write_samples(&scope);
    }
}
```

In `chimera-core/src/dsp/voice.rs`, delete from `Voice::render`:

```rust

        // 6. Scope — capture end-of-chain for oscilloscope display
        crate::scope::write_samples(output);
```

The desktop and firmware still drive a single `Voice` until Tasks 13/14, so feed their scope where the voice renders: in `chimera-desktop/src/audio.rs` after `voice.render(&mut block, params, mod_state);` add `chimera_core::scope::write_samples(&block);`; in `chimera-stm32/src/audio.rs` after `voice.render(work, params, mod_state);` add `chimera_core::scope::write_samples(work);`.

In `.cargo/config.toml` append:

```toml

[env]
# Test threads construct the 250 KB voice pool and FX bus by value; debug
# builds copy them on the stack several times on the way into a Box. Give
# spawned threads room (the default is 2 MiB).
RUST_MIN_STACK = "8388608"
```

(Validated: without it `goldens_match_through_the_instrument` aborts with `has overflowed its stack`; 3 MB is enough. Release builds construct in place; the firmware port must build the pool in place — see ADR 0008/0014.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p chimera-core --test instrument_test --test golden_test --test memory_budget_test -- --nocapture`
Expected: all pass — `goldens_match_through_the_instrument` proves all ten existing goldens bit-for-bit through part 1's bus; `Instrument = 247088 B, budget 286720 B` (x86_64; 246,600 B on the target).
Run: `just check` — Expected: 424 passed / 0 failed / 2 ignored; desktop and firmware `Finished`.

- [ ] **Step 5: Golden procedure (only if Step 4 reports an `instrument_goldens_match` mismatch)**

The four new hashes were recorded from this exact code. A mismatch means the implementation differs from the plan: find the difference; do not re-record. (To record goldens for a deliberate change later: `GOLDEN_RECORD=1 cargo test -p chimera-core --test instrument_test -- --nocapture` and paste the rows over `GOLDENS`.)

- [ ] **Step 6: Commit**

```bash
git add .cargo/config.toml chimera-core chimera-desktop/src/audio.rs chimera-stm32/src/audio.rs
git commit -m "feat(core): Instrument renders the voice pool into three DAC pairs with the FX bus

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Desktop plays the `Instrument`; piano follows the selected Part; solo a DAC pair

**Files:**
- Modify (rewrite): `chimera-desktop/src/audio.rs`; Modify: `chimera-desktop/src/main.rs`, `Justfile`
- Test: unit tests inside `chimera-desktop/src/audio.rs`

**Interfaces:**
- Consumes: `Instrument`, `DacOut` (Task 12), `AudioShared` (Task 11), `NoteQueue`/`NoteEvent`/`NoteKind` (Task 10), `FxBus` (Task 3), `hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE}`.
- Produces: `DesktopAudio::new()`, `update(&mut self, perf: &Performance)`, `note_on(&self, MidiChannel, MidiNote, Velocity)`, `note_off(&self, MidiChannel, MidiNote)`, `solo(&self, pair: u8)` (0 = all, 1..=3); private `stereo_frame(&DacOut, solo: u8, i: usize) -> (f32, f32)`.

The audio callback drains the note queue into `Instrument::handle`, renders a block per 64 frames, sums the three pairs to stereo (or one soloed pair: F1–F3, F4 = all), soft-clips with the existing `tanhf(x · 0.7)`, and writes real stereo frames (the old callback wrote one mono stream into interleaved channels). It asks the device for 48 kHz stereo f32 (hardware parity: Modal's pitch floor and the delay range assume 48 kHz) and falls back to the default config with a warning. The `AudioShared` double buffer keeps today's scheme — the UI writes the buffer the pointer does not name, then swaps (same known race window as today: the audio thread may still be reading the previous buffer when the UI starts rewriting it two frames later; the STM32 port replaces this).

- [ ] **Step 1: Write the failing tests**

At the end of `chimera-desktop/src/audio.rs` add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn dac() -> DacOut {
        let mut d = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
        for (p, pair) in d.iter_mut().enumerate() {
            pair[0] = (p + 1) as f32; // L of frame 0
            pair[1] = 10.0 * (p + 1) as f32; // R of frame 0
        }
        d
    }

    #[test]
    fn pairs_sum_to_stereo() {
        assert_eq!(stereo_frame(&dac(), 0, 0), (6.0, 60.0));
    }

    #[test]
    fn solo_hears_one_pair() {
        assert_eq!(stereo_frame(&dac(), 2, 0), (2.0, 20.0));
        assert_eq!(stereo_frame(&dac(), 3, 0), (3.0, 30.0));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `PKG_CONFIG_PATH=… cargo test -p chimera-desktop`
Expected: FAIL to compile — `cannot find type DacOut`, `cannot find function stereo_frame`, `cannot find value BLOCK_SIZE / DAC_PAIRS`.

- [ ] **Step 3: Write the implementation**

Replace the whole of `chimera-desktop/src/audio.rs` (keeping the test module from Step 1 at the end) with:

```rust
//! Desktop audio: the same `Instrument` the firmware will run (ADR 0013),
//! fed by the `NoteQueue` and a double-buffered `AudioShared`, summed from
//! three DAC pairs to the speakers.

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteQueue};
use chimera_core::preset::Performance;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

/// Everything the audio callback shares with the UI thread.
struct SharedState {
    /// The `AudioShared` buffer the callback reads; the UI fills the other
    /// one and swaps this pointer once per frame.
    current: AtomicPtr<AudioShared>,
    notes: NoteQueue,
    /// 0 = all pairs, 1..=3 = only that DAC pair.
    solo: AtomicU8,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    bufs: Box<[AudioShared; 2]>,
    active_buf: usize,
}

impl DesktopAudio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = stereo_48k(&device).unwrap_or_else(|| {
            let c = device.default_output_config().expect("no output config");
            eprintln!("no 48 kHz stereo f32 output; using {} Hz (Modal pitch floor and delay range assume 48 kHz)", c.sample_rate().0);
            c
        });
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        let mut bufs = Box::new([AudioShared::default(), AudioShared::default()]);
        let initial_ptr = &mut bufs[0] as *mut AudioShared;
        let shared = Arc::new(SharedState {
            current: AtomicPtr::new(initial_ptr),
            notes: NoteQueue::new(),
            solo: AtomicU8::new(0),
        });
        let audio = Arc::clone(&shared);

        let mut inst = Box::new(Instrument::new(sample_rate));
        let mut fx = Box::new(FxBus::new());
        let mut dac: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
        let mut block_pos = BLOCK_SIZE;

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // SAFETY: the pointer always points into `bufs`, owned by
                    // `DesktopAudio`, which outlives the stream. The UI only
                    // writes the buffer this pointer does not name.
                    let shared = unsafe { &*audio.current.load(Ordering::Acquire) };
                    while let Some(ev) = audio.notes.pop() {
                        inst.handle(ev, shared);
                    }
                    let solo = audio.solo.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        if block_pos >= BLOCK_SIZE {
                            inst.render(&mut fx, &mut dac, shared);
                            block_pos = 0;
                        }
                        let (l, r) = stereo_frame(&dac, solo, block_pos);
                        let (l, r) = (libm::tanhf(l * 0.7), libm::tanhf(r * 0.7));
                        match frame {
                            [mono] => *mono = 0.5 * (l + r),
                            [fl, fr, rest @ ..] => {
                                (*fl, *fr) = (l, r);
                                rest.fill(0.0);
                            }
                            [] => {}
                        }
                        block_pos += 1;
                    }
                },
                |err| eprintln!("audio error: {}", err),
                None,
            )
            .expect("failed to build audio stream");

        stream.play().expect("failed to play stream");

        Self { _stream: stream, shared, bufs, active_buf: 0 }
    }

    /// Push the Performance to the audio thread (lock-free swap).
    pub fn update(&mut self, perf: &Performance) {
        let inactive = 1 - self.active_buf;
        let buf = &mut self.bufs[inactive];
        buf.update_from(perf);
        self.shared.current.store(buf as *mut AudioShared, Ordering::Release);
        self.active_buf = inactive;
    }

    pub fn note_on(&self, channel: MidiChannel, note: MidiNote, velocity: Velocity) {
        self.shared.notes.push(NoteEvent { channel, note, kind: NoteKind::On(velocity) });
    }

    pub fn note_off(&self, channel: MidiChannel, note: MidiNote) {
        self.shared.notes.push(NoteEvent { channel, note, kind: NoteKind::Off });
    }

    /// Hear only DAC pair `pair` (1..=3), or all of them (0).
    pub fn solo(&self, pair: u8) {
        self.shared.solo.store(pair.min(DAC_PAIRS as u8), Ordering::Relaxed);
    }
}

/// A 48 kHz stereo f32 config if the device has one (hardware parity).
fn stereo_48k(device: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    device
        .supported_output_configs()
        .ok()?
        .find(|c| {
            c.channels() >= 2
                && c.sample_format() == cpal::SampleFormat::F32
                && (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&SAMPLE_RATE)
        })
        .map(|c| c.with_sample_rate(cpal::SampleRate(SAMPLE_RATE)))
}

/// Frame `i` of the three pairs summed to one stereo pair, or only pair
/// `solo` (1..=3) when `solo` is not 0.
fn stereo_frame(dac: &DacOut, solo: u8, i: usize) -> (f32, f32) {
    let mut l = 0.0;
    let mut r = 0.0;
    for (p, pair) in dac.iter().enumerate() {
        if solo == 0 || solo as usize == p + 1 {
            l += pair[2 * i];
            r += pair[2 * i + 1];
        }
    }
    (l, r)
}

```

`chimera-desktop/src/main.rs`:
- `use chimera_hal::{ChimeraDisplay, MidiNote, Velocity};` → `use chimera_hal::{ChimeraDisplay, MidiChannel, MidiNote, Velocity};`
- `let mut current_note: Option<MidiNote> = None;` →

```rust
    // The held key and the channel it was sent on, so its note-off follows
    // it even if the selected Part changes while it is held.
    let mut current_note: Option<(MidiChannel, MidiNote)> = None;
```

- replace the piano block (from `// Piano keys` through the closing `}` of `if note != current_note { … }`) with:

```rust
        // Solo a DAC pair: F1-F3; F4 hears all three.
        for (key, pair) in [(minifb::Key::F1, 1), (minifb::Key::F2, 2), (minifb::Key::F3, 3), (minifb::Key::F4, 0)] {
            if keys.contains(&key) {
                audio.solo(pair);
            }
        }

        // Piano keys play the selected Part's channel.
        let note = piano_note(&keys)
            .and_then(|n| MidiNote::new((n as i8 + octave * 12).clamp(0, 127) as u8));
        if note != current_note.map(|(_, n)| n) {
            if let Some((ch, n)) = current_note {
                audio.note_off(ch, n);
            }
            current_note = note.map(|n| (ui.performance.parts[ui.active_part].mix.channel, n));
            if let Some((ch, n)) = current_note {
                audio.note_on(ch, n, Velocity::DEFAULT);
            }
        }
```

- replace

```rust
        // Push params + modulation routes to the audio thread (part 0)
        let sound = &ui.performance.parts[0].sound;
        audio.update(&sound.params, &sound.mod_state, &ui.performance.fx);
```

with

```rust
        // Push every Part and the FX to the audio thread.
        audio.update(&ui.performance);
```

`Justfile` `check`: run the desktop's unit tests instead of only type-checking — `    cargo check -p chimera-desktop` → `    cargo test -p chimera-desktop`, and the comment's "desktop\n# type-check" → "desktop\n# build + unit tests".

- [ ] **Step 4: Run tests to verify they pass**

Run: `PKG_CONFIG_PATH=… cargo test -p chimera-desktop` — Expected: 2 passed, no new warnings in `chimera-desktop`.
Run: `just check` — Expected: 424 passed / 0 failed / 2 ignored; desktop 2 passed; firmware `Finished`.
Manual (on a machine with audio): `just desktop`; keys Z…N play Part 1; B2 then keys play Part 2 (channel 2); MIX+B2 → PART page, turn C to route Part 2 to P2, F2 solos pair 2.

- [ ] **Step 5: Commit**

```bash
git add chimera-desktop/src/audio.rs chimera-desktop/src/main.rs Justfile
git commit -m "feat(desktop): play the Instrument; piano follows the selected Part; solo a DAC pair

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Firmware maps all 288 KB of D2 and reserves the voice pool there

**Files:**
- Modify: `chimera-stm32/memory.x`, `chimera-stm32/build.rs`, `chimera-stm32/src/audio.rs`

**Interfaces:**
- Consumes: `chimera_core::instrument::Instrument` (Task 12).
- Produces: linker region `RAM_D2` = 288K at 0x3000_0000; output section `.ram_d2` = input `.ram_d2` (DMA buffers, first) then `.ram_d2.*`; `static mut INSTRUMENT: MaybeUninit<Instrument>` in `.ram_d2.voices`, `#[used]`, never initialised or read in this sub-project (the STM32 port adopts it). The single `VOICE` and raw-pointer params stay as they are (spec § Threading).

The "test" is the linker: the reservation must fail to link against today's 32 KB region and link against the real 288 KB, with `AUDIO_BUF` still at 0x3000_0000.

- [ ] **Step 1: Reserve the pool (the failing "test")**

In `chimera-stm32/src/audio.rs`: add `use core::mem::MaybeUninit;` above `use core::ptr::addr_of_mut;`, add `use chimera_core::instrument::Instrument;` below `use chimera_core::dsp::voice::Voice;`, and above `/// f32 work buffer for Voice rendering.`:

```rust
/// The voice pool's place in D2 SRAM, reserved for the port sub-project
/// (ADR 0014): the linker proves `Instrument` fits beside the DMA buffer.
/// Not initialised or read yet; the single `VOICE` below still plays.
#[used]
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();

```

- [ ] **Step 2: Run the build to verify it fails**

Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf`
Expected: FAIL — `rust-lld: error: section '.ram_d2' will not fit in region 'RAM_D2': overflowed by 214344 bytes` (512 B DMA buffer + 246,600 B pool − 32,768 B).

- [ ] **Step 3: Map the real D2 and order the section**

`chimera-stm32/memory.x`: replace `    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 32K` with

```
    /* D2 SRAM1 (128K) + SRAM2 (128K) + SRAM3 (32K), contiguous (RM0433 §2.3) */
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
```

`chimera-stm32/build.rs`: replace

```rust
    // Linker section for RAM_D2 audio buffers
    fs::write(
        out_dir.join("ram_d2.x"),
        r#"
SECTIONS {
    .ram_d2 (NOLOAD) : ALIGN(4) {
        *(.ram_d2 .ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
```

with

```rust
    // Linker section for RAM_D2: DMA buffers (`.ram_d2`) first so they keep
    // their address at 0x3000_0000, then the voice pool (`.ram_d2.voices`).
    fs::write(
        out_dir.join("ram_d2.x"),
        r#"
SECTIONS {
    .ram_d2 (NOLOAD) : ALIGN(4) {
        *(.ram_d2);
        *(.ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
```

(the `INSERT AFTER .uninit;` line and the rest of `build.rs` stay).

- [ ] **Step 4: Run the build to verify it links, and check the addresses**

Run: `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf` — Expected: `Finished`, no new warnings (the four existing `chimera-stm32` warnings remain).
Run: `nm -S ${CARGO_TARGET_DIR:-target}/thumbv7em-none-eabihf/debug/chimera-stm32 | grep -E 'AUDIO_BUF|INSTRUMENT'`
Expected:

```
30000000 00000200 b …audio9AUDIO_BUF…
30000200 0003c348 b …audio10INSTRUMENT…
```

Run: `just check` — Expected: 424 passed / 0 failed / 2 ignored; desktop 2 passed; firmware `Finished`.

- [ ] **Step 5: Commit**

```bash
git add chimera-stm32/memory.x chimera-stm32/build.rs chimera-stm32/src/audio.rs
git commit -m "build(stm32): map all 288 KB of D2 and reserve the voice pool there

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec deviations and resolved ambiguities

| Spec says | Plan does | Why |
|---|---|---|
| Budget assertions cover voices, Performance/SoundPool/FB/UI, AudioShared | Also asserts the FX bus (AXI) and the whole `Instrument` (D2) | ADR 0013 "all audio-core state"; the FX bus is 423,596 B and did not fit anywhere (ADR 0014) |
| Fix order (1) shrink Modal buffers | Done: 2,048 → 1,200 samples (E1 floor); `MAX_VOICES` stays 6 | Measured: 6 × 40,696 B fits D2 with 40 KB spare |
| Delay 10–1,000 ms | 10–500 ms | Only way the FX bus fits AXI without changing any sound ≤ 500 ms (ADR 0014) |
| `Instrument { voices, alloc, fx: FxBus }` | `Instrument { voices, alloc, … }`; `render(&mut self, fx: &mut FxBus, …)` | Pool (D2) and FX bus (AXI) must live in different regions |
| `note_on(…, cost, sounding_cost)` | `note_on(…, cost, reserved)`; the allocator keeps per-slot costs | Rule 4 must know what a steal frees; one source of truth |
| `AUDIO_CYCLE_BUDGET: u32` | `AUDIO_CYCLE_BUDGET: Cost` | ADR 0012 (the spec's own `Cost` type) |
| `Part` has flat channel/mode/output/level/pan/sends; `PartParams` is a Block | `Part::mix: PartParams` holds them | One struct is both the data and the Block |
| `PartAudio { params, mod_state, mode, output, level, pan, sends }` | `PartAudio { params, mod_state, mix: PartParams }` | Same data (plus the channel the spec routes by) |
| Level/pan get "modulation for free" | Not modulatable | ADR 0010: the mixer applies them, not the voice; per-voice sources cannot drive a part-level stage |
| `justfile` is missing | `Justfile` exists; only `check` changes | — |
| FX bus "stereo" | Mono effects, return on both sides of pair 1 | Effects are mono today; stereo is DSP work (out of scope) |
| Mixer chain: PART + SENDS pages | `[PART, SENDS, CHORUS, DELAY, REVERB]` | The FX pages had no other home once the mis-bound Mixer nodes went |
| B1–B6 select the Part | Also MIX + B<n> selects Part n | Mixer pages edit the Part they show |
| — | MIX + B6 stays Demo: Part 6's mixer pages are unreachable | Moving Demo is outside this spec; flagged |
| "Desktop can solo a pair" | F1–F3 solo, F4 all | Smallest binding that does not collide with existing keys |
| Sends semantics unspecified | Each effect processes the sum of its sends; an effect that is off returns silence; the effect's own `mix` still sets its internal dry/wet | Keeps effect DSP unchanged |
| Engine change mid-note (not in spec) | Voices re-costed each block; `shed` cuts the newest over budget | Review Focus 3 |
| Note-off routing (spec: by current channel at dequeue) | Note-offs release the voices their channel started | Review Focus 2: no stuck notes after a channel change |

Out of scope and untouched: MIDI input from hardware/USB/controllers, Sound/Performance files, the STM32 port of `Instrument`/`AudioShared` and 3-DAC DMA, measuring costs with DWT, glide/legato, composable chains, DSP quality (#10).

## Risks for the port sub-project (not fixed here)

- `Instrument::new` / `FxBus::new` build 250 KB values on the stack; the port must construct them in place in their `MaybeUninit` statics (ADR 0008).
- AXI headroom is ~13 KB and assumes a release build's stack; the stack has no guard.
- D2 SRAM clocks (RCC_AHB2ENR SRAM1/2/3EN) must be checked before the pool is written.
