// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIXER › PRT's live meters (ADR 0293): beside each Part's LEVEL bar, a
//! bar of what its bus plays after its EQ and before LEVEL, falling 20 dB
//! a second with its peak held a second; and over its number a mark while
//! a key is held on its channel (an MPE zone's members its master's). Bank
//! 2's (`bank2.x`'s `ui::part_meter` group, through `UiDoor`'s `meters`
//! entry); bank 1 keeps the state's home (`PartMeters`) and the stubs.

use crate::MidiChannel;
use crate::dsp::DB_PER_OCTAVE;
use crate::hw::MAX_PARTS;
use crate::mpe::Zones;
use crate::part_peaks::{self, PART_PEAKS, Peak, Watch};
use crate::project::PartId;
use crate::ui::renderer::Parts;
use crate::ui::tape_strip::Paint;
use crate::ui::{theme, viz};

/// A level in dB full scale, as the meter shows it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Dbfs(f32);

impl Dbfs {
    /// The meter's floor: its bar's foot.
    pub const FLOOR: Dbfs = Dbfs(-48.0);
    pub const FULL: Dbfs = Dbfs(0.0);

    /// `p`, no lower than `FLOOR`.
    pub fn of(p: Peak) -> Dbfs {
        let v = p.get();
        if v <= 0.0 {
            return Self::FLOOR;
        }
        // 20·log10(v) as 20·log10(2)·log2(v): libm's log2f is bank 1's
        // already, log10f would be new there.
        Dbfs((DB_PER_OCTAVE * libm::log2f(v)).max(Self::FLOOR.0))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// `ms` of the fall from here, no lower than `FLOOR`.
    fn fallen(self, ms: Ms) -> Dbfs {
        Dbfs((self.0 - FALL.over(ms)).max(Self::FLOOR.0))
    }

    /// The bar's height in pixels, 0 at `FLOOR`, `STRIP_H` at `FULL` and over.
    pub fn px(self) -> u8 {
        let t = (self.0 - Self::FLOOR.0) / (Self::FULL.0 - Self::FLOOR.0);
        libm::roundf(crate::dsp::clampf(t, 0.0, 1.0) * viz::STRIP_H as f32) as u8
    }
}

/// Milliseconds, as a frame or a hold counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ms(pub u16);

/// A fall rate in dB a second.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DbPerSec(f32);

impl DbPerSec {
    pub const fn get(self) -> f32 {
        self.0
    }

    /// The dB it falls in `ms`.
    fn over(self, ms: Ms) -> f32 {
        self.0 * f32::from(ms.0) / 1000.0
    }
}

/// The bar's fall (the design's 20 dB/s).
pub const FALL: DbPerSec = DbPerSec(20.0);
/// How long the peak mark holds before it falls.
pub const HOLD: Ms = Ms(1000);

/// Whether a frame's reading passed full scale: the bar's top in ALERT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    Over,
    Clear,
}

impl Over {
    fn of(p: Peak) -> Over {
        if p.get() > 1.0 {
            Over::Over
        } else {
            Over::Clear
        }
    }
}

/// Whether a key is down on the Part's channel (or its MPE zone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyMark {
    Held,
    Up,
}

impl KeyMark {
    fn of(held: bool) -> KeyMark {
        if held { KeyMark::Held } else { KeyMark::Up }
    }
}

/// One Part's meter: the bar, its held peak and how long that has held,
/// whether it passed full scale, and whether a key is down on its channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    bar: Dbfs,
    peak: Dbfs,
    held_for: Ms,
    over: Over,
    key: KeyMark,
}

impl Meter {
    pub const REST: Meter = Meter {
        bar: Dbfs::FLOOR,
        peak: Dbfs::FLOOR,
        held_for: Ms(0),
        over: Over::Clear,
        key: KeyMark::Up,
    };

    /// `dt` on, then this frame's reading `p`.
    pub fn step(&mut self, p: Peak, dt: Ms) {
        let now = Dbfs::of(p);
        self.bar = if now >= self.bar {
            now
        } else {
            self.bar.fallen(dt)
        };
        self.held_for = Ms(self.held_for.0.saturating_add(dt.0));
        if now >= self.peak {
            (self.peak, self.held_for) = (now, Ms(0));
        } else if self.held_for > HOLD {
            let fallen = self.peak.fallen(dt);
            self.peak = if fallen > self.bar { fallen } else { self.bar };
        }
        self.over = Over::of(p);
    }

    pub fn bar(&self) -> Dbfs {
        self.bar
    }

    pub fn peak(&self) -> Dbfs {
        self.peak
    }

    pub fn over(&self) -> Over {
        self.over
    }

    pub fn key(&self) -> KeyMark {
        self.key
    }
}

/// Every Part's meter, and the band's redraw key for them (`key`, kept
/// by each frame so bank 1 reads it without a door call): the UI's state
/// (`UiState::meters`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartMeters {
    meters: [Meter; MAX_PARTS],
    key: u32,
}

impl PartMeters {
    pub const REST: PartMeters = PartMeters {
        meters: [Meter::REST; MAX_PARTS],
        key: 0,
    };

    pub fn get(&self, p: PartId) -> &Meter {
        &self.meters[p.index()]
    }

    /// What the band redraws on: 0 at rest.
    #[inline(always)]
    pub fn key(&self) -> u32 {
        self.key
    }
}

/// What bank 1 asks through `UiDoor`'s one `meters` entry.
pub enum Ask<'a> {
    /// `frame`.
    Frame(&'a mut PartMeters, Shown<'a>),
    /// `paint`.
    Paint(&'a mut dyn Paint, &'a PartMeters),
    /// SMART EQ's live cuts for the mixer's bands (`ui::smart_eq`, ADR
    /// 0296), as a plan bank 1 draws.
    Cuts(
        &'a crate::ui::smart_view::Shown,
        &'a mut crate::ui::smart_view::Plan,
    ),
}

