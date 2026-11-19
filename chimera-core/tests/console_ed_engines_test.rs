//! `ed` on every engine (ADR 0285): addresses with pads, describe, get,
//! set and since on MODAL, VA, TAPE and DRUM, and protocol 3's scale and
//! `#n`.
mod common;
mod screen;
use common::console_fake::{Fake, ask};

use std::collections::BTreeSet;

use chimera_core::console::{
    BusBlock, ED_PANELS, ED_REFUSED, ED_SLOTS, EdAddr, EdBlock, GlobalBlock, MODELS, OrbitBlock,
    PadBlock, PartBlock, Round, Schema, SoundBlock, ed_pages, ed_panels_used, ed_slots_used,
    filter_variant, machine_variant, modal_variant, tape_variant,
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
use chimera_engine::midi_map::Mappable;
use chimera_engine::params::EngineType;

const P1: PartId = PartId::ALL[0];
use chimera_core::console::ANSWER_BYTES_MAX;

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

/// The block a box's block is, named by its own token: `None` for the
/// active Part's (`PART`, reserved), the mirror and a pad's.
fn named(b: BlockRef) -> Option<EdBlock> {
    if let Some(s) = SoundBlock::new(b) {
        return Some(EdBlock::Sound(s));
    }
    Some(match b {
        BlockRef::Chorus => EdBlock::Bus(BusBlock::Chorus),
        BlockRef::Delay => EdBlock::Bus(BusBlock::Delay),
        BlockRef::Reverb => EdBlock::Bus(BusBlock::Reverb),
        BlockRef::Tape => EdBlock::Bus(BusBlock::Tape),
        BlockRef::Comp => EdBlock::Bus(BusBlock::Comp),
        BlockRef::Theme => EdBlock::Global(GlobalBlock::Theme),
        BlockRef::Midi => EdBlock::Global(GlobalBlock::Midi),
        BlockRef::Random => EdBlock::Global(GlobalBlock::Random),
        BlockRef::OrbitChords => EdBlock::Orbit(OrbitBlock::Chords),
        BlockRef::OrbitRing(r) => EdBlock::Orbit(OrbitBlock::Ring(r)),
        BlockRef::Tempo => EdBlock::Orbit(OrbitBlock::Tempo),
        _ => return None,
    })
}

/// Every block an address can name, in the tokens fixture's order: the
/// box's blocks (`PART`, the active Part's, a reserved token since
/// protocol 3 and never reachable), the kit, each pad's, then protocol
/// 4's: each Part's mix and sends, `RANDOM` and `TEMPO`.
fn all_blocks() -> Vec<Option<EdBlock>> {
    let mut v: Vec<Option<EdBlock>> = BlockRef::ALL.into_iter().map(named).collect();
    v.push(Some(EdBlock::Sound(
        SoundBlock::new(BlockRef::Kit).unwrap(),
    )));
    for p in PadId::ALL {
        v.extend(PadBlock::ALL.map(|b| Some(EdBlock::Pad(p, b))));
    }
    for p in PartId::ALL {
        v.extend(PartBlock::ALL.map(|b| Some(EdBlock::Part(p, b))));
    }
    v.push(named(BlockRef::Random));
    v.push(named(BlockRef::Tempo));
    v
}

/// Every block an address names.
fn blocks() -> Vec<EdBlock> {
    all_blocks().into_iter().flatten().collect()
}

#[test]
fn every_address_round_trips() {
    let mut n = 0;
    for at in blocks() {
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
    for at in blocks() {
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
    // Nor a bus, the active Part's or a mirror: those are their own.
    for b in [
        BlockRef::Chorus,
        BlockRef::Part,
        BlockRef::PartMix(PartId::ALL[2]),
        BlockRef::Theme,
        BlockRef::Tempo,
    ] {
        assert_eq!(SoundBlock::new(b), None, "{b:?}");
    }
}

#[test]
fn block_tokens_are_frozen() {
    let mut text = String::new();
    for at in all_blocks() {
        text.push_str(&at.map_or("PART".to_string(), |b| b.to_string()));
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
            // param <addr> "<label>" <kind> <scale> <min> <max> <step> <default> <flags>
            assert_eq!(p.len(), 10, "{p:?}");
            let a = EdAddr::parse(p[1].as_bytes()).unwrap();
            let want = match a.param().spec().unwrap().scale {
                Scale::Linear => "lin",
                Scale::Octaves => "oct",
            };
            assert_eq!(p[4], want, "{p:?}");
            // The flags are the box's own rules (ADR 0289).
            let m = Mappable::new(a.param()).is_some();
            let d = a.param().modulatable();
            let flags = match (m, d) {
                (true, true) => "md",
                (true, false) => "m",
                (false, true) => "d",
                (false, false) => "-",
            };
            assert_eq!(p[9], flags, "{p:?}");
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
        PadId::ALL.iter().map(|&p| Some(p)).collect()
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
                        let ed = EdAddr::shown(a, p.map_or(Round::Shared, Round::Pad))
                            .unwrap()
                            .to_string();
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
/// through every value, by option number; on DRUM every pad at once.
fn variant_edits(e: EngineType) -> Vec<(Vec<String>, usize)> {
    let every_pad = |suffix: &str, ident: &str| -> Vec<String> {
        (1..=12)
            .map(|n| format!("PAD{n}{suffix}.{ident}"))
            .collect()
    };
    let mut v = vec![(vec!["VOICEFX.TYPE".to_string()], FxType::LIST.len())];
    match e {
        EngineType::Drum => {
            v.push((every_pad("", "MACHINE"), Machine::ALL.len()));
            v.push((every_pad("_FLT", "KIND"), FilterKind::BUILT.len()));
        }
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => {
            v.push((vec!["FILTER.KIND".to_string()], FilterKind::BUILT.len()));
        }
    }
    match e {
        EngineType::Modal => v.push((vec!["MODAL.MODE".to_string()], MODELS.len())),
        EngineType::Tape => v.push((vec!["TAPE_ENG.MODEL".to_string()], TapeMachine::ALL.len())),
        EngineType::Algo | EngineType::Va | EngineType::Drum => {}
    }
    v
}

/// Every continuous param `describe` lists at an untidy value: a third of
/// the way, plus a little, so readouts carry their longest decimals.
fn mess(f: &mut Fake, texts: &[String]) {
    for p in params(texts) {
        if p[3] != "cont" {
            continue;
        }
        let a = EdAddr::parse(p[1].as_bytes()).unwrap();
        let spec = a.param().spec().unwrap();
        if !spec.stored || spec.patching {
            continue;
        }
        let (min, max): (f32, f32) = (p[5].parse().unwrap(), p[6].parse().unwrap());
        let at = min + (max - min) * 0.371;
        assert_eq!(
            ask(f, &format!("ed set {} {at:.6}\n", p[1])),
            "OK queued\n",
            "{}",
            p[1]
        );
        apply(f);
    }
}

#[test]
fn every_box_cell_of_every_engine_is_described_and_reads_as_the_box() {
    for e in OTHERS {
        let mut f = fake(e);
        let texts = page_texts(&mut f, e);
        let listed = listed(&texts);
        let mut n = check_box_cells(&mut f, e, &listed);
        // Untidy values too: the longest readouts.
        mess(&mut f, &texts);
        n += check_box_cells(&mut f, e, &listed);
        for (addrs, count) in variant_edits(e) {
            for i in 0..count {
                for a in &addrs {
                    set(&mut f, a, &format!("#{i}"));
                }
                n += check_box_cells(&mut f, e, &listed);
            }
            for a in &addrs {
                set(&mut f, a, "#0");
            }
        }
        assert!(n > 100, "{e:?}: {n}");
    }
}

/// The longest `val` line address `p` (a describe `param` line's fields)
/// can make now: every option, or every step of a continuous range (257
/// points across a longer one), each read back by `ed get`, whose line is
/// `since`'s own. The value is put back afterwards.
fn longest_line(f: &mut Fake, p: &[String]) -> usize {
    let addr = &p[1];
    let (min, max, step): (f32, f32, f32) = (
        p[5].parse().unwrap(),
        p[6].parse().unwrap(),
        p[7].parse().unwrap(),
    );
    let a = EdAddr::parse(addr.as_bytes()).unwrap();
    let spec = a.param().spec().unwrap();
    let was = get(f, addr).0;
    let words: Vec<String> = if p[3] == "cont" {
        let n = ((max - min) / step.max(1e-6)).round() as usize;
        let k = n.clamp(1, 256);
        (0..=k)
            .map(|i| format!("{:.6}", min + (max - min) * i as f32 / k as f32))
            .collect()
    } else {
        (0..=(max - min).round() as usize)
            .map(|i| format!("#{i}"))
            .collect()
    };
    let mut longest = 0;
    for w in &words {
        if !spec.stored || spec.patching {
            break;
        }
        let got = ask(f, &format!("ed set {addr} {w}\n"));
        if got != "OK queued\n" {
            continue;
        }
        apply(f);
        let line = ask(f, &format!("ed get {addr}\n"));
        longest = longest.max(line.lines().next().unwrap().len() + 1);
    }
    let _ = ask(f, &format!("ed set {addr} {was}\n"));
    apply(f);
    // A param no set reaches (read only, or patched): its line now.
    longest.max(
        ask(f, &format!("ed get {addr}\n"))
            .lines()
            .next()
            .unwrap()
            .len()
            + 1,
    )
}

/// The worst DRUM `since 0` answer, bounded from above under these
/// assumptions, each this test's, not proven by it:
/// - a cell's text depends only on its own value, its pad's MACHINE and
///   filter KIND, and its panel's variant;
/// - a continuous param's longest text is among 257 evenly spaced values
///   (every step where it has fewer);
/// - pad 12's lines are each pad's longest: its address tokens are the
///   longest, and its texts are any pad's;
/// - the Part's own lines hold at most a cell's 32-byte text.
///
/// Each address's line is maximised over its values, each pad over its
/// machine and kind, each panel over its variants. A diff of every value
/// may carry `refused` lines as well: the four the unit keeps, each at the
/// longest address and reason, are counted in. DRUM shows no SOURCE
/// cell, so the answer has no `source` line (asserted below; TAPE's
/// answer has one, not bounded here).
#[test]
fn the_worst_drum_answer_fits_one_answer() {
    let mut f = fake(EngineType::Drum);
    let (lines, _, _) = since(&mut f, 0);
    assert!(
        !lines.iter().any(|l| l.starts_with("source ")),
        "DRUM writes no source line"
    );
    let texts = page_texts(&mut f, EngineType::Drum);
    let all = params(&texts);
    let pad12: Vec<&Vec<String>> = {
        let mut seen = BTreeSet::new();
        all.iter()
            .filter(|p| {
                (p[1].starts_with("PAD12.") || p[1].starts_with("PAD12_"))
                    && seen.insert(p[1].clone())
            })
            .collect()
    };
    let knob = |p: &Vec<String>| p[1].starts_with("PAD12.KNOB_");
    let flt = |p: &Vec<String>| p[1].starts_with("PAD12_FLT.") && p[1] != "PAD12_FLT.KIND";
    // A pad's lines on no machine or kind: measured once.
    let mut pad = 0;
    for p in pad12.iter().filter(|p| !knob(p) && !flt(p)) {
        pad += longest_line(&mut f, p);
    }
    // Its machine's knobs on the machine that makes them longest.
    let mut knobs = 0;
    for m in 0..Machine::ALL.len() {
        set(&mut f, "PAD12.MACHINE", &format!("#{m}"));
        let sum: usize = pad12
            .iter()
            .filter(|p| knob(p))
            .map(|p| longest_line(&mut f, p))
            .sum();
        knobs = knobs.max(sum);
    }
    // Its filter's cells on the kind that makes them longest.
    let mut filter = 0;
    for k in 0..FilterKind::BUILT.len() {
        set(&mut f, "PAD12_FLT.KIND", &format!("#{k}"));
        let sum: usize = pad12
            .iter()
            .filter(|p| flt(p))
            .map(|p| longest_line(&mut f, p))
            .sum();
        filter = filter.max(sum);
    }
    let per_pad = pad + knobs + filter;
    // The Part's own lines: `val <addr> <norm> <stored> "<text>"`, the
    // norm at most 9 characters, the stored value its spec's widest, the
    // text at most a cell's 32 bytes.
    let part: usize = {
        let mut seen = BTreeSet::new();
        all.iter()
            .filter(|p| !p[1].starts_with("PAD") && seen.insert(p[1].clone()))
            .map(|p| {
                let wide =
                    |v: &str| v.trim_start_matches('-').split('.').next().unwrap().len() + 1 + 8;
                let stored = wide(&p[5]).max(wide(&p[6]));
                "val ".len() + p[1].len() + 1 + 9 + 1 + stored + 2 + 32 + 2
            })
            .sum()
    };
    // Each panel at its longest variant, as `describe` lists them.
    let mut panels: std::collections::BTreeMap<String, usize> = Default::default();
    for l in texts
        .iter()
        .flat_map(|t| t.lines())
        .filter(|l| l.starts_with("panel "))
    {
        let name = l.split(' ').nth(1).unwrap().to_string();
        let e = panels.entry(name).or_default();
        *e = (*e).max(l.len() + 1);
    }
    let shown: usize = panels
        .iter()
        .filter(|(n, _)| {
            n.ends_with("_MCH")
                || n.ends_with("_FLT")
                || ["FX", "ENV1", "ENV2", "ENV3", "LFO1", "LFO2", "LFO3"].contains(&n.as_str())
        })
        .map(|(_, len)| len)
        .sum();
    let head = "\n".len() + "part 1 DRUM\n".len() + "OK 999999999\n".len();
    // A diff that moved every value says the turns refused too: the
    // newest four, each `refused <ADDR> "<reason>"`, the address the
    // longest listed, the reason a full `Line` (the toast's, cut there).
    let longest_addr = all.iter().map(|p| p[1].len()).max().unwrap();
    let refused = ED_REFUSED
        * ("refused ".len()
            + longest_addr
            + " \"".len()
            + chimera_core::project::LINE_LEN
            + "\"\n".len());
    let worst = head + shown + part + 12 * per_pad + refused;
    println!(
        "the worst DRUM answer: at most {worst} B of {ANSWER_BYTES_MAX} (a pad {per_pad} B, the Part {part} B, the panels {shown} B, the refusals {refused} B)"
    );
    assert!(worst < ANSWER_BYTES_MAX, "{worst} B");
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

/// `ed since <n>`'s PART section (the lines before any other section's
/// header), its number and its bytes. It owes nothing.
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32, usize) {
    let got = ask(f, &format!("ed since {n}\n"));
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.pop().unwrap();
    let m = last.strip_prefix("OK ").unwrap().parse().unwrap();
    (
        lines
            .into_iter()
            .filter(|l| !l.is_empty())
            .take_while(|l| l != "mixer")
            .filter(|l| {
                !l.starts_with("routes ") && !l.starts_with("route ") && !l.starts_with("unroute ")
            })
            .collect(),
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
        assert!(ed_slots_used(Schema::Engine(e)) <= ED_SLOTS, "{e:?}");
        assert!(ed_panels_used(e) <= ED_PANELS, "{e:?}");
    }
    // DRUM, the most, sets both: nothing held for nothing.
    assert_eq!(ed_slots_used(Schema::Engine(EngineType::Drum)), ED_SLOTS);
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

// ---- refusals: what the box's turn refuses, `ed set` writes nothing of ----

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::ui::held::{self, AlgSide, Pick, Source, TapeTurn};
use chimera_engine::addr::Op;
use chimera_engine::addr::ParamAddr;

/// Part 1 TAPE on a streamed 30 s 24 kHz source (tape_musical_ui_test's).
fn streamed() -> Fake {
    use chimera_core::storage::{SourceRef, TapeSource};
    use chimera_core::stream::ring::{Rings, ThirdSlots};
    use chimera_core::stream::test_card::TestCard;
    use common::tape::{held_plain, sine};
    let whole = held_plain(&sine(24_000, 100.7, 720_000), 24_000);
    let rings: &'static Rings = Box::leak(Box::new(Rings::new()));
    let third: &'static ThirdSlots = Box::leak(Box::new(ThirdSlots::new()));
    let (_, card) = rings.sides(third).unwrap();
    let bytes = TestCard::new(card, &whole, 0).held(&[0, 0], 4_800);
    let r = SourceRef {
        crc: 0x5EED,
        frames: 720_000,
        rate: 24_000,
    };
    let mut f = fake(EngineType::Tape);
    f.ui.project_mut().source_store(r, &bytes).unwrap();
    f.ui.project_mut().edit_part(P1).sound.source = Some(TapeSource { r, tune: None });
    let s = &f.ui.project().part(P1).sound;
    assert!(f.ui.project().pool().streamed(s.source_ref().unwrap()));
    f
}

fn ed(a: ParamAddr) -> String {
    EdAddr::sound(a).unwrap().to_string()
}

#[test]
fn a_tape_turn_a_streamed_source_refuses_writes_nothing() {
    let mut f = streamed();
    // The review's repro: SCRUB on a streamed source.
    set(&mut f, "TAPE_ENG.MODEL", "#2");
    assert_eq!(get(&mut f, "TAPE_ENG.MODEL").1, "WALK");
    let (lines, _, _) = since(&mut f, 0);
    // Said only as the refusal: no value reads it.
    assert!(
        !lines
            .iter()
            .any(|l| l.starts_with("val ") && l.contains("NEEDS RAM")),
        "{lines:?}"
    );
    // Every MODEL and DIRECTION: lands where the box's turn lands, else
    // nothing written.
    let mut refused = 0;
    for a in [held::MODEL, held::DIRECTION] {
        for i in 0..4u8 {
            set(&mut f, "TAPE_ENG.MODEL", "#0");
            set(&mut f, &ed(a), "#0");
            let turn = TapeTurn::of(a, f32::from(i)).unwrap();
            let why = Pick::Tape(turn).why(&f.ui.project().part(P1).sound, Source::Streamed);
            set(&mut f, &ed(a), &format!("#{i}"));
            let want = if why.is_some() { 0.0 } else { f32::from(i) };
            assert_eq!(get(&mut f, &ed(a)).0, want, "{} #{i}: {why:?}", ed(a));
            refused += usize::from(why.is_some());
        }
    }
    assert!(refused >= 3, "{refused}");
}

#[test]
fn a_formant_kind_or_algorithm_past_the_budget_writes_nothing() {
    let mut f = fake(EngineType::Algo);
    let formant = OpKind::Formant.code();
    let mut refused = 0;
    for op in Op::ALL {
        let a = ParamAddr::new(
            BlockRef::AlgoOp(op),
            chimera_engine::dsp::algo::params::AlgoOpParams::WAVE,
        );
        let was = get(&mut f, &ed(a)).0;
        let why =
            Pick::Wave(op, OpKind::Formant).why(&f.ui.project().part(P1).sound, Source::InRam);
        set(&mut f, &ed(a), &format!("#{formant}"));
        let want = if why.is_some() {
            was
        } else {
            f32::from(formant)
        };
        assert_eq!(get(&mut f, &ed(a)).0, want, "{op:?}: {why:?}");
        refused += usize::from(why.is_some());
    }
    assert!(refused > 0, "the budget never refused a formant");
    for side in [AlgSide::A, AlgSide::B] {
        let a = ParamAddr::new(BlockRef::Algo, side.param());
        for code in 0..32u8 {
            let was = get(&mut f, &ed(a)).0;
            let why = Pick::Alg(side, AlgoId::clamped(code))
                .why(&f.ui.project().part(P1).sound, Source::InRam);
            set(&mut f, &ed(a), &format!("#{code}"));
            let want = if why.is_some() { was } else { f32::from(code) };
            assert_eq!(get(&mut f, &ed(a)).0, want, "{side:?} {code}: {why:?}");
        }
    }
}

// ---- the box's cue for an ed set: a held choice ends, a refusal says why ----

use chimera_core::ui::busy::ToastStep;

/// TAPE on a streamed source at DECK, as tape_musical_ui_test's `ui_on`.
fn streamed_at_deck() -> Fake {
    use chimera_core::storage::{SourceRef, TapeSource};
    use chimera_core::stream::ring::{Rings, ThirdSlots};
    use chimera_core::stream::test_card::TestCard;
    use common::tape::{held_plain, sine};
    let whole = held_plain(&sine(24_000, 100.7, 720_000), 24_000);
    let rings: &'static Rings = Box::leak(Box::new(Rings::new()));
    let third: &'static ThirdSlots = Box::leak(Box::new(ThirdSlots::new()));
    let (_, card) = rings.sides(third).unwrap();
    let bytes = TestCard::new(card, &whole, 0).held(&[0, 0], 4_800);
    let r = SourceRef {
        crc: 0x5EED,
        frames: 720_000,
        rate: 24_000,
    };
    let mut f = Fake::new();
    screen::load_init(&mut f.ui, EngineType::Tape);
    f.ui.project_mut().source_store(r, &bytes).unwrap();
    f.ui.project_mut().edit_part(P1).sound.source = Some(TapeSource { r, tune: None });
    screen::plus(&mut f.ui, 1);
    f
}

fn toast(f: &mut Fake) -> Option<String> {
    match f.ui.step_toast(0) {
        ToastStep::Show(l) => Some(l.as_str().to_string()),
        _ => None,
    }
}

#[test]
fn an_ed_set_on_a_held_cell_ends_the_hold() {
    use chimera_hal::EncoderId;
    let mut f = streamed_at_deck();
    let dir = ed(held::DIRECTION);
    // The box holds PING-PONG, refused, dimmed.
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::D, 1));
    assert!(f.ui.held_choice().is_some(), "PING-PONG held");
    // The editor's FORWARD lands, and the hold is gone.
    set(&mut f, &dir, "#0");
    assert!(f.ui.held_choice().is_none(), "a landed set ends the hold");
    // Held again; a refused set ends it too, and says why.
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::D, 3));
    assert!(f.ui.held_choice().is_some(), "held again");
    f.ui.step_toast(60_000);
    set(&mut f, &dir, "#3");
    assert!(f.ui.held_choice().is_none(), "a refused set ends the hold");
    assert_eq!(toast(&mut f).as_deref(), Some("PING NEEDS RAM · STREAMED"));
}

#[test]
fn a_refused_ed_set_says_why_as_a_cc_does() {
    // TAPE: the streamed source's reason.
    let mut f = streamed_at_deck();
    f.ui.step_toast(60_000);
    set(&mut f, "TAPE_ENG.MODEL", "#2");
    assert_eq!(toast(&mut f).as_deref(), Some("SCRUB NEEDS RAM · STREAMED"));
    // ALGO: the formant budget, as a CC past it says.
    let mut f = fake(EngineType::Algo);
    let formant = OpKind::Formant.code();
    let mut said = None;
    for op in Op::ALL {
        let a = ParamAddr::new(
            BlockRef::AlgoOp(op),
            chimera_engine::dsp::algo::params::AlgoOpParams::WAVE,
        );
        f.ui.step_toast(60_000);
        set(&mut f, &ed(a), &format!("#{formant}"));
        if let Some(t) = toast(&mut f) {
            said = Some(t);
        }
    }
    assert_eq!(said.as_deref(), Some("FORMANT BUDGET FULL"));
    // DRUM: a 9th group, its own words.
    let mut f = fake(EngineType::Drum);
    f.ui.step_toast(60_000);
    set(&mut f, "PAD3.CHOKE", "OFF");
    assert!(
        toast(&mut f).is_some_and(|t| t.contains("GROUPS")),
        "the groups' reason"
    );
    // A set that lands says nothing.
    f.ui.step_toast(60_000);
    set(&mut f, "PAD5.LEVEL", "40");
    assert_eq!(toast(&mut f), None);
}

// ---- refusals over the protocol: `refused <ADDR> "<reason>"` in since ----

/// `since`'s `refused` lines, each checked to come before its `OK`, and
/// the answer's number.
fn refused_lines(f: &mut Fake, n: u32) -> (Vec<String>, u32) {
    let got = ask(f, &format!("ed since {n}\n"));
    let ok = got.rfind("OK ").unwrap();
    let lines: Vec<String> = got
        .lines()
        .filter(|l| l.starts_with("refused "))
        .map(str::to_string)
        .collect();
    assert!(
        got[..ok].matches("refused ").count() == lines.len(),
        "{got}"
    );
    let m = got[ok + 3..].trim_end().parse().unwrap();
    (lines, m)
}

/// A turn the box refuses is said in the next `since`, in the toast's
/// own words, once.
#[test]
fn a_refused_turn_is_said_in_since_in_the_toasts_words() {
    // TAPE: a streamed source.
    let mut f = streamed_at_deck();
    let (_, m) = refused_lines(&mut f, 0);
    f.ui.step_toast(60_000);
    set(&mut f, "TAPE_ENG.MODEL", "#2");
    let said = toast(&mut f).unwrap();
    let (lines, m) = refused_lines(&mut f, m);
    assert_eq!(lines, [format!("refused TAPE_ENG.MODEL \"{said}\"")]);
    assert_eq!(said, "SCRUB NEEDS RAM · STREAMED");
    // Reported: gone from the next answer.
    assert!(refused_lines(&mut f, m).0.is_empty());

    // ALGO: the formant budget.
    let mut f = fake(EngineType::Algo);
    let (_, mut m) = refused_lines(&mut f, 0);
    let formant = OpKind::Formant.code();
    let mut said = vec![];
    for op in Op::ALL {
        let a = ParamAddr::new(
            BlockRef::AlgoOp(op),
            chimera_engine::dsp::algo::params::AlgoOpParams::WAVE,
        );
        set(&mut f, &ed(a), &format!("#{formant}"));
        let (lines, n) = refused_lines(&mut f, m);
        m = n;
        said.extend(lines);
    }
    assert!(!said.is_empty(), "the budget never refused");
    for l in &said {
        assert!(
            l.starts_with("refused OP_") && l.ends_with(".WAVE \"FORMANT BUDGET FULL\""),
            "{l}"
        );
    }

    // DRUM: a 9th group, kit_groups' words, on the pad it was sent for.
    let mut f = fake(EngineType::Drum);
    let (_, m) = refused_lines(&mut f, 0);
    f.ui.step_toast(60_000);
    set(&mut f, "PAD3.CHOKE", "OFF");
    let said = toast(&mut f).unwrap();
    assert!(said.contains("GROUPS"), "{said}");
    let (lines, _) = refused_lines(&mut f, m);
    assert_eq!(lines, [format!("refused PAD3.CHOKE \"{said}\"")]);
}

/// At most four, the newest, oldest first.
#[test]
fn since_keeps_the_four_newest_refusals() {
    let mut f = streamed_at_deck();
    let (_, m) = refused_lines(&mut f, 0);
    // SCRUB, PING, SCRUB, PING, SCRUB: five, the first dropped.
    for i in 0..5 {
        if i % 2 == 0 {
            set(&mut f, "TAPE_ENG.MODEL", "#2");
        } else {
            set(&mut f, &ed(held::DIRECTION), "#3");
        }
    }
    let (lines, m) = refused_lines(&mut f, m);
    let scrub = "refused TAPE_ENG.MODEL \"SCRUB NEEDS RAM · STREAMED\"";
    let ping = format!(
        "refused {} \"PING NEEDS RAM · STREAMED\"",
        ed(held::DIRECTION)
    );
    assert_eq!(lines, [ping.as_str(), scrub, ping.as_str(), scrub]);
    assert!(refused_lines(&mut f, m).0.is_empty());
}

/// A set refused at `ERR` time says so in its own answer: since never
/// repeats it. Nor is a landed set, or a refused one to the value it has.
#[test]
fn since_says_only_the_turns_refusals() {
    let mut f = streamed_at_deck();
    let (_, m) = refused_lines(&mut f, 0);
    assert_eq!(
        ask(&mut f, "ed set TAPE_ENG.MODEL #99\n"),
        "ERR ed set TAPE_ENG.MODEL has no such option\n"
    );
    apply(&mut f);
    let (lines, m) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
    // A landed set.
    set(&mut f, "TAPE_ENG.MODEL", "#0");
    let (lines, _) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
}

/// A line that is no `since` answer keeps the refusals for the next.
#[test]
fn a_refusal_waits_for_a_since_answer() {
    let mut f = streamed_at_deck();
    let (_, m) = refused_lines(&mut f, 0);
    set(&mut f, "TAPE_ENG.MODEL", "#2");
    // An `ERR` answer (a malformed since) reports nothing and keeps them.
    assert!(ask(&mut f, "ed since x\n").starts_with("ERR "));
    let (lines, _) = refused_lines(&mut f, m);
    assert_eq!(lines.len(), 1, "{lines:?}");
}

/// A refusal is its Part's: a whole answer (another Part, or `since 0`)
/// says none and drops them, and Part 2's answers never carry Part 1's.
#[test]
fn a_refusal_stays_with_its_part() {
    let p2 = PartId::ALL[1];
    let mut f = fake(EngineType::Drum);
    screen::init_part_engine(&mut f.ui, p2, EngineType::Drum);
    // Part 1's PAD3 CHOKE refused, then Part 2 followed: its whole answer
    // has no refusal, nor has the diff after it.
    let (_, m) = refused_lines(&mut f, 0);
    set(&mut f, "PAD3.CHOKE", "OFF");
    assert_eq!(ask(&mut f, "ed part 2\n"), "OK\n");
    apply(&mut f);
    let (lines, m) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "Part 2's whole answer: {lines:?}");
    let (lines, m) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "Part 2's diff: {lines:?}");
    // Queued for Part 1, applied after Part 2 is followed: Part 1's still.
    assert_eq!(ask(&mut f, "ed part 1\n"), "OK\n");
    apply(&mut f);
    let (_, m) = refused_lines(&mut f, m);
    assert_eq!(ask(&mut f, "ed set PAD3.CHOKE OFF\n"), "OK queued\n");
    assert_eq!(ask(&mut f, "ed part 2\n"), "OK\n");
    apply(&mut f);
    let (lines, m) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "Part 2 after Part 1's set: {lines:?}");
    let (lines, _) = refused_lines(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
    // `since 0` is whole: a refusal before it is not said.
    let mut f = fake(EngineType::Drum);
    let (_, _) = refused_lines(&mut f, 0);
    set(&mut f, "PAD3.CHOKE", "OFF");
    let (lines, m) = refused_lines(&mut f, 0);
    assert!(lines.is_empty(), "since 0: {lines:?}");
    assert!(refused_lines(&mut f, m).0.is_empty());
}

