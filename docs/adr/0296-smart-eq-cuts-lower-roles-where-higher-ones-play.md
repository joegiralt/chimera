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
   (`y = x + Σ (G_b − 1) · bp_b`) and what SMART EQ hears: its block mean
   square, followed on power rising with τ 50 ms and falling with τ 0.5 s,
   in dBFS, so a long MODAL tail or a pad's release counts as the
   sustained energy it is (owner, 2026-11-19: "still a little bit muddy",
   MODAL's long tails intrude most). A DRUM pad's side
   takes the same cut through its own filters; the side is not listened to.
3. **The rule**, per block and band, for each written Part `p`:
   `target = AMOUNT · 6 dB · weight(b) · max_q depth(q, p) · active(q) ·
   own(q) · active(p) · own(p) · threat_b(p, q)`, with `active(L) = clamp((L + 66) /
   24, 0, 1)` and `depth` 2/3 one rank apart, 1 two apart (BACK under
   FRONT), 0 otherwise.
   - `threat_b`: how far under `q` the lower Part still masks it. In LOW
     and LOW MID (`Band::muddy`, where the mud lives) fully to 12 dB under
     and some to 30 dB under: a quiet ringing tail still clouds a louder
     Part, so it keeps yielding while it rings and lets go as it fades
     (the cut follows it down at RELEASE's pace). Above the mud, fully at
     `q`'s level and none from 12 dB under.
   - `weight`: 1 in LOW and LOW MID, 3/4 above. `own(q)` is how much
   of the band is `q`'s own sound rather than its other bands' energy
   through this band's skirt: `clamp((L_q,b − max_o (L_q,o + skirt(|o −
   b|))) / 6 dB, 0, 1)`, the skirts exact for Q 2/3 (−8.6, −20.6, −32.6 dB
   one, two, three bands away). Without it a pure 100 Hz bass is "active"
   in HIGH MID (its skirt hears it 20.6 dB down) and cuts a pad's 1.6 kHz
   (the first test caught it, 3.1 dB). FRONT is never cut; one role never
   cuts itself.
4. **The kick's dip (owner decision, 2026-11-19).** In LOW, a higher-role
   DRUM Part (`PartKind::Drum`, from its Sound's engine) presses only
   through a fast dip on its hits; its LOW does not press slowly. LOW
   MID, HIGH MID and HIGH keep the slow rule against every Part.
   - The key is the DRUM Part's own LOW low-pass (the LOW SVF's `v2`, a
     2-pole low-pass at 100 Hz): a snare's 180–330 Hz body barely moves
     it. Its block power rises at once and falls with τ 20 ms; its peak
     falls with τ 0.5 s. `hit = active(peak) · clamp((key − peak + 6) /
     6, 0, 1)`: the kick's attack and the top of its body, not its tail.
   - `kick target = AMOUNT · 6 dB · max_q depth(q, p) · hit(q) ·
     active(p) · own(p) · threat_mud(p, q)` in LOW only.
   - The dip follows its target with τ 3 ms in and τ 100 ms out (owner's
     range 60–150 ms): over a kick's body (50–80 ms) and 95 % back in
     0.3 s, before the next kick at up to 190 BPM in four. No KICK
     RELEASE control: one value serves the tempos the teaser and the drum
     machines play.
   - LOW plays the deeper of its slow cut and the dip; the gain is still
     lerped a sample at a time across each block (a 4 dB dip over two
     blocks is under 0.05 dB a sample).
   - Whole-mix pumping (ducking as an effect) is not this: it stays with
     the DRUM bus work (#13).
5. **Time.** Each cut (`CutDb`, 0..=6 dB) follows its target at block rate,
   τ 0.5 s deeper and τ 2 s shallower; under 0.01 dB it lands on 0. Each
   band's linear gain is lerped across the block from the last block's: at
   most 0.000 25 dB a sample.
6. **Off is free.** A Part whose four gains are unity at both ends of a
   block is not written. With SMART EQ OFF or AMOUNT 0 the cuts release,
   then the state rests and `PartEqs::any_live` is false; bank 1's stub
   (`Instrument::part_eq`) then skips the door as before (it also checks
   `SmartEq::listens`). All-zero bytes are the rest (ITCM's boot zeroing).
7. **Bank 2, behind the Part EQ's door.** `part_eq::dsp::smart` is inside
   the Part EQ's S4 group (`*7part_eq3dsp*`) and runs from `run_parts`, the
   `EqDoor` entry, after LO CUT and TILT. No new door table. `PartsBlock`
   carries the snapshot's `SmartEq`. A closed door rests it with the EQ.
8. **State in ITCM** with the EQ: `PartEqs::smart` (`SmartState`) holds
   each Part's band filters for bus and side, levels, cuts and last gains.
9. **UI.**
   - ROLE: MIXER › PRT › EQ cell C.
   - MIXER › MST › LVL: SMART EQ (D) and AMOUNT (E) under VOL and PAN; its
     band (no band before) draws each Part's four live cuts under its
     number and role letter, the edited Part lit. Not a page of its own: a
     new page id grows `FocusMemory` (a byte per id up to the largest), and
     ids 119 → 120 overflowed D2 by 8 B on bench-algo (measured).
   - PRT › EQ's band draws the Part's four live cuts as bars named LOW,
     LOW MID, HIGH MID, HIGH, with the deepest in dB.
   - Both through one UI door entry (`UiDoor::smart_eq`): bank 2 plans the
     band (`ui::smart_eq::plan_in`: rectangles and short texts) and bank 1
     draws the plan with the display's own `fill_rect` and `text_center`
     (`ui::smart_view::paint`). Painting in bank 2 on ORBIT's `Ink` built
     the draw helpers again for `Ink` in bank 1 on every set without ORBIT
     (default +3,836 B, measured). The audio publishes the cuts once a
     block (`part_eq::smart::SMART_VIEW`, six relaxed words). A closed door
     draws nothing.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 b136ef37 against this
change, each set built in both trees. Each budget line is raised by exactly
its set's measured change, never ADR 0198's slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---:|---:|
| default | +1,124 | +10,368 |
| orbit | +1,156 | +10,368 |
| bank2-gate | +1,132 | +10,368 |
| orbit,usb-config-writer | +1,156 | |
| fault-inject | +1,100 | |
| bench-algo | +1,188 | |
| bench-algo,master-tape | +1,244 | |
| bench-routing, bench-memory, bench-stress | +1,188 | |
| bench-orbit | +1,236 | |
| bench-tape | +1,428 | |
| filter-probes | +1,100 | |

- Bank 1 on ORBIT (900,204 B, 0.4's line 899,048): `COMP_SPECS` +144 B and
  `PART_SPECS` +72 B (three specs), the ROLE and SMART EQ/AMOUNT arms of
  `get`/`write`/`enum_*` (`CompParams::write` +116 B; code, an exception
  to ADR 0198 as ADR 0284's TILT/LO CUT arms are: the codec, load, save,
  MIDI and the editor reach a setting only through them), the names
  (FRONT/MID/BACK, OFF/ON), the renderer's arm and `smart_shown` (148 B)
  with the plan's painter, and the stub's `SmartEq::listens` test.
  - First draft: the band painted in bank 2 on ORBIT's `Ink` cost default
    +3,836 B (the draw helpers built again for `Ink`); the plan brought it
    to +1,124 B.
  - A `core::fmt::Write` impl in bank 2 left its vtable in bank 1's
    rodata, pointing into bank 2: `rust-objcopy -R .bank2` refused the
    split. The plan pushes its bytes instead.
  - First draft: MST › SMART EQ, page id 120, grew `FocusMemory` and
    overflowed D2 by 8 B on bench-algo. SMART EQ and AMOUNT moved to MST ›
    LVL.
- Bank 2 on ORBIT: S4–S5 +10,368 B (`run_parts` with SMART EQ's three
  inlined band loops, `ui::smart_eq`); S0–S3 unchanged at 8,096 B free
  (0.4's own, under its 8 KB warning before this change); total free
  489,216 B.
- RAM: `PartEqs` (`PART_EQS`, ITCM) +744 B: 124 B a Part (four bands'
  filters on bus and side, levels, cuts, gains, the kick's key, peak and
  dip); ITCM 43,280 → 44,024 B
  used on every set. `SMART_VIEW` 24 B in AXI. `PartParams` (20 B) and
  `CompParams` (32 B) unchanged: the snapshots, the project and D2 do not
  grow.
- CPU, host (x86, release, `smart_eq_cost`, a loaded machine): six Parts,
  four cutting, about 2.5–5 µs a 1.33 ms block (0.2–0.4 %).
- CPU, unit, static (the ORBIT ELF's `run_parts`, `tools/loop_count.py`'s
  1.606 cycles an instruction): the cutting pair loop is 122 instructions
  for 4 samples, 49 cycles a sample a pair, so about 98 cycles a sample a
  Part cutting; the listening-only pair loop 67 instructions for 2
  samples, about 108 cycles a sample a Part; the block's rule and slews
  about 25 cycles a sample spread. So about 1 % of the CPU a playing Part:
  four Parts (the teaser's) about 4.2 %, six about 6.5 %. Interleaving
  all four bands in one loop spilled to the stack: 149 instructions for 2
  samples, about 120 cycles a sample a Part. Not yet read on the unit (no
  flash in this change); a ROUTING row sets one Part, and SMART EQ needs
  several and the master.
- Sound: every golden bit for bit (`just check`); OFF, and ON at AMOUNT 0,
  render bit for bit on three Parts at once (`smart_eq_render_test`); with
  every Part one role it listens and writes nothing.

## Tuning
Measured before the kick's dip (Decision 4); the dip's own numbers, and
the overlaps with it, follow in the next section.

The owner, on the pre-SMART EQ teaser renders: "still a little bit
muddy", MODAL's long tails intruding most. Measured on the 250 Hz – 2 kHz
overlap (`smart_eq_ab::overlap`: over Hann frames of 4,096, each bin's
smaller power of every pair where one role outranks the other, the
Parts' buses after their EQ and SMART EQ at their LEVEL), SMART EQ off
against AMOUNT 64 and 127.

**MODAL pad (MID, MODAL's INIT) over a bass (FRONT, ALGO's INIT)**, `just
smart-eq-ab`: overlap −0.7 dB at 64, −1.5 dB at 127; the pad's own
250 Hz – 2 kHz −1.1 / −1.9 dB, the bass untouched (with the kick's dip
and `own(p)`, the final rule: −0.6 / −1.3 dB, the pad −1.0 / −1.9 dB). The pad's LOW MID cut
reaches its cap (2.0 / 4.0 dB) on the strike and holds through the ring:
1.8–2.0 dB (64) and 3.7–4.0 dB (127) on each chord's last beat.

**The six teaser vignettes** (teaser-04, the kits FRONT, the bass MID,
the lead or chords BACK; NYLON in breaks is MODAL), the overlap and each
lower Part's own 250 Hz – 2 kHz:

| Vignette | AMOUNT 64: overlap · bass · top | AMOUNT 127 |
|---|---|---|
| electro | −1.2 · −1.3 · −2.6 dB | −2.3 · −2.4 · −5.0 dB |
| breaks (MODAL NYLON) | −1.4 · −1.1 · −2.0 dB | −2.7 · −2.1 · −3.8 dB |
| glitch | −0.9 · −1.3 · −1.9 dB | −1.7 · −2.6 · −3.8 dB |
| dub | −0.3 · −0.5 · −0.6 dB | −0.6 · −0.9 · −1.1 dB |
| triphop | −1.2 · −1.4 · −1.4 dB | −2.3 · −2.7 · −2.5 dB |
| garage | −0.5 · −1.1 · −1.6 dB | −1.0 · −2.1 · −3.1 dB |

- In breaks at 64 the NYLON chords hold 2.9 dB (mean, of a 3.0 dB cap)
  in LOW and 1.9 dB in LOW MID; the first draft's rule held 2.1 and 2.3
  dB of 3.0 with an overlap of −1.3 dB.
- The kits (FRONT) are never cut. Mean and most cut sit close (breaks'
  chords LOW 2.91 / 3.03 dB): the cuts hold through the bar rather than
  breathing with each hit (attack 0.5 s against beats of 0.4–0.7 s).
- The mix is 0.3–0.8 LUFS quieter at 64 and the limiter does 0.2–0.7 dB
  less: less of the mix sits on top of itself.
- dub's overlap moves least: its sub is under 100 Hz and its chords sit
  above the mud; garage's sub likewise.
- A sub bass as MID under the kick yields its LOW steadily (2.0 dB at 64):
  a sub that should keep its weight belongs in FRONT with the kits. The
  teaser agent's call per vignette.
- The cut is capped (4 dB one rank, 6 dB two, × AMOUNT): that, not the
  detector, now bounds how far the mud drops. A deeper mud cap is the
  owner's call (open question).

## The kick's dip, measured
`just smart-eq-ab`'s second case: a DRUM kit's INIT kick on every beat
(FRONT) over ALGO's INIT bass held at A1 (MID). The kick's transient over
the bass in 40–120 Hz (each onset's next 43 ms, the kick's band energy
over the bass's): −1.7 dB off, +0.8 dB better at AMOUNT 64 and +1.5 dB at
127; the bass's LOW dip 2.0 / 3.9 dB at most; its 250 Hz – 2 kHz −0.07 /
−0.12 dB (its upper harmonics left).

The teaser vignettes, the kick-and-snare kit's transient over the bass in
40–120 Hz (off) and its gain, and the 250 Hz – 2 kHz overlap, at AMOUNT 64
and 127:

| Vignette | Kick over bass, off | 64: kick · overlap | 127: kick · overlap |
|---|---|---|---|
| electro | 12.0 dB | +0.5 · −0.9 dB | +0.8 · −1.6 dB |
| breaks | 6.3 dB | +0.4 · −1.0 dB | +0.8 · −1.9 dB |
| glitch | 9.7 dB | +1.0 · −0.6 dB | +1.7 · −1.1 dB |
| dub | 5.9 dB | +0.9 · −0.1 dB | +1.7 · −0.1 dB |
| triphop | 7.3 dB | +0.5 · −0.7 dB | +0.9 · −1.3 dB |
| garage | 4.2 dB | +0.6 · −0.3 dB | +1.0 · −0.6 dB |

- The dip, not a duck: a sine bass under a 4/4 kick with a snare on the
  off-beats (`smart_eq_dsp_test`) dips more than 3 dB within 30 ms of
  each kick, over half of it by 10 ms, under 30 % of it by 240 ms, and
  the snares do not dip it again; above 2 kHz the dipped bass is within
  1.1 dB of the plain bass's filter residue (−83 dB): no click.
- The overlap drops less than with the slow rule alone (breaks −1.0
  against −1.4 dB at 64): in LOW the kit now presses only on its hits.
  NYLON's mean LOW cut under the kit went 2.9 → 0.8 dB; LOW MID, where
  most of 250 Hz – 2 kHz lives, is as before (1.9 dB). If the owner
  wants both, a steady LOW carve plus the hits' dip, LOW would play the
  slow cut plus the dip (open question).
- `own(p)` joined the rule with the dip: a lower Part masks only with its
  own sound in a band, not a skirt's (a 55 Hz sine bass's LOW MID skirt
  met a snare and took 0.3 dB). The bass's HIGH cuts in the vignettes
  went to 0 with it.

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
- **One threat span in every band, the first draft.** A NYLON chord's
  ring under the breaks vignette's kit fell out of reach within a beat
  and its LOW MID cut eased while it still rang; the owner heard mud.
- **A level followed by peaks (τ 60 ms both ways), the first draft.** A
  tail between strikes read as gone.
- **A KICK RELEASE control.** One value covers the owner's range and the
  tempos played; a control only if a unit test by ear asks for it.
- **Keying the dip from the kit's whole LOW band.** Its 50–200 Hz skirt
  hears a snare's body; LOW's low-pass does not.
- **Summing every higher Part's pressure.** Unbounded with more Parts; the
  max keeps 6 dB the most.
- **A learned, static curve (smart:EQ 4).** A synth's Parts change by the
  bar, and there is no learn button; it listens all the time, slowly.
- **A new S4 door table.** The Part EQ's door already reaches the same
  buses at the same point; one EQ a Part (ADR 0284 § 9).
- **ROLE on PRT itself.** Its six cells are full; EQ is PRT's sub-page and
  the role only drives the EQ.
- **MST › SMART EQ, a page of its own.** Clearer, but a new page id grows
  `FocusMemory` past D2 on bench-algo (8 B, measured).
- **Painting in bank 2 on ORBIT's `Ink`.** Builds the draw helpers a
  second time in bank 1 wherever ORBIT is off (+3.8 KB on default).
- **AMOUNT as a float.** 4 B more in `CompParams` (32 → 36), × 3 snapshots;
  a 7-bit code in the padding costs nothing.

## Consequences
- Every Part has a ROLE and the master SMART EQ and AMOUNT, saved with the
  project; old projects load MID and OFF and play bit for bit.
- With SMART EQ on, cost scales with the Parts that play, not voices.
- The four bands are fixed: no user control of their place or width.
- A docs page (annotated screenshots) is owed for PRT › EQ and MST › LVL.

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
