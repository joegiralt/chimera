# 0137. Bank 2 holds sealed code behind one door

- **Status:** Proposed
- **Deciders:** owner (bank-2 design decisions 1–8, 2026-10-11), firmware

## Context
Bank 1's firmware area (0x08020000–0x080FFFFF, 917,504 B) is nearly full: the shipping build leaves about 50 KB, ORBIT less, and TAPE alone costs about 46 KB (ADR 0132 took it out for alpha). The STM32H753 has a second 1 MB flash bank that nothing uses. Its sectors 4–7 (0x08180000+) are bootloader 1.09's slots, which the stock bootloader jumps to on a held button; sectors 0–3 (0x08100000–0x0817FFFF, 512 KB) are free. Calling code in flash that may be erased, torn, half-written or from another build would fault or worse, so the way in has to make a wrong call impossible, not unlikely (spec `docs/superpowers/specs/2026-10-11-bank2-design.md` §§ 1–2).

## Decision
- **One link, two regions.** One `cargo build` places `.bank2` at 0x08100000 (`BANK2`, 512 KB, `bank2.x`); `just bank2-split` cuts it out as `<out>.bk2` and writes bank 1 as `<out>.bin` with `-R .bank2`. Bank 2 is S0–S3 only: nothing writes, erases or maps for write 0x08180000+, the option bytes, bank 1's flash registers, or bank or mass erase.
- **A 256-byte header** at 0x08100000 (`chimera-image`): magic `CHIMERA2`, format 1, body length and CRC-32, door offset, seal, version, and a commit word (`0xB2C0_FFEE` ×8) programmed last.
- **The seal** is the first 8 bytes of the body's SHA-256, computed on the host after the link (`chimera-utils bank2-seal`) and patched into bank 1's `BANK2_SEAL`. Equal bodies mean every cross-reference is right for either bank 1, so the seal is an exact compatibility key. A build with no seal step keeps `UNSEALED` and never opens bank 2.
- **The judge** (`chimera_image::judge`, pure, host-tested) reads in order: commit word (`Empty`, `Torn`), magic (`Foreign`), format (`Format`), bounds (`Corrupt`), seal (`Stale`), body CRC (`Corrupt`), else `Good`. A read the flash refuses is `Unreadable`, reported `corrupt`.
- **One door, at 0x08100100** (`DOOR_ADDR`, right after the header): a table of function pointers and data. A `Door` exists only from `Door::open`, a `Good` verdict over live bank-2 reads (`Bank2Memory`) for this build's seal; `DOOR_CELL.open` takes it and refuses any other address; bank 1 reaches the table only inside `DOOR_CELL.with`. A post-link check (`door-check`) fails the build if any bank-1 relocation resolves into bank 2.
- **The boot judges bank 2** right after the MPU is set (`chimera_board::bank2::boot_judge`). On `Good` it opens the door and checks it once (`probe(1) == 43`, the table's first and last bytes); anything less closes it and reports `corrupt`.
- **The MPU** keeps region 0 (DMA). Region 1 is 0x08100000, 1 MB, no access, XN, strongly-ordered, for good. Region 2 is 0x08100000, 512 KB, in one of four states: `Closed` (off), `Read` (read-only, XN, normal write-through; the boot's state and the one a closed bank 2 stays in, so the console's `flash` can still read S0), `Write` (read-write, XN, strongly-ordered) and `Open` (read-only, executable, normal write-through). Into `Open`, the D-cache over S0–S3 and the whole I-cache are invalidated first; every switch ends DSB, ISB. The console's `flash` no longer reads 0x08180000.
- **What testers see:** `status` adds `bank2 open <version> seal=<16 hex>`, `bank2 stale have <version>` or `bank2 empty|torn|foreign|corrupt|format <n>|unsealed`; SETTINGS › SYSTEM › ABOUT reads `OK` when open, else `NEEDS UPDATE` (owner decision 8).
- **The only writer is Chimera's own**, from the UI loop, from `/CHIMERA/SYSTEM/BANK2.BK2` staged by the console's `put`: erase S0 first, body, header, read back, commit word last. Never ROM DFU, never the stock bootloader.

## Alternatives considered
- **Data only in bank 2:** about 13 KB to move today, and the UI's fonts must stay in bank 1 to say `NEEDS UPDATE`.
- **Two images with a stable ABI:** `extern "C"` for every shared type, a second `core`/`libm` in bank 2, and Rust has no stable ABI for the engines' state.
- **ROM DFU or the stock bootloader as the writer:** the 2026-10-07 ROM DFU erase of bank 2 hung; the stock bootloader writes bank 1 only, and with a large `.bin` erases its own sector.
- **A version number instead of a seal:** two builds of one version can differ in any address bank 2 calls.
- **SHA-256 on the device:** the seal is a header field compared exactly, and the body's CRC proves the bytes; about 2 KB of SHA saved.

## Consequences
- Releases ship two files, `chimera-<v>.bin` (DFU, as before) and `chimera-<v>.bk2` (`put`, then the unit installs it). A firmware change that leaves bank 2's bytes alone keeps its seal: no reinstall.
- Bank 1 pays for the judge, the MPU states and the status text: about 2.8 KB.
- A stray load or fetch in bank 2 faults with a clear address instead of reading erased or torn flash; anything in 0x08180000+ always does.
- A double ECC error on a bank-2 read is a fault today; its recovery (a mark, one reset, S0 erased) is a later step of the plan.

## Sources
- `docs/superpowers/specs/2026-10-11-bank2-design.md`, `docs/superpowers/plans/2026-10-13-bank2.md`; issue #460.
- RM0433 (STM32H742/743/750/753) § 4 (embedded flash, read-while-write, ECC); ARMv7-M Architecture Reference Manual § B3.5 (MPU RASR: AP, TEX/C/B, XN, SIZE).
- ixox/preenfm3 `bootloader/Src/main.c` (bootloader 1.09's slots).
