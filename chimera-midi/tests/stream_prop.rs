mod common;
use chimera_midi::stream::*;
use common::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut p = Parser::<8>::new();
        run(&mut p, &bytes);
    }

    #[test]
    fn realtime_insertion_invariant(
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
        inserts in proptest::collection::vec((any::<usize>(), 0xF8u8..=0xFF), 0..16),
    ) {
        let mut with = bytes.clone();
        for (at, rt) in inserts {
            let i = at % (with.len() + 1);
            with.insert(i, rt);
        }
        // F9/FD count as faults in the output, so drop every F8..FF item and compare the rest
        let strip = |v: Vec<Item>| v.into_iter().filter(|i| !matches!(i, Item::Rt(_) | Item::Fault(Fault::Undefined))).collect::<Vec<_>>();
        prop_assert_eq!(strip(parse(&bytes)), strip(parse(&with)));
    }
}

#[derive(Clone, Debug)]
enum Msg {
    On(u8, u8, u8),
    Off(u8, u8, u8),
    Cc(u8, u8, u8),
    Prog(u8, u8),
    Press(u8, u8),
    Bend(u8, u8, u8),
}

fn msg() -> impl Strategy<Value = Msg> {
    let ch = 0u8..16;
    let d = 0u8..128;
    prop_oneof![
        (ch.clone(), d.clone(), 1u8..128).prop_map(|(c, k, v)| Msg::On(c, k, v)),
        (ch.clone(), d.clone(), d.clone()).prop_map(|(c, k, v)| Msg::Off(c, k, v)),
        (ch.clone(), d.clone(), d.clone()).prop_map(|(c, k, v)| Msg::Cc(c, k, v)),
        (ch.clone(), d.clone()).prop_map(|(c, p)| Msg::Prog(c, p)),
        (ch.clone(), d.clone()).prop_map(|(c, p)| Msg::Press(c, p)),
        (ch, d.clone(), d).prop_map(|(c, a, b)| Msg::Bend(c, a, b)),
    ]
}

fn encode(m: &Msg) -> (u8, Vec<u8>) {
    match *m {
        Msg::On(c, a, b) => (0x90 | c, vec![a, b]),
        Msg::Off(c, a, b) => (0x80 | c, vec![a, b]),
        Msg::Cc(c, a, b) => (0xB0 | c, vec![a, b]),
        Msg::Prog(c, a) => (0xC0 | c, vec![a]),
        Msg::Press(c, a) => (0xD0 | c, vec![a]),
        Msg::Bend(c, a, b) => (0xE0 | c, vec![a, b]),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn running_status_equivalence(msgs in proptest::collection::vec(msg(), 0..64)) {
        let mut full = vec![];
        let mut running = vec![];
        let mut last = None;
        for m in &msgs {
            let (s, data) = encode(m);
            full.push(s);
            full.extend(&data);
            if last != Some(s) {
                running.push(s);
            }
            running.extend(&data);
            last = Some(s);
        }
        prop_assert_eq!(parse(&full), parse(&running));
    }
}
