# 0082. ORBIT's UI and runner talk through one sequenced command log and one ack

- **Status:** Proposed
- **Deciders:** owner (approved the redesign before ORBIT merges; rulings
  Q1–Q8 of the design, 2026-10-03), firmware

## Context
The UI publishes `OrbitShared` and the runner publishes `OrbitView`, each
through a triple buffer (ADR 0021). A reader there sees only the newest
publish, may see it twice, and never sees an older one after a newer one.
The writer's spare slot is two publishes stale.

Each race found so far got its own guard: `presses` and `presses_seen`,
`arming` in four places, `scene_rev` in four, `landed`, `epoch` in four,
per-ring `rhythm_rev` and `notes_rev` with `revs` and `revs_of`,
`unseen`, `took` against `count`, and seven conditions on `absorb`. Both
of the adversarial review's findings were gaps between two of these
guards. The altitude and simplification reviews recommend one sequenced
intent and ack channel. The owner made space the top priority: AXI must
not grow, and flash must shrink and be measured.

## Decision
- **One log for events.** The UI packs each event into one byte (`Cmd8`):
  `TogglePlay`, `Cue(n)` (a switch, CLEAR of the playing slot, RELOAD),
  `Keep(n)` (SCENE turned back), `Arm(target)`, `Disarm`,
  `EditRhythm(mask)` and `EditNotes(mask)`.
  - The commands go in a ring of `LOG` = 8 bytes in `OrbitShared`, with a
    `u8` head. An entry carries no seq: seq `s` lives in slot `s & 7`.
  - The UI issues `s` only while `s − acked ≤ LOG`.
  - The runner applies `(applied, head]` each block, then sets
    `applied = head`.
  - Under that window the ring always holds the runner's whole gap. Every
    seq compare is between values at most `LOG` apart, so a `u8` never
    aliases, and no command is lost or applied twice however many
    publishes the triple buffer skips (proof in the design, § 1.8).
- **No long-lived seqs.** Results come back as levels, not seqs:
  - `acked`;
  - `cue`, the runner's waiting scene;
  - `armed`, its armed target;
  - `Captured.count` (`u8`): one capture per arming, so it advances at
    most `LOG + 1` between two UI reads.

  The UI lands its newest scene command when every command is acked and
  `cue` is empty. It mirrors `armed` while everything is acked.
- **One fold rule per class.** Within a frame, and while the runner
  stalls, the UI folds each class in a 3 B outbox:
  - presses by parity;
  - scene and arm commands, last wins;
  - edit masks by union.

  The runner folds a batch the same way and applies the flip last.
- **The UI owns `live`; the runner owns `working`.**
  - **Edits.** `SceneRing.touched` bits, set where a rev was bumped,
    become edit masks. The runner takes those rings' fields from
    `settings`.
  - **Drift.** It comes back whole in the view, because skipped views
    would lose deltas. The UI absorbs it once a loop, only while synced
    and with every command acked.
  - **The order in `sync`.** The UI harvests any edit still touched
    before it reads `idle`, and it reads `idle()` again after taking a
    capture, before it absorbs. A capture is an edit the runner has not
    applied yet: with the earlier `idle`, drift at a loop boundary would
    overwrite it.
- **In sync is one bit each way.** The UI publishes
  `synced = (no scene command awaiting landing)`. The runner sets
  `base_landed` from it and clears it only when it adopts a slot.
  Adopting `live` (a `Live` cue) keeps it set, because `working` is then
  the UI's `live`. Clearing it there would drop every edit made between
  that adoption and the UI's landing: the edit would be acked, never
  applied, and the next drift would revert it in `live`. Knobs and edits
  reach `working` only while `base_landed`. `Keep(n)` out of sync cues
  `live`. A start out of sync with nothing cued keeps `working`, the slot
  the UI is about to land, and does not adopt the stale `settings`. The UI's newest scene intent wins, which keeps ADR 0077's
  cancel-race behaviour and closes adversarial finding 1 by construction.
  A re-arm before the view keeps showing armed, because the view reports
  the runner armed again. That closes finding 2 by construction.
- **Load is the one command outside the ring.**
  - `OrbitUi::loaded()` runs before the load publish and takes a fresh
    seq `L` as `OrbitShared.load`. It then issues `Keep(n)` for the
    loaded playing slot `n`, in the same publish, so the runner learns
    what plays.
  - The runner still detects a load by `AudioShared.epoch` (ADR 0046, a
    `u32`), and then sets `applied = L`.
  - **The load gate is the epoch.** `OrbitView` carries the runner's
    `u32` epoch, and the UI keeps its newest load's. A view on another
    epoch is refused, however many loads pass without an audio block: a
    `u32` never aliases. While the load is pending, `Channel::ack` also
    refuses a view with `acked` outside `[L, L + LOG]`. A `u8` seq alone
    can't be the gate: it aliases after 28 back-to-back loads. This is
    the only stale-view check.
  - The runner clears its last capture's target at a load, which
    replaces `Captured.epoch`.
  - The load publish closure gets the post-load `UiState`.
