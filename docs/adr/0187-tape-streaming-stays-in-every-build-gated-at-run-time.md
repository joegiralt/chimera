# 0187. Keep TAPE's streaming in every build, gated at run time

- **Status:** Proposed
- **Deciders:** firmware (#697, review finding M7)

## Context
The type-driven review (`tape-adversarial/types.md` M7) found the stream machinery in builds without `tape`: `pub mod stream` and `tape_engine` are unconditional, `card_dma` is built and `init`'d whenever `sd-probe` is off, and three cuts are runtime ones: the audio shell picks `NoPorts` on `!stream::STREAMS`, `serve::Rot` tests `STREAMS`, the codec's streamed-load arm has `cfg!(feature = "tape")` in a guard. It asked for `#[cfg(feature = "tape")]` on `stream::{cursor, ring, serve, slot, registry, stage, layout}`, an always-on `stream::off`, and `card_dma` behind `any(tape, bench)`.

Measured on the committed tree (5f99415f), release, `rust-size -A`:

| build | `.text` | `.rodata` | `.bss` | `.sram4` | `.ram_d2` |
|---|---|---|---|---|---|
| `--features orbit` (ships, has `tape`) | 760,568 | 136,268 | 465,136 | 64,836 | 239,948 |
| `--no-default-features` (no `tape`) | 787,512 | 122,472 | 461,808 | 64,836 | 239,464 |

What the gate could take out of the `tape`-less build, by symbol: about 10.1 KB of `.text` (`card_dma`, `stream::*`, `card_speed`) and about 104 KB of RAM (`shared::RINGS` 64,836 in SRAM4, `THIRD` 32,305 and `REGISTRY` 2,248 in D2, `card_dma`'s buffer and state 5,280). The shipping build has `tape` on, so it would save nothing there.

A trial gate on the seven modules alone broke `chimera-engine --no-default-features` in 12 files on the first pass (`project::{pool, arena, sources, streamed, streamed::load}`, `storage`, `hw`, `stream::{port, edf, admit, bench}`), before the second pass reached `tape_engine` (ungated, built on `cursor`, `ring` and `stage`), the board (`shared`, `lib`, `card_dma`, `card_speed`, `orbit`, `usb`), the shell (`audio::engine`, `bench`) and the UI (`view`, `source_cell`, `console::{put, card}`). The project pool holds the `StreamRegistry` and the streamed load path in every build. `CARD_BENCH` needs `bank2`, which brings `tape`, so the card bench is no reason to keep the ring.

## Decision
- TAPE's streaming stays compiled into every build. `stream::STREAMS` (`cfg!(feature = "tape")`) is the switch. The shells run `NoPorts` on `!STREAMS`, `Rot`'s methods test it so the code folds away, and the codec keeps its `cfg!` guard.
- A build without `tape` keeps `card_dma`, the rings and the registry in RAM. No build that ships is one of those.
- The `chimera-core` const assert (`cfg!(feature = "tape") == stream::STREAMS`) keeps the two crates' flags from drifting.

## Alternatives considered
- **The compile-time gate M7 proposed.** It needs `cfg` across about 25 files in four crates, an always-on `stream::off` for the types the pool, the ports and the UI name, and a `tape`-less test line that type-checks a second project pool. All of that would save flash and RAM only in builds that never ship (`--no-default-features`, `fault-inject`).
- **Gate only the board's statics and `card_dma`.** That would cut the RAM, but `Shared`, `start_card` and the shell's `Streams` all name the rings, so it spreads `cfg` through the board and the shell for the same unshipped builds.

## Consequences
- The `tape`-less builds type-check and link the streaming code, so they check a change to it too.
- They carry about 10 KB of flash and 104 KB of RAM they never use. If a `tape`-less build ever ships, or bank 1 or SRAM4 runs short in one, supersede this ADR with the gate.

## Sources
- (private notes) M7; #697.
- `chimera-engine/src/stream/mod.rs`, `chimera-engine/src/stream/serve.rs` (`Rot`), `chimera-engine/src/project/codec.rs`, `chimera-board/src/{lib,shared,card_dma}.rs`, `chimera-stm32/src/audio/engine.rs`.
