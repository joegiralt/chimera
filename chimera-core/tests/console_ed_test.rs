mod common;
use common::console_fake::{Fake, ask};

use chimera_core::console::EdAddr;
use chimera_engine::addr::{BlockRef, Op, ParamAddr};
use chimera_engine::dsp::modulator::{EnvSlot, LfoSlot};

fn algo_blocks() -> Vec<BlockRef> {
    let mut v = vec![BlockRef::Algo, BlockRef::Filter, BlockRef::VoiceFx];
    v.extend(
        [Op::A, Op::B, Op::C, Op::D, Op::E, Op::F]
            .into_iter()
            .map(BlockRef::AlgoOp),
    );
    v.extend(
        [EnvSlot::Env1, EnvSlot::Env2, EnvSlot::Env3]
            .into_iter()
            .map(BlockRef::Env),
    );
    v.extend(
        [LfoSlot::Lfo1, LfoSlot::Lfo2, LfoSlot::Lfo3]
            .into_iter()
            .map(BlockRef::Lfo),
    );
    v.extend([BlockRef::Out, BlockRef::Pitch]);
    v
}

#[test]
fn addr_round_trips_every_algo_param() {
    for b in algo_blocks() {
        for s in b.specs() {
            let a = EdAddr(ParamAddr::new(b, s.id));
            let text = a.to_string();
            assert_eq!(EdAddr::parse(text.as_bytes()), Some(a), "{text}");
        }
    }
}

