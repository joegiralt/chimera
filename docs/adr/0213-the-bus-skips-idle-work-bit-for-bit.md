# 0213. Skip the bus's idle work only where the result is known bit for bit

- **Status:** Proposed
- **Deciders:** owner (#793: "easy wins to make algorithms more efficient without changing the way they sound. Very important."), firmware

## Context
Under ADR 0061 every effect runs at MIX 0 and keeps its lines written, so a return brought back up plays what its send is doing now. #793's bus audit (private notes) found a lot of that work goes into producing exact zeros: a delay, chorus or reverb with nothing sent and its tail ended, and output pairs nothing is routed to. The owner's rule for 0.3 is absolute. The sound must not change: every golden, render hash and DAC word stays the same.

## Decision
- **A skip runs only where the skipped work's result is known exactly.** That means the same state and the same output bits, so the DAC words can't change. Each skip's test is the skipped code's own arithmetic on the values it would see. Wherever a skip can't prove that, the full block runs.
- **Exact zero is tracked, never assumed from levels.** A line counts the samples since it was last written anything but 0: `TapeDelay::zeros` (`MAX_DELAY_SAMPLES`), `JunoChorus::zeros` (+0 bits, `MAX_CHORUS_DELAY`) and QUAD's `Ring::zeros` (`RING_LEN`: each of its twelve lines' writes passes every slot of the shared ring in that many). Each counts a block at a time, ORing the block's writes, and saturates. The ring ORs only on a +0 send, through its own monomorph, so a sounding ring pays nothing for it.
- **The skips.** Each one advances its state exactly as the full block would:
  - **The TAPE delay** (`delay::tape::idle`): the send is all +0, the line is zero, the block is steady with no TIME fade, the head is finite, the tone's state is a fixed point of its one-pole on +0, and that state's saturated write is `to_line` 0. Then the transport steps each sample (`Transport::advance`, the phases and the jitter's one-poles without the sines), the write head moves 64, and the return is the send's own +0s.
  - **The chorus:** the send is all +0, both lines are all +0, and MIX and DEPTH are steady with a finite MIX. The return is `SILENT`. Each line's write head moves 64 and its LFO phase moves 64 × its step, both wrapping as 64 writes do.
  - **The reverb, every TYPE:** a +0 send on an all-+0 decimator history leaves `u` at +0. An all-+0 return on +0 low-pass states skips the tail gate's peak and low-pass. An all-+0 return on all-+0 interpolator histories gives `SILENT`, and the gain pass is skipped where the gain is finite and of + sign. At MIX × the tail gate's gain exactly 0 (`unheard`), with the return and the histories finite and under 1e30, the interpolators' histories take the block's last 17 and the return is `SILENT`.
  - **QUAD's ring:** no SIZE crossfade, a +0 send, zero lines, +0 DAMP states, and finite gains and DAMP. The taps are +0 and only the head moves 32.
  - **An output pair nothing is routed to** (not pair 1, which takes the FX return). `DacBlocks` knows which of each block's pairs hold only +0 (`zero`). The mix fills such a pair with +0 once and then skips it. The limiter skips its peak scan, since a +0 never raises the max, and its scaling, since +0 times a gain of + sign is +0. The ramp is monotone, so its two ends bound its sign. The conversion skips it where its DMA half already holds the 0 words (`engine::put`, one flag a pair and half). The compressor, or the master VOL and PAN off unity, scale every pair, so they clear the block's flags. Anything else that writes a block through `mix()` clears them too.
- **A zero's sign is invisible only where it is only ever added onto a sum that starts at +0.** The `unheard` return is `SILENT` where the full path gives `x × ±0`. Pair 1's sums start at +0 and are never −0, so adding ±0 onto them changes nothing. Any new non-additive use of the FX return before the pair sums (a divide, a `copysign`, a bit test) must drop `unheard` or keep its sign.
- **Proof.** `chimera-core/tests/idle_skip_test.rs` runs the whole engine on a score that plays, rings out for 10 s and wakes again, with Part 1 on pair 1 or pair 2, the reverb at MIX 0, the compressor on, and the master VOL and PAN off unity. It runs once with the skips and once with `dsp::NEVER_SKIP` (test-only), with the host's floats and under x86's FTZ and DAZ (the unit's FPSCR FZ), and compares every pair's DAC words and float bits block by block. A pair nothing is routed to must be exactly +0 throughout. It counts each skip (`dsp::SKIPS`, test-only) so no proof is empty. Breaking one (the transport one step short) fails it. Every golden and render hash passes unchanged, and none was re-recorded.

## Alternatives considered
- **A level threshold ("quiet enough"):** it changes the sound by the tail it cuts. That is the owner's ear's call, not an efficiency change.
- **Scanning a line for zeros when it might be idle:** QUAD's 46 KB arena is about 17,000 instructions in one block, a spike in the audio interrupt.
- **Skipping ROOM, HALL, BLOOM, GATE and REVERSE whole:** their eases, crossfades and GRAIN noise run inside each engine's `run`, and their f32 lines and one-poles can settle at a floor (a denormal on a host, the smallest normal under FZ), not at exact 0. Each would need its own split and its own proof (a #793 follow-up). Only the parts every TYPE shares (the decimator, the tail gate, the interpolators and the gain) skip for them.
- **Flushing QUAD's DAMP state to 0 after a silent ring length,** so it idles at high DAMP too: that is a sound change (about −90 dB), so it goes to the owner's ear.

## Consequences
- The counters add a word to `TapeDelay` and `JunoChorus` and a halfword to `Ring`. Bank 1 sees that only as layout. The effects' skip code is all bank 2's (ADR 0212). The pairs' skips are bank 1's (the mix, the limiter and the conversion, on the audio interrupt's path): about +190 B, paid for by the conversion running as one out-of-line loop for the three pairs (`engine::put`) where it was three inlined copies. Each set's bank 1 then sits 224 to 792 B under its line.
- An idle bus costs less, but the bill doesn't change. `FxBus::COST` is reserved every block, so the savings are headroom, not voices, until the bus is billed by what is awake (#793).
- QUAD never idles where its DAMP state holds ±1 LSB for good (high DAMP, `Ring::is_silent`'s note). That is correct, and it stays the full path.

## Sources
- #793; (private notes). ADR 0050 (the limiter), 0061, 0090 (the delay's i16 line), 0142, 0198, 0212.
- `chimera-engine/src/dsp/delay/tape.rs`, `chorus.rs`, `ring.rs`, `rev/mod.rs`, `halfband.rs`, `dsp/mod.rs` (`may_skip`, `all_pos_zero`, `Skip`); `chimera-core/tests/idle_skip_test.rs`.
