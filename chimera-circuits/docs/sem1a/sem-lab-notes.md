# SEM lab notes (epic #720)

> Copied from the first SEM lab's ledger (private notes, branch `sem-lab`). Its code is now `chimera-circuits/src/sem1a/ota_svf.rs` (models) and `stimuli.rs` (stimuli). The `dl/` files it names stay in the ledger and are cited, not copied. The WAV sets are not kept.

Desktop prototype only: no firmware, no PR. Code was `chimera-utils/src/sem.rs` on branch `sem-lab` (def4f753); it is now `chimera-circuits/src/sem1a/ota_svf.rs`. Research and sources are in `sem-lab-research.md` (SEM-1A schematic, Zavalishin 2.1.0). Provenance: the schematic, the book, our own code.

## What was built

- **Candidate `Sem`**: f32 at 48 kHz, ×1, one block's coefficients ramped like CLEAN's moving path.
  - It is Zavalishin's TPT SVF, solved linearly (ZDF), with the circuit's three nonlinearities added cheaply:
    - **OTA integrators**: each integrator input goes through `V0·tanh(v/V0)`, V0 = 23.6 V (CA3080 behind 100 K/220 Ω). This is the tanh series to the fifth power, on a ±13.5 V rail clamp. It is "linearise at 0, push the solved HP and BP through" (Z. §6.6, §6.9).
    - **Diode antisaturator in the damping** (D11/D12 off a 47 K/10 K tap of BP; Z. Fig. 6.51): a divide-free C1 dead-zone, `0.154·φ(|bp| − 0.10 V)` with `W = 3.32 V`. Its worst error against the exact Shockley solve is 0.011 V over 0–13.5 V. It reads BP from the TPT state `s1` (the zero-input prediction of BP), so the linear solve keeps its single divide.
    - **A12's rail** on HP.
  - MODE output is `(1 − m)·LP + m·HP`. The BP switch takes BP instead.
  - Levels: digital 1.0 = 5 V at one SEM input. That is my assumption; everything about where the circuit starts to bend hangs on it.
- **Reference `Model`**: the same equations in f64 at ×4, trapezoidal, Newton on both caps, exact tanh, exact diodes, smooth rails. It is our own model of the schematic; there is no hardware capture.
- **CLEAN**: through the real `Filter`, LP2 / BP2, with MODE mapped to LP2 → NOTCH1 → HP4 in thirds for the sweep. Drive goes through CLEAN's own LEVEL (0, ½, 1 = −12, 0, +12 dB). SEM gets the same input gain.
- **RES law on SEM**: CLEAN's (`clean_k`), floored at `K_MIN = 0.04` (Q 25), so the knob feels the same on every TYPE. The circuit's own pot law, Q 0.33 at RES 0 rising to ∞ at the end (research §1.1), is not used.

**Cheap candidate vs reference**, over all 75 jobs: band-level difference (LSD) mean 0.17 dB, worst 0.54 dB; null mean −39 dB.
- The worst null, −7.8 dB, is the cutoff sweep at RES 1, +12 dB, near the top of the sweep. There the two differ in phase, not in spectrum (LSD 0.54 dB).
- At the cutoff the candidate reads within 0.3 dB of the reference at every probe level (table below).
- Of the two predictors for the diodes, `s1` beats last-sample BP slightly on worst null (−30.6 against −27.7 dB on the RES ≤ 0.85 set). `s1` is kept.

## The A/B set (the ledger's `ab/`, not kept)

The folder is 122 MB of 24-bit mono WAVs, each at −20 dBFS RMS. A set is lowered together only if one file would clip.
- **Layout**: `ab/<stimulus>/r<RES>_<drive>_<x>.wav`, where `<x>` is:
  - `a_clean`: CLEAN.
  - `b_sem`: the SEM as proposed, which never self-oscillates.
  - `c_ref`: the reference model.
  - `d_sem_osc`: the SEM with the oscillating floor. Every RES 1 job and every `res_sweep` job has one.
