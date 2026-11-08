# 0225. Bind the master FX's knobs and ORBIT's ring amounts in the Part's map

- **Status:** Proposed (supersedes in part ADR 0216: "Every other address is reserved and inert")
- **Deciders:** owner (0.3 scope: "get all 0.3 tickets done"), implementer

## Context
The MIDI map's whitelist (`Mappable`, ADR 0139, 0217) took a Sound's own params and the Part's LEVEL and PAN. #512 asks for FX and ORBIT targets (spec `2026-10-13-midi-map-design.md` § 12). The master FX and ORBIT are not a Part's: one copy serves every Part. ORBIT's params also edit through `OrbitSong::edit`, never a Part's blocks (ADR 0083), so the bind path could not reach them. ADR 0216's chart marks every non-`Mappable` address `R`.

## Decision
- **Targets.** `Mappable` adds:
  - every stored, non-patching, continuous param of CHORUS, DELAY, REVERB, the master tape stage and the compressor;
  - per ring, ORBIT's amounts: HITS, ROTATE, CHANCE, MUTATE, GATE, VEL, ACCENT, OCTAVE.
  - Left out: the FX's stepped params (TYPE, MODE, SYNC…), STEPS, MUTE, a ring's choices (RANGE, SNAP, MODE, WALK, TARGET), the chord ring and BPM: switches that reshape a bus, a ring or a song.
- **Ownership.** A binding to a global target lives in one Part's map, like any other, and listens on that Part's channel: the Part it was learned on owns it. Two Parts may bind the same target; both move it, last message wins. The hand wins over the active Part's bindings in its pass.
- **Paths.** An FX knob ramps through the edit path (lerped, ADR 0139); a message that starts a glide on a shared target stops every other Part's glide on it, so two never alternate. A ring's HITS (0 to STEPS) and ROTATE (0 to STEPS − 1) bin over the ring's current range, so a fader covers it once. A ring amount is binned and stepped through `OrbitSong::turn`, so the playing scene and its slot move together, as a turn on its page; `stored` reads ORBIT from the live scene, so echo and pickup see it. All of it is bank 2's, behind `MidiUiDoor`.
- **Learn.** MIX + SEQ arms on ORBIT's pages as on a Part's (`OrbitKey::MixSeq`); any other ORBIT key or a page change cancels it; a turn moves the focus. The Part is the active one.
- **MIDI MAP** names a global target's block first: `REVERB MIX`, `RING 1 CHANCE`.
- **NRPN.** The canonical addresses of the new targets become `W` (76 lines of `nrpn_v1.txt` flip `R` → `W`; no address or law moves). This supersedes in part ADR 0216's "Every other address is reserved".

## Alternatives considered
- **A global channel for global targets** (ADR 0216's "a later ADR may open FX on a global channel"). It adds a setting and a second map with its own save, learn and listing; per-Part maps already reach them, and the owner's rule is a per-Part map (no global CC chart).
- **Binding a ring to the ring's TARGET Part.** A ring may move to another Part; the binding would silently change channel. Learn's Part is stable.
- **STEPS, MUTE and the choices as targets.** STEPS re-lays every hit, MUTE is a switch better left to MIX + B*n*, and a choice sweeping under a fader is a hazard on stage. They can be added later as button targets.

## Consequences
- Bank 1 gains the ORBIT learn keys (`orbit_learn`, out of line) and the ring turn's hand hook in `input`, with outliner churn: measured on main 913fcbd6, default, bank2-gate and usb-audio +392 B, the ORBIT build +152 B (`input` +148, `orbit_learn` +52, the rest churn); their lines rise by those bytes, and the bench sets stay under theirs. The moving code is bank 2's.
- A Sound saved with an FX or ORBIT binding carries it to any Part; it drives the shared target from there. Older firmware drops those entries on load (its whitelist refuses them).
- The NRPN chart doc is regenerated.

## Sources
- #512; spec `docs/superpowers/specs/2026-10-13-midi-map-design.md` § 12.
- ADR 0083 (scene edits), 0139, 0216, 0217.
- Elektron Digitakt/Digitone: FX and per-track params on the track's channel (track-owned CC map).
