//! `ed` on the mod matrix (ADR 0289, protocol 4's slice 2): routes as
//! cells, `ed route`, `ed describe MTX-<ENGINE>`, FLT › MOD's route knobs
//! and `since`'s ROUTES section.
mod common;
mod screen;
use common::console_fake::{Fake, ask};

use chimera_core::console::ANSWER_BYTES_MAX;
use chimera_core::project::PartId;
use chimera_core::ui::PrimeStatus;
use chimera_engine::addr::{BlockRead, BlockRef, ParamAddr};
use chimera_engine::modulation::ModSource;
use chimera_engine::params::EngineType;

const P1: PartId = PartId::ALL[0];

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

/// UI passes enough for any route to reach its amount.
fn settle(f: &mut Fake) {
    for _ in 0..30 {
        apply(f);
    }
}

fn route(f: &mut Fake, cell: &str, to: &str) {
    assert_eq!(
        ask(f, &format!("ed route {cell} {to}\n")),
        "OK queued\n",
        "{cell} {to}"
    );
}

/// `ed since <n>`'s ROUTES lines and its new number (it owes nothing).
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32) {
    let got = ask(f, &format!("ed since {n}\n"));
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.pop().unwrap();
    let m = last.strip_prefix("OK ").unwrap().parse().unwrap();
    let routes = lines
        .into_iter()
        .filter(|l| l.starts_with("route") || l.starts_with("unroute"))
        .collect();
    (routes, m)
}

/// The amount `since` reports for `cell`, from a whole answer.
fn amount(f: &mut Fake, cell: &str) -> Option<i32> {
    let (lines, _) = since(f, 0);
    let head = format!("route {cell} ");
    lines
        .iter()
        .find_map(|l| l.strip_prefix(&head))
        .map(|r| r.split(' ').next().unwrap().parse().unwrap())
}

/// ALGO's registry destinations, from its schema: every `d`-flagged
/// param no fixed column has.
fn reg_dests(f: &mut Fake) -> Vec<String> {
    let n = chimera_core::console::ed_pages(EngineType::Algo);
    let mut v: Vec<String> = Vec::new();
    for i in 0..n {
        for l in ask(f, &format!("ed describe ALGO {i}\n")).lines() {
            let fixed = ["FILTER.CUTOFF", "FILTER.RESO", "OUT.VCA", "VOICEFX.P1"];
            if let Some(rest) = l.strip_prefix("param ")
                && (l.ends_with(" d") || l.ends_with(" md"))
            {
                let a = rest.split(' ').next().unwrap().to_string();
                if !fixed.contains(&a.as_str()) && !v.contains(&a) {
                    v.push(a);
                }
            }
        }
    }
    v
}

fn fake(e: EngineType) -> Fake {
    let mut f = Fake::new();
    if e != EngineType::Algo {
        screen::init_part_engine(&mut f.ui, P1, e);
    }
    f
}

#[test]
fn the_matrix_schema_is_its_sources_and_fixed_columns() {
    let mut f = Fake::new();
    let algo = ask(&mut f, "ed describe MTX-ALGO 0\n");
    let lines: Vec<&str> = algo.lines().collect();
    assert_eq!(lines[0], "page 0 MTX of 1");
    let srcs: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("src "))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    assert_eq!(
        srcs,
        [
            "ENV1", "ENV2", "ENV3", "LFO1", "LFO2", "LFO3", "VEL", "NOTE", "WHEEL", "PRESS",
            "TIMBRE", "OP5", "OP6"
        ]
    );
    for c in [
        "col FREQ FILTER.CUTOFF",
        "col Q FILTER.RESO",
        "col VCA OUT.VCA",
        "col FX VOICEFX.P1",
        "amount -127 127",
        "dests 16",
        "OK",
    ] {
        assert!(lines.contains(&c), "{c}: {algo}");
    }
    // Taps are ALGO's alone.
    let va = ask(&mut f, "ed describe MTX-VA 0\n");
    assert!(!va.contains("src OP5"), "{va}");
    assert_eq!(
        ask(&mut f, "ed describe MTX-VA 1\n"),
        "ERR ed describe MTX-VA has 1 pages\n"
    );
}

