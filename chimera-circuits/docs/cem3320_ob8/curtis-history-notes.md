> **History only.** The CURTIS lab (branch `curtis-lab`, cec352e9) voiced the CEM3320 on the 2040 REV2's structure with a resonance limiter. The owner rejected that model, and it is not in the crate; only its stimuli (`cem3320_ob8::stimuli::{bassline, stab, pad}`) came over. The chip-level model that replaced it is `research.md`. `dl/` names the ledger's downloads (service manuals, data sheet), which are not in the repo.

# CURTIS (CEM3320, OB-8 / OB-Xa 4-pole) vs 2040 REV2 and LADDER: lab notes (#720)

This is desktop only. Nothing went to the unit or firmware, and there is no PR. The sources and derivations are in `research.md`. The lab code is on branch `curtis-lab` (worktree (private notes), committed locally, not pushed): `chimera-utils/src/curtis.rs`, commands `chimera-utils curtis-ab | curtis-drive | curtis-cost`, env `CURTIS_OUT`. The branch carries the `circuit-2040` lab commits for their stimuli and helpers.

## What is in `ab/`

Files are `<stimulus>/r<RES×100>_<drive>dB_<a_curtis|b_rev2|c_ladder>.wav`, 32-bit float, 48 kHz, mono.

- **a_curtis** is the candidate: f32, 1×, the house's 2040 structure on the CEM3320's numbers (below).
- **b_rev2** and **c_ladder** are today's 2040 (VOICE REV2) and LADDER (LP4), each through the real `Filter`, LEVEL ½.
- **Stimuli:**
  - `bassline`: 16 sixteenths at 120 BPM, one saw, per-note filter envelope 180 Hz → 2.9 kHz.
  - `stab`: two A minor 7 stabs, two saws a note, FREQ 500 Hz → 4 kHz.
  - `pad`: F A C E G, two saws a note, FREQ 1.1 kHz.
  - `cutoff_sweep`: 110 Hz saw, 40 Hz → 15 kHz in 5 s.
  - `res_sweep`: 110 Hz saw at 800 Hz, RES 0 → 1 over 4 s, then the loop rings alone from 4.5 s.
