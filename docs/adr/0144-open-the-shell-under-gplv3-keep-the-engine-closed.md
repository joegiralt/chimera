# 0144. Ship binary-only through alpha; at beta open the shell under GPLv3 with a linking exception and keep the engine closed

- **Status:** Proposed
- **Deciders:** owner

## Context
Chimera ships as binaries from the public `chimera-releases` repo under a
tester licence: use on your own PreenFM3, no redistribution, no reverse
engineering. The source repo is private.

Testers ask for the source. On the PreenFM3 thread on Elektronauts
(https://www.elektronauts.com/t/preenfm3/138351/192), one says whether they can
change the code affects whether they will test, and that the licence must be
chosen before anything is opened.

The owner's aims pull the other way:

- **Hard for others:** make it as hard as possible for others to take the work.
- **Easy for the owner:** keep the work as easy as possible to use and sell.
  The DSP is meant to become a product: plugins (CLAP/VST3) and firmware for
  other hardware.
- **The moat is the tuning:** the by-ear tuning and the engines are the
  valuable part, not the UI.

The owner is the sole copyright holder, so any licence binds only others. The
owner can relicense their own code at any time. They can't relicense
contributions they don't own.

## Decision
1. **Alpha stays binary-only** under the current tester licence. No source is
   published.
2. **At beta, source opens, split** along the existing crate line:
   - **Closed, all rights reserved:**
     - `chimera-engine`: the DSP, voices, FX and their tables.
     - ORBIT's runner and generators, wherever they live when the split is
       made.
     - The factory Sounds.

     Shipped only inside release binaries.
   - **Open, GPLv3 with a linking exception:** the shell.
     - The UI: `chimera-core` minus anything moved to the closed side.
     - Hardware: `chimera-hal`, `chimera-stm32`, `chimera-board`,
       `chimera-bootloader`.
     - The file formats: `chimera-fat`, `chimera-image`.
     - The desktop simulator, and the host tools in `chimera-utils`.
   - **A stub engine:** a small open engine with the same interface as
     `chimera-engine` (a plain oscillator, a filter, an envelope), so a public
     checkout builds and plays without the closed crate.
3. **GPLv3 for the shell, with an additional permission under section 7**:

   > As an additional permission under GNU GPL version 3 section 7, you may
   > combine this program with the Chimera DSP engine, under its own licence,
   > and convey the result.

   - Forks of the shell must stay GPLv3: stricter than MPL, which fits the aim.
   - The exception lets every release binary include the closed engine,
     whoever contributed to the shell. Without it, the first merged outside
     contribution would make shipping it beside the closed engine a breach
     of that contributor's GPL.
   - Forks can't ship the engine: they don't have it. They get the stub.
4. **No contributor licence agreement.** The exception covers the firmware,
   and the owner doesn't plan to reuse the shell in products. Open-side code
   the owner later wants in a product must be the owner's alone. The engine
   takes no outside contributions, so the owner may always reuse it anywhere,
   including closed plugins.
5. **The name and logo are protected separately** from the code. Before the
   first public source release:
   - check whether "Chimera" can be registered as a trademark for synth
     firmware and software;
   - add a trademark notice so a fork can't ship as Chimera.
6. **An IP lawyer reviews** the exception's wording and the trademark before
   anything opens.

## Alternatives considered
- **MPL-2.0 for the shell.**
  - Its per-file copyleft allows combining with the closed engine without an
    exception.
  - Rejected: forks could keep their own new files closed. GPLv3 with the
    exception is stricter on forks and still allows the closed engine.
- **GPLv3 for the shell, plus a CLA instead of the exception.**
  - Works too, but every contributor has to sign, and the owner has no plans
    to relicense the shell.
- **GPLv3 on everything, plus a CLA.**
  - The owner could still sell the code closed, under a second licence.
  - But the engines and their tuning become public. A GPL fork could ship the
    same sounds legally, on other hardware or as a plugin.
  - That gives away the moat.
- **Source-available (PolyForm Noncommercial, or BSL that turns into GPL after
  a few years).**
  - Others could read and change the code but not sell it.
  - Rejected: it isn't open source, the forum testers asking for source want
    real open source and will say so, and the engines' tuning would still be
    readable.
- **MIT or Apache on everything.**
  - Anyone could put the engines into closed products.
  - The worst fit for the owner's aims.
- **Binary-only forever.**
  - The strongest protection.
  - Kept for alpha (point 1), but it costs testers and outside fixes, and
    the shell isn't the moat.

## Consequences
- **The commercial asset stays closed.** The engine never leaves the owner's
  hands, so plugins and other hardware need no second licence for it.
- **The split must stay clean.** No engine code in open files, a stable
  engine interface, and a stub engine that keeps building. Code that crosses
  the line, such as ORBIT pieces in `chimera-core`, has to move before the
  split.
- **The open shell is useful.** Testers can fix the UI, the file formats and
  the hardware layer, and port the shell to other boards. They can't change
  the sound.
- **What a fork could do:** a fork of the shell can't use the name, and can't
  play the engines without the owner's binaries.
- **Release notes keep listing third-party components and licences**, as the
  tester licence promises. GPL-derived material, such as the stock PreenFM3
  bootloader, must stay out of the closed engine and out of releases.
- **This is a plan, not legal advice.** Point 6 checks it before anything
  opens.

## Sources
- `chimera-releases/LICENSE.md`: the tester licence.
- Elektronauts PreenFM3 thread, post 192 (2026-10-15).
- GPLv3, section 7 (additional permissions): https://www.gnu.org/licenses/gpl-3.0.html
- GPL FAQ on linking exceptions: https://www.gnu.org/licenses/gpl-faq.html#GPLIncompatibleLibs
- MPL-2.0: https://www.mozilla.org/en-US/MPL/2.0/
- PolyForm Noncommercial: https://polyformproject.org/licenses/noncommercial/1.0.0/
- Business Source License: https://mariadb.com/bsl11/
