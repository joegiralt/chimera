//! Stream admission (slice 1 § 5): the card's byte budget.

use chimera_engine::stream::admit::{
    Admit, Age, Bill, LOUD, ReadRatio, STREAM_BUDGET_BPS, WOW_CENTS, admit, bill, head_frames,
    ratio_max,
};
use chimera_engine::voice_alloc::VoiceIdx;

fn age(released: bool, since: u32) -> Age {
    Age {
        released,
        since,
        level: LOUD,
    }
}

fn v(i: usize) -> VoiceIdx {
    VoiceIdx::ALL[i]
}

#[test]
fn bill_at_1x_24k_is_41143() {
    assert_eq!(bill(24_000, ReadRatio::ONE), Bill(41_143));
}

#[test]
fn plus_12_route_doubles_the_bill() {
    let r = ratio_max(60, 6000.0, 1200.0, 0.0).get() / ratio_max(60, 6000.0, 0.0, 0.0).get();
    assert!((r - 2.0).abs() < 1e-4);
    assert_eq!(bill(24_000, ReadRatio::new(2.0)), Bill(82_286));
}

/// B4's budget sits under eight beds at 1x (329,144): seven fit, the
/// eighth steals.
#[test]
fn seven_beds_fit_the_budget_an_eighth_steals() {
    let b = bill(24_000, ReadRatio::ONE);
    let p = (0..7).map(|i| (v(i), b, age(false, i as u32)));
    assert_eq!(admit(b, p.clone().take(6), STREAM_BUDGET_BPS), Admit::Ok);
    assert_eq!(admit(b, p, STREAM_BUDGET_BPS), Admit::Steal(v(6)));
}

/// A TRON chord at +3 st under wow (TAPE up: the caller adds it): six
/// fit, a seventh steals.
#[test]
fn six_tron_notes_fit() {
    let b = bill(24_000, ratio_max(60, 6000.0, 300.0 + WOW_CENTS, 0.0));
    let total = 8 * b.0;
    assert!((395_000..=403_000).contains(&total), "{total}");
    let p = (0..6).map(|i| (v(i), b, age(false, 0)));
    assert_eq!(admit(b, p.clone().take(5), STREAM_BUDGET_BPS), Admit::Ok);
    assert!(matches!(admit(b, p, STREAM_BUDGET_BPS), Admit::Steal(_)));
}

#[test]
fn ninth_steals_oldest_released_then_oldest_held() {
    let b = Bill(38_000);
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
        admit(Bill(500_000), std::iter::empty(), STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn admit_refuses_a_note_over_the_whole_budget() {
    let p = (0..3).map(|i| (v(i), Bill(100_000), age(true, 5)));
    assert_eq!(
        admit(Bill(STREAM_BUDGET_BPS + 1), p, STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn head_is_100_ms_rounded_up_to_even() {
    assert_eq!(head_frames(24_000), 2_400);
    assert_eq!(head_frames(11_025), 1_104);
}

#[test]
fn a_nan_ratio_bills_the_maximum_and_is_refused() {
    let b = bill(24_000, ReadRatio::new(f32::NAN));
    assert_eq!(b, Bill(u32::MAX));
    assert_eq!(
        admit(b, std::iter::empty(), STREAM_BUDGET_BPS),
        Admit::Refuse
    );
}

#[test]
fn the_callers_re_ask_loop_steals_two_oldest_released_first() {
    let b = Bill(60_000);
    let mut playing = vec![
        (v(0), b, age(false, 900)),
        (v(1), b, age(true, 10)),
        (v(2), b, age(true, 50)),
        (v(3), b, age(false, 100)),
    ];
    let new = Bill(150_000);
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
