# 0270. A kit's TONAL pad plays from C1 up, tuned at middle C; OFF repeats the kit every octave

- **Status:** Proposed
- **Deciders:** owner (2026-11-13, "principle of least surprise"), firmware

## Context
ADR 0265 mapped a DRUM Part's notes as the Rytm does: C0–B0 (MIDI 0–11) the pads, C1 up the *selected* pad chromatic with C1 at its tuned pitch. In use this surprised: what a note played changed with whatever pad KIT showed (and PAD FOLLOW, ADR 0268, moves that on every hit), a kit played from a keyboard's middle octaves sounded a pad five octaves up, and a controller or sequencer sending GM-style drum notes (36 up) reached no pad at its pitch.

## Decision
- **TONAL, a kit setting.** KIT's fifth cell, TONAL: OFF or a pad 01–12 (`Tonal { Off, Pad(PadId) }`), on `BlockRef::Kit` (`KIT_SPECS`, one choice param; not in `BlockRef::ALL`, never in a Sound, no NRPN or block code). It is the kit's, not the selected pad's: the selection no longer decides what a note plays.
- **The mapping (`KitNote::new(MidiNote, Tonal)`, total, never panics).**
  - TONAL OFF: every note strikes pad `note mod 12` at its tuned pitch. The kit repeats every octave.
  - TONAL on pad p: notes 0–11 strike their pads as before; every note from 12 (C1) to 127 plays pad p at `note − 60` semitones, so middle C (60) is its tuned pitch and lower notes go down.
  - `KitNote` is `Pad(PadId)` | `Tonal(PadId, Transpose)`.
  - The TONAL pad keeps its RETRIG and CHOKE: a tonal chord needs the pad on STACK; CUT plays it mono; its choke group applies.
  - A TONAL pad whose machine has no PITCH (CLAP, SHAKER, NOISE; `Machine::pitched`) plays the same hit on every note. TONAL's focus band says so in its hint's place, e.g. NO PITCH ON CLAP.
- **`Transpose`, a new signed type** −60..=+67 (MIDI 0..=127 over middle C), built by `Transpose::new` → `Option`. It replaces 0265's unsigned `Semitones` (0..=115), which is gone. `DrumHit::pitch` and `KernelParams::transposed` take it. `drum_kernel_test` strikes every machine at its knobs' extremes at −60 and +67: finite, under +6 dBFS, finished within 20 s.
- **The audio's view.** `KIT_VIEW` keeps each Part's TONAL code where 0265 kept its selected pad (one `AtomicU8`, same size); `KitView::hit` takes the `MidiNote` and maps it there. A code past the pads (NOLOAD garbage at boot) reads OFF. `KitWriter::select` is gone.
- **Saved with the kit: the `Kit` record (0x0024) grows one byte**, TONAL's code after the 12 pads (157 B): 0 OFF, else the pad's number 1–12, frozen. A 156 B record (a kit saved before TONAL) loads TONAL OFF with its pads as written and is not lossy; an unknown code loads OFF and is lossy; any other length is `Bounds`.
- **Old projects load TONAL OFF**, which now means the kit repeats every octave: a 0.3 project (no `Kit` record) and a pre-TONAL kit alike. Notes 0–11 sound as before; a note from C1 up that played the selected pad chromatically now plays pad `note mod 12` as tuned. DRUM never shipped, so no saved performance relies on the old meaning (no legacy patch compat: old files load safely, not identically).
- `Kit` is 158 B (the `Tonal` enum is 2 B: `PadId` has no niche), `Kits` 6 × 2 B more in ITCM.

## Alternatives considered
- **Keep the selected pad (0265):** what a note plays moves with the screen and with PAD FOLLOW.
- **C1 = tuned pitch (0265):** a pad's tuned pitch five octaves under where a keyboard plays.
- **OFF = 0265's C1 up silent, or OFF = the pads only on C0–B0:** a kit that sounds on one octave only; repeating it is what a drum controller, sequencer or GM-mapped part expects.
- **TONAL in `Pad` (a param of the pad block):** it is one setting per kit; per pad, two pads could both claim C1 up.
- **A new non-critical record for TONAL (ADR 0045):** a second record for one byte of the same kit; the `Kit` record is unreleased and the length tells the shapes apart.
- **A signed `Semitones` reusing the unsigned type's name and fields:** two meanings under one name across the history; `Transpose` says "from the tuned pitch".

## Consequences
- What a note plays depends on the kit alone (saved), not on the UI.
- Supersedes ADR 0265's "Notes, as the Rytm" and its `Semitones` 0..=115, and the selected pad's role in hits ("C1 up the selected pad"); the rest of 0265 stands.
- KIT has one EMPTY cell left.

## Sources
- Owner's design, 2026-11-13. ADR 0265 (kit, `KitNote`, `Kit` record), 0268 (PAD FOLLOW), 0045 (disk codes).
