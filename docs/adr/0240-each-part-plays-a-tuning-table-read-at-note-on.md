# 0240. Each Part plays a tuning table, read at note-on

- **Status:** Proposed
- **Deciders:** owner (0.3 scope, #217: per-Part tuning as stock PreenFM2/3, Scala files over USB, MTS SysEx; "the microtuning UX has to be easy"), firmware

## Context
Stock PreenFM2/3 load Scala tunings per instrument; Chimera played 12-TET only, with an unwired SCALE cell on SETTINGS › TUNING. The owner's notes on #217:
- per Part, like stock's per-instrument scale, saved with the project;
- named temperaments first, picked by ear; Scala `.scl`/`.kbm` the advanced path;
- files copied to the card over USB (the card is inside the case: one folder, never junk it);
- MTS SysEx (single note, scale/octave) on the 0.3 MIDI parser;
- about 3 KB of RAM for a 128-entry table a Part, looked up at note-on, no per-sample cycles; parser and table in bank 2.

Bank 1 is frozen (ADR 0198). Every engine's pitch already takes one offset, `EngineCtx::tune` (SETTINGS › TUNE), through `EngineSlot::set_pitch`.

## Decision
- **The table.** `chimera_engine::tuning::TUNINGS`, a global: 128 offsets from 12-TET, semitones, `f32` in relaxed atomics, one table a Part plus one MTS table, and a bit a Part that plays the MTS table (3,588 B, AXI `.bss`). All zero is 12-TET. Only with the `tuning` feature (every shipping set): the bench builds go without, for their AXI margin, and play 12-TET.
- **The read.** At note-on the `Instrument` reads the Part's offset for the sounding note (after snap) and adds it to the voice's `tune`. A strike starts on it. A glide (STEAL GLIDE on a sounding voice) or a HELD RE-SNAP eases the voice's offset to the new key's at that glide's own pace (STEAL GLIDE's time constant, or RE-SNAP's 40 ms), as the engine's pitch glides, never a step. The per-voice ease is a ZST without `tuning`. A sounding note keeps its pitch when its table changes.
- **The choice.** A Part param, `PartParams::TUNING` (id 11, disk ident `TUNING`, code its index, saved with the project's Part): `12-TET`, `JUST` (5-limit), `PYTHAG`, `MEANTONE` (quarter-comma), `WERCK` (Werckmeister III), `MTS`, `TUNE1`…`TUNE8`. New choices append. The temperaments are rooted on C, C at its 12-TET pitch.
- **The page.** SETTINGS › AUDIO ROUTING › TUNING: cell A TUNE stays (the whole unit); cell B, the SCALE placeholder the owner reserved, becomes TUNING, the active Part's, bound as SETTINGS › PART › LIVE SNAP's cells are. The focus band says where a `TUNEn` choice reads from and that `MTS` follows MIDI in.
- **The card.** `/CHIMERA/TUNINGS` (`Dir::Tunings`), which the UI only reads. The card is inside the case: the console's `put` takes `/CHIMERA/TUNINGS/TUNE1.SCL` to `TUNE8.KBM` (4 KB at most, the old file replaced; a `.SCL` also deletes its slot's `.KBM`, so no stale map stays applied), and `chimera-usb.py tuning <n> <file.scl> [<file.kbm>]` sends them; each `put` makes the Parts on a `TUNEn` read theirs again. `TUNEn` reads `TUNEn.SCL` and, if present, `TUNEn.KBM`, up to 4 KB each. Fixed names, not a listing: 8.3 names are A–Z and 0–9 anyway, and a project's choice never points at another file when files are added.
- **Applying.** Pulled, after the keys (`UiState::card_work`): each Part whose TUNING differs from the one its table plays is applied, so an edit, a project load and a Sound load land the same way. A file that won't read or parse plays 12-TET and a toast says why (`TUNE3.SCL NOT FOUND`, `NOT SCALA`, `TOO BIG`, the card's own error).
- **Scala** (`chimera_theory::tuning`): a pure, `no_std` reader over fixed arrays, property-tested on any bytes. Steps 1–128, cents (with a `.`) or a positive ratio, the period above the root. A `.kbm`'s map size up to 128, `x` unmapped, its formal octave 0 meaning the scale's. With no `.kbm`, degree 0 is middle C at 12-TET's pitch, so a 12-TET `.scl` is exactly 12-TET. Unmapped keys and keys outside the map's range stay 12-TET; every key stays on the MIDI range 0–127.
- **MTS** (`chimera_midi::sysex::mts`): single-note changes real-time (`7F … 08 02`) and with a bank (`08 07`), the bulk dump (`7E … 08 01`, `08 04`, its checksum checked), scale/octave one-byte and two-byte (`08 08`, `08 09`); device 7F or Chimera's (0). The tuning program, bank and channel mask are not kept: the pipe writes the one MTS table at once, and every Part set to `MTS` plays it. MIDI in's SysEx room grows from 32 to 408 B for the bulk dump.
- **Placement.** Bank 2: `chimera_theory::tuning`, `chimera_engine::tuning`'s writers, `ui::tuning` (behind `UiDoor::tuning` and `tuning_note`), `chimera_midi::sysex` and the pipe (already there). Bank 1 keeps the param's closed-door data, the page table, the inlined read and ease, the `tuning_due` compare and the door stubs. Closed, every Part plays 12-TET and MIDI in is deaf as before.

