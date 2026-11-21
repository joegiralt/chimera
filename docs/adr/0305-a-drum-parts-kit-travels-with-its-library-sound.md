# 0305. A DRUM Part's kit travels with its library Sound, and only a library load weighs it

- **Status:** Proposed
- **Deciders:** the owner's reviews of PR #1058 (2026-11-21: the design correction below is their ruling), firmware

## Context
A DRUM Part keeps its kit on the Part, beside its Sound (ADR 0265): `project::Kits`, in ITCM, saved with the project's Part records (`Kit`, then a `KitSound` a pad, ADR 0269, 0270). Two things followed, found in DRUM slice 1's review and verified on 0.4 dac1fa97 with host tests (private issue #949):

1. **A library save dropped the kit.** `encode_library_part` wrote the Sound and the kept map record only; `adopt` and `load_part` never wrote `kits`. A DRUM Part with all 12 pads changed, saved and loaded back, came out with 12 of 12 pads different from the source: the Sound landed on INIT's kit, or on whatever kit the target Part had.
2. **Nothing asked before a kit was lost.** `part_status` compared the Sound with its slot and nothing else.

A project's save and load were right.

What the code had:
- A library file is a Sound file (`FileKind::Sound`): `Engine`, `Build`, `Note`, the Sound's records, a Part's kept map record. `SoundCheck` refused `Kit` and `KitSound` there as a project's own records.
- A library load reads the file into the pool's scratch Sound (`read_library_sound`), maybe streams a source over several passes, then `adopt` stores the scratch in a slot and loads the Part from that slot. A refusal changes nothing.
- A pool slot holds a Sound's records and, closed, its map record. It holds no kit. SAVE PART FIRST saves to a pool slot.

### The first cut, and why it was wrong
This PR's first cut made a kit edit part of `part_status` and of `PartSource`'s guard: any replace of a DRUM Part with an edited kit asked SAVE PART FIRST. Both reviews failed it:
- A program change on a DRUM Part with a kit-only edit was dropped silently (the guard refused it).
- SAVE PART FIRST on a kit-only edit saved a new slot on every press and stayed Edited: a slot can't hold a kit.
- A kit edited on DRUM, the Part then switched to another engine: a DRUM library load replaced the dormant kit unasked, and an edit after the confirmation passed its witness.
- A project load marked each kit as safe, though the project's file is its only copy and the next save writes over it.

## Decision
The correction (the owner's ruling, 2026-11-21): **a kit lives on the Part and is touched only by a library load of a DRUM Sound. So the kit is at risk in exactly one operation, and the guard weighs it there and nowhere else.**

### The file
1. **The library file carries the kit as records after the Sound's.** A DRUM Part's save writes, after `encode_library_part`'s records, the project's own `Kit` record (157 B) and 12 `KitSound` records (58 B each) through `put_kit` and `put_kit_sounds`: 905 B a file with their heads. No new tag, no new file kind, no version.
2. **What a save writes is a type.** `PartSave::{Drum { sound, kit }, Sound(sound)}`, built by the one total `Project::part_save` from the Part's engine: no save can pair another engine's Sound with a kit. `save_library_sound` (a Sound with no Part) is `PartSave::Sound`.
3. **A closed door is a value.** The kit's writer answers `KitPut::{Written(KitCrc), NotDrum, Closed}`. A DRUM Part's save with bank 2 closed is refused whole and its file deleted (`LibNote::KitClosed`, `KIT NOT SAVED · NEEDS UPDATE` on the error line, no SAVED toast): its file would hold no kit under a kit's name.
4. **A save returns its proof.** `save_library_part` returns `SavedPart { part, id, kit: Option<KitMark> }`, the mark made from the CRC of the records written; `Project::library_saved(SavedPart)` (which was `reset_shift`) consumes it. Nothing else marks a kit as the library's.

### The read
5. **Kit records are checked in pass 1 and written in pass 2.** `PartDecoder` (the `LibraryDecoder`, the scratch kit and a `KitScan`) sends every event through `KitDoor::scan`, with no target in pass 1; `PartCheck` does the same for a save's read-back. So a malformed kit is refused with its own error (`Bounds`, `Corrupt`) before anything is written; a failure in pass 2 alone would read `BadCrc`.
6. **One accumulator, the project decoder's.** `codec::KitRead { kit, pads }` is what `ProjectCheck` kept as two fields: a second `Kit` or a pad's second `KitSound` is `Corrupt`, the wrong length or a pad there is none of `Bounds`. The library adds its own: a kit record in a file whose `Engine` is not DRUM's, or before `Engine`, is `Corrupt`; `KitSound`s with no `Kit` are a kit in part, `Corrupt`. `scan` names every `RecordTag`: a new tag is weighed there before it compiles. A value this firmware reads as another counts as one of the read's skips (`SKIPPED`).
7. **A read answers `FileKit::{Absent, Whole(pads)}`.** A `Kit` whose pads lack their own `KitSound` is whole: those pads take the Sound's FILTER and AMP at the landing (ADR 0269, `codec::fill_pad_sounds`, shared with the project decoder). With the door closed the read says `ReadKit::Closed`.
8. **A read is the only thing `adopt` takes.** `read_library_sound(.., go: Confirmed<LibraryKit>) -> ReadPart`, and `Project::adopt(ReadPart, staged)`. `ReadPart` is not `Copy`; it holds the confirmation, the `LibRead`, the file's kit and the scratch's epoch, which every hand-out of the scratch to a writer moves (`Pool::scratch`, `scratch_kept`). A kit or Sound from another file or an earlier read can't land (`AdoptError::Changed`). A project load can't run while a library load's source streams (the UI's card jobs exclude each other: `a_pending_load_blocks_every_other_card_start`); the epoch holds without that.
9. **The landing is one exhaustive match on the Sound's engine and the file's kit** (`kern::land`): DRUM with a whole kit takes the scratch kit, fills its missing pad sounds and marks it; DRUM with none takes `Kit::DEFAULT`, never the target's stale kit; any other engine leaves the Part's kit (every Part has one, and an engine switch keeps it, ADR 0265).
10. **A file with no kit loads the default kit.** A DRUM library Sound from before this ADR, or a Sound's own save, has no kit records. No user has such a file (DRUM is 0.4's); it must load safely, not sound the same.

