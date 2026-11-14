# 0272. Extend bank 2 into the bootloader slots behind restart stubs

- **Status:** Proposed (supersedes in part 0137: "nothing writes, erases or maps for write 0x08180000+", region 2's 512 KB, and "anything in 0x08180000+ always faults"; amends 0251's free-space measure)
- **Deciders:** owner (2026-11-14: open bank-2 sectors 4–7 so 0.4 keeps all its features), firmware

## Context
Bank 1 has about 43 KB free and bank 2's S0–S3 about 8 KB, which is ADR 0251's floor. 0.4's remaining work (DRUM, flat maps, VARIETY) doesn't fit. Bank-2 sectors 4–7 (0x08180000–0x081FFFFF, 512 KB) are bootloader 1.09's optional application slots, and ADR 0137 kept them no-access for good.

Research of Ixox/preenfm3 at 65cb963 (private notes) found:
- 1.09 jumps to 0x08180000 when MIXER (button 9) is held at power-on and to 0x081C0000 when EDIT (10) is held (`bootloader/Src/main.c:116-129`). Like the normal path, it calls `MX_Deinit()` and then loads MSP from word 0 and branches to word 1, with no validity check and without setting VTOR (`bootloader.cpp:304-310`). An erased slot faults into the bootloader's `while(1)`, and the unit hangs until power is cycled. That is inferred from the source, not tested.
- The bootloader never erases or writes bank 2 (`formatFlash` erases bank 1 only; `FLASH_If_Erase` is dead code), and stock firmware writes no flash.
- No firmware, fork or documented user path has ever used the slots.

## Decision
- **One image to 1 MB.** One link, one header, one door at 0x08100100, one seal and one `.bk2`, whose body runs contiguously from 0x08100000 and may now reach 0x081FFFFF (`BANK2_LEN` 1 MB). The header layout is unchanged, but `FORMAT` becomes 2.
- **Three output sections, because the stubs are fixed points.** `.bank2` keeps S0–S3 (`BANK2`, 512 K, `__bank2_end <= 0x08180000`). `.bank2_s4` (`BANK2_S4`, 0x08180000, 256 K) and `.bank2_s6` (`BANK2_S6`, 0x081C0000, 256 K) each open with a 1 KB slot reserve, `KEEP(*(.bank2.slot.mixer))` and `.edit`, asserted at 0x08180000 and 0x081C0000. Then come whole glob groups placed by hand. The `.bk2` is objcopied from all three with `--gap-fill 0xff`.
- **Slot stubs.** Each reserve starts with 64 B from `chimera_image::slot::stub(base)`. That is SP 0x20020000; a reset at `base + 9` that writes VTOR = 0x08020000, DSB, ISB, loads MSP and the reset vector from 0x08020000, and branches; and the tag `CHIMERA-SLOT`, a version byte and a slot byte at 0x30. The rest of the reserve is 0xFF. There is no validity check: a slot jump becomes the normal path's jump, into stock or Chimera alike. Every image links both stubs, however little code sits in S4–S7.
- **The installer guard.** Before its first erase, `install_with` checks each slot (MIXER = S4+S5, EDIT = S6+S7). A slot passes if it is entirely erased, or if its first sector carries the tag at 0x30. Anything else returns `InstallFail::SlotForeign(Slot)` with nothing erased or programmed in any sector. The boot shows `NOT INSTALLED / THE MIXER SLOT HOLDS OTHER CODE. / NOTHING WAS WRITTEN.` instead of DON'T POWER OFF; `status` reads `bank2 slot foreign mixer|edit` and ABOUT `SLOT IN USE`. The install is still one package, one command and one screen.
- **Install order:** S0 first (the commit word), S1–S3, then S5, S4 and the MIXER stub, then S7, S6 and the EDIT stub. After that come the body (skipping the reserves) and the header, with the commit word last. So a power cut leaves each slot erased or tagged, and the guard never locks Chimera out of its own torn install. `Bank2Sector` names S0–S7, and erases stay per sector: still no bank or mass erase, no bank-1 register, no option bytes.
- **MPU:** region 0 is unchanged. Region 1 (0x08100000, 1 MB, no access, XN, strongly-ordered) is unchanged. Region 2 is 0x08100000, **1 MB**, with the same Closed, Read, Write and Open states and attributes: RASR 0x1608_0027 / 0x1300_0027 / 0x0602_0027. No subregion disables. A double ECC error anywhere in bank 2 marks `BANK2_BAD` (one reset, S0 erased).
- **Budget:** `just check-bank2 <set>` prints S0–S3, S4–S5 and S6–S7 used and free, and the total. It holds each set to `tools/bank2-budget.txt` under bank 1's rules, and fails under ADR 0251's 8 KB floor, now measured as the total. `check-door-placed` bounds bank 2 at 0x08200000, and `check-slots` pins the stub bytes, their disassembly and empty reserves.
- **Release gates** run on the owner's unit only after the owner OKs flashing (spec § 6.3): a read-only check of the unit's 1.09 binary, flash, held MIXER and held EDIT at power-on, cold boot, update twice, power pulls at the new marks, revert to stock and back, guard refusal (gate build only), and audio during the install.

