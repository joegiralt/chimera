# ORBIT Command Channel Implementation Plan

**Goal:** Replace ORBIT's seven UI↔runner handshakes (`presses`, `arming`, `scene_rev`, `landed`, `epoch`, the per-ring revs, `unseen`) with one sequenced one-byte command log, one ack, and three levels (`synced`, `cue`, `armed`), with no growth in `UiState` or AXI and no growth in flash.

**Architecture:** A pure `orbit/channel.rs` holds the wire types (`Seq`, `Cmd`, `Cmd8`, `CmdLog`), the UI side (`Channel`: outbox, flush, ack, load gate) and the runner side (`receive`, which folds a gap into a `Batch`). `OrbitUi` and `OrbitRunner` become thin shells over it. The UI owns `live`, and the runner owns `working`: edits travel as `EditRhythm`/`EditNotes` masks from `SceneRing.touched`, and drift comes back whole in the view, absorbed once a loop while every command is acked.

**Tech Stack:** Rust 2024, `no_std` `chimera-core`; the triple buffers of `chimera-core/src/triple.rs` (ADR 0021); the crate's xorshift `orbit::rng::Rng` for property tests; `cargo size` (cargo-binutils) for flash.

**Spec:** `docs/superpowers/specs/2026-10-03-orbit-command-channel-design.md` (binding, with its § 7 owner rulings). Decision record: `docs/adr/0082-orbit-command-channel.md` (Proposed). Read both before any task. The plan cites spec sections as "§ n"; the logic in § 1.5, § 1.6, § 2 and § 3 is not repeated here.

**Branch:** `orbit`, from `2dc9eefd`.

**Owner's answers to the spec's questions (2026-10-03):** the UI's intent wins; the ring holds `LOG` = 8 entries; the CLEAR-down re-seed is accepted; drift is absorbed once a loop; a pause of one frame is accepted; the DAW's transport stays off the channel.

**Deviation from the spec's migration order (a spec gap, see "Spec gaps" at the end):** the spec's commits 2 (scenes and edits) and 4 (load) are one task here (Task 2). The spec's byte table holds `OrbitUi` at 144 B only once both of its `u32` epochs are gone. Scenes and edits alone add 15 B and remove 4 B, which takes `UiState` to 3,844 B, over the 3,832 B gate. Task 2 is still the first wiring commit, so the flash gate stays where the spec puts it.

---

## Global Constraints

**Process**
- Commit messages are terse plain sentences. No type prefixes (`feat:`, `fix:`, `docs:` …).
- Stage by path, never `git add -A` or `git add .`. **Never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`.**
- If the firmware fails to link inside `.claude/worktrees/…`, build from a copy of the tree outside `.claude/worktrees` (`rsync -a --exclude target ./ <scratch>/chimera-build/`), and measure there.
- Every task ends green on **both** feature sets: `just check`. That covers core with and without `orbit`, desktop, the firmware default, `--features orbit` and `--no-default-features`, clippy on all of them, fmt and the stack check.
- No `unsafe` without `// SAFETY:`. No heap, no blocking and no unbounded loop on the audio thread. The runner's added work is at most `LOG` one-byte decodes a block. Every new type is `Copy`, fixed-size and built in place.
- Bugs and follow-ups go to GitHub issues (joegiralt/chimera), not to repo files.

**From the spec**
- `LOG = 8`, a power of two dividing 256. Seq `s` lives in `ring[s & 7]`. An entry carries no seq.
- `Cmd8` byte map: `1` TogglePlay, `2..=9` Cue, `10..=17` Keep, `18..=22` Arm ring, `23..=30` Arm segment, `31` Disarm, `32..=62` EditRhythm, `63..=93` EditNotes. It is never `0`.
- The UI issues seq `s` only while `s − acked ≤ LOG` (wrapping `u8`). The flush order is fixed: scene, arm, rhythm, notes, flip.
- The folds: flip by parity; scene and arm commands last-wins; edit masks by union. The runner applies the flip after the scene commands.
- `OrbitShared` fields are exactly `bpm`, `sync`, `settings`, `next`, `log`, `load`, `synced`. The `let OrbitShared { .. } = out` destructure in `publish` stays.
- The DAW's transport stays `ext_start` and `ext_play`, off the log (owner, Q6).
- CLEAR down's roll salt is the UI's `log.head()` in place of `notes_rev` (owner, Q3).
- Drift is absorbed once a loop (`loops_seen: u8`), only while `scene.is_none()` and idle (owner, Q4 and Q5).

**Hard gates. Each is a verification step in the task where it applies.**

1. **Flash.** Baseline at `2dc9eefd`, measured for this plan with `cargo size --release -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit`: **`text` = 773,724 B** (`.vector_table` 664 + `.text` 613,056 + `.rodata` 160,004; `.data` 3,092). After Task 2, `text` must be ≤ 773,724. If it isn't, apply the levers of § 5 inside Task 2. If it is still over, **the controller stops and reports to the owner. Task 3 does not start.** After Tasks 3 and 4, `text` must not rise above its Task 2 value.
2. **UiState.** `memory_budget_test`'s line `UiState without its Project = N B` must read N ≤ **3,832** after every task. The guard `ui_state_fits_the_ui_reserve` rises to `5 * 1024` in Task 2 (owner's ruling, § 5.0). The design still holds 3,832. If N comes out over, apply the § 5 fallbacks in order: `LOG = 4`, then drop `acked`.
3. **Behaviour.** Every audio golden and every screen golden stays bit-identical, and no golden table literal is edited. The goldens are `golden_test::{goldens_match, goldens_match_through_the_instrument}`, `fx_golden_test::fx_goldens_match`, `instrument_test::instrument_goldens_match`, `screen_golden_test::screen_goldens_match`, `theme_test::default_theme_leaves_every_golden_bit_identical`, `orbit_engine_test::init_scene_events_golden`, `orbit_pages_test::{ring_rhythm_golden, orb_page_golden}` and `disk_codes_test::table_matches_golden`. The one allowed change is the CLEAR-down roll (owner, Q3). No golden at `2dc9eefd` exercises `scatter`, so the expected change to golden tables is **none**. Any golden that moves must be traced to the salt, or the task stops. Every race test in `RACE` below passes. Its assertions on behaviour (what lands, plays, is taken, starts or stops) are unchanged. A line may be rewritten only where it reads or pokes a removed field, as § 4.2 says.

