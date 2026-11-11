# 0263. Each Part answers GM's CCs on its channel, below its own map

- **Status:** Proposed (supersedes in part 0226: "the permanent layer sits above both, and its CCs are refused to learn")
- **Deciders:** owner (#807's permanent map; #880; 2026-11-11: "i trust you", the CC list and the rules), firmware

## Context
#807's owner design gave each Part three layers: a permanent built-in map, the controller map (ADR 0226) and the Part's learned rows. ADR 0226 built the second and third and left the first. A DAW's mixer lanes, a GM sequence or a keyboard's sliders send GM's CCs and expect them to work with no learning.

On 2026-11-11 the owner fixed the list: CC 7 LEVEL, 10 PAN, 11 expression, 71 RESONANCE, 72 RELEASE, 73 ATTACK, 74 CUTOFF, 91 REVERB, 93 CHORUS, 94 DELAY, the GM/GM2 and RP-021 sound controller numbers, on each Part's own channel. CC 1 stays the WHEEL source (ADR 0236, 0250). The owner also set the rules:
- a binding of the Part's own for the same CC wins;
- under MPE, CC 74 is SLIDE (ADR 0252) and must not move CUTOFF on member channels;
- CONTROL channels (ADR 0254) are unaffected;
- a SETTINGS switch turns it off;
- moves glide;
- PARAM OUT stays as ADR 0258 has it.

ORBIT's bank 2 has about 400 B free (ADR 0251), and D2 is full.

## Decision
- **A fixed table, read through the canonical path.** `midi_map::builtin::builtin` maps a CC, the Part's engine and SYSTEM's MIDI settings to canonical NRPN addresses (ADR 0216), which `cc_canonical` moves as it moves an unbound NRPN: through a canonical slot's ramp or bin, `Jump`, the hand winning its pass. That is MIXER's way (ADR 0254).
  - A 7-bit value is the NRPN's coarse value.
  - A continuous cell (every target but ALGO's) covers its range.
- **Targets by engine:**

  | CC | Target |
  |---|---|
  | 7 | the Part's LEVEL |
  | 10 | the Part's PAN |
  | 11 | expression (below) |
  | 71 | FILTER RESONANCE |
  | 74 | FILTER CUTOFF |
  | 91 | OUT REVERB send (`SEND3`) |
  | 93 | OUT CHORUS send (`SEND1`) |
  | 94 | OUT DELAY send (`SEND2`) |
  | 73 ATTACK (relative) | VA and TAPE: ENV 1 ATK, their VCA's envelope (`Sound::plain_init`). ALGO: every operator's AR. MODAL: nothing |
  | 72 RELEASE (relative) | VA and TAPE: ENV 1 REL. ALGO: every operator's RR. MODAL: DAMP |

  - On ALGO, every operator is RANDOMIZE's precedent for "the amp envelope" (ADR 0232). MODAL rings until DAMP stops it (note-off never damps), and its strike has no attack to move.
- **ATTACK and RELEASE are relative, RP-021's way** (review of #942: an absolute value wiped an ALGO patch's per-operator envelope shape).
  - 64 is the Sound as it is. Each target is the Sound's own value shifted by the CC's offset from 64, then clamped (`UiState::cc_shift`), so the operators keep their shape.
  - A continuous cell (ENV 1 ATK and REL, DAMP) moves 1/127 of its range a step.
  - ALGO's rates move by the rounded steps of the offset (31/127 of a step for AR, 14/127 for RR), turned round, since a higher TX81Z rate is a shorter time. AR stops at 1, because AR 0 never rises.
  - The writes go through the edit path, so the voice smooths them as it does a panel turn. ATK, REL, AR and RR are not knobs; DAMP is, and waits for room.
  - The applied offsets live in the Part's padding (`Part::env`, two bytes, beside `expression`: on the chip a `Part` had three spare bytes), so they cost no RAM. A Sound load clears them (the new Sound is 64), a refused one keeps them, and a project load clears them.
  - 64 gives each cell back exactly, at any limit (unit e2e, 2026-11-11: AR at 31, CC 73 at 0 then 64 had left AR at 15). The shift never moves a clamped value again: it moves the Sound's own, kept for each shift while it is off 64 (`builtin_cc::base`, ALGO's six 5-bit rates or the cell's `f32` in a word, 48 B of AXI `.bss`: D2 is full). A cell that no longer holds what its kept value shifts to was edited since, and its unshifted value is taken instead. At 64 the cell is the Sound, so the saves and loads that reset the offset need nothing more.
  - **What you hear is what you see and save** (owner, 2026-11-11, "principle of least surprise"). The shift is written into the Sound through `edit_in`, so the page shows the shifted values and any save stores them as they sound, offset applied and clamped. A save then resets the baseline: CC 64 is the Sound as saved. The resetting saves are `Project::save_part_to` (SAVE TO PROJ, a new slot), a project save (`store`: every Part, the MENU-hold quick save among them) and a LIBRARY save (`Project::reset_shift`). Expression is unchanged: a performance control, never saved (GM).
  - Reset All Controllers takes each Part's shifts on its channel back to 64 (`cc_reset` runs `cc_shift` at 64), so RAC is the Sound as saved, and expression is back to 127.
- **Expression (CC 11)** is `Part::expression`, a byte beside `loads` in the Part's padding (no RAM), 127 by default. It is published as `PartAudio::expression`, also in padding. The mix scales the Part's LEVEL **and its three sends** by (expression / 127)² (`part::expressed`), GM's 40 log₁₀ law: a swell to 0 silences the Part's FX feed too (review of #942). At 127 the factor is 1.0, so every gain is bit for bit as before. The mix eases the gain per sample (`MixState`), so it never steps. It was first a `PartParams` field, but that struct had no padding: +4 B a Part in D2 overflowed `bench-algo`'s D2 by 16 B.
  - The codec never writes it. A project load (each Part's record) and NEW bring it back to 127, and `project_crc` (the `*`) never sees it.
  - Reset All Controllers sets it to 127, as GM resets CC 11. The pipe hands the UI its own event, `CtrlEvent::Reset`, beside the note path's reset. Learn never hears it, and a CC 11 binding doesn't move.
- **Precedence.** A binding on the CC in the Part's map wins over the built-in layer: the Sound's rows and the controller map's layer alike, even a row dimmed for another engine. Learn still binds these CCs. Then the layer. Then nothing.
- **MPE.** A member channel's controls never reach a Part's map (`cc_mpe`), so they never reach the layer either. On an active zone's master channel, CC 74 is SLIDE alone: the layer skips CUTOFF there, and so does a map row binding CC 74 to CUTOFF (`builtin::slide_only`), the row INIT and the factory Sounds ship. Its other CCs still answer, as the master is the zone's Part channel.
- **CONTROL channels.**
  - MIXER's and SEQUENCER's channels are never a Part's (`control_on`), so their charts, which also use CC 71–74 and 91–94, never meet the layer.
  - AUTO rewrites to the focused Part's channel in the pipe, so the layer answers there as the Part's map does.
  - None of the ten CCs is one MIDI in keeps (0, 6, 32, 38, 64, 96–101, 120–127): each is a `FreeCc`, pinned by a test.
- **The switch.** MIDI CONFIG › CONTROL CHANNELS › D, **BUILT-IN** (`OFF`/`ON`, default `ON`): `MidiSettings` param 13, ident `BUILT_IN`, a bit of `Switches`. `MidiSettings` keeps its size. A SYSTEM from before it loads `ON`.
- **Echo and PARAM OUT.** A built-in move has no binding, so no value echo (ADR 0217). It never passes `hand_in`, the shifts included (they write through `edit_in`), so PARAM OUT never sends it (ADR 0258): only on-unit turns go out. A test pins it (`built_in_moves_never_go_out`).
- **A fix it needed.** `cc_claim` (#512) stopped every other Part's glide on the same address, LEVEL and PAN included. Those are each Part's own, so two Parts' CC 7 in one pass left the first mid-glide (MIXER's CC 20–31 too). The claim now skips the `Part` block. Its canonical slots are indexed from `BINDS`, not `MAP_LEN`, the controller layer's slots having moved them (ADR 0226).
- **Placement: bank 1 (ADR 0251).** ORBIT's bank 2 is under 0251's floor. `midi_map::builtin` sits outside bank 2's `midi_map::codec`/`ctl` globs and is `inline(never)`. Bank 2 (`ui::midi`) gains only the call and the `Move` loop, plus the pipe's reset line.
  - Bank 1, measured over main 562bb0ed's lines, each raised naming this ADR:

    | Set | Bank 1 |
    |---|---|
    | `default` | +1,476 B |
    | `orbit` | +1,500 |
    | `bank2-gate` | +1,460 |
    | `orbit,usb-config-writer` | +1,516 |
    | `fault-inject` | +236 |
    | `bench-algo` | +396 |
    | `bench-algo,master-tape` | +348 |
    | `bench-routing` | +388 |
    | `bench-memory` | +404 |
    | `bench-stress` | +396 |
    | `bench-orbit` | +364 |
    | `bench-tape` | +388 |
    | `filter-probes` | +236 |

  - On `default`: `cc_shift` 652 B, `builtin` 280 B (a table; as a `match` it was 854 B), `cc_reset` 96 B, `slide_only` 62 B. The rest is BUILT-IN's spec and its get and write arms, CONTROL's D cell, the Part's shift reset at a load and expression in the mix.
  - The save-is-the-Sound round (saves reset the baseline, RAC undoes the shifts) added `default` +280 B, `orbit` +280, `bank2-gate` +272, `orbit,usb-config-writer` +248, each line raised again; the bench sets +24 to +48, within the slack.
  - ORBIT's bank 2: 523,872 → 524,096 B (+224, the dispatch, the `Reset` event's pack, unpack and push, and `slide_only`'s call), 192 B under the 512 KiB region.
  - No RAM.
  - The exact-64 fix (the unit's e2e, measured against main f09932d9) added `default` +248 B, `orbit` +232, `bank2-gate` +248, `orbit,usb-config-writer` +248, each line raised; the bench sets 0. `.bss` +48 B (the kept values); D2, bank 2 and the stack unchanged.

## Alternatives considered
- **The layer above the Part's map, its CCs refused to learn** (ADR 0226's sketch). The owner chose the other way: a controller that sends CC 74 for something else binds it and wins, with no switch needed.
- **Built-in rows as a fourth binding section** (32 slots more, takeover kinds, echo). A fixed chart needs none of it, and the canonical path already ramps, bins and seats.
- **Expression as a canonical param in the Sound or a stored Part cell (`PartParams`).** It would be saved and show `*`. GM treats expression as performance, reset by RAC.
- **Expression per channel in the audio**, beside the wheel. That is a new note-queue kind, a pipe change in bank 2 and a per-channel glide in bank 1, for the same sound.
- **Expression scaling LEVEL alone** (the first cut). The sends are pre-level, so a swell to 0 still fed the FX.
- **Absolute ATTACK and RELEASE** (the first cut). One value for six operators wiped an ALGO patch's envelope shape.
- **A relative offset applied in the voice, never written.** That is RP-021's ideal, but it touches each engine's envelope code, which is bank 2's.
- **ALGO's carriers only.** Which operators are carriers depends on the algorithm and MORPH. RANDOMIZE already treats every operator's envelope as ALGO's amp envelope.
- **Dropping the six shared rows (CC 74, 71, 7, 10, 91, 93) from INIT's and the factory Sounds' maps** (spec § 9). They now repeat the layer, but they are the Sounds' data, and removing them changes every Sound's MIDI MAP page. Left as they are: removing them is not simple (factory data, wheel maps, many tests). The CC 74 row now yields to SLIDE on an MPE master, as the layer does.

## Consequences
- Any GM source moves the obvious cells on any Part with no setup, and a learned binding still overrides it per CC.
- CC 7, 10, 71, 72, 73, 74, 91, 93 and 94 move stored cells: a Sound cell shows `*`, and LEVEL and PAN are project edits, as with a binding. CC 11 is never an edit.
- A controller that sends one of these CCs for something unrelated moves a cell until it is bound or BUILT-IN is `OFF`.
- Ready-made controller maps (#807's "ship ready-made maps") stay out: the owner wants no device-specific maps.

## Sources
- MIDI 1.0 Detailed Specification (CC 7, 10, 11, 91, 93, 94; Reset All Controllers); General MIDI 2 (CC 11 and CC 7 at 40 log₁₀); MMA RP-021, Sound Controller Defaults (CC 71–74).
- #807 and its comments, #880; ADR 0139, 0216, 0217, 0226, 0232, 0236, 0251, 0252, 0253, 0254, 0258.
