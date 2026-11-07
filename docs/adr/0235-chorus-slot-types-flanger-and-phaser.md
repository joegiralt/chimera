# 0235. The chorus slot's TYPEs: FLANGER and PHASER beside the Juno modes

- **Status:** Proposed
- **Deciders:** owner (#794: CHORUS I, II, I+II, FLANGER, PHASER in the chorus bus slot, cheap, #793), firmware (the DSP, the controls)

## Context
The chorus slot had MODE OFF, I, II, I+II (Juno-60 buttons). #794 adds FLANGER and PHASER to the same slot. Constraints: the existing modes stay bit for bit (`fx_golden_test`); no new RAM for audio; each new TYPE no dearer than I+II on the host; the loops are bank 2's (ADR 0213); sweeps never zipper; clean provenance.

## Decision
- **One param.** The slot's choice (ident `MODE`, kept for disk and NRPN 20/0, ADR 0216) gains codes 4 FLANGER and 5 PHASER, and its cell reads TYPE, as DELAY's and REVERB's do. RATE, DEPTH and MIX are every TYPE's; no new cell, no dims. The feedback is fixed per TYPE (FLANGER 0.6, PHASER 0.5), as on a one-knob pedal: a FDBK cell would need a new param and dims through the UI door, left for a later call.
- **Wet only, as the chorus.** Each TYPE returns the effect alone; the Part's dry beside the return makes the notches, so the CHORUS send sets their depth (full at 1).
- **FLANGER** shares the chorus's two lines (no RAM): line I is the left, line II the right, each read 0.25 ms + DEPTH × 1.25–5 ms × (1 ± tri) before it is written, its tap fed back into its own line. The LFO is the slot's own (1/16–2 Hz over RATE).
- **PHASER**: six first-order all-passes a side, `a = (t − 1)/(t + 1)`, `t = tan(π fc/fs)` (`tan_accurate`), fc swept exponentially round 600 Hz, ±(DEPTH × 1.25–5)/2 octaves, the right side on the inverted triangle, fed back from the chain's end (1/8–4 Hz). Coefficients are set at block ends and ramped linearly across the block: smooth, at block-rate cost. The lines are still written, so a crossfade to a chorus mode reads current audio.
- **Crossfades.** Each TYPE has an eased weight (four `Ease`s); a change runs every TYPE weighted. FLANGER's feedback into the shared lines is scaled by its share of the return, so a line still heard as I or II never steps.
- **Idle.** f32 feedback never decays to +0 alone, so after a FLANGER or PHASER sound, `TAIL` (24,000 samples of silent send, the tail under −400 dB) clears the lines and all-passes; the idle skip (ADR 0214) waits for that clear.
- **The braid** gives FLANGER and PHASER two strands; a TYPE word too wide for the large font beside it (FLANGER, PHASER) drops to the readout size and the braid stays, as TAPE's glyph does.

Host cost (`just chorus-listen`, ns a sample in the loop, steady): I 6.9, II 7.0, I+II 13.8, FLANGER 7.7 (0.56×), PHASER 11.4 (0.83×).

## Alternatives considered
- A FDBK cell dimmed under the Juno modes (DELAY's precedent, ADR 0202): more UI in bank 1 and the UI door; fixed feedback first, the owner's ear decides.
- A third delay line for FLANGER: 8 KB of RAM for nothing the shared lines can't do.
- Per-sample phaser coefficients: a tan and exp a sample; the ramp is as smooth.
- Returning dry + wet: shallower notches once the mixer adds the dry again.

## Consequences
New choices only: nothing loads into them. Bank 1 grows 252 B in the default set (+260 to +352 B over the lines across the sets, `tools/bank1-budget.txt`): the TYPEs' names, disk codes and hints (closed-door data), the braid's two TYPEs and its readout-size fallback; the loops and PHASER's coefficients are bank 2's. Old modes are bit-identical. A FDBK knob, more stages or through-zero flanging would be new decisions.

## Sources
- J. Dattorro, "Effect Design Part 2: Delay-Line Modulation and Chorus", JAES 45(10), 1997, § 3 (flanging).
- J. O. Smith, "An Allpass Approach to Digital Phasing and Flanging", ICMC 1984.
- U. Zölzer (ed.), DAFX, 2nd ed., Wiley 2011, § 2.2.3 (phaser), § 3.3.1 (flanger).
- Written from these; no GPL code read.
