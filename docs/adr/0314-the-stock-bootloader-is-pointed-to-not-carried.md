# 0314. The stock bootloader is pointed to, not carried

- **Status:** Accepted (2026-11-22)
- **Deciders:** the owner (#94), 2026-11-21
- **Supersedes in part:** ADR 0266 (its host check read the repo copy `preenfm3-bootloader-backup.bin`)

## Context
The repo tracked `preenfm3-bootloader-backup.bin`, a 128 KB dump of an older stock PreenFM3 bootloader, 1.05 (Xavier Hosxe, GPLv3, https://github.com/Ixox/preenfm3). `docs/recovery.md` offered it as a restore image, and ADR 0266's ITCM step checked it on the host. #94 asked whether a GPL binary belongs in the repo.

## Decision
- The bootloader is Ixox's software, with Ixox's own update mechanism and instructions. Chimera rides on it and does not carry a copy: the binary is removed from the tree (it stays in the private history).
- `docs/recovery.md` points to the exact image Chimera expects, `p3_boot_1_09.bin` in the stock v1.03 release's `pfm3_firmware_1_03.zip` (https://github.com/Ixox/preenfm3/releases/tag/v1.03), the one the owner's unit runs, with the `bootloader/` source and Ixox's instructions for installing or restoring it. `just backup-bootloader` still reads a local copy off the user's own unit. The removed file was 1.05, older than what Chimera requires, so it was not a valid restore image for 1.09 anyway.
- Chimera expects bootloader 1.09 (the owner's rule). If Ixox releases a new bootloader (a PreenFM3 2.0 is expected), Chimera follows it: it is checked against Chimera on a unit, then the recovery page is updated.

## Consequences
- `THIRD_PARTY.md` no longer lists the file; `just release`'s private-file guard no longer names it.
- ADR 0266's bootloader check (no ITCMCR reference, no vector or stack in ITCM) read the repo copy; it is redone on any new bootloader from a unit backup or Ixox's release, not from a repo copy.
- A user who loses the bootloader and has no backup restores it from Ixox's release, as stock users do.
