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
