# 0147. Stream long TAPE sources from the card by DMA, behind heads held in RAM

- **Status:** Proposed
- **Deciders:** owner (streaming is in; decisions 1, 5 and 8, 2026-10-15), firmware
- **Supersedes in part:** [0093](0093-tape-plays-12-bit-card-sources-through-six-ram-models.md) (TRON's heads sized from UI-loop refills, and the core spec's § 8: refill in the UI loop, rings in D2)

## Context
The arena holds 153,600 B of sources: 4.3 s at 24 kHz. Mellotron-style zone sets (3.5 MB) and long beds and reels need the card.

Today all card I/O is polled from the UI loop. A 4 KB read busy-waits the UI about 5 ms, so 8 streams would take 380 ms of every second. The UI loop runs 22–26 ms a pass under load and stalls up to 100 ms on a full redraw.

M1 (2026-10-15, owner's SDHC card, 62 % audio load) measured random 4 KB reads at median 5.3 ms, p95 5.4–6.3 ms, p99 6.3 ms, max 6.66 ms, and 778 KB/s interleaved over 10 streams with 0 overruns. Load barely moves the latency.

Writes stall every read for up to 250 ms (the SD spec allows it).

## Decision
- **One card layer owns SPI2.** A bus word (Free, Ui, Stream).
  - The UI's `FatStore` runs through `SharedCard`, which takes the bus for one `BlockDevice` call.
  - The stream side takes it for one DMA transfer.
  - Streams always beat the UI (owner, 2026-10-17): a waiting UI gets the bus only between transfers when no stream wants a fill. A slower UI while playing is acceptable.
  - Bank 1, so the card serves the UI with bank 2 closed.
- **DMA2 (SPI2 RX and TX) and a card interrupt at level 12** (under audio and MIDI, over SysTick) run multi-block reads. Each DMA burst clocks the least the read still needs and lands after the data so far; the pure walk finds the gaps and tokens in it and packs the blocks in place, so a chunk takes a few card interrupts, not one a token scan and a block. The busy after CMD12 is polled briefly before any DMA drain. The audio wakes the interrupt by pending it. The pure SPI-mode protocol is in `chimera-fat`, host-tested.
- **Heads** at every place a note can start live in the arena: START and loop start, zone starts, key-reachable splices. They are in the source's `held`, on the meter, under the gate. **100 ms by default**, data in the descriptor (`head_frames`, plus a `heads` count for a streamed `Plain`).
- **Chunks of 8 `SourceData` records** (4,032 B, 2,688 frames, 112 ms at 24 kHz) are read by raw block address. The file's runs are resolved once (≤ 32, else refused as fragmented). Each record's tag is checked as it is de-framed.
- **One ring per voice** (3 slots + 4 guard frames, so a read late by up to a chunk is covered by the spare). SRAM4 holds every slot word and slots A and B (8 × 8,104 B); slot C's bytes are in D2 (8 × 4,038 B), as SRAM4 can't hold three. DMA lands in a 4.7 KB staging area in D2's non-cacheable DMA region (grown to 16 KB). The card interrupt copies samples into the ring, so the rings stay cacheable and need no maintenance. Streams ≤ voices = 8.
- **Slots** move Empty → Wanted (audio, with a deadline in blocks) → Filling → Ready (card) → Empty (audio). One atomic word each, with a generation for cancels. The card serves the **earliest deadline first**.
- **Admission** bills a streamed note in card bytes a second at the fastest speed it can reach: routes, bend, glide, 33 cents of wow. The budget is 315,978 B/s, bench B4's N × 41,143 × 1.2 × 0.8 at N = 8 (eight 24 kHz streams held at 1× with 0 empty rings); raising it needs B4 re-run at the higher rate, and CARD TEST may later set it per card. **Over budget, the new note steals the oldest streamed voice**; if none can go, it is refused before allocation.
- **Failure is a designed drop-out, never a click or a stall:**
  - An underrun fades out over 3 ms, keeps tape time moving, and fades in when the chunk lands.
  - A start with no head (a cold start) fades in when its first chunk lands.
  - A read error retries once, then faults the layer: streams drop out, new streamed notes are refused, `CARD READ FAILED` shows, and the UI re-inits.
- **Every card write while streams play quiets them** (decision 8, applied to all writes): streamed voices fade out over 10 ms with tape time moving, the write runs, and they fade back in. RAM voices and other engines play on.
- **Streamed:** PLAY (forward until slice 5 adds backward), TRON, REEL. **RAM only:** KIT, SCRUB, STRETCH, WAVE, GRAIN. 12-bit mono only.
- **Bench before build** (slice 0, the real driver): B2 sets the chunk size, B3 the head length, B4 the budget, B5 confirms the write quiet, B6 the UI under streams, B7 the cost of backward reads.

## Alternatives considered
- **Polled reads from the UI loop** (the core's plan). Eight streams cost 380 ms of every second of UI time. The loop's 22–100 ms passes need 150 ms+ heads, unbounded during a save.
- **60 ms heads.** An 8-note chord queues about 60 ms of reads at M1's max before its last first refill, with no room for pitch-up.
- **Rings in D2, DMA straight into them.** It needs cache maintenance per refill, or non-cacheable rings that the audio reads slowly. And the record framing must still be stripped.
- **A new unframed source format for streaming.** It would re-name every factory source (`tapes-v1` is pinned by CRC). De-framing costs a 4 KB copy per chunk in the card interrupt.
- **Refuse over-budget notes.** Silence on a key. Stealing matches the voice allocator.
- **Let saves wait for silence.** A save could wait forever under a held bed; the owner chose fades.
- **Per-key TRON tapes (35 heads, 126 KB).** Almost the whole arena for one instrument; zones give 8 s on every key at a third of the RAM.

## Consequences
- SRAM4 (64 KB) is spent on rings. D2 gives 8 KB more to the DMA region and about 2 KB to the stream registry.
- About 8 KB of bank-1 flash; the streamed read path is bank 2's (TapeDeck).
- `stats` gains `stream_underruns`, `stream_cold`, `stream_faults`, `stream_frame_errors` and `card_quiet_ms_max`.
- A streamed note costs a bill in bytes as well as cycles; slow cards get fewer streams.
- Long files need slice 2's resumable `put` (`put`'s limit is 1 MiB, ADR 0106).
- A save while beds play is audible: a tape lifted and dropped.

## Sources
- `docs/superpowers/specs/2026-10-15-tape-sampler-design.md` § 5; `2026-10-15-tape-slice1-streaming-design.md`.
- (private notes) (§ 1, M1 result); `tape-rework/adversarial-review-streaming.md` (B1–B10).
- RM0433 (DMA, DMAMUX1, SPI, the bus matrix, SRAM4); SD Physical Layer Simplified Specification (SPI mode: CMD18, CMD12, data tokens, CRC16, write-busy).
- Precedent: Octatrack Flex (RAM) vs Static (streamed) machines; Kontakt's direct-from-disk preload buffers. Behaviour only (ADR 0114).
