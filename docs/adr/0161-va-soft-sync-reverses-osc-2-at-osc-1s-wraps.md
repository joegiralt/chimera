# 0161. Run VA MODE SOFT SYNC as OSC 2 reversing at OSC 1's wraps

- **Status:** Proposed
- **Deciders:** owner (the mode: the VA MODEs plan, 2026-10-19, issue #604), builder (the roles, ruling R2)
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 1 (SOFT SYNC joins the operators), 3 (its alias gate), 10 (its bill) and 12 (the master ceiling covers SOFT SYNC); builds on [0158](0158-va-oscillators-read-a-modulated-phase-beside-a-clean-clock.md)

## Context
Issue #604 asks for a reversing sync: where SYNC resets the following oscillator at the leader's wrap, SOFT SYNC reverses its direction there. The literature calls it reversing or mirrored sync (Roth et al., DAFx-26 #49): the follower runs forward over one of the leader's periods and back over the next, so the wave is the forward segment and its mirror. ADR 0158 built the turn into `VarOsc` (a `MOD` + `SYNC` render reads a mark as a turn: a BLAMP at the corner, `dir` flips, nothing resets). What was left was which oscillator leads, what BLEND does, the master ceiling and the bill.

#604's wording has the roles the other way round from SYNC ("when OSC2 wraps, OSC1 reverses"), and leaves BLEND to the builder ("how much the reverse is applied, or crossfades to MIX").

## Decision
1. **The MODE.** SOFT SYNC is ident `SOFTSYNC` (`VaMode::SoftSync`), shown `SOFT` (`SOFT SYNC` overruns the focus band's room left of the scope, `values_stay_clear_of_the_glyphs`; the plan's fallback). Disk code: reserved 8, assigned at merge (the branch carries it as 6 on top of FM, ruling R7). Appended; nothing renumbered.
2. **Roles as SYNC, not as #604's wording (ruling R2; for the owner).** OSC 1 leads and OSC 2 reverses at OSC 1's wraps, as SYNC's OSC 1 leads and OSC 2 resets. *Why:* the sounding period and the sub stay on OSC 1, exactly as in SYNC, so moving between SYNC and SOFT SYNC keeps the note, the sub and what CRSE2 means (the slave's brightness, not the pitch); and OSC 1's recording instance (`<…, true, false, true, false>`, the one every MODE shares) is reused, so the only new code is OSC 2's reversing instance. *If the owner wants #604's roles:* swap the two render calls and add one monomorph (OSC 2 recording, OSC 1 reversing with `FLIP`); the sub would then follow OSC 2's clock.
3. **The sounding period is two of OSC 1's.** Forward over one period, back over the next: at BLEND 1 (OSC 2 alone) the wave repeats every `2/f1`, an octave under OSC 1. That octave is already in VA as the sub; BLEND towards 1 brings it in the way SUB does. Measured: autocorrelation 0.9995 at `2/f1` and 0.51 at `1/f1` (A3, OSC 2 saw a fifth up); each second period is the first played backwards within 3.9·10⁻⁴ RMS around the turns that land on a sample.
4. **BLEND crossfades OSC 1 → OSC 2, as SYNC.** `a + (b − a)·x`, ends `1` and `2`. The issue's alternative, "how much reverse", has no meaning for a phase: a read either turns or does not, and half a turn is a stop. A crossfade to MIX would put the unsynced OSC 2 in the patch, which SYNC also does not do.
5. **SYNC's master ceiling applies.** OSC 1 stops at 7 kHz (`SYNC_MASTER_HZ`, 0118 point 12) while SOFT SYNC plays or is coming, as in SYNC, ramped in and out under the MODE duck. *Why:* uncapped, the turns bill at OSC 1's top rate 0.45 and SOFT SYNC counts 635.8 (bill 640, 738 a voice with INIT's +98): five voices on rev Y, under ADR 0040's six. At the ceiling it bills 570 (point 7). *Audibly:* as SYNC, a master asked above 7 kHz plays 7 kHz: OSC 1, and the sub on its clock, stop tracking pitch there.
6. **The scope.** MODE's glyph runs OSC 2's naive phase forward over OSC 1's first period and back over its second (`VA_MIX_RATIO`·`p1`, then `VA_MIX_RATIO`·(1 − `p1`)), through the same crossfade; at BLEND 1 its second half mirrors its first. Locked in `MODE_GOLDENS` (`soft_050`, `soft_100`).
7. **The bill** (amends 0118 point 10). By ADR 0118 point 9's count, with a role for OSC 2's reversing instance (`<64, false, true, false, true>`; its events: the clock's wrap, each edge crossed either way, the turn): OSC 1 the shared master at the 7 kHz ceiling 121.7 (SYNC's), OSC 2's reversing instance 211.3 (TRI/SAW: its clock's wrap 19.3 at 0.45; each edge's two sites, before a turn and any sample's, the dearer billed once at its dearest side with the crossing's divide, either way at 0.45 plus its own motion; a turn's base path 88.3, its BLAMP and the legs' updates, at the master's 7/48), operator and sub 36.2, block 67.5, crossings 30.0, LP24 95: **561.6, bill 570** (`COST_SOFTSYNC`). With INIT's +98 a voice is 668: 8 voices on rev V, 7 beside ORBIT, 7 with the master tape, **6 on rev Y**. The other MODEs' bills are unchanged.
8. **The alias gate** (amends 0118 point 3). `alias_db` with the half-pitch lattice (`f0 = f1/2`, the wave's period), notes 72 and 84, WAVE over `GRID`, SHAPE 0 and ½, BLEND 1, OSC 2 a fifth up: −45 dB everywhere, no exception. Worst −54.2 dB (84, saw); sine and triangle −77 dB or lower; the correction wins 25–35 dB over the uncorrected render.
9. **Softer than SYNC, where it can be.** Above 8 kHz a sine or triangle slave carries 18.6–27.9 dB less than SYNC's (CRSE2 +3, +5, +10): a turn is a corner, a reset a step. Two cases are not softer and are not gated: a saw or pulse slave (its own edges are steps either way: within 2.1 dB), and CRSE2 +7 exactly, where OSC 2 is half a cycle on at each wrap, so a sine's or triangle's reset from there is the same corner as a turn (within 0.1 dB).

## Alternatives considered
- **#604's roles (OSC 2 leads, OSC 1 reverses).** The sounding pitch and the sub would move to OSC 2, unlike SYNC; and it needs a second recording instance. Kept as the fallback in point 2.
- **BLEND as the amount of reverse, or a crossfade to MIX.** Point 4.
- **A reset with a reversed slope (a BLEP and a BLAMP).** That is hard sync's step plus a corner; a turn has no step, which is the whole of its softness.
- **No ceiling in SOFT SYNC.** OSC 1's full range, but five voices on rev Y (point 5).
- **A ceiling of its own.** Any value that holds six voices sits near SYNC's; one ceiling for both sync MODEs keeps the rule one sentence.

## Consequences
- OSC 2's reversing instance (`<64, false, true, false, true>`) is new code in bank 1: bank 1 free 51,724 → 41,876 B (−9,848 B) on `just firmware-orbit`, of which the instance is 8,640 B (its three runs 2,460–3,000 B each and its `render`) and `render_soft_sync` 1,042 B. Bank 1 now sits just above the ~40 KB line. Ruling R8 routes it behind the bank-2 door (ADR 0163, va-door), with FM's MOD instance; closed bank 2 makes SOFT SYNC silent and labelled like the reverb.
- At BLEND 1 the note sounds an octave under OSC 1 (the period is two of OSC 1's); BLEND ½ keeps OSC 1's fundamental in front.
- A saw slave sounds about as bright as in SYNC; the softness is in sine and triangle slaves.
- The bench gains a `SOFT SYNC` row (BLEND 1, OSC 2 a fifth up); its unit reading is pending.

## Sources
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Tasks 9 and 10); ruling R2 (the VA MODEs rulings: SOFT SYNC keeps SYNC's roles).
- Roth, Keller, Castañeda & Studer, "Alias-free oscillator synchronization via additive synthesis", DAFx-26 paper 49, 2026 (CC-BY 4.0): the definition of mirrored sync. Its additive method is not used; the turn is ADR 0158's BLAMP.
- Esqueda, Välimäki & Bilbao, "Rounding Corners with BLAMP", DAFx-16, 2016 (the turn's corner).
- ADR 0118 point 9 (the count), ADR 0158 (the read phase and the turn), ADR 0040 (six voices on rev Y).
