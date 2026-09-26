# Instrument on the Chip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the PreenFM3 play what the desktop plays — six Parts on their MIDI channels sharing the 6-voice pool, level/pan/sends, each Part on one of three DAC pairs — from a DIN keyboard, with the chip's measured CPU load on screen, and let the desktop take real MIDI the same way.

**Architecture:** Functional core, imperative shell. Every algorithm (budgets, triple buffer, note sources, sample conversion, clock planning, load arithmetic, MIDI parsing, in-place construction) is a pure, host-tested piece of `chimera-core`/`chimera-hal`, built first. Then the desktop shell adopts the triple buffer and MIDI input, then the firmware shell is brought up in the spec's six flashable steps (clocks → pair 1 → pairs 2–3 → Instrument + DIN → publishing → measurement), each owning only registers, interrupts and statics.

**Tech Stack:** Rust 2024, `no_std` core, `cortex-m` 0.7.7, `cortex-m-rt` 0.7.5, `stm32h7xx-hal` 0.16 (PAC `stm32h7` 0.15.1, `stm32h743` module), `cpal` 0.15, `midir` 0.10.4, `minifb` 0.28, `just`.

**Spec:** `docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md` (approved, binding). Register-level evidence: the adversarial review behind it (stock PreenFM3 `main.c`, ST HAL, stm32h7xx-hal 0.16), summarised where each firmware task uses it.

## Global Constraints

- Functional core / imperative shell: algorithms are pure functions over plain data in `chimera-core`/`chimera-hal`, tested on the host; `chimera-stm32` and `chimera-desktop` only own peripherals, interrupts and statics.
- Type-driven design (ADR 0012): `MidiChannel` in parsed messages, `DacSample`, `SiliconRev`, `Priority`, `SampleBudget`/`BlockBudget`, the `Writer`/`Reader` halves of a take-once triple buffer, `SourceId`.
- Features cuttable by module and Cargo feature: firmware `midi-din` and `perf-probe` (default on) and `bench` (off); desktop `midi` (default on). Cutting one means deleting its module and its flag.
- CLAUDE.md's rules: a `// SAFETY:` comment on every `unsafe`; no allocation or blocking in the audio path; no libc; nothing snaps.
- Audio goldens bit-identical (`golden_test.rs`, `fx_golden_test.rs`, `signal_chain_test.rs`, …): no task may re-record an audio golden.
- Never stage `docs/chimera-ui-ux-spec.md` (it has unrelated local edits): always `git add` explicit paths, never `git add -A`/`git add .`/`git commit -a`.
- Build/test command: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
- One commit per task. Commit messages: no conventional prefixes (no `fix:`, `feat:`, `style:`, `docs:`, `chore:`, `refactor:`, `test:`), no `Co-Authored-By` trailer and no Claude attribution anywhere; terse, plain and short, saying what changed (e.g. `TripleBuffer for AudioShared`).
- Code comments only where genuinely needed: a `// SAFETY:` justification (required by CLAUDE.md), a non-obvious "why", or a `ponytail:` ceiling note. No other doc or inline comments in new code; `/// # Safety` sections stay on `unsafe fn`s.
- Run `cargo fmt --all` before each check: the plan's code blocks are not hand-wrapped to rustfmt's width.
- Hardware facts used throughout (stock PreenFM3 firmware, ST HAL, stm32h7xx-hal 0.16):
  - DBGMCU_IDC.REV_ID: `0x1003` = rev Y, `0x2003` = rev V; the SAI MCKEN bit exists only from rev B on (REV_ID ≥ `0x2000`).
  - stm32h7xx-hal gates `Pwr::vos0` on its `revision_v` feature; `sys_ck(480 MHz)` needs `PllConfigStrategy::Iterative` (the default `Normal` strategy caps the VCO at 420 MHz and panics).
  - SAI data register is right-aligned: 24-bit samples left-justified in 32 bits need DS = 32 bits (stock `main.c:391`).
  - SAI1 A master with GCR SYNCOUT = 01; SAI1 B SYNCEN = 01; SAI2 A SYNCEN = 10 with SAI2 GCR SYNCIN = 00. Pins PE2 MCLK, PE4 FS, PE5 SCK, PE6 SD-A1, PE3 SD-B1 on AF6; PD11 SD-A2 on **AF10**.
  - DMA1 streams 0/1/2 ↔ DMAMUX1 request IDs 87/88/89 (SAI1_A, SAI1_B, SAI2_A).
  - USART1 RX FIFO: `FIFOEN` (CR1 bit 29) is writable only while `UE = 0`; ORE with RXNEIE refires forever unless `ICR.ORECF` is written.
  - Cortex-M7 NVIC keeps the upper 4 priority bits; `set_priority(…, 3)` is priority 0.

## Review Focus

The five input classes the spec implies but no feature test would otherwise exercise, most likely first. Each has its pinning test in the owning task.

1. **A non-finite sample reaching the DAC** (a filter blowing up gives NaN or ±∞): NaN must come out as digital silence and ±∞ as full scale, never an arbitrary word. → Task 4, `non_finite_input_is_silent_or_clamped`.
2. **DIN cable hot-plug and truncated messages** (a status byte cut off mid-message, junk bytes between messages): the parser must resync on the next status byte and emit exactly the complete messages that follow. → Task 7, table rows `truncated_message_resyncs_on_next_status` and `hot_plug_junk_then_note`.
3. **A silicon revision that is neither V nor Y** (rev X `0x2001`, or `0x0000` if DBGMCU reads blank without a debugger): the chip must fall back to 400 MHz and still pick the SAI divider formula and MCKEN by REV_ID ≥ `0x2000`. → Task 5, `unknown_revisions_fall_back_to_400_mhz_and_sai_by_rev_id`.
4. **Desktop with no MIDI device, or only the ALSA loopback port**: the simulator must start and play from the computer keyboard, not panic or open "Midi Through". → Task 11, `no_ports_or_only_loopback_picks_nothing`.
5. **AUDIO page with extreme counters** (load over 100 % after overruns, counters near `u32::MAX`): every number must stay readable and nothing may draw outside 240×320. → Task 17, `extreme_stats_stay_on_screen`.

---

## File Structure

Created:

| File | Responsibility |
|---|---|
| `chimera-core/src/triple.rs` | `TripleBuffer<T>`, `Writer<T>`, `Reader<T>` (ADR 0021) |
| `chimera-core/src/audio_out.rs` | `DacSample`, `to_dac`, `interleave`, `Half`, `plan_halves`, `desynced` |
| `chimera-core/src/clock_plan.rs` | `SiliconRev`, `Pll3Config`, `pll3_for`, `fs_of`, cycle helpers |
| `chimera-core/src/perf/mod.rs`, `perf/load.rs`, `perf/stack.rs` | `AudioStats`, `load_percent`; stack-paint arithmetic |
| `chimera-core/src/in_place.rs` | `uninit_at`, `by_value`, `field_list!` for in-place constructors |
| `chimera-core/src/ui/audio_page.rs` | System ▸ About ▸ AUDIO sub-page drawing and dirty keys |
| `chimera-hal/tests/midi_parser_test.rs` | parser behaviour table |
| `chimera-core/tests/{triple_buffer,audio_out,clock_plan,perf_load,in_place,scope,audio_page}_test.rs` | host tests |
| `chimera-desktop/src/midi.rs` | midir port choice (feature `midi`) |
| `chimera-stm32/src/{clocks,cache,priority,panic,shared,midi_din,probe,bench}.rs` | firmware shell modules |
| `chimera-stm32/src/audio/{mod,sai,dma,engine}.rs` | SAI blocks, DMA rings + ISR, render engine (replaces `audio.rs`) |
| `docs/adr/0019-note-input-per-source-queues.md`, `0020-audio-clocking-and-output.md`, `0021-take-once-triple-buffer.md` | ADRs |

Modified: `chimera-core/src/{hw,voice_alloc,instrument,note_queue,scope,part,lib}.rs`, `dsp/{voice,engines,modal,fx_bus,chorus,delay,reverb}.rs`, `ui/{mod,renderer,block_def,block_registry,focus}.rs`; `chimera-hal/src/{lib,midi}.rs`; `chimera-desktop/{Cargo.toml,src/audio.rs,src/main.rs}`; `chimera-stm32/{Cargo.toml,memory.x,build.rs,src/main.rs,src/controls.rs,src/display.rs}`; `Justfile`; tests listed per task; `docs/adr/README.md`, `docs/adr/0013-hardware-parity-budgets.md` (status line only).

Names used across tasks (the Interfaces blocks repeat what each task needs):

- `hw::{CPU_HZ_REV_V, CPU_HZ_REV_Y, AUDIO_BUDGET_PERCENT, SampleBudget, BlockBudget}` — Task 1
- `triple::{TripleBuffer, Writer, Reader}` — Task 2
- `note_queue::{MAX_NOTE_SOURCES, SourceId, NoteSources}` — Task 3; `NoteEvent::from_midi` — Task 7
- `audio_out::{DacSample, to_dac, interleave, Half, HalfPlan, plan_halves, desynced}`, `part::DacPair::ALL` — Task 4
- `clock_plan::{SiliconRev, PllRange, VcoRange, Pll3Config, pll3_for, fs_of, vco_hz, cycles_for_us, cycles_for_ns, systick_reload, SYSTICK_MAX_RELOAD, SAI_KER_PER_FS}` — Task 5
- `perf::load::{AudioStats, AVG_BLOCKS, load_percent}`, `perf::stack::{STACK_PAINT, untouched_words}` — Task 6
- `Instrument::init_in_place`, `FxBus::init_in_place`, `Voice::init_in_place` — Task 8
- `scope::{ScopeFrame, ScopeWriter, scope_buffer}` — Task 9
- `UiState::{render_with_audio, render_dirty_with_audio}`, `ui::audio_page`, `block_registry::SYS_AUDIO` — Task 17

---

### Task 1: `SampleBudget`/`BlockBudget`, explicit budgets for the allocator and `Instrument`, ADR 0020

**Files:**
- Modify: `chimera-core/src/hw.rs` (remove `CPU_HZ`, `CYCLES_PER_SAMPLE`, `AUDIO_CYCLE_BUDGET`; add budget types)
- Modify: `chimera-core/src/voice_alloc.rs` (`Allocator` stores its budget; drop `impl Default`)
- Modify: `chimera-core/src/instrument.rs` (`Instrument::new(sample_rate, budget)`)
- Modify: `chimera-desktop/src/audio.rs:54`
- Test: `chimera-core/tests/hw_test.rs`, `cost_test.rs`, `voice_alloc_test.rs`, `instrument_test.rs`, `tests/common/mod.rs:175`
- Create: `docs/adr/0020-audio-clocking-and-output.md`
- Modify: `docs/adr/README.md`, `docs/adr/0013-hardware-parity-budgets.md` (status line only)

**Interfaces:**
- Consumes: `hw::Cost`, `hw::{SAMPLE_RATE, BLOCK_SIZE}`.
- Produces:
  - `pub const CPU_HZ_REV_V: u32 = 480_000_000; pub const CPU_HZ_REV_Y: u32 = 400_000_000; pub const AUDIO_BUDGET_PERCENT: u32 = 70;`
  - `pub struct SampleBudget(u32)` with `pub const fn for_cpu(cpu_hz: u32) -> SampleBudget` and `pub const fn as_cost(self) -> Cost` (cycles per sample, 70 %).
  - `pub struct BlockBudget(u32)` with `pub const fn for_cpu(cpu_hz: u32) -> BlockBudget`, `pub const fn block_cycles(self) -> u32` (the 64-sample deadline) and `pub const fn budget_cycles(self) -> u32` (70 % of it).
  - `Allocator::new(budget: SampleBudget) -> Allocator`, `Allocator::budget(&self) -> SampleBudget`.
  - `Instrument::new(sample_rate: u32, budget: SampleBudget) -> Instrument`.

- [ ] **Step 1: Write the failing budget tests**

Replace `chimera-core/tests/hw_test.rs` with:

```rust

use chimera_core::hw::{self, BlockBudget, Cost, SampleBudget};

#[test]
fn audio_timing_matches_the_chip() {
    assert_eq!(hw::SAMPLE_RATE, chimera_hal::SAMPLE_RATE);
    assert_eq!(hw::BLOCK_SIZE, chimera_hal::BLOCK_SIZE);
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_V).as_cost(),
        Cost(7_000)
    );
}

#[test]
fn budgets_follow_the_cpu_clock() {
    assert_eq!(hw::CPU_HZ_REV_V, 480_000_000);
    assert_eq!(hw::CPU_HZ_REV_Y, 400_000_000);
    assert_eq!(hw::AUDIO_BUDGET_PERCENT, 70);
    assert_eq!(
        SampleBudget::for_cpu(hw::CPU_HZ_REV_Y).as_cost(),
        Cost(5_833)
    );
    let v = BlockBudget::for_cpu(hw::CPU_HZ_REV_V);
    assert_eq!((v.block_cycles(), v.budget_cycles()), (640_000, 448_000));
    let y = BlockBudget::for_cpu(hw::CPU_HZ_REV_Y);
    assert_eq!((y.block_cycles(), y.budget_cycles()), (533_333, 373_333));
}

#[test]
fn capacity_constants() {
    assert_eq!((hw::MAX_VOICES, hw::MAX_PARTS, hw::DAC_PAIRS), (6, 6, 3));
}

#[test]
fn memory_regions_match_the_h750() {
    assert_eq!(hw::AXI_SRAM, 524_288);
    assert_eq!(hw::D2_SRAM, 294_912);
    assert_eq!(hw::DTCM, 131_072);
}

#[test]
fn costs_add_and_compare() {
    assert_eq!(Cost(610) + Cost(1_210), Cost(1_820));
    assert_eq!(
        [Cost(1), Cost(2), Cost(3)].into_iter().sum::<Cost>(),
        Cost(6)
    );
    assert!(Cost(7_001) > SampleBudget::for_cpu(hw::CPU_HZ_REV_V).as_cost());
    assert_eq!(Cost::ZERO, Cost(0));
}
```

In `chimera-core/tests/voice_alloc_test.rs`, change the imports and add the budget constant at the top:

```rust
use chimera_core::MidiNote;
use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};
use chimera_core::part::PartMode::{self, Mono, Poly};
use chimera_core::voice_alloc::{Alloc, Allocator};

const BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);
```

then run, from the repository root:

```bash
sed -i 's/Allocator::new()/Allocator::new(BUDGET)/' chimera-core/tests/voice_alloc_test.rs
sed -i 's/a.sounding_cost() + FX <= AUDIO_CYCLE_BUDGET, "{ctx}"/a.sounding_cost() + FX <= BUDGET.as_cost(), "{ctx}"/' chimera-core/tests/voice_alloc_test.rs
grep -c 'Allocator::new(BUDGET)' chimera-core/tests/voice_alloc_test.rs
```

Expected: `15`. Append this test to the same file:

```rust
#[test]
fn allocator_honours_the_budget_it_was_given() {
    const MODAL: Cost = Cost(1_210);
    const FX: Cost = Cost(600);
    let sounding = |cpu_hz: u32| {
        let mut a = Allocator::new(SampleBudget::for_cpu(cpu_hz));
        for i in 0..MAX_VOICES as u8 {
            a.note_on(i, Poly, n(60 + i), MODAL, FX);
        }
        assert_eq!(a.budget(), SampleBudget::for_cpu(cpu_hz));
        a.slots().iter().filter(|s| !s.is_free()).count()
    };
    assert_eq!(sounding(480_000_000), 5);
    assert_eq!(sounding(400_000_000), 4);
}
```

In `chimera-core/tests/cost_test.rs` change line 7 to `use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};` and the `fits` closure in `budget_capacity_per_engine` to:

```rust
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * Voice::cost(e).0 <= budget.0;
```

In `chimera-core/tests/instrument_test.rs` add after the existing `use` lines:

```rust
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};

const BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);
```

change `Rig::new`'s `inst: Box::new(Instrument::new(SR)),` to `inst: Box::new(Instrument::new(SR, BUDGET)),`, and in `sound_change_mid_chord_stays_in_budget` delete the line `use chimera_core::hw::AUDIO_CYCLE_BUDGET;` and change the assertion to `assert!(a.sounding_cost() + FxBus::COST <= BUDGET.as_cost());`.

In `chimera-core/tests/common/mod.rs` change line 175 to:

```rust
    let mut inst = Box::new(Instrument::new(
        chimera_hal::SAMPLE_RATE,
        chimera_core::hw::SampleBudget::for_cpu(chimera_core::hw::CPU_HZ_REV_V),
    ));
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p chimera-core --test hw_test`
Expected: FAIL to compile with `cannot find type SampleBudget in module hw` (and `BlockBudget`, `CPU_HZ_REV_V`).

- [ ] **Step 3: Implement the budget types**

In `chimera-core/src/hw.rs` replace lines 13–16 (`CPU_HZ`, `CYCLES_PER_SAMPLE`, `AUDIO_CYCLE_BUDGET`) with:

```rust
pub const CPU_HZ_REV_V: u32 = 480_000_000;
pub const CPU_HZ_REV_Y: u32 = 400_000_000;

pub const AUDIO_BUDGET_PERCENT: u32 = 70;

// No `Default`: a rev Y chip must never silently get the 480 MHz budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleBudget(u32);

impl SampleBudget {
    pub const fn for_cpu(cpu_hz: u32) -> Self {
        Self((cpu_hz as u64 * AUDIO_BUDGET_PERCENT as u64 / (100 * SAMPLE_RATE as u64)) as u32)
    }

    pub const fn as_cost(self) -> Cost {
        Cost(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockBudget(u32);

impl BlockBudget {
    pub const fn for_cpu(cpu_hz: u32) -> Self {
        Self((cpu_hz as u64 * BLOCK_SIZE as u64 / SAMPLE_RATE as u64) as u32)
    }

    pub const fn block_cycles(self) -> u32 {
        self.0
    }

    pub const fn budget_cycles(self) -> u32 {
        (self.0 as u64 * AUDIO_BUDGET_PERCENT as u64 / 100) as u32
    }
}
```

In `chimera-core/src/voice_alloc.rs`:
- change the import to `use crate::hw::{Cost, MAX_VOICES, SampleBudget};`
- add a field to `Allocator`, after `refused: u32,`:

```rust
    budget: SampleBudget,
```

- delete the whole `impl Default for Allocator { … }` block (a default would pick a clock silently);
- replace `pub fn new() -> Self` and its body with:

```rust
    pub fn new(budget: SampleBudget) -> Self {
        Self {
            slots: [VoiceSlot::default(); MAX_VOICES],
            clock: 0,
            rr: 0,
            refused: 0,
            budget,
        }
    }

    pub fn budget(&self) -> SampleBudget {
        self.budget
    }
```

- in `shed`, replace `if reserved + self.sounding_cost() <= AUDIO_CYCLE_BUDGET {` with `if reserved + self.sounding_cost() <= self.budget.as_cost() {`;
- in `pick`, replace `total.saturating_sub(freed.0) <= AUDIO_CYCLE_BUDGET.0` with `total.saturating_sub(freed.0) <= self.budget.as_cost().0`.

In `chimera-core/src/instrument.rs`:
- extend the `crate::hw` import with `SampleBudget`;
- replace `pub fn new(sample_rate: u32) -> Self {` … `alloc: Allocator::new(),` with:

```rust
    pub fn new(sample_rate: u32, budget: SampleBudget) -> Self {
        Self {
            voices: core::array::from_fn(|_| Voice::new(sample_rate)),
            alloc: Allocator::new(budget),
```

(the remaining fields are unchanged).

In `chimera-desktop/src/audio.rs` change the import `use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE};` to `use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SAMPLE_RATE, SampleBudget};` and line 54 to:

```rust
        let mut inst = Box::new(Instrument::new(sample_rate, SampleBudget::for_cpu(CPU_HZ_REV_V)));
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p chimera-core --test hw_test --test cost_test --test voice_alloc_test --test instrument_test --test golden_test`
Expected: PASS, every assertion that existed before unchanged.

- [ ] **Step 5: Write ADR 0020 and mark ADR 0013's budget clause superseded**

Create `docs/adr/0020-audio-clocking-and-output.md`:

```markdown
# 0020. Clock the chip by silicon revision; derive the cycle budget from it

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The instrument moves onto the PreenFM3 (STM32H750). Two silicon revisions
exist: rev V runs 480 MHz at VOS0 (stm32h7xx-hal's `revision_v` feature),
rev Y tops out at 400 MHz, and their SAI master-clock dividers differ (rev Y
halves MCLK, and only rev B and later have MCKEN). The firmware ran a fixed
400 MHz with MCKDIV = 5, which by the rev V formula plays 95.8 kHz — an
octave high. ADR 0013 made the cycle budget a compile-time constant for
480 MHz, which would over-budget a rev Y chip by 20 %. The SAI data
register is right-aligned, so a 24-bit left-justified sample needs 32-bit
data. The stack shared AXI with no guard, beside a 150 KB framebuffer.

## Decision
- **CPU speed by revision.** DBGMCU REV_ID `0x2003` (V) runs 480 MHz, VOS0;
  `0x1003` (Y) and anything unknown run 400 MHz. HCLK is half the core.
- **Runtime budget.** `SampleBudget::for_cpu(cpu_hz)` (70 % of
  `cpu_hz / 48 kHz`, the allocator's unit) and `BlockBudget::for_cpu`
  (the 64-sample deadline, the probe's) are built from the real clock and
  have no `Default`. This **supersedes ADR 0013's clause** that the cycle
  budget is a shared compile-time constant in `hw.rs`; the rest of 0013
  stands (the desktop still enforces the 480 MHz chip's budget).
- **48 kHz ± 10 ppm** from an 8 MHz HSE through fractional PLL3
  (`clock_plan::pll3_for`: 49.152 MHz SAI kernel, M 1, N 49, FRACN 1245,
  P 8, −0.46 ppm) and MCKDIV by revision: FS = ker / (MCKDIV × 256) from
  rev B (REV_ID ≥ `0x2000`, MCKDIV 4, MCKEN set), MCLK = ker / (2 × MCKDIV)
  on rev Y (MCKDIV 2, no MCKEN).
- **32-bit SAI slots carrying left-justified 24-bit samples** (`DacSample`),
  I2S, MCLK 256 × FS.
- **Three SAI blocks on one clock:** SAI1 A master exporting its sync,
  SAI1 B internal slave, SAI2 A external slave; three circular DMA streams
  started before the slaves, the master last; only stream 0 interrupts.
- **The stack in DTCM** (128 KB, zero-wait, CPU-only), with a painted
  high-water mark on the AUDIO page.

## Alternatives considered
- **480 MHz unconditionally** (stock PreenFM3 does this) — out of spec on a
  rev Y part; the REV_ID read costs nothing.
- **Keep a 480 MHz constant budget** — a rev Y chip would accept voices it
  can't render in time.
- **DS = 24 with right-aligned sign-extended samples** — works, but diverges
  from stock and gains nothing; the CS4344 reads the top 24 bits of a
  32-bit slot either way.
- **Integer PLL3 (stock's 122.67 MHz, MCKDIV 10)** — 47,917 Hz, 3 cents flat.
- **SAI2 as a second master** — breaks the three-stream DMA lockstep.

## Consequences
Pitch is exact on both revisions; a rev Y chip gets fewer heavy voices, by
the same allocator code. `Instrument::new` and `Allocator::new` take the
budget. The display SPI kernel (PLL1 Q) is 192 MHz on rev V (the 960 MHz
VCO has no 200 MHz divisor) and pclk2 is 120 MHz; the SPI and USART
dividers are computed from the real clocks. DTCM holds `main`'s frame
(`UiState`), so a debug firmware is only link-checked.

## Sources
`docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md`;
`docs/superpowers/plans/2026-09-26-instrument-on-chip.md`; stock PreenFM3
firmware (Ixox/preenfm3, `firmware/Src/main.c`: SAI setup and pins, VOS0);
ST STM32H7 HAL (`stm32h7xx_hal_sai.c`: MCKDIV formula, MCKEN from rev B);
stm32h7xx-hal 0.16 (`pwr.rs`, `rcc/pll.rs`); RM0433.
```

In `docs/adr/0013-hardware-parity-budgets.md` change only the status line to:

```markdown
- **Status:** Accepted (2026-09-24); the cycle-budget clause is superseded by [0020](0020-audio-clocking-and-output.md)
```

In `docs/adr/README.md` change the 0013 row's status cell to `Accepted; budget clause superseded by 0020` and append after the 0017 row:

```markdown
| [0020](0020-audio-clocking-and-output.md) | Clock the chip by silicon revision; derive the cycle budget from it | Accepted |
```

- [ ] **Step 6: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: all tests pass, desktop and firmware build, clippy and fmt clean.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/hw.rs chimera-core/src/voice_alloc.rs chimera-core/src/instrument.rs \
  chimera-desktop/src/audio.rs chimera-core/tests/hw_test.rs chimera-core/tests/cost_test.rs \
  chimera-core/tests/voice_alloc_test.rs chimera-core/tests/instrument_test.rs \
  chimera-core/tests/common/mod.rs docs/adr/0020-audio-clocking-and-output.md \
  docs/adr/0013-hardware-parity-budgets.md docs/adr/README.md
git commit -m "Runtime SampleBudget and BlockBudget from the CPU clock (ADR 0020)"
```

---

### Task 2: `TripleBuffer` with take-once halves, ADR 0021

**Files:**
- Create: `chimera-core/src/triple.rs`
- Modify: `chimera-core/src/lib.rs` (add `pub mod triple;`)
- Test: `chimera-core/tests/triple_buffer_test.rs`
- Create: `docs/adr/0021-take-once-triple-buffer.md`; Modify: `docs/adr/README.md`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct TripleBuffer<T>`; `pub const fn new(first: T, second: T, third: T) -> Self` (the reader starts on `first`);
  - `pub fn init_in_place(slot: &mut MaybeUninit<Self>, init: impl FnMut() -> T) -> &mut Self` (no stack copy of the whole buffer);
  - `impl<T: 'static> TripleBuffer<T> { pub fn split(&'static mut self) -> (Writer<T>, Reader<T>) }`;
  - `Writer::publish(&mut self, f: impl FnOnce(&mut T))` — `f` gets the writer's slot, which holds an older value;
  - `Reader::read(&mut self) -> &T` — newest completed publish, held until the next `read`;
  - `Writer<T>: Send`, `Reader<T>: Send` for `T: Send`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/triple_buffer_test.rs`:

```rust

use chimera_core::triple::{Reader, TripleBuffer, Writer};

fn pair<T: Send + 'static>(a: T, b: T, c: T) -> (Writer<T>, Reader<T>) {
    Box::leak(Box::new(TripleBuffer::new(a, b, c))).split()
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[test]
fn reader_starts_on_the_first_value() {
    let (_w, mut r) = pair(1u32, 2, 3);
    assert_eq!(*r.read(), 1);
}

#[test]
fn reader_gets_the_newest_publish() {
    let (mut w, mut r) = pair(0u32, 0, 0);
    w.publish(|x| *x = 1);
    w.publish(|x| *x = 2);
    w.publish(|x| *x = 3);
    assert_eq!(*r.read(), 3);
    assert_eq!(*r.read(), 3, "nothing newer: the same value again");
}

#[test]
fn a_held_read_is_stable_across_publishes() {
    let (mut w, mut r) = pair([0u64; 8], [0; 8], [0; 8]);
    w.publish(|x| *x = [7; 8]);
    let held = r.read();
    for n in 8..20 {
        w.publish(|x| *x = [n; 8]);
    }
    assert_eq!(*held, [7; 8]);
    assert_eq!(*r.read(), [19; 8]);
}

