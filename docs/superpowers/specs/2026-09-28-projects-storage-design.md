# Projects and Storage: Design

Spec 1 of 4 in the projects roadmap:

1. **Projects and storage.** This spec.
2. **AFX mode.** Per-key sounds on a Part.
3. **The MIDI CC matrix.** A "what does CC N do?" view.
4. **The default project template.**

Specs 2–4 are out of scope here. This spec only reserves room for them in the data model and the file format.

## Intent

Chimera is multitimbral, and today nothing it plays can be saved. Turn the synth on and you rebuild your setup from scratch. This spec makes the **project** the unit you work in and perform with:

- A project is everything musical the synth is doing: its sounds, its six Parts and the shared effects.
- You save it to the SD card and load it back.
- You can move from one song to the next during a set.

**Design principle (owner's words).** "There's only really one way to do things, and everything else is sugar on top." Everything is reached down one ladder:

- **Projects › Project › Part › Sound**, then the sound's own pages.
- EDIT goes down a rung and MENU goes up.
- A breadcrumb always says where you are.
- The synth must explain itself on screen, with no manual needed.

**Done when:**

- A project can be saved to the card, the unit power-cycled, and the project loaded back, bit-identical.
- Sounds can be saved to and picked from a tagged library on the card.
- Every screen on the ladder is covered by host tests and screen goldens, and has been played on the chip.

## Model

| Thing | What it is | Owns |
|---|---|---|
| **Sound** | A patch | Name, tags, engine, all engine and chain parameters, modulation routes. No MIDI, no mix. |
| **Pool** | The project's sounds: 32 numbered slots in RAM | A copy of each Sound. Slot numbers are stable, so AFX mode (spec 2) can refer to them. |
| **Part** | A player | Its own copy of a Sound, the pool slot it came from (`loaded_from`), a *modified* flag, and its mix: MIDI channel, volume, pan, output pair and sends. |
| **Project** | The whole musical state | Name, the pool, the six Parts and the shared FX. Later also tempo, AFX maps (spec 2) and the CC map (spec 3). |
| **Library** | Every Sound on the SD card | Sounds only. It is browsed by tag and filtered by engine. |
| **System (global)** | The synth's own setup, never part of a project | THEME, master tuning, MIDI clock in/out and thru. |

### Copy rules (owner's decisions)

- **Library → Pool** is a copy. A project owns its sounds. Later edits to the library never change a project.
- **Pool → Part** is a copy. A Part edits its own copy, and the Part is marked `*` once it differs from its slot.
- **Part → Pool.** From an edited Part you can:
  - save it over its slot;
  - save it to a new slot;
  - revert it from its slot.
- **Part or Pool → Library.** Save as a new library sound, then name and tag it.
- **Saving a project is explicit.** The header shows `*` when anything differs from the saved file. Loading another project with unsaved changes asks first: **SAVE THEN LOAD**, **LOAD ANYWAY** or **CANCEL**. Nothing is ever lost silently.
- **Saving over a slot other Parts use.** Those Parts keep their own copies, which stops a "one slot, many Parts" change from being silent. After the save, every other Part loaded from that slot with an unmodified copy is shown stale (`◦`) until it reloads. The save screen names them: "P4 also uses slot 03: UPDATE P4 / LEAVE".

The code already has `Sound`, `SoundPool` (32 slots), `Part { sound, loaded_from, mix }` and `Performance { parts, fx }`. This spec keeps them:

- `Performance` becomes the saved project.
- It gains a name and a *modified* state.
- `Part` gains the *modified* and *stale* marks.
- `Sound` gains a tag set.

## Screens: the ladder

Every rung is a page in the existing page system. Its layout, region keys and redraws all work as they do on other pages. The breadcrumb sits in the header, for example `ACID PARTY › P4 ›`.

### Rung 0: PROJECTS

- The projects on the card, with `●` marking the loaded one and `+ NEW` at the end.
- **A** scrolls, **EDIT** opens the project, **SEQ** saves as a new name, and **MENU** goes back to rung 1.
- Opening a project with unsaved changes shows the unsaved-changes prompt.

### Rung 1: PROJECT (hold MENU from anywhere)

- **Header:** the project name, with `*` when it is modified.
- **A stats strip:**
  - PARTS in use (a Part is in use when it has a sound);
  - VOICES, the voice cap;
  - POOL (slots used out of 32), with a bar;
  - CPU (live load from `AudioStats`), with a bar.
- **One row per Part:**
  - the name (`*` if modified, `◦` if stale) and the MIDI channel;
  - a layer mark when another Part shares its channel.
- **A** moves between Parts, **EDIT** opens the focused Part (rung 2), **SEQ** saves the project, and **MENU** goes up to rung 0.
- Holding MENU today opens System. System moves to MENU + PLUS, the way the MIX chains are reached. The owner decides the exact gesture while the plan is written.

### Rung 2: PART

- **Rows:**
  - SOUND (the slot number and name, with `›`);
  - CHANNEL;
  - LAYER, which lists the other Parts on the same channel (read-only);
  - VOLUME and PAN;
  - OUT and SENDS;
  - VOICES, how many voices this Part's sound can get by the cost model (read-only).
- **A** chooses a row. **EDIT** on SOUND goes to rung 3, and EDIT on any other row edits it. **SEQ** opens the Part's save menu. **MENU** goes up.
- **Save menu:** SAVE OVER SLOT n, SAVE TO NEW SLOT, SAVE TO LIBRARY and REVERT TO SLOT n. Options that don't apply are hidden. For example, REVERT is hidden when the Part is not modified.

### Rung 3: SOUND

- **Two tabs, switched with B:**
  - **PROJECT:** the pool as a numbered list (slot, name, tags, and which Parts use it; empty slots show `—`).
  - **LIBRARY:** the card, filtered by tag and engine.
- **In PROJECT:**
  - **EDIT** on a slot loads it into the Part. EDIT on the Part's own current sound goes into its pages (ALG, FLT, MOD…).
  - **SEQ** on an empty slot opens LIBRARY to fill that slot.
  - **MIX+MINUS** clears a slot, after confirming. A slot a Part is using can't be cleared.
- **In LIBRARY:**
  - The tag row sits at the top: ALL, the built-in tags, then the custom tags. **B** picks the tag.
  - **C** filters by engine: ALL, ALG or MDL.
  - **A** scrolls.
  - **EDIT** copies the sound into the target slot and loads it into the Part. The target is the slot LIBRARY was opened for; otherwise it is the first free slot.
  - If the pool is full, show POOL FULL and say which slots the project isn't using.
- **Sugar:** EDIT + B1–B6 jumps straight to rung 3 for that Part. It replaces today's browser, and nothing is reachable only through it.

### Naming and tags

- **The naming screen is "one job per encoder":**
  - A moves the cursor;
  - B cycles A–Z;
  - C cycles 0–9 and `-`;
  - D toggles case;
  - E deletes;
  - F goes to the tags.
- Name length is `NAME_LEN` (16). **SEQ** proposes a random name, **EDIT** goes next, and **MENU** cancels.
- **Tags** are chips:
  - The built-in set: BASS, PAD, KEYS, LEAD, PLUCK, STAB, PERC, FX, DRONE, ARP, ACID, DUB, DARK, BRIGHT, MONO and POLY.
  - Up to 8 custom tags follow them (drawn dashed), then `+ NEW`, which names a custom tag with the same naming screen.
  - **F** moves between chips, **PLUS** toggles one, and **EDIT** saves.
- A Sound stores its tags as a bit set: 16 built-in bits plus 8 custom bits. Custom tag names live in the card's `TAGS` file. Renaming a custom tag renames it everywhere.

## Storage

- **Card.** The SD card on SPI2, formatted FAT32 so a computer can read it for backups and sharing. Access uses `embedded-sdmmc` (no_std, no heap), behind a `chimera-hal` storage trait so the desktop simulator can use a directory on disk. The functional core only sees bytes in and bytes out.
- **Layout:**

  ```
  /CHIMERA/PROJECTS/<name>.CPR   a project: pool, parts, fx, name
  /CHIMERA/SOUNDS/<id>.CSD       one library sound
  /CHIMERA/SOUNDS/INDEX          name, tags and engine per sound, for fast browsing
  /CHIMERA/TAGS                  custom tag names
  /CHIMERA/SYSTEM                global settings
  ```

- **Format.** A versioned, tagged binary, pure Rust in `chimera-core`:
  - A header gives the magic, the format version and a CRC32.
  - Records follow: a tag id, a length, then bytes. A reader skips tags it doesn't know and defaults tags that are missing.
  - A Sound is written by `ParamAddr`: the block and param id with its value, not a raw memory dump. So a new parameter or a reordered struct never breaks old files, and a removed parameter is ignored.
  - Room is reserved as unknown-to-this-version record tags: AFX map per Part (spec 2), CC map (spec 3), tempo and set list.
  - FAT 8.3 names: a sound's file is its id, and its display name is inside the file.
- **Crash-safe saves.** Write `<name>.TMP`, check its CRC, then rename it over the old file. A power loss mid-save leaves the previous file intact.
- **Errors reach the screen, never the audio:**
  - NO CARD: the project and library pages say so, and play continues from RAM.
  - A card error, a full card or a bad CRC shows a message naming the file.
  - A file that fails to parse is listed but greyed, with its error.
- **System settings** (THEME, tuning, MIDI clock and thru) are written to `SYSTEM` when you leave the System chain. They are read at boot. With no card or no file, the built-in defaults apply.

## Loading while playing

- **Card I/O runs in the UI loop, never on the audio path.** The audio callback never waits on the card.
- **Loading a project:**
  1. Parse it into a staging `Performance` in AXI RAM.
  2. Every held note fades out, using the existing 128-sample voice fade.
  3. At the next block boundary the new `Performance` is swapped in through `AudioShared`.
  4. There is no click, and there is no gap longer than the fade.
- **Loading a sound into a Part** works as today, with an engine-switch fade if the engine changes.
- **Saving never touches audio.** Only the serialised copy is written, so you can save while playing.
- **RAM.** The staging copy doubles the size of `Performance` for the duration of a load. The plan must measure `size_of::<Performance>()` and the pool, and place the staging buffer where there is room (AXI has about 91 KB free), or stream the parse Part by Part. It must never touch D2, which holds the voices.

## Tests

- **Round trip:** Performance → bytes → Performance is bit-identical for every factory Sound and a full 32-slot pool.
- **Forward and backward compatibility:**
  - an unknown record is skipped;
  - a missing record takes its default;
  - a file with a removed param loads.
- **Corruption:** a truncated file, a bad CRC and a bad magic each give a clear error and leave the project in RAM unchanged.
- **Crash-safe save:** a simulated power cut between the temp write and the rename leaves the old file readable.
- **Storage fake:** a RAM-backed block device and a directory-backed desktop store both pass the same suite.
- **Copy rules:**
  - editing a Part marks it `*` and leaves its slot untouched;
  - saving over a slot marks other Parts stale;
  - revert restores the slot's sound bit for bit.
- **Unsaved-changes prompt:** each of its three answers does what it says.
- **UI:**
  - every rung is reachable, and every navigation test asserts its landing page;
  - EDIT/MENU moves up and down the ladder;
  - the breadcrumb is right on every rung;
  - screen goldens cover each rung, the save menu, naming, tags, POOL FULL and NO CARD;
  - the all-pages walk passes.
- **Loading while playing:** a project swap with held notes produces no click (the fade is checked sample by sample) and no overrun.
- **Hardware (owner's check):**
  - save, power off, power on and load gives the same sound;
  - a full library of more than 200 sounds browses without lag;
  - pulling the card mid-save leaves the old file intact.

## ADRs

- **Projects own their sounds.** Library → pool → Part are copies, and a project is saved explicitly.
- **One ladder, Projects › Project › Part › Sound.** EDIT goes down, MENU goes up, and hold MENU opens the project. It moves System off plain MENU; no accepted ADR fixes that gesture today.
- **Card format.** FAT32, tagged records keyed by `ParamAddr`, CRC, and save by temp-and-rename.
- **Tags.** A bit set of 16 built-in and 8 custom tags.

## Out of scope

- **AFX mode (spec 2).** Per-key sounds on a Part, per-key pitch, and gliding between patches. See https://eu.novationmusic.com/articles/novation-afx-station/.
- **The MIDI CC matrix and MIDI learn (spec 3).**
- **The default project template (spec 4).** Until then, a new project's pool holds the factory Sounds and the INIT Sounds.
- **A set list or next-song gesture.** The owner hasn't decided yet; the file format reserves a record for it.
- Program change, SysEx dumps, DX7 or PreenFM import, and user waveforms.
