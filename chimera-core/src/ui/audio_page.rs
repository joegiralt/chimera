use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::hw::AUDIO_BUDGET_PERCENT;
use crate::perf::load::AudioStats;
use crate::ui::block_def::BlockDef;
use crate::ui::fmt::FmtBuf;
use crate::ui::{components, draw, theme};

pub const NONE: &str = "--";
const TEXT_Y: i32 = 138;
const METER_Y: i32 = 152;
const METER_H: i32 = 4;

// Keeps every counter within its cell's width: 4294967295 reads 4294M.
fn fmt_count(b: &mut FmtBuf, n: u32) {
    let _ = match n {
        0..10_000 => write!(b, "{n}"),
        10_000..10_000_000 => write!(b, "{}K", n / 1_000),
        _ => write!(b, "{}M", n / 1_000_000),
    };
}

pub fn cell_texts(s: Option<&AudioStats>) -> [FmtBuf; 6] {
    core::array::from_fn(|i| {
        let mut b = FmtBuf::new();
        let Some(s) = s else {
            let _ = b.write_str(NONE);
            return b;
        };
        match i {
            0 => {
                let _ = write!(b, "{}%", s.load_avg);
            }
            1 => {
                let _ = write!(b, "{}%", s.load_peak);
            }
            2 => fmt_count(&mut b, s.overruns),
            3 => {
                for (k, &d) in s.drops[..s.sources as usize].iter().enumerate() {
                    if k > 0 {
                        let _ = b.write_str("/");
                    }
                    fmt_count(&mut b, d);
                }
            }
            4 => fmt_count(&mut b, s.desyncs),
            _ => {
                let _ = write!(b, "{}K", s.stack_used.div_ceil(1024));
            }
        }
        b
    })
}

pub fn draw_cells<D>(d: &mut D, def: &BlockDef, s: Option<&AudioStats>, top: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, text) in cell_texts(s).iter().enumerate() {
        let slot = &def.params[i];
        let cell = components::Cell {
            label: slot.label(),
            text: text.as_str(),
            value: 0.0,
            fmt: slot.format(),
            active: false,
            mod_amount: None,
        };
        components::cell(d, i, top, Some(&cell));
    }
}

pub fn draw_focus<D>(d: &mut D, def: &BlockDef, s: Option<&AudioStats>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let texts = cell_texts(s);
    let value = s.map_or(0.0, |s| s.load_avg.min(100) as f32 / 100.0);
    components::focus_band(
        d,
        def.params[0].label(),
        texts[0].as_str(),
        value,
        false,
        None,
    );
}

pub fn draw_viz<D>(d: &mut D, s: Option<&AudioStats>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line = FmtBuf::new();
    let _ = match s {
        Some(s) => write!(line, "REV {}   {} MHZ", s.rev.label(), s.cpu_hz / 1_000_000),
        None => write!(line, "REV {NONE}   {NONE} MHZ"),
    };
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        line.as_str(),
        theme::MARGIN_X,
        TEXT_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    );
    let (x0, w) = (theme::VIZ_LEFT, theme::VIZ_RIGHT - theme::VIZ_LEFT);
    let at = |pct: u16| x0 + (w as u32 * pct.min(100) as u32 / 100) as i32;
    draw::fill_rect(d, x0, METER_Y, w, METER_H, theme::FAINT);
    if let Some(s) = s {
        let fill = match s.load_avg as u32 {
            100.. => theme::ALERT,
            p if p > AUDIO_BUDGET_PERCENT => theme::WARN,
            _ => theme::ACCENT,
        };
        draw::fill_rect(d, x0, METER_Y, at(s.load_avg) - x0, METER_H, fill);
        draw::fill_rect(
            d,
            at(s.load_peak).min(theme::VIZ_RIGHT - 1),
            METER_Y - 3,
            1,
            METER_H + 6,
            theme::INK2,
        );
    }
    let budget_x = at(AUDIO_BUDGET_PERCENT as u16);
    draw::fill_rect(d, budget_x, METER_Y - 6, 1, METER_H + 12, theme::MID);
    line.clear();
    let _ = write!(line, "{AUDIO_BUDGET_PERCENT}%");
    draw::text_center(
        d,
        &theme::FONT_LABEL,
        line.as_str(),
        budget_x,
        METER_Y + METER_H + 16,
        theme::MID,
        0,
    );
}

fn fold(v: u32) -> u16 {
    (v ^ (v >> 16)) as u16
}

pub fn cells_key(s: Option<&AudioStats>) -> [u16; 6] {
    match s {
        None => [u16::MAX - 1; 6],
        Some(s) => [
            s.load_avg,
            s.load_peak,
            fold(s.overruns),
            fold(s.drops.iter().fold(0u32, |a, &d| a.rotate_left(7) ^ d)),
            fold(s.desyncs),
            fold(s.stack_used.div_ceil(1024)),
        ],
    }
}

pub fn focus_key(s: Option<&AudioStats>) -> u16 {
    s.map_or(u16::MAX - 1, |s| s.load_avg)
}

pub fn viz_key(s: Option<&AudioStats>) -> u32 {
    s.map_or(u32::MAX - 1, |s| {
        [
            s.rev.label().as_bytes()[0] as u32,
            s.cpu_hz / 1_000_000,
            s.load_avg as u32,
            s.load_peak as u32,
        ]
        .iter()
        .fold(0x811c_9dc5u32, |h, &v| (h ^ v).wrapping_mul(0x0100_0193))
    })
}
