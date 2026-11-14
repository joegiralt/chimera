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
        ("ed describe ALGO 0\n", "describe"),
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
