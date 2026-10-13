# 0136. Peek a factory Sound's description per selection into one slot

- **Status:** Accepted (2026-10-13)
- **Deciders:** owner, firmware lead
- **Amends:** [0133](0133-factory-sounds-list-in-a-read-only-factory-tab-and-carry-a-one-line-description.md)

## Context
0133's Consequences priced the description at about 1.9 KB of AXI, 40 B per listed entry, carried by the listing. Built that way, 48 per-row notes (984 B) put the orbit build 736 B under ADR 0087's 32 KiB AXI margin beside the formant operators. The shipped design changed; 0133's cost line no longer describes it.

## Decision
- A listed row keeps no description. `LibState` holds one slot, `Described`: the selected FACTORY row's `FactoryId` and its `Note` (or none).
- When the FACTORY selection moves to a row the slot doesn't hold, the card work behind BUSY peeks that one file (`peek_note`, the `LIB_PEEK` read 0133 set) into the slot. Line 2 is empty until the slot is that row's.
- Everything else in 0133 stands: the `Note` record, the peek size, the id blocks, the tabs and the demo project ids.

## Alternatives considered
- **A note per listed row (0133 as written):** 984 B of AXI; fails ADR 0087's orbit margin. Rejected.
- **A second page static per tab:** about 4.4 KB of AXI to skip a re-list. Rejected.

## Consequences
- FACTORY costs about 80 B of AXI (the slot and the page's one set stamp), held by `memory_budget_test`.
- Each new selection costs one sector peek under BUSY; scrolling fast through FACTORY peeks only the row it stops on.
- Code stays under 0.5 KB of flash; the text costs no flash.

## Sources
Spec `docs/superpowers/specs/2026-10-11-showcase-library-design.md` § 1 (Cost, Listing); ADR 0133; ADR 0087; `chimera-core/src/ui/library.rs` (`Described`, `lib_peek_note`); `chimera-core/tests/memory_budget_test.rs`.
