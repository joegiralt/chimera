//! Stream admission (slice 1 § 5): the card's byte budget.

use chimera_engine::MidiNote;
use chimera_engine::stream::admit::{
    Admit, Age, Bill, LOUD, ReadRatio, STREAM_BUDGET_BPS, WOW_CENTS, admit, bill, head_frames,
    notes_at_root, ratio_max,
};
use chimera_engine::stream::units::{Blocks, Cents, SourceRate};
use chimera_engine::voice_alloc::VoiceIdx;

fn age(released: bool, since: u32) -> Age {
    let since = Blocks::new(since);
    if released {
        Age::Released { since }
    } else {
        Age::Held { since, level: LOUD }
    }
}

fn hz(r: u16) -> SourceRate {
    SourceRate::new(r).unwrap()
}

fn note(n: u8) -> MidiNote {
    MidiNote::new(n).unwrap()
}

fn rm(n: u8, root: f32, reach: f32, bend: f32) -> ReadRatio {
    ratio_max(
        note(n),
        Cents::new(root),
        Cents::new(reach),
        Cents::new(bend),
    )
}

/// The bill as written before it was built from the chunk layout.
fn old_bill(rate: u16, r: f32) -> u32 {
    let bps = rate as f32 * 1.5 * 4608.0 / 4032.0 * r;
    (bps + 0.5) as u32
}

#[test]
fn the_bill_is_the_old_formulas_at_every_rate_and_ratio() {
    for rate in (4_000..=48_000).step_by(1_000) {
        for r in [0.25, 0.5, 1.0, 2.0, 4.0, 16.0] {
            let new = bill(hz(rate), ReadRatio::new(r)).get();
            assert_eq!(new, old_bill(rate, r), "{rate} Hz at {r}x");
        }
    }
}

fn v(i: usize) -> VoiceIdx {
    VoiceIdx::ALL[i]
}

#[test]
fn bill_at_1x_24k_is_41143() {
    assert_eq!(bill(hz(24_000), ReadRatio::ONE), Bill::new(41_143));
}

#[test]
fn plus_12_route_doubles_the_bill() {
    let r = rm(60, 6000.0, 1200.0, 0.0).get() / rm(60, 6000.0, 0.0, 0.0).get();
    assert!((r - 2.0).abs() < 1e-4);
    assert_eq!(bill(hz(24_000), ReadRatio::new(2.0)), Bill::new(82_286));
}

/// B4's budget sits under eight beds at 1x (329,144): seven fit, the
/// eighth steals.
#[test]
fn seven_beds_fit_the_budget_an_eighth_steals() {
    let b = bill(hz(24_000), ReadRatio::ONE);
    let p = (0..7).map(|i| (v(i), b, age(false, i as u32)));
    assert_eq!(admit(b, p.clone().take(6), STREAM_BUDGET_BPS), Admit::Ok);
    assert_eq!(admit(b, p, STREAM_BUDGET_BPS), Admit::Steal(v(6)));
}

/// A TRON chord at +3 st under wow (TAPE up: the caller adds it): six
/// fit, a seventh steals.
#[test]
fn six_tron_notes_fit() {
    let b = bill(hz(24_000), rm(60, 6000.0, 300.0 + WOW_CENTS.get(), 0.0));
    let total = 8 * b.get();
    assert!((395_000..=403_000).contains(&total), "{total}");
    let p = (0..6).map(|i| (v(i), b, age(false, 0)));
    assert_eq!(admit(b, p.clone().take(5), STREAM_BUDGET_BPS), Admit::Ok);
    assert!(matches!(admit(b, p, STREAM_BUDGET_BPS), Admit::Steal(_)));
}

#[test]
fn ninth_steals_oldest_released_then_oldest_held() {
    let b = Bill::new(38_000);
    let mut p = [
        (v(0), b, age(false, 900)),
        (v(1), b, age(true, 10)),
        (v(2), b, age(true, 50)),
        (v(3), b, age(false, 100)),
        (v(4), b, age(false, 100)),
        (v(5), b, age(false, 100)),
        (v(6), b, age(false, 100)),
        (v(7), b, age(false, 100)),
    ];
    assert_eq!(
        admit(b, p.iter().copied(), STREAM_BUDGET_BPS),
        Admit::Steal(v(2))
    );
    p[2].2 = age(false, 0);
    p[1].2 = age(false, 0);
    assert_eq!(
        admit(b, p.iter().copied(), STREAM_BUDGET_BPS),
        Admit::Steal(v(0))
    );
}

