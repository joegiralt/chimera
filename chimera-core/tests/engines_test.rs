//! `EngineSlot` dispatch and the VCA/activity table (spec §3), one test per row:
//!
//! | Engine | Amp env on VCA | Voice active while     |
//! |--------|----------------|------------------------|
//! | Algo   | no             | a carrier's envelope   |
//! | Modal  | no             | `modal.is_active()`    |
//! | Tape   | no (INIT: ENV 1) | its source reads (a loop has no end) |
//! | Va     | no (INIT: ENV 1) | from its first note on (`va_voice_test`) |

mod common;

use chimera_core::dsp::algo::engine::AlgoLive;
use chimera_core::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use chimera_core::dsp::modal::{ResonatorMode, SymPool};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::source_table::GateView;
use chimera_core::taps::TapRows;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::tape::{OneSource, held_plain, saw_with_burst};
use common::{SR, expects_sound};

fn params(kind: EngineType) -> ParamSnapshot {
    ParamSnapshot::for_engine(kind)
}

fn ctx<'a>(sym: &'a mut SymPool, src: &'a GateView<'a>) -> EngineCtx<'a> {
    EngineCtx {
        sym,
        streams: None,
        src,
        part: chimera_core::project::PartId::ALL[0],
        tune: 0.0,
    }
}

fn render(e: &mut EngineSlot, p: &ParamSnapshot, pool: &mut SymPool, blocks: usize) -> f32 {
    let mut out = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    let none = GateView::empty();
    for _ in 0..blocks {
        let live = AlgoLive::from_params(&p.algo);
        e.render(
            &mut out,
            p,
            &live,
            SR,
            &mut ctx(pool, &none),
            &mut TapRows::new([false; 2]),
        );
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

#[test]
fn all_lists_every_engine_once_in_order() {
    assert_eq!(EngineType::ALL.len(), EngineType::Va as usize + 1);
    for (i, &e) in EngineType::ALL.iter().enumerate() {
        assert_eq!(e as usize, i);
        let _ = expects_sound(e); // exhaustive match: new variants fail to compile
    }
}

#[test]
fn every_engine_renders_per_its_expectation() {
    let mut src = OneSource::new(&held_plain(&saw_with_burst(24_000, 1.0), 24_000));
    for kind in EngineType::ALL {
        let p = params(kind);
        let mut pool = SymPool::boxed();
        let mut e = EngineSlot::new(SlotKind::of(&p));
        let id = Some(src.id);
        let mut peak = 0.0f32;
        let mut out = [0.0f32; BLOCK_SIZE];
        for b in 0..8 {
            let view = src.view();
            let mut c = ctx(&mut pool, &view);
            if b == 0 {
                e.note_on(MidiNote::A4, Velocity::DEFAULT, &p, SR, id, &mut c);
            }
            e.render(
                &mut out,
                &p,
                &AlgoLive::from_params(&p.algo),
                SR,
                &mut c,
                &mut TapRows::new([false; 2]),
            );
            peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        assert!(peak.is_finite());
        assert_eq!(peak > 1e-3, expects_sound(kind), "{kind:?}: peak {peak}");
    }
}

#[test]
fn modal_row_no_amp_env_and_lives_until_modes_decay() {
    let kind = EngineType::Modal;
    let mut p = params(kind);
    p.modal.mode = ResonatorMode::Modal;
    p.modal.damp = 0.0;
    let mut pool = SymPool::boxed();
    let mut e = EngineSlot::new(SlotKind::of(&p));
    assert!(!e.is_active());
    let none = GateView::empty();
    e.note_on(
        MidiNote::A4,
        Velocity::DEFAULT,
        &p,
        SR,
        None,
        &mut ctx(&mut pool, &none),
    );
    assert!(e.is_active());
    e.note_off(&mut ctx(&mut pool, &none), false);
    render(&mut e, &p, &mut pool, 1500);
    assert!(!e.is_active());
}

#[test]
fn algo_row_no_amp_env_and_lives_until_its_carriers_release() {
    let p = params(EngineType::Algo);
    let mut pool = SymPool::boxed();
    let mut e = EngineSlot::new(SlotKind::Algo);
    assert!(!e.is_active());
    let none = GateView::empty();
    e.note_on(
        MidiNote::A4,
        Velocity::DEFAULT,
        &p,
        SR,
        None,
        &mut ctx(&mut pool, &none),
    );
    render(&mut e, &p, &mut pool, 1);
    assert!(e.is_active());
    e.note_off(&mut ctx(&mut pool, &none), false);
    // RR 5: T60 about 1.1 s.
    render(&mut e, &p, &mut pool, 1500);
    assert!(!e.is_active());
}
