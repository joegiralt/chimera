# USB Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plug the unit into a computer and read it over USB instead of photographing the panel. It shows up as `/dev/ttyACM0`, and `just status`, `just stats`, `just shot` (and `just usb bench`) answer from it. The desktop sim answers the same protocol on a local socket, so all of it is tested on the host before the one ship flash. And flashing needs no BOOT0 jumper: SETTINGS › SYSTEM › OS UPGRADE or the console's `dfu` restarts the unit into ST's ROM DFU loader, and `just flash` uses `dfu` itself (Task 9).

**Architecture:**
- **Functional core** (`chimera_core::console`, host-tested, no I/O, no allocation, no `unsafe`):
  - bytes in: `Console::push` turns received bytes into a typed `Request` or a `Refusal`;
  - bytes out: `answer` writes the whole answer into an `Out`, asking a `Unit` for what it needs;
  - the shells' arithmetic, kept pure: `LoopTimer`, `Report` (the bench's text) and `serial_hex`.
- **Desktop shell** (`chimera-desktop/src/console.rs`): a non-blocking `TcpListener` on `127.0.0.1:7341`, polled at the top of each frame.
- **USB shell** (`chimera-stm32/src/usb.rs`, feature `usb-console`): CDC-ACM on USB2 OTG_FS (PA11/PA12), HSI48 trimmed by the CRS, polled at the top of the UI loop, never from an interrupt.
- **DFU entry** (`chimera_core::boot`, pure; `chimera-stm32/src/dfu.rs`, the shell): a marker in RTC_BKP0R outlives `SCB::sys_reset`; the top of `main` reads and clears it and, on `boot::after_reset`'s `RomDfu`, jumps to the ROM loader at 0x1FF0_9800. OS UPGRADE's prompt yes is a `Said<RomDfu>`.
- **Host tool** (`tools/chimera-usb.py`): Python 3, standard library only, wrapped by `just` recipes; a udev rule; `to-dfu` before `just flash`.

**Tech Stack:** Rust 2024, `no_std` core. New firmware dependencies, behind `usb-console` only: `stm32h7xx-hal` 0.16's `usb_hs` feature (`synopsys-usb-otg` 0.4.0), `usb-device` 0.3.2, `usbd-serial` 0.2.2. Python 3 (standard library) for the host tool and its tests. `just`.

**Spec:** `docs/superpowers/specs/2026-10-02-usb-console-design.md` (owner-reviewed 2026-10-02, binding, § The owner's answers included). Also binding:
- ADR 0068 (Proposed). Task 12 moves it to Accepted after the flash; it is not otherwise edited once accepted.
- ADRs 0019 (note sources) and 0066 (SETTINGS; USB CONFIG stays `Later`, #269).
- Issues: #203 (USB MIDI), #204 (USB audio), #269 (USB CONFIG).

**Branch:** `usb-console`, cut from `nav-core` at `4653c3a`. It lands after `nav-core` (Pre-flight 1).

---

## Global Constraints

**From the spec**
- **Protocol 1.** ASCII request lines of at most `MAX_LINE` = 64 bytes, ended by LF or CR. An empty line gets no answer, so CRLF is one request. Edge spaces are trimmed, and words are split on one or more spaces. The command word and `raw` match case-insensitively.
- **A command takes at most one argument.** Only `shot` takes one, `raw`. Everything else takes none.
- **Every answer ends in exactly one terminal line**, `OK` or `ERR <reason>`. An `ERR` line is the whole answer, with no body before it. The unit never speaks first.
- **The error lines, exactly:**
  - `ERR unknown command <word>, try help` (`<word>` cut to 16 bytes);
  - `ERR <command> takes no arguments`;
  - `ERR shot takes raw or nothing`;
  - `ERR line too long, 64 max`;
  - `ERR <command> is not in this build`.
- **Text bodies** are `<key> <value>` lines, LF only.
- **`shot`** sends `SHOT 240 320 rgb565be 153600`, then 153 600 bytes (rows from the top, left to right, big-endian RGB565), then `OK`. `shot` maps each pixel through THEME's `Palette::map_raw`; `shot raw` sends the framebuffer as drawn (canonical palette). The header is the same for both.
- **One table:** a `commands!` row per command gives its name, help line and argument type. The `Command` and `Request` enums come from it, and `answer` matches `Request` exhaustively.
- **Polled, never an interrupt:** the console runs at the top of the UI loop (sim: of the frame), answers at most one request per iteration, and stops an answer after `STALL_MS` = 250 ms with no byte taken. The OTG interrupt stays masked.
- **Identity:** `0483:5740` (the stock PreenFM3 identity, github.com/Ixox/preenfm3 `firmware/Src/usbd_desc.c` (`USBD_VID 1155`, `USBD_PID_FS 22336`)), manufacturer `Chimera`, product `Chimera console`, serial number the chip's 96-bit UID as 24 uppercase hex digits, self-powered, 100 mA. Never `0483:DF11`.
- **The port appears** once the UI loop starts, after the splash.
- **Feature `usb-console`** is in the firmware's `default` by the end (Task 8). `--no-default-features` builds without it. The sd-probe build has no console.
- **Desktop:** `127.0.0.1:7341`, one client; a busy port prints `console: 127.0.0.1:7341 busy, console off` and the sim runs on.
- **DFU entry** (spec § Enter DFU from the firmware; the owner, 2026-10-02):
  - OS UPGRADE is `Act::EnterDfu`. Its prompt is `ENTER DFU?` / `PLAY STOPS UNTIL FLASHED OR POWER-CYCLED`, pills `ENTER DFU`, `CANCEL`, sealed by `Said<RomDfu>` (`commits!`, `replace::said`). The empty UPDATES page goes.
  - `dfu` answers `OK`, then the unit restarts into the ROM loader; the sim answers `ERR dfu is not in this build`.
  - The marker is `DFU_MAGIC` in RTC_BKP0R (fallback: 0x3800_FFFC, the last word of D3 SRAM4). It is read and always cleared at the top of `main`, before `boot()`. The jump to 0x1FF0_9800 is the feature's one `unsafe`.
  - `just flash` and `just flash-bench` send `dfu` when `0483:5740` is on the bus, wait up to 10 s for `0483:DF11`, then flash. Otherwise they print the jumper instruction and flash as today.
  - Under 1 KB of flash.

**Repo rules (CLAUDE.md, owner)**
- No `unsafe` without `// SAFETY:`. No heap, no blocking and no allocation on the audio path. No libc (the desktop uses `std::net`; the host tool is not Rust).
- **Types carry the invariants.** The owner: "if we could solve a problem, an imperative problem, using the type system, we absolutely should." In this plan that means:
  - a command's argument is parsed once into its own type (`NoArg`, `Colours`), and `answer` never sees a raw word;
  - `Request` is an enum the table generates, so a new row does not build until `answer` handles it;
  - `Word` can only hold printable ASCII, so an echoed unknown word can't put a control byte or a newline into an answer;
  - `Rung` is a public, exhaustive mirror of `nav`'s private `Loc`, so ORBIT's `Loc::Orbit` won't build until `status` names it;
  - `Report` reserves room for its `# TRUNCATED` line, so a full report can always say it is cut;
  - the DFU prompt's yes is a `Said<RomDfu>`, which only `replace::said` makes, and `dfu::enter` takes it (or the console's `Served::Dfu`), so no other path restarts into the ROM;
  - `boot::after_reset` returns a `BootAction`, and only `DFU_MAGIC` gives `RomDfu`.
- Pure core, thin shells. Each feature can be cut by its module and feature flag.
- ADRs record decisions; an accepted ADR is never edited. Follow-ups are GitHub issues (joegiralt/chimera), cited by URL.
- **Commits:**
  - a terse plain sentence, with no type prefix and no Claude or AI attribution;
  - run `git status --short` first;
  - stage files by name, or by the directories a task lists;
  - **never stage `chimera.bin`** (or `docs/chimera-ui-ux-spec.md`).
- `just check` passes before every commit, and no task ends red.
- No stack frame of 8 KB or more (`just stack-check`). The shot's row buffer is 480 bytes.
- **Hardware:** desktop and host checks between tasks; ONE ship flash at the end (Task 12). Claude does QA; the owner does UAT.

## Review Focus

These failure modes are implied by the spec but not exercised by any spec test. Each is pinned by a test in the task named.

1. **A request split across reads.** Bytes come in packet by packet (USB) or segment by segment (TCP). A line cut anywhere must still give exactly one request. Task 2: `a_line_split_anywhere_parses_once`.
2. **Bytes after a request in the same read.** At most one request per iteration means the next request's bytes must wait, not be dropped. Task 7: `two_requests_in_one_write_answer_on_two_frames`. Task 8 reads one byte at a time for the same reason.
3. **A stalled answer.** After a stall, the host must never see a terminal line it could take for the end of the stalled answer. Task 4: `a_stall_mid_shot_stops_without_a_terminal_line`. Task 10: `a_stalled_tail_is_drained_before_the_next_request`.
4. **A slow frame caused by the console itself.** An iteration that answered must not count as UI loop time, or every `shot` would show up as a 300 ms frame in `stats`. Task 6: `an_answered_iteration_is_not_timed`.
5. **An unknown word with control bytes in it** (`\x1b[A`, a NUL, a lone `\r` inside the word) must not break the one-line `ERR`. Task 2: `unknown_word_answers_in_printable_ascii`.
6. **A palette change in the same frame as the shot.** The shot must match what the panel shows now. The console runs at the loop top, after the previous iteration's flush (a recolour flushes the whole screen), and reads the palette the display last flushed with. Task 8 Step 4 (the hook's position) and U3 at the flash.
7. **`stats` in a build without `perf-probe`, `bench` outside a bench build** answer one `ERR … is not in this build`, with nothing before it. Task 5: `none_from_the_unit_is_one_err_line`.
8. **`dfu`'s `OK` lost to the reset.** The host must read `OK` before the unit leaves the bus, or `just flash` can't tell a restart from a hang. Task 9: `dfu_is_ok_then_the_unit_is_told_once`, and `service` returns `Served::Dfu` only after the flush (Task 9 Step 6); U9.
9. **A marker that traps the unit in DFU.** If the marker outlived the jump, every reset would go back to the ROM. The shell clears it before acting, whatever it held, and only `DFU_MAGIC` jumps. Task 9: `only_the_magic_enters_dfu`, `a_random_marker_boots_the_synth`; U8's power-cycle.

## Pre-flight: where the spec is silent, and what this plan decides

1. **Base and merge order.** `usb-console` is cut from `nav-core` and uses its `Location`, `UiState::crumbs` and cached `project_status`. It lands after `nav-core`. Each task starts with `git fetch origin`. If `origin/nav-core` has moved, `git merge --no-edit origin/nav-core` before the task. Once `nav-core` is on `main`, merge `origin/main` instead and open the PR against `main`. Conflicts in `chimera-core/src/ui/{mod,nav}.rs` are resolved by keeping `nav-core`'s side, then re-applying Task 3's additions.
2. **Task 1 measures before anything is built on it.** It writes the real bring-up (`usb.rs`: clock, CRS, USB2, the CDC device, `poll` in the loop, received bytes discarded) behind `usb-console`, which stays out of `default` until Task 8. So the measurement is of the code that ships, and nothing is thrown away. The functional core (Tasks 2–6) is still built and host-tested before either shell is finished (Tasks 7–8). The core is not linked in Task 1, so Task 1 adds the spec's 3 KB estimate for it. Task 8 measures the whole cost again, under the same 24 KB limit.
3. **`at` for SETTINGS** is the breadcrumb as drawn: the rows' short crumbs (`SETTINGS > SYSTEM > DIAG > AUD LOAD`), NAMING's crumb included, never shortened behind `..`. The spec's example now reads this way. `Crumbs::parts` becomes `pub` (Task 3).
4. **No ORBIT row.** `nav-core` has no `Loc::Orbit`. `Rung` mirrors `Loc` exactly, and `Location::rung` matches exhaustively, so the ORBIT plan's new variant fails the build until it gets a `Rung` and an `at` (`ORBIT`).
5. **A page title** is the header's page name as drawn: `components::header_text(..).name`, with the exciter and ENV A/B suffix rules. Task 3 moves those rules into one `UiState::page_name()`, which `draw_header` and `status` both call, so they can't drift.
6. **`LoopTimer`, `Report` and `serial_hex` live in the core** (Task 6), not in the shells as the spec sketched. They are arithmetic and formatting, so they are host-tested. The shells only read the DWT or `Instant`, the UID registers and the bench's lines. A shell cut by its feature never links them.
7. **The desktop's `shot`** equals the framebuffer through the palette, not the window: the window also applies BRIGHT, which the spec keeps out of `shot` ("panel gamma and backlight are analog"). Task 7 tests that.
8. **The host tool's target** also takes `tcp:HOST:PORT`, so its tests can run a fake unit on a free port. `sim` is still `127.0.0.1:7341`.
9. **Reads are one byte at a time** in both shells, stopping at the first request (`serial.read(&mut [u8; 1])`; `TcpStream::read` into `[u8; 1]`). What follows waits in usbd-serial's buffer or the socket's. A request is 64 bytes at most, so this costs nothing that matters.
10. **`state`** comes from `UiState::project_status()`. On `nav-core` it reads the status cache and hashes only when the cache is behind (about 1.7 ms on the chip, once per request at most).
11. **`firmware`** is `about_page::VERSION`, a space, then `about_page::BUILD` in lowercase (`0.1.0 release`).
12. **The Python tests run in `just check`** (`python3 -m unittest discover -s tools -p 'test_*.py'`). They use the standard library only, like the tool.
13. **The sd-probe build** has `usb-console` on by default but no synth. `usb.rs` and its hook are `#[cfg(all(feature = "usb-console", not(feature = "sd-probe")))]`, as `SynthParts` is, so clippy's dead-code lints stay quiet.
14. **The DFU check runs at the top of `#[entry] fn main`, not in `#[pre_init]`.** cortex-m-rt 0.7.5 documents a Rust `pre_init` as unsound (it runs before RAM is initialised), and `chimera-stm32/src/main.rs` has no `before_main`. At `main`'s top, RAM is ready and `boot()` has set up no clock, peripheral or interrupt. `main` takes both peripheral sets and hands them to `boot(cp, dp)`, so the check reads and writes registers through owned PAC handles: the jump is the only `unsafe`.
15. **The marker is RTC_BKP0R through the PAC**, not the HAL's `rtc::Rtc`, whose `init` resets the backup registers and whose `open_or_init` wants an RTC clock (Chimera has no RTC). The HAL's PWR `freeze` sets DBP (`pwr.rs`) and leaves it set, so `enter` needs only the write. The stock bootloader (`bootloader/Src/main.c`: `HAL_Init`, `SystemClock_Config`, then `HAL_RCC_DeInit` and `HAL_SuspendTick` before jumping) names no RTC, backup or IWDG register. Its `.data`/`.bss` are in DTCM and its stack tops AXI SRAM at 0x2408_0000 (`STM32H753VITX_FLASH.ld`). Nothing of it is in RAM_D3, and nothing of Chimera's either (`memory.x`), so 0x3800_FFFC is the fallback.
16. **OS UPGRADE's page holds nothing.** `UPDATES_LEAF` is a `OnePage` over `SYS_UPDATES`: six `EMPTY` cells, `VizType::None`, so it draws a header only. ABOUT already shows VERSION, BUILD, REV, CLOCK, RESET and CARD, so nothing moves there. Block id 34 is retired.
17. **No SAVE FIRST on the DFU prompt.** Entering DFU is a power-cycle. The footer shows MODIFIED, and MENU hold saves.
18. **The hands-free flash lives in the host tool** (`to-dfu`, Task 10), not in shell lines in the `Justfile`. It finds `0483:5740` and `0483:DF11` in `/sys/bus/usb/devices/*/{idVendor,idProduct}` (`CHIMERA_SYSFS` overrides the root for tests), with no `lsusb`.

