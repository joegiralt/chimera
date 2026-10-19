# FX in the filter: one simple effect per voice, PRE or POST

**Status:** draft for owner review, 2026-10-17. It builds on the filter-types design (`2026-10-05-filter-types-design.md`, ADR 0096/0104) and the throwaway drive spike: the branch `drive-spike` and the private page "Drive Spike", which has mockups and 35 loudness-matched clips.

## Why

Each voice gets one simple effect, and you choose where it sits around the filter. The owner, 2026-10-17: "if people want an effect, you get to choose one effect … drive, a wave folder, the simplest of delays … and then you have your settings."

- Today a voice can run DRV and the AMP page's wavefolder at once. Folding a distorted signal makes little sense, and it costs two stages. One slot replaces both.
- The effects stay simple. The one that has to be convincing is FUZZ. The owner wants to push a VA bass into a Big Muff the way guitarists do, and analog fuzz is coveted because cheap digital distortion sounds thin and fizzy. After the spike, the owner's verdict was "there's something there".

## Goals

- **One FX slot on every filter kind** (CLEAN, DIRTY, LADDER, 2040). It has:
  - a **TYPE**;
  - that type's few pots, plus **MIX**;
  - **PLACE**: PRE runs into the filter, POST runs after it.
- **TYPEs:**
  - **OFF**;
  - **SOFT**: today's DRV clip;
  - **FUZZ**: EHX Big Muff Pi style;
  - **FOLD**: today's wavefolder, moved off the AMP page.
- **Later, not in this design:** **DELAY**, the simplest delay: one buffer that repeats, with TIME, FEEDBACK and MIX. It's a TYPE in the same slot. The cost to settle when it's designed is memory: one buffer per voice. Half a second at f32 on 8 voices is about 770 KB, so it will need short times, a lower bit depth, or a buffer per Part.
- **Sound bar:**
  - FUZZ sounds like a fuzz pedal, judged by the owner's ears at UAT, with no audible aliasing fizz on high notes at a normal setting.
  - SOFT and FOLD sound at least as good as today's DRV and fold.
- **One effect per voice** replaces the two stages (DRV and FOLD) a voice can run today.
- **Kinds and types up front.** The FLT front page shows everything that picks a kind of sound.

## Non-goals

