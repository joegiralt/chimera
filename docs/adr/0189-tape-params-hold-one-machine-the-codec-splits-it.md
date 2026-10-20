# 0189. TAPE's params hold one machine; only the codec splits it into MODEL and MODE

- **Status:** Proposed
- **Deciders:** firmware (TAPE type audit, #697, findings H2 and M3)
- **Supersedes in part:** [0178](0178-tape-model-lists-one-loop-and-ping-and-deck-holds-the-glyphs-knobs.md) § "The block keeps PLAY and its MODE" (the two in-memory fields)

## Context
ADR 0178 kept `model: TapeModel` and `mode: PlayMode` in `TapeEngineParams` so the disk codes held. In memory the pair allows `(TRON, PING)` and every other MODE off PLAY. So each reader had to check `model == Play` before trusting `mode`: `hold`, `at_root`, `needs_ram`, `TapeModel::ordered(mode)` and `reads_in(model, mode, id)` did. `stage::place::heads_in` didn't: it tested the machine bare and was right only because `needs_ram` had already returned `None` three lines earlier. The deck copied the pair again, and `model_cost(m, mode)` and `streamed_cost(m, mode)` took it as two arguments.

The enum params also decoded loosely. TYPE, WOW and RETRIG read `v as u8 == 0` as the first variant and anything else as the second, so a third variant would land silently on the second. A MODE value out of range became PING. MODEL had two value spaces under one id: `get`/`write` used the machine index (0 to 6), while `enum_code`/`set_enum_code` used the model's disk code (0, 3 to 6), and both were plain `u8`/`f32`. `Cell::default` stored a choice as `f32`.

## Decision
- **`TapeEngineParams` holds `machine: TapeMachine`.** The (MODEL code, MODE code) pair exists only at the codec: `enum_code`, `enum_ident`, `set_enum_code` and `translate_v1`.
  - MODEL's code is `machine.model()`'s.
  - MODE's code is PLAY's MODE. Off PLAY it is `kept: KeptMode`: the code the file held, or the last PLAY machine's MODE (`set_machine` keeps it on leaving PLAY). `KeptMode` is opaque: nothing outside the params' codec arms can build or read it, and no machine, cost or UI reader sees it. It exists so a saved Sound keeps its bytes.
  - `set_enum_code(MODEL, c)` makes the machine `TapeMachine::of(model, PLAY's MODE or the kept one)`, as the pair did.
  - `set_enum_code(MODE, c)` sets PLAY's MODE, or off PLAY the kept code. The decoder applies MODEL before MODE (spec order), so a file's pair loads as `TapeMachine::of(model, mode)`, as before.
- **`hold`, `at_root`, `needs_ram`, `heads_in`, `model_cost`, `streamed_cost`, `TapeMachine::ordered`, `jogs` and `wave::playable` match on `TapeMachine`.** `reads_in` is gone: `reads(machine, id)` is the one table. The deck latches `machine` at note-on in place of the pair. The strip's raster and the bank-2 UI door take a `TapeMachine`: on PLAY, the MODE glides with MODEL as before; off PLAY, the machine is the Part's.
- **`Choice` (`from_value(f32) -> Option<Self>`, `value(self) -> f32`)** is implemented exhaustively for `StretchType` (TYPE), `Wow`, `Retrig`, `PlayMode` (MODE) and `TapeMachine` (MODEL). `write` sets a choice only when the value names a variant. MODE's write sets PLAY's MODE, or off PLAY the kept code, as the pair did.
- **MODEL's two domains are named.** `MachineIndex(u8)` is MODEL's value, and `TapeMachine::from_index` takes it (saturating at WAVE, as before). `TapeModel` is the disk code. Passing a `TapeModel` code where a `MachineIndex` is wanted doesn't compile (a `compile_fail` doctest). A second doctest shows that the old `model`/`mode` pair can't be written.
- **`machine::Cell::default()` is a `CellDefault`**: `Unit(f32)` or `Choice(u8)`. A cell is built by `cell` (a fraction) or `choice` (a variant's code), and stores the enum flat (its value and a flag), because the enum's tag would add 4 B to each of the table's cells.

## Alternatives considered
- **Keep the pair and guard every reader.** That is what we had, and one reader already skipped the guard.
- **Drop an off-PLAY MODE** (save LOOP's code off PLAY). The no-legacy-compat rule would allow it, and no factory Sound holds one (the recipes reject `mode` off PLAY). But the test-support `dense` Sounds do: their random codes put TRON and the others over ONE or PING. Dropping the code shrinks each by 5 bytes and moves `sound_browser_pool_full`'s golden. Saved Sounds must stay byte-identical, so the code is kept, in a type no reader can use.
- **Keep it outside the params** (beside the Sound, or in the codec). Code that sets MODE through the `Block` (`dense`, `translate_v1`, CC) writes the params, so a memo kept elsewhere would miss those writes.
- **Seven MODEL disk codes.** ADR 0178 rejected this already: it re-keys every saved PLAY Sound.

## Consequences
- **Disk:** every file loads and saves to the same bytes. The 32 factory TAPE Sounds are pinned by CRC and round-trip (`factory_sounds_save_as_loaded`). Every (MODEL code, MODE code) pair the decoder takes round-trips byte for byte (`every_model_and_mode_code_round_trips`).
- **Memory:** `TapeEngineParams` holds two one-byte fields where it held two, so RAM is unchanged. A MODE off PLAY can't be read as a MODE, since `KeptMode` has no accessor. The old pair's fields are gone (the `compile_fail` doctest).
- **Out-of-range writes:** a direct `write` of a value no variant names is ignored, where it used to land on the last variant (TYPE, WOW, RETRIG) or PING (MODE). `Block::set` clamps to the spec first, so the UI, CC and the matrix never send one. MODEL still saturates at WAVE.
- **Bank 1 flash and RAM:** ORBIT's bank 1 shrinks by 104 B (`.text` 760,568 → 760,424, `.rodata` 136,268 → 136,308). `.bss` is unchanged at 465,136. One trap was found on the way: an exhaustive `heads_in` with its own `return None` arm made LLVM outline `Bounds::of` into bank 1 (+412 B). Merging the arms the guard refuses into ONE's keeps it inlined in bank 2.
- **Debug** prints `model` and `mode` as the disk has them, so `neutral_v1.txt` holds.
- **PR C** rewrites the deck around one latched `TapeMachine` state. Task B3 types `NeedsRam::Machine`, which still carries a `TapeModel` here.

## Sources
- #697 (the type audit), (private notes) H2 and M3.
- ADR 0178 (the flat list), 0170 (the retired codes, `translate_v1`).
- `chimera-engine/src/dsp/tape_engine/{params,machine,mod}.rs`, `chimera-engine/src/stream/stage/place.rs`, `chimera-core/src/ui/tape_strip/raster.rs`, `chimera-core/tests/tape_machines_test.rs`, `chimera-desktop/tests/factory_tapes_test.rs`.
