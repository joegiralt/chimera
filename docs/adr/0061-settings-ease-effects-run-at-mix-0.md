# 0061. Every continuous setting eases; an effect runs at MIX 0

- **Status:** Proposed
- **Deciders:** owner; firmware (Modal 2, task 17)
- **Relates to:** CLAUDE.md ("all parameter changes lerped — never snap"),
  #53 (CUTOFF's per-block ramp), ADR 0053 (the delay's MECHANICS),
  ADR 0056 (the host cost method), ADR 0060 (the voice's DC blocker)

## Context
The parameter sweep found 80 jumps that click:
- OUT LEVEL, Part LEVEL, PAN and sends, each effect's MIX, chorus DEPTH
  and MODE, delay TIME, DRIVE and its MIX, filter DRIVE, and FOLD and
  SYM all took a new value at a block's edge. A LEVEL step on the
  triangle probe reached 27–37× the steady second difference.
- An effect below MIX 0.001 stopped running. Its return cut out, and its
  delay line, chorus lines or reverb ring froze. Bringing MIX back up
  replayed a tail from minutes before (#61, #65).
- The codebase already eased two ways: REV SEND by a 20 ms one-pole once a
  block, ramped across the block, and CUTOFF's `g` by a ramp across the
  block from the last block's value (#53).

## Decision
- **Two eases, as the codebase had them** (`dsp::ease`):
  - `Ease`, for a value only the UI moves: a 20 ms one-pole once a block,
    ramped linearly across the block. It lands within 1e-5 of the target
    (−100 dB), then holds the target exactly, so a steady setting is bit
    for bit what it was.
  - `Ramp`, for a value a route may move every block: from the last
    block's value to this one's across the block.
- **The bus:** each Part's two pan×LEVEL gains and three sends, each
  effect's MIX, chorus DEPTH, and each line's share of chorus MODE (so a
  MODE change crossfades the lines) take `Ease`. The mix plays each gain
  at its block's start. A pass then adds `bus · step·(i + 1)` for each
  moving gain only (`instrument::glide`), so a still mix costs nothing
  extra.
- **The voice:** the stored OUT LEVEL, DRIVE, TONE, MIX, filter DRIVE,
  FOLD, SYM and fold MIX take `Ease`; routes add to the eased value, and
  each stage `Ramp`s what it plays across the block. A fresh note starts
  on its values, unramped.
- **A stage switched off fades out.** Each stage has a gate (`ease::Gate`)
  that fades its wet linearly over 20 ms (15 blocks), then rests; switched
  on, it fades in. The gate is keyed on `Voice::stage_runs`, the same
  answer the allocator bills: a stored DRIVE or FOLD of 0.001 or more, or
  a route that may raise it. So a stage runs exactly 20 ms past the moment
  it is billed off.
- **Delay TIME crossfades two read heads over 20 ms** (`TIME_FADE`, 960
  samples, whole blocks). This is pitch-safe, where gliding the head would
  sweep the pitch. A TIME that keeps moving starts a new fade as each one
  ends.
- **Every effect runs whatever its MIX, which only sets its return.** At
  MIX 0 the return fades out, but the lines and the ring keep taking the
  send. Brought back up, the effect plays what the send is doing now, as a
  send effect on a desk does. This closes #61 and #65.
- **An Algo voice's lifetime (the sweep's D3):**
  - While the key is held, every carrier holds the voice, whichever
    algorithm it belongs to. A MORPH sweep, by hand or by route, never ends
    a held note; this keeps 58b35e9's rule that modulation can't end a
    held Algo note.
  - Once the key is up, only a carrier heard at the MORPH holds it (its
    blended weight above 0), unless MORPH has a route, which may bring the
    other algorithm's carriers back.
  - Before, a carrier only in ALG B kept a released INIT voice at MORPH 0,
    and its bill, for 23 s of silence.
- **Billing (ADR 0056's host method):**
  - The allocator bills what can be held: a route moves a voice's stages
    every block. So `DRIVE_RAMP_COST` and `FOLD_RAMP_COST` are added only
    while a route may move one of that stage's settings: DRIVE, TONE or
    MIX for the drive; FOLD, SYM or MIX for the fold. Unrouted, only the UI
    moves them, briefly, as it moves the bus.
  - A bus setting moves only when the UI moves it (there are no routes to
    it, and MIDI CC is dropped). So `FxBus::COST` bills only the ease work
    it does every block. What an ease costs while a setting moves is brief,
    and sits in the 30 % of headroom the 70 % budget leaves.
  - The worst such case is a Performance load. It moves every Part's five
    gains, a delay TIME, the chorus MODE and every MIX at once: about
    35 × 6 + 50 + 21 ≈ 280 instructions a sample, ~450 cycles, for 20–100
    ms. That is within the headroom (about 2,100 cycles a sample on rev
    V).

## Alternatives considered
- **A one-block ramp for everything:** a full-scale OUT LEVEL jump on
  BANK still clicked (15.5×). 1.3 ms is too short for a UI jump.
- **The one-pole on routed values too:** it low-passes modulation, −3 dB
  at 8 Hz. The UI's value eases and the route rides on top.
- **A smoothed delay read head:** a 20 ms glide over a 490 ms jump plays
  the tape backwards at 24× speed. Slew-limiting it sweeps the pitch for
  up to a second.
- **Clear an effect's buffers when it turns on:** the delay's 96 KB in
  one block is about 375 cycles a sample, over the budget. Spreading the
  clear over blocks still replays whatever isn't cleared yet.
- **Lerp every Part's gains in the fused mix passes whenever one moves:**
  measured at 82 instructions a sample, paid by all six Parts for one
  knob. The same passes with separate sums of starts and steps came to
  98, because register pressure spilled them.

## Consequences
- **Costs by ADR 0056's host method** (thumbv7em release, 1.46 cycles an
  instruction, +10 %):

  | Term | Before | After | From |
  |---|---|---|---|
  | `CHAIN_COST` | 10 | 30 | ADR 0060's blocker 6.5; the stage eases 1.3; the OUT LEVEL and filter DRIVE ramps, about 2 each |
  | `DRIVE_COST` | 57 | 57 | the steady stage |
  | `DRIVE_RAMP_COST` | — | 23 | under a route: the ramp path's 36 instructions a sample, against the steady 22 |
  | `FOLD_COST` | 43 | 45 | the steady stage and the fold's offset, 1 instruction |
  | `FOLD_RAMP_COST` | — | 29 | under a route: the ramp path's 42, against the steady 24 |
  | `FxBus::COST` | 1,160 (1,470 with the tape) | 1,180 (1,490) | the eases' one-poles a block: 6.5 |

- **Not billed (brief, UI-driven), per sample while it moves:**
  - A delay TIME crossfade: 43 instructions, 50 with a MIX ramp.
  - A chorus ease: 21.
  - Each moving Part gain: 7, so all five of one Part's gains 35.
  - The reverb's MIX ramp: none.
- **Voice counts:** ADR 0060 lists them.
- **Always running** costs real CPU at MIX 0 that was idle before. The
  bill doesn't change: the bus was always reserved at its worst, every
  effect on.
- **For a fixed 20 ms after DRIVE or FOLD is billed off** (its gate's
  fade) the stage still runs: at most 80 cycles a voice, within the
  headroom.
- **A hand MORPH sweep can no longer end a held note**, and a released
  voice whose carriers are all unheard ends at once. Raising LEVEL or
  moving MORPH after release doesn't revive it, just as a released
  carrier at LEVEL 0 isn't revived.
- **The first block after boot** lands every setting on its value.

## Sources
`chimera-core/src/dsp/{ease,drive,wavefolder,filter,voice,chorus,delay,ring,fx_bus}.rs`;
`chimera-core/src/instrument.rs` (`MixState`, `glide`);
`param_sweep_test::jumps_never_click`;
`fx_bus_test::an_effect_at_mix_0_never_replays_a_stale_tail`; #53, #61,
#65.
