# 0331. An editor's set takes the cell's focus, as its knob's turn does

- **Status:** Accepted (2026-11-23)
- **Deciders:** owner (bug report), Claude (fix)

## Context
The owner moved MORPH in the browser editor: the value landed (the editor read it back), but the crossfader on the box's screen stayed put. A turn of MORPH's knob moves it.

The redraw was never missing. `ed set` writes through `UiState::edit_to`, the knob's path, and the region keys follow the stored value. What differed was the focus. A knob's turn touches its cell's focus (`UiState::input`), but `ed_cue::landed` never did. The focus band draws the focused cell's glyph only, so an editor's MORPH moved a glyph that wasn't shown. On the ALG page the box's default focus is ALG A, and ADR 0153 draws a picked ALG A or B alone. That froze the diagram too. Every glyph and every viz that follows the focus had the same bug, on every engine.

## Decision
- A set from the editor that lands on a cell of the page shown, for the box's active Part, touches that cell's focus (`ed_cue::landed`, bank 2's `.bank2_ed` section). The band then draws its glyph, and the viz reads it as after a turn.
- A set the box refuses takes the focus too, as a refused turn does: the band shows the cell while its toast says why.
- It leaves the focus alone in these cases:
  - a set off the page shown, or for another Part;
  - a pad's set on a pad the box hasn't selected;
  - while the box is mid-gesture on its focus, which the gesture reads:
    - a choice still held on the shown Part (ADR 0151), so "another cell leaves the hold" still shows the hold;
    - MIDI learn armed or sensing, or MIDI MAP's pointing (ADR 0288): it binds the focused cell, so a drag in the editor never rebinds it;
    - MIX down: MIX + PLUS primes the focused cell.
- `ed_redraw_test` pins each case. The held choice is pinned in `console_ed_engines_test`.

## Alternatives considered
- **Bump a redraw key.** This draws nothing new: the frame was already current.
- **Have the MORPH glyph read the stored value off focus.** This is per-param, and the band has room for one glyph.
- **Move the focus whatever the box is doing.** The editor would then hide a refused choice the box is waiting on, rebind an armed learn, or prime another cell.

## Consequences
- The box follows the editor's last edit: a drag in the editor moves the box's focus, as the knob would.
- The code is in `landed`, which is already bank 2's: +160 B there on the default set, +192 B on ORBIT. Bank 1 measures +8 B over 0.4's lines on six sets (outlining), within the 64 B tolerance; the lines stay.

## Sources
- ADR 0067 (focus glyphs), 0151 (held choices), 0153 (picking ALG B), 0285 (`ed set` as a knob turn), 0288 (pointing).
- `chimera-core/src/ui/ed_cue.rs`, `chimera-core/tests/ed_redraw_test.rs`, `chimera-core/tests/console_ed_engines_test.rs`.
