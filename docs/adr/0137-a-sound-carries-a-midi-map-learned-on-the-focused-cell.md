# 0137. A Sound carries a 16-entry MIDI map, learned with MIX+SEQ on the focused cell

- **Status:** Proposed
- **Deciders:** project owner (2026-10-13: "go with your recommendations"), firmware

## Context
ADR 0128 left every CC except bend and CC 64 dropped until the per-Part MIDI map (#247). The issue asked for learn by "holding a cell's button". On this hardware that has no referent: B*n* acts on press outside ORBIT and is ORBIT's cross inside it, and the encoders have no push switch. An adversarial review (private notes) found three blockers:
- CCs on the note queue would starve note-ons.
- More than `MOVES` (6) knob changes in one publish land every knob.
- A 7-bit CC step on CUTOFF is 1.9× an encoder detent.

The owner approved its eleven recommendations on 2026-10-13. Spec: `docs/superpowers/specs/2026-10-13-midi-map-design.md`.

## Decision
- **Transport.** Non-reserved CCs go through a 64 × `u32` SPSC CC ring per source, filled by the MIDI ISR through a pure `midi_in::route`. The UI drains it once per loop iteration. The note queue keeps notes, bend, CC 64, channel mode and real-time.
- **Where the map lives.** It is in the **Sound**, not the Part: `Sound::map`, at most 16 `MapEntry { cc, target: Mappable, min, max }`, with `min`/`max` as places 0..255 and `min > max` inverting.
  - It is stored as the non-critical Sound record **`MidiMap`, tag `0x001E`**: 5 B an entry, written last, omitted when empty. `0x001D` is `showcase-library`'s `Note`.
  - `MAX_SOUND_COST` goes 2,560 → 2,640.
- **Targets.** `Mappable` (a `ParamAddr` newtype) accepts `in_sound` params that aren't `patching` or `live`, plus Part LEVEL and PAN. It refuses FX, ORBIT, settings and the rest of the Part's mix.
- **Learn.** MIX+SEQ arms learn on the focused cell. SEQ's gate is muted under MIX.
  - While armed, a turn moves the focus, never a value. The first CC on the Part's channel is learned and not applied.
  - The outcome is shown in place, as ADR 0017's. MIX+SEQ, a nav key or a page change cancels. There is no timer, and no learn in ORBIT or SETTINGS.
  - CCs 0, 32, 6, 38, 96–101, 64 and 120–127 are refused.
  - The hint is `MIX + SEQ  LEARNS A CC`.
- **Values.** Values are absolute 7-bit and jump; there is no pickup.
  - Continuous targets ramp on the UI side over the measured inter-arrival time, clamped to 15–120 ms (15 ms after silence), then glide through the existing 30 ms knob glide.
  - At most `MOVES − 1` knob writes go out a Part a pass, round-robin, so nothing lands.
  - Stepped targets are binned with ±2 counts of hysteresis and stepped through the encoder's edit path, so budget refusals and crossfades hold.
  - Equal repeats are ignored, and a ±1 reversal within 50 ms is a dead band.
- **Channel.** Per Part, with no omni. Parts sharing a channel each apply their own map.
- **The MIDI MAP page.** It is a `Screen` under SETTINGS › PART, drawn by the SETTINGS list renderer. Rows read `CC 74  CUTOFF  0–255`. A, B, C and D set the row, CC, MIN and MAX; MIX+MINUS deletes. Entries for another engine are kept and dimmed.
- **The default map.** INIT and every factory Sound carry it explicitly as data:
  - 74 CUTOFF, 71 RESONANCE;
  - 7/10 Part LEVEL/PAN;
  - 91 SEND3 (reverb), 93 SEND1 (chorus);
  - 1 LFO1 DEPTH, with LFO1 → PITCH primed at 0 in INIT;
  - 16–19 the engine's four macros.
- **Latency.** Alpha accepts UI-loop latency. A sample-accurate audio-side path is post-alpha.
- **Superseded in part:**
  - the reverb spec's MIDI MAP lines (`2026-10-04-reverb-types-design.md:753,1077`);
  - ADR 0085's MIDI MAP bullet: FX aren't mappable in alpha;
  - product architecture § 10.3's default CC chart and its "~5 ms" smoothing.

## Alternatives considered
- **The map in the Part (the Octatrack and DAW model).** The rig survives preset browsing, but engine-specific targets (VOWEL, STRUCTURE) vanish across engines. The owner and the library spec chose the Sound. A shared default map covers browsing, and `KEEP MAP` on load is post-alpha.
- **A fixed CC chart (Elektron, stock PreenFM3).** Ruled out by the owner on 2026-09-30 (ADR 0128): too many params across too many engines.
- **CCs on the note queue.** A busy controller drops note-ons, and the audio thread can't edit the Sound.
- **An ISR-side latch (16 ch × 128 CC).** It costs ~2.3 KB. It is kept for the day USB floods show losses.
- **14-bit or NRPN for smoothness.** The ramp makes a 7-bit sweep continuous, and 14-bit only adds static precision. Post-alpha.
- **Pickup or scale modes.** These confuse players and cost UI. The on-unit encoders are endless, so jump is honest.
- **Learn by holding B*n*.** It would slow every Part switch to a tap on release.

## Consequences
- **Flash** is ~5–8 KB, under the ~40 KB escalation line the owner accepted. #460 (bank 2) is the next lever.
- **RAM** is ~2.5 KB UI-side. `AudioShared` is unchanged.
- **Every Sound record context** carries the map with no project code. Older builds skip it.
- **MIX+SEQ in ORBIT** no longer saves the scene. It does nothing.
- **Sequencer p-locks** reach the Part a UI iteration late, and smear when legato. This is filed post-alpha.
- **Merge order.** The CC ring is built on `all-notes-off`'s `from_midi`. Showcase-library's factory Sources get the default map after both branches merge.

## Sources
- #247; ADR 0017, 0044, 0045, 0081, 0083, 0085, 0086, 0087, 0102, 0112, 0116, 0123, 0128.
- `docs/superpowers/specs/2026-10-13-midi-map-design.md`; `docs/superpowers/plans/2026-10-13-midi-map.md`.
- Precedent:
  - Deluge learn: hold LEARN, touch, move;
  - Ableton MIDI map mode;
  - Elektron CC charts and their jump behaviour;
  - Octatrack Part-level CC slots.
