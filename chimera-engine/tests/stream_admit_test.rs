//! Stream admission (slice 1 § 5): the card's byte budget.

use chimera_engine::stream::admit::{
    Admit, Age, Bill, STREAM_BUDGET_BPS, admit, bill, head_frames, ratio_max,
};

fn age(released: bool, since: u32) -> Age {
    Age { released, since }
}

#[test]
fn bill_at_1x_24k_is_41143() {
    assert_eq!(bill(24_000, 1.0), Bill(41_143));
}

#[test]
fn plus_12_route_doubles_the_bill() {
    let r = ratio_max(60, 6000.0, 1200.0, 0.0) / ratio_max(60, 6000.0, 0.0, 0.0);
    assert!((r - 2.0).abs() < 1e-4);
    assert_eq!(bill(24_000, 2.0), Bill(82_286));
}

#[test]
fn eight_beds_fit_the_default() {
    let b = bill(24_000, 1.0);
    let p = (0..8).map(|i| (i, b, age(false, 0)));
    assert_eq!(admit(b, p.take(7), STREAM_BUDGET_BPS), Admit::Ok);
}

#[test]
fn eight_tron_notes_fit() {
    let b = bill(24_000, ratio_max(60, 6000.0, 300.0, 0.0));
    let total = 8 * b.0;
    assert!((395_000..=403_000).contains(&total), "{total}");
    let p = (0..7).map(|i| (i, b, age(false, 0)));
    assert_eq!(admit(b, p, STREAM_BUDGET_BPS), Admit::Ok);
}

#[test]
fn ninth_steals_oldest_released_then_oldest_held() {
    let b = Bill(58_000);
    let mut p = [
        (0, b, age(false, 900)),
        (1, b, age(true, 10)),
        (2, b, age(true, 50)),
        (3, b, age(false, 100)),
        (4, b, age(false, 100)),
        (5, b, age(false, 100)),
        (6, b, age(false, 100)),
        (7, b, age(false, 100)),
    ];
    assert_eq!(
        admit(b, p.iter().copied(), STREAM_BUDGET_BPS),
        Admit::Steal(2)
    );
    p[2].2 = age(false, 0);
    p[1].2 = age(false, 0);
    assert_eq!(
        admit(b, p.iter().copied(), STREAM_BUDGET_BPS),
        Admit::Steal(0)
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
    let p = (0..3).map(|i| (i, Bill(100_000), age(true, 5)));
    assert_eq!(admit(Bill(658_000), p, STREAM_BUDGET_BPS), Admit::Refuse);
}

#[test]
fn head_is_100_ms_rounded_up_to_even() {
    assert_eq!(head_frames(24_000), 2_400);
    assert_eq!(head_frames(11_025), 1_104);
}
