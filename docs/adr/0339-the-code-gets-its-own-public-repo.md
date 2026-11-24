<!-- Copyright (C) 2026 Joe Giralt -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# 0339. The code gets its own public repo, joegiralt/chimera

- **Status:** Accepted (2026-11-24)
- **Deciders:** owner
- **Supersedes in part:** ADR 0330 (§ 6, the public home and `firmware/`;
  § 10, where the release notes link the source)

## Context
ADR 0330 opens the source with 0.4 and puts the export in a `firmware/`
folder of chimera-releases, beside the editor, the site and the guides. A
folder in a release repo is a poor home for code. People need to fork the
code, open pull requests against it and see CI run on it, and a fork of
chimera-releases would carry every download, guide and screenshot with it.

Releases are also per machine. Chimera runs on the PreenFM3 today and is
meant to run on other machines later (the 1010 Blackbox among them). Each
machine's releases carry their own version numbers, so one code tree feeds
several release lines, and the code cannot share its tags with any one of
them.

The owner decided on 2026-11-24 that the code gets its own public repo.

## Decision
1. **The code's public home is `joegiralt/chimera`**, a public repo that
   holds `just oss-export`'s tree at its root: the Cargo workspace, the
   Justfile, `docs/`, `tools/`, `LICENSE` and `THIRD_PARTY.md`, the
   `README.md` and `CONTRIBUTING.md` written for it, and its CI. It
   starts with one import commit of the export, authored by the owner's
   public GitHub identity, as ADR 0330 § 6 said for `firmware/`.
2. **The name and the rename.** On the 0.4 release day this private repo
   is renamed `joegiralt/chimera-private`, and then the public repo is
   created as `joegiralt/chimera`. The rename comes first: GitHub
   redirects a renamed repo's old name only until a new repo takes it.
   The export writes the tracker's links (`github.com/joegiralt/chimera/issues/…`
   and `/pull/…`, and `joegiralt/chimera#…`) as plain text, "private
   issue #N" (or PR), so none of them points at the public repo's own
   issues. The few a user reads (`docs/recovery.md`) say so in the source.
3. **Releases stay per machine in chimera-releases**: the downloads, the
   user guide, the screenshots, the editor and the site. Each machine's
   releases are versioned on their own. The code repo tags the commit a
   release was built from as `<machine>-v<version>` (`preenfm3-v0.4.0`),
   and the release notes link the source, `LICENSE` and `THIRD_PARTY.md`
   at that tag in `joegiralt/chimera` (GPLv3 § 6).
4. **Development moves to the public repo after 0.4.** Once 0.4 ships,
   `joegiralt/chimera` is the source of truth and new work lands there.
   `chimera-private` keeps the old history and the private tracker, and
   takes no new code.
5. **The tracker stays private for now.** Follow-ups and known issues stay
   in `chimera-private`'s tracker; the `#nnn` numbers in the code and the
   ADRs point there. The README and the exported `CLAUDE.md` name that one
   tracker, `joegiralt/chimera-private`. Public bug reports and discussion
   go to chimera-releases' issues, where the testers already are. The
   public repo turns its own Issues off until the owner opens them.
6. **Read-only until a CLA.** As ADR 0330 § 2: no outside pull requests
   before a contributor licence agreement is in place, before 1.0.
   `CONTRIBUTING.md` says so, with the code rules a contributor needs.
7. **CI runs on the public repo**, on GitHub's public runners, with no
   secrets (`.github/workflows/ci.yml`, kept here as
   `tools/oss-root/.github/workflows/ci.yml` so it never runs on this
   repo). Pushes to `main` and pull requests run `just check` in parts, a
   job each: the tools' tests with the header and export scans, rustfmt,
   the host tests, loom, clippy, and the shipping firmware sets with their
   bank budgets (`check-tools`, `check-host`, `loom`, `clippy-main`,
   `check-firmware`). The slow parts run nightly and on demand: every
   bench build, their clippy, the release binaries and the stack check
   (`check-bench`, `clippy-bench`, `check-release`). The actions are
   pinned by commit, cargo is cached (the registry and the dependencies'
   builds, saved from `main` only), and the nightly toolchain is pinned
   once, in `rust-toolchain.toml`, to the one the bank budgets were
   measured with. Dev and test builds carry line tables only
   (`[profile.dev] debug = "line-tables-only"` in `Cargo.toml`); CI
   builds them with none and std's stripped (`CARGO_PROFILE_DEV_DEBUG=0`,
   `CARGO_PROFILE_DEV_STRIP=debuginfo`), since the host tests' target
   measured 25 GB with line tables, past a runner's disk.

## Alternatives considered
- **`firmware/` in chimera-releases (ADR 0330 § 6).** Rejected by the
  owner: forks, pull requests and CI belong to a code repo, and a fork
  would drag the releases along.
- **Keep the name `joegiralt/chimera` for the private repo** and give the
  public one another name. Rejected: the public code is the project, and
  it gets the project's name.
- **Flip this repository public.** Still rejected, as in ADR 0330: its
  history, branches, PRs and issues stay private.
- **Public issues in the code repo now.** Deferred: the `#nnn` numbers in
  the tree are the private tracker's, and a second numbering beside them
  would collide.

## Consequences
- The release notes link `joegiralt/chimera` at `preenfm3-v<version>`;
  that tag must be pushed before the release is published.
- After the rename, links in this repo's own tree to
  `joegiralt/chimera/issues/…` point at the public repo; the export
  makes them plain text, the private tree keeps them.
- The export also drops what only resolves here, in the docs: links to
  private claude.ai pages, and paths into private notes (private notes,
  (private notes)), each becoming "(private notes)".
- The export strips the agent banner that opens each plan and the plans'
  rule against attribution in commit messages; the README says plainly
  that Chimera is AI-assisted. Records written while the workspace was
  MIT say so as history ("was then MIT").
- The bank budgets hold on CI only with the pinned nightly. A toolchain
  bump re-measures the budgets in the same change.
- `just check` is now `check-tools`, `check-host`, `check-firmware`,
  `check-bench`, `clippy`, `loom`, `cargo fmt` and `check-release`, in
  that order, so CI and a local check run the same commands.

## Sources
- ADR 0330, 0271, 0322
- private PR #1113 (the export and its
  runbook)
- https://docs.github.com/en/repositories/creating-and-managing-repositories/renaming-a-repository
