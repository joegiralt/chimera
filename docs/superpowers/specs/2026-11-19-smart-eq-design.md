# SMART EQ: unmask Parts by role (#396)

Date: 2026-11-19. Branch `smart-eq` from origin/0.4 (b136ef37). ADR 0296.

## The owner's design (approved 2026-11-19)

- **Roles.** Each Part has a role, FRONT, MID or BACK, in the project's mix
  (as Part EQ, ADR 0284), never the Sound's. An old project loads MID.
- **Master.** One SMART EQ switch (ON/OFF) and one AMOUNT on the master. An
  old project loads OFF.
- **Listening.** SMART EQ listens to the six Part buses in bands, at block rate.
- **Rule.** Per band: when a higher-priority Part is loud and a lower-priority
  one is active in the same band, the lower one is cut by up to a few dB,
  scaled by AMOUNT. Gains move slowly (slow attack and release), never
  heard as ducking or pumping, never zippered (filter-never-steppy).
- **Where it acts.** Each Part's EQ gains four dynamic cut bands at fixed
  places, LOW, LOW MID, HIGH MID and HIGH, after its LO CUT and TILT. Only
  SMART EQ moves them. They are the Part EQ's (one EQ a Part, never a
  second hidden one), and the Part EQ page shows their live cut.
- **Off is free.** OFF, or AMOUNT 0, is bit for bit today's output once
  the cuts have released (immediately for a project that never had it on).
- **Budget.** About 5 % CPU; a few KB of ITCM; the code in bank 2 behind a
  door; bank 1 only a measured stub.

## Engineering calls

### 1. Listen through the cut bands themselves (4 bands, not 8)

Each cut band is a fixed TPT state-variable band-pass (Zavalishin, *The Art
of VA Filter Design*, ch. 4) on the Part's post-TILT bus. Its unity-peak
band-pass output `bp` is both

- what the cut subtracts: `y = x + (G − 1) · bp`, a bell of gain `G` at the
  centre (the "parallel band-pass" bell), and
- what SMART EQ listens to: the band's level is the mean square of `bp`.

So the level SMART EQ judges is exactly the energy the cut would act on,
in the cut's own shape, and the analysis costs four squares a sample on
top of filters the cut needs anyway.

The owner's sketch said "about 8 bands, a decimated filterbank or a small
FFT". Measured against these (ADR 0296, Measurements):

- **A separate 8-band filterbank** costs eight more filters a Part a sample
  (decimating the four low bands saves about half), and its eight answers
  must still be folded into four decisions, because there are four places
  a cut can be. Finer listening cannot make finer cuts.
- **A small FFT** cannot resolve LOW: a 64-point frame at 48 kHz has 750 Hz
  bins. A 1,024-point frame resolves 47 Hz but needs 1,024 samples a Part
  buffered (24 KB for six, against "a few KB") and adds a frame of delay
  to the decision.

The bands: LOW 100 Hz, LOW MID 400 Hz, HIGH MID 1.6 kHz, HIGH 6.4 kHz, two
octaves apart, each Q 2/3 (a two-octave bandwidth), so neighbours cross at
−3 dB and the four cover 50 Hz–12.8 kHz. In parallel, not in series: every
band hears the same pre-cut signal, so the listening never hears its own
cuts (feed-forward, no loop to go unstable or hunt).

### 2. The rule

Per block, per band `b`, for each Part `p` with voices on its bus:

- `L(p, b)`: the band's level, dBFS, the block's mean square of `bp`
  followed by a one-pole on power (τ 60 ms), so one kick's hit is not a
  verdict.
- `active(L) = clamp((L − (−60)) / 24, 0, 1)`: silent below −60 dBFS, fully
  active from −36 dBFS.
- `threat(p, q) = clamp((L(p) − L(q) + 12) / 12, 0, 1)`: how much `p` can
  mask `q` there: none when `p` sits 12 dB or more under `q`, full at
  level with it or above. This is Perez-Gonzalez and Reiss's (DAFx-08)
  "attenuate the others in proportion to their spectral relation to the
  enhanced channel", kept to a level difference per band.
