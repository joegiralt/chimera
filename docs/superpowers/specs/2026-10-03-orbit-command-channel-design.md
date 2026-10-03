# ORBIT command channel: Design

**Date:** 2026-10-03, revised the same day for space
**Status:** Draft. The owner accepted Q1–Q6 of the first draft (§ 7), made space the top priority, and ruled on the `UiState` cap (§ 5.0). This revision is awaiting review.
**Builds on:** branch `orbit` at 4989eb9d.
**Decision record:** ADR 0082 (Proposed). It supersedes the handshake parts of ADR 0071, 0075 and 0077, and amends 0080's `presses`.
**Inputs:** the altitude and simplification reviews (findings 1–4, 6, 8 and 13 of the altitude review; the "six counters" row of the simplification review), the adversarial review (findings 1 and 2), and the code at 4989eb9d: `orbit/runner.rs`, `orbit/scene.rs`, `ui/orbit/mod.rs`, `triple.rs`, and the shells' publish and view calls.

**What the space revision changed.** The first draft numbered everything with a `u32` sequence number and kept long-lived seq copies (`adopted`, `landed`, `Captured.arm`) to compare against. It cost about +100 B of `UiState` and +150 B of AXI. This draft keeps only **in-flight** state:

- Seqs are `u8`, and every compare between them is inside the log's 8-entry window.
- A command packs into one byte, and an entry has no seq of its own.
- The results come back as levels, `cue` and `armed`, instead of seqs.
- The runner's "in sync" is one bit the UI publishes.

`UiState` stays at its size, AXI shrinks, and the proofs are redone for the window (§ 1.8).

## Intent

The UI thread and the audio-thread runner share two triple buffers (ADR 0021). The UI publishes `OrbitShared` inside `AudioShared`. The runner publishes `OrbitView`. Each race found so far got its own counter or guard:

| Race | Guard added |
|---|---|
| B6 presses between two runner reads | `presses` (u8) and `presses_seen`, compared by parity |
| A capture armed again in the frame one is taken | `arming` in `OrbitShared`, `Take` and `Captured`; `took` against `count` |
| A stale view landing a switch | `scene_rev` (UI, shared, runner, view) and `landed` |
| A view or capture from before a load | `epoch` in `OrbitView`, `Captured`, `OrbitUi` and `OrbitStatus` |
| Drift overwriting an edit, or the reverse | per-ring `rhythm_rev` and `notes_rev`, `revs`, `revs_of`, `copy_knobs` |
| Knobs taken from a `live` the UI hasn't landed | the `unseen` heuristic |
| Drift from another scene than the one landed | `absorb`'s seven conditions |

Each guard answers the same question: *has the other side seen what I did?* Each answers it with its own counter, wrap rule and compare. Both of the adversarial review's findings were gaps between two of these guards.

This spec replaces them with **one sequenced command log from the UI, one ack from the runner, and three small levels** (`synced`, `cue`, `armed`).

**Done when:**

- Every UI-to-runner event is a one-byte `Cmd` in one `CmdLog`. The runner reports `acked`.
- None of these is left in the tree: `presses`, `presses_seen`, `arming` (any of its four), `scene_rev` (any of its four), `OrbitShared.scene`, `.queued`, `.capture`, `incoming`, `unseen`, `requeued`, `landed`, `epoch` in `Captured` and `OrbitStatus`, `rhythm_rev`, `notes_rev`, `Revs`, `revs_of`.
- **Space gates** (measured, § 5):
  - `UiState` without its `Project` ≤ 3,840 B: today's 3,832 plus the UI's load epoch (owner, 2026-10-03, within the 5,120 B guard of § 5.0).
  - AXI resident ≤ today's.
  - Release `--features orbit` firmware flash at or below the baseline measured before commit 2 (§ 6).
- Every race test in § 4.2 passes with its assertion unchanged. The property tests in § 4.3 pass on 4 seeds × 1M steps.

## Principles (binding)

- **One writer per datum.** The UI writes `live`, the slots and its intents. The runner writes `working`, the transport and the capture under way.
- **Events go in the log; levels stay in the snapshot.** A press, a switch, an arming and an edit are events, each acted on once. BPM, SYNC, the knobs and "synced" are levels.
- **Only in-flight state is numbered.** A seq is compared only with another seq at most `LOG` apart. Nothing keeps an old seq to compare later.
- **Functional core.** `orbit/channel.rs` (the packing, the outbox, flush, receive) is pure and property-tested on its own. The runner and `OrbitUi` are thin shells over it.
- **The audio thread never allocates or blocks.** Its added work is at most `LOG` = 8 one-byte entries a block.

## 1. The model

### 1.1 Ring or register

| | Bounded ring, `LOG` = 8 one-byte entries | One latest-intent register |
|---|---|---|
| Two commands in one frame (B6 and SCENE) | Both delivered, in order | The second waits a whole ack round trip (50–100 ms) or overwrites the first |
| Commands while the runner reads only the newest snapshot | All of them | Only the newest |
| UI side | Log (9 B), plus a 3 B outbox for stalls | A queue in the UI anyway |
| Size per copy | 9 B (`head` + 8 entries) | 2 B |

**Chosen: the ring of 8** (owner, Q2). A register needs a UI queue to avoid losing commands, and that queue is a ring with worse latency. The proofs hold for any `LOG` from 1 to 64. At most 5 classes of command can be issued in one frame, so 8 entries hold any frame's commands with room for a frame the runner misses. `LOG` = 4 would save 4 B in `UiState` and in each `OrbitShared` copy, at the cost of one worst-case frame spilling into the next (§ 5, the fallback).

### 1.2 Types

