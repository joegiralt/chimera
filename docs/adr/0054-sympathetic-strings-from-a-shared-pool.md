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
§ 4.2–4.6):

- **A slot** is a `SympatheticSet` (`dsp/modal/mod.rs`): 7 lines, 7 ratios
  and 7 pending outputs (the fused injection's), 27,776 B on the host.
- **The main string stays in the voice**, in `ModelSlot::Sympathetic`'s
  `SympatheticVoice { main, lease }`, which is no larger than Bowed's
  payload (a const assert): Sympathetic never sizes the voice.
- **The pool is in D2, inside the `Instrument`**: `Instrument::sym`, a
  `SymPool { alloc: SymAlloc, sets: [SympatheticSet; 4] }`, built once in
  place (`SymPool::init_in_place`). `size_of::<Instrument>() <=
  VOICE_RAM_BUDGET` stays asserted.
- **The `Lease`** (`sym_alloc.rs`) is the right to one slot: only
  `SymAlloc::lend` makes one, only `give_back` takes one, and it is not
  `Clone` or `Copy`. `SympatheticVoice` holds a `Lease`, not an `Option`,
  and `Model::Sympathetic(Lease)` is the only way to build one: no slot, no
  Sympathetic. A voice reaches its set only through `SymPool::set(&mut
  self, &Lease)`.
- **`EngineSlot::rebuild(kind, pool, voice) -> Rebuilt`** is the one place
  a lease moves. Sympathetic into Sympathetic keeps it; out of Sympathetic
  gives it back; into Sympathetic lends the voice's promise or a free slot,
  and with neither builds `kind.resting()` and returns `Rebuilt::NoSlot`.
  The lease leaves the old payload through `in_place::move_out`, the one
  new `unsafe`.
- **`SymAlloc`** is pure, with the `Allocator`'s note ages (`VoiceSlot::age`,
  `Allocator::next_age`): `place` promises a free slot to the voice
  `Allocator::pick` chose, keeps a retrigger's slot, and with every slot
  held steals the oldest. A steal plays the new note on the slot's own
  voice (`Instrument::handle` books it there, fades it and queues the note),
  so a slot never moves between voices. `lend` takes the voice's promise
  first, and `give_back` hands a stolen or evicted slot on to its waiting
  note.
- **`cancel(v)`**: `v` wants no slot any more. Its promise is freed, and a
  slot lent to another voice but bound for `v` goes free when that lease
  comes back. `v`'s own lease is never touched: only `give_back` ends it.
  The voice owns it, at two sites: `Voice::trigger`, where every note
  starts (for any kind but Sympathetic), and `Voice::rest`, where every
  voice is freed (render step 5's only path to `release_finished` calls
  it). So no slot is ever promised or bound to a voice the `Allocator`
  holds free.
- **Waiting notes, and what drops them.** A note placed on a voice that
  still `awaits` another's fade (a restart's claim) waits as a stolen note
  does, and is tried after each block's render. A newer note can take the
  slot a waiting note was bound for: `place` and `restart` report that
  waiter in `drops`, and the `Instrument` clears its waiting note and
  counts it in `Allocator::dropped_unheard`. A note shed for the budget,
  or by the Mono rule below, before it sounded is counted there too (ADR
  0027).
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
  eight whole rings, 31,488 B. A note-on past what is left waits on the
  stolen note's path, and so does one behind a note already waiting on the
  budget. It starts before its voice renders in the block that lets it in.
  A chord of four slots last played low starts over four blocks (5.3 ms),
  and a typical one at once. Nothing is refused.
- **`SlotKind::resting`** maps `Modal(Sympathetic)` to `Modal(String)`: an
  idle voice holds no lease. `Voice::reset` rebuilds into the Sound's kind
  when a note follows the fade and into `resting()` when none does, and
  `Voice::rest` rests a voice whose engine went quiet on its own.
- **The Rings rule.** The oldest yields, Mono Parts included. The pool
  never refuses a note-on: `place` returns `Refused` only when the CPU
  budget gave no voice and a slot is free (ADR 0026). Every note it places
  sounds, at once or once the fade ahead of it ends, unless a newer note
  takes its slot first (a drop, counted).
- **A Mono Part keeps one voice.** When the pool steals onto another
  voice for a Mono Part's note, `Allocator::book` sheds the Part's old Mono
  voice, which fades out.
- **A switch onto Sympathetic** restarts the last four held notes played:
  `Instrument::render` calls `SymAlloc::restart` for the Part's held,
  sounding notes, newest first. Each claimer fades and restarts through the
  `Instrument`'s waiting note (evicting an older holder if it must); the
  rest fade and stay silent until played again. A waiting note whose slot's
  holder still fades (`awaits`) waits another block without a rebuild.

## Alternatives considered
- **Reserving note-on headroom in the cost model** — one worst note-on a
  block is 41k cycles, 640 a sample: more than a whole Modal voice, billed
  to every patch, all the time, for a transient.
