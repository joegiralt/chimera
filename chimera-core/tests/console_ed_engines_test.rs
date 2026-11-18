//! `ed` on every engine (ADR 0285): addresses with pads, describe, get,
//! set and since on MODAL, VA, TAPE and DRUM, and protocol 3's scale and
//! `#n`.
mod common;
mod screen;
use common::console_fake::{Fake, ask};

use std::collections::BTreeSet;

use chimera_core::console::{
    ED_PANELS, ED_SLOTS, EdAddr, EdBlock, MODELS, PadBlock, SoundBlock, ed_pages, ed_panels_used,
    ed_slots_used, filter_variant, machine_variant, modal_variant, tape_variant,
};
use chimera_core::project::PartId;
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::nav::{Location, PartChain};
use chimera_engine::addr::{BlockRead, BlockRef};
use chimera_engine::block::Scale;
use chimera_engine::dsp::drum::sound::RouteId;
use chimera_engine::dsp::drum::types::{Machine, PadId};
use chimera_engine::dsp::filter::FilterKind;
use chimera_engine::dsp::tape_engine::params::TapeMachine;
use chimera_engine::dsp::voice_fx::params::FxType;
use chimera_engine::params::EngineType;

const P1: PartId = PartId::ALL[0];
/// What one answer can carry (`console_ed_test`'s, from the CDC floor).
const ANSWER_BYTES_MAX: usize = 32_000;

fn fake(e: EngineType) -> Fake {
    let mut f = Fake::new();
    if e != EngineType::Algo {
        screen::init_part_engine(&mut f.ui, P1, e);
    }
    assert_eq!(f.ui.project().part(P1).sound.engine(), e);
    f
}

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

fn page_texts(f: &mut Fake, e: EngineType) -> Vec<String> {
    (0..ed_pages(e))
        .map(|n| ask(f, &format!("ed describe {} {n}\n", e.disk_ident_for_test())))
        .collect()
}

trait Ident {
    fn disk_ident_for_test(self) -> &'static str;
}

impl Ident for EngineType {
    fn disk_ident_for_test(self) -> &'static str {
        use chimera_engine::block::DiskCode;
        self.disk_ident()
    }
}

/// Every `param` line's fields.
fn params(texts: &[String]) -> Vec<Vec<String>> {
    texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| l.starts_with("param "))
        .map(|l| {
            let (head, rest) = l.split_once(" \"").unwrap();
            let (label, tail) = rest.split_once("\" ").unwrap();
            let mut v: Vec<String> = head.split(' ').map(str::to_string).collect();
            v.push(label.to_string());
            v.extend(tail.split(' ').map(str::to_string));
            v
        })
        .collect()
}

fn listed(texts: &[String]) -> BTreeSet<String> {
    params(texts).into_iter().map(|p| p[1].clone()).collect()
}

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn set(f: &mut Fake, addr: &str, v: &str) {
    assert_eq!(
        ask(f, &format!("ed set {addr} {v}\n")),
        "OK queued\n",
        "{addr} {v}"
    );
    apply(f);
}

/// `ed get`'s stored value and text.
fn get(f: &mut Fake, addr: &str) -> (f32, String) {
    let got = ask(f, &format!("ed get {addr}\n"));
    let line = got.lines().next().unwrap();
    let rest = line
        .strip_prefix("val ")
        .unwrap_or_else(|| panic!("{addr}: {got}"));
    let mut w = rest.splitn(4, ' ');
    w.next();
    w.next();
    let stored = w.next().unwrap().parse().unwrap();
    let text = w.next().unwrap();
    (stored, text.trim_matches('"').to_string())
}

const OTHERS: [EngineType; 4] = [
    EngineType::Modal,
    EngineType::Va,
    EngineType::Tape,
    EngineType::Drum,
];

// ---- addresses ----

/// Every block an address can name.
fn all_blocks() -> Vec<EdBlock> {
    let mut v: Vec<EdBlock> = BlockRef::ALL
        .into_iter()
        .chain([BlockRef::Kit])
        .filter_map(SoundBlock::new)
        .map(EdBlock::Sound)
        .collect();
    for p in PadId::ALL {
        v.extend(PadBlock::ALL.map(|b| EdBlock::Pad(p, b)));
    }
    v
}

