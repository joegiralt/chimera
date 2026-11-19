# Editor protocol 4: every page the unit has

**Status:** proposed 2026-11-19. Design only; the owner's questions are at the end. ADR 0289 records the decisions.

## Intent

Protocol 3 (ADR 0285) reaches every engine's pages of the followed Part. The owner wants the editor to control "literally everything the PreenFM3 can" in 0.4 (2026-11-18): the MIXER, the mod matrix, SETTINGS, MIDI MAP and ORBIT. Protocol 4 is the protocol for that. It is a design for the unit's side and the wire. The editor's screens are not in it.

It keeps every rule of slices 1 and 1b (`2026-11-14-editor-algo-slice-design.md`, `2026-11-18-editor-every-engine-design.md`):

- The unit describes itself from its own tables. The editor holds no parameter list.
- Values read as the box writes them.
- The editor pulls with `ed since`. The unit never pushes. No optimistic edits.
- `set` takes the knob path. Every new command takes the box's own path for the same gesture.
- Unknown or garbled lines are refused, never guessed at.

## What protocol 3 can't reach today

- `in_schema` (`since.rs`) admits only Sound blocks: CHORUS, DELAY, REVERB, TAPE, COMP, PART, PARTMIX, THEME, ORBIT, RINGn, TEMPO, MIDI and RANDOM are `false`. `ed get`, `ed set` and `since` refuse them all.
- `addr::token` gives `PartMix(_)`, `Tempo` and `Random` no token (their `disk_ident` is `None`).
- `describe::axis_of` returns `None` for `ReverbPanel`, `SelectedRing` and `Orbit(_)`. The mixer's and ORBIT's chains are never walked.
- FLT › MOD's ENV AMT, KEYBOARD, FREQ MOD and RES MOD (`View::Route`, `filter_panel::RouteView`) describe as `route "<label>" next-slice`. The MTX page (`MOD_MATRIX`, `PageLayout::Matrix`, six `EMPTY` cells) describes as an empty page. A route is no `ParamSpec`: it lives in `Sound::mod_state` (`ModState`: 16 registry columns and 4 fixed columns, 11 sources, `i8` amounts and presence bits), `Sound::dest_registry` and, on ALGO, `params.taps` (OP5 and OP6 into the fixed columns).
- Nothing reaches the MIDI map (`Sound::map`, 16 `Binding`s; the Part's flat controller map, up to 90 rows in ITCM, ADR 0266), learn, the scene slots, the transport, or the projects.
- The editor describes an engine on every connect and engine change. DRUM is 165 pages, about 210 KB, 5 to 6 s.

## Principles this design adds

1. **Everything addressed, nothing selected.** ADR 0285 chose every pad addressed over "follow the box's pad", because an edit could land on whatever the box selects first. The same holds for the active Part in the mixer and the selected ring in ORBIT. Protocol 4 addresses each Part (`PART3.LEVEL`) and each ring (`RING2.HITS`), never "the active one". `BlockRef::Part` and `SelectedRing` stay unreachable on the wire.
2. **Witnessed commands.** A command that overwrites or removes what the box keeps (a scene slot, a saved map, a project) carries the witness the editor saw: a slot's rev, a map's name, a project's generation. The box's own confirmations already work this way (`Confirmed<SceneSlot>`, ADR 0077 "saves are witnessed"). A stale witness refuses in the UI pass and writes nothing. The editor shows its own confirm dialog; the witness makes it safe.
3. **Idempotent, not toggles.** `ed orbit play` and `ed orbit stop`, never "toggle". Two sends of a toggle can cancel out; two sends of `play` play.
4. **Commands queue like `set`.** Every command that changes state is parsed and checked at the boundary, queued, and applied in the UI pass after the frame's hand input, through the box's own function for that gesture (as `ed set` and `ed part` are, ADR 0276). It answers `OK queued` or an `ERR`. What happened comes back through `since`, with the box's toast.

## The frame (lands with slice 1)

### Schemas

`ed describe <SCHEMA> <page>`. A schema is a typed `Schema`, parsed once at the boundary:

```rust
enum Schema { Engine(EngineType), Mixer, Matrix(EngineType), Settings, Map, Orbit }
```

Tokens: `ALGO MODAL VA TAPE DRUM` (as today), `MIXER`, `MTX-<ENGINE>` (`MTX-ALGO`), `SETTINGS`, `MAP`, `ORBIT`. Each has its walk from an existing table:

| Schema | Walks | Rounds |
|---|---|---|
| ALGO … DRUM | `pages::chain(engine)` (as protocol 3) | a pad's pages on DRUM |
| MIXER | `MIXER_CHANNEL_CHAIN` | PRT, VOI and SND once a Part |
| MTX-*e* | the engine's MTX page alone | none |
| SETTINGS | `settings::tree::ROOT`'s `Leaf(&OnePage)` rows in tree order, but `read_only` leaves and DEMO | TUNING and LIVE SNAP once a Part |
| MAP | one page: the binding grammar | none |
| ORBIT | `ORBIT_CHAIN` | RHYTHM, VOICE and NOTES once a ring |

A build without `orbit` has an ORBIT schema of 0 pages, as without `master-tape` MIXER has no REEL page. The schema says so; the editor hides what has no pages.

### One walk with typed rounds

`Pages` generalises from "the Part's, then each pad's" to rounds:

```rust
enum Round { Shared, Pad(PadId), Part(PartId), Ring(RingIdx) }
```

`Schema::round_of(&BlockDef) -> RoundKind` is an exhaustive match, as `per_pad` is now: a page is a Part's when a cell binds `BlockRef::Part` or `BlockRef::Out` (MIXER, SETTINGS), a ring's when a cell binds `SelectedRing` (ORBIT), a pad's as today (DRUM). Shared pages come first, then each round's, labelled as pads are: `PART3_PRT`, `RING2_RHYTHM`. A shared cell on a round's page (TUNING's global TUNE on `PART3_TUN`) reads the same in every round, as a Sound cell on a pad page does today.

