# 0034. Reset on a stalled audio interrupt with the IWDG, kicked from the controls tick

- **Status:** Proposed
- **Deciders:** firmware (audit #86)

## Context
Circular DMA replays its last 2.67 ms ring at full level once the CPU stops
rendering. Panic and HardFault silence the SAI in software, but a lockup (a
fault while stacking, e.g. a stack overflow) or a spin inside the audio
interrupt never reaches that code, and the unit buzzes until power is cycled.

## Decision
IWDG1 is armed late in start-up: after the audio and the first frame, and
before USB, whose core enable spins with interrupts masked (a hang there
must reset, not freeze; `usb::preflight` bounds its preconditions first),
and only once `audio_out::LiveCheck` sees the block count and the
controls tick both moving (`watchdog::Live`, else a reset after
`LIVE_WAIT_MS`). Arming it before the audio, as first decided, put the
start-up's own gaps on its clock: a cold boot reset twice under it
(2026-10-02). The tick kicks only once arming has returned, so no kick can
interleave with its setup. The SysTick controls
tick (500 Hz, lowest priority) kicks it only when the DMA interrupt's block
count has moved since the previous tick (`audio_out::Heartbeat`).

The timeout, `audio_out::WATCHDOG_TIMEOUT_MS`, is 100 ms: the longest
stretch the audio interrupt may starve the controls tick that we accept.
**A sustained overrun longer than the timeout counts as a hang**: a render
that keeps missing its half re-enters the audio interrupt back to back,
SysTick never runs, and the unit resets. A shorter overrun only clicks and
counts in OVERRUNS, as before. 100 ms is well clear of the longest healthy
gap between kicks (two ticks plus one audio block, 5.3 ms), even at the
LSI's fastest 33.6 kHz (95 ms), and at /4 its reload, 800, fits the 12-bit
register; the real LSI rate only stretches or shrinks the time.

The watchdog is frozen while a debugger halts the core. A halt after a
panic, HardFault or DMA error therefore ends in a reboot, not a permanent
halt; the boot reads and clears RCC_RSR, and the AUDIO page shows the last
reset's cause (RST WDOG after a watchdog reset).

## Alternatives considered
- Kick from the main loop, as the audit suggested: the loop's slowest pass
  (full-screen flushes) is unmeasured, so a timeout long enough for it would
  also let the buzz run longer; a hung UI with healthy audio is not the fault
  being caught.
- Kick unconditionally from SysTick: misses a DMA that has stopped.

## Consequences
Nothing may mask interrupts, or hold the audio interrupt and SysTick off, for
the timeout or longer; up to 100 ms of buzz can play before the reset; a
future flash or SD write that must, has to kick or revisit this. A UI-only hang is not caught. The panic LED shows only until
the reset; RST WDOG on the AUDIO page is the lasting trace.

## Sources
RM0433: the independent watchdog and DMA error management chapters;
STM32H750 datasheet: LSI characteristics; audit issue #86; batch 1 review
(M1, M2).
