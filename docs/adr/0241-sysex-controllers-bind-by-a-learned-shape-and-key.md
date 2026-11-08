# 0241. SysEx controllers bind by a learned shape and key

- **Status:** Proposed (amends 0198: bank 1 grows by the binding type and stubs, measured below)
- **Deciders:** owner (0.3 scope, #846: controllers that edit synths by SysEx drive Chimera's params, learnable like CCs), firmware

## Context
Hardware editors and programmers for other synths (Roland PG and Juno programmers, DX7 and Korg editors, generic boxes) send one SysEx message a parameter: fixed header and address bytes, the value in one byte (or an MSB/LSB pair), often a Roland-style checksum. ADR 0217's bindings hear CC, 14-bit CC, NRPN and pressure only, and the pipe dropped every SysEx but the identity request. `Binding` is a bank-1 type, at most 12 B (its assert), 16 a Sound.

## Decision
- **Learn** (`chimera_midi::sxmap::learn`, pure): two bodies of one control at two values, same length. If their last bytes differ and checksum both (from some byte on, the sum is a multiple of 128), the last is a checksum. The one byte that differs before it is the value. Two bytes side by side are never learned as a pair from two messages: an address beside a value (a DX7's `pp vv`, two knobs moved in turn) looks the same, and a pair's key would hear every address. Anything else is not one control's, and learn keeps the newer message to compare with the next.
- **A pair from a carry** (`sxmap::carry`): learn senses on after mapping. When the byte before the value moves by one while the value byte wraps (127 to 0, or back), the 14-bit value stepping at most 16, every other byte kept, that is one carry; the entry becomes the MSB/LSB pair only on a second, the same way on (MSB m to m+1, then m+1 to m+2), each by the same wrap rule. A slow 14-bit sweep shows many carries; two neighbouring knobs (a DT1's 1A at 120, then 1B touched at 2) show at most one in a row, so they stay single bytes. Residual risk: three knobs at consecutive addresses touched in turn, each at the wrap by a small step, in one sensing window.
- **The binding.** `Input::SysEx(SxKey)`: a `Shape` byte (value at, pair, checksum) and a 32-bit FNV-1a key of the body's length and bytes, the value and checksum bytes zeroed. 5 B, so `Input` is 6 B and `Binding` 12 B (the assert's bound, was 10). A later body matches when its own key under the shape is the same; its value is 14 bits (a lone byte spread `v << 7 | v`, as a 7-bit CC).
- **Applied** as an absolute `Jump` through the binding's ramp (ADR 0139's glide, never a snap), on every Part whose Sound holds the binding, whatever its channel. No echo: the key can't rebuild the message. MIDI MAP shows `SYSEX n` (the value's byte, F0 first); its control is fixed, E has one kind, F stays OFF.
- **Learn UI.** MIX+SEQ arms as for a CC; the first SysEx is kept (`MidiUi`, AXI, forgotten whenever learn is off), a second that diffs maps the focused cell, and learn senses on for carries until MIX+SEQ, a nav key or a turn ends it. While it senses, the band reads `SYSEX → <cell>` and `ABSOLUTE`, sure at once, so `MIX + SEQ  DONE` shows as for a CC. A cell no control may drive takes nothing: NOT MAPPABLE stands, and bindings hear the message. A full map says `MAP FULL`.
- **Transport.** The pipe hands a manufacturer's body (not 7E/7F, at most 32 B) to `Sinks::sysex`: a `CtrlEvent::SysEx` marker on the control ring, then the body into the port's `SYSEX_IN` ring (four slots of 36 B), which the shell passes in its sinks, `None` without `midi-map`, so nothing waits that nobody drains. The UI drains the port's ring on each marker. `SxRing` is bank 2's (`bank2.x`).
- **Codec.** `Bindings` version 2, written only when the map holds a SysEx entry: 12 B an entry, version 1's 8 then the key, LE; a SysEx entry `[4 << 5 | JUMP] [shape] [0]`. Version 1 files load as before; a SysEx tag in a version 1 record is skipped and counted.
- **Placement.** `sxmap` bank 2 (`bank2.x`); the matching and learn are `ui::midi`'s, the codec `midi_map::codec`'s (both bank 2). `SxRing`'s push and pop are bank 2's too (`bank2.x`); its statics are AXI `.bss`. Bank 1 carries the type growth, the marker's pack arm and the shells' ring pointer in `DinSinks`.

## Alternatives considered
- **Storing the whole message.** 32 B a binding, 16 a Sound, in every Sound copy: about 20 times the key. A per-project pattern table instead breaks a Sound's map travelling alone (library, pool).
- **A 12- or 16-bit key, to keep `Binding` at 10 B.** A box with a few hundred parameters, a dozen bound, would collide somewhere (per pair 1/4,096 or 1/65,536).
- **Matching in the pipe.** The pipe doesn't hold the maps; the UI does.
- **Nibble pairs, other checksums, echo by template.** Later, from a profile per box if a tester has one.

## Consequences
- Old SysEx boxes move Chimera's cells; a box's message for one parameter can't move another (but for a key collision, about 1 in 4 billion a pair).
- `Binding` is 12 B: every Sound's map grows 32 B in RAM.
- Maps with a SysEx entry don't load on older firmware (version 2 is skipped and counted, as any unknown version).

## Sources
- Roland DT1 / checksum (Roland MIDI implementation charts); Yamaha DX7 parameter change format; FNV-1a (Fowler, Noll, Vo).
- ADR 0139, 0198, 0217. Issue #846.