#[test]
fn random_interleavings_match_the_model() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut w, mut r) = pair([0u64; 4], [0; 4], [0; 4]);
    let mut latest = 0u64;
    let mut next = 1u64;
    for step in 0..20_000 {
        let held = r.read();
        assert_eq!(*held, [latest; 4], "step {step}: read is the newest publish");
        let held_addr = held as *const [u64; 4] as usize;
        let snapshot = *held;
        for _ in 0..rng.below(5) {
            let value = next;
            w.publish(|x| {
                assert_ne!(
                    x as *mut [u64; 4] as usize,
                    held_addr,
                    "step {step}: the writer was handed the held slot"
                );
                *x = [value; 4];
            });
            latest = value;
            next += 1;
        }
        assert_eq!(*held, snapshot, "step {step}: the held read changed");
    }
}

#[test]
fn a_second_thread_never_sees_a_torn_or_older_value() {
    const N: u64 = 200_000;
    let (mut w, mut r) = pair([0u64; 16], [0; 16], [0; 16]);
    let writer = std::thread::spawn(move || {
        for n in 1..=N {
            w.publish(|x| *x = [n; 16]);
        }
    });
    let mut last = 0;
    while last < N {
        let done = writer.is_finished();
        let v = *r.read();
        assert!(v.iter().all(|&e| e == v[0]), "torn read {v:?}");
        assert!(v[0] >= last, "went back from {last} to {}", v[0]);
        last = v[0];
        if done {
            assert_eq!(last, N, "the last publish is visible once the writer is done");
        }
    }
    writer.join().unwrap();
}

#[test]
fn init_in_place_starts_like_new() {
    let slot: &'static mut core::mem::MaybeUninit<TripleBuffer<u32>> =
        Box::leak(Box::new(core::mem::MaybeUninit::uninit()));
    let mut n = 0;
    let tb = TripleBuffer::init_in_place(slot, || {
        n += 1;
        n * 10
    });
    let (mut w, mut r) = tb.split();
    assert_eq!(*r.read(), 10, "the reader starts on the first value built");
    w.publish(|x| *x += 1);
    assert_eq!(*r.read(), 31, "the writer's first slot is the third value");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test triple_buffer_test`
Expected: FAIL to compile with `unresolved import chimera_core::triple`.

- [ ] **Step 3: Implement `triple.rs`**

Create `chimera-core/src/triple.rs`:

```rust

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU8, Ordering};

// The reader's `front`, the writer's `back` and `latest` are always a
// permutation of 0..3. Both swaps are AcqRel on `latest`: the writer's
// Release publishes its slot, the reader's Release hands its old front back
// before the writer can reuse it. Only the reader clears FRESH, so its first
// Relaxed load is just a hint.
const INDEX: u8 = 0b011;
const FRESH: u8 = 0b100;

pub struct TripleBuffer<T> {
    slots: [UnsafeCell<T>; 3],
    latest: AtomicU8,
}

impl<T> TripleBuffer<T> {
    pub const fn new(first: T, second: T, third: T) -> Self {
        Self {
            slots: [
                UnsafeCell::new(first),
                UnsafeCell::new(second),
                UnsafeCell::new(third),
            ],
            latest: AtomicU8::new(1),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>, mut init: impl FnMut() -> T) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from a live `&mut MaybeUninit<Self>`, so it is
        // valid, aligned and unaliased. Each of the three slots and `latest`
        // is written exactly once, through raw field pointers (no reference
        // to uninitialised memory is made), before `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).slots).cast::<UnsafeCell<T>>();
            for i in 0..3 {
                slots.add(i).write(UnsafeCell::new(init()));
            }
            addr_of_mut!((*p).latest).write(AtomicU8::new(1));
            slot.assume_init_mut()
        }
    }
}

impl<T: 'static> TripleBuffer<T> {
    pub fn split(&'static mut self) -> (Writer<T>, Reader<T>) {
        let buf: &'static Self = self;
        (Writer { buf, back: 2 }, Reader { buf, front: 0 })
    }
}

pub struct Writer<T> {
    buf: &'static TripleBuffer<T>,
    back: usize,
}

pub struct Reader<T> {
    buf: &'static TripleBuffer<T>,
    front: usize,
}

// SAFETY: a `Writer` touches only its `back` slot, which neither `latest`
// nor the reader's `front` names, and the index handover is atomic. Moving
// it to another thread moves `T` values across threads, hence `T: Send`.
unsafe impl<T: Send> Send for Writer<T> {}
// SAFETY: a `Reader` touches only its `front` slot, which the writer never
// writes until the reader swaps it back out; `T: Send` as for `Writer`.
unsafe impl<T: Send> Send for Reader<T> {}

impl<T> Writer<T> {
    pub fn publish(&mut self, f: impl FnOnce(&mut T)) {
        // SAFETY: `back` is the writer's own slot (the permutation
        // invariant): no other reference to it exists while `f` runs.
        f(unsafe { &mut *self.buf.slots[self.back].get() });
        let prev = self.buf.latest.swap(self.back as u8 | FRESH, Ordering::AcqRel);
        self.back = (prev & INDEX) as usize;
    }
}

impl<T> Reader<T> {
    pub fn read(&mut self) -> &T {
        if self.buf.latest.load(Ordering::Relaxed) & FRESH != 0 {
            let prev = self.buf.latest.swap(self.front as u8, Ordering::AcqRel);
            self.front = (prev & INDEX) as usize;
        }
        // SAFETY: `front` is the reader's own slot; the writer never writes
        // it until a later `read` swaps it out, and that `read` needs
        // `&mut self`, which this returned borrow prevents.
        unsafe { &*self.buf.slots[self.front].get() }
    }
}
```

Add `pub mod triple;` to `chimera-core/src/lib.rs` after `pub mod scope;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-core --test triple_buffer_test`
Expected: PASS (6 tests). Optional deeper check if a nightly toolchain with Miri is installed: `cargo +nightly miri test -p chimera-core --test triple_buffer_test -- --skip second_thread` — expect no UB reports.

- [ ] **Step 5: Write ADR 0021**

Create `docs/adr/0021-take-once-triple-buffer.md`:

```markdown
# 0021. Audio↔UI shared state uses a take-once triple buffer

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The UI hands the audio side a fresh `AudioShared` every frame, and the
audio side hands back the scope samples (and, on the chip, its load
statistics). The desktop swapped two `AudioShared` buffers through one
pointer: the cpal callback held `&A` for a whole callback (10–40 ms) while a
second `update` could write A — a data race (`chimera-desktop/src/audio.rs`
before this change). The scope used `static mut` front/back arrays with a
known reader race, which on the chip becomes a real interrupt preemption.

## Decision
`chimera_core::triple::TripleBuffer<T>`: three slots and one atomic index.
The writer only ever writes the slot that is neither published nor held;
the reader takes the newest publish and keeps it until its next `read`.
`split(&'static mut self)` returns a `Writer` and a `Reader` once (the
firmware adds an `AtomicBool` take-once guard around its statics); both
halves are `Send`. It carries `AudioShared` (UI → audio), the scope frame
and `AudioStats` (audio → UI) on both builds.

## Alternatives considered
- **Double buffer with an acknowledge index** — the publisher must skip
  frames while the reader holds the back buffer; more states, same memory
  saving.
- **Copy the front buffer at block start** — a 3 KB copy per 1.33 ms block,
  and the copy itself still races without a third buffer.
- **The `triple_buffer` crate** — `std`/`alloc`-oriented; ours is 80 lines,
  `no_std`, `const`-constructible and in-place-constructible.

## Consequences
One more `AudioShared` (3,184 B) plus the scope buffers in AXI, counted by
`AXI_RESIDENT`. The desktop race and the scope race are gone.

## Sources
`docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md` § Shared
state; `chimera-core/src/triple.rs`; `chimera-core/tests/triple_buffer_test.rs`.
```

Append to `docs/adr/README.md` after the 0020 row:

```markdown
| [0021](0021-take-once-triple-buffer.md) | Audio↔UI shared state uses a take-once triple buffer | Accepted |
```

- [ ] **Step 6: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/triple.rs chimera-core/src/lib.rs chimera-core/tests/triple_buffer_test.rs \
  docs/adr/0021-take-once-triple-buffer.md docs/adr/README.md
git commit -m "TripleBuffer with take-once halves (ADR 0021)"
```

---
### Task 3: `NoteSources` — one queue per note source, fixed drain order, ADR 0019

**Files:**
- Modify: `chimera-core/src/note_queue.rs` (append `MAX_NOTE_SOURCES`, `SourceId`, `NoteSources`)
- Test: `chimera-core/tests/note_queue_test.rs` (append)
- Create: `docs/adr/0019-note-input-per-source-queues.md`; Modify: `docs/adr/README.md`

**Interfaces:**
- Consumes: `NoteQueue`, `NoteEvent`, `NOTE_QUEUE_LEN`.
- Produces:
  - `pub const MAX_NOTE_SOURCES: usize = 2;`
  - `pub struct SourceId<const N: usize>(usize)`; `pub const fn new(index: usize) -> Self` (panics — a compile error in a `const` — when `index >= N`); `pub const fn index(self) -> usize`.
  - `pub struct NoteSources<const N: usize>`; `pub const fn new() -> Self` (compile error unless `1 <= N <= MAX_NOTE_SOURCES`); `pub fn source(&self, id: SourceId<N>) -> &NoteQueue`; `pub fn drain(&self, f: impl FnMut(NoteEvent))` (source 0 first, each queue emptied); `pub fn drops(&self) -> [u32; N]`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/note_queue_test.rs` (and extend its `use` line to `use chimera_core::note_queue::{MAX_NOTE_SOURCES, NOTE_QUEUE_LEN, NoteEvent, NoteKind, NoteQueue, NoteSources, SourceId};`):

```rust
const A: SourceId<2> = SourceId::new(0);
const B: SourceId<2> = SourceId::new(1);

#[test]
fn drain_pops_every_source_in_fixed_order() {
    let s: NoteSources<2> = NoteSources::new();
    s.source(B).push(ev(1, 61, 100));
    s.source(A).push(ev(0, 60, 100));
    s.source(B).push(ev(1, 62, 0));
    let mut got = Vec::new();
    s.drain(|e| got.push(e));
    assert_eq!(got, [ev(0, 60, 100), ev(1, 61, 100), ev(1, 62, 0)]);
    s.drain(|_| panic!("already drained"));
}

#[test]
fn drops_are_counted_per_source() {
    let s: NoteSources<2> = NoteSources::new();
    for i in 0..NOTE_QUEUE_LEN + 3 {
        s.source(B).push(ev(0, (i % 128) as u8, 100));
    }
    assert_eq!(s.drops(), [0, 3]);
}

#[test]
fn source_ids_are_positions_in_the_drain_order() {
    assert_eq!((A.index(), B.index()), (0, 1));
    assert_eq!(MAX_NOTE_SOURCES, 2);
}

