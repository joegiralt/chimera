# 0057. The Part button toggles sound and mixer; the mixer opens on SENDS

- **Status:** Superseded by [0044](0044-one-ladder-one-button-map.md)
- **Deciders:** owner; firmware
- **Absorbed by:** ADR 0044, the navigation ladder. The mixer chain
  becomes its Part rung.

## Context
MIX+B<n> opened the mixer chain on its first page, PART. There encoder C
is OUT (P1/P2/P3), in the same place REV sits on SENDS. The owner turned
"the reverb send" and moved the Part to another DAC pair. Its voices kept
playing on P2 or P3, and the main output went quiet, so the synth seemed
to die. It was not a crash. The desktop sums the three pairs, so the
simulator never went quiet. The write-up measured the move: after MIX+B1
and C +1, P1 went from 0.47 to 0 and P2 went to 0.47. Nothing in the
header said where the Part had gone.

B<n> always went to Part n's sound pages, and pressing it again went home.
To reach a Part's mixer you had to hold MIX.

## Decision
- **Tapping B<n>.** From anywhere but Part n's pages, it goes to Part n's
  sound pages, home, as before. From Part n's sound pages, it goes to Part
  n's mixer. From Part n's mixer, it goes back to Part n's sound pages, on
  the page you left. The rule is `chain::next_on_part_button(from, n)`, a
  pure function of the current chain. `ChainNav::go` restores the page.
- **MIX+B<n>** is still a shortcut to Part n's mixer. MIX+B6 still opens
  the demo.
- **The mixer opens on SENDS** (`block_registry::MIXER_HOME`), then on the
  mixer page last used this session. That page is remembered once, for
  every Part, because most mixer pages are the shared FX. It is not saved.
  PART is remembered only from mixer to mixer (MIX+B<m> on a mixer), so
  LEVEL and PAN can be balanced across Parts. Arriving from a sound page,
  System or Demo, a remembered PART opens SENDS: the trap never returns.
- **The sound page left is kept with its engine.** If the Part's engine
  changed meanwhile, B<n> lands on its home instead.
- **The header says SOUND or MIX:** `PART 2 · SOUND`, `PART 2 · MIX`. The
  map is the chain's own map. Mixer pages are no longer numbered (`SENDS`,
  not `SENDS 2`), because the context already names the Part.
- **OUT warning.** On a Part's sound pages and its mixer, when its OUT is
  not P1, the header shows `OUT P2` or `OUT P3` in WARN, right-aligned
  beside the sounding dot. It takes the place of the CPU readout, which is
  bench-only and not fed on hardware. The readout now also gives way to a
  page name it would overlap.
- **Fit.** The context and name end 6 px short of the warning, or of the
  dot. When a name does not fit, the header uses the page's short form
  (`ALG`, `FLT`), as a cell label does. Without a warning every name is
  whole. `every_header_fits` checks every page, model, ENV suffix and OUT.

## Alternatives considered
- **Move OUT off encoder C, or make it an EDIT+turn.** This fixes the trap
  only on PART. The same muscle memory still lands on PART from other
  pages, and it changes a page layout that is otherwise right.
- **Only open on SENDS, with no toggle and no warning.** A Part left on
  P2 would still be silent without any sign.
- **Remember PART from anywhere.** It brings the trap back on the next
  visit from a sound page.
- **Remember the mixer page per Part.** Most of the chain is the shared
  FX, so a per-Part memory would open the same CHORUS page at different
  positions. One slot is simpler.
- **Keep "again: home" on B<n>.** Going home is one or two MINUS presses
  away, but reaching the mixer without MIX is not. The owner chose the
  toggle.
- **Draw the OUT warning below the header or on the map.** The header is
  where the context is read, and the map belongs to the chain.

## Consequences
- The mixer is one button away from any Part's sound pages. Pressing B<n>
  again no longer snaps home. Tests that relied on this now reach home
  through another Part's button.
- With a warning, most sound-page names show their short form. The
  warning is rare, and it is meant to stand out.
- Screen goldens change: every Part and mixer header reads the new
  context. The mixer recipes open on SENDS.
- When the ladder (0044) lands, `next_on_part_button` and `ChainNav::go`
  move into it unchanged. The mixer chain becomes the Part rung.

## Sources
- Root cause: "Edit a send while notes ring: every voice dies" (session
  write-up, 2026-09-29). The mixer chain's page order is in
  `block_registry.rs` (`MIXER_CHANNEL_BLOCKS`), and the routing is
  `mix_parts` in `instrument.rs`.
- Owner's decisions, 2026-09-30 (Modal 2 plan, Task 15).
- Code: `chimera-core/src/ui/chain.rs`, `ui/components.rs`
  (`header_text`, `header_fits`), `ui/draw.rs` (`MIDDOT`). Tests:
  `tests/part_button_test.rs`, `tests/header_map_test.rs`.
