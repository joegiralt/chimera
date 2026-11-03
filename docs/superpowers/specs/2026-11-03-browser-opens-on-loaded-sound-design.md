# EDIT+Part opens the browser on the Part's last-loaded Sound

**Intent (owner):** moving between Parts, you can't tell what's loaded. Rather than put names on the Part pages, EDIT+Part (which already opens that Part's Sound browser) opens with the cursor on the Sound last loaded into that Part, so its name is highlighted there. Owner: "I think that's enough."

**Goals**
- Each Part remembers which Sound it was loaded from (factory or library), saved with the project.
- EDIT+Part opens the browser with that row selected and scrolled into view.

**Non-goals**
- No Sound name on the Part pages or in the mixer.
- No "edited" marker. The Part keeps pointing at its origin after edits.

**Behaviour (my calls, owner to check)**
- Loading a Sound into a Part sets its origin, and saving a Part to the LIBRARY as a new Sound points it at the new entry.
- A fresh or CLEARed Part's origin is `Init(engine)`, so the browser opens on that engine's INIT row (my call: "where it came from" holds for INIT too).
- If the origin slot no longer holds that Sound (the slot is empty, or its generation or CRC no longer matches), the browser opens where it does today (`Browse::default()`).
- Old projects already carry `Origin`, so nothing to migrate.
- Within a session, the browser's own remembered cursor (if any) gives way to the origin on each EDIT+Part.

**Found while planning:** each Part already keeps and saves its origin (`Part.origin`: `Origin::Slot{slot, generation, crc}` or `Origin::Init(engine)`, project record `Origin` 0x000A). A LIBRARY or FACTORY load is copied into a project pool slot, so its origin is that slot, and the slot's PROJECT row shows the same Sound. So no new record is needed: EDIT+Part opens the PROJECT tab with the cursor on the origin's row.

**Approaches**
1. **Chosen:** reuse the existing `Origin`. EDIT+Part maps it to a PROJECT row: a slot gives its pool row, and `Init(e)` gives engine `e`'s INIT row. It needs no format change and no migration.
2. Store a library id per Part. Ids are reused after a delete, so it would need a generation as well, plus a new record. It duplicates what `Origin` already proves.
2. Store the Sound's name and look it up. It breaks on a rename, and it's larger.
3. Remember the cursor only in RAM. It's lost on power-off and on project load, which defeats the point.

**Left to the builder:** bank-1 placement (the browser and nav are bank 1; the change is a few dozen bytes, measured and recorded per ADR 0198/`tools/bank1-budget.txt`), and tests.
