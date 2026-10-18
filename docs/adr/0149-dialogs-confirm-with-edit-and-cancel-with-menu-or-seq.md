# 0149. Dialogs confirm with EDIT and cancel with MENU or SEQ

- **Status:** Proposed
- **Deciders:** owner, from tester feedback (#563)

## Context
ADR 0081 gave every chain one D-pad: SEQ up, EDIT down, MENU leaves. It left
prompts and NAMING out as modal, "keeping SEQ to confirm and MENU to
cancel". A tester read that as two meanings for SEQ: back everywhere, OK in
a dialog.

## Decision
This supersedes the last paragraph of 0081 and the "SEQ confirms" lines of
0066 and 0077. One meaning per button, dialogs included.

- **EDIT confirms** the highlighted pill (EDIT is enter). It acts on the
  press, alone: EDIT in a chord with MIX or a B key confirms nothing
  (`Presses::edit`).
- **MENU and SEQ cancel** (MENU is leave, SEQ is back), on release.
- **A moves the highlight**, clamped. No button keeps a dialog-only meaning.
- **Legends:** prompts read `A PICK · EDIT OK · MENU BACK`, NAMING
  `EDIT SAVE · MENU BACK`. None says SEQ.
- **A prompt opens on its safe pill** (`Answers::START`). The destructive
  ones (DELETE, CLEAR, SAVE OVER, CLEAR and RELOAD SCENE, ENTER DFU, PURGE,
  MAP DELETE) open on CANCEL, so a reflex EDIT loses nothing. SAVE THEN
  LOAD, SAVE PART FIRST, KEEP BOTH, UPDATE and SAVE AS already lead with the
  safe pill.
- NAMING is a dialog: EDIT saves, MENU and SEQ cancel.

## Alternatives considered
- **Relabel SEQ as OK:** fixes the word, keeps one button with two meanings.
- **Cancel on MENU only:** SEQ is back everywhere else, so it would do
  nothing here.
- **Keep every prompt on its first pill:** a reflex EDIT on DELETE would
  delete.

## Consequences
A hand learns the D-pad once. The prompt-opening EDIT cannot confirm its own
prompt, since the press edge is spent before the prompt exists. Console
scripts that answered with `key SEQ` use `key EDIT`.

## Sources
Issue #563; ADRs 0066, 0077, 0081, 0086.
