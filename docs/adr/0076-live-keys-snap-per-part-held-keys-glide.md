# 0076. Live keys snap per Part in the Instrument; a held key re-snaps from its raw key on the pitch path

- **Status:** Proposed
- **Deciders:** owner (spec § 5.2, Q6 2026-10-02), implementer (plan Task 15)

## Context
Spec § 5.2 snaps a played key to ORBIT's key or chord while ORBIT runs,
per Part (SNAP: OFF, SCALE, CHORD), and lets a held key follow a chord
change (HELD: STAY, RE-SNAP) over `RESNAP_GLIDE_MS` = 40 ms. Before this,
a note-off matched the allocator's note, and a voice's pitch was fixed
from its note-on (ADR 0065 glides only at a steal). Three things needed
deciding: where the snap happens, what a note-off matches once the note
sounding is not the key pressed, and how a held voice moves without a
click.

## Decision
- **The `Instrument` snaps, per Part.** The runner hands it the harmony
  every block (`set_harmony`, `None` while ORBIT is stopped or its
  external transport is down), and `handle_from` snaps a `Live` note-on
  for each Part on the channel to that Part's SNAP. ORBIT's notes are
  snapped in their ring and pass as they are.
- **A voice keeps the keys that hold it** (`Instrument::held_keys`, a
  bit per MIDI note, and `keys`, the one it re-snaps from). The allocator
  books the note that sounds (so a Poly re-strike, the stealing rules and
  a waiting note see the pitch), and a note-off matches origin, channel
  and key. Two keys that snap to one pitch on a Part share its voice (a
  Poly re-strike), and it is let go only when the last of them is up
  (owner's ruling, review I3): hold C, tap D (it snaps to C), let D go,
  and C sounds on. A key-up of the snapped pitch releases nothing it did
  not start. The voice re-snaps from its latest key still down.
- **Re-snap from the raw key, whenever the harmony changes.** The runner
  calls `resnap` whenever the harmony it hands on differs from the last
  one: a segment change, a landed scene, a B6 start or a MIDI Start back
  to bar 0 (which reset the segment with no segment step), a chord
  played into the current segment. Each held, live, not-dying voice on a
  Part at SNAP SCALE or CHORD and HELD RE-SNAP moves to its key snapped
  to the new harmony (`Allocator::renote`), never from where it sounded,
  so repeated changes never drift. So a key held while ORBIT is stopped
  (raw) snaps when it starts, on a RE-SNAP Part; a STAY Part keeps it
  raw. SNAP OFF, STAY, released and ORBIT's voices are untouched.
- **The glide is ADR 0065's.** `Voice::retune` moves the engine's note
  with no strike, on the pitch path (ADR 0042): Algo's ratio-1 increment
  and Modal's pitch ratio take a `dsp::Glide` from the pitch heard to 1,
  a one-pole in log pitch with a time constant of `RESNAP_GLIDE_MS / 3`.
  So 40 ms is 95 % of the way, as GLIDE TIME is, and it lands within 0.12
  cent about 100 ms in. The envelopes and modulators run on; NOTE reads
  the new note. Modal sizes its rings for the glide's longest period
  first (`fit`), as a glide steal does: grown block by block on the way
  down, each ring would `copy_within` its growth on the audio path,
  unbilled, which `grow`'s continued loop keeps inaudible
  (`modal::tests::a_retune_grows_no_ring_on_its_way` fails without it).
  Done at once, the growth is one write, which a SYMP voice bills. A
  fading voice moves only the note it starts after the fade.
- **Cost bounded and billed.** A retune is three transcendentals, once
  per voice per change. A Modal Part where a re-snap can act (SNAP on,
  HELD RE-SNAP) bills `ModalEngine::PITCH` (and `HALO_PITCH` on SYMP),
  the per-block retune a glide runs, unless STEAL GLIDE or a PITCH route
  already bills it (`Voice::glide_cost`, sharing `pitch_cost` with
  `EngineSlot::cost`). A SYMP voice's ring growth is taken from the
  block's clear budget (spec § 4.8); one that doesn't fit waits for the
  next block's (`resnap_to`). Nothing allocates or blocks.
- **STRING's and BOWED's growth is not billed,** as a glide steal's is
  not: the clear budget is SYMP's, whose note-on clears eight lines.
  `snap` takes the nearest tone and no quality leaves a gap over 5 st, so
  a snap moves a key at most 2 st and a re-snap at most 4 st: a ring
  grows by at most 26 % of its period (2^(4/12) − 1), about 1 KB of the
  longest ring (`RING_BYTES`, 3,924 B), and eight voices re-snapping in
  one block write about 8 KB, a few thousand cycles against a block's
  640,000.
- **`Voice::retune(note, glide_samples, pool)`** takes the Sympathetic
  pool, which Modal's `fit` needs for the halo; the plan's signature had
  none.
- **SETTINGS › PART › LIVE SNAP** (owner Q6) is a `OnePage` leaf, block
  def 85, with SNAP and HELD bound to `BlockRef::Part`, the active Part's,
  crumb `SNAP`. The cells show words (focus glyph NONE); HELD's two
  values are peers.
- **Disk:** PART block (25) ids 8 SNAP (`OFF`, `SCALE`, `CHORD`, codes
  0–2) and 9 HELD (`STAY`, `RE_SNAP`, codes 0–1; it reads RE-SNAP on
  screen, the ident keeps the `_` every multi-word ident has). A file without them
  decodes to OFF and STAY (ADR 0045).

## Alternatives considered
- **Snap in the runner before `handle`:** one note for every Part on the
  channel, so two Parts could not snap differently, and the note-off
  would need the runner to remember each snapped key.
- **Match note-offs by the sounding note:** a key-up would miss its own
  snapped voice, and a re-snapped voice would be stuck.
- **Re-snap only at a segment step or a landed scene** (the plan's
  list): a MIDI Start or a B6 restart goes back to segment 0's chord with
  no step, and held notes clashed with the keys played after it.
- **One voice per key (a doubled pitch) instead of shared voices:** two
  voices in unison for one pitch, and a chord of merged keys takes twice
  the voices.
- **A glide linear in log pitch, exactly 40 ms:** lands on time, but is
  a second glide law beside ADR 0065's, which the owner chose for steals.
- **Re-strike at the new note:** an attack per chord change; measured as
  a click on Modal BANK (205×) and TX BASS (59× by the control diff).

## Consequences
- Two keys that snap to one pitch on a Part share a voice and re-strike
  it, rather than doubling the pitch; it re-snaps as the latest key down.
- The click judges (`Tape::click`, the control diff) do not hear an
  instant pitch step: a phase-continuous jump has no discontinuity. The
  glide is held by the test's own check (a block in it is still near the
  start; 40 ms in, 95 % there).
- Without `fit` no audio judge hears a difference (`grow` continues the
  loop); the ring-length test holds it.
- A Modal Part at RE-SNAP plays a voice fewer where the budget is tight.

## Sources
- Spec `docs/superpowers/specs/2026-09-30-orbit-design.md` § 5.2; plan
  Task 15; ADR 0042 (pitch path), 0045 (neutral defaults), 0065 (glide
  steal), 0067 (focus glyphs).
- `chimera-core/src/instrument.rs` (`set_harmony`, `resnap`,
  `serve_resnaps`, `PartAudio::live_note`), `dsp/voice.rs`
  (`Voice::retune`), `dsp/engines.rs`, `dsp/algo/engine.rs` and
  `dsp/modal/mod.rs` (`glide_to`), `orbit/runner.rs` (`block`),
  `part.rs` (`LiveSnap`, `Held`), `ui/settings/leaves.rs` (`LIVE_SNAP`).
- Tests: `chimera-core/tests/orbit_snap_test.rs`.
