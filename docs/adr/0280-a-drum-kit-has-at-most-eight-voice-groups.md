# 0280. Keep a DRUM kit to eight voice groups; a STACK pad steals only its own voices

- **Status:** Proposed
- **Deciders:** owner (2026-11-15, #993), firmware

## Context
A DRUM Part plays its kit from the 8-voice pool (ADR 0040), billed by machine (ADR 0274). With chokes off, a busy pattern on rev V refused hits: in the A/B of 2026-11-15, breakbeat line 5 refused 15 hits with the open HAT's, CYMBAL's and toms' chokes off. A spike (`drum-steal-spike`) let a hit steal as many released DRUM tails as it needed. It refused none, but it sounded worse everywhere except the breakbeat. A steal is a choke nobody chose. The same pattern on INIT's chokes (8 groups) and no new stealing refused 3. Those 3 came from the CPU budget (other Parts' chords, the metal machines), not the voice count.

The owner decided (#993) that the kit fits the pool by its chokes, not by stealing across pads. A choke group sounds one pad at a time, and a pad with choke OFF sounds on its own, so a kit of 8 such groups fits 8 voices. INIT DRUM already has exactly 8: KICK, SNARE, A (3+4), TOM 5, B (6+7), C (8+9), D (10+11), SHAKER. Two things could still break this. A choke edit could grow a kit past 8 groups. A STACK pad (TAPE's `Retrig`, ADR 0265) keeps every hit it makes. When the pool was full, its next hit stole the oldest voice (ADR 0015 rule 3), and that voice could belong to another pad.

## Decision
- **Voice groups, typed.** `dsp::drum::groups::VoiceGroups` counts a kit's groups: each choke group in use (A–D) counts once (a `ChokeSet`), and each pad with choke OFF counts once. The count is 1..=12 by construction. `VoiceGroups::MAX` is `MAX_VOICES` (8). `VoiceGroups::choke(kit, pad, group)` is the only check. It returns `Ok(count)` where the edit may land and `Err(count it would make)` where it may not. An edit may land when it keeps the kit within 8, or when it doesn't raise the count.
- **A choke edit that makes a 9th group is refused on the cell, never silently.** This is ADR 0151's pattern, extended to KEYS' CHOKE cell (`held::CHOKE`):
  - An encoder turn onto such a group lands on it dimmed, with the reason in the focus band, `9 GROUPS · KIT HOLDS 8`, and nothing is written (`kit_groups::Ask::Turn`, `Ask::Held`, `Reason::Groups`). The held choice is typed, and the cell is the choice's: `held::Held` is `Sound(Pick)` (`Wave(Op, OpKind)`, `Alg(AlgSide, AlgoId)`, `Tape(TapeTurn)`, where `TapeTurn` is `Model(TapeMachine)` or `Direction(Direction)`) or `Choke(ChokeHold { to: ChokeGroup, kit: KitChokes })`. Its address is derived from it, so no cell and choice can disagree. A turn steps `ChokeGroup::step`, which saturates at the ends.
  - A step no turn held (a relative CC, a stepped CC's bin, or a pushed turn) skips the groups that would make a 9th and lands on the next that fits its way, as the encoder can turn on past a held one. A CC ramp passing a refused group says nothing.
  - An edit that can't land (nothing fits that way, or a set onto a 9th) is worked out on a copy of the pad and not written. The kit and the project's revision stay as they were, so the Part doesn't read Edited. Its reason is shown as a toast, once while that toast is up: a controller sweep sends many messages (`kit_groups::Ask::Edit`). The console's `ed set` reaches only ALGO's cells today, so it can't set a CHOKE.
  - Choosing another pad drops the held choice. So does any change to that kit's chokes since the hold, such as a kit loaded in with other chokes or a CC on another pad's choke: the hold carries the kit's chokes at the time (`KitChokes`). Edits elsewhere (another Part's CC, an expression pedal) leave it, so a turn can go on past a held group.
- **The count is on the page.** CHOKE's focus band shows `GROUPS n/8` in its hint's place (`view::groups_note`, `Ask::Note`). An old kit shows its count over 8, `GROUPS 11/8`, until the player brings it down.
- **Old kits load and play as they did.** The `Kit` record is unchanged. A kit past 8 groups loads as saved, is not lossy, and plays as before. On it, an edit that lowers or keeps the count lands, and one that raises it is refused, the reason naming the count it would make.
- **A STACK pad that needs room ends its own oldest voice, never another pad's.** In `drum_on`, after its CUT and chokes, a hit gathers its pad's live voices (`Instrument::choke` returns them with the hit: same Part, same pad, not dying). The hit carries a `Steal`: `Only(VoiceSet)` of those voices, or `Any` when the pad has none. The voice comes from `Allocator::pick_hit`, which is `pick_where` with that `Steal`:
  - A free voice that fits comes first, as before. A STACK pad grows into an empty pool.
  - Where the pad has live voices of its own, rule 3 (ADR 0015, 0027) takes only from them: its oldest tail first, then its oldest held voice. If that doesn't fit the budget, the hit is refused. Another pad's voice is never taken.
  - A pad with no live voice of its own (every CUT pad, after its own cut) steals as rule 3 always has, one voice if that makes room.
  - Choking the pad's group ends the group's other pads, so a pad's own voices are its group's.
  - With two STACK pads, one filling the pool leaves the other stealing only its own voices, one at a time once it has any (its first hit, with none, steals as rule 3). A CUT pad still steals from either.
- **No cross-pad steal.** The spike's multi-tail steal is not adopted. The CPU-budget refusals are tracked elsewhere: a DRUM Part RESERVE (#966) and cheaper machines (#991).
- **Bank placement (ADR 0198, bank 1 frozen).** `ui::kit_groups` and `dsp::drum::groups` link into bank 2 behind one `UiDoor` entry, `groups`. It takes an `Ask`: a CHOKE turn (`&mut UiState`, as `pit` and `tuning` take it), a CHOKE edit, a held choke's reason, the reason's words, or the count. It answers with a `Said`: `Moved`, `Refused`, `Fits` or `Line`. The turn, the edit, the held check, the toast and both texts are bank 2's. Bank 1 keeps one stub at each call site: `turn_choice`, `edit_in`, `held_choice`, `view::groups_note`. It also keeps `Reason::Groups`, which carries the count. The door table stays within its 128 B (asserted). With the door closed, nothing is held, no count is shown and an edit lands, as with every door entry (ADR 0194 makes bank 2 required). The allocator's filter and `choke`'s own-voice set are in bank 1, because the audio path can't call through a door. The filter is `Steal::pool().contains(v)` on rule 3's existing scan in `pick_where`, with no second pick function. The budget lines give the measured cost.

## Alternatives considered
- **Steal released DRUM tails across pads until the hit fits (the spike):** it refused nothing on the breakbeat but sounded worse everywhere else, because a steal is a choke the player never chose. The owner rejected it on 2026-11-15.
- **Steal only the hit's own pad's or group's tails, several at a time (the spike's `group` mode):** the bounded kit keeps a group within its own voice, so a single own steal frees what a re-strike of the same machine needs. Several steals would only hide budget pressure that #966 and #991 address.
- **Refuse an old kit past 8 groups at load, or rewrite its chokes:** no legacy compatibility is owed, but a project must load safely, and silently rechoking a kit changes what the player saved. It loads as it is, and the page shows the count.
- **Count voice groups in the allocator (refuse a 9th group's hit):** this would refuse hits that fit today, and it duplicates what the choke edit already bounds.
- **A GROUPS cell of its own on KEYS:** this needs a new read-only slot binding and view in bank 1. The focus band's hint line already carries cell notes (TONAL's, TAPE's).

## Consequences
- Everything here stays fully typed, in frozen bank 1 too (owner, 2026-11-17): typing is never traded for flash. The typed held choice costs bank 1 what the budget lines under this ADR record.
- An INIT-derived kit never takes another pad's voice through STACK. A STACK pad holding the pool ends its own oldest voice. A crowded pool (other Parts) still loses its oldest voice to a hit with no voice of its own, as ADR 0015 has it.
- The CHOKE cell's hint line shows `GROUPS n/8` in place of `MIX + -/+ PAD`. The PAD and RETRIG cells on KEYS still show it.
- `drum_keys`' golden is re-recorded for the note.
- The A/B harness (`chimera-core/tests/drum_groups_ab.rs`, ignored) renders the spike's six patterns on this tree in modes `today` (an 11-group old kit) and `groups` (INIT's 8).
- Flash, measured at the end of the branch (ORBIT and default release builds): see the budget lines in `tools/bank1-budget.txt` and `tools/bank2-budget.txt`, which cite this ADR.

## Supersedes in part
- [0015](0015-voice-steal-and-fx-returns.md) rule 1 ("steal the oldest released non-mono voice first … from any part"), for a DRUM hit whose pad has live voices: it steals only from them.
- [0265](0265-drum-voice-and-kit-part.md) "Hits": "then takes a voice without re-striking its key's (STACK keeps the old one)". It still does, until the pool is full. Then a STACK pad ends its own oldest voice.
- Extends [0151](0151-a-refused-kind-or-algorithm-is-shown-dimmed-with-its-reason.md)'s dimmed-with-reason refusal to DRUM's CHOKE.

## Sources
- Issue private issue #993 (owner decision 2026-11-15); #966 (RESERVE), #975 (DRUM voice floor), #991 (bills).
- A/B: an unpublished page; spike branch `drum-steal-spike` (cf6cbf70 and its uncommitted `group` mode), not merged.
- ADR 0015, 0027, 0040, 0151, 0194, 0198, 0251, 0265, 0274.
- `chimera-engine/src/dsp/drum/groups.rs`, `chimera-engine/src/voice_alloc.rs` (`pick_hit`), `chimera-engine/src/instrument.rs` (`drum_on`, `choke`), `chimera-core/src/ui/kit_groups.rs`, `chimera-core/src/ui/held.rs`.
