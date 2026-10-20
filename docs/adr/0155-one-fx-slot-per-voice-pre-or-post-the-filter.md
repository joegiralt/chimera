# 0155. One FX slot per voice, PRE or POST the filter (supersedes in part 0037)

- **Status:** Proposed
- **Deciders:** owner (2026-10-17, rulings 2026-10-19), #614

## Context

A voice can run two shaping stages today: DRV before the filter and the AMP page's wavefolder after it. Folding a distorted signal makes little sense, and both stages bill the voice. The owner wants one simple effect per voice, chosen with where it sits around the filter: "you get to choose one effect … drive, a wave folder, the simplest of delays … and then you have your settings." The one that must convince is a fuzz: a VA bass pushed into a Big Muff, without the thin, fizzy sound of cheap digital distortion. The throwaway drive spike (branch `drive-spike`) showed "there's something there".

## Decision

- **One slot replaces two stages.** DRV leaves the chain (VA · FLT · AMP · MOD, ALGO · ALG · OSC · FLT · AMP · MOD, TAPE · SRC · DECK · FLT · AMP · MOD) and FOLD, FOLD MIX and SYM leave AMP. Every filter kind (CLEAN, DIRTY, LADDER, 2040, and LPG when it is built) carries the slot.
- **The stored block is VOICEFX** (`BlockRef::VoiceFx`, code 36, `VoiceFxParams`): TYPE 0, PLACE 1, P1 2, P2 3, P3 4, MIX 5. P1–P3 and MIX are `Smoothing::STAGE` (20 ms, they glide, ADR 0112/0117); TYPE and PLACE are `Change::Crossfade`.
- **TYPE** (`FxType`) is OFF 0, SOFT 1, FUZZ 2, FOLD 3, in that order on the knob; 4 is held for DELAY. OFF is a TYPE value, first in the list. The pots are cell positions, reinterpreted per type as filter KIND does:

  | TYPE | P1 · P2 · P3 | Default PLACE |
  |---|---|---|
  | SOFT | DRIVE · TONE · LEVEL | PRE |
  | FUZZ | SUSTAIN · TONE · VOLUME | PRE |
  | FOLD | FOLD · SYM · LEVEL | POST |

  P3 is a level, `pot_level` = the filter's `level_gain`: ×¼ to ×4, exactly 1 at ½.
- **PLACE** (`Place`) is PRE 0, POST 1. PRE runs between the source and the filter's input stage; POST runs after the filter, before the DC blocker and VCA. **PLACE follows a type only from OFF:** leaving OFF takes the type's default place, and any later pick keeps the PLACE you set (`VoiceFxParams::set_type`). A load sets TYPE and PLACE as stored (`set_enum_code` never moves PLACE), since a sparse record leaves out a PLACE equal to its base's.
- **INPUT LEVEL** is the filter's LEVEL, renamed (the cell says INPUT). It stays its own knob and drives the 2040's stages whatever PLACE says; in PRE, the effect's level and INPUT stack.
- **Layout A, per kind:**

  | Kind | FLT front | FLT › MOD | FLT › FX |
  |---|---|---|---|
  | CLEAN, DIRTY, LADDER | KIND · MODE · FX · PLACE · FREQ · Q | FM1 · FM2 · FM3 · FM3 IN · QM · QM IN | INPUT · P1 · P2 · P3 · MIX |
  | 2040 | KIND · VOICE · FX · PLACE · CUTOFF · RES | ENV AMT · KEYBD · FM3 · FM3 IN · QM · QM IN | INPUT · P1 · P2 · P3 · MIX |

  The 2040's fixed MODE cell is dropped, which supersedes in part ADR 0037 (its panel kept that dimmed cell): it can't change, so KIND's focus band names it (4-POLE LP). FLT › FX draws a strip of where the effect sits and the type's curve. With TYPE OFF, the pots read as dashes and MIX dimmed. PLACE has its own glyph (an FX box and a FLT box joined by an arrow, the lit one the effect).
