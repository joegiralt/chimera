# 0054. Sympathetic borrows its strings from a shared pool of four

- **Status:** Proposed
- **Deciders:** project owner
- Supersedes [0052](0052-strings-stored-as-16-bit-block-float.md) (16-bit
  block-float strings, never accepted).

## Context
Sympathetic is one main string that sets seven more ringing: eight
`KsString`s of 3,960 B, where every other Modal model holds one string or
a 1,008 B resonator bank. With one engine per voice (ADR 0051), the voice
is sized for its largest model, so every voice paid 31,736 B for
Sympathetic, and the `Instrument` was 271,520 B of the 286,720 B D2 budget,
with 15,200 B left for Modal 2's exciters.

ADR 0052 halved every string to 16-bit block float instead. It fit, but
Sympathetic's high notes missed the −90 dBFS gate (−83.7 dBFS at C6), and
every Q16 read and write cost instructions on the hot path.

Rings (`rings/dsp/part.h`, `part.cc`) caps its polyphony at 4
(`kMaxPolyphony`) and shares a fixed set of 8 strings among the voices it
plays.

## Decision
Every string stays f32. Sympathetic's seven sympathetic lines live in a
pool of four slots, lent to the voices that play it (exclusive-state spec
§ 4.2–4.6). A note rings a slot only if one is free when it starts;
otherwise it plays bare. Nothing is stolen for the pool (the owner's rule,
2026-09-29, which superseded the Rings rule first proposed here).

