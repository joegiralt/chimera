# 0322. Move the editor and the site to chimera-releases under GPLv3

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner

## Context
ADR 0271 keeps the whole repository binary-only until 1.0. The browser
editor (`editor/`) and the website (`site/`, on its own branch) lived here
under that rule. Neither holds DSP or firmware algorithms: the editor is Elm
UI that speaks the unit's `ed` console protocol over USB, and the site
renders the public guides. The firmware may later run on other machines, each
with its own editor, while users already go to the public `chimera-releases`
repository for releases, guides and the site.

The owner decided on 2026-11-22: "Do it. Editor is GPLv3."

## Decision
1. **The editor and the site move to `chimera-releases`** (public), as
   `editor/` and `site/` at its top level, imported fresh, without this
   repository's history. Each carries the GPLv3 text as its `LICENSE`
   (GPLv3 or later, as ADR 0271 chose for 1.0), copyright Joe Giralt.
   `editor/` leaves this repository; `site/` never merged here. Releases
   before 0.4 stay under the tester licence they shipped with
   (chimera-releases' `LICENSE.md`), as its `LICENSING.md` says.
2. **The firmware follows with 0.4.** The owner decided on 2026-11-22 that
   the firmware itself goes open source under GPLv3 or later with the 0.4
   release, with `chimera-releases` as its one public home. A pending ADR
   from the oss-prep task (`oss-prep-04`, `just oss-export`) records that
   and supersedes ADR 0271 for the firmware. Until then `chimera-screen`, the crate that draws
   the unit's screens for the editor, stays here, and its compiled
   `screen.wasm` goes public in the kit, under GPLv3 or later like the rest
   of the kit.
3. **The firmware ships an editor kit per release**: `just editor-kit
   <version>` writes `target/editor-kit/<version>/` with `screen.wasm`, the
   unit's protocol text the editor's tests read (`ed describe` of every
   engine, MIXER, SETTINGS, MAP and ORBIT; each engine's INIT panels; the
   engine list; a Part refusal) and a `KIT` stamp (version and commit). The
   editor builds against `editor/kit/<version>/` in `chimera-releases`,
   never against this tree.
4. **The protocol text is held here** in `chimera-core/tests/fixtures/editor/`,
   checked by the console tests that write it (`BLESS=1` rewrites them);
   `editor-kit` runs those tests before copying. `just check` no longer runs
   the editor's Elm tests; `chimera-releases` runs them against its kit.

This supersedes ADR 0271 for the editor and the site only.

## Alternatives considered
- **Keep the editor here until 1.0.** Rejected: it holds no algorithm worth
  closing, users are already in `chimera-releases`, and an editor per
  machine fits beside the releases better than inside one firmware tree.
- **Publish `chimera-screen`'s source with the editor.** Rejected: it is the
  firmware's own renderer and UI over a headless Part, and goes public with
  the rest of the firmware (the pending oss-prep ADR).
- **Keep the editor's history.** Rejected: the commit messages are private.

## Consequences
- A protocol change lands in two places: the firmware's fixtures here, and a
  new kit in `chimera-releases` with the editor change that reads it.
- `editor-sim` stays here (it runs the simulator); `chimera-releases`'
  `editor-e2e` talks to it.
- Contributions to the editor and the site come under the GPL; ADR 0271's
  contributor agreement still applies to anything the owner may want to use
  in a closed product.

## Sources
- ADR 0271 (binary-only until 1.0; superseded in part by this one and, for
  the firmware, by the pending oss-prep ADR), ADR 0289
  (the editor's protocol), ADR 0317 on the `site` branch (the site shows the
  editor from 0.4)
- https://www.gnu.org/licenses/gpl-3.0.html
