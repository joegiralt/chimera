# Formant Operators for Algo: Design

**Date:** 2026-09-30
**Status:** Draft, from the owner's approved design (2026-09-30); awaiting owner review
**Issue:** private issue #243
**Related:** private issue #247 (the per-Part MIDI map)
**Builds on:** main at 4374cff, plus Modal 2 step A (branch `modal2-resonators`), which brings ADR 0056's cost method.
**Sources:** the research brief (FSVR's prose docs and Yamaha's patent US5610354), the Algo code on main (`dsp/algo/*`), and ADRs 0010, 0022, 0024–0026, 0031, 0045 and 0056.

## Why

The FS1R's formant operators make voices, choirs and breath from FM's parts: a grain train whose pitch is the note and whose spectral peak is fixed in Hz. Algo already has six operators, 32 algorithms, MORPH and a mod matrix, so two new operator kinds give most of #220's sound for far less work than a new engine.

This spec adds a voiced kind, FRMT, and an unvoiced kind, NOIS, to Algo's WAVE list, and a VOWEL macro that moves them together. It is Chimera's own design. FS1R behaviour informs it, but it is not an FS1R clone: in particular, LEVEL means level, and no width or shape control changes loudness.

**Done when:**

- any of the six operators can be FRMT or NOIS, as a carrier or a modulator, in every algorithm and through MORPH;
- VOWEL, FRMT and BW are mod destinations;
- the pages adapt: a patch with no formant operator shows the same pages and cells as today, apart from one dimmed cell (§ 6);
- every test in § Tests passes, and old patches load and sound bit for bit as before;
- ADR 0060 is written;
- the three demo clips have passed the owner's UAT;
- the chip bench's formant rows are measured at the ship flash.

## Decisions (owner, 2026-09-30)

- **Operator kinds.** FRMT (voiced) and NOIS (unvoiced) join the WAVE list, so any of the 6 operators can be one. They work as carrier or modulator, in FM, in every algorithm and in MORPH. The patch is billed from its actual operators, so the allocator plays fewer voices on heavy formant patches.
- **Voiced (FRMT):** windowed-sine grains fired at the note's pitch, with a carrier at the formant frequency inside each grain (the patent's approach). Its parameters are FRMT in Hz, KEY (0–100 % keyboard follow), BW (bandwidth, which sets the window length) and SKIRT (window shape). Phase modulation enters both the grain phase and the carrier phase. There are two overlapping grain lanes, and the second is skipped when the window fits one period.
- **Unvoiced (NOIS):** noise through a resonant band shifted to CENTRE, with WIDTH and RES.
- **Level is normalised:** BW, SKIRT and WIDTH change tone only, never level. This is our own take, not an FS1R clone.
- **VOWEL macro:** one control and one mod destination. It moves every formant operator's frequency along A→E→I→O→U, interpolating a small built-in vowel table, and each operator's FRMT offsets it from its vowel position.
- **Pages (adaptive):**
  - VOWEL takes the empty 6th cell of the ALGO home page, dimmed when no operator is a formant kind.
  - A FRMT group page joins the OSC sub-row only when at least one operator is FRMT or NOIS.
  - Each operator page swaps in its kind's cells.
  - Plain FM patches look as they do today, apart from VOWEL's dimmed cell (§ 6).
- **Modulation:** VOWEL, each operator's FRMT and each operator's BW are modulatable and read every block. SKIRT, KEY and the noise shape are read at note-on.
- **Cost:** estimated by ADR 0056's instruction-count method. The owner's estimates were 75–110 cycles per voiced operator and about 55 per unvoiced one, against 60 for a sine. No new wave tables, and DTCM is untouched. The chip bench confirms the figures at ship.
- **Saving:** frozen disk codes and idents (ADR 0045). Old patches are unaffected.
- **QA:** Claude writes the tests, and the owner does UAT. Unit tests, the param-sweep harness, and three desktop demo clips: a choir pad, a talking bass, and breath.
- **ADR:** covers the patent provenance, the operator kinds and the cost terms.
- **Order:** after Modal 2 merges, alongside ORBIT and projects-core.

## Principles (binding)

- **Chimera's own code.** It uses no FSVR code, tables or constants (GPL-3, ADR 0032). It draws only on behaviour described in FSVR's prose docs, the expired patent US5610354, and public measurement data (§ 5).
- **The table path doesn't move.** Every operator whose kind is a table wave runs exactly today's code, so every existing golden stays bit-identical.
- **Functional core.** The window law, the vowel interpolation, the normalisation and the band laws are pure functions of their inputs, and tests call them directly. The kernel only runs them.
- **The audio thread never allocates, never blocks**, and reads no new table.

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

### Formant order

The formant operators are the operators whose kind is Formant or Noise. A formant operator's **formant index** k is its position among them, counted from operator 1. So the lowest-numbered formant operator is k = 0 (F1), the next k = 1 (F2), the next k = 2 (F3), and any more have k ≥ 3.

- **Why by order, not by operator number:** any set of formant operators gets F1, F2 and F3 in order. A choir on operators 1–3 and a whisper on operators 4–6 both read as vowels.
- **Why NOIS takes an index too:** noise through vowel formants is a whisper. A breath layer that should not follow the vowel goes above the third formant operator.

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

Algo block (`AlgoParams`):

