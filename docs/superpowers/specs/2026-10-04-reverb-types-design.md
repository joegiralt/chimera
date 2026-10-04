# Reverb TYPEs: QUAD plus ROOM, HALL, GATE, REVERSE, BLOOM and ECHO

Status: Approved by owner pending read; reviewed 8/8/8/8 (2026-10-04). ADR [0085](../../adr/0085-the-master-reverb-picks-a-type.md) (Proposed) records the decision. The owner's answers of 2026-10-04 are folded in: modulation is deferred to private issue #345, and QUAD keeps its name. Issue: private issue #343. It is queued after ORBIT ships (owner, 2026-10-03).

## Intent

The owner, 2026-10-03:
- The master reverb gets a TYPE setting, and only the selected model runs.
- The TYPEs are families, not presets.
- The method is to study how the Midiverb II programs work and write our own Rust. Then extend past the hardware with an LFO wobble, mod destinations and tempo-synced times.
- No TYPE may cost more CPU than today's reverb. Amended the same day by the owner's ruling "sound first" (2026-10-04): QUAD's cost becomes the target, and the hard limit is the whole system's worst case inside the audio reserve.
- All TYPEs share today's delay memory, and nothing grows.

This spec turns that into three engines plus an echo, six new TYPEs beside today's ring, one shared parameter block, and a switch that never clicks.

## Rulings this spec follows

| Ruling (owner, 2026-10-03) | Where it lands |
|---|---|
| A TYPE setting. Only the selected model runs. | § Switching TYPE: one engine at a time, the fade included. |
| TYPEs are families: QUAD, ROOM, HALL/PLATE, GATE, REVERSE, BLOOM, ECHO. The engines are LOOP, BURST and BLOOM, plus ECHO. | § Engines; § PLATE is HALL's corner, not a TYPE. |
| Study github.com/thement/midiverb_emulator (GPL-3.0) as documentation only. Never copy its code or transcribe its ROM coefficient tables. Commit 7119f7ad is not revived. | § Provenance. |
| Extend past the hardware: a loop LFO wobble, mod destinations, and tempo-synced PRE-DLY, GATE LENGTH, ECHO times and optionally DECAY. Mod destinations deferred to #345 (2026-10-04). | § Wobble; § Modulation (deferred); § Tempo sync. |
| "Sound first" (owner, 2026-10-04). QUAD's 485 cycles per output sample is the **target**. The **hard limit** is the measured system worst case inside the 30 % reserve. | § CPU budget; § Build order, step 0. |
| One 23,220-sample i16 arena (46,440 B), and nothing grows. About 1–3 KB of flash per TYPE. | § Arena; § Flash. |
| SIZE glides through interpolated read pointers, not QUAD's double-read crossfade. QUAD stays exactly as it is, crossfade included (2026-10-04). | § Glide; § QUAD. |
| A switch fades the old tail out quickly while the arena clears, with no click and no stale echo. Parameter changes are lerped, never snapped. | § Switching TYPE; § Smoothing. |
| No users yet: old projects load safely, as QUAD. | § Storage. |
| User-facing names are sound names. Any MIX or hold gesture gets a contextual focus-band hint. | § UI; the reverb adds no gesture, so no hint (ADR 0086). |
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
- **One knob reaches it.** HALL's SIZE has a **plate zone** at its bottom, 0 → 0.15, with w = 1 − SIZE/0.15 (1 at SIZE 0):
  - the engine reads DIFFUSE as `DIFFUSE + w·(1 − DIFFUSE)`, TONE as `TONE·(1 − w)` and PRE-DLY as `PRE-DLY·(1 − w)`;
  - so turning SIZE to 0 gives the plate's diffusion, brightness and zero pre-delay, whatever those three knobs say, and above 0.15 they are untouched;
  - the stored knobs never change. The SIZE cell's value text reads `PLATE` with its ms in the zone, and TAIL draws the effective values.
  - Cost: three lerps once a block, nothing per sample; about 150 B of flash.
- The **plate corner** is a named, tested setting: HALL with SIZE 0, DECAY 0.35, TONE 0, DIFFUSE 1, DAMP 0.1, PRE-DLY 0, WOBBLE 0.5 and RATE 0.6. It has its own golden and its own A/B render.

Reversing this later is cheap. A PLATE TYPE would be a new disk code (7) mapped to the HALL engine with its own panel row and ranges: about 200 B of flash and no DSP. Nothing in this design blocks it.

## Engines

### Shared machinery

- **Rate.** Every engine runs at half rate, 24 kHz on the chip, behind today's half-band decimator and two interpolators (ADR 0028). They are shared and unchanged. The band limit stays the dark top every TYPE shares.
- **Storage.**
  - Every line is i16, full scale ±2.0, in the one arena (§ Arena).
  - GRIT's grid applies to every write in every TYPE: Δ = 2^(6·GRIT) LSBs (ADR 0028).
  - ADR 0028's Q3 rule carries over: allpass state writes truncate toward zero, and delay and damper writes round.
  - **A loop with no allpass in it truncates its feedback write**: ECHO's line, and BURST's line (REGEN). Rounding there sustains a limit cycle: round(0.891·Δ) = Δ, and any gain above ½ holds ±Δ (as hum or DC, up to −48 dB at GRIT 1). Truncation toward zero always decays.
  - FREEZE is not in this spec. No loop gain reaches 1.
- **Two kinds of read.**
  - A **fixed read** sits at an integer offset that changes only when a block starts. These are QUAD's reads: they go through `ring.rs`'s wrap-free segments, one integer load each, with no per-sample index work.
  - A **moving read** is one whose length changes inside a block: a gliding length, a wobble, or a moving tap. It is a linear pair, `x0 + f·(x1 − x0)`. Its index is computed per sample (integer part, fraction, and a branch-free wrap of both slots), so it stays out of the segment bound. That is about 7 instructions more than two fixed reads (§ CPU budget).
  - Each engine names its moving reads, and keeps them few. Output taps never move: they sit at fixed offsets behind their line's write point, inside its *shortest* length, so SIZE never moves them.
- **Reads never reach the write head.** Every read is at least 2 samples behind its line's write point (`d ≥ 2`, a clamp after each length is computed). The one exception is BURST's tap 0, which is the input value itself, taken from a register. Wobble depth is capped at half the shortest length it moves.
- **Allpass.** `v = x − g·z` is written (truncated onto the grid), and `y = z + g·v`. Same form as QUAD's.
- **Tone filter.** One one-pole lowpass per engine on the input, after the decimator: `y += a·(x − y)`, with `a = 1 − exp(−2π·fc / 24 000)`. TONE sets fc, bright (0) to dark (1). This is where MV2's Bright, Warm and Dark live.
- **Dampers.** One-pole lowpasses inside loops, written onto the grid with rounding, like QUAD's DAMP. In ROOM, HALL, BLOOM and GATE's TAIL each sits in a loop that also holds a truncating allpass. ECHO's TONE damper and REGEN's smoothing do not, and that is harmless: their loops' feedback writes truncate (the Storage bullet above), so those loops still decay to zero.
- **Decay.**
  - Each loop stage's gain is `g_k = 10^(−3·L_k / (RT60·fs))`, where L_k is the stage's share of the round trip (QUAD's `stage_gains`).
  - The new engines cap a stage at **0.985**; QUAD keeps its 0.97. The truncating writes keep every loop decaying, and 0.985⁴ reaches HALL's 10 s at its 120 ms loop (§ Each TYPE's knobs).
  - RT60 has a floor at `t_min`, the longest allpass's own ring, as in QUAD.
  - Gains ramp across each block from the last block's value (QUAD's `RingBlock`).
- **The wobble LFO.**
  - One per engine: a `u32` phase and a branch-free parabolic sine, the same idiom as the chorus's LFO (ADR 0031, `335cd3a`).
  - In ROOM, HALL, BLOOM, GATE and REVERSE, the moved reads run 90° apart, so the left and right tails move differently.
  - It is computed at the block's ends and interpolated linearly across the block's 32 ring samples. At 7 Hz the error is under 5 × 10⁻⁴ of the depth.
- **Input gain.** QUAD injects the send at ½ (unchanged). Each new engine scales its send by `IN[type] = 2 / G[type]`. `G` is the worst-case peak gain from the send to any stored line at the TYPE's maximum feedback, so a full-scale (0 dBFS) send peaks inside the ±2.0 full scale.
  - Estimates, each pinned by a `const fn` and the hot-send test (§ Tests):

