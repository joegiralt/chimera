# TAPE core: 12-bit card sources played from the pool by six RAM models

Status: draft for owner review (2026-10-04), revised the same day after two adversarial reviews. Issue: private issue #279. ADR 0093 (Proposed) records the decision.

It builds on:
- the sound pool budget (private issue #337): ADR 0087, ADR 0090 and the plan at `sound-pool-budget` 4b7ac2c1, all unmerged;
- the USB `put` of ADR 0092, merged in #361 (project files only), extended to sources by ADR 0094 (TAPE plan Task 18);
- billing per ADR 0091 (`orbit-budget`).

TRON, the streamed model, is the second sub-project. RESAMPLE and PROCESS (MELLOW, SMEAR, AGE, neural: private issue #282) come after it. Neither is specified here. § 8 decides what they need now, before any format freezes.

## Intent

TAPE is Chimera's third engine, beside ALGO (six-operator PM) and MODAL. Its samples are **raw oscillator material**, not recordings to reproduce faithfully. The owner's rulings:

- **One engine named TAPE.** MODEL picks the playback algorithm, and SOURCE picks the sample (#279, 2026-10-01).
- **Four shared macros: POSITION, MOTION, TAPE and AGE.** Each model reads them its own way (#279).
- **Sources are stored S950-style:** 12-bit, with a sample rate per source. Every source lives on the card, write-once, under `/CHIMERA/TAPES`.
- **The pool's arena holds a whole short source,** or the first slice of a streamed one. Both count on the one meter (ADR 0087). Projects store only a `SourceRef`.
- **The RAM models in this core:** LOOP, TRANSWAVE, REVERSE, PING-PONG, SCRUB, and **STRETCH**, an S950-style time-stretch.
- **Sources come from WAV import plus a factory set,** which lives on the card only. WAV import travels over USB, because the card is sealed inside the case.
- **CPU:** a voice cost and an admission row, as for every engine (ADR 0040, `cost_test`, ADR 0091).
- **Never snap a parameter, never allocate or block in audio, and never read the card in audio.**
- **Few controls:** every MIX or hold gesture gets a focus-band hint (ADR 0086). Each model's page shows what it does to the sound (product vision).

## Prior art, and what we take

| Instrument | What it does | What we take |
|---|---|---|
| **Akai S950** | 12-bit, rate chosen per sample (7.5–48 kHz). Its TIMESTRETCH is an offline edit that writes a **new sample**. You set a stretch percentage and **D-TIME**, the splice length: "Longer D-TIME values will give the sample a slight tremolo effect whilst shorter D-TIME values give rise to the metallic effect." AUTO-D picks a D-TIME "like autoloop". A type is chosen for "single tones" (MONO) or "polyphonic samples like backing vocals or drum loops" (POLY). (S950 Operator's Manual, p. 32.) | 12-bit storage with a rate per source. STRETCH's controls: SIZE is the D-TIME, and TYPE is MONO or POLY. We take the character, tremolo at long splices and metal at short ones, **in real time** rather than as an offline edit. |
| **E-mu SP-1200** | 12-bit at 26.04 kHz. Pitch is changed by stepping through the sample with no interpolation filter, so pitching up folds energy back down. | The honest precedent for TAPE's **folding on pitch-up**: a character we keep, bounded by the import cut-off. |
| **Elektron Digitakt (I and II)** | Per-track sample SLOT, STRT, LEN, LOOP POSITION, and play modes FORWARD, REVERSE, FORWARD LOOP and REVERSE LOOP. A loop runs "between Loop Position and Length". There is no ping-pong mode. Projects reference samples on the +Drive. (Digitakt II manual, § Playback modes.) | The window and loop as Sound parameters, not sample attributes. SOURCE is a Sound parameter, turned in place. REVERSE loops backward. |
| **Elektron Octatrack** | **Flex machines** play from RAM, and **Static machines** stream from the card. Time-stretch is real time, with pitch and tempo independent. Projects keep a slot list that points into the card's audio pool. (Octatrack manual OS 1.25, § Machines, § Timestretch.) | RAM models now and a streamed model later (TRON), in one engine. A real-time stretch, with pitch and time on separate controls. |
| **Polyend Play / Tracker** | Sample types: one-shot, forward loop, backward loop, **ping-pong**, slice, **wavetable** (POSITION picks the window, WINDOW sets its length) and granular. One memory bar for the sample pool. (Tracker manual v1.5, § Instrument; Play manual, § Samples.) | PING-PONG as its own model. TRANSWAVE's frame and position. The one meter is ADR 0087's. |
| **PPG Wave / Waldorf Microwave and Blofeld** | Wavetables stored with **band-limited copies per octave**, chosen by pitch, so a high note doesn't alias. | TRANSWAVE's octave mip levels, written by the importer. |
| **Make Noise Morphagene** | **Vari-Speed** is "bipolar … speed and direction" of playback. **Slide** is "manual control for traveling through the captured sound and allows for scrubbing". **Gene-Size** shrinks the playback window into grains. **Organize** walks the splices. (Morphagene manual.) | SCRUB: a head with inertia under POSITION (Slide), at a bipolar MOTION (Vari-Speed). MOTION on LOOP walks the loop through the window (Organize). GRAIN, its own model, is later (#279). |
| **Ensoniq VFX/SD-1 Transwaves** | A long sample read as a series of single-cycle frames, the frame chosen live. | TRANSWAVE's name and its law. |

Sources:
- Akai S950 Operator's Manual, p. 32 (TIMESTRETCH): https://www.manualslib.com/manual/629018/Akai-S950.html?page=32
- Akai-style real-time two-grain stretch (JUCE forum, 2024): https://forum.juce.com/t/a-lightweight-akai-style-time-stretch-algorithm-realtime/60514
- Digitakt II manual: https://www.manualslib.com/manual/3436437/Elektron-Digitakt-Ii.html
- Octatrack manual OS 1.25: http://www.elektron.co.jp/wp-content/uploads/2015/01/octatrack_manual_OS1.25.pdf
- Polyend Tracker manual v1.5: https://files.kraftmusic.com/media/ownersmanual/Polyend_Tracker_Manual_v1_5.pdf
- Morphagene manual: https://www.makenoisemusic.com/wp-content/uploads/2024/03/morphagene-manual.pdf
- O. Niemitalo, "Polynomial Interpolators for High-Quality Resampling of Oversampled Audio" (2001): the 4-point, 3rd-order Hermite.
- W. Verhelst and M. Roelands, "An overlap-add technique based on waveform similarity (WSOLA)", ICASSP 1993.

## What exists (main at cf50ee74, plus the unmerged branches it builds on)

- **The engine slot (ADR 0051):** `EngineSlot` holds one engine, rebuilt in place. Adding an engine means one variant there; every `match` lists the arms.
- **The voice chain (`dsp/voice.rs`):** engine → drive → filter → folder → DC blocker → VCA, with three ENVs and three LFOs.
  - A voice lives while its VCA routes hold it (`vca_holds`).
  - `Voice::cost` is `EngineSlot::cost` + `CHAIN_COST` (30) + the filter's delta over LP24 + `ModRouting::cost` (47 at its floor) + the stages. A stage that can run is billed even when it is stored at 0 (`stage_runs`).
- **The budget (`hw.rs`, ADR 0091):** 7 000 cycles/sample on rev V and 5 833 on rev Y, at 70 %.
  - `Instrument::RESERVED` is the bus's 1 180 plus a note-on burst of 836, 2 016 in all (2 326 with the master tape).
  - ORBIT adds 171 while it plays.
- **The master tape stage owns the name in code today:** `BlockRef::Tape` (disk code 23, ident `"TAPE"`), `dsp/tape/`, `TapeParams`, the page short `TAPE` and the matrix labels `TPE` and `TAPE` (ADR 0055; off the chain, behind `master-tape`). Three unmerged branches use `BlockRef::Tape` 54 times, so it keeps the name; § 3 gives the engine its own and changes only the stage's on-screen word.
- **The pool (ADR 0087 and its plan, unmerged):**
  - a 160 KiB arena in AXI, where NEW's 32 Sounds leave **153 600 B for sources**: 4.3 s at 24 kHz, 6.4 s at 16 kHz;
  - `SourceId`, `SourceRef { crc, frames, rate }`, `MAX_SOURCES` = 32;
  - the `SourceTable` (rows `base`, `held`, `rate`; `gate`; a `u32` `sounding` mask);
  - retirement, and compaction under the gate;
  - 12-bit packing, two samples in three bytes;
  - source files at `/CHIMERA/TAPES/<CRC>.SRC`, written by `write_source`, which reads back, verifies and then **seals** the file read-only (`Wrote::{New, Present, Replaced}`). A sealed file that fails its check reads as missing and is never rewritten. A torn, unsealed file is replaced.
- **Flash:** 120 304 B free on `bench,orbit`, the largest image (#337 spec, re-measured with fat LTO). #337 spends about 23 KB, leaving **about 97 KB**.
- **The card,** measured on the owner's unit (`card` on the console, cf50ee74):
  - 2.3 MB/s sequential;
  - 5.4 ms p95 for a random 4 KB read;
  - a 16 GB card.

  That is about 10 streams at 24 kHz/12-bit with margin. Under load the UI loop runs 22–26 ms a pass (stress test, 2026-10-02).
- **USB `put` (ADR 0092, merged in #361):** project files only (`/CHIMERA/PROJECTS/`), at most 1 MiB, written through an A/B save, with a 30 s deadline. It takes no source today.

## 1. Sources

### The source file

ADR 0087's `Source` file is `FileKind` 6, at `/CHIMERA/TAPES/<CRC>.SRC`: a header, `SourceHead` (0x8017), then `SourceData` records (0x0018). This spec fixes `SourceHead`'s payload. ADR 0087 is Proposed and these records aren't built yet, so this is an amendment, not a migration.

| Field | Bytes | In the CRC | What it is |
|---|---|---|---|
| `SourceRef` (crc, frames, rate) | 10 | n/a | the identity |
| `shape` | 1 | **yes** | 0 `Plain`, 1 `Transwave`, 2 `ZoneSet` (TRON, § 8) |
| `count` | 1 | **yes** | Transwave: frames (1–32). ZoneSet: zones (1–24). Plain: 1 |
| `root` | 2 | no | the import's pitch estimate, MIDI cents (6 000 is C4): copied into a Sound when SOURCE is picked |
| `period` | 4 | no | the import's YIN period in frames, Q16.16; 0 means unpitched: copied into a Sound when SOURCE is picked |
| name | 16 | no | |

**34 B.** A `ZoneSet`'s `SourceZones` record (0x0019) follows its head (§ 8).

- **The CRC covers `shape`, `count`, any zone table's layout and the packed bytes:** what the audio's reads depend on. The name, root and period are labels, and every root and period, a zone's included, stays out of it.
  - Re-importing the same audio with a better root therefore makes **the same file**: `write_source` answers `Present`, and nothing is duplicated.
- **Playback identity lives in the Sound.** When SOURCE is picked, the UI copies the file's `root` and `period` into the Sound's `TapeEngineParams` (ROOT and PERIOD, § 3), and a zone set's per-zone roots and periods into the Sound's `ZoneTune` record (§ 8). The voice reads them from the Sound, never from the file.
  - So a project tunes the same on every card, whatever label a copy of the source carries.
  - A wrong root is fixed in the Sound: re-pick the source after a re-import, or turn PIT's PITCH and FINE (ADR 0042). The file is never edited.
- **Mono only.** The S950 and the original Digitakt are mono. A stereo source would double every cost on the meter, and voices are panned by their Part.
- **Loops are not attributes of a source.** They are Sound parameters, as on the Digitakt, so one source serves many Sounds. A WAV's `smpl` loop is printed by the importer as a suggested POSITION and SIZE (§ 6).

### What the arena holds

**`held` begins with a 16 B descriptor:** `shape`, `count`, the rate and `head_frames` (`u32`: for a `ZoneSet`, the frames of each zone's head held in RAM; 0 for a source held whole), the rest reserved. Head length is data, so the format never freezes one. A `ZoneSet`'s zone table follows it (§ 8). The packed samples come after that. The descriptor is inside `held`, so it is on the meter. The audio reads a source's description from the same bytes it plays, and ADR 0087's 12 B `SourceRow` stays as it is.

## 2. The engine

### Structure

```rust
pub enum EngineType { Algo = 0, Modal = 1, Tape = 2 }
pub enum SlotKind   { Algo, Modal(ResonatorMode), Tape }   // one kind: every model has the same state
in_place_enum! { pub enum EngineSlot { Algo(AlgoEngine), Modal(ModalEngine), Tape(TapeEngine) } }   // dsp/tape_engine/

pub enum TapeModel { Loop, Reverse, PingPong, Transwave, Scrub, Stretch }   // disk codes 0..5; TRON takes 6

/// What a voice reads a source through: one block's copy of its row.
pub struct SourceView<'a> {
    bytes: &'a [u8],     // exactly `held` bytes from `base`
    desc: Descriptor,    // its first 16 B, decoded
    frames: u32,         // the frames the rest of `held` holds
}

pub struct TapeEngine {
    src: Option<SourceId>,   // latched at note-on, never re-resolved
    model: TapeModel,        // latched at note-on
    heads: [Head; 4],        // 32.32 fixed-point position and direction; 2 in use, 4 in a TRANSWAVE level change
    inc: Ramp,               // frames per output sample, ramped across the block
    mac: Macros,             // POSITION, MOTION, TAPE, AGE, SIZE: eased (as MODAL's `Macros`)
    bounds: Bounds,          // the loop or window, latched at each wrap or turn
    fade: Fade,              // the one crossfade in progress: its gain ramp and length
    grain: GrainClock,       // STRETCH: time pointer, next splice, fade length
    level: u8,               // TRANSWAVE: the mip level in use
    wow: Transport,          // ADR 0053's transport, run once a block
    age: Age,                // one-pole, hiss xorshift, dropout gain
    declick: u8,             // samples left of the note-on ramp
}
```

- **The voice calls TAPE as it calls the others,** with a context in place of today's bare `&mut SymPool`: `EngineCtx { sym: &mut SymPool, src: &SourceTableView }`. ALGO and MODAL ignore `src`.
- **`TapeEngine` is about 200 B.** It sits in an `EngineSlot` sized by `ModalEngine`, so the engine adds nothing to D2. A test pins `size_of::<TapeEngine>() < size_of::<ModalEngine>()`.
  - **D2 still grows by 448 B:** `ParamSnapshot` gains `TapeEngineParams` (about 56 B), and each of the eight voices keeps a `played` snapshot. It is measured against `VOICE_CHAIN_BYTES` at build step 3.
- **A note-on with no playable source is refused before voice allocation.** That covers no source picked, `SOURCE MISSING`, and TRANSWAVE on a source that isn't a `Transwave`.
  - The Instrument checks the Part's `SourceId` and its row before it asks the allocator.
  - So no voice is stolen and nothing fades, for a note that can't sound.
- **`is_active`** is true from note-on until the voice ends. A loop has no end, so the note ends when its VCA routes let go (`vca_holds`), as a Modal note does. TAPE's INIT routes ENV 1 into VCA.

### Reading a source

**Position and pitch.** A head holds its position as 32.32 fixed-point frames, which is exact over any source the arena can hold. It advances by `inc` each sample:

```
inc = rate / 48 000 · 2^((note·100 + pitch_cents − ROOT) / 1200)
```

`ROOT` is the Sound's copy (§ 1).

- `pitch_cents` is PIT's PITCH and FINE, with their routes (ADR 0042).
- `inc` is recomputed once a block (one `exp2f`) and ramped linearly across it, so a pitch route or a glide never steps. STEAL GLIDE (ADR 0065) glides `inc`.
- **`inc` is clamped to 16:** four octaves over the source's own speed at 48 kHz.
- TRANSWAVE reads its own step (§ TRANSWAVE).

**12-bit decode.** Sample `i` is in pair `i >> 1`, at byte `3·(i >> 1)`.
- An even sample is `b0 | (b1 & 0xF) << 8`, and an odd one is `b1 >> 4 | b2 << 4`. Each is sign-extended with `(x << 20) >> 20` on an `i32`.
- The `1/2048` scale is folded into the interpolator's weights.
- Four consecutive samples take three unaligned word loads, which are free on the M7, and a select between the even and odd layouts for each tap.

**Interpolation: the 4-point, 3rd-order Hermite** (Niemitalo's x-form). Its response, computed from the kernel's Fourier transform, against linear interpolation, with `fs` the source's rate:

| Signal at | Hermite passband | Hermite's worst image | Linear passband | Linear's worst image |
|---|---|---|---|---|
| 0.05 fs | 0.00 dB | −70 dB | −0.07 dB | −51 dB |
| 0.10 fs | −0.02 dB | −51 dB | −0.29 dB | −38 dB |
| 0.20 fs | −0.24 dB | −30 dB | −1.16 dB | −24 dB |
| 0.35 fs | −1.83 dB | −13 dB | −3.65 dB | −11 dB |
| 0.45 fs | −4.3 dB | −4 dB | −6.2 dB | −3.5 dB |

The images are relative to the signal, at `fs − f`.
- Hermite is flatter, and 6–19 dB cleaner below 0.2 fs, where a source's energy is.
- Near the band edge both kernels are poor.
- So **the importer's cut-off is 0.35 × rate,** not 0.45. Above it, the image would sit within 4 dB of the signal.
- At 0.35 fs the top octave's images are 13 dB down, under −1.8 dB of droop. That is the price of a two-tap-wide kernel, and part of the dark sound.
- At 24 kHz the cut-off is 8.4 kHz. #279's "~10 kHz" tape top needs 32 kHz.

**Folding on pitch-up, by design.** There is no anti-aliasing filter on the read.
- Content up to 0.35 × rate folds back once a note plays the source above `24 000 / (0.35 · rate)` times its own speed:
  - +6.2 semitones at 48 kHz;
  - +18.2 at 24 kHz;
  - +37.2 at 8 kHz.
- Lower-rate sources fold less.
- This is the SP-1200's character, not the S950's: the S950 replayed each voice through its own variable clock and analogue filter, so it didn't fold. We keep the fold as a sound, bounded by the cut-off and by `inc`'s clamp, and the voice filter follows.

**Never outside the row.**
- Every one of the four tap indices is clamped to `[0, frames − 1]`, branch-free, from the view's own `frames`.
- TRANSWAVE's taps wrap inside their frame instead (`& (N − 1)`, N a power of two).
- So no read leaves the `held` bytes the row publishes. That is the `// SAFETY:` argument `SourceReader` needs (ADR 0087).
- If `held` is less than the whole source, as with a TRON head (§ 8), the models play inside what is held.

**The note-on ramp.** A head's first 48 samples (1 ms) ramp from 0, so a START inside a waveform never steps.

### The macros, and the window

Every model plays inside the **window** `[START, END]`, as fractions of the source (SRC page); `W` is its length in frames. The four macros (#279) are on the home page, and SIZE is on the model page. All five are modulatable, read every block from the voice's modulated params, and eased `EASE` = 0.3 of the way each block, as MODAL's are (ADR 0056). A note's first block takes them whole.

| | **POSITION** | **MOTION** (bipolar, 0 still) | **TAPE** | **AGE** | **SIZE** (model page) |
|---|---|---|---|---|---|
| **LOOP** | where the loop sits in the window | **walk:** the loop drifts through the window, up to ±1 window a second, wrapping | wow and flutter | wear | the loop's length, 2 ms to `W`, exponential |
| **REVERSE** | as LOOP | as LOOP | as LOOP | as LOOP | as LOOP |
| **PING-PONG** | as LOOP | as LOOP | as LOOP | as LOOP | as LOOP |
| **TRANSWAVE** | the frame, fractional | the scan's rate, 0 to ±8 Hz, a triangle around POSITION | as LOOP | as LOOP | the scan's width, in frames |
| **SCRUB** | the **finger**: where the tape is held | **vari-speed:** −2 to +2 times the note's speed | as LOOP | as LOOP | the window the head loops behind the finger, 2 ms to `W` |
| **STRETCH** | where time starts | the **speed of time:** −1 to +2. 0 freezes, 1 is the source's own | as LOOP | as LOOP | **D-TIME:** the splice length, 10–250 ms, exponential |

- **TAPE** is the transport's wow and flutter: ADR 0053's `Transport`, run **once a block** on the head's speed in cents, and ramped into `inc`. Its depth runs to ±25 cents of wow and ±8 of flutter. Each voice has its own transport and phase, because a Mellotron gives each note its own strip of tape. At 0 it adds exact zeros.
- **AGE** is the wear:
  - a one-pole low-pass whose corner falls from 20 kHz to 2.5 kHz;
  - hiss, from silence to −42 dBFS;
  - above 0.5, dropouts: a gain dipping up to 9 dB at random, for 20–80 ms, eased per block.

  It runs, and is billed, whenever it can be heard: stored above 0, **or routed** (`stage_runs`' rule). Unrouted at 0 it is bypassed and unbilled.
- **A loop's bounds are latched at each wrap.** The loop is `[Ls, Ls + Ll)`, with `Ll` from SIZE and `Ls = START + (POSITION + walk) · (W − Ll)`.
  - The head finishes its pass on the bounds it started with, then takes the eased new ones as it wraps.
  - A moved POSITION or SIZE therefore never cuts a pass short, and the wrap's crossfade covers the step.
  - This is how the Digitakt's loop behaves.

### The models

**LOOP.** The head starts at START, plays forward to the loop's end, then loops.
- **The crossfade** (XFADE, 0 to ½ of the loop; neutral 0.1): over the loop's last `Lx` frames, a second head reads the material `Ll` frames back, and the output fades linearly from the first head to the second. At the wrap the second head becomes the first. Linear, equal-gain, is right for an oscillator loop, whose material is correlated.
- **The crossfade folds inside the loop when it must.** If fewer than `Lx` frames lie before `Ls` (always the case at neutral, where `Ls` = START = 0), the fade reads the loop's own first `Lx` frames instead.
  - The loop then plays `[Ls + Lx, Ls + Ll)` after its first pass, with `[Ls, Ls + Lx)` as the fade's pre-roll.
  - **So no setting, INIT included, ever hard-wraps.** XFADE 0 is the one exception, and it asks for a hard splice.
- Two heads while crossfading; billed at two.

**REVERSE.** LOOP mirrored. The head starts at END and runs backward. Its crossfade reads the material after the loop, and folds inside the loop the same way. This is the Digitakt's REVERSE LOOP.

**PING-PONG.** The head reflects at each edge of the loop: `h = 2·edge − h`, and its direction flips.
- The value is continuous at a turn, so a turn needs no crossfade.
- **New bounds latch at a turn.** If the head then lies outside them (SIZE shrank, or POSITION moved past it), a second head starts at the clamped place inside, and a 5 ms crossfade moves the output there. A bound change never jumps.
- XFADE dims. One head, two during a clamp; billed at two.

**TRANSWAVE.** Plays only a `Transwave` source (§ 6); on any other, the note-on is refused, and SRC reads `NOT A TRANSWAVE`.
- **A transwave is `count` frames, each one cycle,** stored at **octave mip levels** from a top level down to 16 samples. Each level is band-limited to ⌊0.35 · N⌋ harmonics: the PPG and Waldorf approach, computed by the importer.
  - **The top level is 512 by default** (six levels, 512 → 16; 1 008 samples a frame). `--top 1024` adds a seventh (2 032 a frame), at twice the bytes.
  - What 512 loses: below about 94 Hz (F♯2), where 1 024 would serve, the harmonics stop at 179 in place of 358. C1 then reaches 5.9 kHz in place of 11.7 kHz; the loss shrinks to nothing at 94 Hz, where level 512 already reaches 16.8 kHz. Notes above 94 Hz are unchanged. A dark top on bass notes suits tape; a bright sub-bass transwave imports with `--top 1024`.
- **The frame position** is `p = POSITION · (n − 1) + SIZE · (n − 1) · tri(MOTION's phase)`, clamped to `[0, n − 1]`. It is ramped **per sample** across the block, so a scan never zippers at the block rate (750 Hz).
- **Two heads** at one phase `φ` read frames `⌊p⌋` and `⌊p⌋ + 1` at the same level. The output is their lerp by `p`'s fraction. `φ` advances `N_ℓ · f_note / 48 000` a sample: a frame is one cycle at the note's pitch, and the source's rate and root don't enter.
- **The level** at note-on is the **largest** one whose step `N · f / 48 000` is at most 1, so the step starts in (0.5, 1]. A level's content stops at 0.35 N, so a step up to 1.43 is still alias-free.
- **Hysteresis at the alias limit.** The level drops (to a smaller N) only when the step passes 1.43, and rises only when it falls under 0.35. From any note-on step, that is a pitch move of more than **±6.2 semitones** (`12·log2(1.43)`). Vibrato, wow (at most 33 cents) and ordinary bends never change the level.
- **The top notes.** The smallest level, 16 samples, holds 5 harmonics. It is alias-free up to a step of 1.43: a fundamental of **4.3 kHz, just above C8 (4 186 Hz)**. Above that, MIDI 109–127 fold even level 16's harmonics back down, as a plain source folds on pitch-up (§ 2): the SP-1200 character, at notes nobody plays as a wavetable. From about F♯6 (1.5 kHz) up, a note plays level 16's 5 harmonics, which is the price of no aliasing.
- **A level change crossfades over one block,** reading both levels for both frames: four heads in that block.
  - It can happen only when the Sound's pitch can move more than ±6.2 semitones within a note: STEAL GLIDE, or PITCH and FINE routes whose summed depth (|amount| × the destination's range) can pass it.
  - **Only then are the two extra heads billed,** as MODAL's `PITCH` term is (`glide_cost`). A vibrato route costs nothing extra.
- This is the Ensoniq transwave and the Tracker's wavetable mode, band-limited.

**SCRUB.** The finger `F` follows POSITION through a one-pole of DRAG (10–500 ms, the tape's inertia). It is computed once a block and ramped.
- The head is `F + l`. `l` advances `MOTION · inc` a sample, and wraps inside `[0, Ll)` with LOOP's crossfade.
- **Moving the finger drags the head with it,** so a moving POSITION bends the pitch by the finger's speed, as a hand on a reel does.
- **The finger's speed is bounded** so the head's total speed, `|MOTION · inc + dF|`, never passes `inc`'s clamp of 16 frames a sample. A POSITION jump becomes a fast scrub, never a skip.
- At MOTION 0 and a still finger, the head stands still, and the voice's DC blocker (ADR 0060) takes the held value to silence. Sound comes only while the finger moves, so a POSITION route from an LFO is a scratch.
- Two heads; billed as LOOP.

**STRETCH.** The S950's time-stretch, run in real time: two grains spliced by crossfades.
- **The time pointer** `T` moves through the window at `MOTION · rate / 48 000` frames a sample, wrapping in it.
  - At MOTION 1 the source keeps its duration, at 0.5 it takes twice as long, and at 0 it freezes.
  - Below 0, time runs backward while the grains still play forward.
- **A grain** reads at `inc`, so the pitch is the note's whatever MOTION is.
- **Splices.** Every `L/2` output samples (`L` from SIZE, the D-TIME), a splice starts a new grain at `T`, and **one fade** moves the output from the old grain to the new: the old grain's gain is always exactly 1 − the new one's.
  - The fade runs `L/2` samples, the length set when it starts.
  - The next splice comes only once that fade has ended.
  - So a change of D-TIME changes only the next fade's length, and the two gains always sum to 1. A moved SIZE never bumps the level, and a steady tone holds a steady level. In steady state this is two triangular windows half a grain apart.
- **TYPE MONO** (the S950's type for single tones) starts the new grain **in phase with the old one.** With `P` the source's YIN period from its header (fractional, Q16.16), the new grain starts at `T' = h_old − P · round((h_old − T) / P)`: as near `T` as it can be, a whole number of periods from the old head. That is one division a splice. A held tone's splices then line up, and only the S950's tremolo at long D-TIMEs remains.
- **An unpitched source (`period` 0) plays MONO as POLY,** and its TYPE cell reads `POLY (UNPITCHED)`.
- **TYPE POLY** starts at `T`, for chords, loops and noise. It keeps the metal and tremolo that make the S950's stretch recognisable.

**Why period-locking, and not WSOLA.**
- WSOLA searches each splice for the lag of best correlation. The next splice is known `L/2` ahead (at least 5 ms, 3.75 blocks), so the search can be spread over the blocks before it. That removes the burst that ruled it out in the first draft.
- Spread out, a 32-lag search over a 64-sample window costs:
  - 2 048 multiply-adds;
  - about 190 12-bit decodes into a scratch;
  - per splice, 5 ms apart at the shortest D-TIME.

  That is about 22 instructions a sample, **about 35 cycles a voice, billed always.**
- On a pitched source it lands where period-locking does: the best-correlated lag is a whole period. On noise nothing correlates.
- So its one gain is smoother POLY splices on chords and loops: exactly the S950's metal, which POLY exists to keep.
- Period-locking is free and exact on a pitched source, so TAPE takes it. A third TYPE, SMOOTH (WSOLA, +35), is the documented follow-up if UAT asks for it.

### Parameters never snap

- The macros and SIZE are eased per block. `inc`, TRANSWAVE's `p` and the transport are ramped per sample.
- A loop's bounds change only at a wrap, under a crossfade that folds inside the loop when it has to.
- PING-PONG's change at a turn, with a crossfade if the head must move.
- SCRUB's finger is slewed and speed-bounded.
- STRETCH's grains take new settings only at a splice, under one complementary fade.
- TRANSWAVE's level changes under a one-block crossfade.
- MODEL and TYPE latch at note-on, as MODAL's MODEL and MODES do, so a sounding note never changes under them.
- SOURCE latches at note-on: a Part changing SOURCE leaves its ringing notes on the old one (ADR 0087).
- START is read at note-on. END enters the loop's bounds, so it changes at a wrap.

## 3. Parameters, pages, the chain and names

**`TapeEngineParams`** (block `BlockRef::TapeEngine`, code 34, ident `"TAPE_ENG"`; ids frozen once written):

| Id | Param | Range, law | Read | Mod |
|---|---|---|---|---|
| 0 | MODEL | LOOP, REVERSE, PING-PONG, TRANSWAVE, SCRUB, STRETCH | note-on | no |
| 1 | START | 0–1 of the source | note-on | no |
| 2 | END | 0–1, kept ≥ START + 2 ms | each wrap | no |
| 3 | POSITION | 0–1 | block, eased | **yes** |
| 4 | MOTION | −1–+1, per model | block, eased | **yes** |
| 5 | TAPE | 0–1 | block, eased | **yes** |
| 6 | AGE | 0–1 | block, eased | **yes** |
| 7 | SIZE | 0–1, per model | block, eased | **yes** |
| 8 | XFADE | 0–0.5 of the loop | each wrap | no |
| 9 | DRAG | 10–500 ms, exponential | block | no |
| 10 | TYPE | MONO, POLY | note-on | no |
| 11 | ROOT | MIDI cents, copied from the source at pick; no cell | note-on | no |
| 12 | PERIOD | frames, Q16.16, copied at pick; 0 unpitched; no cell | note-on | no |

- **SOURCE is not a param.** It is the Sound's `SourceRef` record (0x0016, ADR 0087), and its cell is a `ParamSlot::source()`, as MODAL's panel cells are `modal_panel`.
- **Neutral** (`Sound::neutral(Tape)`, a format contract under ADR 0087) is LOOP, START 0, END 1, POSITION 0, MOTION 0, TAPE 0, AGE 0, SIZE 1, XFADE 0.1, DRAG 0.3, TYPE MONO. It loops the whole source with a folded 10 % crossfade, so it never hard-wraps.
- **FRAME is gone.** A transwave's frame is fixed at import (§ 6).

**The chain:** `SRC · DECK · DRV · FLT · AMP · MOD`. That is ALGO's shape with MODAL's first node. A Part's TAPE chain opens on DECK, MODAL's home rule (ADR 0066).

| Page | Short | Cells | Viz |
|---|---|---|---|
| Source (id 92) | SRC | SOURCE, START, END, VOL, –, – | the strip |
| Deck, the home (id 93) | DECK | MODEL, POSITION, MOTION, TAPE, AGE, SPACE | the strip |
| The model page (id 94), named for its model: LOOP, REV, PING, WAVE, SCRUB, STRCH | as named | SIZE on every model, then: LOOP, REV: XFADE · PING: XFADE (dimmed) · WAVE: nothing more · SCRUB: XFADE, DRAG · STRCH: TYPE | none |
| PIT (ADR 0042), a sub-page of DECK | PIT | PITCH, FINE, STEAL, GLIDE TIME | none |

- **The home page is MODAL's RES pattern exactly:** MODEL, the four macros, then SPACE (the Part's reverb send). Owner question 1 of the first draft is closed: #279's four macros fit.
- Block ids 92–94 follow main's highest (91). They are checked against the other branches when built.
- **A control the model ignores is dimmed,** with its matrix column, from one `tape_engine::reads` table, as MODAL's `modal::reads` does. PING-PONG and STRETCH dim XFADE.
- **The strip** is how a page shows what it does to the sound: a 216 × 40 px view of the window, the source's min/max per column in `MID`, with START and END marks. Over it, per model:
  - **LOOP and REVERSE:** the loop shaded in `ACCENT_DIM`, the crossfade as a ramp (drawn inside the loop when folded), and the head as a line in `ACCENT` with an arrow for direction. MOTION's walk moves the shading.
  - **PING-PONG:** the loop with `◂▸` at its edges, and the head bouncing.
  - **TRANSWAVE:** frame ticks along the window, the two current frames lit by weight, SIZE's scan as a bracket, and at the right end a single-cycle scope of the blend at the level in use (ADR 0070's WAVE scope).
  - **SCRUB:** the finger as a ▾ mark with a fading trail, the window behind it, and the head.
  - **STRETCH:** the time pointer as a line, and the two grains as ramps under the waveform: their length the D-TIME, their height the fade's gains now.

  The UI computes the overview from the arena when the page opens or SOURCE changes. A 100 000-frame source takes under 1 ms, and the result is a 432 B cache for the one page shown. The heads come from the audio: one `AtomicU64` per Part, holding the newest voice's head and grain positions, stored `Relaxed` once a block (48 B of AXI).
- **SOURCE, the cell.** Turning it steps through the pool's sources first: switching among them is free and instant. A divider follows, then the card's `/CHIMERA/TAPES` files that aren't in the pool, dimmed, each with its cost.
  - The focus band reads the name, rate, length and cost (`CHOIR · 24K · 1.2 S · 41K`), and the meter previews a card row (ADR 0087).
  - The directory listing gives stems and sizes, and so costs, with no extra read. Names are read on demand into a 16-entry head cache, one 4 KB read each (5.4 ms p95).
  - This is the Digitakt's SAMPLE SLOT, turned in place, with the Octatrack's audio pool behind it.
- **Gestures (ADR 0086).** There is one new gesture, with its hint:

| Where | Touched | Hint |
|---|---|---|
| SRC | SOURCE on a card row | `MIX + PLUS LOADS · 41K`, or `WON'T FIT · NEEDS 41K, 12K FREE` in `ALERT` |

  MIX + PLUS adds, as it primes a route in the matrix. The load reads 4 KiB a UI pass behind `ui::busy`, and MENU cancels it (ADR 0087). Once loaded, the row moves above the divider and the Part plays it. Nothing else in TAPE is a MIX or hold gesture.

**Names.** `BlockRef::Tape` stays the master stage: three unmerged branches use it 54 times, and renaming it buys nothing on disk. The engine's names are new:
- `BlockRef::TapeEngine`, block code 34, ident `"TAPE_ENG"` (`"TAPE"` is the stage's frozen ident);
- `dsp/tape_engine/`, `TapeEngine`, `TapeEngineParams`, `TapeModel`;
- `EngineType::Tape`, engine code 2, ident `"TAPE"` (the engine namespace, not the block one).

**On screen, the master stage gets its own word, REEL.** These are UI strings only, with no code, ident or Rust name changed:
- its page short `TAPE` → `REEL` (name `"Reel"`);
- its matrix labels `TPE` / `TAPE` → `REL` / `REEL`;
- the bench row `"TAPE"` → `"REEL"`.

TAPE then means two things only on screen: the engine (the browser's engine tag and the chain's title), and its TAPE macro (#279). The home page is DECK.

## 4. With the pool (#337)

- **A source is loaded by the UI only:** from a project load (pass 2), a library Sound, or MIX + PLUS.
  - Its bytes are staged in the gap and CRC-checked, then indexed.
  - Its row is published with `Release` (ADR 0087).
  - **Every source in this core is held whole,** because all six models are random-access.
- **A Part resolves its Sound's `SourceRef` to a `SourceId`** when it loads, and `AudioShared` carries the id (1 B a Part).
- **A voice latches the `SourceId` at note-on.** It never re-resolves the Part's ref. It reads its row's `base` and `held` at the start of each block, and the descriptor from those bytes. It never keeps a pointer across blocks.
- **The `sounding` mask** is the OR of every active TAPE voice's latched id, stored by the audio once a block. It is what holds compaction off (ADR 0087).
- **No bytes move under a sounding voice.** Compaction waits for silence, then closes the gate, acknowledges, re-checks and moves (ADR 0087). A note-on on a moving source during a closed gate is allocated and latched, and starts on the first block after the gate opens: at most 2.7 ms late.
- **A deleted or purged source still sounding is retired, never cut.** Its row stays live until its bit clears.
- **The meter** counts each source at `16 + held`, rounded up to 16 B, in `MID`.
- **What fits** in the 153 600 B NEW leaves (the meter reads with NEW's 10 240 B):

| Source | `held` | Meter |
|---|---|---|
| 1 s at 24 kHz | 36 016 | 29 % |
| 1 s at 16 kHz | 24 016 | 21 % |
| a 16-frame transwave (6 levels, 512 → 16, the default) | 24 208 | 22 % |
| a 32-frame transwave, default | 48 400 | 36 % |
| a 16-frame transwave, `--top 1024` | 48 784 | 37 % |
| six Parts, each on its own 1 s source, at 24 kHz | 216 096 | **won't fit** |
| the same at 16 kHz | 144 096 | 95 % |
| the same at 24 kHz, 0.7 s each | 151 296 | 99 % |

  A transwave costs the same whatever its rate. Its levels sum to `count · 1 008` samples by default.

  The rate per source is the S950's trade, quality against time, and the factory set uses it (§ 7). 24 kHz stays the import default (#279).

## 5. CPU

Host estimates, in the house manner (instructions × 1.46 × 1.1, as `ModalEngine`'s `COST_*`). **The bench replaces them at build step 3**, before any model past LOOP is built (§ 10).

**One head, counted per sample:**

| | Instructions |
|---|---|
| 32.32 advance; integer and fraction split; the fraction's `vmov` and `vcvt` to float | 6 |
| Four tap indices, each clamped (or wrapped in a frame) | 7 |
| Four taps: address, word load, even/odd select, sign-extend | 22 |
| Four `vmov` + `vcvt` to float | 8 |
| Hermite, with the 1/2048 folded in | 12 |
| **One head** | **≈ 55, 88 cycles** |

**Cache misses grow with `inc`.**
- A 32 B line holds 21.3 packed samples.
- A head at `inc` 1 crosses 0.05 lines a sample. At `inc` 16 it crosses **0.75 lines a sample**.
- At about 15 cycles a fill from AXI SRAM, that is up to **11 cycles a head**.
- The bill takes the worst `inc` (the clamp), because a pitch route can reach it inside a note. It is +12 a head.

| | Per sample |
|---|---|
| Per voice: the fade or window gains and the mix (4), the block's work spread over 64 (an `exp2f`, the transport, the easing, the row; 4), the note-on ramp (1) | 9 instructions, 15 cycles |

| Model | Heads | Instructions | Cycles (with misses) | **Billed** |
|---|---|---|---|---|
| LOOP, REVERSE | 2 | 119 | 191 + 24 = 215 | **230** |
| PING-PONG | 2 (one, two in a clamp) | 119 | 215 | **230** |
| SCRUB | 2 (+ the finger, 2) | 121 | 218 | **230** |
| TRANSWAVE | 2 (+ `p`'s ramp and the scan, 4) | 123 | 222 | **240** |
| TRANSWAVE, pitch able to move > ±6.2 st (STEAL GLIDE, or deep PITCH/FINE routes) | 4 in a level change | 233 | 422 | **450** |
| STRETCH | 2 (+ two gains, the grain clock, 5) | 124 | 223 | **240** |
| AGE stored above 0 or routed, on any model | (one-pole 3, hiss 5, gain 2) | 10 | 16 | **+20** |

`TapeEngine::cost(&TapeEngineParams, &ModState)` returns the model's figure plus AGE's, and `playing_cost` returns the latched model's, as MODAL's does. TRANSWAVE's level-change term is TAPE's `glide_cost`, billed exactly where MODAL's `PITCH` is.

**The voice, and what it is admitted.** The voice is the engine + 30 (chain) + 47 (`ModRouting::BASE`), at LP24 with no DRIVE or FOLD:

| Voice | Cost | rev V (4 984 room) | rev Y (3 817) | rev V, ORBIT (4 813) | rev V, master tape (4 674) |
|---|---|---|---|---|---|
| STRETCH or TRANSWAVE with AGE | 337 | 14 → **8** | 11 → **8** | 14 → **8** | 13 → **8** |
| … with DRIVE and FOLD (+102) | 439 | 11 → **8** | 8 → **8** | 10 → **8** | 10 → **8** |
| TRANSWAVE, pitch able to move > ±6.2 st, with AGE | 547 | 9 → **8** | **6** | 8 → **8** | 8 → **8** |
| … with the worst routing (633 in place of 47) | 1 133 | **4** | **3** | **4** | **4** |
| STRETCH with AGE and the worst routing | 923 | **5** | **4** | **5** | **5** |

**What these numbers support, and no more.**
- Eight voices of any plain TAPE Sound hold on rev V while the benched engine stays under **546** (4 984 / 8 − 77), and on rev Y under **400** (3 817 / 8 − 77). The estimates sit at 260, with room for the bench to come in up to 54 % higher before rev Y loses a voice.
- A TRANSWAVE whose pitch can move more than ±6.2 semitones gets 6 voices on rev Y (a vibrato route doesn't count), and the heaviest routing matches the heaviest ALGO.
- These are counts of the **billed** figures. The guarantee holds only once the bench's cold-cache rows confirm them. Until then they are stated as estimates.

**The bench rows** (build step 3, `--features bench`), each the worst block over four runs, cold cache:
- `TAPE LOOP /VOICE`: eight voices, each on its own source, at `inc` 16;
- `TAPE STRETCH /VOICE`: the same, at the shortest D-TIME, TYPE MONO;
- `TAPE WAVE C7 /VOICE`: TRANSWAVE at C7 with a ±12-semitone pitch LFO forcing a level change every block;
- `TAPE 8 VOICES`: the whole render.

`the_model_bills_every_bench_row_high` holds them. If a row exceeds its bill, the bill rises to it, and the table above is redone in the same commit.

**The `cost_test` row**, `tape_bills_each_model`, after `modal_bills_each_model`: for each model, with AGE stored 0 and 0.5, and with AGE routed at 0, it asserts:
- `EngineSlot::cost` equals the billed literal;
- the voices admitted on rev V, rev Y and with the master tape, beside `Instrument::RESERVED` (ADR 0091), as tabled.

`budget_capacity_per_engine` iterates `EngineType::ALL` and takes TAPE with no change.

## 6. WAV import

**Host only,** as ADR 0088's patch importer is. The firmware links no WAV parser, resampler, dither or mip builder. On-device conversion arrives with RESAMPLE (§ 8). The #337 plan still budgets an on-device importer (its Task 9 puts `quantise_tpdf` in `chimera-core/src/storage/twelve.rs`, and its spec's flash table counts an "importer with dither and decimator" in its 8 KB line). That should move to the host.

`just tape <file.wav> [--rate 24k] [--root C3] [--transwave <cycle> [--top 1024]] [--name CHOIR] [--channel mix|L|R]` runs `chimera-import tape` (in `chimera-desktop`, beside ADR 0088's binary):

1. **Read** PCM 8/16/24/32-bit or float WAV with `hound` (MIT/Apache-2.0, host only). Stereo is mixed `(L + R)/2` unless `--channel` picks a side.
2. **Clean.** A 5 Hz high-pass takes out DC. Leading and trailing silence under −60 dBFS is trimmed. The peak is normalised to −0.5 dBFS, so the 12 bits are used whole; the Sound sets the level.
3. **Pitch.** The root comes from, in order: `--root`; the `smpl` chunk's unity note and fraction; a YIN estimate over the loudest 100 ms. YIN's period goes in the header (Q16.16) if its aperiodicity is under 0.2. Otherwise the source is **unpitched**: `period` 0, root C4, and the log says so.
4. **Resample** to the chosen rate (48, 32, 24, 16, 12 or 8 kHz; 24 by default, #279) with `rubato`'s windowed-sinc resampler (MIT, host only), with its cut-off at **0.35 × rate** and 80 dB of stop band from 0.5 × rate.
5. **Transwave** (`--transwave <cycle>`, the cycle length in input samples):
   - cut the input into whole cycles, at most 32;
   - resample each to the top level's length (512, or 1 024 with `--top 1024`);
   - build the levels down to 16, each keeping ⌊0.35 · N⌋ harmonics by FFT truncation (`rustfft`, MIT/Apache-2.0, host only).

   `shape` 1, `count` frames, rate 48 000 nominal.
6. **Quantise** to 12 bits with TPDF dither and pack two samples in three bytes. This is `chimera-core`'s own `pack12`, so host and unit share the bytes. Dither is host code.
7. **Write** through `chimera-core`'s `write_source` (ADR 0087's plan): a CRC over the structure and the bytes; the file at `<CRC>.SRC`; read back, verified and sealed. It lands in the desktop simulator's card directory (`DirStore`), so QA plays it before the unit does.
8. **Report:** the name, rate, frames, seconds, root, period or `UNPITCHED`, the cost on the meter, any `smpl` loop as a suggested POSITION and SIZE, and a warning if it won't fit beside NEW.
9. **Send:** `just put-tape <file>` runs `chimera-usb.py put <file> /CHIMERA/TAPES/`.

A file is capped at 1 MiB, the `put` limit: 29 s at 24 kHz, 87 s at 8 kHz. RAM models use only what fits the arena. A TRON zone set is larger (12 zones × 8 s at 24 kHz is about 3.5 MB), so it needs the resumable `put` below; this core doesn't send one.

**What TAPE needs from the USB `put` (ADR 0092), as a dependency.** `put` as merged (#361) takes project files only. Sources come with ADR 0094, ADR 0092's extension, built as TAPE plan Task 18 (`docs/superpowers/plans/2026-10-05-tape-core.md`):
- **A second path class:** `/CHIMERA/TAPES/XXXXXXXX.SRC`, eight hex digits, any case. `ls` takes `tapes`.
- **A streaming `SourceCheck`** in place of `ProjectCheck`: the header's kind is `Source`; the `SourceHead`'s frames match the data; the CRC over the structure and data equals the stem. It refuses before writing, as a project's header is refused today.
- **`write_source`'s rules, not A/B:**
  - a sealed file that verifies answers `OK PRESENT`, writing nothing;
  - a torn, unsealed file is deleted and written again;
  - a sealed file that fails its check answers `ERR put: damaged`, is never rewritten, and reads as missing (card cleanup is #267);
  - a failed `put` deletes its unsealed file;
  - only a verified file is sealed.
- **Chunked from the start** (owner, 2026-10-05): `put <path> <size> <crc32>`, the data in 256 KiB chunks, each ending with its own CRC and under its own 30 s deadline. The core's limit is 1 MiB. A failure deletes the unsealed file, and a re-run is the retry.
- **Resume, and zone sets, for TRON,** under a later ADR before TRON ships: `put <path> <size> <crc32> resume` answers the bytes already on the card (the unsealed file's length, checked against the chunks' CRCs) and continues from there, with a `Store` append and an 8 MiB limit. The core reserves the word and refuses it. An abandoned unsealed file is a torn one, and `write_source`'s rule replaces it.

## 7. The factory set

- **Card only** (ADR 0087: 24 KB a second at 16 kHz is too much for flash).
- **About 24 sources, about 1 MB on the card,** across #279's list: piano, choir, flute, strings, guitar, voice, organ, metal, glass, noise, field. Plus four 16-frame transwaves.
- **The rate is chosen per source, as on the S950,** so a six-Part project fits:
  - 32 kHz for bright material (glass, metal);
  - 16 kHz for warm material (choir, flute, strings, organ, voice);
  - 12 kHz for noise and field recordings.

  Most sources are 0.5–1.5 s, 12–36 KB each.
- **Where the audio comes from,** under #282's dataset rule (Chimera's renders, the owner's recordings, clearly licensed audio; never commercial records):
  1. **Chimera's own renders:** ALGO and MODAL patches rendered offline by the desktop simulator from recipes in `tools/factory-tapes/`.
  2. **CC0 recordings** (Freesound CC0).
  3. **The owner's own recordings, later** (owner ruling, 2026-10-05). `tapes-v1` is openly licensed recordings plus Chimera's renders. The owner will record their own samples in time, and they join with **no format change**: each is a new sealed `.SRC` file, with new factory TAPE Sounds in the card library, shipped as the next versioned release (`tapes-v2`, …) beside the earlier files.
- **The built files are frozen, versioned release assets.** `tapes-v1` is a GitHub release holding every `.SRC`, its manifest (name, CRC, rate, source of the audio) and a zip of the **CC0 originals with each licence text**.
  - The manifest's CRCs are pinned by a test, and factory TAPE Sounds reference them.
  - A change to the host renderer never renames a factory source. The recipes record where a source came from; they aren't re-run to rebuild it.
  - A new or changed source is a new release, `tapes-v2`, beside the old files: they are write-once.
  - No audio is committed to the repo, as raw banks aren't (factory library method).
- **Install:** `just put-factory-tapes v1` downloads the release and puts each file. Re-running it is safe, because a present file answers `OK PRESENT`.
- **Each factory source has at least one factory TAPE Sound** in the card library (#317), by `SourceRef`. NEW's 32 stay ALGO and MODAL (ADR 0087).

## 8. Decided now for what comes next

**TRON's zones** (#279: two zones an octave; 12 an instrument; six Parts):
- **One `SourceRef` per TRON instrument: a `ZoneSet`,** not one per zone.
  - Its file holds all its zones, contiguous, in one `<CRC>.SRC`, with a `SourceZones` record (0x0019, defined now) after its head: per zone `start: u32`, `frames: u32`, `key: u8` (the MIDI note it serves), `root: u16` and `period: u32`, 15 B each, up to 24 zones (`count`).
  - **The layout (`start`, `frames`, `key`) is in the CRC,** because a voice's reads and zone choice depend on it. **Each zone's `root` and `period` are labels,** out of the CRC, as a plain source's are.
  - When SOURCE picks a zone set, they are copied into the Sound's **`ZoneTune` record** (0x001A, defined now: per zone `root: u16`, `period: u32`, 6 B each, 72 B for 12). The voice reads them from the Sound, so a TRON project tunes the same on every card. This is the same rule as § 1's.
- **In the arena,** `held` is the descriptor, the zone layout (9 B a zone), then each zone's head, in zone order. The voice picks its zone at note-on from the layout it reads in its own row.
- **A head covers only the time until the first refill lands,** not a fixed 100 ms. Its length is the descriptor's `head_frames`, chosen per instrument when the head is loaded, so nothing here freezes it.
- **TRON's spec sizes it** from benched worst-case stalls and from the refills one UI pass can queue, not from one stall alone. Example, a six-note chord: six 4 KB refills at 5.4 ms p95 queue about 32 ms of reads, so the last lands near **58 ms** after a 26 ms pass; wow (TAPE) can add up to 33 cents of read speed. At 24 kHz played 3 semitones up, that is 58 ms × 29 090 frames/s ≈ 1 690 frames, or 2 048 rounded.
  - **The estimate used below** is the lone-note case, about 35 ms (a 26 ms pass plus a stall): **1 024 frames, 1 536 B a zone.** The figures that follow are estimates for sizing, not promises; at 2 048 frames an instrument is about 37 KB.
  - An instrument is 16 + 108 (layout) + 12 × 1 536 = **18 556 B**, about 11 % of the arena, in place of the 43 200 B ADR 0087 counted.
  - **Six TRON Parts, estimated:** 6 × 18 576 (rounded to the granule, with the 16 B entry) = 111 456 B, about 75 % with NEW, leaving about 42 KB for RAM sources. At 2 048-frame heads six would not fit beside NEW; TRON's spec settles which, from the bench.
  - The streaming ring that follows a head is TRON's, in D2, refilled from the card, sized by the same benched worst case.
- **So `MAX_SOURCES` = 32 and the `u32` `sounding` mask suffice.** Six TRON Parts are six sources, not 72. A zone set is one row, one bit and one meter entry.
- **`SourceRef` (0x0016) freezes as ADR 0087 has it,** `{ crc, frames, rate }`, with `frames` the set's total. A zone set needs nothing from it.
- `TapeModel` leaves code 6 for TRON. A source's `held` is the most its users need: all of it for a RAM model, the layout and the heads for TRON. A Part switching a zone set to a RAM model is then a pool add, refusable, and TRON's spec states it.
- Refill stays in the UI loop. TRON's rings go in D2 (121.5 KB free) and must cover the slowest UI pass under load (26 ms) plus a card stall. At 43 KB/s a note (24 kHz pitched up 3 semitones), #279's estimate of about 10 KB a note holds.

**RESAMPLE and PROCESS** write new sources through `write_source`. A file is named by its CRC, and the FAT layer has no rename, so an on-device writer needs the CRC before it opens the file. Renders are pure functions, so RESAMPLE renders twice: once for the CRC, once to write. Their spec decides, along with on-device dither.

**GRAIN, SPECTRAL and FORMANT** (#279) are further `TapeModel`s. GRAIN reuses STRETCH's grain clock and complementary fade.

## 9. Tests

All host-side (`just test`). Hardware waits for the ship flash.

**Decode and interpolation**
- `pack12`/`unpack12` round-trip every 12-bit value, and odd lengths pad (ADR 0087's test, shared).
- Hermite returns a constant and a linear ramp exactly at any fraction.
- **`hermite_response`:** a sine played at its own rate, at 0.05, 0.1 and 0.2 × the source rate, has its passband and first image within 0.5 dB of § 2's table. Its images sit at or under −69, −50 and −29 dB relative.
- **`reads_stay_in_row`:** proptest over random params, sources (1 to 100 000 frames, every `shape`) and a `held` shorter than the source. Every index the heads form is in `[0, frames)`, checked through a test `SourceView` that panics outside.

**Pitch, time and aliasing**
- **`plays_in_tune`:** a source of known period at root, played at C2–C6 at each rate, measures within 0.1 cent (zero-crossing estimator over 1 s). TRANSWAVE's cycle plays at the note's frequency.
- **`transwave_is_alias_free`:** a 16-frame saw transwave swept C1 → C8 with a pitch route. Energy that isn't a harmonic of the note stays under −60 dB relative at every level and in every level change. Above C8 the test asserts nothing.
- **`transwave_level_is_largest`:** at note-on the level is the largest whose step is ≤ 1, at every note, for `--top` 512 and 1 024. A ±1-semitone vibrato never changes it, and a ±7-semitone route does.
- **`vibrato_costs_no_level_heads`:** a transwave Sound with a shallow PITCH route bills 240, and one with a ±12-semitone route bills 450.
- **`stretch_keeps_pitch`:** at MOTION 0.5, 1 and 2, STRETCH's pitch is the note's within 1 cent, and a marked event in the source arrives at `t / MOTION` within one D-TIME.
- **`stretch_fades_are_complementary`:** the two gains sum to 1 at every sample while SIZE is swept 0 → 1 → 0 in 50 ms. MONO's splices of a sine are click-free by the rule below. POLY on white noise wobbles under 3 dB: the S950's tremolo, measured, not hidden.
- **`unpitched_mono_is_poly`:** a `period` 0 source renders bit-identically under MONO and POLY.

**Click-free.** `click_free_test`'s rule: no sample step over 4× the steady state's largest.
- **`init_never_hard_wraps`:** neutral TAPE on a sine whose length isn't a whole period, through 50 wraps.
- **`loops_are_click_free`:** LOOP, REVERSE and SCRUB at XFADE 0.1 and 0.5, with the loop at START and mid-window.
- **`ping_pong_clamps_with_a_fade`:** SIZE stepped 1 → 0.1 while the head is outside the new loop. Also PING-PONG's turns.
- **`sweeps_are_click_free`:** each modulatable param swept 0 → 1 → 0 in 100 ms, and stepped 0 ↔ 1, per model, on a sine source. TRANSWAVE's scan at MOTION ±1 shows no 750 Hz sideband above −80 dB.
- **`scrub_finger_is_bounded`:** POSITION stepped 0 → 1, and the head's speed never passes 16.
- **`source_switch_leaves_the_ringing_note`:** a Part changes SOURCE mid-note, and the voice's output is bit-identical to an unchanged run until its note-off.

**Model sweeps.** `param_sweep_test` gains `TapeEngineParams`: every param, per model, at 11 values, at notes 24–108. No NaN or infinity, a peak under +6 dBFS before the chain, and silence within 1 s of the VCA letting go.

**Goldens (ADR 0011).** One per model, plus AGE 0.5 and TAPE 0.5 on LOOP. Each is 2 s of C3, C4 and C5 on synthetic sources built in the test: a saw with a noise burst at 24 and 8 kHz, and a 4-frame saw-to-square transwave. They lock the refactor, not the sound.

**Memory, the pool and admission**
- `size_of::<TapeEngine>() < size_of::<ModalEngine>()`, and `[Voice; MAX_VOICES]` still fits D2 (ADR 0013), with the 448 B of `played` snapshots.
- A TAPE voice's latched id sets its `sounding` bit, and the bit clears on the block after the voice ends.
- **`gate_holds_tape_note_on`:** ADR 0087's gate test on a TAPE voice. Its first sample after the move equals the unmoved source's.
- Retire never cuts a TAPE note.
- **`missing_source_refused_before_allocation`:** a note-on with no source, a missing one, or TRANSWAVE on a `Plain` source steals no voice and changes no allocator state.
- A TAPE Sound's project round-trip carries its `SourceRef` and no audio. A missing source loads silent, reading `SOURCE MISSING`.
- **`zone_set_is_one_source`:** a hand-built 12-zone `ZoneSet` with 1 024-frame heads takes one `SourceId`, one bit and 18 576 B on the meter. Six fit beside NEW.
- **`tuning_lives_in_the_sound`:** a project whose source file has a different `root` label (re-imported) plays bit-identically. A zone set's `ZoneTune` does the same.

**Cost and UI**
- `tape_bills_each_model` (§ 5), including AGE routed at 0 and TRANSWAVE's pitch term. The bench rows are held high.
- `hint_fits` holds the LOADS and WON'T FIT hints within 216 px.
- Screen goldens (`docs/screens`): the strip for each model, and SRC on a pool row and a card row.
- `no_four_tapes`: no page short or matrix label but the engine's reads `TAPE`; the master stage reads `REEL`.

**Importer**
- Fuzz the WAV reader.
- A sweep above 0.35 × rate comes out at least 60 dB down.
- The CRC is deterministic across runs.
- `smpl` roots are read. Re-importing with another `--root` writes the same file (`Present`).
- A transwave's levels: 1 008 samples a frame by default, 2 032 with `--top 1024`.
- Each transwave level holds no harmonic above ⌊0.35 · N⌋.
- The `tapes-v1` manifest's CRCs are pinned.

**Desktop QA** (QA before UAT): every factory source through all six models, sweeping each macro. Per-pair levels against the factory median (ADR 0063), with `out_gain` set so TAPE INIT on the reference source sits there. Recordings go on a listening page for the owner's UAT.

## 10. Build order

Each step ships on its own.

**Depends on #337:**
- step 1, the delay as `i16` (ADR 0090), for the 160 KiB arena;
- steps 2–5, the sparse encoder, the linker assert, the arena and the meter;
- Task 9's source half: `SourceId`, the table, the gate, retirement, `Dir::Tapes`, `PoolSource`, `SourceRef`, `write_source` and sealing.

TAPE's steps 1–3 can be built on the host beside that, against a boxed arena.

1. **Names:** the master stage's on-screen word becomes REEL (§ 3), UI strings only, every golden but the screens bit-identical.
2. **The source format:** `SourceHead`'s fields, the descriptor, `SourceZones`, `SourceView`, `SourceCheck`, the Hermite read. Then the importer (`chimera-import tape`, `just tape`), plain and transwave, writing to the simulator's card.
3. **The engine with LOOP, and the bench:**
   - `EngineType::Tape`, `SlotKind::Tape`, `EngineSlot::Tape`, `EngineCtx`;
   - `TapeEngineParams` (block 34), with ROOT and PERIOD copied at pick, neutral and INIT;
   - latching, `sounding`, refusal before allocation;
   - the SRC, DECK and model pages (no strip yet);
   - the cost and its `cost_test` row;
   - LOOP's goldens and click tests;
   - **the bench rows of § 5, flashed and measured** (the first on-unit run, after #359 and #360 are on main, as the #337 plan requires). The other models' bills are then re-derived from the measured head before they are built.
4. **REVERSE, PING-PONG and SCRUB.**
5. **TRANSWAVE,** with its levels and their crossfade.
6. **STRETCH.**
7. **The TAPE and AGE macros.**
8. **The strip,** on SRC and DECK, and the head atomics.
9. **The source cell's card rows,** and MIX + PLUS LOADS.
10. **USB `put` for sources:** ADR 0092's extension under its own ADR, after `console-put` merges. Then `just put-tape`.
11. **The factory set:** the `tapes-v1` release, the factory TAPE Sounds in the card library (#317), and `just put-factory-tapes`.
12. **The ship flash:** the bench rows again on the full engine, then the owner's UAT.

**Flash,** estimated, and measured with `rust-size` at each step:
- the engine, its six models and the level select, about 5 KB;
- params, pages, the strip and the hint, about 4 KB;
- the source reader, descriptor and check, about 1 KB.

**About 10 KB,** from about 97 KB left after #337 (120 304 − 23 KB), leaving **about 87 KB.** If the #337 plan drops its on-device importer, add back its share.

**RAM:**
- **AXI, about 2 KB of the 36 KB margin:**
  - `TapeEngineParams` in each `ParamSnapshot`, about 56 B: six Parts and the triple buffer's copies, about 1 KB;
  - the strip's cache, 432 B;
  - the head atomics, 48 B;
  - the head cache, 512 B.
- **D2: 448 B,** for the eight `played` snapshots. The engine state fits the slot MODAL already sizes.
- **The arena:** the sources the user loads (§ 4).

## Owner questions

None open. The factory set's recordings were settled by the owner on 2026-10-05: openly licensed recordings and Chimera's renders now, and the owner's own recordings later as new `.SRC` files and Sounds in a later release, with no format change (§ 7).

Settled by precedent or the code, not asked:
- **#279's four macros are the home page, with SPACE,** as on MODAL's RES. SIZE is on the model page.
- **Mono sources** (the S950, the original Digitakt; stereo doubles the meter).
- **Hermite interpolation with a 0.35 × rate cut-off,** and folding on pitch-up (the SP-1200's character, bounded).
- **TRANSWAVE's octave levels** (PPG, Waldorf), top level 512 by default. The largest level whose step is ≤ 1; hysteresis at the alias limit (±6.2 st); level-change heads billed only where pitch can move that far.
- **A real-time STRETCH with one complementary fade per splice,** period-locked for MONO. WSOLA is kept as a costed follow-up.
- **Linear crossfades,** folded inside the loop when needed (an oscillator's loop is correlated).
- **No one-shot model:** KIT and DRUM are the one-shot path (#214, #215; #279's own note).
- **Loops are Sound parameters,** not source attributes (the Digitakt).
- **Root and period are labels in the file and identity in the Sound,** copied at pick. A zone set's go in `ZoneTune`.
- **TRON is one zone-set source an instrument,** with 35 ms heads (18.6 KB an instrument). Its files need a chunked `put`, under its own ADR.
- **The master stage keeps `BlockRef::Tape` and reads REEL on screen.** The engine is `BlockRef::TapeEngine`.
- **The import default stays 24 kHz** (#279). The factory set picks a rate per source.
