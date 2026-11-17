# 0283. Play VA's saw alone on its anchor, behind a bank-2 table of its own

- **Status:** Proposed
- **Deciders:** owner (0.3 efficiency: cycle wins sound-identical, bank 1 frozen), firmware

## Context
VA's plain oscillators (every MODE but FM, LOOPFM, SOFT SYNC and PHASE, which have renders of their own) play WAVE as a pair of two waves, crossfaded: sine–tri, tri–saw, saw–pulse. WAVE's default is ⅔, the saw's anchor, where the pair is saw–pulse with the pulse at weight 0. The pair's run still tracks and corrects the pulse's two edges every sample, for steps of ±0. DRUM's pulses already drop a weight-0 saw this way (`SawOff`, `off_kind!`, PR #994).

The pulse can't simply be dropped: when WAVE leaves the anchor, the pair's run must find the pulse's edges where they would have been, or the first block off the anchor differs. So the solo keeps the pulse's edges tracked, crossed and flipped, and only stops correcting them (`quiet_kind!`, `PulseQuiet`, `Kind::SILENT`).

The solo's runs are two new monomorphs of the run loop (OSC 1's recording, flipping instance and OSC 2's plain one). Bank 1 is frozen (ADR 0198), so they go to bank 2. Bank 2's ORBIT build has about 8.2 KB free in S0–S3 once #1008 lands, at ADR 0251's 8 KB floor, so they go to an S4 group (ADR 0272: new 0.4 work goes high), as `console::ed` does.

`Bank2Door`, the door's own table, can't take the entry: it must fit the 256 B before the board's table at `DOOR_ADDR + 0x100` (`size_of::<Bank2Door>() <= 0x100`, `chimera-engine/src/bank2.rs`), and in the ORBIT build it is exactly that (`bank2::DOOR`, 256 B at 0x08100100). Growing it would move every slot table after it (board, UI, bench, ORBIT, MIDI UI, ed), each pinned by an assert in `bank2.x` and read by bank 1 at its fixed address: a re-layout of every table for one 4 B entry.

## Decision
1. **WAVE held on the saw's anchor plays the saw alone.** `osc::Solo::at` (bank 1) answers `Some(Solo::Saw)` when WAVE's ramp is steady (`wave.1 == 0`) and the pulse's weight in the pair, `3·WAVE − 2` as the run reads it, is exactly 0. OSC 1 and OSC 2 (but as SYNC's slave) then play `VarOsc::render_solo::<Saw, PulseQuiet, …>` through the solos' table; anything else, or a closed door, plays the pair's runs as before.
2. **Sound-identical, proven.** `chimera-engine/tests/va_solo_door_test.rs` pins, at origin/0.4's `262039d9` and `807e740d` prints: MIX and SYNC held on each anchor at C2 and C6; MIX and SYNC with WAVE stepped on and off the anchors mid-note (a jump, a dwell, a stepped sweep, OSC 2 a third behind); RING, XOR, BITS, AM, MAX, FM, LOOPFM, SOFT SYNC and PHASE at BLEND 1 held on each anchor and moving. Door open, closed and absent: bit for bit. It also pins which blocks solo (`VaEngine::solos`).
3. **Only the saw's anchor.** The sine's solo measured no faster (on the first draft of this work, `engine-cost-va-solo`); the triangle's (⅓) and the pulse's (1) would each be two more run instances in flash, and are left to their pairs.
4. **Its own table, at the head of the S4 groups.** `osc::solo::SoloDoor` (magic 0xB2A5_0283, one entry) at `VA_SOLO_DOOR_ADDR` = 0x08180400, just after MIXER's 1 KB reserve, checked by the boot's door check (`solo::door_ok`). Its code, `va::osc::solo` (`*2va3osc4solo*`), is the S4 group after ed's; `check-door-placed` pins both to S4–S5. It is the first door table outside S0: a slot after ed's (`DOOR_ADDR + 0x400`) costs 128 B of S0–S3 and took ORBIT's S0–S3 to 8,160 B free, under ADR 0251's 8 KB.
5. **Bank 1 gains only the stub** (`Solo::at`, the table call) and shrinks: `oscs`, the two oscillators, is now one out-of-line copy for every MODE instead of one inlined into each. No bank-1 line moves (ADR 0198, 0223 not needed). Bank 2's three lines rise by their measured bytes (+3,040, ORBIT +3,072), naming this ADR.