- **Stimuli**:
  - `bass`: A1 A1 E2 G1 saws, with the cutoff falling 2.6 kHz → 150 Hz on each note.
  - `brass` and `brass_bp`: a Cm7 stab of two saws a note, the cutoff opening 250 Hz → 3 kHz in 80 ms and settling at 1.4 kHz.
  - `pad`: Am7, three detuned saws a note, 4 s, at 900 Hz.
  - `mode_sweep`: the same chord held 6 s at 1 kHz, MODE 0 → 1.
  - `cutoff_sweep`: a 110 Hz saw, 40 Hz → 15 kHz over 5 s.
  - `res_sweep`: a 110 Hz saw at 800 Hz, RES 0 → 1 over 4 s, then the saw stops and the filter rings for 1.5 s.
- **Settings**: RES 0, 0.5, 0.85 and 1, at −12, 0 and +12 dB.
- **Tables**: `sem-lab-ab-table.md` has every job's unmatched RMS and the spectral distances. `sem-lab-probes.md` has the numbers below.

What to listen for: `mode_sweep` (it has no CLEAN equivalent), the RES 0.85 and RES 1 files at 0 and +12 dB (resonance under level), and `res_sweep` b against d (the self-oscillation choice).

## How SEM differs from CLEAN (measured)

Small-signal at low RES the two are the same transfer function. Up to RES 0.5, at −12 and 0 dB, the band levels differ by 0.0–0.2 dB. The differences are all at level, at high RES and in MODE.

**1. Resonance under level.** Gain at the cutoff, 1 kHz sine at fc 1 kHz, LP:

| RES | in | CLEAN | SEM | ref | THD CLEAN | THD SEM |
|---|---|---|---|---|---|---|
| 0 | −12 / 0 / +12 dB | −6.0 / −6.0 / −12.6 | −6.0 / −6.1 / −6.4 | −6.0 / −6.1 / −6.4 | 0 / 0 / 5.2 % | 0.01 / 0.05 / 0.20 % |
| 0.5 | −12 / 0 / +12 | +0.5 / +0.4 / −9.3 | +0.4 / +0.1 / −0.8 | +0.4 / +0.1 / −0.8 | 0 / 0.08 / 1.4 % | 0.03 / 0.11 / 0.32 % |
| 0.85 | −12 / 0 / +12 | +9.6 / −0.4 / −11.6 | +12.0 / +9.7 / +3.0 | +12.0 / +9.7 / +3.2 | 0.29 / 3.7 / 3.4 % | 0.11 / 0.23 / 0.54 % |
| 1 | −12 / 0 / +12 | +7.8 / −3.3 / −14.7 (oscillating) | +18.0 / +13.5 / +3.0 | +18.0 / +13.2 / +3.2 | 2.7 / 4.7 / 4.3 % | 0.20 / 0.42 / 0.57 % |

- **CLEAN** compresses by clipping: its states saturate at ±1–1.5 and LEVEL's clipper engages above unity. Loud signals lose 6–15 dB at the peak and gain 1–5 % distortion.
- **SEM** compresses by damping. The diodes widen the peak as it gets louder: at RES 0.85 the peak falls from +12 to +3 dB over 24 dB of input, and THD stays under 0.6 %. Loud resonance stays round instead of buzzing.
- At +12 dB, SEM is almost clean at RES 0. Its OTAs bend at most 9 % inside the rails, which is the circuit by design.
- **The drive character is therefore mild.** SEM has no "dirty" side at these levels, only the resonance squash. If the owner wants grit, the honest knob is more LEVEL range, not a different curve. Both conclusions depend on the 5 V assumption: halve it and everything shifts 6 dB.

**2. The resonance ceiling.** Time to fall 40 dB after a 2 ms burst at fc, RES 1:

| fc | CLEAN | SEM | SEM, K_OSC floor |
|---|---|---|---|
| 200 Hz | holds (oscillates) | 175 ms | holds |
| 1 kHz | holds | 35 ms | holds |
| 4 kHz | holds | 10 ms | holds |

The SEM rings and decays. That is the original: the diodes can only add damping.

**3. MODE is a moving notch, which CLEAN cannot do.** The deepest point, against the predicted fc·√((1−m)/m), at fc 1 kHz:

| m | notch measured (predicted) | depth | 100 Hz | 10 kHz |
|---|---|---|---|---|
| 0.1 | 2934 Hz (3000) | −55 dB | −1.0 dB | −20.7 dB |
| 0.25 | 1745 Hz (1732) | −48 dB | −2.6 dB | −12.3 dB |
| 0.5 | 1008 Hz (1000) | −47 dB | −6.2 dB | −6.2 dB |
| 0.75 | 582 Hz (577) | −51 dB | −12.4 dB | −2.6 dB |
| 0.9 | 336 Hz (333) | −57 dB | −20.9 dB | −1.0 dB |

