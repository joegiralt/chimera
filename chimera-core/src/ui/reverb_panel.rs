//! Each reverb TYPE's panel as `const` data (reverb TYPEs spec § Pages,
//! the pattern ADR 0037's `filter_panel` set): REV's knobs 1–4 and
//! REV › MORE's six, each slot's word, value text and encoder step under
//! that TYPE. TYPE and MIX sit on REV outside the panel.

use crate::block::{Block, ParamId, find_spec};
use crate::dsp::rev::room;
use crate::dsp::rev::sync::{DECAY_BEATS, FoldState, Tempo, index_of, p_of};
use crate::dsp::reverb::{
    KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType, knobs, plate_pull, synced,
};

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
static LOOPS: RevKnob = k(P::SHAPE, "LOOPS");
static SIZE: RevKnob = k(P::SIZE, "SIZE");
static SWELL: RevKnob = k(P::SIZE, "SWELL");
static DECAY: RevKnob = k(P::TIME, "DECAY");
static DIFFUSE: RevKnob = RevKnob {
    short: Some("DIFF"),
    ..k(P::DIFFUSE, "DIFFUSE")
};
static PREDELAY: RevKnob = RevKnob {
    short: Some("PRE"),
    ..k(P::PREDELAY, "PRE-DLY")
};
/// A word choice: one tick a word (ROOM's walk skips ALL).
static SYNC: RevKnob = RevKnob {
    step: 1.0,
    ..k(P::SYNC, "SYNC")
};

/// SIZE · TIME · DAMP · SMEAR; MORE: RATE.
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

/// SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · DAMP · SMEAR · RATE ·
/// SYNC · LOOPS (ADR 0095).
static ROOM: Panel = [
    Some(&SIZE),
    Some(&DECAY),
    Some(&TONE),
    Some(&DIFFUSE),
    Some(&PREDELAY),
    Some(&DAMP),
    Some(&SMEAR),
    Some(&RATE),
    Some(&SYNC),
    Some(&LOOPS),
];

/// SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · DAMP · SMEAR · RATE ·
/// SYNC.
static HALL: Panel = [
    Some(&SIZE),
    Some(&DECAY),
    Some(&TONE),
    Some(&DIFFUSE),
    Some(&PREDELAY),
    Some(&DAMP),
    Some(&SMEAR),
    Some(&RATE),
    Some(&SYNC),
    None,
];

/// SWELL · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · SMEAR · RATE ·
/// SYNC (ADR 0095).
static BLOOM: Panel = [
    Some(&SWELL),
    Some(&DECAY),
    Some(&TONE),
    Some(&DIFFUSE),
    Some(&PREDELAY),
    None,
    Some(&SMEAR),
    Some(&RATE),
    Some(&SYNC),
    None,
];

/// LENGTH · TAIL · TONE · DENSITY; MORE: — · SHAPE · SMEAR · RATE · SYNC.
static GATE: Panel = [
    Some(&LENGTH),
    Some(&TAIL),
    Some(&TONE),
    Some(&DENSITY),
    None,
    Some(&SHAPE),
    Some(&SMEAR),
    Some(&RATE),
    Some(&SYNC),
    None,
];

/// LENGTH · REGEN · TONE · DENSITY; MORE: — · CURVE · SMEAR · RATE ·
/// SYNC.
static REVERSE: Panel = [
    Some(&LENGTH),
    Some(&REGEN),
    Some(&TONE),
    Some(&DENSITY),
    None,
    Some(&CURVE),
    Some(&SMEAR),
    Some(&RATE),
    Some(&SYNC),
    None,
];

fn panel(t: ReverbType) -> &'static Panel {
    match t {
        ReverbType::Room => &ROOM,
        ReverbType::Hall => &HALL,
        ReverbType::Gate => &GATE,
        ReverbType::Reverse => &REVERSE,
        ReverbType::Bloom => &BLOOM,
        ReverbType::Quad => &QUAD,
    }
}

/// The TYPEs TYPE walks, in screen order: each joined with its engine,
/// and now every one has its own.
pub fn types_on_screen() -> &'static [ReverbType] {
    &ReverbType::ALL
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

/// One encoder tick of `id` under `p`'s panel: a division, or a beat
/// count, while synced.
pub fn step(p: &ReverbParams, id: ParamId) -> f32 {
    match notches(p, id) {
        Some(n) => p_of(1, n),
        None => find(shown(p.kind), id).map_or(TICK, |c| c.step),
    }
}

/// How many notes or beat counts slot `id` walks under `p`'s SYNC.
pub fn notches(p: &ReverbParams, id: ParamId) -> Option<usize> {
    let s = synced(p);
    match s.time {
        Some((sid, list)) if sid == id => Some(list.len()),
        _ => (s.beats && id == P::TIME).then_some(DECAY_BEATS.len()),
    }
}

/// SYNC as `p.kind` reads it: ROOM's stored ALL is TIME.
fn sync_read(p: &ReverbParams) -> RevSync {
    match (p.kind, p.sync) {
        (ReverbType::Room, RevSync::All) => RevSync::Time,
        (_, s) => s,
    }
}

