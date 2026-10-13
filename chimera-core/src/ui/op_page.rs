//! OP1–OP6: an operator's own page, a sub-page of OSC after the twelve
//! group pages (ADR 0130). Its cells follow the operator's kind
//! (`page_cells`); the band draws the envelope with the kind inset beside
//! it: a table's wave, a formant's grain at its FC and BW_eff, a noise
//! band's bell at its centre (formant spec § 6).

use crate::addr::Op;
use crate::block::ParamId;
use crate::dsp::algo::engine::FormantView;
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::formant::{Skirt, window};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::params::{AlgoOpParams as P, AlgoParams};
use crate::dsp::algo::waves::WaveId;
use crate::ui::block_def::{BlockDef, ParamSlot, VizType};
use crate::ui::fmt::FmtBuf;
use crate::ui::{draw, glyph, page::PageLayout, theme, viz};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

/// A cell: the operator's param and its label.
pub type Cell = Option<(ParamId, &'static str)>;

/// Full words where they fit a cell's 62 px (Ruling 12): FEEDBACK is 65.
const TABLE: [Cell; 6] = [
    Some((P::WAVE, "WAVE")),
    Some((P::COARSE, "COARSE")),
    Some((P::FINE, "FINE")),
    Some((P::LEVEL, "LEVEL")),
    Some((P::FEEDBACK, "FDBK")),
    Some((P::VELOCITY, "VELOCITY")),
];
const FORMANT: [Cell; 6] = [
    Some((P::WAVE, "WAVE")),
    Some((P::FRMT, "FORMANT")),
    Some((P::BW, "BW")),
    Some((P::SKIRT, "SKIRT")),
    Some((P::KEY, "KEY")),
    Some((P::VSLOT, "SLOT")),
];
const NOISE: [Cell; 6] = [
    Some((P::WAVE, "WAVE")),
    Some((P::FRMT, "CENTRE")),
    Some((P::BW, "WIDTH")),
    Some((P::RES, "RES")),
    Some((P::VSLOT, "SLOT")),
    Some((P::LEVEL, "LEVEL")),
];

/// The OP page's cells for an operator of `kind` (spec § 6): a formant's
/// COARSE, FINE, LEVEL, FDBK and VELOCITY stay on their group pages.
pub const fn page_cells(kind: OpKind) -> [Cell; 6] {
    match kind {
        OpKind::Table(_) => TABLE,
        OpKind::Formant => FORMANT,
        OpKind::Noise => NOISE,
    }
}

/// Cell `k` of a table operator's page: what a slot reads out of context.
pub fn table_cell(k: u8) -> Cell {
    TABLE.get(usize::from(k)).copied().flatten()
}

const fn page(id: u16, op: Op, name: &'static str, short: &'static str) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::OpPage(op),
        params: [
            ParamSlot::op_panel(op, 0),
            ParamSlot::op_panel(op, 1),
            ParamSlot::op_panel(op, 2),
            ParamSlot::op_panel(op, 3),
            ParamSlot::op_panel(op, 4),
            ParamSlot::op_panel(op, 5),
        ],
    }
}

pub static ALGO_OP_PAGES: [BlockDef; 6] = [
    page(100, Op::A, "Operator 1", "OP1"),
    page(101, Op::B, "Operator 2", "OP2"),
    page(102, Op::C, "Operator 3", "OP3"),
    page(103, Op::D, "Operator 4", "OP4"),
    page(104, Op::E, "Operator 5", "OP5"),
    page(105, Op::F, "Operator 6", "OP6"),
];

/// The kind's inset at the band's top right: the envelope's peak sits at
/// the left unless the attack is slow. A formant or noise inset carries
/// its readout to the left of its trace.
const INSET_W: i32 = 56;
const INSET_H: i32 = 30;
const INSET_PAD: i32 = 4;
const READOUT_W: i32 = 56;
/// The trace's points: one a pixel across the inset's inner width.
const N: usize = (INSET_W - 2 * INSET_PAD) as usize + 1;

/// What the inset draws: a table's wave, a formant's grain, a noise band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Inset {
    Wave(WaveId),
    Grain { skirt: Skirt, fc: f32, bw: f32 },
    Band { fc: f32, bw: f32 },
}

impl Inset {
    /// Operator `op` of `algo`, `live` the edited Part's newest voice.
    pub fn of(algo: &AlgoParams, live: &FormantView, op: Op) -> Inset {
        let band = || shown(algo, live, op);
        match algo.kinds().get(op.index()) {
            OpKind::Table(w) => Inset::Wave(w),
            OpKind::Formant => {
                let (fc, bw) = band();
                let skirt = Skirt::from_code(algo.ops[op.index()].skirt);
                Inset::Grain { skirt, fc, bw }
            }
            OpKind::Noise => {
                let (fc, bw) = band();
                Inset::Band { fc, bw }
            }
        }
    }
}

/// What operator `op`'s band shows, `(fc, bw_eff)` Hz: the live voice's
/// while one sounds and has taken the operator's law, else the stored
/// params' at C4 (plan Ruling 8).
pub fn shown(algo: &AlgoParams, live: &FormantView, op: Op) -> (f32, f32) {
    let i = op.index();
    if live.sounding && live.fc[i] > 0.0 {
        return (live.fc[i], live.bw_eff[i]);
    }
    let r = FormantView::resting(algo);
    (r.fc[i], r.bw_eff[i])
}

