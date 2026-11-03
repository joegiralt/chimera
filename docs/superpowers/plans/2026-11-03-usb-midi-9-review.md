# PR #844 USB MIDI: independent final review

Base 6808f323 → head 8399e05c, read at (private notes). Read-only: nothing was built or run. Paths are relative to the worktree.

## Scores

| Dimension | Score |
|---|---|
| 1. Code quality | 7 |
| 2. Efficiency | 8 |
| 3. Performance | 7 |
| 4. Principled-ness | 7 |
| 5. Correctness and tests | 7 |

**1. Code quality: 7.** The types carry real invariants. `Link` (`chimera-board/src/usb.rs:953`) can only come from `install`, and `with` borrows it `&mut`, so a hold can't nest. `Closed` (`usb_midi.rs:65`) hands bank 1 an endpoint address and never the class, so the closed path can't reach bank 2's descriptor shim. `Cable`, `Cycles` (`chimera-hal/src/units.rs`), `Port` and `Left` (`chimera-engine/src/midi_in.rs`) close off bad values. The guard's logic is pure and lives in core (`chimera-core/src/console/shell.rs:144-171`), with a thin board shell around it (`usb.rs:1063-1092`). `OutPorts`/`Tee` (`chimera-engine/src/out_ring.rs:128-203`) turn the per-port echo into data. What holds it back:
- `Errand`'s proof is not sealed (`chimera-core/src/errand.rs:12`, see M1).
- USB MIDI's mutable state sits in a separate `static mut IN` (`usb_midi.rs:132`). The ADR's own rule says new state goes in the cell.
- `Midi::poll` encodes three states as `Result<(), Option<Closed>>` (`usb_midi.rs:77-99`).
- The most stateful new code, `door::send`'s held-packet buffer and packer (`usb_midi.rs:292-324`) and the Configured-edge detector (`usb_midi.rs:259-273`), sits in the target-only board crate instead of a host-testable core.
- `Usb::service` is cfg-heavy enough that the destructure at `usb.rs:434-450` is hard to read.

**2. Efficiency: 8.** Idle cost is small:
- About four NVIC mask/unmask holds per UI lap: `host_step` (`usb.rs:1253`), `service` (`usb.rs:455`) and the errand's two (`usb.rs:969,972`).
- One redundant `poll_all` per lap (the handler also polls). It is needed for the `held` case.
- `door::send` returns before doing anything when the ring and the held packets are empty (`usb_midi.rs:294`).
- Control rings are drained in one tagged pass with one budget (`midi_in.rs` `CtrlPorts::drain`).

The waste is small:
- The card-work errand runs every lap whether or not any card work is pending (`chimera-board/src/lib.rs:843`). That costs two extra holds and puts the service test to sleep for that span.
- `UsbOut::recv` takes one full hold per byte (`usb.rs:812-823`). During a `put`, each unmask lets a pending RXFLVL re-enter the handler.
- `midi_out` pends `OTG_FS` every lap the USB ring is non-empty even when nothing reads it. The poll then discards it (`lib.rs:393`, `usb_midi.rs:88-97`), which is cheap.

**3. Performance: 7.** The good numbers come from the unit readings recorded in the ADR and the PR body; I didn't re-measure them:
- The handler is cheap: 6–7 µs worst entry, `usb_irq_pct` 1.2 % in a flood.
- `OTG_FS` sits at the MIDI level under audio.
- The audio path only gains a third note queue to drain.
- The handler's stack is 712 B.

Against that:
- **The storm guard's realistic trip path under load is the 20 ms tick test (I1).** Each trip starves everything below level 4 for 20 ms, every UI lap: the card DMA IRQ that streams TAPE (level 12), SysTick's control scan (level 15) and the UI.
- Bank 1 grows about 1.6–2.1 KB per set against ADR 0198's freeze. It is documented, measured and largely unavoidable (the vector, the cell, the vtable, the closed path), but it is real growth into frozen flash.
- AXI grows about 3.5 KB for the 512-slot USB control ring.

**4. Principled-ness: 7.**
- Strong: the guard measures starvation directly after three failed CPU-share formulas, all of them recorded. The wall clock moved from SysTick to DWT because SysTick stalls during a storm. Bank placement and the outliner hazard are explained, and `door-check` enforces them.
- Weak: the known root cause of the storm (a CDC packet at the head of the shared RX FIFO keeps RXFLVL set) is left unfixed and contained by a guard. The guard's thresholds are judgement, not derived: 200 entries/ms is still "provisional", and § 13.5's 4× rule was never applied. 20 ms is far above what SysTick's 2 ms step needs.
- Several workarounds exist for the linker and outliner rather than a structural fix. These are reasonable given ADR 0198, but they are workarounds: `#[inline(never)] usb_feed`/`usb_lost` (`midi_in.rs`), the bank-1 vtable shims (`usb_midi.rs:175-183`), and `NoteProducer::push` forced inline.