The middle of MODE is −6 dB in both passbands. That is the circuit's linear pot, and in the sweep it reads as a dip in loudness: the sweep's RMS sits 2–3.5 dB under CLEAN's stepped modes. For firmware, either keep the dip (authentic) or add a trim of up to +6 dB at the centre. CLEAN's NOTCH1 is a different filter: a fixed zero at 2·fc with the treble −12 dB.

**4. Smaller differences.**
- SEM's BP is unboosted, so it sits 6 dB under CLEAN's BP2 (×2).
- SEM's HP end (m = 1) is 12 dB/oct. CLEAN's only HP is HP4, at 24 dB/oct.
- CLEAN trims its output as RES rises (LP2 by up to −6.5 dB at k ≤ 0). The SEM prototype has no trim yet, so at RES 0.85–1 it is 3–12 dB louder before matching. Firmware should pick a trim the way CLEAN's `trim` was picked.

## Self-oscillation: the decision

**Recommendation: keep the original. SEM never self-oscillates** (`K_MIN` = 0.04, Q 25).
- CLEAN, 2040 and LADDER already self-oscillate.
- A resonance that sings, compresses and lets go is what makes the SEM itself.
- The alternative, `K_OSC` = −0.06 (anti-damping held by the diodes, Z. p.212), gives a stable diode-limited sine that never decays. It is rendered as `d_sem_osc` for the owner to choose.
- If chosen, it would be the top few % of RES only (RES ≥ 0.98, where `clean_k` crosses −0.06). Nothing below that changes.

## Controls (KIND SEM)

- **CUTOFF**.
- **RES**: CLEAN's law floored at K_MIN.
- **MODE**: continuous, LP 0 … NOTCH ½ … HP 1. It is modulatable, so it is a param, not an enum; a route or LFO on MODE is the classic SEM move.
- **BP**: an on/off switch, which overrides MODE.
- **LEVEL**: as on CLEAN.
- **STABILITY** and **FB**: see below.

Disk: a new frozen KIND code, appended. MODE needs a new continuous param id, since the existing MODE byte is an enum.

## Cost (M7, static count, house method ceil5(instr × 1.606 + 12 × vdiv))

The crate was the ledger's `m7probe/` (not kept), compiled for cortex-m7 at opt-level 2; the assembly is `m7probe/sem.s`.

| Path | Instructions | vdiv | Cycles/sample/voice |
|---|---|---|---|
| SEM steady, diodes quiet | 54 | 1 per block | **90** |
| SEM steady, diodes conducting | 59 | 1 per block | **95** |
| SEM moving (a divide every sample) | 68–73 | 1 per sample | **125–135**, i.e. +35–40 over steady |
| CLEAN LP2 steady, same method | 24 | — | 40 |

- About 18 of SEM's instructions are clamps (vcmp/vmrs/vsel: the M7 has no vmaxnm). They were already cut from three clamps to the HP rail plus the OTA's.
- Against the engine's LP24 baseline (95), SEM's steady bill is about 0.
- These are probe counts. Re-bill on the bench ELF with the block overhead as usual.
- Host, for scale: 18 ns a sample for the SEM tick, against 9.7 ns for CLEAN's whole `Filter::process`.

## STABILITY and FB

- **STABILITY applies, and fits the instrument's history**: the polyphonic Oberheims were N separate SEMs, each with its own INIT FREQ trim (T11), and they drifted. ADR 0168's spread works on the block's ramp ends: `g × cut` for offset, drift and tracking, and `k × res` for the RES spread. Both are exactly SEM's coefficients. Today it is gated to the four-pole kinds (`four_pole()`), so it needs a gate for SEM.
- **FB applies**: the output into `g` per sample, run as the moving loop (a divide a sample) behind the bank-2 door, like `Clean::fed`. Budget it like CLEAN's FB (`FB_COST` 100).

## Not modelled

- The OTA offset trims (T9, T10).
- C22: the damping is AC-coupled at 14.6 Hz, so Q would rise for cutoffs under about 30 Hz.
- C15–C17: the inputs are high-passed at 4.8 Hz.
- The pots' tapers: the schematic doesn't print them; MODE is assumed linear.
