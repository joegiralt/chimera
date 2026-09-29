# Exclusive State: Design

**Date:** 2026-09-29
**Status:** Draft, awaiting owner review
**Builds on:** main at 937b89f (8 voices, ADR 0040 strings to G1, ADR 0042 voice pitch).
**Supersedes:** ADR 0008 ("Engines are persistent; never constructed in the audio interrupt"), through a new ADR written with the plan.

## Roadmap

Sub-project 1 of Modal 2.

1. **Exclusive state (this spec).** A voice holds only the engine it plays, Modal holds only the model it plays, and strings are stored in 16 bits. The sound does not change.
2. **Modal 2.** Elements-style exciters feed Rings-style resonators, and the #191 fixes land. It gets its own spec, and it spends the memory this one frees.

## Intent

Today every voice carries every engine and every Modal model, whether it plays them or not. A Voice is 35,248 B, and 32,736 B of that is Modal. The pool is 281,984 B against a 286,720 B budget, so Modal 2's exciters have nowhere to go.

This spec makes each voice hold one engine and each Modal voice hold one model, and halves the string delay lines. About 138 KB of D2 comes free. Nothing audible changes except for a 16-bit storage difference below −90 dB.

**Done when:**

- `Voice` holds an `EngineSlot` (Algo or Modal), and `ModalEngine` holds a `ModelSlot` (bank, string, bowed or sympathetic). Neither holds state for anything it isn't playing.
- String delay lines are 16-bit, and every test in § Tests passes.
- The Algo goldens are bit-identical. The Modal goldens are re-recorded once, in the 16-bit task, with the reason given.
- The chip's sizes, the bench's MODAL rows and the stack mark are measured and committed.

## Principles (binding)

- **Types decide what's possible** (ADR 0012). A slot's variant *is* the engine or model. No separate field records which one is active, so there is nothing to disagree with it. Render dispatches by `match` on the slot, so the Algo arm only ever sees an `AlgoEngine`.
- **Functional core, imperative shell.** Pure and host-tested: `SlotKind::of`, the 16-bit store and load, and the exponent step. The voice's switch logic is a thin shell over them.
- **`unsafe` stays in `in_place.rs`,** in its existing pattern, and every block has a `// SAFETY:` comment.
- **The audio thread never allocates or blocks,** and nothing large goes on its stack.

## Measured today (host, 64-bit)

| Type | Bytes | Made of |
|---|---|---|
| `Voice` | 35,248 | `Engines` 33,416 + chain 1,832 |
| `Engines` | 33,416 | `ModalEngine` 32,736 + `AlgoEngine` 676 + sample rate |
| `ModalEngine` | 32,736 | 8 × `KsString` 3,960 (31,680) + 48 `Svf` 960 + 96 of shared state |
| `KsString` | 3,960 | `[f32; 984]` 3,936 + 24 |
| `[Voice; 8]` | 281,984 | budget `VOICE_RAM_BUDGET` 286,720; 4,736 left |

The eight strings are one main string and seven sympathetic strings (`modal/mod.rs:49-64`, `string.rs:12,38`). Sympathetic uses all eight. String and Bowed use the main one. The bank uses the SVFs.

**What follows from this.** Exclusivity alone saves little today: the largest model, Sympathetic, already needs all eight strings, so only the bank and Algo overlap (about 1.6 KB a voice). Most of the memory comes from the 16-bit strings. Exclusivity is what keeps Modal 2 cheap: a new model or exciter adds to the *largest* slot, not to the sum.

## 1. The engine slot

```rust
#[repr(C, u8)]
pub enum EngineSlot { Algo(AlgoEngine), Modal(ModalEngine) }

pub enum SlotKind { Algo, Modal(ResonatorMode) }
impl SlotKind { pub fn of(p: &ParamSnapshot) -> Self }   // pure: engine, then MODE
impl EngineSlot {
    pub fn kind(&self) -> SlotKind;
    pub fn rebuild(&mut self, kind: SlotKind);           // in place
}
```

