# 0163. The VA MOD oscillators live in bank 2

- **Status:** Proposed
- **Deciders:** owner (new mode DSP over about 1.5 KB of bank 1 goes behind the door; ruling R8, 2026-10-19), firmware

## Context
VA MODE FM (ADR 0159) added OSC 1's `MOD` render, `VarOsc::render::<64, true, false, true, true>` and its three runs, to bank 1: about 9 KB on the ORBIT build. PHASE (#602) reuses that instance with `k = (0, 1)`; SOFT SYNC (#604) adds OSC 2's reversing instance, `<64, false, true, false, true>`. The owner's rule puts new mode DSP of more than about 1.5 KB behind bank 2's door (ADR 0137), as the reverb's engines went (ADR 0142). A `MOD` instance is a monomorph of the same methods as the plain ones, so its symbols carry the plain ones' paths: a linker glob on `VarOsc::run` would take MIX's runs too.

## Decision
- **The `MOD` path is one module, `va::osc::door`.** It holds each `MOD` render the engine plays as a plain function (`osc1_mod`) and `run`, the only instances of `VarOsc::run`'s body with `MOD` (`run_in`, inlined). `VarOsc::render` and `run` are thin `#[inline(never)]` wrappers over `render_in` and `run_in`; `render_in` sends a `MOD` run to `door::run` and a plain one to `VarOsc::run`, so the plain monomorphs keep their names, code and bits. An entry inlines `render_in`, STABILITY's jitter draw built inside it (`1 + jd·word`), so its fn type is concrete (`ModRender`).
- **With `bank2`, `bank2.x` links the module whole** (`*(.text.*3osc4door* .rodata.*3osc4door*)`), and `Bank2Door::va`, a `VaDoor { osc1_mod }`, is the way in. `VarOsc::block::<_, true>`, `enter::<true>`, `end_shape` and the other shared helpers stay in bank 1; bank 2 calls them. Without `bank2` the glob is left out and `render_fm` calls `door::osc1_mod` directly in bank 1.
- **`VaEngine::render_fm` is bank 1's side.** OSC 2 renders plain and is scaled as before; OSC 1's render runs inside `DOOR_CELL.with`. Closed (no door), the voice's block is silent and returns before the operator: OSC 1's state waits as it was, the MODE duck runs on in `render`, and every other MODE, which never touches the door, plays as before. No heap, no blocking, no panic.
- **Closed, MODE reads `NEEDS UPDATE` at FM** (the focus band whole, the cell `UPDATE`), the reverb TYPE's way. `chimera_core::bank2::closed_text` now takes the shown value: it names VA's MODE when the value's MODE is `in_bank2()` (FM today). Any `Bank2Status` but `Open`; no status (the desktop, bench sets) never. Desktop's `CHIMERA_BANK2=closed` shows it.
- **Adding SOFT SYNC's instance** is one function and one field, no glob: in `door.rs`, `pub fn osc2_rev(...)` calling `o.render_in::<BLOCK_SIZE, false, true, false, true>(...)` like `osc1_mod`, a field `osc2_rev: ModRender` in `VaDoor` and `DOOR`, its call site through `DOOR_CELL.with` (directly without `bank2`), and `VaMode::SoftSync` in `in_bank2`. Its runs land in `door::run` and the module's glob takes them. PHASE adds only its `in_bank2` arm.
- **Adding a whole MODE render** (LOOPFM's `VaEngine::render_loopfm` with `lbfm::render_loop`, ruling R12) is one entry plus one glob: a `VaDoor` field of its own `fn` type (a `VaEngine` method's signature is fine: `fn(&mut VaEngine, &mut [f32; BLOCK_SIZE], ...)`), the render `#[inline(never)]`, its call site in `render`'s match through `DOOR_CELL.with` (silent on `None`, the duck untouched), one `bank2.x` glob on its module or symbol (`*(.text.*4lbfm* .rodata.*4lbfm* .text.*8VaEngine13render_loopfm*)`, behind `CARGO_FEATURE_BANK2` with the `va` line), and its `in_bank2` arm, which labels MODE. Code it shares with the plain MODEs stays in bank 1.

## Alternatives considered
- **Globs on the monomorphs' mangled const generics** (`Kb1_` in the v0 name): they name argument order, not intent, and break when a generic is added or reordered.
- **The whole `render_fm` or VA behind the door:** a closed bank 2 would silence MIX and INIT (ADR 0142's reason).
- **Leave FM in bank 1:** 7 to 9 KB a `MOD` instance, two of them by SOFT SYNC; against the 1.5 KB rule.
- **A `dyn FnMut` jitter draw in the door's fn type:** an indirect call a wrap; building the closure in the entry costs nothing.

## Consequences
- Bank 1 of the ORBIT build: 865,780 → 858,748 B (51,724 → 58,756 B free, +7,032 B). The `.bk2` body grows from 121,152 to 128,192 B.
- While bank 2 isn't open, FM (and PHASE, SOFT SYNC as they join) is silent and its MODE says so; a Sound in FM loads and is silent; the other MODEs play.
- FM's OSC 1 runs from bank 2 every block a voice plays FM; parity with bank 1 is expected (ADR 0142's Gate 3), unmeasured.
- The `MOD` runs' symbols move to `va::osc::door::run::<...>` and the entry `door::osc1_mod`: the bill count (`bills2.py`'s `mod_master` role, ADR 0159 point 8) reads them there.
- The bench sets without `bank2` (`bench-stress,orbit`) keep FM in bank 1 and gain nothing.

## Sources
- ADR 0137 (the door), ADR 0142 (the reverb's move, its pattern), ADR 0141 (TAPE Parts closed), ADR 0158 (the `MOD` path), ADR 0159 (FM); ruling R8 (private notes).
- `chimera-engine/src/dsp/va/osc/door.rs`, `VaEngine::render_fm`, `chimera-stm32/build.rs` (`bank2.x`), `chimera-engine/tests/va_proxy_test.rs`, `chimera-core/tests/bank2_status_test.rs`.
