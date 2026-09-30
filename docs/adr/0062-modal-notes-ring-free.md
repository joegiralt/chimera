# 0062. A Modal note rings free; a re-struck key re-plucks its own string

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

A re-struck key took a new voice and started a fresh, cleared string
beside the old one: the owner wants a key "like re-plucking one of six
guitar strings".

## Decision
- **Note-off stops the exciter, nothing else.** STRING and SYMP's main
  string keep their loop gain: the note rings on DAMP's T60, as held.
  A lifted bow still ramps from the bow's loop to DAMP's ring
  (`Release::lift`). The string's `Release` and `RELEASE_T60` are gone.
- **A note is freed on its own level**, 60 dB under its peak for 10
  blocks, never over −120 dBFS (ADR 0056): a quiet high note rings as
  long as a loud low one.

- **A re-struck key takes its own voice back** (`Allocator::pick`, rule
  2): same Part, same note, not Mono, not dying, held or ringing. Steals
  still take the oldest released tail first, then the oldest held note.
- **A re-strike adds to the ring and clears nothing.** STRING and SYMP's
  main string add a new pluck, shaped as a fresh one is, over the period
  the loop reads next (`KsString::add_pluck`, streamed from the noise
  state, no scratch line); SYMP's halo rings on; a bow is set back on its
  ringing string; BANK's strike always added. The macros, the DC blocker
  and the note's peak carry on: nothing snaps.
- **A steal of another note** still starts its string clean, after the
  voice's 2.7 ms fade where the engine or model changes; a same-model
  steal of a loud C2 tail passes the click check without one.

## Alternatives considered
- **A release that follows DAMP** (the old ramp, to DAMP's T60): the
  same sound as none, with a ramp to keep.
- **A release T60 knob:** a macro Rings does not have. DAMP and a
  modulated VCA already shape the tail.

- **Re-strike by the engine alone** (a same-note note-on on a fresh
  voice): two voices then ring the one string's pitch, and the pool fills
  with copies of a repeated note.
- **Shape the combined line in place:** the comb and COLOR's passes would
  filter the old ring as well as the new pluck.

## Consequences
- A released STRING or SYMP note holds its voice for its whole ring: 14 s
  at INIT's DAMP, SYMP's halo twice that. More steals, of louder tails.
- The sweep judges every Modal model as ringing on after note-off.
- A repeated note costs one voice, not one a strike. A key re-struck from
  another channel re-strikes the same voice; that channel's note-off
  releases it.

## Sources
- Owner UAT, 2026-09-30 (PR #252); `a_released_string_rings_on_damp`,
  `an_undamped_c5_frees_60_db_under_its_peak`, `a_restruck_key_reuses_its_voice`,
  `a_restrike_adds_to_the_ring`, `a_restrike_reuses_its_string_and_nothing_clicks`.
- Mutable Instruments Rings (`rings/dsp/part.cc`, MIT): no note-off
  release.
