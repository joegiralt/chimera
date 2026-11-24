// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The panel behind the framebuffer: a register-level ILI9341 fed by
//! `chimera_hal::panel`, the protocol `chimera-board`'s `Stm32Display`
//! runs (each command and each data run its own CS frame), in the UI loop's
//! order (`lib.rs`: a full flush at start, then `render_dirty`'s regions,
//! then the panel's health check).
//!
//! An editor session on a real unit (2026-11-22): the glass showed TAPE and VA
//! fragments over a tinted ground and stayed there, while `shot` read a
//! clean SETTINGS. The first test drives that session through `ed part` and
//! `ed set` and finds the stream keeps the glass equal to the framebuffer:
//! the UI and the flush are not where it broke. The second puts one bad
//! bit on one command byte, as the 50 MHz link can, and needs the health
//! check to bring the panel back (ADR 0326).

mod common;
mod screen;

use common::console_fake::{Fake, ask};
use screen::{Fb, H, W, init_part_engine, tap};

use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::panel::{self, Health, Link, Mode};
use chimera_hal::{ButtonId, ChimeraDisplay, Ms};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

use chimera_hal::panel::{COLMOD, MADCTL};

/// ILI9341 (datasheet V1.11 § 8.2) as far as `display.rs` drives it, plus
/// the mode commands a corrupted byte can land on.
struct Panel {
    awake: bool,
    on: bool,
    colmod: u8,
    madctl: u8,
    col: [u8; 4],
    page: [u8; 4],
    cmd: u8,
    args: Vec<u8>,
    x: u16,
    y: u16,
    hi: Option<u8>,
    ram: Vec<u16>,
    /// A fault init can't clear: DISPON is ignored.
    stuck_off: bool,
    /// What the glass shows: RAM while the panel scans, else the last
    /// scanned image, held.
    glass: Vec<u16>,
}

impl Panel {
    /// As `init` leaves it.
    fn new() -> Self {
        Self {
            awake: true,
            on: true,
            colmod: COLMOD,
            madctl: MADCTL,
            col: [0, 0, 0, 0xEF],
            page: [0, 0, 0x01, 0x3F],
            cmd: 0,
            args: Vec::new(),
            x: 0,
            y: 0,
            hi: None,
            ram: vec![0; W * H],
            stuck_off: false,
            glass: vec![0; W * H],
        }
    }

    fn span(a: [u8; 4]) -> (u16, u16) {
        (
            u16::from_be_bytes([a[0], a[1]]),
            u16::from_be_bytes([a[2], a[3]]),
        )
    }

    /// CSX high: the serial interface's byte and pixel phase restart.
    fn cs_high(&mut self) {
        self.hi = None;
    }

    fn command(&mut self, c: u8) {
        self.cmd = c;
        self.args.clear();
        self.hi = None;
        match c {
            0x01 => {
                // SWRESET: sleep in, display off, registers to reset values.
                self.awake = false;
                self.on = false;
                self.colmod = 0x66;
                self.madctl = 0;
            }
            0x10 => self.awake = false,
            0x11 => self.awake = true,
            0x28 => self.on = false,
            0x29 => self.on = !self.stuck_off,
            0x2C => {
                self.x = Self::span(self.col).0;
                self.y = Self::span(self.page).0;
            }
            _ => {}
        }
    }

    fn data(&mut self, b: u8) {
        match self.cmd {
            0x2A | 0x2B => {
                self.args.push(b);
                if self.args.len() == 4 {
                    let a = [self.args[0], self.args[1], self.args[2], self.args[3]];
                    if self.cmd == 0x2A {
                        self.col = a;
                    } else {
                        self.page = a;
                    }
                }
            }
            0x36 if self.args.is_empty() => {
                self.args.push(b);
                self.madctl = b;
            }
            0x3A if self.args.is_empty() => {
                self.args.push(b);
                self.colmod = b;
            }
            0x2C => self.pixel_byte(b),
            _ => {}
        }
    }

    fn pixel_byte(&mut self, b: u8) {
        // Anything but 16 bits a pixel: the bytes land as garbage; model it
        // as each byte a pixel of its own.
        let px = if self.colmod == COLMOD {
            match self.hi.take() {
                None => {
                    self.hi = Some(b);
                    return;
                }
                Some(h) => u16::from_be_bytes([h, b]),
            }
        } else {
            u16::from(b) * 0x0101
        };
        let (c0, c1) = Self::span(self.col);
        let (_, p1) = Self::span(self.page);
        if (self.x as usize) < W && (self.y as usize) < H {
            // MADCTL as `init` sets it maps column n to fb column n; any
            // other value mirrors it.
            let x = if self.madctl == MADCTL {
                self.x as usize
            } else {
                W - 1 - self.x as usize
            };
            self.ram[self.y as usize * W + x] = px;
        }
        if self.x >= c1 {
            self.x = c0;
            self.y = if self.y >= p1 {
                Self::span(self.page).0
            } else {
                self.y + 1
            };
        } else {
            self.x += 1;
        }
    }