- **A slot** is a `SympatheticSet` (`dsp/modal/mod.rs`): 7 lines, 7 ratios
  and 7 pending outputs (the fused injection's), 27,832 B on the host.
- **The main string stays in the voice**, in `ModelSlot::Sympathetic`'s
  `SympatheticVoice { main, halo }`, which is no larger than Bowed's
  payload (a const assert): Sympathetic never sizes the voice.
- **The pool is in D2, inside the `Instrument`**: `Instrument::sym`, a
  `SymPool { alloc: SymAlloc, sets: [SympatheticSet; 4] }`, built once in
  place (`SymPool::init_in_place`). `size_of::<Instrument>() <=
  VOICE_RAM_BUDGET` stays asserted.
- **The `Lease`** (`sym_alloc.rs`) is the right to one slot: only
  `SymAlloc::lend` makes one, from a slot promised to the voice, only
  `give_back` takes one, and it is not `Clone` or `Copy`.
- **The `Halo`**, `Full(Lease)` or `Bare`, is a Sympathetic note's seven
  lines, fixed when it starts. It is an enum, not an `Option<Lease>`: it has
  no `insert`, `get_or_insert` or `replace`, and `SympatheticVoice` has no
  setter, so only an in-place rebuild writes one. A rebuild happens only as
  a note starts or ends, so a bare note never borrows mid-note, and `Full`
  always holds a real slot's lease. A voice reaches its set only through
  `SymPool::halo(&mut self, &Halo)`. Rendered bare, a note plays its main
  string alone.
- **`EngineSlot::rebuild(kind, pool, voice)`** is the one place a lease
  moves. Sympathetic `Full` into Sympathetic keeps it; into another kind
  gives it back; anything into Sympathetic lends the voice's promise, and
  builds `Bare` without one. The lease leaves the old payload through
  `in_place::move_out`, the one new `unsafe`.
- **`SymAlloc`** is pure and keeps no ages: `place(voice)` keeps a slot the
  voice holds or is promised, else promises a free one, else the note plays
  bare; `lend` lends only a promise; `give_back` frees; `cancel` frees a
  promise. A `Promised` slot is reserved for a note that waits, on another
  Part's fade or on the clear budget, before it starts.
- **Who places.** `Instrument::handle` places a Sympathetic note on the
  voice `Allocator::pick` chose. `Instrument::start` (render steps 0 and 5)
  places a waiting note as it starts, since its Part may have become
  Sympathetic while it waited. A bare Sympathetic voice whose new note is
  promised a slot is rebuilt as that note starts (`Voice::trigger`), never
  while a note sounds.
- **`cancel(v)`**: `v` wants no slot now, and its promise is freed. The
  voice owns it, at two sites: `Voice::trigger`, where every note starts
  (for any kind but Sympathetic), and `Voice::rest`, where every voice is
  freed. So no slot is ever promised to a voice the `Allocator` holds free.
- **A switch onto Sympathetic** restarts every held note of the Part:
  `Instrument::render` places them newest first, so the last four played
  take the free slots and the rest play bare, and each fades and restarts
  through the `Instrument`'s waiting note. Nothing is evicted.
- **A note-on clears every line it starts** (spec § 4.8), so a set handed
  to another voice carries nothing of its last note: a note on a reused
  slot sounds exactly as on a fresh pool. Each line clears only up to its
  **dirty extent**: every sample at or past `KsString::dirty` is already
  0.0, and `set_freq` raises it to each new loop length. A line last
  played high clears a loop, not its ring, and the result is bit-identical
  to clearing the whole ring.
- **At most one worst-case clear a block.** `SymPool::note_on_clear`
  gives, purely, the bytes a Sympathetic note-on will clear.
  `Instrument` spends `SYM_CLEAR_BUDGET` a block: one worst-case note-on,
  eight whole rings, 31,488 B. A note-on past what is left waits a block on
  the waiting path, its slot still promised, and so does one behind a note
  already waiting on the budget. Waiting notes start oldest first, before
  their voices render, in the block that lets them in. A chord of four
  slots last played low starts over four blocks (5.3 ms), and a typical one
  at once. A bare note clears only its main string. Nothing is refused.
- **`SlotKind::resting`** maps `Modal(Sympathetic)` to `Modal(String)`: an
  idle voice holds no lease. `Voice::reset` rebuilds into the Sound's kind
  when a note follows the fade and into `resting()` when none does, and
  `Voice::rest` rests a voice whose engine went quiet on its own.

## Alternatives considered
- **The Rings rule: the oldest yields** (proposed here first, 2026-09-29)
  — with every slot held, a new note stole the oldest slot, whose voice
  faded over `Voice::FADE` first, Mono Parts included; on a switch only the
  last four held notes restarted and the rest fell silent. Rejected by the
  owner: it cut ringing notes short. It also needed ages, claims bound for
  other voices (`Then::To`), evictions, dropped waiters and a Mono
  exception to the `Allocator`'s rules, all deleted with it.
- **Reserving note-on headroom in the cost model** — one worst note-on a
  block is 41k cycles, 640 a sample: more than a whole Modal voice, billed
  to every patch, all the time, for a transient.
- **Q16 strings (ADR 0052)** — lost precision: −83.7 dBFS at C6 against the
  −90 dBFS gate, and extra work per sample.
- **f32 in every voice** — an `Instrument` of 271,520 B, leaving 15,200 B of
  D2 for Modal 2.
- **The main string in the pool too** — +15,840 B (4 × 3,960) for no saving
  in the voices, which are sized for Bowed's string anyway.
- **Slot `s` bound to voice `s`** — a Sympathetic note on voices 4–7 would
  play bare while slots sat free.
- **`Option<Lease>` for the halo** — the same two states, but with
  `insert` and `get_or_insert` at hand to attach a lease to a sounding note.
- **Refusing a fifth note** — a key that makes no sound; bare keeps the note
  and loses only its halo.
- **The pool in AXI** — 111,104 B of sets against about 24.9 KB of AXI kept
  spare.

## Consequences
- At most 4 Sympathetic notes ring a set at once; more play bare, at once,
  on voices of their own. Nothing fades, is stolen or is lost for the pool,
  and other models keep 8 voices. A note keeps the halo it started with.
- A bare note is billed as a ringing one (`COST_SYMPATHETIC`): it costs
  less, so the bill is an upper bound.
- One more rebuild per Sympathetic note: `rest`, at its natural end (a
  `KsString`'s worth of stores, once per note).
- **Measured on the chip (bench f59bc92, rev V at 480 MHz, 2026-09-29).**
  - SYM POOL 110,912 B.
  - MDL STR /VOICE 424, within String's billed 447.
  - MDL SYM: totals 1,120, 1,974, 2,830 and 3,698 at one to four notes,
    flat past four, the pool's cap under the Rings rule. That is a slope of
    859 a ringing voice, within the 1,457 billed. `COST_SYMPATHETIC` is now
    802, the slope less the Modal Sound's chain of 57, as String's is.
    Beside this branch's FX bus (1,360), it gives 6 voices on rev V and 5
    on rev Y. Beside 1,160, the bus after PR #212, it gives the same 6 and
    5. Four ring; the rest play bare.
  - SYM NOTE-ON 40,906 cycles with the whole-ring clear. That reading led
    to the dirty extent and the per-block budget above. With them, the
    estimate is about 9–10k for A4 after A4. The worst case, a slot last
    played at or below G1, stays about 41k, and the budget takes one a
    block. The bench's `SYM NOTE-ON` and `SYM NOTE-ON LOW` rows read both
    at the ship build.
- **Host sizes with the dirty extent, the clear budget and the halo:**
  `Voice` 5,848 B, `SymPool` 111,336 B (`SymAlloc` 8 B), `Instrument`
  160,792 B, which leaves 125,928 B of D2.
- ADR 0051's bound, three rebuilds per voice per block, is unchanged: a
  restart on a switch is a fade end resting (one) and its waiting note
  (two), as a cross-Part steal is, and a note waiting on the budget spends
  none.
- One `move_out` in `in_place.rs`, called only by `EngineSlot::rebuild`.
- The cap is what the per-Part VOICES control shows
  (https://github.com/joegiralt/chimera/issues/207): `SYM_SLOTS = 4`.
- The `Allocator`'s rule 1, that a Mono voice is never stolen, now holds
  without exception: a Mono Sympathetic Part plays bare on its own voice
  when the pool is full.
- A note that waited on another Part's fade while its own Part became
  Sympathetic is placed as it starts, and plays bare with no slot free
  (`a_note_waiting_across_a_switch_to_sympathetic_is_placed`).
- A lone `Voice` driven without the `Instrument`'s placement plays
  Sympathetic bare.

## Sources
- `docs/superpowers/specs/2026-09-29-exclusive-state-design.md` § 4 and
  § Memory.
- The exclusive-state plan, Tasks 7–9.
- Mutable Instruments Rings, `rings/dsp/part.h` and `part.cc` (MIT, Emilie
  Gillet): the design idea only, no code.
- ADRs [0051](0051-a-voice-holds-one-engine-rebuilt-in-place.md) and
  [0052](0052-strings-stored-as-16-bit-block-float.md).
