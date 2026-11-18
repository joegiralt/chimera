# 0281. Give each Part a MAX VOICES cap and a RESERVE floor in the shared pool

- **Status:** Proposed
- **Deciders:** owner (approved #966 on 2026-11-15, from dspmax's request in chimera-releases#5), firmware

## Context
Chimera's six Parts share one pool of 8 voices (ADR 0015, 0027, 0040). When the pool is full, rules 3 and 4 of `Allocator::pick_where` steal the oldest non-mono voice of any Part. The load net's `shed_one` (ADR 0273) also picks across all Parts. Stock PreenFM3 gives each timbre a fixed set of voices (`Synth.cpp:719-736, 776-790`) and steals only within the timbre (`Timbre.cpp:378-433`). In chimera-releases#5, a VA pad with long held notes cut out whenever Part 5 got busy. MONO on Part 5 was too little. The owner approved a per-Part cap and floor (#966). The default had to keep today's behaviour bit for bit.

## Decision
- **Two Part settings, typed.** These live in `chimera_engine::voice_limit`:
  - `VoiceCount` is 0..=8 (`MAX_VOICES`) by construction.
  - `VoiceCap` is 1..=8.
  - `MaxVoices` is `Auto` or `Cap(VoiceCap)`.
  - `Reserve` wraps a `VoiceCount`.
  - `VoiceLimit { max, reserve }` holds both, and `Limits` holds one per `PartId`.
  - A Part's live voices are a `VoiceSet` (booked, not dying), and its count is `VoiceSet::count`.
  - The defaults are `MaxVoices::Auto` and `Reserve::NONE` (`VoiceLimit::AUTO`).
- **MAX VOICES (the cap).** A Part's live voices never exceed its cap. Rule 1 (Mono retrigger) and rule 2 (a key re-struck) come first, as before, because they reuse a voice the Part already owns. A new note on a Part at its cap does not take a free voice. It steals one of the Part's own non-mono voices, using rule 3's order (oldest tail, then oldest held note), even with free voices in the pool. If that steal does not fit the CPU budget, the note is refused (rule 4). AUTO caps nothing.
- **RESERVE (the floor).** A Part at or below its reserve, counting live voices, is guarded: another Part's note can't steal its voices (rules 3 and 4). A Part's own note may still take its own voices (at its cap, or in rule 3 when the oldest voice is its own). A Part above its reserve can lose voices one at a time, down to its reserve. A reserve doesn't hold free voices back. A Part with nothing sounding still gets a free voice first, or steals from an unguarded Part. A reserve of 0 guards nothing: a Part with a voice has a count of at least 1. Mono voices are never stolen (ADR 0015), guarded or not.
- **Shed (ADR 0027, 0273).** `shed_one` keeps its order (non-mono first, the oldest tail, then the newest held note), but guarded voices go last: `(guarded, mono, held, age)`. A budget shed or the load net takes an unguarded voice whenever one sounds. It takes a guarded voice only when every voice left is guarded. The reserve protects a Part from other Parts' notes. An overrun belongs to no Part, and a block past its deadline glitches every Part, so the net must always be able to free a voice. The cap needs nothing in shed: shedding only lowers a count.
- **Defaults are today's.** With every Part on `AUTO` and a reserve of 0, the cap branch is never taken and no voice is guarded. `pick_where` and `shed_one` then make the same choices as before, so the goldens are bit-identical.
- **What can't be honoured is refused on the cell (ADR 0151's pattern).** These are the rules:
  - A Part's RESERVE may not exceed its own MAX VOICES. The reason reads `RESERVE n · VOICES m`.
  - The six Parts' reserves may not sum past the pool's 8. The reason reads `RESERVES n · POOL HOLDS 8`. A reserve of 8 on one Part is allowed: that Part's voices can never be stolen by another Part's note.
  - `ui::part_voices::refusal` checks an edit against the project's other Parts. It refuses an edit that raises either excess, reserve over cap or reserves over 8. It never refuses an edit that keeps or lowers both, so a project saved past the rules can only come down, as an old kit does in ADR 0280. In normal use both excesses start at 0, so any excess is refused.
  - An encoder turn onto a refused value lands on it dimmed, with the reason in the focus band, and nothing is written (`held::Choice::Voices { to, rev }`, `Reason::Voices`). The next turn continues from the held value. The hold drops when the project changes (`rev`), as ADR 0280's held CHOKE does.
  - A set that no turn held (any `edit_in` path) is worked out on a copy and not written. Its reason is shown once as a toast.
  - The cap's other side is also refused: turning MAX VOICES below the Part's RESERVE is refused with the same `RESERVE n · VOICES m` reason.
