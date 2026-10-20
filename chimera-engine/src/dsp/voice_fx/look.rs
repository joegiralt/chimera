//! FLT › FX's curve as rows: the type's memoryless transfer (`curve`, with
//! FUZZ's real one) traced over one sine period for the band to draw. With
//! `bank2` this module and the curves are bank 2's (`bank2.x`), reached only
//! through the door (`trace`); closed, the band draws no curve.

use super::params::FxType;

/// The band's columns, `VIZ_LEFT` to `VIZ_RIGHT` (the LFO scope's).
pub const COLS: usize = crate::lfo_glyph::COLS;
/// Half the curve's height, px.
pub const AMP: f32 = 16.0;

/// Rows above the middle: `[0]` the sine, `[1]` the type's curve of it.
pub type Rows = [[i8; COLS]; 2];

/// `t`'s curve at `pots` (the set values, ADR 0152) into `out`.
pub fn render(t: FxType, pots: [f32; 3], out: &mut Rows) {
    let row = |v: f32| (v.clamp(-1.0, 1.0) * AMP) as i8;
    for i in 0..COLS {
        let s = crate::dsp::fast_sin_turns(i as f32 / (COLS - 1) as f32);
        out[0][i] = row(s);
        out[1][i] = row(super::curve(t, s, pots));
    }
}

/// `render`, bank 2's through the door. True if drawn; closed, `out` is
/// left. A host draws in place; a firmware without bank 2 (the bench sets)
/// links none.
pub fn trace(t: FxType, pots: [f32; 3], out: &mut Rows) -> bool {
    #[cfg(feature = "bank2")]
    {
        crate::bank2::DOOR_CELL
            .with(|d| (d.look.render)(t, pots, out))
            .is_some()
    }
    #[cfg(all(not(feature = "bank2"), not(target_os = "none")))]
    {
        render(t, pots, out);
        true
    }
    #[cfg(all(not(feature = "bank2"), target_os = "none"))]
    {
        let _ = (t, pots, out);
        false
    }
}

/// The curve's entry in the door's table.
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct LookDoor {
    pub render: fn(FxType, [f32; 3], &mut Rows),
}

#[cfg(feature = "bank2")]
impl LookDoor {
    pub const DOOR: LookDoor = LookDoor { render: render };
}