#[test]
fn every_address_round_trips() {
    let mut n = 0;
    for at in all_blocks() {
        let mut i = 0;
        while let Some(id) = at.id_at(i) {
            i += 1;
            let a = EdAddr::new(at, id).unwrap();
            let text = a.to_string();
            assert_eq!(EdAddr::parse(text.as_bytes()), Some(a), "{text}");
            n += 1;
        }
    }
    assert!(n > 900, "{n}");
    assert_eq!(
        EdAddr::parse(b"PAD12_RT4.AMOUNT").map(|a| a.at()),
        Some(EdBlock::Pad(pad(12), PadBlock::Route(RouteId::ALL[3])))
    );
}

#[test]
fn idents_are_unique_in_every_block() {
    for at in all_blocks() {
        let mut seen = BTreeSet::new();
        let mut i = 0;
        while let Some(id) = at.id_at(i) {
            i += 1;
            let ident = EdAddr::new(at, id).unwrap().param().spec().unwrap().ident;
            assert!(seen.insert(ident), "{at} has two {ident}");
        }
    }
}

#[test]
fn bad_pad_and_kit_tokens_are_refused() {
    for bad in [
        "PAD0.LEVEL",
        "PAD13.LEVEL",
        "PAD01.LEVEL",
        "PAD.LEVEL",
        "PAD1_RT5.AMOUNT",
        "PAD1_RT0.AMOUNT",
        "PAD1_XYZ.LEVEL",
        "PAD1.CUTOFF",
        // A pad's filter holds only its six.
        "PAD1_FLT.LEVEL",
        "KIT.NOPE",
        "KIT.LEVEL",
    ] {
        assert_eq!(EdAddr::parse(bad.as_bytes()), None, "{bad}");
    }
    // The kit's and a pad's block are never a Sound block.
    assert_eq!(SoundBlock::new(BlockRef::Pad), None);
    assert!(SoundBlock::new(BlockRef::Chorus).is_some());
}

