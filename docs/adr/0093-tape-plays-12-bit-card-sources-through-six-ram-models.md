# 0093. TAPE plays 12-bit card sources from the pool through six RAM models; STRETCH is the S950's stretch in real time (amends 0087)

- **Status:** Proposed
- **Deciders:** owner (#279 rulings, 2026-10-01 to 2026-10-04), firmware;
  revised after two adversarial reviews (2026-10-04)
- **Amends:** [0087](0087-sound-pool-by-memory-budget.md) (Proposed, on
  `sound-pool-budget`):
  - `SourceHead`'s payload is fixed: shape, count, root, period;
  - the CRC covers the structure and the bytes, never a root or period;
  - `held` starts with a 16 B descriptor;
  - a TRON instrument is one `ZoneSet` source.

## Context
The owner ruled that TAPE is one engine whose samples are raw oscillator
material:

- Four shared macros: POSITION, MOTION, TAPE and AGE.
- Sources are 12-bit, with a rate per source, S950-style. They live
  write-once on the card as sealed `/CHIMERA/TAPES/<CRC>.SRC` files.
- They are held whole in ADR 0087's arena for the random-access models.
- The models are LOOP, TRANSWAVE, REVERSE, PING-PONG, SCRUB, and STRETCH,
  an S950-style time-stretch.
- TRON (streamed) is next, and RESAMPLE/PROCESS come later. Neither may be
  blocked.

The card is sealed in the case, so WAVs arrive over USB. The 160 KiB arena
leaves 153 600 B for sources beside NEW. The budget is 7 000 cycles a
sample on rev V and 5 833 on rev Y; ADR 0091 reserves 2 016 of either.
Flash has about 97 KB left after #337.

The master tape stage holds the name `Tape` in code (`BlockRef::Tape`,
code 23, ident `"TAPE"`, ADR 0055).

## Decision
- **Names.** `BlockRef::Tape` stays the master stage: three unmerged
  branches use it 54 times.
  - The engine is `EngineType::Tape` (engine code 2), with block
    `BlockRef::TapeEngine` (code 34, ident `"TAPE_ENG"`), `dsp/tape_engine/`
    and `TapeEngineParams`.
  - The master stage's on-screen word becomes **REEL** (page short, matrix
    labels, bench row; UI strings only).
  - The engine's home page is **DECK**.
  - On screen, TAPE names only the engine and its macro.
- **The engine:** one `SlotKind::Tape`, about 200 B, inside the slot
  `ModalEngine` already sizes. `TapeModel` codes 0–5; 6 is TRON's.
- **Sources.** Mono, 12-bit and packed (ADR 0087).
  - `SourceHead` holds the `SourceRef`, `shape` (`Plain`, `Transwave` or
    `ZoneSet`), `count`, `root` (MIDI cents), `period` (YIN, Q16.16; 0 is
    unpitched) and a name: 34 B.
  - The CRC covers shape, count, any zone layout and the bytes. Root and
    period are labels in the file, so re-importing with a corrected root
    makes the same file.
  - **Playback identity lives in the Sound:** picking SOURCE copies the
    root and period into the Sound (ROOT, PERIOD; a zone set's into a
    `ZoneTune` record, 0x001A). A project then tunes the same on every
    card.
  - `held` begins with a 16 B descriptor (shape, count, rate, head
    frames), so the audio
    reads a source's shape from its own bytes.
  - Files are written by ADR 0087's `write_source`: verified, then sealed.
    A sealed failing file reads as missing and is never rewritten. A torn,
    unsealed one is replaced.
  - Loops are Sound parameters, as on the Digitakt.
