# 0289. Editor protocol 4: every page addressed, `since` in sections, a schema hash

- **Status:** Accepted (owner, 2026-11-19: every recommendation in the spec's questions)
- **Deciders:** firmware; owner (the spec's six questions, answered 2026-11-19 as recommended)

## Context
Protocol 3 (ADR 0285) reaches the followed Part's engine pages only. `in_schema` refuses the mixer's, the settings' and ORBIT's blocks. `PartMix`, `Tempo` and `Random` have no token. `axis_of` drops `ReverbPanel`, `SelectedRing` and `Orbit(_)`. FLT › MOD's route knobs and the MTX page describe as `route "<label>" next-slice`. Nothing reaches the MIDI map, learn, scene slots, the transport or projects. The owner wants the editor to control everything the box can in 0.4 (2026-11-18). A full answer for every page would pass the 32,000 B answer bound (ADR 0285). Describing DRUM again on every engine change takes 5 to 6 s. The spec is `docs/superpowers/specs/2026-11-19-ed-protocol-4-design.md`.

## Decision
- **The owner's decisions (2026-11-19), every recommendation accepted:**
  - **ORBIT:** a song is the project's 8 scene slots with queued switching, and "create a song" is a new project. Print is SCN's SAVE, which keeps the playing scene's drift. `ed scene move` is in. "Advance to the next scene" is out of scope for protocol 4.
  - **Projects:** option c. The editor may save, save-as into a new slot, and load. A load is witnessed by the listing's generation and refused over unsaved edits unless the editor sends `discard` after its own confirm. No `ed` command ever formats the card or removes a card file.
  - **Destructive actions:** as the table in question 3 recommends. In: saving a scene over a saved one; CLEAR of the playing scene; removing a Sound's map row; removing a controller map row, with a confirm; saving rows into the picked map; renaming a map; RANDOMIZE's roll; removing a matrix route. Out for 0.4: deleting a map file; Part CLEAR, RELOAD FROM PROJ and SAVE TO LIBRARY.
  - **ORBIT notes and chords:** in protocol 4 (`ed chord`, `scale`, `step`, `pool`).
  - **Matrix amounts:** the box's −127..+127, both on the box and in the editor.
  - **`ed learn`:** it moves the box to the cell's page, focuses the cell and arms learn there.
- **`PROTOCOL` is 4.** The editor requires exactly 4 (UPDATE THE UNIT below, UPDATE THE EDITOR above, as ADR 0285). Slices 1–5 (MIXER, MTX, SETTINGS, MIDI MAP, ORBIT) land in 0.4 inside protocol 4. The grammar freezes at the 0.4 release. After that, any change to a line's grammar is protocol 5.
- **Schemas.** `ed describe <SCHEMA> <page>`, `Schema` typed: the five engines, `MIXER` (`MIXER_CHANNEL_CHAIN`), `MTX-<ENGINE>`, `SETTINGS` (`settings::tree::ROOT`'s leaves but read-only and DEMO, each page with a `crumb` line), `MAP` (the binding grammar from `midi::page`'s words and `Input::kinds`) and `ORBIT` (`ORBIT_CHAIN`). No hand-written lists.
- **One walk with typed rounds.** `Pages` walks `Round { Shared, Pad, Part, Ring }`. `Schema::round_of` is an exhaustive match: a page binding `Part` or `Out` is described once a Part (MIXER, SETTINGS), `SelectedRing` once a ring (ORBIT), a pad's once a pad (DRUM). `describe`, `get`, `set`, `since`, `find` and `fmt_of` all use it.
- **Everything addressed, nothing selected.** `EdBlock` gains `Part(PartId, Mix|Sends)` (`PART<n>`, `PART<n>_SND` holding only SEND1–3), `Bus` (`CHORUS DELAY REVERB TAPE COMP`), `Global` (`THEME MIDI RANDOM`) and `Orbit` (`ORBIT`, `RING1`–`RING5`, `TEMPO`). `TEMPO`, `RANDOM` and the `PART` forms are new tokens, appended to `ed_block_tokens.txt`. The active Part's `PART` and the selected ring stay unreachable. `reachable(EdBlock)` replaces `in_schema`.
- **A set lands where it was taken for** (`Taken { Sound, Part, Global, Orbit }`). A Part's set is `edit_to(n, …)` for Part *n*, voices included (ADR 0281). An ORBIT set is `OrbitSong::edit` with a set-to closure that marks the ring touched (ADR 0082, 0083), never `part_page::edit`. `RANDOM.*` rolls only when the followed Part is still the active one.
- **`param` lines gain flags:** `m` (`Mappable`) and `d` (`modulatable`). MTX and MIDI MAP describe no target lists of their own.
- **Reverb panels.** `Axis::Reverb`, `Group::Reverb(ReverbType)`, panel `REVERB`, reported in `since`.
- **Routes are cells.** `ed route <SRC>:<DEST> <−127..127|off>`. The source is a `Jack`'s frozen `disk_ident`; FLT › MOD's jack knobs are the cells `FM3IN` and `QMIN`, resolved in the UI pass. Applied through `MatrixState::set`/`clear` and `sync_into`, priming as MIX+PLUS does. The amount moves at most the grid's fastest turn a pass, never a jump. `route "<label>" next-slice` becomes `route "<label>" <cell> <route|keybd>`. The wire carries the box's stored `i8` and its text.
- **Commands queue like `set`.** They are typed at parse, held in an 8-entry queue, applied after the sets in the UI pass through the box's own function for the gesture, and answered `OK queued` or `ERR`. A command that overwrites or removes kept state carries a witness (a slot's rev into `Confirmed<SceneSlot>`, a map row, a listing's generation). A stale witness writes nothing. Transport is idempotent (`ed orbit play|stop`). `ed learn <addr>` takes the box to the cell and arms learn there.
- **`since` in sections:** PART, ROUTES, MAP, MIXER, ORBIT, SETTINGS. Each has its own kept copy and its own rule for answering whole, and opens with a header line when whole. ROUTES diffs by (source, destination address). MAP answers whole when a 32-bit hash of its rows moves. ORBIT and SETTINGS carry `song`, `slot`, `project`, `sound`, `card`, `proj` and `toast` lines.
- **Whole answers page.** Sections are measured, then taken in order while the total fits `ANSWER_BYTES_MAX`, the first owed always. The rest are owed, and the answer ends `OK <m> more`. The editor asks again with `m`. A stale number starts over. A test holds every section's worst case under the bound alone.
- **The schema hash.** `ed schema <SCHEMA>` answers `schema <SCHEMA> <pages> <hash>`: a 64-bit FNV-1a of describe's own bytes over every page, computed by the walk into `Fnv64` (bank 1's `fmt_buf`, beside `ByteCount`, for the vtable rule) and kept for the boot. The editor caches schemas in IndexedDB keyed by `(protocol, schema, hash)`. If the first DRUM hash costs over 20 ms of UI loop on the unit, it is filled a page a UI pass after `ed hello`, and `ed schema` answers `pending` until it is done.
- **Never through `ed`:** OS UPGRADE, FORMAT CARD, and removing card files (#1015 and #1016 own those paths).
- **The owner's answers (2026-11-19), each the spec's recommendation:**
  - ORBIT's song is the project's 8 scene slots with queued switching; print is SCN's SAVE; `ed scene move` swaps two slots, both revs witnessed; advancing to the next scene is out of protocol 4.
  - Projects: read, `ed project save`, `saveas <name>` into a new slot, and `load <id> <gen>` witnessed by the listing's generation, refused over unsaved edits unless the editor sends `discard` after its own confirm. No project RENAME, CLEAR or DELETE through `ed`.
  - Destructive actions in protocol 4: save a scene over a saved one, CLEAR the playing scene, remove a Sound's map row, remove a controller map's row (editor confirm), save rows into the picked map, rename a map, RANDOMIZE's roll (labelled "ROLLS THE SOUND"), remove a matrix route. Out for 0.4: delete a map file; Part CLEAR, RELOAD FROM PROJ and SAVE TO LIBRARY.
  - ORBIT's chord degrees, CUSTOM scale, STEP notes and pool notes: `ed chord`, `ed scale`, `ed step`, `ed pool`, with `chord`, `scale`, `step` and `pool` lines in `since`.
  - Matrix amounts are the box's −127..+127.
  - `ed learn <addr>` takes the box to the cell's page and arms learn there.
- **Placement.** All new code is in `console::ed`, bank 2 S4, each fn in its own `.bank2_ed.*` section (ADR 0285). `EdState` stays in ITCM.

## Alternatives considered
- **Follow the active Part and the selected ring** (`PART.LEVEL`, `RING.HITS`). Cheaper, but an edit lands on whatever the box selects before the UI pass: the race ADR 0285 removed for pads.
- **One kept copy and one unbounded whole answer.** About 53 KB worst case: refused by the bound, and cutting it would break ADR 0276's recovery.
- **A `since` per section, each with its own number.** More round trips per poll and six recovery paths. One number with sections owns one recovery.
- **Separate destination and target lists in the MTX and MAP schemas.** A second copy of what `param` lines say; flags can't drift.
- **Address a route by grid column.** The registry's columns shift when one is removed. A route is its source and destination.
- **Resolve FLT › MOD's jack knobs in the editor.** It would race a re-patch on the box. The cell is resolved in the UI pass, as the knob does.
- **Schema cache keyed by the build string.** Every rebuild drops it, and dirty builds share one string. **A hash pinned in source:** describe depends on cargo features, so one constant can't hold every build. **Per-page hashes:** 165 values for nothing more.
- **Toggles for transport.** Two sends cancel out.
- **Learn armed on an address without moving the box.** It needs a target in bank 1's `Learn`, and the box would show nothing.

## Consequences
- **Bank 1:** about +80 B for `Fnv64` and its vtable. Target under 200 B for all five slices, each raised line naming this ADR and ADR 0251, measured.
- **Bank 2 S4:** about +15.5 KB (slice 1 3.5, slice 2 3, slice 3 2, slice 4 4, slice 5 3). ORBIT's S0–S3 is untouched.
- **ITCM:** about +1.6 KB: value slots 711 → 952 (+964 B), the routes' copy about 340 B, section and song state, schema hashes, the act queue. 8 KiB was free (ADR 0286). The non-PART copies can move to AXI if a later round needs ITCM.
- **Answers:** a messy DRUM Part with every section full takes two answers. Every other engine takes one. Diffs are as small as before.
- **The editor** caches schemas, follows `more`, and speaks the new lines. Its screens for these pages wait for the owner's mockups.
- **Frozen by this ADR at 0.4:** the new tokens, the route cell grammar, the section headers and the `param` flags field.
- **Answered:** the spec's six owner questions (2026-11-19), recorded above. No slice waits on them.

## As built

### Slice 1: MIXER and the frame
- **Schemas:** `Schema { Engine(EngineType), Mixer }`; `ed schema <S>` and `ed describe <S> <page>`. MIXER is 33 pages: CHR, DLY, CHAR, GRAIN, REV, MORE, MOR2, MST, LVL shared (REEL with `master-tape`), then PRT, VOI, EQ (ADR 0284's, merged since the design) and SND once a Part, `PART3_PRT`.
- **Rounds:** `Round { Shared, Pad, Part }`; a mixer page is a Part's when a cell binds `Part` or `Out`. `Pages` walks shared pages, then each round's.
- **Addresses:** every `EdBlock` arm is in (`Part`, `Bus`, `Global`, `Orbit`); `reachable` admits `Global` and `Orbit` with their slices. `SoundBlock` now holds only a Sound's blocks and the kit's: protocol 3 parsed `CHORUS.MIX` as a Sound address and refused it later. `PART` (the active Part's block) was in protocol 3's token fixture; it stays there as a reserved token no address takes.
- **`param` flags** are on every schema's lines; the engines' fixtures changed only by that field.
- **`since` in sections:** PART, then MIXER. MIXER answers whole on a reverb TYPE, chorus MODE, delay TYPE, plate or bank 2 change; the reverb's values again when its texts' key moves (PRE-DLY's pull, the clock). Paging takes sections in order, skipping one that doesn't fit for a later one that does (the design said "while it fits"); what isn't taken is owed. `OK <m> more`. A section past the bound alone is `ERR ed since too large`.
- **Answer sizes:** a whole MIXER section is about 3.1 KB in every reverb TYPE with every value at its max (the design's 7.5 KB was an upper estimate). Every engine's whole answer with MIXER still fits one answer; paging is tested at a smaller bound (`since.rs`).
- **Words in bank 2:** a `str` literal or a `write!`'s pieces are anonymous rodata, which bank 1 holds. Protocol 4's words are `ed::lit::Lit` statics in `.bank2_ed.lit`, numbers are written by `lit::uint`, never `write!`.
- **Not built:** `ed schema` answers synchronously. `schema <S> pending` (a page a pass) waits for the unit gate's timing of `ed schema DRUM`.
- **The editor:** protocol 4; the connect walk asks `ed schema` for the followed engine and MIXER, recalls the cached pages (IndexedDB, `Cache` ports, key `4/<SCHEMA>/<hash>`) or describes and keeps them; a new engine is read once the `since` answer ends; `OK <m> more` asks again on the next tick; the `part` and `mixer` headers drop their own section's values and panels.
- **Bank 1:** default 883,320 → 883,472 (+152), ORBIT 898,008 → 898,168 (+160), the gate 887,840 → 887,976 (+136), ORBIT with the writer +168; the bench sets +224 to +352. What grew: `Fnv64` (`write_str` 50 B, its vtable's `write_char` and `write_fmt`, 24 B of vtable), `EdState`'s layout in `Usb::service` and `tend_usb` (+20), the usage text (+8). Lines raised by the measured deltas.
- **Bank 2:** +3,296 B on default, ORBIT and the gate, all in S4; ORBIT's S0–S3 stays at 8,480 B free.
- **ITCM:** 43,064 → 43,816 B (+752): MIXER's copy and mask, the hashes, the owed set.

## Supersedes in part
- [0285](0285-editor-protocol-3-every-engine-pads-addressed.md): `PROTOCOL` 3 → 4; "the slots are laid out per engine" → per engine plus the common sections; `SLOTS` 711 → 952; `param`'s grammar gains flags; `ERR ed since too large` only for a single section past the bound.
- [0276](0276-browser-editor-protocol-ed-over-the-console-pulled.md): `route "<label>" next-slice` → a route cell; "every `ed since` answer that isn't a diff starts with `\n`" → every answer holding a whole section.

## Sources
- `docs/superpowers/specs/2026-11-19-ed-protocol-4-design.md`
- `chimera-core/src/console/ed/`, `ui/block_registry.rs`, `ui/settings/{tree,leaves}.rs`, `ui/orbit/pages.rs`, `ui/mod_grid.rs`, `ui/filter_panel.rs`, `ui/midi/page.rs`, `chimera-engine/src/{modulation,taps,midi_map/mod}.rs`, `chimera-engine/src/orbit/scene.rs`
- ADR 0077, 0080, 0082, 0083, 0139, 0198, 0217, 0226, 0232, 0251, 0253, 0255, 0266, 0276, 0281, 0285, 0286
