# 0332. The real panel's answer: judge COLMOD on its DBI field, clock the answer with DC high

- **Status:** Accepted (2026-11-23)
- **Deciders:** owner, firmware

## Context
On the owner's unit (firmware 046257ca: 0.4 with #1123 and #1129), ADR
0326's health check never armed. `stats` read `panel_armed 0`,
`panel_reinits 0` and `spi_errors 0` many minutes after boot, so the
recovery for #967 was off on the hardware it exists for.

A diagnostic build (5586796e) added `stats`' `panel_read`. On the unit it
read:

    panel_read 9c00 4800 05ff reads 6

That is RDDPM 0x9C and RDDMADCTL 0x48, as ADR 0326 expects. RDDCOLMOD,
though, read **0x05**, where ADR 0326 expects 0x55 (the value `init`
writes to COLMOD). This panel reports the DBI field (bits 2:0, 0b101 =
16 bits a pixel) and reads its DPI bits (6:4) as 0. The check compared all
eight bits, so it never saw the panel healthy.

The reads clock their answer straight after the command. That matches the
ILI9341 datasheet V1.11 (p. 38, 4-wire serial protocol, 8-bit read for
0Ah–0Fh): the read data follows D0 of the command on SDO, with no dummy bit.
The byte after each answer showed no split value either. The diagnostic
build had also changed the read so that DC rises after the command, and
the answer is clocked in by a 0xFF dummy. Before that, D/CX stayed low for
the dummy, so the panel could take the 0xFF as a new command. Reference
readers raise DC the same way (Adafruit `readcommand8`, ropg/spi_lcd_read).

## Decision
This supersedes in part ADR 0326, its healthy mode's "COLMOD 0x55" and its
read's single CS frame with DC held low.

- **COLMOD's DBI field only.** `Mode::healthy` compares RDDCOLMOD's DBI field
  alone: `colmod & 0x07 == 0x05`. RDDPM 0x9C or 0xDE and MADCTL 0x48 are
  unchanged. A garbled COLMOD still reads off-mode: 0x00, or 0x06 for 18
  bits a pixel.
- **The answer is clocked with DC high.** The read sends the command with
  DC low and waits for it to finish shifting. It then raises DC and clocks
  the answer in with one 0xFF dummy, all in one CS frame, at ADR 0326's
  6.25 MHz.
- **`panel_read` stays in `stats` as a support line.** It reads
  `panel_read 9c 48 05 reads <n>`: each register's last answer in hex,
  `none` for a read that failed, and the count of reads attempted. It is
  written in bank 2 through the panel's stats function pointer, and nothing
  of it lands in bank 1.

## Alternatives considered
- **Learn the mode from the first healthy answer.** That was ADR 0326's
  first draft, dropped in review: a noisy first read would become the
  reference. The known values stay. Only COLMOD's field is narrowed, to the
  bits this panel reports.
- **Read COLMOD's DPI field too.** The DPI field is the RGB interface's
  format, and this unit doesn't use that interface, so it carries no signal
  here.

## Consequences
- On the owner's panel the check arms within one check period after boot,
  and recovery is on.
- A panel that reports the full 0x55 also passes, because its DBI field is
  0b101.
- Bank 1 +0 B; bank 2 +512 B in default and orbit, +480 B in bank2-gate
  (against 0.4 at 72e1f7b61). The lines are raised under ADR 0326 and this
  ADR.

## Sources
- The owner's unit, `stats` on 2026-11-23 (diagnostic build 5586796e):
  `panel_read 9c00 4800 05ff reads 6`.
- ILI9341 datasheet V1.11: p. 38 (4-wire serial protocol, 8-bit read) and
  § 19.3.4 (serial timing).
- Adafruit_ILI9341 `readcommand8`; ropg/spi_lcd_read
  (https://github.com/ropg/spi_lcd_read). Neither was copied: the
  technique is the datasheet's.
- `chimera-core/tests/panel_link_test.rs::the_real_panels_answer_arms`.
- #967, #1123, ADR 0326.