`describe`, `mark_described`, `value::find`, `edits::fmt_of` and `since` all take `Pages`. Nothing else walks a chain. The page count stays a `const` checked to fit a `u8` (MIXER 28, SETTINGS about 26, ORBIT 19, DRUM 165).

### Addresses

`EdBlock` gains four arms. Every one is a typed value built only by `parse` or the walk:

```rust
enum EdBlock {
    Sound(SoundBlock),           // the followed Part's Sound or kit (as today)
    Pad(PadId, PadBlock),        // as today
    Part(PartId, PartBlock),     // PART<n> and PART<n>_SND
    Bus(BusBlock),               // CHORUS DELAY REVERB TAPE COMP
    Global(GlobalBlock),         // THEME MIDI RANDOM
    Orbit(OrbitBlock),           // ORBIT, RING1–RING5, TEMPO
}
enum PartBlock { Mix, Sends }
enum OrbitBlock { Chords, Ring(RingIdx), Tempo }
```

- `PART<n>` (1–6) is Part *n*'s `PartParams` (`BlockRef::PartMix(n)`'s table, `PART_SPECS`): `PART3.LEVEL`, `PART3.MAX_VOICES`.
- `PART<n>_SND` is Part *n*'s Sound's OUT, holding only `SEND1`–`SEND3`, as a pad's filter holds only `PadFilter::IDS`. `PART3_SND.SEND2`. The followed Part's sends are also `OUT.SEND2` on its AMP › SENDS page: one value, two addresses, both reported. That is accepted: the mixer shows every Part's sends whatever Part is followed.
- `CHORUS`, `DELAY`, `REVERB`, `TAPE` (the master reel), `COMP`, `THEME`, `MIDI`, `ORBIT`, `RING1`–`RING5`: their `disk_ident`s, the patch file's tokens, as ADR 0276 asks.
- `TEMPO` and `RANDOM` are new tokens (no `disk_ident`).
- All are appended to `chimera-core/tests/fixtures/ed_block_tokens.txt`. Nothing in it changes.
- `PART` (the active Part), the bare `SoundBlock(Part)` and `SelectedRing` stay unparseable.

`in_schema(engine, block)` becomes `reachable(EdBlock)`: Sound and Pad blocks by the followed engine (as now); Part, Bus, Global and Orbit blocks always, but for the features a build lacks.

### Where a set lands

`Pending` keeps what it was taken for, typed:

```rust
enum Taken { Sound { part: PartId, engine: EngineType }, Part(PartId), Global, Orbit }
```

- A Sound set is dropped if the Part changed engine (as now).
- A Part set lands on its Part: `edit_to(n, Part.<id>)`, so MODE, MAX VOICES and RESERVE take `voices_act` (ADR 0281) for Part *n* whatever Part the box shows. Sends: `edit_to(n, OUT.SEND<k>)`.
- A Bus or Global set lands through `UiBlocks` as the SETTINGS and mixer pages' turns do.
- `RANDOM.*` rolls the active Part (ADR 0232). It is taken only when the followed Part is the active one, and dropped if the box changed Part before the UI pass, so a roll never lands on a Part the editor wasn't showing.
- ORBIT never goes through `part_page::edit`: its `block_mut` refuses ORBIT blocks (`debug_assert`, ADR 0083). An ORBIT set is `OrbitSong::edit` with a set-to closure, marking the ring `touched` as `turn` does (ADR 0082), then `cc_hand`. A ring's set lands on its ring, never the selected one. `TEMPO.BPM` under a MIDI clock is dimmed on the box and refused here the same way (`view::is_dimmed`).

`ed part` is unchanged. Setting `PART4.LEVEL` doesn't move the box.

### Commands

`EdArg` grows. Each argument is parsed into a typed value at the boundary, and every refusal is its own `ERR` (`EdRefusal`, `SetRefusal`, and one refusal enum per new command family). A line is still at most `MAX_LINE` (64 B); every command below fits.

