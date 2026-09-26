use chimera_core::clock_plan::{
    Pll3Config, PllRange, SYSTICK_MAX_RELOAD, SiliconRev, VcoRange, cycles_for_ns, cycles_for_us,
    fs_of, pll3_for, systick_reload, vco_hz,
};

const HSE: u32 = 8_000_000;
const FS: u32 = 48_000;

fn ppm(fs: f64) -> f64 {
    (fs - FS as f64) / FS as f64 * 1e6
}

#[test]
fn pll3_plan_for_both_revisions() {
    let v = pll3_for(HSE, FS, SiliconRev::V);
    assert_eq!(
        v,
        Pll3Config {
            m: 1,
            n: 49,
            fracn: 1245,
            p: 8,
            range: PllRange::R8To16,
            vco: VcoRange::Wide,
            mckdiv: 4
        }
    );
    let y = pll3_for(HSE, FS, SiliconRev::Y);
    assert_eq!(y, Pll3Config { mckdiv: 2, ..v });
}

#[test]
fn both_revisions_land_within_10_ppm_of_48_khz() {
    for rev in [SiliconRev::V, SiliconRev::Y] {
        let fs = fs_of(&pll3_for(HSE, FS, rev), HSE, rev);
        assert!(ppm(fs).abs() < 10.0, "{rev:?}: {fs} Hz ({} ppm)", ppm(fs));
        assert!(ppm(fs).abs() < 1.0, "{rev:?}: {fs} Hz");
    }
}

#[test]
fn every_field_is_in_range() {
    for rev in [SiliconRev::V, SiliconRev::Y] {
        let c = pll3_for(HSE, FS, rev);
        assert!((1..=63).contains(&c.m), "{rev:?} DIVM3 {}", c.m);
        assert!((4..=512).contains(&c.n), "{rev:?} DIVN3 {}", c.n);
        assert!((1..=128).contains(&c.p), "{rev:?} DIVP3 {}", c.p);
        assert!(c.fracn < 8192, "{rev:?} FRACN3 {}", c.fracn);
        let ref_hz = HSE / c.m as u32;
        let band = match c.range {
            PllRange::R1To2 => 1_000_000..=2_000_000,
            PllRange::R2To4 => 2_000_000..=4_000_000,
            PllRange::R4To8 => 4_000_000..=8_000_000,
            PllRange::R8To16 => 8_000_000..=16_000_000,
        };
        assert!(
            band.contains(&ref_hz),
            "{rev:?} ref {ref_hz} outside {:?}",
            c.range
        );
        assert_eq!(c.vco, VcoRange::Wide);
        let vco = vco_hz(&c, HSE);
        assert!((192e6..=836e6).contains(&vco), "{rev:?} VCO {vco}");
        let mckdiv_max = if rev.new_sai() { 63 } else { 15 };
        assert!(
            (1..=mckdiv_max).contains(&c.mckdiv),
            "{rev:?} MCKDIV {}",
            c.mckdiv
        );
    }
}

#[test]
fn fs_of_reproduces_stock_and_the_old_firmware() {
    let stock = Pll3Config {
        m: 1,
        n: 46,
        fracn: 0,
        p: 3,
        range: PllRange::R8To16,
        vco: VcoRange::Wide,
        mckdiv: 10,
    };
    assert!((fs_of(&stock, HSE, SiliconRev::V) - 47_916.667).abs() < 0.01);
    let old = Pll3Config { mckdiv: 5, ..stock };
    assert!((fs_of(&old, HSE, SiliconRev::V) - 95_833.333).abs() < 0.01);
    assert!((fs_of(&old, HSE, SiliconRev::Y) - 47_916.667).abs() < 0.01);
}

#[test]
fn rev_ids_map_to_revisions() {
    assert_eq!(SiliconRev::from_rev_id(0x2003), SiliconRev::V);
    assert_eq!(SiliconRev::from_rev_id(0x1003), SiliconRev::Y);
    assert_eq!(
        (SiliconRev::V.cpu_hz(), SiliconRev::Y.cpu_hz()),
        (480_000_000, 400_000_000)
    );
    assert!(SiliconRev::V.new_sai() && !SiliconRev::Y.new_sai());
    assert_eq!((SiliconRev::V.label(), SiliconRev::Y.label()), ("V", "Y"));
}

#[test]
fn unknown_revisions_fall_back_to_400_mhz_and_sai_by_rev_id() {
    let x = SiliconRev::from_rev_id(0x2001);
    assert_eq!(x, SiliconRev::Unknown(0x2001));
    assert_eq!(
        (x.cpu_hz(), x.new_sai(), x.label()),
        (400_000_000, true, "?")
    );
    let blank = SiliconRev::from_rev_id(0x0000);
    assert_eq!((blank.cpu_hz(), blank.new_sai()), (400_000_000, false));
    assert_eq!(pll3_for(HSE, FS, x).mckdiv, 4);
    assert_eq!(pll3_for(HSE, FS, blank).mckdiv, 2);
}

#[test]
fn cycle_helpers() {
    assert_eq!(cycles_for_us(480_000_000, 250_000), 120_000_000);
    assert_eq!(cycles_for_us(400_000_000, 1), 400);
    assert_eq!(cycles_for_ns(400_000_000, 250), 100);
    assert_eq!(cycles_for_ns(480_000_000, 250), 120);
    assert_eq!(cycles_for_ns(480_000_000, 1), 1, "rounds up, never 0");
}

#[test]
fn systick_reload_hits_500_hz_within_24_bits() {
    assert_eq!(systick_reload(480_000_000, 500), 959_999);
    assert_eq!(systick_reload(400_000_000, 500), 799_999);
    assert!(systick_reload(480_000_000, 500) <= SYSTICK_MAX_RELOAD);
}
