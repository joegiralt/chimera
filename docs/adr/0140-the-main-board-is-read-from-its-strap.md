# 0140. Read the main board from its strap and wire the HC165 to match

- **Status:** Proposed
- **Deciders:** owner (product), Claude (implementation)

## Context
Chimera hard-wired the HC165 (every encoder and button) to PF0 CLK, PF1
LOAD, PF2 DATA. That is the LQFP176 and LQFP144 main boards' wiring. The
PreenFM3 main PCB labelled "1.6 - 100" (Nov 2021) carries an STM32H7 in
LQFP100, which has no port F: its HC165 sits on PA0 DATA, PA1 LOAD,
PA2 CLK. On that board every control of Chimera was dead, the failure
stock's v0.109 release notes describe for older stock firmware (#408).

Each main board says which it is with a 4-bit strap on PD0..PD3, read as
pull-up inputs, PD0 the top bit. Stock's values: 0b0001 LQFP176,
0b0010 LQFP144, 0b0011 LQFP100. Stock reads the strap first, before any
other GPIO, and with no settle delay beyond its own init calls.

Facts only came from stock (`lib/Src/preenfm3lib.c`, `lib/Inc/preenfm3_pins.h`,
GPLv3): the pin numbers and the strap values. No code was copied.

## Decision
- `chimera_engine::board` (re-exported as `chimera_core::board`) is pure:
  `BoardId { Lqfp176, Lqfp144, Lqfp100, Unknown(u8) }` from
  `from_straps(u8)` or `from_pins([PD0, PD1, PD2, PD3])`, and the HC165
  wiring as data, `Hc165Pins { port, data, load, clk }`.
- Supported boards:

  | Strap  | Board          | HC165               |
  |--------|----------------|---------------------|
  | 0b0001 | LQFP176        | PF2 DATA, PF1 LOAD, PF0 CLK |
  | 0b0010 | LQFP144        | PF2 DATA, PF1 LOAD, PF0 CLK |
  | 0b0011 | LQFP100 ("1.6 - 100") | PA0 DATA, PA1 LOAD, PA2 CLK |
  | other  | Unknown        | as LQFP176, and says so |

- The shell (`chimera-board::boot`) reads PD0..PD3 with pull-ups after the
  clocks and before the HC165 pins are configured, waits 10 us for the
  pull-ups (the RC is well under 1 us; stock waits none), and configures
  DATA/LOAD/CLK on the port the board names. The LQFP100 never touches
  port F. The pins stay pulled-up inputs.
- The SysTick read is port-agnostic: `controls::wire` stores the port's
  base and the three pin masks in atomics before `enable`; each tick
  loads them once. Five loads in a tick of about 12 us of HC165 clocking
  every 2 ms, off the audio path: no measurable cost.
- The board is visible: the console's `status` prints
  `board <lqfp176|lqfp144|lqfp100|unknown> straps=0bXXXX`, and SETTINGS >
  SYSTEM > ABOUT's chip cell, relabelled from REV to CHIP, shows the
  silicon rev then the package's pin count (`V 176`, `V 100`; `V 176?`
  for an unknown strap). Both describe the MCU; `V LQFP176` does not fit
  the 74 px column. No viz line is added.
- Chimera uses nothing else on PA0..PA2 or PD0..PD3 on any board (PA5/PA7
  SPI1, PA9 SPI2, PA11/PA12 USB; no ADC), and every other pin it uses
  exists on LQFP100.
- Not supported yet, post-alpha: front-panel (control board) variants.
  N12 encoders (a second quadrature table), and the SMD control board
  v2.x, whose wiring was not checked. Neither is detectable: stock makes
  them user settings: private issue #536.

## Alternatives considered
- A user setting for the board instead of the strap: a dead panel cannot
  reach SETTINGS. Rejected.
- Per-port monomorphised ISRs chosen by a function pointer: the same
  register writes either way, more flash for nothing measurable.
- A fourth ABOUT viz line for the board: another branch takes that space.
- Unknown straps halting or warning on screen: the commonest board's
  wiring is the useful guess, and ABOUT and `status` already say so.

## Consequences
- A "1.6 - 100" unit gets working controls; LQFP176/144 units behave as
  before.
- Any new board with a new strap value boots with LQFP176 wiring until a
  row is added here and in `board.rs`.
- The silicon rev and the board are now in `AudioStats`, which ABOUT reads.

## Sources
- Issue #408; research ledger (private notes).
- Stock PreenFM3 (GPLv3, facts only): `lib/Src/preenfm3lib.c`,
  `lib/Inc/preenfm3_pins.h`, release v0.109 notes.
- Relates to ADR 0020 (silicon rev Y/V clock plan).
