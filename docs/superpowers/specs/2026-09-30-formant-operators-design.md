# Formant Operators for Algo: Design

**Date:** 2026-09-30
**Status:** Refreshed 2026-10-11 against main 4473f0a1; awaiting owner review of the delta
**Issue:** private issue #243
**Related:** private issue #247 (the per-Part MIDI map); #37 (operator pages, a prerequisite)
**Builds on:** main at 4473f0a1. Modal 2, ORBIT, projects-core, the param sweep, ADR 0044's ladder and ADR 0108's crate split have all merged: the DSP and the model live in `chimera-engine`, the UI in `chimera-core`. The CPU share of ADR 0124 (75 %, branch `audio-share-75`) is a prerequisite for three formants (§ 1).
**Sources:** the research brief (FSVR's prose docs and Yamaha's patent US5610354), the Algo code on main (`chimera-engine/src/dsp/algo/*`), and ADRs 0010, 0022, 0024–0026, 0031, 0045, 0056, 0063, 0067, 0081, 0087, 0091, 0096, 0108, 0109, 0112, 0119, 0122 and 0126.

**Refresh (2026-10-11).** Every path, type, constant and ADR number now matches main at 4473f0a1. The Decisions are unchanged. What moved (the ledger's `refresh-delta.md` lists it for the owner):
- **The budget.** Today's cost model bills the filter's moving path and its LEVEL clipper under heavy routes, and the bus and chain cost more. The budget is 72 at 70 % and 197 at 75 %, so N = 3 needs `audio-share-75` and the 120 target (§ 1).
- **Labels follow the full-word rule** (§ 2, § 6): FORMANT, CENTRE, COARSE, VELOCITY and NOISE. BW, RES, SLOT and FDBK stay short because the full words don't fit.
- **Loudness** is ADR 0063's reference at ADR 0109's gain, −11.0 LUFS (§ 3, Demo clips).
- **Saving** goes through ADR 0087's neutral omission, and the Sound cap grows (§ 10).
- **Navigation** is ADR 0044's ladder, so focus by identity goes through `NavCtx`, `Recall` and `PageAt` (§ 6).

**Revision (2026-09-30).** An adversarial review found four promises the first draft broke and eight weaker points. The owner and the controller ruled on them, and this revision makes these changes:
- **Owner's rulings:**
  - VOWEL runs U–O–A–E–I;
  - each operator has a VOWEL SLOT.
- **Level:** the level law uses the grain's exact energy. Peaks are bounded, and a modulator's depth is peak-referenced.
- **Grain timing:** grains start on the unmodulated clock, and PM moves them without ever cutting one.
- **Lanes:** the lanes always alternate, so there is no lane-mode switch.
- **NOIS:** the noise is scaled before its poles.
- **Width:** the width floor is the same at every SKIRT.
- **Aliasing:** the alias rule uses the skirt's −60 dB extent.
- **Everything else:** the tests, the INIT and codec text, the operator-page dependency, and the costs are all revised.

## Why

The FS1R's formant operators make voices, choirs and breath from FM's parts: a grain train whose pitch is the note and whose spectral peak is fixed in Hz. Algo already has six operators, 32 algorithms, MORPH and a mod matrix, so two new operator kinds give most of #220's sound for far less work than a new engine.

This spec adds a voiced kind, FRMT, and an unvoiced kind, NOIS, to Algo's WAVE list, and a VOWEL macro that moves them together. It is Chimera's own design: FS1R behaviour informs it, but it is not an FS1R clone. In particular, LEVEL means level, and no width or shape control changes loudness.

**Done when:**

- any of the six operators can be FRMT or NOIS, as a carrier or a modulator, in every algorithm and through MORPH;
- VOWEL, FRMT and BW are mod destinations;
- the pages adapt: a patch with no formant operator shows the same pages and cells as today, apart from one dimmed cell (§ 6);
- every test in § Tests passes, and old patches load and sound bit for bit as before;
- its ADR is written (the next free number; 0127 is the highest on any branch on 2026-10-11, 0128 is reserved by `midi-bend-sustain` and 0129 is `va-glyphs`', so expect 0131, after Phase 0's 0130);
- the three demo clips have passed the owner's UAT;
- the chip bench's formant rows are measured at the ship flash.

## Decisions (owner, 2026-09-30)

- **Operator kinds.** FRMT (voiced) and NOIS (unvoiced) join the WAVE list, so any of the 6 operators can be one.
  - They work as carrier or modulator, in FM, in every algorithm and in MORPH.
  - The patch is billed from its actual operators, so the allocator plays fewer voices on heavy formant patches.
- **Voiced (FRMT).** Windowed-sine grains are fired at the note's pitch, with a carrier at the formant frequency inside each grain (the patent's approach).
  - Its parameters are FRMT in Hz, KEY (0–100 % keyboard follow), BW (bandwidth, which sets the window length) and SKIRT (window shape).
  - Phase modulation enters both the grain phase and the carrier phase.
  - Two grain lanes overlap.
- **Unvoiced (NOIS).** Noise runs through a resonant band shifted to CENTRE, with WIDTH and RES.
- **Level is normalised.** BW, SKIRT and WIDTH change tone only, never level. This is our own take, not an FS1R clone.
- **Clean peaks (owner, 2026-09-30).** A formant operator at full LEVEL sits about 12 dB under a sine's RMS and never exceeds full scale on its own. Patch loudness is set against ADR 0058's loudness reference, not by raising the operator.
- **The four-voice floor holds (owner, 2026-09-30).** No formant patch may drop below ADR 0026's four voices on rev V, even with heavy routes and the master tape.
  - A per-patch formant budget, derived from the cost terms, enforces this by construction (§ 1): it allows at most 3 FRMT operators, with NOIS counted by its own cost.
- **VOWEL macro.** One control and one mod destination move every formant operator's frequency along **U→O→A→E→I**, the vowel triangle from dark to bright, by interpolating a small built-in vowel table. Each operator's FRMT offsets it from its vowel position.
- **VOWEL SLOT (owner, 2026-09-30).** Each operator has a note-on cell on its operator page: AUTO (by order), F1, F2, F3 or OFF.
  - Two operators may share a slot, for a detuned choir.
  - OFF ignores VOWEL.
- **Pages (adaptive):**
  - VOWEL takes the empty 6th cell of the ALGO home page, dimmed when no operator is a formant kind.
  - A FRMT group page joins the OSC sub-row only when at least one operator is FRMT or NOIS.
  - Each operator page swaps in its kind's cells.
  - Plain FM patches look as they do today, apart from VOWEL's dimmed cell (§ 6).
- **Modulation.** VOWEL, each operator's FRMT and each operator's BW are modulatable and read every block. SKIRT, KEY, VOWEL SLOT and the noise shape are read at note-on.
- **No MIDI CC mod source.** External control is the per-Part MIDI map, #247.
- **Cost.** It is estimated by ADR 0056's instruction-count method.
  - The owner's estimates were 75–110 cycles per voiced operator and about 55 per unvoiced one, against 60 for a sine.
  - There are no new wave tables, and DTCM is untouched.
  - The chip bench confirms the figures at ship.
- **Saving.** Disk codes and idents are frozen (ADR 0045). Old patches are unaffected.
- **QA.** Claude writes the tests, and the owner does UAT: unit tests, the param-sweep harness, and three desktop demo clips (a choir pad, a talking bass, and breath).
- **ADR.** It covers the patent provenance, the operator kinds and the cost terms.
- **Order.** This comes after Modal 2 merges, alongside ORBIT and projects-core, and after #37's operator pages.

## Principles (binding)

- **Chimera's own code.** No FSVR code, tables or constants (GPL-3, ADR 0032). The sources are only:
  - behaviour described in FSVR's prose docs;
  - the expired patent US5610354;
  - public measurement data (§ 5).
- **The table path doesn't move.** Every operator whose kind is a table wave runs exactly today's code, so every existing golden stays bit-identical.
- **Functional core.** These laws are pure functions of their inputs, and tests call them directly: the window, the width floor, the level and peak gains, the vowel interpolation, the slot resolution, and the band and edge laws. The kernel only runs them.
- **The audio thread never allocates, never blocks**, and reads no new per-sample table.
- **No `unsafe` for the new state.** Each operator's formant state is a plain enum.

## 1. Operator kinds

### Types

- `WaveId` stays what it is: an index into the 16 generated tables (`WAVE_COUNT = 16`, `chimera-engine/src/dsp/algo/waves.rs`). The tables, `chimera-waves` and the DTCM tables rendered at boot (ADR 0122, amending 0025) are unchanged.
- A new `OpKind` is what an operator is:

  ```rust
  pub enum OpKind { Table(WaveId), Formant, Noise }
  ```

  It has `from_code(u8) -> Option<OpKind>`, `clamped(u8)`, `code(self) -> u8` and `is_formant(self)`, which is true for Formant and Noise.
- `AlgoOpParams.wave` stays a `u8` code, and the engine reads it only through `OpKind`. `AlgoEngine.waves: [WaveId; OPS]` becomes `kinds: [OpKind; OPS]`.

### WAVE's list and codes

| Code | Name | Ident | Kind |
|---|---|---|---|
| 0–15 | W1 … SSAW | as today | `Table(WaveId(code))` |
| 16 | FRMT | `FRMT` | `Formant` |
| 17 | NOISE | `NOIS` | `Noise` |

- **Names follow the full-word rule** (refresh 2026-10-11). A value is measured in `theme::FONT_VALUE` against `theme::CELL_BAR_W` (62 px).
  - NOISE fits (46 px), so it is the name, while its frozen ident stays `NOIS`.
  - FORMANT doesn't fit (74 px), so code 16 reads FRMT.
- `OP_KIND_NAMES` and `OP_KIND_IDENTS` (18 entries each) are `WAVE_NAMES` and `WAVE_IDENTS` followed by the two new entries. `ALGO_OP_SPECS[0]` lists them, with `max` 17.
- The const assert becomes `ALGO_OP_SPECS[0].max as usize == OP_KIND_COUNT - 1`.
- `set_enum_code` accepts 0–17. Codes 16 and 17 are appended to `disk_codes_v1.txt` (K 3–8, `OP_A` … `OP_F`) for each of the six operator blocks.
- The OSC home page (WAVE) is unchanged: FRMT and NOISE are the last two entries of its list.
- **WAVE's focus glyph** is `FocusGlyph::Wave` (ADR 0070), which today clamps the value to a `WaveId`, so codes 16 and 17 would draw SSAW. It resolves through `OpKind` instead:
  - a table draws `wave_trace`;
  - FRMT draws one grain;
  - NOISE draws its band.

