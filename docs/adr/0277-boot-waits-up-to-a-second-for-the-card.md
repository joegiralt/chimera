# 0277. Give the card up to a second to answer at boot

- **Status:** Proposed
- **Deciders:** firmware (field report chimera-releases#16)

## Context
ADR 0045 asks whether a card is in the slot before each acquire (#186):
CMD0 with CS low, `PRESENCE_TRIES` (3) times, about 0.3 ms a try, and no
answer is NO CARD. That keeps an empty-slot re-read from stalling the UI.

At boot it was the only chance. Boot's first acquire (SYSTEM, then the
project) ran the check twice per mount, a re-init's wake clocks between,
so the card got about 2 ms of CMD0s in one burst and never another until
the user read the card again. The stock driver tries CMD0 up to 100 times;
its bootloader's SD Card Access, seconds later, always found the card.

A tester on barrel power saw NO CARD on 8 of 11 normal boots with a good
card (chimera-releases#16). The SD spec (Physical Layer Simplified
Specification § 6.4.1) gives a card up to 1 ms after VDD is stable, then
74 clocks, before CMD0; real cards and slow rails take longer, and nothing
on the board says when the card's rail is stable.

## Decision
- The first acquire after reset waits for the card: wake clocks (CS high)
  and the presence check, again every `BOOT_RETRY_EVERY` (10 ms), until
  the card answers or `BOOT_WINDOW` (1 000 ms of pauses) has passed
  (`chimera_fat::sd::Presence`, a host-tested core; `SdSpi::start_op` is
  the shell).
- Every later acquire asks once, as #186: an empty-slot re-read stays fast.
- What the boot found is kept as `BootCard`, `Ready(ms)` or `Absent(ms)`,
  and the console's `status` prints it as `boot_card ready after N ms`,
  `boot_card no answer after N ms` or `boot_card untried`. With the boot's
  toast it tells the three cases apart: no answer (NO CARD, `no answer`),
  answered but init failed (NO CARD, `ready`), mount failed (any other
  card message).
- N counts the pauses only; each try's own time (wake and up to three
  CMD0s, about 1 ms) comes on top, so a boot with no card waits about 1.1 s.

## Alternatives considered
- **More `PRESENCE_TRIES`.** Back-to-back tries cover milliseconds, not a
  rail that is still rising, and every UI re-read would pay them.
- **A fixed delay before the first acquire.** Every boot pays it, and it
  still guesses one number for every card.
- **Retry NO CARD later from the UI loop.** The project and SYSTEM are
  chosen at boot; a card found later would replace what is playing.
- **Drop the presence check at boot and let the acquire's `SD_ACQUIRE_MS`
  run.** embedded-sdmmc's CMD0 loop gives up after `SD_ACQUIRE_RETRIES`
  short retries, not after a time, so it would not wait either.

## Consequences
- A boot with no card, or a dead one, takes about 1.1 s longer behind the
  splash. The card is inside the case, so that boot is rare.
- A boot with a slow card waits for it instead of loading the starter
  project.
- The watchdog is armed after the card's boot work (the start-up order in
  chimera-board's `run`), so the wait never runs on its clock.
- Bank 1 grows 172 to 344 B a set (default +304, orbit +296, bank2-gate
  +344, orbit,usb-config-writer +300,
  bench-orbit +188, bench-tape +172; the other bench sets fit their
  lines): the wait sits in the SD bus's
  `start_op`, bank-1 code with no door, and the `status` line in the
  console, which has none either; an ADR 0251 bank-1 exception.
- Supersedes in part ADR 0045's "presence check before each acquire": at
  boot the check repeats for up to `BOOT_WINDOW`.

## Sources
- chimera-releases#16 (field report: 8 of 11 boots NO CARD, barrel power).
- private issue #186 (the presence check).
- SD Physical Layer Simplified Specification v9.00, § 6.4.1 Power Up and
  § 7.2.1 (SPI mode entry).
- PreenFM3 stock `lib/Src/adafruit_802_sd.c` (`SD_GoIdleState`,
  `SD_MAX_TRY` 100) and `bootloader/Src/main.c` (a normal boot jumps
  without touching the card), read for behaviour only.
