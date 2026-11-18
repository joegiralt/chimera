# Browser editor, every engine: implementation plan

**Goal:** `ed describe/get/set/since` for MODAL, VA, TAPE and DRUM, plus protocol 3 (scales, choices by number). The Elm screens follow once the owner approves the mockup.

**Spec:** `docs/superpowers/specs/2026-11-18-editor-every-engine-design.md`, approved 2026-11-18.

## Plan rulings
- **`describe` resolves cells through the box's own resolver,** `ui::view::view(def, slot, &ctx)`, with one `SlotCtx` per variant (a `Group` sets the axis it varies). The per-binding copies in `describe.rs` (`filter_cell`, `fx_cell`, `slot_cell`, `op_cell`, `fixed_cell`) go away, so describe and the box cannot disagree on any engine. The one exception is FILTER and the pad's MODE: their range is the kind's (`FilterParams::spec` after `set_kind`), as today.
- **Addresses** are `EdAddr { at: EdBlock, id: ParamId }`. `EdBlock` is either `Sound(SoundBlock)` or `Pad(PadId, PadBlock)`:
  - `SoundBlock` is a `BlockRef` with a token: its `disk_ident`, or `KIT` for `BlockRef::Kit`. It is built only by `SoundBlock::new`, which refuses pad blocks.
  - `PadBlock` is `Pad | Filter | Amp | Env | Lfo | Route(RouteId)`. Its tokens are `PADn`, `PADn_FLT`, `PADn_AMP`, `PADn_ENV`, `PADn_LFO` and `PADn_RT1`…`PADn_RT4`.
  - A pad filter holds only `PadFilter::IDS`, and `parse` refuses its other idents.
  - A fixture pins the tokens.
- **DRUM pages edit a pad** when any cell resolves against the pad (`per_pad(def)`: an exhaustive match over `SlotBinding`; FLT's sub-pages count, since on DRUM they resolve against the pad's KIND). Such a page is described once a pad: `page <n> PAD3_KIT of <count>`. Its pad-axis panels (`PADn_FLT`, `PADn_MCH`, `PADn_RTr`) and its fixed panel carry the pad. FX, ENV and LFO panels keep their names.
- **The panels `since` reports** are a typed `Shown` per engine: FILTER, FX, ENV1–3, LFO1–3, OP1–6 on ALGO, `RESONATOR` on MODAL, `MACHINE` on TAPE, `PADn_FLT` and `PADn_MCH` on DRUM. `PANELS` is the most any engine has.
- **Slots** are laid out per engine, in `BlockRef::ALL` order for the Sound's blocks, then KIT, then the pads 1–12, each in `PadBlock` order. A pad filter takes `IDS.len()` slots.
- **Text context on DRUM** is the live context with pad 1 and route 1 set (`on_kit(kit, PAD1)`), never the box's selected pad, so playing or selecting pads doesn't make every answer whole. A pad's cells read under its own context (`on_kit(kit, n).on_drum()`).
- **A pad edit** is applied in the UI pass on the named pad: the Part's selected pad is set to it, `edit_to` runs (the knob path, ADR 0280's CHOKE check included), and the selection is put back, in one straight-line function in bank 2. The box never sees the selection move.
- **`set` takes what `describe` lists** for the followed Part's engine (the `Described` mask), and nothing else.
- **SOURCE** (TAPE) is read-only. `describe` lists `source "<label>"` in its panel, and `since` sends `source "<text>"` (the box's `SourceState` text) in every whole answer. `ctx.src` is part of the text context, so a change makes the answer whole.
- **Protocol 3:** the `param` line carries `lin|oct` after the kind, and `ed set <addr> #<n>` picks option `n`.

## Tasks

### Task 1: protocol 3 grammar
- [ ] Tests: `hello` says 3; the `param` line has `lin|oct` matching `ParamSpec::scale` for every described param; `#n` sets a choice and `#n` past the last option is refused; `#n` on a continuous param is refused.
- [ ] Code: `PROTOCOL = 3`, `EdValue::Index(OptIndex)`, the `scale` word, `resolve` for an index.

### Task 2: typed addresses with pads and KIT
- [ ] Tests: every param of every engine's blocks round-trips through `Display` and `parse`; pad tokens 1–12 and routes 1–4 round-trip; `PAD0`, `PAD13`, `PAD1_RT5`, `PAD1_FLT.KIND` (not in IDS) and `KIT.NOPE` are refused; the token fixture.
- [ ] Code: `EdAddr`, `EdBlock`, `SoundBlock`, `PadBlock` in `addr.rs`.

### Task 3: describe for every engine
- [ ] Tests: the coverage test over every engine and variant (the want-set built from the bindings, as slice 1's); each engine's page count equals its chain's (with the pad copies on DRUM); every variant is a described panel; `describe` is static; every page fits the answer clock; schema fixtures for all five engines.
- [ ] Code: `chain(engine)`, `pages(engine)`, `page_at(engine, n) -> (def, Option<PadId>)`, `Group` with Modal, Tape, drum Machine and Route, and cells from `view::view`.

### Task 4: get and set on every engine
- [ ] Tests: `get` text matches the box's for every param a page shows, on each engine (DRUM on the selected pad and on another pad); `set` then `get` round-trips on each engine; a pad `set` lands on the named pad and leaves the selection; CHOKE past 8 groups writes nothing; `apply` drops a set whose Part changed engine.
- [ ] Code: `value::reading` and `edits::resolve` through one `find_cell`; `Pending` keeps its engine; the pad edit in `apply`.

### Task 5: since for every engine
- [ ] Tests: `since 0` answers each engine whole (with `source` on TAPE); a knob, a CC and a load are reported on each engine; selecting a pad on the box answers only a diff; a pad's machine change reports `panel PADn_MCH` and that pad's texts; `SLOTS` holds every engine; a full DRUM answer fits the answer clock.
- [ ] Code: per-engine `in_schema`, slot walk over `EdBlock`s, `Shown`, `Described` per engine.

### Task 6: the editor speaks protocol 3 (no new screens)
- [ ] Elm tests: the `param` line with its scale parses and the old line is refused; `Gesture` uses the described scale (the guess is deleted); a choice is sent as `#n`, so GATE EXT is settable; `Unit.Engine` is the five engines; the stale `since` answer is dropped (#1002).
- [ ] The e2e (`sim.mjs`) runs each engine's describe/set/get against the simulator.

### Task 7: ADR 0285, budgets, full check
- [ ] ADR 0285 and its README row; measured `ADR 0285` lines in `tools/bank1-budget.txt` and `tools/bank2-budget.txt`; ITCM measured.
- [ ] `just check`, `just firmware-orbit`.

### Task 8 (after the owner's yes): the mockup, then the Elm screens
- [ ] One HTML mockup of MODAL, VA, TAPE and DRUM on ALGO's grid, in `.superpowers/mockup/`, for the owner's yes or no. Stop there.
