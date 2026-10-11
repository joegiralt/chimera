# 0121. Bound SMEAR by the pitch a held tone plays, and run RATE 0.1 → 2 Hz

- **Status:** Proposed
- **Deciders:** project owner, 2026-10-11 ("make SMEAR clearly audible": at SMEAR max a lush, moving, chorus-like tail on every TYPE, never detuned; ≤ ~15 cents at max RATE, ~5–10 at the default RATE). Firmware measured and chose the numbers.
- Amends [0095](0095-every-reverb-type-matches-its-midiverb-ii-presets.md) and [0085](0085-the-master-reverb-picks-a-type.md): SMEAR's bound and depths, and RATE's law. Supersedes nothing else.

## Context
The owner found SMEAR "doesn't seem to do anything". The investigation
(ledger `reverb-smear.md`) found it wired on every TYPE but nearly still:

- **RATE was slow.** The default RATE played 0.13–0.45 Hz (a new wander
  target every 1–4 s), and the tops were 0.5–2 Hz.
- **ROOM, HALL and BLOOM squared the knob.** The engine scaled the
  depth that `knobs()` had already scaled by SMEAR, so SMEAR 0.5 played a
  quarter of the depth, 0.3 a tenth.
- **The depths came from a worst case.** ADR 0085 capped each loop's
  smeared excursion at 1.45 % of its round trip, 25 cents if every line
  sat at its extreme together and stayed there. That is a static bound:
  it says nothing about how fast the pitch moves, and the smeared lines
  are allpasses, whose phase moves up to (1 + g)/(1 − g) times faster
  than a plain delay's near their resonances, so it did not even bound
  what plays.

So a held tone's pitch was measured instead, the way it is heard. Its
power-weighted centroid within ±60 Hz, in 100 ms windows, reads a
reverb's mixture of differently shifted copies as their mean shift; zero
crossings read it as phase slips. Four probes (350, 700, 1 000 and
1 400 Hz) pool their windows over 90 s at TIME 1, and the 95th percentile
of |cents| is "the pitch deviation" (P95). It reads a pure ±10-cent
vibrato as 10.0, and SMEAR 0 as 0–2.

Before this ADR, at SMEAR 1 on the starting knobs (P95, cents):

| TYPE | default RATE | P95 | RATE's top | P95 |
|---|---|---|---|---|
| QUAD | 0.22 Hz | 4.8 | 1 Hz | 9.4 |
| ROOM | 0.32 Hz | 1.8 | 1 Hz | 4.7 |
| HALL | 0.30 Hz | 7.5 | 1.5 Hz | 22.4 |
| BLOOM | 0.13 Hz | 3.3 | 0.5 Hz | 8.3 |
| GATE | 0.45 Hz | 4.1 | 2 Hz | 15.4 |
| REVERSE | 0.45 Hz | 2.7 | 2 Hz | 7.4 |

A held tone's pitch at a given depth rises with RATE: as the depth times
the rate on a single pass (GATE's and REVERSE's decorrelators), and about
as its 0.6–0.9 power in a loop. The owner's two numbers, ~5–10 cents at
the default and ≤ 15 at the top, so allow the top about twice the
default: RATE's top cannot be 3–4 Hz without the default falling under 5
cents.

## Decision
1. **SMEAR's bound is the pitch a held tone plays:** at SMEAR 1 and
   RATE's top, the four probes' P95 is at most 15 cents on every TYPE at
   SIZE 0, its start and 1. The static 1.45 % excursion bound (ADR 0085,
   the reverb TYPEs spec § The SMEAR bound) no longer sets a depth; each
   engine still clamps its smeared reads to twice its cap, and the read
   bounds' const asserts hold at the new caps.
2. **RATE is 0.1 → 2 Hz, exponential, on every TYPE** (QUAD's
   `ring::smear_hz` too), and it starts at 0.75 (`RATE_DEFAULT`): 0.95 Hz,
   a chorus's speed, on every TYPE and in `ReverbParams::default()`.
3. **SMEAR scales its depth linearly to the cap**, on every TYPE: D =
   SMEAR × min(depth at 1, cap × loop). `EngineKnobs::smear_full_ms` is
   the depth at 1, which the engine scales by SMEAR's ease; `smear_ms` is
   the depth that plays, what the cell shows. This ends ROOM's, HALL's
   and BLOOM's squared law.
