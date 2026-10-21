# 0193. FLT › MOD and FLT › JACK use plain names on every filter TYPE (supersedes in part 0096 and 0190)

- **Status:** Proposed
- **Deciders:** owner

## Context
ADR 0096 named CLEAN's mod cells after the Cascadia's jacks (FM1, FM2), and ADR 0190 added FM3, QM, FM3 IN and QM IN. The 2040 already said ENV AMT and KEYBD, so one control had two names by TYPE and the jack names meant nothing without the Cascadia.

## Decision
Display strings only, on CLEAN, DIRTY, LADDER, SEM and the 2040 alike:

| Control | Was | Now |
|---|---|---|
| ENV 1 → cutoff | FM1 | ENV AMT |
| NOTE → cutoff | FM2, KEYBD | KEYBOARD |
| assignable source → cutoff | FM3 | FREQ MOD |
| assignable source → resonance | QM | RES MOD |
| FLT › JACK source for FREQ MOD | FM3 IN | FREQ SRC |
| FLT › JACK source for RES MOD | QM IN | RES SRC |

Full words where they fit the 70 px label slot (label face, 1 px tracking): KEYBOARD 66 px, ENV AMT 53, FREQ MOD 64, RES MOD 56, FREQ SRC 61, RES SRC 53. FREQUENCY MOD (107) and RESONANCE MOD (108) do not fit.

Saved Sounds, disk codes and idents (`FM3IN`, `QMIN`), param ids and MIDI-map identifiers are unchanged.

## Alternatives considered
- FREQUENCY MOD and RESONANCE MOD: wider than the slot.
- Keeping KEYBD on the 2040: KEYBOARD fits, and one name per control wins.

## Consequences
Everything that shows a name (cells, focus band, MIDI MAP, hints, docs) reads the new words. The Cascadia names survive only in ADRs 0096 and 0190 as history.

## Sources
Owner's approval of the label table; `chimera-core/src/ui/filter_panel.rs`.