#[test]
fn a_landed_ed_set_is_silent_and_another_cell_leaves_the_hold() {
    use chimera_core::dsp::tape_engine::TapeEngineParams;
    use chimera_hal::EncoderId;
    let mut f = streamed_at_deck();
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::D, 1));
    assert!(f.ui.held_choice().is_some(), "DIRECTION held");
    // AGE lands, says nothing, and DIRECTION's hold stays.
    let age = ed(ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::AGE));
    let (now, _) = get(&mut f, &age);
    f.ui.step_toast(60_000);
    set(&mut f, &age, &format!("{now}"));
    assert_eq!(toast(&mut f), None, "a landed set is silent");
    assert!(f.ui.held_choice().is_some(), "another cell leaves the hold");
    // So does a refused set on another cell: MODEL's SCRUB.
    set(&mut f, "TAPE_ENG.MODEL", "#2");
    assert!(
        f.ui.held_choice().is_some(),
        "a refusal elsewhere leaves it"
    );
    // ALGO: a WAVE that lands says nothing either.
    let mut f = fake(EngineType::Algo);
    let wave = ed(ParamAddr::new(
        BlockRef::AlgoOp(Op::ALL[0]),
        chimera_engine::dsp::algo::params::AlgoOpParams::WAVE,
    ));
    f.ui.step_toast(60_000);
    set(&mut f, &wave, "#1");
    assert_eq!(toast(&mut f), None);
}

