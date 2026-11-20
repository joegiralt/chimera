# Chimera Site Map

Every chain, page and sub-page in the instrument, as built on `main`. The source of truth is `chimera-core/src/ui/block_registry.rs` (Part and mixer chains), `ui/orbit/pages.rs` (ORBIT) and `ui/settings/tree.rs` (SETTINGS). Navigation rules: ADR 0081, 0145 and 0149.

Short names are the chain map's node labels; the sub-page row spells pages in full words.

---

## Navigation

| Key | Does |
|---|---|
| B1-B6 | Part *n*'s pages. The same key again: its mixer; from that Part's mixer it returns to your page. From anywhere else it lands on the engine's home page |
| MIX + B*n* | Part *n*'s mixer (inside the mixer: keeps the page) |
| EDIT + B*n* | The Sound browser for Part *n* |
| MINUS / PLUS | Previous / next node along the chain |
| EDIT / SEQ | Under ROW (the default): previous / next sub-page, SEQ wrapping from the last. Under STACK: EDIT down (wrapping), SEQ up |
| hold SEQ | ORBIT |
| MENU | SETTINGS; inside, back a level (SEQ goes back a level too). Hold: quick-save |
| Dialogs | EDIT confirms; MENU or SEQ cancels; any encoder moves the highlight |

SETTINGS › PERSONALIZE › CHAIN MAP › PAGES picks ROW or STACK; TEXT SIZE › SIZE picks NORMAL or LARGE.

---

## Part chains (B1-B6)

The chain follows the Part's engine. Every chain ends in MOD, and FLT and AMP are shared.

```
ALGO    ALG ── OSC ── FLT ── AMP ── MOD
         └ PIT  └ CRS … FBK (12), OP1 … OP6 (6)
                 (a formant operator adds FRM first)

MODAL   EXC ── RES ── FLT ── AMP ── MOD
                └ MDL2, PIT

VA      VA ── FLT ── AMP ── MOD
        └ OSC1, OSC2, NOISE, PIT

shared  FLT └ FMD (FILTER MOD), FX, JACK
        AMP └ STEREO, SENDS
        MOD   MTX (matrix) └ E1 E2 E3, SPD, L1 L2 L3
```

TAPE has a chain in the code (SRC, DECK with its model page and PIT, FLT, AMP, MOD). TAPE rows in the Sound browser load like any engine's (ADR 0187); with bank 2 closed the Part reads NEEDS UPDATE (ADR 0141).

### TAPE

MODEL lists the machines WALK, STRETCH (STRCH), SCRUB and WAVE. Each machine has its own DECK cells after MODEL and its own model page after TAPE, AGE and WOW; the model page is named by the machine (ADR 0178, 0203). A cell reads STRCH and FWD where the word doesn't fit; the focus readout says STRETCH and FORWARD.

| Page | Cells |
|---|---|
| SRC | SOURCE · START · END · VOL |
| DECK on WALK | MODEL · LENGTH · FADE · DIRECTION (FWD, REVERSE, LOOP, PING) · POSITION · DRIFT |
| WALK | TAPE · AGE · WOW · TRON (OFF, ON) · XFADE · RETRIG. XFADE reads on LOOP and on FWD or REVERSE with TRON on, dimmed once through with TRON off (ONLY LOOP AND TRON CROSSFADE) and on PING-PONG, TRON on or off (PING-PONG TURNS · NO SEAM); RETRIG reads on FWD and REVERSE with TRON off, dimmed on LOOP and PING-PONG (CUTS ONLY ONCE THROUGH) and with TRON on (CUTS ONLY WITH TRON OFF) |
| DECK on STRETCH | MODEL · TYPE · SPEED · LENGTH · D-TIME · POSITION |
| STRCH | TAPE · AGE · WOW · GRAIN · SPRAY · END |
| DECK on SCRUB | MODEL · FINGER · SPEED · LENGTH · XFADE · DRAG |
| DECK on WAVE | MODEL · FRAME · SCAN · RANGE |
| PIT | under DECK, after the model page |

