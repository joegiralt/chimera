//! TAPE's glyph through the door (ADR 0173, 0175): open, `from_set` and
//! `paint` are bank 2's `draw::inputs` and `draw::render`; closed, ONE's
//! still pose and no paint. The fixed slots after the door
//! (`DoorCell::with_at`) answer only on the unit: the host's door is a
//! plain static. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_engine::bank2::{BOARD_DOOR_ADDR, DOOR_CELL, UI_DOOR_ADDR};
use chimera_engine::tape_glyph::{Canvas, Kind, TapeGauge, draw};

#[test]
fn glyph_inputs_and_paint_open_and_closed() {
    let set = [0.0, 0.5, 0.1, 0.9, 0.3, 0.6, 0.4, 0.2, 0.7, 0.8, 0.5];
    let mut out = Canvas::new();

    DOOR_CELL.close();
    assert_eq!(TapeGauge::from_set(set, false), TapeGauge::still(Kind::One));
    assert!(!chimera_engine::tape_glyph::paint(&draw::inputs(set, false), &mut out));

    DOOR_CELL.open_host();
    let live = TapeGauge::from_set(set, false);
    assert_eq!(live, draw::inputs(set, false));
    assert_ne!(live, TapeGauge::still(Kind::One));
    assert!(chimera_engine::tape_glyph::paint(&live, &mut out));
    // SAFETY: never dereferenced: the host's door has no slots.
    let slots = unsafe {
        (
            DOOR_CELL.with_at::<u32, _>(BOARD_DOOR_ADDR, |_| ()),
            DOOR_CELL.with_at::<u32, _>(UI_DOOR_ADDR, |_| ()),
        )
    };
    assert_eq!(slots, (None, None));
    DOOR_CELL.close();
}
