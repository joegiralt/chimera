//! Each reverb TYPE's panel as `const` data (reverb TYPEs spec § Pages,
//! the pattern ADR 0037's `filter_panel` set): REV's knobs 1–4 and
//! REV › MORE's six, each slot's word, value text and encoder step under
//! that TYPE. TYPE and MIX sit on REV outside the panel.

use crate::block::{Block, ParamId, find_spec};
use crate::dsp::reverb::{KnobMemory, REVERB_SPECS, ReverbParams, ReverbType, knobs};

/// The SIZE cell's word in HALL's plate zone (ADR 0085: the label, not
/// the value).
pub const PLATE: &str = "PLATE";
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
static LENGTH: RevKnob = k(P::SIZE, "LENGTH");
static TAIL: RevKnob = k(P::TIME, "TAIL");
static REGEN: RevKnob = k(P::TIME, "REGEN");
static TONE: RevKnob = k(P::TONE, "TONE");
static DENSITY: RevKnob = RevKnob {
    short: Some("DENS"),
    ..k(P::DIFFUSE, "DENSITY")
};
static SHAPE: RevKnob = k(P::SHAPE, "SHAPE");
static CURVE: RevKnob = k(P::SHAPE, "CURVE");
static SIZE: RevKnob = k(P::SIZE, "SIZE");
static DECAY: RevKnob = k(P::TIME, "DECAY");
static DIFFUSE: RevKnob = RevKnob {
    short: Some("DIFF"),
    ..k(P::DIFFUSE, "DIFFUSE")
};
static PREDELAY: RevKnob = RevKnob {
    short: Some("PRE"),
    ..k(P::PREDELAY, "PRE-DLY")
};

/// SIZE · TIME · DAMP · SMEAR; MORE: RATE. BLOOM's lands with its engine
/// (Task 11).
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

/// SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · SMEAR · RATE. SYNC
/// joins with tempo sync (Task 12).
static ROOM: Panel = [
    Some(&SIZE),
    Some(&DECAY),
    Some(&TONE),
    Some(&DIFFUSE),
    Some(&PREDELAY),
    None,
    Some(&SMEAR),
    Some(&RATE),
    None,
    None,
];

/// SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · DAMP · SMEAR · RATE.
/// SYNC joins with tempo sync (Task 12).
static HALL: Panel = [
    Some(&SIZE),
    Some(&DECAY),
    Some(&TONE),
    Some(&DIFFUSE),
    Some(&PREDELAY),
    Some(&DAMP),
    Some(&SMEAR),
    Some(&RATE),
    None,
    None,
];

/// LENGTH · TAIL · TONE · DENSITY; MORE: — · SHAPE · SMEAR · RATE.
static GATE: Panel = [
    Some(&LENGTH),
    Some(&TAIL),
    Some(&TONE),
    Some(&DENSITY),
    None,
    Some(&SHAPE),
    Some(&SMEAR),
    Some(&RATE),
    None,
    None,
];

/// LENGTH · REGEN · TONE · DENSITY; MORE: — · CURVE · SMEAR · RATE.
static REVERSE: Panel = [
    Some(&LENGTH),
    Some(&REGEN),
    Some(&TONE),
    Some(&DENSITY),
    None,
    Some(&CURVE),
    Some(&SMEAR),
    Some(&RATE),
    None,
    None,
];

fn panel(t: ReverbType) -> &'static Panel {
    match t {
        ReverbType::Room => &ROOM,
        ReverbType::Hall => &HALL,
        ReverbType::Gate => &GATE,
        ReverbType::Reverse => &REVERSE,
        ReverbType::Quad | ReverbType::Bloom => &QUAD,
    }
}

/// The TYPEs TYPE walks, in screen order: each joins with its engine
/// (BLOOM with Task 11).
pub fn types_on_screen() -> &'static [ReverbType] {
    &[
        ReverbType::Quad,
        ReverbType::Room,
        ReverbType::Hall,
        ReverbType::Gate,
        ReverbType::Reverse,
    ]
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

/// Slot `id`'s value as `p`'s panel shows it, in units, from the maps
/// the engine plays (`dsp::reverb::knobs`).
pub fn value_text(id: ParamId, p: &ReverbParams) -> FmtBuf {
    let k = knobs(p);
    let burst = matches!(p.kind, ReverbType::Gate | ReverbType::Reverse);
    let off = match id {
        P::SMEAR => k.smear_ms == 0.0,
        // TAIL 0 is the fast gate; REGEN 0 none.
        P::TIME if burst => k.tail == 0.0 && k.regen == 0.0,
        _ => false,
    };
    let (x, places, unit) = match id {
        P::SIZE => (k.length_ms, 0, "ms"),
        // Seconds: `0.55s`, `2.5s`, `10s`.
        P::TIME => (
            k.rt60_s,
            (k.rt60_s < 9.95) as u32 + (k.rt60_s < 0.995) as u32,
            "s",
        ),
        // A cutoff: `6.1k`, `11k`.
        P::DAMPING => khz(k.damp_hz),
        P::TONE => khz(k.tone_hz),
        // Taps: `10.2`; else the allpass g, `0.60`.
        P::DIFFUSE if burst => (k.taps, 1, ""),
        P::DIFFUSE => (k.diffuse_g, 2, ""),
        P::PREDELAY => (k.predelay_ms, 0, "ms"),
        // The curve end to end, whole dB: `-4dB`, `24dB` (tenths outrun
        // the focus band).
        P::SHAPE if burst => (k.shape_db, 0, "dB"),
        // Tenths: hundredths would run into the cube.
        P::SMEAR => (k.smear_ms, 1, "ms"),
        P::RATE => (k.rate_hz, 2, "Hz"),
        _ => (0.0, 0, ""),
    };
    let mut b = FmtBuf::new();
    if off {
        let _ = core::fmt::Write::write_str(&mut b, "OFF");
    } else if places > 0 || !unit.is_empty() {
        fixed(&mut b, x, places, unit);
    }
    b
}

/// `hz` as kHz: `6.1k`, `11k`.
fn khz(hz: f32) -> (f32, u32, &'static str) {
    (hz / 1000.0, (hz < 9_950.0) as u32, "k")
}