| Id | Ident | Label | Range | Law | Default | Read | Modulatable |
|---|---|---|---|---|---|---|---|
| 4 | `VOWEL` | VOWEL | 0–127 | position `x = 4v/127` along A E I O U (§ 5) | 0 (A) | every block | yes |

- **One field serves both kinds for frequency and width.** FRMT and CENTRE are the same stored field, and so are BW and WIDTH. Each pair means the same measurable thing on both kinds: the frequency of the spectral peak, and the peak's −3 dB full width in Hz. So the FRMT group page can show all six frequencies side by side, and a kind change keeps the sound's region. SKIRT, KEY and RES mean different things, so each has its own field.
- **"The noise shape" is RES.** That is the one NOIS setting read at note-on. WIDTH is BW's field, so it is modulatable like BW.
- **Why these ranges:**
  - FRMT spans every Peterson & Barney formant (270 Hz to 3.0 kHz) with room above for bright formants and below for talking-bass F1s. 50 cents a step is at the formant's just-noticeable difference, about 3–5 %.
  - BW reaches down to 20 Hz, where a noise band is nearly a tone. It reaches up to 10 kHz, which is a full breath or a click-like grain.
  - SKIRT stops at 3 because at q = 16 the window's sidelobes are already under −80 dB. A fifth step would only lengthen the window into the two-period cap (§ 3).
  - KEY and RES are percentages, as the owner asked.
- **Why these defaults:**
  - FRMT 800 Hz is near an open vowel's F1.
  - BW 160 Hz is a narrow, vowel-like formant. The two-period cap widens it on high notes by itself (§ 3).
  - SKIRT 1 is a clean formant without the q = 2 window's −28 dB sidelobes.
  - KEY 0 keeps the formant still, which is the point of formants.
  - RES 0 is pure band noise.
  - VOWEL A is the table's reference, where the FRMT offset is 1 (§ 5).
- **Display.** A new `ValFmt::LogHz { base: u16, per_octave: u8 }` shows Hz below 1 kHz ("734") and kHz above ("1.10k"). A new `ValFmt::Vowel` shows the nearest vowel's letter, and the cell's arc shows the exact position. KEY and RES are `ValFmt::Int(100)`, and SKIRT is `ValFmt::Int(3)`.
- **INIT.** `AlgoParams::default` sets operators 1–3's FRMT to the grid points nearest A's F1–F3: 69 (734 Hz), 83 (1.10 kHz) and 111 (2.47 kHz). Operators 4–6 get 123 (3.49 kHz, a steady fourth formant). So switching operators 1–3 of INIT to FRMT sounds A at once, and VOWEL then sweeps true vowels. The spec default stays 72, as INIT's LEVELs already differ from LEVEL's spec default (ADR 0049).
- **ParamId 13 onward** are taken here. #37's LEVEL SCALE, which isn't built, takes the next free id.

### What the existing operator parameters mean

| Parameter | Table | FRMT | NOIS |
|---|---|---|---|
| COARSE, FINE, DETUNE | pitch | the grain rate, which is the pitch; FRMT doesn't move | unused (dimmed) |
| LEVEL, VEL, AR … RR, RS | as today | as today | as today |
| FEEDBACK | into its phase | into the carrier phase only (§ 3) | unused (dimmed) |
| incoming links (PM) | into its phase | into the grain phase and the carrier phase | into the centre's phase |

- **COARSE, FINE and DETUNE tune the grain clock, never FRMT.** A formant is a property of the vocal tract, not of the harmonic. Two FRMT operators DETUNEd apart keep one formant, with beating harmonics under it: a choir's width.
- **Feedback goes into the carrier only.** Fed into the grain clock, it would make the pitch depend on the operator's own output, and above FDBK 4 the note audibly detunes. Fed into the carrier, it skews the sine inside each grain toward a saw, adding formants at 2·FC and 3·FC while the pitch stays locked: a buzzier formant.
- **NOIS has no pitch clock and ignores FEEDBACK.** Feeding noise back into its own centre would only add more noise, and leaving it out keeps the bill honest (§ 9).

## 3. The voiced operator (FRMT)

### Structure

- **Grain clock.** The grain clock is the operator's existing phase accumulator, at `f0 = op_cycles(...)` (note × COARSE/FINE × DETUNE), plus the incoming links' PM.
  - A grain starts on each **forward** wrap of that modulated phase.
  - A backward wrap starts none, and at most one grain starts per sample.
- **Lanes.** There are two lanes, A and B. Each holds a window phase `w` and a carrier phase `c`.
  - A lane is idle once `w ≥ 1`.
  - A grain start restarts the next lane. In two-lane mode the lanes alternate, so each fires every other period. In one-lane mode lane A fires every period.
- **Two-lane mode.** The second lane runs only while the window is longer than one period (`r_w < f0`, below). This is the owner's "skip lane B when BW ≥ f0", with the SKIRT's span included.
  - The mode is chosen per block.
  - A lane already sounding runs its window out, so a mode change never cuts a grain.
- **Sub-sample grain starts.** At a start, the lane begins where the wrap put it: `elapsed = g / inc_g` samples, where `g` is the grain phase just past the wrap and `1/inc_g` is a per-block reciprocal. The lane's `w` starts at `elapsed · r_w`, and its `c` starts at `c0 + elapsed · inc_c`.
  - This removes the sample-grid jitter that would smear the harmonics.
  - Under PM `elapsed` is approximate, but PM moves the grains anyway.

### Window law

