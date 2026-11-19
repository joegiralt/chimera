# 0290. `ed since` says the turns it refused

- **Status:** Accepted (2026-11-19)
- **Deciders:** firmware, the editor (editor-ui review fix round, I4)

## Context
ADR 0285 kept `ed set`'s answer `OK queued` for a set the box later refuses at its turn (TAPE's MODEL or DIRECTION on a streamed source, an operator kind or algorithm past the formant budget, a CHOKE onto a 9th voice group). The editor saw only the value not moving. The box says why in a toast; the editor showed nothing. The review asked that the editor show the box's own reason on the control (`editor-ui-fix-contract.md` § 6).

## Decision
- **`ed since` answers a `refused` line per turn the box refused since the last answer:** `refused <ADDR> "<reason>"`, after the `source` line and before `OK <m>`. The reason is the box's toast, word for word: `held::reason` (TAPE's `… NEEDS RAM · STREAMED`), `FORMANT BUDGET FULL`, or kit_groups' `9 GROUPS · KIT HOLDS 8`.
- **The newest four.** `EdState` keeps them (`edits::Refusals`, a fixed array, oldest dropped). An answer that writes them clears them; a `Measure` pass or a refused answer (`ERR ed since too large`, a malformed line) keeps them for the next.
- **A refusal at `ERR` time is said in that answer alone** (`ERR ed set <addr> <words>`, `ERR busy`): `since` never repeats it. A refused set to the value the Part holds stays quiet, as its toast does (ADR 0285).
- **CHOKE refuses before the turn**, in `edits::refusal` with the others, through `kit_groups::refuses` (bank 2 to bank 2, no door call): the same check and words as `edit_in`'s, so `turn` knows it was refused. Supersedes ADR 0285's "CHOKE in `edit_in`" for `ed set`; a CC's CHOKE still refuses there.
- **Bank placement.** All of it is in S4 (`.bank2_ed.edits.*`, kit_groups' module glob). The refusals are uninit past their count (`MaybeUninit`), so `EdState::new`, which bank 1 builds in place (`take_ed`), writes one byte for them.
- `PROTOCOL` stays 3: a new line kind the editor reads when it knows it.

## Alternatives considered
- **Repeat `ERR` refusals in `since`.** The set's own answer already carries them; the editor would show each twice.
- **A new `ed refused` ask.** One more round trip a poll, for what `since` already answers.
- **Keep every refusal.** Unbounded, or a cap that drops the newest; the editor needs the latest reason per control.
- **Ask kit_groups through the UI door (`Ask::Refuses`).** Measured +48 B in bank 1: a larger `Ask` copied by every bank-1 caller of `bank2::groups`.

## Consequences
- **Bank 1:** default 879,012 → 879,036 B (+24), the gate build 883,492 → 883,508 B (+16), ORBIT 893,524 → 893,548 B (under its line); fault-inject, bench-tape and filter-probes +24, bench-orbit +16, their lines raised by that: `EdState`'s layout moves `since`'s offsets in `Usb::service` and `tend_usb`, and the line's format pieces sit in bank 1's unnamed rodata.
- **Bank 2:** +576 B on default, ORBIT and the gate build, all in S4.
- **ITCM:** `EdState` +208 B.
- The editor (Elm) parses `refused` into its answer lines and shows the reason on the control until the next value.

## Sources
- `docs/adr/0285-editor-protocol-3-every-engine-pads-addressed.md`
- `chimera-core/src/console/ed/{edits,since}.rs`, `chimera-core/src/ui/kit_groups.rs`
- Tests: `console_ed_engines_test`'s `a_refused_turn_is_said_in_since_in_the_toasts_words`, `since_keeps_the_four_newest_refusals`, `since_says_only_the_turns_refusals`, `a_refusal_waits_for_a_since_answer`
