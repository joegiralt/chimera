# 0338. CLEAN LP4's open top under taps, FB and resonance

- **Status:** Accepted (2026-11-24)
- **Deciders:** firmware (the review of #1135, #695); amends ADR 0327

## Context
The review of #1135 (ADR 0327's open top) found four problems.

- **Filter FM was lost near the top (I1).** Under a tap or FB, the open top lifted and weighed the ramp's two ends and only then ran the taps and FB on the lifted `g`. At CUTOFF 20 kHz the weight is 1, so the output was the dry exactly. FB and the OP5/OP6 FREQ taps did nothing on any INIT Sound. A saw at RES ½ shows it: RMS of modulated minus unmodulated was 0 for FB 0.2, FB 0.5 and an OP6 tap at ½. In 0.4 (7b94ccd17), a plain LP4 at 20 kHz, it was 0.0050, 0.0077 and 0.0152.
- **Bank 1 grew.** ORBIT's bank 1 was +304 B over 0.4 (913216 against 912912).
  - The ALGO floor fade was not the cause: `OpEnv::enter` +44 and `AlgoEngine::note_on` −46.
  - The door's stub was: `Clean::open_top` +152 and `Clean::process` +160, with the taps' shape and the mods' fields copied into `OpenCall`.
- **A resonant sweep through the top rang (minor 4).** RES 0.9 on a 1 kHz sine, 8 kHz to the top and back, gave a worst second difference of 0.0196 on a one-block jump and 0.0082 on a 64-block sweep (0.4: 0.003 and 0.0024). The lift carries the resonant peak toward Nyquist, where a fold or fuzz after the filter aliases it down.
- **STRUCTURE's kink bound.** ADR 0327 loosened `a_structure_step_at_g1_does_not_click` to 8×. That bound measured the open filter's top octave, not the resonator.

## Decision
1. **Taps and FB move each sample's `g` before the band reads it** (`clean::open`, `Clean::moved`). Any LP4 block whose `g` moves per sample runs one loop: a tap, FB, or a sweep that reaches the band.
   - Each sample's `g` is the ramp's, then through the taps, then FB's.
   - The band's place `u` is taken from the lower of the ramp's `g` and the moved one.
   - A tap or FB moving `g` up keeps the ramp's lift, toward the dry. Moving it down takes the band back as CUTOFF would, so it reaches as deep as under the band.
   - That sample's `g` is lifted at that `u`, and its dry is weighed by the lifted `g`.
   - Under the band (`u = 0`) the loop is `moving::<2>`'s, or FB's `fed`, bit for bit.
2. **The band damps resonance** (`OpenTop::damped`, `K_OPEN` 0.5). A `k` under 0.5 moves toward 0.5 as `u` rises, so the peak the lift carries toward Nyquist fades out instead of ringing there.
   - The trim is linear in `k` up to 0.5. It rises with the damping to 1 at the top, the dry's level, so a resonant LP4 meets its untrimmed input without a level step.
   - A `k` already past 0.5 (RES under 0.71) is kept, and is untrimmed anyway.
   - The held and glided ends are damped the same way.
3. **The door carries the mods, not their shape.** `OpenCall` takes `&FilterMods`, and the taps' `TapShape` is built behind the door.
4. **With the door closed, LP4 plays held blocks on its moving loop** (`Clean::closed_lp4`).
   - That is `steady`'s bits: equal ends give the same coefficients, and `held`'s cascade is `tick`'s arithmetic. `closed_lp4_is_run_to_the_bit` checks it.
   - Bank 1 keeps no held copy of LP4 for a path only the closed door plays. With the door open, bank 2 plays every LP4 block.
5. **STRUCTURE's test runs LP4 at 15 kHz, under the band,** so it measures the resonator again, at its 3× bound. The glided kink is 1.81×.

## Alternatives considered
- **Lifting the tapped `g` as it stands.** The band maps four octaves of lift onto a third of an octave of CUTOFF. A ±4-octave tap at 8 kHz from CUTOFF 10 kHz chopped between filter and dry: −46 dB of products off the tap's sidebands, against −89 dB free and 0.4's −77 dB (`taps_never_blow_up_or_alias_past_the_margin`).
- **Moving the lifted `g` by the taps' ratio.** It doesn't alias, but at the top the lift sits four octaves past 20 kHz. A tap's down-swing then never reached the audible range: 0.37× 0.4's effect at 20 kHz.
- **Damping `k` to 2 (RES 0).**
  - With the trim on the damped `k`, the trim reaches 1 a fifth of the way into the band at RES 1, ahead of the peak it trims: a one-block jump peaked at 0.957, over `clean_res_1_one_block_jumps_stay_bounded`'s 0.92.
  - With the trim on the undamped `k`, the dry's level comes in over the band's last quarter: the jump read 0.0162.
- **Ramping `u` across the block between its ends.** It spreads a jump's opening over 64 samples. It read rougher than per-sample `u` with the damping above (RES 0 jump 1.52× the input's own second difference, against 1.26×), and it moved three goldens.
- **Raising bank 1's line.** Bank 1 is frozen.

## Consequences
- **Modulation reaches the top.** At 20 kHz, RMS of modulated minus unmodulated against 0.4's: OP6 tap 1.26×; FB 0.2 2.3×; FB 0.5 2.6×. At 18 kHz all three read 1.1–1.5×.
  - FB reads higher at the top because the unmodulated output it is measured from is now the full-band input, not a 20 kHz LP4.
  - `clean_open_test::fb_and_taps_still_move_lp4_fully_open` holds each at 0.5–3× 0.4's.
- **Resonant sweeps are smooth.** On the review's 1 kHz sine, every sweep and RES 0.9's jump read 1.0× the input's own second difference (0.0043), which is the floor, since fully open is the input. Jumps at RES 0 and ½ read 1.26× and 1.24× (0.4: 1.26× at RES 0).
  - 0.4 read 0.003 at RES 0.9, under the input's, because it kept RES 0.9's trim (0.52) at the top.
  - `a_resonant_sweep_through_the_top_stays_smooth` pins this.
- **Goldens.** Every golden, door table and factory render is unchanged: none drives LP4 past 15.87 kHz under a tap or FB, or with RES over 0.71 in the band. `filter_types_test`'s exact reference models the damping and the trim (`open_top_ref`, `open_u`).
- **Flash.**
  - Bank 1 is −856 B on every set, under 0.4's lines: ORBIT 912360 (0.4: 912904), default 897336 (0.4: 897880).
  - Bank 2 is +640 B on default and +608 on ORBIT and bank2-gate, all in `clean::open`'s S4 group. S0–S3 keeps its 64 B on ORBIT.
- **CPU.**
  - A tapped or FB'd LP4 at or past 15.87 kHz adds the lift's `exp2`, the band's place and the weight a sample to FB's or the tap's loop.
  - A held LP4 with the door closed divides every sample instead of once a block.

## Sources
private issue #695, #1135 (the review); ADR 0327 (the open top), 0190 (FB's door), 0198 (bank 1's lines), 0272 (bank 2's).