- The window is `W(w) = (4w(1 − w))^q` for `0 ≤ w < 1`, with `q = 2^(SKIRT+1)`: 2, 4, 8 or 16.
  - It is computed as `u = w − w·w`, then SKIRT + 1 squarings.
  - The constant `4^q` is folded into the per-block gain.
- **Why this law, not the patent's `sin^p`:** both are the same bell family. Both behave as `x^q` at the ends, so the skirts fall at the same rate, about 6(q + 1) dB per octave, and both tend to a Gaussian as q grows. The polynomial needs no table read, about 7 instructions less per lane, and its energy has a closed form.
- **The window is symmetric.** The FS1R's formant rises as `sin^p` and falls as `sin^2`. We don't copy that asymmetry: with a symmetric window and the carrier centred on it (below), every grain is odd-symmetric and carries **no DC at any FRMT**. An asymmetric window would put DC on low formant carriers.

SKIRT's constants, pinned by tests that measure them:

| SKIRT | q | Span `K` (−3 dB full width × window length) | Energy `c_q` (mean of `W²`) | First sidelobe |
|---|---|---|---|---|
| 0 | 2 | 1.375 | 0.40635 | −28 dB |
| 1 | 4 | 1.734 | 0.29954 | −38 dB |
| 2 | 8 | 2.291 | 0.21653 | −56 dB |
| 3 | 16 | 3.121 | 0.15486 | −87 dB |

`c_q = 4^(2q) · ((2q)!)² / (4q + 1)!`, and `K` is solved numerically from the window's transform. They are two 4-entry `f32` constant arrays, 32 B of flash, not tables.

### Bandwidth: the window length

- **BW is the formant's −3 dB full width in Hz**, at any note and any SKIRT. The window is `L = K / BW` seconds, so its rate is `r_w = BW / (K · sr)` cycles per sample.
  - So SKIRT changes only how steeply the skirts fall, never the width.
  - Without that compensation, SKIRT would be a second width knob.
- **The two-period cap.** Two lanes hold a window of at most two periods, so `r_w ≥ f0/2`.
  - A narrow BW on a high note therefore widens to `BW_eff = K · f0 / 2`, the patent's comparator, which picks the faster of BW and pitch.
  - At SKIRT 1 and the 160 Hz default, the cap starts widening the formant above about 185 Hz.
- BW is read every block, so `r_w` steps per block.
  - A lane's window phase stays continuous, and its ends stay at zero, so a step never clicks.

### Carrier

- The carrier is the sine table W1 at mip 0. It is already in DTCM, so no new table is needed.
- A lane restarts its carrier at `c0 = −inc_c / (2 · r_w)` cycles, which puts the sine's zero crossing at the window's centre. The grain is then odd, and its DC is zero.
- The carrier frequency is `FC` (below), read every block. The carrier phase is continuous, so a change never clicks.
  - The FS1R latches FC per grain. We don't need to, because our carrier phase never jumps.

### Phase modulation

- The links' PM goes into both the grain phase and the carrier phase (patent claim 3).
  - In the grain phase, it moves the harmonics, putting sidebands around each one.
  - In the carrier phase, it moves the formant's content: a copy of the formant at each sideband (claims 1–2).
- FEEDBACK goes into the carrier phase only (§ 2).
- If heavy PM speeds the grain clock past a window's length, the older grain is cut when its lane restarts. That is part of FM's grit, as on the FS1R, and the cap still uses the unmodulated period.

### Frequency: FRMT, VOWEL and KEY

```
log2 FC = log2 FRMT_Hz + Δ_k(VOWEL) + KEY · (pitch − 60) / 12
```

- `Δ_k` is the vowel offset for formant index k (§ 5); it is 0 for k ≥ 3.
- `pitch` is the voice's pitch in semitones: note + TRNSP + the voice pitch offset (ADR 0042), the value `AlgoEngine::cycles` already uses.
  - KEY follows the note, not COARSE, FINE or DETUNE (§ 2).
  - KEY includes bends and vibrato, so at KEY 100 the formant keeps its place among the harmonics.
- **KEY_REF is MIDI 60 (C4)**, the middle of the singing range. So FRMT is the Hz you hear at middle C at any KEY.
- The KEY amount is latched at note-on. The pitch it follows moves per block, as every operator's pitch does.
- One `exp2` per operator per block gives FC.

### Level normalisation

- **The output's RMS equals a full-scale sine operator's (1/√2) at every BW, SKIRT and note.** LEVEL, VEL and the envelope then scale it as they scale any operator.
- **Duty.** The window's length in periods is `D = f0 / r_w`, with `0 < D ≤ 2`.
- **Gain.** Grains of energy `c_q/2` per window sample, at one per period, give the gain `g = 1 / sqrt(D · c_q)`.
  - It is computed per block (one divide and one `inv_sqrt`) and folded into the operator's gain ramp, so it costs nothing per sample and never steps.
- **Crest cap.** `g` is capped at `G_MAX = 8` (+18 dB of crest). A narrow grain on a low note is a peaky pulse train, like a glottal pulse, and 18 dB is the crest a voice-like signal needs. The cap also bounds a formant modulator's peak phase swing.
  - Below a duty of `1/(64 · c_q)` the level falls, but only far outside the level test's domain.