#[test]
fn flt_mods_knobs_are_route_cells() {
    let mut f = Fake::new();
    let n = chimera_core::console::ed_pages(EngineType::Algo);
    let all: String = (0..n)
        .map(|i| ask(&mut f, &format!("ed describe ALGO {i}\n")))
        .collect();
    for want in [
        "route \"ENV AMT\" ENV1:FILTER.CUTOFF route",
        "route \"KEYBOARD\" NOTE:FILTER.CUTOFF keybd",
        "route \"FREQ MOD\" FM3IN route",
        "route \"RES MOD\" QMIN route",
    ] {
        assert!(all.contains(want), "{want}");
    }
    assert!(!all.contains("next-slice"));
}

#[test]
fn every_source_into_every_fixed_column_round_trips() {
    for e in [EngineType::Algo, EngineType::Va] {
        let mut f = fake(e);
        let taps: &[&str] = match e {
            EngineType::Algo => &["OP5", "OP6"],
            _ => &[],
        };
        let srcs = ModSource::ALL
            .iter()
            .map(|s| s.disk_ident())
            .chain(taps.iter().copied());
        for s in srcs {
            for c in ["FILTER.CUTOFF", "FILTER.RESO", "OUT.VCA", "VOICEFX.P1"] {
                let cell = format!("{s}:{c}");
                route(&mut f, &cell, "-77");
                settle(&mut f);
                assert_eq!(amount(&mut f, &cell), Some(-77), "{e:?} {cell}");
                route(&mut f, &cell, "off");
                settle(&mut f);
                assert_eq!(amount(&mut f, &cell), None, "{e:?} {cell}");
            }
        }
    }
}

#[test]
fn a_route_glides_a_fast_turn_a_pass_never_a_jump() {
    let mut f = Fake::new();
    route(&mut f, "ENV2:FILTER.CUTOFF", "127");
    apply(&mut f);
    assert_eq!(amount(&mut f, "ENV2:FILTER.CUTOFF"), Some(10));
    apply(&mut f);
    assert_eq!(amount(&mut f, "ENV2:FILTER.CUTOFF"), Some(20));
    // A new amount for the same cell turns the one act round.
    route(&mut f, "ENV2:FILTER.CUTOFF", "15");
    apply(&mut f);
    assert_eq!(amount(&mut f, "ENV2:FILTER.CUTOFF"), Some(15));
    settle(&mut f);
    assert_eq!(amount(&mut f, "ENV2:FILTER.CUTOFF"), Some(15));
}

#[test]
fn a_registry_destination_is_primed_then_routed_and_full_writes_nothing() {
    let mut f = Fake::new();
    // A modulatable param no fixed column has: primed, as MIX + PLUS.
    route(&mut f, "LFO1:OP_C.LEVEL", "30");
    settle(&mut f);
    assert_eq!(amount(&mut f, "LFO1:OP_C.LEVEL"), Some(30));
    let s = &f.ui.project().part(P1).sound;
    assert!(s.dest_registry.is_primed(ParamAddr::new(
        BlockRef::AlgoOp(chimera_engine::addr::Op::C),
        chimera_engine::dsp::algo::params::AlgoOpParams::LEVEL
    )));
    // Fill the registry to 16, then more: a full registry primes nothing.
    let cells: Vec<String> = reg_dests(&mut f)
        .into_iter()
        .take(20)
        .map(|d| format!("ENV1:{d}"))
        .collect();
    assert_eq!(cells.len(), 20);
    let mut routed = 0;
    for c in &cells {
        if ask(&mut f, &format!("ed route {c} 5\n")) != "OK queued\n" {
            continue;
        }
        settle(&mut f);
        if amount(&mut f, c) == Some(5) {
            routed += 1;
        } else {
            assert_eq!(f.ui.prime_status(), Some(PrimeStatus::Full), "{c}");
        }
    }
    let s = &f.ui.project().part(P1).sound;
    assert_eq!(s.dest_registry.len(), 16);
    assert_eq!(routed, 16, "16 columns, OP_C.LEVEL among them");
}