| Command | Slice | Answer |
|---|---|---|
| `ed schema <SCHEMA>` | 1 | `schema <SCHEMA> <pages> <hash>` |
| `ed route <cell> <amount\|off>` | 2 | `OK queued` |
| `ed projects` | 3 | `OK queued` (a re-list) |
| `ed project save` / `saveas <name>` / `load <gen> <slot> [discard]` | 3 | owner question 2 |
| `ed map add\|set\|rm\|pick\|keep\|save\|rename …` | 4 | `OK queued` |
| `ed learn <addr>` / `ed learn off` | 4 | `OK queued` |
| `ed orbit play\|stop` | 5 | `OK queued` |
| `ed scene queue\|save\|clear\|reload\|move …` | 5 | `OK queued` |
| `ed chord <seg> <degree>`, `ed pool <ring> …`, `ed step <ring> …` | 5 | owner question 4 |

Commands queue in a small fixed array beside the sets (`EdState::acts`, 8 entries, `Pushed::Full` past it), applied in order after the sets in the UI pass.

### `ed since`: sections, and paging when whole

`since` keeps its one number (`Gen`) and its pull model. Its answer is made of sections, each with its own kept copy and its own rule for answering whole:

| Section | Holds | Whole when |
|---|---|---|
| PART | the followed Part's values, panels, `source` (as protocol 3) | another Part or engine, a text-context or panel change |
| ROUTES | the followed Part's matrix | another Part or engine |
| MAP | the followed Part's map rows, its controller map, learn | any change (map edits are rare) |
| MIXER | `PART<n>`, `PART<n>_SND`, the buses, `panel REVERB` | a change in the mixer's text context (reverb TYPE, chorus MODE, delay TYPE, PLATE) |
| ORBIT | the ORBIT blocks, the song line, the slots | a text-context change (the clock source) |
| SETTINGS | THEME, MIDI, RANDOM, the project's and Parts' state, the listing | a text-context change (`clash`, `pages`) |

- A diff answer is each section's diff, in that order. Lines name what they report (`val <addr>`, `route`, `row`), so a diff needs no headers.
- A whole section opens with its header line (`part <n> <ENGINE>` as today, `routes <n>`, `map <n>`, `mixer`, `orbit`, `settings`). The editor drops what it held for that section, so anything not repeated is gone.
- Every answer holding a whole section opens with the empty line (ADR 0276), so a cut answer's fragment never swallows a header.

**Bound and paging.** ADR 0285 refuses an answer past `ANSWER_BYTES_MAX` (32,000 B) and measures first (`Pass::Measure`, then `Pass::Commit`). Protocol 4 keeps both passes but measures each section. A full answer for a messy DRUM Part with every section filled is about 53 KB (sizes below), so it can't be one answer. `since` then answers whole in pages:

1. Sections are taken in their fixed order while the running total fits the bound. The first section owed is always taken: a test holds every section's worst case under the bound by itself.
2. Sections taken are committed (their copies kept). The rest are marked owed in `EdState`, and the answer ends `OK <m> more`.
3. The editor asks again at once with `m`. Sections already sent answer their diffs; owed ones answer whole. This repeats until an answer ends `OK <m>`.
4. A stale or zero number starts over: everything owed.

Nothing is cut and nothing is lost. An answer that never arrived leaves the editor with a stale number, which heals as today. `ERR ed since too large` stays only for a single section past the bound, which the test forbids.