- `depth(q, p)`: how far `q` outranks `p`. MID under FRONT and BACK under
  MID: 2/3. BACK under FRONT: 1. Same role or `p` higher: 0.
- `target(p, b) = AMOUNT · 6 dB · max over q of depth · active(q) ·
  active(p) · threat(p, q)`.

So the deepest cut is 6 dB (BACK under FRONT at full AMOUNT), 4 dB one rank
apart; AMOUNT 64 (the default when switched on) halves those. The max over
`q`, not the sum, keeps it bounded however many Parts outrank `p`.

FRONT is never cut. Parts of one role never cut each other: sonible's
smart:EQ 4 balances members of one layer against each other; we leave
that out so a role is a promise (two FRONT Parts stay as mixed).

### 3. Time

- The cut follows its target in dB at block rate: deeper with τ 0.5 s
  (attack), shallower with τ 2 s (release). A kick at 120 BPM hits every
  0.5 s, so the cut it causes settles near its average instead of
  breathing with each hit. Pestana and Reiss (DAFx-14) found the slower
  of their two cross-adaptive time constants rated clearly better.
- The block's end gain `G = 10^(−cut/20)` per band is lerped across the
  block from the last block's, sample by sample: no step. At τ 0.5 s the
  steepest change is 6 dB × 64 / (0.5 s × 48 kHz) per block, 0.000 25 dB a
  sample (test bound: 0.001 dB a sample).
- A cut under 0.01 dB lands on 0; its last block lerps to unity.

### 4. Off is free

Bank 1's stub (`Instrument::part_eq`) already skips the door when every
Part's EQ is flat and settled. It now also needs SMART EQ quiet: OFF or
AMOUNT 0 in the snapshot and every cut released (`PartEqs::any_live`
covers the smart state). Within the door:

- A Part whose cuts are unity at both ends of the block is never written:
  a FRONT Part, or any Part with nothing to cut, passes bit for bit while
  SMART EQ listens.
- OFF or AMOUNT 0: targets are 0, the cuts release (τ 2 s), then SMART EQ
  rests and the stub skips it again. Nothing snaps.
- The rest state is all-zero bytes (ITCM's boot zeroing), as Part EQ's.

### 5. Where it lives

- Settings: `Role` in `PartParams::role` (the struct's padding: 20 B
  stays 20 B; Part block id 17); `SmartEq` (ON bit and AMOUNT's 7-bit code,
  one byte) in `CompParams::smart` (padding: 32 B stays 32 B; Comp block
  ids 8 SMART EQ and 9 AMOUNT). The audio's snapshots do not grow.
- State: `PartEqs` gains `SmartState` (each Part's four bands' filter
  memory on its bus and side, level, cut and last gain), in ITCM with the
  rest of the EQ (`PART_EQS`).
- Code: `part_eq::dsp::smart`, inside the Part EQ's S4 group and run by
  the same door entry (`EqDoor`), after LO CUT and TILT. No new door slot
  (tester-04's PRT meters take 0x08180420).
- UI: `UiDoor::smart_eq`, one entry, paints both views.

### 6. UI

- **ROLE** is cell C of MIXER › PRT › EQ, beside TILT and LO CUT. PRT's own
  page is full (CH, MODE, OUT, LEVEL, PAN, BEND); EQ is PRT's sub-page and
  the role only matters to the EQ. The owner said "the mixer's Part page";
  this is its EQ sub-page (open question 1).
- **PRT › EQ's band** shows what SMART EQ is doing to the Part now: four
  bars, LOW · LOW MID · HIGH MID · HIGH, each drawn down from the top by
  its live cut, the deepest's dB written under it; nothing drawn while
  SMART EQ is off and rested.
- **MST › SMART EQ** (page id 120, MST's second sub-page after LVL): cell
  A SMART EQ (OFF/ON), cell B AMOUNT (0–127). Its band is the coarse view:
  each Part's four cuts as a small column of bars under its number and
  role letter (F, M, B), the edited Part lit.
- AMOUNT is CC-mappable (a master knob); SMART EQ and ROLE are switches,
  the hand's (ADR 0217's rule).
- The cells use full words: SMART EQ, AMOUNT, ROLE; FRONT, MID, BACK.

### 7. What we took from sonible smart:EQ 4, and what we did not

Read: the smart:EQ 4 manual's group pages (Group Section, Group View,
"Create hierarchy with layers"). UX precedent only; no text, assets or
method copied.

Taken:
- Three layers named Front, Middle and Back as the whole hierarchy; Front
  is kept clear, Back yields most. Our names: FRONT, MID, BACK.
- One group switch for A/B and one impact amount: our SMART EQ and AMOUNT.
- A wider rank gap yields more (Front over Back deeper than one rank).
- A view of every member at once beside each member's own EQ: our MST ›
  SMART EQ band and PRT › EQ's live bars.

Not taken:
- **Learning.** smart:EQ 4 learns for about ten seconds, then holds a
  static curve. A synth's Parts change by the bar (a pattern change, a
  filter sweep), and there is no learn button on the panel; ours listens
  all the time and moves slowly.
