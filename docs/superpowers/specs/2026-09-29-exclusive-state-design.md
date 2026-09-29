# Exclusive State: Design

**Date:** 2026-09-29
**Status:** Draft, revised 2026-09-29 after the owner's sympathetic-pool decision; awaiting owner review
**Builds on:** main at 937b89f (8 voices, ADR 0040 strings to G1, ADR 0042 voice pitch).
**Supersedes:** ADR 0008 ("Engines are persistent; never constructed in the audio interrupt"), through ADR 0051.
**Revision (2026-09-29).** § 4 was "Sixteen-bit strings" (plan Task 5, ADR 0052). The owner replaced it: strings stay f32, and Sympathetic's seven sympathetic lines move to a shared pool of four slots, as in Rings. ADR 0052 is superseded, and Task 5's commits (a32923e..cf3a23f) are undone, except for three pieces that are bit-identical in f32 (§ 4.8).

## Roadmap

Sub-project 1 of Modal 2.

1. **Exclusive state (this spec).** A voice holds only the engine it plays, Modal holds only the model it plays, and Sympathetic's sympathetic strings live in a shared pool of four. The sound does not change.
2. **Modal 2.** Elements-style exciters feed Rings-style resonators, and the #191 fixes land. It gets its own spec, and it spends the memory this one frees.

## Intent

Today every voice carries every engine and every Modal model, whether it plays them or not. A Voice is 35,248 B, and 32,736 B of that is Modal. The pool is 281,984 B against a 286,720 B budget, so Modal 2's exciters have nowhere to go.

This spec makes each voice hold one engine and each Modal voice hold one model. Sympathetic, the one model eight strings wide, keeps its main string in the voice and borrows its seven sympathetic strings from a pool of four slots. So voices are sized for Bowed, the largest of the other models. About 126 KB of D2 stays free. Nothing audible changes: every string stays f32.

**Done when:**

- `Voice` holds an `EngineSlot` (Algo or Modal), and `ModalEngine` holds a `ModelSlot` (bank, string, bowed or sympathetic). Neither holds state for anything it isn't playing.
- A Sympathetic voice holds a `Lease` on one of four `SympatheticSet`s in the `Instrument`'s `SymPool`, while its note sounds, and every test in § Tests passes.
- Every golden is bit-identical to main's. The ones Task 5 re-recorded go back to their earlier values.
- The chip's sizes, the bench's MODAL rows and the stack mark are measured and committed.

## Principles (binding)

- **Types decide what's possible** (ADR 0012). A slot's variant *is* the engine or model. No separate field records which one is active, so there is nothing to disagree with it. Render dispatches by `match` on the slot, so the Algo arm only ever sees an `AlgoEngine`. A Sympathetic model can't be built without a `Lease`, and a `Lease` can't be copied.
- **Functional core, imperative shell.** Pure and host-tested: `SlotKind::of`, `SlotKind::resting` and the pool allocator `SymAlloc`. The voice's switch logic and the Instrument's placement are thin shells over them.
- **`unsafe` stays in `in_place.rs`,** in its existing pattern, and every block has a `// SAFETY:` comment.
- **The audio thread never allocates or blocks,** and nothing large goes on its stack.

## Measured today (host, 64-bit)

At main 937b89f:

| Type | Bytes | Made of |
|---|---|---|
| `Voice` | 35,248 | `Engines` 33,416 + chain 1,832 |
| `Engines` | 33,416 | `ModalEngine` 32,736 + `AlgoEngine` 676 + sample rate |
| `ModalEngine` | 32,736 | 8 × `KsString` 3,960 (31,680) + 48 `Svf` 960 + 96 of shared state |
| `KsString` | 3,960 | `[f32; 984]` 3,936 + 24 |
| `[Voice; 8]` | 281,984 | budget `VOICE_RAM_BUDGET` 286,720; 4,736 left |

The eight strings are one main string and seven sympathetic strings (`modal/mod.rs:49-64`, `string.rs:12,38`). Sympathetic uses all eight. String and Bowed use the main one. The bank uses the SVFs.

At cf3a23f, the branch with Tasks 1–4 done, with the store set to f32:

| Type | Bytes |
|---|---|
| `KsString` | 3,960 |
| `ModalBank` | 1,008 |
| `BowedString` | 3,968 |
| `SympatheticStrings` (main, 7 strings, `ratios`, `pending`) | 31,736 |
| `ModelSlot` | 31,744 |
| `ModalEngine` | 31,768 |
| `EngineSlot` | 31,776 |
| `Voice` (chain 1,832) | 33,608 |
| `Instrument` less its voices (allocator, buses, sends, pans) | 2,656 |

The 33,584 B owner figure is from before c3194bc, which added Sympathetic's `pending` (7 × 4 B, plus padding). This spec uses the table above.

**What follows from this.** Exclusivity alone saves little: while Sympathetic holds eight strings, it is the largest model by eight times, so the voice is sized for it. Taking its seven sympathetic lines out of the voice is what frees the memory. Exclusivity is what keeps Modal 2 cheap: a new model or exciter adds to the *largest* slot, not to the sum.

## 1. The engine slot

```rust
#[repr(C, u8)]
pub enum EngineSlot { Algo(AlgoEngine), Modal(ModalEngine) }

pub enum SlotKind { Algo, Modal(ResonatorMode) }
impl SlotKind {
    pub fn of(p: &ParamSnapshot) -> Self;   // pure: engine, then MODE
    pub fn resting(self) -> Self;           // pure: § 4.5
}
impl EngineSlot {
    pub fn kind(&self) -> SlotKind;
    pub fn rebuild(&mut self, kind: SlotKind, pool: &mut SymAlloc, voice: VoiceIdx) -> Rebuilt; // in place, § 4.4
}
```