On WALK, FADE reads 10 ms to 500 ms, its last step HOLD on FWD and REVERSE with TRON off. DRIFT, a percentage of the window, moves POSITION by a random step at each note. TRON on gives each key a tape of 0.25 to 8 s (LENGTH, read in seconds on FWD and REVERSE), the window coming round until it runs out, and lifts it at key up. WALK's glyph is footsteps: their line, loop or lanes by DIRECTION, scattered by DRIFT, a tape's end with TRON. The model page on WAVE reads WAVE. On a streamed source, DIRECTION holds REVERSE and PING dimmed with `… NEEDS RAM · STREAMED`.

TAPE STRETCH (ADR 0204) has two TYPEs, CLASSIC and GRAIN. DECK holds MODEL · TYPE · SPEED · LENGTH · D-TIME · POSITION: SPEED reads the pointer's factor (`1.00x`, `0.00x` frozen, −1× to 2×), LENGTH 0.25–8 s (GRAIN's), D-TIME and POSITION CLASSIC's. The model page STRCH holds TAPE · AGE · WOW · GRAIN (30–250 ms) · SPRAY · END (HOLD or STOP), the last three GRAIN's. Each TYPE's cells dim on the other, the band saying GRAIN TYPE ONLY or CLASSIC TYPE ONLY.

### SCRUB

DECK: MODEL · FINGER · SPEED · LENGTH · XFADE · DRAG. The SCRUB page: TAPE · AGE · WOW · KEY (ADR 0205).

- **SPEED** reads the loop's factor of the source's speed (`1.00x`, −2× to 2×); **DRAG** reads 10 ms to 500 ms.
- **FINGER** turns the reel. Its encoder is a jog that reads the hand's speed and glides to a stop through DRAG once let go. A CC or a route moves it too.
- **LENGTH** is the loop behind the finger, as a time: 2 ms to 2 s, never longer than the window. Its default is 70 ms, so a FINGER sweep moves the sound from the first turn.
- **KEY POSITION**, the default, makes a key place the finger and plays at ROOT. C is the window's start and each semitone up is 1/12 further along, in every octave. FINGER adds to the key's place. On FINGER the focus band reads KEYS PLACE THE FINGER.
- **KEY PITCH** plays the scrub at the key's pitch.

### VA

| Page | Cells |
|---|---|
| VA | WAVE1 · WAVE2 · MODE · BLEND · SUB · STABILITY |
| OSC1, OSC2 | CRSE · FINE · SHAPE, D-F empty |
| NOISE | KIND (WHITE, PINK, RED, DIGITAL) · LEVEL, C-F empty. One noise source a voice, mixed beside SUB after MODE, before the filter. DIGITAL is a sample-and-hold that steps 16 times each OSC 1 cycle, so it brightens up the keyboard |

### FLT and AMP

