# 0258. Send each panel edit as its NRPN under MIDI CONFIG › ECHO › PARAM OUT

- **Status:** Proposed
- **Deciders:** owner (#913, 0.3 scope: parity with stock), firmware

## Context
Stock PreenFM3 sends an NRPN for every parameter turned on its panel, so a DAW or sequencer can record and replay edits. Chimera's only output was ECHO (ADR 0217): the value of each *bound* control, sent back to its controller. An unbound edit never left the unit.

ADR 0216 already gives every stored param a frozen NRPN address and a value law, and the unit answers those NRPNs with no binding. A sender only needs the inverse of the receiver's law.

## Decision
- **The setting.** MIDI CONFIG › ECHO's cell B, **PARAM OUT**: `OFF` / `NRPN`. `MidiSettings` param 12, ident `PARAM_OUT`, a bit of `Switches` (the struct stays its size). Default `OFF`, and a SYSTEM from before it loads `OFF`: nothing new goes out by surprise. Precedent: the Digitakt's PARAM OUTPUT, stock's MIDI send setting.
- **Under ECHO.** ECHO stays DIN OUT's master switch (ADR 0217): with ECHO off, PARAM OUT sends nothing.
- **What sends: the hand alone, pushed.** `hand_in`, the door's call for every panel turn (`cc_hand`), notes the Part and address; the reverb's panel turn now calls it too. A value MIDI moved (a binding, a canonical NRPN, MIXER, program change), a load, ORBIT's playing and modulation never pass there, so nothing that came in goes back out and a thru loop can't feed back. ADR 0217 chose a pull compare for echo; that can't tell the hand from MIDI, a push from the one hand path can.
- **What is sent.** Only `W` addresses (ADR 0139's mappable set), so whatever a sequencer records plays back. On the Part's channel, out of DIN and USB through the out pass's `Tee`, after the echo. `Place14` and `Index14` send CC 99, 98, 6, 38; `Index7` and `Code7` CC 99, 98, 6. Values are the receive law's inverse (`param_out::data`): a place rounded to 14 bits, a step count from the minimum, a frozen code. A test reads every Sound address's value back through `place_of`.
- **Coalesced and paced by DIN.** Eight slots (`ParamOut`, ~56 B in `MidiUi`): a turn marks its slot due; each out pass sends each due slot's value *now* while DIN's ring has room, else it stays due. A value equal to the last sent is skipped. A load on the Part drops its slots. No backlog forms: a fast turn sends at most the latest value each pass.
- **The master FX** send on the channel of the Part whose page turned them.
- **Placement: bank 1 (ADR 0251).** ORBIT's bank 2 was ~2.5 KB free, under 0251's 8 KB floor; in bank 2 this cost +1,696 B. `ui::param_out` sits outside `ui::midi` (whose sections bank 2 takes by name), its entries `inline(never)`, and calls nothing of bank 2's: it writes the NRPN bytes itself rather than through `chimera_midi::out`, and `nrpn::law` and `NrpnAddr::of` are `inline(always)`. Bank 2's `hand_in` and out pass call it (bank 2 to bank 1 is allowed; `door-check` passes). Its state (`ParamOut`) is a field of `MidiUi`. Bank 1 also keeps the spec, its page slot, `MidiSettings`' get/write arms and the reverb path's `cc_hand` call; each raised `tools/bank1-budget.txt` line names this ADR. Measured against main 0f38830b: bank 1 +2,284 B (default), +2,180 (orbit), +2,292 (bank2-gate), +228 to +260 on the bench sets (the closed-door data alone); bank 2 −160 B, ORBIT's −192 (2,048 B free); D2 −72 B (`MidiUi`'s slots).

## Alternatives considered
- **Pull compare of every stored param (as echo).** ~430 params × 6 Parts a pass, and it sends what MIDI and loads moved: feedback loops.
- **Independent of ECHO.** One switch that silences everything is worth keeping; ECHO is on by default.
- **Send `R` addresses too.** A recorded lane would do nothing on playback.
- **CC output (Elektron's CC option).** Chimera has no global CC chart (ADR 0128); NRPN is the frozen space.
- **Under PORT CONFIG** (Elektron's home for it). That row is a later issue (#265); ECHO's page is MIDI OUT's.

## Consequences
- A DAW can record panel edits and play them back through the canonical NRPN path.
- A bound control and its address both send when echo is on for the binding: two messages, both correct.
- A Part's channel shared by two Parts carries both Parts' edits; the receiver can't tell them apart (as with notes).
- An edit path that skips `cc_hand` doesn't send: new edit paths must call it, as they must already for the hand-wins rule.

## Sources
- #913; ADR 0216, 0217, 0222, 0243, 0251, 0254.
- Elektron Digitakt manual, MIDI CONFIG › PORT CONFIG › PARAM OUTPUT; PreenFM3 manual, MIDI settings.