- **TRON** (decided now, before 0x0016 freezes): **one `ZoneSet` source per
  instrument.**
  - It is a single file with up to 24 contiguous zones, described by a
    `SourceZones` record (0x0019). Its layout (start, frames, key) is in
    the CRC; each zone's root and period are labels, copied into the
    Sound's `ZoneTune`.
  - In the arena: the descriptor, the layout, then each zone's head.
  - **A head covers only the time until its first refill lands.** Its
    length is the descriptor's `head_frames`, so the format freezes none.
    TRON's spec sizes it from benched worst-case stalls and the refills one
    UI pass queues: a six-note chord queues about 32 ms of 5.4 ms reads,
    landing the last near 58 ms, and wow adds up to 33 cents of speed.
    The lone-note estimate (35 ms, 1 024 frames, 18 576 B an instrument,
    six Parts about 75 % with NEW) is an estimate, not a promise.
  - At about 3.5 MB, a zone set is past `put`'s 1 MiB. It needs ADR 0094's
    chunked `put` made resumable (256 KiB chunks, each with its own CRC and deadline,
    resumed from the unsealed file's length, sealed when whole), under its
    own future ADR, before TRON ships.
  - Six TRON Parts are six sources, so `MAX_SOURCES` = 32 and the `u32`
    `sounding` mask stand, and `SourceRef` freezes unchanged.
- **The voice:**
  - It latches the `SourceId` at note-on, and reads its row each block,
    keeping no pointer across blocks.
  - Its id sets the audio's `sounding` mask.
  - Every tap index is clamped to the row's frames, or wrapped inside a
    transwave frame: the `SourceReader`'s safety argument.
  - A note-on with no playable source is refused before voice allocation.
- **The read:**
  - a 32.32 head;
  - `inc = rate/48 000 · 2^((note·100 + pitch − root)/1200)`, computed once
    a block, ramped per sample, and clamped to 16;
  - a 12-bit decode;
  - 4-point, 3rd-order Hermite interpolation (Niemitalo).

  Its passband is −0.24 dB at 0.2 fs and −1.83 dB at 0.35 fs. Its worst
  image is −51 dB at 0.1 fs, −30 dB at 0.2 fs and −13 dB at 0.35 fs. The
  importer therefore band-limits at **0.35 × rate.**

  There is no anti-aliasing on pitch-up. Content folds above
  `24 000 / (0.35 · rate)` times a source's own speed (+18.2 semitones at
  24 kHz). That is the SP-1200's character, kept and bounded. The S950,
  with its per-voice clocks, didn't fold.
- **Macros.** The home page is MODEL, POSITION, MOTION, TAPE, AGE and SPACE,
  as on MODAL's RES. SIZE is on the model page with XFADE, DRAG and TYPE.
  - All five are modulatable and eased per block (ADR 0056).
  - TAPE is ADR 0053's `Transport` on the head's speed, once a block, one
    per voice.
  - AGE is a falling one-pole, hiss and dropouts. It runs, and is billed,
    when stored above 0 or routed (`stage_runs`' rule).
  - MODEL and TYPE latch at note-on.
- **The models:**
  - **LOOP:** plays from START, then loops `[Ls, Ls + Ll)`, latching new
    bounds at each wrap, with a linear crossfade (XFADE).
    - The crossfade reads the material before the loop. Where there is
      too little (always, at neutral), it **folds inside the loop**, so
      INIT never hard-wraps.
    - MOTION walks the loop through the window.
  - **REVERSE:** LOOP mirrored.
  - **PING-PONG:** reflects at the loop's edges. New bounds latch at a
    turn, and a head left outside them moves inside under a 5 ms
    crossfade.
  - **TRANSWAVE:** plays only `Transwave` sources (on others the note-on is
    refused).
    - A transwave is up to 32 single-cycle frames, stored at octave mip
      levels from 512 (by default; `--top 1024` doubles the bytes) down to
      16 samples. Each level is band-limited to ⌊0.35 N⌋ harmonics by the
      importer: the PPG and Waldorf approach.
    - The 512 default costs only notes below about 94 Hz half their
      harmonics: C1 tops out at 5.9 kHz, not 11.7.
    - Two heads lerp adjacent frames, and the position `p` is ramped per
      sample.
    - At note-on the level is the **largest** whose step is ≤ 1. It changes
      only past the alias limit (a step over 1.43 or under 0.35), a move
      of more than ±6.2 semitones. Level 16 is alias-free up to a 4.3 kHz
      fundamental, just above C8; higher notes fold.
    - A change crossfades over one block, reading four heads. That is
      billed only when pitch can move more than ±6.2 semitones (STEAL
      GLIDE, or deep PITCH/FINE routes), as MODAL's `PITCH` term is. A
      vibrato costs nothing.
  - **SCRUB:** a finger, slewed by DRAG, drags a head that loops at
    MOTION's bipolar vari-speed (Morphagene's Slide and Vari-Speed). The
    finger's speed is bounded so the head's never passes `inc`'s clamp.
  - **STRETCH:** a time pointer at MOTION (−1 to +2), and grains read at
    the note's pitch.
    - A splice every D-TIME/2 (SIZE, 10–250 ms) starts a grain under one
      complementary fade, whose length is set when it starts. The next
      splice waits for it to end, so the gains always sum to 1 and a
      D-TIME change never bumps the level.
    - TYPE MONO starts each grain a whole number of the source's stored
      YIN periods from the old grain's head. An unpitched source plays MONO
      as POLY. These are the S950's MONO and POLY types.
- **Pages:**
  - `SRC · DECK · DRV · FLT · AMP · MOD`, opening on DECK;
  - SRC holds SOURCE, START, END and VOL;
  - the model page holds SIZE plus XFADE, DRAG or TYPE;
  - PIT is under DECK;
  - ignored controls dim from `tape::reads`;
  - SRC and DECK draw the strip: the waveform with each model's loop,
    frames, finger or grains, and the heads the audio publishes once a
    block.