- `EngineSlot` replaces `Engines` in `dsp/engines.rs`. `Voice.active_engine` goes, because `slot.kind()` is the active engine. `sample_rate` moves into `Voice`.
- `note_on`, `note_off`, `render`, `set_pitch` and `is_active` each become one `match` on the slot, as they are on `Engines` today.
- `Engines::cost` becomes `EngineSlot::cost`, unchanged: it reads only the params.

**Why ADR 0008's risk is handled now.** ADR 0008 rejected an enum because building a variant and moving it into place put 66 KB on the interrupt stack, with no guard. The repo has since grown in-place construction: `in_place.rs` (`uninit_at`, `by_value`, `field_list!`), and every engine has an `init_in_place` that writes its fields through a pointer. A slot is rebuilt where it lives. Nothing the size of an engine is ever a stack value.

**How.** A `#[repr(C, u8)]` enum has a defined layout: a `repr(C)` struct of a `u8` tag and a `repr(C)` union of the variants' fields. `in_place.rs` gains one macro, `in_place_enum!`, which declares the enum with that repr, a private mirror of the layout, and for each variant `V(P)`:

- `unsafe fn rebuild_v(&mut self, init: impl FnOnce(&mut MaybeUninit<P>) -> &mut P)`: writes the tag, then runs `init` on the payload's address;
- `unsafe fn init_v(slot: &mut MaybeUninit<Self>, init: …) -> &mut Self`, for `Voice::init_in_place`.

They carry `by_value`'s `# Safety` contract (`init` writes every field; the in-place constructors do, and `field_list!` guards them). The macro also asserts at compile time that no payload has drop glue, as `engines.rs` does today. Callers in `engines.rs` and `modal/mod.rs` use one `// SAFETY:` line each. There is no other new `unsafe`.

## 2. The model slot

```rust
#[repr(C, u8)]
enum ModelSlot {
    Bank(ModalBank),                   // 48 Svf, CosineOsc, resolution
    String(KsString),                  // the plucked KS+ string
    Bowed(KsString),                   // the bowed string
    Sympathetic(SympatheticStrings),   // main + [KsString; 7] + ratios
}
```

One variant per `ResonatorMode`. The owner's three kinds of state (bank, single string, sympathetic strings) are the payloads. Bowed's string carries its bow force (`BowedString`), so String and Bowed share a string, not a payload type.

| `ResonatorMode` | Variant | State | Host bytes after § 4 |
|---|---|---|---|
| Modal | `Bank` | 48 SVFs, cosine osc, resolution, noise burst | 984 |
| String | `String` | 1 string | 2,000 |
| Bowed | `Bowed` | 1 string, bow force | 2,000 |
| Sympathetic | `Sympathetic` | 8 strings, 7 ratios | 16,032 |

