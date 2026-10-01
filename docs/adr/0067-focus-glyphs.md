# 0067. Focus glyphs: hand-assigned per parameter, ARC by default; composites draw from set values only

- **Status:** Proposed
- **Deciders:** project owner (2026-10-01); firmware

## Context
Every page's focus band shows the focused parameter's value and, at its
right, an arc gauge (`draw::arc_gauge`). One gauge does not suit every
parameter: a word choice (STRING) runs into the arc, an on/off reads
better as a switch, a level as a slider. The owner chose a set of glyph
kinds, each built as its own story: NONE (word choices: no gauge, the
value text gets the band), SWITCH (two-state), LEVEL BAR (vertical
slider with tick dots), CROSSFADER (horizontal, Octatrack-style), and
animated COMPOSITES, one glyph for up to three params of an effect:
reverb cube (SIZE, TIME, AMOUNT), delay rings (TIME, FEEDBACK, TONE),
chorus braid (RATE, DEPTH, MIX; MODE is the strand count).

## Decision
- **A glyph is hand-assigned on the parameter's spec**, never derived
  from its format or range: `ParamSpec::glyph`, set with
  `.glyph(FocusGlyph::…)`. Every constructor sets ARC, so a spec without
  one is ARC. `tests/focus_glyph_test.rs` pins every registered param to
  ARC unless listed in its `ASSIGNED` table.
- **`FocusGlyph`** names every planned kind: `Arc`, `None`, `Switch`,
  `LevelBar`, `Crossfader`, `Composite(CompositeId)` with `ReverbCube`,
  `DelayRings`, `ChorusBraid`. Slots with no spec (legacy, fixed text)
  show ARC. A page slot may override its param's glyph
  (`ParamSlot::with_glyph`): the glyph demo pages show a glyph on a
  real param before the owner assigns it to any spec.
- **What is drawn is `FocusGlyph::gauge(..) -> Gauge`**, the one place an
  unbuilt glyph falls back to ARC. `Gauge` holds only built glyphs, each
  with its inputs (`Arc { value, bipolar }`, `None`, `Switch { on }`,
  `LevelBar { value, ticks }`, ticks from the format); the focus band
  matches on it exhaustively, so nothing unbuilt can be drawn
  or panic.
- **Word choices take NONE** (owner-approved 2026-10-01): a named
  choice whose value is a word (MODEL, KIND, filter MODE, ALG A/B,
  WAVE, the ENV and LFO TYPE/MODE/FORM/SPEED/HOLD, STEAL, Part MODE and
  OUT, THEME GAMMA and ACCENT). Named choices that are numbers on a
  scale (CRSE, MODES, RATIO, BRIGHT, BLACK) keep ARC. A test makes every
  named choice one or the other, and checks every word of a NONE param
  fits the band at the focus size.
- **Toggles take SWITCH** (owner-approved 2026-10-01): a two-value
  choice where one value is "off" (LFO SYNC, FREE / RETRIG). Two-value
  choices of two peers (ENV and LFO TYPE, Part MODE, STEAL) keep NONE.
  A test makes every two-value choice one or the other, and checks each
  SWITCH word fits before the pill. Chorus MODE (OFF, I, II, I+II), LFO
  SHAPE and SYNC are named on their specs, so they read as words
  everywhere; names are display only, disk codes are unchanged.
- **Set-and-leave levels take LEVEL BAR** (owner-approved
  2026-10-01): unipolar, 0 to max: OUT LEVEL, each operator's LEVEL
  and FDBK, Part LEVEL and its CHR/DLY/REV sends, chorus, delay,
  reverb, tape and comp MIX, delay REV, comp MAKEUP. Bipolar and
  centred params keep ARC; a test checks every LEVEL BAR is unipolar
  from 0.
- **A composite draws from its params' set (stored) values only**, never
  the modulated ones: when it moves, that is always its own animation,
  never automation. The params stay modulatable; their cells and mod
  bars still show modulation.
- **Animation runs on a UI clock** (`animation::UiClock`, frames, ticked
  by `UiState::update`), never the audio thread. While the focused
  glyph animates (`Gauge::animates`), the focus band's dirty key carries
  the frame (`glyph::anim_key`), so the band redraws every frame.
- **Each glyph gets one Demo page** ("Glyph: X", MIX + B6), one glyph per
  page, its slots bound to the params that drive it: several animated
  panels at once can't be judged.

## Alternatives considered
- **Deriving the glyph from the format** (Names → NONE, two choices →
  SWITCH): the owner wants to pick per parameter; a rule would mis-assign
  some and could not express composites.
- **Composites from modulated values:** an LFO on reverb SIZE would move
  the cube, and its own motion could no longer be told from automation.
- **Animating from the audio thread's clock:** the audio thread must not
  do UI work, and the UI frame rate is what the motion is seen at.

## Consequences
- Until a glyph's story lands, assigning it changes nothing on screen;
  each story adds its `Gauge` variant and match arms, and moves its kind
  in `gauge()`. A composite's `set` comes from one function,
  `renderer::composite_set`, which reads the stored params only.
- An animated glyph costs one focus-band redraw per frame while shown;
  only that band, only while focused.
- Demo glyph pages edit the active Part's real sound (GLYPH: ARC turns
  Part 1's VOLUME and PAN), as the older demo pages do.
- Demo glyph pages take `BlockDef` ids 70 and up (68–69 are reserved
  for the nav branch's CHANNELS and OUTPUTS); `focus::MAX_PAGES` (72) is
  raised when a later page needs it.
- A Demo page with bound slots is slot-driven (`PageId::from_nav`), as
  System › THEME is, so each glyph page is data only.

## Sources
- Owner decisions, 2026-10-01 (glyph kinds and composites).
- `chimera-core/src/ui/glyph.rs`, `components::focus_band`,
  `region::RegionData::Focus`, `block_registry::DEMO_GLYPH_ARC`.
