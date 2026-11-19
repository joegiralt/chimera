//! `ed` on the mixer (ADR 0289, protocol 4's slice 1): every Part's mix
//! and sends and the buses addressed, `ed describe MIXER`, the reverb's
//! panels, `param` flags, `ed schema`'s hash, and `since`'s MIXER section.
mod common;
use common::console_fake::{Fake, ask};

use std::collections::BTreeSet;

use chimera_core::console::{
    ANSWER_BYTES_MAX, BusBlock, ED_MIXER_SLOTS, EdAddr, EdBlock, PartBlock, Round, Schema,
    ed_schema_pages, ed_slots_used, reverb_variant,
};
use chimera_core::project::PartId;
use chimera_core::ui::block_def::{PageAt, slot_addr};
use chimera_core::ui::block_registry::MIXER_CHANNEL_CHAIN;
use chimera_core::ui::nav::{Location, MixPage};
use chimera_engine::addr::BlockRef;
use chimera_engine::dsp::reverb::{ReverbParams, ReverbType};
use chimera_engine::params::OutParams;
use chimera_engine::part::PartParams;

const P1: PartId = PartId::ALL[0];
const P3: PartId = PartId::ALL[2];

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

fn mixer_pages(f: &mut Fake) -> Vec<String> {
    (0..ed_schema_pages(Schema::Mixer))
        .map(|n| ask(f, &format!("ed describe MIXER {n}\n")))
        .collect()
}

