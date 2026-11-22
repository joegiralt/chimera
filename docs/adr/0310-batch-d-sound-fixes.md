# 0310. Fix batch D's sound defects in place, each bit-identical where it isn't broken

- **Status:** Accepted (2026-11-22)
- **Deciders:** Claude (the 0.4 bug sweep, owner rule 2026-11-21: every known bug ships fixed); the owner hears the before/after renders

## Context
The 0.4 bug sweep's batch D (sound and DSP) found five defects with a clear fault: a clamped chorus tap (#62), an unbounded loop (#236), a zippered VCA amount (#180), a click entering SYNC (#670) and a pluck sized at the wrong length (#233). It also found a simulator that moved MODAL's 48 kHz constants on a 44.1 kHz device (#239). Each fix has a test that failed on origin/0.4. Each changes sound only where the old behaviour was broken.

## Decision
- **Chorus (#62):** a BBD line's taps sit at `max(base, depth + 1)` ± `tri · depth` (`BbdLine::taps`). II above DEPTH 0.71 moves its centre out instead of clamping; every other setting keeps `base`, bit for bit.
- **Halo fold (#236):** `chords::fold` first tests the period for finiteness: a finite one fits after about 118 halvings at most (from `f32::MAX`), and an infinite or NaN one folds to `FITS / 2`. A counted loop was tried first and unrolled into about 10 KB of bank 1.
- **VCA amounts (#180):** a VCA route's amount ramps across the block from the last block's `VcaRoutes` (the voice already keeps it), for ENV sources in `Envelope::add_ramped` (its amount is now `(from, to)`) and for the others in the voice. A route deleted or moved to another source ramps out the same way. When the last route goes, the block ramps from the routes (BASE and AMP's VEL with them) into the engine's own pass-through. A fresh note starts on its own. An unchanged matrix takes the old arithmetic, bit for bit.
- **SYNC entry (#670):** at the MODE duck's swap into SYNC (its gain is 0 there), OSC 2's phase is set to `φ1 · inc2 / inc1` at the block's end: where the master's sync would have put it.
- **Routed pluck (#233):** a fresh STRING or SYMP pluck keeps its level for `shape` in the float a re-strike's level already used (`string::Pending`: `−level` fresh, `+level` re-strike, `+0` none). On the note's first block, if the voice's pitch ratio is not the one the strike used, the string is plucked again at the routed length before `shape`, in bank 2's block, from the noise state the first pluck drew (`StringVoice::seed`, `Voice` 6,728 → 6,736 B), so the RNG moves once a note and the routed pluck is the stored offset's. A Sympathetic note-on a PITCH route may move bills `REPLUCK_BYTES` (the second clear and noise fill, a ring each at most) on the clear budget; such a note may pass the budget only in a block no other clear touched, so a note with no PITCH route is admitted as before. A Sound with any PITCH route, INIT's vibrato route among them, now bills the re-pluck and can start a chord's notes a block later.
- **Simulator rate (#239):** the desktop's engine always runs at `SAMPLE_RATE`. A device at another rate gets linear interpolation, one frame late; at 48 kHz frames pass untouched. The firmware is unchanged.

The #180 and #670 code sits in bank 1 (the voice's VCA and `VaEngine::render`'s swap). That is allowed because bank 1 shrinks overall: keeping MODAL's pluck out of line more than pays for both.

## Alternatives considered
- #62: raising `base` for every mode (it changes every chorus's delay and goldens), or scaling DEPTH down in II (it narrows the knob's top).
- #180: per-source `f32` amount history (more RAM); the stored `VcaRoutes` already is it.
- #670: resetting both oscillators at the swap (it moves the master too, and MIX's phases).
- #233: deferring every pluck to the first block (it changes routed COLOR and EXCITE too, and every golden).
- #239: threading the sample rate through MODAL's constructors (it grows bank 1 for a desktop-only fault).

## Consequences
Sounds change only at DEPTH > 0.71 on chorus II and I+II, while a VCA amount turns, at a MODE change into SYNC, and on MODAL notes with a note-on PITCH route. The renders are in (private notes). The VA noise hash in `va::tests::noise_off_is_bit_identical` is re-pinned for #670's swap.

## Sources
private issue #62, #180, #233, #236, #239, #670; branch bugs-d.
