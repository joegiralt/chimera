mod common;
mod screen;
use common::console_fake::{Fake, ask};

use chimera_core::console::{EdAddr, Schema};
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
            let a = EdAddr::sound(ParamAddr::new(b, s.id)).unwrap();
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
    assert_eq!(got, format!("ed hello 4 {build} part 1 ALGO\nOK\n"));
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
    // Sixteen characters is a name, a decimal a number: they parse, and
    // the answer is the value's own (a choice's name, a param's number).
    for line in [
        "ed set OP_C.CRSE ABCDEFGHIJKLMNOP\n",
        "ed set OP_C.CRSE -12.5\n",
        "ed set FILTER.CUTOFF ABCDEFGHIJKLMNOP\n",
    ] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed set "), "{line}: {got}");
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
fn ed_watch_is_unknown() {
    let mut f = Fake::new();
    for line in ["ed watch\n", "ed unwatch\n"] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed takes "), "{line}: {got}");
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
use chimera_core::console::ANSWER_BYTES_MAX;

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
    let va = ed_pages(EngineType::Va);
    assert!(va > 0);
    assert!(ask(&mut f, &format!("ed describe VA {va}\n")).starts_with("ERR ed describe VA has"));
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
    set.insert(EdAddr::sound(ParamAddr::new(b, id)).unwrap().to_string());
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
    assert!(flt.contains("route \"ENV AMT\" ENV1:FILTER.CUTOFF route"));
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
        .join("tests/fixtures/editor/algo-schema.txt");
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

// ---- get ----

use chimera_core::project::PartId;
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::nav::Location;
use chimera_engine::addr::Blocks;

/// `val <addr> <normalized> <stored> "<text>"` then `OK`.
fn parse_val(got: &str) -> (f32, f32, String) {
    let line = got.lines().next().unwrap();
    let rest = line.strip_prefix("val ").unwrap_or_else(|| panic!("{got}"));
    assert_eq!(got.lines().nth(1), Some("OK"), "{got}");
    let mut w = rest.splitn(4, ' ');
    w.next().unwrap();
    let n = w.next().unwrap().parse().unwrap();
    let s = w.next().unwrap().parse().unwrap();
    let t = w.next().unwrap();
    (
        n,
        s,
        t.strip_prefix('"')
            .unwrap()
            .strip_suffix('"')
            .unwrap()
            .to_string(),
    )
}

#[test]
fn get_text_matches_the_box_for_every_algo_param() {
    let mut f = Fake::new();
    let mut seen = BTreeSet::new();
    for n in [0.0_f32, 0.37, 1.0] {
        for b in algo_blocks() {
            let mut view = f.ui.params_mut().block_mut(b).unwrap();
            for spec in view.specs() {
                view.set(spec.id, spec.denormalize(n));
            }
        }
        for node in 0..ALGO_CHAIN.blocks.len() {
            let mut sub = 0;
            while let Some(at) = ALGO_CHAIN.page(node, sub) {
                sub += 1;
                f.ui.go_for_test(Location::pages(PartId::ALL[0], at));
                let (def, ctx) = (f.ui.page_def(), f.ui.slot_ctx_for_test());
                for i in 0..6 {
                    let Some(a) = slot_addr(def, i, &ctx) else {
                        continue;
                    };
                    let want = f.ui.cell_text_for_test(i);
                    let got = ask(&mut f, &format!("ed get {}\n", EdAddr::sound(a).unwrap()));
                    let (norm, stored, text) = parse_val(&got);
                    assert_eq!(text, want.as_str(), "{a:?} at {n} on {}", def.short);
                    let p = f.ui.params();
                    let blk = chimera_engine::addr::BlockRead::block(p, a.block).unwrap();
                    assert!((stored - blk.get(a.param)).abs() < 1e-5, "{a:?}");
                    assert!((norm - blk.normalized(a.param)).abs() < 1e-5, "{a:?}");
                    seen.insert(EdAddr::sound(a).unwrap().to_string());
                }
            }
        }
    }
    assert!(seen.len() > 60, "{}", seen.len());
}

use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::ValFmt;

/// The spec's format at the stored value, as the box reads it: LogHz and
/// Vowel take the stored value, every other format the 0..1 one.
fn spec_text(f: &Fake, a: ParamAddr) -> String {
    let blk = chimera_engine::addr::BlockRead::block(f.ui.params(), a.block).unwrap();
    let spec = a.spec().unwrap();
    let v = match spec.fmt {
        ValFmt::LogHz { .. } | ValFmt::Vowel => blk.get(a.param),
        _ => blk.normalized(a.param),
    };
    let mut b = FmtBuf::new();
    fmt_val(&mut b, v, spec.fmt);
    b.as_str().to_string()
}

fn shown_now(f: &mut Fake) -> Vec<ParamAddr> {
    let mut shown = Vec::new();
    for node in 0..ALGO_FORMANT_CHAIN.blocks.len() {
        let mut sub = 0;
        while let Some(at) = ALGO_FORMANT_CHAIN.page(node, sub) {
            sub += 1;
            f.ui.go_for_test(Location::pages(PartId::ALL[0], at));
            let (def, ctx) = (f.ui.page_def(), f.ui.slot_ctx_for_test());
            for i in 0..6 {
                shown.extend(slot_addr(def, i, &ctx));
            }
        }
    }
    shown
}

#[test]
fn get_reads_a_param_no_cell_shows_by_its_stored_value() {
    let mut f = Fake::new();
    for n in [0.0_f32, 0.33, 1.0] {
        for b in algo_blocks() {
            let mut view = f.ui.params_mut().block_mut(b).unwrap();
            for spec in view.specs() {
                view.set(spec.id, spec.denormalize(n));
            }
        }
        let shown = shown_now(&mut f);
        let mut hidden = 0;
        for b in algo_blocks() {
            for spec in b.specs() {
                let a = ParamAddr::new(b, spec.id);
                if shown.contains(&a) {
                    continue;
                }
                hidden += 1;
                let (_, _, text) = parse_val(&ask(
                    &mut f,
                    &format!("ed get {}\n", EdAddr::sound(a).unwrap()),
                ));
                assert_eq!(text, spec_text(&f, a), "{a:?} at {n}");
            }
        }
        assert!(hidden > 3, "{hidden}");
    }
}

#[test]
fn get_bw_and_vowel_read_right_in_every_ui_state() {
    use chimera_engine::dsp::algo::params::{AlgoOpParams, AlgoParams};
    let bw = ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::BW);
    let vowel = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
    let mut f = Fake::new();
    // Op B a table (BW hidden), then a formant (BW on its page), then the
    // sound edited with another op selected.
    for wave in [None, Some(16.0_f32)] {
        if let Some(w) = wave {
            let mut v =
                f.ui.params_mut()
                    .block_mut(BlockRef::AlgoOp(Op::B))
                    .unwrap();
            v.set(AlgoOpParams::WAVE, w);
            assert_eq!(
                f.ui.slot_ctx_for_test().op_kinds[1],
                chimera_engine::dsp::algo::kinds::OpKind::Formant
            );
        }
        for sel in [Op::A, Op::B, Op::F] {
            f.ui.select_op_for_test(sel);
            for a in [bw, vowel] {
                let (_, _, text) = parse_val(&ask(
                    &mut f,
                    &format!("ed get {}\n", EdAddr::sound(a).unwrap()),
                ));
                assert_eq!(text, spec_text(&f, a), "{a:?} wave {wave:?} sel {sel:?}");
            }
        }
    }
}