/// Whether a turn of `id` goes whole ticks, fine or not: a word choice,
/// or a synced knob's notes.
pub fn whole_ticks(p: &ReverbParams, id: ParamId) -> bool {
    matches!(id, P::TYPE | P::SYNC) || notches(p, id).is_some()
}

/// TYPE `t`, leaving `p`'s knobs in `mem` (spec § A TYPE change loads
/// that TYPE's knobs): a nudge away and back loses nothing.
pub fn retype(p: &ReverbParams, t: ReverbType, mem: &mut KnobMemory) -> ReverbParams {
    mem.store(p);
    p.with_type(t, mem)
}

/// One encoder turn of `id` under `p` at `tempo`: TYPE walks the TYPEs on
/// screen (a TYPE off the list stays put), SYNC its words converting the
/// knobs (ROOM's OFF and TIME only), a synced knob its notes, a knob its
/// `step`.
pub fn turn(
    p: &ReverbParams,
    id: ParamId,
    delta: i8,
    mem: &mut KnobMemory,
    tempo: Tempo,
) -> ReverbParams {
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
    if id == P::SYNC {
        let walk: &[RevSync] = match p.kind {
            ReverbType::Room => &RevSync::ALL[..2],
            _ => &RevSync::ALL,
        };
        let i = walk.iter().position(|&s| s == sync_read(p)).unwrap_or(0) as i32;
        let to = walk[(i + delta as i32).clamp(0, walk.len() as i32 - 1) as usize];
        return if to == sync_read(p) {
            *p
        } else {
            p.with_sync(to, tempo)
        };
    }
    q.set(id, q.get(id) + delta as f32 * step(p, id));
    q
}

/// Slot `id`'s value as `p`'s panel shows it at `tempo`, in units, from
/// the maps the engine plays (`dsp::reverb::knobs`): a synced time as the
/// note that plays, marked ▾ or ▴ by the engine's `fold`; DECAY in beats,
/// marked ▾ or ▴ where the row's range clamps it.
pub fn value_text(id: ParamId, p: &ReverbParams, tempo: Tempo, fold: FoldState) -> FmtBuf {
    let mut b = FmtBuf::new();
    let s = synced(p);
    if id == P::SYNC {
        let _ = core::fmt::Write::write_str(&mut b, RevSync::NAMES[sync_read(p) as usize]);
        return b;
    }
    if let Some((sid, list)) = s.time
        && sid == id
    {
        list[index_of(plate_pull(p).get(id), list.len())].write(fold, &mut b);
        return b;
    }
    if s.beats && id == P::TIME {
        // A count the row clamps plays shorter (▾) or longer (▴) than it
        // reads, marked as a folded note is.
        let n = DECAY_BEATS[index_of(p.time, DECAY_BEATS.len())];
        let asked = f32::from(n) * tempo.beat_ms() * 1e-3;
        let plays = knobs(p, tempo, &mut { fold }).rt60_s;
        let mark = if plays < 0.999 * asked {
            "▾"
        } else if plays > 1.001 * asked {
            "▴"
        } else {
            ""
        };
        let _ = core::fmt::Write::write_fmt(&mut b, format_args!("{mark}{n} BT"));
        return b;
    }
    // REGEN's RT60 rings for the LENGTH that plays.
    let k = knobs(p, tempo, &mut { fold });
    let burst = matches!(p.kind, ReverbType::Gate | ReverbType::Reverse);
    let off = match id {
        P::SMEAR => k.smear_ms == 0.0,
        // ROOM's DAMP at its top opens the dampers.
        P::DAMPING if p.kind == ReverbType::Room => k.damp_hz >= room::DAMP_OPEN_HZ,
        // And its TONE at its top the input filter.
        P::TONE if p.kind == ReverbType::Room => k.tone_hz >= room::TONE_OPEN_HZ,
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
        // ROOM's opens toward its top: what the filter does.
        P::TONE if p.kind == ReverbType::Room => khz(room::tone_shown_hz(k.tone_hz)),
        P::TONE => khz(k.tone_hz),
        // Taps: `10.2`; else the allpass g, `0.60`.
        P::DIFFUSE if burst => (k.taps, 1, ""),
        P::DIFFUSE => (k.diffuse_g, 2, ""),
        P::PREDELAY => (k.predelay_ms, 0, "ms"),
        // The curve end to end, whole dB: `-4dB`, `24dB` (tenths outrun
        // the focus band).
        P::SHAPE if burst => (k.shape_db, 0, "dB"),
        // LOOPS, θ in whole degrees: `0`, `90` (the face has no "°").
        P::SHAPE => (k.shape_db, 0, ""),
        // Tenths: hundredths would run into the cube.
        P::SMEAR => (k.smear_ms, 1, "ms"),
        P::RATE => (k.rate_hz, 2, "Hz"),
        _ => (0.0, 0, ""),
    };
    if off {
        let _ = core::fmt::Write::write_str(&mut b, "OFF");
    } else if places > 0 || !unit.is_empty() || id == P::SHAPE {
        fixed(&mut b, x, places, unit);
    }
    b
}

/// `hz` as kHz: `6.1k`, `11k`.
fn khz(hz: f32) -> (f32, u32, &'static str) {
    (hz / 1000.0, (hz < 9_950.0) as u32, "k")
}