- **Two effects the RMS estimate leaves out:**
  - When fewer than about four harmonics sit under the formant, the level follows which harmonics FC lands on. That is the physics of a sampled envelope, and it is heard as the formant's colour.
  - When FC falls below BW/2, the odd grain's two halves start to cancel, and the level falls toward 0 Hz.
  - The level test's domain (§ Tests) excludes both.

### Band-limiting

- **Timing.** The grain train's harmonics are exact multiples of f0, because the grain starts are sub-sample exact.
- **Where aliasing can come from.** Only energy past Nyquist can alias: the formant's upper edge and its skirts. The carrier restarts where the window and its first q − 1 derivatives are zero, so a restart adds no edge.
- **Rule.** The formant's upper edge, `FC + BW_eff/2`, stays under 20 kHz:
  - the operator's gain fades linearly from 1 at an edge of 16 kHz to 0 at 20 kHz;
  - FC is clamped where the edge reaches 20 kHz;
  - FC's floor is 20 Hz.
  - VOWEL and KEY can push FC anywhere, and the fade keeps a formant from sticking at the clamp.
- **What still folds.** Past the edge, the skirts fall from their first sidelobe at about 6(q + 1) dB per octave. So at SKIRT 0 folded energy is 28 dB down and falling; at SKIRT ≥ 1 it is under −38 dB.
- **Downstream mips.** A table operator's mip still reads `incoming()`'s estimate, which prices a modulator at the target's own frequency. A formant modulator's spectrum reaches to FC + BW, so this is the same approximation today's high-ratio modulators get, and it is left alone.

## 4. The unvoiced operator (NOIS)

```
n   = xorshift32 → f32 in [−1, 1)
y1 += c·(n − y1);  y2 += c·(y1 − y2)                  two identical one-poles
out = (α · k_n · y2 + β) · sin(2π·FC·t + pm)
```

- **Band.** The two low-passes are ring-modulated up to the centre, which gives a band symmetric about FC.
  - Their corner is `f_lp = 0.777 · WIDTH`, so the band's −3 dB full width is WIDTH: two poles in series are −3 dB at 0.6436 · f_lp, doubled by the ring modulation.
  - The coefficient is `c = 1 − e^(−2π f_lp / sr)`, computed per block by `exp2`. It stays in (0, 1], so the filter is stable at any WIDTH and under any modulation.
  - Identical poles are our choice. The FS1R's poles differ, which we don't need.
- **Normalisation.** `k_n` scales `y2` to unit RMS: `k_n = √3 / (c² · sqrt((1 + a²)/(1 − a²)³))` with `a = 1 − c`, from the two-pole noise gain and the uniform source's variance, 1/3.
  - It is computed per block and ramped across the block, so it never steps.
- **RES** is our own continuous take. The FS1R's is a threshold. `R = RES/100`, `α = sqrt(1 − R)`, `β = sqrt(R)`.
  - β is a DC term, and DC through the ring modulator is a pure tone at the centre. So RES blends band noise into a tone at FC.
  - Because `α² + β² = 1`, the RMS stays 1/√2 at every RES and every WIDTH.
  - RES is latched at note-on.
- **Centre.** `FC` follows the same law as FRMT's, with VOWEL and no KEY (the owner's NOIS cells have no KEY). The centre sine is W1 at mip 0, and links add PM to its phase.
- **Edges.** They follow the same rule as FRMT's, with WIDTH for BW_eff. A centre below WIDTH/2 folds the band through 0 Hz, which gives a low rumble. The noise and the sine carry no DC, so neither does the fold.
- **Seed.** Each operator's xorshift state is seeded once, when the engine slot is built, from the voice index and the operator number. It is never reset at note-on, so repeated notes don't repeat their noise, and renders stay reproducible from a given start.

## 5. The VOWEL macro

### The table

| Vowel | As in | F1 | F2 | F3 |
|---|---|---|---|---|
| A | /ɑ/ *hod* | 730 | 1090 | 2440 |
| E | /ɛ/ *head* | 530 | 1840 | 2480 |
| I | /i/ *heed* | 270 | 2290 | 3010 |
| O | /ɔ/ *hawed* | 570 | 840 | 2410 |
| U | /u/ *who'd* | 300 | 870 | 2240 |

