# USB audio follow-ups (#885): design

Status: step 1 (this note, ADR 0260 draft, the pure halves). Base: main ae687093.
Rulings it builds on: ADR 0222 (USB from its interrupt), 0243 (Part cables), 0248 (UAC1 capture), 0256 (audio ships, one cable), 0257 (72-frame packets), 0251 (bank 1 once bank 2 is full).

## 1. What #885 asks

- (a) Host → device audio: a UAC1 OUT stream, asynchronous, with explicit feedback.
- (b) A six-channel alt setting carrying all three DAC pairs.
- (c) OTG DMA to cut the FIFO writes in the USB interrupt.
- (d) A control buffer past usb-device 0.3.2's 256 B if the layout must grow (249 B today, 7 B left).

## 2. The control buffer (d), settled first

(a) + (b) is 375 B. No usb-device 0.3.2 feature goes past 256 B (`control-buffer-256` is the largest). The ways out:

| Way | Cost | Verdict |
|---|---|---|
| Fork or vendor usb-device with a 512-B `CONTROL_BUF_LEN` | carry a crate (`[patch]`), +256 B RAM in the cell, every rebase rereads it | rejected |
| Reshape the layout under 256 B (7-byte endpoints, no AudioControl terminals, drop the CDC IAD) | non-compliant descriptors; Windows refuses them (ADR 0248) | rejected |
| **The class answers GET_DESCRIPTOR(configuration) itself** from a `&'static [u8]` | a 512-B static (AXI), a builder (`chimera_midi::usb_config`), the `control_in` shim in bank 1 | **chosen** |

usb-device 0.3.2 offers every control IN to the classes before its own handling (`device.rs` `control_in`: `for cls in classes ... if !self.control.waiting_for_response() { return }`), and `ControlIn::accept_with_static` sends a static slice of any length, a packet at a time, cut to wLength (`control_pipe.rs` `accept_in_static`, `start_in_transfer`). The control buffer is never touched. So the class builds the whole configuration once at allocation into a 512-B static, freezes it as `&'static [u8]`, and answers the standard device GET_DESCRIPTOR(configuration, 0) with it. The writer can't be usb-device's (`DescriptorWriter::new` is `pub(crate)`), so `usb_config::write` writes the bytes itself, the console's CDC block as usbd-serial 0.2 writes it.

The guard against a second stranding: the host test holds `usb_config::write`'s bytes for today's shipping (249 B) and bench (217 B) layouts equal, byte for byte, to what usb-device serves through its own `poll` (`static_bytes_match_what_usb_device_serves`). So step 2 can move the shipping layout onto the static path with no descriptor change at all, and flash that alone. A const assert holds the largest layout (every Part cable beside both streams and six channels, 417 B) inside the static's 512 B; the host test enumerates the 375-B and 417-B layouts through usb-device's poll (wLength 9 and 1023).

This also lifts ADR 0256's limit: the Part cables (ADR 0243, #840) fit beside audio (417 B). That is a separate decision (a DAW's port list), not taken here.

## 3. The byte budget

| Block | Bytes | Today (249) | Full (375) | Full + Part cables (417) |
|---|---|---|---|---|
| Configuration | 9 | 9 | 9 | 9 |
| CDC (usbd-serial, IAD included) | 66 | 66 | 66 | 66 |
| Audio IAD | 8 | 8 | 8 | 8 |
| AudioControl interface | 9 | 9 | 9 | 9 |
| AC header (8 + a byte a stream interface) | 10 / 11 | 10 | 11 | 11 |
| Capture terminals IT 1 (12) + OT 2 (9) | 21 | 21 | 21 | 21 |
| Playback terminals IT 3 (12) + OT 4 (9) | 21 | — | 21 | 21 |
| Capture: alt 0, alt 1, AS general, format, endpoint 9 + 7 | 52 | 52 | 52 | 52 |
| Capture alt 2 (six channels): interface, AS general, format, endpoint 9 + 7 | 43 | — | 43 | 43 |
| Playback: alt 0, alt 1, AS general, format, OUT 9 + 7, feedback 9 | 61 | — | 61 | 61 |
| MIDIStreaming interface | 9 | 9 | 9 | 9 |
| MIDIStreaming class block (`ms_total`) | 65 / 107 | 65 | 65 | 107 |

(a) alone is +83 B (332 B); (b) alone +43 B (292 B); both 375 B. Every one is past 256: none can ship on usb-device's own path.

Interfaces: 0–1 console, 2 AudioControl, 3 capture, 4 playback, 5 MIDIStreaming (MIDI moves from 4 to 5: a new layout, so `bcdDevice` 0x0040 for Windows' driver cache, ADR 0256's reason).

## 4. The streams

### 4.1 Playback (a): where it lands

