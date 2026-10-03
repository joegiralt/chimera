# 0078. ORBIT saves after the six Parts, and a load takes what it can

- **Status:** Proposed
- **Deciders:** implementer (plan Task 18), for the owner's UAT

## Context
ORBIT's scenes, drift and tempo are project data (spec § 8, ADR 0077), so
a project file must carry them, and `project_status` must see them: the
status is the CRC of `encode_project` (ADR 0066), and until this task no
ORBIT record was in it (plan ruling R9). The card format is ADR 0045's
TLV records under a CRC, written A/B (ADR 0046); record tags and disk
codes are frozen once written. Projects saved before ORBIT must still
load, and there are no users yet, so an old or damaged file must load
safely, not sound the same.

## Decision
- **Tags** (frozen): `Tempo` 0x000B (the projects spec's reserved
  tempo), `OrbitBlock` 0x0010, `OrbitNotes` 0x0011, `OrbitChord`
  0x0012, `Orbit` 0x0013, and the existing `OrbitScale` 0x0014. All
  non-critical. `RecordTag::is_orbit` names them; a Sound file or SYSTEM
  holding one is `Corrupt`.
- **0x000B is ORBIT's tempo for good.** The projects spec reserved it for
  a project tempo; it is now that tempo, owned by ORBIT: `is_orbit`
  holds it, so it is `Corrupt` anywhere before Part 6 and is read into
  `OrbitSong::bpm`. A later global tempo (LFO or delay sync) takes this
  one, ORBIT's, or a new tag; it can't sit at the top level before the
  Parts under 0x000B.
- **Where:** at the top level after Part 6. The first ORBIT record closes
  the last Part (`SoundCheck::finish`) and needs all six read, else
  `Corrupt`; after it, only ORBIT records and unknown non-critical ones
  may follow.
- **What, in order:** each slot whose `saved` bit is set (0–7), then
  `live` as scene 8; per scene, `OrbitBlock` for blocks 27–32, an
  `OrbitNotes` per ring (the hits mask, the pool, the 32 step notes),
  an `OrbitChord` per segment (all `MAX_SEGS`, degree and exact tones),
  one `OrbitScale`; then `Orbit` (the playing scene and the nine seeds)
  and `Tempo` (`f32`). A slot left out is neutral, which is exactly what
  the reader's base is, so the file is lossless, and NEW's ORBIT is
  1,563 B (slot 1 and `live`) of a 21,080 B file, not 6.9 KB.
- **A switch waiting is saved landed** (Review Focus 7): `live` is the
  queued slot and the playing scene its index (`OrbitSong::landing`). So
  the project's CRC does not move when the switch lands.
- **A load's base** (`OrbitSong::clear_for_load`, at pass 2's header):
  every slot and `live` neutral, 120 BPM, slot 1 playing, nothing
  queued. Each slot is reset in place and its rev moved, so no `Confirmed<SceneSlot>` or `SceneAsk` from before the load holds
  after it. `finish` sets the cached `saved` mask from the slots read.
- **A load takes what it can.** An ORBIT record of the wrong shape, or
  naming a scene past 8 or a ring past 5, is skipped; a value that can't
  be one keeps the base's: a pool through `Pool::new`, a mask through
  `HitMask::new` then `Ring::with_mask` (the fill's own hits moved), a
  step note through `Note::new`, a degree through its `DiskCode`, a
  played chord missing its bass or under two tones as none, the tempo
  rounded and clamped (NaN 120). Pass 1 has nothing ORBIT can fail, so
  ORBIT never costs a project its Parts. A skipped record leaves only its
  own part of the scene at the base; the rest loads, so a damaged scene
  is valid but mixed (an `OrbitBlock` skipped leaves the fill 1/1, so the
  ring's `OrbitNotes` mask is refused while its pool and steps are
  taken).
- **A lossy load is never Saved** (controller ruling, Task 18 review).
  `orbit::records::read` says whether it took a record whole: the song
  then writes that very record back, byte for byte. A record skipped, a
  value defaulted or clamped, or bits dropped (a chord's tones past B, a
  set's past the 12) is lossy, and so is a file with no ORBIT records at
  all (projects-core v1), which a save would grow. `ProjectCheck`
  latches it in pass 2, and the load leaves `saved_crc` empty, so the
  project reads Modified and offers the save that writes it whole. A
  file this firmware wrote loads Saved.
- **Decode order is free.** SCALE is a coded param, decoded through
  `set_enum_code`, which never seeds the CUSTOM set (that is
  `ChordRing::write`'s, a UI edit's), so `OrbitScale` lands the file's
  set before or after the block.
- **Run-time revs are no part of a value.** `SceneRing`'s `rhythm_rev`
  and `notes_rev` (the runner's handshake) and a `Pool`'s slots past its
  length no longer count in `==`, as `slot_revs` already didn't in
  `OrbitSong`'s: a scene loaded is the scene saved. The revs a load
  leaves are whatever its writes moved them to; `notes_rev` also seeds
  CLEAR down's roll, so the first roll after a load differs from the
  first one before the save. Harmless: a roll is new notes either way.
- The transport is never saved: a load bumps the epoch, which stops the
  runner with no edge (ADR 0071), and it adopts the file's playing scene
  and `live` whole.

## Alternatives considered
- **Every slot written, neutral or not** (the plan's text): NEW's file
  25 % larger (5.3 KB more), and as much more to hash on every status
  check, for no information.
- **A malformed ORBIT record `Corrupt`**: the whole project refused, its
  Parts with it, over the sequencer's settings.
- **The whole song back to neutral on any bad record**: the passes reset
  their state at the header, so pass 1 would have to smuggle a flag into
  pass 2; and the rest of a damaged song is still the user's, worth
  keeping (the load reads Modified instead).
- **Lenient loads reading Saved**, like a Sound's unknown enum codes:
  SAVED would claim the card holds what the unit plays when it doesn't.
- **Zeroing the revs after a load and keeping them in `==`**: a scene
  edited and saved would never equal itself loaded.

## Consequences
- Old projects (projects-core v1) load with every scene neutral and every
  ring muted, at 120 BPM, and read Modified until saved.
- A project grows by ~0.76 KB per saved scene plus `live`, and 49 B for
  the seeds and tempo; all eight slots saved is 6,862 B.
- `project_status` sees every ORBIT edit and absorbed drift; NEW's
  `TemplateCrc` covers NEW's ORBIT records, so a fresh project is still
  Pristine.
- The codec has no `cfg`: a firmware without the `orbit` feature still
  loads and re-saves ORBIT's records.

## Sources
- Spec `docs/superpowers/specs/2026-09-30-orbit-design.md` § 8; plan
  `docs/superpowers/plans/2026-09-30-orbit.md` Task 18 and Decisions.
- ADR 0045, 0046, 0066, 0071, 0077.
- Tests: `chimera-core/tests/orbit_codec_test.rs`; the frozen fixture
  `chimera-core/tests/fixtures/v1/orbit.prj` pins every ORBIT payload's
  layout (ADR 0045 § Fixtures).
