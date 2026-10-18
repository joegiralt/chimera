# 0150. The header shows the current Part as six dots (extends 0126)

- **Status:** Proposed
- **Deciders:** project owner

## Context
Issue #562: a tester found `PART 1 · SOUND` too quiet to say which Part a
page edits. The owner asked for a clearer indicator, possibly six dots, and
that the current Part's dot be bright green: the accent is hard to see at
that size.

## Decision
- A Part's SOUND and MIX headers draw six dots in a row above the text, from
  the left margin (`theme::PART_DOT_*`), one a Part. The text stays.
- The current Part: a filled dot one pixel larger, in `theme::PART_LIVE`
  (`#29ff29`), whatever the ACCENT. A Part with a voice sounding: a 1 px
  accent ring. The rest: a small `FAINT` dot.
- `PART_LIVE` is a sixth exact-swap token in light mode (`LIVE_LIGHT`,
  darkened to the same 3:1 rule as WARN and ALERT), so INVERT keeps it green
  (extends 0126).
- The audio publishes the sounding Parts once a block as a `PartSet`
  (`meter::publish_sounding_parts`); the UI reads it once a frame. A voice
  counts from its note-on until it ends, release tail included.
- The header region is keyed on `HeaderStatus`, which holds the current Part
  and the set, so it redraws only when either changes.
- ORBIT's RING and SCENE headers and SETTINGS draw no Part dots: they name a
  ring, scene or breadcrumb, not the edited Part.

## Alternatives considered
- **A large "P1"**: crowds the page name on a 240 px line.
- **Dots right of the text**: collide with the status and beat dots and
  with long page names; the row above the text is empty.
- **Accent for the current Part**: the owner found it too dim at 7 px.

## Consequences
One more compare per pixel in light mode's flush. A global atomic joins
`MASTER_GR` and `FILTER_CLIP`. Every Part and mixer screen golden moves.
