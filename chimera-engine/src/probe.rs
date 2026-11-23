// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bench-only phase stamps: a cycle clock the target hands in, a stamp
//! per phase boundary, and accumulators for costs a block repeats.

use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering::Relaxed};

pub const STAMPS: usize = 16;
pub const ACCS: usize = 12;

static CLOCK: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
pub static AT: [AtomicU32; STAMPS] = [const { AtomicU32::new(0) }; STAMPS];
/// Which stamps this block took.
pub static SEEN: AtomicU32 = AtomicU32::new(0);
pub static ACC: [AtomicU32; ACCS] = [const { AtomicU32::new(0) }; ACCS];

pub fn set_clock(f: fn() -> u32) {
    CLOCK.store(f as *mut (), Relaxed);
}

#[inline(always)]
pub fn now() -> u32 {
    let p = CLOCK.load(Relaxed);
    if p.is_null() {
        return 0;
    }
    // SAFETY: only `set_clock` stores a non-null pointer, a `fn() -> u32`.
    let f: fn() -> u32 = unsafe { core::mem::transmute(p) };
    f()
}

/// A new block: the stamps and accumulators cleared, stamp 0 taken.
#[inline(always)]
pub fn begin() {
    for a in &ACC {
        a.store(0, Relaxed);
    }
    SEEN.store(1, Relaxed);
    AT[0].store(now(), Relaxed);
}

#[inline(always)]
pub fn stamp(i: usize) {
    AT[i].store(now(), Relaxed);
    SEEN.fetch_or(1 << i, Relaxed);
}

#[inline(always)]
pub fn acc(i: usize, cycles: u32) {
    ACC[i].fetch_add(cycles, Relaxed);
}

#[inline(always)]
pub fn max(i: usize, cycles: u32) {
    ACC[i].fetch_max(cycles, Relaxed);
}
