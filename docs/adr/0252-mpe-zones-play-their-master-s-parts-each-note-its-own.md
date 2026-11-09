# 0252. Play MPE zones on their master channel's Parts, each note's expression its own

- **Status:** Proposed
- **Deciders:** owner (0.3 scope, #845: MPE zones, per-note bend, pressure and CC 74, MCM, bend range per zone, on the per-Part map); firmware

## Context
0.3 makes Chimera answer MIDI controllers fully (#845). An MPE controller sends each note on its own member channel with its own pitch bend, channel pressure and CC 74 (timbre); a zone's master channel carries what reaches every note. Chimera's Parts each listen on one channel (`PartParams::channel`), the bend glides per Part (ADR 0128), and the wheel and pressure are matrix sources (ADR 0236), whose §6 left room for MPE: a voice's own pressure, and timbre "a third per-voice source the same way". The note queue packs a kind above bit 18 (nine kinds used). MIDI in parses in bank 2 behind `MidiDoor`, bindings resolve in bank 2 behind `MidiUiDoor` (ADR 0217); the Instrument and the voices, bank 1's, play the audio a closed door must still play (ADR 0198, 0236).

## Decision
1. **Zones** (`chimera_engine::mpe::Zones`): a lower zone, master channel 1, members 2 up; an upper zone, master 16, members 15 down; each its member count (0 off, at most 15) and its members' bend range (0–96 semitones, the spec's 48 by default). Zones never overlap: setting one shrinks the other (15 lower leaves no upper zone). Off out of the box.
2. **A zone plays its master's Parts.** A live note on a member channel plays every Part on the master channel and no Part on the member channel; ORBIT's notes, already on their Part's channel, are never moved. The voice keeps the member channel (`note_channel`), so its note-off, poly pressure and own expression find it, and the master it played (`master`, a byte a voice, set at note-on), so the master's sustain pedal, All Notes Off and All Sound Off reach it even after the zones change.
3. **Each note's own expression** (`Voice::ease_own`, `Own`): a live note on a member channel (never ORBIT's), its bend over the zone's range (on top of the Part's, which the master's wheel moves over the Part's BEND), its pressure (PRESSURE reads the higher of the Part's and its own, as poly pressure), and its CC 74. All three glide at the KNOB coefficient, never step; a new note's land at its channel's values at note-on (an MPE sender sets them before the note), so no note scoops in.
4. **SLIDE, the eleventh matrix row** (`ModSource::Timbre`, index 10): CC 74 on the Part's channel, 0..1, glided per Part; a voice reads the higher of that and its own member channel's. Readout and tag `SLIDE` (the five-letter row label; ROLI's and Ableton's name for MPE's timbre), disk code 12, ident `TIMBRE`; its routes go to `PerfRoutes` with WHEEL's and PRESSURE's (a full record 240 B). CC 74 rides the note queue as `Expr::Timbre` (kind 10) and still reaches the map, as CC 1 does; a refused one is owed, latest wins (`ExprOwed` a bit per channel and kind, now 48 in a `u64`).
5. **Configuration.** An MPE Configuration Message (RPN 6 on channel 1 or 16, the MSB its member count) sets that zone and its member bend to 48; RPN 0 on a member channel sets its zone's member bend; RPN 0 on a master is the Part's BEND, as before. A member channel's other controls are its note's, never a Part map's, and learn never hears them. RPN 6 elsewhere is no message of MPE's. MIDI CONFIG › MPE sets them by hand: A LOWER (OFF, 1–15), B L BEND, C UPPER, D U BEND (0–96). The zones are SYSTEM's (`MidiSettings::mpe`, one word, params 5–8, idents `MPE_LOWER`, `MPE_LBEND`, `MPE_UPPER`, `MPE_UBEND`; 4 held for PROGRAM IN, #891), published to the audio each pass (`AudioShared::mpe`).
6. **Placement.** Bank 2 (`chimera_engine::mpe`, one `MidiDoor` entry, `mpe::Call`): a live note's master (`kern::master`), every voice's own targets once a block (`kern::own`), the four cells' writes (`kern::write`; closed, a write stores the value as given, no shrink rule, so a SYSTEM loaded and saved with the door closed keeps its zones, as #815's maps); the MCM and member-channel handling (`UiState::cc_mpe`, behind `MidiUiDoor`); CC 74's parse and owing (the pipe). Bank 1: the door stubs, each voice's ease of its own (three glides), its stored master, a high note's zone reach, SLIDE's row in the voice, the publish's copy, and MPE's closed-door data (its four param specs, the OFF–15 names, page, leaf and row, SLIDE's disk code). Closed: every channel its own, each voice its poly pressure alone; MIDI in is deaf anyway.

## Alternatives considered
- **MPE a per-Part switch** (each Part its own zone). The spec's zones are the device's, set by one message; a Part on channel 1 or 16 already says which.
- **The pipe moving member notes to the master channel.** The voice must know the member channel for its own bend and pressure; the note would need a second channel in the queue's 32 bits.
- **CC 74 driving the WHEEL row per note.** No row for a Part's CC 74 then, and a wheel route would move with a finger's slide.
- **A Part's BEND at ±2 on a configuration message** (the spec's master default). It would rewrite a Part's saved BEND each time a controller starts; the Part's BEND stays the hand's.
- **The zones in the project.** They describe the controller on the desk, as SYNC and ECHO do: SYSTEM's.

## Consequences
- **RAM:** a voice grows 16 B (`VOICE_CHAIN_BYTES` 2,448 → 2,464; SLIDE's value, own bend and slide), the Instrument 24 B more for CC 74 a channel and a master a voice, a Part's knobs 12 B, `ModState` 20 B (a row of amounts and of fixed amounts); `MAX_SOUND_COST` 2,928 → 3,008 (`PerfRoutes` full at 244 B).
- **Matrix:** ALGO's thirteen rows sit at 10 px (cells 8 px), the other engines' eleven at 12 px.
- **The door:** one entry for MPE, so ORBIT's table keeps its 256 B. Own targets cost one door call a block for all voices, not one a voice (a note-on reads them for the voice it starts: one read a note event measured 408 B more of bank 1), so no bench row was needed to weigh a per-voice call; the ease itself (three glides a voice) stays in bank 1 beside the voice.
- **Bank 1** grows by the measured bytes (`CHIMERA_STAMP_CLEAN=1`, over main fde5e961's lines), each line in `tools/bank1-budget.txt` naming this ADR:

  | Set | Bank 1 |
  |---|---|
  | `default` | +1,624 B |
  | `orbit` | +1,488 |
  | `bank2-gate` | +1,512 |
  | `usb-audio` | +1,632 |
  | `fault-inject` | +2,208 |
  | `bench-algo` | +1,864 |
  | `bench-algo,master-tape` | +2,056 |
  | `bench-routing` | +2,240 |
  | `bench-memory` | +2,384 |
  | `bench-stress` | +2,928 |
  | `bench-orbit` | +2,576 |
  | `bench-tape` | +2,728 |
  | `filter-probes` | +2,192 |

  On `default`: the closed-door data about 700 B (`MIDI_SPECS` 288, the names 128, the page 120, leaf, row and SLIDE's code); `MidiSettings` 3 → 8 B in SYSTEM's copies and compares about 1,070 B (`UiState::input` 312, `synth` 252, `SystemSync::sync` 246, `take_ui` 100, `region_data` 88); the audio block's ease and stub, a high note's reach (`recost` 200) about 760 B; SLIDE's row through the codec about 160 B. Bank 2 (`default`) +1,312 B.
- **Not moved:** `SystemSync::sync` takes SYSTEM by reference, not a copy: 104 to 112 B back on every set. `Zones` stored as four bytes, to keep `SystemSettings`' alignment, cost `fault-inject` 504 B more and saved nothing elsewhere, so it stays a word. What stays (over f19b95bb: `sync` +326, `UiState::input` +324, `synth` +252, `take_ui` +116, `region_data` +112) follows MIDI's block having eight params, not four, through the SYSTEM codec, the settings edit path and the boot's build, not a struct copy; keeping the zones out of SYSTEM's struct would need a global the SYSTEM codec reads, which the tests share across threads. The owner accepted it under ADR 0251 (2026-11-09).
- **Bench RAM:** `bench-algo`'s D2 had 48 B free: its report text gives 384 B more (6,016 → 5,632; 8 B left). Builds without `midi-map` (deaf: the bench sets) keep one canonical NRPN slot and one wait a Part, not 8 and 16, which gives their AXI margin back about 1.1 KB (`bench-algo` 1,260 B over its 8 KiB). Whether `bench-algo`'s report still fits 5,632 B is the unit's to show (`# TRUNCATED`).
- **Billing:** a high note's reach adds its zone's member bend (`Zones::reach`); a TAPE note's stream is billed on its Part's bend at note-on, as before: a member bend beyond it caps as a wheel push does.
- Per-note expression arrives only through MIDI; ORBIT's notes are on their Part's channel and have none.
- **CONTROL channels** (ADR 0254): a member channel of an MPE zone is the zone's, never MIXER's, SEQUENCER's or AUTO's (`control::control_on` yields to it, as to a Part's channel); `cc_mpe` runs before `cc_dedicated`. AUTO on channel 1 or 16 with no Part there moves an MPE Configuration Message to the focused Part's channel with everything else, so the controller's zone never configures: AUTO's rewrite is per message at layer 1, before the RPN is known, and passing RPN 6 through would track each channel's RPN selection there. Set the zones on MIDI CONFIG › MPE, or keep AUTO off channels 1 and 16, with an MPE controller.

## Sources
- MIDI Polyphonic Expression 1.0 (MMA, 2018): zones, the MCM, member and master bend sensitivity defaults, CC 74 as the third dimension.
- #845, #514, #807; ADR 0128 (bend per Part), 0198 (bank 1 frozen), 0217 (the parser and doors), 0226 (maps), 0236 (wheel and pressure).