Precedent: Elektron's class-compliant boxes (Digitakt II, Digitone II) sum USB in to the main out at a "USB TO MAIN" level and can sample it; Roland's boutiques and most class-compliant interfaces keep "loopback" off, so the host never records its own playback.

Chimera: the host's stereo is summed into DAC pair 1 after the master bus, at a SETTINGS level `USB IN` (0–100, default 100 = unity, lerped, never snapped), and the capture keeps tapping the master bus before that sum (no loopback; the host records the synth, as ADR 0248 says, and monitors itself in the DAW). A stream starting or running dry fades a block in or out (CLAUDE.md: never snap). Sampling the host into TAPE is a later TAPE ticket (a source TAPE can record), not part of this.

### 4.2 Playback: the feedback

An asynchronous sink (USB 2.0 § 5.12.4.2): the SAI's clock (HSE PLL) is the master, so the host sends what the device asks for. The ask is a PI servo on the OUT ring's smoothed fill (`uac_out::Feedback`), run at each SOF while the stream is on:

- the fill smoothed by one pole at 1/32 (it saws by a 64-frame block each render);
- trim = error × 1/256 frame a frame + its sum / 256, critically damped (time constant about 0.5 s), clamped to ±1 frame (10.14: ±16,384), so the integral settles drift and a steady loss at the target;
- 10.14 in 3 bytes (full speed), `bRefresh` 2 (a value every 4 frames).

A rate counter (SAI frames between SOFs) was the other choice. Its 64-frame block granularity needs the DMA's NDTR read at SOF and a long window; the fill servo needs nothing but the ring and is what TinyUSB's FIFO-count mode does.

`OUT_FRAMES` = 52 (wMaxPacketSize 312 B): Linux caps its packets and the feedback it takes at wMaxPacketSize's frames, so 52 leaves the servo's 49 room. The OUT ring is 512 frames, `TARGET` 192 (4 ms): at 128 the model ran dry once a minute with 0.1 % of packets lost. The audio interrupt's `Drain` plays silence until the ring holds `TARGET`, then takes a block a render; a ring short of a block plays silence and primes again (`short`). A packet with no room is dropped whole (`overruns`).

### 4.3 Capture's six channels (b)

Alt 2 of the capture interface: 6 channels, 24-bit, the three DAC pairs in order. The input terminal declares 6 channels with wChannelConfig 0x0003: channels 1–2 are L/R (predefined), 3–6 non-predefined (UAC1 § 3.7.2.3), so alt 1 carries the first two of the same cluster. Same interface, so the host runs stereo or six, never both.

Full speed's periodic budget is 90 % of 1,500 B a frame, 9 B a transaction (USB 2.0 § 5.6.4). Six channels at 72 frames would be 1,296 B, past 1,023 (full speed's isochronous limit) and past the budget beside the playback. So alt 2's room is `SIX_FRAMES` = 52 (936 B): 936 + 312 + 3 + 27 = 1,278 B with the playback and its feedback on. A packet owed after a missed frame catches up 4 frames a packet (stereo: 24), so the six-channel pacer needs its own model run (step 4).

RAM is (b)'s real cost: a 512-frame ring of six words is 12 KB, +8 KB over today's stereo ring. See § 6.

## 5. Host compatibility

| Host | Playback with feedback | Six-channel alt |
|---|---|---|
| Linux snd-usb-audio | Read in mainline (2026-11) `sound/usb/endpoint.c` and `pcm.c`: the data endpoint's bSynchAddress must name the feedback IN and the feedback's own must be 0 (`snd_usb_audioformat_set_sync_ep`, "invalid sync pipe"); usage 0x10 is explicit (only 0x20 is implicit); a 3-byte value is masked to 24 bits and its shift auto-detected (10.14 → 16.16), and it is taken only in [nominal − 1/8, freqmax], freqmax capped by wMaxPacketSize; `syncinterval` = bRefresh; packets from a 16.16 phase (`synced_next_packet_size`). The model test runs exactly that host. | format's bNrChannels; the terminal's wChannelConfig only feeds the channel map |
| macOS (AppleUSBAudio) | UAC1 asynchronous OUT with explicit feedback is the common class-compliant interface shape; 3-byte 10.14 at full speed. Unverified here | unverified |
| Windows usbaudio.sys (UAC1) | supports asynchronous OUT with a feedback endpoint; caches the driver by VID/PID/bcdDevice (0x0040). Unverified here | risk: a stereo alt under a 6-channel terminal. Fallback: the six channels as their own interface with their own terminals (+31 B, 406 B, still in the static) |

