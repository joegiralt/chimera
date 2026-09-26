//! Spec § Waves: the generated tables are band-limited per mip, fit their
//! flash budget, and `WaveId` names them.

use chimera_core::dsp::algo::waves::{
    MIPS, Table, WAVE_COUNT, WAVE_FLASH_BUDGET, WAVE_LEN, WAVES, WaveId,
};
use std::f64::consts::TAU;

fn magnitude(t: &Table, k: usize) -> f64 {
    let (mut a, mut b) = (0.0, 0.0);
    for (n, &v) in t.iter().enumerate() {
        let ph = TAU * (k * n) as f64 / WAVE_LEN as f64;
        a += v as f64 * ph.cos();
        b += v as f64 * ph.sin();
    }
    a.hypot(b)
}

#[test]
fn every_mip_is_band_limited() {
    for w in 0..WAVE_COUNT as u8 {
        let id = WaveId::clamped(w);
        for mip in 0..MIPS {
            let keep = (128 >> mip).min(127);
            let t = id.table(mip);
            let peak = (1..=keep).map(|k| magnitude(t, k)).fold(0.0, f64::max);
            let above = (keep + 1..=WAVE_LEN / 2)
                .map(|k| magnitude(t, k))
                .fold(0.0, f64::max);
            assert!(
                above < peak * 1e-3,
                "{} mip {mip}: {:.1} dB above its band",
                id.name(),
                20.0 * (above / peak).log10()
            );
        }
    }
}

#[test]
fn the_tables_fit_their_flash_budget() {
    let bytes = core::mem::size_of_val(&WAVES);
    assert_eq!(bytes, WAVE_COUNT * MIPS * WAVE_LEN * 2);
    assert!(bytes <= WAVE_FLASH_BUDGET, "{bytes} B");
}

#[test]
fn wave_ids_name_their_tables() {
    assert_eq!(WaveId::W1.name(), "W1");
    assert_eq!(WaveId::SAW.name(), "SAW");
    assert_eq!(WaveId::SSAW.name(), "SSAW");
    assert_eq!(WaveId::clamped(200), WaveId::SSAW);
    assert_eq!(WaveId::clamped(3).get(), 3);
    assert!(core::ptr::eq(
        WaveId::W1.table(99),
        WaveId::W1.table(MIPS - 1)
    ));
}
