# 0236. Make the mod wheel and aftertouch matrix sources

- **Status:** Proposed
- **Deciders:** owner (MIDI IO design 2026-10-15, decisions 3 and 4); firmware

## Context
Stock PreenFM3 offers the mod wheel and aftertouch as matrix sources (#301). Chimera only let CC 1 and channel pressure be learned as map inputs: each move was a Sound edit, and neither could reach a route. The matrix had eight sources (`MAX_MOD_SOURCES`), route presence was a `u8`, and the `Routes` record's 640 B bound (ADR 0102) holds exactly eight sources × twenty columns. The note queue packs a kind in bits 18 up, eight kinds used. Bank 1 is frozen (ADR 0198). MPE (#845) will need per-voice pressure.

## Decision
1. **Two rows after NOTE: WHEEL (`ModSource::Wheel`, index 8) and PRESS (`ModSource::Press`, index 9).** Readout names MOD WHEEL and PRESSURE. `MAX_MOD_SOURCES` is 10; presence is `SourceBits` (`u16`). Every engine's matrix shows them; ALGO's twelve rows (with OP 5 and OP 6) sit at 11 px.
2. **Values.** WHEEL is CC 1 on the Part's channel, 0..1. PRESSURE is the higher of the channel's pressure and the voice's own key's poly pressure (one row for both kinds of keyboard, owner decision 3). Poly pressure reaches only voices a player struck (never ORBIT's), through the voice's latest key; a note-on, or a fallback to an older held key, starts it at 0. CC 121 zeroes a channel's wheel and pressure and its voices' poly pressure.
3. **Never steppy.** The wheel and channel pressure glide per Part (`Knobs::ease_expr`), the poly value per voice (`Voice::ease_press`), both at the KNOB coefficient (τ 30 ms), as the pitch bend does; a silent Part lands.
4. **Transport: the note queue.** `InEvent::Expr` (`Wheel`, `Pressure`, `Poly`) packs as a note does (channel, key, value), kinds 8–10 above bit 18. An expression event leaves half the queue free (`EXPR_RESERVE`), so a pressure flood never crowds out a note-on. CC 1 and channel pressure still reach the control ring, so learn and existing map entries work as before.
5. **Disk.** Codes 10 and 11 (8 and 9 are `Jack`'s OP 5 and OP 6), idents `WHEEL` and `PRESS`. Their routes go to a new non-critical record, `PerfRoutes` (`0x0020`), written after `Routes` and only when there is one; `Routes` keeps the first eight sources and its bound. `ModDests`' source count is written as `Routes`' (8) and not read: every Sound decodes with all ten rows. `infer_jacks` reads only the first eight sources' bits. A Sound without the new routes encodes byte for byte as before. `MAX_SOUND_COST` 2,768 → 2,928.
6. **MPE slots in per voice.** A voice-scoped pressure (MPE's channel pressure per member channel) writes the same per-voice value poly pressure does; timbre (CC 74) would be a third per-voice source the same way.

## Alternatives considered
- **Latest-wins atomics per channel for the wheel and channel pressure** (the 2026-10-15 design): nothing dropped, but a new shared static in both shells and bank 1's ISR, and an ordering rule against the queue. The queue already carries the bend, which has the same shape; the reserve answers the flood.
- **One `Routes` record of 800 B.** An older build refuses a record over 640 B and with it the whole Sound; the record buffer and decode staging grow on every stack.
- **WHEEL → LFO 1 DEPTH in INIT, CC 1 out of the default map** (owner decision 4). LFO DEPTH is not a destination yet (LFO blocks are not voice-read), so INIT keeps CC 1 → LFO1 DEPTH in its map; filed as a follow-up.
- **Scroll ALGO's matrix vertically.** A gesture and a chevron for two rows; 11 px rows hold the bold 8 px amounts.

## Consequences
Each voice's chain grows 8 B (`VOICE_CHAIN_BYTES` 2,440 → 2,448); `ModState` grows 60 B (two `amounts` rows, two `fixed` rows, `u16` presence). The matrix sums run ten rows; the new rows cost two stores a voice a block and a glide a Part (and a voice) a block, nothing more until routed. **Bank 1 grows 1.5–2.3 KB a set** (ORBIT +2,168 B to 829,668, about 88 KB free; default +1,872), an exception to ADR 0198 measured here: the voice's render runs ten rows (`render_knobs` +370), the codec writes and reads the second record (`encode_routes` net +336, `set_route_addr` out of line), and `Jack`'s codes no longer equal the sources' indexes (FilterParams' `set_enum_code` +382). The knob glides' and the publisher's route loops are rolled (`black_box` trip count), which took back about 1.7 KB. These are the audio path and the file format a closed door must still play and load, so they stay in bank 1; the lines in `tools/bank1-budget.txt` name this ADR. A wheel or channel pressure the queue refuses is owed, latest wins, and lands once there is room (USB's poll retries it; DIN's handler, pended by the main loop while `MIDI_OWES`, #884); poly pressure, per key, is not kept.

**Placement (#884).** Where a glob can't reach a helper the machine outliner made (an `OUTLINED_FUNCTION` lands in bank 1 and tail-calls into bank 2), the tool is an explicit `link_section` on the bank-2 function it came from: LLVM outlines nothing from a function with an explicit section. `IrqState::work` is the first (`.bank2.text.card_work`).

## Sources
- #514, #301, #845; MIDI IO design `docs/superpowers/specs/2026-10-15-midi-io-design.md` (branch `midi-io`, commit 2ab016a3), § 1.
- ADR 0045 (records), 0102 (the 640 B `Routes` bound), 0128 (bend per Part), 0139 and 0217 (the map), 0198 (bank 1 frozen).
