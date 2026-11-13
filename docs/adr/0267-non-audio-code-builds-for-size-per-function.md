# 0267. Non-audio code in the fast crates builds for size, per function

- **Status:** Accepted (accepted by the owner, 2026-11-12/13; supersedes in part 0108: its rejection of `#[optimize(size)]` per function as "nightly only")
- **Deciders:** owner (2026-11-12: "the size optimization ADR, that's also allowed. The thing is that the sounds can't change. That's what's important."), firmware

## Context
ADR 0108 split the firmware by crate: `chimera-engine` and the binary build at opt-level 2, the UI's crates at "z". It rejected `#[optimize(size)]` per function because it needs nightly. The firmware builds on nightly now, so that reason is gone.

`chimera-engine` holds the DSP and also the model the audio never runs: the project, its codec and store, the card's file formats, the Sound records, ORBIT's song records and RANDOMIZE. ADR 0108's alternatives priced moving them to a "z" crate at about 40 KB, too big a move then. The flash audit (2026-11-12) measured the attribute on them instead: about −20 KB of bank 1, with no audio function changed. At this branch's head (`21e758ea`) the ORBIT build has 45,816 B of bank 1 free and 1,440 B of bank 2, under ADR 0251's 8 KB floor.

On this nightly (rustc 1.100.0-nightly 2026-08-27) the attribute applies to functions only. On a module or an `impl` block it is an error.

## Decision
1. **What may build for size.** `#[optimize(size)]` (`#![feature(optimize_attribute)]` in `chimera-engine`) is allowed on:
   - code that neither the audio interrupt (`DMA1_STR0`) nor MIDI's (`USART1`, `OTG_FS`) reaches, by a direct or tail call or through a door or other pointer;
   - the drum strike's setup (`DrumEngine::strike`, `KernelState::strike_in_place`). It runs once per hit, not per sample, and the null test checks its output.

   The hot audio paths stay at opt-level 2, as ADR 0108 says.
2. **Sound must not change.** The audio goldens, the screen goldens (`cargo test -p chimera-core --tests`) and the drum nulls (`cargo test -p chimera-utils drums::`, 72 at −inf) stay bit-identical, or null below −120 dB. A size lever that changes them is left out.
3. **Applied per function, by module.** Each function with a body in these files carries the attribute (test modules aside): `project/**`, `storage/**`, `preset.rs`, `orbit/records.rs`, `randomize.rs`, `randomize/**` and `glyph.rs` (859 functions; `project::test_support`, host only, aside), plus the two strike functions. Nine functions keep opt-level 2 under a comment: at size their frames grow by 0.4–1.8 KB on the stack check's deepest chain (`Sound::neutral`, `neutral_params`, `SoundCheck::step`, `Sound::plain_init`, `load_into`, `stream_sources`, `load_kind`, `SourceLayout::whole`, `Records::record`). `LibId::new`, `UserId::new` and `FactoryId::new` keep opt-level 2: the interrupt walk reaches `LibId::new`, and at size it called the other two and `RangeInclusive::<u32>::contains` out of line; back at opt-level 2 it is identical to before this ADR (the final review, 2026-11-13). `Pool::source_pass` keeps opt-level 2 too: at size, the bench's closure gets a `dyn FnMut` vtable in bank-1 rodata that points into bank 2, and `door-check` fails it on `bench-tape`. A new function in these modules takes the attribute.
4. **How reachability is checked.** It is checked on the ORBIT ELF, before and after, with the scripts in the flash audit's notes:
   - Walk direct and tail calls from `DMA1_STR0`, `USART1` and `OTG_FS` (`reach.py`).
   - Compare every function in the ELF, not only those the walk finds, so door and `dyn Block` targets are covered too. The comparison masks placement: branch offsets, outlined-function numbers, address `movw`/`movt` and literal-pool offsets, `(.N)` name suffixes and `0xd4d4` padding (`cmpall.py`).

   A function counts as changed only if its instructions differ once placement is masked. The rule holds when:
   - only the size-optimised functions changed, plus functions the audio does not reach (their callers are UI or load paths);
   - every function the interrupts reach is identical, except the two strike functions.

   `just check`'s stack check must also pass: size-optimised code can grow a frame.
5. **Bank-2 code stays at opt-level 2 where the outliner would cross the door.** Each minsize function's repeated sequences become outlined functions in bank 1. A bank-1 outline that tail-calls a bank-2 function fails `door-check` (ADR 0175). The LFO and TAPE glyph drawers (`lfo_glyph`, `tape_glyph`) did that (16 relocations) and stay out. ORBIT's door module stays out too: its `block`, `realtime` and `live` entries run from the audio and MIDI interrupts.

## Alternatives considered
- **Move the non-audio model to a "z" crate (ADR 0108's alternative):** the same saving, but it means moving most of the model and redesigning the project's witnesses.
- **A module-level attribute:** rustc refuses it on this nightly.
- **A proc-macro that adds the attribute to every function in a module:** it needs two more unstable features (custom inner attributes, non-inline modules in macro input) and a proc-macro crate, to save writing one line per function.
- **opt-level "s" or "z" on `chimera-engine`:** slows the DSP 10–40 % (ADR 0108).
- **Size-optimise `lfo_glyph`, `tape_glyph` and `orbit::door` too (the audit's extras, ~16 KB more of bank 2):** the glyphs cross the door (point 5) and ORBIT's door is audio-reachable. Taking them needs outliner work first.

## Consequences
- ORBIT build (`CHIMERA_STAMP_CLEAN=1`): bank 1 871,688 → 847,320 B (−24,368; 45,816 → 70,184 B free), the strike's share about −1.6 KB; bank 2 1,440 → 8,672 B free (+7,232). The other sets drop 22,624–24,344 B. Every set's budget line drops by its measured bytes, each naming this ADR.
- Bank 2 is back above ADR 0251's 8 KB floor.
- The project, store and codec paths run minsize code. Loads and saves are slower by an amount not measured, and they are card-bound anyway.
- The drum strike setup is smaller and slower per hit (once a hit, not per sample). The strike's output is bit-identical (drum nulls at −inf).
- The ORBIT build's nested stack is 62,824 B of the 63,232 B limit (62,760 before). The pointer charge is 16,296 B (16,392 before). At size, the nine functions in point 3 pushed it to 67,672 B, over the limit. The stack check guards that. On `bench-algo,master-tape` the move put `take_ui`'s address on a data word in the ALGO block tables (bytes 01 04 05 08), which the check took for a pointer and charged as the deepest pointer target (71,404 B). `tools/stack_depth.py` now lists it in `NEVER_TAKEN`: it is called once at boot, by name. That set is 54,300 B.
- The attribute ties the code to nightly until `optimize_attribute` is stabilised.

## Sources
- ADR 0108, 0175, 0198, 0251, 0265.
- The flash audit's notes: (private notes) (`audio_reach.txt`, `callers.sh`, `cmpfn.py`, `reach.py`), and this ADR's: (private notes) (`cmpall.py`, `cmpn_lib.py`, `upreach.py`, `addopt.py`, the ELFs).
- Branch `drum-size` 075f7569 (the strike's first size build, −1,646 B).
