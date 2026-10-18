# 0147. The screensaver waits longer, and a waking touch acts

- **Status:** Proposed
- **Deciders:** owner (design), Claude (implementation)

Supersedes the timeout list, the default and the swallowed first touch of
[0127](0127-an-oscilloscope-screensaver-reads-the-notes-and-the-scope.md).
The rest of 0127 stands.

## Context
Testers found 7 s a distracting default and 60 s too short a maximum. One
dislikes screensavers because a touch is needed before the values come back,
and that touch did nothing: 0127 swallowed it, and the press through its
release.

## Decision
- SCREENSAVER steps: OFF / 5 / 7 / 10 / 30 / 60 s / 2 / 5 / 10 min. The
  first six keep their disk indices (0..5) and the minutes append (6..8);
  `B 26 6 SAVER` stores the index. Default 5 min (index 7), also for a SYSTEM
  with no SAVER record. OFF never starts it. Minutes are the range between
  "resting while I think" and "left the room"; 10 min caps it.
- Any button or encoder wakes the screen. A button also acts: the press, hold
  and release are the UI's, so one press navigates. A turn loses its first
  detent (the rest act, so n detents change the value by n - 1): a turn you
  couldn't see never changes the sound. MIDI still does not wake it.

## Alternatives considered
- Seconds-only steps up to 600: a long list to scroll for no gain.
- Keeping the swallow: it is the friction the testers named.
- Dropping 5 / 7 / 10 s: breaks stored indices.
- Acting on every waking touch: a turn could change a value the player has
  not seen (the saver shows the scope, not the page).

## Consequences
The first detent of a waking turn is lost, as 0127 did for the whole touch;
a button no longer needs a second press.

## Sources
Forum feedback 2026-10-16; issue #560.
