# 0285. Editor protocol 3: every engine, every pad addressed, scales, choices by number

- **Status:** Proposed
- **Deciders:** owner (2026-11-18, #1014: the whole kit, SOURCE read-only, the screens on ALGO's grid), firmware

## Context
The browser editor (ADR 0276) reached ALGO only. `ed describe` walked `ALGO_FORMANT_CHAIN` with copies of each binding's resolution. `ed set` refused any Part that wasn't ALGO, and `ed since` answered a non-ALGO Part with its `part` line alone. The owner approved the editor for every engine in 0.4 (#1014). Three slice 1 loose ends ride along. #996: `GATE EXT` can't be set, because `ed set` takes one word. #997: `describe` doesn't say a param's scale, so the editor guesses it. #1002: one of its tidy-ups.

Every DRUM page edits the Part's selected pad (`BlockRef::Pad`, `PadFilter`, `PadAmp`, `PadEnv`, `PadLfo`, `PadRoute(r)`). None of these has a `disk_ident`, because a kit is stored as records (ADR 0265). ADR 0276 freezes addresses as the patch file's tokens, so DRUM needed new tokens either way. The owner chose the whole kit (2026-11-18): every pad addressed, a 12-row overview and one pad's detail picked in the editor, never on the box. The other option was to follow the box's selected pad. With that, an edit lands on whatever pad the box selects in the meantime, which is a class of sync bug.

The spec is `docs/superpowers/specs/2026-11-18-editor-every-engine-design.md` and the plan is `docs/superpowers/plans/2026-11-18-editor-every-engine.md`.

## Decision
- **`PROTOCOL` is 3.** The editor requires exactly 3. A unit on 2 shows UPDATE THE UNIT, and a higher one UPDATE THE EDITOR.
- **Every engine is described.** `ed describe <ENGINE> <page>` walks the engine's chain (`describe::chain`, an exhaustive match over `EngineType`): `ALGO_FORMANT_CHAIN`, `MODAL_PLUCK_CHAIN`, `VA_CHAIN`, `TAPE_CHAIN`, `DRUM_CHAIN`.
- **`describe` resolves cells through the box's own resolver.** For each page, each `Group` sets one `SlotCtx` axis to one variant and asks `view::view` what each cell is. The axes are FILTER kind, FX type, ENV and LFO kinds, operator kinds, MODAL's model, TAPE's machine, a pad's machine and a pad's route. A cell's axis is `axis_of(SlotBinding)`, an exhaustive match. Slice 1's per-binding copies (`filter_cell`, `fx_cell`, `slot_cell`, `op_cell`, `fixed_cell`) are gone. ALGO's schema came out byte-identical but for the new scale word (checked against slice 1's fixture). A panel is listed wherever its page has a cell on that axis, even with no cells under that variant (FX OFF, BOWED's MDL2), so the editor always knows the variant. FILTER's range is still the kind's (`FilterParams::spec` after `set_kind`).
- **Addresses** are `EdAddr { at: EdBlock, id }`. `EdBlock` is `Sound(SoundBlock)` or `Pad(PadId, PadBlock)`. An `EdAddr` exists only for a param its block holds.
  - A `SoundBlock` is a `BlockRef` with a token, never a pad's. Its token is the `disk_ident`, or **`KIT`** for the kit's own block (TONAL).
  - A pad's tokens are **`PAD<n>`** (1–12, no leading zero) followed by `""`, `_FLT`, `_AMP`, `_ENV`, `_LFO` or `_RT1`–`_RT4`. Examples: `PAD3.LEVEL`, `PAD12_FLT.CUTOFF`, `PAD5_RT2.AMOUNT`.
  - A pad's filter holds only `PadFilter::IDS`, and `parse` refuses its other idents.
  - The tokens are frozen and pinned by `chimera-core/tests/fixtures/ed_block_tokens.txt`, to which entries may only be appended.
- **The pad AMP's GATE DECAY has its own ident, `GATE_DECAY`.** It was copied from OUT's DECAY with that spec's ident, so `PadAmp` had two `DECAY`s. A pad isn't in the patch text, so no file changes. A test now holds every block's idents unique.
- **DRUM's pad pages are described once a pad.** A page is a pad's when any cell resolves against the pad. That is `pad_binding`, an exhaustive match, and it counts FLT's sub-pages, which read the pad's KIND on DRUM. The Part's pages come first, then each pad's, labelled `PAD<n>_KIT`, and so on. DRUM describes 165 pages. The pad-axis panels are `PAD<n>_FLT`, `PAD<n>_MCH` and `PAD<n>_RT<r>`; FX, ENV and LFO keep their names. DRUM has no `FILTER` panel.
- **A variant is one token.** A name's spaces are written `-` (`TONE KICK` is `TONE-KICK`). `since` writes them the same way.
- **`param` carries the scale.** The line is `param <addr> "<label>" <kind> <lin|oct> <min> <max> <step> <default>`, from `ParamSpec::scale`. The editor's guess (`Gesture.scaleOf`'s numbers) is gone.
- **Choices by number.** `ed set <addr> #<n>` sets option `n` (`EdValue::Index(OptIndex)`), checked against the cell's names: `ERR ed set <addr> has no such option`, or `takes a number` on a continuous param. `#` starts no name or number. The editor sends every choice this way, so `GATE EXT` is set (#996). Names stay accepted for a human. This supersedes ADR 0276's "choices are set by their option names, never by number".
- **`set` takes what `describe` lists** for the followed Part's engine (the `Described` mask, built per engine), and nothing else. A queued set keeps the engine it was taken for, and a Part that changed engine drops it.
- **A pad's set lands on its pad.** In the UI pass, `edits::turn` selects the named pad on the Part, runs `edit_to` (the knob's whole path, ADR 0280's CHOKE check included) and puts the selection back. This happens in one straight-line function in bank 2, so the box never sees the selection move. A CHOKE that would make a 9th group writes nothing.
- **What the box's turn refuses, `ed set` writes nothing of (ADR 0151).** The turn path's refusals that the write path doesn't make are applied in `edits::refused` before the edit: TAPE's MODEL and DIRECTION onto a machine or way a streamed source can't play, through the same `Pick::Tape(..).why()` as `turn_model` and a CC's `cc_cost`. The rest already refuse in the write: WAVE and ALG A/B in `try_set_kind` and `try_set_algs` (the formant budget), CHOKE in `edit_in` (ADR 0280). The answer is still `OK queued` (ADR 0276), and the editor sees the unchanged value. Tests set every MODEL and DIRECTION on a streamed source, a formant on every operator and every ALG A and B, each against what the box's own `why` says.
  - A refused `ed set PAD<n>.CHOKE` shows the box's toast (`9 GROUPS · KIT HOLDS 8`) as a CC's does, even when the box shows another Part.
- **Reads follow the named pad.** `value::block` reads pad `n`'s blocks from the kit, never the selected pad's. A pad's cell reads under its own context (`on_kit(kit, n).on_route(r).on_drum()`). The Sound's cells on DRUM read under pad 1's.
- **`since` is per engine.**
  - The slots are laid out per engine: the Sound's blocks in `BlockRef::ALL` order (`in_schema`, an exhaustive match), then on DRUM `KIT` and each pad in `PadBlock::ALL` order. `SLOTS` is 711, DRUM's, which is the most.
  - The panels it reports are a typed `Shown` per engine: FILTER (not on DRUM), FX, ENV1–3, LFO1–3, then OP1–6 on ALGO, `RESONATOR` on MODAL, `MACHINE` on TAPE, or each pad's `PAD<n>_FLT` and `PAD<n>_MCH` on DRUM. `PANELS` is 31.
  - The text context on DRUM is the live one with pad 1 and route 1 set, never the box's selected pad, so choosing a pad on the box answers nothing.
  - A panel change (a pad's machine relabels its knobs) answers the values whole, as a context change does.
  - TAPE's SOURCE is read-only. `describe` lists `source "<label>"`, and every whole `since` answer carries `source "<text>"`, the box's text. `ctx.src` is in the text context, so a new source answers whole. The picker is #1015's.
- **A `since` answer past one answer's bound is refused, never cut.** `ANSWER_BYTES_MAX` (`console`) is half what the console's floor rate moves in `ANSWER_MS`: 64 B × 1,000 packets/s × 1 s / 2 = 32,000 B. `since` first measures its answer, writing to a `ByteCount` and keeping nothing (`Pass::Measure`), then writes and keeps it (`Pass::Commit`). Past the bound it answers `ERR ed since too large` and keeps nothing. The editor shows THE UNIT'S ANSWER IS TOO LARGE FOR ONE READ and stops asking: asking again gets the same. `ByteCount` is bank 1's, in `fmt_buf`: a `Write` impl's vtable is bank 1's and may not point into bank 2.
- **One page walk.** `pages::Pages` walks the chain once, in `describe`'s order (the Part's pages, then each pad's), or one round alone. `find` walks only the address's own pad's pages (a Sound address: the Part's, then pad 1's). A full answer is O(addresses × that pad's pages), not O(addresses × every page × the chain). The page count is `const`: a compile-time assert holds every engine's within a `u8`.
- **One table for panel names.** `describe::PanelName` writes every panel's name, for `describe`'s `panel` lines and `since`'s alike.
- **Refusals are typed.** `ed set`'s are a `SetRefusal` (`NoSuchOption`, `TakesANumber`, `TakesAName`), each its own words.
- **Bank placement (ADR 0198, 0272).** Everything new is in `console::ed`, in the S4 group. Every `ed` function has its own `.bank2_ed.<module>.<fn>` section, and the S4 glob takes `.bank2_ed.*`. LLVM outlines nothing from a function with an explicit section (build.rs's `card_work` note). Before this, the outliner had put `OUTLINED_FUNCTION`s that call `describe::pad_prefix` and `axis_of` into bank 1, which the split refused. `SlotCtx::on_kit` and `on_drum`, `Project::select_pad`, `Kits::select` and `PadRoutes::route` are `#[inline(always)]`, so bank 1 keeps no out-of-line copy for bank 2 to call.

## After merge: measure on the unit
The bound is the console's documented floor, not a reading (#1002). Once flashed:
1. `chimera-usb.py status` reads `protocol 3`.
2. Load INIT DRUM on Part 1, then make the kit messy: run `ED_SERIAL=/dev/ttyACM0 node editor/e2e/sim.mjs`, whose DRUM pass sets every param it shows to its max and min, so it ends untidy. Then put every pad on TONE COWBELL, and every pad's filter on LADDER.
3. Time `ed since 0` from the host, the request's write to its `OK <m>`, ten times. Note the bytes and the slowest time. On the host the same answer is 24,192 B, and 2.8 ms for both passes on x86 in release.
4. Watch the box's `stats` (AUDIO LOAD) during step 3: no overrun, and the UI loop's time stays under its budget.
5. Record the numbers on #1002 and set `ANSWER_BYTES_MAX` from the measured rate if it is lower than the floor's.

## Alternatives considered
- **Follow the box's selected pad (`ed pad <n>`, `PAD.LEVEL`).** Cheaper, about 50 slots. But an edit lands on whatever pad the box selects before the UI pass. The owner chose the whole kit.
- **Describe a pad's pages once, with a pad placeholder in the addresses.** About 165 fewer pages at connect, but a new line grammar and an editor that expands templates. The schema is static, so the editor reads it once a session.
- **Keep slice 1's per-binding copies and add MODAL's, TAPE's and DRUM's.** Each copy can drift from the box. `view::view` is what the box draws, and a test now walks every page of every engine through the box's own context and compares `ed get`'s text with the cell's.
- **Quote names with spaces in `set` (#996's other option).** A quoted word in the console's word splitter. The option number is already in every `opt` line.
- **Refuse a too-large answer by an upper bound on its lines.** Each `val` line's longest is about 80 B, so DRUM's 672 addresses would be refused at about 53 KB when the real answer is 24 KB. Measuring the answer costs a second pass of the same work.
- **A pad-aware edit path in bank 1 (`edit_pad_to`).** Bank 1 is frozen. Selecting and restoring in bank 2 takes the knob's whole path unchanged.

## Consequences
- **ITCM:** `EdState`'s `since` copy grows from 256 to 711 slots. `.itcm` went from 17,432 B to 19,512 B (+2,080 B) of 63 KB.
- **Bank 1:**
  - ORBIT: 890,192 → 890,344 B (+152, built clean as `just check` builds; its line +88 then +32, from 890,224).
  - Default: 875,888 → 876,040 B (+152), within its line.
  - What grew: `ByteCount` (+74 code, and its vtable), the `GATE_DECAY` ident, and codegen shuffle (`edit_to` +22). `Project::select_pad`, `Kits::select`, `PadRoutes::route`, `SlotCtx::on_kit` and `on_drum` are inlined, so none of them is out of line for bank 2's sake.
- **Bank 2:**
  - ORBIT: 526,560 → 531,840 B (+5,280). Its S0–S3 stays at 8,320 B free; all of the growth is in S4.
  - Default and the gate build: 498,368 → 503,648 B (+5,280).
- **Answer sizes:**
  - Full `since` answers, in `console_ed_engines_test`: ALGO 6.1 KB, MODAL 3.4 KB, TAPE 3.6 KB, VA 3.2 KB, DRUM 21.4 KB. All are under the 32,000 B answer clock.
  - A messy kit (`a_messy_kit_fits_one_answer`: every pad on each machine in turn, the longest filter name, untidy values) is at most 24,192 B. Both passes take at most 2.8 ms on x86 in release; it was 18–25 ms before the one page walk.
  - DRUM's schema is 165 pages and 210 KB, read once a session; the largest page is under the clock.
- **The editor (slice 1 screens unchanged):** it speaks protocol 3, reads the scale, sends choices by number, types the five engines (an unknown one says UPDATE THE EDITOR) and keeps TAPE's source text. The other engines' screens wait for the owner's mockup approval.
- **#1002:** the "dimmed values", the measured answer bound, text-context narrowing and the gate-build answers stay open. The back-to-back answers after a stall are already counted and harmless (`owed`).

## Supersedes in part
- [0276](0276-browser-editor-protocol-ed-over-the-console-pulled.md):
  - `PROTOCOL` is 2 → 3.
  - "Choices are set by their option names, never by number" → by number too.
  - "`ed describe ALGO <page>`" → every engine.
  - "Addresses are `<BLOCK>.<IDENT>`, the block's `disk_ident`" → plus `KIT` and the pad tokens.
  - "A full answer costs … about 218 addresses" → DRUM's is about 590.
- [0280](0280-a-drum-kit-has-at-most-eight-voice-groups.md): "The console's `ed set` reaches only ALGO's cells today, so it can't set a CHOKE" no longer holds. `ed set PAD<n>.CHOKE` takes the same check, and a 9th group writes nothing.

## Sources
- Issues #1014, #996, #997, #1002; spec and plan above.
- `chimera-core/src/console/ed/`, `chimera-core/tests/console_ed_engines_test.rs`, `chimera-core/tests/console_ed_test.rs`, `editor/tests/fixtures/*-schema.txt`, `chimera-stm32/build.rs` (the `ed` group), `tools/bank1-budget.txt`, `tools/bank2-budget.txt`.
