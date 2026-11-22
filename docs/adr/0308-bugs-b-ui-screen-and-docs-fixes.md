# 0308. Fix the 0.4 UI, screen and docs bugs as recorded here

- **Status:** Accepted (2026-11-21)
- **Deciders:** owner (every known bug ships fixed in 0.4), Claude (the batch)

## Context
The owner's rule for 0.4: every known bug ships fixed. Batch B took the
open `bug` issues on the UI, the screen and the docs. Most were small and
fixed in place; a few needed a choice that later work should not undo
without a reason, and the batch's bank-2 code moved its budget lines.

## Decision
- **The header and map key on the Part's chain (#1068).** `loc_tag`'s
  place for a Part's pages is `64 + 8 × PartChain + n`, so a program change
  that swaps the engine under a page that stays put redraws the strip and
  the tabs on the frame the body does. FRM's own map key folds into it.
- **HOLD under 0.1 ms reads three places (#159).** `0.001 ms` and up, never
  `0.0 ms`; any time law whose value is above 0 and under 0.095 ms does the
  same. No µ: the fonts are ASCII.
- **A fragmented source reads `FRAGMENTED · SILENT`, its cell `FRAG`
  (#643).** True, inside the band, and no copy promised: `put` will not
  replace a sealed file (ADR 0106).
- **A browser row's cost stops 3 px clear of the widest tag (#640).**
  `COST_RIGHT = TAG_RIGHT − TAG_ROOM − 3`; the name's gap to the cost or tag
  is 4 px (was 6), so the names keep their room.
- **MST's `OUT` sits 3 px lower (#1066)**, clear of KEY's hint by three rows.
- **ENV A's stage bars show the routes they prime (#149).** ATTACK, DECAY,
  RELEASE and HOLD move by the slot's TIME sum as `ACoefs` takes it,
  `v − 5·Σ/oct` of each law's octaves (Σ clamped to ±1); SUS by the LEVEL
  route's peak, `v × clamp(Σ, 0, 1)`. Bank 2's (`ui::notes`, `Ask::Stage`);
  closed, the bars stay as they were.
- **A glyph's params off its page ease (#284, #365).** Each gets a lane: a
  param and its value, eased at `KNOB_SPEED`, kept while the last draw
  asked for it, at most `TAPE_GLYPH_PARAMS.len() + 1`. The lanes and their
  pass are bank 2's (`notes::Lanes`, `Ask::Ease` and `Ask::Lane`); bank 1
  keeps two door calls and `eased_set` out of line for every size. Closed,
  the glyph reads the stored values, as before. The still glyphs' region
  keys and the composites' per-frame redraw then follow the lane.
- **The reverb's TAIL takes SYNC and the reverb's clock (#366)**: the
  lengths the reverb plays, at its tempo and fold.
- **Under LOOPFM, WAVE's glyph is `Gauge::VaLoop` (#667)**: the oscillator's
  `lbfm` at `target_b0(BLEND, WAVE)`, as the engine plays it; SHAPE, which
  LOOPFM ignores, leaves it.
- **Tap and hold (#289).** In a frame stalled past `HOLD_MS` across a
  release and a re-press, the release's tap wins and the new press's hold
  fires the frame after. A key pressed while MIX was down, MIX let go
  after it inside the same frame (`chorded`, MIX pressed in that frame or
  held from before), is muted, as a held MIX mutes it.
- **WAVE on a source it can't play (#709, owner 2026-11-22):** the band
  reads `NEEDS A WAVE TABLE` (178 px), the hint's place `TURN SOURCE TO A
  WAVE TABLE`; the owner's `NEEDS A WAVE TABLE · TURN SOURCE` is 312 px.
  The SOURCE list by machine and a playable source on a MODEL change are
  0.5.0's enhancement.
- **PART's LIVE SNAP row is ORBIT's alone (#323).** Without ORBIT no
  harmony reaches `live_note`, so the row did nothing. What the page should
  draw is an enhancement, filed apart.
- **Budgets.** Bank 2's lines rise by the code above, measured:
  default and bank2-gate +1056 B, orbit +960 B. Bank 1 stays inside its
  lines.

## Alternatives considered
- #1068: forcing a full redraw on every program change; the key is cheaper
  and covers any other path that swaps a chain.
- #159: µs (`1 us`): no µ glyph, and `us` reads oddly beside `ms`.
- #643: a fix in the words (`COPY IT AGAIN`) is false
  on the internal card; `FRAGMENTED · WON'T STREAM` and `· NO STREAM` are
  wider than the band.
- #640: moving the cost 4 px left alone clipped GLASS PLUCK's K.
- #284: lanes in `Renderer` in bank 1 cost 2 KB there (inlined in every
  composite and the layout shift of a bigger `UiState`); a static of lanes
  in bank 2 races between host tests.
- #289: returning two presses a frame; the hold a frame later costs
  33 ms once in a stall that is itself the bug.

## Consequences
- Every change above is pinned by a test that failed before it.
- Goldens moved: env_a, row_mod (#159); every browser and library list
  (#640); mixer_master, mixer_master_key (#1066);
  mixer_fx_reverb_gate_sync (#366); docs/screens re-recorded whole (#454,
  #525).

## Sources
- Issues #1068, #1066, #149, #159, #284, #365, #366, #640, #643, #667,
  #289, #323, #454, #525.
- ADR 0106 (sealed sources), 0129 (VA glyphs), 0162 (LOOPFM), 0198 and
  0272 (budgets), 0289 (inlined helpers bank 2 calls).
