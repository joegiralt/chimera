# 0124. Audio gets 75% of the CPU

- **Status:** Accepted (2026-10-11); supersedes in part [0020](0020-audio-clocking-and-output.md) (its "70 %" figure for `SampleBudget` and `BlockBudget`); amends [0027](0027-shedding-fades-tails-first.md), [0054](0054-sympathetic-strings-from-a-shared-pool.md) and [0085](0085-the-master-reverb-picks-a-type.md) where they quote 70 %
- **Deciders:** project owner

## Context
`AUDIO_BUDGET_PERCENT` was 70 since ADR 0020: 30 % left for the UI, MIDI and
interrupts. The owner asked whether audio could have more. The share was run
on the unit (PreenFM3, rev V, 480 MHz) at 70, 75 and 80 %, with the STRESS
bench and with normal firmware padded so the audio sat at its budget in every
block, real notes held on top (ledger
(private notes)):

- Audio load tracked the share (75 / 75 / 75 at 75 %); no overruns, drops,
  desyncs, DMA errors, watchdog resets or USB console loss at any share.
- MIDI onset jitter was unchanged, plus or minus one audio block (1.33 ms): the
  DIN interrupt fills a queue the audio block drains, and the UI loop is not
  on that path.
- The UI loop takes longer in proportion to the share it is left: average
  1.0x at 70 %, 1.2x at 75 % (about 1.2 ms, peaks under 2.4 ms), 1.5x at 80 %.
  The console round trip went from 2 ms to under 6 ms at 75 %.
- STRESS worst blocks (budget 480,000 at 75 %): all rows under except one HALL
  row at 482,532 (0.5 % over). At 70 % the worst blocks already ran about 4.5 %
  over (468,053 against 448,000): that is the bill under-counting HALL, BLOOM
  and MOVE, not the share.
- Voices bought at 75 %, with ORBIT stopped / playing: ALGO factory 0-3 6 to 7
  while ORBIT plays, MORPH 6 to 7 / 5 to 6, MODAL BOWED 6 to 7 and SYMPATHETIC
  7 to 8 while ORBIT plays, TAPE nothing (the pool cap of 8 binds).

## Decision
- `AUDIO_BUDGET_PERCENT` is **75**: 7,500 cycles a sample and 480,000 a
  64-sample block on rev V; 6,250 and 400,000 on rev Y. Everything is derived
  from the constant (`SampleBudget::for_cpu`, `BlockBudget::for_cpu`, the
  AUDIO page's budget mark, the bench's budget lines).
- **Admission still bills worst case.** The allocator prices each voice by
  patch shape, and the bus and ORBIT reserves, exactly as before; only the
  budget it fills grew. No shortcut for typical load.
- **80 % waits.** It also ran clean, but gains one more voice only on ALGO 0-3
  with ORBIT playing, and the UI's own worst cases (project load, SD save,
  full-screen page switches) could not be triggered from the console. 80 % is
  considered only after those are timed at a 20 % share.

## Alternatives considered
- **Stay at 70 %.** Costs voices the owner wants for no measured benefit: the
  overshoot in STRESS is the bill's, and 70 % did not prevent it.
- **80 %.** Not yet: 1.5x UI slowdown, one extra voice in one case, and the
  unmeasured load/save timing above.
- **Raise it only for rev V.** Rejected: one constant, both revisions; rev Y
  gains the same proportion.

## Consequences
- More voices on most ALGO factory Sounds and MODAL BOWED / SYMPATHETIC; the
  `cost_test` voice fits, the `hw_test` budget cycles (7,500 and 6,250 a sample;
  480,000 and 399,999 a block) and the tests that build a budget from a
  voice share follow the constant.
- The UI runs about 1.2x slower under full audio load; MIDI timing and USB are
  unchanged.
- ADR 0020, 0027, 0054 and 0085 quote 70 %, and 0020's text is left as
  written; the constant in `hw.rs` and this ADR are the record.
- Reopen when a UI worst case measured at 25 % left misses its deadline.

## Sources
- (private notes) and its `-raw` data.
- `chimera-engine/src/hw.rs`, `chimera-core/tests/cost_test.rs`.
