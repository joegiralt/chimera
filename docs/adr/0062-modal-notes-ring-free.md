# 0062. A Modal note rings free after note-off

- **Status:** Proposed
- **Deciders:** project owner (UAT 2026-09-30); firmware (Modal 2, task 19)
- **Supersedes in part:** [0056](0056-modal-resonators-share-four-macros.md)
  (a note-off releases a string, #51)

## Context
ADR 0056 gave STRING and SYMP's main string a release: a note-off ramped
the loop gain over 5 ms to a 0.12 s T60 (`RELEASE_T60`). At the owner's
UAT a released string stopped dead: "even if I let go, it should continue
ringing — that's why it's called rings". Rings has no release; the
resonator rings on its own damping, and only a new strike or a steal
touches it. BANK, a lifted bow and SYMP's halo already rang on.

## Decision
- **Note-off stops the exciter, nothing else.** STRING and SYMP's main
  string keep their loop gain: the note rings on DAMP's T60, as held.
  A lifted bow still ramps from the bow's loop to DAMP's ring
  (`Release::lift`). The string's `Release` and `RELEASE_T60` are gone.
- **A note is freed on its own level**, 60 dB under its peak for 10
  blocks, never over −120 dBFS (ADR 0056): a quiet high note rings as
  long as a loud low one.

## Alternatives considered
- **A release that follows DAMP** (the old ramp, to DAMP's T60): the
  same sound as none, with a ramp to keep.
- **A release T60 knob:** a macro Rings does not have. DAMP and a
  modulated VCA already shape the tail.

## Consequences
- A released STRING or SYMP note holds its voice for its whole ring: 14 s
  at INIT's DAMP, SYMP's halo twice that. More steals, of louder tails.
- The sweep judges every Modal model as ringing on after note-off.

## Sources
- Owner UAT, 2026-09-30 (PR #252); `a_released_string_rings_on_damp`,
  `an_undamped_c5_frees_60_db_under_its_peak`.
- Mutable Instruments Rings (`rings/dsp/part.cc`, MIT): no note-off
  release.
