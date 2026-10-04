//! Each reverb TYPE's panel as `const` data (reverb TYPEs spec § Pages,
//! the pattern ADR 0037's `filter_panel` set): REV's knobs 1–4 and
//! REV › MORE's six, each slot's word, value text and encoder step under
//! that TYPE. TYPE and MIX sit on REV outside the panel.

use core::fmt::Write;

use crate::block::{Block, ParamId, find_spec};
use crate::dsp::clampf;
use crate::dsp::rev::sync::{DECAY_BEATS, FoldState, Tempo, list_index};
use crate::dsp::reverb::{
    KnobMemory, PLATE_ZONE, REVERB_SPECS, RevSync, ReverbParams, ReverbType, damp_hz,
    decay_in_beats, knobs, synced_list,
};
use crate::ui::fmt::FmtBuf;

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

const fn short(id: ParamId, label: &'static str, short: &'static str) -> RevKnob {
    RevKnob {
        id,
        label,
        short: Some(short),
        step: TICK,
    }
}

use ReverbParams as P;

// Each word once; the panels share them.
/// QUAD's SIZE ticks one table step (`ring::SIZE_STEPS`).
static QUAD_SIZE: RevKnob = RevKnob {
    step: 1.0 / 31.0,
    ..k(P::SIZE, "SIZE")
};
static SIZE: RevKnob = k(P::SIZE, "SIZE");
static SWELL: RevKnob = k(P::SIZE, "SWELL");
static LENGTH: RevKnob = k(P::SIZE, "LENGTH");
static TIME: RevKnob = k(P::TIME, "TIME");
static DECAY: RevKnob = k(P::TIME, "DECAY");
static TAIL: RevKnob = k(P::TIME, "TAIL");
static REGEN: RevKnob = k(P::TIME, "REGEN");
static DAMP: RevKnob = k(P::DAMPING, "DAMP");
static TONE: RevKnob = k(P::TONE, "TONE");
static DIFFUSE: RevKnob = short(P::DIFFUSE, "DIFFUSE", "DIFF");
static DENSITY: RevKnob = short(P::DIFFUSE, "DENSITY", "DENS");
static PRE: RevKnob = short(P::PREDELAY, "PRE-DLY", "PRE");
static SHAPE: RevKnob = k(P::SHAPE, "SHAPE");
static CURVE: RevKnob = k(P::SHAPE, "CURVE");
static SMEAR: RevKnob = k(P::SMEAR, "SMEAR");
static RATE: RevKnob = k(P::RATE, "RATE");
static SYNC: RevKnob = k(P::SYNC, "SYNC");

const fn row(main: [&'static RevKnob; 4], more: [Option<&'static RevKnob>; 6]) -> Panel {
    [
        Some(main[0]),
        Some(main[1]),
        Some(main[2]),
        Some(main[3]),
        more[0],
        more[1],
        more[2],
        more[3],
        more[4],
        more[5],
    ]
}

/// SIZE · TIME · DAMP · SMEAR; MORE: RATE.
static QUAD: Panel = row(
    [&QUAD_SIZE, &TIME, &DAMP, &SMEAR],
    [None, None, None, Some(&RATE), None, None],
);
static ROOM: Panel = row(
    [&SIZE, &DECAY, &TONE, &DIFFUSE],
    [
        Some(&PRE),
        None,
        Some(&SMEAR),
        Some(&RATE),
        Some(&SYNC),
        None,
    ],
);
static HALL: Panel = row(
    [&SIZE, &DECAY, &TONE, &DIFFUSE],
    [
        Some(&PRE),
        Some(&DAMP),
        Some(&SMEAR),
        Some(&RATE),
        Some(&SYNC),
        None,
    ],
);
static BLOOM: Panel = row(
    [&SWELL, &DECAY, &TONE, &DIFFUSE],
    [
        Some(&PRE),
        None,
        Some(&SMEAR),
        Some(&RATE),
        Some(&SYNC),
        None,
    ],
);
static GATE: Panel = row(
    [&LENGTH, &TAIL, &TONE, &DENSITY],
    [
        None,
        Some(&SHAPE),
        Some(&SMEAR),
        Some(&RATE),
        Some(&SYNC),
        None,
    ],
);
static REVERSE: Panel = row(
    [&LENGTH, &REGEN, &TONE, &DENSITY],
    [
        None,
        Some(&CURVE),
        Some(&SMEAR),
        Some(&RATE),
        Some(&SYNC),
        None,
    ],
);

