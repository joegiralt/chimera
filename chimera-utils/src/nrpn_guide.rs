//! `docs/guide/NRPN.md` (#926): every writable NRPN address (ADR 0216),
//! grouped by engine and block, with the cell's screen name, its MSB:LSB
//! and the values it takes, written from `nrpn::chart`, the same code the
//! firmware answers with. Tests below keep it and `docs/midi/nrpn-chart.md`
//! current.

use std::fmt::Write;

use serde::Serialize;

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::nrpn::{self, ChartLine};
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::ui::mod_grid::block_name;
use chimera_midi::law::ValueLaw;

/// The guide's sections, in order: each engine's own blocks, the blocks
/// every engine reads, the Part, the master FX, ORBIT, then DRUM's pads
/// once the chart has them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Algo,
    Modal,
    Va,
    Tape,
    EveryEngine,
    Part,
    MasterFx,
    Orbit,
    Drum,
}

impl Section {
    fn heading(self) -> &'static str {
        match self {
            Section::Algo => "ALGO",
            Section::Modal => "MODAL",
            Section::Va => "VA",
            Section::Tape => "TAPE",
            Section::EveryEngine => "Every engine",
            Section::Part => "Part",
            Section::MasterFx => "Master FX",
            Section::Orbit => "ORBIT",
            Section::Drum => "DRUM",
        }
    }

    /// A writable cell's section; None for the UI's own blocks, which have
    /// no writable address.
    fn of(b: BlockRef) -> Option<Section> {
        Some(match b {
            BlockRef::Algo | BlockRef::AlgoOp(_) => Section::Algo,
            BlockRef::Modal => Section::Modal,
            BlockRef::Va => Section::Va,
            BlockRef::TapeEngine => Section::Tape,
            BlockRef::Filter
            | BlockRef::VoiceFx
            | BlockRef::Env(_)
            | BlockRef::Lfo(_)
            | BlockRef::Out
            | BlockRef::Pitch => Section::EveryEngine,
            BlockRef::Part | BlockRef::PartMix(_) => Section::Part,
            BlockRef::Chorus
            | BlockRef::Delay
            | BlockRef::Reverb
            | BlockRef::Tape
            | BlockRef::Comp => Section::MasterFx,
            BlockRef::OrbitChords | BlockRef::OrbitRing(_) => Section::Orbit,
            BlockRef::Pad
            | BlockRef::Kit
            | BlockRef::PadFilter
            | BlockRef::PadAmp
            | BlockRef::PadEnv
            | BlockRef::PadLfo
            | BlockRef::PadRoute(_) => Section::Drum,
            BlockRef::Theme | BlockRef::Midi | BlockRef::Tempo | BlockRef::Random => {
                return None;
            }
        })
    }
}

/// How a stepped cell's steps travel: CC 6 alone, or CC 6 and CC 38.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepBits {
    Msb,
    Fourteen,
}

/// The value column's kind, as the JSON names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum Kind {
    #[serde(rename = "14bit")]
    Fourteen,
    #[serde(rename = "msb")]
    Msb,
    #[serde(rename = "msb_code")]
    MsbCode,
}

/// A stepped cell's range as shown on screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Screen {
    from: String,
    to: String,
}

/// One choice: its frozen code and its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Code {
    code: u8,
    name: String,
}

/// A choice's codes: never empty, in code order, each code once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Codes(Vec<Code>);

impl Codes {
    /// Sorted by code; None if empty or a code repeats.
    fn new(mut codes: Vec<(u8, String)>) -> Option<Codes> {
        codes.sort_by_key(|c| c.0);
        if codes.is_empty() || codes.windows(2).any(|w| w[0].0 == w[1].0) {
            return None;
        }
        Some(Codes(
            codes
                .into_iter()
                .map(|(code, name)| Code { code, name })
                .collect(),
        ))
    }

    fn lowest(&self) -> u8 {
        self.0[0].code
    }

    fn highest(&self) -> u8 {
        self.0[self.0.len() - 1].code
    }
}

/// What a cell's value carries.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    /// The whole range over 0 to 16383.
    Place14,
    /// Steps from the lowest value, `0..=steps`; `screen` is the range as
    /// shown, when it isn't the steps themselves.
    Steps {
        bits: StepBits,
        steps: u16,
        screen: Option<Screen>,
    },
    /// A choice.
    Codes(Codes),
}

