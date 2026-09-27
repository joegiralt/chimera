# Algo Engine — Pages, Gang Edit and Level Scaling (sub-project 4)

Sub-project 4 of the algorithmic engine: see `2026-09-26-algo-engine-design.md` § Roadmap and its addendum on the home page and operator navigation. It adds per-operator pages, gang edit, TX81Z level scaling and a living ALGO diagram. It uses no audio CPU beyond a few stores per block.

## Intent

- Every operator can be edited on its own page, reached in one gesture.
- One parameter can move across all six operators at once without losing their relative settings.
- LS (level scaling) completes the TX81Z scaling pair next to RATE SCALE.
- The ALGO page shows what is sounding, not just the routing.

**Done when:** all of these are covered by host tests and screen goldens, and have been seen on the chip:
- long-press operator pages;
- gang edit;
- the LEVEL SCALE group page;
- the living diagram.

## Operator pages

### Opening
- On a Part whose engine is Algo, holding **B1–B6 for ≥ 1 s** opens operator *n*'s page for the Part being edited. The edited Part does not change.
- A press shorter than 1 s selects the Part, as today, but it now fires on **release**.
- On a Modal Part, a long hold does nothing, and the release still selects the Part.
- Long-press detection is a pure function `press_kind(pressed_at, now, released) -> PressKind { Pending, Short, Long }`. The UI loop feeds it each tick.

### Content
- **Viz band:** the operator's wave, one cycle drawn from its table, next to its envelope, drawn from AR, D1R, D1L, D2R and RR.
  - While a voice of the Part sounds, a playhead dot rides the envelope at the operator's current level.
- **Cells:** WAVE, COARSE, FINE, LEVEL, FB and VEL, all bound to `AlgoOp(n)`.
  - This reuses the `SelectOp` / `SelectedOp` binding kept in sub-project 1.

### Navigation
- On an operator page, **PLUS / MINUS** step to the next or previous operator, wrapping 6 → 1 and 1 → 6.
- **EXIT**, or pressing the same B button again, returns to the page the operator page was opened from.
- Every operator page is reachable in the all-pages walk.

## Gang edit

- **Where:** only on group pages, which show one parameter across six operators: WAVE, COARSE, FINE, DETUNE, LEVEL, VELOCITY, the five ENV stages, RATE SCALE, LEVEL SCALE and FEEDBACK.
- **Gesture:**
  1. Hold **MINUS**, then turn any encoder.
  2. The first turn during the hold starts the gesture. It records every operator's value for that page's parameter as its *anchor*.
  3. The gesture tracks the total turned, Δ. Each operator's value is `clamp(anchor + Δ)`.
  4. An operator that hits a limit stops there. Turning back restores it, because the anchor is kept.
  5. Releasing MINUS ends the gesture and drops the anchors.
- **MINUS on its own:** a press and release of MINUS with no turn in between does what it does today. Once an encoder turns during the hold, the MINUS action is suppressed on release.
- **Smoothing:** values reach the engine through the usual parameter path, so they are lerped and never snapped (project rules).
- **MIX + MINUS** (un-prime) is unchanged. Gang edit needs MINUS without MIX.
- The gesture state is a small pure struct, `GangGesture { anchors: [u8; 6], delta: i16 }`, with a pure `apply(anchor, delta, spec) -> u8`.

## Level scaling (LS)

- **Parameter:** `AlgoOpParams.level_scale: u8`, 0–99. The default of 0 means no scaling.
- **Page:** the group page **LEVEL SCALE**, listed after RATE SCALE in OSC's sub-pages.
- **DSP:** at note-on, an operator's level is attenuated by `LS/99 × max(0, note − 48) × 0.375 dB`. That is 0 below C3 and linear in semitones above it, reaching about −30 dB at LS 99 over 7 octaves.
  - This follows the TX81Z's one-sided scaling. The attenuation is computed once per note-on in the engine, so it costs nothing per sample.
- LS is not a modulation destination.

## Living ALGO diagram

It builds on sub-project 1's A/B blend and #30's overlap fix.

- **Node size:** each node's radius scales with its operator's LEVEL, between 60 % and 100 % of the layout radius. The overlap guarantee is checked at 100 %.
- **Rings:** a ring around each node is lit in proportion to that operator's current envelope level. It is dark when nothing sounds.
- **Links:** each link's stroke weight follows its effective weight after MORPH. A link that only exists in B has weight 0 at MORPH 0 and full weight at MORPH 1.

## Envelope telemetry

- The audio thread publishes `OpLevels = [u8; 6]` once per block through a `TripleBuffer`, the same mechanism the scope uses.
  - Each value is the operator's envelope level, scaled 0–255, for the edited Part's most recently started voice. It is all zeros when that Part has no sounding voice.
- The edited Part's index reaches the audio side the way other UI state does, through `AudioShared`.
- The UI reads the latest frame when it redraws the ALGO page or an operator page.
- **Cost:** 6 stores and a publish per block. The RAM is three copies of 6 bytes.

## Tests

All tests run on the host.

- **Long-press:**
  - 999 ms → Short, and 1,000 ms → Long.
  - A release before 1 s selects the Part on release.
  - A long hold on a Modal Part does not select anything.
  - Pressing the same B again returns to the previous page.
- **Operator pages:**
  - PLUS and MINUS wrap across all six operators.
  - EXIT returns to the page it came from.
  - Each cell edits `AlgoOp(n)`'s parameter.
  - Every operator page is in the all-pages walk.
- **Gang edit:**
  - With anchors 95, 50 and 0 at +10, the values are 99, 60 and 10.
  - Turning back −10 restores 95, 50 and 0.
  - Releasing MINUS ends the gesture.
  - MINUS without a turn keeps its old action.
  - It does nothing on a non-group page.
  - Its edits pass through the lerp.
- **LS:**
  - LS 0 leaves every level unchanged.
  - With LS > 0 the level is unchanged at or below C3, falls monotonically above it, and is about −30 dB at LS 99 seven octaves up.
- **Telemetry:**
  - It publishes zeros with no voice.
  - The published levels follow a held note's envelope, and fall to zero after release.
- **Diagram:**
  - Every node stays in the band with no overlap at 100 % radius, for all 32×32 pairs at 101 MORPH steps.
  - A link that exists only in B has weight 0 at MORPH 0.
  - The rings are dark with no voice.
- **Screen goldens:**
  - an operator page with a held note (the dot on its envelope);
  - the LEVEL SCALE page;
  - the ALGO page with a held note.

## ADRs

- **Long-press B1–B6 opens operator pages, and Part select fires on release.**
- **Gang edit is hold MINUS + turn, anchored, clamp-and-remember.**

## Out of scope

- A gang edit on operator pages.
- DX7-style breakpoint scaling, which has left and right curves.
- LS as a modulation destination.
- The modulation modes (sub-project 2) and more waves (sub-project 3).