```rust
pub const LOG: usize = 8;            // a power of two dividing 256: `seq & 7` stays continuous across the u8 wrap

/// A position in the log. Wraps; compared only within LOG of another (§ 1.8).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Seq(u8);

/// What the logic matches on.
pub enum Cmd {
    TogglePlay,
    Cue(SceneIdx),           // land slot n fresh at the boundary: a switch, CLEAR or RELOAD
    Keep(SceneIdx),          // play `live` as slot n: SCENE turned back (the old "cancel")
    Arm(ArmTarget),          // `Capture` without `Off`: a ring 0..5 or a segment 0..8
    Disarm,
    EditRhythm(RingMask),    // these rings' hits replaced in `live` (mask 1..=31)
    EditNotes(RingMask),     // these rings' notes and pool replaced
}

/// One byte on the wire: 1 TogglePlay, 2..=9 Cue, 10..=17 Keep, 18..=22 Arm ring,
/// 23..=30 Arm segment, 31 Disarm, 32..=62 EditRhythm, 63..=93 EditNotes.
/// Never 0, so `Option<Cmd8>` is one byte.
#[derive(Clone, Copy)] pub struct Cmd8(NonZeroU8);

pub struct CmdLog { head: Seq, ring: [u8; LOG] }   // 9 B; the slot of seq s is s & 7
```

An entry carries no seq. The slot of seq `s` is `s & 7`, and Lemma R (§ 1.8) shows that every slot the runner reads holds the seq it expects.

`OrbitShared` becomes:

| Field | Kind | Note |
|---|---|---|
| `bpm`, `sync` | level | unchanged |
| `settings: Scene` | level | `live`: the knobs, and the data an edit points at |
| `next: Scene` | level | the queued slot (else the playing one), what a `Cue` lands |
| `log: CmdLog` | events | 9 B |
| `load: Seq` | event marker | § 3 |
| `synced: bool` | level | the UI has landed its newest scene command (§ 2.3) |

`OrbitView` keeps its `u32` `epoch` (the load gate, § 1.8 (4)) and loses `scene_rev` and both `RingView` revs. It gains `acked: Seq` and `armed: Capture`, the runner's armed target. `Captured` loses `arming` and `epoch`, and `count` narrows to `u8`.

### 1.3 The commands

| Command | Issued by | Runner action | Fold |
|---|---|---|---|
| `TogglePlay` | B6 | Flip the transport as it was before the DAW's word (ADR 0080) | Parity |
| `Cue(n)` | SCENE to another slot; CLEAR of the playing slot; RELOAD | `cue = Slot(n)`: lands `next` at the boundary, at once when stopped | Last scene command wins |
| `Keep(n)` | SCENE back to the playing slot | `cue = None` if in sync (§ 2.3), else `cue = Live(n)`: lands `settings` at the boundary | Last scene command wins |
| `Arm(t)` | LEARN or CHRD PLAY up | Arm `t`, drop what was under way | Last arm command wins |
| `Disarm` | LEARN or PLAY down; SEGS shrinking under a segment | Disarm, drop what was under way | Last arm command wins |
| `EditRhythm(m)`, `EditNotes(m)` | A change to those rings' fill, or notes and pool, in `live`, harvested once a frame (§ 2.2) | If in sync: take those fields from `settings` | Union |

**Not in the log:**

- **The DAW's transport.** It arrives on the audio thread through the note queues (ADR 0072). It stays `ext_start` and `ext_play` (owner, Q6).
- **BPM, SYNC, the knobs and `synced`.** These are levels, and the UI writes every field of every publish (the `let OrbitShared { .. } = out` destructure stays).
- **Load.** See § 3.

### 1.4 The UI side

`OrbitUi` keeps its log copy, a 3 B outbox, and a few bits (§ 5 lists them byte by byte).

1. **Issue.** An act writes into the outbox, never into the log. The outbox has one `Option<Cmd8>` each for arm, rhythm edit and notes edit, a flip bit, and the scene command (`scene: Option<Cmd8>` with a `sent` bit). Two B6 presses in a frame cancel, a second SCENE turn replaces the first, and masks OR together.
2. **Flush,** at the end of the frame, before the publish. Each outbox slot moves into the log as `ring[(head+1) & 7]`, then `head += 1`, in a fixed order: scene, arm, rhythm, notes, flip. A slot moves only while `head − acked < LOG`, in wrapping u8, where `acked` is the newest accepted view's. What doesn't fit stays folded in the outbox until the next frame. That happens only when the runner has stalled for a frame.
3. **Publish.** All of `OrbitShared` is written, the 9 B log included, from the UI's copy.

### 1.5 The runner side

Each block, before the transport, the clock and the steps:

1. **Load** (§ 3). If `shared.epoch != self.epoch`, reset as today and set `applied = o.load`.
2. **Receive.** `n = o.log.head − applied` (wrapping u8). `n == 0` is the common case: one compare. Otherwise the runner decodes `ring[s & 7]` for each `s` in `applied+1 ..= head`, applying scene, arm and edit commands as they come and XOR-ing flips into one parity bit. Then `applied = head`. `n ≤ LOG` by Lemma R (`debug_assert`).
3. **In sync.** If `o.synced`, set `base_landed = true` (§ 2.3).
4. **The flip** is applied after the scene commands, so a cue that arrives with a start lands at the start. That is today's rule, where a start adopts `incoming()`. Then `advance` and `block` run as today, with `cue: Option<Incoming>` (`Slot(n)` → `&o.next`, `Live(n)` → `&o.settings`). An adoption sets `base_landed = false`.

`OrbitView` reports `acked = applied`, `cue` (as `queued` does today), and `armed = take.armed`.

### 1.6 The UI's view handling (`OrbitUi::sync`)

The UI's view handling runs once per frame, in one function and one order:

