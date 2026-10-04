# TAPE core: 12-bit card sources played from the pool by six RAM models

Status: draft for owner review (2026-10-04). Issue: private issue #279. ADR 0093 (Proposed) records the decision. It builds on the sound pool budget (private issue #337, ADR 0087, ADR 0090), which is unmerged on `sound-pool-budget`. It needs the USB `put` of ADR 0092 (`console-put`) extended to sources. Billing follows ADR 0091 (`orbit-budget`). TRON, the streamed model, is the second sub-project, and RESAMPLE/PROCESS (MELLOW, SMEAR, AGE, neural: private issue #282) come after it. Neither is specified here; § Not blocking says what this core keeps open for them.

## Intent

TAPE is Chimera's third engine, beside ALGO (six-operator PM) and MODAL. Its samples are **raw oscillator material**, not recordings to reproduce faithfully. The owner's rulings:

- **One engine named TAPE.** MODEL picks the playback algorithm, and SOURCE picks the sample (#279, 2026-10-01).
- **Sources are stored S950-style:** 12-bit, with a sample rate per source. Every source lives on the card, write-once, under `/CHIMERA/TAPES`.
- **The pool's arena holds a whole short source,** or the first slice of a streamed one. Both count on the one meter (ADR 0087). Projects store only a `SourceRef`.
- **The RAM models in this core:** LOOP, TRANSWAVE, REVERSE, PING-PONG, SCRUB, and **STRETCH**, an S950-style time-stretch.
- **Sources come from WAV import plus a factory set,** which lives on the card only.
- **WAV import travels over USB:** the card is sealed inside the case.
- **CPU:** a voice cost and an admission row, as for every engine (ADR 0040, `cost_test`, ADR 0091).
- **Never snap a parameter, never allocate or block in audio, and never read the card in audio.**
- **Few controls:** every MIX or hold gesture gets a focus-band hint (ADR 0086). Each model's page shows what it does to the sound (product vision).

## Prior art, and what we take

| Instrument | What it does | What we take |
|---|---|---|
| **Akai S950** | 12-bit, rate chosen per sample (7.5–48 kHz). Its TIMESTRETCH is an offline edit that writes a **new sample**. You set a stretch percentage and **D-TIME**, the splice length: "Longer D-TIME values will give the sample a slight tremolo effect whilst shorter D-TIME values give rise to the metallic effect." AUTO-D picks a D-TIME "like autoloop". A type is chosen for "single tones" (MONO) or "polyphonic samples like backing vocals or drum loops" (POLY). (S950 Operator's Manual, p. 32.) | 12-bit storage with a rate per source. STRETCH's controls: SIZE is the D-TIME, and TYPE is MONO or POLY. We take the character (tremolo at long splices, metal at short ones), but **in real time** rather than as an offline edit. |
| **Elektron Digitakt (I and II)** | Per-track sample SLOT, STRT, LEN, LOOP POSITION, and play modes FORWARD, REVERSE, FORWARD LOOP and REVERSE LOOP. A loop runs "between Loop Position and Length". There is no ping-pong mode. Projects reference samples on the +Drive. (Digitakt II manual, § Playback modes.) | The window and loop as Sound parameters, not sample attributes. SOURCE is a Sound parameter that picks a project slot. REVERSE loops backwards. |
| **Elektron Octatrack** | **Flex machines** play from RAM, and **Static machines** stream from the card. Time-stretch is real time, with pitch and tempo independent. Projects keep a slot list that points into the card's audio pool. (Octatrack manual OS 1.25, § Machines, § Timestretch.) | RAM models now and a streamed model later (TRON), in one engine. A real-time stretch, with pitch and time on separate controls. |
| **Polyend Play / Tracker** | Sample types: one-shot, forward loop, backward loop, **ping-pong**, slice, **wavetable** (POSITION picks the window, WINDOW sets its length) and granular. One memory bar for the sample pool. (Tracker manual v1.5, § Instrument; Play manual, § Samples.) | PING-PONG as its own model. TRANSWAVE's frame and position. The one meter is ADR 0087's. |
| **Make Noise Morphagene** | **Vari-Speed** is "bipolar … speed and direction" of playback. **Slide** is "manual control for traveling through the captured sound and allows for scrubbing". **Gene-Size** shrinks the playback window into grains. **Organize** walks the splices. (Morphagene manual.) | SCRUB: a head with inertia under POSITION (Slide), at a bipolar MOTION (Vari-Speed). MOTION on LOOP walks the loop through the window (Organize). GRAIN, its own model, is later (#279). |
| **Ensoniq VFX/SD-1 Transwaves** | A long sample read as a series of single-cycle frames, with the frame chosen live. | TRANSWAVE's name and its law. |

Sources:
- Akai S950 Operator's Manual, p. 32 (TIMESTRETCH): https://www.manualslib.com/manual/629018/Akai-S950.html?page=32
- Akai-style real-time two-grain stretch (JUCE forum, 2024): https://forum.juce.com/t/a-lightweight-akai-style-time-stretch-algorithm-realtime/60514
- Digitakt II manual: https://www.manualslib.com/manual/3436437/Elektron-Digitakt-Ii.html
- Octatrack manual OS 1.25: http://www.elektron.co.jp/wp-content/uploads/2015/01/octatrack_manual_OS1.25.pdf
- Polyend Tracker manual v1.5: https://files.kraftmusic.com/media/ownersmanual/Polyend_Tracker_Manual_v1_5.pdf
- Morphagene manual: https://www.makenoisemusic.com/wp-content/uploads/2024/03/morphagene-manual.pdf
- O. Niemitalo, "Polynomial Interpolators for High-Quality Resampling of Oversampled Audio" (2001): the 4-point, 3rd-order Hermite.

## What exists (main at cf50ee74, plus the unmerged branches it builds on)

- **The engine slot (ADR 0051):** `EngineSlot` holds one engine, rebuilt in place. Adding an engine means one variant there; every `match` lists the arms. `SlotKind::of` reads `p.engine()`.
- **Voice chain (`dsp/voice.rs`):** engine → drive → filter → folder → DC blocker → VCA, with three ENVs and three LFOs. A voice lives while its VCA routes hold it (`vca_holds`). `Voice::cost` is `EngineSlot::cost` + `CHAIN_COST` (30) + the filter's delta over LP24 + `ModRouting::cost` (47 at its floor) + the stages.
- **The budget (`hw.rs`, ADR 0091):** 7 000 cycles/sample on rev V and 5 833 on rev Y, at 70 %. `Instrument::RESERVED` = bus 1 180 + note-on burst 836 = 2 016 (2 326 with the master tape). ORBIT adds 171 while it plays.
- **Engine costs today:** ALGO 370 base + 60 an operator + 8 a link. MODAL is 330 for STRING, 540 for SYMP, 640 for BOWED and 460 + 45·modes for BANK.
- **The name TAPE is taken in code** by the master tape stage: `BlockRef::Tape` (disk code 23, ident `"TAPE"`), `dsp/tape/` and ADR 0055. The engine therefore takes **`EngineType::Tape`** (engine disk code 2, ident `"TAPE"`, which is the engine namespace, not the block one), and its block and module are named **`Reel`**: `BlockRef::Reel` (block code 34, ident `"REEL"`), `dsp/reel/`, `ReelEngine`, `ReelParams`. The user only ever reads TAPE. Nothing is renamed.
- **The pool (ADR 0087, unmerged):** a 160 KiB arena in AXI. NEW's 32 Sounds leave **153 600 B for sources**, which is 4.3 s at 24 kHz or 6.4 s at 16 kHz. It also defines `SourceId`, `SourceRef { crc, frames, rate }`, the `SourceTable` (rows `base`, `held`, `rate`; `gate`; `sounding`), retirement, compaction under a gate, 12-bit packing (two samples in three bytes), and `/CHIMERA/TAPES` files named by CRC.
- **The card:** measured on the owner's unit (`card` on the console, cf50ee74): 2.3 MB/s sequential, and 5.4 ms p95 for a random 4 KB read, on a 16 GB card. That is about 10 streams at 24 kHz/12-bit with margin. Under load the UI loop runs 22–26 ms a pass (stress test, 2026-10-02).
- **USB `put` (ADR 0092, unmerged):** project files only, at most 1 MiB, written through an A/B save, with a 30 s deadline.

## 1. Sources

### The source file

ADR 0087's `Source` file (`FileKind` 6) is unchanged, except for one field. **`SourceHead` (0x8017) gains `root: u16`**, the source's pitch in MIDI cents (0–12 700; 6 000 is C4). The payload becomes the `SourceRef` (10 B), the root (2 B) and the 16 B name: 28 B. ADR 0087 is Proposed and its records aren't built yet, so this is an amendment, not a migration.

- **The CRC covers the root, then the packed bytes.** A source's identity, and so its file name (`1A2B3C4D.A`), is its tuning plus its audio. The same audio with another root is another source, and `SourceRef` stays `{ crc, frames, rate }`.
- **Mono only.** The S950 and the original Digitakt are mono. A stereo source would double every cost on the meter, and voices are panned by their Part.
- **Loops are not attributes of a source.** They are Sound parameters, as on the Digitakt, so one source serves many Sounds. A WAV's `smpl` loop is printed by the importer as a suggested POS and SIZE (§ 6).

### The audio's view

ADR 0087's `SourceRow` keeps its 12 B. Its `rate: AtomicU32` carries **the rate in the low 16 bits and the root in the high 16**: one load, published with the row. A voice reads `base`, `held` and `rate_root` at the start of each block, as ADR 0087 has it.

## 2. The engine

### Structure

```rust
pub enum EngineType { Algo = 0, Modal = 1, Tape = 2 }
pub enum SlotKind   { Algo, Modal(ResonatorMode), Tape }   // one kind: every model has the same state
in_place_enum! { pub enum EngineSlot { Algo(AlgoEngine), Modal(ModalEngine), Tape(ReelEngine) } }

pub enum ReelModel { Loop, Reverse, PingPong, Transwave, Scrub, Stretch }   // disk codes 0..5; TRON takes 6

/// What a voice reads a source through: one block's copy of its row.
pub struct SourceView<'a> {
    bytes: &'a [u8],     // exactly `held` bytes from `base`
    frames: u32,         // frames those bytes hold: ⌊held / 3⌋ · 2
    rate: u16,
    root: u16,           // MIDI cents
}

pub struct ReelEngine {
    src: Option<SourceId>,   // latched at note-on, never re-resolved
    model: ReelModel,        // latched at note-on
    heads: [Head; 2],        // a 32.32 fixed-point frame position and a direction each
    inc: Ramp,               // frames per output sample, ramped across the block
    mac: Macros,             // POS, SIZE, MOTION, TAPE, AGE, eased (as MODAL's `Macros`)
    bounds: Bounds,          // the loop or window, latched at each wrap
    grain: GrainClock,       // STRETCH: the time pointer and the splice phase
    wow: Transport,          // ADR 0053's transport, run once a block
    age: Age,                // one-pole, hiss xorshift, dropout gain
    declick: u8,             // samples left of the note-on ramp
}
```

- **The voice calls TAPE as it calls the others,** with a context in place of today's bare `&mut SymPool`: `EngineCtx { sym: &mut SymPool, src: &SourceTableView }`. ALGO and MODAL ignore `src`.
- **`ReelEngine` is about 160 B** (two 16 B heads, the ramps, five eased macros, `Transport`'s 20 B, the age state). It sits in an `EngineSlot` sized by `ModalEngine`, so **D2 grows by nothing**. A test pins `size_of::<ReelEngine>() < size_of::<ModalEngine>()`.
- **`is_active`** is true from note-on until the voice ends. A loop has no end, so the note ends when its VCA routes let go (`vca_holds`), as a Modal note does. TAPE's INIT routes ENV 1 into VCA. A note-on on an unresolved source (`SOURCE MISSING`, or no source picked) leaves the engine inactive, so the voice frees at once and plays nothing.

### Reading a source

**Position and pitch.** A head holds its position as 32.32 fixed-point frames, which is exact over any source the arena can hold. It is advanced by `inc` each sample:

```
inc = rate / 48 000 · 2^((note·100 + pitch_cents − root) / 1200)
```

`pitch_cents` is the PIT page's PITCH and FINE with their routes (ADR 0042). `inc` is recomputed once a block (one `exp2f`, about 2 cycles a sample spread over 64) and ramped linearly across the block, so a pitch route or a glide never steps. STEAL GLIDE (ADR 0065) glides `inc`. TRANSWAVE ignores `root` (§ TRANSWAVE). `inc` is clamped to 16 (four octaves over the source's own speed at 48 kHz). The clamp only bounds the read's reach, since the cost doesn't depend on `inc`.

**12-bit decode.** A sample `i` is in pair `i >> 1`, at byte `3·(i >> 1)`. Even samples are `b0 | (b1 & 0xF) << 8`, odd ones `b1 >> 4 | b2 << 4`. Each is sign-extended with `(x << 20) >> 20` on an `i32`. The `1/2048` scale is folded into the interpolator's weights. Four consecutive samples span two pairs (6 bytes) from an even index, or three (9 bytes) from an odd one. They take three unaligned word loads (free on the M7) and about 16 shift-and-mask instructions.

**Interpolation: 4-point, 3rd-order Hermite** (Niemitalo's "x-form"), about 12 floating-point operations a sample. The choice:
- **Linear** costs about 6 fewer operations, but a source at 8–16 kHz is upsampled 3–6× to 48 kHz. Linear leaves images about 30 dB down, which buzz over a dark source.
- **Hermite's images** are some 15 dB lower, and it passes DC and ramps exactly.
- **The S950** replayed through per-voice clocks and analogue filters, which is smooth, not drop-sample. Hermite is the nearer digital answer.

**No anti-aliasing on pitch-up, by design.** The importer band-limits every source to 0.45 × its rate (§ 6). A source then aliases only when a note plays it above `48 000 / (0.9 · rate)` times its own speed: more than 13.8 semitones over its root at 24 kHz, or 32.8 at 8 kHz. Lower-rate sources alias less, as on the S950 and the SP-1200. It is the character, and the voice filter follows.

**Never outside the row.** Every index the heads read is `min(i, frames − 1)` (and `max(i, 0)` going back), branch-free, from the view's own `frames`. So no read leaves the `held` bytes the row publishes. That is the `// SAFETY:` argument `SourceReader` needs (ADR 0087). If `held` is less than the whole source (a TRON source's head, later), the models play inside what is held.

**The note-on ramp.** A head's first 48 samples (1 ms) ramp from 0, so a START inside a waveform never steps. That is below any audible attack; the ENVs shape the rest.

### The window, and the macros

Every model plays inside the **window** `[START, END]`, as fractions of the source (SRC page). `W` is its length in frames. Four macros are read every block from the voice's modulated params, and eased `EASE` = 0.3 of the way each block, as MODAL's are (ADR 0056). A note's first block takes them whole.

| | **POS** | **SIZE** | **MOTION** (bipolar, 0 still) | **TAPE** | **AGE** |
|---|---|---|---|---|---|
| **LOOP** | where the loop sits in the window | the loop's length, 2 ms to `W`, exponential | **walk**: the loop's place drifts through the window, up to ±1 window a second, wrapping | wow and flutter | wear |
| **REVERSE** | as LOOP | as LOOP | as LOOP | as LOOP | as LOOP |
| **PING-PONG** | as LOOP | as LOOP | as LOOP | as LOOP | as LOOP |
| **TRANSWAVE** | the frame, fractional | the scan's width, in frames | the scan's rate, 0 to ±8 Hz, a triangle around POS | as LOOP | as LOOP |
| **SCRUB** | the **finger**: where the tape is held | the window the head loops behind the finger, 2 ms to `W` | **vari-speed**: −2 to +2 times the note's speed | as LOOP | as LOOP |
| **STRETCH** | where time starts | **D-TIME**: the splice length, 10–250 ms, exponential | the **speed of time**: −1 to +2, 0 freezes, 1 is the source's own | as LOOP | as LOOP |

- **TAPE** is the transport's wow and flutter: ADR 0053's `Transport`, run **once a block** on the head's speed (in cents), ramped into `inc`. Depth runs 0 to ±25 cents of wow and ±8 of flutter. Each voice has its own transport and phase, because a Mellotron gives every note its own strip of tape. At 0 it adds exact zeros.
- **AGE** is the wear: a one-pole low-pass whose corner falls from 20 kHz to 2.5 kHz, hiss rising from silence to −42 dBFS, and above 0.5, dropouts: a gain dipping up to 9 dB, at random, for 20–80 ms, eased per block. At 0 it is bypassed and unbilled, as MODAL's BODY is.
- **The loop's bounds are latched at each wrap.** The loop is `[Ls, Ls + Ll)` with `Ll` from SIZE and `Ls = START + (POS + walk) · (W − Ll)`. The head finishes its pass on the bounds it started with, then takes the eased new ones as it wraps. A moved POS or SIZE therefore never cuts a pass short, and the wrap's crossfade covers the step. This is how the Digitakt's loop behaves.

### The models

**LOOP.** The head starts at START, plays forward to the loop's end, then loops `[Ls, Ls + Ll)`.
- **The crossfade (XFADE, 0 to ½ of the loop):** over the loop's last `Lx` frames, a second head reads the same distance before the loop's start, and the output fades linearly from the first to the second. At the wrap the second head becomes the first.
- **Linear, equal gain,** is right for a loop of correlated material (a tone's period), which is what an oscillator loop is. `Lx` is clamped to the material before `Ls`, which may lie outside the window but never outside the source.
- **Cost:** two heads while crossfading, one otherwise; billed at two.

**REVERSE.** LOOP mirrored: the head starts at END and runs backward through the loop. Its crossfade reads the material after the loop. It is the Digitakt's REVERSE LOOP.

**PING-PONG.** The head reflects at each edge of the loop: `h = 2·edge − h`, and its direction flips. The value is continuous at a turn (only the slope changes), so it needs no crossfade, and XFADE dims. One head.

**TRANSWAVE.** The window is cut into frames of FRAME samples (256, 512, 1 024 or 2 048; latched at note-on). Frame `k` covers `[START + k·F, START + (k+1)·F)`.
- The frame position `p = POS · (n − 1) + SIZE · (n − 1) · tri(MOTION's phase)`, clamped to `[0, n − 1]`.
- **Two heads** at the same phase `φ` read frames `⌊p⌋` and `⌊p⌋ + 1`, and the output is their lerp by `p`'s fraction.
- `φ` advances `F · f_note / 48 000` a sample, so **a frame is one cycle at the note's pitch**, and the source's rate and root don't enter. A frame's wrap is periodic, so a frame not cut on its own cycle adds harmonics, never a click. This is the Ensoniq transwave, and the Tracker's wavetable mode with POSITION and WINDOW.
- Any source works. A 2 048-sample frame of choir becomes a buzzing, morphing cycle, which is the point.

**SCRUB.** The finger `F` follows POS through a one-pole of DRAG (10–500 ms: the tape's inertia). It is computed once a block and ramped.
- The head is `F + l`. `l` advances `MOTION · inc` a sample and wraps in `[0, Ll)` with LOOP's crossfade. The head is never let run off the window.
- **Moving the finger drags the head with it,** so a moving POS bends the pitch by the finger's speed, as a hand on a reel does.
- **At MOTION 0 and a still finger** the head stands still, and the voice's DC blocker (ADR 0060) takes the held value to silence. Sound comes only while the finger moves: a POS route from an LFO is a scratch.
- Two heads, billed as LOOP's.

**STRETCH.** The S950's time-stretch, run in real time. It splices two overlapping grains (a two-splice overlap-add).
- **The time pointer** `T` moves through the window at `MOTION · rate / 48 000` frames a sample, wrapping in it. At MOTION 1 the source keeps its duration, at 0.5 it takes twice as long, at 0 it freezes, and below 0 time runs backward while the grains still play forward.
- **The grains:** every `L/2` output samples (`L` from SIZE, the D-TIME), a new grain starts at `T`. Its head reads at `inc`, so the pitch is the note's, whatever MOTION is. Each grain lasts `L` under a triangular window. Two triangles half a grain apart sum to exactly 1, so a steady tone keeps a steady level.
- **TYPE MONO** (the S950's single-tone type) starts each grain **in phase with the grain it overlaps**. With the source's period `P = rate · 2^(−root/1200) / 8.1758` frames, the new grain starts at `T' = h_old − P · round((h_old − T) / P)`: as near `T` as possible, a whole number of periods from the old head. A held tone's splices then line up, and only the S950's tremolo at long D-TIMEs remains. That is one division a grain. **POLY** starts at `T` itself, for chords, loops and noise, and keeps the metal and tremolo that make the S950's stretch recognisable.
- **Why not WSOLA.** WSOLA searches each splice for the lag of best correlation: 64 lags over 128 samples is about 8 000 multiply-adds at each grain start. Averaged, that is only a few cycles a sample, but it lands in single blocks: 8 000 cycles a voice in one 1.33 ms block is about 125 cycles a sample of that block. Admission has to bill the worst block, so WSOLA-lite would cost every STRETCH voice about 125 more. Period-locking gets the same in-phase splice for pitched sources at no cost, and pitched sources are what an oscillator plays. For the rest, the S950 had no search either.
- Two heads and two window weights, always.

### Parameters never snap

- The macros are eased per block.
- `inc` and the transport are ramped per sample.
- The loop's bounds change only at a wrap, under a crossfade.
- SCRUB's finger is slewed.
- STRETCH's grains take new settings only at a splice, under the window.
- MODEL, FRAME and TYPE are latched at note-on, as MODAL's MODEL and MODES are, so a sounding note never changes under them.
- SOURCE is latched at note-on too: a Part changing SOURCE leaves its ringing notes on the old one (ADR 0087).
- START is read at note-on. END enters the loop's bounds, so it changes at a wrap.

## 3. Parameters, pages and the chain

**`ReelParams`** (block `Reel`, code 34; ids frozen once written):

| Id | Param | Range, law | Read | Mod |
|---|---|---|---|---|
| 0 | MODEL | LOOP, REVERSE, PING-PONG, TRANSWAVE, SCRUB, STRETCH | note-on | no |
| 1 | START | 0–1 of the source | note-on | no |
| 2 | END | 0–1, kept ≥ START + 2 ms | each wrap | no |
| 3 | POS | 0–1 | block, eased | **yes** |
| 4 | SIZE | 0–1, exponential per model | block, eased | **yes** |
| 5 | MOTION | −1–+1, per model | block, eased | **yes** |
| 6 | TAPE | 0–1 | block, eased | **yes** |
| 7 | AGE | 0–1 | block, eased | **yes** |
| 8 | XFADE | 0–0.5 of the loop | each wrap | no |
| 9 | FRAME | 256, 512, 1 024, 2 048 | note-on | no |
| 10 | DRAG | 10–500 ms, exponential | block | no |
| 11 | TYPE | MONO, POLY | note-on | no |

SOURCE is not a param. It is the Sound's `SourceRef` record (0x0016, ADR 0087), and its cell is a `ParamSlot::source()`, as MODAL's panel cells are `modal_panel`. Neutral (`Sound::neutral(Tape)`, a format contract under ADR 0087) is LOOP, START 0, END 1, POS 0, SIZE 1, MOTION 0, TAPE 0, AGE 0, XFADE 0.1, FRAME 2 048, DRAG 0.3, TYPE MONO.

**The chain:** `SRC · TAPE · DRV · FLT · AMP · MOD`, which is ALGO's shape with MODAL's first node. A Part's TAPE chain opens on TAPE, MODAL's home rule (ADR 0066).

| Page | Short | Cells | Viz |
|---|---|---|---|
| Source (id 92) | SRC | SOURCE, START, END, VOL, –, – | the strip |
| Tape, home (id 93) | TAPE | MODEL, POS, SIZE, MOTION, TAPE, AGE | the strip |
| The model page (id 94), named for the model: LOOP, REV, PING, WAVE, SCRUB, STRCH | as named | LOOP and REV: XFADE · PING: XFADE, dimmed · WAVE: FRAME · SCRUB: XFADE, DRAG · STRCH: TYPE | none |
| PIT (ADR 0042), a sub-page of TAPE | PIT | PITCH, FINE, STEAL, GLIDE TIME | none |

Block ids 92–94 follow main's highest (91). They are checked against the other branches when built.

- **SPACE leaves the home page.** MODAL's RES puts SPACE (the Part's reverb send) last. TAPE's home needs all six cells for MODEL and the five macros (POS, SIZE, MOTION, TAPE, AGE). SPACE stays one step away on SENDS. This is owner question 1.
- **A control the model ignores is dimmed,** with its matrix column, from one `reel::reads` table, as MODAL's `modal::reads` does. PING-PONG dims XFADE, TRANSWAVE dims nothing on the home page, and STRETCH dims XFADE.
- **The strip** is how each page shows what it does to the sound. It is a 216 × 40 px view of the window, drawn as the source's min/max per column in `MID`, with START and END marks. Over it, per model:
  - **LOOP and REVERSE:** the loop shaded in `ACCENT_DIM`, its crossfade a ramp, the head a line in `ACCENT` with an arrow for direction. MOTION's walk moves the shading.
  - **PING-PONG:** the loop with `◂▸` at its edges, and the head bouncing.
  - **TRANSWAVE:** frame ticks along the window, the current two frames lit by weight, SIZE's scan as a bracket, and a single-cycle scope of the blend at the right end, ADR 0070's WAVE scope.
  - **SCRUB:** the finger as a ▾ mark with a fading trail, the window behind it, and the head.
  - **STRETCH:** the time pointer as a line, and the two grains as triangles under the waveform, their width the D-TIME, their height their weight now.

  The overview is computed by the UI from the arena when the page opens or SOURCE changes. A 100 000-frame source takes under 1 ms, and the result is a 432 B cache for the one page shown. The heads come from the audio: one `AtomicU64` per Part, holding the newest voice's head and grain positions and stored `Relaxed` once a block. That is 48 B of AXI.
- **SOURCE, the cell.** Turning it steps through the pool's sources first: switching among them is free and instant. A divider follows, then the card's `/CHIMERA/TAPES` files that are not in the pool, dimmed, each with its cost.
  - The focus band reads the name, rate, length and cost (`CHOIR · 24K · 1.2 S · 41K`), and the meter previews a card row (ADR 0087).
  - The directory listing gives stems and sizes, and so costs, with no extra read. Names are read on demand into a 16-entry head cache, one 4 KB read each (5.4 ms p95).
  - This is the Digitakt's SAMPLE SLOT, turned in place, with the Octatrack's audio pool behind it.
- **Gestures (ADR 0086).** There is one new gesture, with its hint:

| Where | Touched | Hint |
|---|---|---|
| SRC | SOURCE on a card row | `MIX + PLUS LOADS · 41K`, or `WON'T FIT · NEEDS 41K, 12K FREE` in `ALERT` |

  MIX + PLUS adds, as it primes a route in the matrix. The load reads 4 KiB a UI pass behind `ui::busy`, and MENU cancels it (ADR 0087 § Failure modes). Once loaded, the row moves above the divider and the Part plays it. Nothing else in TAPE is a MIX or hold gesture. PURGE and the meter's leaf are ADR 0087's.

## 4. With the pool (#337)

- **A source is loaded by the UI only:** from a project load (pass 2), a library Sound, or MIX + PLUS. Its bytes are staged in the gap and CRC-checked, then indexed. Its row is then published with `Release` (ADR 0087). **Every source in this core is held whole**, because all six models are random-access.
- **A Part resolves its Sound's `SourceRef` to a `SourceId`** when it loads, and `AudioShared` carries the id (1 B a Part).
- **A voice latches the `SourceId` at note-on.** It never re-resolves the Part's ref, and it reads its row's `base`, `held` and `rate_root` at the start of each block. It never keeps a pointer across blocks.
- **The `sounding` mask** is the OR of every active TAPE voice's latched id, stored by the audio once a block. It is what holds compaction off (ADR 0087).
- **No bytes move under a sounding voice.** Compaction waits for silence, then closes the gate, acknowledges, re-checks and moves (ADR 0087). A note-on on a moving source during a closed gate is allocated and latched, and starts on the first block after the gate opens: at most 2.7 ms late.
- **A deleted or purged source still sounding is retired, never cut.** Its row stays live until its bit clears.
- **The meter** counts each source at `16 + held`, rounded to 16 B, in `MID`. TAPE adds nothing to it.
- **What fits** in the 153 600 B NEW leaves:

| Source | 12-bit bytes | Meter (with NEW's 7 %) |
|---|---|---|
| 1 s at 24 kHz | 36 000 | 29 % |
| 1 s at 16 kHz | 24 000 | 21 % |
| a 16-frame × 2 048 transwave at 48 kHz (0.68 s) | 49 152 | 37 % |
| six Parts, each on its own 1 s source, at 24 kHz | 216 000 | **won't fit** |
| the same at 16 kHz | 144 000 | 95 % |
| the same at 24 kHz, 0.7 s each | 151 200 | 99 % |

  The rate per source is the S950's trade, quality against time, and the factory set uses it (§ 7). 24 kHz stays the import default (#279).

## 5. CPU

Host estimates, in the house manner (instructions × 1.46 × 1.1, as `ModalEngine`'s `COST_*`), to be replaced by bench rows at the ship flash:

| Term | Instructions a sample |
|---|---|
| One head: 32.32 advance, index, clamp (4); decode four 12-bit samples (19); Hermite (12); wrap test (3) | **≈ 38** |
| A crossfade or window weight, and the mix | 4 |
| Block work spread over 64: `inc`'s `exp2f`, the transport, the macros' easing, the row read | ≈ 4 |
| The note-on ramp | 1 |

| Model | Heads | Instructions | Cycles | **Billed** |
|---|---|---|---|---|
| PING-PONG | 1 | 47 | 75 | **100** |
| LOOP, REVERSE | 2 | 89 | 143 | **170** |
| TRANSWAVE | 2 (+ the scan, 2) | 91 | 146 | **170** |
| SCRUB | 2 (+ the finger's ramp, 2) | 91 | 146 | **170** |
| STRETCH | 2, two windows (+ the grain clock, 3) | 96 | 154 | **180** |
| AGE above 0, on any model | (one-pole 3, hiss 5, gain 2) | 10 | 16 | **+20** |

The billed figures carry room for the AXI loads' cache misses: eight voices reading two heads each, mostly forward, from a 16 KB D-cache. `ReelEngine::cost(&ReelParams)` returns the model's figure plus AGE's, and `playing_cost` returns the latched model's, as MODAL's does. `glide_cost` is zero, because `inc` is recomputed every block anyway.

**The voice and its admission.** The worst TAPE engine is 200 (STRETCH with AGE). The voice is 200 + 30 (chain) + 47 (`ModRouting::BASE`) = **277** at LP24, with no DRIVE or FOLD:

| | rev V | rev Y | rev V, ORBIT playing | rev V, master tape |
|---|---|---|---|---|
| Room after the reserve | 7 000 − 2 016 = 4 984 | 5 833 − 2 016 = 3 817 | 4 984 − 171 = 4 813 | 7 000 − 2 326 = 4 674 |
| TAPE voices (277) | 17 → **8** | 13 → **8** | 17 → **8** | 16 → **8** |
| With DRIVE and FOLD (+102, 379) | 13 → **8** | 10 → **8** | 12 → **8** | 12 → **8** |
| With the worst routing (633 in place of 47: 863) | **5** | **4** | **5** | **5** |

The eight-voice pool binds before the budget for every plain TAPE Sound, as it does for a one-operator ALGO patch. The routing-heavy case matches the heaviest ALGO.

**The `cost_test` row**, `tape_bills_each_model`, after `modal_bills_each_model`: for each model, with AGE 0 and 0.5, it asserts `EngineSlot::cost` equals the billed literal, and the voices admitted on rev V, rev Y and with the master tape, beside `Instrument::RESERVED` (ADR 0091). Every one is 8. `budget_capacity_per_engine` iterates `EngineType::ALL` and takes TAPE with no change. The bench (`--features bench`) gains `TAPE LOOP /VOICE`, `TAPE STRETCH /VOICE` and `TAPE 8 VOICES` rows, and `the_model_bills_every_bench_row_high` holds them.

## 6. WAV import

**Host first,** as ADR 0088's patch importer is. The firmware links no importer: no dither, no decimator, no WAV parser. ADR 0087's flash estimate for sources drops by those (about 3 KB). On-device conversion arrives with RESAMPLE.

`just tape <file.wav> [--rate 24k] [--root C3|auto] [--name CHOIR] [--channel mix|L|R]` runs `chimera-import tape` (in `chimera-desktop`, beside ADR 0088's binary):

1. **Read** PCM 8/16/24/32-bit or float WAV with `hound` (MIT/Apache-2.0, host only). Stereo is mixed `(L + R)/2` unless `--channel` picks one side.
2. **Clean.** A 5 Hz high-pass takes out DC. Leading and trailing silence under −60 dBFS is trimmed. The peak is normalised to −0.5 dBFS, so the 12 bits are used whole; the Sound sets the level.
3. **Root.** In order: `--root`; the `smpl` chunk's unity note and pitch fraction; a YIN estimate over the loudest 100 ms, kept if its aperiodicity is under 0.2. Otherwise C4, and the log says `UNPITCHED`.
4. **Resample** to the chosen rate (48, 32, 24, 16, 12 or 8 kHz; 24 by default, #279) with `rubato`'s windowed-sinc resampler (MIT, host only), its cut-off at **0.45 × rate**, attenuating 80 dB above 0.5 × rate.
5. **Quantise** to 12 bits with TPDF dither and pack two samples in three bytes. This is `chimera-core`'s own `pack12`, so host and unit share the bytes.
6. **Write** the `Source` file through `chimera-core`'s `SourceWriter`, the one writer RESAMPLE and PROCESS will use. Its CRC runs over the root and the bytes, and its name is that CRC. The file lands in the desktop simulator's card directory (`DirStore`), so QA plays it before the unit does.
7. **Report:** the name, rate, frames, seconds, root, the cost on the meter, any `smpl` loop as a suggested POS/SIZE, and a warning if it won't fit beside NEW (over 153 600 B).
8. **Send:** `just put-tape <file>` runs `chimera-usb.py put <file> /CHIMERA/TAPES/`.

A file is capped at 1 MiB, the `put` limit: 29 s at 24 kHz, 87 s at 8 kHz. RAM models can only use what fits the arena. Longer files are for TRON.

**What TAPE needs from the USB `put` (ADR 0092), as a dependency.** These are built as its extension under its own ADR (ADR 0092 § Consequences says so), not here:

- **A second path class:** `/CHIMERA/TAPES/XXXXXXXX.A`, eight hex digits, any case. `ls` takes `tapes`.
- **A streaming `SourceCheck`** in place of `ProjectCheck`: the header's kind is `Source`; the `SourceHead`'s frames match the data; the CRC over root and data equals the stem. It refuses before writing, as a project's header is refused now.
- **Write-once, not A/B.**
  - An existing file that verifies answers `OK EXISTS` and writes nothing.
  - An existing file that fails its check (a torn write from a power cut) is deleted and written again.
  - A failed `put` deletes what it wrote. A file named by its CRC can't shadow a good one.
- **The limit stays 1 MiB.** The deadline scales with the size: 30 s plus 30 s a MiB, measured on the unit when built.

## 7. The factory set

- **Card only** (ADR 0087: 24 KB a second at 16 kHz against about 77 KB of flash left). It is installed with `just put-factory-tapes`, which puts each file. Re-running it is safe, because a file that is already there answers `OK EXISTS`.
- **About 24 sources, about 1 MB on the card,** across #279's list: piano, choir, flute, strings, guitar, voice, organ, metal, glass, noise, field. There are also four transwave tables.
- **The rate is chosen per source, as on the S950,** so a six-Part project fits:
  - 32 kHz for bright material (glass, metal);
  - 16 kHz for warm material (choir, flute, strings, organ, voice);
  - 12 kHz for noise and field recordings;
  - 48 kHz for the transwave tables, whose frames must keep their top.

  Most sources are 0.5–1.5 s: 12–36 KB each.
- **Where the audio comes from,** under #282's dataset rule (Chimera's own renders, the owner's recordings, clearly licensed audio; never commercial records):
  1. **Chimera's own renders.** ALGO and MODAL patches are rendered offline by the desktop simulator. The recipes, a patch and a note list, are in the repo, and `chimera-import factory` renders them. Their CRCs are pinned in a test, so the factory sources are reproducible bit for bit, and no audio is committed.
  2. **CC0 recordings** (Freesound CC0), each with its licence logged in `tools/factory-tapes/SOURCES.md`. The audio stays out of the repo, as raw banks do (factory library method).
  3. **The owner's own recordings,** if they supply them (owner question 2).
- **Each factory source has at least one factory TAPE Sound** in the card library (#317), referencing it by `SourceRef`. NEW's 32 stay ALGO and MODAL (ADR 0087).

## 8. Not blocking what comes next

- **TRON.** `ReelModel` leaves code 6 free. A source's `held` is the most its users need:
  - all of it for a RAM model;
  - the 100 ms head for TRON.

  A Part switching a source from TRON to a RAM model is then a pool add, which can be refused, and TRON's spec states it. `SourceView` already plays inside what is held. Refill stays in the UI loop. TRON's rings go in D2 (121.5 KB free) and must cover the slowest UI pass under load (26 ms) plus a card stall. At 43 KB/s a note (24 kHz pitched up 3 semitones), the #279 estimate of about 10 KB a note holds. TAPE takes no D2.
- **RESAMPLE and PROCESS** write new sources through `SourceWriter`. A file is named by its CRC, and the FAT layer has no rename, so an on-device writer needs the CRC before it opens the file. The renders are pure functions, so RESAMPLE renders twice: once for the CRC, once to write. The alternative is a FAT rename. Their spec decides.
- **GRAIN, SPECTRAL and FORMANT** (#279) are further `ReelModel`s. GRAIN reuses STRETCH's grain clock.

## 9. Tests

All host-side (`just test`). Hardware waits for the ship flash.

**Decode and interpolation**
- `pack12` and `unpack12` round-trip every 12-bit value, and odd lengths pad (ADR 0087's test, shared).
- Hermite returns a constant and a linear ramp exactly, at any fraction. A sine at 0.1 × the source rate, played at root, has its images under −70 dBFS.
- `reads_stay_in_row`: proptest over random params, sources (1 frame to 100 000) and `held` shorter than the source. Every index the heads form is in `[0, frames)`, checked through a test `SourceView` that panics outside.

**Pitch and time**
- `plays_in_tune`: a source of known period at root, played at C2–C6 on each rate, measures within 0.1 cent (zero-crossing estimator over 1 s). TRANSWAVE's frame plays at the note's frequency.
- `stretch_keeps_pitch`: at MOTION 0.5, 1 and 2, STRETCH's pitch is the note's within 1 cent, and a marked event in the source arrives at `t / MOTION` within one D-TIME.
- `stretch_window_sums_to_one`: the two triangles sum to 1 at every sample. MONO's splice of a sine is click-free by the rule below. POLY's level on white noise wobbles under 3 dB, the S950's tremolo, measured, not hidden.

**Click-free.** These use `click_free_test`'s rule: no sample step over 4× the steady state's largest.
- `loops_are_click_free`: LOOP, REVERSE and SCRUB at XFADE 0.1 over a sine whose loop isn't cut on a period, through 50 wraps. PING-PONG's turns.
- `sweeps_are_click_free`: each modulatable macro swept 0 → 1 → 0 in 100 ms and stepped 0 ↔ 1, per model, on a sine source.
- `source_switch_leaves_the_ringing_note`: a Part changes SOURCE mid-note, and the voice's output is bit-identical to an unchanged run until its note-off.

**Model sweeps.** `param_sweep_test` gains `ReelParams`: every param, per model, at 11 values, at notes 24–108. No NaN or infinity, a peak under +6 dBFS before the chain, and silence within 1 s of the VCA letting go.

**Goldens (ADR 0011).** One per model, plus AGE 0.5 and TAPE 0.5 on LOOP. Each is 2 s of C3, C4 and C5 on a synthetic source built in the test: a saw with a noise burst, at 24 and 8 kHz. They lock the refactor, not the sound.

**Memory and the pool**
- `size_of::<ReelEngine>() < size_of::<ModalEngine>()`, and `[Voice; MAX_VOICES]` still fits D2 (ADR 0013).
- A TAPE voice's latched id sets its `sounding` bit, and the bit clears on the block after the voice ends.
- `gate_holds_tape_note_on`: ADR 0087's gate test, played on a TAPE voice. Its first sample after the move equals the unmoved source's.
- Retire never cuts a TAPE note.
- A TAPE Sound's project round-trip carries its `SourceRef` and no audio. A missing source loads silent, reading `SOURCE MISSING`.

**Cost and UI.** `tape_bills_each_model` (§ 5), and the bench rows held high. `hint_fits` holds the LOADS and WON'T FIT hints within 216 px. Screen goldens (`docs/screens`) of the strip for each model, and of SRC on a pool row and a card row.

**Importer.** Fuzz the WAV reader. A sweep above 0.45 × rate comes out at least 60 dB down. The CRC is deterministic across runs. `smpl` roots are read. The factory recipes' CRCs are pinned.

**Desktop QA** (QA before UAT): every factory source through all six models, sweeping each macro. Per-pair levels against the factory median (ADR 0063), with `out_gain` set so TAPE INIT on the reference source sits there. Recordings go on a listening page for the owner's UAT.

## 10. Build order

Each step ships on its own.

**Depends on #337:**
- step 1, the delay as `i16` (ADR 0090), for the 160 KiB arena;
- steps 2–5, the sparse encoder, the linker assert, the arena and the meter;
- step 7's source half: `SourceId`, the table, the gate, retirement, `Dir::Tapes`, `PoolSource`, `SourceRef`.

TAPE's steps 1–2 can be built and tested on the host beside #337 step 7, against a boxed arena.

1. **The source format:** `pack12`/`unpack12`, `SourceHead` with its root, `SourceWriter`, `SourceCheck`, `SourceView` and the Hermite read. Host tests.
2. **The importer:** `chimera-import tape` and `just tape`, writing to the simulator's card.
3. **The engine with LOOP:**
   - `EngineType::Tape`, `SlotKind::Tape`, `EngineSlot::Tape`, `EngineCtx`;
   - `ReelParams` (block 34), neutral, INIT;
   - latching and `sounding`;
   - the SRC, TAPE and model pages (no strip yet);
   - the cost and the `cost_test` row;
   - LOOP's goldens and click tests.
4. **REVERSE, PING-PONG, TRANSWAVE and SCRUB.**
5. **STRETCH.**
6. **The TAPE and AGE macros.**
7. **The strip,** on SRC and TAPE, and the head atomics.
8. **The source cell's card rows and MIX + PLUS LOADS.**
9. **USB `put` for sources:** ADR 0092's extension, under its own ADR, after `console-put` merges. Then `just put-tape`.
10. **The factory set:** the recipes, CC0 picks, the owner's recordings if any, and the factory TAPE Sounds in the card library (#317).
11. **The ship flash:** bench rows replace § 5's estimates, and the owner's UAT.

**Flash,** estimated and measured with `rust-size` at each step:
- the engine and its six models, about 4 KB;
- params, pages, the strip and the hint, about 4 KB;
- the source reader and check, about 1 KB.

That is **about 9 KB**, against about 77 KB left after #337.

**AXI:**
- `ReelParams` in each `ParamSnapshot`, about 56 B: six Parts and the triple buffer's three copies, about 1 KB;
- the strip's cache, 432 B;
- the head atomics, 48 B;
- the head cache, 512 B.

That is about **2 KB of the 36 KB margin.** D2 is +8 × 56 B for each voice's `played` snapshot, inside `VOICE_CHAIN_BYTES` if the chain assert allows it; measured at step 3.

## Owner questions

1. **The home page.** TAPE's home needs MODEL plus five macros (POS, SIZE, MOTION, TAPE, AGE), so SPACE moves off it to SENDS, unlike MODAL's RES. Alternatively, AGE waits for PROCESS/CHARACTER and SPACE stays. Which?
2. **The factory set's recordings.** It is built from Chimera's own renders and CC0 audio. Do you want to supply recordings of your own (voice, guitar, a room, a field recording) for it, and if so, which?

Settled by precedent or the code, not asked:
- **Mono sources** (the S950, the original Digitakt; stereo doubles the meter).
- **Hermite interpolation,** and no anti-aliasing on pitch-up (the S950's and SP-1200's character; sources are band-limited at import).
- **A real-time two-grain STRETCH, period-locked for MONO,** over WSOLA (its search lands in single blocks, § 5).
- **Linear crossfades** (an oscillator's loop is correlated).
- **No one-shot model:** KIT and DRUM are the one-shot path (#214, #215; #279's own note).
- **Loops are Sound parameters,** not source attributes (the Digitakt).
- **The root lives in the source,** set at import.
- **The engine's block is named `Reel` in code,** because `Tape` is the master stage's.
- **The import default stays 24 kHz** (#279), and the factory set picks a rate per source.
