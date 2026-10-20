# 0179. Feed each filter's output back into its cutoff (FB), its loops in bank 2 (supersedes in part 0155)

- **Status:** Proposed
- **Deciders:** owner (2026-10-07 request, #393; 2026-10-20 "build it")

## Context

The Kobol Expander's 'Surfer' patch (#37) feeds the audio out back through the voltage processor into VCF FREQUENCY. A desktop demo (private notes, branch `filter-fb-demo`) found three things. FB ±8 octaves per unit of output is the reach that moves a pluck. The 2040 can top full scale pre-VCA at full FB and RES. A one-pole on the FB signal should hold the max-FB fizz on CLEAN, DIRTY and the 2040. A second copy of every TYPE's per-sample loop costs about 6 KB, and bank 1 already sits under 40 KB free. FLT › MOD's six cells were all taken.

## Decision

- **A param.** Filter param 14 is STAB (ADR 0168), and FB is filter param 15, ident `FB`. It is `Bi`, −1..1, neutral 0, glides as a knob and is modulatable, so it is also a MOD-matrix destination. Every TYPE reads it.
- **The loop.** Each sample's `g` becomes `g · 2^(8 · FB · y)` (DIRTY's `f` likewise), where `y` is the filter's own output one sample late, clamped to ±1, through a one-pole. The result is held to the type's FREQ-tap range. FB ramps across the block from the last block's value, so a route's step never zippers. The one-pole's coefficient is `1 − |FB| · (1 − a₂.₅ₖ)`: open at 0, 2.5 kHz at full FB.
- **The 2040's guard.** With FB on, the 2040's output leans into `sat_2040` by |FB|: `o + |FB| · (sat(o) − o)`. It is continuous in FB and holds the output within full scale at full FB and RES.
- **Paths.** FB off is today's code, bit for bit and cost for cost: no branch is added to any existing loop. FB on runs its own loop per type: CLEAN's and DIRTY's one loop for every mode (the mode matched a sample), LADDER's, and the 2040's per VOICE. Bank 1 builds the block's arrays (ramps, taps, mix) and calls the loop through the bank-2 door (`FedDoor`, as FUZZ's, ADR 0157). The loops live in each type's `fed` module, linked into `.bank2` by `*6filter*3fed*`. While the door is closed, the block runs its plain moving path, so FB is silent until bank 2 opens.
- **The bill.** With FB stored off 0 or routed, the voice pays the type's moving path (unless a route already bills it) plus `FB_COST`, 100. That is the costliest count (the 2040 EXP's loop, 105 → 166 instructions, × 1.606), billed for every type.
- **The page.** FLT › MOD reads FM1 · FM2 · FM3 · FB · QM, with F empty. FM3 IN and QM IN move to a new FLT › JACK page (id 98, after FX). FM3 IN sits in C and QM IN in E, under FM3's and QM's encoders on the page above. The jacks are patching, not sound. EDIT walks FLT → MOD → FX → JACK. This supersedes the FLT › MOD row of ADR 0155's layout table. Routes, maps and saved Sounds are addressed by param, so none move.
- **Memory.** The voice grows 24 B: the FB signal in CLEAN, DIRTY and the four-pole slot, the last block's amount, and the played FB.

## Alternatives considered

- **The FB math inside the existing moving loops behind a branch** (the demo): it costs every moving block 4 instructions with FB off, and bank-1 flash.
- **A copy of each loop in bank 1:** about 6 KB of the last 35 KB.
- **FB as a hidden gesture on FM3's cell:** MIX + turn is taken (landmarks), and the encoders have no push (no push encoders).
- **Dropping a jack cell:** loses patching that only the matrix could then do.
- **A zero-delay solve:** the explicit one-sample loop is what #393 asks for. At 48 kHz it is 7.5° of phase at 1 kHz on a cutoff-FM path.

## Consequences

FB is on every TYPE and in the matrix. With FB on, a voice costs the moving path plus 100: about one voice fewer on rev V for a VA Sound. Bank 1 grows 3,240 B (ORBIT, 34,376 B under the SD-flash limit) and bank 2 5,536 B. The jacks are one page further down. Goldens for FLT › MOD and `row_flt` are re-recorded.

## Sources

#393. The demo ledger (private notes, `acid/`). The Kobol Expander patch sheet #37. ADR 0155, 0157, 0168. The A/B renders in (private notes).
