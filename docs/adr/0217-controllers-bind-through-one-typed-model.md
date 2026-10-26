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
    - A Part ramps at most four canonical NRPN params at once. An address arriving while all four slots are still moving is dropped.
    - Relative detents heard in one pass turn their entry once, as their sum, through `edit_addr`, under the same room as a ramp.
    - MIDI MAP's keys and words are bank 2's (`ui::midi::page`): bank 1 reads one binding's words a door call (`map_text`) and draws the rows and the bar's lines, and its key stub hands bank 2 the six detents and MIX+MINUS. Learn's armed and refusal words are bank 2's too (`note`), from named word tables, never `write!`. The pickup hint's way is read through the door (`focus_note`).
  - **The USART1 ISR** drains the RX FIFO and makes one door call per entry: about 0.1 % of the CPU at the DIN maximum (estimate, measured by `MIDI_CYCLES`).
  - **Bank 1 keeps:** the value newtypes; the `Binding`, `Input` and `MidiMap` type definitions and default map data (the Sound holds them); `InEvent`, `NoteQueue`, `CtrlRing` and `OutRing`; the ISR stub; the UI stubs; the ECHO spec.
  - **Bank 1 exceptions** (ADR 0198 point 1, measured in their PRs): PB6 AF7, USART1 `TE`/`TXFTIE`/`RTOEN`, and the `Binding` type growth in each Sound.
  - **Bank 1 drops** `MidiParser`, `MidiMessage`, `midi_in::route` and `InEvent::from_midi`, so its lines are lowered.
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
- Bank 2 grows by about 14–18 KB (estimate). Bank 1 shrinks by about 1–2 KB net.

## Sources
- `reports/MIDI controller and USB schemes.md` (all sections; Phase 1). `research_notes/MIDI controller and USB schemes/` (`midi_core_mpe_relative.md`, `surfaces_feedback.md`, `sysex_midici_definitions.md`).
- MIDI 1.0 summary (midi.org); RP-018 (increment/decrement); Faderfox EC4 manual V03; Behringer BC MIDI Implementation 1.2.9; Launch Control XL 3 programmer's reference; Mackie Control protocol (TouchMCU doc); Ableton Live 12 manual, MIDI and Key Remote Control.
- ADR 0019, 0086, 0128, 0137, 0139, 0175, 0198, 0208, 0216.
- Spec `docs/superpowers/specs/2026-10-25-midi-phase1-design.md`. Plan `docs/superpowers/plans/2026-10-25-midi-phase1.md`.
