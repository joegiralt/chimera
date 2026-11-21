# 0305. A DRUM Part's kit travels with its library Sound, and a kit edit makes the Part Edited

- **Status:** Proposed
- **Deciders:** firmware (#949, verified on 0.4 dac1fa97); the owner's review decides

## Context
A DRUM Part keeps its kit on the Part, beside its Sound (ADR 0265): `project::Kits`, in ITCM, saved with the project's Part records (`Kit`, then a `KitSound` a pad, ADR 0269, 0270). Two things followed, both found in DRUM slice 1's review and verified on 0.4 dac1fa97 with host tests (private issue #949):

1. **A library save dropped the kit.** `encode_library_part` wrote the Sound and the kept map record only; `adopt` and `load_part` never wrote `kits`. A DRUM Part with all 12 pads changed, saved and loaded back, came out with 12 of 12 pads different from the source: the Sound landed on INIT's kit, or on whatever kit the target Part had.
2. **A kit edit was invisible to the replace guard.** `part_status` compared the Sound with its slot and nothing else, so a Part with only its kit edited read Clean and a load or replace went ahead with no SAVE PART FIRST prompt.

A project's save and load were right (0 of 12 pads differ).

What the code had:
- A library file is a Sound file (`FileKind::Sound`): `Engine`, `Build`, `Note`, the Sound's records, a Part's kept map record. `SoundCheck` refused `Kit` and `KitSound` there as a project's own records (`Corrupt`).
- A library load reads the file into the pool's scratch Sound (`read_library_sound`), maybe streams a source over several passes, then `adopt` stores the scratch in a slot and loads the Part from that slot. A refusal changes nothing.
- A pool slot holds a Sound's records and, closed, its map record. It holds no kit.
- SAVE PART FIRST saves to a pool slot (`part_actions`), then asks the guard again.

## Decision
1. **The library file carries the kit as records after the Sound's.** A DRUM Part's save (`save_library_part`) writes, after `encode_library_part`'s records, the project's own `Kit` record (157 B) and 12 `KitSound` records (58 B each), through `put_kit` and `put_kit_sounds`: 905 B a file with their heads. A Part on any other engine writes none, and neither does `save_library_sound` (a Sound with no Part has no kit). No new tag, no new file kind, no version.
2. **A Sound file passes over kit records.** `SoundCheck` treats `Kit` and `KitSound` after `Engine` as it treats `Build` and `Note`: not the Sound's, any repeat, any length. So every reader of a Sound file (the save's read-back, the console's `put`, a delete's check, a slot's decode) takes the new files. Before `Engine` they are `Corrupt`, as any record is.
3. **A library load reads them onto a scratch kit.** `read_library_sound` decodes through `PartDecoder`: the `LibraryDecoder` and the scratch kit (`Kits::scratch_mut`, a seventh kit after the Parts', which no `PartId` reaches). In pass 2 the file's header resets the scratch kit to `Kit::DEFAULT`, and each kit record lands on it through the project's decoders (`get_kit`, `get_kit_sound`); the wrong length or a pad there is none of is `Bounds` and fails the read. Pass 1 leaves kit records to `SoundCheck`: a bad one is found in pass 2, where only the scratch is written, and only `adopt` reads the scratch.
4. **Adopting a DRUM Sound replaces the Part's kit** with the scratch kit, after the Part's load, so a refusal still changes nothing. A Sound of another engine leaves the Part's kit as it is (ADR 0265: every Part has one, and an engine switch keeps it).
5. **A file with no kit loads the default kit.** A DRUM library Sound from before this ADR, or one a Sound's own save wrote, has no kit records: the scratch kit is still `Kit::DEFAULT`, and that lands. It does not keep the target Part's kit: a stale kit under a loaded Sound's name is the bug. No user has such a file (DRUM is 0.4's); it must load safely, not sound the same.
6. **Each kit has a mark, and a DRUM Part off its mark is Edited.** `KitMark` (`Init`, or `At` the kit's CRC, the CRC of its records) is set to the kit as it is by what loads or holds it: NEW (`Init`), a project load (each Part's kit as read), a library load's adopt, and a DRUM Part's library save (`Project::library_saved`, which was `reset_shift`: the UI calls it after a save, as before). `part_status(&Project, PartId)` is the Sound's status, and `Edited` too while the Part plays DRUM and its kit's CRC is not its mark's. Turned back to the marked values it is Clean again. A Part on another engine never reads its kit: out of play, it asks nothing.
7. **The Sound's own status keeps its meaning under a new name.** `sound_status(&Part, &Pool)` is what `part_status` was. What a slot can answer weighs it: `part_actions` and `apply_part_action`, a save's Stale Parts, a project save's `differ`, a program change and KEEP MAP. The Part's `*` (`PartMark`) and the replace guard weigh `part_status`.
8. **The guard's witness holds the kit.** `Target for PartSource` hashes `KitSeen`'s witness (the kit's CRC, inverted while it is edited; 0 for a Part not on DRUM) after the Sound's CRC and Origin: a kit edit between the confirmation and the replace refuses it (`Changed`). `LibraryPart` shares both.
9. **A slot holds no kit, and nothing here pretends it does.** A slot load, INIT and a revert leave the Part's kit as it is (ADR 0265, unchanged). A save to a slot saves the Sound and leaves the kit's mark: a kit-edited Part stays Edited after SAVE TO PROJ, and SAVE PART FIRST on a kit edit saves the Sound, then asks again (REPLACE or CANCEL). Only a library save, which holds the kit, clears it. That is the honest state until slots carry kits: private issue #1050.
10. **Bank 2, behind `KitDoor`.** The kit's records, CRC, mark, landing and the DRUM test are `project::kit_library::kern`, an S4 group. The door's own table is full (ADR 0283), so `KitDoor` (a mark and six entries: `put`, `read`, `land`, `settle`, `saved`, `seen`, 28 B) has its own slot in the S4 groups' tables, a stride after the stereo's (`KIT_DOOR_ADDR` 0x08180440, two strides' room, `KIT_DOOR_ROOM`), checked at boot with the others (`kit_door_ok`). Bank 1 gains the door's calls and the boot's check. Closed, a save writes no kit, a load reads none and leaves the Part's kit, and no kit edit is seen: as before this ADR.

## Alternatives considered
- **The kit inside the pool's slots** (a slot's entry ends with the kit's records, as it ends with a kept map record): the whole answer, since a DRUM Sound would then be one thing in a slot, the library and a project, and SAVE PART FIRST would hold the kit. It changes the pool's entry, its CRC and `matches`, the project file's `Slot` context, pass 1's costs, a program change's recall copy and `ed`, all bank 1's. Too wide for this fix; it is #1050, and this ADR's records and marks carry over to it.
- **A slot or INIT load resets the kit to the default.** Uniform ("a DRUM Sound's load brings its kit, or the default"), but RELOAD and a program change would wipe a kit no slot can give back.
- **The kit as the project's alone, like the Part's mix: no Part status.** Then a library load, the one thing that replaces a kit outside a project load, would replace it unasked.
- **A slot save clears the kit's mark.** The prompt would close after SAVE PART FIRST and the library load would then discard a kit saved nowhere.
- **A new record tag, or a `Kit` file kind.** The project's records already say it, with their decoders and lossy rules; a second encoding is a second thing to keep right.
- **The kit's baseline as a copy, not a CRC.** 964 B a Part against 8 B.
- **Decoding the kit straight onto the Part at the read.** The read comes before the budget and a streamed source; `adopt` may still refuse.

## Consequences
- A DRUM Part's library file is 905 B longer. A released firmware (0.3 and before) has no `Kit` tag: it skips the records as unknown ones and loads the Sound. A 0.4 build from before this ADR refuses the new files as `Corrupt`; none shipped.
- A `Kit` or `KitSound` record in a project's `Slot` context is now passed over instead of refused. #1050 will read it.
- `Kits` grows by the scratch kit (964 B) and six marks (48 B): 5,828 B to 6,840 B, in ITCM (ADR 0266, 0286), with the margin kept (`itcm.x`'s assert; `memory_budget_test`).
- A Part's status costs one kit CRC (two while its mark is `Init`) for a DRUM Part, about 0.9 KB hashed, on the paths that already hash the Sound.
- `part_status`'s signature changed; its 69 test uses that mean the Sound's status are `sound_status` now.
- A project load marks each kit as loaded: a kit edited before the project's save reads Clean after its load, where a Sound edited from its slot still reads Edited. The kit has no slot to differ from until #1050.
- **Bank 1 grows by 460 B on `orbit`, an ADR 0198 exception.** What it gains is the door's side only: `KitDoor`'s calls (a save's `put`, a load's `read` and `land`, a project load's `settle`, `library_saved`, the one out-of-line `seen` the status and the witness share), `part_status` over `sound_status`, the boot's `kit_door_ok`, the marks' build and reset, and the scratch kit as a seventh in the kits' one loop. The kit's four coders are inlined (`put_kit`, `get_kit`, `put_kit_sounds`, `get_kit_sound`): a project's codec keeps its one copy in bank 1 as before, and `kern` has its own in bank 2. A first cut cost 956 B; the DRUM test, the scratch's reset and the witness's fold moved behind the door. Bank 1 on `orbit` is 906,444 B, 11,060 B under the stock bootloader's SD-flash limit (917,504 B).
- **Bank 2 grows by 1,568 B on `orbit`,** all in S4: 586,944 B used, S4-S5 191,360 B free; S0-S3 unchanged at 7,072 B free.

Every set against origin/0.4 dac1fa97 (its budget lines are its measure: checked on `orbit`, `default`, `bank2-gate` and `fault-inject`), the lines raised by exactly these:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---:|---:|
| orbit | +460 | +1,568 |
| default | +520 | +1,600 |
| bank2-gate | +504 | +1,568 |
| orbit,usb-config-writer | +476 |  |
| fault-inject | +304 |  |
| bench-algo | +316 |  |
| bench-algo,master-tape | +300 |  |
| bench-routing | +308 |  |
| bench-memory | +328 |  |
| bench-stress | +320 |  |
| bench-orbit | +308 |  |
| bench-tape | +324 |  |
| filter-probes | +304 |  |

## Sources
- private issue #949 and its verification comment (2026-11-20); the verifier's host tests.
- ADR 0265 (the kit on the Part), 0269 (pad sounds), 0270 (TONAL's byte), 0087 (the pool), 0198 and 0251 (bank 1 frozen), 0266 and 0286 (ITCM), 0253 (KEEP MAP's status).
- `chimera-engine/src/project/kit_library.rs`, `kit_library/kern.rs`, `library.rs`, `marks.rs`, `guard.rs`, `codec.rs`; `chimera-core/tests/kit_library_test.rs`, `kit_library_closed_test.rs`.
