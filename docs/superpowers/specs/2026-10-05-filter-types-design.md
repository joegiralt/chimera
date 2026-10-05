# Filter Types: CLEAN (Cascadia), 2040 and DIRTY, and ALGO's Audio-Rate Taps: Design

- **Status:** Proposed. Docs only; no code is written until the owner approves.
- **ADR:** [0096](../../adr/0096-filter-types-clean-2040-dirty-and-algo-taps.md) (Proposed).
- **Builds on:** the filter-routing spec ([2026-09-27-filter-routing-design.md](2026-09-27-filter-routing-design.md)) and ADR 0037.
- **Issues:** it answers private issue #128 for these three types. It redirects private issue #126 from the Rev 3 CEM3320 to the Rev 1/2 SSM2040, and addresses private issue #57 for the filter.
- **Revision 2:**
  - It folds in the adversarial review (B1–B5, H1–H3, M1–M6) and every owner ruling of 2026-10-05.
  - It adds the owner's ALGO audio-rate taps (§ 6).
- **Revision 3:** it applies the re-review (H-new-1 to 4, M-new-1 to 4, N4, L-new-1 and 2, and M6's leftover). ADR 0096 now supersedes ADR 0037, which is Accepted, rather than amending it.

**Implementation starts after `modal-stagger` merges.** That branch may replace `filter.rs`'s `saturate()` with stmlib's rational soft clipper, which divides. The CLEAN loop's in-loop saturate must stay divide-free on its common path, and the cost probes assert it (§ Cost). If `modal-stagger` lands a dividing clipper, the filter keeps today's C1 `saturate` for its own use.

## Goal

- **CLEAN** (the type the code calls `Svf`) becomes the home filter.
  - It has the Intellijel Cascadia VCF's UX: its controls, its words and its modes.
  - It keeps today's ZDF/TPT state-variable core, which the owner likes.
  - It self-oscillates at the top of RES.
- **Notches:**
  - NOTCH1 puts its notch an octave above FREQ, as the Cascadia's NT2 does.
  - NOTCH2 makes two notches that sweep with FREQ, as the Cascadia's PHZR does.
  - Today the double-notch mode (PHASER) has no notches, and the single notch sits at FREQ.
- **Two new types, each a "machine"** with its own modes and panel:
  - **2040**, the SSM2040 of the Prophet-5 Rev 1 and Rev 2 and of the RSF Kobol Expander;
  - **DIRTY**, a Chamberlin SVF: cheap, digital, and edgy near Nyquist.
- **Taps:** in ALGO, OP 5 and OP 6 become audio-rate modulation sources ("taps") for filter FREQ, filter Q, the VCA and FOLD.
- **Cost:** every type, mode and tap destination has a static instruction budget and is billed. No `tan` runs per sample.

## The owner's words (2026-10-05)

> The "home filter" aims for Intellijel Cascadia's VCF UX. Keep today's clean ZDF/TPT state-variable filter, which "reminds me of a 101" … because it "allows you to move very very easily".

> PROPHET bass loss: AUTHENTIC. PROPHET modes: LP4 only. LEVEL: attenuates below unity, like Cascadia. Defaults: QM IN ← ENV 3, FM3 IN ← LFO 1. Names: home type "CLEAN"; "DIRTY". DIRTY: accept ~12 kHz ceiling at 2×.

> After the review: CLEAN self-oscillates at the top of RES (k slightly negative, bounded by the in-loop saturate). PROPHET renamed "2040" (trademark). PHZR matches Cascadia: no self-oscillation (fb ≤ 0.7). LP1 → lp + bp.

> Name the modes by what they do: the single notch is NOTCH1, the double-notch phaser NOTCH2. Disk codes unchanged.

> ALGO only, OP 5 and OP 6 become audio-rate mod sources ("taps"), also still playing their algorithm role. They may not target operator pitch or level. Per-sample destinations, exactly four: filter FREQ (FM3), filter Q, VCA level (AM/ring), FOLD amount. Other destinations greyed out, with a hint. Measured into the voice bill.

## What exists today

| Claim | Evidence |
|---|---|
| One ZDF/TPT SVF (Simper/Zavalishin form), with C1 `saturate` on the integrator states | `dsp/filter.rs` `tick_svf_nonlinear`, `saturate` |
| `g = fast_tan(π·fc/fs)` once a block, ramped linearly. `k = 2(1 − RES) + 0.01` is not ramped, and is never ≤ 0, so it never self-oscillates. | `SvfFilter::process` |
| Every 2-pole stage divides each sample by `1 + kg + g²`. LP6 divides by `1 + g`. | `tick_svf_nonlinear`, `tick_onepole` |
| `fast_tan` (Taylor) puts a 20 kHz cutoff at about 18.3 kHz | `dsp/mod.rs:115`, #57 |
| PHASER outputs two SVF all-passes with no dry signal: unit magnitude, no notches | `tick` |
| NOTCH is `lp + hp`, a notch **at** FREQ | `tick` |
| LP6 is a plain one-pole that ignores RES | `tick_onepole` |
| Filter DRIVE is a pre-gain `1 + 4·DRIVE`, with no clip and no indicator | `process` |
| The panel is CUTOFF · RES · MODE · ENV · KEY. FLT › MODE holds DRIVE and LFO. ENV, KEY and LFO are views of the routes ENV 1, NOTE and LFO 1 → CUTOFF. | `ui/filter_panel.rs`, ADR 0037 |
| The matrix has 8 sources × 16 destinations (all 8 source rows are used). It is summed once a block from block-start values. | `modulation.rs`, `MAX_MOD_SOURCES` |
| ALGO renders operator-major into per-op 64-sample rows, or per sample when the plan has delayed links | `algo/kernel.rs` `render_ops`, `render_samples` |
| `ModSource` disk codes 0–7 are frozen | `storage/codes.rs` |
| Each SVF mode is billed as its delta over LP24, which the engine terms carry: PHASER 17, BP24 and HP24 2 | `FilterKind::cost`, `cost_test.rs` |

## Decisions

### 1. TYPE, MODE and the machines

- The screen calls KIND **TYPE**, as the ENV, LFO and reverb slots already do. The ParamId (6), the ident `KIND` and the disk codes are unchanged.

| TYPE (code) | MODEs, in knob order, **default** in bold |
|---|---|
| CLEAN (0) | LP1, LP2, **LP4**, BP2, BP4, HP4, NOTCH1, NOTCH2 |
| 2040 (4) | **LP4** only |
| DIRTY (6) | **LP2**, BP2, HP2, NOTCH1 |

- **Mode names say what the mode does.**
  - NOTCH1 is one notch and NOTCH2 is two. NOTCH2 is the Cascadia's PHZR.
  - CLEAN's NOTCH1 notches at 2 × FREQ (the Cascadia's NT2). DIRTY's NOTCH1 notches at FREQ (Chamberlin's own).
  - A mode's disk code and ident are its identity and are unchanged: `Notch` = 6 "NOTCH" and `Phaser` = 7 "PHASER". Only the screen label changes.
