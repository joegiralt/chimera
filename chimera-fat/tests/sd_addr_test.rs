use chimera_fat::sd_addr::*;
use chimera_hal::units::Lba;
use embedded_sdmmc::embedded_sdmmc_types::sdcard::CardType;

#[test]
fn cmd18_arg_addresses_by_card_type() {
    let lba = Lba::new(1_000);
    assert_eq!(
        AddrMode::of(CardType::SdhcSdxc).arg(lba).unwrap().get(),
        1_000
    );
    assert_eq!(AddrMode::of(CardType::SD2).arg(lba).unwrap().get(), 512_000);
    assert_eq!(AddrMode::of(CardType::SD1).arg(lba).unwrap().get(), 512_000);
}

#[test]
fn sdsc_arg_past_4_gib_is_refused() {
    assert_eq!(AddrMode::Byte.arg(Lba::new(1 << 23)), None);
    assert!(AddrMode::Byte.arg(Lba::new((1 << 23) - 1)).is_some());
}

#[test]
fn card_addr_unknown_until_set() {
    let a = CardAddr::new();
    assert_eq!(a.get(), None);
    a.set(AddrMode::Byte);
    assert_eq!(a.get(), Some(AddrMode::Byte));
    a.clear();
    assert_eq!(a.get(), None);
}
