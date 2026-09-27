//! Spec § Waves: a target copies the tables into faster RAM once, and every
//! read goes there after. Its own test binary: the copy is process-wide.

use chimera_core::dsp::algo::waves::{WAVES, WaveId, Waves, copy_into};
use core::mem::MaybeUninit;

#[test]
fn after_the_copy_tables_are_read_from_it() {
    let before = WaveId::SAW.table(3);
    assert!(core::ptr::eq(before, &WAVES[9][3]));
    let ram: &'static mut MaybeUninit<Waves> = Box::leak(Box::new_uninit());
    let at = ram.as_ptr() as usize;
    copy_into(ram);
    for (w, mips) in WAVES.iter().enumerate() {
        for (mip, flash) in mips.iter().enumerate() {
            let t = WaveId::clamped(w as u8).table(mip);
            let offset = t.as_ptr() as usize - at;
            assert!(offset < core::mem::size_of::<Waves>());
            assert_eq!(t, flash);
        }
    }
}
