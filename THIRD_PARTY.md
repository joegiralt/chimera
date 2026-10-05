# Third-party files

- `preenfm3-bootloader-backup.bin`
  - **What it is:** a backup dump of the stock PreenFM3 bootloader, the first 128 KB of flash, kept so the board can be restored.
  - **Licence:** GPLv3.
  - **Copyright:** Xavier Hosxe.
  - **Source:** https://github.com/Ixox/preenfm3
  - **Distribution:** it isn't part of Chimera's MIT-licensed code, and it's distributed under the GPLv3 terms, together with that source link.
  - **Status:** kept for now by the owner's choice (#94).
- `chimera-core/src/dsp/modal/` (`mod.rs`, `rings.rs`, `dispersion.rs`, `chords.rs`): see ADR 0032 for the Rings and stmlib (MIT) notice. `chords.rs` holds Rings' chord table (`dsp/part.cc`). The bank's staggered re-tune follows Elements' `dsp/resonator.cc` (MIT, same notice; ADR 0096).
- `chimera-core/src/dsp/mod.rs` (`fast_tanh`): stmlib's `SoftClip` (`dsp/dsp.h`, MIT, Copyright 2012 Emilie Gillet; ADR 0096).
