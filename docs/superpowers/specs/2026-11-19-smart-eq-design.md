# SMART EQ: unmask Parts by role (#396)

Date: 2026-11-19. Branch `smart-eq` from origin/0.4 (b136ef37). ADR 0296
records each decision as built, with its measurements; this is the design
and its reasons.

## The owner's design (approved 2026-11-19)

- **Roles.** Each Part has a role, FRONT, MID or BACK, in the project's mix
  (as Part EQ, ADR 0284), never the Sound's. An old project loads MID.
- **Master.** One SMART EQ switch (ON/OFF) and one AMOUNT on the master. An
  old project loads OFF.
- **Listening.** SMART EQ listens to the six Part buses in bands, at block
  rate.
- **Rule.** Per band: when a higher-priority Part is loud and a
  lower-priority one is active in the same band, the lower one is cut.
  Gains move slowly and are never zippered (filter-never-steppy).
- **Where it acts.** Each Part's EQ gains four dynamic cut bands at fixed
  places, LOW, LOW MID, HIGH MID and HIGH, after its LO CUT and TILT. Only
  SMART EQ moves them. One EQ a Part; the Part EQ page shows their live
  cut.
- **Off is free.** OFF, or AMOUNT 0, is bit for bit today's output.
- **Budget.** About 5 % CPU asked; a few KB of ITCM; the code in bank 2
  behind a door; bank 1 only a measured stub. As built: 4.4 % of rev V
  for six Parts at worst and all of it billed (below).

## The owner's later decisions (2026-11-19)

1. **Mud and tails.** "Still a little bit muddy", MODAL's long tails the
   worst: a tail counts as sustained energy (levels rise in 50 ms, fall
   over 0.5 s); in LOW and LOW MID a lower Part yields fully to 12 dB
   under the higher one and some to 30 dB under; its cut follows the tail
   down, never ahead of it.
2. **The kick's dip.** When a higher-role DRUM Part's kick hits, lower
   members' LOW takes a quick dip (3 ms in, 100 ms out), keyed by the
   kit's own LOW low-pass so its snare does not key it. In LOW a kit
   presses only this way. Kept so for now (the steady LOW carve under a
   kit gone), for the owner's ear.
3. **AMOUNT goes past good.** "Deep is like 75 % of the slider", and the
   top keeps going "to the point where it sounds like garbage", so there
   is always more to reach for. AMOUNT is a curve through one typed table
   (`dsp::smart::KNOTS`): 64 polite (4 dB one rank, 6 two, everywhere),
   95 DEEP (LOW and LOW MID 8 and 12, the kick 10 and 15), 127 OVER,
   provisional (14 and 21, the kick 18 and 27, HIGH MID and HIGH 6 and
   9). **"Never audible as ducking or pumping" now holds only for the
   lower part of the range**, to about 64: the owner's decision.
4. **The reverb has a role of its own.** Its return is a member, BACK by
   default, settable; it yields and is dipped by the kick like a Part,
   and makes nothing yield.

## Engineering calls

### 1. Listen through the cut bands themselves (4 bands, not 8)

Each cut band is a fixed band-pass on the Part's post-TILT bus. Its
unity-peak output `bp` is both what the cut subtracts (`y = x + d · bp`)
and what SMART EQ listens to (its mean square). So the level judged is the
energy the cut would act on, in the cut's own shape.

The owner's sketch said "about 8 bands, a decimated filterbank or a small
FFT":

- **A separate 8-band filterbank** costs eight more filters a Part, and
  its eight answers must still be folded into four decisions.
- **A small FFT** cannot resolve LOW (a 64-point frame has 750 Hz bins); a
  1,024-point one needs 24 KB of frames and a frame's delay.

The bands: LOW 100 Hz, LOW MID 400 Hz, HIGH MID 1.6 kHz, HIGH 6.4 kHz, two
octaves apart, Q 2/3, in parallel: every band hears the same pre-cut
signal, so the listening never hears its own cuts (feed-forward, no loop).

### 2. The cost, after the review

The first build ran every Part's four bands as TPT state-variable filters
at the full rate, with the rules for every band every block: about 9 % of
a block for six Parts, unbilled. Now:

- LOW and LOW MID are Chamberlin state-variable filters (three
  multiply-adds), HIGH MID and HIGH the same response as a direct form
  (four): 68 cycles a sample a cut Part, from 109.
- A Part only listened to runs the bands decimated (LOW and LOW MID at a
  quarter of the rate, the others at half): 21 cycles.
- Only the bus is listened to; a pad's side takes only the pairs that cut.
- The slow rule runs one band every other block, the kick's the blocks
  between; cuts are stepped only where they move; the rate's coefficients
  are made once.
- Rejected: half-rate cut bands (images 40 dB under the source near
  20 kHz); a broadband gain on the side (does not null).
- **Billed** on each Part's first voice and beside the bus for the
  return, as ADR 0124 and 0262 bill fixed costs; the Part EQ (ADR 0284)
  with it. ADR 0296 has the worst case against 0262 and 0273.

### 3. The rule

`want = cap(AMOUNT, band) · max over q of depth(q, p) · active(q) · own(q)
· active(p) · own(p) · threat(p, q)`:

- `active`: silent below −66 dBFS, fully active from −42.
- `own`: how much of the band is the member's own sound, not its other
  bands' energy through this band's skirt (a pure 100 Hz bass "plays" in
  HIGH MID 20.6 dB under its LOW).
- `threat`: how much `p` can mask `q` there, by how far under `q` it sits
  (Perez-Gonzalez and Reiss, DAFx-08: attenuate the others in proportion
  to their spectral relation to the enhanced channel).
