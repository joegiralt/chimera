# 0297. `ed get` prints a stored value that reads back as itself, and the mirror draws a still

- **Status:** Accepted (2026-11-20)
- **Deciders:** firmware, the editor (#1035, found by `chimera-screen`'s `a_render_is_the_desktop_simulators`)

## Context
`chimera-screen`'s mirror (the editor's glyphs, ADR 0285's reader) is fed the stored values `ed get` and `ed since` print and must draw what the unit draws. Its test against the simulator failed once a new parameter moved its random Sounds (it had passed on one seed by luck). Two causes:

1. **The stored value didn't read back.** `val <addr> <norm> <stored> "<text>"` printed both numbers through `describe::num`: seven decimals, by float arithmetic. An `f32` needs up to nine significant digits, so a value such as 0.542487 printed as text that parses to its neighbour, 0.5424871: the mirror held the unit's Sound an ulp off (ENV 3's REL, an LFO's RATE), and an ulp can move a pixel.
2. **A render depended on the renders before it.** `UiState::update` steps the display LFOs (the stand-ins that animate a modulated cell's bar and gauge) once a frame, whenever the Sound routes anything, and eases the map's scroll. The mirror ticks once a paint, one mirror an engine for every `<chimera-screen>` on the page, so a gauge sat wherever the paints before it had left the LFO: the same spec painted different pixels (5 on PIT's PITCH gauge in the test), and the test's second pass compared a fresh mirror with a simulator a pass older.

Core's float `Display` would print exactly, but it is 20 KB of bank 1 that `ed` can't be placed out of (the reason `num` exists).

## Decision
1. **The stored value prints in the fewest decimals that read back as the same `f32`** under a correctly rounded parse (Rust's, JavaScript's): `console::ed::exact::stored`. Exact, in integers: the fraction is `n / 2^s`; its digits come off a 160-bit remainder a decimal at a time; it stops at the first place where what is cut (or, rounding up, added) is under half the gap to the neighbour on that side (half an ulp; a quarter below a binade's lower edge). A value on a step prints as before (`0.5`, `0.0078125`, `127`); one between steps prints up to nine significant digits. `-0` prints `0`; a whole part past `u32` holds at its top (no stored value is near it).
2. **Still a plain decimal:** an optional `-`, digits, an optional point and digits, never an exponent. The editor's `Protocol.num` and the unit's own `ed set` (`chimera_theory::tuning::decimal`) take it as they did. `PROTOCOL` stays 3.
3. **The normalized value keeps `num`'s seven decimals,** and so do `describe`'s min, max, step and default: a display's place and a spec's constants, read by nobody for exactness. The fixtures don't move.
4. **A mirror render is a still:** before its frame, the display's own motion is put at rest (`UiState::still_for_test`, test-support only, so no firmware build holds it): the display LFOs back at their start, the map's scroll on its page's row (the values' animators are already snapped by the page's entry). The picture is the Sound's, the page's and the focus's alone. A modulated cell shows its first frame of motion, as a simulator just opened on that page does.
5. **The test holds both, on four seeds** (0x5EED_0000, 0x5EED_0100, 0x0BAD_C0DE, 0x1035_1035, each plus the engine's index): the mirror's stored values are the simulator's bit for bit, not only to `ed get`'s text; every page's pixels are the simulator's at the same still; and a page drawn first, twice, and after every other page is the same picture (`a_render_depends_on_nothing_drawn_before`).

## Alternatives considered
- **Nine significant digits, always:** reads back, but 0.1 prints `0.100000001` and every fixture and answer grows.
- **More fixed decimals (nine, twelve):** still not exact for small values (a leading zero costs a digit), and longer for all.
- **The value's bits in hex:** exact and short, but a second number format in a text protocol people read, and a new parser in the editor.
- **Core's `Display`:** 20 KB of bank 1.
- **Leave the display LFOs running and build a fresh simulator for each comparison:** the test would pass, and the editor's stills would still depend on what was painted before them.
- **Draw the mirror's stills with no modulation at all:** not what any unit frame shows; the first frame is.

## Consequences
- A `val` line's stored word can be longer between steps (up to nine significant digits; under 1e-7, more: 46 decimals at the very most). `the_worst_drum_answer_fits_one_answer` now says its assumption outright: values on their params' steps (as its points always were); it gives 29,900 B of 32,000. An answer that outgrew the bound is refused whole (`ERR ed since too large`, ADR 0285), never cut.
- The unit's own `ed set` reads a decimal by float arithmetic (`i + f / 10^k`), not correctly rounded: a value `ed get` printed, sent back as text, can land an ulp off. The mirror takes values as floats, so this doesn't reach it; making `decimal` exact is not done here (private issue #1038). A stored NaN or infinity prints saturated (private issue #1039).
- The editor's screens of a Sound whose LFOs move a shown cell change once: to the still, from wherever the page's paint history had left them.
- Flash: below.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, 9c197f842 against this change, every budgeted set built in both trees. Bank 1: +0 B on every set (orbit 900,300 B, 17,204 B free; all thirteen unchanged). Bank 2 used: +768 B on default, orbit and bank2-gate, all in S4 (`ed`'s group), each line raised by exactly that; S0-S3 on ORBIT stays at 7,776 B free. `still_for_test` is test-support's: in no firmware build.

## Sources
- #1035; `chimera-screen/tests/screen.rs`.
- Code: `chimera-core/src/console/ed/exact.rs`, `value.rs` (`line`), `ui/mod.rs` (`still_for_test`, `update`'s display LFOs), `chimera-screen/src/lib.rs` (`Mirror::show`).
- ADR 0285 (the editor's protocol), ADR 0290 (`ed since`).