## Alternatives considered
- **Stay in S0–S3 and bank 1 (ADR 0137, 0251):** 0.4's features don't fit; shaving more is sound-identical rewrites at most a few KB at a time.
- **A second image or door for S4–S7:** this is ADR 0137's rejected stable-ABI option again.
- **Stubs written once and kept outside the image:** S4 and S6 never erased leaves 256 KB of code room, half the gain.
- **Stubs only when an image needs S4–S7:** the slots' state would then vary by release, and so would the revert and the guard, to save ~3 s of erase per seal change.
- **A stub that checks bank 1's vectors, or resets instead of jumping:** a check adds a failure mode the normal path doesn't have. A reset re-enters the bootloader, which loops while the button is still held.
- **`--enable-non-contiguous-regions` to flow one glob list across the regions:** not verified for the toolchain's rust-lld; revisit if hand-placing groups becomes a chore.
- **MPU subregions hiding sectors past the image:** the door check already proves bank 1 can't reach them, and it would add a header-dependent MPU word.
- **A custom bootloader:** the owner ruled it out (2026-11-12); 1.09 stays.

## Consequences
- Bank 2 grows by ~510 KB (512 KB less two 1 KB reserves), so 0.4 keeps its features. ADR 0251's bank-1 fallback has room again.
- A held MIXER or EDIT at power-on boots normally on every unit that ran a 0.4+ install. That keeps working after a revert to stock or to Chimera ≤ 0.3, since neither writes the slots. Before the first install, and in a ~1 s window of each install (S4 or S6 erased, stub not yet programmed), a held button hangs as it does today.
- Each seal change erases 7–8 sectors instead of up to 4, adding ~3–4 s of install. The `.bk2` is at least ~769 KB, most of it 0xFF.
- A unit whose slot holds foreign code refuses every install. The new bank 1 then runs with bank 2 closed, mostly silent, until the slot is cleared or the previous release is reflashed. No such unit is known.
- Chimera ≤ 0.3 reads a 0.4 header as `format 2` and reinstalls its own image into S0–S3 only.
- Chimera now occupies the slots. If another firmware wants them later, it must go through a new ADR.

## Sources
- Spec `docs/superpowers/specs/2026-11-14-bank2-slots-design.md`; ADR 0137, 0251; spec `2026-10-11-bank2-design.md` (decision 6, reversed here).
- (private notes); Ixox/preenfm3 65cb963 (GPLv3, read only, not copied): `bootloader/Src/main.c:116-129`, `bootloader/Src/bootloader.cpp:250-251, 304-310, 373-410`, `bootloader/Inc/flash_if.h:64-74`, `bootloader/Src/flash_if.c:55-101`, `firmware/Src/system_stm32h7xx.c:108,229`, `firmware/preenfm3_2MB.ld:50`.
- RM0433 § 4 (dual-bank flash, sector erase, ECC, FLASH_ECC_FA2R); ARMv7-M ARM § B3.5 (MPU RASR SIZE, SRD), § B1.5.3 (VTOR); `chimera-bootloader/src/main.rs` (the same jump, with checks).