- **SOURCE is turned in place:** the pool's sources, then the card's,
  dimmed, each with its cost and the meter's preview. One new gesture,
  with its hint (ADR 0086): `MIX + PLUS LOADS · 41K`, or `WON'T FIT ·
  NEEDS 41K, 12K FREE`.
- **CPU,** in cycles a sample. These are host estimates, with one head at
  88 instructions (measured on `SourceView::read`, TAPE Task 2), a two-head
  fade sample at 224 (measured on LOOP's loop, TAPE Task 3) and the cache
  misses of `inc` 16 (+12 a head). The budget is checked a block at a time
  and a whole block can lie inside a fade, so each two-head model bills
  its fade sample. The bench at build step 3 replaces them:

  | | LOOP, REVERSE, PING-PONG | SCRUB | TRANSWAVE | TRANSWAVE, pitch moving > ±6.2 st | STRETCH | AGE on |
  |---|---|---|---|---|---|---|
  | Billed | 390 | 390 | 400 | 770 | 400 | +20 |

  - **Admission.** A plain TAPE voice is admitted 8 on rev V, with ORBIT
    and with the master tape (497 at worst; 7 with the master tape when
    DRIVE and FOLD are on). On rev Y, LOOP, REVERSE, PING-PONG and SCRUB
    with AGE off (467) keep 8; with AGE, and STRETCH or TRANSWAVE with
    AGE (497), 7; with DRIVE and FOLD, 6. A TRANSWAVE whose pitch can move
    past ±6.2 semitones (867) is admitted 4 on rev Y and 5 on rev V.
  - **Eight voices hold while the benched engine stays under 546 on rev V
    and 400 on rev Y.**
  - **The bench rows are build step 3:** cold-cache worst blocks with
    eight voices on distinct sources at `inc` 16, STRETCH at its shortest
    D-TIME, and TRANSWAVE at C7 changing level every block. They are
    flashed before any model past LOOP is built, and a row over its bill
    raises it.
  - `cost_test` gains `tape_bills_each_model`.
- **Import is host only,** in `chimera-import tape` and `just tape`:
  - `hound` reads the WAV;
  - a high-pass, a trim and normalisation to −0.5 dBFS;
  - the root from `--root`, the `smpl` chunk or YIN, and the period from
    YIN;
  - `rubato` resamples at a 0.35 × rate cut-off (24 kHz by default);
  - `--transwave` builds the mip levels with `rustfft` (top 512, or
    `--top 1024`);
  - TPDF dither, then `pack12` and `write_source`.

  The firmware links no importer, dither or resampler.
- **The USB `put` extension is a dependency.** `put` as merged (ADR 0092,
  #361) takes project files only. Sources come with ADR 0094, built as the
  TAPE plan's Task 18:
  - a `/CHIMERA/TAPES/XXXXXXXX.SRC` path class;
  - a streaming `SourceCheck` (the CRC must equal the stem);
  - `write_source`'s rules: `OK PRESENT` for a sealed, verified file; a
    torn file replaced; `ERR put: damaged` for a sealed, failing one;
    sealing only after verifying;
  - 1 MiB at most, sent in 256 KiB chunks, each with its own CRC and 30 s
    deadline; a failure deletes the unsealed file, and a re-run retries.
    Resume is TRON's.
- **The factory set:** about 24 sources and four transwaves, about 1 MB.
  - It is frozen as **versioned release assets** (`tapes-v1`): the `.SRC`
    files, a manifest with pinned CRCs, and the CC0 originals with their
    licences.
  - A host render change never renames a source.
  - The rate is chosen per source.
  - The audio follows #282's dataset rule. None is committed.
  - **Owner ruling (2026-10-05):** `tapes-v1` is openly licensed
    recordings plus Chimera's renders. The owner's own recordings come
    later as new sealed `.SRC` files and new factory Sounds in a later
    versioned release, with no format change.

## Alternatives considered
- **Rename the master stage to `MasterTape`** (the second draft). It
  touches 54 uses on three unmerged branches for no change on disk.
- **Name the engine `Reel`** (the first draft). The user-facing name and
  the code would disagree.
- **Linear interpolation.** It is 1–2 dB more droop and 6–13 dB more
  imaging below 0.2 fs, to save about 20 instructions a head.
- **A 0.45 × rate cut-off.** At 0.45 fs, either kernel's image is within
  4 dB of the signal.
- **TRANSWAVE without mip levels.** The cycle step escapes both the import
  cut-off and `inc`'s clamp, so high notes alias hard.
- **TRANSWAVE levels blended every sample (trilinear).** Four heads always,
  where a block's crossfade on a level change needs them only when pitch
  moves.
- **WSOLA STRETCH, its search spread across the blocks before each splice.**
  About +35 cycles a voice, always. It matches period-locking on pitched
  sources, and only smooths POLY, whose metal is the point. It is kept as a
  costed follow-up TYPE.
- **An offline STRETCH writing a new source, as the S950 does.** It can't
  be played or modulated live, and it spends pool bytes.
- **The root in the CRC** (the first draft). Fixing a root would have
  duplicated the source.
- **One `SourceRef` per TRON zone.** 72 sources for six instruments, past
  `MAX_SOURCES` and the `u32` mask.
- **100 ms TRON heads** (ADR 0087's count). 43 200 B an instrument; six
  wouldn't fit beside NEW. A head only has to outlast the first refill.
- **Root and period read from the file.** A re-labelled copy on another
  card would retune a project.
- **A TRANSWAVE top level of 1 024 by default.** It doubles every
  transwave (37 % of the meter for 16 frames, against 22 %) for brightness
  below 94 Hz.
- **SPACE off the home page, with SIZE on it** (the first draft). #279
  rules four macros, and MODAL's RES pattern fits them.
- **Stereo sources.** They double every cost on the meter.
- **Import on the device.** The WAV parser, resampler, FFT and dither cost
  flash. The host has them for free.

## Consequences
- A third engine at about 10 KB of flash (leaving about 87 KB) and about
  4 KB of AXI (the params block's snapshot copies alone measure 2 976 B).
  **D2: 384 B** (measured) for the voices' `played` snapshots. The engine
  state itself fits the existing slot.
- Six one-second 24 kHz sources don't fit beside NEW. At 16 kHz, or at
  0.7 s each, they do. A 16-frame transwave costs 22 % of the meter, and six
  TRON instruments an estimated 75 %.
- The voice guarantees are host estimates until build step 3's bench. If
  the bench comes in higher, the counts are restated.
- MONO is smooth only on pitched sources. POLY keeps the S950's tremolo
  and metal.
- An on-device RESAMPLE needs a source's CRC before it opens the file, so
  it renders twice, or the FAT layer gains a rename.
- The #337 plan's on-device importer (Task 9's `quantise_tpdf`, and the
  importer in its flash table) moves to the host.
- Tests:
  - Hermite's measured response;
  - reads stay in the row (proptest);
  - pitch within 0.1 cent;
  - TRANSWAVE alias-free from C1 to C8, its level the largest with step
    ≤ 1, and vibrato billing no level heads;
  - tuning lives in the Sound;
  - STRETCH keeps pitch, scales time, and its fades are complementary;
  - INIT never hard-wraps;
  - PING-PONG clamps with a fade;
  - a missing source is refused before allocation;
  - a zone set is one source;
  - click-free sweeps, goldens, the cost row, the bench rows and importer
    fuzzing.

## Sources
- Spec: `docs/superpowers/specs/2026-10-04-tape-core-design.md`.
- Issues: #279 (TAPE), #282 (dataset rule), #337 (pool; plan at
  `sound-pool-budget` 4b7ac2c1), #317, #214, #215, #267.
- ADRs: 0011, 0013, 0040, 0042, 0045, 0051, 0053, 0055, 0056, 0060, 0063,
  0065, 0066, 0070, 0086, 0087, 0088, 0090, 0091, 0092.
- Code: `chimera-core/src/dsp/engines.rs`, `dsp/voice.rs`, `dsp/delay.rs`
  (`Transport`), `dsp/tape/` (the master stage), `params.rs`, `addr.rs`, `hw.rs`,
  `storage/codes.rs`, `ui/block_registry.rs`, `ui/mod_grid.rs`,
  `project/codec.rs`, `project/parts.rs`; `chimera-core/tests/cost_test.rs`.
- Akai S950 Operator's Manual, p. 32 (TIMESTRETCH: D-TIME, AUTO-D, MONO and
  POLY).
- E-mu SP-1200 (12-bit, 26.04 kHz, uninterpolated pitch).
- Elektron Digitakt II manual; Octatrack manual OS 1.25.
- Polyend Tracker manual v1.5.
- PPG Wave and Waldorf Microwave wavetables.
- Make Noise Morphagene manual.
- O. Niemitalo, "Polynomial Interpolators for High-Quality Resampling of
  Oversampled Audio", 2001.
- W. Verhelst and M. Roelands, WSOLA, ICASSP 1993.
- `hound`, `rubato`, `rustfft` (MIT/Apache-2.0): host only.
