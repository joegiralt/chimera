mod common;
use chimera_midi::stream::*;
use chimera_midi::value::*;
use common::*;

fn on(ch: u8, k: u8, v: u8) -> Item {
    Item::Voice(
        ch,
        Voice::NoteOn {
            key: key(k),
            vel: vel(v),
        },
    )
}
fn off(ch: u8, k: u8, v: u8) -> Item {
    Item::Voice(
        ch,
        Voice::NoteOff {
            key: key(k),
            vel: u7(v),
        },
    )
}

#[test]
fn realtime_interleaves() {
    assert_eq!(
        parse(&[0x90, 0xF8, 0x3C, 0xF8, 0x64]),
        [
            Item::Rt(RealTime::Clock),
            Item::Rt(RealTime::Clock),
            on(0, 0x3C, 0x64)
        ]
    );
}

#[test]
fn all_realtime_bytes() {
    use RealTime::*;
    let got = parse(&[0xF8, 0xFA, 0xFB, 0xFC, 0xFE, 0xFF]);
    let want: Vec<_> = [Clock, Start, Continue, Stop, ActiveSensing, Reset]
        .map(Item::Rt)
        .into();
    assert_eq!(got, want);
}

#[test]
fn running_status_note_on_pairs() {
    assert_eq!(
        parse(&[0x91, 60, 100, 62, 90, 64, 80]),
        [on(1, 60, 100), on(1, 62, 90), on(1, 64, 80)]
    );
}

#[test]
fn vel0_is_note_off() {
    assert_eq!(parse(&[0x90, 60, 0]), [off(0, 60, 0)]);
}

#[test]
fn note_off_keeps_velocity() {
    assert_eq!(parse(&[0x8F, 60, 33]), [off(15, 60, 33)]);
}

