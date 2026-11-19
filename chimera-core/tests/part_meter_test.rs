//! MIXER › PRT's meters (ADR 0293): each Part's bus peak after its EQ and
//! before LEVEL, measured only while the page is up and never touching the
//! sound; the key mark from the held keys on the Part's channel (an MPE
//! zone's members its master's); the meter's bar and peak mark beside
//! each LEVEL bar. One test a global (`PART_PEAKS`, `played`, the watch).

mod common;
mod screen;

use chimera_core::hw::SampleBudget;
use chimera_core::mpe::Zones;
use chimera_core::part_peaks::{self, PART_PEAKS, Peak, Watch};
use chimera_core::project::PartId;

const P2: PartId = PartId::ALL[1];
const P3: PartId = PartId::ALL[2];
use chimera_core::ui::part_meter::{self, Ms, PartMeters, Shown};
use chimera_core::ui::{UiState, theme, viz};
use chimera_core::{MidiChannel, played};
use chimera_hal::{ButtonId, EncoderId};
use common::InstRig;
use screen::*;

/// The meters, the watch and the held keys are process-wide: one test at
/// a time.
static GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    GLOBALS.lock().unwrap_or_else(|e| e.into_inner())
}

fn rig() -> InstRig {
    InstRig::with_budget(SampleBudget::for_cpu(u32::MAX))
}

/// A note on Part 1 for `blocks`, LEVEL at `level`, the meters watched or
/// not: what the pairs played and what Part 1's meter read.
fn play(level: f32, watched: bool, blocks: usize) -> (Vec<f32>, f32) {
    part_peaks::watch(if watched { Watch::Up } else { Watch::Down });
    PART_PEAKS.iter().for_each(|m| {
        m.take();
    });
    let mut r = rig();
    r.shared.parts[0].mix.level = level;
    r.note_on(0, 60);
    let mut out = Vec::new();
    for _ in 0..blocks {
        r.render(1);
        out.extend(r.out()[0].iter());
    }
    let peak = PART_PEAKS[0].take().get();
    assert_eq!(PART_PEAKS[1].take(), Peak::ZERO, "Part 2 played nothing");
    (out, peak)
}

#[test]
fn the_meter_reads_the_bus_before_level_and_leaves_the_sound_alone() {
    let _g = lock();
    let (quiet, unwatched) = play(1.0, false, 30);
    assert_eq!(unwatched, 0.0, "not shown, not measured");
    let (heard, peak) = play(1.0, true, 30);
    assert!(peak > 0.01, "a note reads: {peak}");
    assert!(
        quiet
            .iter()
            .zip(&heard)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "measuring changes no sample"
    );
    let (silent, faded) = play(0.0, true, 30);
    assert!(silent.iter().all(|&x| x == 0.0), "LEVEL 0 is silent");
    assert_eq!(faded, peak, "the meter is before LEVEL");
    part_peaks::watch(Watch::Down);
}

/// The key mark: Part 1's channel holds a key; a member of the lower MPE
/// zone holds for its master, channel 1.
#[test]
fn a_held_key_marks_its_channel_and_its_zones_master() {
    let _g = lock();
    played::clear_held();
    let ch = |c| MidiChannel::new(c).unwrap();
    let mut r = rig();
    assert!(!part_meter::held_on(ch(0), Zones::OFF));
    r.note_on(0, 60);
    assert!(part_meter::held_on(ch(0), Zones::OFF));
    assert!(!part_meter::held_on(ch(1), Zones::OFF));
    r.note_off(0, 60);
    assert!(!part_meter::held_on(ch(0), Zones::OFF));
    played::note_on(3, chimera_core::project::PartId::ALL[0], 62, 100, 62, 0);
    let lower = Zones::OFF.with_lower(4);
    assert!(
        part_meter::held_on(ch(0), lower),
        "channel 4 is a member of 1's zone"
    );
    assert!(
        !part_meter::held_on(ch(0), Zones::OFF),
        "without the zone, 4 is its own"
    );
    played::clear_held();
}

/// The UI's frame: up, each Part's reading taken; down, the meters rest.
#[test]
fn the_frame_takes_readings_while_shown_and_rests_when_not() {
    let _g = lock();
    played::clear_held();
    PART_PEAKS.iter().for_each(|m| {
        m.take();
    });
    let ui = UiState::new();
    let parts = ui.project().perf().parts();
    let mut m = PartMeters::REST;
    PART_PEAKS[2].raise(Peak::new(0.5));
    let on = |m: &mut PartMeters| {
        part_meter::frame(
            m,
            Shown::On {
                parts,
                zones: Zones::OFF,
                dt: Ms(50),
            },
        )
    };
    on(&mut m);
    assert!(part_peaks::watched());
    let bar = m.get(P3).bar().get();
    assert!((bar + 6.02).abs() < 0.01, "{bar}");
    assert_eq!(m.get(P2).bar(), part_meter::Dbfs::FLOOR);
    on(&mut m);
    assert!(m.get(P3).bar().get() < bar, "falling");
    part_meter::frame(&mut m, Shown::Off);
    assert!(!part_peaks::watched());
    assert_eq!(m, PartMeters::REST);
}

/// A reading raised as the page closes (a block in flight) is dropped:
/// the page reopens on rested meters, not on stale audio.
#[test]
fn a_reopened_page_shows_no_stale_reading() {
    let _g = lock();
    played::clear_held();
    let ui = UiState::new();
    let parts = ui.project().perf().parts();
    let mut m = PartMeters::REST;
    part_meter::frame(&mut m, Shown::Off);
    PART_PEAKS[2].raise(Peak::new(0.5));
    part_meter::frame(&mut m, Shown::Off);
    part_meter::frame(
        &mut m,
        Shown::On {
            parts,
            zones: Zones::OFF,
            dt: Ms(50),
        },
    );
    assert_eq!(m.get(P3).bar(), part_meter::Dbfs::FLOOR, "fresh");
    part_meter::frame(&mut m, Shown::Off);
}

/// On MIXER › PRT the band draws Part 3's bar beside its LEVEL bar, its
/// peak mark, and the key mark over its number.
#[test]
fn prt_draws_the_meter_beside_the_level_bar() {
    let _g = lock();
    PART_PEAKS.iter().for_each(|m| {
        m.take();
    });
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::press(ButtonId::Minus)); // SENDS → PART
    feed(&mut ui, Input::turn(EncoderId::D, 0));
    PART_PEAKS[2].raise(Peak::new(1.0));
    played::note_on(2, chimera_core::project::PartId::ALL[2], 64, 100, 64, 0);
    ui.update(chimera_core::ui::animation::UiTick::for_test());
    let fb = render_ui(&ui);
    let x = part_meter::bar_x(P3) + 1;
    let foot = viz::STRIP_TOP + viz::STRIP_H - 1;
    assert_eq!(fb.at(x, foot), theme::INK2, "the bar's foot");
    assert_eq!(
        fb.at(x, viz::STRIP_TOP + 3),
        theme::INK2,
        "full scale: to the top"
    );
    assert_eq!(
        fb.at(part_meter::bar_x(P2) + 1, foot),
        theme::BG,
        "Part 2 reads nothing"
    );
    assert_eq!(
        fb.at(x, viz::STRIP_NUM_Y - 5),
        theme::ACCENT,
        "Part 3's key mark"
    );
    fb.dump("mixer_part_meters");
    played::clear_held();
    part_peaks::watch(Watch::Down);
}
