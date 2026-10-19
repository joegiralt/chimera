# TAPE slice 1: the DMA card layer, the streaming core, and PLAY streaming a long bed

- **Status:** Draft for owner review (2026-10-15)
- **Umbrella:** `2026-10-15-tape-sampler-design.md` (§ 5 streaming, § 9 slices 0–1)
- **ADRs:** 0147 (streaming by DMA behind heads), 0145 (PLAY folds ONESHOT, LOOP and PING-PONG)
- **Issue:** private issue #476
- **Rules:** no_std; no heap, blocking or panic in audio; every `unsafe` with `// SAFETY:`; params never snap; host first, the unit last.

## 1. Scope

**In:**
- **Part A, the bench (slice 0):** the real DMA read path and SPI-mode protocol, driven from the console's `card` in `bench-stress,bank2`, and benches B2–B7. Their numbers set the constants in § 2.
- **Part B, the product:**
  - the card layer: `SharedCard`, the bus word, the card interrupt, EDF;
  - SRAM4 rings;
  - the slot protocol;
  - heads in the arena;
  - the stream registry;
  - admission;
  - designed drop-outs;
  - quiet for writes;
  - fault handling;
  - `stats` counters;
  - the desktop's card model;
  - **PLAY**: the fold of ONESHOT, LOOP and PING-PONG, MODE and FADE, streaming forward ONE and LOOP.