#[test]
fn block_tokens_are_frozen() {
    let mut text = String::new();
    for at in all_blocks() {
        text.push_str(&at.to_string());
        text.push('\n');
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ed_block_tokens.txt");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    // Appending is allowed; a shipped token never changes.
    assert!(
        text.starts_with(&on_disk) && !on_disk.is_empty(),
        "a token moved: BLESS=1 rewrites {}",
        path.display()
    );
}

// ---- describe ----

#[test]
fn describe_walks_every_engine() {
    for e in EngineType::ALL {
        let mut f = Fake::new();
        let n = ed_pages(e);
        assert!(n > 0, "{e:?}");
        let texts = page_texts(&mut f, e);
        for (i, t) in texts.iter().enumerate() {
            assert!(t.starts_with(&format!("page {i} ")), "{e:?}: {t}");
            assert!(t.lines().next().unwrap().ends_with(&format!(" of {n}")));
            assert!(t.ends_with("\nOK\n"), "{t}");
            assert!(t.len() < ANSWER_BYTES_MAX, "{e:?} page {i}: {} B", t.len());
        }
        let past = ask(
            &mut f,
            &format!("ed describe {} {n}\n", e.disk_ident_for_test()),
        );
        assert!(past.starts_with("ERR ed describe "), "{past}");
        let label = |t: &String| {
            t.lines()
                .next()
                .unwrap()
                .split(' ')
                .nth(2)
                .unwrap()
                .to_string()
        };
        let labels: Vec<String> = texts.iter().map(label).collect();
        let on = |p: u8| {
            labels
                .iter()
                .filter(|l| l.starts_with(&format!("PAD{p}_")))
                .count()
        };
        if e == EngineType::Drum {
            let per = on(1);
            assert!(per >= 10, "{labels:?}");
            for p in 1..=12 {
                assert_eq!(on(p), per, "pad {p}");
            }
            assert!(labels.contains(&"PAD12_KIT".to_string()), "{labels:?}");
            assert!(labels.contains(&"MTX".to_string()), "{labels:?}");
        } else {
            assert_eq!(on(1), 0, "{e:?}");
            let chain = PartChain::base(e).def();
            let flat: usize = chain.blocks.iter().map(|b| 1 + b.sub_pages.len()).sum();
            let extra = usize::from(e == EngineType::Algo);
            assert_eq!(usize::from(n), flat + extra, "{e:?}");
        }
    }
}

#[test]
fn every_engine_variant_is_a_panel() {
    let has = |e: EngineType, head: &str| {
        let mut f = Fake::new();
        page_texts(&mut f, e)
            .join("")
            .contains(&format!("panel {head}\n"))
    };
    let modal = {
        let mut f = Fake::new();
        page_texts(&mut f, EngineType::Modal).join("")
    };
    for m in MODELS {
        let v = modal_variant(m);
        assert!(!v.contains(' ') && v != "-", "{m:?}");
        assert!(modal.contains(&format!("panel RESONATOR {v}\n")), "{m:?}");
    }
    for t in TapeMachine::ALL {
        let v = tape_variant(t);
        assert!(!v.contains(' ') && v != "-", "{t:?}");
        assert!(has(EngineType::Tape, &format!("MACHINE {v}")), "{t:?}");
    }
    let drum = {
        let mut f = Fake::new();
        page_texts(&mut f, EngineType::Drum).join("")
    };
    for m in Machine::ALL {
        let v = machine_variant(m).replace(' ', "-");
        assert!(v != "-", "{m:?}");
        assert!(drum.contains(&format!("panel PAD7_MCH {v}\n")), "{m:?}");
    }
    for k in FilterKind::BUILT {
        assert!(
            drum.contains(&format!("panel PAD7_FLT {}\n", filter_variant(k))),
            "{k:?}"
        );
    }
    for r in 1..=4 {
        assert!(drum.contains(&format!("panel PAD7_RT{r} -\n")), "{r}");
    }
    // A panel line is three words: the variant is one token.
    for e in EngineType::ALL {
        let mut f = Fake::new();
        for l in page_texts(&mut f, e)
            .join("")
            .lines()
            .filter(|l| l.starts_with("panel "))
        {
            assert_eq!(l.split(' ').count(), 3, "{l}");
        }
    }
    // DRUM has no Part filter panel: the pad's is FLT's.
    assert!(!drum.contains("panel FILTER "));
    // TAPE's SOURCE is read-only: a `source` line, never a param.
    let mut f = Fake::new();
    assert!(
        page_texts(&mut f, EngineType::Tape)
            .join("")
            .contains("source \"SOURCE\"\n")
    );
}

#[test]
fn every_param_line_says_its_scale() {
    let mut oct = 0;
    for e in EngineType::ALL {
        let mut f = Fake::new();
        for p in params(&page_texts(&mut f, e)) {
            // param <addr> "<label>" <kind> <scale> <min> <max> <step> <default>
            assert_eq!(p.len(), 9, "{p:?}");
            let a = EdAddr::parse(p[1].as_bytes()).unwrap();
            let want = match a.param().spec().unwrap().scale {
                Scale::Linear => "lin",
                Scale::Octaves => "oct",
            };
            assert_eq!(p[4], want, "{p:?}");
            assert!(
                ["cont", "choice", "toggle"].contains(&p[3].as_str()),
                "{p:?}"
            );
            oct += usize::from(want == "oct");
        }
    }
    assert!(oct > 0);
}

#[test]
fn engine_fixtures() {
    for e in OTHERS {
        let mut f = Fake::new();
        let text = page_texts(&mut f, e).join("");
        let name = e.disk_ident_for_test().to_lowercase();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../editor/tests/fixtures/{name}-schema.txt"));
        if std::env::var_os("BLESS").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            on_disk == text,
            "the schema changed: BLESS=1 rewrites {}",
            path.display()
        );
    }
}

// ---- get: every cell the box shows, read as the box reads it ----

