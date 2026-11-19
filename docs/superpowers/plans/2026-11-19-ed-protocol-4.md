# Editor protocol 4: implementation plan

Spec: `docs/superpowers/specs/2026-11-19-ed-protocol-4-design.md`. ADR 0289 (Proposed, the owner's answers recorded 2026-11-19).

Branch `ed-proto4`, worktree (private notes), on `origin/0.4` 18d92288. A sibling branch, `editor-ui`, adds ADR 0290 (`refused` lines in `since`) and reworks the Elm screens; it merges later. `since`'s sections leave room for its `refused` lines (the PART section's tail), and 0290 keeps its number.

Every slice is test-first, firmware and the editor's `Protocol`/`Model` together (the editor speaks the slice's grammar exactly; its screens for the new pages are not in scope). Each slice ends with `just check`, `just editor-test` and `just firmware-orbit`, its bank 1, bank 2 and ITCM deltas measured and noted in ADR 0289, and one commit.

Builds: `CARGO_BUILD_JOBS=6 CARGO_INCREMENTAL=0 nice -n 15 <cmd>`.

## Rules carried by every task

- Bank 1 frozen: all `ed` code in `console::ed` (bank 2 S4), each fn `#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.<module>.<fn>"))]`. Bank 1 gains only `Fnv64` (beside `ByteCount`) and `#[inline(always)]` helpers bank 2 calls. Each raised budget line is the measured delta with an ADR 0289 / 0251 note; never the 64 B slack.
- Types: every new address, round, schema, section, command and refusal is an enum or newtype, built only at the parse boundary or by the walk. No catch-all `_` over an owned enum.
- No audio change; goldens bit-identical.
- Elm: elm-review's rules, every `let` annotated.

## Slice 1: MIXER and the frame

Firmware (`chimera-core/src/console/ed/`):

1. `schema.rs`: `Schema { Engine(EngineType), Mixer }` with its token (`ALGO`…, `MIXER`), `parse`, `ALL`; `RoundKind { None, Pads, Parts }`, `Round { Shared, Pad(PadId), Part(PartId) }`; `Schema::in_round(def)` an exhaustive match (a pad's page as `per_pad`; a mixer page binding `Part` or `Out`). Tests: tokens round-trip, page counts fit a `u8` (const assert over `Schema::ALL`).
2. `pages.rs`: `Pages` over a `Schema`: shared pages first, then each round's; `Pages::round(schema, Round)`; item `(def, Round)`. `page_at(schema, n)`. Existing engine walks unchanged in order (fixtures prove it).
3. `addr.rs`: `EdBlock::Part(PartId, PartBlock)` (`PART<n>`, `PART<n>_SND` holding SEND1–3), `EdBlock::Bus(BusBlock)` (`CHORUS DELAY REVERB TAPE COMP`). `EdAddr::param()` is the box's knob address (`Part`, `Out`, the bus); `EdAddr::shown(a, Round)`. `reachable(EdBlock)`. Tokens appended to `ed_block_tokens.txt`. Tests: every new address round-trips; `PART`, `PART0`, `PART7`, `PART3_SND.LEVEL` refused.
4. `describe.rs`: `describe(schema, page)`; `Axis::Reverb`, `Group::Reverb(ReverbType)` named `REVERB`; `param` gains `<flags>` (`m` from `Mappable::new`, `d` from `modulatable`, `-`). Engine fixtures regenerated (only the flags field changes). Test: every MIXER cell in every reverb TYPE described; no cell `axis_of` drops but selectors.
5. `value.rs`: reads of Part *n*'s mix and sends and the buses (`Project::read_part`); `find` walks the address's own schema and round; a Part round reads with Part *n* as the cell's Part.
6. `edits.rs`: `Taken { Sound { part, engine }, Part(PartId), Bus }`; a Part's set is `edit_to(n, …)` (MAX VOICES and RESERVE through `voices_act`); a bus's through the active Part's blocks, as the mixer's turn. Tests: `set`/`get` round-trip on a Part other than the followed one; MAX VOICES past the pool refused as the box does.
7. `since.rs` + `sections.rs`: `Section { Part, Mixer }`; each with its copy and whole rule; `mixer` header, `panel REVERB <TYPE>`; paging: each section measured, taken in order while it fits, the rest owed, `OK <m> more`; a section alone past the bound is `ERR ed since too large`. Tests: a whole MIXER answer fits; a messy DRUM Part with MIXER pages into two answers, each under the bound; a stale number mid-paging starts over; a lost page heals.
8. `ed schema <S>`: `Fnv64` in `chimera_engine::fmt_buf` (bank 1); the hash kept per schema in `EdState`. Tests: stable over two boots; moves with one byte of a page.
9. `PROTOCOL` 3 → 4. Help/usage text. `ed describe <SCHEMA> <page>`.

Editor (`editor/src/`):

10. `Protocol`: `Schema` type (`Engine e | Mixer`), `DescribeIn Schema Int`, `SchemaOf Schema`; `schema <S> <pages> <hash>`, `mixer` header, `OK <m> more`, the flags field (`Flags { mappable, modulation }`).
11. `Model`: per-schema `Schema` store; the connect walk asks `ed schema` for the followed engine and MIXER, uses a cached copy (port `cacheGet`/`cachePut` to IndexedDB in `ports.js`, keyed `(4, schema, hash)`) or describes every page and stores it; `since` follows `more`; a whole section's header drops that section's values. Tests in `ModelTest`/`ProtocolTest`.

## Slice 2: MTX

Firmware: `Schema::Matrix(EngineType)`; `ed/route.rs` with `RouteCell { Matrix { src: Jack, dest: EdAddr }, Jack(JackSlot) }`; `ed route <cell> <amount|off>` queued and applied through `MatrixState::set`/`clear` + `sync_into`, priming as MIX+PLUS, stepping at most the grid's fastest turn a pass; `route "<label>" <cell> <route|keybd>` from `RouteView`; ROUTES section (`routes <n>`, `route …`, `unroute …`, diffed by (source, destination)). Editor: the route lines, `Route` command, the ROUTES section. Details written at the slice's start from `mod_grid.rs`, `filter_panel.rs`, `modulation.rs`, `taps.rs`.

## Slice 3: SETTINGS

Firmware: `Schema::Settings` (tree leaves, `crumb` lines), `EdBlock::Global(GlobalBlock)` (`THEME MIDI RANDOM`), RANDOM's roll only on the active Part; SETTINGS section (`project`, `sound`, `card`, `toast`, `proj`); `ed projects`, `ed project save|saveas|load … [discard]` through `settings::job`. Editor: the lines and commands.

## Slice 4: MIDI MAP

Firmware: `Schema::Map`; MAP section (`map`, `row`, `learn`, `mapfile`); `ed map add|set|rm|pick|keep|save|rename`, `ed maps`, `ed learn <addr>|off` (takes the box to the cell). Editor: the lines and commands.

## Slice 5: ORBIT

Firmware: `Schema::Orbit`, `Round::Ring`, `EdBlock::Orbit(OrbitBlock)` (`ORBIT RING1–5 TEMPO`), sets through `OrbitSong::edit`; `act` lines; ORBIT section (`song`, `slot`, `chord`, `scale`, `step`, `pool`); `ed orbit play|stop`, `ed scene queue|save|clear|reload|move`, `ed chord|scale|step|pool`. Editor: the lines and commands.

Slices 2–5 are detailed at their start, after slice 1's frame is in, as the code they touch is read then.
