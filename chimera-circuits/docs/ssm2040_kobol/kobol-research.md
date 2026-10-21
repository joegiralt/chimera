# Kobol Expander filter vs Chimera 2040 EXP (research, read-only)

Date 2026-10-07. Our model read from origin/main (k2040.rs, ADR 0096, filter-types spec rev 4).
Tags: [SCHEM] = read off the RSF schematic by me (https://yusynth.net/gear/RSF/schema-RSF-KOBOL.pdf, p.8, 15/6/79,
page is scanned upside-down); [ADR] = taken from our ADR/spec, which cites the same schematic; [HEARSAY] = forum/press.

## 1. What the real filter is

- Chip: a real SSM 2040 (BOM p.11 "CI4 SSM 2040", per ADR 0096 [ADR]; schematic p.8 shows the 4-cap chip with R-ladder
  on pins [SCHEM]). Not discrete, not CA3080 poles. Corroboration: Muzines/E&MM 1983 review, "standard 24dB/octave
  voltage controlled lowpass", controls Cutoff, Resonance, ADS2 amount, Keyboard amount, audio in
  (https://www.muzines.co.uk/articles/rsf-kobol-expander/5929); sdiy wiki SSM2040 page (https://sdiy.info/wiki/SSM2040);
  Synthtopia comment thread "a huge part of the Kobol sound is the SSM2040 VCF" [HEARSAY]
  (https://www.synthtopia.com/content/2023/06/14/behringer-says-their-copy-of-the-rsf-kobol-aka-the-french-minimoog-now-scheduled-for-production/).
- Poles/modes: 4-pole LP only. No LP/BP/HP morph, no mode or slope control on the Expander. Nothing "missing" in modes.
  (Press: Muzines above; Wikipedia RSF_Kobol only says "24db low pass".)
- Resonance: feedback from the chip's OUT4 via C6 (4.7 uF) into a CA3080 (CI5), current set by T3 (2N3906) from the RES
  pot [SCHEM+ADR]. That 3080 is the resonance VCA (the Prophet uses half an SSM2020 here, per data sheet Fig. 9).
- Post-filter: R31 10 K into a second CA3080 (CI3, "VCA lineaire"), biased by T2/R47 from the ADS-VCA envelope
  chip (SSM2050), then CI2 opamp, trimmer "Cont Rej" (feedthrough null), 4.7 uF out [SCHEM]. So CI3 is the
  instrument's amplitude VCA, whose input stage saturates, not a dedicated saturator. Our model treats it as a static saturator, which is the right part to keep.
- Input: R24 100 K into stage 1 vs the Prophet's 200 K [ADR]. The mixer level feeding it is illegible on the scan,
  so "2x drive" is an assumption (ADR says so itself).
- Cutoff CV: 1 V/oct exponential converter, opamp + 2N3906 (CI6/CI7, T4/T5) summing Freq knob, ADS2, keyboard [SCHEM].
  Envelopes are SSM2050 ADS generators [SCHEM]; shape not modelled by us (it's the matrix's ENV).
- Bass loss: inherent (DC gain 1/(1+k)); nothing compensates it [ADR; consistent with sdiy: some modern clones add compensation, original does not]. Fine to leave out.
- Clones: Behringer Kobol Expander 2023 uses a discrete SSM2040 (matched pairs + V3046 array) and an LM13700-style OTA for
  voltage-controlled resonance; reviewers say "smooth oscillation", "grittier than any 2040" [HEARSAY]
  (https://amsynths.co.uk/2024/01/19/behringer-kobol-expander/). Schmitz RS2040 is a Haible-derived SSM2040 clone
  without VC resonance (https://www.schmitzbits.de/rs2040.html), so it says nothing about the Kobol's resonance path.
- Not found: any primary source for a distinctive Kobol resonance/limiter behaviour beyond the CA3080 (my deduction,
  and ours); forums I could reach (Mod Wiggler, Gearspace) returned 403. Treat "natural saturation" as [HEARSAY].

Topology verdict: yes, a genuine SSM2040 LP4, resonance through a CA3080, output through a second 3080 VCA. Our block diagram matches. Whatever is missing is not topology; it's calibration or things around the filter.

## 2. Our EXP, in one line each (from source)

TPT stages, per-stage `sat` (C1 cubic) on the true error, +/-R clamp (R = 0.2 model units = 1 V), linear one-step loop,
EXP resonance limiter `L=0.25`, ALPHA_EXP 0.2, output `sat(beta*y4)` beta = 0.62/R, k = 4*RES/0.95, no compensation.

## 3. Differences, ranked by likely audibility

1. Per-stage OTA nonlinearity is probably ~3x too small in argument (~10x in distortion) at the chip's own swing.
   [my arithmetic, verify] Real stage: 220 ohm attenuator against 10 K gives about +/-21.5 mV at the OTA for a 1 V error, 0.41 of 2V_T
   (same 0.42 ADR 0096 uses for CI3). Our `sat` is the cubic c - 4/27 c^3, i.e. a tanh scaled so c = 1.5x; CI3 is mapped with 0.62 at 1 V
   (1.5 x 0.416), but the stage OTAs get only R = 0.2 for the same 1 V. So the stages bend 0.2 where the same circuit logic gives 0.62.
   At 1 V swing: model compression ~0.6 %, tanh-at-0.41 ~6 %. ALPHA_REV2 was "tuned with L" (tests 7/2), not derived, so
   this may have been absorbed there. If the stage bend is wrong, EXP loses the "dirty/liquid" grit that is the chip's contribution
   and that the owner described as "so dirty". Fix: derive stage argument from the circuit, 1 V -> 0.62 in `sat` units
   (keep the +/-1 V clamp), then retune ALPHA and L (they were tuned against the weak stage); resonance level and bass-loss tests will move.
   Cost: zero cycles (constants), a retune day. Also Fix risk: REV2 shares the stages; Prophet at 0.5 V per pole is also bent 3x more, so make it
   per-voice only if REV2 is already "right" by ear.

2. Drive level into the chip is unverified; real Kobol saw/mixer may hit stage 1 much harder than 2x Prophet. Unreadable mixer value
   (ADR admits). The Kobol "grittier" reputation [HEARSAY] is consistent with higher drive. Our unity (LEVEL 0.5) already puts EXP at
   the swing, but Chimera's engine outputs are not Kobol-oscillator levels. Fix: not a model change; offer LEVEL range / EXP default
   LEVEL > 0.5, or measure a real unit. Cost 0.

3. Resonance 3080 limiter: ours is a hard-ish static `L*sat(./L)` on the summed loop signal. Real CI5 is a differential pair
   driven single-ended from a 4.7 K/220 ohm divider (0.87 of 2V_T at 1 V), its gm set by T3 current, and in a real pair the nonlinearity bends
   gm *and* adds input-offset/even harmonic through imbalance; the clone uses an LM13700 for the same job. Possible audible gaps:
   the squelch curve shape (tanh vs our cubic with flat top at 1.5), and RES law (3080 gm is linear in Iabc, fine). Fix: swap cubic for
   a real tanh-shaped limiter (e.g. rational Pade) inside `L*sat()`; cost ~ +8-12 cycles/sample (one divide-free rational), so EXP ~145.

4. Resonance feedback has its own phase: C6 4.7 uF HPF (6.9 Hz) is dropped (sub-audio, fine) but the 3080's output is a current into stage 1's
   summing node through the same 220 ohm attenuator, so the loop passes through the input attenuator's own nonlinearity
   (resonance bends stage 1's error as well as the signal). Our loop adds `u` before stage 1 whose OTA already sees error x-y; similar.
   Probably inaudible. Cost 0 to check, +4 cycles for a 6.9 Hz loop HP if wanted.

5. Envelope and keyboard context: "snappy" [HEARSAY] is as much SSM2050 ADS envelopes (exponential-ish segments, strong ADS2 amount)
   and 1 V/oct exact tracking as the filter. Our ENV 1 -> FM1 shape and amount law decide whether EXP sounds like the instrument.
   Fix: out of the filter (envelope curves, default patch). Cost: a few cycles per control block.

6. CI3 VCA is the amp VCA with the ADS envelope on its gm; saturation is at its input and independent of VCA gain, so a faded
   note stays crunchy while quiet. Ours: post-stage saturator before the amp env? Check order: our `sat(beta*y4)` is in the filter, then
   the envelope VCA, same order: OK. Missing: "Cont Rej" trimmer feedthrough and small DC/asymmetry (even harmonics): negligible.

7. Not differences (checked): modes (LP4 only), pole count, bass loss, output coupling (3.3 Hz), 2V_T mapping of CI3 (0.62).

## 4. Recommendation

Primary suspect is #1 (stage nonlinearity under-scaled) plus #2 (level). Before changing anything: A/B EXP against the Behringer/clone at
stimuli per chimera-utils, with a bypassed output stage; sweep stage sat factor 1x, 2x, 3x. Cheapest probe: set the stage sat scale to
3x (c scaled by 3.1 = 0.62/0.2) in a branch and listen. No cycle cost. None of this needs new modes.
