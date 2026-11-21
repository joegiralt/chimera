# 0307. Bug batch A: a console install restarts the unit, a key held from power-on waits for its release

- **Status:** Proposed
- **Deciders:** firmware (bug batch A: boot, SD card, USB, update and host tools; owner's rule of 2026-11-21, every known bug ships fixed in 0.4), for the owner's review

## Context
Batch A's fixes are mostly local. Four decide behaviour later work has to keep:

1. **#1064.** USB lays out once per boot: `usb::init` allocates USB MIDI and audio only if bank 2's door is open then (ADR 0256), and a host never sees the layout change under it. The boot's own install (`bank2::boot_install`) runs before `usb::init`, so a boot that installs comes up whole. A console `bank2 install` (`chimera-usb.py bank2`, and `update` onto firmware too old to stage the .bk2 first) opens the door in a running session, after the layout: the unit stayed console-only until a power cycle, while `status` said bank 2 open (confirmed on the unit 2026-11-21).
2. **#1000.** Bootloader 1.09 picks a bank 2 slot by the key held at power-on (ADR 0272). The controls start seconds later (after the project loads); a key still held then debounced into a press, and the unit came up on MIXER 2 › SENDS.
3. **#766.** The card watchdog judged a stream hold stuck, then asked an abort; a give and re-take between the two landed the abort on the next transfer.
4. **#354.** SETTINGS › SYSTEM › DIAGNOSTICS › DEMO is a debug build's (`cfg(debug_assertions)`), and 21 screen and console tests walk to it: `cargo test --release` failed them all.

## Decision

### A console install that opens the door restarts the unit (#1064)
- `Bank2Answer::restarts` (exhaustive over the answers): only `Open` restarts. Which restart an answer ends in is one pure table, `console::restart_after(dfu, sweep, install, sent)`: `dfu` first, then `sweep`'s request, each only once its answer is out; an install that opened the door even if its answer stalled, as its USB needs the boot either way. `Usb::service` maps it to its typed tokens (`Restart::Bank2(Bank2Opened)`), drains the answer and returns; the UI loop syncs SYSTEM, as before any console restart, and resets. The boot it restarts into judges bank 2 open and lays USB out whole.
- Before anything is read or written, `console::install_guard(pending, unsaved)` refuses: card work pending is `ERR bank2: busy`, a Modified project `ERR bank2: unsaved changes, save first` (`Bank2Refusal::Unsaved`, `sweep`'s words: its restart would lose them too). The tool prints the refusal and exits 1.
- The answer's words are unchanged. `chimera-usb.py` waits after any `OK bank2 open` for the unit to answer `status` again (`RESTART_WAIT_S`, 60 s; the post-flash wait's own loop, by USB id and serial), says `restarting` on stderr, and `update` reads its final `status` from that answer.

### A switch down at its first read waits for its release (#1000)
- `chimera_hal::StartHold`, one bitmask beside the switches' `Debounce`s in the SysTick read: a switch down at the controls' first tick sets its bit and reads up, press and release alike, until the raw level and the settled one are both up, which clears it; a switch without its bit reads the settled level, as before.

### An abort carries the hold it judged (#766)
- `CardBus::stuck` returns the `Hold` it judged (the stream hold's stamp), `held` the hold now. `ask_abort(Hold)` re-reads the hold and asks only if it is still that one (false otherwise, and the UI's wait may judge again; it marks itself asked only on an ask); it stores the stamp before the flag. `take_abort` is true only if the flag was set and the stamp is still the hold's: a re-take stamps anew, a burst's `renew` moves it, so neither is stopped by an abort judged before it. A gate build's close asks of the hold as it is, read again up to four times.

### Tests reach DEMO in any profile (#354)
- DEMO's rows are `cfg(any(debug_assertions, feature = "test-support"))`. No firmware enables `test-support`, so a release unit still has no DEMO row.

### Smaller decisions recorded here
- **#315.** The console takes a line whose first word is `OK` or `ERR` (any case) for no request: an echoing tty's copy of an answer.
- **#197.** `UiState::show_system_note`, after `boot_project`: a SYSTEM file this firmware can't read toasts its file error with the subject `SYSTEM` (`Subject::System`, e.g. `FILE IS CUT SHORT: SYSTEM`); no card, no file and card faults keep the project's note.
- **#314.** With no `CHIMERA_USB`, the tool's port on Linux is the udev rule's `/dev/chimera`; else the tty whose USB device has the console's id (`unit_ports`), one whose product string says Chimera before others with that id (a stock PreenFM3's); one found only by its id must answer `status` with Chimera's `firmware` line, or the tool stops and asks for `CHIMERA_USB`. Never a guessed `/dev/ttyACM0`.
- **#1065.** The tool ends an answer on `OK <words>` as on `OK`, and prints the line. In `ls`'s answer a `<name> <size>` row is a row whatever its name (a file named `OK` or `ERR`): the protocol's grammar is unchanged.
- **#943.** The class answers GET_DESCRIPTOR for the device, configuration and BOS by type at any index; USB audio counts a re-aim only for a packet whose held parity (DIEPCTL.EONUM) was not the next frame's, and leaves such a packet's DIEPCTL unwritten (the write would have set the parity it holds).
- **#829.** `bank2::ui_door` answers on a host whether or not `tape` is linked: its table carries MAX VOICES, CHOKE, the feel and the band's words too.
- **#379, #505, #637, #811, #830, #1018.** The Justfile reads built firmware from `CARGO_TARGET_DIR`; `check` compiles every fuzz crate, runs the whole page walks, and runs `exclusive_state_test` under `bank2`; `clippy` runs core and engine with `bank2,test-support` on all targets, where bank 1's TAPE DSP tests are `cfg(not(feature = "bank2"))`.

## Alternatives considered
- **#1064, re-enumerate in place** (D+ off, a new allocator, the class added): usb-device's allocator and the classes are boot singletons, borrowed `'static` by the device; rebuilding them is a new USB stack lifecycle in bank 1. Rejected.
- **#1064, document a power cycle**: testers run `update`; a step on the box they can't see the need for is the bug. Rejected.
- **#1064, have the tool reset the unit** (`dfu` then `:leave`): a DFU round trip for every developer install, and nothing for a hand-typed `bank2 install`. Rejected.
- **#1000, seed `Debounce` from the first read**: a held key then reads down with no press, but its release is a release edge the UI may act on. Rejected.
- **#354, gate the tests to debug**: they would never run under `--release`, which is what the issue found missing. Rejected.

## Consequences
- After a console install that opens bank 2 the unit restarts; anything that drives `bank2 install` waits for the console again (the tool does), and saves first.
- Bank 1 grows by the fixes (#1064's restart, #1000's read, #197's note, #315's filter, #766's stamp, #943's re-aim); the measured lines in `tools/bank1-budget.txt` cite this ADR. Bank 2 moves within its 64 B.
- Not decided here, listed for the owner: #544 and #547 (NEEDS UPDATE's copy), #569 (stream admission against the UI's card turn), #710 (the card's block count costs bank 1 about 1 KB), #943's runtime-closed-door and GET_LINE_CODING items.

## Sources
- Issues #1064, #1000, #766, #354, #315, #197, #314, #1065, #943, #829, #379, #505, #637, #811, #830, #1018 (joegiralt/chimera).
- ADR 0198 (bank 1 frozen), 0256 (USB audio and MIDI by default), 0260 (the configuration's static), 0272 (bank 2 slots).
- RM0433 § 57.14.43 (DIEPCTL, EONUM).