**`RACE`: the race tests (§ 4.2), all must pass after every task**
- `orbit_blocks_test`: `one_press_is_one_start_through_the_triple_buffer`, `stop_play_keeps_the_drift`, `one_b6_starts_after_a_self_stop`, `presses_between_runner_reads_all_count`, `b6_with_a_load_never_sticks`, `a_view_from_before_a_load_brings_no_drift`
- `orbit_scenes_test`: `a_stale_view_lands_nothing`, `a_stale_view_through_the_buffers_lands_nothing`, `the_cancel_race_plays_the_switch_one_loop`, `requeued_in_the_cancel_window_lands_on_the_next_loop`, `a_clear_adopted_then_cancelled_leaves_live_alone`, `a_switch_turned_back_lands_nothing`, `turning_back_cancels_the_switch`, `scene_switch_lands_on_the_loop`, `edits_while_queued_play_in_the_playing_scene`, `save_while_queued_is_refused`, `save_captures_the_drift`
- `orbit_capture_test`: `runner_takes_one_capture_per_arming`, `a_load_drops_the_capture`, `capture_survives_the_triple_buffer`, `a_load_disarms_the_runner`, `a_key_held_through_a_load_ends_nothing`, `a_capture_in_the_card_read_is_dropped`, `rearmed_before_the_view_takes_the_next`, `segs_shrinking_disarms_a_segment`
- `orbit_runner_test`: `edits_keep_the_drift`, `knobs_copy_while_cued_and_the_cue_lands_whole`, `a_project_load_stops_orbit_cleanly`
- `settings_orbit_test`: `a_landing_takes_no_knobs_the_ui_has_not_landed`, `clear_and_reload_wait_for_a_switch`, `reload_drops_the_drift`
- `orbit_midi_clock_test`: `b6_and_daw_stop_in_one_block_stop`, `b6_and_daw_start_in_one_block_toggle_the_transport_before`, `switching_sync_while_playing`, `b6_gates_orbit_under_an_external_clock`, `a_load_drops_a_pending_daw_start`
- `orbit_codec_test`: `project_save_keeps_the_drift`, `quick_save_while_queued_saves_the_landing`, `knobs_reach_the_runner_after_a_load`

**Gate commands, used by every task**
- `G-race`: `cargo test -p chimera-core --test orbit_blocks_test --test orbit_scenes_test --test orbit_capture_test --test orbit_runner_test --test settings_orbit_test --test orbit_midi_clock_test --test orbit_codec_test`. Expected: all pass, 0 failed.
- `G-gold`: `cargo test -p chimera-core --test golden_test --test fx_golden_test --test instrument_test --test screen_golden_test --test theme_test --test orbit_engine_test --test orbit_pages_test --test disk_codes_test`, then the same with `--no-default-features`. Then `git diff 2dc9eefd -- chimera-core/tests/golden_test.rs chimera-core/tests/fx_golden_test.rs chimera-core/tests/instrument_test.rs chimera-core/tests/screen_golden_test.rs chimera-core/tests/disk_codes_test.rs` must print no changed hex literal, and `orbit_engine_test.rs`'s and `orbit_pages_test.rs`'s golden tables must be unchanged.
- `G-mem`: `cargo test -p chimera-core --test memory_budget_test -- --nocapture --test-threads=1`. Read `UiState without its Project`, `AXI`, `OrbitView x3` and the `OrbitRunner = …, OrbitView = …, OrbitShared = …` line. Baseline at `2dc9eefd`: UiState 3,832; AXI 442,132; OrbitView 336 (×3 1,012); OrbitRunner 832; OrbitShared 876. AXI must be ≤ 442,132.
- `G-flash`: `cargo size --release -p chimera-stm32 --target thumbv7em-none-eabihf --features orbit`. Read `text`.

## Review Focus

Five inputs the spec implies but its tests don't pin. Each has a test in the task that owns the code.

1. **A switch waiting on the loop.** RHYTHM and ORB keep drawing the runner's drift for the whole wait, as today. They must not fall back to the scene's bare hits for up to a loop. → Task 2, `rhythm_draws_the_drift_while_a_switch_waits`.
2. **CLEAR down turned once a frame while ORBIT runs.** Each turn rolls new dice. The salt is `log.head()`, so this holds only while the runner acks. → Task 2, `clear_down_rolls_new_dice_through_the_runner`.
3. **SCENE spun across several slots while the runner is stalled for more than `LOG` frames.** Only the last slot lands, and no slot in between ever plays. → Task 2, `a_scene_spin_during_a_stall_lands_only_the_last`.
4. **A DAW Start while a switch waits.** ORBIT starts in the waiting scene, as `adopt(incoming())` does today. → Task 2, `a_daw_start_lands_the_waiting_switch`.
5. **B6 pressed several times while the runner is stalled past `LOG` frames.** The transport ends by the parity of all the presses once the runner catches up. No press is lost to the window. → Task 3, `b6_presses_during_a_stall_toggle_by_parity`.

---

### Task 1: `orbit/channel.rs`, the pure core (spec commit 1)

**Files:**
- Create: `chimera-core/src/orbit/channel.rs`
- Modify: `chimera-core/src/orbit/mod.rs` (add `pub mod channel;`)
- Test: `chimera-core/tests/orbit_channel_test.rs` (new, `#![cfg(feature = "orbit")]`), plus unit tests in `channel.rs`