- **Settings:** RES 0, 0.5 and 0.85, at the same loop gain `k = 4·RES/0.95` on all three. RES 1 differs: CURTIS stops at k 4.05, the others at 4.21 (see "Resonance").
- **Level matching:** the input drive is undone at the output and every file is ×½, as in the 2040 lab. All three are unity at DC with RES 0; the rms columns in `table.md` agree within 0.1 dB at −12 and 0 dB. **So any loudness change you hear across the drives is the filter compressing, not gain.** Every file has a 15.9 Hz output blocker (the OB-8 has 21 Hz: CX21/RX62).
- **0 dB** is full scale = 3 V at stage 1's output-equivalent, half the chip's ±6 V swing (REV2 also runs at half its swing). On the OB-8 that is about both oscillators' saws at max in 4-pole: the bus is shunted by RX40 22 K on the 4P switch, my reading.
- **Start with:**
  - `bassline/r50_+12dB` a vs b
  - `stab/r50_+12dB`
  - `res_sweep/r100_+00dB` (the ring's onset)
  - `pad/r85_+12dB`

## The model (what CURTIS is)

The house structure: four TPT one-poles, the global loop solved linearly in one step, each stage's nonlinearity on its linear error, the state clamped. The CEM3320's own parts:

1. **Class A cells.** The cell is current-in, current-out: `(I_REF − I_IN)·gain`, its input a diode node. It is linear in current until `I_IN` reaches zero, then it starves: the stage slews one way only.
   - The starved side alternates stage by stage. Model: linear above −0.8, a C1 knee, floored at −1 (1 unit = 5.73 V of stage error).
   - Its knee (5.73 V) sits at the swing (±6 V). The 2040's OTA knee (2.7 V) is far outside its ±1 V swing.
2. **The buffers' ±6 V swing,** a hard clamp, as REV2's.
3. **The resonance OTA as a limiter** on the predicted feedback (EXP's 3080 form), `u = x − L'·tanh((x − u_l)/L')`.
   - Its knee is the OTA's at pin 8, through the OB-8's 130 K/3.6 K divider: from 2 V at the output at low RES to 8 V near the onset (the data sheet's fig. 6 fitted as a degenerated pair).
   - At low and mid RES the feedback saturates at normal levels.
4. **RES:** the house law to the onset, then only k 4.05 at RES 1. The OB-8 sits ~1 % past the onset, where the OTA, not stage 1's clip, sets the ring.

## How it differs, in numbers (`ab/drive.md`, `ab/table.md`)

### At normal level (−12 / 0 dB) it is the same linear four-pole as REV2

- Third-octave level difference (LSD) vs REV2 is 0.03–0.25 dB on every musical job at −12 and 0 dB. Vs LADDER it is 0.04–1.1 dB, and 2.0 on the 0 dB r85 cutoff sweep, where LADDER's input pair is already bending.
- Small signal: the RES 0.85 peak at FREQ 1 kHz, saw harmonic 9:
  - CURTIS **+3.2 dB**
  - REV2 +2.0 (its cubic stage trims the peak)
  - LADDER +4.5
  - The 2040 lab's ngspice Prophet circuit read +3.2, the same linear-cascade value.
- Passband sine (220 Hz, FREQ 2 kHz, RES 0) is clean on CURTIS and REV2 to +6 dB. Both clip at the same point (+9 dB: −1.7 dB compression, H3 −19 dBc) because both clamp at twice full scale. LADDER bends from 0 dB on (H3 −32 dBc at 0 dB).
- **Even harmonics from starvation are small** with these stimuli. The cell starves only on fast edges in one direction (saw resets, the CEM3340's falling ones). It trims the highest partials, about 1 dB at +12 dB, H9 −33.2 vs REV2 −32.3, with no audible H2 at these levels. At nominal level the CEM3320's "second harmonic" is the diode node's ~0.1 %, which the model leaves out (−60 dB).

### Resonance under drive: CURTIS squashes and darkens harder (the OTA)

- 110 Hz saw, FREQ 1 kHz, harmonic 9 (on the peak) re H1, from −18 to +12 to +18 dB:

  | RES | CURTIS | REV2 | LADDER |
  |---|---|---|---|
  | 0.5 | −14.5 → −26.5 → −32.6 | −14.7 → −23.9 → −28.9 | −14.0 → −30.0 → −30.9 |
  | 0.85 | +3.2 → −6.7 → −27.2 | +2.0 → −5.1 → −17.9 | +4.5 → −12.6 → −17.0 |

- Centroid at RES 0.5: CURTIS 311 → 206 → 147 Hz; REV2 310 → 217 → 163 Hz.
- So CURTIS starts ducking at ~0 dB (H9 −15.0 vs −14.7 at RES 0.5). REV2 starts at +6. At +18 CURTIS's peak is 9 dB further down than REV2's at RES 0.85. LADDER ducks earliest (its input pair), from −6 dB on.

### Bass loss shrinks with level (only CURTIS)

- A 55 Hz sine under FREQ 1 kHz at −12 / 0 / +12 dB:

  | RES | CURTIS | REV2 | LADDER |
  |---|---|---|---|
  | 0.25 | −6.6 / −6.3 / −5.4 | −6.6 at every level | −6.6 / −6.6 / −6.5 |
  | 0.5 | −10.2 / −10.1 / **−8.5** | −10.2 flat | −10.2 / −10.2 / −10.1 |
  | 0.85 | −13.5 / −13.5 / −13.4 | −13.5 | −13.5 |
  | 0.95 | −14.3 | −14.3 | −14.3 |

- The 110 Hz saw's fundamental at RES 0.5 rises from −9.8 dB to −9.3 at +12 and −8.7 at +18. REV2 goes −9.8 → −9.9 → −10.1.
- The OTA saturates on loud content, the feedback stops growing, and **the bass comes back as you drive it**, by up to 1.7 dB at low and mid RES. Near the onset its knee is high (8 V), so high-RES bass loss is the plain 1/(1+k) of every four-pole.
- This is CURTIS's own bass-loss behaviour, and no other TYPE has it: REV2's 2020 is linear, and EXP's 3080 limits at a fixed level.

### The ring grows gently

- RES 0.97 from a click: CURTIS rings at **−18.9 dBFS**, REV2 −2.9 (already at its clip), LADDER −16.3.
- RES 1: CURTIS −3.0 dBFS at −9 cents, REV2 −2.9 at 0 cents, LADDER −12.7 at 0 cents.
- So the OB-8's sits just past the onset and swells over the last 5 % of RES (the data sheet's "advancing further will merely increase the amplitude"). It is a sine that only just reaches stage 1's swing at RES 1. REV2 jumps straight to its clip.
- Under a 0 dB saw all three sag to −17 cents.
- **Pitch:** CURTIS needs a TRIM of about +9 cents (REV2 has one, LADDER +21).

### Is 1× enough?

Yes. CURTIS 1× vs the same model at 8×: LSD 0.01–0.26 dB on every job (worst on the cutoff sweeps at 0 dB, from the top octave).

## Controls (as a TYPE)

- **KIND CURTIS** with the standard cells:
  - FREQ, RES and LEVEL. LEVEL is the drive into stage 1. Its LED lights as stage 1 nears the ±6 V swing (as on the 2040, about LEVEL 0.75 on a full-scale input).
  - Keytrack, FM, the character slot and FB, as every TYPE.
- **MODE:** LP4 only. The OB-Xa/OB-8 12 dB mode is a separate 3320 state-variable filter. That is SEM's ground, and SEM is the next TYPE in #720, so the OB 2-pole sound lives there. LADDER-style Xpander taps (LP2/BP/HP from y₀…y₄) would cost nothing extra if wanted later, but no OB has them.
- **No VOICE.** The Prophet-5 rev 3 uses the same chip, but with its resonance tapped after a ×3.4 output buffer (Electric Druid). That would be a constants-only VOICE if ever wanted. It is not proposed ("no more filters").
- **RES taper:**
  - For the A/B it is the house law to the onset, then +1 % to RES 1.
  - The chip's own fig. 6 taper (a linear RES CV into the OTA: `k = 5.39·r/(r + 0.33)`) would put k 3.25 at RES 0.5 and leave very fine control at the top.
  - That taper is a by-ear choice for the owner. It is not adopted here, so the A/B compares like with like.

## STABILITY and FB

- **STABILITY (#714)** applies unchanged. CURTIS reads `g` and `k` through `mods.spread`, as the 2040 and LADDER do.
  - It is also authentic: the OB-Xa manual calibrates each voice's filter by ear against a reference voice (initial frequency and V/oct trimmers, no resonance trim on the 4-pole).
  - The 3320 is temperature compensated, apart from 1/V_T. Its I_REF ±25 % and A_IO 0.7–1.3 give voice-to-voice cutoff spread.
  - Use the 2040's spread and drift depths. Drift could go lower, since the 3320 is compensated (the Prophet's R479 is not).
- **FB (#393, ADR 0190)** applies as on the 2040: output → `g` per sample through `fed`.
  - The OTA limiter bounds the loop's own feedback, so FB's headroom guard has less to do. Keep it anyway: FB can still push the stages to the swing.

## Cost

- **Method:** the house static count. The steady inner loop is compiled for thumbv7em with `target-cpu=cortex-m7`, opt-level 2 (`m7probe/`, with REV2 and EXP restated beside CURTIS for calibration).

  | | loop instructions | vdiv | unit, measured | CURTIS estimate |
  |---|---|---|---|---|
  | 2040 REV2 | 109 (the 2040 lab counted 110) | 0 | 173 total | — |
  | 2040 EXP | 132 | 0 | 231 | — |
  | CURTIS | **109** | 0 | — | **~171** by REV2's ratio (1.57/instr); **~191** by EXP's (1.75) |

- **Estimate: ~175–190 cycles/sample/voice** total, i.e. **REV2's 173 + 0…+18**. The limiter costs what the cubic stage costs.
- Not yet counted:
  - L′ moving with RES needs a per-block divide plus a per-sample lerp (+2–4 instructions on the moving path).
  - The trim is free (a ratio on FREQ).
- Voices, default VA Sound (`c2040-cost` on main's `Voice::cost`): rev V keeps **8**, rev Y keeps **6**, at both +0 and +18 (rev V holds 8 up to +32).
- Host ns from `curtis-cost` came out noisy on the loaded machine (the 2040 Filter at 232 ns against the 2040 lab's ~46), so they are not used.
- A bench on the unit would settle the estimate. The unit was not touched.

## Caveats

- These parts are read off scans or assumed, not measured:
  - RX64 = 130 K (scan "13ØK")
  - RX40's 4P role
  - the OTA's tanh shape
  - the starvation knee's width
  - symmetric buffer rails
- The OB-Xa voice-card schematic is not in the service manual I found. The OB-8's 4-pole stands in for it, per the OB-8 manual's own statement.
- There is no circuit reference (ngspice) for the 3320: its log/antilog cell has no simple netlist, and circuit modelling is parked. So the numbers above compare models, not a model against the chip. A real OB-8 or OB-Xa recording (RES sweep; drive at one and two oscillators) would check the two things that make CURTIS CURTIS: the OTA limiter's level and the ring's onset.
- **What you will hear:** at −12 and 0 dB, CURTIS and REV2 are very close (the LSD above), apart from a slightly higher peak and a softer ring onset. The character is in the push and in the resonance:
  - a peak that melts away under drive
  - bass that comes back as you drive it
  - a ring that swells rather than snaps on