### The formant budget

**The floor's worst case.** This is ADR 0026's convention: LP24 on the clean SVF (steady bill 0), no FOLD, DRIVE or ALGO taps (ADR 0096), and A16 ∪ A17's 12 links. On top of that, **heavy routes and the master tape**:

- **Heavy routes (refreshed 2026-10-11).** The 2026-09-30 case is priced on today's model. In 2026-09-30's matrix, "all 16 destinations" included CUTOFF. Since filter-types (ADR 0096), CUTOFF and RES are fixed columns (FREQ and Q) beside 16 registry columns, and a route into them bills the filter's moving path. So the case is:
  - ENV B in BURST mode on the VCA, with its LEVEL routed;
  - a slide;
  - FREQ and Q, and all 16 registry destinations, the filter's LEVEL among them;
  - all three LFOs as FUNC.
- **What it costs:**
  - `ModRouting` is BASE 47 + CLAMP 24 + ENV_B 98 + BURST 76 + LEVEL 10 + SLIDE 33 + DEST_FIRST 6 + 17 × DEST 1 + 3 × FUNC 5 = **326**. CURVE isn't billed in BURST mode: `env_slot_cost`'s arms are exclusive.
  - The filter adds its moving path, `CLEAN_MOVE_COST` 75, and LEVEL's clipper, `FILTER_LEVEL_COST` 35. Neither was billed on 2026-09-30.
  - With `Voice::CHAIN_COST` 36 (was 10), the voice's overhead is 326 + 110 + 36 = **472**.
- **Four voices on rev V with the tape:** ⌊(7,000 − 1,494) / 4⌋ = 1,376 cycles a voice at main's 70 % share. That leaves the engine **904**.
  - At ADR 0124's 75 % (branch `audio-share-75`): ⌊(7,500 − 1,494) / 4⌋ = 1,501, which leaves **1,029**.
- **The shape's fixed terms:** `COST_BASE` 370, 12 links × 8 = 96, and six operators at `COST_OP` + `COST_FEEDBACK` = 366, so 832 in all (unchanged). That leaves the formant operators' extra cost over a table operator **72 cycles at 70 %**, or **197 at 75 %**.
- **ORBIT is outside the convention.** While it plays, ADR 0091 reserves 1,028 more, and even the costliest table patch under heavy routes gets 3 voices.

The budget:

