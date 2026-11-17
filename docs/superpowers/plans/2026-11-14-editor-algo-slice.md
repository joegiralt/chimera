# Browser editor slice 1 (ALGO on one screen) Implementation Plan

**Goal:** a Chrome/Edge page that connects to Chimera over USB (or to the desktop simulator) and shows and edits every ALGO parameter live, both ways.

**Architecture:** new `ed` console commands in `chimera-core/src/console/ed/` (pure, host-tested; in bank 2 on the firmware) give the editor a static schema (`describe`), values with the box's own text (`get`), edits through the knob path (`set`) and pulled changes (`since`; owner 2026-11-16, replacing pushed `watch`). A WebSocket bridge in `chimera-utils` exposes the simulator's TCP console to a browser. The editor is Elm with a small JS port shim for Web Serial and WebSocket.

**Tech Stack:** Rust (no_std core, nightly), Elm 0.19.1, elm-test, node 22 (tooling only), tungstenite (host bridge only).

**Spec:** `docs/superpowers/specs/2026-11-14-editor-algo-slice-design.md`

**Recon:** the file:line facts this plan relies on are cited inline in each task.

## Plan rulings (made while planning; the spec is approved)
- **Block tokens are the existing `BlockRef::disk_ident()`** (`chimera-engine/src/storage/codes.rs:76`: `ALGO`, `OP_A`…`OP_F`, `FILTER`, `ENV1`…`ENV3`, `LFO1`…`LFO3`, `OUT`, `PITCH`, `VOICEFX`), already pinned by `disk_codes_test.rs::table_matches_golden`. An address is `<disk_ident>.<spec ident>`, e.g. `OP_C.CRSE`, `FILTER.CUTOFF`. This replaces the spec's illustrative `OP3.CRSE`/`FLT.CUT`: the same rule (frozen, fixture-pinned), with one table instead of two.
- **FLT › MOD's cells (ENV AMT, KEYBD, FM1–3, QM) are mod-matrix routes** (`View::Route`), not `ParamSpec`s, so they move to slice 2 with the matrix. Slice 1's FLT panel shows a "MOD: next slice" placeholder there.
- **Glyphs come after slice 1.** `wasm32-unknown-unknown` and `wasm-pack` aren't installed. Slice 1 draws plain placeholders (the algorithm as text `ALG 12 → 7`, envelopes as numbers).
- **`describe` is per page** (`ed describe ALGO <page>`): one answer must finish within `ANSWER_MS` = 1000 ms.
- **WebSocket lives in a bridge**, `chimera-utils ed-bridge`, not in `chimera-desktop` (which has no WebSocket dependency).
- **`PROTOCOL` goes from 1 to 2.** `chimera-usb.py` doesn't read it, and the existing commands don't change.

