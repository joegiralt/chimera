# Browser editor, slice 1: ALGO on one screen

**Status:** draft for the owner's approval (2026-11-14)

## Intent

Chimera's browser editor is a big screen for the PreenFM3, not a replacement for it. The unit stays the instrument; the editor is where the jobs that are a chore on six encoders and a small display get easy: seeing a whole engine at once, typing labels, moving scenes, managing the card, updating the firmware. The owner calls it a mini DAW: a place to see and fix things, not a timeline.

Slice 1 is **ALGO with every parameter on one screen, live both ways**. ALGO is the engine that hurts most on the box (six operators, the algorithm, envelopes and modulation spread across many pages). The slice's real job is the foundation every later slice stands on: the USB link, the unit describing itself, live two-way editing, and testing against the desktop simulator.

## Goals

- Connect to a PreenFM3 running Chimera from Chrome or Edge with one click, and see the Part's ALGO Sound on one screen.
- Every ALGO parameter the box shows (ALG, OSC with all six operators, FLT, AMP, the MOD chain's envelope and LFO pages, plus PIT) is visible and editable.
- A change in the editor sounds on the unit at once. A knob turned on the unit moves in the editor at once.
- Values read exactly as the box writes them (`C4 +12ct`, `1.2 s`, option names).
- Develop and test against `chimera-desktop` with no unit attached.

## Non-goals (slice 1)

- The mod matrix (routes, source × destination amounts) and its patch-bay view: slice 2, on the protocol slice 1 proves (owner, 2026-11-14). Routes aren't `ParamSpec` parameters, so they need their own addressing.
- Other engines, the mixer, FX, ORBIT, the card, MIDI MAP labels, firmware update, recording. Each is a later slice on this foundation.
- Saving. The Sound is edited live, and saving stays on the box (MENU hold) for slice 1.
- Firefox or Safari (no Web Serial).
- Offline editing. The editor shows the unit's state and nothing else.

## Anti-goals (a failure even if it works)

- The editor and the unit disagree about a value, a range, a label or what exists.
- An editor that only works with one firmware version.
- A parameter list written by hand in the editor.
- Anything the editor sends making the unit glitch audio, lose its UI, or need a power cycle.

## Approaches (decided)

| | Approach | Trade-off |
|---|---|---|
| **A (chosen)** | USB console (Web Serial); the unit describes its own parameters | One protocol for everything, no version matrix, MIDI stays free. Chrome/Edge only; a small firmware addition. |
| B | Console; parameters generated into the editor per firmware release | Less firmware work, but a second copy of every table and an editor × firmware version matrix forever. |
| C | MIDI SysEx; parameters built in | Firefox works, but 7-bit packing, slow file transfer, Windows MIDI ports held by one app, and the console still needed for files and updates, so two protocols. |

The owner chose A (2026-11-14): "the most principled, least bugs in the future, as a solo developer". Its rules:

1. **One source of truth for parameters.** The description is generated from the firmware's `ParamSpec` tables and page layouts, never written by hand. A test proves that every page and cell the box shows is described.
2. **One source of truth for state.** The unit owns the Sound. The editor holds only what the unit last told it, plus edits in flight.
3. **The protocol is versioned and checked.** Unknown or garbled messages are refused, never guessed at.
4. **Simulator first.** Everything runs against `chimera-desktop` before a unit.

## Decided design

### Transport
- Web Serial to the console the unit already runs (USB CDC, 0483:5740). The console is one client at a time, so the editor and `chimera-usb.py` can't both be connected. The editor says so when the port is busy.
- The desktop simulator's console is a TCP socket (`127.0.0.1:7341`, `chimera-desktop/src/console.rs`). A browser can't open raw TCP, so the simulator also needs a WebSocket on the same protocol (or a small bridge in `chimera-utils`). The editor talks to either through one transport interface.

### Protocol: the editor commands
These are new console commands. They follow the existing console's style: text lines, `ERR …` on a refusal, and `PROTOCOL` raised (today 1). *(Calls marked "my call" were settled after the builder check, not by the owner.)*
- **Namespace (my call):** every editor command sits under one prefix, `ed` (`ed hello`, `ed get`, …). Existing commands, `chimera-usb.py` and the update path don't change, and the card `get` keeps its meaning.
- **Addresses (my call):** a parameter is addressed by a frozen block token plus its `ident`, e.g. `OP3.CRSE`, `ENV2.ATK`, `FLT.CUT`. Idents are only unique within a block, so the block part needs the same discipline: block tokens are explicit literals pinned by a fixture test, exactly as `(id, ident)` already is.
- **`ed hello`:** the protocol version, the firmware build, the Part selected on the box and its engine. If the editor doesn't support the unit's protocol version, it says "update the editor" or "update the unit". It never runs half-working.
- **`ed describe <engine>` gives the full, static schema (my call):** every parameter any of that engine's pages can show, in every state: every filter type's, every ENV MODE/FORM's, every operator kind's (formant FRMT/BW/SKIRT included), every FX TYPE's. Each comes with its label, kind (continuous/choice/toggle), min, max, step, default and option names, grouped by page and panel. The schema never changes while connected, so the layout is laid out once.
- **What the box shows right now is state, not schema (my call):** `watch` reports which panel variant each switching page shows (`FLT shows LADDER`, `OP3 shows FORMANT`, `ENV1 shows ADSR`) and which operator is selected. The editor shows the matching panel, the way the box does. The coverage test loops over every variant: every parameter in the schema is placed in some variant of the layout.
- **`ed get <addr>` / `ed set <addr> <value>`:** read or write one parameter of the Part the editor follows. A `set` goes through the same path a knob turn takes on the box (ADR 0112 smoothing, `Part` knob path), so it can never snap or zipper. The answer carries the value and its text as the box formats it (`ValFmt`), so the editor never re-implements formatting.
- **`ed watch`:** the unit pushes a line for every parameter that changes on the followed Part from any source (knob, MIDI, the editor itself, loading a Sound), with value and text, plus panel-variant changes, a change of selected Part, and a change of engine. It runs until `ed unwatch` or disconnect. Changes are coalesced per UI frame so a turning knob can't flood the link.
- **Which Part (my call):** the editor follows the Part selected on the box (live mirror: the unit is the instrument). Changing Part on the box changes it in the editor; the editor also has Part buttons that select the Part on the box, as B1–B6 do. A Part that isn't ALGO shows a plain screen in slice 1: "PART 2 PLAYS VA. THE EDITOR EDITS ALGO FOR NOW."
- **Integrity (my call):** every unit-to-editor line carries a sequence number, so a dropped line is noticed and the editor resyncs with fresh `ed get`s. No checksum: USB bulk transfers are already CRC-checked by the hardware, so a line arrives whole or not at all. Unknown commands and malformed arguments are refused with `ERR`.

### The unit side
- The describe, get, set and watch handlers live in `chimera-core`'s console module (pure, host-tested), behind bank-2 doors on the firmware (bank 1 frozen). Only door stubs touch bank 1.
- Text formatting reuses the box's own `ValFmt`, so a value reads identically on the box and in the editor.

### The editor side
- **Elm**, all the way, with a thin JavaScript port shim for Web Serial and WebSocket (Elm can't call them directly).
- The Elm model is "the last state the unit reported + edits in flight". `update` sends a `set` and shows the change optimistically until the unit's echo confirms it, and a refused edit snaps back to the unit's value.
- **The look comes from the editor, the facts from the unit.** The ALGO panel follows mockup v10: the Prophet-6 grid, DX touches, the algorithm chart, operator rows, envelopes as sliders, CUTOFF as the hero knob. It places controls by `ident`. If the unit describes a parameter the layout doesn't place, the editor shows it in a plain "unplaced" strip and never hides it. A test fails if any `ident` is unplaced for the current firmware.
- **Glyphs** (algorithm chart, envelope and wave screens) are drawn by the firmware's own Rust glyph code compiled to WebAssembly and called through the port shim, so they match the box (owner, 2026-11-14).
- **Hosting:** a static site on GitHub Pages, from the public `chimera-releases` repo, because a private repo's Pages needs a paid plan.

### Testing
- **Firmware (host):** describe covers every ALGO cell on every page; `set` then `get` round-trips through the knob path; `watch` reports a knob turn, a MIDI CC and a Sound load; malformed lines are refused; bank-1 size delta reported with an ADR 0251 line.
- **Editor:** Elm unit tests on the model (echo confirms, refusal reverts, resync); an end-to-end run against `chimera-desktop` (connect, change every ALGO parameter, see each reflected in the simulator's state, turn a simulated knob and see the editor follow).
- **Unit gate:** the same end-to-end run against the PreenFM3 over USB, with audio recorded while sweeping parameters, and no overrun or glitch.

## Constraints

- Firmware: no heap, no blocking in audio, `// SAFETY:` on any unsafe, new code in bank 2 behind doors, sounds unchanged (goldens bit-identical), ≤10 build cores.
- Elm on the editor side; no framework beyond Elm and the port shim.
- Only Chrome and Edge (Web Serial).
- Unit labels and names use the box's own words (full-word labels where they fit).

## Left to the builder

- The exact line grammar of the new commands, and how `describe` is chunked across lines.
- WebSocket in the simulator, or a bridge in `chimera-utils`.
- The Elm module layout and the shim's message shapes.
- Whether WASM glyphs land in slice 1 or right after it (slice 1 may draw placeholders).
- Where the editor's source lives (a folder in this repo, built and published to `chimera-releases`'s Pages, is the default).