## File structure

| File | Responsibility |
|---|---|
| `chimera-core/src/lib.rs` | `pub mod console;` |
| `chimera-core/src/console/mod.rs` | `MAX_LINE`, `PROTOCOL`, `STALL_MS`, `commands!`, `Command`, `Request`, `Arg`, `NoArg`, `Colours`, `Words`, `Word`, `Refusal`, `Console`; re-exports. |
| `chimera-core/src/console/status.rs` | `write_status`, `state_word`. |
| `chimera-core/src/console/shot.rs` | `Out`, `Stalled`, `Frame`, `write_shot`. |
| `chimera-core/src/console/answer.rs` | `Unit`, `Stats`, `answer`, the help, stats and bench writers, the `ERR` lines. |
| `chimera-core/src/console/shell.rs` | `LoopTimer`, `Report`, `serial_hex`. |
| `chimera-core/src/ui/nav.rs` | `Rung`, `Location::rung`. |
| `chimera-core/src/boot.rs` | `DFU_MAGIC`, `ROM_DFU_BASE`, `BootAction`, `after_reset`, `RomDfu`. |
| `chimera-core/src/ui/settings/{tree,part,prompt,replace,mod,leaves}.rs`, `ui/{mod,block_registry}.rs`, `project/guard.rs` | OS UPGRADE → `Act::EnterDfu`; `DfuAnswer`, `EnterDfu`, `Ask::EnterDfu`; `take_dfu`; UPDATES goes; `RomDfu` sealed. |
| `chimera-core/src/ui/{mod,renderer,components}.rs`, `ui/settings/view.rs` | `UiState::page_name`; `draw_header` uses it; `Crumbs::parts` public. |
| `chimera-core/tests/console_{parse,status,shot,answer,shell}_test.rs`, `boot_test.rs`, `enter_dfu_test.rs` | The core's tests. |
| `chimera-desktop/src/console.rs` | `ADDR`, `SocketConsole`, `DeskUnit`, `StreamOut`; its tests. |
| `chimera-desktop/src/{main,display}.rs` | The hook at the frame's top; `DesktopDisplay::frame`. |
| `chimera-stm32/Cargo.toml`, `Cargo.lock` | `usb-console`, the three crates. |
| `chimera-stm32/src/usb.rs` | `UsbParts`, `init`, `Usb`, `UsbOut`, `ChipUnit`, `Served`. |
| `chimera-stm32/src/dfu.rs` | `MARKER`, `after_reset`, `enter`, `DfuFrom`. |
| `chimera-stm32/src/{main,display,bench,priority}.rs` | `UsbParts` from `boot()`; the hook; `Stm32Display::frame`; the bench's `Report`; the comment. |
| `tools/chimera-usb.py`, `tools/test_chimera_usb.py`, `tools/70-chimera.rules` | The host tool, its tests, the udev rule. |
| `Justfile` | `usb`, `shot`, `stats`, `status`; `check` runs the tool's tests; `flash` and `flash-bench` run `to-dfu` first. |
| `docs/adr/0068-usb-console.md`, `docs/adr/README.md` | Accepted after the flash (Task 12). |
| this plan | `## Measured`. |

## Task order

1. The USB crates and the bring-up, measured (STOP above 24 KB).
2. `console`: the command table and the parser.
3. `console`: `status`, and `Rung` in `nav`.
4. `console`: `shot`, `Out` and the stall.
5. `console`: `answer`, `Unit`, help, `stats`, `bench` and the refusals.
6. `console`: the shells' pure helpers (`LoopTimer`, `Report`, `serial_hex`).
7. The desktop socket shell.
8. The USB shell on the chip; `usb-console` on by default; the bench's report.
9. Enter DFU from the firmware: OS UPGRADE and `dfu` (STOP above 1 KB).
10. The host tool, its `just` recipes, `to-dfu` and the udev rule.
11. Desktop QA and the measurements.
12. The ship flash (STOP for the owner).

Each task depends on the one before it, except: 2 depends only on 1's merge step (it touches no firmware file); 3, 4 and 6 depend only on 2; 5 depends on 3 and 4; 7 depends on 5; 8 depends on 1, 5 and 6; 9 depends on 5, 7 and 8; 10 depends on 7 and 9 (its end-to-end step runs the sim, and `to-dfu` needs `dfu`).

## Pre-flight: tasks that share a file or an interface

One row per pair of tasks that touch the same file or the same interface, and how the later one stays out of the earlier one's way.

| Pair | Shared | Resolution |
|---|---|---|
| 1 · 8 | `chimera-stm32/src/usb.rs`, `main.rs`, `Cargo.toml` (`usb-console`, `default`); `Justfile` (`check`, `clippy`) | Task 1 writes `UsbParts`, `init` and a bare `poll`, and adds two temporary `--features usb-console` lines to `check` and `clippy`. Task 8 adds `service`, `UsbOut`, `ChipUnit` and the loop timer, puts `usb-console` in `default`, and removes exactly those two lines. Task 8 changes no line of Task 1's bring-up order. |
| 1 · 9 | `chimera-stm32/src/main.rs` (`main`, `boot`) | Task 9 makes `main` take both peripheral sets, call `dfu::after_reset`, and pass them to `boot(cp, dp)`. Inside `boot` it changes only the two `take()` lines, and none of Task 1's `UsbParts` lines. |
| 1 · 10 | `Justfile` (`check`) | Task 10 appends its `python3 -m unittest` line after the last `cargo` line and touches neither of Task 1's lines (gone by then). |
| 1 · 11 | this plan's `## Measured` (Flash) | Task 1 writes the "bring-up" row. Task 11 adds the "whole" row beside it and never rewrites Task 1's. |
| 2 · 3 | `console/mod.rs` | Task 3 adds only `mod status; pub use status::*;`. |
| 2 · 4 | `console/mod.rs`; `Colours` | Task 2 defines `Colours`. Task 4 consumes it and adds only `mod shot; pub use shot::*;`. |
| 2 · 5 | `console/mod.rs`; `Request`, `Refusal`, `Command::{ALL, name, about, usage}`, `PROTOCOL` | Task 5 consumes them unchanged and adds only `mod answer; pub use answer::*;`. A new command is a `commands!` row (Task 2's file) plus an `answer` arm (Task 5's file), nothing else. |
| 2 · 6 | `console/mod.rs`; `STALL_MS` | Task 6 adds only `mod shell; pub use shell::*;`. `STALL_MS` is defined once, in Task 2. |
| 3 · 5 | `write_status` | Task 5 calls it as Task 3 left it. |
| 2 · 9 | `console/mod.rs` (`commands!`); `console_parse_test.rs` | Task 9 appends one row, `Dfu`, after `Shot`, and updates `the_table_is_the_one_list` and `commands_without_arguments_refuse_one` to include it. No other row or parser line changes. |
| 3 · 9 | `chimera-core/src/ui/mod.rs` | Task 3 adds `page_name`. Task 9 adds `take_dfu`, the `dfu` field, `run_act`'s arm and the answer's arm, and touches none of Task 3's lines. |
| 5 · 9 | `Unit`, `answer`; `console_answer_test.rs`'s `Fake` | Task 9 adds `Unit::dfu` and the `Request::Dfu` arm. `Fake` gains `dfu` and `dfu_calls`, and every Task 5 test passes unchanged except the two lists it extends (`only_stats_reads_the_stats`, the property test's words). |
| 4 · 5 | `Out`, `Stalled`, `Frame`, `write_shot` | Task 5 calls them as Task 4 left them. Task 5's `Unit::frame` returns Task 4's `Frame`. |
| 4 · 7 | `Out`, `Stalled`, `Frame` | `StreamOut` implements `Out`; `DesktopDisplay::frame` returns `Frame`. |
| 4 · 8 | `Out`, `Stalled`, `Frame` | `UsbOut` implements `Out`; `Stm32Display::frame` returns `Frame`. |
| 5 · 7 | `Unit`, `Stats`, `answer` | `DeskUnit` implements `Unit`: `stats` and `bench` return `None`. |
| 5 · 8 | `Unit`, `Stats`, `answer` | `ChipUnit` implements `Unit`. |
| 6 · 8 | `LoopTimer`, `Report`, `serial_hex` | Consumed unchanged. `BENCH_TEXT_LEN` (6 144) is defined in `bench.rs`, not in the core. |
| 7 · 9 | `DeskUnit`; `chimera-desktop/src/main.rs` | Task 9 adds `DeskUnit::dfu` (`None`), one test, and one `take_dfu` line after the frame's input. The console hook stays the loop's first statement. |
| 8 · 9 | `chimera-stm32/src/usb.rs` (`service`, `ChipUnit`); the loop in `main.rs` | Task 9 gives `service` a `Served` return and `ChipUnit` a `dfu` method. It adds two lines after `service` and after the frame's input. Task 8's polling, pump and stall lines are unchanged. |
| 9 · 10 | the `dfu` command; `Justfile` (`flash`, `flash-bench`) | Task 10's `to-dfu` sends exactly `dfu` and needs `OK`. Task 10 adds one `to-dfu` line to each recipe, between `rust-objcopy` and `dfu-util`, and changes no other line. |
| 9 · 11 | this plan's `## Measured` (Flash) | Task 9 writes the "DFU entry" row. Task 11 never rewrites it. |
| 7 · 10 | the sim's address `127.0.0.1:7341`; `CHIMERA_USB=sim` | `ADDR` in `console.rs` and `SIM` in the tool are the same literal. Task 10's test `sim_target_is_the_desktop_console_address` reads `console.rs` and checks it. |
| 8 · 10 | the identity `0483:5740` (builder), the udev rule and `to-dfu`'s match | All three are written from the spec's § Identity. Task 10's test `udev_rule_matches_the_firmware_identity` reads `usb.rs` and the rule and checks they agree. |
| 10 · 11 | `Justfile` (`usb`, `shot`, `stats`, `status`, `check`, `flash`) | Task 11 runs the recipes and edits no recipe. |
| 11 · 12 | this plan's `## Measured` | Task 12 fills the ship checklist's empty slots and edits nothing Task 11 wrote. |
| 9 · `nav-core` (outside) | `ui/settings/{tree,prompt,replace,mod}.rs`, `ui/mod.rs` | Pre-flight 1: keep `nav-core`'s side and re-apply Task 9's additions (one `Act`, one `Ask`, one `Answered`, one `commits!` row). |
| 3 · `nav-core` (outside) | `chimera-core/src/ui/{mod,nav,renderer,components}.rs`, `ui/settings/view.rs` | Pre-flight 1: keep `nav-core`'s side and re-apply Task 3's additions. |

---

### Task 1: The USB crates and the bring-up, measured (STOP above 24 KB)

**Files:**
- Modify: `chimera-stm32/Cargo.toml`, `Cargo.lock`, `chimera-stm32/src/main.rs`, `Justfile` (two temporary lines, Step 7); this plan (`## Measured`)
- Create: `chimera-stm32/src/usb.rs`

**Interfaces:**
- Produces:

```rust
// chimera-stm32/Cargo.toml
// [features] usb-console = ["stm32h7xx-hal/usb_hs", "dep:usb-device", "dep:usbd-serial"]   (not in default yet)
// [dependencies] usb-device = { version = "0.3.2", optional = true }
//                usbd-serial = { version = "0.2.2", optional = true }

// chimera-stm32/src/usb.rs, #[cfg(all(feature = "usb-console", not(feature = "sd-probe")))]
pub const VID_PID: (u16, u16) = (0x0483, 0x5740);   // stock PreenFM3 ID: Ixox/preenfm3 firmware/Src/usbd_desc.c
pub struct UsbParts {                               // moved out of boot(), nothing enabled
    pub dm: PA11<Alternate<10>>, pub dp: PA12<Alternate<10>>,
    pub global: pac::OTG2_HS_GLOBAL, pub device: pac::OTG2_HS_DEVICE, pub pwrclk: pac::OTG2_HS_PWRCLK,
    pub rec: rec::Usb2Otg, pub crs: pac::CRS,
}
pub struct Usb { dev: UsbDevice<'static, UsbBus<USB2>>, serial: SerialPort<'static, UsbBus<USB2>> }
pub fn init(parts: UsbParts, clocks: &CoreClocks) -> Usb;   // spec § Bring-up steps 2–6
impl Usb { pub fn poll(&mut self); }                          // Task 1: poll, read and discard
```

- [ ] **Step 1: Merge.** `git fetch origin`. If `origin/nav-core` is ahead of `HEAD`'s base, `git merge --no-edit origin/nav-core` (Pre-flight 1). Then `just check` → PASS.
- [ ] **Step 2: Baseline.** Record the default release build's flash size:

```bash
size="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-size"
elf=target/thumbv7em-none-eabihf/release/chimera-stm32
flash() { "$size" -A "$elf" | awk '/^\.(vector_table|text|rodata|data) /{s+=$2} END{print s}'; }
ram()   { "$size" -A "$elf" | awk '/^\.(data|bss|axisram|ram_d2)/{s+=$2} END{print s}'; }
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf && echo "base $(flash) $(ram)"
```

  The section names in `ram()` are the ones `llvm-size -A` lists for this ELF; adjust the pattern to them once and record it.
