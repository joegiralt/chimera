# 0095. Match every reverb TYPE to its MidiVerb II presets; rebuild ROOM and BLOOM on the programs' structure

- **Status:** Proposed; supersedes in part [0085](0085-the-master-reverb-picks-a-type.md) (ROOM's and BLOOM's designs, its provenance rule for structure numbers, and its ±30 % A/B pass for every TYPE with a MidiVerb counterpart)
- **Deciders:** project owner, 2026-10-05 ("yes to all three": structure numbers, ROOM's cost, ROOM's two new knobs). Firmware drafted it from the reference harness's findings.

## Context
ADR 0085 judged each TYPE against a named Midiverb program within ±30 % on
time metrics. That was a sanity window, not a match: the owner wants a named
setting to *be* the preset.

A reference harness now renders the MidiVerb II's 49 reverb programs through
thement/midiverb_emulator (GPL-3.0) at the unit's native 23.4 kHz, and renders
Chimera's real `FxBus` from the same stimuli. It measures both sides the same
way. The harness lives outside the repository (the session scratchpad's
`midiverb-ref/`: `README.md`, `run.sh`, `model.py`, `room_v2.py`,
`bloom_model.py`, `fits/*.json`).

What it found:
- **ROOM as built (Task 9) fails every ROOM preset.** Four causes:
  - its first arrival can't go below 2.9 ms;
  - 3 taps a side leave C50 0.3–3.2 dB short;
  - its echo density sags 7–32 % by 100 ms;
  - its damper is fixed at 8 kHz.
- **The unit's ROOMs are 4 loops × 2 stages, not 2 × 2.** A float model of
  that structure (ROOM v2) passes 6 of 9 ROOM presets.
- **The unit's Blooms are an allpass cascade with no plain delay**, not the
  spec's ten allpasses plus a SWELL delay. A float model of the cascade
  passes Bloom 1 and Bloom 2.
- **GATE and REVERSE miss as well.** GATE is near Fast Gate 150 on cut and
  slope, but it has no pre-delay (the presets use 10–71 ms), is twice as
  dense early, and is 25 % brighter. REVERSE is far off. The unit is silent
  for about 60 ms, rises 45 dB over 400 ms, fires a loud delayed copy at
  483 ms, then cuts hard.

Matching took the unit's own delay lengths, gains and tap offsets, read from
the emulator's decompiled programs. ADR 0085 forbade that. The owner lifted
the ban on 2026-10-05.

## Decision
- **The MidiVerb II is the reference for every TYPE.** A named knob setting
  must be indistinguishable from the same-named preset. It passes only if,
  measured by the harness on the Rust render:
  - band RT60 is within ±5 % in every octave from 250 Hz to 4 kHz
    (125 Hz and 8 kHz are reported, not judged);
  - pre-delay is within ±1 ms;
  - C50 is within ±1 dB;
  - NED (Abel–Huang echo density) is within ±10 % at 20, 50 and 100 ms;
  - the tail's spectral centroid is within ±5 %.

  The metrics are those of the harness's `mvlib.verdict`: interrupted noise
  for RT60 (ISO 3382-2, 16 seeds), and the click response for the rest.
  The owner's UAT by ear still comes after the numbers pass.
- **Which presets belong to which TYPE:**

  | TYPE | MidiVerb II programs |
  |---|---|
  | ROOM | 1, 2, 3, 5, 8, 9, 10, 11, 14 |
  | HALL | 4, 6, 7, 12, 13, 17–20, 23–26, 28, 29; and 15, 16, 21, 22, 27 |
  | GATE | 30–39 |
  | REVERSE | 40–44, 46, 47, 48 |
  | BLOOM | 45, 49 |
  | QUAD | none; it stays Chimera's own |

  Programs 15, 16, 21, 22 and 27 would be ROOM's if its DECAY reached 3.5 s.
  At ROOM's 2.0 s they are HALL's.