/// Every page of the Part's chain, every pad and route the box can select:
/// each cell's address is described and `ed get` reads the box's text.
fn check_box_cells(f: &mut Fake, e: EngineType, listed: &BTreeSet<String>) -> usize {
    let chain = PartChain::of(&f.ui.project().part(P1).sound).def();
    let drum = e == EngineType::Drum;
    let pads: Vec<Option<PadId>> = if drum {
        vec![Some(pad(1)), Some(pad(12))]
    } else {
        vec![None]
    };
    let was = f.ui.project().selected_pad(P1);
    let mut n = 0;
    for node in 0..chain.blocks.len() {
        let mut sub = 0;
        while let Some(at) = chain.page(node, sub) {
            sub += 1;
            for &p in &pads {
                if let Some(p) = p {
                    f.ui.project_mut().select_pad(P1, p);
                }
                f.ui.go_for_test(Location::pages(P1, at));
                let routes: &[RouteId] = if f.ui.page_def().short == "ROUTE" {
                    &RouteId::ALL
                } else {
                    &RouteId::ALL[..1]
                };
                for &r in routes {
                    f.ui.select_route(r);
                    let (def, ctx) = (f.ui.page_def(), f.ui.slot_ctx_for_test());
                    for i in 0..6 {
                        let Some(a) = slot_addr(def, i, &ctx) else {
                            continue;
                        };
                        let ed = EdAddr::shown(a, p).unwrap().to_string();
                        assert!(listed.contains(&ed), "{e:?} {ed} on {}", def.short);
                        let want = f.ui.cell_text_for_test(i);
                        let (_, text) = get(f, &ed);
                        assert_eq!(text, want.as_str(), "{e:?} {ed} on {}", def.short);
                        n += 1;
                    }
                }
            }
        }
    }
    f.ui.project_mut().select_pad(P1, was);
    f.ui.select_route(RouteId::ALL[0]);
    n
}

/// The variant edits that move what a page shows on `e`: each axis
/// through every value, by option number.
fn variant_edits(e: EngineType) -> Vec<(String, usize)> {
    let mut v = vec![("VOICEFX.TYPE".to_string(), FxType::LIST.len())];
    match e {
        EngineType::Drum => {
            v.push(("PAD1.MACHINE".to_string(), Machine::ALL.len()));
            v.push(("PAD12_FLT.KIND".to_string(), FilterKind::BUILT.len()));
        }
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => {
            v.push(("FILTER.KIND".to_string(), FilterKind::BUILT.len()));
        }
    }
    match e {
        EngineType::Modal => v.push(("MODAL.MODE".to_string(), MODELS.len())),
        EngineType::Tape => v.push(("TAPE_ENG.MODEL".to_string(), TapeMachine::ALL.len())),
        EngineType::Algo | EngineType::Va | EngineType::Drum => {}
    }
    v
}

#[test]
fn every_box_cell_of_every_engine_is_described_and_reads_as_the_box() {
    for e in OTHERS {
        let mut f = fake(e);
        let listed = listed(&page_texts(&mut f, e));
        let mut n = check_box_cells(&mut f, e, &listed);
        for (addr, count) in variant_edits(e) {
            for i in 0..count {
                set(&mut f, &addr, &format!("#{i}"));
                n += check_box_cells(&mut f, e, &listed);
            }
            set(&mut f, &addr, "#0");
        }
        assert!(n > 100, "{e:?}: {n}");
    }
}

// ---- set ----

/// The machine knobs each pad's machine shows now: the params under the
/// `PADn_MCH <variant>` panel `since` reports.
fn knobs_shown(f: &mut Fake, texts: &[String]) -> BTreeSet<String> {
    let got = ask(f, "ed since 0\n");
    let heads: BTreeSet<String> = got
        .lines()
        .filter(|l| l.starts_with("panel ") && l.contains("_MCH "))
        .map(str::to_string)
        .collect();
    let mut shown = BTreeSet::new();
    let mut on = false;
    for l in texts.iter().flat_map(|t| t.lines()) {
        if l.starts_with("panel ") {
            on = heads.contains(l);
        } else if on && let Some(rest) = l.strip_prefix("param ") {
            shown.insert(rest.split(' ').next().unwrap().to_string());
        }
    }
    shown
}

