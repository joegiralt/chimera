//! On-unit SD bring-up (`--features sd-probe`): clocks, acquire in each SPI
//! mode, the volume, then through `SdStore` (our FAT layer, ADR 0048) a
//! mount and a `/CHIMERA` listing, a 16 KB write/read/delete at two clocks,
//! and the RAM sizes plan 1 re-measures. Then halts, LED on.

use core::fmt::Write as _;
use core::ops::ControlFlow;

use chimera_core::hw::{AXI_SRAM, VOICE_RAM_BUDGET};
use chimera_core::instrument::{AXI_RESIDENT, AudioShared, Instrument};
use chimera_core::preset::{Performance, Sound};
use chimera_core::project::Pool;
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::UiState;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_fat::volume::{PartitionType, boot_sector, first_partition};
use chimera_fat::{BusPhase, SdBus};
use chimera_hal::ChimeraDisplay;
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, ReadSink, Store};
use cortex_m::peripheral::DWT;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx, SdCardError};
use stm32h7xx_hal::spi;
use stm32h7xx_hal::time::Hertz;

use crate::clocks::{self, Clocks};
use crate::sd::{SD_ACQUIRE_MS, SD_INIT_HZ, SdDevice, SdStore};

const LINE_H: i32 = 10;
/// Long enough to photograph a full page before the next one clears it.
const PAGE_HOLD_S: u32 = 15;
const LIST_MAX: usize = 8;
const PROBE_BLOCKS: u32 = 32;
const PROBE_KB: u32 = PROBE_BLOCKS / 2;
const TEST_HZ: [u32; 2] = [12_500_000, 25_000_000];
/// Each tried from a cold wake; the first that answers is used.
const MODES: [(&str, spi::Mode); 3] = [("0", spi::MODE_0), ("3", spi::MODE_3), ("1", spi::MODE_1)];

/// Lines down the screen, a page at a time.
struct Console<'a, D: ChimeraDisplay> {
    display: &'a mut D,
    cpu_hz: u32,
    y: i32,
    buf: FmtBuf,
}

impl<D: ChimeraDisplay> Console<'_, D> {
    fn clear(&mut self) {
        draw::fill_rect(
            self.display,
            0,
            0,
            theme::SCREEN_W,
            theme::SCREEN_H,
            theme::BG,
        );
        self.y = LINE_H;
    }

    fn line(&mut self, args: core::fmt::Arguments) {
        if self.y > theme::SCREEN_H - 2 {
            for _ in 0..PAGE_HOLD_S {
                clocks::delay_us(self.cpu_hz, 1_000_000);
            }
            self.clear();
        }
        self.buf.clear();
        let _ = self.buf.write_fmt(args);
        draw::text(
            self.display,
            &theme::FONT_LABEL,
            self.buf.as_str(),
            4,
            self.y,
            theme::INK,
        );
        self.y += LINE_H;
        // Shown as it goes, so a hang shows where.
        self.display.flush();
    }
}

fn ms_since(cpu_hz: u32, start: u32) -> u32 {
    (u64::from(DWT::cycle_count().wrapping_sub(start)) * 1_000 / u64::from(cpu_hz)) as u32
}

/// The probe file's byte at `at`: each 512 B block shifted by its index.
fn pattern(at: u32) -> u8 {
    ((at / 512) * 7 + at % 512) as u8
}

/// Compares a read against `pattern`, byte by byte.
struct Compare {
    at: u32,
    same: bool,
}

impl ReadSink for Compare {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.same = len == PROBE_BLOCKS * 512;
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        for &b in bytes {
            self.same &= b == pattern(self.at);
            self.at += 1;
        }
        ControlFlow::Continue(())
    }
}