/// Every `param` line's address.
fn listed(texts: &[String]) -> BTreeSet<String> {
    texts
        .iter()
        .flat_map(|t| t.lines())
        .filter_map(|l| l.strip_prefix("param "))
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

// ---- addresses ----

#[test]
fn part_and_bus_addresses_parse_and_bad_ones_are_refused() {
    assert_eq!(
        EdAddr::parse(b"PART3.LEVEL").map(|a| a.at()),
        Some(EdBlock::Part(P3, PartBlock::Mix))
    );
    assert_eq!(
        EdAddr::parse(b"PART6_SND.SEND2").map(|a| a.at()),
        Some(EdBlock::Part(PartId::ALL[5], PartBlock::Sends))
    );
    assert_eq!(
        EdAddr::parse(b"CHORUS.MIX").map(|a| a.at()),
        Some(EdBlock::Bus(BusBlock::Chorus))
    );
    for bad in [
        // The active Part is never addressed.
        "PART.LEVEL",
        "PART0.LEVEL",
        "PART7.LEVEL",
        "PART03.LEVEL",
        "PART3_SN.SEND1",
        // A Part's sends hold SEND1–SEND3 alone.
        "PART3_SND.VOLUME",
        "PART3_SND.LEVEL",
        "PART3.SEND1",
    ] {
        assert_eq!(EdAddr::parse(bad.as_bytes()), None, "{bad}");
    }
}

// ---- describe ----

#[test]
fn the_mixer_walks_its_shared_pages_then_each_parts() {
    let mut f = Fake::new();
    let texts = mixer_pages(&mut f);
    let labels: Vec<String> = texts
        .iter()
        .map(|t| {
            t.lines()
                .next()
                .unwrap()
                .split(' ')
                .nth(2)
                .unwrap()
                .to_string()
        })
        .collect();
    let shared = ["CHR", "DLY", "CHAR", "GRAIN", "REV", "MORE", "MOR2"];
    assert_eq!(&labels[..shared.len()], &shared);
    let per_part = ["PRT", "VOI", "EQ", "SND"];
    let rounds = &labels[labels.len() - per_part.len() * PartId::ALL.len()..];
    for (k, p) in PartId::ALL.into_iter().enumerate() {
        for (i, short) in per_part.iter().enumerate() {
            assert_eq!(
                rounds[k * per_part.len() + i],
                format!("PART{}_{short}", p.number())
            );
        }
    }
    let n = ed_schema_pages(Schema::Mixer);
    assert!(texts[0].starts_with(&format!("page 0 CHR of {n}\n")));
    assert_eq!(
        ask(&mut f, &format!("ed describe MIXER {n}\n")),
        format!("ERR ed describe MIXER has {n} pages\n")
    );
}

#[test]
fn every_reverb_type_is_a_panel_with_its_cells() {
    let mut f = Fake::new();
    let texts = mixer_pages(&mut f).join("");
    for t in ReverbType::ALL {
        assert!(
            texts.contains(&format!("panel REVERB {}\n", reverb_variant(t))),
            "{t:?}"
        );
    }
    // HALL's MORE has its own cells, under its panel.
    let more = texts
        .split("page ")
        .find(|p| p.contains(" MORE of "))
        .unwrap();
    let hall = more.split("panel REVERB HALL\n").nth(1).unwrap();
    assert!(hall.starts_with("param REVERB."), "{hall}");
}

#[test]
fn param_lines_carry_their_flags() {
    let mut f = Fake::new();
    let texts = mixer_pages(&mut f).join("");
    let flags = |addr: &str| {
        texts
            .lines()
            .find(|l| l.starts_with(&format!("param {addr} ")))
            .unwrap_or_else(|| panic!("{addr}"))
            .rsplit(' ')
            .next()
            .unwrap()
            .to_string()
    };
    // LEVEL is mappable; CH isn't; no mixer cell is a mod destination.
    assert_eq!(flags("PART3.LEVEL"), "m");
    assert_eq!(flags("PART3.CH"), "-");
    assert_eq!(flags("CHORUS.MIX"), "m");
    assert_eq!(flags("CHORUS.MODE"), "-");
    // An engine's: CUTOFF is both.
    let algo: String =
        (0..chimera_core::console::ed_pages(chimera_engine::params::EngineType::Algo))
            .map(|n| ask(&mut f, &format!("ed describe ALGO {n}\n")))
            .collect();
    let cutoff = algo
        .lines()
        .find(|l| l.starts_with("param FILTER.CUTOFF "))
        .unwrap();
    assert!(cutoff.ends_with(" md"), "{cutoff}");
}

/// Each mixer page as the box shows it, for Part `p` (its own) or the
/// FX pages entered from it.
fn box_cells(f: &mut Fake, p: PartId, listed: &BTreeSet<String>) -> usize {
    let mut n = 0;
    let chain = &MIXER_CHANNEL_CHAIN;
    for node in 0..chain.blocks.len() {
        let mut sub = 0;
        while let Some(at) = chain.page(node, sub) {
            sub += 1;
            let loc = match (node, at.sub()) {
                (0, 0) => Location::mixer(p, MixPage::Part),
                (0, 1) => Location::mixer(p, MixPage::Voices),
                (0, 2) => Location::mixer(p, MixPage::Eq),
                (1, 0) => Location::mixer(p, MixPage::Sends),
                _ => Location::fx(p, PageAt::of(at.node(), at.sub())),
            };
            f.ui.go_for_test(loc);
            let (def, ctx) = (f.ui.page_def(), f.ui.slot_ctx_for_test());
            let round = if node < 2 {
                Round::Part(p)
            } else {
                Round::Shared
            };
            for i in 0..6 {
                let Some(a) = slot_addr(def, i, &ctx) else {
                    continue;
                };
                let ed = EdAddr::shown(a, round).unwrap().to_string();
                assert!(listed.contains(&ed), "{ed} on {}", def.short);
                let want = f.ui.cell_text_for_test(i);
                let (_, text) = get(f, &ed);
                assert_eq!(text, want.as_str(), "{ed} on {}", def.short);
                n += 1;
            }
        }
    }
    n
}

#[test]
fn every_mixer_cell_in_every_reverb_type_is_described_and_reads_as_the_box() {
    let mut f = Fake::new();
    let listed = listed(&mixer_pages(&mut f));
    let mut n = 0;
    for t in ReverbType::ALL {
        f.ui.edit_to(
            P1,
            chimera_engine::addr::ParamAddr::new(BlockRef::Reverb, ReverbParams::TYPE),
            t.index() as f32,
        );
        for p in [P1, P3] {
            n += box_cells(&mut f, p, &listed);
        }
    }
    assert!(n > 300, "{n}");
}

// ---- set and get ----

#[test]
fn a_parts_set_lands_on_its_part_and_leaves_the_box() {
    let mut f = Fake::new();
    let at = f.ui.page_def().id;
    set(&mut f, "PART3.LEVEL", "0.25");
    set(&mut f, "PART4_SND.SEND2", "0.75");
    set(&mut f, "CHORUS.MIX", "0.5");
    let read = |f: &Fake, p: PartId, b, id| f.ui.project().read_part(p).block(b).unwrap().get(id);
    assert_eq!(read(&f, P3, BlockRef::Part, PartParams::LEVEL), 0.25);
    assert_ne!(read(&f, P1, BlockRef::Part, PartParams::LEVEL), 0.25);
    let p4 = PartId::ALL[3];
    assert_eq!(read(&f, p4, BlockRef::Out, OutParams::SEND2), 0.75);
    assert_ne!(read(&f, P1, BlockRef::Out, OutParams::SEND2), 0.75);
    assert_eq!(get(&mut f, "PART3.LEVEL").0, 0.25);
    assert_eq!(get(&mut f, "PART4_SND.SEND2").0, 0.75);
    assert_eq!(get(&mut f, "CHORUS.MIX").0, 0.5);
    assert_eq!(f.ui.active_part, P1, "the box follows no set");
    assert_eq!(f.ui.page_def().id, at);
}

#[test]
fn reserves_past_the_pool_are_refused_as_the_box_refuses_them() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    set(&mut f, "PART2.RESERVE", "8");
    assert_eq!(get(&mut f, "PART2.RESERVE").0, 8.0);
    // Another Part's reserve past the pool's 8 writes nothing.
    set(&mut f, "PART3.RESERVE", "2");
    assert_eq!(get(&mut f, "PART3.RESERVE").0, 0.0);
}

