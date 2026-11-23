// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A load the pool refuses (an empty slot) leaves the Part's `loads` where
//! they were: the audio sees no new Sound, so its held voices play on
//! (#14's fade keys on `loads`).

use chimera_core::project::{
    PartFrom, PartId, PartSource, Project, ReplaceError, ReplaceGuard, SlotId,
};

#[test]
fn a_refused_load_moves_no_loads() {
    let (mut p, t) = Project::boxed();
    let empty = SlotId::ALL
        .into_iter()
        .find(|&s| p.pool().get(s).is_none())
        .expect("an empty slot");
    let part = PartId::ALL[0];
    let before = p.part(part).loads();
    let src = PartSource {
        part,
        from: PartFrom::Slot(empty),
    };
    let c = ReplaceGuard::check(&p, t, src).unwrap_or_else(|n| n.into_pending().anyway(&p));
    assert_eq!(p.replace_part(c), Err(ReplaceError::SlotEmpty));
    assert_eq!(p.part(part).loads(), before);
}