- **Labels and defaults belong to the type.**
  - `FilterKind::mode_label(mode)` replaces the global name tables (#165).
  - `FilterKind::default_mode()` replaces "`modes()[0]` is the default" (ADR 0037, superseded by 0096). MODE's `ParamSpec` default resolves through it, so index 0, now LP1, never becomes a default.
- **Display names:**
  - The code's `FilterKind::Svf` keeps ident "SVF" and shows **CLEAN**.
  - **2040** is named after the chip. "Prophet" is Sequential's trademark, so no screen, ident or doc label uses it.
- **The default TYPE depends on the engine** (owner, 2026-10-05). ALGO, MODAL and TAPE start on **CLEAN**; the future VA engine starts on **2040**.
  - **One source.** The engine's neutral Sound sets it, through `FilterKind::default_for(engine)`, which `ParamSnapshot::for_engine` applies. So both `Sound::init` (a new or INIT patch) and `neutral_params` (`storage/sound.rs`, the frozen base a file decodes onto) start on that type, at the type's `default_mode()`. `Sound::init` seeds NOTE → CUTOFF from that kind's `key_default()`, not `FilterKind::default()`'s.
  - **Existing patches** keep the TYPE they stored, because a stored KIND overrides the neutral base. A file without KIND decodes onto its engine's neutral. For ALGO, MODAL and TAPE that is CLEAN, today's only kind, so nothing changes.
  - **Fixture.** The neutral-Sound fixture gains one row per engine, appended only: `(engine, KIND code, MODE code)`, i.e. ALGO/MODAL/TAPE → (0, 2), and VA → (4, 2) when VA lands. A test checks `Sound::init(e)` and `neutral_params(e)` against each row.
- **New codes,** all append-only (ADR 0009):
  - `FilterKind::K2040 = 4` (the slot the routing spec reserved for the Prophet kind), ident "2040";
  - `FilterKind::Dirty = 6`, ident "DIRTY";
  - `FilterMode::Hp12 = 9` (the routing spec's code), ident "HP12".
  - An unknown code loads as CLEAN LP4.
- **A TYPE change:**
  - It keeps MODE if the new type has it; otherwise the new type's default.
  - CUTOFF, RES and DRIVE are shared; each type maps them through its own law.
  - Routes are never touched.
- **Never snapped: TYPE and MODE changes are warm-started and crossfaded (B5).**
  1. **Warm start.** The incoming filter's state is seeded from the outgoing filter's outputs, so it starts near steady state:
     - CLEAN → 2040: every `sᵢ ← lp`.
     - CLEAN → DIRTY: `lp, bp ← lp, bp`.
     - 2040 → CLEAN or DIRTY: both stages `lp ← y₄`, `bp ← 0`.
     - DIRTY → CLEAN: `lp, bp ← lp, bp`.
     - Within CLEAN, entering NOTCH2: each all-pass state `s ← lp`, which is the all-pass's DC steady state.
     - Any state a type stops using is zeroed when it is left, so it is clean the next time it is entered.
  2. **Crossfade.** The old and new filters run together for two blocks (128 samples), crossfaded linearly. A MODE change runs the old mode on a stack copy of CLEAN's state. That copy and the second output buffer are 256 B of stack (DTCM).
  3. **Billing.** The two blocks are not billed: they are brief and in the headroom (ADR 0061's rule for UI-only moves).

### 2. CLEAN, the home filter: the Cascadia mapping

#### Controls

| Cascadia (manual § 8) | Chimera | Label | Where |
|---|---|---|---|
| FREQ [8.5] | CUTOFF (Filter 0), 20 Hz – 20 kHz | **FREQ** | FLT 2 |
| Q [8.6] | RES (1) | **Q** | FLT 3 |
| MODE [8.7] | MODE (7) | **MODE** | FLT 4 |
| FM 1 [8.1], normalled to ENV B | route ENV 1 → CUTOFF (was `env_amount`) | **FM1** | FLT 5 |
| FM 2 [8.2], ±1 V/oct, normalled to pitch | route NOTE → CUTOFF (was `key_track`); ±100 % is ±1 oct/oct | **FM2** | FLT 6 |
| LEVEL [8.8], attenuates, and past unity clips with a red LED | new param **LEVEL** (Filter **12**), replacing DRIVE (2, retired); the law, migration and LED are below | **LEVEL** | FLT › MOD 1 |
| FM 3 [8.3], free | route *FM3 IN* → CUTOFF (was the LFO knob, `fm_amount`) | **FM3** | FLT › MOD 2 |
| FM 3 jack [8.C] | new enum `fm3_src: Jack` (Filter **8**), default **LFO 1** | **FM3 IN** | FLT › MOD 3 |
| QM [8.4] | route *QM IN* → RES | **QM** | FLT › MOD 4 |
| Q MOD jack [8.D] | new enum `qm_src: Jack` (Filter **11**), default **ENV 3** | **QM IN** | FLT › MOD 5 |

- **Layout.** FLT knob 1 is TYPE. FLT › MODE is renamed **FLT › MOD**. Its sixth cell is MODE, fixed and dimmed, on single-mode types only (2040), so that every type shows its mode somewhere (M6).
- **Ids.** Filter ids 9 and 10 stay unused, because the routing spec left them for never-built params.
- **Route knobs** are views of matrix cells, −100..+100 % (ADR 0035, 0037).
  - FM1, FM2 and FM3 act in octaves (`CUTOFF_OCTAVES`).
  - *Departures from the Cascadia:* its FM1 is unipolar and its FM3 is linear FM. Ours are one bipolar, exponential amount each (routing-spec decision 24).
- **FM3 IN and QM IN are the jacks (M4, H-new-3).**
  - **The type.** `enum Jack { Mod(ModSource), Tap(TapOp) }`, with `enum TapOp { Op5, Op6 }`. `Jack` has its own disk codes: 0–7 are the `ModSource` codes, and 8 and 9 are OP 5 and OP 6. `ModSource` itself is **not** extended: it is the matrix-row type, and its `ALL` and `index()` callers (about 91) assume exactly 8 rows, so two more variants would index out of bounds.
  - **Choices.** FM3 IN steps through LFO 1, LFO 2, LFO 3, ENV 2, ENV 3, VEL, and on ALGO also OP 5 and OP 6 (§ 6). QM IN steps through all eight matrix sources, and on ALGO also OP 5 and OP 6.
  - **Repatching moves the cable.** One UI-side function, `filter_panel::repatch(sound, jack, new: Jack)`, matches on the (old, new) pair and moves the amount in one write. The cutoff and Q move through their ramps, so the move doesn't click.
    - Mod → Mod: the matrix cell's amount and presence bit move to the new source's cell.
    - Mod → Tap or Tap → Mod: the amount moves between `ModState` and `TapRoutes`, and is converted to keep its depth in octaves (M-new-2). A CUTOFF route at 100 % is 10 octaves; a FREQ tap at 100 % is 4. So `a_tap = clamp(a·10/4, ±1)` going to a tap, and `a = a_tap·4/10` coming back. Q routes and Q taps are both linear in RES, so their amount carries over unchanged.
    - Tap → Tap: the amount moves between the two tap entries.
    - It never moves into an occupied cell or entry; that source is skipped.
  - **Exclusions.** Both params are excluded from MIDI learn and from the parameter sweep: they are patching, not sound.
  - **Loading a patch that lacks them.** FM3 IN is taken from the existing → CUTOFF route that is neither ENV 1 nor NOTE, if exactly one exists; otherwise it defaults. QM IN is taken from the single existing → RES route if there is one; otherwise it defaults.
- **On a non-ALGO Sound** a jack holding OP 5 or OP 6 shows, and acts as, its default source (LFO 1 or ENV 3). The stored tap stays inert and is kept, so it returns if the Sound goes back to ALGO (L-new-2).
- **QM** is a route into RES (linear law, ADR 0010), read per block and ramped per sample. A tap reaches it per sample (§ 6).
- **Hints (ADR 0086).**
  - MIX + MINUS on any route view of any type deletes that route. That covers FM1, FM2, FM3, QM, ENV AMT and KEYBD. The focus band then shows `MIX + MINUS DELETES THE ROUTE` (`mod_grid::DELETE`). ADR 0086's table gains this row (FLT and FLT › MOD, any route knob).
  - The tap rows' greyed cells get their own hint (§ 6).

#### LEVEL: attenuation, a transparent clipper and its LED (H1)

- **The law, Cascadia-style:** `gain = 2^(4·LEVEL − 2)`, one `fast_exp2` per block. LEVEL 0 is ×¼ (−12 dB), **0.5 is unity**, and 1 is ×4 (+12 dB). The default is 0.5. The gain is ramped per sample, which is today's pre-gain ramp at no new cost.
- **A new id, not a new meaning (H-new-4, ADR 0009).** LEVEL is a new param, Filter id **12**, and DRIVE (id 2) is retired and never reused. Giving id 2 the new law would load every DRIVE-0 patch 12 dB quieter.
  - **Translation on load.** A file that stores DRIVE but not LEVEL is translated through `storage/codes.rs`'s translation table, the precedent being Modal's `translate_v1`: `LEVEL = clamp((log₂(1 + 4·DRIVE) + 2)/4, 0, 1)`. This preserves the old gain `1 + 4·DRIVE` up to the new ×4 ceiling, so DRIVE 0 becomes LEVEL 0.5 (unity).
  - Routes into DRIVE's address are re-addressed to LEVEL's, with their amount kept.
  - `Sound::neutral`'s DRIVE stays frozen at its v1 value (0), so a file with neither param decodes to LEVEL 0.5.
  - Factory Sounds and old patches keep their level.
- **The clipper:** linear to ±1, then today's C1 knee, `1 + u − u²/2` for `u = |x| − 1`, flat at ±1.5 from ±2. It is transparent below ±1, unlike stmlib's rational clipper (0.78 at 1) or a cubic (0.85).
  - **Gate.** It runs while the *filter's* LEVEL gives a gain > 1, or while a route or tap moves it (not the DRIVE stage's `p.drive.drive`).
  - **Fade.** Its own 20 ms fade blends dry into clipped when the gate opens, and back when it closes, so crossing unity never steps.
  - **Bill.** It is billed while it runs (§ Cost).
- **2040:** LEVEL's gain feeds the OTA stages directly, and the clipper does not run (H2), because on the 2040 the stages are the clipper. With the 2040's small α (§ 3), LEVEL is its drive control.
- **The LED.** While the clipper runs, a voice keeps its block's `peak = max|x·gain|`. On the 2040 the peak is `max|(1 − G)(u − s₁)|`, the first OTA's error.
  - The voice reports `clip = clamp((peak − 1)/2, 0, 1)`.
  - Each Part publishes the maximum over its voices to `meter::FILTER_CLIP[part]`, one relaxed store a block, like `MASTER_GR`.
  - On screen it is a 3 px `theme::ALERT` ring beside LEVEL's value and in the FLT viz's corner. Its fill is a radius, held and falling over 150 ms. It isn't drawn while the clipper is off.

#### Modes against the Cascadia

| Mode | Cascadia (§ 8.7) | Today | Revised |
|---|---|---|---|
| LP1 | 6 dB/oct; Q emphasises the cutoff, to self-oscillation | plain one-pole, ignores RES | **`lp + bp`** of one stage: `(1 + s)/(s² + ks + 1)` (M1). At Q 0 (`k = 2`) it is exactly the one-pole `1/(1 + s)`, −3 dB at FREQ. Q adds a peak of `√2/k` at FREQ. |
| LP2, LP4, BP2, BP4, HP4 | 12 or 24 dB/oct | as named | unchanged, except the second stage gets its own `d₂` (§ DSP) |
| NOTCH1 (NT2) | notch "approximately an octave above the cutoff", Q emphasises FREQ | `lp + hp`, notch **at** FREQ | **`lp + ¼·hp`**: a zero at 2 × FREQ (digitally, where `tan(πf/fs) = 2g`). DC is 0 dB and the treble −12 dB, which is inherent. At Q 0, FREQ itself is an 8.6 dB dip; Q turns it into a peak (M3). Old NOTCH patches get darker. The viz draws this shape. |
| NOTCH2 (PHZR) | "a double notch … responds particularly well to sweeping" | no notches | dry plus four all-passes (below) |

#### CLEAN's DSP

- **The core stays:** the TPT SVF, the in-loop C1 `saturate` on both states, and the LP4 cascade with `k₂ = 1.2k + 0.3`.
- **Resonance and self-oscillation (B2).**
  - **Law:** `k = 2·(1 − RES/0.95)`. So `k = 2` at RES 0, `k = 0` at RES 0.95 (onset), and `k = −0.105` at RES 1.
  - **Stability bound:** `1 + kg + g² > 0` for every `g > 0` exactly when `k > −2`. With `k ≥ −0.105`, `1 + kg + g² ≥ 0.997`, so `d` never exceeds 1.003 and no divide approaches zero.
  - **Bounded.** For `k < 0` the linear loop grows, and `saturate` (|output| ≤ 1.5) caps every state read. Then `hp` is bounded by `(|x| + (0.105 + g)·1.5 + 1.5)·d`, and each state `= v + 2g·hp` stays bounded for any finite input and any `g ≤ tan(π·0.49)`. The oscillation's level is set by the saturator.
  - **One onset everywhere.** Every resonant type maps RES so it begins to oscillate at **RES 0.95**: CLEAN here, 2040 at `k = 4` (§ 3), and DIRTY at `q = 0` (§ 4).
  - **LP4:** `k₂ = 1.2k + 0.3`, clamped ≥ 0.1. The second stage resonates but does not oscillate on its own.
  - **Per-mode trim at onset (M-new-1).** Untrimmed, CLEAN's self-oscillation is hot and uneven by mode, up to +8.4 dBFS on BP2. Each resonant mode gets an output trim, `trim(mode, k) = 1` for `k ≥ 0.5`, falling linearly to the mode's pinned `TRIM_OSC[mode]` at `k ≤ 0`. The trims are set at build so every mode's self-oscillation sits below 0 dBFS, within a band shared with the 2040's (test 2). It is one multiply on the output, ramped with `k`.
- **`tan` per block (M2).** `g = tan_accurate(min(fc/fs, 0.497))`, using stmlib's FREQUENCY_ACCURATE polynomial. The clamp is ours, because ACCURATE has none. It replaces `fast_tan` in the filter, which addresses #57 there. The Modal resonators' second approximation stays #57's.
- **Coefficients: exact where they move, precomputed where they don't (B1).**
  - **Steady block** (g and k equal at both ends, no tap on FREQ or Q): `d = 1/(1 + kg + g²)` and `d₂` are computed once, with no per-sample divide. This is the common case and the billed baseline.
  - **Moving block** (FREQ or RES changed, a route moves them, or a tap drives them): `g` and `k` ramp per sample. `d` and `d₂` are **divided per sample, exactly.** Lerping `d` is unsafe: `d` is convex in `g`, so a one-block 10 → 20 kHz jump errs by 95 %, and a 1 ms envelope's +10-octave jump from 100 Hz peaks at 413 against 0.97.
  - The loop is chosen once a block, as the mode is.
  - **Bill.** A moving block costs about 24 cycles (two `vdiv`s at LP4). It is billed whenever CUTOFF or RES is routed or tapped (`CLEAN_MOVE_COST`). Unrouted, only the UI moves them: briefly, in the headroom (ADR 0061).
- **NOTCH2,** after the SSM2040 data sheet's "Multiple Notch (Phase Shifter)" (Fig. 10: four all-pass sections summed with the dry signal give "two deep notches"). Fig. 10 has no feedback, so Q as feedback is our addition, as on the Cascadia.

  ```
  u   = x + fb · w[n−1]                  // feedback, one sample late: no delay-free loop, no divide
  a   = AP(AP(AP(AP(u))))                // four TPT first-order all-passes at G₁ = g/(1+g)
  w   = a
  out = √(1 − fb) · ½ · (u + a)          // dry taken after the feedback node (B4)
  AP(v): t = (v − s)·G₁; lp = t + s; s = lp + t; return 2·lp − v
  ```

  - **Two exact notches at every Q.** `out ∝ (1 + A)·u`, so it is zero wherever the cascade's phase is −180° or −540°. In analog terms that is 0.414 × and 2.414 × FREQ, 2.54 octaves apart and centred on FREQ in log frequency. Digitally the notches are at `(fs/π)·atan(g·tan(π/8))` and `(fs/π)·atan(g·tan(3π/8))`. They move with FREQ in octaves, which is why it sweeps well.
  - **Q is feedback:** `fb = 0.7·RES`. It cannot self-oscillate, matching the Cascadia (owner). The `√(1 − fb)` scaling keeps loudness within about 1 dB for FREQ up to about 2 kHz, while the peaks between the notches rise by up to 5.2 dB with Q. Above that the one-sample delay lets Q add loudness: about +2.4 dB at 5 kHz and +3 dB at 10 kHz at full Q, which is accepted (L-new-1).
  - **Stable.** The all-pass chain has unit gain and `|fb| ≤ 0.7`.
  - **Coefficient.** `G₁` is computed per block, or per sample (one `vdiv`) on a moving block, as `d` is.
  - **Peak position.** The one-sample delay slides the peak below FREQ at high FREQ. That is accepted, and the test pins it.

### 3. 2040: the SSM2040

#### What the chip is (data sheet, Solid State Music, July 1977)

- **Sections.** Four identical sections. Each is a transconductance amplifier ("G", an OTA) into a grounded capacitor, buffered by a follower ("B"). The cutoff is exponential over 10,000:1.
- **Fig. 1, the low-pass real pole.** The input and the section's own output meet at the OTA's input through a 200 Ω attenuator, so the OTA sees the stage's *error*. A one-pole of the form `ẏ = ω·tanh(x − y)`: the nonlinearity bends the slew, not the level, unlike the Moog ladder's `tanh(x) − tanh(y)`.
- **Distortion** (THD vs input level graph): about 0.01 % at 30 mV, about 0.4–0.5 % at 80 mV. The table lists 0.1 % typical at 30 mVpp. It is soft and gradual.
- **Fig. 9, the VCF.** Four low-pass sections, with resonance as feedback from the fourth output through an external 3080 OTA. The chip has no resonance VCA of its own (Electric Druid). "At high resonance settings the filter will oscillate with a pure sine wave."
- **Bass loss.** Negative feedback `k` around four unity-DC poles gives a DC gain of `1/(1 + k)`, −14 dB at `k = 4`. Fig. 9 compensates nothing, and the owner chose that authentic behaviour.

#### The second reference: the RSF Kobol Expander

- **The same chip.** RSF's factory schematic of the Kobol Expander (1979) uses the SSM2040 as its VCF. The BOM on page 11 ("Expander Kobol", dated 20/11/79) lists CI4 as "SSM 2040". The Expander's VCF is on page 8 (dated 15/6/79).
- **The same resonance path as the data sheet.** Page 8 takes resonance from CI4's output back to its input through a CA3080 (CI5), whose bias is set by a 2N3906 (T3) on the "Res VCF" control input.
  - That is Fig. 9's structure, so the 3080 feedback limiter in the model below (B3) is the circuit in both instruments, not a guess.
  - The scan is reference only, not redistributable, and is not in the repo.
- **One type, two voicings.** The 2040 type covers both the Prophet-5 Rev 1/2 and the Kobol's voicing.
  - Both are four SSM2040 low-pass sections with 3080 resonance and no pass-band compensation.
  - Their differences are in the drive into the chip and the resonance law. LEVEL and RES span both.
  - The Kobol's reputed "natural saturation" is not confirmed by the schematic as a distinct circuit, so it gets no separate model (ADR 0096).
- **A modern discrete build.** Behringer's 2023 clone builds a discrete SSM2040 after René Schmitz's design (https://www.schmitzbits.de/rs2040.html). Its transistor-level stage is a useful check on the per-stage OTA nonlinearity below; `sat`'s curve is compared against it at build.

#### Model

Per block: `g = tan_accurate(fc/fs)`, `G = g/(1 + g)`, `k = 4·RES/0.95` (so 4 at RES 0.95 and 4.21 at RES 1), and `D = 1/(1 + k·G⁴)`. On a moving block, `G` and `D` are divided per sample, exactly, as CLEAN's `d` is (B1).

```
x_c = α · x · gain                                // α ≈ 0.1, tuned with L (H-new-1); no compensation (c = 0)
S   = (1 − G)·(s₄ + G·(s₃ + G·(s₂ + G·s₁)))        // the stages' state through the linear cascade
u_l = (x_c − k·S) · D                             // one-step ZDF: the global loop solved linearly
u   = x_c − L·sat((x_c − u_l)/L)                  // the 3080's feedback limit, L = ¼ (B3)
for i in 1..=4:                                   // x₁ = u, xᵢ₊₁ = yᵢ
    t  = g · sat((1 − G)·(xᵢ − sᵢ))               // the OTA on the true error x − y (H2)
    yᵢ = sᵢ + t
    sᵢ = yᵢ + t
out = y₄ · (1/α) · OUT_2040                       // OUT_2040 pinned so LEVEL ½, RES 0 is 0 dB ± 0.5
sat(v) = c − (4/27)·c³,  c = clamp(v, ±1.5)       // C1 cubic, no divide
```

- **ZDF/TPT one-poles.** Each stage is the trapezoidal one-pole, exact at FREQ and stable under modulation (Zavalishin ch. 3 and 5). In the linear case `t = g(1 − G)(x − s) = G(x − s)`, so the explicit-nonlinearity form reduces exactly to the linear TPT stage.
- **Solving the nonlinear loop in one step.** The global loop is solved linearly and exactly (Zavalishin's ladder solution). The stage nonlinearities then run explicitly on the true linear error, Zavalishin's "cheap" method (ch. 6). Newton with two fixed iterations was rejected: about +100 instructions and a `vdiv` per iteration.
- **Self-oscillation pitch (B3).**
  - Without the feedback limit, the model oscillates 330 cents flat: the OTAs saturate inside the loop. An RK4 reference agrees, so the error is in the model, not the solver.
  - Limiting the feedback, as the 3080 in Fig. 9 and in the Kobol's CI5 does (`L = ¼`), keeps the stages near-linear, which brings the pitch to within about −3 to −5 cents.
  - `L` is tuned at build against the 1 V/oct test.
- **The input scale α is tuned with `L` (H-new-1).**
  - With α = ½ and L = ¼, a full-scale input saturates the limiter, so resonance collapses at normal levels. At full scale 14.5 dB of resonance is lost, and the bass loss reads −7.7 dB instead of −13.6. Self-oscillation sits near −24 dBFS, against CLEAN's +0.6 to +8.4, so a TYPE switch at RES 1 jumps 25–30 dB.
  - **α ≈ 0.1** keeps the pitch within −1 cent, puts self-oscillation near −8 dBFS (inside the band CLEAN's trims target), and keeps the authentic −13.5 dB bass loss at full scale.
  - α and `L` are pinned together at build by tests 2 and 7.
  - With α this small, ordinary levels stay near-linear, and the 2040's grit comes from LEVEL.
- **Bounded.** `sat` is bounded with slope ≤ 1. Each stage moves its state by at most `2G` per sample toward its input, and `u` is a bounded combination of the input and the states. The feedback limiter bounds the loop further.
- **Bass loss:** DC gain `1/(1 + k)`. That is −14 dB at onset and −14.3 dB at RES 1. Nothing compensates it.
- **Mode:** LP4 only. FLT › MOD's sixth cell shows "LP4", fixed and dimmed.
- **Panel:**
  - FLT: TYPE · CUTOFF · RES · ENV AMT · KEYBD · DRIVE.
  - FLT › MOD: — · FM3 · FM3 IN · QM · QM IN · MODE (fixed).
  - **KEYBD** is a stepped view of the continuous route NOTE → CUTOFF. It shows OFF, HALF or FULL when the amount is exactly 0, 64 or 127, and its percentage otherwise. One detent moves to the next step in that direction.
  - **DRIVE is LEVEL (Filter 12) under the 2040's panel word, shown in one place only:** FLT's knob 6, with the LED. FLT › MOD's first cell is empty on the 2040 (M6).
- **State:** four floats.

### 4. DIRTY: the Chamberlin SVF

```
per sample, run twice (×2, the input held for both sub-steps):
    lp += f·bp
    hp  = x − q·bp − lp
    bp += f·hp
    lp, bp = clamp(lp, ±2), clamp(bp, ±2)
out = clamp(½ · ½·(m₁ + m₂), ±1)                 // m = the mode's output at each sub-step; ½ output gain (M5)
```

- **Modes:** LP2 = `lp`, BP2 = `bp`, HP2 = `hp`, and NOTCH1 = `lp + hp` (a notch at FREQ).
- **Panel:** CLEAN's panel and words, FREQ · Q · MODE · FM1 · FM2 on FLT, and FLT › MOD as CLEAN's (M6).
- **Coefficients per block:**
  - `f = 2·fast_sin(π·fc/(2·fs))`;
  - `q = 2·(1 − RES/0.95)`, so `q = 0` at RES 0.95 and −0.105 at RES 1. Below zero, DIRTY self-oscillates into the clamps: a hard-clipped, digital oscillation.
  - Both are ramped per sample. DIRTY has no divide.
- **Why it can't blow up.** The update matrix is `[[1, f], [−f, 1 − fq − f²]]`, with determinant `1 − fq` and trace `2 − fq − f²`. Jury's test gives stability exactly when `0 < fq` and `f² + 2fq < 4`, that is `f < √(q² + 4) − q` (consistent with Dattorro, and with Lazzarini and Timoney).
  1. **`f` is clamped** per block to `0.95·(√(q₊² + 4) − q₊)`, where `q₊` is the largest `q` the block can reach, its ramp or a Q tap included (one `vsqrt` a block).
  2. **×2 oversampling** halves `f` for a given FREQ. At RES 0 (`q = 2`) the ceiling is about **12 kHz** (12.35 kHz); at ×1 it would be about 6 kHz. **The ceiling rises with RES**: it is uncapped by the bound once `q ≤ 0`. That is part of the machine's edge.
  3. **The hard clamps** on `lp`, `bp` and the output bound every state and output, for any input, `f`, `q`, ramp or tap. The stable region isn't convex, so a ramp can leave it for a sample, and the clamp covers that.
- **Aliasing.** The two-sub-step average is not anti-aliasing. It only trims the top. Aliasing is part of DIRTY.
- **Output gain ½.** Without it DIRTY runs about +3 dBFS RMS hot. LEVEL drives it into the clamps, and the LED reads at the input clipper as on CLEAN.
- **State:** two floats.

### 5. UX: TYPE versus MODE, without a blank canvas

- **One knob picks the machine, the next its voice.** Every type opens at its default mode with the Sound's CUTOFF, RES and DRIVE, and with its routes where they were.
- **Each type nudges:**
  - CLEAN speaks Cascadia.
  - The 2040 has one mode and the Prophet's panel words, and its bass falls as RES rises.
  - DIRTY is four plain modes that clip and scream near the top.
- **The viz** (`viz::Response`) gains:
  - CLEAN's NOTCH1: a peak or dip at FREQ, a notch at 2 × FREQ, and −12 dB treble;
  - NOTCH2: two notches at 0.414 and 2.414 × FREQ, with peaks raised by Q (today it is flat);
  - LP1's resonant 6 dB slope;
  - DIRTY's NOTCH1 at FREQ;
  - the 2040's pass band, lowered by `1/(1 + k)`.
- **Screen goldens:**
  - FLT and FLT › MOD per type;
  - the 2040's fixed MODE;
  - the LED at 0, ½ and 1;
  - FM3 IN and QM IN after a repatch;
  - the matrix with ALGO's tap rows (§ 6).

### 6. ALGO's audio-rate taps: OP 5 and OP 6

#### What they are

- **The sources.** On an ALGO Sound, OP 5 and OP 6 are also modulation sources. Each still plays its algorithm role, as a carrier or modulator, unchanged.
- **The value.** A tap's value is the operator's own output sample, `out[op][n]`: its wave × its LEVEL × its envelope, clamped to ±1.
  - **The op's LEVEL and envelope are the tap's depth.** Turning OP 6's LEVEL down narrows its sweep, and its envelope shapes the modulation over the note.
  - The route amount sets the range.
- **What they may not reach.** Taps never reach an operator's pitch or level, which would modulate the FM a second time.
- **Exactly four destinations,** each updated every sample:

  | Destination | Effect of `a · tap[n]` (a = amount, −1..1) |
  |---|---|
  | **FREQ** (the filter's CUTOFF, through FM3) | `g[n] = clamp(g_blk · exp2_poly(4·a·tap[n]), g_min, g_max)`: ±4 octaves at 100 % |
  | **Q** (RES, through QM) | `RES[n] = clamp(RES_blk + a·tap[n], 0, 1)`, which maps to `k`, `fb` or `q` per sample |
  | **VCA** | a tap-VCA stage, `x[n] · (1 − |a| + a·tap[n])`: AM at partial depth, ring modulation at 100 %; a negative amount inverts the phase. It runs **before** the DC blocker (below). |
  | **FOLD** (FOLD amount) | `fold[n] = clamp(fold_blk + a·tap[n], 0, 1)` on the folder's per-sample path |

- **The tap-VCA runs before the DC blocker (H-new-2).** The chain becomes engine → DRIVE → filter → FOLD → **tap-VCA** → DC blocker → VCA. A carrier tapped onto the VCA, or a 1:1 FM pair, multiplies the signal by a term correlated with it, which makes DC proportional to the envelope; after the blocker that DC would thump. Before it, the blocker removes it (ADR 0060). The envelope VCA stays where it is, after the blocker.
- **Out of the voice's lifetime.** Taps never hold a voice. The VCA rule of ADR 0035 is unchanged, and the tap-VCA only scales the signal.

#### Coexisting with the block-rate matrix

- **Taps are not matrix rows.** The matrix's 8 rows are full, and raising `MAX_MOD_SOURCES` to 16 would add 128 B to every `ModState`, about 7 KB of AXI. Instead each Sound's `ParamSnapshot` gains `taps: TapRoutes`, at most 8 entries (2 taps × 4 destinations). Living in `ParamSnapshot`, it is seen by `Voice::cost(p, mods)` without a signature change (M-new-3).
  - **Storage (M-new-4).** It is stored as a record of 4-byte entries, `(Jack code, dest block code, dest param id, amount i8)`, as `Routes` are. Each entry is validated on load through `ValidAddr`, and its destination must be one of the four. An invalid entry is dropped, and unknown codes are skipped. So it is append-only in the sense of ADR 0009: no positional blob.
- **Order each block:**
  1. The block-rate matrix runs as today, giving each destination its block value: `fc_blk`, `RES_blk`, the VCA's block terms and `fold_blk`.
  2. The engine renders. ALGO copies OP 5's and OP 6's 64-sample rows into a stack `tap: [[f32; 64]; 2]` (512 B, DTCM), but only for a tapped op.
     - In `render_ops` that is a row copy.
     - In `render_samples` it is one store per sample per tapped op.
  3. The filter and folder add the tap's per-sample term to their block value, and the tap-VCA multiplies, as in the table.
- **Amounts never snap.** A tap's amount is ramped across each block from its previous value, so enabling, disabling or turning a tap is click-free.
- **Exponential FM on cutoff without a per-sample `tan`.**
  - **The scaling.** `tan` is close to linear well below Nyquist, so scaling `g` by `2^x` scales the cutoff by `2^x`. `exp2_poly` is a 3rd-order polynomial on the fraction plus an exponent-bit add for the integer part. It is about 8 instructions, divide-free, and accurate to about 0.1 % (about 2 cents).
  - **Above about fs/8** the product reads *flat* for upward moves (L-new-2): `tan` grows faster than linear, so `g·2^x` is less than `tan(2^x·ω)`, and the cutoff lands below the target. Downward moves read slightly sharp. Then the clamp to `g_max = tan_accurate(0.45)`, computed per block, keeps the filter inside its range.
  - **By type:**
    - The 2040 applies the same scaling to `g`.
    - DIRTY applies it to `f`, which `sin` keeps near-linear, and clamps to the block's `f` bound.
    - NOTCH2 scales `G₁`'s `g`.
- **Interaction with B1.** A FREQ or Q tap makes every block a moving block for that voice. CLEAN then divides `d` (and `d₂`) per sample; the 2040 divides `G` and `D`; NOTCH2 divides `G₁`. Those divides are part of the tap's bill.
- **The matrix view.** On an ALGO Sound the MTX grid shows two more rows, **OP5** and **OP6**, below the eight.
  - Only the CUTOFF, RES, VCA and FOLD columns are live in those rows. A missing column is added on the first turn, as a route knob does.
  - Every other cell in the tap rows is greyed. Touching one shows the hint `OP TAPS REACH FREQ · Q · VCA · FOLD`, a new row in ADR 0086's table.
  - On FLT › MOD, FM3 IN and QM IN offer OP 5 and OP 6 on ALGO. The FM3 or QM knob then edits the tap's amount.
  - On a non-ALGO Sound the tap rows are hidden, and stored taps are inert.
- **Gating and billing (M-new-3).**
  - A FOLD tap counts as a route in `Voice::stage_runs`, so the folder runs. Its bill includes `FOLD_COST` (45) when FOLD would otherwise be off.
  - A VCA tap is its own multiply in the tap-VCA stage, before the DC blocker, and is billed as such. ALGO's no-route VCA path is untouched.
- **Disk codes, append-only.**
  - `Jack` has codes 0–9 (ModSource 0–7, OP 5 = 8, OP 6 = 9) and idents (the ModSource idents, then "OP5" and "OP6"), pinned in the codes fixture with `ModSource`'s own `DiskCode` (M4).
  - `ModSource` is unchanged.
  - `TapRoutes` is a new record (above).

## Cost

### Method (the house manner: ADR 0056 and TAPE's ADR 0093)

- **Count today's LP24 first (H3).** Build task 1 counts the old LP24 loop before anything changes, because that is what the benched engine terms carry. The budgets below are then re-checked against it.
- **Static count.** Each (type, mode) and each tap destination has an `#[inline(never)]` probe, `filter_probe::<T, M>`, that runs one 64-sample block on the steady path and on the moving path.
  - It is built for `thumbv7em-none-eabihf`, `-C target-cpu=cortex-m7`, opt-level 2, and read with `llvm-objdump -d --mcpu=cortex-m7`.
  - The loop is counted from head to back-edge.
  - Per-block work is divided by 64.
  - **The probe asserts 0 `vdiv` in each steady loop.** That is the guard against a dividing `saturate`.
- **Cycles** = instructions × 1.46 × 1.1, + 12 per `vdiv` or `vsqrt` left per sample, rounded up to 5.
- **Bill (steady path)** = `max(0, cycles − cycles(old LP24))`. A type cheaper than the old LP24 bills 0 until a re-bench at ship time lowers the engine terms.
- **Extra terms,** each billed only while it applies:
  - `CLEAN_MOVE_COST` (and the 2040's) while CUTOFF or RES is routed or tapped;
  - `FILTER_LEVEL_COST` while the clipper runs;
  - `TAP_COST[dest]` per tap destination with a present, nonzero amount.
- Each constant's doc comment records its count, as the `COST_*` constants do.

### Budgets (instructions a sample, hand-counted per H3; the build may not exceed them)

| Path | Budget (instr) | Per-sample `vdiv` | Est. cycles | Est. bill |
|---|---|---|---|---|
| CLEAN LP4 / BP4 / HP4, steady | ≤ 56 | 0 | 90 | 0 (old LP24 ≈ 50 + 2 `vdiv` ≈ 105; counted in task 1) |
| CLEAN LP1 / LP2 / BP2 / NOTCH1, steady | ≤ 30 | 0 | 50 | 0 |
| CLEAN NOTCH2, steady | ≤ 34 | 0 | 55 | 0 (was 17) |
| CLEAN, moving (LP4) | +6 | 2 | +35 | `CLEAN_MOVE_COST` 35 |
| 2040, steady | ≤ 70 | 0 | 115 | 10 |
| 2040, moving | +6 | 2 | +35 | +35 |
| DIRTY, any mode | ≤ 30 | 0 | 50 | 0 |
| LEVEL clipper + meter + fade | ≤ +12 | 0 | +20 | +20 |
| Tap capture (per tapped op) | +1 | 0 | +2 | in each tap's cost |
| Tap → FREQ | +12, plus the moving path | 0 | +20 | 20 + the type's moving cost (CLEAN 55, 2040 55, DIRTY 20) |
| Tap → Q | +4, plus the moving path | 0 | +7 | 7 + the moving cost (CLEAN 42, 2040 42, DIRTY 10) |
| Tap → VCA | +4 (the tap-VCA multiply) | 0 | +7 | 10 |
| Tap → FOLD | +4, plus `FOLD_RAMP_COST` | 0 | +7 | 10 + 29, + `FOLD_COST` 45 when FOLD would otherwise be off |

- **Not double-billed.** FREQ and Q taps on the same voice share the moving path's divides, so the moving cost is billed once.
- **Per block:** `tan_accurate` is about 14 instructions; up to 3 `vdiv`s, 1 `vsqrt`, 1 `fast_exp2` and the ramps add under 1.5 cycles a sample. That is included above.
- **The crossfade blocks** are not billed (§ 1).
- **`cost_test`** replaces `each_svf_mode_bills_its_delta_over_lp24` with `filter_bills_each_type_and_mode` and `taps_bill_each_destination`. Every bill is ≥ 0. Every built type and mode is listed. `Voice::cost` adds the move, LEVEL and tap terms exactly when they apply, reading `p.taps`, and adds nothing when no tap is present.
- The bench's SVF row (#177) gains 2040, DIRTY and tap rows at ship time.

### Memory

- **Per voice:** CLEAN 4 floats plus NOTCH2's 5; the 2040 4; DIRTY 2; the ramp endpoints; the block's clip peak and fade; and the tap amounts' previous values. That is about 110 B against 24 B today, ×8 ≈ 0.7 KB, under ADR 0051's assert. Types keep separate, flat state, so the crossfade can run two at once.
- **Stack:** the crossfade's 256 B and the tap rows' 512 B, both in DTCM.
- **Per Sound:** `FilterParams` grows 6 B (two `Jack`s and LEVEL) and `TapRoutes` adds up to 33 B (8 entries × 4 B, plus a count). Across the pool, the triple buffer and the Performance that is about 2 KB of AXI.
- **Elsewhere:** `meter::FILTER_CLIP` is 6 × 4 B.
- **Flash:** about 2.5 KB, reported in one line.

## Constraints held

- **No alloc and no panic in audio.** Modes and paths are matched once a block. Indices are const or clamped, and clamps are `vmaxnm`/`vminnm`.
- **Never snapped.** Every coefficient, gain and tap amount ramps across the block. TYPE and MODE warm-start and crossfade over two blocks. The LEVEL clipper fades over 20 ms.
- **Old patches load safely,** but needn't sound the same:
  - LP1, NOTCH1 and NOTCH2 change sound, and the top octave moves with the new `tan`;
  - DRIVE translates to LEVEL on load (`2^(4·LEVEL − 2) = 1 + 4·DRIVE`), so levels hold exactly up to DRIVE 0.75. Above that LEVEL clamps at 1 (×4), so a patch at DRIVE 1 loads 1.94 dB quieter;
  - a translated patch with DRIVE > 0 now passes through LEVEL's clipper, so material above ±1 gets some grit it didn't have;
  - a route into DRIVE keeps its amount, but under the exponential law that amount moves the level by a different number of dB;
  - goldens are re-recorded and listed in the PR;
  - all new params, codes and `TapRoutes` are append-only (DRIVE's id 2 retired, LEVEL new at 12), and unknown codes load as CLEAN LP4.

## Tests the build must add

1. **Frequency response per (type, mode)** at FREQ 250 Hz, 1 kHz and 4 kHz, RES 0, against the bilinear-mapped analog prototype:
   - slopes over 2–4 × FREQ: LP1 −6 ± 1.5, LP2/HP2 ±12 ± 2, LP4/HP4 ±24 ± 3 dB/oct;
   - LP1 −3 dB at FREQ ± 3 %;
   - BP's peak at FREQ ± 3 %;
   - CLEAN NOTCH1's zero at 2 × FREQ ± 3 %, at least 30 dB deep, with its −8.6 dB dip at FREQ at Q 0;
   - DIRTY NOTCH1 at FREQ ± 5 % up to fs/8;
   - the 2040's −3 dB point within ±5 % of the linear prototype at a small input.
2. **Self-oscillation, level and 1 V/oct** at RES 1, for CLEAN (every mode but NOTCH2), the 2040 and DIRTY. Each is started by one impulse and read over 0.5 s after 0.25 s.
   - The level is steady (±1 dB) and below 0 dBFS in every mode, across FREQ 100 Hz – 8 kHz (M-new-1). CLEAN's trimmed modes, the 2040 and DIRTY share one pinned band, so a TYPE switch at RES 1 jumps no more than that band's width (H-new-1).
   - With FM2 at +100 %, notes C2–C6: each octave is within ±10 cents and the absolute pitch within ±25 cents, up to 5 kHz for CLEAN and the 2040, and up to fs/8 for DIRTY.
   - Onset sits at RES 0.95 ± 0.01 on each type.
3. **Click-free sweeps** with `tests/common/sweep.rs` (`Tape::click`, `CLICK_RATIO`, `CLICK_FLOOR`) and `common::clicks`:
   - FREQ 20 Hz → 20 kHz → 20 Hz over 1 s, and Q 0 → 1 → 0, on a held saw, for every (type, mode);
   - the same as UI steps through `Block::set`;
   - QM driven by a stepped LFO square;
   - FM3 IN and QM IN repatched mid-note;
   - LEVEL 0.5 ↔ 0.51 on a full-scale saw with no step (the clipper's gate and fade);
   - a DRIVE-only file loads at the same level as before, within 0.1 dB, for DRIVE ≤ 0.75 (the H-new-4 translation); above 0.75, at most 1.94 dB quieter, at the ×4 ceiling.
4. **One-block jumps (B1):** FREQ 100 Hz ↔ 20 kHz in one block, and a 1 ms ENV 1 → CUTOFF at +100 %, at RES 0 and RES 1, on every type, against a per-sample reference in f64 that uses `tan_accurate` and exact divides. At RES 0 the max deviation must be ≤ 1e-3 relative. At RES 1, where self-oscillation makes relative error meaningless, the check is boundedness: no output exceeds the steady bound (N4).
5. **TYPE and MODE changes (B5):** every TYPE change and every MODE change, mid-note on a held saw. Measured from the end of the crossfade (N4), the output's max deviation from the destination's own steady output must be ≤ 10 %, and it must click at no point.
6. **NOTCH2:** white noise, averaged spectrum.
   - There are exactly two minima at least 25 dB deep, at the digital formula's 0.414 × and 2.414 × FREQ (±3 %), for FREQ 200 Hz – 1.6 kHz in octaves.
   - A doubling of FREQ moves both up one octave (±0.05).
   - The notches hold at Q 0.5 and 1.
   - The peak near FREQ rises with Q by ≤ 5.2 dB.
   - For FREQ ≤ 2 kHz, broadband loudness stays within ±1.5 dB across Q (L-new-1).
   - It never self-oscillates.
7. **The 2040's bass loss and resonance at level (H-new-1):** the gain at 100 Hz with FREQ 2 kHz, for RES 0–1 in tenths, at a small input **and at full scale**. It falls monotonically and stays within ±1 dB of `−20·log₁₀(1 + k)`. At RES 0.95 (`k = 4`) it is −14 ± 1 dB, at full scale included. The resonant peak's gain at full scale for RES ≤ 0.8 stays within a pinned number of dB of the small-signal gain.
8. **Bounded at every setting:**
   - a grid of FREQ (24 log points) × RES (11) × LEVEL (0, ½, 1) × every mode of every type;
   - inputs: silence, a full-scale saw, a square at fs/4, white noise ±1, and impulses of ±10;
   - plus 10 s of random per-block changes of FREQ, RES, MODE and TYPE.
   - Every output is finite. DIRTY's output stays within ±1 and its states within ±2. CLEAN's and the 2040's bounds are pinned at build.
9. **Taps:**
   - **Sidebands at the op ratio.** An ALGO Sound with OP 6 at ratio r, its tap → VCA at 50 % and at 100 %, and a sine carrier at f_c: the sidebands at f_c ± r·f_note must sit within ±1 dB of the AM/RM theory. With the amount at 0 they must be under −90 dB.
     - Tap → FREQ on LP2 with a saw: sidebands at each harmonic ± r·f_note, and none at amount 0.
     - Tap → FOLD: the fold's spectrum moves at r·f_note.
   - **No aliasing blow-up.** Every type and mode, with tap → FREQ and → Q at 100 %, OP 6 at 8 kHz and an inharmonic op ratio (√2), FREQ at 10 kHz: the output is finite and bounded as in test 8. Energy off the expected bins (the carrier's harmonics ± multiples of the op frequency) is at most a pinned margin above the tap-free filter's (N4).
   - **No DC from the tap-VCA (H-new-2).** A carrier tapped onto the VCA at 100 % (OP 6 as carrier, 1:1) leaves output DC under −80 dB after the blocker settles.
   - **Click-free enable and disable.** The tap amount goes 0 → 100 % → 0 in UI steps, and the tap source is repatched mid-note, under the click detector.
   - **Scope.** A tap cannot be routed to an operator pitch or level, or to any destination but the four (pure test on `TapRoutes` and the view). Taps are inert on non-ALGO Sounds, where a jack on OP 5 or OP 6 acts as its default source. They never hold a voice.
   - **Units.** A Mod → Tap → Mod repatch keeps the depth in octaves, within one amount step.
10. **Cost:**
    - `filter_bills_each_type_and_mode` and `taps_bill_each_destination`, including FOLD_COST for a FOLD tap with FOLD off;
    - the probe disassembly shows 0 `vdiv` per steady loop;
    - each constant's count is in its doc comment.
11. **Pure tests:**
    - `default_mode() ∈ modes()`;
    - `mode ∈ modes()` after every write;
    - labels fit their cells;
    - a TYPE change leaves the `ModState` byte-identical;
    - `repatch` moves exactly one route and refuses an occupied cell;
    - load without FM3 IN or QM IN infers the source from the routes;
    - codec round-trips of KIND 4 and 6, MODE 9, Filter 8, 11 and 12, `Jack` 0–9, and `TapRoutes` entries, with invalid entries dropped;
    - unknown codes load as CLEAN LP4;
    - the `ModSource` and `Jack` codes fixtures;
    - no `ModSource::ALL` or `index()` caller ever sees a tap.
12. **LEVEL's LED:** 0 while the clipper is off and for any signal ≤ 1; it rises monotonically with LEVEL on a full-scale saw; it is the Part's maximum over its voices; screen goldens at 0, ½ and 1.

## Build order

The build starts after `modal-stagger` merges.

1. Count today's LP24 loop. Then ACCURATE `tan`, steady and moving paths, the `k` law with self-oscillation, and `d₂`. Goldens are re-recorded.
2. LP1 (`lp + bp`) and NOTCH1.
3. NOTCH2.
4. LEVEL (new id 12, DRIVE's translation), its clipper, fade and LED.
5. The Cascadia panel, labels, FM3 IN and QM IN, and route delete.
6. TYPE and MODE warm start and crossfade.
7. The 2040.
8. DIRTY.
9. ALGO taps.
10. Costs and `cost_test`.

Each task gets desktop QA (sweeps, per-pair levels) before the owner's ear.

## Out of scope

- The other kinds: MOOG private issue #123, MS-20 private issue #124, SH-101 private issue #125, TB-303 private issue #127.
- Taps on engines other than ALGO.
- Taps from other operators.
- Taps to other destinations.
- The Cascadia's always-on LP4 and HP4 outputs.
- DRIVE and FOLD with PRE/POST (private issue #278): LEVEL here is the filter's own input.

## Open questions for the owner

None. Every question from revision 1 is answered above. Two constants are tuned at build against their tests and are not taste calls: the 2040's limiter `L`, and `OUT_2040`.

## Sources

- Intellijel, *Cascadia Manual*, firmware 1.2, rev. 2023-10-15, § 8 "VCF" (pp. 44–48). Behaviour and UX only. https://intellijel.com/downloads/manuals/cascadia_manual_v1.2_2023.10.15.pdf
- Solid State Music, *SSM 2040 Voltage Controlled Filter Circuit*, data sheet, July 1977: the block diagram, Figs. 1, 9 and 10, and THD vs input level. http://bitsavers.trailing-edge.com/components/solidStateMusic/SSM2040.pdf
- RSF, *Kobol Expander* factory schematic (Toulouse, 1979): p. 8 (VCF, CI4 and CI5, T3) and p. 11 (BOM: CI4 "SSM 2040", CI5 "CA 3080"). Reference only, not redistributable, and kept out of the repo. Scan: https://yusynth.net/gear/RSF/schema-RSF-KOBOL.pdf
- R. Schmitz, "RS2040", a discrete SSM2040; Behringer's 2023 clone follows it. https://www.schmitzbits.de/rs2040.html
- Synth DIY Wiki, "SSM2040" (https://sdiy.info/wiki/SSM2040) and "Sequential Circuits Prophet-5" (https://sdiy.info/wiki/Sequential_Circuits_Prophet-5).
- T. Wiltshire (Electric Druid), "Multimode filters, Part 1: Reconfigurable filters". https://electricdruid.net/multimode-filters-part-1-reconfigurable-filters/
- Synthtopia, on the SSI2140 reissue and its Q compensation (2020). https://www.synthtopia.com/content/2020/05/27/sound-semiconductor-intros-updated-version-of-classic-ssm2040-analog-filter-chip/
- V. Zavalishin, *The Art of VA Filter Design*, rev. 2.1.2 (2020). https://archive.org/details/the-art-of-va-filter-design-rev.-2.1.2 (rev. 1.1.1: https://www.native-instruments.com/fileadmin/ni_media/downloads/pdf/VAFilterDesign_1.1.1.pdf)
- A. Simper (Cytomic), "Linear Trapezoidal Integrated State Variable Filter". https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf
- A. Huovilainen, "Non-Linear Digital Implementation of the Moog Ladder Filter", DAFx-04. https://dafx.de/paper-archive/2004/P_061.PDF
- stmlib `dsp/filter.h`, `OnePole::tan<FREQUENCY_FAST | FREQUENCY_ACCURATE>` (MIT). https://github.com/pichenettes/stmlib/blob/master/dsp/filter.h
- H. Chamberlin, *Musical Applications of Microprocessors*, 2nd ed. (1985), ch. 14.
- J. Dattorro, "Effect Design, Part 1", JAES 45(9), 1997. https://ccrma.stanford.edu/~dattorro/EffectDesignPart1.pdf
- V. Lazzarini and J. Timoney, "Improving the Chamberlin Digital State Variable Filter" (2021). https://arxiv.org/pdf/2111.05592
- Chimera:
  - the filter-routing spec;
  - ADRs 0009, 0010, 0026, 0035, 0036, 0037, 0051, 0056, 0061, 0086, 0089;
  - issues #53, #57, #123–#128, #165, #177, #278.