1. **Load gate.** Refuse the view unless `v.epoch` is the epoch of the UI's newest load. Both are the `u32` `AudioShared.epoch`, which never aliases (ADR 0046). `Channel::ack` also refuses, while `load_pending`, a view with `v.acked − L > LOG` (wrapping u8); the first view that passes clears `load_pending`. This is the only stale-view check (§ 3).
2. **Status.** Set `acked = v.acked` and `status = OrbitStatus::of(v)`.
3. **All acked** is `idle = (acked == head) && outbox empty`: the runner has applied every command the UI issued.
4. **Capture.** If `v.captured.count != took`: `Scene::take(c)`, `took = count`, and harvest the edit (§ 2.2). LEARN and PLAY show `v.armed` whenever `idle`, and the UI's own intent otherwise. A re-arm before the view (adversarial finding 2) keeps showing armed: the view reports the runner armed again. The capture before it is still taken, because the count moved.
5. **Landing.** If `scene` is sent, `idle`, and `v.cue.is_none()`, the runner has applied the newest scene command and adopted whatever it brings. For `Cue(n)`, `song.land(n)`. For `Keep`, nothing moves. Then `scene = None`, which publishes `synced = true`.
6. **Drift.** `absorb(v)` if `scene.is_none()`, `idle`, and `v.loops as u8 != loops_seen` (once a loop, owner, Q4).

### 1.7 What each level means

| Level | Writer | Reader's rule |
|---|---|---|
| `o.synced` | UI: `scene.is_none()` | The runner sets `base_landed = true` on it, and clears `base_landed` only by its own adoption |
| `v.cue` | runner | With all acked, `None` means the newest scene command has been fully adopted |
| `v.armed` | runner | With all acked, it is what the newest arm command left, after any capture since |
| `v.captured.count` | runner | A change is a new capture |

### 1.8 Proofs

**Lemma TB (triple buffer).** `triple.rs` keeps `{front, back, latest}` a permutation of `0..3`, swapping on `latest` with AcqRel. So:

- (a) A read returns a whole publish.
- (b) A read never returns `Pⱼ` after `Pₖ` with `k > j`.
- (c) A read may repeat a publish or skip publishes.

The writer's back slot is two publishes stale, which is harmless because every publish writes every field.

**Invariant W.** The UI issues `s` only if `s − acked_ui ≤ LOG` (wrapping). `acked_ui` is a value of the runner's `applied`, which only moves forward, so the true distance `applied − acked_ui` is never negative.

**Lemma R (the ring holds the gap).** When the runner reads `Pₖ` with gap `(A, Hₖ]`, every `s` in it is the newest seq `≤ Hₖ` in slot `s & 7`, and the gap has at most `LOG` seqs.

*Proof.*

- `Hₖ` was issued when `Hₖ − acked_ui ≤ LOG`, and `acked_ui` is at or before `A`, so `Hₖ − A ≤ LOG`.
- The `LOG` seqs `(Hₖ − LOG, Hₖ]` have distinct residues mod 8, so each sits in its own slot as the newest with that residue.
- `(A, Hₖ]` is inside them.
- The u8 wrap changes nothing: 8 divides 256, so residues stay continuous across `255 → 0`. The true distance is at most 8 < 256, so wrapping subtraction gives it exactly. ∎

**Exactly once.** The runner applies `(A, Hₖ]` and sets `A = Hₖ`. A later read has a head at or after `Hₖ` (TB b), and a repeated read has an empty gap (TB c). ∎

**None lost.** Every issued `s` reaches every later publish's `head`. `A` moves only to a head after applying the whole gap (Lemma R). Only seqs below a load's `L` are passed over (§ 3). ∎

**Ring wrap.** Slot `s & 7` is next written by `s + 8`, which W allows only once `acked_ui ≥ s`. ∎

**Coalescing.**

- (i) Per-frame folding happens before publish, and the runner sees only publishes.
- (ii) Each fold (parity, last-wins, union) is associative, and the runner applies a batch the same way. An outbox slot held back during a stall therefore lands on the same state the runner reaches by applying the unfolded commands in order, but later.
- (iii) Classes touch disjoint runner state (transport, cue, take, drift fields), except scene with flip, which the fixed order settles as today.

**No seq aliasing.** Every compare of two seqs is one of these:

- (1) the receive gap;
- (2) W's `head − acked`;
- (3) `idle`'s `acked == head`;
- (4) `Channel::ack`'s `v.acked − L`, the load gate's second check.

In (1)–(3) the true distance is at most `LOG`:

- (1) and (2) by W and Lemma R.
- (3) by W.
A u8 resolves distances below 128, so none of these aliases.

(4) is not bounded by `LOG`, and an earlier draft's claim that it was is withdrawn. `loaded()` sets `acked_ui = L` without the runner having seen anything, so with no view accepted, each load moves `L` up to `LOG + 1` further from the runner's last `applied`: up to 8 pushes against `acked = L`, then the reservation. After `k` back-to-back loads a pre-load view is `9k` behind, and the u8 check accepts it once `9k mod 256 ≥ 248`, first at `k = 28` (pinned by `back_to_back_loads_refuse_the_old_view_until_the_alias`). So the seq check cannot be the gate. **The gate is the view's `u32` `epoch`** (owner, 2026-10-03). A view's epoch is the load the runner was under when it wrote the view, and it equals the UI's only once the runner has read that load's snapshot. A `u32` epoch never aliases, so no pre-load view is accepted, however many loads pass without an audio block. Every post-load view then has `acked` in `[L, L + LOG]`, so the seq check passes it. No other seq is kept.

**The other narrowed counters:**

- **`Captured.count: u8`.** A capture needs an arming, and the runner disarms after each. So between two UI view reads it advances at most once per `Arm` applied in between, plus one: at most `LOG + 1` = 9. It can't advance by 256 between reads.
- **`loops_seen: u8`.** This is not a seq. It aliases only if 256 loops pass between two UI reads (over 200 s at the shortest loop, longer than any card job). Even then the cost is one absorb deferred a loop, never a wrong one.

