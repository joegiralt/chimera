# 0271. Open the whole source under GPLv3 at 1.0

- **Status:** Accepted (2026-11-14)
- **Deciders:** owner

## Context
ADR 0144 (Proposed) planned to open only the shell at beta, under GPLv3 with a
linking exception, and keep the DSP engine, ORBIT and the factory Sounds
closed, to protect a plugin business built on the engine.

The owner has rethought it. What testers value is how Chimera sounds and how
easy it is to use within a very limited UX, and neither is taken by copying
code. Most building blocks are known techniques (TPT filters, phase
modulation, Karplus-Strong, the Rings and Beads pieces under MIT). The
bespoke parts are ORBIT, the UX system, and the circuit models and their
calibration. Those live in taste and tuning as much as in code. The owner
has said publicly that Chimera goes open source at 1.0
(https://www.elektronauts.com/t/preenfm3/138351/238).

## Decision
1. **Until 1.0, nothing changes.** Releases stay binary-only from
   `chimera-releases` under the tester licence.
2. **At 1.0, the whole repository opens under GPLv3 (or later)**: engine,
   ORBIT, UI, hardware, tools and factory Sounds. There is no split, no
   closed engine, no stub and no linking exception.
3. **The owner keeps the right to use their own code elsewhere**, including
   in closed products such as plugins. As sole copyright holder, they can
   license their own work again on any terms; the GPL binds only others.
4. **Outside contributions need a contributor agreement** that grants the
   owner the right to relicense, so the owner can still use a contributed
   part in a closed product. Without one, a contribution stays GPL-only and
   must be kept out of any closed product.
5. **Third-party code keeps its notices.** The MIT code (Rings, stmlib, Beads)
   is GPL-compatible. Its notice ships in the source and with every binary,
   as the release notes do now.

## Alternatives considered
- **ADR 0144's split (closed engine, open GPLv3 shell).** Rejected: it protects
  code that isn't the real moat, and costs a stub engine plus a linking
  exception to keep up.
- **MIT or Apache.** Rejected: forks could close the code. GPLv3 keeps
  forks open.
- **Stay closed.** Rejected: testers ask for the source, and the owner has
  committed to opening it at 1.0.

## Consequences
- Anyone can fork Chimera, port it, or build a plugin on its engine, as long
  as they ship their source under GPLv3.
- The owner's plugin plan stays open to them alone in closed form, as long as
  every contribution in a closed product came under the contributor
  agreement.
- Before 1.0: add licence headers or a top-level `LICENSE` (GPLv3), choose the
  contributor agreement, and check that every file's provenance is clean
  (ADR 0032, 0114).

## Sources
- ADR 0144 (superseded by this one), ADR 0032 (MODAL provenance), ADR 0114
  (curated DSP library)
- https://www.gnu.org/licenses/gpl-3.0.html
