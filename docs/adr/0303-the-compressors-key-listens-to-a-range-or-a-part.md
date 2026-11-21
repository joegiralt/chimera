# 0303. The compressor's KEY listens to a range or a Part

- **Status:** Proposed
- **Deciders:** owner (2026-11-20, private issue #1037, the design's four questions answered), firmware

## Context
The master compressor (ADR 0030) keyed itself from the whole mix: one detector on the sum of all three DAC pairs, one gain on every pair. A kick and a pad through it gave glue and never the pump a kick-keyed compressor gives, and the owner asked for a sidechain (#1037): "follow the Analog Rytm: the compressor works on ranges, not Parts", then the Parts after the ranges.

Precedent, read in the manuals (design: (private notes), Part 1):
- **Elektron Analog Rytm MKII** (manual OS 1.71, B.4): SEQ, the sidechain EQ, is OFF, LPF ("characteristic pumping"), HPF (avoids pumping) or HIT ("balanced"); four fixed curves on what the compressor analyses, never on the audio, with no corner published and no corner control. MIX 0–127 is parallel compression. RAT ends at MAX. A gain-reduction bar.
- **Elektron Digitakt** (OS 1.54, §13.4) and **Digitakt II** (§12.5, §9.8.1): SCS picks a track as the sidechain source (the manual's own example: the kick's track ducks the mix); SCF is a low-pass-to-high-pass filter on it; MIX is parallel. On the Digitakt II's COMPRESSOR ROUTING page a track left out of the compressor bypasses it and can still be its source: the kick keys the mix and passes unducked.
- **SSL bus compressor** (SSL Native v6.5 guide, p. 24): a sidechain high-pass, so low-frequency transients don't pump it; a dry/wet MIX; a gain-reduction meter.
- No unit publishes a corner. Every unit with a sidechain offers a ratio at or past 20:1.

What the code had (origin/0.4 `dac1fa97`):
- `MasterComp::process` and `mix_sided` are bank 1's, and bank 1 is frozen (ADR 0198). MST's six cells were all taken; MST › LVL's cell C was free. A page of its own would take a page id, and `FocusMemory` then overflows D2 on bench-algo (ADR 0296).
- `CompParams` is 32 B with two bytes of padding left after SMART EQ's (ADR 0296).
- When the master section runs, each Part's bus (after its EQ, before LEVEL and PAN), its side, its pair and its eased gains are all still in hand in `mix_sided`.
- MIX is `e = 1 + x·(g − 1)` on the same samples: an exact parallel blend already.

## Decision
1. **KEY: one cell, the ranges then the Parts.** `FULL · LOW · HIGH · HIT · PART 1 … PART 6`, the owner's order. A typed `comp_key::CompKey { Full, Low, High, Hit, Part(PartId) }` in `CompParams`' padding (the struct stays 32 B), with frozen, append-only codes 0–9 and disk idents `FULL`, `LOW`, `HIGH`, `HIT`, `PART1`…`PART6`. Compressor param 10, ident `KEY`, a choice that snaps, not modulatable (ADR 0010). A project saved before has none and loads FULL.
2. **FULL is the compressor as it was, bit for bit.** `MasterComp::process` is unchanged in what it computes: its loop now takes the detector's peak and what follows each chunk's gain from a `Listen`, and FULL's (`WholeMix`) is the old sum and nothing after. Under FULL bank 1 makes no door call.
3. **LOW, HIGH and HIT filter the detector only.** The detector's left and right (the pairs summed, as before) run through fixed sections before the chunk's peak; the audio is only ever scaled by the one linked gain.
   - LOW: a second-order Butterworth low-pass at 150 Hz (`LOW_CORNER`).
   - HIGH: the matching high-pass at 150 Hz (`HIGH_CORNER`).
   - HIT: ITU-R BS.1770-4's K-weighting, its shelf (+4 dB above about 1.7 kHz) then its high-pass (38 Hz), made from the analogue prototypes its printed 48 kHz coefficients come from, so they are those coefficients at 48 kHz (to 2e-6) and the same curve at any rate.
   - The corners are fixed, as the Rytm's are, and are Chimera's own starting points: no manual publishes one. They are tuned by ear later (a constant each).
   - A new key's sections start from silence; the reduction follows at ATK and REL, so a switch is never a step in the audio.
4. **PART n: the detector hears that Part's bus, after its EQ and SMART EQ and before its LEVEL, PAN and expression** (its mid; a DRUM Part's side is not heard). The fader never changes the pump, and a Part turned all the way down still pumps the rest.
5. **The key Part always passes unducked**, on whichever output pair it is on: after the gain, `(1 − e) · key` is added back on its pair, `e` the per-sample multiplier with MIX folded in and `key` the Part's own share of the pair (its bus and side at its eased gains, ramped as the mix ramps them). So `out = e·mix + (1 − e)·key`. The pass fades in and out over 10 ms (`PASS_FADE_S`); a Part gives way to another only once faded out. Its FX returns still duck with the mix, and MAKEUP lifts the rest but not it. No setting turns the pass off (the Digitakt II's routing page is the precedent; a second cell was not worth a cell).
6. **A silent key releases.** A key Part with no voices reads as no level: the reduction lets go at REL.
7. **RATIO gains MAX**, appended as index 8 (slope 1: everything over THRESH comes off), disk ident `MAX`. Indices 0–7 keep their ratios and idents, so every stored value keeps its meaning. No auto release in 0.4.
8. **MIX is unchanged**: it was already a true parallel blend.
9. **The page.** KEY is MST's cell E, beside the curve and the meter it moves. MAKEUP, a level, moves to MST › LVL's cell C: the same param (id 4, ident `MAKEUP`, NRPN 24/4), the same stored value, only its page slot moved. The GR meter stays, and under the curve the page names the KEY in use. While KEY's cell is touched MST's status line says what the choice does (`ui::notes::key`, bank 2's): `HEARS THE WHOLE MIX`, `THE KICK PUMPS THE MIX`, `HEARS ALL BUT THE BASS`, `EVERY HIT PUSHES IT ALIKE`, `DUCKS ALL BUT ITSELF`, each after the choice's name.
10. **Bank 2, behind a door of its own.** `comp_key::kern` (the sections, a Part's key and the pass, with the compressor's loop inlined into it) is an S4 group (`*8comp_key4kern*`), reached through `comp_key::door::KeyDoor`, whose table sits a stride after the stereo's (`COMP_KEY_DOOR_ADDR`); the boot's door check reads its mark. Bank 1 keeps the type, the spec and its arms, the names, the bill, the door stub (`FxBus::master_keyed`) and `mix_sided`'s five references (`comp_key::Tap`). **A closed door plays FULL**, and puts KEY's state at rest.
11. **KEY's state** (`comp_key::KeyState`, 96 B: two sections, their memory, the pass's weight and the key Part's last gains) is the compressor's, in AXI. Zero bytes are its rest. Nothing in ITCM or DTCM.
12. **Billed beside the bus**, as SMART EQ's wash is (`comp_key::bill::of`, added to `rev_extra`): nothing at FULL, so every bill is as before; LOW or HIGH 52 cycles a sample, HIT 72, PART n 56 (the counts are under Measurements). The bill follows the stored KEY whether or not the compressor is on or the door open: it over-bills then, never under.
13. **One loop for the keyed block.** FULL keeps its two loops (the second drops the engage fade's arithmetic once fully in). The keyed block runs the one with the fade for every block, to the same bits (a fade of 1 multiplies by 1.0): a few instructions a chunk for half of `kern::run`'s flash (8,892 B with both).

## Alternatives considered
- **A continuous sidechain filter** (the Digitakt's SCF, the SSL's HPF knob): a second cell the page does not have; the Rytm's fixed curves are the owner's pick. A later option if testers ask.
- **Source and filter as two cells** (the Digitakt's SCS and SCF): the owner's one-cell list, ranges first.
- **KEY on MST › LVL's free cell C, nothing moved:** KEY would sit away from the compressor and its meter.
- **A new MST › KEY page:** a page id, and bench-algo's D2 (ADR 0296).
- **The key after LEVEL:** turning the kick down would weaken the pump.
- **A choice of ducking the key Part too:** not worth a cell; the Digitakt II's routing is the precedent for always.
- **Auto release** (the Rytm's A1 and A2, the SSL's Auto): not in 0.4.
- **The detector's override and the add-back in bank 1** (precomputed peaks, kept gains): about 600–900 B of bank 1; the trait keeps it to the stub.
- **A per-pad key** (the kick alone out of a kit): out of scope; LOW is the answer for a kit on one Part.

## Consequences
- "Glue, never pumps" holds for FULL, HIGH and HIT at polite settings only: LOW and PART n are there to pump, and RATIO MAX with THRESH at the bottom is past good on purpose (knobs go past good).
- A kit on one Part keys as the whole kit under PART n.
- With `master-tape` (ADR 0055, never shipped) the tape colours pair 1 before the compressor, so a key Part on pair 1 is put back clean beside its taped self: the pass is exact only without the tape.
- The key Part's output pair moving mid-note steps, as the mix itself does there.
- The bill is charged while the compressor is off (RATIO 1:1, no MAKEUP) if KEY isn't FULL.
- Docs: `docs/chimera-sitemap.md` describes MST, KEY, MAKEUP's cell and when to reach for SMART EQ, REV › DUCK or KEY. MST and MST › LVL need new annotated screenshots for the tester guide, and KEY's hint one of its own (`mixer_master.png`, `mixer_master_level.png`, `mixer_master_key.png`).
- Sound work: the corners and the 10 ms fade are starting points for the owner's ear.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, against origin/0.4 `dac1fa97`. ORBIT, default, bank2-gate and fault-inject were built in both trees, and the base's sat exactly on their budget lines (both banks); the other nine sets' bases are their lines, which that commit set to its own measure. Each budget line is raised by exactly its set's change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---:|---:|
| orbit | +1,100 | +6,336 |
| default | +1,252 | +6,336 |
| bank2-gate | +1,276 | +6,336 |
| orbit,usb-config-writer | +1,084 |  |
| fault-inject | +1,220 |  |
| bench-algo | +1,116 |  |
| bench-algo,master-tape | +996 |  |
| bench-routing | +1,132 |  |
| bench-memory | +1,180 |  |
| bench-stress | +1,140 |  |
| bench-orbit | +1,332 |  |
| bench-tape | +1,060 |  |
| filter-probes | +1,228 |  |

ORBIT after: bank 1 907,084 B (10,420 B under the SD-flash limit; it was 905,984); bank 2 591,712 B used, all of the growth in S4 (186,592 B free there; S0-S3 unchanged at 7,072 B free).

Bank 1's growth on ORBIT (+1,100 B), by symbol: the audio's `block` +288 B (the master section's stub: KEY's check, the ask, the door call and the rest's clear; `mix_sided`'s tap; the bill), the page's draw +92 B (the key's name under the curve, the hint's door call), `KEY_NAMES` and `KEY_IDENTS` 80 B each, `COMP_SPECS` +72 B (KEY's spec), `CompParams`' arms +234 B (`write` +72, `enum_ident` +70, `get` +40, `set_enum_code` +30, `enum_code` +22), the boot's door check +12 B, `RATIO_NAMES` +8 B, and the names' and idents' text. Over the design's 600–900 B: the typed arms cost more than a bare code would, and typing is not traded for flash.

Bank 2 (+6,336 B, S4): `comp_key::kern::run` 5,952 B (the compressor's loop inlined once with the three detectors and the pass, the sections' coefficients, the pass's setup), `ui::notes`' KEY hints and words 380 B, the table 8 B. With FULL's second loop kept in the keyed block it was 8,892 B (decision 13).

CPU, counted by hand in the ORBIT ELF's `kern::run` (`tools/loop_count.py` cannot split a loop that holds three detectors; the disassembly is kept in the ledger), instructions a sample at 1.606 cycles each:

| | Detector | Past FULL's 22.75 | Pass | Loop and block | Added | Cycles | Billed |
|---|---:|---:|---:|---:|---:|---:|---:|
| FULL | 22.75 | 0 | 0 | 0 | 0 | 0 | 0 |
| LOW, HIGH | 38.5 | +15.75 | 0 | 6.25 | 22 | 35.3 | 52 |
| HIT | 50.5 | +27.75 | 0 | 6.25 | 34 | 54.6 | 72 |
| PART n | 5.75 | −17 | 37.5 | 6.25 | 26.75 | 43.0 | 56 |

"Loop and block" is what the one keyed loop runs past FULL's settled one: the engage fade's arithmetic and the detector's dispatch each chunk (13 instructions a chunk), and the state's copy, the pass's setup and the memory's check each block (about 190). The bills were set with room before the last count and are not trimmed to it: the count is by hand. Against the bus's 1,190 cycles a sample HIT is 4.6 % counted, 6 % billed. Not read on the unit.

FULL in bank 1: the same source, bit for bit (below). Its two loops in the audio's `block` are 219 and 226 instructions a chunk; they were 218 and 234.

RAM: `KeyState` 96 B in the compressor (AXI). `CompParams` stays 32 B.

Null: `comp_test`'s `full_hashes_as_it_did_before_key` holds two FNV hashes recorded on origin/0.4 `dac1fa97`, before the change: the compressor alone with every control moving (256 blocks of noise on three pairs), and the whole mix through `mix_parts` with the compressor on, after the limiter. Both stand, as do `golden_test` and the screen goldens but MST's and MST › LVL's. `comp_key_door_test` holds a closed door to FULL, bit for bit.

Tuning check, the teaser's trip hop and dub (private notes merged with this branch; the whole kit is Part 1) at the design's moderate setting, 6:1 over −20 dB, 3 ms in, 150 ms out, no MAKEUP, MIX 127. Gain reduction, dB, the median over the kicks; "between" is the 50 ms before the next kick:

| | Trip hop on a kick | between | Dub on a kick | between |
|---|---:|---:|---:|---:|
| FULL | 10.8 | 1.9 | 10.7 | 6.9 |
| LOW | 10.6 | 1.8 | 9.6 | 4.7 |
| PART 1 | 15.5 | 0.8 | 13.3 | 1.3 |

The trip hop's strings (the pad) between kicks, −36.4 dBFS on their bus at their LEVEL, leave the compressor at −41.0 (FULL), −39.8 (LOW) and −37.7 (PART 1); on a kick, from −38.5, at −48.8, −48.0 and −52.9. The kit itself: under FULL and LOW its kicks leave 10 dB down (−9.0 to −19.0 and −18.4); under PART 1 they pass at −9.0. In both pieces the kick is far above the rest, so FULL is already keyed by it and LOW changes little; the dub's sub (−17 dBFS, under 150 Hz) holds LOW 4.7 dB down between kicks, where PART 1 lets go to 1.3. PART 1 is the clear pump on both. The renders are in the ledger for the owner's ear; nothing was tuned from them.


## Sources
- Design: (private notes); issue private issue #1037 and the owner's answers of 2026-11-20.
- Elektron Analog Rytm MKII User Manual, OS 1.71, Appendix B.4; Digitakt User Manual, OS 1.54, §13.4; Digitakt II User Manual, §12.5 and §9.8.1; SSL Native v6.5 User Guide, Stereo Bus Compressor.
- ITU-R BS.1770-4, Annex 1 (the K-weighting pre-filter and its 48 kHz coefficients).
- Code: `chimera-engine/src/comp_key/` (`mod.rs`, `kern.rs`, `door.rs`), `dsp/comp.rs` (`Listen`, `WholeMix`, `process_keyed`), `dsp/fx_bus.rs` (`master_keyed`), `instrument.rs` (`mix_sided`'s tap, the bill), `chimera-core/src/ui/notes.rs` (`key`), `block_registry::MASTER`, `MASTER_LEVEL`.
- ADRs 0030 (the master section), 0045 (frozen idents), 0198 (bank 1 frozen), 0284, 0294 and 0296 (the S4 doors this follows).
