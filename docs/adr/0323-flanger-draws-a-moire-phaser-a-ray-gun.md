# 0317. FLANGER and PHASER draw what they do to the spectrum

- **Status:** Accepted (2026-11-22)
- **Deciders:** the owner (design approved 2026-11-22)

## Context
The focus band drew one glyph, the braid (`glyph::Braid`), for every chorus MODE: OFF, I, II, I+II, FLANGER and PHASER. The braid pictures voices twisting round a dry line, which is what a chorus does; for FLANGER and PHASER it differed only in twist count and speed, so it said nothing about what those two do to the sound. Bank 1 is frozen (new code goes to bank 2 behind a door).

## Decision
- **Pictures by MODE** (`ui::notes::chorus::picture`, an exhaustive match): OFF, I, II and I+II keep the braid; FLANGER draws a **comb**, PHASER a few **wide dips**. Both are a response curve in the braid's box (80 × 56 px, 2 px in): flat on a top line, cut down toward a floor line.
- **FLANGER's comb:** six evenly spaced notches, `(1 − sin πd)²` at `d` spacings off a notch's centre, measured from the column's nearest edge, so a notch between columns still reaches the floor as it slides (no flicker in its depth). They slide together, the full DEPTH ±1.5 spacings.
- **PHASER's dips:** three, `1/(1 + q²)²` each, combined as filters do (`1 − Π(1 − cut)`); the first at 0.22 of the width, the others 2.6× and 4.2× as far, each as wide as 0.6 of the first's place, so they spread and widen as they glide up (a phaser's notches on a linear axis). The full DEPTH moves the first ±60 %.
- **RATE** sets the sweep's speed with the braid's clock: phase `frame × (0.05 + 0.45 RATE) × speed` in f64 (`glyph::wrap`), speed 0.5 for the comb (its notch never faster than the braid's crest, about 4 px a frame at full RATE) and 0.35 for the dips (they glide). The sweep is `DEPTH × sin(phase)`. **DEPTH** is how far, **MIX** how deep they cut: MIX 0 is a flat line.
- **The braid's look:** the floor line takes the braid's dry-line colours by MIX (INK2, MID, FAINT), the curve ACCENT 1 to 3 px by MIX; palette colours only. The focused part lights as the braid's: MODE a dot at the left, RATE a bead riding the leading notch (the comb's third, the first dip), DEPTH the sweep's bounds dotted in MID, MIX the floor line in INK.
- **MODE as stored:** the band's braid takes its MODE from the stored chorus MODE (`f.fx.chorus.mode`), not the eased set value, so a MODE change switches the picture at once and never rolls through the modes between (ADR 0308's rule for VA's MODE). This applies to the braid's own modes too.
- **Bank 2:** `ui::notes::chorus`, asked through `UiDoor`'s `notes` entry (`Ask::Chorus`), links with `ui::notes`' S4 group. Bank 1 gains the stub in `components::braid` (the door call; the braid is drawn when it answers no picture) and the stored-MODE read in the renderer. Closed, FLANGER and PHASER draw the braid, as before.

## Alternatives considered
- **A new `Gauge` variant per picture:** every `Gauge` match in bank 1 (layout, rect, animates, keys) would grow; the braid's variant already carries every value the pictures need.
- **A true flanger sweep (notches fanning out from the left as the delay changes):** truer to the delay line, but the owner's design asks for notches that slide together, and a fan reads as a zoom at this size.
- **Sampling the comb per column:** a 2 px notch sliding between columns flickers its depth by several pixels each frame; the edge-distance form keeps the floor reached.

## Consequences
- Bank 1 grows by the door stub and the stored-MODE read: BANK1_DELTA. Bank 2: BANK2_DELTA (`ui::notes::chorus`).
- New goldens `mixer_fx_chorus_flanger` and `mixer_fx_chorus_phaser` (and their master-tape rows); their docs screenshots.
- A future chorus MODE must choose its picture: `picture` is exhaustive.

## Sources
- `chimera-core/src/ui/notes/chorus.rs`, `chimera-core/tests/chorus_glyph_test.rs`.
- ADR 0308 (MODE read as stored), ADR 0292 (`UiDoor::notes`), ADR 0272 (S4 groups).