- **Source.** Peterson & Barney (1952), the averages for adult male speakers, in Hz. They are measurements, facts that anyone may use, and this table is our transcription.
- **O is /ɔ/**, the one back rounded mid vowel P&B measured, since their /o/ is a diphthong and wasn't measured as a monophthong.
- **Why the male averages:** VOWEL moves formants by ratios (below), so a speaker of any size is FRMT's offset. Raising every FRMT by about 17 % gives a female-like voice without a second table.

### Interpolation and mapping

- **Position.** VOWEL's position is `x = 4 · v / 127` in [0, 4], after modulation. A, E, I, O and U sit at 0, 1, 2, 3 and 4, which is stored 0, 32, 64, 95 and 127.
- **Offset.** `Δ_k(x)` interpolates linearly in log frequency between the two neighbouring vowels, relative to A: `Δ_k(x) = lerp(log2 T_k[i], log2 T_k[i+1], x − i) − log2 T_k[A]`.
  - It is stored as a constant table of octave offsets. A test recomputes it from the Hz table above.
  - **Why log, relative to A:** formant perception is close to logarithmic, and an operator's FRMT offset is a ratio, so the offset holds at every vowel. At VOWEL A, the default, every Δ is 0, so FRMT is exactly the Hz shown. A patch that never touches VOWEL plays FRMT as absolute Hz.
- **"Offsets from its vowel position."** An operator's frequency is its vowel position, `T_k(x)`, times the ratio `FRMT / T_k(A)`. So FRMT sets where the operator sits relative to the vowel, and VOWEL moves every formant operator together.
- **Mapping to formants.** Formant index k = 0, 1 and 2 (§ 1) follow F1, F2 and F3.
  - Index 3 and above don't move with VOWEL. F4 and higher barely move across vowels, and they carry the voice's colour, not the vowel's identity.
- **Cost.** The segment and fraction are computed once per voice per block, and each formant operator then does one lerp. It adds to the per-block `exp2` that FC already needs (§ 3).
- **Re-indexing.** If an operator's kind changes, the formant indices of the operators above it can change.
  - The changed operator ducks, as any WAVE change does.
  - The others step to their new index's offset at the next block. Their carrier phases stay continuous, so the step is a timbre change at an edit, never a click.
  - At VOWEL A every offset is 0 and nothing moves.

## 6. Pages

**Plain FM patches look as they do today,** with one exception: VOWEL's dimmed cell in what was the ALGO home page's empty sixth slot. It is dimmed and inert, exactly as MORPH is when ALG A = ALG B (ADR 0049). A dimmed cell shows the feature exists without being in the way. The ALGO home screen golden is re-recorded once for it. No other page or cell of a plain patch changes.

### ALGO home

- The cells are ALG A, ALG B, MORPH, TRNSP, VOL and **VOWEL**.
- `view::dimmed` gains the rule `(Algo, VOWEL) => no operator is a formant kind`.

### OSC sub-row

- `ALGO_OSC_SUB_PAGES` becomes a pure function, `osc_sub_pages(&AlgoParams) -> &'static [&BlockDef]`, returning one of two static lists:
  - the list as it is today;
  - the same list with **FRMT** first, a `group(...)` page of `AlgoOpParams::FRMT` across OP1–OP6, named "Formant", short "FRM".
- The FRMT page comes first because it is what a formant patch is edited by most. It appears as the player sets a WAVE to FRMT or NOIS on the OSC home, so one step right lands on it.
- On the FRMT page, a table operator's cell is dimmed (`(AlgoOp(n), FRMT) => kind is Table`), and a NOIS operator's cell shows its centre.
- If the page being shown leaves the list, for example when a Sound load removes the last formant operator, focus moves to the OSC home.
- The all-pages walk includes the FRMT page exactly when a formant operator exists.

### Operator pages

- **Dependency.** The operator pages of the Algo pages spec (`2026-09-27-algo-pages-design.md` § Operator pages, #37: hold B1–B6, PLUS/MINUS, EXIT) aren't built on main. They are this project's prerequisite.
  - If #37 hasn't landed when this plan starts, the plan's first task builds that section as specced, and nothing more.
  - Gang edit, LEVEL SCALE and the living diagram stay in #37.
- **Cells by kind.** Cells are chosen through the existing `page_cells`-style lookup, keyed on the operator's kind:

  | Kind | Cells |
  |---|---|
  | Table | WAVE, COARSE, FINE, LEVEL, FB, VEL (unchanged) |
  | FRMT | WAVE, FRMT, BW, SKIRT, KEY, LEVEL |
  | NOIS | WAVE, CNTR, WIDTH, RES, LEVEL, VEL |

  - FRMT's COARSE, FINE, FB and VEL stay on their group pages.
- **Viz band.**
  - A FRMT operator's page draws one grain, window times carrier at the current FRMT, BW and SKIRT, where a table operator draws its wave.
  - A NOIS operator's page draws its band as a bell over a log-frequency axis.
  - The envelope beside it is unchanged.

### Dimming on the group pages

COARSE, FINE, DETUNE and FDBK cells are dimmed for NOIS operators, through `view::dimmed`.

## 7. Modulation

- **New destinations.** VOWEL (`Algo`, id 4), FRMT (`AlgoOp(n)`, id 13) and BW (id 14) are `modulatable`, per ADR 0010, because the engine reads them every block.
  - `AlgoLive` gains `vowel: f32`, `frmt: [f32; OPS]` and `bw: [f32; OPS]`, and `AlgoLive::offset` gains the three arms, with ADR 0010's formula unchanged.
- **Routes to operators that aren't formant kinds.** A route into FRMT or BW on a table operator is allowed and does nothing, like a LEVEL route to an operator outside the algorithm. The matrix already shows such columns as inert, through `view::dimmed`.
- **Not modulatable:**
  - SKIRT: an exponent change mid-grain would jump the window.
  - KEY and RES: the owner's note-on rule.
  - Each is latched at note-on, per operator, beside the envelope coefficients.
- **Clicks.** VOWEL, FRMT and BW sweeps can't click: the carrier and window phases are continuous and the gain ramps. `vowel_sweeps_do_not_click` tests this.
- **External control.** There is no new MIDI CC mod source (owner, 2026-09-30). VOWEL, FRMT and BW, like any parameter, are mappable through the per-Part MIDI map, #247: a CC drives a parameter over a range, saved with the Part, assigned by MIDI learn and listed on a MIDI MAP page. This project builds nothing for it.
- **Supersedes in part:** ADR 0022's rule that "MORPH and the six LEVELs are the destinations", now extended by these three.

## 8. Engine and kernel

- **Per-block dispatch.** The kernel picks each operator's path once per block, by kind:
  - `render_ops` calls `run_one::<N>` for a table operator, as today; `run_formant::<N, S>` for FRMT, specialised on SKIRT (four variants), so the squarings are straight-line code; and `run_noise::<N>` for NOIS.
  - `run_pair` pairs only two table operators.
  - `render_samples` matches on the kind in `step`. The branch is the same every sample, so the predictor carries it.
- **Formant state.** It lives in `Kernel`, in a per-operator union of the two kinds' states:
  - FRMT: `w_a, w_b: f32`, `c_a, c_b: u32`, and the lane flags;
  - NOIS: `rng: u32`, `y1, y2: f32`.
  - The per-block `OpBlock` gains the kind's increments and gains. Its `lo`/`hi` table pair is unused by these kinds.
- **Swap.** A kind change is a WAVE code change, so `Swap::Ducking` ducks the operator over one block and swaps it, as today. The new kind's state starts idle: lanes at `w = 1`; the NOIS filter at 0, keeping its seed.
- **Idle.** A silent formant operator advances its grain clock and envelope, as `idle` does today. Its lanes go idle.
- **Unchanged:**
  - The mip machinery (`mip_target`, `mip_step`) is skipped for these kinds.
  - MORPH, `carrier_norm`, the plan, VELOCITY and the envelopes are untouched.
  - A formant carrier adds to the output like any carrier.

## 9. Cost and memory

### Method

This follows ADR 0056's method: the sine operator's benched `COST_OP` (60), plus the hot path's added instructions at 1.46 cycles an instruction, plus 10 %, rounded up to 10. Per-block work is spread over the block's 64 samples.

The counts below are estimates from the planned code paths. The plan recounts them from the release build's `llvm-objdump -d --mcpu=cortex-m7` once the code exists, and the chip bench confirms them at ship.

| Path | Added instructions a sample (vs a sine operator) |
|---|---|
| The second mip read and its crossfade, dropped | −13 |
| Grain clock's forward-wrap test | +3 |
| Lane A: window phase, idle select, `w − w²`, four squarings (SKIRT 3), carrier phase and PM, product | +12 |
| Lane B: the same, plus its own carrier read, fused into A's product | +26 |
| FEEDBACK kept apart for the carrier | +1 |
| Per block: two `exp2`, a divide, an `inv_sqrt`, the fade, over 64 | +1.2 |
| Grain restarts: one branch a period, at the bench's notes | +0.3 |
| **FRMT, one lane** | **+4.5 → 67 → 70** |
| **FRMT, two lanes** | **+30.5 → 109 → 110** |
| NOIS: the dropped read (−13); xorshift 3, convert 2, two poles 4, `α·k_n` ramp and β 2, product 1, per block 1 | 0 → 60 → **60** |

### Terms

- `COST_FRMT = 110`: two lanes, always billed.
  - Whether lane B runs depends on the note and on a modulated BW, and ADR 0026 bills a patch from its shape, not per note.
  - The one-lane saving, about 40 cycles, is real CPU headroom but isn't billed.
- `COST_NOIS = 60`. The owner's estimate was about 55. The count gives 60, which ADR 0056's rounding keeps.
- **Billing.** `AlgoEngine::cost` prices each active operator by its kind: `COST_OP`, `COST_FRMT` or `COST_NOIS`.
  - `COST_FEEDBACK` is billed for table and FRMT operators with FEEDBACK above 0, never for NOIS.
  - Links into any active operator are billed `COST_LINK`, as today.

Voices beside the whole FX bus, per ADR 0031's arithmetic: rev V budget 7,000, bus 1,160, voice = engine cost + 57.

| Patch | Engine cost | Rev V | Rev Y |
|---|---|---|---|
| Any plain FM patch | unchanged | unchanged | unchanged |
| Two NOIS, A1 (a breath) | 490 | 8 | 8 |
| One sine + three FRMT, A1 | 760 | 7 | 5 |
| Four FRMT + one NOIS, A1 (a choir) | 870 | 6 | 5 |
| Six FRMT, A1 | 1,030 | 5 | 4 |
| Six FRMT with feedback, A16 ∪ A17 (the new costliest) | 1,132 | 4 | 3 |

- ADR 0026's floor of four voices on rev V still holds.
- ADR 0031's "the costliest patch gets six voices on rev V" no longer holds: a six-FRMT patch gets four. The ADR records this partial supersession.
- With the master tape, the costliest gets 4 and 3.
- Mod routes add their `ModRouting` terms as usual.

### Memory

| Item | Size |
|---|---|
| Per operator: the lane/noise union (16 B), lane flags (1 B), latched SKIRT and KEY-or-RES (5 B) | 22 B |
| `kinds: [OpKind; 6]` over `waves: [WaveId; 6]` | +6 B |
| **Per voice** | **about 140 B** |

- The owner's estimate was about 100 B; the count gives about 140 B.
- `AlgoEngine` grows from 676 B to about 816 B (host, main at 4374cff). The `EngineSlot` is sized by `ModalEngine` (4,008 B), so `Voice` and the pool don't grow. `voice_is_its_chain_plus_one_slot` pins this.
- There are no new tables, and DTCM is untouched: both kinds read W1's mip 0, which is already there. The only new constants are the SKIRT arrays (32 B) and the vowel table (60 B), in flash.
- Flash grows by 30 new kernel variants: `run_formant` 6 × 4, and `run_noise` 6. That is well inside the 896 KB region.
- The bench checks the I-cache: a formant operator's block touches one variant.

### Bench rows (ship flash)

- **FRMT1:** one FRMT operator, BW 4 kHz, so one lane.
- **FRMT2:** one FRMT operator, BW 20 Hz, so two lanes.
- **NOIS:** one NOIS operator.
- **FRMT6:** six FRMT operators with feedback, on A16 ∪ A17.
- **CHOIR:** the CHOIR factory Sound.

## 10. Saving and compatibility

- The disk codes are frozen (ADR 0045):
  - WAVE codes 16 FRMT and 17 NOIS;
  - operator param ids 13–17, with idents `FRMT`, `BW`, `SKIRT`, `KEY` and `RES`;
  - Algo param id 4, `VOWEL`.
  - All are appended to `disk_codes_v1.txt`.
- **Old patches** carry none of the new ids, so they load the spec defaults.
  - Their WAVE codes are 0–15, so every operator is a table operator, and the new parameters are inert.
  - They sound bit for bit as before, and `sound_codec_test` and the goldens prove it.
- The file format and version don't change. New values are new ids in the existing `Block` records.

## Tests

Each test fails on today's code or pins a law of this spec.

| Test | Proves |
|---|---|
| `formant_peak_stays_put_across_pitch` | KEY 0, FRMT 1 kHz, BW 300 Hz: from C2 to C4, the power-weighted centroid of the partials within ±BW of FRMT lies within 5 % of 1 kHz. |
| `key_100_tracks_the_note` | KEY 100, FRMT 800 Hz: at C3, C4 and C5 the formant peak is at 400, 800 and 1,600 Hz ±3 %. At KEY 50, C5 lands at 1,131 Hz ±3 %. |
| `bw_and_skirt_change_tone_not_level` | C3, FRMT 1 kHz: BW from 4·f0 to 4 kHz, at each SKIRT, has RMS within ±0.5 dB of a full-scale sine operator's. The −3 dB width measured from the partials' envelope is within 15 % of BW_eff. |
| `skirt_constants_are_measured` | `K` and `c_q` equal the window's measured −3 dB span and mean square within 0.5 %. |
| `formant_grains_carry_no_dc` | FRMT 100 Hz, BW 10 kHz, every SKIRT: \|mean\| < 1e-4 over 1 s. |
| `harmonics_are_exact` | FRMT alone at A3: partials at k·f0 within 0.1 cent, and the floor between partials 60 dB under the peak (sub-sample grain starts; sample-grid starts fail it). |
| `lane_b_runs_only_when_the_window_is_long` | The lane-mode switch at `r_w = f0`. A mode change mid-note cuts no grain, and passes the click detector. |
| `high_formants_fade_before_nyquist` | An upper edge at 20 kHz or above is silent; at 18 kHz the gain is half. At FRMT 12.8 kHz, BW 1 kHz, SKIRT 1, energy below 8 kHz stays 60 dB under the peak. |
| `feedback_moves_the_formant_not_the_pitch` | FDBK 0 against 7 on FRMT: f0 is unchanged within 0.1 cent, and energy near 2·FC rises. |
| `pm_enters_grain_and_carrier` | A sine modulator into FRMT gives sidebands around both the harmonics and the formant (patent Table 1's shape). |
| `vowel_sweeps_do_not_click` | VOWEL swept 0→127 over 50 ms, and a square LFO jumping 0↔127, on FRMT and NOIS formants: the click detector passes. |
| `vowel_table_is_peterson_barney` | The octave-offset constants recompute from the Hz table; at each vowel's position, operators 1–3 at INIT's FRMTs sit on that vowel's F1–F3 within 50 cents. |
| `formant_index_is_by_order` | FRMT on operators 2, 4 and 5 map to F1, F2 and F3; a fourth formant operator doesn't move with VOWEL. |
| `noise_band_centre_and_width` | CENTRE 2 kHz, WIDTH 500 Hz, RES 0: the averaged spectrum peaks within 5 % of 2 kHz, with a −3 dB width within 15 % of 500 Hz. |
| `noise_level_is_normalised` | WIDTH from 20 Hz to 4 kHz and RES 0–100: RMS within ±0.5 dB of a sine operator's; RES 100 is a tone at CENTRE. |
| `formant_operators_are_stable` | Every new parameter at its ends, FDBK 7, full PM from five modulators, 30 s: finite, bounded by `G_MAX` × the gains, no growth. |
| `plain_fm_is_bit_identical` | Every existing Algo golden and factory render is unchanged. |
| `old_patches_load_unchanged` | Every stored v1 fixture decodes to the same params and render hash. |
| `disk_codes_v1` | The appended codes and idents. |
| `pages_adapt_to_formant_kinds` | The FRMT page is present exactly when a formant operator exists. VOWEL, and the FRMT cells of table operators, are dimmed. NOIS's COARSE/FINE/DETUNE/FDBK are dimmed. Operator pages show each kind's cells. |
| `formant_costs_bill_by_kind` | Each term, and the voice-count table above, in `cost_test`. |
| memory | `voice_is_its_chain_plus_one_slot` still passes, and `Voice`'s size is unchanged. |

- **Param-sweep harness** (branch `param-sweep`, on main before this plan ends). It picks up the new registry parameters automatically. It gets a formant fixture Sound, with operators 1–3 FRMT and operator 4 NOIS, where each new parameter must move the output. ADR 0010's modulatable test uses the same fixture.
- **Screen goldens.** The ALGO home is re-recorded once, for the dimmed VOWEL cell. New goldens cover the FRMT page and a FRMT and a NOIS operator page.

## Demo clips (owner UAT)

Three new factory Sounds. The clips render from them, so what the owner approves is what ships.

- **CHOIR.** A pad on A1:
  - operators 1–3 FRMT at A's formants, DETUNEd −2, 0 and +2 for width;
  - operator 4 FRMT at 3.49 kHz, low;
  - operator 5 NOIS as breath (CENTRE 1.5 kHz, WIDTH 3 kHz), low;
  - slow attack, and LFO 1 slowly into VOWEL between A and O.
- **TALK BASS.** A1:
  - operator 1 W1 at 0.50 for the body;
  - operators 2–4 FRMT for F1–F3;
  - VOWEL stored at O, with ENV 2 into VOWEL, so each note opens toward A and closes ("wow").
- **BREATH.** A1:
  - operator 1 NOIS (CENTRE 1.2 kHz, WIDTH 2.5 kHz);
  - operator 2 NOIS (CENTRE 5 kHz, WIDTH 4 kHz), lower;
  - slow attack and release.

An `#[ignore]`d host test, `render_formant_demo_clips`, plays a short phrase of each Sound through the `Instrument` and writes 48 kHz WAVs to `target/demo/`, with no new dependency. The plan pins each Sound's voice count in `cost_test`.

## Provenance

- **From Yamaha's patent US5610354** (expired): the structure. A window oscillator at the pitch, a carrier reset at each window cycle, two π-offset window lanes with a cap at two periods, PM into the window and carrier phases, and noise low-passed, offset by DC and ring-modulated to a centre.
- **From FSVR's prose docs** (behaviour descriptions and measurements only, no code, tables or constants; GPL-3, ADR 0032): that the width is a time, so constant in Hz; that the formant holds still because the carrier restarts each grain; that the noise band is a low-pass ring-modulated to the centre, with an absolute width in Hz.
- **From Peterson & Barney (1952):** the vowel formant averages, which are measurement data.
- **Ours:**
  - the polynomial window and its SKIRT law;
  - BW as a −3 dB width held across SKIRT;
  - the odd, centred carrier;
  - sub-sample grain starts;
  - RMS normalisation and the crest cap;
  - continuous RES;
  - carrier-only feedback;
  - the edge fade;
  - the VOWEL macro, its log interpolation and its mapping by order;
  - every range and unit.
- THIRD_PARTY.md doesn't change.

## ADR

ADR 0060 (or the next free number when it is written): "Formant operators are Algo operator kinds, after the FS1R patent". It records:

- **Provenance:** the split above, and FSVR's GPL status.
- **Kinds:** `OpKind`, the WAVE codes, and the formant order.
- **Cost terms:** `COST_FRMT` and `COST_NOIS` as host estimates by ADR 0056's method until the bench, the voice-count table, and the two-lane billing rule.
- **Partial supersessions:**
  - ADR 0022's destination list (§ 7);
  - ADR 0026's cost terms, which gain per-kind terms;
  - ADR 0031's statement that the costliest patch gets six voices.

It is added to `docs/adr/README.md`.

## Interactions with in-flight branches

- **`modal2-resonators`** must merge first. It brings ADR 0056's method and edits the ALGO/Modal registries. This project's `block_registry.rs` edits (the ALGO home cell and the OSC sub-row) are Algo-only.
- **ORBIT and projects-core** run alongside. Both append to `disk_codes_v1.txt`, which is append-only, so a clash is resolved by keeping both appends in merge order. Neither touches `dsp/algo`.
- **#37** (operator pages) is a prerequisite (§ 6).
- **`param-sweep`** supplies the harness (§ Tests).

## Out of scope

- The FS1R's other spectral forms (all, odd and res). Today's WAVE tables cover those roles.
- Formant sequences (Fseq) and a CENTRE that follows another operator (the patent's CSEL).
- Per-note billing of lane B.
- A MIDI CC mod source. External control is the per-Part MIDI map (#247).
- A second vowel table (female or child). FRMT offsets cover a speaker's size.
- Gang edit, LEVEL SCALE and the living diagram (#37).

## Plan order

Each task leaves the tree green.

1. Operator pages, as `2026-09-27-algo-pages-design.md` § Operator pages specifies, if #37 hasn't landed.
2. `OpKind`, the WAVE list, the new params and their disk codes. The engine treats FRMT and NOIS as silent. Goldens bit-identical.
3. The pure laws: the window, the SKIRT constants, normalisation, the vowel table and Δ, the NOIS band and gains, and the edge fade. Each has its unit tests.
4. The NOIS kernel path, with its tests.
5. The FRMT kernel path (lanes, sub-sample starts, PM, feedback), with its tests.
6. VOWEL, and the modulation destinations.
7. Pages: the ALGO cell, the FRMT group page, the operator-page cells, dimming, and the screen goldens.
8. Costs: the objdump recount, the terms, `cost_test`, and ADR 0060.
9. The factory Sounds, the demo clips, and the owner's UAT.
10. At ship: the flash, the bench rows, and the terms updated from the bench.

## Open questions

None. The design choices the owner hasn't seen are marked above with their reasons: the window law, the SKIRT range, the shared frequency and width fields, feedback into the carrier only, and the demo Sounds.

## Closes

#243.
