# 0327. CLEAN LP4 fully open is its input: the top third of an octave opens it

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner (2026-11-22, #695: bypass when CUTOFF is fully open, and never steppy); firmware (bug batch D3)

## Context
CLEAN LP4 at CUTOFF 20 kHz was 4 dB down at 15 kHz, with a spectral centroid 31 % lower (#695), and every INIT Sound sits there. A dry/wet crossfade alone notches: LP4 lags ~180° at its cutoff, so mixing the two cut 13 dB mid-band. A hard bypass at the top steps by 4 dB.

## Decision
- **`clean::OpenTop`.** Over CUTOFF's top third of an octave, 15.87 to 20 kHz:
  - LP4's `g` lifts to `2^(4u)` times itself, where `u` is the place in the band, linear in `g`. The cascade opens toward Nyquist, where it passes everything.
  - Over the band's last quarter, where the lifted cascade lags under 20° at 15 kHz, the dry (after LEVEL) comes in linearly in the lifted `g`.
  - At the top the output is the LEVEL'd input, bit for bit.
  - Both follow `g`'s ramp across the block, so a sweep onto the top or off it never steps.
  - A moving block that reaches the band lifts each sample's `g` on CUTOFF's own ramp and weighs that sample's dry by it (`Clean::sweep`). Ramping the two lifted ends straight opened the cascade in a step: a 1 ms sweep from 1 kHz to the top kinked 4× harder than 0.4's (`filter_test::a_cutoff_sweep_does_not_click`); per sample it kinks exactly as 0.4's did. Under taps or FB the ends' ramp stands.
  - The filter keeps running, so leaving the top starts from live states.
- **ALGO's floor fades.** An operator envelope's release or second decay that reaches its −80 dB floor now falls to silence over a block (`Stage::Fade`, 64 samples, straight), where it used to snap to 0. LP4 at 20 kHz hid that step under its own ring; fully open, the voice's end showed it (`param_sweep_test::a_self_oscillating_filter_dies_with_its_note`, ALGO INIT). `algo_env_test::the_floor_fades_to_silence_never_snaps` pins it.
- **The band per rate.** The band's two `tan`s and two lifts are taken once per rate: at 48 kHz they are a constant (`OpenTop::AT_48K`; `tan_accurate`, `g_of` and the lift are `const fn`). Another rate takes them per call.
- **Bank 2.** The code is bank 2's, through one `FedDoor` entry (`open`, `OpenTop::call`) that takes LP4's whole block on its raw ramps (`OpenCall`): the lifts, the sweep, the cascade and the mix, in the door table ADR 0320 grew. Bank 1 keeps the stub `Clean::open_top`; with the door closed, LP4 plays as it always has.
- **Cost.** The mix (~3 cycles a sample) sits inside CLEAN's steady bill, which carries LP4's 80 under `OLD_LP24`'s 95.

## Alternatives considered
- A dry/wet crossfade over the band alone: a 13 dB notch mid-band.
- A hard bypass at the top: a 4 dB step.
- A time crossfade at the top only: a step in CUTOFF's law.
- Skipping the filter at the top: stale states when leaving.
- Three door entries: the table's room (the review asked for one).

## Consequences
Sound changes for CLEAN LP4 above 15.87 kHz, INIT Sounds' default filter among them, and at the end of every ALGO release (−80 dB down, one block). DIRTY, LADDER, the 2040 and CLEAN's other modes are unchanged.

- **Factory TAPE sources.** 18 rendered sources move and are re-pinned in `manifest-v1.json` (owner, 2026-11-23: re-render, don't freeze): SINE C4, EPIANO C4, EPIANO STAB, TX BASS C2 (two frames longer: the floor's fade), BRASS C4, BELL C5, KEYS C4, PLUCK C3, PLUCK E2, HARP C5, BOWED C3, BOWED CHORD, METAL C4, GLASS C6, SYMP C3, TRON BOWED, WAVE BOWED, WAVE BRASS. WAVE MORPH stays frozen (ADR 0325), so TINZ is unchanged. Each source peaks at −0.5 dBFS, so a brighter attack lowers its loudness: PLUCK HIT, PLUCK LOW and HARP HIT are re-trimmed +0.5, +0.5 and +1.0 dB, back within 0.4 dB of their 0.4 loudness (`factory_sounds_sit_at_the_median`).
- **ALGO's fast release.** The open top passes the corner where an operator's release starts, which LP4 at 20 kHz rounded off. INIT ALGO's bass in `engine_clicks_test`, click ratio on 0.4 → now: RR 8–10 1.06 → 1.01, RR 11 1.18 → 1.81, RR 12 2.69 → 3.49, RR 13 3.50 → 6.50, RR 14 5.02 → 6.94, RR 15 4.00 → 10.75; RUBBER BASS 1.91 both. At CUTOFF 15 kHz, under the band, both read alike at every RR (1.14–2.97): the floor's fade adds nothing. RR 12 joins 13–15 as keeping its snap; `algo_short_release_does_not_click` checks RR 8, 10 and 11 (owner, 2026-11-23: inaudible on before/after clips, accepted).
- **STRUCTURE's kink bound.** `modal_resonator_test::a_structure_step_at_g1_does_not_click` bounds the glided kink at 8× a held STRUCTURE's, was 3×. The resonator is unchanged; its top octave now passes, where LP4 at 20 kHz took it off, and the test's second difference weighs that octave most. Through the open filter the glided step is 5.5× and the unglided 11.1× (2.6× and 4.2× before), so 8× still tells the glide from none.
- **References.** `filter_types_test`'s exact reference lifts `g` and mixes the dry as `OpenTop` does (`open_top_ref`). The cost probe's LP4 jump stays under the band: the open top is bank 2's path, not a probed loop.
- **Flash.** Bank 1 +312 B on default and ORBIT (+232 to +368 on the other sets, with 0.4's drift since their lines): `Clean::open_top`, the door's stub and its call, and ALGO's floor fade, with `OpEnv::enter` out of line so the kernel calls bank 1's one copy. Bank 2 +5,792 B on default, +5,824 on ORBIT, the open top in its own S4 group (`clean::open`); S0-S3 takes the fade in ALGO's kernel and the door's entry, leaving ORBIT 64 B there (0.4: 160). Lines in `tools/bank1-budget.txt` and `tools/bank2-budget.txt`.
- **Goldens.** Every render through an INIT-based filter at CUTOFF 20 kHz moves, and every ALGO render that reaches its floor: `golden_test`'s ALGO, MODAL, factory and TAPE rows (TAPE's INIT filter is CLEAN LP4 too), `voice_fx_golden_test`'s ALGO rows, `instrument_test`'s four mixes and their pre-limiter taps, `codec_compat_test`'s fixture renders (INIT VA's saw is unchanged), `fixed_columns_test`, ORBIT's four songs' audio (events and view unchanged) and the ALGO and MODAL door tables. `tape_legacy_test`'s render hashes move too; the three re-trimmed Sounds no longer render as their old files. `factory_level_test` allows factories 0-3 and 7 0.15-0.46 dB per block, the top they now keep. DRUM, VA and the screens are unchanged.

## Sources
private issue #695; ADR 0190 (FB's door), 0320 (the door table), 0316 (batch D3).
