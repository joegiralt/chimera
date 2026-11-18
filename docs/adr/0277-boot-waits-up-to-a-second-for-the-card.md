# 0277. Give the card up to a second to answer at boot

- **Status:** Proposed
- **Deciders:** firmware (field report chimera-releases#16)

## Context
A tester on barrel power saw NO CARD on 8 of 11 normal boots with a good
card (chimera-releases#16); the bootloader's SD Card Access, seconds
later, always found it.

What the code shows. ADR 0045 asks whether a card is in the slot before
each acquire (#186): CMD0 with CS low, `PRESENCE_TRIES` (3) times, about
0.3 ms a try, and no R1 is NO CARD. At boot that was the only chance:
each mount ran the check twice (a re-init's wake clocks between), so the
card got about 2 ms of CMD0s in one burst, and nothing asked again until
the user re-read the card. The stock driver tries CMD0 up to 100 times.
And any R1 counted as a card, 0x00 included: a card held in power-up (DO
low) passed the check, then failed embedded-sdmmc's CMD0, which wants
idle (0x01) within `SD_ACQUIRE_RETRIES` tries, and read NO CARD too.

What it does not show. By the first CMD0 the card has had at least
575 ms of rail since reset (`lib.rs`'s 250 ms before the display, the
display's ~325 ms of init delays; `asm::delay` is a minimum), far past the
SD spec's 1 ms power-up (Physical Layer Simplified Specification
§ 6.4.1). Why this card misses 2 ms of CMD0s that late (a slow rail on
barrel power, a card with a long internal start, a card that answers
0x00 for a while) is unproven. This decision fixes boot's patience and the
0x00-as-ready path, and records what the card did; the tester's
`boot_card` readings decide whether #16 is fixed.

## Decision
- The first acquire after reset waits for the card to go idle: wake
  clocks (CS high) and a presence burst, again every `BOOT_RETRY_EVERY`
  (10 ms, a `NonZeroMillis`), until a CMD0 answers R1 0x01 or
  `BOOT_WINDOW` (1 000 ms, measured on the cycle counter from the first
  burst; the pauses' sum where it doesn't count) has passed
  (`chimera_fat::sd::Presence`, host-tested; `SdSpi::start_op` is the
  shell).
- A card that answered but never went idle in the window is still a card
  (#186): the acquire tries it.
- Every later acquire asks one burst, any R1 a card, as #186: an
  empty-slot re-read stays fast.
- What boot found is a `BootCard`, its time measured from the first CMD0:
  `Ready` (idle), `Busy` (answered, never idle), `Absent` (no answer), or
  `Late` (no answer in the window, found by a later ask, such as mount's
  re-init retry). A card put in by hand after an empty-slot boot
  also reads `Late` (rare: the card is inside the case). `status` prints
  `boot_card ready|busy|no answer after N ms`, `boot_card late, none in
  N ms` (N the window, not when it answered), or `boot_card untried`. With the boot's toast: NO CARD and
  `no answer` is an empty or dead slot; NO CARD and `ready` or `busy` a
  failed init; any other card message a failed mount.
- The status cell stores `BootCard::to_bits` (a 3-bit tag over 29 bits of
  ms, saturating at `BootCard::MAX_MS`, about six days).

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
- A boot with no card, or a dead one, takes about 1 s longer behind the
  splash. The card is inside the case, so that boot is rare.
- A boot with a slow card waits for it instead of loading the starter
  project.
- The watchdog is armed after the card's boot work (the start-up order in
  chimera-board's `run`), so the wait never runs on its clock.
- Bank 1 grows 160 to 712 B a set (default +576, orbit +568, bank2-gate
  +712, orbit,usb-config-writer +584, the bench sets +160 to +488): the
  wait sits in the SD bus's `start_op`, bank-1 code with no door, and the
  `status` line in the console, which has none either; an ADR 0251 bank-1
  exception.
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
