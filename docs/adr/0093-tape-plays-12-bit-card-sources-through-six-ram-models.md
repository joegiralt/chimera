# 0093. TAPE plays 12-bit card sources from the pool through six RAM models; STRETCH is the S950's stretch in real time (amends 0087)

- **Status:** Proposed
- **Deciders:** owner (#279 rulings, 2026-10-01 to 2026-10-04), firmware
- **Amends:** [0087](0087-sound-pool-by-memory-budget.md) (Proposed, on
  `sound-pool-budget`): `SourceHead` gains a root, and the CRC covers it;
  `SourceRow.rate` carries the root in its high half.

## Context
The owner ruled that TAPE is one engine whose samples are raw oscillator
material:

- Sources are stored S950-style: 12-bit, with a rate per source.
- They live write-once on the card in `/CHIMERA/TAPES`.
- They are held whole in ADR 0087's arena for the random-access models.
- The models are LOOP, TRANSWAVE, REVERSE, PING-PONG, SCRUB, and STRETCH, an
  S950-style time-stretch.
- TRON (streamed) is next. RESAMPLE and PROCESS come later. Neither may be
  blocked.

The card is sealed in the case, so WAVs arrive over USB. The 160 KiB arena
leaves 153 600 B for sources beside NEW: 4.3 s at 24 kHz. The budget is
7 000 cycles a sample on rev V, and ADR 0091 reserves 2 016 of it. The name
`Tape` is taken in code by the master tape stage (`BlockRef::Tape`, block code
23, ADR 0055).

## Decision
- **One engine, `EngineType::Tape`** (engine code 2, `"TAPE"`).
  - Its state is `ReelEngine`, and its block is `BlockRef::Reel` (block code
    34, `"REEL"`). Nothing is renamed.
  - It is one `SlotKind::Tape` of about 160 B, inside the slot `ModalEngine`
    already sizes, so D2 grows by nothing.
  - The models are `ReelModel` codes 0–5. Code 6 is left for TRON.
- **Sources:**
  - mono, 12-bit and packed (ADR 0087);
  - `SourceHead` gains `root: u16`, in MIDI cents, set at import;
  - the CRC, and so the file's name, covers the root and the bytes;
  - loops are Sound parameters, not source attributes, as on the Digitakt.
- **The voice:**
  - It latches the `SourceId` at note-on, and reads its row (`base`, `held`,
    rate and root) each block. It keeps no pointer across blocks.
  - Its id sets the audio's `sounding` mask, which holds off ADR 0087's
    compaction.
  - Every index is clamped to the row's frames, branch-free: the
    `SourceReader`'s safety argument.
- **The read:**
  - a 32.32 fixed-point head;
  - `inc = rate/48 000 · 2^((note·100 + pitch − root)/1200)`, computed once a
    block and ramped across it;
  - a 12-bit decode of four samples;
  - 4-point, 3rd-order Hermite interpolation.

  There is no anti-aliasing on pitch-up. Sources are band-limited to 0.45 ×
  their rate at import, so a lower rate aliases less, as on the S950. A
  note-on ramps its first 1 ms.
- **The window and macros.**
  - Every model plays inside START–END.
  - POS, SIZE, MOTION, TAPE and AGE are modulatable and eased per block, as
    MODAL's are (ADR 0056).
  - TAPE is ADR 0053's `Transport` on the head's speed, run once a block, one
    per voice.
  - AGE is a falling one-pole, hiss and dropouts. At 0 it is bypassed and
    unbilled.
  - MODEL, FRAME and TYPE latch at note-on.
  - A loop's bounds latch at each wrap, under its crossfade, so nothing snaps.
- **The models:**
  - **LOOP:** plays from START, then loops `[Ls, Ls + Ll)` with a linear
    crossfade (XFADE), reading the material before the loop. MOTION walks
    the loop through the window.
  - **REVERSE:** LOOP mirrored.
  - **PING-PONG:** reflects at the loop's edges. One head, no crossfade.
  - **TRANSWAVE:** FRAME-sample frames, one cycle at the note's pitch. Two
    heads lerp adjacent frames by POS, and MOTION scans a SIZE-wide
    triangle.
  - **SCRUB:** a finger slewed by DRAG drags the head, which loops at
    MOTION's bipolar vari-speed (−2 to +2) behind the finger: Morphagene's
    Slide and Vari-Speed. At MOTION 0 it sounds only while the finger moves.
  - **STRETCH:** a time pointer at MOTION (−1 to +2, 0 freezes) and two
    grains under triangular windows half a grain apart, which sum to 1.
    - Their length is SIZE, the S950's D-TIME (10–250 ms), and they read at
      the note's pitch.
    - TYPE MONO starts each grain a whole number of the root's periods from
      the grain it overlaps (one division a grain). POLY doesn't. These are
      the S950's MONO and POLY types.
    - No correlation search.
- **The chain is `SRC · TAPE · DRV · FLT · AMP · MOD`,** and it opens on TAPE.
  - SRC holds SOURCE, START, END and VOL.
  - TAPE holds MODEL and the five macros.
  - A model page named for the model holds XFADE, FRAME, DRAG or TYPE.
  - PIT is a sub-page of TAPE.
  - Controls a model ignores dim from one `reel::reads` table.
  - SRC and TAPE draw **the strip:** the window's waveform with each model's
    loop, frames, finger or grains, and its heads, published by the audio
    once a block.
- **SOURCE is turned in place:**
  - the pool's sources first;
  - then, dimmed, the card's, each with its cost and the meter's preview.

  One new gesture loads a card source, with its hint (ADR 0086): `MIX +
  PLUS LOADS · 41K`, or `WON'T FIT · NEEDS 41K, 12K FREE`.
- **Costs** are cycles a sample, host estimates until the bench:

  | | PING-PONG | LOOP, REVERSE, TRANSWAVE, SCRUB | STRETCH | AGE on |
  |---|---|---|---|---|
  | Billed | 100 | 170 | 180 | +20 |

  The worst voice is 277 at LP24. Beside `Instrument::RESERVED`, it admits
  all **8** voices on rev V, on rev Y, with ORBIT and with the master tape.
  `cost_test` gains `tape_bills_each_model`.
- **Import is host-side:** `chimera-import tape` and `just tape`.
  - It reads with `hound`, high-passes, trims and normalises to −0.5 dBFS.
  - The root comes from `--root`, the `smpl` chunk or YIN, in that order.
  - It resamples with `rubato` at a 0.45 × rate cut-off (24 kHz by default),
    TPDF-dithers to 12 bits, and writes through `chimera-core`'s
    `SourceWriter`.
  - The firmware links no importer.
- **The USB `put` extension is a dependency,** under its own ADR (ADR 0092
  says so):
  - a `/CHIMERA/TAPES/XXXXXXXX.A` path class;
  - a streaming `SourceCheck` (the CRC must equal the stem);
  - write-once: a verified file already there answers `OK EXISTS`, and a
    torn one is replaced;
  - 1 MiB at most;
  - a deadline that scales with the size.
- **The factory set is card-only:** about 24 sources, about 1 MB.
  - The rate is chosen per source (16 kHz for warm material, 32 kHz for
    bright, 12 kHz for noise, 48 kHz for transwaves).
  - The audio is Chimera's own renders from committed recipes, with pinned
    CRCs, plus CC0 audio and the owner's recordings, never committed (#282's
    dataset rule).
  - It is installed with `just put-factory-tapes`.

