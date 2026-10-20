# 0191. EXP keeps the Kobol's bass and rings over its input (supersedes in part 0096 and 0105)

- **Status:** Proposed
- **Deciders:** owner ("add the bass compensation to the 2040", on the Expander voicing only), with Claude (DSP)

## Context
On 2026-10-20 the owner recorded his RSF Kobol Expander through the MOTU (`kobol-session`). He held A1 from VCO 1 (56.4 Hz) and swept the cutoff at RES 0, at RES knob 5 and at RES max. With the cutoff open, the fundamental against RES 0 reads −3.3 dB at knob 5 and −6.8 to −7.9 dB at max (about −7). EXP, as ADR 0096 built it, follows the uncompensated 4-pole's `1/(1 + k)`: −8.3 dB at RES 0.4 and −13.9 at RES 1, full scale. RES 0.4 and 1 are the EXP settings the comparison matched to knob 5 and max by the bump over the bass (`compare/notes.md`).

The comparison also found the following:
- The Kobol's loss is flat from 56 Hz to 1.7 kHz, and its bump over the bass at knob 5 (+5.2 dB) is what EXP's loop gives at RES 0.4. So the Kobol behaves like a make-up gain that grows with RES, not like a shelf or a high-pass in the loop.
- At RES max the Kobol keeps ringing over its VCO, the ring 11 dB over the fundamental. REV2 rings +8 dB over a full-scale saw. EXP's 3080 limit (`L = ¼`) squelched the ring above about −12 dBFS of input.
- The Kobol's ring has H2 at −39 dBc and H3 at −44. EXP has no H2, and its H3 is at −60.

ADR 0096 chose the authentic bass loss for both voices. These measurements show that the Kobol does not lose that much bass. REV2 (Prophet) is not in question: the owner ruled that the change applies to EXP only, and REV2 stays bit for bit.

## Decision
- **Make-up gain, EXP only.** EXP's output is multiplied by `1 + m(k)`, where `m = k·(0.6 − 0.0752·k)` (`MAKE_UP_C1`, `MAKE_UP_C2`). The gain is applied after the 3080 output stage, outside the loop, and folded into the output scale. It is 1 at RES 0, +5.0 dB at RES 0.4 and +6.9 dB at RES 1. It is fitted so that a full-scale A1 at FREQ 9.4 kHz reads −3.3 and −7.0 dB against RES 0, the Kobol's curve. It costs nothing on the steady path, where it is applied once a block, and 5 instructions a sample on the moving path.
- **The limit is raised.** `L_2040` goes from ¼ to ½. EXP now rings over a full-scale saw (+5.5 to +5.7 dB over the fundamental, FREQ 1 kHz). Its self-oscillation reaches the ±1 V clip and sits at −5.1 to −5.3 dBFS, in REV2's band. Before this change it sat at −17.1 dBFS, in a band of its own. The compression under drive at RES 0.8 goes from −6.4 dB to −2.65 dB. EXP's limiter still squelches harder than REV2's linear 2020.
- **The trim follows.** The clip-bound ring runs 21–29 cents flat, so `TRIM_EXP` goes from +11.5 cents to +36. The tracking error over C2–C6 is −4.5 to +4.1 cents, within the 5-cent test.
- **What stays.** REV2 does not change: a test pins its output hash from before this change. EXP's stage bend, drive, POST stage and RES law are untouched. The filter picture draws EXP's make-up (`Response::K2040Exp`). LADDER ignores VOICE.
- **Bills.** EXP's steady bill stays at 136: the probe counts the same 81 instructions a sample. `K2040_MOVE_COST` goes from 60 to 65, because EXP's moving loop has gone from 95 to 100 instructions (200 − 135). FB's loop (ADR 0190) carries the make-up too, into FB's signal and its headroom guard. On the ORBIT build, EXP's moving loop goes from 105 to 111 instructions and its FB loop from 166 to 172. The difference is still 61, so `FB_COST` stays 100. STABILITY's spread (ADR 0168) scales each slot's `k` before the make-up is taken, so each chip's make-up follows its own RES.

## Alternatives considered
- **A bass shelf after the stages** (`o + m·lp(o)`, with a one-pole at FREQ/8). This was built first. It meets the bass curve and moves the peak only +0.26 dB. But it flattens the bump over the bass from the Kobol's +5.2 dB to about +1, and the Kobol's loss is flat up to 1.7 kHz, so the shelf is the wrong shape.
- **Feeding a share of the input into the loop sum**, `u = x·(1 + c·k) − k·y₄` (the comparison's suggestion). It is linearly the same as the make-up. But it drives stage 1 up to 2.2× harder, so the ring squelched under a full-scale input even with a linear loop (−3 dB against the make-up's +5.5).
- **A high-pass in the feedback path**, as compensated ladders do. It is in the loop, so it moves the self-oscillation pitch by tens of cents, and it is not flat in frequency.
- **`L = ∞`** (REV2's linear law). It measures within 1 dB of ½. ½ keeps EXP's 3080 element as the owner ruled it (ADR 0096), with a softer squelch.
- **Cell offsets for the ring's H2** (the #711 lab model gives −46 dBc). Not done: this is a sound-character change of its own, so it is left for a follow-up. EXP's ring stays odd-symmetric.

## Consequences
- EXP is louder at high RES, by up to +6.9 dB. Its peaks, its bass and its self-oscillation all rise. The worst output bound goes from 1.0 to 2.19 (the grid reads 2.07). A Sound that used EXP at high RES is louder. No legacy-compatibility promise applies here (no users yet).
- The make-up is fitted to one note, a held A1 with the cutoff open. The fit assumes the Kobol's output level did not change between the takes; the Kobol is packed away, so this cannot be re-checked.
- EXP's RES 0 corner sits 36 cents sharp of FREQ (it was 11.5).
- ADR 0096's "Compensating the 2040's bass loss: the owner chose the authentic loss" is overturned for EXP. ADR 0096's EXP self-oscillation band (about −17 dBFS) and ADR 0105's `TRIM_EXP` are superseded.

## Sources
- (private notes) (sweep1, sweep_res5, sweep_res10, MOTU input 5) and `compare/notes.md`, `sweeps.md`, `selfosc.md`.
- (private notes) (A/B renders, today against compensated, beside the Kobol's takes).
- ADR 0096, ADR 0105, ADR 0037.