- `EngineSlot` replaces `Engines` in `dsp/engines.rs`. `Voice.active_engine` goes, because `slot.kind()` is the active engine. `sample_rate` moves into `Voice`.
- `note_on`, `note_off`, `render`, `set_pitch` and `is_active` each become one `match` on the slot, as they are on `Engines` today. `note_on`, `note_off` and `render` also take `&mut SymPool`, which only the Sympathetic arm reads.
- `Engines::cost` becomes `EngineSlot::cost`, unchanged: it reads only the params.

**Why ADR 0008's risk is handled now.** ADR 0008 rejected an enum because building a variant and moving it into place put 66 KB on the interrupt stack, with no guard. The repo has since grown in-place construction: `in_place.rs` (`uninit_at`, `by_value`, `field_list!`), and every engine has an `init_in_place` that writes its fields through a pointer. A slot is rebuilt where it lives. Nothing the size of an engine is ever a stack value.

**How.** A `#[repr(C, u8)]` enum has a defined layout: a `repr(C)` struct of a `u8` tag and a `repr(C)` union of the variants' fields. `in_place.rs` gains one macro, `in_place_enum!`, which declares the enum with that repr, a private mirror of the layout, and for each variant `V(P)`:

- `unsafe fn rebuild_v(&mut self, init: impl FnOnce(&mut MaybeUninit<P>) -> &mut P)`: writes the tag, then runs `init` on the payload's address;
- `unsafe fn init_v(slot: &mut MaybeUninit<Self>, init: …) -> &mut Self`, for `Voice::init_in_place`.

They carry `by_value`'s `# Safety` contract (`init` writes every field; the in-place constructors do, and `field_list!` guards them). The macro also asserts at compile time that no payload has drop glue, as `engines.rs` does today. Callers in `engines.rs` and `modal/mod.rs` use one `// SAFETY:` line each. § 4.4 adds one helper, `move_out`, for the lease. There is no other new `unsafe`.

## 2. The model slot

```rust
#[repr(C, u8)]
enum ModelSlot {
    Bank(ModalBank),                   // 48 Svf, CosineOsc, resolution, burst
    String(KsString),                  // the plucked KS+ string
    Bowed(BowedString),                // the bowed string and its bow force
    Sympathetic(SympatheticVoice),     // the main string + a Lease on a pool slot (§ 4)
}
```

One variant per `ResonatorMode`. The owner's three kinds of state (bank, single string, sympathetic strings) are the payloads. Bowed's string carries its bow force (`BowedString`), so String and Bowed share a string, not a payload type.

| `ResonatorMode` | Variant | State | Host bytes: in the voice / in the pool |
|---|---|---|---|
| Modal | `Bank` | 48 SVFs, cosine osc, resolution, noise burst | 1,008 / — |
| String | `String` | 1 string | 3,960 / — |
| Bowed | `Bowed` | 1 string, bow force | 3,968 / — |
| Sympathetic | `Sympathetic` | main string, lease / 7 strings, 7 ratios, 7 pending | 3,968 / 27,776 |

- `ModalEngine` keeps only what every model uses: frequency, pitch, tuned, released, active and the silence counter. `active_mode` goes: the variant is the mode. `sym_ratios` moves into the pool's `SympatheticSet`. The exciter state is its model's: the bank's noise burst (remaining, amp, noise, lowpass) lives in `ModalBank` and Bowed's bow force in `BowedString`, so a rebuild starts them fresh, and a MODE change mid-burst can't leave a burst no model counts down (the voice would never go idle). The bank reports its burst to the silence check.
- `ModalEngine::init_in_place(slot, model: Model)` builds the shared fields and the one model. `Model` is `Bank | String | Bowed | Sympathetic(Lease)`: a Sympathetic build takes the lease by value (§ 4.3). `playing()` reads the variant.
- Each `render_*` takes its own payload: `render_sympathetic(&mut KsString, &mut SympatheticSet, …)`, the main string from the voice and the set from the pool. No model can render another's state.
- **Sound is unchanged.** Each model's render and note-on code runs exactly as today, on the same state, just held in a variant or a pool slot. Today a MODE edit keeps the old model's state around, but nothing ever reads it: every note-on writes or clears what its model reads. The only thing that can differ is a noise seed carried over from an earlier note of another model, because a rebuild starts the seeds fresh. No golden covers that case.
- Bowed stays exactly as it is. It becomes an exciter in Modal 2.

## 3. Switching

**Rule (owner, 2026-09-29).** A switch is a change to the Sound's `SlotKind`: its engine or its Modal MODE. When a Part switches while its voices sound, those voices fade and are rebuilt once they are silent. The new engine or model plays from the next note. Knob moves are not switches.

The machinery already exists. `Voice::render` fades a voice whose Sound changed engine, over `Voice::FADE` = 128 samples (2.7 ms), and `fade_ended` → `reset` already rebuilds the engine in place. This spec widens the check from the engine to the `SlotKind`, and the reset rebuilds into the Sound's kind:

| Where | Today | After |
|---|---|---|
| `render`, sounding voice | `params.engine() != active_engine` → fade (restart if held) | `slot.kind() != SlotKind::of(params)` → the same fade |
| `fade_ended` / `reset` | rebuild the old engine | a note follows: `SlotKind::of(params)`; none: `SlotKind::of(params).resting()` (§ 4.5). The one rebuild |
| `trigger`, idle voice | reset the engine left behind | rebuild in place if the kind differs, then play: same block |
| `note_on` on a sounding voice of the old kind | waits for the fade | unchanged |

