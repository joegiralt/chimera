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

This version is read-only, with one exception: `dfu` restarts the unit into ST's ROM DFU loader, and so does SETTINGS › SYSTEM › OS UPGRADE. Then `just flash` needs no BOOT0 jumper (§ Enter DFU from the firmware). USB MIDI on the same cable (#203) and the other write commands come later; § Later says what this design keeps open for them.

## Research

| Question | Answer | Source |
|---|---|---|
| Which peripheral and pins drive the USB connector? | **USB2 OTG_FS on PA11 (DM) and PA12 (DP), AF10**, internal full-speed PHY, device only, VBUS sensing off. In the HAL this is `usb_hs::USB2` with `OTG2_HS_GLOBAL/DEVICE/PWRCLK`, the same block under RM0433's other name. PB14/PB15 (OTG_HS's internal PHY) are ruled out on this board: they are SPI2's MISO and MOSI for the SD card (`chimera-stm32/src/main.rs`, `SdParts`; `docs/chimera-synth-design.md` § Storage). | Stock firmware `firmware/Src/usbd_conf.c` (`HAL_PCD_MspInit`: `USB_OTG_FS`, `GPIO_PIN_11\|GPIO_PIN_12`, `GPIO_AF10_OTG1_FS`; `Init.vbus_sensing_enable = DISABLE`, `phy_itface = PCD_PHY_EMBEDDED`) and `firmware/preenfm3.ioc` (`PA11.Signal=USB_OTG_FS_DM`, `PA12.Signal=USB_OTG_FS_DP`, `USB_OTG_FS.VirtualMode=Device_Only`), github.com/Ixox/preenfm3 at `master`. HAL: `stm32h7xx-hal-0.16.0/src/usb_hs.rs` (`USB2::new` takes `PA11<Alternate<10>>`, `PA12<Alternate<10>>`). |
| Where does the 48 MHz USB clock come from? | **HSI48, trimmed by the CRS from USB2's start-of-frame.** The HAL's `freeze` always turns HSI48 on (`rcc/mod.rs`: `hsi48on().on()`), and `kernel_usb_clk_mux(UsbClkSel::Hsi48)` routes it to USB. A PLL output is not an option: PLL1 runs the core at a per-revision rate (`clocks.rs`, `rev.cpu_hz()`), PLL2_P is pinned at 100 MHz for the SPIs, and PLL3 is fractional for the SAI's 48 kHz (`init_pll3`), so none can also give an exact 48 MHz. The stock firmware runs USB from HSI48 with no CRS (`firmware/Src/main.c`: `UsbClockSelection = RCC_USBCLKSOURCE_HSI48`), and it works. Chimera adds the CRS anyway, since full speed needs ±0.25 % and the CRS costs three register writes. The CRS sync source for USB2's SOF is `SYNCSRC = 0b11` (`RCC_CRS_SYNC_SOURCE_USB2 = SYNCSRC_1\|SYNCSRC_0`, stock `stm32h7xx_hal_rcc_ex.h`). The core's AHB clock (`hclk` = CPU/2, 200 or 240 MHz) is far above OTG's 30 MHz floor. | As cited. |
| Which crate versions work with today's `Cargo.lock`? | **`stm32h7xx-hal` 0.16.0** (already locked) with its `usb_hs` feature, which pulls **`synopsys-usb-otg` 0.4.0**; **`usb-device` 0.3.2**; **`usbd-serial` 0.2.2**. `synopsys-usb-otg` 0.4 and `usbd-serial` 0.2 both require `usb-device ^0.3`; the HAL's own examples use exactly this set. Adding them locks six new packages (`embedded-io` 0.6.1, `heapless` 0.8.0, `portable-atomic` 1.15.0 and the three USB crates) and changes nothing already locked. `cargo check -p chimera-stm32 --target thumbv7em-none-eabihf` passes with the `usb_hs` feature on (checked 2026-10-02, then reverted). `synopsys-usb-otg` 0.5.0 exists, but the HAL 0.16 pins 0.4. | crates.io dependency metadata; `stm32h7xx-hal-0.16.0/Cargo.toml`; a local `cargo tree -i usb-device`. |
| Does anything here get in the way of `just flash`? (DFU coexistence) | **No, and the console now opens the way to it.** `just flash` runs `dfu-util -d 0x0483:0xdf11`, which talks to ST's ROM DFU loader. The unit reaches that loader in two ways. With BOOT0 bridged, as before. Or, new here, through `dfu` or OS UPGRADE: Chimera restarts and jumps to the loader before its own boot (§ Enter DFU from the firmware). Either way no Chimera code runs while the loader does, the console included. The ROM loader uses the same OTG_FS on PA11/PA12 (ST AN2606, STM32H74x/75x: USB DFU on PA11/PA12), so the connector is shared and nothing else is. The console never uses 0483:DF11 (§ Identity), never writes flash or option bytes, and never touches BOOT0. The stock PreenFM3 bootloader at 0x08000000 exposes the SD card as USB mass storage (`bootloader/Src/usbd_storage_if.c`), not DFU. It runs before Chimera on every reset, the DFU restart included, and jumps on to 0x08020000 (`bootloader/Src/main.c`: `bootJumpToApplication(APPLICATION_ADDRESS)` unless a button is held). | `Justfile` `flash`; `docs/chimera-synth-design.md` § Firmware Loading; AN2606; stock `bootloader/Src/main.c`. |