**5. Correctness and tests: 7.** I found no ISR/UI race:
- `with` masks, then `dsb`/`isb`, before it touches the cell (`usb.rs:1037-1044`), and unmasks only when `held` is clear (`usb.rs:1050-1055`).
- Only the top of `service` clears `held` (`usb.rs:458`), so a trip stays masked across a long answer's pumps and errands.
- The cell is written before the unmask, with a compiler fence (`usb.rs:992-1008`). `IN` is written before connect (`usb_midi.rs:146-157`).
- Every `unsafe` block has a SAFETY comment.
- The USB note producer and control ring stay single-producer, because the handler and the UI's polls are serialised by the mask.

The pure parts are well covered:
- Packer/unpack round trips and running status (`chimera-midi/tests/usb_test.rs`).
- Enumeration of both layouts through usb-device's own `poll` (`usb_class_test.rs`).
- The guard's tests, including the DWT wrap, and `WallClock` (`console_shell_test.rs`).
- `Tee` (`out_ring_test.rs`), echo per port (`midi_echo_test.rs`), and `lets_go`/`lost` (`midi_pipe_test.rs`).

Not host-tested, only covered by the unit run:
- `door::poll`'s Configured edge.
- `door::send`'s `WouldBlock` hold, error drop and `!up` reset.
- `Closed::drain`.
- How the handler, `held` and `unlock` work together.

`Errand`'s "fails to build" guarantee can be bypassed (M1).

## Findings

### Critical

None.

### Important

**I1. Under audio load the storm guard only trips after 20 ms, starving everything below MIDI's level each UI lap.**
- *Where:* `chimera-core/src/console/shell.rs:144-153`, `:157`; `chimera-board/src/usb.rs:1078`.
- *What goes wrong:* `storm` counts entries per 1 ms of wall DWT time, and audio's preemption inside that window counts against it. A RXFLVL storm whose entries cost a few µs, at 65–70 % audio load, can't fit 200 entries into a millisecond. It runs on until `starved`'s tick test fires at `TICK_STALL_MS` = 20. The PR's own test says this is what happened on the unit: `chimera-core/tests/console_shell_test.rs:234-240`, "each 22 ms lap the handler storms until the tick test trips at 20 ms of DWT, SysTick stalled all the while".
- *What it costs:* for the length of a console flood (or the storm a large `put` raises, which the ADR lists as untested), every UI lap loses about 20 ms below level 4:
  - `DMA2_STR0`, the card IRQ that streams TAPE (level 12, `priority.rs`): underruns are plausible, and the flood run reported audio overruns only, not `stream_underruns`.
  - SysTick's HC165 scan and tick (level 15): missed encoder steps.
  - The UI.
- *ADR text that is wrong:* "A storm costs at most one window's entries per UI lap" (ADR 0222, storm guard paragraph) doesn't hold under load.
- *Fix:*
  - Trip on the storm's signature instead of on wall-window entries, for example N consecutive entries whose poll moved no bytes. Alternatively, count only the handler's own cycles, or entries per handler-millisecond.
  - Lower `TICK_STALL_MS` to about 4–6 ms; SysTick steps every 2 ms at `CONTROLS_HZ` 500 (`controls.rs:34`).
  - Count trips per test (entries vs tick vs service) so `stats` shows which one fires.
  - The root fix is the RXFLVL mask that § 13.5 reserved as its own task.
  - Re-run the flood with TAPE streaming and read `stream_underruns`.

**I2. The most stateful new code is not host-tested and can't be in its current place.**
- *Where:* `chimera-board/src/usb_midi.rs:249-324` (`door::poll`, `door::send`) and `:106-111` (`Closed::drain`).
- *What it holds:*
  - The Configured edge that decides `lost` (`:259-273`).
  - The 16-packet held buffer across `WouldBlock`.
  - The packer reset and ring discard when `!up`.
  - Drop accounting on IN errors.
  - The OUT read loop's cable filter.
- *Why it matters:* this all lives in the target-only board crate, so a regression only shows up on hardware. For example, a later edit that resets `tx.n` on `WouldBlock` would silently drop up to 16 packets, or a missed edge would leave stuck notes. Hardware checks are deferred to ship time in this project.
- *Fix:* move the pure parts into `chimera-midi`, the same way `Packer` already is:
  - an `InQueue { packer, held, n }` whose `fill(ring)` and `sent(result)` are tested on a fake ring and a write closure;
  - a `Configured` edge type that returns `Option<Left>`.

  `door::poll` becomes a thin shell over them.

### Minor

**M1. The `Errand` proof can be bypassed.**
- *Where:* `chimera-core/src/errand.rs:12`; used directly at `chimera-board/src/lib.rs:845` and `usb.rs:970,1267`.
- *Problem:* `Errand::run` is `pub` in every build, so firmware code can call `Errand::run(|e| ui.card_work(e, ...))` anywhere and skip the guard's sleep. The ADR and PR claim "a long call outside an errand fails to build". That is a convention, not a type guarantee. The cost of misuse is a false trip, which delays USB MIDI by one lap.
- *Fix:* gate `run` behind a capability only the board can mint, such as a sealed `ErrandScope` trait implemented by `Link` and by the console-less shell. Alternatively, make it `unsafe fn` with the contract in its SAFETY doc. Then correct the ADR sentence.

