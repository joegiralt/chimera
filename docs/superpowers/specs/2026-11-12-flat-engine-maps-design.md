# Flat engine maps (0.4): design

Status: owner-approved model, 2026-11-12 ("makes sense"). Supersedes ADR 0226's sections and ADR 0264's SUGGEST once its ADR is accepted.

## Owner's model
- A controller map is "just a bunch of mini learn stuff": one flat list of up to 90 rows (control → cell, with the row's MIN, MAX, INPUT, ECHO), no shared/engine sections.
- The factory set ships one map per engine, named after it: ALGO, MODAL, VA, TAPE (DRUM with the drum engine). "Here's how we did it, you can do it any way you like." Each as rich as 90 rows allow: both ENV TYPEs' cells (A: ATK DEC SUS REL; B: RISE FALL SHAPE TIME) for ENV 1–2, LFOs, filter, FX slot, voice, and the engine's own cells by usefulness. Built-in GM CCs stay as they are (ADR 0263), and factory maps use CCs MIDI leaves free.
- A Part loads a map only when it picks one; NONE holds nothing extra. A map stays with the Part across Sound loads.
- Any map plays on any engine; rows for cells the Part's engine lacks dim and do nothing ("that's on you"). A user map ("Joe's super awesome TB303") works the same.
- The Sound's own learned rows stay 16 per Part and win over the map's on the same control.

## Constraints found (survey 2026-11-12, read-only, at 1b50b1ca)
- D2 is full (~8 B; ADR 0260, 0263). AXI ~4.8 KB above its 32 KiB margin. SRAM4 ~700 B. DTCM is the stack.
- **ITCM: 64 KB at 0x00000000, unused**, not in memory.x; CPU and both banks reach it at zero wait (MPU PRIVDEFENA, cache.rs:11); DMA cannot (map rows need none). Needs a MEMORY entry and a NOLOAD section in chimera-stm32/build.rs, starting above 0; a unit check that it holds data; a bootloader 1.09 sanity check.
- Today: `Layers` (ui/midi/layer.rs:58–85) in `MidiSlot`, `.ram_d2`: six 16-row layers + a 5-section `scratch` (~2.2 KB) — moving rows to ITCM frees that D2.
- Per-row runtime state (~55 B a row a Part: ramps, pickup `TakeState`, pending turns, echo) is per Part and cannot be shared; `SLOTS <= 64` (handed: u64, ui/midi/mod.rs:74) and `BINDS <= 32` (owed: u32, echo.rs:57).
- Card path: map read buffer 702 B on the stack (ctl.rs:185) and `read_small` refuses longer files; `codec::decode` refuses > MAP_LEN entries; `RecordBuf` max 640 B (storage/record.rs:10). A 90-row file is ~760 B (8-B rows) / ~1.12 KB (SysEx rows). Console `put` already allows 4 KiB.
- MIDI MAP page: row index u8 (nav.rs:135, 756) fits 1 + 90 + 16; loops over BINDS/SLOTS in focus_note, hand_in, cc_turns, cc_step, cc_sysex, echo.
- Code: `midi_map::ctl`, `codec`, all `ui::midi` are bank 2 (build.rs:238–239, 303); bank 2 ~400 B free → net growth to bank 1 under ADR 0251 (owner approved bank 1 to ~20 KB free, 2026-11-12).

## Decisions (Claude's calls, owner may override)
- **Rows:** six fixed per-Part map slots of 90 rows in ITCM (~6.5 KB with 12-B `Binding`), static, never growing. Dedup (Parts sharing a loaded copy by name) is optional: it does not lower the worst case; take it only if it keeps code smaller (one card read, edits seen by every Part on the map).
- **Runtime state:** a pool of 16 seats per Part keyed by row, as canonical NRPN slots are seated (`PartMidi::seat`, mod.rs:197): a heard row takes a resting seat; an evicted PICKUP row waits for pickup again. `BINDS`, `SLOTS` and their asserts stay; no RAM growth.
- **Echo:** seated rows plus the bursts on Sound/map/Part changes; no per-row `last` for all 90 (saves ~1.1 KB).
- **Lookup:** linear scan (~106 compares a message, main loop); no CC index.
- **File:** one flat map record format, rows spanning records or written straight to the file buffer; read straight into the Part's slot or one static scratch, never a bigger stack buffer (ADR 0025 stack check). Old sectioned maps (ADR 0226) load safely: flattened (shared rows + every engine's rows, first 90) or refused with a clear word; SUGGEST's file is not shipped any more.
- **Factory maps:** `just suggest-map` becomes the engine-map generator from the param tables; the Cirklon .cki files (private, (private notes)) are regenerated from them.

## Next
ADR (superseding 0226's sections and 0264), then a plan; runs as its own 0.4 slice after DRUM slice 1 (shared build slots).
