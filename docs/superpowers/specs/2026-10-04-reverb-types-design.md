# Reverb TYPEs: QUAD plus ROOM, HALL, GATE, REVERSE and BLOOM

Status: Approved by owner; reviewed (4 rounds), 2026-10-04. ADR [0085](../../adr/0085-the-master-reverb-picks-a-type.md) (Proposed) records the decision. Issue: private issue #343. It is queued after ORBIT ships (owner, 2026-10-03).

The owner's answers of 2026-10-04 are folded in:
- modulation is deferred to private issue #345;
- QUAD keeps its name;
- "sound first" on CPU;
- ECHO is dropped, since the master tape delay covers it, and its design moves to a separate per-Part echo issue;
- reverbs stay pitch-stable.

## Intent

The owner, 2026-10-03:
- The master reverb gets a TYPE setting, and only the selected model runs.
- The TYPEs are families, not presets.
- The method is to study how the Midiverb II programs work and write our own Rust. Then extend past the hardware with an LFO wobble, mod destinations and tempo-synced times.
- No TYPE may cost more CPU than today's reverb. The owner amended this the same day with "sound first" (2026-10-04): QUAD's cost becomes the target, and the hard limit is the cost model's headroom (§ CPU budget).
- All TYPEs share today's delay memory, and nothing grows.

This spec turns that into three engines and five new TYPEs beside today's ring. They share one parameter block, switch without a click, and never re-pitch a tail.

## Rulings this spec follows

| Ruling (owner) | Where it lands |
|---|---|
| A TYPE setting. Only the selected model runs. | § Switching TYPE: one engine at a time, the fade included. |
| TYPEs are families: QUAD, ROOM, HALL (PLATE a zone of it), BLOOM, GATE, REVERSE. The engines are LOOP, BURST and BLOOM. ECHO is dropped (2026-10-04). | § Engines; § PLATE is a zone of HALL. |
| Study github.com/thement/midiverb_emulator (GPL-3.0) as documentation only. Never copy its code or transcribe its ROM coefficient tables. Commit 7119f7ad is not revived. | § Provenance. |
| Extend past the hardware: a loop LFO wobble, mod destinations, and tempo-synced PRE-DLY, GATE LENGTH and optionally DECAY. Mod destinations are deferred to #345 (2026-10-04). | § Wobble; § Modulation (deferred); § Tempo sync. |
| "Sound first" (2026-10-04). QUAD's 485 cycles per output sample is the **target**. The **hard limit** is the cost model's headroom, enforced by a test, plus a stress run on the unit. | § CPU budget; § Build order, step 0. |
| One 23,220-sample i16 arena (46,440 B), and nothing grows. About 1–3 KB of flash per TYPE. | § Arena; § Flash. |
| QUAD stays exactly as it is, crossfade included (2026-10-04). | § QUAD. |
| **Reverbs stay pitch-stable** (2026-10-04). In LOOP, the round-trip sum moves together and the tail is never re-pitched pass after pass. In BURST and REGEN, the cumulative shift per pass is bounded. The tape-echo pitch dive belongs to the future per-Part echo. This supersedes the earlier "SIZE glides via interpolated read pointers". | § Moving a length; § Pitch bounds. |
| A switch fades the old tail out quickly while the arena clears, with no click and no stale echo. Parameter changes are lerped, never snapped. | § Switching TYPE; § Smoothing. |
| No users yet: old projects load safely, as QUAD. | § Storage. |
| User-facing names are sound names. Any MIX or hold gesture gets a contextual focus-band hint. | § UI. The reverb adds no gesture, so it has no hint (ADR 0086). |
| Every review dimension ≥ 8/10. | § Review bar. |

## What the Midiverb II teaches (summary of the #343 spike)

The spike read the emulator's machine model and decompiled programs as documentation, and measured impulse responses. It produced no code and no tables. What carries over:

- **The machine.**
  - 16-bit words in a 16,384-word ring, about 700 ms at 23.4 kHz.
  - 128 instructions per sample.
  - Gains are dyadic (½, ¾, ⅝ …).
  - The only filters are a one-pole average (about 2.6 kHz, "dark") and a two-sample average (about 5.9 kHz, "bright").
  - **No reverb program is modulated, and none is tempo-aware.**
- **The families**, found by dataflow-graph shape:
  - diffuse-loop reverbs, 29 presets: input filter → allpass diffuser → N parallel loops of M "delay → allpass" stages → stereo taps, with N·M usually 8;
  - gated, 10 presets: a flat tap burst → decorrelators, with an optional short loop;
  - reverse, 8 presets: a rising tap burst → decorrelators, with optional regen;
  - bloom, 2 presets: about 15 allpasses in one feedback loop;
  - the rest (flange, chorus, echo, multitap, regen delay, decorrelators) are delays. None of them is a reverb TYPE here.
- **What varies inside a family** is the knob set this spec gives each TYPE:
  - size = the loop's round trip;
  - decay = the per-stage gain;
  - brightness = mostly the *input* filter (about 4.5 dB at 4 kHz between Bright and Dark), with a mild in-loop damper;
  - density = allpass g, ½ or ¾;
  - gate time = the burst span;
  - the reverse ramp = about 24 dB, in 6 dB steps.
- **Measured targets**, kept as acceptance windows of ±30 %, because the clock and the preset names disagree:

| Family | RT60 | Other |
|---|---|---|
| Loop | 0.27 s to about 2.7 s; Xlarge presets 5.2 and 8.4 s | Loop round trip 120–560 ms; onset 0.6–34 ms; L/R correlation ≈ 0 |
| Gate | Burst end (−20 dB) at 185–660 ms; after the cliff 0.16–0.8 s | |
| Reverse | | Envelope peaks at 300–600 ms, then cuts; L/R correlation 0.4–0.65 |
| Bloom | 8.6 s | Onset 140–225 ms; peak at 550–630 ms |

## Provenance

- **Licensing.** The emulator is GPL-3.0, and this workspace was then MIT (`Cargo.toml`; GPL-3.0-or-later since ADR 0330). Nothing from the emulator enters the repository:
  - no code;
  - no instruction listings;
  - no ROM coefficient, offset or tap tables, whole or in part.
- **What the spec records.** The spike's analysis scripts and notes stay outside the repository. This spec records only:
  - prose descriptions of structure;
  - acoustic measurements: RT60, onset, peak, tilt and correlation.

  Those are facts about a sound, not expression.
- **The topologies are textbook ideas**, older than the Midiverb, and Chimera's own composition of them:
  - allpass diffusers (Schroeder, 1962);
  - delay-and-allpass loops (Gardner, 1992; Dattorro, 1997);
  - tap-burst gated and reverse reverbs (1980s studio practice).
- **Every number is ours.** Every length, gain, tap position, sign pattern and range in this spec is chosen here:
  - lengths are generated by `const fn` as distinct primes from our own base values, the way `ring.rs` builds QUAD's `SIZE_TABLE`;
  - gains follow from RT60 targets by formula;
  - the final values are tuned by ear and by measurement against the Midiverb VST.