| TYPE | Worst-case G | IN |
|---|---|---|
| ECHO | 1/(1 − 0.891) = 9.2: the coherent comb, +19 dB | ≈ 0.22 |
| GATE, REVERSE | REGEN's 1/(1 − 0.708) = 3.4 (+10.7 dB) × the burst sum's Σ\|w\| 3.46 (+10.8 dB) ≈ 11.8 | ≈ 0.17 |
| BLOOM | 1/(1 − 0.866) ≈ 7.5 (SWELL 0, DECAY 12 s) | ≈ 0.27 |
| ROOM, HALL | ≤ 4 (diffused loops, gains spread over stages) | ½ |

  - A smaller IN costs resolution: GRIT's grid and #142's truncation bite a little earlier. `WET[type]` puts the level back on the way out.
- **Output.** The return is ×`WET[type]` × MIX, eased as today. Each TYPE's `WET` is set so its default return's RMS matches QUAD's default within ±1 dB. A test pins this (§ Tests); QUAD's `WET_GAIN` 4.54 is unchanged.

### Glide

New TYPEs never crossfade; QUAD always does (owner ruling, 2026-10-04: QUAD stays exactly as it is, with a clean, pitch-free crossfade; the new TYPEs' slight pitch bend is part of their vintage character).

- **Targets.** Each moving length has a target in f32 samples: SIZE's mapping, or the tempo-synced value.
- **Following.**
  1. The read length follows its target through a one-pole, τ = 100 ms, once a block.
  2. It is slew-limited to at most **⅛ sample per sample**, which bounds the Doppler shift to −2.3 / +2.0 semitones.
  3. It is interpolated linearly across the block.
- **What it sounds like.** A small turn gives a slight bend. A full sweep of HALL's SIZE (120 → 600 ms, 11,520 samples) takes about 3.8 s at the slew limit, with the old tail sliding in pitch.
- **BURST taps follow a different rule** (§ BURST engine).
- **Tuning.** The constants (τ and the slew limit) are starting values, to be tuned by ear at A/B. Tests pin the invariants: continuity, the pitch bound, and that each length lands on its target.

### LOOP engine: ROOM and HALL

```
send → TONE → PRE-DLY → diffuser (4 AP, fixed) ─┬─► loop A: [delay → AP] → [delay → AP] → DAMP ─┐
                                                 │                                                ├─ rotation g·R(θ) ─► back into A and B
                                                 └─► loop B (ROOM only): same shape, own lengths ─┘
outputs: 3 fixed taps per side, from different stage delays, signs ±
```

- **Shape.** The engine is one generic, `LoopEngine<const SHAPE: LoopShape>`, monomorphised twice: ROOM is 2 loops × 2 stages, HALL is 1 loop × 4 stages.
- **SIZE** scales only the stage *delays*, by k; they are the moving reads, along with PRE-DLY. The stage allpasses and the diffuser are fixed reads.
- **Coupling in ROOM.**
  - The two loops' outputs (a, b) re-enter through an energy-preserving rotation: A gets `c·a + s·b`, and B gets `−s·a + c·b`. Here c = cos θ, s = sin θ and c² + s² = 1; θ = 0.25 rad, a light coupling.
  - The stage gains carry the decay, so the round trip's gain is exactly g and RT60 tracks DECAY. An additive ±¼ cross-feed would not be gain-preserving: |λ| = √(g² + 1/16), so it would ring about 2.5× long at DECAY 0.2 s.
  - The send enters A at +½ and B at −½, as QUAD injects into S1 and S3.
- **Dampers.** ROOM has one per loop. HALL has two, after stages 2 and 4.
- **Wobble.** It moves the stage delays that are already moving reads: in ROOM A2 and B1, in HALL stages 1 and 3, in quadrature.

Starting lengths in ms at SIZE 1. Each becomes the nearest distinct prime in samples at 24 kHz, as `ring.rs` builds QUAD's:

| | Diffuser APs | Stage 1 delay / AP | Stage 2 delay / AP | Stage 3 delay / AP | Stage 4 delay / AP | k (SIZE 0 → 1) |
|---|---|---|---|---|---|---|
| ROOM loop A | 2.3, 3.7, 5.9, 8.9 | 59 / 17 | 101 / 23 | | | 0.375 → 1 (round trip 100 → 200 ms) |
| ROOM loop B | (shared) | 73 / 13 | 95 / 19 | | | same k |
| HALL | 1.7, 3.1, 5.3, 8.9 | 113 / 11 | 157 / 17 | 124 / 13 | 146 / 19 | 0.111 → 1 (round trip 120 → 600 ms) |

Output taps are (line, fraction of the line's **maximum** length, sign). They are fixed reads:
- **ROOM:** L = (A1, **0.10 of SIZE 0**, +), (B2, 0.57, −), (A2, 0.83, +). R = (B1, **0.12 of SIZE 0**, +), (A2, 0.53, −), (B2, 0.79, +).
- **HALL:** L = (S1, **0.30 of SIZE 0**, +), (S3, 0.61, −), (S2, 0.83, +). R = (S2, **0.25 of SIZE 0**, +), (S4, 0.49, −), (S1, 0.91, +).
- **The early tap.** Each side's first tap is a fraction of its line's SIZE-0 length, not its maximum: about 2–3 ms in ROOM and about 4 ms in HALL. With every tap at the maximum's fractions, the plate corner's earliest tap would be about 36 ms late. The early tap replaces a maximum-length tap, so the count and the cost do not change.

- **Why the maximum** (the other two taps). A line owns its maximum length's slots, and the shared write head writes every one of them each pass. So a tap anywhere inside the maximum reads written data at any SIZE, at the same cost.
- **What a tap reads.** At SIZE 1 the taps spread across each whole stage. At a small SIZE a tap past the current read point reads the line's input from an earlier pass: still the decaying tail, just older.
- **Why not the minimum.** Fractions of the SIZE-0 length would read only the first 11 % of HALL's 540 ms stages at SIZE 1, and the output could pulse once per pass.
- **The trade.** The early pattern stays put as SIZE moves. The ear judges it at A/B. SIZE-scaled taps would cost 6 more moving pairs, well over the target. **They do not fit**: 6 pairs is about 190–225 cycles per output sample, over the 96 (23) headroom. They would need an owner ruling to give up a voice guarantee (§ CPU budget).

### BURST engine: GATE and REVERSE

```
send → TONE → pre-diffuser (2 AP) → burst line ──► 12 taps over [0, LENGTH], weights w_k, signs s_k ─► Σ ─┬─► decorrelator L (2 AP) → L
                     ▲                                    │                                         └─► decorrelator R (2 AP) → R
                     └── REGEN (REVERSE): tap 11 × g ─────┘          GATE: Σ also feeds a 1-stage TAIL loop (delay → AP → ×g), added back to Σ
```

- **One engine, one code path.** GATE runs with REGEN's gain at 0, and REVERSE with TAIL's gain at 0. Both pay for both paths, so there is only one monomorph.
- **Taps.**
  - 12 taps. Tap k sits at `LENGTH·(k + j_k)/11` for k = 0…11.
  - j_0 = j_11 = 0. The others are a fixed jitter within ±0.3, from a `const fn` LCG seeded in this spec (seed `0x5EED_0343`). The jitter breaks the comb a regular spacing would ring.
  - Signs s_k are a fixed ± pattern from the same LCG.
  - **Tap 0 is the line's input value**, taken from the register before the write: no read.
  - **Tap 11 is also REGEN's feedback read**: one read serves both.
- **How taps follow LENGTH.**
  - A tap glide would make every tap a moving read, about 15 pairs and roughly 330 instructions against QUAD's 239 (§ CPU budget). Instead taps 1–11 are fixed integer reads, and **at most 4 move at a time**.
  - Each block, a scheduler picks the (up to) 4 taps farthest from their targets. Those read as moving pairs, sliding toward their targets. The rest stay where they are.
  - **Speed.** Taps 1–10 move at up to **½ sample per sample** (16 samples a block). **Tap 11 moves at ⅛**, the glide's limit, because it is REGEN's feedback: a faster move would pitch-shift the regenerating tail by −12/+7 semitones on every LENGTH or tempo move. At ⅛ the bound is −2.3/+2.0 semitones, as for the loops.
  - **Leaving the moving set.** A tap leaves only when it sits on an integer slot (fraction 0), so the next block's fixed read is the same sample and nothing steps.
  - Inside a diffused burst, a moving tap's Doppler is smear, not pitch, so the faster slew is safe here. The click test (§ Tests) holds it.
  - Moving the burst 250 → 500 ms (1/8 → 1/4 at 120 BPM) moves the taps 36,000 samples in all, at up to 64 per block: about 0.75 s for taps 1–10. Tap 11 moves 6,000 samples at ⅛ sample per sample, about 2 s; tap 0 and the rest of the burst are already in place.
- **Curves.**
  - GATE: `w_k ∝ 10^(−SHAPE_dB·k/11/20)`, with SHAPE_dB = 18·SHAPE. Flat (a hard cut) to a falling 18 dB (the "dying" slow gate).
  - REVERSE: `w_k ∝ 10^(−CURVE_dB·(11−k)/11/20)`, with CURVE_dB = 12 + 18·CURVE. A rising ramp of 12–30 dB; the MV2 used about 24 in steps, ours is smooth.
  - The weights are normalised so the sum of their squares is constant: loudness holds as SHAPE, CURVE and DENSITY move.
- **DENSITY.**
  - Taps 6–11 fade in one after another across the knob: tap 6 + i weighs `clamp(6·DENSITY − i, 0, 1)` before normalising, so 6 taps at 0 and 12 at 1, never a step.
  - DENSITY also sets the pre-diffuser's g, 0.5 → 0.7.
  - Diffusing *before* the taps is what lets 11 reads do the work of the MV2's 47–90.
- **TAIL (GATE).** The burst sum also feeds one loop stage: a 37 ms delay, then an 11 ms AP. Its output is ×g and added back into the sum. g comes from TAIL's RT60 and is scaled by `min(1, TAIL/0.05)`, so TAIL 0 is exactly the fast gate with no step.
- **REGEN (REVERSE).** Tap 11 is fed back into the burst line's input through a one-pole at about 6 kHz, so each pass darkens. The gain is g = 0.708·REGEN, up to −3 dB per pass. The line write that carries it truncates (§ Shared machinery).
- **Decorrelators.**
  - Two APs per side at g 0.6: L 3.1 and 8.3 ms, R 4.7 and 6.1 ms.
  - Each side's second AP is the one the wobble moves.
  - The taps sum mono first (the MV2 shape), and the decorrelators make the stereo.

### BLOOM engine

```
send → PRE-DLY → (+) → AP1 → … → AP10 → SWELL delay → DAMP(TONE) → ×g ─┐
                  ▲_____________________________________________________┘
L = AP5's output → decorrelator L (2 AP) ;  R = AP10's output → decorrelator R (2 AP)
```

- **The loop** is ten fixed allpasses and one plain delay. The density keeps building for several passes, and that is the swell.
- **Allpass lengths:** 7, 11, 13, 19, 23, 29, 31, 37, 41 and 43 ms, about 254 ms in all. They are fixed reads.
- **SWELL** sets the loop delay, 0 → 446 ms, so the round trip runs about 250 → 700 ms. That delay is the one read that glides. Scaling all ten allpasses instead would cost ten moving pairs, about 282 instructions against 239.
- **Risk: an echo instead of a swell.** At SWELL 1, 446 ms of the 700 ms loop is plain delay. The loop may then repeat audibly once per pass instead of blooming.
  - The A/B checks this explicitly (BLOOM at SWELL 1 against Bloom 1).
  - **Fallback:** split the SWELL delay in two and put one fixed allpass between the halves (17 ms, g 0.5). That adds diffusion inside the long delay for 2 fixed accesses (+10 instructions). It is paid for by wobbling 2 APs instead of 3 (−1 pair, −15 to −20), as the CPU table shows. Alone it adds about 19 cycles per output sample. That fits both headrooms (96 / 23).
- **TONE** is the in-loop damper here (fc 10 kHz → 1.5 kHz). The bloom's darkness lives in the loop, as the spike measured: HF RT60 about 0.9× LF.
- **DIFFUSE** sets the loop APs' g, 0.45 → 0.65.
- **Taps.** The outputs are values already in registers (AP5's and AP10's outputs), not reads, so they cost nothing.
- **Decorrelators:** L 4.1 and 9.7 ms, R 5.3 and 7.9 ms, at g 0.6.
- **Wobble** moves APs 3, 6 and 9, as moving reads: L-phase, quadrature and R-phase.

### ECHO engine

```
send → (+) → line ──► 6 taps at k/6 · TIME (k = 1…6), per-side weights by PATTERN ─► Σ_L, Σ_R → 2 AP per side (DIFFUSE) → L, R
        ▲                                │
        └── tap 6 × REPEATS ← DAMP(TONE) ┘   (feedback write truncates)
```

- **Taps.** Six taps at sixths of TIME, the moving reads. Sixths hold halves and thirds, which is every pattern below. Both sides read the same six taps, so it costs 6 moving pairs and not 12.
- **Wobble.** One LFO phase moves all six taps together; ECHO has no quadrature. The stereo comes from PATTERN's per-side weights.
  - The depth is capped at TIME/12, half the shortest tap: at TIME 20 ms the wobble is at most ±1.7 ms whatever WOBBLE says.
  - The panel's value text shows the depth that plays.
- **PATTERN** sets per-side weights on the same six taps. A change eases the weights over 20 ms, so it crossfades and never snaps.

| PATTERN | Taps (k: L / R weight) | After |
|---|---|---|
| SINGLE | 6: 1 / 1 | MV2 single echo |
| PAIR | 3: ½ / ½; 6: 1 / 1 | MV2 "2 Tap Ambient" (best with DIFFUSE up) |
| PAN | 2: 1 / 0; 4: ½ / ½; 6: 0 / 1 | MV2 "3 Tap Pan" |
| CLOUD | 1…6 falling 6 dB per tap, alternating sides and signs | MV2 "Multitap" |
| RISE | 1…6 rising 6 dB per tap, alternating sides and signs | MV2 "Multitap Reverse Pan" |

- **Feedback** always comes from tap 6 (TIME), through the TONE damper, ×REPEATS. The line write that carries it **truncates**.
- **DIFFUSE** sets the output APs' g from 0 (clean repeats) to 0.7 (smeared). The AP lengths are L 3.7 and 7.9 ms, R 4.3 and 6.7 ms.
- **ECHO is not a second delay.** The per-Part TAPE delay (ADR 0053) is a mono tape echo with MECHANICS and SAT. ECHO is the master space's echo: multitap, stereo, diffused, synced. The owner approved it as the sixth new TYPE.

### QUAD

Owner ruling (2026-10-04): leave QUAD exactly as it is.
- QUAD is today's `dsp::ring`, bit for bit. Its sound and its cost are unchanged.
- Its only change is internal: `Ring` borrows the arena instead of owning `lines`.
- It keeps its 32-step double-read SIZE crossfade, which is clean and pitch-free. That crossfade is also its measured worst case, the target every other TYPE aims at (§ CPU budget).
- Only the new TYPEs glide through interpolated read pointers (§ Glide). Their slight pitch bend is part of their vintage character.
- QUAD shows no MORE knobs, takes no wobble or sync, and its goldens do not move (ADR 0011).

## Arena

- The arena is `ReverbArena { lines: [i16; 23_220] }`, 46,440 B in `FxBus` (AXI, ADR 0014). It is today's `Ring::lines` moved out, unchanged in size.
- Each TYPE has a `const` layout:
  - every line owns `max_len + 2` slots: its longest length, plus the wobble's depth, plus one slot for a pair's second read;
  - all lines rotate with one shared write head, `ring.rs`'s scheme (`OFFSET`, `HEAD`, wrap-free segments for the fixed reads);
  - a `const` assert per TYPE proves its layout fits in 23,220.

| TYPE | Lines at their maximum (samples at 24 kHz) | Total | Headroom |
|---|---|---|---|
| QUAD | 12 lines (`RING_LEN`) | 23,220 | 0 |
| ROOM | PRE 1,200 · diffuser 498 · delays at k = 1 7,872 · stage APs 1,728 · wobble 48 | 11,346 | 11,874 |
| HALL | PRE 3,000 · diffuser 456 · delays at k = 1 12,960 · stage APs 1,440 · wobble 96 | 17,952 | 5,268 |
| GATE, REVERSE | pre-diffuser 274 · burst line 19,200 (REVERSE's 800 ms) · TAIL loop 1,152 · decorrelators 533 · wobble 48 | 21,207 | 2,013 |
| BLOOM | PRE 3,000 · loop APs 6,096 · SWELL delay 10,704 · decorrelators 648 · wobble 144 | 20,592 | 2,628 |
| ECHO | line 21,600 · APs 542 · wobble 240 | 22,382 | 838 |

Each line also takes its 2 guard slots (under 50 per TYPE), inside the headroom shown. The arena and `RING_LEN` do not change.

**State beside the arena.**
- Each engine's small state (dampers, the tone filter, the LFO phase, current lengths, the tap scheduler, gains) lives in one `#[repr(C, u8)] enum EngineState`, sized to the largest variant. It targets ≤ 160 B; a `memory_budget_test` assert pins it.
- `FxBus::init_in_place` zeroes it, as it zeroes everything. Variant 0 is `Quad`, and every field of every variant is an integer or a float. So zero bytes are a valid `Quad`: discriminant 0 with zeroed fields. The `// SAFETY:` comment on the zeroing says exactly that, and a test checks that a zeroed `EngineState` equals `EngineState::Quad(Default::default())`.
- That is the only growth in `FxBus`, and the arena does not grow.

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

That is 14 ids of the 32. Slot 3's static step becomes 1/128, so ECHO TIME and GATE LENGTH move about 3 % and 2 % per tick, not 13 % and 9 %. QUAD's panel keeps its 1/31 tick (§ Pages).

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
| REGEN | 1 | lin (amplitude) | feedback 0 → 0.708 (−3 dB per pass): RT60 up to about 16 s at 800 ms | 0 → none |
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
| WOBBLE | 11 | lin | ±0–5 ms (all taps), capped at TIME/12 | 0 → off |
| RATE | 12 | exp | 0.1–7 Hz | 0.3 → 0.36 Hz |

With a short TIME and WOBBLE up, ECHO is the MV2's flange and chorus family.

**QUAD** (main: SIZE · TIME · DAMP · GRIT; MORE: all absent). Its ids, ranges and laws are today's (ADR 0028). GRIT moves to the main page for QUAD only, because QUAD has no MORE knobs.

### A TYPE change loads that TYPE's starting knobs

The issue's naming ruling (owner, 2026-10-03): "Each TYPE is an engine … plus its tuned settings. The existing knobs … still adjust from that starting point." So:
- Turning TYPE in the UI writes, in one edit, the new TYPE's defaults (the tables above) into slots 1, 2, 3, 7, 8, 9, 10, 11, 12 and 14.
- MIX, GRIT and SYNC are kept: they are the player's level, grain and clock.
- `ReverbParams::with_type(t) -> ReverbParams` is the pure function, and the UI's TYPE edit calls it. `Block::write`, decode and MIDI-CC writes of TYPE only store the TYPE. A project or Sound load therefore keeps its stored knobs.
- **An accidental nudge does not wipe the tweaks.** `UiState` keeps each TYPE's last knobs in RAM: 7 TYPEs × 10 slots as `u16` (the stored 0..1 at 1/65,535), 140 B.
  - Leaving a TYPE stores its knobs there. Coming back restores them instead of the starting point.
  - The memory is never saved to the card: a project stores only the current TYPE's knobs. A project load clears it.
  - 140 B fits `UiState`'s guard (5,120 B, ADR 0082 on the ORBIT branch; `UiState` reads 3,832 B there).
  - `with_type(t, memory)` takes the memory, and falls back to the starting knobs for a TYPE not yet visited.

## Switching TYPE

The audio thread runs one engine. A change of `params.reverb.kind` from the one running starts this sequence (`BLOCK_SIZE` 64, 48 kHz):

| Phase | Blocks | Engine | Return |
|---|---|---|---|
| FADE OUT | 15 (20 ms) | the old TYPE runs on its own lines, with its **latched** knobs | its return × a linear ramp 1 → 0, applied after the interpolators |
| CLEAR | 1 (1.3 ms) | none runs | exactly 0 |
| FADE IN | 15 (20 ms) | the new TYPE, fresh state, knobs primed (no glide from the old TYPE's values) | × a linear ramp 0 → 1 |

- **The latch.** The UI's TYPE edit has already written the new TYPE's starting knobs into the shared slots (`with_type`). So when the switch starts, the dispatcher copies the running engine's last `ReverbParams`, and FADE OUT runs on that copy. The old engine never plays the new TYPE's values.
- **CLEAR.**
  - It zeroes the whole arena in one block: 46,440 B as 64-bit stores.
    - The cost is bound by the bus: at least about 11.6 k cycles.
    - Zeroing 46 KB also evicts the 16 KB D-cache, so the next block refills the voices' working set.
    - The reverb's per-block share is 485 × 64 ≈ 31 k, and no engine runs in that block.
  - It also zeroes the new engine's state and both interpolators' histories, so no sample of the old tail survives anywhere.
  - The decimator's history is the send's own and is kept.
  - The on-chip **REV SWITCH MAX** row measures it (§ Bench rows). If its maximum block breaks the deadline, CLEAR spreads over 4 blocks of a quarter each.
- **The send during CLEAR** is dropped. A note struck in that 1.3 ms gets no reverb.
- **A change during a switch.**
  - The target follows the newest TYPE. In FADE OUT or CLEAR the sequence simply continues.
  - In FADE IN, a new FADE OUT starts from the current ramp value, never from 1, with the latch taken from the knobs that engine is running.
  - Scrolling through TYPEs plays silence, then the last TYPE.
- **Under the switch.**
  - A project swap (ADR 0046) carrying another TYPE uses the same path, latch included.
  - At MIX 0 the switch still runs, silently (ADR 0061).
- **No stale echo.** After the switch the engine is bit for bit a fresh engine fed the same send from the first FADE IN block. A test asserts this (§ Tests).

## Smoothing

- **Continuous knobs.** Every continuous knob eases: moving lengths by the glide (§ Glide), BURST taps by their scheduler, gains, TONE, DAMP, DIFFUSE, SHAPE and the curve weights by a per-block one-pole ramped across the block (τ 50 ms for DECAY, as QUAD's TIME, and 20 ms for the rest). GRIT eases as in QUAD (20 ms), and MIX by `Ease` (ADR 0061).
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
  - ROOM: the stage delays A2 and B1;
  - HALL: stages 1 and 3;
  - BLOOM: APs 3, 6 and 9;
  - GATE and REVERSE: the second decorrelator AP per side;
  - ECHO: all six taps, together.
- It is the TYPEs' only modulation (§ Modulation). Its cost is in § CPU budget.

## Modulation (deferred to #345)

Owner, 2026-10-04: every mod source today lives inside a Part, and nothing exists at the mixer level. A mixer-level LFO or matrix gets its own thought before anything routes into shared FX (private issue #345).
- **This build has no external mod destinations.** Every reverb spec stays `modulatable: false`. The only motion is each TYPE's own wobble (§ Wobble), set by its WOBBLE and RATE knobs.
- **Out of scope, deferred to #345:** Part-matrix routes into the reverb, an envelope follower or DUCK on the send, and MIX+PLUS priming on reverb cells.
- **What #345 will have to change:**
  - **Disk: nothing.** Routes are stored by `ParamAddr` (block and id), and the ids exist.
  - **ADR 0010.** It makes only params that `Voice` reads per block modulatable, and the reverb runs on the bus, outside every `Voice`. Whatever #345 decides must supersede ADR 0010 for bus destinations.
  - **What a destination means.** The slots are semantic: slot 3 is SIZE under ROOM, LENGTH under GATE and TIME under ECHO. A route to slot 3 therefore changes meaning with TYPE. #345 must decide whether a destination is the slot, labelled per TYPE, or a per-TYPE address.
- **No slot is reserved.** Every knob maps through one pure function of its stored value, so an offset can later be added before the mapping without touching storage.

## Tempo sync

- **Source.** The ORBIT clock (ADR 0071, 0072, 0080): the internal BPM, or the leading MIDI clock's average under SYNC EXT. The runner already has it on the audio thread each block.
  - `FxBus::process` takes `Tempo`, a newtype of samples per beat.
  - **Quantised.** The reverb takes the tempo in steps of 0.25 BPM, and moves to a new step only when the clock is 0.15 BPM past the current one. A MIDI clock's average jitters, and every tempo move retargets the synced lengths.
  - Until ORBIT is on `main`, `Tempo` is 120 BPM and SYNC is hidden; the tempo step in § Build order waits for ORBIT.
- **SYNC** (slot 13, MORE page): OFF, TIME or ALL.

| SYNC | HALL, BLOOM | ROOM | GATE, REVERSE | ECHO | QUAD |
|---|---|---|---|---|---|
| OFF | ms | ms | ms | ms | — |
| TIME | PRE-DLY in note values | PRE-DLY in note values | LENGTH in note values | TIME in note values (the taps stay on its sixths) | — |
| ALL | as TIME, and DECAY in beats | not offered: the cell steps OFF ↔ TIME, and a stored ALL reads as TIME | as TIME | as TIME | — |

ROOM has no ALL because 7 of the 10 beat values would clamp to its 1.5 s maximum.

- **Note values.** A synced knob walks a list of divisions, one encoder tick each (the panel gives the cell its step). Each TYPE's list is the divisions that fall inside its ms range at 120 BPM:

| Knob | Divisions (at 120 BPM: ms) |
|---|---|
| ROOM PRE-DLY | 0, 1/128 (16), 1/64T (21), 1/64 (31), 1/32T (42) |
| HALL, BLOOM PRE-DLY | 0, 1/128, 1/64T, 1/64, 1/32T, 1/32 (63), 1/16T (83), 1/32D (94), 1/16 (125) |
| GATE LENGTH | 1/32T (42), 1/32, 1/16T, 1/32D, 1/16, 1/8T (167), 1/16D (188), 1/8 (250), 1/4T (333), 1/8D (375), 1/4 (500) |
| REVERSE LENGTH | 1/8T (167), 1/16D, 1/8, 1/4T, 1/8D, 1/4 (500), 1/2T (667), 1/4D (750) |
| ECHO TIME | 1/64T (21) … 1/4D (750): every division between |
| HALL, BLOOM DECAY (ALL) | 1, 2, 3, 4, 6, 8, 12, 16, 24 or 32 beats |

- **How a division is stored.** In the slot's 0..1 value: for a list of n divisions, division i is stored as `p = i/(n − 1)`, and read as `i = round(p·(n − 1))`.
- **Toggling SYNC.** A UI edit of SYNC converts the slot so the sound stays close. It is a pure function beside `with_type`:
  - OFF → TIME or ALL stores the division nearest the current ms at the current tempo;
  - TIME or ALL → OFF stores the p whose ms equals the division that was playing.

  Decode and CC writes store SYNC alone, and then the slot is simply read the new way. The target changes, and the length glides.
- **Out of range.**
  - A synced time longer than its TYPE's maximum at the current tempo **halves until it fits**; one shorter than the minimum **doubles**. It stays on the grid, so 1/4 at 55 BPM plays 1/8.
  - **The fold has 2 % hysteresis.** A fold state changes only when the unfolded time is 2 % past the limit, inside or out. At 66.7 BPM, ECHO's 1/4 is 900 ms, exactly its maximum; without hysteresis, clock jitter would flip it between 900 and 450 ms and set off 1–2 s glides.
  - A beat DECAY outside its TYPE's seconds range is **clamped**, since a decay has no grid to stay on.
  - The cell's value text shows the division that plays. A fold marks it with 0086's ▾ (halved) or ▴ (doubled), as in `▾1/8`.
  - The arena never grows for a slow tempo. The spike's alternative, about 800 more samples so ECHO fits a quarter at 60 BPM, is ruled out by "nothing grows".
- **A tempo change** moves the targets, and the lengths glide (§ Glide).
- **The fold** (`fold(division, tempo, lo, hi, state) -> (Samples, state)`) is a pure function with its own unit tests.

## UI

### Pages

- **REV** (`EFX`, mixer chain, unchanged id 16): TYPE · knob 1 · knob 2 · knob 3 · knob 4 · MIX. Cells 2–5 come from the TYPE's panel. MIX stays last on every TYPE, so the level is always in the same place.
- **REV › MORE** is a new sub-page, short `MORE`, `ChainBlock::with_subs` as DLY › CHAR. Its cells are PRE-DLY · DAMP, SHAPE, CURVE or PATTERN · WOBBLE · RATE · SYNC · GRIT, by panel. Under QUAD every MORE cell is absent.
- **How the cells look.**
  - An absent cell draws the dash, a dimmed one the dim look; `renderer::look` decides, as for the filter (ADR 0037).
  - A cell's label, its value text and its **encoder step** come from the panel:
    - units such as `141ms`, `2.5s`, `6.6k`, `0.30Hz`, `-9dB` and `1/8`, from the same pure mappings the DSP uses;
    - slot 3's static step is 1/128, and QUAD's panel overrides it to 1/31, so QUAD's SIZE still moves one table step per tick;
    - a synced knob steps one division per tick.
  - **MIDI MAP** (the per-Part CC map) shows the current TYPE's panel label for a reverb slot: a CC on slot 3 reads SIZE under ROOM and LENGTH under GATE. It never shows the static spec's label.
  - **Short forms**, ≤ 6 characters, for cells whose label outruns its bar (ADR 0053's MECHANICS → MECH rule): PRE-DLY → `PRE`, DENSITY → `DENS`, REPEATS → `REPEAT`, PATTERN → `PATT`, DIFFUSE → `DIFF`. The focus band shows the full word.
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
  - The focused knob's part of the shape is drawn in the accent.
  - The x axis spans the TYPE's own range (ROOM 1.5 s, HALL 10 s, BLOOM 12 s, GATE 1.4 s, REVERSE 2 s, ECHO 6 × TIME), so a turn always moves the shape.
- **Cost.** One polyline and a dot per frame, inside the box rules ADR 0067 set: only its box is cleared and flushed while it animates.

### Hints

Hints follow ADR **0086** ("Every gesture shows a contextual hint in the focus band"). 0086 is Proposed, on the ORBIT branch, and not yet on `main`, so this section depends on it.
- **0086's model.** One `ui::hint` component draws static `Gesture`s at baseline 115 under the focus band's value. Each user can turn hints off. A test holds every hint within 216 px. A hint exists for a *gesture* on the touched cell, and is never an always-on line.
- **The reverb cells carry no gesture.** Every new control is a knob, and navigation stays the D-pad (ADR 0081). Modulation is deferred (§ Modulation), so MIX+PLUS priming does not reach these cells. So under 0086 no reverb cell has a hint, and `View::hint` returns none for them.
- **What would have been informational text goes elsewhere.**
  - A folded synced time shows in the value text, with 0086's ▾/▴ mark (§ Tempo sync).
  - The plate corner is one knob away (HALL's SIZE plate zone), has a golden, and gets no hint.
- **If 0086 changes or never lands,** this spec still needs no hint: there is no reverb gesture to explain. A later gesture on a reverb cell (#345's priming) brings its static `Gesture` with it.

## CPU budget

### Target and hard limit ("sound first")

The owner, 2026-10-04: **"sound first."**

- **The target** is QUAD's worst case: 485 cycles per output sample on the REVERB row, and 239 instructions per ring sample on the host (ADR 0031, 2026-09-27; re-measured at step 0). Every TYPE is designed to it, and the table below shows they all meet it by estimate.
- **The hard limit is the cost model's headroom.**
  - **The budget.** The audio thread may use **70 % of the CPU**: `AUDIO_BUDGET_PERCENT` = 70 in `chimera-core/src/hw.rs` ("30% is left for UI, MIDI and interrupt overhead"). `SampleBudget::for_cpu` builds it from the real clock (ADR 0020): **7,000 cycles per sample on rev V** (480 MHz) and **5,833 on rev Y** (400 MHz).
  - **Headroom** is how far `FxBus::COST` can rise before any voice-count assertion in `cost_test` fails. It is the **minimum**, over every one of those assertions, of `budget − FxBus::COST − n × voice`, on both chip revisions and both builds (default and `master-tape`). From the code on 2026-10-04 (voice = `CHAIN_COST` + `ModRouting::BASE` 47 + the engine's cost, at LP24):

| Assertion in `cost_test` | Voice | Rev V (default / tape) | Rev Y (default / tape) |
|---|---|---|---|
| A16 ∪ A17 (`the_costliest_patch_gets_six_voices_on_rev_v`) | 909 | 6 voices: 366 / 56 | 5 / 4: 108 / 707 |
| A16 ∪ A17 + FOLD | 954 | 6 / 5: **96** / 740 | 4 / 4: 837 / 527 |
| A16 ∪ A17 + FOLD + DRIVE | 1,011 | 5 / 5: 765 / 455 | 4 / 4: 609 / 299 |
| TX factory Sounds (`every_factory_sound…`), up to | 712 | 8 / 7: 124 / 526 | ≥ 5: 1,093 / 783 |
| MORPH KEYS | 864 | 6 / 6: 636 / 326 | ≥ 5: 333 / **23** |
| MORPH PAD | 853 | 6 / 6: 702 / 392 | ≥ 5: 388 / 78 |

  The headroom is therefore **96 cycles per sample in the default build** (FOLD on rev V: 5,820 − 6 × 954) and **23 with `master-tape`** (MORPH KEYS on rev Y: 4,343 − 5 × 864). The factory rows use the TX Sounds' highest cost, 712; the derived test below uses every Sound's real cost.
  - **The gate: ΔCOST ≤ headroom**, where ΔCOST is how far the costliest TYPE's BUS row reads above QUAD's on the same build.
  - **The gate is a test, so it cannot go stale.** A `reverb_headroom` test in `cost_test` computes the headroom from `cost_test`'s own constants and functions (`voices_at`, the factory Sounds' `Voice::cost`, `FOLD_COST`, `DRIVE_COST`, `SampleBudget` for both revisions). It asserts `FxBus::COST − QUAD_BUS ≤ headroom`, where `QUAD_BUS` is the recorded BUS reading with QUAD. Raising `FxBus::COST` for a dearer TYPE then fails that test, and the existing voice-count tests, at once.
  - **"Sound first" has little room.** 96 cycles (23 with the tape) is about one moving pair per ring sample in each case.
    - A TYPE costing more than QUAD by more than that needs either a cheaper design, or an owner ruling that gives up a voice guarantee in `cost_test` (for example FOLD's sixth voice on rev V).
    - The "allowed if the A/B asks" items each say below whether they fit.
  - **The stress run is a second, real gate.** On the unit, with the TYPE under test, it runs:
    - six admitted voices of A16 ∪ A17;
    - every FX at its bench worst;
    - ORBIT running all five rings;
    - the UI editing, with an animated glyph in focus.

    It must show OVER 0 and DROPS 0 and a peak block load under 100 %. The bill can pass while the chip fails: ADR 0040's run peaked at 87 % on cache effects the bill does not model. So the run is not a formality.
  - **Host instruction counts** (ADR 0031's trace) are design estimates only, never a gate. ADR 0013 rejects emulation for budgets.
- **A TYPE may exceed the target** when the A/B shows its cheaper design sounds wrong, provided its ΔCOST fits the headroom (96 / 23) and the stress run passes.
- **The QUAD regression check stays.** After step 1a, REV QUAD must read ≤ step 0's baseline + 1 %. QUAD's cost is unchanged by ruling, and the dispatcher must not hide a regression.
- **Why re-measure.** Since 2026-09-27 the bench subtracts the MIX row, and MIX now includes the limiter.

### The model, with moving reads costed

QUAD measured 158 instructions fixed and 239 crossfading, for 30 and 48 accesses: about **5.0 instructions per fixed access**, everything around it included.
- **A fixed access** is QUAD's kind: one load or store at a segment offset.
- **A moving pair** costs two loads plus the per-sample index work: integer part, fraction, the wrap of both slots, and the lerp.
  - That work is estimated at **+7** instructions (this spec's estimate) to **+10** (the review's), so 17 to 20 instructions a pair. Step 0 measures it.
  - ADR 0031's history shows why it matters: the ring's per-tap wrapping read 688 instructions, against 274 once segments removed it.
- **On the chip,** cycles per output sample ≈ 40 + 1.86 × instructions per ring sample (QUAD: 40 + 1.86 × 239 = 485).

| TYPE | Fixed accesses (writes + reads) | Moving pairs | Instructions, pair +7 | Instructions, pair +10 | Cycles, +7 / +10 |
|---|---|---|---|---|---|
| QUAD | 30 steady / 48 crossfading | 0 | 158 / **239 measured** | same | 320 / **485 measured** |
| ROOM | 13 + 14 (taps fixed) | 5: PRE, 4 stage delays (2 wobbled) | 224 | **239** (at the limit) | 457 / 485 |
| HALL | 13 + 14 (taps fixed) | 5: PRE, 4 stage delays (2 wobbled) | 220 | 235 | 449 / 477 |
| GATE, REVERSE | 9 + 13 (7 fixed taps; tap 0 the register; REGEN shares tap 11) | 6: 4 scheduled taps, 2 wobbled decorrelator APs | 212 | 230 | 434 / 468 |
| BLOOM | 16 + 11 (7 fixed loop APs) | 5: PRE, SWELL delay, 3 wobbled APs | 220 | 235 | 449 / 477 |
| ECHO | 5 + 4 | 6: the taps | 147 | 165 | 313 / 347 |

- **What a naive design cost.** GATE with all taps gliding was about 15 pairs, ≈ 330 instructions; BLOOM with all ten APs scaled was ≈ 282. That is why taps are fixed or scheduled, and SWELL moves one delay.
- **ROOM, then HALL and BLOOM, are the closest to the target at +10.** Step 0 benches their worst-case kernels, not only BURST's.
- **Fallbacks are a last resort**, not the plan ("sound first").
  - A fallback is taken only in two cases: the system worst case breaches the reserve with the full design, or the A/B shows the cheaper design sounds the same.
  - A TYPE over the 485 target alone is no reason to take one.
  - Each fallback is a concrete design that drops moving pairs. Each is a constant in the engine and changes a golden, so the owner hears it at A/B.
  - The saving is a pair less 5 for each fixed read that replaces it: 12 at +7, 15 at +10.

| TYPE | Fallback design | Saving (+7 / +10) | Result at +10 |
|---|---|---|---|
| ROOM | **SIZE moves the stage-2 delays only** (A2, B2). A1 and B1 become fixed; A2 spans 1–101 ms and B2 0–95 ms, so the round trip still runs about 100 (105 for B) → 200 ms. The wobble moves A2 and B2. PRE-DLY still glides. | −2 pairs + 2 fixed: 24 / 30 | 209 |
| HALL | **SIZE moves stages 2 and 4 only.** S1 and S3 are fixed at 13 and 14 ms; S2 + S4 span 33 → 513 ms, so the round trip is still 120 → 600 ms. The wobble moves S2 and S4. PRE-DLY still glides. | 24 / 30 | 205 |
| GATE, REVERSE | **3 scheduled taps, not 4** (tap 11 still among them when moving). The 250 → 500 ms move takes about 1 s for taps 1–10. | 12 / 15 | 215 |
| BLOOM | **Wobble 2 APs, not 3** (APs 3 and 9). If the A/B also wants the split SWELL delay (§ BLOOM engine), it costs +10 and lands at 230. | 12 / 15 | 220 (230 with the split) |
| ECHO | none needed (165 at +10) | — | — |

- **PRE-DLY is never stepped.** Every fallback keeps it a gliding moving read. Stepping it by whole samples at block edges would snap, against CLAUDE.md's rule.

- **Per-sample maths** besides the accesses: tone and dampers (≤ 3 one-poles), gains, the grid quantiser and the lerps. They are in the 5.0 per access, as they are in QUAD's. The LFO, gain ramps and the tap scheduler are per block.
- **`FxBus::COST`.** It does not change while every TYPE ≤ QUAD. The bench's BUS row runs the costliest TYPE's worst settings, chosen by name in `bench.rs` from the REV rows. A TYPE over QUAD raises `COST` to the new BUS reading, so the bill stays honest.

### Bench rows

- The REVERB row on the FX screen stays, as **REV QUAD** (today's `worst_reverb`).
- A new **REVERB bench screen** carries the six new rows, three to a line in today's FX layout, so nothing falls past y 320. The console's `bench` (ADR 0068) prints every row.
- Each row runs at its worst:
  - **ROOM** and **HALL**: SIZE alternating between 1 and 30/31 every block, so the glide always runs; DECAY 1; WOBBLE 1, RATE 1; GRIT 1; MIX 0.5.
  - **GATE** and **RVRS**: LENGTH alternating, so 4 taps always move; DENSITY 1; TAIL or REGEN 1; WOBBLE 1.
  - **BLOOM**: SWELL alternating; DECAY 1; WOBBLE 1.
  - **ECHO**: TIME alternating; WOBBLE 1; RATE 1; DIFFUSE 1; PATTERN CLOUD.
- **The gates are absolute.**
  - **ΔCOST ≤ headroom** (§ Target and hard limit), enforced by the `reverb_headroom` test as each new TYPE lands, plus the stress run on the unit.
  - **Each REV row** is recorded against the 485 target. A row over the target is reported, and is allowed when the A/B justifies it.
- **REV SWITCH MAX** is an on-chip row that records the *maximum* block, not the mean.
  - It runs six A16 ∪ A17 voices while TYPE is forced to switch every 40 blocks (QUAD → HALL → GATE → …), so FADE OUT, CLEAR, the refill after it and FADE IN all land inside the window.
  - A mean over 64 blocks would hide the one CLEAR block, which is why this row takes the maximum.
  - **Gate:** the maximum whole-callback block stays within `BlockBudget::budget_cycles` (70 % of the 64-sample block).
- **Cache.** The REV rows and BUS run with voice traffic between blocks: six A16 ∪ A17 voices play, so the reverb's streams start cold as they do in use. Streams differ (QUAD 48, the new TYPEs 30–37), and each stream misses again after the voices evict it: about 9 cycles per output sample. A warm bench would flatter the TYPEs with fewer streams less than it flatters QUAD.
- The plan records every row's reading, the build and the date, as ADR 0031 did.

## Flash

- **Method.** Release `text`, fat LTO and one codegen unit, on a `git archive` copy: ADR 0083's method, on the ORBIT branch. Thin LTO moves about ±1 KB between builds of the same code.
- **Gate.** Each step's plan task records its delta. A TYPE over 3 KB is cut back (its own code shared or moved out of line) before the next TYPE starts.

| Step | Adds | Estimate |
|---|---|---|
| Scaffold: dispatcher, switch, arena split, read kinds, allpass, tone, LFO, panels, MORE page, TAIL glyph | shared | 3–4 KB |
| GATE + REVERSE | one `BurstEngine` with its tap scheduler, two panel rows, tap tables | 2–2.5 KB (1–1.3 KB per TYPE) |
| ROOM | `LoopEngine<ROOM>`, panel row, layout | 1.5–2 KB |
| HALL | `LoopEngine<HALL>`, panel row, layout | 1.5–2 KB |
| BLOOM | `BloomEngine`, panel row | 1–1.5 KB |
| ECHO | `EchoEngine`, PATTERN weights, panel row | 1–1.5 KB |
| Tempo sync | division lists, fold, SYNC conversion | ≈ 0.5 KB |
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

- `fx_golden_test`'s reverb cases, `reverb_test`, `gain_staging_test`, `fx_bus_test` and `mixer_page_test`'s SIZE nudge (+1/31) pass unchanged, bit for bit, with QUAD on the shared arena.
- That proves the arena move, the dispatcher and slot 3's new 1/128 static step changed nothing. `size_step` still rounds to 32 steps, and the panel keeps QUAD's tick at 1/31.

### Per engine (unit)

- **Layout.** Each TYPE's layout is distinct primes that fit, and is ≤ 23,220 (`const` assert plus test).
- **Reads.**
  - Every computed read length is ≥ 2 (except BURST tap 0, which is not a read), across every knob's ends, WOBBLE 1 and the shortest TIME.
  - ECHO's wobble never exceeds TIME/12.
  - A moving pair at fraction 0 equals the fixed read; at 0.5 it is the mean; and it is correct across the wrap.
- **Mappings.** Each pure mapping hits its table's endpoints and defaults to 1e-3; `exp` and `lin` are monotonic.
- **Gains and floor.**
  - Stage gains ≤ 0.985 (new TYPEs) and ≤ 0.97 (QUAD).
  - HALL reaches its 10 s target at SIZE 0.
  - RT60's floor `t_min` behaves as QUAD's.
- **ROOM's rotation** preserves energy: with the stage gains at 1 and the dampers off, the loop pair's energy is constant to 1e-6 over 10 round trips.
- **Glide.**
  - The length lands on its target.
  - No per-sample length step exceeds ⅛.
  - A full SIZE sweep's instantaneous pitch stays within −2.4 / +2.1 semitones (zero crossings of a steady sine through the line).
- **BURST.**
  - Weights are normalised (Σw² constant within 0.1 dB across SHAPE, CURVE and DENSITY).
  - DENSITY fades taps in continuously: no weight jumps more than 1/64 per 1/128 of the knob.
  - The scheduler never moves more than 4 taps in a block, and every tap reaches its target.
  - Tap 11 never moves faster than ⅛ sample per sample, and the others never faster than ½.
  - A tap leaves the moving set only at fraction 0.
- **ECHO.** PATTERN's weights equal the table; a PATTERN change takes 20 ms with no step.
- **Sync.**
  - `fold` halves and doubles to fit across 30–300 BPM, and every result is in its TYPE's range.
  - A clock jittering ±1 % around a fold boundary (66.7 BPM, ECHO 1/4) never flips the fold.
  - The tempo quantiser holds through ±0.1 BPM of jitter.
  - DECAY in beats clamps. ROOM never offers ALL.
  - The division encoding round-trips. Toggling SYNC on and off keeps the length within one division.
  - Each division list's 120 BPM values are inside the ms range.
- **The `EngineState` zero test** (§ Arena).

### Sound (render)

- **Stable and silent.** At GRIT 0 and 1, every TYPE's energy never grows after the send stops. The return reaches exact zero within **1.5 × the TYPE's longest RT60 at the tested settings**: for ECHO at REPEATS 1 and TIME 900 ms that RT60 is 3 × 0.9 / 0.050 ≈ 54 s, so 81 s. These long tests run the engine alone at 24 kHz, not the whole bus.
  - This includes ECHO at REPEATS 1 and REVERSE at REGEN 1: their truncating feedback writes must reach zero, with no ±Δ hum or DC.
  - These follow `falls_silent_within` and `no_limit_cycle_at_grit_*`, which test QUAD.
- **RT60 tracks DECAY.** Measured on a 0.5 s noise burst (Schroeder EDC, as `rt60_follows_time`), at p = 0, 0.25, 0.5, 0.75 and 1, within ±35 % of the target at GRIT 0. The same tolerance and method as ADR 0028. ROOM at DECAY 0 (0.2 s) checks the rotation.
- **The spike's windows (±30 %).**
  - ROOM at defaults: RT60 0.55 s and onset ≤ 34 ms.
  - HALL at defaults: L/R correlation |ρ| ≤ 0.2.
  - HALL plate corner: onset ≤ 8 ms, with the onset measured as in the A/B (1 ms RMS above −40 dB relative to the peak).
  - GATE at 177 ms: −20 dB end between 185 and 660 ms, and a cliff, the energy in the 50 ms after the end at least 20 dB under the plateau's at TAIL 0.
  - REVERSE: envelope peak within ±15 % of LENGTH, and |ρ| between 0.3 and 0.75.
  - BLOOM at defaults: onset 140–300 ms, peak 450–750 ms.
  - ECHO SINGLE: the first repeat at TIME ±1 sample at 24 kHz.
- **Band-limited**, as `the_return_is_band_limited`.
- **Loudness parity.** Each TYPE's default return RMS is within ±1 dB of QUAD's default on the same noise burst at MIX 0.5. This sets `WET[type]`.
- **Wobble.** At WOBBLE 0 the output equals the output with the LFO frozen. At WOBBLE 1 the spectral peaks of a sustained tail move, as measured by a short-time spectrum.
- **Moving taps don't click.** A full LENGTH sweep (40 → 600 ms) and a SYNC 1/8 → 1/4 step, mid-burst, keep the max second difference within `click_free_test`'s bound.
- **REGEN's tail holds its pitch.** A 1 kHz sine regenerating through REVERSE at REGEN 1 keeps its instantaneous pitch within −2.4 / +2.1 semitones (zero crossings) through a LENGTH move of 150 → 800 ms.
- **Sanitising.** NaN and out-of-range knobs render finite and recover, as QUAD's do.
- **Hot send.** Every TYPE at its maximum feedback (REPEATS 1, REGEN 1, DECAY 1) takes:
  - a full-scale sine swept 50 Hz → 10 kHz over 10 s: the clamp hits on every line write are counted, and must be ≤ 0.1 % of writes;
  - a −12 dBFS pink-noise pad, held 10 s: **zero** clamp hits.

### Switch

- **All 42 ordered pairs of TYPEs**, mid-tail at MIX 1, through `FxBus`:
  - **no click**: the return's max second difference in the switch window ≤ its steady neighbours', `click_free_test`'s measure;
  - the return is **exactly 0** in the CLEAR block;
  - **no stale echo**: from the first FADE IN block, the new engine's pre-gain output is bit-identical to a fresh engine's fed the same send;
  - **the latch**: FADE OUT's output is bit-identical to the old engine run on its old knobs, after `with_type` has overwritten the slots;
  - a second switch during FADE IN starts its fade from the current gain.
- **Peak block cost** is the on-chip REV SWITCH MAX row (§ Bench rows), not a host test.
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
- with sync, after the tempo step: `rev_gate_sync_8th_120`.

They lock refactors, not quality (ADR 0011). Ear tuning re-records them deliberately.

### UI

- Panels: every TYPE's main and MORE labels fit their cells, every unit string fits, and folded values (`▾1/8`) fit.
- No reverb cell returns a hint (ADR 0086).
- TAIL: `tail_shape` is pure, and changes under every knob of its TYPE. Composites draw set values only (ADR 0067's existing test, extended). The box-clear and status tests cover TAIL.
- Screen goldens (`screen_golden_test`) for REV and REV › MORE under QUAD, HALL and ECHO.

### A/B against the Midiverb VST, for the owner

- **The plugin.** The Midiverb II plugin built from github.com/thement/midiverb_emulator. The owner builds it, and the commit hash goes into the A/B report's header before the first render. If the owner uses a different Midiverb VST, its name and version replace this line first. It is run, never linked into or copied into the repository.
- **The stimuli.** Generated by a script committed under `tools/ab/` (no audio committed):
  - a single-sample impulse at −6 dBFS;
  - a 0.5 s pink-noise burst at −12 dBFS RMS;
  - a dry snare and a short saw chord, each peaking at −12 dBFS.
  All are 48 kHz, followed by 12 s of silence.
- **Render settings.**
  - Chimera: rendered on the host through the real bus (`FxBus`), MIX 1, dry off, GRIT 0 unless the pair says otherwise.
  - The VST: 100 % wet in a host at 48 kHz, with its reported latency compensated. A render of the plugin's Defeat program confirms the alignment: the impulse must come back at sample 0 ± 1.
- **Measurement.** The same metrics on both sides:
  - RT60 by T20 (Schroeder EDC);
  - onset: the first sample where the 1 ms RMS envelope rises above −40 dB relative to the response's peak;
  - the envelope's peak time and its −20 dB end;
  - 4 kHz vs 500 Hz tilt;
  - L/R correlation.

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
| BLOOM, SWELL 1 | Bloom 1: listen for a once-per-pass echo instead of a swell (fallback in § BLOOM engine) |
| ECHO PAN | 3 Tap Pan |
| ECHO CLOUD | Multitap |

- **The pass.**
  - Every time metric (RT60, onset, peak, −20 dB end) of every pair lies within ±30 % of the VST's value.
  - The tilt lies within ±3 dB, and the correlation within ±0.15.
  - A pair that fails passes only by the owner's signed-off ear exception, recorded with a reason in the A/B report.
  - ADR 0085 is accepted only when every pair passes on these terms.
- **What the owner gets.** I hand over the table of measurements and the WAV pairs. The owner judges by ear (UAT), and ear changes go back into the constants and goldens, then are re-measured.

## Build order

The CPU risk comes first. Each step after step 0 is a mergeable slice with its tests, goldens, bench row and flash delta.

0. **Baseline and spike.** Nothing for the user yet; this step decides whether the CPU plan holds.
   - Re-measure the REVERB row and QUAD's host count on today's code. Record them as the absolute baseline.
   - Measure one moving pair's marginal cost (the +7 versus +10 question) with a microbench of fixed reads against moving pairs.
   - Write three worst-case kernels, each run on a synthetic arena, benched on the chip as temporary rows and counted on the host:
     - **BURST:** 12 taps (tap 0 from the register, tap 11 shared with REGEN), 4 scheduled moving taps, 2 wobbled decorrelator APs, the TAIL loop and the truncating write;
     - **ROOM loop**, the tightest: PRE-DLY, the diffuser, 2 × 2 stages with all four delays gliding and two wobbled, the rotation and 6 fixed taps;
     - **HALL loop:** PRE-DLY, the diffuser, 1 × 4 stages with all four delays gliding and two wobbled, and 6 fixed taps.
   - BLOOM and ECHO are estimated from the measured pair cost, since BLOOM's count equals HALL's.
   - **Land the `reverb_headroom` test** with QUAD's BUS reading as `QUAD_BUS`, and record the headroom it computes (96 default, 23 with `master-tape`, on 2026-10-04's numbers). Run the stress run with QUAD (six admitted voices, every FX, ORBIT and the UI) and record its OVER, DROPS and peak block load as the reference.
   - **Gate ("sound first"):**
     - each kernel's excess over the baseline QUAD (its ΔCOST) must fit the headroom, 96 (23);
     - a kernel over the 485 target is recorded and kept, and the A/B decides;
     - a kernel whose excess would breach the reserve takes its TYPE's fallback design and is benched again before any TYPE work.

     The kernels become steps 2–4's engines once they pass.
1. **Scaffold, in two slices.**
   - **1a. The arena and the dispatcher.** `ReverbArena` comes out of `Ring`, and `dsp::reverb_bus::Reverb` replaces `RingReverb` in `FxBus`. QUAD stays bit-identical, and REV QUAD reads ≤ the step 0 baseline + 1 %. The `EngineState` enum and its zero test land here.
   - **1b. The parameters and the switch.** The new slots and disk codes; `ReverbType` with QUAD only selectable; `with_type` and the latch; the switch state machine, tested QUAD → QUAD by a forced restart, with the peak-block host test; the read kinds, allpass, tone, LFO and glide helpers, unit-tested alone; `reverb_panel.rs`, REV › MORE and `CompositeId::Reverb`.
2. **GATE, then REVERSE**, on `BurstEngine` (the step 0 kernel): the tap scheduler, the curves, TAIL and REGEN, the TAIL glyph, the first A/B pairs, and the GATE and RVRS rows. The step 0 kernels already settle CPU for steps 2–4, so this order follows the build's dependencies, not its cost.
3. **ROOM**: `LoopEngine`, the rotation, the glide and the wobble; the ROOM row.
4. **HALL** on the same engine: the plate corner and the HALL row.
5. **BLOOM**: the BLOOM row.
6. **ECHO**: PATTERN and the ECHO row.
7. **Tempo sync**, once ORBIT is on `main`: `Tempo` into `FxBus`, its quantiser, SYNC with its conversion, the division lists and the fold. This is the owner's "big one" for GATE. It lands as soon as ORBIT merges, even if steps 5–6 are still open, so the order of 5, 6 and 7 can swap to suit.
8. **A/B and UAT**: the owner's listening pass against § A/B's pass, the ear-tuned constants and goldens, the final bench readings and flash totals, then ADR 0085 to Accepted.

## Review bar

Each plan task's review grades quality, efficiency, performance and principled-ness (the owner's rubric) and needs ≥ 8 on each. This design aims at that:
- **Quality.**
  - One parameter block with typed choices.
  - Pure mappings shared by the DSP and the UI.
  - QUAD locked by goldens.
  - Every TYPE has the same test battery, including limit cycles and the latch.
- **Efficiency.**
  - One code path per engine, and monomorphs only where the graph differs.
  - One arena, cleared in one block.
  - No new gesture or page type.
- **Performance.**
  - Moving reads are costed, kept to five or six per TYPE, and benched on the chip at step 0 before any TYPE work.
  - Every TYPE aims at QUAD's cost and the estimates meet it. The hard gates are ΔCOST ≤ the cost model's headroom (96, or 23 with the tape; a test derives it), and the stress run on the unit ("sound first"), and QUAD itself stays ≤ the absolute baseline.
- **Principled-ness.**
  - The provenance rule is checked per task.
  - Families are graphs, so PLATE is a corner, not a TYPE.
  - The switch is provably stale-free.
  - Absent parameters are absent, not inert.

## Risks

- **A moving pair's real cost** is 17–20 instructions by estimate. Step 0 measures it, and the fallback table covers either end.
- **BURST density with 12 taps.** Pre-diffusion should make 11 reads sound like the MV2's 47–90. If the A/B says grainy:
  - raise the pre-diffuser's g first (free);
  - the next step is 14 taps, +2 fixed reads (+10 instructions, about +19 cycles): BURST stays at or under QUAD, so it fits.
- **Fixed output taps in ROOM and HALL.** The taps sit at fractions of each line's maximum length, so the early pattern no longer scales with SIZE. The ear decides at A/B whether that matters. Scaling them would cost 6 moving pairs, about 190–225 cycles per output sample over the target. That does not fit the 96 (23) headroom; it would take an owner ruling to give up a voice guarantee.
- **ROOM is at the limit if a pair costs +10.** Step 0 benches it, and its fallback design is ready (§ CPU budget).
- **BLOOM's long delay** may echo rather than swell at SWELL 1. The A/B checks it, and the fallback is ready (§ BLOOM engine).
- **The glide's pitch smear.** It may be too obvious on HALL and BLOOM. Slower τ and slew constants are free; only the time to reach a new SIZE grows.
- **ECHO's headroom** is 838 samples, less its guard slots. The wobble's ±5 ms is inside it. A longer TIME is not.
- **#142** (quiet sends lose their tail to i16 truncation) applies to every TYPE alike. It is fixed or not for all of them at once, outside this spec. The new truncating feedback writes add to it at low send levels; the silent-tail tests show how much.
- **ADR 0086** is not yet on `main` (§ Hints).
- **Flash.** ≈ 11–13.5 KB total. Each step's delta is gated (§ Flash).