#[test]
fn refuse_when_nothing_can_be_stolen() {
    assert_eq!(
        admit(Bill::new(500_000), std::iter::empty(), STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn admit_refuses_a_note_over_the_whole_budget() {
    let p = (0..3).map(|i| (v(i), Bill::new(100_000), age(true, 5)));
    assert_eq!(
        admit(Bill::new(STREAM_BUDGET_BPS.get() + 1), p, STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn head_is_100_ms_rounded_up_to_even() {
    assert_eq!(head_frames(SourceRate::new(24_000).unwrap()), 2_400);
    assert_eq!(head_frames(SourceRate::new(11_025).unwrap()), 1_104);
}

#[test]
fn a_nan_ratio_bills_the_maximum_and_is_refused() {
    let b = bill(hz(24_000), ReadRatio::new(f32::NAN));
    assert_eq!(b, Bill::new(u32::MAX));
    assert_eq!(
        admit(b, std::iter::empty(), STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn the_callers_re_ask_loop_steals_two_oldest_released_first() {
    let b = Bill::new(60_000);
    let mut playing = vec![
        (v(0), b, age(false, 900)),
        (v(1), b, age(true, 10)),
        (v(2), b, age(true, 50)),
        (v(3), b, age(false, 100)),
    ];
    let new = Bill::new(150_000);
    let mut stolen = Vec::new();
    loop {
        match admit(new, playing.iter().copied(), STREAM_BUDGET_BPS) {
            Admit::Ok => break,
            Admit::Steal(i) => {
                stolen.push(i);
                playing.retain(|v| v.0 != i);
            }
            Admit::Refuse => panic!("refused"),
        }
    }
    assert_eq!(stolen, [v(2), v(1)]);
}

/// #698: over budget a held chord gives up its quietest note, the most
/// decayed, not its oldest (the root it was built on); released notes still
/// go first, the oldest of them.
#[test]
fn the_quietest_held_note_goes_first() {
    let b = bill(hz(24_000), ReadRatio::ONE);
    let held = |i: usize, since: u32, level: u16| {
        (
            v(i),
            b,
            Age::Held {
                since: Blocks::new(since),
                level,
            },
        )
    };
    // The root, oldest and loudest; a quiet fifth; a louder third.
    let chord = [held(0, 90, LOUD), held(1, 50, 2_000), held(2, 40, 40_000)];
    let full = || {
        chord
            .iter()
            .copied()
            .chain((3..7).map(|i| held(i, 0, LOUD)))
    };
    assert_eq!(admit(b, full(), STREAM_BUDGET_BPS), Admit::Steal(v(1)));
    // A released note, however loud, goes before any held one.
    let released = (
        v(7),
        b,
        Age::Released {
            since: Blocks::ZERO,
        },
    );
    let p = full().take(6).chain([released]);
    assert_eq!(admit(b, p, STREAM_BUDGET_BPS), Admit::Steal(v(7)));
}

/// #698: a 24 kHz source fits 7 notes at ROOT with the wheel at rest and
/// TAPE 0 (no bend, no wow billed), 11 at 16 kHz.
#[test]
fn notes_at_root_fit_the_budget() {
    assert_eq!(notes_at_root(hz(24_000), false), 7);
    assert_eq!(notes_at_root(hz(16_000), false), 11);
    assert_eq!(notes_at_root(hz(24_000), true), 7);
    assert_eq!(notes_at_root(hz(48_000), false), 3);
}

/// #706: a chord struck over a few ms keeps its notes: one in its attack
/// (its VCA still rising, `ATTACK_BLOCKS`) ranks loud, so the next note
/// takes the quietest decayed one, or the oldest, never the note struck
/// a moment ago.
#[test]
fn a_note_in_its_attack_is_not_the_quietest() {
    let b = bill(hz(48_000), ReadRatio::ONE);
    let held = |i: usize, since: u32, level: u16| {
        (
            v(i),
            b,
            Age::Held {
                since: Blocks::new(since),
                level,
            },
        )
    };
    // A 48 kHz source fits three: struck 20, 10 and 2 blocks ago, the
    // youngest still at a whisper under its attack.
    let chord = [held(0, 20, 30_000), held(1, 10, 9_000), held(2, 2, 300)];
    assert_eq!(
        admit(b, chord.into_iter(), STREAM_BUDGET_BPS),
        Admit::Steal(v(0))
    );
}