| Page | Cells |
|---|---|
| FLT | TYPE (the filter's kind: CLEAN, 2040, DIRTY, LADDER, SEM) · FX · Q · MODE · FX PLACE · FREQ. On the 2040: VOICE in MODE's place, CUTOFF and RES, and the TYPE readout names its one mode (4-POLE LP). On SEM: RES, and MODE is a knob that sweeps the output from low-pass through a notch at FREQ to high-pass |
| FMD | ENV AMT · KEYBOARD · FREQ MOD · FDBK (feedback) · RES MOD, F empty. FREQ MOD and RES MOD read the sources set on JACK. On SEM: F is OUTPUT, MODE (the MODE knob's sweep) or BP (band-pass) |
| FX | INPUT (the filter's input level) · the FX type's three pots · MIX · STABILITY (on the 2040, LADDER and SEM). SOFT: DRIVE · TONE · LEVEL. FOLD: FOLD · SYMMETRY · LEVEL. OFF: dashes, MIX dimmed |
| JACK | FREQ SRC in C and RES SRC in E: the sources FREQ MOD and RES MOD read |
| AMP | VEL (the VCA's velocity) · LPG · DECAY · STRIKE · BASE, F empty. LPG, DECAY and STRIKE turn the VCA into a low-pass gate; all three at 0 is a plain VCA. LPG ties darkening to gain, so quieter is darker. DECAY is how slowly the gate closes. STRIKE pings the gate at each note-on, as hard as the note is played: an accent over the envelope into VCA, which a held note falls back to; with no route into VCA the strike alone plays it at 1. VEL scales the routes into VCA. Routes into VCA are the amp: they replace the engine's own contour, and their sum starts at BASE and stops at full. BASE 127 with PRESS at −64 plays the engine's own contour and ducks it under pressure; BASE 64 with PRESS at +63 swells it; BASE 0 with PRESS alone is silent until pressed. On ALGO and MODAL a BASE above 0 keeps the carriers' or the resonator's own tail after key up. Until a route exists, VEL and BASE are dimmed, the band on BASE saying NEEDS A ROUTE INTO VCA. In the matrix, the VCA column says so on an empty cell: VCA: ROUTES REPLACE CONTOUR, or with an ENV routed, VCA: SUM STARTS AT AMP BASE |
| STEREO | PAN · WIDTH, C-F empty. PAN places each note of ALGO, MODAL, VA and TAPE between the Part's outputs (C the middle, L64 hard left, R63 hard right), under the Part's own PAN; it is a destination (MIX + PLUS on it primes it in the matrix), so an LFO, an envelope, VEL or NOTE moves each note on its own. A note set or moved off the middle sends its middle to the effects. WIDTH scales the Part's stereo: 0 folds every panned note (and on DRUM every panned pad) to the middle, 64 plays them as panned, 127 spreads them twice as wide, past the outputs; left plus right never changes. A turn of WIDTH glides. The band says PLACES EACH NOTE, ROUTABLE on PAN and 0 MONO, 64 AS PANNED, 127 WIDE on WIDTH. DRUM's pads keep their own PAN: DRUM has no STEREO page, and WIDTH is F on its AMP › GATE |
| SENDS | CHORUS · DELAY · REVERB (the three sends), the mixer's SENDS page |

### PIT

Every engine has PIT: PITCH · FINE · STEAL · TIME (glide) · SNAP, F empty. The scope's top-right corner is a tuner (ADR 0201): the pitch the Part's last note sounds at, as a note and cents (`C4 +12ct`, A4 = 440 Hz) over its frequency (`261.6 Hz`). It is worked out from the note, PITCH, FINE, TUNING and the engine's own transposition (ALGO's TRNSP, VA's OSC 1), so it moves the moment PITCH or FINE turns. It is lit while the Part sounds and dim between notes; before the Part's first note it reads middle C, marked `REF C4`. Glide, bend, modulation and wow are left out: it reads where the pitch rests.

On TAPE a source plays at the key it is played on, by its root. A source whose import found no pitch has no known root: the corner reads `ROOT ?` over the shift from the source's own speed (`+7st +0ct`). TRANSWAVE plays the note; SCRUB plays at twice MOTION's speed, `STILL` at 0. A TAPE Part with no source shows nothing. With bank 2 closed the corner is empty.

### Pages that draw the whole band

FLT (the filter's response curve, with CUTOFF and RES), MST, E1-E3, SPD, ORBIT's ORB and CUSTOM, and SETTINGS' ABOUT have a picture in place of a focus band. The MOD matrix keeps its focus band.

---

## Mixer (MIX + B*n*)

```
PRT ── SND ── CHR ── DLY ── REV ── MST
                      │      │      │
                      CHAR,  MORE,  LVL
                      GRAIN  DUCK,
                             MORE 2
```

Opens on SND, or on the effect page last left. PRT: CH, MODE, OUT, LEVEL, PAN, BEND. Its band shows each Part's LEVEL bar and pan dot; beside each LEVEL bar a thin live meter shows what the Part plays after its EQ and before LEVEL, so a Part pulled down still reads (foot −48 dB, top 0 dB; it falls 20 dB a second, its peak a bright tick held a second; its top lights red when the Part's bus passes full scale). A small square right of a Part's number is lit while a key is held on its MIDI channel (for an MPE zone, on any of its channels), so a Part that hears notes but plays nothing stands out. The meters run only while PRT, VOICES or EQ is up. SND: the three send levels. CHR: TYPE (OFF, I, II, I+II, FLANGER, PHASER), RATE, DEPTH, MIX. I, II and I+II are the Juno-60's chorus buttons; FLANGER is a short swept delay fed back on itself, PHASER six all-passes a side swept and fed back. Each returns the effect alone, so the notches come with the dry beside it: the CHORUS send sets how deep they cut. RATE and DEPTH set each TYPE's sweep. DLY: TYPE (TAPE or GRAIN), TIME, FDBK, TONE, REV, MIX; below it CHAR (TAPE's MECHANICS and SAT) and GRAIN (SIZE, DENSITY, PITCH, TEXTURE), each heard under its TYPE and dimmed under the other, the band saying TAPE TYPE ONLY or GRAIN TYPE ONLY. SYNC (FREE or SYNC) is cell E of both CHAR and GRAIN, and MIX + a turn of TIME flips it from DLY: right for SYNC, left for FREE. FREE, TIME is 10 to 500 ms. Under SYNC, TIME turns a note of the tempo, one a detent: 1/32T, 1/32, 1/16T, 1/32D, 1/16, 1/8T, 1/16D, 1/8, 1/4T, 1/8D, 1/4 (T a triplet, D dotted; 1/8D to start, 375 ms at 120 BPM). The tempo is the MIDI clock's while one leads, otherwise the project's. Both TYPEs follow the note: TAPE's repeat, and how far back GRAIN's grains start (DENSITY stays free). A note longer than 500 ms at the tempo plays halved, still on the beat, and reads as it plays with a mark, ▾1/8 for a quarter at 90 BPM, the band saying HALVED TO FIT 500 MS. A small tempo change bends TAPE's repeats like tape, by a third of a semitone at most, and a large one, a new note or SYNC itself crossfades in 20 ms; GRAIN's next grains start on the new time. The free TIME and the note are kept apart: back in FREE the ms is the one you left. While bank 2 needs an update, TYPE reads NEEDS UPDATE at GRAIN, which plays as TAPE. REV › DUCK, under every TYPE: AMOUNT and RELEASE turn the reverb's return down while its own input is loud, so a big reverb doesn't smear a pluck's attack, and bring the tail back in the gaps. The input is what the reverb hears: the Parts' REVERB sends and the delay's REV send. AMOUNT 0 is no duck; at 127 a loud input (−6 dB and up) takes the return 30 dB down, a quieter one less, nothing below −42 dB. RELEASE (1 ms to 2 s) is how long the tail takes to come all the way back once the input stops. EDIT from PRT or SND goes down to the Sound browser.

---

## Sound browser (EDIT + B*n*)

Tabs PROJECT, LIBRARY and FACTORY (encoder 2). Rows: INIT ALGO, INIT MODAL, INIT VA, INIT TAPE and INIT DRUM (DRUM COMING SOON), then the tab's Sounds. MINUS / PLUS change Part; SEQ goes back to the mixer.

---

## ORBIT (hold SEQ)

```
ORB ── RING ── CHRD ── SCN
 └ CUSTOM  └ RHYTHM, VOICE, NOTES
```

B6 plays and stops; B1-B5 open a ring; hold B1-B5 goes to the ring's Part; MIX + B1-B5 mutes it. MENU leaves.

---

## SETTINGS (MENU)

```
PROJECT       LOAD PROJECT, SAVE PROJECT AS, MANAGE PROJECTS, POOL, PURGE UNUSED SRC
PART          RENAME, CLEAR, SAVE TO PROJ, RELOAD FROM PROJ, LIVE SNAP,
              SAVE TO LIBRARY, MIDI MAP
ORBIT         RENAME (later), CLEAR, SAVE TO PROJ, RELOAD FROM PROJ
MIDI CONFIG   SYNC, PORT CONFIG (later), CHANNELS, CONTROL CHANNELS, ECHO
SYSEX DUMP    later
AUDIO ROUTING OUTPUTS, SENDS (later), TUNING
PERSONALIZE   THEME, SCREENSAVER, CHAIN MAP, TEXT SIZE
SYSTEM        OS UPGRADE, STORAGE (later), FORMAT CARD (later), USB CONFIG (later),
              DIAGNOSTICS, PUSH ENCODERS, ABOUT
```

THEME holds BRIGHT, GAMMA, ACCENT, BLACK, HINTS and INVERT. SCREENSAVER: OFF, 5, 7, 10, 30, 60 s, 2, 5, 10 min (default 5 min). DIAGNOSTICS: AUDIO LOAD, TEST TONE (later), INPUT TEST (later). Rows marked later are dimmed and labelled LATER; the bar lands on them, but EDIT does nothing.

---

## Not on the map yet

The chain editor, send-effect routing (SENDS under AUDIO ROUTING), SYSEX DUMP, STORAGE, FORMAT CARD and USB CONFIG are not built. Track them in the issue tracker, not here.