pub fn run(display: &mut impl ChimeraDisplay, clk: Clocks, store: &mut SdStore) -> ! {
    let cpu_hz = clk.cpu_hz;
    let mut c = Console {
        display,
        cpu_hz,
        y: LINE_H,
        buf: FmtBuf::new(),
    };
    c.clear();

    let sd = store.device();
    let (ker, sck, counts) = sd.spi(|s| {
        let (ker, sck) = s.sck_hz();
        (ker, sck, s.counts_cycles())
    });
    let ok = if sck <= SD_INIT_HZ { "OK" } else { "HIGH" };
    c.line(format_args!("KER {}K SCK {sck} {ok}", ker / 1_000));
    let dwt = if counts { "RUNS" } else { "STOPPED" };
    c.line(format_args!(
        "DWT {dwt} REV {} {} MHZ",
        clk.rev.label(),
        cpu_hz / 1_000_000
    ));

    if acquire(&mut c, sd) && volume(&mut c, sd) {
        files(&mut c, store);
    }
    sizes(&mut c);
    c.line(format_args!("DONE"));
    loop {
        cortex_m::asm::wfi();
    }
}

/// Acquires from a cold wake in each of `MODES`, each on its own, then
/// again in the first that answered, which the card steps then use.
fn acquire<D: ChimeraDisplay>(c: &mut Console<D>, sd: &SdDevice) -> bool {
    let mut first = None;
    for &(name, mode) in &MODES {
        let (bytes, ms, to) = cold_acquire(c.cpu_hz, sd, mode);
        match bytes {
            _ if to => c.line(format_args!("M{name} TO (NO VERDICT) {ms} MS")),
            Ok(n) => {
                first = first.or(Some((name, mode)));
                c.line(format_args!("M{name} OK {ms} MS {} MB", n >> 20));
                c.line(format_args!("  {:?}", sd.get_card_type()));
            }
            Err(e) => {
                c.line(format_args!("M{name} FAIL {ms} MS"));
                c.line(format_args!("  {e:?}"));
            }
        }
    }
    let Some((name, mode)) = first else {
        c.line(format_args!("NO MODE ACQUIRED"));
        return false;
    };
    let ok = cold_acquire(c.cpu_hz, sd, mode).0.is_ok();
    c.line(format_args!(
        "USING M{name} {}",
        if ok { "OK" } else { "FAIL" }
    ));
    ok
}

/// Rebuilds in `mode` at the init clock, wakes and acquires: the result,
/// its ms and whether the deadline ended it.
fn cold_acquire(
    cpu_hz: u32,
    sd: &SdDevice,
    mode: spi::Mode,
) -> (Result<u64, SdCardError>, u32, bool) {
    sd.spi(|s| {
        s.set_mode(mode);
        s.wake();
        s.arm(SD_ACQUIRE_MS);
    });
    sd.mark_card_uninit();
    let t = DWT::cycle_count();
    let bytes = sd.num_bytes();
    let ms = ms_since(cpu_hz, t);
    (bytes, ms, sd.spi(|s| s.timed_out()))
}

/// The MBR and the partition's boot sector, read raw: true if both read.
fn volume<D: ChimeraDisplay>(c: &mut Console<D>, sd: &SdDevice) -> bool {
    // Acquired: the bus is in its data phase, which `SdStore` goes on from.
    sd.spi(|s| {
        s.set_phase(BusPhase::Data);
        s.start_op();
    });
    let mut blk = [Block::new()];
    if let Err(e) = sd.read(&mut blk, BlockIdx(0)) {
        c.line(format_args!("MBR {e:?}"));
        return false;
    }
    let p = match first_partition(&blk[0].contents) {
        Ok(p) => p,
        Err(e) => {
            c.line(format_args!("MBR {e:?}"));
            return false;
        }
    };
    let ty = match p.kind {
        PartitionType::Fat(b) => b.get(),
        PartitionType::ExfatOrNtfs => 0x07,
        PartitionType::Other(b) => b.get(),
    };
    c.line(format_args!("P1 LBA {} TYPE {ty:02X}", p.lba));
    match sd.read(&mut blk, BlockIdx(p.lba)) {
        Ok(()) => match boot_sector(&blk[0].contents, p.kind) {
            Ok((kind, id)) => {
                let label = core::str::from_utf8(&id.label).unwrap_or("?");
                c.line(format_args!("{kind:?} {:08X} {label}", id.serial));
                true
            }
            Err(e) => {
                c.line(format_args!("BOOT {e:?}"));
                false
            }
        },
        Err(e) => {
            c.line(format_args!("BOOT {e:?}"));
            false
        }
    }
}