#[test]
fn set_round_trips_on_every_engine() {
    for e in OTHERS {
        let mut f = fake(e);
        let texts = page_texts(&mut f, e);
        let knobs = knobs_shown(&mut f, &texts);
        let mut done = 0;
        for p in params(&texts) {
            if p[3] != "cont" {
                continue;
            }
            // A machine's knob takes a set only on a pad of that machine.
            if p[1].contains(".KNOB_") && !knobs.contains(&p[1]) {
                continue;
            }
            let a = EdAddr::parse(p[1].as_bytes()).unwrap();
            let spec = a.param().spec().unwrap();
            if !spec.stored || spec.patching {
                continue;
            }
            let (min, max): (f32, f32) = (p[5].parse().unwrap(), p[6].parse().unwrap());
            let at = (min + max) / 2.0;
            set(&mut f, &p[1], &format!("{at:.6}"));
            let (stored, _) = get(&mut f, &p[1]);
            let want = spec.quantized(at);
            assert!(
                (stored - want).abs() <= 1e-3 * want.abs().max(1.0),
                "{e:?} {}: {at} -> {stored} not {want}",
                p[1]
            );
            done += 1;
        }
        assert!(done > 20, "{e:?}: {done}");
    }
}

#[test]
fn a_pad_set_lands_on_its_pad_and_leaves_the_selection() {
    let mut f = fake(EngineType::Drum);
    f.ui.project_mut().select_pad(P1, pad(2));
    let level = |f: &Fake, n: u8| f.ui.project().kit(P1).pad(pad(n)).level().get();
    let (two, five) = (level(&f, 2), level(&f, 5));
    let to = if five > 60 { 20 } else { 100 };
    set(&mut f, "PAD5.LEVEL", &format!("{to}"));
    assert_ne!(level(&f, 5), five);
    assert_eq!(level(&f, 2), two);
    assert_eq!(f.ui.project().selected_pad(P1), pad(2));
    // The kit's TONAL is the kit's, on no pad.
    let (_, text) = get(&mut f, "KIT.TONAL");
    assert!(!text.is_empty());
}

#[test]
fn a_choke_set_past_eight_groups_writes_nothing() {
    let mut f = fake(EngineType::Drum);
    // INIT has 8 groups; pad 3 out of A (with pad 4) makes a 9th.
    let choke = |f: &mut Fake| get(f, "PAD3.CHOKE").1;
    let was = choke(&mut f);
    assert_ne!(was, "OFF");
    set(&mut f, "PAD3.CHOKE", "OFF");
    assert_eq!(choke(&mut f), was);
    // One that keeps the kit within 8 lands.
    set(&mut f, "PAD1.CHOKE", &was);
    assert_eq!(get(&mut f, "PAD1.CHOKE").1, was);
}

#[test]
fn a_choice_is_set_by_its_number() {
    let mut f = Fake::new();
    let opts = page_texts(&mut f, EngineType::Algo).join("");
    assert!(
        opts.contains("opt ENV1.HOLD 2 \"GATE EXT\"\n"),
        "GATE EXT moved"
    );
    set(&mut f, "ENV1.HOLD", "#2");
    assert_eq!(get(&mut f, "ENV1.HOLD").0, 2.0);
    assert_eq!(
        ask(&mut f, "ed set ENV1.HOLD #99\n"),
        "ERR ed set ENV1.HOLD has no such option\n"
    );
    assert_eq!(
        ask(&mut f, "ed set FILTER.CUTOFF #1\n"),
        "ERR ed set FILTER.CUTOFF takes a number\n"
    );
    for bad in ["#", "#-1", "#x", "#256"] {
        let got = ask(&mut f, &format!("ed set ENV1.HOLD {bad}\n"));
        assert!(got.starts_with("ERR ed takes "), "{bad}: {got}");
    }
}

// ---- since ----

fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32, usize) {
    let got = ask(f, &format!("ed since {n}\n"));
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.pop().unwrap();
    let m = last.strip_prefix("OK ").unwrap().parse().unwrap();
    (
        lines.into_iter().filter(|l| !l.is_empty()).collect(),
        m,
        got.len(),
    )
}

fn vals(lines: &[String]) -> usize {
    lines.iter().filter(|l| l.starts_with("val ")).count()
}

