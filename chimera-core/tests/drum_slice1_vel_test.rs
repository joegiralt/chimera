// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! At VEL 1 a pad's law is slice 1's own (v / 127, the kernel's level,
//! before the filter; ADR 0269): the slice-1 fixture with every pad on
//! VEL 1 renders slice 1's golden, from before the VEL ruling re-recorded
//! it, bit for bit, but where a pad's STRIKE runs: slice 1 struck at
//! the velocity on top of the kernel's level, the law twice, and a hit now
//! strikes at full height (#963, ADR 0316; re-recorded). Its own binary:
//! hit seeds are global, so the render must be the process's first.

mod common;

use common::*;

#[test]
fn drum_slice1_at_vel_1_is_slice_1() {
    use chimera_core::dsp::Unit;
    use chimera_core::dsp::drum::types::PadId;
    use chimera_core::project::PartId;
    let (out, lr) = common::drum_slice1::render(|p| {
        for part in PartId::ALL {
            for pad in PadId::ALL {
                let a = p.kit_mut(part).pad_mut(pad).sound_mut().amp_mut();
                a.set_vel(Unit::ONE);
            }
        }
    });
    golden::check(
        &[
            (
                "drum_slice1_kit",
                (
                    0xcb96f64f73aefdaa,
                    [
                        744912150, 1015558077, 1021473894, 994788127, 3152333225, 3152330541,
                        3125716523, 973063543,
                    ],
                ),
            ),
            (
                "drum_slice1_kit_lr",
                (
                    0x4d4eb14cbe6e7cc3,
                    [
                        0, 3147987561, 3169244927, 1015640034, 965129506, 958479304, 3148154150,
                        3147272863,
                    ],
                ),
            ),
        ],
        &[
            ("drum_slice1_kit", (fnv1a(&out), spots(&out))),
            ("drum_slice1_kit_lr", (fnv1a(&lr), spots(&lr))),
        ],
    );
}