impl Value {
    fn kind(&self) -> Kind {
        match self {
            Value::Place14
            | Value::Steps {
                bits: StepBits::Fourteen,
                ..
            } => Kind::Fourteen,
            Value::Steps {
                bits: StepBits::Msb,
                ..
            } => Kind::Msb,
            Value::Codes(_) => Kind::MsbCode,
        }
    }

    fn range_md(&self) -> String {
        match self {
            Value::Place14 => "0 to 16383, lowest to highest".into(),
            Value::Steps {
                steps,
                screen: None,
                ..
            } => format!("0 to {steps}"),
            Value::Steps {
                steps,
                screen: Some(sc),
                ..
            } => format!("0 to {steps} ({} to {})", sc.from, sc.to),
            Value::Codes(codes) => codes
                .0
                .iter()
                .map(|c| format!("{} {}", c.code, c.name))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// The lowest and highest value CC 6 (and CC 38) carry.
    fn span(&self) -> (u16, u16) {
        match self {
            Value::Place14 => (0, 16383),
            Value::Steps { steps, .. } => (0, *steps),
            Value::Codes(codes) => (u16::from(codes.lowest()), u16::from(codes.highest())),
        }
    }
}

impl Kind {
    fn md(self) -> &'static str {
        match self {
            Kind::Fourteen => "14-bit",
            Kind::Msb => "MSB",
            Kind::MsbCode => "MSB code",
        }
    }
}

/// One writable address, as the page and the JSON list it.
struct Cell {
    block: BlockRef,
    name: &'static str,
    msb: u8,
    lsb: u8,
    value: Value,
}

impl Cell {
    fn section(&self) -> Section {
        Section::of(self.block).expect("a writable cell has a section")
    }

    fn number(&self) -> u16 {
        u16::from(self.msb) << 7 | u16::from(self.lsb)
    }
}

/// Every writable cell of the chart, by section, then the chart's block
/// and spec order.
fn cells() -> Vec<Cell> {
    let mut cells: Vec<Cell> = nrpn::chart()
        .filter_map(|l| match l {
            ChartLine::Live { at, block, spec } => {
                nrpn::writable(ParamAddr::new(block, spec.id))?;
                let a = at.get();
                Some(Cell {
                    block,
                    name: spec.label,
                    msb: a.msb().get(),
                    lsb: a.lsb().get(),
                    value: values(block, spec),
                })
            }
            ChartLine::Retired(_) => None,
        })
        .collect();
    // Stable: the chart's block and spec order within a section.
    cells.sort_by_key(Cell::section);
    cells
}

/// NRPN.json's shape; a change to a field bumps it.
pub const SCHEMA: u32 = 1;

/// NRPN.json, in its field order.
#[derive(Serialize)]
struct Doc<'a> {
    schema: u32,
    firmware: &'static str,
    cells: Vec<JsonCell<'a>>,
}

/// One cell of NRPN.json.
#[derive(Serialize)]
struct JsonCell<'a> {
    section: &'static str,
    block: &'static str,
    name: &'static str,
    msb: u8,
    lsb: u8,
    nrpn: u16,
    kind: Kind,
    min: u16,
    max: u16,
    screen: Option<&'a Screen>,
    codes: Option<&'a Codes>,
}

/// `NRPN.json` (#983): the page's cells for a script, from the same rows.
pub fn json() -> String {
    let cells = cells();
    let doc = Doc {
        schema: SCHEMA,
        firmware: chimera_core::ui::about_page::VERSION,
        cells: cells
            .iter()
            .map(|c| {
                let (min, max) = c.value.span();
                JsonCell {
                    section: c.section().heading(),
                    block: block_name(c.block),
                    name: c.name,
                    msb: c.msb,
                    lsb: c.lsb,
                    nrpn: c.number(),
                    kind: c.value.kind(),
                    min,
                    max,
                    screen: match &c.value {
                        Value::Steps { screen, .. } => screen.as_ref(),
                        Value::Place14 | Value::Codes(_) => None,
                    },
                    codes: match &c.value {
                        Value::Codes(codes) => Some(codes),
                        Value::Place14 | Value::Steps { .. } => None,
                    },
                }
            })
            .collect(),
    };
    let mut s = serde_json::to_string_pretty(&doc).expect("plain JSON");
    s.push('\n');
    s
}

