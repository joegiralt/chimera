# 0127. An oscilloscope screensaver reads the notes played and the scope

- **Status:** Accepted (2026-10-11)
- **Deciders:** owner (design), Claude (implementation)

## Context
The panel's TN display wants resting, and an idle synth can show what it
hears. The UI already has the scope capture (`scope`, 240 samples, 48 kHz)
but knows nothing of the notes: they go straight to the audio thread.

## Decision
- SETTINGS › PERSONALIZE › SCREENSAVER (own row, one cell): OFF / 5 / 7 / 10 / 30 / 60 s, default 7 s, stored in
  SYSTEM in the THEME block (`B 26 6 SAVER`; 5 is INVERT's). After that long with no button or
  encoder input (a prompt or card work, and the time a blocking card operation took, count as attention) the screen shows
  the master scope large in the accent, through the palette. MIDI never wakes
  it. Any button or encoder wakes it; that press, and everything until it is
  released, is swallowed.
- The readout: IN (last note received, velocity, Part; `+N` for more keys
  held), OUT (the note as the Part sounds it: after live SNAP, plus PITCH and
  FINE, as nearest note and cents), "(heard ..)" measured off the scope, then
  the held keys of every Part and their chord. The cent sign is not in the
  ASCII fonts: cents read `ct`.
- OUT comes from the audio thread's note-on, which stores the note after snap
  and the Part's PITCH + FINE in cents (`played`: a few relaxed atomic ops per
  note event; and one scan of the voice slots per block to let go a held key no
  voice holds, a lost note-off). Held keys are tracked per (channel, key);
  the panel names each key number once. A project load clears them. No pitch bend or global tuning exists
  in the engine, so there is none to add; modulation of PITCH is not in OUT.
- Measuring runs in the UI loop at ~7 Hz: YIN's cumulative-mean difference
  finds a period only below a dip of 0.1 (so noise and silence read nothing),
  sharpened by a linearly interpolated rising zero crossing k periods on.
  The 5 ms scope window limits it to about 300 Hz and up; below that nothing
  is shown. Error on clean synthetic sines and saws is under 1 cent (tested to 3).
  "heard" shows only when it is trustworthy: one key held at most, OUT at
  D4 or above, and the measure within a semitone of OUT, so a bass note or a
  strong 2nd harmonic (which reads an octave up) shows nothing, never a wrong
  octave.
- Chords come from `chimera-theory`'s `Chord::from_notes`: the lowest key is
  tried as the root first (root position wins: C E G A is C6), then the other
  tones rising; slash chords name the lowest key; "?" when no quality fits
  exactly. One key shows its name, two the interval.

## Alternatives considered
- A sixth THEME cell: INVERT's.
- Measuring in the audio thread or capturing a longer window: costs the audio
  path or changes the scope for every page.
- Waking on MIDI: a playing synth would never rest.

## Consequences
About 4.4 KB of flash. The screensaver redraws the whole screen at ~7 Hz.
Low notes (under ~300 Hz) show OUT but no "heard".

## Sources
de Cheveigné and Kawahara, "YIN, a fundamental frequency estimator for speech
and music", JASA 2002.
