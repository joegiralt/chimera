# 0296. SMART EQ cuts a lower-role Part's four fixed bands where a higher-role Part plays

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-11-19, #396; the kick's dip, AMOUNT's curve and the reverb return's role, 2026-11-19), firmware

## Context
#396: the owner wants Parts to stop masking each other without manual EQ,
after sonible smart:EQ 4's groups. ADR 0284 gave every Part one EQ (LO CUT,
TILT) at the end of its channel and said SMART EQ would add its cuts there.
The owner's design (2026-11-19): a role per Part (FRONT, MID, BACK) in the
project's mix; SMART EQ ON/OFF and AMOUNT on the master; listening to the
Part buses in bands at block rate; per band, a lower-role Part active where
a higher-role Part is loud is cut, slowly; four dynamic cut bands at fixed
places after TILT, moved only by SMART EQ and shown live on the Part EQ
page; OFF or AMOUNT 0 bit for bit; the code in bank 2 behind a door.

Later the same day, after the teaser renders, the owner added:
- "still a little bit muddy", MODAL's long tails intruding most;
- a kick-keyed fast dip in LOW only;
- AMOUNT re-scaled: "Deep is like 75 % of the slider", and the top goes
  on past good "to the point where it sounds like garbage", so there is
  always more to reach for;
- "we should give the reverb its role of its own".

The review (2026-11-19) failed the first build on CPU: unbilled, 7–13 % of
a block on top of ADR 0262's 89 % worst block, under ADR 0273's 95 % net.

Bank 1 is frozen (ADR 0198). The door's own table is full (ADR 0283). The
audio's snapshots live in ITCM (ADR 0286). D2, where the `Instrument` and
the UI live, has no room: a page id more overflowed it by 8 B on
bench-algo.

## Decision
1. **Settings.**
   - `part_eq::smart::Role { Front, Mid, Back }` in `PartParams::role`, Part
     block id 17 `ROLE`; `Mid` is the default and an old project's.
   - `SmartEq(u8)` in `CompParams::smart`: ON in bit 7, AMOUNT 0..=127 in
     bits 0–6; `SmartEq::OFF` (OFF, AMOUNT 64) the default and an old
     project's. Comp block ids 8 `SMART_EQ` and 9 `AMOUNT`.
   - The reverb return's role, `ReverbParams::role`, Reverb id 17 `ROLE`
     (15 and 16 are REV › DUCK's, ADR 0295); `Back` the default and an old
     project's.
   - All three sit in their structs' padding: `PartParams` 20 B,
     `CompParams` 32 B and `ReverbParams` unchanged; the snapshots do not
     grow. AMOUNT is CC-mappable; the roles and the switch are the hand's
     (ADR 0217).
2. **Four fixed bands a member, listened through and cut through.** LOW
   100 Hz, LOW MID 400 Hz, HIGH MID 1.6 kHz, HIGH 6.4 kHz (`Band`), Q 2/3,
   in parallel on the Part's bus after LO CUT and TILT: a band's unity-peak
   band-pass `bp` is what it cuts (`y = x + Σ d_b · bp_b`) and what SMART
   EQ hears (its block mean square, rising τ 50 ms, falling τ 0.5 s, so a
   long MODAL tail counts as the sustained energy it is).
   - LOW and LOW MID are Chamberlin state-variable filters (three
     multiply-adds a sample, quiet in `f32` far under the rate); HIGH MID
     and HIGH the same bilinear band-pass as a transposed direct form II
     (four).
   - **A Part being cut** runs the four at the full rate and is heard
     through them.
   - **A Part only listened to** (FRONT, or nothing to yield to: its dips
     at unity at both ends of the block) is read, never written, through
     the same bands decimated: LOW and LOW MID at a quarter of the rate,
     HIGH MID and HIGH at half (a boxcar, no other filter before it:
     16 to 23 kHz folds onto 1 to 8 kHz only 8 to 12 dB down, so a Part
     that is mostly air reads some of it in HIGH MID and HIGH; a filter
     ahead would cost every listened Part). A change of rate starts the filters empty.
   - **Only the bus is listened to.** A DRUM pad's side (`Sides`, there
     only while a panned pad sounds) takes the pairs of bands that cut,
     and no others, with the bus's dips; a listened Part's side is not
     touched and its side filters rest. A broadband gain on the side
     would not null: the cut is a band's, and left and right must take
     the same one.