- **The old port stays dead.** Commit 7119f7ad ("faithful MidiVerb II emulation from decompiled DASP microcode", retired by ADR 0028) is not revived, consulted for values or cherry-picked.
- **Checked per task.** The review checklist for every engine task has one item for this: no constant may come from the emulator or 7119f7ad. The task's report lists where each table's starting values came from (this spec's tables) and what the ear changed.

## The TYPE list

| # on screen | TYPE (word on screen) | Engine | Disk code | Ident |
|---|---|---|---|---|
| 1 | QUAD (the owner's name) | today's ring (`dsp::ring`, ADR 0028) | 0 | `QUAD` |
| 2 | ROOM | LOOP, 2 loops × 2 stages | 1 | `ROOM` |
| 3 | HALL | LOOP, 1 loop × 4 stages | 2 | `HALL` |
| 4 | BLOOM | BLOOM | 5 | `BLOOM` |
| 5 | GATE | BURST, flat-to-falling curve, tail loop | 3 | `GATE` |
| 6 | REVERSE | BURST, rising curve, regen | 4 | `REVERSE` |

- The screen order runs from the plainest spaces to the effects. It is display only.
- Disk codes are frozen in `tests/fixtures/disk_codes_v1.txt` and never reused (ADR 0045).
- Code 6 was never assigned, because ECHO never shipped.

### PLATE is a zone of HALL

The owner's rule is that TYPEs are families, not presets, and a family is a different dataflow graph. ROOM (2 × 2) and HALL (1 × 4) are different graphs on one engine, so both are TYPEs. A plate on the HALL graph differs from a hall only in constants:
- a short loop;
- maximum diffusion;
- a bright input;
- no pre-delay.

That makes it a preset, and the MV2 itself had no plate topology. So:
- **HALL's SIZE reaches plate territory**: its range goes down to a 120 ms loop (§ HALL).
- **One knob reaches it.** HALL's SIZE has a **plate zone** at its bottom, 0 → 0.15, with w = 1 − SIZE/0.15 (1 at SIZE 0):
  - the engine reads DIFFUSE as `DIFFUSE + w·(1 − DIFFUSE)`, TONE as `TONE·(1 − w)` and PRE-DLY as `PRE-DLY·(1 − w)`;
  - so turning SIZE to 0 gives the plate's diffusion, brightness and zero pre-delay, whatever those three knobs say. Above 0.15 they are untouched;
  - the stored knobs never change. The SIZE cell's value text reads `PLATE` with its ms in the zone, and TAIL draws the effective values;
  - DIFFUSE and TONE take the zone's values through their per-block eases. **PRE-DLY's** effective length moves, so it goes through the crossfade path like SIZE (§ Moving a length), in steps of at most 5 ms. Nothing is lerped per sample.
  - Cost: the eases and the crossfade setup already exist, and the zone itself is about 150 B of flash.
- **The plate corner** is a named, tested setting: HALL with SIZE 0, DECAY 0.35, TONE 0, DIFFUSE 1, DAMP 0.1, PRE-DLY 0, WOBBLE 0.5 and RATE 0.6. It has its own golden and its own A/B render.
- **Reversing this later is cheap.** A PLATE TYPE would be a new disk code mapped to the HALL engine, with its own panel row and ranges: about 200 B of flash and no DSP.

## Engines

### Shared machinery

- **Rate.** Every engine runs at half rate, 24 kHz on the chip, behind today's half-band decimator and two interpolators (ADR 0028). They are shared and unchanged. The band limit stays the dark top every TYPE shares.
- **Storage.**
  - Every line is i16, full scale ±2.0, in the one arena (§ Arena).
  - GRIT's grid applies to every write in every TYPE: Δ = 2^(6·GRIT) LSBs (ADR 0028).
  - ADR 0028's Q3 rule carries over: allpass state writes truncate toward zero, and delay and damper writes round.
  - **A loop with no allpass in it truncates its feedback write.** That means BURST's line when REGEN is up. Rounding there would sustain a limit cycle: any gain above ½ holds ±Δ, as hum or DC, up to −48 dB at GRIT 1. Truncation toward zero always decays.
  - FREEZE is not in this spec. No loop gain reaches 1.
- **Two kinds of read.**
  - **Fixed reads** sit at an integer offset that changes only at a crossfade's start. These are QUAD's kind of read: they go through `ring.rs`'s wrap-free segments, one integer load each, with no per-sample index work.
  - **Moving reads** are only the wobble's. Each is a linear pair, `x0 + f·(x1 − x0)`. Its index is computed per sample (integer part, fraction, and a branch-free wrap of both slots), so it stays out of the segment bound. That costs about 7–10 instructions more than two fixed reads.
  - **Every length that a knob or the tempo moves** (SIZE, LENGTH, SWELL, PRE-DLY) changes by a pitch-free crossfade between fixed reads (§ Moving a length), never by a moving read.
  - **Output taps never move.** They sit at fixed offsets behind their line's write point (§ LOOP engine).
- **Reads never reach the write head.** Every read is at least 2 samples behind its line's write point (`d ≥ 2`, a clamp after each length is computed). The one exception is BURST's tap 0, which is the input value itself, taken from a register.
- **Allpass.** `v = x − g·z` is written (truncated onto the grid), and `y = z + g·v`. It has the same form as QUAD's.
- **Tone filter.** One one-pole lowpass per engine on the input, after the decimator: `y += a·(x − y)`, with `a = 1 − exp(−2π·fc / 24 000)`. TONE sets fc, from bright (0) to dark (1). This is where MV2's Bright, Warm and Dark live.
- **Dampers.**
  - One-pole lowpasses inside loops, written onto the grid with rounding, like QUAD's DAMP.
  - In ROOM, HALL, BLOOM and GATE's TAIL, each sits in a loop that also holds a truncating allpass.
  - REGEN's smoothing one-pole does not. That is harmless: its loop's feedback write truncates (the Storage bullet above), so the loop still decays to zero.
- **Decay.**
  - Each loop stage's gain is `g_k = 10^(−3·L_k / (RT60·fs))`, where L_k is the stage's share of the round trip (QUAD's `stage_gains`).
  - The new engines cap a stage at **0.985**; QUAD keeps its 0.97. The truncating writes keep every loop decaying, and 0.985⁴ reaches HALL's 10 s at its 120 ms loop (§ Each TYPE's knobs).
  - RT60 has a floor at `t_min`, the longest allpass's own ring, as in QUAD.
  - Gains ramp across each block from the last block's value (QUAD's `RingBlock`).
- **The wobble LFO.**
  - One per engine: a `u32` phase and a branch-free parabolic sine, the same idiom as the chorus's LFO (ADR 0031, `335cd3a`).
  - Its moved reads run 90° apart, so the left and right tails move differently.
  - It is computed at the block's ends and interpolated linearly across the block's 32 ring samples. At its fastest rate, 2 Hz, the error is under 10⁻⁴ of the depth.
  - The pitch it can cause is bounded (§ Pitch bounds).
- **Input gain is ½ in every engine**, as in QUAD, so every TYPE keeps QUAD's resolution. Headroom comes from the level plan instead (§ Level plan).
- **Output.** The return is ×`WET[type]` × MIX, eased as today. Each TYPE's `WET` is set so its default return's RMS matches QUAD's default within ±1 dB. A test pins this (§ Tests). QUAD's `WET_GAIN`, 4.54, is unchanged.

### Level plan

Every stored value must stay inside the i16 full scale, ±2.0 in line units, at maximum DECAY and with the worst-case signal. This section derives each bound end to end. Tests hold each one (§ Tests).

**Terms.**
- **The send.** A full-scale (0 dBFS) send sample is 1.0. It is injected at ½, so the line-unit send peak is **A = 0.5**.
- **Bounds are H∞ bounds.** A bound is the peak gain for a steady sine at the worst frequency (the H∞ norm). That is the standard bound for a level plan, and it is what a held tone on a loop mode reaches.
  - An allpass has output gain 1 and state gain 1/(1 − g).
  - A one-pole lowpass has gain 1.
  - A loop at maximum DECAY has G = 1/(1 − round-trip gain): 12.5 for HALL (0.92 at 10 s on its 120 ms loop), about 7.5 for BLOOM, and 2.7 for ROOM.
- **Transients** can briefly exceed an H∞ bound: an allpass's output can overshoot to (1 + g)/(1 − g) of its input for a few samples. § Transients covers that case.

**The four rules.**
1. **R1: every loop's re-entry passes a soft clip.**
   - The clip is `s(x) = x` for |x| ≤ 1.0, and `s(x) = sign(x)·(1 + u − u²/4)` with u = min(|x| − 1, 2) above that. It reaches exactly 2.0, with slope 0, at |x| = 3.
   - It is branch-free, about 6 instructions.
   - The clips sit at ROOM's two rotation outputs, HALL's re-entry, BLOOM's re-entry, BURST's line write (which carries REGEN) and GATE's TAIL re-entry.
   - So every signal written inside a loop is ≤ 2.0: delays, dampers (gain 1) and re-entries.
2. **R2: every allpass inside a loop stores its state normalised.**
   - It writes `w = (1 − g)·v` and reads back `v = w · 1/(1 − g)`, with 1/(1 − g) computed once a block.
   - Because |v| ≤ |x|/(1 − g), it follows that |w| ≤ |x| ≤ 2.0, whatever g is.
   - Its output, at gain 1, is ≤ 2.0.
   - Cost: 2 multiplies per in-loop allpass.
   - **The density comes back.** Since the bound no longer depends on g, in-loop g runs up to **0.75** again, the Midiverb's ¾ density:
     - ROOM and HALL stage APs, by DIFFUSE, 0.5 → 0.75;
     - BLOOM's loop APs, 0.45 → 0.65;
     - GATE's TAIL AP, fixed at 0.5.
   - Noise: the stored error comes back ×1/(1 − g). That is +6 dB at g 0.5, +8 at 0.6, +9 at 0.65 and +12 at 0.75, on those states' error only (§ Noise floor below).
3. **R3: diffusers outside loops are f32.** A chain of allpasses has a worst-case (L1) output gain of 1 + 2g per allpass, not 1: 2.5 at g 0.75. So a 0.5 peak can drive the next diffuser state to about 4.8, and normalising each state would not stop the chain's gain from compounding.
   - So the ROOM and HALL diffusers and BURST's pre-diffuser store f32, in the arena's own free slots, at 2 i16 slots per sample. There is no i16 bound to break, and no quantisation noise.
   - That is cheaper than normalising: an f32 store replaces the quantise-and-store, so the cost is +0 instructions, against about +8 for normalisation.
   - Their output, ≤ (1 + 2g)⁴ × 0.5 at worst, enters the loop sum or the burst line, and R1's clip bounds it there.
   - DIFFUSE's diffuser range goes back to 0.5–0.75.
   - GRIT's grain lives in the lines and loops, as in QUAD. The f32 diffusers take none, a few ms of pre-echo density where grain is inaudible.
4. **R4: output decorrelators are f32.** BURST's and BLOOM's decorrelators sit after a sum that can exceed 2.0. BURST's tap sum Σ is ≤ Σ|w| × 2.0 = 6.9, and a decorrelator at g 0.6 raises a state to 2.5 × its input. So they store f32, like R3's diffusers (§ Arena), with no i16 bound and no quantisation noise.

**Every stored quantity, at maximum DECAY:**

| Stored quantity | TYPEs | Bound | By |
|---|---|---|---|
| PRE-DLY line | ROOM, HALL, BLOOM | 0.5 (the send, through TONE at gain 1) | — |
| Diffuser and pre-diffuser AP states | ROOM, HALL, GATE, REVERSE | f32 | R3 |
| Burst line (pre-diffused input + REGEN) | GATE, REVERSE | 2.0 | R1, at the line write |
| Stage delays, SWELL delay | ROOM, HALL, BLOOM | 2.0 | R1 |
| Dampers, REGEN smoothing | all loops | 2.0 | R1, gain 1 |
| In-loop AP states (stage APs, BLOOM's ten, TAIL's AP) | ROOM, HALL, BLOOM, GATE | 2.0 | R2 |
| TAIL delay | GATE | 2.0: its input is Σ/Σ\|w\| (≤ 2.0), clipped at re-entry; it returns ×Σ\|w\| in float | R1 |
| Decorrelator states | GATE, REVERSE, BLOOM | f32 | R4 |
| Output tap sums (LOOP L/R, BLOOM AP5/AP10) | all | float, never stored as i16 | — |

**Transients.** H∞ bounds hold for steady signals. Inside a loop, an allpass output can overshoot its input by up to its L1 gain, 1 + 2g, for a few samples: 2.5× at g 0.75. That output is written to the next stage's delay before the re-entry clip.
- **What catches it.** The i16 store saturates, as QUAD's always has.
- **How it is bounded.** By test, not by assumption: zero clamp hits for a held loop-mode sine and for −12 dBFS pink noise, and ≤ 0.1 % of writes for a full-scale swept sine at maximum DECAY (§ Tests).
- **Why not more clips.** Clipping every write would cost about 6 instructions per write, 78 a sample in ROOM.

**Transparency.**
- **At the default DECAY**, a −12 dBFS RMS pink send peaks near 0 dBFS, which is 0.5 in line units. Recirculation adds about +3 dB, to about 0.7, under the 1.0 knee. The clip never engages, and the output is bit-identical to an unclipped build.
- **At maximum DECAY**, recirculation adds about +8 dB, so pink-noise peaks cross the knee. The clip shaves those peaks only: a few percent of samples, with distortion ≤ −40 dB against an unclipped f32 reference.
- **A held full-scale tone on a loop mode** saturates softly. Its THD is bounded at 3 %.

**Aliasing.** The clip is odd-symmetric, so it makes odd harmonics, at the 24 kHz rate. A held 5 kHz tone's 3rd harmonic, at 15 kHz, folds to 9 kHz.
- The clip only engages under coherent build-up, and its 3rd harmonic is bounded by the 3 % THD.
- Alias energy is bounded by test at −40 dB, on a held loop mode near 5 kHz.
- Oversampling the clip was rejected: it would double the clip's cost on every loop.

**Noise floor**, at MIX 1, against QUAD's estimated −66 dBFS at GRIT 0 and −55 at GRIT 0.3:

| TYPE | Estimate | Why |
|---|---|---|
| ROOM, HALL | +2 to +3 dB at the default DIFFUSE (g 0.6); +4 to +6 dB at DIFFUSE 1 (g 0.75) | 4 normalised stage APs, +8 to +12 dB on their error; the f32 diffusers add none |
| GATE, REVERSE | ±2 dB | f32 decorrelators (no noise); the TAIL path alone runs up to 10.8 dB lower, inside a tail ≥ 20 dB below the burst |
| BLOOM | +3 to +4 dB at the default (g 0.55); +4 to +5 dB at DIFFUSE 1 (g 0.65) | ten normalised loop APs, +7 to +9 dB on their error |

- **GRIT means the same on every TYPE.** Its step is `2^(6·GRIT)` LSBs × IN/0.5, which is 1 today, since IN is ½ everywhere.
- **The noise test bound** is QUAD's measured floor + 4 dB at each TYPE's defaults, and + 6 dB at DIFFUSE 1.
- **Density against grain is an A/B item.** DIFFUSE 1 buys the Midiverb's ¾ density in ROOM and on the PLATE zone, at +4 to +6 dB of noise in quiet tails. The A/B compares DIFFUSE 0.4 and 1 on a quiet tail at GRIT 0. If the owner hears the grain, DIFFUSE's top stops lower, at the g the ear accepts.

### Moving a length

Owner, 2026-10-04: reverbs stay pitch-stable.
- A tail must never be re-pitched pass after pass.
- In LOOP the round-trip sum moves together.
- In BURST and REGEN the cumulative shift per pass is bounded.

**Why not a glide.** A read pointer that glides inside a loop re-pitches the tail on every pass. Over a whole move, a component that keeps circulating ends up shifted by the length ratio, L_old/L_new: HALL's SIZE sweep from 120 to 600 ms would drop a ringing tail 28 semitones. Slowing the glide does not change that total, it only spreads it out. So no length that a knob moves is ever glided.

**How lengths move instead: pitch-free crossfades, all together.**
- **LOOP.** A new SIZE target, or a PRE-DLY or tempo target, starts one crossfade if none is running. A request during a crossfade waits for its end (QUAD's rule).
  - **The crossfade.** Every line whose length changes (the stage delays and PRE-DLY) reads both its old and its new integer length as fixed reads, and blends them linearly over 720 ring samples (30 ms, QUAD's `XFADE`).
  - **All together.** Every stage delay of the loop switches inside the same crossfade, so the round trip moves as one sum.
  - **The slew.** One crossfade moves the round trip by at most **5 ms** (120 samples), and every other crossfading line (PRE-DLY) by at most 5 ms too. That is QUAD's own step, about 4.7 ms. Long moves take more steps, back to back:
    - ROOM 100 → 200 ms: 20 crossfades, about 0.6 s;
    - HALL 120 → 600 ms: 96 crossfades, about 2.9 s;
    - BLOOM 250 → 700 ms: 90 crossfades, about 2.7 s;
    - PRE-DLY 0 → 125 ms: 25 crossfades, about 0.75 s, run in the same crossfades as SIZE.
  - **Why so small.** A crossfade between two lengths duplicates or drops a slice of the circulating tail as long as the step. A 60 ms slice is a ghost slap on every pass, and cancels at the 50 % point. A 5 ms slice is a brief comb at 200 Hz spacing, as in QUAD.
  - **Pitch.** During a crossfade the loop's content is not resampled: each read is at a fixed position, and only the blend weight moves.
    - **While a crossfade runs**, the blend of two reads 5 ms apart slides the phase by up to π over 30 ms. That is up to about **29 cents** instantaneously, and about **7 cents** averaged over a 120 ms pass.
    - **Once the move settles**, nothing is left of it: no read was resampled, so the tail rings on the loop modes of the new length at its original pitch, with no accumulated shift.
    - While the blend runs there is a 30 ms comb colour, as in QUAD.
- **BLOOM.** The SWELL delay and PRE-DLY crossfade together, under the same rule.
- **BURST.** Taps crossfade, at most 4 at a time (§ BURST engine). Tap 11, REGEN's feedback read, crossfades like the others, so the regenerating tail carries no accumulated shift once the move settles.
- **Cost.** A crossfading line costs one more fixed read and a blend. A long move is many crossfades back to back, so while it runs the line is *always* crossfading. That is the worst case the CPU table bills, plus a crossfade setup of about 10 instructions per line every 720 samples, under 0.1 instruction a sample (§ CPU budget). It is still cheaper than the moving pair a glide would need.

### Pitch bounds

The bound, stated per pass and cumulatively, for every TYPE:

| Source | Per pass | Cumulative |
|---|---|---|
| SIZE, LENGTH, SWELL, PRE-DLY and tempo moves (all TYPEs) | while a crossfade runs: up to about 29 cents instantaneously, about 7 averaged over a 120 ms pass; none once settled | 0 once settled: the spectral peak sits within 1/L of the input's |
| REGEN's feedback (REVERSE), during LENGTH moves | the same | the same |
| Wobble inside a loop (ROOM, HALL, BLOOM) | ≤ 25 cents | ≤ 25 cents, with no drift: the shift returns to 0 each LFO cycle |
| Wobble on BURST's decorrelators (outside every loop) | single pass only: ≤ 22 cents at ±1 ms and 2 Hz | not fed back, so no accumulation |

**The wobble bound.**
- A delay that wobbles inside a loop can only stretch the circulating content by its own excursion. Over any number of passes, a component's frequency stays within the ratio `L / (L − E)`, where E is the loop's total peak-to-peak wobble excursion and L its round trip.
- **The cap.** Each loop caps its wobble so that E ≤ 1.45 % of L, which gives 1200·log2(1/(1 − 0.0145)) ≈ **25 cents**:
  - **ROOM:** one wobbled allpass per loop, so E = 2D, and D ≤ 0.72 % of L: 0.72 ms at a 100 ms loop.
  - **HALL:** two wobbled allpasses in quadrature, so E = 2√2·D, and D ≤ 0.51 % of L: 0.62 ms at the 120 ms plate, 1.75 ms at the default 342 ms.
  - **BLOOM:** three wobbled allpasses at 0°, 90° and 180°. The 0° and 180° ones cancel in the loop sum, so E = 2D, and D ≤ 0.72 % of L: 1.8 ms at 250 ms.
- The cap applies after WOBBLE's mapping, as the knob tables say. The panel's value text shows the depth that plays.
- At the defaults the wobble sits well under its cap: ROOM about 9 cents, HALL about 7, BLOOM about 3.

### LOOP engine: ROOM and HALL

```
send → TONE → PRE-DLY → diffuser (4 AP, fixed) ─┬─► loop A: [delay → AP] → [delay → AP] → DAMP ─┐
                                                 │                                                ├─ rotation g·R(θ) ─► back into A and B
                                                 └─► loop B (ROOM only): same shape, own lengths ─┘
outputs: 3 fixed taps per side, from different stage delays, signs ±
```

- **Shape.** The engine is one generic, `LoopEngine<const SHAPE: LoopShape>`, monomorphised twice: ROOM is 2 loops × 2 stages, HALL is 1 loop × 4 stages.
- **SIZE** scales only the stage *delays*, by k, and they move by crossfade (§ Moving a length). The stage allpasses and the diffuser are fixed.
- **Coupling in ROOM.**
  - The two loops' outputs (a, b) re-enter through an energy-preserving rotation: A gets `c·a + s·b`, and B gets `−s·a + c·b`. Here c = cos θ, s = sin θ and c² + s² = 1; θ = 0.25 rad, a light coupling.
  - The stage gains carry the decay, so the round trip's gain is exactly g and RT60 tracks DECAY.
  - The send enters A at +½ and B at −½, as QUAD injects into S1 and S3.
- **Dampers.** ROOM has one per loop. HALL has two, after stages 2 and 4.
- **Wobble** moves stage *allpasses*, which never crossfade: in ROOM, A2's AP and B1's AP; in HALL, the APs of stages 1 and 3, in quadrature.

Starting lengths in ms at SIZE 1. Each becomes the nearest distinct prime in samples at 24 kHz, as `ring.rs` builds QUAD's:

| | Diffuser APs | Stage 1 delay / AP | Stage 2 delay / AP | Stage 3 delay / AP | Stage 4 delay / AP | k (SIZE 0 → 1) |
|---|---|---|---|---|---|---|
| ROOM loop A | 2.3, 3.7, 5.9, 8.9 | 59 / 17 | 101 / 23 | | | 0.375 → 1 (round trip 100 → 200 ms) |
| ROOM loop B | (shared) | 73 / 13 | 95 / 19 | | | same k |
| HALL | 1.7, 3.1, 5.3, 8.9 | 113 / 11 | 157 / 17 | 124 / 13 | 146 / 19 | 0.111 → 1 (round trip 120 → 600 ms) |

Output taps are (line, fraction, sign). They are fixed reads:
- **ROOM:** L = (A1, **0.10 of SIZE 0**, +), (B2, 0.57, −), (A2, 0.83, +). R = (B1, **0.12 of SIZE 0**, +), (A2, 0.53, −), (B2, 0.79, +).
- **HALL:** L = (S1, **0.30 of SIZE 0**, +), (S3, 0.61, −), (S2, 0.83, +). R = (S2, **0.25 of SIZE 0**, +), (S4, 0.49, −), (S1, 0.91, +).

Fractions without a mark are of the line's **maximum** length.
- **The early tap.** Each side's first tap is a fraction of its line's SIZE-0 length: about 2–3 ms in ROOM and about 4 ms in HALL. That keeps the plate corner's onset short.
- **Why the maximum, for the other two.** A line owns its maximum length's slots, and the shared write head writes every one of them each pass. So a tap anywhere inside the maximum reads written data at any SIZE, at the same cost. At a small SIZE, a tap past the current read point reads the line's input from an earlier pass: still the decaying tail, just older.
- **Why not the minimum.** Fractions of the SIZE-0 length would read only the first 11 % of HALL's 540 ms stages at SIZE 1, and the output could pulse once per pass.
- **The trade.** The pattern stays put as SIZE moves, and the ear judges it at A/B. SIZE-scaled taps would add 6 crossfading reads to every SIZE move: about +96 cycles over QUAD for ROOM. That is at the default build's limit (96) and over `master-tape`'s (23), so they need an owner ruling (§ CPU budget).

### BURST engine: GATE and REVERSE

```
send → TONE → pre-diffuser (2 AP) → burst line ──► 12 taps over [0, LENGTH], weights w_k, signs s_k ─► Σ ─┬─► decorrelator L (2 AP) → L
                     ▲                                    │                                         └─► decorrelator R (2 AP) → R
                     └── REGEN (REVERSE): tap 11 × g ─────┘          GATE: Σ also feeds a 1-stage TAIL loop (delay → AP → ×g), added back to Σ
```

- **One engine, one code path.** GATE runs with REGEN's gain at 0, and REVERSE with TAIL's gain at 0. Both pay for both paths, so there is only one monomorph.
- **Taps.**
  - There are 12 taps. Tap k sits at `LENGTH·(k + j_k)/11`, for k = 0…11.
  - j_0 = j_11 = 0. The others are a fixed jitter within ±0.3, from a `const fn` LCG seeded in this spec (seed `0x5EED_0343`). The jitter breaks the comb a regular spacing would ring.
  - Signs s_k are a fixed ± pattern from the same LCG.
  - **Tap 0 is the line's input value**, taken from the register before the write, so it needs no read.
  - **Tap 11 is also REGEN's feedback read**: one read serves both.
- **How taps follow LENGTH.**
  - Taps 1–11 are fixed reads.
  - When LENGTH or the tempo moves, a scheduler crossfades **at most 4 taps at a time**, the ones farthest from their targets.
  - Each crossfade lasts 128 ring samples (4 blocks) and moves a tap by at most **120 samples** (5 ms), QUAD's step.
  - Moving the burst from 250 to 500 ms (1/8 → 1/4 at 120 BPM) moves the taps 36,000 samples in all, at up to 4 × 120 per 128 samples: about 0.4 s. Tap 11 alone moves 6,000 samples in 50 steps, about 0.27 s.
  - Tap 11 crossfades like the others, so REGEN's tail is never re-pitched (§ Pitch bounds).
- **Curves.**
  - GATE: `w_k ∝ 10^(−SHAPE_dB·k/11/20)`, with SHAPE_dB = 18·SHAPE. It runs from flat (a hard cut) to a falling 18 dB (the "dying" slow gate).
  - REVERSE: `w_k ∝ 10^(−CURVE_dB·(11−k)/11/20)`, with CURVE_dB = 12 + 18·CURVE. It is a rising ramp of 12–30 dB; the MV2 used about 24 dB in steps, and ours is smooth.
  - The weights are normalised so the sum of their squares is constant. Loudness holds as SHAPE, CURVE and DENSITY move.
- **DENSITY.**
  - Taps 6–11 fade in one after another across the knob. Tap 6 + i weighs `clamp(6·DENSITY − i, 0, 1)` before normalising: 6 taps at 0 and 12 at 1, never a step.
  - DENSITY also sets the pre-diffuser's g, 0.5 → 0.7.
  - Diffusing *before* the taps is what lets 11 reads do the work of the MV2's 47–90.
- **TAIL (GATE).** The burst sum also feeds one fixed loop stage: a 37 ms delay, then an 11 ms AP. Its output is ×g and added back into the sum. g comes from TAIL's RT60 and is scaled by `min(1, TAIL/0.05)`, so TAIL 0 is exactly the fast gate, with no step.
- **REGEN (REVERSE).** Tap 11 is fed back into the burst line's input through a one-pole at about 6 kHz, so each pass darkens. The gain is g = 0.708·REGEN, up to −3 dB per pass. The line write that carries it truncates (§ Shared machinery).
- **Decorrelators.**
  - Two APs per side, at g 0.6: L 3.1 and 8.3 ms, R 4.7 and 6.1 ms, stored f32 (§ Level plan).
  - Each side's second AP is the one the wobble moves. They sit outside every loop, so their wobble is a single-pass vibrato (§ Pitch bounds).
  - The taps sum mono first (the MV2 shape), and the decorrelators make the stereo.

### BLOOM engine

```
send → PRE-DLY → (+) → AP1 → … → AP10 → SWELL delay → DAMP(TONE) → ×g ─┐
                  ▲_____________________________________________________┘
L = AP5's output → decorrelator L (2 AP) ;  R = AP10's output → decorrelator R (2 AP)
```

- **The loop** is ten fixed allpasses and one plain delay. The density keeps building for several passes, and that is the swell.
- **Allpass lengths:** 7, 11, 13, 19, 23, 29, 31, 37, 41 and 43 ms, about 254 ms in all.
- **SWELL** sets the loop delay, 0 → 446 ms, so the round trip runs from about 250 to 700 ms. It moves by crossfade, together with PRE-DLY (§ Moving a length).
- **Risk: an echo instead of a swell.** At SWELL 1, 446 ms of the 700 ms loop is plain delay, and the loop may repeat audibly once per pass instead of blooming.
  - The A/B checks this explicitly.
  - **Fallback:** split the SWELL delay in two, with one fixed allpass between the halves (17 ms, g 0.5). That costs 2 fixed accesses, +10 instructions, about +19 cycles per output sample, and fits the headroom (§ CPU budget).
- **TONE** is the in-loop damper here, fc 10 kHz → 1.5 kHz. The bloom's darkness lives in the loop, as the spike measured: HF RT60 about 0.9× LF.
- **DIFFUSE** sets the loop APs' g, 0.45 → 0.65. Their states are stored normalised (§ Level plan).
- **Taps.** The outputs are values already in registers (AP5's and AP10's outputs), not reads, so they cost nothing.
- **Decorrelators:** L 4.1 and 9.7 ms, R 5.3 and 7.9 ms, at g 0.6, stored f32 (§ Level plan).
- **Wobble** moves APs 3, 6 and 9 (moving reads), at 0°, 90° and 180°.

### QUAD

Owner ruling (2026-10-04): leave QUAD exactly as it is.
- QUAD is today's `dsp::ring`, bit for bit. Its sound and its cost are unchanged.
- Its only change is internal: `Ring` borrows the arena instead of owning `lines`.
- It keeps its 32-step double-read SIZE crossfade, which is clean and pitch-free. The new TYPEs now move their lengths the same pitch-free way, on fewer lines (§ Moving a length).
- That crossfade is QUAD's measured worst case, and the target the other TYPEs aim at (§ CPU budget).
- QUAD shows no MORE knobs, takes no wobble or sync, and its goldens do not move (ADR 0011).

## Arena

- The arena is `ReverbArena { lines: [i16; 23_220] }`, 46,440 B in `FxBus` (AXI, ADR 0014). It is today's `Ring::lines` moved out, unchanged in size.
- Each TYPE has a `const` layout:
  - every line owns `max_len + 2` slots: its longest length, plus the wobble's depth where it wobbles, plus one slot for a pair's second read;
  - all lines rotate with one shared write head, `ring.rs`'s scheme (`OFFSET`, `HEAD`, wrap-free segments for the fixed reads);
  - a `const` assert per TYPE proves its layout fits in 23,220.

| TYPE | Lines at their maximum (samples at 24 kHz) | Total | Headroom |
|---|---|---|---|
| QUAD | 12 lines (`RING_LEN`) | 23,220 | 0 |
| ROOM | PRE 1,200 · diffuser 498 f32 = 996 slots · delays at k = 1 7,872 · stage APs 1,728 · wobble 48 | 11,844 | 11,376 |
| HALL | PRE 3,000 · diffuser 456 f32 = 912 slots · delays at k = 1 12,960 · stage APs 1,440 · wobble 96 | 18,408 | 4,812 |
| GATE, REVERSE | pre-diffuser 274 f32 = 548 slots · burst line 19,200 (REVERSE's 800 ms) · TAIL loop 1,152 · decorrelators 533 f32 = 1,066 slots · wobble 48 | 22,014 | 1,206 |
| BLOOM | PRE 3,000 · loop APs 6,096 · SWELL delay 10,704 · decorrelators 648 f32 = 1,296 slots · wobble 144 | 21,240 | 1,980 |

Each line also takes its 2 guard slots, under 50 per TYPE, inside the headroom shown. The arena and `RING_LEN` do not change.

**f32 regions.** The diffusers (R3) and decorrelators (R4) are f32 regions inside the i16 arena.
- `ReverbArena` is `#[repr(C, align(4))]`, and each f32 region starts on an even slot.
- One function, `ReverbArena::f32_region(offset, len) -> &mut [f32]`, gives access to them, with a `// SAFETY:` comment: alignment holds by the type and the even offset, the region is in bounds by the `const` layout, and no i16 view of it is alive while it is used.
- CLEAR's zeroing makes them +0.0.

**State beside the arena.**
- Each engine's small state lives in one `#[repr(C, u8)] enum EngineState`, sized to the largest variant. That state is the dampers, the tone filter, the LFO phase, the current and target lengths, the crossfade counter, the tap scheduler and the gains.
- It targets ≤ 160 B; a `memory_budget_test` assert pins it.
- `FxBus::init_in_place` zeroes it, as it zeroes everything. Variant 0 is `Quad`, and every field of every variant is an integer or a float, so zero bytes are a valid `Quad`: discriminant 0 with zeroed fields.
  - The `// SAFETY:` comment on the zeroing says exactly that.
  - A test checks that a zeroed `EngineState` equals `EngineState::Quad(Default::default())`.
- That is the only growth in `FxBus`, and the arena does not grow.

## Parameters

### One block, shared slots

- **One block.** The reverb stays one block, `BlockRef::Reverb` (disk code 22), with one static `REVERB_SPECS`.
- **Why slots.** The storage codec walks the static table (`ValidAddr::of_block`) and caps a block at 32 params (`MAX_BLOCK_PARAMS`). Per-TYPE specs would need about 6 × 8 = 48 ids, so they are out.
- **Semantic slots.** Each id is a semantic slot, stored 0..1 like today's reverb params. A per-TYPE **panel** gives each slot its label, range, unit and encoder step under that TYPE, or marks it absent. The panel is `const` data in `ui/reverb_panel.rs`, the pattern ADR 0037's `filter_panel.rs` set.

| ParamId | Ident (frozen) | Kind | Slot meaning | Spec default | New? |
|---|---|---|---|---|---|
| 0 | — | — | retired TYPE of the pre-diet bus, never reused (ADR 0009, 0028) | — | — |
| 1 | `TIME` | continuous 0..1 | how long it lasts: QUAD TIME, DECAY, TAIL, REGEN | 0.5 | no |
| 2 | `DAMP` | continuous 0..1 | in-loop HF damping: QUAD DAMP, HALL DAMP | 0.3 | no |
| 3 | `SIZE` | continuous 0..1 | how big or how long the space is: SIZE, LENGTH, SWELL | 0.5 | no |
| 4 | `MIX` | continuous 0..1 | return level | 0.0 | no |
| 5 | `GRIT` | continuous 0..1 | the i16 grid | 0.3 | no |
| 6 | `TYPE` | Enum, `ReverbType` | the TYPE | QUAD | yes |
| 7 | `TONE` | continuous 0..1 | input brightness (BLOOM: the in-loop damper) | 0.3 | yes |
| 8 | `DIFFUSE` | continuous 0..1 | density: DIFFUSE, DENSITY | 0.4 | yes |
| 9 | `PREDELAY` | continuous 0..1 | PRE-DLY | 0.0 | yes |
| 10 | `SHAPE` | continuous 0..1 | the burst's curve: GATE SHAPE, REVERSE CURVE | 0.2 | yes |
| 11 | `WOBBLE` | continuous 0..1 | the LFO's depth | 0.0 | yes |
| 12 | `RATE` | continuous 0..1 | the LFO's rate | 0.5 | yes |
| 13 | `SYNC` | Enum, `RevSync` | OFF, TIME, ALL | OFF | yes |

- **Count.** That is 13 ids of the 32.
- **Slot 3's step.** Its static step becomes 1/128, so GATE LENGTH moves about 2 % per tick, not 9 %. QUAD's panel keeps its 1/31 tick (§ Pages).
- **Typed choices.** `ReverbType` and `RevSync` are enums with explicit `DiskCode` matches, never discriminants (ADR 0045). `RevSync` is OFF 0, TIME 1, ALL 2. The fixture gains rows `B 22 6 TYPE` … `B 22 13 SYNC` and their `E` rows.
- **Pure mappings.** Every mapping from a knob to a quantity is a pure function of (TYPE, slot value, tempo), in `dsp::reverb` (functional core). The laws are `exp(lo, hi, p) = lo·(hi/lo)^p` and `lin(lo, hi, p)`. The DSP and the UI's value text call the same functions.

### Each TYPE's knobs

Values are p (0..1) → quantity. The main page holds cells 2–5, and REV › MORE is the sub-page (§ Pages). "—" means absent.

**ROOM** (main: SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SIZE | 3 | exp | loop 100–200 ms | 0.5 → 141 ms |
| DECAY | 1 | exp | RT60 0.2–1.5 s (floor `t_min`) | 0.5 → 0.55 s |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0.3 → 6.6 kHz |
| DIFFUSE | 8 | lin | diffuser (f32) and stage AP g together, 0.5–0.75 (§ Level plan) | 0.4 → 0.60 |
| PRE-DLY | 9 | lin | 0–50 ms | 0.1 → 5 ms |
| WOBBLE | 11 | lin | ±0–1 ms, capped at 0.72 % of the loop (§ Pitch bounds) | 0.3 → ±0.3 ms |
| RATE | 12 | exp | 0.1–1 Hz | 0.5 → 0.32 Hz |

**HALL** (main: SIZE · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · DAMP · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SIZE | 3 | exp | loop 120–600 ms; 0–0.15 is the plate zone | 0.65 → 342 ms |
| DECAY | 1 | exp | RT60 0.8–10 s (floor `t_min`) | 0.45 → 2.5 s |
| TONE | 7 | exp | input fc 10 → 2.5 kHz | 0.3 → 6.6 kHz |
| DIFFUSE | 8 | lin | diffuser (f32) and stage AP g together, 0.5–0.75 | 0.4 → 0.60 |
| DAMP | 2 | exp | in-loop fc 11 → 1.5 kHz (QUAD's `damp_coef` law) | 0.3 → 6.1 kHz |
| PRE-DLY | 9 | lin | 0–125 ms | 0.15 → 19 ms |
| WOBBLE | 11 | lin | ±0–2 ms, capped at 0.51 % of the loop | 0.35 → ±0.7 ms |
| RATE | 12 | exp | 0.1–1.5 Hz | 0.4 → 0.30 Hz |

The plate corner is SIZE 0 (120 ms), DECAY 0.35 (1.9 s), TONE 0 (10 kHz), DIFFUSE 1 (0.75), DAMP 0.1, PRE-DLY 0, WOBBLE 0.5 (±1 ms set, ±0.62 ms played under the cap) and RATE 0.6 (0.51 Hz).

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
| WOBBLE | 11 | lin | ±0–1 ms (decorrelators) | 0 → off |
| RATE | 12 | exp | 0.1–2 Hz | 0.5 → 0.45 Hz |

**BLOOM** (main: SWELL · DECAY · TONE · DIFFUSE; MORE: PRE-DLY · — · WOBBLE · RATE · SYNC · GRIT)

| Knob | Slot | Law | Range | Default p → value |
|---|---|---|---|---|
| SWELL | 3 | exp | loop 250–700 ms (time to peak about 0.3–0.75 s) | 0.7 → 514 ms |
| DECAY | 1 | exp | RT60 2–12 s | 0.6 → 5.9 s |
| TONE | 7 | exp | in-loop fc 10 → 1.5 kHz | 0.35 → 5.1 kHz |
| DIFFUSE | 8 | lin | loop AP g 0.45–0.65 (normalised states, § Level plan) | 0.5 → 0.55 |
| PRE-DLY | 9 | lin | 0–125 ms | 0 → 0 ms |
| WOBBLE | 11 | lin | ±0–2 ms, capped at 0.72 % of the loop | 0.3 → ±0.6 ms |
| RATE | 12 | exp | 0.05–0.5 Hz | 0.4 → 0.13 Hz |

**QUAD** (main: SIZE · TIME · DAMP · GRIT; MORE: all absent). Its ids, ranges and laws are today's (ADR 0028). GRIT sits on the main page for QUAD only, because QUAD has no MORE knobs.

### A TYPE change loads that TYPE's knobs

The issue's naming ruling (owner, 2026-10-03): "Each TYPE is an engine … plus its tuned settings. The existing knobs … still adjust from that starting point." So:
- **What a TYPE change writes.** Turning TYPE in the UI writes the new TYPE's knobs, in one edit, into slots 1, 2, 3, 7, 8, 9, 10, 11 and 12.
- **What it keeps.** MIX, GRIT and SYNC are kept: they are the player's level, grain and clock.
- **Who calls it.** `ReverbParams::with_type(t, memory) -> ReverbParams` is the pure function, and the UI's TYPE edit calls it. `Block::write`, decode and MIDI-CC writes of TYPE only store the TYPE, so a project or Sound load keeps its stored knobs.
- **An accidental nudge does not wipe the tweaks.** `UiState` keeps each TYPE's last knobs in RAM: 6 TYPEs × 9 slots as `u16` (the stored 0..1 at 1/65,535), 108 B.
  - Leaving a TYPE stores its knobs there, and coming back restores them. A TYPE not yet visited gets its starting knobs (the tables above).
  - The memory is never saved to the card: a project stores only the current TYPE's knobs. A project load clears it.
  - 108 B fits `UiState`'s guard of 5,120 B (ADR 0082, on the ORBIT branch, where `UiState` reads 3,832 B).

## Switching TYPE

The audio thread runs one engine. When `params.reverb.kind` differs from the one running, this sequence starts (`BLOCK_SIZE` 64, 48 kHz):

| Phase | Blocks | Engine | Return |
|---|---|---|---|
| FADE OUT | 15 (20 ms) | the old TYPE runs on its own lines, with its **latched** knobs | its return × a linear ramp 1 → 0, applied after the interpolators |
| CLEAR | 1 (1.3 ms) | none runs | exactly 0 |
| FADE IN | 15 (20 ms) | the new TYPE, with fresh state and its knobs primed (no crossfade from the old TYPE's lengths) | × a linear ramp 0 → 1 |

- **The latch.** The UI's TYPE edit has already written the new TYPE's knobs into the shared slots (`with_type`). So when the switch starts, the dispatcher copies the running engine's last `ReverbParams`, and FADE OUT runs on that copy. The old engine never plays the new TYPE's values.
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
  - The target follows the newest TYPE. In FADE OUT or CLEAR, the sequence simply continues.
  - In FADE IN, a new FADE OUT starts from the current ramp value, never from 1. Its latch is taken from the knobs that engine is running.
  - Scrolling through TYPEs plays silence, then the last TYPE.
- **Under the switch.**
  - A project swap (ADR 0046) carrying another TYPE uses the same path, latch included.
  - At MIX 0 the switch still runs, silently (ADR 0061).
- **No stale echo.** After the switch, the engine is bit for bit a fresh engine fed the same send from the first FADE IN block. A test asserts this (§ Tests).

## Smoothing

- **Continuous knobs.** Every continuous knob eases:
  - lengths by pitch-free crossfades (§ Moving a length);
  - gains, TONE, DAMP, DIFFUSE, SHAPE and the curve weights by a per-block one-pole ramped across the block: τ 50 ms for DECAY (as QUAD's TIME), 20 ms for the rest;
  - GRIT as in QUAD (20 ms);
  - MIX by `Ease` (ADR 0061).
- **Discrete choices.** SYNC changes a length's target, which crossfades. TYPE runs the switch.
- **Nothing snaps** except a fresh engine's priming inside FADE IN, which no one can hear move.

## Wobble

The owner asked for an LFO wobble on the loops. The MV2's reverbs were static, which is why its small bright rooms ring metallic.
- Every new TYPE has WOBBLE (depth) and RATE.
- It is on by default in ROOM, HALL and BLOOM, and off in GATE and REVERSE.
- Which reads it moves:
  - ROOM: the APs of stages A2 and B1;
  - HALL: the APs of stages 1 and 3;
  - BLOOM: loop APs 3, 6 and 9;
  - GATE and REVERSE: the second decorrelator AP per side.
- Its pitch is bounded: ≤ 25 cents, with no drift (§ Pitch bounds).
- It is the TYPEs' only modulation (§ Modulation). Its cost is in § CPU budget.

## Modulation (deferred to #345)

Owner, 2026-10-04: every mod source today lives inside a Part, and nothing exists at the mixer level. A mixer-level LFO or matrix gets its own thought before anything routes into shared FX (private issue #345).
- **This build has no external mod destinations.** Every reverb spec stays `modulatable: false`. The only motion is each TYPE's own wobble (§ Wobble), set by its WOBBLE and RATE knobs.
- **Out of scope, deferred to #345:** Part-matrix routes into the reverb, an envelope follower or DUCK on the send, and MIX+PLUS priming on reverb cells.
- **What #345 will have to change:**
  - **Disk: nothing.** Routes are stored by `ParamAddr` (block and id), and the ids exist.
  - **ADR 0010.** It makes only params that `Voice` reads per block modulatable, and the reverb runs on the bus, outside every `Voice`. Whatever #345 decides must supersede ADR 0010 for bus destinations.
  - **What a destination means.** The slots are semantic: slot 3 is SIZE under ROOM and LENGTH under GATE, so a route to slot 3 changes meaning with TYPE. #345 must decide whether a destination is the slot, labelled per TYPE, or a per-TYPE address.
  - **Pitch.** A modulated length would have to move by crossfade too, to stay within § Pitch bounds.
- **No slot is reserved.** Every knob maps through one pure function of its stored value, so an offset can later be added before the mapping without touching storage.

## Tempo sync

- **Source.** The ORBIT clock (ADR 0071, 0072, 0080): the internal BPM, or the leading MIDI clock's average under SYNC EXT. The runner already has it on the audio thread each block.
  - `FxBus::process` takes `Tempo`, a newtype of samples per beat.
  - **Quantised.** The reverb takes the tempo in steps of 0.25 BPM, and moves to a new step only when the clock is 0.15 BPM past the current one. A MIDI clock's average jitters, and every tempo move retargets the synced lengths.
  - Until ORBIT is on `main`, `Tempo` is 120 BPM and SYNC is hidden. The tempo step in § Build order waits for ORBIT.
- **SYNC** (slot 13, MORE page): OFF, TIME or ALL.

| SYNC | HALL, BLOOM | ROOM | GATE, REVERSE | QUAD |
|---|---|---|---|---|
| OFF | ms | ms | ms | — |
| TIME | PRE-DLY in note values | PRE-DLY in note values | LENGTH in note values | — |
| ALL | as TIME, and DECAY in beats | not offered: the cell steps OFF ↔ TIME, and a stored ALL reads as TIME | as TIME | — |

ROOM has no ALL, because 7 of the 10 beat values would clamp to its 1.5 s maximum.

- **Note values.** A synced knob walks a list of divisions, one encoder tick each; the panel gives the cell its step. Each TYPE's list is the divisions that fall inside its ms range at 120 BPM:

| Knob | Divisions (at 120 BPM: ms) |
|---|---|
| ROOM PRE-DLY | 0, 1/128 (16), 1/64T (21), 1/64 (31), 1/32T (42) |
| HALL, BLOOM PRE-DLY | 0, 1/128, 1/64T, 1/64, 1/32T, 1/32 (63), 1/16T (83), 1/32D (94), 1/16 (125) |
| GATE LENGTH | 1/32T (42), 1/32, 1/16T, 1/32D, 1/16, 1/8T (167), 1/16D (188), 1/8 (250), 1/4T (333), 1/8D (375), 1/4 (500) |
| REVERSE LENGTH | 1/8T (167), 1/16D, 1/8, 1/4T, 1/8D, 1/4 (500), 1/2T (667), 1/4D (750) |
| HALL, BLOOM DECAY (ALL) | 1, 2, 3, 4, 6, 8, 12, 16, 24 or 32 beats |

- **How a division is stored.** In the slot's 0..1 value: for a list of n divisions, division i is stored as `p = i/(n − 1)` and read back as `i = round(p·(n − 1))`.
- **Toggling SYNC.** A UI edit of SYNC converts the slot so the sound stays close, by a pure function beside `with_type`:
  - OFF → TIME or ALL stores the division nearest the current ms at the current tempo;
  - TIME or ALL → OFF stores the p whose ms equals the division that was playing.

  Decode and CC writes store SYNC alone, and then the slot is simply read the new way. The target changes, and the length crossfades to it.
- **Out of range.**
  - A synced time longer than its TYPE's maximum at the current tempo **halves until it fits**; one shorter than the minimum **doubles**. It stays on the grid: GATE's 1/4 at 90 BPM (667 ms) plays 1/8.
  - **The fold has 2 % hysteresis.** A fold state changes only when the unfolded time is 2 % past the limit, inside or out. At 100 BPM, GATE's 1/4 is 600 ms, exactly its maximum; without hysteresis, clock jitter would flip it between 600 and 300 ms.
  - A beat DECAY outside its TYPE's seconds range is **clamped**, since a decay has no grid to stay on.
  - The cell's value text shows the division that plays. A fold marks it with 0086's ▾ (halved) or ▴ (doubled), as in `▾1/8`.
  - The arena never grows for a slow tempo: folding stays on the grid instead.
- **A tempo change** moves the targets, and the lengths crossfade to them, pitch-free (§ Moving a length).
- **The fold** is a pure function, `fold(division, tempo, lo, hi, state) -> (Samples, state)`, with its own unit tests.

## UI

### Pages

- **REV** (`EFX`, mixer chain, unchanged id 16): TYPE · knob 1 · knob 2 · knob 3 · knob 4 · MIX.
  - Cells 2–5 come from the TYPE's panel.
  - MIX stays last on every TYPE, so the level is always in the same place.
- **REV › MORE** is a new sub-page, short `MORE`, made with `ChainBlock::with_subs` as DLY › CHAR is.
  - Its cells, by panel: PRE-DLY · DAMP, SHAPE or CURVE · WOBBLE · RATE · SYNC · GRIT.
  - Under QUAD every MORE cell is absent.
- **How the cells look.**
  - An absent cell draws the dash, and a dimmed one the dim look; `renderer::look` decides, as for the filter (ADR 0037).
  - A cell's label, value text and **encoder step** come from the panel:
    - units such as `141ms`, `2.5s`, `6.6k`, `0.30Hz` and `1/8`, from the same pure mappings the DSP uses;
    - slot 3's static step is 1/128, and QUAD's panel overrides it to 1/31, so QUAD's SIZE still moves one table step per tick;
    - a synced knob steps one division per tick.
  - **MIDI MAP** (the per-Part CC map) shows the current TYPE's panel label for a reverb slot: a CC on slot 3 reads SIZE under ROOM and LENGTH under GATE. It never shows the static spec's label.
  - **Short forms** of ≤ 6 characters stand in when a label outruns its bar (ADR 0053's MECHANICS → MECH rule): PRE-DLY → `PRE`, DENSITY → `DENS`, DIFFUSE → `DIFF`. The focus band shows the full word.
- **The demo page.** `DEMO_GLYPH_CUBE` stays bound to QUAD's slots.

### TYPE and the words

- TYPE is a word choice, so it takes NONE (ADR 0067), and so does SYNC.
- The existing test that every NONE word fits the band at the focus size covers `REVERSE`.
- QUAD keeps the word `QUAD` on screen: the owner named it (2026-10-04).

### Glyph

The cube (ADR 0067) is QUAD's picture of a room. For the new TYPEs a cube is the wrong picture, and the owner asked to keep it cheap. So:
- **One composite, two drawings.** `CompositeId::ReverbCube` becomes `CompositeId::Reverb`.
  - The renderer's composite callback reads TYPE, and draws the **cube** under QUAD and the **TAIL** under the others.
  - Every reverb param keeps one glyph on its spec, so ADR 0067's "no per-slot override" rule holds.
  - This amends 0067's "the reverb shows the cube", and ADR 0085 records it.
- **TAIL** is the TYPE's energy envelope, drawn as a 60-column polyline in the cube's 64 × 56 box.
  - It comes from set values only, by a pure `tail_shape(type, knobs) -> [u8; 60]`:
    - ROOM, HALL: a pre-delay gap, then an exponential fall;
    - GATE: a plateau and a cliff, sloped by SHAPE, plus the TAIL;
    - REVERSE: a rising ramp that cuts dead, with echoes of it under REGEN;
    - BLOOM: a swell to its peak, then the long fall.
  - A playhead dot crosses it at the TYPE's real time scale, on the UI clock.
  - The focused knob's part of the shape is drawn in the accent.
  - The x axis spans the TYPE's own range (ROOM 1.5 s, HALL 10 s, BLOOM 12 s, GATE 1.4 s, REVERSE 2 s), so a turn always moves the shape.
- **Cost.** One polyline and a dot per frame, inside the box rules ADR 0067 set: only its box is cleared and flushed while it animates.

### Hints

Hints follow ADR **0086** ("Every gesture shows a contextual hint in the focus band"). 0086 is Proposed, on the ORBIT branch, and not yet on `main`, so this section depends on it.
- **0086's model.**
  - One `ui::hint` component draws static `Gesture`s at baseline 115, under the focus band's value.
  - Each user can turn hints off.
  - A test holds every hint within 216 px.
  - A hint exists for a *gesture* on the touched cell, and is never an always-on line.
- **The reverb cells carry no gesture.** Every new control is a knob, and navigation stays the D-pad (ADR 0081). Modulation is deferred (§ Modulation), so MIX+PLUS priming does not reach these cells. So under 0086 no reverb cell has a hint, and `View::hint` returns none for them.
- **Where informational text goes instead.**
  - A folded synced time shows in the value text, with 0086's ▾/▴ mark (§ Tempo sync).
  - The plate is one knob away (HALL's SIZE plate zone), and its value text says `PLATE`.
- **If 0086 changes or never lands,** this spec still needs no hint: there is no reverb gesture to explain. A later gesture on a reverb cell (#345's priming) brings its static `Gesture` with it.

## CPU budget

### Target and hard limit ("sound first")

The owner, 2026-10-04: **"sound first."**

- **The target** is QUAD's worst case: 485 cycles per output sample on the REVERB row (ADR 0031, 2026-09-27; re-measured at step 0). Every TYPE is designed to it, and by the estimates below they all meet it.
- **The hard limit is the cost model's headroom.**
  - **The budget.** The audio thread may use **70 % of the CPU**: `AUDIO_BUDGET_PERCENT` = 70 in `chimera-core/src/hw.rs` ("30% is left for UI, MIDI and interrupt overhead"). `SampleBudget::for_cpu` builds it from the real clock (ADR 0020): **7,000 cycles per sample on rev V** (480 MHz) and **5,833 on rev Y** (400 MHz).
  - **Headroom** is how far `FxBus::COST` can rise before any voice-count assertion in `cost_test` fails.
    - It is the **minimum**, over every one of those assertions, of `budget − FxBus::COST − n × voice`.
    - It is taken on both chip revisions and both builds, default and `master-tape`.
    - A voice is `CHAIN_COST` + `ModRouting::BASE` (47) + the engine's cost, at LP24.

  From the code on 2026-10-04:

| Assertion in `cost_test` | Voice | Rev V (default / tape) | Rev Y (default / tape) |
|---|---|---|---|
| A16 ∪ A17 (`the_costliest_patch_gets_six_voices_on_rev_v`) | 909 | 6 voices: 366 / 56 | 5 / 4: 108 / 707 |
| A16 ∪ A17 + FOLD | 954 | 6 / 5: **96** / 740 | 4 / 4: 837 / 527 |
| A16 ∪ A17 + FOLD + DRIVE | 1,011 | 5 / 5: 765 / 455 | 4 / 4: 609 / 299 |
| TX factory Sounds (`every_factory_sound…`), up to | 712 | 8 / 7: 124 / 526 | ≥ 5: 1,093 / 783 |
| MORPH KEYS | 864 | 6 / 6: 636 / 326 | ≥ 5: 333 / **23** |
| MORPH PAD | 853 | 6 / 6: 702 / 392 | ≥ 5: 388 / 78 |

  - **The figures.** The headroom is **96 cycles per sample in the default build** (FOLD on rev V: 5,820 − 6 × 954), and **23 with `master-tape`** (MORPH KEYS on rev Y: 4,343 − 5 × 864). The factory rows use the TX Sounds' highest cost, 712; the derived test below uses every Sound's real cost.
  - **The gate: ΔCOST ≤ headroom.** ΔCOST is how far the costliest TYPE's BUS row reads above QUAD's, on the same build.
  - **The gate is `cost_test` itself.** There is no separate headroom test, so nothing duplicates or halves the margin.
    - A TYPE that reads over QUAD sets `FxBus::COST` to the costliest TYPE's BUS reading, and updates `cost_test`'s `bus` literal (1,180, or 1,490 with the tape) to match.
    - `cost_test` then runs unchanged, on both builds. Its voice-count assertions are the gate: if any fails, ΔCOST exceeded the headroom.
    - The headroom table above only explains where that line falls.
  - **If re-baselining eats the tape's 23.** Step 0 re-measures QUAD's BUS row with voice traffic. If that reading exceeds today's `COST`, `COST` rises first, as a QUAD-only change recorded like ADR 0031's readings.
    - If that alone fails a `master-tape` voice assertion, it goes to the owner before any TYPE work: either the tape build's guarantee is re-ruled, or every TYPE must be ≤ QUAD (ΔCOST ≤ 0) in the tape build.
    - The same applies to the default build's 96, though there it is unlikely.
  - **"Sound first" has little room.** 96 cycles (23 with the tape) is about one moving pair per ring sample.
    - A TYPE costing more than QUAD by more than that needs either a cheaper design, or an owner ruling that gives up a voice guarantee in `cost_test` (FOLD's sixth voice on rev V, for example).
    - Each "if the A/B asks for it" item in this spec says whether it fits.
  - **The stress run is a second, real gate.** On the unit, with the TYPE under test, it runs:
    - six admitted voices of A16 ∪ A17;
    - every FX at its bench worst;
    - ORBIT running all five rings;
    - the UI editing, with an animated glyph in focus.

    It must show OVER 0, DROPS 0, and a peak block load under 100 %. The bill can pass while the chip fails: ADR 0040's run peaked at 87 % on cache effects the bill does not model.
  - **Host instruction counts** (ADR 0031's trace) are design estimates only, never a gate. ADR 0013 rejects emulation for budgets.
- **A TYPE may exceed the target** when the A/B shows its cheaper design sounds wrong, provided its ΔCOST fits the headroom (96, or 23 with the tape) and the stress run passes.
- **The QUAD regression check stays.** After step 1a, REV QUAD must read ≤ step 0's baseline + 1 %. QUAD's cost is unchanged by ruling, and the dispatcher must not hide a regression.
- **Why re-measure.** Since 2026-09-27 the bench subtracts the MIX row, and MIX now includes the limiter.

### The model

QUAD measured 158 instructions fixed and 239 crossfading, for 30 and 48 accesses: about **5.0 instructions per fixed access**, everything around it included.
- **A fixed access** is QUAD's kind: one load or store at a segment offset. A crossfading line adds one fixed read and a blend (about 2 instructions).
- **A moving pair**, the wobble's only, costs two loads plus the per-sample index work: integer part, fraction, the wrap of both slots and the lerp. That work is estimated at **+7** (this spec's estimate) to **+10** (the review's), so 17–20 instructions a pair. Step 0 measures it.
- **On the chip,** cycles per output sample ≈ 40 + 1.86 × instructions per ring sample (QUAD: 40 + 1.86 × 239 = 485).

Worst case: every crossfadable line crossfading, wobble at its maximum.

| TYPE | Fixed accesses, worst (writes + reads + crossfade reads) | Moving pairs | Blends and extras | Instructions, pair +7 / +10 | Cycles, +7 / +10 |
|---|---|---|---|---|---|
| QUAD | 30 steady / 48 crossfading | 0 | — | 158 / **239 measured** | 320 / **485 measured** |
| ROOM | 13 + 17 + 5 (PRE, 4 delays) = 35 | 2 (wobbled APs) | 10 + 4 (rotation) + 12 (2 soft clips) + 8 (4 normalised APs) | **243 / 249** | 492 / 503 |
| HALL | 13 + 17 + 5 = 35 | 2 | 10 + 6 (soft clip) + 8 (4 normalised APs) | **233 / 239** | 473 / 485 |
| GATE, REVERSE | 9 + 17 + 4 (crossfading taps) = 30 | 2 (wobbled decorrelator APs) | 8 + 12 (2 soft clips) + 3 (TAIL's normalised AP, Σ scale) | **207 / 213** | 425 / 436 |
| BLOOM | 16 + 13 + 2 (PRE, SWELL) = 31 | 3 (wobbled loop APs) | 4 + 6 (soft clip) + 20 (10 normalised APs) | **236 / 245** | 479 / 496 |

- **Where TYPEs exceed QUAD.**
  - ROOM is over QUAD at either pair cost: about +7 / +18 cycles.
  - BLOOM is over at +10: about +11.
  - HALL is at QUAD.
  - All of these fit both headrooms, 96 and 23. This is "sound first": the level plan's clips and normalised states buy QUAD's noise floor at full input gain.
- **Why it is cheaper than the earlier draft.** Crossfades replaced every moving read except the wobble's. A crossfading line costs 5 + 2 instructions only while it moves; a glide's moving pair cost 17–20 all the time.
- **The tightest is ROOM**, at 249 instructions (503 cycles, ΔCOST ≈ +18 of the tape build's 23) if a pair costs +10. Step 0 benches it.
- **What fits if the A/B asks for it:**
  - BLOOM's split SWELL delay (+10 instructions, ≈ +19 cycles). It takes BLOOM to about +30 by this table. The review's cache-aware estimate puts BLOOM's base design about **+14** and the split about **+33**:
    - it **fits the default build** (96);
    - in the **`master-tape` build it does not fit** (23). There, the split is not offered unless the owner rules to give up a voice guarantee.
  - 14 BURST taps (+2 fixed reads, +10 instructions): **fits**.
  - SIZE-scaled LOOP taps (+6 crossfading reads, +42 instructions, ≈ +78 cycles, taking ROOM to about 581, ΔCOST ≈ +96): this is **at the default build's limit** (96) and over `master-tape`'s (23). It needs an owner ruling in either build.
- **Fallbacks are a last resort**, not the plan ("sound first").
  - A fallback is taken only when ΔCOST would breach the headroom or the stress run fails, or when the A/B shows the cheaper design sounds the same.
  - A TYPE over the 485 target alone is no reason to take one.
  - Each is a constant in the engine and changes a golden, so the owner hears it at A/B.

| TYPE | Fallback design | Saving (+7 / +10) |
|---|---|---|
| ROOM | **ROOM-T** (below): wobble loop B's AP only, then 2 output taps a side | 12 / 15, then 10 more |
| HALL | Wobble one stage AP (stage 3) instead of two | 12 / 15 |
| GATE, REVERSE | 3 crossfading taps at a time, not 4 (a 250 → 500 ms move takes about 0.53 s) | 7 |
| BLOOM | Wobble 2 APs (3 and 9), not 3 | 12 / 15 |

- **ROOM-T, the pre-written tape-build fallback.**
  - ROOM sits at about +18 cycles of the tape build's 23 headroom by this table. A pessimistic estimate puts it near +43. **Step 0's bench of the ROOM kernel decides.**
  - If the kernel's ΔCOST exceeds 23, ROOM takes ROOM-T before any TYPE work, a constant change and not a redesign:
    1. **Wobble loop B's AP only** (B1's AP). Loop A's AP becomes a fixed read. This saves 12 / 15 instructions, about 22–28 cycles. Loop A stays static, B still moves, so the tail still drifts apart L/R.
    2. **If that is not enough, 2 output taps a side**: drop (A2, 0.83) on the left and (B2, 0.79) on the right, and keep the early tap and one mid tap. That saves 10 more instructions, about 19 cycles.
  - Together these save about 41–47 cycles, which covers the pessimistic +43.
  - ROOM-T applies to both builds, so ROOM sounds the same everywhere, and its golden is re-recorded. The A/B hears it.
- **The recount.**
  - Normalising ROOM's and HALL's 4 diffuser APs would have cost about +8 instructions. R3's f32 diffusers cost +0, since an f32 store replaces the quantise-and-store. So the table above stands.
  - Restoring in-loop g to 0.75 changes no instruction count.
- **Per-sample maths** besides the accesses: tone and dampers (≤ 3 one-poles), gains, the grid quantiser and the blends. They are in the 5.0 per access, as they are in QUAD's. The LFO, gain ramps, crossfade setup and tap scheduler run per block.
- **`FxBus::COST`.** It does not change while every TYPE ≤ QUAD. The bench's BUS row runs the costliest TYPE's worst settings, chosen by name in `bench.rs` from the REV rows. A TYPE over QUAD raises `COST` to the new BUS reading, and `cost_test`'s voice counts check it.

### Bench rows

- **REV QUAD.** The REVERB row on the FX screen stays, as REV QUAD (today's `worst_reverb`).
- **The new rows.** A new **REVERB bench screen** carries the five new rows, three to a line in today's FX layout, so nothing falls past y 320. The console's `bench` (ADR 0068) prints every row.
- **Each row runs at its worst:**
  - **ROOM** and **HALL**: SIZE sweeping between 0 and 1 without pause, so a crossfade always runs, as in a long move; DECAY 1; WOBBLE 1, RATE 1; GRIT 1; MIX 0.5;
  - **GATE** and **RVRS**: LENGTH alternating, so 4 taps always crossfade; DENSITY 1; TAIL or REGEN 1; WOBBLE 1;
  - **BLOOM**: SWELL and PRE-DLY alternating; DECAY 1; WOBBLE 1.
- **The gates are absolute.**
  - **ΔCOST ≤ headroom** (§ Target and hard limit), enforced by `cost_test`'s voice counts with `COST` set to the costliest TYPE's BUS reading, as each new TYPE lands. The stress run on the unit is the second gate.
  - **Each REV row** is recorded against the 485 target. A row over the target is reported, and is allowed when the A/B justifies it and the headroom holds.
- **REV SWITCH MAX** is an on-chip row that records the *maximum* block, not the mean.
  - It runs six A16 ∪ A17 voices while TYPE is forced to switch every 40 blocks (QUAD → HALL → GATE → …), so FADE OUT, CLEAR, the refill after it and FADE IN all land inside the window.
  - A mean over 64 blocks would hide the one CLEAR block.
  - **Gate:** the maximum whole-callback block stays within `BlockBudget::budget_cycles`, 70 % of the 64-sample block.
- **Cache.** The REV rows and BUS run with voice traffic between blocks: six A16 ∪ A17 voices play, so the reverb's streams start cold, as they do in use.
  - The stream counts differ: QUAD 48, the new TYPEs 30–37.
  - Each stream misses again after the voices evict it, about 9 cycles per output sample.
- **Records.** The plan records every row's reading, the build and the date, as ADR 0031 did.

## Flash

- **Method.** Release `text`, fat LTO and one codegen unit, on a `git archive` copy: ADR 0083's method, on the ORBIT branch. Thin LTO moves about ±1 KB between builds of the same code.
- **Gate.** Each step's plan task records its delta. A TYPE over 3 KB is cut back, its own code shared or moved out of line, before the next TYPE starts.

| Step | Adds | Estimate |
|---|---|---|
| Scaffold | dispatcher, switch, arena split, crossfade and read kinds, allpass, tone, LFO, panels, MORE page, TAIL glyph, per-TYPE memory | 3–4 KB |
| GATE + REVERSE | one `BurstEngine` with its tap scheduler, two panel rows, tap tables | 2–2.5 KB (1–1.3 KB per TYPE) |
| ROOM | `LoopEngine<ROOM>`, panel row, layout | 1.5–2 KB |
| HALL | `LoopEngine<HALL>`, panel row, layout, plate zone | 1.6–2.2 KB |
| BLOOM | `BloomEngine`, panel row | 1–1.5 KB |
| Tempo sync | division lists, fold, SYNC conversion | ≈ 0.5 KB |
| **Total** | | **≈ 9.5–12.5 KB** |

Every TYPE stays inside 1–3 KB. The memory map gives the firmware 896 KB of flash, and ORBIT's builds read about 715 KB.

## Storage

- **Old projects.**
  - A project saved before this work has a Reverb block with ids 1–5 and no id 6. Decode leaves an absent id at its target's value.
  - A load decodes into a fresh `Project` whose reverb is `ReverbParams::default()`, so TYPE is QUAD: **every existing project loads as QUAD**, and sounds as it did.
  - Ids 7–13 also take their spec defaults, and QUAD ignores them.
- **New projects** default to QUAD as well: one rule, and today's sound is the starting point.
- **A future TYPE code on this firmware**, a file from a newer build, is an unknown code. The value keeps the target's, QUAD (`set_enum_code` returns false), so it loads safely.
- **`FxParams`** grows by about 26 B: six more f32 (TONE, DIFFUSE, PREDELAY, SHAPE, WOBBLE, RATE) and two enum bytes. It sits in `Performance`, in the triple buffer (ADR 0021); `memory_budget_test` re-pins it.

## Tests

The usual battery, per TYPE, on the host first (owner's rule: QA before UAT).

### Refactor lock (QUAD)

- `fx_golden_test`'s reverb cases, `reverb_test`, `gain_staging_test`, `fx_bus_test` and `mixer_page_test`'s SIZE nudge (+1/31) pass unchanged, bit for bit, with QUAD on the shared arena.
- That proves the arena move, the dispatcher and slot 3's new 1/128 static step changed nothing. `size_step` still rounds to 32 steps, and the panel keeps QUAD's tick at 1/31.

### Pitch (the owner's per-pass test)

- **Moves are pitch-free.** For each loop (ROOM, HALL, BLOOM), with DECAY at its maximum, dampers open and WOBBLE 0, a 1 kHz sine circulates through a full SIZE (or SWELL) sweep, then a PRE-DLY sweep, then a tempo step.
  - The pitch is measured from zero crossings, once per pass.
  - **The pass:** once the move has settled (the last crossfade done, plus one round trip), the tail's spectral peak sits within **1/L Hz** of the input's 1 kHz, where L is the new round trip in seconds. That is the loop's own mode spacing.
  - **Not judged, only reported:** the in-crossfade deviation, up to about 29 cents instantaneously and about 7 cents averaged over a 120 ms pass.
- **REGEN is pitch-free.** The same check, with a 1 kHz sine regenerating through REVERSE at REGEN 1 during a LENGTH move from 150 to 800 ms. Once settled, the spectral peak sits within 1/L of 1 kHz, with L the new LENGTH.
- **No slap, no cancellation.** After a full SIZE sweep (and a full LENGTH move under REGEN), a noise-fed tail is compared with a static tail at the end setting.
  - Normalised autocorrelation of the moved tail over lags of 1–10 ms (the step slice's range) must show **no peak above −20 dB**, and no lag more than 3 dB above the static tail's.
  - The short-time energy (10 ms windows) must never dip more than 3 dB below the static tail's envelope trend during the move.
- **The wobble is bounded.** At WOBBLE 1 and RATE 1, at each TYPE's smallest SIZE, the circulating sine's pitch stays within **±25 cents on every pass**. Its mean over each LFO cycle stays within ±1 cent, so there is no drift.

### Per engine (unit)

- **Layout.** Each TYPE's layout is distinct primes that fit, and is ≤ 23,220 (`const` assert plus test).
- **Reads.**
  - Every computed read length is ≥ 2 (except BURST tap 0, which is not a read), across every knob's ends and WOBBLE 1.
  - A moving pair at fraction 0 equals the fixed read; at 0.5 it is the mean; and it is correct across the wrap.
- **Crossfades.**
  - Every length lands on its target.
  - One LOOP crossfade moves the round trip, and PRE-DLY, by at most 5 ms, with every stage delay switching in the same crossfade.
  - The BURST scheduler never crossfades more than 4 taps at once, never moves a tap more than 120 samples in one crossfade, and brings every tap to its target.
  - The wobble cap holds at every SIZE.
- **Mappings.** Each pure mapping hits its table's endpoints and defaults to 1e-3; `exp` and `lin` are monotonic.
- **Gains and floor.**
  - Stage gains are ≤ 0.985 for the new TYPEs and ≤ 0.97 for QUAD.
  - HALL reaches its 10 s target at SIZE 0.
  - RT60's floor `t_min` behaves as QUAD's.
- **ROOM's rotation** preserves energy: with the stage gains at 1 and the dampers off, the loop pair's energy is constant to 1e-6 over 10 round trips.
- **BURST.**
  - Weights are normalised: Σw² is constant within 0.1 dB across SHAPE, CURVE and DENSITY.
  - DENSITY fades taps in continuously: no weight jumps more than 1/64 per 1/128 of the knob.
- **The plate zone.** At SIZE 0 the effective DIFFUSE, TONE and PRE-DLY are the plate's, above 0.15 they equal the stored knobs, and the change between is continuous.
- **Sync.**
  - `fold` halves and doubles to fit across 30–300 BPM, and every result is in its TYPE's range.
  - A clock jittering ±1 % around a fold boundary (100 BPM, GATE 1/4) never flips the fold.
  - The tempo quantiser holds through ±0.1 BPM of jitter.
  - DECAY in beats clamps, and ROOM never offers ALL.
  - The division encoding round-trips, and toggling SYNC on and off keeps the length within one division.
  - Each division list's 120 BPM values are inside the ms range.
- **Knob memory.** A TYPE change and back restores the knobs exactly, at the `u16` resolution. A project load clears the memory.
- **The `EngineState` zero test** (§ Arena).

### Sound (render)

- **Stable and silent.** At GRIT 0 and 1, every TYPE's energy never grows after the send stops.
  - The return reaches exact zero within **1.5 × the TYPE's longest RT60 at the tested settings**. REVERSE at REGEN 1 and LENGTH 800 ms has an RT60 of about 16 s, so its deadline is 24 s.
  - Long tests run the engine alone at 24 kHz, not the whole bus.
  - REVERSE at REGEN 1 must reach zero with no ±Δ hum or DC, from its truncating write.
  - These follow `falls_silent_within` and `no_limit_cycle_at_grit_*`, which test QUAD.
- **RT60 tracks DECAY.** It is measured on a 0.5 s noise burst (Schroeder EDC, as `rt60_follows_time`), at p = 0, 0.25, 0.5, 0.75 and 1, and must be within ±35 % of the target at GRIT 0. That is ADR 0028's tolerance and method. ROOM at DECAY 0 (0.2 s) checks the rotation.
- **The spike's windows (±30 %).**
  - ROOM at defaults: RT60 0.55 s, and onset ≤ 34 ms.
  - HALL at defaults: L/R correlation |ρ| ≤ 0.2.
  - HALL plate corner: onset ≤ 8 ms, measured as in the A/B (1 ms RMS above −40 dB relative to the peak).
  - GATE at 177 ms: the −20 dB end falls between 185 and 660 ms. There is a cliff: at TAIL 0, the energy in the 50 ms after the end is at least 20 dB under the plateau's.
  - REVERSE: the envelope peaks within ±15 % of LENGTH, and |ρ| is between 0.3 and 0.75.
  - BLOOM at defaults: onset 140–300 ms, peak 450–750 ms.
- **Band-limited**, as `the_return_is_band_limited`.
- **Loudness parity.** Each TYPE's default return RMS is within ±1 dB of QUAD's default, on the same noise burst at MIX 0.5. This sets `WET[type]`.
- **Wobble.** At WOBBLE 0, the output equals the output with the LFO frozen. At WOBBLE 1, a short-time spectrum shows the spectral peaks of a sustained tail moving.
- **Crossfades don't click.** A full SIZE, SWELL or LENGTH sweep, and a SYNC 1/8 → 1/4 step mid-tail, keep the max second difference within `click_free_test`'s bound.
- **Hot send.** Every TYPE at its maximum feedback (REGEN 1, DECAY 1, smallest SIZE) takes these stimuli:
  - a full-scale sine swept from 50 Hz to 10 kHz over 10 s. Clamp hits are counted on every line write, and must be ≤ 0.1 % of writes. These are the transients of § Level plan;
  - a full-scale sine **held on a loop mode**: at k / round trip, the k whose frequency is nearest 500 Hz, held for 3 × RT60 so the coherent build-up peaks. The soft clip engages here. The return's **THD must stay ≤ 3 %** (−30 dB), and there must be **zero** i16 clamp hits.
    - **Why 3 %.** It is the level of light tape saturation, and it applies only to a pathological input: a pure tone held exactly on a loop mode for seconds at maximum decay. Anything below it sits under the reverb's own diffusion smear. A tighter bound would need a lower input gain, which costs the noise floor above;
  - a full-scale sine **held on the loop mode nearest 5 kHz**, for 3 × RT60. The energy at non-harmonic frequencies (aliases such as 9 kHz) must stay ≤ **−40 dB** against the fundamental. That is a third of the 3 % THD bound, and below the diffusion smear at that level;
  - a −12 dBFS RMS pink-noise pad held for 10 s, at the **default DECAY**. The soft clip never engages, so the output is **bit-identical** to the same engine built without the clip, with zero clamp hits;
  - the same pad at **maximum DECAY**. The clip shaves peaks, and the return's difference from an unclipped f32 reference must stay ≤ −40 dB, with zero clamp hits;
  - **every bound in § Level plan's table**, as an assertion: the engine records each stored quantity's peak magnitude through all of the stimuli above. Each must stay within its bound, or show only the counted transient clamp hits.
- **Noise floor.** At GRIT 0 and 0.3, each TYPE's quantisation noise is measured as its i16 engine's output minus an f32-line build of the same engine, on the same noise-burst tail at MIX 1. It must lie no more than **4 dB** above QUAD's, measured the same way (§ Level plan's estimates: +2 to +4).
- **Sanitising.** NaN and out-of-range knobs render finite and recover, as QUAD's do.

### Switch

- **All 30 ordered pairs of TYPEs**, mid-tail at MIX 1, through `FxBus`:
  - **no click**: the return's max second difference in the switch window is ≤ its steady neighbours', `click_free_test`'s measure;
  - the return is **exactly 0** in the CLEAR block;
  - **no stale echo**: from the first FADE IN block, the new engine's pre-gain output is bit-identical to a fresh engine's fed the same send;
  - **the latch**: FADE OUT's output is bit-identical to the old engine run on its old knobs, after `with_type` has overwritten the slots;
  - a second switch during FADE IN starts its fade from the current gain.
- **Peak block cost** is the on-chip REV SWITCH MAX row (§ Bench rows), not a host test.
- **A project swap** that changes TYPE takes the same path (`fx_swap_test`).

### Sweeps

- `param_sweep_test` becomes TYPE-aware: under every TYPE, it sweeps every knob that TYPE shows, and skips the absent ones.
- The verdicts are today's: finite, under the ceiling, no DC, no clicks the knob doesn't cause, not silent unless it silences, and never billed over budget.
- `sweep_thorough` adds every TYPE × every SYNC × 30, 120 and 300 BPM.
- Allowed behaviour, each listed with its reason as the sweep's rules require:
  - TYPE's switch dips the return by design;
  - TAIL 0 and REGEN 0 are allowed short tails.

### Goldens

New `fx_golden_test` cases, FNV-hashed, with the same burst and length as today's:
- `rev_room`, `rev_hall`, `rev_hall_plate`, `rev_gate`, `rev_gate_flat_tail0`, `rev_reverse`, `rev_reverse_regen`, `rev_bloom`;
- after the tempo step, with sync: `rev_gate_sync_8th_120`.

They lock refactors, not quality (ADR 0011). Ear tuning re-records them deliberately.

### UI

- **Panels.** Every TYPE's main and MORE labels fit their cells, short forms included. Every unit string fits, and so do folded values (`▾1/8`) and `PLATE`.
- **Hints.** No reverb cell returns a hint (ADR 0086).
- **MIDI MAP** shows each TYPE's panel labels.
- **TAIL.** `tail_shape` is pure, and changes under every knob of its TYPE. Composites draw set values only (ADR 0067's existing test, extended). The box-clear and status tests cover TAIL.
- **Screen goldens** (`screen_golden_test`) for REV and REV › MORE under QUAD, HALL and GATE.

### A/B against the Midiverb VST, for the owner

- **The plugin.** The Midiverb II plugin built from github.com/thement/midiverb_emulator.
  - The owner builds it, and the commit hash goes into the A/B report's header before the first render.
  - If the owner uses a different Midiverb VST, its name and version replace this line first.
  - It is run, never linked into or copied into the repository.
- **The stimuli** are generated by a script committed under `tools/ab/`; no audio is committed. Each is 48 kHz, followed by 12 s of silence:
  - a single-sample impulse at −6 dBFS;
  - a 0.5 s pink-noise burst at −12 dBFS RMS;
  - a dry snare and a short saw chord, each peaking at −12 dBFS.
- **Render settings.**
  - Chimera renders on the host through the real bus (`FxBus`), at MIX 1, dry off, GRIT 0 unless the pair says otherwise.
  - The VST renders 100 % wet in a host at 48 kHz, with its reported latency compensated. A render of the plugin's Defeat program confirms the alignment: the impulse must come back at sample 0 ± 1.
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
| HALL plate corner | none (MV2 had no plate): by ear, against a plate the owner picks |
| GATE defaults | Slow Gate 200 |
| GATE, TAIL 0, SHAPE 0 | Fast Gate 150 |
| REVERSE defaults | Reverse 300 |
| REVERSE, REGEN 0.7 | Reverse Regen 2 s |
| BLOOM defaults | Bloom 1 |
| BLOOM, SWELL 1 | Bloom 1: listen for a once-per-pass echo instead of a swell (fallback in § BLOOM engine) |

- **The pass.**
  - Every time metric (RT60, onset, peak, −20 dB end) of every pair lies within ±30 % of the VST's value.
  - The tilt lies within ±3 dB, and the correlation within ±0.15.
  - A pair that fails passes only by the owner's signed-off ear exception, recorded with a reason in the A/B report.
  - ADR 0085 is accepted only when every pair passes on these terms.
- **What the owner gets.** I hand over the table of measurements and the WAV pairs. The owner judges by ear (UAT); ear changes go back into the constants and goldens, and are re-measured.

## Build order

The CPU risk comes first. Each step after step 0 is a mergeable slice with its tests, goldens, bench row and flash delta.

0. **Baselines and kernels.** Nothing for the user yet; this step decides whether the CPU plan holds.
   - **Re-measure** the REVERB row on today's code, with voice traffic, and record it as the absolute baseline (`QUAD_BUS` comes from the BUS row).
   - **Record the headroom** from `cost_test`'s assertions: 96 in the default build and 23 with `master-tape`, on 2026-10-04's numbers. If QUAD's re-measured BUS reading exceeds today's `COST`, raise `COST` first, and take any broken voice assertion to the owner (§ Target and hard limit).
   - **Run the stress run with QUAD** (six admitted voices, every FX, ORBIT and the UI), and record its OVER, DROPS and peak block load as the reference.
   - **Measure one moving pair's marginal cost** (the +7 vs +10 question) with a microbench of fixed reads against moving pairs.
   - **Write three worst-case kernels.** Run each on a synthetic arena, bench it on the chip as a temporary row with voice traffic, and count it on the host:
     - **BURST:** 12 taps (tap 0 from the register, tap 11 shared with REGEN), 4 taps crossfading, 2 wobbled decorrelator APs, the TAIL loop and the truncating write;
     - **ROOM loop**, the tightest: PRE-DLY and all four stage delays crossfading, 2 wobbled APs, the f32 diffuser, the rotation, 2 soft clips, 4 normalised APs and 6 fixed taps. **It decides the tape-build margin**: a ΔCOST over 23 takes ROOM-T (§ CPU budget) and is benched again, both stages of ROOM-T if needed;
     - **HALL loop:** PRE-DLY and four stage delays crossfading, 2 wobbled APs, the diffuser and 6 fixed taps.

     BLOOM is estimated from the measured costs, since its counts sit between BURST's and HALL's.
   - **Gate ("sound first"):**
     - each kernel's ΔCOST over the baseline must fit the headroom, 96 (23 with the tape);
     - a kernel over the 485 target is recorded and kept, and the A/B decides;
     - a kernel whose ΔCOST would breach the headroom takes its TYPE's fallback and is benched again, before any TYPE work.

     The kernels become steps 2–4's engines once they pass.
1. **Scaffold, in two slices.**
   - **1a. The arena and the dispatcher.**
     - `ReverbArena` comes out of `Ring`, and `dsp::reverb_bus::Reverb` replaces `RingReverb` in `FxBus`.
     - QUAD stays bit-identical, and REV QUAD reads ≤ the step 0 baseline + 1 %.
     - The `EngineState` enum and its zero test land here.
   - **1b. The parameters and the switch.**
     - The new slots and disk codes, with `ReverbType` allowing QUAD only.
     - `with_type`, the knob memory and the latch.
     - The switch state machine, tested QUAD → QUAD by a forced restart, and the REV SWITCH MAX row.
     - The crossfade, read-kind, allpass, tone and LFO helpers, unit-tested alone.
     - `reverb_panel.rs`, REV › MORE and `CompositeId::Reverb`.
2. **GATE, then REVERSE**, on `BurstEngine` (the step 0 kernel): the tap scheduler, the curves, TAIL and REGEN, REGEN's pitch test, the TAIL glyph, the first A/B pairs, and the GATE and RVRS rows.
3. **ROOM**: `LoopEngine`, the rotation, the crossfades, the wobble and its cap, the per-pass pitch test, and the ROOM row.
4. **HALL**, on the same engine: the plate zone and corner, and the HALL row.
5. **BLOOM**: the BLOOM row, and the SWELL 1 A/B item.
6. **Tempo sync**, once ORBIT is on `main`: `Tempo` into `FxBus`, its quantiser, SYNC with its conversion, the division lists and the fold. This is the owner's "big one" for GATE. It lands as soon as ORBIT merges, even if step 5 is still open.
7. **A/B and UAT**: the owner's listening pass against § A/B's pass, the ear-tuned constants and goldens, the final bench readings and flash totals, then ADR 0085 to Accepted.

## Review bar

Each plan task's review grades quality, efficiency, performance and principled-ness (the owner's rubric), and needs ≥ 8 on each. This design aims at that:
- **Quality.**
  - One parameter block, with typed choices.
  - Pure mappings shared by the DSP and the UI.
  - QUAD locked by goldens.
  - Every TYPE gets the same test battery, including the per-pass pitch test, limit cycles and the latch.
- **Efficiency.**
  - One code path per engine, and monomorphs only where the graph differs.
  - One arena, cleared in one block.
  - Crossfades instead of moving reads.
  - No new gesture or page type.
- **Performance.**
  - Moving reads are costed and limited to the wobble.
  - Every TYPE estimates within +18 cycles of QUAD at either pair cost, inside both headrooms.
  - Step 0 benches the tightest kernels on the chip before any TYPE work.
  - `cost_test`'s voice counts are the gate, and so is the stress run.
- **Principled-ness.**
  - The provenance rule is checked per task.
  - Families are graphs, so PLATE is a zone, not a TYPE.
  - Tails are never re-pitched: moves are pitch-free by construction, and the wobble is bounded with no drift.
  - The switch is provably stale-free.
  - Absent parameters are absent, not inert.

## Risks

- **A moving pair's real cost** is 17–20 instructions by estimate. Step 0 measures it. Only the wobble uses moving pairs now, so even +10 leaves every TYPE under QUAD.
- **Crossfade colour.** A 30 ms crossfade blends two lengths of a line, and colours the tail briefly as a comb, as QUAD's does. Slowing SIZE to more, smaller crossfades is free if the A/B hears it.
- **BURST density with 12 taps.** Pre-diffusion should make 11 reads sound like the MV2's 47–90. If the A/B says grainy:
  - first raise the pre-diffuser's g, which is free;
  - then go to 14 taps, which fits (§ CPU budget).
- **Fixed output taps in ROOM and HALL.** The taps sit at fractions of each line's maximum length (with one early tap per side), so the pattern does not scale with SIZE. The ear decides at A/B. Scaled taps fit the default build's headroom but not the tape's (§ CPU budget).
- **BLOOM's long delay** may echo rather than swell at SWELL 1. The A/B checks it, and the fallback fits.
- **#142** (quiet sends lose their tail to i16 truncation) applies to every TYPE alike, and is fixed or not for all of them at once, outside this spec. The new truncating feedback writes add to it at low send levels; the silent-tail tests show how much. Input gains stay at QUAD's ½, so nothing else does.
- **ADR 0086** is not yet on `main` (§ Hints).
- **Flash** is ≈ 9.5–12.5 KB in total. Each step's delta is gated (§ Flash).
