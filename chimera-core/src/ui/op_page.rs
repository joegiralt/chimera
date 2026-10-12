//! OP1–OP6: an operator's own page, a sub-page of OSC after the twelve
//! group pages (ADR 0130). Cells are WAVE, COARSE, FINE, LEVEL, FDBK and
//! VELOCITY; the band draws the envelope with the wave beside it.

use crate::addr::{BlockRef, Op};
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::params::AlgoOpParams as P;
use crate::dsp::algo::waves::WaveId;
use crate::ui::block_def::{BlockDef, ParamSlot, VizType};
use crate::ui::{draw, glyph, page::PageLayout, theme, viz};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

const fn page(id: u16, op: Op, name: &'static str, short: &'static str) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::OpPage(op),
        params: [
            ParamSlot::param(BlockRef::AlgoOp(op), P::WAVE).with_label("WAVE"),
            ParamSlot::param(BlockRef::AlgoOp(op), P::COARSE).with_label("COARSE"),
            ParamSlot::param(BlockRef::AlgoOp(op), P::FINE).with_label("FINE"),
            ParamSlot::param(BlockRef::AlgoOp(op), P::LEVEL).with_label("LEVEL"),
            ParamSlot::param(BlockRef::AlgoOp(op), P::FEEDBACK).with_label("FDBK"),
            ParamSlot::param(BlockRef::AlgoOp(op), P::VELOCITY).with_label("VELOCITY"),
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

/// The wave's inset at the band's top right: the envelope's peak sits at
/// the left unless the attack is slow.
const INSET_W: i32 = 56;
const INSET_H: i32 = 30;
const INSET_PAD: i32 = 4;

/// The operator's envelope across the band, `wave` traced in a framed
/// inset over its top right.
pub fn draw_band<D>(d: &mut D, rates: EnvRates, wave: WaveId)
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
    const N: usize = (INSET_W - 2 * INSET_PAD) as usize + 1;
    let (x0, y0) = (theme::VIZ_RIGHT - INSET_W, theme::VIZ_BAND_TOP + 2);
    let cy = y0 + INSET_H / 2;
    draw::fill_rect(d, x0, y0, INSET_W, INSET_H, theme::BG);
    draw::round_outline(d, x0, y0, INSET_W, INSET_H, 4, theme::FAINT);
    let trace = glyph::wave_trace::<N>(wave, INSET_H / 2 - INSET_PAD);
    for i in 1..N {
        let px = |i: usize| x0 + INSET_PAD + i as i32;
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

/// What `draw_band` reads, for dirty tracking: AR D1R D2R 5 bits, D1L RR 4,
/// the wave 5.
pub fn key(r: EnvRates, wave: WaveId) -> u32 {
    u32::from(wave.get()) << 23
        | u32::from(r.ar) << 18
        | u32::from(r.d1r) << 13
        | u32::from(r.d2r) << 8
        | u32::from(r.d1l) << 4
        | u32::from(r.rr)
}
