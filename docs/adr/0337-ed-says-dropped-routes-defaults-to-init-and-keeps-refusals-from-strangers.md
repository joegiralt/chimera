# 0337. `ed` says a dropped route, defaults to INIT, and keeps its refusals from strangers

- **Status:** Accepted (2026-11-23)
- **Deciders:** coordinator (editor e2e coverage), Claude

## Context
Four faults came out of the editor's expanded e2e.

1. **A route past the registry vanished.** `ed route` answered `OK queued`, and at its turn the box couldn't prime a new destination (the registry is full at 16). It set the focus band's MATRIX FULL and dropped the route, but no `refused` line said so. In the e2e, 38 routes were queued and the unit kept 22.
2. **`ed describe`'s defaults were the specs', not INIT's.** On ALGO, FILTER.CUTOFF defaulted to 1000 against INIT's 20000, OP_A–D LEVEL to 0 against 99 or 72, and so on: 143 params across the five engines, DRUM's pads among them. The editor's double-click to default silenced an operator.
3. **Any `since` cleared the refusals.** A second client's `ed since` (`chimera-usb`, or a reconnect) took the editor's refusals with it.
4. **Four slots hid the drops.** The refusal list kept the newest four. With 24 routes queued and 16 kept, only 4 of the 8 drops were said. A refusal repeated at one address took a slot each time.

## Decision
1. **A dropped route is refused.** `ed_cue::route` sets the focus band's status only when the registry refuses a destination. The queue takes the band's status before the route's pass and reads it after. If the pass set one, the queue pushes a refusal for that Part at the route's destination address, in the band's words, and the band keeps the new status. Otherwise the band gets its old status back. Like a set's refusal, it is said once, at the end of PART, while PART is a diff.
   - `ed_cue::route` is bank 1's code as it was: a refusal it returned itself moved bank 1's layout.
   - The band's words are copied to bank 2 (`route_words`), because a call to `PrimeStatus::label` splits it out of bank 1.
2. **describe's default is INIT's value.** For an engine's pages, each `param` line's default is what `Sound::init(<engine>)` stores at that address. A pad's default is `Kit::default_pad(n)`'s, each pad its own. The kit's TONAL is `Kit::DEFAULT`'s, read at compile time: building a `Kit` would call bank 1's `Kit::init` and put 964 B on the stack. The shared schemas (MIXER, SETTINGS, ORBIT) keep their specs' defaults. `console_ed_defaults_test` walks every engine and compares every listed param.
3. **Only the number last issued clears refusals.** An answer to any other `<n>` (0, another client's, a stale one) says none in a whole PART and drops none. The first diff after says them, once.
   - Per-client copies were the fuller fix, but they cost about 1.2 KB of ITCM, where `ed`'s state lives. The coordinator chose this one; per-client slots are filed for 0.5 (private issue #1133).
   - A second client still forces whole answers on the editor; that is what the slots would fix.
4. **The refusals are coalesced, so four slots never hide one.** The list keeps four items, oldest first, and a count:
   - **One line an address.** A second refusal at an address already listed replaces its words and takes no slot.
   - **One line a full matrix, a Part.** A route dropped for MATRIX FULL while that Part already lists a MATRIX FULL item is only counted.
   - **The oldest goes when full.** A fifth item drops the oldest, and the dropped one is counted.
   - **The count is said last.** After the Part's lines comes `refused more <n>`, when `n` is at least 1. It counts every refusal no line says since the last answer that cleared them, and it stops at 31. 31 means 31 or more.
   - The count lives in the top 5 bits of the list's length byte, and a full matrix is known by its words. So `Refusals` keeps its 217 B, and `EdState`'s layout, which bank 1 builds in place, doesn't move.
5. **The line format, for the editor:**
   - `refused <ADDR> "<why>"`: a turn refused at `ADDR`, said once. For a dropped route, `ADDR` is its destination and `why` is the band's words (`MATRIX FULL`, `NOT MODULATABLE`, `ALREADY ROUTED`).
   - `refused more <n>`: `n` more refusals that no line says (31 at most, meaning at least 31). It is said at most once an answer, after the `refused <ADDR>` lines, at the end of PART.
   - With 24 routes queued and 16 kept, the answer says `refused <first dropped DEST> "MATRIX FULL"` and `refused more 7`.

## Alternatives considered
- **(1) A `refused` line naming the route cell (`<SRC>:<DEST>`).** The editor's `refused` reads an address, so a cell would read as `Unknown`. The destination is the param the user was routing to.
- **(2) Changing the specs' defaults to INIT's.** A spec's default is also what the box's push-to-default and older paths read, and that is a sound change on the box. describe is the editor's view of INIT.
- **(3) Per-client slots.** See above.
- **(4) A count for each Part, or a flag on each item.** Either one grows `Refusals` past `EdState`'s 3 B of padding, which moves `EdState`'s fields and bank 1's stores into it. Every answer to the number last issued clears all the refusals, so one count serves the Part the answer follows.
- **(5) A `refused <DEST> "MATRIX FULL" <n>` line.** It changes the `refused` line's grammar. A separate `refused more` line leaves every existing line as it was.

## Consequences
- The editor sees a refused route, with its reason, and its double-click to default lands on INIT.
- The editor always knows that something was dropped. A full registry is one line plus the count, however many routes it refused.
- The editor's parser takes `refused more <n>` (`more` is no address: addresses are upper case).
- The engine schemas' hashes change once (their bytes did), so the editor re-reads them.
- A reconnecting editor may hear a refusal from before it connected, once.

## Sources
- ADR 0285, 0289 (protocol), 0151 (refusals), `chimera-core/src/ui/ed_cue.rs`, `console/ed/{edits,since,describe}.rs`.
- Tests: `console_ed_matrix_test` (`a_route_dropped_on_a_full_registry_is_refused_in_since`, `eight_dropped_routes_are_one_line_and_a_count`, `another_clients_since_leaves_the_editors_refusals`), `console_ed_defaults_test`, `console_ed_engines_test` (`a_refusal_stays_with_its_part`, `since_says_an_address_refused_again_once`, updated), `console::ed::edits` unit tests (`an_address_refused_twice_is_one_line`, `a_full_matrix_is_its_first_drop_and_a_count`, `a_dropped_refusal_is_counted`, `the_unsaid_count_stops_at_its_most`).
