# 0333. `since` says every Part's engine

- **Status:** Accepted (2026-11-23)
- **Deciders:** coordinator (editor follow-up), Claude

## Context
The editor can name the engine of the Part it follows only. `ed hello` and PART's `part <n> <ENGINE>` are the followed Part's, and `since` carries no engine for the others. So the editor's Part tabs show Sound names instead of engines, and an unfollowed Part loading another engine goes unseen.

## Decision
- `since`'s SETTINGS section says **`engine <n> <ENGINE>`** for every Part, after the Parts' `sound` lines. `<n>` is the Part, 1–8. `<ENGINE>` is the engine's disk token, the word `part` and `hello` use (`ALGO`, `MODAL`, `VA`, `TAPE`, `DRUM`).
- It is said whole, as the rest of SETTINGS is, and then only when a Part's line changes, through the section's line hashes (`LineSeen`), as `sound` is.
- The protocol stays 4. The editor's `Protocol.parse` never fails: a line it doesn't know is `Unknown`. A `since` line that is `Unknown` is kept as an `Unread` notice ("THE UNIT SAID A LINE THIS EDITOR CAN'T READ"), and the rest of the answer reads as before. So an editor older than this line loses nothing but says so, once per line kept. The editor adds `[ "engine", n, engine ]` to `Protocol.parse` and `engine` to the contract test's `KINDS`, which fails on a kind it doesn't know, in the editor kit that ships with this firmware.
- The line and its copy are bank 2's (`.bank2_ed.settings.engine_line`, `since.rs`'s SETTINGS section).
- Flash: bank 2 +352 B (default, bank2-gate), +480 B (ORBIT); bank 1 +0.

## Alternatives considered
- **A fifth word on `sound`.** An older editor matches `sound` by its four words exactly, so the line would turn `Unknown` and the editor would lose the Sound's name and state.
- **`part <n> <ENGINE>` for every Part.** The editor reads `part` as "the followed Part is this one". Eight of them would re-follow on each.
- **A protocol 5.** That costs a hello negotiation for one line. The editor kit ships per firmware version (ADR 0322), so the editor that meets this line reads it.

## Consequences
- The editor can name every Part's engine, and sees an engine change on any Part within one poll.
- SETTINGS whole grows by eight short lines, about 110 B. `a_whole_settings_answer_fits_the_bound` still holds.

## Sources
- ADR 0289 (protocol 4), `chimera-core/src/console/ed/settings.rs`, `since.rs`, `chimera-core/tests/console_ed_settings_test.rs`.
- chimera-releases `editor/src/Protocol.elm` (`Unknown`), `editor/e2e/contract.mjs` (`KINDS`).
