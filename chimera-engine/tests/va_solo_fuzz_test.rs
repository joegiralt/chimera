//! VA's saw solo (ADR 0283) fuzzed (`va_fuzz`): every scene plays twice
//! in one tree, the solos taken where they may and then every plain block
//! forced to its pair (`ForcePairs`), and each block's bits must match.
//! The first `PINNED` scenes' digests must also be those origin/0.4
//! `807e740d` played, before the solo: the pairs' own path can't drift
//! either. With bank 2 the scenes open and close its door mid-note; without
//! it (the door absent) they play bank 1's own renders. A process-wide
//! door: one test in its own binary.

mod va_fuzz;

use chimera_engine::dsp::va::Played;
use chimera_engine::dsp::va::osc::SAW_ANCHOR;
use chimera_engine::dsp::va::osc::solo::ForcePairs;

/// Scenes played: about a second in `just check`.
const SCENES: u64 = 400;
/// Scenes pinned to 807e740d's digests.
const PINNED: usize = 64;

#[cfg(feature = "bank2")]
fn door(open: bool) {
    use chimera_engine::bank2::DOOR_CELL;
    if open {
        DOOR_CELL.open_host()
    } else {
        DOOR_CELL.close()
    }
}

/// No door: bank 1 plays everything.
#[cfg(not(feature = "bank2"))]
fn door(_: bool) {}

#[test]
fn solos_play_their_pairs_and_807e740d_bit_for_bit() {
    assert_eq!(va_fuzz::SAW, SAW_ANCHOR.get());
    let mut alone = 0;
    for seed in 0..SCENES {
        let solos = va_fuzz::play(seed, door, |e| {
            alone += e
                .played()
                .iter()
                .filter(|x| **x == Some(Played::Alone))
                .count();
        });
        let pairs = {
            let _pairs = ForcePairs::engage();
            va_fuzz::play(seed, door, |_| {})
        };
        if let Some(b) = solos.iter().zip(&pairs).position(|(a, b)| a != b) {
            panic!("scene {seed}: block {b}: the solo differs from its pair");
        }
        if std::env::var_os("VA_FUZZ_PRINT").is_some() && (seed as usize) < PINNED {
            println!("    0x{:016x},", va_fuzz::digest(&solos));
        } else if let Some(&want) = DIGESTS.get(seed as usize) {
            assert_eq!(
                va_fuzz::digest(&solos),
                want,
                "scene {seed}: not as at 807e740d"
            );
        }
    }
    // the fuzz reaches the solo: a good share of its plain blocks
    assert!(alone > 5_000, "only {alone} solo blocks");
}

/// `va_fuzz::digest` of scenes `0..PINNED` at origin/0.4 `807e740d`,
/// door toggled (`bank2`, `test-support`). Re-taken for AM's corrected
/// product (#660, ADR 0313): the scenes that play AM moved.
#[cfg(feature = "bank2")]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xc6603f02da7758b2,
    0x965cf6ba43a1ac6f,
    0xf57d864f161a6fd3,
    0xb34f2305157ce67c,
    0x789874ed1c619d35,
    0x9404e6e48a6f2583,
    0x74fa72ae7c26393e,
    0xb37079e0715faa85,
    0xdc8950b2c5080582,
    0x88610126cd3b56f8,
    0xc10bc9ab6532988c,
    0x85157cd57d2e941d,
    0xd2f6f52d979bc386,
    0x1ddcc0a7aa98b813,
    0x2b98e4c869ccf767,
    0xf30e9486a3a69b39,
    0x0040c63decbdce81,
    0xad1d3c7fbd546115,
    0xf1f91cf6ad2a2f4d,
    0x30885edd2d3bc4b2,
    0x96053206474e9afa,
    0xf1fbf4f46f0aa3ae,
    0x675ca4214b34fb3f,
    0xda51a4322ba95480,
    0x460e1c47b90c317d,
    0xd2388b326e62b1e3,
    0x2a1deb0b3db9a7e8,
    0x6858c3989d113462,
    0x02cb389be25702f0,
    0x8d704454f2d8286b,
    0x5a87020de2b65dc8,
    0x94cb71187a76f3c7,
    0x37c957af68915806,
    0x37cd94b1c26adc1f,
    0xac3104d257e3c145,
    0x2efc15ba131b3d8f,
    0x6f737824917eaa7a,
    0xf7b7a03aa8faf4b8,
    0x1558714593b5223b,
    0x407a8bc898ab246b,
    0xb483854eb23f3cbc,
    0x5b0999a28f8f308d,
    0x766ff1ffce9bd8c2,
    0x08ef7e53b335f073,
    0xa9f6d210f7e08f89,
    0x08008d8d26d634e3,
    0x0e11ca0b6a0f661f,
    0xc29305530e3f05ff,
    0x5d957295ee16c7d3,
    0x74ff81b57c21acde,
    0x050a76a5cee4d15e,
    0x1eafe51cc5fd16e6,
    0x2c2193dd30b87fa0,
    0x97fecbf44498854b,
    0x73e5747fc4bc4c2e,
    0xd91465686dddea37,
    0xbde697dbe1d8ac18,
    0x7c20238b680e4e8c,
    0xa7cc860ccabeb2f2,
    0xbb26db112397f753,
    0x4e7f825a5fbef847,
    0x01359a5b19500136,
    0x7e2350af8af9eeda,
];

