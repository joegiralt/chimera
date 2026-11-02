# MIDI Phase 2: USB MIDI and an interrupt-driven USB port

- **Date:** 2026-11-02
- **Owner direction:** "you're going to be able to drive this yourself": the scope is Claude's call. The unit is a class-compliant USB MIDI **device** beside the console; USB host is out.
- **Builds on:** `2026-10-25-midi-phase1-design.md` (§ 13 non-goals: "USB MIDI and the interrupt-driven USB ADR: Phase 2"), ADR 0068 (USB console, polled), ADR 0217 (USB is another source with its own pipe), ADR 0175 (an interrupt's work behind `BoardDoor`), ADR 0198 (bank 1 frozen).
- **Research:** `reports/MIDI controller and USB schemes.md` (Phase 2), (private notes) (interrupt-driven USB on this board, 2026-10-25).
- **ADR:** 0222, superseding 0068's "polled, never interrupt-driven".

## 1. Why

A DAW or a USB-only sequencer plugged into the unit's one USB port should play it as DIN does: notes, CCs, learn, echo, Identity. Today the port is the console only, polled from the UI loop. The unit reads `loop_avg_us 641`, `loop_peak_us 40019`, so a polled MIDI note could wait 40 ms. USB MIDI therefore needs the port serviced from an interrupt. The interrupt must never cost the console and `update`, the only way testers install firmware.

## 2. Goals, non-goals, anti-goals

**Goals**
- The unit enumerates as a composite device: the CDC console (unchanged to `chimera-usb.py`) plus a USB MIDI 1.0 function with one cable each way, named "Chimera".
- USB MIDI in does everything DIN in does: notes to Parts by channel, CCs, bindings and learn, NRPN/RPN, takeover, Identity Request.
- USB MIDI out carries what DIN OUT carries: panel echo and the Identity Reply.
- The port is serviced from the `OTG_FS` interrupt. A note's latency is bounded by the USB frame (1 ms) plus the interrupt, not the UI loop.
- The console and `update` keep working when the interrupt misbehaves (§ 6).

**Non-goals**
- Several cables (the report's "cable per Part"). Phase 2 ships one. Bindings name no source (phase 1 spec § 6.1), so adding cables later breaks nothing. Filed as a follow-up.
- USB audio (the spike's UAC1), MIDI 2.0, MIDI-CI, USB host, MIDI thru between DIN and USB.
- macOS and Windows testing. Only Linux is on the bench; the descriptors follow the class spec, and testers report the rest.

**Anti-goals**
- A USB fault that leaves the unit without its console, so `update` can't run (the spike's DFU strand).
- USB MIDI that works with bank 2 closed. Closed, MIDI in is deaf, as DIN's (phase 1 spec § 13), and the console must still work.
- Bank 1 growth beyond the interrupt shim, the lock and the fallback (ADR 0198).

## 3. Approaches

1. **Poll MIDI from the UI loop beside the console.** Least code, no ADR. Up to 40 ms of note jitter on the unit's own reading. Rejected: it fails the goal it exists for.
2. **The `OTG_FS` interrupt services the port; the UI reaches the console under a lock that masks it (chosen).** The spike ran this (CDC + UAC1) and the console survived five `update`s. MIDI is bulk, so none of the spike's isochronous walls apply.
3. **Move to embassy-usb, driven from the interrupt.** A new USB stack under the console and `update` for no gain MIDI needs. Rejected; it is the USB-audio question, not this one.

## 4. The device

- **Descriptors:** `composite_with_iads()` (class EF/02/01). CDC-ACM as today (usbd-serial writes its IAD), then the Audio function: AudioControl interface and MIDIStreaming interface, one embedded and one external jack each way, two bulk endpoints of 64 bytes. **Claude's call:** the `usbd-midi` crate (MIT) if its release for usb-device 0.3 gives these descriptors and raw packet read/write; otherwise an in-tree class of the same shape. Either way the jacks are named "Chimera".
- **Identity:** VID:PID stays 0483:5740 (memory: software on PreenFM3). `bcdDevice` goes up, because Windows caches drivers per VID/PID/bcdDevice and the interface layout changes. The product string becomes "Chimera" (was "Chimera console"). The `chimera-usb.py` port lookup must still find it: check how it matches before changing the string.
- **Bank 2 closed:** the MIDI class isn't passed to `poll`, so the configuration the host reads is the console alone. The open-or-closed state is fixed before `connect`, so a host never sees the layout change under it.

## 5. The interrupt

- **Who owns the device.** `UsbDevice`, the `SerialPort` and the MIDI class move into one static cell. The `OTG_FS` handler polls it. The UI loop's `service` and every console path that polls (`UsbOut::pump`, `flush`, `read_byte`, the DFU drain) reach it through `with(|core| …)`, which masks `OTG_FS` in the NVIC for the closure. This is the spike's `usb_audio::with`. Under the mask the UI polls the device itself, so a long answer (`put`, `bank2 install`) still moves its bytes; USB MIDI waits until the answer ends.
- **One poll.** `poll_all` is the only poll: open, it polls the console and the MIDI class through the board's door; closed, the console alone in bank 1. The handler and the UI's pumps both call it.
- **Priority:** `Priority::MIDI`, as USART1. Audio stays above it.
- **The handler's work, after the poll:** read every OUT packet the MIDI endpoint holds, turn each into bytes (§ 7), and feed them through USB's pipe in one door call per packet. Then pack USB's out ring into IN packets while the endpoint takes them. Its own cycles go to `USB_CYCLES`, timed as USART1 times `MIDI_CYCLES` (the audio's share subtracted).
- **Bank placement (ADR 0175's pattern):** bank 1 keeps the `OTG_FS` shim, the cell, `with`, the guard (§ 6) and the closed path. The MIDI class, the packet codec and the handler's work after the poll are bank 2's, in `BoardDoor`. `door-check` must pass, including no vtable for a bank-2 type in bank 1's rodata: the dyn coercion for the MIDI class happens inside bank 2's poll.
- **Enumeration timing** is unchanged: `connect` runs at the UI loop's top, and the handler is unmasked only after it.

## 6. The fallback: the console survives

- **Storm guard.** The handler counts its entries per millisecond (`controls::now_ms`). Over `STORM_LIMIT` in one millisecond, it masks `OTG_FS` and counts a trip. The UI loop's next `service` polls under its lock as today, then unmasks. A storm costs at most `STORM_LIMIT` entries per UI lap, and the console runs at the UI loop's pace, as in 0.2.
- **The known storm:** a full console buffer. When the host writes console bytes faster than the UI reads them, the CDC packet stays in the shared RX FIFO and RXFLVL stays set. The guard catches it. Nothing is vendored: synopsys-usb-otg stays at 0.4.0. Known limit: while a CDC packet waits at the FIFO's head, MIDI packets behind it wait too, up to one UI lap. That only happens while the console is mid-transfer.
- **`STORM_LIMIT`:** above a full-speed bulk flood (19 packets a frame at most). **Claude's call:** 64.
- **No other mode.** There is no setting or boot flag to turn the interrupt off. The guard is the fallback, and it is tested (§ 10).
- **`stats`** gains `usb_irq_pct`, `usb_irq_us_max`, `usb_storms`, `usb_midi_drops`.

## 7. USB MIDI in and out (functional core, `chimera-midi`)

- **In:** a 4-byte USB-MIDI event packet becomes 0–3 MIDI bytes by its Code Index Number (USB MIDI 1.0 § 4, table 4-1). SysEx spans packets (CIN 4 continues; 5, 6 and 7 end with 1, 2 or 3 bytes). Packets for a cable other than 0, and CIN 0 and 1 (reserved), are dropped and counted. The bytes go through a second `PipeSlot` (USB's), so running status, SysEx framing, 14-bit pairs and NRPN state are USB's own (ADR 0019, 0217).
- **Out:** bytes from USB's out ring become packets, with a small state that splits SysEx into CIN 4 and 5–7 and passes realtime bytes as CIN F. The UI's out pass pushes the same echo bytes to DIN's ring and USB's.
- **Identity:** the Request is a `CtrlEvent` on the ring of the port it came in on, and the Reply goes out that port only.
- **Detach:** a USB bus reset, a suspend or an unplug lets go of every Part's notes on USB's pipe, as Active Sensing's timeout does (`NotesOff` on all 16 channels and `SensingLost`), through a new door entry. A host that vanishes mid-chord leaves no stuck notes.
- **Pure and tested on the host:** the CIN table both ways, SysEx split and join across packets, the cable filter, and a round trip of every message type through `packet → bytes → packet`.

## 8. Wiring

- `NOTE_SOURCES` gains `USB` (2, or 3 with `bench`); `MAX_NOTE_SOURCES` follows. `CTRL_USB` and `OUT_USB` rings sit beside DIN's in `shared.rs`, built the same way. The UI drains `CTRL_USB` as it drains DIN's, through the same code.
- **Features:** `midi-usb` beside `midi-din`, on wherever `midi-din` is. A build without it keeps today's polled console byte for byte.
- **Desktop:** no change; it already takes MIDI from the OS. The codec and ring wiring are host-tested.

## 9. Bank 1

- Expected growth: the shim, the cell, `with` replacing direct calls, the guard, the second source's statics and the `stats` lines. The spike's equivalent, without its UAC class, was well under its +7.4 KB. ADR 0222 measures each set and raises its line in `tools/bank1-budget.txt`, as ADR 0217 did for DIN OUT.
- If a set's growth passes 2 KB, move more behind the door before raising.

## 10. Verification

**Host:** the codec tests (§ 7); `just check` green, including `check-bank1`, `door-check` and the stack check (the handler's door call is charged its known target, as `DMA2_STR0`'s).

**Unit, run by Claude over the console** (SAVED checked first; flash only at 0x08020000):
1. `lsusb -v` shows the composite: CDC plus Audio/MIDIStreaming; `amidi -l` lists "Chimera".
2. A note from `amidi` over USB plays on the MOTU's inputs 5–6 at the level a DIN note does.
3. A CC learn over USB (absolute and relative), the panel echo arriving on USB, the Identity Reply on USB and not on DIN, and the reverse for a DIN request.
4. Latency and jitter: the same note sent 200 times over USB while the screen redraws. Onset spread under 2 ms, against DIN's.
5. A dense stream over USB (5,000 B): no overruns, no drops, `usb_irq_pct` read.
6. Unplugging mid-chord releases it.
7. **The fallback:** `put` a 1 MB file while a USB note stream plays. The console finishes, `usb_storms` counts trips, MIDI resumes. Then `update` the unit twice in a row.
8. **Bank 2 closed** (the ORBIT .bin over main's bank 2, as phase 1's test): the host sees the console alone, `update` restores.

## 11. Left to the builder
- `usbd-midi` against an in-tree class (§ 4), by what fits usb-device 0.3.2 and bank 2's door.
- The door entries' exact shape in `BoardDoor`.
- Whether `with` sits in `usb.rs` or its own module.
- The `chimera-usb.py` match on the new product string.

## 12. Follow-ups to file
- Cables per Part (cable 0–5 → Parts 1–6, cable 6 global), the report's layout.
- macOS and Windows enumeration, by a tester.
- USB audio (Phase 5): the spike's walls, on top of this interrupt.

## 13. Rulings from the builder check (override §§ 4–10 where they differ)

Full questions: (private notes). All Claude's calls.

1. **MIDI OUT is always emptied.** Under `with`, the UI's pumps call the same `poll_all` plus the handler's after-poll work (read, feed, pack), so MIDI is fed at the pump's pace during an answer and never blocks the shared RX FIFO. With the door closed and MIDI configured, bank 1 drains MIDI's OUT endpoint by its address (`UsbBus::read`) and drops the packets, counted in `usb_midi_drops`. A few dozen bytes of bank 1.
2. **Open or closed:** MIDI is configured only if the door was open at `connect` (a boot snapshot). `poll_all` uses the door only while that snapshot is set **and** the door is open now. Otherwise it takes the bank-1 path: console poll, plus the drain from 1. A door that opens later (`bank2 install`) brings MIDI at the next boot; a gate build's `bank2 erase` leaves MIDI configured but deaf, never calling unmapped code. §10 gains: `bank2 erase` on a gate build while `amidi` streams, and the console still answers.
3. **IN starts from the UI:** after its out pass the UI pends `OTG_FS`, as `din_out` pends USART1. No SOF interrupt.
4. **Echo per port:** one out pass writes each message to both rings. DIN's ring keeps today's hold-back. USB's copy is dropped when its ring is full, or skipped when the device isn't Configured, counted in `usb_midi_drops`; it never holds DIN back. `hear_ccs` drains both control rings in one pass with one budget, tagging each event with its port. Identity is a flag per port, and the Reply goes out the port that asked.
5. **Storm guard on DWT:** entries are counted per 1 ms of DWT cycles, not `now_ms` (2 ms steps, and stalled during a storm). `STORM_LIMIT` is measured, not guessed. Provisionally 200, then set to 4× the worst entries per millisecond read on the unit during test 5 plus a console `get` of 1 MB. **The plan's first hardware task is the interrupt with the console alone:** `update` twice, `put` and `get` 1 MB, and the stimulus from 6, before any MIDI code. If that fails, the RXFLVL-mask fix from the spike's vendored synopsys-usb-otg comes in, as its own task.
6. **Test 7's storm stimulus:** 8 KB written to the console's tty with no newline, so the UI can't drain it in one lap. Pass: `usb_storms` > 0, then the console answers `status`, and MIDI resumes. A `put` under a note stream separately must finish (storms may stay 0).
7. **The device poll runs in bank 2 while open:** one door entry polls console and MIDI, monomorphised in bank 2, so no bank-2 vtable sits in bank 1. A sealed bank 2 is trusted, and the closed path is bank 1's own. The MIDI class's endpoints are allocated at `init` through a door entry into a bank-1 `MaybeUninit` slot (`PipeSlot`'s pattern). Closed at boot, nothing is allocated.
8. **Suspend releases notes**, as a bus reset does: a sleeping host leaves nothing stuck. `SensingLost` stays shared; no new UI.
9. **The card interrupt's own-time stamp subtracts `USB_CYCLES`** too, as it does MIDI's. In scope.
10. **`MAX_NOTE_SOURCES` goes to 3**, with `midi-usb` on in the bench sets that have `midi-din`. Measured into ADR 0222 with the rest.