#[test]
fn since_answers_every_engine_whole_and_fits_the_clock() {
    for e in EngineType::ALL {
        let mut f = fake(e);
        let (lines, m, bytes) = since(&mut f, 0);
        assert_eq!(lines[0], format!("part 1 {}", e.disk_ident_for_test()));
        assert!(bytes < ANSWER_BYTES_MAX, "{e:?}: {bytes} B");
        assert!(vals(&lines) > 40, "{e:?}: {}", vals(&lines));
        // Every val is a described address.
        let listed = listed(&page_texts(&mut f, e));
        for l in lines.iter().filter(|l| l.starts_with("val ")) {
            let a = l.split(' ').nth(1).unwrap();
            assert!(listed.contains(a), "{e:?}: {a}");
        }
        assert_eq!(
            lines.iter().any(|l| l.starts_with("source \"")),
            e == EngineType::Tape,
            "{e:?}"
        );
        assert!(since(&mut f, m).0.is_empty(), "{e:?}");
        println!("{e:?}: a full answer is {bytes} B");
    }
}

#[test]
fn since_reports_every_pads_panels() {
    let mut f = fake(EngineType::Drum);
    let (lines, _, _) = since(&mut f, 0);
    for p in 1..=12 {
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with(&format!("panel PAD{p}_MCH "))),
            "{p}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with(&format!("panel PAD{p}_FLT "))),
            "{p}"
        );
    }
    assert!(!lines.iter().any(|l| l.starts_with("panel FILTER ")));
}

#[test]
fn a_pad_edit_is_a_diff_and_choosing_a_pad_is_nothing() {
    let mut f = fake(EngineType::Drum);
    let (_, m, _) = since(&mut f, 0);
    set(&mut f, "PAD5.LEVEL", "33");
    assert_eq!(get(&mut f, "PAD5.LEVEL").0, 33.0);
    let (lines, m, _) = since(&mut f, m);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("val PAD5.LEVEL "), "{lines:?}");
    // The box choosing a pad or a route moves no text the editor holds.
    f.ui.project_mut().select_pad(P1, pad(9));
    f.ui.select_route(RouteId::ALL[2]);
    let (lines, m, _) = since(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
    // A pad's machine relabels its knobs: its panel, and the values whole.
    let to = (get(&mut f, "PAD4.MACHINE").0 as usize + 1) % Machine::ALL.len();
    set(&mut f, "PAD4.MACHINE", &format!("#{to}"));
    let (lines, _, _) = since(&mut f, m);
    let want = format!(
        "panel PAD4_MCH {}",
        machine_variant(Machine::ALL[to]).replace(' ', "-")
    );
    assert!(lines.contains(&want), "{lines:?}");
    assert!(vals(&lines) > 300, "{}", vals(&lines));
}

#[test]
fn the_copy_holds_every_engine() {
    for e in EngineType::ALL {
        assert!(ed_slots_used(e) <= ED_SLOTS, "{e:?}");
        assert!(ed_panels_used(e) <= ED_PANELS, "{e:?}");
    }
    // DRUM, the most, sets both: nothing held for nothing.
    assert_eq!(ed_slots_used(EngineType::Drum), ED_SLOTS);
    assert_eq!(ed_panels_used(EngineType::Drum), ED_PANELS);
}

#[test]
fn reads_follow_the_named_pad_not_the_selection() {
    let mut f = fake(EngineType::Drum);
    let blk = |f: &Fake, n: u8| f.ui.project().kit(P1).pad(pad(n)).level().get() as f32;
    for n in [1u8, 6, 12] {
        f.ui.project_mut().select_pad(P1, pad(13 - n));
        let (stored, _) = get(&mut f, &format!("PAD{n}.LEVEL"));
        assert_eq!(stored, blk(&f, n), "pad {n}");
    }
    // On a Part that isn't DRUM, a pad is no address.
    let mut a = fake(EngineType::Va);
    assert_eq!(
        ask(&mut a, "ed get PAD1.LEVEL\n"),
        "ERR ed get has no PAD1.LEVEL here\n"
    );
    let _ = BlockRead::block(a.ui.params(), BlockRef::Va).is_some();
}