**M2. `static mut IN` sits outside the cell, so its exclusion is by comment only.**
- *Where:* `usb_midi.rs:132-157`, `:94`, `:253-258`.
- *Problem:* `IN` is sound only because every reader also holds `&mut UsbCore`, and nothing ties the two together. ADR 0222 itself says "New state goes in the cell, not in statics".
- *Fix:* fold `MidiIn` into `Midi` or `UsbCore` (filled at `init`/`connect` instead of `set_in`), so the borrow checker proves the exclusion. If the outliner forbids it, say so in the ADR.

**M3. `Midi::poll`'s `Result<(), Option<Closed>>` packs three states into nested `Result`/`Option`.**
- *Where:* `usb_midi.rs:77-99`, consumed at `usb.rs:899-904`.
- *Fix:* an enum such as `Polled { ByBank2, Console(Option<Closed>) }` reads directly at the call site.

**M4. `usb_midi_drops` adds up three different units.**
- *Where:* `usb_midi.rs:280`, `:109`, `:302`, `:319`.
- *Problem:* it adds OUT event packets dropped (by cable or CIN, or drained while closed), echo *messages* lost by `Tee`, and IN *packets* lost to endpoint errors. On the unit, one number can't say whether the host's input or the echo is losing data.
- *Fix:* split it into in and out drops, or document the mix in `UsbTally`.

**M5. A suspend after every USB note is already released still cuts DIN-held notes.**
- *Where:* `midi_in.rs:463`, `:635`.
- *Problem:* `played` means "a note-on since the last `lost`", not "a note still held". If USB plays and releases a note and the host later autosuspends, `lost` sends NotesOff on all 16 channels and cuts notes held from DIN. This matches the § 13.8 amendment, but that amendment exists to avoid exactly this.
- *Fix:* track held USB notes (a 16×128-bit set or a per-channel count) and let go only while one is held.

**M6. The 16 NotesOff of `lost` can be refused by a full USB note queue.**
- *Where:* `midi_in.rs:509-519`, called from `usb_midi.rs:271`.
- *Problem:* the PR's dense-burst test overflowed USB's note queue (840 events). A bus reset that lands inside such a burst has its NotesOff refused (counted, not retried), which leaves notes stuck.
- *Fix:* retry `lost` from the poll until it is accepted (a pending flag), or reserve queue room for channel-mode events.

**M7. The `alive` stamp has no wrap guard past 8.9 s.**
- *Where:* `shell.rs:168-171`.
- *Problem:* if the UI wedges outside an errand for more than one DWT wrap, `now.since(alive)` comes back under 250 ms for a window every 8.9 s. The tick-stamped watchdog doesn't catch a wedged UI, so the service test is the only guard there.
- *Fix:* none needed beyond a comment, or saturate `alive` on the handler's tick changes, as `tick_at` is.

**M8. § 13.5's `STORM_LIMIT` 4× rule is still open.**
- *Where:* `shell.rs:117` ("Provisional").
- *Fix:* file it as an issue with the dense-stream measurement it waits on, and reference that issue from the constant.

**M9. `UsbOut::recv` takes one hold per byte.**
- *Where:* `usb.rs:812-823`.
- *Problem:* each hold's unmask lets a pending RXFLVL re-enter the handler. This is the mechanism behind "the storm a large console `put` raises".
- *Fix:* read up to a packet per hold into a small buffer in `UsbOut` and hand bytes out from it.

**M10. The card-work errand wraps every lap unconditionally.**
- *Where:* `lib.rs:843`.
- *Problem:* two extra holds per lap, and the service test sleeps through the card-work span even when nothing runs.
- *Fix:* enter the errand only when `card_pending()` or a sync or DFU is pending.

## What would raise each score to 9

- **Code quality:**
  - Seal `Errand` (M1).
  - Move `IN` into the cell (M2).
  - Replace the nested `Result`/`Option` with an enum (M3).
  - Extract `send`'s queue and the Configured edge into `chimera-midi` (I2).
- **Efficiency:**
  - Buffered `recv` (M9).
  - Errand only when work is pending (M10).
- **Performance:**
  - Make storm trips cheap under load (I1): a no-progress or own-time entry test, a tick test of about 5 ms, or the RXFLVL root fix.
  - Show `stream_underruns` = 0 under a flood with TAPE streaming.
- **Principled-ness:**
  - Fix RXFLVL at the root (the § 13.5 fallback task), or derive the guard's thresholds from measurements and record per-test trip counts.
  - Close § 13.5's 4× rule (M8).
- **Correctness and tests:**
  - Host tests for the extracted `send`/edge logic, including `WouldBlock` hold and `!up` reset (I2).
  - A test that `lost` survives a full queue (M6).
  - Held-note tracking for suspend (M5).