- `depth`: 2/3 one rank apart, 1 two apart. FRONT is never cut; one role
  never cuts itself; the max over `q`, not the sum.

### 4. Stacked bands

The bands overlap, so their dips are unmixed to sum to the cut asked, and
the pages read what the bands really cut at each centre. Parallel
band-passes stop near 12 dB: deeper, a neighbour's quarter-turn part holds
a centre up (two adjacent bands asked 21 dB reach 10 to 18). The pages
say so. If OVER must truly reach 21 dB, the path is cascaded bells with a
graphic EQ's interaction solve (Välimäki and Liski), about 20 cycles a
sample more a cut Part: open question 1.

### 5. Time

- Slow cuts: τ 0.5 s deeper, 2 s shallower (Pestana and Reiss, DAFx-14:
  the slower of their two time constants rated clearly better). A kick at
  120 BPM hits every 0.5 s: the slow cuts sit near their average.
- Every dip is lerped across its block, sample by sample: at polite
  depths under 0.001 dB a sample for the slow cuts, 0.05 for the kick's.
- Dense kicks (onsets to 100 ms apart, fading out by 220 ms) hold one
  steady dip; sparse ones each dip and let go.
- The tail counts (owner's decision, 2026-11-20): a long 808 tail or a
  held sub on a higher-role DRUM Part keeps the lower Parts' LOW dipped
  for its whole length.

### 6. Where it lives

- Settings in their structs' padding (no snapshot grows); state in ITCM
  with the EQ; the DSP in the Part EQ's S4 group behind its one door
  entry; the return's pass asked through the same entry later in the
  block.
- ROLE is cell C of MIXER › PRT › EQ; SMART EQ, AMOUNT and REV ROLE are
  cells D, E and F of MIXER › MST › LVL (a page of their own overflowed
  D2; the REV pages' cells are all their TYPE's panel's).

## What we took from sonible smart:EQ 4, and what we did not

Read: the smart:EQ 4 manual's group pages (Group Section, Group View,
"Create hierarchy with layers"). UX precedent only; no text, assets or
method copied.

Taken:
- Three layers named Front, Middle and Back as the whole hierarchy; Front
  is kept clear, Back yields most. Our names: FRONT, MID, BACK.
- One group switch for A/B and one impact amount: our SMART EQ and AMOUNT.
- A wider rank gap yields more (Front over Back deeper than one rank).
- A view of every member at once beside each member's own EQ: our MST ›
  LVL band and PRT › EQ's live bars.

Not taken:
- **Learning.** smart:EQ 4 learns for about ten seconds, then holds a
  static curve. A synth's Parts change by the bar (a pattern change, a
  filter sweep), and there is no learn button on the panel; ours listens
  all the time and moves slowly.
- **Free curves and profiles.** smart:EQ 4 computes a full EQ curve per
  track from instrument profiles. Ours moves four fixed bands only: what
  it does is always readable on four bars.
- **Balancing within a layer.** A role is a promise: two FRONT Parts
  stay as mixed.
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
  on three Parts at once and on a Part feeding the reverb.
- A FRONT bass and a MID pad clashing about 100 Hz: the pad's LOW cut
  deepens, its HIGH bands do not, the bass is never written.
- No slow gain moves more than 0.001 dB in a sample at AMOUNT 64, on and
  off; the kick's under 0.05.
- No subnormals in the state after silence follows sound.
- A ringing tail keeps its LOW cut while it rings within reach.
- A sine bass under a 4/4 kick with off-beat snares: a LOW dip on each
  kick, gone by 240 ms, none on the snares, no click above 2 kHz.
- 16ths at 170 BPM hold one dip (under 0.5 dB peak to peak); quarter
  notes at 120 BPM each let go.
- Every combination of the knots' cuts on the real filters, on a fine
  grid: the measured bounds of ADR 0296 § 6, and the readout is what the
  filters do.
- Kick gaps from 60 to 400 ms: no cliff; 32nds at 120 BPM one dip.
- A Part moved to FRONT is billed as cut until its cuts let go.
- AMOUNT's curve passes its knots and only deepens.
- A listened Part's side is never run; a cut Part's side takes the cut.
- The reverb's return yields by its role, takes the kick's dip, makes
  nothing yield, and at rest is not touched.
- The bills: each Part's first voice, the return's reserve, a full pool a
  voice fewer.
- ROLE, SMART EQ, AMOUNT and REV ROLE save and load with the project; an
  old project loads MID, OFF and BACK.
- The tuning: `just smart-eq-ab` and the teaser's vignettes (ADR 0296).

## Next: the delay's return

Designed, not built. The delay returns mono (added to both sides of pair
1), so it would take one bank of bands (59 cycles a sample cut) and a
decimated listen (21): a member like the reverb's return, with its own
role (a `DelayParams` byte), bill and column on MST › LVL. Its REV SEND
taps the delay's return before these bands would act (`FxBus::process`),
so a carved delay would still feed the reverb whole; the reverb's own
role then carves that. Wait for the owner to ask: the reverb's wash is
what clouds a mix; a delay's repeats are usually wanted forward.

## Open questions for the owner

1. OVER and DEEP ask more than parallel bands give cleanly (§ 4): the
   pages read the real cut. Keep, or build cascaded bells for a true
   21 dB?
2. ROLE on PRT › EQ (cell C) rather than PRT itself, which is full; SMART
   EQ, AMOUNT and REV ROLE on MST › LVL rather than a page of their own.
3. Listening in the four cut bands rather than about 8 (§ 1).
4. The kick's dip replaces the steady LOW carve under a kit (kept for
   now, for the owner's ear).
5. The delay's return as a member (above).