**Landing is right without seqs.** Take the first view with `idle` and `cue == None` after the UI's newest scene command `X`:

- The runner has applied `X` (idle, TB b) and has no cue left.
- After applying `X`, the runner's cue is `X`'s outcome, which is `Slot(n)`, `Live(n)`, or `None` for a `Keep` in sync.
- A cue leaves only by adopting it. So `working` is now `X`'s scene, or, for an in-sync `Keep`, the scene the UI already has.
- An adoption from an older, superseded command would have happened before `X` was applied. After `X` is applied, nothing but `X`'s outcome can be adopted. So the UI never lands a superseded adoption (owner, Q1: intent wins).

**`synced` is never stale-true.** The runner sets `base_landed = true` only from a snapshot with `synced`. A snapshot has `synced` only if the UI had landed its newest scene command and the snapshot carries no newer one. By the landing argument, the runner had adopted that command's outcome and has no cue. With no cue, it cannot adopt again until a newer scene command, and the snapshot carrying that command has `synced = false`. A load sets both sides synced (§ 3). So `base_landed` is true only while `working`'s base is the UI's `live`.

## 2. Drift ownership

### 2.1 The choice: (b), the UI owns `live`

| | (a) The runner owns the working scene; every edit is a command | (b) The UI owns `live`; the runner sends drift |
|---|---|---|
| Who writes `live` | The runner, through the view; the UI mirrors it | The UI alone |
| A knob turn | A command per (ring, param) per frame: the log floods | A level in `settings`: no command |
| Lerped, instant UI | An optimistic copy reconciled with the view: two writers again | The UI draws `live` |
| Saves | Read a view a frame stale, or wait for the ack | Read `live`, as today |
| `OrbitView` | + a whole `Scene` (392 B, ×3 = +1,176 B AXI) | Shrinks (§ 5) |

**Chosen: (b).** Each datum has one writer. Drift goes from `working` to `live` only when the UI accepts it, and edits go from `live` to `working` only through edit commands. Drift comes back **whole**, not as deltas, because the triple buffer skips views (TB c): a delta in a skipped view would be lost, and a snapshot can't be.

### 2.2 Edits

- **Detection.** `SceneRing` loses `rhythm_rev` and `notes_rev` and gains `touched: u8` (two bits, run-time only, no part of `==`, never saved). Exactly the calls that bump a rev today set it: `refill` sets the rhythm bit; `renoted`, `set_chip`, `clear`, `scatter`, `take` and `transpose` set the notes bit. `SceneRing` is 68 B either way, because the byte sits in the revs' padding.
- **Harvest.** `OrbitUi::harvest` ORs the set bits into the outbox's two masks and clears them. It runs at the start of `sync`, after a capture's take, and before every publish. A `debug_assert` in `publish` checks that nothing was missed.
- **Gate.** Drift is absorbed only while `idle` (§ 1.6), which means every edit has been applied. The runner took the edited fields from a `settings` at least as new (TB b), so its drift sits on top of the edit. This replaces ten per-ring revs with one compare. It costs at most a frame of delay (owner, Q5).
- **Runner.** Each block in sync, the knob copy takes `settings`' knobs. It keeps `working`'s hits (via `with_mask`, so a fill changed with no edit still re-lays) and its notes and pool, except where this block's edit masks name the ring.
- **CLEAR down's roll** takes the UI's `head` as its salt in place of `notes_rev` (owner, Q3).
- **The engine's unread Swap and Nudge rev bumps** (`engine.rs:250-251`) go.

### 2.3 Scenes: the UI's intent wins

- `base_landed` is the runner's "in sync". It is set by `o.synced` and cleared by the runner's own adoption. It replaces `unseen`, `o.scene == self.playing`, `scene_rev` and `landed`.
- **Knobs and edits** reach `working` only while it is set. That is the rule `a_landing_takes_no_knobs_the_ui_has_not_landed` asks for.
- **`Keep(n)`** cues nothing in sync. Out of sync, the runner has adopted something the UI no longer wants, so it cues `Live(n)`.

| Case | Today | Here |
|---|---|---|
| `Cue(B)` waits; SCENE back to A before the boundary | Cancel | `Keep(A)` in sync: the cue is cleared and nothing lands |
| `Cue(B)` adopted unseen; SCENE back to A | B plays a loop, then A lands | Same: `Keep(A)` out of sync cues `Live(A)` |
| As above, then B again | B lands again at the next boundary | Same |
| CLEAR adopted unseen; SCENE away and back (adversarial finding 1) | Fixed in 4989eb9d by a seventh condition | Closed by construction: the CLEAR is superseded, never landed, and no drift is absorbed until synced |

**Slot writes stay safe.** `Cue(n)` lands `next`. The UI's landing copies slot n from its own song. They match because `save_to`, `overwrite_scene` and `clear_scene` are already refused while `queued.is_some()`, which holds from the `Cue` to its landing or a newer scene command.

## 3. Load as a command

- **UI.** `UiState::load_with` calls `OrbitUi::loaded()` *before* the load publish. It does four things:
  1. allocates `L = head + 1` and sets `head = L`, so no entry uses `L`;
  2. sets `load = L`, `load_pending = true` and `acked = L`;
  3. empties the outbox, sets `scene = None` and `took = 0`, and drops every `touched` bit, because the decode's writes set them and the runner adopts `live` whole;
  4. drops the capture and the SAVE choice, as today.

  The publish closure gets `&UiState` after `loaded()`. The shells' pre-load copy, `let (orbit, sync_mode) = (*ui.orbit(), ui.midi().sync)`, goes.
