// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! An incoming data value as a `Place`, by the parameter's value law (ADR 0216).

use crate::ctl::DataValue;
use crate::take::Place;
use crate::value::U7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueLaw {
    /// Continuous: 14 bits over the normalised span; an MSB alone is bit-replicated.
    Place14,
    /// Discrete, ≤ 128 values: MSB = steps from min, LSB ignored.
    Index7,
    /// Discrete, > 128 values: 14-bit steps from min.
    Index14,
    /// A choice with frozen enum codes: MSB = the code, LSB ignored.
    Code7,
}

/// `steps` is the highest index (max - min); an index past it clamps.
/// `index_of_code` maps a frozen code to its index, `None` when no live
/// value has that code; only `Code7` calls it.
pub fn place_of(
    law: ValueLaw,
    v: DataValue,
    steps: u16,
    index_of_code: impl FnOnce(U7) -> Option<u16>,
) -> Option<Place> {
    let msb = match v {
        DataValue::Coarse(m) => m,
        DataValue::Fine(f) => f.msb(),
    };
    let index = match law {
        ValueLaw::Place14 => {
            return Some(match v {
                DataValue::Coarse(m) => Place::from_u7_replicated(m),
                DataValue::Fine(f) => Place::new(f.get())?,
            });
        }
        ValueLaw::Index7 => msb.get() as u16,
        ValueLaw::Index14 => match v {
            DataValue::Coarse(m) => (m.get() as u16) << 7,
            DataValue::Fine(f) => f.get(),
        },
        ValueLaw::Code7 => index_of_code(msb)?,
    };
    Some(index_place(index, steps))
}

/// Index i of 0..=steps at round(i · 16383 / steps).
fn index_place(index: u16, steps: u16) -> Place {
    if steps == 0 {
        return Place::ZERO;
    }
    let i = index.min(steps) as u32;
    let s = steps as u32;
    Place::new(((i * 16383 + s / 2) / s) as u16).unwrap_or(Place::MAX)
}
