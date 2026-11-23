# Flat MIDI maps for 0.4 (#1103, #1099): implementation plan

Owner model approved 2026-11-12, reaffirmed 2026-11-22. ADR 0323 (replaces 0266, supersedes 0226's sections and 0264's SUGGEST). Base `0.4`, branch `midi-maps-04`.

This replaces `2026-11-12-flat-engine-maps.md` where they differ: rows in AXI (not ITCM), 8 B a row, Parts on one map share its copy, the map code stays bank 2's behind `MidiDoor`/`MidiUiDoor`, and no streaming reader.

## Data model
- `midi_map::ctl` (bank 2's glob, unchanged name) becomes the flat map. `MAP_ROWS = 90`; `MapLen` (0..=90) and `MapRowIx` (0..90) are newtypes.
- A row is `PackedRow([u8; 8])`: the `Bindings` v1 disk row (`[tag|kind][hi][lo][block][param][min][max][flags]`), decoded to a `Binding` on use by the codec's own `input_of`/`addr_of`. A SysEx row keeps its 4-B key in the map's key table (`[u32; 16]`), its index in the row's `lo` byte. A 17th SysEx key is `MapFull`.
- `FlatMap { rows: [PackedRow; 90], keys: [u32; 16], len: MapLen, nkeys: u8 }`, about 786 B: `get`, `push` (a target already held is replaced in place), `set`, `remove`, `find`, `absorb(&MidiMap)` (all or nothing), `heard_by(&Ctl)`.
- `MapPool` (`ui::midi::pool`, bank 2's code): six `FlatMap` slots, each with its name, whether its card copy is behind (`dirty`), and which Parts point at it. Parts on one map share its slot; six slots always suffice (one a Part). About 4.7 KB plus a 1.3 KB file buffer, a static in AXI `.bss` declared in bank 1's `MidiSlot::take` beside `RECALL`, reached through one pointer (`MidiSlot.maps`). Fixed, never grows; picking maps can't fill RAM.
- D2 gives back `Layers.scratch` (about 1 KB). The six 16-row `layers` stay as each Part's **seats**.

## Runtime: seats
- A Part's 32 binding slots stay: the Sound's 16, then 16 seats (today's layer). `BINDS`, `SLOTS` and their asserts are unchanged.
- A map load or edit refills a Part's seats: ECHO rows first, then the map's order, live and unshadowed only, up to 16. A map of 16 rows or fewer plays exactly as today.
- A message no Sound row or seat hears scans the Part's map (at most 90 compares) and seats each live, unshadowed row on that control into a resting seat (no ramp moving, no bin waiting, not the hand's this pass), the one heard least recently; only that slot's runtime state resets. None resting: the message is dropped. A seat is never torn from a moving ramp.
- Echo and the pickup hint cover the seats, as they covered the layer.

## Disk format
- The file is ADR 0226's frame (kind `Controller`, name in the header, A/B sides, generation, CRC). The body is `MapRows` records (`0x8024`, critical; bank 1's `RecordTag` gains the variant so the console's `put` check reads it). Each is a `Bindings` payload of up to 16 rows (the codec's own `encode`/`decode`), in map order.
- Old `MapSection` files (16-row sections, SUGGEST) load flattened: sections in file order, a duplicate target keeps the last, rows past 90 counted as skipped. The next save writes `MapRows` only.
- Read and written whole through bank 1's `read_small`/`write_small` into the pool's file buffer, not the stack. `FILE_MAX` covers 90 SysEx rows.

## Card work
- RAM is the truth: a row edit, add, drop or retarget changes the shared slot at once and marks it dirty; the next card work writes it (no read-modify-write, no CARD BUSY for row edits).
- A pick loads the map into a free slot, or shares the slot already holding it. A console `put`/`rm`, another card, or a pick reads again; a save, rename or delete stays a queued job as today.

## Factory maps
- `chimera-utils engine-maps` (`just engine-maps`, replaces `suggest-map`) writes ALGO, MODAL, VA, TAPE and DRUM through `ctl::save`.
- CCs: MIDI's undefined CCs first, then the rest `LearnCc` passes, less ADR 0263's built-ins and the controls gear sends by itself (1, 2, 4, 65–69, 84): 91 CCs.
- Rows: the shared cells first (ENV 1–3 in both TYPEs, LFO 1–3, filter, FX slot, pitch, out), each on the same CC in every map; then the engine's own cells by use; cells the built-in layer already drives are left out. Every row absolute `Jump`, echo off.
- DRUM's pad and kit cells are not `Mappable` (no disk code, selected-pad semantics): DRUM's map holds the shared cells a DRUM Part reads. Owner decision listed.
- `tools/factory_zip.py` requires the five maps and refuses `SUGG05F6.A`.

## UI (MIDI MAP)
- Rows: 0 the pick (NONE, then the card's maps), the map's rows (dimmed: the engine lacks it, or a Sound row holds its control), the Sound's own rows, `+ NEW BINDING`. Up to 108 rows; the row index stays a byte.
- `+ NEW BINDING` (ADR 0288's pointing): with a map picked the new row joins the map; with NONE, the Sound. EDIT on any row, a map row too, retargets it (control, range, input and echo kept).
- Finer range: MIX held while turning C or D moves MIN or MAX one place a frame, no acceleration (see ADR 0323 for why a step, not more bits).
- Screen goldens for: a factory map picked, a dimmed row, the bar on `+ NEW BINDING` past a long map.

## Protocol
- `ed` keeps protocol 4: `M<i>` rows run to `M90`; `row`, `map set`, `map rm` take them; `describe MAP` says the row limit. NRPN's canonical map is untouched.

## Order (test-first)
1. ADR + plan (this). 2. `FlatMap` + codec + old-format load (engine tests). 3. Pool, seats, card work (core tests). 4. Page, NEW BINDING, retarget, fine step (page tests). 5. `ed` rows. 6. Generator, factory zip, guide. 7. Goldens, `just check`, orbit build delta, PR. 8. Cirklon `.cki` regeneration.
