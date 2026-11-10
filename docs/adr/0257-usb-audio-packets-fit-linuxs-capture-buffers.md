# 0257. Hold USB audio's packets to 72 frames, the room Linux gives each

- **Status:** Proposed
- **Deciders:** firmware (the unit's 2026-11-10 hour-long capture)

## Context
ADR 0248 set wMaxPacketSize to 576 B (96 frames) so a bus frame the device was late for is made up in the next packet. On 2026-11-10 an hour's `pw-record` from the unit lost 3.4 % of its frames once the host was loaded (load average 25–30). The kernel logged `xhci_hcd: WARN: buffer overrun event` 11,244 times. By the day's end the kernel's count (11,677) equals the unit's `bench` `missed` (11,677) exactly: every missed bus frame made one overrun. Ten `nice` busy loops (load 18) made neither; the overruns came with the kernel's own late work (`workqueue: vmstat_update hogged CPU`, `xhci_hcd: Frame ID ... beyond range`) under the builds.

A missed frame made the next packet 96 frames. Linux's snd-usb-audio does not size a capture packet's buffer from wMaxPacketSize. It uses its own maximum, half again the nominal rate (sound/usb/endpoint.c, mainline as of 2026-11, `snd_usb_endpoint_set_params`: `ep->freqmax = ep->freqn + (ep->freqn >> 1)`; `curpacksize = maxsize` unless the format sets MaxPacketsOnly; `prepare_inbound_urb` sets each `iso_frame_desc[i].length = ep->curpacksize`). At 48 kHz that is 72 frames, 432 B. xHCI drops a larger packet whole. The missed frames came from the host: under load it skipped frames' IN tokens, so the armed packet went a frame late.

## Decision
- **`uac::MAX_FRAMES` is `NOMINAL + NOMINAL / 2` (72), and wMaxPacketSize 432 B.** The descriptor is the same length; only the value changes.
- **A missed bus frame is owed, not sent at once.** `Pacer` keeps `owe`: `NOMINAL` for each bus frame skipped (a gap of 2 to `HIGH / NOMINAL` frames; `missed` counts the frames). Each packet pays what fits under `MAX_FRAMES` after its trimmed `NOMINAL`. The fill's error leaves out what is owed, so the trim does not pay it twice. A resync, a short or `stop` clears it.

This supersedes 0248's 576-B wMaxPacketSize and its catch-up in one packet.

## Alternatives considered
- **Keep 576 and set MaxPacketsOnly (`bmAttributes` bit 7) in the class-specific endpoint.** Linux would then give every packet 576 B, but the bit says the device always fills packets, which this one does not, and other hosts may act on it.
- **Keep the 576-B descriptor and cap at 72 in the pacer only.** It works on Linux, but the descriptor would claim room that no packet uses.
- **Cap at 72 with no owed frames.** The heavy-render model went short (the smoothed fill took the debt as error, then overshot it).

## Consequences
- A missed frame is made up over two packets (72, then 72), not one. The heavy-render model (60–95 %) and a host that skips 2 % of tokens both run clean for a minute; a skipping host at 1 % runs clean for an hour.
- AXI: the packet buffer is 144 B smaller.
- A host that skips many tokens in a row can still trigger a resync: 72 frames is the most a packet catches up.

## Sources
- Linux mainline (2026-11) `sound/usb/endpoint.c` (`snd_usb_endpoint_set_params`, `prepare_inbound_urb`); `sound/usb/pcm.c` `retire_capture_urb` takes a packet's `actual_length` whatever its status.
- USB 2.0 § 5.12.4.1 (asynchronous source); UAC1 § 4.6.1.2 (MaxPacketsOnly).
- `chimera-midi/tests/uac_test.rs`: `stream_holds_when_the_host_skips_frames`, `pacer_never_sends_more_than_the_host_has_room_for`.