- **Only that Part.** The check compares each voice with the Sound of the Part it sounds for. Other Parts' voices never see the change. The FX bus is not touched, so its tails ring on.
- **Knob moves never rebuild.** `SlotKind::of` reads exactly two fields, the engine and MODE. Every other param reaches ringing notes every block, as today, and as Rings does: patch params reach every voice each block, and only pitch is latched per voice.
- **What changes for the player.** Today a MODE edit leaves ringing notes on their old model until they end. After this, they fade in 2.7 ms, as an engine change does. `Voice::held_model_extra` (#183) stays: it bills the old model for the ≤ 2 blocks of the fade.
- **Held keys.** A held key restarts its note on the new kind after the fade, as an engine change does today (`AfterFade::Restart`), now for MODE too. A switch onto Sympathetic restarts the last four played through the `Instrument` instead (§ 4.5).
- **A rebuild count.** `Voice` keeps `rebuilds: u16` (wrapping) with a read-only getter, for the tests and the bench.

**Cost of a rebuild.** A rebuild writes the new slot, at most Bowed's 3,968 B. A Sympathetic rebuild writes its main string and its lease, also 3,968 B: its seven lines are the pool's and are not rebuilt (§ 4.4). That is about 1,000 word stores, estimated at 1,000–2,000 cycles on D2 with the D-cache. A block is 640,000 cycles at 480 MHz, so one rebuild is ≤ 0.3 % of a block. ADR 0051 proves at most three rebuilds per voice per block (§ 4.6 keeps the bound), so the worst voice-block is ≤ 6,000 cycles, and all eight voices ≤ 48,000, 7.5 %. **This is not new cost.** Today every fade end already rebuilds the whole 32,736 B `ModalEngine` in the interrupt. The rebuild is not billed in `Cost`, which is per sample, as today's reset isn't. The bench gets a REBUILD row.

## 4. The sympathetic slot pool

### 4.1 Why

Sympathetic is one main string that sets seven more ringing. In f32 that is 8 × 3,960 B, eight times any other model, and every voice paid for it. Task 5 halved every string to 16 bits instead. It worked, but Sympathetic's high notes missed the −90 dBFS gate (−83.7 dBFS at C6, ADR 0052), and every Q16 read and write cost instructions on the hot path.

**Owner decision (2026-09-29).** Keep Sympathetic at full f32 precision, but not in every voice. The model is Rings: it caps polyphony at 4 (`kMaxPolyphony = 4`) and shares a fixed set of `kNumStrings = 8` strings, `kNumStrings / polyphony` to a voice (`rings/dsp/part.h`, `part.cc`; MIT, Emilie Gillet; the idea only, no code). Here:

1. Voices are sized for the largest other model (Bowed, 3,968 B host).
2. A pool of four slots, each a full f32 sympathetic set. A voice playing Sympathetic borrows a slot for as long as its note sounds, and gives it back when the voice goes idle or rebuilds.
3. Sympathetic plays at most four notes. A fifth steals the oldest slot: that voice fades out over `Voice::FADE`, then the new note takes the slot. Other models keep eight voices. Knob moves never rebuild.

**Owner ruling (2026-09-29): the Rings rule.** In Rings (`part.cc`), a new strike always takes the next voice round-robin, so the oldest yields and a note is never refused. Here that means two things. A new Sympathetic note always sounds: when every slot is held, including by a Mono Part, the oldest slot's voice fades and the new note takes the slot. And when more than four held notes switch to Sympathetic, the last four played restart; the rest fade and stay silent until they are played again.
4. Strings go back to f32, and ADR 0052 is superseded.
5. The per-Part VOICES control is https://github.com/joegiralt/chimera/issues/207 (MODE MONO·POLY·UNISON·PARA and VOICES 1·2·4·6·8, with Sympathetic topping out at 4). It is not built here. This spec provides the cap it will show: `SYM_SLOTS = 4`.

Rings divides its strings statically by its polyphony setting. Chimera has eight voices and five other models that don't need the strings, so it lends them dynamically instead.

### 4.2 What a slot holds, and where it lives

**A slot is the seven sympathetic lines, and the main string stays in the voice.**

```rust
struct SympatheticSet {                      // one pool slot: 27,776 B host
    strings: [KsString; NUM_SYMPATHETIC],    // 7 × 3,960
    ratios: [f32; NUM_SYMPATHETIC],          // set at note-on
    pending: [f32; NUM_SYMPATHETIC],         // the fused injection's held outputs (§ 4.8)
}
struct SympatheticVoice { main: KsString, lease: Lease }   // 3,968 B host
```

Why the main string stays in the voice:

- **It is free there.** The voice is already sized for Bowed, a `KsString` and an `f32`: 3,968 B. The main string plus a one-byte lease packs into the same 3,968 B, so `ModelSlot` doesn't grow by a byte.
- **Moving it would cost 15,840 B.** That is 4 × 3,960 in the pool, for nothing saved in the voices.
- **The main string is String's string.** It is excited, tuned and damped like String's. Only the seven lines are what makes the model eight strings wide.

A const assert pins the first point: `size_of::<SympatheticVoice>() <= size_of::<BowedString>().max(size_of::<KsString>())`. Sympathetic can never set the voice's size.

**Where: D2, inside the `Instrument`.**

- `Instrument` gains `sym: SymPool`, beside `voices`. `SymPool { alloc: SymAlloc, sets: [SympatheticSet; SYM_SLOTS] }`.
- The `Instrument` is already one `.ram_d2.voices` static, and `size_of::<Instrument>() <= VOICE_RAM_BUDGET` is already asserted (`instrument.rs:120`). So the linker proves the pool fits, beside the DMA reserve, with no new section.
- **AXI can't take it.** AXI has about 88.9 KB free (the storage ADRs), and `axi_residents_fit` keeps 64 KB of that spare, which leaves about 24.9 KB. The pool is 111,104 B.
- D2 is also where the voices that read it live, so the render's memory traffic is unchanged.

### 4.3 Types

```rust
// chimera-core/src/sym_alloc.rs: pure, no DSP
pub const SYM_SLOTS: usize = 4;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SymSlot(u8);                    // private field: always < SYM_SLOTS
#[must_use]
pub struct Lease(SymSlot);                 // private field; not Clone, not Copy
pub enum Place { On(VoiceIdx), Steal(VoiceIdx), Refused }
pub enum Restart { Claimed { evict: Option<VoiceIdx> }, Silent }
pub struct SymAlloc { slots: [State; SYM_SLOTS] }
enum State {
    Free,
    Promised { voice: VoiceIdx, age: u32 },           // for a note that hasn't started
    Lent { voice: VoiceIdx, age: u32, then: Then },   // a Lease is out
}
enum Then { Free, To(VoiceIdx) }           // where the slot goes when the lease comes back
// `age` is the note's `Allocator` age (its clock at note-on): lower is older.

// chimera-core/src/voice_alloc.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VoiceIdx(u8);                   // private field: always < MAX_VOICES; VoiceIdx::ALL
```

**What the types make impossible:**

- **Two voices holding one slot.** A slot's state names one voice or none. Only `SymAlloc::lend` makes a `Lease`, and only for a slot that isn't `Lent`, so at most one `Lease` per slot exists at a time. `Lease` has no `Clone`, `Copy` or public constructor, so a `Lease` can move but never be duplicated. To hold a slot is to hold its `Lease`.
- **A Sympathetic voice without a slot.** `SympatheticVoice` has a `lease: Lease` field, not an `Option`. `Model::Sympathetic(Lease)` is the only way to build one, so a voice can't play Sympathetic without a slot.
- **A set reference that outlives its lease or aliases another.** A voice reaches its set through `SymPool::set(&mut self, lease: &Lease) -> &mut SympatheticSet`. The reference borrows both the pool (mutably) and the lease. So no two sets are reachable at once, and `SymAlloc::give_back(&mut self, lease: Lease)` can't be called while one is. After `give_back`, the moved lease can't name the slot again (use after move, E0382).

**What tests check instead,** because Rust can't express it:

- **A voice holding two slots.** `place` and `restart` never promise a voice that already holds or is promised a slot, and `lend` takes the voice's own promise first. `sym_alloc_never_gives_a_voice_two_slots`, a seeded random walk, checks this.
- **A dropped `Lease` leaks its slot.** Rust has no linear types. A leak is safe: the slot stays `Lent` and nothing else can hold it. The only home of a `Lease` is a `SympatheticVoice`, and the only way out is `EngineSlot::rebuild`, which hands it to `give_back`. `every_lease_comes_home` checks that every slot is `Free` once every voice is idle.
- **A lease used on another pool.** There is one pool per `Instrument`. A lease isn't branded to its pool; branding would put a lifetime on `Voice`. Tests that build several pools keep each one's leases to itself.

### 4.4 In place

- **The sets are built once.** `SymPool::init_in_place` runs from `Instrument::init_in_place`. It builds each `SympatheticSet` through `uninit_at`, as the voices are built, and writes `SymAlloc::new()` by value (about 64 B). `field_list!` guards both.
- **The sets are never rebuilt.** A note-on clears its lines, as today (§ 4.8): one memset per line, 27,552 B, through `&mut` in place. So `SympatheticSet` needs no `in_place_enum!`.
- **The voice's side is the existing `ModelSlot`.** `SympatheticVoice::init_in_place(slot, lease)` builds the main string with `KsString::init_in_place`, then writes the lease by value (one byte).

**Taking the lease out on a rebuild.** A rebuild overwrites the old payload in place. If that payload is a `SympatheticVoice`, its `Lease` has to come out first, and moving a non-`Copy` field out of `&mut` needs a read without a write-back. `in_place.rs` gains one helper, beside `by_value`:

```rust
/// # Safety
/// The caller overwrites `*r` before it is read or dropped again.
pub(crate) unsafe fn move_out<T>(r: &mut T) -> T { unsafe { core::ptr::read(r) } }
```

`EngineSlot::rebuild` is its only caller, with one `// SAFETY:` line. Every branch of `rebuild` overwrites the payload next, through the macro's generated `rebuild_*`. That rebuild aborts rather than unwinds (`__AbortOnUnwind`), so the stale copy can never be read. `Lease` has no drop glue and no pointer, so even a stale read would be memory-safe; the helper's contract is what stops a logical duplicate.

`EngineSlot::rebuild(kind, pool, voice) -> Rebuilt` is the one place a lease moves:

| From | Into | Lease |
|---|---|---|
| Sympathetic | Sympathetic | kept: moved out, then into the new payload; never returns to the pool |
| Sympathetic | another kind | given back (`give_back`) |
| another kind | Sympathetic | `pool.lend(voice)`: its promise, else a free slot. If neither, it builds `kind.resting()` and returns `Rebuilt::NoSlot` |
| another kind | another kind | none |

### 4.5 Rules

These are pure `SymAlloc` methods, except the shells named at the end. Ages are the `Allocator`'s note ages (`VoiceSlot::age`; `Allocator::next_age()` for a note being placed), so "oldest" and "last played" mean the same thing to both allocators.

- **`place(pick, age) -> Place`.** The `Instrument` calls this for a note-on on a Part whose kind is `Modal(Sympathetic)`. `pick` is the voice `Allocator::pick` would use, or `None` if the CPU budget refuses the note.
  1. `pick` already holds or is promised a slot: that slot's age becomes `age`. If it is `Lent`, `then = To(pick)`. Result: `On(pick)`. A retrigger keeps its slot.
  2. A slot is `Free` and `pick` is `Some`: the slot becomes `Promised { pick, age }`. Result: `On(pick)`.
  3. No slot is `Free`: the oldest slot, whoever holds it, Mono Parts included. Its age becomes `age`, and `then = To(u)` if it is `Lent`. Result: `Steal(u)`. The note plays on `u`, the slot's own voice, so a steal never moves a slot between voices.
  4. A slot is `Free` but `pick` is `None`: `Refused`. That is the CPU budget's refusal (ADR 0026), as for every model, never the pool's. The pool itself never refuses a note.
- **`restart(voice, age) -> Restart`.** The `Instrument` calls this for each held note of a Part whose kind has just become `Modal(Sympathetic)`, newest first.
  1. `voice` already holds or is promised a slot: `Claimed { evict: None }`.
  2. A slot is `Free`: it becomes `Promised { voice, age }`. Result: `Claimed { evict: None }`.
  3. The oldest slot whose age is below `age`. If it is `Promised { w }`, it becomes `Promised { voice, age }` and the result is `Claimed { evict: Some(w) }`. If it is `Lent { u }`, it gets `age` and `then = To(voice)`, and the result is `Claimed { evict: Some(u) }`.
  4. Otherwise: `Silent`.

  Called newest first, the newest four held notes claim slots, and no older note evicts a newer one. So the last four played restart, whatever else held the slots.
- **`lend(voice) -> Option<Lease>`.** First the voice's own `Promised` slot, else any `Free` slot, else `None`. The slot becomes `Lent { then: Free }`, keeping its age.
- **`give_back(lease)`.** `Lent { then: Free }` becomes `Free`, and `Lent { then: To(w) }` becomes `Promised { w }`. A stolen voice's slot is therefore still its own when its waiting note starts. An evicted slot goes to the restart that claimed it, and no other voice rendering in between can take it.
- **`awaits(voice) -> bool`.** Some slot is `Lent` to another voice with `then = To(voice)`: that voice's restart is waiting for an eviction's fade to end.
- **`forfeit(voice)`.** The voice's `Promised` slot becomes `Free`. Nothing else changes.

**Shells:**

- **`Instrument::handle`**, for a Sympathetic note: `place`, then book the note on the voice it names (`Allocator::book`). `On(v)` goes down today's path. `Steal(u)` always takes the fade path, even for the same Part and kind: `waiting[u] = Some(vel)`, then `voices[u].kill()`. So the stolen note fades over `FADE` (owner rule 3). A waiting note it replaces counts in `dropped_unheard`, as today. A stolen Mono voice is booked to the new note; its Part's next note takes a voice as a fresh Mono note does. Other kinds skip `place`.
- **`Instrument::render`, before the voices,** compares each Part's `SlotKind::of` with the last block's (`last_kind: [SlotKind; MAX_PARTS]`). For a Part that has just become `Modal(Sympathetic)`, it takes that Part's held, sounding voices, newest first by `Allocator` age, and calls `restart` for each.
  - `Claimed { evict }`: `waiting[v] = Some(voices[v].velocity())` and `voices[v].kill()`, so the note fades out and restarts once the voice is idle, as a stolen note does. For `evict: Some(u)`, `voices[u].kill()`, and a waiting note of `u`'s is dropped, counted in `dropped_unheard`.
  - `Silent`: `voices[v].kill()`. The voice fades and stays booked, held and silent until its key-up, as a held note whose engine goes quiet does today. Played again, it is a new note under `place`.
  - A killed voice's fade is already running when its own render sees the switch, so no voice-level `Restart` is armed for these notes. Held keys on the other kinds keep § 3's voice-level restart.
- **Render step 5** plays a waiting note on an idle voice, as today, except that a Sympathetic note whose voice `awaits` an eviction stays waiting. It is tried again after the next block's render, and only then does `note_on` run, so no rebuild is spent on the wait.
- **`Voice::trigger`** rebuilds as in § 3 (`EngineSlot::rebuild` lends on the way into Sympathetic). If the rebuild returns `NoSlot`, the note does not start and the voice stays idle. That is reachable only by a lone `Voice` driven without the `Instrument`. Triggering any other kind calls `forfeit(id)`, so a promise made for a note that was then replaced doesn't outlive it.
- **`Voice::rest`**: the voice is idle and no note waits for it. The `Instrument` calls it at render step 5, in the `release_finished` branch. It rebuilds a Sympathetic slot into `resting()` (giving the lease back), then calls `forfeit(id)`. It covers the one way to go idle without a rebuild: the engine going quiet on its own.

**Resting.** An idle voice never holds a lease. `SlotKind::resting` maps `Modal(Sympathetic)` to `Modal(String)`, the same main string without the set, and leaves every other kind alone. `reset` rebuilds into `resting()` when no note follows it (a fade that ends idle, or the VCA's lifetime), and into the Sound's kind when one does (`AfterFade::Note` or `Restart`). So a fade that ends in a note is still one rebuild.

**Mono.** A Mono Part gets no protection from the pool (owner, 2026-09-29). A Mono Sympathetic Part retriggers its own voice and keeps its slot (`place` rule 1). When it holds the oldest slot, a newer note takes it (rule 3). The `Allocator`'s own rule 1, that a Mono voice is never stolen for budget, is unchanged. ADR 0054 records this exception.

**Cost.** The pool does not change a voice's `Cost`. `ModalEngine::COST_SYMPATHETIC` stays 1,400, and it is billed only on the voices that play it, now at most four. Every `SymAlloc` method scans four slots, and `restart` runs at most `MAX_VOICES` times per switch. All of them run at note events, switches and voice ends, never per sample.

### 4.6 The fade and the rebuild bound

ADR 0051 proves that a voice rebuilds at most three times between two blocks, one per stage: the drain (`trigger`), its render (a fade end or a VCA-lifetime reset), and after its render (the waiting note). The pool keeps each stage at one:

- **The drain.** `trigger` rebuilds once. A `NoSlot` build is that one rebuild, into `resting()`.
- **Its render.** `reset` rebuilds once, into the Sound's kind or `resting()`, never both: it chooses by whether a note follows.
- **After its render.** Either the waiting note's `trigger` or `rest`, never both. `rest` runs only when no note waits, and it rebuilds only a Sympathetic slot.

**A steal adds no rebuild beyond the bound.** The stolen voice, in its fade-end block, rests into String inside its render (one rebuild) and gives its slot back promised. Then, after its render, its waiting note rebuilds into Sympathetic and takes that same slot (two). That is today's cross-Part steal exactly. A restart on a switch is the same shape: the voice's fade end rests (one) and its waiting note rebuilds (two). A restart that awaits an eviction spends nothing while it waits, because `note_on` doesn't run. `a_voice_rebuilds_at_most_three_times_a_block` and the switch-storm test stay as pinned, and `a_fifth_sympathetic_note_steals_the_oldest` counts the two.

What the pool adds is one rebuild at a natural end. A Sympathetic note that decays to silence used to leave its model in the idle voice. Now `rest` rebuilds it into String, 3,968 B, about 1,000 cycles. It happens once per note, not per block.

### 4.7 Memory

See § Memory.

### 4.8 Undoing the 16-bit strings

Task 5 made `KsString` and `ModalEngine` generic over a `Store` and set `Q16` as the default (a32923e, 6a3269d, c3194bc, cf3a23f).

**What goes:**

- `modal/q16.rs`: `Exp`, `Q16`, `StepBudget`, `Store`, `store`, `load` and `next_exp`.
- The `S` parameter on `KsString`, `BowedString`, `SympatheticStrings`, `ModelSlot` and `ModalEngine`. A `KsString`'s line is a `[f32; MAX_STRING_DELAY]` again.
- The tests that compare two stores: `q16_saturates_and_steps`, `strings_i16_match_f32`, `strings_i16_with_feedback_stay_bounded`, `pitch_down_after_a_long_tail_reads_no_burst` and `a_short_loop_steps_at_most_once_a_block`.
- Every "ADR 0052" reference in the code.

**The generic store does not earn its place.** It existed to compare Q16 against f32. With one store left, the parameter is dead flexibility on every string type. `in_place_enum!`'s optional generic parameter is Task 1's. It stays, tested by its toy slot, and costs nothing unused.

**What stays.** Each of these is bit-identical in f32:

- **The fused injection.** Each sympathetic string keeps its last output in `pending` and stores it once, with the next sample's coupled input (`KsString::tick_coupled`). It is bit-identical to storing, then loading and storing again. It cut Sympathetic from about 593 to 411 hot-path instructions a sample (ADR 0052's table).
- **One-pass `damp(passes)`.** It loads once, multiplies `passes` times and stores once. That is bit-identical to repeated passes.
- **A note-on clears every line it starts, the whole ring.** It changes sound only where a PITCH route lengthens a loop after a retrigger: that loop now reads silence where it read the last note's tail, and no golden plays that. With a pool this matters more: a slot handed to a new voice carries nothing of its last holder's note, so a note on a reused slot sounds exactly as on a fresh pool (`a_handed_over_slot_carries_nothing`).

**Goldens (ADR 0011).** Measured on cf3a23f with both defaults switched to f32, every render Task 5 re-recorded matches its pre-Task-5 value exactly:

- `modal_init` 0x90f1197c153d0b05
- `modal_lfo_cutoff` 0xe9e4fe3dda9b0262
- `algo_to_modal_switch` 0x501ad70c947a9c4d
- instrument `two_parts_two_pairs` 0x851eab45ed8a2f86, and 0x016712a2b7e18d83 before the limiter
- the `init_modal.snd` render 0x90f1197c153d0b05

So the revert restores those rows and their comments byte for byte (`git checkout 0a7d468 --` on the three test files, whose only Task 5 changes are those rows).

No golden plays Sympathetic. So the revert task records one, `modal_sympathetic`, while the set is still inline. Moving the set into the pool must keep it bit-identical. That is the proof that the pool changed no sound.

## Memory

Host sizes (64-bit), measured at cf3a23f and projected from those measurements. The plan measures them on `thumbv7em`, where `usize` is 4 B, and commits the figures.

| | f32 in every voice (cf3a23f, f32 store) | Q16 (Task 5, superseded) | Pool (this spec) |
|---|---|---|---|
| `KsString` | 3,960 | 2,000 | 3,960 |
| `ModelSlot` (largest payload + tag) | 31,744 | 16,064 | 3,976 = 3,968 + 8 |
| `ModalEngine` (+ 24 shared) | 31,768 | 16,088 | 4,000 |
| `EngineSlot` (+ 8 tag) | 31,776 | 16,096 | 4,008 |
| `Voice` (+ chain 1,832) | 33,608 | 17,928 | 5,840 |
| `[Voice; 8]` | 268,864 | 143,424 | 46,720 |
| `SympatheticSet` × 4 | — | — | 4 × 27,776 = 111,104 |
| `SymAlloc` | — | — | ≈ 64 |
| Rest of `Instrument` | 2,656 | 2,656 | 2,656 |
| **`Instrument`** | 271,520 | 146,080 | ≈ 160,544 |
| **Left under `VOICE_RAM_BUDGET` (286,720)** | 15,200 | 140,640 | **≈ 126,176** |

- **Per voice:** 5,840 B, down from 33,608 B in f32. Of that, 4,008 B is the slot.
- **Pool:** 111,104 B, four sets of 7 × 3,960 + 28 + 28.
- **Total:** 46,720 + 111,104 + 64 + 2,656 = 160,544 B.
- **Headroom:** 126,176 B. That is 14,464 B less than Q16 left, and 110,976 B more than f32 in every voice.
- **AXI:** unchanged, because nothing moves there.
- The `SymAlloc` figure is an estimate (four states of ≤ 12 B and a clock). Its `VoiceIdx`, if `Voice` stores one (plan Task 9), may add up to 8 B a voice, inside `VOICE_CHAIN_BYTES`.

**What it's for:** headroom for Modal 2's exciters. None of it is spent here. `MAX_VOICES` stays 8, `MAX_STRING_DELAY` stays 984 (ADR 0040), and `SYM_SLOTS` is 4.

**The asserts.** `voice.rs`'s `[Voice; MAX_VOICES] <= VOICE_RAM_BUDGET` and `instrument.rs`'s `Instrument <= VOICE_RAM_BUDGET` stay. These are added:

- `size_of::<EngineSlot>() <= max(size_of::<AlgoEngine>(), size_of::<ModalEngine>()) + align_of::<EngineSlot>()`, and the same for `ModelSlot` over its payloads. This proves the slots are exclusive: the largest payload plus the tag, never the sum.
- `size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>()`, with `VOICE_CHAIN_BYTES = 2048`. The voice is its chain plus its largest slot, and the chain can't grow unnoticed.
- `size_of::<SympatheticVoice>() <= size_of::<BowedString>().max(size_of::<KsString>())`. Sympathetic never sizes the voice (§ 4.2).

`memory_budget_test` prints the chain, each payload, the pool, the `Instrument` and the headroom.

## Tests

| Test | Asserts |
|---|---|
| `switch_never_clicks` | A held Algo note, whose Part switches to Modal. From the switch to the fade's last sample, the largest \|y[n] − y[n−1]\| ≤ S + A / `FADE`, where S is the largest step and A the peak in the block before the switch: the signal's own slew plus the fade's slope. After the fade, the output is bit-identical to a fresh voice playing the new Sound, so any step there is the new note's own onset. The same for a MODE switch (String → Sympathetic). |
| `idle_voice_switches_in_the_same_block` | A voice that went quiet on Algo, then the Sound becomes Modal: the next note-on's first block is bit-identical to a fresh Modal voice's, and `rebuilds` rose by 1. |
| `other_parts_are_untouched_by_a_switch` | Parts 1 and 2 sounding; Part 1 switches engine. Part 2's bus is bit-identical to a run without the switch, block for block, and `FxBus` is never reset: a reverb tail ringing at the switch is still non-zero in the next block. |
| `model_switch_rebuilds_once` | A ringing String note whose MODE becomes Sympathetic: `rebuilds` rises by exactly 1 from the switch through the held note's restart, and the restarted note is bit-identical to a fresh Sympathetic voice's. |
| `knob_moves_never_rebuild` | A ringing Sympathetic note while every Modal param but MODE, and every chain param, moves each block: `rebuilds` is unchanged, the voice never fades, and the output differs from the unmoved run (the knobs reach the ringing note). |
| `sym_alloc_*` (pure, `sym_alloc.rs`) | `place` promises a free slot to its pick; a retrigger keeps its slot; the fifth `place` steals the oldest, the sixth the next oldest (ages refresh on steal); a full pool steals even when `pick` is `None`; `give_back` after a steal leaves the slot promised to the same voice; `lend` takes the voice's promise before a free slot; `forfeit` frees only a promise. |
| `a_mono_holder_is_stolen_when_newest_arrives` | Pure, and again through the `Instrument`: a Mono Part's voice holds the oldest of four slots. A new Sympathetic note on another Part steals it, the Mono voice fades over `FADE`, and the new note sounds on it. |
| `last_four_played_restart_on_switch` | Pure: `restart` called newest first on eight held notes with four free slots claims the four newest, and the four oldest get `Silent`. With two slots held by older notes of another Part, the two evictions are those holders, and `awaits` is true for the claimers until `give_back`. |
| `sym_alloc_never_gives_a_voice_two_slots` | A seeded random walk of 100,000 `place`/`restart`/`lend`/`give_back`/`forfeit` calls over 8 voices. After each: every voice appears in at most one slot; the `Lent` count equals the leases the walk holds; `Free + Promised + Lent == 4`. |
| `Lease` doc tests | `compile_fail,E0599`: `lease.clone()`. `compile_fail,E0382`: `alloc.give_back(lease); pool.set(&lease)`. `compile_fail,E0423`: `Lease(…)` outside `sym_alloc`. |
| `a_fifth_sympathetic_note_steals_the_oldest` | Part 1 (Sympathetic, Poly) holds notes 48, 50, 52 and 53; Part 2 (Sympathetic) then plays 55. The voice of 48 fades over `FADE`, and Part 1's largest step obeys `switch_never_clicks`'s bound. Then 55 sounds on that voice, and Part 2's bus is bit-identical to a lone fresh 55 from its first block. At every block, at most four voices sound Sympathetic and `lent() <= 4`. The stolen voice's `rebuilds` rise by exactly 2 in its fade-end block. |
| `other_models_keep_eight_voices` | Eight held notes on String, on Bowed and on the bank each sound on eight voices while `sym.free() == 4`. |
| `four_sympathetic_notes_sound_as_alone` | Each of four Sympathetic notes on four Parts, whose buses are recorded, is bit-identical to that note alone. The pool shares no state between slots. |
| `a_handed_over_slot_carries_nothing` | Note A takes slot 0 on voice 0 and ends. Note B then takes slot 0 on voice 1. B is bit-identical to B on a fresh pool. |
| `every_lease_comes_home` | A seeded storm over 3 Parts: note-ons and -offs, MODE flips among all four models, ENGINE flips and kills, for 2,000 blocks. At every block, the voices whose slot is Sympathetic number exactly `lent()`. After all notes are off and the voices are idle, `free() == 4`. |
| `the_last_four_played_restart_through_the_instrument` | Eight String notes, 60 to 67 in that order, are held; MODE becomes Sympathetic. After the fade, the voices of 64–67 sound Sympathetic, and those of 60–63 are idle, held and silent. Each restarted voice's `rebuilds` rose by exactly 2 (rest, then the note). Key-up and re-press 60: it sounds, and 64's voice, the oldest, fades. |
| `a_resting_voice_gives_its_slot_back` | A Sympathetic note that decays to silence: in the block it goes idle, `rebuilds` rises by 1, its slot is `Free`, and its slot kind is `Modal(String)`. |
| `resting_maps_only_sympathetic` | Pure: `SlotKind::resting` maps `Modal(Sympathetic)` to `Modal(String)` and returns every other kind unchanged. |
| `modal_sympathetic` (golden) | Recorded before the pool (Task 7); bit-identical after it (Task 9). |
| `slot_layout_matches_repr` | For each variant of both slots, after a rebuild, `match` yields a payload whose address is the mirror's payload address, and the tag reads back. |
| Size asserts | The const asserts in § Memory, repeated in `memory_budget_test` with printed numbers. |
| `rebuild_fits_a_small_stack` | Engine and model switches, Sympathetic included, run on a `std::thread` whose stack is `size_of::<SympatheticSet>()` bytes: they pass. The plan proves the test can fail: building a set by value at note-on (`*set = SympatheticSet::new()`) must overflow it. On the chip, the bench runs a switch storm and records the AUDIO page's stack mark (ADR 0025), which may not grow by more than 1 KB over today's. |

The existing goldens, sanity, instrument and cost tests pass unchanged. The goldens Task 5 re-recorded go back to their earlier values (§ 4.8).

## Interactions with in-flight branches

This project touches `dsp/engines.rs`, `dsp/voice.rs`, `dsp/modal/{mod,string}.rs`, `in_place.rs`, `voice_alloc.rs`, `instrument.rs` and the new `sym_alloc.rs`, plus tests.

`Voice::note_on`, `note_off` and `render` gain a `&mut SymPool` argument, and `Voice::init_in_place` a `VoiceIdx`. `kill`, `is_active`, `cost` and `held_model_extra` keep their signatures. The test suites that drive a `Voice` directly move to a `common::Rig` (a boxed voice and pool) with the old signatures.

- **`gain-staging`** is merged (0a7d468). It added the limiter; this project doesn't touch `fx_bus.rs` or `limiter.rs`.
- **`algo-init-morph`** is merged. `algo_to_modal_switch`'s restored row carries both of its earlier comments.
- **`projects`** adds `DiskCode` to `ResonatorMode` in `modal/params.rs`. This project doesn't touch `params.rs`, and `ResonatorMode` keeps its four variants and codes. A project load kills every voice (projects spec, step 4). Each fade end rests or rebuilds into the Part's new kind, down the paths in § 3 and § 4.5, and gives any lease back. No new code. Loading a Sound into a Part is a switch as in § 3.
- **#207** (per-Part MODE and VOICES, plan 2) reads `SYM_SLOTS` as Sympathetic's ceiling. It is not built here.
- **#205** (a Modal → Algo fade isn't billed as `held_model_extra` bills Modal ones) is unchanged.

## Out of scope

- Modal 2: the exciters, the model pages, and the #191 behaviours (String feedback self-oscillation, Bowed reading no page params, no dimming, the inaudible ensemble).
- The cost model, the voice count, the UI and the string length.
- The per-Part VOICES control (#207).

## Plan order

Each task leaves the tree green. Tasks 1–4 are done. Tasks 5 and 6 are replaced.

1. `in_place_enum!` and `slot_layout_matches_repr`.
2. `EngineSlot` replaces `Engines`. Goldens bit-identical.
3. `ModelSlot` in `ModalEngine`. Goldens bit-identical.
4. Switching on `SlotKind`, the rebuild count, and the switch tests. Goldens bit-identical.
5. ~~16-bit block-float strings~~ (done, then replaced by 7).
6. ~~Chip figures~~ (replaced by 10).
7. Strings back to f32, without the generic store. The Task 5 goldens are restored, and `modal_sympathetic` is recorded.
8. `SymAlloc`, `Lease` and `VoiceIdx`: the pure allocator and its tests.
9. The pool wired in: `SympatheticSet`, `SymPool`, `SympatheticVoice`, `EngineSlot::rebuild`'s leases, `rest`, the `Instrument`'s placement, and § Tests' pool tests. ADR 0054 is written here.
10. Chip: sizes, bench rows (MODAL, SYMPATHETIC, REBUILD, SYM NOTE-ON), and the stack mark. The figures go in ADRs 0051 and 0054.

## Decisions made (owner, 2026-09-29)

- Exclusive state is sub-project 1 of Modal 2. Modal 2 follows in its own spec.
- A voice holds only its active engine, in an exclusive slot rebuilt in place. This supersedes ADR 0008.
- Modal holds only its active model's state (bank, single string, or sympathetic strings). Bowed stays as it is here and becomes an exciter in Modal 2.
- An engine or MODE change fades that Part's sounding voices over about 3 ms (the existing 128-sample fade). Each slot is rebuilt once silent, and the new kind plays from the next note; a held key restarts on it. An idle voice switches at once. Other Parts and the FX tails are untouched.
- Knob moves never rebuild; they reshape ringing notes, as today and as in Rings.
- Engine sound, the cost model, 8 voices, the UI and the #191 behaviours don't change here.
- The freed memory is headroom for Modal 2's exciters and is not spent here.
- A held key restarts its note on the new engine or model after the fade, as engine changes already do.
- **(2026-09-29, superseding the 16-bit decisions.)** Sympathetic keeps full f32 precision through a shared pool of four sympathetic slots, after Rings (`kMaxPolyphony = 4`). Voices are sized for the largest other model. A voice borrows a slot while its note sounds and gives it back when it goes idle or rebuilds. Sympathetic plays at most four notes: a fifth steals the oldest slot, whose voice fades over `FADE` before the new note takes it. Other models keep eight voices.
- **(2026-09-29.)** The 16-bit strings are undone, and strings are f32 again. ADR 0052 is superseded, because the pool makes 16-bit storage unnecessary and it lost precision (−83.7 dBFS at worst).
- **(2026-09-29.)** The per-Part VOICES control is #207, not built here. This project provides the cap, `SYM_SLOTS = 4`.
- **(2026-09-29, the Rings rule.)** When more than four held notes switch to Sympathetic, the last four played restart. The rest fade and stay silent until they are played again.
- **(2026-09-29, the Rings rule.)** A new Sympathetic note always sounds. If every slot is held, including by Mono Parts, the oldest slot's voice fades over `FADE` and the new note takes that slot. Mono gets no protection from the pool.

**Design choices in § 4, for the owner's review:**

- The main string stays in the voice, and a slot holds the seven lines (§ 4.2).
- The pool is in D2, inside the `Instrument`.
- A steal plays the new note on the stolen slot's own voice.
- A restart on a switch goes through the `Instrument`'s waiting note, ranked by `Allocator` age, not through the voice-level `Restart`.
- The generic store is removed.

## Open questions

None.