Lessons kept from ADR 0257: never size a packet past what Linux gives it (the OUT direction is the host's sizing, capped by our 52; the six-channel capture by its 936 B); a missed frame is owed, not sent at once.

The feedback endpoint is isochronous IN, `bInterval` 1, read every 4 frames: an armed value waits in its parity, and the incomplete-IN interrupt re-aims it each frame the host passes it by (ADR 0248's `aim`), about one extra entry a frame while streaming (SOF already enters each frame).

## 6. Costs

Budgets (coordinator, 2026-11-10, after #934/#936): bank 1 ~65 KB free on ORBIT; bank 2 ~352 B (ORBIT) — nothing new goes there; D2 ~8 B (bench-algo) — none; AXI: ORBIT 37,576 B from `__sheap` to AXI's end against the 32-KiB margin the link asserts (ADR 0087), so ~4.8 KB spendable.

| | Flash (bank 1, ADR 0251) | AXI | CPU (480 MHz) |
|---|---|---|---|
| (d) static configuration | builder ~0.6 KB, `control_in` shim | 512 B | once at boot |
| (a) playback | servo, drain, OUT pump, arm feedback, sum and level ~1.5 KB; SETTINGS row ~0.3 KB | ring 4,096 + state ~64 B | OUT FIFO read ~74 words, unpack and push 49 frames: ~1–2 µs a ms (~0.2 %); the render's pop and sum ~0.3 % |
| (b) six channels | pack and pump ~0.4 KB | +8 KB (a 12-KB ring of six words for the 4-KB stereo one) | ~216 FIFO words a ms instead of 108: ~+0.2 % |
| (c) OTG DMA | a fork of synopsys-usb-otg (DMA mode is the core's, every endpoint and EP0 alike) | descriptors | saves the FIFO words: under 0.5 % |

(a) and (d) fit (~4.6 KB of ~4.8 KB). (b) does not fit beside them: it needs a ruling (lower ADR 0087's shipping margin to 24 KiB, or a smaller six-channel ring with its own `TARGET`/`HIGH`, model-tested). Bench sets: no change (they build without `usb-audio`).

(c) is not worth a fork of the driver that carries the console and `update`: its win is under half a percent of the CPU, read as `usb_irq_pct`. It is taken up only if the unit reads `usb_irq_pct` past 3 % with both streams on.

## 7. Risk

- Static path: a byte wrong in the static is a stranded unit. Held by the parity test, the parse tests, and step 2 flashing the shipping bytes alone on the new path before any new layout.
- The feedback: a host that ignores or misreads it drifts until the ring runs dry or overflows (counted); Linux's acceptance window is modelled, macOS and Windows are testers' (#841).
- Windows and the six-channel cluster (§ 5).
- AXI for (b) (§ 6).
- The feedback endpoint's parity (§ 5): unit-only.
- Loopback: the capture taps before the USB sum by design; a DAW monitoring its input through to the output still hears itself, which is the DAW's setting.

## 8. Order of work

| Step | What | Proven on the host | Needs the unit |
|---|---|---|---|
| 1 (this PR) | `usb_config` (bytes, lengths, parity with usb-device, enumeration past 256 B), `uac_out` (feedback servo, drain, the Linux-host model for a minute and an hour) | all of it | — |
| 2 | Serve today's 249 B through `control_in` + the static, `bcdDevice` unchanged | parity test | enumeration on Linux; `update` still works |
| 3 | (a): playback, feedback, `USB IN` level, `bench` line `usb_play … short … overruns … fb …`, `bcdDevice` 0x0040, guide page | the model; door/placement checks | enumeration, `pw-play`/`aplay` an hour (short 0, no "feedback out of range" in dmesg), start/stop many times, cable pull; macOS/Windows by testers (#841) |
| 4 | (b), after the AXI ruling: six-channel pacer model (52-frame room), pack, alt 2 | model, descriptors | six channels recorded on Linux; Windows' view of the cluster |
| 5 | (c), only if step 3 reads `usb_irq_pct` past 3 % | — | all |

## 9. Amendments in step 3

- The playback interface is 5, after MIDIStreaming (4, unchanged), so bank 1 allocates it after the door's allocation; the shipping layout with playback is 332 B (`usb_config::duplex`), `bcdDevice` 0x0040.
- No six channels in 0.3 (owner).
- USB IN's steps: `OFF`, −24, −18, −12, −9, −6, −3, 0 dB; default 0 dB, on SETTINGS › AUDIO ROUTING › USB AUDIO.
- AXI: ORBIT was 224 B short of the 32-KiB margin with the 512-B static and a packet buffer in the cell; the static is the layout's 332 B and the packet is read on the stack. ORBIT keeps 264 B over the margin.
- The OUT endpoint and the feedback IN are aimed at the frame now once a frame (SOF) and the OUT at the next frame after each packet; no incomplete-OUT interrupt is used.