/// #1039: a stored value that is no number reads `nan`, `inf` or `-inf`
/// in `val`'s stored word, never a number in its place; `since` the same.
#[test]
fn get_says_a_stored_value_that_is_no_number() {
    let mut f = Fake::new();
    let a = cutoff();
    let ed = EdAddr::sound(a).unwrap();
    for (v, word) in [
        (f32::NAN, "nan"),
        (f32::INFINITY, "inf"),
        (f32::NEG_INFINITY, "-inf"),
    ] {
        // No knob, CC or `set` stores one (`set` rests it at the least):
        // only a write past them.
        f.ui.project_mut().edit_part(P1).sound.params.filter.cutoff = v;
        let got = ask(&mut f, &format!("ed get {ed}\n"));
        let words: Vec<_> = got.lines().next().unwrap().split(' ').collect();
        assert_eq!(
            (words[0], words[1], words[3]),
            ("val", ed.to_string().as_str(), word),
            "{got}"
        );
        assert_eq!(got.lines().nth(1), Some("OK"), "{got}");
        let (lines, _) = since(&mut f, 0);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with(&format!("val {ed} ")) && l.split(' ').nth(3) == Some(word)),
            "since: {word}"
        );
    }
}

#[test]
fn get_unknown_addr_is_refused() {
    let mut f = Fake::new();
    for line in [
        "ed get NOPE.X\n",
        "ed get OP_C.NOPE\n",
        "ed get OP_C\n",
        "ed get\n",
    ] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed takes "), "{line}: {got}");
    }
    // A real address the ALGO Sound doesn't hold is another refusal.
    assert_eq!(
        ask(&mut f, "ed get MODAL.MODE\n"),
        "ERR ed get has no MODAL.MODE here\n"
    );
}

// ---- set ----

use chimera_core::console::EdState;
use chimera_engine::block::ParamKind;

fn apply(f: &mut Fake) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let q = f.ed.as_mut().unwrap();
    chimera_core::bank2::ed_apply(q, &mut f.ui);
}

fn is_choice(a: ParamAddr) -> bool {
    matches!(a.spec().unwrap().fmt, ValFmt::Names(_))
}

fn stored_of(f: &mut Fake, a: ParamAddr) -> f32 {
    parse_val(&ask(f, &format!("ed get {}\n", EdAddr::sound(a).unwrap()))).1
}

/// `v` in a word the decimal parse reads back exactly enough.
fn word(v: f32) -> String {
    format!("{v:.6}")
}

#[test]
fn set_then_get_round_trips() {
    let mut f = Fake::new();
    let mut n = 0;
    // `set` takes what `describe` lists, nothing else.
    let listed = addrs(&all_pages(&mut f));
    for b in algo_blocks() {
        for spec in b.specs() {
            let a = ParamAddr::new(b, spec.id);
            let ed = EdAddr::sound(a).unwrap().to_string();
            if !listed.contains(&ed) {
                assert!(
                    ask(&mut f, &format!("ed set {ed} 0\n")).starts_with("ERR ed set has no "),
                    "{ed}"
                );
                continue;
            }
            if is_choice(a) || !spec.stored || spec.patching {
                continue;
            }
            for at in [spec.min, spec.max, (spec.min + spec.max) / 2.0] {
                let want = spec.quantized(at);
                let got = ask(
                    &mut f,
                    &format!("ed set {} {}\n", EdAddr::sound(a).unwrap(), word(at)),
                );
                assert_eq!(got, "OK queued\n", "{a:?} {at}");
                apply(&mut f);
                let stored = stored_of(&mut f, a);
                let tol = 1e-4 * want.abs().max(1.0);
                assert!(
                    (stored - want).abs() <= tol,
                    "{a:?}: {at} -> {stored} not {want}"
                );
                n += 1;
            }
        }
    }
    assert!(n > 150, "{n}");
}

/// `val`'s stored word, as printed.
fn stored_word(f: &mut Fake, a: ParamAddr) -> String {
    let got = ask(f, &format!("ed get {}\n", EdAddr::sound(a).unwrap()));
    got.split(' ').nth(3).unwrap().to_string()
}

/// #1038: `ed get`'s stored word, sent back through `ed set`, stores the
/// same bits, on every ALGO knob's grid.
#[test]
fn get_then_set_is_bit_exact() {
    let mut f = Fake::new();
    let listed = addrs(&all_pages(&mut f));
    let mut n = 0;
    for b in algo_blocks() {
        for spec in b.specs() {
            let a = ParamAddr::new(b, spec.id);
            let ed = EdAddr::sound(a).unwrap().to_string();
            if !listed.contains(&ed) || is_choice(a) || !spec.stored || spec.patching {
                continue;
            }
            for k in 0..=1280 {
                let at = spec.denormalize(k as f32 / 1280.0);
                ask(&mut f, &format!("ed set {ed} {}\n", word(at)));
                apply(&mut f);
                let (was, text) = (stored_of(&mut f, a), stored_word(&mut f, a));
                ask(&mut f, &format!("ed set {ed} {}\n", word(spec.min)));
                apply(&mut f);
                assert_eq!(ask(&mut f, &format!("ed set {ed} {text}\n")), "OK queued\n");
                apply(&mut f);
                let now = stored_of(&mut f, a);
                assert_eq!(
                    now.to_bits(),
                    was.to_bits(),
                    "{ed}: {text} came back {now:e}"
                );
                n += 1;
            }
        }
    }
    assert!(n > 60_000, "{n}");
}