const HEAD: &str = "# NRPN chart

Every cell below has a fixed NRPN address that never changes from one firmware to the next, so a sequencer's instrument definition (a Cirklon CK instrument, a Hapax definition, a DAW's NRPN lane) made once keeps working. No binding is needed: send the NRPN on the Part's MIDI channel and the cell moves, gliding as a knob does. A binding on the same NRPN number in the Sound's MIDI map wins over the address (see [MIDI control](MIDI.md)).

An NRPN is CC 99 (the address MSB), CC 98 (the address LSB), then CC 6 (the value MSB) and, for a 14-bit value, CC 38 (the value LSB). The **NRPN** column gives the address as MSB:LSB, then as one 14-bit number (MSB × 128 + LSB).

The **Value** column says what the value carries:
- **14-bit**: the cell's whole range over 0 to 16383. A 7-bit sender's MSB alone (0 to 127) covers the range too.
- **MSB**: a step count from the cell's lowest value, in CC 6 alone (CC 38 is ignored); the range in brackets is the value shown on screen.
- **MSB code**: a choice, its code in CC 6. The codes never change; they are not the choices' order on screen.

An ENGINE's cells move only on a Part playing that engine. The master FX are shared by every Part: their NRPNs work on any Part's channel. ORBIT's ring cells move the playing scene. Addresses not listed here are reserved and do nothing.

With SETTINGS › MIDI CONFIG › ECHO › PARAM OUT at `NRPN`, Chimera sends these same NRPNs, on the Part's channel, for every cell you turn on the panel, so a sequencer can record your edits. A master FX or ORBIT ring cell goes out on the active Part's channel.
";

/// The page's word on NRPN.json; `{SCHEMA}` is filled in.
macro_rules! json_note {
    () => {
        "Each release also ships this chart as `NRPN.json`, for a script that builds a controller template. It holds `schema` (its shape's version, now {SCHEMA}), `firmware` (the version it lists) and `cells`, one object per row below, in order: `section` and `block` (the headings), `name` (the cell's screen name), `msb`, `lsb` and `nrpn` (MSB × 128 + LSB), `kind` (`14bit`, `msb` or `msb_code`, the **Value** column), `min` and `max` (what CC 6, with CC 38 for `14bit`, carries: the lowest and highest code for `msb_code`), `screen` (`from` and `to`, the bracketed range shown on screen, else null) and `codes` (a choice's `code` and `name` list, else null)."
    };
}

/// The page, from the chart.
pub fn page() -> String {
    let mut s = String::from(HEAD);
    let _ = writeln!(s, concat!("\n", json_note!()), SCHEMA = SCHEMA);
    let mut at: Option<(Section, BlockRef)> = None;
    for c in cells() {
        let sec = c.section();
        if at.map(|a| a.0) != Some(sec) {
            let _ = write!(s, "\n## {}\n", sec.heading());
        }
        if at.map(|a| a.1) != Some(c.block) {
            // MODAL's one block is MODAL: no second heading.
            if block_name(c.block) != sec.heading() {
                let _ = write!(s, "\n### {}\n", block_name(c.block));
            }
            s.push_str("\n| Cell | NRPN | Value | Range |\n|---|---|---|---|\n");
        }
        at = Some((sec, c.block));
        let _ = writeln!(s, "{}", row(&c));
    }
    s
}

/// A cell's table row.
fn row(c: &Cell) -> String {
    format!(
        "| {} | {}:{} ({}) | {} | {} |",
        c.name,
        c.msb,
        c.lsb,
        c.number(),
        c.value.kind().md(),
        c.value.range_md()
    )
}

