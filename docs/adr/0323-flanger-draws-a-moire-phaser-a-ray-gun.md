# 0323. FLANGER draws a moiré, PHASER a ray gun

- **Status:** Accepted (2026-11-22)
- **Deciders:** the owner (designs chosen 2026-11-22 from animated concepts: F1 moiré, P4 ray gun)

## Context
The focus band drew one glyph, the braid (`glyph::Braid`), for every chorus MODE: OFF, I, II, I+II, FLANGER and PHASER. The braid pictures voices twisting round a dry line, which is what a chorus does; for FLANGER and PHASER it differed only in twist count and speed. A first cut drew their frequency responses (a comb of notches, a few wide dips) as lines; the owner rejected it: lines are for waves. The chorus pictures are objects, as the braid's barber pole and the boxed glyphs are. Bank 1 is frozen (new code goes to bank 2 behind a door).

## Decision
- **Pictures by MODE** (`ui::notes::chorus::picture`, an exhaustive match): OFF, I, II and I+II keep the braid; FLANGER draws a **moiré**, PHASER a **ray gun**, in the braid's box (80 × 56 px, 2 px in), filled boxes only.
- **One clock:** the braid's, `frame × (0.05 + 0.45 RATE)` radians, in f64 and wrapped before it reaches f32 (`glyph::wrap`), so motion stays smooth however long the unit has run.
- **FLANGER, a moiré:** a back comb of 2 px bars every 5 px, fixed, MID; over it a shorter front comb of 2 px bars, ACCENT, INK2 where a front bar covers a back bar, so bright bands beat through where they line up. The front comb slides right (RATE: 3 px a radian, as a share of its own pitch, so a stretch never jolts it) and its pitch stretches ±18 % by DEPTH at half the clock. MIX is how solid the front comb is: solid at the braid's top MIX level, every second or third row below it, none at MIX 0 (the back comb alone).
- **PHASER, a ray gun:** a blocky hand gun at the left, ACCENT: body with a panel slot, grip, a MID trigger-guard notch under the body's front, an emitter nozzle. DEPTH morphs it from a skinny pistol (body 17 × 5 px) to a wide boxy blaster (22 × 12 px) and fattens the beam (1 to 6 px). Four pulses travel the beam to the box's right edge, fired twice a turn of half the clock (RATE sets how fast, at most about 4 px a frame). MIX is the beam's brightness: its band FAINT or MID and its pulses MID, ACCENT or INK2 by the braid's MIX level; at MIX 0 the beam is off and only the gun shows. An original shape, not any film prop.
- **Focus,** in INK as the braid's: MODE a dot under the picture's left, for both. Ray gun: RATE the pulses, DEPTH the gun's outline, MIX the beam's band (drawn at MIX 0 too, so the turn has a mark). Moiré: RATE the front comb (what slides); DEPTH the bands where the bars line up, the rest of the front FAINT (the stretch is what makes them beat); MIX the back comb (what the front is weighed against, as MIX lights the braid's dry line).
- **MODE as stored:** the band's braid takes its MODE from the stored chorus MODE (`f.fx.chorus.mode`), not the eased set value, so a MODE change switches the picture at once and never rolls through the modes between (ADR 0308's rule for VA's MODE). This applies to the braid's own modes too.
- **Bank 2:** `ui::notes::chorus`, asked through `UiDoor`'s `notes` entry (`Ask::Chorus`), links with `ui::notes`' S4 group; no new door entry. Bank 1 gains one out-of-line stub, `bank2::chorus` (the door call, false when closed), called from `components::braid`, and the stored-MODE read in the renderer. Closed, FLANGER and PHASER draw the braid, as before.

## Alternatives considered
- **Frequency responses as lines** (a comb of notches, a few wide dips): built first, rejected by the owner: lines are for waves.
- **The other concepts** (F2 a jet tunnel of nested boxes, F3, F4 a snake of boxes and its delayed shadow; P1 to P3): shown animated; the owner chose F1 and P4.
- **A new `Gauge` variant per picture:** every `Gauge` match in bank 1 would grow; the braid's variant already carries every value the pictures need.
- **The door call inline in the generic `braid`:** +108 B in `draw_gauge`; one out-of-line helper is smaller.

## Consequences
- Flash, against 0.4 6fb04d0e: bank 1 +184 B on ORBIT (909,280 B), +168 B default (894,648 B): the door call (`bank2::chorus`, 76 B, and its call in `draw_gauge`) and codegen churn; the call inline in `braid` measured the same, and in `draw_gauge`'s arm +248 B. The budget lines stay at 0.4's (owner, 2026-11-22). Bank 2 +1,856 B on ORBIT (S4 +1,888 B, S0-S3 −32 B): `ui::notes::chorus`.
- New goldens `mixer_fx_chorus_flanger` and `mixer_fx_chorus_phaser` (and their master-tape rows), with their docs screenshots.
- A future chorus MODE must choose its picture: `picture` is exhaustive.

## Sources
- `chimera-core/src/ui/notes/chorus.rs`, `chimera-core/tests/chorus_glyph_test.rs`; the owner's concepts page (F1, P4; 160 × 96 px, scaled to the band's box).
- ADR 0308 (MODE read as stored), ADR 0292 (`UiDoor::notes`), ADR 0272 (S4 groups).
