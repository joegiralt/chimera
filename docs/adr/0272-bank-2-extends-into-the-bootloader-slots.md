# 0272. Extend bank 2 into the bootloader slots behind restart stubs

- **Status:** Proposed. It supersedes in part 0137 ("nothing writes, erases or maps for write 0x08180000+", region 2's 512 KB, "anything in 0x08180000+ always faults") and amends 0251's free-space measure.
- **Deciders:** owner (2026-11-14: open bank-2 sectors 4–7 so 0.4 keeps all its features); controller (2026-11-14: refusal wording kept, no DFU pre-check in 0.4, stubs in every image, DRUM stays in bank 1); firmware

## Context
Bank 1 has about 43 KB free and bank 2's S0–S3 about 8 KB, which is ADR 0251's floor. 0.4's remaining work doesn't fit. Bank-2 sectors 4–7 (0x08180000–0x081FFFFF, 512 KB) are bootloader 1.09's optional application slots, which ADR 0137 kept no-access for good.

Research of Ixox/preenfm3 at 65cb963 (private notes) found:
- **The jump.** 1.09 jumps to 0x08180000 when MIXER (button 9) is held at power-on, and to 0x081C0000 when EDIT (10) is. Like the normal path, it calls `MX_Deinit()`, loads MSP from word 0 and branches to word 1, with no validity check and no VTOR. An erased slot hangs the unit until power is cycled. That is inferred, not tested.
- **Nothing writes bank 2.** The bootloader never erases or writes it, and stock firmware writes no flash.
- **No firmware, fork or user path has ever used the slots.**

## Decision
- **One image of exactly 1 MB, in one output section.** `BANK2` is 0x08100000, 1M. `.bank2` keeps its one header, door at 0x08100100, seal and `.bk2`. It places its groups around four fixed points:
  - `. = base + 0x80000`: MIXER's 1 KB reserve;
  - `+ 0xBFFE0`: S5's end marker;
  - `+ 0xC0000`: EDIT's reserve;
  - `+ 0xFFFE0`: S7's end marker.

  Each group's end is asserted below its next fixed point, and the section ends `} > BANK2 =0xFF`. `-R/-j .bank2`, `door_hits` and the stack check are unchanged. `FORMAT` is 2, and `body_len` is 1 MB − 256.
- **The stub.** Each reserve starts with two flash words from `chimera_image::slot`, then 960 B of 0xFF.
  - The code word holds SP 0x20020000 and a reset at `base + 9`. The reset writes VTOR = 0x08020000, runs DSB and ISB, loads MSP and the reset vector from 0x08020000, and branches. There is no validity check, and it boots stock or Chimera alike.
  - The tag word holds the literals and `CHIMERA-SLOT`, a version and the slot.
  - The end markers hold `CHIMERA-SLOT-END`.
- **Install order a slot:** erase S5 (skipped when the guard found it erased), program S5's marker, erase S4, program the tag word, then the code word. The invariant is that one clean tag or marker survives at every instant, so two cuts on consecutive installs can't leave a slot that reads foreign. A torn erase is assumed to leave ECC words anywhere in its sector, so a sector is erased only while the other holds clean evidence, and an erased sector is never erased again. The order follows the guard's evidence (spec § 3.1):
  - a tag goes S5 first;
  - a marker alone, or the boot's slot mark, goes S4 first: erase S4, tag word, code word, erase S5, marker;
  - a scan with both erased writes marker, tag and code, with no erase;
  - a scan with S4 erased only writes tag and code before any erase;
  - a scan with S5 erased only writes the marker first;
  - a scan with neither erased goes S4 first. That case has a residual: three cuts on consecutive installs (a torn marker, a torn tag program, then a torn erase) can still read foreign. These are separate writes, and the EDIT slot (S7/S6) goes the same way. The full order is: S0 first (the commit word), S1–S3, the MIXER slot, the EDIT slot, the body (skipping those six words), the header, and the commit word last. Erases stay per sector: no bank or mass erase, no bank-1 register, no option bytes.
- **`slot_guard()`** runs separately, after `bank2_mark(BANK2_INSTALLING)` and before the install screen, in the boot path and in the console's install. `InstallFail::SlotForeign(Slot)` comes from it, before any erase or program. A slot is ours if (a) S4's tag word reads cleanly, or (b) S5's marker reads cleanly and S4's code word is ours, torn or erased; it reads those first. The guard reports which evidence it accepted, and the install picks the order from it. The stub's code bytes are frozen: changing the stub needs a new ADR and a new guard rule. Otherwise (c) every word of both sectors must be erased, except S4's code word, S4's tag word and S5's marker word, the positions only Chimera writes. Rule (c) widens the review's "fully erased", so that a power cut during a first install's marker write can't lock Chimera out.
- **ECC-safe reads.** The guard reads with FAULTMASK set and CCR.BFHFNMIGN = 1, then checks and clears FLASH_SR2.DBECCERR and SNECCERR. A flagged word is not clean. At the three Chimera positions, a word that was silently corrected (SNECCERR) passes as a torn one does. That a flash double-ECC error is a precise, maskable bus error must be verified on a spike gate first. If it isn't, the fallback is a `BANK2_SLOT_TORN | slot` reset mark that makes the next guard treat that slot as ours-torn.
- **On a refusal** nothing is written. The boot shows `NOT INSTALLED / THE MIXER SLOT HOLDS OTHER CODE. / NOTHING WAS WRITTEN.` for 5 s or a button. `status` reads `bank2 slot foreign mixer|edit`, and ABOUT reads `SLOT IN USE`. Recovery is `update` with the 0.3 package. Only the owner may clear a slot, with the gate build's `bank2 erase slots`. ROM DFU never writes 0x0818xxxx.
- **MPU:** regions 0 and 1 are unchanged. Region 2 is 0x08100000, **1 MB**, with the same four states: RASR 0x1608_0027 / 0x1300_0027 / 0x0602_0027. No subregions.
- **Tooling:**
  - `chimera-usb.py`'s `BK2_MAX` becomes 1 MB;
  - 0.2.0's and 0.3.0's `put` usage lines join `OLD_PUT_REFUSALS`, so an update from them flashes first and stages after, still one command and one install screen;
  - the install waits rise to 120 s;
  - `just check-bank2` (per region and total, `tools/bank2-budget.txt`, ADR 0251's 8 KB floor on the total);
  - `just check-slots` (the bytes, the 0xFF fill, the disassembly, empty reserves);
  - `check-door-placed`'s bound becomes 0x08200000.
- **Release gates** run on the owner's unit only after the owner OKs flashing (spec § 6.3):
  - read-only checks of the 1.09 binary, the slots and bank 2's WRP;
  - flash from 0.3, the ECC spike, held MIXER, held EDIT, cold boot and update twice;
  - pulls that tear S4's and S6's erases and land between the tag and code words;
  - revert to stock and back, guard refusal, audio, and boot times.

## Alternatives considered
- **Stay in S0–S3 and bank 1:** 0.4 doesn't fit.
- **Three output sections:** `door_hits` exempts only `.bank2`, and lld fills inter-section gaps with zeros or traps, not 0xFF, which breaks the read-back CRC.
- **A second image or door:** ADR 0137's rejected stable-ABI option.
- **Stubs kept outside the image:** half the room.
- **Stubs or length that vary by release:** the slots' state, revert and guard would then vary too.
- **A stub that checks bank 1, or resets:** a check adds a failure mode, and a reset loops while the button is held.
- **Tag word or code word first, with no end marker (the first draft):** a torn S4 erase or tag write left the slot unreadable as ours, and an ECC fault on it cost a reset every other boot.
- **A DFU pre-check of the slots in `update`:** not in 0.4 (decided); no foreign slot is known.
- **MPU subregions:** the door check already covers it.
- **A custom bootloader:** ruled out (2026-11-12).

## Consequences
- Bank 2 grows by ~510 KB. New 0.4 work goes to S4–S7, and DRUM stays in bank 1.
- A held MIXER or EDIT at power-on boots normally on every unit that ran a 0.4+ install, including after a revert to stock or to ≤ 0.3. It still hangs before the first install, and after a power cut inside a slot's window. The window lasts from S4's (or S6's) erase to its code word: ~1 s typical, up to ~4 s.
- Every install erases S0–S3 and each slot sector not already erased: up to eight sectors, ~8 s typical, ~32 s worst before programming. Every boot CRCs 1 MB. The `.bk2` is always 1,048,576 B.
- A unit with a foreign slot refuses every install and runs the new bank 1 with bank 2 closed until it is reflashed with 0.3. No such unit is known.
- Chimera ≤ 0.3 reads a 0.4 header as `format 2` and reinstalls into S0–S3 only.
- Chimera now occupies the slots, and giving them back takes a new ADR.

## Sources
- Spec `docs/superpowers/specs/2026-11-14-bank2-slots-design.md` (revised after review B1–B6); ADR 0137, 0251; spec `2026-10-11-bank2-design.md` (decision 6, reversed here).
- (private notes); Ixox/preenfm3 65cb963 (GPLv3, read only): `bootloader/Src/main.c:116-129`, `bootloader/Src/bootloader.cpp:250-251, 304-310, 373-410`, `bootloader/Inc/flash_if.h:64-74`, `bootloader/Src/flash_if.c:55-101`.
- `chimera-utils/src/bank2.rs:145-151, 181` (`door_hits`); `chimera-core/src/console/put.rs:270`; `tools/chimera-usb.py:107-114, 858`; `chimera-image/src/regs.rs:78-80`.
- RM0433 § 4 (flash, ECC, FLASH_ECC_FA2R); DS12117 Table 51 (erase time); ARMv7-M ARM § B3.5 (MPU), § B1.5.3 (VTOR), § B3.2.8 (CCR.BFHFNMIGN), § B1.4.3 (FAULTMASK).
