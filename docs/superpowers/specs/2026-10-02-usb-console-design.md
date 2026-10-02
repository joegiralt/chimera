# USB console: read the unit over USB instead of photographing it

Status: owner-reviewed (2026-10-02); the open questions are settled (§ The owner's answers). ADR 0068 (Proposed until the ship flash) records the decision. Plan: `docs/superpowers/plans/2026-10-02-usb-console.md`.

## Intent

The owner, 2026-10-02: "You see how I have to take pictures of the screen every time I want to do anything? What I want is to be able to read that information via USB… where you can just read arbitrary information over the USB."

Plug the unit into the computer and it shows up as a serial port (`/dev/ttyACM0` on Linux, no driver). A one-line request gets one answer:

- `stats`: the AUDIO LOAD numbers;
- `bench`: the bench numbers;
- `status`: where the unit is;
- `shot`: the screen as a PNG, in THEME's colours as the panel shows it; `shot raw` in the canonical palette, as `docs/screens` shows it.

The `just` recipes wrap it. The desktop simulator answers the same requests over a local socket, so the protocol is tested on the host before the chip sees it.

This version is read-only. USB MIDI on the same cable (#203) and write commands come later; § Later says what this design keeps open for them.

## Research

| Question | Answer | Source |
|---|---|---|
| Which peripheral and pins drive the USB connector? | **USB2 OTG_FS on PA11 (DM) and PA12 (DP), AF10**, internal full-speed PHY, device only, VBUS sensing off. In the HAL this is `usb_hs::USB2` with `OTG2_HS_GLOBAL/DEVICE/PWRCLK`, the same block under RM0433's other name. PB14/PB15 (OTG_HS's internal PHY) are ruled out on this board: they are SPI2's MISO and MOSI for the SD card (`chimera-stm32/src/main.rs`, `SdParts`; `docs/chimera-synth-design.md` § Storage). | Stock firmware `firmware/Src/usbd_conf.c` (`HAL_PCD_MspInit`: `USB_OTG_FS`, `GPIO_PIN_11\|GPIO_PIN_12`, `GPIO_AF10_OTG1_FS`; `Init.vbus_sensing_enable = DISABLE`, `phy_itface = PCD_PHY_EMBEDDED`) and `firmware/preenfm3.ioc` (`PA11.Signal=USB_OTG_FS_DM`, `PA12.Signal=USB_OTG_FS_DP`, `USB_OTG_FS.VirtualMode=Device_Only`), github.com/Ixox/preenfm3 at `master`. HAL: `stm32h7xx-hal-0.16.0/src/usb_hs.rs` (`USB2::new` takes `PA11<Alternate<10>>`, `PA12<Alternate<10>>`). |
| Where does the 48 MHz USB clock come from? | **HSI48, trimmed by the CRS from USB2's start-of-frame.** The HAL's `freeze` always turns HSI48 on (`rcc/mod.rs`: `hsi48on().on()`), and `kernel_usb_clk_mux(UsbClkSel::Hsi48)` routes it to USB. A PLL output is not an option: PLL1 runs the core at a per-revision rate (`clocks.rs`, `rev.cpu_hz()`), PLL2_P is pinned at 100 MHz for the SPIs, and PLL3 is fractional for the SAI's 48 kHz (`init_pll3`), so none can also give an exact 48 MHz. The stock firmware runs USB from HSI48 with no CRS (`firmware/Src/main.c`: `UsbClockSelection = RCC_USBCLKSOURCE_HSI48`), and it works. Chimera adds the CRS anyway, since full speed needs ±0.25 % and the CRS costs three register writes. The CRS sync source for USB2's SOF is `SYNCSRC = 0b11` (`RCC_CRS_SYNC_SOURCE_USB2 = SYNCSRC_1\|SYNCSRC_0`, stock `stm32h7xx_hal_rcc_ex.h`). The core's AHB clock (`hclk` = CPU/2, 200 or 240 MHz) is far above OTG's 30 MHz floor. | As cited. |
| Which crate versions work with today's `Cargo.lock`? | **`stm32h7xx-hal` 0.16.0** (already locked) with its `usb_hs` feature, which pulls **`synopsys-usb-otg` 0.4.0**; **`usb-device` 0.3.2**; **`usbd-serial` 0.2.2**. `synopsys-usb-otg` 0.4 and `usbd-serial` 0.2 both require `usb-device ^0.3`; the HAL's own examples use exactly this set. Adding them locks six new packages (`embedded-io` 0.6.1, `heapless` 0.8.0, `portable-atomic` 1.15.0 and the three USB crates) and changes nothing already locked. `cargo check -p chimera-stm32 --target thumbv7em-none-eabihf` passes with the `usb_hs` feature on (checked 2026-10-02, then reverted). `synopsys-usb-otg` 0.5.0 exists, but the HAL 0.16 pins 0.4. | crates.io dependency metadata; `stm32h7xx-hal-0.16.0/Cargo.toml`; a local `cargo tree -i usb-device`. |
| Does anything here get in the way of `just flash`? | **No.** `just flash` runs `dfu-util -d 0x0483:0xdf11`, which talks to ST's ROM DFU loader. The unit reaches that loader only with BOOT0 bridged, and then no Chimera code runs, the console included. The ROM loader uses the same OTG_FS on PA11/PA12 (ST AN2606, STM32H74x/75x: USB DFU on PA11/PA12), so the connector is shared and nothing else is. The console never uses 0483:DF11 (§ Identity), never writes flash or option bytes, and never touches BOOT0. The stock PreenFM3 bootloader at 0x08000000 exposes the SD card as USB mass storage (`bootloader/Src/usbd_storage_if.c`), not DFU, and it runs before Chimera, so it is not affected either. | `Justfile` `flash`; `docs/chimera-synth-design.md` § Firmware Loading; AN2606. |

## What this changes

- **New in `chimera-core`:** `console`, the functional core. Bytes go in, bytes come out, and there is no I/O.
- **New in `chimera-stm32`:** `usb.rs`, the USB shell, behind the feature `usb-console`. It is in `default`, so `--no-default-features` builds without it, as it does without MIDI DIN. The sd-probe build has no console: the synth is not built there.
- **New in `chimera-desktop`:** `console.rs`, the socket shell.
- **New host tool:** `tools/chimera-usb.py` (Python 3, standard library only), its `just` recipes, and `tools/70-chimera.rules` (udev).
- **Touched:** `main.rs`, which calls the shell once per UI loop iteration. `bench.rs` keeps its report text. `display.rs` (both shells) gains a read-only `frame()`. `priority.rs` gains a comment: USB has no interrupt priority because it is polled.
- **Not touched:** the audio path, `priority.rs`'s levels, the SETTINGS tree. SYSTEM › USB CONFIG (#269) stays `Later`.

## Protocol

ASCII lines, newline-terminated. The unit never speaks first: no banner, no unsolicited output, so every byte from it answers a request.

### Requests

- One request per line, at most `MAX_LINE` = 64 bytes before the terminator.
- The terminator is LF or CR, so a terminal's Enter works. An empty line is ignored and gets no answer, so CRLF is one request.
- Leading and trailing spaces are trimmed. The command word matches case-insensitively.
- Grammar: `<command> [<arg>]`, separated by single or repeated spaces. A command takes at most one argument. In this version only `shot` takes one, `raw`, which also matches case-insensitively. Every other command takes none.

### Answers

Every answer ends with exactly one terminal line:

- `OK`; or
- `ERR <reason>`, which also stands for the whole answer: an error has no body.

A text body is lines of `<key> <value>`: a lowercase key, one space, then the value to the end of the line. Line ending is LF only. A terminal wants `picocom --imap lfcrlf`; the host tool doesn't care.

| Error line | When |
|---|---|
| `ERR unknown command <word>, try help` | The word is not in the table. `<word>` is cut to 16 bytes. |
| `ERR <command> takes no arguments` | Anything follows `help`, `status`, `stats` or `bench`. |
| `ERR shot takes raw or nothing` | `shot` is followed by a word other than `raw`, or by more than one word. |
| `ERR line too long, 64 max` | The line ran past `MAX_LINE`; everything up to the next terminator is dropped. |
| `ERR <command> is not in this build` | The shell has no data for it: `stats` without `perf-probe` or on the desktop, `bench` outside a bench build. |

### The commands

**`help`**

```
chimera console 1
help    this list
status  firmware, project, Part and where the UI is
stats   AUDIO LOAD and the UI loop's time
bench   the bench's numbers (bench builds)
shot    the screen in THEME's colours; shot raw: canonical
OK
```

The first line names the protocol version, `1`. A change that breaks a host parser bumps it. Rows come from the command table in table order.

**`status`**

```
firmware 0.1.0 release
protocol 1
project LATE SET
state MODIFIED
part 3
at SETTINGS > SYSTEM > DIAG > AUD LOAD
OK
```

- `firmware` is `about_page::VERSION` and `BUILD`, as ABOUT shows them.
- `project` is the project's name, as the footer draws it.
- `state` is one of `NEW`, `SAVED` or `MODIFIED`, the footer's words for `ProjectStatus::Pristine`, `Saved` and `Modified`. It comes from `UiState::project_status()`, which may hash the project once (about 1.7 ms on the chip, the B1 row). That is acceptable once per request.
- `part` is the active Part, 1 to 6.
- `at` is `Location` in ASCII, with ` > ` between levels:

  | `Loc` | `at` |
  |---|---|
  | `Pages(p, page)` | `PART <n> > <page title>` |
  | `Part(p, MixPage)` | `MIXER <n> > PART` or `MIXER <n> > SENDS` |
  | `Sound(p, _)` | `SOUND <n>` |
  | `Fx(p, page)` | `FX > <page title>` |
  | `Settings(at)` | the breadcrumbs as drawn (`UiState::crumbs()`, which starts at `SETTINGS`), with the rows' short crumbs (`DIAG`, `AUD LOAD`) and NAMING's crumb included, but never shortened behind `..` |
  | `Orbit(_)` | `ORBIT`, once the ORBIT plan adds `Location::Orbit` (it is not on `nav-core`) |

  A page title is the text that page's header draws.

**`stats`**

```
load_pct 23
peak_pct 41
overruns 0
drops 0
desyncs 0
stack_bytes 12288
loop_avg_us 812
loop_peak_us 4210
OK
```

- The first six lines are `AudioStats` as the AUDIO LOAD page reads them.
- `drops` holds one count per note source (`sources` of them), separated by spaces.
- `stack_bytes` is `stack_used` (the page rounds it to K; this does not).
- `loop_avg_us` and `loop_peak_us` time the UI loop: the DWT time from one loop top to the next. The shell keeps them since the last `stats`, and reading resets both. An iteration that answered a request is left out, so a `shot` never shows up as a slow frame. These two lines are new: they read rows B5 and B7 of the ship checklist (frame time while turning a cell) without a camera. B4 and B6 (overruns) are `overruns`.

**`bench`**

A bench build's numbers as text: the text the bench screens draw, one screen line per answer line, in the order the screens show them. That covers the voice rows, KERNEL, the FX row, the routing pages and the MEMORY screen's `PROJ CRC`, rebuild and note-on rows. Each screen starts with a line `# <screen title>`, and the answer ends with `OK`. This reads B1 to B3 of the checklist (`docs/superpowers/plans/2026-10-01-settings-navigation.md` § Ship flash checklist).

`bench::run` still shows its screens and holds each one, then falls through to the synth as today. It also writes the same lines into `BENCH_TEXT`, a static `[u8; 6144]` that exists only in bench builds. The console serves that text once the UI loop runs. If the text outgrows the buffer, its last line is `# TRUNCATED`, and the bench's text is estimated at about 4 KB (about 60 lines).

**`shot`**

```
SHOT 240 320 rgb565be 153600
<153600 bytes>
OK
```

- The header line gives the width, height, pixel format and body length. The body follows at once.
- Pixels go row by row from the top, left to right in each row. Each pixel is two bytes, big-endian RGB565: the bytes the panel is sent.
- `shot` is the screen as THEME shows it: each pixel goes through the display's `Palette::map_raw`, the same map `write_pixels` applies. Panel gamma and backlight are analog and are not in it.
- `shot raw` is the framebuffer as drawn, in the canonical palette (`Palette::IDENTITY`): the colours of `docs/screens` and the goldens, whatever THEME is set to. The header line is the same; the host knows which it asked for.
- A host checks `width × height × 2 = length` and reads exactly `length` bytes, then the `OK` line.

## The functional core: `chimera_core::console`

No I/O, no allocation, no `unsafe`; the module is unit-tested on the host.

```rust
/// One table: each command's name and help line. Adding a command adds a
/// row here; the enum, `ALL`, `name` and `about` all come from it, and
/// `answer`'s exhaustive match fails the build until the new variant is
/// answered.
/// A row's argument type says what may follow the word: `NoArg` takes
/// nothing, `Colours` takes `raw` or nothing. `answer` gets the parsed
/// value, never the word.
commands! {
    Help   (NoArg)   => "help",   "this list",
    Status (NoArg)   => "status", "firmware, project, Part and where the UI is",
    Stats  (NoArg)   => "stats",  "AUDIO LOAD and the UI loop's time",
    Bench  (NoArg)   => "bench",  "the bench's numbers (bench builds)",
    Shot   (Colours) => "shot",   "the screen in THEME's colours; shot raw: canonical",
}
// expands to:
// #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Command { Help, Status, Stats, Bench, Shot }
// impl Command { pub const ALL: [Command; 5]; pub const fn name(self) -> &'static str;
//                pub const fn about(self) -> &'static str; pub const fn usage(self) -> &'static str; }
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum Request { Help(NoArg), Status(NoArg), Stats(NoArg), Bench(NoArg), Shot(Colours) }
// impl Request { pub const fn command(self) -> Command; }

pub trait Arg: Sized {
    const USAGE: &'static str;                       // "no arguments", "raw or nothing"
    fn parse(words: Words<'_>) -> Option<Self>;      // None: refused
}
pub struct NoArg;                                    // only an empty `Words` parses
pub enum Colours { Theme, Raw }                      // nothing, or `raw`

pub const MAX_LINE: usize = 64;
pub const PROTOCOL: u8 = 1;

pub enum Refusal { Unknown(Word), Arguments(Command), TooLong }   // Arguments: `ERR <name> takes <usage>`
pub struct Word { bytes: [u8; 16], len: u8 }        // the unknown word, cut to 16

/// Bytes in, one request out per complete line.
pub struct Console { line: [u8; MAX_LINE], len: u8, overflowed: bool }
impl Console {
    pub const fn new() -> Self;
    /// One received byte. `Some` when it ends a non-empty line.
    pub fn push(&mut self, byte: u8) -> Option<Result<Request, Refusal>>;
}

/// What a shell can tell. `None` means "not in this build".
pub trait Unit {
    fn ui(&self) -> &UiState;
    fn stats(&mut self) -> Option<Stats>;     // reading resets the loop timer
    fn bench(&self) -> Option<&str>;
    fn frame(&self) -> Frame<'_>;
}
pub struct Stats { pub audio: AudioStats, pub loop_avg_us: u32, pub loop_peak_us: u32 }
pub struct Frame<'a> { pub fb: &'a [u16; FB_SIZE], pub palette: Palette }   // `shot raw` ignores `palette`

/// Where answers go. `Stalled`: the host stopped taking bytes.
pub trait Out { fn put(&mut self, bytes: &[u8]) -> Result<(), Stalled>; }
pub struct Stalled;

/// The whole answer, terminal line included.
pub fn answer(req: Result<Request, Refusal>, unit: &mut impl Unit, out: &mut impl Out)
    -> Result<(), Stalled>;
```

- **Parsing is total.** Every byte sequence becomes a `Request` or a `Refusal`. Nothing panics, and no byte is an error except as part of a line.
- **Text goes straight out.** `answer` formats through a `core::fmt::Write` adapter over `Out`, so there are no response buffers. `shot` fills one 480-byte row at a time, on the stack (well under the stack check's 8 KB step), and puts it.
- **`commands!`** is one `macro_rules!` in the module. It is the only place a command's name, help line and argument type live.

## The USB shell: `chimera-stm32/src/usb.rs`

### Bring-up

`boot()` owns `ccdr`, so it does step 1 and moves PA11, PA12, `USB2OTG` and the three `OTG2_HS_*` register blocks into a `UsbParts` in `SynthParts`, as it does `SdParts` for the card. Nothing is enabled yet. `usb::init(parts, &clocks)` does steps 2 to 6 once, in `synth()`, after the audio and MIDI DIN start and just before the UI loop's first frame. By then the boot's card work and the bench are done, so the loop starts polling at once and enumeration never waits on them. In order:

1. `ccdr.peripheral.kernel_usb_clk_mux(UsbClkSel::Hsi48)`, which asserts `ccdr.clocks.hsi48_ck()` is `Some`.
2. CRS: `RCC_APB1HENR.CRSEN = 1`, `CRS_CFGR.SYNCSRC = 0b11` (USB2 SOF), `CRS_CR.AUTOTRIMEN = 1, CEN = 1`, through the PAC, each write with a `// SAFETY:` note. The CRS trims HSI48 from the host's 1 kHz SOF from the first frame on.
3. `pin_dm = gpioa.pa11.into_alternate::<10>()` and `pin_dp = gpioa.pa12.into_alternate::<10>()`, then `USB2::new(dp.OTG2_HS_GLOBAL, dp.OTG2_HS_DEVICE, dp.OTG2_HS_PWRCLK, pin_dm, pin_dp, ccdr.peripheral.USB2OTG, &ccdr.clocks)` (`USB2OTG` is the HAL's `rec::Usb2Otg`). The HAL sets `PWR_CR3.USB33DEN`. `synopsys-usb-otg` turns off VBUS sensing and forces the B-session valid, as the stock firmware does.
4. `UsbBus::new(usb, EP_MEMORY)`. `EP_MEMORY` is a static `[u32; 256]` (1 KB), handed out once through an `AtomicBool` take, like `take_framebuffer`. It holds the OUT packets only (EP0 and CDC's bulk OUT, 64 bytes each). The TX FIFOs live in the core's own 4 KB RAM.
5. The `UsbBusAllocator` goes in a take-once static, so the device and the class can borrow it for `'static`.
6. `SerialPort::new(&bus)`, then the `UsbDeviceBuilder` with § Identity's values and `device_class(USB_CLASS_CDC)`.

The OTG interrupt is never unmasked in the NVIC (`pac::Interrupt::OTG_FS` stays disabled): nothing runs outside the UI loop.

### Identity

| Field | Value |
|---|---|
| VID:PID | `1209:0001`, the pid.codes test PID (the owner, 2026-10-02). Applying for a free pid.codes PID is https://github.com/joegiralt/chimera/issues/299. It is never `0483:DF11`, so `dfu-util -d 0x0483:0xdf11` cannot match the running synth. |
| Manufacturer | `Chimera` |
| Product | `Chimera console` |
| Serial number | the chip's 96-bit unique ID (the unique device ID registers at `0x1FF1_E800`, RM0433) as 24 uppercase hex digits, formatted once at `init` into a static. `/dev/serial/by-id/usb-Chimera_Chimera_console_<uid>-if00` is then stable per unit. |
| Power | Self-powered, `bMaxPower` 100 mA (`.self_powered(true).max_power(100)`), as the stock firmware's MIDI descriptor declares (`usbd_midi.c`: `bmAttributes 0xC0`, `bMaxPower 0x32`; `usbd_conf.h`: `USBD_SELF_POWERED 1`). |

### Polling, once per UI loop iteration

The UI loop calls `usb::service(&mut usb_parts, &mut unit)` at its top, before `controls.snapshot()`:

1. `usb_dev.poll(&mut [&mut serial])`.
2. Read bytes one at a time with `serial.read(&mut [u8; 1])` into `Console::push`, until it yields a request or the port is empty. Unread bytes wait in usbd-serial's 128-byte buffer, and past that the hardware NAKs the host. So a pasted burst is never lost, only slowed.
3. With a request: `answer` it into a `UsbOut`, then drop the iteration from the loop timer. **At most one request per iteration**, so a burst of `shot`s cannot hold the UI for longer than one shot at a time.

`UsbOut::put` pumps: it alternates `usb_dev.poll` and `serial.write` until every byte is taken. If no byte is taken for `STALL_MS` = 250 ms (DWT), it returns `Stalled`; the answer stops there and the loop goes on. That covers a host that stopped reading, a pulled cable and a suspended bus. The host tool resynchronises (§ Host tool).

### Why polling and not an interrupt

| | Polled from the UI loop (chosen) | OTG interrupt below audio |
|---|---|---|
| Audio | Can't preempt it: the console only runs where the UI does. | Would need its own level under `AUDIO` (0) and `MIDI` (4) in `priority.rs`, e.g. 8. It also could not touch the framebuffer or `UiState` (both the UI loop's), so requests would cross to the loop through a queue and answers come back through another. |
| Framebuffer | Read between frames by the only code that draws: no copy, no tearing (§ Snapshot point). | Needs a 150 KB copy or a handshake with the loop. |
| Latency | One loop iteration (normally a few ms). During card work under BUSY, up to that work's length (about 100 ms for a project load). Linux gives each control transfer 5 s (`USB_CTRL_GET_TIMEOUT`), and bulk transfers simply NAK until served. | Sub-millisecond, which a console doesn't need. |
| State shared with the audio interrupt | None. | None directly, but more `static` sharing. |

Polling wins for a console. USB MIDI (#203) will need the interrupt (§ Later).

The USB crates take short critical sections (`interrupt::free` in `synopsys-usb-otg`'s `poll` and endpoint read/write). Each one copies at most a 64-byte packet through the FIFO, so it is microseconds long. The audio DMA's half buffer is 2.67 ms, so that delay to the audio interrupt costs no samples. The ship check confirms it (§ Tests): `overruns` stays at 0 through repeated `shot`s.

### Snapshot point: no tearing, no copy

The console runs at the top of the UI loop. Only the UI loop writes the framebuffer: the audio, MIDI and SysTick interrupts never draw. At the loop's top, the previous iteration has already flushed every dirty region, or the whole screen after a recolour, BUSY or a toast. So the framebuffer through the palette is exactly what the panel shows.

`answer` reads it there and streams it before the loop draws again. While a shot streams, the UI does not run. At full speed's real bulk rate on Linux (about 0.5 to 1 MB/s for a polled device), the 153,600 bytes take 0.15 to 0.3 s. The screen holds still for that long, and audio and MIDI go on. Key presses during it are latched by the control tick with their timestamps (`2026-10-01-settings-menu-design.md` § State and storage) and act on the next frame. The watchdog is kicked from SysTick while the audio is alive (`watchdog::kick_if_audio_alive`), so a held UI loop never resets the chip.

### The unit's `Unit`

`ChipUnit { ui: &UiState, stats: Option<&mut Reader<AudioStats>>, loop_timer: &mut LoopTimer, display: &Display }`:

- `stats()` returns `None` without `perf-probe`. Otherwise it gives the latest `AudioStats` with `stack_used` filled, as the loop does for the page, and takes and resets the loop timer.
- `bench()` returns the bench text in bench builds, and `None` otherwise.
- `frame()` returns `Stm32Display::frame()`, a new `&self` method returning `(&fb, palette)`.

`LoopTimer` is in the shell: a DWT stamp at each loop top, a sum, a count and a peak. It costs nothing when the console is cut, because it is cut with it.

## The desktop shell: `chimera-desktop/src/console.rs`

- **One socket, one client.** A `std::net::TcpListener` on `127.0.0.1:7341`, non-blocking, created at launch. If the port is taken, the sim prints `console: 127.0.0.1:7341 busy, console off` and runs without it.
- **Polled like the chip.** At the top of each frame, the shell accepts a waiting client, which replaces any old one, and reads what is there into `Console::push`. It answers at most one request per frame, at the same snapshot point: before the frame's input and draw.
- **The same stall rule.** The stream is non-blocking. `Out::put` retries `WouldBlock` with no progress until the same 250 ms stall, measured with `Instant`.
- **Its `Unit`:**
  - `stats()` is `None` (the desktop has no AUDIO LOAD) and `bench()` is `None`;
  - `ui()` and `frame()` are real, with `frame()` added to `chimera-desktop/src/display.rs`;
  - so `help`, `status` and `shot` work there in full.
- **Why a socket and not a PTY:** a socket is in `std` and works with no libc call and no `nix`. The host tool already speaks to both.

## Host tool

`tools/chimera-usb.py`: Python 3, standard library only (`termios`, `tty`, `socket`, `zlib`, `struct`). A Rust bin would need a termios or serial-port crate and a PNG crate in the lockfile for about 120 lines of glue, and the host side has no real-time or `no_std` constraint for the type system to guard.

- **Target:** `CHIMERA_USB` picks it. If unset, `/dev/chimera` (the udev symlink below) when it exists, else `/dev/ttyACM0`. `sim` means `127.0.0.1:7341`. Any other value is a device path, e.g. `/dev/serial/by-id/usb-Chimera_…`.
- **Serial setup:** `tty.setraw` on the device. Without raw mode, the tty's default echo would send the unit's own answers back to it as requests.
- **Each request:**
  1. drain any pending input for 50 ms, which drops the tail of an answer that stalled earlier;
  2. send `<cmd>\n`;
  3. read until the terminal line.

  If nothing arrives for 2 s, it fails with `no answer from <target>`. The exit status is 0 on `OK` and 1 on `ERR` or a timeout. The body goes to stdout, and the `ERR` line to stderr.
- **`shot` and `shot raw`:** checks the header, reads exactly `length` bytes, and writes a PNG at 2× nearest-neighbour, as `just screens` does. RGB565 widens to RGB888 by bit replication (`r8 = r5 << 3 | r5 >> 2`), so white stays 255. The file is `target/shots/shot-<YYYYmmdd-HHMMSS>.png`, or `shot-raw-…` for `shot raw`, and the tool prints the path.

`Justfile`:

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

`tools/70-chimera.rules`. The owner installs it once, into `/etc/udev/rules.d/`:

```
SUBSYSTEM=="tty", ATTRS{idVendor}=="1209", ATTRS{idProduct}=="0001", ENV{ID_MM_DEVICE_IGNORE}="1", TAG+="uaccess", SYMLINK+="chimera"
```

- `ID_MM_DEVICE_IGNORE` keeps ModemManager from probing the port with AT commands when it appears. The console would only answer them with `ERR unknown command`, but the probe holds the port for seconds.
- `uaccess` gives the logged-in user the port without the `dialout` group.
- `/dev/chimera` is a stable name for the tool's default target.

## Real-time rules, checked

| Rule | How |
|---|---|
| USB never disturbs audio | No USB interrupt. The console runs in the UI loop. Crate critical sections are packet-sized; `overruns` is checked across shots at ship. |
| No heap | None in the core or the shells. `usb-device` and `usbd-serial` are `no_std`, with no `alloc`. |
| No `unsafe` without `// SAFETY:` | The take-once statics (`EP_MEMORY`, the bus allocator) and the CRS register writes, each with its note. |
| Audio thread never blocks | Untouched. |
| Framebuffer read safely | At the loop top, by the only writer (§ Snapshot point). |

## Cost

| | Estimate | Basis |
|---|---|---|
| Flash | **about 20 KB**: `usb-device` 6–8, `synopsys-usb-otg` 4–6, `usbd-serial` 2, the console core 3, the shell and CRS 1–2 | Typical sizes of these crates in Cortex-M CDC builds at `opt-level = 2`. `core::fmt` is already linked. |
| Flash headroom | The brief puts flash at about 79 % of 896 KB (708 KB). With this, it is about 81 %. The last local release ELF (2026-10-01) measured 615,104 bytes (`.vector_table + .text + .rodata + .data`, 67 %). Either way the console is about 2.2 points. | `llvm-size -A`. |
| AXI SRAM | **about 2.5 KB**: `EP_MEMORY` 1 KB, `UsbBusAllocator` + `UsbDevice` + `SerialPort` (two 128-byte buffers) about 1 KB, `Console` 66 bytes, serial string 24 bytes, `LoopTimer` 16 bytes. 6 KB more for `BENCH_TEXT` in bench builds only. | `.data + .bss` is 372 KB of 512 KB today, so about 140 KB is free. |
| Stack | The 480-byte row buffer and the pump frames, under 1 KB at the deepest. `just stack-check`'s 8 KB step rule holds. | |
| CPU, idle | One `poll` per loop iteration, which reads `GINTSTS`: around a microsecond. | |

The plan's first task measures the real flash cost with `llvm-size`. Over 24 KB, it stops and reports before going on.

## Tests

- **Core unit tests** (`chimera-core`, host), with a fake `Unit` and a `Vec<u8>`-backed `Out` in the test:
  - **parsing:** every `Command::ALL` name parses, in any case. `shot` gives `Colours::Theme` and `shot raw` (any case) `Colours::Raw`; `shot x` and `shot raw raw` give `Arguments(Shot)`. `help` lists `ALL` in order and nothing else. LF, CR and CRLF each give one request. Empty lines give none. Leading and trailing spaces are trimmed. A 64-byte command line parses; a 65-byte one gives `TooLong`, and the next line parses cleanly. An unknown word is cut to 16 bytes in its error. Arguments give `Arguments`.
  - **every refusal and every `None`** gives exactly one `ERR` line and nothing before it.
  - **`status`:** each `Loc` variant's `at` line, with SETTINGS' breadcrumbs from a walked path, and each `ProjectStatus` word.
  - **`shot`:** the header, a body of exactly 153,600 bytes, then `OK`. A framebuffer with known pixels comes back big-endian through a non-identity palette, and through no palette for `shot raw`.
  - **`Stalled` mid-shot** stops the answer at that point.
  - **a property test:** random byte streams never panic, and every answer ends in exactly one `OK` or `ERR` line.
- **Desktop QA** (`qa.rs` harness): the sim answers `status` after a scripted key walk with the expected `at` line. `CHIMERA_USB=sim just shot` writes a PNG equal, pixel for pixel, to the window's frame.
- **The ship flash** (the owner, once, with this branch's other checks):

  | # | Check |
  |---|---|
  | U1 | The unit enumerates: `/dev/chimera` (or `/dev/ttyACM0`) appears, and `just usb help` answers. |
  | U2 | `just status` matches the screen, in SETTINGS and on a Part page. |
  | U3 | `just shot` matches the panel, THEME accent included; `just shot raw` is in teal on black whatever THEME is set to. |
  | U4 | `just stats` while a chord plays: `overruns` the same before and after ten `just shot`s in a row. |
  | U5 | Unplug USB mid-shot: the UI resumes within 250 ms, and the next `just status` answers after replugging. |
  | U6 | `just flash` still works, with BOOT0 bridged and the console port open in another terminal. |
  | U7 | A bench build: `just usb bench` gives the B1–B3 numbers the screens showed. |

## Later, kept open but out of scope

- **USB MIDI on the same cable (#203)** makes the device composite: CDC-ACM and MIDI streaming behind interface association descriptors (`UsbDeviceBuilder::composite_with_iads()` in `usb-device` 0.3).
  - OTG_FS has 9 endpoints (`ENDPOINT_COUNT`). CDC uses 3 plus EP0 and MIDI needs 2, so they fit, as do the TX FIFOs in 4 KB.
  - MIDI needs lower latency than the UI loop gives (it stalls under BUSY and during a shot). So MIDI moves USB to an interrupt at its own level, below `AUDIO`, with the console's bytes crossing to the UI loop through SPSC queues. `NoteSources` already has room for a second chip source (ADR 0019, `MAX_NOTE_SOURCES = 2`).
  - The console core does not change: it only ever sees bytes. That switch needs its own ADR, superseding this one's "polled" decision.
- **USB audio (#204)** would join the same composite device, and has the same consequence.
- **Write commands** take arguments under the same grammar. Each becomes a `commands!` row with a typed argument parser (an enum per argument kind, never a raw string past the parser), e.g. a key press by name, a cell value, or `dfu` to restart into the ROM loader (system memory, AN2606) so `just flash` needs no BOOT0 bridge. A write acts through the same input path the keys use, so it obeys the lerp rule and the prompts.
- **SYSTEM › USB CONFIG (#269)** could later turn the console off or choose the composite's parts.

## The owner's answers (2026-10-02)

The five open questions are resolved; the design above already reads this way.

1. **USB identity:** the pid.codes test ID `1209:0001` for now. Applying for a free pid.codes PID is https://github.com/joegiralt/chimera/issues/299. The shared V-USB ID `16C0:27DD` is not used.
2. **What `shot` shows:** both. `shot` is THEME's colours, as the panel shows them. `shot raw` is the canonical palette, matching `docs/screens` and the goldens. So `shot` takes one optional argument (§ Requests, § Answers).
3. **The screen during a shot:** the UI holds still for the 0.15 to 0.3 s the shot streams. No 150 KB copy, no compression.
4. **Host tool language:** Python 3, standard library only, no dependencies.
5. **When the port appears:** once the UI loop starts, after the splash. A hang during boot is not readable over it.
