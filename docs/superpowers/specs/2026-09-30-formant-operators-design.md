# Formant Operators for Algo: Design

**Date:** 2026-09-30
**Status:** Draft, revised 2026-09-30 after an adversarial review and the owner's rulings; awaiting owner review
**Issue:** private issue #243
**Related:** private issue #247 (the per-Part MIDI map); #37 (operator pages, a prerequisite)
**Builds on:** main at 4374cff, plus Modal 2 step A (branch `modal2-resonators`), which brings ADR 0056's cost method.
**Sources:** the research brief (FSVR's prose docs and Yamaha's patent US5610354), the Algo code on main (`dsp/algo/*`), and ADRs 0010, 0022, 0024–0026, 0031, 0045, 0056 and 0058.

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
- ADR 0060 is written;
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

- `WaveId` stays what it is: an index into the 16 generated tables (`WAVE_COUNT = 16`). The tables, `chimera-waves` and ADR 0025's DTCM copy are unchanged.
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
| 17 | NOIS | `NOIS` | `Noise` |

- `OP_KIND_NAMES` and `OP_KIND_IDENTS` (18 entries each) are `WAVE_NAMES` and `WAVE_IDENTS` followed by the two new entries. `ALGO_OP_SPECS[0]` lists them, with `max` 17.
- The const assert becomes `ALGO_OP_SPECS[0].max as usize == OP_KIND_COUNT - 1`.
- `set_enum_code` accepts 0–17. Codes 16 and 17 are appended to `disk_codes_v1.txt` for each of the six operator blocks.
- The OSC home page (WAVE) is unchanged: FRMT and NOIS are the last two entries of its list.

### The formant budget

**The floor's worst case.** This is ADR 0026's convention: LP24, no FOLD or DRIVE, and A16 ∪ A17's 12 links. On top of that, **heavy routes and the master tape**:

- **Heavy routes:** ENV B in BURST mode on the VCA with its curve, a slide, all 16 destinations routed, and all three LFOs as FUNC.
  - That is `ModRouting` 47 + 24 + 98 + 1 + 76 + 33 + 6 + 15 + 15 = **315**, so the voice's overhead is 315 + `CHAIN_COST` 10 = **325**.
- **Four voices on rev V with the tape:** (7,000 − 1,470) / 4 = 1,382 cycles a voice. That leaves the engine **1,057**.
- **The shape's fixed terms:** `COST_BASE` 370, 12 links × 8 = 96, and six operators at `COST_OP` + `COST_FEEDBACK` = 366. That leaves **225 cycles** for the formant operators' extra cost over a table operator.

The budget:

| Kind | Charge (its cost − `COST_OP`) |
|---|---|
| FRMT (billed at SKIRT 1–3's 130, so a SKIRT edit can never break the budget) | 70 |
| NOIS | 10 |
| Table | 0 |

```
FORMANT_BUDGET = 1,382 − 325 − 370 − 96 − 366 = 225
Σ charges ≤ 225   ⇒   N = ⌊225 / 70⌋ = 3 FRMT operators
```

With 3 FRMT the 15 left admits 1 NOIS; with 2 FRMT or fewer, every other operator may be NOIS.

- **Derived, not typed in.** `FORMANT_BUDGET` is a `const` computed from `COST_*`, `FxBus::COST` with the tape, the heavy-route overhead and the rev V budget. When the bench updates a term, N follows.
- **Only a patch within budget can be built.** Kinds live in a newtype, `Kinds([OpKind; OPS])`, held by `AlgoParams`:
  - Its only mutator, `AlgoParams::try_set_kind(op, kind) -> Result<(), OverBudget>`, refuses a kind that would overspend.
  - `AlgoOpParams` no longer carries its own wave byte. The `(AlgoOp(n), WAVE)` address is served through `AlgoParams`, so every write path goes through the check: the UI, `set_enum_code`, the decoder, the param sweep and a future gang edit.
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
| 13 | `FRMT` | FRMT | CNTR | 0–168 | `100 · 2^(v/24)` Hz: 100 Hz to 12.8 kHz, 50 cents a step | 72 (800 Hz) | every block | yes |
| 14 | `BW` | BW | WIDTH | 0–216 | `20 · 2^(v/24)` Hz: 20 Hz to 10.24 kHz, 50 cents a step | 72 (160 Hz) | every block | yes |
| 15 | `SKIRT` | SKIRT | — | 0–3 | window exponent `q = 2^(SKIRT+1)` (§ 3) | 1 | note-on | no |
| 16 | `KEY` | KEY | — | 0–100 | % keyboard follow | 0 | note-on | no |
| 17 | `RES` | — | RES | 0–100 | % tone at the centre (§ 4) | 0 | note-on | no |
| 18 | `VSLOT` | SLOT | SLOT | 0–4 | AUTO, F1, F2, F3, OFF (idents `AUTO` `F1` `F2` `F3` `OFF`, frozen codes 0–4) | 0 (AUTO) | note-on | no |

Algo block (`AlgoParams`):

| Id | Ident | Label | Range | Law | Default | Read | Modulatable |
|---|---|---|---|---|---|---|---|
| 4 | `VOWEL` | VOWEL | 0–100 | position `x = v/25` along U O A E I (§ 5) | 50 (A) | every block | yes |

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
  - A new `ValFmt::LogHz { base: u16, per_octave: u8 }` shows Hz below 1 kHz ("734") and kHz above ("1.10k").
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
- BW is read every block, and `r_w`, `1/D` and the slack (below) ramp across the block. A lane's window phase stays continuous, so a BW change never clicks.

### Carrier

- The carrier is the sine table W1 at mip 0. It is already in DTCM, so no new table is needed.
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
  - **The owner accepted this (2026-09-30):** clean peaks, and loudness through ADR 0058's reference.
  - **Why −12 dB:** a grain train is peaky. Its crest runs 4–15 dB across the vowel range. At equal RMS to a sine it would peak up to +12 dB over a sine and break the factory and sanity tests' peak of 1.0. So the reference is set so the ceiling `1/p̂` binds only past a 15 dB crest.
  - **The peak is at most 1.0 by construction**, like a sine's.
- **How the gains are applied.** `g_m` is folded into the operator's gain ramp, and `g_c / g_m` into its carrier-weight ramp. Both are computed per block and ramped, so they cost nothing per sample and never step.
- **The curves.** `ρ_q` and `p_q` are two 33-point curves per SKIRT over `FC·L` in [0, 4], each flat beyond 4, in flash. That is about 1 KB, read per block, never per sample.

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

- The cells are ALG A, ALG B, MORPH, TRNSP, VOL and **VOWEL**.
- `view::dimmed` gains `(Algo, VOWEL) => no operator is a formant kind`.

### OSC sub-row: dynamic, and focus by identity

- **Two sub-rows.** The Algo chain gets two static `ChainDef2`s, identical except the OSC block's sub-pages: today's list, and the same list with **FRMT** first. FRMT is a `group(...)` page of `AlgoOpParams::FRMT` across OP1–OP6, named "Formant", short "FRM".
  - A pure `fn algo_chain(&AlgoParams) -> &'static ChainDef2` picks the list: the formant one when any operator is a formant kind.
  - `block_def.rs` and `dungeon_map.rs` read the chain through it.
- **Focus by identity** (review I8). The UI's page position is kept by `BlockDef.id`, not by the `sub_page: u8` index. When the list changes, the focus resolves to the same page's new index.
  - If the page left the list, the focus moves to the OSC home.
  - So inserting FRMT first never lands a Sound load or an undo on the wrong page.
- **Why FRMT first:** it is what a formant patch is edited by most, and it appears as the player sets a WAVE to FRMT or NOIS on the OSC home, so one step right lands on it.
- **Cells.**
  - On the FRMT page, a table operator's cell is dimmed (`(AlgoOp(n), FRMT) => kind is Table`).
  - A NOIS operator's cell shows its centre.
- The all-pages walk includes the FRMT page exactly when a formant operator exists.

### Operator pages (prerequisite phase)

- **Dependency.** The operator pages of `2026-09-27-algo-pages-design.md` § Operator pages (#37: hold B1–B6, PLUS/MINUS, EXIT, WAVE/COARSE/FINE/LEVEL/FB/VEL cells, the viz band) aren't built on main. BW, SKIRT, KEY, RES and SLOT have no page without them.
  - They land first, as their own branch, with their own owner UAT. This spec builds on that branch.
  - Gang edit, LEVEL SCALE and the living diagram stay in #37.
- **Cells by kind.** Cells come from a pure lookup keyed on the operator's kind:

  | Kind | Cells |
  |---|---|
  | Table | WAVE, COARSE, FINE, LEVEL, FB, VEL (as #37 specs them) |
  | FRMT | WAVE, FRMT, BW, SKIRT, KEY, SLOT |
  | NOIS | WAVE, CNTR, WIDTH, RES, SLOT, LEVEL |

  - FRMT's COARSE, FINE, LEVEL, FB and VEL stay on their group pages.
- **Viz band.**
  - A FRMT page draws one grain (window times carrier) at the live FC and BW_eff, and prints both. So the page shows what VOWEL, KEY and the width floor are doing, even when the cell's stored Hz differs.
  - A NOIS page draws its band as a bell over a log-frequency axis at the live centre.
  - The envelope beside it is unchanged.

### Dimming on the group pages

COARSE, FINE, DETUNE and FDBK cells are dimmed for NOIS operators, through `view::dimmed`.

## 7. Modulation

- **New destinations.** VOWEL (`Algo`, id 4), FRMT (`AlgoOp(n)`, id 13) and BW (id 14) are `modulatable` under ADR 0010, because the engine reads them every block.
  - `AlgoLive` gains `vowel: f32`, `frmt: [f32; OPS]` and `bw: [f32; OPS]`.
  - `AlgoLive::offset` gains the three arms, with ADR 0010's formula unchanged.
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
- **Supersedes in part:** ADR 0022's rule that "MORPH and the six LEVELs are the destinations", now extended by these three.

## 8. Engine and kernel

- **Per-block dispatch.** The kernel picks each operator's path once per block, by kind:
  - `render_ops` calls `run_one::<N>` for a table operator, as today;
  - `run_formant::<N, S>` for FRMT, specialised on SKIRT (four variants), so the squarings are straight-line code;
  - `run_noise::<N>` for NOIS.
  - `run_pair` pairs only two table operators.
  - `render_samples` matches on the kind in `step`. The branch is the same every sample, so the predictor carries it.
- **Formant state.** Per operator, `Kernel` holds a plain enum, with no `unsafe`:

  ```rust
  enum OpState {
      Table,
      Formant { w: [f32; 2], c: [u32; 2], older: u8 },
      Noise { rng: u32, y1: f32, y2: f32 },
  }
  ```

  - The per-block `OpBlock` gains the kind's increments, ramps and gains. Its `lo`/`hi` table pair is unused by these kinds.
- **Swap.** A kind change is a WAVE code change, so `Swap::Ducking` ducks the operator over one block and swaps it, as today.
  - The new kind's state starts closed: a FRMT's lanes at `w = 1` with its first grain fired; a NOIS's poles at 0, keeping its seed.
- **Idle.** A silent formant operator advances its grain clock and envelope, as `idle` does today.
- **Unchanged:**
  - the mip machinery (`mip_target`, `mip_step`) is skipped for these kinds;
  - MORPH, `carrier_norm`, the plan, VELOCITY and the envelopes are untouched;
  - a formant carrier adds to the output like any carrier, through its carrier-weight ramp.

## 9. Cost and memory

### Method

- This is ADR 0056's method: the sine operator's benched `COST_OP` (60), plus the hot path's added instructions at 1.46 cycles an instruction, plus 10 %, rounded up to 10.
- Per-block work is spread over the block's 64 samples.
- The counts are estimates from the planned code paths. The plan recounts them from the release build's `llvm-objdump -d --mcpu=cortex-m7` once the code exists, and the chip bench confirms them at ship.
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

Voices beside the whole FX bus, per ADR 0031's arithmetic, without mod routes: rev V budget 7,000, rev Y 5,833, bus 1,160, voice = engine cost + 57.

| Patch | Engine cost | Rev V | Rev Y |
|---|---|---|---|
| Any plain FM patch | unchanged | unchanged | unchanged |
| Two NOIS, A1 (BREATH) | 510 | 8 | 8 |
| One sine + three FRMT at SKIRT 1, A1 (TALK BASS) | 820 | 6 | 5 |
| Three FRMT + two NOIS, A1 (CHOIR) | 900 | 6 | 4 |
| Two FRMT + four NOIS, A1 | 910 | 6 | 4 |
| Three FRMT + one NOIS + two table, all with feedback where billed, A16 ∪ A17 (**the worst allowed**) | 1,051 | 5 | 4 |

**The worst allowed patch under the floor's worst case** (heavy routes, § 1): its voice bills 1,051 + 325 = 1,376.

| | Rev V | Rev Y |
|---|---|---|
| With the tape | (7,000 − 1,470) / 1,376 = 4.02 → **4** | (5,833 − 1,470) / 1,376 = 3.2 → 3 |
| Without the tape | (7,000 − 1,160) / 1,376 = 4.24 → **4** | 3 |

- The other extreme allowed, two FRMT and four NOIS on A16 ∪ A17, bills 1,008 + 325 = 1,333: 4 voices on rev V, with and without the tape.
- ADR 0026's floor holds for every patch that can be built. The owner's floor is rev V's; on rev Y, as for the costliest table patch under heavy routes, it is 3.
- **The convention is ADR 0026's.** The floor was never promised under every route at once plus FOLD and DRIVE: `ModRouting`'s full-pool worst (633) takes even the costliest table patch to 3 voices with the tape. The heavy-route case above is the floor's defined worst.
- ADR 0031's "the costliest patch gets six voices on rev V" still holds without routes: the worst allowed formant patch bills 1,051 + 57 = 1,108, which gives 5 voices, and the costliest table patch still gets its six.

### Memory

| Item | Size |
|---|---|
| Per operator: `OpState` (20 B), latched SKIRT and slot (2 B), KEY-or-RES (4 B) | 26 B |
| `kinds: [OpKind; 6]` over `waves: [WaveId; 6]` | +6 B |
| **Per voice** | **about 160 B** |

- `AlgoEngine` grows from 676 B to about 836 B (host, main at 4374cff). The owner's estimate was about 100 B.
- The `EngineSlot` is sized by `ModalEngine` (4,008 B), so `Voice` and the pool don't grow. `voice_is_its_chain_plus_one_slot` pins that.
- No new wave table, and DTCM is untouched: both kinds read W1's mip 0, which is already there.
- The new constants are all in flash and read per block: the SKIRT arrays (80 B), the `ρ_q` and `p_q` curves (about 1 KB), and the vowel table (60 B).
- Flash grows by 30 kernel variants (`run_formant` 6 × 4, `run_noise` 6), well inside the 896 KB region. The bench checks the I-cache: a formant operator's block touches one variant.

### Bench rows (ship flash)

| Row | Patch | Tests |
|---|---|---|
| FRMT0 | one FRMT operator, SKIRT 0, BW 20 Hz | `COST_FRMT_ONE` |
| FRMT3 | one FRMT operator, SKIRT 3, BW 20 Hz | both lanes overlapping |
| FRMT PM | FRMT3 with a full-level ratio-1 modulator | the delay path |
| NOIS | one NOIS operator | `COST_NOIS` |
| FRMT MAX | the worst allowed patch (§ 9) | the budget's worst case |
| CHOIR | the CHOIR factory Sound | a real patch |

## 10. Saving and compatibility

- **Frozen codes (ADR 0045),** all appended to `disk_codes_v1.txt`:
  - WAVE codes 16 FRMT and 17 NOIS;
  - operator param ids 13–18, with idents `FRMT`, `BW`, `SKIRT`, `KEY`, `RES` and `VSLOT`;
  - VSLOT's codes 0–4, with idents `AUTO`, `F1`, `F2`, `F3` and `OFF`;
  - Algo param id 4, `VOWEL`.
- **How old patches decode** (review I7). A file decodes onto `Sound::neutral`, which is `ParamSnapshot::for_engine`, which is `AlgoParams::default()` (INIT). Then the file's own values are applied. An old file carries none of the new ids, so its new fields take INIT's values:
  - FRMT 69, 83, 111, 123, 123 and 123;
  - BW 72, SKIRT 1, KEY 0, RES 0, VSLOT AUTO;
  - VOWEL 50.
  - Its WAVE codes are 0–15, so every operator is a table operator and those fields are inert.
  - It sounds bit for bit as before.
  - Because INIT is the decode base, INIT's new values are frozen with the codes. `old_patches_load_unchanged` pins them.
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
| `old_patches_load_unchanged` | Every stored v1 fixture decodes to the same params, including INIT's new fields (§ 10), and the same render hash. |
| `disk_codes_v1` | The appended codes and idents. |
| `pages_adapt_to_formant_kinds` | The FRMT page is present exactly when a formant operator exists. The focus is kept by `BlockDef.id` through a list change (on DETUNE, a Sound load that adds FRMT stays on DETUNE). VOWEL and the table operators' FRMT cells are dimmed, as are NOIS's COARSE, FINE, DETUNE and FDBK. Operator pages show each kind's cells. |
| `formant_costs_bill_by_kind` | Each term and the voice-count table above, in `cost_test`. |
| `formant_budget_keeps_four_voices` | `FORMANT_BUDGET` recomputes from the cost terms. Every allowed kind assignment (all 3⁶ over table/FRMT/NOIS, within budget), at every SKIRT, with feedback on every billed operator, on the costliest union (A16 ∪ A17) under the heavy routes, bills ≥ 4 voices on rev V, with and without the tape. The first refused assignment of each shape would bill 3. |
| `over_budget_kinds_cannot_be_built` | `try_set_kind` refuses a fourth FRMT, and a second NOIS beside three FRMT. A property test drives random writes through every path (UI encoder, `set_enum_code`, the decoder, the sweep): no `AlgoParams` ever exceeds the budget. The WAVE encoder steps over refused kinds and toasts. An over-budget file decodes its excess as W1. `Kinds::new` rejects an over-budget const at compile time. |
| memory | `voice_is_its_chain_plus_one_slot` still passes, and `Voice`'s size is unchanged. |

- **ADR 0010's modulatable test.** `modulatable_test::recipe` makes the destination audible. For `AlgoOp(n)`'s FRMT or BW, operator n's WAVE is set to FRMT (NOIS for a second case). For `Algo`'s VOWEL, operators 1–3 are FRMT on A1.
- **Param-sweep harness** (branch `param-sweep`, on main before this plan ends). It picks up the new registry parameters automatically.
  - It gets a formant fixture Sound: operators 1–3 FRMT, operator 4 NOIS, on A1, where each new parameter must move the output.
  - It plays at C3, not at KEY_REF, so KEY is heard.
- **Screen goldens.** The ALGO home is re-recorded once, for the dimmed VOWEL cell. New goldens cover the FRMT page and a FRMT and a NOIS operator page.

## Demo clips (owner UAT)

Three new factory Sounds. The clips render from them, so what the owner approves is what ships.

- **CHOIR:** a pad on A1, within the budget (3 FRMT, 2 NOIS: 6 voices on rev V).
  - Operator 1 is FRMT on slot F1.
  - Operators 2 and 3 are FRMT on slot F2, DETUNE −2 and +2. F2 carries the vowel's identity, and its beating is the most audible, so doubling it gives the choir its width.
  - Operator 4 is NOIS on slot F3: air on the third formant, which follows the vowel.
  - Operator 5 is NOIS on slot OFF: breath, CENTRE 1.5 kHz, WIDTH 3 kHz, low.
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
  - Each Sound is measured against ADR 0058's loudness reference (−15 LUFS). Equal RMS isn't equal loudness: formants at 2–4 kHz weigh 2–4 dB louder under K-weighting.
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

ADR 0060 (or the next free number when it is written): "Formant operators are Algo operator kinds, after the FS1R patent". It records:

- **Provenance:** the split above, including FSVR's measured skirt law, and FSVR's GPL status.
- **Kinds:** `OpKind`, the WAVE codes, and VOWEL SLOT.
- **Level:** the −12 dB reference, the peak ceiling and the peak-referenced modulator signal, and why they differ from a table operator's.
- **Width:** the SKIRT-free floor.
- **Cost terms:** `COST_FRMT_ONE`, `COST_FRMT` and `COST_NOIS`, as host estimates by ADR 0056's method until the bench, with the voice-count table and billing by SKIRT.
- **The formant budget:** its derivation, N = 3, the `Kinds` newtype, and the floor's heavy-route worst case.
- **The clean-peak decision (owner, 2026-09-30).**
- **Partial supersessions:**
  - ADR 0022's destination list (§ 7);
  - ADR 0026's cost terms (per-kind terms), with its floor kept by the budget;
  - ADR 0031's statement that the costliest patch gets six voices.

It is added to `docs/adr/README.md`.

## Interactions with in-flight branches

- **#37's operator pages** land first, as their own branch and UAT (§ 6).
- **`modal2-resonators`** must merge first. It brings ADR 0056's method and 0058's loudness reference.
- **The chain-lookup change** (`algo_chain`, and focus by `BlockDef.id`) touches `block_def.rs` and `dungeon_map.rs`, which other UI branches also read. It is its own task, with the page-walk tests.
- **ORBIT and projects-core** run alongside. Both append to `disk_codes_v1.txt`, which is append-only, so a clash is resolved by keeping both appends in merge order. Neither touches `dsp/algo`.
- **`param-sweep`** supplies the harness (§ Tests).

## Out of scope

- The FS1R's other spectral forms (all, odd and res). Today's WAVE tables cover those roles.
- Formant sequences (Fseq), and a CENTRE that follows another operator (the patent's CSEL).
- A third lane, for formants narrower than the floor on high notes.
- A second vowel table (female or child). FRMT offsets cover a speaker's size.
- A MIDI CC mod source. External control is the per-Part MIDI map (#247).
- Gang edit, LEVEL SCALE and the living diagram (#37).

## Plan order

Each task leaves the tree green. Phase 0 is its own branch.

0. **Phase 0, #37's operator pages**, as `2026-09-27-algo-pages-design.md` § Operator pages specifies, with its owner UAT.
1. The dynamic OSC sub-row: `algo_chain`, and focus by `BlockDef.id`. Every screen golden bit-identical.
2. `OpKind`, `Kinds` and the formant budget, the WAVE list and its encoder, the new params and their disk codes, INIT's values, and the decode pin. The engine treats FRMT and NOIS as silent. Goldens bit-identical.
3. The pure laws: the window, the constants and curves, the width floor, the level and peak gains, `fc_hz`, the vowel table and Δ, slot resolution, the NOIS band and gains, and the edge laws. Each has its unit tests.
4. The NOIS kernel path, with its tests.
5. The FRMT kernel path (lanes, exact starts, the grain delay, the two signals, feedback), with its tests.
6. VOWEL, VSLOT, and the modulation destinations, with the modulatable recipe.
7. Pages: the ALGO cell, the FRMT group page, the operator-page cells and viz, dimming, and the screen goldens.
8. Costs: the objdump recount, the terms, `cost_test`, and ADR 0060.
9. The factory Sounds, the loudness check, the demo clips, and the owner's UAT.
10. At ship: the flash, the bench rows, and the terms updated from the bench.

## For the owner

Settled by the owner (2026-09-30): clean peaks at the −12 dB reference, and the four-voice floor through the formant budget (at most 3 FRMT operators).

These change what the owner approved, and the owner should confirm them:
- **The cost terms rose:** 90 and 130 for FRMT and 70 for NOIS, against 75–110 and 55. They set N (§ 1): if the bench lowers COST_FRMT to 110 or less, N becomes 4.
- **Per-voice memory is about 160 B,** against 100 B. No pool growth.
- **The floor's heavy-route case** (§ 1) follows ADR 0026's convention: the matrix's full-pool worst plus FOLD and DRIVE was never covered, even for table patches.

Design choices the owner hasn't seen, each with its reason above:
- the window law;
- the width floor;
- the shared frequency and width fields;
- feedback into the carrier only;
- PM as a grain delay;
- AUTO counting only AUTO operators.

## Closes

#243.