**Interfaces:**
- Consumes: `orbit::scene::SceneIdx`, `orbit::RingIdx`, `orbit::runner::Capture`, `triple::TripleBuffer`, `orbit::rng::Rng`.
- Produces (all `Copy`, `Debug`; `Seq`, `RingMask`, `Cmd` and `ArmTarget` also `PartialEq, Eq`):
  - `pub const LOG: usize = 8;`
  - `pub struct Seq(u8)`: `Seq::ZERO`, `const fn next(self) -> Seq`, `const fn since(self, older: Seq) -> u8` (wrapping distance), `const fn get(self) -> u8`.
  - `pub struct RingMask(u8)` (bits 0..5): `EMPTY`, `fn with(self, RingIdx) -> RingMask`, `fn contains(self, RingIdx) -> bool`, `fn union(self, RingMask) -> RingMask`, `fn is_empty(self) -> bool`.
  - `pub enum ArmTarget { Ring(RingIdx), Segment(u8) }`. `impl From<ArmTarget> for Capture`. `fn ArmTarget::of(c: Capture) -> Option<ArmTarget>`.
  - `pub enum SceneCmd { Cue(SceneIdx), Keep(SceneIdx) }`
  - `pub enum Cmd { TogglePlay, Cue(SceneIdx), Keep(SceneIdx), Arm(ArmTarget), Disarm, EditRhythm(RingMask), EditNotes(RingMask) }`
  - `pub struct Cmd8(NonZeroU8)`: `impl From<Cmd> for Cmd8` (`debug_assert!` on a non-empty mask), `fn cmd(self) -> Cmd`, `fn from_byte(u8) -> Option<Cmd8>` (`None` for 0 and 94..=255).
  - `pub struct CmdLog { head: Seq, ring: [u8; LOG] }` (9 B): `const EMPTY`, `fn head(&self) -> Seq`, `fn push(&mut self, c: Cmd8) -> Seq`, `fn reserve(&mut self) -> Seq` (takes `head + 1` and writes no entry: the load's `L`).
  - `pub struct Batch { pub flip: bool, pub scene: Option<SceneCmd>, pub arm: Option<Capture>, pub rhythm: RingMask, pub notes: RingMask }` (`Disarm` folds to `Some(Capture::Off)`).
  - `pub fn receive(log: &CmdLog, applied: &mut Seq) -> Batch`: folds `(applied, head]` in order, sets `*applied = head`, and `debug_assert!(head.since(*applied) as usize <= LOG)`.
  - `pub struct Outbox { arm: Option<Cmd8>, rhythm: Option<Cmd8>, notes: Option<Cmd8> }` (3 B).
  - `pub struct Channel { log: CmdLog, outbox: Outbox, scene: Option<Cmd8>, flags: u8, load: Seq, acked: Seq }` (16 B, align 1). `flags` holds `FLIP`, `SENT` and `LOAD_PENDING`. Methods: `const fn new() -> Channel`, `fn flip(&mut self)`, `fn scene(&mut self, c: SceneCmd)` (replaces; clears `SENT`), `fn arm(&mut self, to: Capture)`, `fn edit(&mut self, rhythm: RingMask, notes: RingMask)` (unions), `fn flush(&mut self)`, `fn ack(&mut self, acked: Seq) -> bool` (`false` = the load gate refused the view), `fn idle(&self) -> bool`, `fn awaiting(&self) -> Option<SceneCmd>` (sent and not yet landed), `fn landed(&mut self)` (`scene = None`), `fn synced(&self) -> bool` (`scene.is_none()`), `fn loaded(&mut self) -> Seq`, `fn log(&self) -> &CmdLog`, `fn load(&self) -> Seq`, `fn acked(&self) -> Seq`.

- [ ] **Step 1: Write the codec and window unit tests in `channel.rs`**

```rust
#[test] fn every_cmd_round_trips_through_one_byte() // all 1+8+8+5+8+1+31+31 = 93 commands: Cmd8::from(c).cmd() == c, bytes are exactly 1..=93
#[test] fn zero_and_past_93_are_not_commands()     // from_byte(0) and from_byte(94..=255) are None
#[test] fn option_cmd8_is_one_byte()                // size_of::<Option<Cmd8>>() == 1, size_of::<CmdLog>() == 9, size_of::<Outbox>() == 3, size_of::<Channel>() == 16
#[test] fn a_full_window_holds_back_the_rest()      // 8 edits flushed with acked fixed: head.since(acked) == 8; a 9th stays in the outbox; ack(head) then flush moves it
#[test] fn two_flips_in_a_frame_cancel()            // flip(); flip(); flush(): head unchanged, idle()
#[test] fn a_frame_flushes_in_the_fixed_order()     // scene, arm, rhythm, notes, flip issued in reverse; receive() sees them as seqs h+1..=h+5 in that order
#[test] fn load_gate_refuses_views_before_l()       // L = loaded(); ack(L-1) is false, and so is ack(L - 1 - LOG as u8); ack(L) is true and clears LOAD_PENDING; afterwards ack(anything) is true
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p chimera-core --lib orbit::channel`
Expected: compile error, `channel` not found.

- [ ] **Step 3: Implement the types and `receive` in `channel.rs`**

Decode `Cmd8` with range compares against the byte map in the Global Constraints. No table: a table costs flash, which § 5 counts. `receive` walks `s = applied+1 ..= head`, reads `ring[s & 7]`, and folds as the spec's coalescing rule says (§ 1.8). An undecodable byte is a `debug_assert!` and is skipped in release.

- [ ] **Step 4: Implement `Channel`**

`flush` moves scene (when not `SENT`, then sets `SENT`), arm, rhythm, notes and flip into the log while `head.next().since(acked) as usize <= LOG`. `idle` is `acked == head && outbox empty && !FLIP && (scene is None || SENT)`. `loaded` follows § 3 steps 1–3: `L = log.reserve()`, `load = acked = L`, set `LOAD_PENDING`, empty the outbox, `scene = None`, clear `FLIP`.

- [ ] **Step 5: Run the unit tests**

Run: `cargo test -p chimera-core --lib orbit::channel`
Expected: PASS.

- [ ] **Step 6: Write the three channel property tests in `tests/orbit_channel_test.rs`**

The harness: 4 seeds (`0x0B17_0001`, `0xC0FF_EE00`, `0xDEAD_BEEF`, `1`) × `STEPS`. `STEPS` is `1_000_000`, or `20_000` under `cfg!(debug_assertions)` if one seed takes over 30 s in debug. The UI's `Channel` publishes `(CmdLog, Seq /*load*/)` through a leaked `TripleBuffer` (`Writer::publish`, so the back slot is two publishes stale). The runner side `receive`s from `Reader::read` and publishes `applied` back through a second `TripleBuffer<Seq>`, which the UI `ack`s. Each step is one of: a UI frame (0–6 random acts, `flush`, publish); a runner read; 0–2 UI ack reads; a `loaded()`; or a stall of 1–50 frames or blocks. The reference keeps every issued command with a `u32` number and the load boundaries.

```rust
#[test] fn log_applies_every_command_once()  // each post-load command is applied exactly once, in issue order; no pre-load command is applied after its load; every receive gap ≤ LOG
#[test] fn log_survives_the_u8_wrap()        // the same harness, with assert!(issued > 10_000 * 256) at the end
#[test] fn outbox_folds_like_the_runner()     // stalls up to 50 frames: the runner's final parity, last scene cmd, last arm cmd and mask unions == a reference fold of the unfolded acts
```

- [ ] **Step 7: Run them**

Run: `cargo test -p chimera-core --release --test orbit_channel_test`, then `cargo test -p chimera-core --test orbit_channel_test`
Expected: PASS, 3 tests.

- [ ] **Step 8: Gates**

Run `just check`. Expected: green. Nothing is wired yet, so with `--no-default-features` the module compiles out (it sits under `orbit`).
Run `G-flash`. Expected: `text` = 773,724 (the module is linked out).
Run `G-mem`. Expected: the baseline numbers, unchanged.

- [ ] **Step 9: Commit**

```bash
git add chimera-core/src/orbit/channel.rs chimera-core/src/orbit/mod.rs chimera-core/tests/orbit_channel_test.rs
git commit -m "ORBIT's command channel as a pure module: the one-byte log, the outbox and their property tests"
```

---

### Task 2: Scenes, edits and loads through the log (spec commits 2 and 4; the first wiring commit; flash gate)

**Files:**
- Modify: `chimera-core/src/orbit/runner.rs`: `OrbitShared`, `OrbitView`, `RingView`, `Captured.epoch`, `OrbitRunner`, `adopt`, `copy_knobs`, `advance`, `view`. Remove `incoming`, `Revs` and `revs_of`.
- Modify: `chimera-core/src/orbit/scene.rs`: `SceneRing` (`touched` in place of the revs), `refill`/`renoted`, `Scene::scatter`, `Scene::harvest`, `OrbitSong::absorb`.
- Modify: `chimera-core/src/orbit/engine.rs:250-251`. Delete the Swap and Nudge rev bumps; `apply(m)` stays.
- Modify: `chimera-core/src/ui/orbit/mod.rs`: `OrbitUi` fields, `publish`, `queue`, `sync`, `loaded`. Remove `see`, `requeueing` and `RingStatus.rhythm_rev`.
- Modify: `chimera-core/src/ui/orbit/cells.rs` (CLEAR's salt), `chimera-core/src/ui/orbit/draw.rs:55-64` (`hits` asks `fresh()`).
- Modify: `chimera-core/src/ui/mod.rs`: `handle_input` flushes at its end; `load_with`/`load_project` call `loaded()` before `publish`, and the closure gets `&UiState`; the CLEAR and RELOAD answers.
- Modify: `chimera-core/src/ui/settings/job.rs:96-101` (`card_work`'s closure type).
- Modify: `chimera-desktop/src/main.rs:165-171`, `chimera-desktop/src/qa.rs:66`, `chimera-stm32/src/orbit.rs` (remove `Snap`), `chimera-stm32/src/main.rs:451-460`.
- Modify tests: `common/orbit.rs` (`load`), `screen/mod.rs`, the files in `RACE` (mechanics only), `orbit_engine_test.rs`, `orbit_pages_test.rs`, `orbit_qa_test.rs`, `orbit_nav_test.rs`, `settings_manage_test.rs`, `settings_project_test.rs`, `project_boot_test.rs` (closure arg `p` becomes `u.project()`), `memory_budget_test.rs`.

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces:
  - `OrbitShared { bpm, sync, settings: Scene, next: Scene, log: CmdLog, load: Seq, synced: bool, presses, capture, arming }`. The last three stay until Task 3. `Default` has `log: CmdLog::EMPTY, load: Seq::ZERO, synced: true`.
  - `pub fn OrbitShared::push(&mut self, c: Cmd) -> Seq`. Always compiled: `bench.rs` needs it in Task 3.
  - `#[cfg(any(test, feature = "test-support"))] pub fn fake_load(shared: &mut AudioShared)` in `runner.rs`. It bumps `shared.epoch` and sets `shared.orbit.load = shared.orbit.log.reserve()`. Rigs that faked a load by bumping the epoch alone call it.
  - `OrbitView`: drops `epoch`, `scene_rev` and `RingView.{rhythm_rev, notes_rev}`. It gains `acked: Seq`. `queued` is renamed `cue: Option<SceneIdx>`.
  - `enum Incoming { Slot(SceneIdx), Live(SceneIdx) }` (private to `runner.rs`). `OrbitRunner` gains `applied: Seq` and `base_landed: bool`, and drops `revs` and `scene_rev`. Its `cue` becomes `Option<Incoming>`.
  - `pub struct Touched(u8)` with `RHYTHM`, `NOTES` and `fn contains`. `SceneRing.touched: Touched` is excluded from `==` and never saved. `pub fn Scene::harvest(&mut self) -> (RingMask, RingMask)` (rhythm, notes) clears the bits. `pub fn Scene::untouched(&self) -> bool`.
  - `pub fn Scene::scatter(&mut self, r: RingIdx, salt: Seq)`. The seed is `self.seed.wrapping_add(u32::from(salt.get()) << 16)`.
  - `pub fn OrbitSong::absorb(&mut self, v: &OrbitView)`, unconditional: hits via `with_mask` and notes and pool for every ring. The gate moves to `OrbitUi::sync`.
  - `OrbitUi` fields: `seg, cursor, chip, presses, capture, arming, took: u8, save, loops_seen: u8, chan: Channel, status, anim`. `presses` and `arming` stay until Task 3. `took` narrows now to keep 144 B; the count it compares with stays `u16` until Task 3 and is compared as `c.count as u8`.
  - `pub fn OrbitUi::flush(&mut self, song: &mut OrbitSong)` harvests, then `chan.flush()`. `pub fn OrbitUi::issue_scene(&mut self, song: &OrbitSong)` issues `Cue(q)` when `song.queued() == Some(q)`, else `Keep(song.current())`. `pub fn OrbitUi::queue(&mut self, s, song)` is `song.queue(s)` then `issue_scene`. `pub fn OrbitUi::loaded(&mut self, song: &mut OrbitSong)` takes no epoch. `pub fn OrbitUi::fresh(&self) -> bool` is `chan.idle()` (Review Focus 1, see Spec gaps). `pub fn OrbitUi::log(&self) -> &CmdLog`. `pub fn OrbitUi::publish(&self, song, sync_mode, out)` also writes `log`, `load` and `synced`, and `debug_assert!(song.live.untouched())`.
  - `pub fn UiState::flush_orbit(&mut self)` (the tail of `handle_input`, and the rigs' hook). `load_project` and `card_work` take `publish: impl FnOnce(Swap, &UiState) -> R`.
  - `OrbitStatus` drops `epoch`; `RingStatus` drops `rhythm_rev`.

- [ ] **Step 1: Write the new tests (they fail to compile until Step 4)**

In `orbit_scenes_test.rs`:
```rust
#[test] fn scenes_converge_after_any_interleaving()   // § 4.3: ShellRig, MUTATE 0, random SCENE/CLEAR/RELOAD/B6 and frame:block ratios, 4 seeds × STEPS; after 2 quiet loops: song.current() == view.playing, ui.orbit() awaiting() == None, runner in sync, live.rings == working's (via view rings); while awaiting, live changes only by the UI's own acts
#[test] fn rhythm_draws_the_drift_while_a_switch_waits() // Review Focus 1: playing with MUTATE 100 until drift; SCENE → 2 mid-loop; for every frame until the boundary OrbitFrame::hits(r) == view.rings[r].hits
#[test] fn a_scene_spin_during_a_stall_lands_only_the_last() // Review Focus 3: playing; the runner stalls 20 frames while SCENE turns 1→2→3→4→3; it resumes: no block's view has playing ∉ {S[0], S[2]}, and after the boundary song.current() == S[2]
#[test] fn a_daw_start_lands_the_waiting_switch()      // Review Focus 4: stopped under SYNC EXT, SCENE → 2 published, a MIDI Start in the same block: the first running view has playing == S[1], and working == slot 2
```
In `orbit_runner_test.rs` (or `orbit_scenes_test.rs`), with ShellRig:
```rust
#[test] fn edits_never_lost_to_drift()   // § 4.3: MUTATE 100, random NOTE/STEPS/CLEAR edits; once ui.orbit().log().head() is acked, every edited field is in working (view rings); absorb never overwrites a ring whose edit is unacked (live keeps the edit across every frame)
#[test] fn loads_void_everything_before() // § 4.3: random acts and fake loads through ShellRig + card_work; after each load: no pre-load capture is taken into live, no pre-load drift absorbed, view.running == false, and once a post-load view arrives ui synced and the runner's base_landed (via a landed knob reaching working)
```
In `orbit_pages_test.rs`:
```rust
#[test] fn clear_down_rolls_new_dice_through_the_runner() // Review Focus 2: ShellRig playing; 10 frames, each one CLEAR down on ring 2: 10 distinct pools
```

- [ ] **Step 2: Rewrite the mechanics of the existing tests this task touches**

Change only lines that read or poke a removed field, as § 4.2 says. Assertions on behaviour stay as they are.
- `orbit_scenes_test`: `a_stale_view_lands_nothing`, `a_stale_view_through_the_buffers_lands_nothing`, `the_cancel_race_plays_the_switch_one_loop`, `requeued_in_the_cancel_window_lands_on_the_next_loop`, `a_clear_adopted_then_cancelled_leaves_live_alone`. Waits and checks on `scene_rev` become waits on `ui.orbit()`'s `awaiting() == None`, or on `view.cue == None` with `view.acked == ui.orbit().log().head()`.
- `orbit_blocks_test`: `b6_with_a_load_never_sticks` and `a_view_from_before_a_load_brings_no_drift` use `fake_load`. A "view from before" is one with `acked < L`.
- `orbit_capture_test::a_load_drops_the_capture`: `acked` in place of `epoch`.
- `orbit_runner_test`: `edits_keep_the_drift`, `knobs_copy_while_cued_and_the_cue_lands_whole` and `a_project_load_stops_orbit_cleanly` drive `shared.orbit.push(Cmd::Cue(..)/Cmd::EditNotes(..))` and `fake_load`.
- `orbit_midi_clock_test::a_load_drops_a_pending_daw_start`: `fake_load`.
- The non-race rev tests move from "a rev moved" to the outbox or the log. Each one reads `receive(ui.orbit().log(), &mut before)` and asserts its `rhythm` or `notes` mask contains the ring:
  - `rhythm_edits_bump_the_rev_and_knobs_do_not`, renamed `rhythm_edits_touch_and_knobs_do_not`: a knob leaves `log().head()` unchanged;
  - `octave_moves_the_notes`;
  - `capture_ninth_note_dropped`;
  - `step_ring_plays_its_steps`;
  - `clear_down_randomises_in_key` ("a new notes_rev" becomes "an EditNotes for the ring"; "new dice each turn" stays);
  - `clear_needs_mix`;
  - `loop_boundary_mutates_after_the_first_loop` (its mask and notes checks stay; the rev checks go).
- `orbit_qa_test`: `song.live.scatter(r)` becomes `scatter(r, ui.log().head())`. The rig's publish step calls `ui.flush(&mut song)` first.
- Every `card_work` and `load_project` closure: `|swap, p|` becomes `|swap, u|`, with `p` becoming `u.project()`.

- [ ] **Step 3: Run the new tests to see them fail**

Run: `cargo test -p chimera-core --test orbit_scenes_test --test orbit_pages_test --test orbit_runner_test`
Expected: compile errors naming `Seq`, `fake_load`, `push`, `awaiting`, `log` and `fresh`.

- [ ] **Step 4: The scene side: `scene.rs` and `engine.rs`**

`touched` is set at exactly the call sites that bump a rev today (§ 2.2): `refill` sets `RHYTHM`; `renoted` (`set_chip`, `clear`, `scatter`, `take`, `transpose`) sets `NOTES`. Remove `rhythm_rev` and `notes_rev` everywhere, and the engine's two bumps. Change `absorb` and `scatter` as the Interfaces block says.

- [ ] **Step 5: The runner: `runner.rs`**

Each block follows § 1.5:
- **Load.** On `shared.epoch != self.epoch`, keep today's reset and add `applied = o.load`, `base_landed = true`, `take.done.target = Capture::Off`, `take.done.len = 0`.
- **Receive.** `let b = receive(&o.log, &mut self.applied)`. Then `Cue(n)` sets `cue = Some(Slot(n))`. `Keep(n)` sets `cue = None` if `base_landed`, else `Some(Live(n))`.
- **In sync.** `if o.synced { base_landed = true }`.
- **Transport.** The transport and start logic run as today. A start adopts `cue`'s scene if any, else `settings`. Every adoption sets `playing`, clears `cue` and sets `base_landed = false`.
- **Knob copy.** The knob copy runs only while `base_landed`. It takes `settings`' knobs, and keeps `working`'s hits (via `with_mask`) and its notes and pool, except for rings in `b.rhythm` / `b.notes`.
- **View.** `view()` reports `acked = applied`, `cue` (the `Incoming`'s scene) and `playing`.

- [ ] **Step 6: The UI: `ui/orbit/mod.rs`, `cells.rs`, `draw.rs`, `ui/mod.rs`**

- **`sync(v, project)`** runs § 1.6 in order:
  1. `if !chan.ack(v.acked) { return }`;
  2. `status = OrbitStatus::of(v)`;
  3. `idle = chan.idle()`;
  4. the capture take: for now it keeps the `arming` check, drops the epoch check, compares `took` as `u8`, then harvests;
  5. the landing: `if let Some(c) = chan.awaiting() && idle && v.cue.is_none()`. For `Cue(n)`, `song.land(n)` and `save = None`. Then `chan.landed()`;
  6. drift: `if chan.synced() && idle && v.loops as u8 != loops_seen`, `absorb(v)` and `loops_seen = v.loops as u8`.
- **CLEAR and RELOAD.** The answers run `act(song)?`, then `if song.queued().is_some() { ui.orbit.issue_scene(song) }`.
- **`handle_input`.** Its body moves to a private `input()`. `handle_input` calls `input()`, then `flush_orbit()`.
- **The load.** `load_with` calls `self.orbit.loaded(self.project.orbit_mut())` before `publish(swap, self)`.

- [ ] **Step 7: The shells**

- **Desktop** (`main.rs`): drop the `(orbit, sync_mode)` copy. The closure publishes with `u.publish_orbit(o)` and `u.project().perf()`.
- **stm32** (`orbit.rs`, `main.rs`): delete `Snap`. The closure uses `orbit::publish(u, b)`.
- **`qa.rs`**: same as the desktop.

- [ ] **Step 8: `memory_budget_test.rs`**

`ui_state_fits_the_ui_reserve` asserts `rest <= 5 * 1024`. Its comment cites the owner's 2026-10-03 ruling and § 5.0. `axi_residents_fit` adds `assert!(total <= 442_132, "AXI grew past 2dc9eefd's")`. `orbit_runner_size` adds `assert!(size_of::<OrbitView>() <= 336)`.

- [ ] **Step 9: Run the task's tests**

Run: `cargo test -p chimera-core --release --test orbit_scenes_test --test orbit_runner_test --test orbit_pages_test`, then `just test`.
Expected: PASS, the new tests included.

- [ ] **Step 10: Behaviour gate**

Run `G-race`. Expected: every `RACE` test passes. Then `git diff 2dc9eefd -- chimera-core/tests/orbit_*_test.rs chimera-core/tests/settings_orbit_test.rs | grep '^-.*assert'`. Every removed `assert` line must read a removed field (`scene_rev`, `epoch`, a rev), with its replacement next to it.
Run `G-gold`. Expected: all pass, no golden literal changed.

- [ ] **Step 11: UiState and AXI gate**

Run `G-mem`. Expected: `UiState without its Project` ≤ 3,832, AXI ≤ 442,132 and OrbitView ≤ 336. If UiState is over, apply `LOG = 4` (`Cmd8` is unchanged; `CmdLog` becomes 5 B and `Channel` 12 B; update `option_cmd8_is_one_byte`), re-run, then drop `acked`, as § 5 says.

- [ ] **Step 12: FLASH GATE (hard stop)**

Run `G-flash`. Expected: `text` ≤ **773,724**.
If it is over, apply the § 5 levers inside this task, in order, re-running `G-flash` after each:
1. decode `Cmd8` inline by range in `receive`, not through `Cmd`;
2. merge `Cue` and `Keep` into one decode arm;
3. `#[inline(never)]` on the runner's load branch and on `OrbitUi::loaded`.

If `text` is still over 773,724, **stop. Do not commit. Report the number, the delta and the levers tried to the owner, and do not start Task 3.**

- [ ] **Step 13: `just check`**

Run: `just check`. Expected: green, `--no-default-features` included.

- [ ] **Step 14: Commit**

```bash
git add chimera-core/src/orbit/runner.rs chimera-core/src/orbit/scene.rs chimera-core/src/orbit/engine.rs \
  chimera-core/src/ui/orbit/mod.rs chimera-core/src/ui/orbit/cells.rs chimera-core/src/ui/orbit/draw.rs \
  chimera-core/src/ui/mod.rs chimera-core/src/ui/settings/job.rs \
  chimera-desktop/src/main.rs chimera-desktop/src/qa.rs chimera-stm32/src/orbit.rs chimera-stm32/src/main.rs \
  chimera-core/tests/<each test file changed, by path>
git commit -m "Scenes, edits and loads ride ORBIT's command log

UiState <N> B, AXI <N> B, OrbitView <N> B, OrbitShared <N> B, flash text <N> B (2dc9eefd: 3832, 442132, 336, 876, 773724)."
```

---

### Task 3: Transport and capture through the log (spec commit 3)

**Files:**
- Modify: `chimera-core/src/orbit/runner.rs`: drop `OrbitShared.{presses, capture, arming}`, `OrbitRunner.presses_seen`, `Take.arming` and `Captured.arming`; `Captured.count` becomes `u8`; `OrbitView` gains `armed`.
- Modify: `chimera-core/src/ui/orbit/mod.rs`, `chimera-core/src/ui/orbit/cells.rs:190-196`, `chimera-core/src/ui/mod.rs:1632,1679`.
- Modify: `chimera-stm32/src/bench.rs:867`, `chimera-desktop/src/audio.rs:510`.
- Modify tests: `orbit_blocks_test.rs`, `orbit_capture_test.rs`, `orbit_nav_test.rs`, `orbit_codec_test.rs`, `orbit_runner_test.rs`, `settings_orbit_test.rs`, `comp_test.rs`, `screen/mod.rs`, `common/orbit.rs`, `memory_budget_test.rs`.

**Interfaces:**
- Consumes: `Channel::{flip, arm}`, `Batch.{flip, arm}`, `OrbitShared::push` and `fake_load` (Task 2).
- Produces:
  - `OrbitShared { bpm, sync, settings, next, log, load, synced }`: § 1.2's final shape.
  - `OrbitView.armed: Capture`. `Captured { count: u8, target, notes, len }`.
  - `Take::set(&mut self, to: Capture)`. Arming anything restarts the take; `Off` disarms and restarts.
  - `OrbitUi` loses `presses` and `arming`. `took: u8` is compared directly. `pub fn OrbitUi::arm(&mut self, to: Capture)` sets `capture` and calls `chan.arm(to)`. `pub fn OrbitUi::disarm(&mut self)` does the same with `Off`. `play_stop` calls `chan.flip()`. `fit` disarms through `disarm()`. `OrbitUi` ends at § 5's 141 B of fields, 144 with padding.

- [ ] **Step 1: Write the new tests**

```rust
// orbit_capture_test.rs
#[test] fn captures_taken_once_after_any_interleaving() // § 4.3: ShellRig, random LEARN/PLAY up and down and key chords, 4 seeds × STEPS: every capture finished under a post-load Arm is taken into live once; after each frame with chan idle, ui capture == view.armed; LEARN/PLAY show armed iff the newest arm act was Arm and its capture is untaken
// orbit_blocks_test.rs
#[test] fn b6_presses_during_a_stall_toggle_by_parity() // Review Focus 5: stopped; the runner stalls for 12 frames, B6 pressed in 11 of them (some twice in a frame); it resumes: running == (total presses odd) once acked, and stays so
```

- [ ] **Step 2: Rewrite the mechanics** (§ 4.2)

- `presses_between_runner_reads_all_count` asserts on the outbox flip bit and the log. Two presses in a frame leave `log().head()` unchanged; three leave one `TogglePlay`. Its transport assertions stay.
- `runner_takes_one_capture_per_arming` drives `shared.orbit.push(Cmd::Arm(..))` in place of bumping `arming`.
- `bench.rs` and the desktop `audio.rs` test replace `presses = 1` with `orbit.push(Cmd::TogglePlay)`.
- `comp_test`, `screen/mod.rs` and `common/orbit.rs` replace their `presses`/`arming` pokes with `play_stop()`/`arm()` or `push`.

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p chimera-core --test orbit_capture_test --test orbit_blocks_test`
Expected: compile errors on `armed` and the removed fields.

- [ ] **Step 4: The runner**

`block` stops calling `take.arm`. In `advance`, the batch's `arm` goes to `take.set` after receive. `pressed` becomes `b.flip`, applied after the scene commands as today. `Take::finish` drops its `epoch` argument. `view()` sets `armed = take.armed`.

- [ ] **Step 5: The UI**

- **`sync` step 4.** `if v.captured.count != took`: `took = count`, `Scene::take`, harvest. When `chan.idle()`, `capture = v.armed`.
- **`cells.rs`'s `arm()`.** Down calls `ui.disarm()` when `ui.capture == to`.
- **`loaded`.** It still drops `capture` with no command, because the load voids it.

- [ ] **Step 6: `memory_budget_test.rs`**

`orbit_runner_size` adds `assert!(size_of::<OrbitShared>() <= 880)` (§ 5: ×3 at most +12).

- [ ] **Step 7: Run the task's tests**

Run: `cargo test -p chimera-core --release --test orbit_capture_test --test orbit_blocks_test`, then `just test`.
Expected: PASS.

- [ ] **Step 8: Gates**

- `G-race`: all pass. Apply the same `assert`-diff check as Task 2 Step 10.
- `G-gold`: all pass, no literal changed.
- `G-mem`: UiState ≤ 3,832; AXI ≤ 442,132 and ≤ Task 2's; OrbitShared ≤ 880.
- `G-flash`: `text` ≤ Task 2's recorded value. If it isn't, apply the § 5 levers here; if it is still over, stop and report.
- `just check`: green.

- [ ] **Step 9: Confirm the removals**

Run: `git grep -nE '\b(presses|presses_seen|arming|scene_rev|incoming|unseen|requeue|landed: u16|rhythm_rev|notes_rev|Revs|revs_of)\b' -- chimera-core/src chimera-desktop/src chimera-stm32/src`
Expected: no hits, except `hold::Presses` and the button gates (`self.gates`), which are not ORBIT's. Also `git grep -n 'epoch' -- chimera-core/src/orbit chimera-core/src/ui/orbit` must show only `OrbitRunner.epoch` (§ 4.1: kept).

- [ ] **Step 10: Commit**

```bash
git add chimera-core/src/orbit/runner.rs chimera-core/src/ui/orbit/mod.rs chimera-core/src/ui/orbit/cells.rs \
  chimera-core/src/ui/mod.rs chimera-stm32/src/bench.rs chimera-desktop/src/audio.rs \
  chimera-core/tests/<each test file changed, by path>
git commit -m "B6 and capture arming ride ORBIT's command log

UiState <N> B, AXI <N> B, OrbitView <N> B, OrbitShared <N> B, flash text <N> B (2dc9eefd: 3832, 442132, 336, 876, 773724)."
```

---

### Task 4: Docs (spec commit 5)

**Files:**
- Modify: `docs/superpowers/specs/2026-09-30-orbit-design.md` (§ 4.1 "Settings flow (UI → audio)", line 285; the Decisions row at line 615, "revs say who wins").
- Modify: `docs/adr/README.md` (status column of rows 0071, 0075, 0077 and 0080; row 0082 when the owner rules).
- Modify: `docs/adr/0082-orbit-command-channel.md`: **the Status line only**, and only once the owner rules. It is Proposed, so the edit is allowed.

**Interfaces:**
- Consumes: the final numbers from Task 3's commit message.
- Produces: nothing code depends on.

- [ ] **Step 1: The ORBIT design spec**

Rewrite § 4.1 to the log, the ack, `synced`, `cue` and `armed`. Cite ADR 0082 and the channel spec, and drop the revs. The Decisions row becomes "The engine owns the drift; the UI owns the knobs; edits travel as commands, drift comes back whole (ADR 0082)".

- [ ] **Step 2: The ADR index**

Rows 0071, 0075 and 0077 append "; superseded in part by [0082](0082-orbit-command-channel.md)". Row 0080 appends "; amended by [0082](0082-orbit-command-channel.md)". The ADR files 0071, 0075, 0077 and 0080 are not edited.

- [ ] **Step 3: ADR 0082's status**

Ask the owner for the ruling: Accepted or still Proposed. Set the `Status:` line and the README row to that ruling. Change nothing else in the ADR.

- [ ] **Step 4: The final numbers**

Re-run `G-mem` and `G-flash`. The commit message carries them with the baseline.

- [ ] **Step 5: `just check`**

Expected: green. The docs don't touch code; this confirms the tree.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-09-30-orbit-design.md docs/adr/README.md docs/adr/0082-orbit-command-channel.md
git commit -m "ORBIT's design and the ADR index follow the command channel

UiState <N> B, AXI <N> B, flash text <N> B (2dc9eefd: 3832, 442132, 773724)."
```

---

## Spec gaps found while planning

1. **The migration order breaks the UiState gate.** The spec's commit 2 alone gives `OrbitUi` +15 B (`log` 9, outbox 3, `scene`, `flags`, `acked`) and −4 B (`scene_rev`, `landed`). That is 143 → 154 → 156 B, and `UiState` 3,844 B > 3,832. Only removing both `u32` epochs (spec commit 4) pays for it. Resolution: Task 2 = spec commits 2 + 4.
2. **The spec's size figures are stale.** At `2dc9eefd`, `OrbitShared` is 876 B (spec 796), `OrbitRunner` 832 (800), `Scene` 432 (392), `SceneRing` 76 (68) and `OrbitSong` 3,912 (3,552). `OrbitView` (336), `OrbitUi` (144) and `UiState` (3,832) match. The gates here use the measured numbers.
3. **`fresh()` as specified (`idle && scene.is_none()`) would stop RHYTHM and ORB drawing drift for up to a loop while a switch waits.** That is a visible regression from today. This plan defines `fresh()` as `chan.idle()` and pins it with Review Focus 1. The owner should confirm.
4. **The CLEAR salt (`log.head()`) stops advancing while the log is full.** With no runner, or a runner stalled for 8 or more commands, repeated CLEAR-down turns roll the same dice. The owner accepted the salt; Review Focus 2 pins the running case only.
5. **The spec doesn't say where harvest and flush run against the shells' `&self` publish.** This plan flushes at the end of `UiState::handle_input`, through `flush_orbit()`, and `publish` `debug_assert`s that nothing is untouched.
6. **The spec says "an assertion against the baseline `OrbitShared` size", but `OrbitShared` grows by design** (§ 5: ×3 at most +12). This plan asserts `≤ 880` after Task 3 and AXI ≤ 442,132 throughout.