    /// RESX low: as SWRESET.
    fn hard_reset(&mut self) {
        self.command(0x01);
        self.cmd = 0;
    }

    /// What RDDPM, RDDMADCTL and RDDCOLMOD answer.
    fn answer(&self, c: u8) -> u8 {
        match c {
            0x0A => 0x80 | u8::from(self.awake) << 4 | 0x08 | u8::from(self.on) << 2,
            0x0B => self.madctl,
            // As a real panel answers (2026-11-23): DBI only, DPI 0.
            0x0C => self.colmod & 0x07,
            _ => 0,
        }
    }

    /// A refresh of the glass.
    fn scan(&mut self) {
        if self.awake && self.on {
            self.glass.copy_from_slice(&self.ram);
        }
    }
}

/// `Stm32Display`: the framebuffer, its link to the panel (which can flip
/// bits of the `n`th command byte), and the health check's state.
struct Unit {
    fb: Fb,
    panel: Panel,
    cmds: usize,
    fault: Option<(usize, u8)>,
    health: Health,
    reinits: usize,
    now: Ms,
    /// MISO dead: every read fails.
    dead_reads: bool,
}

impl Unit {
    fn new() -> Self {
        Self {
            fb: Fb::new(),
            panel: Panel::new(),
            cmds: 0,
            fault: None,
            health: Health::new(),
            reinits: 0,
            now: Ms(0),
            dead_reads: false,
        }
    }
}

impl Link for Unit {
    fn cmd(&mut self, c: u8) {
        let flip = match self.fault {
            Some((n, mask)) if n == self.cmds => mask,
            _ => 0,
        };
        self.cmds += 1;
        self.panel.command(c ^ flip);
        self.panel.cs_high();
    }

    fn data(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.panel.data(b);
        }
        self.panel.cs_high();
    }

    fn read(&mut self, c: u8) -> Option<u8> {
        self.panel.command(c);
        self.panel.cs_high();
        (!self.dead_reads).then(|| self.panel.answer(c))
    }

    fn reset(&mut self, low: bool) {
        if low {
            self.panel.hard_reset();
        }
    }

    fn delay_us(&mut self, _: u32) {}
}

impl OriginDimensions for Unit {
    fn size(&self) -> Size {
        self.fb.size()
    }
}

impl DrawTarget for Unit {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        p: I,
    ) -> Result<(), Self::Error> {
        self.fb.draw_iter(p)
    }
}

impl ChimeraDisplay for Unit {
    /// `Stm32Display::flush_region`: the window, then the pixels (palette
    /// TEAL / BLACK 0: the identity).
    fn flush_region(&mut self, y_start: u16, y_end: u16) {
        panel::window(self, y_start, y_end);
        let px: Vec<u8> = self.fb.px[y_start as usize * W..y_end as usize * W]
            .iter()
            .flat_map(|p| p.to_be_bytes())
            .collect();
        self.data(&px);
    }
    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.fb.px
    }
}

const SCOPE: [f32; SCOPE_LEN] = [0.0; SCOPE_LEN];

/// The UI loop's boot: a full render, a full flush, the regions primed.
fn boot(ui: &mut UiState, d: &mut Unit) {
    ui.update(UiTick::for_test());
    ui.render_with_audio(d, &PerfStats::zero(), None, &SCOPE);
    d.flush();
    ui.prime_regions(&PerfStats::zero(), None, &SCOPE);
    d.panel.scan();
}

/// One UI loop pass from `apply_ed` on: tick, dirty regions, their flush.
fn frame(f: &mut Fake, d: &mut Unit) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    chimera_core::bank2::ed_apply(f.ed.as_mut().unwrap(), &mut f.ui);
    f.ui.update(UiTick::for_test());
    f.ui.step_idle(33, false);
    let regions =
        f.ui.render_dirty_with_audio(d, &PerfStats::zero(), None, &SCOPE);
    for (ys, ye) in regions {
        if ys != ye {
            d.flush_region(ys, ye);
        }
    }
    // The shell's health check: a re-init takes THEME's gamma (not
    // modelled) and a full flush.
    d.now = Ms(d.now.0 + 33);
    let (mut h, now) = (d.health, d.now);
    if panel::tend(&mut h, now, d) {
        d.reinits += 1;
        d.flush();
    }
    d.health = h;
    d.panel.scan();
}

fn part(n: u8) -> PartId {
    PartId::new(n - 1).unwrap()
}

