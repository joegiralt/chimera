# Browser editor, slice 1b: every engine

**Status:** draft 2026-11-18, waiting on the owner for the three questions at the end (#1014). Nothing is built on them.

## Intent

The editor reaches ALGO only. `ed describe` walks `ALGO_FORMANT_CHAIN`, `ed set` refuses a Part that isn't ALGO, `ed since` answers a non-ALGO Part with its `part` line alone, and the editor says "THE EDITOR EDITS ALGO FOR NOW." This slice brings MODAL, VA, TAPE and DRUM to the same footing, so the editor is worth unhiding on the site at 0.4 (`Features.editor`). It also folds in #996 (a choice whose name has a space can't be set), #997 (`describe` emits each param's scale) and one of #1002's items.

It follows slice 1 (`2026-11-14-editor-algo-slice-design.md`, ADR 0276) everywhere: the unit describes itself from its own tables, the editor holds no parameter list, values read as the box writes them, the editor pulls with `ed since`, and `set` takes the knob path. Nothing here adds a new mechanism where slice 1 has one.

## Non-goals

- The Part mod matrix and patch bay (slice 2). MOD's MTX cells stay `route "<label>" next-slice` on every engine.
- Mixer, FX returns, ORBIT, card files (#1015), update (#1016), saving.
- Per-engine "ancestor" looks (Rings, Prophet, Machinedrum, a reel deck). See question 3.

## What each engine's screen shows

Each engine's screen holds every page its chain shows on the box, all at once, in chain order, as ALGO's does. The shared nodes (FLT with MOD, FX and JACKS; AMP with SENDS; MOD's ENV1–3, SPEED and LFO1–3; PIT) are the same panels ALGO has. The editor's layout code for them moves out of `Layout/Algo.elm` into a shared module, unchanged.

| Engine | Chain | Engine pages | Panels with variants |
|---|---|---|---|
| ALGO | `ALGO_FORMANT_CHAIN` | ALG, WAVE and its 19 sub-pages, OP1–OP6 | OPn per `OpKind` (as today) |
| MODAL | `MODAL_PLUCK_CHAIN` | EXC, RES (home), MDL2, PIT | EXC and MDL2 per `ResonatorMode`: STRING, MODAL, BOWED, SYMP (`modal::page_cells`). BOWED's MDL2 is empty, so it shows no panel. |
| VA | `VA_CHAIN` | VA (home), OSC1, OSC2, NOISE, PIT | none: every cell is a fixed param |
| TAPE | `TAPE_CHAIN` | SRC, DECK (home), MODEL, PIT | DECK and MODEL per `TapeMachine` (WALK, STRETCH, SCRUB, WAVE), labelled by the machine (`MachineSlot`, ADR 0178). SRC's SOURCE cell: question 2. |
| DRUM | `DRUM_CHAIN` | KIT, KEYS, MCH, MORE, pad FLT, pad AMP, GATE, PENV, PLFO, ROUTE; the Part's FLT subs, SENDS and MOD pages | MCH and MORE per `Machine` (12, `laws::spec`); the pad's FLT per the pad's `FilterKind`; the pad's 4 routes. How the 12 pads are addressed and shown: question 1. |

FILTER, FX, ENV and LFO panels keep slice 1's variants on every engine.

## How `ed describe` generalises

- **One chain per engine, typed.** `describe::chain(engine) -> &'static ChainDef2`, an exhaustive match over `EngineType` with no `_` arm. `pages(engine)` and `def_at(engine, n)` walk it. `ed describe <ENGINE> <page>` already takes the engine. Every engine is described, so the `0 pages` case goes away.
- **`Group` gains one variant per new binding**, each named by the firmware's own names, never a new list:
  - `Modal(ModalPage, ResonatorMode)`, panel `EXC` or `MDL2`, variant the MODE option name.
  - `Machine(MachineSlot page, TapeMachine)`, panel `DECK` or `MODEL`, variant the MODEL option name.
  - DRUM's `Knob(Machine)` and `PadFilter(FilterKind)`, panel `MCH` and `PADFLT`, variant the MACHINE option name and the filter kind's name.
  `Group::on` and `Group::cell` stay exhaustive over `SlotBinding`. Today they end in `_ => false` and `_ => None` across two owned enums, and this slice replaces those arms with the full match, as the standard asks. A binding no editor cell can show (`Legacy`, `SelectedRing`, `Orbit`: no engine chain holds them) is matched by name and returns `None`, and a test proves no engine chain holds one.
- **The variants a page lists are per engine.** `group_at(i)` becomes `group_at(engine, i)`. The common groups come first, then the engine's own: OPs only for ALGO, EXC and MDL2 only for MODAL, and so on. The walk and `mark_described` stay one walk, so `since` can't report an address `describe` didn't list.
- **The param line gains its scale (#997).** It becomes `param <addr> "<label>" <kind> <lin|oct> <min> <max> <step> <default>`, from `ParamSpec::scale` (`Scale::Linear` or `Scale::Octaves`, matched exhaustively, not guessed). `Gesture.scaleOf`'s guess is deleted, and the editor reads the scale from the schema.
- **Choices by index (#996).** `ed set <addr> #<n>` sets option `n` (`EdValue::Index(OptIndex)`, checked against the cell's names once at parse time). `#` can't start a name or a number, so it collides with neither. The editor sends every choice by index, so `GATE EXT` and any future name with a space just work. Names stay accepted for a human at a terminal. ADR 0276's "never by number" is superseded in part by the new ADR. The index is the `opt` line's own, which `describe` already emits.

## What `ed since`, `ed get` and `ed set` change

- **`in_schema` becomes per engine.** `in_schema(engine, block)` is an exhaustive match over `EngineType`. MODAL keeps `Modal`, VA `Va`, TAPE `TapeEngine`, and each keeps the shared blocks (Filter, VoiceFx, Env, Lfo, Out, Pitch). The slot layout is per engine, so a slot never moves within an engine, and a new engine answers whole, as a new Part does today.
- **`SLOTS` is the most any engine uses.** It is a `const` checked by a test over every engine, as today's is. ALGO is about 218, and MODAL, VA and TAPE are under it. DRUM depends on question 1: about 50 slots for the selected pad, or about 590 for all twelve. The copy is 4 B a slot in ITCM (ADR 0266), which has about 50 KB free.
- **`PANELS` becomes per engine.** Seen keeps `[&str; PANELS]`, with `PANELS` the most any engine shows: 14, ALGO's. Each engine's names are listed once, beside its groups.
- **`value::reading` and `edits::fmt_of` walk the followed Part's chain**, not ALGO's.
- **`set` checks the engine the address belongs to**, not `== Algo`. `Pending` keeps the engine it was taken for, and `apply` drops a set whose Part has since changed engine. This replaces "a Part that is no longer ALGO drops its sets".
- **DRUM's CHOKE goes through ADR 0280's check.** `ed set` of a CHOKE reaches `edit_in`, which already routes a CHOKE edit through `kit_groups::Ask::Edit`. A 9th group is refused with its toast, nothing is written, and the editor sees the unchanged value. ADR 0280's "the console's `ed set` reaches only ALGO's cells" stops being true, and the new ADR says so. A test pins it.
- **TAPE's held turn** (`Held::Sound(Pick::Tape(..))`) is box hover state, as ALGO's held WAVE and ALG are. It stays #1002's "dimmed values" item and isn't folded in here.

## Protocol version

`PROTOCOL` goes from 2 to 3, because the `param` line's grammar changes (the scale), `set` takes `#n`, and every engine answers. The editor requires exactly 3, as slice 1 required 2. A unit on 2 shows UPDATE THE UNIT, and a higher one UPDATE THE EDITOR. The site isn't touched: it ships no editor until 0.4, so there is no old editor in the field to keep working.

## Firmware placement

- Everything new is in `console::ed`, which `bank2.x` already places in bank 2 S4 behind `EdDoor` (`check-door-placed`). The door's signature doesn't change, and bank 1 should gain nothing.
- The risk is shared generic code getting instantiated for the walk: `modal::page_cells`, the tape machine labels, `laws::spec` and `filter_panel::knob` are called from the new groups. Each is checked with `nm` on the ORBIT and default builds. Anything that lands in bank 1 is moved or wrapped, as `addr::block_token` is. The bank 1 and bank 2 budget lines get measured `ADR 0285 +N … (from X)` notes. ORBIT's bank 2 S0–S3 is untouched (S4 code), and its ≥ 8 KB free is checked.
- ITCM: `EdState` grows by 4 B for each slot `SLOTS` grows. That's 0 for MODAL, VA and TAPE, and for DRUM it depends on question 1. `just check-itcm` still runs.

## The editor (Elm)

- `Unit.Engine` becomes the five engines (`Algo | Modal | Va | Tape | Drum`), replacing `Other String`. An engine name the editor doesn't know is a protocol refusal, which shows UPDATE THE EDITOR. `Main` stops gating the screen on `Algo`. Each engine gets `Layout/<Engine>.elm` and `View/<Engine>.elm` on the shared sections, and a `case` over `Engine` picks them, with no catch-all (elm-review `NoCatchAllForSpecificRemainingPatterns`).
- On connect and on every engine change, the editor describes that engine (`ed describe <ENGINE> 0..n`) and caches each engine's schema for the session. The schema is static per engine and firmware.
- Placement is checked per engine. The fixtures from `gen-fixture.js` are generated for each engine from the simulator, and `LayoutTest` fails if any described address of any engine has no place. The unplaced strip stays as the visible fallback.
- #1002's back-to-back answers: the editor ignores an `ed since` answer that arrives when none is outstanding (the second of a pair after a stall). That is the narrow fix. The rest of #1002 stays open (see below).

## Testing

- **Host (firmware):** for each engine, `describe` covers every cell of every page of its chain in every variant (the slice 1 coverage test, looped over `EngineType::ALL`). `set` then `get` round-trips through the knob path. `since` reports a knob turn, a CC and a Sound load, and answers whole on an engine change. `#n` sets a choice and an out-of-range `#n` is refused. The scale field matches `ParamSpec::scale` for every param. A CHOKE `set` past 8 groups writes nothing. The protocol fixtures are re-recorded at 3.
- **Elm:** protocol parse of the new `param` line and `#n` encode, per-engine layout coverage, and the stale-answer drop.
- **e2e (`editor/e2e/sim.mjs`):** one pass per engine on the simulator: set the Part's engine, then change every described address and see it in `ed get`, and turn a simulated knob and see the editor follow.
- **Unit gate:** the same e2e with `ED_SERIAL` after merge and flash (not in this task).

## #1002, what this slice leaves open

- Dimmed values, now ALGO's and TAPE's held turns: a `held` line in `since` is its own small protocol change.
- The answer size bound is a measured figure from the unit, and needs a flash.
- Text-context changes still answer everything. That's still fine, and DRUM's larger answer (question 1, option B) is the first place it might matter. It gets measured on the unit.
- The gate-build console answers belong to `chimera-usb.py`, not the editor.

## ADR

ADR 0285, "The editor protocol 3: every engine, scales, choices by index", supersedes ADR 0276 in part ("Choices are set by their option names, never by number", "`ed describe ALGO <page>`", the 218-address cost). It also notes what it changes in ADR 0280 ("the console's `ed set` reaches only ALGO's cells"). If question 1 is answered B, it also freezes DRUM's pad tokens.

## Questions for the owner

### 1. How does the editor show and address DRUM's 12 pads?

On the box, every DRUM page edits the Part's selected pad (`BlockRef::Pad`, `PadFilter`, `PadAmp`, `PadEnv`, `PadLfo` and `PadRoute(r)` all resolve against it), and none of them has a disk ident. ADR 0276 freezes addresses as the patch file's tokens, so DRUM needs new frozen tokens either way.

- **A. Follow the box's pad.** Tokens address the selected pad (`PAD.LEVEL`, `PADFLT.CUTOFF`). A new `ed pad <n>` selects a pad on the box, as `ed part` selects a Part, and `since` reports the selected pad. The editor shows one pad at a time with a 12-pad strip. This is cheap (about 50 slots, existing edit paths). But an `ed set` lands on whatever pad the box has selected when the UI pass runs: press another pad on the box between the click and the pass, and the edit lands on the wrong pad. That's a sync bug class.
- **B. Every pad addressed.** Tokens carry the pad (`PAD1.LEVEL` … `PAD12.LEVEL`, `PAD1_FLT.CUTOFF`, `PAD1_RT2.AMOUNT`). The editor shows the whole kit at once: a 12-row overview of MACHINE, LEVEL, PAN, CHOKE and RETRIG (Machinedrum's kit view), plus one pad's full detail, chosen in the editor without moving the box. The cost is about 590 slots (about 2.3 KB more ITCM, which has room), a full DRUM answer of roughly 25 KB, and a pad-targeted edit path in bank 2: today `edit_to` reaches only the selected pad, and CHOKE's group check must run on the named pad.

**Recommendation: B.** It removes the wrong-pad race by construction, which is the kind of design the owner prefers over detecting drift (2026-11-16). It matches ALGO's precedent, where all six operators are shown although the box selects one. And seeing a whole kit is what the editor is for. The pad routes (4 a pad) are plain `ParamSpec` params, so they're edited as rows here, unlike the Part matrix (slice 2).

### 2. What does TAPE's SOURCE do over USB?

SOURCE isn't a param. It's the Sound's `SourceRef` (a CRC, frames and rate), and picking one on the box loads a prepared sample from the card as a job.

- **a.** Read-only in this slice: the editor shows the source's name as the box writes it (a new `source "<name>"` line in `since`). Picking stays on the box.
- **b.** A picker now: `ed sources` lists the card's prepared sources and `ed source <name>` queues the box's own load job.
- **c.** Leave SOURCE out until #1015.

**Recommendation: a**, with the picker built in #1015 (files on the unit), which owns listing the card. Building it here would design card listing twice.

### 3. Do the new screens need a mockup approved first?

ALGO's screen follows the owner-approved mockup v10. Nothing exists yet for MODAL, VA, TAPE or DRUM, and the memory notes say each engine eventually gets its ancestor's look.

**Recommendation:** for 0.4, lay each engine out on ALGO's grid and widgets: Prophet-6 sections in the box's chain order, the shared FLT, AMP, MOD and PIT sections exactly as ALGO's, and glyph placeholders as ALGO has. I'd show one mockup artifact with all four for a yes or no before writing the Elm screens. The ancestor looks stay a later pass. The firmware half (protocol 3, all engines described, host tests, simulator) doesn't depend on this answer, and could start as soon as question 1 is answered.
