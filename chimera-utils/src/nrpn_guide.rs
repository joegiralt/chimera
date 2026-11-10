//! `docs/guide/NRPN.md` (#926): every writable NRPN address (ADR 0216),
//! grouped by engine and block, with the cell's screen name, its MSB:LSB
//! and the values it takes, written from `nrpn::chart`, the same code the
//! firmware answers with. Tests below keep it and `docs/midi/nrpn-chart.md`
//! current.

use std::fmt::Write;

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::nrpn::{self, ChartLine};
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::ui::mod_grid::block_name;
use chimera_midi::law::ValueLaw;

/// The guide's sections, in order: each engine's own blocks, the blocks
/// every engine reads, the Part, the master FX, ORBIT.
const SECTIONS: [&str; 8] = [
    "ALGO",
    "MODAL",
    "VA",
    "TAPE",
    "Every engine",
    "Part",
    "Master FX",
    "ORBIT",
];

fn section(b: BlockRef) -> usize {
    match b {
        BlockRef::Algo | BlockRef::AlgoOp(_) => 0,
        BlockRef::Modal => 1,
        BlockRef::Va => 2,
        BlockRef::TapeEngine => 3,
        BlockRef::Part | BlockRef::PartMix(_) => 5,
        BlockRef::Chorus | BlockRef::Delay | BlockRef::Reverb | BlockRef::Tape | BlockRef::Comp => {
            6
        }
        BlockRef::OrbitChords | BlockRef::OrbitRing(_) => 7,
        _ => 4,
    }
}

const HEAD: &str = "# NRPN chart

Every cell below has a fixed NRPN address that never changes from one firmware to the next, so a sequencer's instrument definition (a Cirklon CK instrument, a Hapax definition, a DAW's NRPN lane) made once keeps working. No binding is needed: send the NRPN on the Part's MIDI channel and the cell moves, gliding as a knob does. A binding on the same NRPN number in the Sound's MIDI map wins over the address (see [MIDI control](MIDI.md)).

An NRPN is CC 99 (the address MSB), CC 98 (the address LSB), then CC 6 (the value MSB) and, for a 14-bit value, CC 38 (the value LSB). The **NRPN** column gives the address as MSB:LSB, then as one 14-bit number (MSB × 128 + LSB).

The **Value** column says what the value carries:
- **14-bit**: the cell's whole range over 0 to 16383. A 7-bit sender's MSB alone (0 to 127) covers the range too.
- **MSB**: a step count from the cell's lowest value, in CC 6 alone (CC 38 is ignored); the range in brackets is the value shown on screen.
- **MSB code**: a choice, its code in CC 6. The codes never change; they are not the choices' order on screen.

An ENGINE's cells move only on a Part playing that engine. The master FX are shared by every Part: their NRPNs work on any Part's channel. ORBIT's ring cells move the playing scene. Addresses not listed here are reserved and do nothing.

With SETTINGS › MIDI CONFIG › ECHO › PARAM OUT at `NRPN`, Chimera sends these same NRPNs, on the Part's channel, for every cell you turn on the panel, so a sequencer can record your edits.
";

/// The page, from the chart.
pub fn page() -> String {
    let mut s = String::from(HEAD);
    let mut lines: Vec<(usize, BlockRef, &'static ParamSpec, u8, u8)> = nrpn::chart()
        .filter_map(|l| match l {
            ChartLine::Live { at, block, spec } => {
                nrpn::writable(ParamAddr::new(block, spec.id))?;
                let a = at.get();
                Some((section(block), block, spec, a.msb().get(), a.lsb().get()))
            }
            ChartLine::Retired(_) => None,
        })
        .collect();
    // Stable: the chart's block and spec order within a section.
    lines.sort_by_key(|l| l.0);
    let mut at: Option<(usize, BlockRef)> = None;
    for (sec, b, spec, m, l) in lines {
        if at.map(|a| a.0) != Some(sec) {
            let _ = write!(s, "\n## {}\n", SECTIONS[sec]);
        }
        if at.map(|a| a.1) != Some(b) {
            // MODAL's one block is MODAL: no second heading.
            if block_name(b) != SECTIONS[sec] {
                let _ = write!(s, "\n### {}\n", block_name(b));
            }
            s.push_str("\n| Cell | NRPN | Value | Range |\n|---|---|---|---|\n");
        }
        at = Some((sec, b));
        let (kind, range) = values(b, spec);
        let _ = writeln!(
            s,
            "| {} | {m}:{l} ({}) | {kind} | {range} |",
            spec.label,
            u16::from(m) << 7 | u16::from(l)
        );
    }
    s
}

/// A param's value column and range.
fn values(b: BlockRef, spec: &ParamSpec) -> (&'static str, String) {
    let n = |v: f32| format!("{}", v);
    match nrpn::law(b, spec) {
        ValueLaw::Place14 => ("14-bit", "0 to 16383, lowest to highest".into()),
        ValueLaw::Index7 | ValueLaw::Index14 => {
            let kind = if nrpn::law(b, spec) == ValueLaw::Index7 {
                "MSB"
            } else {
                "14-bit"
            };
            let steps = nrpn::steps(spec);
            let shown = match spec.fmt {
                ValFmt::Names(names) if names.len() > usize::from(steps) => {
                    format!("{} to {}", names[0], names[usize::from(steps)])
                }
                _ if spec.min == 0.0 => return (kind, format!("0 to {steps}")),
                _ => format!("{} to {}", n(spec.min), n(spec.max)),
            };
            (kind, format!("0 to {steps} ({shown})"))
        }
        ValueLaw::Code7 => ("MSB code", codes(b, spec)),
    }
}

/// A Sound choice's codes, `0 SVF, 1 LADDER`, in code order; FILTER MODE's
/// through every TYPE.
fn codes(b: BlockRef, spec: &ParamSpec) -> String {
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
        _ => i.to_string(),
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
    out.sort_by_key(|o| o.0);
    out.iter()
        .map(|(c, n)| format!("{c} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/guide/NRPN.md");
    const CHART: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/midi/nrpn-chart.md");

    #[test]
    fn guide_page_is_current() {
        let now = std::fs::read_to_string(GUIDE).unwrap_or_default();
        assert!(
            now == page(),
            "docs/guide/NRPN.md is stale: `just nrpn-guide` writes it"
        );
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