## Measurements
Flash, `CHIMERA_STAMP_CLEAN=1` release builds, origin/0.4 `807e740d` against this change, every budget set:

| Sets | Bank 1 (B) | Bank 2 S0–S3 | Bank 2 S4–S5 |
|---|---|---|---|
| default, bank2-gate, fault-inject, bench-algo, bench-routing, bench-memory, bench-stress, bench-tape, filter-probes | −128 | 0 | +3,040 |
| orbit, orbit,usb-config-writer, bench-algo,master-tape, bench-orbit | −128 | 0 | +3,072 |

ORBIT's S0–S3 stays at 8,288 B free. S4–S5 holds the 8 B table at 0x08180400 and `solo::play` (3,036 B, both runs inlined) after ed's group, plus alignment. No `PulseQuiet` code links outside it.

Cost, host only so far (x86, release, `VaEngine::render` of one held C3 voice, oscillators and operator, best of 8 interleaved rounds of 40,000 blocks, STABILITY ½, OSC 2 a fifth up; door closed = the pair runs as before):

| Scene | Pair (ns/block) | Solo (ns/block) | |
|---|---|---|---|
| MIX, WAVE ⅔ | 646 | 552 | −15 % |
| RING, WAVE ⅔ | 665 | 545 | −18 % |
| SYNC, WAVE ⅔ (OSC 1 only) | 670 | 616 | −8 % |
| MIX, WAVE 0.7 (off the anchor) | 643 | 640 | noise |
| SYNC, WAVE 0.7 | 676 | 673 | noise |

The static count of the M7 loops (`tools/loop_count.py`) shows no shorter fast path, so the unit's saving may be smaller than the host's; it is unread. The unit reading is the bench's VA screen (`bench-algo`), whose MIX, RING and SYNC rows sit on WAVE's default, the saw's anchor, before and after this change. No row holds WAVE steady off an anchor.

## Alternatives considered
- **An entry in `Bank2Door`.** No room under the 256 B the board's table leaves it (above).
- **A slot after ed's in S0, as the other tables.** 128 B of S0–S3 for an 8 B table: ORBIT's S0–S3 8,288 → 8,160 B free, under the 8 KB floor, before #1008.
- **The solos in S0–S3 with VA's door code (`*3osc4door*`).** It would take S0–S3 under ADR 0251's floor once #1008 lands.
- **Drop the pulse outright (`off_kind!`).** Leaving the anchor would find its edges stale: not bit for bit.
- **Solos on every anchor.** Flash for anchors the default patch doesn't sit on; WAVE's default is the saw's.

## Consequences
- INIT VA, and any Sound whose WAVE sits at ⅔ unmodulated, renders each plain oscillator as one run of the saw, the pulse's edges tracked but never corrected. WAVE turned off ⅔ (its knob steps by 1/128, so ⅔ is the default only), or modulated, plays the pair as before.
- A door table in S4: the boot's door check reads 0x08180400, inside the sealed image, as it reads the S0 slots. S0's next slot stays `DOOR_ADDR + 0x400`.
- The quiet kind (`quiet_kind!`) is a pattern a later solo (another anchor, another engine's pair) can reuse.

## Sources
- ADR 0163 (VA's MOD oscillators in bank 2), 0198, 0223, 0251, 0272; PR #994 (DRUM's `SawOff`).
- `chimera-engine/src/dsp/va/osc.rs` (`Solo`, `quiet_kind!`, `render_solo`), `osc/solo.rs`, `dsp/va/mod.rs` (`oscs`, `solo`), `chimera-stm32/build.rs`.