- `ModalEngine` keeps only what every model uses: frequency, pitch, tuned, released, active and the silence counter. `active_mode` goes: the variant is the mode. `sym_ratios` moves into `SympatheticStrings`. The exciter state is its model's: the bank's noise burst (remaining, amp, noise, lowpass) lives in `ModalBank` and Bowed's bow force in `BowedString`, so a rebuild starts them fresh, and a MODE change mid-burst can't leave a burst no model counts down (the voice would never go idle). The bank reports its burst to the silence check.
- `ModalEngine::init_in_place(slot, mode)` builds the shared fields and the one model. `playing()` reads the variant.
- Each `render_*` takes its own payload: `render_sympathetic(&mut SympatheticStrings, …)`. No model can render another's state.
- **Sound is unchanged.** Each model's render and note-on code runs exactly as today, on the same state, just held in a variant. Today a MODE edit keeps the old model's state around, but nothing ever reads it: every note-on writes or clears what its model reads (String's `trigger` fills `[..delay_len]`, Bowed clears its whole buffer, Sympathetic clears the seven strings). The only thing that can differ is a noise seed carried over from an earlier note of another model, because a rebuild starts the seeds fresh. No golden covers that case.
- Bowed stays exactly as it is. It becomes an exciter in Modal 2.

## 3. Switching

**Rule (owner, 2026-09-29).** A switch is a change to the Sound's `SlotKind`: its engine or its Modal MODE. When a Part switches while its voices sound, those voices fade and are rebuilt once they are silent. The new engine or model plays from the next note. Knob moves are not switches.

The machinery already exists. `Voice::render` fades a voice whose Sound changed engine, over `Voice::FADE` = 128 samples (2.7 ms), and `fade_ended` → `reset` already rebuilds the engine in place. This spec widens the check from the engine to the `SlotKind`, and the reset rebuilds into the Sound's kind:

| Where | Today | After |
|---|---|---|
| `render`, sounding voice | `params.engine() != active_engine` → fade (restart if held) | `slot.kind() != SlotKind::of(params)` → the same fade |
| `fade_ended` / `reset` | rebuild the old engine | `slot.rebuild(SlotKind::of(params))`: the one rebuild |
| `trigger`, idle voice | reset the engine left behind | rebuild in place if the kind differs, then play: same block |
| `note_on` on a sounding voice of the old kind | waits for the fade | unchanged |

- **Only that Part.** The check compares each voice with the Sound of the Part it sounds for. Other Parts' voices never see the change. The FX bus is not touched, so its tails ring on.
- **Knob moves never rebuild.** `SlotKind::of` reads exactly two fields, the engine and MODE. Every other param reaches ringing notes every block, as today, and as Rings does: patch params reach every voice each block, and only pitch is latched per voice.
- **What changes for the player.** Today a MODE edit leaves ringing notes on their old model until they end. After this, they fade in 2.7 ms, as an engine change does. `Voice::held_model_extra` (#183) stays: it bills the old model for the ≤ 2 blocks of the fade.
- **Held keys.** A held key restarts its note on the new kind after the fade, as an engine change does today (`AfterFade::Restart`), now for MODE too. See Open questions.
- **A rebuild count.** `Voice` keeps `rebuilds: u16` (wrapping) with a read-only getter, for the tests and the bench.

**Cost of a rebuild.** It writes the new slot: the Sympathetic model is 16,032 B, mostly the zero-fill of eight 1,968 B buffers. That is 4,008 word stores, estimated at 4,000–8,000 cycles on D2 with the D-cache. A block is 640,000 cycles at 480 MHz, so one rebuild is ≤ 1.3 % of a block. The worst case is all eight voices in one block (a chord right after a switch): ≤ 64,000 cycles, 10 %. **This is not new cost.** Today every fade end already rebuilds the whole 32,736 B `ModalEngine` in the interrupt. This halves it. A steal across Parts of different kinds can rebuild twice in its last block (fade end, then the waiting note): ≤ 2 per voice per block. The rebuild is not billed in `Cost`, which is per sample, as today's reset isn't. The bench gets a REBUILD row.

## 4. Sixteen-bit strings

Every `KsString` buffer becomes `[i16; 984]` (3,936 → 1,968 B). Every computation stays `f32`; only storage is 16-bit.

**Fixed-point alone fails.** A host prototype (in the scratchpad, not committed) ran today's string code with f32 storage and with Q1.14 storage, over one second:

- **Round to nearest never goes quiet.** A KS loop loses about 0.1 % a pass. Below roughly 500 LSB (−30 dBFS) rounding hands the loss back, and the string limit-cycles. At A4 with no feedback, f32 goes quiet at 2.24 s; Q1.14 never does, so the voice is never freed.
- **Truncation halves the tails.** The same note goes quiet at 1.07 s.
- **The error misses the gate either way:** −49 to −84 dBFS RMS.

**The design: a block exponent per string.** Each string stores `q = sat16(round(x · 2^e))` and reads `x = q · 2^−e`. It keeps one exponent `e` and the peak |x| written since its write position last wrapped.

- **Write.** Round to nearest, then saturate to `[−32768, 32767]`. A string that overshoots its storage clips; it never wraps. NaN stores 0. Rounding comes first, then Rust's `as i16`, which saturates and maps NaN to 0.
- **Range.** `e` starts every note at 14 (full scale ±2.0). That covers the feedback clamp at ±1.5 and the sympathetic injection, so the note's first period can't clip. `e` is capped at 24, where the silence threshold (0.001) is still 16,777 LSB.
- **Step.** On each wrap (once a period; the whole 984-sample ring for Bowed), a pure `next_exp(peak, e)` decides:
  - peak < ¼ full scale and `e < 24`: `e += 1`, and every sample shifts left one bit. That is exact.
  - peak > ½ full scale and `e > 14`: `e −= 1`, and every sample shifts right with rounding.
  - The signal stays between ¼ and ½ of full scale, a bit of headroom kept. Resolution follows the level down, so the rounding dead band shrinks with the signal and the tail decays as in f32.
- **Spike bound.** At most one string per voice steps per block; another due in the same block waits for its next wrap. Waiting is safe: saturation covers growth. A step is ≤ 984 halfword shifts, about 1,000 cycles. The worst case is 8 per block, about 1.3 % of a block.

**Prototype, block exponent, one second.** Velocity 100, the default Modal params except as listed:

| Case | Error RMS | 10 ms envelope | Quiet (f32 / i16) |
|---|---|---|---|
| String A4, FDBK 0 | −103.7 dBFS | ≤ 0.00 dB | 2.24 s / 2.24 s |
| String A4, FDBK 0, DECAY 0 | −102.4 dBFS | ≤ 0.02 dB | 7.20 s / 7.19 s |
| String G1, FDBK 0, DECAY 0 | −94.8 dBFS | ≤ 0.00 dB | — |
| String C6, FDBK 0 | −108.7 dBFS | ≤ 0.00 dB | 1.19 s / 1.19 s |
| String A2, ensemble on, FDBK 0 | −96.7 dBFS | ≤ 0.00 dB | — |
| Sympathetic A3, FDBK 0 | −94.9 dBFS | ≤ 0.01 dB | — |
| Sympathetic A3, FDBK 0, DECAY 0 | −93.4 dBFS | ≤ 0.01 dB | — |
| Bowed A3, released at 2 s | −90.6 dBFS | ≤ 0.01 dB | 5.98 s / 5.98 s |
| String A4, default (FDBK 0.2) | −79.2 dBFS | ≤ 0.00 dB | never / never |
| Sympathetic A3, default (FDBK 0.2) | −22.5 dBFS | ≤ 1.69 dB | never / never |

The last two rows are the #191 self-oscillation: FDBK 0.2 drives the loop to its ±1.5 clamp. Such a loop amplifies any difference, however small, so comparing sample by sample measures the loop's sensitivity, not the storage. The gate therefore has two parts (§ Tests): sample error with FDBK 0, and envelope and bounds with feedback.

**Cost.** Reads gain a multiply by 2^−e, writes a multiply, a round and a saturate, and each write a max for the peak. The buffers halve, which helps Sympathetic's D-cache misses (174 a block today). `ModalEngine::COST_*` stay as they are. The plan re-benches String and Sympathetic. If either exceeds its billed figure, the plan stops and reports; it doesn't raise the constant on its own.

**Goldens** (ADR 0011). The 16-bit task re-records every golden that plays Modal, once, in its own commit: `modal_init`, `modal_lfo_cutoff`, `algo_to_modal_switch`, and any factory case on Modal. The comment on each reads: "Re-recorded: Modal strings stored as 16-bit block float (exclusive-state spec § 4)." Tasks 1–4 of the plan keep every golden bit-identical. That is the proof that exclusivity changed no sound.

## Memory

Host sizes, from a layout mock of the new types. The plan measures them on `thumbv7em`, where `usize` is 4 B, and commits the figures.

| | Today | After |
|---|---|---|
| `KsString` | 3,960 | 2,000 |
| `ModalEngine` | 32,736 | ≈ 16,080 |
| Engine storage (`Engines` → `EngineSlot`) | 33,416 | ≈ 16,088 |
| `Voice` | 35,248 | ≈ 17,928 |
| `[Voice; 8]` | 281,984 | ≈ 143,424 |
| Left under `VOICE_RAM_BUDGET` (286,720) | 4,736 | ≈ 143,296 |

**Freed:** about 17,320 B a voice and 138,560 B across eight. Of that, about 1,640 B a voice comes from exclusivity (the bank and Algo now overlap Sympathetic) and about 15,680 B from the 16-bit strings.

**What it's for:** headroom for Modal 2's exciters. None of it is spent here. `MAX_VOICES` stays 8 and `MAX_STRING_DELAY` stays 984 (ADR 0040).

**The asserts.** `voice.rs`'s `[Voice; MAX_VOICES] <= VOICE_RAM_BUDGET` stays. Two are added:

- `size_of::<EngineSlot>() <= max(size_of::<AlgoEngine>(), size_of::<ModalEngine>()) + align_of::<EngineSlot>()`, and the same for `ModelSlot` over its payloads. This proves the slots are exclusive: the largest payload plus the tag, never the sum.
- `size_of::<Voice>() <= VOICE_CHAIN_BYTES + size_of::<EngineSlot>()`, with `VOICE_CHAIN_BYTES = 2048`. The voice is its chain plus its largest slot, and the chain can't grow unnoticed.

`memory_budget_test` prints the chain, each payload and the total.

## Tests

| Test | Asserts |
|---|---|
| `switch_never_clicks` | A held Algo note, whose Part switches to Modal. From the switch to the fade's last sample, the largest \|y[n] − y[n−1]\| ≤ S + A / `FADE`, where S is the largest step and A the peak in the block before the switch: the signal's own slew plus the fade's slope. After the fade, the output is bit-identical to a fresh voice playing the new Sound, so any step there is the new note's own onset. The same for a MODE switch (String → Sympathetic). |
| `idle_voice_switches_in_the_same_block` | A voice that went quiet on Algo, then the Sound becomes Modal: the next note-on's first block is bit-identical to a fresh Modal voice's, and `rebuilds` rose by 1. |
| `other_parts_are_untouched_by_a_switch` | Parts 1 and 2 sounding; Part 1 switches engine. Part 2's bus is bit-identical to a run without the switch, block for block, and `FxBus` is never reset: a reverb tail ringing at the switch is still non-zero in the next block. |
| `model_switch_rebuilds_once` | A ringing String note whose MODE becomes Sympathetic: `rebuilds` rises by exactly 1 from the switch through the held note's restart, and the restarted note is bit-identical to a fresh Sympathetic voice's. |
| `knob_moves_never_rebuild` | A ringing Sympathetic note while every Modal param but MODE, and every chain param, moves each block: `rebuilds` is unchanged, the voice never fades, and the output differs from the unmoved run (the knobs reach the ringing note). |
| `strings_i16_match_f32` | `KsString` is generic over its store (`f32`, `Q16`); a unit test in `string.rs` runs both. FDBK 0 over String, Bowed and Sympathetic × G1, A4, C6 × DECAY 0 and 0.3, plus ensemble on: error RMS ≤ −90 dBFS over 1 s, 10 ms envelope within 0.1 dB while above −60 dBFS, and quiet within 20 ms of f32. FDBK 0.2 and 1.0: finite, within ±1.5, envelope within 2 dB. |
| `q16_saturates_and_steps` | Pure: `store` of ±3.0, ±∞ and NaN at `e = 14` gives 32767, −32768 and 0, never a wrapped value; `next_exp` steps up below ¼ full scale, down above ½, and stays inside 14..=24. |
| `slot_layout_matches_repr` | For each variant of both slots, after a rebuild, `match` yields a payload whose address is the mirror's payload address, and the tag reads back. |
| Size asserts | The const asserts in § Memory, repeated in `memory_budget_test` with printed numbers. |
| `rebuild_fits_a_small_stack` | Engine and model switches on a `std::thread` whose stack is `size_of::<ModalEngine>()` bytes: it passes. The plan proves the test can fail: a by-value `*slot = EngineSlot::Modal(ModalEngine::new())` must overflow it. On the chip, the bench runs a switch storm and records the AUDIO page's stack mark (ADR 0025), which may not grow by more than 1 KB over today's. |

The existing goldens, sanity, instrument and cost tests pass unchanged, apart from the Modal re-record in § 4.

## Interactions with in-flight branches

This project touches `dsp/engines.rs`, `dsp/voice.rs`, `dsp/modal/{mod,string}.rs` and `in_place.rs`, plus tests. `Voice`'s public API (`note_on`, `note_off`, `kill`, `render`, `is_active`, `cost`, `held_model_extra`) keeps its signatures, so `instrument.rs` is untouched.

- **`gain-staging`** adds the output limiter in `fx_bus.rs`, `limiter.rs` and `instrument.rs`. No shared files.
- **`algo-init-morph`** changes the Algo INIT and the carrier norm in `algo/engine.rs`. This project only moves `AlgoEngine` into a variant. Both re-record rows in `golden_test.rs`; `algo_to_modal_switch` covers both engines. Whichever lands second rebases and re-records that row.
- **`projects`** adds `DiskCode` to `ResonatorMode` in `modal/params.rs`. This project doesn't touch `params.rs`, and `ResonatorMode` keeps its four variants and codes. A project load kills every voice (projects spec, step 4); each fade end rebuilds its slot into the Part's new kind, down the path in § 3. No new code. Loading a Sound into a Part is a switch as in § 3.

## Out of scope

Modal 2: the exciters, the model pages, and the #191 behaviours (String feedback self-oscillation, Bowed reading no page params, no dimming, the inaudible ensemble). Also the cost model, the voice count, the UI and the string length.

## Plan order

Each task leaves the tree green.

1. `in_place_enum!` and `slot_layout_matches_repr`.
2. `EngineSlot` replaces `Engines`. Goldens bit-identical.
3. `ModelSlot` in `ModalEngine`. Goldens bit-identical.
4. Switching on `SlotKind`, the rebuild count, and the switch tests. Goldens bit-identical.
5. 16-bit block-float strings, `strings_i16_match_f32` and `q16_saturates_and_steps`, and the one Modal golden re-record.
6. Chip: sizes, bench rows (MODAL, SYMPATHETIC, REBUILD, exponent step), stack mark. The ADRs are written here: one superseding 0008 (a voice holds one engine, rebuilt in place), and one for the 16-bit block-float strings. Both are added to `docs/adr/README.md`.

## Decisions made (owner, 2026-09-29)

- Exclusive state is sub-project 1 of Modal 2. Modal 2 follows in its own spec.
- A voice holds only its active engine, in an exclusive slot rebuilt in place. This supersedes ADR 0008.
- Modal holds only its active model's state (bank, single string, or sympathetic strings). Bowed stays as it is here and becomes an exciter in Modal 2.
- An engine or MODE change fades that Part's sounding voices over about 3 ms (the existing 128-sample fade). Each slot is rebuilt once silent, and the new kind plays from the next note; a held key restarts on it. An idle voice switches at once. Other Parts and the FX tails are untouched.
- Knob moves never rebuild; they reshape ringing notes, as today and as in Rings.
- String and sympathetic delay lines are stored as i16, computed in f32, saturating at write, and tested against f32 at −90 dB.
- Engine sound, the cost model, 8 voices, the UI and the #191 behaviours don't change here.
- The freed memory is headroom for Modal 2's exciters and is not spent here.
- The 16-bit strings use a per-string block exponent (§ 4), with the gate defined as RMS error re full scale over one second with FDBK 0, plus envelope and bounds checks for self-oscillating feedback. Chosen over f32 strings, which would free only ~13 KB.
- A held key restarts its note on the new engine or model after the fade, as engine changes already do.

## Open questions

None.