## Global Constraints
- Firmware: no heap, no blocking in audio, `// SAFETY:` on every unsafe, no libc; the `ed` code is in bank 2 behind a door (ADR 0251 rules: bank 1 gains only the door stub; report the ORBIT bank-1 delta and add an ADR 0251 line in `tools/bank1-budget.txt`).
- Sounds unchanged: every golden bit-identical.
- Every editor command is under `ed`. Existing console commands, `chimera-usb.py` and the update path don't change.
- Addresses are `<disk_ident>.<ident>`, never labels or numeric ids.
- `ed set` goes through the box's own edit path (`part_page::edit` with `Edit::To`, via `UiState`), so smoothing (ADR 0112) applies.
- Value text is the box's own text: one shared function, used by both the renderer and `ed`.
- The unit never pushes: it only answers. The editor pulls with `ed since <n>`; a stale `<n>` gets everything. No sequence numbers, no checksum (USB bulk transfers are CRC-checked).
- The editor never guesses: it shows only what the unit last answered (a pending mark while an edit is unconfirmed).
- The editor follows the Part selected on the box; a non-ALGO Part shows `PART 2 PLAYS VA. THE EDITOR EDITS ALGO FOR NOW.`
- Elm only for the editor's logic and view; JS only in the port shim. Chrome/Edge (Web Serial).
- Builds: `CARGO_BUILD_JOBS=10 nice -n 15 …`; targeted tests in a task, `just check` before each firmware task's commit.
- Commits: terse, no type prefix; never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`; no git stash.

## Review Focus
1. **A knob turned on the box while the editor's own `set` is pending**: the editor shows the box's value. Pinned in Task 9 (`shows_only_the_units_value`).
2. **A Sound load on the box (or a Part switch, or an engine change)**: the next `since` answer carries every value that differs, so the editor never shows a mix. Pinned in Task 5 (`since_reports_load_part_and_engine`) and Task 9 (`part_change_follows_the_box`).
3. **The USB cable is pulled or the port is busy (`chimera-usb.py` holds it)**: the editor says so plainly and can reconnect, never hangs. Pinned in Task 8 (`port_busy_and_lost_are_reported`).
4. **`set` out of range, on a choice by name, or on a param the current variant hides**: clamped and quantized as a knob would; an unknown address or value is `ERR`, never a panic. Pinned in Task 4 (`set_clamps_quantizes_and_refuses`).
5. **A knob turned fast, or a lost answer**: a fast turn is one `val` per address per `since`; a lost answer is healed by the next `since` answering everything; audio never overruns. Pinned in Task 5 (`since_coalesces_a_fast_turn`, `stale_number_answers_everything`) and in the unit gate.

---

### Task 1: `ed` namespace, addresses, `ed hello`

**Files:**
- Create: `chimera-core/src/console/ed/mod.rs`, `chimera-core/src/console/ed/addr.rs`
- Modify: `chimera-core/src/console/mod.rs` (one `table!` row `Ed (EdArg) => "ed", "editor protocol"`; `PROTOCOL = 2`), `chimera-core/src/console/answer.rs` (one match arm into `ed::answer`)
- Test: `chimera-core/tests/console_ed_test.rs` (reuse `console_answer_test.rs`'s `Sink`/`Fake`/`ask` pattern; extract them to `tests/common/console_fake.rs` if both need them)

**Interfaces:**
- Produces: `pub struct EdAddr(pub ParamAddr)`, with `EdAddr::parse(&[u8]) -> Option<EdAddr>` (`<disk_ident>.<ident>`, the ident matched against `BlockRef::specs()`) and `impl fmt::Display` (the same text back); `pub enum EdArg { Hello, Describe { engine: EngineType, page: u8 }, Get(EdAddr), Set(EdAddr, EdValue), Since(Gen) }` (Task 5 replaces Task 1's `Watch, Unwatch` with `Since`); `pub enum EdValue { Num(f32), Name(Word) }`; `pub fn answer(arg: EdArg, unit: &mut impl Unit, out: &mut impl Out) -> Result<(), Stalled>`.

- [ ] **Step 1: Write failing tests**: `addr_round_trips_every_algo_param` (every `ParamAddr` of ALGO's blocks parses back from its own `Display`); `addr_refuses_unknown_block_and_ident` (`OP_G.CRSE`, `OP_A.NOPE`, `CRSE`, `OP_A.` → `None`); `hello_reports_protocol_build_part_engine` (`ask(&mut f, "ed hello")` yields `ed hello 2 <build> part 1 ALGO` then `OK`); `ed_without_sub_is_refused` (`ERR ed takes …`).
- [ ] **Step 2:** run `cargo test -p chimera-core --test console_ed_test`; expect them to fail to compile or to fail.
- [ ] **Step 3: Implement** the `table!` row, `EdArg::parse`, `EdAddr` and `ed::answer` for `hello` (the others answer `ERR ed <sub> is not in this build` until their tasks). The engine word is `EngineType`'s disk name (`ALGO`, `MODAL`, `VA`, `TAPE`, `DRUM`).
- [ ] **Step 4:** run the test file and `console_parse_test` (`the_table_is_the_one_list` must list `ed`); both pass.
- [ ] **Step 5:** `just check`, then commit `Console: ed namespace, addresses and hello`.

### Task 2: `ed describe` — the static schema, page by page

**Files:**
- Create: `chimera-core/src/console/ed/describe.rs`
- Test: `chimera-core/tests/console_ed_test.rs`

**Interfaces:**
- Consumes: Task 1's `EdAddr`, `EdArg::Describe`.
- Produces: the answer grammar, one line per item, then `OK`:
  - `page <n> <LABEL> of <count>`
  - `panel <name> <variant>` (variant: a `FilterKind`/`FxType`/`EnvKind`/`LfoKind`/`OpKind` name; `-` for a fixed page)
  - `param <addr> "<label>" <cont|choice|toggle> <min> <max> <step> <default>`
  - `opt <addr> <index> "<name>"`
  - `route <cell label> next-slice` (FLT › MOD's route cells)
- `pub fn pages(engine: EngineType) -> u8`, the page count the editor walks.

- [ ] **Step 1: Write failing tests**:
  - `describe_walks_every_algo_page`: page 0..`pages(Algo)` each answer `OK`; `pages(Algo)` matches the ALGO and ALGO_FORMANT chains' pages and sub-pages (`ALGO_CHAIN`, `ALGO_FORMANT_CHAIN`, `block_registry.rs:765–791`).
  - `describe_covers_every_param_every_variant`: the union of `param` lines over all pages equals every `ParamSpec` of ALGO's blocks reachable from any cell under any variant, i.e. iterate every `FilterKind`, `FxType`, `EnvKind`, `LfoKind` and `OpKind` through `filter_panel::knob`, `fx_panel::cells`, `mod_panel::env_panel`/`lfo_panel` and `op_page::page_cells`. Filter MODE's options come per KIND from `FilterParams::spec(MODE)` (`params.rs:294–312`).
  - `describe_is_static`: the answer for every page is byte-identical before and after changing KIND, an op kind, ENV TYPE and loading another Sound.
  - `describe_fits_the_answer_clock`: every page's answer through a `Sink` that counts bytes stays under the bytes 1000 ms of USB full speed carries at the console's measured rate (the bound in `console_shell_test`), with a 2× margin.
- [ ] **Step 2:** run the tests; they fail.
- [ ] **Step 3: Implement `describe`** by walking the chain defs and panel functions above. It reads only the static tables, never the current Sound.
- [ ] **Step 4:** the tests pass. Write the full schema of every page to `editor/tests/fixtures/algo-schema.txt`, from a test named `writes_editor_schema_fixture` that fails when the file differs (the editor's coverage test in Task 10 reads it). Bless with `BLESS=1`.
- [ ] **Step 5:** `just check`, then commit `Console: ed describe, ALGO's schema page by page`.

