# Tuning

Each Part plays in a tuning of its own. Pick it on **SETTINGS › AUDIO ROUTING › TUNING**, cell B, **TUNING**: it sets the active Part's, as LIVE SNAP's cells do (pick another Part to set its own). Cell A, **TUNE**, still shifts the whole unit. A new note plays the tuning; a note already sounding keeps its pitch.

![SETTINGS › AUDIO ROUTING › TUNING with bohlen-pierce.scl picked, annotated](img/tuning.png)

1. **TUNE**: the whole unit, in cents.
2. **TUNING**: the active Part's tuning. Turn past `MTS` and it walks the Scala files on the card, by file name. The cell shows a name's first 7 characters.
3. **MAPPING**: how a Scala file lies on the keys, `CONTIN.` (continuous) or `KEYBD` (keyboard).
4. The Scala file's own name for its scale (its description line), or the file's name if it has none.
5. While TUNING has focus, a longer file name scrolls here and in the cell: it holds still, runs to its end, holds, and starts again.

| TUNING | What it plays |
|---|---|
| `12-TET` | Equal temperament, every semitone 100 cents. |
| `JUST` | 5-limit just intonation on C: pure thirds and fifths in C's keys. |
| `PYTHAG` | Pythagorean on C: pure fifths, bright wide thirds. |
| `MEANTONE` | Quarter-comma meantone: pure major thirds, a wolf fifth on G♯–E♭. |
| `WERCK` | Werckmeister III: a well temperament, every key playable, each its own colour. |
| `MTS` | The tuning a DAW or editor sends as MIDI Tuning Standard SysEx (below). |
| a file name, such as `PELOG` | A Scala file from the card. |

C stays at its usual pitch in every named temperament, so a Part in `JUST` plays in tune with one in `12-TET` on C.

## Scala files

The card is inside the case, so a Scala file goes onto it over USB, in one of two ways.

**With the bootloader's USB disk mode** (no tools needed): power the unit on into its bootloader, as for a firmware update from the card, and pick its USB disk mode. The card shows up on the computer as a drive. Copy the files into its **CHIMERA/TUNINGS** folder (make the folder if it isn't there), eject the drive, and power the unit on as usual.

Files keep the names they have: `bohlen-pierce.scl` shows as `BOHLEN-PIERCE`, in capitals (letters outside A–Z show as `?`, and the first 32 characters are shown). A keyboard map pairs with the scale of the same name: `bohlen-pierce.kbm` with `bohlen-pierce.scl`. The hidden files a Mac leaves beside the ones you copy (`._bohlen-pierce.scl`, `.DS_Store`) are harmless: they are never listed.

**With `chimera-usb.py`**, Chimera's tool (Python 3, Linux or macOS), with Chimera connected and on:

```
python3 chimera-usb.py tuning pelog.scl
python3 chimera-usb.py tuning pelog.scl java.kbm
```

The file keeps its own name: `pelog.scl` lands as `PELOG.SCL` in the card's **CHIMERA/TUNINGS** folder, and a `.kbm` sent with it lands beside it under the same name (`PELOG.KBM`). The tool only takes names of 1 to 8 letters and digits, so rename `bohlen-pierce.scl` to, say, `bohlen.scl` for it, or copy it in USB disk mode under its own name. Sending a file of the same name again replaces it, and every Part playing it plays the new one at once. A new `.scl` also clears its old `.kbm`, so send the map with the scale, as the command does. Chimera only reads that folder; nothing else on the card changes.

On TUNING, turning past `MTS` steps through the `.scl` files in name order, and the cell shows the file's name; the line under the big readout shows the scale's own name. The first 128 `.scl` files in the folder are reached, in the order they were copied; turning past the last of them when the folder holds more says `FIRST 128 FILES ONLY`.

- A `.scl` file holds the scale: up to 128 steps, in cents or as ratios. Thousands are in the Scala archive (huygens-fokker.org).
- **MAPPING** lays the scale on the keys:
  - `CONTIN.` (continuous): middle C plays the scale's first step at its usual pitch, and each key up plays the next step, whatever the scale's size.
  - `KEYBD` (keyboard): each scale starts on a C. The scale's steps go on the keys from C up; the octave is rounded up to whole octaves of keys (a 7-step scale takes 12 keys, a 19-step scale 24), and the keys left over are silent. A 12-step scale plays the same either way.
- A `.kbm` file of the same name (`PELOG.KBM`), if present, maps keys to the scale's steps and sets the reference pitch, in place of MAPPING. Keys it leaves out (`x`), or outside its range, play equal temperament.
- To take a file off the card again, move it out of the folder in USB disk mode, or name it to the tool: `python3 chimera-usb.py rm /CHIMERA/TUNINGS/PELOG.SCL` (or `PELOG.KBM`). A file copied with a long name is easiest to take off in USB disk mode. Every Part playing it then plays `12-TET` (or, with only the `.kbm` gone, the scale by its MAPPING).
- A file is read when you pick it, when a project using it loads, and when a new one is sent or removed. If it is missing or can't be read, the Part plays `12-TET` and the screen says why, such as `PELOG.SCL NOT FOUND` or `PELOG.SCL NOT SCALA`. Files up to 4 KB are read.

The project keeps the file's name, not its place in the list: a file added later moves nothing, a file copied onto the card again under the same name is still the one it plays, and a changed `PELOG.SCL` on the card changes every project that plays `PELOG`.

## MIDI Tuning Standard

A Part on `MTS` plays the tuning received over MIDI, DIN or USB. Chimera takes:
- **single-note tuning changes** (real-time, and the bank form);
- **bulk tuning dumps** (all 128 keys);
- **scale/octave tuning**, one-byte and two-byte (each pitch class in every octave).

Messages for device 7F (all) or 0 are taken. Every Part on `MTS` follows one MTS tuning; the tuning program and channel bits in a message are ignored. A received tuning lasts until the unit is turned off: it isn't saved with the project.