- **The count is on the page.** The RESERVE cell's focus band shows `RESERVES n/8` in its hint's place, the sum over all six Parts.
- **Where the cells live.** MIXER › PRT is full (CH, MODE, OUT, LEVEL, PAN, BEND), so PRT gets one sub-page, VOICES (`VOI`, `MixPage::Voices`). It is down from PRT as a sub-page is (ADR 0081, 0145): EDIT under STACK, SEQ under ROW, and back up the other way. Until now EDIT on PRT went straight down to the Sound rung. Under STACK it now reaches VOICES first, and EDIT on VOICES goes on to the Sound rung. Under ROW, EDIT from PRT still goes to the Sound rung. Its cells are MODE, MAX VOICES and RESERVE, so the cap sits beside MONO/POLY. MODE stays on PRT too. RESERVE fits a cell in full. MAX VOICES is 76 px against the cell's 62, so its cell reads `VOICES`, the full word that fits, and the refusal says `VOICES` to match; its disk ident stays `MAX_VOICES`. No hidden gesture is added.
- **DRUM (ADR 0265, 0280).** The rules compose, because a DRUM Part's cap is its own smaller pool:
  - A hit's `Steal` (ADR 0280) is applied inside the Part's voices at the cap: `Steal::Only(own) ∩ part's`, or every voice of the Part for `Any`. A STACK pad with live voices still ends only its own oldest voice. A CUT pad's hit, or a pad with no voice left, at the cap ends the Part's oldest voice: the oldest tail first, then the oldest held. That voice may be another pad's. ADR 0280 already lets such a hit steal any voice of the pool. The cap only narrows the pool to the Part.
  - Chokes and CUT run first (`Instrument::choke`). The voices they end are dying, so they no longer count toward the cap, and the hit takes a free or dying slot as before.
  - A kit holds at most 8 voice groups (ADR 0280). A DRUM Part capped below its group count sounds at most its cap at once. The next group's hit takes the Part's oldest voice. This is the player's choice and is not refused on the cell. The `GROUPS n/8` note on CHOKE is unchanged.
  - A DRUM Part's RESERVE guards its hits from other Parts' notes. This is the use ADR 0280 deferred to #966: a busy synth Part can no longer take a hit's tail.
- **Mono.** A Mono Part retriggers its one voice (rule 1), which is never stolen. A cap of 1 is the same as Mono for voice count, but it steals rather than retriggers, so a new note on a capped Poly Part starts a new voice in the Part's slot with its own envelope.
- **One byte with MODE (AXI).** Each Part's block is copied into the audio's three snapshots (ADR 0021). `PartParams` was 16 B with no padding, so any new field cost 4 B × 6 Parts × 3 = 72 B of AXI. That put ORBIT 12 B under ADR 0087's 32 KiB margin. So MODE, MAX VOICES and RESERVE share one byte, `part::Voicing`: `mode × 81 + MAX VOICES × 9 + RESERVE`, below 162 by construction, decoded with `mode()` and `limit()`. `PartParams` replaces its `mode` field with `voicing`, read and written through `mode()`, `set_mode`, `voices()` and `set_voices`. It stays 16 B (asserted in `part_voices_test`). Mode and the two limits belong together: they are the Part's voicing, beside each other on VOICES.
- **Saved with the Part.** `PartParams` gains MAX VOICES and RESERVE in `voicing`, with `PartParams::MAX_VOICES` (13) and `RESERVE` (14), both choice cells stored by enum code: MAX VOICES `AUTO` = 0 and `1`..`8` = 1..8, RESERVE `0`..`8` = 0..8. Disk idents are `AUTO`, `1`..`8` and `0`..`8`. The project codec stores the Part block generically, so the entries come with it. An old project has neither id and loads with AUTO and 0, the target's values. An unknown code keeps the default. A project past the rules loads as saved and plays by the allocator's rules above, which are defined for any values.
- **MIDI and `ed`.** Only LEVEL and PAN on the Part block are mappable (`midi_map::Mappable`, ADR 0217), so MAX VOICES and RESERVE get no CC, NRPN or learn, the same as CH and MODE. `ed set` reaches only ALGO's cells (ADR 0276), so the console can't set them. Neither was added.
- **The audio side reads the settings once a snapshot.** `Instrument::recost` hands `Allocator::set_limits` the six Parts' limits, before any note-on of the block is judged. There is no allocation and no lock. When reserves and caps are default, the per-pick cost is the cap check, one scan of 8 slots. Guard sets are built only where rule 3 steals or a shed picks.
- **Bank placement (ADR 0198, 0251; bank 1 frozen).** The allocator's cap and guard, `set_limits`, the `voice_limit` types, and the two specs with their get/write/code arms are bank 1. The audio path can't call through a door, and specs are closed-door data. The PRT sub-page's `BlockDef` is bank-1 data too. `ui::part_voices` holds the rule, the turn, the edit, the held check, the toast and the words. It links into bank 2's S4 group behind one `UiDoor` entry, `voices`, which takes an `Ask` (`Turn`, `Edit`, `Held`, `Why`, `Note`) and answers with a `Said`. Bank 1 keeps one stub at each call site, as ADR 0280 does. With the door closed, nothing is held, no note is shown, and an edit lands unchecked. The budget lines carry the measured deltas.