- **Free curves and profiles.** smart:EQ 4 computes a full EQ curve per
  track from instrument profiles. Ours moves four fixed bands only, by up
  to 6 dB: what it does is always readable on four bars.
- **Balancing within a layer.** See § 2: a role is a promise.
- **Groups and remote control of other instances.** One instrument, six
  Parts: every Part is in the one group.

## Research read

- E. Perez Gonzalez, J. D. Reiss, "Improved control for selective
  minimization of masking using inter-channel dependancy effects", DAFx-08
  (read in full): a user-chosen channel is unmasked by lowering the others
  in proportion to their spectral relation to it, measured as amplitude
  overlap per band; no psychoacoustic model. Our rule's shape (priority,
  proportion, overlap per band) is this; it attenuates whole channels by
  filter-bank class, we cut per band.
- P. D. Pestana, J. D. Reiss, "A cross-adaptive dynamic spectral panning
  technique", DAFx-14 (read in full): cross-adaptive unmasking by bin;
  "slow variations through time are shown to produce better results".
- S. Hafezi, J. D. Reiss, "Autonomous multitrack equalization based on
  masking reduction", JAES 63(5), 2015: abstract only (paywalled). Real-time
  and offline masking-reduction EQ, a simplified masking measure from
  mixing practice; the semi-autonomous one-knob version came close to a
  professional mix. Supports one AMOUNT knob.
- E. Perez Gonzalez, J. D. Reiss, "Automatic equalization of multichannel
  audio using cross-adaptive methods", AES 127, 2009: abstract only.
- D. Ronan et al., "Automatic minimisation of masking in multitrack audio
  using subgroups" (arXiv 1803.09960): withdrawn by its author; not used.
- The DAFx reading list (private notes) has
  no masking or cross-adaptive EQ entry.

Clean room: no code read or copied from any implementation.

## Tests (TDD)

- OFF, and ON at AMOUNT 0, is bit for bit the render without the feature,
  on multi-Part material.
- A FRONT bass and a MID pad clashing about 100 Hz: the pad's LOW cut
  deepens, its HIGH bands do not, the bass is untouched.
- No gain moves more than 0.001 dB in a sample, on and off, AMOUNT swept.
- No subnormals in the state after silence follows sound.
- ROLE, SMART EQ and AMOUNT save and load with the project; an old project
  loads MID and OFF.

## Open questions for the owner

1. ROLE on PRT › EQ (cell C) rather than PRT itself, which is full.
2. Listening in the four cut bands rather than about 8 (§ 1).