#[test]
fn addr_refuses_unknown_block_and_ident() {
    for bad in ["OP_G.CRSE", "OP_A.NOPE", "CRSE", "OP_A.", ".CRSE", ""] {
        assert_eq!(EdAddr::parse(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn hello_reports_protocol_build_part_engine() {
    let mut f = Fake::new();
    let mut build = String::new();
    chimera_core::ui::about_page::write_build_rev(&mut build).unwrap();
    let build = build.replace(' ', "-");
    let got = ask(&mut f, "ed hello\n");
    assert_eq!(got, format!("ed hello 2 {build} part 1 ALGO\nOK\n"));
    // One word for the build: a fixed word count.
    assert_eq!(got.lines().next().unwrap().split(' ').count(), 7, "{got}");
}

#[test]
fn set_refuses_non_finite_and_long_names() {
    let mut f = Fake::new();
    for line in [
        "ed set OP_C.CRSE inf\n",
        "ed set OP_C.CRSE -INF\n",
        "ed set OP_C.CRSE NaN\n",
        "ed set OP_C.CRSE +infinity\n",
        "ed set OP_C.CRSE ABCDEFGHIJKLMNOPQ\n",
    ] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed takes "), "{line}: {got}");
    }
    // Sixteen characters is a name; decimals are numbers.
    for line in [
        "ed set OP_C.CRSE ABCDEFGHIJKLMNOP\n",
        "ed set OP_C.CRSE -12.5\n",
        "ed set OP_C.CRSE .5\n",
    ] {
        assert_eq!(
            ask(&mut f, line),
            "ERR ed set is not in this build\n",
            "{line}"
        );
    }
}

#[test]
fn ed_without_sub_is_refused() {
    let mut f = Fake::new();
    for line in ["ed\n", "ed bogus\n", "ed hello now\n", "ed get OP_G.CRSE\n"] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed takes "), "{line}: {got}");
    }
}

#[test]
fn the_rest_wait_for_their_tasks() {
    let mut f = Fake::new();
    for (line, sub) in [
        ("ed get OP_C.CRSE\n", "get"),
        ("ed set OP_C.CRSE 3.5\n", "set"),
        ("ed watch\n", "watch"),
        ("ed unwatch\n", "unwatch"),
    ] {
        assert_eq!(
            ask(&mut f, line),
            format!("ERR ed {sub} is not in this build\n"),
            "{line}"
        );
    }
}

// ---- describe ----

use chimera_core::console::{
    ed_pages, env_variant, filter_variant, fx_variant, lfo_variant, op_variant,
};
use chimera_core::ui::block_def::{ChainDef2, SlotBinding};
use chimera_core::ui::block_registry::{ALGO_CHAIN, ALGO_FORMANT_CHAIN};
use chimera_core::ui::view::{EnvKind, LfoKind};
use chimera_core::ui::{filter_panel, fx_panel, mod_panel, op_page};
use chimera_engine::addr::BlockRef as B;
use chimera_engine::dsp::algo::kinds::OpKind;
use chimera_engine::dsp::filter::FilterKind;
use chimera_engine::dsp::modulator::{EnvForm, EnvSpeed, Func, LfoForm};
use chimera_engine::dsp::voice_fx::params::FxType;
use chimera_engine::params::EngineType;
use std::collections::BTreeSet;

/// What one answer can carry. The console has no measured bytes/s, so the
/// rate is its documented floor: the CDC bulk IN endpoint is 64 B and full
/// speed polls it once a 1 ms frame (`chimera-board/src/usb.rs`, `DRAIN_MS`),
/// so 64,000 B/s. An answer is cut at `ANSWER_MS` (`console::ANSWER_MS`),
/// and half that is the margin.
const BULK_PACKET_BYTES: usize = 64;
const BULK_PACKETS_PER_S: usize = 1000;
const ANSWER_BYTES_MAX: usize =
    BULK_PACKET_BYTES * BULK_PACKETS_PER_S * chimera_core::console::ANSWER_MS as usize / 1000 / 2;

fn page_text(f: &mut Fake, n: u8) -> String {
    ask(f, &format!("ed describe ALGO {n}\n"))
}

fn all_pages(f: &mut Fake) -> Vec<String> {
    (0..ed_pages(EngineType::Algo))
        .map(|n| page_text(f, n))
        .collect()
}

fn chain_pages(c: &ChainDef2) -> usize {
    c.blocks.iter().map(|b| 1 + b.sub_pages.len()).sum()
}

#[test]
fn describe_walks_every_algo_page() {
    let mut f = Fake::new();
    let n = ed_pages(EngineType::Algo);
    assert_eq!(usize::from(n), chain_pages(&ALGO_FORMANT_CHAIN));
    assert_eq!(
        chain_pages(&ALGO_FORMANT_CHAIN),
        chain_pages(&ALGO_CHAIN) + 1
    );
    for i in 0..n {
        let t = page_text(&mut f, i);
        assert!(t.starts_with(&format!("page {i} ")), "{t}");
        assert!(
            t.lines().next().unwrap().ends_with(&format!(" of {n}")),
            "{t}"
        );
        assert!(t.ends_with("\nOK\n"), "{t}");
    }
    assert!(page_text(&mut f, n).starts_with("ERR ed describe ALGO has"));
    assert!(ask(&mut f, "ed describe VA 0\n").starts_with("ERR ed describe VA has 0"));
}

fn addrs(texts: &[String]) -> BTreeSet<String> {
    texts
        .iter()
        .flat_map(|t| t.lines())
        .filter_map(|l| l.strip_prefix("param "))
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect()
}

fn add(set: &mut BTreeSet<String>, b: B, id: chimera_engine::block::ParamId) {
    set.insert(EdAddr(ParamAddr::new(b, id)).to_string());
}

#[test]
fn describe_covers_every_param_every_variant() {
    let mut f = Fake::new();
    let got = addrs(&all_pages(&mut f));
    let mut want = BTreeSet::new();
    let mut routes = 0;
    for b in ALGO_FORMANT_CHAIN.blocks {
        for def in std::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
            for s in &def.params {
                match s.binding {
                    SlotBinding::Empty | SlotBinding::SelectOp => {}
                    SlotBinding::Param(a) => add(&mut want, a.block, a.param),
                    SlotBinding::FilterPanel(k) => {
                        for kind in FilterKind::BUILT {
                            match filter_panel::knob(kind, k).map(|n| n.target) {
                                Some(filter_panel::PanelTarget::Filter(id)) => {
                                    add(&mut want, B::Filter, id)
                                }
                                Some(_) => routes += 1,
                                None => {}
                            }
                        }
                    }
                    SlotBinding::FxPanel(k) => {
                        for t in FxType::LIST {
                            if let Some(c) = fx_panel::cells(t) {
                                add(&mut want, B::VoiceFx, c[usize::from(k)].id);
                            }
                        }
                    }
                    SlotBinding::EnvPanel(slot, k) => {
                        for e in all_env_kinds() {
                            if let Some(Some(mod_panel::PanelSlot::Param { id, .. })) =
                                mod_panel::env_panel(e).slots.get(usize::from(k))
                            {
                                add(&mut want, B::Env(slot), *id);
                            }
                        }
                    }
                    SlotBinding::LfoPanel(slot, k) => {
                        for l in all_lfo_kinds() {
                            if let Some(Some(mod_panel::PanelSlot::Param { id, .. })) =
                                mod_panel::lfo_panel(l).slots.get(usize::from(k))
                            {
                                add(&mut want, B::Lfo(slot), *id);
                            }
                        }
                    }
                    SlotBinding::OpPanel(op, k) => {
                        for kind in all_op_kinds() {
                            if let Some((id, _)) = op_page::page_cells(kind)[usize::from(k)] {
                                add(&mut want, B::AlgoOp(op), id);
                            }
                        }
                    }
                    other => panic!("an ALGO page has a {other:?} the schema doesn't know"),
                }
            }
        }
    }
    assert_eq!(got, want);
    assert!(routes > 0);
    let flt = all_pages(&mut f).join("");
    assert!(flt.contains("route \"ENV AMT\" next-slice"));
}

fn all_env_kinds() -> Vec<EnvKind> {
    let mut v: Vec<EnvKind> = EnvSpeed::ALL.into_iter().map(EnvKind::A).collect();
    for f in EnvForm::ALL {
        v.push(EnvKind::B(Func::Env(f)));
        v.push(EnvKind::B(Func::Burst(f)));
    }
    v.extend(LfoForm::ALL.into_iter().map(|f| EnvKind::B(Func::Lfo(f))));
    v
}

fn all_lfo_kinds() -> Vec<LfoKind> {
    let mut v = vec![LfoKind::Classic];
    v.extend(LfoForm::ALL.into_iter().map(LfoKind::Func));
    v
}

fn all_op_kinds() -> Vec<OpKind> {
    (0..=17).map(|c| OpKind::from_code(c).unwrap()).collect()
}

#[test]
fn every_variant_of_every_kind_is_a_described_panel() {
    let mut f = Fake::new();
    let text = all_pages(&mut f).join("");
    let has = |name: &str, v: &str| text.contains(&format!("panel {name} {v}\n"));
    for k in FilterKind::BUILT {
        assert!(has("FILTER", filter_variant(k)), "{k:?}");
    }
    for t in FxType::LIST {
        assert!(has("FX", fx_variant(t)), "{t:?}");
    }
    for k in all_env_kinds() {
        assert!(has("ENV2", env_variant(k)), "{k:?}");
    }
    for k in all_lfo_kinds() {
        assert!(has("LFO3", lfo_variant(k)), "{k:?}");
    }
    for k in all_op_kinds() {
        assert!(has("OP6", op_variant(k)), "{k:?}");
    }
    // Same-named variants are the same panel: no two kinds share a name
    // with different cells.
    for a in all_env_kinds() {
        for b in all_env_kinds() {
            let same = std::ptr::eq(mod_panel::env_panel(a), mod_panel::env_panel(b));
            assert_eq!(same, env_variant(a) == env_variant(b), "{a:?} {b:?}");
        }
    }
    for a in all_lfo_kinds() {
        for b in all_lfo_kinds() {
            let same = std::ptr::eq(mod_panel::lfo_panel(a), mod_panel::lfo_panel(b));
            assert_eq!(same, lfo_variant(a) == lfo_variant(b), "{a:?} {b:?}");
        }
    }
}

#[test]
fn filter_mode_options_are_per_kind() {
    let mut f = Fake::new();
    let flt = all_pages(&mut f)
        .into_iter()
        .find(|t| t.contains(" FLT of "))
        .unwrap();
    let section = |kind: &str| {
        let head = format!("panel FILTER {kind}\n");
        let at = flt.find(&head).unwrap();
        let rest = &flt[at + head.len()..];
        rest[..rest.find("panel ").unwrap_or(rest.len())].to_string()
    };
    let opts = |s: &str| {
        s.lines()
            .filter(|l| l.starts_with("opt FILTER.MODE "))
            .count()
    };
    assert!(opts(&section("CLEAN")) > 1);
    assert_eq!(opts(&section("2040")), 0, "the 2040 has no MODE cell");
    assert_ne!(section("CLEAN"), section("LADDER"));
}

#[test]
fn describe_is_static() {
    use chimera_core::block::Block;
    use chimera_core::params::EnvParams;
    use chimera_core::preset::Sound;
    use chimera_core::project::PartId;
    let mut f = Fake::new();
    let before = all_pages(&mut f);
    let mut s = Sound::init(EngineType::Algo);
    s.params.filter.set_kind(FilterKind::Sem);
    s.params.fx.set_type(FxType::Fold);
    s.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    s.params.algo.try_set_kind(1, OpKind::Noise).unwrap();
    s.params.envelopes[0].set(EnvParams::TYPE, 1.0);
    let was = chimera_core::ui::nav::PartChain::of(&f.ui.project().part(PartId::ALL[0]).sound);
    **f.ui.project_mut().edit_part(PartId::ALL[0]).sound = s;
    f.ui.replaced_for_test(was);
    assert_eq!(all_pages(&mut f), before);
}

#[test]
fn describe_fits_the_answer_clock() {
    let mut f = Fake::new();
    let sizes: Vec<usize> = all_pages(&mut f).iter().map(String::len).collect();
    let (at, max) = sizes
        .iter()
        .copied()
        .enumerate()
        .max_by_key(|&(_, n)| n)
        .unwrap();
    assert_eq!(ANSWER_BYTES_MAX, 32_000);
    assert!(max < ANSWER_BYTES_MAX, "page {at}: {max} B");
}

#[test]
fn writes_editor_schema_fixture() {
    let mut f = Fake::new();
    let text = all_pages(&mut f).join("");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../editor/tests/fixtures/algo-schema.txt");
    if std::env::var_os("BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == text,
        "the schema changed: BLESS=1 rewrites {}",
        path.display()
    );
}