4. **The depths at SMEAR 1:**

| TYPE | depth at SMEAR 1 (was) | plays (SIZE 0 → 1) | P95 at 0.95 Hz | P95 at 2 Hz |
|---|---|---|---|---|
| QUAD | 0.15 % of the round trip (0.18 %) | 0.51–1.45 ms | 5.5–8.2 | 8.4–13.3 |
| ROOM | 1.65 ms, cap 1.4 % (1.0 ms, 0.72 %) | 1.12–1.65 ms | 7.1–9.2 | 11.1–14.0 |
| HALL | 0.6 ms, cap 0.11 % (2.0 ms, 0.36 %) | 0.13–0.60 ms | 4.1–6.4 | 9.6–13.9 |
| BLOOM | 0.66 ms, cap 0.12 % (2.0 ms, 0.24 %) | 0.35–0.66 ms | 5.7–8.2 | 9.0–14.2 |
| GATE | 0.85 ms (1.0 ms) | 0.85 ms | 6.9 | 14.0 |
| REVERSE | 2.4 ms (1.0 ms) | 2.4 ms | 5.7–7.3 | 9.8–14.0 |

   At the default RATE the probes put 7–35 % of their power more than
   1.5 Hz off their peaks (−11.4 to −4.5 dB), against under 1 % at
   SMEAR 0 and 1–20 % before at SMEAR 1 (HALL's the most).
5. **SMEAR 0 is untouched.** It is still the MidiVerb II preset bit for
   bit, at any RATE; every SMEAR 0 golden holds. Goldens with SMEAR on
   (`reverb_ring_smear`, `rev_hall_plate`, QUAD's walking sweeps) and the
   REV pages' screens are re-recorded.

## Alternatives considered
- **RATE to 3–4 Hz (the owner's example).** At the same 15-cent top the
  default (0.9–1.1 Hz) would play 4.5–7 cents, GATE's and REVERSE's
  under 5. A random wander faster than 2 Hz reads as warble, not chorus.
- **Shrink the depth as RATE rises**, to hold the pitch flat across
  RATE. It would let the default reach 9–10 cents under a 3–4 Hz top, but
  it is a law per TYPE (loops and single passes scale differently) for
  about 2 cents; not worth it.
- **Raise the depths and keep RATE.** At 0.13–0.45 Hz a deeper wander is
  a slow detune, not motion: HALL already played 7.5 cents at its default
  and still read as still.
- **Keep the worst-case bound and only raise RATE.** Under ADR 0085's
  bound HALL plays 22 cents at 1.5 Hz: the bound never held what plays.
- **A start knob with SMEAR on.** ADR 0095 makes each TYPE's start its
  preset exactly; SMEAR starts at 0.

## Consequences
- SMEAR moves at the default RATE on every TYPE and never passes 15
  cents (P95) at any SIZE or RATE; HALL and GATE play shallower at their
  tops than before, ROOM and REVERSE deeper everywhere.
- HALL's plate zone smears shallowly (0.13 ms at its 120 ms loop, 4 cents
  at the default RATE): its loop is short, so the same cents need less.
- No cost: SMEAR's per-sample work is its moving pairs, the same at any
  depth, and a faster RATE draws a target at most four times a second.
  RevBill and FxBus COST are unchanged.
- `reverb_smear_depth_test` holds the bound and the floor (P95 ≥ 5 cents
  at the default RATE, ≤ 15 at the top, spread ≥ −10 dB) on the four
  probes over 60 s, and RATE's deafness at SMEAR 0. Every TYPE failed it
  before. The worst-case tests keep their structural checks (reads within
  [M − 2D, M], never under 2) at the new caps.

## Sources
- Ledger (private notes) (the investigation) and
  `reverb-smear/v2/` (per TYPE, a held chord at SMEAR 0, SMEAR 1 at the
  default RATE and SMEAR 1 at RATE's top).
- Reverb TYPEs spec (`docs/superpowers/specs/2026-10-04-reverb-types-design.md`)
  § SMEAR, § Pitch bounds.
- Lexicon 480L manual (Random Hall: random delay modulation as chorus).