- **Routed cells are matrix views.** ENV AMT, KEYBD, FM3 and QM on the filter pages are views of MOD matrix cells, as today. The matrix's fixed FOLD column becomes **FX** (`FixedCol::Fx`) and moves the current type's P1; ALGO's OP 5/6 taps on it act only while TYPE is FOLD, and are dimmed and unbilled otherwise.
- **Each type bills its real cost** in the voice's admission bill: OFF nothing, SOFT today's DRV, FOLD today's fold, FUZZ its own measured bill. A **TYPE or PLACE change crossfades over 20 ms** (`FX_FADE` = 960 samples, the stage gate's): old and new run together, and the fade blocks are not billed, as with the filter's fade. A change arriving mid-fade waits for its end; the last one wins.
- **A fixed reference level per engine, `EngineType::fx_scale`:** the signal is multiplied by it into the slot and divided by it out, so the same knob position bites the same on every engine and a stage at MIX 0 is the dry signal. Never a running normaliser.
- **Old Sounds load safely, not the same.** DRV's DRIVE, TONE and MIX load as SOFT PRE; the fold's FOLD, SYM and MIX as FOLD POST. If both ran (`Voice::stage_runs`), **DRV wins** and the fold is dropped. Routes and MIDI-map entries on DRIVE, FOLD and SYM move to P1 or P2 by position. Blocks 9 (DRIVE) and 11 (FOLDER) are retired, never reused.
- **LPG's KIND code 7 and ident `LPG` are reserved** for the low-pass gate (built with DRUM). It is not selectable, and nothing loads as it.

## Alternatives considered

- **Keep the two stages (DRV and FOLD) and add FUZZ as a third.** More cost a voice, a chain step most Sounds never use, and folding a distorted signal is rarely wanted.
- **More pedals now** (OVERDRIVE BD-2 style, DISTORTION RAT style, both rendered in the spike): they can return as TYPEs.
- **Amp sims, cabinets and neural models:** out of scope (BYOD's catalogue is GPL besides).
- **A per-Part effect after the voices:** the Part FX bus already covers that; the slot is about the filter's input.
- **Two effects on one voice:** the cost and the panel of two stages, which this replaces.
- **PLACE always following TYPE:** it would throw away a PLACE the player set each time the type changes.

## Consequences

- One block, six params, and one more stored code; DRIVE and FOLDER retire.
- `ParamSnapshot` grows 20 B (`VoiceFxParams`), and `Voice` holds its played copy, so a `Voice` less its slot is 2,184 B: **`VOICE_CHAIN_BYTES` 2,176 → 2,240**, the next 64 (ADR 0051/0101/0131's rule). Retiring DRV and the fold's state and params is expected to free more than that; to be measured in Tasks 6 and 10.
- A FUZZ Part gets fewer voices only near the CPU limit, through the admission bill.
- DELAY can come later as a TYPE in the same slot, with code 4; its cost is memory (a buffer a voice).

## Built

- PLACE has no large focus-band glyph: it sits on FLT's front page, which is a BigViz with no focus band. Its cell draws the two boxes small (`MiniGlyph::Place`) and its focus behaviour is the word's.
- FLT › FX's band (strip and curve) draws from bank 2 through the door (`voice_fx::look`); with bank 2 closed the band draws nothing extra and the cells still work. FUZZ's curve is the kernel's own.
- TYPE and PLACE are not MIDI-mappable (switches); P1-P3 and MIX are.

## Sources

- Spec `docs/superpowers/specs/2026-10-17-fx-in-filter-design.md` (chimera-fx-plan) and plan `2026-10-19-fx-in-filter.md`; issue #614.
- ADR 0045 (card format, frozen codes), 0051/0101/0131 (voice chain bytes), 0096 and 0104 (filter types), 0112 and 0117 (motion and knob glide), 0132 (COMING SOON).
- The drive spike: branch `drive-spike`, the private "Drive Spike" page (layout v2 mockups, 35 loudness-matched clips).
