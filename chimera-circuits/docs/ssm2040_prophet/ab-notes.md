> Kept from the circuit-2040 lab ledger (private notes, 2026-10-20). Paths and commands below are the lab's, on branch `circuit-2040`; the models now live in `chimera-circuits/src/ssm2040_prophet/` (see `README.md`). The WAVs it names stay in the ledger.

# 2040 REV2 vs the Prophet-5 rev 2 circuit: A/B notes (#711, circuit 1)

Desktop only. Research and sources: `../research.md`. Lab code: branch `circuit-2040`, `chimera-utils/src/circuit2040.rs`
(`just`-free: `chimera-utils c2040-ab | c2040-drive | c2040-variants | c2040-cost`, env `NGSPICE`, `C2040_OUT`).

## What is in this folder

`<stimulus>/r<RES×100>_<drive>dB_<version>.wav`, 32-bit float, 48 kHz, mono.

| version | what |
|---|---|
| `a_chimera` | today's Chimera 2040, VOICE REV2, LEVEL ½, through the real `Filter` (unchanged main, 5c216cdd) |
| `b_circuit` | **the reference**: ngspice-45 on our netlist of the SSM 2040 wired as the Prophet-5 rev 1/2 (SD411): transistor pair per cell, 10 K/10 K/200 Ω nodes, 300 K input, ×−10 U479, SSM 2020 resonance current into stage 1 |
| `c_model` | the component model: the same node equations, TR-BDF2 + Newton at 2× (lab only, f64) |
| `d_fast` | the unit candidate: today's TPT structure on the circuit's numbers, f32, 2×, Padé tanh |
| `e_fast1x` | the cheap candidate: the same at 1×, divide-free quintic tanh |

- Stimuli: bass, stab, sine_short, sine_long, texture, hats, snare (the listening set, band-limited to 19 kHz, peak 1.0 = 0 dB), FREQ per stimulus (bass 600, stab 1500, sine_short 1000, sine_long 600, texture 2500, hats 8000, snare 2000 Hz); `cutoff_sweep` (110 Hz saw, 40 Hz → 15 kHz in 5 s); `res_sweep` (110 Hz saw at 800 Hz, RES 0 → 1 over 4 s, the saw stops at 4.5 s and the loop rings alone).
- RES 0, 0.5, 0.85 (k = 0, 2.1, 3.6; the same `k = 4·RES/0.95` on all versions).
- Drive −12 / 0 / +12 dB into the filter. **0 dB = full scale = the Prophet's maximum saw** (0.5 V peak at each pole, tech manual § 5-13).
- **Level matching:** none needed at the filter — all five are unity at DC, RES 0 (measured: equal to 0.1 dB on every RES 0 job below +6 dB). Every file has the input drive undone at the output (−12 dB files × 4, +12 dB files ÷ 4) and × ½ for headroom, so **loudness changes you hear across the three drives are compression, not gain**. All have the Prophet's 15.9 Hz output coupling (C413/R471).
- Start with: `bass/r85_+00dB_a` vs `_b`; then `sine_long/r00_+12dB`, `stab/r85_+12dB`, `cutoff_sweep/r85_+12dB`, `res_sweep/r100_+00dB`.

## Does the model match the reference?

`table.md` (every job: RMS, third-octave LSD and null of each version vs `b_circuit`), `../variants.log`:

| version | LSD vs circuit, mean / worst (dB) | at +12 dB mean | null median |
|---|---|---|---|
| a today REV2 | 0.65 / 5.13 | 1.34 | −30 dB |
| c component model | ~0.1 / 0.7 | 0.15 | −38 dB |
| d fast 2× Padé | 0.16 / 1.15 | 0.33 | −35 dB |
| d′ fast 2× quintic | 0.18 / 1.19 | 0.36 | −34 dB |
| e fast 1× quintic | 0.35 / 3.16 | 0.55 | −34 dB |
| e′ fast 1× Padé | 0.38 / 5.29 | 0.62 | −35 dB |

The component model tracks ngspice to within 0.1 dB in third-octave level on nearly every job (worst: hats at RES 0.85, 0.4–0.7 dB, where both are noise-dominated). Nulls of −40 to −53 dB on the tonal stimuli.

## How each one changes with drive (`drive.md`)

### Below the swing (−18 … +3 dB): all are nearly the same linear filter