| Kind | Charge (its cost − `COST_OP`) |
|---|---|
| FRMT (billed at SKIRT 1–3's 130, so a SKIRT edit can never break the budget) | 70 |
| NOIS | 10 |
| Table | 0 |

```
FORMANT_BUDGET = ⌊(budget − FxBus::COST with the tape) / 4⌋ − 472 − 832
               = 1,376 − 472 − 832 =  72   at 70 % (main)
               = 1,501 − 472 − 832 = 197   at 75 % (audio-share-75)
Σ charges ≤ FORMANT_BUDGET   ⇒   N = ⌊FORMANT_BUDGET / charge(FRMT)⌋
```

| Share | Budget | N at the estimate (charge 70) | N at the target (charge 60) |
|---|---|---|---|
| 70 % (main) | 72 | 1 | 1 |
| 75 % (`audio-share-75`) | 197 | 2 | **3** (CHOIR: 3 · 60 + 10 = 190 ≤ 197) |

- **History.** The first draft had 225, and Modal 2's terms gave 200. Today's 197 at 75 % is that same picture. At 70 % the filter's newly billed routes take the margin down to one FRMT.
- **The budget is derived at build time** and never typed in.
- **Optimise first, fall back to 2 (owner, 2026-09-30).** A performance pass makes FRMT cheap enough that 3 fit on these terms. It uses block-at-a-time spans between grain starts, hoisted per-block and per-grain work, and output bit-exact against the per-sample path. The target is `COST_FRMT` ≤ 120 by ADR 0056's method.
  - **Refresh (2026-10-11).** The fallback to 2 needs the 75 % share, and on main's 70 % neither outcome is reachable. The plan stops for the owner at Task 3 if the derived N is under 2.
  - If the target is met, N = 3, and CHOIR and TALK BASS keep three formants.
  - If not, N = 2, and they take two-formant voicings. Nothing stops.
  - The ADR records both the target and the outcome.
- **Why three is enough (owner, 2026-09-30).**
  - F1 and F2 carry a vowel's identity, and F3 adds speech clarity. More than three isn't worth the polyphony it costs.
  - Formant operators are meant to be mixed with regular FM operators, not to fill all six.

**The rule the type enforces is exactly `charge(FRMT)·n_FRMT + 10·n_NOIS ≤ FORMANT_BUDGET`**, checked on every assignment. At 75 % with the 120 target (charge 60, budget 197):
- 3 FRMT go with at most 1 NOIS (190);
- 2 FRMT go with up to 4 NOIS (160);
- any number of NOIS goes with fewer FRMT;
- 3 FRMT with 2 NOIS (200) is refused.

- **Derived, not typed in.** `FORMANT_BUDGET` is a `const` computed from:
  - `COST_*`;
  - the bus with the tape (`FxBus::COST` under `master-tape`, today 1,494);
  - the heavy-route overhead, from `ModRouting`'s terms, `CLEAN_MOVE_COST`, `FILTER_LEVEL_COST` and `Voice::CHAIN_COST`;
  - the rev V `SampleBudget` (`hw::AUDIO_BUDGET_PERCENT`).

  When the bench or the share updates a term, N follows.
- **Only a patch within budget can be built.** Kinds live in a newtype, `Kinds([OpKind; OPS])`, held by `AlgoParams`:
  - Its only mutator, `AlgoParams::try_set_kind(op, kind) -> Result<(), OverBudget>`, refuses a kind that would overspend.
  - `AlgoOpParams` no longer carries its own wave byte. The `(AlgoOp(n), WAVE)` address is served through `AlgoParams` (`BlockRead::block` and `Blocks::block_mut` in `chimera-engine/src/addr.rs`), so every write path goes through the check: the UI, `set_enum_code`, the decoder, the param sweep and a future gang edit.
  - `Kinds::new` is `const` and checks its argument, so INIT and the factory Sounds are checked at compile time.
- **The WAVE cell.** Turning the encoder steps over a refused FRMT or NOIS, and a toast reads "FORMANT BUDGET FULL". The value never lands on a refused kind.
- **Decoding.** A file that would overspend decodes its excess formant operators as W1, in operator order, and the load toast says so. No firmware writes such a file.
- **Why a budget and not a cap on voices:** the allocator can't un-build a patch mid-song, and ADR 0026 promises the floor for every patch. Refusing the fourth FRMT at edit time is the smallest rule that holds by construction.

### Formant slots

Each formant operator resolves its **slot** at note-on, from its VOWEL SLOT setting (§ 2):

| Setting | Slot |
|---|---|
| F1, F2, F3 | that formant |
| AUTO | its position among the formant operators set to AUTO, counted from operator 1: the first is F1, the second F2, the third F3, and any further one is none |
| OFF | none: VOWEL doesn't move it |

- **Why AUTO counts only AUTO operators:** an explicit slot doesn't shift anyone else. With operators 1 and 2 set to F1 for a detuned pair, AUTO operators 3 and 4 still get F1 and F2. That doubles F1, which the owner allows.
- **Why a fourth AUTO operator gets no slot:** F4 and higher barely move across vowels; they carry the voice's colour, not the vowel's identity.
- **Latched at note-on.** A kind or slot edit while a note is held doesn't move that note's other operators. The next note resolves the slots again, so an edit never steps a sounding formant (review I1).

## 2. Parameters

### New stored parameters

Operator block (`AlgoOpParams`):

| Id | Ident | FRMT label | NOIS label | Range (stored) | Law | Default | Read | Modulatable |
|---|---|---|---|---|---|---|---|---|
| 13 | `FRMT` | FORMANT | CENTRE | 0–168 | `100 · 2^(v/24)` Hz: 100 Hz to 12.8 kHz, 50 cents a step | 72 (800 Hz) | every block | yes |
| 14 | `BW` | BW | WIDTH | 0–216 | `20 · 2^(v/24)` Hz: 20 Hz to 10.24 kHz, 50 cents a step | 72 (160 Hz) | every block | yes |
| 15 | `SKIRT` | SKIRT | — | 0–3 | window exponent `q = 2^(SKIRT+1)` (§ 3) | 1 | note-on | no |
| 16 | `KEY` | KEY | — | 0–100 | % keyboard follow | 0 | note-on | no |
| 17 | `RES` | — | RES | 0–100 | % tone at the centre (§ 4) | 0 | note-on | no |
| 18 | `VSLOT` | SLOT | SLOT | 0–4 | AUTO, F1, F2, F3, OFF (idents `AUTO` `F1` `F2` `F3` `OFF`, frozen codes 0–4) | 0 (AUTO) | note-on | no |

Algo block (`AlgoParams`):

| Id | Ident | Label | Range | Law | Default | Read | Modulatable |
|---|---|---|---|---|---|---|---|
| 4 | `VOWEL` | VOWEL | 0–100 | position `x = v/25` along U O A E I (§ 5) | 50 (A) | every block | yes |

- **Labels follow the full-word rule** (refresh 2026-10-11). A label is measured with `draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING)` against `theme::CELL_BAR_W` (62 px).
  - These fit: FORMANT 57, CENTRE 48, WIDTH 38, SKIRT 34, KEY 24 and VOWEL 42.
  - These don't, so the short forms stay: BANDWIDTH (72) is BW, RESONANCE (76) is RES, and VOWEL SLOT (76) is SLOT.
  - The idents are frozen and unchanged.
- **Motion (ADR 0112).** Each spec declares one:
  - FRMT, BW and VOWEL are `Continuous(Smoothing::KNOB)`. The Part's knobs glide them through `AlgoLive`, as FINE and DETUNE are.
  - KEY and RES are `Continuous(Smoothing::NoteOn)`.
  - SKIRT and VSLOT are `Stepped(Change::NoteOn)`.
  - `param_declarations_test` and `param_motion_test` cover them.
- **Focus glyphs (ADR 0067).** FRMT, BW, KEY, RES and VOWEL take the default `Arc`, and SKIRT and VSLOT take `None` (word choices).

- **One field serves both kinds for frequency and width.** FRMT and CENTRE are the same stored field, and so are BW and WIDTH.
  - Each pair means the same measurable thing on both kinds: the frequency of the spectral peak, and the peak's −3 dB full width in Hz.
  - So the FRMT group page can show all six frequencies side by side, and a kind change keeps the sound's region.
  - SKIRT, KEY and RES mean different things, so each has its own field.
- **"The noise shape" is RES.** That is the NOIS setting read at note-on. WIDTH is BW's field, so it is modulatable like BW.
- **Why these ranges:**
  - **FRMT** spans every Peterson & Barney formant (270 Hz to 3.0 kHz), with room above for bright formants and below for talking-bass F1s. 50 cents a step is at a formant's just-noticeable difference, about 3–5 %.
  - **BW** reaches down to 20 Hz, where a noise band is nearly a tone, and up to 10 kHz, a full breath.
  - **SKIRT** stops at 3: at q = 16 the sidelobes are already under −80 dB.
  - **KEY and RES** are percentages, as the owner asked.
  - **VOWEL**'s 0–100 puts each vowel exactly on a stored step, 25 apart.
- **Why these defaults:**
  - FRMT 800 Hz sits near an open vowel's F1.
  - BW 160 Hz is a narrow, vowel-like formant on low voices. Above about A2 the width floor sets the width (§ 3).
  - SKIRT 1 gives a clean formant without the q = 2 window's −28 dB sidelobes.
  - KEY 0 keeps the formant still, which is the point of formants.
  - RES 0 is pure band noise.
  - VOWEL A is the table's reference, where every operator's offset is its FRMT exactly (§ 5).
- **Display.**
  - A new `ValFmt::LogHz { base: u16, per_octave: u8 }` reads as CUTOFF's `Law::Cutoff` does (`ui/fmt.rs`): whole Hz under 1 kHz ("734 Hz"), then kHz to three figures ("1.10 kHz", "12.8 kHz").
  - A new `ValFmt::Vowel` shows the nearest vowel's letter, and the cell's arc shows the exact position.
  - KEY and RES are `ValFmt::Int(100)`, SKIRT is `ValFmt::Int(3)`, and VSLOT is `ValFmt::Names`.
  - FRMT's cell shows the stored Hz, which is the Hz heard at VOWEL A and C4. The operator page's viz shows the live FC and BW_eff (§ 6).
  - A mod-matrix column is labelled from the static spec, so BW on a NOIS operator reads "BW" there. That is accepted.
- **INIT.** `AlgoParams::default` sets FRMT per operator to the grid points nearest A's formants:

  | Operators | FRMT (stored) | Hz |
  |---|---|---|
  | 1 | 69 | 734 (F1) |
  | 2 | 83 | 1,100 (F2) |
  | 3 | 111 | 2,470 (F3) |
  | 4–6 | 123 | 3,490 (a steady fourth formant) |

  - INIT's algorithms are T1 (A) and A1 (B). Setting operators 1–3 to FRMT at MORPH A gives T1's stack: one FM-smeared formant on operator 1. At MORPH B (A1, all carriers) it gives the vowel A.
  - The spec default stays 72, as INIT's LEVELs already differ from LEVEL's spec default (ADR 0049).
  - These INIT values are part of the frozen decode base (§ 10), so they never change.
- **ParamId 13 onward** are taken here. #37's LEVEL SCALE takes the next free id.

### What the existing operator parameters mean

| Parameter | Table | FRMT | NOIS |
|---|---|---|---|
| COARSE, FINE, DETUNE | pitch | the grain rate, which is the pitch; FRMT doesn't move | unused (dimmed) |
| LEVEL, VEL, AR … RR, RS | as today | as today | as today |
| FEEDBACK | into its phase | into the carrier phase only (§ 3) | unused (dimmed) |
| incoming links (PM) | into its phase | grain time and carrier phase (§ 3) | into the centre's phase |

- **COARSE, FINE and DETUNE tune the grain clock, never FRMT.** A formant is a property of the vocal tract, not of the harmonic. Two FRMT operators on the same slot, DETUNEd apart, keep one formant with beating harmonics under it: a choir's width.
- **Feedback goes into the carrier only.** Fed into the grain clock, it would make the pitch depend on the operator's own output. Fed into the carrier, it skews the sine inside each grain toward a saw, adding formants at 2·FC and 3·FC while the pitch stays locked: a buzzier formant.
  - Feedback reads the operator's peak-referenced signal (§ 3), so FDBK means the same on FRMT as on a sine, at any BW.
- **NOIS has no pitch clock and ignores FEEDBACK.** Feeding noise back into its own centre only adds noise, and leaving it out keeps the bill honest (§ 9).

## 3. The voiced operator (FRMT)

### Grain clock and lanes

- **Grain clock.** The operator's existing phase accumulator runs at `f0 = op_cycles(...)`: the note times COARSE/FINE, times DETUNE.
  - A grain starts on each wrap of the **unmodulated** accumulator (`phase += inc` carries past 2^32).
  - So grain starts are exact and never depend on PM (review C2).
- **Two lanes, A and B, always alternating.** Each holds a window phase `w` and a carrier phase `c`, and a grain start restarts the lane that fired less recently. So each lane restarts every two periods.
  - There is no one-lane mode and no mode switch, so no BW change can cut a grain (review I4).
  - Each lane is computed only while its window is open. When the window is shorter than a period, at most one lane runs at a time.
- **Sub-sample starts.** At a start the lane begins where the wrap put it:
  - `elapsed = phase / inc` samples, from a per-block reciprocal;
  - `w = elapsed · r_w`;
  - `c = c0 + elapsed · inc_c`.
  - This is exact, because the clock is unmodulated, so the harmonics are exact multiples of f0.
- **First grain at note-on.** `Kernel::reset` fires a grain at once, so a note doesn't wait a period (31 ms at C1) for its first grain.
  - The reset grain takes no base delay: its delay is `clamp(pm, 0, s)`, not `s/2 + pm`. So it opens at once at every pitch, C1 included (amended 2026-09-30, controller ruling).

### Window law

- The window is `W(x) = (4x(1 − x))^q` for `0 ≤ x ≤ 1` and 0 outside, with `q = 2^(SKIRT+1)`: 2, 4, 8 or 16.
  - It is computed by clamping `x` to [0, 1], forming `4x(1 − x)` (which is at most 1, so there are no denormals on the host), then squaring SKIRT + 1 times.
  - `w` itself needs no saturation, because a lane restarts every two periods.
- **Why the law doubles the exponent per step:** that is the FS1R's measured skirt law, credited to FSVR's docs (§ Provenance).
- **Why the polynomial, not the patent's `sin^p`:** both are the same bell family. Both behave as `x^q` at the ends, so the skirts fall at the same rate, about 6(q + 1) dB per octave, and both tend to a Gaussian as q grows. The polynomial needs no table read, and its energy has a closed form. The polynomial is ours.
- **The window is symmetric, and the carrier sits centred on it.** So every grain is odd-symmetric and carries no DC at any FRMT. The FS1R's asymmetric formant window would put DC on low formant carriers.

SKIRT's constants, measured on the host and pinned by tests:

| SKIRT | q | Span `K` (−3 dB full width × window length) | Energy `c_q` (mean of `W²`) | First sidelobe | −60 dB extent `m_q` (× BW_eff) |
|---|---|---|---|---|---|
| 0 | 2 | 1.375 | 0.40635 | −28 dB | 5.49 |
| 1 | 4 | 1.734 | 0.29954 | −38 dB | 2.62 |
| 2 | 8 | 2.291 | 0.21653 | −56 dB | 2.07 |
| 3 | 16 | 3.121 | 0.15486 | −87 dB | 1.97 |

`c_q = 4^(2q) · ((2q)!)² / (4q + 1)!`. `K` and `m_q` are solved numerically from the window's transform.

### Bandwidth and the width floor

- **The width floor.** Two lanes hold a window of at most two periods. To keep SKIRT a pure shape knob at every note (review I3), the floor is set by the longest-span SKIRT, and is the same at every SKIRT:

  ```
  B_min  = K_3 · f0 / 2          (1.56 · f0)
  BW_eff = hypot(BW, B_min)
  L      = K_q / BW_eff          (the window's length in seconds)
  r_w    = BW_eff / (K_q · sr)   (cycles per sample)
  ```

- **What the floor gives:**
  - `BW_eff` is the formant's −3 dB full width at every SKIRT. SKIRT changes only how steeply the skirts fall.
  - The window never exceeds two periods: `D = L · f0 ≤ 2 K_q / K_3`, which is 0.88, 1.11, 1.47 and 2.0 for SKIRT 0–3.
  - `hypot` makes BW's whole travel live and smooth. Every step widens the formant, gently below the floor and exactly above it. A hard cap would leave up to half of BW's range dead.
- **Why the floor costs no musical range:** a formant narrower than about 1.5 harmonic spacings can't be heard as narrower. It just selects the nearest harmonic.
  - At G2 the floor is 153 Hz, and at A3 343 Hz.
  - Below about A2, BW sets the width. Above it, the floor does.
- BW is read every block, but it takes effect **grain by grain** (amended 2026-09-30, controller ruling, after the plan review's simulations).
  - Each lane latches `1/D`, the slack and the carrier's start at its restart.
  - Its window is read from the clock: `x = (t − o)/D`, where `t` is the periods since the lane's start (the clock's fraction, plus 1 for the older lane).
  - A restart comes exactly two clock periods later, so the old grain has closed by construction under any BW, pitch or PM change. Each grain is whole, so a BW change never clicks.

### Carrier

- The carrier is the sine table W1 at mip 0. It is already in DTCM (rendered there at boot, ADR 0122), so no new table is needed.
- A lane restarts its carrier at `c0 = −inc_c / (2 · r_w)` cycles, which puts the sine's zero crossing at the window's centre.
- The carrier frequency `FC` is read every block. The carrier phase is continuous, so a change never clicks.

### Phase modulation

- **Carrier phase.** The links' PM adds to the carrier phase, as it does to any operator. The result is a copy of the formant at each sideband (patent claims 1–2).
- **Grain time.** The same PM also moves each grain in time, by a bounded delay (claim 1: sidebands around the harmonics). Slack is the time a lane is free between windows, and the delay is taken within it:

  ```
  s  = 2 − D                          the slack, in periods
  o  = clamp(s/2 + pm, 0, s)          the delay, in periods
  x  = w − o / D                      the window read
  c' = c + pm − o · (FC / f0)         the carrier read, shifted with its window
  ```

  - A grain can only be delayed, never advanced, and never by more than the slack. So at every restart the old grain has already closed and the new one hasn't yet opened: **no grain is ever cut, at any PM depth** (review C2).
  - Window and carrier shift together, so the grain stays odd and DC-free.
  - With no PM the delay is a constant s/2, which is inaudible.
  - When D is near 2 (SKIRT 3 on the floor) the slack is near 0, so PM moves the carrier only.
- **Feedback** goes to the carrier phase only (§ 2).

### Frequency: FRMT, VOWEL and KEY

```
log2 FC = log2 FRMT_Hz + Δ_slot(VOWEL) + KEY · (pitch − 60) / 12
```

- `Δ_slot` is the vowel offset of the operator's latched slot (§ 5). It is 0 with no slot.
- `pitch` is the voice's pitch in semitones: note + TRNSP + the voice pitch offset (ADR 0042), the value `AlgoEngine::cycles` already uses.
  - KEY follows the note, not COARSE, FINE or DETUNE (§ 2).
  - KEY includes bends and vibrato, so at KEY 100 the formant keeps its place among the harmonics.
- **KEY_REF is MIDI 60 (C4)**, the middle of the singing range. So FRMT is the Hz heard at middle C, at any KEY.
- The KEY amount is latched at note-on. The pitch it follows moves per block, as every operator's pitch does.
- FC's floor is 20 Hz.

### Level: the carrier signal and the modulator signal

The operator produces one raw grain signal, `y = Σ lanes W(x) · sin(c')`, times the envelope and LEVEL's gain. It is used two ways (review I2).

- **As a modulator, and for feedback, it is peak-referenced like a sine.**
  - `y · g_m`, with `g_m = 1/p̂`: the raw signal scaled to a peak of 1.
  - `p̂ = min(1, p_q(FC·L) · (D > 1 ? 2 : 1))`, where `p_q` is the single-lane peak of `W · sin`.
  - So a modulator's depth, and FDBK's swing, don't change with BW or SKIRT.
- **As a carrier it is RMS-normalised, with a peak ceiling.**
  - `y · g_c`, with `g_c = min(R · g_rms, 1/p̂)`.
  - The exact RMS gain is `g_rms = 1 / sqrt(D · c_q · max(1 − ρ_q(2·FC·L), 0.05))`, where `ρ_q` is W²'s normalised transform. The bracket is the odd grain's exact energy: its negative-frequency image cancels part of it (review C1).
  - **The reference is `R = −12 dB`, relative to a full-scale sine's RMS.** A formant operator at LEVEL 99 has the RMS of a sine at LEVEL 83.
  - **The owner accepted this (2026-09-30):** clean peaks, and loudness through ADR 0058's reference (ADR 0063's today, at ADR 0109's gain).
  - **Why −12 dB:** a grain train is peaky. Its crest runs 4–15 dB across the vowel range. At equal RMS to a sine it would peak up to +12 dB over a sine and break the factory and sanity tests' peak of 1.0. So the reference is set so the ceiling `1/p̂` binds only past a 15 dB crest.
  - **The peak is at most 1.0 by construction**, like a sine's.
- **How the gains are applied.** `g_m` is folded into the operator's gain ramp, and `g_c / g_m` into its carrier-weight ramp. Both are computed per block and ramped, so they cost nothing per sample and never step.
- **The curves (amended 2026-09-30, controller ruling).** Both are flash tables per SKIRT, about 1.5 KB, read per block, never per sample.
  - `ρ_q` runs over ν = 2·FC·L in [0, 12] and is 0 past it; ρ₁₆(12) is under 1e-4.
  - `p_q` runs over FC·L in [0, 16] and is **1.0** past it, not flat. `p` tends to 1, so p̂ never underestimates the peak.

**Level held, measured by a host probe of the exact one-period synthesis:**
- Across the level-held domain the level matches R within 0.01 dB at every SKIRT:
  - C3 with FC 1 kHz and BW up to 2 kHz;
  - C4 with BW up to 4 kHz;
  - C2 with BW up to 500 Hz;
  - C5 with FC 270–300 Hz, where the first draft lost 3–5 dB.
- The peak stays under 1.0 there (0.3–0.92).
- **Past the level-held domain**, where wide BW meets low notes, the ceiling holds the peak at 1.0 and the level falls by the excess crest: −2.2 dB at C2 with BW 2 kHz, and −5.9 dB at C3 with BW 10 kHz.
- **Two small effects remain:**
  - Where D > 1 (SKIRT ≥ 1 near the floor), overlapping lanes add a ripple as FC moves against the harmonics. It is about ±0.5 dB at SKIRT 1 and negligible above.
  - When FC falls under about BW/4, the bracket's 0.05 floor binds and the level falls. The formant is then mostly cancelled at 0 Hz.

### Band-limiting

- **Timing.** The harmonics are exact, because grain starts are exact.
- **Where aliasing can come from.** Only energy past Nyquist can alias: the formant's skirts. The window and its first q − 1 derivatives are zero at every restart, so a restart adds no edge.
- **The rule uses the skirt's real reach** (review I5). The formant's −60 dB upper extent, `FC + m_q · BW_eff`, is kept under 24 kHz:
  1. `BW_eff` is narrowed to at most `(24 kHz − FC) / m_q`, but never below `B_min`, so a band narrows rather than folds.
  2. When even `B_min` doesn't fit, the gain fades linearly to 0 as the room `(24 kHz − FC) / m_q` falls from `B_min` to `B_min / 2`, and FC is clamped there.
  - At SKIRT 0 with BW 10 kHz, a formant at 8 kHz narrows to 2.9 kHz, and its folded skirts stay under −60 dB.
- **Downstream mips.** A table operator's mip still reads `incoming()`'s estimate. That is the same approximation today's high-ratio modulators get, and it is left alone.

## 4. The unvoiced operator (NOIS)

```
n   = xorshift32 → f32 in [−1, 1)
x   = k_n · n                                    scaled before the poles
y1 += c·(x − y1);  y2 += c·(y1 − y2)             two identical one-poles: y2 has unit RMS
out = g_n · (α · clamp(y2, −3.8, 3.8) + β) · sin(2π·FC·t + pm)
```

- **Band.** The two low-passes are ring-modulated up to the centre, which gives a band symmetric about FC.
  - Their corner is `f_lp = 0.777 · WIDTH`, so the band's −3 dB full width is WIDTH: two poles in series are −3 dB at 0.6436 · f_lp, doubled by the ring modulation.
  - The coefficient is `c = 1 − e^(−2π f_lp / sr)`, per block by `exp2`. It stays in (0, 1], so the filter is stable at any WIDTH.
  - The digital one-poles hold this width law up to about WIDTH 4 kHz, and above it the band grows a little wider than WIDTH. Tests stop at 4 kHz.
- **Scaling before the poles** (review C3).
  - `k_n = √3 / (c² · sqrt((1 + a²)/(1 − a²)³))`, with `a = 1 − c`, is the two-pole noise gain and the uniform source's variance, 1/3. It gives y2 unit RMS in steady state.
  - It is applied to the poles' input and ramped across the block, so a WIDTH jump rescales only new noise. The energy already in the poles decays at the old level instead of being amplified: a WIDTH step from 4 kHz to 20 Hz peaks at about +7 dB over 10 ms, against +27 dB when scaled after.
- **RES** is our own continuous take; the FS1R's is a threshold.
  - `R = RES/100`, `α = sqrt(1 − R)` and `β = sqrt(R)`. β is a DC term, and DC through the ring modulator is a pure tone at the centre, so RES blends band noise into a tone at FC.
  - `α² + β² = 1`, so the RMS is the same at every RES and every WIDTH. RES is latched at note-on.
- **Level and peak.** `g_n = 0.251`, the same −12 dB reference as FRMT (§ 3). The clamp at 3.8 σ bounds the peak: `g_n · (3.8 α + β) ≤ 0.251 · √(3.8² + 1) = 0.99`.
  - It clips only tails past 3.8 σ, about 1 sample in 7,000, which moves the RMS by under 0.01 dB.
  - The modulator signal is the same, so NOIS as a modulator swings at most 0.99 × its weight.
- **Centre.** `FC` follows FRMT's law with VOWEL (by its slot) and no KEY: the owner's NOIS cells have no KEY. The centre sine is W1 at mip 0, and links add PM to its phase.
- **Edges.**
  - The band's upper edge, `FC + WIDTH/2`, fades the gain linearly from 1 at 16 kHz to 0 at 20 kHz. Folded noise is still noise, so the −3 dB edge is enough here.
  - A centre below WIDTH/2 folds the band through 0 Hz, which gives a low rumble. The noise and the sine carry no DC, so neither does the fold.
- **Seed.** Each operator's xorshift state is seeded once, when the engine slot is built, from the voice index and the operator number. It is never reset at note-on, so repeated notes don't repeat their noise, and renders stay reproducible from a given start.

## 5. The VOWEL macro

### The table

The table is in path order, from dark to bright round the vowel triangle:

| Vowel | Position `x` (stored) | As in | F1 | F2 | F3 |
|---|---|---|---|---|---|
| U | 0 (0) | /u/ *who'd* | 300 | 870 | 2240 |
| O | 1 (25) | /ɔ/ *hawed* | 570 | 840 | 2410 |
| A | 2 (50) | /ɑ/ *hod* | 730 | 1090 | 2440 |
| E | 3 (75) | /ɛ/ *head* | 530 | 1840 | 2480 |
| I | 4 (100) | /i/ *heed* | 270 | 2290 | 3010 |

- **Source.** Peterson & Barney (1952), the averages for adult male speakers, in Hz. These are measurements, facts anyone may use, and this table is our transcription.
- **O is /ɔ/,** the one back rounded mid vowel P&B measured as a monophthong.
- **Why the male averages:** VOWEL moves formants by ratios (below), so a speaker of any size is just FRMT's offset. Raising every FRMT by about 17 % gives a female-like voice without a second table.
- **Why the owner's path:** U–O–A–E–I is the triangle's edge. Every neighbour pair is a real glide, "wow" (U–A–U) and "yeah" (I–E–A) are single moves, and a full sweep never jumps across the triangle. A–E–I–O–U's I→O step drops F2 by 1.45 octaves.

### Interpolation and mapping

- `x = v / 25`, after modulation, in [0, 4].
- `Δ_s(x) = lerp(log2 T_s[i], log2 T_s[i+1], x − i) − log2 T_s[A]`, for slot s ∈ {F1, F2, F3}.
  - It is stored as a constant table of octave offsets, which a test recomputes from the Hz table.
- **Why log, relative to A:** formant perception is close to logarithmic, and an operator's FRMT offset is a ratio, so the offset holds at every vowel.
  - At VOWEL A, the default, every Δ is 0. So FRMT is exactly the Hz shown, and a patch that never touches VOWEL plays FRMT as absolute Hz.
- **"Offsets from its vowel position."** An operator's frequency is its vowel position `T_s(x)` times the ratio `FRMT / T_s(A)`. So FRMT sets where the operator sits relative to the vowel, and VOWEL moves every slotted operator together.
- **Cost.** The segment and fraction are computed once per voice per block. Each slotted operator then does one lerp, which feeds the per-block `exp2` FC already needs.
- **Stored steps.** VOWEL is stored in 25 steps a vowel. A MIDI-mapped CC (#247) moves in those steps: up to about 60 cents of F2 a step on E–I. That is borderline stair-stepping, so it is a question for #247's easing. Modulation is unrounded.

## 6. Pages

**Plain FM patches look as they do today,** with one exception: VOWEL's dimmed cell in what was the ALGO home page's empty sixth slot.
- It is dimmed and inert, exactly as MORPH is when ALG A = ALG B (ADR 0049).
- A dimmed cell shows the feature exists without being in the way.
- The ALGO home screen golden is re-recorded once for it. No other page or cell of a plain patch changes.

### ALGO home

- The cells are ALG A, ALG B, MORPH, TRNSP, VOL and **VOWEL**. `ALGO_ALG.params[5]` is `EMPTY` on main, so VOWEL takes it.
- `view::dimmed` gains `(Algo, VOWEL) => no operator is a formant kind`, beside its MORPH arm.

### OSC sub-row: dynamic, and focus by identity

- **Two sub-rows.** `ALGO_CHAIN` gets a twin, `ALGO_FORMANT_CHAIN`. The two are identical except the OSC block's sub-pages: today's list, and the same list with **FORMANT** first.
  - FORMANT is a `group(...)` page of `AlgoOpParams::FRMT` across OP1–OP6, named "Formant", short "FRM". Map pills are 34 px, so the map keeps the short, as every group page does.
- **The chain is picked per Sound** (ADR 0044's ladder).
  - `nav_cx` builds `NavCtx` from each Part's Sound. Today it carries `engines: [EngineType; MAX_PARTS]`, and `nav.rs` reads a chain through `chain_def_for(engine)`.
  - It carries a `PartChain` per Part instead, `PartChain::of(&Sound)`: the formant chain when any operator is a formant kind.
  - `chain_def_for(engine)` stays as each engine's base chain, for the browser and the tests.
- **Focus by identity** (review I8). A page is kept by `BlockDef.id`, not by its `PageAt` index. When a Part's list changes, `refocus` maps the `PageAt` to the same page's new place.
  - The list changes on a WAVE edit, a project load (`UiState::project_replaced`), or `Recall`'s stored page (`Recall::pages_of`, which today goes home only on an engine change).
  - If the page left the list, the focus moves to the OSC home.
  - So inserting FORMANT first never lands the player on the wrong page. There is no undo on main.
- **Why FORMANT first:** it is what a formant patch is edited by most, and it appears as the player sets a WAVE to FRMT or NOISE on the OSC home, so one EDIT lands on it.
- **Cells.**
  - On the FORMANT page, a table operator's cell is dimmed (`(AlgoOp(n), FRMT) => kind is Table`).
  - A NOISE operator's cell shows its centre.
- The all-pages walk includes the FORMANT page exactly when a formant operator exists.

### Operator pages (prerequisite phase)

- **Where they live (amended 2026-09-30, owner).** OP1 … OP6 are sub-pages of the OSC node, after its twelve group pages, which stay unchanged: CRS, FIN, DET, LVL, VEL, AR, D1R, D1L, D2R, RR, RS and FBK.
  - There is no envelope-page merge. That is the controller's default, which the owner may revisit.
  - EDIT and SEQ step through them as on every page. EDIT wraps from OP6 back to OSC (ADR 0081).
  - There is no new gesture: B*n* keeps meaning Part *n*, and #37's hold-B1–B6 is dropped.
- **Dependency.** No operator page is built on main at 4473f0a1, and BW, SKIRT, KEY, RES and SLOT have no page without one.
  - The OP sub-pages land first, as their own branch (Phase 0), with the dynamic sub-row and focus by identity. This spec builds on that branch.
  - Gang edit, LEVEL SCALE and the living diagram stay in #37.
- **Cells by kind.** Cells come from a pure lookup keyed on the operator's kind, `op_page::page_cells(kind)`. It is resolved in `view::view` through a new `SlotBinding::OpPanel(op, k)`, as MDL2's cells are through `SlotBinding::ModalPanel` and `modal::page_cells`. Labels follow the full-word rule (62 px):

  | Kind | Cells |
  |---|---|
  | Table | WAVE, COARSE, FINE, LEVEL, FDBK, VELOCITY |
  | FRMT | WAVE, FORMANT, BW, SKIRT, KEY, SLOT |
  | NOISE | WAVE, CENTRE, WIDTH, RES, SLOT, LEVEL |

  - COARSE (50 px) and VELOCITY (59) fit. FEEDBACK (65) doesn't, so the cell reads the spec's FDBK.
  - FRMT's COARSE, FINE, LEVEL, FDBK and VELOCITY stay on their group pages.
  - Only the OP pages of formant operators swap in formant cells (amended 2026-09-30, owner). A table operator's OP page is always the Table row.
- **Viz band.**
  - A FRMT page draws one grain (window times carrier) at the live FC and BW_eff, and prints both. So the page shows what VOWEL, KEY and the width floor are doing, even when the cell's stored Hz differs.
  - A NOISE page draws its band as a bell over a log-frequency axis at the live centre.
  - The envelope beside it is today's `viz::op_env`, and the table operator's wave is `glyph::wave_trace`.
  - Everything is drawn in theme tokens only (`ui::theme`). INVERT maps them (ADR 0126), and `light_golden_test` fails on any other colour.

### Dimming on the group pages

- COARSE, FINE, DETUNE and FDBK cells are dimmed for NOISE operators, through `view::dimmed`.
- One pure `kinds::reads(kind, id) -> bool` decides, as `modal::reads` and `tape_engine::reads` do for their pages.

## 7. Modulation

- **New destinations.** VOWEL (`Algo`, id 4), FRMT (`AlgoOp(n)`, id 13) and BW (id 14) are `modulatable` under ADR 0010, because the engine reads them every block.
  - `AlgoLive` (`chimera-engine/src/dsp/algo/engine.rs`) holds today `morph`, `level`, `patch`, `fine`, `detune`, `feedback`, `velocity`, `routed` and `morph_routed`. It gains `vowel: f32`, `frmt: [f32; OPS]` and `bw: [f32; OPS]`.
  - `AlgoLive::slot` gains the three arms, so the Part's knobs glide them (ADR 0112).
  - `AlgoLive::offset` gains them too, so routes reach them, with ADR 0010's formula unchanged.
  - They are registry columns: the matrix has 16 of them beside its four fixed columns (FREQ, Q, VCA, FOLD, ADR 0096).
- **Routes to non-formant operators.** A route into FRMT or BW on a table operator is allowed and does nothing, like a LEVEL route to an operator outside the algorithm. The matrix already shows such columns as inert, through `view::dimmed`.
- **Not modulatable:**
  - SKIRT, because an exponent change mid-grain would jump the window;
  - KEY, RES and VSLOT, by the owner's note-on rule.
  - Each is latched at note-on, per operator, beside the envelope coefficients.
- **Clicks.** VOWEL, FRMT, BW and WIDTH sweeps and jumps can't click or burst:
  - carrier and window phases are continuous;
  - every gain ramps;
  - NOIS scales before its poles.
  - § Tests covers both square-LFO jumps and sweeps.
- **External control.** There is no new MIDI CC mod source (owner, 2026-09-30). VOWEL, FRMT and BW, like any parameter, are mappable through the per-Part MIDI map, #247:
  - a CC drives a parameter over a range;
  - the map is saved with the Part, assigned by MIDI learn, and listed on a MIDI MAP page.
  - This project builds nothing for it.
- **ALGO taps (ADR 0096).** A formant operator is a tap source like any operator. Its tap row takes the peak-referenced signal (§ 3), clamped to ±1 as every row is.
- **Supersedes in part:** ADR 0022's rule that "MORPH and the six LEVELs are the destinations", now extended by these three.

## 8. Engine and kernel

- **Per-block dispatch.** The kernel picks each operator's path once per block, by kind:
  - `render_ops` calls `run_one::<N>` for a table operator, as today;
  - `run_formant::<N, S>` for FRMT, specialised on SKIRT (four variants), so the squarings are straight-line code;
  - `run_noise::<N>` for NOIS.
  - `run_pair` pairs only two table operators.
  - `render_samples` matches on the kind in `step`. The branch is the same every sample, so the predictor carries it.
  - Each path writes its operator's block into the row `render_ops` keeps for the links and the taps (`bufs`, then `TapRows`), as `run_one` does.
- **Formant state.** Per operator, `Kernel` holds a plain enum, with no `unsafe`:

  ```rust
  enum KindState {   // kernel.rs already has a lane struct named `OpState`
      Table,
      Formant { w: [f32; 2], c: [u32; 2], older: u8 },
      Noise { rng: u32, y1: f32, y2: f32 },
  }
  ```

  - The per-block `OpBlock` gains the kind's increments, ramps and gains. Its `lo`/`hi` table pair is unused by these kinds.
- **Swap.** A kind change is a WAVE code change, so `Swap::Ducking` (its `waves` mask) ducks the operator over one block and swaps it, as today.
  - `Kernel::reset(op)` becomes `reset(op, kind)`.
  - The new kind's state starts closed: a FRMT's lanes at `w = 1` with its first grain fired; a NOIS's poles at 0, keeping its seed.
- **Idle.** A silent formant operator advances its grain clock and envelope, as `idle` does today.
- **Glides.** `glide_on` and `glide_to` (#254) move the pitch, so they move the grain clock like any operator's. FC follows only through KEY.
- **In-place build.** `AlgoEngine::init_in_place` writes every new field, and `field_list!` fails to compile until it does.
- **Seeding.** `EngineSlot::build`'s Algo arm seeds the noise from its `VoiceIdx`, as VA's `place` and TAPE's `seed` do.
- **Unchanged:**
  - the mip machinery (`mip_target`, `mip_step`) is skipped for these kinds;
  - MORPH, `carrier_norm`, the plan, VELOCITY and the envelopes are untouched;
  - a formant carrier adds to the output like any carrier, through its carrier-weight ramp.

## 9. Cost and memory

### Method

- This is ADR 0056's method: the sine operator's benched `COST_OP` (60), plus the hot path's added instructions at 1.46 cycles an instruction, plus 10 %, rounded up to 10.
- Per-block work is spread over the block's 64 samples.
- The counts are estimates from the planned code paths. The plan recounts them from the `bench-algo` build with `tools/loop_count.py` (ADR 0056's count, automated) once the code exists, and the chip bench confirms them at ship.
- The M7 is in-order, so the squaring chain's dependent latency may cost more than its count. The bench decides.

| Path | Added instructions a sample (vs a sine operator) |
|---|---|
| The second mip read and its crossfade, dropped | −13 |
| Grain clock's wrap test (unmodulated) | +2 |
| The grain delay: `o = clamp(s/2 + pm, 0, s)`, and the `s` and `1/D` ramps | +5 |
| One lane: window read `w − o/D` and its clamp, `4x(1 − x)`, four squarings (SKIRT 3), carrier phase, PM and delay offset, product, open-flag test | +16 |
| The second lane when open: the same, plus its own carrier read, fused into the product | +30 |
| FEEDBACK kept apart for the carrier | +1 |
| Per block: three divides, two `exp2`, an `inv_sqrt`, two curve reads, the edge law, over 64 | about 2 cycles |
| **FRMT, SKIRT 0** (D ≤ 0.88, so one lane open at a time) | +11 → 60 + 17.7 + 2 = 80 → **90** |
| **FRMT, SKIRT 1–3** (D can exceed 1, so both lanes open) | +41 → 60 + 65.8 + 2 = 128 → **130** |
| **NOIS:** the dropped read (−13); xorshift 3, convert 2, the `k_n` ramp and scale 2, two poles 4, the clamp 2, + β 1, product 1; per block about 1 cycle | +2 → 60 + 3.2 + 1 = 64 → **70** |

### Terms

- `COST_FRMT_ONE = 90`, for SKIRT 0. With the floor, SKIRT 0's window is at most 0.88 periods, so the two lanes never sound at once.
- `COST_FRMT = 130`, for SKIRT 1–3, where the lanes can overlap.
  - SKIRT is stored and latched at note-on, so this is billed from the patch's shape (ADR 0026), not per note.
- `COST_NOIS = 70`.
- **Against the owner's estimates:** they were 75–110 and about 55. The revision adds the grain delay (C2), the peak clamp and the pre-pole scaling (I2, C3), which the first draft's 110 and 60 didn't carry.
- **Billing.** `AlgoEngine::cost` prices each active operator by its kind and SKIRT: `COST_OP`, `COST_FRMT_ONE`, `COST_FRMT` or `COST_NOIS`.
  - `COST_FEEDBACK` is billed for table and FRMT operators with FEEDBACK above 0, never for NOIS.
  - Links into any active operator are billed `COST_LINK`, as today.

Voices beside the whole FX bus, per ADR 0031's arithmetic, without mod routes (refreshed 2026-10-11):
- rev V's budget is 7,500 at 75 % (7,000 at main's 70 %), and rev Y's 6,250 (5,833);
- the bus is 1,204, or 1,494 with the tape;
- a voice is the engine's cost + 83 (`CHAIN_COST` 36 + `ModRouting::BASE` 47).

The counts are at 75 %; brackets show 70 % where it differs. Rows with three FRMT need N = 3, so they need the 75 % share and the 120 target.

| Patch | Engine cost (estimates) | Rev V | Rev Y |
|---|---|---|---|
| Any plain FM patch | unchanged | unchanged | unchanged |
| Two NOIS, A1 (BREATH) | 510 | 8 | 8 [7] |
| One sine + three FRMT at SKIRT 1, A1 (TALK BASS) | 820 (790 at the target) | 6 | 5 |
| Three FRMT + one NOIS, A1 (CHOIR) | 830 (800 at the target) | 6 | 5 |
| Two FRMT + four NOIS, A1 | 910 | 6 [5] | 5 [4] |
| Three FRMT at the target + one NOIS + two table, all with feedback where billed, A16 ∪ A17 (**the worst allowed** at N = 3) | 1,021 | 5 | 4 |

**The worst allowed patch under the floor's worst case** (heavy routes, § 1): its voice bills 1,021 + 472 = 1,493. By construction it is never more than ⌊(budget − 1,494) / 4⌋.

| At 75 % | Rev V | Rev Y |
|---|---|---|
| With the tape | (7,500 − 1,494) / 1,493 = 4.02 → **4** | (6,250 − 1,494) / 1,493 = 3.2 → 3 |
| Without the tape | (7,500 − 1,204) / 1,493 = 4.2 → **4** | 3 |

- ADR 0026's floor holds for every patch that can be built. The owner's floor is rev V's. On rev Y it is 3, as it is for the costliest table patch under heavy routes: (5,833 − 1,494) / 1,304 = 3.3.
- **The convention is ADR 0026's.** The floor was never promised under every route at once plus FOLD, DRIVE or ALGO taps, nor while ORBIT plays (ADR 0091's reserve of 1,028). The heavy-route case above is the floor's defined worst.
- ADR 0031's "the costliest patch gets six voices on rev V" still holds without routes. The worst allowed formant patch bills 1,021 + 83 = 1,104, which gives 5 voices, and the costliest table patch (915 with `ModRouting::BASE`) still gets its six (`the_costliest_patch_gets_six_voices_on_rev_v`).

### Memory

| Item | Size |
|---|---|
| Per operator: `KindState` (20 B), latched SKIRT and slot (2 B), KEY-or-RES (4 B) | 26 B |
| `kinds: [OpKind; 6]` over `waves: [WaveId; 6]` | +6 B |
| **Per voice** | **about 160 B** |

- `AlgoEngine` grows from 700 B to about 860 B (host, main at 4473f0a1). The owner's estimate was about 100 B.
- The `EngineSlot` (4,248 B) is sized by `ModalEngine` (4,240 B), so `Voice` (6,304 B) and the pool don't grow. `voice_is_its_chain_plus_one_slot` pins that.
- No new wave table, and DTCM is untouched: both kinds read W1's mip 0, which is already there.
- The new constants are all in flash and read per block: the SKIRT arrays (80 B), the `ρ_q` and `p_q` curves (about 1.5 KB), and the vowel table (60 B).
- **Flash is now the tight resource** (refresh 2026-10-11). About 58 KB is free on main at 4473f0a1, in the 917,504 B firmware area.
  - Today `Kernel::render_ops`, with its 15 inlined `run_one` and `run_pair` variants, is 14.5 KB. So 30 more variants (`run_formant` 6 × 4, `run_noise` 6) could cost 15–25 KB.
  - The plan measures them at Task 10. If they pass 10 KB, it specialises `run_formant` on SKIRT only, with the link count a runtime loop (4 variants).
  - Free flash must stay above the owner's 40 KB line.
- The bench checks the I-cache: a formant operator's block touches one variant.

### Bench rows (ship flash)

| Row | Patch | Tests |
|---|---|---|
| FRMT0 | one FRMT operator, SKIRT 0, BW 20 Hz | `COST_FRMT_ONE` |
| FRMT3 | one FRMT operator, SKIRT 3, BW 20 Hz | both lanes overlapping |
| FRMT PM | FRMT3 with a full-level ratio-1 modulator | the delay path |
| NOIS | one NOIS operator | `COST_NOIS` |
| FRMT MAX | the worst allowed patch (§ 9) | the budget's worst case |
| CHOIR | the CHOIR factory Sound | a real patch |

The rows join the `bench-algo` build, one bench area a build. Claude flashes it and reads the rows over the USB console (`just flash-bench algo`, `just usb bench`).

## 10. Saving and compatibility

- **Frozen codes (ADR 0045),** all appended to `disk_codes_v1.txt`:
  - WAVE codes 16 FRMT and 17 NOIS;
  - operator param ids 13–18, with idents `FRMT`, `BW`, `SKIRT`, `KEY`, `RES` and `VSLOT`;
  - VSLOT's codes 0–4, with idents `AUTO`, `F1`, `F2`, `F3` and `OFF`;
  - Algo param id 4, `VOWEL`.
- **How old patches decode** (review I7). A file decodes onto `Sound::neutral` (`chimera-engine/src/storage/sound.rs`): `ParamSnapshot::for_engine`, which is `AlgoParams::default()` (INIT), with OUT VOLUME 0.8 and RR 8. Then the file's own values are applied. An old file carries none of the new ids, so its new fields take INIT's values:
  - FRMT 69, 83, 111, 123, 123 and 123;
  - BW 72, SKIRT 1, KEY 0, RES 0, VSLOT AUTO;
  - VOWEL 50.
  - Its WAVE codes are 0–15, so every operator is a table operator and those fields are inert.
  - It sounds bit for bit as before.
- **Neutral omission (ADR 0087).** `Sound::neutral` is a format contract: `encode_sound` omits every param at its neutral value.
  - So INIT's new values are frozen with the codes, and a Sound that leaves them alone saves no new bytes.
  - `neutral_v1.txt` gains the new fields at those values, and the kinds' new representation, with every existing value unchanged.
  - `v1_fixtures_render_identically` and `v1_fixture_bytes_are_frozen` pin the old files.
- **The Sound cap grows.** `MAX_SOUND_COST` (2,352 B, pinned by `sparse_bound`) counts every param off neutral at 5 B.
  - 37 new params (6 × 6 operator ids, plus VOWEL) take the worst case from 2,349 B to 2,534 B, so the cap becomes 2,544.
  - The pool's meter and refusal goldens move with it, as they did for VA.
- **The sound library (ADR 0119).** A formant Sound saved to the card and loaded by an older build reads as "newer".
  - Its unknown ids are skipped (`unknown_non_critical_skipped`), and WAVE 16 or 17 is refused by the old `set_enum_code`, so those operators stay W1.
  - It loads safely, not the same, which is the owner's rule for old builds.
- The file format and version don't change. New values are new ids in the existing `Block` records.

## Tests

Each test fails on the first draft's design or on a plausible bug, or it pins a constant against measurement.

| Test | Proves |
|---|---|
| `fc_law_is_exact` | The pure `fc_hz(frmt, vowel, slot, key, pitch)` matches the formula within 0.1 %: KEY 0, 50 and 100 at C3, C4 and C5 (KEY 50 at C5: 1,131 Hz), VOWEL at each vowel, and every slot. |
| `formant_peak_stays_put_across_pitch` | KEY 0, FRMT 1 kHz, BW 1 kHz, so grains are isolated: from C2 to C4 the carrier frequency, measured by zero-crossing intervals inside grains, is 1 kHz ±1 %. |
| `key_100_tracks_the_note` | KEY 100 and KEY 50, measured the same way at C3, C4 and C5, within 1 % of `fc_law`. |
| `level_is_normalised` | Within ±0.5 dB of R (−12 dB re a sine's RMS), at every SKIRT: C3 with FC 1 kHz and BW from 524 Hz to 2 kHz; C4 with BW from 1,047 Hz to 4 kHz; C5 with FC 270 and 300 Hz (the first draft's −3 to −5 dB case); and VOWEL swept across all five vowels at C5. |
| `peak_never_exceeds_one` | A lone FRMT carrier at LEVEL 99 across a grid of notes (C1–C7), BW, SKIRT, FRMT and VOWEL, including C2 with BW 10 kHz: every sample ≤ 1.0. Where the ceiling binds, the level equals the predicted `min(R·g_rms, 1/p̂)` within 0.5 dB. |
| `bw_does_not_change_modulation_depth` | A FRMT modulator at a fixed LEVEL into a sine: its peak output, and so the carrier's modulation index, varies under 0.5 dB as BW sweeps 20 Hz–10 kHz. So does FDBK's swing. |
| `width_floor_keeps_skirt_a_shape` | At A3 and C5 with BW 20 Hz, the measured −3 dB width is within 10 % of `BW_eff` at every SKIRT, and equal across SKIRTs within 10 %. Each BW step from 20 Hz up widens it. |
| `pm_never_cuts_a_grain` | Full-level ratio-1 and ratio-2 modulators into FRMT at SKIRT 0–3: the kernel's debug counter of restarts with an open window read stays 0, the click detector passes, and the peak stays ≤ 1.0. The first draft's modulated-wrap starts fail it at a PM of 1 cycle. |
| `bw_jumps_never_cut_a_grain` | A square LFO on BW across D = 1 at the note's pitch, at the worst phase: counter 0, click detector passes. |
| `harmonics_are_exact` | FRMT alone at A3: partials at k·f0 within 0.1 cent, and the floor between partials 60 dB under the peak. Sample-grid starts fail it. |
| `first_grain_sounds_at_note_on` | At C1 the first grain opens within the first block. |
| `high_formants_do_not_alias` | SKIRT 0, BW 10 kHz, FC 6, 8 and 10 kHz at C3: energy off the harmonic grid is under −60 dB re the total. The first draft's −3 dB-edge rule fails it. Also: a full-level ratio-1 modulator into a 5 kHz formant at C3 stays under −60 dB off the grid, excluding the PM sidebands' own grid. |
| `formant_edge_fades` | With the room `(24 kHz − FC)/m_q` at `B_min/2`, the operator is silent; at `0.75·B_min`, the gain is half. |
| `feedback_moves_the_formant_not_the_pitch` | FDBK 0 against 7 on FRMT: f0 unchanged within 0.1 cent, and energy near 2·FC rises. |
| `pm_enters_grain_and_carrier` | A sine modulator into FRMT puts sidebands around both the harmonics and the formant (the shape of the patent's Table 1). |
| `vowel_sweeps_do_not_click` | VOWEL swept 0→100 over 50 ms, and a square LFO jumping 0↔100, on FRMT and NOIS: the click detector passes. |
| `width_jumps_do_not_burst` | A square LFO on WIDTH, 4 kHz ↔ 20 Hz: the worst 10 ms RMS stays within +8 dB of steady state, and the peak ≤ 1.0. Scaling after the poles fails it (+27 dB). |
| `vowel_table_is_peterson_barney` | The octave offsets recompute from the Hz table. At A1 and each vowel's position, operators 1–3 at INIT's FRMTs sit on that vowel's F1–F3 within 50 cents. |
| `vowel_slots_resolve` | AUTO by order among AUTO operators (FRMT on operators 2, 4 and 5 gives F1, F2, F3; a fourth AUTO operator doesn't move). Two F1 operators both follow F1. OFF doesn't move. A slot edit while held moves nothing until the next note. |
| `noise_band_centre_and_width` | CENTRE 2 kHz, WIDTH 500 Hz, RES 0: the averaged spectrum peaks within 5 % of 2 kHz, with a −3 dB width within 15 % of 500 Hz. WIDTH up to 4 kHz. |
| `noise_level_is_normalised` | WIDTH 20 Hz–4 kHz and RES 0–100: RMS within ±0.5 dB of R. RES 100 is a tone at CENTRE. The peak is ≤ 1.0. |
| `formant_operators_are_stable` | Every new parameter at its ends, FDBK 7, full PM from five modulators, 30 s: finite, every carrier ≤ 1.0 before `carrier_norm`, \|DC\| < 1e-3, no growth. |
| `laws_are_pinned` | K, c_q, m_q, ρ_q and p_q against the window's measured transform and peaks, within 0.5 %. |
| `plain_fm_is_bit_identical` | Every existing Algo golden and factory render is unchanged. |
| `old_patches_load_unchanged` | Every stored v1 fixture decodes to the same params, including INIT's new fields (§ 10), and the same render hash. Beside `codec_compat_test`'s `v1_fixtures_render_identically` and `v1_fixture_bytes_are_frozen`, and `sound_codec_test`'s `neutral_is_a_contract` and `sparse_bound`. |
| `disk_codes_test` | The appended codes and idents in `disk_codes_v1.txt`. |
| `pages_adapt_to_formant_kinds` | The FORMANT page is present exactly when a formant operator exists. The focus is kept by `BlockDef.id` through a list change (on DETUNE, a WAVE edit or a project load that adds FRMT stays on DETUNE). VOWEL and the table operators' FRMT cells are dimmed, as are NOIS's COARSE, FINE, DETUNE and FDBK. Operator pages show each kind's cells. |
| `formant_costs_bill_by_kind` | Each term and the voice-count table above, in `cost_test`. |
| `formant_budget_keeps_four_voices` | `FORMANT_BUDGET` recomputes from the cost terms. Every allowed kind assignment (all 3⁶ over table/FRMT/NOIS, within budget), at every SKIRT, with feedback on every billed operator, on the costliest union (A16 ∪ A17) under the heavy routes, bills ≥ 4 voices on rev V, with and without the tape. The first refused assignment of each shape would bill 3. |
| `over_budget_kinds_cannot_be_built` | `try_set_kind` refuses the (N+1)th FRMT, and the NOIS past the budget beside N FRMT. A property test drives random writes through every path (UI encoder, `set_enum_code`, the decoder, the sweep): no `AlgoParams` ever exceeds the budget. The WAVE encoder steps over refused kinds and toasts. An over-budget file decodes its excess as W1. `Kinds::new` rejects an over-budget const at compile time. |
| memory | `voice_is_its_chain_plus_one_slot` still passes, and `Voice`'s size is unchanged. |

- **ADR 0010's modulatable test.** `modulatable_test::recipe` makes the destination audible. For `AlgoOp(n)`'s FRMT or BW, operator n's WAVE is set to FRMT (NOIS for a second case). For `Algo`'s VOWEL, operators 1–3 are FRMT on A1.
- **Param-sweep harness** (`chimera-core/tests/param_sweep_test.rs`, on main). It picks up the new registry parameters automatically.
  - It gets a formant fixture Sound: operators 1–3 FRMT, operator 4 NOIS, on A1, where each new parameter must move the output.
  - It plays at C3, not at KEY_REF, so KEY is heard.
- **Screen goldens.** The ALGO home goldens are re-recorded once, for the dimmed VOWEL cell: `engine_algo`, `algo_alg`, `algo_alg_morph_dimmed` and `algo_out_p3`.
  - The pool and library goldens move with `MAX_SOUND_COST` (§ 10).
  - New goldens cover the FORMANT page and a FRMT and a NOISE operator page.
  - `light_golden_test` checks every case's colours against the theme tokens (ADR 0126).

## Demo clips (owner UAT)

Three new factory Sounds. The clips render from them, so what the owner approves is what ships.

- **CHOIR:** a pad on A1, within the budget at N = 3 (3 FRMT at the 120 target and 1 NOIS: 3·60 + 10 = 190 ≤ 197 at 75 %; 6 voices on rev V). At N = 2 it takes the plan's two-formant voicing.
  - Operator 1 is FRMT on slot F1.
  - Operators 2 and 3 are FRMT on slot F2, DETUNE −2 and +2. F2 carries the vowel's identity, and its beating is the most audible, so doubling it gives the choir its width.
  - Operator 4 is NOIS on slot F3: breath and air on the third formant, which follows the vowel (WIDTH 3 kHz, low).
  - All the FRMT operators are at SKIRT 1.
  - Slow attack, VOWEL stored between O and A (37), and LFO 1 slowly across O–A.
- **TALK BASS:** A1.
  - Operator 1 is W1 at 0.50, the body.
  - Operators 2–4 are FRMT on AUTO (F1–F3).
  - VOWEL is stored at U (0), with ENV 2 into VOWEL reaching A, so each note goes U→A→U: "wow".
- **BREATH:** A1.
  - Operator 1 is NOIS (CENTRE 1.2 kHz, WIDTH 2.5 kHz).
  - Operator 2 is NOIS (CENTRE 5 kHz, WIDTH 4 kHz), lower.
  - Both are on slot OFF, with slow attack and release.
- **LEVELs** are set so `factory_test`'s peak ≤ 1.0 holds with `carrier_norm`.
  - Each Sound is measured against the loudness reference, ADR 0063's at ADR 0109's gain (`REFERENCE_LUFS` −11.0 in `param_sweep_test`). Equal RMS isn't equal loudness: formants at 2–4 kHz weigh 2–4 dB louder under K-weighting.
- **The factory bank.** It is `chimera-engine/src/factory.rs`'s `factory.bin` blob, checked against the `authored` Sounds (`blob_matches_authored`; `GOLDEN_RECORD=1` rewrites it).
  - The three Sounds append after the eight (`FACTORY_LEN` 8 to 11), before the INITs.
- **Rendering.** An `#[ignore]`d host test, `render_formant_demo_clips`, plays a short phrase of each Sound through the `Instrument` and writes 48 kHz WAVs to `target/demo/`, with no new dependency.
- The plan pins each Sound's voice count in `cost_test`.

## Provenance

- **From Yamaha's patent US5610354** (expired): the structure.
  - A window oscillator at the pitch, with a carrier reset at each window cycle.
  - Two π-offset window lanes with a two-period window.
  - PM into the window and carrier phases.
  - Noise low-passed, offset by DC and ring-modulated to a centre.
- **From FSVR's prose docs** (behaviour descriptions and measurements only; no code, tables or constants; GPL-3, ADR 0032):
  - the width is a time, so it is constant in Hz;
  - the formant holds still because the carrier restarts each grain;
  - the skirt law doubles the window exponent per step (`p = 2 · 2^skirt`), which SKIRT's `q` follows;
  - the noise band is a low-pass ring-modulated to the centre, with an absolute width in Hz.
- **From Peterson & Barney (1952):** the vowel formant averages, which are measurement data.
- **Ours:**
  - the polynomial window, and BW as a −3 dB width held across SKIRT;
  - the SKIRT-free width floor and its `hypot`;
  - the odd, centred carrier;
  - sub-sample starts on the unmodulated clock;
  - PM as a bounded grain delay;
  - the exact-energy level law, the −12 dB reference and the peak ceiling;
  - the peak-referenced modulator signal;
  - continuous RES, and pre-pole scaling;
  - carrier-only feedback;
  - the −60 dB edge law;
  - the VOWEL macro, its U–O–A–E–I path, its log interpolation and the VOWEL SLOT;
  - every range and unit.
- THIRD_PARTY.md doesn't change.

## ADR

The next free ADR number when it is written. On 2026-10-11 the highest on any branch is 0127 (`screensaver`); 0123 is reserved by `sound-sends` and 0124 is `audio-share-75`'s, 0128 is reserved by `midi-bend-sustain` and 0129 is `va-glyphs`' (PR #473), so expect 0131, after Phase 0's 0130: "Formant operators are Algo operator kinds, after the FS1R patent". It records:

- **Provenance:** the split above, including FSVR's measured skirt law, and FSVR's GPL status.
- **Kinds:** `OpKind`, the WAVE codes, and VOWEL SLOT.
- **Level:** the −12 dB reference, the peak ceiling and the peak-referenced modulator signal, and why they differ from a table operator's.
- **Width:** the SKIRT-free floor.
- **Cost terms:** `COST_FRMT_ONE`, `COST_FRMT` and `COST_NOIS`, as host estimates by ADR 0056's method until the bench, with the voice-count table and billing by SKIRT.
- **The formant budget:** its derivation on the share it ships with (ADR 0124's 75 %), N, the `Kinds` newtype, and the floor's heavy-route worst case with the filter's moving path and LEVEL clipper.
- **The clean-peak decision (owner, 2026-09-30).**
- **Partial supersessions:**
  - ADR 0022's destination list (§ 7);
  - ADR 0026's cost terms (per-kind terms), with its floor kept by the budget;
  - ADR 0031's statement that the costliest patch gets six voices.

It is added to `docs/adr/README.md`.

## Interactions with in-flight branches

Branches open on 2026-10-11:
- **#37's operator pages** land first, as their own branch and UAT (§ 6). Modal 2, ORBIT, projects-core and the param sweep have merged.
- **`audio-share-75`** (ADR 0124, `hw::AUDIO_BUDGET_PERCENT` 75) must merge before the budget is derived. On main's 70 % the budget is 72, so N = 1. It also edits `cost_test`, `param_sweep_test` and `hw.rs`.
- **`sound-sends`** (ADR 0123) adds OUT params and edits:
  - `addr.rs`, `params.rs` and `storage/sound.rs` (`MAX_SOUND_COST` 2,352 to 2,368);
  - `disk_codes_v1.txt` and `neutral_v1.txt`;
  - `common/sweep.rs`, `param_declarations_test` and `bench.rs`.

  It overlaps Tasks 4 and 5 most. Whichever lands second rebases the appends and recomputes `sparse_bound`'s bound.
- **`va-glyphs`** will touch `FocusGlyph` and `ui/glyph.rs`'s gauges, where WAVE's glyph learns FRMT and NOISE (§ 1).
- **`screensaver`** (ADR 0127) adds `played.rs`, an audio-to-UI reader, and an AXI line in `memory_budget_test`. The OP page's live view is a second such reader, so the two share the AXI headroom.
- **`browser-init-top`** edits `ui/browser.rs` and `ui/mod.rs`, which Task 1 also touches (`chain_def_for`, `project_replaced`).
- **`midi-bend-sustain` and `master-vol-tune`** have no commits yet.
  - A pitch bend reaches KEY through the voice's pitch, as the spec wants.
  - A master-volume change that moves `REFERENCE_LUFS` re-tunes the demo Sounds' VOL.
- `disk_codes_v1.txt` and `neutral_v1.txt` are append-only, so a clash is resolved by keeping both appends in merge order.

## Out of scope

- The FS1R's other spectral forms (all, odd and res). Today's WAVE tables cover those roles.
- Formant sequences (Fseq), and a CENTRE that follows another operator (the patent's CSEL).
- A third lane, for formants narrower than the floor on high notes.
- A second vowel table (female or child). FRMT offsets cover a speaker's size.
- A MIDI CC mod source. External control is the per-Part MIDI map (#247).
- Gang edit, LEVEL SCALE and the living diagram (#37).

## Plan order

Each task leaves the tree green. Phase 0 is its own branch.

0. **Phase 0, #37's operator pages**, as `2026-09-27-algo-pages-design.md` § Operator pages specifies, with its owner UAT. It brings the dynamic OSC sub-row (`PartChain` in `NavCtx`) and focus by `BlockDef.id` (`refocus`).
1. `OpKind`, `Kinds` and the formant budget (after `audio-share-75`), the WAVE list and its encoder, the new params and their disk codes, INIT's values, neutral and the Sound cap, and the decode pin. The engine treats FRMT and NOISE as silent. Goldens are bit-identical but for the pool's.
2. The pure laws: the window, the constants and curves, the width floor, the level and peak gains, `fc_hz`, the vowel table and Δ, slot resolution, the NOISE band and gains, and the edge laws. Each has its unit tests.
3. The NOISE kernel path, with its tests.
4. The FRMT kernel path (lanes, exact starts, the grain delay, the two signals, feedback), with its tests, its flash measured, then the span pass and the count.
5. VOWEL, VSLOT, and the modulation destinations, with the modulatable recipe.
6. Pages: the ALGO cell, the FORMANT group page, the operator-page cells and viz, WAVE's glyph, dimming, and the screen goldens.
7. Costs: the `tools/loop_count.py` recount, the terms, `cost_test`, and the ADR.
8. The factory Sounds, the loudness check, the demo clips, and the owner's UAT.
9. At ship: the flash, the bench rows read over the USB console, and the terms updated from the bench.

## For the owner

Settled by the owner (2026-09-30): clean peaks at the −12 dB reference, and the four-voice floor through the formant budget (at most 3 FRMT operators).

These change what the owner approved, and the owner should confirm them:
- **The cost terms rose:** 90 and 130 for FRMT and 70 for NOIS, against 75–110 and 55. They set N (§ 1): if the bench lowers COST_FRMT to 110 or less, N becomes 4.
- **Per-voice memory is about 160 B,** against 100 B. No pool growth.
- **The floor's heavy-route case** (§ 1) follows ADR 0026's convention: the matrix's full-pool worst plus FOLD and DRIVE was never covered, even for table patches.

New since 2026-09-30 (refresh 2026-10-11; the ledger's `refresh-delta.md`):
- **Three formants need the 75 % share.** Today's model bills a heavy-route voice 472 over its engine (was 325). The budget is 72 at 70 %, which allows one FRMT, and 197 at 75 %, which allows 2 at the estimate and 3 at the 120 target.
- **Labels are full words where they fit:** FORMANT, CENTRE, COARSE, VELOCITY and NOISE. BW, RES, SLOT and FDBK stay short.
- **Flash is the tight resource:** about 58 KB is free, and 30 kernel variants could take 15–25 KB. The plan caps them at 10 KB.
- **ORBIT playing** lowers every patch's floor to 3 under heavy routes (ADR 0091's reserve). The floor's convention excludes it, as it does for table patches.

Design choices the owner hasn't seen, each with its reason above:
- the window law;
- the width floor;
- the shared frequency and width fields;
- feedback into the carrier only;
- PM as a grain delay;
- AUTO counting only AUTO operators.

## Closes

#243.
