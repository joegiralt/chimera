# 0226. Keep controller maps as card files that each Part names, under its Sound's rows

- **Status:** Proposed (superseded in part by 0323: sections, layer merge, `MapSection` writes)
- **Deciders:** owner (#807 and its comments of 2026-10-26: controller-agnostic, parameter-targeted, shared plus per-engine sections, picked at the top of MIDI MAP per Part, a project remembers each Part's pick), firmware

## Context
A Sound's MIDI map (ADR 0139, 0217) travels with the Sound, so every new project means learning the controller again. The owner asked for named controller maps (FADERFOX, CIRKLON, LCXL3…) set up once and kept across projects:
- A map is message → parameter, whatever the controller sends; it works on any page.
- One map has a shared section and a section an engine (VA / ALGO / TAPE / MODAL). A Part plays the shared rows and its engine's; switching engine swaps them.
- Three layers per Part: a permanent built-in map (CC 1, 7, 10, 71, 74, 91, 93…), the controller map picked at the top of MIDI MAP, and the Part's own learned rows.
- Maps live on the unit, not in a project: save, load, copy over USB. A project stores each Part's pick.

#517 (KEEP MAP on load) and #808 (MIXER, SEQUENCER and AUTO channels with their own maps) build on this. Bank 1 is frozen (ADR 0198), and AXI has about 13 KB left above the 32 KB margin.

## Decision
- **A map is a card file.** `/CHIMERA/MAPS/<stem>.A|B`, an A/B pair (ADR 0045), kind `Controller` (7).
  - The header holds the map's name (a `SoundName`, NAMING's 16 characters).
  - Then one `MapSection` record (`0x0022`, not critical) a non-empty section: its code (0 shared, 1 + the engine's disk code), then a `Bindings` payload (ADR 0217's codec, up to 16 rows).
  - An unknown section code is skipped; a bad row is skipped and counted, as in a Sound.
- **The name makes the stem.** Up to four of the name's letters and digits, upper case, then a 16-bit FNV-1a hash of the name as four hex digits (`FADE3A7C.A`). A load computes the stem from the name, so it needs no listing. A header naming another map is `Missing`, never a wrong map.
- **A binding's section is its target's engine.** A target in an engine's own block (ALGO, ALGO OPs, MODAL, VA, TAPE) is that engine's. Anything every engine reads (filter, out, LFOs, voice FX, LEVEL, PAN) is shared.
- **A Part's layer.** Its engine's rows, then the shared rows on controls the engine's rows don't use, at most 16 (`MAP_LEN`).
- **Precedence: the Sound's own rows, then the layer.**
  - A Sound row on a control shadows every layer row on that control. The page shows a shadowed row dimmed.
  - Inside the layer, an engine's row beats a shared row on the same control.
  - The permanent layer is not built here. When it comes it sits above both, and its CCs are refused to learn.
- **A project names each Part's map.** A `Controller` record (`0x0021`, not critical, 16 B: the padded name) follows the Part's `Origin`. NEW and a Part with none write nothing. The map itself is read again from the card on every project load, so editing a map reaches every project that names it.
- **Loading is pulled, not pushed.** Bank 2 keeps each Part's layer and what it was read for: name, engine and a save count. Each `card_work` pass reads at most one stale layer. A load, NEW, an engine change, a pick and a save all simply make layers stale.
  - A missing or damaged file leaves the layer empty until the pick, the engine or a save changes.
  - A busy card is tried again the next pass.
- **MIDI MAP** (SETTINGS › PART › MIDI MAP):
  - Row 0 is `CONTROLLER` with the Part's map, or `NONE`.
    - B steps through the maps on the card (listed when the page opens, at most 16; with more, the focus band says `CARD HOLDS MORE: 16 SHOWN`) and `NONE`.
    - EDIT saves: NAMING, starting from the map's name.
  - Then the layer's rows, each marked with the map's name in its focus band. A row the Sound shadows, or one the engine doesn't read, is dimmed.
  - Then the Sound's own rows, as before.
  - On a layer row, B–F do nothing; MIX+MINUS asks DELETE and drops the row (its control and target) from the map file. A map gone from the card since it was read is never written: `FILE NOT FOUND`.
- **Save moves the Part's rows into the map.** It writes what the Part plays.
  - Each Sound row clears the rows on its control or target in its target's section (a shared row in the Part's engine's section too), then joins its section.
  - The other sections are kept.
  - Saved under another name, the Part's map is copied first ("save as").
  - Then the Part picks the saved map, and its Sound's map is emptied: a Sound edit, so `*` shows.
  - A save that would put more than 16 rows in any engine's layer is refused (`MAP FULL`), with nothing written.
