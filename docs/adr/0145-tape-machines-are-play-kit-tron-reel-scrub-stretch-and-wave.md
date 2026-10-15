# 0145. TAPE's machines are PLAY, KIT, TRON (on probation), REEL, SCRUB, STRETCH and WAVE

- **Status:** Proposed
- **Deciders:** owner (answers to `prep-and-play.md` § 3, 2026-10-15), firmware
- **Supersedes in part:** [0093](0093-tape-plays-12-bit-card-sources-through-six-ram-models.md) § Machines (the lineup, ONESHOT's SLICE, TRON on `Plain` sources held whole)

## Context
#476: the owner found TAPE's seven machines "samey", and wanted TAPE to be a sampler with a preparation stage and a playing stage. ONESHOT, LOOP and PING-PONG play one window three ways. ONESHOT's SLICE (16 equal slices) is a thin kit. TRON as built is mostly ONESHOT with a run-out. The owner answered the lineup, KIT's defaults, TRON's status, REEL's programs and RESAMPLE on 2026-10-15 (spec `2026-10-15-tape-sampler-design.md`, decisions 1, 4, 5, 6, 7).

## Decision
- **MODEL, in screen order:** PLAY, KIT, TRON, REEL, SCRUB, STRETCH, WAVE. GRAIN is later (code 10 reserved).
- **Codes:** PLAY 0 (LOOP's), TRON 6, SCRUB 4, STRETCH 5, WAVE 3; KIT 8 and REEL 9 are new. ONESHOT (7) loads as PLAY with MODE ONE and FADE HOLD; PING-PONG (2) as PLAY with MODE PING. ONESHOT's SLICE loads and is ignored (no-legacy-compat).
- **Each machine takes prep types** (ADR 0146) and refuses others before allocation, with a reason: PLAY RAW or ZONES; KIT KIT; TRON ZONES or RAW; REEL REEL; SCRUB, STRETCH and WAVE RAW.
- **PLAY:** MODE (ONE, LOOP, PING) over the existing paths; a release FADE (DRAG relabelled), whose last step HOLD exists only in MODE ONE; streams automatically when the source doesn't fit the arena (ADR 0147).
- **KIT:** up to 64 slices on keys from BASE (C1, 36); per slice CUT or STACK, a choke group (0 none, 1–4), LEVEL, TUNE, REV; by default black keys CUT in group 1 and white keys STACK. Whole source in RAM.
- **Cut and choke are one module both KITs use** (`chimera-engine/src/choke.rs`: `Rehit`, `ChokeGroup`, `KeyPolicy`, `victims`), run by the allocator at note-on and cutting through its existing fade. TAPE's KIT and #215's KIT Part each keep their own policy table. Splitting them later is a copy of that one file.
- **TRON is on probation:** PLAY's voice and streaming with its own page (RUN, FADE, WOW) and defaults (RUN 8 s, FADE 32 ms, WARBLE). By the release it must offer something PLAY can't, MOTOR (one tape motor a Part) or a spring REWIND being the candidates. If it doesn't, it folds into PLAY: factory TRON Sounds become PLAY Sounds and code 6 loads as PLAY with the same settings. Zones now (two an octave); per-key tapes later.
- **REEL:** up to 32 splices; SPLICE (live, modulatable) and SPEED (bipolar, ±2×) on DECK; up to 8 programs of up to 8 steps (SPLICE, DIR → ← ↔, REPEATS 1–16, SPEED, CHANCE 0–100 %) and an end (LOOP, STOP, NEXT). CHANCE is rolled once a step, from a per-Part seed, one step early, so the next splice is always known a step ahead.
- **RESAMPLE ships in the first TAPE release:** one note of a Part's Sound rendered offline to a new `.SRC` (spec § 6).
- New params (ids fixed when each slice builds): MODE 15, RUN 16, SHIFT 17, CHOKE 18, REVERSE 19, PROGRAM 20, GAP 21.

## Alternatives considered
- **Keep ONESHOT, LOOP and PING-PONG as machines.** Three MODEL steps for one window played three ways: the "samey" complaint.
- **KIT as ONESHOT's SLICE grown to 64.** No per-key policy, no choke, no per-slice tune or level: what the owner's example needs.
- **Cut and choke only in #215's KIT Part.** TAPE's KIT would wait on #215, or build a parallel copy now.
- **One shared kit type for both KITs (slices and patches in one table).** It couples a sample's slices to a project's keymap; the owner is unsure the two should share at all.
- **Drop TRON now.** The owner keeps it provisionally; MOTOR may make it worth a MODEL step.
- **REEL programs without CHANCE.** The owner asked for ORBIT-like probability.

## Consequences
- Old Sounds load safely as PLAY; their sound may differ (ONESHOT's SLICE is gone).
- The allocator gains a policy check at note-on: a no-op for every Part but a KIT's.
- KIT and REEL publish their tables as prep rows in the pool (ADR 0146), sharing `MAX_SOURCES`.
- REEL's name collides with the master tape stage's on-screen word (ADR 0093). One must change before slice 5 (spec open question 1).
- TRON's verdict is a release gate.

## Sources
- `docs/superpowers/specs/2026-10-15-tape-sampler-design.md` §§ 2, 6, 9.
- (private notes) (§ 4: the owner's answers).
- Precedents (behaviour only, ADR 0114): Akai MPC chop → drum program and mute groups; Octatrack 64 slices; Ableton Simpler slicing (poly vs mono); Make Noise Morphagene reels, splices, Organize, Vari-Speed; Mellotron M400.
- Issues #476, #215.
