//! Each reverb TYPE's panel as `const` data (reverb TYPEs spec § Pages,
//! the pattern ADR 0037's `filter_panel` set): REV's knobs 1–4 and
//! REV › MORE's six, each slot's word, value text and encoder step under
//! that TYPE. TYPE and MIX sit on REV outside the panel.

use crate::block::{Block, ParamId, find_spec};
use crate::dsp::reverb::{KnobMemory, REVERB_SPECS, ReverbParams, ReverbType, knobs};
use crate::ui::fmt::{FmtBuf, fixed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevPage {
    /// REV's knobs 1–4, between TYPE and MIX.
    Main,
    /// REV › MORE.
    More,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevKnob {
    pub id: ParamId,
    /// The TYPE's word for the slot.
    pub label: &'static str,
    /// What a cell shows when `label` outruns its bar (ADR 0053).
    pub short: Option<&'static str>,
    /// One encoder tick, unsynced.
    pub step: f32,
}

/// REV's knobs 1–4, then REV › MORE's six.
type Panel = [Option<&'static RevKnob>; 10];

const TICK: f32 = 1.0 / 128.0;

const fn k(id: ParamId, label: &'static str) -> RevKnob {
    RevKnob {
        id,
        label,
        short: None,
        step: TICK,
    }
}

use ReverbParams as P;

/// QUAD's SIZE ticks one table step (`ring::SIZE_STEPS`).
static QUAD_SIZE: RevKnob = RevKnob {
    step: 1.0 / 31.0,
    ..k(P::SIZE, "SIZE")
};
static TIME: RevKnob = k(P::TIME, "TIME");
static DAMP: RevKnob = k(P::DAMPING, "DAMP");
static SMEAR: RevKnob = k(P::SMEAR, "SMEAR");
static RATE: RevKnob = k(P::RATE, "RATE");

/// SIZE · TIME · DAMP · SMEAR; MORE: RATE. Each other TYPE's panel lands
/// with its engine (Tasks 8–11).
static QUAD: Panel = [
    Some(&QUAD_SIZE),
    Some(&TIME),
    Some(&DAMP),
    Some(&SMEAR),
    None,
    None,
    None,
    Some(&RATE),
    None,
    None,
];

fn panel(t: ReverbType) -> &'static Panel {
    match t {
        ReverbType::Quad
        | ReverbType::Room
        | ReverbType::Hall
        | ReverbType::Bloom
        | ReverbType::Gate
        | ReverbType::Reverse => &QUAD,
    }
}

/// The TYPEs TYPE walks: each joins with its engine (Tasks 8–11).
pub fn types_on_screen() -> &'static [ReverbType] {
    &[ReverbType::Quad]
}

/// The panel `t` shows: its own once its engine runs, else QUAD's, which
/// is what plays. A TYPE a CC or a newer project set stays stored.
pub fn shown(t: ReverbType) -> ReverbType {
    if types_on_screen().contains(&t) {
        t
    } else {
        ReverbType::Quad
    }
}

/// Cell `k` of `page` under `t`'s panel; `None` where it has none.
pub fn knob(t: ReverbType, page: RevPage, k: u8) -> Option<&'static RevKnob> {
    let (at, n) = match page {
        RevPage::Main => (0, 4),
        RevPage::More => (4, 6),
    };
    if k >= n {
        return None;
    }
    panel(t)[at + k as usize]
}

fn find(t: ReverbType, id: ParamId) -> Option<&'static RevKnob> {
    panel(t).iter().flatten().copied().find(|c| c.id == id)
}

/// Slot `id`'s word under `t`: what any CC map shows (MIDI MAP, #247),
/// never the static spec's where the TYPE names it.
pub fn label(t: ReverbType, id: ParamId) -> &'static str {
    find(t, id)
        .map(|c| c.label)
        .or_else(|| find_spec(&REVERB_SPECS, id).map(|s| s.label))
        .unwrap_or("--")
}

/// One encoder tick of `id` under `p`'s panel.
pub fn step(p: &ReverbParams, id: ParamId) -> f32 {
    find(shown(p.kind), id).map_or(TICK, |c| c.step)
}

/// TYPE `t`, leaving `p`'s knobs in `mem` (spec § A TYPE change loads
/// that TYPE's knobs): a nudge away and back loses nothing.
pub fn retype(p: &ReverbParams, t: ReverbType, mem: &mut KnobMemory) -> ReverbParams {
    mem.store(p);
    p.with_type(t, mem)
}

/// One encoder turn of `id` under `p`: TYPE walks the TYPEs on screen (a
/// TYPE off the list stays put), a knob its `step`.
pub fn turn(p: &ReverbParams, id: ParamId, delta: i8, mem: &mut KnobMemory) -> ReverbParams {
    if id == P::TYPE {
        let list = types_on_screen();
        let Some(i) = list.iter().position(|&t| t == p.kind) else {
            return *p;
        };
        let j = (i as i32 + delta as i32).clamp(0, list.len() as i32 - 1) as usize;
        return if list[j] == p.kind {
            *p
        } else {
            retype(p, list[j], mem)
        };
    }
    let mut q = *p;
    q.set(id, q.get(id) + delta as f32 * step(p, id));
    q
}

/// Slot `id`'s value as QUAD's panel shows it, in units, from the maps the
/// engine plays (`dsp::reverb::knobs`). The other TYPEs' texts land with
/// their engines.
pub fn value_text(id: ParamId, p: &ReverbParams) -> FmtBuf {
    let k = knobs(p);
    let (x, places, unit) = match id {
        P::SIZE => (k.length_ms, 0, "ms"),
        // Seconds: `0.55s`, `2.5s`, `10s`.
        P::TIME => (
            k.rt60_s,
            (k.rt60_s < 9.95) as u32 + (k.rt60_s < 0.995) as u32,
            "s",
        ),
        // A cutoff: `6.1k`, `11k`.
        P::DAMPING => (k.damp_hz / 1000.0, (k.damp_hz < 9_950.0) as u32, "k"),
        // Tenths: hundredths would run into the cube.
        P::SMEAR => (k.smear_ms, 1, "ms"),
        P::RATE => (k.rate_hz, 2, "Hz"),
        _ => (0.0, 0, ""),
    };
    let mut b = FmtBuf::new();
    if id == P::SMEAR && k.smear_ms == 0.0 {
        let _ = core::fmt::Write::write_str(&mut b, "OFF");
    } else if !unit.is_empty() {
        fixed(&mut b, x, places, unit);
    }
    b
}
