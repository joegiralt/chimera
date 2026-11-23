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
/// door toggled (`bank2`, `test-support`). Re-taken on bugs-d when SYNC's
/// slave started on the master's phase at a MODE swap (#670, ADR 0310):
/// the scenes that change MODE into SYNC moved.
#[cfg(feature = "bank2")]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0x001dc45f33b47647,
    0x965cf6ba43a1ac6f,
    0x53069f467315a07f,
    0xc837799a2b56410b,
    0x789874ed1c619d35,
    0x0b393c26ca8d3e5d,
    0x3a448d7110e10ba4,
    0xaef9677e99de6861,
    0xe11c1e4619dd2f06,
    0xcaec1f30efc8da95,
    0xc10bc9ab6532988c,
    0x9e6ed31a1f9d21e4,
    0x771596ed9eab75a2,
    0x84c2e411c2ca3229,
    0x41eb2768589294b8,
    0xf30e9486a3a69b39,
    0xd3b813cda45929cf,
    0xbc9fe34a3221ccd1,
    0xf1f91cf6ad2a2f4d,
    0x645bcd736c0e07bb,
    0x86e169a4d1da1658,
    0xbd79d75b68aef8a9,
    0x675ca4214b34fb3f,
    0x447fd46fbd15131e,
    0x995b62fd1f266823,
    0xd2388b326e62b1e3,
    0x5d926e90f1fc1f48,
    0x1b568c994b831543,
    0x900f991daea0a7df,
    0xfaf808a6746c527d,
    0x5a87020de2b65dc8,
    0x3f5cf7820236502e,
    0xa83adc60533694a5,
    0x2a2d4a37baba9f83,
    0xac3104d257e3c145,
    0x2efc15ba131b3d8f,
    0x8fc954a850e70f0f,
    0x836e266e13e493eb,
    0x5c44c56cf29fcaf1,
    0x407a8bc898ab246b,
    0xb483854eb23f3cbc,
    0x1f8eece36d8fba4c,
    0x766ff1ffce9bd8c2,
    0x08ef7e53b335f073,
    0x6f2abd768db4c5e4,
    0xbf1743423bbdacc3,
    0x0e11ca0b6a0f661f,
    0xad5de5f3b6da1ce6,
    0x779658f54849385d,
    0xdd87f3dce99337da,
    0xb9343bd7baa2dd6e,
    0x175a4a90386d918e,
    0xeee53294b47b6663,
    0x7107389b3b40d629,
    0x5675a6fca2b0f541,
    0xd91465686dddea37,
    0x731d7021e10e9236,
    0x7c20238b680e4e8c,
    0xa7cc860ccabeb2f2,
    0xad48b49a2552f4ff,
    0x4e07fce77e3da11f,
    0x1b4b456d0ea68f5b,
    0xba69629f3bdd4c60,
];

/// `va_fuzz::digest` of scenes `0..PINNED` at origin/0.4 `807e740d`, no
/// door (`test-support`), re-taken as the bank-2 table was (#670).
#[cfg(not(feature = "bank2"))]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xd0d5fc82260c0dc9,
    0x965cf6ba43a1ac6f,
    0x37992f65046e8f0a,
    0xc837799a2b56410b,
    0x6e7e2d03ff6e6db3,
    0x0b393c26ca8d3e5d,
    0x3a448d7110e10ba4,
    0xd15d480742125cda,
    0xe11c1e4619dd2f06,
    0x289f0bfd8cfb5c52,
    0xc10bc9ab6532988c,
    0x9e6ed31a1f9d21e4,
    0xce61d1bd6a2a013b,
    0x84c2e411c2ca3229,
    0x41eb2768589294b8,
    0xaf1abad4ce0c3a89,
    0xd3b813cda45929cf,
    0xbc9fe34a3221ccd1,
    0xf1f91cf6ad2a2f4d,
    0x613b4c07985040e2,
    0x86e169a4d1da1658,
    0xbd79d75b68aef8a9,
    0xc3381be2fd46daba,
    0x447fd46fbd15131e,
    0x7a62407e8d73f166,
    0xa21ca6a49b38656f,
    0x79f306a64f1fdc8c,
    0x0f30f35ed6fc7253,
    0x900f991daea0a7df,
    0xfaf808a6746c527d,
    0x6e00c5acb23af019,
    0x88dc2c0d788832fd,
    0xf7aabb1b59e774bd,
    0x2a2d4a37baba9f83,
    0x213830f628869060,
    0x0579da769abb9ac6,
    0x8fc954a850e70f0f,
    0x4926580945bd3a24,
    0x57ba472e112b64f7,
    0x407a8bc898ab246b,
    0xca6b9fe15f1c6822,
    0x1f8eece36d8fba4c,
    0x766ff1ffce9bd8c2,
    0x94787481953166ba,
    0x9b7fd7dc274986fb,
    0xa4a225e269a45a63,
    0x0e11ca0b6a0f661f,
    0xad5de5f3b6da1ce6,
    0x0c9f134c2fe2f225,
    0xf3e555cbf9be1d6e,
    0xb9343bd7baa2dd6e,
    0x31965e7264717c92,
    0xeee53294b47b6663,
    0x8e3d9fea86a82bf6,
    0x5675a6fca2b0f541,
    0xd91465686dddea37,
    0x49e82cf202b93b54,
    0x1d6b6ab259901d8c,
    0x41a02a0c76b3f6e2,
    0x317847966b6068a4,
    0x0fc73eedce76255d,
    0xfa94889fa4bb3419,
    0xdb14b02bd042763f,
];