### Task 3: `ed get` — the value and the box's own text

**Files:**
- Create: `chimera-core/src/console/ed/value.rs`
- Modify: `chimera-core/src/ui/renderer.rs` (move the private `value_text`/`param_text` at :1436 into `pub fn cell_text(ui: &UiState, part: PartId, addr: ParamAddr, buf: &mut FmtBuf)` in a new `chimera-core/src/ui/cell_text.rs`; the renderer calls it, so the box draws exactly as before)
- Test: `chimera-core/tests/console_ed_test.rs`; the renderer's screenshot goldens

**Interfaces:**
- Produces: `ed get <addr>` → `val <addr> <normalized 0..1> <stored> "<text>"` then `OK`; `pub fn cell_text(...)`.

- [ ] **Step 1: Write failing tests**: `get_text_matches_the_box_for_every_algo_param` (for each `param` in the schema, at three stored values, `ed get`'s text equals the screen's text, read with `cell_text` and compared with the renderer's output, through the existing shot or page test harness); `get_unknown_addr_is_refused`.
- [ ] **Step 2:** run them; they fail.
- [ ] **Step 3: Implement** the extraction (move the code without rewriting it) and `get`.
- [ ] **Step 4:** the tests pass; the renderer and screenshot goldens are unchanged (`all_pages_walk_test`, `page_identity_test`, shot tests).
- [ ] **Step 5:** `just check`, then commit `Console: ed get, values in the box's own words`.

### Task 4: `ed set` — through the knob path

**Files:**
- Create: `chimera-core/src/console/ed/edits.rs` (`EdQueue`: a fixed array of pending `(ParamAddr, EdValue)`, as `Hands` queues `Gesture`s, `console/hands.rs:200`)
- Modify: `chimera-core/src/ui/mod.rs` (add `pub fn edit_to(&mut self, part: PartId, a: ParamAddr, v: f32) -> bool`, wrapping the private `edit_in(part, a, Edit::To(v))` at :1855, so the CC hand, PARAM OUT and WALK run as for a knob); `answer.rs` `Unit` trait (add `fn ed(&mut self) -> Option<&mut EdQueue>`, defaulting to `None`); both shells drain the queue in their UI pass (`chimera-desktop/src/main.rs:147–158`, `chimera-board/src/usb.rs` `hand` :572 / `lib.rs:806`)
- Test: `chimera-core/tests/console_ed_test.rs`

**Interfaces:**
- Produces: `ed set <addr> <number|"name">` → `OK queued` (applied on the next UI pass; the value comes back through `since` or a later `get`). `EdQueue::push(addr, v) -> bool` (false when full → `ERR busy`); `EdQueue::apply(&mut self, ui: &mut UiState)`.

- [ ] **Step 1: Write failing tests**:
  - `set_then_get_round_trips` for every continuous ALGO param at min, max and mid.
  - `set_glides_like_a_knob`: after `set` and one `apply`, the `KnobPublisher` delta for a `Continuous` param is a glide, not a snap (compare with a knob turn via `Hands`).
  - `set_clamps_quantizes_and_refuses`: above max → max; a step param lands on a step; a choice by name (`ed set FILTER.KIND "LADDER"`) works; an unknown name, an unknown address and a non-number are `ERR`; a param the current variant hides is still set (it's stored).
  - `set_queue_full_is_busy`.
  - `set_on_a_non_algo_part_is_refused`.
- [ ] **Step 2:** run them; they fail.
- [ ] **Step 3: Implement** as in the Files block.
- [ ] **Step 4:** the tests pass; the goldens are bit-identical.
- [ ] **Step 5:** `just check`, then commit `Console: ed set through the knob path`.

### Task 5: `ed since` — the editor pulls changes

*(Owner 2026-11-16: replaces the pushed `ed watch`. The stopped push attempt is on branch `editor-watch-wip`; its snapshot and variant reporting may be reused, its push machinery not.)*

**Files:**
- Create: `chimera-core/src/console/ed/since.rs` (`EdSince`: the last-reported copy — followed Part, engine, panel variants, every value of the ALGO schema at f32 bits — and its `Gen`)
- Modify: `EdArg` (`Watch`/`Unwatch` → `Since(Gen)`; `ed watch` becomes an unknown command); the `Unit` trait gives the door the `EdSince` alongside the UI (as `ed()` does for `EdQueue`); the door (bump `ED_MAGIC` to 0xB2C0_0ED3 if its layout or signatures change)
- Test: `chimera-core/tests/console_ed_test.rs`

**Interfaces:**
- Produces: `ed since <n>` → zero or more of `part <n> <ENGINE>`, `panel <name> <variant>`, `val <addr> <norm> <stored> "<text>"` (the same text path as `ed get`), then `OK <m>`. `Gen` is a u32 newtype that is never 0 (0 means "I have nothing", so the editor's first ask is `ed since 0`); it advances on every `since` answer. If `<n>` ≠ the saved `Gen`, everything is answered. A non-ALGO Part answers only its `part` line.

- [ ] **Step 1: Write failing tests**:
  - `since_zero_answers_everything`: every schema address once, the Part, the engine and every panel variant, then `OK <m>`, m ≠ 0.
  - `since_current_with_no_change_is_just_ok`.
  - `since_reports_knob_cc_load_and_set`: a knob turn (via `Hands`), a MIDI CC (`cc_step`), a Sound load and an applied `ed set` each show up as `val` lines in the next answer, and nothing else.
  - `since_reports_load_part_and_engine`: `key B2` → `part 2 VA`.
  - `since_reports_variant_change`: KIND → `panel FILTER LADDER`.
  - `since_coalesces_a_fast_turn`: 20 detents between two asks → one `val` line for that address.
  - `stale_number_answers_everything`: an answer the editor never saw (ask with the older number) → everything again.
  - `refused_set_reports_nothing`: an over-budget formant `set` that the writer refuses leaves no `val` line.
  - `full_answer_fits_the_answer_clock`: the full answer is under `ANSWER_BYTES_MAX`.
  - `ed_watch_is_unknown`.
  - From Task 4's review: `set_glides_like_a_knob` must fail if smoothing were bypassed (compare the published move with a knob turn's, or show a ramp, not a jump); `set_queue_full_is_busy` asserts the real capacity (16, an exported const).
- [ ] **Step 2:** run them; they fail.
- [ ] **Step 3: Implement.** Compare every value each ask (no `Stamp` shortcut: real values, not write paths). Runs only in the console's answer path, never in audio.
- [ ] **Step 4:** the tests pass; goldens bit-identical; `tools/test_chimera_usb.py` passes.
- [ ] **Step 5:** `just check`, then commit `Console: ed since, the editor pulls`.

### Task 6: `ed` in bank 2

**Files:**
- Modify: `chimera-stm32/build.rs` (a glob for `console::ed`, `ui::cell_text` stays shared: the renderer runs in bank 1); `chimera-core/src/bank2.rs` (an `EdDoor` `#[repr(C)]` table in a new fixed slot; add the slot address in `chimera-engine/src/bank2.rs:27–38` with its linker assert); `console/answer.rs` (dispatch through the door; door closed → `ERR ed needs bank 2, update the unit`); `tools/bank1-budget.txt`
- Test: the door tests' pattern (`ui_door` tests), `just check`'s door-check

**Interfaces:**
- Produces: `pub fn ed_door<R>(f: impl FnOnce(&EdDoor) -> R) -> Option<R>`, as `ui_door` (`chimera-core/src/bank2.rs:507`).

- [ ] **Step 1: Write failing tests**: `ed_door_closed_refuses_plainly`; `door_check_has_no_bank1_to_bank2_calls` (the existing `door-check` in `just bank2-split` reports none).
- [ ] **Step 2:** run them; they fail.
- [ ] **Step 3: Implement** the door, the build.rs glob and the slot.
- [ ] **Step 4:** `just firmware-orbit` reports the bank-1 delta (the door stub only, target ≤ 300 B) and door-check is clean; all `console_ed_test` tests pass.
- [ ] **Step 5:** `just check`, add the ADR 0251 line, then commit `Console: ed lives in bank 2`.

### Task 7: the simulator bridge

**Files:**
- Create: `chimera-utils/src/ed_bridge.rs`; the subcommand `ed-bridge [--listen 127.0.0.1:7342] [--console 127.0.0.1:7341]`
- Modify: `chimera-utils/Cargo.toml` (`tungstenite`, host only); `Justfile` (`editor-sim`: runs `just desktop` and the bridge)
- Test: `chimera-utils/tests/ed_bridge_test.rs`

**Interfaces:**
- Produces: one WebSocket text message per console line, both ways; a closed console closes the socket; one browser at a time.

- [ ] **Step 1: Write failing tests**: `bridges_lines_both_ways` (against a fake TCP console: send `ed hello`, receive its lines); `console_gone_closes_socket`; `second_client_replaces_first` (as the desktop console does).
- [ ] **Step 2:** run them; they fail.
- [ ] **Step 3: Implement**, blocking with two threads per connection (host tool; keep it plain).
- [ ] **Step 4:** the tests pass.
- [ ] **Step 5:** commit `chimera-utils: ed-bridge, the simulator console over WebSocket`.

### Task 8: editor scaffold, port shim, transport

**Files:**
- Create: `editor/elm.json`, `editor/package.json` (dev deps: `elm-test`, `elm-format` only), `editor/src/Main.elm`, `editor/src/Transport.elm`, `editor/src/Protocol.elm`, `editor/public/index.html`, `editor/public/ports.js`, `editor/tests/ProtocolTest.elm`, `editor/tests/TransportTest.elm`
- Modify: `Justfile` (`editor-test`: `cd editor && npx elm-test`; `editor-build`: `elm make --optimize` into `editor/dist/`)

**Interfaces:**
- Produces:
  - `ports.js`: `connectSerial()` (`navigator.serial.requestPort({filters:[{usbVendorId:0x0483, usbProductId:0x5740}]})`, 115200 8N1, line-split UTF-8), `connectSocket(url)`, `send(line)`. Ports in: `lineIn : (String -> msg) -> Sub msg`, `linkState : (String -> msg) -> Sub msg` with `"open" | "busy" | "lost" | "unsupported"`.
  - `Protocol.elm`: `type Line = Answer AnswerLine | Ok_ (Maybe Int) | Err_ String`; `parse : String -> Line`; `encode : Command -> String`, where `type Command = Hello | Describe String Int | Get Addr | Set Addr Value | Since Int`.

- [ ] **Step 1: Write failing tests**: `ProtocolTest`: every line form from Tasks 1–5 parses (including `OK <m>`) (use lines copied from the Rust tests' expected output), and garbage parses to an `Err_`-like `Unknown` that is ignored, never crashes; `TransportTest.port_busy_and_lost_are_reported`: the `busy`/`lost`/`unsupported` link states map to the user texts `THE PORT IS IN USE (CLOSE chimera-usb.py)`, `THE UNIT WAS DISCONNECTED`, `USE CHROME OR EDGE`.
- [ ] **Step 2:** `just editor-test` fails.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `just editor-test` passes; `just editor-build` produces `editor/dist/index.html`.
- [ ] **Step 5:** commit `Editor: scaffold, port shim and protocol`.

### Task 9: editor model — only the unit's state

**Files:**
- Create: `editor/src/Model.elm`, `editor/src/Schema.elm`, `editor/tests/ModelTest.elm`

**Interfaces:**
- Produces: `type alias Model = { link : Link, schema : Schema, values : Dict String Value, pending : Dict String Time.Posix, part : Int, engine : String, variants : Dict String String, gen : Int }`; `update : Msg -> Model -> ( Model, Cmd Msg )`. On connect it runs `ed hello`, then `ed describe ALGO 0..n-1`, then `ed since 0`, then `ed since <gen>` every 33 ms, one request in flight at a time.

- [ ] **Step 1: Write failing tests**:
  - `shows_only_the_units_value`: after an edit, the shown value is the unit's until a `since` answer changes it; the control is marked pending.
  - `pending_clears_on_report_or_timeout`: on a `val` for that address, or after 250 ms with none.
  - `since_answer_replaces_values_and_gen`.
  - `one_since_in_flight`: no second `since` before the first answer.
  - `part_change_follows_the_box`.
  - `non_algo_part_shows_message` (exact text `PART 2 PLAYS VA. THE EDITOR EDITS ALGO FOR NOW.`).
- [ ] **Step 2:** the tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `just editor-test` passes.
- [ ] **Step 5:** commit `Editor: model follows the unit`.

### Task 10: the ALGO panel

**Files:**
- Create: `editor/src/View/Algo.elm`, `editor/src/View/Controls.elm` (knob, slider, choice, toggle), `editor/src/Layout/Algo.elm` (placement table: address → section and grid cell, per variant), `editor/public/style.css`, `editor/tests/LayoutTest.elm`

**Interfaces:**
- Consumes: `editor/tests/fixtures/algo-schema.txt` (Task 2).
- The look follows mockup v10 (the ALGO panel): a Prophet-6 unit grid with rounded-square sections (ALG, OPERATORS 1–6 as rows, FILTER with CUTOFF as the large hero knob, AMP, ENV 1–3 as sliders, LFO 1–3, PITCH, VOICE FX). The panel for the current variant comes from `variants`. Unplaced addresses go in an `UNPLACED` strip. FLT › MOD shows `MOD: NEXT SLICE`.

- [ ] **Step 1: Write failing tests**: `every_schema_param_is_placed` (parse the fixture; every `param` address under every `panel` variant has a placement); `no_placement_points_nowhere` (every placement's address exists in the schema); `hero_is_cutoff` (`FILTER.CUTOFF` is placed as `Hero`).
- [ ] **Step 2:** the tests fail.
- [ ] **Step 3: Implement.** Mouse drag and wheel on knobs send `ed set` at most once per animation frame per control; a value shows the unit's `text`.
- [ ] **Step 4:** `just editor-test` passes.
- [ ] **Step 5:** commit `Editor: ALGO on one screen`.

### Task 11: end to end against the simulator

**Files:**
- Create: `editor/e2e/sim.mjs` (node 22's built-in `WebSocket`, no deps), `Justfile` `editor-e2e` (starts the simulator headless if `chimera-desktop` supports it, or documents `just editor-sim` first; then runs the script)

- [ ] **Step 1: Write the script**: connect to the bridge; `ed hello`; describe every page; for every `param`, `ed set` to its max then min and `ed get` each back; turn a simulator knob with `key`/`turn` (the console's `Hands`) and see its `val` in the next `ed since`; switch Part with `key B2` and see `part 2 …`; load a Sound via the browser keys and see the changed values; ask with a stale number and get everything. Exit non-zero on any mismatch.
- [ ] **Step 2:** `just editor-e2e` passes against the simulator.
- [ ] **Step 3: Manual browser pass (controller):** `just editor-sim`, open `editor/dist/index.html` in Chrome, connect to `ws://127.0.0.1:7342`, sweep every control, and turn knobs in the simulator; record any mismatch as a fix.
- [ ] **Step 4:** commit `Editor: end-to-end run against the simulator`.

### Task 12 (controller): unit gate
- Flash the branch's ORBIT build (owner OK for unit work is standing for this effort), connect the editor in Chrome over Web Serial, run `editor-e2e`'s steps over the real port (via `chimera-usb.py`'s serial path or the page), sweep every ALGO control while recording audio on MOTU inputs 6–7, and check `stats`: 0 overruns, no glitch, no reset. Publishing to Pages is the owner's call (outward-facing).
