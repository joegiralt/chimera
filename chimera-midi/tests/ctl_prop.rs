mod common;
use chimera_midi::ctl::*;
use common::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        hear_bytes(&mut Controllers::new(), &bytes);
    }

    #[test]
    fn channel_isolation(
        a in proptest::collection::vec((0u8..=127, 0u8..=127), 0..64),
        b in proptest::collection::vec((0u8..=119, 0u8..=127), 0..64),
        mix in proptest::collection::vec(any::<bool>(), 0..128),
    ) {
        // Whole three-byte messages on channel 0 (A) and channel 1 (B), merged by `mix`.
        let msg = |ch: u8, (n, v): (u8, u8)| [0xB0 | ch, n, v];
        let alone: Vec<_> = a.iter().flat_map(|&m| msg(0, m)).collect();
        let (mut ia, mut ib) = (a.iter(), b.iter());
        let mut mixed = Vec::new();
        for take_b in mix {
            let next = if take_b { ib.next().map(|&m| msg(1, m)) } else { ia.next().map(|&m| msg(0, m)) };
            mixed.extend(next.into_iter().flatten());
        }
        mixed.extend(ia.flat_map(|&m| msg(0, m)));
        let only_a = |v: Vec<(u8, Heard)>| v.into_iter().filter(|x| x.0 == 0).collect::<Vec<_>>();
        let solo = only_a(hear_bytes(&mut Controllers::new(), &alone));
        let both = only_a(hear_bytes(&mut Controllers::new(), &mixed));
        prop_assert_eq!(solo, both);
    }
}