#[test]
#[should_panic(expected = "note source index out of range")]
fn a_runtime_source_id_past_n_panics() {
    let i = std::hint::black_box(2);
    let _ = SourceId::<2>::new(i);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test note_queue_test`
Expected: FAIL to compile: `no NoteSources in note_queue`.

- [ ] **Step 3: Implement**

Append to `chimera-core/src/note_queue.rs`:

```rust
pub const MAX_NOTE_SOURCES: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceId<const N: usize>(usize);

impl<const N: usize> SourceId<N> {
    pub const fn new(index: usize) -> Self {
        assert!(index < N, "note source index out of range");
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

pub struct NoteSources<const N: usize> {
    queues: [NoteQueue; N],
}

impl<const N: usize> Default for NoteSources<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> NoteSources<N> {
    pub const fn new() -> Self {
        const {
            assert!(N > 0);
            assert!(N <= MAX_NOTE_SOURCES);
        }
        Self {
            queues: [const { NoteQueue::new() }; N],
        }
    }

    pub fn source(&self, id: SourceId<N>) -> &NoteQueue {
        &self.queues[id.0]
    }

    pub fn drain(&self, mut f: impl FnMut(NoteEvent)) {
        for q in &self.queues {
            while let Some(ev) = q.pop() {
                f(ev);
            }
        }
    }

    pub fn drops(&self) -> [u32; N] {
        core::array::from_fn(|i| self.queues[i].dropped())
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-core --test note_queue_test`
Expected: PASS.

- [ ] **Step 5: Write ADR 0019**

Create `docs/adr/0019-note-input-per-source-queues.md`:

```markdown
# 0019. One parser and one single-producer queue per note source

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
Notes arrive from several contexts: the chip's DIN USART interrupt (and USB
in sub-project 2), the desktop's midir callback thread and its UI thread
(computer keyboard). `NoteQueue` is single-producer/single-consumer: two
producers sharing one queue would race on its tail.

## Decision
Each source owns one `MidiParser` (where it parses bytes) and one
`NoteQueue` inside `NoteSources<N>`, and is that queue's only producer.
The audio side drains every queue once per block in a fixed order (source 0
first) and passes each `NoteEvent` to `Instrument::handle`, which keeps the
routing: every Part on the event's channel plays it, and note-offs release
by the recorded note-on channel. There is no cross-source timestamp: within
one block a note-off from one source may be applied before a note-on from
another. Each queue counts its own drops (shown per source on the AUDIO page).

## Alternatives considered
- **One multi-producer queue** — needs a CAS loop or a lock in the interrupt.
- **Timestamps and a merge** — no source has a shared clock yet (tempo is
  sub-project 4); ordering inside 1.33 ms is inaudible.
- **Routing per source** — duplicates `Instrument::handle`'s channel logic.

## Consequences
Adding a source is a queue, a `SourceId` and a producer; `MAX_NOTE_SOURCES`
bounds the stats array. Sub-project 2 adds USB as a second chip source.

## Sources
`docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md` § Module
layout, § MIDI parsing; `chimera-core/src/note_queue.rs`.
```

In `docs/adr/README.md` insert before the 0020 row:

```markdown
| [0019](0019-note-input-per-source-queues.md) | One parser and one single-producer queue per note source | Accepted |
```

- [ ] **Step 6: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/note_queue.rs chimera-core/tests/note_queue_test.rs \
  docs/adr/0019-note-input-per-source-queues.md docs/adr/README.md
git commit -m "NoteSources, one queue per note source (ADR 0019)"
```

---

### Task 4: `audio_out` — `DacSample`, `to_dac`, `interleave`, half-buffer plan, desync check

**Files:**
- Create: `chimera-core/src/audio_out.rs`
- Modify: `chimera-core/src/lib.rs` (add `pub mod audio_out;`), `chimera-core/src/part.rs` (`DacPair::ALL`)
- Test: `chimera-core/tests/audio_out_test.rs`

**Interfaces:**
- Consumes: `instrument::DacOut` (`[[f32; BLOCK_SIZE * 2]; DAC_PAIRS]`, already L,R-interleaved per pair), `part::DacPair`.
- Produces:
  - `#[repr(transparent)] pub struct DacSample(i32)`; `pub const ZERO: DacSample`; `pub const fn get(self) -> i32`. Only `to_dac` builds non-zero values, so the low 8 bits are always 0.
  - `pub const DAC_FULL_SCALE: f32 = 8_388_607.0;`
  - `pub fn to_dac(x: f32) -> DacSample` — clamp to ±1.0, round to 24 bits, shift left 8; NaN → `ZERO`.
  - `pub fn interleave(dac: &DacOut, pair: DacPair, out: &mut [DacSample; BLOCK_SIZE * 2])`.
  - `pub enum Half { First, Second }` with `pub const fn index(self) -> usize`.
  - `pub struct HalfPlan { pub halves: [Option<Half>; 2], pub overrun: bool }`; `pub const fn plan_halves(half_done: bool, full_done: bool) -> HalfPlan`.
  - `pub const fn desynced(a: u16, b: u16, ring: u16, tolerance: u16) -> bool` (circular distance of two DMA NDTR values).
  - `DacPair::ALL: [DacPair; DAC_PAIRS]`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/audio_out_test.rs`:

```rust

use chimera_core::audio_out::{DacSample, Half, HalfPlan, desynced, interleave, plan_halves, to_dac};
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::instrument::DacOut;
use chimera_core::part::DacPair;

#[test]
fn full_scale_is_the_24_bit_extremes_left_justified() {
    assert_eq!(to_dac(1.0).get(), 0x7FFF_FF00);
    assert_eq!(to_dac(-1.0).get(), -0x7FFF_FF00);
    assert_eq!(to_dac(0.0), DacSample::ZERO);
    assert_eq!(DacSample::ZERO.get(), 0);
}

#[test]
fn out_of_range_clamps_to_full_scale() {
    assert_eq!(to_dac(1.5), to_dac(1.0));
    assert_eq!(to_dac(-7.0), to_dac(-1.0));
}

#[test]
fn rounds_to_the_nearest_24_bit_step() {
    assert_eq!(to_dac(0.5).get(), 4_194_304 << 8);
    assert_eq!(to_dac(-0.5).get(), -(4_194_304 << 8));
    assert_eq!(to_dac(1.0 / 8_388_607.0).get(), 1 << 8);
}

#[test]
fn the_low_byte_is_always_zero() {
    for i in -1000..=1000 {
        let x = i as f32 / 997.0;
        assert_eq!(to_dac(x).get() & 0xFF, 0, "{x}");
    }
}

#[test]
fn non_finite_input_is_silent_or_clamped() {
    assert_eq!(to_dac(f32::NAN), DacSample::ZERO);
    assert_eq!(to_dac(f32::INFINITY), to_dac(1.0));
    assert_eq!(to_dac(f32::NEG_INFINITY), to_dac(-1.0));
}

#[test]
fn interleave_converts_one_pair_in_slot_order() {
    let mut dac: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    for (p, pair) in dac.iter_mut().enumerate() {
        for (i, s) in pair.iter_mut().enumerate() {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            *s = sign * (p as f32 + 1.0) * 0.1 + i as f32 * 1e-4;
        }
    }
    for pair in DacPair::ALL {
        let mut out = [DacSample::ZERO; BLOCK_SIZE * 2];
        interleave(&dac, pair, &mut out);
        for i in 0..BLOCK_SIZE * 2 {
            assert_eq!(out[i], to_dac(dac[pair.index()][i]), "{pair:?} word {i}");
        }
    }
}

#[test]
fn dac_pairs_in_order() {
    assert_eq!(DacPair::ALL, [DacPair::P1, DacPair::P2, DacPair::P3]);
    assert_eq!((Half::First.index(), Half::Second.index()), (0, 1));
}

#[test]
fn half_transfer_renders_the_first_half() {
    assert_eq!(
        plan_halves(true, false),
        HalfPlan { halves: [Some(Half::First), None], overrun: false }
    );
}

#[test]
fn transfer_complete_renders_the_second_half() {
    assert_eq!(
        plan_halves(false, true),
        HalfPlan { halves: [Some(Half::Second), None], overrun: false }
    );
}

#[test]
fn both_flags_render_both_halves_oldest_first_and_count_one_overrun() {
    assert_eq!(
        plan_halves(true, true),
        HalfPlan { halves: [Some(Half::First), Some(Half::Second)], overrun: true }
    );
}

#[test]
fn no_flag_renders_nothing() {
    assert_eq!(plan_halves(false, false), HalfPlan { halves: [None, None], overrun: false });
}

#[test]
fn desync_tolerates_16_words_either_way_across_the_wrap() {
    assert!(!desynced(128, 128, 256, 16));
    assert!(!desynced(128, 144, 256, 16));
    assert!(!desynced(144, 128, 256, 16));
    assert!(!desynced(4, 244, 256, 16));
    assert!(desynced(128, 145, 256, 16));
    assert!(desynced(0, 128, 256, 16));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test audio_out_test`
Expected: FAIL to compile: `unresolved import chimera_core::audio_out`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/audio_out.rs`:

```rust

use crate::hw::BLOCK_SIZE;
use crate::instrument::DacOut;
use crate::part::DacPair;

pub const DAC_FULL_SCALE: f32 = 8_388_607.0;

// The SAI data register is right-aligned: with 32-bit data the CS4344 reads
// bits 31..8. Only `to_dac` builds one, so the low byte is always zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct DacSample(i32);

impl DacSample {
    pub const ZERO: DacSample = DacSample(0);

    pub const fn get(self) -> i32 {
        self.0
    }
}

pub fn to_dac(x: f32) -> DacSample {
    let steps = libm::roundf(x.clamp(-1.0, 1.0) * DAC_FULL_SCALE) as i32;
    DacSample(steps << 8)
}

pub fn interleave(dac: &DacOut, pair: DacPair, out: &mut [DacSample; BLOCK_SIZE * 2]) {
    for (o, &s) in out.iter_mut().zip(&dac[pair.index()]) {
        *o = to_dac(s);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    First,
    Second,
}

impl Half {
    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalfPlan {
    pub halves: [Option<Half>; 2],
    pub overrun: bool,
}

pub const fn plan_halves(half_done: bool, full_done: bool) -> HalfPlan {
    match (half_done, full_done) {
        (true, true) => HalfPlan {
            halves: [Some(Half::First), Some(Half::Second)],
            overrun: true,
        },
        (true, false) => HalfPlan {
            halves: [Some(Half::First), None],
            overrun: false,
        },
        (false, true) => HalfPlan {
            halves: [Some(Half::Second), None],
            overrun: false,
        },
        (false, false) => HalfPlan {
            halves: [None, None],
            overrun: false,
        },
    }
}

pub const fn desynced(a: u16, b: u16, ring: u16, tolerance: u16) -> bool {
    let d = (a as u32 + ring as u32 - b as u32) % ring as u32;
    let d = if d > ring as u32 - d { ring as u32 - d } else { d };
    d > tolerance as u32
}
```

In `chimera-core/src/part.rs` add inside `impl DacPair`, before `pub const fn index`:

```rust
    pub const ALL: [DacPair; crate::hw::DAC_PAIRS] = [DacPair::P1, DacPair::P2, DacPair::P3];
```

Add `pub mod audio_out;` to `chimera-core/src/lib.rs` (alphabetical, before `pub mod block;`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-core --test audio_out_test`
Expected: PASS (13 tests).

- [ ] **Step 5: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/audio_out.rs chimera-core/src/lib.rs chimera-core/src/part.rs \
  chimera-core/tests/audio_out_test.rs
git commit -m "DacSample, to_dac, interleave and the DMA half plan"
```

---

### Task 5: `clock_plan` — `SiliconRev`, `pll3_for`, `fs_of`, cycle helpers

**Files:**
- Create: `chimera-core/src/clock_plan.rs`
- Modify: `chimera-core/src/lib.rs` (add `pub mod clock_plan;`)
- Test: `chimera-core/tests/clock_plan_test.rs`

**Interfaces:**
- Consumes: `hw::{CPU_HZ_REV_V, CPU_HZ_REV_Y}` (Task 1).
- Produces:
  - `pub enum SiliconRev { Y, V, Unknown(u16) }` with `pub const REV_ID_Y: u16 = 0x1003`, `pub const REV_ID_V: u16 = 0x2003`, `pub const fn from_rev_id(id: u16) -> Self`, `pub const fn cpu_hz(self) -> u32`, `pub const fn new_sai(self) -> bool` (REV_ID ≥ `0x2000`: MCKEN exists, FS = ker / (MCKDIV × 256)), `pub const fn label(self) -> &'static str` (`"V"`, `"Y"`, `"?"`).
  - `pub enum PllRange { R1To2, R2To4, R4To8, R8To16 }`, `pub enum VcoRange { Wide, Medium }`.
  - `pub struct Pll3Config { pub m: u8, pub n: u16, pub fracn: u16, pub p: u8, pub range: PllRange, pub vco: VcoRange, pub mckdiv: u8 }`.
  - `pub const SAI_KER_PER_FS: u32 = 1024;`
  - `pub const fn pll3_for(hse_hz: u32, fs_hz: u32, rev: SiliconRev) -> Pll3Config`.
  - `pub fn vco_hz(c: &Pll3Config, hse_hz: u32) -> f64`, `pub fn fs_of(c: &Pll3Config, hse_hz: u32, rev: SiliconRev) -> f64`.
  - `pub const fn cycles_for_us(cpu_hz: u32, us: u32) -> u32`, `pub const fn cycles_for_ns(cpu_hz: u32, ns: u32) -> u32` (rounded up), `pub const fn systick_reload(cpu_hz: u32, tick_hz: u32) -> u32`, `pub const SYSTICK_MAX_RELOAD: u32 = 0x00FF_FFFF;`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/clock_plan_test.rs`:

```rust

use chimera_core::clock_plan::{
    Pll3Config, PllRange, SYSTICK_MAX_RELOAD, SiliconRev, VcoRange, cycles_for_ns, cycles_for_us,
    fs_of, pll3_for, systick_reload, vco_hz,
};

const HSE: u32 = 8_000_000;
const FS: u32 = 48_000;

fn ppm(fs: f64) -> f64 {
    (fs - FS as f64) / FS as f64 * 1e6
}

#[test]
fn pll3_plan_for_both_revisions() {
    let v = pll3_for(HSE, FS, SiliconRev::V);
    assert_eq!(
        v,
        Pll3Config { m: 1, n: 49, fracn: 1245, p: 8, range: PllRange::R8To16, vco: VcoRange::Wide, mckdiv: 4 }
    );
    let y = pll3_for(HSE, FS, SiliconRev::Y);
    assert_eq!(y, Pll3Config { mckdiv: 2, ..v });
}

#[test]
fn both_revisions_land_within_10_ppm_of_48_khz() {
    for rev in [SiliconRev::V, SiliconRev::Y] {
        let fs = fs_of(&pll3_for(HSE, FS, rev), HSE, rev);
        assert!(ppm(fs).abs() < 10.0, "{rev:?}: {fs} Hz ({} ppm)", ppm(fs));
        assert!(ppm(fs).abs() < 1.0, "{rev:?}: {fs} Hz");
    }
}

#[test]
fn every_field_is_in_range() {
    for rev in [SiliconRev::V, SiliconRev::Y] {
        let c = pll3_for(HSE, FS, rev);
        assert!((1..=63).contains(&c.m), "{rev:?} DIVM3 {}", c.m);
        assert!((4..=512).contains(&c.n), "{rev:?} DIVN3 {}", c.n);
        assert!((1..=128).contains(&c.p), "{rev:?} DIVP3 {}", c.p);
        assert!(c.fracn < 8192, "{rev:?} FRACN3 {}", c.fracn);
        let ref_hz = HSE / c.m as u32;
        let band = match c.range {
            PllRange::R1To2 => 1_000_000..=2_000_000,
            PllRange::R2To4 => 2_000_000..=4_000_000,
            PllRange::R4To8 => 4_000_000..=8_000_000,
            PllRange::R8To16 => 8_000_000..=16_000_000,
        };
        assert!(band.contains(&ref_hz), "{rev:?} ref {ref_hz} outside {:?}", c.range);
        assert_eq!(c.vco, VcoRange::Wide);
        let vco = vco_hz(&c, HSE);
        assert!((192e6..=836e6).contains(&vco), "{rev:?} VCO {vco}");
        let mckdiv_max = if rev.new_sai() { 63 } else { 15 };
        assert!((1..=mckdiv_max).contains(&c.mckdiv), "{rev:?} MCKDIV {}", c.mckdiv);
    }
}

#[test]
fn fs_of_reproduces_stock_and_the_old_firmware() {
    let stock = Pll3Config { m: 1, n: 46, fracn: 0, p: 3, range: PllRange::R8To16, vco: VcoRange::Wide, mckdiv: 10 };
    assert!((fs_of(&stock, HSE, SiliconRev::V) - 47_916.667).abs() < 0.01);
    let old = Pll3Config { mckdiv: 5, ..stock };
    assert!((fs_of(&old, HSE, SiliconRev::V) - 95_833.333).abs() < 0.01);
    assert!((fs_of(&old, HSE, SiliconRev::Y) - 47_916.667).abs() < 0.01);
}

#[test]
fn rev_ids_map_to_revisions() {
    assert_eq!(SiliconRev::from_rev_id(0x2003), SiliconRev::V);
    assert_eq!(SiliconRev::from_rev_id(0x1003), SiliconRev::Y);
    assert_eq!((SiliconRev::V.cpu_hz(), SiliconRev::Y.cpu_hz()), (480_000_000, 400_000_000));
    assert!(SiliconRev::V.new_sai() && !SiliconRev::Y.new_sai());
    assert_eq!((SiliconRev::V.label(), SiliconRev::Y.label()), ("V", "Y"));
}

#[test]
fn unknown_revisions_fall_back_to_400_mhz_and_sai_by_rev_id() {
    let x = SiliconRev::from_rev_id(0x2001);
    assert_eq!(x, SiliconRev::Unknown(0x2001));
    assert_eq!((x.cpu_hz(), x.new_sai(), x.label()), (400_000_000, true, "?"));
    let blank = SiliconRev::from_rev_id(0x0000);
    assert_eq!((blank.cpu_hz(), blank.new_sai()), (400_000_000, false));
    assert_eq!(pll3_for(HSE, FS, x).mckdiv, 4);
    assert_eq!(pll3_for(HSE, FS, blank).mckdiv, 2);
}

#[test]
fn cycle_helpers() {
    assert_eq!(cycles_for_us(480_000_000, 250_000), 120_000_000);
    assert_eq!(cycles_for_us(400_000_000, 1), 400);
    assert_eq!(cycles_for_ns(400_000_000, 250), 100);
    assert_eq!(cycles_for_ns(480_000_000, 250), 120);
    assert_eq!(cycles_for_ns(480_000_000, 1), 1, "rounds up, never 0");
}

#[test]
fn systick_reload_hits_500_hz_within_24_bits() {
    assert_eq!(systick_reload(480_000_000, 500), 959_999);
    assert_eq!(systick_reload(400_000_000, 500), 799_999);
    assert!(systick_reload(480_000_000, 500) <= SYSTICK_MAX_RELOAD);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test clock_plan_test`
Expected: FAIL to compile: `unresolved import chimera_core::clock_plan`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/clock_plan.rs`:

```rust

use crate::hw::{CPU_HZ_REV_V, CPU_HZ_REV_Y};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiliconRev {
    Y,
    V,
    Unknown(u16),
}

impl SiliconRev {
    pub const REV_ID_Y: u16 = 0x1003;
    pub const REV_ID_V: u16 = 0x2003;

    pub const fn from_rev_id(id: u16) -> Self {
        match id {
            Self::REV_ID_Y => SiliconRev::Y,
            Self::REV_ID_V => SiliconRev::V,
            other => SiliconRev::Unknown(other),
        }
    }

    pub const fn cpu_hz(self) -> u32 {
        match self {
            SiliconRev::V => CPU_HZ_REV_V,
            SiliconRev::Y | SiliconRev::Unknown(_) => CPU_HZ_REV_Y,
        }
    }

    // Rev B and later (REV_ID >= 0x2000, the ST HAL's test) have MCKEN and
    // FS = ker / (MCKDIV × 256); rev Y halves MCLK: ker / (2 × MCKDIV).
    pub const fn new_sai(self) -> bool {
        match self {
            SiliconRev::V => true,
            SiliconRev::Y => false,
            SiliconRev::Unknown(id) => id >= 0x2000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SiliconRev::V => "V",
            SiliconRev::Y => "Y",
            SiliconRev::Unknown(_) => "?",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PllRange {
    R1To2,
    R2To4,
    R4To8,
    R8To16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VcoRange {
    Wide,
    Medium,
}

// `n` and `p` are divide ratios; the registers hold ratio − 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pll3Config {
    pub m: u8,
    pub n: u16,
    pub fracn: u16,
    pub p: u8,
    pub range: PllRange,
    pub vco: VcoRange,
    pub mckdiv: u8,
}

// 49.152 MHz at 48 kHz: MCKDIV 4 (rev B+) or 2 (rev Y) gives MCLK = 256 × FS.
pub const SAI_KER_PER_FS: u32 = 1024;
const VCO_TARGET_HZ: u64 = 400_000_000;
const FRACN_ONE: u64 = 8192;

pub const fn pll3_for(hse_hz: u32, fs_hz: u32, rev: SiliconRev) -> Pll3Config {
    let ker = fs_hz as u64 * SAI_KER_PER_FS as u64;
    let m = (hse_hz as u64).div_ceil(16_000_000);
    let ref_hz = hse_hz as u64 / m;
    let p = (VCO_TARGET_HZ + ker / 2) / ker;
    // N + FRACN/8192 = ker × P / ref, rounded to the nearest 1/8192.
    let x = (ker * p * FRACN_ONE * 2 + ref_hz) / (2 * ref_hz);
    let range = if ref_hz >= 8_000_000 {
        PllRange::R8To16
    } else if ref_hz >= 4_000_000 {
        PllRange::R4To8
    } else if ref_hz >= 2_000_000 {
        PllRange::R2To4
    } else {
        PllRange::R1To2
    };
    let mckdiv = if rev.new_sai() {
        SAI_KER_PER_FS / 256
    } else {
        SAI_KER_PER_FS / 512
    };
    Pll3Config {
        m: m as u8,
        n: (x / FRACN_ONE) as u16,
        fracn: (x % FRACN_ONE) as u16,
        p: p as u8,
        range,
        vco: VcoRange::Wide,
        mckdiv: mckdiv as u8,
    }
}

pub fn vco_hz(c: &Pll3Config, hse_hz: u32) -> f64 {
    hse_hz as f64 / c.m as f64 * (c.n as f64 + c.fracn as f64 / FRACN_ONE as f64)
}

pub fn fs_of(c: &Pll3Config, hse_hz: u32, rev: SiliconRev) -> f64 {
    let ker = vco_hz(c, hse_hz) / c.p as f64;
    let div = match (c.mckdiv, rev.new_sai()) {
        (0, _) => 1.0,
        (d, true) => d as f64,
        (d, false) => 2.0 * d as f64,
    };
    ker / (div * 256.0)
}

pub const fn cycles_for_us(cpu_hz: u32, us: u32) -> u32 {
    (cpu_hz as u64 * us as u64 / 1_000_000) as u32
}

pub const fn cycles_for_ns(cpu_hz: u32, ns: u32) -> u32 {
    (cpu_hz as u64 * ns as u64).div_ceil(1_000_000_000) as u32
}

pub const SYSTICK_MAX_RELOAD: u32 = 0x00FF_FFFF;

pub const fn systick_reload(cpu_hz: u32, tick_hz: u32) -> u32 {
    cpu_hz / tick_hz - 1
}
```

Add `pub mod clock_plan;` to `chimera-core/src/lib.rs` (after `pub mod block;`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-core --test clock_plan_test`
Expected: PASS (8 tests).

- [ ] **Step 5: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/clock_plan.rs chimera-core/src/lib.rs chimera-core/tests/clock_plan_test.rs
git commit -m "clock_plan: SiliconRev, fractional PLL3 for 48 kHz, cycle helpers"
```

---

### Task 6: `AudioStats` and stack-paint arithmetic

**Files:**
- Create: `chimera-core/src/perf/mod.rs`, `chimera-core/src/perf/load.rs`, `chimera-core/src/perf/stack.rs`
- Modify: `chimera-core/src/lib.rs` (add `pub mod perf;`)
- Test: `chimera-core/tests/perf_load_test.rs`

**Interfaces:**
- Consumes: `hw::BlockBudget` (Task 1), `clock_plan::SiliconRev` (Task 5), `note_queue::MAX_NOTE_SOURCES` (Task 3).
- Produces:
  - `pub const AVG_BLOCKS: u32 = 64;`
  - `pub fn load_percent(cycles: u32, budget: BlockBudget) -> u16` — percent of the block deadline, rounded, saturating at `u16::MAX`.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct AudioStats { pub load_avg: u16, pub load_peak: u16, pub overruns: u32, pub desyncs: u32, pub drops: [u32; MAX_NOTE_SOURCES], pub sources: u8, pub stack_used: u32, pub rev: SiliconRev, pub cpu_hz: u32, /* private window state */ }`
  - `pub const fn AudioStats::new(rev: SiliconRev, cpu_hz: u32) -> AudioStats`; `pub fn record(&mut self, cycles: u32, budget: BlockBudget)`.
  - `pub const STACK_PAINT: u32 = 0xC0DE_C0DE;` `pub fn untouched_words(words: impl IntoIterator<Item = u32>) -> usize`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/perf_load_test.rs`:

```rust
use chimera_core::clock_plan::SiliconRev;
use chimera_core::hw::BlockBudget;
use chimera_core::note_queue::MAX_NOTE_SOURCES;
use chimera_core::perf::load::{AVG_BLOCKS, AudioStats, load_percent};
use chimera_core::perf::stack::{STACK_PAINT, untouched_words};

const V: BlockBudget = BlockBudget::for_cpu(480_000_000);

#[test]
fn load_is_a_percentage_of_the_block_deadline() {
    assert_eq!(load_percent(0, V), 0);
    assert_eq!(load_percent(320_000, V), 50);
    assert_eq!(load_percent(448_000, V), 70);
    assert_eq!(load_percent(640_000, V), 100);
}

#[test]
fn a_block_over_its_deadline_reads_over_100_and_saturates() {
    assert_eq!(load_percent(1_280_000, V), 200);
    assert_eq!(load_percent(u32::MAX, V), u16::MAX);
}

#[test]
fn average_is_the_mean_of_the_last_full_window() {
    let mut s = AudioStats::new(SiliconRev::V, 480_000_000);
    for _ in 0..AVG_BLOCKS - 1 {
        s.record(320_000, V);
    }
    assert_eq!(s.load_avg, 0, "no full window yet");
    s.record(320_000, V);
    assert_eq!(s.load_avg, 50);
    for i in 0..AVG_BLOCKS {
        s.record(if i % 2 == 0 { 64_000 } else { 192_000 }, V);
    }
    assert_eq!(s.load_avg, 20);
}

#[test]
fn peak_holds_the_worst_block_since_boot() {
    let mut s = AudioStats::new(SiliconRev::Y, 400_000_000);
    let y = BlockBudget::for_cpu(400_000_000);
    s.record(100_000, y);
    s.record(400_000, y);
    s.record(50_000, y);
    assert_eq!(s.load_peak, 75);
}

#[test]
fn new_stats_carry_the_chip_and_zero_counters() {
    let s = AudioStats::new(SiliconRev::V, 480_000_000);
    assert_eq!((s.rev, s.cpu_hz), (SiliconRev::V, 480_000_000));
    assert_eq!((s.load_avg, s.load_peak, s.overruns, s.desyncs, s.stack_used), (0, 0, 0, 0, 0));
    assert_eq!((s.drops, s.sources), ([0; MAX_NOTE_SOURCES], 0));
}

#[test]
fn untouched_words_counts_the_paint_from_the_bottom() {
    let p = STACK_PAINT;
    assert_eq!(untouched_words([p, p, p, 0, p]), 3);
    assert_eq!(untouched_words([p; 8]), 8);
    assert_eq!(untouched_words([1, p, p]), 0);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test perf_load_test`
Expected: FAIL to compile: `unresolved import chimera_core::perf`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/perf/mod.rs`:

```rust
pub mod load;
pub mod stack;
```

Create `chimera-core/src/perf/load.rs`:

```rust
use crate::clock_plan::SiliconRev;
use crate::hw::BlockBudget;
use crate::note_queue::MAX_NOTE_SOURCES;

pub const AVG_BLOCKS: u32 = 64;

pub fn load_percent(cycles: u32, budget: BlockBudget) -> u16 {
    let block = budget.block_cycles() as u64;
    ((cycles as u64 * 100 + block / 2) / block).min(u16::MAX as u64) as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStats {
    pub load_avg: u16,
    pub load_peak: u16,
    pub overruns: u32,
    pub desyncs: u32,
    pub drops: [u32; MAX_NOTE_SOURCES],
    pub sources: u8,
    pub stack_used: u32,
    pub rev: SiliconRev,
    pub cpu_hz: u32,
    window_sum: u32,
    window_len: u32,
}

impl AudioStats {
    pub const fn new(rev: SiliconRev, cpu_hz: u32) -> Self {
        Self {
            load_avg: 0,
            load_peak: 0,
            overruns: 0,
            desyncs: 0,
            drops: [0; MAX_NOTE_SOURCES],
            sources: 0,
            stack_used: 0,
            rev,
            cpu_hz,
            window_sum: 0,
            window_len: 0,
        }
    }

    pub fn record(&mut self, cycles: u32, budget: BlockBudget) {
        let pct = load_percent(cycles, budget);
        self.load_peak = self.load_peak.max(pct);
        self.window_sum += pct as u32;
        self.window_len += 1;
        if self.window_len == AVG_BLOCKS {
            self.load_avg = (self.window_sum / AVG_BLOCKS) as u16;
            self.window_sum = 0;
            self.window_len = 0;
        }
    }
}
```

Create `chimera-core/src/perf/stack.rs`:

```rust
pub const STACK_PAINT: u32 = 0xC0DE_C0DE;

pub fn untouched_words(words: impl IntoIterator<Item = u32>) -> usize {
    words.into_iter().take_while(|&w| w == STACK_PAINT).count()
}
```

Add `pub mod perf;` to `chimera-core/src/lib.rs` (after `pub mod part;`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-core --test perf_load_test`
Expected: PASS (6 tests).

- [ ] **Step 5: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/perf chimera-core/src/lib.rs chimera-core/tests/perf_load_test.rs
git commit -m "AudioStats load arithmetic and stack paint count"
```

---

### Task 7: `MidiChannel` in parsed messages, parser behaviour table, `NoteEvent::from_midi`

**Files:**
- Modify: `chimera-hal/src/lib.rs:142-164` (`MidiMessage` channels become `MidiChannel`)
- Modify: `chimera-hal/src/midi.rs` (`make_message` builds `MidiChannel`; `MidiParser::new` becomes `const`)
- Modify: `chimera-core/src/note_queue.rs` (`NoteEvent::from_midi`)
- Create: `chimera-hal/tests/midi_parser_test.rs`
- Test: `chimera-core/tests/midi_types_test.rs` (channel literals), `chimera-core/tests/note_queue_test.rs` (append)

**Interfaces:**
- Consumes: `MidiChannel::clamped`, `NoteEvent`, `NoteKind`.
- Produces:
  - `MidiMessage::{NoteOn { channel: MidiChannel, note, velocity }, NoteOff { channel: MidiChannel, note, velocity: u8 }, ControlChange { channel: MidiChannel, cc, value }, PitchBend { channel: MidiChannel, value }}`.
  - `pub const fn MidiParser::new() -> MidiParser` (so a `static` can hold one).
  - `pub fn NoteEvent::from_midi(msg: MidiMessage) -> Option<NoteEvent>` — `NoteOn`/`NoteOff` convert, everything else is `None` (sub-project 2 consumes it).

- [ ] **Step 1: Write the failing tests**

Create `chimera-hal/tests/midi_parser_test.rs`:

```rust
use chimera_hal::midi::MidiParser;
use chimera_hal::{MidiChannel, MidiMessage, MidiNote, Velocity};

fn ch(c: u8) -> MidiChannel {
    MidiChannel::new(c).unwrap()
}

fn on(c: u8, n: u8, v: u8) -> MidiMessage {
    MidiMessage::NoteOn { channel: ch(c), note: MidiNote::new(n).unwrap(), velocity: Velocity::new(v).unwrap() }
}

fn off(c: u8, n: u8, v: u8) -> MidiMessage {
    MidiMessage::NoteOff { channel: ch(c), note: MidiNote::new(n).unwrap(), velocity: v }
}

fn cc(c: u8, number: u8, value: u8) -> MidiMessage {
    MidiMessage::ControlChange { channel: ch(c), cc: number, value }
}

fn bend(c: u8, value: i16) -> MidiMessage {
    MidiMessage::PitchBend { channel: ch(c), value }
}

fn cases() -> Vec<(&'static str, Vec<u8>, Vec<MidiMessage>)> {
    vec![
        ("running_status", vec![0x90, 60, 100, 62, 100], vec![on(0, 60, 100), on(0, 62, 100)]),
        ("realtime_mid_message", vec![0x90, 60, 0xF8, 100, 0xFE, 0x92, 0xFA, 61, 0xFC, 1], vec![on(0, 60, 100), on(2, 61, 1)]),
        ("sysex_with_note_like_bytes", vec![0xF0, 0x7E, 60, 100, 0xF7, 60, 100, 0x90, 60, 100], vec![on(0, 60, 100)]),
        ("system_common_then_stray_data", vec![0xF3, 5, 60, 100, 0xF2, 0x10, 0x20, 60, 100], vec![]),
        ("velocity_zero_is_note_off", vec![0x93, 64, 0], vec![off(3, 64, 0)]),
        ("program_change_takes_one_byte", vec![0xC0, 5, 6, 7, 0x90, 60, 100], vec![on(0, 60, 100)]),
        ("channel_pressure_takes_one_byte", vec![0xD2, 0x40, 0x41, 0x92, 60, 1], vec![on(2, 60, 1)]),
        ("pitch_bend", vec![0xE5, 0x00, 0x40, 0xE0, 0x7F, 0x7F, 0xE0, 0, 0], vec![bend(5, 0), bend(0, 8191), bend(0, -8192)]),
        ("control_change_on_channel_16", vec![0xBF, 7, 127], vec![cc(15, 7, 127)]),
        ("junk_before_first_status", vec![60, 100, 0x3C, 0x90, 60, 100], vec![on(0, 60, 100)]),
        ("truncated_message_resyncs_on_next_status", vec![0x90, 60, 0x80, 64, 0], vec![off(0, 64, 0)]),
        ("hot_plug_junk_then_note", vec![0x3C, 0xF7, 0x40, 0x92, 0x3C, 0x40], vec![on(2, 60, 64)]),
    ]
}

#[test]
fn parser_behaviour_table() {
    for (name, bytes, want) in cases() {
        let mut p = MidiParser::new();
        let got: Vec<MidiMessage> = bytes.iter().filter_map(|&b| p.feed(b)).collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn a_parser_can_live_in_a_static() {
    static PARSER: MidiParser = MidiParser::new();
    let _ = &PARSER;
}
```

In `chimera-core/tests/midi_types_test.rs` add `MidiChannel` to the `use chimera_core::{…}` line and replace each `channel: 0,` / `channel: 1,` / `channel: 2,` literal with `channel: MidiChannel::new(0).unwrap(),` / `MidiChannel::new(1).unwrap(),` / `MidiChannel::new(2).unwrap(),`:

```bash
sed -i -E 's/channel: ([0-9]+),/channel: MidiChannel::new(\1).unwrap(),/' chimera-core/tests/midi_types_test.rs
sed -i 's/^use chimera_core::{MidiNote, Velocity};/use chimera_core::{MidiChannel, MidiNote, Velocity};/' chimera-core/tests/midi_types_test.rs
```

Append to `chimera-core/tests/note_queue_test.rs` (add `use chimera_hal::MidiMessage;`):

```rust
#[test]
fn only_note_on_and_off_become_note_events() {
    let c = MidiChannel::new(4).unwrap();
    let n = MidiNote::new(60).unwrap();
    let on = MidiMessage::NoteOn { channel: c, note: n, velocity: Velocity::MAX };
    let off = MidiMessage::NoteOff { channel: c, note: n, velocity: 64 };
    assert_eq!(NoteEvent::from_midi(on), Some(ev(4, 60, 127)));
    assert_eq!(NoteEvent::from_midi(off), Some(ev(4, 60, 0)));
    assert_eq!(NoteEvent::from_midi(MidiMessage::ControlChange { channel: c, cc: 1, value: 2 }), None);
    assert_eq!(NoteEvent::from_midi(MidiMessage::PitchBend { channel: c, value: 0 }), None);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-hal --test midi_parser_test`
Expected: FAIL to compile: `expected u8, found MidiChannel` in `MidiMessage` construction, and `MidiParser::new` is not `const`.

- [ ] **Step 3: Implement**

In `chimera-hal/src/lib.rs` change the four `channel: u8,` fields of `MidiMessage` to `channel: MidiChannel,`.

In `chimera-hal/src/midi.rs`:
- change `use crate::{MidiMessage, MidiNote, Velocity};` to `use crate::{MidiChannel, MidiMessage, MidiNote, Velocity};`
- change `pub fn new() -> Self {` to `pub const fn new() -> Self {`
- in `make_message` change `let channel = self.running_status & 0x0F;` to `let channel = MidiChannel::clamped(self.running_status & 0x0F);`

In `chimera-core/src/note_queue.rs` add `use chimera_hal::MidiMessage;` and, inside `impl NoteEvent`, before `fn pack`:

```rust
    pub fn from_midi(msg: MidiMessage) -> Option<Self> {
        match msg {
            MidiMessage::NoteOn { channel, note, velocity } => Some(Self { channel, note, kind: NoteKind::On(velocity) }),
            MidiMessage::NoteOff { channel, note, .. } => Some(Self { channel, note, kind: NoteKind::Off }),
            MidiMessage::ControlChange { .. } | MidiMessage::PitchBend { .. } => None,
        }
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p chimera-hal && cargo test -p chimera-core --test midi_types_test --test note_queue_test`
Expected: PASS.

- [ ] **Step 5: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add chimera-hal/src/lib.rs chimera-hal/src/midi.rs chimera-hal/tests/midi_parser_test.rs \
  chimera-core/src/note_queue.rs chimera-core/tests/midi_types_test.rs chimera-core/tests/note_queue_test.rs
git commit -m "MidiChannel in parsed messages, parser behaviour table"
```

---

### Task 8: In-place construction for `Instrument`, `FxBus`, `Voice` and the effects

**Files:**
- Create: `chimera-core/src/in_place.rs`
- Modify: `chimera-core/src/lib.rs` (add `mod in_place;`)
- Modify: `chimera-core/src/instrument.rs`, `dsp/voice.rs`, `dsp/engines.rs`, `dsp/modal.rs`, `dsp/fx_bus.rs`, `dsp/chorus.rs`, `dsp/delay.rs`, `dsp/reverb.rs`
- Test: `chimera-core/tests/in_place_test.rs`

**Interfaces:**
- Consumes: `SampleBudget` (Task 1).
- Produces:
  - `pub fn Instrument::init_in_place(slot: &mut MaybeUninit<Instrument>, sample_rate: u32, budget: SampleBudget) -> &mut Instrument`
  - `pub fn FxBus::init_in_place(slot: &mut MaybeUninit<FxBus>) -> &mut FxBus` (no sample rate: `FxBus::new()` takes none)
  - `pub fn Voice::init_in_place(slot: &mut MaybeUninit<Voice>, sample_rate: u32) -> &mut Voice`
  - `pub fn Engines::init_in_place(slot, sample_rate)`, `pub fn ModalEngine::init_in_place(slot)`, `pub fn JunoChorus::init_in_place(slot)`, `pub fn TapeDelay::init_in_place(slot)`, `pub fn Reverb::init_in_place(slot)`
  - crate-private `in_place::{uninit_at, by_value, field_list!}`; `Instrument::new`, `Voice::new`, `Engines::new`, `ModalEngine::new` build through their in-place constructors (one list of initial values each).

Strategy (spec § Memory): fields over 4 KB are built in place through `addr_of_mut!`; `f32` arrays are zero-filled with `write_bytes`; small fields are written by value. The effects' power-on state is all zeros and every field of theirs is `f32`/`usize` (or arrays of them), so each effect is one `write_bytes`. `field_list!` makes adding a field a compile error next to the constructor.

- [ ] **Step 1: Write the failing equality tests**

Create `chimera-core/tests/in_place_test.rs`:

```rust
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiChannel, MidiNote, Velocity};

const SR: u32 = chimera_hal::SAMPLE_RATE;
const BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);

fn event(ch: u8, note: u8, kind: NoteKind) -> NoteEvent {
    NoteEvent { channel: MidiChannel::new(ch).unwrap(), note: MidiNote::new(note).unwrap(), kind }
}

fn every_engine_and_every_effect() -> AudioShared {
    let mut s = AudioShared::default();
    s.parts[0].params = ParamSnapshot::for_engine(EngineType::Pizza);
    s.parts[1].params = ParamSnapshot::for_engine(EngineType::Fm);
    s.parts[2].params = ParamSnapshot::for_engine(EngineType::Modal);
    s.parts[2].params.modal.mode = ResonatorMode::String;
    s.parts[3].params = ParamSnapshot::for_engine(EngineType::Modal);
    s.parts[3].params.modal.mode = ResonatorMode::Sympathetic;
    for p in 0..4 {
        s.parts[p].mix.sends = [0.3; FX_SENDS];
    }
    s.fx.chorus.mode = 1;
    s.fx.chorus.mix = 0.5;
    s.fx.delay.mix = 0.5;
    s.fx.reverb.mix = 0.5;
    s
}

fn play(inst: &mut Instrument, fx: &mut FxBus) -> Vec<u32> {
    let shared = every_engine_and_every_effect();
    let notes = [(0, 60), (1, 64), (2, 40), (3, 45)];
    let mut out: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
    let mut bits = Vec::new();
    for b in 0..200 {
        for &(ch, n) in &notes {
            if b == 0 {
                inst.handle(event(ch, n, NoteKind::On(Velocity::DEFAULT)), &shared);
            }
            if b == 100 {
                inst.handle(event(ch, n, NoteKind::Off), &shared);
            }
        }
        inst.render(fx, &mut out, &shared);
        bits.extend(out.iter().flatten().map(|s| s.to_bits()));
    }
    bits
}

#[test]
fn instrument_built_in_place_renders_like_new() {
    let mut by_value = Box::new(Instrument::new(SR, BUDGET));
    let mut fx_a = Box::new(FxBus::new());
    let mut slot = Box::<Instrument>::new_uninit();
    let mut fx_slot = Box::<FxBus>::new_uninit();
    let in_place = Instrument::init_in_place(&mut slot, SR, BUDGET);
    let fx_b = FxBus::init_in_place(&mut fx_slot);
    assert_eq!(play(&mut by_value, &mut fx_a), play(in_place, fx_b));
    assert_eq!(in_place.allocator().budget(), BUDGET);
}

#[test]
fn fx_bus_built_in_place_processes_like_new() {
    let mut a = Box::new(FxBus::new());
    let mut slot = Box::<FxBus>::new_uninit();
    let b = FxBus::init_in_place(&mut slot);
    let mut params = every_engine_and_every_effect().fx;
    let mut x = 0x1234_5678u32;
    for block in 0..300 {
        params.reverb.reverb_type = (block / 100) as u8;
        let mut sends = [[0.0f32; BLOCK_SIZE]; FX_SENDS];
        for s in sends.iter_mut().flatten() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *s = (x as f32 / u32::MAX as f32) - 0.5;
        }
        let mut sends_b = sends;
        let (mut ret_a, mut ret_b) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        a.process(&mut sends, &params, SR, &mut ret_a);
        b.process(&mut sends_b, &params, SR, &mut ret_b);
        assert_eq!(ret_a.map(f32::to_bits), ret_b.map(f32::to_bits), "block {block}");
    }
}

#[test]
fn voice_built_in_place_renders_like_new_for_every_engine() {
    let modes = [ResonatorMode::String, ResonatorMode::Modal, ResonatorMode::Bowed, ResonatorMode::Sympathetic];
    for engine in EngineType::ALL {
        for mode in modes {
            let mut params = ParamSnapshot::for_engine(engine);
            params.modal.mode = mode;
            let mut a = Box::new(Voice::new(SR));
            let mut slot = Box::<Voice>::new_uninit();
            let b = Voice::init_in_place(&mut slot, SR);
            let note = MidiNote::new(52).unwrap();
            a.note_on(note, Velocity::DEFAULT, &params);
            b.note_on(note, Velocity::DEFAULT, &params);
            let (mut xa, mut xb) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
            for block in 0..60 {
                a.render(&mut xa, &params, &ModState::new());
                b.render(&mut xb, &params, &ModState::new());
                assert_eq!(xa.map(f32::to_bits), xb.map(f32::to_bits), "{engine:?} {mode:?} block {block}");
            }
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test in_place_test`
Expected: FAIL to compile: `no function or associated item named init_in_place found for struct Instrument`.

- [ ] **Step 3: Add the in-place helpers**

Create `chimera-core/src/in_place.rs`:

```rust
use core::mem::MaybeUninit;

/// # Safety
/// `p` must be non-null, aligned, valid for writes for `'a` and not aliased.
pub(crate) unsafe fn uninit_at<'a, T>(p: *mut T) -> &'a mut MaybeUninit<T> {
    // SAFETY: `MaybeUninit<T>` has `T`'s layout; the caller guarantees the
    // pointer is valid, aligned and unaliased for `'a`.
    unsafe { &mut *p.cast::<MaybeUninit<T>>() }
}

pub(crate) fn by_value<T>(init: impl FnOnce(&mut MaybeUninit<T>) -> &mut T) -> T {
    let mut slot = MaybeUninit::uninit();
    init(&mut slot);
    // SAFETY: `init` is always one of this crate's in-place constructors,
    // which return `&mut T` only after writing every field of `slot`.
    unsafe { slot.assume_init() }
}

// Adding a field fails to compile here until the in-place constructor
// next to it is updated too.
macro_rules! field_list {
    ($ty:ty => $name:ident { $($field:ident),* $(,)? }) => {
        const _: fn(&$ty) = |v| {
            let $name { $($field: _),* } = v;
        };
    };
}
pub(crate) use field_list;
```

Add `mod in_place;` to `chimera-core/src/lib.rs` (after `pub mod dsp;`).

- [ ] **Step 4: Zero-fill the effects**

In `chimera-core/src/dsp/chorus.rs` add `use core::mem::MaybeUninit;` and inside `impl JunoChorus`, after `new`:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field (two `BbdLine { buffer: [f32; N], write_pos:
        // usize, lfo_phase: f32 }`) is valid as zero bytes, and zero is
        // exactly `new()`'s state; `write_bytes` covers the whole slot.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }
```

and at module level after the `BbdLine` impl:

```rust
crate::in_place::field_list!(JunoChorus => JunoChorus { line_i, line_ii });
crate::in_place::field_list!(BbdLine => BbdLine { buffer, write_pos, lfo_phase });
```

In `chimera-core/src/dsp/delay.rs` add `use core::mem::MaybeUninit;` and inside `impl TapeDelay`, after `new`:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field (`[f32; N]`, `usize`, three `f32`) is valid as
        // zero bytes, and zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }
```

and `crate::in_place::field_list!(TapeDelay => TapeDelay { buffer, write_pos, lp_state, wow_phase, flutter_phase });`.

In `chimera-core/src/dsp/reverb.rs` add `use core::mem::MaybeUninit;` and inside `impl Reverb`, after `new`:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the plate, FDN and MidiVerb reverbs hold only `DelayLine`s
        // (`[f32; N]` + `usize`), `OnePole`s (`f32`) and two `f32`s, all valid
        // as zero bytes; zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }
```

and at module level:

```rust
crate::in_place::field_list!(Reverb => Reverb { plate, fdn, midiverb });
crate::in_place::field_list!(PlateReverb => PlateReverb { ap_in, ap_tank, del_tank, lp });
crate::in_place::field_list!(FdnReverb => FdnReverb { lines, lp });
crate::in_place::field_list!(MidiVerbReverb => MidiVerbReverb { ap_diff, ap_net_a, ap_net_b, recirc_a, recirc_b });
crate::in_place::field_list!(DelayLine<1> => DelayLine { buffer, write_pos });
crate::in_place::field_list!(OnePole => OnePole { state });
```

In `chimera-core/src/dsp/fx_bus.rs` add `use core::mem::MaybeUninit;` and `use core::ptr::addr_of_mut;` and `use crate::in_place::uninit_at;`, and inside `impl FxBus`, after `new`:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from `&mut MaybeUninit<Self>` (valid, aligned,
        // unaliased); each effect's in-place constructor initialises its
        // whole field before `assume_init_mut`.
        unsafe {
            JunoChorus::init_in_place(uninit_at(addr_of_mut!((*p).chorus)));
            TapeDelay::init_in_place(uninit_at(addr_of_mut!((*p).delay)));
            Reverb::init_in_place(uninit_at(addr_of_mut!((*p).reverb)));
            slot.assume_init_mut()
        }
    }
```

and `crate::in_place::field_list!(FxBus => FxBus { chorus, delay, reverb });`.

- [ ] **Step 5: Build the voice tree in place**

In `chimera-core/src/dsp/modal.rs` add `use core::mem::MaybeUninit;`, `use core::ptr::addr_of_mut;`, `use crate::in_place::{by_value, uninit_at};`. Delete `KsString::new` and replace it inside `impl KsString` with:

```rust
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the 4.8 KB `[f32]` buffer is
        // zero-filled (zero bytes are 0.0) and every other field is written
        // once by value before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).buffer).write_bytes(0, 1);
            addr_of_mut!((*p).write_pos).write(0);
            addr_of_mut!((*p).delay_len).write(100);
            addr_of_mut!((*p).ens_lfo_phase).write(0);
            addr_of_mut!((*p).noise_state).write(0x8765_4321);
            slot.assume_init_mut()
        }
    }
```

Replace `ModalEngine::new`'s body with `by_value(Self::init_in_place)` and add after it:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the eight strings are built in
        // place, every other field (the largest, `filters`, is 960 B) is
        // written once by value, before `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).filters).write(core::array::from_fn(|_| Svf::new()));
            addr_of_mut!((*p).cos_osc).write(CosineOsc::new());
            addr_of_mut!((*p).resolution).write(0);
            KsString::init_in_place(uninit_at(addr_of_mut!((*p).string)));
            let sym = addr_of_mut!((*p).sym_strings).cast::<KsString>();
            for i in 0..NUM_SYMPATHETIC {
                KsString::init_in_place(uninit_at(sym.add(i)));
            }
            addr_of_mut!((*p).bow_state).write(0.0);
            addr_of_mut!((*p).frequency).write(220.0 / 48000.0);
            addr_of_mut!((*p).active_mode).write(ResonatorMode::Modal);
            addr_of_mut!((*p).released).write(false);
            addr_of_mut!((*p).exciter_remaining).write(0);
            addr_of_mut!((*p).exciter_amp).write(0.0);
            addr_of_mut!((*p).noise_state).write(0x1234_5678);
            addr_of_mut!((*p).exciter_lp).write(0.0);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).silence_counter).write(0);
            slot.assume_init_mut()
        }
    }
```

and at module level:

```rust
crate::in_place::field_list!(KsString => KsString { buffer, write_pos, delay_len, ens_lfo_phase, noise_state });
crate::in_place::field_list!(ModalEngine => ModalEngine {
    filters, cos_osc, resolution, string, sym_strings, bow_state, frequency, active_mode,
    released, exciter_remaining, exciter_amp, noise_state, exciter_lp, active, silence_counter,
});
```

In `chimera-core/src/dsp/engines.rs` add the same three `use` lines (`MaybeUninit`, `addr_of_mut`, `by_value, uninit_at`), replace `Engines::new`'s body with `by_value(|slot| Self::init_in_place(slot, sample_rate))` and add:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; Modal (40 KB) is built in place,
        // Pizza and FM (about 1.1 KB) by value, each field once.
        unsafe {
            addr_of_mut!((*p).pizza).write(PizzaOsc::new());
            addr_of_mut!((*p).fm).write(FmEngine::new());
            ModalEngine::init_in_place(uninit_at(addr_of_mut!((*p).modal)));
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            slot.assume_init_mut()
        }
    }
```

and `crate::in_place::field_list!(Engines => Engines { pizza, fm, modal, sample_rate });`.

In `chimera-core/src/dsp/voice.rs` add the same `use` lines, replace `Voice::new`'s body with `by_value(|slot| Self::init_in_place(slot, sample_rate))` and add:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the engines are built in place
        // and every other (small) field is written once before
        // `assume_init_mut`.
        unsafe {
            Engines::init_in_place(uninit_at(addr_of_mut!((*p).engines)), sample_rate);
            addr_of_mut!((*p).drive).write(Drive::new());
            addr_of_mut!((*p).filter).write(SvfFilter::new());
            addr_of_mut!((*p).folder).write(Wavefolder::new());
            addr_of_mut!((*p).amp_env).write(Envelope::new());
            addr_of_mut!((*p).lfo).write(Lfo::new());
            addr_of_mut!((*p).active_engine).write(EngineType::Pizza);
            addr_of_mut!((*p).active).write(false);
            addr_of_mut!((*p).last_note).write(MidiNote::A4);
            addr_of_mut!((*p).last_velocity).write(Velocity::DEFAULT);
            slot.assume_init_mut()
        }
    }
```

and `crate::in_place::field_list!(Voice => Voice { engines, drive, filter, folder, amp_env, lfo, active_engine, active, last_note, last_velocity });`.

In `chimera-core/src/instrument.rs` add `use core::mem::MaybeUninit;`, `use core::ptr::addr_of_mut;`, `use crate::in_place::{by_value, uninit_at};`, replace `Instrument::new`'s body with `by_value(|slot| Self::init_in_place(slot, sample_rate, budget))` and add:

```rust
    pub fn init_in_place(slot: &mut MaybeUninit<Self>, sample_rate: u32, budget: SampleBudget) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the six voices are built in
        // place and the rest (the largest, `buses`, is 1.5 KB) written once
        // by value before `assume_init_mut`.
        unsafe {
            let voices = addr_of_mut!((*p).voices).cast::<Voice>();
            for v in 0..MAX_VOICES {
                Voice::init_in_place(uninit_at(voices.add(v)), sample_rate);
            }
            addr_of_mut!((*p).alloc).write(Allocator::new(budget));
            addr_of_mut!((*p).note_channel).write([MidiChannel::clamped(0); MAX_VOICES]);
            addr_of_mut!((*p).buses).write([[0.0; BLOCK_SIZE]; MAX_PARTS]);
            addr_of_mut!((*p).sends).write([[0.0; BLOCK_SIZE]; FX_SENDS]);
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            slot.assume_init_mut()
        }
    }
```

and `crate::in_place::field_list!(Instrument => Instrument { voices, alloc, note_channel, buses, sends, sample_rate });`.

- [ ] **Step 6: Run to verify it passes, audio bit-identical**

Run: `cargo test -p chimera-core --test in_place_test --test golden_test --test fx_golden_test --test signal_chain_test --test modal_test --test memory_budget_test`
Expected: PASS; no golden changes.

- [ ] **Step 7: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add chimera-core/src/in_place.rs chimera-core/src/lib.rs chimera-core/src/instrument.rs \
  chimera-core/src/dsp/voice.rs chimera-core/src/dsp/engines.rs chimera-core/src/dsp/modal.rs \
  chimera-core/src/dsp/fx_bus.rs chimera-core/src/dsp/chorus.rs chimera-core/src/dsp/delay.rs \
  chimera-core/src/dsp/reverb.rs chimera-core/tests/in_place_test.rs
git commit -m "In-place construction for Instrument, FxBus, Voice and the effects"
```

---

### Task 9: Scope through a `TripleBuffer`; AXI accounting for the triple buffers

**Files:**
- Modify: `chimera-core/src/scope.rs` (statics → `ScopeWriter` + `scope_buffer`)
- Modify: `chimera-core/src/instrument.rs` (`render` takes `&mut ScopeWriter`; `AXI_RESIDENT`)
- Modify: `chimera-core/src/ui/mod.rs` (delete `render`, `render_dirty`; `prime_regions` takes the scope)
- Modify: `chimera-core/tests/common/mod.rs`, `instrument_test.rs`, `in_place_test.rs`, `memory_budget_test.rs`
- Create: `chimera-core/tests/scope_test.rs`
- Create: `chimera-stm32/src/shared.rs`; Modify: `chimera-stm32/src/main.rs`, `chimera-stm32/src/audio.rs`
- Modify: `chimera-desktop/src/main.rs`, `chimera-desktop/src/audio.rs`

**Interfaces:**
- Consumes: `TripleBuffer`, `Writer`, `Reader` (Task 2); `AudioStats` (Task 6, for the AXI sum).
- Produces:
  - `pub type ScopeFrame = [f32; SCOPE_LEN];`
  - `pub const fn scope_buffer() -> TripleBuffer<ScopeFrame>`
  - `pub struct ScopeWriter`; `pub fn new(out: Writer<ScopeFrame>) -> ScopeWriter`; `pub fn write(&mut self, samples: &[f32])` (same trigger search as before; publishes a frame when the 480-sample back buffer fills).
  - `Instrument::render(&mut self, fx: &mut FxBus, out: &mut DacOut, shared: &AudioShared, scope: &mut ScopeWriter)`.
  - `UiState::prime_regions(&mut self, perf: &PerfStats, scope: &[f32; SCOPE_LEN])`; `UiState::render` and `UiState::render_dirty` removed (shells call `render_with_scope` / `render_dirty_with_scope`).
  - Firmware `shared::take_scope() -> Option<(Writer<ScopeFrame>, Reader<ScopeFrame>)>`; `audio::init_scope(w: Writer<ScopeFrame>)`.
  - Desktop `DesktopAudio::new(scope: Writer<ScopeFrame>) -> DesktopAudio`.

- [ ] **Step 1: Write the failing scope tests**

Create `chimera-core/tests/scope_test.rs`:

```rust
use chimera_core::scope::{SCOPE_LEN, ScopeWriter, scope_buffer};

fn writer_and_reader() -> (ScopeWriter, chimera_core::triple::Reader<[f32; SCOPE_LEN]>) {
    let (w, r) = Box::leak(Box::new(scope_buffer())).split();
    (ScopeWriter::new(w), r)
}

#[test]
fn a_full_back_buffer_publishes_from_the_first_rising_zero_crossing() {
    let (mut sw, mut r) = writer_and_reader();
    let samples: Vec<f32> = (0..2 * SCOPE_LEN)
        .map(|i| if i < 100 { -0.5 } else { (i - 99) as f32 * 0.001 })
        .collect();
    for block in samples.chunks(64) {
        sw.write(block);
    }
    assert_eq!(&r.read()[..], &samples[100..100 + SCOPE_LEN]);
}

#[test]
fn nothing_is_published_until_the_back_buffer_fills() {
    let (mut sw, mut r) = writer_and_reader();
    sw.write(&[0.25; 400]);
    assert_eq!(*r.read(), [0.0; SCOPE_LEN]);
}

#[test]
fn without_a_zero_crossing_the_window_starts_at_the_beginning() {
    let (mut sw, mut r) = writer_and_reader();
    let samples: Vec<f32> = (0..2 * SCOPE_LEN).map(|i| 0.1 + i as f32 * 1e-4).collect();
    sw.write(&samples);
    assert_eq!(&r.read()[..], &samples[..SCOPE_LEN]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test scope_test`
Expected: FAIL to compile: `no ScopeWriter in scope`.

- [ ] **Step 3: Rewrite `scope.rs`**

Replace the statics, `write_samples` and `read_samples` in `chimera-core/src/scope.rs` (keep `peak` and `SOUNDING_PEAK` unchanged) with:

```rust
use crate::triple::{TripleBuffer, Writer};

pub const SCOPE_LEN: usize = 240;
// Twice the window, so a trigger point can be searched for.
const BACK_LEN: usize = SCOPE_LEN * 2;

pub type ScopeFrame = [f32; SCOPE_LEN];

pub const fn scope_buffer() -> TripleBuffer<ScopeFrame> {
    TripleBuffer::new([0.0; SCOPE_LEN], [0.0; SCOPE_LEN], [0.0; SCOPE_LEN])
}

pub struct ScopeWriter {
    back: [f32; BACK_LEN],
    pos: usize,
    out: Writer<ScopeFrame>,
}

impl ScopeWriter {
    pub fn new(out: Writer<ScopeFrame>) -> Self {
        Self { back: [0.0; BACK_LEN], pos: 0, out }
    }

    pub fn write(&mut self, samples: &[f32]) {
        for &s in samples {
            if self.pos < BACK_LEN {
                self.back[self.pos] = s;
                self.pos += 1;
            }
        }
        if self.pos >= BACK_LEN {
            let trigger = (1..BACK_LEN - SCOPE_LEN)
                .find(|&i| self.back[i - 1] <= 0.0 && self.back[i] > 0.0)
                .unwrap_or(0);
            let back = &self.back;
            self.out.publish(|f| f.copy_from_slice(&back[trigger..trigger + SCOPE_LEN]));
            self.pos = 0;
        }
    }
}
```

Remove the now-unused `use core::sync::atomic::…` line from `scope.rs`.

- [ ] **Step 4: `Instrument::render` takes the scope writer; count the triple buffers in AXI**

In `chimera-core/src/instrument.rs`:
- add `use crate::perf::load::AudioStats;`, `use crate::scope::{ScopeFrame, ScopeWriter};`, `use crate::triple::TripleBuffer;`
- replace `AXI_RESIDENT` with:

```rust
pub const AXI_RESIDENT: usize = FB_BYTES
    + UI_RESERVE
    + size_of::<Performance>()
    + size_of::<SoundPool>()
    + size_of::<TripleBuffer<AudioShared>>()
    + size_of::<TripleBuffer<ScopeFrame>>()
    + size_of::<ScopeWriter>()
    + size_of::<TripleBuffer<AudioStats>>()
    + size_of::<FxBus>();
```

- change the `render` signature to `pub fn render(&mut self, fx: &mut FxBus, out: &mut DacOut, shared: &AudioShared, scope: &mut ScopeWriter) {` and its last line `crate::scope::write_samples(&scope);` to `scope.write(&scope_block);`, renaming the local `let mut scope = [0.0f32; BLOCK_SIZE];` to `let mut scope_block = [0.0f32; BLOCK_SIZE];` and `scope[i] += bus[i];` to `scope_block[i] += bus[i];`.

In `chimera-core/tests/memory_budget_test.rs` replace the `parts` array in `axi_residents_fit` with:

```rust
    use chimera_core::perf::load::AudioStats;
    use chimera_core::scope::{ScopeFrame, ScopeWriter};
    use chimera_core::triple::TripleBuffer;
    let parts = [
        ("framebuffer", hw::FB_BYTES),
        ("UI reserve", hw::UI_RESERVE),
        ("Performance", size_of::<Performance>()),
        ("SoundPool", size_of::<SoundPool>()),
        ("AudioShared x3", size_of::<TripleBuffer<AudioShared>>()),
        ("scope x3", size_of::<TripleBuffer<ScopeFrame>>()),
        ("scope writer", size_of::<ScopeWriter>()),
        ("AudioStats x3", size_of::<TripleBuffer<AudioStats>>()),
        ("FxBus", size_of::<FxBus>()),
    ];
```

In `chimera-core/tests/common/mod.rs` add:

```rust
use chimera_core::scope::{ScopeWriter, scope_buffer};

pub fn scope_writer() -> ScopeWriter {
    let (w, _unread) = Box::leak(Box::new(scope_buffer())).split();
    ScopeWriter::new(w)
}
```

and in `render_case_through_instrument` add `let mut scope = scope_writer();` before the loop and change `inst.render(&mut fx, &mut dac, s);` to `inst.render(&mut fx, &mut dac, s, &mut scope);`.

In `chimera-core/tests/instrument_test.rs` add a `scope: chimera_core::scope::ScopeWriter,` field to `Rig`, initialise it with `scope: common::scope_writer(),` in `Rig::new`, and change `self.inst.render(&mut self.fx, &mut self.out, shared);` to `self.inst.render(&mut self.fx, &mut self.out, shared, &mut self.scope);`.

In `chimera-core/tests/in_place_test.rs` add `mod common;` and in `play` add `let mut scope = common::scope_writer();` and change `inst.render(fx, &mut out, &shared);` to `inst.render(fx, &mut out, &shared, &mut scope);`.

- [ ] **Step 5: The UI stops reading the global scope**

In `chimera-core/src/ui/mod.rs`:
- delete `pub fn render<D>(&self, display: &mut D, perf: &PerfStats)` and `pub fn render_dirty<D>(&mut self, display: &mut D, perf: &PerfStats) -> …` entirely;
- change `prime_regions` to:

```rust
    pub fn prime_regions(&mut self, perf: &PerfStats, scope: &[f32; SCOPE_LEN]) {
        self.region_set.set_layout(self.nav.active_block_def().layout);
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, scope);
            for (d, r) in data.iter_mut().zip(self.region_set.active_regions()) {
                *d = self.region_data(r.kind, &f);
            }
        }
        for (r, d) in self.region_set.active_regions_mut().iter_mut().zip(data) {
            r.prev_data = d;
        }
    }
```

- [ ] **Step 6: Run the core tests**

Run: `cargo test -p chimera-core`
Expected: PASS, including `scope_test`, `memory_budget_test` (prints the new AXI total, under 524,288), and every audio and screen golden unchanged.

- [ ] **Step 7: The desktop owns a scope buffer**

In `chimera-desktop/src/audio.rs`:
- add `use chimera_core::scope::{ScopeFrame, ScopeWriter};` and `use chimera_core::triple::Writer;`
- change `pub fn new() -> Self {` to `pub fn new(scope: Writer<ScopeFrame>) -> Self {`
- after `let mut block_pos = BLOCK_SIZE;` add `let mut scope = ScopeWriter::new(scope);`
- change `inst.render(&mut fx, &mut dac, shared);` to `inst.render(&mut fx, &mut dac, shared, &mut scope);`

In `chimera-desktop/src/main.rs`:
- add `use chimera_core::scope::scope_buffer;`
- replace `let mut audio = audio::DesktopAudio::new();` with:

```rust
    let (scope_w, mut scope_r) = Box::leak(Box::new(scope_buffer())).split();
    let mut audio = audio::DesktopAudio::new(scope_w);
```

- replace `ui.render(&mut display, &perf.stats);` with `ui.render_with_scope(&mut display, &perf.stats, scope_r.read());`

- [ ] **Step 8: The firmware owns a scope buffer**

Create `chimera-stm32/src/shared.rs`:

```rust
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::scope::{ScopeFrame, scope_buffer};
use chimera_core::triple::{Reader, TripleBuffer, Writer};

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();
static SCOPE_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_scope() -> Option<(Writer<ScopeFrame>, Reader<ScopeFrame>)> {
    if SCOPE_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `SCOPE` ever made.
    Some(unsafe { &mut *addr_of_mut!(SCOPE) }.split())
}
```

In `chimera-stm32/src/audio.rs`:
- add `use chimera_core::scope::{ScopeFrame, ScopeWriter};` and `use chimera_core::triple::Writer;`
- add after `static mut MOD_STATE_PTR …`:

```rust
static mut SCOPE: MaybeUninit<ScopeWriter> = MaybeUninit::uninit();

pub fn init_scope(w: Writer<ScopeFrame>) {
    // SAFETY: called once from `main` before `prefill_buffer` and before the
    // DMA interrupt is unmasked; nothing else touches `SCOPE` yet.
    unsafe { (*addr_of_mut!(SCOPE)).write(ScopeWriter::new(w)) };
}
```

- in `render_block` replace `chimera_core::scope::write_samples(work);` with `(*addr_of_mut!(SCOPE)).assume_init_mut().write(work);` (it is already inside the function's `unsafe` block, whose SAFETY comment covers the ISR-only statics; add `SCOPE` to the list it names).

In `chimera-stm32/src/main.rs`:
- add `mod shared;`
- before `audio::init_pll3();` add:

```rust
    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    audio::init_scope(scope_w);
```

- replace `ui.render(&mut display, &perf.stats);` with `ui.render_with_scope(&mut display, &perf.stats, scope_r.read());`
- replace `ui.prime_regions(&perf.stats);` with `ui.prime_regions(&perf.stats, scope_r.read());`
- replace `let flush_list = ui.render_dirty(&mut display, &perf.stats);` with `let flush_list = ui.render_dirty_with_scope(&mut display, &perf.stats, scope_r.read());`

- [ ] **Step 9: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS (firmware builds and links).

- [ ] **Step 10: Commit**

```bash
git add chimera-core/src/scope.rs chimera-core/src/instrument.rs chimera-core/src/ui/mod.rs \
  chimera-core/tests/scope_test.rs chimera-core/tests/common/mod.rs chimera-core/tests/instrument_test.rs \
  chimera-core/tests/in_place_test.rs chimera-core/tests/memory_budget_test.rs \
  chimera-stm32/src/shared.rs chimera-stm32/src/main.rs chimera-stm32/src/audio.rs \
  chimera-desktop/src/main.rs chimera-desktop/src/audio.rs
git commit -m "Scope through a TripleBuffer"
```

---

### Task 10: Desktop `AudioShared` through a `TripleBuffer`

**Files:**
- Modify: `chimera-desktop/src/audio.rs` (`SharedState.current`, `bufs`, `active_buf` → `Writer`/`Reader`)
- Modify: `chimera-core/src/instrument.rs` (the `AudioShared`/`Instrument` docs that describe the old double buffer: delete the "back buffer" sentences)

**Interfaces:**
- Consumes: `TripleBuffer::new`, `split`, `Writer::publish`, `Reader::read` (Task 2); `AudioShared::update_from`.
- Produces: `DesktopAudio::update(&mut self, perf: &Performance)` publishes through the writer; the cpal callback reads once per callback and holds that buffer for the whole callback (the race in `audio.rs:66-76` is gone).

- [ ] **Step 1: Swap the buffers**

In `chimera-desktop/src/audio.rs`:
- add `use chimera_core::triple::{TripleBuffer, Writer};` and remove `AtomicPtr` from the atomics import;
- in `SharedState` delete the `current: AtomicPtr<AudioShared>,` field;
- in `DesktopAudio` replace `bufs: Box<[AudioShared; 2]>,` and `active_buf: usize,` with `shared_audio: Writer<AudioShared>,`;
- in `new`, replace the `bufs`/`initial_ptr` lines and the `current:` initialiser with:

```rust
        let (shared_audio, mut shared_reader) = Box::leak(Box::new(TripleBuffer::new(
            AudioShared::default(),
            AudioShared::default(),
            AudioShared::default(),
        )))
        .split();
```

- in the callback replace the `unsafe` pointer load (and its SAFETY comment) with `let shared = shared_reader.read();`
- in the returned `Self { … }` replace `bufs, active_buf: 0,` with `shared_audio,`;
- replace `update` with:

```rust
    pub fn update(&mut self, perf: &Performance) {
        self.shared_audio.publish(|b| b.update_from(perf));
    }
```

In `chimera-core/src/instrument.rs` the docs that describe the old double buffer become wrong; `Writer::publish` now enforces what they asked of callers:
- `AudioShared`'s doc: replace `/// The Performance state the audio needs, double-buffered by the platform` / `/// (one pointer swap per UI frame). The Sound names, pool and UI stay behind.` with `/// The Performance state the audio needs (ADR 0021: through a triple buffer).`
- `update_from`'s doc: delete its last two lines (`/// Callers must only ever run this on the back buffer — never on the` / `/// copy the audio thread is currently reading.`) and change `(the UI's per-frame refresh of the back` / `/// buffer)` to `(the UI's per-frame publish)`.
- `Instrument`'s contract: replace the second bullet (`/// - The `&AudioShared` passed to …` through `///   blocks.`) with `/// - The `&AudioShared` passed to `handle` and `render` is the reader's current buffer.`

- [ ] **Step 2: Run the desktop tests and build**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig cargo test -p chimera-desktop`
Expected: PASS (`pairs_sum_to_stereo`, `solo_hears_one_pair`).

- [ ] **Step 3: Check by ear**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just desktop`
Expected: Z..N play Part 1; turning an encoder on the FILTER page lerps the sound as before; MIX+B2 then changing Part 2's level/pan is heard.

- [ ] **Step 4: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-desktop/src/audio.rs chimera-core/src/instrument.rs
git commit -m "Desktop AudioShared through a TripleBuffer"
```

---

### Task 11: Desktop note sources — keyboard, and midir as the optional `midi` source

**Files:**
- Modify: `chimera-desktop/Cargo.toml` (`midi` feature, `midir` optional)
- Create: `chimera-desktop/src/midi.rs`
- Modify: `chimera-desktop/src/audio.rs` (`NoteQueue` → `NoteSources`, midir connection kept alive), `chimera-desktop/src/main.rs` (`mod midi`)

**Interfaces:**
- Consumes: `NoteSources`, `SourceId` (Task 3); `NoteEvent::from_midi`, `MidiParser` (Task 7).
- Produces:
  - Desktop `const N_SOURCES: usize` (2 with `midi`, 1 without), `const KEYS: SourceId<N_SOURCES> = SourceId::new(0)`, `#[cfg(feature = "midi")] const MIDI: SourceId<N_SOURCES> = SourceId::new(1)`.
  - `midi::pick_port(names: &[String], wanted: Option<&str>) -> Option<usize>`.
  - `DesktopAudio` keeps `Option<midir::MidiInputConnection<()>>` for the app's lifetime (feature `midi`); env var `CHIMERA_MIDI_PORT` picks a port by name.

- [ ] **Step 1: Make midir optional**

In `chimera-desktop/Cargo.toml` replace `midir = "0.10"` with `midir = { version = "0.10", optional = true }` and add:

```toml
[features]
default = ["midi"]
midi = ["dep:midir"]
```

- [ ] **Step 2: `pick_port` with its tests**

Create `chimera-desktop/src/midi.rs`:

```rust
pub fn pick_port(names: &[String], wanted: Option<&str>) -> Option<usize> {
    let lower = |s: &str| s.to_ascii_lowercase();
    match wanted {
        Some(w) => names.iter().position(|n| lower(n).contains(&lower(w))),
        None => names.iter().position(|n| !lower(n).contains("through")),
    }
}

#[cfg(test)]
mod tests {
    use super::pick_port;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_first_real_port_wins() {
        let n = names(&["Midi Through:Midi Through Port-0 14:0", "KeyStep 37:KeyStep 37 MIDI 1 24:0"]);
        assert_eq!(pick_port(&n, None), Some(1));
    }

    #[test]
    fn a_wanted_name_matches_case_insensitively() {
        let n = names(&["Arturia KeyStep", "nanoKEY2"]);
        assert_eq!(pick_port(&n, Some("NANOkey")), Some(1));
        assert_eq!(pick_port(&n, Some("launchpad")), None);
    }

    #[test]
    fn no_ports_or_only_loopback_picks_nothing() {
        assert_eq!(pick_port(&[], None), None);
        assert_eq!(pick_port(&names(&["Midi Through:Midi Through Port-0 14:0"]), None), None);
    }
}
```

In `chimera-desktop/src/main.rs` add after `mod display;`:

```rust
#[cfg(feature = "midi")]
mod midi;
```

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig cargo test -p chimera-desktop midi`
Expected: PASS for the three `pick_port` tests.

- [ ] **Step 3: Keys and MIDI as note sources**

In `chimera-desktop/src/audio.rs`:
- change `use chimera_core::note_queue::{NoteEvent, NoteKind, NoteQueue};` to `use chimera_core::note_queue::{NoteEvent, NoteKind, NoteSources, SourceId};`
- add after the imports:

```rust
#[cfg(feature = "midi")]
const N_SOURCES: usize = 2;
#[cfg(not(feature = "midi"))]
const N_SOURCES: usize = 1;

const KEYS: SourceId<N_SOURCES> = SourceId::new(0);
#[cfg(feature = "midi")]
const MIDI: SourceId<N_SOURCES> = SourceId::new(1);
```

- in `SharedState` change `notes: NoteQueue,` to `notes: NoteSources<N_SOURCES>,` and its initialiser to `notes: NoteSources::new(),`
- add a field to `DesktopAudio`:

```rust
    #[cfg(feature = "midi")]
    _midi: Option<midir::MidiInputConnection<()>>,
```

- in the callback replace `while let Some(ev) = audio.notes.pop() { inst.handle(ev, shared); }` with `audio.notes.drain(|ev| inst.handle(ev, shared));`
- in `note_on` and `note_off` replace `self.shared.notes.push(` with `self.shared.notes.source(KEYS).push(`
- in the returned `Self { … }` add `#[cfg(feature = "midi")] _midi: connect_midi(Arc::clone(&shared)),` (build `shared` before `Self`, as it already is);
- add at module level:

```rust
#[cfg(feature = "midi")]
fn connect_midi(shared: Arc<SharedState>) -> Option<midir::MidiInputConnection<()>> {
    use chimera_hal::midi::MidiParser;
    let input = midir::MidiInput::new("chimera")
        .map_err(|e| eprintln!("MIDI unavailable: {e}"))
        .ok()?;
    let ports = input.ports();
    let names: Vec<String> = ports.iter().map(|p| input.port_name(p).unwrap_or_default()).collect();
    let wanted = std::env::var("CHIMERA_MIDI_PORT").ok();
    let Some(i) = crate::midi::pick_port(&names, wanted.as_deref()) else {
        eprintln!("no MIDI input port; playing from the keyboard only");
        return None;
    };
    eprintln!("MIDI in: {}", names[i]);
    let mut parser = MidiParser::new();
    input
        .connect(
            &ports[i],
            "chimera-in",
            move |_stamp, bytes, _| {
                for &b in bytes {
                    if let Some(ev) = parser.feed(b).and_then(NoteEvent::from_midi) {
                        shared.notes.source(MIDI).push(ev);
                    }
                }
            },
            (),
        )
        .map_err(|e| eprintln!("MIDI connect failed: {e}"))
        .ok()
}
```

- [ ] **Step 4: Build both ways and run the desktop tests**

Run:

```bash
export PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig
cargo test -p chimera-desktop
cargo build -p chimera-desktop --no-default-features
cargo clippy -p chimera-desktop --no-default-features --all-targets -- -D warnings
```

Expected: tests PASS; the no-`midi` build and clippy are clean (only `Source::Keys` exists).

- [ ] **Step 5: Check with a USB MIDI keyboard**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just desktop` with a USB MIDI keyboard plugged in.
Expected: the terminal prints `MIDI in: <keyboard name>`; keys on MIDI channel 1 play Part 1, channel 2 Part 2 (MIX+B2 ▸ CH to confirm), chords sound all their notes, note-offs release; the computer keyboard still plays at the same time. Unplugged: `no MIDI input port; playing from the keyboard only`, and the simulator runs.

- [ ] **Step 6: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add chimera-desktop/Cargo.toml Cargo.lock chimera-desktop/src/midi.rs chimera-desktop/src/audio.rs chimera-desktop/src/main.rs
git commit -m "Desktop keyboard and midir as note sources"
```

---
### Task 12: Bring-up step 1 — clocks by revision, D2 clocks, caches/MPU, stack in DTCM, priorities, SysTick, delays, safe panic, boot splash

**Files:**
- Modify: `chimera-stm32/Cargo.toml` (HAL feature `revision_v`; drop `panic-halt`)
- Modify: `chimera-stm32/memory.x` (DTCM region, stack symbols)
- Modify: `chimera-stm32/build.rs` (`.ram_d2_dma` 4 KB section + asserts, then `.ram_d2`)
- Create: `chimera-stm32/src/clocks.rs`, `cache.rs`, `priority.rs`, `panic.rs`
- Modify: `chimera-stm32/src/main.rs`, `controls.rs`, `display.rs`, `audio.rs`

**Interfaces:**
- Consumes: `clock_plan::{SiliconRev, cycles_for_us, cycles_for_ns, systick_reload}` (Task 5); `ui::{draw, theme, fmt::FmtBuf}`.
- Produces (firmware):
  - `clocks::HSE_HZ: u32 = 8_000_000`; `#[derive(Clone, Copy, Debug)] pub struct Clocks { pub cpu_hz: u32, pub rev: SiliconRev }`; `clocks::read_rev(&pac::DBGMCU) -> SiliconRev`; `clocks::freeze(pac::PWR, pac::RCC, &pac::SYSCFG, SiliconRev) -> (Ccdr, Clocks)`; `clocks::delay_us(cpu_hz: u32, us: u32)`.
  - `cache::enable_d2_sram()`; `cache::init(&mut MPU, &mut SCB, &mut CPUID)`.
  - `priority::Priority` with `AUDIO` (0x00) and `SYSTICK` (0xF0) (`MIDI` arrives in Task 15), `Priority::bits(self) -> u8`; `priority::set_irq(&mut NVIC, pac::Interrupt, Priority)`; `priority::set_systick(&mut SCB, Priority)`.
  - `controls::CONTROLS_HZ: u32 = 500`; `controls::start_systick(cpu_hz: u32)`.
  - `Stm32Display::init(&mut self, cpu_hz: u32)`.
  - `audio::init_dma(nvic: &mut NVIC)`.
  - Linker symbols `__sram_d2_dma` (= `0x3000_0000`), `__eram_d2_dma` (= `0x3000_1000`), `_stack_start` (= `0x2002_0000`), `_stack_end` (= `0x2000_0000`).

Clock facts this task relies on (stm32h7xx-hal 0.16 source): `Pwr::vos0` exists only with the `revision_v` feature; the default PLL1 strategy caps the VCO at 420 MHz, so 480 MHz needs `PllConfigStrategy::Iterative` (VCO 960 MHz, P 2); from that VCO, PLL1 Q can't be 200 MHz (÷5 = 192 MHz, SPI then runs at 48 MHz instead of 50); with HCLK 240 MHz, pclk2 is 120 MHz (÷2) — the ÷2.4 needed for 100 MHz doesn't exist. Rev Y keeps today's exact configuration (400/200/100/100, PLL1 Q 200).

- [ ] **Step 1: Cargo features and linker layout**

In `chimera-stm32/Cargo.toml` change the HAL line to `stm32h7xx-hal = { version = "0.16", features = ["stm32h750", "revision_v", "rt"] }` and delete `panic-halt = "1.0"`.

Replace `chimera-stm32/memory.x` with:

```
MEMORY
{
    FLASH (rx) : ORIGIN = 0x08020000, LENGTH = 896K
    RAM  (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
    /* D2 SRAM1 (128K) + SRAM2 (128K) + SRAM3 (32K), contiguous (RM0433 §2.3) */
    RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 288K
    DTCM (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
}

/* ADR 0020: the stack lives in DTCM (zero-wait, CPU-only), not beside the framebuffer. */
_stack_start = ORIGIN(DTCM) + LENGTH(DTCM);
_stack_end = ORIGIN(DTCM);
```

In `chimera-stm32/build.rs` replace the `ram_d2.x` string with:

```rust
        r#"
/* The DMA rings alone in the first 4 KB of D2: the one MPU region that is
   non-cacheable (ADR 0020). The voice pool follows. */
SECTIONS {
    .ram_d2_dma (NOLOAD) : ALIGN(4096) {
        __sram_d2_dma = .;
        *(.ram_d2.dma .ram_d2.dma.*);
        . = __sram_d2_dma + 4096;
        __eram_d2_dma = .;
    } > RAM_D2
    .ram_d2 (NOLOAD) : ALIGN(32) {
        *(.ram_d2.voices .ram_d2.voices.*);
        *(.ram_d2 .ram_d2.*);
        . = ALIGN(4);
    } > RAM_D2
}
INSERT AFTER .uninit;
ASSERT(__sram_d2_dma == ORIGIN(RAM_D2), "the DMA rings must start D2, where the MPU region is");
ASSERT(__eram_d2_dma - __sram_d2_dma == 4096, "the DMA rings must fill exactly their 4 KB MPU region");
"#,
```

(If the rings ever outgrow 4 KB, `. = __sram_d2_dma + 4096` moves the location counter backwards and the link fails.)

- [ ] **Step 2: `clocks.rs`**

Create `chimera-stm32/src/clocks.rs`:

```rust
use chimera_core::clock_plan::{SiliconRev, cycles_for_us};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::prelude::*;
use stm32h7xx_hal::rcc::{Ccdr, PllConfigStrategy};

pub const HSE_HZ: u32 = 8_000_000;

#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    pub cpu_hz: u32,
    pub rev: SiliconRev,
}

pub fn read_rev(dbgmcu: &pac::DBGMCU) -> SiliconRev {
    SiliconRev::from_rev_id(dbgmcu.idc.read().rev_id().bits())
}

pub fn freeze(pwr: pac::PWR, rcc: pac::RCC, syscfg: &pac::SYSCFG, rev: SiliconRev) -> (Ccdr, Clocks) {
    let pwr = pwr.constrain();
    let pwrcfg = match rev {
        SiliconRev::V => pwr.vos0(syscfg).freeze(),
        SiliconRev::Y | SiliconRev::Unknown(_) => pwr.freeze(),
    };
    let cpu = rev.cpu_hz();
    let pclk = cpu / 4;
    let rcc = rcc
        .constrain()
        .use_hse(HSE_HZ.Hz())
        .sys_ck(cpu.Hz())
        .hclk((cpu / 2).Hz())
        .pclk1(pclk.Hz())
        .pclk2(pclk.Hz())
        .pclk3(pclk.Hz())
        .pclk4(pclk.Hz())
        .pll1_q_ck(200.MHz());
    let rcc = match rev {
        SiliconRev::V => rcc.pll1_strategy(PllConfigStrategy::Iterative),
        SiliconRev::Y | SiliconRev::Unknown(_) => rcc,
    };
    let ccdr = rcc.freeze(pwrcfg, syscfg);
    let cpu_hz = ccdr.clocks.c_ck().raw();
    (ccdr, Clocks { cpu_hz, rev })
}

pub fn delay_us(cpu_hz: u32, us: u32) {
    cortex_m::asm::delay(cycles_for_us(cpu_hz, us));
}
```

- [ ] **Step 3: `cache.rs`**

Create `chimera-stm32/src/cache.rs`:

```rust
use cortex_m::peripheral::{CPUID, MPU, SCB};
use stm32h7xx_hal::pac;

const DMA_REGION_BASE: u32 = 0x3000_0000;
const RBAR_VALID: u32 = 1 << 4;
// XN | AP = full access | TEX 001, C 0, B 0 (Normal, non-cacheable) | S 0 | SIZE 11 (4 KB) | ENABLE
const DMA_REGION_RASR: u32 = (1 << 28) | (0b011 << 24) | (0b001 << 19) | (11 << 1) | 1;
const _: () = assert!(DMA_REGION_RASR == 0x1308_0017);
// ENABLE | PRIVDEFENA: the default memory map everywhere else.
const MPU_CTRL: u32 = 0b101;

pub fn enable_d2_sram() {
    // SAFETY: read-modify-write of RCC_AHB2ENR's SRAM1/2/3EN before anything
    // touches D2 (ADR 0014); single-threaded, before the HAL owns RCC.
    let rcc = unsafe { &*pac::RCC::ptr() };
    rcc.ahb2enr.modify(|_, w| w.sram1en().enabled().sram2en().enabled().sram3en().enabled());
    let _ = rcc.ahb2enr.read();
    cortex_m::asm::dsb();
}

pub fn init(mpu: &mut MPU, scb: &mut SCB, cpuid: &mut CPUID) {
    cortex_m::asm::dmb();
    // SAFETY: the MPU is reprogrammed with both caches still off and no DMA
    // running; region 0 covers exactly the linker-asserted 4 KB DMA block.
    unsafe {
        mpu.ctrl.write(0);
        mpu.rnr.write(0);
        mpu.rbar.write(DMA_REGION_BASE | RBAR_VALID);
        mpu.rasr.write(DMA_REGION_RASR);
        mpu.ctrl.write(MPU_CTRL);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    scb.enable_icache();
    scb.enable_dcache(cpuid);
}
```

- [ ] **Step 4: `priority.rs`**

Create `chimera-stm32/src/priority.rs`:

```rust
use cortex_m::peripheral::scb::SystemHandler;
use cortex_m::peripheral::{NVIC, SCB};
use stm32h7xx_hal::pac;

// The H7 keeps the upper 4 bits of each priority byte: level n is n << 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority(u8);

impl Priority {
    pub const AUDIO: Priority = Priority::level(0);
    pub const SYSTICK: Priority = Priority::level(15);

    const fn level(level: u8) -> Self {
        assert!(level < 16);
        Priority(level << 4)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

const _: () = assert!(Priority::AUDIO.bits() == 0x00 && Priority::SYSTICK.bits() == 0xF0);

pub fn set_irq(nvic: &mut NVIC, irq: pac::Interrupt, p: Priority) {
    // SAFETY: priorities can break priority-based critical sections; this
    // firmware has none (it shares state through atomics and lock-free
    // buffers), and each interrupt is set before it is unmasked.
    unsafe { nvic.set_priority(irq, p.bits()) };
    debug_assert_eq!(NVIC::get_priority(irq), p.bits());
}

pub fn set_systick(scb: &mut SCB, p: Priority) {
    // SAFETY: as in `set_irq`.
    unsafe { scb.set_priority(SystemHandler::SysTick, p.bits()) };
    debug_assert_eq!(SCB::get_priority(SystemHandler::SysTick), p.bits());
}
```

- [ ] **Step 5: `panic.rs`**

Create `chimera-stm32/src/panic.rs`:

```rust
use core::panic::PanicInfo;

use cortex_m_rt::{ExceptionFrame, exception};
use stm32h7xx_hal::pac;

const GPIOE_BSRR: *mut u32 = 0x5802_1018 as *mut u32;
const LED_ON: u32 = 1 << 1;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    silence_and_halt()
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    silence_and_halt()
}

// Circular DMA keeps looping the last buffer after the CPU stops; with the
// SAI blocks disabled the DACs lose their clocks and go quiet instead.
fn silence_and_halt() -> ! {
    cortex_m::interrupt::disable();
    // SAFETY: interrupts are off and nothing runs after this; SAIEN is only
    // cleared on blocks whose APB2 clock is on, so no access faults, and
    // GPIOE_BSRR is PE's set/reset register (PE1 = LED).
    unsafe {
        let rcc = &*pac::RCC::ptr();
        let enabled = rcc.apb2enr.read();
        if enabled.sai1en().bit_is_set() {
            let sai1 = &*pac::SAI1::ptr();
            sai1.cha().cr1.modify(|_, w| w.saien().clear_bit());
            sai1.chb().cr1.modify(|_, w| w.saien().clear_bit());
        }
        if enabled.sai2en().bit_is_set() {
            (*pac::SAI2::ptr()).cha().cr1.modify(|_, w| w.saien().clear_bit());
        }
        core::ptr::write_volatile(GPIOE_BSRR, LED_ON);
    }
    loop {
        cortex_m::asm::nop();
    }
}
```

- [ ] **Step 6: SysTick and the HC165 delay from the real clock**

In `chimera-stm32/src/controls.rs`:
- add `use chimera_core::clock_plan::{cycles_for_ns, systick_reload};`
- add after the `SYST_CVR` constant:

```rust
pub const CONTROLS_HZ: u32 = 500;
// The HC165 was clocked with 100-cycle spins at 400 MHz: keep 250 ns at any clock.
const HC165_HALF_PERIOD_NS: u32 = 250;
static HC165_DELAY: AtomicU32 = AtomicU32::new(100);
```

- replace `start_systick` with:

```rust
pub fn start_systick(cpu_hz: u32) {
    HC165_DELAY.store(cycles_for_ns(cpu_hz, HC165_HALF_PERIOD_NS), Ordering::Relaxed);
    let reload = systick_reload(cpu_hz, CONTROLS_HZ);
    // SAFETY: SYST_CSR/RVR/CVR are the Cortex-M SysTick registers at their
    // fixed addresses; `enable()` gates the ISR on `READY`, so nothing reads
    // these before this single-threaded init runs.
    unsafe {
        core::ptr::write_volatile(SYST_CSR, 0);
        core::ptr::write_volatile(SYST_RVR, reload);
        core::ptr::write_volatile(SYST_CVR, 0);
        core::ptr::write_volatile(SYST_CSR, 0b111);
    }
}
```

- in `isr_tick`, add `let d = HC165_DELAY.load(Ordering::Relaxed);` before the `let bits = unsafe {` block and replace each of the four `cortex_m::asm::delay(100);` with `cortex_m::asm::delay(d);`
- in `snapshot` replace the index loop over `self.btn_cur` (clippy `needless_range_loop`) with:

```rust
        for (i, cur) in self.btn_cur.iter_mut().enumerate() {
            *cur = debounced & (1 << i) != 0;
        }
```

- [ ] **Step 7: Display delays in microseconds**

In `chimera-stm32/src/display.rs` change `pub fn init(&mut self) {` to `pub fn init(&mut self, cpu_hz: u32) {` and replace the delays: `cortex_m::asm::delay(5_000_000);` (both) with `crate::clocks::delay_us(cpu_hz, 12_500);` and `cortex_m::asm::delay(60_000_000);` (both) with `crate::clocks::delay_us(cpu_hz, 150_000);`.

- [ ] **Step 8: Old audio path — DMA buffer in the non-cacheable section, typed priority**

In `chimera-stm32/src/audio.rs`:
- change `#[unsafe(link_section = ".ram_d2")]` on `AUDIO_BUF` to `#[unsafe(link_section = ".ram_d2.dma")]`
- in `init_pll3` replace `cortex_m::asm::delay(100);` with `let _ = rcc.apb2enr.read();`
- change `pub fn init_dma() {` to `pub fn init_dma(nvic: &mut NVIC) {`; replace its `cortex_m::asm::delay(100);` with `let _ = rcc.ahb1enr.read();`; replace the `unsafe { let mut core = cortex_m::Peripherals::steal(); core.NVIC.set_priority(pac::Interrupt::DMA1_STR0, 3); NVIC::unmask(pac::Interrupt::DMA1_STR0); }` block with:

```rust
    crate::priority::set_irq(nvic, pac::Interrupt::DMA1_STR0, crate::priority::Priority::AUDIO);
    // SAFETY: the buffer is pre-filled and the handler only touches the
    // audio statics of this module.
    unsafe { NVIC::unmask(pac::Interrupt::DMA1_STR0) };
```

- [ ] **Step 9: `main.rs`**

Replace `chimera-stm32/src/main.rs` with:

```rust
#![no_std]
#![no_main]

mod audio;
mod cache;
mod clocks;
mod controls;
mod display;
mod panic;
mod priority;
mod shared;

use chimera_core::clock_plan::SiliconRev;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::perf::PerfTracker;
use chimera_core::ui::{UiState, draw, theme};
use chimera_hal::ChimeraDisplay;
use controls::Stm32Controls;
use cortex_m_rt::{entry, exception, pre_init};
use display::Stm32Display;
use priority::Priority;
use stm32h7xx_hal::{pac, prelude::*, spi};

#[pre_init]
unsafe fn before_main() {
    // SAFETY: runs once, before `main` and before interrupts are enabled, on
    // a single core. 0xE000_ED08 is the SCB->VTOR register (a valid, aligned,
    // memory-mapped address on every Cortex-M7), and 0x0802_0000 is our
    // linked vector table's flash address.
    unsafe {
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, 0x0802_0000);
    }
}

#[exception]
fn SysTick() {
    controls::isr_tick();
}

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();

    cache::enable_d2_sram();
    let rev = clocks::read_rev(&dp.DBGMCU);
    let (ccdr, clk) = clocks::freeze(dp.PWR, dp.RCC, &dp.SYSCFG, rev);
    cache::init(&mut cp.MPU, &mut cp.SCB, &mut cp.CPUID);

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
    let _hc_data = gpiof.pf2.into_floating_input();
    let _hc_load = gpiof.pf1.into_push_pull_output();
    let _hc_clk = gpiof.pf0.into_push_pull_output();

    let mut led = gpioe.pe1.into_push_pull_output();
    let mut backlight = gpioe.pe11.into_push_pull_output();
    backlight.set_high();

    let _sai_mclk = gpioe.pe2.into_alternate::<6>();
    let _sai_fs = gpioe.pe4.into_alternate::<6>();
    let _sai_sck = gpioe.pe5.into_alternate::<6>();
    let _sai_sd_a = gpioe.pe6.into_alternate::<6>();
    led.set_high();

    let mut sck = gpioa.pa5.into_alternate::<5>();
    let mut mosi = gpioa.pa7.into_alternate::<5>();
    sck.set_speed(stm32h7xx_hal::gpio::Speed::High);
    mosi.set_speed(stm32h7xx_hal::gpio::Speed::High);
    let dc = gpiod.pd8.into_push_pull_output();
    let reset = gpiod.pd9.into_push_pull_output();
    let cs = gpiod.pd10.into_push_pull_output();

    let spi = dp.SPI1.spi(
        (sck, spi::NoMiso, mosi),
        spi::Config::new(spi::MODE_0),
        50.MHz(),
        ccdr.peripheral.SPI1,
        &ccdr.clocks,
    );

    let mut display = Stm32Display::new(spi, dc, reset, cs);
    clocks::delay_us(clk.cpu_hz, 250_000);
    display.init(clk.cpu_hz);
    boot_splash(&mut display, &clk);

    let mut controls = Stm32Controls::new();
    let mut ui = UiState::new();
    let perf = PerfTracker::new();

    controls::start_systick(clk.cpu_hz);
    priority::set_systick(&mut cp.SCB, Priority::SYSTICK);
    controls::enable();

    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    audio::init_scope(scope_w);
    audio::init_pll3();
    audio::init_sai1a();

    // SAFETY: ui.performance lives in main's stack frame which never returns (-> !).
    // Part 0's sound params/mod_state outlive the audio DMA for the same reason.
    unsafe {
        audio::init_voice(
            &ui.performance.parts[0].sound.params as *const _,
            &ui.performance.parts[0].sound.mod_state as *const _,
        );
    }
    audio::trigger_note(chimera_hal::MidiNote::A4, chimera_hal::Velocity::DEFAULT);

    audio::prefill_buffer();
    audio::init_dma(&mut cp.NVIC);
    audio::enable_sai();

    ui.update();
    ui.render_with_scope(&mut display, &perf.stats, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, scope_r.read());
    led.set_low();

    loop {
        controls.snapshot();
        if controls.has_activity() {
            ui.handle_input(&controls);
        }
        ui.update();
        let flush_list = ui.render_dirty_with_scope(&mut display, &perf.stats, scope_r.read());
        for &(ys, ye) in &flush_list {
            if ys != ye {
                display.flush_region(ys, ye);
            }
        }
    }
}

// Temporary (bring-up step 1): which revision and clock this board runs;
// the AUDIO page replaces it in step 6.
fn boot_splash(display: &mut impl ChimeraDisplay, clk: &clocks::Clocks) {
    use core::fmt::Write;
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let _ = write!(line, "REV {}  {} MHZ", clk.rev.label(), clk.cpu_hz / 1_000_000);
    draw::text(display, &theme::FONT_VALUE, "CHIMERA", theme::MARGIN_X, 140, theme::INK);
    draw::text(display, &theme::FONT_VALUE, line.as_str(), theme::MARGIN_X, 162, theme::INK2);
    if let SiliconRev::Unknown(id) = clk.rev {
        line.clear();
        let _ = write!(line, "REV_ID 0x{id:04X}");
        draw::text(display, &theme::FONT_LABEL, line.as_str(), theme::MARGIN_X, 180, theme::MID);
    }
    display.flush();
    clocks::delay_us(clk.cpu_hz, 1_500_000);
}
```

- [ ] **Step 10: Build and check the link layout**

Run:

```bash
cargo build -p chimera-stm32 --target thumbv7em-none-eabihf
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
rust-nm -n target/thumbv7em-none-eabihf/release/chimera-stm32 | grep -E ' (_stack_start|_stack_end|__sram_d2_dma|__eram_d2_dma|.*AUDIO_BUF.*|.*INSTRUMENT.*)$'
```

Expected: both builds link; `_stack_end` 20000000, `_stack_start` 20020000, `__sram_d2_dma` 30000000 with `AUDIO_BUF` at 30000000, `__eram_d2_dma` 30001000, `INSTRUMENT` at or after 30001000.

- [ ] **Step 11: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 12: On-device checks (bring-up step 1)**

Run `just flash`, then:
- [ ] The splash shows `REV V  480 MHZ` (or `REV Y  400 MHZ`) for 1.5 s, then the normal UI. Record which revision this board is in the PR description.
  - If it shows `REV ?  400 MHZ` with `REV_ID 0x0000`, DBGMCU reads blank without a debugger: change `read_rev` to return `SiliconRev::V` when the read is `0x0000` (the stock firmware runs this board at 480 MHz unconditionally, `main.c:217`), reflash, and open a GitHub issue with the finding.
  - If it shows `REV ?` with another ID, keep the 400 MHz fallback and open an issue with the ID.
- [ ] The UI draws cleanly (SPI now 48 MHz on rev V) and responds; buttons don't bounce; one slow encoder click moves one step; a fast spin accelerates (the controls tick is now the intended 500 Hz, not 1 kHz).
- [ ] The A4 test tone still plays (on rev V it is still an octave high — MCKDIV 5 by the rev V formula — until Task 13).
- [ ] Safe panic: temporarily add `panic!();` right after `audio::enable_sai();`, flash, confirm the outputs go silent (no 750 Hz buzz) and the LED stays on; remove the line and reflash. Do not commit the temporary line.

- [ ] **Step 13: Commit**

```bash
git add chimera-stm32/Cargo.toml Cargo.lock chimera-stm32/memory.x chimera-stm32/build.rs \
  chimera-stm32/src/main.rs chimera-stm32/src/clocks.rs chimera-stm32/src/cache.rs \
  chimera-stm32/src/priority.rs chimera-stm32/src/panic.rs chimera-stm32/src/controls.rs \
  chimera-stm32/src/display.rs chimera-stm32/src/audio.rs
git commit -m "Clocks by silicon revision, caches and MPU, stack in DTCM, typed priorities, safe panic"
```

---

### Task 13: Bring-up step 2 — pair 1 at 48 kHz with 32-bit data, the old voice as tone

**Files:**
- Delete: `chimera-stm32/src/audio.rs`
- Create: `chimera-stm32/src/audio/mod.rs`, `chimera-stm32/src/audio/sai.rs`, `chimera-stm32/src/audio/dma.rs`
- Modify: `chimera-stm32/src/clocks.rs` (add `init_pll3`), `chimera-stm32/src/main.rs`

**Interfaces:**
- Consumes: `clock_plan::{Pll3Config, PllRange, VcoRange, pll3_for}` (Task 5); `audio_out::{DacSample, Half, interleave, plan_halves}`, `DacPair::ALL` (Task 4); `ScopeWriter` (Task 9); `priority` (Task 12).
- Produces (firmware):
  - `clocks::init_pll3(cfg: &Pll3Config)` (also selects PLL3 P for SAI1 and SAI2/3).
  - `audio::sai::{Role, init(new_sai: bool, mckdiv: u8), start(), data_register(pair: DacPair) -> u32}`.
  - `audio::dma::{RING_WORDS, OVERRUNS, clear(), half_mut(pair, half) -> &'static mut [DacSample; BLOCK_SIZE * 2], init(&mut NVIC), start()}` and the `DMA1_STR0` handler, which calls `audio::render_half(half)` per `plan_halves`.
  - `audio::{render_half(half: Half), prefill(), init_scope(w), init_voice, trigger_note}`.

- [ ] **Step 1: PLL3 from the plan**

Append to `chimera-stm32/src/clocks.rs` (and extend its import to `use chimera_core::clock_plan::{Pll3Config, PllRange, SiliconRev, VcoRange, cycles_for_us};`):

```rust
pub fn init_pll3(cfg: &Pll3Config) {
    // SAFETY: single-threaded init after the HAL's `freeze` (which leaves
    // PLL3 alone) and before any SAI runs; nothing else touches PLL3.
    let rcc = unsafe { &*pac::RCC::ptr() };
    rcc.cr.modify(|_, w| w.pll3on().off());
    while rcc.cr.read().pll3rdy().is_ready() {}
    rcc.pllckselr.modify(|_, w| w.divm3().bits(cfg.m));
    // SAFETY: DIVN3 = N − 1 with N in 4..=512 and DIVP3 = P − 1 with P in
    // 1..=128 (clock_plan tests); DIVQ3/DIVR3 = 1, their outputs stay off.
    rcc.pll3divr.write(|w| unsafe {
        w.divn3().bits(cfg.n - 1).divp3().bits(cfg.p - 1).divq3().bits(1).divr3().bits(1)
    });
    // FRACN3 is latched when FRACEN goes from 0 to 1.
    rcc.pllcfgr.modify(|_, w| w.pll3fracen().reset());
    rcc.pll3fracr.write(|w| w.fracn3().bits(cfg.fracn));
    rcc.pllcfgr.modify(|_, w| {
        let w = match cfg.vco {
            VcoRange::Wide => w.pll3vcosel().wide_vco(),
            VcoRange::Medium => w.pll3vcosel().medium_vco(),
        };
        let w = match cfg.range {
            PllRange::R1To2 => w.pll3rge().range1(),
            PllRange::R2To4 => w.pll3rge().range2(),
            PllRange::R4To8 => w.pll3rge().range4(),
            PllRange::R8To16 => w.pll3rge().range8(),
        };
        w.pll3fracen().set().divp3en().enabled()
    });
    rcc.cr.modify(|_, w| w.pll3on().on());
    while !rcc.cr.read().pll3rdy().is_ready() {}
    rcc.d2ccip1r.modify(|_, w| w.sai1sel().pll3_p().sai23sel().pll3_p());
}
```

- [ ] **Step 2: `audio/sai.rs` — SAI1 A master, 32-bit I2S**

Create `chimera-stm32/src/audio/sai.rs`:

```rust
use chimera_core::part::DacPair;
use stm32h7xx_hal::pac;

// CR1 bit 27, rev B and later only (absent from the rev-Y-based PAC).
const MCKEN: u32 = 1 << 27;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Master,
    InternalSlave,
    ExternalSlave,
}

pub fn init(new_sai: bool, mckdiv: u8) {
    // SAFETY: single-threaded init before the audio interrupt is unmasked;
    // RCC's APB2ENR and SAI1 are not used elsewhere yet.
    let (rcc, sai1) = unsafe { (&*pac::RCC::ptr(), &*pac::SAI1::ptr()) };
    rcc.apb2enr.modify(|_, w| w.sai1en().enabled());
    let _ = rcc.apb2enr.read();
    configure(sai1.cha(), Role::Master, mckdiv, new_sai);
}

pub fn start() {
    // SAFETY: called once from `main` after the DMA stream is enabled and
    // the ring pre-filled; only SAIEN is set.
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    sai1.cha().cr1.modify(|_, w| w.saien().set_bit());
}

pub fn data_register(pair: DacPair) -> u32 {
    // SAFETY: only register addresses are taken; nothing is read or written.
    let (sai1, sai2) = unsafe { (&*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    match pair {
        DacPair::P1 => sai1.cha().dr.as_ptr() as u32,
        DacPair::P2 => sai1.chb().dr.as_ptr() as u32,
        DacPair::P3 => sai2.cha().dr.as_ptr() as u32,
    }
}

fn configure(ch: &pac::sai1::CH, role: Role, mckdiv: u8, new_sai: bool) {
    ch.cr1.modify(|_, w| w.saien().clear_bit());
    while ch.cr1.read().saien().bit_is_set() {}
    // SAFETY: MCKDIV is a 6-bit field and `pll3_for` gives 4 or 2 (clock_plan
    // tests); every other field is set through its enumerated variants.
    ch.cr1.write(|w| {
        let w = match role {
            Role::Master => w.mode().master_tx().syncen().asynchronous(),
            Role::InternalSlave => w.mode().slave_tx().syncen().internal(),
            Role::ExternalSlave => w.mode().slave_tx().syncen().external(),
        };
        let w = w
            .prtcfg()
            .free()
            .ds()
            .bit32()
            .lsbfirst()
            .msb_first()
            // The CS4344 samples on SCK rising edges, so data must change on
            // falling ones (the ST HAL's I2S transmit setting).
            .ckstr()
            .rising_edge()
            .mono()
            .stereo()
            .nodiv()
            .master_clock()
            .dmaen()
            .enabled();
        unsafe { w.mckdiv().bits(mckdiv) }
    });
    if role == Role::Master && new_sai {
        // SAFETY: sets only MCKEN in this block's CR1; the caller checked the
        // silicon has it.
        ch.cr1.modify(|r, w| unsafe { w.bits(r.bits() | MCKEN) });
    }
    ch.cr2.write(|w| w.fth().quarter1().fflush().set_bit());
    // SAFETY: FRL 63 (64-bit frame), FSALL 31 (FS half the frame), NBSLOT 1
    // (two slots) and SLOTEN 0b11 are within their RM0433 field widths.
    ch.frcr.write(|w| {
        unsafe { w.frl().bits(63).fsall().bits(31) }
            .fsdef()
            .set_bit()
            .fspol()
            .falling_edge()
            .fsoff()
            .before_first()
    });
    ch.slotr.write(|w| unsafe { w.nbslot().bits(1).sloten().bits(0b11) }.slotsz().bit32());
}
```

- [ ] **Step 3: `audio/dma.rs` — rings in `.ram_d2.dma`, stream 0**

Create `chimera-stm32/src/audio/dma.rs`:

```rust
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_core::audio_out::{DacSample, Half, plan_halves};
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::part::DacPair;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::priority::{self, Priority};

pub const RING_WORDS: usize = 2 * BLOCK_SIZE * 2;
// DMAMUX1 requests for SAI1_A, SAI1_B, SAI2_A (RM0433; stock PreenFM3 uses the same).
const REQUEST_ID: [u8; DAC_PAIRS] = [87, 88, 89];

#[repr(C, align(32))]
struct Rings([[[DacSample; BLOCK_SIZE * 2]; 2]; DAC_PAIRS]);
const _: () = assert!(core::mem::size_of::<Rings>() == DAC_PAIRS * RING_WORDS * 4);

#[unsafe(link_section = ".ram_d2.dma")]
static mut RINGS: MaybeUninit<Rings> = MaybeUninit::uninit();

pub static OVERRUNS: AtomicU32 = AtomicU32::new(0);

pub fn clear() {
    // SAFETY: before any DMA runs; D2 is NOLOAD, and zero bytes are valid
    // `DacSample`s, so later references point at initialised memory.
    unsafe { addr_of_mut!(RINGS).cast::<Rings>().write_bytes(0, 1) };
}

pub fn half_mut(pair: DacPair, half: Half) -> &'static mut [DacSample; BLOCK_SIZE * 2] {
    // SAFETY: `clear` ran first; only the audio interrupt, and the pre-fill
    // before it is unmasked, write the rings, one half at a time, while the
    // DMA reads the other half.
    unsafe { &mut (*addr_of_mut!(RINGS).cast::<Rings>()).0[pair.index()][half.index()] }
}

fn ring_addr(pair: DacPair) -> u32 {
    addr_of!(RINGS) as u32 + (pair.index() * RING_WORDS * 4) as u32
}

pub fn init(nvic: &mut NVIC) {
    // SAFETY: single-threaded init before the stream-0 interrupt is unmasked;
    // DMA1 and DMAMUX1 are used only here and in the handler below.
    let (rcc, dma1, dmamux) = unsafe { (&*pac::RCC::ptr(), &*pac::DMA1::ptr(), &*pac::DMAMUX1::ptr()) };
    rcc.ahb1enr.modify(|_, w| w.dma1en().set_bit());
    let _ = rcc.ahb1enr.read();
    clear_flags(dma1);
    configure_stream(dma1, dmamux, DacPair::P1, true);
    priority::set_irq(nvic, pac::Interrupt::DMA1_STR0, Priority::AUDIO);
    // SAFETY: the rings are pre-filled; the handler touches only the rings,
    // the render path and DMA1's stream 0–2 flags.
    unsafe { NVIC::unmask(pac::Interrupt::DMA1_STR0) };
}

pub fn start() {
    // SAFETY: called once from `main` after `init`; only EN is set.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    dma1.st[DacPair::P1.index()].cr.modify(|_, w| w.en().enabled());
}

fn configure_stream(
    dma1: &pac::dma1::RegisterBlock,
    dmamux: &pac::dmamux1::RegisterBlock,
    pair: DacPair,
    interrupts: bool,
) {
    let st = &dma1.st[pair.index()];
    st.cr.modify(|_, w| w.en().disabled());
    while st.cr.read().en().is_enabled() {}
    // SAFETY: the request ID is this pair's SAI block; PAR is that block's
    // data register and M0AR a word-aligned ring of RING_WORDS words in D2,
    // which DMA1 can reach.
    unsafe {
        dmamux.ccr[pair.index()].modify(|_, w| w.dmareq_id().bits(REQUEST_ID[pair.index()]));
        st.par.write(|w| w.pa().bits(super::sai::data_register(pair)));
        st.m0ar.write(|w| w.m0a().bits(ring_addr(pair)));
    }
    st.ndtr.write(|w| w.ndt().bits(RING_WORDS as u16));
    st.cr.write(|w| {
        let w = w
            .dir()
            .memory_to_peripheral()
            .circ()
            .enabled()
            .minc()
            .incremented()
            .pinc()
            .fixed()
            .msize()
            .bits32()
            .psize()
            .bits32()
            .pl()
            .very_high();
        if interrupts { w.htie().enabled().tcie().enabled() } else { w }
    });
}

fn clear_flags(dma1: &pac::dma1::RegisterBlock) {
    dma1.lifcr.write(|w| {
        w.ctcif0().clear().chtif0().clear().cteif0().clear().cdmeif0().clear().cfeif0().clear()
            .ctcif1().clear().chtif1().clear().cteif1().clear().cdmeif1().clear().cfeif1().clear()
            .ctcif2().clear().chtif2().clear().cteif2().clear().cdmeif2().clear().cfeif2().clear()
    });
}

#[interrupt]
fn DMA1_STR0() {
    // SAFETY: once `start` has run, this handler is the only reader and
    // clearer of DMA1's stream 0–2 flags.
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    let lisr = dma1.lisr.read();
    let (half_done, full_done) = (lisr.htif0().is_half(), lisr.tcif0().is_complete());
    dma1.lifcr.write(|w| {
        if half_done {
            w.chtif0().clear();
        }
        if full_done {
            w.ctcif0().clear();
        }
        w
    });
    let plan = plan_halves(half_done, full_done);
    for half in plan.halves.into_iter().flatten() {
        super::render_half(half);
    }
    let after = dma1.lisr.read();
    let late = after.htif0().is_half() || after.tcif0().is_complete();
    let overruns = plan.overrun as u32 + late as u32;
    if overruns > 0 {
        OVERRUNS.fetch_add(overruns, Ordering::Relaxed);
    }
}
```

- [ ] **Step 4: `audio/mod.rs` — the old voice as a 48 kHz tone on pair 1**

Delete `chimera-stm32/src/audio.rs` with `git rm chimera-stm32/src/audio.rs` (the deletion is then staged) and create `chimera-stm32/src/audio/mod.rs`:

```rust
pub mod dma;
pub mod sai;

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::audio_out::{Half, interleave};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS};
use chimera_core::instrument::{DacOut, Instrument};
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::part::DacPair;
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::Writer;
use chimera_hal::{MidiNote, Velocity};

// SAFETY: never read or written before Task 15, which builds it in place
// under the audio interrupt's single-owner discipline.
#[used]
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();

static mut WORK: [f32; BLOCK_SIZE] = [0.0; BLOCK_SIZE];
static mut DAC: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
static mut VOICE: Option<Voice> = None;
static mut PARAMS: Option<*const ParamSnapshot> = None;
static mut MOD_STATE_PTR: Option<*const ModState> = None;
static DEFAULT_MOD_STATE: ModState = ModState::new();
static mut SCOPE: MaybeUninit<ScopeWriter> = MaybeUninit::uninit();

pub fn init_scope(w: Writer<ScopeFrame>) {
    // SAFETY: called once from `main` before `prefill` and before the DMA
    // interrupt is unmasked; nothing else touches `SCOPE` yet.
    unsafe { (*addr_of_mut!(SCOPE)).write(ScopeWriter::new(w)) };
}

pub fn render_half(half: Half) {
    // SAFETY: only the DMA1 stream 0 interrupt and `prefill` (before that
    // interrupt is unmasked) call this, never concurrently; they are the only
    // users of these statics after init. `init_scope` ran before `prefill`.
    unsafe {
        let work = &mut *addr_of_mut!(WORK);
        let dac = &mut *addr_of_mut!(DAC);
        match ((*addr_of_mut!(VOICE)).as_mut(), *addr_of_mut!(PARAMS)) {
            (Some(voice), Some(params)) => {
                let mod_state = match *addr_of_mut!(MOD_STATE_PTR) {
                    Some(p) => &*p,
                    None => &DEFAULT_MOD_STATE,
                };
                voice.render(work, &*params, mod_state);
            }
            _ => work.fill(0.0),
        }
        (*addr_of_mut!(SCOPE)).assume_init_mut().write(work);
        for (i, &s) in work.iter().enumerate() {
            dac[0][2 * i] = s;
            dac[0][2 * i + 1] = s;
        }
        interleave(dac, DacPair::P1, dma::half_mut(DacPair::P1, half));
    }
}

pub fn prefill() {
    render_half(Half::First);
    render_half(Half::Second);
}

/// # Safety
/// `params_ptr` and `mod_ptr` must outlive the audio system.
pub unsafe fn init_voice(params_ptr: *const ParamSnapshot, mod_ptr: *const ModState) {
    // SAFETY: called once during single-threaded init before the ISR is active.
    unsafe {
        addr_of_mut!(VOICE).write(Some(Voice::new(chimera_hal::SAMPLE_RATE)));
        addr_of_mut!(PARAMS).write(Some(params_ptr));
        addr_of_mut!(MOD_STATE_PTR).write(Some(mod_ptr));
    }
}

pub fn trigger_note(note: MidiNote, velocity: Velocity) {
    // SAFETY: called during init before the ISR is active.
    unsafe {
        if let (Some(voice), Some(p)) = ((*addr_of_mut!(VOICE)).as_mut(), *addr_of_mut!(PARAMS)) {
            voice.note_on(note, velocity, &*p);
        }
    }
}
```

`Voice::new` puts a 40 KB `Voice` on the DTCM stack once at boot, as before; this path is deleted in Task 15.

- [ ] **Step 5: `main.rs` — new start sequence and SAI pin speed**

In `chimera-stm32/src/main.rs`:
- add `use chimera_core::clock_plan::pll3_for;` (extend the existing `clock_plan` import) and `use stm32h7xx_hal::gpio::Speed;`
- replace the four SAI pin lines with:

```rust
    let mut sai_mclk = gpioe.pe2.into_alternate::<6>();
    let mut sai_fs = gpioe.pe4.into_alternate::<6>();
    let mut sai_sck = gpioe.pe5.into_alternate::<6>();
    let mut sai_sd_a1 = gpioe.pe6.into_alternate::<6>();
    // MCLK is 12.288 MHz, the edge of the low-speed GPIO range.
    sai_mclk.set_speed(Speed::Medium);
    sai_fs.set_speed(Speed::Medium);
    sai_sck.set_speed(Speed::Medium);
    sai_sd_a1.set_speed(Speed::Medium);
```

- replace everything from `audio::init_pll3();` to `audio::enable_sai();` with:

```rust
    let pll3 = pll3_for(clocks::HSE_HZ, chimera_hal::SAMPLE_RATE, clk.rev);
    clocks::init_pll3(&pll3);
    audio::sai::init(clk.rev.new_sai(), pll3.mckdiv);

    // SAFETY: ui.performance lives in main's stack frame which never returns (-> !).
    // Part 0's sound params/mod_state outlive the audio DMA for the same reason.
    unsafe {
        audio::init_voice(
            &ui.performance.parts[0].sound.params as *const _,
            &ui.performance.parts[0].sound.mod_state as *const _,
        );
    }
    audio::trigger_note(chimera_hal::MidiNote::A4, chimera_hal::Velocity::DEFAULT);

    audio::dma::clear();
    audio::prefill();
    audio::dma::init(&mut cp.NVIC);
    audio::dma::start();
    audio::sai::start();
```

- [ ] **Step 6: Build and run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS (firmware builds and links; `RINGS` is 3 KB inside `.ram_d2_dma`).

- [ ] **Step 7: On-device checks (bring-up step 2)**

Run `just flash`, then:
- [ ] Output 1 plays A4. Measure it with a tuner: **440.0 Hz ± 1 cent**, both channels. On rev V this is an octave *down* from before (it was 95.8 kHz playback of a 48 kHz render); on rev Y it is 3 cents higher than before.
  - 880 Hz or 220 Hz means the MCKDIV formula is wrong for this REV_ID: note the splash's revision, flip the result of `SiliconRev::new_sai` for that revision in `clock_plan.rs` (and its test), reflash to confirm 440 Hz, and open a GitHub issue — ADR 0020's formula statement then needs a superseding ADR.
  - Silence: check MCLK on PE2 with a scope (12.288 MHz). None on rev V means MCKEN didn't take; none on rev Y means the SAI clock isn't running — check `SAI1SEL`.
  - Audible crackle or distortion on a clean tone: set `.ckstr().falling_edge()` in `sai::configure` (the pre-plan value), reflash, and note it in the PR.
- [ ] Optional: a frequency counter on PE4 (FS) reads 47,999.98 Hz (±2 ppm plus the crystal's tolerance).
- [ ] The tone is clean at full level with no zipper or periodic click (the 32-bit slot carries the sample's top 24 bits; a DS mismatch would sound like loud noise).
- [ ] The scope viz still shows the tone.

- [ ] **Step 8: Commit**

```bash
git add chimera-stm32/src/audio chimera-stm32/src/clocks.rs chimera-stm32/src/main.rs
git commit -m "Pair 1 at 48 kHz with 32-bit SAI slots"
```

---

### Task 14: Bring-up step 3 — pairs 2 and 3 on SAI1 B and SAI2 A

**Files:**
- Modify: `chimera-stm32/src/audio/sai.rs` (`init`, `start`), `chimera-stm32/src/audio/dma.rs` (`init`, `start`, desync check), `chimera-stm32/src/audio/mod.rs` (`render_half`), `chimera-stm32/src/main.rs` (pins PE3, PD11)

**Interfaces:**
- Consumes: `audio_out::desynced` (Task 4).
- Produces (firmware): `sai::init` configures all three blocks and the sync scheme; `sai::start` enables SAI2 A, SAI1 B, then SAI1 A; `dma::init` configures streams 0–2 (only 0 interrupts); `dma::start` enables all three; `dma::DESYNCS: AtomicU32`.

- [ ] **Step 1: Three SAI blocks on one clock**

In `chimera-stm32/src/audio/sai.rs` replace `init` and `start` with:

```rust
pub fn init(new_sai: bool, mckdiv: u8) {
    // SAFETY: single-threaded init before the audio interrupt is unmasked;
    // RCC's APB2ENR, SAI1 and SAI2 are not used elsewhere yet.
    let (rcc, sai1, sai2) = unsafe { (&*pac::RCC::ptr(), &*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    rcc.apb2enr.modify(|_, w| w.sai1en().enabled().sai2en().enabled());
    let _ = rcc.apb2enr.read();
    configure(sai1.cha(), Role::Master, mckdiv, new_sai);
    configure(sai1.chb(), Role::InternalSlave, mckdiv, new_sai);
    configure(sai2.cha(), Role::ExternalSlave, mckdiv, new_sai);
    // SAFETY: SYNCOUT = 01 exports block A's FS and SCK as SAI1's sync
    // output; SYNCIN = 00 makes SAI1 SAI2's sync source. Written while every
    // block is disabled, as RM0433 requires.
    unsafe {
        sai1.gcr.write(|w| w.syncout().bits(0b01));
        sai2.gcr.write(|w| w.syncin().bits(0b00));
    }
}

pub fn start() {
    // SAFETY: called once from `main` after the three DMA streams run and the
    // rings are pre-filled; only SAIEN is set. Slaves first, master last, so
    // all three start on the master's first frame.
    let (sai1, sai2) = unsafe { (&*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    sai2.cha().cr1.modify(|_, w| w.saien().set_bit());
    sai1.chb().cr1.modify(|_, w| w.saien().set_bit());
    sai1.cha().cr1.modify(|_, w| w.saien().set_bit());
}
```

- [ ] **Step 2: Three DMA streams and the desync check**

In `chimera-stm32/src/audio/dma.rs`:
- extend the `audio_out` import with `desynced`;
- add after `OVERRUNS`:

```rust
pub static DESYNCS: AtomicU32 = AtomicU32::new(0);
// Streams 1 and 2 may trail stream 0 by the SAI FIFO (8 words) plus the DMA's.
const DESYNC_TOLERANCE: u16 = 16;
```

- in `init` replace `configure_stream(dma1, dmamux, DacPair::P1, true);` with:

```rust
    for pair in DacPair::ALL {
        configure_stream(dma1, dmamux, pair, pair == DacPair::P1);
    }
```

- replace `start`'s body line with:

```rust
    for pair in DacPair::ALL {
        dma1.st[pair.index()].cr.modify(|_, w| w.en().enabled());
    }
```

- in `DMA1_STR0`, insert after the flag-clearing `dma1.lifcr.write(…)`:

```rust
    let ndtr = |p: DacPair| dma1.st[p.index()].ndtr.read().ndt().bits();
    let lead = ndtr(DacPair::P1);
    let trailing_apart = [DacPair::P2, DacPair::P3]
        .into_iter()
        .any(|p| desynced(lead, ndtr(p), RING_WORDS as u16, DESYNC_TOLERANCE));
    if trailing_apart || lisr.teif1().is_error() || lisr.teif2().is_error() {
        DESYNCS.fetch_add(1, Ordering::Relaxed);
        dma1.lifcr.write(|w| w.cteif1().clear().cteif2().clear());
    }
```

- [ ] **Step 3: A distinguishable tone on each pair**

In `chimera-stm32/src/audio/mod.rs` replace the `for (i, &s) in work…` loop and the `interleave(…)` line in `render_half` with:

```rust
        // Pair 1 both channels, pair 2 left only, pair 3 right only: each
        // jack and channel can be told apart by ear.
        for (i, &s) in work.iter().enumerate() {
            dac[0][2 * i] = s;
            dac[0][2 * i + 1] = s;
            dac[1][2 * i] = s;
            dac[1][2 * i + 1] = 0.0;
            dac[2][2 * i] = 0.0;
            dac[2][2 * i + 1] = s;
        }
        for pair in DacPair::ALL {
            interleave(dac, pair, dma::half_mut(pair, half));
        }
```

- [ ] **Step 4: Pins for pairs 2 and 3**

In `chimera-stm32/src/main.rs` after the PE6 lines add:

```rust
    let mut sai_sd_b1 = gpioe.pe3.into_alternate::<6>();
    let mut sai_sd_a2 = gpiod.pd11.into_alternate::<10>();
    sai_sd_b1.set_speed(Speed::Medium);
    sai_sd_a2.set_speed(Speed::Medium);
```

(PD11 is SAI2_SD_A on **AF10**, not AF6 — stock PreenFM3 `stm32h7xx_hal_msp_pfm3.c`.)

- [ ] **Step 5: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 6: On-device checks (bring-up step 3)**

Run `just flash`, then with headphones on each output jack in turn:
- [ ] Output 1: A4 in both ears. Output 2: left ear only. Output 3: right ear only.
- [ ] All three are the same pitch (440.0 Hz on a tuner) and stay in phase: plug outputs 1 and 2 into a mixer, pan both centre — no beating or flanging over a minute (one FS for all three blocks).
- [ ] No clicks at start-up beyond the first frame and none in steady state.
  - A pair silent: its data pin's alternate function (PE3 AF6, PD11 AF10) or its DMAMUX ID (88, 89); SAI2 silent also means SAI23SEL or the GCR sync bits.

- [ ] **Step 7: Commit**

```bash
git add chimera-stm32/src/audio chimera-stm32/src/main.rs
git commit -m "Pairs 2 and 3 on SAI1 B and SAI2 A, synced to SAI1 A"
```

---

### Task 15: Bring-up step 4 — `Instrument` in place, DIN MIDI on pair 1

**Files:**
- Create: `chimera-stm32/src/audio/engine.rs`, `chimera-stm32/src/midi_din.rs`
- Modify: `chimera-stm32/src/audio/mod.rs` (old voice path deleted), `chimera-stm32/src/shared.rs` (`take_audio`), `chimera-stm32/src/priority.rs` (`MIDI`), `chimera-stm32/src/main.rs`, `chimera-stm32/Cargo.toml` (`midi-din` feature)

**Interfaces:**
- Consumes: `Instrument::init_in_place`, `FxBus::init_in_place` (Task 8); `TripleBuffer::init_in_place` (Task 2); `NoteSources`, `SourceId` (Task 3); `NoteEvent::from_midi`, `const MidiParser::new` (Task 7); `SampleBudget` (Task 1).
- Produces:
  - Firmware: `audio::engine::{NOTE_SOURCES: usize = 1, NOTES: NoteSources<1>, DIN: SourceId<1> (feature midi-din), init(SampleBudget, Reader<AudioShared>, Writer<ScopeFrame>), render_half(Half), unsafe fn slots()}`; `shared::take_audio() -> Option<(Writer<AudioShared>, Reader<AudioShared>)>`; `Priority::MIDI` (0x40, feature `midi-din`); `midi_din::{ERRORS: AtomicU32, init(&mut NVIC, pclk2_hz: u32)}`.

The old single-voice path (`init_voice`, `trigger_note`, the A4 tone, `VOICE`, `WORK`, `PARAMS`, `MOD_STATE_PTR` and the raw pointers into `ui.performance`) has no caller once the Instrument renders, so it is deleted here rather than in step 5.

- [ ] **Step 1: `shared::take_audio` — the `AudioShared` buffer built in place**

In `chimera-stm32/src/shared.rs` add `use core::mem::MaybeUninit;` and `use chimera_core::instrument::AudioShared;`, and append:

```rust
static mut AUDIO: MaybeUninit<TripleBuffer<AudioShared>> = MaybeUninit::uninit();
static AUDIO_TAKEN: AtomicBool = AtomicBool::new(false);

pub fn take_audio() -> Option<(Writer<AudioShared>, Reader<AudioShared>)> {
    if AUDIO_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `AUDIO`; it is built in place, three 3 KB defaults in turn.
    let slot = unsafe { &mut *addr_of_mut!(AUDIO) };
    Some(TripleBuffer::init_in_place(slot, AudioShared::default).split())
}
```

- [ ] **Step 2: `audio/engine.rs`**

Create `chimera-stm32/src/audio/engine.rs`:

```rust
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::audio_out::{Half, interleave};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
#[cfg(feature = "midi-din")]
use chimera_core::note_queue::SourceId;
use chimera_core::note_queue::NoteSources;
use chimera_core::part::DacPair;
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::{Reader, Writer};

use super::dma;

pub const NOTE_SOURCES: usize = 1;
#[cfg(feature = "midi-din")]
pub const DIN: SourceId<NOTE_SOURCES> = SourceId::new(0);
pub static NOTES: NoteSources<NOTE_SOURCES> = NoteSources::new();

#[used]
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();
static mut FX: MaybeUninit<FxBus> = MaybeUninit::uninit();

struct Engine {
    inst: &'static mut Instrument,
    fx: &'static mut FxBus,
    shared: Reader<AudioShared>,
    scope: ScopeWriter,
    dac: DacOut,
}

static mut ENGINE: MaybeUninit<Engine> = MaybeUninit::uninit();

/// # Safety
/// Only before `init`, and from one context at a time.
pub unsafe fn slots() -> (&'static mut MaybeUninit<Instrument>, &'static mut MaybeUninit<FxBus>) {
    // SAFETY: the caller guarantees no other reference to these statics is live.
    unsafe { (&mut *addr_of_mut!(INSTRUMENT), &mut *addr_of_mut!(FX)) }
}

pub fn init(budget: SampleBudget, shared: Reader<AudioShared>, scope: Writer<ScopeFrame>) {
    // SAFETY: called once from `main` before the DMA interrupt is unmasked,
    // so nothing else holds these statics; the Instrument (D2) and FX bus
    // (AXI) are built in place, and `Engine` (3.5 KB) by value.
    unsafe {
        let (inst_slot, fx_slot) = slots();
        let inst = Instrument::init_in_place(inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(fx_slot);
        (*addr_of_mut!(ENGINE)).write(Engine {
            inst,
            fx,
            shared,
            scope: ScopeWriter::new(scope),
            dac: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS],
        });
    }
}

pub fn render_half(half: Half) {
    // SAFETY: after `init`, only the DMA1 stream 0 interrupt and `prefill`
    // (before that interrupt is unmasked) call this, never concurrently and
    // never re-entrantly, so this is the only live reference to `ENGINE`.
    let e = unsafe { (*addr_of_mut!(ENGINE)).assume_init_mut() };
    let shared = e.shared.read();
    NOTES.drain(|ev| e.inst.handle(ev, shared));
    e.inst.render(e.fx, &mut e.dac, shared, &mut e.scope);
    for pair in DacPair::ALL {
        interleave(&e.dac, pair, dma::half_mut(pair, half));
    }
}
```

Replace `chimera-stm32/src/audio/mod.rs` with:

```rust
pub mod dma;
pub mod engine;
pub mod sai;

use chimera_core::audio_out::Half;

pub fn render_half(half: Half) {
    engine::render_half(half);
}

pub fn prefill() {
    render_half(Half::First);
    render_half(Half::Second);
}
```

- [ ] **Step 3: `Priority::MIDI` and `midi_din.rs`**

In `chimera-stm32/src/priority.rs` add, inside `impl Priority` after `AUDIO`:

```rust
    #[cfg(feature = "midi-din")]
    pub const MIDI: Priority = Priority::level(4);
```

and after the existing `const _` assertion:

```rust
#[cfg(feature = "midi-din")]
const _: () = assert!(Priority::MIDI.bits() == 0x40);
```

Create `chimera-stm32/src/midi_din.rs`:

```rust
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_core::note_queue::NoteEvent;
use chimera_hal::midi::MidiParser;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::{self, interrupt};

use crate::audio::engine::{DIN, NOTES};
use crate::priority::{self, Priority};

const BAUD: u32 = 31_250;

pub static ERRORS: AtomicU32 = AtomicU32::new(0);
static mut PARSER: MidiParser = MidiParser::new();

pub fn init(nvic: &mut NVIC, pclk2_hz: u32) {
    // SAFETY: single-threaded init before USART1's interrupt is unmasked;
    // RCC's APB2ENR and USART1 are not used elsewhere.
    let (rcc, usart) = unsafe { (&*pac::RCC::ptr(), &*pac::USART1::ptr()) };
    rcc.apb2enr.modify(|_, w| w.usart1en().set_bit());
    let _ = rcc.apb2enr.read();
    // FIFOEN can only be written while UE is 0.
    usart.cr1.reset();
    usart.brr.write(|w| w.brr().bits((pclk2_hz / BAUD) as u16));
    usart.icr.write(|w| w.orecf().clear().fecf().clear().ncf().clear());
    usart.cr1.write(|w| w.fifoen().set_bit().rxneie().set_bit().re().set_bit());
    usart.cr1.modify(|_, w| w.ue().set_bit());
    priority::set_irq(nvic, pac::Interrupt::USART1, Priority::MIDI);
    // SAFETY: the handler touches only USART1, its own `PARSER` and the DIN
    // queue, whose single producer it is.
    unsafe { NVIC::unmask(pac::Interrupt::USART1) };
}

#[interrupt]
fn USART1() {
    // SAFETY: this handler owns USART1 after `init`, and `PARSER` is touched
    // only here; the handler can't preempt itself.
    let (usart, parser) = unsafe { (&*pac::USART1::ptr(), &mut *addr_of_mut!(PARSER)) };
    loop {
        let isr = usart.isr.read();
        // An uncleared ORE with RXFNEIE set refires this interrupt forever.
        if isr.ore().bit_is_set() || isr.fe().bit_is_set() || isr.nf().bit_is_set() {
            usart.icr.write(|w| w.orecf().clear().fecf().clear().ncf().clear());
            ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        if isr.rxne().bit_is_clear() {
            break;
        }
        let byte = usart.rdr.read().rdr().bits() as u8;
        if let Some(ev) = parser.feed(byte).and_then(NoteEvent::from_midi) {
            NOTES.source(DIN).push(ev);
        }
    }
}
```

In `chimera-stm32/Cargo.toml` add:

```toml
[features]
default = ["midi-din"]
midi-din = []
```

- [ ] **Step 4: `main.rs` — the Instrument replaces the tone**

In `chimera-stm32/src/main.rs`:
- add `#[cfg(feature = "midi-din")] mod midi_din;` after `mod display;` and `use chimera_core::hw::SampleBudget;`
- after the `gpiod`/`gpioe`/`gpiof` splits add:

```rust
    #[cfg(feature = "midi-din")]
    let _midi_rx = dp.GPIOB.split(ccdr.peripheral.GPIOB).pb7.into_alternate::<7>();
```

- replace everything from `let (scope_w, mut scope_r) = shared::take_scope()…` to `audio::sai::start();` with:

```rust
    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    let (_shared_w, shared_r) = shared::take_audio().expect("audio buffer taken once");
    audio::engine::init(SampleBudget::for_cpu(clk.cpu_hz), shared_r, scope_w);

    let pll3 = pll3_for(clocks::HSE_HZ, chimera_hal::SAMPLE_RATE, clk.rev);
    clocks::init_pll3(&pll3);
    audio::sai::init(clk.rev.new_sai(), pll3.mckdiv);
    audio::dma::clear();
    audio::prefill();
    audio::dma::init(&mut cp.NVIC);
    audio::dma::start();
    audio::sai::start();

    #[cfg(feature = "midi-din")]
    midi_din::init(&mut cp.NVIC, ccdr.clocks.pclk2().raw());
```

- [ ] **Step 5: Build all feature sets and run the full check**

Run:

```bash
cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features
PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check
```

Expected: both PASS; the link places `INSTRUMENT` (≈247 KB) in `.ram_d2` after `.ram_d2_dma` within 288 KB.

- [ ] **Step 6: On-device checks (bring-up step 4)**

Run `just flash`, connect a DIN keyboard (MIDI channel 1) to the MIDI IN socket, headphones on output 1:
- [ ] Keys play Part 1's default Sound, in tune (A4 key = 440 Hz on a tuner); note-offs release.
- [ ] A six-note chord sounds all six; a seventh note steals the oldest tail/held voice (ADR 0015); nothing sticks after releasing everything.
- [ ] Switching the keyboard to channel 2 plays Part 2, also on output 1 (every Part defaults to P1; the UI's edits are not published until Task 16).
- [ ] Unplug and replug the DIN cable while holding keys: the UI keeps responding (no USART livelock); new notes play after replug.
- [ ] Fast playing / pitch-bend and mod-wheel sweeps (dropped for now) cause no glitches or stuck notes.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/instrument.rs chimera-core/tests/instrument_test.rs \
  chimera-stm32/src/audio chimera-stm32/src/midi_din.rs chimera-stm32/src/shared.rs \
  chimera-stm32/src/priority.rs chimera-stm32/src/main.rs chimera-stm32/Cargo.toml
git commit -m "Instrument built in place on the chip, played from DIN MIDI"
```

---

### Task 16: Bring-up step 5 — publish `AudioShared` from the UI, per-Part pair routing

**Files:**
- Modify: `chimera-stm32/src/main.rs`

**Interfaces:**
- Consumes: `shared::take_audio` (Task 15), `Writer::publish`, `AudioShared::update_from`.
- Produces: the main loop publishes the Performance once per frame; the engine's `Instrument::handle`/`render` route each Part to its `mix.output` pair, all three pairs already interleaved.

- [ ] **Step 1: Publish once per frame**

In `chimera-stm32/src/main.rs` rename `_shared_w` to `mut shared_w` and in the loop, after `ui.update();`, add:

```rust
        shared_w.publish(|b| b.update_from(&ui.performance));
```

(`update_from` builds one 3.2 KB temporary on the DTCM stack per frame; the audio interrupt keeps reading its own buffer meanwhile.)

- [ ] **Step 2: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS.

- [ ] **Step 3: On-device checks (bring-up step 5)**

Run `just flash`, DIN keyboard connected, headphones moving between the three outputs:
- [ ] MIX+B1 ▸ PART: OUT to P2 moves Part 1 to output 2 (silent on 1); P3 to output 3. Level and pan changes glide (lerped by the UI), never step.
- [ ] Each of the six Parts, set to its own channel and output, plays from the keyboard on the right jack; layering two Parts on one channel plays both.
- [ ] Sends: raise Part 1's reverb send with the reverb's MIX up: the wet return is on output 1 both sides (ADR 0015) even when Part 1 is on output 2.
- [ ] Editing a Sound (filter cutoff) while holding a note changes it smoothly; loading an FM or Modal Sound into a Part plays it.
- [ ] The oscilloscope viz follows what is played.

- [ ] **Step 4: Commit**

```bash
git add chimera-stm32/src/main.rs
git commit -m "Publish AudioShared from the UI each frame"
```

---
### Task 17: The AUDIO sub-page (System ▸ About ▸ AUDIO) and its screen golden

**Files:**
- Create: `chimera-core/src/ui/audio_page.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod audio_page;`, `frame` takes the stats, `render_with_audio`, `render_dirty_with_audio`, `prime_regions`, `region_data`)
- Modify: `chimera-core/src/ui/renderer.rs` (`Frame.audio`, AUDIO branches)
- Modify: `chimera-core/src/ui/block_def.rs` (`VizType::AudioStats`), `chimera-core/src/ui/block_registry.rs` (`SYS_AUDIO`, About's sub-page), `chimera-core/src/ui/focus.rs` (id comment)
- Modify: `chimera-stm32/src/main.rs` (`prime_regions` call)
- Test: `chimera-core/tests/audio_page_test.rs` (create), `chimera-core/tests/screen/mod.rs`, `screen_golden_test.rs`, `binding_test.rs`, `block_def_tests.rs`

**Interfaces:**
- Consumes: `AudioStats` (Task 6), `SiliconRev::label` (Task 5), `AUDIO_BUDGET_PERCENT` (Task 1).
- Produces:
  - `pub static SYS_AUDIO: BlockDef` (id 41, name "Audio", short "AUD", `CellGrid`, `VizType::AudioStats`, slots LOAD, PEAK, OVER, DROPS, DESYNC, STACK); `SYSTEM_BLOCKS[4] = ChainBlock { def: &SYS_ABOUT, sub_pages: &[&SYS_AUDIO] }`.
  - `ui::audio_page::{NONE: &str = "--", cell_texts(Option<&AudioStats>) -> [FmtBuf; 6], draw_cells, draw_focus, draw_viz, cells_key(Option<&AudioStats>) -> [u16; 6], focus_key(Option<&AudioStats>) -> u16, viz_key(Option<&AudioStats>) -> u32}`.
  - `UiState::render_with_audio<D>(&self, display: &mut D, perf: &PerfStats, audio: Option<&AudioStats>, scope: &[f32; SCOPE_LEN])`; `UiState::render_dirty_with_audio<D>(&mut self, display: &mut D, perf: &PerfStats, audio: Option<&AudioStats>, scope: &[f32; SCOPE_LEN]) -> [(u16, u16); MAX_REGIONS]`; `UiState::prime_regions(&mut self, perf: &PerfStats, audio: Option<&AudioStats>, scope: &[f32; SCOPE_LEN])`. `render_with_scope` / `render_dirty_with_scope` keep their signatures and pass `None`.
  - `renderer::Frame { …, pub audio: Option<&'a AudioStats> }`.

The page is a sub-page of About (EDIT from the About node), so the `system` golden (MIDI Setup) is unchanged. The fonts are ASCII-only, so "no measurement" is `--` (the spec's "—" would render as nothing).

- [ ] **Step 1: Write the failing tests and the golden case**

Add to `chimera-core/tests/screen/mod.rs` (imports `use chimera_core::clock_plan::SiliconRev;` and `use chimera_core::perf::load::AudioStats;`):

```rust
pub fn audio_fixture() -> AudioStats {
    let mut s = AudioStats::new(SiliconRev::V, 480_000_000);
    s.load_avg = 23;
    s.load_peak = 41;
    s.overruns = 2;
    s.desyncs = 1;
    s.drops = [0, 3];
    s.sources = 2;
    s.stack_used = 12_000;
    s
}
```

change `render` and `render_dirty` in the same file to draw with the fixture:

```rust
pub fn render(name: &str) -> Fb {
    let ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_with_audio(&mut fb, &PerfStats::zero(), Some(&audio_fixture()), &scope_fixture());
    fb.dump(name);
    fb
}

pub fn render_dirty(name: &str) -> Fb {
    let mut ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_dirty_with_audio(&mut fb, &PerfStats::zero(), Some(&audio_fixture()), &scope_fixture());
    fb
}
```

and append to `CASES`:

```rust
    ("system_audio", |ui| {
        feed(ui, Input::press(ButtonId::Menu));
        plus(ui, 4);
        feed(ui, Input::press(ButtonId::Edit));
    }),
```

In `chimera-core/tests/screen_golden_test.rs` append `("system_audio", 0x0000000000000000),` to `GOLDENS` (recorded in Step 5).

Create `chimera-core/tests/audio_page_test.rs`:

```rust
mod screen;

use chimera_core::clock_plan::SiliconRev;
use chimera_core::ui::UiState;
use chimera_core::ui::audio_page::cell_texts;
use chimera_core::ui::block_registry::SYS_AUDIO;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::ButtonId;
use screen::*;

fn on_audio_page() -> UiState {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::Menu));
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::press(ButtonId::Edit));
    settle(&mut ui);
    ui
}

fn texts(s: Option<&chimera_core::perf::load::AudioStats>) -> Vec<String> {
    cell_texts(s).iter().map(|b| b.as_str().to_string()).collect()
}

#[test]
fn the_page_is_system_about_audio() {
    assert_eq!(on_audio_page().nav.active_block_def().id, SYS_AUDIO.id);
}

#[test]
fn cells_show_the_stats() {
    assert_eq!(texts(Some(&audio_fixture())), ["23%", "41%", "2", "0/3", "1", "12K"]);
}

#[test]
fn without_stats_every_cell_shows_dashes() {
    assert_eq!(texts(None), ["--"; 6]);
}

#[test]
fn a_changed_counter_is_redrawn_and_matches_a_full_render() {
    let mut ui = on_audio_page();
    let (perf, scope) = (PerfStats::zero(), scope_fixture());
    let mut s = audio_fixture();
    let mut fb = Fb::new();
    ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    let idle = ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    assert!(idle.iter().all(|r| r.0 == r.1), "nothing changed, nothing flushed");
    s.overruns += 1;
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, Some(&s), &scope);
    assert!(flushed.iter().any(|r| r.0 != r.1), "the new overrun is drawn");
    let mut full = Fb::new();
    ui.render_with_audio(&mut full, &perf, Some(&s), &scope);
    assert_eq!(fb.hash(), full.hash());
}

#[test]
fn extreme_stats_stay_on_screen() {
    let ui = on_audio_page();
    let mut s = audio_fixture();
    s.load_avg = 250;
    s.load_peak = u16::MAX;
    s.overruns = u32::MAX;
    s.desyncs = u32::MAX;
    s.drops = [u32::MAX; 2];
    s.stack_used = 131_072;
    s.rev = SiliconRev::Unknown(0x2001);
    s.cpu_hz = 400_000_000;
    let mut fb = Fb::new();
    ui.render_with_audio(&mut fb, &PerfStats::zero(), Some(&s), &scope_fixture());
    assert_eq!(fb.oob, 0);
    assert_eq!(texts(Some(&s)), ["250%", "65535%", "4294M", "4294M/4294M", "4294M", "128K"]);
}

#[test]
fn other_pages_ignore_the_stats() {
    let ui = UiState::new();
    let (mut a, mut b) = (Fb::new(), Fb::new());
    ui.render_with_audio(&mut a, &PerfStats::zero(), Some(&audio_fixture()), &scope_fixture());
    ui.render_with_audio(&mut b, &PerfStats::zero(), None, &scope_fixture());
    assert_eq!(a.hash(), b.hash());
}
```

In `chimera-core/tests/binding_test.rs` add `&reg::SYS_AUDIO,` after `&reg::SYS_ABOUT,` in the id-uniqueness list. In `chimera-core/tests/block_def_tests.rs` add:

```rust
#[test]
fn about_has_the_audio_sub_page() {
    let about = &block_registry::SYSTEM_CHAIN.blocks[4];
    assert_eq!(about.def.name, "About");
    assert_eq!(about.sub_pages.len(), 1);
    assert_eq!(about.sub_pages[0].name, "Audio");
    assert_eq!(about.sub_pages[0].id, 41);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p chimera-core --test audio_page_test`
Expected: FAIL to compile: `no audio_page in ui`, `no method render_with_audio`.

- [ ] **Step 3: Registry and viz type**

In `chimera-core/src/ui/block_def.rs` add `AudioStats,` as the last `VizType` variant.

In `chimera-core/src/ui/block_registry.rs`, after `SYS_ABOUT`:

```rust
pub static SYS_AUDIO: BlockDef = BlockDef {
    id: 41,
    name: "Audio",
    short: "AUD",
    layout: PageLayout::CellGrid,
    viz: VizType::AudioStats,
    params: [
        ParamSlot::legacy("LOAD", ValFmt::Int(0)),
        ParamSlot::legacy("PEAK", ValFmt::Int(0)),
        ParamSlot::legacy("OVER", ValFmt::Int(0)),
        ParamSlot::legacy("DROPS", ValFmt::Int(0)),
        ParamSlot::legacy("DESYNC", ValFmt::Int(0)),
        ParamSlot::legacy("STACK", ValFmt::Int(0)),
    ],
};
```

and change the About entry of `SYSTEM_BLOCKS` to `ChainBlock { def: &SYS_ABOUT, sub_pages: &[&SYS_AUDIO] },`. In `chimera-core/src/ui/focus.rs` change `ids are 0..=40 today` to `ids are 0..=41 today`.

- [ ] **Step 4: `audio_page.rs`, the renderer and `UiState`**

Create `chimera-core/src/ui/audio_page.rs`:

```rust
use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::hw::AUDIO_BUDGET_PERCENT;
use crate::perf::load::AudioStats;
use crate::ui::block_def::BlockDef;
use crate::ui::fmt::FmtBuf;
use crate::ui::{components, draw, theme};

pub const NONE: &str = "--";
const TEXT_Y: i32 = 138;
const METER_Y: i32 = 152;
const METER_H: i32 = 4;

// Keeps every counter within its cell's width: 4294967295 reads 4294M.
fn fmt_count(b: &mut FmtBuf, n: u32) {
    let _ = match n {
        0..10_000 => write!(b, "{n}"),
        10_000..10_000_000 => write!(b, "{}K", n / 1_000),
        _ => write!(b, "{}M", n / 1_000_000),
    };
}

pub fn cell_texts(s: Option<&AudioStats>) -> [FmtBuf; 6] {
    core::array::from_fn(|i| {
        let mut b = FmtBuf::new();
        let Some(s) = s else {
            let _ = b.write_str(NONE);
            return b;
        };
        match i {
            0 => {
                let _ = write!(b, "{}%", s.load_avg);
            }
            1 => {
                let _ = write!(b, "{}%", s.load_peak);
            }
            2 => fmt_count(&mut b, s.overruns),
            3 => {
                for (k, &d) in s.drops[..s.sources as usize].iter().enumerate() {
                    if k > 0 {
                        let _ = b.write_str("/");
                    }
                    fmt_count(&mut b, d);
                }
            }
            4 => fmt_count(&mut b, s.desyncs),
            _ => {
                let _ = write!(b, "{}K", s.stack_used.div_ceil(1024));
            }
        }
        b
    })
}

pub fn draw_cells<D>(d: &mut D, def: &BlockDef, s: Option<&AudioStats>, top: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, text) in cell_texts(s).iter().enumerate() {
        let slot = &def.params[i];
        let cell = components::Cell {
            label: slot.label(),
            text: text.as_str(),
            value: 0.0,
            fmt: slot.format(),
            active: false,
            mod_amount: None,
        };
        components::cell(d, i, top, Some(&cell));
    }
}

pub fn draw_focus<D>(d: &mut D, def: &BlockDef, s: Option<&AudioStats>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let texts = cell_texts(s);
    let value = s.map_or(0.0, |s| s.load_avg.min(100) as f32 / 100.0);
    components::focus_band(d, def.params[0].label(), texts[0].as_str(), value, false, None);
}

pub fn draw_viz<D>(d: &mut D, s: Option<&AudioStats>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line = FmtBuf::new();
    let _ = match s {
        Some(s) => write!(line, "REV {}   {} MHZ", s.rev.label(), s.cpu_hz / 1_000_000),
        None => write!(line, "REV {NONE}   {NONE} MHZ"),
    };
    draw::text_tracked(d, &theme::FONT_LABEL, line.as_str(), theme::MARGIN_X, TEXT_Y, theme::MID, theme::LABEL_TRACKING);
    let (x0, w) = (theme::VIZ_LEFT, theme::VIZ_RIGHT - theme::VIZ_LEFT);
    let at = |pct: u16| x0 + (w as u32 * pct.min(100) as u32 / 100) as i32;
    draw::fill_rect(d, x0, METER_Y, w, METER_H, theme::FAINT);
    if let Some(s) = s {
        let fill = match s.load_avg as u32 {
            100.. => theme::ALERT,
            p if p > AUDIO_BUDGET_PERCENT => theme::WARN,
            _ => theme::ACCENT,
        };
        draw::fill_rect(d, x0, METER_Y, at(s.load_avg) - x0, METER_H, fill);
        draw::fill_rect(d, at(s.load_peak).min(theme::VIZ_RIGHT - 1), METER_Y - 3, 1, METER_H + 6, theme::INK2);
    }
    let budget_x = at(AUDIO_BUDGET_PERCENT as u16);
    draw::fill_rect(d, budget_x, METER_Y - 6, 1, METER_H + 12, theme::MID);
    line.clear();
    let _ = write!(line, "{AUDIO_BUDGET_PERCENT}%");
    draw::text_center(d, &theme::FONT_LABEL, line.as_str(), budget_x, METER_Y + METER_H + 16, theme::MID, 0);
}

fn fold(v: u32) -> u16 {
    (v ^ (v >> 16)) as u16
}

pub fn cells_key(s: Option<&AudioStats>) -> [u16; 6] {
    match s {
        None => [u16::MAX - 1; 6],
        Some(s) => [
            s.load_avg,
            s.load_peak,
            fold(s.overruns),
            fold(s.drops.iter().fold(0u32, |a, &d| a.rotate_left(7) ^ d)),
            fold(s.desyncs),
            fold(s.stack_used.div_ceil(1024)),
        ],
    }
}

pub fn focus_key(s: Option<&AudioStats>) -> u16 {
    s.map_or(u16::MAX - 1, |s| s.load_avg)
}

pub fn viz_key(s: Option<&AudioStats>) -> u32 {
    s.map_or(u32::MAX - 1, |s| {
        [s.rev.label().as_bytes()[0] as u32, s.cpu_hz / 1_000_000, s.load_avg as u32, s.load_peak as u32]
            .iter()
            .fold(0x811c_9dc5u32, |h, &v| (h ^ v).wrapping_mul(0x0100_0193))
    })
}
```

In `chimera-core/src/ui/renderer.rs`:
- add `use crate::perf::load::AudioStats;` and `use crate::ui::audio_page;`
- add a field to `Frame` after `prime_status`: `pub audio: Option<&'a AudioStats>,`
- at the top of `draw_band_viz`'s `match f.def.viz`, add the arm `VizType::AudioStats => audio_page::draw_viz(display, f.audio),`
- in `viz_inputs`, inside the `PageLayout::CellGrid => match f.def.viz {` block add the arm `VizType::AudioStats => ([0; 6], audio_page::viz_key(f.audio)),`
- at the start of `draw_focus`, after the Matrix early return, add:

```rust
        if f.def.viz == VizType::AudioStats {
            return audio_page::draw_focus(display, f.def, f.audio);
        }
```

- at the start of `draw_cells` add:

```rust
        if f.def.viz == VizType::AudioStats {
            return audio_page::draw_cells(display, f.def, f.audio, top);
        }
```

In `chimera-core/src/ui/mod.rs`:
- add `pub mod audio_page;` to the module list and `use crate::perf::load::AudioStats;` and `use block_def::VizType;`
- replace `render_with_scope`'s body with `self.render_with_audio(display, perf, None, scope);` and add after it:

```rust
    pub fn render_with_audio<D>(&self, display: &mut D, perf: &PerfStats, audio: Option<&AudioStats>, scope: &[f32; SCOPE_LEN])
    where
        D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
    {
        if let UiMode::SoundBrowser { part, cursor, scroll } = self.ui_mode {
            browser::draw(display, &self.pool, part, cursor, scroll);
            return;
        }
        self.renderer.draw_with_def(display, &self.frame(perf, audio, scope));
    }
```

- change `fn frame<'a>(&'a self, perf: &'a PerfStats, scope: &'a [f32; SCOPE_LEN])` to take `audio: Option<&'a AudioStats>` between `perf` and `scope`, and add `audio,` to the `renderer::Frame { … }` literal;
- in `region_data` compute `let audio_page = f.def.viz == VizType::AudioStats;` at the top, and change the `RegionKind::Focus => RegionData::focus(…)` arm's value argument to `if audio_page { audio_page::focus_key(f.audio) } else { qvalues[f.focus] }` and the `RegionKind::Cells => RegionData::cells(…)` arm's values argument to `if audio_page { audio_page::cells_key(f.audio) } else { qvalues }`;
- change `prime_regions` to `pub fn prime_regions(&mut self, perf: &PerfStats, audio: Option<&AudioStats>, scope: &[f32; SCOPE_LEN])` and its `self.frame(perf, scope)` to `self.frame(perf, audio, scope)`;
- rename `render_dirty_with_scope` to `render_dirty_with_audio`, add the `audio: Option<&AudioStats>` parameter after `perf`, change its `self.frame(perf, scope)` to `self.frame(perf, audio, scope)`, and add back:

```rust
    pub fn render_dirty_with_scope<D>(&mut self, display: &mut D, perf: &PerfStats, scope: &[f32; SCOPE_LEN]) -> [(u16, u16); region::MAX_REGIONS]
    where
        D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>
            + chimera_hal::ChimeraDisplay,
    {
        self.render_dirty_with_audio(display, perf, None, scope)
    }
```

In `chimera-stm32/src/main.rs` change `ui.prime_regions(&perf.stats, scope_r.read());` to `ui.prime_regions(&perf.stats, None, scope_r.read());`.

- [ ] **Step 5: Run, then record the one new golden**

Run: `cargo test -p chimera-core --test audio_page_test --test block_def_tests --test binding_test`
Expected: PASS.

Run: `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture`
Expected: 13 rows printed; the first twelve identical to the existing `GOLDENS` (nothing else changed). Paste the printed `("system_audio", 0x…)` row over the placeholder, then look at it: `SCREEN_DUMP=$(pwd)/target/screens cargo test -p chimera-core --test screen_golden_test` and open `target/screens/system_audio.ppm` — header `SYSTEM AUDIO`, focus `LOAD 23%` with its arc, viz `REV V   480 MHZ` over a meter with a peak tick and a `70%` budget mark, cells `23% 41% 2 / 0/3 1 12K`.

Run: `cargo test -p chimera-core --test screen_golden_test`
Expected: PASS.

- [ ] **Step 6: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS (the all-pages walk visits the new sub-page; dirty equals full).

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/ui/audio_page.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs \
  chimera-core/src/ui/block_def.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/focus.rs \
  chimera-core/tests/audio_page_test.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs \
  chimera-core/tests/binding_test.rs chimera-core/tests/block_def_tests.rs chimera-stm32/src/main.rs
git commit -m "AUDIO sub-page under System > About"
```

---

### Task 18: Bring-up step 6 — the CPU probe, the AUDIO page on the chip, the bench

**Files:**
- Create: `chimera-stm32/src/probe.rs`, `chimera-stm32/src/bench.rs`
- Modify: `chimera-stm32/Cargo.toml` (`perf-probe`, `bench`), `chimera-stm32/src/audio/dma.rs` (render through the probe), `chimera-stm32/src/main.rs` (paint the stack, stats reader, AUDIO page, bench, splash removed)

**Interfaces:**
- Consumes: `AudioStats`, `STACK_PAINT`, `untouched_words` (Task 6); `BlockBudget` (Task 1); `TripleBuffer` (Task 2); `engine::{NOTES, DIN, slots}`, `dma::{OVERRUNS, DESYNCS}`, `midi_din::ERRORS` (Tasks 14–15); `render_*_with_audio`, `prime_regions` (Task 17).
- Produces (firmware): `probe::{paint_stack(), stack_used() -> u32, init(&mut DCB, &mut DWT, Clocks) -> Option<Reader<AudioStats>>, enable_cycle_counter(&mut DCB, &mut DWT) -> bool, measure(impl FnOnce())}` (a zero-cost stub without `perf-probe`); `bench::run(&mut impl ChimeraDisplay, Clocks)` (feature `bench`: shows the table for 30 s, then returns and the instrument boots).

- [ ] **Step 1: Features**

In `chimera-stm32/Cargo.toml` replace the `[features]` table with:

```toml
[features]
default = ["midi-din", "perf-probe"]
midi-din = []
perf-probe = []
bench = ["perf-probe"]
```

- [ ] **Step 2: `probe.rs`**

Create `chimera-stm32/src/probe.rs`:

```rust
#[cfg(feature = "perf-probe")]
pub use imp::*;
#[cfg(not(feature = "perf-probe"))]
pub use stub::*;

#[cfg(feature = "perf-probe")]
mod imp {
    use core::ptr::{addr_of, addr_of_mut};
    use core::sync::atomic::{AtomicBool, Ordering};

    use chimera_core::clock_plan::SiliconRev;
    use chimera_core::hw::BlockBudget;
    use chimera_core::perf::load::AudioStats;
    use chimera_core::perf::stack::{STACK_PAINT, untouched_words};
    use chimera_core::triple::{Reader, TripleBuffer, Writer};
    use cortex_m::peripheral::{DCB, DWT};

    use crate::audio::{dma, engine};
    use crate::clocks::Clocks;

    const BLANK: AudioStats = AudioStats::new(SiliconRev::Unknown(0), 0);
    const PAINT_MARGIN: usize = 256;

    static mut STATS_BUF: TripleBuffer<AudioStats> = TripleBuffer::new(BLANK, BLANK, BLANK);
    static mut STATS: AudioStats = BLANK;
    static mut WRITER: Option<Writer<AudioStats>> = None;
    static mut BUDGET: BlockBudget = BlockBudget::for_cpu(chimera_core::hw::CPU_HZ_REV_Y);
    static READY: AtomicBool = AtomicBool::new(false);
    static TAKEN: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        static _stack_start: u32;
        static _stack_end: u32;
    }

    pub fn paint_stack() {
        let bottom = (&raw const _stack_end) as *mut u32;
        let limit = cortex_m::register::msp::read() as usize - PAINT_MARGIN;
        let mut p = bottom;
        while (p as usize) < limit {
            // SAFETY: [_stack_end, SP − margin) is DTCM stack below every live
            // frame (main's included), and nothing else runs yet.
            unsafe {
                p.write_volatile(STACK_PAINT);
                p = p.add(1);
            }
        }
    }

    pub fn stack_used() -> u32 {
        let bottom = (&raw const _stack_end) as *const u32;
        let words = ((&raw const _stack_start) as usize - bottom as usize) / 4;
        // SAFETY: volatile reads inside the linker's stack region; a read that
        // races an interrupt's frame only moves the mark by that word.
        let untouched = untouched_words((0..words).map(|i| unsafe { bottom.add(i).read_volatile() }));
        ((words - untouched) * 4) as u32
    }

    pub fn enable_cycle_counter(dcb: &mut DCB, dwt: &mut DWT) -> bool {
        dcb.enable_trace();
        dwt.enable_cycle_counter();
        if counting() {
            return true;
        }
        // Without a debugger the M7's DWT can come up software-locked.
        DWT::unlock();
        dwt.enable_cycle_counter();
        counting()
    }

    fn counting() -> bool {
        let start = DWT::cycle_count();
        cortex_m::asm::delay(1_000);
        DWT::cycle_count() != start
    }

    pub fn init(dcb: &mut DCB, dwt: &mut DWT, clocks: Clocks) -> Option<Reader<AudioStats>> {
        if !enable_cycle_counter(dcb, dwt) || TAKEN.swap(true, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: the flag lets one caller past, before the audio interrupt is
        // unmasked, so nothing else touches these statics yet.
        unsafe {
            *addr_of_mut!(STATS) = AudioStats::new(clocks.rev, clocks.cpu_hz);
            *addr_of_mut!(BUDGET) = BlockBudget::for_cpu(clocks.cpu_hz);
            let (w, r) = (&mut *addr_of_mut!(STATS_BUF)).split();
            *addr_of_mut!(WRITER) = Some(w);
            READY.store(true, Ordering::Release);
            Some(r)
        }
    }

    pub fn measure(render: impl FnOnce()) {
        if !READY.load(Ordering::Acquire) {
            return render();
        }
        let start = DWT::cycle_count();
        render();
        let cycles = DWT::cycle_count().wrapping_sub(start);
        // SAFETY: after `init`, only the audio interrupt calls `measure`, and
        // it does not re-enter.
        let (stats, budget, writer) =
            unsafe { (&mut *addr_of_mut!(STATS), *addr_of!(BUDGET), (*addr_of_mut!(WRITER)).as_mut()) };
        stats.record(cycles, budget);
        stats.overruns = dma::OVERRUNS.load(Ordering::Relaxed);
        stats.desyncs = dma::DESYNCS.load(Ordering::Relaxed);
        let drops = engine::NOTES.drops();
        stats.drops[..drops.len()].copy_from_slice(&drops);
        #[cfg(feature = "midi-din")]
        {
            stats.drops[engine::DIN.index()] += crate::midi_din::ERRORS.load(Ordering::Relaxed);
        }
        stats.sources = drops.len() as u8;
        if let Some(w) = writer {
            let s = *stats;
            w.publish(|out| *out = s);
        }
    }
}

#[cfg(not(feature = "perf-probe"))]
mod stub {
    use chimera_core::perf::load::AudioStats;
    use chimera_core::triple::Reader;
    use cortex_m::peripheral::{DCB, DWT};

    use crate::clocks::Clocks;

    pub fn paint_stack() {}

    pub fn stack_used() -> u32 {
        0
    }

    pub fn init(_: &mut DCB, _: &mut DWT, _: Clocks) -> Option<Reader<AudioStats>> {
        None
    }

    pub fn measure(render: impl FnOnce()) {
        render()
    }
}
```

- [ ] **Step 3: Time each render in the DMA handler**

In `chimera-stm32/src/audio/dma.rs` change the render loop in `DMA1_STR0` to:

```rust
    for half in plan.halves.into_iter().flatten() {
        crate::probe::measure(|| super::render_half(half));
    }
```

- [ ] **Step 4: `bench.rs`**

Create `chimera-stm32/src/bench.rs`:

```rust
use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, DAC_PAIRS, MAX_VOICES, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::scope::{ScopeFrame, ScopeWriter, scope_buffer};
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::ChimeraDisplay;
use cortex_m::peripheral::DWT;

use crate::audio::engine;
use crate::clocks::Clocks;

const WARM_BLOCKS: u32 = 8;
const TIMED_BLOCKS: u32 = 64;
const REVERB_TYPES: usize = 3;
const HOLD_SECONDS: u32 = 30;

static mut SCOPE: TripleBuffer<ScopeFrame> = scope_buffer();

struct Rig {
    inst_slot: &'static mut MaybeUninit<Instrument>,
    fx_slot: &'static mut MaybeUninit<FxBus>,
    scope: ScopeWriter,
    dac: DacOut,
}

pub fn run(display: &mut impl ChimeraDisplay, clocks: Clocks) {
    // SAFETY: the bench runs once from `main`, before `engine::init` and
    // before any audio interrupt is unmasked, so it is the only user of the
    // engine's slots and of its own scope buffer; its references are gone
    // when it returns, before `engine::init` takes the slots.
    let (inst_slot, fx_slot, scope_w) = unsafe {
        let (i, f) = engine::slots();
        let (w, _unread) = (&mut *addr_of_mut!(SCOPE)).split();
        (i, f, w)
    };
    let mut rig = Rig { inst_slot, fx_slot, scope: ScopeWriter::new(scope_w), dac: [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS] };
    let mut voices = [[0u32; MAX_VOICES]; EngineType::ALL.len()];
    for (row, &engine) in EngineType::ALL.iter().enumerate() {
        for n in 1..=MAX_VOICES {
            voices[row][n - 1] = rig.time(|s| s.parts[0].params = ParamSnapshot::for_engine(engine), n);
        }
    }
    let fx: [u32; REVERB_TYPES] = core::array::from_fn(|t| {
        rig.time(
            |s| {
                s.fx.chorus.mode = 1;
                s.fx.chorus.mix = 0.5;
                s.fx.delay.mix = 0.5;
                s.fx.reverb.mix = 0.5;
                s.fx.reverb.reverb_type = t as u8;
            },
            0,
        )
    });
    show(display, clocks, &voices, &fx);
    // Long enough to photograph the table; then the instrument boots as usual.
    for _ in 0..HOLD_SECONDS {
        crate::clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
}

impl Rig {
    fn time(&mut self, setup: impl FnOnce(&mut AudioShared), voices: usize) -> u32 {
        // The allocator must not refuse what the bench wants to measure.
        let budget = SampleBudget::for_cpu(u32::MAX);
        let inst = Instrument::init_in_place(self.inst_slot, SAMPLE_RATE, budget);
        let fx = FxBus::init_in_place(self.fx_slot);
        let mut shared = AudioShared::default();
        setup(&mut shared);
        for v in 0..voices {
            let note = MidiNote::new(48 + 5 * v as u8).unwrap_or(MidiNote::A4);
            let ev = NoteEvent { channel: MidiChannel::clamped(0), note, kind: NoteKind::On(Velocity::DEFAULT) };
            inst.handle(ev, &shared);
        }
        for _ in 0..WARM_BLOCKS {
            inst.render(fx, &mut self.dac, &shared, &mut self.scope);
        }
        let start = DWT::cycle_count();
        for _ in 0..TIMED_BLOCKS {
            inst.render(fx, &mut self.dac, &shared, &mut self.scope);
        }
        DWT::cycle_count().wrapping_sub(start) / (TIMED_BLOCKS * BLOCK_SIZE as u32)
    }
}

fn name(e: EngineType) -> &'static str {
    match e {
        EngineType::Pizza => "PIZZA",
        EngineType::Fm => "FM",
        EngineType::Modal => "MODAL",
        EngineType::Va => "VA",
    }
}

fn show(display: &mut impl ChimeraDisplay, clocks: Clocks, voices: &[[u32; MAX_VOICES]; 4], fx: &[u32; REVERB_TYPES]) {
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let _ = write!(line, "BENCH REV {} {} MHZ", clocks.rev.label(), clocks.cpu_hz / 1_000_000);
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, 20, theme::INK);
    draw::text(display, &theme::FONT_LABEL, "CYCLES/SAMPLE, 1..6 VOICES", 4, 36, theme::MID);
    for (row, (&engine, cycles)) in EngineType::ALL.iter().zip(voices).enumerate() {
        let y = 58 + row as i32 * 30;
        let per_voice = cycles[MAX_VOICES - 1].saturating_sub(cycles[0]) / (MAX_VOICES as u32 - 1);
        line.clear();
        let _ = write!(line, "{} /VOICE {}", name(engine), per_voice);
        draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, y, theme::INK);
        line.clear();
        for c in cycles {
            let _ = write!(line, "{c} ");
        }
        draw::text(display, &theme::FONT_LABEL, line.as_str(), 4, y + 13, theme::INK2);
    }
    line.clear();
    let _ = write!(line, "FX PLATE {} FDN {} MV {}", fx[0], fx[1], fx[2]);
    draw::text(display, &theme::FONT_VALUE, line.as_str(), 4, 58 + 4 * 30, theme::INK);
    display.flush();
}
```

- [ ] **Step 5: `main.rs` — stack paint, probe, AUDIO page, bench; splash removed**

In `chimera-stm32/src/main.rs`:
- add `mod probe;` and `#[cfg(feature = "bench")] mod bench;`
- make `probe::paint_stack();` the first line of `main`;
- delete the `boot_splash` function, its call, and the imports only it used (`SiliconRev`, `FmtBuf`, `draw`, `theme`);
- right after `display.init(clk.cpu_hz);` add:

```rust
    let mut stats_r = probe::init(&mut cp.DCB, &mut cp.DWT, clk);
    #[cfg(feature = "bench")]
    bench::run(&mut display, clk);
```

- replace the initial render and prime lines with:

```rust
    ui.render_with_audio(&mut display, &perf.stats, None, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, None, scope_r.read());
```

- in the loop, replace the `render_dirty_with_scope` line with:

```rust
        let stats = stats_r.as_mut().map(|r| {
            let mut s = *r.read();
            s.stack_used = probe::stack_used();
            s
        });
        let flush_list = ui.render_dirty_with_audio(&mut display, &perf.stats, stats.as_ref(), scope_r.read());
```

(`use chimera_core::perf::load::AudioStats;` is not needed: the type is inferred.)

- [ ] **Step 6: Build every feature set and run the full check**

Run:

```bash
cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features
cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --features bench
PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check
```

Expected: all PASS.

- [ ] **Step 7: On-device checks (bring-up step 6)**

Run `just flash`, DIN keyboard connected:
- [ ] MENU, PLUS ×4 (About), EDIT: the AUDIO page shows `REV V   480 MHZ` (or Y/400), LOAD and PEAK in %, OVER 0, DROPS 0, DESYNC 0, STACK in K. Idle LOAD is a few %.
- [ ] Playing six voices raises LOAD and moves the meter; PEAK holds the maximum. Record idle, 6-voice Pizza and 6-voice Modal (with all FX on) loads in the PR.
- [ ] OVER stays 0 in normal play; DESYNC stays 0.
- [ ] STACK stays well under 128K (record the value).
- [ ] If LOAD and PEAK read `--` on the device, the DWT counter never counted even after the LAR unlock: note it in an issue; everything else still works.
- [ ] Bench: flash the bench build (`cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench`, then `rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-bench.bin` and `dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-bench.bin -s 0x8020000:leave`); the table appears with non-zero numbers for 30 s, then the instrument boots and plays. Reflash the default build with `just flash`.

- [ ] **Step 8: Commit**

```bash
git add chimera-stm32/src/probe.rs chimera-stm32/src/bench.rs chimera-stm32/src/audio/dma.rs \
  chimera-stm32/src/main.rs chimera-stm32/Cargo.toml
git commit -m "CPU probe, AUDIO page on the chip, per-engine bench"
```

---

### Task 19: `Justfile` build variants and firmware clippy

**Files:**
- Modify: `Justfile`

**Interfaces:**
- Consumes: the firmware features `midi-din`, `perf-probe`, `bench` (Tasks 15, 18) and the desktop feature `midi` (Task 11).
- Produces: `just check` builds and links the firmware with default features, `--no-default-features` and `--features bench`; builds the desktop with `--no-default-features`; runs clippy on the firmware target for all three feature sets. New recipe `just flash-bench`.

- [ ] **Step 1: Extend `check`, `clippy`, add `flash-bench`**

In `Justfile` replace the `check` recipe's body with:

```just
check:
    cargo test -p chimera-core -p chimera-hal
    cargo test -p chimera-desktop
    cargo build -p chimera-desktop --no-default-features
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features
    cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --features bench
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop --all-targets -- -D warnings
    cargo clippy -p chimera-desktop --no-default-features --all-targets -- -D warnings
    cargo clippy -p chimera-stm32 --target thumbv7em-none-eabihf -- -D warnings
    cargo clippy -p chimera-stm32 --target thumbv7em-none-eabihf --no-default-features -- -D warnings
    cargo clippy -p chimera-stm32 --target thumbv7em-none-eabihf --features bench -- -D warnings
    cargo fmt --all -- --check
```

Replace the `clippy` recipe's body with:

```just
clippy:
    cargo clippy -p chimera-core -p chimera-hal -p chimera-desktop --all-targets -- -D warnings
    cargo clippy -p chimera-stm32 --target thumbv7em-none-eabihf -- -D warnings
```

Append:

```just
flash-bench:
    cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features bench
    rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 target/chimera-bench.bin
    dfu-util -a0 -d 0x0483:0xdf11 -D target/chimera-bench.bin -s 0x8020000:leave
```

Replace the comment above `check` with:

```just
# Everything must pass before a commit (ADR 0013): core + hal tests, desktop
# tests and its no-MIDI build, the firmware built and linked with default
# features, with none, and with the bench, clippy on host and firmware (every
# feature set) and rustfmt. The desktop needs ALSA's pkg-config file; point
# PKG_CONFIG_PATH at it if it is not installed system-wide.
```

- [ ] **Step 2: Run the full check**

Run: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS, including the three firmware clippy runs with zero warnings. (The one pre-existing firmware lint, `needless_range_loop` in `controls.rs`, was fixed in Task 12.)

- [ ] **Step 3: Commit**

```bash
git add Justfile
git commit -m "just check builds every firmware feature set and runs firmware clippy"
```

---

### Task 20: Measured `Cost` values from the bench

**Files:**
- Modify: `chimera-core/src/dsp/pizza.rs` (`PizzaOsc::COST`), `dsp/engine_fm.rs` (`FmEngine::COST`), `dsp/modal.rs` (`ModalEngine::COST`), `dsp/voice.rs` (`Voice::CHAIN_COST`), `dsp/fx_bus.rs` (`FxBus::COST`)
- Test: `chimera-core/tests/cost_test.rs`, and any assertion that derives from those costs (`instrument_test.rs::sound_change_mid_chord_stays_in_budget`)

**Interfaces:**
- Consumes: the bench table (Task 18) from real hardware.
- Produces: measured per-engine `Cost`s (ADR 0013: "replaced by measurements"). `VA_COST` stays an estimate: the VA engine is a silent placeholder.

This is a manual-data step: the numbers come from the device.

- [ ] **Step 1: Run the bench**

Put the PreenFM3 in DFU mode and run `just flash-bench`. For 30 s after boot the screen shows `BENCH REV x N MHZ`, one row per engine (`/VOICE` slope, then cycles per sample for 1–6 voices) and `FX PLATE a FDN b MV c`. Photograph it; transcribe every number into the PR description with the revision and MHz. Reflash the default build with `just flash` afterwards.

- [ ] **Step 2: Derive the costs**

Round each up to the next multiple of 10:
- `Voice::CHAIN_COST` = VA's `/VOICE` (the VA engine renders nothing, so its voice cost is the chain).
- `PizzaOsc::COST` = PIZZA `/VOICE` − `CHAIN_COST`; `FmEngine::COST` = FM `/VOICE` − `CHAIN_COST`; `ModalEngine::COST` = MODAL `/VOICE` − `CHAIN_COST`.
- `FxBus::COST` = the largest of PLATE, FDN, MV (zero voices, every effect on: the bus plus the Instrument's fixed mixing).

- [ ] **Step 3: Write them in**

In each file replace the value and the `// estimate` suffix, for example in `chimera-core/src/dsp/fx_bus.rs`:

```rust
    pub const COST: Cost = Cost(NNN); // measured 2026-MM-DD, bench, rev V at 480 MHz
```

(`NNN` and the date are the values from Steps 1–2; same pattern for the other four constants.)

- [ ] **Step 4: Update the cost-derived test expectations**

In `chimera-core/tests/cost_test.rs`:
- `voice_costs_follow_the_design_table`: rename to `voice_costs_are_the_bench_measurements`, and set the five expected `Cost(…)` values to Step 2's (the `Engines::cost(e) + Voice::CHAIN_COST` loop stays).
- `budget_capacity_per_engine`: for each engine compute `k = min(6, (7000 − FxBus::COST) / Voice::cost(e))` with the new numbers and assert exactly that capacity: `assert!(fits(e, k))` and, when `k < 6`, `assert!(!fits(e, k + 1))`.

In `chimera-core/tests/instrument_test.rs::sound_change_mid_chord_stays_in_budget`, set the expected number of surviving Modal voices to `min(6, (7000 − FxBus::COST) / Voice::cost(Modal))` with the new numbers.

- [ ] **Step 5: Run the allocator tests and the full check**

Run: `cargo test -p chimera-core --test cost_test --test voice_alloc_test --test instrument_test`, then `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`
Expected: PASS; the audio goldens don't move (costs only change which notes are refused over budget).

- [ ] **Step 6: If six voices plus FX don't fit**

If Step 4 shows any engine below six voices at 480 MHz (or the 400 MHz capacity matters for this board's revision), open a GitHub issue in joegiralt/chimera with the bench table and the capacities; any change to `MAX_VOICES` is decided there (spec § Risks), not in this commit.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/dsp/pizza.rs chimera-core/src/dsp/engine_fm.rs chimera-core/src/dsp/modal.rs \
  chimera-core/src/dsp/voice.rs chimera-core/src/dsp/fx_bus.rs chimera-core/tests/cost_test.rs \
  chimera-core/tests/instrument_test.rs
git commit -m "Measured engine and FX costs from the bench"
```
