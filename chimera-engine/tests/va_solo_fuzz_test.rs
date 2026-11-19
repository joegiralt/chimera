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
        if let Some(&want) = DIGESTS.get(seed as usize) {
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
/// door toggled (`bank2`, `test-support`).
#[cfg(feature = "bank2")]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0x001dc45f33b47647,
    0x965cf6ba43a1ac6f,
    0xf57d864f161a6fd3,
    0xb34f2305157ce67c,
    0x789874ed1c619d35,
    0x0b393c26ca8d3e5d,
    0x74fa72ae7c26393e,
    0xb37079e0715faa85,
    0xe11c1e4619dd2f06,
    0xb0729a10ed61610f,
    0xc10bc9ab6532988c,
    0x9e6ed31a1f9d21e4,
    0xd2f6f52d979bc386,
    0x84c2e411c2ca3229,
    0x2b98e4c869ccf767,
    0xf30e9486a3a69b39,
    0x523609b269d867c4,
    0xad1d3c7fbd546115,
    0xf1f91cf6ad2a2f4d,
    0x30885edd2d3bc4b2,
    0x25ff568c71a5f03a,
    0xd66bd57a5651ee8a,
    0x675ca4214b34fb3f,
    0x447fd46fbd15131e,
    0x460e1c47b90c317d,
    0xd2388b326e62b1e3,
    0x2a1deb0b3db9a7e8,
    0x1b568c994b831543,
    0x900f991daea0a7df,
    0xe79ac888b64d893a,
    0x5a87020de2b65dc8,
    0x0ba845baac65e5f4,
    0x37c957af68915806,
    0x2a2d4a37baba9f83,
    0xac3104d257e3c145,
    0x2efc15ba131b3d8f,
    0x8fc954a850e70f0f,
    0xf7b7a03aa8faf4b8,
    0x5c44c56cf29fcaf1,
    0x407a8bc898ab246b,
    0xb483854eb23f3cbc,
    0xa31a08aa23b117f2,
    0x766ff1ffce9bd8c2,
    0x08ef7e53b335f073,
    0xa9f6d210f7e08f89,
    0x3ed891b9541ada68,
    0x0e11ca0b6a0f661f,
    0xc29305530e3f05ff,
    0x5d957295ee16c7d3,
    0xdd87f3dce99337da,
    0x975c7a9b1c11efb1,
    0x175a4a90386d918e,
    0xeee53294b47b6663,
    0x97fecbf44498854b,
    0x5675a6fca2b0f541,
    0xd91465686dddea37,
    0x731d7021e10e9236,
    0x7c20238b680e4e8c,
    0xa7cc860ccabeb2f2,
    0xbb26db112397f753,
    0x4e07fce77e3da11f,
    0x01359a5b19500136,
    0xba69629f3bdd4c60,
];

/// `va_fuzz::digest` of scenes `0..PINNED` at origin/0.4 `807e740d`, no
/// door (`test-support`).
#[cfg(not(feature = "bank2"))]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xd0d5fc82260c0dc9,
    0x965cf6ba43a1ac6f,
    0xdca802f2d0febc4d,
    0xb34f2305157ce67c,
    0x6e7e2d03ff6e6db3,
    0x0b393c26ca8d3e5d,
    0x74fa72ae7c26393e,
    0x27dc40258d432826,
    0xe11c1e4619dd2f06,
    0x592bcfc94a9e3498,
    0xc10bc9ab6532988c,
    0x9e6ed31a1f9d21e4,
    0xae3579d255219b03,
    0x84c2e411c2ca3229,
    0x2b98e4c869ccf767,
    0xaf1abad4ce0c3a89,
    0x523609b269d867c4,
    0xad1d3c7fbd546115,
    0xf1f91cf6ad2a2f4d,
    0x6fdde5bb10e5a0f0,
    0x25ff568c71a5f03a,
    0xd66bd57a5651ee8a,
    0xc3381be2fd46daba,
    0x447fd46fbd15131e,
    0x2225be1e9b01e2e0,
    0xa21ca6a49b38656f,
    0x9b289cb2f3a019af,
    0x0f30f35ed6fc7253,
    0x900f991daea0a7df,
    0xe79ac888b64d893a,
    0x6e00c5acb23af019,
    0xc1b352a3daa37a03,
    0x8e1e5bd589698833,
    0x2a2d4a37baba9f83,
    0x213830f628869060,
    0x0579da769abb9ac6,
    0x8fc954a850e70f0f,
    0x7d1ae7c3a2251767,
    0x57ba472e112b64f7,
    0x407a8bc898ab246b,
    0xca6b9fe15f1c6822,
    0xa31a08aa23b117f2,
    0x766ff1ffce9bd8c2,
    0x94787481953166ba,
    0x65a755d5de511451,
    0x805fba7e716152a8,
    0x0e11ca0b6a0f661f,
    0xc29305530e3f05ff,
    0x6954f455a2e7b07b,
    0xf3e555cbf9be1d6e,
    0x975c7a9b1c11efb1,
    0x31965e7264717c92,
    0xeee53294b47b6663,
    0x4c22c827f4b75a7a,
    0x5675a6fca2b0f541,
    0xd91465686dddea37,
    0x49e82cf202b93b54,
    0x1d6b6ab259901d8c,
    0x41a02a0c76b3f6e2,
    0x65dd11f68386f528,
    0x0fc73eedce76255d,
    0x1099d62a886835b4,
    0xa646966e27d3f2d5,
];
