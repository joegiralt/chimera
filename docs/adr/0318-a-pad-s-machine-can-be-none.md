# 0318. A pad's MACHINE can be NONE

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner (asked for it, 2026-11-22, for 0.4); Claude (list place, inert cells, glyph)

## Context
A DRUM Part's bill (`kit_bill`) folds every pad of its kit: every filter
shape, the costliest mods, the gate, and the costliest kernel its machines
can run up. Each hit bills its own kernel (ADR 0287), but the rest of the
fold is what admission bills every hit of the Part beside it, so a pad the
player never strikes still costs voices if its filter, routes or gate are
dear: a spare pad on LADDER with a CUTOFF route and its gate open bills
every KICK for LADDER's moving path, the routed mods and the gate. The
owner asked for a way to say "no machine on this pad".

## Decision
- **The type.** A pad holds a `PadMachine`: `Machine(Machine)` or `None`.
  `Machine` stays the runnable machines only, so the kernel, its laws,
  `KernelBill` and `Ticking` never see NONE. A `DrumHit` carries its
  `Machine`: `DrumHit::new` and `KitView::hit` are `None` on a NONE pad, so
  no hit, kernel or bill can be built for one.
- **The code.** NONE is disk code 13, the next free one, never reused (ADR
  0045); ident `NONE`. Codes 1–12 are unchanged, and a code this firmware
  doesn't know still loads as the pad's default. Old kits load exactly as
  before.
- **Its place in the list: last.** MACHINE's value is the place in
  `PadMachine::ALL`, the machines in `Machine::ALL`'s order and then NONE
  (value 12). "First" would read as "empty" at the top of the turn, but it
  would move every machine's value by one: the value is what `ed set
  PAD<n>.MACHINE #i`, the editor's option numbers, the UI's header key and
  `drum_mch_*`'s turns use. Last moves none of them. (Pads are no NRPN,
  MIDI-map or parameter-dump address: `BlockRef::Pad` is not in
  `BlockRef::ALL` and has no disk code, so those wires carry no MACHINE
  value at all.)
- **A strike.** A note on a NONE pad sounds nothing: no voice is booked,
  no voice stolen or faded, no CUT or choke applied, nothing refused or
  counted (`Instrument::choke` returns `None` first). A live one still
  selects its pad on the KIT page (`KitView::struck`, before the NONE
  check), so the player finds the pad they hit. A hit waiting out a fade
  whose pad turns NONE before it starts is dropped unheard, as one whose
  Part left DRUM. TONAL on a NONE pad plays nothing from C1 up, and its
  band says `NONE PLAYS NOTHING`. A hit already ringing when its pad turns
  NONE rings on and keeps its bill until it ends (ADR 0311's
  `ringing_kits`).
- **The bill.** `kit_bill` skips a NONE pad whole: its filter shape, routes,
  mods, gate and kernel. The gain is in the shape, mods and gate every hit
  is billed beside its own kernel; the kernel part lowers only the Part's
  whole-voice bill (`PartBill::whole`). A kit of only NONE pads bills
  `KitBill::NONE`, the fold's empty bill (its kernel the "no kit read"
  costliest; it never runs, as no hit can start).
- **Voice groups.** A NONE pad still counts toward its kit's voice groups
  (ADR 0280), as its choke group or as OFF: were it left out, NONE, a CHOKE
  edit into a new group and a machine again would take a kit past 8.
- **The pages.** The MACHINE cell reads `NONE` and stays live. The machine
  knobs (MCH, MORE) are empty, as a knob the machine lacks always is. Every
  other cell of the pad (LEVEL, PAN, CHOKE, RETRIG, its FLT, AMP, GATE, ENV,
  LFO, routes) is dimmed and inert, the pattern for cells the model ignores
  (TAPE's and MODAL's `dimmed`, the encoder ignored): their values stay, so
  turning MACHINE back brings back the pad's level, pan, chokes and sound.
  Not its knobs: NONE sets them to 0, as a machine's unused knobs are, and
  the machine chosen after NONE takes its own defaults on MCH and MORE, as
  any MACHINE change does. The pad picker, TONAL
  and the Part's FX TYPE and FX PLACE stay live. The header reads `· 01
  NONE`.
- **The glyph.** NONE's icon is a 12 × 10 box in the mark's full tone, its
  inside in the dim tone, an X of two-pixel strokes through its middle in
  the full tone, and no family badge: the same palette and line weights as
  the other machine marks (`kit_band`). The focus band draws it large, the
  band small, the editor's strip through `screen.wasm`. Its mark and the
  TONAL band's words are bank 2's, behind `UiDoor`'s `notes` entry
  (`ui::notes`); with the door closed no box is drawn and TONAL says
  nothing. The word `NONE` is one constant, `PadMachine::NONE_NAME`.
- **The editor.** `ed describe` lists `NONE` as MACHINE's last option and a
  `panel PAD<n>_MCH NONE` with no cells; the Elm editor's DRUM layout reads
  both from the schema (fixtures re-blessed).
- **VARIETY** has no code on 0.4 yet; when it lands it latches per hit, and
  a NONE pad has no hit.
- **Factory kits** are untouched.

## Alternatives considered
- **`Machine::None` as a thirteenth machine.** Every kernel path (`kernel`,
  `cost`, `resolve`, `KernelBill`, the laws' table) would need an arm for a
  machine that can't run, and a hit could be built for it.
- **NONE first in the list.** Reads as "empty", but shifts every stored and
  sent MACHINE value (above).
- **Hiding a NONE pad's cells.** Hidden cells move the page under the
  player's hands, and the values would look lost; dimmed matches how every
  other inert cell reads.
- **A NONE pad still chokes its group.** It has no voice to sound; a strike
  that silently cuts other pads would be a hidden side effect.

## Consequences
- An unused pad costs nothing: setting a dear pad NONE takes its shape,
  mods and gate off every hit's bill, so the kit fits more hits (a test
  admits one more KICK on rev V).
- No new door entry: the checks sit in place (`kit_bill`'s skip, the
  strike's, the cell's dim); NONE's mark and TONAL's words ask through the
  `notes` entry; `ed`'s and the voice groups' parts are bank 2's already.
  Bank 1, ORBIT: +936 B over origin/0.4's line (911,564 → 912,500 B,
  6b85290a); the other sets +848 to +960. The pieces: `choke`, `hit_of`
  out of line (16 B under it inlined), `kit_band::icon`, `KitView::hit`,
  `drum_on`, `is_dimmed`, `Pad::write`, `KitWriter::publish`,
  `kit_band::name`. `SympatheticSet::note_on` is pinned out of line and
  built for size: the merge's inlining had moved it into
  `ModalEngine::strike` with its chord glides unrolled (+1,484 B), and
  `PadMachine` compares its one byte (the derive's match was 96 B). Bank
  2: +128 B.
- The editor schema hash for DRUM changes once.

## Sources
- Owner request, 2026-11-22.
- ADR 0045 (disk codes), 0265 (DRUM), 0269 (pad sounds), 0280 (voice
  groups), 0287 (kernel bills), 0311 (ringing-hit bills, PR #1080).
