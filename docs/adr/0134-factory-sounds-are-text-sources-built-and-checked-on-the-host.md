# 0134. Build factory Sounds from text Sources, checked on the host

- **Status:** Accepted (2026-10-11)
- **Deciders:** owner, firmware lead
- **Relates to:** 0088 (the patch importer, Proposed on `importer-spec`), [0133](0133-factory-sounds-list-in-a-read-only-factory-tab-and-carry-a-one-line-description.md)

## Context
The showcase set (spec `2026-10-11-showcase-library-design.md`) is about 15 public Sounds now and 16 TX81Z ports later, all on the card at 0 B of flash. A Sound authored on the unit and copied off is a binary nobody can review or diff, and a description or id typed on the unit can't be checked against the FACTORY tab's width or the id blocks. The ports are derivatives of Yamaha voice data whose licence (#410) is unanswered, so they must never reach git or a public build.

## Decision
- **Sources.** Each factory Sound is a `[[sound]]` table of `key = "value"` lines in `tools/factory-sounds/` (`va.toml`, `fm.toml`, `modal.toml`; demo projects in `projects.toml`), read by `listen::lined_tables` (the `recipes.toml` parser, now taking dotted keys and trailing comments). Keys: `id`, `name`, `engine` (VA, ALGO or MODAL), `note`, `demo` (mono or poly), `plays` (render scripts: bass, stab, short, long, texture, glide), `home` (C4 is 60), `BLOCK.PARAM` by the frozen idents of `disk_codes_v1.txt`, `mod.<SOURCE>` and `tap.OP5`/`tap.OP6` as `DEST ±amount, …`, `trim` (dB on OUT LEVEL), `level = "by ear: …"`, `status = "placeholder"`, and, for ports only, `from` and `edits`. A param left out keeps `Sound::neutral`'s value.
- **Values are exact.** A number must be what the block stores (no clamp, no rounding); an enum is its ident, resolved in file order so MODE reads its KIND. A route's destination must be a fixed column or a modulatable param; a tap's must be a fixed column (FREQ, Q, VCA, FOLD) on an ALGO Sound. Each failure names `file:line`.
- **Checks.** Ids in `S9100001`-`S9199999` and unique; names unique (placeholders included); a description of `Note`'s characters, 1-40 B, and no wider than `CONTENT_W` in `FONT_LABEL` at `LABEL_TRACKING` by the firmware's own `draw::text_width`.
- **The public build** (`just factory-sounds`) refuses any Source with a `from` line and any id in the port block `S9100010`-`S9100025`, skips placeholders, and writes `target/factory-sounds/public/CHIMERA/SOUNDS/S91xxxxx.A` through `encode_library_sound` (`Engine`, `Build`, `Note`, then the Sound) plus `manifest.json` (id, name, CRC, size). `Build` carries the version and the commit the Sources were built from, dirty when they differ from it.
- **Ports are private.** Port Sources live in `$CHIMERA_TX_PORTS`, outside the repo; they need `from` and `edits` and build only into `target/factory-sounds/private/`. TX81Z factory banks only. Out of git and out of the public set until #410 is answered.
- **Canonical text.** `factory-sounds fmt` appends each numeric param's display value as a comment (`# 2.40 kHz`); comments are ignored on read, so `fmt` changes no Sound and is idempotent. A golden Source builds to committed bytes under a fixed stamp, and its param, route and tap lines round-trip through `Sound`.
- **QA gates** (loudness, limiter, clicks, aliasing, tails, taps, sameness) render each Source through the real `Instrument` before the owner listens; they are the QA task's, under this ADR.

## Alternatives considered
- **Author on the unit and copy the files off:** binary, unreviewable, and no check of width or id. Rejected.
- **A real TOML crate:** a dependency for a subset `listen::tables` already reads, and it would accept nested tables and arrays that make a Source more than one line a setting. Rejected.
- **Params by `ParamId` number or by label:** numbers are unreadable, labels change. The frozen idents are permanent (ADR 0045).
- **Clamping out-of-range values:** a Source would say one thing and the card hold another. Rejected.

## Consequences
- Every factory Sound is diffable text; review sees each change as a line.
- A firmware ident rename can't happen (they are frozen), so Sources never rot; a new param is a new key.
- The public build can't carry a port, by `from` and by id; with the git index scan (`no_private_ports_in_index`) and the release's id check that is three walls.
- Sends live in the Part today; Sources set them once `sound-sends` merges.

## Sources
Spec `docs/superpowers/specs/2026-10-11-showcase-library-design.md` §§ 3, 6, 8, 9; plan `docs/superpowers/plans/2026-10-11-showcase-library.md` Task 5; ADR 0045 (frozen codes), 0088 (importer), 0133; `tools/factory-tapes/recipes.toml` (the text recipe model).