/// Whether MIXER › PRT is up, with what its key marks read.
pub enum Shown<'a> {
    Off,
    On {
        parts: &'a Parts,
        zones: Zones,
        dt: Ms,
    },
}

/// `UiDoor`'s entry.
pub(crate) fn ask_in(a: Ask<'_>) {
    match a {
        Ask::Frame(m, s) => frame(m, s),
        Ask::Paint(on, m) => paint(on, m),
        Ask::Cuts(s, plan) => crate::ui::smart_eq::plan_in(s, plan),
    }
}

/// A frame: the audio told whether to measure; up, each Part's reading
/// taken and its key mark read; down, every meter at rest and every
/// reading dropped (a block in flight as the page closed included), so
/// the page opens on fresh ones.
pub fn frame(m: &mut PartMeters, s: Shown<'_>) {
    match s {
        Shown::Off => {
            part_peaks::watch(Watch::Down);
            for meter in &PART_PEAKS {
                meter.take();
            }
            *m = PartMeters::REST;
        }
        Shown::On { parts, zones, dt } => {
            part_peaks::watch(Watch::Up);
            for (&p, meter) in PartId::ALL.iter().zip(m.meters.iter_mut()) {
                meter.step(part_peaks::of(p).take(), dt);
                meter.key = KeyMark::of(held_on(parts[p.index()].mix.channel, zones));
            }
            m.key = key(&m.meters);
        }
    }
}

/// A key is down on `ch`, or on a member channel of the MPE zone whose
/// master `ch` is.
pub fn held_on(ch: MidiChannel, zones: Zones) -> bool {
    (0..16)
        .map(MidiChannel::clamped)
        .any(|c| zones.master_of(c) == ch && crate::played::channel_held(c.get()))
}

/// The bar's width, and its gap from the LEVEL bar's right edge.
pub const BAR_W: i32 = 3;
pub const BAR_GAP: i32 = 2;
/// The LEVEL bar's width (`viz::parts_overview`).
const LEVEL_W: i32 = 8;

/// Part `p`'s meter's left edge.
pub fn bar_x(p: PartId) -> i32 {
    viz::strip_x(p.index()) + LEVEL_W + BAR_GAP
}

/// Each Part's bar beside its LEVEL, its peak a 1 px mark over it (the
/// bar's top in ALERT past full scale), and its key mark: a 3 px square
/// right of its number.
pub fn paint(on: &mut dyn Paint, m: &PartMeters) {
    let foot = viz::STRIP_TOP + viz::STRIP_H;
    for (&p, meter) in PartId::ALL.iter().zip(&m.meters) {
        let x = bar_x(p);
        let h = i32::from(meter.bar.px());
        if h > 0 {
            on.rect(x, foot - h, BAR_W, h, theme::INK2);
            if meter.over == Over::Over {
                on.rect(x, viz::STRIP_TOP, BAR_W, 2, theme::ALERT);
            }
        }
        let peak = i32::from(meter.peak.px());
        if peak > h {
            on.rect(x, foot - peak, BAR_W, 1, theme::INK);
        }
        if meter.key == KeyMark::Held {
            on.rect(x, viz::STRIP_NUM_Y - 6, 3, 3, theme::ACCENT);
        }
    }
}

/// What the band redraws on: every bar's and peak's pixels, the overs and
/// the key marks.
fn key(m: &[Meter; MAX_PARTS]) -> u32 {
    m.iter().fold(0x811c_9dc5u32, |h, meter| {
        let w = u32::from(meter.bar.px())
            | u32::from(meter.peak.px()) << 8
            | u32::from(meter.over == Over::Over) << 16
            | u32::from(meter.key == KeyMark::Held) << 17;
        (h ^ w).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_is_the_top_and_the_floor_the_foot() {
        assert_eq!(Dbfs::of(Peak::new(1.0)).px(), viz::STRIP_H as u8);
        assert_eq!(Dbfs::of(Peak::new(4.0)).px(), viz::STRIP_H as u8);
        assert_eq!(Dbfs::of(Peak::ZERO).px(), 0);
        assert_eq!(
            Dbfs::of(Peak::new(0.001)).px(),
            0,
            "−60 dB is under the floor"
        );
        assert_eq!(Dbfs::of(Peak::new(0.063)).px(), 19, "−24 dB, half way");
    }

    #[test]
    fn the_bar_falls_20_db_a_second_and_the_peak_holds_one() {
        let mut m = Meter::REST;
        m.step(Peak::new(1.0), Ms(50));
        assert_eq!((m.bar.get(), m.peak.get()), (0.0, 0.0));
        m.step(Peak::ZERO, Ms(500));
        assert!((m.bar.get() + 10.0).abs() < 1e-4, "{:?}", m.bar);
        assert_eq!(m.peak.get(), 0.0, "held");
        m.step(Peak::ZERO, Ms(500));
        assert!((m.bar.get() + 20.0).abs() < 1e-4);
        assert_eq!(m.peak.get(), 0.0, "held its second");
        m.step(Peak::ZERO, Ms(100));
        assert!(
            m.peak.get() < 0.0 && m.peak >= m.bar,
            "falls after, never under the bar"
        );
        m.step(Peak::new(2.0), Ms(50));
        assert_eq!(m.over, Over::Over);
        m.step(Peak::new(0.5), Ms(50));
        assert_eq!(m.over, Over::Clear);
    }

    #[test]
    fn the_bars_sit_beside_the_level_bars() {
        for p in PartId::ALL {
            assert!(
                bar_x(p) + BAR_W < viz::strip_x(p.index() + 1) - 6,
                "clear of the next pan line"
            );
        }
    }
}
