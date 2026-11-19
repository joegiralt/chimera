# 0296. SMART EQ cuts a lower-role Part's four fixed bands where a higher-role Part plays

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-11-19, #396), firmware

## Context
#396: the owner wants Parts to stop masking each other without manual EQ,
after sonible smart:EQ 4's groups. ADR 0284 gave every Part one EQ (LO CUT,
TILT) at the end of its channel and said SMART EQ would add its cuts there.
The owner's design (2026-11-19): a role per Part (FRONT, MID, BACK) in the
project's mix; SMART EQ ON/OFF and AMOUNT on the master; listening to the
six Part buses in bands at block rate; per band, a lower-role Part active
where a higher-role Part is loud is cut by up to a few dB, slowly, never
pumping, never zippered; four dynamic cut bands at fixed places after TILT,
moved only by SMART EQ and shown live on the Part EQ page; OFF or AMOUNT 0
bit for bit; about 5 % CPU, a few KB ITCM, bank 2 behind a door.

Bank 1 is frozen (ADR 0198); ORBIT's has about 18 KB left. The door's own
table is full (ADR 0283); the S4 tables after Part EQ's (0x08180410) are
being taken (tester-04's PRT meters, 0x08180420, ADR 0293). The audio's
snapshots live in ITCM (ADR 0286).

## Decision
1. **Settings.**
   - `part_eq::smart::Role { Front, Mid, Back }` in `PartParams::role`, Part
     block id 17 `ROLE`, codes FRONT 0, MID 1, BACK 2; `Mid` is the default
     and what a project without it loads. It sits in `PartParams`' padding
     (20 B before and after).
   - `part_eq::smart::SmartEq(u8)` in `CompParams::smart`: ON in bit 7,
     AMOUNT's code 0..=127 in bits 0–6, every byte a setting; `SmartEq::OFF`
     (OFF, AMOUNT 64) the default and an old project's. Comp block ids 8
     `SMART_EQ` (OFF/ON) and 9 `AMOUNT` (stepped 0–127). In `CompParams`'
     padding (32 B before and after).
   - AMOUNT is CC-mappable; SMART EQ and ROLE are switches, the hand's
     (ADR 0217).
2. **Four fixed bands a Part, listened through and cut through.** LOW
   100 Hz, LOW MID 400 Hz, HIGH MID 1.6 kHz, HIGH 6.4 kHz (`Band`), each a
   TPT state-variable band-pass, Q 2/3, on the Part's bus after LO CUT and
   TILT, in parallel. A band's unity-peak band-pass `bp` is what it cuts
   (`y = x + Σ (G_b − 1) · bp_b`) and what SMART EQ hears (its block mean
   square, followed on power with τ 60 ms, in dBFS). A DRUM pad's side
   takes the same cut through its own filters; the side is not listened to.
3. **The rule**, per block and band, for each written Part `p`:
   `target = AMOUNT · 6 dB · max_q depth(q, p) · active(q) · active(p) ·
   threat(p, q)`, with `active(L) = clamp((L + 60) / 24, 0, 1)`,
   `threat = clamp((L_p − L_q + 12) / 12, 0, 1)` and `depth` 2/3 one rank
   apart, 1 two apart (BACK under FRONT), 0 otherwise. FRONT is never cut;
   one role never cuts itself.
4. **Time.** Each cut (`CutDb`, 0..=6 dB) follows its target at block rate,
   τ 0.5 s deeper and τ 2 s shallower; under 0.01 dB it lands on 0. Each
   band's linear gain is lerped across the block from the last block's: at
   most 0.000 25 dB a sample.
5. **Off is free.** A Part whose four gains are unity at both ends of a
   block is not written. With SMART EQ OFF or AMOUNT 0 the cuts release,
   then the state rests and `PartEqs::any_live` is false; bank 1's stub
   (`Instrument::part_eq`) then skips the door as before (it also checks
   `SmartEq::listens`). All-zero bytes are the rest (ITCM's boot zeroing).
6. **Bank 2, behind the Part EQ's door.** `part_eq::dsp::smart` is inside
   the Part EQ's S4 group (`*7part_eq3dsp*`) and runs from `run_parts`, the
   `EqDoor` entry, after LO CUT and TILT. No new door table. `PartsBlock`
   carries the snapshot's `SmartEq`. A closed door rests it with the EQ.
7. **State in ITCM** with the EQ: `PartEqs::smart` (`SmartState`) holds
   each Part's band filters for bus and side, levels, cuts and last gains.
8. **UI.**
   - ROLE: MIXER › PRT › EQ cell C.
   - MIXER › MST › SMART EQ (page id 120, after LVL): SMART EQ (A), AMOUNT
     (B); its band draws each Part's four live cuts under its number and
     role letter, the edited Part lit.
   - PRT › EQ's band draws the Part's four live cuts as bars named LOW,
     LOW MID, HIGH MID, HIGH, with the deepest in dB.
   - Both through one UI door entry (`UiDoor::smart_eq`); the audio
     publishes the cuts once a block (`part_eq::smart::SMART_VIEW`, six
     relaxed words). A closed door draws nothing.

## Measurements
Filled in at the end of the branch.

## Alternatives considered
- **A separate 8-band filterbank.** Eight more band filters a Part a
  sample, and eight answers folded into four decisions, since only four
  bands can cut.
- **A small FFT.** 64 points cannot resolve LOW (750 Hz bins); 1,024
  points need 24 KB of frames for six Parts and a frame's delay.
- **Bands in series.** Each band would hear the cuts before it; parallel
  bands hear the same pre-cut signal: feed-forward, no loop.
- **Balancing within a role, as smart:EQ 4's layers.** A role is a
  promise: two FRONT Parts stay as mixed.
- **Summing every higher Part's pressure.** Unbounded with more Parts; the
  max keeps 6 dB the most.
- **A learned, static curve (smart:EQ 4).** A synth's Parts change by the
  bar, and there is no learn button; it listens all the time, slowly.
- **A new S4 door table.** The Part EQ's door already reaches the same
  buses at the same point; one EQ a Part (ADR 0284 § 9).
- **ROLE on PRT itself.** Its six cells are full; EQ is PRT's sub-page and
  the role only drives the EQ.
- **AMOUNT as a float.** 4 B more in `CompParams` (32 → 36), × 3 snapshots;
  a 7-bit code in the padding costs nothing.

## Consequences
- Every Part has a ROLE and the master SMART EQ and AMOUNT, saved with the
  project; old projects load MID and OFF and play bit for bit.
- With SMART EQ on, cost scales with the Parts that play, not voices.
- The four bands are fixed: no user control of their place or width.
- A docs page (annotated screenshots) is owed for PRT › EQ and MST › SMART EQ.

## Sources
- #396; owner's design 2026-11-19; spec
  `docs/superpowers/specs/2026-11-19-smart-eq-design.md`, plan
  `docs/superpowers/plans/2026-11-19-smart-eq.md`.
- Perez Gonzalez and Reiss, DAFx-08 (selective minimization of masking);
  Pestana and Reiss, DAFx-14 (cross-adaptive spectral panning); Hafezi and
  Reiss, JAES 2015 (abstract); Zavalishin, *The Art of VA Filter Design*
  (TPT SVF).
- sonible smart:EQ 4 manual (groups, layers, impact): UX precedent only.
- ADR 0198, 0217, 0251, 0266, 0272, 0283, 0284, 0286, 0293.