#[test]
fn set_glides_like_a_knob() {
    use chimera_core::dsp::knobs::KnobPublisher;
    use chimera_core::instrument::AudioShared;
    use chimera_core::params::FilterParams;
    let cutoff = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut by_ed = Fake::new();
    let mut by_knob = Fake::new();
    let (mut ka, mut kb) = (KnobPublisher::new(), KnobPublisher::new());
    let (mut sa, mut sb) = (AudioShared::default(), AudioShared::default());
    sa.update_from_project(by_ed.ui.project(), 0, &mut ka);
    sb.update_from_project(by_knob.ui.project(), 0, &mut kb);
    // One tick of the knob, and the value it lands on.
    let part = PartId::ALL[0];
    assert!(by_knob.ui.edit_addr(part, cutoff, -1, false));
    let to = stored_of(&mut by_knob, cutoff);
    assert_eq!(
        ask(&mut by_ed, &format!("ed set FILTER.CUTOFF {}\n", word(to))),
        "OK queued\n"
    );
    // Queued, not applied: nothing moved yet.
    sa.update_from_project(by_ed.ui.project(), 0, &mut ka);
    assert_eq!(sa.parts[0].knobs.count(), 0);
    apply(&mut by_ed);
    sa.update_from_project(by_ed.ui.project(), 0, &mut ka);
    sb.update_from_project(by_knob.ui.project(), 0, &mut kb);
    // One move, a glide, never the whole Sound landing at once.
    assert_eq!(sa.parts[0].knobs.count(), 1);
    assert_eq!(sa.parts[0].knobs.count(), sb.parts[0].knobs.count());
    assert!((stored_of(&mut by_ed, cutoff) - to).abs() < 1e-2 * to);
    // The very move the knob made, from where it was to where it lands: a
    // snap would publish `from == to`, or another target.
    let (ed_move, knob_move) = (moves(&sa.parts[0].knobs), moves(&sb.parts[0].knobs));
    assert_eq!(ed_move, knob_move);
    let (from, to) = ed_move;
    assert!(from != to, "{from} -> {to}: a landing, not a glide");
}

/// The delta's newest move, `(from, to)`.
fn moves(d: &chimera_core::dsp::knobs::KnobDelta) -> (f32, f32) {
    d.last_move().expect("a move")
}

#[test]
fn set_clamps_quantizes_and_refuses() {
    use chimera_engine::dsp::algo::params::AlgoOpParams;
    use chimera_engine::params::FilterParams;
    let mut f = Fake::new();
    let cut = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let spec = cut.spec().unwrap();
    assert_eq!(
        ask(&mut f, "ed set FILTER.CUTOFF 99999999\n"),
        "OK queued\n"
    );
    apply(&mut f);
    assert_eq!(stored_of(&mut f, cut), spec.max);
    // A stepped param lands on a step.
    let lvl = ParamAddr::new(BlockRef::AlgoOp(Op::C), AlgoOpParams::COARSE);
    assert!(is_choice(lvl));
    let steppy = BlockRef::AlgoOp(Op::C)
        .specs()
        .iter()
        .find(|s| s.kind == ParamKind::Stepped && !matches!(s.fmt, ValFmt::Names(_)))
        .expect("a stepped, nameless param");
    let a = ParamAddr::new(BlockRef::AlgoOp(Op::C), steppy.id);
    let mid = (steppy.min + steppy.max) / 2.0 + 0.3;
    assert_eq!(
        ask(
            &mut f,
            &format!("ed set {} {}\n", EdAddr::sound(a).unwrap(), word(mid))
        ),
        "OK queued\n"
    );
    apply(&mut f);
    let got = stored_of(&mut f, a);
    assert_eq!(got, got.round(), "{a:?} {got}");
    // A choice by name, the typed word kept: CRSE's "1.00" is the name.
    let ValFmt::Names(names) = lvl.spec().unwrap().fmt else {
        unreachable!()
    };
    let at = names.iter().position(|&n| n == "1.00").expect("1.00");
    assert_eq!(ask(&mut f, "ed set OP_C.CRSE 1.00\n"), "OK queued\n");
    apply(&mut f);
    assert_eq!(stored_of(&mut f, lvl), at as f32);
    // FILTER.KIND by name.
    let kind = ParamAddr::new(BlockRef::Filter, FilterParams::KIND);
    let ValFmt::Names(kinds) = kind.spec().unwrap().fmt else {
        unreachable!()
    };
    let lad = kinds.iter().position(|&n| n == "LADDER").expect("LADDER");
    assert_eq!(ask(&mut f, "ed set FILTER.KIND LADDER\n"), "OK queued\n");
    apply(&mut f);
    assert_eq!(stored_of(&mut f, kind), lad as f32);
    // Refusals: a choice takes only its own names, a number param only a number.
    for line in [
        "ed set FILTER.KIND NOPE\n",
        "ed set FILTER.KIND 1\n",
        "ed set OP_C.CRSE 1.0\n",
        "ed set FILTER.CUTOFF LADDER\n",
    ] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed set "), "{line}: {got}");
    }
    let before = stored_of(&mut f, kind);
    apply(&mut f);
    assert_eq!(stored_of(&mut f, kind), before, "a refusal queues nothing");
    // An unknown address is a parse refusal.
    let got = ask(&mut f, "ed set NOPE.X 1\n");
    assert!(got.starts_with("ERR ed takes "), "{got}");
}

#[test]
fn set_stores_a_param_the_variant_hides() {
    use chimera_engine::dsp::algo::params::AlgoOpParams;
    let mut f = Fake::new();
    // Op B is a table: BW is hidden, and still set.
    let bw = ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::BW);
    let spec = bw.spec().unwrap();
    let to = (spec.min + spec.max) / 2.0;
    assert_eq!(
        ask(&mut f, &format!("ed set OP_B.BW {}\n", word(to))),
        "OK queued\n"
    );
    apply(&mut f);
    let got = stored_of(&mut f, bw);
    assert!(
        (got - spec.quantized(to)).abs() <= 1e-3 * to.abs().max(1.0),
        "{got}"
    );
}

#[test]
fn set_queue_full_is_busy() {
    let mut f = Fake::new();
    let mut taken = 0;
    while ask(&mut f, "ed set FILTER.CUTOFF 1000\n") == "OK queued\n" {
        taken += 1;
        assert!(taken < 1000, "never full");
    }
    assert_eq!(ask(&mut f, "ed set FILTER.CUTOFF 1000\n"), "ERR busy\n");
    assert_eq!(taken, EdState::CAP);
    // An apply empties it.
    apply(&mut f);
    assert_eq!(ask(&mut f, "ed set FILTER.CUTOFF 1000\n"), "OK queued\n");
}

