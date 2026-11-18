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
- **Reads follow the named pad.** `value::block` reads pad `n`'s blocks from the kit, never the selected pad's. A pad's cell reads under its own context (`on_kit(kit, n).on_route(r).on_drum()`). The Sound's cells on DRUM read under pad 1's.
- **`since` is per engine.**
  - The slots are laid out per engine: the Sound's blocks in `BlockRef::ALL` order (`in_schema`, an exhaustive match), then on DRUM `KIT` and each pad in `PadBlock::ALL` order. `SLOTS` is 711, DRUM's, which is the most.
  - The panels it reports are a typed `Shown` per engine: FILTER (not on DRUM), FX, ENV1–3, LFO1–3, then OP1–6 on ALGO, `RESONATOR` on MODAL, `MACHINE` on TAPE, or each pad's `PAD<n>_FLT` and `PAD<n>_MCH` on DRUM. `PANELS` is 31.
  - The text context on DRUM is the live one with pad 1 and route 1 set, never the box's selected pad, so choosing a pad on the box answers nothing.
  - A panel change (a pad's machine relabels its knobs) answers the values whole, as a context change does.
  - TAPE's SOURCE is read-only. `describe` lists `source "<label>"`, and every whole `since` answer carries `source "<text>"`, the box's text. `ctx.src` is in the text context, so a new source answers whole. The picker is #1015's.
- **Bank placement (ADR 0198, 0272).** Everything new is in `console::ed`, in the S4 group. Every `ed` function has its own `.bank2_ed.<module>.<fn>` section, and the S4 glob takes `.bank2_ed.*`. LLVM outlines nothing from a function with an explicit section (build.rs's `card_work` note). Before this, the outliner had put `OUTLINED_FUNCTION`s that call `describe::pad_prefix` and `axis_of` into bank 1, which the split refused. `SlotCtx::on_kit` and `on_drum` are `#[inline(always)]`, so bank 1 keeps no out-of-line copy for bank 2 to call.

## Alternatives considered
- **Follow the box's selected pad (`ed pad <n>`, `PAD.LEVEL`).** Cheaper, about 50 slots. But an edit lands on whatever pad the box selects before the UI pass. The owner chose the whole kit.
- **Describe a pad's pages once, with a pad placeholder in the addresses.** About 165 fewer pages at connect, but a new line grammar and an editor that expands templates. The schema is static, so the editor reads it once a session.
- **Keep slice 1's per-binding copies and add MODAL's, TAPE's and DRUM's.** Each copy can drift from the box. `view::view` is what the box draws, and a test now walks every page of every engine through the box's own context and compares `ed get`'s text with the cell's.
- **Quote names with spaces in `set` (#996's other option).** A quoted word in the console's word splitter. The option number is already in every `opt` line.
- **A pad-aware edit path in bank 1 (`edit_pad_to`).** Bank 1 is frozen. Selecting and restoring in bank 2 takes the knob's whole path unchanged.

## Consequences
- **ITCM:** `EdState`'s `since` copy grows from 256 to 711 slots. `.itcm` went from 17,432 B to 19,512 B (+2,080 B) of 63 KB.
- **Bank 1:**
  - ORBIT: 890,192 → 890,336 B (+144; the line +112, from 890,224).
  - Default: 875,888 → 876,088 B (+200), within its line.
  - The growth is `Project::select_pad` and `PadRoutes::route` out of line now that bank 2 calls them, the `GATE_DECAY` ident, and codegen shuffle.
- **Bank 2:**
  - ORBIT: 526,560 → 531,968 B (+5,408). Its S0–S3 stays at 8,320 B free; all of the growth is in S4.
  - Default: 498,368 → 503,776 B (+5,408).
- **Answer sizes:**
  - Full `since` answers, in `console_ed_engines_test`: ALGO 6.1 KB, MODAL 3.4 KB, TAPE 3.6 KB, VA 3.2 KB, DRUM 21.4 KB. All are under the 32,000 B answer clock.
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
