//! The FLT pages are honest (filter-routing spec § Tests "Knobs are
//! honest", "Route knobs").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModSource;
use chimera_core::params::EngineType;
use chimera_core::params::FilterParams;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::renderer::amount_of;
use chimera_core::ui::{PrimeStatus, UiState};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId};
use screen::*;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const ENC: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

fn flt_node(ct: EngineType) -> usize {
    chain_def_for(ct)
        .blocks
        .iter()
        .position(|b| b.def.id == FILTER.id)
        .expect("FLT is on every Part chain")
}

/// Part 1 on `ct`'s init Sound with a held saw worth filtering, on FLT.
fn on_flt(ct: EngineType) -> UiState {
    let mut ui = UiState::new();
    load_init(&mut ui, ct);
    let p = ui.params_mut();
    p.filter.cutoff = 2000.0;
    p.algo.ops[0].wave = WaveId::SAW.get();
    for _ in 0..flt_node(ct) {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

/// 32 blocks of note 72 (so KEY moves the cutoff) from the UI's Sound.
fn render(ui: &UiState) -> Vec<f32> {
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::DEFAULT, ui.params());
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..32 {
        v.render(&mut b, ui.params(), ui.mod_state());
        out.extend_from_slice(&b);
    }
    out
}

/// Every bound FLT and FLT › MODE knob changes a held note on Algo and
/// Modal once turned (a route knob once its route is nonzero).
#[test]
fn every_flt_knob_changes_a_held_note() {
    for ct in EngineType::ALL {
        for (sub, slots) in [(0, &[1usize, 2, 3, 4, 5][..]), (1, &[0usize, 1, 2][..])] {
            for &slot in slots {
                let mut ui = on_flt(ct);
                if sub == 1 {
                    feed(&mut ui, Input::press(ButtonId::Edit));
                }
                let before = render(&ui);
                feed(&mut ui, Input::turn(ENC[slot], 40));
                assert_ne!(render(&ui), before, "{ct:?} page {sub} slot {slot}");
            }
        }
    }
}

/// Removes CUTOFF from Part 1's matrix and reloads it (B1 snaps home).
fn without_cutoff(ui: &mut UiState, fill: bool) {
    let sound = &mut ui.performance.parts[0].sound;
    sound.dest_registry.remove(CUTOFF);
    if fill {
        for b in BlockRef::ALL {
            for s in b.specs() {
                let a = ParamAddr::new(b, s.id);
                if a != CUTOFF && a.modulatable() {
                    let _ = sound.dest_registry.add(a, *b"FILL\0\0\0\0");
                }
            }
        }
    }
    // The audio-side matrix follows the registry (no routes kept).
    sound.mod_state = chimera_core::modulation::ModState::from_registry(&sound.dest_registry, 8);
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

#[test]
fn a_route_knob_creates_its_column() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, false);
    let col =
        |ui: &UiState| (0..ui.mod_state().num_dests()).find(|&d| ui.mod_state().dest(d) == CUTOFF);
    assert_eq!(col(&ui), None);
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // ENV
    let d = col(&ui).expect("the turn created CUTOFF's column");
    assert_eq!(ui.mod_state().amount(ModSource::Env1.index(), d), 5);
    // The knob and the matrix cell show the same amount.
    settle(&mut ui);
    assert_eq!(amount_of(ui.renderer.anim[4].current()), 5);
}

#[test]
fn a_full_matrix_keeps_the_route_knob_off() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, true);
    feed(&mut ui, Input::turn(EncoderId::F, 5)); // KEY
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Full));
    assert!((0..ui.mod_state().num_dests()).all(|d| ui.mod_state().dest(d) != CUTOFF));
}
