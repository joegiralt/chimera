# 0201. PIT reads the computed pitch in the scope's corner

- **Status:** Proposed
- **Deciders:** owner (request: "if I press C, is that actually a C?"), Claude (design)

## Context
Nothing on the instrument says what pitch a key sounds. The owner tunes a
Sound by bringing out a tuner: shape the Sound, go to PIT, turn PITCH and
FINE until the note is right. PIT is on every engine's chain and its band
is the live scope (`VizType::None`), so its corner is free.

Two ways to know the pitch:

- **Compute it** from what the voice is given: the note after the scale's
  snap, PITCH, FINE, TUNING, and the engine's own transposition. That is
  exact, costs a few dozen instructions a frame on the UI thread, and is
  there between notes, so it can follow PITCH and FINE while nothing
  plays.
- **Detect it** off the output (YIN, as the screensaver's "heard" does).
  The scope buffer is the master mix of every Part, a 5 ms window whose
  floor is about 300 Hz (D4); chords, drums, noise and other Parts
  confuse it, it costs ~12 k multiply-adds a measure, and it has nothing
  to say between notes.

TAPE needs more: a source's head carries `root` (MIDI cents) and `period`
(frames, 0 when unpitched), the importer's estimate (`--root`, the WAV's
`smpl` chunk, else YIN, else C4), copied into the Sound's ROOT and PERIOD
at pick (ADR 0093). The voice reads at `2^((note·100 + pitch − root)/1200)`
of the source's rate, so a source whose root is right sounds at the key.
With `period` 0 the import found no pitch and `root` may be the C4
fallback: its sounding note is unknown.

Bank 1 is frozen (ADR 0198, proposed): new code goes behind a bank-2
door.

## Decision
1. **Computed, not detected.** PIT's band draws the scope and, in its
   top-right corner, the pitch the edited Part's last note rests at:
   `C4 +12ct` (nearest note, cents −50..=+49, A4 = 440 Hz; the fonts have
   no cent sign, so `ct`, as the screensaver) over `261.6 Hz`. The sum is
   the voice's (`calc::pitch`): note + PITCH + FINE + TUNING, plus ALGO's
   TRNSP or VA's OSC 1 CRSE and FINE. Glide, bend, routes, wow and a
   SCRUB finger are left out: they move the pitch for a moment; the corner
   reads where it rests. No detection anywhere.
2. **TAPE.** A source with a period plays at the key (its root cancels).
   TRANSWAVE plays the note. SCRUB adds `1200·log2(2·|MOTION|)`, and
   reads `STILL` at MOTION 0. A source with no period reads `ROOT ?` over
   the shift off its own speed (`+7st +12ct`): never a confident wrong
   note. No source: nothing.
3. **Which note.** Each Part's last live note (`played`, after the snap),
   kept per Part while the UI runs. Lit (INK, INK2) while the Part
   sounds; dim (MID) between notes, recomputed under PITCH and FINE as
   they are now, so the owner tunes between notes; before the Part's
   first note, middle C, the second line led by `REF C4`. ORBIT's notes
   don't publish to `played`, so they don't move it.
4. **Drawing.** The corner is the viz region's: the scope is drawn, a
   backing box cleared right of the lines' left edge from the band's top,
   the lines right-aligned on `VIZ_RIGHT`. The region's key folds the
   corner's key (its text, lit, size) into the scope's: dirty is full,
   nothing redraws at idle, the text changes only at 1 cent and 0.1 Hz.
   NORMAL draws helvB10 over helvR08, LARGE helvB14 over helvR10 where
   both fit `CORNER_W` (112 px), else NORMAL; the box stays above the
   scope's middle line.
5. **Banks.** The working and the paint (`ui::pit_tuner::calc`) are
   bank 2's (`bank2.x` glob, `check-door-placed`), reached through
   `UiDoor::pit` (the pass, once a frame, on `UiState`) and
   `UiDoor::pit_paint` (onto the strip's `dyn Paint`; the fonts' raster
   is instanced over `calc::Runs`, so its name carries the module and
   the glob takes it). Its words are a named static and its numbers its
   own digits, so no unnamed rodata lands in bank 1; `played::last_in`
   and `last_out` are inlined so neither is outlined for its second
   caller. Bank 1 keeps the `Tuner` state (RAM), the two door calls, the
   frame's field and the key's XOR into the scope's; the corner's key
   is 0 off PIT, so every scope page shares the one arm. Measured on
   the default firmware against origin/tape-streaming (0d026a9a):
   bank 1 +144 B (.text; .rodata and .data +0), bank 2 +2,528 B.
   Closed, the corner is empty.

6. **Types.** A pitch is a `Tone` (cents above MIDI 0) and gives its
   `Hz` and its `Named` note; the played key is a `MidiNote`; a source
   with no root plays at a `Shift`, which names no note. What the corner
   shows is a `Readout`: `None`, `Lit`, `Dim` or `Reference` of a `Known`
   (a `Tone` or SCRUB's `Still`), or `RootUnknown(Shift, Light)`, so a
   root that isn't known can't be drawn as a confident note, and a
   reference can't be lit.

## Alternatives considered
- **Detection only.** Rejected: noisy on drums and chords, deaf below D4,
  reads the master mix not the Part, and silent between notes, when the
  owner tunes.
- **Hybrid, detection where speed varies (SCRUB).** Rejected for now:
  SCRUB's rest speed is MOTION's, which is computed; only a finger's
  gesture moves it beyond that, as a bend does, and the scope's floor and
  mix make that reading worse than none.
- **Trusting `root` when `period` is 0.** Rejected: it is the C4 fallback
  more often than not, and a confident wrong note is worse than `ROOT ?`.
  A `--root` given on an unpitched import reads `ROOT ?` too; telling
  the two apart needs a flag in the head.
- **Nothing between notes.** Rejected: the owner tunes between notes.

## Consequences
- The owner can tune by eye on every engine, with no lag and no audio.
- An FM Sound whose carrier ratio isn't 1 sounds its carrier, not the
  note: the corner reads the note (the ratio is the timbre's).
- The corner covers up to 112 × 33 px of the scope's top-right.
- RAM: the `Tuner` state (six last notes and the two lines). Flash: 144 B
  of bank 1, 2,528 B of bank 2.

## Sources
- Request: owner, 2026-10-21 (PIT tuner).
- `chimera-engine/src/dsp/engines.rs` `set_pitch`; `tape_engine::inc`,
  `wave::cycles`, `scrub_block`; `storage/source_file.rs` `SourceHead`;
  `chimera-desktop/src/import/tape` (root and period).
- ADR 0093 (TAPE sources), 0175 (the UI's door), 0198 (bank 1 frozen).
- de Cheveigné & Kawahara 2002, YIN (the screensaver's "heard").
