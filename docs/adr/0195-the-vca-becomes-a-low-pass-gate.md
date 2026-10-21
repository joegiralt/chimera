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
- **The VCA stays a VCA.** AMP gains three knobs: OUT params 7 LPG, 8 DECAY and 9 STRIKE, idents `LPG`, `DECAY`, `STRIKE`, `Uni` 0..1, default 0. They are smoothed (`Smoothing::KNOB`), modulatable and MIDI-mappable. With all three at 0 the voice runs today's VCA code, untouched. Every existing Sound renders bit for bit as before (`the_gate_at_zero_is_todays_vca_bit_for_bit`, FNV-1a `0xf54a71e5c050585d`, pinned by cf970323 on origin/main 5a2e50e7 before the change). It covers ALGO, MODAL, TAPE and VA INIT; no route, ENV 1 into VCA, and ENV 1 with LFO 1; CLEAN, 2040, DIRTY, LADDER and SEM; VEL 1 and ½; a re-strike at another velocity; key up and tail; uneased and glided; and every VA MODE. Old Sounds have no record for 7–9, so they load at 0.
- **LPG** is Plaits' COLOR inverted, `COLOR = 1 − LPG`, so 0 means off. At 0 the bleed is 1, so the gated signal passes the filter untouched: a VCA. At 1 the gate is all filter, the darkest. LPG also weighs in voice.cc's compressed LEVEL, `l + LPG·(1.3·l/(0.3 + l) − l)`, so the gain law moves to Plaits' only as LPG reaches 1. Plaits' default COLOR ½ is LPG ½.
- **DECAY** is Plaits' decay (voice.cc: the short decay `200·12/fs·2^(−8·d)` a step, the tail `20·12/fs·2^(−6·d + COLOR)`). Its first 1/64, two knob ticks, fades the vactrol in: the step's coefficient is `1 − w·(1 − c)`, `w = min(1, 64·DECAY)`. At 0 the state is the level each step, and the routes' detail between the steps rides on the step's ramp, weighted `(1 − w)(1 − STRIKE)`. So at DECAY 0 the gain is the routes' own, sample for sample: the gate at LPG 0 nulls the plain VCA at −100 dB or better. From DECAY 1/64 up it is Plaits' decay as built.
- **STRIKE** pings the gate at each note-on, as Plaits' `ProcessPing` does. The vactrol rises by `2·12·f_note/fs` a step, as Plaits', to the level plus STRIKE × the note's velocity, then falls through DECAY. The level is × (1 − STRIKE), so at 1 the strike alone drives the gate, and in between the strike adds to the routed envelope. It rings out whether or not the key is held. A note on a voice at rest starts the gate shut, as Plaits' before its first note, so every note's strike rises from rest at its velocity; a strike on a sounding note rises from where the vactrol is, and one struck softer than the vactrol already stands never drops it to its peak (Plaits' peak is 1, so it never meets this). The strike reads the velocity itself, not AMP's VEL, so VEL stays the routes' (and dimmed with none), as today.
- **What drives it.** With routes into VCA: the routes' gain × AMP's VEL, Plaits' LEVEL (`ProcessLP`). With no route on ALGO and MODAL: 1, since their own envelopes shape the sound. With no route on TAPE and VA, where the key ends the note today: the key, and after key up a fall over the voice's 128-sample fade, a step at a time. OUT LEVEL multiplies after the gate, ramped as today.
- **No double gating.** Under the gate, the routes drive it and the VCA multiplies by OUT LEVEL alone. At LPG 1/128 the gate plays VA INIT's level within 0.2 dB, and ENV 1's SUSTAIN ½ is 6 dB under SUSTAIN 1, not 12 (`the_gate_never_gates_twice`).
- **Voices end when the gate shuts.** While the gate runs, it holds the voice until its gain is under −80 dB and no strike is rising. Below STRIKE 1, the voice is also held as the plain VCA would hold it: a route's source still holding it, or, with no route, its engine. At STRIKE 1 the gate alone holds it, so a struck note ends when the gate shuts even with the key held (the key keeps its slot, as any held note's). The key-up fade of a TAPE or VA note with no route gives way to the gate's fall.
- **The bleed ramps.** Plaits holds its bleed for its 12 samples; here it ramps across them, so a fast LPG never steps. At LPG 1 the bleed is 0 throughout, so the fixtures still null.
- **Turning it on.** When the gate starts on a note already sounding it starts where the plain VCA is: its state at the level, the bleed whole and LPG and STRIKE ramping in from 0, with no ring left.
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
- **Flash** (each set as `just check` builds it, against origin/main 5a2e50e7):
  - **Bank 2** grows 4,384 B on `orbit` (`.bk2` 185,504 → 189,888): the kernel with `Lpg::block` inlined into it, and the bill.
  - **Bank 1** on `orbit` grows 748 B (886,120 → 886,868). What it is, from the symbols:
    - **348 B of closed-door data:** `OUT_SPECS`' three specs (216), their `KNOBS` entries (60), `ROW_WORDS`' AMP and SENDS (24), `spec`'s switch table (12), and the words LPG, DECAY, STRIKE, DCAY, STRK and SENDS (32).
    - **200 B for the Sound's 12 more bytes** (`OutParams::gate`). This was measured by building main with that one field added and nothing else: 886,320 B. Every struct that holds a Sound moves, and its offsets need longer instructions.
    - **About 170 B of door stubs** that cannot move:
      - `Instrument::render_with` +56: the call into `GateDoor::run` and the plain VCA as its fallback.
      - `Voice::trigger` +68: the strike's flag. The store is one instruction; the rest is the inliner's.
      - `Voice::init_chain` +34: the gate's state written with the voice's.
      - `recost` +16: the bill's call into `GateDoor::cost`.
    - The rest is alignment and inlining noise, which swings about ±200 B from build to build (`handle_from_with`, `render_half`).
  - **Outside ADR 0198's exception 1.** The ~348 B of data and ~170 B of stubs are exception 1's. The ~200 B of layout churn is not new code but exceeds the guard's 64 B slack, as GRAIN's did (ADR 0202), so like 0202's it needs the owner's acceptance by name; it is recorded here, pending that. No stub can move: each is the bank-1 call or store the door needs.
  - The other sets, bank 1 before → after (B): `default` 865,716 → 865,832 (+116); `bank2-gate` 867,600 → 867,732 (+132); `fault-inject` 838,924 → 839,752 (+828); `bench-algo,bank2` 877,564 → 878,168 (+604); `bench-algo,master-tape,bank2` 873,528 → 874,004 (+476); `bench-routing,bank2` 874,816 → 875,436 (+620); `bench-memory,bank2` 863,652 → 864,432 (+780); `bench-stress,bank2` 877,288 → 878,012 (+724); `bench-orbit,bank2` 868,828 → 869,768 (+940); `bench-tape,bank2` 848,168 → 849,132 (+964). `filter-probes` 892,932 → 897,128 (+4,196) also carries `lpg_probe`, the count's probe, and is never shipped.
  - Each `tools/bank1-budget.txt` line is raised to its measured size and names this ADR (ADR 0198's exception 1 for the data and stubs; the layout churn as above).
- **RAM:** the voice grows 72 B (6,584 → 6,656): the gate's 68 B and padding. `VOICE_CHAIN_BYTES` goes from 2,336 to 2,408. A Sound's params grow 12 B (`ParamSnapshot` 648 → 660). `MAX_SOUND_COST` goes from 2,656 to 2,672 B, since a dense Sound now has 282 params (2,658 B).
- **Cost** (`tools/loop_count.py` on the `filter-probes` ELF, `lpg::lpg_probe`): the routed loop is unrolled 4×, 127 instructions a time round (31.75 a sample), and the unrouted one 68 (17). The block's 654 instructions are 10.2 a sample. Its step, about 122 instructions and 2 `vdiv`, runs 5⅓ times a block: 8.3 more and 0.21 `vdiv` a sample with the block's own. So `ceil5(50.2 × 1.606 + 12 × 0.21)` = **85 cycles routed** and 60 unrouted. `LPG_COST` bills 85 either way, on top of the VCA it does not credit, and only while LPG, DECAY or STRIKE is above 0 or routed. This is a static count, not yet read on the unit.
- **Voices** (`the_gate_bills_only_when_up_and_its_voices`), INIT Sounds beside the reserve, rev V (480 MHz) / rev Y (400 MHz):

  | Filter in front | ALGO INIT | ALGO INIT, gate up | VA INIT | VA INIT, gate up |
  |---|---|---|---|---|
  | SEM | 7 / 5 | 6 / 4 | 8 / 6 | 7 / 5 |
  | 2040 (REV2) | 6 / 5 | 6 / 4 | 7 / 6 | 7 / 5 |

  The gate costs one voice on rev Y everywhere, and one on rev V with SEM.
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
