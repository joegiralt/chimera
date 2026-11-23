// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_core::board::{BoardId, HC165_PA, HC165_PF, Port};

#[test]
fn every_strap_value_names_a_board() {
    for s in 0..=0x0Fu8 {
        let b = BoardId::from_straps(s);
        let want = match s {
            0b0001 => BoardId::Lqfp176,
            0b0010 => BoardId::Lqfp144,
            0b0011 => BoardId::Lqfp100,
            _ => BoardId::Unknown(s),
        };
        assert_eq!(b, want, "strap {s:04b}");
        assert_eq!(b.straps(), s, "strap {s:04b} round trips");
    }
}

#[test]
fn only_the_100_moves_the_hc165_to_port_a() {
    for s in 0..=0xFFu8 {
        let b = BoardId::from_straps(s);
        let want = if s == 0b0011 { HC165_PA } else { HC165_PF };
        assert_eq!(b.hc165(), want, "strap {s:08b}");
    }
}

#[test]
fn the_pins_are_stocks() {
    assert_eq!(
        (HC165_PF.port, HC165_PF.data, HC165_PF.load, HC165_PF.clk),
        (Port::F, 2, 1, 0)
    );
    assert_eq!(
        (HC165_PA.port, HC165_PA.data, HC165_PA.load, HC165_PA.clk),
        (Port::A, 0, 1, 2)
    );
}

#[test]
fn pd0_is_the_top_bit() {
    assert_eq!(
        BoardId::from_pins([false, false, false, true]),
        BoardId::Lqfp176
    );
    assert_eq!(
        BoardId::from_pins([false, false, true, false]),
        BoardId::Lqfp144
    );
    assert_eq!(
        BoardId::from_pins([false, false, true, true]),
        BoardId::Lqfp100
    );
    assert_eq!(
        BoardId::from_pins([true, false, false, false]),
        BoardId::Unknown(0b1000)
    );
    // No strap fitted: every pull-up reads high.
    assert_eq!(BoardId::from_pins([true; 4]), BoardId::Unknown(0b1111));
}

#[test]
fn an_unknown_strap_says_its_wiring_is_a_guess() {
    assert_eq!(BoardId::Unknown(0b1111).label(), "176?");
    assert_eq!(BoardId::Unknown(0b1111).word(), "unknown");
    assert_eq!(BoardId::Lqfp100.label(), "100");
    assert_eq!(BoardId::Lqfp144.word(), "lqfp144");
}
