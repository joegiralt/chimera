# 0278. Put the Part sends on a squared taper

- **Status:** Proposed
- **Deciders:** owner (approved the change in #1004), Claude (law)

## Context
A Part's CHORUS, DELAY and REVERB sends (`OutParams::sends`, 0..1 on a
1/128 grid, shown 0–127) fed the FX bus as linear gain. Step 0 → 1 jumped
from off to −42 dB and each low step after it moved 6 dB, 3.5 dB…; a
tester heard the reverb snap in at the bottom of the knob
(joegiralt/chimera-releases#6, filed as private issue #1004). The
upper half of the knob also covered only the top 6 dB.

## Decision
The mixer feeds each send `send_gain(level) = level²` (`params::send_gain`,
a `const fn`, one multiply per send per Part per block in `mix_sided`),
then expression (ADR 0263) as before. The stored level, its grid, its
0–127 readout, MIDI CC and NRPN mapping are unchanged; only the gain they
make changes. The gain is still eased by `MixState` (20 ms one-pole,
ADR 0123): a turn never snaps.

| step | linear (old) | squared (this) | cubed | dB-linear, −60 dB floor |
|---:|---:|---:|---:|---:|
| 1 | −42.1 | −84.3 | −126.4 | −59.5 |
| 4 | −30.1 | −60.2 | −90.3 | −58.1 |
| 16 | −18.1 | −36.1 | −54.2 | −52.5 |
| 32 | −12.0 | −24.1 | −36.1 | −45.0 |
| 64 | −6.0 | −12.0 | −18.1 | −30.0 |
| 96 | −2.5 | −5.0 | −7.5 | −15.0 |
| 128 | 0 | 0 | 0 | 0 |

Floor: step 1 lands at −84 dB under the dry signal, under the reverb
return's own noise and far under a −60 dB audibility line for a send, so
0 → 1 is a fade-in, not an onset; the slope is zero at 0, so the low steps
grow from silence (−84, −72, −65, −60 dB) instead of jumping. Half way is
−12 dB, the common "audio taper" midpoint, so the top half of the knob
spans 12 dB instead of 6.

The factory Sounds keep their voicing: the three authored demos in
`factory.rs` (CHOIR, BREATH, VOX) and the 46 sends of the factory
library's recipes (`tools/factory-sounds/*.toml`, TINZ in
`tools/factory-tapes/recipes.toml`) move to the grid level whose square
is their old gain (0.35 → 76, 0.3 → 70, 0.25 → 64, 0.15 → 50 of 128),
each within 0.21 dB of before.

## Alternatives considered
- **dB-linear with a −60 dB floor:** the most even per-step loudness, but
  0 → 1 still jumps to −60 dB (audible on a loud reverb), half way sits
  at −30 dB so most sends people set today would fall 20+ dB, and it
  costs an `exp2` per send per block.
- **Cubed:** half way −18 dB and the bottom eighth of the knob below
  −54 dB: a dead zone a player has to turn through.
- **A 128-entry table:** no gain over one multiply, 512 B of flash, and
  the send is continuous (MIDI, NRPN and the editor write off-grid
  values), so it would need interpolation.

## Consequences
- Saved user Sounds and projects play their sends quieter except at 0
  and 127 (a send at 64 drops 6 dB). No legacy compatibility is kept: they load
  safely, they do not sound the same (owner rule).
- Goldens whose renders use a nonzero send below 1.0 change; those are
  regenerated and named in the PR.
- The send glyph and the FX flow viz still draw the stored level.

## Sources
- private issue #1004; tester report joegiralt/chimera-releases#6.
- ADR 0123 (sends eased by the mixer), ADR 0263 (expression).
