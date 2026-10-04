# Reverb TYPEs: QUAD plus ROOM, HALL, GATE, REVERSE, BLOOM and ECHO

Status: approved pending owner read (2026-10-04). ADR [0085](../../adr/0085-the-master-reverb-picks-a-type.md) (Proposed) records the decision. The owner's answers of 2026-10-04 are folded in: modulation is deferred to private issue #345, and QUAD keeps its name. Issue: private issue #343. It is queued after ORBIT ships (owner, 2026-10-03).

## Intent

The owner, 2026-10-03:
- The master reverb gets a TYPE setting, and only the selected model runs.
- The TYPEs are families, not presets.
- The method is to study how the Midiverb II programs work and write our own Rust. Then extend past the hardware with an LFO wobble, mod destinations and tempo-synced times.
- No TYPE may cost more CPU than today's reverb.
- All TYPEs share today's delay memory, and nothing grows.

This spec turns that into three engines plus an echo, six new TYPEs beside today's ring, one shared parameter block, and a switch that never clicks.

## Rulings this spec follows

| Ruling (owner, 2026-10-03) | Where it lands |
|---|---|
| A TYPE setting. Only the selected model runs. | § Switching TYPE: one engine at a time, the fade included. |
| TYPEs are families: QUAD, ROOM, HALL/PLATE, GATE, REVERSE, BLOOM, ECHO. The engines are LOOP, BURST and BLOOM, plus ECHO. | § Engines; § PLATE is HALL's corner, not a TYPE. |
| Study github.com/thement/midiverb_emulator (GPL-3.0) as documentation only. Never copy its code or transcribe its ROM coefficient tables. Commit 7119f7ad is not revived. | § Provenance. |
| Extend past the hardware: a loop LFO wobble, mod destinations, and tempo-synced PRE-DLY, GATE LENGTH, ECHO times and optionally DECAY. Mod destinations deferred to #345 (2026-10-04). | § Wobble; § Modulation (deferred); § Tempo sync. |
| CPU never exceeds QUAD's worst case: 485 cycles per 48 kHz output sample on the bench's REVERB row. | § CPU budget. |
| One 23,220-sample i16 arena (46,440 B), and nothing grows. About 1–3 KB of flash per TYPE. | § Arena; § Flash. |
| SIZE glides through interpolated read pointers, not QUAD's double-read crossfade. QUAD stays exactly as it is, crossfade included (2026-10-04). | § Glide; § QUAD. |
| A switch fades the old tail out quickly while the arena clears, with no click and no stale echo. Parameter changes are lerped, never snapped. | § Switching TYPE; § Smoothing. |
| No users yet: old projects load safely, as QUAD. | § Storage. |
| User-facing names are sound names. Any MIX or hold gesture gets a contextual focus-band hint. | § UI. |
| Every review dimension ≥ 8/10. | § Review bar. |

## What the Midiverb II teaches (summary of the #343 spike)

The spike read the emulator's machine model and decompiled programs as documentation, and measured impulse responses. It produced no code and no tables. What carries over:

- **The machine.** 16-bit words in a 16,384-word ring (about 700 ms at 23.4 kHz). 128 instructions per sample. Gains are dyadic (½, ¾, ⅝ …), and the only filters are a one-pole average (about 2.6 kHz, "dark") and a two-sample average (about 5.9 kHz, "bright"). **No reverb program is modulated, and none is tempo-aware.**
- **The families**, found by dataflow-graph shape:
  - diffuse-loop reverbs, 29 presets: input filter → allpass diffuser → N parallel loops of M "delay → allpass" stages → stereo taps; N·M is usually 8;
  - gated, 10 presets: a flat tap burst → decorrelators, with an optional short loop;
  - reverse, 8 presets: a rising tap burst → decorrelators, with optional regen;
  - bloom, 2 presets: about 15 allpasses in one feedback loop;
  - the rest (flange/chorus, echo, multitap, regen delay, decorrelators) are delays.
- **What varies inside a family** is the knob set this spec gives each TYPE:
  - size = the loop's round trip;
  - decay = the per-stage gain;
  - brightness = mostly the *input* filter (about 4.5 dB at 4 kHz between Bright and Dark), with a mild in-loop damper;
  - density = allpass g, ½ or ¾;
  - gate time = the burst span;
  - the reverse ramp = about 24 dB in 6 dB steps.
- **Measured targets**, kept as acceptance windows of ±30 % because the clock and the preset names disagree:

| Family | RT60 | Other |
|---|---|---|
| Loop | 0.27 s to about 2.7 s; Xlarge presets 5.2 and 8.4 s | Loop round trip 120–560 ms; onset 0.6–34 ms; L/R correlation ≈ 0 |
| Gate | Burst end (−20 dB) at 185–660 ms; after the cliff 0.16–0.8 s | |
| Reverse | | Envelope peaks at 300–600 ms, then cuts; L/R correlation 0.4–0.65 |
| Bloom | 8.6 s | Onset 140–225 ms; peak at 550–630 ms |

## Provenance

- The emulator is GPL-3.0, and this workspace was then MIT (`Cargo.toml`; GPL-3.0-or-later since ADR 0330). Nothing from the emulator enters the repository:
  - no code;
  - no instruction listings;
  - no ROM coefficient, offset or tap tables, whole or in part.
- The spike's analysis scripts and notes stay outside the repository. This spec records only prose descriptions of structure and acoustic measurements (RT60, onset, peak, tilt, correlation), which are facts about a sound, not expression.
- The topologies are textbook ideas, older than the Midiverb, and Chimera's own composition of them:
  - allpass diffusers (Schroeder, 1962);
  - delay-and-allpass loops (Gardner, 1992; Dattorro, 1997);
  - tap-burst gated and reverse reverbs (1980s studio practice).
- Every length, gain, tap position, sign pattern and range in this spec is ours, chosen here:
  - lengths are generated by `const fn` as distinct primes from our own base values, the way `ring.rs` builds QUAD's `SIZE_TABLE`;
  - gains follow from RT60 targets by formula;
  - the final values are tuned by ear and by measurement against the Midiverb VST.
