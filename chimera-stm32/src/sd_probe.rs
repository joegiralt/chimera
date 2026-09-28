//! On-unit SD bring-up (`--features sd-probe`): clocks, acquire in each SPI
//! mode, the volume, a root listing, a 16 KB write/read/delete at two
//! clocks, and the RAM sizes plan 1 re-measures. Then halts, LED on.

use core::fmt::Write as _;
use core::ops::ControlFlow;

use chimera_core::hw::{AXI_SRAM, VOICE_RAM_BUDGET};
use chimera_core::instrument::{AXI_RESIDENT, AudioShared, Instrument};
use chimera_core::preset::{Performance, Sound, SoundPool};
use chimera_core::triple::TripleBuffer;
use chimera_core::ui::UiState;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::{draw, theme};
use chimera_fat::volume::{PartitionType, boot_sector, first_partition};
use chimera_hal::ChimeraDisplay;
use cortex_m::peripheral::DWT;
use embedded_sdmmc::{
    Block, BlockDevice, BlockIdx, Mode, SdCardError, TimeSource, Timestamp, VolumeIdx,
};
use stm32h7xx_hal::spi;
use stm32h7xx_hal::time::Hertz;

use crate::clocks::{self, Clocks};
use crate::sd::{SD_ACQUIRE_MS, SD_FAST_HZ, SD_IDLE_MS, SD_INIT_HZ, SdDevice};

const LINE_H: i32 = 10;
/// Long enough to photograph a full page before the next one clears it.
const PAGE_HOLD_S: u32 = 15;
const LIST_MAX: usize = 8;
const PROBE_FILE: &str = "CHIMPROB.TXT";
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

/// No RTC yet: every probe file carries this date.
struct Fixed;

impl TimeSource for Fixed {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 8,
            zero_indexed_day: 28,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

fn ms_since(cpu_hz: u32, start: u32) -> u32 {
    (u64::from(DWT::cycle_count().wrapping_sub(start)) * 1_000 / u64::from(cpu_hz)) as u32
}

fn pattern(block: u32, buf: &mut [u8; 512]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (block as usize * 7 + i) as u8;
    }
}

pub fn run(display: &mut impl ChimeraDisplay, clk: Clocks, sd: SdDevice) -> ! {
    let cpu_hz = clk.cpu_hz;
    let mut c = Console {
        display,
        cpu_hz,
        y: LINE_H,
        buf: FmtBuf::new(),
    };
    c.clear();

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

    if acquire(&mut c, &sd) {
        card(&mut c, sd);
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

fn card<D: ChimeraDisplay>(c: &mut Console<D>, sd: SdDevice) {
    sd.spi(|s| {
        s.set_hz(Hertz::from_raw(SD_FAST_HZ));
        s.arm(SD_IDLE_MS);
    });
    let mut blk = [Block::new()];
    if let Err(e) = sd.read(&mut blk, BlockIdx(0)) {
        c.line(format_args!("MBR {e:?}"));
        return;
    }
    match first_partition(&blk[0].contents) {
        Ok(p) => {
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
                    }
                    Err(e) => c.line(format_args!("BOOT {e:?}")),
                },
                Err(e) => c.line(format_args!("BOOT {e:?}")),
            }
        }
        Err(e) => c.line(format_args!("MBR {e:?}")),
    }

    let vm: embedded_sdmmc::VolumeManager<_, _> = embedded_sdmmc::VolumeManager::new(sd, Fixed);
    vm.device(|d| d.spi(|s| s.arm(SD_IDLE_MS)));
    let t = DWT::cycle_count();
    let vol = match vm.open_volume(VolumeIdx(0)) {
        Ok(v) => v,
        Err(e) => {
            c.line(format_args!("MOUNT {e:?}"));
            return;
        }
    };
    c.line(format_args!("MOUNT {} MS", ms_since(c.cpu_hz, t)));
    let root = match vol.open_root_dir() {
        Ok(r) => r,
        Err(e) => {
            c.line(format_args!("ROOT {e:?}"));
            return;
        }
    };
    // Listed first, drawn after: no flush inside the directory walk.
    let mut names: [FmtBuf; LIST_MAX] = core::array::from_fn(|_| FmtBuf::new());
    let mut n = 0;
    let listed = root.iterate_dir(|e| {
        let _ = write!(names[n], "  {} {}", e.name, e.size);
        n += 1;
        if n < LIST_MAX {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    });
    for name in &names[..n] {
        c.line(format_args!("{}", name.as_str()));
    }
    if let Err(e) = listed {
        c.line(format_args!("LIST {e:?}"));
    }

    for hz in TEST_HZ {
        vm.device(|d| {
            d.spi(|s| {
                s.set_hz(Hertz::from_raw(hz));
                s.arm(SD_IDLE_MS);
            })
        });
        let mut buf = [0u8; 512];
        let mut expect = [0u8; 512];
        let t = DWT::cycle_count();
        let wrote = root
            .open_file_in_dir(PROBE_FILE, Mode::ReadWriteCreateOrTruncate)
            .and_then(|f| {
                for b in 0..PROBE_BLOCKS {
                    pattern(b, &mut buf);
                    f.write(&buf)?;
                }
                f.close()
            });
        let w_ms = ms_since(c.cpu_hz, t).max(1);
        let t = DWT::cycle_count();
        let read = wrote.and_then(|()| {
            let f = root.open_file_in_dir(PROBE_FILE, Mode::ReadOnly)?;
            let mut same = true;
            for b in 0..PROBE_BLOCKS {
                pattern(b, &mut expect);
                let got = f.read(&mut buf)?;
                same &= got == buf.len() && buf == expect;
            }
            f.close()?;
            Ok(same)
        });
        let r_ms = ms_since(c.cpu_hz, t).max(1);
        let deleted = root.delete_entry_in_dir(PROBE_FILE);
        let gap = vm.device(|d| d.spi(|s| s.max_gap_us()));
        let mhz10 = hz / 100_000;
        let (w, r) = (PROBE_KB * 1_000 / w_ms, PROBE_KB * 1_000 / r_ms);
        match (read, deleted) {
            (Ok(true), Ok(())) => c.line(format_args!(
                "{}.{}M OK W{w} R{r} KB/S",
                mhz10 / 10,
                mhz10 % 10
            )),
            (Ok(false), _) => c.line(format_args!("{}.{}M FAIL MISMATCH", mhz10 / 10, mhz10 % 10)),
            (Err(e), _) | (Ok(true), Err(e)) => {
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
        size_of::<SoundPool>()
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
