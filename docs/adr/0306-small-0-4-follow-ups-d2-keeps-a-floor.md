# 0306. Small 0.4 follow-ups: D2 keeps a floor, KEY reads NEEDS UPDATE, the kit's guard re-reads the engine

- **Status:** Accepted (2026-11-21)
- **Deciders:** firmware (the `small-04b` batch: #1053, #1055, #1057, #1059, #1060, #1063), for the owner's review

## Context
Six 0.4 follow-up tickets share one branch (owner's rule: small tickets are batched). Four decide something later work has to keep:

1. **#1063.** `bench-algo`'s link had 16 B of RAM_D2 left (288 KB). It was kept linking by trimming the bench report's text (`BENCH_TEXT_LEN`) seven times, 6,016 B to 5,152 B (ADR 0217, 0252, 0292, 0293, 0294 twice, 0289 with 0305). ADR 0296 had already found a page id more overflowed it. The bench-only D2 residents are `card_speed::HIST` (10,004 B) and `card_dma::bench::BEDS` (8,544 B). Measured at 0.4 26465c7aa (`CHIMERA_STAMP_CLEAN=1`, release): AXI on the bench sets with the card bench (ALGO, ROUTING, MEMORY, STRESS) keeps about 38 KB above `__sheap`, 30 KB over its 8 KB bench margin (ADR 0087).
2. **#1057.** With bank 2's door closed every KEY plays FULL (ADR 0303), and MST's curve label says so, but KEY's cell showed the stored key (LOW, PART 3, ...). The delay's TYPE reads `NEEDS UPDATE` there (`bank2::closed_text`, bank 1's).
3. **#1060.** A kit's library mark (`KitMark`, ADR 0305) outlives the file it came from; the kit's question trusts the listing's engine, and the read never re-checked it; and a kit question dropped by a Part key left SAVE PART FIRST's Stale Parts question waiting, to pop after a later, unrelated load.

4. **#1055.** `ed describe` said `cont` for every number, so the editor could not tell a param the box stores only on its steps (`ParamKind::Stepped`, `ParamSpec::quantize`) from one it stores anywhere (`Continuous`, which a turn moves by `step` from wherever it is). A fader drag stored DELAY's note between two notes (0.55).

#1053 (a test race) and #1059 (protocol 4's minor findings) decide little and are noted under Consequences.

## Decision

### `describe` says which numbers are stepped; the editor snaps only those (#1055)
- A `param` line's kind is `stepped` where the box's `quantize` rounds the value (`ParamKind::Stepped` and an unnamed `Enum`), `cont` where nothing does (`Continuous`); a named one stays `choice` or `toggle`. The editor parses it into its own `Kind` (`Cont | Stepped | Choice | Toggle`).
- The editor snaps a `Stepped` param to `round(v / step) · step` (half away from zero, as `roundf`), counted from zero, not from its bottom, then clamped to its range, exactly as `quantize` stores it. A `Cont` param is never snapped: LFO RATE reaches 0.01 Hz and a 0..1 level lands on 0.8. A wheel notch on a stepped param goes to the next step that way and never past it (from an off-step top, the step under it); on a continuous one, a notch of its travel.
- DELAY's NOTE becomes a stepped spec (step 1/10, its own glide unchanged): the box's turns stored `i / 10` already, and now the editor's and `ed set`'s numbers land on a note too. Its NRPN law moves from `Place14` to `Index7` (same number 21/13, `nrpn_v1.txt`, the guide's chart regenerated): the owner's ruling (2026-11-21) is that **an entry no release carried is not yet frozen**. NOTE was added on 2026-11-20 (#959, 253637233) and no tag holds it; ADR 0216's freeze protects shipped entries. A project stored with NOTE between two notes loads to the nearest (the decoder's `set` quantizes), and the engine still reads it through `DelayNote::at`.
- The schema's bytes change (`stepped`), so its hash does: the editor's cache misses once and reads it again (ADR 0289).

### D2 keeps a floor (#1063)
- `card_speed::HIST` moves from D2 to AXI's `.uninit` (NOLOAD, built in place by `take` as before). It is the CPU's alone (no DMA reads it), so cached AXI suits it. `BEDS` stays in D2.
- `BENCH_TEXT_LEN` goes back to 6,144 B for every bench set but ROUTING's 12,288: ALGO's branch is gone.
- The link asserts a floor of free D2 after `.ram_d2` (`d2_floor.x`, written by `chimera-stm32/build.rs` beside the AXI margin): **4 KiB** in a bench build, **1 KiB** in any other. A build under it fails to link with `D2: under the floor (ADR 0306)`. `just check` links every set, so it holds them all.
- A growth that trips the floor is a decision (move a bench buffer, a UI block, or lower the floor in a new ADR), never another trim of the report.

Measured after (release, clean stamp): `bench-algo` D2 free 9,032 B (was 16), AXI 22,136 B above `__sheap` (8 KB margin); ROUTING, MEMORY and STRESS each gain the same 10,004 B; ORBIT, TAPE and the master tape's bench sets carry no card bench and are unchanged (28,416, 28,416 and 18,568 B free). Shipping (`default`, `orbit`, `bank2-gate`): 2,928 B free, over its 1 KiB floor by 1,904 B.

### KEY reads NEEDS UPDATE while it plays FULL (#1057)
`bank2::closed_text` answers `NEEDS UPDATE` for the compressor's KEY at any key but FULL while bank 2 isn't open, as the delay's TYPE does: the cell, the focus band and `ed`'s text alike. FULL plays from bank 1 and reads FULL. FULL is code 0 (a const assert), so the test is the display value above 0. This is closed-door data in bank 1 under ADR 0198's exception: `closed_text` +44 B.

### The kit's guard re-reads the engine; a dropped question drops what waits on it (#1060)
- `library::recheck`, which every library read and description peek runs, refuses a file whose re-peeked engine differs from the listing's as `FILE CHANGED` (`LibNote::Changed`), as it refuses a moved generation or name. The kit's question was asked of the listed engine, so a kit no question weighed never lands. Unreachable on the unit today (a put always writes a new generation); bank 1, about 28 B.
- `UiState::dropped`: a prompt dropped unanswered (a Part key, learn's jump to a page) clears SAVE PART FIRST's waiting Stale Parts question with it. Answered prompts still run `closed` alone, so a CANCEL of the kit's question still asks it (`lib_ended`). Inlined at the drop sites.
- **A mark outlives its file: noted, not checked.** `KitMark` holds the kit's CRC, not the file it came from, so a check needs a `LibId` per Part (bytes in the project, whose D2 is what this ADR rations) and a card read inside the guard, which is synchronous. The user deletes the file themselves (console `rm` or a put over it); the load then replaces a kit that no file holds, without the KIT NOT IN LIBRARY question. Accepted as the cost; a library save of the Part writes the mark again.

## Alternatives considered
- **Gate the card bench out of `bench-algo`** (the issue's other example): it is the one bench set that runs ALGO's patches beside the card; moving its buffer keeps the bench whole for no RAM the set needs.
- **Move `BEDS` too:** 8.5 KB more D2 for the same AXI; not needed for the floor, and AXI's bench margin is the next region to ration.
- **Lower the bench floor to 2 KiB:** five of the seven trims were under 400 B each; 4 KiB lets the next several pass and still stops a large one.
- **KEY's cell shows FULL closed** (what plays): the precedent is `NEEDS UPDATE` (the delay's TYPE, ADR 0213), which says why the stored key isn't heard and that an update brings it back.
- **Snap every number to its step (#1055, the first cut of this PR):** a continuous param's step is a turn's size, not a grid; LFO RATE lost its bottom 0.15 Hz and about 60 levels their off-step defaults.
- **The editor guesses stepped from the step's size:** a guess drifts from the spec; the box's own kind on the wire can't.

## Consequences
- Bank 1 grows by the closed word, the engine compare and the drop's store: `default` +64 B, `orbit` +32 B over their lines at 0.4 26465c7aa, the bench, fault and probe sets +72 to +168 B (bench-algo's report buffer among them); `bank2-gate`'s bank 1 came out 104 B smaller, the drop's call inlined there. Bank 2 `default` +256 B, `bank2-gate` +288 B, `orbit` +352 B (`ed since`'s clock and the note's text at its fold, describe's `stepped`, the drop inlined into `ed`'s cues). Every set's line is set to its measured value under this ADR.
- **#1053:** `midi_map::maps_put` is a thread-local on a host (each test a thread, its own card's count), the static atomic on the unit unchanged. `controller_page_test`'s mutex, which serialised the tests that moved it, is gone.
- **#1055:** `ed describe`'s `stepped` kind, the editor's snap and wheel by it, DELAY's NOTE a stepped spec with an `Index7` NRPN law (above).
- **#1059:** `ed since` reads the synced cells' clock once an answer (`Reading.clock`), so MIXER's commit writes what its measure counted, and DELAY's note's text is written at that fold (`delay_sync::note_text_at`, bank 2), not the meter's now; `QuarterBpm::of`; MIXER's clock is `Option` (no zero sentinel); `reverb_variant` a total match; a test holds PART's texts the same under FREE and SYNC on every engine.
- **Not done:** #1056 (KEY's pass fading out when the door closes mid-play) needs the pass or a fade in bank 1, not a stub. #1060's item 4 (`PartStatus` and its kin, read only by tests) stays for #1050. `library_ui_test` under `--features bank2` with the door closed fails on base too (#1060 item 6).

## Sources
- private issue #1053, 1055, 1057, 1059, 1060, 1063
- ADR 0087 (AXI's margin), 0198 (bank 1 frozen), 0213, 0296, 0303, 0305