/// The operator's envelope across the band, `inset` framed over its top
/// right.
pub fn draw_band<D>(d: &mut D, rates: EnvRates, inset: Inset)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (widths, heights) = viz::op_env(rates);
    viz::envelope_from(
        d,
        viz::BAND_PLOT_TOP,
        &widths,
        &heights,
        &viz::OP_ENV_LABELS,
        None,
    );
    let w = match inset {
        Inset::Wave(_) => INSET_W,
        Inset::Grain { .. } | Inset::Band { .. } => INSET_W + READOUT_W,
    };
    let (x0, y0) = (theme::VIZ_RIGHT - w, theme::VIZ_BAND_TOP + 2);
    draw::fill_rect(d, x0, y0, w, INSET_H, theme::BG);
    draw::round_outline(d, x0, y0, w, INSET_H, 4, theme::FAINT);
    let half = INSET_H / 2 - INSET_PAD;
    let cy = y0 + INSET_H / 2;
    let trace: [i32; N] = match inset {
        Inset::Wave(wave) => glyph::wave_trace::<N>(wave, half),
        Inset::Grain { skirt, fc, bw } => {
            readout(d, x0, y0, [("FC", Some(fc)), ("BW", Some(bw))]);
            glyph::resample::<N>(&grain_curve(skirt, grain_cycles(skirt, fc, bw)), half)
        }
        Inset::Band { fc, bw } => {
            readout(d, x0, y0, [("CENTRE", None), ("", Some(fc))]);
            // The bell stands on the inset's floor.
            let b = band_curve(fc, bw);
            core::array::from_fn(|i| {
                let v = i32::from(b[i * 63 / (N - 1)]);
                v * 2 * half / 255 - half
            })
        }
    };
    let px = |i: usize| theme::VIZ_RIGHT - INSET_W + INSET_PAD + i as i32;
    for i in 1..N {
        draw::line(
            d,
            px(i - 1),
            cy - trace[i - 1],
            px(i),
            cy - trace[i],
            theme::ACCENT,
            1,
        );
    }
}

/// Two lines left of the trace: a label and, if any, its Hz.
fn readout<D>(d: &mut D, x0: i32, y0: i32, lines: [(&str, Option<f32>); 2])
where
    D: DrawTarget<Color = Rgb565>,
{
    for (n, (label, hz)) in lines.into_iter().enumerate() {
        let y = y0 + 12 + 12 * n as i32;
        let x = x0 + 2 * INSET_PAD;
        let x = x + draw::text(d, &theme::FONT_LABEL, label, x, y, theme::MID);
        if let Some(hz) = hz {
            let mut buf = FmtBuf::new();
            let _ = hz_short(&mut buf, hz);
            let gap = if label.is_empty() { 0 } else { 4 };
            draw::text(d, &theme::FONT_LABEL, buf.as_str(), x + gap, y, theme::INK2);
        }
    }
}

/// Whole Hz under 1 kHz, then kHz to a tenth, then whole kHz: the
/// readout's room.
fn hz_short(buf: &mut FmtBuf, hz: f32) -> core::fmt::Result {
    let hz = libm::roundf(hz) as u32;
    let tenths = (hz + 50) / 100;
    match hz {
        0..1000 => write!(buf, "{hz}"),
        _ if tenths < 100 => write!(buf, "{}.{}k", tenths / 10, tenths % 10),
        _ => write!(buf, "{}k", (hz + 500) / 1000),
    }
}

/// The carrier's cycles across one grain: FC times the window's length,
/// `K/BW_eff`, held to what the inset can draw.
fn grain_cycles(s: Skirt, fc: f32, bw: f32) -> f32 {
    crate::dsp::clampf(fc * s.k() / bw.max(1.0), 1.0, 8.0)
}

/// What `draw_band` reads, for dirty tracking: AR D1R D2R 5 bits, D1L RR
/// 4, the kind's code 5; a formant's or a band's drawn Hz and SKIRT mixed
/// in.
pub fn key(r: EnvRates, inset: Inset) -> u32 {
    let rates = u32::from(r.ar) << 18
        | u32::from(r.d1r) << 13
        | u32::from(r.d2r) << 8
        | u32::from(r.d1l) << 4
        | u32::from(r.rr);
    let hz = |fc: f32, bw: f32| {
        (libm::roundf(fc) as u32).wrapping_mul(0x9e37_79b1)
            ^ (libm::roundf(bw) as u32).wrapping_mul(0x85eb_ca77)
    };
    let (code, drawn) = match inset {
        Inset::Wave(w) => (w.get(), 0),
        Inset::Grain { skirt, fc, bw } => (OpKind::Formant.code(), hz(fc, bw) ^ skirt as u32),
        Inset::Band { fc, bw } => (OpKind::Noise.code(), hz(fc, bw)),
    };
    (u32::from(code) << 23 | rates) ^ drawn
}

/// One grain at `s`: its window times a sine of `fc_l` cycles across it
/// (FC·L), 64 points to ±127.
pub fn grain_curve(s: Skirt, fc_l: f32) -> [i8; 64] {
    core::array::from_fn(|i| {
        let x = i as f32 / 63.0;
        let y = window(x, s) * libm::sinf(core::f32::consts::TAU * fc_l * x);
        libm::roundf(y * 127.0) as i8
    })
}

/// A noise band at centre `fc` and width `bw` (Hz) over a log axis from
/// 20 Hz to 20 kHz, 64 points to 255: the two-pole band-pass's magnitude,
/// `1/√(1 + Q²(f/fc − fc/f)²)` with `Q = fc/bw`.
pub fn band_curve(fc: f32, bw: f32) -> [u8; 64] {
    let q = fc / bw.max(1.0);
    core::array::from_fn(|i| {
        let f = 20.0 * libm::powf(1000.0, i as f32 / 63.0);
        let x = q * (f / fc - fc / f);
        libm::roundf(255.0 / libm::sqrtf(1.0 + x * x)) as u8
    })
}
