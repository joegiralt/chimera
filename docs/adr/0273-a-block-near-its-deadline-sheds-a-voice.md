# 0273. A block near its deadline sheds a voice

- **Status:** Proposed (owner approved the feature 2026-11-14: "add the safety net too")
- **Deciders:** owner, firmware

## Context
Voices are admitted by bill (ADR 0026, 0262). When a bill is under a voice's real cost nothing stops the audio. On the unit (2026-11-14, ce1f5243, INIT DRUM) DRUM was billed as ALGO (#951) while the open METAL HAT and TONE CYMBAL cost about 53 % of a block each. Eight overlapping hits overran the audio. Twelve kept the audio interrupt re-entering back to back, so the controls tick never kicked the IWDG and the unit reset (ADR 0034). Tasks 1 and 2 of the drum-perf plan make the kernels cheaper and the bill right. But the next under-bill, in any engine, would reset the unit the same way.

## Decision
- **Admission is unchanged.** Voices are still admitted, stolen and shed by bill. The net is only a backstop for a bill that is wrong.
- **The measure.** The audio interrupt times each half's `render_half` with the cycle counter (`chimera-stm32` `audio/dma.rs`), as `Cycles`. This is the whole block: voices, bus, ORBIT and USB audio, the same span the AUDIO page's load reads.
- **Only on a counter that counts.** The net (`perf::load::LoadNet`) is built only from a `clocks::CycleCounter`, which `clocks::cycle_counter` returns only when the DWT counts. Without one every span would read 0 and the net never fire, so there is no net, and the console's `status` says so: `load_net OFF` (`on` when built, `none` on the desktop).
- **The threshold.** A span over `BlockBudget::shed_at()` sheds: 95 % (`LOAD_SHED_PERCENT`, private to `hw.rs`) of the block's deadline, rounded down, worked out once when the net is built (`block_cycles`: 640,000 on rev V, 533,333 on rev Y). It does not use the 75 % audio share. ADR 0262 lets the unbilled fixed overhead use the UI's share, so a correctly billed worst block reads up to 81 % with one Part and about 89 % with six. A threshold on the share would fire on healthy loads. 95 % sits above every billed load and below the overrun. The margin is thinner on rev Y: the overhead is the same cycles over a smaller deadline, so a six-Part worst block, ADR 0262's 89,700 over the 400,000 share (75 % of 533,333), reads about 92 %. That leaves about 3 points under the net, against about 6 on rev V.
- **The response.** An overloaded block calls `Instrument::shed_for_load` (through `LoadNet::after`, the one function the firmware calls) before the next block. It sheds exactly one voice: the one a budget shed picks (`Allocator::shed_one`: non-mono first, the oldest released tail, then the newest held note) among the voices still sounding and not already fading. A booked voice gone silent (a DRUM hit ended with its key held) or already fading frees nothing, so the net neither sheds nor counts it. The voice fades out as any shed voice does (ADR 0027: `Voice::FADE`, two blocks, then reset and freed). A note waiting on it is dropped unheard and counted in `refused`. The net sheds one voice per overloaded block, so a burst keeps shedding until the load is back under 95 %.
- **Already fading.** A dying voice is never picked again (`shed_one` skips dying slots), so the net never sheds the same voice twice.
- **Nothing left to shed.** When no booked voice is both sounding and not dying, `shed_for_load` returns `false` and does nothing. The load is then the bus, ORBIT or fades, which the net cannot reduce. ADR 0034's watchdog remains the last line.
- **Counted.** Each voice the net sheds bumps `LOAD_SHEDS` (`chimera-board` `shared.rs`). The console's `stats` shows it as `load_sheds`, after `overruns`. A count above 0 means a bill is under a voice's real cost. The unit gate reads it.
- **Real-time.** The net allocates nothing and never blocks. On the audio side it costs two cycle-counter reads and a compare against the kept threshold per half and, only when overloaded, a scan of eight slots. The decision and its count are host-tested through `LoadNet::after` (`hw_test`, `instrument_test`, `drum_bill_test`). The firmware only measures and calls.

## Why the fade does not make it worse
A shed voice keeps rendering at its old cost through its two-block fade. Its reset (for MODAL, zeroing about 43 KB) lands in the block where the fade ends. So shedding does not lower the load for two blocks, and in the block where the fade ends it adds the reset. All of this is already true of every budget shed and every steal. ADR 0027 bounds it: at most the voices already sounding, for at most two blocks, and at most six resets in one block. The net adds no new voice and no new work. It only brings forward an end the voice would reach anyway. During a sustained overload the net can shed up to two more voices before the first fade frees its cycles (one in each overloaded block of that fade). That over-shed is accepted. The net exists to keep the unit from resetting, and two extra voices lost from a mis-billed burst cost less than an IWDG reset.

## Alternatives considered
- **Wait out each fade before the next shed.** Sheds fewer voices, but each step keeps an overloaded block running for two more blocks. A choke's or a steal's fade, which is not the net's, would also hold the net off. A burst that adds a voice a block would outrun it. Rejected: the owner chose one shed per overloaded block.
- **Cut the voice instead of fading it.** Frees its cycles at once but clicks (ADR 0027's reason for fades).
- **Threshold on the 75 % audio share.** Would fire on ADR 0262's healthy worst blocks (81–89 % of the deadline).
- **Shed the voice that measures costliest.** No per-voice cycle count exists on the audio path, and adding one costs cycles every block. The bill order already takes tails first.
- **Trust the bills alone.** That was the state before this ADR, and it reset the unit when a bill was wrong.

## Consequences
- A wrong bill now costs voices that fade out instead of a reset. The overloaded blocks may still overrun and click before the shed takes effect: the net bounds an overload, it does not prevent its first blocks.
- `load_sheds` makes under-billing visible on the unit without a probe build.
- A single healthy block that spikes past 95 % (no billed load does, per ADR 0262) would shed one voice. `load_sheds` shows it if it ever happens.
- The console's `stats` gains a line (the usb-console spec's `stats` block, which `console_answer_test` holds the answer to), and `status` the `load_net` line (`console_status_test`).
- Bank 1 (ORBIT): +496 B (ADR 0251).

## Sources
- `docs/superpowers/plans/2026-11-14-drum-perf.md` (Task 3); #951.
- ADR 0027 (shed order and fade), 0034 (IWDG), 0124 (75 % share), 0262 (worst billed block 81–89 % of the deadline).
- `chimera-engine/src/hw.rs` (`BlockBudget::shed_at`), `chimera-engine/src/perf/load.rs` (`LoadNet`), `chimera-engine/src/voice_alloc.rs` (`shed_one`), `chimera-engine/src/instrument.rs` (`shed_for_load`), `chimera-board/src/clocks.rs` (`CycleCounter`), `chimera-stm32/src/audio/dma.rs` and `audio/engine.rs`.