/// `va_fuzz::digest` of scenes `0..PINNED` at origin/0.4 `807e740d`, no
/// door (`test-support`), re-taken as the bank-2 table was (#660).
#[cfg(not(feature = "bank2"))]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xa3ead584ca6479b2,
    0x965cf6ba43a1ac6f,
    0xdca802f2d0febc4d,
    0xb34f2305157ce67c,
    0x1f37012b451cbcca,
    0x9404e6e48a6f2583,
    0x74fa72ae7c26393e,
    0x27dc40258d432826,
    0xed907508d427fe84,
    0x31be7687fdc62f3b,
    0xc10bc9ab6532988c,
    0x85157cd57d2e941d,
    0xae3579d255219b03,
    0x1ddcc0a7aa98b813,
    0x2b98e4c869ccf767,
    0x10e1e184248318b9,
    0x0040c63decbdce81,
    0xad1d3c7fbd546115,
    0xf1f91cf6ad2a2f4d,
    0x6fdde5bb10e5a0f0,
    0x96053206474e9afa,
    0xf1fbf4f46f0aa3ae,
    0xc3381be2fd46daba,
    0xda51a4322ba95480,
    0x73d63b22190389ce,
    0xa21ca6a49b38656f,
    0x872836196c59fcb6,
    0x69a31ea78e1d7329,
    0x938f4379cb2751fc,
    0x8d704454f2d8286b,
    0xc65abcd31cb68cc0,
    0x607509c7f5e29436,
    0x8e1e5bd589698833,
    0x37cd94b1c26adc1f,
    0x6302f8c051d2406f,
    0xcfb47ee68c050696,
    0x6f737824917eaa7a,
    0x7d1ae7c3a2251767,
    0x99a892b39494c545,
    0x407a8bc898ab246b,
    0xd1c05b7fcb1ceee9,
    0x1221749da8feb0e6,
    0x766ff1ffce9bd8c2,
    0x378791dee7c63c28,
    0x65a755d5de511451,
    0xa46cd9f801c3fb00,
    0x0e11ca0b6a0f661f,
    0xc29305530e3f05ff,
    0x6954f455a2e7b07b,
    0x4356c8628abf046e,
    0x050a76a5cee4d15e,
    0x0f7545d6a0ba36c9,
    0x023857833f26e0fd,
    0x9d5232bdf7f5fbb6,
    0x73e5747fc4bc4c2e,
    0xd91465686dddea37,
    0xd1c4f1793e55b2e1,
    0x1d6b6ab259901d8c,
    0x41a02a0c76b3f6e2,
    0x65dd11f68386f528,
    0x98c448b511fb0ef5,
    0x304a22a9ebe2737d,
    0x9986bd54aa7cebf4,
];
