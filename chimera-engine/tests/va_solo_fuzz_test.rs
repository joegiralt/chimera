// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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
/// the scenes that change MODE into SYNC moved; and for AM's corrected
/// product (#660, ADR 0321): the scenes that play AM.
#[cfg(feature = "bank2")]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xc6603f02da7758b2,
    0x965cf6ba43a1ac6f,
    0x53069f467315a07f,
    0xc837799a2b56410b,
    0x789874ed1c619d35,
    0x9404e6e48a6f2583,
    0x3a448d7110e10ba4,
    0xaef9677e99de6861,
    0xdc8950b2c5080582,
    0xd0cf174e99804312,
    0xc10bc9ab6532988c,
    0x85157cd57d2e941d,
    0x771596ed9eab75a2,
    0x1ddcc0a7aa98b813,
    0x41eb2768589294b8,
    0xf30e9486a3a69b39,
    0x5b62828a0210f2e6,
    0xbc9fe34a3221ccd1,
    0xf1f91cf6ad2a2f4d,
    0x645bcd736c0e07bb,
    0xcaba6406d6f35af1,
    0x15311718e27d0e81,
    0x675ca4214b34fb3f,
    0xda51a4322ba95480,
    0x995b62fd1f266823,
    0xd2388b326e62b1e3,
    0x5d926e90f1fc1f48,
    0x6858c3989d113462,
    0x02cb389be25702f0,
    0x087fd415064ce0aa,
    0x5a87020de2b65dc8,
    0xb48bc1368a940dc1,
    0xa83adc60533694a5,
    0x37cd94b1c26adc1f,
    0xac3104d257e3c145,
    0x2efc15ba131b3d8f,
    0x6f737824917eaa7a,
    0x836e266e13e493eb,
    0x1558714593b5223b,
    0x407a8bc898ab246b,
    0xb483854eb23f3cbc,
    0xf3f901589b7cd100,
    0x766ff1ffce9bd8c2,
    0x08ef7e53b335f073,
    0x6f2abd768db4c5e4,
    0xe9719702cfb02690,
    0x0e11ca0b6a0f661f,
    0xad5de5f3b6da1ce6,
    0x779658f54849385d,
    0x74ff81b57c21acde,
    0x32f02ea8a1eea77f,
    0x1eafe51cc5fd16e6,
    0x2c2193dd30b87fa0,
    0x7107389b3b40d629,
    0x73e5747fc4bc4c2e,
    0xd91465686dddea37,
    0xbde697dbe1d8ac18,
    0x7c20238b680e4e8c,
    0xa7cc860ccabeb2f2,
    0xad48b49a2552f4ff,
    0x4e7f825a5fbef847,
    0x1b4b456d0ea68f5b,
    0x7e2350af8af9eeda,
];

/// `va_fuzz::digest` of scenes `0..PINNED` at origin/0.4 `807e740d`, no
/// door (`test-support`), re-taken as the bank-2 table was (#670).
#[cfg(not(feature = "bank2"))]
const DIGESTS: [u64; PINNED] = [
    0xda5eda69b4c38822,
    0xa3ead584ca6479b2,
    0x965cf6ba43a1ac6f,
    0x37992f65046e8f0a,
    0xc837799a2b56410b,
    0x1f37012b451cbcca,
    0x9404e6e48a6f2583,
    0x3a448d7110e10ba4,
    0xd15d480742125cda,
    0xed907508d427fe84,
    0x9066bf3aabd01911,
    0xc10bc9ab6532988c,
    0x85157cd57d2e941d,
    0xce61d1bd6a2a013b,
    0x1ddcc0a7aa98b813,
    0x41eb2768589294b8,
    0x10e1e184248318b9,
    0x5b62828a0210f2e6,
    0xbc9fe34a3221ccd1,
    0xf1f91cf6ad2a2f4d,
    0x613b4c07985040e2,
    0xcaba6406d6f35af1,
    0x15311718e27d0e81,
    0xc3381be2fd46daba,
    0xda51a4322ba95480,
    0xb8d31ca0abea6814,
    0xa21ca6a49b38656f,
    0x9da5c7ce9d50c422,
    0x69a31ea78e1d7329,
    0x938f4379cb2751fc,
    0x087fd415064ce0aa,
    0xc65abcd31cb68cc0,
    0x8bc4a523332fa61e,
    0xf7aabb1b59e774bd,
    0x37cd94b1c26adc1f,
    0x6302f8c051d2406f,
    0xcfb47ee68c050696,
    0x6f737824917eaa7a,
    0x4926580945bd3a24,
    0x99a892b39494c545,
    0x407a8bc898ab246b,
    0xd1c05b7fcb1ceee9,
    0xd08212566036d200,
    0x766ff1ffce9bd8c2,
    0x378791dee7c63c28,
    0x9b7fd7dc274986fb,
    0xf10fac33c3e249ca,
    0x0e11ca0b6a0f661f,
    0xad5de5f3b6da1ce6,
    0x0c9f134c2fe2f225,
    0x4356c8628abf046e,
    0x32f02ea8a1eea77f,
    0x0f7545d6a0ba36c9,
    0x023857833f26e0fd,
    0xff548a9162d79dfa,
    0x73e5747fc4bc4c2e,
    0xd91465686dddea37,
    0xd1c4f1793e55b2e1,
    0x1d6b6ab259901d8c,
    0x41a02a0c76b3f6e2,
    0x317847966b6068a4,
    0x98c448b511fb0ef5,
    0x6ebc0c55c61d1fa0,
    0x244f0a7de71a8fc0,
];
