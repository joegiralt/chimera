# Third-party files

- `preenfm3-bootloader-backup.bin`
  - **What it is:** a backup dump of the stock PreenFM3 bootloader, the first 128 KB of flash, kept so the board can be restored.
  - **Licence:** GPLv3.
  - **Copyright:** Xavier Hosxe.
  - **Source:** https://github.com/Ixox/preenfm3
  - **Distribution:** it isn't part of Chimera's MIT-licensed code, and it's distributed under the GPLv3 terms, together with that source link.
  - **Status:** kept for now by the owner's choice (#94).
- `chimera-core/src/dsp/modal/` (`mod.rs`, `rings.rs`, `dispersion.rs`, `chords.rs`): see ADR 0032 for the Rings and stmlib (MIT) notice. `chords.rs` holds Rings' chord table (`dsp/part.cc`).
- Host-only crates (`chimera-desktop`, never linked into the firmware), used by the TAPE importer (TAPE spec § 6):
  - `hound` 3.5 (WAV reading): Apache-2.0. https://github.com/ruuda/hound
  - `rubato` 5.0 (sinc resampler): MIT OR Apache-2.0. https://github.com/HEnquist/rubato. It pulls in `audioadapter`, `audioadapter-buffers`, `audioadapter-sample` (MIT OR Apache-2.0), `windowfunctions` (MIT), `audio-codec-algorithms` (0BSD OR Apache-2.0), `visibility` (Zlib OR MIT OR Apache-2.0) and `num-traits`, `num-integer` (MIT OR Apache-2.0).
  - `rustfft` 6.4 (the transwave mips' FFT truncation): MIT OR Apache-2.0. https://github.com/ejmahler/RustFFT. It pulls in `num-complex`, `primal-check`, `strength_reduce`, `transpose` (MIT OR Apache-2.0), `num-traits`, `num-integer` and, to build, `autocfg` (MIT OR Apache-2.0).
