# 0334. The screen kit draws the MIXER's FX and takes a frame

- **Status:** Accepted (2026-11-23)
- **Deciders:** firmware, the editor (its MIXER page's floating frame)

## Context
`chimera-screen` (`screen.wasm`, ADR 0297's mirror) drew the five engines' pages only. The editor's MIXER page has a frame for the selected effect's picture, the art the unit's MIXER/FX pages draw in the focus band: the chorus braid, FLANGER's moiré and PHASER's ray gun (ADR 0324), the delay's rings, the reverb's cube (QUAD) or tail, the master's compressor curve. Those glyphs move with the UI clock (`UiClock`, ticked by `update`), which a mirror's paint never set: a render is a still (ADR 0297), so an animated glyph had no defined phase.

## Decision
- **MIXER is the kit's sixth engine:** `engines_ptr`'s list is `EngineType::PLAYABLE`'s names, then `MIXER` (`Schema::Mixer`'s token); the engines keep their numbers. Its mirror (`Mirror::mixer`, `Board::Mixer`) walks `ed describe MIXER`'s page numbers, showing the shared FX pages (CHR, DLY, CHAR, GRAIN, REV, MORE, DUCK, MOR2, MST, LVL) as Part 1's mixer enters them (`Location::fx`); a Part's own (PRT, VOI, EQ, SND) is `NotShown`. It is fed the FX cells' stored values through `apply`, as the engines are; `reset` puts the project's default FX back.
- **`show` takes a frame:** `show(engine, page, region, focus, pad, frame)`. The mirror sets the UI clock to `frame` before it draws (`UiState::frame_for_test`, test-support only), so the same frame is the same picture whatever was drawn before (ADR 0297 kept), and successive frames animate. A caller passing five arguments gets frame 0.
- **The pictures' calls:** CHORUS page 0, DELAY page 1, REVERB page 4, each region `gauge` with a focus on a cell whose glyph is the picture (CHR any of 0-3; DLY and REV 1-5); the master's curve page 8 region `screen`, MST › LVL's SMART EQ band page 9 region `screen`.

## Alternatives considered
- **A parallel export set for the mixer** (`mixer_show`, ...): two ABIs for one renderer; the engine number and page walk already carry it.
- **Ticking `update` `frame` times:** the cost grows with the frame and other state (eases, toasts) would step with it.
- **A separate `set_frame` export:** state between calls the editor would have to order; an argument is one call.

## Consequences
- Firmware unchanged: `chimera-screen` is host and wasm only, and `frame_for_test`/`UiClock::at` are `test-support`.
- The editor animates a picture by repainting with a rising frame (the unit runs 20 a second, `UI_FPS`).
- A new shared mixer page appears in the kit with its `ed describe MIXER` number.

## Sources
- `chimera-screen/src/{lib,abi,pages}.rs`, `chimera-screen/tests/mixer.rs`.
- ADR 0297, ADR 0324, ADR 0289 (`ed_page_at` and `Schema`).
