# 0314. The stock bootloader is pointed to, not carried

- **Status:** Accepted (2026-11-22)
- **Deciders:** the owner (#94), 2026-11-21
- **Supersedes in part:** ADR 0266 (its host check read the repo copy `preenfm3-bootloader-backup.bin`)

## Context
The repo tracked `preenfm3-bootloader-backup.bin`, a 128 KB dump of the stock PreenFM3 bootloader 1.09 (Xavier Hosxe, GPLv3, https://github.com/Ixox/preenfm3). `docs/recovery.md` offered it as a restore image, and ADR 0266's ITCM step checked it on the host. #94 asked whether a GPL binary belongs in the repo.

## Decision
- The bootloader is Ixox's software, with Ixox's own update mechanism and instructions. Chimera rides on it and does not carry a copy: the binary is removed from the tree (it stays in the private history).
- `docs/recovery.md` points to Ixox's releases (https://github.com/Ixox/preenfm3/releases) and the `bootloader/` source, and to Ixox's instructions, for installing or restoring it. It keeps the SHA-256 of the 128 KB image a unit running 1.09 reads back (`ad8ef9800ce2a584c74831532f2258f811d2e7aaba7699c1c406c4aadf2a4124`) so a backup or a restore can be checked. `just backup-bootloader` still reads a local copy off the user's own unit.
- Chimera expects bootloader 1.09 (the owner's rule). If Ixox releases a new bootloader (a PreenFM3 2.0 is expected), Chimera follows it: it is checked against Chimera on a unit, then the recovery page and the hash are updated.

## Consequences
- `THIRD_PARTY.md` no longer lists the file; `just release`'s private-file guard no longer names it.
- ADR 0266's bootloader check (no ITCMCR reference, no vector or stack in ITCM) was done once on the 1.09 image; it is redone on any new bootloader from a unit backup or Ixox's release, not from a repo copy.
- A user who loses the bootloader and has no backup restores it from Ixox's release, as stock users do.
