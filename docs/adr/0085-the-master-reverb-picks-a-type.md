# 0085. The master reverb picks a TYPE: QUAD plus ROOM, HALL, GATE, REVERSE and BLOOM

- **Status:** Proposed; superseded in part by [0095](0095-every-reverb-type-matches-its-midiverb-ii-presets.md) (ROOM's and BLOOM's designs, the provenance rule for structure numbers, and the ±30 % A/B pass); supersedes in part [0028](0028-one-alesis-style-reverb.md) (its "one reverb", "TYPE retires" and GRIT, which leaves the reverb); amends [0067](0067-focus-glyphs.md) (the reverb composite); SMEAR's bound and depths amended by [0121](0121-smear-is-bounded-by-the-pitch-it-plays-and-rate-runs-to-2-hz.md)
- **Deciders:** project owner. The rulings are those of 2026-10-03 on #343 and these of 2026-10-04:
  - QUAD stays exactly as it is, and keeps its name;
  - modulation is deferred to #345;
  - "sound first" on CPU;
  - ECHO is dropped;
  - reverbs stay pitch-stable;
  - GRIT leaves the reverb, and SMEAR, a random wander, replaces the wobble on every TYPE, QUAD included.

  Firmware drafted it.

## Context
ADR 0028 cut the FX bus to one reverb, the Alesis-style ring, and retired
TYPE. The cut was for cost: there were three reverbs by TYPE, each of them
costly. The ring reads 485 cycles per 48 kHz output sample at its worst, on
the bench's REVERB row (ADR 0031).

The owner now wants more spaces. The rulings of 2026-10-03:
- a TYPE setting, with only the selected model running;
- TYPEs that are families, not presets;
- one shared delay arena that does not grow;
- about 1–3 KB of flash per TYPE;
- a switch with no click and no stale echo;
- sound names on screen;
- the method: study the Midiverb II through github.com/thement/midiverb_emulator
  (GPL-3.0) as documentation only, and write our own code;
- extensions the hardware never had: modulation of the loops, mod destinations and tempo sync.