- **Q16 strings (ADR 0052)** — lost precision: −83.7 dBFS at C6 against the
  −90 dBFS gate, and extra work per sample.
- **f32 in every voice** — an `Instrument` of 271,520 B, leaving 15,200 B of
  D2 for Modal 2.
- **The main string in the pool too** — +15,840 B (4 × 3,960) for no saving
  in the voices, which are sized for Bowed's string anyway.
- **Slot `s` bound to voice `s`** — a Sympathetic note would steal an Algo
  note on voices 0–3 while voices 4–7 sit free.
- **A registry without a token** — a Sympathetic model without a slot, or a
  slot named by two voices, becomes representable.
- **Protecting Mono, or refusing a fifth note** — the owner chose Rings'
  rule, that the oldest always yields.
- **The pool in AXI** — 111,104 B of sets against about 24.9 KB of AXI kept
  spare.

## Consequences
- **Memory (host, measured 2026-09-29):** `Voice` 5,840 B (the `VoiceIdx`
  fits its padding), `[Voice; 8]` 46,720 B, `SympatheticSet` 27,776 B,
  `SymPool` 111,136 B, `Instrument` 160,520 B, which leaves 126,200 B of
  D2. The spec projected 5,840, 111,104 + 64, 160,544 and 126,176. The
  chip's figures come with plan Task 10.
- At most 4 Sympathetic notes sound at once; a fifth steals the oldest,
  whose voice fades over `Voice::FADE` first. Other models keep 8 voices.
- One more rebuild per Sympathetic note: `rest`, at its natural end (a
  `KsString`'s worth of stores, once per note).
- **Measured on the chip (bench f59bc92, rev V at 480 MHz, 2026-09-29).**
  - SYM POOL 110,912 B.
  - MDL STR /VOICE 424, within String's billed 447.
  - MDL SYM: totals 1,120, 1,974, 2,830 and 3,698 at one to four notes,
    flat past four, the pool's cap. That is a slope of 859 a sounding
    voice, within the 1,457 billed. `COST_SYMPATHETIC` is now 802, the
    slope less the Modal Sound's chain of 57, as String's is. Beside this
    branch's FX bus (1,360), it gives 6 voices on rev V and 5 on rev Y.
    Beside 1,160, the bus after PR #212, it gives the same 6 and 5. The
    pool sounds 4 of them.
  - SYM NOTE-ON 40,906 cycles with the whole-ring clear. That reading led
    to the dirty extent and the per-block budget above. With them, the
    estimate is about 9–10k for A4 after A4. The worst case, a slot last
    played at or below G1, stays about 41k, and the budget takes one a
    block. The bench's `SYM NOTE-ON` and `SYM NOTE-ON LOW` rows read both
    at the ship build.
- **Host sizes since the dirty extent (4 B a line) and the clear budget:**
  `Voice` 5,848 B, `SymPool` 111,360 B, `Instrument` 160,816 B, which
  leaves 125,904 B of D2. On the chip, the firmware's `.ram_d2` is
  160,244 B.
- ADR 0051's bound, three rebuilds per voice per block, is unchanged: a
  steal or a restart is a fade end resting (one) and its waiting note (two),
  as a cross-Part steal was, and a note that `awaits` spends none.
- One `move_out` in `in_place.rs`, called only by `EngineSlot::rebuild`.
- The cap is what the per-Part VOICES control shows
  (https://github.com/joegiralt/chimera/issues/207): `SYM_SLOTS = 4`.
- A Mono voice can be stolen by the pool: an exception to the
  `Allocator`'s rule 1, which still never steals a Mono voice for budget.
- Edge cases, each pinned by a test in `sym_pool_test.rs`:
  - A note-on for a voice whose restart awaits an eviction waits for that
    fade too, instead of starting without a slot.
  - A claimer booked again for another kind, or shed, before its eviction's
    fade ends leaves no promise behind.
  - A Mono Part whose held note went `Silent` on a switch, then plays, sheds
    that silent voice and sounds on the stolen one.
- A pool steal plays the new note on the stolen slot's voice after a
  fade: a future glide (portamento) will not carry across it.
- `Voice::trigger` can still meet `Rebuilt::NoSlot`, but only for a lone
  `Voice` driven without the `Instrument`'s placement: the note does not
  start.

## Sources
- `docs/superpowers/specs/2026-09-29-exclusive-state-design.md` § 4 and
  § Memory.
- The exclusive-state plan, Tasks 7–9.
- Mutable Instruments Rings, `rings/dsp/part.h` and `part.cc` (MIT, Emilie
  Gillet): the design idea only, no code.
- ADRs [0051](0051-a-voice-holds-one-engine-rebuilt-in-place.md) and
  [0052](0052-strings-stored-as-16-bit-block-float.md).
