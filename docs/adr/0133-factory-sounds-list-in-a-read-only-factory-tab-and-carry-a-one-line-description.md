# 0133. List factory Sounds in a read-only FACTORY tab and carry a one-line description

- **Status:** Accepted (2026-10-11)
- **Deciders:** owner, firmware lead
- **Amends:** [0119](0119-a-sound-library-on-the-card.md)

## Context
The showcase set (spec `2026-10-11-showcase-library-design.md`) puts every factory Sound on the card, 0 B of flash. Mixed into LIBRARY they would look like the player's own Sounds, could be overwritten by a save, and say nothing about what each one teaches. 0119 reserved `S9000001` and up for factory ids (the TAPE set uses `S9000001`-`S9000032`) but gave them no tab, no type and no description.

## Decision
- **Note record.** `RecordTag::Note` is `0x001D`, not critical. Its payload is 1-40 B of `A`-`Z`, `0`-`9`, space and `. , ' - + / : ( ) & %`, and must fit `CONTENT_W` in `FONT_LABEL` (the host tool checks it with `draw::text_width`). It is written third (`Engine`, `Build`, `Note`) so the listing's peek reaches it, and `LIB_PEEK` grows from 64 to 128 B (28 + 5 + 25 + 4 + 40 = 102). `Note::new` is the only way to build one.
- **Decoding.** A `Note` is known to this build and skipped like `Build`: never decoded into `Sound`, never counted as skipped, ignored when not third, second or past 40 B, `Corrupt` before `Engine`. An older build skips it as an unknown non-critical record and shows `LOADED · SOME PARAMS SKIPPED`. It is never written in a project and is skipped in every project context.
- **Id blocks.** User `S0000001`-`S8999999`; TAPE factory `S9000001`-`S9099999`; showcase `S9100001`-`S9199999`. `LibId` splits into `UserId` and `FactoryId`; the library writer takes `UserId` only, so a save cannot name a factory file.
- **FACTORY tab.** Encoder B turns PROJECT, LIBRARY, FACTORY. FACTORY lists only `FactoryId` files, lowest id first, read-only (EDIT loads; saves write nothing), and shows the selected row's description on line 2. LIBRARY lists only `UserId` files.
- **Demo project ids.** `P0000001`-`P0000009` are reserved for the factory demo projects; a project save allocates `max(top, 9) + 1`.

## Alternatives considered
- **Description decoded into `Sound`:** costs pool arena and Part RAM for text that only the browser reads. Rejected; a later build can carry it if players ask.
- **A sidecar index file:** a second file to keep in step with the Sounds. Rejected; the peek already reads one sector.
- **Factory Sounds in LIBRARY, marked:** a stray save could overwrite them and the player's list would fill. Rejected.
- **Description in the Sound name:** names are 12 characters.

## Consequences
- Code under 0.5 KB of flash, about 1.9 KB of AXI for 40 B per listed entry (reported when the listing carries it). The text itself costs no flash.
- A factory Sound loaded and then saved is the player's own `UserId` Sound with no description.
- LIBRARY draws no description in alpha: the unit cannot write one.
- The id blocks and `P0000001`-`P0000009` are constraints on later work; the Source format and tool are decided in the next ADR.

## Sources
Spec `docs/superpowers/specs/2026-10-11-showcase-library-design.md` §§ 1, 2, 9; ADR 0119; plan `docs/superpowers/plans/2026-10-11-showcase-library.md` Task 2.
