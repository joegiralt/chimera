# 0108. The audio path builds at opt-level 2, everything else at "z", split by crate

- **Status:** Accepted (owner, 2026-10-08)
- **Deciders:** owner (flash diet, #336); firmware

## Context
Flash is 896 KB. The target: at least 150 KB free on the shipping build with
ORBIT, before reverb types and the VA engine. At main 95a17024 the release build with ORBIT left
29,284 B free and the STRESS+ORBIT bench 28 B. Reverb types and the VA
engine need room. #336's on-unit bench (orbit da9afe2a) priced the
global options: opt-level "s" everywhere saves 261 KB but costs the hot
DSP 10–40 % (MODAL +33 %, A16+17 +10 %, MIX +40 %, BUS +36 %), which
breaks the voice counts (ADR 0031). Fat LTO in one codegen unit (ADR
0089) was already in.

Cargo sets opt-level per package, on stable. A function compiles at the
level of the crate whose codegen unit holds it: a non-generic,
non-`#[inline]` function in its defining crate; a generic or `#[inline]`
one in each crate that instantiates it. rustc also copies small functions
into their callers' crates on its own (cross-crate inlinable), like
`#[inline]`. rustc marks each with the
crate's `optsize`/`minsize`, and LLVM's inliner and unroller follow those
per function. Fat LTO's own pass pipeline runs at the binary crate's
level: with the binary at "s", the hot DSP shrank 12 % even with its own
crate at 2 (`Dirty::process` 1,309 → 723 instructions), so the binary
stays at 2.

`chimera-core` held both the DSP and the UI, and the UI's generics were
instantiated in `chimera-stm32`, the binary. A size profile could reach
neither without moving code.

## Decision
Four crates, two profiles (`[profile.release.package.*]` in the workspace
`Cargo.toml`):

| Crate | Holds | opt-level |
|---|---|---|
| `chimera-stm32` (bin) | `main`, the audio interrupt (`audio/`), MIDI DIN, the panic halt, the bench, the filter probes | 2 (the LTO pipeline's) |
| `chimera-engine` | the DSP, the Instrument, ORBIT, the project, storage, params: everything `chimera-core` held but the UI and console | 2 |
| `chimera-core` | the UI and the console; for now re-exports every `chimera-engine` module under its old path (`chimera_core::dsp` is `chimera_engine::dsp`) | "z" |
| `chimera-board` | the boot, the UI loop (`synth`), display, controls, card, USB console, DFU, the probe's UI side | "z" |

Third-party crates, `chimera-hal`, `chimera-theory` and `chimera-fat`
stay at 2: "z" there saved nothing (−256 B to +1 KB) and `libm` and
`Key::chord` are on the audio path.

The rules:
1. Code the audio interrupt runs lives in a fast crate. The binary and
   `chimera-engine` are the fast crates; a new crate is small only if
   the audio never reaches it.
2. A small crate never instantiates a generic the audio runs (its closure
   in `Instrument::run_block`, say): the binary does.
3. What the audio calls in a small crate is generic or `#[inline]`, so
   it compiles in the caller (`probe::measure`, `orbit::Runner`). Such a
   function whose soundness rests on being called only from an interrupt
   is an `unsafe fn` with that contract (`probe::measure`,
   `controls::isr_tick`), and the binary's call says why it holds.
4. The binary reaches the UI loop through plain function pointers
   (`chimera_board::Shell`), so `synth` is not generic over them and is
   instantiated in `chimera-board`.
5. `tools/stack_depth.py` fails the stack check if `DMA1_STR0` or
   `USART1` reaches, by direct or tail calls (conditional ones too), any
   function whose name holds `chimera_board` or `chimera_core` (`SMALL`):
   a small crate's own code or a generic instantiated with its types.
   Its limits: calls through a pointer are not followed (the audio tree
   has two, a `dyn Block` and `TapShape`'s, both into `chimera-engine`
   today); code inlined from a small crate is invisible to it, by rule 3;
   and a rule-3 generic compiled in the binary still carries the small
   crate's path, so it passes only while LLVM inlines it.
6. Only the models the engine needs move down to it, nothing of the
   panel's or the screen's: the focus glyph's names (`glyph`), `FmtBuf`,
   THEME's and MIDI CONFIG's values with their disk codes and blocks, and
   the prompt's answers, `Choice` and `Said` (`confirm`), so the
   only-a-pill guarantee stays a module-private constructor. THEME's
   colours, PWM duty and ILI9341 gamma tables stay in
   `chimera_core::ui::theme_settings`, the pills' words in
   `prompt::Pill`, and `Choice::input` takes the picking encoder's turn,
   not PreenFM3's controls. The engine depends on no graphics crate.
7. The binary imports the engine as `chimera_engine`. `chimera-core`'s
   re-exports are transitional, for the UI's callers (the desktop and the
   tests in `chimera-core/tests`) until they import the engine directly.

## Alternatives considered
- **opt-level "s" or "z" everywhere:** −261 KB, but 10–40 % on the hot
  DSP (#336's bench).
- **The binary at "s", the DSP's crate at 2:** the LTO pipeline at Os
  re-optimises the DSP anyway (12 % fewer instructions on the audio
  path, unmeasured on the unit).
- **A DSP-only crate at 2, the rest of `chimera-core` at "z":** the DSP
  depends on params, the project, storage and ORBIT, and they on it; the
  cut would move most of the model and redesign the project's witnesses.
  Measured as the ceiling (`chimera-core` at "z" whole, hot path aside):
  about 40 KB more than this decision. Left for later.
- **`#[optimize(size)]` per function, or `build-std` with
  `panic_immediate_abort` (#336 #3):** nightly only.
- **`#[inline]` on cold UI functions to move their codegen into
  `chimera-board`:** works, but places code by an inlining hint.

## Consequences
- Free flash (B): shipping 50,988 → 172,464; orbit 29,284 → 152,620;
  the tightest bench, STRESS+ORBIT, 28 → 127,132. The target is met in
  kB (152.6 kB), not in KiB (149.0 KiB). `chimera-board` alone
  gave 66 KB of it, `chimera-core` at "z" 57 KB more.
- The audio interrupt's call tree is the same code: 153 of its 157
  functions have the same instruction sequence. `DMA1_STR0` is 35
  instructions shorter and `render_half` 1 (registers around the
  counters, whose statics moved crates; once a block), `TapeEngine::start`
  1 (alignment), a bounds-check stub the same length. Every filter
  probe's static loop count (`tools/loop_count.py`) is unchanged, and the
  audio and screen goldens pass in a release build.
- The UI, the UI loop and SysTick's key scan run "z" code: slower than
  before by an amount not measured. The UI frame time and the console's
  answers want a look on the unit.
- Crossing the crate boundary duplicates a few generic instances (each
  crate keeps its own copy; `Block::snap` and kin, 1.7 KB).
- Items the UI used from the model by `pub(crate)` are `pub` now
  (`OrbitSong::land`/`capture`/`harvest`, `still_over`, `SceneAsk::of`,
  `RecordWriter::copy`, `NUMERALS`, `BEATS_PER_BAR`), and `in_place`'s
  helpers are public for `UiState`'s constructor. `ModState` takes the
  matrix's columns, not `MatrixState` (`MatrixState::sync_into`). THEME's
  looks are free functions over the engine's values (`palette(&t)`,
  `tables(g)`, `duty(b, max)`, `accent_color(a)`, `ground(b)`).
- The stack check now decodes the M7's FP instructions
  (`--mcpu=cortex-m7`), so `vpush` frames count: orbit's nested sum is
  40,720 B and the worst set's (STRESS+ORBIT) 49,832 B of 63,232.
- `just check` tests, lints and stack-checks `chimera-engine` and
  `chimera-board` beside the others.

## Sources
- #336 (the audit and the on-unit bench), ADR 0089 (fat LTO), ADR 0031
  (voice counts from the bill), ADR 0025 (stack), ADR 0066.
- The report: (private notes).
- rustc: `-C opt-level` sets the crate's functions' `optsize`/`minsize`;
  fat LTO runs the pass pipeline at the final crate's level.
