# 0122. Render the wave tables at boot, not in flash

- **Status:** Proposed
- **Deciders:** owner (flash diet 2, 2026-10-11); firmware

## Context
Main with the sound library and VA is about 56 KB over the 917,504 B
firmware area (flash audit 2026-10-11). The ALGO wave tables (`WAVES`,
16 waves × 8 mips × 257 `i16`, 65,792 B) were rendered on the host by
`chimera-engine`'s build script and shipped as `.rodata`; the boot then
copied them into DTCM (ADR 0025) and the audio read only the copy. The
flash table was used once, at boot.

## Decision
1. `chimera-waves` is `no_std` (libm): its recipes and render are one
   pure function, `render_all(&mut Waves)`. The firmware (`boot-waves`
   feature, set by `chimera-board`) renders straight into the existing
   DTCM copy at boot (`waves::render_into`) and ships no `WAVES`. Hosts
   keep the static, written by the build script from the same function.
2. The render is bit-for-bit the table it replaced: a test pins the
   FNV-1a hash of the tables the old std generator wrote at 07160052.
   The arithmetic is IEEE f64 on both sides (no contraction; `VMLA.F64`
   rounds its product), libm's code on both, and nothing reaches the
   subnormals the boot's flush-to-zero would change.
3. The analysis turns one phasor per harmonic instead of calling sin and
   cos per term, and folds mirrored samples: about 4 M multiply-adds.
   The audio path is unchanged: the same DTCM tables through `ACTIVE`.

This replaces ADR 0025's "the flash tables stay the source"; the rest of
0025 (DTCM holds the tables and the stack) stands.

## Alternatives considered
- Store f32 spectra (16 KB) and render from them: −48 KB, and f32
  coefficients move samples by an LSB, so not bit-identical.
- Store mip 0 and truncate its DFT per mip: mip 0 is already quantised,
  so the other mips move.
- The old analysis on the device (one sin/cos per term, 15.6 M calls):
  seconds of boot.

## Consequences
- Flash −57.8 KB on every build.
- Boot ≈ 0.2–0.3 s longer (estimated from the loops' instruction mix;
  not measured on the unit).
- A recipe change now changes the firmware's boot work, and the hash
  test fails until it is re-pinned on purpose.
- No new RAM: the DTCM copy existed.

## Sources
- (private notes) (cut A),
  (private notes).
- ADR 0023 (the recipes), ADR 0025 (DTCM), ADR 0108 (profiles).
