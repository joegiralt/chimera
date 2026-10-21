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
        └ OSC 1, OSC 2, PIT

shared  FLT └ FMD (FILTER MOD), FX, JACK
        AMP
        MOD   MTX (matrix) └ E1 E2 E3, SPD, L1 L2 L3
```

TAPE has a chain in the code (SRC, DECK with its model page and PIT, FLT, AMP, MOD). TAPE rows in the Sound browser load like any engine's (ADR 0187); with bank 2 closed the Part reads NEEDS UPDATE (ADR 0141).

### SCRUB

DECK: MODEL · FINGER · SPEED · LENGTH · XFADE · DRAG. The SCRUB page: TAPE · AGE · WOW · KEY (ADR 0205).

- **FINGER** turns the reel. Its encoder is a jog that reads the hand's speed and glides to a stop through DRAG once let go. A CC or a route moves it too.
- **LENGTH** is the loop behind the finger, as a time: 2 ms to 2 s, never longer than the window. Its default is 70 ms, so a FINGER sweep moves the sound from the first turn.
- **KEY POSITION**, the default, makes a key place the finger and plays at ROOT. C is the window's start and each semitone up is 1/12 further along, in every octave. FINGER adds to the key's place. On FINGER the focus band reads KEYS PLACE THE FINGER.
- **KEY PITCH** plays the scrub at the key's pitch.

TAPE STRETCH (ADR 0204) has two TYPEs, CLASSIC and GRAIN: DECK holds MODEL · TYPE · SPEED (0 freezes) · LENGTH (0.25–8 s, GRAIN) · D-TIME (CLASSIC) · POSITION (CLASSIC); its model page STRCH holds TAPE · AGE · WOW · GRAIN (30–250 ms) · SPRAY · END (HOLD or STOP), the last three GRAIN's. Each TYPE's cells dim on the other.

### FLT and AMP

| Page | Cells |
|---|---|
| FLT | TYPE (the filter's kind: CLEAN, 2040, DIRTY, LADDER, SEM) · FX · Q · MODE · FX PLACE · FREQ. On the 2040: VOICE in MODE's place, CUTOFF and RES, and the TYPE readout names its one mode (4-POLE LP). On SEM: RES, and MODE is a knob that sweeps the output from low-pass through a notch at FREQ to high-pass |
| FMD | ENV AMT · KEYBOARD · FREQ MOD · FDBK (feedback) · RES MOD, F empty. FREQ MOD and RES MOD read the sources set on JACK. On SEM: F is OUTPUT, MODE (the MODE knob's sweep) or BP (band-pass) |
| FX | INPUT (the filter's input level) · the FX type's three pots · MIX · STABILITY (on the 2040, LADDER and SEM). SOFT: DRIVE · TONE · LEVEL. FOLD: FOLD · SYMMETRY · LEVEL. OFF: dashes, MIX dimmed |
| JACK | FREQ SRC in C and RES SRC in E: the sources FREQ MOD and RES MOD read |
| AMP | CHORUS · DELAY · REVERB (the three sends) · VEL (the VCA's velocity) |

### Pages that draw the whole band

FLT (the filter's response curve, with CUTOFF and RES), MST, E1-E3, SPD, ORBIT's ORB and CUSTOM, and SETTINGS' ABOUT have a picture in place of a focus band. The MOD matrix keeps its focus band.

---

## Mixer (MIX + B*n*)

```
PRT ── SND ── CHR ── DLY ── REV ── MST
                      │      │      │
                      CHAR   MORE,  LVL
                             MORE 2
```

Opens on SND, or on the effect page last left. PRT: CH, MODE, OUT, LEVEL, PAN, BEND. SND: the three send levels. EDIT from PRT or SND goes down to the Sound browser.

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
MIDI CONFIG   SYNC, PORT CONFIG (later), CHANNELS
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