## What this changes

- **New in `chimera-core`:** `console`, the functional core. Bytes go in, bytes come out, and there is no I/O.
- **New in `chimera-stm32`:** `usb.rs`, the USB shell, behind the feature `usb-console`. It is in `default`, so `--no-default-features` builds without it, as it does without MIDI DIN. The sd-probe build has no console: the synth is not built there.
- **New in `chimera-desktop`:** `console.rs`, the socket shell.
- **New host tool:** `tools/chimera-usb.py` (Python 3, standard library only), its `just` recipes, and `tools/70-chimera.rules` (udev).
- **New for DFU entry:** `chimera_core::boot`, the pure decision after a reset, and `chimera-stm32/src/dfu.rs`, the marker and the jump (§ Enter DFU from the firmware).
- **Touched:** `main.rs`, which calls the shell once per UI loop iteration and checks the DFU marker at the top of `main`. `bench.rs` keeps its report text. `display.rs` (both shells) gains a read-only `frame()`. `priority.rs` gains a comment: USB has no interrupt priority because it is polled. In the SETTINGS tree, SYSTEM › OS UPGRADE changes from an empty one-page leaf to an action row with a prompt.
- **Not touched:** the audio path and `priority.rs`'s levels. SYSTEM › USB CONFIG (#269) stays `Later`.

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
| `ERR <command> takes no arguments` | Anything follows `help`, `status`, `stats`, `bench` or `dfu`. |
| `ERR shot takes raw or nothing` | `shot` is followed by a word other than `raw`, or by more than one word. |
| `ERR line too long, 64 max` | The line ran past `MAX_LINE`; everything up to the next terminator is dropped. |
| `ERR <command> is not in this build` | The shell has no data for it: `stats` without `perf-probe` or on the desktop, `bench` outside a bench build, `dfu` on the desktop. |

### The commands

**`help`**

```
chimera console 1
help    this list
status  firmware, project, Part and where the UI is
stats   AUDIO LOAD and the UI loop's time
bench   the bench's numbers (bench builds)
shot    the screen in THEME's colours; shot raw: canonical
dfu     restart into the ROM loader for just flash
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
- On the chip only, a last line `boot marker=<hex> readback=<hex> action=Synth rsr=<hex> dbp=<0|1> boots=<n> from=<menu|console|none> jump_rsr=<hex> last_stage=<stage> last_usb=<state>`: what the top of `main` saw (`boot::BootSeen`). `marker` and `readback` are RTC_BKP0R before and after the clear, `rsr` is RCC_RSR before RMVF, `dbp` is PWR_CR1.DBP on entry, `boots` counts in BKP1R (it carries on through a power-off only if VBAT kept the backup domain), `from` is BKP2R, written by `dfu::enter`, `jump_rsr` is BKP3R, RCC_RSR at the last jump to the ROM, and `last_stage` is BKP4R on entry, the `boot::BootStage` the boot before this one reached (`entry`, `card`, `audio`, `armed`, `running`, or `none`), and `last_usb` is BKP5R, how that boot's USB step ended: `on`, `off(<hsi48|usb33|ahbidl|csrst>)` when `usb::preflight` found a precondition that never came true in 50 ms and the boot played without USB, or `none`. USB comes up after the watchdog is armed, behind that bounded preflight, because `connect`'s core enable spins with interrupts masked. The desktop has no such line.
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
voices 3 5 of 8
cost_pct 47
loop_avg_us 812
loop_peak_us 4210
OK
```