- [ ] **Step 3: Add the crates and the feature** as above. `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --features usb-console` → PASS. `git diff Cargo.lock` adds only `usb-device` 0.3.2, `usbd-serial` 0.2.2, `synopsys-usb-otg` 0.4.0, `embedded-io` 0.6.1, `heapless` 0.8.0 and `portable-atomic` 1.15.0, and changes no locked version. Anything else: stop and report.
- [ ] **Step 4: Write the bring-up** exactly as the spec's § Bring-up and § Identity say:
  - `boot()`: under the cfg, `ccdr.peripheral.kernel_usb_clk_mux(UsbClkSel::Hsi48)`, `assert!(ccdr.clocks.hsi48_ck().is_some())`, and a `UsbParts` moved into `SynthParts` (the PA11/PA12 pins in AF10, the three OTG2 blocks, `ccdr.peripheral.USB2OTG`, `dp.CRS`).
  - `usb::init`: the CRS (`RCC_APB1HENR.CRSEN`, `CRS_CFGR.SYNCSRC = 0b11`, `CRS_CR.AUTOTRIMEN | CEN`, each write with a `// SAFETY:` note); `USB2::new`; `EP_MEMORY: [u32; 256]` and the `UsbBusAllocator` behind take-once `AtomicBool`s, as `take_framebuffer` does; `SerialPort::new`; `UsbDeviceBuilder::new(bus, UsbVidPid(VID_PID.0, VID_PID.1))` with the strings (serial `"000000000000000000000000"` until Task 8), `.self_powered(true)`, `.max_power(100)`, `.device_class(USB_CLASS_CDC)`.
  - `synth()`: `let mut usb = usb::init(usb_parts, &clocks);` after MIDI DIN starts, before the first frame; `usb.poll();` as the loop's first line.
  - `pac::Interrupt::OTG_FS` is never unmasked.
- [ ] **Step 5: Measure.** Build with `--features usb-console` and record `flash()` and `ram()`. The cost is `with − base`. Add the core's estimated 3 072 bytes (spec § Cost, "the console core 3"), since nothing calls it yet.
- [ ] **Step 6: STOP if the flash cost plus 3 072 is above 24 576 bytes (24 KB).** Record the numbers under `## Measured`, commit them alone (`git add` this plan only, message "USB console bring-up measured over budget"), revert the code with `git checkout -- chimera-stm32 Cargo.lock`, and report to the owner with the per-crate split (`cargo bloat` is not installed; use `"$size" -A` on the two ELFs and `llvm-nm --size-sort -S` filtered to `usb_device|usbd_serial|synopsys`). No further task starts.
- [ ] **Step 7: Otherwise,** record the numbers under `## Measured` (the "bring-up" row) and run `just check` → PASS. (`usb-console` is not in `default`, so `check` builds it only through Step 3's command; add `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --features usb-console` and its clippy line to the `check` and `clippy` recipes now, and Task 8 removes them when the feature joins `default`.)
- [ ] **Step 8: Commit**

```bash
git status --short
git add chimera-stm32/Cargo.toml Cargo.lock chimera-stm32/src/usb.rs chimera-stm32/src/main.rs Justfile docs/superpowers/plans/2026-10-02-usb-console.md
git commit -m "The USB device comes up from the UI loop behind usb-console, and its flash cost is measured"
```

---

### Task 2: `console`: the command table and the parser

**Files:**
- Create: `chimera-core/src/console/mod.rs`; test `chimera-core/tests/console_parse_test.rs`
- Modify: `chimera-core/src/lib.rs`

**Interfaces:**
- Produces:

```rust
pub const MAX_LINE: usize = 64;
pub const PROTOCOL: u8 = 1;
pub const STALL_MS: u32 = 250;
pub const WORD_MAX: usize = 16;

/// The words after the command, split on one or more spaces.
#[derive(Clone, Copy, Debug)] pub struct Words<'a> { /* rest: &'a [u8] */ }
impl<'a> Iterator for Words<'a> { type Item = &'a [u8]; }

pub trait Arg: Sized + Copy {
    const USAGE: &'static str;                  // the tail of `ERR <name> takes <USAGE>`
    fn parse(words: Words<'_>) -> Option<Self>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct NoArg;             // USAGE "no arguments"
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Colours { Theme, Raw } // USAGE "raw or nothing"

commands! {
    Help   (NoArg)   => "help",   "this list",
    Status (NoArg)   => "status", "firmware, project, Part and where the UI is",
    Stats  (NoArg)   => "stats",  "AUDIO LOAD and the UI loop's time",
    Bench  (NoArg)   => "bench",  "the bench's numbers (bench builds)",
    Shot   (Colours) => "shot",   "the screen in THEME's colours; shot raw: canonical",
}
// generates:
// #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Command { Help, Status, Stats, Bench, Shot }
// impl Command { pub const ALL: [Command; 5]; pub const fn name(self) -> &'static str;
//                pub const fn about(self) -> &'static str; pub const fn usage(self) -> &'static str; }
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum Request { Help(NoArg), Status(NoArg), Stats(NoArg), Bench(NoArg), Shot(Colours) }
// impl Request { pub const fn command(self) -> Command; }

/// Printable ASCII only (0x20..=0x7E; anything else is stored as `?`), at most WORD_MAX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Word { /* bytes: [u8; WORD_MAX], len: u8 */ }
impl Word { pub fn new(raw: &[u8]) -> Word; pub fn as_str(&self) -> &str; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal { Unknown(Word), Arguments(Command), TooLong }

pub struct Console { /* line: [u8; MAX_LINE], len: u8, overflowed: bool */ }
impl Console {
    pub const fn new() -> Self;
    /// One received byte. `Some` when it ends a non-empty line.
    pub fn push(&mut self, byte: u8) -> Option<Result<Request, Refusal>>;
}
```

`Word::as_str` is safe code: the constructor admits printable ASCII only, so the bytes are UTF-8 (`core::str::from_utf8(..).unwrap_or("?")`, never `from_utf8_unchecked`).

- [ ] **Step 1: Write the failing tests** in `console_parse_test.rs`, with the helper `fn feed(c: &mut Console, s: &[u8]) -> Vec<Result<Request, Refusal>>` (push each byte, collect the `Some`s):

```rust
use chimera_core::console::*;

#[test]
fn every_name_parses_in_any_case() {
    for c in Command::ALL {
        for name in [c.name().to_string(), c.name().to_uppercase(), mixed_case(c.name())] {
            let got = feed(&mut Console::new(), format!("{name}\n").as_bytes());
            assert_eq!(got.len(), 1, "{name}");
            assert_eq!(got[0].map(|r| r.command()), Ok(c), "{name}");
        }
    }
}

#[test]
fn shot_takes_raw_or_nothing() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    assert_eq!(one("shot\n"), Ok(Request::Shot(Colours::Theme)));
    assert_eq!(one("SHOT RaW\n"), Ok(Request::Shot(Colours::Raw)));
    assert_eq!(one("shot  raw  \n"), Ok(Request::Shot(Colours::Raw)));
    assert_eq!(one("shot x\n"), Err(Refusal::Arguments(Command::Shot)));
    assert_eq!(one("shot raw raw\n"), Err(Refusal::Arguments(Command::Shot)));
    assert_eq!(Command::Shot.usage(), "raw or nothing");
}

#[test]
fn commands_without_arguments_refuse_one() {
    for c in [Command::Help, Command::Status, Command::Stats, Command::Bench] {
        let got = feed(&mut Console::new(), format!("{} now\n", c.name()).as_bytes());
        assert_eq!(got, vec![Err(Refusal::Arguments(c))]);
        assert_eq!(c.usage(), "no arguments");
    }
}

#[test]
fn lf_cr_and_crlf_each_end_one_request() {
    for s in ["help\n", "help\r", "help\r\n"] {
        assert_eq!(feed(&mut Console::new(), s.as_bytes()), vec![Ok(Request::Help(NoArg))], "{s:?}");
    }
    assert_eq!(feed(&mut Console::new(), b"help\r\nstatus\r\n").len(), 2);
}

#[test]
fn empty_and_blank_lines_give_nothing() {
    assert!(feed(&mut Console::new(), b"\n\r\n   \r\n\n").is_empty());
}

#[test]
fn only_a_space_separates_words() {
    let got = feed(&mut Console::new(), b"\t\n");
    assert_eq!(got, vec![Err(Refusal::Unknown(Word::new(b"\t")))]);   // shown as "?"
}

#[test]
fn edge_spaces_are_trimmed_and_repeated_spaces_split() {
    assert_eq!(feed(&mut Console::new(), b"   stats   \n"), vec![Ok(Request::Stats(NoArg))]);
    assert_eq!(feed(&mut Console::new(), b"shot     raw\n"), vec![Ok(Request::Shot(Colours::Raw))]);
}

#[test]
fn sixty_four_bytes_parse_sixty_five_refuse_then_recover() {
    let line = |n: usize| format!("{}help\n", " ".repeat(n - 4));
    assert_eq!(feed(&mut Console::new(), line(64).as_bytes()), vec![Ok(Request::Help(NoArg))]);
    let mut c = Console::new();
    assert_eq!(feed(&mut c, line(65).as_bytes()), vec![Err(Refusal::TooLong)]);
    assert_eq!(feed(&mut c, b"status\n"), vec![Ok(Request::Status(NoArg))]);
    let mut c = Console::new();
    let long = format!("{}\n", "x".repeat(500));
    assert_eq!(feed(&mut c, long.as_bytes()), vec![Err(Refusal::TooLong)], "one refusal per line");
}

#[test]
fn unknown_word_is_cut_to_sixteen_bytes() {
    let got = feed(&mut Console::new(), b"abcdefghijklmnopqrstuvwxyz\n");
    assert_eq!(got, vec![Err(Refusal::Unknown(Word::new(b"abcdefghijklmnop")))]);
    let Err(Refusal::Unknown(w)) = got[0] else { unreachable!() };
    assert_eq!(w.as_str(), "abcdefghijklmnop");
}

#[test]
fn unknown_word_answers_in_printable_ascii() {
    let got = feed(&mut Console::new(), b"\x1b[A\x00zz\n");
    let Err(Refusal::Unknown(w)) = got[0] else { panic!("{got:?}") };
    assert_eq!(w.as_str(), "?[A?zz");
    assert!(w.as_str().bytes().all(|b| (0x20..=0x7e).contains(&b)));
    assert_eq!(Word::new(&[0xff; 40]).as_str(), "????????????????");
}

#[test]
fn a_line_split_anywhere_parses_once() {
    let whole = b"  SHOT raw \r\n";
    for cut in 0..=whole.len() {
        let mut c = Console::new();
        let mut got = feed(&mut c, &whole[..cut]);
        got.extend(feed(&mut c, &whole[cut..]));
        assert_eq!(got, vec![Ok(Request::Shot(Colours::Raw))], "cut at {cut}");
    }
}

#[test]
fn the_table_is_the_one_list() {
    let names: Vec<_> = Command::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(names, ["help", "status", "stats", "bench", "shot"]);
    for c in Command::ALL {
        assert!(!c.about().is_empty() && c.about().len() <= 60, "{c:?}");
        assert!(c.name().len() <= 7, "help's column is 8 wide");
    }
}
```

  `mixed_case("status")` is `"sTaTuS"`. `only_a_space_separates_words` pins the grammar's "spaces" to 0x20: a tab is part of a word.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test console_parse_test` → FAIL: `chimera_core::console` not found.
- [ ] **Step 3: Implement** `console/mod.rs` and `pub mod console;` in `lib.rs`.
  - `commands!` is one `macro_rules!` taking `$($v:ident ($arg:ty) => $name:literal, $about:literal),+ $(,)?`. It generates both enums, `ALL`, `name`, `about`, `usage` (`<$arg as Arg>::USAGE`), `Request::command`, and a private `fn parse(word: &[u8], rest: Words) -> Result<Request, Refusal>` that matches `word` case-insensitively against each name and calls `<$arg as Arg>::parse(rest)`.
  - `Console::push`: LF or CR ends the line. While `len == MAX_LINE`, one more non-terminator byte sets `overflowed`. At the terminator: overflowed gives `TooLong`; a line that trims to empty gives `None`; else `parse`. Then reset.
  - No `unsafe`, no allocation, no panic path (`get` and slices bounded by `len`).
- [ ] **Step 4: Run** the test → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src/console chimera-core/src/lib.rs chimera-core/tests/console_parse_test.rs
git commit -m "The console parses request lines into typed requests from one command table"
```

---

### Task 3: `console`: `status`, and `Rung` in `nav`

**Files:**
- Create: `chimera-core/src/console/status.rs`; test `chimera-core/tests/console_status_test.rs`
- Modify: `chimera-core/src/console/mod.rs` (`mod status; pub use status::*;`), `chimera-core/src/ui/nav.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/components.rs`, `chimera-core/src/ui/settings/view.rs`

**Interfaces:**
- Produces:

```rust
// ui/nav.rs: a public mirror of the private Loc. `rung` matches Loc exhaustively,
// so a new Loc variant does not build until it has a Rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rung { Pages(PartId, PageAt), Mixer(PartId, MixPage), Fx(PartId, PageAt), Sound(PartId), Settings(SettingsAt) }
impl Location { pub fn rung(self) -> Rung; }

// ui/mod.rs
impl UiState {
    /// The page name the header draws (`FILTER`, `SENDS`, the exciter's name,
    /// with ` / A` or ` / B` on an ENV page); `None` on lists and the Sound rung.
    pub fn page_name(&self) -> Option<FmtBuf>;
}
// ui/settings/view.rs
impl Crumbs { pub fn parts(&self) -> &[Crumb]; }    // was private

// console/status.rs
pub fn state_word(s: ProjectStatus) -> &'static str;   // NEW, SAVED, MODIFIED
/// The status body, without the terminal line.
pub fn write_status(ui: &UiState, w: &mut impl core::fmt::Write) -> core::fmt::Result;
```

`draw_header` gets its name from `page_name` (one source). `header_text` keeps the context and warning parts and takes the name as an argument.

- [ ] **Step 1: Write the failing tests** in `console_status_test.rs` (with `mod screen;` for `feed`, `tap`, `to_leaf`, `Input`):

```rust
mod screen;
use chimera_core::console::{state_word, write_status};
use chimera_core::project::ProjectStatus;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::nav::{MixPage, Rung};
use chimera_hal::ButtonId;
use screen::{Input, feed, tap, to_leaf};

fn status(ui: &UiState) -> String { let mut s = String::new(); write_status(ui, &mut s).unwrap(); s }
fn line<'a>(s: &'a str, key: &str) -> &'a str {
    s.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(' '))).unwrap_or_else(|| panic!("no {key} in {s}"))
}

#[test]
fn status_has_its_lines_in_order() {
    let ui = Box::new(UiState::new());
    let keys: Vec<_> = status(&ui).lines().map(|l| l.split(' ').next().unwrap().to_string()).collect();
    assert_eq!(keys, ["firmware", "protocol", "project", "state", "part", "at"]);
    let s = status(&ui);
    assert_eq!(line(&s, "firmware"), format!("{} {}", chimera_core::ui::about_page::VERSION,
        chimera_core::ui::about_page::BUILD.to_ascii_lowercase()));
    assert_eq!(line(&s, "protocol"), "1");
    assert_eq!(line(&s, "project"), ui.project().meta().name().as_str());
    assert_eq!(line(&s, "part"), "1");
    assert!(s.ends_with('\n') && !s.contains('\r'));
}

