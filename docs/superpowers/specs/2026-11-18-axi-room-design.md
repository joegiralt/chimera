# AXI room: AUDIO's snapshots to ITCM

Owner-approved approach (2026-11-18): free AXI once by moving CPU-only state
into ITCM, so per-Part settings (#342's EQ, #396's SMART EQ, tester features)
stop hitting ADR 0087's 32 KiB margin.

## Baseline (ORBIT, origin/0.4 1ff9f2a2)

`just firmware-orbit`, `rust-size -A` and `rust-nm -C -S --size-sort`:

- AXI: `.data` 300 B + `.bss` 491,172 B; `__sheap` 0x24077FD4, so
  0x24080000 − `__sheap` = 32,812 B, **44 B over the 32 KiB margin**.
- ITCM: `.itcm` 17,432 B of 64,512 B (63 K from 0x400): `ED` 1,400, `KITS`
  5,828, `KIT_VIEW` 10,200. 47,080 B free. ITCM has no margin assert, only
  overflow.

## Audit: AXI residents by size

| Symbol | Bytes | What it is | Who touches it | ITCM? |
|---|---:|---|---|---|
| `shared::POOL_ARENA` | 163,840 | The Sound/sample arena (ADR 0087) | UI, card | No: too big, and it stays by brief |
| `display::FRAMEBUFFER` | 153,600 | ILI9341 frame | UI, SPI1 DMA | No: too big (2.4× ITCM), and DMA reads it |
| `audio::engine::FX` | 112,456 | FX bus, the `i16` delay line most of it (ADR 0090) | audio ISR | No: too big |
| `shared::AUDIO` | **23,104** | `TripleBuffer<AudioShared>`: the UI's three Performance snapshots for the audio (ADR 0021) | UI writes, audio ISR reads, bench lends slot 0 before | **Yes: chosen** |
| `audio::engine::ENGINE` | 5,936 | The audio's `Engine` (readers, `DacBlocks` mix pairs, runner) | audio ISR only | Yes in principle (CPU-only; the DMA rings are D2's `.ram_d2.dma`); left for later |
| `shared::CTRL` | 4,632 | MIDI in control rings | MIDI ISRs, UI | Not without work: zero `.bss`, needs a build-in-place first; small |
| `usb_audio::RING`, `PLAY` | 4,104 each | USB audio rings | USB ISR (OTG FIFO), audio ISR | Not chosen: zero `.bss`, needs in-place build; near the USB peripheral path |
| `tuning::TUNINGS` | 3,684 | Tuning tables | UI, audio | No: `static` with const contents, needs flash init |
| `shared::SCOPE` | 2,884 | Scope triple buffer | audio, UI | No: const-initialised (`scope_buffer()`) |
| `usb::CORE`, USB `VAR`s | ≤ 1,892 | usb-device state | USB ISR | No: small, peripheral driver state |
| `sd::STORE` | 1,464 | Card store | UI, card IRQ | No: small, beside the card's DMA path |
| `shared::ORBIT` | 940 | `TripleBuffer<OrbitView>` | runner, UI | Yes in principle; too small to be worth an ADR |
| the rest | < 900 each | pipes, latches, counters | | Not worth moving |

## Choice: `AUDIO` alone

`AUDIO` is the one big, clean move:

- **CPU-only.** The UI publishes through `Writer`, the audio interrupt reads
  through `Reader`; no DMA stream, SPI, SAI, SD or USB peripheral addresses
  it. Its only other user is the bench's `bench_audio`, slot 0 lent before
  `take_audio`, also CPU.
- **Built in place already.** It is `MaybeUninit`, written whole by
  `TripleBuffer::init_slots_in_place` / `AudioShared::init_in_place` in
  `take_audio`, so NOLOAD is no change: nothing reads it before that.
- **Safe in every context.** The same `Reader`/`Writer` split and
  `AtomicU8` latest index guard it wherever it lives; ITCM changes the
  address, not the protocol. ITCM is reachable by the CPU's data side at
  zero wait from either bank (ADR 0266).
- **23,104 B freed**, 5.8× the 4 KB target, in one move. ITCM goes to
  40,536 B used, 23,976 B free.

`ENGINE` (5.9 KB) and `ORBIT` (0.9 KB) qualify too but are not needed; they
stay as a reserve for a later round, which keeps ITCM's room for a future hot
kernel (ADR 0266's note).

No product trade-off: placement only, no behaviour, no limit changes.

## Guards

- `itcm.x` gains a margin: 8 KiB of ITCM stay free (ADR 0286), as ADR
  0087's bench margin.
- `just check-itcm <elf>`: every data symbol in ITCM is on an allowlist of
  CPU-only state (`ED`, `KITS`, `KIT_VIEW`, `AUDIO`), so a DMA buffer can't
  land there unreviewed, and `AUDIO` must be there. Run on every set
  `just check` builds and on every `bank2-split`.
- The host's `memory_budget_test` reports ITCM's residents and asserts they
  fit under the margin.

## Speed

The audio reads its Part params from the snapshot every block. AXI is
cached (a hit is zero wait, a miss a line fill over the AXI bus); ITCM is
zero wait always, so the change can only remove misses. No bench was flashed
(brief: no flash); the expected change is within noise.
