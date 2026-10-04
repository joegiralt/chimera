# 0089. Ship the firmware with fat LTO in one codegen unit

- **Status:** Proposed
- **Deciders:** project owner (2026-10-04, on the bench below); firmware

## Context
The release profile was thin LTO with the default codegen units. Flash is
896 KB and every feature (ORBIT, the console, the bench) has to fit in it.
Flash deltas in ADRs 0082, 0083 and 0086 were already measured with fat
LTO and one codegen unit, set by environment variables on a git-archive
copy, because thin LTO moves by hundreds of bytes from noise. The shipped
build and the measured one were different builds.

The on-unit bench on #336 compared the two: `.text` drops from 812,108 B
to 771,036 B (−41,072 B, about 5 %) with fat LTO, and every audio row of
the bench is within ±1 % of thin LTO's.

## Decision
`[profile.release]` in the workspace `Cargo.toml` sets `lto = "fat"` and
`codegen-units = 1`, keeping `opt-level = 2` and `panic = "abort"`. Every
release build, `just firmware`, `just flash`, `just flash-bench`, `just
check` and the stack check, is this build. A flash measurement is the
plain release build's `text`: no environment variables, no git-archive
copy.

## Alternatives considered
- **Thin LTO, fat only for measuring:** the build measured is then not
  the build shipped, and thin's noise hides a few hundred bytes.
- **opt-level `s` or `z`:** smaller still, but the audio path's cost is
  what the voice counts are set from (ADR 0031); not measured here.
- **Fat LTO with the default 16 codegen units:** most of the gain, but
  one unit is what the earlier ADRs measured.

## Consequences
- About 41 KB more room in flash on the bench build; the release
  `--features orbit` build's `text` is 721,532 B at the commit that
  adopts this (thin LTO: about 788 KB).
- Release builds are slower: after a change to `chimera-core`, rebuilding
  `--features orbit` took 10.3 s against thin LTO's 4.7 s on the dev host.
  `just check` builds seven release feature sets and pays it each time.
- The audio cost is unchanged within ±1 %, so the voice counts of ADR
  0031 stand.
- The fat-LTO git-archive method in ADRs 0082, 0083 and 0086 is now just
  the release build; their numbers compare directly with release builds
  from here on.
- Fat LTO inlines across crates more freely: it pulled the project's
  build-up (six Sounds, the scenes) into `synth`, a 14.2 KB frame, which
  the stack check refuses. `shared::take_ui` is `#[inline(never)]` so it
  stays on its own frame; every feature set passes again.
- Debug builds are untouched.

## Sources
- private issue #336#issuecomment-5978819602
  (the on-unit bench, thin against fat LTO).
- ADRs 0082, 0083, 0086 (flash measured with fat LTO).
- The Cargo Book, "Profiles": `lto`, `codegen-units`.