- **Learn** still adds the Sound's own rows (MIX+SEQ), as the owner's third layer says.
- **Placement.**
  - **Bank 2:** the map's type, layer, save merge and file framing (`midi_map::ctl`, globbed beside the codec). The page's rows, keys and card work go through `MidiUiDoor` (`map_rows`, `map_row`, `ctl_due`, `ctl_work`, `ctl_ask`).
  - **Bank 1 gains:** the project's field, record and setter; `Dir::Maps`; the page's row count and drawing stubs, row 0's EDIT to NAMING, NAMING's answer and one `card_work` call. It also gains three small-file helpers (`storage::read_small`, `write_small`, `list_stems`).
  - **Why the helpers are bank 1's:** a trait object that bank 2 builds of its own type gets an anonymous vtable that no glob places, and `objcopy` refuses a bank 1 that points into bank 2. So bank 2 reads and writes a map whole through bank-1 sinks and frames it itself: the A/B sides, a generation on, the CRC and the header's name.
  - **Closed:** the page reads `NEEDS UPDATE`, and no map is read or written. The project still saves each Part's pick.
- **Resolution.** A Part resolves 32 binding slots: the Sound's 16, then the layer's 16. Canonical NRPN slots follow. The hand, the ramps, the echo and the pickup hint treat both alike.

## Alternatives considered
- **The map inside the project** (copied on pick). The owner's design keeps maps on the unit, and a copy would go stale the moment the map is edited.
- **Ids for stems with a name lookup.** It needs a listing and a header read per map before any load. The stem from the name loads in one read and survives copying files between cards.
- **All sections in RAM per Part.** About 5 KB of AXI. Reading the one layer again on an engine change costs one small file read.
- **Layer rows editable on the Part (B–F).** That needs an edited-map state, a second dirty mark and a save path for it. Learning a Part row over the control and saving gives the same result with the one path.
- **Learn writing into the controller map** (the issue's first sketch). The owner's later three-layer design keeps learn on the Part's own rows; save moves them.
- **Asking before a save overwrites a map of the same name.** Saving under the Part's own map's name is the normal update. NAMING starts from that name, so another name has to be typed on purpose.

## Consequences
- **#517 (KEEP MAP):** a Sound load can keep the Part's own rows. The controller layer already survives any load: it belongs to the Part and the project.
- **#808 (MIXER, SEQUENCER, AUTO):** a channel role names a map as a Part does, and reads it through the same loader. Its targets (FX, ORBIT) are shared-section targets. A future section code (5+) can hold role-only rows without breaking older readers, which skip unknown sections.
- **Measured** (`tools/bank1-budget.txt`, main 4b9ceefd against this branch):
  - **Bank 1, sets with MIDI MAP:** `default` +2424 B, `orbit` +2520 B, `bank2-gate` +2408 B, `usb-audio` +2440 B (over main's lines). By symbol against main 913fcbd6 (`default`): `encode_project` +332 (the `Controller` record) and `ProjectCheck::step` +150; `UiState::input` +572 (row 0's EDIT, NAMING's answer and `name_controller`, inlined); the small-file helpers about 620 (`list_stems` and its closure, `read_small`, `write_small`, their sinks and vtable shims); the card-work stub `ctl_card` 176 and its errand closure in `synth` +212; `map_deleted` +84 and the DELETE prompt +44; the rest under 40 B a symbol (outliner and inlining churn).
  - **Bank 1, bench sets** (no MIDI MAP): +16 to +40 B: `Dir::Maps` and the codec, with `put_song` kept out of line (inlined into `encode_project` it cost about 250 B in every set). They hold no controller names (`chimera-engine/controller-maps` is off), since their D2 had 56 B free, so a bench build's project save writes no pick.
  - **RAM (AXI):**
    - with `midi-map`, the MIDI UI slot is 7896 → 14552 B (+6656 on main 913fcbd6): each Part's 16 more slots, the echo's 16 more (an echo slot 8 B, a sentinel in place of an `Option`), and the layers and listing (1400 B). In AXI that left `usb-audio` 2272 B under its 32 KiB margin, so with `midi-map` the whole slot lives in D2 (`.ram_d2`, cacheable, NOLOAD, its fields written in place by `MidiSlot::take`): AXI gains back the main slot's 7896 B too, and D2 keeps 16608 B free in `default`, `orbit`, `bank2-gate` and `usb-audio` on main 4b9ceefd, with #889's tuning tables in it (about 7.8 KB once #891's program change takes its 8.8 KB); free AXI 40696 B (`usb-audio`) to 45424 B (`default`). DTCM (the stack) and SRAM4 (700 B free) can't hold it;
    - the bench builds' slot stays in AXI (their D2 is full, 56 B free in `bench-algo`) and grows by about 500 B, since they hold no layer slots (`bench-algo` free AXI 8316 B of its 8 KiB margin);
    - the `Project` grows by 104 B (each Part's name), in D2; none in the bench builds.
- Two names sharing a stem (a 16-bit hash collision on the same four letters) can't both be on one card: saving the second is refused (`NAME TAKEN`), and the first stays.
- Maps are copied over USB as files (`/CHIMERA/MAPS`). The console's `ls`/`put`/`get` don't reach the folder yet (follow-up), and no ready-made maps ship yet.

## Sources
- #807 and its comments; #517; #808.
- ADR 0045 (card format), 0139, 0198, 0216, 0217.
- Elektron (Digitakt/Digitone) per-track CC maps; Squarp Hapax instrument definitions (an instrument definition in reverse, the owner's phrase).