/// The first continuous param `ed describe` lists for `e`'s first page.
fn first_param(f: &mut Fake, e: &str) -> String {
    let text = ask(f, &format!("ed describe {e} 0\n"));
    text.lines()
        .find(|l| l.starts_with("param ") && l.contains(" num "))
        .or_else(|| text.lines().find(|l| l.starts_with("param ")))
        .and_then(|l| l.split(' ').nth(1))
        .unwrap_or_else(|| panic!("a param on {e} 0: {text}"))
        .to_string()
}

/// Parts 1–3 VA, TAPE, ALGO, as on that unit; the editor's session on
/// them: `ed part` between the three, `ed set`s as a knob drags, then MENU
/// on the box. `check` runs after every frame.
fn session(f: &mut Fake, d: &mut Unit, mut check: impl FnMut(&Fake, &Unit, &str)) {
    let engines = [(1, "VA"), (2, "TAPE"), (3, "ALGO")];
    let addrs: Vec<(u8, String)> = engines
        .iter()
        .map(|&(n, e)| {
            assert_eq!(ask(f, &format!("ed part {n}\n")), "OK\n");
            frame(f, d);
            (n, first_param(f, e))
        })
        .collect();
    for round in 0..4 {
        for (n, addr) in &addrs {
            assert_eq!(ask(f, &format!("ed part {n}\n")), "OK\n");
            frame(f, d);
            check(f, d, &format!("round {round}, ed part {n}"));
            for step in 0..12 {
                let v = format!("#{}", (round * 12 + step) % 3);
                let got = ask(f, &format!("ed set {addr} {v}\n"));
                let v = if got.starts_with("ERR") {
                    format!("{}", 0.1 * f64::from(step))
                } else {
                    v
                };
                let _ = ask(f, &format!("ed set {addr} {v}\n"));
                frame(f, d);
                check(f, d, &format!("round {round}, Part {n}, set {addr} {v}"));
            }
        }
    }
    tap(&mut f.ui, ButtonId::Menu);
    for i in 0..30 {
        frame(f, d);
        check(f, d, &format!("SETTINGS frame {i}"));
    }
}

fn rig() -> (Fake, Unit) {
    let mut f = Fake::new();
    init_part_engine(&mut f.ui, part(1), EngineType::Va);
    init_part_engine(&mut f.ui, part(2), EngineType::Tape);
    let mut d = Unit::new();
    boot(&mut f.ui, &mut d);
    (f, d)
}

