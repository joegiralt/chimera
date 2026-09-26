# Instrument on the Chip — Design

**Date:** 2026-09-26
**Status:** Draft (rev 2, after adversarial review), awaiting user review
**Phase:** Hardware milestone, sub-project 1 of 4:
1. **instrument on the chip** (this spec);
2. full MIDI and expression (USB MIDI, bend, mod wheel, sustain, aftertouch, MPE, glide/legato, MIDI learn);
3. storage (SD, Sound banks, Performances, program change);
4. tempo (MIDI clock, internal BPM, LFO/delay sync, tempo-locked reverb #5).

**Builds on:** `instrument-core` (desktop `Instrument`, `AudioShared`, `NoteQueue`), ADR 0013 (hardware parity), ADR 0014 (audio memory map).
**Hardware reference:** the stock PreenFM3 firmware (Ixox/preenfm3), which runs on the same board. It is cited where it confirms pins, clocks and SAI setup.

## Intent

Make the PreenFM3 play what the desktop plays:
- six Parts on their own MIDI channels, sharing the 6-voice pool;
- level, pan and FX sends;
- each Part on one of the three DAC pairs.

It plays from a DIN MIDI keyboard, and the desktop takes real MIDI input the same way.

**Done when:**
- you can play all six Parts from a keyboard over DIN, on three outputs, in tune, with the chip's measured CPU load visible on the synth;
- the desktop plays the same from a USB MIDI keyboard.

## Principles (binding for the plan)

- **Functional core, imperative shell.** Algorithms are pure functions over plain data in `chimera-core`/`chimera-hal`, tested on the host: MIDI parsing, sample conversion, interleaving, clock planning, and load arithmetic. `chimera-stm32` and `chimera-desktop` are thin shells that own the peripherals, interrupts and statics.
- **Type-driven** (ADR 0012), for example:
  - `MidiChannel` in parsed messages;
  - `DacSample`;
  - `SiliconRev`;
  - `Priority`;
  - `SampleBudget`/`BlockBudget`;
  - the `Writer`/`Reader` halves of a take-once triple buffer.
- **Cuttable.** One module per concern. The optional peripheral features `midi-din` and `perf-probe`, plus the off-by-default `bench`, sit behind Cargo features. Cutting one means deleting its module and its flag.

## Scope

In:
- clocks chosen by silicon revision;
- 48 kHz (±10 ppm), 24-bit output;
- three SAI blocks on one clock, with DMA;
- caches and the MPU;
- the stack moved to DTCM;
- `Instrument` and `FxBus` built in place;
- triple-buffered `AudioShared` and scope;
- DIN MIDI note on/off;
- a safe panic/fault stop;
- the CPU probe, the AUDIO page, and the bench;
- desktop MIDI in through midir, with the existing desktop race fixed.

Out, for later sub-projects: USB MIDI, CC/bend/pressure/program change, MPE, glide, MIDI learn, storage, clock sync and MIDI thru.

## Module layout

### Core (pure, host-tested)

| Module | Contents |
|---|---|
| `chimera-hal/src/midi.rs` | The existing `MidiParser`. Its system-common and SysEx handling already exists (`midi.rs:30-38`, `62-96`). The change is `MidiMessage` channels becoming `MidiChannel`, plus a test table that locks the behaviour. |
| `chimera-core/src/note_queue.rs` | Unchanged `NoteQueue`. Adds `NoteSources<const N: usize>`: one queue per source, each with exactly one producer context; `drain(f)` pops every queue in a fixed order. |
| `chimera-core/src/audio_out.rs` | `DacSample(i32)`: a 24-bit value left-justified in 32 bits, the low 8 bits zero. `to_dac(f32) -> DacSample` clamps to ±1.0. `interleave(&DacOut, DacPair, &mut [DacSample; BLOCK_SIZE * 2])`. |
| `chimera-core/src/clock_plan.rs` | `const fn pll3_for(hse_hz, fs_hz, SiliconRev) -> Pll3Config`, holding M, N, FRACN, P, PLL3RGE, VCOSEL and MCKDIV. It applies the revision's MCKDIV formula: rev V (OSR=0) gives FS = ker / (MCKDIV × 256), and rev Y gives MCLK = ker / (2 × MCKDIV) with FS = MCLK / 256. `fs_of(&Pll3Config, hse_hz, SiliconRev) -> f64`. |
| `chimera-core/src/triple.rs` | `TripleBuffer<T>`, a lock-free single-writer, single-reader triple buffer. `split(&'static mut self) -> (Writer<T>, Reader<T>)`, callable once. `Writer::publish(\|&mut T\|)` and `Reader::read() -> &T`, where `read` returns the newest published buffer and keeps it until the next `read`. Both halves are `Send`. |
| `chimera-core/src/perf/load.rs` | `AudioStats { load_avg, load_peak, overruns, desyncs, drops: [u32; N], stack_free, rev, cpu_mhz }` and `AudioStats::record(cycles, BlockBudget)`. Load is a percentage of the block's total cycles, and the 70 % budget is shown as a reference line. |
| `chimera-core/src/hw.rs` | `SampleBudget(u32)`: cycles per sample, 70 % of `cpu_hz / 48 kHz`, used by the allocator. `BlockBudget(u32)`: cycles per 64-sample block, used by the probe. Both are built only from a `cpu_hz` via `for_cpu`; there is no `Default`, so the wrong budget can't be picked silently. |

`Allocator::new(budget: SampleBudget)` and `Instrument::new(sample_rate, SampleBudget)` take the budget explicitly. The desktop passes `SampleBudget::for_cpu(480 MHz)`. Existing tests that use `AUDIO_CYCLE_BUDGET` (`hw_test.rs:10`, `cost_test.rs:32`, `voice_alloc_test.rs:248`, `instrument_test.rs:353`) move to the new constructors, with their assertions unchanged.

### Firmware shell (`chimera-stm32/src/`)

| Module | Owns |
|---|---|
| `clocks.rs` | `SiliconRev::{V, Y, Unknown}` from DBGMCU IDCODE REV_ID (0x2003 is V, 0x1003 is Y). V runs at 480 MHz with VOS0 (stm32h7xx-hal feature `revision_v`); Y and Unknown run at 400 MHz. PLL3 comes from `pll3_for(8 MHz, 48 kHz, rev)`. Returns `Clocks { cpu_hz, rev }`. |
| `cache.rs` | Enables the D2 SRAM1/2/3 clocks (ADR 0014), then the I- and D-cache, with an MPU region marking the DMA section non-cacheable. |
| `audio/sai.rs` | SAI1 A master with GCR SYNCOUT = block A; SAI1 B an internal sync slave; SAI2 A externally synced (SYNCEN=10, SAI2 GCR SYNCIN=00). Also the SAI2 RCC enable and SAI23SEL = PLL3_P. Data size 32 bits, I2S, MCLK 256×. Pins: PE2/4/5/6 and PE3 on AF6, **PD11 on AF10**. The MCKEN bit is set only on rev V. |
| `audio/dma.rs` | DMA1 streams 0, 1 and 2, circular, DMAMUX request IDs 87/88/89 (SAI1 A, SAI1 B, SAI2 A). Only stream 0 interrupts. There's a desync check with a ±16-word tolerance around the half boundary. |
| `audio/engine.rs` | The `Instrument` (D2) and `FxBus` (AXI) statics, `render_half(half)` and the overrun policy. |
| `midi_din.rs` | *(feature `midi-din`)* USART1 RX on PB7 at 31,250 baud. FIFOEN is set before UE, with the RX-FIFO-not-empty interrupt. The interrupt drains the FIFO into the parser and pushes `NoteEvent`s to the DIN queue, and clears the ORE, FE and NE flags every time, so an error can never make it refire forever. |
| `shared.rs` | Builds the `AudioShared` and scope `TripleBuffer` statics in AXI. Hands the `Writer`s to `main` and the `Reader`s to the engine and UI. |
| `priority.rs` | A `Priority` newtype that encodes the upper nibble (the H7 has 4 priority bits): `AUDIO = 0x00`, `MIDI = 0x40`, `SYSTICK = 0xF0`, the last applied through `SCB::set_priority`. |
| `panic.rs` | The `#[panic_handler]` and a HardFault handler. Both stop SAI1 A/B and SAI2 A, then halt, so a crash is silent instead of a 750 Hz buzz from the looping DMA. Replaces `panic-halt`. |
| `probe.rs` | *(feature `perf-probe`)* Turns on DWT (DCB trace enable, `enable_cycle_counter`, and a DWT_LAR unlock if CYCCNT stays at 0). Wraps `render_half` and feeds `AudioStats`. With the feature off, a zero-cost stub. |
| `bench.rs` | *(feature `bench`, off by default)* The per-engine cycle benchmark. |

### Desktop shell

- midir becomes an **optional** dependency behind the `midi` feature (default on). Today it is an unused, unconditional dependency.
- The midir connection object is kept alive for the app's lifetime. Its callback owns its own `MidiParser` and is the single producer for `Source::Midi`. The computer keyboard is the single producer for `Source::Keys`. The two sources are ordered by the fixed drain order, since there is no cross-source timestamp.
- The desktop moves from its current two-buffer `AudioShared` swap to `TripleBuffer`. This fixes an existing data race: the cpal callback holds `&A` for a whole callback while a second `update` can write A (`chimera-desktop/src/audio.rs:66-76`).
- With the `midi` feature off, only `Source::Keys` exists.

## Audio output

**Clocks and side effects.** `clocks.rs` sets the CPU to 480 MHz (rev V) or 400 MHz (rev Y or Unknown), with HCLK at half that. Clocks derived from it shift, and are computed from the real values rather than assumed:
- `pll1_q`, the display SPI kernel clock, becomes 192 MHz at 480 MHz (the HAL's PLL1 strategy rounds its divider up), so the display SPI runs at 48 MHz instead of 50;
- pclk2, the USART1 kernel clock, becomes 120 MHz; the baud divider is computed from it.

Timing that scales with the CPU clock is fixed:
- SysTick reload comes from the real `cpu_hz`. Today `start_systick(200 MHz)` against a 400 MHz clock gives 1 kHz, not the intended 500 Hz. The intended controls rate is preserved.
- `asm::delay` calls are expressed in microseconds through a `delay_us(cpu_hz)` helper.

**Sample rate.**
- PLL3 uses its fractional divider for a 49.152 MHz SAI kernel clock, and MCKDIV comes from `pll3_for`. On rev V that's MCKDIV=4.
- An 8 MHz crystal with a 13-bit FRACN lands within about ±2 ppm of 48 kHz, not exactly on it. A host test asserts `fs_of(pll3_for(8 MHz, 48 kHz, rev))` is within 10 ppm for both revisions, and checks every field is in range.
- Today's MCKDIV=5 gives 47.9 kHz on rev Y but 95.8 kHz, an octave high, on rev V. Step 1 of bring-up shows the revision, which settles which one this board has.

**Format.** SAI data size 32 bits, I2S, `DacSample` left-justified. The SAI data register is right-aligned, which is why the data size must be 32 bits and not 24. This matches stock `main.c:391`. The CS4344 reads the top 24 bits.

**Three pairs on one clock.**

| Pair | SAI block | Role | Pins |
|---|---|---|---|
| 1 | SAI1 A | master, SYNCOUT | PE2 MCLK, PE4 FS, PE5 SCK, PE6 SD (AF6) |
| 2 | SAI1 B | internal synchronous slave | PE3 SD (AF6) |
| 3 | SAI2 A | external sync from SAI1 | PD11 SD (**AF10**) |

Stock PreenFM3 confirms this sync scheme and these pins, so every pair shares one FS.

**DMA.**
- Three circular buffers, one per pair: 2 halves × 64 frames × 2 channels × `DacSample`, 1 KB each.
- They live in their own linker section `.ram_d2.dma` at the start of D2. The section is 4 KB, aligned to 4 KB, and a linker assert checks size and alignment. It is the only MPU non-cacheable region, and the `Instrument` follows it in `.ram_d2.voices`.
- Boot order:
  1. pre-fill every buffer;
  2. enable all three DMA streams;
  3. enable the SAI slaves;
  4. enable the master last.
- Only stream 0 raises half- and full-transfer interrupts, and each interrupt renders that half for all three pairs.

**Overrun policy.** If both the half and full flags are set when the interrupt runs, it renders both halves, oldest first, which is the current behaviour, and counts one overrun. If a flag is still set on exit, it counts an overrun.

**Rendering, once per half (64 frames, 1.33 ms), in `audio/engine.rs::render_half`:**
1. `reader.read()` gives the `AudioShared` for this block.
2. `NoteSources::drain`, passing each event to `Instrument::handle`.
3. `Instrument::render(fx, &mut dac_out, shared)`. That method already writes the scope (`instrument.rs:262`). The scope write moves to the scope `TripleBuffer`'s `Writer`, fixing the existing `scope.rs` `static mut` race, which becomes a real preemption race on the chip.
4. For each pair, `interleave` into that pair's half-buffer.

Routing stays in `Instrument::handle`. It plays every Part on the event's channel, which gives layering, and note-offs release by the recorded `note_channel`, so a channel change can't leave a voice stuck. The routing and render code is otherwise unchanged apart from taking the budget and the scope writer.

**Memory and the stack.**
- `memory.x` gains `DTCM (rwx) : ORIGIN = 0x20000000, LENGTH = 128K`, and the stack moves there (`_stack_start`). Nothing large may be built on the stack.
- A painted-stack high-water mark shows on the AUDIO page.
- **In-place construction:**
  - `Instrument::init_in_place(&mut MaybeUninit<Self>, sample_rate, SampleBudget)`, `FxBus::init_in_place(&mut MaybeUninit<Self>)` (`FxBus::new` takes no sample rate; the effects' power-on state is all zeros), and the per-member helpers they call (`Voice::init_in_place` and each effect's) write field by field through `addr_of_mut!`.
  - Large arrays are zero-filled with `write_bytes`, and only types whose all-zero bit pattern is a valid value may be zero-filled; each such use has a `// SAFETY:` comment.
  - No value larger than 4 KB is built on the stack.
  - A host test checks that `init_in_place` and `new` produce identical output for the same notes, and the painted stack mark checks the stack on hardware.
- `.ram_d2` stays NOLOAD. Everything in it is written by `init_in_place` or the pre-fill before first use.

**Interrupt priorities** (typed `Priority`, upper nibble):
- **Audio DMA, 0x00:** the highest.
- **USART1, 0x40:** the FIFO covers the about 4 bytes that can arrive during a 1.33 ms render.
- **SysTick, 0xF0:** the lowest.

Today everything is at priority 0; `set_priority(…, 3)` is truncated.

## MIDI parsing

- `MidiMessage` channels become `MidiChannel`. The shells convert `NoteOn`/`NoteOff` to `NoteEvent`, and drop every other message for now; sub-project 2 consumes them.
- Test table, locking existing behaviour:
  - running status;
  - a realtime byte mid-message;
  - SysEx containing note-like bytes;
  - system common followed by stray data;
  - velocity 0 treated as note-off;
  - one-data-byte messages (program change, channel pressure);
  - pitch bend;
  - junk before the first status byte.

## Shared state

- `AudioShared` and the scope samples each use a `TripleBuffer`.
- The main loop calls `writer.publish(|b| b.update_from(&ui.performance))` once per frame.
- The engine calls `reader.read()` once per block. The UI reads the scope `Reader` once per frame.
- The triple buffer is sound for a real second thread (desktop) and for a preempting interrupt (chip). `split` taking `&'static mut` makes it take-once, and the two halves can't do each other's job.
- AXI cost: one more `AudioShared` (3,184 B) plus the scope buffers. There is about 18.9 KB of headroom today (505,360 of 524,288 B), and `AXI_RESIDENT` and its assertion count all three copies.

**Removed from the firmware:**
- `init_voice`, `trigger_note`, and the A4 test tone;
- the `VOICE`, `WORK_BUF`, `PARAMS` and `MOD_STATE_PTR` statics;
- every raw pointer into `ui.performance`;
- `panic-halt`.

**Audio to UI.**
- The header's sounding dot keeps coming from the scope peak (`ui/mod.rs:646`), so no extra channel is needed.
- `AudioStats` comes through a `TripleBuffer`, published once per block by `probe.rs`.
- The UI receives it as `Option<AudioStats>`, passed into `ui.render` beside `PerfStats`. It is `None` on desktop and with `perf-probe` off, and the AUDIO page then shows "--" (the u8g2 fonts are ASCII-only).

## CPU measurement

- `probe.rs` reads CYCCNT around `render_half` and calls `AudioStats::record(cycles, BlockBudget::for_cpu(cpu_hz))`.
- The System page gains an **AUDIO** sub-page, using the existing cell layout:
  - LOAD (average %);
  - PEAK (%);
  - OVERRUNS;
  - DROPS (by source);
  - DESYNC;
  - STACK (the high-water mark);
  - REV (V/Y/?);
  - CPU MHz.

  It fits Direction A and needs no new visualisation.
- **Bench** (`bench` feature, off by default):
  - at boot, it renders each engine with 1 to 6 voices and no DMA output, timed by DWT;
  - it shows cycles per voice per engine;
  - a separate commit replaces the `// estimate` `Cost` values with the measured ones and re-runs the allocator tests.

## Testing

**Host tests (pure core):**
- the parser table;
- `NoteSources` drain order and per-source drop counts;
- `to_dac`: clamping, the ±1.0 extremes, 0.0, the low 8 bits zero, and rounding;
- `interleave` layout per pair;
- `pll3_for` within 10 ppm and in field range, for revs V and Y;
- `AudioStats` average, peak and overrun arithmetic;
- `TripleBuffer`:
  - the reader always gets the newest completed publish;
  - a held read stays stable across publishes;
  - a model-based test of random publish/read interleavings, checking that no buffer is ever written while held;
- `init_in_place` equals `new` (render equality) for `Instrument` and `FxBus`;
- `SampleBudget`/`BlockBudget::for_cpu` at 400 and 480 MHz, and that the allocator honours the budget it was given.

**Goldens:** instrument audio goldens stay bit-identical. The `system` screen golden gains an AUDIO-page case.

**Builds.** `just check` builds and links the firmware:
- with default features;
- with `--no-default-features`;
- with `--features bench`.

It also builds the desktop with `--no-default-features`, and runs clippy on the firmware target, proving every optional module can be cut.

**On hardware:** the bring-up checklist below, each step flashed and checked by ear or eye.

## Bring-up order

Each step can be flashed on its own. If one stalls, the earlier ones still stand.
1. **Clocks, caches, stack, priorities, safe panic.**
   - Revision-selected CPU speed, D2 clocks, caches and MPU, the stack in DTCM, typed priorities, SysTick at its intended rate, and the panic/fault stop.
   - A temporary boot splash shows REV and CPU MHz.
   - The UI and the old single voice still work.
2. **Pair 1 at 48 kHz, 24-bit.** Exact rate and new format, with the old voice as a tone. It's in tune; check against a tuner.
3. **Pairs 2 and 3.** SAI1 B and SAI2 A synced, with a tone on each pair.
4. **Instrument and DIN on pair 1.** The `Instrument` built in place, DIN MIDI through `NoteSources`, all Parts on pair 1. The old voice path and its raw pointers are deleted here, since nothing calls them once the Instrument renders. It plays from a keyboard.
5. **Publishing.** `TripleBuffer` for `AudioShared`, all three pairs routed per Part.
6. **Measure.** The probe and the AUDIO page (replacing the step 1 splash), a bench run, and the measured `Cost` values committed.

Desktop parity (midir, the desktop `TripleBuffer`) lands alongside steps 4 and 5.

## ADRs

- **0019:** Note input is one parser and one single-producer queue per source, drained by the audio side each block; routing stays in `Instrument::handle`.
- **0020:** Audio clocking and output:
  - CPU speed chosen by silicon revision, with 400 MHz for rev Y or unknown;
  - the cycle budget becomes a runtime value derived from it. This **supersedes ADR 0013's clause** that the budget is a shared compile-time constant; the rest of 0013 stands.
  - 48 kHz ±10 ppm via fractional PLL3;
  - 32-bit SAI slots carrying left-justified 24-bit samples;
  - three SAI blocks sharing SAI1's clock;
  - the stack in DTCM.
- **0021:** Audio↔UI shared state uses a take-once triple buffer (fixes the desktop race).

## Risks

- **Rev Y silicon** means 400 MHz and fewer heavy voices. The runtime budget handles it; the allocator refuses over-budget notes. Step 1 reveals which revision this board is.
- **CPU:** 6 voices plus FX has never been measured. Step 6 shows it. If it doesn't fit, the measured `Cost`s make the allocator refuse the excess, and any change to `MAX_VOICES` is decided with those numbers.
- **DWT without a debugger:** CYCCNT may need the LAR unlock on the Cortex-M7 (unverified). The probe detects a stuck counter and shows "--".
- **AXI RAM:** a third `AudioShared` plus the scope leaves roughly 10.7 KB of headroom. The const assertion fails the build if that's exceeded, and #16 (the reverb arena) is the release valve.