- **Runner.** Detection stays on `AudioShared.epoch`, a `u32` that never aliases. That is ADR 0046's contract, and it reaches the `Instrument` in the same snapshot. On a new epoch the runner keeps today's reset: it forgets pending offs, stops the transport, drops the DAW's pending word, forgets the take and adopts `settings` whole. It also sets `applied = o.load` and `base_landed = true`, and clears `done.target` to `Off` with `done.len = 0`. So a capture finished during the card read changes `count` but takes nothing (`Scene::take` on `Off` is a no-op). That replaces `Captured.epoch`.
- **Pre-load commands are void.** They have seqs below `L`, so `applied = o.load` passes them over. Post-load ones have seqs above `L` and are applied, whether or not the runner read the load snapshot itself. The load publish carries the new epoch and `load` together (TB a).
- **One stale-view check.** A pre-load view carries an older `epoch`, and the gate in § 1.6 step 1 refuses it. That one gate replaces `v.epoch == self.epoch` ×3 and `OrbitStatus.epoch`. `OrbitView.epoch` and the UI's `u32` load epoch stay (owner, 2026-10-03), because a seq compare alone aliases after 28 back-to-back loads (§ 1.8).
- **Test rigs** that fake a load by bumping `shared.epoch` alone call a `load()` helper that also sets `o.load`.

## 4. Inventory

### 4.1 Every field, counter and guard