Worst whole sections (host estimates, to be measured as ADR 0285's were): PART 24.2 KB (the messy kit, measured), ROUTES 10 KB (all 11 sources × 20 columns present), MIXER 7.5 KB (131 values), MAP 6.5 KB (16 + 90 rows), ORBIT 4.5 KB, SETTINGS 3 KB. A messy DRUM Part takes two answers; any other engine fits one. A typical diff while dragging is under 200 B.

**What each section keeps, and where.** `EdState` is in ITCM (ADR 0276, 0286).

| Kept | Size |
|---|---|
| value slots: 711 → 952 (MIXER 131, ORBIT 80, SETTINGS 30 added to every engine's layout) | +964 B |
| ROUTES: a copy of `ModState`'s amounts, presence and columns, and the taps | about 340 B |
| MAP: a 32-bit FNV of the Sound's rows, of the controller map's rows and name, and learn's state | 16 B |
| ORBIT: the song line and the 8 slots' revs | 24 B |
| SETTINGS: project and Part status, listing generation, last toast | 48 B |
| owed sections, schema hashes (below), the act queue | about 220 B |

About +1.6 KB of ITCM in all, which has 8 KiB free (ADR 0286). If a later ITCM round needs it back, the non-PART copies can move to AXI: nothing but the console reads them. The `Described` mask grows with the slots (30 words).

### The schema cache

The schema is static per firmware build, but a firmware update can change it, and dev builds differ while their build string doesn't. The editor needs to know a schema it cached is still the unit's, without reading it again.

- **`ed schema <SCHEMA>`** answers `schema <SCHEMA> <pages> <hash>`, `hash` sixteen hex digits: a 64-bit FNV-1a of every byte `ed describe <SCHEMA> 0..pages` would write, in page order.
- The unit computes it with describe's own walk into a hashing writer (`Fnv64`, a `core::fmt::Write`) and keeps it in `EdState` for the rest of the boot (one `Option<u64>` a schema, 13 schemas). Equal hash means the same text, so the editor's cache can't disagree with `describe`: the hash *is* describe.
- `Fnv64` sits in bank 1's `fmt_buf` beside `ByteCount`. The reason is the one ADR 0285 gives: a `Write` impl's vtable is bank 1's and may not point into bank 2. About 80 B of bank 1, under ADR 0251, measured.
- **The editor** keeps each schema's pages in IndexedDB, keyed by `(protocol, schema, hash)`. On connect and on each engine change it asks `ed schema <S>`. If it holds that key, it uses the cache. Otherwise it describes every page and stores them. A DRUM change is one short round trip instead of 5 to 6 s.
- **Cost on the unit.** The first `ed schema DRUM` of a boot walks 165 pages to a hasher: about 210 KB of formatting in the console's answer path. On x86 release, protocol 3's two `since` passes over 24 KB took 2.8 ms. The unit gate times `ed schema DRUM`. The budget is 20 ms of UI loop. If it is over, the hash is filled one page a UI pass in the background after `ed hello`, and `ed schema` answers `schema DRUM pending` until it is done (a cursor in `EdState`). Host tests pin that the hash changes when any page's text does, and that it is the same over two boots.

Rejected: the build string as the key (every rebuild drops the cache, and dirty dev builds share one string); a hash pinned in source by a test (describe's text depends on cargo features: `orbit`, `master-tape`, `usb-audio`, `push-encoders`, `midi-map`, so one constant can't hold every build); per-page hashes (165 lines to compare, for nothing the single hash doesn't give).

### Versioning

`PROTOCOL` goes 3 → 4 in slice 1. The editor requires exactly 4: a unit on 3 says UPDATE THE UNIT, a higher one UPDATE THE EDITOR, as protocol 3 does. The editor isn't on the site until 0.4, so no 3-speaking editor is in the field. Slices 2–5 land on 0.4 inside protocol 4. The grammar freezes at the 0.4 release, not at slice 1: before then a slice may add lines and commands, and each records them in ADR 0289's consequences. After 0.4, any change to a line's grammar is protocol 5.

`param` lines gain a flags field (below), so protocol 3's grammar changes either way.

## Slice 1: MIXER

**Pages.** `MIXER_CHANNEL_CHAIN`: PRT and VOI and SND per Part, then CHR, DLY with CHAR and GRAIN, REV with MORE and MOR2, REEL (with `master-tape`), MST with LVL.

**Reverb's panels.** REV, MORE and MOR2's cells bind `ReverbPanel(page, k)`, resolved by `reverb_panel::knob(ctx.rev, page, k)`. `axis_of` gains `Axis::Reverb` and `Group::Reverb(ReverbType)`, one panel a TYPE, named `REVERB`, variant the TYPE's option name. `since` reports `panel REVERB <TYPE>` in the MIXER section (`Shown::Reverb`).

**Delay and chorus.** DLY's TYPE dims the other TYPE's cells (`delay_note`), but the cells stay the same params: they describe once, fixed, and the box's text says DIM where it does. Chorus MODE (CHORUS, FLANGER, PHASER, ADR 0235) changes no cell.

**Flags on `param`.** The line becomes `param <addr> "<label>" <kind> <lin|oct> <min> <max> <step> <default> <flags>`, flags a word of `m` (MIDI-mappable: `Mappable::new` admits it), `d` (a mod destination: `ParamAddr::modulatable`), or `-`. The editor's MIDI MAP and MTX pickers read it. No separate target list exists, so none can drift.

**Text context.** A Part round's cells read under Part *n*'s context: its Sound's `SlotCtx` for SND, its mix for PRT and VOI. The buses read the live context.

**Tests.** Every MIXER cell of every page in every reverb TYPE is described. `set` then `get` round-trips for `PART<n>.*`, `PART<n>_SND.*` and each bus, on a Part other than the followed one. MAX VOICES past the pool is refused as the box refuses it (ADR 0281). A full MIXER answer fits the bound. The schema hash is stable over two boots and moves with a label.

**Cost.** Bank 2 S4 about +3.5 KB (the address arms, the rounds, the reverb group, `ed schema`, sections and paging). Bank 1 about +80 B (`Fnv64`).

## Slice 2: MTX, the Part's mod matrix

**A route's address** is a cell: `<SRC>:<DEST>`.

- `SRC` is a `Jack`'s `disk_ident` (`taps.rs`, frozen): `ENV1 ENV2 ENV3 LFO1 LFO2 LFO3 VEL NOTE WHEEL PRESS TIMBRE`, and `OP5 OP6` on ALGO.
- `DEST` is an `EdAddr` of the followed Part's Sound that `ParamAddr::modulatable` admits.
- `LFO1:OP_C.CRSE`, `ENV1:FILTER.CUTOFF`, `OP5:FILTER.RESO`.
- Typed: `RouteCell::Matrix { src: Jack, dest: EdAddr }`. A tap is only valid into a fixed column, and only on ALGO (`MatrixState::is_x`, `filter_panel::repatch`'s rule).

FLT › MOD's jack knobs (FREQ MOD, RES MOD) edit "whatever FM3 IN or QM IN names". Resolving that in the editor would race a re-patch on the box. They are their own cells: `RouteCell::Jack(JackSlot)`, written `FM3IN` and `QMIN` (the jack params' idents), resolved in the UI pass by `RouteView::Jack(j).cell(sound)`, as the knob does.

**Commands.**

- `ed route <cell> <amount>`: the cell's amount, an integer −127..127, creating the route if absent. On a registry destination not yet primed, it primes it (`ModDestRegistry::add` with `mod_grid::dest_label`), as MIX+PLUS then a grid turn would. A full registry (16) refuses with the box's toast.
- `ed route <cell> off`: the route's presence cleared (`MatrixState::clear`), the grid's DELETE. A primed column left empty stays primed, as on the box.
- Applied through the matrix's own path: `MatrixState::set` or `clear`, then `sync_into` the Sound's `ModState` (taps into `params.taps`), never `ModState` directly. KEYBOARD's cell takes `keybd_step` (OFF, HALF, FULL).
- **No snap.** A route amount isn't a glided param. Instead of a jump, the amount moves toward the target by at most the grid's largest accelerated step a UI pass, the same distance a hand's fastest turn covers. The step comes from the grid's jog table, never a new constant.

**Amount range.** The box stores `i8` −127..127 and writes it as is (`fmt_amount`: `+42`). The wire carries that number and the box's text. The mockups' "−100..+100" is owner question 5.

**Schema: `MTX-<ENGINE>`.** One page:

```text
page 0 MTX of 1
src <JACK> "<label>"            (ROW_ORDER's order, PART_MOD_SOURCES' names; OP5, OP6 on ALGO)
col <FREQ|Q|VCA|FX> <addr>      (FixedCol::ALL: FILTER.CUTOFF, FILTER.RESO, OUT.VCA, VOICEFX.P1)
amount -127 127
dests 16
```

Destinations are every param with the `d` flag in the engine's schema, plus the four fixed columns (OUT.VCA is on no page). So MTX describes no list of its own.

**FLT › MOD's cells.** `route "<label>" next-slice` becomes `route "<label>" <cell> <route|keybd>`: `route "ENV AMT" ENV1:FILTER.CUTOFF route`, `route "KEYBOARD" NOTE:FILTER.CUTOFF keybd`, `route "FREQ MOD" FM3IN route`, `route "RES MOD" QMIN route`. From `RouteView`, an exhaustive match.

**`since` (ROUTES).** Whole: `routes <n>` then a `route <SRC>:<DEST> <amount> "<text>"` line for every present cell, registry and fixed, and the taps on ALGO. Diff: `route …` for a cell whose amount or presence changed, `unroute <SRC>:<DEST>` for one gone. Compared by (source, destination address), never by column index, since the registry's columns shift when one is removed.

**Tests.** Every `RouteView` and every source × fixed column round-trips. A tap off ALGO, or into a registry column, is refused at parse. Priming past 16 writes nothing. A re-patched FM3 IN between the set and the pass lands on the new source's cell. A whole ROUTES answer with every cell present fits the bound.

**Cost.** Bank 2 about +3 KB. ITCM about 340 B.

## Slice 3: SETTINGS

**Pages.** The tree's leaves with params, in tree order. `page` lines are followed by `crumb "<LIST>" "<ROW>"` (the tree's own crumbs), so the editor can lay SETTINGS out as the box's lists without a list of its own. CHANNELS and OUTPUTS bind `PartMix(n)` and read as `PART<n>.CH` and `PART<n>.OUT`, the same addresses as the MIXER's: one value, one address. TUNING and LIVE SNAP bind the active Part and are described once a Part.

**RANDOM.** The five cells are params (`RANDOM.OSC` … `RANDOM.SEED`). A set rolls the followed Part's Sound, as a turn does (ADR 0232), only when it is the active Part (see "Where a set lands"). Whether the editor may roll at all is owner question 3.

**Project and Part state (read-only)** in the SETTINGS section:

```text
project "<name>" <slot|new> <clean|edited> gen <g>
sound <n> "<name>" <clean|edited|stale>          (each Part, as `part_status` reads it)
card <free KB> <total KB>
toast "<text>"                                    (the box's last toast, when it changes)
```

**Listing.** `ed projects` queues the box's own re-list (`card_work` reads it, as LOAD PROJECT does, at most `MAX_LISTED`). The SETTINGS section then carries the listing whole: `proj <id> "<name>" <ok|bad|newer> gen <g>`. No card read happens in the answer path.

**Project commands** depend on owner question 2. Whatever is allowed goes through `settings::job` (`card_work`, under BUSY, the box's prompts' witnesses: `Confirmed<OverwriteTarget>`, `Pending<ProjectSource>`). A name is one word with `_` for a space, accepted only if NAMING's character set has no `_` (a test; else another byte outside the set). Never through `ed`: OS UPGRADE (`EnterDfu`, the updater's, #1016), FORMAT CARD, the card's files (#1015 owns `ls`, `get`, `put` and `rm`).

**Tests.** Every leaf cell described. A THEME set reaches the panel's colours on the next frame. A RANDOM set on a Part the box left writes nothing. `ed projects` while BUSY waits for the job, never stalls the answer.

**Cost.** Bank 2 about +2 KB; ITCM about 120 B (slots and state).

## Slice 4: MIDI MAP

**The rows.** The followed Part has two lists: the Sound's own rows (`Sound::map`, at most 16) and its controller map's (flat, at most 90, ADR 0266). A row is ADR 0217's `Binding { input, target, span, feedback }`.

```text
map <n> <"<map name>"|none> keep <on|off>
row S<i> <input> <kind> <addr> <min> <max> <echo> <live|dead|shadowed>
row M<i> <input> <kind> <addr> <min> <max> <echo> <live|dead|shadowed>
learn <off|armed <addr>|sensing S<i>> "<note>"
mapfile "<name>"                                     (after `ed maps`, at most 16)
```

- `input` is `CC<n>`, `CC14:<msb>`, `NRPN:<n>`, `PRESS` or `SYSEX`. `kind` is the box's word (`midi::page`'s `WORDS`) with spaces as `-`: `JUMP`, `PICKUP`, `SCALE`, `TWOS-COMP`, `OFFSET`, `SIGNED`, `SIGNED-2`, `INC-DEC`, `MOMENTARY`, `TOGGLE`.
- `addr` is the target as protocol 4 addresses it: the Sound's params, `PART<n>.LEVEL` and `.PAN` for the Part, the buses' knobs, `RING<k>.*` amounts (`Mappable`). Every target `Mappable` admits has a token now.
- `min` and `max` are the span's places 0..255. `min > max` inverts. The model has no curve: a binding's shape is its span and its input's takeover, and protocol 4 adds none.
- `shadowed` is a map row a Sound row overrides on its control (ADR 0226's dimming).

**Commands.**

- `ed map add <input> <kind> <addr> [<min> <max>]`: a Sound row, through `MidiMap::learn` (a target appears once; `MAP FULL` past 16). The input and kind are parsed as one typed `Input`, so an illegal pair (INC-DEC on a CC) is refused at parse, as `Input` can't hold it.
- `ed map set <S<i>|M<i>> <control|min|max|input|echo> <value>`: the box's B–F on that row. A map row's edit is a card job, one waiting at a time (ADR 0255: `CARD BUSY`).
- `ed map rm <S<i>|M<i>> <witness>`: the witness is the row as `since` reported it (its input and target), so a row that moved or changed between refuses.
- `ed map pick <"name"|none>`, `ed map keep <on|off>` (ADR 0253), `ed map save`, `ed map save <name>`, `ed map rename <name>`, `ed maps` (the box's listing, as a job): row 0's gestures. Saving over or deleting a map is owner question 3.
- `ed learn <addr>` arms learn on that cell the box's way: it takes the box to the cell's page (`value::find` gives the page and slot; `Location::step` and `go`, as `ed part` does), focuses it, and arms (`Learn::Armed`, MIX+SEQ's path). The box shows the learn in its focus band, and learn ends as it does on the box (a nav key, a page change, a turn, or `ed learn off`). Learn's state stays bank 1's `Learn` and needs no target field. ORBIT and SETTINGS cells can't be learned, as on the box (ADR 0139); the editor offers `ed map add` with a typed control there. Whether learn may move the box is owner question 6.

**Schema: `MAP`.** One page, from the tables: the input forms, each `Input::kinds()` with its word and its line (`ABSOLUTE · JUMPS TO THE KNOB`), the learnable CCs (`LearnCc::new` over 0..127), `span 0 255`, `rows 16`, `maprows 90`.

**`since` (MAP).** Whole on any change: the rows are hashed (FNV-1a 32 of the Sound's bindings, and of the controller map's rows and name), and a moved hash answers the section whole. Learn's state is compared as a value.

**Tests.** add, set, rm and the witness. A map-row edit while a job waits says `CARD BUSY`. `learn` takes the box to a FILTER cell and a CC learns it. An ORBIT address is refused for learn. A whole MAP answer with 106 rows fits the bound.

**Cost.** Bank 2 about +4 KB. ITCM 16 B.

## Slice 5: ORBIT

**Pages.** `ORBIT_CHAIN`: ORB and CUSTOM, CHRD, SCN shared; RHYTHM, VOICE and NOTES once a ring. `RING<k>.*` comes from `SelectedRing(id)` read in round *k*. `ORBIT.*` is the chord ring (`CHORD_SPECS`: KEY, SCALE, BARS, SEGS) and `TEMPO.BPM` the tempo.

**ORBIT's own cells** (`OrbitCell`: RING, SCENE, SAVE, SWITCH, DEGREE, PLAY, CURSOR, SET, NOTE, LEARN, CLEAR) are no params. They describe as `act "<label>" <TOKEN>`, an exhaustive match over `OrbitCell`, so the editor can place the command that does the cell's job. Capture (LEARN, PLAY) stays on the box: it records what the keys play.

**`since` (ORBIT).**

```text
song <playing|stopped> clock <INT|EXT> scene <n> queued <n|-> edited <yes|no>
slot <n> <saved|neutral> rev <r>        (each of 8; on change)
val ORBIT.KEY … / val RING2.HITS … / val TEMPO.BPM …
```

Values are `live`'s, the playing scene with its drift (ADR 0083), as the pages show. MUTATE's drift moves hits and notes, not the params, so a running ORBIT doesn't flood `since`.

**Commands**, each through the box's own path:

- `ed orbit play` / `ed orbit stop`: `Cmd::TogglePlay` only when the state differs. Under SYNC EXT the DAW drives the transport (ADR 0080), and both are refused as B6 is.
- `ed scene queue <n>`: SCENE turned to *n* (`OrbitSong::queue`). To the playing one cancels a pending switch.
- `ed scene save <n>`: SCN's SAVE (`save_to`). Over a saved slot other than the playing one, the box asks. Here the editor sends `ed scene save <n> over <rev>`, the slot's rev from `since`, made into `Confirmed<SceneSlot>`. A slot written since refuses (`Refused::SlotChanged`). Saving while a switch waits refuses (`SwitchPending`), as on the box.
- `ed scene clear <n> <rev>` and `ed scene reload <rev>`: SETTINGS › ORBIT's CLEAR and RELOAD FROM PROJ, witnessed the same way.
- `ed scene move <a> <b> <rev a> <rev b>`: swap two slots. The box has no move. The owner named "move scenes" in the editor's scope (2026-11-14), and it is part of owner question 1.

**Not in this slice unless owner question 4 says so:** chord degrees per segment, the custom scale, STEP notes and pool notes. They are scene data but no `ParamSpec`s.

**Tests.** Every cell in every ring round. A set on RING3 lands on ring 3 while the box shows ring 1. A set marks the ring touched, and the runner hears it on the next loop. Save-over with a stale rev writes nothing. play while playing does nothing. Transport under EXT refuses.

**Cost.** Bank 2 about +3 KB. ITCM about 344 B (slots and song state).

## Firmware placement and budgets

- **Bank 1 is frozen** (ADR 0198). New: `Fnv64` (about 80 B, its vtable), and any helper bank 2 calls that the compiler won't inline, made `#[inline(always)]` as protocol 3's were. Target: under 200 B for all five slices, each raised line naming ADR 0289 and 0251, never riding the slack.
- **Bank 2.** All of `console::ed`, in S4, every function in its own `.bank2_ed.<module>.<fn>` section (ADR 0285). New modules: `ed::schema` (the `Schema` type, rounds, hash), `ed::route`, `ed::map`, `ed::orbit`, `ed::project`, `ed::sections`. About 15.5 KB in all (slice 1 3.5, 2 3, 3 2, 4 4, 5 3). ORBIT's S0–S3 keeps ≥ 8 KB free, untouched. `check-door-placed` and `nm` on the ORBIT and default builds after each slice, as protocol 3 did.
- **ITCM.** About +1.6 KB, `just check-itcm`'s list updated.
- **No audio change.** Every command applies in the UI pass. Goldens stay bit-identical.

## Testing, across slices

- **Coverage.** For each schema, every cell of every page, in every variant and round, is described. A test fails if a page has a cell `axis_of` drops that isn't a selector (the protocol 3 test, over `Schema::ALL`).
- **One walk.** `ed get`'s text equals the cell's text on the box (`view::view` and `cell_text`) for every address of every schema, under the box's own context: the protocol 3 test, over the new schemas.
- **Sections and paging.** A messy DRUM Part with every section full answers in two pages, each under the bound. A stale number mid-paging starts over. A lost page heals.
- **Fixtures.** `ed_block_tokens.txt` appended. Schema fixtures for the editor regenerated per schema (`editor/tests/fixtures/*-schema.txt`). Protocol fixtures re-recorded at 4.
- **e2e** (`editor/e2e/sim.mjs`): a pass per schema on the simulator, setting every described address and checking `ed get`; each command family once.
- **Unit gate**, after merge and flash: ADR 0285's steps, plus `ed schema DRUM`'s time, a two-page `since`, and AUDIO LOAD while the editor drags a route.

## Questions for the owner

### 1. What do ORBIT's "print" and "song" mean, and is "advance to the next scene" in this protocol?

Your words (2026-11-14): the project is the song; ORBIT's song holds 8 scenes with queued switching; the only new box feature is an option to advance to the next scene. You also named "print a scene", "move scenes" and "create songs" in the editor's scope. Nothing on the box is called print.

- **Song.**
  - **a.** The song is the project's 8 scene slots with queued switching, as now. "Create a song" is a new project. The editor shows the 8 slots, which is playing, which is queued, and saved or neutral.
  - **b.** A song is a new object: an ordered list of scenes with repeats, played in order. That's a box feature first, and a later protocol.
- **Print.**
  - **a.** Print is SCN's SAVE: the playing scene with its drift (MUTATE's changes) written into a slot, so what you hear is kept. It exists; `ed scene save` does it.
  - **b.** Print freezes the drift into STEP notes and hit masks and sets MUTATE to 0, so the scene stops evolving.
  - **c.** Print exports the song as a MIDI file to the computer.
- **Move scenes.** `ed scene move` swaps two slots, witnessed by both revs. It's editor-only: the box has no move.
- **Advance to the next scene.** A box option (for example, after N loops queue the next saved slot). It's out of protocol 4's scope: it's a box feature with no design yet. If it lands as a `ParamSpec` on an ORBIT page, the protocol picks it up from the tables with no grammar change.

**Recommendation:** song a, print a, move in (it's the editor's job and cheap), advance out of scope.

### 2. May the editor load or save projects, and may it ever format or erase anything on the card?

The card is internal (files move over USB, never fill or junk it), and tester-facing paths never include destructive commands.

- **a. Read-only.** The editor lists projects and shows the loaded one's name and edited state. Saving and loading stay on the box.
- **b. Save.** `ed project save` (MENU hold's quick save over the loaded project's own slot) and `ed project saveas <name>` into a new slot, never over another project.
- **c. b plus load.** `ed project load <id> <gen>`, witnessed by the listing's generation. With unsaved edits it refuses, unless the editor sends `discard`, after its own confirm dialog naming the edited Parts.
- **d. c plus MANAGE.** RENAME, CLEAR and DELETE a project from the editor.
- **Formatting or erasing the card:** never through `ed`, in any option. No `ed` command removes a card file. The card's files belong to #1015, over the existing `ls`, `get`, `put` and `rm`, which already guard every write.

**Recommendation: c.** Save and load are the main reason to have a big screen. The witness and the explicit `discard` make a load as safe as the box's prompt. DELETE, CLEAR and RENAME (d) stay on the box for 0.4. Format: never.

### 3. Which destructive actions may the protocol expose?

Each would come with the editor's own confirm dialog and a witness (principle 2), so a stale confirm writes nothing.

| Action | Undo on the box | Options | Recommendation |
|---|---|---|---|
| Save a scene over another saved scene | none | in / out | **in**, witnessed by the slot's rev |
| CLEAR a scene | none (RELOAD only before saving) | in for the playing scene only (as the box) / any slot / out | **playing scene only**, as the box |
| Remove a Sound's MIDI map row | RELOAD FROM PROJ (a Sound edit) | in / out | **in**, witnessed by the row |
| Remove a controller map's row (writes the card) | none | in / out | **in**, witnessed, with a confirm |
| Save rows into the picked map (overwrites it on the card) | none | in / save-as only / out | **in**: it's EDIT on the box, which doesn't ask either |
| Rename a map | rename back | in / out | **in** |
| Delete a map file | none | in / out | **out** for 0.4: do it on the box (MIX+MINUS on row 0) |
| RANDOMIZE's roll (any set of RANDOM.*) | RELOAD FROM PROJ | in / out | **in**: it's an edit, and the editor labels it "ROLLS THE SOUND" as the box does |
| Remove a matrix route | none (the box doesn't ask) | in / out | **in**, as the box's MIX+MINUS |
| Part CLEAR, RELOAD FROM PROJ, SAVE TO LIBRARY | varies | in / out | **out** for 0.4 unless you want them |

### 4. Does 0.4's editor edit ORBIT's notes and chords?

"Full ORBIT editing" was in your scope list. Chord degrees per segment, the CUSTOM scale, STEP notes (up to 32 a ring) and pool notes (up to 8) are scene data, but no `ParamSpec`s, so they need their own small commands.

- **a. In protocol 4.** `ed chord <seg> <degree>`, `ed scale <mask>`, `ed step <ring> <i> <note>`, `ed pool <ring> <notes…>`, each through `OrbitSong::edit`, and `chord`, `scale`, `step` and `pool` lines in `since`. About +2 KB of bank 2 and 200 B of ITCM. Capture from the keys stays on the box.
- **b. Later.** The editor shows the rings' params and the scenes. Notes and chords are set on the box.

**Recommendation: a.** It's where a big screen helps most (a piano roll of the pool, the progression as a row), and it's small.

### 5. Matrix amounts: the box's ±127, or ±100 in the editor?

The box stores and shows a route's amount as −127..+127 (`+42`). The patch-bay notes say −100..+100.

- **a.** The editor shows the box's numbers. That's protocol 3's rule (values read as the box writes them), and an amount reads the same on both.
- **b.** The editor shows percent. Then the box should also show percent (a box UI change, sound-identical), or the two disagree.

**Recommendation: a** for 0.4. If you prefer percent, change it on the box too, and the protocol follows the box's text.

### 6. May `ed learn` move the box?

Learn arms on the box's focused cell. To learn a cell from the editor:

- **a.** `ed learn` takes the box to that cell's page, focuses it and arms it. You see the learn on the box, which is where you'll turn the controller. It costs no bank 1.
- **b.** Learn arms on the address without moving the box. That needs a target in bank 1's `Learn` state, and the box shows nothing.

**Recommendation: a.**
