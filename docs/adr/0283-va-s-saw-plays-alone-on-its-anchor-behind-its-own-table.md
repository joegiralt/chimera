# 0283. Play VA's saw alone on its anchor, behind a bank-2 table of its own

- **Status:** Accepted (2026-11-18)
- **Deciders:** owner (0.3 efficiency: cycle wins sound-identical, bank 1 frozen), firmware

## Context
VA's plain oscillators (every MODE but FM, LOOPFM, SOFT SYNC and PHASE, which have renders of their own) play WAVE as a pair of two waves, crossfaded: sine–tri, tri–saw, saw–pulse. WAVE's default is ⅔, the saw's anchor, where the pair is saw–pulse with the pulse at weight 0. The pair's run still tracks the pulse's two edges and corrects them at each crossing, for steps of ±0. DRUM's pulses already drop a weight-0 saw this way (`SawOff`, `off_kind!`, PR #994).

The pulse can't simply be dropped: when WAVE leaves the anchor, the pair's run must find the pulse's edges where they would have been, or the first block off the anchor differs. So the solo keeps the pulse's edges tracked, crossed and flipped, and only stops correcting them (`quiet_kind!`, `PulseQuiet`, `Edges::Silent`).

The pair's run lerps its constants from the ones the oscillator holds from its last run. LOOPFM renders without them, so a block back from LOOPFM can find the pulse's held constants at another weight while WAVE already sits on the anchor (MIX at WAVE 0.8, LOOPFM, MIX at ⅔: the first draft's solo differed there, −35 dB in one block). The solo is the pair's only when those held constants are the pulse's weight-0 ones too.

The solo's runs are two new monomorphs of the run loop (OSC 1's recording, flipping instance and OSC 2's plain one). Bank 1 is frozen (ADR 0198), so they go to bank 2. Bank 2's ORBIT build has about 8.2 KB free in S0–S3 once #1008 lands, at ADR 0251's 8 KB floor, so they go to an S4 group (ADR 0272: new 0.4 work goes high), as `console::ed` does.

`Bank2Door`, the door's own table, can't take the entry: it must fit the 256 B before the board's table at `DOOR_ADDR + 0x100` (`size_of::<Bank2Door>() <= 0x100`, `chimera-engine/src/bank2.rs`), and in the ORBIT build it is exactly that (`bank2::DOOR`, 256 B at 0x08100100). Growing it would move every slot table after it (board, UI, bench, ORBIT, MIDI UI, ed), each pinned by an assert in `bank2.x` and read by bank 1 at its fixed address: a re-layout of every table for one 4 B entry.

## Decision
1. **The block plays the saw alone only with its proof.** `VarOsc::on_anchor` (bank 1) takes the oscillator and the block's `Ramps` and gives back an `OnAnchor` (its own module, `osc::anchor`, private fields: `osc::solo` and `osc::door` can't build one), or the oscillator for its pair's runs. It holds when WAVE equals the saw's anchor (`Wave::anchor`, a `Unit`) at both ends of the block, so through it, and the oscillator's `HeldPair` (the pair and the WAVE its held constants were taken at) is not saw–pulse at any WAVE but the anchor. Anchor-ness is WAVE against the anchor, never a weight read as 0: a WAVE a ulp off the anchor plays the pair. The proof owns the oscillator until it plays or is released, so it plays only that one and nothing changes its held pair in between. OSC 1 and OSC 2 but as SYNC's slave play it through the solos' table: `run` takes a `PlainMode` (exactly MIX, SYNC, RING, XOR, BITS, AM, MAX) from its seven callers, which gives OSC 2's `Osc2Run`. Anything else, or a closed door, plays the pair's runs as before (`Played::Pair`).
   `on_anchor` admits a block whose WAVE ramp has a nonzero step too small to move it: both ends round to ⅔ exactly, so every sample between them does (the ramp is linear, its rounding monotone), and the pair's run would read weight 0 at every sample too, bit for bit. Nothing in the proof may depend on the ramp's `step` again; only WAVE's values decide.
2. **Sound-identical: the tests.** `chimera-engine/tests/va_solo_door_test.rs` pins, at origin/0.4's `262039d9` and `807e740d` prints: MIX and SYNC held on each anchor at C2 and C6; MIX and SYNC with WAVE stepped on and off the anchors mid-note; RING, XOR, BITS, AM, MAX, FM, LOOPFM, SOFT SYNC and PHASE at BLEND 1 held on each anchor and moving; and the scenes of `tests/va_scenes`: LOOPFM, FM, PHASE and SOFT SYNC detours from the solo and from off the anchor (WAVE onto it inside the detour or with MIX back), SYNC through LOOPFM, two Sounds browsed (MODE and WAVE in one block), glide, a steal onto the anchor, a restart, SHAPE swept and at its ends. Door open, closed (where bank 1 plays the MODE) and absent: bit for bit. It also pins what each block's plain renders played (`VaEngine::played`), and that the first block back from LOOPFM plays the pair. The identity evidence beyond them is `tests/va_solo_fuzz_test.rs`: 64 fuzzed scenes per build (with bank 2, its door toggled mid-note; and without bank 2), each pinned to the digest 807e740d played, solo and forced-pair runs alike, and 400 scenes each played with the solos and with every plain block forced to its pair, bit for bit.
3. **Only the saw's anchor.** The sine's solo measured no faster (on the first draft of this work, `engine-cost-va-solo`); the triangle's (⅓) and the pulse's (1) would each be two more run instances in flash, and are left to their pairs.
4. **Its own table, at the head of the S4 groups.** `osc::solo::SoloDoor` (magic 0xB2A5_0283, one entry) at `VA_SOLO_DOOR_ADDR` = `Slot::Mixer.base() + slot::RESERVE` (0x08180400), just after MIXER's 1 KB reserve, checked by the boot's door check (`solo::door_ok`). Its code, `va::osc::solo` (`*2va3osc4solo*`), is the S4 group after ed's; `check-door-placed` pins both to S4–S5 and fails on any `PulseQuiet` code outside bank 2. It is the first door table outside S0: a slot after ed's (`DOOR_ADDR + 0x400`) costs 128 B of S0–S3 and took ORBIT's S0–S3 to 8,160 B free, under ADR 0251's 8 KB.
5. **Bank 1 gains only the stub** (`on_anchor`, the table call) and shrinks: `oscs`, the two oscillators, is now one out-of-line copy for every MODE instead of one inlined into each. No bank-1 line moves (ADR 0198, 0223 not needed). Bank 2's three lines are re-based on their measured use (+2,656, ORBIT +2,688 over the lines before), naming this ADR.

## Measurements
Flash, `CHIMERA_STAMP_CLEAN=1` release builds, origin/0.4 `807e740d` against this change, every budget set:

| Sets | Bank 1 (B) | Bank 2 S0–S3 | Bank 2 S4–S5 |
|---|---|---|---|
| default, bank2-gate | −400 | −416 | +3,040 |
| orbit, orbit,usb-config-writer | −368 | −416 | +3,072 |
| bench-algo, bench-routing, bench-memory, bench-stress, bench-tape | −416 to −424 | −352 to −416 | +3,040 |
| bench-algo,master-tape, bench-orbit | −416, −408 | −416, −384 | +3,072 |
| fault-inject, filter-probes | −104 | −416, −384 | +3,040 |

ORBIT's S0–S3 goes 8,288 → 8,704 B free: the WAVE each run ends on is now read once (`Ramps::wave_at`, kept in `HeldPair`), which the plain and `MOD` runs share. S4–S5 holds the 8 B table at 0x08180400 and `solo::play` (both runs inlined) after ed's group, plus alignment. No `PulseQuiet` code links outside it. Bank 2's lines are re-based on the measured use: default and bank2-gate 498,272 B, orbit 526,432 B.

Cost, host only so far (x86, release, `VaEngine::render` of one held C3 voice, oscillators and operator, best of 8 interleaved rounds of 40,000 blocks, STABILITY ½, OSC 2 a fifth up; door closed = the pair runs as before):

| Scene | Pair (ns/block) | Solo (ns/block) | |
|---|---|---|---|
| MIX, WAVE ⅔ | 646 | 552 | −15 % |
| RING, WAVE ⅔ | 665 | 545 | −18 % |
| SYNC, WAVE ⅔ (OSC 1 only) | 670 | 616 | −8 % |
| MIX, WAVE 0.7 (off the anchor) | 643 | 640 | noise |
| SYNC, WAVE 0.7 | 676 | 673 | noise |

The static count of the M7 loops (`tools/loop_count.py`) shows no shorter fast path, so the unit's saving may be smaller than the host's; it is unread. The unit reads it on the bench's ROUTING screens (`bench-routing`): `VA` sits on WAVE's default, the saw's anchor, and `VA OFF` holds WAVE at 0.7, off every anchor (the pair's runs), in the same build; the VA screen (`bench-algo`) is full at 11 rows. Before and after this change, `VA` against `VA OFF` is the solo's saving.

