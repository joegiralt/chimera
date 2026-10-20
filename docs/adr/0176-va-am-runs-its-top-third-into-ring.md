# 0176. Run VA MODE AM's top third into ring

- **Status:** Proposed
- **Deciders:** owner (UAT 2026-10-19 and the A/B pick, issue #675), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 3 (AM's alias gate) and 10 (AM's bill)

## Context
VA MODE AM (#603, PR #664) had no ADR. It is unipolar AM, `y = k·a·((1 − x/2) + (x/2)·b̂)`, with `b̂ = b/pk.b` in −1..1, BLEND `x` the depth, and a level make-up `k` (`am_gain`). At BLEND 1 it is 100 % AM. On the unit the owner found it "really good on bass notes" but without "a very distinctive voice" (#675).

Three variants were rendered on the desktop for an A/B (private notes): bass hit, chord stab, lead line and a BLEND sweep, with OSC 2 at 1:1, a fifth and a tritone, level-matched:
- today's AM;
- **A**, today's range in BLEND 0–85, then on into ring;
- **B**, a Buchla/Serge-style offset AM that reaches 100 % AM at BLEND 32 and spends the rest over-modulated.

The owner picked A.

## Decision
1. **BLEND's reach.** `am_reach(x)` maps BLEND to the operator's AMOUNT on today's scale, `u`:
   - for `x` ≤ `AM_KNEE` = 85/127, `u = x·127/85`;
   - above it, `u = 1 + (x − AM_KNEE)·127/42`.

   The operator stays `k·a·((1 − u/2) + (u/2)·b̂)`. At BLEND 85 (`u` = 1) it is today's BLEND 127, 100 % AM. From 85 to 127 the offset `1 − u/2` falls to 0: the AM over-modulates, then becomes the ring `k·a·b̂` at 127. `|g| ≤ 1` for every `u` in 0..2, so the 0.9 peak cap holds unchanged.
2. **Today's AM, kept.** At BLEND ≤ 85, AM renders today's AM at BLEND `x·127/85`. `am_below_the_knee_is_todays` holds the engine to fixtures recorded from main before this change: 9 of 12 cases bit for bit, the rest within 1.5·10⁻⁸ (the mapping's rounding), asserted at 10⁻⁵. The per-sample loop is the same code: only the lane's ends are mapped, once a block.
3. **Click-free.** The BLEND ramp's two ends are mapped through `am_reach` once a block, and the lane runs straight between them, so a sweep through the knee is continuous (the slope changes from 1.49 to 3.02 per unit BLEND, with no step). The make-up `am_gain` takes BLEND and follows the same `u`. At `u` = 2 it is `pk.b/R`, the ring's level.
4. **The words and the scope.** BLEND's ends read `CLEAN` and `RING`. The glyph runs `combine`, which maps BLEND through `am_reach` itself, so AM's scope at BLEND 1 is RING's (`the_mode_glyph_differs_per_mode` expects that one pair equal).
5. **The alias gate** (amends 0118 point 3). `am_alias_gate`, C5 lattice, saw/pulse/triangle/sine:
   - **BLEND ≤ 85:** −45 dB wherever MIX passes. It keeps #660's exception at BLEND 85, saw × saw at the just fifth: −43.
   - **Above 85:** AM is held to RING's own figure at BLEND 1 for the same setting, `max(−45, RING + 0.5)`. RING at BLEND 1 is the product AM becomes, and it aliases the same way. Saw × saw at the just fifth reads −37.4 dB in both.
   - **Why not fix #660 here.** The fix is a polyBLAMP on the product's slope jump at every edge of either oscillator, `Δa·b′` and `Δb·a′`. It needs both oscillators' edge times and the other's slope a sample, in the operator loop. MAX's one-kink corrector costs 37.7 cycles a sample (COST_MAX). Two such correctors would take AM, and RING with it, past 570, against the 540 bill this change must keep. #660 stays open for AM and RING together.
6. **DC: the same as RING.** At unison the ring term `a·b̂` carries DC: at FINE2 0 it is constant, at FINE2 +6 it is a 1.6 Hz beat. Raw, at BLEND 127, it reads −15.7 dB re the RMS, against −23.4 dB for today's AM. AM does what RING does and leaves it to the voice's DC blocker (5 Hz, ADR 0060). `am_ring_dc_is_rings` holds AM's raw DC at BLEND 1 to RING's at BLEND 1, and AM's DC through the voice's blocker under −55 dB re the RMS (−59.3 measured, the window's own leakage). A per-voice trim in the operator would cost about 3 instructions a sample (a running mean and its subtraction). It would leave AM and RING handling DC differently.
7. **The bill** (amends 0118 point 10). The four per-sample loops of `render_am` count the same as before (`tools/loop_count.py`, release ELF). `render` is 54 instructions longer (985 → 1039), `am_reach` at the BLEND ramp's two ends and in `am_gain`, about half of it on AM's path a block: +0.7 cycles a sample, 532.9 → 533.6. **COST_AM stays 540**, and `cost_test`'s rooms keep their voices.

## Alternatives considered
- **B, the offset AM.** Over-modulation from BLEND 32 thins the bass 2–7 dB at settings people play bass at (C2 fundamental at BLEND 64–100). After the level make-up, any one-knob (offset, depth) path is the same family as A with a different taper, so a second knob would add nothing.
- **Today's curve unchanged, with a second MODE for ring-AM.** RING already exists. The owner asked for AM itself to gain a voice.
- **Ring reached through a smooth curve (no knee).** It would move today's AM off its BLEND values everywhere. The knee keeps 0–85 an exact rescale of today's.
- **A DC trim in the operator.** See point 6.

## Consequences
- AM keeps its bass below BLEND 85. A saved Sound's BLEND above 85 now plays over-modulated or ring (no legacy compatibility is owed).
- AM's top third aliases as RING does until #660 is fixed for both.
- AM at 127 has RING's DC and relies on the same voice DC blocker.

## Sources
- Issue #675 (owner UAT), #660 (the product's slope kinks), PR #664 (AM).
- The A/B: (private notes) and `measure.md`.
