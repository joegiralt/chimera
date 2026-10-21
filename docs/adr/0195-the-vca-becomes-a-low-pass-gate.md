# 0195. The VCA becomes a low-pass gate (LPG, DECAY, STRIKE)

- **Status:** Proposed
- **Deciders:** owner (chose Plaits' LPG by ear over our Buchla 292 circuit model and the SuperCollider LPG, 2026-10-21, epic #720; then the VCA design over a filter TYPE, 2026-10-21), firmware (the port, the knobs, bank 2)

## Context
The LPG labs built three candidates and the owner listened to all three (private notes, `notes.md` and `gate/gate.md`): our circuit model of the Buchla 292B, the SuperCollider LPG, and a port of Plaits' LPG (MIT). Plaits "sounds much better"; ours and SuperCollider's "sound very harsh". The circuit goes dark within about 20 ms and shuts in under 0.4 s. Plaits holds its highs for tens of milliseconds and closes over about a second, so a strike rings bright and dies away.

PR #739 built the port as filter TYPE 7. The owner paused it: a low-pass gate is a VCA that darkens as it closes, so it belongs to the VCA, and any filter TYPE should be able to sit in front of it. That PR's port was verified line by line against Plaits' `low_pass_gate.h`, `envelope.h` and `voice.cc` and stmlib's `Svf`. This ADR keeps that port and moves it.

Plaits' gate is behavioural, not a circuit. One state (Plaits calls it the vactrol) does three jobs every 12 samples:
- it is the gain, so the gate shuts;
- it sets the cutoff, `(0.003 + 0.3·v⁴ + 0.04·COLOR)·fs`, so the highs fall steeply as it shuts;
- its fall slows as it falls.

## Decision
- **The VCA stays a VCA.** AMP gains three knobs: OUT params 7 LPG, 8 DECAY and 9 STRIKE, idents `LPG`, `DECAY`, `STRIKE`, `Uni` 0..1, default 0. They are smoothed (`Smoothing::KNOB`), modulatable and MIDI-mappable. With all three at 0 the voice runs today's VCA code, untouched. Every existing Sound renders bit for bit as before (`the_gate_at_zero_is_todays_vca_bit_for_bit`, FNV-1a `0xf54a71e5c050585d`, pinned on origin/main f745c0a1 before the change). It covers ALGO, MODAL, TAPE and VA INIT; no route, ENV 1 into VCA, and ENV 1 with LFO 1; CLEAN, 2040, DIRTY, LADDER and SEM; VEL 1 and ½; a re-strike at another velocity; key up and tail; uneased and glided; and every VA MODE. Old Sounds have no record for 7–9, so they load at 0.
- **LPG** is Plaits' COLOR inverted, `COLOR = 1 − LPG`, so 0 means off. At 0 the bleed is 1, so the gated signal passes the filter untouched: a VCA. At 1 the gate is all filter, the darkest. LPG also weighs in voice.cc's compressed LEVEL, `l + LPG·(1.3·l/(0.3 + l) − l)`, so the gain law moves to Plaits' only as LPG reaches 1. Plaits' default COLOR ½ is LPG ½.
- **DECAY** is Plaits' decay (voice.cc: the short decay `200·12/fs·2^(−8·d)` a step, the tail `20·12/fs·2^(−6·d + COLOR)`). Its first 1/64, two knob ticks, fades the vactrol in: the step's coefficient is `1 − w·(1 − c)`, `w = min(1, 64·DECAY)`. At 0 the state is the level each step, and the routes' detail between the steps rides on the step's ramp, weighted `(1 − w)(1 − STRIKE)`. So at DECAY 0 the gain is the routes' own, sample for sample: the gate at LPG 0 nulls the plain VCA at −100 dB or better. From DECAY 1/64 up it is Plaits' decay as built.
- **STRIKE** pings the gate at each note-on, as Plaits' `ProcessPing` does. The vactrol rises by `2·12·f_note/fs` a step, as Plaits', to the level plus STRIKE × VEL, then falls through DECAY. The level is × (1 − STRIKE), so at 1 the strike alone drives the gate, and in between the strike adds to the routed envelope. It rings out whether or not the key is held. VEL scales the strike, so the AMP's VEL cell is live under a strike with no route into VCA.
- **What drives it.** With routes into VCA: the routes' gain × AMP's VEL, Plaits' LEVEL (`ProcessLP`). With no route on ALGO and MODAL: 1, since their own envelopes shape the sound. With no route on TAPE and VA, where the key ends the note today: the key, and after key up a fall over the voice's 128-sample fade, a step at a time. OUT LEVEL multiplies after the gate, ramped as today.
- **No double gating.** Under the gate, the routes drive it and the VCA multiplies by OUT LEVEL alone. At LPG 1/128 the gate plays VA INIT's level within 0.2 dB, and ENV 1's SUSTAIN ½ is 6 dB under SUSTAIN 1, not 12 (`the_gate_never_gates_twice`).
- **Voices end when the gate shuts.** While the gate runs, it holds the voice until its gain is under −80 dB and no strike is rising. Below STRIKE 1, the voice is also held as the plain VCA would hold it: a route's source still holding it, or, with no route, its engine. At STRIKE 1 the gate alone holds it, so a struck note ends when the gate shuts even with the key held (the key keeps its slot, as any held note's). The key-up fade of a TAPE or VA note with no route gives way to the gate's fall.
- **The port** is `chimera-engine/src/dsp/lpg.rs`: no_std f32, no allocation, no `unsafe`, the MIT notice in the file, and THIRD_PARTY.md credits Emilie Gillet. Its step, gain ramp and SVF are #739's: `LPGEnvelope::ProcessLP` and `ProcessPing`, the cutoff and bleed read off the state before its move; `LowPassGate::Process`, the gain ramped across each 12 samples and stmlib's `Svf` at Q 0.4 with FREQUENCY_DIRTY held for them. The 12-sample step runs across the voice's 64-sample blocks with its phase carried, so a block holds five or six steps. At LPG 1 it nulls the lab's f64 port on #739's fixture at −129.5 dB, and STRIKE 1 nulls an f64 port of `ProcessPing` at −116 to −129 dB.
- **Bank 2.** The gate runs only through `GateDoor { run }` (`Bank2Door::gate`): `voice::gate::kern::run`, which `bank2.x` links into `.bank2` (`*5voice4gate4kern*`), with `Lpg::block` inlined into it. `check-door-placed` checks `voice::gate::kern::`. With the door closed, the voice plays the plain VCA, bit for bit (`gate_proxy_test`). A firmware built without bank 2 (`--no-default-features`, a build check, never shipped) has no room for the kernel in bank 1, so there the gate is always its closed door. Host builds without bank 2 (the tests, the desktop) run the kernel in place.
- **Pages.** AMP is VEL · LPG · DECAY · STRIKE, E and F free. VEL stays first. No convention puts it elsewhere, and A to D are the knobs in the order they shape a note: how hard, how dark, how long, how struck. The sends move to AMP › SENDS, which is the mixer's own SENDS page (id 30: CHORUS · DELAY · REVERB). So one value has one page and one focus, and no page is added. The sends keep their idents (`SEND1`–`SEND3`) and ids (4–6), so saved Sounds and MIDI maps keep working. The words are full: VEL, LPG (the owner's term), DECAY, STRIKE. The matrix header for DECAY reads DCAY, which fits.
- **No filter kind.** #739's TYPE 7, its FLT page cells and its filter param 17 are not carried. Code 7 was never on main, so nothing is retired. It stays unused, and the filter's comment says why.
- **Cost** (`tools/loop_count.py`'s method on the `filter-probes` ELF, `lpg::lpg_probe`): see Consequences. `Voice::cost` bills `LPG_COST` only while LPG, DECAY or STRIKE is stored above 0 or routed. At 0 a voice costs what it did.

## Alternatives considered
- **LPG as filter TYPE 7 (#739):** the filter slot would be spent on the gate, so no SEM or 2040 could sit in front of it, and COLOR and DECAY would have borrowed RES and a new filter param. The owner chose the VCA.
- **A KIND switch on AMP (VCA / LPG), with COLOR and a trigger mode:** a mode switch would jump the sound. Three knobs that are all 0 by default reach the same places continuously, and 0 is today's VCA.
- **DECAY 0 as Plaits' shortest decay:** that is still a 5–50 ms vactrol, so DECAY could never be off. The fade-in over DECAY's first 1/64 keeps Plaits' whole range from there up.
- **DECAY 0 tracking the routes at the step's rate alone:** an 11 ms pluck played 1.1 dB softer and a step late. The routes' per-sample detail fixes that for four operations a sample.
- **Full Plaits compression at any LPG above 0:** a jump of up to +4 dB as LPG leaves 0.
- **The strike × the level, or a strike only with no route (Plaits'):** at STRIKE between 0 and 1, the strike adds to the routed envelope, as the owner's table asks.
- **Our Buchla 292B circuit model, or the SuperCollider LPG:** the owner heard both as harsh (#739's gate checks).
- **A new AMP › SENDS page:** a second copy of the mixer's SENDS, with its own focus and its own flash, for the same three values.
- **The gate in bank 1:** bank 1 is frozen (owner). Its kernel is about 1 KB that only gated Sounds run.

## Consequences
- **Flash** (ORBIT build, against origin/main f745c0a1): BANK1_TBD
- **RAM:** the voice grows 80 B (6,576 → 6,656): the gate's 64 B and padding. `VOICE_CHAIN_BYTES` goes from 2,336 to 2,408. `MAX_SOUND_COST` goes from 2,656 to 2,672 B, since a dense Sound now has 280 params (2,648 B).
- **Cost and voices:** COST_TBD
- **Gate checks** on one 220 Hz saw hit at LPG 1, DECAY ½ (onset centroid, 50 ms centroid, time to −20 and −40 dB, level at 1 s). Each is within 1 % / 2 ms / 0.3 dB of the f64 port:
  - the lab's ping into the routes: 2418 Hz, 1299 Hz, 342 ms, 1022 ms, −38.6 dB;
  - STRIKE 1 alone: 2457 Hz, 1287 Hz, 340 ms, 1017 ms, −38.6 dB.

  #739's row (COLOR ½, 2448 Hz to −35.5 dB) is LPG ½ with Plaits' full compression. This design does not have that, since LPG ½ weighs the compression in by half.
- **Long DECAYs hold voices.** At DECAY 1 the tail falls at about 3 s a time constant, so a gate takes some 30 s to reach −80 dB and free its voice. That is the sound DECAY asks for; voice stealing reclaims the oldest.
- **STRIKE 1 ends held notes.** `param_sweep_test` allows the silence it finds once the ping has shut.
- **Goldens.** `amp_gate` and `amp_sends` are new. `row_amp`, `amp_vel_dimmed`, `amp_vel_live`, `modal_amp`, `large_modal_amp` and `large_bigviz_filter` (an AMP screen despite its name) are re-recorded for the new AMP, after comparing them. `sound_browser_pool_full` is re-recorded because a dense Sound is 15 B larger. #734 changes it too, so whichever lands second re-records it.
- **After tape-streaming merges main** (ADR 0194, bank 2 is required), `build.rs`'s gate glob becomes unconditional, the `not(bank2)` firmware arm of `gate::through` goes, and this ADR's sentence about builds without bank 2 is obsolete.

## Sources
- Mutable Instruments Plaits, github.com/pichenettes/eurorack: `plaits/dsp/fx/low_pass_gate.h`, `plaits/dsp/envelope.h` (`LPGEnvelope`), `plaits/dsp/voice.cc`, stmlib `dsp/filter.h` (`Svf`), MIT, Copyright 2014–2016 Emilie Gillet.
- PR #739 (branch `lpg-filter`, head 82c48d14): the port, its fixture `lpg_plaits_v1.bin` and gate checks; the lab, branch `lpg-circuit-lab` (`chimera-utils/src/lpg_gate.rs`); epic #720.
- The listening page: (private notes), rendered per `filters-last/ab-method.md` (`chimera-utils lpg-amp render`).
- ADRs 0060, 0110, 0123, 0137, 0155, 0192, 0194.