- **The DAW transport stays on the audio thread**, off the log.
- **Space,** measured after the transport and capture moved onto the
  log, against 2dc9eefd (host `memory_budget_test`; release
  `--features orbit` firmware):
  - `UiState` without its `Project` stays at 3,832 B, within
    `OrbitUi`'s padding (gate 3,840 B).
  - AXI shrinks by 60 B, to 442,072: `OrbitView` 336 → 308 B (×3 −84),
    `OrbitShared` 876 → 880 B (×3 +12), `OrbitRunner` unchanged at
    832 B (its removed bytes were padding).
  - Flash `text`, fat LTO with one codegen unit (thin LTO moves by
    about ±1 KB between builds of the same code): 715,428 → 715,620 B,
    +192 B. Thin LTO: 773,724 → 776,108 B. Scenes, edits and loads
    alone measured +1,208 B fat; the transport and capture took back
    976 B. The gate is ≤ 0, so the owner rules on the rest.
  - The `UiState` guard rises from 4 KB to 5,120 B (owner's ruling). It
    guards no stack, copy or memory region: `UiState` is an AXI static
    built in place, charged a fixed 64 KB `UI_RESERVE`, never cloned or
    moved.

## Supersedes
- **ADR 0071**, "The engine owns the drift, the UI owns the knobs, and
  revs say who wins", and the consequence "The UI must bump a rev". They
  are replaced by the edit masks, `touched` and `base_landed`. The
  transport, window and note-off decisions stand.
- **ADR 0075**, "Arming is a new arming generation" and `Captured`'s load
  epoch. They are replaced by `Arm`, `Disarm`, `armed` and the load's
  capture clear. The capture-content rules stand.
- **ADR 0077**, "A switch is a generation", the knob-copy clause of "A
  queued slot always lands", and the mechanics of "The cancel race". They
  are replaced by `Cue`, `Keep`, `cue`, `synced` and `base_landed`. The
  behaviour, the copy at landing, the saves and the witness stand.
- **ADR 0080**, amended: "B6 is a press" now means a `TogglePlay` byte in
  the log instead of the `presses` count. Its toggle rule stands.

## Alternatives considered
- **One latest-intent register.** Two commands in one frame lose one, or
  queue in the UI, one per ack round trip. Rejected.
- **`u32` seqs and kept results** (`adopted`, `landed`, `Captured.arm`,
  this ADR's first draft). Correct, but about +100 B of `UiState` and
  +150 B of AXI. Its equality tests needed a "2³² commands" bound instead
  of a window. Rejected for space.
- **The runner owns the scene, every edit a command.** The UI still needs
  the scene to draw, lerp and save. The view would grow by a whole
  `Scene` (+1,176 B AXI). Rejected.
- **Drift as deltas.** The triple buffer skips views. Rejected.
- **Edits as one 10-bit command.** It doesn't fit a byte. Two 5-bit kinds
  keep `Cmd8` at one byte, at the cost of one more class per frame.
- **`LOG` = 4.** It saves 4 B per copy, but a worst-case frame (five
  classes) spills one command into the next frame. Kept as the first
  fallback if `UiState` measures over.

## Consequences
- 25 fields go and 14 come: net −11. `incoming`, `revs_of`, `requeued`,
  `unseen`, three epoch tests and the generation logic go. `count`,
  `took` and `loops_seen` narrow to `u8`.
- The runner's common block makes one `u8` compare where it made four
  generation compares and ten rev compares. Its worst block decodes 8
  bytes, about 0.2 µs.
- Drift absorption pauses, for at most a frame, while any command is
  unacked.
- CLEAR down's roll salt moves from `notes_rev` to the UI's seq.
- Flash is estimated at −0.7 to +0.5 KB and is not guaranteed to shrink.
  The measurement gate decides, and the levers are in the design's § 5.
- Tests that poked removed fields drive commands instead, with unchanged
  assertions (design, § 4.2). Seven property tests are added, one across
  more than 10,000 `u8` wraps (design, § 4.3).

## Sources
- Design: `docs/superpowers/specs/2026-10-03-orbit-command-channel-design.md`
- `.superpowers/sdd/2026-09-30-orbit/adversarial-review.md` (findings 1, 2)
- The altitude and simplification cleanup reviews of the `orbit` branch
- ADR 0021, 0025, 0046, 0071, 0075, 0077, 0078, 0080
- `chimera-core/src/triple.rs`, `orbit/runner.rs`, `orbit/scene.rs`,
  `ui/orbit/mod.rs`, `chimera-stm32/src/shared.rs`, `hw.rs`
  (`UI_RESERVE`), `tests/memory_budget_test.rs`, at 4989eb9d
