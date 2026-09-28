# 0034. Reset on a stalled audio interrupt with the IWDG, kicked from the controls tick

- **Status:** Proposed
- **Deciders:** firmware (audit #86)

## Context
Circular DMA replays its last 2.67 ms ring at full level once the CPU stops
rendering. Panic and HardFault silence the SAI in software, but a lockup (a
fault while stacking, e.g. a stack overflow) or a spin inside the audio
interrupt never reaches that code, and the unit buzzes until power is cycled.

## Decision
IWDG1 starts right after the SAI. The SysTick controls tick (500 Hz, lowest
priority) kicks it only when the DMA interrupt's block count has moved since
the previous tick (`audio_out::Heartbeat`). The timeout is
`audio_out::watchdog_timeout_ms`: twice the longest healthy gap between kicks
(one tick plus one audio block the tick can be held off by), 7 ms today. The
watchdog is frozen while a debugger halts the core. A halt after a panic,
HardFault or DMA error therefore ends in a reboot, not a permanent halt.

## Alternatives considered
- Kick from the main loop, as the audit suggested: the loop's slowest pass
  (full-screen flushes) is unmeasured, so a timeout long enough for it would
  also let the buzz run longer; a hung UI with healthy audio is not the fault
  being caught.
- Kick unconditionally from SysTick: misses a DMA that has stopped.

## Consequences
Nothing may mask interrupts, or hold the audio interrupt and SysTick off, for
the timeout or longer; a future flash or SD write that must, has to kick or
revisit this. A UI-only hang is not caught. The panic LED shows only until
the reset.

## Sources
RM0433: the independent watchdog and DMA error management chapters; audit issue #86.
