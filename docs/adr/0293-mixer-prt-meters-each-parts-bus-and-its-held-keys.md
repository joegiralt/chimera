# 0293. MIXER › PRT meters each Part's bus and marks its held keys

- **Status:** Accepted (2026-11-19)
- **Deciders:** owner (2026-11-19, tester request chimera-releases#15, the design's recommendation A), firmware

## Context
A tester sequences six Parts from Bitwig. Without leaving the mixer they need to see which Part received a note and which is making sound, and to tell a silent patch from a routing problem. They asked, first, for a live audio bar beside each LEVEL bar, before the shared FX, and a separate note mark (design: (private notes), #15).

What the code had (origin/0.4 b136ef37):
- PRT's viz band (`viz::parts_overview`) draws each Part's LEVEL bar from the set value and its pan dot.
- The header's six dots ring a Part with a sounding voice (ADR 0150): coarse, not a level.
- `meter::ClipMeter`: a per-Part atomic max the audio raises each block and the UI takes each frame (FILTER_CLIP). A peak meter is the same pattern.
- `played::HELD`: the audio already publishes the held keys per MIDI channel, for the screensaver.
- Bank 1 is frozen (ADR 0198); new 0.4 code goes to bank 2's S4 groups (ADR 0272), the door's own table being full (ADR 0283).

## Decision
1. **A per-Part peak, after the Part's EQ and before its LEVEL, PAN and sends.** My call, the design's recommendation: pre-LEVEL answers "is the patch making sound" whatever the fader, and the LEVEL bar beside it already shows the fader. Each block, for each Part whose bus was written, the most of `|mid|` (or `|mid| + |side|`, the louder of L and R, where a DRUM pad or a panned voice wrote a side) is raised into `part_peaks::PART_PEAKS[p]`, a `PeakMeter` (`ClipMeter`'s pattern: `fetch_max` on a `Peak`'s bits, which order as its values; `Peak` never negative, −0 or NaN, an infinity its largest finite value).
2. **Measured only while the meters are up.** The UI says each frame whether MIXER › PRT is shown (`part_peaks::watch`); the audio's stub runs the pass only while it is (`watched`), after `part_eq`; a stage that writes a Part's bus or side later goes before it.
3. **The pass is bank 2's, behind its own table.** `part_peaks::kern::run` is an S4 group (`*10part_peaks4kern*`), reached through `part_peaks::door::PeakDoor` (magic 0xB2E0_0293, one entry) at `PART_PEAKS_DOOR_ADDR` = 0x08180420, one stride after the EQ's (`s4_tables.rs`, `bank2.x`'s placement and assert, the boot's door check `door_ok`). A closed door measures nothing: the meters rest.
4. **The meter.** Beside each LEVEL bar (8 px), 2 px on, a 3 px bar (`part_meter::bar_x`), its foot −48 dBFS, its top 0 dBFS (`Dbfs`, `Dbfs::px` over the strip's 38 px). It rises at once and falls 20 dB a second; its peak is a 1 px INK mark held a second, then falling as the bar does, never under it. A frame whose reading passes full scale lights the bar's top 2 px in ALERT (a Part's bus may pass 0 dBFS before its LEVEL). The meters reset to rest whenever the page is left, so it opens fresh.
5. **The key mark is held keys, not a flash.** My call, the design's: a 3 px ACCENT square right of the Part's number while a key is held on the Part's MIDI channel, or on a member channel of the MPE zone whose master that is (`part_meter::held_on`, through `Zones::master_of`; `played::channel_held`). It reads `played::HELD`, so it adds no audio work. A flash on each note-on (from `last_in`) is left out: at a DAW's dense notes a held mark already blinks with the gate, and a flash would need its own timer per Part. Notes ORBIT plays aren't live notes and don't mark (`played` publishes live ones only).
6. **Mixer only.** The header's dots (ADR 0150) are unchanged: the design's third question is left for later.
7. **The UI is bank 2's.** `ui::part_meter` (ballistics, the key marks, the paint and the band's key) is an S4 group (`*12chimera_core2ui10part_meter*`), reached through one new `UiDoor` entry, `meters` (`Ask::Frame`, `Paint`). Each frame also keeps the band's redraw key in the state (`PartMeters::key`: every bar's and peak's pixels, the overs and the marks), so bank 1 folds it into the band's key with a field read, no door call. Bank 1 keeps the state's home (`UiState::meters`), the frame's stub and the band's paint call after `parts_overview`. A closed door draws no meters.

## Alternatives considered
- **A voice-level meter (the design's B):** each voice's VCA level, max per Part. Cheaper, but it shows the VCA's gain, not the sound: a dark filter reads loud, and it misses the silent patch the tester asked to find.
- **A PERFORM page (C):** six vertical meters, mute and solo. New state and its own design; it overlaps ADR 0281's PRT › VOICES. Left for after 0.4.
- **Post-LEVEL:** what reaches the mix; but a Part pulled down reads silent, the case the tester wanted told apart.
- **Metering always:** about 420 cycles a block for a page that is rarely up.

## Consequences
- CPU: only while MIXER › PRT is up, six Parts' abs-max over 64 samples. Host, release, best of five runs of 20,000 blocks, six Parts each holding an ALGO note: the instrument's block 21,372 ns unwatched, 21,775 ns watched (+1.9 %; the pass is sized, not unrolled). The design estimated about 70 cycles a Part on the unit (0.07 % of a block); not read on the unit. None while the page is down.
- RAM: 24 B of atomics and a flag; the UI's `PartMeters` (6 × 20 B and its key) with `UiState`: AXI on the shipping builds, D2 on bench-algo's, whose bench text gives 64 B for it.
- The band redraws while a meter moves (its key carries every bar's pixels).
- Docs: `docs/chimera-sitemap.md`'s PRT text describes the meters and the key mark. MIXER › PRT needs a new annotated screenshot for the tester guide (meter, peak mark, over, key mark).

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, against ADR 0292's commit, each set built in both trees. Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| orbit | +304 | +1,120 |
| default | +312 | +1,120 |
| bank2-gate | +392 | +1,120 |
| orbit,usb-config-writer | +256 |  |
| fault-inject | +216 |  |
| bench-algo | +232 |  |
| bench-algo,master-tape | +240 |  |
| bench-routing | +256 |  |
| bench-memory | +264 |  |
| bench-stress | +232 |  |
| bench-orbit | +568 |  |
| bench-tape | +616 |  |
| filter-probes | +224 |  |

Bank 1's growth is the audio's stub (the watch test and the door call), `UiState::update`'s frame stub, the band's paint call and the meters' state's key read; on bench-orbit and bench-tape the UI's code links in bank 1 with no UI door. The peak pass is sized (`#[optimize(size)]`, 296 B: unrolled it was 1.5 KB); the dB is `log2f`, bank 1's already, not `log10f`. bench-algo's D2 had no room for `UiState`'s 128 B more: its bench text gives 64 B more (`BENCH_TEXT_LEN` 5,504 → 5,440). ORBIT bank 1 has 17,952 B free after it.

## Sources
- Design: (private notes) § #15.
- Tester request: chimera-releases#15.
- Code: `chimera-engine/src/part_peaks/`, `chimera-core/src/ui/part_meter.rs`, `instrument::Instrument::render`'s Part stage, `meter::ClipMeter` (the pattern), `played.rs`.
- Octatrack MKII manual, MIXER page (live input meters beside the level controls).
