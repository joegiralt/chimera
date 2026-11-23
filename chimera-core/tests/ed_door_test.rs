// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ed` behind bank 2's door (`EdDoor`, ADR 0251), under `bank2` on a
//! host, where the table answers only while the door cell is open. One
//! test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

mod common;
use common::console_fake::{Fake, ask};

use chimera_core::bank2::DOOR_CELL;

#[test]
fn ed_door_closed_refuses_plainly() {
    let mut f = Fake::new();
    for line in ["ed hello\n", "ed\n", "ed bogus\n", "ed get OP_C.CRSE\n"] {
        assert_eq!(
            ask(&mut f, line),
            "ERR ed needs bank 2, update the unit\n",
            "{line}"
        );
    }
    DOOR_CELL.open_host();
    assert!(ask(&mut f, "ed hello\n").ends_with("OK\n"));
    assert!(ask(&mut f, "ed\n").starts_with("ERR ed takes "));
    DOOR_CELL.close();
    assert_eq!(
        ask(&mut f, "ed hello\n"),
        "ERR ed needs bank 2, update the unit\n"
    );
}