/// Lines 4 and 5, through `SdStore`: mount and list `/CHIMERA`, then write,
/// read back and delete `/CHIMERA/CHIMPROB.TXT` at each of `TEST_HZ`.
fn files<D: ChimeraDisplay>(c: &mut Console<D>, store: &mut SdStore) {
    let t = DWT::cycle_count();
    let vol = match store.mount() {
        Ok(v) => v,
        Err(e) => {
            c.line(format_args!("MOUNT {e:?}"));
            return;
        }
    };
    c.line(format_args!("MOUNT {} MS", ms_since(c.cpu_hz, t)));
    // Listed first, drawn after: no flush inside the directory walk.
    let mut names: [FmtBuf; LIST_MAX] = core::array::from_fn(|_| FmtBuf::new());
    let mut n = 0;
    let listed = store.list(vol, Dir::Chimera, &mut |f, len| {
        if n < LIST_MAX {
            let stem = core::str::from_utf8(f.stem()).unwrap_or("?");
            let ext = core::str::from_utf8(f.ext()).unwrap_or("?");
            let _ = write!(names[n], "  {stem}.{ext} {len}");
            n += 1;
        }
    });
    for name in &names[..n] {
        c.line(format_args!("{}", name.as_str()));
    }
    if let Err(e) = listed {
        c.line(format_args!("LIST {e:?}"));
    }
    if let Err(e) = store.make_dir(vol, Dir::Chimera) {
        c.line(format_args!("MKDIR {e:?}"));
        return;
    }
    let Some(file) = FileName::new(Dir::Chimera, b"CHIMPROB", b"TXT") else {
        return;
    };

    for hz in TEST_HZ {
        store.device().spi(|s| s.set_hz(Hertz::from_raw(hz)));
        let mut gap = 0;
        let mut gap_since = |store: &SdStore| {
            gap = gap.max(store.device().spi(|s| s.max_gap_us()));
        };
        let t = DWT::cycle_count();
        let wrote = store.write(vol, file, &mut |sink: &mut dyn ByteSink| {
            let mut buf = [0u8; CHUNK];
            for block in 0..PROBE_BLOCKS {
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = pattern(block * 512 + i as u32);
                }
                sink.put(&buf)?;
            }
            Ok(())
        });
        let w_ms = ms_since(c.cpu_hz, t).max(1);
        gap_since(store);
        let t = DWT::cycle_count();
        let mut cmp = Compare { at: 0, same: false };
        let read = wrote.and_then(|_| store.read(vol, file, &mut cmp));
        let r_ms = ms_since(c.cpu_hz, t).max(1);
        gap_since(store);
        let deleted = store.delete(vol, file);
        gap_since(store);
        let mhz10 = hz / 100_000;
        let (w, r) = (PROBE_KB * 1_000 / w_ms, PROBE_KB * 1_000 / r_ms);
        match (read, deleted, cmp.same) {
            (Ok(()), Ok(()), true) => c.line(format_args!(
                "{}.{}M OK W{w} R{r} KB/S",
                mhz10 / 10,
                mhz10 % 10
            )),
            (Ok(()), _, false) => {
                c.line(format_args!("{}.{}M FAIL MISMATCH", mhz10 / 10, mhz10 % 10))
            }
            (Err(e), _, _) | (Ok(()), Err(e), true) => {
                c.line(format_args!("{}.{}M FAIL", mhz10 / 10, mhz10 % 10));
                c.line(format_args!("  {e:?}"));
            }
        }
        c.line(format_args!("  GAP {gap} US"));
    }
}

/// Spec: "Plan 1 re-measures them on the chip".
fn sizes<D: ChimeraDisplay>(c: &mut Console<D>) {
    use core::mem::size_of;
    c.line(format_args!(
        "SOUND {} POOL {}",
        size_of::<Sound>(),
        size_of::<Pool>()
    ));
    c.line(format_args!(
        "PERF {} UI {}",
        size_of::<Performance>(),
        size_of::<UiState>()
    ));
    c.line(format_args!(
        "AUDIO3 {}",
        size_of::<TripleBuffer<AudioShared>>()
    ));
    c.line(format_args!("AXI FREE {}", AXI_SRAM - AXI_RESIDENT));
    c.line(format_args!(
        "VOICE FREE {}",
        VOICE_RAM_BUDGET - size_of::<Instrument>()
    ));
}