## Alternatives considered
- **Rename the master stage to free `Tape`.** That is churn across seven
  files for a name the user never sees. The stage's disk ident `"TAPE"` is
  frozen anyway.
- **Linear interpolation.** Its images sit about 30 dB down when an 8–16 kHz
  source is upsampled 3–6×. It saves only about 6 instructions a head.
- **Drop-sample playback** (the SP-1200's crunch). It suits AGE or PROCESS
  later, not the default.
- **An anti-aliasing filter on pitch-up.** Costly per voice, and it removes
  the character. Band-limiting at import already bounds the aliasing by
  rate.
- **WSOLA-lite STRETCH.** A 64-lag search at each splice is about 8 000
  multiply-adds landing in one block, which admission must bill at about
  125 cycles a sample a voice. Period-locking aligns pitched sources for
  nothing.
- **An offline STRETCH that writes a new source, as the S950 does.** It
  can't be played or modulated live, and it spends pool bytes.
- **Equal-power crossfades.** They swell on correlated material, and an
  oscillator's loop is correlated.
- **Loops and roots both in the source.** Loops belong to Sounds (the
  Digitakt), so one source serves many Sounds. The root belongs to the
  material.
- **Stereo sources.** They double every cost on the meter.
- **A separate source browser rung.** It means another screen and ladder
  step. The Digitakt turns its SAMPLE SLOT in place.
- **Import on the device.** The WAV parser, the resampler and the dither
  cost flash. The host has them for free.

## Consequences
- A third engine at about 9 KB of flash, about 2 KB of AXI and no D2.
- Six one-second sources at 24 kHz don't fit beside NEW. At 16 kHz, or at
  0.7 s each, they do. The rate per source is the user's trade, as on the
  S950.
- A TAPE Part needs its card's sources. A missing one plays silent, reading
  `SOURCE MISSING`.
- STRETCH's POLY keeps the S950's tremolo and metal. MONO is smooth only on
  pitched sources with a right root.
- An on-device RESAMPLE needs a source's CRC before it opens the file,
  because the FAT layer has no rename. It renders twice, or the FAT layer
  gains a rename: its spec decides.
- The SPACE cell leaves TAPE's home page (owner question 1 in the spec).
- Tests:
  - decode and Hermite exactness;
  - reads stay in the row (proptest);
  - pitch within 0.1 cent;
  - STRETCH keeps pitch and scales time;
  - click-free loops and sweeps;
  - a SOURCE switch leaves the ringing note;
  - `param_sweep_test` over `ReelParams`;
  - goldens per model;
  - the gate and retirement with TAPE voices;
  - the cost row and the bench rows;
  - fuzzing the importer.

## Sources
- Spec: `docs/superpowers/specs/2026-10-04-tape-core-design.md`.
- Issues: #279 (TAPE), #282 (neural, the dataset rule), #337 (pool), #317
  (factory library), #214 and #215 (one-shots).
- ADRs: 0011, 0013, 0040, 0042, 0051, 0053, 0055, 0056, 0060, 0063, 0065,
  0066, 0070, 0086, 0087, 0088, 0090, 0091, 0092.
- Code: `chimera-core/src/dsp/engines.rs`, `dsp/voice.rs`, `dsp/delay.rs`
  (`Transport`), `dsp/modal/params.rs`, `params.rs`, `hw.rs`,
  `ui/block_registry.rs`, `storage/codes.rs`, `console/card.rs`;
  `chimera-core/tests/cost_test.rs`.
- Akai S950 Operator's Manual, p. 32 (TIMESTRETCH: percentage, D-TIME,
  AUTO-D, MONO and POLY types).
- Elektron Digitakt II manual (play modes, loop position); Octatrack manual
  OS 1.25 (Flex and Static machines, timestretch).
- Polyend Tracker manual v1.5 (ping-pong, wavetable POSITION and WINDOW).
- Make Noise Morphagene manual (Vari-Speed, Slide, Gene-Size, Organize).
- O. Niemitalo, "Polynomial Interpolators for High-Quality Resampling of
  Oversampled Audio", 2001.
- `hound` (MIT/Apache-2.0), `rubato` (MIT): host only.