#[test]
fn set_takes_only_the_engines_addresses() {
    use chimera_core::preset::Sound;
    let mut f = Fake::new();
    let was = chimera_core::ui::nav::PartChain::of(&f.ui.project().part(PartId::ALL[0]).sound);
    **f.ui.project_mut().edit_part(PartId::ALL[0]).sound = Sound::init(EngineType::Modal);
    f.ui.replaced_for_test(was);
    let got = ask(&mut f, "ed set OP_A.CRSE 2\n");
    assert!(got.starts_with("ERR ed set has no OP_A.CRSE"), "{got}");
    assert_eq!(ask(&mut f, "ed set FILTER.CUTOFF 1000\n"), "OK queued\n");
    assert_eq!(ask(&mut f, "ed set MODAL.BRIGHT 0.5\n"), "OK queued\n");
}

#[test]
fn set_without_a_queue_is_not_in_this_build() {
    let mut f = Fake::new();
    f.ed = None;
    assert_eq!(
        ask(&mut f, "ed set FILTER.CUTOFF 1000\n"),
        "ERR ed set is not in this build\n"
    );
}

#[test]
fn apply_drops_a_set_for_a_part_that_changed_engine() {
    use chimera_core::preset::Sound;
    let mut f = Fake::new();
    assert_eq!(ask(&mut f, "ed set FILTER.CUTOFF 1000\n"), "OK queued\n");
    let was = chimera_core::ui::nav::PartChain::of(&f.ui.project().part(PartId::ALL[0]).sound);
    **f.ui.project_mut().edit_part(PartId::ALL[0]).sound = Sound::init(EngineType::Modal);
    f.ui.replaced_for_test(was);
    let rev = f.ui.project().part(PartId::ALL[0]).sound.stamp();
    apply(&mut f);
    assert_eq!(f.ui.project().part(PartId::ALL[0]).sound.stamp(), rev);
}

// ---- since ----

use chimera_core::midi_in::{Cc, CcEvent};
use chimera_core::midi_map::{Input as MapInput, Mappable};
use chimera_hal::{ButtonId, Ms};
use screen::{Input, feed};

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

fn cutoff() -> ParamAddr {
    ParamAddr::new(
        BlockRef::Filter,
        chimera_engine::params::FilterParams::CUTOFF,
    )
}

/// What `ed since <n>` answered for PART: its lines but the closing `OK
/// <m>`, and `m`.
fn since(f: &mut Fake, n: u32) -> (Vec<String>, u32) {
    let got = ask(f, &format!("ed since {n}\n"));
    assert!(got.ends_with('\n'), "{got}");
    let mut lines: Vec<String> = got.lines().map(str::to_string).collect();
    // A full answer opens with an empty line (its own test below).
    if lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    let last = lines.pop().unwrap();
    let m = last
        .strip_prefix("OK ")
        .unwrap_or_else(|| panic!("{last}"))
        .parse()
        .unwrap();
    // PART's section alone: ROUTES, MAP and MIXER have their own tests.
    let lines = lines
        .into_iter()
        .take_while(|l| l != "mixer")
        .filter(|l| {
            ![
                "routes ", "route ", "unroute ", "map ", "row ", "learn ", "mapfile ",
            ]
            .iter()
            .any(|h| l.starts_with(h))
        })
        .collect();
    (lines, m)
}

/// `ed get`'s `val` line for `addr`.
fn get_line(f: &mut Fake, addr: &str) -> String {
    ask(f, &format!("ed get {addr}\n"))
        .lines()
        .next()
        .unwrap()
        .to_string()
}

fn vals(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.starts_with("val ")).collect()
}

#[test]
fn since_zero_answers_everything() {
    let mut f = Fake::new();
    let (lines, m) = since(&mut f, 0);
    assert_ne!(m, 0);
    assert_eq!(lines[0], "part 1 ALGO");
    let panels: Vec<&String> = lines.iter().filter(|l| l.starts_with("panel ")).collect();
    assert_eq!(panels.len(), 14, "{panels:?}");
    assert!(panels.iter().any(|l| l.starts_with("panel FILTER ")));
    assert!(panels.iter().any(|l| l.starts_with("panel OP6 ")));
    // Every address `describe` lists once, in `get`'s own words, and no other.
    let mut want = 0;
    for name in addrs(&all_pages(&mut f)) {
        let line = get_line(&mut f, &name);
        assert_eq!(
            lines.iter().filter(|l| **l == line).count(),
            1,
            "{name}: {line}"
        );
        want += 1;
    }
    // ADR 0292: AMP's BASE, OUT.VCA, on its page; ADR 0294: AMP ›
    // STEREO's PAN and WIDTH.
    assert_eq!(want, 221, "the schema's addresses");
    assert_eq!(vals(&lines).len(), want);
    assert_eq!(lines.len(), 1 + 14 + want);
}

#[test]
fn a_full_answer_starts_with_an_empty_line() {
    let mut f = Fake::new();
    let got = ask(&mut f, "ed since 0\n");
    assert!(got.starts_with("\npart 1 ALGO\n"), "{got:?}");
    // A stale number is a full answer again; a current one is a diff, with none.
    let again = ask(&mut f, "ed since 7\n");
    assert!(again.starts_with("\npart 1 ALGO\n"), "{again:?}");
    let m: u32 = again.lines().last().unwrap()[3..].parse().unwrap();
    let diff = ask(&mut f, &format!("ed since {m}\n"));
    assert!(!diff.starts_with('\n'), "{diff:?}");
}

#[test]
fn since_current_with_no_change_is_just_ok() {
    let mut f = Fake::new();
    let (_, m) = since(&mut f, 0);
    let (lines, m2) = since(&mut f, m);
    assert!(lines.is_empty(), "{lines:?}");
    assert_ne!(m2, 0);
    assert_ne!(m2, m, "advances on every answer");
    assert!(since(&mut f, m2).0.is_empty());
}