## Alternatives considered
- **Static voice ownership, as stock does.** This splits 8 voices into fixed shares, so an idle Part's share is wasted. With six Parts on 8 voices that is too few for anyone. A floor that guards only against stealing keeps the pool shared.
- **A reserve that holds free voices back for its Part.** This is closer to stock. But it refuses another Part's note while voices sit free, which sounds like a bug. It also can't be bit-identical at 0 without a second free-voice path. Guarding against steals covers the reported case: the pad's held notes are what got stolen.
- **Shed never takes a guarded voice.** Then the load net could leave a block overrunning, which is worse for every Part, the guarded one included (ADR 0273's reset). Guarded voices go last instead.
- **Clamp a reserve to the cap silently, or scale reserves down at load.** That would change what the player saved without saying so. Refusing an edit on the cell and loading a project as saved matches ADR 0151 and 0280.
- **Part priority (keep one Part ringing whatever happens).** Filed as a separate follow-up in the issue. RESERVE covers the reported use.
- **Put MAX VOICES and RESERVE on PRT in place of BEND or OUT.** This moves cells players know. A sub-page under PRT keeps PRT as it was.

## Consequences
- A pad held on a Part with RESERVE 4 keeps its four voices while another Part plays dense runs. That Part steals only from the other Parts' voices (oldest first), or from its own.
- A capped Part can't grow past its cap. In multitimbral use this keeps one Part from taking the whole pool, as stock's per-timbre voices do, without fixing the split.
- Reserves summing to 8 can starve an unreserved Part when every voice is guarded: its note is refused. That is what the player asked for, and the reason is on the cell (`RESERVES 8/8`).
- The mixer's PRT map node gains one sub-page.
- Flash, measured on drum-groups 33794f79 plus this branch, release builds. Bank 1: ORBIT +3516 B (888424 → 891940), default +3228, bank2-gate +3340, bench sets +2372 to +2748. That is the allocator's cap and guard, `Voicing`'s decode at every MODE read, the two specs and their arms, VOICES' page, row word and nav, and the UI stubs. Bank 2: +1344 B on ORBIT and +1536 on default and bank2-gate, for `ui::part_voices` and kit_groups' one-call `tries`. ORBIT's bank 1 then has 25 564 B under the 917 504 B SD-flash limit.
- AXI: `Voicing` keeps `PartParams` at 16 B, so the audio's snapshots don't grow. The allocator's `Limits` (18 B) sits in the `Instrument`, outside AXI.
- The machine outliner shared a repeated `refusal` call, and then `VoiceGroups::choke` (ADR 0280's code, reshuffled by this change), from bank 1, which broke the bank split (ADR 0175). `part_voices::ask_in` and `kit_groups::ask_in` now each call their rule from one site.

## Supersedes in part
- [0015](0015-voice-steal-and-fx-returns.md) rule 1 ("steal … from any part"): a guarded Part's voices are not stolen by another Part's note, and a Part at its cap steals only its own voices, even with voices free.
- [0027](0027-shedding-fades-tails-first.md) "Order": guarded voices are shed last.
- [0273](0273-a-block-near-its-deadline-sheds-a-voice.md) "The response": the voice the net sheds is the one `shed_one` picks with guarded voices last.
- [0280](0280-a-drum-kit-has-at-most-eight-voice-groups.md) "A pad with no live voice of its own … steals as rule 3 always has": at its Part's cap it steals within the Part.
- Extends [0151](0151-a-refused-kind-or-algorithm-is-shown-dimmed-with-its-reason.md)'s dimmed-with-reason refusal to MAX VOICES and RESERVE.

## Sources
- Issue private issue #966 and its owner comment (2026-11-15). Request: https://github.com/joegiralt/chimera-releases/issues/5 (dspmax).
- Stock PreenFM3 (GPLv3, reference only): `Synth.cpp:719-736, 776-790`, `Timbre.cpp:378-433` at 65cb963.
- ADR 0015, 0027, 0040, 0151, 0198, 0251, 0265, 0273, 0276, 0280.
- `chimera-engine/src/voice_limit.rs`, `chimera-engine/src/voice_alloc.rs` (`pick_where`, `shed_one`), `chimera-engine/src/part.rs`, `chimera-core/src/ui/part_voices.rs`, `chimera-core/src/ui/block_registry.rs` (`PART_VOICES`).
