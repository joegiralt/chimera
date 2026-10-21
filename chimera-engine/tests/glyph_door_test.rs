//! TAPE's glyph through the door (ADR 0173, 0175): open, `TapeGauge::of` and
//! `paint` are bank 2's `draw::inputs` and `draw::render`; closed, ONE's
//! still pose and no paint. The fixed slots after the door
//! (`DoorCell::with_slot`) answer only on the unit: the host's door is a
//! plain static. One test: the door is a process-wide static.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use chimera_engine::bank2::{BOARD_DOOR_ADDR, DOOR_CELL, DoorSlot, UI_DOOR_ADDR};
use chimera_engine::tape_glyph::{Canvas, GlyphInputs, Kind, Pose, TapeGauge, draw};

#[test]
fn glyph_inputs_and_paint_open_and_closed() {
    let set = GlyphInputs::from_set(
        [
            0.0, 0.1, 0.9, 0.3, 0.6, 0.4, 0.2, 0.7, 0.8, 0.5, 0.0, 0.0, 0.0, 0.5, 0.0,
        ],
        Pose::Live,
    );
    let mut out = Canvas::new();

    DOOR_CELL.close();
    assert_eq!(TapeGauge::of(&set), TapeGauge::still(Kind::One));
    assert!(!chimera_engine::tape_glyph::paint(
        &draw::inputs(&set),
        &mut out
    ));

    DOOR_CELL.open_host();
    let live = TapeGauge::of(&set);
    assert_eq!(live, draw::inputs(&set));
    assert_ne!(live, TapeGauge::still(Kind::One));
    assert!(chimera_engine::tape_glyph::paint(&live, &mut out));
    // SAFETY: never dereferenced: the host's door has no slots.
    let (board, ui) = unsafe {
        (
            DoorSlot::<u32>::new(BOARD_DOOR_ADDR),
            DoorSlot::<u32>::new(UI_DOOR_ADDR),
        )
    };
    let slots = (
        DOOR_CELL.with_slot(board, |_| ()),
        DOOR_CELL.with_slot(ui, |_| ()),
    );
    assert_eq!(slots, (None, None));
    DOOR_CELL.close();
}