#[test]
fn since_reports_knob_cc_load_and_set() {
    use chimera_core::project::{PartFrom, PartSource, ReplaceGuard};
    let mut f = Fake::new();
    let (_, mut m) = since(&mut f, 0);
    // A knob turn.
    assert!(f.ui.edit_addr(P1, cutoff(), -1, false));
    let (lines, m2) = since(&mut f, m);
    m = m2;
    assert_eq!(lines, [get_line(&mut f, "FILTER.CUTOFF")]);
    // A set: through the queue, applied as a knob turn.
    assert_eq!(ask(&mut f, "ed set FILTER.CUTOFF 1234\n"), "OK queued\n");
    apply(&mut f);
    let (lines, m2) = since(&mut f, m);
    m = m2;
    assert_eq!(lines, [get_line(&mut f, "FILTER.CUTOFF")]);
    // A CC, learned onto the cutoff (the since() after it only takes in the learn's own write).
    f.ui.project_mut()
        .edit_part(P1)
        .sound
        .map
        .learn(MapInput::cc(74).unwrap(), Mappable::new(cutoff()).unwrap())
        .unwrap();
    since(&mut f, m);
    let (_, m2) = since(&mut f, 0);
    m = m2;
    let ev = CcEvent {
        channel: f.ui.project().part(P1).mix.channel,
        cc: Cc::new(74).unwrap(),
        value: 0,
    };
    if cfg!(feature = "midi-map") {
        f.ui.hear_ccs(vec![ev].into_iter(), Ms(1000), true);
        for i in 1..=40 {
            f.ui.hear_ccs(std::iter::empty::<CcEvent>(), Ms(1000 + 4 * i), true);
        }
        let (lines, m2) = since(&mut f, m);
        m = m2;
        assert_eq!(lines, [get_line(&mut f, "FILTER.CUTOFF")], "the CC");
    }
    // A Sound load: only the values it changed.
    let before = get_line(&mut f, "FILTER.CUTOFF");
    let src = PartSource {
        part: P1,
        from: PartFrom::Init(EngineType::Algo),
    };
    let proj = f.ui.project();
    let c = match ReplaceGuard::check(proj, f.ui.template(), src) {
        Ok(c) => c,
        Err(n) => n.into_pending().anyway(proj),
    };
    f.ui.project_mut().replace_part(c).unwrap();
    let (lines, _) = since(&mut f, m);
    assert!(!lines.is_empty());
    // SETTINGS' marks may say the Part's Sound moved (ADR 0289).
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("val ") || l.starts_with("sound ") || l.starts_with("project ")),
        "{lines:?}"
    );
    let after = get_line(&mut f, "FILTER.CUTOFF");
    assert_ne!(before, after);
    assert!(lines.contains(&after), "{lines:?}");
}

#[test]
fn since_reports_load_part_and_engine() {
    use chimera_core::preset::Sound;
    let mut f = Fake::new();
    screen::init_part_engine(&mut f.ui, P2, EngineType::Va);
    let (_, m) = since(&mut f, 0);
    // Another Part is selected: its engine is told, never its values.
    feed(&mut f.ui, Input::press(ButtonId::B2));
    assert_eq!(f.ui.active_part, P2);
    let (lines, m) = since(&mut f, m);
    assert_eq!(lines[0], "part 2 VA");
    assert!(lines.iter().any(|l| l.starts_with("val VA.")), "{lines:?}");
    // Back: the editor reads the whole Sound again.
    feed(&mut f.ui, Input::press(ButtonId::B1));
    let (lines, m) = since(&mut f, m);
    assert_eq!(lines[0], "part 1 ALGO");
    assert!(vals(&lines).len() > 100, "{}", lines.len());
    // The engine of the selected Part changes under it.
    let was = chimera_core::ui::nav::PartChain::of(&f.ui.project().part(P1).sound);
    **f.ui.project_mut().edit_part(P1).sound = Sound::init(EngineType::Modal);
    f.ui.replaced_for_test(was);
    let (lines, m) = since(&mut f, m);
    assert_eq!(lines[0], "part 1 MODAL");
    assert!(
        lines.iter().any(|l| l.starts_with("panel RESONATOR ")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("val MODAL.")),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.starts_with("val OP_")), "{lines:?}");
    assert!(since(&mut f, m).0.is_empty());
}

#[test]
fn since_reports_variant_change() {
    let mut f = Fake::new();
    let (_, m) = since(&mut f, 0);
    assert_eq!(ask(&mut f, "ed set FILTER.KIND LADDER\n"), "OK queued\n");
    apply(&mut f);
    let (lines, m) = since(&mut f, m);
    assert_eq!(
        lines.iter().filter(|l| *l == "panel FILTER LADDER").count(),
        1,
        "{lines:?}"
    );
    assert!(lines.iter().any(|l| l.starts_with("val FILTER.KIND ")));
    // The panel comes before the values that read under it.
    let panel = lines.iter().position(|l| l.starts_with("panel")).unwrap();
    let val = lines.iter().position(|l| l.starts_with("val")).unwrap();
    assert!(panel < val, "{lines:?}");
    assert!(since(&mut f, m).0.is_empty());
}

#[test]
fn since_coalesces_a_fast_turn() {
    let mut f = Fake::new();
    let (_, m) = since(&mut f, 0);
    for _ in 0..20 {
        f.ui.edit_addr(P1, cutoff(), -1, false);
    }
    let (lines, _) = since(&mut f, m);
    assert_eq!(lines, [get_line(&mut f, "FILTER.CUTOFF")]);
}

#[test]
fn stale_number_answers_everything() {
    let mut f = Fake::new();
    let (all, m1) = since(&mut f, 0);
    f.ui.edit_addr(P1, cutoff(), -1, false);
    // The answer for `m1` never reached the editor: it asks with 0 again,
    // or with the number before: both are everything.
    let (_, m2) = since(&mut f, m1);
    f.ui.edit_addr(P1, cutoff(), -1, false);
    let (again, m3) = since(&mut f, m1);
    assert_eq!(again.len(), all.len());
    assert_ne!(m3, m2);
    // A number never issued is as stale.
    let (never, _) = since(&mut f, m3 + 5);
    assert_eq!(never.len(), all.len());
    // The editor's current number still gets a diff.
    let (_, m4) = since(&mut f, 0);
    assert!(since(&mut f, m4).0.is_empty());
}

