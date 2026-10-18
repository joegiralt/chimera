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

SETTINGS › PERSONALIZE › CHAIN MAP › PAGES picks ROW or STACK.

---

## Part chains (B1-B6)

The chain follows the Part's engine. Every chain ends in MOD, and FLT and AMP are shared.

```
ALGO    ALG ── OSC ── DRV ── FLT ── AMP ── MOD
         └ PIT  └ CRS … FBK (12), OP1 … OP6 (6)
                 (a formant operator adds FRM first)

MODAL   EXC ── RES ── FLT ── AMP ── MOD
                └ MDL2, PIT

VA      VA ── DRV ── FLT ── AMP ── MOD
        └ OSC 1, OSC 2, PIT

shared  FLT └ FMD (FILTER MOD)
        AMP └ SYM
        MOD   MTX (matrix) └ E1 E2 E3, SPD, L1 L2 L3
```

TAPE has a chain in the code (SRC, DECK with its model page and PIT, DRV, FLT, AMP, MOD). In 0.1.0 and until it ships, TAPE rows in the Sound browser read COMING SOON and don't load.

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
PERSONALIZE   THEME, SCREENSAVER, CHAIN MAP
SYSTEM        OS UPGRADE, STORAGE (later), FORMAT CARD (later), USB CONFIG (later),
              DIAGNOSTICS, PUSH ENCODERS, ABOUT
```

THEME holds BRIGHT, GAMMA, ACCENT, BLACK, HINTS and INVERT. SCREENSAVER: OFF, 5, 7, 10, 30, 60 s, 2, 5, 10 min (default 5 min). DIAGNOSTICS: AUDIO LOAD, TEST TONE (later), INPUT TEST (later). Rows marked later are dimmed and labelled LATER; the bar lands on them, but EDIT does nothing.

---

## Not on the map yet

The chain editor, send-effect routing (SENDS under AUDIO ROUTING), SYSEX DUMP, STORAGE, FORMAT CARD and USB CONFIG are not built. Track them in the issue tracker, not here.