- **Today's REV2 already matches the circuit at the Prophet's own levels.** At −12 and 0 dB the LSD vs the circuit is 0.02–0.4 dB on bass, stab, sines, texture and snare. That is the most important finding: **at nominal level the Prophet's 2040 is a clean linear 4-pole**, and REV2 is one.
- The OTAs barely bend there. The 200 Ω attenuator gives each OTA 1/52 of the stage error, so its tanh knee sits at 2.7 V of error while the signal is 0.5 V. Measured, 220 Hz sine in the passband at 0 dB: circuit H3 −97 dBc, H2 −153 dBc (REV2: H3 −114, no H2). With RES 0.85: circuit H2 −88, H3 −79 dBc (REV2 −149). Real, but 80–90 dB down: inaudible.
- Remaining small-signal differences, all small:
  - **High cutoff** (hats at 8 kHz): REV2 is 0.8–1.6 dB off in third-octave level (the 1× one-step solve at fc/fs = 1/6; the 2× models are 0.1–0.4 dB off).
  - **Resonance peak:** 110 Hz saw, FREQ 1 kHz, RES 0.85, harmonic 9 (990 Hz) re the fundamental: circuit +3.2 dB, REV2 +2.0 dB (the models +3.9: this harmonic sits on the peak, so it reads tuning to a few cents). Spectral centroid 683 Hz (circuit) vs 644 Hz (REV2).
  - **Self-oscillation** at RES 1, 1 kHz: circuit −6 cents, −3.9 dBFS; REV2 0 cents, −2.9 dBFS (1 dB louder). With a saw under it at 0 dB: circuit −17 c, REV2 −11 c.
  - **Bass loss at high Q** is the same on all: −13.1 dB at RES 0.85 (1/(1+k)), uncompensated, at every drive up to +18 dB.

### Pushed (+6 … +18 dB): this is where they differ

- **Where it starts:** the circuit starts to compress at **+4 to +6 dB** (each pole near ±1 V): 220 Hz sine, RES 0: −0.75 dB at +6 (REV2 −0.23); −2.9 dB at +9 (REV2 −1.9); −5.5 dB at +12 (REV2 −4.5); −11.3 dB at +18 (REV2 −10.2). So the circuit compresses about 1 dB more and ~3 dB earlier: its swing limit bends before the hard ±1 V rail REV2 clips at.
- **Even harmonics:** the circuit's clipping is lopsided (each cell's offset × 52 puts tens of mV of DC on the poles): H2 −38 dBc at +6 dB, −45 at +12, against REV2's none (perfectly odd). H3 is about the same on both once clipping (−28 / −13 dBc at +6 / +12 on the circuit; REV2 reaches −19 at +9, −14 at +12).
- **Resonance under drive:** input and feedback share stage 1's swing, so a hot input squashes the resonance. 110 Hz saw, FREQ 1 kHz, RES 0.85, H9 re H1 from −18 → +12 → +18 dB: circuit +3.2 → −6.6 → −24.2 dB; REV2 +2.0 → −5.1 → −17.9 dB. Centroid: circuit 683 → 331 → 192 Hz; REV2 644 → 374 → 207 Hz. Both duck; **the circuit ducks harder and darkens more** (about 1.5 dB more resonance loss at +12, 6 dB more at +18).
- **Bass under drive:** the 110 Hz fundamental holds within 0.1 dB at RES 0.85 on all (the loss is in the peak, not the bass); at RES 0, +12 dB: circuit −2.4 dB, REV2 −1.6 dB.
- **Does the cutoff move with level?** Not on its own: the OTA-knee "darker when louder" effect is negligible here (centroid at RES 0 stays 160 Hz to +6 dB on all). What moves is the resonance, through the shared swing (above). Self-oscillation pitch sags with a loud signal under it on all of them (−17 c with a 0 dB saw).
- **Today's REV2 at +12 dB vs circuit:** LSD 2.5–5 dB on the sines (missing H2, harder clip), ~1 dB on bass at RES 0, 0.8–1.7 dB on hats and texture at RES 0.85; elsewhere 0.1–0.4 dB.

## What it costs

M7 cycles are a static count of the inner loop (thumbv7em, `-C target-cpu=cortex-m7`, opt-level 2, `../m7probe`), billed the house way (instructions × 1.46 × 1.1 + 12 per `vdiv`). The unit measured today's REV2 at 78 cycles over `OLD_LP24` (95) where the static count gave 15, so static counts run low on this serial, latency-bound loop. Below, "calibrated" scales by REV2's unit-to-static ratio (173/110 = 1.57). It is a rough estimate. The unit was not touched; a bench run would settle it.