#[test]
fn refused_set_reports_no_value() {
    let mut f = Fake::new();
    let (_, m) = since(&mut f, 0);
    // Every op a FRMT: the formant budget holds fewer, so the writer
    // refuses the last (`OK queued` all the same).
    let ops = ["OP_A", "OP_B", "OP_C", "OP_D", "OP_E", "OP_F"];
    for o in ops {
        assert_eq!(
            ask(&mut f, &format!("ed set {o}.WAVE FRMT\n")),
            "OK queued\n"
        );
        apply(&mut f);
    }
    let (lines, m) = since(&mut f, m);
    let refused: Vec<&str> = ops
        .iter()
        .copied()
        .filter(|o| !get_line(&mut f, &format!("{o}.WAVE")).contains("FRMT"))
        .collect();
    assert!(
        !refused.is_empty() && refused.len() < ops.len(),
        "{refused:?}"
    );
    // The landed formants moved the panels' context, so that answer is
    // whole (a refused op's line among them, as `get` has it).
    for o in &refused {
        let want = get_line(&mut f, &format!("{o}.WAVE"));
        assert!(lines.contains(&want), "{want}");
    }
    // Asked again, still refused: no value to report, only the refusal.
    assert_eq!(
        ask(&mut f, &format!("ed set {}.WAVE FRMT\n", refused[0])),
        "OK queued\n"
    );
    apply(&mut f);
    let (lines, _) = since(&mut f, m);
    assert_eq!(
        lines,
        [format!(
            "refused {}.WAVE \"FORMANT BUDGET FULL\"",
            refused[0]
        )]
    );
}

#[test]
fn full_answer_fits_the_answer_clock() {
    let mut f = Fake::new();
    let got = ask(&mut f, "ed since 0\n");
    assert!(got.len() < ANSWER_BYTES_MAX, "{} B", got.len());
    // A load changes everything: no more than the full answer.
    for b in algo_blocks() {
        if let Some(mut blk) = f.ui.params_mut().block_mut(b) {
            for s in b.specs() {
                blk.set(s.id, s.max);
            }
        }
    }
    let got = ask(&mut f, "ed since 0\n");
    assert!(got.len() < ANSWER_BYTES_MAX, "{} B", got.len());
}

#[test]
fn since_malformed_and_unbuilt_are_refused() {
    let mut f = Fake::new();
    for line in [
        "ed since\n",
        "ed since x\n",
        "ed since 1 2\n",
        "ed since -1\n",
    ] {
        let got = ask(&mut f, line);
        assert!(got.starts_with("ERR ed takes "), "{line}: {got}");
    }
    f.ed = None;
    assert_eq!(
        ask(&mut f, "ed since 0\n"),
        "ERR ed since is not in this build\n"
    );
}

#[test]
fn gen_is_never_zero_and_wraps_past_it() {
    use chimera_core::console::{ED_SLOTS, Gen, ed_slots_used};
    assert_eq!(Gen::new(0), None);
    assert_eq!(Gen::new(1_000_000_000), None);
    let last = Gen::new(999_999_999).unwrap();
    assert_eq!(last.next(), Gen::FIRST);
    assert_eq!(Gen::FIRST.next().get(), 2);
    // The copy holds every param of every block.
    let schema: usize = algo_blocks().iter().map(|b| b.specs().len()).sum();
    assert_eq!(ed_slots_used(Schema::Engine(EngineType::Algo)), schema);
    for e in EngineType::ALL {
        assert!(
            ed_slots_used(Schema::Engine(e)) <= ED_SLOTS,
            "{e:?}: {} of {ED_SLOTS}",
            ed_slots_used(Schema::Engine(e))
        );
    }
    // DRUM, the most, sets it: no slot held for nothing.
    assert_eq!(ed_slots_used(Schema::Engine(EngineType::Drum)), ED_SLOTS);
    // A number past nine digits is refused, not wrapped.
    let mut f = Fake::new();
    let got = ask(&mut f, "ed since 1000000000\n");
    assert!(got.starts_with("ERR ed takes "), "{got}");
}

/// Every ALGO address's `get` line: its bits and its text.
fn all_lines(f: &mut Fake) -> Vec<(String, String)> {
    let mut v = Vec::new();
    for b in algo_blocks() {
        for s in b.specs() {
            let a = EdAddr::sound(ParamAddr::new(b, s.id)).unwrap().to_string();
            v.push((a.clone(), get_line(f, &a)));
        }
    }
    v
}

fn bits_of(line: &str) -> &str {
    // `val <addr> <norm> <stored> "<text>"`: the numbers.
    line.rsplit_once(" \"").unwrap().0
}

#[test]
fn a_variant_change_resends_text_that_changed_under_unchanged_bits() {
    let mut f = Fake::new();
    let mut vacuous = true;
    // Each switch the Sound has: its panel variant changes with the param's
    // own write, every other param's bits stay.
    let switches = [
        (BlockRef::Filter, chimera_engine::params::FilterParams::KIND),
        (BlockRef::VoiceFx, chimera_core::block::ParamId(0)),
        (
            BlockRef::AlgoOp(Op::A),
            chimera_engine::dsp::algo::params::AlgoOpParams::WAVE,
        ),
    ];
    for (block, id) in switches {
        let spec = ParamAddr::new(block, id).spec().unwrap();
        let (_, m0) = since(&mut f, 0);
        let mut m = m0;
        let mut v = spec.min;
        while v <= spec.max {
            let before = all_lines(&mut f);
            if let Some(mut blk) = f.ui.params_mut().block_mut(block) {
                blk.set(id, v);
            }
            let after = all_lines(&mut f);
            let (lines, m2) = since(&mut f, m);
            m = m2;
            for ((a, x), (_, y)) in before.iter().zip(&after) {
                if x != y && bits_of(x) == bits_of(y) {
                    vacuous = false;
                    assert!(lines.contains(y), "{a}: text {x} -> {y} not resent");
                }
            }
            v += 1.0;
        }
    }
    assert!(!vacuous, "no param's text moved under unchanged bits");
}

#[test]
fn no_algo_cell_reads_the_reverb_clock() {
    // A tempo or fold change moves a text only through a reverb panel cell
    // (`cell_text::param_text`); the ALGO pages have none, so `since` keeps
    // no clock in its copy.
    use chimera_core::ui::view::is_reverb_panel;
    for b in ALGO_FORMANT_CHAIN.blocks {
        for def in std::iter::once(b.def).chain(b.sub_pages.iter().copied()) {
            for i in 0..def.params.len() {
                assert!(!is_reverb_panel(def, i), "slot {i}");
            }
        }
    }
}

#[test]
fn a_bank2_status_change_resends_the_closed_text() {
    use chimera_core::bank2::Bank2Status;
    let kind = chimera_engine::params::FilterParams::KIND;
    let spec = ParamAddr::new(BlockRef::Filter, kind).spec().unwrap();
    // A KIND that plays from bank 2 reads NEEDS UPDATE while bank 2 is closed.
    let mut found = false;
    let mut v = spec.min;
    while v <= spec.max && !found {
        // A fresh unit each: a status, once set, stays.
        let mut f = Fake::new();
        f.ui.params_mut()
            .block_mut(BlockRef::Filter)
            .unwrap()
            .set(kind, v);
        let (_, m) = since(&mut f, 0);
        let open = get_line(&mut f, "FILTER.KIND");
        f.ui.set_bank2(Bank2Status::Empty);
        let closed = get_line(&mut f, "FILTER.KIND");
        if open != closed {
            found = true;
            assert!(closed.contains("NEEDS UPDATE"), "{closed}");
            let (lines, _) = since(&mut f, m);
            assert!(lines.contains(&closed), "{closed} not resent: {lines:?}");
        }
        v += 1.0;
    }
    assert!(found, "no bank 2 FILTER.KIND");
}