| Where | Today | Fate |
|---|---|---|
| `OrbitShared.presses: u8` | B6 count | **Removed** → `TogglePlay` |
| `OrbitRunner.presses_seen: u8` | Its mirror | **Replaced** → `applied: Seq` (same byte) |
| `OrbitShared.scene_rev: u16` | Switch generation | **Removed** → `Cue`, `Keep` |
| `OrbitShared.scene: SceneIdx` | Playing slot | **Removed** → in `Keep(n)` |
| `OrbitShared.queued: Option<SceneIdx>` | Waiting switch | **Removed** → `Cue(n)` |
| `OrbitShared::incoming()` | Which scene a rev brings | **Removed** → `Incoming::{Slot, Live}` |
| `OrbitShared.capture: Capture` | Armed target | **Removed** → `Arm`, `Disarm` |
| `OrbitShared.arming: u8` | Arming generation | **Removed** |
| `OrbitShared.next`, `.settings`, `.bpm`, `.sync` | Levels | **Kept** |
| `Take.arming: u8` | Generation acted on | **Removed**: `Arm` restarts the take directly |
| `Captured.count: u16` | Capture identity | **Narrowed** → `u8` (§ 1.8) |
| `Captured.arming: u8` | Arming it was taken under | **Removed** → `v.armed` |
| `Captured.epoch: u32` | Load it was played on | **Removed** → `done.target = Off` at load |
| `OrbitView.epoch: u32` | Runner's load | **Kept**: the load gate (§ 1.8 (4), owner 2026-10-03) |
| `OrbitView.scene_rev: u16` | Rev adopted | **Removed** → `cue` with all acked |
| `RingView.rhythm_rev`, `.notes_rev` | Revs the drift was played under | **Removed** → edit gate |
| `OrbitRunner.revs`, `Revs`, `revs_of` | Revs last copied | **Removed** |
| `OrbitRunner.scene_rev: u16` | Switch adopted | **Replaced** → `base_landed: bool` |
| `OrbitRunner.epoch: u32` | Load | **Kept** (ADR 0046's tag, never aliases) |
| `OrbitRunner.cue: Option<SceneIdx>` | Waiting switch | **Retyped** → `Option<Incoming>` |
| `OrbitRunner.ext_start`, `.ext_play` | The DAW's word | **Kept** |
| local `unseen` | Knob-copy heuristic | **Removed** → `base_landed` |
| `copy_knobs`' rev branches | Who wins per ring | **Replaced** → the block's edit masks |
| `OrbitUi.presses: u8` | B6 count | **Removed** → outbox flip bit |
| `OrbitUi.arming: u8` | Generation | **Removed** |
| `OrbitUi.took: u16` | Count taken | **Narrowed** → `u8` |
| `OrbitUi.scene_rev: u16`, `.landed: u16` | Switch generation and landing | **Removed** → `scene: Option<Cmd8>` + `sent` bit |
| `OrbitUi.loops_seen: u32` | Once-a-loop cadence | **Narrowed** → `u8` |
| `OrbitUi.epoch: u32` | Load | **Kept**, for the load gate; `load: Seq` + `load_pending` bit are added |
| `OrbitUi::requeued`, `was` snapshots | Rev bump after CLEAR or RELOAD | **Removed**: they issue `Cue(current)` |
| `OrbitStatus.epoch`, `RingStatus.rhythm_rev` | Draw-side copies | **Removed**. The ORB and RHYTHM drawing asks `OrbitUi::fresh()` (`idle && scene.is_none()`) instead of comparing revs |
| `SceneRing.rhythm_rev`, `.notes_rev` | Edit revs, scatter seed | **Removed** → `touched: u8` |
| Engine Swap and Nudge rev bumps | Unread | **Removed** |
| `absorb`'s playing check and two rev tests | Drift gate | **Removed** → § 1.6 step 6 |
| `OrbitSong.slot_revs` | Confirmation witness | **Kept**: not a thread handshake |

**Count:**

- **23 fields removed.**
  - `OrbitShared` 6: `presses`, `scene_rev`, `scene`, `queued`, `capture`, `arming`.
  - `OrbitView` with `RingView` and `Captured` 5: `scene_rev`, two revs, `Captured.arming` and `Captured.epoch`.
  - `OrbitRunner` with `Take` 4: `revs`, `scene_rev`, `presses_seen`, `Take.arming`.
  - `OrbitUi` with `OrbitStatus` and `RingStatus` 6: `presses`, `arming`, `scene_rev`, `landed`, `OrbitStatus.epoch`, `RingStatus.rhythm_rev`.
  - `SceneRing` 2.
- **14 added.**
  - `OrbitShared` 3: `log`, `load`, `synced`.
  - `OrbitView` 2: `acked`, `armed`.
  - `OrbitRunner` 2: `applied`, `base_landed`.
  - `OrbitUi` 6: `log`, `outbox`, `flags` (flip, sent, `load_pending`), `load`, `scene`, `acked`.
  - `SceneRing` 1: `touched`.
- **Net −9.** (`OrbitView.epoch` and `OrbitUi.epoch` stay for the load gate, § 1.8 (4).) Three narrowed: `count`, `took`, `loops_seen`. One retyped: `cue`.
- **Removed functions and guards:** `incoming`, `revs_of`, `requeued`, `unseen`, three epoch tests, `copy_knobs`' rev branches, `Take::arm`'s generation logic, the parity trick, and four of `absorb`'s seven conditions.

### 4.2 Race tests that must still pass

Each keeps its assertion. "Mechanics" means it drives commands or the `load()` helper instead of poking a removed field.

- **`orbit_blocks_test`:**
  - unchanged: `one_press_is_one_start_through_the_triple_buffer`, `stop_play_keeps_the_drift`, `one_b6_starts_after_a_self_stop`
  - mechanics: `presses_between_runner_reads_all_count` (asserts on the outbox flip bit and the log), `b6_with_a_load_never_sticks` (`load()`), `a_view_from_before_a_load_brings_no_drift` (a view with `acked < L`)
- **`orbit_scenes_test`:**
  - mechanics: `a_stale_view_lands_nothing`, `a_stale_view_through_the_buffers_lands_nothing`, `the_cancel_race_plays_the_switch_one_loop`, `requeued_in_the_cancel_window_lands_on_the_next_loop`, `a_clear_adopted_then_cancelled_leaves_live_alone` (they waited on `scene_rev` and now wait on `cue` with all acked)
  - unchanged: `a_switch_turned_back_lands_nothing`, `turning_back_cancels_the_switch`, `scene_switch_lands_on_the_loop`, `edits_while_queued_play_in_the_playing_scene`, `save_while_queued_is_refused`, `save_captures_the_drift`
- **`orbit_capture_test`:**
  - mechanics: `runner_takes_one_capture_per_arming` (`Arm` commands), `a_load_drops_the_capture` (`acked` instead of `epoch`)
  - unchanged: `capture_survives_the_triple_buffer`, `a_load_disarms_the_runner`, `a_key_held_through_a_load_ends_nothing`, `a_capture_in_the_card_read_is_dropped`, `rearmed_before_the_view_takes_the_next`, `segs_shrinking_disarms_a_segment`
- **`orbit_runner_test`** (mechanics, via a test-support `OrbitShared::push`): `edits_keep_the_drift`, `knobs_copy_while_cued_and_the_cue_lands_whole`, `a_project_load_stops_orbit_cleanly`
- **`settings_orbit_test`** (unchanged): `a_landing_takes_no_knobs_the_ui_has_not_landed`, `clear_and_reload_wait_for_a_switch`, `reload_drops_the_drift`
- **`orbit_midi_clock_test`:**
  - unchanged: `b6_and_daw_stop_in_one_block_stop`, `b6_and_daw_start_in_one_block_toggle_the_transport_before`, `switching_sync_while_playing`, `b6_gates_orbit_under_an_external_clock`
  - mechanics: `a_load_drops_a_pending_daw_start` (`load()`)
- **`orbit_codec_test`** (unchanged): `project_save_keeps_the_drift`, `quick_save_while_queued_saves_the_landing`, `knobs_reach_the_runner_after_a_load`

**Non-race tests that assert a rev moved** change to assert a `touched` bit or an edit mask in the outbox. Their behavioural assertions stay. They are:

- `rhythm_edits_bump_the_rev_and_knobs_do_not`, renamed `rhythm_edits_touch_and_knobs_do_not`
- `octave_moves_the_notes`
- `capture_ninth_note_dropped`
- `step_ring_plays_its_steps`
- `clear_down_randomises_in_key`
- `clear_needs_mix`
- `loop_boundary_mutates_after_the_first_loop` (it keeps its mask and notes checks)

### 4.3 Property tests to add

All use the crate's xorshift `Rng` (as `property_test.rs` does), 4 seeds × 1M steps, and the real `TripleBuffer` driven from one thread. Each step is one of the following:

- a UI frame: 0–6 acts, harvest, flush, publish;
- a runner block;
- zero to two UI view reads;
- a load;
- a stall of k blocks or k frames.

Skips and repeats exercise TB(c). Every publish goes through `Writer::publish`, so the back slot is two publishes stale.

| Test | Drives | Asserts |
|---|---|---|
| `log_applies_every_command_once` | `channel` alone, with a reference list of issued commands that keeps full `u32` numbers outside the system | The receiver applies each post-load command once, in order, and no pre-load one after the load. Every gap is `≤ LOG` |
| `log_survives_the_u8_wrap` | The same, for > 10,000 wraps of the u8 seq | The same |
| `outbox_folds_like_the_runner` | `channel` with stalls up to 50 frames | The final parity, last scene and arm command, and the mask unions equal a reference fold of the unfolded acts |
| `loads_void_everything_before` | `UiState` + `OrbitRunner`, random acts and loads | After each load: no pre-load capture taken, no pre-load drift absorbed, the transport off, synced on both sides once the view arrives |
| `scenes_converge_after_any_interleaving` | MUTATE 0, random SCENE, CLEAR, RELOAD and B6, random frame and block ratios | After 2 quiet loops: `song.current() == view.playing`, `scene == None`, `base_landed`, and `live`'s rings equal `working`'s. While unsynced, `live` changes only by the UI's own acts |
| `captures_taken_once_after_any_interleaving` | Random LEARN and PLAY up and down, key chords | Every capture finished under a post-load `Arm` is taken once. LEARN and PLAY show armed iff the newest arm act was `Arm` and its capture is untaken |
| `edits_never_lost_to_drift` | MUTATE 100, random NOTE, STEPS and CLEAR edits | An edit's value is in `working` once acked. `absorb` never writes a field whose edit is unacked |

## 5. Cost

### 5.0 What the `UiState` cap protects (finding, and the ruling applied)

- **The guard** is `ui_state_fits_the_ui_reserve` in `chimera-core/tests/memory_budget_test.rs`: `size_of::<UiState>() − size_of::<Project>() ≤ 4 * 1024`. It came in at 2 KB and was raised to 4 KB in 7c87aab0 (SETTINGS, #300) for the project listing (`MAX_LISTED`). No ADR set it.
- **Where `UiState` lives.** On the unit it is one `static mut UI: MaybeUninit<UiState>` in AXI (`chimera-stm32/src/shared.rs`), built in place by `UiState::init_in_place`. It never passes through `main`'s frame. The stack is in DTCM (ADR 0025, which also notes `UiState` moved to AXI).
- **What the AXI budget charges for it.** `axi_residents_fit` charges a fixed `UI_RESERVE` of 64 KB (`hw.rs`) for everything in `UiState` besides its `Project`, whatever its real size. The 3,832 B in use is 6 % of that. So up to 64 KB of growth changes neither `AXI_RESIDENT` nor the 64 KB of AXI kept spare.
- **Copies.** `UiState` is neither `Clone` nor `Copy`. The firmware holds it as `&'static mut` and never moves it. The host builds it with `UiState::new()` (`by_value`) under `Box::new`, on a host thread stack of several MB. The one by-value copy in the shells is `*ui.orbit()` (the 144 B `OrbitUi`) in both `card_work` closures, which § 3 removes.

**Finding: the 4 KB cap protects nothing physical.** No stack, no copy and no memory region depends on it. It is a growth tripwire we chose. **Ruling applied (owner, 2026-10-03):** raise the cap to 5,120 B (commit 2 of § 6). This design still keeps `UiState` at 3,832 B (below), so the raise is headroom for later, not something this redesign spends.

These were measured at 4989eb9d (host, 64-bit, `memory_budget_test`, plus a scratch `size_of` probe). The "after" figures are byte arithmetic and are confirmed or refuted by the gates of § 6.

**`OrbitUi`, byte by byte.** All fields have alignment 1 except `status`, `anim` and the struct itself (4).

| Today | B | After | B |
|---|---|---|---|
| `seg`, `cursor`, `chip` | 3 | same | 3 |
| `presses` | 1 | — | |
| `capture` | 2 | `capture` | 2 |
| `arming` | 1 | — | |
| `took: u16` | 2 | `took: u8` | 1 |
| `scene_rev: u16` | 2 | `scene: Option<Cmd8>` | 1 |
| `save` | 2 | `save` | 2 |
| `landed: u16` | 2 | `flags` (flip, sent, `load_pending`) | 1 |
| `loops_seen: u32` | 4 | `loops_seen: u8` | 1 |
| `epoch: u32` | 4 | `load: Seq`, `acked: Seq` | 2 |
| | | `log: CmdLog` | 9 |
| | | `outbox`: arm, rhythm, notes | 3 |
| `status: OrbitStatus` | 88 | without `epoch` | 84 |
| `anim` | 32 | same | 32 |
| **Sum** | 143 → **144** | | 141 → **144** |

`UiState` without its `Project` stays at **3,832 B**, with 3 B of slack inside `OrbitUi`'s padding. The table predates the owner's 2026-10-03 ruling: `OrbitUi` keeps its `u32` load epoch, which adds 4 B, so the gate is **3,840 B**. If the measurement comes out over, these fallbacks apply in order:

1. `LOG` = 4 (−4 B).
2. `acked` dropped and read from the view at flush time (−1 B).

**AXI:**

| Resident | Today | After (est.) | Δ |
|---|---|---|---|
| `OrbitShared` | 796 B | ≈ 798 → 800 B | −9 (six fields), +11 (`log` 9, `load` 1, `synced` 1); `Scene` ×2 unchanged |
| `OrbitShared` ×3, in `AudioShared` ×3 | 2,388 B | ≤ 2,400 B | ≤ +12 B |
| `OrbitView` | 336 B | ≈ 308 B | `RingView` 56 → 52 (×5: −20); `Captured` 20 → 12 (count u8, no `arming`, no `epoch`: −8); −`epoch` 4, −`scene_rev` 2; +`acked` 1, +`armed` 2 |
| `OrbitView` ×3 | 1,012 B | ≈ 928 B | ≈ −84 B |
| `OrbitRunner` (`ENGINE` static) | 800 B | ≈ 780 B | `revs` −10, `scene_rev` → `base_landed` −1, `Take.arming` −1, `done` −8 |
| `Scene`, `SceneRing`, `OrbitSong` | 392, 68, 3,552 B | unchanged | `touched` sits in the revs' padding |
| **AXI net** | | | **≈ −90 B** (worst case with padding: −60 B) |

**Flash** (release, thumbv7em, `--features orbit`). This is estimated, not measured; the gate in § 6 is the authority.

- **Removed:** the generation and rev logic, about 100 lines. That is `Take::arm`'s generation branch, `copy_knobs`' rev branches, `revs_of`, `incoming`, the `unseen` and `scene_rev` cue logic, the rev copies in `view()` and `OrbitStatus::of`, three epoch tests, `take`'s and `see`'s and `sync`'s generation checks, `requeued` and `was`, `absorb`'s rev tests, the engine's bumps, and the `Captured` arming and epoch plumbing.
- **Added:** about 90 lines. That is the `Cmd8` codec (a table-free range decode), the outbox fold and flush, the receive loop, and the reshaped `sync` and landing.
- **Estimate:** at 10–16 B of Thumb-2 per line, −1.0 to −1.6 KB removed and +0.9 to +1.4 KB added, so net −0.7 to +0.5 KB. It trends down but **is not guaranteed** to shrink, hence the gate.
- **Levers if it comes out over:**
  - decode `Cmd8` by range compares inline in the receive loop instead of to `Cmd` and back;
  - merge `Cue` and `Keep` into one `Scene(n, keep)` arm;
  - drop the `debug_assert`s from release (they already are);
  - mark the cold load path `#[inline(never)]`.

**Audio-thread CPU per block** (64 samples, 1,333 µs, 480 MHz):

- **Common case:** one u8 compare (`head == applied`) and one bool read. Today a block makes four generation compares and ten rev compares, so this gets cheaper.
- **Worst case:** 8 one-byte decodes, about 100 cycles, 0.2 µs, 0.015 % of a block.
- **Removed:** `revs_of` on adoption and the rev copies in `view()`.

**UI per frame:** a flush of at most 5 bytes and a 9 B copy into the slot.

## 6. Migration plan, with the space gate

Every commit builds with `--features orbit` and `--no-default-features`, and passes `just check`, `just test` and `just clippy`. The tests for a mechanism change in the commit that changes it.

**The gate.** Before commit 2, record the baseline:

- `memory_budget_test`'s `UiState`, `AXI` and `OrbitView` lines;
- the release `--features orbit` firmware's `.text` + `.rodata` (`cargo size` or `arm-none-eabi-size` on the `just firmware` ELF).

After each of commits 2–5, record the same numbers in the commit message.

- **Hard limits, every commit:** `UiState` ≤ 3,840 B (owner, 2026-10-03) and AXI ≤ baseline. In commit 2, `memory_budget_test` raises `ui_state_fits_the_ui_reserve`'s cap from 4 KB to 5,120 B (the owner's ruling, § 5.0). It also gains an assertion against the baseline `OrbitView` and `OrbitShared` sizes.
- **Flash:** after commit 2 the cumulative delta must be ≤ 0. If it isn't, apply the levers in § 5 within commit 2. If it is still > 0, **stop and report to the owner** before commit 3. After commits 3–5 the cumulative delta must not rise above its commit-2 value.

**The commits:**

1. **`orbit/channel.rs`, pure.** `Seq`, `Cmd`, `Cmd8`, `ArmTarget`, `RingMask`, `CmdLog`, `Outbox`, flush and receive, with unit tests and the first three property tests. Nothing is wired yet. It is unused by the firmware, so it is linked out and the flash is unchanged (checked).
2. **Scenes and edits through the log.** These are the two classes that delete the most code, so the flash trend shows at the first wiring commit.
   - The log, `synced` and `acked` go in, with `base_landed`, `Cue`, `Keep`, the edit masks, `touched` and harvest.
   - The landing and the drift gate move to § 1.6.
   - Removed: `scene_rev` ×4, `landed`, `OrbitShared.scene` and `.queued`, `incoming`, `unseen`, `requeued` and `was`, both revs everywhere, `Revs`, `revs_of`, the engine's bumps, `absorb`'s playing and rev checks, and `RingStatus.rhythm_rev`.
   - Tests added: `scenes_converge_after_any_interleaving` and `edits_never_lost_to_drift`.
   - Record the gate.
3. **Transport and capture through the log.** `TogglePlay`, `Arm`, `Disarm` and `v.armed`. Remove `presses` ×2, `arming` ×4 and `Take`'s generation logic; narrow `count` and `took`. Add `captures_taken_once_after_any_interleaving`. Record the gate.
4. **Load.** `load: Seq`, the load gate, and `done.target = Off` at load. The publish closure gets `&UiState` after `loaded()` in both shells. Remove `epoch` from `Captured` and `OrbitStatus`; `OrbitView` and `OrbitUi` keep theirs for the load gate. Narrow `loops_seen`. Add `loads_void_everything_before`. Record the gate.
5. **Docs.**
   - Update the ORBIT design spec's § 4.1 and its Decisions row ("revs say who wins").
   - Set ADR 0082's status as the owner rules.
   - Mark 0071, 0075, 0077 and 0080 "superseded in part by 0082" in the README's status column. The ADR files themselves are not edited.
   - Record the final numbers.

The no-alloc and bounded-cost rules hold throughout. The receive loop is bounded by `LOG`, and every new type is `Copy`, has a fixed size, and is built in place.

## 7. Owner rulings (2026-10-03)

1. The cancel race keeps "the UI's intent wins". A superseded adoption plays a loop and is never landed.
2. The ring has 8 entries. `LOG` = 4 remains the first fallback if `UiState` measures over.
3. CLEAR down's roll salt moves from `notes_rev` to the UI's seq: the rolls change once.
4. Drift is absorbed once a loop (`loops_seen`, now `u8`).
5. Drift pauses for at most a frame while any command is unacked.
6. The DAW transport stays off the log.
7. **Space is the top priority.** AXI must not grow, and flash must shrink and be measured. Flash is estimated at −0.7 to +0.5 KB and is gated after the first wiring commit (§ 6).
8. **The `UiState` cap** guards no stack, copy or region (§ 5.0), so it rises to 5,120 B. This design holds `UiState` at or under 3,840 B: 3,832 plus the kept load epoch (owner, 2026-10-03).
