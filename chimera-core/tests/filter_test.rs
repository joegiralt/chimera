//! The filter: typed modes (#111) and retired ids (#112).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::filter::{FilterMode, SVF_MODES, SvfFilter};
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::block_registry::{FILTER, FILTER_MODE};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

#[test]
fn mode_is_typed_and_stays_in_the_svf_list() {
    let mut f = FilterParams::default();
    assert_eq!(f.mode(), FilterMode::Lp24, "every Sound is on LP24 today");
    for m in FilterMode::ALL {
        assert!(f.set_mode(m));
        assert_eq!(f.mode(), m);
    }
    // The block value is the index in the SVF's list, so an encoder steps it.
    f.set(FilterParams::MODE, 0.0);
    assert_eq!(f.mode(), SVF_MODES[0]);
    assert_eq!(SVF_MODES[0], FilterMode::Lp24, "the default comes first");
    f.set(FilterParams::MODE, 99.0);
    assert_eq!(f.mode(), FilterMode::Phaser);
    assert_eq!(f.get(FilterParams::MODE), 7.0);
}

#[test]
fn the_discriminants_are_the_old_mode_byte() {
    for (i, m) in FilterMode::ALL.iter().enumerate() {
        assert_eq!(*m as u8, i as u8);
    }
}

#[test]
fn retired_filter_ids_have_no_spec() {
    let p = ParamSnapshot::default();
    for id in [3, 4, 5] {
        assert!(p.filter.spec(ParamId(id)).is_none(), "id {id} is retired");
    }
}

#[test]
fn mode_is_on_the_flt_pages() {
    let mode = ParamAddr::new(BlockRef::Filter, FilterParams::MODE);
    let on = |def| (0..6).any(|i| slot_addr(def, i, chimera_core::addr::Op::A) == Some(mode));
    assert!(on(&FILTER) && on(&FILTER_MODE));
}

/// Each mode filters a saw differently, and all stay finite.
#[test]
fn every_mode_renders_finite_and_distinct() {
    let mut seen = Vec::new();
    for m in FilterMode::ALL {
        let mut p = FilterParams::default();
        p.set_mode(m);
        p.resonance = 0.4;
        let mut f = SvfFilter::new();
        let mut out = Vec::new();
        for b in 0..20 {
            let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                .map(|i| ((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0)
                .collect();
            f.process(&mut buf, &p, SR);
            out.extend(buf);
        }
        assert!(out.iter().all(|x| x.is_finite()), "{m:?}");
        let bits: Vec<u32> = out.iter().map(|x| x.to_bits()).collect();
        assert!(!seen.contains(&bits), "{m:?} renders like another mode");
        seen.push(bits);
    }
}
