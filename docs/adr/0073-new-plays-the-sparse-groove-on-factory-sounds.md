# 0073. NEW plays the sparse groove on factory Sounds

- **Status:** Proposed (accepted at the ORBIT plan's ship flash)
- **Deciders:** owner (ORBIT UAT, 2026-10-03), firmware

## Context
Task 11 tuned ORBIT's init scene on NEW's Parts, which were all INIT Algo:
a sine. At UAT the owner heard it, then the same scene on factory Sounds,
then four variants of the scene. INIT made every ring a sine; the first
scene had the bass on 3rds and 5ths (it hit the chord's root on 9 of 33
notes), and in V1 to V3 the pad sat about 10 dB under the e-piano. The
owner picked V4, "sparse groove", on factory Sounds.

ADR 0043 describes NEW's template as "Parts on INIT Algo". This changes
that, and nothing else in 0043.

## Decision
- **`Scene::init()` is V4:** A minor, BARS 4, I–VI–VII–I a bar each;
  every ring CHANCE 100, MUTATE 0, ACCENT 0, RANGE 0.
  - Ring 1, Part 1: two steps, both hits, walking up A1 A1 F2 F2 G2 G2 A2 A2
    snapped to the chord, GATE 90: the bass is each chord's root.
  - Ring 2, Part 2: 3 of 8, PINGPONG over A4 C5 E5 snapped to the chord,
    GATE 70, VEL 80.
  - Rings 3 and 4, both Part 3: one held note a bar (E4 and A3 snapped to
    the chord), GATE 100, VEL 90: the pad.
  - Ring 5, Part 5: muted, to bring in.
- **NEW's Parts 1–5 load TX BASS, TX EPIANO, MORPH PAD, TX BELL and SAW
  LEAD from their pool slots** (`NEW_PARTS`), with `Origin::Slot`. Part 6
  stays INIT Algo. `TemplateCrc` is taken after the load, so NEW still
  reads `Pristine` and every Part reads clean. `encode_new_project` writes
  the same Parts (`new_project_part`).
- **The QA gate follows:** its loudness reference is a held C4 triad on
  TX EPIANO, the scene's loudest voice (−21.2 LUFS; INIT Algo's, −16.9,
  sits 6.7 dB over the scene). The steal check and the ORBIT parameter
  sweep stay on INIT Algo (`OrbitRig::on_init`), the Sound their
  thresholds were set by: they judge ORBIT's timing, and TX EPIANO's tine
  near 18 kHz makes an off two blocks into its attack read as a click.

## Alternatives considered
- **Keep NEW on INIT Algo and the scene on its rings.** What the owner
  rejected at UAT: every ring a sine.
- **Store the factory Sounds on the Parts with `Origin::Init`.** INIT
  origin means "an INIT Sound": the Parts would read edited.
- **The loudness reference as INIT Algo's chord with a wider bound.** The
  bound would then mean nothing; a factory Sound's held chord is what the
  scene plays.

## Consequences
- A NEW project sounds like an instrument, and its Parts' names say what
  they play.
- CLEAR on a Part goes to INIT, not to its NEW Sound; RELOAD FROM PROJ goes
  to the slot.
- NEW's slots 1, 2, 4, 5 and 7 are in use, so a pool CLEAR of one asks for
  its Part first.
- Tests whose subject is an INIT Part load INIT on it first
  (`screen::init_part`, `OrbitRig::on_init`).

## Sources
- `.superpowers/sdd/2026-09-30-orbit/progress.md`: the UAT variants and
  the owner's pick.
- `docs/superpowers/plans/2026-09-30-orbit.md` § Measured.
- ADR 0043 (projects own their sounds), ADR 0063 (levels at the factory
  median).