fn differing(d: &Unit) -> usize {
    d.panel
        .glass
        .iter()
        .zip(&d.fb.px)
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
fn the_editor_session_keeps_the_glass_equal_to_the_framebuffer() {
    let (mut f, mut d) = rig();
    assert_eq!(differing(&d), 0, "after boot");
    session(&mut f, &mut d, |_, d, at| {
        assert_eq!(differing(d), 0, "{at}: the glass left the framebuffer");
    });
    assert!(d.cmds > 100, "the session flushed: {} commands", d.cmds);
}

/// One flipped bit on one command byte, mid-session: CASET (2Ah) read as
/// DISPOFF (28h), COLMOD (3Ah) or MADCTL-adjacent garbage. However long the
/// UI runs after, and through a page change that redraws every row, the
/// glass must come back to the framebuffer: the health check reads the
/// mode back and re-inits (stock's TFT AUTO REINIT, #967).
#[test]
fn one_bad_command_byte_and_the_panel_comes_back() {
    let mut stuck = Vec::new();
    for (what, mask) in [("2Ah as DISPOFF 28h", 0x02), ("2Ah as COLMOD 3Ah", 0x10)] {
        let (mut f, mut d) = rig();
        // A CASET (every flush's first command) a little way in.
        d.fault = Some((d.cmds + 3 * 40, mask));
        session(&mut f, &mut d, |_, _, _| {});
        let off = differing(&d);
        assert!(d.reinits >= 1, "{what}: the check re-inited");
        if off != 0 {
            stuck.push(format!(
                "{what}: {off} of {} pixels never came back (awake {}, on {}, colmod {:02x})",
                W * H,
                d.panel.awake,
                d.panel.on,
                d.panel.colmod
            ));
        }
    }
    assert!(stuck.is_empty(), "{stuck:#?}");
}

#[test]
fn a_clean_session_never_re_inits() {
    let (mut f, mut d) = rig();
    session(&mut f, &mut d, |_, _, _| {});
    assert_eq!(d.reinits, 0);
}

const UP: Mode = Mode::UP;

fn mode(power: u8) -> Option<Mode> {
    Some(Mode { power, ..UP })
}

/// A link that reads nothing (MISO dead: every read fails, or every byte
/// 0) never arms the check, so it never re-inits in a storm.
#[test]
fn a_panel_that_never_answers_is_left_alone() {
    let mut h = Health::new();
    for t in 0..20 {
        assert!(!h.judge(Ms(t * 500), None));
        assert!(!h.judge(
            Ms(t * 500 + 250),
            Some(Mode {
                power: 0,
                madctl: 0,
                colmod: 0
            })
        ));
    }
    assert!(!h.armed());
}

/// Armed, a read that fails counts as off-mode: two in a row re-init.
#[test]
fn a_failed_read_counts_as_off_mode() {
    let mut h = Health::new();
    assert!(!h.judge(Ms(0), Some(UP)));
    assert!(h.armed());
    assert!(!h.judge(Ms(500), None));
    assert!(h.judge(Ms(1000), None));
    // Through the link: MISO goes dead on an armed panel.
    let (mut f, mut d) = rig();
    for _ in 0..30 {
        frame(&mut f, &mut d);
    }
    d.dead_reads = true;
    for _ in 0..60 {
        frame(&mut f, &mut d);
    }
    assert!(d.reinits >= 1, "a dead read re-inits");
}

/// One wrong read is noise; two in a row re-init, and the count restarts.
/// RDDPM 0xDE reads healthy, as stock takes it.
#[test]
fn two_wrong_reads_in_a_row_re_init() {
    let bad = mode(0x98);
    let mut h = Health::new();
    assert!(!h.judge(Ms(0), Some(UP)));
    assert!(!h.judge(Ms(500), bad));
    assert!(!h.judge(Ms(1000), mode(0xDE)));
    assert!(!h.judge(Ms(1500), bad));
    assert!(h.judge(Ms(2000), bad));
    assert!(!h.judge(Ms(3000), bad));
    assert!(h.judge(Ms(4000), bad));
}

/// A noisy first answer neither arms nor is learned as the mode: the
/// check arms on the first healthy read and judges against the known mode.
#[test]
fn a_noisy_first_answer_is_not_learned() {
    let mut h = Health::new();
    let noise = Some(Mode {
        power: 0x9C,
        madctl: 0x00,
        colmod: 0x55,
    });
    assert!(!h.judge(Ms(0), noise));
    assert!(!h.armed());
    assert!(!h.judge(Ms(500), Some(UP)));
    assert!(h.armed());
    assert!(!h.judge(Ms(1000), noise));
    assert!(h.judge(Ms(1500), noise));
}

/// A fault init can't clear backs off: each re-init doubles the wait, to
/// 16 s, so a minute holds the UI in init a handful of times, not 60.
#[test]
fn a_fault_that_survives_init_backs_off() {
    let (mut f, mut d) = rig();
    for _ in 0..30 {
        frame(&mut f, &mut d);
    }
    assert!(d.health.armed());
    d.panel.stuck_off = true;
    d.panel.on = false;
    // A minute of 33 ms frames.
    for _ in 0..(60_000 / 33) {
        frame(&mut f, &mut d);
    }
    // 1, 2, 4, 8, 16, 16 s after two bad reads each: about six.
    assert!(
        (4..=8).contains(&d.reinits),
        "{} re-inits in a minute",
        d.reinits
    );
    // Healthy again, the wait is back to 500 ms.
    d.panel.stuck_off = false;
    let at = d.reinits;
    for _ in 0..(40_000 / 33) {
        frame(&mut f, &mut d);
    }
    assert_eq!(d.reinits, at + 1, "the next re-init clears it");
    let t = d.now;
    let mut h = d.health;
    assert!(!h.judge(t, Some(UP)));
    assert!(!h.due(Ms(t.0 + 499)) && h.due(Ms(t.0 + 500)));
}

/// A real panel (ILI9341, 2026-11-23, `panel_read 9c 48 05`): RDDCOLMOD
/// answers 0x05, the DBI field alone, for the 0x55 sent. It must arm.
#[test]
fn the_real_panels_answer_arms() {
    let real = Some(Mode {
        power: 0x9C,
        madctl: 0x48,
        colmod: 0x05,
    });
    let mut h = Health::new();
    assert!(!h.judge(Ms(0), real));
    assert!(h.armed());
    // An 18-bit DBI (0x06) is off-mode.
    assert!(!h.judge(Ms(500), Some(Mode { colmod: 0x06, ..UP })));
    assert!(h.judge(Ms(1000), Some(Mode { colmod: 0x06, ..UP })));
}

#[test]
fn the_check_is_due_every_half_second() {
    let mut h = Health::new();
    assert!(h.due(Ms(0)));
    h.judge(Ms(0), Some(UP));
    assert!(!h.due(Ms(499)));
    assert!(h.due(Ms(500)));
    // Across the clock's wrap.
    h.judge(Ms(u32::MAX - 100), Some(UP));
    assert!(!h.due(Ms(u32::MAX)));
    assert!(h.due(Ms(400)));
}
