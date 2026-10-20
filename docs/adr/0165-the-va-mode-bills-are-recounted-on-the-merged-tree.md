# 0165. Recount the VA MODE bills on the merged tree

- **Status:** Proposed
- **Deciders:** owner (ruling R11), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) point 10 (the bills), [0159](0159-va-fm-runs-osc-1-through-zero.md), [0161](0161-va-soft-sync-reverses-osc-2-at-osc-1s-wraps.md), [0162](0162-va-loopfm-loops-each-oscillator-in-closed-form.md), [0160](0160-va-phase-offsets-osc-1s-phase-by-osc-2.md) point 8 (their bills)

## Context
Each VA MODE was billed on its own branch, on the base it had then. `VaEngine::render`'s block work grew with every MODE, the oscillators moved behind the bank-2 door (ADR 0163) and SOFT SYNC's OSC 2 moved with them. The six MODEs merged (FM #658, AM #664, MAX #673, LOOPFM #678, SOFT SYNC #680, PHASE #689) and the committed bills no longer matched one count: SYNC, RING and BITS counted 550, 540, 540 against 540, 530, 530 committed.

## Decision
One recount of every MODE on `origin/main` (eb421021), by ADR 0118 point 9's method (`tools/loop_count.py`'s arithmetic, `bills2.py` with a role for each door instance and the MAX kink), rounded up to 10:

| MODE | count | bill | was |
|---|---|---|---|
| MIX | 525.4 | 530 | 530 |
| SYNC | 543.0 | **550** | 540 |
| RING | 532.4 | **540** | 530 |
| XOR | 544.8 | 550 | 550 |
| BITS | 532.5 | **540** | 530 |
| FM | 575.8 | **580** | 590 |
| AM | 534.3 | 540 | 540 |
| MAX | 584.4 | 590 | 590 |
| LOOPFM | 349.4 | 350 | 350 |
| SOFT SYNC | 586.9 | **590** | 570 |
| PHASE | 641.9 | 650 | 650 |

- `render`'s call through the door to LOOPFM's render is billed apart (it is LOOPFM's own, 350), so the base block no longer carries it: that is the 34 cycles the PHASE and MAX counts read high on the merged tree.
- FM falls 10: its count dropped from 585.7 with the shared door closure. SOFT SYNC rises 20: its OSC 2 through the door costs 232.5 (211.3 as a plain instance) — a counted difference, the closure and the jitter's call.
- PHASE stays 650 (its count 641.9 is under it).

## Alternatives considered
- Keep the committed bills and let SYNC, RING and BITS read 10 low. The bill is the budget's only input; a low bill steals voices from the CPU on the unit.
- Round to 5. The repo's counts round to 10 (ADR 0118).

## Consequences
A voice is the bill plus INIT's 161 (REV2, chain, routing). The voices on rev V / rev Y / beside ORBIT / with the master tape (`va_bills_each_mode`):

| MODE | rev V | rev Y | ORBIT | tape |
|---|---|---|---|---|
| MIX | 7 | 6 | 7 | 7 |
| SYNC | 7 | 5 | 7 | 7 |
| RING | 7 | **5** (was 6) | 7 | 7 |
| XOR | 7 | 5 | 7 | 7 |
| BITS | 7 | **5** (was 6) | 7 | 7 |
| FM | 7 | 5 | 7 | 6 |
| AM | 7 | 5 | 7 | 7 |
| MAX | 7 | 5 | 7 | 6 |
| LOOPFM | 8 | 8 | 8 | 8 |
| SOFT SYNC | 7 | 5 | 7 | **6** (was 7) |
| PHASE | 6 | 5 | 6 | 6 |

RING and BITS lose their sixth voice on rev Y: 4,186 / 701 = 5.97. Their counts are 2.4 and 2.5 over 530; a sixth voice there needs a bill of 530 and a voice of at most 697. ADR 0040's six voices on rev Y now holds in MIX and LOOPFM only.

## Sources
- Ruling R11; the counts: (private notes).
- ADR 0118 points 9 and 10, 0159, 0160, 0161, 0162, 0163, 0176.