// ---- part ----

#[test]
fn part_follows_as_its_key_does() {
    let mut f = Fake::new();
    screen::init_part_engine(&mut f.ui, P2, EngineType::Va);
    let mut keyed = Fake::new();
    screen::init_part_engine(&mut keyed.ui, P2, EngineType::Va);
    feed(&mut keyed.ui, Input::press(ButtonId::B2));
    let (_, m) = since(&mut f, 0);
    assert_eq!(ask(&mut f, "ed part 2\n"), "OK\n");
    // Queued: the UI pass moves it.
    assert_eq!(f.ui.active_part, P1);
    apply(&mut f);
    assert_eq!(f.ui.active_part, P2);
    assert_eq!(f.ui.location(), keyed.ui.location());
    let (lines, m) = since(&mut f, m);
    assert_eq!(lines[0], "part 2 VA");
    assert!(lines.iter().any(|l| l.starts_with("val VA.")), "{lines:?}");
    assert_eq!(ask(&mut f, "ed part 1\n"), "OK\n");
    apply(&mut f);
    let (lines, _) = since(&mut f, m);
    assert_eq!(lines[0], "part 1 ALGO");
    assert!(vals(&lines).len() > 100, "{}", lines.len());
}

#[test]
fn part_already_followed_stays_put() {
    // Its key would flip pages and mixer; `ed part` is the editor's
    // "follow", so asking again changes nothing.
    let mut f = Fake::new();
    let at = f.ui.location();
    assert_eq!(ask(&mut f, "ed part 1\n"), "OK\n");
    apply(&mut f);
    assert_eq!(f.ui.location(), at);
}

#[test]
fn part_out_of_range_is_refused() {
    let mut f = Fake::new();
    let max = PartId::ALL.len();
    let want = format!("ERR ed part takes 1-{max}\n");
    for line in [
        "ed part 0\n",
        "ed part 7\n",
        "ed part x\n",
        "ed part\n",
        "ed part -1\n",
    ] {
        assert_eq!(ask(&mut f, line), want, "{line}");
    }
    assert!(ask(&mut f, "ed part 2 3\n").starts_with("ERR ed takes "));
    apply(&mut f);
    assert_eq!(f.ui.active_part, P1);
}

#[test]
fn part_without_a_queue_is_not_in_this_build() {
    let mut f = Fake::new();
    f.ed = None;
    assert_eq!(
        ask(&mut f, "ed part 2\n"),
        "ERR ed part is not in this build\n"
    );
}

/// `ed part` straight into the UI pass, as the shell applies it.
fn ed_part(f: &mut Fake, n: u32) {
    assert_eq!(ask(f, &format!("ed part {n}\n")), "OK\n");
    apply(f);
}

/// MIX + SEQ: learn armed on the page's focused cell.
#[cfg(feature = "midi-map")]
fn arm_learn(f: &mut Fake) {
    use chimera_core::ui::learn::Learn;
    feed(
        &mut f.ui,
        Input::chord(ButtonId::Mix, ButtonId::Seq).at(10_000),
    );
    feed(
        &mut f.ui,
        Input::release(ButtonId::Seq)
            .and_held(ButtonId::Mix)
            .at(10_100),
    );
    feed(&mut f.ui, Input::release(ButtonId::Mix).at(10_133));
    assert!(matches!(f.ui.learn(), Learn::Armed { .. }));
}

#[test]
fn part_drops_a_prompt_as_its_key_does() {
    use chimera_core::project::{PartFrom, PartSource};
    let open = |f: &mut Fake| {
        f.ui.edit_to(P1, cutoff(), 500.0);
        let src = PartSource {
            part: P1,
            from: PartFrom::Init(EngineType::Va),
        };
        assert!(f.ui.ask_replace_for_test(src), "an edited Part asks");
    };
    let mut keyed = Fake::new();
    open(&mut keyed);
    feed(&mut keyed.ui, Input::press(ButtonId::B2));
    let mut f = Fake::new();
    open(&mut f);
    ed_part(&mut f, 2);
    assert_eq!(f.ui.prompt_kind_for_test(), None);
    assert_eq!(keyed.ui.prompt_kind_for_test(), None);
    assert_eq!(f.ui.active_part, P2);
    assert_eq!(f.ui.location(), keyed.ui.location());
}

// Learn is MIDI MAP's.
#[cfg(feature = "midi-map")]
#[test]
fn part_waits_for_a_job_and_keeps_learn_as_its_key_does() {
    use chimera_core::ui::learn::Learn;
    let mut f = Fake::new();
    arm_learn(&mut f);
    f.ui.queue_load_list_for_test();
    assert!(f.ui.job_queued_for_test());
    let at = f.ui.location();
    ed_part(&mut f, 2);
    assert_eq!(f.ui.active_part, P1);
    assert_eq!(f.ui.location(), at);
    assert!(
        matches!(f.ui.learn(), Learn::Armed { .. }),
        "the key breaks before learn"
    );
}

#[test]
fn part_from_the_mixer_goes_where_its_key_does() {
    let mut keyed = Fake::new();
    feed(&mut keyed.ui, Input::press(ButtonId::B1));
    let mixer = keyed.ui.location();
    assert!(mixer.part().is_some_and(|p| p == P1));
    feed(&mut keyed.ui, Input::press(ButtonId::B2));
    let mut f = Fake::new();
    feed(&mut f.ui, Input::press(ButtonId::B1));
    assert_eq!(f.ui.location(), mixer);
    ed_part(&mut f, 2);
    assert_eq!(f.ui.active_part, P2);
    assert_eq!(f.ui.location(), keyed.ui.location());
}

#[test]
fn part_wakes_the_screen_only_when_it_moves() {
    let mut f = Fake::new();
    f.ui.step_idle(24 * 3_600_000, false);
    assert!(f.ui.asleep());
    ed_part(&mut f, 1);
    assert!(f.ui.asleep(), "nothing moved");
    ed_part(&mut f, 2);
    assert!(!f.ui.asleep());
}