#[test]
fn each_state_has_its_word() {
    assert_eq!(state_word(ProjectStatus::Pristine), "NEW");
    assert_eq!(state_word(ProjectStatus::Saved), "SAVED");
    assert_eq!(state_word(ProjectStatus::Modified), "MODIFIED");
}

#[test]
fn at_names_a_parts_page_by_its_header() {
    let mut ui = Box::new(UiState::new());
    ui.update(UiTick::for_test());
    assert!(matches!(ui.location().rung(), Rung::Pages(..)));
    let name = ui.page_name().expect("a page has a name");
    assert_eq!(line(&status(&ui), "at"), format!("PART 1 > {}", name.as_str()));
    tap(&mut ui, ButtonId::B3);
    assert_eq!(line(&status(&ui), "part"), "3");
    assert!(line(&status(&ui), "at").starts_with("PART 3 > "));
}

#[test]
fn at_names_the_mixer_and_the_fx() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B2));
    feed(&mut ui, Input::release(ButtonId::Mix));
    let Rung::Mixer(_, m) = ui.location().rung() else { panic!("{:?}", ui.location()) };
    let want = match m { MixPage::Part => "MIXER 2 > PART", MixPage::Sends => "MIXER 2 > SENDS" };
    assert_eq!(line(&status(&ui), "at"), want);
    for _ in 0..3 {
        if matches!(ui.location().rung(), Rung::Fx(..)) { break; }
        tap(&mut ui, ButtonId::Plus);
    }
    assert_eq!(line(&status(&ui), "at"), "FX > CHORUS");
}

#[test]
fn at_names_the_sound_rung() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B4));   // Part 4's mixer
    feed(&mut ui, Input::release(ButtonId::Mix));
    tap(&mut ui, ButtonId::Edit);                               // EDIT on the mixer: the Sound rung
    assert_eq!(ui.location().rung(), Rung::Sound(chimera_core::project::PartId::ALL[3]));
    assert_eq!(line(&status(&ui), "at"), "SOUND 4");
}

#[test]
fn at_in_settings_is_the_breadcrumb_as_drawn() {
    let mut ui = Box::new(UiState::new());
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"]);
    assert_eq!(line(&status(&ui), "at"), "SETTINGS > SYSTEM > DIAG > AUD LOAD");
    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    assert_eq!(line(&status(&ui), "at"), "SETTINGS > PERSONAL > THEME");
}

#[test]
fn a_long_breadcrumb_is_never_shortened() {
    let mut ui = Box::new(UiState::new());
    to_leaf(&mut ui, &["MIDI CONFIG", "CHANNELS"]);
    let at = line(&status(&ui), "at").to_string();
    assert!(!at.contains(".."), "{at}");
    assert!(at.starts_with("SETTINGS > "), "{at}");
}

#[test]
fn state_follows_an_edit() {
    let mut ui = Box::new(UiState::new());
    ui.update(UiTick::for_test());
    assert_eq!(line(&status(&ui), "state"), "NEW");
    feed(&mut ui, Input::press(ButtonId::Plus));                // a page with a bound cell
    feed(&mut ui, Input::turn(chimera_hal::EncoderId::A, 1));
    ui.update(UiTick::for_test());
    assert_eq!(line(&status(&ui), "state"), "MODIFIED");
}

#[test]
fn page_name_is_the_upper_case_page() {
    let mut ui = Box::new(UiState::new());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::release(ButtonId::Mix));
    let want = match ui.location().rung() { Rung::Mixer(_, MixPage::Part) => "PART", _ => "SENDS" };
    assert_eq!(ui.page_name().unwrap().as_str(), want);
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS"]);               // a list
    assert!(ui.page_name().is_none());
}
```

  `draw_header` reads the same `page_name` (Step 3), so the header and `at` have one source; the goldens (Step 4) prove the header did not change. The `chord`/`release` pair is the MIX+B*n* idiom `nav_test.rs` uses; copy it from there if it differs.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test console_status_test` → FAIL: `write_status`, `Rung`, `page_name` not found.
- [ ] **Step 3: Implement.**
  - `Rung` and `Location::rung` in `nav.rs`: `Loc::Pages(p, at) → Rung::Pages(p, at)`, `Part(p, m) → Mixer(p, m)`, `Fx(p, at) → Fx(p, at)`, `Sound(p, _) → Sound(p)`, `Settings(s) → Settings(s)`. No `_` arm.
  - `page_name`: the `named(true)` logic of `header_text` moves to `UiState::page_name`, using `page_def()`, the active model and the ENV A/B suffix (`title_type`'s rule, now over `(def, envs)` instead of `Frame`). `draw_header` calls it through the `Frame` (a `name: FmtBuf` field filled by `UiState::frame`).
  - `write_status`: six lines. `at` by `rung()`:
    - `Pages(p, _)` → `PART {p+1} > {page_name}`;
    - `Mixer(p, Part|Sends)` → `MIXER {p+1} > PART|SENDS`;
    - `Fx(_, _)` → `FX > {page_name}`;
    - `Sound(p)` → `SOUND {p+1}`;
    - `Settings(_)` → `ui.crumbs()`'s `parts()` joined with ` > ` (each crumb through its `Display`).
- [ ] **Step 4: Run** the test → PASS; `cargo test -p chimera-core --test screen_golden_test --test screen_atlas_test` → PASS (the header is unchanged); `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "The console's status says where the UI is, from the header's own page name"
```

---

### Task 4: `console`: `shot`, `Out` and the stall

**Files:**
- Create: `chimera-core/src/console/shot.rs`; test `chimera-core/tests/console_shot_test.rs`
- Modify: `chimera-core/src/console/mod.rs` (`mod shot; pub use shot::*;`)

**Interfaces:**
- Produces:

```rust
/// The host stopped taking bytes for STALL_MS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Stalled;
/// Where answers go.
pub trait Out { fn put(&mut self, bytes: &[u8]) -> Result<(), Stalled>; }
/// The screen as the display holds it, and the palette it flushes through.
#[derive(Clone, Copy)] pub struct Frame<'a> { pub fb: &'a [u16; FB_SIZE], pub palette: Palette }
pub const SHOT_HEADER: &str = "SHOT 240 320 rgb565be 153600\n";
/// The header, the body row by row (one 480-byte row on the stack at a time), then `OK`.
pub fn write_shot(f: Frame<'_>, c: Colours, out: &mut impl Out) -> Result<(), Stalled>;
```

`SHOT_HEADER`'s numbers are checked at compile time against `SCREEN_WIDTH`, `SCREEN_HEIGHT` and `FB_SIZE * 2` (a `const _: () = assert!(..)` over a `const fn` that formats nothing: compare the literal's digits with `concat!`-free arithmetic, or build the header from the constants with `write!` and test it in Step 1).

- [ ] **Step 1: Write the failing tests** in `console_shot_test.rs`:

```rust
use chimera_core::console::{Colours, Frame, Out, SHOT_HEADER, Stalled, write_shot};
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::{Accent, Palette, ThemeSettings};
use chimera_hal::FB_SIZE;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};

struct Sink(Vec<u8>);
impl Out for Sink { fn put(&mut self, b: &[u8]) -> Result<(), Stalled> { self.0.extend_from_slice(b); Ok(()) } }
/// Takes `left` bytes, then stalls; counts puts after the stall.
struct Stalls { got: Vec<u8>, left: usize, after: usize }
impl Out for Stalls {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        if self.left == 0 { self.after += 1; return Err(Stalled); }
        let n = b.len().min(self.left);
        self.got.extend_from_slice(&b[..n]); self.left -= n;
        if n < b.len() { Err(Stalled) } else { Ok(()) }
    }
}
fn raw(c: embedded_graphics::pixelcolor::Rgb565) -> u16 { RawU16::from(c).into_inner() }
fn fb() -> Box<[u16; FB_SIZE]> {
    let mut fb = Box::new([0u16; FB_SIZE]);
    for (i, p) in fb.iter_mut().enumerate() { *p = (i as u16).wrapping_mul(2654) ^ 0x5a5a; }
    fb[0] = raw(theme::ACCENT); fb[1] = raw(theme::BG); fb[FB_SIZE - 1] = 0x1234;
    fb
}
fn amber() -> Palette { ThemeSettings { accent: Accent::Amber, ..ThemeSettings::DEFAULT }.palette() }

#[test]
fn shot_is_header_body_ok() {
    let fb = fb();
    let mut s = Sink(Vec::new());
    write_shot(Frame { fb: &fb, palette: Palette::IDENTITY }, Colours::Theme, &mut s).unwrap();
    assert_eq!(SHOT_HEADER, format!("SHOT {} {} rgb565be {}\n",
        chimera_hal::SCREEN_WIDTH, chimera_hal::SCREEN_HEIGHT, FB_SIZE * 2));
    assert!(s.0.starts_with(SHOT_HEADER.as_bytes()));
    assert_eq!(s.0.len(), SHOT_HEADER.len() + 153_600 + 3);
    assert!(s.0.ends_with(b"OK\n"));
}

#[test]
fn pixels_go_big_endian_through_the_palette() {
    let fb = fb();
    let pal = amber();
    assert_ne!(pal, Palette::IDENTITY);
    let mut s = Sink(Vec::new());
    write_shot(Frame { fb: &fb, palette: pal }, Colours::Theme, &mut s).unwrap();
    let body = &s.0[SHOT_HEADER.len()..SHOT_HEADER.len() + 153_600];
    for (i, px) in body.chunks(2).enumerate() {
        assert_eq!(u16::from_be_bytes([px[0], px[1]]), pal.map_raw(fb[i]), "pixel {i}");
    }
    assert_eq!(u16::from_be_bytes([body[0], body[1]]), raw(pal.accent));
}

#[test]
fn shot_raw_ignores_the_palette() {
    let fb = fb();
    let mut s = Sink(Vec::new());
    write_shot(Frame { fb: &fb, palette: amber() }, Colours::Raw, &mut s).unwrap();
    let body = &s.0[SHOT_HEADER.len()..SHOT_HEADER.len() + 153_600];
    for (i, px) in body.chunks(2).enumerate() {
        assert_eq!(u16::from_be_bytes([px[0], px[1]]), fb[i], "pixel {i}");
    }
}

#[test]
fn a_stall_mid_shot_stops_without_a_terminal_line() {
    let fb = fb();
    for left in [0, 10, SHOT_HEADER.len(), 1000, 153_600, SHOT_HEADER.len() + 153_600 + 1] {
        let mut s = Stalls { got: Vec::new(), left, after: 0 };
        let r = write_shot(Frame { fb: &fb, palette: Palette::IDENTITY }, Colours::Theme, &mut s);
        assert_eq!(r, Err(Stalled), "left {left}");
        assert!(!s.got.ends_with(b"OK\n"), "left {left}");
        assert_eq!(s.after, 0, "nothing is put after a stall (left {left})");
    }
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test console_shot_test` → FAIL: `write_shot` not found.
- [ ] **Step 3: Implement** `shot.rs`: put the header; for each of 320 rows fill `[u8; 480]` (`map_raw` for `Theme`, the pixel for `Raw`, `to_be_bytes`) and put it, returning at the first `Err`; put `OK\n`.
- [ ] **Step 4: Run** the test → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src/console chimera-core/tests/console_shot_test.rs
git commit -m "The console streams the screen row by row, in THEME's colours or raw"
```

---

### Task 5: `console`: `answer`, `Unit`, help, `stats`, `bench` and the refusals

**Files:**
- Create: `chimera-core/src/console/answer.rs`; test `chimera-core/tests/console_answer_test.rs`
- Modify: `chimera-core/src/console/mod.rs` (`mod answer; pub use answer::*;`)

**Interfaces:**
- Consumes: Task 2's `Request`, `Refusal`, `Command`; Task 3's `write_status`; Task 4's `Out`, `Stalled`, `Frame`, `write_shot`.
- Produces:

```rust
#[derive(Clone, Copy, Debug)]
pub struct Stats { pub audio: AudioStats, pub loop_avg_us: u32, pub loop_peak_us: u32 }

/// What a shell can tell. `None`: not in this build.
pub trait Unit {
    fn ui(&self) -> &UiState;
    fn stats(&mut self) -> Option<Stats>;   // reading resets the loop timer
    fn bench(&self) -> Option<&str>;
    fn frame(&self) -> Frame<'_>;
}

/// The whole answer, terminal line included. Text goes out through a
/// `core::fmt::Write` adapter over `out`: no response buffer.
pub fn answer(req: Result<Request, Refusal>, unit: &mut impl Unit, out: &mut impl Out) -> Result<(), Stalled>;
```

  `answer` matches `Request` with no `_` arm. `Unit::stats` is called only for `Request::Stats`, so a `status` never resets the loop timer.

- [ ] **Step 1: Write the failing tests** in `console_answer_test.rs`. A `Fake` unit holds a `Box<UiState>`, a framebuffer, `stats: Option<Stats>`, `bench: Option<String>` and a `stats_reads` counter. `Sink` is Task 4's.

```rust
fn ask(u: &mut Fake, line: &str) -> String {
    let mut c = Console::new();
    let mut s = Sink(Vec::new());
    for &b in line.as_bytes() { if let Some(r) = c.push(b) { answer(r, u, &mut s).unwrap(); } }
    String::from_utf8(s.0).unwrap()
}

#[test]
fn help_lists_the_table_in_order() {
    let mut u = Fake::new();
    let mut want = String::from("chimera console 1\n");
    for c in Command::ALL { want += &format!("{:<8}{}\n", c.name(), c.about()); }
    want += "OK\n";
    assert_eq!(ask(&mut u, "help\n"), want);
}

#[test]
fn each_refusal_is_one_err_line() {
    let mut u = Fake::new();
    assert_eq!(ask(&mut u, "frob\n"), "ERR unknown command frob, try help\n");
    assert_eq!(ask(&mut u, "abcdefghijklmnopqrstuvwxyz\n"), "ERR unknown command abcdefghijklmnop, try help\n");
    assert_eq!(ask(&mut u, "status now\n"), "ERR status takes no arguments\n");
    assert_eq!(ask(&mut u, "shot x\n"), "ERR shot takes raw or nothing\n");
    assert_eq!(ask(&mut u, &format!("{}\n", "y".repeat(65))), "ERR line too long, 64 max\n");
}

#[test]
fn none_from_the_unit_is_one_err_line() {
    let mut u = Fake::new();          // stats None, bench None
    assert_eq!(ask(&mut u, "stats\n"), "ERR stats is not in this build\n");
    assert_eq!(ask(&mut u, "bench\n"), "ERR bench is not in this build\n");
}

