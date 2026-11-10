# 0256. Ship USB audio in every firmware build, with one USB MIDI cable

- **Status:** Proposed
- **Deciders:** owner (2026-11-10: "turn on USB audio for 0.3"), firmware

## Context
ADR 0248 built the UAC1 main-out stream behind feature `usb-audio`, off by default until the unit had run it. On 2026-11-10 the `--features usb-audio` build ran on the unit: it enumerated on Linux as "Chimera", stereo 24-bit 48 kHz; a capture of 9,301 packets had none missed or short, no resync, no dropout, the right pitch; a 6-minute run read sent 360,976, missed 0, short 0, resyncs 1. The owner turned it on for 0.3.

The stream and USB MIDI share one configuration in usb-device's 256-B control buffer. With the stream there is room for one MIDI cable (249 B), not ADR 0243's cable a Part (291 B). The owner accepts one cable.

## Decision
- **`usb-audio` is in chimera-stm32's default features.** Every shipping build has it: `default`, `orbit` (the released firmware) and `bank2-gate`, and so `release-bin` and `release`. The bench sets (`--no-default-features`) stay without it: they keep ADR 0243's cable a Part.
- **One USB MIDI cable in the shipping builds**, "Chimera", both ways, every channel (ADR 0222's). The Part cables of ADR 0243 remain only in the bench builds. This supersedes ADR 0243's per-Part cables as the default and ADR 0248's "Off by default".
- **`bcdDevice` 0x0030** for the shipping device (ADR 0248's), new under 0483:5740, so Windows matches its driver afresh instead of reusing the 0x0021 cache of the cable-a-Part layout.
- **Checks.** The separate `usb-audio` set of `just check`, `clippy`, `stack-check` and `tools/bank1-budget.txt` is gone: it is `default` now. The budget, stack, door and placement checks cover `default`, `orbit` and `bank2-gate` (`bank2-gate`'s door check and placement are now run by `just check`; `release-bin` runs the others'). The host test (`chimera-midi/tests/usb_class_test.rs`) enumerates the console alone, the shipping layout (stream and one cable) and the bench layout (cable a Part).

## Alternatives considered
- **Keep audio off and ship the Part cables.** The owner chose audio.
- **Both: audio and six Part cables.** 291 B, past the 256-B buffer: GET_DESCRIPTOR stalls and the host gives the port up. Fitting it means a vendored usb-device with a larger buffer.
- **A runtime switch between the layouts.** Two configurations, a re-enumeration and a SETTINGS row, for a choice the owner has made.

## Consequences
- Measured with `CHIMERA_STAMP_CLEAN=1` on main edc1e39c, each set against itself without the feature:
  - **Bank 1:** `default` 828,024 → 829,392 (+1,368), `orbit` 844,008 → 845,344 (+1,336), `bank2-gate` 829,752 → 831,320 (+1,568): the ring push in the audio interrupt, the three alt-setting shims, the closed-door mask and the `bench` line (ADR 0248's bank-1 side), with churn. Each line of `tools/bank1-budget.txt` is raised by its measure, naming this ADR.
  - **Bank 2:** +1,248 B in each set. ORBIT's bank 2 has 2,496 B free (from 3,744), under ADR 0251's 8 KB line: further features may go to bank 1 under 0251.
  - **AXI:** free −4,720 B (`orbit` −4,728: the 4,104-B ring, the 576-B packet buffer, the stream's state), leaving 39,352 B (`default`), 37,576 B (`orbit`) and 39,348 B (`bank2-gate`) from `__sheap` to the end of AXI, against the 32-KiB margin the link asserts. D2 unchanged.
  - **Stack:** nested total unchanged (`default` 58,996, `orbit` 59,572 B of 63,232).
- A DAW sees one MIDI port; each Part plays from its own MIDI channel, as on DIN.
- The configuration sits 7 B under the control buffer: any descriptor growth in a shipping build is a decision, held by the host test and the const asserts.

## Sources
- ADR 0222, 0243, 0248, 0251; the unit's captures of 2026-11-10 (coordinator).
- `chimera-stm32/Cargo.toml`, `Justfile`, `tools/bank1-budget.txt`, `chimera-midi/src/usb_class.rs`, `chimera-midi/tests/usb_class_test.rs`, `docs/guide/MIDI.md`.