#[test]
fn a_refused_set_to_the_value_it_has_changes_nothing() {
    use chimera_core::block::Block;
    use chimera_core::dsp::tape_engine::TapeEngineParams;
    use chimera_hal::EncoderId;
    // DIRECTION already PING-PONG on a streamed source: refused, but no
    // change. The box's held REVERSE stays and nothing is said.
    let mut f = streamed_at_deck();
    f.ui.project_mut()
        .edit_part(P1)
        .sound
        .params
        .tape
        .set(TapeEngineParams::MODE, 3.0);
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::D, -2));
    let held = f.ui.held_choice().map(|s| s.held);
    assert!(held.is_some(), "REVERSE held");
    f.ui.step_toast(60_000);
    set(&mut f, &ed(held::DIRECTION), "#3");
    assert_eq!(f.ui.held_choice().map(|s| s.held), held, "the hold stays");
    assert_eq!(toast(&mut f), None, "nothing to say");
}

/// INIT DRUM, pad 3's CHOKE held at OFF on the box (a 9th group), as
/// drum_groups_door_test's.
fn choke_held() -> Fake {
    use chimera_hal::{ButtonId, EncoderId};
    let mut f = Fake::new();
    screen::stack_map(&mut f.ui);
    screen::load_init(&mut f.ui, EngineType::Drum);
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::A, 1));
    screen::feed(&mut f.ui, screen::Input::turn(EncoderId::A, 1));
    assert_eq!(f.ui.selected_pad(P1), pad(3));
    screen::tap(&mut f.ui, ButtonId::Edit);
    for d in [1, -1, -1] {
        screen::feed(&mut f.ui, screen::Input::turn(EncoderId::B, d));
    }
    assert!(f.ui.held_choice().is_some(), "OFF held on pad 3");
    f
}