// ---- since ----

/// `ed since <n>`'s lines, its new number, whether more is owed, and its
/// bytes.
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32, bool, usize) {
    let got = ask(f, &format!("ed since {n}\n"));
    let lines: Vec<String> = got.lines().map(str::to_string).collect();
    let last = lines.last().unwrap().clone();
    let rest = last.strip_prefix("OK ").unwrap_or_else(|| panic!("{got}"));
    let (m, more) = match rest.strip_suffix(" more") {
        Some(m) => (m, true),
        None => (rest, false),
    };
    (lines, m.parse().unwrap(), more, got.len())
}

#[test]
fn since_answers_the_mixer_whole_then_its_diffs() {
    let mut f = Fake::new();
    let (lines, m, more, bytes) = since(&mut f, 0);
    assert!(!more);
    assert!(bytes <= ANSWER_BYTES_MAX);
    assert_eq!(lines[0], "", "a whole answer opens with the empty line");
    let mixer = lines.iter().position(|l| l == "mixer").expect("the header");
    assert_eq!(lines[mixer + 1], "panel REVERB QUAD");
    for p in PartId::ALL {
        let level = format!("val PART{}.LEVEL ", p.number());
        assert!(lines.iter().any(|l| l.starts_with(&level)), "{level}");
    }
    assert!(lines.iter().any(|l| l.starts_with("val CHORUS.MIX ")));
    // Nothing moved: an empty diff.
    let (lines, m2, _, _) = since(&mut f, m);
    assert_eq!(lines, vec![format!("OK {m2}")]);
    // One Part's level: one line.
    set(&mut f, "PART5.LEVEL", "0.5");
    let (lines, m3, _, _) = since(&mut f, m2);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("val PART5.LEVEL 0.5 "), "{lines:?}");
    // A reverb TYPE relabels the panels: the mixer whole again, not the
    // Part.
    set(&mut f, "REVERB.TYPE", "HALL");
    let (lines, _, _, _) = since(&mut f, m3);
    assert_eq!(lines[0], "");
    assert!(lines.contains(&"mixer".to_string()));
    assert!(lines.contains(&"panel REVERB HALL".to_string()));
    assert!(!lines.iter().any(|l| l.starts_with("part ")));
}

#[test]
fn the_mixer_copy_holds_its_slots() {
    assert_eq!(ed_slots_used(Schema::Mixer), ED_MIXER_SLOTS);
}

// ---- the schema hash ----