- More pedals for now. OVERDRIVE (BD-2 style) and DISTORTION (RAT style) were rendered in the spike and can come back as TYPEs later.
- Amp sims, cabinets and neural models (BYOD's catalogue is GPL and out of scope anyway).
- A per-Part effect after the voices.
- Two effects on one voice.

## Anti-goals (a failure even if it works)

- FUZZ sounds digital: fizzy highs, aliasing on high notes.
- Turning a knob zippers. All parameter changes are lerped (CLAUDE.md), and sweeps must be perfectly smooth (the "filter never steppy" rule).
- It doesn't fit: 8 voices of FUZZ on a busy patch blow the audio budget.
- An old Sound fails to load. Old Sounds must load safely; they needn't sound the same (no-legacy-compat).

## Decided by the owner

1. **The DRV block goes away.** The chains become:
   - **VA:** VA · FLT · AMP · MOD
   - **ALGO:** ALG · OSC · FLT · AMP · MOD
   - **TAPE:** SRC · DECK · FLT · AMP · MOD
   - **MODAL:** unchanged, but it gains the slot through its FLT.
2. **FOLD moves into the slot as a TYPE.** The AMP page loses FOLD and FOLD MIX, and AMP › SYM goes away. AMP keeps SEND 1–3 and VCA VEL; its two free cells stay empty for now. A DJ-style isolator EQ was discussed and belongs to the mixer work (#570), not here.
3. **The filter's LEVEL stays its own knob and is renamed INPUT LEVEL** (the cell says INPUT). It's what drives the 2040's saturating OTA stages, the Kobol Expander character, whatever PLACE says. In PRE, the effect's output level and INPUT LEVEL stack.
4. **The name is FX.** The cell on the front page is FX and the sub-page is FLT › FX.
5. **Layout A** (mocked up on the Drive Spike page as layout v2, where the slot was still called DRIVE):
   - **FLT front page:** KIND · VOICE · FX (the TYPE) · PLACE · CUTOFF · RES. On CLEAN, DIRTY and LADDER, which have no VOICE, that cell is MODE. FREQ and Q keep their Cascadia words on CLEAN.
   - **FLT › MOD:** ENV AMT · KEYBD · FM3 · FM3 IN · QM · QM IN.
   - **FLT › FX:** INPUT · the type's pots · MIX. Above the cells, a strip shows where the effect sits (VA › FUZZ › FLT, or VA › FLT › FUZZ) and the type's curve: a faint sine in, the shaped wave out.
6. **PLACE has a glyph:** an FX box and a FLT box joined by an arrow, the lit box being the effect (lit left = PRE, lit right = POST). It's drawn large in the focus band and small in the cell.
7. **Routed cells are MOD matrix cells.** ENV AMT, KEYBD, FM3 and QM on the filter pages are views of the matrix cells (one stored value, two places to edit it), as they are today.
8. **Cost:** each TYPE bills its real cost (see Cost).

**Per kind (my call, from today's panels):**

| Kind | FLT front | FLT › MOD |
|---|---|---|
| CLEAN, DIRTY, LADDER | KIND · MODE · FX · PLACE · FREQ · Q | FM1 · FM2 · FM3 · FM3 IN · QM · QM IN |
| 2040 | KIND · VOICE · FX · PLACE · CUTOFF · RES | ENV AMT · KEYBD · FM3 · FM3 IN · QM · QM IN |

- CLEAN's FM1 and FM2 are the routes the 2040 calls ENV AMT and KEYBD.
- The 2040's fixed MODE cell is dropped. It can't change, so KIND's focus band names it (4-POLE LP).
- LEVEL, now INPUT, moves from FLT › MOD to FLT › FX.
- Nothing else is lost. The front page keeps its filter-response picture (BigViz); the strip and curve are FLT › FX's own new viz.

## LPG: a fifth filter KIND (mocked now, built with DRUM)

The owner, 2026-10-17: add a low-pass gate as a filter kind. "It doesn't need to work right now. Once we start doing the drum stuff… that's when it really will matter."

- **In this design:** the mockups only (Drive Spike page, section LPG), plus a KIND value reserved so that the codec and UI are ready. Until DRUM, it isn't selectable, or it shows COMING SOON per ADR 0132's pattern (the builder picks).
- **The model:** a Buchla 292 style vactrol low-pass gate. Build it from the circuit and the published research (Parker and D'Angelo, DAFx, on the Buchla LPG). Surge's LPG is GPL-3.0, so we don't read its code.
- **Mocked cells:**
  - **front:** KIND · MODE (LP · COMBO · VCA, default COMBO) · FX · PLACE · OPEN (the 292's offset) · DECAY (how slowly the vactrol lets go);
  - **FLT › MOD:** STRIKE (the route that pings the gate, ENV 1 by default, a MOD matrix cell) · KEYBD · FM3 · FM3 IN · QM · QM IN.
- **The front-page picture** is the vactrol's response (a strike, then the slow curved close), not a frequency response.
- **The FX slot applies to the LPG** like every other kind.

## Pots per TYPE (cells on FLT › FX after INPUT; MIX last)

| TYPE | Pots | Default PLACE |
|---|---|---|
| SOFT | DRIVE · TONE · LEVEL | PRE |
| FUZZ | SUSTAIN · TONE · VOLUME | PRE |
| FOLD | FOLD · SYM · LEVEL | POST (today's place) |

- **FUZZ's TONE is the Muff's tone stack:** an LPF/HPF blend, which gives the mid scoop at noon.
- **SOFT keeps today's DRIVE and TONE behaviour.** Its TONE moves the clip's pre and post gains; it isn't a filter.
- **Each Sound stores one set of FX values,** the pots by cell position: TYPE, PLACE, P1–P3 and MIX are new params with new idents. Switching TYPE keeps the cell values and reinterprets them, as filter KIND does today (my call).
- **MOD matrix and MIDI-map targets** that point at the retired DRIVE, FOLD and SYM params move to P1 or P2 by position. They act on whatever TYPE is set. The migration is its own plan task.

## FUZZ: research first

The owner: "there's probably papers on this to make convincing fuzzes. We should pay heed."

- **Before FUZZ is built,** read the strongest 3–6 sources and write down what each says to do:
  - Big Muff and transistor-fuzz modelling;
  - diode clippers (Yeh, Abel and Smith, DAFx-07);
  - cheap anti-aliasing for static and stateful nonlinearities (ADAA and its stateful extensions);
  - wave-digital fuzz models.
- Start from the curated DAFx reading list (the DAFx reading list memory), as for other features.
- **Build FUZZ to what the papers say.** The spike's model is the starting point and the A/B baseline, not the answer.
- **Verify the Muff's stage gains** against published schematics. They're unverified estimates in the spike; the tone-stack values are the published ones.

## Placement and levels

- **PRE** runs between the source (oscillators, exciter or deck) and the filter's input stage. **POST** runs after the filter, before the DC blocker and VCA. That's where the spike's hook sat. It's inside the voice, before the amp envelope.
- **Engines:** VA, ALGO, TAPE and MODAL all get the slot through their FLT. There's one slot per voice. DRUM gets it when DRUM exists.
- **Each engine needs a fixed reference level at the slot,** a constant scale per engine and never a running normaliser, because the effect's dynamics must survive. It's needed so that the same knob position bites the same on every engine. In the spike, MODAL STRING peaked at only 0.29 pre-filter against VA's 1.0. Without a reference, SOFT on MODAL acts as a +15 dB clean boost. The builder picks each engine's reference and documents it.

## Cost

The spike's host numbers, per voice, ns/sample:

| SOFT | FUZZ, 8× half-band |
|---|---|
| 0.5 | 124 |

That's a guess of roughly 70–120 % of the M7 for 8 voices of FUZZ. As built in the spike, it doesn't fit.

- **The budget (owner, 2026-10-17): each TYPE bills its real cost.** The admission bill rises with TYPE, so a Part on FUZZ gets fewer voices only when a project nears the CPU limit. OFF and SOFT cost what today's DRV does, and FOLD what today's fold does.
- **The ceiling is about one filter's cost per voice** for FUZZ (my call), measured on the unit bench. The first task measures today's DRV, FOLD and filter bills on the unit so that the ceiling is a number.
- **Expected route:** what the research step says, likely ADAA at 2× instead of 8× half-band cascades, with table-driven diode laws as in the spike.
- **The sound bar applies after the cost work.** Re-render the spike's FUZZ and SOFT clips with the shipped models and A/B them against the spike's, loudness-matched, before UAT.
- **If FUZZ can't meet the sound bar under the ceiling,** stop and bring the numbers to the owner.

## Provenance

- **Written from circuits and papers.** No GPL code: never read BYOD's sources.
- **Record the provenance in an ADR** (the DSP library curation rule).

## Loading old Sounds

- DRV's DRIVE, TONE and MIX load as SOFT, PRE, with those values.
- The folder's FOLD, SYM and MIX load as FOLD, POST.
- **If an old Sound ran both, DRV wins** and the fold is dropped. "Ran" means `Voice::stage_runs` says the stage is on (DRIVE above 0 for DRV, FOLD at 0.001 or more for the fold). My call: either choice is defensible.
- The retired params translate on load (the existing `Retired` translation path), and the disk idents stay frozen.

## Tests and proof

- **Host:**
  - goldens per TYPE, PRE and POST;
  - ramps click-free on every pot and on TYPE and PLACE changes (crossfade on switch);
  - an aliasing floor test for FUZZ at a high note;
  - load translations for old DRV and FOLD Sounds.
- **Unit bench:** the per-voice cost of each TYPE at 8 voices.
- **UAT by the owner's ears:** the re-rendered clips, then the unit.

## Left to the builder

- FUZZ's exact topology and values, within the research step's findings.
- The anti-aliasing scheme, as long as it meets the budget and the sound bar.
- Whether OFF is a TYPE value or the bottom of the list.
- How the strip's curve is drawn from the live pots.
- The type switch crossfade time (the existing stage gate is 20 ms).
- The ADR numbers, and the order of tasks in the plan.

## Sequencing

This queues behind TAPE slice 1, which is running now, and goes into the 0.2.0 release if it's ready.
