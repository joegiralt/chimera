# Projects and Storage: Design

Spec 1 of 4 in the projects roadmap:

1. **Projects and storage.** This spec.
2. **AFX mode.** Per-key sounds on a Part.
3. **The MIDI CC matrix.** A "what does CC N do?" view.
4. **The default project template.**

Specs 2–4 are out of scope here. This spec only reserves room for them in the data model and the file format.

Revised after the adversarial review (`projects-spec-review.md`). It targets main after PR #184 (8 voices, `ModState` presence, `BlockRef::Env/Lfo/Pitch`).

## Intent

Chimera is multitimbral, and today nothing it plays can be saved. Turn the synth on and you rebuild your setup from scratch. This spec makes the **project** the unit you work in and perform with:

- A project is everything musical the synth is doing: its sounds, its six Parts and the shared effects.
- You save it to the SD card and load it back. The last one comes back at power-on.
- You can move from one song to the next during a set.

**Design principle (owner's words).** "There's only really one way to do things, and everything else is sugar on top." Everything is reached down one ladder:

- **Projects › Project › Part › Sound**, then the sound's own pages.
- EDIT goes down. A tap on MENU goes up. A hold on MENU opens System.
- A breadcrumb and a key legend always say where you are and what the keys do.

**Done when:**

- A project can be saved, the unit power-cycled, and the same project comes back bit-identical.
- Sounds can be saved to, and picked from, a tagged library on the card.
- Every Location is covered by host tests and screen goldens, and has been played on the chip.

## Plans

Three plans. Each ships on its own and can be cut.

1. **Storage foundation.** SD on SPI2, brought up on the chip first. Then the `Store` HAL trait, the desktop store, the codec with pinned code tables, A/B files, the SYSTEM file, boot load of SYSTEM, and the fuzz test.
2. **Projects and the ladder.** `Project`, `PartId`, `Location`, derived marks, `ReplaceGuard`, project save and load, the load protocol, and boot load of the last project. The ladder ADR is accepted before this plan starts.
3. **Library, tags and naming.** Library sounds, INDEX, the fresh-card factory library, tags and the naming screen.

## Measured sizes

These are host `size_of` figures (64-bit) from the review, taken on the routing branch, which is now main. On the chip, `usize` fields shrink them by about 12 B per Sound. **Plan 1 re-measures them on the chip.**

| Type | Bytes |
|---|---|
| `Sound` | 888 |
| `SoundPool` (32 × `Option<Sound>`) | 28 416 |
| `Part` / `Performance` | 920 / 5 640 |
| Project (pool + `Performance`) | ≈ 34 KB |
| `TripleBuffer<AudioShared>` | 13 136 |
| `UiState` | 35 664 |
| AXI free | 90 940 |
| D2 free | 2 080 |
| A serialised project (estimate) | 50–60 KB |

What follows from these numbers:

- There is no staging copy and no serialised buffer.
- Saves stream from live state, and loads use two passes (§ Loading while playing).
- Nothing new goes in D2.
- Every new static is added to `AXI_RESIDENT` in `instrument.rs`. That means the card store (our own FAT layer, ADR 0048: one 512 B block buffer and one 512 B FAT-sector cache, under 2 KB with the driver) and the library index. Plan 1 also fixes the stale stack comment on `UI_RESERVE` there (ADR 0025).

## Model

| Thing | What it is | Owns |
|---|---|---|
| **Sound** | A patch | Name, tags, engine, every engine and chain param, and the mod routes. No MIDI, no mix, no MONO/POLY. |
| **Pool** | The project's sounds: 32 numbered slots | A copy of each Sound, plus a `u16` generation per slot. Slot numbers are stable, so AFX (spec 2) can refer to them. |
| **Part** | A player | Its own copy of a Sound, its `Origin`, and its mix: CHANNEL, MODE (MONO/POLY), OUT, PART LVL, PAN and the three sends. |
| **Project** | The whole musical state | Its id, name, pool, `Performance` (six Parts plus FX), and reserved records (tempo, AFX maps, CC map, set list). |
| **Library** | Every Sound on the card | Sounds only, browsed by tag and filtered by engine. |
| **System** | The synth's own setup, never part of a project | Only what exists today: THEME (BRIGHT, GAMMA, ACCENT, BLACK). Plus the id of the last project saved or loaded. |

Changes to the code on main:

- A new `Project { meta: ProjectMeta, pool: Pool, perf: Performance }`, where `ProjectMeta { id: Option<ProjectId>, name: ProjectName, saved_crc: Option<u32> }`.
  - `Performance.name` moves into `ProjectMeta`.
  - `AudioShared::update_from` keeps reading only `project.perf()`.
- `UiState.performance` and `UiState.pool` merge into one private `project: Project`.
- `SoundPool` becomes `Pool`. It is indexed by `SlotId` and carries `gens: [u16; 32]`.
- `Part { sound, loaded_from: Option<u8>, mix }` becomes `Part { sound, origin: Origin, mix }`. Nothing else changes in `Part`.
- `Sound` gains `tags: TagSet`.
- `active_part: usize`, `Performance::edit(usize)` and every other Part index become `PartId`.
- `BlockRef::Channels`, `impl Block for [Part; MAX_PARTS]` and System › MIDI Setup are deleted. Channels move to rung 2.

### Types decide what's possible (owner's rule)

Every rule here is carried by a type or by a pure function, never by a stored flag that could drift. All of these types are pure `chimera-core`.

| Type | Shape | What it rules out |
|---|---|---|
| `SlotId` | Newtype over `u8`. Built only by `SlotId::new(n) -> Option<_>` (n < 32) or `SlotId::ALL`. | Out-of-range slots, and raw `usize` pool indexes. |
| `PartId` | **New.** Newtype over `u8`. Built only by `PartId::new(n) -> Option<_>` (n < 6) or `PartId::ALL`. It replaces every `usize` Part index. | Out-of-range Parts. |
| `Pool` | `[Option<Sound>; 32]` plus `gens`, private. `get(SlotId) -> Option<&Sound>`. `store` and `clear` bump the slot's generation. `clear` refuses a slot a Part uses. | Loading an empty slot, because a load takes `&Sound`. A slot change the generation doesn't see. |
| `Origin` | `enum { Slot { slot: SlotId, gen: u16, crc: u32 }, Init(EngineType) }`. Set only by load, save-over-slot and revert. `crc` is the Sound's CRC at that moment. | A Part pointing at a missing slot. |
| `PartStatus` | `enum { Clean, Edited, Stale(SlotId) }`, **returned** by `part_status(&Part, &Pool) -> PartStatus`, never stored. See § Derived marks. | A `*` that disagrees with the bits. `Stale` on an INIT origin (L3). |
| `PartAction` | `enum { OverSlot(SlotId), NewSlot(SlotId), ToLibrary(LibTarget), Revert(SlotId) }`. `part_actions(status, &Pool) -> ArrayVec<PartAction, 4>` offers only what applies: `Clean` offers no `Revert` and no `OverSlot`, and `NewSlot` needs a free slot. | A menu option that can't apply. The menu is built from this list. |
| `ProjectStatus` | `enum { Pristine, Saved, Modified }`, **returned** by `project_status(&Project, template_crc)`. See § Derived marks. | A missed "mark unsaved". The "save first?" prompt on an untouched project. |
| `ReplaceGuard` | `ReplaceGuard::check(&Project, Replace) -> Result<Confirmed, NeedsConfirm<Pending>>`, where `Replace = { PartFromSlot(PartId, SlotId), PartFromLibrary(PartId, LibId), ProjectFromFile(ProjectId), ProjectNew }`. `Confirmed` has a private constructor. Only `check` (on a clean target) and `Pending::{save_then, anyway}` build it. `Pending` holds the source **id**, not parsed data, and re-validates it on confirm. | Losing edits silently on any replace. A replace path that skips the prompt. |
| `Name<N>` | Validated A–Z, a–z, 0–9, space and `-`, 1..=N chars. `ProjectName` and `SoundName` are `Name<16>`. | Unprintable, empty or over-long names. |
| `Tag` / `TagSet` | `Tag = { Builtin(BuiltinTag), Custom(CustomTag) }`. `BuiltinTag` has 16 closed variants. `CustomTag` is 0–7. `TagSet(u32)` supports only insert, remove and contains. | A 17th built-in tag, a 9th custom tag, and tag strings stored in a Sound. |
| `EngineFilter` | `enum { All, Engine(EngineType) }`. It cycles `EngineType::ALL`, so new engines appear by themselves (L4). | A filter value that no engine matches. |
| `Location` | See § Navigation. An opaque struct over an enum private to the nav module. `down`, `up` and the sugar jumps are its only constructors. | A second route anywhere. |
| `Card` | `enum { Absent, Ready(VolumeId), Failed { err: CardError, last: Option<VolumeId> } }`, with the transitions in § Card. `CardError` has no NO CARD variant. Listing and saving take a `&Ready`, which is neither `Copy` nor `Clone` and is lent only inside `Card::run`. | Browsing or saving with no card. Writing to a swapped card. Skipping the per-operation mount. |
| `ValidAddr` | A `ParamAddr` known to have a spec, built only by iterating `block.specs()` (L1). `ParamRecord { addr: ValidAddr, value }`. | Writing a record for a param that doesn't exist. |
| `RecordTag` | A closed enum on the writer. `Unknown(u16)` exists only in the reader's `ReadTag` (L2). | Writing a record this version doesn't define. |
| `LoadEpoch` | `AtomicU32` `LOAD_EPOCH` and `LOAD_ACK`, outside the triple buffer. `Project::replace` is the only writer of `LOAD_EPOCH`. | Publishing a new project while voices from the old one still sound. |

**How the mutation paths go through these types:**

- `Project`'s fields are private. It exposes these operations and nothing else:
  - `perf()` and `pool()` for reading;
  - `edit_part(PartId) -> PartEdit` for encoder edits, the matrix, `edit_route` and dest-registry priming;
  - `edit_fx()`;
  - `pool_store(SlotId, Sound)` and `pool_clear(SlotId)`;
  - `set_name(ProjectName)`;
  - `replace_part_sound(Confirmed)` and `replace(Confirmed, source)`.
- Edits can go through `&mut` freely, because the marks are derived. An edit can't leave a mark wrong.
- Only the four replaces can lose work, and each one needs `Confirmed`.
- Compile-fail tests (`trybuild`) cover: building a `Confirmed`, constructing a `Location` variant outside the nav module, loading an empty slot, and saving without a `ProjectId`.

### Derived marks

**Part `*`.** A Part's `*` means its sound differs bit for bit from its pool slot. `Sound::bits_eq` compares floats by `to_bits`. `part_status(part, pool)` works like this:

- `Origin::Init(e)`: `Clean` if the sound `bits_eq(Sound::init(e))`, else `Edited`.
- `Origin::Slot { slot, gen, crc }` and the sound `bits_eq`s the slot: `Clean`.
- The generation matches (the slot hasn't changed since the load): `Edited`.
- The generation moved and `crc(sound) == crc` (the Part is unedited, the slot changed under it): `Stale(slot)`, shown as `◦`.
- The generation moved and the Part was edited: `Edited`.

**Project `*`.** `project_status` works like this:

- `Pristine`: the CRC of the canonical serialisation equals `template_crc`, the CRC of a NEW project, computed once at boot.
- `Saved`: the CRC equals `meta.saved_crc`.
- `Modified`: otherwise.

The CRC is recomputed on every UI frame that had input, because only input changes a project (loads reset `saved_crc`). Undoing an edit returns to `Saved`. Plan 2 measures the cost; the estimate is well under 1 ms for 60 KB.

**Consequences:**

- **Saving a project doesn't clear Part marks.** Those compare the Part with its slot, not with the file. The Part row keeps `*`, and the save toast says "P2 DIFFERS FROM SLOT 03".
- **Pool changes** (a library copy, a slot clear, a save over a slot) change the project CRC, so they mark the project.
- **FX, mix and name edits** mark the project, not any Part.
- **THEME and the other System settings** are outside the project and never mark it.
- **Stale marks aren't saved.** A load sets every `Origin::Slot` to the slot's current generation and CRC. A Part that was stale when saved reloads as `*`, because it differs from its slot. That is the definition of `*`.

### Copy rules (owner's decisions)

- **Library → Pool** is a copy. A project owns its sounds, and later library edits never change a project.
- **Pool → Part** is a copy. A Part edits its own copy.
- **Part → Pool.** From an edited Part you can save it over its slot, save it to a new slot, or revert it from its slot. Save-over and revert set its `Origin` fresh.
- **Part → Library.** Save as a new library sound, or overwrite an existing one after a confirm. Then name and tag it.
- **Saving over a slot other Parts use.** The other Parts keep their own copies. The unedited ones derive `Stale` (`◦`) until reloaded. The save screen names them: "P4 ALSO USES SLOT 03: UPDATE P4 / LEAVE". UPDATE is a replace on a Part that is `Stale`, not `Edited`, so it needs no guard prompt.
- **Saving a project is explicit.** Every replace that can lose work goes through `ReplaceGuard`:
  - a slot or library sound loaded into an `Edited` Part asks **SAVE PART FIRST / REPLACE / CANCEL**;
  - `+ NEW`, or loading a project, while the project is `Modified` asks **SAVE THEN LOAD / LOAD ANYWAY / CANCEL**.
  - `Pristine` and `Saved` never ask.

### Unsaved-state edge cases (review M4)

| Case | Behaviour |
|---|---|
| A Part edited back to its slot's exact bits | `Clean`. Derived. |
| A project saved while Parts show `*` | The project shows `Saved`. The Part `*`s stay, and the toast names them. |
| The pool changes (library copy, clear, save over slot) | The project becomes `Modified`. |
| FX or Part mix edits | Mark the project, not the Part. |
| THEME | Never marks the project. |
| Stale across a save and reload | Reloads as `*` (see above). |
| SAVE THEN LOAD on a never-saved project | Runs the naming screen inside the pending replace. A cancelled or failed save **aborts** the load. |
| SAVE AS with a name that already exists | Allowed, since ids are unique. The save screen warns "NAME EXISTS" and offers **KEEP BOTH / OVERWRITE THAT ONE** (confirm). |
| A power cycle | The last saved or loaded project loads. A never-saved project is lost, and the Project rung says so in its footer ("NOT SAVED"). |

## Navigation: one `Location`

```rust
pub struct Location(Loc);              // opaque; `Loc` is private to ui::nav
enum Loc {
    Projects,                           // rung 0
    Project,                            // rung 1
    Part(PartId),                       // rung 2
    Sound(PartId),                      // rung 3
    Pages(PartId, PageAt),              // a sound's pages (today's Part chain)
    Fx(PageAt),                         // CHORUS, DELAY, EFX, TAPE, MASTER
    System(PageAt),                     // THEME, UPDATES, ABOUT (+ AUDIO)
}
pub struct PageAt { node: u8, sub: u8 }
```

- **`down(self, row: Row) -> Option<Location>`** is EDIT on the focused row.
  - Projects: the loaded project → `Project`. Another project or `+ NEW` → the guarded replace, then `Project`. `SYSTEM ›` → `System(0,0)`.
  - Project: Part n → `Part(n)`. `FX ›` → `Fx(0,0)`.
  - Part: `SOUND ›` → `Sound(p)`.
  - Sound: the Part's current sound → `Pages(p, 0,0)`. Another slot → a guarded replace; you stay on the rung. An empty slot → LIBRARY aimed at that slot.
  - Pages, Fx, System: sub-page down, as EDIT does today.
- **`up(self) -> Location`** is a tap on MENU. It goes up one rung from anywhere:

  | From | To |
  |---|---|
  | `Projects` | `Projects` (no-op) |
  | `Project` | `Projects` |
  | `Part(p)` | `Project` |
  | `Sound(p)` | `Part(p)` |
  | `Pages(p, _)` | `Part(p)`, the owner's rule |
  | `Fx(_)` | `Project` |
  | `System(_)` | `Projects` |

- **Sugar**, each defined in code *as* a composition of `down` calls:
  - hold MENU = `Projects.down(SYSTEM)`;
  - plain B1–B6 = `…down(Part n).down(SOUND).down(current)` = `Pages(n, 0,0)`, exactly as today;
  - EDIT+B1–B6 = `…down(Part n).down(SOUND)` = `Sound(n)`.
- **Property test (M7).** Every `Location` has a `down` path from `Projects`, and every jump lands where its path ends.
- **What goes away:**
  - `ChainNav`'s own `ChainId` and `UiMode::SoundBrowser` are folded into `Location`. The chain defs stay as the page source for `Pages`, `Fx` and `System`.
  - `ChainId::Mixer` and MIX+B1–B5 go: the PART and SENDS pages become rung 2, and the FX nodes become `Fx`.
  - MIX+B6 (Demo) becomes a `DEMO ›` row in System, in debug builds only.

### The keys (H3)

Each key has one meaning, and every rung shows it in its legend.

| Key | Meaning |
|---|---|
| **EDIT** | Down or open (on pages, sub-page down). |
| **MENU tap** | Up one rung (the `up` table). Acts **on release**, and only if released before `MENU_HOLD_MS`. |
| **MENU hold** | System, from anywhere. Fires when MENU has been down for `MENU_HOLD_MS` = **600 ms**, a tunable constant. The release after a hold does nothing. |
| **SEQ** | Save the thing this rung is about. On pages and in System, it keeps today's sub-page up; the legend says so. |
| **PLUS / MINUS** | Sideways: the next or previous node on pages, and the next or previous Part on rungs 2 and 3. Inert elsewhere, except the chip toggle on the modal tag screen. |
| **MIX+MINUS** | Remove: a pool slot, a project on rung 0, or a library sound. Always behind a confirm. |
| **MIX+PLUS** | Prime a route on pages (ADR 0017). On a library entry, it opens rename and retag. |
| **MIX+MENU** | Stays reserved for the chain editor (UX spec § Chain Editor). |

Timing:

- `ButtonState::Held` means "down for two frames" today and carries no duration.
- Plan 2 adds press timestamps and **edge latching** in the controls tick. That gives a tap on release, a hold at the threshold, and no press lost during a blocking card operation (H6).
- The stock bootloader's hold-MENU-at-power-on happens before the firmware runs, so the two don't collide.

Plan 2 amends `docs/chimera-ui-ux-spec.md`:

- "MENU always enters the system chain" (§ line 602) becomes "hold MENU".
- The MIX+Bn Mixer chain is removed.

**Owner review (interpretation).** The owner said "tap/release → project, tap/hold → system menu". This spec reads it as follows. A tap goes up one rung, and from a sound's pages it lands on the Part rung. So a tap reaches the project in one to three presses. A literal reading, "a tap always jumps to the Project rung", is a one-line change to `up`. The owner should confirm which one is meant.

### Legend (M10)

Every rung has a one-line footer, and the screen goldens include it:

- Rung 0: `EDIT open · SEQ save as · MIX- delete · hold MENU system`
- Rung 1: `EDIT open · SEQ save · MENU up · hold MENU system`
- Rung 2: `B edit · EDIT sound · SEQ save part · MENU up`
- Rung 3: `B source · C engine · EDIT load · SEQ save part · MENU up`

### Rung 0: PROJECTS

- The rows are the projects on the card, `+ NEW`, then `SYSTEM ›`. `●` marks the loaded project.
- **A** scrolls and **EDIT** opens.
- **SEQ** is SAVE AS: it runs the naming screen for a new id. That is also how you duplicate a project.
- **MIX+MINUS** deletes the focused project after a confirm. You can't delete the loaded project.
- A project whose file needs newer firmware, or fails to parse, is greyed and shows its error.
- With no card, the rung shows NO CARD and lists only `+ NEW` and `SYSTEM ›`.

### Rung 1: PROJECT (breadcrumb `ACID PARTY *`)

- **The header** shows the name, with `*` when `Modified`.
- **The stats strip:**
  - PARTS in use: Parts whose sound isn't bit-equal to `Sound::init` of its engine. There's no "off" channel in `MidiChannel`, so this is the only honest definition;
  - VOICES, the voice cap;
  - POOL, slots used out of 32, with a bar;
  - CPU, from `AudioStats`, with a bar.
- **The rows:**
  - NAME: EDIT opens the naming screen, and a rename marks the project;
  - one row per Part, with its sound name, `*`/`◦`, its channel, and a layer mark when another Part shares the channel;
  - `FX ›`.
- **A** moves between the rows. **EDIT** opens the focused row. **SEQ** saves the project, or runs SAVE AS if it has never been saved.

### Rung 2: PART (breadcrumb `ACID PARTY › P4`)

- This rung replaces the Mixer chain's PART and SENDS pages and System › MIDI Setup.
- **The rows:**
  - `SOUND ›`, showing the slot and name;
  - CHANNEL;
  - MODE (MONO/POLY);
  - OUT;
  - PART LVL;
  - PAN;
  - CHR SEND, DLY SEND, REV SEND;
  - LAYER, read-only: the other Parts on this channel;
  - VOICES, read-only: this sound's voices under the cost model.
- **A** picks a row and **B** changes its value, lerped like any param. **EDIT** on `SOUND ›` goes down. **SEQ** opens the Part menu, built from `part_actions`.
- PART LVL is named so it can't be confused with the Sound's OUT page volume (L5).

### Rung 3: SOUND (breadcrumb `ACID PARTY › P4 › SOUND`)

- **B** picks the source. The first entry is PROJECT (the pool), then LIBRARY ALL, then each built-in tag, then each custom tag. So one control does both "tab" and "tag".
- **In PROJECT:** a numbered list showing slot, name, tags, and which Parts use each slot. Empty slots show `—`.
  - **EDIT** on the Part's current sound opens its pages.
  - **EDIT** on another slot is a guarded replace.
  - **EDIT** on an empty slot switches the source to LIBRARY, aimed at that slot.
  - **MIX+MINUS** clears a slot after a confirm. A slot a Part uses can't be cleared.
- **In LIBRARY:**
  - **C** filters by engine (`EngineFilter`). **A** scrolls.
  - **EDIT** copies the sound into the aimed slot (or the first free one) and does a guarded replace into the Part.
  - If the pool is full, show POOL FULL and name the slots no Part uses.
  - INIT sounds are ordinary library entries, one per engine, tagged by engine name.
  - An empty library explains itself: "NO SOUNDS YET · SEQ ON A PART SAVES ONE".
- **SEQ** opens the Part menu, the same one as on rung 2.

### Naming and tags

- **The naming screen gives each encoder one job:**
  - A moves the cursor;
  - B cycles A–Z;
  - C cycles 0–9, space and `-`;
  - D toggles case;
  - E deletes;
  - F goes to the tags.
- It opens with a proposed random name (for example `DUB-042`) already filled in.
- **EDIT** goes next, **SEQ** saves, and a **MENU** tap cancels.
- **Tags are chips.** This is a modal screen, so **PLUS** toggles a chip here; the legend says so.
  - The 16 built-in tags: BASS, PAD, KEYS, LEAD, PLUCK, STAB, PERC, FX, DRONE, ARP, ACID, DUB, DARK, BRIGHT, MONO and POLY.
  - Up to 8 custom tags follow (drawn dashed), then `+ NEW`.
  - **F** moves between chips.
- **Custom tags travel with sounds.**
  - A sound file stores the *names* of the custom tags it uses.
  - Reading a sound or project into this card's library merges those names into `TAGS`. A matching name (case-insensitive) reuses its bit; a new name takes a free bit.
  - **On overflow** (all 8 bits in use), the extra tags are dropped from the card copy only. The screen says "TAG DUBSTEP DROPPED: 8 CUSTOM TAGS IN USE", and the source file keeps the name.
  - A rename rewrites `TAGS` and every library file that carries the tag, with a progress bar.
  - A custom tag can be deleted only when no library sound uses it, so a bit is never reused under old sounds.

## Storage

### Card (plan 1, on the chip first)

- **Bring-up.** SD in SPI mode on SPI2: SCK PA9, MISO PB14, MOSI PB15. The CS and card-detect pins come from the PreenFM3 schematic, and plan 1 pins them down.
  - Init at ≤ 400 kHz, then switch to the fast clock.
  - Add the `embedded-sdmmc` dependency **as the SD block driver only** (`SdCard`). The FAT16/FAT32 file layer is our own (`chimera-fat`, ADR 0048): 8.3 names in the fixed folders, streamed read and write, delete, make_dir and list. Check that `stm32h7xx-hal` 0.16 provides embedded-hal 1.0 `SpiDevice` and `DelayNs`, or wrap them.
  - Transfers are **polled**, with no DMA. The stack is in DTCM, which DMA1 and DMA2 can't reach, and D2 is full. This supersedes the design doc's DMA2 note.
- **Blocking.** Card I/O runs in the UI loop, never on the audio path.
  - Boot draws BUSY before the SYSTEM read, which can take a cold acquire of up to about 3 s. Leaving System draws nothing first, since the save is quicker than BUSY can be read. It then shows a toast in the same box: SAVED for about 600 ms after a write, or the error's message for about 1.2 s. A load (the theme changing is the feedback) or nothing to do shows no toast. The toast blocks nothing, and new input takes it down.
  - Every operation has a timeout.
  - Audio and the watchdog run above the main loop and aren't affected. Button edges are latched, so no press is lost.
- **States.** `Absent`, `Ready(VolumeId)` and `Failed(CardError)`:
  - The card is **mounted per operation**. Every handle is closed on success and on error.
  - Any card error sets `Failed`, which keeps the last `VolumeId`. NO CARD sets `Absent`. A missing or damaged file is a file error and leaves the state alone.
  - A full card is a file-level condition, not a card fault: the card is healthy and a delete makes room, so `Full` leaves the state alone too, with no re-init.
  - `Failed` or `Absent` becomes `Ready` only after a fresh init and mount.
  - `Absent` forgets the last `VolumeId`, so even the same card put back mounts as new and every cache is rebuilt. This is conservative by design.
  - Each mount compares the volume serial and label with the cached `VolumeId`. On a mismatch, the library index and project list are dropped, and any `Pending` replace re-validates.
- **Formats.** FAT16 and FAT32 only. exFAT (the default on SDXC over 32 GB) shows "CARD IS EXFAT: FORMAT FAT32".
- **Fresh card.** A card with no `/CHIMERA` gets the directories, the factory library (the factory Sounds plus one INIT Sound per engine) and an empty `TAGS`, with a progress bar.
- **HAL.** A `chimera-hal` `Store` trait. The desktop simulator backs it with a directory. The functional core sees only bytes in and bytes out.

### Layout: 8.3 names, keyed by id (C2)

```
/CHIMERA/PROJECTS/P0000001.A  .B    a project; its name is in the header
/CHIMERA/SOUNDS/S0000001.A    .B    one library sound; its name is in the header
/CHIMERA/SNDINDEX                   the library index, a rebuildable cache
/CHIMERA/TAGS.A               .B    custom tag names
/CHIMERA/SYSTEM.A             .B    global settings
```

- Ids are assigned as the highest existing id + 1.
- Display names live in headers, so duplicate names are allowed. The save screen warns "NAME EXISTS" (§ Unsaved-state edge cases).
- The project list reads one header block per project.
- `SNDINDEX` sits outside `SOUNDS/`, so writing it doesn't change the directory it describes.

### A/B saves (C1)

Our FAT layer (ADR 0048) has no rename, and a FAT rename isn't atomic anyway. So:

1. Every file is a pair, `<ID>.A` and `<ID>.B`. The header carries a `u32` generation.
2. A save truncates and rewrites the file the reader would not take (the older one, or the missing or broken one) with generation + 1, then flushes.
3. The CRC32 goes in a **trailer**, so the writer streams and never seeks back.
4. The reader takes the valid file with the highest generation. A torn or invalid file is ignored, except one that needs newer firmware: an older file never shadows it.
5. A delete removes the older file first, then the newer one. A cut in between leaves one valid file, which reads as "not deleted".

A cut during FAT cluster allocation can leak clusters, which a computer's disk check repairs. It never touches the other file of the pair.

This assumes a 512 B block write is atomic: a cut leaves each block wholly old or wholly new, and never disturbs another block. SD cards don't promise it. The two files of a pair share a directory sector and, when small, FAT sectors, so a block torn there can lose both. ADR 0045 records the assumption, and plan 1's tests characterise the torn case.

### Format (H2)

- **Framing.** A header gives the magic, format version, kind, generation and display name. TLV records follow, then the CRC trailer.
  - Each record is a `u16` tag, a `u16` length and the bytes.
  - Tag bit 15 is **must-understand**. A reader that meets an unknown critical record lists the file greyed as "NEEDS NEWER FIRMWARE", and never loads it altered.
  - An unknown non-critical record is skipped by its length.
- **Code tables.** `BlockRef::disk_code()`, `ModSource::disk_code()` and a `disk_code` for every stored enum (`FilterKind`, `FilterMode`, `EnvForm`, `EnvType`, `LfoType`, `PartMode`, `DacPair`, `EngineType`, the Algo WAVE and ALG) are exhaustive `match` tables.
  - **Golden tests** pin the (block code, param id, spec name) triples and each enum's codes.
  - A retired-ids list stops any id from being reused.
- **Values.**
  - Enums are stored by frozen code, never by UI position. FilterMode is not stored as an index into `kind.modes()`.
  - Continuous values are stored as `f32` in their spec units.
- **Apply order.** Decode a whole block, then write it through `Block::write` in canonical spec order, KIND before MODE. A test pins that order.
- **Validation.**
  - Every value is clamped and quantised through its `ParamSpec`.
  - A NaN or infinite value is rejected, and its record takes the default.
  - An enum code out of range takes the default in a non-critical record, and greys the file in a critical one.
- **Rescaling.** A param that is rescaled or changes meaning gets a **new `ParamId`** and a pure `migrate(old) -> new`. The old id goes on the retired list.
- **Defaults.** Decoding starts from a frozen base: every param at its *neutral* default, no routes and an empty registry. It does **not** start from `Sound::init`, whose pre-wired routes may change. A param added after v1 has a neutral default that reproduces v1's sound, and a test enforces it against the fixtures.
- **Routes.** Every present route is written, zero amounts included (routing presence). A route is keyed by (source code, dest block code, dest param id).
- **Counts are bounded.** Routes are limited to `MAX_MOD_DESTS`, registry entries to `MAX_REGISTRY_DESTS`, and record lengths to the remaining file size. Past a bound, the file gets a clean error and is greyed.
- **Project records:** meta, the pool (sound records per filled slot), the six Parts (origin slot or INIT engine, then the mix), FX, and the custom tag names used.
  - Reserved: tempo, the AFX map per Part (spec 2), the CC map (spec 3) and the set list.
- **Fixtures.** A corpus of real v1 files is checked in under `chimera-core/tests/fixtures/v1/`. Every later version must load it and render it identically.

### Library index (M1)

- `SNDINDEX` is a cache. Its header holds a hash of the `SOUNDS/` directory listing (short name, size, first cluster) and the entry count.
  - At mount, the listing takes one linear directory scan. A mismatch rebuilds the index from the file headers, with a progress bar.
- A library save writes the sound file first, then `SNDINDEX`. A crash in between just triggers a rebuild.
- **RAM cap:** 512 entries × 28 B (id, name, tags, engine) ≈ 14 KB of AXI, counted in `AXI_RESIDENT`. Past it, LIBRARY FULL.
- **"Without lag"** means scrolling and filtering take less than one UI frame at 20 fps. This is all RAM work.

### Saving (M3)

- The serialiser is an iterator of ≤ 512 B chunks over the live `Project`, with a running CRC and a trailer. It needs no copy.
- The UI loop is blocked while it saves, so nothing mutates the project mid-stream. The audio keeps playing from its last snapshot.
- A save sets `meta.saved_crc`, and writes the project's id into SYSTEM as the last project.

### SYSTEM (M6)

- **Contents (owner's rule: only settings that exist).**
  - `ThemeSettings`: BRIGHT, GAMMA, ACCENT and BLACK;
  - the last project id.
  - Tuning (TUNE and SCALE are legacy placeholders, not wired), MIDI clock and thru don't exist yet. Each joins SYSTEM when it does, under a new record tag.
- **When it's written:**
  - on leaving System, only if the bytes changed;
  - on a project save or load, for the last project id.
- **Write rule.** SYSTEM is written exactly when RAM differs from what the card in the slot is known to hold, keyed by volume. Untouched defaults never go over a card's SYSTEM: leaving System loads that card's file instead, and creates it only when it has none.
- It uses A/B like every other file.
- This implicit save is the owner's ruling. "Saves are explicit" applies to projects and sounds.

### Errors reach the screen, never the audio

- With NO CARD, the ladder says so and play continues from RAM.
- A card error, a full card, a bad CRC or an unsupported format shows a message that names the file.
- A file that fails to parse, or needs newer firmware, is listed greyed with its error.
- The parser never panics (§ Tests).

## Loading while playing (C4)

**A project load** amends how ADR 0021's triple buffer is used:

1. **Validate.** Stream the file and check its CRC, structure, bounds and must-understand records. On error, RAM is untouched.
2. **Parse** into `ui.project` and **don't publish**. The audio keeps playing its last `AudioShared` snapshot.
3. **Bump** `LOAD_EPOCH`.
4. **Fade.** On a new epoch, the audio kills every voice. Each one fades over the existing 128-sample fade (about 2.7 ms) on its own `played` params, through the **old** mix and FX, because nothing new has been published. The audio then clears `waiting` and stores `LOAD_ACK = epoch`.
5. **Publish.** The UI waits for `LOAD_ACK` (≤ 10 ms timeout), then publishes the new snapshot. The gap is at most one block.

**FX at the swap.** The FX params switch with the new snapshot, through the FX bus's existing per-block smoothing where it has it. The delay and reverb buffers aren't cleared, so the old tails ring on under the new params and die away naturally. Plan 2 lists every FX param that would step under a live tail (delay time, for example) and either smooths it or records the accepted step in the ADR.

**MIDI during a load:**

| When | What happens |
|---|---|
| During the card read (steps 1–2, tens of ms) | Notes play on the old project, and are faded at step 4. |
| Between the epoch and the publish (≤ 1 block) | The audio stops draining the note queues, so note-ons wait and play on the new project. Queue order is kept, so a matching note-off still follows its note-on. |
| A note-off for a killed voice | Ignored. |
| A key held through the swap | It isn't retriggered, and stays silent until played again. |
| Pitch bend, mod wheel and other controller state | Kept. They belong to the player, not the project. |
| MIDI clock and thru | Not implemented. Nothing happens to them. |

**If the card fails in step 2** (the file validated, then the card was pulled mid-parse), UI RAM is partly overwritten. `ReplaceGuard` has already accepted losing the old state, so the UI falls back to a NEW project with the message "LOAD FAILED: <file>", and then publishes through steps 3–5.

**After any load,** the renderer's animators snap to the new values, as `snap_to_current` does on a KIND change (L6). This is CLAUDE.md's only exception to "never snap": the audible change is covered by the voice fade.

**Loading a sound into a Part** works as today, with the engine-switch fade, after `ReplaceGuard`.

## Boot (H5)

1. Mount the card and read SYSTEM, then apply THEME. With no card, no file or an error, the built-in defaults apply.
2. Load the last project id. Nothing is sounding yet, so the project is parsed and published without the fade.
3. With no id, no card, a fresh card (after the factory library is written) or any load error, boot starts a **NEW** project and shows the reason.
4. A NEW project is `Performance::new` plus a pool holding the factory Sounds and INIT Sounds, until spec 4. Its CRC is `template_crc`, so it is `Pristine` and never asks "save first?".

## Tests

- **Round trip:** Project → bytes → Project is bit-identical for every factory Sound and a full 32-slot pool.
- **Code tables:** golden triples and enum codes. A retired id is never reused.
- **Fixtures:** every v1 fixture loads and renders identically in every later version.
- **Compatibility:**
  - an unknown non-critical record is skipped;
  - an unknown critical record greys the file;
  - a missing record takes its neutral default;
  - a migrated param maps through `migrate`.
- **Fuzz (M9):** arbitrary bytes into the parser never panic and never yield a value outside its `ParamSpec`. This runs as a proptest in CI, plus a `cargo fuzz` target.
- **Corruption:** a truncated file, a bad CRC or a bad magic gives a clear error and leaves RAM unchanged.
- **A/B:** a simulated power cut at every block write of a save leaves the previous generation loadable.
- **Store fake:** a RAM block device and the desktop directory store pass the same suite, including a volume swap between operations.
- **Derived marks:**
  - an edit back to the slot's bits is `Clean`;
  - a save over a slot makes other unedited Parts `Stale`;
  - revert is bit-exact;
  - pool, FX and mix edits mark the project, and THEME doesn't;
  - a NEW project is `Pristine`.
- **ReplaceGuard:** every `Replace` variant prompts when the target isn't clean, and each answer does what it says. There are compile-fail tests for `Confirmed`.
- **Navigation:**
  - the `Location` property test (M7);
  - the `up` table;
  - every jump equals its `down` path;
  - tap versus hold at the threshold, with the tap acting on release;
  - no press is lost across a blocking operation;
  - the breadcrumb and legend are right in every `Location`;
  - screen goldens for each rung, the Part menu, naming, tags, POOL FULL, NO CARD, LIBRARY FULL and the empty library;
  - the all-pages walk.
- **Load protocol:** driven end to end with held notes: parse, epoch, ack, publish.
  - No sample step during the swap exceeds that of a 128-sample linear fade of the loudest voice (L7).
  - A fading voice's mod routes come from its own kept state.
  - A note-on arriving between the epoch and the publish plays on the new project.
  - No overrun.
- **Hardware (owner's check):**
  - SD bring-up (plan 1);
  - save, power off, power on: the same project loads;
  - a library of more than 200 sounds browses within one frame per step;
  - pulling the card mid-save leaves the previous generation;
  - a card swap is detected.

## ADRs

Numbers are provisional.

- **0043 Projects own their sounds.** Copies from library to pool to Part, derived Part and project marks, one `ReplaceGuard`, and explicit project saves.
- **0044 One ladder.** The `Location` tree, tap MENU = `up` (from pages, to the Part rung), hold MENU (600 ms) = System, the key table, and B1–B6 and EDIT+Bn as `down` compositions. It retires the MIX+Bn Mixer chain and System › MIDI Setup, and amends the UX spec. **Accepted before plan 2.**
- **0045 Card format.** 8.3 id names, A/B generations with a CRC trailer, frozen code tables, the must-understand bit, neutral defaults, migration by new `ParamId`, and the fixture corpus.
- **0046 Project load protocol.** `LOAD_EPOCH` and `LOAD_ACK` alongside ADR 0021's triple buffer, the MIDI rules and FX tails.
- **0047 Tags.** 16 built-in and 8 custom bits, with names that travel in files.
- **0048 Own FAT layer.** Our own FAT16/FAT32 file layer over `embedded-sdmmc`'s SD block driver (added by plan 1 after the Task 4 review).

## Out of scope

- **AFX mode (spec 2).** Per-key sounds on a Part, per-key pitch, and gliding between patches. See https://eu.novationmusic.com/articles/novation-afx-station/.
- **The MIDI CC matrix and MIDI learn (spec 3).**
- **The default project template (spec 4).**
- **Set lists and next-song preload.** The format reserves a record for set lists. Until then, you change song in a set from rung 0: EDIT on a project, which gives a ~3 ms fade and ringing tails.
- Program change, SysEx dumps, DX7 or PreenFM import, and user waveforms.

## Decisions awaiting owner review

1. **MENU (interpretation).** A tap goes up one rung and acts on release. From a sound's pages, it lands on the Part rung. A hold (600 ms, tunable) opens System from anywhere. The alternative is "a tap always jumps to the Project rung".
2. **One ladder.** B1–B6 are sugar for a Part's pages. The Mixer chain's PART and SENDS pages become rung 2, and MIX+Bn goes. FX is a row on the Project rung. Channels move to rung 2. System is a row on rung 0. Demo moves to System, in debug builds only.
3. **Keys.** SEQ saves on the ladder and stays sub-page up on pages. B on rung 3 picks the source (PROJECT first, then the library tags). The naming screen puts space on C and pre-fills a random name. MIX+MENU stays reserved.
4. **Load with held notes.** Every voice fades in about 3 ms, and the FX tails ring on under the new FX params. Note-ons between the epoch and the publish play on the new project. Held keys aren't retriggered.
5. **Boot.** The last saved or loaded project loads. A fresh card starts NEW and gets the factory library. An untouched project is `Pristine` and never prompts.
6. **Plans.** Storage foundation (SD bring-up first), then projects and the ladder, then library, tags and naming. The ladder ADR comes before plan 2.
7. **Derived marks.** A Part's `*` means it differs from its slot bit for bit. Stale comes from the per-slot generation. The project's `*` comes from its CRC. A project save doesn't clear Part marks. Stale marks aren't saved.
8. **ReplaceGuard** covers Part ← slot, Part ← library, `+ NEW` and project loads.
9. **Library editing.** Overwrite, rename, retag and delete, each behind a confirm. Duplicate names are allowed with a warning, for sounds and projects alike.
10. **Fresh card.** The firmware writes the factory Sounds and one INIT per engine. INIT sounds are library entries.
11. **Custom tags travel.** Their names are stored in files and merged on import, 8 at most. On overflow, the tag is dropped from the card copy with a message. A custom tag can be deleted only when unused.
12. **SYSTEM** holds THEME and the last project id. It is saved on leaving System (and on a project save or load).
13. **Next-song preload** is out of scope. A set-list record is reserved.
14. **MONO/POLY** is a Part property, on rung 2 as MODE.
15. **"Parts in use"** means Parts whose sound isn't bit-equal to its engine's INIT.