#[test]
fn stats_reads_audio_load_and_the_loop() {
    let mut u = Fake::new();
    let mut a = AudioStats::default();          // or the constructor nav-core has
    (a.load_avg, a.load_peak, a.overruns, a.desyncs, a.sources, a.stack_used) = (23, 41, 0, 2, 2, 12_288);
    a.drops = [5, 7];
    u.stats = Some(Stats { audio: a, loop_avg_us: 812, loop_peak_us: 4210 });
    assert_eq!(ask(&mut u, "stats\n"),
        "load_pct 23\npeak_pct 41\noverruns 0\ndrops 5 7\ndesyncs 2\nstack_bytes 12288\nloop_avg_us 812\nloop_peak_us 4210\nOK\n");
    a.sources = 1;
    u.stats = Some(Stats { audio: a, loop_avg_us: 0, loop_peak_us: 0 });
    assert!(ask(&mut u, "stats\n").contains("\ndrops 5\n"), "one count per source");
}

#[test]
fn only_stats_reads_the_stats() {
    let mut u = Fake::new();
    for l in ["help\n", "status\n", "bench\n", "shot\n", "nope\n"] { ask(&mut u, l); }
    assert_eq!(u.stats_reads, 0);
    ask(&mut u, "stats\n");
    assert_eq!(u.stats_reads, 1);
}

#[test]
fn bench_is_its_text_then_ok() {
    let mut u = Fake::new();
    u.bench = Some("# VOICES\nALG 1  12 24 36\n".into());
    assert_eq!(ask(&mut u, "bench\n"), "# VOICES\nALG 1  12 24 36\nOK\n");
}

#[test]
fn status_is_its_lines_then_ok() {
    let mut u = Fake::new();
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    assert_eq!(ask(&mut u, "status\n"), want + "OK\n");
}

#[test]
fn shot_answers_through_the_units_frame() {
    let mut u = Fake::new();
    let mut s = Sink(Vec::new());
    answer(Ok(Request::Shot(Colours::Raw)), &mut u, &mut s).unwrap();
    assert_eq!(s.0.len(), SHOT_HEADER.len() + 153_600 + 3);
}

#[test]
fn random_streams_never_panic_and_end_each_answer_once() {
    // xorshift, as codec_fuzz_test's Rng; 5 000 streams of up to 200 bytes,
    // drawn 70 % from command words, "raw", spaces, CR and LF, 30 % any byte.
    let mut rng = Rng(0x5eed_cafe);
    let mut u = Fake::new();
    u.stats = Some(Stats { audio: Default::default(), loop_avg_us: 1, loop_peak_us: 2 });
    for _ in 0..5_000 {
        let mut c = Console::new();
        for b in rng.stream() {
            let Some(r) = c.push(b) else { continue };
            let mut s = Sink(Vec::new());
            answer(r, &mut u, &mut s).unwrap();
            let text = strip_shot_body(&s.0);       // drops exactly 153 600 bytes after a SHOT header
            let lines: Vec<&str> = text.lines().collect();
            let last = *lines.last().expect("an answer");
            assert!(last == "OK" || last.starts_with("ERR "), "{text}");
            let terminals = lines.iter().filter(|l| **l == "OK" || l.starts_with("ERR ")).count();
            assert_eq!(terminals, 1, "{text}");
            assert!(text.bytes().all(|b| b == b'\n' || (0x20..=0x7e).contains(&b)), "{text:?}");
        }
    }
}
```

  `Fake::new()` boots `UiState::new()` in a `Box` and fills the framebuffer with `0xBEEF`. `AudioStats`' private window fields mean the test builds one through whatever public constructor or `Default` `nav-core` has; if neither exists, add `impl Default for AudioStats` (zeroes, `SiliconRev`'s and `ResetCause`'s defaults) in this task.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test console_answer_test` → FAIL: `answer` not found.
- [ ] **Step 3: Implement** `answer.rs`:
  - a private `Text<'a, O: Out> { out: &'a mut O, stalled: bool }` implementing `fmt::Write`. `write_str` puts, and on `Stalled` records it and returns `fmt::Error`. The callers map that `fmt::Error` back to `Stalled`;
  - `Err(Unknown(w))` → `ERR unknown command {w}, try help`; `Err(Arguments(c))` → `ERR {name} takes {usage}`; `Err(TooLong)` → `ERR line too long, {MAX_LINE} max`;
  - `Help` → `chimera console {PROTOCOL}`, then `{name:<8}{about}` per `ALL`;
  - `Status` → `write_status`; `Stats` → the eight lines, with `drops` the first `sources` counts; `Bench` → the text as it is, with a `\n` added if it lacks one;
  - `Shot(c)` → `write_shot(unit.frame(), c, out)`;
  - every arm but `Shot` ends `OK\n`, and `None` from the unit gives `ERR {name} is not in this build`.
- [ ] **Step 4: Run** the test → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests/console_answer_test.rs
git commit -m "The console answers every request in full, ending in one OK or ERR line"
```

---

### Task 6: `console`: the shells' pure helpers

**Files:**
- Create: `chimera-core/src/console/shell.rs`; test `chimera-core/tests/console_shell_test.rs`
- Modify: `chimera-core/src/console/mod.rs` (`mod shell; pub use shell::*;`)

**Interfaces:**
- Produces:

```rust
/// The UI loop's time between loop tops, since the last `take`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopTimer { /* sum_us: u64, laps: u32, peak_us: u32, skip: bool */ }
impl LoopTimer {
    pub const fn new() -> Self;
    /// One loop top `us` after the last; dropped if `answered` was called since.
    pub fn lap(&mut self, us: u32);
    /// This iteration answered a request: its lap is not timed.
    pub fn answered(&mut self);
    /// (avg, peak) in µs, then reset. (0, 0) with no laps.
    pub fn take(&mut self) -> (u32, u32);
}

/// Report text in a fixed buffer. The last TRUNCATED.len() bytes are kept
/// back, so a report that runs out of room always ends in `# TRUNCATED`.
pub struct Report<const N: usize> { /* buf: [u8; N], len: usize, cut: bool */ }
impl<const N: usize> Report<N> {
    pub const TRUNCATED: &'static str = "# TRUNCATED\n";
    pub const fn new() -> Self;                // N > TRUNCATED.len(), checked at compile time
    pub fn heading(&mut self, title: &str);    // `# <title>`
    pub fn line(&mut self, text: &str);        // one screen line; a line that won't fit cuts the report
    pub fn as_str(&self) -> &str;
}

/// The chip's 96-bit UID as 24 uppercase hex digits, word 0 first, each word big-endian.
pub fn serial_hex(uid: [u32; 3]) -> [u8; 24];
```

- [ ] **Step 1: Write the failing tests** in `console_shell_test.rs`:

```rust
use chimera_core::console::{LoopTimer, Report, serial_hex};

#[test]
fn the_timer_averages_and_peaks_then_resets() {
    let mut t = LoopTimer::new();
    assert_eq!(t.take(), (0, 0));
    for us in [800, 900, 4200, 700] { t.lap(us); }
    assert_eq!(t.take(), (1650, 4200));
    assert_eq!(t.take(), (0, 0), "reading resets");
}

#[test]
fn an_answered_iteration_is_not_timed() {
    let mut t = LoopTimer::new();
    t.lap(800);
    t.answered();
    t.lap(300_000);                 // the shot's iteration
    t.lap(900);
    assert_eq!(t.take(), (850, 900));
}

#[test]
fn a_long_run_does_not_overflow() {
    let mut t = LoopTimer::new();
    for _ in 0..10_000_000 { t.lap(u32::MAX / 2); }
    assert_eq!(t.take(), (u32::MAX / 2, u32::MAX / 2));
}

#[test]
fn a_report_is_headings_and_lines() {
    let mut r = Report::<64>::new();
    r.heading("VOICES");
    r.line("ALG 1  12 24");
    assert_eq!(r.as_str(), "# VOICES\nALG 1  12 24\n");
}

#[test]
fn a_full_report_ends_in_truncated() {
    let mut r = Report::<40>::new();
    for i in 0..10 { r.line(&format!("line {i}")); }
    assert!(r.as_str().ends_with("# TRUNCATED\n"), "{}", r.as_str());
    assert!(r.as_str().len() <= 40);
    let before = r.as_str().to_string();
    r.line("more");
    assert_eq!(r.as_str(), before, "nothing after the cut");
    assert_eq!(r.as_str().matches("# TRUNCATED").count(), 1);
}

#[test]
fn a_report_that_fits_exactly_is_not_cut() {
    let mut r = Report::<{ 6 + 12 }>::new();  // "abcde\n" plus the reserve
    r.line("abcde");
    assert_eq!(r.as_str(), "abcde\n");
}

