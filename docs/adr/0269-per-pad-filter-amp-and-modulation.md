# 0269. Give each DRUM pad its own FILTER, AMP, mod envelope, LFO and four routes

- **Status:** Proposed
- **Deciders:** owner (2026-11-10 ruling 4, compact pads; 2026-11-13 slice order and AMP MODE), firmware

## Context
Slice 1 (ADR 0265) gave a DRUM Part a kit of 12 pads, each with a machine, eight knobs, level, pan, choke and RETRIG, but one FILTER and one AMP for the whole Part: every pad of a kit went through the same cutoff and the same VCA. A drum machine's pads each shape their own sound (the Rytm's FILTER and AMP pages are per pad), and the drum spec's "The voice" and owner ruling 4 ask for a compact Rytm-style pad: its own filter and amp, one mod envelope, one LFO, a few routes. The pad must stay small, as a kit lives in ITCM (`Kits`, `KIT_VIEW`; ADR 0266) where ~51 KB is free, while D2 is full and AXI sits at its margin; and a slice-1 kit must keep sounding bit for bit as it does.

## Decision
- **A pad carries a `PadSound`** (`dsp::drum::sound`, every field private and typed): `PadFilter` (KIND, MODE, VOICE, CUTOFF `Hz`, RESONANCE and MORPH `Unit`: the FLT front's cells, `PadFilter::IDS`; MODE always one of KIND's, by `FilterParams::set_kind`'s rule), `PadAmp` (an AHD `PadEnv`, VEL, the gate's LPG, DECAY and STRIKE as `lpg::Knobs`, and MODE), a mod `PadEnv`, a `PadLfo` (WAVE `LfoShape`, SPEED, DEPTH, PHASE, MODE `LfoMode { Free, Trig }`) and `PadRoutes`, four `PadRoute`s. `Pad::new` gives `PadSound::DEFAULT`; `Pad::with_sound`, `sound`, `sound_mut`.
- **Overlaid on the voice's `played` snapshot.** On a DRUM voice the pad's FILTER and AMP cells are laid over `played` right after the Part's knobs (`PadSound::overlay`: `PadFilter::apply`, `out.vca_vel`, `out.gate`). From there the Sound chain runs unchanged. `PadSound::from_sound` takes a Sound's FILTER and AMP cells (the rest `DEFAULT`'s), and its overlay is the identity bit for bit, so a pad copied from its Part's Sound nulls against slice 1.
- **Shared by the Part, not the pad:** the MTX, E1–E3 and L1–L3, the FX slot, SENDS, and the filter's INPUT, FB, STABILITY and jacks.
- **The AMP envelope is AHD:** attack, hold, decay (127 is `EnvDecay::Infinite`: the level holds), SPEED FAST/MED/SLOW as Envelope A's; it runs as Envelope A with sustain 0 and release the decay (`PadEnv::env_params`). `PadEnv::OPEN` (A 0, H 0, D ∞) is slice 1's AMP: a constant 1.
- **AMP MODE** (`AmpMode`, codes 1 ONE-SHOT, 2 GATED, fixed by hand; an unknown code loads the default's): ONE-SHOT, the default, ignores note-off, so a pad's AMP plays out as in slice 1 and note-off is heard only by the Part's VCA routes; GATED releases the AMP's decay stage at note-off. The precedent is the Elektron AMP envelope's mode (the Rytm's and the Digitakt's AMP page). This ADR fixes the type and its code; the release is a later task's.
- **Four routes**, `RouteId` 0..=3, each `source → dest` by `Amount` −127..=127. Sources: MOD ENV, LFO, VEL (the pad's) and LFO 1–3 (the Part's shared LFOs). Destinations: CUTOFF, RESO, VOLUME, LFO SPEED, LFO DEPTH. Codes fixed by hand, 1-based, 0 never used. `PadRoute::OFF` is MOD ENV → CUTOFF at 0. A route moves the filter at audio rate, as a Part route does.
- **VOLUME** is a gain on the hit after its AMP envelope, never over unity. Each VOLUME route sets its own depth from unity: with a = amount / 127 and its source's value v read as 0..1 (a bipolar LFO as (v + 1) / 2), a > 0 opens with the source, g = 1 − a·(1 − v) (the Part VCA's polarity); a < 0 ducks, g = 1 − |a|·v; a = 0 is 1. The pad's VOLUME gain is the product of its routes' gains, held to 0..1, and ramps per sample from the last block's. Every amount is heard; nothing boosts.
- **Latched or live, the Rytm's hybrid.** Choices latch at the hit: filter TYPE and MODE, LFO WAVE and MODE, a route's SOURCE and DEST, env SPEED, AMP MODE. Continuous values follow live edits of the pad on its sounding hits, glided (never snapped): CUTOFF, RES, env times, VEL, LPG, LFO SPEED and DEPTH, route AMOUNT. The Rytm's FILTER and AMP knobs move a hit that is still ringing. Without the glide task, everything latches at the hit.
- **The pad MOD page** is ordered PENV, PLFO, ROUTE. The FX slot stays the Part's: there is no per-pad FX.
- **Bytes.** `PadSound::to_bytes` is 57 B (`PAD_SOUND_BYTES`): filter 15 (KIND, MODE, VOICE by their disk codes, then CUTOFF, RES, MORPH as f32 LE), AMP 21 (A, H, D, SPEED, MODE, then VEL, LPG, DECAY, STRIKE as f32 LE), mod env 4, LFO 5 (WAVE, SPEED, DEPTH, PHASE, MODE), routes 12 (SOURCE, DEST, AMOUNT ×4). `from_bytes` is total: an unknown code or a non-finite float takes the default's field, a value past its range is held to it, a MODE the KIND lacks is the default's if the KIND has it, else the KIND's default. `Pad::to_bytes`/`from_bytes` stay 13 B (the `Kit` record's pad is frozen) and keep the default's sound; how a pad's sound is saved is the saving task's rule.
- **Saved as a `KitSound` record a pad** (0x0025, not critical), after the Part's `Kit`: the pad's index 0–11, then its 57 B, 58 B a record, all twelve always written. A wrong length or a pad past 12 is `Bounds`, a pad's second record `Corrupt`; an unknown code or a non-finite float takes `DEFAULT`'s field and makes the load lossy. A pad with no record keeps `DEFAULT`, except on a Part with a `Kit` whose Sound is DRUM (a slice-1 file): there it takes `PadSound::from_sound` of the Part's Sound, settled at the Part's end once that Sound is finished (its refused codes retried), so the kit sounds as it did. Neither absent case makes the load lossy. The new codes are frozen in `disk_codes_v1.txt` (`C DRUM_ROUTE_SOURCE`, `DRUM_ROUTE_DEST`, `DRUM_LFO_MODE`, `DRUM_AMP_MODE`).
- **`PadSound::DEFAULT`** is DRUM INIT's Sound (CLEAN LP4 at 20 kHz, RES 0, VEL 1, no gate) with an open AMP on ONE-SHOT, a mod envelope A 0 H 0 D 64 MED, the default LFO (SINE, SPEED 6, DEPTH 127, PHASE 0, FREE) and no routes.
- **Memory, in ITCM only.** `PadSound` is 64 B (57 of values, padded to its f32s' alignment), a `Pad` 80 B (13 before), a `Kit` 964 B. `Kits`: 990 B → 5,828 B (~1.0 → ~5.7 KB). `KIT_VIEW`, once it publishes the 57 B sound beside each pad's 13 in both seqlock slots: 1,968 B → 10,176 B (1.9 → ~10.0 KB). ITCM grows ~13.0 KB of its ~51 KB free; D2 and AXI do not grow.
- **Flash.** DRUM is bank 1's (ADR 0251, 0265); slice 2 is estimated at +9–14 KB of bank 1, each task's delta measured and its line raised under ADR 0251. The types alone (this ADR's first task) add +1,056 B to ORBIT's bank 1 (+1,048 to +1,168 across the sets), mostly the 80 B pads of `Kit::DEFAULT`, the decode's fallback.

## Alternatives considered
- **A full per-pad Sound (the Part's whole chain a pad):** ~1 KB a pad, ~72 KB the six kits; no room in ITCM, and the compact pad is the owner's ruling 4.
- **A per-pad FX slot:** the FX slot is a voice's costliest block and the Part's character; a pad's FX would multiply the bill by 12. It stays Part-wide.
- **Everything latched at the hit:** a ringing hit would not answer a filter sweep, unlike the Rytm; kept only as the fallback if the glide task is cut.
- **Everything live:** a TYPE, MODE or route SOURCE change on a ringing hit would be a click or a jump; choices latch.
- **AMP always one-shot (no MODE):** a held pad (a pad played as a drone, a gated hat) could not be shortened by its key; the Elektron AMP mode is the known answer.
- **One `KitSound` record a Part (684 B):** past `MAX_RECORD_LEN` (640 B), which sizes every record buffer on the stack; raising it grows each of them.
- **Pad sound bytes in the `Kit` record's pad (13 B → 70 B):** the record's pad is frozen (ADR 0045); the sound is saved by its own rule.

## Consequences
- Each pad shapes its own sound; a slice-1 kit and every non-DRUM Sound sound exactly as before (`drum_slice1_kit` golden, drum nulls).
- `Pad`, `Kit` and `DrumHit` grow: a hit copies ~80 B, so `DrumHit` is passed by reference on the stack-checked path.
- ITCM's free room falls to ~38 KB.
- The pad's routes and LFO are a second, smaller modulation layer beside the Part's: the Part's MTX still reaches the overlaid filter and VCA.

## Sources
- `docs/superpowers/specs/2026-11-10-drum-engine-design.md` ("The voice", "Chain and pages", owner rulings 2026-11-10 item 4, owner 2026-11-13).
- `docs/superpowers/plans/2026-11-13-drum-slice-2-pad-filter-amp.md` (Architecture).
- ADR 0045 (disk codes), 0251 (bank 1), 0265 (kit on the Part, `KIT_VIEW`), 0266 (ITCM), 0267 (size-opt), 0270 (TONAL).
- Elektron Analog Rytm MKII and Digitakt manuals: the AMP page's envelope mode, per-pad FILTER and AMP.
