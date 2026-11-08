# 0243. Give each Part a USB MIDI cable into Chimera, cable 0 staying "Chimera"

- **Status:** Proposed
- **Deciders:** Claude (owner's standing 0.3 instruction: decide UX by precedent)

## Context
ADR 0222 shipped one USB MIDI cable, "Chimera", carrying every channel. #840 asks for a cable per Part, so that a DAW sees one port per Part and each port plays its Part whatever channel the track sends on. The research report (`reports/MIDI controller and USB schemes.md`) proposed cables 0–5 for Parts 1–6 and cable 6 for global traffic.

usb-device 0.3.2 writes the configuration descriptor into its control buffer. At most that buffer is 256 B (`control-buffer-256`), and a configuration past it stalls GET_DESCRIPTOR, so the host gives the port up (a 175-B configuration once passed the 128-B buffer and stranded the unit). The one-cable configuration is 175 B. A full cable each way costs 32 B with external jacks, or 15 B with embedded jacks only, so six Part cables would need 192 B or 90 B, against 81 B of room.

## Decision
- **Cable 0 stays "Chimera"**, both ways, every channel, as in 0222. Existing DAW setups and controllers keep working, and SysEx, clock, Identity and the value echo stay on it.
- **Cables 1–6 are "Chimera Part 1" to "Chimera Part 6", into Chimera only.** Each is one embedded MIDI IN jack, named by its own string, plus one byte in the bulk OUT endpoint's jack list: 7 B each, 42 B in all. The configuration is 217 B, which leaves 39 B under the 256-B buffer. A host test enumerates it and checks the length against the buffer with a 32-B margin, the jacks against both endpoints' lists, and every jack's name.
- **Routing.** A channel message on cable n is moved to Part n's channel and goes through USB's one pipe, so it plays Part n and any Part sharing that channel. A key's note-off and pressure, and the pedal's up, are the exception: they go on the channel the key's note-on or the pedal's down went on (`usb::PartCables`, in the poll's port: a nibble and a held bit per key per cable, plus a byte per cable for the pedal, 486 B). A key the cable doesn't hold, or a pedal it hasn't put down, goes where the Part is now. The audio side releases a voice by the channel its note-on came on, so a Part whose channel moves while its cable holds a key or the pedal still lets go. Anything else on a Part cable (SysEx, system common, real time) is dropped and counted in `usb_midi_drops`' IN. The Parts' channels reach USB's poll through `midi_in::PART_CHANNELS`, which the UI stores each pass (`hear_in`, bank 2). Until the first pass, Part n sits on channel n.
- **One pipe for every cable, one parser for cable 0.** A Part cable's message is whole (USB-MIDI packets carry their own status), so it goes to the pipe's controllers and sinks past the parser (`UsbSlot::feed_msg`). Cable 0's SysEx or running status around it is never cut. A 14-bit pair or NRPN sent on a Part cable and on cable 0 to the same channel at once would share that channel's controller state, as two DIN controllers merged on one channel do.
- **Out stays cable 0's.** The echo goes on the Part's channel, on cable 0, as in 0222.
- `bcdDevice` goes from 0x0020 to 0x0021, because Windows caches its driver binding by VID, PID and bcdDevice. The jack names are reached from the bank-1 `UsbClass` shim through a pointer that bank 2's `alloc` stores (`jack_name`), like the descriptor writer.

## Alternatives considered
- **The report's layout, Parts on cables 0–5 and global on cable 6.** It moves every existing setup's port, so it was rejected.
- **A cable each way per Part.** It does not fit the 256-B buffer: 90 B even with embedded jacks only. A larger buffer means patching usb-device or moving stacks, which is out of scope.
- **A pipe per cable.** It would cost about 0.5 KB of RAM per cable for state that only clashes in the merged case above.
- **Tagging events with a Part instead of moving the channel.** It would change `InEvent` and the audio side's routing, for no gain over the move.

## Consequences
- A DAW sees seven ports into Chimera and one out of it. Per-Part echo on a Part's own port is ruled out until the descriptor budget grows.
- A Part's channel moved while its cable holds keys or the pedal: they release on the old channel, and new notes play on the new one. A note-off for a key the cable doesn't hold goes to the Part's channel now.
- Builds without the MIDI map never store the channels, so their Part cables play on the defaults. Those are bench builds only.
- The descriptor budget is a build-time check, per layout. `usb_class::config_len` must stay `MARGIN` (32 B) under `CONTROL_BUF` for the default build (217 B), and inside it for the `usb-audio` build. That build keeps one MIDI cable (`cable_count(true)`) beside the stream, 249 B. With the Part cables it would be 291 B, so it has none, and its Part routing never sees a Part cable. The host test enumerates both layouts and checks each length, jack list and jack name.
- Enumeration on macOS and Windows, and the port names a DAW shows, need the unit (the PR's list).

## Sources
#840; ADR 0222; `chimera-midi/src/usb_class.rs` and `chimera-midi/tests/usb_class_test.rs`; usb-device 0.3.2 `control_pipe.rs` (`CONTROL_BUF_LEN`); USB Device Class Definition for MIDI Devices 1.0, § 6 and appendix B; `reports/MIDI controller and USB schemes.md`.
