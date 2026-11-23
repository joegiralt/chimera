// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_midi::sense::Sensing;
use chimera_midi::stream::RealTime;

#[test]
fn sense_off_until_first_fe() {
    let mut s = Sensing::Off;
    s.heard(RealTime::Clock);
    s.heard(RealTime::Reset);
    assert!(!s.armed());
    assert!(!s.timed_out());
    s.heard(RealTime::ActiveSensing);
    assert!(s.armed());
    s.heard(RealTime::ActiveSensing);
    assert!(s.armed());
}

#[test]
fn sense_times_out_once_then_off() {
    let mut s = Sensing::Off;
    s.heard(RealTime::ActiveSensing);
    assert!(s.timed_out());
    assert!(!s.armed());
    assert!(!s.timed_out());
    s.heard(RealTime::ActiveSensing);
    assert!(s.armed());
}
