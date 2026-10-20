# 0183. Streamed notes are billed by the reach in use, the quietest held note goes first, and TRON streams

- **Status:** Proposed
- **Deciders:** owner (#698), firmware
- **Amends:** slice 1 § 5.5 (admission's reach), § 2.3 (what streams)

## Context
Musical findings 1, 5 and 13, perf finding 2. A note's card bill took every PITCH route at full swing (INIT's wheel vibrato at DEPTH 0 included), the whole bend range and 33 cents of wow at TAPE 0: ×1.17 over what the note reads. A 24 kHz bed fitted 6 notes at ROOT, and over budget each note stole the oldest held, a chord's foundation. TRON, whose note is ONE's path, was refused on streamed sources. `COST_STREAM`'s 100 cycles included a `vdiv.f32` a sample.

## Decision
- **The reach in use** (`Instrument::bill_in`): each route's source by its live swing (an LFO by its DEPTH unless DEPTH is routed, so the wheel's vibrato at rest reaches nothing), wow only while TAPE runs (`tape_runs`), and the bend the wheel holds now, up (the glided reach or where the wheel sits). A wheel or DEPTH moving up re-bills the note each block (`refresh_in`, `raise`); if the card can't take it, the note holds at its cap, counted (`held_at_cap`), as before.
- **Steals:** released voices first, the oldest; then the quietest held (its VCA's gain last block, `Age::level`), then the oldest (`Age::rank`).
- **TRON streams** (`needs_ram`): one START head, as ONE (`heads_in` places one head off LOOP); reversed it needs RAM.
- **`STREAM_BUDGET_BPS` stays 315,978** until the unit's stress run measures 2× B4's load; `STREAM_BUDGET_2X_BPS` is named for it.
- **`COST_STREAM` 90:** the gain multiplies by `1/1440` and skips its loop at rest (perf finding 2: 14 cycles a sample), pending the unit's re-measure.
- **SOURCE's band says the polyphony** (`notes_at_root`, ADR 0186).

## Alternatives considered
- **Keep the bend range billed.** Every chord pays for a bend nobody plays; bending a full chord over budget now caps its pitch instead, the rarer case.
- **Steal the oldest held.** It takes the chord's root (the owner's "choke").
- **LOOP billed by the loop it can reach.** The bill is a Part's, from params alone, with no source frames; it needs the source's length in the cost path, filed as a follow-up.

## Consequences
- At ROOT with the wheel at rest and TAPE 0, a 24 kHz bed fits 7 notes (8 × 41,143 B/s is over the budget by 4 %); 16 kHz fits 11.
- A held chord bent up past the budget holds at its cap until a released note frees room.

## Sources
- Musical findings 1, 5, 13; perf findings 1, 2; #698, #623 (B4).
- `chimera-engine/src/instrument/admission.rs`, `stream/admit.rs`, `stream/cursor.rs`, `dsp/engines.rs` (`pitch_reach_by`), `dsp/tape_engine/params.rs` (`needs_ram`).