The rulings of 2026-10-04 add four more:
- "Sound first": QUAD's cost becomes the target rather than a cap.
- Modulation waits for a mixer-level design (#345).
- ECHO is dropped, since the master tape delay covers it. Its Midiverb
  design moves to a per-Part echo, filed separately.
- Reverbs stay pitch-stable. A tail is never re-pitched pass after pass,
  and the tape-echo pitch dive belongs to that future per-Part echo.

The #343 spike grouped the Midiverb II's 100 programs by dataflow graph. It
found three reverb engines: diffuse loops, tap bursts and an allpass bloom.

## Decision
- **The TYPEs.** The master reverb has a TYPE: Reverb ParamId 6, a
  `ReverbType` enum with explicit disk codes. The values, with their codes:
  - QUAD 0, today's ring, unchanged;
  - ROOM 1, the LOOP engine, 2 loops × 2 stages;
  - HALL 2, the LOOP engine, 1 loop × 4 stages;
  - GATE 3, the BURST engine;
  - REVERSE 4, the BURST engine;
  - BLOOM 5, the BLOOM engine.

  QUAD keeps the word QUAD on screen, the owner's name for it. The screen
  lists the TYPEs QUAD, ROOM, HALL, BLOOM, GATE, REVERSE. ParamId 0 stays
  retired (ADR 0009). Code 6 was never assigned.
- **One engine runs at a time.**
  - All TYPEs share the 23,220-sample i16 arena (46,440 B), lifted out of
    `Ring` unchanged.
  - Each TYPE's line layout is a `const` asserted to fit.
  - All TYPEs run at half rate behind the existing half-bands, with ADR
    0028's Q3 quantiser at a fixed 1 LSB.
- **GRIT leaves the reverb** (owner, 2026-10-04).
  - **Why.** Today's GRIT requantises every write inside the feedback loop,
    undithered, so tails break into "crinkling noise on top". A converter
    model was drafted, then moved out of the reverb altogether.
  - **What stays.** No converters and no `Grid`. The arena keeps plain i16
    storage with fixed rounding (Q3 at 1 LSB), which has a grain of its
    own, like the Midiverb's memory.
  - **Where converter colour goes.** Bits, high cut, noise floor and
    compander grain become a future master-stage feature, filed as its own
    issue. The converter code is kept on branch `converters-wip`.
  - **The id.** Reverb ParamId 5 (`GRIT`) is retired, as id 0 is: `(22, 5)`
    joins `RETIRED`, and an old project's stored GRIT loads safely and is
    ignored.
  - **Bit-identity and cost.** At GRIT 0, `Grid` was exactly the fixed
    quantiser, so every GRIT-0 output is unchanged. Dropping its per-write
    scaling saves 14–56 instructions per ring sample.
- **SMEAR replaces the wobble** (owner, 2026-10-04), after the Lexicon
  480L's Spin/Wander and Random Hall programs.
  - **How it moves.** Every modulated delay line in a loop gets its own
    slow random drift: per-line random targets every 1/(2·RATE), offset in
    phase, from one xorshift32 and smoothed by a 2-pole low-pass at RATE,
    once a block.
  - **What it sounds like.** No two lines move together and none moves
    periodically, so the tail smears into a wash with no vibrato.
  - **Bounds.** Reads only shorten, within [d − 2D, d], so no line needs
    more slots. The depth is capped so the loop's worst-case total
    excursion is ≤ 1.45 % of its round trip (25 cents), and the mean pitch
    over a window of 4/RATE stays within ±1 cent.
  - **Cost.** About 0.4 instructions a sample per line, on top of each
    moving read's pair.
  - **The knobs.** SMEAR takes slot 11 (ident `SMEAR`), and RATE stays at
    12 as the wander speed.
  - **QUAD gets SMEAR** on its four stage delays, its depth scaled to the
    ring: D = SMEAR × 0.18 % of the round trip, so SMEAR 1 is the 25-cent
    cap at every SIZE and the whole knob does something. A ±0–2 ms law
    capped there went flat from SMEAR 0.3 at SIZE 0, since the cap is
    only 0.61 ms there (1.74 ms at SIZE 1).
    - Its main page becomes SIZE · TIME · DAMP · SMEAR, with RATE on its
      MORE page, so RATE sits in the same place on every TYPE.
    - At SMEAR 0, QUAD runs its existing code path and is bit-identical to
      the Task 4 reference. That is its default.
    - SMEAR and QUAD's SIZE crossfade never run together: the depth eases to
      0 for a SIZE move. So QUAD's worst case stays its crossfade, and a
      bench row of its own, REV QUAD S, gates the SMEAR path.
- **PLATE is a zone of HALL, not a TYPE.** A plate on HALL's graph differs
  only in constants, so by the owner's rule it is a preset.
  - HALL's SIZE reaches a 120 ms loop.
  - Below SIZE 0.15, a plate zone pulls DIFFUSE up, and TONE and PRE-DLY to
    the plate's values. DIFFUSE and TONE ease per block, and PRE-DLY moves
    through the same 5 ms crossfades as SIZE. The zone is about 150 B of
    flash.
  - The plate corner is a tested golden.
- **QUAD stays exactly as it is** (owner, 2026-10-04): the same sound, the
  same cost, and its double-read SIZE crossfade.
- **Lengths move by pitch-free crossfades, never by glides** (owner,
  2026-10-04: reverbs stay pitch-stable). This supersedes the earlier
  ruling that the new TYPEs glide SIZE through interpolated read pointers.
  - **Why not a glide.** A gliding read inside a loop re-pitches the tail
    on every pass. Over a move, a circulating component shifts by the whole
    length ratio: about 28 semitones for HALL's 120 → 600 ms.
  - **How lengths move.** Every length a knob or the tempo moves (SIZE,
    LENGTH, SWELL, PRE-DLY) crossfades between two fixed integer reads,
    as QUAD's do.
    - In LOOP, all stage delays switch in one crossfade, so the round-trip
      sum moves together. Each 30 ms crossfade moves it at most 5 ms, as
      QUAD's steps do, so a long move is many steps. A larger step would
      duplicate or drop a slice of the tail on every pass, which is a ghost
      slap.
    - In BURST, at most 4 taps crossfade at a time. Tap 11 is REGEN's
      feedback read, and it crossfades like the others, in steps of at most
    5 ms.
  - **The bounds.** No read is resampled, so a move leaves no accumulated
    shift. Once it settles, the tail's spectral peak sits within 1/L of the
    input's. While a crossfade runs, the blend of two reads 5 ms apart
    slides the phase: up to about 29 cents instantaneously, about 7 averaged
    over a 120 ms pass. That is stated, not hidden.
  - **SMEAR** is the only continuous motion. Inside a loop it is capped so
    the loop's worst-case total excursion is ≤ 1.45 % of the round trip.
    A component then stays within 25 cents over any number of passes, with
    no drift. Outside loops (BURST's decorrelators) it is a single pass of
    drift, ≤ 22 cents.
  - **Tests** check both, per pass and cumulatively.
- **Cheaper as well as stable.** A crossfading line costs one more fixed
  read only while it moves, where a glide's moving pair cost 17–20
  instructions every sample. Moving pairs remain only for SMEAR.
- **Reads and loops.**
  - Output taps are fixed. One early tap per side sits at a fraction of the
    SIZE-0 length, and the others at fractions of the maximum length.
  - Tap 0 of BURST is the input register.
  - Every read stays ≥ 2 samples behind the write head.
  - Any loop without an allpass (REVERSE's REGEN) truncates its feedback
    write, so no ±Δ limit cycle survives.
  - ROOM couples its loops through an energy-preserving rotation.
  - New engines cap a stage gain at 0.985 (QUAD keeps 0.97).
  - **Input gain is ½ in every engine, as in QUAD.** The level plan below
    covers the arena. A level plan keeps
    every stored value inside ±2.0 at maximum DECAY, by four rules:
    1. a branch-free soft clip (knee 1.0, reaching 2.0 with zero slope) at
       every loop's re-entry;
    2. normalised state storage, w = (1 − g)·v, for every allpass inside a
       loop. Its bound holds at any g, so in-loop g runs up to 0.75 again,
       the Midiverb's ¾ density, by DIFFUSE;
    3. f32 diffusers outside loops. Their worst-case gain compounds at
       1 + 2g per allpass, and an f32 store costs nothing over the
       quantise it replaces;
    4. f32 output decorrelators, after sums that can exceed 2.0.

    Both f32 kinds live in the arena's own free slots.

    The bounds are H∞ (held-sine) bounds. Transient overshoot is caught by
    the saturating store and bounded by a clamp-hit test.
    - At the default DECAY a −12 dBFS program is bit-identical to an
      unclipped build. At maximum DECAY the clip shaves peaks at ≤ −40 dB.
      A held loop-mode tone stays ≤ 3 % THD, with aliases ≤ −40 dB.
    - The arena's noise floor stays within +4 dB of QUAD's estimated
      −66 dBFS at the defaults, and within +6 dB at DIFFUSE 1. That
      density-against-noise trade is an A/B item.
    - Scaling the send down instead (HALL to 0.16) was rejected: about
      10 dB more audible grain in quiet tails.
- **CPU: "sound first"** (owner, 2026-10-04).
  - **The target** is QUAD's worst case, 485 cycles per output sample.
  - **The hard limit** is the cost model's headroom inside the audio
    budget. The budget is 70 % of the CPU (`AUDIO_BUDGET_PERCENT` in
    `hw.rs`, ADR 0020): 7,000 cycles per sample on rev V and 5,833 on
    rev Y. It keeps the 30 % reserve for UI, MIDI and interrupts whole.
    - The headroom is the minimum, over every voice-count assertion in
      `cost_test`, on both revisions and both builds, of
      budget − `FxBus::COST` − n × voice.
    - Task 1 measured it at **76 cycles per sample in the default build**
      (with `COST` 1,188: BOWED, 717, eight voices on rev V) and **23 with
      `master-tape`** (MORPH KEYS, 864, five voices on rev Y).
  - **The gate is ΔCOST ≤ headroom**, where ΔCOST is the costliest TYPE's
    BUS row above QUAD's. `cost_test` itself is the gate, so the margin is
    never duplicated or halved: `FxBus::COST` takes the costliest TYPE's BUS
    reading, and the voice-count assertions must still pass on both builds.
  - **If step 0's QUAD re-baseline**, measured with voices, alone breaks the
    tape build's 23, it goes to the owner before any TYPE work.
  - **BLOOM's split-SWELL fallback**, about +33 by the review's cache-aware
    estimate, fits the default build but not the tape build. There it needs
    an owner ruling. BLOOM's base design, about +14, fits both.
  - **The stress run is a second, real gate:** six admitted voices, every
    FX, ORBIT and the UI, with OVER 0, DROPS 0 and a peak block load under
    100 %.
  - **The room is small.** A TYPE may exceed the target when the A/B shows
    its cheaper design sounds wrong, but only within the headroom. Beyond
    it, a TYPE needs a cheaper design or an owner ruling that gives up a
    voice guarantee. Cheaper fallback designs are a last resort.
- **One block, semantic slots.**
  - The Reverb block stays one static table: 12 live ids of the codec's 32.
  - Ids 1–4 keep their idents, and 0 and 5 are retired. Ids 6–13 are TYPE,
    TONE, DIFFUSE, PREDELAY, SHAPE, SMEAR, RATE and SYNC.
  - A per-TYPE `const` panel gives each slot its label, range, unit and
    encoder step, or marks it absent (ADR 0037's pattern).
  - Slot 3's static step is 1/128. QUAD's panel keeps its 1/31 tick.
  - A UI edit of TYPE loads that TYPE's knobs and keeps MIX and SYNC.
    A TYPE visited before gets its last knobs back from a 108 B RAM memory
    in `UiState`, which is never saved. Decode and CC writes store TYPE
    alone.
  - MIDI MAP shows the current TYPE's panel labels. Labels of 7 characters
    get short forms of ≤ 6 (ADR 0053's rule).
  - Value texts are in units, QUAD's included (`663ms`, `1.9s`, `6.1k`;
    MIX `0%`), each short enough for its cell and for the focus band beside
    the glyph: no face steps down. HALL's plate zone shows `PLATE` in the
    SIZE cell's label, not its value.
  - A TYPE whose engine has not landed (set by a CC or a newer project)
    stays stored and plays QUAD, and its page shows QUAD's panel.
- **The switch.**
  1. The old engine runs on knobs latched when the switch starts. Its
     return ramps linearly to 0 over 20 ms.
  2. Four blocks clear the arena, a quarter each, with no engine running.
     Zeroing it all in one block would cost about 11.6 k cycles, bound by
     the bus, and evict the D-cache: more than the master tape's headroom.
     A quarter costs less than the engine work it replaces. The last block
     clears the engine state and the half-bands' histories too.
  3. The new engine starts fresh and ramps in over 20 ms.

  A test holds the new engine bit-equal to a fresh engine from its first
  block. An on-chip REV SWITCH MAX row, with voices playing, takes the
  maximum block over forced switches, in the default and master-tape
  builds.
- **The extensions.**
  - **SMEAR** on every TYPE, QUAD included.
  - **No external mod destinations** (owner, 2026-10-04): every mod source
    lives inside a Part, and a mixer-level LFO or matrix is
    private issue #345. #345 must
    supersede ADR 0010 for bus destinations, decide what a semantic slot
    means as a destination, and move any modulated length by crossfade.
    Disk needs no change, because routes are stored by `ParamAddr`.
  - **SYNC** (OFF, TIME, ALL), on ORBIT's clock: PRE-DLY and LENGTH in note
    values, and DECAY in beats under ALL.
    - A synced time halves or doubles to fit its TYPE's range, with 2 %
      hysteresis.
    - The tempo is quantised to 0.25 BPM.
    - A division is stored as its index over the slot's 0..1. A UI SYNC
      toggle converts the slot so the sound stays close.
    - ROOM has no ALL.

    QUAD takes none of these.
- **The glyph.** `CompositeId::ReverbCube` becomes `CompositeId::Reverb`.
  It draws the cube under QUAD and a TAIL envelope under the other TYPEs,
  from set values only. This amends ADR 0067's "the reverb shows the cube".
- **No hints.** The reverb adds no gesture, so under ADR 0086 (Proposed, on
  the ORBIT branch) no reverb cell has a hint. A folded synced time and the
  plate zone are shown in the value text.
- **Projects.** Old projects lack id 6, so they load as QUAD and sound as
  before. New projects default to QUAD.
- **Provenance.**
  - No code, instruction listing, or ROM coefficient, offset or tap table
    from the emulator or from commit 7119f7ad enters the repository, whole
    or in part.
  - Lengths are our own primes from `const fn`, and gains come from RT60
    formulas. Tap positions, signs and ranges are chosen in the spec, then
    tuned by ear and by measurement against the Midiverb VST.
  - Only prose structure and acoustic measurements carry over.
  - Every engine task's review checks this.

## Alternatives considered
- **One "80s" Midiverb model (the issue's first framing):** one graph cannot
  be a room, a gate and a bloom. The families are what the hardware's
  presets actually were.
- **A faithful MV2 port, reviving 7119f7ad:** GPL-derived code and tables
  in what was then an MIT workspace (GPL-3.0-or-later since ADR 0330). It was also retired once already, for cost.
- **PLATE as its own TYPE:** a preset presented as a family. It could be
  added later as a new code mapped to HALL, for about 200 B.
- **ECHO as a sixth TYPE:** dropped by the owner. The master tape delay
  covers it, and its pitch-diving character belongs to a per-Part echo, not
  a pitch-stable reverb.
- **Per-TYPE parameter ids:** about 48 ids against the codec's 32-param
  cap. Shared slots plus panels need 13.
- **Gliding lengths through interpolated read pointers** (this ADR's
  earlier draft):
  - it re-pitches a circulating tail by the whole length ratio over a move,
    and slowing the glide only spreads the shift out;
  - every moving read pays 17–20 instructions all the time, and gliding
    every BURST tap reached about 330 instructions against QUAD's 239.
- **SIZE-scaled LOOP taps:** about +27 cycles over QUAD in ROOM. That fits
  the default headroom (76) but not `master-tape`'s (23); the ear judges the fixed
  taps at A/B.
- **Running both engines through a switch crossfade:** twice the cost for
  20 ms. Fading out, clearing and fading in costs one engine.
- **A sine wobble:** a periodic, audible vibrato, and lines moving together.
  SMEAR's independent random wander smears instead.
- **GRIT as converters inside the reverb:** drafted and built, then moved
  out by the owner. It is a master-stage feature, not a reverb one.
- **Growing the arena so long synced times fit:** the owner ruled that
  nothing grows. Halving to fit stays on the grid.

## Consequences
- **CPU.** These are estimates. Each counts a fixed access at 5.0
  instructions and a SMEAR pair at 17–20. Host instructions per ring
  sample are against QUAD's 239, and cycles per output sample against 485:

  | TYPE | Instructions (pair +7 / +10) | Cycles |
  |---|---|---|
  | QUAD, crossfading | 183 | 380 |
  | QUAD, SMEAR 1 | 152 / 164 | 323 / 345 |
  | ROOM | 206 / 212 | 423 / 434 |
  | HALL | 196 / 202 | 405 / 416 |
  | GATE, REVERSE | 193 / 199 | 399 / 410 |
  | BLOOM | 187 / 196 | 388 / 405 |

  These include the soft clips, the normalised states, SMEAR, and GRIT's
  removal. Every TYPE is under the pre-refactor QUAD row (485), which
  `FxBus::COST` bills, so ΔCOST ≤ 0 and both headrooms (76 and 23) are
  untouched. ROOM-T stays pre-written in case step 0's bench disagrees:
  1. smear loop B's allpass only, saving about 22–28 cycles;
  2. then 2 output taps a side, about 19 more.
  `FxBus::COST` changes only if the bench confirms it.

  Step 0 comes before any TYPE work. It:
  - re-measures the baselines, with voice traffic;
  - records the headroom from `cost_test`;
  - runs the stress run with QUAD;
  - measures a moving pair's cost;
  - benches the worst-case BURST, ROOM-loop and HALL-loop kernels on the
    chip.

  After the refactor, REV QUAD must read ≤ its baseline + 1 %.
- **Memory.** The arena does not grow. `FxBus` gains one engine-state enum
  (≤ 160 B), `FxParams` about 26 B, and `UiState` 108 B of knob memory.
- **Flash.** Estimated at 9.5–12.5 KB in all: about 3–4 KB of shared
  scaffold, then 1–2.5 KB per TYPE. Each step's fat-LTO delta is recorded,
  and a TYPE over 3 KB is cut back before the next one starts.
- **BURST, as built (Task 8, 2026-10-04).** The following departures from
  the spec are accepted.
  - **DENSITY keeps taps {0, 2, 4, 7, 9, 11} and fades {5, 6, 3, 8, 1, 10}
    in, each over a third of the knob.** The spec faded taps 6–11 at
    `clamp(6·D − i)`. That moved a weight 3/64 a tick, against its own
    1/64 bound. It also faded REVERSE's loudest taps, and it muted
    GATE's tap 11 at the default DENSITY, so the burst ended before LENGTH.
  - **The decorrelators share their first allpass** (3.1 ms), then L 8.3
    and R 6.1 ms; the 4.7 ms allpass is dropped. Two independent g 0.6
    chains correlate at about g⁴ (0.13), under the spike's 0.4–0.65.
    Sharing the first gives about g² (measured 0.38), and each side stays
    an allpass of the sum. A dry share in each side echoed it at 3–8 ms.
  - **The tap weights step once a block, eased over 20 ms, not ramped
    across it.** That saves about 400 B, and a load, an add and a store
    per weight per ring sample. The knob-ease test holds.
  - **BURST is laid out for the 24 kHz ring.** At another bus rate its times
    scale with the rate, as QUAD's lengths do.
  - **SHAPE and CURVE read in whole dB.** Tenths outran the focus band.
  - **Level: GATE and REVERSE match QUAD by short-term loudness on a held
    input,** not by the RMS of a burst's 3 s. Matching the burst's RMS left
    them hot on sustained notes. The test is the loudest 400 ms of a held
    −12 dBFS pink pad at MIX 0.5, within ±1 dB, with the peak at most 1 dB
    over QUAD's. WET is 6.75 for GATE and 7.05 for REVERSE.
- **ROOM, as built (Task 9, 2026-10-05).** The following departures from
  the spec are accepted.
  - **A crossfade moves one stage delay a loop, by at most 5 ms, and every
    loop in the same crossfade:** the delay farthest from its goal. Moving
    both delays of a loop by k stepped them by unrelated amounts (44 and 74
    samples), so their two combs didn't line up, and a held tone's phase
    scrambled: its settled peak sat 28 Hz off at a 5 Hz mode spacing. One
    5 ms slice is the spec's own comb at 200 Hz spacing; the peak settles
    within 1/L, and moves 3.9 cents at most while it slides.
  - **The re-entries and the in-loop allpass states write by
    `store_settle`:** rounding, but 0 under 1 LSB. With rounding re-entries
    and truncating states, ROOM held a 1080 Hz cycle at −86 dB for good: a
    normalised allpass truncates only its scaled state, and 0.6 of 1 LSB
    rounds back up through the loop. A truncating re-entry kills it too,
    but its error is a square wave on a held tone's sign, 1.4 dB on the
    floor; settled states sit 1 dB under truncated ones.
  - **No `t_min` floors ROOM's DECAY.** The stage allpasses take DIFFUSE's
    g held to the stage gain at the longest one's length, so their own ring
    never outlasts the RT60 (it binds below about 0.33 s). Without it,
    DIFFUSE 1 at DECAY 0 rang 0.37 s for 0.2; QUAD's rule would floor DECAY
    at 0.47 s, a third of the knob.
  - **ROOM's RT60 and slap are judged from 100 ms after the send.** Before
    then its longest fixed tap (84 ms) replays the send, and the diffuser
    rings out as an echo train at its own lengths: they are not the tail.
  - **ROOM's SMEAR bound is held in the engine:** each smeared read stays in
    [M − 2D, M], 2D ≤ 1.45 % of its own loop's trip at every SIZE, which is
    the spec's 25 cents. Zero crossings can't judge two coupled loops 100
    and 95.4 ms long: a still ROOM reads ±45 cents pass to pass. Loop B is
    held to its own trip's cap, up to 5 % under A's, which the panel shows.
    A held tone's tail keeps its spectral peak within 25 cents of SMEAR 0's
    (2.8 cents at SMEAR 1, RATE 1).
  - **The switch test starts ROOM's tone burst 70 blocks early,** past the
    same 84 ms tap, whose end makes ROOM's own tail 3× rougher.
  - **WET is 10.8**, by the held-input loudness rule: ROOM's 0.55 s holds
    less than QUAD's 1.9 s.
- **HALL, as built (Task 10, 2026-10-05).** The following departures from
  the spec are accepted.
  - **The LOOP engine's stage allpasses store f32**, in ROOM and HALL,
    beside the diffuser, not R2's normalised i16 states. Those held HALL at
    SIZE 0, DECAY 1 in a limit cycle 45 dB down for good: their rounding
    error, ×4 at g 0.75, outran the loop's 8 % loss a round trip.
    Truncating them killed it but about halved the RT60 at low levels. f32
    is R3's answer, costs fewer instructions, and fits: HALL 19,774 slots,
    ROOM 13,540. ROOM's sound moved by it (its floor 1.4 dB down); ROOM's
    "states settle" above is moot, its re-entries still settle.
  - **The dampers do not round.** They are state beside the arena; rounding
    them put HALL's floor at its +4 dB bound, and a rounding one-pole can
    hold ±1 LSB of DC under a coefficient of ½ (DAMP's 1.5 kHz is 0.32).
  - **HALL's level plan allows 20 % THD** on a 0 dBFS tone held on a loop
    mode at 10 s and 120 ms (G 12.5): R1's clip runs far past its knee
    (16.4 %). Every other level-plan step holds as specified.
  - **HALL's settling is judged by the 1 kHz ± 25 Hz centroid**, in 1.2 s
    windows at least 20 dB over the LSB, within 0.5 Hz of the input: the
    tail's modes 1.67 Hz apart trade the spectral peak as they decay, so a
    peak reads up to 5 Hz off while nothing has shifted.
  - **HALL's SMEAR bound is held in the engine**, as ROOM's, with a check
    that the tail's peak hops no more than 25 cents beyond SMEAR 0's.
  - **HALL's switch test starts its burst 600 blocks early:** its default
    tail is 2 kHz-rough until the dampers darken it, which hides the
    gain-jump control.
  - **The level plan's stored regions follow the rotating head**, and each
    noise floor is read from a fresh engine; Task 9's region readings were
    of fixed slots, not of the lines.
  - **Exact silence is judged by 2 × RT60 + 1 s at HALL's 10 s:** the
    burst stands 81 dB over the LSB and the plate measures 11.3 s.
  - **WET is 9.7**, by the held-input loudness rule (+6.6 dB over QUAD's).
  - **The host flushes subnormals** on its audio thread (MXCSR FTZ and
    DAZ), as the chip's FPSCR FZ does, so f32 state reaches exact 0 on both.
- **Interactions.**
  - A TYPE switch drops about 5.3 ms of send and dips the return for about
    45 ms; sweeps list this as by design.
  - A SIZE move colours the tail with a 30 ms comb per crossfade, as QUAD's
    does.
  - #142's quiet-send tail loss applies to every TYPE alike.
- **Order and acceptance.** Step 0 comes first, and tempo sync waits for
  ORBIT on `main`. ADR 0085 is accepted once every A/B pair passes the
  spec's criterion: each time metric within ±30 % of the named Midiverb
  plugin's, or an owner-signed ear exception. In addition, every TYPE's
  ΔCOST must fit the headroom, the stress run and the pitch tests must
  pass, and UAT must pass. The measured CPU and flash are then recorded
  here.
- **What it rules out**, unless a later ADR adds them:
  - FREEZE;
  - glides and pitch dives in the reverb;
  - saving per-TYPE knobs to the card;
  - external modulation of the reverb (#345).

  Per-TYPE knob memory exists in RAM only.

## Sources
- `docs/superpowers/specs/2026-10-04-reverb-types-design.md`.
- private issue #343: the owner's rulings of 2026-10-03, and the naming ruling.
- private issue #345: mixer-level modulation.
- The #343 research spike, kept outside the repository; its prose and measurements are summarised in the spec.
- github.com/thement/midiverb_emulator (GPL-3.0), read as documentation only.
- M. R. Schroeder, "Natural sounding artificial reverberation", JAES, 1962.
- W. G. Gardner, "A realtime multichannel room simulator", 1992.
- J. Dattorro, "Effect Design, Part 1: Reverberator and Other Filters", JAES, 1997.
- ADRs 0009, 0010, 0011, 0013, 0020, 0028, 0031, 0037, 0040, 0045, 0053, 0055, 0061 and 0067; ORBIT's 0071, 0072, 0080, 0081, 0082, 0083 and 0086 (orbit branch).
- `chimera-core/src/dsp/ring.rs`, `reverb.rs`, `fx_bus.rs`, `hw.rs`; `chimera-core/tests/cost_test.rs`; `chimera-stm32/src/bench.rs`.
