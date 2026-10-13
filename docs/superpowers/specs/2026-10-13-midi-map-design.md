# Per-Part MIDI map with MIDI learn (alpha): Design

- **Status:** Approved 2026-10-13 (owner: go with your recommendations)
- **Date:** 2026-10-13
- **Issue:** #247 (per-Part MIDI map). Related: #440 (USB MIDI), #43 (MPE, program change), #460 (bank 2), #337 (memory budget)
- **Builds on:** ADR 0017 (prime status), 0044 and 0081 (the keys), 0045 and 0087 (records, neutral omitted), 0083 (ORBIT edits), 0086 (hints), 0102 (record ceiling), 0112, 0115 and 0116 (knob glide and deltas), 0123 (sends in the Sound), 0128 (bend and sustain per Part, every other CC the map's)
- **Review:** (private notes) (against 18a8f7e7). This spec is checked against `origin/main` d708d591; § 13 lists what the review got wrong.

## Owner decisions (2026-10-13)

The owner approved all eleven of the review's recommendations as written:

1. **The map lives in the Sound,** with a shared default map in INIT and the factory Sounds, so preset browsing keeps the basics.
2. **Learn is MIX+SEQ on the focused cell.** While armed, a turn moves the focus and never the value. "Hold a cell's button" is dropped: B*n* acts on press outside ORBIT and is ORBIT's cross inside it.
3. **A default map ships as data written into each Sound,** not as a global chart: CC 74 CUTOFF, 71 RESO, 7/10 Part LEVEL/PAN, 91/93 the reverb/chorus SEND, 1 → LFO1 DEPTH, and 16–19 the engine's four macros.
4. **16 mappings a Sound.**
5. **Targets are Sound params plus Part LEVEL and PAN.** No FX, ORBIT or settings for alpha. The reverb spec's MIDI MAP line is superseded.
6. **A controller that disagrees with the stored value jumps (glided), as Elektron does.** No pickup.
7. **Smoothing is a UI-side ramp over the CC inter-arrival time, 15–120 ms.** 14-bit is deferred. Confirm by ear on a resonant CC 74 sweep.
8. **UI-loop latency is accepted for alpha.** The audio-side p-lock path is filed post-alpha.
9. **CC 1 stays a map entry,** with no mod-wheel source, for alpha. Revisit with MPE (#43).
10. **CC 120/121/123 are fixed now, as a separate alpha issue.** That work is in progress on branch `all-notes-off`, outside this spec.
11. **Flash: ~5–8 KB is approved.** #460 (bank 2) is the next lever.

**The owner's two rulings on § 14 (2026-10-13, with the plan's approval):**

1. **Mod-wheel vibrato out of the box.** INIT sets LFO1 DEPTH to 0 and routes LFO1 → PITCH at amount 2 (±0.38 semitone at DEPTH 1). CC 1 → LFO1 DEPTH brings the vibrato in from 0. See § 9.
2. **MIDI MAP lives at SETTINGS › PART › MIDI MAP** (§ 8, confirmed).

**Record tag (correction in the ruling):** the map's Sound record is **`MidiMap`, `0x001E`**, not critical. `0x001C` is `Build` on main. `0x001D` is `Note` on `showcase-library` (`chimera-engine/src/storage/record.rs:104`). No other branch or worktree defines a tag past `0x001C` (checked 2026-10-13 across every local and remote branch and the seven worktrees). `0x0C`–`0x0F` stay reserved for the project's own records (library spec line 60).

## 1. Transport: a CC ring per source

CCs never ride the note queue. The note queue is 64 slots with 8 reserved for note-offs (`chimera-engine/src/note_queue.rs:36-40,276-292`). It is held through a project load's card reads (ADR 0099), and bend already shares it (ADR 0128). A turning knob there would drop note-ons. A mapped CC also edits the Sound, which only the UI may do.

- **The route is a pure function in `chimera-engine`:** `midi_in::route(msg) -> Routed`, where `Routed` is `Queue(InEvent)`, `Cc(CcEvent)` or `Drop`.
  - `Queue` is `InEvent::from_midi`'s set, unchanged: notes, bend, CC 64 and real-time, plus 120/121/123 once `all-notes-off` lands.
  - `Cc` is every other CC.
  - Both shells' ingress sites call it: `chimera-stm32/src/midi_din.rs:70` and `chimera-desktop/src/audio.rs:346`.
- **`CcEvent { channel: MidiChannel, cc: Cc, value: u7 }`** packs into a `u32`. `Cc` is a newtype over 0..=127.
- **`CcRing`** is an SPSC ring of 64 × `AtomicU32` (256 B), with the same orderings as `NoteQueue::push`/`pop`. A full ring drops the newest and counts it in `dropped()`. There is no reserve: every CC is equal.
- **One ring per source:** DIN today. USB gets its own when #440 lands. A desktop MIDI port is its one source.
- **The UI drains every ring once per loop iteration,** before `handle_input`: `UiState::hear_ccs(&mut self, ccs: impl Iterator<Item = CcEvent>, now: Ms)`. The UI loop spins unthrottled (`chimera-board/src/lib.rs:548`), so latency is one iteration. A full-page redraw or card work stalls it.
- **Coalescing is latest-wins per mapped entry.** A full ring loses intermediate values, never the resting one, unless the last message is the one dropped. CCs lost during a card stall are accepted: a load replaces the Sound anyway.

## 2. Data model: 16 entries in the Sound

```rust
pub struct MapEntry { pub cc: Cc, pub target: Mappable, pub min: u8, pub max: u8 }
pub struct MidiMap { entries: [MapEntry; 16], len: u8 }   // Sound::map
```

- **`min` and `max`** are places 0..=255 of the param's normalized span (`ParamSpec::normalize`), so CUTOFF's are octaves. `min > max` inverts. Defaults are 0 and 255.
- **Uniqueness:** a param appears at most once, so re-learning a param replaces its CC. A CC may drive several params (a macro).
- **Several Parts on one channel each apply their own map:** layering, as ADR 0128's per-channel bend.
- **`Sound` gains `pub map: MidiMap`.** `MidiMap::EMPTY` is the neutral value, so `Sound::neutral` and decode bases are empty. `bits_eq` and `sound_crc` include it.

**The record, `MidiMap` `0x001E`:**
- **Format:** 5 B an entry: `cc`, the block's disk code, the param id, `min`, `max`. This follows `TapRoutes`' 4 B form (`storage/sound.rs:149-158`).
- **Writing:** written last by `encode_sound`, after `ZoneTune`, and only when the map is non-empty. **Neutral = empty = omitted** (ADR 0045/0087). The default map is written explicitly, so "no record" never means "default".
- **Decode:**
  - A length that is not a multiple of 5, or over 80 B, is `Bounds`.
  - An entry is dropped and counted as skipped when its address isn't in this firmware (`addr_of`) or isn't `Mappable`, or when its CC is reserved (§ 4) or over 127. That reads as `LOADED · SOME PARAMS SKIPPED`.
  - A duplicate target keeps the last.
- **Once per Sound:** `SoundCheck::seen` is a `u8` with all eight bits taken (`storage/sound.rs:177-187`), so it widens to `u16` and `MidiMap` takes `1 << 8`.
- **Contexts:** it rides every Sound context (library file, a project's `Slot` and `Part`) through `SoundCheck` with no project code of its own.
- **Older builds skip it** as unknown non-critical (`codec_compat_test.rs:662`).
- **Cost:** 4 + 80 B at most, against ADR 0102's 640 B. `MAX_SOUND_COST` goes 2,560 → 2,640 (2,549 + 84 = 2,633, rounded up to 16). `sparse_bound` pins it, and it feeds the arena meter (ADR 0087).

## 3. `Mappable`: the typed whitelist

`Mappable` is a `ParamAddr` newtype. Its only constructor is `Mappable::new(a: ParamAddr) -> Option<Mappable>`. It accepts:

- `a.block.in_sound()` params (`addr.rs:170`) whose spec is not `patching` (FM3 IN, QM IN) and not `live`;
- `ParamAddr(BlockRef::Part, PartParams::LEVEL | PartParams::PAN)`, since CC 7 and 10 are what players expect.

It refuses everything else: CH, MODE, OUTPUT, BEND, SNAP and HELD on the Part, `PartMix(_)`, every FX block, ORBIT (`OrbitChords`, `OrbitRing`, `Tempo`), `Theme` and `Midi`. So an ORBIT address can never reach `UiBlocks::block_mut`'s `debug_assert!(false)` (`chimera-core/src/ui/mod.rs:3318`).

**Engine switch:** a Sound's map may name `Modal`, `Va`, `Algo` or `AlgoOp` params that the Part's current engine doesn't read.
- Those entries are kept and shown dimmed on MIDI MAP. They come back if the engine returns.
- A CC to one does nothing.
- Never refused on load (the no-legacy note: such files need only "load safely").

## 4. Learn: MIX+SEQ on the focused cell

**Keys:**
- **MIX+SEQ** (on SEQ's press while MIX is down) arms learn on the **focused cell** of the active Part's page. The focused cell is `FocusMemory`'s slot, resolved by `slot_addr`, exactly as MIX+PLUS primes a route (ADR 0017).
- **SEQ's gate is muted under MIX** (`chimera-core/src/ui/hold.rs:114`: `false` → `mix`, as MENU's at `:113`), so MIX+SEQ fires no SEQ tap and no SEQ hold. The ORBIT side effect: MIX+SEQ no longer saves a scene there, and does nothing.
- **Armed:** a turn of any encoder moves the focus to that cell and **never changes its value**. Any visible cell can be targeted.
- **Confirm:** the first CC from any source on the active Part's channel. Its value is **not applied**, so nothing jumps. The new entry's ramp starts at the stored value.
- **Cancel:** MIX+SEQ again, any nav key, or a page or Part change. No timer (ADR 0017).
- **Learn is never reachable in ORBIT** (`orbit_keys` has no learn key) **nor in SETTINGS.**

**Focus-band text while armed and after.** On BigViz pages it goes on the viz band's top line, as ADR 0017 does.

| State | Text | Learn after |
|---|---|---|
| Armed on a mappable cell | `MOVE A KNOB · CUTOFF` | armed |
| Armed on a cell that isn't `Mappable` | `CUTOFF NOT MAPPABLE` (a CC is ignored) | armed |
| Mapped | `CC 74 → CUTOFF` | off |
| Wrong channel | `CC 3 ON CH 2, PART ON CH 1` | armed |
| Reserved CC | `CC 120 RESERVED` | armed |
| 16 entries, the cell's param not among them | `MAP FULL` | off |

- **Labels** are the cell's full-word label (the full-words note), and a TYPE-dependent label where a slot has one (ADR 0085).
- **Reserved CCs**, refused by learn and by decode:
  - 0 and 32: bank select (#43);
  - 6, 38 and 96–101: RPN/NRPN, where RPN 0 is the standard bend range;
  - 64: sustain;
  - 120–127: channel mode.
- **Hint (ADR 0086):** every mappable cell's hint gains `MIX + SEQ  LEARNS A CC`. The hint-width test covers it.
- **Edits:** a learn is a Sound edit. It dirties the Part (`*`), and Revert and Save carry it.

## 5. Continuous targets: jump, then the inter-arrival ramp

- **Absolute 7-bit, jump:** value `v` maps to place `min + (max − min) · v / 127`, then to the value through the spec's `denormalize`. No pickup, no relative modes.
- **Jitter (review L3):**
  - A CC equal to the entry's last value is ignored.
  - A ±1 reversal within 50 ms of the last change is a dead band.
- **The UI ramp (review H3).** One CC step on CUTOFF is 10/127 ≈ 0.079 octave (`dsp/filter/mod.rs:324`), 1.9× an encoder detent (`params.rs:113-127`). The 30 ms knob glide (`block.rs:184,207`) alone would make a slow sweep a staircase. Each mapped entry therefore keeps a `Ramp { at: f32, to: f32, per_ms: f32, last: Ms, last_v: u8 }`.
  - A new CC sets `to`, and a rate that crosses `|to − at|` in `T` ms. `T` is the measured inter-arrival `dt`, clamped to 15–120 ms. After silence (`dt` > 120 ms, a p-lock) `T` is the 15 ms floor.
  - Each UI iteration advances `at` by `per_ms × elapsed` (from the controls' ms clock, not frames) and writes it through the edit path (§ 6). The existing 30 ms glide smooths the remaining sub-steps of at most ~0.02 octave.
- **The hand wins:** an encoder turn on a param cancels its ramp (`at = to =` the new stored value).
- **Ramps reset when the Part's Sound is replaced:** its `Loads` count moves (a load, NEW, Revert, CLEAR).
- **The MOVES cap (review H2).**
  - `MOVES = 6` (`dsp/knobs.rs:378-382`). A publish of more knob changes than that overflows (`:408-411`, `:517`), and the audio lands every knob (`:738-739`).
  - So the CC pass writes **at most `MOVES − 1` = 5 knob params a Part a pass**, leaving one for a hand turn in the same iteration.
  - Only targets in `KNOBS` count. Part LEVEL/PAN and the SENDs are `Smoothing::Own`, and choices are stepped.
  - Entries over budget keep their ramp and are served first next pass (a round-robin cursor per Part), so none starves and nothing lands.
- **Bill (review M6):** `Knobs::COST` is already 6 cycles a sample a sounding Part. A STRESS `CC` row (8 knobs on every Part every pass) is added at ship, with no new bill until the unit says so.

## 6. Stepped targets: binned with hysteresis through the edit path

- **Binning:** a `Motion::Stepped` target divides its mapped range into its `N` steps' equal bins. It moves only when `v` is 2 counts past the boundary of the bin it's in.
- **Edit path:** the new bin is reached as a signed step **through the UI edit path**, the same function an encoder turn calls: `nudge`/`snap`, `machine_edit`, and Algo's `step_kind`/`step_alg` with their formant-budget refusal (`chimera-core/src/ui/mod.rs:1415-1435`, `part_page.rs:85-97`). That function is extracted from `apply_under`/`snap_under`/the encoder handler as `UiState::edit_addr(part, a, delta, snap)`, so budget refusals and crossfades hold.
  - A refusal shows the existing `FORMANT FULL` error only when the active Part is the target's Part.
- **`NoteOn` params** take effect on the next note, as a turn does.
- **Not ramped:** stepped targets bypass the ramp and the MOVES budget.

## 7. Channel

- **Per Part, no omni:** a CC applies to every Part whose `PartParams::channel` equals its channel. Learn takes CCs only on the active Part's channel.
- **ORBIT:** CCs apply on every page, ORBIT included. Only learn is unreachable there.

## 8. The MIDI MAP page

- **Place:** a row **`MIDI MAP`** in SETTINGS › PART (the active Part's list, `ui/settings/tree.rs:108`), opening a `Screen` that the SETTINGS list renderer draws (`ui/settings/view.rs`: rows, bar, `RowLook`). No new page type (review M4).
  - ADR 0044's "Part rung" is reached as SETTINGS › PART, because ADR 0081 made SEQ "up" on every chain.
- **Rows:** `CC 74   CUTOFF   0–255`, in learn order. A dead entry (§ 3) is dimmed: `CC 16  — STRUCTURE`. With no rows: `NO CCS · MIX+SEQ ON A CELL`.
- **Keys:**
  - A moves the row (the bar). B sets the CC, skipping reserved CCs; a CC may repeat. C sets MIN and D sets MAX, in places 0..255 shown as `0–255`.
  - MIX+MINUS deletes the row, behind the shared confirm.
  - The page's hint line names them (ADR 0086).
- **Edits** go through `UiState`'s Sound edit, so `*` and Revert work.

## 9. The default map

`MidiMap::default_for(engine) -> MidiMap` holds the following. Every entry is full range (0–255). Built-in `Sound::init` and factory Sounds carry it explicitly.

| CC | Target |
|---|---|
| 74 | Filter CUTOFF |
| 71 | Filter RESONANCE |
| 7 | Part LEVEL |
| 10 | Part PAN |
| 91 | Out SEND3 (reverb, `FxBus` order) |
| 93 | Out SEND1 (chorus) |
| 1 | LFO1 DEPTH |
| 16–19, ALGO | MORPH, VOWEL, DRIVE, FOLD |
| 16–19, MODAL | STRUCTURE, BRIGHT, DAMP, POS |
| 16–19, VA | AMOUNT, SHAPE1, SHAPE2, SUB |
| 16–19, TAPE | none (COMING SOON, ADR 0132) |

- **Size:** 11 entries, a 59 B record.
- **CC 1 → LFO1 DEPTH: mod-wheel vibrato out of the box** (owner ruling, 2026-10-13, superseding the "routed at 0" form).
  - INIT sets **LFO1 DEPTH = 0** and primes `ParamAddr(Pitch, PitchParams::PITCH)` as a registry destination with **LFO1 → PITCH at amount 2**. PITCH's offset law is `Semitones(24)`, so amount 2 is 2/127 × 24 ≈ **±0.38 semitone** at DEPTH 1 (amount 1 would be ±0.19). The wheel at 0 gives no vibrato; full wheel gives a classic vibrato.
  - **What changes for INIT's LFO1 → FREQ (CUTOFF) route:** it stays at amount 0, so at rest nothing changes. But DEPTH scales every LFO1 route, so a player who later dials LFO1 → CUTOFF (or any LFO1 route) hears it only as far as DEPTH (the wheel) is up. LFO1 is the wheel's LFO on INIT; LFO2 and LFO3 keep DEPTH 1 for free-running modulation.
  - **At rest INIT sounds the same.** The LFO's output is `raw × DEPTH` = 0 (`dsp/lfo.rs:531`), every LFO1 route's sum is ±0.0, and the voice skips an offset of 0 (`dsp/voice.rs:1019`). INIT's render with the wheel at 0 is bit-identical to before (Task 8 pins it).
  - The factory Sounds keep their own LFO1 DEPTH and routes; they get only the map, so the wheel jumps DEPTH on first touch (§ 5).
- **Factory Sounds on main** are the 12 built-ins in `factory.bin` (`chimera-engine/src/factory.rs:15`: 8 ALGO and the 4 formant demos), plus INIT per playable engine. Each carries the default map. `GOLDEN_RECORD=1` regenerates the blob.
- **Factory Sounds on `showcase-library`** are TOML Sources built by the `factory-sounds` tool: 15 public on the card, plus the private TX81Z ports. After both branches merge, the tool writes `default_for(engine)` into every Source it builds unless the Source states its own `map`. The ports, built by the same tool, get it too. See the plan's coordination task.

## 10. Budgets

- **Flash:** 5–8 KB in all, against the review's ~37 KB free on ORBIT. That is under the flash-counting note's ~40 KB escalation line, which the owner has accepted. #460 (bank 2) is next. Roughly:
  - the ring and route, ~0.5 KB;
  - map, codec and `Mappable`, ~1.5 KB;
  - learn and ramp, ~2 KB;
  - MIDI MAP, ~1.5 KB, reusing the list renderer;
  - default maps in `factory.bin`, ~15 × 63 B ≈ 0.9 KB.
- **Reporting:** a one-line delta, before and after (`just firmware-orbit`).
- **RAM, all UI-side, none in `AudioShared`** (`PartAudio` holds params and mod state only, `instrument.rs:79-92`):
  - `MidiMap` ≈ 98 B a Sound: six Parts plus the pool's scratch Sound and any other copy (`memory_budget_test` pins it);
  - ramps 6 × 16 × 16 B ≈ 1.5 KB;
  - one ring, 256 B, plus 256 B for USB later;
  - learn state, ~8 B.
  - Total ≈ 2.5 KB.

## 11. Tests

| Test | Pins |
|---|---|
| `route_splits_ccs` | 64 and real-time go to `Queue`; CC 74 goes to `Cc`; nothing else is lost |
| `cc_flood_never_drops_note_ons` | 10,000 CCs with 32 note-ons interleaved: every note-on is queued; the CC ring counts its drops |
| `ring_keeps_the_last_value` | A full ring, then a final CC: the applied value is the final one |
| `map_round_trips` | 16 entries, inverted and partial ranges, through `encode_sound`/decode, a library file and a project Part |
| `empty_map_writes_no_record` | INIT-neutral Sound bytes are unchanged (`neutral_v1.txt` holds) |
| `map_record_bounds` | Lengths 4, 81 and 85 are `Bounds`; an unknown address, an unmappable target or a reserved CC is a skipped entry |
| `mappable_whitelist` | Every `BlockRef` × spec: accepted iff the § 3 rule holds; FM3 IN, QM IN, CH, BEND, reverb SIZE and ORBIT are refused |
| `cc_sweep_never_snaps` | 8 mapped CC knobs swept together on one Part for 2 s: every publish has ≤ `MOVES` moves (no overflow); the voice's step is bounded as in `param_motion_test` |
| `slow_cc74_step_bound` | CC 74 0→127 at a step per 80 ms on a resonant saw: the per-block CUTOFF step at the voice is ≤ 0.03 octave |
| `p_lock_crosses_in_floor` | A single CC after 1 s of silence reaches its target within 15 ms + 30 ms glide |
| `stepped_cc_hysteresis` | A filter TYPE map: 63/64/63/64 around a boundary moves once; a WAVE beyond the formant budget is refused as by the encoder |
| `dead_entries_after_engine_switch` | A Modal map on a Part switched to ALGO: entries kept, applied to nothing, back on MODAL |
| `learn_refusals` | Every row of § 4's table; MIX+SEQ fires no SEQ tap; armed turns leave values unchanged; any nav key cancels |
| `default_map_on_init_and_factory` | `Sound::init(e)` and every `factory_sound(i)` carry `default_for(engine)` |
| hint and page tests | the MIX+SEQ hint fits; MIDI MAP's rows render (screenshot fixtures) |

**Ears-on QA (QA before UAT):** a resonant saw, a slow CC 74 sweep over 4 octaves, against the encoder.

## 12. Post-alpha (filed, not built)

- NRPN or 14-bit CC pairs (static precision only).
- Relative encoders, as a per-entry flag.
- Pickup and scale modes.
- FX and ORBIT targets.
- An audio-side, sample-accurate CC path for sequencer p-locks (review H5): each Part's map published as `(cc → knob, min, max)`, ~1 KB AXI.
- Mod wheel and aftertouch sources (with MPE, #43; a stock-parity gap).
- Program change and bank select (#43).
- MIDI out and CC feedback.
- `KEEP MAP` on load (MIX+PLUS).
- The ISR-side latch, 16 ch × 128 × `u8` + a dirty bitmap (~2.3 KB), if USB floods show losses.

## 13. Corrections to the review (checked on d708d591)

Every other file:line the review cites matches main. These did not:

1. **`ui/settings/listing.rs` is the projects listing, not a list renderer.** The SETTINGS list look is `ui/settings/view.rs` over `tree.rs`'s static rows. MIDI MAP reuses those.
2. **H5's "animation runs at 20 fps" is not the CC latency.** The loop spins unthrottled. `handle_input` and the publish run every iteration (`chimera-board/src/lib.rs:518,554`); only animation keeps `UI_FPS` (`animation.rs:125`). Latency is one iteration, and stalls are redraws and card work. The ramp therefore runs on the ms clock.
3. **`MAX_SOUND_COST` is 2,560, not 2,112** (`storage/sound.rs:34`). With the map it is 2,640.
4. **`SoundCheck::seen` is a full `u8`.** The map's once-bit needs `u16`.
5. **M3's vibrato default doesn't work as written.**
   - INIT's LFO1 route goes to the fixed FREQ column, which is **filter CUTOFF**, not pitch (`modulation.rs:41-43`, `preset.rs:38-40`).
   - LFO DEPTH defaults to 1.0 (`dsp/lfo.rs:172-183`).
   - A route at 0 gives no vibrato at any DEPTH.
   - **Resolved by the owner (2026-10-13):** INIT's LFO1 DEPTH is 0 and LFO1 → PITCH is at amount 2, so the wheel plays vibrato from 0 (§ 9).
6. **"The 32 factory Sounds" is out of date.** Main's NEW holds 12 built-ins plus the INITs. The showcase set lives on the card (§ 9).
7. **`knobs.rs:737` is `:738-739`.** The hold-gate citation is `hold.rs:113-114`, not `:99-111`.
8. **M2 (CC 120/121/123) is already in progress** on `all-notes-off` (worktree `chimera-notesoff`, uncommitted). It edits `InEvent::from_midi`, so § 1's `route` is built on top of it, whichever lands second rebases.
9. **More superseded text than the review names:**
   - the reverb spec's lines 753 and 1077;
   - ADR 0085's MIDI MAP bullet (line 219);
   - product architecture § 10.3's default CC chart (`docs/chimera-product-architecture.md:772`).
   - The new ADR supersedes those in part.
   - `reverb_panel::label` stays: it is used by the panel.

## 14. Resolved by the owner (2026-10-13)

- **Vibrato on the wheel** (§ 13 item 5): out of the box. INIT's LFO1 DEPTH is 0 and LFO1 → PITCH is at amount 2 (§ 9). The cost the owner accepted: any new LFO1 route on INIT is silent until DEPTH or the wheel is raised.
- **MIDI MAP's place** (§ 8): SETTINGS › PART › MIDI MAP, confirmed.