#[test]
fn part_numbers_count_from_one() {
    assert_eq!(PartId::from_number(0), None);
    assert_eq!(PartId::from_number(1), Some(P1));
    assert_eq!(PartId::from_number(2), Some(P2));
    let last = PartId::ALL.len() as u32;
    assert_eq!(PartId::from_number(last), PartId::ALL.last().copied());
    assert_eq!(PartId::from_number(last + 1), None);
    assert_eq!(PartId::from_number(u32::MAX), None);
    for p in PartId::ALL {
        assert_eq!(PartId::from_number(u32::from(p.number())), Some(p));
    }
}

/// Where a test starts, by name.
type Start = (&'static str, fn(&mut Fake));

#[test]
fn part_from_off_its_ladder_comes_back_as_its_key_does() {
    use chimera_core::ui::settings::{Screen, screen_path};
    let starts: [Start; _] = [
        ("SETTINGS", |f| screen::tap(&mut f.ui, ButtonId::Menu)),
        ("LOAD PROJECT", |f| {
            f.ui.go_for_test(Location::settings_at(screen_path(Screen::LoadProject), 0))
        }),
        ("browser", |f| {
            feed(&mut f.ui, Input::chord(ButtonId::Edit, ButtonId::B1))
        }),
        #[cfg(feature = "midi-map")]
        ("MIDI MAP", |f| {
            f.ui.go_for_test(Location::settings_at(screen_path(Screen::MidiMap), 0))
        }),
    ];
    for (name, start) in starts {
        let mut keyed = Fake::new();
        start(&mut keyed);
        let off = keyed.ui.location();
        assert_eq!(keyed.ui.active_part, P1, "{name}");
        feed(&mut keyed.ui, Input::press(ButtonId::B1));
        assert_ne!(keyed.ui.location(), off, "{name}: the key comes back");
        let mut f = Fake::new();
        start(&mut f);
        assert_eq!(f.ui.location(), off, "{name}");
        ed_part(&mut f, 1);
        assert_eq!(f.ui.location(), keyed.ui.location(), "{name}");
    }
}

#[test]
fn part_on_its_own_mixer_stays_put() {
    let mut f = Fake::new();
    feed(&mut f.ui, Input::press(ButtonId::B1));
    let at = f.ui.location();
    assert!(at.part() == Some(P1));
    ed_part(&mut f, 1);
    assert_eq!(f.ui.location(), at);
}

#[test]
fn a_key_and_ed_part_in_one_frame_land_once() {
    let mut keyed = Fake::new();
    feed(&mut keyed.ui, Input::press(ButtonId::B2));
    let mut f = Fake::new();
    assert_eq!(ask(&mut f, "ed part 2\n"), "OK\n");
    // One frame as the shells run it: the hand's input, then the console's.
    feed(&mut f.ui, Input::press(ButtonId::B2));
    apply(&mut f);
    assert_eq!(f.ui.location(), keyed.ui.location());
}

/// The unit's refusal of a Part it hasn't, for the editor's tests to pin
/// its Part count against (its `UnitTest`, in chimera-releases, through
/// the editor kit).
#[test]
fn writes_editor_part_fixture() {
    let mut f = Fake::new();
    let text = ask(&mut f, "ed part 0\n");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/editor/part-refusal.txt");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(on_disk == text, "BLESS=1 rewrites {}", path.display());
}

/// The proof trait's name, never spelled whole in this file's code: the
/// scan below reads this file too.
fn proof_word() -> String {
    ["Editor", "Proof"].concat()
}

/// How often `text` names the proof trait outside its comment lines. Any
/// naming counts, not an `impl` line alone: an alias (`use … as`) names
/// it once, and an impl split across lines names it on one of them.
fn proof_mentions(text: &str) -> usize {
    let word = proof_word();
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .map(|l| l.matches(word.as_str()).count())
        .sum()
}

/// The review's I3: the scene's yes takes the engine's `Held`, made only
/// by `Held::check` (its fields private to the engine, a doctest there);
/// a load's `discard` takes the proof trait, and the workspace names that
/// trait in two files only: where the engine declares and takes it, and
/// `ed`'s one implementor (`project::Discard`, made by the `discard`
/// word, its field private to its module). The backstop behind the
/// types: a new implementor, however it is spelled, is a new mention.
#[test]
fn only_the_ed_path_proves_an_editors_yes() {
    use std::path::Path;
    fn walk(dir: &Path, out: &mut Vec<(String, usize, String)>, root: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out, root);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                let n = proof_mentions(&text);
                if n > 0 {
                    let at = p.strip_prefix(root).unwrap().to_str().unwrap();
                    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    out.push((at.to_string(), n, flat));
                }
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut found = Vec::new();
    for c in std::fs::read_dir(root).unwrap().flatten() {
        let name = c.file_name();
        if name.to_str().is_some_and(|n| n.starts_with("chimera-")) {
            walk(&c.path().join("src"), &mut found, root);
            walk(&c.path().join("tests"), &mut found, root);
        }
    }
    found.sort();
    let got: Vec<(&str, usize)> = found.iter().map(|(a, n, _)| (a.as_str(), *n)).collect();
    // The engine: the trait's declaration and `editor_discard`'s argument.
    let want = [
        ("chimera-core/src/console/ed/project.rs", 1),
        ("chimera-engine/src/confirm.rs", 2),
    ];
    assert_eq!(got, want);
    let word = proof_word();
    let the_impl = format!(
        "impl chimera_engine::confirm::{word}<crate::project::ProjectSource> for Discard {{}}"
    );
    assert!(found[0].2.contains(&the_impl), "{the_impl}");
    let the_trait = format!("pub trait {word}<T: Witnessed> {{}}");
    assert!(found[1].2.contains(&the_trait), "{the_trait}");
}

/// The scan's two old blind spots: an implementor under an alias, and an
/// `impl` split across lines. Comment lines alone are no mention.
#[test]
fn the_proof_scan_sees_an_alias_and_a_split_impl() {
    let word = proof_word();
    let alias = format!(
        "use chimera_engine::confirm::{word} as P;\nimpl P<ProjectSource> for Forged {{}}\n"
    );
    assert_eq!(proof_mentions(&alias), 1);
    let split = format!(
        "impl\n    chimera_engine::confirm::{word}<ProjectSource>\n    for Forged\n{{\n}}\n"
    );
    assert_eq!(proof_mentions(&split), 1);
    let comments = format!("/// `{word}`\n// {word}\n//! {word}\nstruct Plain;\n");
    assert_eq!(proof_mentions(&comments), 0);
}