fn panel(t: ReverbType) -> &'static Panel {
    match t {
        ReverbType::Quad => &QUAD,
        ReverbType::Room => &ROOM,
        ReverbType::Hall => &HALL,
        ReverbType::Bloom => &BLOOM,
        ReverbType::Gate => &GATE,
        ReverbType::Reverse => &REVERSE,
    }
}

/// The TYPEs TYPE walks: each joins with its engine (Tasks 8–11).
pub fn types_on_screen() -> &'static [ReverbType] {
    &[ReverbType::Quad]
}

/// Cell `k` of `page` under `t`; `None` where the TYPE has none.
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

/// The short form of a panel word.
pub fn short_of(label: &str) -> Option<&'static str> {
    [&DIFFUSE, &DENSITY, &PRE]
        .into_iter()
        .find(|c| c.label == label)
        .and_then(|c| c.short)
}

/// One encoder tick of `id` under `p`: one division where it is synced,
/// one beat count for DECAY under ALL, else the panel's.
#[inline(never)]
pub fn step(p: &ReverbParams, id: ParamId) -> f32 {
    let of = |n: usize| 1.0 / n.saturating_sub(1).max(1) as f32;
    if let Some(list) = synced_list(p, id) {
        of(list.len())
    } else if id == P::TIME && decay_in_beats(p) {
        of(DECAY_BEATS.len())
    } else {
        find(p.kind, id).map_or(TICK, |c| c.step)
    }
}

/// `step` for a reverb seen as a `Block`.
pub fn step_in(b: &dyn Block, id: ParamId) -> f32 {
    let p = ReverbParams {
        kind: ReverbType::at(b.get(P::TYPE)),
        sync: RevSync::at(b.get(P::SYNC)),
        ..ReverbParams::default()
    };
    step(&p, id)
}

/// TYPE `t`, leaving `p`'s knobs in `mem` (spec § A TYPE change loads
/// that TYPE's knobs): a nudge away and back loses nothing.
pub fn retype(p: &ReverbParams, t: ReverbType, mem: &mut KnobMemory) -> ReverbParams {
    mem.store(p);
    p.with_type(t, mem)
}

/// One encoder turn of `id` under `p`: TYPE walks the TYPEs on screen,
/// SYNC its TYPE's choices (ROOM has no ALL), a knob its `step`.
#[inline(never)]
pub fn turn(
    p: &ReverbParams,
    id: ParamId,
    delta: i8,
    tempo: Tempo,
    mem: &mut KnobMemory,
) -> ReverbParams {
    let walk = |n: usize, i: usize| (i as i32 + delta as i32).clamp(0, n as i32 - 1) as usize;
    match id {
        P::TYPE => {
            let list = types_on_screen();
            let i = list.iter().position(|&t| t == p.kind).unwrap_or(0);
            match list[walk(list.len(), i)] {
                t if t == p.kind => *p,
                t => retype(p, t, mem),
            }
        }
        P::SYNC => {
            let choices: &[RevSync] = match p.kind {
                ReverbType::Quad => return *p,
                ReverbType::Room => &[RevSync::Off, RevSync::Time],
                _ => &RevSync::ALL,
            };
            let now = p.sync.under(p.kind);
            let i = choices.iter().position(|&s| s == now).unwrap_or(0);
            p.with_sync(choices[walk(choices.len(), i)], tempo)
        }
        _ => {
            let mut q = *p;
            q.set(id, q.get(id) + delta as f32 * step(p, id));
            q
        }
    }
}