## Alternatives considered
- **An entry in `Bank2Door`.** No room under the 256 B the board's table leaves it (above).
- **A slot after ed's in S0, as the other tables.** 128 B of S0–S3 for an 8 B table: ORBIT's S0–S3 8,288 → 8,160 B free in the first draft, under the 8 KB floor, before #1008.
- **The solo on WAVE's ramp alone (the first draft's `Solo::at`).** It ignored the held constants: after a LOOPFM block it played the saw alone while the pair would still be lerping the pulse home (−35 dB in one block).
- **The solos in S0–S3 with VA's door code (`*3osc4door*`).** It would take S0–S3 under ADR 0251's floor once #1008 lands.
- **Drop the pulse outright (`off_kind!`).** Leaving the anchor would find its edges stale: not bit for bit.
- **Solos on every anchor.** Flash for anchors the default patch doesn't sit on; WAVE's default is the saw's.

## Consequences
- INIT VA, and any Sound whose WAVE sits at ⅔ unmodulated, renders each plain oscillator as one run of the saw, the pulse's edges tracked but never corrected. WAVE turned off ⅔ (its knob steps by 1/128, so ⅔ is the default only), or modulated, plays the pair as before.
- A door table in S4: the boot's door check reads 0x08180400, inside the sealed image, as it reads the S0 slots. S0's next slot stays `DOOR_ADDR + 0x400`.
- The quiet kind (`quiet_kind!`) is a pattern a later solo (another anchor, another engine's pair) can reuse.

## Sources
- ADR 0163 (VA's MOD oscillators in bank 2), 0198, 0223, 0251, 0272; PR #994 (DRUM's `SawOff`).
- `chimera-engine/src/dsp/va/osc.rs` (`Pair::at`, `Pair::weight`, `Weight`, `HeldPair`, `Wave::anchor`, `Edges`, `quiet_kind!`), `osc/anchor.rs` (`OnAnchor`, `VarOsc::on_anchor`), `osc/solo.rs` (`SoloDoor`, `play`), `dsp/va/mod.rs` (`PlainMode`, `Osc2Run`, `plain`, `VaEngine::oscs`), `dsp/va/stability.rs` (`JitterScale`, `WrapJitter`), `chimera-stm32/build.rs`.
- Tests: `chimera-engine/tests/va_solo_door_test.rs`, `tests/va_scenes`, and `tests/va_solo_fuzz_test.rs` with `tests/va_fuzz` (64 scenes per build, with and without bank 2, pinned to their digests at 807e740d; 400 scenes each played with the solos and with every plain block forced to its pair, bit for bit; it fails on the first draft's flaw forced back in, on `piece`'s 0.25 → 0.26 and on the jitter ×1.001).
