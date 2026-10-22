# 0205. SCRUB is playable: smoothed jog, musical defaults, keys place the finger

- **Status:** Proposed
- **Deciders:** owner (approved SCRUB improvements 1–3 of the TAPE machines report, 2026-10-21), firmware
- **Amends:** [0200](0200-scrub-s-finger-aims-before-it-drags.md) (the aim is two poles of 20 ms, not one of 40), [0093](0093-tape-plays-12-bit-card-sources-through-six-ram-models.md) § SCRUB (LENGTH's law, the key)

## Context
After #747 (ADR 0200) SCRUB's finger no longer snaps, but three things kept it from playing well:
- **The jog still rippled.** The jog's speed was the count of the detents in the last 125 ms. At 25 detents a second that count is 3 or 4, so the speed stepped by a third as detents entered and left the window. A turn's time is also its UI pass's (33 ms), not the hand's. On the deck's own head probe (a 4 s window, SPEED 0, two-block readings from 1.0 to 1.9 s), the head's speed under that jog swung by 0.77, 0.28 and 0.11 of its mean at DRAG 10, 32 and 120 ms.
- **FINGER did nothing as loaded.** SCRUB's LENGTH defaulted to 1, which is a loop the length of the window, so the finger had no room. A musical scrub took three steps (shorten LENGTH, then move FINGER, then fix XFADE). LENGTH was also a share of the window, so no single default could be a given time: a share that gives 70 ms on a 4 s source gives 37 ms on 1 s and 180 ms on 30 s.
- **A key did nothing but pitch.** SCRUB is vari-speed, so pitch already comes from SPEED and the finger. A key could not choose where in the source to play, as an Octatrack slice or a Digitakt start does.

## Decision
1. **The jog reads the hand's rate, not a count** (`ui::jog`).
   - The rate is the detents after the first, over the time from the first turn to the last, in a 250 ms window. One turn alone reads as one detent a window.
   - The speed eases toward that rate through a 50 ms one-pole (`JOG_EASE_S`).
   - The hand has let go once no turn has come for 1.5 times its time between turns. From then on, turns left in the window never move the reel again, and the speed glides out through DRAG as before.
   - A turn against the last one clears the window, so a reversal never averages with the turns before it.
   - `Jog::pass` is bank 2's, reached through the UI's door (`UiDoor::jog`). With the door closed, the reel stands (TAPE is silent then anyway).
2. **The aim is two poles.** POSITION reaches the finger through two one-poles of `AIM_S` = 20 ms (`Scrub::lead`, then `Scrub::aim`), then DRAG's. Their delay is the 40 ms of 0200's single pole, but a 33 ms UI pass's staircase leaves less than half the swing. This holds for every input path: the jog, a CC, the matrix, a jump, and a legato key.
3. **SCRUB's LENGTH is a time.** It runs `2 ms · 1000^LENGTH`, from 2 ms to `SCRUB_LEN_MAX_S` = 2 s, and never longer than the window. The deck maps it onto the window's law (`TapeDeck::scrub_size`). The other machines' LENGTH is unchanged.
4. **SCRUB's defaults play.** LENGTH defaults to 66/128, a 70 ms loop (70.5 ms). It is written on a MODEL change to SCRUB, and by the factory SCRUB Sounds, which take the machine's defaults. FINGER moves the sound from the first turn. SPEED stays 0.5 (the source's own speed) and XFADE stays 0.1.
   - **Why 70 ms.** Listening and measurement on SAW SCRUB, with the FINGER swept 0 → 1 over 3 s and LENGTH from 20 to 160 ms:
     - Under about 50 ms the loop repeats at 20 Hz or more, where it fuses into a buzz with its own pitch.
     - Over about 100 ms a sweep sounds like stuttered echoes, and the finger answers late.
     - Between 60 and 80 ms the swept take departs most from the parked one (a null of 1.5–1.8 dB), and the seam flutter stays at the range's floor (0.56–0.82 dB, against 0.58–0.83 across 20–160 ms).
     - XFADE 0.5 flutters more than 0.1 at most lengths (1.1–2.5 dB against 0.6–0.8), so SCRUB keeps 0.1.
     - 70 ms is the middle of the band: the loop repeats at 14 Hz, well under fusion, and holds several periods of anything pitched.
5. **KEY: POSITION or PITCH** (param 21, `KeyMode`, on SCRUB's page). The default is POSITION.
   - **POSITION:** a key places the finger and plays at ROOT.
     - **The places.** There are 12 places an octave (`KEY_PLACES`), every octave alike. C places the finger at the window's start, C♯ 1/12 of the way along, and up to B at 11/12. This is SLICE's octave grid (#748, ADR 0199 on the `tape-slice` branch), so any octave and either hand play the whole source.
     - **The range.** Places are relative to START and END: they are shares of the finger's room, the window less the loop.
     - **Quantised, not continuous.** Each key is one of 12 steps. FINGER gives the fine placement.
     - **FINGER adds to the key's place.** The finger sits at `place + FINGER`, at most the room's end. At FINGER 0 a key plays its own place. Turning FINGER, a CC or a route moves the finger from there. C plays exactly as before.
     - **Legato.** A legato key change (STEAL GLIDE) moves the finger to the new key's place through the aim.
     - **The hint.** On SCRUB, FINGER's focus band reads `KEYS PLACE THE FINGER` at KEY POSITION (`view::KEY_HINT`, the hidden-gesture rule).
   - **PITCH:** the key transposes the vari-speed scrub, as before this ADR.
   - **Why POSITION is the default.** SCRUB's pitch already belongs to SPEED and the finger, so a key as pitch on top of it makes a chord a smear of the same spot. A key as place makes the keyboard a map of the source, which is the slice and start precedent (Octatrack slices, Digitakt's start, an MPC chop). A loaded SCRUB Sound then answers on every key straight away. PITCH remains for melodic scrubbing.
6. **The units are types** (the owner's type-driven rule):
   - `Place` is a place in the finger's room. It is held to 0..1, and NaN becomes the start. `key_place` returns one, and `Place::plus` adds FINGER without leaving the room.
   - `LoopTime` is SCRUB's loop in seconds, made only from LENGTH's knob (`of_length`). `share_of` turns it into the window's law.
   - The jog's turns are timed in `Ms`, the hand's rate is a `DetentRate`, and the reel's speed is a `ReelSpeed`.
   - `KeyMode` is the KEY enum. The deck holds the key as `Option<Place>`, where `None` is PITCH, so no PITCH note can carry a place.

   The tests cover only what the types can't: the timings, the ripple bounds, the codec and the audio.
7. **Codes are append-only.** KEY is param 21 (17 and 18 are SLICE's on #748, 19 and 20 PLAY's on tape-play), and its disk codes are 0 POSITION and 1 PITCH. A Sound saved before this ADR has no KEY and loads at POSITION, and its LENGTH reads by the new law. Under the owner's no-legacy-compat rule it loads safely but may sound different. Every other machine is bit for bit: `golden_test`'s ONE, LOOP, PING, TRON, STRETCH and WAVE renders are unchanged.

## Measured
The deck's head probe as described in Context. The swing is (highest − lowest) over the mean.

| Input | DRAG | #747 | This ADR |
|---|---|---|---|
| jog, 25 detents/s, 33 ms pass | 10 ms | 0.772 (×2.23) | 0.215 (×1.25) |
| | 32 ms | 0.280 (×1.32) | 0.098 (×1.10) |
| | 120 ms | 0.111 (×1.12) | 0.059 (×1.06) |
| CC, 1/128 every 23 ms | 10 ms | 0.167 | 0.075 |
| | 32 ms | 0.063 | 0.035 |
| | 120 ms | 0.035 | 0.029 |

At the default DRAG the jog's swing is 0.35 of #747's, more than halved. `scrub_finger_speed_never_steps` (0200's test, at DRAG 0, 0.3, 0.6 and 1) now measures:
- a jump at 0.15 of its peak at most, for its first block and its largest change;
- a 33 ms staircase at ×1.16 at most.

Its bounds are tightened from 0.4 and ×1.4 to 0.2 and ×1.2.

## Alternatives considered
- **A longer count window or a heavier one-pole on the old count.** A block-rate model of the jog, the aim and DRAG put the count's ripple at ×1.31, and longer windows made it worse: ×1.6–3.0 at 250 ms. The interval rate alone reached ×1.16, and the rest was the UI pass's staircase.
- **A longer single aim pole (60 ms).** It cuts the staircase by a third but slows every jump. Two 20 ms poles keep 0200's delay and halve the swing.
- **A continuous key place (each key 1/128 of the window).** On a 4 s source adjacent keys would be 30 ms apart, closer than the loop, so neighbouring keys would sound the same. Twelve places an octave, plus FINGER for the fine placement, gives one musical position per key.
- **The key's place relative to FINGER at note-on** (the finger at the key's place, then moved by FINGER's change since). It loses parking: FINGER would no longer say where a C plays. The sum keeps C's behaviour exactly.
- **A window-relative default for LENGTH.** No share is 60–80 ms on every source (see Context).

## Consequences
- SCRUB loads playable: keys place the finger and FINGER sweeps are heard. The factory SAW SCRUB and RAIN SCRUB take the new defaults and are re-trimmed to the set's median (−3.0 and +5.5 dB, from −1.0 and +7.0). Their Sound CRC is re-pinned.
- **Cost:** two multiply-adds a block per SCRUB voice (the second aim pole), and 12 B more `Scrub` state (`lead` and `key`). `COST_SCRUB` is unchanged. The jog runs once a UI pass.
- **Bank 1 (frozen, ADR 0198):** the only new code is the door's wrapper. `Jog::pass` moves out of bank 1 into bank 2. Bank 1 gains only closed-door data:
  - KEY's param spec, its names and idents, its codec arms (`get`, `write`, `enum_code`, `enum_ident`, `set_enum_code`), `KeyMode`'s disk-code table;
  - SCRUB's cell on its page, and LENGTH's new default;
  - FINGER's hint (one string and its arm), `SlotCtx::keys_place`;
  - `UiDoor::jog`, and `MAX_SOUND_COST` 2 656 → 2 672 (one more param).

  Measured on the ORBIT build (`just firmware-orbit`) against tape-streaming `8d0faa8d` (#747 merged):
  - **bank 1: +396 B** (904,952 → 905,348 B). By symbol: KEY's codec arms and spec about +260 B (`write`, `set_enum_code`, `enum_ident`, `enum_code`, `get` and `TAPE_ENGINE_SPECS` +226 B, their switch tables the rest), FINGER's hint and `SlotCtx::keys_place` +56 B, the door call in the input path +48 B, and the rest outlining and alignment;
  - **bank 2: +2,208 B** (258,144 → 260,352 B): the jog's estimator, `scrub`'s units, the key's place and the second aim pole.

  tape-streaming has no `tools/bank1-budget.txt` yet, so there is no line to raise. When it takes main's guard, the ORBIT line rises by these 396 B under this ADR.
- `MAX_SOUND_COST` rises to 2 672 B (`sparse_bound`: 2 658 B worst).
- Follow-ups: the SRC strip draws the finger at FINGER's value, not at the key's place. DRAG is still latched at note-on (0200).

## Sources
- `chimera-core/src/ui/jog.rs`, `chimera-engine/src/dsp/tape_engine/mod.rs` (`scrub_block`, `scrub_size`, `key_place`, `AIM_S`), `machine.rs` (`SCRUB_LENGTH_DEFAULT`), `params.rs` (`KeyMode`), `chimera-core/src/ui/view.rs` (`KEY_HINT`).
- Tests: `tape_scrub_play_test`, `tape_models_test::scrub_finger_speed_never_steps` and `scrub_drag_slews`, `tape_machines_test::jog_*`.
- (private notes) (the probe, the block-rate model, the renders and the listening page).
- Precedent: Elektron Octatrack slice mode and Digitakt sample start (a key chooses where); Morphagene's Slide (a slewed position); SLICE's octave grid (ADR 0199 on the `tape-slice` branch, #748).