- The first eight lines are `AudioStats` as the AUDIO LOAD page reads them.
- `drops` holds one count per note source (`sources` of them), separated by spaces; `-` when `sources` is 0.
- `stack_bytes` is `stack_used` (the page rounds it to K; this does not).
- `voices <now> <peak> of 8`: the voices sounding after the last audio block (`Instrument::sounding`), the most since the last `stats`, and `MAX_VOICES`. Reading starts the peak over, as it does `loop_peak_us`. The audio side keeps both in `AudioStats` with plain stores; the console's read asks for the restart through one atomic flag the next block takes.
- `cost_pct` is the voice budget booked after the last block, the FX bus included (`Allocator::cost_pct`): the share the allocator steals and sheds against (ADR 0027). Over 100 means voices are fading to fit; it saturates at 255.
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

**`dfu`**

```
OK
```

Then the unit restarts into ST's ROM DFU loader and leaves the bus as `0483:5740`. Within about 2 s it comes back as `0483:DF11`. The `OK` is flushed, then the port keeps polling for 20 ms so the host collects it, before the restart. A host that still misses it sees the port vanish and DF11 appear, which `to-dfu` also counts as success. The desktop answers `ERR dfu is not in this build`. § Enter DFU from the firmware has the details.

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
    Dfu    (NoArg)   => "dfu",    "restart into the ROM loader for just flash",
}
// expands to:
// #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Command { Help, Status, Stats, Bench, Shot, Dfu }
// impl Command { pub const ALL: [Command; 6]; pub const fn name(self) -> &'static str;
//                pub const fn about(self) -> &'static str; pub const fn usage(self) -> &'static str; }
// #[derive(Clone, Copy, Debug, PartialEq, Eq)]
// pub enum Request { Help(NoArg), Status(NoArg), Stats(NoArg), Bench(NoArg), Shot(Colours), Dfu(NoArg) }
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
    /// Arms the restart into the ROM loader, which the shell makes once
    /// `OK` is out. `None`: not in this build (the desktop).
    fn dfu(&mut self) -> Option<()>;
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
4. `UsbBus::new(usb, EP_MEMORY)`. `EP_MEMORY` is a `[u32; 256]` (1 KB) from `cortex_m::singleton!`, so it is handed out once. It holds the OUT packets only (EP0 and CDC's bulk OUT, 64 bytes each). The TX FIFOs live in the core's own 4 KB RAM.
5. The `UsbBusAllocator` goes in a `singleton!` too, so the device and the class can borrow it for `'static`.
6. `SerialPort::new(&bus)`, then the `UsbDeviceBuilder` with § Identity's values and `device_class(USB_CLASS_CDC)`.

The OTG interrupt is never unmasked in the NVIC (`pac::Interrupt::OTG_FS` stays disabled): nothing runs outside the UI loop.

### Identity

| Field | Value |
|---|---|
| VID:PID | `0483:5740`, the identity the stock PreenFM3 firmware uses (the owner, 2026-10-02; github.com/Ixox/preenfm3 `firmware/Src/usbd_desc.c` (`USBD_VID 1155`, `USBD_PID_FS 22336`)). 0x5740 is ST's example PID for a virtual COM port, which suits CDC-ACM. The stock strings were "STMicroelectronics" / "STM32 Audio Class"; ours stay below. It is never `0483:DF11`, so `dfu-util -d 0x0483:0xdf11` cannot match the running synth. |
| Manufacturer | `Chimera` |
| Product | `Chimera console` |
| Serial number | the chip's 96-bit unique ID (the unique device ID registers at `0x1FF1_E800`, RM0433) as 24 uppercase hex digits, formatted once at `init` into a static. `/dev/serial/by-id/usb-Chimera_Chimera_console_<uid>-if00` is then stable per unit. |
| Power | Self-powered, `bMaxPower` 100 mA (`.self_powered(true).max_power(100)`), as the stock firmware's MIDI descriptor declares (`usbd_midi.c`: `bmAttributes 0xC0`, `bMaxPower 0x32`; `usbd_conf.h`: `USBD_SELF_POWERED 1`). |

### Polling, once per UI loop iteration

The UI loop calls `usb.service(ui, stats, bench, display.frame())` at its top, before `controls.snapshot()`:

1. `usb_dev.poll(&mut [&mut serial])`.
2. Read bytes one at a time with `serial.read(&mut [u8; 1])` into `Console::push`, until it yields a request, the port is empty, or 256 bytes have been read this iteration (so a flood without a newline can't hold the UI). Unread bytes wait in usbd-serial's 128-byte buffer, and past that the hardware NAKs the host. So a pasted burst is never lost, only slowed.
3. With a request: `answer` it into a `UsbOut`, then drop the iteration from the loop timer (`Usb` keeps the iteration's `Served` and hands it to the next `LoopTimer::lap`; `service` returns nothing). **At most one request per iteration**, so a burst of `shot`s cannot hold the UI for longer than one shot at a time.

`UsbOut::put` pumps: it alternates `usb_dev.poll` and `serial.write` until every byte is taken. If no byte is taken for `STALL_MS` = 250 ms, or the whole answer passes `ANSWER_MS` = 1000 ms (both read by one `AnswerClock` per answer, on `controls::now_ms()`: SysTick always runs, while the DWT counter can fail to start), it returns `Stalled`; the answer stops there and the loop goes on. That covers a host that stopped reading, a pulled cable and a suspended bus. The host tool resynchronises (§ Host tool).

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

`Usb::service(&mut self, ui: &UiState, stats: Option<&mut Reader<AudioStats>>, bench: Option<&str>, frame: Frame<'_>)` builds a private `ChipUnit { ui, stats, timer, bench, frame }` for the one answer, borrowing the `LoopTimer` from `Usb`'s own fields:

- `stats()` returns `None` without `perf-probe` (no reader). Otherwise it gives `main.rs`'s `audio_stats` (the latest `AudioStats` with `stack_used` filled, the same read the AUDIO LOAD page uses) and takes and resets the loop timer.
- `bench()` returns the bench text in bench builds, and `None` otherwise.
- `frame()` returns `Stm32Display::frame()`, a `&self` method returning `Frame { fb, palette }`.

`LoopTimer` is the core's (`console::shell`), owned by `Usb`. `service` stamps DWT at each loop top and laps the time since the last one, passing the last iteration's `Served`, which `Usb` keeps, so an answered iteration is not counted. It costs nothing when the console is cut, because it is cut with it.

## The desktop shell: `chimera-desktop/src/console.rs`

- **One socket, one client.** A `std::net::TcpListener` on `127.0.0.1:7341`, non-blocking, created at launch. If the port is taken, the sim prints `console: 127.0.0.1:7341 busy, console off` and runs without it.
- **Polled like the chip.** At the top of each frame, the shell accepts a waiting client, which replaces any old one, and reads what is there into `Console::push`. It answers at most one request per frame, at the same snapshot point: before the frame's input and draw.
- **The same stall rule.** The stream is non-blocking. `Out::put` retries `WouldBlock` until the same `AnswerClock` expires: 250 ms with no progress, or 1000 ms for the whole answer, measured with `Instant`.
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
SUBSYSTEM=="tty", ATTRS{idVendor}=="0483", ATTRS{idProduct}=="5740", ENV{ID_MM_DEVICE_IGNORE}="1", TAG+="uaccess", SYMLINK+="chimera"
```

- `ID_MM_DEVICE_IGNORE` keeps ModemManager from probing the port with AT commands when it appears. The console would only answer them with `ERR unknown command`, but the probe holds the port for seconds.
- `uaccess` gives the logged-in user the port without the `dialout` group.
- `/dev/chimera` is a stable name for the tool's default target.

## Enter DFU from the firmware

The owner's ask (2026-10-02): today, flashing means moving the BOOT0 jumper on the back of the unit, re-plugging it, then running `just flash`. The unit should be able to enter DFU itself instead, from the menu or from the console, so `just flash` runs with no jumper.

### The trigger: SETTINGS › SYSTEM › OS UPGRADE

- Today OS UPGRADE is `Leaf(&UPDATES_LEAF)`, a `OnePage` over `SYS_UPDATES` (`block_registry.rs`: six `EMPTY` cells, `VizType::None`). It draws a header and nothing else, so it holds nothing that ABOUT doesn't already show (VERSION, BUILD, REV, CLOCK, RESET, CARD). Nothing moves. `SYS_UPDATES`, `UPDATES_BLOCKS` and `UPDATES_LEAF` go, and block id 34 is retired, not reused.
- The row becomes `crumb("OS UPGRADE", "OS", Kind::Act(Act::EnterDfu))`. SEQ on it opens a prompt:

  | | |
  |---|---|
  | Question | `ENTER DFU?` |
  | Reason | `PLAY STOPS UNTIL FLASHED OR POWER-CYCLED` (the prompt wraps it to two lines, as it does other long reasons) |
  | Pills | `ENTER DFU`, `CANCEL` (`DfuAnswer`, `Two`; the confirming pill first, as DELETE's and CLEAR's are) |

- It is sealed like every other confirming prompt. `commits!(DfuAnswer => EnterDfu, RomDfu)` and `replace::said` make the only `Said<RomDfu>`. `UiState` keeps that yes until the shell takes it (`take_dfu_synced`), and `dfu::enter` takes it as its argument. `take_dfu_synced` runs `sync_system_now` (a card write) before it hands the yes over, as leaving SETTINGS would: the restart never leaves SETTINGS, so a THEME change made in the same visit would otherwise be lost. The console's `dfu` path calls `sync_system_now` before `enter` too. So no code path enters DFU from the menu without a SEQ on `ENTER DFU`. MENU and `CANCEL` close the prompt and play goes on.
- `RomDfu` is a unit type in `chimera_core::boot`. It is `Witnessed` with `Witness = ()`, sealed in `project::guard` beside the four targets: entering DFU loses nothing on the card, so there is nothing to witness.
- There is no SAVE FIRST pill. Entering DFU is a power-cycle, so an unsaved project is lost exactly as it is at the power switch. The footer shows MODIFIED, and MENU hold saves.

### The console command

`dfu` (§ The commands) answers `OK`. The shell flushes it and keeps polling for `DRAIN_MS` (20 ms) so the host collects the last IN packet, then enters DFU exactly as the menu's yes does. Only `Usb::service` makes the `DfuAsked` that `DfuFrom::Console` needs, and only after that drain. `Unit::dfu` returns `None` on the desktop, which answers `ERR dfu is not in this build`, and the menu's yes there prints `dfu: not in this build` on stderr and play goes on. `dfu` takes no argument.

### The mechanism

1. **The marker.** The firmware writes `DFU_MAGIC` to RTC_BKP0R (0x5800_4050). This register is in the backup domain, which a system reset does not clear. RCC_APB4ENR.RTCAPBEN clocks its bus, and PWR_CR1.DBP unlocks it for writing (the HAL's PWR `freeze` sets DBP and leaves it set, `pwr.rs`). Chimera uses no RTC. The HAL's `rtc::Rtc` is not used: `Rtc::init` resets the backup registers, and `open_or_init` wants an RTC clock source. The PAC's `RTC.bkpr[0]` is enough.
2. **The reset.** `cortex_m::peripheral::SCB::sys_reset()`. ABOUT's RESET will read SOFTWARE after it.
3. **The stock bootloader** at 0x08000000 runs first, as on every reset. It does `HAL_Init`, `SystemClock_Config`, then `MX_Deinit` (`HAL_RCC_DeInit`, `HAL_SuspendTick`) and jumps to 0x08020000 unless a button is held. Its `main.c` names no RTC, backup or IWDG register, and `HAL_RCC_DeInit` leaves RCC_BDCR alone. Its data and stack are in DTCM and the top of AXI SRAM (`STM32H753VITX_FLASH.ld`: `.data`/`.bss` in DTCMRAM, `_estack = 0x24080000`). So the marker should survive it, but that is checked on the unit (§ The risk).
4. **The check, at the top of `#[entry] fn main`,** before `boot()`. RAM is set up there, and no clock or peripheral of Chimera's is touched yet. It does not go in `#[pre_init]`: cortex-m-rt 0.7.5 calls a Rust `pre_init` unsound because it runs before RAM is initialised, and Chimera has no `before_main`. `main` takes `cortex_m::Peripherals` and `pac::Peripherals` itself and hands them to `boot(cp, dp, checked)`. It reads BKP0R through the owned `RTC`, after setting RTCAPBEN through the owned `RCC`. It **always** clears the register (DBP through the owned `PWR`, read back), whatever it held, and reads it back. The pure `boot::after_reset(marker, readback)` decides: only `DFU_MAGIC` that read back 0 jumps. A stuck marker (a read-back that isn't 0) boots the synth, so it can never trap the unit in DFU; a transient stuck leaves the magic in place, so a later clean reset may still enter DFU, which is accepted (the ROM loader can't brick the unit). `after_reset` returns `dfu::Checked`, which `boot` takes, and `boot` pairs it with the owned `RTC` into the `dfu::Marker` that `enter` needs:
   - `BootAction::Synth`: carry on into `boot()`, as today;
   - `BootAction::RomDfu`: stop SysTick (`SYST_CSR = 0`; the stock bootloader's `HAL_SuspendTick` leaves it counting), disable and unpend every NVIC line (ICER and ICPR 0..8), set `SCB.VTOR = ROM_DFU_BASE`, and `cortex_m::asm::bootload(ROM_DFU_BASE as *const u32)`. That reads the MSP from the ROM's vector table, sets it, and branches to the ROM's reset vector. `ROM_DFU_BASE` is 0x1FF0_9800, the STM32H74x/75x system memory bootloader (ST AN2606). This block is the feature's one `unsafe`, with a `// SAFETY:` note naming what it relies on: a fixed ROM vector table; PRIMASK clear; the NVIC cleared; the caches and MPU as the stock bootloader left them; RTCAPBEN and DBP set, which the loader ignores.
5. **The ROM loader** enumerates as `0483:DF11` on PA11/PA12, and `dfu-util` flashes as it does after the jumper. `:leave` resets into the stock bootloader and then into the new Chimera, with the marker clear.

The decision is pure and host-tested:

```rust
// chimera_core::boot
pub const DFU_MAGIC: u32 = 0x4446_5521;          // "DFU!"
pub const ROM_DFU_BASE: u32 = 0x1FF0_9800;       // ST AN2606, STM32H74x/75x system memory
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction { Synth, RomDfu }
/// The marker read after a reset. Only `DFU_MAGIC` enters the ROM loader.
/// 0, a power-on's garbage and anything else boot the synth.
pub const fn after_reset(marker: u32, readback: u32) -> BootAction;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;                               // what the prompt's yes confirms
```

Because the shell clears the marker before it acts, a marker can never trap the unit in DFU: the next reset or power-cycle always plays.

### The risk, checked on the unit

The stock bootloader, the reset or Chimera's own early path could clobber RTC_BKP0R. If one does, `dfu` answers `OK`, the unit restarts, and the synth comes back as `0483:5740` instead of `0483:DF11`. `just flash` then times out with that message, and ABOUT's RESET reads SOFTWARE. The flash checklist's clobber check (U10) looks for exactly that, and an issue is filed if it fails. A possible future alternative is a word of D3 SRAM4 (0x3800_FFFC): neither linker script maps RAM_D3, and SRAM keeps its contents through a system reset. It is not built. Two caveats come with it: SRAM is ECC-protected, so a read of a word never written since power-on can raise an ECC error on a cold boot, and the marker write must reach SRAM past the D-cache before the reset (a non-cacheable MPU region or a clean).

Two more things are only checked on the unit:

- the ROM loader runs after the stock bootloader's `HAL_RCC_DeInit` state, as it does from reset (U8: `0483:DF11` appears);
- the IWDG, started by the previous Chimera, does not survive the system reset into the ROM loader (U8: the DFU device is still there 30 s later).

### `just flash` hands-free

`just flash` and `just flash-bench` build, then run `python3 tools/chimera-usb.py to-dfu` before `dfu-util`:

- **The console is there** (a USB device `0483:5740` in `/sys/bus/usb/devices`): it sends `dfu`, needs `OK`, then waits up to 10 s for `0483:DF11` to appear and exits 0. The port vanishing before `OK` arrives, then DF11 appearing, is also success: the `OK` was lost to the reset, not refused. If DF11 does not appear, it exits 1 with `no DFU device after dfu: see U10 (marker clobbered?)`, and the recipe stops before `dfu-util`.
- **DF11 is already there** (the jumper): it does nothing and exits 0.
- **Neither:** it prints `no console: bridge BOOT0 on the back and re-plug for DFU` and exits 0, so `dfu-util` runs and behaves as today.

### Safety

The unit cannot be bricked by this. The ROM loader is in system memory and cannot be written. Chimera never writes flash, option bytes or BOOT0. The marker is cleared before the jump, so every following reset plays. And the BOOT0 jumper still reaches the ROM loader whatever is in flash.

## Real-time rules, checked

| Rule | How |
|---|---|
| USB never disturbs audio | No USB interrupt. The console runs in the UI loop. Crate critical sections are packet-sized; `overruns` is checked across shots at ship. |
| No heap | None in the core or the shells. `usb-device` and `usbd-serial` are `no_std`, with no `alloc`. |
| No `unsafe` without `// SAFETY:` | The take-once statics (`EP_MEMORY`, the bus allocator), the CRS register writes and the jump to the ROM loader, each with its note. |
| DFU entry never touches audio mid-block | `dfu::enter` runs from the UI loop. The reset stops the SAI with everything else, which is a power-cycle's click at worst. |
| Audio thread never blocks | Untouched. |
| Framebuffer read safely | At the loop top, by the only writer (§ Snapshot point). |

## Cost

| | Estimate | Basis |
|---|---|---|
| Flash | **26,040 B measured** (Task 8): `synopsys-usb-otg` 12,886, the console core 7,342, `usb-device` 1,952, the shell 1,820, `usbd-serial` 154, the rest about 1,900 | `llvm-size -A`, release, default build against `midi-din,perf-probe`. |
| Flash headroom | The brief puts flash at about 79 % of 896 KB (708 KB). With this, it is about 81 %. The last local release ELF (2026-10-01) measured 615,104 bytes (`.vector_table + .text + .rodata + .data`, 67 %). The console is about 2.8 points: 747,144 bytes with it. | `llvm-size -A`. |
| AXI SRAM | **+1,484 B measured** (Task 8): `EP_MEMORY` 1 KB, the bus allocator, the serial string. `Usb` (device, port, `Console`, `LoopTimer`) lives on `synth`'s stack. 6,152 B more for the bench report in bench builds only. | `.data + .bss` is 372 KB of 512 KB today, so about 140 KB is free. |
| Stack | The 480-byte row buffer and the pump frames, under 1 KB at the deepest. `just stack-check`'s 8 KB step rule holds. | |
| CPU, idle | One `poll` per loop iteration, which reads `GINTSTS`: around a microsecond. | |
| DFU entry | **well under 1 KB of flash**: `after_reset` and the early check (tens of instructions), the prompt's words and pills (about 70 bytes), the `dfu` table row (about 50 bytes), `enter`. No RAM: the marker is a backup register. The empty UPDATES page's block and leaf go. Boot time: one register read and one write before `boot()`. | The plan's DFU task measures it and stops above 1 024 bytes. |

The plan's first task measures the real flash cost with `llvm-size`. Over 24 KB, it stops and reports before going on. Task 8 measured the whole console at 26,040 B; budget raised to 28 KB by the owner, 2026-10-02.

## Tests

- **Core unit tests** (`chimera-core`, host), with a fake `Unit` and a `Vec<u8>`-backed `Out` in the test:
  - **parsing:** every `Command::ALL` name parses, in any case. `shot` gives `Colours::Theme` and `shot raw` (any case) `Colours::Raw`; `shot x` and `shot raw raw` give `Arguments(Shot)`. `help` lists `ALL` in order and nothing else. LF, CR and CRLF each give one request. Empty lines give none. Leading and trailing spaces are trimmed. A 64-byte command line parses; a 65-byte one gives `TooLong`, and the next line parses cleanly. An unknown word is cut to 16 bytes in its error. Arguments give `Arguments`.
  - **every refusal and every `None`** gives exactly one `ERR` line and nothing before it.
  - **`status`:** each `Loc` variant's `at` line, with SETTINGS' breadcrumbs from a walked path, and each `ProjectStatus` word.
  - **`shot`:** the header, a body of exactly 153,600 bytes, then `OK`. A framebuffer with known pixels comes back big-endian through a non-identity palette, and through no palette for `shot raw`.
  - **`Stalled` mid-shot** stops the answer at that point.
  - **a property test:** random byte streams never panic, and every answer ends in exactly one `OK` or `ERR` line.
  - **`dfu`:** `OK` and one `Unit::dfu` call; `None` gives `ERR dfu is not in this build`.
  - **`boot::after_reset`:** only `DFU_MAGIC` gives `RomDfu`.
  - **OS UPGRADE:** SEQ opens the DFU prompt with its words; `ENTER DFU` gives exactly one `Said<RomDfu>` through `take_dfu_synced`; `CANCEL` and MENU give none.
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
  | U8 | SETTINGS › SYSTEM › OS UPGRADE: `CANCEL` plays on. `ENTER DFU` stops the sound, and `0483:DF11` appears and stays. A power-cycle without flashing plays. |
  | U9 | No jumper: `just flash` (then `just flash-bench`) sends `dfu`, flashes, and the new build boots and enumerates. |
  | U10 | The clobber check: if U8 or U9 brings back `0483:5740` instead of `0483:DF11`, the marker was cleared on the way. File an issue (§ The risk names SRAM4 as a possible alternative, with its caveats). |

## Later, kept open but out of scope

- **USB MIDI on the same cable (#203)** makes the device composite: CDC-ACM and MIDI streaming behind interface association descriptors (`UsbDeviceBuilder::composite_with_iads()` in `usb-device` 0.3).
  - OTG_FS has 9 endpoints (`ENDPOINT_COUNT`). CDC uses 3 plus EP0 and MIDI needs 2, so they fit, as do the TX FIFOs in 4 KB.
  - MIDI needs lower latency than the UI loop gives (it stalls under BUSY and during a shot). So MIDI moves USB to an interrupt at its own level, below `AUDIO`, with the console's bytes crossing to the UI loop through SPSC queues. `NoteSources` already has room for a second chip source (ADR 0019, `MAX_NOTE_SOURCES = 2`).
  - The console core does not change: it only ever sees bytes. That switch needs its own ADR, superseding this one's "polled" decision.
- **USB audio (#204)** would join the same composite device, and has the same consequence.
- **Write commands** take arguments under the same grammar. Each becomes a `commands!` row with a typed argument parser (an enum per argument kind, never a raw string past the parser), e.g. a key press by name or a cell value. (`dfu`, the first command that acts, is in this version: § Enter DFU from the firmware.) A write acts through the same input path the keys use, so it obeys the lerp rule and the prompts.
- **SYSTEM › USB CONFIG (#269)** could later turn the console off or choose the composite's parts.

## The owner's answers (2026-10-02)

The five open questions are resolved; the design above already reads this way.

1. **USB identity:** `0483:5740`, the stock PreenFM3 identity (github.com/Ixox/preenfm3 `firmware/Src/usbd_desc.c` (`USBD_VID 1155`, `USBD_PID_FS 22336`)). The shared V-USB ID `16C0:27DD` is not used.
2. **What `shot` shows:** both. `shot` is THEME's colours, as the panel shows them. `shot raw` is the canonical palette, matching `docs/screens` and the goldens. So `shot` takes one optional argument (§ Requests, § Answers).
3. **The screen during a shot:** the UI holds still for the 0.15 to 0.3 s the shot streams. No 150 KB copy, no compression.
4. **Host tool language:** Python 3, standard library only, no dependencies.
5. **When the port appears:** once the UI loop starts, after the splash. A hang during boot is not readable over it.

A later ask the same day:

6. **Enter DFU from the firmware.** Flashing should need no BOOT0 jumper. SETTINGS › SYSTEM › OS UPGRADE asks `ENTER DFU?`, and the console's `dfu` does the same. `just flash` uses `dfu` when the console is there, and falls back to the jumper otherwise (§ Enter DFU from the firmware).