#[test]
fn serial_is_24_uppercase_hex_digits() {
    assert_eq!(&serial_hex([0x0012_00AB, 0xDEAD_BEEF, 0x0000_0001]), b"001200ABDEADBEEF00000001");
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test console_shell_test` → FAIL: not found.
- [ ] **Step 3: Implement** `shell.rs`. `LoopTimer` sums in `u64`. `Report::new` has `const { assert!(N > Self::TRUNCATED.len()) }`. A line goes in only if it and the reserve both fit (`len + line + 1 + TRUNCATED.len() <= N`), except that the reserve is released when nothing more is written: `as_str` returns the buffer, and the cut writes `TRUNCATED` into the reserve once.
- [ ] **Step 4: Run** the test → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src/console chimera-core/tests/console_shell_test.rs
git commit -m "The loop timer, the bench report and the serial number are pure and tested"
```

---

### Task 7: The desktop socket shell

**Files:**
- Create: `chimera-desktop/src/console.rs`
- Modify: `chimera-desktop/src/main.rs`, `chimera-desktop/src/display.rs`

**Interfaces:**
- Consumes: Task 5's `Unit`, `answer`; Task 4's `Out`, `Stalled`, `Frame`; Task 2's `Console`, `STALL_MS`.
- Produces:

```rust
pub const ADDR: &str = "127.0.0.1:7341";
pub struct SocketConsole { /* listener: TcpListener, client: Option<TcpStream>, core: Console */ }
impl SocketConsole {
    pub fn bind(addr: &str) -> std::io::Result<Self>;   // non-blocking listener
    pub fn local_addr(&self) -> std::net::SocketAddr;
    /// The frame's top: accept a waiting client (it replaces the old one),
    /// read byte by byte into `Console`, answer at most one request. True if it answered.
    pub fn service(&mut self, unit: &mut impl Unit) -> bool;
}
pub struct DeskUnit<'a> { pub ui: &'a UiState, pub frame: Frame<'a> }   // stats, bench: None
struct StreamOut<'a> { s: &'a mut TcpStream }   // WouldBlock retried until STALL_MS with no progress
// display.rs
impl DesktopDisplay { pub fn frame(&self) -> Frame<'_>; }    // (&fb, palette); BRIGHT is not in it
```

  `main.rs`: `let mut console = match SocketConsole::bind(console::ADDR) { Ok(c) => Some(c), Err(_) => { eprintln!("console: {} busy, console off", console::ADDR); None } };` before the loop. The loop's first statement: `if let Some(c) = console.as_mut() { c.service(&mut DeskUnit { ui: &ui, frame: display.frame() }); }`.

- [ ] **Step 1: Write the failing tests** in `console.rs`'s `#[cfg(test)] mod tests`. Each binds `127.0.0.1:0`. A `client(&SocketConsole) -> TcpStream` helper connects with a 2 s read timeout. `fb_unit()` gives a `DeskUnit` over a `Box<UiState>` and a framebuffer of known pixels with a non-identity palette.

```rust
#[test]
fn status_after_a_key_walk_names_the_place() {
    let mut desk = crate::qa::Desk::launch(tempdir());
    desk.tap(minifb::Key::M);                   // MENU in the sim's key map; opens SETTINGS on release
    let fb = Box::new([0u16; FB_SIZE]);
    let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
    let mut c = client(&con);
    c.write_all(b"status\n").unwrap();
    let unit = &mut DeskUnit { ui: &desk.ui, frame: Frame { fb: &fb, palette: Palette::IDENTITY } };
    assert!(serve_until_answered(&mut con, unit));
    let text = read_until_terminal(&mut c);
    assert!(text.contains("\nat SETTINGS\n"), "{text}");
    assert!(text.ends_with("OK\n"));
}

#[test]
fn shot_matches_the_framebuffer_through_the_palette() {
    let (ui, fb, pal) = fb_unit();
    let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
    let mut c = client(&con);
    c.write_all(b"shot\n").unwrap();
    assert!(serve_until_answered(&mut con, &mut DeskUnit { ui: &ui, frame: Frame { fb: &fb, palette: pal } }));
    let body = read_shot(&mut c);                 // checks the header, reads 153 600, then "OK\n"
    for (i, px) in body.chunks(2).enumerate() { assert_eq!(u16::from_be_bytes([px[0], px[1]]), pal.map_raw(fb[i])); }
}

#[test]
fn shot_raw_matches_the_framebuffer() { /* as above with "shot raw\n" and fb[i] */ }

#[test]
fn two_requests_in_one_write_answer_on_two_frames() {
    let (ui, fb, pal) = fb_unit();
    let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
    let mut c = client(&con);
    c.write_all(b"help\nstatus\n").unwrap();
    let mut unit = DeskUnit { ui: &ui, frame: Frame { fb: &fb, palette: pal } };
    assert!(serve_until_answered(&mut con, &mut unit));
    let first = read_until_terminal(&mut c);
    assert!(first.starts_with("chimera console 1\n"), "{first}");
    assert!(con.service(&mut unit), "the second request waited, and the next frame answers it");
    assert!(read_until_terminal(&mut c).starts_with("firmware "));
}

#[test]
fn a_new_client_replaces_the_old() {
    let (ui, fb, pal) = fb_unit();
    let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
    let _old = client(&con);
    let mut new = client(&con);
    new.write_all(b"help\n").unwrap();
    assert!(serve_until_answered(&mut con, &mut DeskUnit { ui: &ui, frame: Frame { fb: &fb, palette: pal } }));
    assert!(read_until_terminal(&mut new).ends_with("OK\n"));
}

#[test]
fn a_client_that_stops_reading_stalls_a_shot_in_250_ms() {
    // The client never reads. Loopback buffers hold some shots; ask for
    // shots until one answer can't go out, then time that one.
    let (ui, fb, pal) = fb_unit();
    let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
    let mut c = client(&con);
    let mut unit = DeskUnit { ui: &ui, frame: Frame { fb: &fb, palette: pal } };
    let mut stalled = None;
    for _ in 0..200 {
        c.write_all(b"shot\n").unwrap();
        let t = Instant::now();
        serve_until_answered(&mut con, &mut unit);
        if t.elapsed() >= Duration::from_millis(250) { stalled = Some(t.elapsed()); break; }
    }
    let took = stalled.expect("a reader that never reads stalls a shot");
    assert!(took < Duration::from_millis(1500), "{took:?}");
}

#[test]
fn a_taken_port_leaves_the_console_off() {
    let first = SocketConsole::bind("127.0.0.1:0").unwrap();
    assert!(SocketConsole::bind(&first.local_addr().to_string()).is_err());
}
```

  `serve_until_answered` calls `service` up to 100 times, 5 ms apart, until it returns true (the client's bytes take a moment to arrive). After a stall the shell drops the client (Step 3), so the stall test's `write_all` after the stalled shot is never reached.
- [ ] **Step 2: Run** `cargo test -p chimera-desktop console` → FAIL: module not found.
- [ ] **Step 3: Implement** `console.rs`, `DesktopDisplay::frame` and the hook in `main.rs` (`mod console;`). `StreamOut::put` writes in a loop: `Ok(n)` advances and resets the stall clock; `WouldBlock` sleeps 1 ms; past `STALL_MS` without progress, or on any other error (the client left), it drops the client and returns `Stalled`.
- [ ] **Step 4: Run** the tests → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-desktop/src
git commit -m "The desktop sim answers the console on 127.0.0.1:7341"
```

---

### Task 8: The USB shell on the chip; `usb-console` on by default; the bench's report

**Files:**
- Modify: `chimera-stm32/src/usb.rs`, `chimera-stm32/src/main.rs`, `chimera-stm32/src/display.rs`, `chimera-stm32/src/bench.rs`, `chimera-stm32/src/priority.rs`, `chimera-stm32/Cargo.toml`, `Justfile` (Task 1's two temporary lines go); this plan (`## Measured`)

**Interfaces:**
- Consumes: Task 1's `UsbParts`, `init`, `Usb`; Task 5's `Unit`, `Stats`, `answer`; Task 6's `LoopTimer`, `Report`, `serial_hex`; Task 4's `Out`, `Stalled`, `Frame`.
- Produces:

```rust
// usb.rs
impl Usb {
    /// The loop's top: time the lap, poll, read byte by byte into `Console`,
    /// answer at most one request.
    pub fn service(&mut self, unit: &mut impl Unit);
    pub fn take_loop_time(&mut self) -> (u32, u32);        // ChipUnit::stats calls it
}
struct UsbOut<'a> { dev: &'a mut UsbDevice<..>, serial: &'a mut SerialPort<..>, cpu_hz: u32 }  // impl Out
pub struct ChipUnit<'a> {
    pub ui: &'a UiState,
    pub stats: Option<&'a mut Reader<AudioStats>>,
    pub loop_time: &'a mut LoopTimer,
    pub bench: Option<&'static str>,
    pub frame: Frame<'a>,
}
// display.rs
impl Stm32Display<..> { pub fn frame(&self) -> Frame<'_>; }
// bench.rs (feature "bench")
pub const BENCH_TEXT_LEN: usize = 6144;
pub fn take_report() -> Option<&'static mut Report<BENCH_TEXT_LEN>>;   // take-once, AXI SRAM .bss
pub fn run(display: &mut impl ChimeraDisplay, clocks: Clocks, project: &mut Project,
           report: &mut Report<BENCH_TEXT_LEN>);
```

  `Usb` owns the `Console` and the `LoopTimer`. `ChipUnit` borrows the timer back out of `Usb` for the one call, so `service`'s signature is `service(&mut self, ui, stats, bench, frame)` if the borrow checker won't split it; the observable behaviour is the spec's either way.

- [ ] **Step 1: Wire the serial number.** `init` reads the UID (the HAL's `signature::Uid` if 0.16 exports it, else three `read_volatile`s at `0x1FF1_E800`, `0x1FF1_E804`, `0x1FF1_E808` with a `// SAFETY:` note: read-only system memory, always mapped), formats it once with `serial_hex` into a take-once `static [u8; 24]`, and passes it to `.serial_number(..)` (`core::str::from_utf8` of hex digits, never unchecked).
- [ ] **Step 2: `service` and `UsbOut`** as the spec's § Polling says. `UsbOut::put` loops `serial.write`, and on `WouldBlock` or a 0-byte write it calls `dev.poll(&mut [serial])`. The stall clock is DWT cycles (`STALL_MS * cpu_hz / 1000`, compared with `wrapping_sub`) and restarts whenever bytes are taken. After the whole answer, `serial.flush()` is pumped the same way. The lap is `DWT` cycles since the last loop top, divided by `cpu_hz / 1_000_000`. `answered()` is called before the answer goes out.
- [ ] **Step 3: `ChipUnit`.** `stats()` returns `None` without `perf-probe`. Otherwise it returns `*reader.read()` with `stack_used = probe::stack_used()`, as the loop does for the page, plus `loop_time.take()`. `bench()` is `Some(report.as_str())` in bench builds, else `None`. `frame()` is `display.frame()`.
- [ ] **Step 4: The hook.** In `synth()`, Task 1's `usb.poll();` becomes `usb.service(&mut ChipUnit { .. });`, still the loop's first statement, before `controls.snapshot()`. That is the snapshot point: every path through the previous iteration (the `continue` after BUSY or a toast, the recolour's full flush, the dirty-region flush) has flushed before the loop comes back to the top. `apply_theme` sets the palette in the same iteration as the flush, so `frame().palette` is the palette the panel was last sent.
- [ ] **Step 5: The bench's report.** `bench::run` takes the report. Each line that `show`, `show_routing`, `voice_row` and `show_memory` draw with `draw::text` also goes to `report.line(..)` from the same `FmtBuf`, and each screen title to `report.heading(..)`. `synth()` takes the report with `take_report()` before `bench::run`, and keeps `&'static str` of it for `ChipUnit::bench`.
- [ ] **Step 6: `priority.rs`** gains one comment line: "USB has no level: it is polled from the UI loop (ADR 0068)."
- [ ] **Step 7: Default on.** `default = ["midi-din", "perf-probe", "usb-console"]`; remove Task 1's two temporary `Justfile` lines (`check` and `clippy` now build it through the default sets).
- [ ] **Step 8: Measure** as in Task 1 Step 2: `--no-default-features --features midi-din,perf-probe` (base) against the default build (with). Record the whole flash cost, RAM cost, and the bench build's extra RAM under `## Measured`. **STOP above 24 576 bytes**, as Task 1 Step 6 (commit the numbers only, revert the rest of this task, report).
- [ ] **Step 9: Run** `just check` → PASS (it includes `just stack-check` over default, no-default, bench and sd-probe).
- [ ] **Step 10: Commit**

```bash
git status --short
git add chimera-stm32 Justfile docs/superpowers/plans/2026-10-02-usb-console.md
git commit -m "The unit answers the console over USB from its UI loop, and the bench keeps its text for it"
```

---

### Task 9: Enter DFU from the firmware: OS UPGRADE and `dfu`

**Files:**
- Create: `chimera-core/src/boot.rs`; tests `chimera-core/tests/boot_test.rs`, `chimera-core/tests/enter_dfu_test.rs`; `chimera-stm32/src/dfu.rs`
- Modify:
  - `chimera-core/src/lib.rs` (`pub mod boot;`), `chimera-core/src/project/guard.rs` (`RomDfu` sealed and `Witnessed`);
  - `chimera-core/src/ui/settings/tree.rs` (OS UPGRADE → `Act::EnterDfu`), `ui/settings/part.rs` (`part_cmd`'s `None` arm), `ui/settings/prompt.rs` (`DfuAnswer`, `EnterDfu`), `ui/settings/replace.rs` (the `commits!` row), `ui/settings/mod.rs` (`Ask::EnterDfu`, `Answered::EnterDfu`, the `ask_kinds!` row), `ui/settings/leaves.rs` and `ui/block_registry.rs` (`UPDATES_BLOCKS`, `UPDATES_LEAF`, `SYS_UPDATES` and its `FIXED` entry go), `ui/mod.rs` (`run_act`'s arm, the answer, `take_dfu`);
  - `chimera-core/src/console/mod.rs` (the `dfu` row), `console/answer.rs` (`Unit::dfu`, the arm);
  - `chimera-core/tests/console_parse_test.rs` (the table's list), `console_answer_test.rs` (`Fake::dfu`), `page_block_test.rs` (`SYS_UPDATES` goes), `screen_atlas_test.rs` (`prompt`'s `EnterDfu` arm), `prompt_naming_test.rs` (`every_prompt_fits` covers `EnterDfu`);
  - `chimera-desktop/src/console.rs` (`DeskUnit::dfu` → `None`), `chimera-desktop/src/main.rs` (a menu yes: one stderr line);
  - `chimera-stm32/src/main.rs` (the check at the top of `main`; `boot(cp, dp)`; `rtc` into `SynthParts`; `take_dfu` and the console's `dfu` in the loop), `chimera-stm32/src/usb.rs` (`ChipUnit::dfu`, `service`'s return);
  - `docs/screens/` (`just screens`); this plan (`## Measured`)

**Interfaces:**
- Consumes: Task 2's `commands!`; Task 5's `Unit`, `answer`; Task 7's `DeskUnit`; Task 8's `ChipUnit`, `Usb::service`; `ui::settings::replace::said` and `commits!` (`nav-core`).
- Produces:

```rust
// chimera-core/src/boot.rs: pure, no I/O
/// "DFU!". 0, a power-on's garbage and every other value boot the synth.
pub const DFU_MAGIC: u32 = 0x4446_5521;
/// ST AN2606, STM32H74x/75x: the system memory bootloader's vector table.
pub const ROM_DFU_BASE: u32 = 0x1FF0_9800;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction { Synth, RomDfu }
pub const fn after_reset(marker: u32) -> BootAction;
/// What the DFU prompt's yes confirms. `Witnessed<Witness = ()>`: nothing on the card is at stake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;

// ui/settings/prompt.rs
answers!(DfuAnswer { EnterDfu => "ENTER DFU", Cancel => "CANCEL" } Two);
pub struct EnterDfu;            // Prompt: "ENTER DFU?" / "PLAY STOPS UNTIL FLASHED OR POWER-CYCLED"
// ui/settings/replace.rs
commits!( ...; DfuAnswer => EnterDfu, RomDfu; );
// ui/mod.rs
impl UiState {
    /// The DFU prompt's yes, once.
    pub fn take_dfu(&mut self) -> Option<Said<RomDfu>>;
}

// console/mod.rs: one more row
    Dfu    (NoArg)   => "dfu",    "restart into the ROM loader for just flash",
// console/answer.rs
pub trait Unit { /* ... */
    /// Arms the restart, made once `OK` is out. `None`: not in this build.
    fn dfu(&mut self) -> Option<()>;
}

// chimera-stm32/src/dfu.rs
/// The marker: RTC_BKP0R. Fallback if U10 finds it clobbered: the last word of D3 SRAM4, 0x3800_FFFC.
pub enum DfuFrom { Menu(Said<RomDfu>), Console }
/// The top of `main`: enable RTCAPBEN, read BKP0R, set DBP, clear BKP0R, then jump on `RomDfu`.
pub fn after_reset(cp: &mut cortex_m::Peripherals, rcc: &pac::RCC, pwr: &pac::PWR, rtc: &pac::RTC);
/// Writes `DFU_MAGIC` to BKP0R, then `SCB::sys_reset()`.
pub fn enter(rtc: &pac::RTC, from: DfuFrom) -> !;
// usb.rs
impl Usb { pub fn service(&mut self, unit: &mut impl Unit) -> Served; }   // Served { Idle, Answered, Dfu }
```

  `Served::Dfu` means `dfu` was answered and its `OK` was flushed (`serial.flush()` pumped, as every answer is). `Served::Answered` replaces Task 8's bare `answered()` path. If Task 8 left `service` returning `()`, this task adds the return and changes no other line of it.

- [ ] **Step 1: Write the failing tests.**
  - `chimera-core/tests/boot_test.rs`:

```rust
use chimera_core::boot::{BootAction, DFU_MAGIC, ROM_DFU_BASE, after_reset};

#[test]
fn only_the_magic_enters_dfu() {
    assert_eq!(after_reset(DFU_MAGIC), BootAction::RomDfu);
    for m in [0, u32::MAX, DFU_MAGIC ^ 1, DFU_MAGIC.swap_bytes(), DFU_MAGIC.rotate_left(8)] {
        assert_eq!(after_reset(m), BootAction::Synth, "{m:#010x}");
    }
}

#[test]
fn a_random_marker_boots_the_synth() {
    let mut x = 0x5eed_cafe_u32;                       // xorshift32
    for _ in 0..1_000_000 {
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        if x != DFU_MAGIC { assert_eq!(after_reset(x), BootAction::Synth, "{x:#010x}"); }
    }
}

#[test]
fn the_rom_loader_is_an2606s() {
    assert_eq!(ROM_DFU_BASE, 0x1FF0_9800);             // STM32H74x/75x system memory
}
```

  - `chimera-core/tests/enter_dfu_test.rs` (with `mod screen;` for `to_leaf`, `tap`, `feed`, `Input`, and `tree`):

```rust
mod screen;
use chimera_core::ui::UiState;
use chimera_core::ui::settings::{Act, AskKind, Kind};
use chimera_core::ui::settings::prompt::{Answers, DfuAnswer, EnterDfu, Line, Prompt};
use chimera_hal::{ButtonId, EncoderId};
use screen::{Input, feed, tap, to_leaf};

/// SETTINGS › SYSTEM, the cursor on OS UPGRADE (row 0), SEQ.
fn ask(ui: &mut UiState) {
    to_leaf(ui, &["SYSTEM"]);
    tap(ui, ButtonId::Seq);
}

#[test]
fn os_upgrade_is_an_action_row() {
    use chimera_core::ui::settings::rows;
    let top = rows(&[]).iter().position(|r| r.label == "SYSTEM").unwrap() as u8;
    let r = &rows(&[top])[0];
    assert_eq!((r.label, r.crumb), ("OS UPGRADE", "OS"));
    assert!(matches!(r.kind, Kind::Act(Act::EnterDfu)), "{:?}", r.kind);
}

#[test]
fn os_upgrade_asks_before_dfu() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    assert_eq!(ui.prompt_kind_for_test(), Some(AskKind::EnterDfu));
    assert!(ui.take_dfu().is_none(), "asking is not a yes");
}

#[test]
fn the_prompt_says_what_it_does() {
    let (mut q, mut r) = (Line::new(""), Line::new(""));
    EnterDfu.words(&mut q, &mut r);
    assert_eq!((q.as_str(), r.as_str()), ("ENTER DFU?", "PLAY STOPS UNTIL FLASHED OR POWER-CYCLED"));
    let pills: Vec<_> = DfuAnswer::ALL.as_slice().iter().map(|a| a.label()).collect();
    assert_eq!(pills, ["ENTER DFU", "CANCEL"]);
}

#[test]
fn enter_dfu_gives_one_yes() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    tap(&mut ui, ButtonId::Seq);                       // the first pill: ENTER DFU
    assert!(ui.take_dfu().is_some());
    assert!(ui.take_dfu().is_none(), "taken once");
    assert_eq!(ui.prompt_kind_for_test(), None);
}

#[test]
fn cancel_and_menu_give_none() {
    let mut ui = Box::new(UiState::new());
    ask(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, 1));       // CANCEL
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.take_dfu().is_none());
    ask(&mut ui);
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.take_dfu().is_none());
    assert_eq!(ui.prompt_kind_for_test(), None);
}
```

  `rows` is the walk `to_leaf` uses. `Said<RomDfu>` can't be built outside `replace` (the module's `compile_fail` doctests already pin that for `Said<T>` in general).
  - In `console_parse_test.rs`, `the_table_is_the_one_list` now expects `["help", "status", "stats", "bench", "shot", "dfu"]`, and `commands_without_arguments_refuse_one` adds `Command::Dfu`.
  - In `console_answer_test.rs`, `Fake` gains `dfu: Option<()>` (default `None`) and a `dfu_calls` counter, and two tests:

```rust
#[test]
fn dfu_is_ok_then_the_unit_is_told_once() {
    let mut u = Fake::new();
    u.dfu = Some(());
    assert_eq!(ask(&mut u, "dfu\n"), "OK\n");
    assert_eq!(u.dfu_calls, 1);
    assert_eq!(ask(&mut u, "DFU now\n"), "ERR dfu takes no arguments\n");
    assert_eq!(u.dfu_calls, 1, "a refusal arms nothing");
}

#[test]
fn dfu_without_the_chip_is_one_err_line() {
    let mut u = Fake::new();                           // dfu None
    assert_eq!(ask(&mut u, "dfu\n"), "ERR dfu is not in this build\n");
}
```

  `only_stats_reads_the_stats` adds `"dfu\n"` to its list, and the property test's word list gains `dfu`. The `Fake` keeps `dfu: None` there, so a random `dfu` answers its `ERR` line.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test boot_test --test enter_dfu_test --test console_parse_test --test console_answer_test` → FAIL: `boot` not found, `Act::EnterDfu` not found, the table has five rows.
- [ ] **Step 3: Implement the core.**
  - `boot.rs` as above: `after_reset` is `if marker == DFU_MAGIC { RomDfu } else { Synth }`.
  - `guard.rs`: `impl Sealed for crate::boot::RomDfu {}` and `impl Witnessed for RomDfu { type Witness = (); }`.
  - The prompt: `answers!(DfuAnswer ...)`, `EnterDfu`'s `words`, the `commits!` row, `Ask::EnterDfu(Choice<DfuAnswer>)`, `Answered::EnterDfu(Answer<DfuAnswer>)`, `ask_kinds!`'s `EnterDfu => "enter_dfu"`, and `with_view`'s arm.
  - `tree.rs`: `crumb("OS UPGRADE", "OS", Kind::Act(Act::EnterDfu))`; `Act::EnterDfu`; `part_cmd` maps it to `None`; `run_act`'s arm opens `Ask::EnterDfu(Choice::new())`.
  - The answer: `Answered::EnterDfu(a)` → `self.dfu = said(a)` (a `UiState` field `dfu: Option<Said<RomDfu>>`); `take_dfu` is `self.dfu.take()`.
  - The UPDATES page goes: `UPDATES_BLOCKS`, `UPDATES_LEAF` (`leaves.rs`), `SYS_UPDATES` (`block_registry.rs`, id 34 retired: a comment on the `SYS_ABOUT` line says so), its `FIXED` entry (`[&ChainDef2; 9]`), its imports, and its line in `page_block_test.rs`.
  - The console: the `Dfu` row; `answer`'s arm `Request::Dfu(NoArg)` → `unit.dfu()`, then `OK`, or `ERR dfu is not in this build` on `None`.
  - `screen_atlas_test.rs`'s `prompt`: `AskKind::EnterDfu => { to_leaf(ui, &["SYSTEM"]); tap(ui, ButtonId::Seq); }`. `prompt_naming_test.rs`'s `every_prompt_fits` adds `EnterDfu`.
- [ ] **Step 4: Run** the Step 2 tests → PASS; `cargo test -p chimera-core --test screen_golden_test --test screen_atlas_test --test all_pages_walk_test` → PASS; `just screens`, then check that `docs/screens/settings_system_os.png` is gone and `settings_prompt_enter_dfu.png` reads as the table in the spec's § The trigger, with the reason on two lines.
- [ ] **Step 5: The desktop.** `DeskUnit::dfu` returns `None`. In `main.rs`, after the frame's input: `if ui.take_dfu().is_some() { eprintln!("dfu: not in this build"); }`. Add a test in `console.rs`: `dfu_on_the_sim_is_not_in_this_build` (a client sends `dfu\n` and reads `ERR dfu is not in this build\n`). Run `cargo test -p chimera-desktop console` → PASS.
- [ ] **Step 6: The chip.**
  - `main`: `let mut cp = cortex_m::Peripherals::take().unwrap(); let dp = pac::Peripherals::take().unwrap(); dfu::after_reset(&mut cp, &dp.RCC, &dp.PWR, &dp.RTC); let board = boot(cp, dp);`. `boot` takes them instead of calling `take()` and changes nothing else. `probe::paint_stack()` stays its first line. `dp.RTC` moves into `SynthParts` as `rtc`. This runs in every build, sd-probe included, so a DFU request is honoured whatever is flashed.
  - `dfu::after_reset`: `rcc.apb4enr.modify(|_, w| w.rtcapben().set_bit())`; `let m = rtc.bkpr[0].read().bits()`; `pwr.cr1.modify(|_, w| w.dbp().set_bit())` and wait for it to read back; `rtc.bkpr[0].write(|w| w.bkp().bits(0))`; then `match boot::after_reset(m)`. `Synth` returns. `RomDfu`: `cp.SYST.disable_counter()` and `disable_interrupt()`, then one `unsafe` block with its `// SAFETY:` note: `cp.SCB.vtor.write(ROM_DFU_BASE)` and `cortex_m::asm::bootload(ROM_DFU_BASE as *const u32)`. The note says it runs before any clock, peripheral or interrupt of Chimera's is set up, on the stock bootloader's de-initialised state, and that 0x1FF0_9800 is the immutable ROM's vector table (AN2606).
  - `dfu::enter`: `rtc.bkpr[0].write(|w| w.bkp().bits(DFU_MAGIC))`, then `SCB::sys_reset()`. RTCAPBEN and DBP are already set (`after_reset`, the HAL's PWR `freeze`). The `Said<RomDfu>` in `DfuFrom::Menu` is the proof that SEQ took the pill; `Console` is only made where `service` returned `Served::Dfu`.
  - The loop: after `service`, `if let Served::Dfu = served { dfu::enter(&rtc, DfuFrom::Console) }`. After the frame's input: `if let Some(yes) = ui.take_dfu() { dfu::enter(&rtc, DfuFrom::Menu(yes)) }`. `ChipUnit::dfu` sets the flag `service` reads and returns `Some(())`.
  - The marker's location is one `const MARKER` in `dfu.rs` (`Bkp0`, or `Sram4(0x3800_FFFC)` for U10's fallback), so the swap is one line.
- [ ] **Step 7: Run** `just check` → PASS (stack-check included).
- [ ] **Step 8: Measure.** As in Task 1 Step 2: the default release build at the previous commit against this one. Record the flash under `## Measured` (the "DFU entry" row). **STOP above 1 024 bytes**: commit the numbers alone, revert the rest of this task, and report the split from `llvm-nm --size-sort -S` filtered to `boot|dfu|EnterDfu|DfuAnswer`.
- [ ] **Step 9: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-desktop/src chimera-stm32/src docs/screens docs/superpowers/plans/2026-10-02-usb-console.md
git commit -m "OS UPGRADE and the console's dfu restart the unit into the ROM loader"
```

---

### Task 10: The host tool, its `just` recipes, `to-dfu` and the udev rule

**Files:**
- Create: `tools/chimera-usb.py`, `tools/test_chimera_usb.py`, `tools/70-chimera.rules`
- Modify: `Justfile` (the console recipes, `check`, and one `to-dfu` line in `flash` and `flash-bench`)

**Interfaces:**
- Produces:
  - `python3 tools/chimera-usb.py <cmd> [<arg>]`: exit 0 on `OK`; 1 on `ERR` (the line to stderr) or on no answer within 2 s (`no answer from <target>` to stderr). Bodies go to stdout. `shot` and `shot raw` print the PNG's path.
  - Targets: `CHIMERA_USB` unset → `/dev/chimera` if it exists, else `/dev/ttyACM0`; `sim` → `127.0.0.1:7341`; `tcp:HOST:PORT`; anything else is a device path.
  - Module-level functions the tests import (`importlib` from the hyphenated file): `target(env) -> Target`, `request(conn, line) -> (status, body: bytes)`, `rgb565be_to_png(body, w, h, scale=2) -> bytes`, `shot_path(raw: bool, now) -> Path`.
  - `Justfile`:

```
# Read the unit over USB (CHIMERA_USB=sim for the desktop sim)
usb +cmd:
    python3 tools/chimera-usb.py {{cmd}}
shot *args:
    python3 tools/chimera-usb.py shot {{args}}
stats:
    python3 tools/chimera-usb.py stats
status:
    python3 tools/chimera-usb.py status
```

  and `check` gains `python3 -m unittest discover -s tools -p 'test_*.py'`.
  - `tools/70-chimera.rules`, the spec's line exactly.
  - `python3 tools/chimera-usb.py to-dfu` (spec § `just flash` hands-free), using `usb_devices(sysfs) -> set[(vid, pid)]` read from `<sysfs>/bus/usb/devices/*/idVendor` and `idProduct`:
    - `(0x0483, 0x5740)` present: it sends `dfu`, needs `OK`, then polls every 100 ms for up to `CHIMERA_DFU_WAIT` s (default 10) until `(0x0483, 0xDF11)` appears, and exits 0. On a timeout it prints `no DFU device after dfu: see U10 (marker clobbered?)` and exits 1.
    - `(0x0483, 0xDF11)` already present: exit 0, nothing sent.
    - neither: it prints `no console: bridge BOOT0 on the back and re-plug for DFU` and exits 0.
  - `flash` and `flash-bench` gain `python3 tools/chimera-usb.py to-dfu` between `rust-objcopy` and `dfu-util`.

- [ ] **Step 1: Write the failing tests** in `tools/test_chimera_usb.py` (`unittest`, standard library only). A `FakeUnit` thread serves canned answers on `127.0.0.1:0`, keyed by request line:

```python
class Tool(unittest.TestCase):
    def test_status_prints_the_body_and_exits_0(self):
        with FakeUnit({b"status\n": b"firmware 0.1.0 release\nOK\n"}) as u:
            r = run_tool(["status"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, b"firmware 0.1.0 release\n"))

    def test_err_goes_to_stderr_and_exits_1(self):
        with FakeUnit({b"stats\n": b"ERR stats is not in this build\n"}) as u:
            r = run_tool(["stats"], u.target)
        self.assertEqual(r.returncode, 1)
        self.assertEqual(r.stdout, b"")
        self.assertIn(b"ERR stats is not in this build", r.stderr)

    def test_silence_fails_in_about_two_seconds(self):
        with FakeUnit({}) as u:                      # never answers
            t = time.monotonic(); r = run_tool(["status"], u.target); took = time.monotonic() - t
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"no answer from", r.stderr)
        self.assertTrue(1.8 < took < 4.0, took)

    def test_shot_writes_a_png_of_the_pixels(self):
        px = bytes(range(256)) * 600                 # 153 600 bytes
        with FakeUnit({b"shot\n": b"SHOT 240 320 rgb565be 153600\n" + px + b"OK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot"], u.target, cwd=d)
            path = pathlib.Path(d, r.stdout.decode().strip())
            w, h, rgb = read_png(path)               # zlib + struct, in the test file
        self.assertEqual((w, h), (480, 640))
        self.assertEqual(rgb_at(rgb, w, 0, 0), widen(px[0] << 8 | px[1]))
        self.assertEqual(rgb_at(rgb, w, 1, 1), widen(px[0] << 8 | px[1]), "2x nearest")
        self.assertEqual(widen(0xFFFF), (255, 255, 255))
        self.assertTrue(path.name.startswith("shot-") and not path.name.startswith("shot-raw-"))

    def test_shot_raw_asks_for_raw_and_names_it(self):
        px = bytes(153600)
        with FakeUnit({b"shot raw\n": b"SHOT 240 320 rgb565be 153600\n" + px + b"OK\n"}) as u, tempfile.TemporaryDirectory() as d:
            r = run_tool(["shot", "raw"], u.target, cwd=d)
        self.assertEqual(r.returncode, 0)
        self.assertIn(b"shot-raw-", r.stdout)

    def test_a_bad_shot_header_is_refused(self):
        with FakeUnit({b"shot\n": b"SHOT 240 320 rgb565be 999\n" + bytes(999) + b"OK\n"}) as u:
            self.assertEqual(run_tool(["shot"], u.target).returncode, 1)

    def test_a_stalled_tail_is_drained_before_the_next_request(self):
        # The unit first sends the tail of an earlier, stalled shot, then answers.
        with FakeUnit({b"status\n": b"firmware x\nOK\n"}, preamble=b"\x00" * 5000) as u:
            r = run_tool(["status"], u.target)
        self.assertEqual((r.returncode, r.stdout), (0, b"firmware x\n"))

    def test_sim_target_is_the_desktop_console_address(self):
        src = (ROOT / "chimera-desktop/src/console.rs").read_text()
        self.assertIn('pub const ADDR: &str = "127.0.0.1:7341";', src)
        self.assertEqual(tool.target({"CHIMERA_USB": "sim"}), ("tcp", "127.0.0.1", 7341))

    def test_udev_rule_matches_the_firmware_identity(self):
        rule = (ROOT / "tools/70-chimera.rules").read_text()
        usb = (ROOT / "chimera-stm32/src/usb.rs").read_text()
        self.assertIn("VID_PID: (u16, u16) = (0x0483, 0x5740)", usb)
        for want in ('ATTRS{idVendor}=="0483"', 'ATTRS{idProduct}=="5740"', 'ENV{ID_MM_DEVICE_IGNORE}="1"', 'TAG+="uaccess"', 'SYMLINK+="chimera"'):
            self.assertIn(want, rule)

    def test_to_dfu_sends_dfu_and_waits_for_the_rom(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({b"dfu\n": b"OK\n"}, on_answer=lambda: fs.replace({(0x0483, 0xDF11)}, after=0.3)) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual(r.returncode, 0)
        self.assertEqual(u.requests, [b"dfu\n"])

    def test_to_dfu_times_out_when_the_rom_never_comes(self):
        with FakeSysfs({(0x0483, 0x5740)}) as fs, FakeUnit({b"dfu\n": b"OK\n"}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root, env={"CHIMERA_DFU_WAIT": "1"})
        self.assertEqual(r.returncode, 1)
        self.assertIn(b"marker clobbered", r.stderr)

    def test_to_dfu_with_the_jumper_sends_nothing(self):
        with FakeSysfs({(0x0483, 0xDF11)}) as fs, FakeUnit({}) as u:
            r = run_tool(["to-dfu"], u.target, sysfs=fs.root)
        self.assertEqual((r.returncode, u.requests), (0, []))

    def test_to_dfu_without_a_console_prints_the_jumper(self):
        with FakeSysfs(set()) as fs:
            r = run_tool(["to-dfu"], "tcp:127.0.0.1:9", sysfs=fs.root)
        self.assertEqual(r.returncode, 0)
        self.assertIn(b"bridge BOOT0", r.stdout + r.stderr)

    def test_to_dfu_matches_the_firmware_identity(self):
        self.assertEqual(tool.CONSOLE_ID, (0x0483, 0x5740))
        self.assertEqual(tool.ROM_DFU_ID, (0x0483, 0xDF11))
        just = (ROOT / "Justfile").read_text()
        for recipe in ("flash:", "flash-bench:"):
            body = just.split(recipe, 1)[1].split("\n\n", 1)[0]
            self.assertLess(body.index("to-dfu"), body.index("dfu-util"), recipe)
```

  `FakeUnit(preamble=...)` sends the preamble as soon as a client connects, before the request arrives, as a stalled answer's tail would sit in the port. `FakeUnit` records `requests` and calls `on_answer` after it answers. `FakeSysfs` writes `bus/usb/devices/<n>/idVendor` and `idProduct` (lowercase hex, as the kernel does) under a temp dir, and `replace(ids, after)` swaps them from a timer thread, as a device leaving and the ROM loader arriving would. `run_tool(..., sysfs=)` sets `CHIMERA_SYSFS`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools -p 'test_*.py'` → FAIL: no `chimera-usb.py`.
- [ ] **Step 3: Implement** the tool as the spec's § Host tool says:
  - serial: `os.open(O_RDWR | O_NOCTTY)`, `tty.setraw`;
  - socket: `socket.create_connection`;
  - each request: drain for 50 ms, send `<cmd>\n`, read lines until `OK` or `ERR`; a `SHOT` header switches to reading exactly `length` bytes; 2 s of silence fails;
  - the PNG: `zlib` + `struct` (IHDR, IDAT with filter byte 0 per row, IEND), 2× nearest, RGB565 widened by bit replication, written to `target/shots/` under the repo root (`git rev-parse --show-toplevel`, or the tool's parent's parent);
  - `to-dfu` as above;
  - the `Justfile` recipes, the `check` line and the two `to-dfu` lines; the udev rule.
- [ ] **Step 4: Run** the tests → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add tools Justfile
git commit -m "A dependency-free host tool reads the console, with just recipes, a udev rule and a hands-free flash"
```

---

### Task 11: Desktop QA and the measurements

**Files:**
- Modify: this plan (`## Measured`)

- [ ] **Step 1: The whole check.** `just check` → PASS. Record the test counts.
- [ ] **Step 2: The sim end to end.** `cargo build --release -p chimera-desktop`, then run it in the background (`timeout 30 target/release/chimera-desktop &`, with `CHIMERA_CARD` on a fresh `mktemp -d`), wait for the port (`until nc -z 127.0.0.1 7341; do sleep 0.2; done`), and run:
  - `CHIMERA_USB=sim just usb help` → the help, exit 0;
  - `CHIMERA_USB=sim just status` → six lines, `state NEW`, `part 1`, `at PART 1 > …`;
  - `CHIMERA_USB=sim just stats` → `ERR stats is not in this build`, exit 1;
  - `CHIMERA_USB=sim just shot` and `CHIMERA_USB=sim just shot raw` → two PNGs. Compare `shot raw` with the matching `docs/screens` image (`magick compare -metric AE`) when the sim's boot screen has one, and record the count. A difference is expected only where live values (the scope, the load) differ, so record what differs rather than requiring 0;
  - `CHIMERA_USB=sim just usb frob` → `ERR unknown command frob, try help`, exit 1;
  - `CHIMERA_USB=sim just usb dfu` → `ERR dfu is not in this build`, exit 1;
  - in the window, SETTINGS › SYSTEM › OS UPGRADE opens the DFU prompt; `CANCEL` closes it, and `ENTER DFU` prints `dfu: not in this build` on stderr while the sim plays on.

  If no window can open here (as in the SETTINGS plan's QA), say so and rely on Task 7's and Task 9's tests, which run the same code without a window.
- [ ] **Step 3: The costs.** Record Task 8's flash, RAM and bench-RAM numbers beside Task 1's, Task 9's DFU row, and the stack check's verdict, under `## Measured`.
- [ ] **Step 4: Prepare the ship checklist** below (it is already written; check that every command in it exists).
- [ ] **Step 5: Commit**

```bash
git status --short
git add docs/superpowers/plans/2026-10-02-usb-console.md
git commit -m "Desktop QA for the USB console recorded"
```

---

### Task 12: The ship flash (STOP for the owner)

**Files:**
- Modify: this plan (`## Measured`); on success, `docs/adr/0068-usb-console.md` and `docs/adr/README.md` (Status → `Accepted (<date>)`)

- [ ] **Step 1: STOP.** The owner flashes `just flash` (the default build, console on) with the BOOT0 jumper, as today, since the unit runs a build without `dfu`. Then U8 to U10 (no jumper from here on), and `just flash-bench` for U7 and U9. Claude reads out the checklist under `## Measured` › Ship flash checklist and records each result as the owner gives it.
- [ ] **Step 2: Record** every result. On all passing, set ADR 0068 to `Accepted (<date>)` in its file and in `docs/adr/README.md`. On any failure, file an issue (joegiralt/chimera), cite it here, and no ADR moves.
- [ ] **Step 3: Commit**

```bash
git status --short
git add docs/superpowers/plans/2026-10-02-usb-console.md docs/adr/0068-usb-console.md docs/adr/README.md
git commit -m "USB console ship flash recorded; ADR 0068 accepted"
```

## Self-review against the spec

| Spec section | Where |
|---|---|
| § Intent: `stats`, `bench`, `status`, `shot` (and `shot raw`) over `/dev/ttyACM0`; the sim on a socket | Tasks 2–10; U1–U7 |
| § Research: OTG_FS on PA11/PA12 AF10, HSI48 + CRS, the crate versions, `just flash` unaffected | Task 1 (Steps 3–4); U6 |
| § What this changes: `console` core, `usb.rs` behind `usb-console` in `default`, desktop `console.rs`, the tool, `frame()` on both displays, `priority.rs`'s comment, the bench's text | Tasks 2–10 (Task 8 Steps 5–7) |
| § Requests: 64 bytes, LF/CR, empty lines, trimming, case, one optional argument | Task 2 |
| § Answers: one terminal line, `key value` bodies, the error table | Tasks 2, 5 |
| `help` | Task 5 `help_lists_the_table_in_order` |
| `status`: each line, `state` words, `at` per `Loc`, crumbs | Task 3 |
| `stats`: eight lines, `drops` per source, loop time excluding answered iterations, reset on read | Tasks 5, 6, 8 |
| `bench`: screen lines, `# <title>`, `BENCH_TEXT` 6 KB, `# TRUNCATED` | Tasks 6, 8; U7 |
| `shot`: header, body, `OK`, THEME palette; `shot raw` canonical | Tasks 4, 7; U3 |
| § Functional core: `commands!`, `Console`, `Unit`, `Out`, `Stalled`, `answer`; total parsing; no buffers; 480-byte rows | Tasks 2, 4, 5 |
| § USB shell: bring-up order, take-once statics, OTG interrupt masked | Tasks 1, 8 |
| § Identity: `0483:5740` (stock PreenFM3, Ixox/preenfm3 `firmware/Src/usbd_desc.c`), strings, UID serial, self-powered 100 mA | Tasks 1, 6, 8 |
| § Polling: one request per iteration, `UsbOut` pumps, 250 ms stall | Task 8; Review Focus 2–3 |
| § Snapshot point | Task 8 Step 4; Review Focus 6; U3 |
| § The unit's `Unit` | Task 8 Step 3 |
| § Desktop shell: one client, busy port, polled at the frame's top, same stall, `stats`/`bench` `None` | Task 7 |
| § Host tool: targets, raw mode, drain, 2 s, exit codes, PNG at 2×, bit replication, file names, recipes, udev | Task 10 |
| § Enter DFU from the firmware: OS UPGRADE's prompt and `Said<RomDfu>`, the UPDATES page gone, `dfu`, the marker and its fallback, the check at the top of `main`, the jump, `just flash` hands-free, safety | Tasks 9, 10; U8–U10 |
| § Real-time rules | Global Constraints; Task 8; U4 |
| § Cost: measured, STOP over 24 KB; DFU entry under 1 KB | Tasks 1, 8, 9, 11 |
| § Tests: core unit tests, property test | Tasks 2–6 |
| § Tests: desktop QA | Tasks 7, 9, 11 |
| § Tests: ship U1–U10 | Task 12 |
| § Later | Not built. `Request`'s typed arguments are the hook for write commands; `Rung` for ORBIT. |
| § The owner's answers 1–6 | 1: Tasks 1, 8, 10. 2: Tasks 2, 4. 3: Task 4 (no copy). 4: Task 10. 5: Task 1 Step 4 (`init` before the first frame). 6: Tasks 9, 10. |

Gaps found and closed in this pass:
- The spec's `at` example used row labels; the screen draws short crumbs. The spec now matches what is drawn (Pre-flight 3).
- The spec listed `Orbit(_)`, which `nav-core` lacks. `Rung` makes the gap a build error when ORBIT lands (Pre-flight 4).
- The spec put `LoopTimer` in the shell. It is pure arithmetic, so it moved to the core, where it is tested (Pre-flight 6).
- "A PNG equal to the window's frame": the window adds BRIGHT, so the test compares with the framebuffer through the palette (Pre-flight 7).
- The core's flash can't be measured before it exists. Task 1 adds the estimate; Task 8 measures the whole (Pre-flight 2).
- The DFU ask suggested `#[pre_init]` or an existing `before_main`. There is no `before_main`, and cortex-m-rt calls a Rust `pre_init` unsound, so the check goes at the top of `main` (Pre-flight 14).
- The spec had no section named DFU coexistence. Its § Research row on `just flash` now carries that, and it points to § Enter DFU from the firmware.

## Measured

Filled by Tasks 1, 8, 9, 11 and 12.

### Flash and RAM

| | flash (bytes) | RAM (bytes) | Task |
|---|---|---|---|
| base (default, no console) | 722 528 | AXI 374 088, D2 168 364 | 1 |
| bring-up (`--features usb-console`), + 3 072 core estimate | 13 128 + 3 072 = **16 200** (735 656, 80.18 %) | AXI +1 448 (375 536), D2 +0 | 1 |
| whole (default with `usb-console`) − (`midi-din,perf-probe` only) | ____ | ____ | 8 |
| bench build's extra RAM (`BENCH_TEXT`) | n/a | ____ | 8 |
| DFU entry: default build after Task 9 − before | ____ | ____ | 9 |
| limit | 24 576; DFU entry 1 024 | | |

Release builds of `usb-console` 5d05cd2+, `llvm-size -A`: flash is `.vector_table + .text + .rodata + .data`, AXI is `.data + .bss`, D2 is `.ram_d2 + .ram_d2_dma`. `.text` +16 360, `.rodata` −3 232. `llvm-nm -S` by crate, roughly (generics land under the crate that names them): `synopsys-usb-otg` 12.6 KB, `usb-device` 2.0 KB, the shell 2.3 KB.

### Desktop QA (Task 11)

____

### Ship flash checklist (Task 12, the owner)

Flash `just flash` first, with the BOOT0 jumper: the build on the unit has no `dfu` yet. Take the jumper off after it. Leave a chord playing where a check says so.

| # | Check | Result |
|---|---|---|
| U1 | The unit enumerates as `/dev/ttyACM0` (`ls /dev/ttyACM*`; `lsusb` shows `0483:5740`, the stock PreenFM3 ID, with strings `Chimera` / `Chimera console`) about 1 s after power-on, after the splash. With the udev rule installed (`sudo cp tools/70-chimera.rules /etc/udev/rules.d/ && sudo udevadm control --reload`), `/dev/chimera` appears too. `just usb help` answers. | ____ |
| U2 | `just status` matches the screen on a Part page, on a mixer page and in SETTINGS › SYSTEM › DIAGNOSTICS › AUDIO LOAD. | ____ |
| U3 | `just shot` matches the panel, THEME accent included (set a non-TEAL accent first). `just shot raw` is in the canonical teal on black. | ____ |
| U4 | `just stats` answers with live numbers. With a chord playing: `overruns` is the same before and after ten `just shot`s in a row, and each shot holds the screen still for about 0.15–0.3 s while the sound goes on. | ____ |
| U5 | Unplug USB mid-shot: the UI moves again within 250 ms. Replug: the next `just status` answers. | ____ |
| U6 | `just flash` still works afterwards: bridge BOOT0, run `just flash` with `just usb help` having just run (and with a terminal holding the port open). It flashes, and the new firmware boots and enumerates. | ____ |
| U7 | `just flash-bench`: `just usb bench` gives the B1–B3 numbers the bench screens showed, screen by screen. | ____ |
| U8 | Enter DFU from the menu. SETTINGS › SYSTEM › OS UPGRADE asks `ENTER DFU?` / `PLAY STOPS UNTIL FLASHED OR POWER-CYCLED`. `CANCEL` and MENU leave the chord playing. `ENTER DFU`: the sound stops, `0483:5740` leaves `lsusb`, and `lsusb -d 0483:df11` shows the ROM loader within 2 s. It is still there 30 s later (no watchdog reset). Note what the panel shows. Then power-cycle without flashing: the synth plays (the marker was cleared). | ____ |
| U9 | `just flash` hands-free from the console. With the synth running, no jumper and the cable in: `just flash` prints nothing about BOOT0, sends `dfu`, waits for `0483:DF11`, flashes, and the new build boots and enumerates as `0483:5740`. Then `just flash-bench` the same way, and back with `just flash`. Also `just usb dfu` alone answers `OK` and the ROM loader appears. | ____ |
| U10 | The clobber check. The marker must survive the stock bootloader. If U8 or U9 brings the unit back as `0483:5740` instead of `0483:DF11` (so `to-dfu` fails with `marker clobbered?` and ABOUT's RESET reads SOFTWARE), RTC_BKP0R was cleared on the way. Set `dfu::MARKER` to the SRAM4 fallback (0x3800_FFFC), `just flash` with the jumper, and repeat U8 and U9. Record which location works, and file an issue if neither does. | ____ |

On all passing: ADR 0068 → `Accepted (<date>)` (Task 12 Step 2). On any failure: an issue, and no ADR moves.
