# 0008. Engines are persistent; never constructed in the audio interrupt

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
The first refactor draft used `enum Engine { Pizza(..), Fm(..), Modal(..) }`
and built a new variant on engine change. Measured: `ModalEngine` is
66,752 B. Building it in the DMA ISR can put 66 KB+ on the stack (more in
debug builds); there is no stack guard, and the stack shares the 512 KB RAM
region with `.bss`, so overflow silently corrupts memory. The enum saved only
~1 KB over holding every engine.

## Decision
`Voice` keeps one persistent instance per engine inside an `Engines` struct;
all dispatch is one exhaustive `match` per method in that struct. VCA use and
voice lifetime are separate, explicit per-engine rules.

## Alternatives considered
- **Enum-of-engines with construct-and-move** — stack overflow risk.
- **Enum with in-place reset (`ptr::write`)** — unsafe code for ~1 KB saving.

## Consequences
Memory is the sum of engines. Fine for one voice; Modal's buffers must move
to a shared pool before polyphony.

## Sources
Adversarial review round 1 of the engine refactor spec (measured sizes);
`chimera-stm32/memory.x`.
