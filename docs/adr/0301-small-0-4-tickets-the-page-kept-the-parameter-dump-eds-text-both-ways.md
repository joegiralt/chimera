# 0301. Small 0.4 tickets: B*n* keeps the page, a controller reads a Sound, `ed`'s text is exact both ways

- **Status:** Proposed
- **Deciders:** firmware (the `small-tickets` batch: #957, #961, #980, #984, #1038, #1039), for the owner's review

## Context
Six small 0.4 tickets share one branch (owner, 2026-11-20). Four of them decide something later work has to keep:

1. **#957** (a tester, 0.3.0): B*n* from Part *m*'s sound pages landed on Part *n*'s home (ADR 0044), so comparing two Parts' filters took a walk back to FLT each time. MIX + B*n* already kept the mixer page.
2. **#984** (owner, 2026-11-15): a controller can set any cell by NRPN and hear panel edits back (PARAM OUT, ADR 0258), but cannot ask what the Sound's values are. After a load its LED rings and screens are stale until each knob moves.
3. **#1038 and #1039** (ADR 0297's two follow-ups): `ed set` read a decimal by float arithmetic (`i + f / 10^k`), an ulp off on about 1 % of what `ed get` prints; `ed get` printed a stored NaN as `0` and an infinity, or anything from 2^32 up, as `4294967295`.
4. **#961** (a tester, with video): turning NOTE on ORBIT's NOTES shrank every chip's font at a sharp, and sometimes turned the whole row into pitch marks.

#980 (VA's SHAPE draws its wave) and #1033 (two tests racing on process state) decide nothing new and are noted under Consequences.

## Decision

### B*n* from a sound page keeps the page (#957)
From Part *m*'s sound pages, B*n* (another Part) opens the same page on Part *n*: the same `PageAt` when both Parts are on the same chain (`PartChain`), else the page with the same id on Part *n*'s chain (`ChainDef2::locate`), else Part *n*'s home (`nav::kept`). FLT on an ALGO Part opens FLT on a VA Part; MODAL's RES, which no other engine has, opens the other Part's home. From anywhere else (the mixer, the Sound rung, SETTINGS) B*n* still lands on the home, and from Part *n*'s own pages it still toggles to its mixer. `Recall` is untouched: the page kept is the one on screen, not one remembered.

### The parameter dump (#984)
- **Request:** `F0 7D 43 48 01 pp F7`. `7D 43 48` is the head of Chimera's own messages (`chimera_midi::sysex::CHIMERA`): the maker and family bytes the identity reply gives, in the reply's order. `01` is the request. `pp` is `DumpWho`: `00`-`0F` a Part by number (`PartNo`; six exist), `10`-`1F` the Part on that MIDI channel (the lowest of several, the Part whose echo a shared control hears), `7F` the Part being edited. Any other `pp` is no request. It is taken before SysEx bindings see it, so it can't be learned.
- **Answer:** out of the port that asked, on the Part's channel, one NRPN for each stored cell of a block of the Sound (`BlockRef::in_sound`) that is writable (`Mappable`, the chart's `W`) and that the Part's engine reads (`Mappable::live`), in the chart's order (`BlockRef::ALL`, then spec order), its value `param_out::data`'s: exactly PARAM OUT's message for that cell (ADR 0216's address and law). Then `F0 7D 43 48 02 pp F7`, `pp` the Part that answered.
- **Not in it:** the chart's `R` cells (the filter FX's TYPE and PLACE), the matrix's routes, the MIDI map, names, the Part block (LEVEL, PAN, TILT have no NRPN) and the project's FX. It is a reading of the knobs, not a Sound transfer (#263 stays the whole-Sound SysEx).
- **Pacing:** a walk (`param_dump::Walk`: Part, cursor, the Sound's `Loads` and engine) kept between passes, one a port. Each out pass, after the echo and PARAM OUT, it pushes whole messages while half the port's ring stays free (`KEPT_FREE`, 128 B: 41 ms of DIN), so DIN's UART paces it and a knob's echo never waits behind more than half a ring. Measured on the host at DIN's 3,125 B/s: ALGO 235 cells, 1,724 B, 548 ms; MODAL 133, 340 ms; TAPE 140, 352 ms; VA 130, 328 ms; DRUM 116, 292 ms.
- **Restarts:** a load or an engine change on the Part, or a ring that lost what it held (USB's poll drops everything), starts the walk again from the first cell; the end message is sent once, after a whole run. A new request on a port replaces that port's walk.
- **Refused:** with MIDI CONFIG › ECHO off nothing is answered, then or later (as the identity request); PARAM OUT need not be on. A `pp` naming no Part gets no answer.
- **Placement:** the pipe is untouched: the request reaches the UI as any manufacturer's body (`SYSEX_IN`, `CtrlEvent::SysEx`), where `chimera_midi::sysex::dump_request` takes it before learn and the bindings (a `CtrlEvent::Dump` of its own cost bank 1 216 B in `CtrlEvent::pack`); the walk in `ui::param_dump`, an S4 group, called only from bank 2's `ui::midi` (its hear and out passes); its state under 16 B in `MidiUi`. Plain loops, no iterator chains, so no adaptor lands in bank 1.

### `ed`'s stored text is exact both ways (#1038, #1039)
- **`ed set` reads a decimal correctly rounded** (`console::ed::exact::read`): the `f32` nearest the text, ties to even, by integers. The whole part is a `u32` (nine digits, the grammar unchanged: `-12.5`, `440.`, `.5`); the fraction's digits are doubled in place, a bit out each time, until 25 bits stand or a denormal's last place is reached; 150 decimals are kept (a float and the midpoint of two are whole multiples of 2^-150, so of 10^-150) and a longer tail only says something lies past them. What `ed get` printed, `ed set` stores bit for bit. `chimera_theory::tuning::decimal` stays as it is for Scala files, whose cents don't need it and whose callers sit in S0-S3.
- **`ed get` hides nothing:** a stored NaN prints `nan`, an infinity `inf` or `-inf`, and a finite value past `u32` every digit of its whole part (`exact::huge`; `f32::MAX` is 39 digits). The words are not decimals: `ed set` refuses all three (it already did), and the editor's `Protocol.num` reads the line as `Unknown` and keeps what it had. `PROTOCOL` stays 3. The normalized word keeps `num`'s seven places, as in ADR 0297.

### NOTES' chips take their font from their width (#961)
The chips' font is the largest in which every note name from C0 up fits the chip (`orbit::draw::chip_font`), so it depends on how many chips are shown, never on the notes: eight chips are Helvetica Bold 8, six or fewer Bold 10. A chip whose own name is still too wide (a sharp in octave −1 at eight chips) marks its pitch by height, that chip alone. The selection following its note as a pool re-sorts is ADR 0074's and stays.

## Alternatives considered
- **#957, keep the node by index on another engine** (`refocus`'s fallback): node 1 is FLT on ALGO and on VA by luck, not by meaning. By id, or home.
- **#957, restore each Part's remembered page** (`Recall::pages_of`): B*n* would land somewhere the hand didn't ask for; the ticket asks for the page on screen.
- **#984, stock's way:** `docs/parity.md` lists stock's "send the current patch as NRPN" as a panel action (#263). It can come later on the same walk; a controller still needs to ask.
- **#984, an NRPN as the request:** it would need a channel, could collide with a controller's own NRPN bindings, and the chart keeps 127/127 as the null. The ticket asks for SysEx.
- **#984, answer out of both ports** (as PARAM OUT's tee): a DIN controller's request would also flood a DAW on USB.
- **#984, send `R` cells too:** more for a screen to show, but not "what PARAM OUT sends"; adding them later only adds NRPNs a reader already skips.
- **#984, no end message:** a reader could not tell a finished dump from a stalled one. Seven bytes; a controller that ignores SysEx loses nothing.
- **#984, fill the ring:** an echo copy that finds USB's ring full is counted lost, and USB's poll then drops all it holds, the dump with it.
- **#984, a dump on every Sound load (an ECHO option):** the ticket's optional part; it needs a SETTINGS cell, its page golden and guide picture. Left out: a controller asks after its own program change.
- **#1038, make `chimera_theory::tuning::decimal` itself exact:** it is `inline(always)` into Scala's two callers in S0-S3, which has under 8 KB free.
- **#1039, an `ERR` answer:** one corrupt cell would void a whole `ed since`.
- **#961, each chip its own largest font:** naturals large beside small sharps; a row of mixed sizes.
- **#961, the selection stays on its index:** ADR 0074 rejected it (the next turn edits another note).

## Consequences
- Tests and guides that reached "Part 1's home" by B2 then B1 from a page now stay on the page (`flt_page_test` walks home with MINUS).
- A third party can rely on the dump's bytes: the head `7D 43 48`, messages `01` and `02`, `DumpWho`'s three ranges. Message numbers from `03` and the `pp` values `20`-`7E` are free for later.
- `7D` is the non-commercial maker: the head moves with the identity reply's when an MMA ID is bought (`ui::midi::out::ID`'s note).
- The unit gate (a dump read back after a Sound load, on DIN and USB) waits for the next flash.
- A STEP ring's eight chips, and any pool of seven or eight, are always in the smaller font (`orbit_notes_step`, `orbit_notes_step32` re-recorded).
- VA's SHAPE 1 and 2 draw their oscillator's scope (`FocusGlyph::Va(VaGlyph::Wave)`, the glyph WAVE had) for the arc (#980): one cycle of `osc::wave_at` at the knob's WAVE and SHAPE. The issue's "follow KIND" has nothing to follow: 0.4 has no oscillator KIND. No DSP changed.
- `header_dot_pulses` is alone in its test binary and the tuning tests' lock covers every card pass (#1033): both raced on process statics (`meter::sounding_parts`, `TUNINGS`).
- Tempo sync for the delay (#959) is not in this batch: it needs its own decisions (note values against the line's 500 ms, the delay door's signature, what a GRAIN repeat is).

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, a1dd3a9c against this branch, all thirteen budgeted sets built in both trees; each line raised by its set's own delta.

- **Bank 1:** +296 to +320 B on the shipping sets (default 886,408 → 886,728; orbit 900,300 → 900,604, 16,900 B free; bank2-gate +304; orbit,usb-config-writer +296) and +192 to +208 B on the nine bench sets. It is #957's: `Location::next` +140 B and `ChainDef2::locate` +112 B now out of line, `refocus` −70 B, `ParamOut::clear` +24 B out of line, the rest rodata and outlining churn. `nav` steps keys before any door opens, so it stays bank 1's. Nothing of the dump is in bank 1: a first cut gave it a `CtrlEvent` of its own, which cost 216 B there in `CtrlEvent::pack`, and was dropped for the SysEx body path.
- **Bank 2:** S4 +1,440 B on default, orbit and bank2-gate (`exact::read`, `exact::huge`, `ui::param_dump`); S0-S3 +416 (default), +448 (orbit), +384 (bank2-gate): the request's parse and the out pass's call in `ui::midi`, `chimera_midi::sysex`'s two functions, NOTES' `chip_font`. ORBIT's S0-S3 goes from 7,776 to 7,328 B free (under the per-region 8 KB warning, as before; the total keeps 491,328 B). The bench sets: S4 +576 to +608, S0-S3 +32 to +64.
- **Sound:** no DSP file changed; the audio goldens are untouched.

## Sources
- #957, #961, #980, #984, #1033, #1038, #1039; Elektronauts PreenFM3 thread, posts 241 and 251.
- Code: `ui/nav.rs` (`kept`), `chimera-midi/src/sysex.rs`, `ui/midi/mod.rs` (the SysEx bodies' pass), `ui/param_dump.rs`, `ui/midi/out.rs`, `console/ed/exact.rs` (`read`, `huge`), `ui/orbit/draw.rs` (`chip_font`), `dsp/va/mod.rs` (`VA_SPECS`).
- Tests: `nav_test`, `midi_dump_test`, `chimera-midi/tests/sysex_test.rs`, `console_ed_test` (`get_then_set_is_bit_exact`, `get_says_a_stored_value_that_is_no_number`), `orbit_pages_test` (`a_note_turn_changes_only_its_chip`), `va_glyph_test`, `header_dot_test`, `tuning_files_test`.
- ADR 0044 (superseded in part: B*n* from another Part's sound pages), ADR 0297 (superseded in part: the saturated whole part, `ed set`'s float parse), ADR 0074, 0216, 0217, 0258.
- Guide: `docs/guide/MIDI.md`, "Reading a Sound's values".