/// A param's value.
fn values(b: BlockRef, spec: &ParamSpec) -> Value {
    let stepped = |bits| {
        let steps = nrpn::steps(spec);
        let screen = match spec.fmt {
            ValFmt::Names(names) if names.len() > usize::from(steps) => Some(Screen {
                from: names[0].to_string(),
                to: names[usize::from(steps)].to_string(),
            }),
            ValFmt::Names(_)
            | ValFmt::Uni
            | ValFmt::Hold
            | ValFmt::Inf
            | ValFmt::Bi
            | ValFmt::Int(_)
            | ValFmt::OneBased(_)
            | ValFmt::Signed(_)
            | ValFmt::Pan
            | ValFmt::Route
            | ValFmt::Keybd
            | ValFmt::Law(_)
            | ValFmt::Bpm
            | ValFmt::LogHz { .. }
            | ValFmt::Vowel
            | ValFmt::Knob(..) => (spec.min != 0.0).then(|| Screen {
                from: format!("{}", spec.min),
                to: format!("{}", spec.max),
            }),
        };
        Value::Steps {
            bits,
            steps,
            screen,
        }
    };
    match nrpn::law(b, spec) {
        ValueLaw::Place14 => Value::Place14,
        ValueLaw::Index7 => stepped(StepBits::Msb),
        ValueLaw::Index14 => stepped(StepBits::Fourteen),
        ValueLaw::Code7 => {
            Value::Codes(Codes::new(codes(b, spec)).expect("a choice has codes, each once"))
        }
    }
}