#[test]
fn an_ed_set_on_another_pads_choke_leaves_the_hold() {
    let mut f = choke_held();
    // Pad 1's CHOKE to the OFF it has: pad 3's hold stays.
    set(&mut f, "PAD1.CHOKE", "OFF");
    assert!(f.ui.held_choice().is_some(), "a no-op on pad 1");
    // Pad 6 (B with pad 7) to OFF: refused, still pad 3's.
    set(&mut f, "PAD6.CHOKE", "OFF");
    assert!(f.ui.held_choice().is_some(), "a refusal on pad 6");
    assert_eq!(f.ui.selected_pad(P1), pad(3));
    // Pad 3's own refused set ends it.
    set(&mut f, "PAD3.CHOKE", "OFF");
    assert!(f.ui.held_choice().is_none(), "pad 3's set ends it");
}

/// The editor's engine list is the firmware's: every engine the box plays,
/// in its order (`EngineType::PLAYABLE`, which holds all of `ALL`), for the
/// Elm test that `Unit.allEngines` names each and no other.
#[test]
fn writes_editor_engines_fixture() {
    for e in EngineType::ALL {
        assert!(EngineType::PLAYABLE.contains(&e), "{e:?} plays");
    }
    let text: String = EngineType::PLAYABLE
        .iter()
        .map(|e| format!("{}\n", e.disk_ident_for_test()))
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/engines.txt");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == text,
        "the engines changed: BLESS=1 rewrites {}",
        path.display()
    );
}