- **Provenance.**
  - **The source.** ROOM's and BLOOM's structure numbers (delay lengths,
    allpass lengths, gains and tap offsets) come from the MidiVerb II
    programs as decompiled by github.com/thement/midiverb_emulator
    (GPL-3.0). The owner approved their use in the firmware on 2026-10-05.
  - **What enters the repository.** The numbers, in ms, as constants in our
    own Rust, each table with a comment naming this ADR. Nothing else:
    - no emulator code, no instruction listing, no ROM image;
    - no harness code and no renders or other outputs.

    The emulator stays an external tool.
  - **The licensing question, named plainly.** These numbers are read out of
    Alesis's ROM through a GPL-3.0 decompilation, and the workspace was then MIT (GPL-3.0-or-later since ADR 0330).
    Whether delay lengths and gains are protectable expression or bare facts
    is not settled here. The owner has decided to use them and to resolve the
    question before V1, with the other licensing matters held until then.
    Until it is resolved, every table that carries these numbers is named in
    one place (this ADR's tables), so it can be replaced as a unit.
  - **Knob values are fitted, never copied.** The float model rings 11–30 %
    longer than the unit's 16-bit tank. So each preset's knob setting comes
    from fitting the Rust render to the reference, never from the unit's
    gains.
  - This supersedes ADR 0085's "no ROM coefficient, offset or tap table"
    and "lengths are our own primes" for ROOM and BLOOM. It also covers HALL,
    GATE and REVERSE if their fits need the same.
- **ROOM v2.** At 24 kHz, ms converted to the nearest sample:
  - **Input:** TONE's one-pole, then PRE-DLY.
  - **Diffuser:** 5 series allpasses, g 0.75, fixed at 0.81, 1.92, 3.59,
    4.91 and 6.24 ms.
  - **Tank:** 4 loops × 2 stages. Each stage is a delay then an allpass, and
    each loop ends in one one-pole damper.

    | Loop | Stage delays at SIZE 1 (ms) | Stage allpasses, fixed (ms) |
    |---|---|---|
    | A | 37.1 / 57.5 | 29.0 / 13.3 |
    | B | 59.4 / 42.3 | 28.9 / 18.4 |
    | C | 52.7 / 41.9 | 30.4 / 16.3 |
    | D | 41.5 / 43.6 | 27.1 / 14.2 |

    SIZE scales the stage delays by k, from 0.4 to 1.0. The allpasses and
    the diffuser don't move.
  - **Coupling:** the loops re-enter in pairs, (A, B) and (C, D), through a
    Householder reflection [[cos θ, sin θ], [sin θ, −cos θ]]. θ 0° is four
    2-stage loops, and θ 90° is two 4-stage loops. This replaces ADR 0085's
    fixed 0.25 rad rotation.
  - **Send:** the diffused send enters every loop at +1.
  - **Taps:** 8 a side, one per stage line, each of weight ¾, offsets scaled
    by k with their lines:
    - R: 0.51 (fixed), 31.4, 10.8, 20.4, 15.8, 21.3, 30.9 and 9.1 ms;
    - L: 5.5, 5.6, 36.7, 11.8, 24.1, 8.9, 18.0 and 38.1 ms.

    Each tap stays inside its line at every SIZE. The scaled taps move by
    crossfade, in the same crossfade as their lines. This replaces ADR
    0085's fixed taps for ROOM.
- **ROOM v2's knobs:**

  | Knob | Slot | Range |
  |---|---|---|
  | SIZE | 3 | k 0.4 → 1.0 |
  | DECAY | 1 | stage gain from RT60 0.2–**2.0 s** (was 1.5) |
  | TONE | 7 | input fc 10 → 2.5 kHz |
  | DIFFUSE | 8 | stage allpass g 0.45–0.70 (the diffuser stays at 0.75) |
  | DAMP (new) | 2 | loop one-pole, 1.5–10 kHz |
  | LOOPS (new) | 10 | θ 0–90° |
  | PRE-DLY | 9 | 0–50 ms |
  | SMEAR | 11 | as ADR 0085; 0 matches the unit |
  | RATE | 12 | as ADR 0085 |

  - DAMP takes the MORE page's second cell, as HALL's does. LOOPS takes the
    MORE page's sixth cell, on slot 10, which ROOM never used. So the block
    keeps its 12 live ids, `KnobMemory` keeps its 108 B, and no disk code
    changes.
  - SYNC stays OFF and TIME only.
- **ROOM v2's near misses.** In the float model, #9 misses C50 by 0.04 dB.
  #10 and #14 miss 4 kHz RT by about 10 %, and #10 also misses NED at 20 ms.
  Two things to try, in order:
  1. let DAMP's top end open the damper fully;
  2. tie the tap weight to θ: some Mediums use ×9/8.

  A preset that still fails after both goes to the owner, with its residuals.
- **BLOOM.** This replaces ADR 0085's ten allpasses plus SWELL delay:
  - an input one-pole, then PRE-DLY;
  - a loop of 15 series unity allpasses, g ½, from 0.56 to 89 ms and
    580 ms in all, with no plain delay;
  - feedback from the cascade's end through a one-pole and a gain (the
    unit's ¾);
  - outputs from the cascade's end, through 3-allpass decorrelators: L 10.6,
    2.1 and 0.9 ms; R 6.3, 1.8 and 2.4 ms.

  Bloom 2 is Bloom 1 with 85.5 ms more pre-delay, and both have an 11.1 s RT.
  - **The float model passes both** with the cascade × 1.031, feedback
    0.662, both one-poles at about 3.1 kHz, and PRE-DLY 2.3 ms (#45) and
    87.8 ms (#49).
  - **The knobs:**
    - TONE drives both one-poles together;
    - SWELL scales the whole cascade, × 0.5–1.2, by crossfade;
    - DECAY sets the feedback gain from RT60, 2–12 s;
    - DIFFUSE sets the cascade's g;
    - PRE-DLY, SMEAR and RATE are as before.
- **CPU and memory.**
  - **ROOM about doubles its memory accesses** (owner, 2026-10-05). The
    accesses go from 4 stage delays, 4 stage allpasses, 4 diffuser
    allpasses and 6 fixed taps to 8, 8, 5 and 16, and the taps now
    crossfade with SIZE. Doubling Task 9's REV ROOM (17 cycles default, 41 tape) gives
    about 34 and 82. Against REV QUAD's 55 and 71, that is ΔCOST about −21
    and +11, inside the 76 and 23 headrooms. The tape build's margin is
    thin, and the bench decides. ADR 0085's ΔCOST ≤ headroom gate still
    holds.
  - **ROOM's arena:** about 9.0 k i16 slots of stage delays, 4.3 k f32 stage
    allpass states (8.5 k slots, f32 as Task 10 ruled), 0.4 k f32 diffuser
    (0.8 k slots), and 1.2 k of PRE-DLY. That is about 19.6 k of the
    23,220-slot arena, which does not grow.
  - **BLOOM's arena:** about 14.4 k i16 slots of cascade at the presets'
    × 1.031, and 16.7 k at SWELL's × 1.2. Then 3 k of PRE-DLY and about 0.5 k
    f32 (1 k slots). That is about 20.7 k at most. The cascade can't afford
    f32 states (2 slots each), so its allpasses store normalised i16 (ADR
    0085's R2). That brings back Task 10's limit-cycle risk, which BLOOM's
    tests must catch.
  - **BLOOM's CPU:** about 5 more allpass updates a sample than ADR 0085's
    estimate (15 in the loop and 6 in the decorrelators, against 10 and 4).

- **ROOM v2, as built (Task 9b, 2026-10-05).** The following departures
  are measured, and need the owner's ruling.
  - **Its own engine, `rev/room.rs`,** not `LoopEngine<4, 2>`: ROOM v2 runs
    59 i16 and 26 f32 accesses against HALL's 21 and 16, and sharing the
    access tables would size HALL's to ROOM's. HALL is untouched
    (`rev_hall` bit-identical).
  - **It runs stage by stage over each segment,** not sample by sample:
    the sample loop spilled its 85 bases and read 705 instructions a ring
    sample. Every feedback read sits 319 samples back or more, so the
    order is the sample loop's arithmetic. Each line keeps one block of
    guard slots (`GUARD`, +1,120 slots), since a line's write lands on the
    slot the line above reads 1–2 samples earlier.
  - **The taps sum exactly,** as integers, and a crossfade adds w ×
    (to − from) for the moving lines' taps only.
  - **DIFFUSE's g, the dampers and the reflection step once a block** on
    their 20 ms ease, as BURST's weights do. SMEAR still ramps per sample.
  - **The stage gains step once a block too** (fix round 1), on their
    50 ms ease. At still knobs that is bit-identical; under SIZE toggling
    every block (the bench's worst) the return differs from the per-sample
    ramp by 62 dB SNR.
  - **The stage gains count the stage allpasses' slow modes:** each stage
    is (D + M·Φ) long for the RT60, with Φ = (1 + 4.8 g⁴)(1.37 − 0.37 k).
    Without it the 1 kHz RT60 ran 6–90 % long, more with DIFFUSE and at
    small SIZE. With it, DECAY tracks within ±10 % over DECAY ¼–1, SIZE
    0–1 and DIFFUSE 0–1; DECAY 0 reads +4 % at SIZE ½ and +15 % at SIZE 0
    and 1, where the diffuser's own ring adds.
  - **Φ's SIZE slope is fit evidence, not pinned by a test** (fix round 2).
    It was fit at that grid's knobs (LOOPS 18°, DAMP 10 kHz). At the
    starting knobs as fitted (LOOPS 65°, dampers open) the measured 1 kHz
    RT60 at DIFFUSE 0, DECAY ½ and 1, reads −9 and −7 % at SIZE 0 and −1
    and +2 % at SIZE 1; with k held at 0.7 in Φ it reads −4 and −1 %, and
    −5 and −2 %. Both sit inside the test's ±10 %, so no tolerance the data
    allows kills the flat mutant. At DIFFUSE 1, DECAY 1, both read short:
    the fitted Φ −16, −10 and −9 % at SIZE 0, ½ and 1. DECAY's honesty at
    high LOOPS and DIFFUSE wants a refit of Φ with θ in it.
  - **DAMP's top opens the dampers** (the first near-miss step): DAMP 0 is
    a bypass, and its cell reads OFF. Its first tick is a cliff, from no
    damper to a 9.9 kHz one-pole (about −0.3 dB a trip at 4 kHz), eased
    over 20 ms: for the owner to judge at UAT.
  - **The tap weight stays ¾ at every θ.** The second near-miss step
    scales every tap alike, so it moves only the level, and every judged
    metric is level-free: it can't change a verdict.
  - **The first arrival is 0.52 ms past the bus's half-bands** (34
    samples at 48 kHz, ahead of every TYPE): 1.23 ms on the bus.
  - **ΔCOST is a range until the unit reads it** (Task 13). On the
    emulator ROOM v2 runs +91.7 instructions an output sample over Task
    9's ROOM. The two calibration anchors, 0.47 cycles an instruction
    (Task 9's REV ROOM) and 1.06 (REV QUAD), put the default build at about
    +5 to +60 against its 76, and the tape build at +13 to +67 against its
    23. If the tape build reads over, the fallback order is:
    1. revert the `moving` unswitch: about −6.8 KB of flash for +20
       instructions, and it frees I-cache (`Room::run`'s text is 21.5 KB
       against the 16 KB I-cache);
    2. the parked truncation of the stage allpass states.
  - **`EngineState` grows to 244 B** (248 B with the tests' probes),
    against ADR 0085's 160 B: four loops' gains, targets, dampers and
    smear offsets.
  - **LOOPS's cos and sin come from an f32 series,** normalised so c² + s²
    is 1 to the rounding: libm's ran in double precision.
  - **ΔCOST with GRAIN** (Task 9c): GRAIN adds 15.5 instructions an
    output sample, counted in the release firmware (below), 7.3 to 16.4
    cycles at 0.47 and 1.06 cycles an instruction. That puts the default
    build at about +12 to +76 against its 76, and the tape build at +20 to
    +83 against its 23. The unit decides at Task 13; the fallback order above stands, and
    GRAIN is cuttable on its own (it never feeds back).

- **ROOM's GRAIN, as built (Task 9c, 2026-10-06).** The MidiVerb II's
  sizzle is its arithmetic grain: about 40 truncations a sample in 16-bit
  words carrying a 13-bit signal leave a noise floor at a fixed level under
  every tail (the sizzle report, owner-approved 2026-10-06). ROOM adds it on
  its return, at the ring's 24 kHz, in `room.rs`'s `grain`:
  - **Noise:** one LCG (Numerical Recipes'), two draws a sample, L then R;
    each side's draw plus 0.3 × its last (`Tilt`), −3.6 dB at 9 kHz
    against 3 kHz, as the unit's grain sits under its tails. The seed is
    fixed (`GRAIN.seed()`): every fresh ROOM draws the same grain, and the
    goldens reproduce.
  - **Level:** 2.6 LSB of 13 bits a side at the wet return's full scale
    (MIX 1), fitted by the GRAIN metric. The unit's own is 3.6–4.3 LSB13 at
    its input's scale; the harness drives the unit at the A/B's −1 dBFS
    peak (the notched burst's peak is about −5 dBFS) and Chimera as is, so
    its grain reads about 4 dB lower at Chimera's scale.
  - **DECAY law:** the level is full from an RT60 of 0.8 s up, and × (RT60
    / 0.8 s)^½ under it. Truncation fed round fewer, shorter trips leaves
    less grain, and the short programs (#1–#3) read it 2–5 dB under the
    medium ones. A SIZE law alone could not do it: #1 and #9 sit at SIZE's
    k 0.65 and 0.66 and want opposite gates.
  - **Gate:** a 20 ms one-pole on |l| + |r| before the grain; the grain is
    full while that envelope stands a knee × its own level over a 1-LSB
    floor, fades linearly under it, and is exactly nothing under the floor.
    The knee is tied to SIZE (the report's ask for #1): geometric in k, 0.8
    at k 0.4 to 135 at k 1. A small room keeps its grain on as its tail
    sinks into the unit's dead band; a large one fades it sooner. The gate
    reads the return before the grain, so it never opens on the grain, and
    a dead tail's envelope falls under the floor in finite time: the return
    is exact zeros at idle and after the tail, as before.
  - **Branch-free:** the gate is a clamp, the noise integer arithmetic; the
    same work at every level. The scale and the knee are cached with the
    other targets (DECAY, and the lengths crossfading to) and eased on the
    20 ms ease once a block (fix round 1): a DECAY or SIZE move never snaps
    the grain's level. The floor sits under the knee at DECAY's shortest
    RT60 too, checked at compile time.
  - **Cost, counted statically** in the release firmware's disassembly
    (the review's count; GRAIN is inlined in `Room::run`): a 120-instruction
    body (99 VFP) four samples at a time, 8 times a 32-sample ring block,
    plus about 30 of entry and exit: about 990 a block, **15.5 an output
    sample** (a mirror kernel read 15.2). No data-dependent branch, so the
    dynamic count is the same.
  - **`EngineState` grows to 268 B** (272 with the tests' probes): the
    envelope, the seed, and the scale and knee cached and eased.
  - **GRAIN and HF share join the harness's bar** (`chimera-utils`'s
    `grain.rs`), judged for ROOM, reported for the others. GRAIN is a
    noise-power ratio: four notched noise bursts, the notch cores' power
    density over their flanks' in the burst's last 500 ms and 50–200 and
    200–400 ms after it; rms Δ ≤ 3.5 dB, none past 6.5 dB. HF share is
    P(4–11.7 kHz) / P(0.5–2 kHz) at 0–50, 50–200 and 200–600 ms after the
    burst, each ±1 dB: it catches the 2–3 dB dark top end the centroid's
    ±5 % let through on #5 and #11.
  - **TONE's top opens the input filter,** as DAMP's top opens the dampers
    (the first near-miss step): the unit has no input filter, and the
    10 kHz one-pole cost #5 and #10 1–1.5 dB of HF share no knob could win
    back. It opens over the knob's last 5 % (9.33 → 10 kHz), so the fit and
    a hand find every brightness between, with no cliff; TONE 0 reads OFF,
    and inside the ramp the cell reads the cutoff the opening filter has
    (fix round 1), not the knob's.
  - **START is program 5 exactly** (fix round 1): its fitted knobs and
    SMEAR 0. With SMEAR 0.3 it failed #5 (GRAIN 4.7/9.5 dB, HF 3.8 dB, C50
    +1.5 dB): SMEAR's motion fills the notches on top of the grain.
    `chimera-utils mv-start` judges each TYPE's START against its source
    program, as `mv-verify` judges a fit: ROOM's against #5 PASSES. BLOOM's
    keeps SMEAR 0.3 for the owner's UAT and FAILS #45 (centroid −43 %,
    4 kHz RT −29 %, HF 5.5 dB).
  - **The fit, every TYPE's** (fix round 1): its distance adds 4 for each
    judged metric past 0.8 of its bar, so the search keeps a margin rather
    than parking on the bars. Scoring a setting by the worst of it and
    SIZE ±0.001 (SIZE's lengths are whole samples) is opt-in,
    `--size-worst`, off by default: it tripled a fit's time and left ROOM's
    refits worse (below). `mv-verify --robust` nudges every fitted knob
    ±0.001 and names each failing nudge: a report, not a gate.
  - **Acceptance is 9/9 at the committed knobs** (ruling, 2026-10-06). A
    one-sample length change reshuffling the echoes is below hearing, and
    the owner judged ROOM indistinguishable blind; the robust counts below
    are recorded, not required.
  - **The refit:** all nine ROOM presets pass with GRAIN and HF share in.
    Held to ±0.001 on every knob, #3 and #5 pass every nudge; 120 of the
    135 nudges pass. The rest fail on the metrics' own grain, not on a
    trend:
    - SIZE ±0.001 moves whole-sample lengths and reshuffles the echo fine
      structure: NED at 100 ms moves ±5–9 %, and on the short programs
      (#1, #2) HF share's 200–600 ms window moves 1.3–1.5 dB. #1, #2, #9,
      #14 fail at SIZE ±0.001, #8 and #10 at one side;
    - #9, #10 and #14 fit DAMP 0, the dampers' bypass: DAMP +0.001 is the
      step to a 9.9 kHz one-pole (DAMP's top, above), and fails. **For
      UAT:** DAMP 0's first step is a structural cliff, the bypass to a
      damper engaging at 9.9 kHz, on every preset that sits at DAMP 0;
    - #10 at TONE −0.001, #11 at PRE-DLY +0.001 (NED −10 %, its bar).
    The robust search itself (from the Task 9c fits, at 0.03/0.012 and at
    0.012/0.004/0.002) found no setting passing all three SIZE settings
    for the others, and left nominal passes at 3 and 4 of 9; the committed
    fits keep each preset's best: the Task 9c fit, or a robust or
    fewer-failing neighbour (#3, #5, #8). At the committed knobs, ±0.001
    on every knob:

    | # | at its knobs | ±0.001 nudges that fail |
    |---|---|---|
    | 1 | PASS | SIZE ± |
    | 2 | PASS | SIZE ± |
    | 3 | PASS | none |
    | 5 | PASS | none |
    | 8 | PASS | SIZE − |
    | 9 | PASS | SIZE ±, DAMP + |
    | 10 | PASS | SIZE +, TONE −, DAMP + |
    | 11 | PASS | PRE-DLY + (NED −10 %) |
    | 14 | PASS | SIZE ±, DAMP + |

- **BLOOM, as built (Task 11, 2026-10-05).** The following departures
  are measured, and need the owner's ruling.
  - **Its own module, `rev/bloom.rs`, sample by sample.** The cascade
    comes back one sample after its end, as the unit's does, so no stage
    can run a segment ahead; its i16 lines need no guard. The
    decorrelators, outside the loop, run stage by stage, with a block's
    guard each.
  - **The cascade's lengths are the ADR's ms to two places** (0.56, 3.55,
    4.96, 10.21, 14.49, 20.56, 23.72, 35.17, 42.69, 47.01, 57.56, 66.79,
    78.68, 85.51, 89.02: 580.48 ms), and the decorrelators' to one, as
    above. The click metrics are chaotic in the lengths: one SWELL step
    (0.001, about 2 samples on the long allpasses) moves Bloom's pre-delay
    by up to 30 ms and C50 by 2 dB, through which sample of the bloom
    peaks. So the fit takes a point where they hold still: SWELL 0.705,
    where 0.700–0.706 read within 1 ms of each other. There both presets
    pass at one knob set, only PRE-DLY differing as on the unit (0.132 and
    0.815), and each still passes a step off on every knob (fix round 1).
    Any change that moves a length (the SWELL law, `exp2`, the rounding)
    must refit.
  - **DECAY is the 1 kHz RT60** over the loop's slow length, (Σ lengths +
    1) × (1.07 + 1.9 g⁴), fitted over DIFFUSE, SWELL and DECAY ½–1, and
    through the loop one-pole's 1 kHz gain, so TONE leaves it. That lift
    reaches DC too (the one-pole passes it whole), so the gain is also
    held to √ of the open loop's: DC rings at most twice DECAY (fix round
    1; uncapped, SWELL 0 at TONE 1 rang 60 Hz for 139 s at DECAY 1). The
    cap binds only at dark TONE with long DECAY: there 1 kHz falls short
    (10.2–10.8 s measured against 12 at SWELL 0, TONE 1); never at the
    presets. At SWELL 0 DECAY 0, ½ and 1 read 2.04, 4.82 and 11.69 s. At the starting SWELL
    DECAY 0 is three trips of a 0.7 s loop, a staircase, whose T20 read
    12 % short.
  - **The cascade's states settle** (`store_settle`), as ROOM's
    re-entries: the return reaches exact zero from DECAY 1.
  - **WET_BLOOM 5.72, by the held-input loudness rule** every other TYPE's
    return gain follows (−6.70 dB against QUAD's −6.73; peak +5.4 dBFS
    against +5.95), not the burst rule the plan named.
  - **DECAY in beats marks a clamped count** ▴ or ▾, as a folded note
    is: at 120 BPM 1–3 beats play BLOOM's 2 s floor, 32 its 12 s top.
  - **The noise floor is inside the plan's bound:** 15.6 dB against
    QUAD's 12.0 at DECAY 0 (+3.6; bound 4), 17.9 at DIFFUSE 1 (+5.9; bound
    6): fifteen normalised states, each error read × 1/(1 − g). The tests
    hold +4 and +6.5.
  - **SMEAR's window mean drifts 1.25 cents** at the cap (bound 1): the
    three smeared lines share one loop, so a 4/RATE window's eight targets
    leave its mean about 1.4 cents (1σ) off. Per pass it holds 25 cents,
    and no vibrato.
  - **SWELL is `exp`, as the spec's table,** over 290–697 ms; one
    allpass moves a crossfade, 89 crossfades from SWELL 0 to 1.

- **HALL, as built (Task 10b and its fix round 1, 2026-10-06).** The
  owner approved the redesign below and #19 as a signed miss. The near
  misses get signed off after listening.
  - **Why it changed.** HALL as it stood passed no HALL preset: C50 2–12
    dB high and NED at 100 ms 16–70 % low on most presets, and #29 rang
    past DECAY's 10 s top. The unit's halls are four structures:
    - 1 loop × 3 stages, 3 taps a side: #4, 17, 18, 23–26, 28, 29;
    - 2 × 2 with 4 taps: #6, 7, 19, 20;
    - 1 × 8 with 8 taps: #12, 13;
    - 2 × 4 with 7 taps: #15, 16, 21, 22, 27.

    Each one's C50 and NED follow its diffuser and taps, not its tank.
  - **The tank stays** `LoopEngine<1, 4>`: its lengths, SMEAR, and the
    crossfades. SIZE is now `lin`, 120 → 600 ms.
  - **The diffuser:** six allpasses at program 17's 13.718, 18.419,
    28.547, 34.530, 8.718 and 6.154 ms (`DIFFUSER_MS`), with g from
    DIFFUSE.
    - **SPREAD** scales them whole, × 0.05–1.10. It is the `Spread`
      newtype, on SHAPE's slot (10), in MORE's sixth cell. The cell reads
      the diffuser's length in ms.
    - SPREAD moves them by crossfade: one allpass and at most 16 samples
      a crossfade (`DIF_STEP`), in the same crossfades as SIZE and PRE-DLY.
    - The plate zone pulls SPREAD to 0 with TONE and PRE-DLY.
  - **The four shortest diffuser allpasses store f32** (13.7, 18.4, 8.7
    and 6.2 ms, `DIF_F32`); the two longest store normalised i16 (R2), by
    `store_settle_ap`: rounded, but truncated under 3 LSB.
    - The arena holds no more f32: 23,020 slots of 23,220 as built (17,628
      i16 and 2,696 f32), and
      about 24.7 k with all six f32.
    - A rounding allpass at g 0.8 holds ±2 LSB for good, and the loop rang
      on it. Under 3 LSB it truncates, which breaks that hold.
  - **Taps: 8 a side**, one per stage line in turn, at our own fractions
    of each line, scaled with SIZE and read from the lines' crossfades.
    - Each tap is signed by its line's stage-allpass parity.
    - Each reads `LATENCY` (17 ring samples) nearer its write: the bus's
      half-bands delay the return 0.71 ms, which the float model did not
      have, and every fit read +0.7 ms of pre-delay PRE-DLY could not
      take back.
    - **DENSITY** fades taps 4–8 in, a fifth of the knob each (the
      `TapCount` newtype, 3–8 a side), the weights × √(3/Σw²) so the
      level holds (within 1 dB from 3 to 8 taps; the peak at MIX 1 stays
      within 1 dB of QUAD's).
    - DENSITY has its own id, ParamId 14, disk ident `DENSITY`.
      `KnobMemory` grows to 120 B and AXI by 16 B.
    - Its cell is on a new page, REV › MORE 2. EDIT skips that page under
      every TYPE with no cell there.
    - A Sound saved before DENSITY loads at `DENSITY_DEFAULT`, HALL's
      starting 0.207298 (4.04 taps).
    - Tying DENSITY to DIFFUSE cost 2 presets in the model, to TONE 4.
  - **One damper a loop**, after stage 4. DAMP is `exp` 14 → 1.3 kHz,
    HALL's own law, no longer QUAD's.
  - **TONE 0 is OFF**, an open one-pole (`Ranges::tone_off`,
    `ToneOff::AtZero`); above it, 12 → 2 kHz.
  - **DIFFUSE is g 0.3–0.8.**
  - **DECAY's top is 16 s.** A round trip's gain is also held to 0.92 (G
    12.5, the level plan's), which binds only at small SIZE.
  - **The plate zone is SIZE < 0.05** (was 0.15): at 0.15 the pull played
    #16's and #21's fitted knobs as plate.
  - **The stage delays write by `store_settle_ap`**, as the diffuser does.
    Rounding writes at DAMP's 14 kHz held 3 LSB in the plate corner for
    good.
  - **Five read paths, by what moves this block.** A still block (most)
    reads each line once. A SIZE or PRE-DLY crossfade blends the stage
    delays and PRE-DLY, reads every tap at its new length and adds (1 − w)
    of the way back for the ≤ 4 taps on the moving line. A SPREAD
    crossfade blends the diffuser. A DENSITY move ramps the tap weights
    each sample. Two at once run all three. A still read's old and new
    slots are one, so a blend at w 1 is exact.
  - **The Rust render, `mv-verify HALL` (16 seeds):** 8 of 20 pass at the
    fitted knobs: #7, 12, 13, 15, 16, 22, 23 and 27. **7 of 20 hold a
    step off,** passing with every fitted knob ±0.001 (`mv-verify
    --robust`'s nudges): #12, 13, 15, 16, 22, 23 and 27; #7 fails at SIZE
    ±0.001 and SPREAD +0.001. The fits scored the worst of each point and
    SIZE and SPREAD ±0.001 at 0.8× the bar.
    - SIZE and SPREAD move lengths a sample at a step, and the click
      metrics are chaotic in the lengths (as BLOOM's SWELL is): before
      this round, 9 passed at the fitted knobs and 3 a step off.
    - **The gap to the float model's 14:** the model had no bus latency
      (fixed, `LATENCY`), the plate pull caught 2 fits (narrowed), and the
      rest is the firmware's storage and the robustness the fits now ask:
      the float model's points were not checked a step off.
    - Near misses, for the owner's ear: #4 (NED +11 % at 50 ms), #6 (1
      kHz −8 %, 4 kHz +9 %, C50 −1.0 dB), #17 (NED −18 % at 20 ms), #18
      (NED +14 % at 100 ms), #20 (pre-delay +2.0 ms, NED +33 % at 20 ms),
      #21 (NED +10 % at 100 ms), #24 (NED +19 % at 20 ms), #25 (NED +11
      and +13 % at 20 and 50 ms), #26 (NED −14 % at 50 ms), #28 (pre-delay
      +1.5 ms), #29 (pre-delay +1.9 ms, NED +21 % at 100 ms).

    #19 is signed off as a miss.
  - **START is program 23 exactly** (Large Bright 1.45, a fit that holds a
    step off), with SMEAR 0: `rev_hall` and `rev_hall_mv23` are one
    render. SMEAR's interpolated reads low-pass the loop each trip, so
    START at SMEAR 0.3 was not the preset.
  - **WET_HALL is 14.0,** by the loudness rule: −7.9 dB against QUAD's
    −6.7, its peak 0.3 dB under QUAD's, so the brightest preset goldens
    stay under full scale.
  - **Departures for the owner:**
    - **The noise floor** is 3.9 dB over QUAD's at START and 5.5 dB at
      DIFFUSE 1, inside ADR 0085's 4 and 6 dB again with the f32
      allpasses (5.3 and 7.6 dB under the all-i16 diffuser's at that WET).
    - **A 0 dBFS 5 kHz tone held at DECAY's top in the plate corner
      aliases −30 dB through R1's clip** (−21 before this round). The test
      holds −20 for the owner's sign-off (bound −40 elsewhere). With one
      damper, 5 kHz's mode has G 4.5, against two dampers' 2.4.
    - **SPREAD moves under a held tone beat its level,** up to 1.9×: the
      diffuser changes the tone's phase into a 3.5 s loop, so the old and
      new phases beat, as PRE-DLY's does (1.4×). The move's d2 follows the
      level within 1.2×: no click rides on it. DENSITY's moves read 1.2×
      at most.
    - **A held tone's tail sits 0.8 Hz over 1 kHz,** steady and not
      walking. That is where the SIZE-1 loop's modes (1.67 Hz apart)
      weight the band. The test holds 1 Hz and 0.25 Hz of walk.
    - **REV › MORE 2** is a new page.
  - **Cost** (static count, `Hall::run`, Thumb release): the per-sample
    loop is 361 instructions a ring sample still, 458 in a SIZE or
    PRE-DLY crossfade, 398 in a SPREAD crossfade or a DENSITY move,
    against 280 for HALL before Task 10b.
    - Against the MIX row (276.5 an output sample) that is about +42 still
      and +91 moving.
    - At 0.47 cycles an instruction: REV HALL about 20–43 cycles, ΔCOST
      about −35 to −12 default (gate 76), −27 to −4 tape (gate 23).
    - At 1.06: about 45–96 cycles, ΔCOST −10 to +41 default, −2 to +49
      tape: over the tape gate while SIZE moves.
    - The bench's REV HALL moves SIZE every block, so it reads the moving
      end. The unit decides at Task 13.
    - `Hall::run` is 18,544 B of text, five monomorphs.
  - **Flash:** plain release `text` 790,340 B, +19,196 over `reverb-types` (8740cfb9): the five read paths.
  - **Provenance:** only `DIFFUSER_MS` carries emulator numbers. The tank,
    the taps, the laws and every knob value are our own.

## Alternatives considered
- **Keep ADR 0085's ±30 % window:** it passes reverbs the owner can tell
  apart from the preset at once. The new bar is set where the harness's own
  repeatability (≤ 1.4 % a band) leaves room to judge.
- **Tune ROOM as built (2 × 2, 3 taps):** its 2.9 ms first arrival, thin
  C50 and sagging density are structural, and no knob setting passes a
  preset.
- **Our own lengths, fitted to the presets (keeping ADR 0085's provenance):**
  possible in principle, but the fit would have to rediscover the structure
  by search, over 30-odd lengths, with no assurance of passing. The owner
  chose the numbers.
- **Copy the unit's gains as knob values:** the float model rings 11–30 %
  longer than the 16-bit unit, so copied gains miss RT60 by more than ±5 %.
- **f32 states for BLOOM's cascade:** they need about 29 k slots, past the
  arena.
- **Vendor the emulator or its renders as test fixtures:** GPL-3.0 code and
  output in an MIT repository. The harness stays external, and the repo's
  own tests check structure and bounds, not the match.

## Consequences
- **ROOM and BLOOM become the unit's structures,** reached by our own code
  with fitted knobs. HALL, GATE and REVERSE are fitted the same way (plan
  Tasks 10b and 11b). Any structural change those fits need is recorded here
  as an "as built" section while this ADR is Proposed.
- **Acceptance moves to the harness.** A TYPE with a counterpart is accepted
  when every preset in its row passes at its fitted knobs, or the owner signs
  off a named miss. The verdicts are recorded in the plan's § Measured. The
  harness isn't in CI, so a TYPE's in-repo tests pin the structure (lengths,
  taps, energy, bounds) and a golden pins the sound.
- **ROOM's cost roughly doubles,** within the headroom, approved.
- **A licensing debt** is open until the owner settles it before V1. If it
  goes against using the numbers, ROOM's and BLOOM's tables are replaced and
  their presets refitted.
- **It rules out** taking knob values or gains from the unit directly, and
  any emulator code or output in the repository.

## Sources
- ADR 0085 and its "ROOM, as built" and "HALL, as built" sections.
- `docs/superpowers/specs/2026-10-04-reverb-types-design.md`, § MidiVerb II
  reference; `docs/superpowers/plans/2026-10-04-reverb-types.md`, Tasks 9b,
  10b, 11, 11b and 13.
- github.com/thement/midiverb_emulator (GPL-3.0): run as an external program
  at 23.4 kHz, with its decompiled programs as the source of the structure
  numbers above. Not vendored.
- The reference harness, outside the repository: the scratchpad's
  `midiverb-ref/` (`README.md`, `run.sh`, `mvlib.py`, `model.py`,
  `room_v2.py`, `bloom_model.py`, `fits/room_v2_best.json`,
  `fits/bloom_model_final.json`, `fits/recon.json`).
- J. S. Abel and P. Huang, "A simple, robust measure of reverberation echo
  density", AES 121st Convention, 2006.
- ISO 3382-2:2008, reverberation time by interrupted noise.