- Commit 7119f7ad ("faithful MidiVerb II emulation from decompiled DASP microcode", retired by ADR 0028) is not revived, consulted for values or cherry-picked.
- The review checklist for every engine task has one item for this: no constant may come from the emulator or 7119f7ad. The task's report lists where each table's starting values came from (this spec's tables) and what the ear changed.

## The TYPE list

| # on screen | TYPE (word on screen) | Engine | Disk code | Ident |
|---|---|---|---|---|
| 1 | QUAD (owner's name, 2026-10-04) | today's ring (`dsp::ring`, ADR 0028) | 0 | `QUAD` |
| 2 | ROOM | LOOP, 2 loops × 2 stages | 1 | `ROOM` |
| 3 | HALL | LOOP, 1 loop × 4 stages | 2 | `HALL` |
| 4 | BLOOM | BLOOM | 5 | `BLOOM` |
| 5 | GATE | BURST, falling-or-flat curve, tail loop | 3 | `GATE` |
| 6 | REVERSE | BURST, rising curve, regen | 4 | `REVERSE` |
| 7 | ECHO | ECHO | 6 | `ECHO` |

The screen order runs from the plainest spaces to the effects. Disk codes are frozen in `tests/fixtures/disk_codes_v1.txt` and never reused (ADR 0045). The order on screen is display only.

### PLATE is HALL's corner, not a TYPE

The owner's rule is that TYPEs are families, not presets. A family is a different dataflow graph. ROOM (2 × 2) and HALL (1 × 4) are different graphs on one engine, so both are TYPEs. A plate on the HALL graph differs from a hall only in constants:
- a short loop;
- maximum diffusion;
- a bright input;
- no pre-delay;
- a little wobble.

That makes it a preset. The MV2 itself had no plate topology. So:
- HALL's SIZE range reaches down into plate territory, a 120 ms loop (§ HALL).
- The **plate corner** is a named, tested setting: HALL with SIZE 0, DECAY 0.35, TONE 0, DIFFUSE 1, DAMP 0.1, PRE-DLY 0, WOBBLE 0.5 and RATE 0.6. It has its own golden and its own A/B render.
- A focus-band hint on HALL's SIZE and DIFFUSE says how to get there (§ Hints).

Reversing this later is cheap. A PLATE TYPE would be a new disk code (7) mapped to the HALL engine with its own panel row and ranges: about 200 B of flash and no DSP. Nothing in this design blocks it.

## Engines

### Shared machinery

- **Rate.** Every engine runs at half rate, 24 kHz on the chip, behind today's half-band decimator and two interpolators (ADR 0028). They are shared and unchanged. The band limit stays the dark top every TYPE shares.
- **Storage.**
  - Every line is i16, full scale ±2.0, in the one arena (§ Arena).
  - GRIT's grid applies to every write in every TYPE: Δ = 2^(6·GRIT) LSBs (ADR 0028).
  - ADR 0028's Q3 rule carries over: allpass state writes truncate toward zero, and delay, burst and damper writes round.
  - FREEZE is not in this spec. No loop gain reaches 1.
- **Interpolated reads.**
  - Every read whose length can move, through a SIZE glide, a tempo change, a pre-delay change or the wobble, is linear: `x0 + f·(x1 − x0)`, from two adjacent slots (usually one cache line).
  - The new engines always read this way, fraction 0 included. There is one code path, no `GLIDE` const generic and no branch, so the steady cost *is* the worst-case cost. That is what `FxBus::COST` bills anyway, and it halves the flash a two-path version would take.
  - QUAD keeps its own crossfade code untouched (§ QUAD).
- **Allpass.** `v = x − g·z` is written (truncated onto the grid), and `y = z + g·v`. Same form as QUAD's.
- **Tone filter.** One one-pole lowpass per engine on the input, after the decimator: `y += a·(x − y)`, with `a = 1 − exp(−2π·fc / 24 000)`. TONE sets fc, bright (0) to dark (1). This is where MV2's Bright, Warm and Dark live.
- **Dampers.** One-pole lowpasses inside loops, written onto the grid with rounding, like QUAD's DAMP.
- **Decay.**
  - Each loop stage's gain is `g_k = 10^(−3·L_k / (RT60·fs))`, where L_k is the stage's share of the round trip. The cap is `MAX_GAIN` 0.97. This is QUAD's `stage_gains`.
  - RT60 has a floor at `t_min`, the longest allpass's own ring, as in QUAD.
  - Gains ramp across each block from the last block's value (QUAD's `RingBlock`).
- **The wobble LFO.**
  - One per engine: a `u32` phase and a branch-free parabolic sine, the same idiom as the chorus's LFO (ADR 0031, `335cd3a`).
  - The two modulated reads per side, or per loop, run 90° apart, so the left and right tails move differently.
  - It is computed at the block's ends and interpolated linearly across the block's 32 ring samples. At ≤ 7 Hz the error is below 10⁻⁴ of the depth.
- **Output.** The return is ×`WET[type]` × MIX, eased as today. Each TYPE's `WET` is set so its default return's RMS matches QUAD's default within ±1 dB. A test pins this (§ Tests); QUAD's `WET_GAIN` 4.54 is unchanged.

### Glide

New TYPEs never crossfade; QUAD always does (owner ruling, 2026-10-04: QUAD stays exactly as it is, with a clean, pitch-free crossfade; the new TYPEs' slight pitch bend is part of their vintage character).

- **Targets.** Each line length is a target in f32 samples: SIZE's mapping or the tempo-synced value.
- **Following.**
  1. The line's read length follows the target through a one-pole, τ = 100 ms, once a block.
  2. It is then slew-limited to at most **⅛ sample per sample**, which bounds the Doppler shift on a big twist to −2.3 / +2.0 semitones.
  3. It is interpolated linearly across the block.
- **What it sounds like.** A small turn gives a slight bend. A full sweep of HALL's SIZE (120 → 600 ms, 11,520 samples) takes about 3.8 s at the slew limit, with the old tail sliding in pitch. The spike notes that the MV2's LFO-moved programs did the same.
- **Tuning.** The constants (τ and the slew limit) are starting values to tune by ear at A/B. Tests pin the invariants: continuity, the pitch bound, and that the length lands on its target.

### LOOP engine: ROOM and HALL

```
send → TONE → PRE-DLY → diffuser (4 AP, fixed) ─┬─► loop A: [delay → AP] → [delay → AP] → DAMP → ×g ─┐
                                                 │      ▲________________________________________────┘
                                                 └─► loop B (ROOM only): same shape, own lengths
outputs: 3 taps per side, read from different stage delays, signs ±; L and R taps on different lines
```

- The engine is one generic, `LoopEngine<const SHAPE: LoopShape>`, monomorphised twice: ROOM is 2 loops × 2 stages, HALL is 1 loop × 4 stages.
- **SIZE** scales only the stage *delays*, by k; the stage allpasses and the diffuser stay fixed. Fixed allpasses keep their reads integer-cheap and their colour constant.
- In ROOM the send enters loop A at +½ and loop B at −½, as QUAD injects into S1 and S3. The two loops cross-feed: A's output adds into B's input at −¼, and B's into A's at +¼. This is a light coupling, so the sides share one decay.
- **Dampers.** ROOM has one per loop. HALL has two, after stages 2 and 4.
- **Wobble.** ROOM moves one stage delay per loop (A2 and B1). HALL moves stages 1 and 3, in quadrature.

Starting lengths in ms at SIZE 1. Each becomes the nearest distinct prime in samples at 24 kHz, as `ring.rs` builds QUAD's:

| | Diffuser APs | Stage 1 delay / AP | Stage 2 delay / AP | Stage 3 delay / AP | Stage 4 delay / AP | k (SIZE 0 → 1) |
|---|---|---|---|---|---|---|
| ROOM loop A | 2.3, 3.7, 5.9, 8.9 | 59 / 17 | 101 / 23 | | | 0.375 → 1 (round trip 100 → 200 ms) |
| ROOM loop B | (shared) | 73 / 13 | 95 / 19 | | | same k |
| HALL | 1.7, 3.1, 5.3, 8.9 | 113 / 11 | 157 / 17 | 124 / 13 | 146 / 19 | 0.111 → 1 (round trip 120 → 600 ms) |

Output taps, as (line, fraction of its current length, sign):
- **ROOM:** L = (A1, 0.31, +), (B2, 0.57, −), (A2, 0.83, +). R = (B1, 0.29, +), (A2, 0.53, −), (B2, 0.79, +).
- **HALL:** L = (S1, 0.37, +), (S3, 0.61, −), (S2, 0.83, +). R = (S2, 0.23, +), (S4, 0.49, −), (S1, 0.91, +).

The taps move with SIZE, so they read interpolated.

### BURST engine: GATE and REVERSE

```
send → TONE → pre-diffuser (2 AP) → burst line ──► 12 taps over [0, LENGTH], weights w_k, signs s_k ─► Σ ─┬─► decorrelator L (2 AP) → L
                     ▲                                    │                                         └─► decorrelator R (2 AP) → R
                     └── REGEN (REVERSE): line end × g ───┘          GATE: Σ also feeds a 1-stage TAIL loop (delay → AP → ×g), added back to Σ
```

- **One engine, one code path.** GATE runs with REGEN's gain at 0, and REVERSE with TAIL's gain at 0. Both pay for both paths, 6 accesses (§ CPU budget), so there is only one monomorph.
- **Taps.**
  - 12 taps. Tap k sits at `LENGTH·(k + j_k)/11` for k = 0…11.
  - j_0 = j_11 = 0. The others are a fixed jitter within ±0.3, from a `const fn` LCG seeded in this spec (seed `0x5EED_0343`). The jitter breaks the comb a regular spacing would ring.
  - Signs s_k are a fixed ± pattern from the same LCG. Taps read interpolated, since LENGTH glides.
- **Curves.**
  - GATE: `w_k ∝ 10^(−SHAPE_dB·k/11/20)`, with SHAPE_dB = 18·SHAPE. Flat (a hard cut) to a falling 18 dB (the "dying" slow gate).
  - REVERSE: `w_k ∝ 10^(−CURVE_dB·(11−k)/11/20)`, with CURVE_dB = 12 + 18·CURVE. A rising ramp of 12–30 dB; the MV2 used about 24 in steps, ours is smooth.
  - The weights are normalised so the sum of their squares is constant: loudness holds as SHAPE, CURVE and DENSITY move.
- **DENSITY.**
  - Taps 6–11 fade in one after another across the knob: tap 6 + i weighs `clamp(6·DENSITY − i, 0, 1)` before normalising, so 6 taps at 0 and 12 at 1, never a step.
  - DENSITY also sets the pre-diffuser's g, 0.5 → 0.7.
  - Diffusing *before* the taps is what lets 12 reads do the work of the MV2's 47–90.
- **TAIL (GATE).** The burst sum also feeds one loop stage: a 37 ms delay, then an 11 ms AP. Its output is ×g and added back into the sum. g comes from TAIL's RT60 and is scaled by `min(1, TAIL/0.05)`, so TAIL 0 is exactly the fast gate with no step.
- **REGEN (REVERSE).** The tap at LENGTH is fed back into the burst line's input through a one-pole at about 6 kHz, so each pass darkens. The gain is g = 0.708·REGEN, up to −3 dB.
- **Decorrelators.**
  - Two APs per side at g 0.6: L 3.1 and 8.3 ms, R 4.7 and 6.1 ms.
  - Each side's second AP is the one the wobble moves.
  - The taps sum mono first (the MV2 shape), and the decorrelators make the stereo.

### BLOOM engine

```
send → PRE-DLY → (+) → AP1 → AP2 → … → AP10 → DAMP(TONE) → ×g ─┐
                  ▲_____________________________________________┘
L = AP5's output → decorrelator L (2 AP) ;  R = AP10's output → decorrelator R (2 AP)
```

- **The loop** is ten allpasses and no plain delays. The density keeps building for several passes, and that is the swell.
- **Lengths** at SWELL 1: 13, 23, 37, 53, 61, 79, 89, 103, 113 and 127 ms, a round trip of about 698 ms. SWELL scales all ten by k = 0.358 → 1, giving 250 → 700 ms. They glide, so the reads are interpolated.
- **TONE** is the in-loop damper here (fc 10 kHz → 1.5 kHz). The bloom's darkness lives in the loop, as the spike measured: HF RT60 about 0.9× LF.
- **DIFFUSE** sets the loop APs' g, 0.45 → 0.65.
- **Taps.** The outputs are values already in registers (AP5's and AP10's outputs), not reads, so they cost nothing.
- **Decorrelators:** L 4.1 and 9.7 ms, R 5.3 and 7.9 ms, at g 0.6.
- **Wobble** moves APs 3, 6 and 9: L-phase, quadrature and R-phase.

### ECHO engine

```
send → (+) → line ──► 6 taps at k/6 · TIME (k = 1…6), per-side weights by PATTERN ─► Σ_L, Σ_R → 2 AP per side (DIFFUSE) → L, R
        ▲                                │
        └── tap 6 × REPEATS ← DAMP(TONE) ┘
```

- **Taps.** Six taps at sixths of TIME. Sixths hold halves and thirds, which is every pattern below. Both sides read the same six slots, so it costs 6 reads and not 12. They read interpolated, since TIME glides and WOBBLE moves them.
- **PATTERN** sets per-side weights on the same six taps. A change eases the weights over 20 ms, so it crossfades and never snaps.

| PATTERN | Taps (k: L / R weight) | After |
|---|---|---|
| SINGLE | 6: 1 / 1 | MV2 single echo |
| PAIR | 3: ½ / ½; 6: 1 / 1 | MV2 "2 Tap Ambient" (best with DIFFUSE up) |
| PAN | 2: 1 / 0; 4: ½ / ½; 6: 0 / 1 | MV2 "3 Tap Pan" |
| CLOUD | 1…6 falling 6 dB per tap, alternating sides and signs | MV2 "Multitap" |
| RISE | 1…6 rising 6 dB per tap, alternating sides and signs | MV2 "Multitap Reverse Pan" |

- **Feedback** always comes from tap 6 (TIME), through the TONE damper, ×REPEATS.
- **DIFFUSE** sets the output APs' g from 0 (clean repeats) to 0.7 (smeared). The AP lengths are L 3.7 and 7.9 ms, R 4.3 and 6.7 ms.
- **ECHO is not a second delay.** The per-Part TAPE delay (ADR 0053) is a mono tape echo with MECHANICS and SAT. ECHO is the master space's echo: multitap, stereo, diffused, synced. The owner approved it as the sixth new TYPE.

### QUAD

Owner ruling (2026-10-04): leave QUAD exactly as it is.
- QUAD is today's `dsp::ring`, bit for bit. Its sound and its cost are unchanged.
- Its only change is internal: `Ring` borrows the arena instead of owning `lines`.
- It keeps its 32-step double-read SIZE crossfade, which is clean and pitch-free. That crossfade is also its measured worst case, the 485 every other TYPE is held under.
- Only the new TYPEs glide through interpolated read pointers (§ Glide). Their slight pitch bend is part of their vintage character.
- QUAD shows no MORE knobs, takes no wobble or sync, and its goldens do not move (ADR 0011).

## Arena

- The arena is `ReverbArena { lines: [i16; 23_220] }`, 46,440 B in `FxBus` (AXI, ADR 0014). It is today's `Ring::lines` moved out, unchanged in size.
- Each TYPE has a `const` layout:
  - every line owns `max_len + 2` slots: its longest length, plus the wobble's depth, plus one slot for the interpolation's second read;
  - all lines rotate with one shared write head, `ring.rs`'s scheme (`OFFSET`, `HEAD`, wrap-free segments);
  - a `const` assert per TYPE proves its layout fits in 23,220.

| TYPE | Lines at their maximum (samples at 24 kHz) | Total | Headroom |
|---|---|---|---|
| QUAD | 12 lines (`RING_LEN`) | 23,220 | 0 |
| ROOM | PRE 1,200 · diffuser 498 · delays at k = 1 7,872 · stage APs 1,728 · wobble 48 | 11,346 | 11,874 |
| HALL | PRE 3,000 · diffuser 456 · delays at k = 1 12,960 · stage APs 1,440 · wobble 96 | 17,952 | 5,268 |
| GATE, REVERSE | pre-diffuser 274 · burst line 19,200 (REVERSE's 800 ms) · TAIL loop 1,152 · decorrelators 533 · wobble 48 | 21,207 | 2,013 |
| BLOOM | PRE 3,000 · loop APs 16,752 · decorrelators 648 · wobble 144 | 20,544 | 2,676 |
| ECHO | line 21,600 · APs 542 · wobble 240 | 22,382 | 838 |

Each line also takes its 2 guard slots (under 50 per TYPE), inside the headroom shown. The arena and `RING_LEN` do not change.

**State beside the arena.** Each engine's small state (dampers, the tone filter, the LFO phase, current lengths, gains) lives in one `enum EngineState` sized to the largest variant. It targets ≤ 160 B; a `memory_budget_test` assert pins it. That is the only growth in `FxBus`, and the arena does not grow.

## Parameters

### One block, shared slots

- The reverb stays one block, `BlockRef::Reverb` (disk code 22), with one static `REVERB_SPECS`.
- The storage codec walks the static table (`ValidAddr::of_block`) and caps a block at 32 params (`MAX_BLOCK_PARAMS`).
- Per-TYPE specs would need about 7 × 8 = 56 ids, so they are out.
- Instead each id is a **semantic slot**, stored 0..1 like today's reverb params. A per-TYPE **panel** (`ui/reverb_panel.rs`, `const` data, the pattern ADR 0037's `filter_panel.rs` set) gives each slot its label, its range and its unit under that TYPE, or marks it absent.

| ParamId | Ident (frozen) | Kind | Slot meaning | Spec default | New? |
|---|---|---|---|---|---|
| 0 | — | — | retired TYPE of the pre-diet bus, never reused (ADR 0009, 0028) | — | — |
| 1 | `TIME` | continuous 0..1 | how long it lasts: QUAD TIME, DECAY, TAIL, REGEN, REPEATS | 0.5 | no |
| 2 | `DAMP` | continuous 0..1 | in-loop HF damping: QUAD DAMP, HALL DAMP | 0.3 | no |
| 3 | `SIZE` | continuous 0..1 | how big or how long the space is: SIZE, LENGTH, SWELL, ECHO TIME | 0.5 | no |
| 4 | `MIX` | continuous 0..1 | return level | 0.0 | no |
| 5 | `GRIT` | continuous 0..1 | the i16 grid | 0.3 | no |
| 6 | `TYPE` | Enum, `ReverbType` | the TYPE | QUAD | yes |
| 7 | `TONE` | continuous 0..1 | input brightness (BLOOM and ECHO: the in-loop damper) | 0.3 | yes |
| 8 | `DIFFUSE` | continuous 0..1 | density: DIFFUSE, DENSITY | 0.4 | yes |
| 9 | `PREDELAY` | continuous 0..1 | PRE-DLY | 0.0 | yes |
| 10 | `SHAPE` | continuous 0..1 | the burst's curve: GATE SHAPE, REVERSE CURVE | 0.2 | yes |
| 11 | `WOBBLE` | continuous 0..1 | the LFO's depth | 0.0 | yes |
| 12 | `RATE` | continuous 0..1 | the LFO's rate | 0.5 | yes |
| 13 | `SYNC` | Enum, `RevSync` | OFF, TIME, ALL | OFF | yes |
| 14 | `PATTERN` | Enum, `EchoPattern` | ECHO's tap pattern | SINGLE | yes |

That is 14 ids of the 32.

**Typed choices.** `ReverbType`, `RevSync` and `EchoPattern` are enums with explicit `DiskCode` matches, never discriminants (ADR 0045):
- `RevSync`: OFF 0, TIME 1, ALL 2.
- `EchoPattern`: SINGLE 0, PAIR 1, PAN 2, CLOUD 3, RISE 4.

The fixture gains rows `B 22 6 TYPE` … `B 22 14 PATTERN` and their `E` rows.

**Pure mappings.** Every mapping from a knob to a quantity is a pure function of (TYPE, slot value, tempo), in `dsp::reverb` (functional core):
- `exp(lo, hi, p) = lo·(hi/lo)^p`;
- `lin(lo, hi, p)`.

The DSP and the UI's value text call the same functions.

### Each TYPE's knobs

Values are p (0..1) → quantity. The main page is cells 2–5, and REV › MORE is the sub-page (§ Pages). "—" means absent.

**ROOM** (main: SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SIZE | 3 | exp | loop 100–200 ms | 0.5 → 141 ms |
| DECAY | 1 | exp | RT60 0.2–1.5 s (floor `t_min`) | 0.5 → 0.55 s |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0.3 → 6.6 kHz |
| DIFFUSE | 8 | lin | AP g 0.5–0.75 | 0.4 → 0.60 |
| PRE-DLY | 9 | lin | 0–50 ms | 0.1 → 5 ms |
| WOBBLE | 11 | lin | ±0–1 ms | 0.3 → ±0.3 ms |
| RATE | 12 | exp | 0.1–1 Hz | 0.5 → 0.32 Hz |

**HALL** (main: SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · DAMP · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SIZE | 3 | exp | loop 120–600 ms | 0.65 → 342 ms |
| DECAY | 1 | exp | RT60 0.8–10 s (floor `t_min`) | 0.45 → 2.5 s |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0.3 → 6.6 kHz |
| DIFFUSE | 8 | lin | diffuser and stage AP g 0.5–0.75 | 0.4 → 0.60 |
| DAMP | 2 | exp | in-loop fc 11 → 1.5 kHz (QUAD's `damp_coef` law) | 0.3 → 6.1 kHz |
| PRE-DLY | 9 | lin | 0–125 ms | 0.15 → 19 ms |
| WOBBLE | 11 | lin | ±0–2 ms | 0.35 → ±0.7 ms |
| RATE | 12 | exp | 0.1–1.5 Hz | 0.4 → 0.30 Hz |

The plate corner is SIZE 0 (120 ms), DECAY 0.35 (1.9 s), TONE 0 (10 kHz), DIFFUSE 1 (0.75), DAMP 0.1, PRE-DLY 0, WOBBLE 0.5 (±1 ms) and RATE 0.6 (0.51 Hz).

**GATE** (main: LENGTH · TAIL · TONE · DENSITY; MORE: — · SHAPE · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| LENGTH | 3 | exp | burst 40–600 ms | 0.55 → 177 ms |
| TAIL | 1 | exp, gain × min(1, p/0.05) | RT60 after the burst 0.15–0.8 s; 0 = none | 0.3 → 0.25 s |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0.2 → 7.6 kHz |
| DENSITY | 8 | lin | taps 6–12; pre-diffuser g 0.5–0.7 | 0.7 → 10.2 taps, g 0.64 |
| SHAPE | 10 | lin | flat → falling 18 dB | 0.2 → −3.6 dB |
| WOBBLE | 11 | lin | ±0–1 ms (decorrelators) | 0 → off |
| RATE | 12 | exp | 0.1–2 Hz | 0.5 → 0.45 Hz |

**REVERSE** (main: LENGTH · REGEN · TONE · DENSITY; MORE: — · CURVE · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| LENGTH | 3 | exp | burst 150–800 ms | 0.5 → 346 ms |
| REGEN | 1 | lin (amplitude) | feedback 0 → 0.708 (−3 dB): RT60 up to about 12 s at 800 ms | 0 → none |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0 → 10 kHz |
| DENSITY | 8 | lin | taps 6–12; pre-diffuser g 0.5–0.7 | 0.8 → 10.8 taps, g 0.66 |
| CURVE | 10 | lin | rising ramp 12–30 dB | 0.67 → 24 dB |
| WOBBLE | 11 | lin | ±0–1 ms | 0 → off |
| RATE | 12 | exp | 0.1–2 Hz | 0.5 → 0.45 Hz |

**BLOOM** (main: SWELL · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SWELL | 3 | exp | loop 250–700 ms (time to peak about 0.3–0.75 s) | 0.7 → 514 ms |
| DECAY | 1 | exp | RT60 2–12 s | 0.6 → 5.9 s |
| TONE | 7 | exp | in-loop fc 10 → 1.5 kHz | 0.35 → 5.1 kHz |
| DIFFUSE | 8 | lin | loop AP g 0.45–0.65 | 0.5 → 0.55 |
| PRE-DLY | 9 | lin | 0–125 ms | 0 → 0 ms |
| WOBBLE | 11 | lin | ±0–2 ms | 0.3 → ±0.6 ms |
| RATE | 12 | exp | 0.05–0.5 Hz | 0.4 → 0.13 Hz |

**ECHO** (main: TIME · REPEATS · TONE · DIFFUSE; MORE: — · PATTERN · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| TIME | 3 | exp | 20–900 ms | 0.7 → 287 ms |
| REPEATS | 1 | lin (amplitude) | feedback 0 → 0.891 (−1 dB) | 0.4 → 0.36 (−9 dB) |
| TONE | 7 | exp | feedback damper fc 10 → 1.5 kHz | 0.3 → 5.7 kHz |
| DIFFUSE | 8 | lin | output AP g 0–0.7 | 0 → clean |
| PATTERN | 14 | choice | SINGLE, PAIR, PAN, CLOUD, RISE | SINGLE |
| WOBBLE | 11 | lin | ±0–5 ms (all taps) | 0 → off |
| RATE | 12 | exp | 0.1–7 Hz | 0.3 → 0.36 Hz |

With a short TIME and WOBBLE up, ECHO is the MV2's flange and chorus family.

**QUAD** (main: SIZE · TIME · DAMP · GRIT; MORE: all absent). Its ids, ranges and laws are today's (ADR 0028). GRIT moves to the main page for QUAD only, because QUAD has no MORE knobs.

### A TYPE change loads that TYPE's starting knobs

The issue's naming ruling (owner, 2026-10-03): "Each TYPE is an engine … plus its tuned settings. The existing knobs … still adjust from that starting point." So:
- Turning TYPE in the UI writes, in one edit, the new TYPE's defaults (the tables above) into slots 1, 2, 3, 7, 8, 9, 10, 11, 12 and 14.
- MIX, GRIT and SYNC are kept: they are the player's level, grain and clock.
- `ReverbParams::with_type(t) -> ReverbParams` is the pure function, and the UI's TYPE edit calls it. `Block::write`, decode and MIDI-CC writes of TYPE only store the TYPE. A project or Sound load therefore keeps its stored knobs.
- Leaving a TYPE and coming back gives its starting point, not the last tweak. Per-TYPE memory would need either 56 stored ids or a UI cache that a save would drop.

## Switching TYPE

The audio thread runs one engine. A change of `params.reverb.kind` from the one running starts this sequence (`BLOCK_SIZE` 64, 48 kHz):

| Phase | Blocks | Engine | Return |
|---|---|---|---|
| FADE OUT | 15 (20 ms) | the old TYPE runs on its own lines | its return × a linear ramp 1 → 0, applied after the interpolators |
| CLEAR | 4 (5.3 ms) | none runs | exactly 0 |
| FADE IN | 15 (20 ms) | the new TYPE, fresh state, knobs primed (no glide from the old TYPE's values) | × a linear ramp 0 → 1 |

- **CLEAR.**
  - Each block zeroes one quarter of the arena: 5,805 samples, 11,610 B, as 64-bit stores (about 2–3 k cycles; § CPU budget).
  - The last CLEAR block also zeroes the new engine's state and both interpolators' histories, so no sample of the old tail survives anywhere.
  - The decimator's history is the send's own and is kept.
- **The send during CLEAR** is dropped. A note struck in those 5.3 ms gets no reverb.
- **A change during a switch.**
  - The target follows the newest TYPE. In FADE OUT or CLEAR the sequence simply continues.
  - In FADE IN a new FADE OUT starts from the current ramp value, never from 1.
  - Scrolling through TYPEs plays silence, then the last TYPE.
- **Under the switch.**
  - A project swap (ADR 0046) carrying another TYPE uses the same path.
  - At MIX 0 the switch still runs, silently (ADR 0061).
- **No stale echo.** After the switch the engine is bit for bit a fresh engine fed the same send from the first FADE IN block. A test asserts this (§ Tests).

## Smoothing

- **Continuous knobs.** Every continuous knob eases: lengths by the glide (§ Glide); gains, TONE, DAMP, DIFFUSE, SHAPE and the curve weights by a per-block one-pole (τ 50 ms for DECAY, as QUAD's TIME, and 20 ms for the rest), ramped across the block. GRIT eases as in QUAD (20 ms), and MIX by `Ease` (ADR 0061).
- **Discrete choices.**
  - PATTERN crossfades its weights over 20 ms.
  - SYNC changes a length's target, which glides.
  - TYPE runs the switch.
- **Nothing snaps** except a fresh engine's priming inside FADE IN, which no one can hear move.

## Wobble

The owner asked for an LFO wobble on the loops. The MV2's reverbs were static, which is why its small bright rooms ring metallic.
- Every new TYPE has WOBBLE (depth) and RATE.
- It is on by default in ROOM, HALL and BLOOM, and off in GATE, REVERSE and ECHO.
- Which reads it moves:
  - ROOM: one stage delay per loop.
  - HALL: stages 1 and 3.
  - BLOOM: APs 3, 6 and 9.
  - GATE and REVERSE: the second decorrelator AP per side.
  - ECHO: all six taps.
- The moved reads are interpolated already (§ Shared machinery), so the wobble adds the LFO's per-block maths and nothing per access.

## Modulation (deferred to #345)

Owner, 2026-10-04: every mod source today lives inside a Part, and nothing exists at the mixer level. A mixer-level LFO or matrix gets its own thought before anything routes into shared FX (private issue #345).
- **This build has no external mod destinations.** Every reverb spec stays `modulatable: false` (ADR 0010 unchanged). The only motion is each TYPE's own wobble (§ Wobble), set by its WOBBLE and RATE knobs.
- **Out of scope, deferred to #345:** Part-matrix routes into the reverb, an envelope follower or DUCK on the send, and MIX+PLUS priming on reverb cells.
- **Kept open at no cost.** Adding destinations later changes no disk code: modulatability is a spec flag, not stored, and the natural targets (slots 1, 3, 7 and 11) already exist as ids. Every knob maps through one pure function of its stored value, so an offset can later be added before the mapping without touching storage. No slot is reserved, because none is needed.

## Tempo sync

- **Source.** The ORBIT clock (ADR 0071, 0072, 0080): the internal BPM, or the leading MIDI clock's average under SYNC EXT. The runner already has it on the audio thread each block.
  - `FxBus::process` takes `Tempo`, a newtype of samples per beat.
  - Until ORBIT is on `main`, `Tempo` is 120 BPM and SYNC is hidden; the tempo step in § Build order waits for ORBIT.
- **SYNC** (slot 13, MORE page): OFF, TIME or ALL.

| SYNC | ROOM, HALL, BLOOM | GATE, REVERSE | ECHO | QUAD |
|---|---|---|---|---|
| OFF | ms | ms | ms | — |
| TIME | PRE-DLY in note values | LENGTH in note values | TIME in note values (the taps stay on its sixths) | — |
| ALL | as TIME, and DECAY in beats | as TIME | as TIME | — |

- **Note values.** A synced knob walks a list of divisions, one encoder tick each (the panel gives the cell its step). Each TYPE's list is the divisions that fall inside its ms range at 120 BPM:

| Knob | Divisions (at 120 BPM: ms) |
|---|---|
| ROOM PRE-DLY | 0, 1/128 (16), 1/64T (21), 1/64 (31), 1/32T (42) |
| HALL, BLOOM PRE-DLY | 0, 1/128, 1/64T, 1/64, 1/32T, 1/32 (63), 1/16T (83), 1/32D (94), 1/16 (125) |
| GATE LENGTH | 1/32T (42), 1/32, 1/16T, 1/32D, 1/16, 1/8T (167), 1/16D (188), 1/8 (250), 1/4T (333), 1/8D (375), 1/4 (500) |
| REVERSE LENGTH | 1/8T (167), 1/16D, 1/8, 1/4T, 1/8D, 1/4 (500), 1/2T (667), 1/4D (750) |
| ECHO TIME | 1/64T (21) … 1/4D (750): every division between |
| DECAY (ALL) | 1, 2, 3, 4, 6, 8, 12, 16, 24 or 32 beats |

- **Out of range.**
  - A synced time longer than its TYPE's maximum at the current tempo **halves until it fits**; one shorter than the minimum **doubles**. It stays on the grid, so 1/4 at 55 BPM plays 1/8.
  - A beat DECAY outside its TYPE's seconds range is **clamped**, since a decay has no grid to stay on.
  - The cell shows the chosen division, and the hint names what plays (§ Hints).
  - The arena never grows for a slow tempo. The spike's alternative, about 800 more samples so ECHO fits a quarter at 60 BPM, is ruled out by "nothing grows".
- **A tempo change** moves the targets, and the lengths glide (§ Glide).
- **The fold** (`fold(division, tempo, lo, hi) -> Samples`) is a pure function with its own unit tests.

## UI

### Pages

- **REV** (`EFX`, mixer chain, unchanged id 16): TYPE · knob 1 · knob 2 · knob 3 · knob 4 · MIX. Cells 2–5 come from the TYPE's panel. MIX stays last on every TYPE, so the level is always in the same place.
- **REV › MORE** is a new sub-page, short `MORE`, `ChainBlock::with_subs` as DLY › CHAR. Its cells are PRE-DLY · DAMP, SHAPE, CURVE or PATTERN · WOBBLE · RATE · SYNC · GRIT, by panel. Under QUAD every MORE cell is absent.
- **How the cells look.**
  - An absent cell draws the dash, a dimmed one the dim look; `renderer::look` decides, as for the filter (ADR 0037).
  - A cell's label and value text come from the panel: units such as `141ms`, `2.5s`, `6.6k`, `0.30Hz`, `-9dB` and `1/8`, from the same pure mappings the DSP uses.
  - The static spec's own label (`SIZE`, `TONE` …) is what the matrix and the MIDI MAP page show.
- **The demo page** `DEMO_GLYPH_CUBE` stays bound to QUAD's slots.

### TYPE and the words

- TYPE is a word choice, so it takes NONE (ADR 0067); SYNC and PATTERN do too.
- The existing test that every NONE word fits the band at the focus size covers `REVERSE` and `PATTERN`'s `SINGLE`.
- QUAD keeps the word `QUAD` on screen: the owner named it (2026-10-04).

### Glyph

The cube (ADR 0067) is QUAD's picture of a room. For the new TYPEs a cube is the wrong picture, and the owner asked to keep it cheap. So:
- **One composite, two drawings.** `CompositeId::ReverbCube` becomes `CompositeId::Reverb`. The renderer's composite callback reads TYPE and draws the **cube** under QUAD and the **TAIL** under the others. Every reverb param keeps one glyph on its spec, so ADR 0067's "no per-slot override" rule holds. This amends 0067's "the reverb shows the cube" and is recorded in ADR 0085.
- **TAIL** is the TYPE's energy envelope, drawn as a 60-column polyline in the cube's 64 × 56 box.
  - It comes from set values only, by a pure `tail_shape(type, knobs) -> [u8; 60]`:
    - ROOM, HALL: a pre-delay gap, then an exponential fall;
    - GATE: a plateau and a cliff, sloped by SHAPE, plus the TAIL;
    - REVERSE: a rising ramp that cuts dead, with echoes of it under REGEN;
    - BLOOM: a swell to its peak, then the long fall;
    - ECHO: a comb of spikes falling by REPEATS, placed by PATTERN.
  - A playhead dot crosses it at the TYPE's real time scale on the UI clock.
  - WOBBLE ripples the line by up to ±1 px at RATE. GRIT drops grains along it, new each frame, as the cube's do. TONE dims the area under the line from solid (bright) to dotted (dark). The focused knob's part of the shape is drawn in the accent.
  - The x axis spans the TYPE's own range (ROOM 1.5 s, HALL 10 s, BLOOM 12 s, GATE 1.4 s, REVERSE 2 s, ECHO 6 × TIME), so a turn always moves the shape.
- **Cost.** One polyline and a dot per frame, inside the box rules ADR 0067 set: only its box is cleared and flushed while it animates.

### Hints

- **No new gesture.** Every new control is a knob, and navigation stays the D-pad (ADR 0081 on the ORBIT branch; the owner's rule of 2026-10-03).
- **No reverb cell takes a gesture.** MIX+PLUS priming stays off them, since nothing routes into the reverb (§ Modulation, deferred to #345).
- **Where a hint shows.** The focus band's status line, where `PrimeStatus` shows elsewhere (ADR 0017). It shows while nothing is pending; a status replaces it until the next input.
- **One pure function** chooses the hint: `rev_hint(type, focused slot, values, tempo) -> Option<RevHint>`. A `RevHint` is either a static string or a fold (played division, BPM), formatted at draw time.
- A test checks every hint fits the band at `FONT_LABEL`.

| Focus | Condition | Hint |
|---|---|---|
| TYPE | always | `TURN: NEW SPACE, ITS OWN KNOBS` |
| HALL SIZE or DIFFUSE | SIZE ≤ 0.15 or DIFFUSE ≥ 0.85 | `SMALL SIZE + HIGH DIFFUSE = PLATE` |
| a synced cell | the division folded | `PLAYS 1/8 AT 55 BPM` (the actual values) |
| DECAY | the RT60 floor (`t_min`) is binding | `SHORTEST AT THIS SIZE` |
| SYNC | ALL on GATE, REVERSE or ECHO | `NO DECAY TO SYNC: SAME AS TIME` |

## CPU budget

- **The gate.** On the chip, each TYPE's bench row ≤ the REV QUAD row on the same build. Today that row is the REVERB row, 485 cycles per 48 kHz output sample (ADR 0031). Before the chip, each TYPE's host instruction count (the emulator method of ADR 0031) must be ≤ QUAD's crossfading count, 239 per ring sample.
- **The design rule** is that a TYPE's line accesses per ring sample stay at or under QUAD's crossfading 48.
  - That rule is conservative. QUAD's 48 include 18 crossfade reads at unrelated addresses. An interpolated pair reads two adjacent slots, so the second read usually hits the line the first one missed.
  - The estimate scales QUAD's measured 485 = 40 (half-bands and glue) + 48 × 9.3.
  - Every read of a new TYPE is the interpolated pair (§ Shared machinery), so its steady cost is its worst.

| TYPE | Writes | Reads (pairs ×2) | Accesses per ring sample | Estimated cycles per output sample | Margin to 485 |
|---|---|---|---|---|---|
| QUAD | 12 | 18 steady / 36 crossfading | 30 / **48** | 320 / **485 measured** | 0 |
| ROOM | PRE 1 · diffuser 4 · loops 8 | PRE 2 · diffuser 4 · delays 8 · APs 4 · taps 12 | 43 | ≈ 440 | ≈ 9 % |
| HALL | PRE 1 · diffuser 4 · loop 8 | PRE 2 · diffuser 4 · delays 8 · APs 4 · taps 12 | 43 | ≈ 440 | ≈ 9 % |
| GATE, REVERSE | line 1 · pre-diffuser 2 · TAIL 2 · decorrelators 4 | pre-diffuser 2 · taps 24 · TAIL 2 · regen 2 · decorrelators 6 | 45 | ≈ 460 | ≈ 5 % |
| BLOOM | PRE 1 · loop 10 · decorrelators 4 | PRE 2 · loop 20 · decorrelators 4 | 41 | ≈ 420 | ≈ 13 % |
| ECHO | line 1 · APs 4 | taps 12 · APs 4 | 21 | ≈ 235 | ≈ 52 % |
| switch, CLEAR block | 1,452 × 64-bit stores | — | — | ≈ 35–50 (no engine runs) | — |

- **Per-sample maths** besides the accesses: tone and dampers (≤ 3 one-poles), gains, the grid quantiser and the lerps. QUAD pays the same kind. The LFO and gain ramps are per block.
- **Fallbacks if a TYPE reads over QUAD**, in this order:
  - BURST: 10 taps instead of 12 (−4), with DENSITY 5 → 10;
  - LOOP: 2 output taps per side instead of 3 (−4);
  - BLOOM: 8 loop APs instead of 10 (−6).
  - Each is a constant in the engine and changes a golden, so the owner hears it at A/B.
- **`FxBus::COST` does not change while every TYPE ≤ QUAD.** The bench's BUS row runs the costliest TYPE's worst settings, chosen by name in `bench.rs` from the REV rows, so the bill stays honest if that ever flips.

### Bench rows

- The REVERB row becomes **REV QUAD** (today's `worst_reverb`).
- New rows, each at its worst:
  - **REV ROOM** and **REV HALL**: SIZE alternating between 1 and 30/31 every block, so the glide always runs; DECAY 1; WOBBLE 1, RATE 1; GRIT 1; MIX 0.5.
  - **REV GATE** and **REV RVRS**: LENGTH alternating; DENSITY 1; TAIL or REGEN 1; WOBBLE 1.
  - **REV BLOOM**: SWELL alternating; DECAY 1; WOBBLE 1.
  - **REV ECHO**: TIME alternating; WOBBLE 1; RATE 1; DIFFUSE 1; PATTERN CLOUD.
  - **REV SWITCH**: TYPE stepping QUAD → HALL → GATE → … every 40 blocks, so FADE OUT, CLEAR and FADE IN all land in the window.
- `FX_ROWS` grows by 7, from 6 to 13 without `master-tape`. The bench screen goes to five lines of three, and the console's `bench` (ADR 0068) prints every row.
- The plan records every row's reading, the build and the date, as ADR 0031 did.

## Flash

- **Method.** Release `text`, fat LTO and one codegen unit, on a `git archive` copy: ADR 0083's method, on the ORBIT branch. Thin LTO moves about ±1 KB between builds of the same code.
- **Gate.** Each step's plan task records its delta. A TYPE over 3 KB is cut back (its own code shared or moved out of line) before the next TYPE starts.

| Step | Adds | Estimate |
|---|---|---|
| Scaffold: dispatcher, switch, arena split, interpolated read, allpass, tone, LFO, panels, MORE page, TAIL glyph, hints | shared | 3–4 KB |
| ROOM | `LoopEngine<ROOM>`, panel row, layout | 1.5–2 KB |
| HALL | `LoopEngine<HALL>`, panel row, layout | 1.5–2 KB |
| GATE + REVERSE | one `BurstEngine`, two panel rows, tap tables | 2–2.5 KB (1–1.3 KB per TYPE) |
| BLOOM | `BloomEngine`, panel row | 1–1.5 KB |
| ECHO | `EchoEngine`, PATTERN weights, panel row | 1–1.5 KB |
| Tempo sync | division lists, fold | ≈ 0.5 KB |
| **Total** | | **≈ 11–13.5 KB** |

Every TYPE stays inside 1–3 KB. The memory map has 896 KB of flash for the firmware, and ORBIT's builds read about 715 KB.

## Storage

- **Old projects.**
  - A project saved before this work has a Reverb block with ids 1–5 and no id 6. Decode leaves an absent id at its target's value. A load decodes into a fresh `Project` whose reverb is `ReverbParams::default()`, so TYPE is QUAD: **every existing project loads as QUAD**, and sounds as it did.
  - Ids 7–14 also take their spec defaults, and QUAD ignores them.
- **New projects** default to QUAD as well: one rule, and today's sound is the starting point.
- **A future TYPE code on this firmware**, a file from a newer build, is an unknown code: the value keeps the target's, QUAD (`set_enum_code` returns false). It loads safely.
- **`FxParams`** grows by about 28 B: six more f32 (TONE, DIFFUSE, PREDELAY, SHAPE, WOBBLE, RATE) and three enum bytes. It sits in `Performance`, in the triple buffer (ADR 0021); `memory_budget_test` re-pins it.

## Tests

The usual battery, per TYPE, on the host first (owner's rule: QA before UAT).

### Refactor lock (QUAD)

- `fx_golden_test`'s reverb cases, `reverb_test`, `gain_staging_test` and `fx_bus_test` pass unchanged, bit for bit, with QUAD on the shared arena.
- That proves the arena move and the dispatcher changed nothing.

### Per engine (unit)

- **Layout.** Each TYPE's layout is distinct primes that fit, and is ≤ 23,220 (`const` assert plus test).
- **Mappings.** Each pure mapping hits its table's endpoints and defaults to 1e-3; `exp` and `lin` are monotonic.
- **Gains and floor.** Stage gains < `MAX_GAIN`; RT60's floor `t_min` as QUAD's.
- **Interpolated read.** At fraction 0 it equals the integer read; at 0.5 it is the mean.
- **Glide.**
  - The length lands on its target.
  - No per-sample length step exceeds ⅛.
  - A full SIZE sweep's instantaneous pitch stays within −2.4 / +2.1 semitones (zero crossings of a steady sine through the line).
- **BURST.**
  - Weights are normalised (Σw² constant within 0.1 dB across SHAPE, CURVE and DENSITY).
  - DENSITY fades taps in continuously: no weight jumps more than 1/64 per 1/128 of the knob.
- **ECHO.** PATTERN's weights equal the table; a PATTERN change takes 20 ms with no step.
- **Sync.**
  - `fold` halves and doubles to fit across 30–300 BPM, and every result is in its TYPE's range.
  - DECAY in beats clamps.
  - Each division list's 120 BPM values are inside the ms range.

### Sound (render)

- **Stable and silent.** At GRIT 0 and 1, every TYPE's energy never grows after the send stops, and the return reaches exact zero within the TYPE's DECAY maximum × 3. Truncation's job, as `falls_silent_within` and `no_limit_cycle_at_grit_*` do for QUAD.
- **RT60 tracks DECAY.** Measured on a 0.5 s noise burst (Schroeder EDC, as `rt60_follows_time`), at p = 0.25, 0.5 and 0.75, within ±35 % of the target at GRIT 0. The same tolerance and method as ADR 0028.
- **The spike's windows (±30 %).**
  - ROOM at defaults: RT60 0.55 s and onset ≤ 34 ms.
  - HALL at defaults: L/R correlation |ρ| ≤ 0.2.
  - GATE at 177 ms: −20 dB end between 185 and 660 ms, and a cliff, the energy in the 50 ms after the end at least 20 dB under the plateau's at TAIL 0.
  - REVERSE: envelope peak within ±15 % of LENGTH, and |ρ| between 0.3 and 0.75.
  - BLOOM at defaults: onset 140–300 ms, peak 450–750 ms.
  - ECHO SINGLE: the first repeat at TIME ±1 sample at 24 kHz.
- **Band-limited**, as `the_return_is_band_limited`.
- **Loudness parity.** Each TYPE's default return RMS is within ±1 dB of QUAD's default on the same noise burst at MIX 0.5. This sets `WET[type]`.
- **Wobble.** At WOBBLE 0 the output equals the output with the LFO frozen. At WOBBLE 1 the spectral peaks of a sustained tail move, as measured by a short-time spectrum.
- **Sanitising.** NaN and out-of-range knobs render finite and recover, as QUAD's do.

### Switch

- **All 42 ordered pairs of TYPEs**, mid-tail at MIX 1, through `FxBus`:
  - **no click**: the return's max second difference in the switch window ≤ its steady neighbours', `click_free_test`'s measure;
  - the return is **exactly 0** in the CLEAR blocks;
  - **no stale echo**: from the first FADE IN block, the new engine's pre-gain output is bit-identical to a fresh engine's fed the same send;
  - a second switch during FADE IN starts its fade from the current gain.
- **A project swap** that changes TYPE goes the same path (`fx_swap_test`).

### Sweeps

- `param_sweep_test` becomes TYPE-aware: under every TYPE, it sweeps every present knob of that TYPE (absent ones are skipped).
- The verdicts are today's: finite, under the ceiling, no DC, no clicks the knob doesn't cause, not silent unless it silences, and never billed over budget.
- `sweep_thorough` adds every TYPE × every SYNC × 30, 120 and 300 BPM.
- Allowed behaviour, each listed with its reason as the sweep's rules require:
  - TYPE's switch dips the return by design;
  - TAIL 0 and REGEN 0 are allowed short tails.

### Goldens

New `fx_golden_test` cases, FNV-hashed, with the same burst and length as today's:
- `rev_room`, `rev_hall`, `rev_hall_plate`, `rev_gate`, `rev_gate_flat_tail0`, `rev_reverse`, `rev_reverse_regen`, `rev_bloom`, `rev_echo_single`, `rev_echo_cloud`;
- with sync after the tempo step: `rev_gate_sync_8th_120`.

They lock refactors, not quality (ADR 0011). Ear tuning re-records them deliberately.

### UI

- Panels: every TYPE's main and MORE labels fit their cells, and every unit string fits.
- Hints: every hint fits the band; `rev_hint` returns the table's hint in each condition.
- TAIL: `tail_shape` is pure, and changes under every knob of its TYPE. Composites draw set values only (ADR 0067's existing test, extended). The box-clear and status tests cover TAIL.
- Screen goldens (`screen_golden_test`) for REV and REV › MORE under QUAD, HALL and ECHO.

### A/B against the Midiverb VST, for the owner

- **Stimuli.** I render Chimera's side on the host through the real bus (`FxBus`, MIX 1, dry off), as 48 kHz WAVs:
  - an impulse;
  - a 0.5 s pink-noise burst;
  - a dry snare;
  - a short saw chord.
  The stimuli files and their seed are committed under `tools/ab/` as a generator script, never as audio.
- **The owner's side.** The owner runs the same stimuli through the VST on the paired programs (names, as the unit shows them):

| Chimera setting | Midiverb II program |
|---|---|
| ROOM defaults | Medium Bright (about 0.5 s) |
| ROOM, TONE 0.8 | Medium Dark |
| HALL defaults | Large Warm 2.2 |
| HALL, DECAY 0.85 | Xlarge Warm 5 s |
| HALL plate corner | none (MV2 had no plate): by ear against a plate the owner picks |
| GATE defaults | Slow Gate 200 |
| GATE, TAIL 0, SHAPE 0 | Fast Gate 150 |
| REVERSE defaults | Reverse 300 |
| REVERSE, REGEN 0.7 | Reverse Regen 2 s |
| BLOOM defaults | Bloom 1 |
| ECHO PAN | 3 Tap Pan |
| ECHO CLOUD | Multitap |

- **The comparison.** I measure both sides with the same metrics (RT60 by T20, onset, envelope peak, −20 dB end, 4 kHz vs 500 Hz tilt, L/R correlation) and hand the owner a table plus the WAV pairs. The owner judges by ear (UAT), and ear changes go back into the constants and goldens.

## Build order

Each step is a mergeable slice with its tests, goldens, bench row and flash delta. The order puts the risky shared parts first, then the engines from the simplest proof to the costliest.

1. **Scaffold + QUAD.**
   - `ReverbArena` out of `Ring` (QUAD bit-identical).
   - `ReverbType` with QUAD only selectable, plus `ReverbParams`' new slots and disk codes.
   - The dispatcher (`dsp::reverb_bus::Reverb`, replacing `RingReverb` in `FxBus`) with the switch state machine, tested QUAD → QUAD by a forced restart.
   - The interpolated read, allpass, tone, LFO and glide helpers, unit-tested alone.
   - `reverb_panel.rs`, REV › MORE, the hint line, `CompositeId::Reverb`, and the REV QUAD and REV SWITCH bench rows.
2. **ROOM.** It proves `LoopEngine`, the glide and the wobble. It adds the TAIL glyph, the first A/B pair and the REV ROOM row.
3. **HALL** on the same engine: the plate corner and its hint, and the REV HALL row.
4. **GATE, then REVERSE** on `BurstEngine`: the tap tables, the curves, TAIL and REGEN, and the REV GATE and REV RVRS rows.
5. **BLOOM**: the REV BLOOM row.
6. **ECHO**: PATTERN and the REV ECHO row.
7. **Tempo sync**, once ORBIT is on `main`: `Tempo` into `FxBus`, SYNC, the division lists, the fold and its hint.
8. **A/B and UAT**: the owner's listening pass, the ear-tuned constants and goldens, the final bench readings and flash totals, then ADR 0085 to Accepted.

GATE's tempo-synced LENGTH is the owner's "big one". Step 7 lands right after ORBIT merges, even if steps 5–6 are still open, so the order of 5, 6 and 7 can swap to suit.

## Review bar

Each plan task's review grades quality, efficiency, performance and principled-ness (the owner's rubric) and needs ≥ 8 on each. This design aims at that:
- **Quality.**
  - One parameter block with typed choices.
  - Pure mappings shared by the DSP and the UI.
  - QUAD locked by goldens.
  - Every TYPE has the same test battery.
- **Efficiency.**
  - One code path per engine.
  - Monomorphs only where the graph differs.
  - One arena.
  - No new gesture or page type.
- **Performance.**
  - Every TYPE ≤ QUAD by design rule and by bench row.
  - Steady cost = worst cost, so the bill is exact.
- **Principled-ness.**
  - The provenance rule is checked per task.
  - Families are graphs, so PLATE is a corner, not a TYPE.
  - The switch is provably stale-free.
  - Absent parameters are absent, not inert.

## Risks

- **BURST density with 12 taps.** Pre-diffusion should make 12 reads sound like the MV2's 47–90. If the A/B says grainy:
  - raise the pre-diffuser's g first (free);
  - the next step is 14 taps, +4 accesses, still ≤ 48.
- **The glide's pitch smear.** It may be too obvious on HALL and BLOOM. Slower τ and slew constants are free; only the time to reach a new SIZE grows.
- **ECHO's headroom** is 838 samples, less its guard slots. The wobble's ±5 ms is inside it. A longer TIME is not.
- **#142** (quiet sends lose their tail to i16 truncation) applies to every TYPE alike. It is fixed or not for all of them at once, outside this spec.
- **Flash.** ≈ 11–13.5 KB total. Each step's delta is gated (§ Flash).
