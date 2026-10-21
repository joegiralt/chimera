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
        └ OSC 1, OSC 2, NOISE, PIT

shared  FLT └ FMD (FILTER MOD), FX, JACK
        AMP
        MOD   MTX (matrix) └ E1 E2 E3, SPD, L1 L2 L3
```

TAPE has a chain in the code (SRC, DECK with its model page and PIT, FLT, AMP, MOD). In 0.1.0 and until it ships, TAPE rows in the Sound browser read COMING SOON and don't load.

### VA

| Page | Cells |
|---|---|
| VA | WAVE1 · WAVE2 · MODE · BLEND · SUB · STABILITY |
| OSC 1, OSC 2 | CRSE · FINE · SHAPE, D-F empty |
| NOISE | KIND (WHITE, PINK, RED, DIGITAL) · LEVEL, C-F empty. One noise source a voice, mixed beside SUB after MODE, before the filter. DIGITAL is a sample-and-hold that steps 16 times each OSC 1 cycle, so it brightens up the keyboard |

### FLT and AMP

| Page | Cells |
|---|---|
| FLT | TYPE (the filter's kind: CLEAN, 2040, DIRTY, LADDER, SEM) · FX · Q · MODE · FX PLACE · FREQ. On the 2040: VOICE in MODE's place, CUTOFF and RES, and the TYPE readout names its one mode (4-POLE LP). On SEM: RES, and MODE is a knob that sweeps the output from low-pass through a notch at FREQ to high-pass |
| FMD | FM1 · FM2 · FM3 · FDBK (feedback) · QM, F empty. On the 2040: ENV AMT · KEYBD in FM1's and FM2's places. On SEM: F is OUTPUT, MODE (the MODE knob's sweep) or BP (band-pass) |
| FX | INPUT (the filter's input level) · the FX type's three pots · MIX · STABILITY (on the 2040, LADDER and SEM). SOFT: DRIVE · TONE · LEVEL. FOLD: FOLD · SYMMETRY · LEVEL. OFF: dashes, MIX dimmed |
| JACK | FM3 IN in C and QM IN in E: the rows FM3 and QM read |
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

Tabs PROJECT, LIBRARY and FACTORY (encoder 2). Rows: INIT ALGO, INIT MODAL, INIT VA, INIT TAPE and INIT DRUM (the last two COMING SOON), then the tab's Sounds. MINUS / PLUS change Part; SEQ goes back to the mixer.

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
