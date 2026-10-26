# 0217. Controllers bind through one typed model: input kind, target, feedback

- **Status:** Proposed (would supersede in part 0139: its pickup and scale rejection, "values are absolute 7-bit and jump", "14-bit or NRPN post-alpha", `0x001E` as the written record, the MIDI MAP page's keys; would amend 0128: RPN 0 sets BEND)
- **Deciders:** owner (0.3 scope; type-driven, functional core and imperative shell; per-Part CC control; bank 1 frozen), firmware

## Context
ADR 0139 shipped a 16-entry per-Sound CC map: absolute 7-bit values that jump. The 0.3 research (`reports/MIDI controller and USB schemes.md`) found that generic controllers fail for two more reasons:
- The receiver guesses an encoding it should have asked about. Four relative byte encodings and NRPN increment/decrement all ship in current hardware, and one byte (01) means +1 in one and −63 in another.
- The receiver can't send the controller its current value.

The DIN parser (`chimera-hal::midi::MidiParser`) also lacks SysEx state, System Common cancel, CC 124–127 and Active Sensing. Bank 1 is frozen (ADR 0198).

## Decision
- **A two-layer typed parser** in a new leaf crate `chimera-midi` (`no_std`, no dependencies, `forbid(unsafe_code)`).
  - Layer 1 turns bytes into events. Real-time is returned apart without disturbing a half-built message. A note-on of velocity 0 is a note-off. Channel Mode is its own enum. SysEx sits in a fixed buffer, and faults are typed.
  - Layer 2 is per source and per channel: RPN/NRPN selection (last written wins, 127/127 null for both), Data Entry applied at the MSB and refined at the LSB, increment/decrement one step with the data byte ignored, and a bank latch. Its output is a `Ctl` enum.
  - Both layers are host-tested, property-tested and fuzzed.
- **One binding type**, stored in the Sound's map (ADR 0139's place, 16 entries, the Part's channel):
  - `Binding { input: Input, target: Mappable, span, feedback: Feedback }`.
  - `Input` is one enum: `Cc(LearnCc, CcKind)`, `Cc14(Cc14Msb, Takeover)`, `Nrpn(U14, NrpnKind)` or `Pressure(Takeover)`.
  - `CcKind` is `Absolute(Takeover) | Relative(ByteEncoding) | Button(ButtonMode)`. `NrpnKind` adds `IncDec`.
  - `ByteEncoding` is `TwosComplement | BinaryOffset | SignBitDown | SignBitUp`, `Takeover` is `Jump | Pickup | Scale`, `ButtonMode` is `Momentary | Toggle`, and `Feedback` is `Off | Value`.
  - Illegal pairs (increment/decrement on a CC, relative on pressure) can't be built.
- **Arithmetic.**
  - A relative unit is one panel detent through the encoder's edit path. Controller-side acceleration is taken at face value, and there is no receiver acceleration.
  - `Pickup` engages within 2/127 of the span or on a crossing, and re-arms on any other change. `Scale` converges both ends (Ableton's value scaling).
  - Every absolute move still goes through ADR 0139's ramp and the knob glide: never a snap.
  - Default takeover is `Jump`. Echo defaults to off; learn turns it on for relative encoders and 14-bit controls (CC14, NRPN 14-bit). MIDI MAP's ECHO overrides per binding.
- **Learn senses the kind.** MIX+SEQ arms as today. The first control heard writes the entry, then learn stays open (no timer) while `classify` reads up to 24 samples and rewrites the entry's `Input`. Nothing the sensed control sends is applied while sensing. MIX+SEQ, a nav key, a page change or a panel turn ends it. MIDI MAP's E (INPUT) and F (ECHO) correct any guess.
- **Value echo out of DIN, per Part.**
  - A pull model: each UI pass compares each feedback binding's quantised param to what was last sent. It sends on the Part's channel, on the binding's own control and encoding.
  - A binding's own echo is held for 250 ms after it last received.
  - The dirty set coalesces, so the latest value wins.
  - Bursts on a Part change, a Sound load and a project load.
  - Priority: identity, then the active Part, then the rest.
  - On a shared channel only the lowest Part echoes.
  - A 256 B `OutRing` feeds a USART1 TX interrupt.
  - SETTINGS › MIDI CONFIG › ECHO (`OFF`/`ON`, SYSTEM `Midi` param 3, default `ON`) is the master switch for all output; each binding's `Feedback` decides what is echoed.
- **Universal Identity Reply.** `F0 7E <7F|00> 06 01 F7` is answered with `F0 7E 00 06 02 7D 43 48 01 00 <maj min patch 0> F7`. `7D` (non-commercial) is the owner's pick for now; a bought MMA ID replaces it before anything is sold.
- **Stream policy:**
  - CC 124–127 act only as All Notes Off (Mode 3 per Part). CC 122 is ignored.
  - FF is All Sound Off plus Reset All Controllers on every channel.
  - Active Sensing arms on the first FE and times out after 300 ms through USART1's receiver timeout, releasing every channel's notes.
- **Persistence.** A new non-critical Sound record `Bindings` `0x001F`: a version byte, then 8 B an entry. `0x001E` is read as `Cc(_, Absolute(Jump))` with echo off, and never written. Bad entries are skipped and counted.
- **Placement (ADR 0198).**
  - **Bank 2, behind `Bank2Door::midi` (`MidiDoor`):** `chimera_midi`'s `stream`, `ctl`, `rel`, `take`, `law`, `learn`, `out`, `sysex` and `sense`; `chimera_engine::midi_in`'s pipe; `nrpn`; the map codec.
  - **Bank 2, behind `MidiUiDoor` at `DOOR_ADDR + 0x300`:** the UI's resolution, learn sensing, echo and MIDI MAP page, together with today's `cc.rs`, `midi_map::ramp` and the map page's code.
    - Its state (`MidiUi`: each Part's ramps, bins, takeover runtime, detents waiting and canonical-NRPN slots) lives in AXI `.bss`, not in D2 with `UiState`, which bench builds fill. Bank 2 builds it on first use from a named static (`ui::midi::REST`), so bank 1 carries no image of it. Bank 1 keeps `ui::cc`: the slot, the stubs and the hand's count.
    - A Part ramps at most eight canonical NRPN params at once, and holds the latest value of 16 more on a wait line until a slot frees. An address arriving past those 24 is dropped.
    - Relative detents heard in one pass turn their entry once, as their sum, through `edit_addr`, under the same room as a ramp.
    - MIDI MAP's keys and words are bank 2's (`ui::midi::page`): bank 1 reads one binding's words a door call (`map_text`) and draws the rows and the bar's lines, and its key stub hands bank 2 the six detents and MIX+MINUS. Learn's armed and refusal words are bank 2's too (`note`), from named word tables, never `write!`. The pickup hint's way is read through the door (`focus_note`).
  - **The USART1 ISR** drains the RX FIFO and makes one door call per entry: about 0.1 % of the CPU at the DIN maximum (estimate, measured by `MIDI_CYCLES`).
  - **DIN OUT.** The UI's out pass (`MidiUiDoor::out`, bank 2) writes whole messages into the `OutRing`, then the main loop pends USART1. The ISR moves the ring into the TX FIFO and keeps `TXFTIE` set only while bytes wait, so every `CR3` write is the ISR's. DIN OUT links only with `midi-map`; bench builds are deaf to controllers, so they send nothing.
  - **Value echo.** `ui::midi::echo::EchoPlan` lives in `MidiUi` (AXI). It goes through the same out pass, so there is one bank-1 stub. Each pass compares every feedback binding's param, read in its control's resolution, with what it last sent. It sends on the Part's channel, with running status inside a pass, and in this order: identity reply, active Part, other Parts, bursts. A burst comes from a load, a Part change or a map change. A binding that heard its control waits 250 ms. MIDI CONFIG › ECHO (`MidiSettings` param 3) is DIN OUT's master switch. PUSH and ECHO share one byte (`Switches`), so `MidiSettings` stays 3 bytes and SYSTEM's copies in bank 1 keep their shape.
  - **Bank 1 keeps:** the value newtypes; the `Binding`, `Input` and `MidiMap` type definitions and default map data (the Sound holds them); `InEvent`, `NoteQueue`, `CtrlRing` and `OutRing`; the ISR stub; the UI stubs; the ECHO spec.
  - **Bank 1 exceptions** (ADR 0198 point 1; each raise carries `# ADR 0217` on its `tools/bank1-budget.txt` line, measured on `orbit`):
    - the `Binding` type growth in each Sound (`Binding` 10 B, `MapEntry` was 6): +856 B;
    - the DIN TX path: PB6 AF7, USART1 `TE` and `TXFTCFG`, the TX FIFO fill from the `OutRing` in the ISR, the out pass's stub and the main loop's pend: +376 B measured (+384 B on the line);
    - ECHO's closed-door data: its spec, page, leaf and row, and the `Midi` block's get and write arms for param 3: +332 B;
    - Active Sensing: the receiver timeout's arm, flag and door call in the USART1 ISR (`RTOR`/`RTOEN` set once at init, `RTOIE` alone arms), FE/NF bytes skipped, the control ring's drops counted apart (`ctrl_drops` in `MidiStats` and the console's status): +184 B.
    - within the check's slack, with no line raised: the MIDI MAP page's stubs (+152 B), and closed learn's `NEEDS UPDATE` with the boot building NEW after a staged install (+40 B).
  - **Bank 1 drops** `MidiParser`, `MidiMessage`, `midi_in::route` and `InEvent::from_midi` (−288 B measured on `orbit`, its line −344 B), then the bindings' resolution with today's `cc.rs` and the ramp (−2844 B on the line), so its lines are lowered.
  - **Closed bank 2:** MIDI in is deaf (notes included), nothing is sent, maps aren't decoded, and MIDI MAP reads `NEEDS UPDATE`.

## Alternatives considered
- **Auto-detecting the encoding silently.** One byte is ambiguous between encodings (report). Sensing with a stated guess and a correction cell is asking, not guessing.
- **Pickup as the default (the report, Ableton).** ADR 0139's jump with a glide is honest on a unit whose own encoders are endless. Elektron, the sequencer precedent this project follows, jumps, and echo removes the jump on capable controllers. One cell flips it.
- **Layer 2 on the UI side, raw CCs in the ring.** A full ring could drop one CC out of an NRPN quadruple and corrupt the selection. In the ISR, a drop loses a whole semantic event.
- **Parsing in the audio block from a raw byte ring.** It moves the parser onto the audio thread and away from where bytes arrive (ADR 0019), and coarsens clock arrival.
- **Batching the RX FIFO with a threshold and receiver timeout.** It adds up to 320 µs of note latency to save cycles that cost 0.1 % of the CPU.
- **A push model for echo (a hook in every edit path).** Every new edit path would need to remember it. The pull compare catches all of them for 96 compares a pass.
- **Keeping the old parser as a closed-door fallback.** It keeps about 1 KB of bank 1 forever so that a stale `.bk2` plays VA's plain modes. The owner rejected it: the header's bank-2 warning covers a stale unit.
- **The binding in the Part (Octatrack, DAW).** Already rejected by 0139: engine-specific targets don't survive engine changes.

## Consequences
- Generic controllers work properly over DIN: EC4, BCR2000, Launch Control XL 3 custom modes, and Hapax, Cirklon and OXI NRPN lanes. Mackie Control and other surfaces become profiles over the same types (Phase 4).
- USB (Phase 2) adds a source, with its own `DinPipe`-style pipe and ring, and no binding change: a cable resolves to a Part before channel routing.
- DIN OUT carries data for the first time. A downstream device on a Part's channel sees the echoes. ECHO `OFF` stops them.
- Old Sounds load with absolute-jump bindings. A closed-door save drops the map.
- **Measured** (`orbit`, base origin/main 4eae5d81 against Phase 1's head; `size -A` of the release ELF):
  - Bank 1 (`target/bank1/orbit.bin`): 824852 → 823460 B (−1392). Bank 2 (`.bk2`): 438368 → 466624 B (+28256), well past the 14–18 KB estimate: learn's sensing, the MIDI MAP page's keys and words, echo and the canonical NRPN line all landed there.
  - RAM: AXI `.data + .bss` 466860 → 474876 B (+8016: `MidiSlot`'s `MidiUi` 6924 B with `EchoPlan` and learn's `Seen`, the DIN pipe and `CtrlRing`, the 256 B `OutRing`); 49412 B of 512 KiB free. D2 `.ram_d2` 242924 → 241336 B (−1588: `UiState`'s `CcState` and the old CC ring left D2 for AXI, net of the Sounds' larger maps). bench-algo's D2 keeps 1784 B free.
  - Every set's `tools/bank1-budget.txt` line, origin/main → now: `orbit` 824892 → 823460 (−1432), `default` 808136 → 806872 (−1264), `bank2-gate` 809940 → 808692 (−1248), `fault-inject` 782088 → 782336 (+248), `bench-algo` 823568 → 824060 (+492), `bench-algo,master-tape` 801136 → 801592 (+456), `bench-routing` 822672 → 823140 (+468), `bench-memory` 825156 → 825632 (+476), `bench-stress` 827192 → 827660 (+468), `bench-orbit` 801292 → 801484 (+192), `bench-tape` 789488 → 790040 (+552), `filter-probes` 783200 → 783440 (+240). The bench sets grow: they link no `midi-map`, so they keep no `cc.rs` to move out, and carry only the type growth, Active Sensing and, on most, ECHO's data.
  - Placement notes: bank-2-only helpers that the machine outliner pulls into `OUTLINED_FUNCTION`s land in bank 1, with no crate path to glob (about 446 B by learn's task); `MidiMap::learn` is `#[inline(always)]` (its callers are bank 2's: −228 B bank 1); learn's and MIDI MAP's words come from named byte tables, never `write!` (a literal costs bank 1's unnamed rodata); values are compared by a packed code, not `Binding`'s `eq`, which would be emitted out of line in bank 1; a second `BlockRef::eq` copy (+50 B) stays in bank 1.
  - The USART1 ISR's two door calls (`feed`, `idle`) are charged their targets' depth by `tools/stack_depth.py`: 312 B.

## Sources
- `reports/MIDI controller and USB schemes.md` (all sections; Phase 1). `research_notes/MIDI controller and USB schemes/` (`midi_core_mpe_relative.md`, `surfaces_feedback.md`, `sysex_midici_definitions.md`).
- MIDI 1.0 summary (midi.org); RP-018 (increment/decrement); Faderfox EC4 manual V03; Behringer BC MIDI Implementation 1.2.9; Launch Control XL 3 programmer's reference; Mackie Control protocol (TouchMCU doc); Ableton Live 12 manual, MIDI and Key Remote Control.
- ADR 0019, 0086, 0128, 0137, 0139, 0175, 0198, 0208, 0216.
- Spec `docs/superpowers/specs/2026-10-25-midi-phase1-design.md`. Plan `docs/superpowers/plans/2026-10-25-midi-phase1.md`.