#[test]
fn bend_lsb_first() {
    let bend = |a, b| {
        Item::Voice(
            0,
            Voice::PitchBend(Bend::from_u14(U14::from_parts(u7(b), u7(a)))),
        )
    };
    assert_eq!(parse(&[0xE0, 0x00, 0x00]), [bend(0, 0)]);
    let got = parse(&[0xE0, 0x00, 0x40, 0x7F, 0x7F, 0x00, 0x00]);
    let vals: Vec<i16> = got
        .iter()
        .map(|i| match i {
            Item::Voice(_, Voice::PitchBend(b)) => b.get(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(vals, [0, 8191, -8192]);
}

#[test]
fn cc_120_to_127_are_channel_mode() {
    use ChannelMode::*;
    let want = [
        AllSoundOff,
        ResetAllControllers,
        LocalControl(false),
        AllNotesOff,
        OmniOff,
        OmniOn,
        MonoOn(u7(0)),
        PolyOn,
    ];
    for (i, m) in want.into_iter().enumerate() {
        assert_eq!(
            parse(&[0xB2, 120 + i as u8, 0]),
            [Item::Mode(2, m)],
            "cc {}",
            120 + i
        );
    }
}

#[test]
fn cc_119_is_a_control() {
    let want = Item::Voice(
        0,
        Voice::Control {
            cc: Ctrl::new(119).unwrap(),
            value: u7(5),
        },
    );
    assert_eq!(parse(&[0xB0, 119, 5]), [want]);
}

#[test]
fn local_control_off_on() {
    assert_eq!(
        parse(&[0xB0, 122, 0]),
        [Item::Mode(0, ChannelMode::LocalControl(false))]
    );
    assert_eq!(
        parse(&[0xB0, 122, 127]),
        [Item::Mode(0, ChannelMode::LocalControl(true))]
    );
}

#[test]
fn mono_on_carries_count() {
    assert_eq!(
        parse(&[0xB0, 126, 4]),
        [Item::Mode(0, ChannelMode::MonoOn(u7(4)))]
    );
}

#[test]
fn program_and_pressure_take_one_byte() {
    assert_eq!(
        parse(&[0xC3, 5, 6, 0xD3, 7, 8]),
        [
            Item::Voice(3, Voice::Program(u7(5))),
            Item::Voice(3, Voice::Program(u7(6))),
            Item::Voice(3, Voice::ChannelPressure(u7(7))),
            Item::Voice(3, Voice::ChannelPressure(u7(8))),
        ]
    );
}

#[test]
fn poly_pressure_pairs() {
    let want = Item::Voice(
        0,
        Voice::PolyPressure {
            key: key(60),
            value: u7(9),
        },
    );
    assert_eq!(parse(&[0xA0, 60, 9]), [want]);
}

#[test]
fn sysex_ends_on_f7() {
    assert_eq!(
        parse(&[0xF0, 0x7D, 1, 2, 0xF7]),
        [Item::SysEx(vec![0x7D, 1, 2])]
    );
    assert_eq!(parse(&[0xF0, 0xF7]), [Item::SysEx(vec![])]);
}

#[test]
fn sysex_cut_by_status_starts_the_new_message() {
    assert_eq!(
        parse(&[0xF0, 0x7D, 0x01, 0x90, 0x3C, 0x64]),
        [Item::Fault(Fault::SysExCut), on(0, 0x3C, 0x64)]
    );
}

#[test]
fn sysex_overflow_at_n_plus_one() {
    let mut p = Parser::<4>::new();
    assert!(run(&mut p, &[0xF0, 1, 2, 3, 4]).is_empty());
    assert_eq!(run(&mut p, &[5]), [Item::Fault(Fault::SysExOverflow)]);
    // the rest is ignored until F7, which delivers nothing
    assert!(run(&mut p, &[6, 7, 0xF7]).is_empty());
    assert_eq!(p.faults().sysex_overflow, 1);
    // exactly N bytes is fine
    assert_eq!(
        run(&mut p, &[0xF0, 1, 2, 3, 4, 0xF7]),
        [Item::SysEx(vec![1, 2, 3, 4])]
    );
    // overflowed then cut by a status: no second fault
    assert_eq!(
        run(&mut p, &[0xF0, 1, 2, 3, 4, 5, 6]),
        [Item::Fault(Fault::SysExOverflow)]
    );
    assert_eq!(run(&mut p, &[0x90, 60, 1]), [on(0, 60, 1)]);
    assert_eq!(p.faults().sysex_cut, 0);
}

#[test]
fn sysex_realtime_inside_kept_out() {
    assert_eq!(
        parse(&[0xF0, 1, 0xF8, 2, 0xF7]),
        [Item::Rt(RealTime::Clock), Item::SysEx(vec![1, 2])]
    );
}

#[test]
fn system_common_cancels_running_status() {
    for s in 0xF1..=0xF6u8 {
        let got = parse(&[0x90, 60, 100, s, 1, 2, 3]);
        assert_eq!(got[0], on(0, 60, 100));
        assert!(
            got.last() == Some(&Item::Fault(Fault::Orphan)),
            "status {s:X}: {got:?}"
        );
    }
}

#[test]
fn f4_f5_cancel_and_are_undefined() {
    let mut p = Parser::<32>::new();
    assert_eq!(
        run(&mut p, &[0x90, 60, 100, 0xF4, 60, 100, 0xF5]),
        [
            on(0, 60, 100),
            Item::Fault(Fault::Undefined),
            Item::Fault(Fault::Orphan),
            Item::Fault(Fault::Orphan),
            Item::Fault(Fault::Undefined)
        ]
    );
    assert_eq!(p.faults().undefined, 2);
}

#[test]
fn f9_fd_ignored_like_realtime() {
    // state untouched: running status survives, a SysEx continues
    assert_eq!(
        parse(&[0x90, 60, 0xF9, 100, 0xFD, 61, 100]),
        [
            Item::Fault(Fault::Undefined),
            on(0, 60, 100),
            Item::Fault(Fault::Undefined),
            on(0, 61, 100)
        ]
    );
    assert_eq!(
        parse(&[0xF0, 1, 0xF9, 2, 0xF7]),
        [Item::Fault(Fault::Undefined), Item::SysEx(vec![1, 2])]
    );
}

#[test]
fn song_position_is_u14() {
    let want = Item::Common(Common::SongPosition(U14::new((2 << 7) | 1).unwrap()));
    assert_eq!(parse(&[0xF2, 1, 2]), [want]);
    assert_eq!(parse(&[0xF3, 9]), [Item::Common(Common::SongSelect(u7(9)))]);
    assert_eq!(
        parse(&[0xF1, 0x15]),
        [Item::Common(Common::TimeCode(u7(0x15)))]
    );
}

#[test]
fn tune_request_after_cut_sysex_wins() {
    let mut p = Parser::<32>::new();
    assert_eq!(
        run(&mut p, &[0xF0, 1, 0xF6]),
        [Item::Common(Common::TuneRequest)]
    );
    assert_eq!(p.faults().sysex_cut, 1);
}

#[test]
fn interrupted_message_is_a_fault() {
    let mut p = Parser::<32>::new();
    assert_eq!(
        run(&mut p, &[0x90, 60, 0x91, 61, 1]),
        [Item::Fault(Fault::Interrupted), on(1, 61, 1)]
    );
    assert_eq!(
        run(&mut p, &[0xF2, 1, 0xF6]),
        [Item::Common(Common::TuneRequest)]
    );
    assert_eq!(p.faults().interrupted, 2);
}

#[test]
fn orphan_data_before_any_status() {
    let mut p = Parser::<32>::new();
    assert_eq!(
        run(&mut p, &[1, 2]),
        [Item::Fault(Fault::Orphan), Item::Fault(Fault::Orphan)]
    );
    assert_eq!(p.faults().orphan, 2);
}

#[test]
fn redundant_status_accepted() {
    assert_eq!(
        parse(&[0x90, 60, 100, 0x90, 62, 90]),
        [on(0, 60, 100), on(0, 62, 90)]
    );
}

#[test]
fn stray_f7_is_silent_and_cancels_running_status() {
    assert_eq!(parse(&[0x90, 0xF7, 60]), [Item::Fault(Fault::Orphan)]);
}

#[test]
fn fault_counts_saturate() {
    let mut p = Parser::<32>::new();
    for _ in 0..70_000 {
        p.push(1);
    }
    assert_eq!(p.faults().orphan, u16::MAX);
}