#[test]
fn taps_off_algo_and_into_the_registry_are_refused() {
    let mut f = fake(EngineType::Va);
    assert_eq!(
        ask(&mut f, "ed route OP5:FILTER.CUTOFF 3\n"),
        "ERR ed route has no OP5:FILTER.CUTOFF here\n"
    );
    let mut g = Fake::new();
    for bad in [
        "ed route OP5:OP_C.CRSE 3",
        "ed route LFO1:PART3.LEVEL 3",
        "ed route LFO1:FILTER.CUTOFF 128",
        "ed route LFO1:FILTER.CUTOFF -128",
        "ed route LFO1:FILTER.CUTOFF x",
        "ed route NOPE:FILTER.CUTOFF 3",
        "ed route LFO1:FILTER.KIND 3",
        "ed route LFO1:FILTER.CUTOFF",
        "ed route FM3IN 3 4",
    ] {
        assert_eq!(
            ask(&mut g, &format!("{bad}\n")),
            "ERR ed route takes <cell> <-127..127|off>\n",
            "{bad}"
        );
    }
}

#[test]
fn a_jack_knob_lands_on_whatever_its_jack_names_in_the_pass() {
    let mut f = Fake::new();
    let lfo1 = amount(&mut f, "LFO1:FILTER.CUTOFF");
    route(&mut f, "FM3IN", "40");
    // Re-patched before the pass: the set applies first, the route after.
    assert_eq!(ask(&mut f, "ed set FILTER.FM3IN #1\n"), "OK queued\n");
    settle(&mut f);
    let jack =
        f.ui.project()
            .part(P1)
            .sound
            .params
            .block(BlockRef::Filter)
            .unwrap()
            .get(chimera_engine::params::FilterParams::FM3_IN);
    assert_eq!(jack, 1.0);
    assert_eq!(amount(&mut f, "LFO2:FILTER.CUTOFF"), Some(40));
    assert_eq!(
        amount(&mut f, "LFO1:FILTER.CUTOFF"),
        lfo1,
        "the old jack's cell"
    );
}

#[test]
fn since_diffs_routes_by_source_and_destination() {
    let mut f = Fake::new();
    let (lines, m) = since(&mut f, 0);
    assert!(lines.contains(&"routes 1".to_string()), "{lines:?}");
    route(&mut f, "LFO1:OP_C.LEVEL", "30");
    route(&mut f, "LFO2:OP_D.LEVEL", "20");
    settle(&mut f);
    let (lines, m) = since(&mut f, m);
    assert!(
        lines.contains(&"route LFO1:OP_C.LEVEL 30 \"+30\"".to_string()),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"route LFO2:OP_D.LEVEL 20 \"+20\"".to_string()),
        "{lines:?}"
    );
    // OP_C's column removed from the registry: OP_D's shifts left, and is
    // no change; OP_C's route is gone.
    route(&mut f, "LFO1:OP_C.LEVEL", "off");
    settle(&mut f);
    let (lines, _) = since(&mut f, m);
    assert_eq!(lines, vec!["unroute LFO1:OP_C.LEVEL".to_string()]);
}

#[test]
fn a_whole_routes_section_with_every_cell_fits_the_bound() {
    let mut f = Fake::new();
    let srcs: Vec<&str> = ModSource::ALL.iter().map(|s| s.disk_ident()).collect();
    for s in &srcs {
        for c in ["FILTER.CUTOFF", "FILTER.RESO", "OUT.VCA", "VOICEFX.P1"] {
            route(&mut f, &format!("{s}:{c}"), "-127");
        }
        settle(&mut f);
    }
    let dests = reg_dests(&mut f);
    for d in dests.iter().take(16) {
        for s in &srcs {
            let _ = ask(&mut f, &format!("ed route {s}:{d} -127\n"));
        }
        settle(&mut f);
    }
    let got = ask(&mut f, "ed since 0\n");
    let routes: usize = got
        .lines()
        .filter(|l| l.starts_with("route"))
        .map(|l| l.len() + 1)
        .sum();
    println!("a full ROUTES section is {routes} B");
    assert!(routes > 5000 && routes < ANSWER_BYTES_MAX / 2, "{routes}");
}