### The guard
11. **`PartSource` does not weigh the kit.** A slot load, INIT, RELOAD, an engine switch and a MIDI program change replace the Sound and leave the kit: they ask about the Sound alone (`sound_status`), as before this PR.
12. **A library load asks twice, in types.** `LibraryPart` is the Sound's question, as `PartSource`'s. Its confirmation loads nothing: `ReplaceGuard::kit(Confirmed<LibraryPart>)` makes the only `LibraryKit`, whose confirmation is what `read_library_sound` takes. So the Sound's SAVE PART FIRST flow runs first and whole, then the kit's question.
13. **The kit is at risk** when the file may be a DRUM Sound's (the listing's peeked engine, carried in `LibFile.engine`; unknown counts) and the Part's kit is neither `Kit::DEFAULT` nor the kit its mark says the library holds: whatever engine the Part plays now, and whatever loaded the kit. `KitSeen::{Closed, Default(crc), InLibrary(crc), Unsaved(crc)}` says which.
14. **Its prompt is `Prompt::KitUnsaved`: `LOAD INTO P1?` / `KIT NOT IN LIBRARY`, REPLACE or CANCEL** (`KitAnswer`), opening on CANCEL. Never SAVE PART FIRST: a slot can't save a kit. To keep the kit, cancel and SAVE TO LIBRARY.
15. **`LibraryKit`'s witness always covers the kit:** the Part's own witness, then `KitSeen`'s tag and CRC, never empty. `Confirmed<LibraryKit>::still` also holds the Part to the witness the Sound's question confirmed. An edit of the Sound or the kit after either answer refuses the adopt, in every engine state.
16. **`KitMark` is the library's copy, and only the library sets it:** `Option<KitCrc>` in one representation, set by a library load's landing and by `library_saved`. A project load sets none: a kit that came from a project is not in the library until it is saved there. `KitCrc` is its own type; a Sound's CRC can't stand in for it.