/// FNV-1a 64, the unit's.
fn fnv64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &x| {
        (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn the_schema_hash_is_describes_bytes_and_the_same_every_boot() {
    for s in Schema::ALL {
        let mut f = Fake::new();
        let n = ed_schema_pages(s);
        let text: String = (0..n)
            .map(|i| {
                let a = ask(&mut f, &format!("ed describe {} {i}\n", s.token()));
                a.strip_suffix("OK\n").unwrap().to_string()
            })
            .collect();
        assert!(text.len() > 300 * usize::from(n), "{s:?} {}", text.len());
        let want = format!(
            "schema {} {n} {:016x}\nOK\n",
            s.token(),
            fnv64(text.as_bytes())
        );
        assert_eq!(ask(&mut f, &format!("ed schema {}\n", s.token())), want);
        // Kept for the boot, and another boot answers the same.
        assert_eq!(ask(&mut f, &format!("ed schema {}\n", s.token())), want);
        let mut g = Fake::new();
        assert_eq!(ask(&mut g, &format!("ed schema {}\n", s.token())), want);
        // One byte of a label moves it.
        let mut moved = text.into_bytes();
        moved[10] ^= 1;
        assert_ne!(fnv64(&moved), fnv64(&moved[..0]));
        assert_ne!(
            format!("{:016x}", fnv64(&moved)),
            want.split(' ').nth(3).unwrap().trim_end_matches("\nOK\n")
        );
    }
    let mut f = Fake::new();
    assert_eq!(
        ask(&mut f, "ed schema NOPE\n"),
        "ERR ed takes hello, schema, describe, get, set, since or part\n"
    );
}

/// The editor's fixture of the mixer's schema, as the default build
/// describes it (`master-tape` adds REEL).
#[cfg(not(feature = "master-tape"))]
#[test]
fn mixer_fixture() {
    let mut f = Fake::new();
    let text = mixer_pages(&mut f).join("");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/mixer-schema.txt");
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

/// Every `param` line's address, max, min and options.
fn params(texts: &[String]) -> Vec<(String, f32, f32, Vec<usize>)> {
    let mut out: Vec<(String, f32, f32, Vec<usize>)> = Vec::new();
    for l in texts.iter().flat_map(|t| t.lines()) {
        if let Some(rest) = l.strip_prefix("param ") {
            let addr = rest.split(' ').next().unwrap().to_string();
            let tail = rest.rsplit_once("\" ").unwrap().1;
            let w: Vec<&str> = tail.split(' ').collect();
            out.push((addr, w[3].parse().unwrap(), w[2].parse().unwrap(), vec![]));
        } else if let Some(rest) = l.strip_prefix("opt ") {
            let mut w = rest.split(' ');
            let addr = w.next().unwrap();
            let i: usize = w.next().unwrap().parse().unwrap();
            if let Some(p) = out.iter_mut().rev().find(|p| p.0 == addr) {
                p.3.push(i);
            }
        }
    }
    out
}

#[test]
fn every_mixer_param_round_trips_on_a_part_the_box_doesnt_follow() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    let texts = mixer_pages(&mut f);
    let mut n = 0;
    let mut seen = BTreeSet::new();
    for (addr, max, min, opts) in params(&texts) {
        // Part 3's and the buses; RESERVE and MAX VOICES refuse past the
        // pool (their own test), the reverb's TYPE relabels the rest.
        let ours = addr.starts_with("PART3.")
            || addr.starts_with("PART3_SND.")
            || !addr.starts_with("PART");
        if !ours
            || addr.ends_with(".RESERVE")
            || addr.ends_with(".MAX_VOICES")
            || !seen.insert(addr.clone())
        {
            continue;
        }
        let targets: Vec<(String, f32)> = match opts.last() {
            Some(&last) => vec![(format!("#{last}"), last as f32), ("#0".into(), 0.0)],
            None => vec![(max.to_string(), max), (min.to_string(), min)],
        };
        for (text, want) in targets {
            set(&mut f, &addr, &text);
            let (got, _) = get(&mut f, &addr);
            assert!((got - want).abs() < 1e-3, "{addr} {text}: {got}");
            n += 1;
        }
    }
    assert!(n > 60, "{n}");
    assert_eq!(f.ui.active_part, P1);
}

#[test]
fn the_mixers_worst_whole_section_fits_the_bound() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut f = Fake::new();
    let texts = mixer_pages(&mut f);
    // Every value at its max: the longest texts most cells read.
    for (addr, max, _, opts) in params(&texts) {
        if addr.ends_with(".RESERVE") || addr.ends_with(".MAX_VOICES") || addr == "REVERB.TYPE" {
            continue;
        }
        let text = match opts.last() {
            Some(&last) => format!("#{last}"),
            None => max.to_string(),
        };
        let _ = ask(&mut f, &format!("ed set {addr} {text}\n"));
        apply(&mut f);
    }
    for t in ReverbType::ALL {
        set(&mut f, "REVERB.TYPE", &format!("#{}", t.index()));
        let (lines, _, more, _) = since(&mut f, 0);
        assert!(!more);
        let mixer: usize = lines
            .iter()
            .skip_while(|l| *l != "mixer")
            .map(|l| l.len() + 1)
            .sum();
        assert!(
            mixer > 3000 && mixer < ANSWER_BYTES_MAX / 2,
            "{t:?}: {mixer} B"
        );
        println!("{t:?}: the mixer whole is {mixer} B");
    }
}
