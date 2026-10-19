# 0151. A refused kind or algorithm is shown dimmed, with its reason

- **Status:** Proposed
- **Deciders:** owner (2026-10-17, #591)

## Context
ADR 0131 refuses an edit that would overspend the formant budget, and says
"WAVE's and ALG A/B's encoders step over refused kinds and algorithms, and
the cell toasts FORMANT BUDGET FULL". The toast fires only when every
target is refused, so a turn that merely skipped FRMT said nothing. The
owner asked why operator 3 could be a formant and operators 1 and 2 could
not: a FRMT other operators modulate charges 390, an unmodulated one 200.
The cell hid that rule, which breaks "every hold or combo says what it does"
and "nothing fails silently".

## Decision
This supersedes the encoder-stepping and toast sentences of 0131's "An edit
that would overspend is refused"; the refusal itself, the budget and the
load behaviour stand.

- **WAVE lists every kind on every operator**, FRMT and NOIS included.
  ALG A and B list every algorithm.
- **A turn lands on the code `delta` on**, clamped to the list's ends
  (`step_kind`, `step_alg`), whether the budget allows it or not. MIX +
  turn runs to the end.
- **An allowed target is written.** A refused one is held by the UI
  (`ui::held`): the cell shows its name dimmed (MID), and **nothing is
  written**. The next turn goes on from the held code, so FRMT in the
  middle of the list never blocks NOIS beyond it.
- **The focus band says why**, in `ALERT` in the hint's place and shown
  with HINTS off too, as it is an explanation:
  - `OP 1 MODULATED · OVER BUDGET`: the operator has a modulator and
    would fit unlinked (`Why::Modulated`).
  - `BUDGET FULL · OTHER FORMANTS`: the other formants have used it
    (`Why::Full`).
  - `LINKS A FORMANT · OVER BUDGET`: an ALG that would link into a formant
    the budget can't carry (`Why::Links`).
  Each is at most 216 px (`the_reasons_fit_the_band`).
- **The hold ends** at any button press, a turn on another
  cell, a page change or a load. It is UI state only: the Sound, the
  file and the engine never see it.
- **Unchanged:** the FORMANT BUDGET FULL toast on loads, and on a CC or a
  pushed turn that lands on a refused target, which have no cell to show it
  on. `Block::nudge` (CC, fine) lands on the target and the write is still
  refused by `try_set_kind` / `try_set_algs`.

## Alternatives considered
- **Keep stepping over, toast every skip:** a toast per turn is noise, and
  it names no cause.
- **Hide FRMT and NOIS on operators that can't take them:** the owner could
  not then learn that modulation is the cause.
- **A per-operator "FRMT OK" marker:** a second place to read the same fact.

## Consequences
- The list is the same length on every operator and every algorithm.
- `step_kind` and `step_alg` take the `from` code and return `refused`
  where they returned `skipped`.
- Goldens change only where a dimmed kind or a reason shows.

## Sources
- ADR 0131, 0086 (gesture hints); #591.
