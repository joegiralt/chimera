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
- **A voice keeps the key that started it** (`Instrument::keys`). The
  allocator books the note that sounds (so a Poly re-strike, the
  stealing rules and a waiting note see the pitch), and a note-off
  matches origin, channel and key. A key-up releases its snapped note on
  every Part, and a key-up of the snapped pitch releases nothing it did
  not start.
- **Re-snap from the raw key.** At a segment change or a landed scene
  (`Events::seg_changed`, `adopted`) the runner calls `resnap`: each
  held, live, not-dying voice on a RE-SNAP Part moves to its key snapped
  to the new harmony (`Allocator::renote`), never from where it sounded,
  so repeated changes never drift. SNAP OFF there snaps to the key
  itself. STAY voices, released voices and ORBIT's are untouched.
- **The glide is ADR 0065's.** `Voice::retune` moves the engine's note
  with no strike, on the pitch path (ADR 0042): Algo's ratio-1 increment
  and Modal's pitch ratio take a `dsp::Glide` from the pitch heard to 1,
  a one-pole in log pitch with a time constant of `RESNAP_GLIDE_MS / 3`.
  So 40 ms is 95 % of the way, as GLIDE TIME is, and it lands within 0.12
  cent about 100 ms in. The envelopes and modulators run on; NOTE reads
  the new note. Modal sizes its rings for the glide's longest period
  first (`fit`), as a glide steal does. A fading voice moves only the
  note it starts after the fade.
- **Cost bounded and billed.** A retune is a `log2f` and an `expf`, once
  per voice per change. A Part at HELD RE-SNAP on Modal bills
  `ModalEngine::PITCH` (and `HALO_PITCH` on SYMP), the per-block retune a
  glide runs, unless STEAL GLIDE or a PITCH route already bills it
  (`Voice::glide_cost`). A SYMP voice's ring growth is taken from the
  block's clear budget (spec § 4.8); one that doesn't fit waits for the
  next block's (`resnap_to`). Nothing allocates or blocks.
- **`Voice::retune(note, glide_samples, pool)`** takes the Sympathetic
  pool, which Modal's `fit` needs for the halo; the plan's signature had
  none.
- **SETTINGS › PART › LIVE SNAP** (owner Q6) is a `OnePage` leaf, block
  def 85, with SNAP and HELD bound to `BlockRef::Part`, the active Part's,
  crumb `SNAP`. The cells show words (focus glyph NONE); HELD's two
  values are peers.
- **Disk:** PART block (25) ids 8 SNAP (`OFF`, `SCALE`, `CHORD`, codes
  0–2) and 9 HELD (`STAY`, `RE-SNAP`, codes 0–1). A file without them
  decodes to OFF and STAY (ADR 0045).

## Alternatives considered
- **Snap in the runner before `handle`:** one note for every Part on the
  channel, so two Parts could not snap differently, and the note-off
  would need the runner to remember each snapped key.
- **Match note-offs by the sounding note:** a key-up would miss its own
  snapped voice, and a re-snapped voice would be stuck.
- **A glide linear in log pitch, exactly 40 ms:** lands on time, but is
  a second glide law beside ADR 0065's, which the owner chose for steals.
- **Re-strike at the new note:** an attack per chord change; measured as
  a click on Modal BANK (205×) and TX BASS (59× by the control diff).

## Consequences
- Two keys that snap to one pitch on a Poly Part share a voice (a Poly
  re-strike), and the first key's key-up then releases nothing.
- The click judges (`Tape::click`, the control diff) do not hear an
  instant pitch step: a phase-continuous jump has no discontinuity. The
  glide is held by the test's own check (a block in it is still near the
  start; 40 ms in, 95 % there).
- Modal's `fit` is not proven needed at re-snap intervals (removing it
  passes the judges at 3 semitones); it stays for the longer ones and for
  parity with the glide steal.
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