/// A Sound choice's codes, `0 SVF, 1 LADDER`, each once (the first name
/// a code is seen with); FILTER MODE's through every TYPE.
fn codes(b: BlockRef, spec: &ParamSpec) -> Vec<(u8, String)> {
    let mut snap = ParamSnapshot::default();
    let mut out: Vec<(u8, String)> = Vec::new();
    let mut add = |c: u8, name: String| {
        if !out.iter().any(|o| o.0 == c) {
            out.push((c, name));
        }
    };
    let name = |i: u8| match spec.fmt {
        ValFmt::Names(n) => n[usize::from(i)].to_string(),
        ValFmt::OneBased(_) => (i + 1).to_string(),
        ValFmt::Uni
        | ValFmt::Hold
        | ValFmt::Inf
        | ValFmt::Bi
        | ValFmt::Int(_)
        | ValFmt::Signed(_)
        | ValFmt::Pan
        | ValFmt::Route
        | ValFmt::Keybd
        | ValFmt::Law(_)
        | ValFmt::Bpm
        | ValFmt::LogHz { .. }
        | ValFmt::Vowel
        | ValFmt::Knob(..) => i.to_string(),
    };
    if (b, spec.id) == (BlockRef::Filter, FilterParams::MODE) {
        for k in FilterKind::BUILT {
            let mut blk = snap.block_mut(b).expect("a Sound block");
            assert!(blk.set_enum_code(FilterParams::KIND, k.disk_code()));
            for (i, label) in k.mode_labels().iter().enumerate() {
                blk.set(spec.id, i as f32);
                add(blk.enum_code(spec.id).expect("a code"), label.to_string());
            }
        }
    } else {
        let mut blk = snap.block_mut(b).expect("a Code7 cell is a Sound's");
        for i in 0..=spec.max as u8 {
            blk.set(spec.id, f32::from(i));
            add(blk.enum_code(spec.id).expect("a code"), name(i));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide/NRPN.md");
    const CHART: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/midi/nrpn-chart.md");

    const JSON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide/NRPN.json");

    #[test]
    fn guide_page_is_current() {
        let now = std::fs::read_to_string(GUIDE).unwrap_or_default();
        assert!(
            now == page(),
            "docs/guide/NRPN.md is stale: `just nrpn-guide` writes it"
        );
        let now = std::fs::read_to_string(JSON).unwrap_or_default();
        assert!(
            now == json(),
            "docs/guide/NRPN.json is stale: `just nrpn-guide` writes it"
        );
    }

    fn parsed() -> serde_json::Value {
        serde_json::from_str(&json()).expect("valid JSON")
    }

    /// The field names, value types and min/max meaning a template script
    /// reads: changing one bumps SCHEMA.
    #[test]
    fn json_schema_is_pinned() {
        use serde_json::Value as J;
        let doc = parsed();
        let keys = |v: &J| {
            let mut k: Vec<String> = v.as_object().expect("an object").keys().cloned().collect();
            k.sort();
            k
        };
        let u = |v: &J| v.as_u64().expect("an unsigned number");
        let text = |v: &J| assert!(!v.as_str().expect("a string").is_empty());
        assert_eq!(SCHEMA, 1);
        assert_eq!(keys(&doc), ["cells", "firmware", "schema"]);
        assert_eq!(doc["schema"], SCHEMA);
        assert_eq!(doc["firmware"], chimera_core::ui::about_page::VERSION);
        let cells = doc["cells"].as_array().expect("cells");
        assert!(!cells.is_empty());
        for c in cells {
            assert_eq!(
                keys(c),
                [
                    "block", "codes", "kind", "lsb", "max", "min", "msb", "name", "nrpn", "screen",
                    "section"
                ]
            );
            for f in ["section", "block", "name"] {
                text(&c[f]);
            }
            let (msb, lsb) = (u(&c["msb"]), u(&c["lsb"]));
            assert!(msb < 128 && lsb < 128, "{c}");
            assert_eq!(u(&c["nrpn"]), msb * 128 + lsb);
            let (min, max) = (u(&c["min"]), u(&c["max"]));
            assert!(min <= max, "{c}");
            match &c["screen"] {
                J::Null => {}
                sc => {
                    assert_eq!(keys(sc), ["from", "to"]);
                    text(&sc["from"]);
                    text(&sc["to"]);
                }
            }
            match c["kind"].as_str().expect("a kind") {
                // The steps (or 0 to 16383) CC 6 and CC 38 carry.
                "14bit" => {
                    assert!(c["codes"].is_null());
                    assert!(min == 0 && max <= 16383, "{c}");
                }
                // Steps in CC 6 alone.
                "msb" => {
                    assert!(c["codes"].is_null());
                    assert!(min == 0 && max <= 127, "{c}");
                }
                // A choice: min and max its lowest and highest code.
                "msb_code" => {
                    assert!(c["screen"].is_null());
                    let codes = c["codes"].as_array().expect("codes");
                    let nums: Vec<u64> = codes
                        .iter()
                        .map(|o| {
                            assert_eq!(keys(o), ["code", "name"]);
                            text(&o["name"]);
                            u(&o["code"])
                        })
                        .collect();
                    assert!(
                        nums.windows(2).all(|w| w[0] < w[1]),
                        "sorted, distinct: {c}"
                    );
                    assert_eq!((min, max), (nums[0], nums[nums.len() - 1]));
                    assert!(max <= 127, "{c}");
                }
                k => panic!("kind {k}"),
            }
        }
    }

    /// The page names the schema version the JSON carries.
    #[test]
    fn page_names_the_schema_version() {
        assert!(page().contains(&format!("`schema` (its shape's version, now {SCHEMA})")));
    }

    /// The JSON's sections and blocks run in the page's heading order.
    #[test]
    fn json_runs_in_heading_order() {
        let doc = parsed();
        let runs = |field: &str| {
            let mut out: Vec<String> = Vec::new();
            for c in doc["cells"].as_array().expect("cells") {
                let v = c[field].as_str().expect("a string").to_string();
                if out.last() != Some(&v) {
                    out.push(v);
                }
            }
            out
        };
        let page = page();
        let headings = |prefix: &str| -> Vec<String> {
            page.lines()
                .filter_map(|l| l.strip_prefix(prefix).map(str::to_string))
                .collect()
        };
        assert_eq!(runs("section"), headings("## "));
        // A block heading is left out where it repeats its section (MODAL).
        let blocks: Vec<String> = {
            let mut out: Vec<String> = Vec::new();
            for c in doc["cells"].as_array().expect("cells") {
                let (b, sec) = (c["block"].as_str().unwrap(), c["section"].as_str().unwrap());
                let key = format!("{sec}/{b}");
                if out.last() != Some(&key) {
                    out.push(key);
                }
            }
            out.into_iter()
                .filter_map(|k| {
                    let (sec, b) = k.split_once('/').unwrap();
                    (sec != b).then(|| b.to_string())
                })
                .collect()
        };
        assert_eq!(blocks, headings("### "));
        let order: Vec<String> = runs("section");
        let sections = [
            Section::Algo,
            Section::Modal,
            Section::Va,
            Section::Tape,
            Section::EveryEngine,
            Section::Part,
            Section::MasterFx,
            Section::Orbit,
            Section::Drum,
        ];
        let at = |h: &str| sections.iter().position(|s| s.heading() == h).expect(h);
        assert!(order.windows(2).all(|w| at(&w[0]) < at(&w[1])), "{order:?}");
    }

    #[test]
    fn codes_are_non_empty_sorted_and_distinct() {
        assert!(Codes::new(Vec::new()).is_none());
        assert!(Codes::new(vec![(1, "A".into()), (1, "B".into())]).is_none());
        let c = Codes::new(vec![(3, "C".into()), (0, "A".into())]).expect("codes");
        assert_eq!((c.lowest(), c.highest()), (0, 3));
        assert_eq!(
            serde_json::to_string(&c).unwrap(),
            r#"[{"code":0,"name":"A"},{"code":3,"name":"C"}]"#
        );
    }

    /// Every NRPN.md row, rebuilt from the JSON alone, in order: the two
    /// can't disagree.
    #[test]
    fn json_round_trips_every_page_row() {
        let doc = parsed();
        let s = |v: &serde_json::Value| v.as_str().expect("a string").to_string();
        let u = |v: &serde_json::Value| v.as_u64().expect("a number");
        let from_json: Vec<String> = doc["cells"]
            .as_array()
            .expect("cells")
            .iter()
            .map(|c| {
                let (msb, lsb) = (u(&c["msb"]), u(&c["lsb"]));
                assert_eq!(u(&c["nrpn"]), msb * 128 + lsb);
                let (kind, range) = match s(&c["kind"]).as_str() {
                    "msb_code" => (
                        "MSB code",
                        c["codes"]
                            .as_array()
                            .expect("codes")
                            .iter()
                            .map(|o| format!("{} {}", u(&o["code"]), s(&o["name"])))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    k => {
                        let kind = if k == "msb" { "MSB" } else { "14-bit" };
                        let (min, max) = (u(&c["min"]), u(&c["max"]));
                        let range = match (&c["screen"], max) {
                            (serde_json::Value::Null, 16383) if k == "14bit" => {
                                "0 to 16383, lowest to highest".to_string()
                            }
                            (serde_json::Value::Null, _) => format!("{min} to {max}"),
                            (sc, _) => {
                                format!("{min} to {max} ({} to {})", s(&sc["from"]), s(&sc["to"]))
                            }
                        };
                        (kind, range)
                    }
                };
                format!(
                    "| {} | {msb}:{lsb} ({}) | {kind} | {range} |",
                    s(&c["name"]),
                    u(&c["nrpn"])
                )
            })
            .collect();
        let from_page: Vec<String> = page()
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| Cell |"))
            .map(str::to_string)
            .collect();
        assert_eq!(from_json, from_page);
        // The page's headings, from the JSON's section and block.
        let page = page();
        for c in doc["cells"].as_array().expect("cells") {
            assert!(page.contains(&format!("\n## {}\n", s(&c["section"]))));
            assert!(
                page.contains(&format!("\n### {}\n", s(&c["block"]))) || c["block"] == c["section"]
            );
        }
    }

    /// The chart's listing is `chimera-utils nrpn-chart`'s, line for line.
    #[test]
    fn chart_listing_is_current() {
        let doc = std::fs::read_to_string(CHART).unwrap();
        let body = doc.split("```\n").nth(1).expect("the listing");
        let want: String = nrpn::chart()
            .map(|l| format!("{}\n", l.labelled()))
            .collect();
        assert!(
            body == want,
            "docs/midi/nrpn-chart.md's listing is stale: paste `chimera-utils nrpn-chart`"
        );
    }

    #[test]
    fn page_names_cells_and_codes() {
        let p = page();
        assert!(p.contains("\n### FILTER\n"), "by block");
        assert!(p.contains("| CUTOFF | 10:0 (1280) | 14-bit | "), "{p}");
        assert!(
            p.contains("| TYPE | 10:6 (1286) | MSB code | 0 CLEAN, 1 LADDER"),
            "codes"
        );
        assert!(p.contains("| PITCH | 19:0 (2432) | MSB | 0 to 48 (-24 to 24) |"));
        // Reserved addresses are left out: SYSTEM's MIDI block.
        assert!(!p.contains("### MIDI"));
        let at = |h: &str| p.find(h).unwrap_or_else(|| panic!("{h}"));
        assert!(at("## ALGO") < at("## MODAL") && at("## Every engine") < at("## ORBIT"));
    }
}
