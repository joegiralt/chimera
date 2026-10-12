# Showcase factory library (alpha): Design

- **Status:** Approved 2026-10-11
- **Date:** 2026-10-11
- **Issues:** #317 (factory library, FACTORY tab), #302 (importer, TX81Z slice), #243 (formant operators, placeholders), #476 (TAPE coming soon)
- **Builds on:** ADR 0119 and `2026-10-09-sound-library-design.md` (the card library); `2026-10-04-patch-importer-design.md` and ADR 0088 (Proposed, branch `importer-spec`: the TX81Z parser, map and guards); ADR 0109 (−11 LUFS reference); `tools/factory-tapes/recipes.toml` (the text recipe model)
- **Read against:** `origin/main` d5e59a86; facts from (private notes)

## Owner decisions on the open questions (2026-10-11)

1. **The TX81Z ports stay private until #410 (counsel) is answered.** They are built and kept locally, outside the repo and outside the public card set. The public alpha set is the non-port Sounds (9 VA, 2 FM vocal, 4 MODAL: **15 Sounds**) plus the 4 demo projects. The importer and the private pipeline are still built now, so the 16 ports are ready the day #410 clears.
2. **Bank origin:** the Yamaha TX81Z factory banks, the voices that come with the instrument ("all the TX81Z patches that come with it"). No third-party bank is used.
3. **Demo project ids:** `P0000001`–`P0000009` are reserved for demos, and a project save allocates from `P0000010` up (§ 5).
4. **Tester install:** over USB with the console tool (`tools/chimera-usb.py`), or by copying the files onto the card in the stock bootloader's SD-card-access mode (tested working 2026-10-11). The release zip mirrors the card's layout so the copy is a drag and drop (§ 3).
5. **VA split:** 2 Boards-of-Canada pads, plus 1 Sound that is a pad played softly and brass played hard (NEON BRASS).

## Owner decisions (approved in chat, 2026-10-11)

