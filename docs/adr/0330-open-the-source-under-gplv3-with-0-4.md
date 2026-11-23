# 0330. Open the source under GPLv3 with 0.4

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner
- **Supersedes in part:** ADR 0271 (its timing, § 1: "until 1.0, nothing changes")

## Context
ADR 0271 opens the whole repository under GPLv3 at 1.0. #1104 argues for
opening sooner: decompiling a binary with AI tools now recovers technique
anyway, and a public GPLv3 tree is a dated record of authorship that binds
anyone who copies it to open their work too. On 2026-11-22 the owner decided
to open the firmware with the 0.4 release. #1071 is the punch list from a
review of what the tree holds.

## Decision
1. **The source opens with 0.4,** under the GNU GPL version 3 or later
   (`LICENSE`, the GNU text unchanged; every crate's `license` is
   `GPL-3.0-or-later`). Everything else in ADR 0271 stands: the whole tree,
   no split, the author free to reuse their own code elsewhere, a
   contributor agreement for outside code.
2. **Read-only before 1.0:** no outside pull requests; issues are welcome
   (README § Contributions).
3. **The public tree is an export, not this repository.** `just oss-export
   <dir>` (`tools/oss_export.py`) writes HEAD's files without history,
   leaves out ledgers, reports, research notes, agent memory and build
   output, rewrites `/home/<user>/` paths to `~/`, and fails on any scan
   hit: a personal email, a token or key, a local hostname or home path, or
   a sample, patch bank or sysex dump.
4. **Every published source file carries a header:** the upstream authors'
   copyright lines where the file derives from theirs, then the project's
   copyright line and `SPDX-License-Identifier`. `just headers`
   (`tools/headers.py`) inserts and rewrites them from one constant each;
   `tools/test_headers.py` fails on a file without one.
5. **The reverb's emulator-derived values stay for 0.4.** ROOM, BLOOM and
   LOOP start from delay and coefficient values derived from
   thement/midiverb_emulator (GPL-3.0). The parameterizable engine built
   around them is original to Chimera. They are credited in `THIRD_PARTY.md` and in each file's
   header, and cannot go into a closed product until re-derived (filed
   for 0.5).

6. **Fresh history, one public home.** The public tree starts with one
   import commit of the export, authored by the owner's public GitHub
   identity; this repository keeps the full history and stays private. The
   public home is chimera-releases, beside its editor, site, guides and
   releases. The copyright line is `Copyright (C) 2026 Joe Giralt`.

   The firmware goes in `firmware/` there (the editor is chimera-releases'
   own, so the export drops `editor/`).
7. **The tracker stays private.** Public issues live in chimera-releases;
   the README says the source's `#nnn` numbers point to the private tracker.
8. **The design history is published.** `docs/superpowers/` (the specs
   and implementation plans) stays where it is, committed, and goes out
   with the rest: the links into it from the code and the ADRs resolve, and
   the export's path rewrite and scan cover it like any other file.
9. **Build output leaves the tree:** the root `bootloader.bin` (the jump
   stub's image, never read) is deleted.
10. **Release notes** name the GPL and link the source at the release's tag
    in chimera-releases; the tester licence line goes from 0.4 on.

## Alternatives considered
- **Wait for 1.0 (ADR 0271).** Rejected by the owner: closed binaries no
  longer hide technique, and an earlier public record is worth more.
- **Flip this repository public.** Rejected (#1071): 600+ branches, every PR
  and issue, agent plans and ledgers, and personal emails in the history.
- **MIT, as `Cargo.toml` declared.** Rejected by ADR 0271: forks could close
  the code; and the reverb values are GPL-3.0.

## Consequences
- Binaries built from 0.4 on are GPLv3 works: each release's notes link the
  corresponding source at its tag, so the source import lands in
  chimera-releases before the release is published.
- A closed product built on the engine must leave out or re-derive the
  reverb values and any contribution taken without the agreement.
- New files need a header: `just headers` writes it, `just check` fails
  without one.

## Sources
- ADR 0271, 0032 (MODAL provenance), 0095 (the reverb), 0114 (curated DSP)
- private issue #1071, /issues/1104
- https://www.gnu.org/licenses/gpl-3.0.html