## Alternatives considered
- **The table in the Sound or `AudioShared`.** 512 B a Part through the triple buffer and every voice's `played` copy; the tables change only on a pick.
- **A listing of the folder as the choice.** Names that move when a file is added, and a dynamic cell format bank 1 lacks.
- **The whole synth one tuning** (the owner's first note). Superseded by his later per-Part call, which matches stock.
- **A MIX › PART sub-page** (stock's place). EDIT on PART opens the Sound rung, so a sub-page can't be reached by the D-pad; a mixer node would be an eighth on the map, which seven fill.
- **Retuning sounding notes.** MTS real-time allows it; held notes keeping their pitch is simpler and matches how a new table is picked. Later if asked.
- **Unmapped keys silent.** Needs a note-on refusal in bank 1; 12-TET is heard and harmless.
- **ORBIT and live snap in the tuning's steps** (#217's text). Not in this step: snap still works in semitones, then the table tunes the result.

## Consequences
- A Part can play just intonation by ear in one turn; Scala archives work once renamed `TUNEn.SCL`.
- A project keeps the choice, not the table: a `TUNEn` file changed on the card changes the project's sound.
- **Bank 1** (exceptions, each line naming this ADR): `default` +868 B, `orbit` +812, `bank2-gate` +820; the bench sets and `fault-inject` +380 to +1,076, without the tables (the `tuning` feature is off there). It is the TUNING param's closed-door data (spec, names, the SETTINGS cell), the note-on read and per-voice ease, the door stubs, the read's sink (a `dyn ReadSink`'s vtable lands in bank 1, so its methods are bank 1's), and MIDI in's larger SysEx room.
- **Placement lessons:** no core float or integer `FromStr` (its float tables cost about 2.5 KB of bank 1): the reader has its own digit loop and works in `f32`; built-in texts, words, file names and `Keymap::LINEAR` are named statics, never promoted constants (bank 1's anonymous rodata); helpers called twice are inlined so the machine outliner shares no call into bank 2.
- **RAM** (AXI `.bss`, with `tuning`): the tables 3,588 B, the card read's buffer 4,104 B (a static, so a door entry's stack stays light); every build with MIDI in: +376 B a pipe (DIN, USB).
- **Stack** (`stack_depth.py`, nested): `orbit` 60,756 B, `default` 60,204 B of 63,232, within a few bytes of `main`.

## Sources
- Scala file formats, Manuel Op de Coul (huygens-fokker.org/scala/scl_format.html, help.htm#mappings).
- MIDI Tuning Standard, MMA CA-020 and CA-021 (midi.org).
- PreenFM2/3 manual, Mixer › Scala.
- ADR 0010, 0042, 0112, 0137, 0198, 0217. Issue #217.