/// Slot `id`'s value as a cell under `t` shows it, from the same maps the
/// engine plays (`dsp::reverb::knobs`), at `tempo` from fold `fold`.
#[inline(never)]
pub fn value_text(
    t: ReverbType,
    id: ParamId,
    p: &ReverbParams,
    tempo: Tempo,
    fold: FoldState,
) -> FmtBuf {
    let p = &ReverbParams { kind: t, ..*p };
    let mut b = FmtBuf::new();
    let mut st = fold;
    let k = knobs(p, tempo, &mut st);
    let word = |b: &mut FmtBuf, w: &str| {
        let _ = b.write_str(w);
    };
    let (x, places, unit) = match id {
        P::TYPE => return text(ReverbType::NAMES[t.index()]),
        P::SYNC => return text(RevSync::NAMES[p.sync.under(t) as usize]),
        P::MIX => (clampf(p.mix, 0.0, 1.0) * 127.0, 0, ""),
        P::SIZE | P::PREDELAY if synced_list(p, id).is_some() => {
            let list = synced_list(p, id).unwrap_or(&[]);
            let v = if id == P::SIZE { p.size } else { p.predelay };
            let mark = match st.0 {
                0 => "",
                n if n > 0 => "▾",
                _ => "▴",
            };
            if let Some(d) = list.get(list_index(v, list.len())) {
                let _ = write!(b, "{mark}{}", d.folded(st.0));
            }
            return b;
        }
        P::SIZE => {
            if t == ReverbType::Hall && p.size < PLATE_ZONE {
                word(&mut b, "PLATE ");
            }
            (k.length_ms, 0, "ms")
        }
        P::PREDELAY => (k.predelay_ms, 0, "ms"),
        P::TIME if decay_in_beats(p) => {
            let n = DECAY_BEATS[list_index(p.time, DECAY_BEATS.len())];
            (n as f32, 0, if n == 1 { " BEAT" } else { " BEATS" })
        }
        P::TIME => match t {
            ReverbType::Gate if k.tail == 0.0 => return text("OFF"),
            ReverbType::Reverse if k.regen == 0.0 => return text("OFF"),
            ReverbType::Reverse => (k.regen * 100.0, 0, "%"),
            _ => secs(k.rt60_s),
        },
        P::DAMPING => khz(damp_hz(p.damping)),
        P::TONE => khz(k.tone_hz),
        P::DIFFUSE if k.taps > 0.0 => (k.taps, 1, ""),
        P::DIFFUSE => (k.diffuse_g, 2, ""),
        P::SHAPE => (k.shape_db, (k.shape_db.abs() < 9.95) as u32, "dB"),
        P::SMEAR if k.smear_ms == 0.0 => return text("OFF"),
        P::SMEAR => (k.smear_ms, 2, "ms"),
        P::RATE => (k.rate_hz, 2, "Hz"),
        _ => return b,
    };
    num(&mut b, x, places, unit);
    b
}

fn text(w: &str) -> FmtBuf {
    let mut b = FmtBuf::new();
    let _ = b.write_str(w);
    b
}

/// `x` to `places` decimals, then `unit`, in integers: `{:.1}` would link
/// core's float formatting, several KB.
#[inline(never)]
fn num(b: &mut FmtBuf, x: f32, places: u32, unit: &str) {
    let k = 10u32.pow(places);
    let n = libm::roundf(x * k as f32) as i32;
    if n < 0 {
        let _ = b.write_str("-");
    }
    let n = n.unsigned_abs();
    let _ = write!(b, "{}", n / k);
    if places > 0 {
        let _ = b.write_char('.');
        let mut d = k / 10;
        while d > 0 {
            let _ = b.write_char((b'0' + (n / d % 10) as u8) as char);
            d /= 10;
        }
    }
    let _ = b.write_str(unit);
}

/// Seconds: `0.55s`, `2.5s`, `10s`.
fn secs(s: f32) -> (f32, u32, &'static str) {
    let places = if s < 0.995 {
        2
    } else if s < 9.95 {
        1
    } else {
        0
    };
    (s, places, "s")
}

/// A cutoff: `6.6k`, `10k`.
fn khz(hz: f32) -> (f32, u32, &'static str) {
    (hz / 1000.0, (hz < 9_950.0) as u32, "k")
}
