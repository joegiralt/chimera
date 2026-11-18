// ITCM's state region and free floor (ADR 0266, 0286), the one source:
// `hw` includes this file, and chimera-stm32's build.rs includes it too and
// writes `memory.x`'s ITCM region and `itcm.x`'s asserts from these values.

/// Where ITCM's state starts: clear of address 0.
pub const ITCM_ORIGIN: usize = 0x400;
/// ITCM's state, the 64 KiB less the first KiB: CPU-only, never a DMA's
/// (the kits, `ed`'s state, the audio's snapshots).
pub const ITCM_STATE: usize = 63 * 1024;
/// What the linker keeps free of it.
pub const ITCM_MARGIN: usize = 8 * 1024;