**Out (later slices):** ZONES and TRON (2), PREP, SAMPLES, CARD TEST, the catalog (3), KIT (4), backward and ping-pong streaming, REEL (5), RESAMPLE (6), files over 1 MiB on the card (2's resumable `put`).

## 2. Constants, and what sets them

| Constant | Value | Where | Set by |
|---|---|---|---|
| `CHUNK_RECORDS` | 8 (4,032 B of samples, 2,688 frames) | `stream::layout` | B2: `t = L + n/BW`. If L p99 > 5 ms, try 12 records and revisit the ring size. |
| `HEAD_MS` | 100 | `stream::admit` | B3: `head ≥ ratio_max × p99.9(8th read of a burst)`, rounded up to 20 ms |
| `STREAM_BUDGET_BPS` | 466,000 (60 % of M1's 778 KB/s) | `stream::admit` | B4: the largest N with zero empty rings for 10 min at 75 % load, × 0.8 |
| `URGENT_MS` | 15 | `stream::edf` | B6: the UI stays ≥ 15 fps under N streams |
| `TOKEN_TIMEOUT_MS` | 100 | `chimera-fat::sd_proto` | B2's max × 10 |
| `QUIET_HOLD_MS` | 20 | `chimera-hal::card_bus` | — (a write burst's gaps) |
| `DROP_FADE` | 144 samples (3 ms) | `stream::cursor` | click test (§ 10) |
| `QUIET_FADE` | 480 samples (10 ms) | `stream::cursor` | click test |
| `GUARD_FRAMES` | 4 (2 packed pairs, 6 B) | `stream::ring` | Hermite's 4 taps |
| Card IRQ priority | level 12 | `chimera-board::priority` | between MIDI (4) and SysTick (15) |

B5 confirms writes stall reads (expected: yes, so § 8's quiet stays). B7 measures backward reads for slice 5 and costs nothing here.

## 3. Memory map

| Change | File | Detail |
|---|---|---|
| SRAM4 joins the map | `chimera-stm32/memory.x`, `chimera-stm32/build.rs` | `RAM_D3 (rwx) : ORIGIN = 0x38000000, LENGTH = 64K` in `memory.x`; the `.sram4 (NOLOAD)` output section in a script `build.rs` writes (as it writes `ram_d2.x`), with an assert that it fits `RAM_D3` |
| Rings in SRAM4 | `chimera-engine/src/stream/ring.rs` | `#[link_section = ".sram4"] static RINGS: Rings` (8 × 8,128 B = 65,024 B), asserted ≤ 64 KiB. CPU access only: the card IRQ copies in, the audio reads. Cacheable (the default map). |
| The DMA region grows to 16 KiB | `chimera-board/src/cache.rs`, `chimera-stm32/build.rs` (`ram_d2.x`), `chimera-engine/src/hw.rs` | The `.ram_d2_dma` output section (input `.ram_d2.dma`, where `audio/dma.rs`' `RINGS` already lands) at `ORIGIN(RAM_D2)` grows from 4,096 to 16,384 B, `ALIGN(16384)`, holding the SAI rings (3 KB, as now) and the card staging: one buffer of a 128 B token-scan area plus 9 blocks and a trailing CRC (4,738 B). The IRQ de-frames before it starts the next transfer, so a second buffer has no use. Its two asserts in `build.rs` change with it. MPU region 0's SIZE goes from 11 (4 KB) to 13 (16 KB); `DMA_REGION_RASR` and its assert change (`0x1308_0017` → `0x1308_001B`). `D2_DMA_RESERVE` 8 → 16 KiB. Nothing else may land in `.ram_d2.dma`. |
| SRAM4's clock | `chimera-board/src/cache.rs` (`enable_d2_sram`'s neighbour) | None needed: RCC_AHB4ENR has no SRAM4 enable on this part (only AHB4LPENR's `SRAM4LPEN`, for sleep; checked against the `stm32h7` PAC). A boot self-test writes and reads one word of each ring and reports `sram4 ok` in `status`. |
| Stream registry | `chimera-engine/src/stream/registry.rs` | A D2 static, ~2.2 KB: 8 rows, each a streamed source's `SourceId`, `Extents` and framing (§ 5) |

D2 free goes 86,240 → about 76,000 B. AXI grows by under 1 KB (per-voice cursor state lives in `TapeDeck`, already sized by MODAL's slot).

## 4. Part A: the protocol and the DMA path (slice 0)

The bench is built once, for real: the bench's `card` drives the same driver slice 1 ships.

### 4.1 Pure protocol: `chimera-fat/src/sd_proto.rs`

Beside `sd.rs` (`CMD0`, `r1_within_ncr`). Host-tested; no hardware types.

```rust
pub const CMD12: u8 = 12;  // STOP_TRANSMISSION
pub const CMD17: u8 = 17;  // READ_SINGLE_BLOCK
pub const CMD18: u8 = 18;  // READ_MULTIPLE_BLOCK
pub const DATA_TOKEN: u8 = 0xFE;
pub const BLOCK_WITH_CRC: usize = 514;
/// A token-scan burst. Under one block and its CRC, so the bytes after a
/// token are always the head of one block: one range.
pub const SCAN_BYTES: usize = 128;

/// A command frame with its CRC7 (CRC is on: `AcquireOpts::use_crc`).
/// The argument comes only from `AddrMode::arg` (byte or block addressing).
pub fn command(index: CmdIndex, arg: CmdArg) -> [u8; 6];
pub fn crc7(bytes: &[u8]) -> u8;
pub fn crc16(bytes: &[u8]) -> u16;           // CCITT, the data blocks'

/// Scans clocked-in bytes for the data token, across DMA bursts.
pub struct TokenScan { waited: Bytes }
pub enum Scan { Waiting, Token { at: usize }, Error(u8) }  // 0b000x_xxxx error token

/// One multi-block read's progress: which block, how much of it, its CRC.
pub struct MultiRead { /* blocks, done, scan */ }
pub enum Step { Scan, Rest { block: u32, have: Bytes, want: Bytes }, Stop }
pub enum Scanned { Waiting, Token { tail: Range<usize> } }
impl MultiRead {
    pub fn new(blocks: BlockCount) -> Self;
    pub fn step(&self) -> Step;
    /// A scan burst: the token's tail is the block's first bytes.
    pub fn scanned(&mut self, burst: &[u8; SCAN_BYTES]) -> Result<Scanned, ProtoError>;
    /// The block and its CRC, contiguous: the CRC16 check.
    pub fn landed(&mut self, block: &[u8; BLOCK_WITH_CRC]) -> Result<(), ProtoError>;
}
/// CMD12's answer: skip the stuff byte, then R1 is the first byte with
/// bit 7 clear within NCR (a data byte still clocking out is not R1).
pub fn stop_response(bytes: &[u8; 9]) -> Result<(), ProtoError>;
pub enum ProtoError { Crc, ErrorToken(u8), Timeout, R1(u8) }
```

### 4.2 The DMA shell: `chimera-board/src/card_dma.rs`

- **DMA2 stream 0** = SPI2 RX, **DMA2 stream 1** = SPI2 TX, through DMAMUX1 channels 8 and 9 (`ccr[8]`, `ccr[9]`: DMA2's streams follow DMA1's 0–7). The request ids are 39 (`Spi2RxDma`) and 40 (`Spi2TxDma`), as the `stm32h7` PAC's `DMAREQ_ID_A` has them. DMA1 streams 0–2 on channels 0–2 stay the audio's (`chimera-stm32/src/audio/dma.rs`); nothing uses DMA2 today.
- **TX sends 0xFF** from one constant byte, memory increment off. RX writes the staging buffer.
- **The transfer, as a state machine** run in `DMA2_STR0` (RX complete; also pended in software to wake it):
  1. **Command** (polled, about 3 µs): CS low, `command(CMD18, AddrMode::arg(lba))`, read R1 within NCR.
  2. **Token:** DMA `SCAN_BYTES` bursts of 0xFF into a small scan area (about 41 µs each at 25 MHz); `MultiRead::scanned` each one. On a token, its tail (the block's first bytes) is copied to the block's place in the staging.
  3. **Rest:** DMA the rest of the block and its 2 CRC bytes straight after the tail, so the blocks lie contiguous for `deframe` with no memmove (a block's CRC lands where the next block starts, and is checked by `landed` before that block's tail overwrites it). Then the next block's Token.
  4. **Stop:** `CMD12` (polled), `stop_response` (the stuff byte skipped, R1 within NCR), then polled bytes until MISO reads 0xFF (not busy), bounded by `TOKEN_TIMEOUT_MS`. CS high, one 0xFF. A stop error after CRC-good blocks keeps the data and is counted (`stop_errors`); the next CMD18's R1 says whether the card is well.
  5. **Done:** de-frame the samples into the ring slot (§ 5.3), mark it Ready, pick the next (§ 6).
- **One state machine, owned by the IRQ.** Bench reads from the UI thread only post a request and poll for its answer; the thread never runs a transfer or takes the bus, so nothing waits on its own interrupt. Entries are gated on the DMA flags (TC, TE) and the IRQ's own in-flight state, so a software pend mid-transfer is ignored.
- **Abort and watchdog.** TEIE is on; a TE, an OVR or MODF, or any bound passed (every poll is bounded by the cycle counter) runs the abort: both DMA streams EN = 0 and wait for it; CSUSP, wait SUSP, SPE = 0; clear LIFCR and the SPI's IFCR flags; CMD12, `stop_response`, busy (bounded); CS high and 8 clocks; the bus given back. A transfer whose TC never comes (a TE that cleared EN, a MODF) is caught by the watchdog: a bus held as `Stream` for over `WATCHDOG_MS` (200 ms) makes the UI loop, or `SharedCard`'s wait, ask the IRQ to abort. If CMD12's busy never ends, `fault = Read` and nothing starts until a UI mount clears it.
- The SPI peripheral is found as `SdSpi`'s HAL `Spi` leaves it: 8-bit, mode 3, SSM = 1 and SSI = 1, an endless transaction running (TSIZE 0, CSTART set by `send`), CFG2 written whole (so AFCNTR = 0 after every `rebuild`). The register sequences are data (`chimera-fat::h7_spi`), host-tested for order, and the shell runs them one access at a time:
  - **Take:** CSUSP and wait SUSP (only if CSTART), clear the flags, SPE = 0, save CFG1, write MBR for the fast clock itself (the UI may have left 400 kHz), AFCNTR = 1 (SCK stays driven while SPE = 0, mid-CMD18 with CS low).
  - **Burst:** suspend, SPE = 0, AFCNTR = 1, TSIZE = n, RXDMAEN, the streams (flags cleared first), TXDMAEN, SPE, CSTART (RM0433's SPI DMA order).
  - **Burst end:** wait EOT, check OVR and MODF (a protocol error), clear EOT, TXTF and the rest through IFCR, SPE = 0, the DMA enables off.
  - **Give:** suspend, SPE = 0, the DMA enables off, CFG1 restored, TSIZE 0, SPE = 1, so the HAL finds the peripheral as it left it.
  - Every CR1 write carries SSI (a write that drops it is MODF, which clears SPE and master mode); the type allows no other.
  - `// SAFETY:` the bus word (§ 4.3) is `Stream` for the whole transfer, taken by this handler, so no other code touches SPI2's registers or CS. The UI path takes the word before any register access, its re-init and mount included.
- **Memory:** the staging is a `static mut MaybeUninit` in `.ram_d2.dma`, reached by raw pointer while a stream may run and read as a slice only after TC, a DMB and an acquire fence. The IRQ's state lives in one cell only its handler opens. The IRQ never holds `SdStore`: it needs only SPI2, CS and `CARD_ADDR`.
- **IRQ cost:** a 5 ms token wait is about 120 scan bursts at about 1 µs of work each. Non-cacheable D2 byte loads cost about 10–20 cycles each from the M7, so de-framing 4,032 B and the CRC16 over 4,608 B come to about 250–350 µs a chunk (word loads bring it down; B2 measures CRC16, and slice 0's report carries `card_irq_us_max`). At about 86 chunks a second that is under 3 % of the CPU, all of it under the UI, none under audio.
- B2 also reports CRC16's time. If it passes 200 µs a chunk, the H7's CRC unit takes it (a later change, not this slice).

### 4.3 The bus word and `SharedCard`

```rust
// chimera-hal/src/card_bus.rs: pure atomics, host- and loom-tested
// (`chimera-core/tests/loom_card_bus.rs`); chimera-fat, chimera-engine and
// chimera-board all see it.
#[repr(u8)] pub enum Owner { Free = 0, Ui = 1, Stream = 2 }
pub enum Took { Taken, Wanted }
pub enum Wake { Pend, Quiet }
pub struct CardBus {
    owner: AtomicU8, since: AtomicU32 /* a Stream take's Ms */, ui_waiting: AtomicBool,
    wanted: AtomicBool, abort: AtomicBool,
    quiet: AtomicU8, fault: AtomicU8, streams_active: AtomicU8,
}

// chimera-fat/src/shared_card.rs: generic, host-tested over a fake `Medium`.
/// The UI's card, for `FatStore`: every touch of the device takes the bus.
pub struct SharedCard<D: Medium> { dev: D, bus: &'static CardBus, addr: &'static CardAddr, hooks: Hooks, .. }
pub enum SharedError<E> { Busy, Dev(E) }
impl<D: Medium> BlockDevice for SharedCard<D> { /* take(Ui) → dev → give() → pend() on Wake::Pend */ }
impl<D: Medium> Medium for SharedCard<D> { /* start_op, reinit, mounted under the bus; classify(Busy) = StoreError::Busy */ }
```

- `chimera-board/src/sd.rs`: `pub type SdStore = FatStore<SharedCard<SdDevice>>`, `pend` pending `DMA2_STR0`. `STORE_RESERVE` still holds (the wrapper adds a few pointers).
- **Every SPI2 touch takes the bus.** Not only `read`, `write` and `num_blocks`, but `Medium::start_op` (an acquire's CMD0), `reinit` (it rebuilds SPI2 at 400 kHz, sends wake clocks and marks the card uninit) and `mounted` (it rebuilds SPI2 at the fast clock). `reinit` clears `CARD_ADDR` and `mounted` sets it (and clears `fault`), both while holding the bus. The bench's and the probe's raw device calls go through `with_device`, which takes it too.
- **A bus wait that times out is `Busy`, never a card error.** `SharedCard`'s own error, `Busy`, classifies as `StoreError::Busy`, which `FatStore` never re-inits on and the UI treats as a retry, not a fault. Otherwise a UI wait timing out behind a long stream transfer would re-init SPI2 under an in-flight CMD18.
- **The UI takes the bus** by setting `ui_waiting`, then spinning on `Free → Ui` under the operation's deadline (`SD_IDLE_MS`). The card IRQ, after each transfer, gives the bus up when `ui_waiting` is set and no stream is urgent (`URGENT_MS`). The UI waits at most one transfer (about 7 ms). While it spins, a bus held as `Stream` past `WATCHDOG_MS` asks the IRQ to abort (§ 4.2).
- **Wakeups, with none lost:**
  1. The audio pends `DMA2_STR0` on every new want, whoever owns the bus (one NVIC write; the handler ignores a spurious entry).
  2. The IRQ takes only through `take_or_want`: a failed CAS sets `wanted`, then retries the CAS once.
  3. `give` stores `Free` first, then swaps `wanted` out; `Pend` if it was set, and the giver pends. A `want` the swap misses landed after `Free`, so the retry in 2 takes the bus.
  4. The IRQ, after giving, re-scans the wants and pends if any is left.
- **Writes quiet the streams** (§ 8) inside `SharedCard::write`, so every writer is covered without touching its call site.
- The bench's `card_speed.rs` drops its own read path and posts its reads to the IRQ. `Extents` moves from `chimera-core/src/console/card.rs` to `chimera-hal/src/store.rs`, unchanged, so the bench, the registry and the IRQ share it (chimera-engine can't see `chimera-fat`).

### 4.4 The benches

The console's `card` gains verbs (`chimera-core/src/console/card.rs`), each printing raw lines (n, median, p95, p99, p99.9, max, overruns, `loaded_load_pct`, card name and size, git hash), at ≥ 20,000 samples for p99.9. Each runs at the INIT load and at ≥ 70 % (a heavy Sound held through the console keys).

| Verb | Bench | Pass | Fail |
|---|---|---|---|
| `card dma chunk` | B2: DMA CMD18 random reads of 1, 2, 4, 8, 16 KB; fit `t = L + n/BW`; CRC16's time | L p99 ≤ 5 ms | L p99 > 8 ms |
| `card dma burst` | B3: 8 reads queued at once, 1,000 bursts; the 8th's time | p99.9 ≤ 60 ms | > 90 ms |
| `card dma sustain N` | B4: N rings at 41 KB/s × 1.2 through the real slot protocol and EDF, 10 min, each stream its own bed | N = 8 (the rings' cap) with zero empties | N < 6 |
| `card dma stall` | B5: one 4 KB write a second to a pre-allocated 4 MiB file while B4 runs at N/2, 30 min | write-busy p99.9 ≤ 100 ms, max ≤ 250 ms | max > 250 ms |
| `card dma ui` | B6: B4 at N with full-screen redraws and a project save | UI ≥ 15 fps, the save completes, no overrun | UI frozen > 200 ms, or an overrun |
| `card dma back` | B7: a backward 4 KB walk | ≤ 1.3 × forward random | > 2 × |
| `card dma listen N` | B8: B4 at N, stream 0 played aloud and recorded. Each stream reads its own aperiodic bed (a chirp: no period divides a chunk, so a repeated, skipped or misplaced chunk shows), and the firmware checks every chunk it consumes, on every stream, against a host-computed table on the card (`BEDS.CHK`) | zero discontinuities in the recording; zero order and check errors on every stream | any |
| `card dma inject <abort\|te\|modf>` | B9: 100 injected faults mid-transfer under B4 at N = 2, each followed by a UI card read | every UI read succeeds; no fault; streams resume | any |
| `card dma wake N` | B10: B4 at N = 1 and 2 with a UI card read every 50 ms, 10 min | zero empties | any (a lost wakeup) |

Each result goes into (private notes) as raw text, and `card clean` follows. A fail on B2–B4 stops slice 1 and goes back to the owner with the numbers.

## 5. The source side

### 5.1 The descriptor

`chimera-engine/src/dsp/tape_engine/desc.rs`. The descriptor is RAM only (`held`'s first 16 B), so this changes no file.

- **Byte 8 becomes `heads`:** the number of heads a streamed source holds (0: held whole). Bytes 9–15 stay reserved, zero.
- `from_bytes` now takes `head_frames > 0` on a `Plain` when `heads ≥ 1` (it took it only on a `ZoneSet`). A `ZoneSet` keeps its implicit heads, one per zone start (ADR 0093). `heads` is 0 on a `ZoneSet` in this slice.
- **A streamed `Plain`'s `held`:** the descriptor, then `heads` × `at: u32` (each head's first frame, even), then each head's `head_frames` of packed samples.

### 5.2 Framing: `chimera-engine/src/stream/layout.rs`

A `.SRC` is the 28 B header, the `SourceHead` record (4 + 34), a `ZoneSet`'s `SourceZones` record, then `SourceData` records of 4 + 504 B, the last shorter (`storage/source_file.rs`). Packed byte `p` therefore sits at file offset `data_off + 508·⌊p/504⌋ + 4 + p mod 504`. 504 B is 168 whole pairs, so no pair straddles a record.

```rust
pub struct StreamFile {
    pub ext: Extents,        // ≤ 32 runs (MAX_RUNS), from FatStore::runs
    pub data_off: u32,       // the first SourceData record's offset in the file
    pub data_len: u32,       // packed bytes
    pub frames: u32,
}
pub struct ChunkIdx(pub u32);
pub fn chunk_of(frame: u32) -> ChunkIdx;            // frame / 2,688
pub fn chunk_frames(c: ChunkIdx, f: &StreamFile) -> Range<u32>;
/// The file blocks a chunk's records cover, and where its samples start in them.
pub fn chunk_blocks(c: ChunkIdx, f: &StreamFile) -> (Range<u32>, u16 /* skip */);
/// Copies a chunk's samples out of its raw blocks, checking each record's
/// tag (0x0018) and length; a mismatch is a read error, never audio.
pub fn deframe(raw: &[u8], skip: u16, out: &mut [u8]) -> Result<usize, Deframe>;
```

`source_file.rs` gains `data_offset(&SourceLayout) -> u32` beside `data_start`, so the writer and the reader agree from one place.

### 5.3 Rings: `chimera-engine/src/stream/ring.rs`

- Voice `n` owns ring `n`: 2 slots, each `GUARD` (6 B) + 4,032 B, padded to 4,064 B.
- The card IRQ writes a slot's guard from the other slot's last 6 B when that slot holds the chunk before (it is Ready, so only read). After a jump there is no chunk before: the slot is marked guardless, and the cursor starts reading it one frame in.

### 5.4 The slot protocol: `chimera-engine/src/stream/slot.rs`

One `AtomicU32` a slot: state (2 bits: Empty, Wanted, Filling, Ready), generation (6 bits), chunk (24 bits: 2,688 × 2²⁴ frames is past any 4 MiB file). One `AtomicU32` deadline a slot, in audio blocks.

| Who | Move | Ordering |
|---|---|---|
| Audio | Empty → Wanted(chunk, gen+1), deadline first | deadline `Relaxed`, word `Release` |
| Card IRQ | Wanted → Filling (CAS on the whole word) | `Acquire` |
| Card IRQ | Filling → Ready, after the copy and guard | `Release` |
| Audio | Ready → Empty when the head leaves; Wanted or Filling → Empty with gen+1 to cancel | `Release` |
| Card IRQ | a fill whose CAS to Ready fails (the generation moved) is dropped | — |

The audio reads a slot's bytes only after an `Acquire` load shows Ready under its own generation. Loom covers it, as `source_table.rs`' gate is covered: `chimera-core/tests/loom_stream_slot.rs`, added to `just loom` (which runs only `loom_source_table` today).

### 5.5 The registry: `chimera-engine/src/stream/registry.rs`

- `StreamRegistry`, a D2 static: 8 rows, each a `SourceId` and its `StreamFile` (a 32-run `Extents` is 268 B, so 32 rows, one a `MAX_SOURCES`, would be about 9 KB). A ninth streamed source refuses to load (`STREAMS FULL`, provisional copy).
- The UI writes a row before it publishes the source's pool row (`Release` on the pool row orders it). It clears it only after the source is retired and its `sounding` bit is clear (ADR 0087's retire).
- The card IRQ reads rows only for ids a Wanted slot names.

### 5.6 Loading: whole or streamed

`chimera-core/src/ui/source_cell.rs` (`SourceLoad`) and project-load pass 2:

- **Whole** if `held_len(whole)` fits the arena's free space, as now.
- **Streamed** otherwise, if MODEL is PLAY in MODE ONE or a forward LOOP. Its `held` is the descriptor, the head table and the heads: START's head, and LOOP's loop-start head when MODE is LOOP. A head is `HEAD_MS` long, or to END if that is shorter. The loop-start head begins XFADE (at most 100 ms) before the loop's start, so the wrap's crossfade, which reads the material before the loop, reads RAM. The UI resolves `runs` through `Store::runs`, new in `chimera-hal` beside `read_at` (`SourceLoad` is generic over `Store`; `FatStore` forwards its own `runs`; `DirStore` and `MemStore` give one synthetic run per file that the desktop's card model maps back; the default is `Io`) (more than 32 refuses: `FRAGMENTED · COPY IT AGAIN`), writes the registry row, reads the heads with `read_heads_into` (beside `read_source_into`, the same 4 KB a UI pass), and publishes.
- **Refused** otherwise: `PING NEEDS RAM`, `REVERSE NEEDS RAM`, or the machine's own reason (SCRUB, STRETCH and WAVE are RAM only). The meter's preview shows the heads' cost.
- A streamed source whose MODEL or MODE later changes to one that needs RAM refuses its note-ons with the same reasons (`TapeDeck::refuses`), before allocation.
- **Re-heading.** START and the loop's start aren't modulatable (note-on, ADR 0093), but they can be turned. 200 ms after the last turn, the UI re-stages the heads: a new row under the gate, as a compaction moves bytes, or, while a note holds the source, beside it under a free id on the same file, the old id retiring as its notes end (#598, ADR 0147). Notes started before that read a stale head: their START has none, so they start cold (§ 7.3).

## 6. Scheduling: `chimera-engine/src/stream/edf.rs`

Pure:

```rust
pub struct Want { pub voice: u8, pub slot: u8, pub deadline: u32 }
pub enum Pick { Fill(Want), YieldToUi, Idle }
pub fn pick(wants: impl Iterator<Item = Want>, now: u32, ui_waiting: bool) -> Pick;
```

- The earliest deadline wins; ties go to the lower voice.
- With `ui_waiting`, it yields unless the earliest deadline is within `URGENT_MS` (11 blocks).
- The IRQ scans 16 slot words a pick: about 50 cycles.

The deadline the audio writes is `now + frames_ahead / (inc_max · 64)` blocks. `frames_ahead` is the material Ready between the head and the wanted chunk's start; `inc_max` is the bill's fastest speed (§ 7.4).

## 7. The audio side

### 7.1 The context

`EngineCtx` (`chimera-engine/src/dsp/engines.rs`) gains `streams: &'a StreamPort<'a>`: this voice's ring and slot words, the registry's frame counts, the block counter and the quiet flag. ALGO, MODAL and VA ignore it, as they ignore `src`. `TapeDoor`'s `start` and `play` take `&StreamPort<'_>` (bank 2's seal changes, as for any release).

### 7.2 Spans: `chimera-engine/src/dsp/tape_engine/span.rs` (bank 2: `build.rs`' `bank2.x` gains a `11tape_engine4span` glob; it links bank 2 by module path, so a new module not named there lands in bank 1)

```rust
/// A run of contiguous source frames the voice can read: a head in the
/// arena, or a Ready ring slot (guard included).
pub struct Span<'a> { view: SourceView<'a>, first: u32 /* source frame of view's frame 0 */ }
impl Span<'_> {
    /// Whether a Hermite read at source frame `i` (taps i−1..=i+2) lies inside.
    pub fn holds(&self, i: u32) -> bool;
}
```

`SourceView` gains `SourceView::span(bytes, desc)` for a slice that isn't a whole row; its clamping and SAFETY argument are unchanged, now relative to the span.

### 7.3 The cursor: `chimera-engine/src/stream/cursor.rs` (pure; linked into bank 2 with `TapeDeck` by a `6stream6cursor` glob added to `bank2.x`)

Per streamed voice, about 48 B inside `TapeDeck`:

```rust
pub struct Cursor { want_next: ChunkIdx, gen: [u8; 2], gain: Fade, state: Flow }
pub enum Flow { Head(u8), Ring(u8), Starved { since: u32 }, Quiet, Cold }
```

Once a block, before the model renders:

1. **Plan the block's travel:** `[pos, pos + inc_max · 64 + 2]`. If one span holds it, render in one run. Otherwise split the block at the span's last readable frame and switch spans between the runs. ONE's and LOOP's loops (`oneshot.rs`, `heads.rs`) run per span; their per-sample paths don't change.
2. **Ask ahead:** a slot leaves Ready → Empty when the head passes its last frame, and is asked again (Wanted) for the next chunk at once. At note-on both slots are asked: the chunk after the head, then the one after that.
3. **Underrun:** if the block's travel plus `DROP_FADE` would reach a frame no Ready span holds, the gain fades to 0 over `DROP_FADE` samples, ending at the last held frame; the state becomes `Starved`. The head keeps moving at `inc` (tape time runs on). Every block while starved re-asks for the chunk under the head (cancelling stale wants by generation). When it is Ready, the gain fades in over `DROP_FADE`. `stream_underruns` counts each one.
4. **Quiet** (§ 8): the gain fades out over `QUIET_FADE`; wants are cancelled; the head moves on. On release the cursor re-asks from the head and fades in as from a cold start.
5. **Cold start** (a START or loop start with no head, or after quiet): silent until the first chunk is Ready, then a `DROP_FADE` fade in. `stream_cold` counts each one.
6. **LOOP's wrap** jumps to the loop-start head, which the source always has while MODE is LOOP. The crossfade's outgoing head reads the loop's end from the ring; its incoming head reads the material before the loop from the loop-start head, which begins XFADE early (§ 5.6). XFADE is capped at 100 ms of source (the cell's focus band shows `STREAMED · XFADE ≤ 100 MS`).

The fades are gains on the model's output, ramped per sample, so nothing steps (ADR 0112). The voice's own 1 ms note-on ramp still runs.

### 7.4 Admission: `chimera-engine/src/stream/admit.rs`

```rust
pub struct Bill(pub u32);  // bytes/s of card reads
pub fn bill(rate: u16, ratio_max: f32) -> Bill;   // rate × 1.5 × 4,608/4,032 × ratio_max
pub fn ratio_max(note: u8, root_cents: f32, pitch_reach_cents: f32, bend_cents: f32) -> f32; // + 33 cents of wow
pub enum Admit { Ok, Steal(usize), Refuse }
pub fn admit(new: Bill, playing: impl Iterator<Item = (usize, Bill, Age)>, budget: u32) -> Admit;
```

- `Instrument::refuses` (`instrument.rs`) and its caller run it after the source checks and before allocation. `Steal(v)` releases voice `v` through `Allocator::fade_from`, its stream fading with it. The oldest released voice goes first, then the oldest held one (decision: umbrella open question 2).
- `pitch_reach_cents` sums `|amount| × range` of the PITCH and FINE routes, as TRANSWAVE's level term does (`LEVEL_CENTS`), plus STEAL GLIDE's reach.
- **The CPU bill:** `COST_STREAM` (+20, an estimate) on top of PLAY's per-mode bill for a streamed source, re-measured by the bench rows (§ 10.4).

### 7.5 PLAY

`chimera-engine/src/dsp/tape_engine/params.rs`, `machine.rs`:

- `TapeModel::Play = 0` replaces `Loop`. `Oneshot` (7) and `PingPong` (2) stay as load codes only: MODEL's code sets one param, so the load's translation (beside `translate_v1`, which already turns REVERSE's code 1 into LOOP) turns code 7 into PLAY with MODE ONE and FADE HOLD, and code 2 into PLAY with MODE PING; `TapeModel::from_disk_code` no longer yields them. Their SLICE values load and are ignored.
- **MODE** (id 15: ONE, LOOP, PING; note-on). The engine's dispatch on `model` becomes a dispatch on MODE inside PLAY; the three paths are the existing ones.
- **FADE** is DRAG (id 9) on PLAY's page, with a last step **HOLD** (note-off ignored) in MODE ONE only, dimmed in LOOP and PING. ONE's default is HOLD; LOOP and PING's is 32 ms (the existing TRON release's law, `tron::release_k`).
- MODEL's order in this slice: PLAY, TRON, SCRUB, STRETCH, WAVE. KIT and REEL take their places in slices 4 and 5.
- **Machine page PLAY:** MODE, LENGTH, XFADE, FADE, WOW. DECK: MODEL, POSITION, WALK, TAPE, AGE.
- `MACHINE_NAMES`, `MODEL_NAMES` and the factory TAPE Sounds' text sources (`tools/factory-tapes/recipes.toml`, built by `chimera-desktop/src/factory_tapes.rs`) follow.

## 8. Quiet for writes

Decision 8, applied to every card write while streams play (umbrella open question 3).

1. `SharedCard::write` sees `streams_active > 0` and `quiet` is 0: it sets `quiet = Asked` and spins until the audio answers `Quiet`. The audio answers after every streamed voice's gain reaches 0: `QUIET_FADE`, at most 2 blocks more. A 50 ms deadline guards the spin.
2. The card IRQ, seeing `Asked`, finishes its transfer and starts no other.
3. The write runs. The quiet holds across a burst. The UI loop's tick releases it once `QUIET_HOLD_MS` passes with no write (`CardBus::settle(now)`).
4. On release, every streamed voice re-asks from where its head now is and fades in when its chunk lands (§ 7.3, step 5).

RAM voices and every other engine play on throughout. `stats` reports `card_quiet_ms_max`. A boot-time write (the sweep's heal, a bank-2 install) finds no streams and doesn't quiet.

## 9. Faults

| Event | Card layer | Audio | UI |
|---|---|---|---|
| CRC16 mismatch or error token | One retry of that transfer | — | — |
| A second failure, a token timeout (`TOKEN_TIMEOUT_MS`), a bad CMD18 R1, or an abort whose CMD12 never leaves busy | `fault = Read`; Filling slots go back to Wanted; nothing more starts | Every streamed voice starves (§ 7.3); new streamed notes are refused (`Admit::Refuse`) | TAPE pages' focus band reads `CARD READ FAILED`. Next pass: `Medium::reinit` through `SharedCard`. Success clears `fault`; the IRQ resumes. |
| The volume changed after a re-init (`StoreError::VolumeChanged`) | Registry cleared | Streamed sources' notes refused | Streamed sources read `SOURCE MISSING` until the project resolves them again |
| A record tag or length mismatch in `deframe` | As a read error, without the retry | as above | as above, plus `stats` `stream_frame_errors` |
| The same chunk, at the same blocks, failing to de-frame again after the re-init (#609) | The file's: no fault; counted in `stream_frame_errors`; the chunk passed over from then on (`serve::Rot`); a second such chunk condemns the file whole, every chunk passed over | The voices reading it drop out over it and fade back in past it | A condemned file's SOURCE cell reads `BAD FILE`, its band `BAD FILE · DROPS OUT`; a copy at other blocks or another card clears it |
| A bad CMD12 R1 after CRC-good blocks | The chunk is kept; `stop_errors` counts it; the next CMD18's R1 decides | — | — |
| A DMA TE, an SPI OVR or MODF, a bound passed, or a lost TC (the watchdog) | The abort (§ 4.2): the bus is always given back; the transfer counts as a failure of that transfer | as for a read error | — |
| A UI bus wait that times out | `Busy`: no re-init, no fault | — | the operation retries on a later pass |

No path panics, blocks the audio or allocates. The audio's only failure mode is silence that fades.

## 10. Tests

All host-side first (`just test`). The unit is the last gate.

### 10.1 Pure

- **`chimera-fat/tests/sd_proto_test.rs`:** `command` matches the SD spec's CMD0, CMD17 and CMD18 frames and CRC7s; `crc16` matches the SD Physical Layer spec's example (512 × 0xFF → 0x7FA1; check it against the spec when writing the test); `TokenScan` finds a token at every offset of a burst and across burst edges, reports error tokens, and times out; `MultiRead` walks 9 blocks with tokens anywhere in a scan burst; `stop_response` skips junk before R1.
- **`chimera-fat/tests/h7_spi_test.rs`:** the register sequences' order (suspend before SPE = 0, RM0433's DMA order, the abort's DMA-first stop, the HAL's state restored on give).
- **`chimera-fat/tests/shared_card_test.rs`:** a `FatStore` over a held bus answers `Busy` and never re-inits; every `Medium` call that does I/O takes the bus.
- **`card_bus_test.rs`** and **`loom_card_bus.rs`:** no want is left stranded with the bus Free and no pend outstanding.
- **`chimera-engine/tests/stream_layout_test.rs`:** proptest over sources of 64 frames to 4 MiB, `Plain` and `ZoneSet`, and random run splits up to 32: for every chunk, `chunk_blocks` and `deframe` give exactly the bytes a whole-file de-frame gives. A corrupted record tag is a `Deframe` error.
- **`stream_edf_test.rs`:** EDF order; ties; the UI yields unless urgent; a starvation bound (the UI waits at most one transfer when nothing is urgent).
- **`stream_slot_test.rs`** (and a loom model): every interleaving of want, claim, fill, cancel and re-want leaves the audio reading only bytes of its own generation; a cancelled fill is dropped.
- **`stream_admit_test.rs`:** bills at `ratio_max` (a +12 route doubles the bill); 8 beds at 1× fit the default budget; 8 TRON-speed notes fit; a ninth bill steals the oldest released voice, then the oldest held; refuse when nothing can be stolen.

### 10.2 The voice

A `TestCard` in `chimera-engine/src/stream/test_card.rs` (feature `test-support`) serves Wanted slots from an in-memory `.SRC` after a scripted latency in blocks, so the tests run the real slot protocol, cursor and models.

- **`streamed_equals_whole`:** PLAY ONE and LOOP on a 30 s source, streamed with an on-time card, render **bit-identically** to the same source held whole, at notes 36–96, with TAPE and AGE on, across a START turn re-heading. This is the slice's key test.
- **`underrun_drops_out_cleanly`:** a late chunk gives a fade out ending at the last held frame, silence, then a fade in at the frame tape time has reached; the click rule (`click_free_test`: no step over 4× the steady state's largest) holds at both edges; the counter counts one.
- **`chord_burst_holds`:** 8 note-ons in one block at TRON speed, with the card at M1's max (6.66 ms a read, in blocks) and one transfer in flight: no underrun with `HEAD_MS` 100; with 60 ms, the test shows where it starves (this pins the head rule).
- **`quiet_fades_and_returns`:** quiet asked mid-note: the streamed voice is at 0 within `QUIET_FADE` + 2 blocks; a RAM voice is untouched (bit-identical to a run without quiet); release fades back in, click-free.
- **`fault_starves_and_recovers`:** an injected fault drops every stream; new streamed notes are refused before allocation; clearing it resumes them.
- **`play_loads_old_codes`:** codes 7 and 2 load as PLAY with the MODEs and FADEs above; the goldens of ONESHOT, LOOP and PING-PONG are bit-identical on PLAY (ADR 0011: a refactor lock).
- **`hold_only_in_one`:** FADE HOLD is unreachable in LOOP and PING; no setting makes a loop sound forever.
- **`span_reads_stay_in_bounds`:** `reads_stay_in_row`'s proptest, run through spans: every tap lands inside its span.

### 10.3 Memory and cost

- `RINGS` fits SRAM4; `.dma` is exactly the SAI rings and the staging; D2 still fits `VOICE_RAM_BUDGET`.
- `size_of::<TapeDeck>() < size_of::<ModalEngine>()` still, with the cursor in it.
- `tape_bills_each_model` gains PLAY's modes, streamed and not.

### 10.4 Bench rows (bench build)

- `TAPE STREAM /VOICE`: eight PLAY LOOP voices streamed at `inc` 1.2, cold cache, a span switch every block (the worst). It replaces `COST_STREAM`'s estimate.
- `CARD IRQ`: the card interrupt's worst time per transfer and its share of a second.

### 10.5 The desktop

`chimera-desktop/src/card_stream.rs`: a thread serving Wanted slots from `DirStore` files, with a latency model from `CHIMERA_CARD_LATENCY` (`m1`, replaying M1's distribution; `zero`; `slow`, 3× M1) and `CHIMERA_CARD_FAULT=after:N` to inject a fault. QA (qa-before-uat) plays PLAY streamed on every factory bed under `m1` and `slow` and records the listening page.

## 11. Flash and RAM

- **Bank 1:** about 5 KB for the protocol, shell, bus word and `SharedCard`; about 3 KB for the registry, slot protocol, EDF and admission. It is measured with `rust-size` at the slice's start and end, and reported in a line (flash-counting-proportion).
- **Bank 2:** about 3 KB (spans, the cursor, PLAY's dispatch).
- **RAM:** SRAM4 65,024 B; D2 +8 KiB (the DMA region) and about 2 KB (registry); AXI under 1 KB; the arena 7.2 KB a streamed PLAY source (two heads).

## 12. The unit proof (the last gate)

Claude flashes and reads the unit over the console (hardware-checks-only-at-ship); the owner only for a power pull.

1. `just firmware-orbit`, then `python3 tools/chimera-usb.py update target/chimera-orbit.bin target/chimera-orbit.bk2` (`just flash` builds the default set, not ORBIT). `status` shows bank 2 open and `sram4 ok`.
2. Put four 24 kHz beds of 25–28 s (each under `put`'s 1 MiB; host-built from CC0 rain, crackle, room tone and tape hiss) with `just put-tape`.
3. A project: Parts 1–4 PLAY LOOP on the beds (streamed: each is bigger than the arena's free space); Part 5 a 4-voice chord on a heavy Sound held through the console keys at ≥ 70 % load.
4. Ten minutes. `stats`: `stream_underruns 0`, `overruns 0`, `stream_faults 0`, the UI's `loop_peak_us` and frame rate recorded.
5. A MENU-hold save mid-run: the beds fade out and back (`card_quiet_ms_max` recorded), the save completes, Part 5 never drops.
6. `fault card` (fails the next transfer twice): the beds drop out, the TAPE pages read `CARD READ FAILED`, and within a second they recover. `card` is bench-only (`chimera-board/bench`), so this verb joins the `fault` command of `fault-inject` builds, and this step runs on an ORBIT build with `fault-inject` (`--features orbit,fault-inject`); if that does not fit FLASH, the controller rules.
7. Raw console text in the ledger. Then the owner's UAT by ear: the beds loop for minutes with no clicks, and a save sounds like a tape lifted and dropped.

## 13. What the bench may change

- **B2's L p99 > 5 ms:** bigger chunks (12 records), rings re-sized, SRAM4 no longer enough: rings move to D2 (76 KB free) and the arena shrinks nothing. The umbrella's § 5 numbers are redone in the same commit.
- **B3's p99.9 > 60 ms:** `HEAD_MS` grows (120–140), and every head's cost on the meter with it.
- **B4 < 8 streams:** the default budget drops, and the minimum card spec for testers becomes a release note (B8 later, on more cards).
- **B5 max ≤ 30 ms on this card:** quiet could become optional. It stays in this slice anyway, because other cards differ.