1. **A showcase factory library for the alpha.** It shows off the engines, sounds musical and organic, and is never samey. Each Sound has its own character and one sentence saying what it demonstrates.
2. **All of it lives on the SD card, at zero flash cost.** About 26 KB of flash is free; NEW's 8 built-in ALGO Sounds stay as they are.
3. **No new DSP** (alpha sound freeze). TAPE is COMING SOON, so the showcase is VA, ALGO (FM), MODAL and effects only.
4. **The set:** 9 VA, 2 FM vocal Sounds, formant placeholders (#243), 4 MODAL, and 4 effects demo projects in public; 16 modernised TX81Z ports kept private until #410 (answer 1 above).
5. **Every Sound is alive through designed modulation**: envelopes, LFOs, velocity, NOTE, STABILITY. Short sounds get a low-pass-gate action (factory-library method).
6. **Claude does QA** (levels, clicks, aliasing); then the owner listens on a page of renders; only then does anything go on the unit (QA before UAT).

## 1. Descriptions

**The record.** `Note`, tag `0x001D`, not critical (bit 15 clear). Its payload is 1–40 bytes of ASCII text, with no terminator and no padding.

| Rule | Value |
|---|---|
| Position | Third, straight after `Build` (`Engine`, `Build`, `Note`, then the rest), so a listing's peek reaches it |
| Length | 40 B at most, **and** it must fit `CONTENT_W` (216 px) in `FONT_LABEL` at `LABEL_TRACKING`, about 34 capitals. The tool checks this with the firmware's own `draw::text_width`; the firmware clips with `clip` anyway |
| Character set | `A`–`Z`, `0`–`9`, space, and `. , ' - + / : ( ) & %`. The tool refuses anything else; the firmware draws any byte outside 0x20–0x7E as `?`, as `Build` does |
| Default | None: no record is written. Every existing Sound reads as "no description" |
| Decoding | Known to this build and skipped in pass 2, never counted as skipped. A `Note` that is not third, a second `Note`, or one past 40 B is ignored. A `Note` before `Engine` is `Corrupt` (today's rule for any record before `Engine`) |
| Older builds | Skip it as an unknown non-critical record (`unknown_non_critical_skipped`), so they load the Sound and say `LOADED · SOME PARAMS SKIPPED` |
| Projects | Never written in a project. Skipped in every project context, as `Build` is |

**The description stays in the file.** It is read by the listing's peek and drawn in the browser. It is not decoded into `Sound`, the pool or the Part. So `Sound` keeps its shape, the pool arena pays nothing, and a factory Sound loaded and then saved to LIBRARY is the player's own Sound, with no description. A later build can carry it into the pool if players ask for that.

**The peek** grows from 64 B to 128 B: header 28 + `Engine` 5 + `Build` 25 + `Note` 4 + 40 = 102 B. One sector is read either way.

**Drawing.** In the FACTORY tab, line 2 (`SAVE_Y`, free there because nothing saves) shows the selected row's description in `INK`. It is information, so it draws with HINTS off too. The theme's tokens carry it through INVERT (ADR 0126); no new colour. LIBRARY doesn't draw descriptions in alpha: the unit can't write one, so a user file never has one.

**Cost.** Card Sounds: 0 B of flash for the text. Code: the record, the peek and one line of drawing, estimated under 0.5 KB. RAM: 40 B more per listed entry, 48 entries, about 1.9 KB of AXI. Each is reported in a line (flash rule).

## 2. The FACTORY tab (#317)

**Ids.** `S9000001`–`S9099999` stay the TAPE set's (tapes-v1 already uses `S9000001`–`S9000032`). The showcase takes `S9100001`–`S9199999`. Each Source states its id, so ids never shift when Sounds are added. Types: `LibId` splits into `UserId` and `FactoryId`. The library writer (`save_library_sound`, `save_ab` onto a library pair) takes `UserId` only, so a save can't name a factory file; that is a compile-time guard. Only the console's `put` writes factory ids (the install), and `rm` already refuses them (`ERR rm: factory`).

**Tabs.** Encoder B turns PROJECT → LIBRARY → FACTORY, as it turns between two tabs today. The tab is kept for the session, and is PROJECT at boot. Title row: `PROJECT  LIBRARY  FACTORY  B ◂▸`, then `· P1` at the right. That is about 200 px against the 216 px available, in full words; the text-width test pins it. If it doesn't fit, `B ◂▸` moves to hint line 1, and the tab names are not abbreviated.

**Rows.** Every `S9…` file on the card, **lowest id first**, which is the curated order (VA, FM, vocal, MODAL). 48 to a page, with `‹ PREVIOUS` and `NEXT ›` paging rows (alpha's set is one page). A row shows the name, its estimated cost and the engine tag, exactly as a LIBRARY row does; greyed rows and error rows follow LIBRARY's table. A row whose engine is COMING SOON (a stray TAPE factory file) is greyed, and EDIT says `COMING SOON`.

| Line | y | FACTORY tab |
|---|---|---|
| 1 | `HINT_Y` | `EDIT LOADS · +0.3K`, or `WON'T FIT` in `ALERT` (a hint) |
| 2 | `SAVE_Y` | The description, in `INK` (information) |
| 3 | `STAMP_Y` | `FACTORY · 0.1.0 1A2B3C4D`: the set's build stamp (information) |
| — | `METER_Y` | The pool meter with the row's preview, as in LIBRARY |

- **Empty:** one dimmed row, `NO FACTORY SOUNDS`. **No card:** the `StoreError` row, as in LIBRARY.
- **Keys (D-pad rule, ADR 0081):** A scrolls; EDIT loads; SEQ goes up to the Sound rung; MINUS and PLUS move between Parts; MENU leaves.
  - MIX + EDIT and MIX + MINUS do nothing in FACTORY, so there are no hints for them.
- **Loading** is the LIBRARY load unchanged: the same guard, `LibraryPart` target, dedupe by `sound_crc`, budget, `adopt` and toasts. `LibFile` holds a `FactoryId` or a `UserId`.
- **Listing:** one directory pass keeps the 48 lowest factory ids above the page's floor, then peeks those. LIBRARY and FACTORY share one page static, keyed by tab and `VolumeId`, so turning between them re-lists (about 0.4 s for 36 files, behind BUSY). A second static would cost about 4.4 KB of AXI to save that wait.

## 3. Sources and the `factory-sounds` tool

**Where.** Public Sources are committed in `tools/factory-sounds/`: `va.toml`, `fm.toml` (the vocal Sounds), `modal.toml` and `projects.toml`. **Port Sources are never in the repo** (§ 6): they live in `$CHIMERA_TX_PORTS`, default (private notes) (durable, under `~/dev`, and ignored by git through `.superpowers/`). They are read by `listen::tables`, the parser `recipes.toml` uses, extended to accept dotted keys (valid TOML). Only `key = "value"` lines, `[[table]]` headers and comments are allowed, so a file stays one line per setting, readable and diffable.

```toml
[[sound]]
id = "S9100006"
name = "GLIDE BASS"
engine = "VA"
note = "MONO PART: 80 MS GLIDE, SUB BENEATH"
demo = "mono"                     # the Part MODE for renders and demo projects
plays = "bass texture glide"      # render scripts (§ 4)
home = "C2"                       # level and aliasing note
phrase = "bass"                   # the listening phrase (§ 4): bass, lead, pad, arp or wonky
VA.WAVE1 = "0.667"                # SAW
VA.SUB = "0.45"
PITCH.STEAL = "GLIDE"
PITCH.GLIDE_TIME = "0.080"        # 80 ms
FILTER.KIND = "2040"
mod.ENV_2 = "FILTER.FREQ +48"
mod.VELOCITY = "FILTER.FREQ +20, AMP.FOLD +12"
trim = "-1.0"                     # dB on the output level, written by `fmt --level`
```

- **Keys** are `<BLOCK>.<PARAM>` by the frozen idents in `chimera-core/tests/fixtures/disk_codes_v1.txt`. Values are stored values or enum idents. A param left out keeps `Sound::neutral`'s value: sparse, as the encoder is.
- **`mod.<SOURCE>`** lists that matrix row's routes as `DEST ±amount` (amount −127…127). **`tap.OP5`** and **`tap.OP6`** list ALGO's audio-rate filter routes (FREQ, Q, VCA, FOLD). An unroutable destination fails the build with its name.
- **Provenance** (ports only, so private Sources only): `from = "tx81z <sha256 of the bank, first 8 hex digits> #<voice>"` (never the Yamaha voice name), plus `edits = "filter: …; lfo: …"`, naming at least one of filter, fold, LFO, envelope or effects.
- **`status = "placeholder"`** reserves an id and name. The build skips it.
- **`fmt`** rewrites each value line with a trailing comment showing its display value (`# 80 ms`). Comments are ignored on read, so the canonical text round-trips exactly.

**The tool.** A `factory-sounds` bin in `chimera-desktop`, beside `factory-tapes`. Both share `listen` (tables, `write_wav`, `lufs`, `qa`, `page.html`). It renders through the real `Instrument` headlessly, as `loudness_test` does. `chimera-utils` stays the reference A/B tool; it gains one subcommand, `stim <dir>`, which writes its listening set as WAVs for § 4's effects renders.

| `just …` | Does |
|---|---|
| `factory-sounds` | Parses, checks (unique ids and names, id ranges, description width and characters, routes) and builds the public set into `target/factory-sounds/public/CHIMERA/{SOUNDS,PROJECTS}/` (`S91xxxxx.A`, `P000000n.A`) plus `manifest.json` (id, name, CRC, size). **Refuses any Source with a `from` line** |
| `factory-sounds --private` | Also reads `$CHIMERA_TX_PORTS/*.toml` (each must have `from` and `edits`), and builds the ports into `target/factory-sounds/private/CHIMERA/SOUNDS/`. Never into `public/` |
| `factory-sounds fmt [--level] [--private]` | Canonical text. `--level` writes each `trim` (§ 4) |
| `factory-sounds qa [--wav] [--private]` | Renders every Sound and demo project, measures, writes `report.txt`, and writes `listening.html` with every Sound that passes (a failing one is named in `report.txt` and kept off) and the placeholders, all into the ledger folder (§ 4) |
| `put-factory-sounds [dir]` | `chimera-usb.py put --over` for each file under `dir/CHIMERA/` (default the public build): Sounds into `/CHIMERA/SOUNDS/`, demo projects into `/CHIMERA/PROJECTS/`. The owner's unit takes `private` too, for UAT |

Stamp: `Build` carries the version and the commit the Sources were built from; the golden test passes a fixed stamp. The release gets `factory-v1.zip`: the **public** build only, laid out as `CHIMERA/SOUNDS/…` and `CHIMERA/PROJECTS/…`, plus the manifest. A tester installs it either with `put-factory-sounds` over USB, or by copying the `CHIMERA` folder onto the card in the stock bootloader's SD-card-access mode. `just release` refuses a zip holding any id in the port range `S9100010`–`S9100025`.

## 4. QA before UAT

**Render scripts** (the A/B listening stimuli, played as notes; velocity 100 unless noted; 48 kHz; FX off unless the Source's sends are set):

| Script | Notes |
|---|---|
| `bass` | The home note (C2 for basses), held 0.3 s, then a 1.5 s tail |
| `stab` | C3 E3 G3 B3, held 0.25 s |
| `short` | C4, held 80 ms (attack and clicks) |
| `long` | The home note, held 2 s, then released for 2 s (motion, drift, loudness) |
| `texture` | 16 sixteenth notes at 120 BPM over two octaves, at velocities 40 to 127 (velocity response) |
| `glide` | Overlapping C2, G2, C3, played on a MONO Part (the glide Sounds) |

**Phrases** (owner, round 1: a render under a second does not show a Sound). Each Sound is auditioned first over a four-bar phrase at 120 BPM, picked by its `phrase` key; a VA Source without one fails the build, and ALGO and MODAL may use any or none. The phrases are original, written in the manner of famous basslines and leads (rhythm and contour only, never transcribed), as pure note tables in `qa` (pitch from the home note, start and length in sixteenths, velocity):

| Phrase | Notes |
|---|---|
| `bass` | A bouncy syncopated bassline with octave jumps, notes held into the next (glides on a MONO Part) and velocities 68 to 124 |
| `lead` | A melodic line of long and short notes, legato slides, and a held note of 1.6 s for vibrato and filter motion |
| `pad` | Four held chords, one a bar |
| `arp` | A syncopated arpeggio grouped in threes across the bar, pushed onto the off-sixteenths and tied over the beat; short notes, no overlaps, velocities 70 to 120 |
| `wonky` | A wonky melodic bassline with wide leaps, accents off the beat (velocities 68 to 124) and five notes held into the next (slides on a MONO Part) |

The phrase plays on the Sound's demo mode, with its sends, as a stereo WAV. It is a listening render, not a gate: its limiter reading is shown and flagged over the 3 dB chord bar for `pad` and the 1 dB bar otherwise. A phrase that is not finite fails the run. The scripts above stay the gates.

Effects demos: each project's Parts play `stab` and `short` through every reverb TYPE (QUAD, ROOM, HALL, BLOOM, GATE, REVERSE), CHORUS I, II and I+II, and the delay. Then `chimera-utils stim`'s hats and snare go through the same FX bus.

**Gates**, each printed per Sound in `report.txt`:

| Check | Pass |
|---|---|
| Loudness | `loudness_test`'s protocol: the home note at v100, held 1 s, P1. The result must be −11 LUFS ± 2 LU (ADR 0109's reference). `fmt --level` sets `trim` to centre it. A Sound kept off-centre by ear says `level = "by ear: <reason>"` |
| Limiter | A single note: at most 1 dB of gain reduction. `stab`: at most 3 dB |
| Clicks | `listen::qa`'s click ratio at every note-on, note-off and render edge. DC under −60 dBFS. Finite output |
| Aliasing | For harmonic Sounds: `long` at C6 and C7, with inharmonic energy at least 40 dB under the harmonic energy. The Source's `harmonic = "yes" \| "no"` decides; it defaults to yes for VA and no for ALGO and MODAL, so integer-ratio ALGO (the FM vocal Sounds, ports with integer ratios) opts in. A Sound marked no (bells, MODAL) is exempt and marked for listening |
| Tails | Silent (−90 dBFS) within the release plus 2 s, or 10 s for MODAL, which keeps ringing after note-off |
| Tap | The vocal Sounds: the render with the tap differs from one without it by at least 6 dB of spectral error, so the op 5 or op 6 route is audible |
| Samey | No two Sounds' `long` renders within 3 dB on all 32 ERB bands (the importer spec's spectral metric). A pair that fails is reported for a by-ear verdict |

**The owner's page.** Renders, `report.txt` and `listening.html` go in (private notes) (`$CHIMERA_SHOWCASE_LEDGER` overrides it), never `/tmp` or the scratchpad (durable ledgers rule). `listening.html` lists each Sound with its name, description, demo mode, players for each render, LUFS, peak and the `edits` line. Each Sound carries a badge: APPROVED (its Source has `approved = "round 2"`, the owner's listening round), NEW (every other buildable Sound) or PLACEHOLDER (listed by name, no renders). Under each, a "what it is" block generated from the built Sound (`factory_sounds::about`): ENGINE, the engine's key settings, FILTER, and every route under MODULATED, values as the unit shows them. Claude may publish it as a private artifact. The owner's verdicts go in `listening.md` in the same folder. Private ports render into the same page, in their own section marked PRIVATE. Only Sounds the owner keeps are installed on the unit for UAT.

## 5. Effects demo projects

The effects belong to the performance (`FxParams`, one set per project), so the effects demos are projects. A project file is the ordinary `P#######.A` (`encode_project`): the `Fx` record, the pool's slots holding the Sounds it uses, and the Parts. It is built by the tool from `projects.toml` on NEW's template.

```toml
[[project]]
id = "P0000001"
name = "REVERB ROOMS"
fx.REVERB.TYPE = "ROOM"
part.1 = "NYLON, POLY, CH 1"
part.2 = "GAMELAN, POLY, CH 2"
```

| Id | Name | FX | Parts (MIDI channels 1–4) | What to try |
|---|---|---|---|---|
| P0000001 | REVERB ROOMS | REVERB ROOM | NYLON, GAMELAN, FOLD BASS | Turn TYPE through QUAD, ROOM and HALL |
| P0000002 | REVERB BLOOM | REVERB BLOOM, long | FADED HYMN, CELLO SWELL | Turn TYPE to REVERSE |
| P0000003 | REVERB GATE | REVERB GATE | GROWL BASS, RING LEAD, VOWEL BUZZ | Gated percussive tails |
| P0000004 | CHORUS DELAY | CHORUS I+II, delay 3/16 | TAPE DRIFT, GLIDE BASS (MONO), FIFTH SLIDE (MONO) | CHORUS I, II and I+II; delay feedback |

- **Public Sounds only.** A demo project never holds a port, so the demos ship in the public set (the build refuses a `part.n` naming a private Sound). They may be recast with ports after #410.
- **Players find them** at the top of SETTINGS › LOAD PROJECT, where they list first. Their names are their content. The release notes give the "What to try" column.
- **Ids.** `P0000001`–`P0000009` are reserved. `ProjectId`'s save allocation becomes `max(top, 9) + 1` (one line), so a new user project never takes a demo id. A player may save over a demo (it is an ordinary project); `put-factory-sounds` restores it.
- **Sends.** With the `sound-sends` branch (sends on the Sound's OUT block), a Source sets its own sends, and a Sound loaded from FACTORY arrives with them. Without it, the demo project's Part sends carry them. Either way the project holds the FX settings.

## 6. The TX81Z importer, local only (#302)

**Scope:** the TX81Z slice of the importer spec. That is § 5.2's VMEM/VCED/ACED parser (`no_std`, in `chimera-engine/src/import/`, where the crate split puts the spec's `chimera-core/src/import/`) and § 6's TX81Z rows (map in `chimera-desktop/src/import/`), without DX7, PreenFM3, or the ymfm A/B (the ports are mutated, and the owner judges them by ear). The one change from that spec: it **emits text Sources**, not card files.

`just import-tx <bank.syx> [--voices 1,4,9-12] [--out DIR]` writes one `<voice>.toml` per voice and an import report (the importer spec's loss lines). `DIR` defaults to `$CHIMERA_TX_PORTS`. The CLI refuses any output path inside a git work tree unless `git check-ignore` says it is ignored (so (private notes) is allowed, `tools/` is not).

**The map** (ADR 0088's TX81Z rows): ALG n → T n on ops 1–4, with ops 5 and 6 at LEVEL 0; CRS/FINE → CRSE/FINE through `dsp/algo/tx.rs`'s `COARSE`/`FINE_TOP` indices (`ratio`); DET → DETUN; OUT → LEVEL; AR, D1R, D1L, D2R, RR, RS 1:1; KVS → VEL; FBL → FDBK on op 4; OSW → W1–W8; TRPS → TRNSP; LFO wave, speed, PMD × PMS and AMD × AMS/AME → LFO 1 with routes. The filter is open, and ENV 1, ENV 3, LFO 2 and LFO 3 are left free for the mutation.

**What can't be mapped** (each logged in the report):
- fixed frequency: becomes the nearest ratio at a per-voice reference key;
- EG shift: dropped; the operator decays fully;
- REV (reverb rate): dropped;
- LFO delay: dropped, because LFO DEPTH is not a matrix destination;
- key level scaling: approximated by a NOTE → LEVEL route, at most 7.4 dB/oct;
- amplitude modulation: approximated; ours is bipolar;
- FINE: within about 14 c of the TX81Z's steps;
- mod wheel, breath, foot controller, aftertouch, pitch-bend range, portamento and poly/mono: dropped, because Chimera has no such sources and mono and glide belong to the Part.

**What may be committed** (ADR 0114 § 6, ADR 0032, the factory-library method, importer spec § 9, owner decision 1):
- **Never:** bank files, verbatim conversions, Yamaha voice names, **and port Sources, mutated or not, until #410 clears.** `.gitignore` gains `*.syx`, `*.SYX`, `*.bnk`, `*.mix`, `*.wav` and `/tools/factory-sounds/private/` (a local fallback folder for anyone who keeps ports in the tree).
- **The guard:** a test, `no_private_ports_in_index`, runs `git ls-files -z` (tracked **and staged** files) and fails on any tracked bank file, any path under `tools/factory-sounds/private/`, and any `.toml` file with a `from = "tx81z` line (only `.toml`, so docs and tests that quote the rule don't trip it). The fuzz seeds under `chimera-desktop/fuzz/seeds/` are the one allowed `*.wav` path. It runs in `just test`, so `just check` (required before every commit, ADR 0013) fails the moment a port Source is staged. The public build's refusal of `from` (§ 3) is the second wall; the release's id check is the third.
- **Edits verified:** `factory-sounds --private` re-imports each voice from its bank (`$CHIMERA_TX_BANKS`) and fails if a port changes nothing in the categories its `edits` names.
- **Rights:** our edits sit on top of voice parameter data that isn't ours. ADR 0114's rule allows facts from a manual (the ratio tables), and ADR 0032 bars GPL code; neither rules on patch data, which is why the ports wait for #410. When #410 clears, a new ADR superseding 0134's port rule moves the ports into the repo and the public set; their ids are already reserved.

## 7. The patch list

The ids run in browse order. Names are at most 12 characters. Descriptions are drafts: the tool's width check (§ 1) is final.

**VA, `S9100001`–`S9100009`.** The default filter is 2040 LP4 REV2 unless noted.

| Id | Name | Description | Modulation it shows |
|---|---|---|---|
| 01 | TAPE DRIFT | STABILITY AND A SLOW LFO WARP SAWS | STAB 0.6; LFO 1 0.15 Hz → FINE 2 and PITCH; LFO 2 → FREQ; chorus send |
| 02 | FADED HYMN | ENV 2 SWEEPS WAVE FROM SINE TO SAW | ENV 2 slow → WAVE 1 and WAVE 2; S&H LFO → SHAPE 1 (flutter); STAB 0.5 |
| 03 | NEON BRASS | PLAY SOFT FOR PAD, HARD FOR BRASS | VELOCITY → FREQ and level; ENV 2 → FREQ; LFO 1 → FINE 2 (the CS-style detune); ENV 3 slow → FINE (a pitch rise; a delayed vibrato is impossible because LFO DEPTH isn't a destination) |
| 04 | GROWL BASS | LFO ON SHAPE MAKES THE SAWS GROWL | LFO 1 synced → SHAPE 1 and SHAPE 2; LADDER LP4 driven; VELOCITY → FOLD |
| 05 | FOLD BASS | VELOCITY DRIVES THE FOLD AND BITS | MODE BITS; ENV 2 → BLEND; VELOCITY → FOLD and FREQ |
| 06 | GLIDE BASS | MONO PART: 80 MS GLIDE, SUB BENEATH | STEAL GLIDE, GLIDE TIME 80 ms; SUB; ENV 2 → FREQ; STAB 0.2 |
| 07 | FIFTH SLIDE | MONO PART: A GLIDING FIFTH ON OSC 2 | OSC 2 COARSE +7; GLIDE TIME 120 ms; LFO 1 → BLEND |
| 08 | SYNC SCREAM | ENV 2 SWEEPS OSC 2 THROUGH SYNC | MODE SYNC; ENV 2 → OSC 2's pitch; VELOCITY → sweep depth (the ENV 2 amount) |
| 09 | RING LEAD | RING MODE; VELOCITY ADDS THE BELL | MODE RING; VELOCITY → BLEND; LFO 1 → FINE (vibrato) |

Coverage: WAVE (02), BLEND (05, 07, 09), SHAPE (02, 04), filter (all), STABILITY (01, 02, 03, 06), velocity (03, 04, 05, 08, 09), LFO (01, 02, 04, 07, 09), ENV (02, 03, 05, 06, 08).

**FM, 16 TX81Z ports, `S9100010`–`S9100025`: private until #410.** Their ids are reserved; their Sources live in `$CHIMERA_TX_PORTS`. Each starts from a port and changes character by its `edits`.

| Id | Name | Kind | Description | Edits (category: change) |
|---|---|---|---|---|
| 10 | SOLID BASS | bass | PUNCHY FM BASS UNDER A LADDER | filter: LADDER LP4, ENV 2 → FREQ; envelope: D2R slower |
| 11 | WOOD BASS | bass | ONE ENVELOPE ON LEVEL AND TONE | envelope + filter: low-pass-gate pluck, ENV 1 → FREQ and VCA |
| 12 | RUBBER BASS | bass | ENV 2 MORPHS TWO ALGORITHMS | envelope: ALG B T5, ENV 2 → MORPH |
| 13 | TINE PIANO | keys | VELOCITY BRINGS OUT THE TINE BARK | LFO: LFO 2 tremolo on VCA; effects: chorus send |
| 14 | GLASS KEYS | keys | FOLD ADDS GLASS AS YOU PLAY HARD | fold: VELOCITY → FOLD |
| 15 | FUNK CLAV | keys | BANDPASS ENVELOPE GIVES THE QUACK | filter: CLEAN BP2, ENV 2 → FREQ, VELOCITY → depth |
| 16 | DRIFT ORGAN | keys | SLOW LFOS DRIFT THE DRAWBARS | LFO: LFO 2 and 3 → op 2 and op 3 LEVEL |
| 17 | TUBULAR | bell | INHARMONIC RATIOS RING ON | envelope: RR longer; effects: reverb send |
| 18 | ICE BELLS | bell | BRIGHT BELLS INTO THE DELAY | effects: delay send; envelope: NOTE → D1R |
| 19 | MUSIC BOX | bell | HIGH NOTES DECAY FASTER | envelope: NOTE → D2R route, RS 3 |
| 20 | FM BRASS | brass | ENV 2 RAISES FEEDBACK: THE BLAT | envelope: ENV 2 → op 4 FDBK |
| 21 | SOFT HORNS | brass | LFO BREATHES THE FILTER | LFO + filter: 2040 LP4, LFO 2 0.3 Hz → FREQ |
| 22 | WARM PAD | pad | LFO MORPHS THE ALGORITHM SLOWLY | LFO: ALG B a second algorithm, LFO 2 0.05 Hz → MORPH |
| 23 | GLASS PAD | pad | S&H SPARKLE ON OP 3 | LFO: LFO 3 S&H → op 3 LEVEL; effects: reverb send |
| 24 | KOTO PLUCK | pluck | LOW-PASS-GATE PLUCK, BONGO-LIKE | envelope + filter: one fast ENV on VCA and FREQ |
| 25 | MALLET | pluck | VELOCITY SETS THE MALLET HARDNESS | envelope: VELOCITY → modulator LEVEL and FREQ |

Which TX81Z voice each starts from is chosen at the porting step, by the nearest character in the Yamaha TX81Z factory banks (owner decision 2). The bank-to-voice record stays in the private ledger.

**FM vocal, `S9100026`–`S9100027`.** The filter is TYPE 2040, mode LP4, VOICE REV2, with OP 5 or OP 6 on FREQ at audio rate (ADR 0096). A tap reads its operator's output, so that operator must be sounding: either a carrier heard as part of the voice, or a modulator in an A algorithm.

| Id | Name | Description | How |
|---|---|---|---|
| 26 | VOWEL BUZZ | OP 5 SWEEPS THE 2040 AT AUDIO RATE | T-algorithm, op 5 a carrier at ratio 1.00 (the buzz) tapped to FREQ; ENV 2 → op 5 LEVEL moves the vowel; Q high |
| 27 | THROAT SING | OP 6 ON FILTER FREQ AND Q | A6 (6 → 5 → 1): op 6 at ratio 2.00 tapped to FREQ and Q; LFO 1 slow → op 6 LEVEL (overtone singing) |

**Formant placeholders, `S9100028`–`S9100030`** (`status = "placeholder"`, filled when #243 lands): VOWEL CHOIR, TALK BASS, FORMANT LEAD. Their descriptions are written with their Sounds.

**MODAL, `S9100031`–`S9100034`.** Note-off never damps MODAL (it rings like Rings).

| Id | Name | Model | Description | Modulation |
|---|---|---|---|---|
| 31 | NYLON | STRING | PLUCKED STRING; POSITION FOLLOWS NOTE | NOTE → POS; VELOCITY → BRIGHT |
| 32 | GAMELAN | BANK | HARDER STRIKES RING BRIGHTER | VELOCITY → BRIGHT (and the strike itself: EXCITE is read at note-on, scaled by velocity, not routable); LFO 1 slow → DAMP; LFO 2 → BRIGHT and STRUCT (shimmer) |
| 33 | CELLO SWELL | BOWED | ENV 2 SWELLS THE BOWED TONE | ENV 2 → BRIGHT and FREQ (FORCE is not a mod destination); LFO 2 → POS (SPEED is not a mod destination); LFO 1 → FINE (vibrato) |
| 34 | SITAR DRONE | SYMP | SYMPATHETIC STRINGS RING ON | STRUCT fixed; COUPLE and HALO high; reverb send |

**What's missing, and how each Sound still delivers:**
- **No mod wheel or aftertouch.** Expression comes from velocity, envelopes and LFOs; every Sound uses velocity for at least level or tone.
- **No legato flag.** Glide applies on a steal (STEAL GLIDE), which on a MONO Part is legato in effect.
- **MONO is a Part setting, not a Sound setting.** GLIDE BASS and FIFTH SLIDE say `MONO PART:` in their descriptions, set `demo = "mono"` for their renders, and sit on MONO Parts in CHORUS DELAY. Loaded onto a POLY Part, they still play, gliding only when a voice is stolen.
- **No unison or chord stack.** The fifths come from OSC 2's COARSE.

## 8. Tests

- **Codec:** a `Note` round-trips (encode, peek returns the text, decode gives the same `Sound` as without it). 41 B is ignored; non-printable bytes draw as `?`; a second or misplaced `Note` is ignored; `Note` before `Engine` is `Corrupt`; `Note` in a project's `Slot`, `Part` and `Fx` contexts is skipped; this build's skip count is 0; `unknown_non_critical_skipped` covers `0x001D`. A golden pins the tag and layout.
- **Old Sounds:** every v1 fixture and `library_1.snd` load and render identically (`v1_fixtures_render_identically`). A described fixture, `factory_1.snd`, joins them.
- **FACTORY guards:** `save_library_sound` takes only `UserId` (a `compile_fail` doctest with a `FactoryId`). MIX + EDIT and MIX + MINUS in FACTORY write nothing (a counting store). LIBRARY never lists `S9…`, and FACTORY lists only `S9…`. `rm` refuses `S9…` (existing). A FACTORY load goes through the guard and `adopt` like a LIBRARY load. A save on an empty card writes `P0000010`.
- **UI:** screen goldens, dark and INVERT (`light_golden_test`): FACTORY with a description, empty, NO CARD, a greyed COMING SOON row, and the three-tab title. `dpad_nav_test`: B over three tabs, and the inert MIX combinations. The text-width test covers the title and every committed description.
- **Tool:** a golden Source (`chimera-desktop/tests/fixtures/factory_sounds/golden.toml`) builds to committed bytes under a fixed stamp. `fmt` is idempotent, and text → `Sound` → text is identical. Every committed Source builds and passes `check`. Bad Sources fail with their line: an unknown ident, an unroutable destination, a duplicate id, an id outside its range, a description that is too wide, a `from` without `edits` (private build), a `from` at all (public build), a demo `part.n` naming a private Sound.
- **Privacy:** `no_private_ports_in_index` (§ 6) passes on the branch, and fails on a scratch repo where a Source with `from = "tx81z` is staged. The release check refuses a zip holding `S9100010`–`S9100025`.
- **Importer:** the importer spec's § 8.1 parser tests for TX81Z (fixtures built in code, a proptest that it never panics, the checksum warning), the § 8.2 TX81Z round trip for E rows, and the work-tree guard. `no_private_ports_in_index` fails on a tracked `*.syx`.

## 9. Task order (for the plan)

1. **ADR numbers.** Re-run `git for-each-ref --format='%(refname)' refs/heads refs/remotes | xargs -I{} git ls-tree --name-only {} docs/adr/`. Today 0128–0131 are taken or reserved, and `tape-coming-soon` may take 0132, so this work takes **0133** and **0134**.
2. **`Note` record and peek** (`storage/record.rs`, `storage/library.rs`), with codec tests. ADR 0133 lands here.
3. **FACTORY tab:** `UserId` and `FactoryId`, the listing, the rows and lines, the guards, the project id floor; goldens; flash and AXI deltas in a line.
4. **Source format and the `factory-sounds` build/fmt/check**, with the golden and the just recipes. ADR 0134 lands here.
5. **Render, QA and the page** (`factory-sounds qa`, `chimera-utils stim`).
6. **TX81Z importer** (`just import-tx`), with the guards and `.gitignore`. The importer spec and ADR 0088 are brought over from `importer-spec` (docs only; no code exists there).
7. **Author the VA 9.** After `sound-sends` merges, Sources set their own sends.
8. **Port and mutate the FM 16, privately**, then the vocal 2 (public).
9. **MODAL 4**, then **the 4 demo projects**.
10. **QA pass**, then **the owner's listening page**, then fixes. Then put the set on the unit for UAT.
11. **Release:** `factory-v1.zip` (public set only) and install notes (USB console, or bootloader SD-card-access copy) in the public releases repo, with `put-factory-sounds` for the unit.
12. **After #243:** fill the formant placeholders and re-run the QA pass for them.

**ADRs:**
- **0133**, "Factory Sounds list in a read-only FACTORY tab and carry a one-line description": the `Note` record `0x001D`; the factory id blocks (TAPE `S9000001`–`S9099999`, showcase `S9100001`–`S9199999`); `UserId`/`FactoryId`; the FACTORY tab; the reserved demo project ids `P0000001`–`P0000009`. It amends 0119.
- **0134**, "Factory Sounds are text Sources built and checked on the host": the Source format, the tool, the QA gates, and the TX81Z-derivative rule (ports are private, out of git and out of the public set until #410; Yamaha factory banks only). It relates to 0088.

## 10. Risks

- **Licensing.** The 16 ports are derivatives of voice data that isn't ours, so they stay private until #410 (owner decision 1). The public alpha has 15 showcase Sounds and 4 demo projects. The residual risk is a port leaking into git or a release; three checks stop it (§ 6).
- **Dependencies on branches in flight:** `sound-sends` (sends in the Sound), `tape-coming-soon` (#476), `formant-ops` (#243) and `importer-spec` (ADR 0088). Each task above names the one it waits on; none blocks tasks 2–5.
- **"Never samey" is a judgement.** The samey check only flags pairs; the owner's ear decides. Plan for at least two listening rounds.
- **Tester install:** USB console or bootloader SD-card copy (owner decision 4). The bootloader path writes the card from a computer, so the zip's layout must be exact; the release notes say where the `CHIMERA` folder goes.
