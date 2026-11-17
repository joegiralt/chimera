# 0280. Keep a DRUM kit to eight voice groups; a STACK pad steals only its own voices

- **Status:** Proposed
- **Deciders:** owner (2026-11-15, #993), firmware

## Context
A DRUM Part plays its kit from the 8-voice pool (ADR 0040), billed by machine (ADR 0274). With chokes off, a busy pattern on rev V refused hits: in the A/B of 2026-11-15, breakbeat line 5 refused 15 hits with the open HAT's, CYMBAL's and toms' chokes off. A spike (`drum-steal-spike`) let a hit steal as many released DRUM tails as it needed. It refused none, but it sounded worse everywhere except the breakbeat. A steal is a choke nobody chose. The same pattern on INIT's chokes (8 groups) and no new stealing refused 3. Those 3 came from the CPU budget (other Parts' chords, the metal machines), not the voice count.

The owner decided (#993) that the kit fits the pool by its chokes, not by stealing across pads. A choke group sounds one pad at a time, and a pad with choke OFF sounds on its own, so a kit of 8 such groups fits 8 voices. INIT DRUM already has exactly 8: KICK, SNARE, A (3+4), TOM 5, B (6+7), C (8+9), D (10+11), SHAKER. Two things could still break this. A choke edit could grow a kit past 8 groups. A STACK pad (TAPE's `Retrig`, ADR 0265) keeps every hit it makes. When the pool was full, its next hit stole the oldest voice (ADR 0015 rule 3), and that voice could belong to another pad.

## Decision
- **Voice groups, typed.** `dsp::drum::groups::VoiceGroups` counts a kit's groups: each choke group in use (A–D) counts once, and each pad with choke OFF counts once (1..=12). `VoiceGroups::MAX` is `MAX_VOICES` (8). `VoiceGroups::choke(kit, pad, group)` is the only check. It returns `Ok(count)` where the edit may land and `Err(count it would make)` where it may not. An edit may land when it keeps the kit within 8, or when it doesn't raise the count.
- **A choke edit that makes a 9th group is refused on the cell, never silently.** This is ADR 0151's pattern, extended to KEYS' CHOKE cell (`held::CHOKE`):
  - An encoder turn onto such a group lands on it dimmed, with the reason in the focus band, `9 GROUPS · KIT HOLDS 8`, and nothing is written (`UiState::turn_choke`, `Held::why_kit`, `Reason::Groups`).
  - A set no turn held (a CC, the console's `ed set`) is put back, and its reason is shown as a toast (`UiState::choke_undo`).
  - Choosing another pad drops the held choice.
- **The count is on the page.** CHOKE's focus band shows `GROUPS n/8` in its hint's place (`view::groups_note`). An old kit shows its count over 8, `GROUPS 11/8`, until the player brings it down.
- **Old kits load and play as they did.** The `Kit` record is unchanged. A kit past 8 groups loads as saved, is not lossy, and plays as before. On it, an edit that lowers or keeps the count lands, and one that raises it is refused, the reason naming the count it would make.
- **A STACK pad that needs room ends its own oldest voice, never another pad's.** In `drum_on`, after its CUT and chokes, a hit gathers its pad's live voices (`Instrument::own_voices`: same Part, same pad, not dying). The voice comes from `Allocator::pick_hit`:
  - A free voice that fits comes first, as before. A STACK pad grows into an empty pool.
  - Where the pad has live voices of its own, rule 3 (ADR 0015, 0027) takes only from them: its oldest tail first, then its oldest held voice. If that doesn't fit the budget, the hit is refused. Another pad's voice is never taken.
  - A pad with no live voice of its own (every CUT pad, after its own cut) steals as rule 3 always has, one voice if that makes room.
  - Choking the pad's group ends the group's other pads, so a pad's own voices are its group's.
- **No cross-pad steal.** The spike's multi-tail steal is not adopted. The CPU-budget refusals are tracked elsewhere: a DRUM Part RESERVE (#966) and cheaper machines (#991).
- **Bank placement (ADR 0198).** `ui::kit_groups` (the held check, the put-back, the count's and the reason's text) and `dsp::drum::groups` link into bank 2 behind four `UiDoor` entries (`choke_held`, `choke_undo`, `groups_note`, `groups_why`). The door table stays within its 128 B (asserted). With the door closed, nothing is held, no count is shown and a set lands, as with every door entry (ADR 0194 makes bank 2 required). The allocator's filter, `own_voices` and the UI's stubs are in bank 1. The allocator can't be behind a door, so it is an ADR 0251 exception, measured below.

## Alternatives considered
- **Steal released DRUM tails across pads until the hit fits (the spike):** it refused nothing on the breakbeat but sounded worse everywhere else, because a steal is a choke the player never chose. The owner rejected it on 2026-11-15.
- **Steal only the hit's own pad's or group's tails, several at a time (the spike's `group` mode):** the bounded kit keeps a group within its own voice, so a single own steal frees what a re-strike of the same machine needs. Several steals would only hide budget pressure that #966 and #991 address.
- **Refuse an old kit past 8 groups at load, or rewrite its chokes:** no legacy compatibility is owed, but a project must load safely, and silently rechoking a kit changes what the player saved. It loads as it is, and the page shows the count.
- **Count voice groups in the allocator (refuse a 9th group's hit):** this would refuse hits that fit today, and it duplicates what the choke edit already bounds.
- **A GROUPS cell of its own on KEYS:** this needs a new read-only slot binding and view in bank 1. The focus band's hint line already carries cell notes (TONAL's, TAPE's).

## Consequences
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
- `chimera-engine/src/dsp/drum/groups.rs`, `chimera-engine/src/voice_alloc.rs` (`pick_hit`), `chimera-engine/src/instrument.rs` (`drum_on`, `own_voices`), `chimera-core/src/ui/kit_groups.rs`, `chimera-core/src/ui/held.rs`.
