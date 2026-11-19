//! MIXER › PRT's live meters (ADR 0293): beside each Part's LEVEL bar, a
//! bar of what its bus plays after its EQ and before LEVEL, falling 20 dB
//! a second with its peak held a second; and over its number a mark while
//! a key is held on its channel (an MPE zone's members its master's). Bank
//! 2's (`bank2.x`'s `ui::part_meter` group, through `UiDoor`'s `meters`
//! entry); bank 1 keeps the state's home (`PartMeters`) and the stubs.

use crate::MidiChannel;
use crate::hw::MAX_PARTS;
use crate::mpe::Zones;
use crate::part_peaks::{PART_PEAKS, Peak};
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
        Dbfs((self.0 - FALL_DB_PER_S * f32::from(ms.0) / 1000.0).max(Self::FLOOR.0))
    }

    /// The bar's height in pixels, 0 at `FLOOR`, `STRIP_H` at `FULL` and over.
    pub fn px(self) -> u8 {
        let t = (self.0 - Self::FLOOR.0) / (Self::FULL.0 - Self::FLOOR.0);
        libm::roundf(crate::dsp::clampf(t, 0.0, 1.0) * viz::STRIP_H as f32) as u8
    }
}

/// dB a doubling: 20·log10(2).
const DB_PER_OCTAVE: f32 = 6.020_6;

/// Milliseconds, as a frame or a hold counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ms(pub u16);

/// The bar's fall (the design's 20 dB/s).
pub const FALL_DB_PER_S: f32 = 20.0;
/// How long the peak mark holds before it falls.
pub const HOLD: Ms = Ms(1000);

/// One Part's meter: the bar, its held peak and how long that has held,
/// and whether a key is down on its channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    bar: Dbfs,
    peak: Dbfs,
    held_for: Ms,
    /// Its peak this frame passed full scale: the bar's top in ALERT.
    over: bool,
    key: bool,
}

impl Meter {
    pub const REST: Meter = Meter {
        bar: Dbfs::FLOOR,
        peak: Dbfs::FLOOR,
        held_for: Ms(0),
        over: false,
        key: false,
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
        self.over = p.get() > 1.0;
    }

    pub fn bar(&self) -> Dbfs {
        self.bar
    }

    pub fn peak(&self) -> Dbfs {
        self.peak
    }

    pub fn key(&self) -> bool {
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

    pub fn get(&self, p: usize) -> &Meter {
        &self.meters[p]
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
    }
}

/// A frame: the audio told whether to measure; up, each Part's reading
/// taken and its key mark read; down, every meter at rest, so the page
/// opens on fresh ones.
pub fn frame(m: &mut PartMeters, s: Shown<'_>) {
    match s {
        Shown::Off => {
            crate::part_peaks::watch(false);
            *m = PartMeters::REST;
        }
        Shown::On { parts, zones, dt } => {
            crate::part_peaks::watch(true);
            for (p, meter) in m.meters.iter_mut().enumerate() {
                meter.step(PART_PEAKS[p].take(), dt);
                meter.key = held_on(parts[p].mix.channel, zones);
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

/// Part `i`'s meter's left edge.
pub fn bar_x(i: usize) -> i32 {
    viz::strip_x(i) + LEVEL_W + BAR_GAP
}

/// Each Part's bar beside its LEVEL, its peak a 1 px mark over it (the
/// bar's top in ALERT past full scale), and its key mark: a 3 px square
/// right of its number.
pub fn paint(on: &mut dyn Paint, m: &PartMeters) {
    let foot = viz::STRIP_TOP + viz::STRIP_H;
    for (i, meter) in m.meters.iter().enumerate() {
        let x = bar_x(i);
        let h = i32::from(meter.bar.px());
        if h > 0 {
            on.rect(x, foot - h, BAR_W, h, theme::INK2);
            if meter.over {
                on.rect(x, viz::STRIP_TOP, BAR_W, 2, theme::ALERT);
            }
        }
        let peak = i32::from(meter.peak.px());
        if peak > h {
            on.rect(x, foot - peak, BAR_W, 1, theme::INK);
        }
        if meter.key {
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
            | u32::from(meter.over) << 16
            | u32::from(meter.key) << 17;
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
        assert!(m.over);
        m.step(Peak::new(0.5), Ms(50));
        assert!(!m.over);
    }

    #[test]
    fn the_bars_sit_beside_the_level_bars() {
        for i in 0..MAX_PARTS {
            assert!(
                bar_x(i) + BAR_W < viz::strip_x(i + 1) - 6,
                "clear of the next pan line"
            );
        }
    }
}
