use chimera_core::boot::{BootAction, DFU_MAGIC, ROM_DFU_BASE, after_reset};

#[test]
fn only_the_magic_enters_dfu() {
    assert_eq!(after_reset(DFU_MAGIC), BootAction::RomDfu);
    for m in [
        0,
        u32::MAX,
        DFU_MAGIC ^ 1,
        DFU_MAGIC.swap_bytes(),
        DFU_MAGIC.rotate_left(8),
    ] {
        assert_eq!(after_reset(m), BootAction::Synth, "{m:#010x}");
    }
}

#[test]
fn a_random_marker_boots_the_synth() {
    let mut x = 0x5eed_cafe_u32; // xorshift32
    for _ in 0..1_000_000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x != DFU_MAGIC {
            assert_eq!(after_reset(x), BootAction::Synth, "{x:#010x}");
        }
    }
}

#[test]
fn the_rom_loader_is_an2606s() {
    assert_eq!(ROM_DFU_BASE, 0x1FF0_9800); // STM32H74x/75x system memory
}