3. **The slow rule**, a band every other block in turn, for each member
   `p`: `want = cap(AMOUNT, band) · max_q depth(q, p) · active(q) · own(q)
   · active(p) · own(p) · threat_b(p, q)`.
   - `active(L) = clamp((L + 66 dBFS) / 24, 0, 1)`; `depth` 2/3 one rank
     apart, 1 two apart (BACK under FRONT), 0 otherwise.
   - `own`: how much of a band is a member's own sound, not its other
     bands' energy through this band's skirt (−8.6, −20.6, −32.6 dB one,
     two, three bands away): its excess over the loudest skirt, full at
     6 dB.
   - `threat_b`: in LOW and LOW MID (`Band::muddy`) full to 12 dB under
     `q` and some to 30 dB under (a quiet ringing tail still clouds);
     above, full at `q`'s level and none from 12 dB under.
   - FRONT is never cut; one role never cuts itself.
4. **The kick's dip (owner).** In LOW a higher-role DRUM Part
   (`PartKind::Drum`) presses only through a fast dip on its hits, never
   slowly; the other bands keep the slow rule against every Part.
   - The key is the DRUM Part's own LOW low-pass, so a snare barely keys
     it: its power rises at once and falls with τ 20 ms; a hit is the key
     within 6 dB of its recent peak (τ 0.5 s).
   - **A sustained low tone on a DRUM Part ducks: the tail counts
     (owner's decision, 2026-11-20).** A kit's LOW that
     stays up (a held 50 Hz sub, an 808 two seconds long) is a hit for as
     long as it holds within those 6 dB: a lower Part's LOW stays dipped
     under it (measured 9.6 falling to 7.5 dB over 2 s at AMOUNT 95).
     The key does not tell a long kick from a held note, and is not
     meant to: a long 808 tail on a FRONT kit holds everything under it
     down in LOW for its whole length.
   - The dip follows with τ 3 ms in and τ 100 ms out; the rule runs on the
     blocks between the slow rule's (2.7 ms).
   - **Dense kicks hold one dip.** Between kicks the dip is held, for
     1.5 of their gap past each onset: whole for onsets to 100 ms apart,
     fading to none at 220 ms, so no gap is a cliff (a hard edge at
     150 ms was 5.6 dB of flutter one millisecond past it). 16ths at
     170 BPM (88 ms) and 32nds at 120 BPM (62.5 ms) are one steady dip
     (under 0.5 dB peak to peak at AMOUNT 95); from 60 to 400 ms no 10 ms
     of gap moves the swing by 1 dB. Slower kicks each dip and let go.
     Held, not a release scaled to the gap: a release that slow would
     smear a sparse kick's dip into a duck.
   - An onset is the key hitting after quiet, or rising 2 dB between the
     rule's turns while still high (a kick over the last one's body), no
     sooner than 20 ms after the last.
   - LOW plays the deeper of its slow cut and the dip. Whole-mix pumping
     is not this (#13).
5. **AMOUNT is a curve (owner).** How deep BACK yields under FRONT (one
   rank apart 2/3 of it), in one typed table, `dsp::smart::KNOTS`:

   | AMOUNT | LOW, LOW MID | HIGH MID, HIGH | the kick's dip |
   |---|---|---|---|
   | 64, polite | 6 dB (4 one rank) | 6 (4) | 6 (4) |
   | 95, DEEP, 75 % of the slider | 12 (8) | 6 (4) | 15 (10) |
   | 127, OVER, provisional | 21 (14) | 9 (6) | 27 (18) |

   A monotone cubic (Fritsch and Carlson) through the knots and 0, a
   table by AMOUNT's code (`CAPS`): smooth, and more AMOUNT is never a
   shallower cut. `CutDb` goes to 27 dB. OVER's numbers wait for the
   owner's ear.
   - **"Never audible as pumping" holds for the lower part of the range
     only (owner's decision).** To about AMOUNT 64 the cuts are 4 to 6 dB
     and move over seconds. Past it the kick's dip is 10 to 27 dB in
     3 ms: heard, and meant to be.
6. **Stacked bands sum to the cut asked.** The bands overlap (a neighbour
   is 0.14 to 0.18 in phase at a centre, 0.34 a quarter turn off), so
   four 6 dB dips summed raw reach −8.8 dB at 428 Hz. Each band's dip is
   unmixed (`UNMIX`, the inverse of the bands' in-phase parts at the
   centres, measured from the filters as they run). Measured on 160
   points from 20 Hz to 20 kHz, over every combination of AMOUNT's knots'
   cuts (the kick's 15 and 27 dB in LOW among them), at 64 · 95 · 127:
   - the sum is at most 0.05 · 0.01 · 0.003 dB deeper than the deepest
     cut asked;
   - **it rises over unity between two bands**, where a cut's neighbour
     is lifted to hold its own centre: 0.27 · 0.46 · 0.56 dB at most,
     near 500 to 650 Hz (0.2 dB for one 6 dB cut). Holding the lift down
     costs more than it saves (no lift at all: 0.6 · 1.6 · 7.3 dB too
     deep), so it stays;
   - **deep, the bands' phase holds a centre up**: a band cut alone is
     0.01 · 0.14 · 2.3 dB short (LOW asked 27 dB reaches 24.7), and two
     adjacent bands asked 21 dB reach 10 to 18 dB. Parallel band-passes
     cannot do more, and they stay (owner's decision, 2026-11-20): OVER
     asks up to 21 dB and gives what the bands give cleanly;
   - **the pages read what is heard**: the view carries each centre's
     real cut (`heard`, the sum's size from the measured responses),
     within 0.001 dB of the filters, never the cut asked. No readout
     wraps: a byte a band at 1/8 dB reaches 31.9 dB.
7. **Time.** Each slow cut follows its target at block rate, τ 0.5 s
   deeper and τ 2 s shallower; under 0.01 dB it lands on 0. Each band's
   dip is lerped across the block from the last block's.
8. **The reverb's return is a member (owner).** It yields as a Part of
   its role would (the slow rule and the kick's dip), and makes nothing
   yield.
   - Its bands run on the return itself, after the reverb plays and
     before it joins pair 1 (`FxBus::process_washed`), so the tail
     already ringing is carved and dipped. Left and right take the same
     dips through the pairs that cut; it is heard as their sum at
     equal power ((L + R)/√2: a reverb's sides are decorrelated, so it
     reads as one side does, as a Part's bus would), decimated; at unity it is read, never written; a reverb at rest
     (`Reverb::play` silent) is not run.
   - **Where the sends are tapped (verified, `mix_sided`):** a Part's
     three sends take its bus after its EQ and SMART EQ's cuts
     (`Instrument::part_eq` runs before `mix_sided`), scaled by the send
     and the expression pedal, before LEVEL and PAN, the bus only (never
     a pad's side). So a Part cut by SMART EQ already feeds the reverb
     cut; the return's own bands act on what the reverb makes of it.
   - **Against REV › DUCK (ADR 0295):** DUCK is level-only and self-keyed:
     the whole return turned down under its own input, inside the
     reverb's play. This is per band and keyed by the Parts that outrank
     the return, on what DUCK leaves: they stack, and SMART EQ hears the
     ducked return.
   - The delay's return is not a member: it is mono and would be half
     the cost, but needs its own role, bill, state and column; left out,
     for the owner to ask.
9. **Off is free.** A member whose four dips are unity at both ends of a
   block is not written. With SMART EQ OFF or AMOUNT 0 the cuts release,
   then the state rests and bank 1's stub skips the door (it also checks
   `SmartEq::listens`). All-zero bytes are the rest (ITCM's boot zeroing).
10. **Billed (ADR 0124, 0262).** While SMART EQ's bands run (ON with an
    AMOUNT, or still letting go: `bill::SmartLoad`), admission and
    `recost` book them, in cycles a sample:
    - on a Part's first voice, as its knobs are (ADR 0112): `LISTEN` 36
      for a FRONT Part, `CUT` 84 for MID or BACK (whether or not anything
      outranks it now), `SIDE` 60 more for a DRUM Part that can be cut;
    - beside the bus, with the reverb TYPE's extra: `WASH_CUT` 160 for the
      return (`WASH_LISTEN` 36 in FRONT; nothing with the reverb off).
    - **The Part EQ (ADR 0284) is billed the same way now**: `TILT` 50 on
      the first voice of a Part whose EQ is not flat, twice for a DRUM
      Part.
    Flat, off and at rest every bill is as before.
11. **Bank 2, behind the Part EQ's door.** `part_eq::dsp::smart` is inside
    the Part EQ's S4 group (`*7part_eq3dsp*`). The door's one entry takes
    an `Ask`: the Parts' block, or the return's later in the block. No new
    door table. A closed door rests it with the EQ.
12. **State in ITCM** with the EQ (`PartEqs::smart`).
13. **UI.**
    - ROLE: MIXER › PRT › EQ cell C. Its band keeps the Parts' overview
      and meters (ADR 0293) and adds, beside each Part's meter, its four
      bands as thin bars (by the square root of the cut heard, 27 dB the
      whole bar), the edited Part's lit with its deepest cut in dB by its
      number.
    - MIXER › MST › LVL: SMART EQ (D), AMOUNT (E), REV ROLE (F) under VOL
      and PAN; its band, each Part's four cuts and role letter and the
      return's (`REV`). Not a page of its own (D2). The return's role is
      also REV › DUCK's cell C, SMART ROLE, beside the duck.
    - Bank 2 plans the band (`ui::smart_eq::plan_in`); bank 1 draws the
      plan. It is asked through `UiDoor`'s `meters` entry
      (`part_meter::Ask::Cuts`): with tester-04's entries the table is
      full at 32 words, and the bench's, ORBIT's and the MIDI UI's tables
      follow it at fixed slots, so it is not grown. Its helpers are all
      inlined into `plan_in`: a call between them is one the outliner
      shared with bank 1 (a jump into bank 2, caught by the split). A
      closed door draws nothing.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 fda23e52 (tester-04
merged) against this change merged with it, each set built in both trees.
Each budget line is 0.4's plus exactly its set's measured change, never
ADR 0198's slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---:|---:|
| default | +2,376 | +23,872 |
| orbit | +2,552 | +23,872 |
| bank2-gate | +2,368 | +23,872 |
| orbit,usb-config-writer | +2,552 | |
| fault-inject | +2,608 | |
| bench-algo | +2,552 | |
| bench-algo,master-tape | +2,496 | |
| bench-routing | +2,688 | |
| bench-memory | +2,808 | |
| bench-stress | +2,656 | |
| bench-orbit | +2,456 | |
| bench-tape | +2,808 | |
| filter-probes | +2,600 | |

- Bank 1 on ORBIT (903,156 B; +1,880 before the merge, 144 B the bill's `StillCut` and its Part, the rest PRT ›
  EQ's band over tester-04's overview and meters and REV › DUCK's cell): the three settings' specs and
  `get`/`write`/`enum_*` arms (code, an exception to ADR 0198 as ADR
  0284's are), the names, the bills (`bill::first_voice`, `bill::wash`,
  admission's with the door open or closed), the stub's test and the
  return's ask in `FxBus`, the renderer's arm and the plan's painter.
- Bank 2 on ORBIT: S4–S5 +23,872 B (`answer` 10.0 KB with the Parts'
  bands and the rules inlined, the return's pass 4.4 KB, the rate's
  making 2.3 KB, AMOUNT's table 1.5 KB, `ui::smart_eq`); total free
  467 KB. S0–S3 is untouched: 7,328 B free on ORBIT, as on 0.4 (under
  ADR 0251's 8 KB warning before this change).
- RAM: ITCM +1,464 B (`PART_EQS`: seven members' filters, levels, cuts
  and dips, the return's listening filters, the rate's coefficients):
  44,840 B used. `SMART_VIEW` 28 B in AXI. D2 unchanged: bench-algo, whose
  D2 a page id overflowed, links with tester-04's page 119 and no new id.

### CPU
The ORBIT build's instruction counts (`tools/loop_count.py`'s 1.606 cycles
an instruction), cycles a sample:

| | Instructions a sample | Cycles a sample |
|---|---:|---:|
| A cut Part's four bands (86 + 82 for 4 samples) | 42.0 | 68 |
| A listened Part's (81 for 16, 61 for 8) | 12.7 | 21 |
| A cut DRUM Part's side (73 for 4, twice) | 36.5 | 59 |
| The return, cut (its sum, its listening, left and right) | 91 | 146 |
| The return, listened to | 18 | 29 |

- Block-rate work (the rules, the levels, the dips, the cuts' steps):
  not a loop to count. On the host its time is 0.24 of five cut Parts'
  bands (`smart_eq_cost`): about 80 cycles a sample with six Parts.
  Counting every instruction of every block-rate loop body each trip, an
  over-count, gives 130.
- **Worst case, six Parts** (one FRONT, five behind it cut in every
  band): 5 × 68 + 21 + 80 = 441 cycles a sample, **4.4 % of rev V's
  10,000, 5.3 % of rev Y's 8,333** (4.9 % and 5.9 % by the over-count).
  The first build was 5 × 109 + 120 + 240, about 9 %.
- With the reverb's return cut too: +146 + 10, 6.0 % (rev V).
- A cut DRUM Part with a panned pad sounding: +59 each.
- The teaser's four Parts (two kits FRONT, a bass and a lead cut, the
  return cut): 2 × 21 + 2 × 68 + 55 + 156 = 389, 3.9 %.
- **Against ADR 0262 and 0273.** All of it is billed, inside admission's
  75 % share: the worst block stays where ADR 0262 put it, the share
  plus the unbilled fixed overhead (160 a sample, and 230 a sounding
  Part): 89 % of rev V's deadline with six Parts (569,700 of 640,000 cycles),
  93.5 % of rev Y's (the same overhead in cycles on 533,333), under
  ADR 0273's 95 % on both. Unbilled, SMART EQ would have put six Parts
  at 93 to 95 % on rev V and past the net on rev Y. If a bill is under
  its bands' real cost, the net sheds a voice and `load_sheds` counts it.
- A member moved to FRONT keeps its bands at the full rate until its
  cuts let go (seconds) and is billed as cut until then (`StillCut`).
- **A bill that grows can shed a sounding voice** when the pool is full:
  turning SMART EQ on, a role leaving FRONT, or TILT leaving flat each
  add to a Part's first voice.
- What the bills cost in voices: 84 a cut Part is about a tenth of a
  7-voice Sound's voice; a full pool on four Parts with the return cut
  gives up 372 cycles a sample of 7,500, up to one voice while SMART EQ
  is on.
- Not yet read on the unit (no flash in this change).

### Sound
- Every golden bit for bit (`just check`); OFF, and ON at AMOUNT 0,
  render bit for bit on three Parts at once and on a Part feeding the
  reverb (`smart_eq_render_test`); with every Part one role and nothing
  sent to the reverb it listens and writes nothing.

## Tuning
Measured on the 250 Hz – 2 kHz overlap (`smart_eq_ab::overlap`: over Hann
frames of 4,096, each bin's smaller power of every pair where one role
outranks the other, the Parts' buses after their EQ and SMART EQ at their
LEVEL) and on the kick's transient over the bass in 40–120 Hz (each kick
onset's next 43 ms, the kick's band energy over the bass's), SMART EQ off
against AMOUNT 64, 95 and 127.

**`just smart-eq-ab`.**
- A MODAL pad (MID, MODAL's INIT) over a bass (FRONT): overlap −1.1, −2.2
  and −3.1 dB; the pad's own 250 Hz – 2 kHz −1.7, −2.4, −2.9 dB, the bass
  untouched. The pad's LOW MID cut reads 3.9, 7.9 and 13.8 dB at most and
  holds through each ring (3.6–3.9, 7.0–7.9, 11.3–13.5 dB on each chord's
  last beat).
- A DRUM kit's kick on the beat (FRONT) over a held bass (MID): the
  kick's transient is −1.7 dB against the bass off, +1.4, +2.5 and
  +3.0 dB better on; the bass's LOW dip reads 3.9, 9.6 and 17.3 dB at
  most, its 250 Hz – 2 kHz within 0.03 dB.

**The six teaser vignettes** (teaser-04's, the two kits FRONT, the bass
MID, the lead or chords BACK, the reverb's return BACK), rendered through
the host instrument at this branch. Overlap and the kick over the bass,
against off:

| Vignette | Kick over bass, off | 64: overlap · kick | 95 (DEEP) | 127 (OVER) |
|---|---|---|---|---|
| electro (01) | 12.0 dB | −1.6 · +0.7 dB | −2.6 · +1.2 | −3.7 · +1.6 |
| breaks (02, MODAL NYLON) | 6.4 dB | −1.0 · +0.7 | −2.0 · +1.3 | −3.1 · +1.8 |
| glitch | 9.7 dB | −1.0 · +1.6 | −1.6 · +2.9 | −2.6 · +3.5 |
| dub (04) | 5.9 dB | −0.1 · +1.8 | −0.1 · +3.1 | −0.2 · +3.9 |
| triphop | 7.1 dB | −1.2 · +0.9 | −2.1 · +1.6 | −3.1 · +1.9 |
| garage | 4.2 dB | −1.4 · +1.2 | −2.0 · +2.1 | −3.0 · +2.6 |

- **The reverb's return** (mean / most cut heard, LOW MID, where the wash
  sits): triphop 2.5/3.1 dB at 64, 4.6/5.6 at 95, 7.4/9.6 at 127; breaks
  3.6/5.5, 7.0/10.8, 11.2/16.1; garage 2.6/4.3, 5.2/8.5, 8.5/13.6. In dub
  the kick dips its LOW by up to 4.6, 11.6 and 20.5 dB.
- **breaks' NYLON chords** (MODAL, BACK): LOW MID 3.5/5.4 dB at 64,
  7.0/10.6 at 95, 11.5/17.4 at 127.
- The mix is 0.3–1.6 LUFS quieter at 64 (0.4–2.0 at 95, 0.6–2.6 at 127)
  and the limiter does up to 2.3 dB less: less of the mix sits on top of
  itself.
- dub's overlap barely moves: its sub is under 100 Hz and its chords sit
  above the mud; there the kick's dip does the work (+1.8 to +3.9 dB).
- The bills are heard too: with SMART EQ on a full pool gives up a voice
  (electro's notes that found every voice busy went from 12 to 3: fewer
  are admitted). And the Part EQ's bill is new with it off: these renders'
  off takes are not the pre-SMART EQ renders' voice for voice.
- A sub bass as MID under the kick dips on every kick (3.9 dB at 64): a
  sub that should keep its weight belongs in FRONT with the kits. The
  teaser agent's call per vignette.

## Alternatives considered
- **A separate 8-band filterbank, or a small FFT.** Eight more filters a
  Part for answers folded into four decisions; 64 points cannot resolve
  LOW, 1,024 need 24 KB of frames and a frame's delay.
- **Cascaded bells (a graphic EQ's, with its interaction solve): not
  doing (owner, 2026-11-20).** They
  reach any depth cleanly, where parallel band-passes stop near 12 dB
  (Decision 6). About 20 cycles a sample more a cut Part (their
  coefficients move with the gain and must be lerped), and the bands
  could no longer be heard through. The parallel bands stay; the pages
  show the real cut.
- **Half-rate cut bands.** Their images are 40 dB under the source near
  20 kHz: not for an instrument sold on its sound.
- **A broadband gain on a pad's side.** Does not null against a band's
  cut.
- **The kick's release scaled to the gap between hits.** Smears sparse
  kicks; a hold only acts where hits outrun the release.
- **Cutting the reverb's send, not its return.** A third of the cost (the
  send is mono), but the tail already in the tank would not be carved,
  and the kick could not dip it.
- **Billing a fixed reserve whenever SMART EQ is on.** Costs a voice with
  one Part playing; by Part, each pays for its own bands.
- **Balancing within a role, as smart:EQ 4's layers.** A role is a
  promise: two FRONT Parts stay as mixed.
- **A learned, static curve (smart:EQ 4).** A synth's Parts change by the
  bar, and there is no learn button; it listens all the time, slowly.
- **A KICK RELEASE control.** One value and the hold cover what is
  played; a control only if a unit test by ear asks for it.
- **Keying the dip from the kit's whole LOW band.** Its skirt hears a
  snare's body; LOW's low-pass does not.
- **A new S4 door table, or a second entry.** The EQ's door already
  reaches the buses; a second entry outgrows the table's stride on a
  host, an `Ask` does not.
- **ROLE on PRT itself; MST › SMART EQ, a page of its own.** PRT's cells
  are full; a new page id grows `FocusMemory` past D2 on bench-algo.
- **Painting in bank 2 on ORBIT's `Ink`; `core::fmt` there.** The draw
  helpers built twice (+3.8 KB of bank 1); a `Write` vtable in bank 1
  pointing into bank 2.

## Consequences
- Every Part and the reverb's return have a role, and the master SMART EQ
  and AMOUNT, saved with the project; old projects load MID, BACK and OFF
  and play bit for bit.
- With SMART EQ on, a Part's first voice and the reverb cost more of the
  budget; a full pool plays a voice fewer.
- A Part's EQ (TILT, LO CUT) is billed now: a Part with its EQ off flat
  pays 50 cycles a sample on its first voice.
- Past about AMOUNT 64 the kick's dip is audible by design, and past
  about 12 dB the bands cut less than asked; the pages read what is
  heard.
- The four bands are fixed: no user control of their place or width.
- A docs page (annotated screenshots) is owed for PRT › EQ and MST › LVL.

## Sources
- #396; owner's design and decisions 2026-11-19; the review of
  2026-11-19; spec `docs/superpowers/specs/2026-11-19-smart-eq-design.md`,
  plan `docs/superpowers/plans/2026-11-19-smart-eq.md`.
- Perez Gonzalez and Reiss, DAFx-08 (selective minimization of masking);
  Pestana and Reiss, DAFx-14 (cross-adaptive spectral panning); Hafezi and
  Reiss, JAES 2015 (abstract); Zavalishin, *The Art of VA Filter Design*;
  Chamberlin, *Musical Applications of Microprocessors* (the
  state-variable filter); Bristow-Johnson, the Audio EQ Cookbook (the
  band-pass); Fritsch and Carlson, "Monotone piecewise cubic
  interpolation", SIAM J. Numer. Anal. 17 (1980); Välimäki and Liski,
  "Accurate cascade graphic equalizer", IEEE SPL 24 (2017) (the
  alternative, unread beyond its abstract).
- sonible smart:EQ 4 manual (groups, layers, impact): UX precedent only.
- ADR 0112, 0124, 0198, 0217, 0251, 0262, 0266, 0272, 0273, 0283, 0284,
  0286, 0293, 0295.
