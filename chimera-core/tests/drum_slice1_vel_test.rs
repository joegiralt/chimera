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
                    0xc2a1c68cb4e18524,
                    [
                        744912150, 1015558077, 1021275702, 994767959, 3152327482, 3152324800,
                        3125716229, 973063495,
                    ],
                ),
            ),
            (
                "drum_slice1_kit_lr",
                (
                    0x5e45c7723c3b2d49,
                    [
                        0, 3147987561, 3169244756, 1015513240, 965258014, 958784398, 3148135933,
                        3147268870,
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