| version | static loop | +halfband up/down (2×) | calibrated total | vs today's 173 |
|---|---|---|---|---|
| today REV2 (unit-measured) | — | — | 173 | 0 |
| e fast 1× quintic | 113 instr, 0 vdiv → 181 | — | ~285 | **+~110** |
| fast 1× Padé | 111 instr, 4 vdiv → 226 | — | ~355 | +~180 |
| d′ fast 2× quintic | 222 instr, 0 vdiv → 357 | ~40 | ~620 | +~450 |
| d fast 2× Padé | 219 instr, 8 vdiv → 448 | ~40 | ~770 | +~600 |
| c component model (TR-BDF2, Newton, 2×) | ~3.7 Newton iterations × 2 stages × 2×, 4 tanh + 8 exp + 5 div each | | ≫ 8,000 | lab only |

Host (i7-13700K, one core, ns per output sample, filter loop only; `c2040-cost`): fast 1× 46, fast 2× 93, fast 4× 196, component model 837.

Voices, default VA Sound (2040 REV2; 749 cycles a voice, 78 of them the filter's bill), bus 1,204 + reserve 24, 8-voice cap (`c2040-cost` computes this from the real `Voice::cost`):

| | today | e 1× quintic (+110) | d′ 2× quintic (+450) | d 2× Padé (+600) |
|---|---|---|---|---|
| rev V (480 MHz, 6,272 for voices) | **8** | 7 | 5 | 4 |
| rev Y (400 MHz, 5,022) | **6** | 5 | 4 | 3 |

Rev V holds 8 voices only up to 784 cycles a voice (+35). A filter costing more than +35 loses a voice on rev V; on rev Y the limit is +88.

## Worth it?

- **What you would hear:** at the Prophet's own level (0 dB and below) today's REV2 is already the circuit to within a fraction of a dB. The things that differ there are small: the resonance peak is ~1 dB higher on the circuit, self-oscillation ~1 dB quieter and a few cents flat, and the top octave is cleaner at 2×. The audible difference is all in the push (+6 dB and up): the circuit compresses earlier and softer, has even harmonics from the cells' offsets, and its resonance ducks and darkens harder under a hot input. **Listen to the +12 dB files.** If those are "the Prophet", the drive behaviour is the thing to buy. If even `bass/r85_+00dB` a vs b sounds alike to you (it measures alike), then **what makes REV2 "not a Prophet" is not in the filter core**. Candidates: the 2030 oscillators, the fast 2050 envelopes, the untrimmed and temperature-drifting cutoff (R479 has no tempco), the voice-to-voice spread, or the mixer level the Prophet actually runs at.
- **Caveat:** the two things that make the circuit differ when pushed, the soft ±1 V swing limit and the cells' offsets, are the parts the sources do not pin down. The data sheet gives only "±1 V" and "2 mV typ". Our diode clamp and offset set are plausible, not measured. A real rev 2 recording (RES sweep, drive at 3 mixer levels) would confirm or correct exactly these.
- **Cost:** the faithful 2× candidate (`d`) costs 2–3 voices: rev V 8 → 4–5, rev Y 6 → 3–4. Not affordable as the VA default.
- **Cheaper variant that keeps most of it:** `e`, today's structure at 1× with the circuit's numbers: stage knee at the attenuator's scale, a soft swing limit instead of the hard clamp, the offsets, a divide-free tanh. It halves REV2's error against the circuit (LSD 0.35 vs 0.65 mean, 0.55 vs 1.34 at +12 dB). It is the only candidate that comes near the budget: ~+110 calibrated, so rev V 8 → 7 and rev Y 6 → 5. Its static count is only +71 over REV2's. Folding stage 1's own pole into the others and keeping the clamp branch-free should bring it to tens of cycles. It gives up the top-octave accuracy of 2× (hats 1–3 dB off, as REV2 is) and some precision in the hard-pushed resonance.

## Recommendation

1. **Owner's ears first, on `+12dB` and `res_sweep`:** a vs b. If the pushed circuit is the Prophet you miss, adopt `e`: a constants-and-limiter change to REV2 at 1×, budgeted as one VOICE (REV2 → circuit numbers). Then bench it on the unit, keeping it inside +35 cycles if 8 voices on rev V must hold.
2. **If a vs b at 0 dB sound the same to you** (they measure the same), stop on the filter core. Look outside it: the oscillators/envelopes, per-voice cutoff spread and drift, the mixer level. Ask for a real rev 2 recording when the hardware is back.
3. Don't ship 2× or the Newton model on the unit. They are references.
