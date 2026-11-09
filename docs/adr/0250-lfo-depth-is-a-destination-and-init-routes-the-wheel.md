# 0250. Make LFO DEPTH a destination and route INIT's wheel to it

- **Status:** Proposed
- **Deciders:** owner (MIDI IO design 2026-10-15, decision 4); firmware

## Context
ADR 0236 made the mod wheel a matrix source (WHEEL). LFO blocks were not voice-read destinations, though, so INIT kept its vibrato as a map entry, CC 1 → LFO 1 DEPTH. Each wheel move was then a Sound edit: the Part showed Edited and its stamp moved, though the player had only played. The voice runs its LFOs from the played Sound before the matrix sums, so a route written into the played copy after the sums is overwritten next block before an LFO reads it. ENV slots meet the same order with `EnvMods`: last block's sums, read by the slot this block (spec § Signal flow 1). Bank 1 is frozen (ADR 0198). Most factory Sounds were voiced with LFO 1 at DEPTH 1 and a route from LFO 1, so CC 1, a jump, is how the wheel plays them.

## Decision
1. **LFO DEPTH is modulatable.** `LFO_SPECS` DEPTH takes routes and `BlockRef::Lfo` is voice-read. RATE, SHAPE and the rest of the LFO's params do not. The matrix header reads `DPTH` (`short`).
2. **Applied as EnvMods, one block late, in bank 2.** Before its LFOs run, a voice that isn't fading sums each LFO's DEPTH column over the last block's source values (`Voice::mod_values`, which it keeps already). On a retrigger it uses the new note's VEL, NOTE, WHEEL and PRESS, as the ENV slots' retrigger sums do. It adds the sum to the played copy's DEPTH on its Linear law, clamped to 0..1. A zero sum leaves DEPTH as stored, so an unrouted LFO runs bit for bit as before. A fading voice keeps the depth it last played. FUNC ignores DEPTH, as it did. The pass is bank 2's (`voice::depth::kern`, behind `Bank2Door::depth`, `DepthDoor { run }`). With the door closed, DEPTH takes no routes and plays as stored. Bank 1 keeps only the door call and the general route loop's skip of LFO columns.
3. **INIT routes WHEEL → LFO 1 DEPTH at +127** (registry label `LF1 DEPT`), beside LFO 1 → PITCH at 2. LFO 1 DEPTH rests at 0. At rest INIT sounds bit for bit as before.
4. **CC 1 leaves the default map** (`default_for`). There are six shared entries: ten on ALGO, MODAL and VA, six on TAPE. Moving the wheel on INIT, or on a Sound built from it, no longer edits the Part.
5. **The factory rule** (`Sound::factory_map`, used by the firmware blob's and the desktop library's generators): a factory Sound whose LFO 1 has a nonzero route keeps CC 1 → LFO 1 DEPTH where it always sat (`wheel_map_for`), and no WHEEL route. Factory TAPE Sounds start from `Sound::factory_init`, which is INIT without the WHEEL route. These Sounds encode byte for byte as before and play the wheel as they were voiced. The wheel still edits them and marks the Part Edited. A factory Sound without an LFO 1 route gets the plain default map, since CC 1 moved an LFO nobody heard. Whether to move them to DEPTH 0 and WHEEL +127 instead is a later ear decision for the owner.
6. **Streaming's reach** (`Admission::reach`, #698) takes an LFO's swing as its DEPTH plus its DEPTH routes' |amounts|, WHEEL's at where the wheel sits and every other source's at full, clamped to 1. INIT at rest reaches nothing, and a wheel moving up re-bills the note (`refresh_in`).

## Alternatives considered
- **Write the route into the played LFO block after the sums** (the generic path). The next block's `clone_from` overwrites it before the LFO reads it.
- **Run the LFOs after the sums.** The LFOs are sources of those sums, so DEPTH routed from an LFO would read itself.
- **Keep the sums in the voice, as `env_mods`.** That costs 16 B a voice with padding, 128 B of D2, and `bench-algo` no longer links (D2 over by 8 B). The source values the sums come from are kept already.
- **The DEPTH pass in bank 1, as an audio-path exception to ADR 0198.** It measured +200 to +500 B a set, and ADR 0198 § 3 wants a bench row a bank for that, which can't be taken while the unit is away. Behind the door it costs bank 1 only the call.
- **Keep CC 1 in the map and add the route on INIT.** The wheel would both route and edit, doubling the depth and still dirtying the Part.
- **Migrate the factory Sounds to WHEEL +127.** Their LFO 1 sits at DEPTH 1, so the clamp makes the route inaudible and the wheel would do nothing (option b, DEPTH 0 and WHEEL +127, changes the sound at rest: an ear decision).
- **Bill a routed DEPTH at full swing** (`moves_addr` → 1). INIT's vibrato at rest would cost a streamed TAPE chord its last note (#698's finding 1).

## Consequences
- **Voice RAM:** none. `Voice` stays 6,696 B.
- **Render cost:** with the door open, each voice makes one door call a block, a scan of its registry columns. INIT has one DEPTH column, so it pays a ten-row sum a voice a block.
- **INIT's file:** gains a registry column and a `PerfRoutes` record. Its matrix screens show the new column, headed LF1 DPTH.
- **Factory files:** unchanged where LFO 1 is routed. The rest lose their CC 1 entry.
- **Bank 1:** grows by the door call and INIT's second route. That is +104 to +168 B on the sets past the slack (default +120, orbit +128, on main f52b0da6), at the lines in `tools/bank1-budget.txt` that name this ADR. There is no audio-path exception, so no bench row is owed.

## Sources
- #876, #514, #698; ADR 0236 (WHEEL and PRESS), 0198 (bank 1 frozen), 0195 (the gate's door, the pattern), 0139 and 0217 (the map); MIDI IO design `docs/superpowers/specs/2026-10-15-midi-io-design.md` (branch `midi-io`, commit 2ab016a3), decision 4.