### The status
17. **`SoundStatus` and `PartStatus` are two types.** `sound_status(&Part, &Pool) -> SoundStatus` is what `part_status` was, and every caller that asks what a slot can answer takes it by type (the guards, Part actions, a save's Stale Parts, `differ`, a program change, KEEP MAP, the UI's Part mark). `PartStatus::{Clean, Edited(Edits), Stale(slot)}` with `Edits::{Sound, Kit, Both}` is made only by the total `PartStatus::of(SoundStatus, KitState)`; `part_status(&Project, PartId)` reads it, and `test_support::same` compares it. No guard reads it. The UI's Part mark stays the Sound's for now: a `*` no SAVE TO PROJ can clear would mislead until slots hold kits (#1050).

### Flash
18. **Bank 2, behind `KitDoor`.** The kit's records, scan, CRC, mark and landing are `project::kit_library::kern`, an S4 group. The door's own table is full (ADR 0283), so `KitDoor` (a mark and five entries: `put`, `scan`, `found`, `land`, `seen`, 24 B) has its own slot in the S4 groups' tables, a stride after the compressor KEY's (`KIT_DOOR_ADDR` 0x08180450, four strides' room, `KIT_DOOR_ROOM`), checked at boot with the others (`kit_door_ok`). The kit's four coders, `KitRead` and `fill_pad_sounds` are inlined: a project's codec keeps its one copy in bank 1 and `kern` has its own in bank 2.
19. **`SoundCheck` passes over `Kit` and `KitSound` after `Engine`,** as it does `Build` and `Note`, in every Sound-kind file. Narrowing that to a library Part's file would take a mode in `SoundCheck` and a second constructor at each of its generic users (the console's `put`, its delete and its pair judge name `SoundCheck` by type), all bank 1's. The library's own readers (`PartDecoder`, `PartCheck`) check every kit record; a pool slot's or a project `Slot`'s decode skips one, and nothing writes one there.

## Alternatives considered
- **The kit in every guard and in the Part's status** (this PR's first cut): see above.
- **The kit inside the pool's slots** (a slot's entry ends with the kit's records): then a DRUM Sound is one thing in a slot, the library and a project, and SAVE PART FIRST could hold a kit. It changes the pool's entry, its CRC and `matches`, the project file's `Slot` context, pass 1's costs, a program change's recall copy and `ed`, all bank 1's: private issue #1050.
- **A slot or INIT load resets the kit to the default.** RELOAD and a program change would wipe a kit no slot can give back.
- **A project load settles the kit as safe.** The project's file is its only copy then, and the next save writes over it.
- **SAVE PART FIRST on the kit's prompt.** It saves to a slot; a slot holds no kit.
- **A new record tag, or a `Kit` file kind.** The project's records already say it, with their decoders and lossy rules.
- **The kit's mark as a copy, not a CRC.** 964 B a Part against 8 B.
- **Decoding the kit straight onto the Part at the read.** The read comes before the budget and a streamed source; `adopt` may still refuse.
- **A closed-door DRUM save that writes the Sound alone.** A file named for a kit that loads the default kit.

## Consequences
- A DRUM Part's library file is 905 B longer. A 0.4 build from before this ADR refuses the new files as `Corrupt`; none shipped.
- A kit loaded from a project asks before a DRUM library load even if it once came from the library: the mark is not stored. Saving it to the library answers it.
- `Kits` grows by the scratch kit (964 B) and six marks (48 B): 5,828 B to 6,840 B, in ITCM (ADR 0266, 0286), with the margin kept. `Pool` gains the scratch's epoch (2 B).
- `part_status`'s old meaning is `sound_status` and its type `SoundStatus`; its test uses were renamed.
- FLASH_DELTA

## Sources
- private issue #949 and its verification comment (2026-11-20); PR #1058's two reviews and the ruling on them (2026-11-21).
- ADR 0265 (the kit on the Part), 0269 (pad sounds), 0270 (TONAL's byte), 0087 (the pool), 0198 and 0251 (bank 1 frozen), 0266 and 0286 (ITCM), 0283 (the S4 tables), 0303 (the table before this one).
- `chimera-engine/src/project/kit_library.rs`, `kit_library/kern.rs`, `library.rs`, `marks.rs`, `guard.rs`, `codec.rs`; `chimera-core/tests/kit_library_test.rs`, `kit_library_closed_test.rs`, `library_ui_test.rs`, `midi_program_test.rs`.
