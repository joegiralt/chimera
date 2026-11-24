<!-- Copyright (C) 2026 Joe Giralt -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Contributing to Chimera

## Pull requests

Chimera is read-only for now. It takes no outside pull requests until a
contributor licence agreement (CLA) is in place, before 1.0. Every change
is still tested on the hardware by one person, and code from outside
needs the CLA first.

You are free to fork it and change it under the GPL (`LICENSE`).

## Bugs and questions

Open an issue in
[chimera-releases](https://github.com/joegiralt/chimera-releases/issues).
A bug report helps most with:

- the firmware version, from SETTINGS › SYSTEM › ABOUT;
- the steps that trigger the bug;
- what you expected, and what happened instead;
- a photo, screenshot or short recording, if you can.

## Code rules

These hold for every change, and the checks enforce most of them.

- No `unsafe` without a `// SAFETY:` comment saying why it is sound.
- No heap allocation in the audio callback. The audio thread never
  blocks and never allocates.
- No libc.
- Parameter changes are smoothed, never snapped.
- Encode invariants in types (newtypes for units and ranges, enums over
  magic numbers) before reaching for a runtime check.
- Every source file carries the copyright and SPDX header
  (`just headers` writes it).
- A decision that constrains future work gets an ADR in `docs/adr/`,
  from `docs/adr/0000-template.md`, with a row in `docs/adr/README.md`.
  An accepted ADR is never edited: a new one supersedes it.
- Bank 1 of the firmware is frozen: new features go to bank 2, and
  `just check` holds each build to its budget.
- `just check` passes: tests, clippy with `-D warnings`, `cargo fmt`,
  the header and export scans, and the firmware builds.
