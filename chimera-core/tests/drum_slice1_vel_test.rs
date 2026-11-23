// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! At VEL 1 a pad's law is slice 1's own (v / 127, the kernel's level,
//! before the filter; ADR 0269): the slice-1 fixture with every pad on
//! VEL 1 renders slice 1's golden, from before the VEL ruling re-recorded
//! it, bit for bit. Its own binary: hit seeds are global, so the render
//! must be the process's first.

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
                    0x761f40bc2378fa6e,
                    [
                        744912150, 1015558105, 1021275839, 994768360, 3152329701, 3152327064,
                        3125714216, 973063012,
                    ],
                ),
            ),
            (
                "drum_slice1_kit_lr",
                (
                    0xd7862cb238b8a8de,
                    [
                        0, 3147991076, 3169244237, 1015513075, 965131471, 958524652, 3148130774,
                        3147266680,
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
