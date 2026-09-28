# Filter Page and Modulation Routing — Design

Nothing is hardwired. Every connection from a modulator to the sound is a mod-matrix route, and the defaults are routes the matrix shows and the user can edit. Every voice runs a fixed pool of six modulators: ENV 1–3, each of TYPE A (a Cascadia-style AHDSR) or B (a Cascadia-style function generator), and LFO 1–3, each CLASSIC (today's LFO) or FUNC (Envelope B cycling). The VCA is a matrix destination. CUTOFF's matrix routes are in octaves. The filter page's ENV, LFO and KEY knobs are shortcuts that edit the routes ENV 1 → CUTOFF, LFO 1 → CUTOFF and NOTE → CUTOFF. KIND, the page's first knob, picks a filter model and lays out the other five knobs as that synth's panel. The chain's FLD block becomes FLD / VCA (AMP) and ends every Part's chain. This is the routing and panel half of #128; the per-model DSP is the other half. Epic #120.

## Roadmap position

This comes before the filter models (#123–#127). It builds the modulator pool, the matrix's new sources and destinations, the FLD / VCA block and the KIND machinery with the one model that exists, the SVF. Each model issue then adds its `FilterKind` variant, its panel row, its modes and its cost. It closes #121 and the routing part of #122, and closes #112's filter and envelope items (FM is retired; the ENV and KEY knobs become route views; ENV 2 and 3 get readers).

## Intent

- Every knob on every FLT page changes the sound, or is visibly fixed or absent.
- One source of truth for routing: the matrix. A knob that looks like a route is a view of a matrix cell.
- The SH-101 one-envelope feel is two matrix edits: route ENV 1 → VCA, delete ENV 2 → VCA.
- The factory Sounds sound exactly as they do today.
- A voice with no VCA route pays about 8 cycles per sample for the modulators on top of today's chain; each envelope routed to the VCA adds 6 (type A) to 24 (type B, curved).

**Done when:**
- ENV 1–3 and LFO 1–3 run in every voice, each with its TYPE, and each is a matrix source;
- the matrix has the eight sources ENV1, LFO1, ENV2, ENV3, LFO2, LFO3, VEL, NOTE, with indices 0 and 1 unchanged;
- a new Sound carries § 2's default routes, and the filter page's ENV, LFO and KEY knobs edit them;
- CUTOFF's routes are in octaves and KEY at 100 % tracks one octave per octave;
- VCA is a destination; its sum-and-clamp rule, the per-engine behaviour with no route and the lifetime rule are in, and no voice drones;
- the FLD / VCA block ends every Part's chain (Algo and Modal), and every factory Sound renders bit-identically to today;
- the `FilterKind` panel machinery, the mode lists and the kind-change rule are in, with the SVF as the one built kind;
- the bench's new MODS and SVF rows are measured and their costs committed.

## What exists today

| Claim | Evidence |
|---|---|
| One 2-pole SVF, saturation in the feedback loop, 8 modes | `dsp/filter.rs`: `FilterMode` `Lp1`…`Phazor`; `SvfFilter::process` |
| The filter reads cutoff, resonance, drive and mode only | `filter.rs:55-59`; nothing reads `fm_amount`, `env_amount`, `key_track` (`params.rs:34-36`, #112) |
| MODE is on no page; every Sound is on LP4 | `FilterParams.mode: u8`, default 2, no `ParamSpec` (`params.rs:13,25`); `FILTER` binds CUTOFF, RES, DRIVE, FM, ENV, KEY (`block_registry.rs:94`) |
| The cutoff is computed once per block | `filter.rs:61-62`: one `fast_tan` per `process` call |
| Three `EnvParams`, only `envelopes[0]` runs | `ParamSnapshot.envelopes: [EnvParams; 3]` (`params.rs:375`); `Voice` holds one `amp_env` (`voice.rs:32`) |
| The amp envelope ticks per sample, with a divide per sample, and doesn't shape the output | `voice.rs:298-304`; `envelope.rs:60,70,80`: `rate = 1.0 / (time * sr)` per sample; ADR 0022: "no engine puts it on the VCA" |
| A voice ends when its engine does | `voice.rs:307` |
| The fold runs before the output level | `voice.rs:286-304`: engine → drive → filter → folder → `volume` |
| FLD is FOLD · SYM · MIX and three empty slots, on the Algo chain only | `FOLDER` (id 9, `block_registry.rs:74`); `ALGO_BLOCKS` has it, `MODAL_PLUCK_BLOCKS` doesn't, though `Voice` runs the folder for every engine |
| ENV_FILTER and ENV_AUX exist but nothing reads them | `block_registry.rs:149-195`: `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` are `ParamSlot::legacy`; their chain `ENVELOPE_CHAIN` is not returned by `chain_def_for` |
| The page that edits ENV 1 is `ENVELOPE` (id 11), under MOD | `block_registry.rs:116`, `MOD_MATRIX_SUB_PAGES = [&ENVELOPE, &LFO]` |
| The matrix has two sources, by position | `PART_MOD_SOURCES = ["ENV", "LFO"]`; `voice.rs:256-263`: source 0 = `amp_env.current_level()` (contour × velocity) at block start, source 1 = the LFO |
| The matrix has 8 source rows and 16 destination columns | `MAX_MOD_SOURCES = 8`, `MAX_MOD_DESTS = 16` (`modulation.rs:14-15`); `ModState.amounts: [[i8; 16]; 8]` |
| A matrix cell of 0 is "no route" | `ModState` stores amounts only; `sum_for` skips 0 |
| One LFO, run once per block, only when the matrix has ≥ 2 sources; its value is taken before the block's advance | `voice.rs:261-263`; `lfo.rs:241-294` |
| Matrix offsets are linear in the param's range | `apply_offset`: `v + off·(max − min)` (ADR 0010) |
| A new Sound has an empty matrix | `Sound::init` (`preset.rs:56`): `ModState::new()`; the factory builds on it (`factory.rs:43`) |
| No factory Sound or golden routes source 0 | `factory.rs`: MORPH PAD routes source 1 (LFO) to MORPH; `tests/common/mod.rs`: every modulated golden uses source 1. `modulation_integration_test.rs` routes source 0 to CUTOFF, asserting only that it moves |
| The FLT and ADSR vizzes read slots by position | `ui/renderer.rs:102` `viz::filter(a(0), a(1))`; `:105` ATK/DEC/SUS/REL from `a(0..3)` |
| Sounds are not persisted | `SoundPool` is RAM only; the factory bank is code (`factory.rs`); no serializer exists |
| RAM today | `size_of::<Voice>()` 40,688 B; `size_of::<Instrument>()` 246,728 B of `VOICE_RAM_BUDGET` 286,720 (`instrument.rs:92`); `AXI_RESIDENT` 419,044 B of 524,288; `ParamSnapshot` 300 B; measured 2026-09-27, host build |

## Decisions

These record the owner's decisions of 2026-09-27, made precise. They replace the first version's fixed routes with SOURCE selectors, and the second version's fixed wiring.

### 1. The modulator pool

- Every voice runs six modulators, always: ENV 1, ENV 2, ENV 3 (`EnvSlot`) and LFO 1, LFO 2, LFO 3 (`LfoSlot`). There is no add or remove, and the MOD page lists all six.
- A slot is wired to nothing. What it drives is whatever the matrix routes from it (§ 2).
- Default TYPEs: ENV 1 A, ENV 2 A, ENV 3 B (MODE ENV, FORM AD); LFO 1–3 CLASSIC. Any ENV slot can be A or B; any LFO slot CLASSIC or FUNC.
- Types A and B follow the Intellijel Cascadia's Envelope A and Envelope B (manual v1.2, 2023-10-15, pp. 28–39 and 82–97): its behaviour and ranges, none of its code. Where Chimera departs from it, the departure is listed under "Defaults chosen".
- **Outputs:** an ENV slot outputs 0..1 (A; B in ENV and BURST) or −1..1 (B in LFO, as the Cascadia's ±5 V), without velocity (VEL is a matrix source, AMP's VEL scales the VCA, and ENV n LEVEL scales an A slot). An LFO slot outputs −1..1.
- A value a TYPE, MODE or FORM doesn't use is kept, unread and unshown, so switching back restores it.
- **Sliders are positions.** A time or rate slider stores a position `p` in 0..1, and its quantity follows an exponential law, `q = q_min · (q_max / q_min)^p`, one `fast_exp2` per slider per block. A matrix route adds to `p` (linear law, clamped 0..1), which is therefore a route in octaves of the quantity, as the Cascadia's MOD inputs are V/oct.

#### Envelope A: AHDSR

- **Sliders** H, A, D, S, R: H, A, D and R are positions with the SPEED ranges below; S is a level, 0..1.
- **Stages:** Attack to 1; Hold (per HOLD POSITION); Decay to S; Sustain while the gate is high; Release to 0 when it goes low, from whatever stage is running. A note-on restarts Attack from the current level (the Cascadia's RETRIG, and today's behaviour). A slider's time is the stage's time from its start level to its end level at full swing (0→1, 1→0).
- **HOLD POSITION** (`HoldPos`):
  - **OFF:** a plain ADSR; H is ignored.
  - **AHDSR** (default): after the attack, hold at 1 for H, then decay.
  - **GATE EXT:** the gate is the key gate OR a gate H long from the note-on, so a short stab still plays the full envelope; H adds no stage.
- **SPEED** (`EnvSpeed`), the manual's ranges exactly; default MED:

| SPEED | H | A | D and R |
|---|---|---|---|
| FAST | 0.001 ms – 2.5 s | 0.2 ms – 1.5 s | 0.6 ms – 2.5 s |
| MED | 0.001 ms – 10 s | 2 ms – 10 s | 3.5 ms – 10 s |
| SLOW | 0.001 ms – 60 s | 9.3 ms – 60 s | 30 ms – 60 s |

- **Shape: one fixed RC curve, divide-free.** The Cascadia has no curve control, so A has none. Each stage is a one-pole approach to a target past its end, as an analog envelope's capacitor charging toward a rail: `L += c · (T − L)` per sample, `c = 1 − 2^(−1/(τ·fs·ln 2))` computed once per block.
  - Attack aims at T = 1.3 and ends at 1: τ = A / ln(1.3 / 0.3) = A / 1.466.
  - Decay aims at T = S − 0.01 and ends at S; Release aims at T = −0.01 and ends at 0: τ = time / ln(1.01 / 0.01) = time / 4.615, so a full swing takes exactly the slider's time.
  - Per sample: one multiply-add and a compare. `advance(n)` is closed form, `L_n = T + (L − T)·2^(−n/(τ·fs·ln 2))`, with a `fast_log2` to place a stage end inside the block.
- **ENV n LEVEL** and **ENV n TIME** are matrix destinations: they are the Cascadia's CTRL SOURCE (LEVEL / OFF / TIME), done in the matrix.
  - LEVEL: with no route into it, the envelope peaks at 1 (CTRL OFF). With a route, its peak is `clamp(Σ, 0, 1)` (base 0, as the VCA's): VEL → ENV 2 LEVEL at 100 % is the Cascadia's default velocity-to-level.
  - TIME: every stage's time (H, A, D, R) is scaled by `2^(−5·Σ)`: +100 % is 32 × shorter, −100 % 32 × longer, 0 the sliders' times.
  - Both are read per block. No Sound gets a route to them by default, so every Sound keeps its behaviour. They are inert on a type-B slot.
- Idle: after Release reaches 0.
- **Page:** A · D · S / R · H · TYPE. SPEED and HOLD POSITION are on the shared SPD page (§ UI).

#### Envelope B: function generator

- **MODE** ENV, LFO or BURST; **FORM** (the Cascadia's TYPE SELECT, renamed so it doesn't clash with the slot's TYPE): CYCLE, AHR or AD in ENV and BURST; FREE, SYNC or LFV in LFO. **RISE**, **FALL** and **SHAPE** are three sliders whose meaning follows MODE and FORM, as on the Cascadia's two-line panel. RISE and FALL are positions (exponential law); SHAPE is 0..1, centre ½.

| MODE, FORM | RISE | FALL | SHAPE |
|---|---|---|---|
| ENV (AD, AHR, CYCLE) | rise time, 2 ms – 5 s | fall time, 2 ms – 5 s | curvature: log · linear · exp |
| LFO FREE | RATE, 0.05 – 800 Hz | PHASE, 0° – 360° | TILT: saw · triangle · ramp |
| LFO SYNC | clock ÷8 … ×8 with #44; RATE until then | PHASE | TILT |
| LFO LFV | RATE, 0.05 – 800 Hz | DELTA, 0 – 1 | SLEW, 0 – 1 |
| BURST (AD, AHR, CYCLE) | pulse RATE, 0.05 Hz – 1 kHz | LENGTH, 10 ms – 20 s | TILT of the burst and its pulses |

- **ENV mode.**
  - **AD:** a note-on rises from the current level to 1 over RISE, then falls to 0 over FALL; key-up is ignored.
  - **AHR:** rises over RISE, holds 1 while the key is held, falls over FALL at key-up; a key-up during the rise falls from where it is.
  - **CYCLE:** rise and fall loop, period RISE + FALL; a note-on resets the cycle to the start of the rise.
  - **SHAPE** bends both segments. With `s = 2·SHAPE − 1` and `w = 2^(4·s)`, the segment's linear position `x` gives rising `L = f(x)`, falling `L = 1 − f(1 − x)`, `f(x) = x / (x + (1 − x)·w)`. Centre is linear (and skips the divide); below centre logarithmic (fast start, slow finish), above exponential (slow start, fast finish), as the manual's drawings. At the ends a segment has covered 94 % or 6 % of its swing at half its time. A change of direction remaps `x` so `L` doesn't move: `f⁻¹(y) = w·y / (1 − y + w·y)`.
- **LFO mode,** output −1..1.
  - **FREE:** phase φ runs at RATE; the output is the tilted wave at `frac(φ + PHASE)`. With `r = TILT position`, `u = φ/r` for φ < r, `(1 − φ)/(1 − r)` after (r = 0: `1 − φ`; r = 1: `φ`), out = `2u − 1`. Bottom is the saw (instant rise, linear fall), middle the triangle, top the ramp (linear rise, instant fall), as the manual's p. 88. The phase runs free across notes.
  - **SYNC:** the Cascadia multiplies or divides a clock at GATE/SYNC. Until the Tempo epic ([#44](https://github.com/joegiralt/chimera/issues/44)) gives Chimera a clock, RISE reads as RATE (as FREE) and SYNC resets φ at each note-on, so the wave starts at PHASE. With #44, RISE becomes the clock ratio (÷8 … ÷2, ×1, ×2 … ×8, the manual's default set) and SYNC follows the clock.
  - **LFV** (low-frequency vacillator): each cycle at RATE picks a target `t_k = clamp(t_{k−1} + DELTA · r_k, −1, 1)`, `r_k` uniform in −1..1 from the slot's PRNG, and the output moves linearly from `t_{k−1}` to `t_k` across the cycle. SLEW rounds it with a one-pole low-pass of time constant SLEW × one cycle. DELTA 1 is a fairly random walk around 0; DELTA near 0 a slow drunken walk. This covers random and S&H behaviour.
- **BURST mode,** output 0..1: pulses at RATE inside a burst envelope LENGTH long.
  - The burst envelope: with `p = TILT position`, it rises over p·LENGTH and falls over (1 − p)·LENGTH. Bottom: pulses shrink across the burst; top: they grow; middle: the loudest pulse is at the midpoint.
  - The pulse shape: under AD and AHR, square at either end of TILT, morphing to a sine (`½ − ½·cos 2πφ`) at the middle: `(1 − m)·square + m·sine`, `m = 1 − |2p − 1|`. Under CYCLE, the tilting saw (saw · triangle · ramp, as LFO FREE, unipolar), as the manual says CYCLE always uses.
  - **AD:** one burst per note-on; key-up ignored.
  - **AHR:** the burst envelope rises, holds at its peak while the key is held (pulses continue), then falls after key-up.
  - **CYCLE:** the burst repeats while the key is held; at key-up the running burst finishes.
- **Rates.** A B slot routed to VCA runs per sample and has the manual's full ranges. A B slot evaluated per block clamps any rate to 93.75 Hz (block rate ÷ 8), so it isn't aliased at the block rate; per-sample operation is reserved for the VCA, where audio-rate AM is the point.
- **ENV n RISE, FALL** and **SHAPE** are matrix destinations: the Cascadia's RISE MOD, FALL MOD and SHAPE MOD sliders are the route amounts. A route adds to the slider's position, so RISE and FALL modulate in octaves of their quantity (+100 % is the slider's full travel), and SHAPE linearly. Read per block. Inert on a type-A slot.
- Idle: AD after its fall or burst; AHR after its fall or the burst's fall. CYCLE and LFO are never idle.
- **Page:** MODE · RISE · FALL / SHAPE · FORM · TYPE, one page; the labels follow MODE and FORM (RISE, RATE, CLOCK; FALL, PHASE, DELTA, LENGTH; SHAPE, TILT, SLEW).

#### LFO slots

- **CLASSIC** is today's LFO: sine, triangle, saw, square, S&H; RATE SHAPE SYNC PHASE DEPTH, and OFFSET stored but no longer applied (below); its value is taken before the block's advance, with today's arithmetic, so LFO 1 is bit-identical.
- **FUNC** is Envelope B locked to LFO mode, FORM FREE, SYNC or LFV (default FREE), with its own RISE, FALL and SHAPE. Output −1..1. DEPTH is not applied under FUNC (it isn't on the FUNC page; the route's amount is the depth).
- **Pages,** with TYPE in the last cell as on the ENV pages:
  - CLASSIC: RATE · SHAPE · SYNC / PHASE · DEPTH · TYPE. TYPE takes OFST's cell. OFFSET leaves the page and is no longer applied: a constant added to a source only shifts its destination, which the destination's own knob already does, and no Sound sets it (default 0, so nothing changes). DEPTH stays: MORPH PAD's amount is computed from it.
  - FUNC: MODE (LFO, fixed, dimmed) · RATE · PHASE or DELTA / TILT or SLEW · FORM · TYPE: B's page with MODE locked.

#### TYPE, MODE or FORM change mid-note: the level never jumps

- Into A with the key up: Release from the current level. With the key down: from a rising segment, Attack; from anything else, Decay if L > S, else Sustain at L (Sustain holds whatever it reached). A's state is its level, so nothing needs remapping.
- Into B ENV: from a rising segment, rise from the current level; from anything else, fall from it (`x` remapped).
- Into B LFO or BURST, CLASSIC or FUNC: the new shape starts where it would, and the difference `d` between the last output before the change and the first after glides linearly to 0 over 256 samples, read at the slot's rate (per sample on the VCA, per block elsewhere).
- Slider changes rely on the UI's lerp (CLAUDE.md), as every parameter does.

### 2. Every connection is a matrix route

- **Sources,** eight, which fill `MAX_MOD_SOURCES` (8):

| Index | Source | Value |
|---|---|---|
| 0 | ENV1 | ENV 1's output (was source 0, the amp envelope) |
| 1 | LFO1 | LFO 1's output (as today's source 1) |
| 2 | ENV2 | ENV 2's output |
| 3 | ENV3 | ENV 3's output |
| 4 | LFO2 | LFO 2's output |
| 5 | LFO3 | LFO 3's output |
| 6 | VEL | the note's velocity, 0..1 |
| 7 | NOTE | (note − 60) / 120, clamped to −1..1 |

  Indices 0 and 1 keep their meaning, so MORPH PAD's route and every golden's route are unchanged. Source 0 loses its velocity factor (§ 1); no Sound or golden routes it.
- **Route presence.** A route exists apart from its amount: `ModState` and `MatrixState` gain `present: [u8; MAX_MOD_DESTS]`, one bit per source, copied by `sync_from_matrix`. Turning an empty cell creates the route (sets the bit); turning a route to 0 keeps it; MIX+MINUS on a matrix cell deletes it (clears the bit and the amount). `set_amount` with a nonzero amount sets the bit, so code-built states (MORPH PAD, tests) keep working. The audio thread's sums ignore the bits (a route at 0 adds 0); the VCA rule (§ 4), the lifetime rule and the knob views read them.
- **Destinations:** every modulatable parameter, as today (ADR 0010), plus:
  - **CUTOFF** sums in octaves (§ 3);
  - **VCA,** the voice's output level (§ 4), a new destination on the Out block;
  - **ENV n LEVEL, TIME, RISE, FALL, SHAPE** for n = 1–3 (§ 1), fifteen new destinations. LEVEL and TIME act on type A, RISE, FALL and SHAPE on type B; each is inert on the other type.
- **Capacity.** The fifteen only need to be available. A Sound's matrix holds 16 destination columns, added on demand when a parameter is primed or a route knob creates one (`ModDestRegistry`, `MAX_MOD_DESTS`); the set of addressable destinations is not bounded by the grid. Nothing new is added by default, so the defaults still take one column (two on VA).
- **Default routes** in every new Sound (`Sound::init`, and so every factory Sound):

| Route | Amount | On |
|---|---|---|
| ENV 1 → CUTOFF | 0 | every engine |
| LFO 1 → CUTOFF | 0 | every engine |
| NOTE → CUTOFF | the kind's key-tracking default: SVF 0 | every engine |
| ENV 2 → VCA | 100 % (127) | VA only |

  Algo and Modal Sounds don't get ENV 2 → VCA, so their VCA stays today's pass-through (§ 4). All three CUTOFF routes are at 0 there, so every factory Sound renders bit-identically; they show in the matrix and the filter knobs edit them.
- **Room.** The defaults take one destination column (CUTOFF) on Algo and Modal and two (CUTOFF, VCA) on VA, leaving 14 or 15 of the 16. The eight source rows are exactly the eight sources; a ninth source needs `MAX_MOD_SOURCES` raised to 16, which adds 128 B to every `ModState` and widens `present` to `u16`.
- **The SH-101 one-envelope feel:** route ENV 1 → VCA at 100 % and delete ENV 2 → VCA. One `Envelope` then feeds the cutoff (per block) and the VCA (per sample); nothing is copied.
- **KIND never edits the matrix,** on a new Sound or any other. A KIND change keeps every route. The SH-101 kind therefore doesn't set ENV 1 → VCA: a KIND that rewrote routes would destroy the user's routing, and there is no "new Sound of kind X" path (a new Sound's kind is SVF). A kind's key-tracking default applies only to a new Sound's NOTE → CUTOFF, and with the SVF as the only kind a new Sound can have, that is 0.

### 3. CUTOFF in octaves

- Routes into CUTOFF sum as today, `Σ = Σᵢ sᵢ · amtᵢ / 127`, but apply in octaves:
  `fc = clamp(base · 2^(CUTOFF_OCTAVES · Σ), 20 Hz, min(20 kHz, 0.49·fs))`, with `CUTOFF_OCTAVES = 10` and `base` the CUTOFF knob.
- When Σ is 0, `fc` is `base` bit for bit (no `exp2` runs).
- 100 % from ENV 1 is +10 octaves at the envelope's peak; 100 % from NOTE is exactly one octave per octave (the NOTE source's 1/120 cancels the 10).
- `ParamSpec` gains an offset law, `Linear` (every other parameter, ADR 0010) or `Octaves(f32)` (CUTOFF). This supersedes ADR 0010's linear law for CUTOFF only. It changes the sound of any Sound that routes to CUTOFF; no factory Sound does, and the two `*_lfo_cutoff` test goldens are re-recorded (§ Migration).
- The CUTOFF offset is computed per block from block-start values, as every matrix offset is. The filter ramps its coefficient `g` linearly across the block from the previous block's `fc` to this block's; on a fresh note there is no ramp. When the two are equal, `g` is held as today.

### 4. The VCA is a destination

- VCA is `ParamAddr(Out, OutParams::VCA)`: a hidden parameter, 0..1, stored value 0, on no page and never edited, with the linear law. Its value is therefore `clamp(0 + Σ, 0, 1)`: the **sum** of the routes into it, clamped to 0..1.
- **Sum, not product.** The sum is the one rule every destination already follows, so an amount means the same thing everywhere. A route at part depth reaches silence when its envelope does (ENV 2 → VCA at 50 % is half the level, and 0 when ENV 2 is idle), which is when the lifetime rule ends the voice. A product with depth (`1 − a + a·s`) would leave a floor of `1 − a` at an idle envelope, so the voice would end on a step, and VCA would be the one destination that multiplies. The price: an LFO route on top of an envelope adds to it rather than scaling it (tremolo clamps at 1, and after release the LFO's positive half can still sound until the voice ends, when the fade below removes it).
- **Per sample.** The voice computes the VCA gain per sample, not through the block-rate offset path:
  `g[n] = clamp(Σ_ENV routes aᵢ · eᵢ[n] + Σ_other routes aⱼ · lerp(prevⱼ, curⱼ, n/64), 0, 1) · vel`,
  where an ENV slot routed to the VCA ticks per sample into a 64-sample buffer, the other sources (LFOs, VEL, NOTE) are ramped linearly from the previous block's value, and `vel = 1 − VEL + VEL · v` (AMP's VEL, `v` the note's velocity).
- **No route into VCA** (no present bit in the VCA column): the engine decides.
  - Algo, Modal: the VCA is a pass-through at 1: `sample · volume`, today's expression, bit for bit. VEL is dimmed and unread. The engine's own envelopes shape the sound.
  - VA: the VCA is a gate: 1 while the key is held, 0 after key-up, with a 64-sample linear ramp at each edge, times `vel`. VA notes can't drone.
- **Voice lifetime.** The voice goes inactive at the end of the first block in which its end condition holds:
  - with routes into VCA: no source routed to VCA **holds** the voice, or the engine is inactive. An ENV slot of type A, or B with FORM AD or AHR, holds it until it is idle (A: its Release ends; AD: its fall or burst ends; AHR: its fall, or its burst's fall, ends after key-up). A type-B slot in CYCLE or LFO holds it only while the key is held (BURST CYCLE: until the burst running at key-up ends). An LFO slot, VEL or NOTE holds it while the key is held;
  - no route, Algo or Modal: the engine is inactive (today);
  - no route, VA: the key is up and the gate's ramp has reached 0, or the engine is inactive.

  If the VCA gain isn't 0 when the condition first holds (a non-ending source still contributes, as an LFO at key-up), the voice ends through ADR 0027's fade (`FADE`, 128 samples) instead of at once. A silent engine can't sound through any VCA, so the engine's end always ends the voice. A fade from shedding (ADR 0027) ends it as today.
- A type-B slot cycling in LFO mode, routed to VCA, holds the voice only while the key is held: it can't drone.

### 5. FLD / VCA ends every chain

- The FLD block becomes FLD / VCA: the fold, then the VCA. Its short label is **AMP**; its name is "Fold / VCA". Its page is FOLD · SYM · MIX · **VEL** · — · —.
- Every Part's chain has it, last before MOD: the Algo chain (where FLD is today) and the Modal chain (which gains it).
- The fold comes before the VCA, as `Voice` already runs it, so the fold's colour doesn't change with level.
- **VEL** is the VCA's velocity sensitivity, 0–100 %, default 100 %. It applies whenever the VCA isn't the Algo/Modal pass-through (a route into VCA, or VA's gate), and is dimmed otherwise.
- There is no VCA source setting: the matrix's VCA column replaces it.

### 6. KIND lays out the panel

- KIND is knob 1 of the FLT page. It sets knobs 2–6 to that synth's panel. Every kind has CUTOFF and RES.

| KIND | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|
| SVF | CUTOFF | RES | MODE | ENV | KEY |
| MOOG | CUTOFF | RES | ENV | KEY | DRIVE |
| SH-101 | FREQ | RES | ENV | MOD (LFO) | KYBD (KEY) |
| PROPHET-5 | CUTOFF | RES | ENV AMT | KEYBD (KEY, off/half/full) | DRIVE |
| TB-303 | CUTOFF | RESO | ENV MOD | DECAY (shortcut) | ACCENT |
| MS-20 | CUTOFF | PEAK | EG (ENV) | MG (LFO) | HP/LP (MODE) |

- A label is the original panel's word; the target is the same everywhere. SH-101's FREQ is CUTOFF.
- **Route knobs** are views of matrix cells: ENV is ENV 1 → CUTOFF, LFO is LFO 1 → CUTOFF, KEY is NOTE → CUTOFF. The value shown is the route's amount, −100..+100 %, KEY included (the matrix can hold a negative KEY).
  - Turning a knob whose route is absent creates it at 0 and applies the turn. If CUTOFF has no column, one is added; if all 16 are taken, the knob stays "—" and reports "matrix full" in place (ADR 0017's status line).
  - An absent route shows "—" until the knob is turned.
  - The matrix always applies: a kind without a KEY knob still tracks if NOTE → CUTOFF exists. "Not shown, not applied" holds only for the filter's own parameters (DRIVE on a kind without it, a mode the kind lacks).
- **Shortcuts to elsewhere:** TB-303's DECAY is ENV 1's DECAY (ENV 1 is the default filter envelope). If ENV 1 is type B, DECAY shows dimmed and does nothing.
- The Moog panel follows the Minimoog's filter section: cutoff, emphasis, amount of contour, keyboard control; DRIVE stands for driving the filter from the mixer.
- **Prophet's KEYBD** is a stepped view of NOTE → CUTOFF: OFF, HALF or FULL at amounts 0, 64 and 127, and otherwise its percentage; one detent moves to the next step in that direction. HALF is 64/127, 0.504 octaves per octave.
- **SVF extras:** DRIVE and LFO sit on SVF's FLT › MODE sub-page.
- **ACCENT** (TB-303) is a new filter parameter, defined with the 303 model in #127.

### 7. MODE follows KIND

| KIND | MODE choices (the first is the default) |
|---|---|
| SVF | LP24, LP6, LP12, BP12, BP24, HP24, NOTCH, PHASER |
| MOOG, SH-101, PROPHET-5 | LP24 |
| MS-20 | LP12, HP12 |
| TB-303 | LP18 |

- The SVF's default is LP24, today's mode, so it is listed first; the others keep #122's order.
- A single-mode kind shows its mode fixed and dimmed (for example "LP18"); turning it does nothing.
- Changing KIND keeps MODE if the new kind has it; otherwise MODE becomes the new kind's default. This is the only thing a KIND change moves.
- `mode ∈ kind.modes()` always holds: `FilterParams` enforces it on every write of KIND or MODE.

## Data model

```rust
/// Which filter model. A variant lands with its model (#123–#127);
/// until then only `Svf` exists.
#[repr(u8)]
pub enum FilterKind { Svf = 0 /*, Moog = 1, Ms20 = 2, Sh101 = 3, Prophet5 = 4, Tb303 = 5 */ }

/// Every mode any kind has. The discriminants 0–7 are today's `mode` u8.
#[repr(u8)]
pub enum FilterMode { Lp6 = 0, Lp12 = 1, Lp24 = 2, Bp12 = 3, Bp24 = 4, Hp24 = 5, Notch = 6, Phaser = 7, Lp18 = 8, Hp12 = 9 }

pub enum EnvSlot { Env1, Env2, Env3 }
pub enum LfoSlot { Lfo1, Lfo2, Lfo3 }
#[repr(u8)] pub enum EnvType { A = 0, B = 1 }
#[repr(u8)] pub enum EnvSpeed { Fast = 0, Med = 1, Slow = 2 }
#[repr(u8)] pub enum HoldPos { Off = 0, Ahdsr = 1, GateExt = 2 }
#[repr(u8)] pub enum FuncMode { Env = 0, Lfo = 1, Burst = 2 }
/// Cascadia's TYPE SELECT. ENV and BURST use Cycle, Ahr, Ad; LFO uses Free, Sync, Lfv.
#[repr(u8)] pub enum FuncForm { Cycle = 0, Ahr = 1, Ad = 2, Free = 3, Sync = 4, Lfv = 5 }
#[repr(u8)] pub enum LfoType { Classic = 0, Func = 1 }

/// The matrix's source rows, in `Voice`'s order (§ 2). Indices are stored.
pub enum ModSource { Env1, Lfo1, Env2, Env3, Lfo2, Lfo3, Vel, Note }

/// Envelope B's three sliders and two switches; also a FUNC LFO's
/// (whose mode is always Lfo). Sliders are positions, 0..1.
pub struct FuncParams { pub mode: FuncMode, pub form: FuncForm, pub rise: f32, pub fall: f32, pub shape: f32 }
```

- `EnvParams`: `attack`, `decay`, `release` become slider positions; it gains `env_type`, `hold` (a position), `speed`, `hold_pos`, `time` (the TIME destination's stored 0) and `func: FuncParams`. `level` (today unread) becomes the LEVEL destination. `ParamSnapshot::default()` sets `envelopes[2]` to type B, MODE ENV, FORM AD.
- `ParamSnapshot.lfo` becomes `lfos: [LfoParams; 3]`; `LfoParams` gains `lfo_type` and `func: FuncParams` (mode LFO, form FREE).
- `FilterParams` gains `kind: FilterKind`; `mode` becomes `FilterMode`. Both are private, set through `set_kind` and `set_mode`, which keep `mode ∈ kind.modes()`. `fm_amount`, `env_amount` and `key_track` are deleted: the knobs are route views.
- `OutParams` gains `vca: f32` (the VCA destination's stored 0) and `vca_vel: f32`.
- `ModState` and `MatrixState` gain `present: [u8; MAX_MOD_DESTS]`. `ModState::routes_into(addr) -> u8` returns the column's present bits (0 if no column).
- `BlockRef::AmpEnv`, `FilterEnv`, `AuxEnv` become `BlockRef::Env(EnvSlot)`; `BlockRef::Lfo` becomes `BlockRef::Lfo(LfoSlot)`, like `AlgoOp(Op)`. Matrix tags: E1, E2, E3, LF1, LF2, LF3, VEL, NTE (≤ 3 characters, #15).
- `Envelope` becomes the ENV-slot modulator: it runs A or B from `EnvParams`, exposes `output()` (raw contour), `is_idle()` and `holds(key_down)`, `tick()` per sample and `advance(n)` per block. `Lfo` runs CLASSIC or FUNC; B's state machine is one `FuncGen` shared by both.
- `Voice` holds `envs: [Envelope; 3]`, `lfos: [Lfo; 3]`, the previous block's `fc` and VCA-source values (for the ramps), the gate ramp's level and the glide `d` per slot.

### ParamIds

No id is reused (ADR 0009). The first version's ENV SRC (Filter 9) and LFO SRC (Filter 10) were never built; their ids stay free.

| Block | Param | ParamId | Kind | Range | Default | Modulatable |
|---|---|---|---|---|---|---|
| Filter | CUTOFF | 0 | continuous, `Octaves(10)` | 20..20,000 Hz | 1,000 | yes |
| Filter | RES | 1 | continuous | 0..1 | 0 | yes |
| Filter | DRIVE | 2 | continuous | 0..1 | 0 | yes |
| Filter | FM | 3 | **retired** | | | |
| Filter | ENV | 4 | **retired**: now the route ENV 1 → CUTOFF | | | |
| Filter | KEY | 5 | **retired**: now the route NOTE → CUTOFF | | | |
| Filter | KIND | 6 (new) | enum, `FilterKind` | built kinds | SVF | no |
| Filter | MODE | 7 (new) | enum, `FilterMode` | the kind's list | LP24 | no |
| Env n | A, D, R | 0, 1, 3 | position, exponential by SPEED (was seconds) | 0..1 | A 0.189, D and R 0.559 (10 ms, 300 ms at MED) | no |
| Env n | S | 2 | continuous | 0..1 | 0.7 | no |
| Env n | LEVEL | 4 | hidden destination; peak = `clamp(Σ, 0, 1)` when routed, else 1 | 0..1 | 1 | yes |
| Env n | VEL | 5 | unread, off the page (#112) | | | no |
| Env n | H | 6 (new) | position, exponential by SPEED | 0..1 | 0 (0.001 ms) | no |
| Env n | TYPE | 7 (new) | enum, `EnvType` | A, B | A (ENV 3: B) | no |
| Env n | SPEED | 8 (new) | enum, `EnvSpeed` | FAST, MED, SLOW | MED | no |
| Env n | HOLD | 9 (new) | enum, `HoldPos` | OFF, AHDSR, GATE EXT | AHDSR | no |
| Env n | TIME | 10 (new) | hidden destination; times × `2^(−5·Σ)` | 0 | 0 | yes |
| Env n | MODE | 11 (new) | enum, `FuncMode` | ENV, LFO, BURST | ENV | no |
| Env n | FORM | 12 (new) | enum, `FuncForm` | the MODE's three | AD (LFO: FREE) | no |
| Env n | RISE | 13 (new) | position | 0..1 | 0.206 (10 ms in ENV) | yes |
| Env n | FALL | 14 (new) | position | 0..1 | 0.640 (300 ms in ENV) | yes |
| Env n | SHAPE | 15 (new) | continuous, centre ½ | 0..1 | 0.5 | yes |
| LFO n | as today | 0–5 | OFFSET (5) no longer applied | | | no |
| LFO n | TYPE | 6 (new) | enum, `LfoType` | CLASSIC, FUNC | CLASSIC | no |
| LFO n | FORM | 7 (new) | enum, `FuncForm` | FREE, SYNC, LFV | FREE | no |
| LFO n | RISE, FALL, SHAPE | 8–10 (new) | as Env n 13–15, LFO meanings | 0..1 | 0.309 (1 Hz), 0, 0.5 (triangle) | no |
| Out | VCA | 2 (new) | continuous, hidden | 0..1 | 0 | yes (the VCA destination) |
| Out | VEL | 3 (new) | continuous | 0..1 | 1 | no |

- ACCENT takes the next free Filter id when #127 lands.
- Of the envelope and LFO parameters, only ENV n LEVEL, TIME, RISE, FALL and SHAPE are modulatable (`voice_reads` true for exactly those, read per block). A FUNC LFO's sliders, and every other envelope or LFO parameter, stay non-modulatable: opening them is a separate decision.
- MODE's `ParamSpec` names all ten modes, for storage; the UI steps through `kind.modes()` only.

### Per-kind data, as `const`

```rust
pub enum PanelTarget {
    /// A Filter parameter.
    Filter(ParamId),
    /// The amount of the matrix route `source → CUTOFF`.
    Route(ModSource),
    /// A parameter of ENV 1 (TB-303's DECAY).
    Env1(ParamId),
}
pub enum KnobView { Spec, Steps(&'static [(i8, &'static str)]) }
pub struct PanelKnob { pub target: PanelTarget, pub label: &'static str, pub view: KnobView }
pub struct KindPanel {
    /// Knobs 2–6 of the FLT page.
    pub main: [PanelKnob; 5],
    /// Slots 2–3 of FLT › MODE.
    pub extras: [Option<PanelKnob>; 2],
}

impl FilterKind {
    pub const fn panel(self) -> &'static KindPanel;
    pub const fn modes(self) -> &'static [FilterMode];   // never empty; [0] is the default
    pub const fn key_default(self) -> i8;                // NOTE → CUTOFF on a new Sound
    pub const fn applies(self) -> Applies;               // the filter's own params, from `panel`
    pub const fn cost(self, mode: FilterMode) -> Cost;
}

pub struct ModPanel { pub slots: [Option<ParamId>; 6] }
impl EnvType { pub const fn panel(self, mode: FuncMode, form: FuncForm) -> &'static ModPanel; }
impl LfoType { pub const fn panel(self, form: FuncForm) -> &'static ModPanel; }
impl FuncMode { pub const fn forms(self) -> &'static [FuncForm]; }   // [0] is the default
```

- `SVF_PANEL`: main CUTOFF, RES, MODE, Route(Env1), Route(Note); extras DRIVE, Route(Lfo1).
- The other kinds' panels are § 6's table, each added with its model issue.
- Pure functions, no state: `routed_cutoff(base, sum) -> f32`, `vca_gain(...)`, `kind_change(old, new, params) -> FilterParams` (MODE only), `slider(p, min, max)`, `rc_coeff(tau, fs)`, `curve(x, w)`, `curve_inv(y, w)`, `burst(t, phase, tilt, form)`, `type_change(state, level, new) -> State`.

## Signal flow and rates

Per voice, per block, in `Voice::render`:

1. **Modulators.** Read the ENV destinations (LEVEL, TIME, RISE, FALL, SHAPE) from the previous block's matrix offsets, so a slot never waits on the matrix it feeds. Record every slot's block-start output. Each ENV slot with a present route into VCA ticks per sample into a 64-sample stack buffer; every other ENV slot calls `advance(64)`. Each LFO slot processes once (CLASSIC exactly as today).
2. **Matrix,** as today, from the block-start values of the eight sources, over every destination but VCA. CUTOFF's offset uses the octave law.
3. **Engine → drive → filter,** the filter ramping `g` from the previous block's `fc` to this block's.
4. **Folder, then VCA:** pass-through `sample · volume` (Algo or Modal, no route), or `sample · volume · g[n]` with § 4's gain or VA's gate.
5. **Lifetime:** § 4's rule, checked once at the end of the block.

A fading voice (ADR 0027) keeps its last cutoff and routes, as it keeps `played`; its modulators keep running.

| Work | Rate | Paid when |
|---|---|---|
| ENV slot tick and VCA multiply-add | per sample | per ENV slot routed to VCA (A: one multiply-add and a compare) |
| SHAPE divide in that tick | per sample | a type-B slot in ENV mode with SHAPE off centre |
| Other VCA sources' ramp, multiply-add | per sample | per LFO, VEL or NOTE route into VCA |
| VA gate ramp and multiply | per sample | VA with no VCA route |
| Every other ENV slot, `advance(64)`, closed form | per block | always |
| LFO 1–3, CLASSIC or FUNC | per block | always (today: only LFO 1, and only when the matrix has ≥ 2 sources) |
| Slider laws (`fast_exp2` per slider), A's RC coefficients | per block | always |
| Output curve of a per-block B slot in ENV mode | per block | SHAPE off centre |
| `fast_tan` for the block's `g` | per block | always, as today |
| `fast_exp2` in `routed_cutoff` | per block | CUTOFF's sum ≠ 0 |
| `g` ramp | per sample, one add | `fc` changed |
| Matrix sum, 8 sources × up to 16 destinations | per block | as today, with 8 rows instead of 2 |
| VEL, NOTE | per note | always |

- An envelope precomputes its coefficients once per block, removing today's per-sample divide.
- `advance(n)` is closed form (A: the RC step raised to `n`; B: the linear `x` or φ) and carries overflow across stage ends, so it equals `n` ticks within 1e-6 at the block boundary; B's curve is applied once, at the read.
- `fast_exp2` and `fast_log2` are new in `dsp/mod.rs`: no libm; `fast_exp2` exact at integers and within 0.1 cent between them.

## CPU

- `Voice::cost = Engines::cost + CHAIN_COST + FilterKind::cost(kind, mode) + ModRouting::cost(p, mods)`.
- `ModRouting::cost` is estimated, rounded high:
  - a base of 8: six per-block modulators at up to about 60 cycles each (slider laws, a closed-form advance, one curve divide or CLASSIC's `fast_sin`), about 6 per sample; the eight-row matrix sum and `fast_exp2`, about 2;
  - plus, for each ENV slot routed to VCA: 6 for type A (divide-free); 10 for type B, and 14 more in ENV mode with SHAPE off centre (one `vdiv.f32` per sample);
  - plus 1 for each other route into VCA, and 2 for the VCA's clamp and multiply when any route exists.
  - Examples: an Algo or Modal Sound with the defaults, 8; VA with the defaults (ENV 2, type A, on VCA), 16; the worst case (all three ENV slots type B with curved SHAPE on VCA, five other VCA routes), 8 + 72 + 5 + 2 = 87.
- Today's per-sample amp-envelope tick and its per-sample divide (`voice.rs:301-302`), which the FLOOR row measured inside `CHAIN_COST`, go away: ENV 1 advances per block. `CHAIN_COST` is not lowered until the bench shows it.
- It is a function of the Sound, as `Engines::cost` is, so a light Sound keeps its voice count. The bench's MODS row replaces the estimate.
- `FilterKind::cost(Svf, _)` is 0 until measured: today's SVF runs inside the chain `CHAIN_COST` (10, from the FLOOR row) was measured over. The bench's SVF row (1 OP at PHASER, its costliest mode, minus the 1 OP row) settles it.
- Each model issue commits its own kind's cost from its own bench row, the way ADR 0026 bills engines.
- The FX diet leaves about 158 cycles per voice for the modulation modes on the costliest patch. The defaults take 8 (Algo, Modal) or 16 (VA); the worst case, 87, would cost a heavy patch voices, as ADR 0026 intends.
- A change that raises a Sound's cost (a KIND, a route into VCA, a slot's TYPE or SHAPE) can cut held notes (ADR 0026, #31).
- **RAM per voice (D2):** `played` grows by about 210 B (three `EnvParams` from 24 to about 56 B, `LfoParams` from one of 20 B to three of about 40 B, Filter and Out about 12 B); the modulator state by about 260 B (six slots of about 48 B, which covers B's phase, LFV's targets and slew, the burst position, the PRNG and the glide, against 28 B today); the ramps by about 60 B. About 530 B a voice, 3.2 KB for six: `Instrument` from 246,728 to about 250,000 B of 286,720, under the existing `const` assert. The VCA buffer is on the audio stack (256 B, DTCM).
- **RAM elsewhere (AXI):** each `ParamSnapshot` grows about 210 B and each `ModState` 16 B, in the SoundPool (32), the Performance's Sounds and the `AudioShared` triple buffer (3 × 6 Parts): about 12 KB against about 105 KB free (`AXI_RESIDENT` 419,044 of 524,288).

## Migration

Sounds live in RAM and the factory bank is code, so nothing on disk migrates. Every default reproduces today's sound:

| Old | New | Why nothing changes |
|---|---|---|
| `mode: u8` = 2 | `FilterMode::Lp24` (2) | Same discriminant, same code path |
| — | KIND SVF | Today's filter |
| `env_amount`, `key_track`, `fm_amount` (ids 3–5), never read | Deleted; ids retired; ENV and KEY become routes at 0 | Never read |
| An empty matrix in `Sound::init` | ENV 1, LFO 1 and NOTE → CUTOFF at 0 (and ENV 2 → VCA at 100 % on VA) | Amount 0 adds nothing and Σ = 0 gives `fc = base` bit for bit; no Algo or Modal Sound has a VCA route, so the VCA is today's pass-through |
| MORPH PAD's own registry and `ModState::from_registry(&reg, 2)` | It builds on the default `ModState` (eight sources, the CUTOFF column) and appends its MORPH column; its route is still source 1, LFO 1 | Only the column index and the source count change; the added sources and routes are 0 |
| `envelopes[0]`, the amp envelope | ENV 1, TYPE A, SPEED MED, HOLD AHDSR with H 0.001 ms; A, D, R as positions giving 10 ms, 300 ms, 300 ms | Its shape changes (RC instead of linear), it advances per block and outputs its raw contour; no Sound or golden routes source 0, so nothing audible changes |
| `envelopes[1..2]`, never read | ENV 2 (A), ENV 3 (B) | No Algo or Modal Sound routes them |
| `lfo` | `lfos[0]` CLASSIC; `lfos[1..2]` default | Same arithmetic; MORPH PAD's `lfo.rate` becomes `lfos[0].rate` |
| Matrix sources ENV, LFO | ENV1, LFO1 at indices 0, 1 | `ModState` indices unchanged; a two-source `ModState` still maps 0 → ENV1, 1 → LFO1 |
| FLD on the Algo chain | FLD / VCA (AMP) on the Algo and Modal chains | The folder already ran for both engines; only the page is new on Modal |
| CUTOFF routes linear (ADR 0010) | Octaves | No factory Sound routes CUTOFF |

- **Check:** before any change, record one audio golden per factory Sound (8). They stay bit-identical through this work.
- The `*_lfo_cutoff` goldens change: their LFO → CUTOFF route is now in octaves, and their cutoff ramps across each block instead of stepping. They are re-recorded after the sanity gate (ADR 0011). Every other audio golden stays bit-identical.
- Screen goldens that show the matrix (eight rows, the default CUTOFF column) and the Algo map (AMP) are re-recorded.

## UI

- **FLT:** KIND · then the kind's five knobs. Layout BigViz, viz FilterResponse. KIND lists built kinds only. A route knob shows its amount or "—" (§ 6).
- **FLT › MODE**, a new sub-page: MODE · extra · extra · — · — · —. Layout CellGrid. A single-mode kind shows MODE fixed, dimmed. Extras are the kind's (SVF: DRIVE, LFO); an empty extra is "—".
- **AMP (FLD / VCA),** the `FOLDER` def (id 9) renamed "Fold / VCA", short AMP: FOLD · SYM · MIX · VEL · — · —. VEL is dimmed on Algo and Modal with no VCA route. The Algo map reads ALG · OSC · DRV · FLT · AMP · MOD; the Modal map MDL · FLT · AMP · MOD.
- **Dimmed:** a fixed or inapplicable slot draws its label and value in `theme::MID` with no value bar. Its encoder is ignored, and MIX+PLUS on it reports "not modulatable" (ADR 0017).
- **The MOD node's pages,** one flat list (EDIT steps down, SEQ up; there is no second level): **E1 · E2 · E3 · SPD · L1 · L2 · L3 · MTX**. E1 (id 11, was ENVELOPE) becomes the node's own page, and the matrix (MOD_MATRIX, id 22) moves to the end of its sub-pages.
  - **Owner's change, 2026-09-28:** the matrix comes first, as the node's home: **MTX · E1 · E2 · E3 · SPD · L1 · L2 · L3**. The map label stays MOD.
  - An ENV page shows its TYPE's panel (§ 1): A · D · S / R · H · TYPE, or MODE · RISE · FALL / SHAPE · FORM · TYPE with MODE's and FORM's labels. Its Adsr viz draws the TYPE's shape (B: one cycle, burst or LFV stretch of its MODE and FORM).
  - **SPD** is one shared page for type A's switches, one parameter pair across slots, as the Algo group pages are: E1 SPEED · E2 SPEED · E3 SPEED / E1 HOLD · E2 HOLD · E3 HOLD, a column per slot under its pills (HOLD is HOLD POSITION: OFF, AHDSR, GATE EXT). A type-B slot's column is dimmed and inert. (Reordered 2026-09-28: pair by pair, the knobs under E2 drove E1 HOLD and E3 SPEED.)
  - An LFO page shows CLASSIC's or FUNC's panel (§ 1), TYPE in the last cell.
- **Page ids:** six new (FLT › MODE, E2, E3, SPD, L2, L3), taken in that order from the next free id at build time: 59 today, skipping 63 (the test page in `part_page_test.rs`). The VA spec also takes the next free id; whichever lands first takes 59.
- **Retired:** `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` (ids 13–15) and `ENVELOPE_CHAIN`. Their ids are not reused.
- **Slot binding:** new `SlotBinding::FilterPanel(u8)` and `SlotBinding::ModPanel(u8)` resolve against the Sound's KIND, or the slot's TYPE and MODE, as `SelectedOp` resolves against the selected operator. `slot_addr` takes a `SlotCtx { sel_op, kind, env_modes, lfo_modes }` in place of `sel_op`. A `Route` target resolves to a matrix cell, not a `ParamAddr`.
- **Vizzes read by address, not slot:** FilterResponse reads CUTOFF, RES and MODE; Adsr reads the TYPE's stages. Today's `a(0), a(1)` and `a(0..3)` would read KIND and the wrong stage.
- **Animation:** a KIND, TYPE or MODE change re-seeds the page's animators to the new slots' values. A lerp between two different parameters' values would draw a meaningless sweep.
- **Matrix:** eight rows, tagged E1 E2 E3 LF1 LF2 LF3 VEL NTE; VCA is a column like any destination. A present route at 0 draws as a route (its cell shows 0), an absent one as empty. MIX+MINUS on a cell deletes its route, and the matrix hint says so. A modulated CUTOFF's value bar uses the octave law. The UI's stand-in source values (`ui/mod.rs`) cover all eight: an A slot its SUS, a B slot ½; each CLASSIC LFO its own display LFO, a FUNC LFO 0; VEL 1; NOTE 0.

## Tests

- **Knobs are honest:** for every built kind, every FLT and FLT › MODE knob changes a render of a held saw (Algo and Modal) once its route is nonzero; an inapplicable filter parameter leaves it bit-identical.
- **Route knobs:** turning ENV on a Sound whose ENV 1 → CUTOFF was deleted creates the route; the knob shows "—" before; with 16 columns taken and no CUTOFF column it stays "—" and reports "matrix full"; the knob and the matrix cell always show the same amount.
- **Routed cutoff (pure):** Σ = 0 returns `base` bit for bit; NOTE at 100 % at notes 48, 60, 72, 84 gives base/2, base, 2·base, 4·base within 1e-6 relative; ENV at 100 % with the envelope at 1 is +10 octaves before the clamp; the clamp holds at both ends.
- **`fast_exp2`:** exact at −12..12; within 0.1 cent on a 1/1000 grid.
- **Clicks:** a CUTOFF sweep (ENV 1 attack 1 ms and 50 ms, route ±100 %, RES 0.5) has a largest second difference at most 1.5× that of the same render with the cutoff held at each block's mean.
- **Envelope A:** at each SPEED, the slider ends give the manual's times within one sample, and a full-swing stage takes its slider's time; HOLD OFF ignores H, AHDSR holds 1 for H, GATE EXT plays the full envelope from a one-sample gate; the per-sample step has no divide; `advance(64)` matches 64 ticks within 1e-6 across every stage boundary; release during attack and a note-on during release don't move the level; LEVEL with no route peaks at 1 and with VEL → LEVEL peaks at the velocity; TIME +100 % makes every stage 32 × shorter.
- **Envelope B:** ENV AD ignores key-up, AHR holds while held, CYCLE's period is RISE + FALL; SHAPE bottom, centre and top give log, linear and exp; LFO FREE's TILT bottom, centre and top give saw, triangle and ramp, and FALL offsets the phase 0°–360°; SYNC resets φ at each note-on (until #44); LFV stays within ±1, never steps by more than DELTA per cycle, and SLEW smooths it; BURST AD gives one burst LENGTH long, AHR sustains while held, CYCLE repeats while held; TILT bottom, centre and top put the loudest pulse at the start, middle and end; per-block B rates clamp at 93.75 Hz; RISE, FALL and SHAPE routes move their sliders and are inert on type A; a FUNC LFO matches B's LFO mode.
- **TYPE, MODE and FORM changes:** every change between A, B ENV, B LFO, B BURST, CLASSIC and FUNC, mid-note, with the key down and up: the level never steps by more than the glide allows (no step at all into A or B ENV).
- **LFO 1 CLASSIC** is bit-identical to today's LFO over 1,000 blocks.
- **Matrix:** eight sources, in order; a route from each moves its destination; a two-source `ModState` still maps 0 → ENV1 and 1 → LFO1; presence bits survive `sync_from_matrix`; a present route at 0 adds nothing.
- **VCA:**
  - no route, Algo and Modal: every factory Sound is bit-identical to its golden, and VEL changes nothing;
  - no route, VA: output while held, 0 exactly 64 samples after key-up, voice freed then; the gate edge passes the click test;
  - ENV 2 → VCA at 100 %: an Algo, a Modal and a VA note follow ENV 2's contour within 1 %; the voice frees at the end of the block in which ENV 2 goes idle;
  - ENV 2 → VCA at 50 %: half the level; silence and the voice's end when ENV 2 idles;
  - the SH-101 feel: ENV 1 → VCA and ENV 1 → CUTOFF, ENV 2 → VCA deleted: changing ENV 1's decay moves both the cutoff sweep and the level;
  - two routes sum and clamp at 1;
  - **no drone:** ENV 3 as B LFO, B ENV CYCLE and B BURST CYCLE → VCA, LFO 1 → VCA, and VEL → VCA each end the voice after key-up (CYCLE bursts after the running burst), through the 128-sample fade when the gain isn't 0; ENV 3 as B BURST AD → VCA ends the voice after its burst, key held or not;
  - lifetime: under every configuration an inactive engine ends the voice;
  - VEL: at 0 two velocities give the same level; at 100 % the level scales with velocity;
  - the fold comes before the VCA: with FOLD on, halving ENV 2's amount halves the output exactly.
- **Defaults:** `Sound::init` for Algo and Modal has exactly the three CUTOFF routes at 0; for VA, those and ENV 2 → VCA at 127.
- **Kinds and modes (pure, over every built kind):** `modes()` non-empty; `mode ∈ kind.modes()` after every `set_kind` and `set_mode`; a single-mode kind can't be moved off its mode; `applies()` matches the panel; a KIND change leaves the `ModState` byte-identical.
- **Shortcut:** the 303's DECAY edits ENV 1's DECAY and dims when ENV 1 is B (with #127).
- **Cost:** `Voice::cost` includes `ModRouting::cost` and `FilterKind::cost`; the allocator's voice count for each factory Sound is recomputed with 8 added, and `cost_test` pins the new counts.
- **RAM:** the existing `const` asserts hold (`[Voice; MAX_VOICES]`, `Instrument`, `AXI_RESIDENT`).
- **UI:** new screen goldens: FLT (SVF), FLT › MODE (SVF), AMP (VEL dimmed and live), ENV pages at A and at each B MODE and FORM, SPD with one B slot dimmed, LFO pages at CLASSIC and FUNC, the matrix with eight rows; the Algo and Modal maps with AMP. The dimmed readout gets its golden with the first single-mode kind (#123).
- **Goldens:** the 8 factory goldens bit-identical; the two `*_lfo_cutoff` re-recorded; all other audio goldens unchanged.
- **Bench:** a MODS row (1 OP, ENV 2 type B in ENV mode with SHAPE off centre → VCA, ENV 1 → CUTOFF, every source routed somewhere, all three LFOs FUNC) and an SVF row (1 OP, PHASER), each minus the 1 OP row.

## ADRs

- **New: every connection is a matrix route.** The six-slot modulator pool; the eight sources in that order; route presence; the VCA destination (sum and clamp, per sample, the per-engine rule with no route, the lifetime rule and its fade); CUTOFF in octaves (supersedes ADR 0010's linear law for CUTOFF only); the default routes; filter knobs as route views; KIND never edits the matrix. It supersedes in part ADR 0022's note that no engine puts the amp envelope on the VCA. The VA spec's ADR 0033 then points here instead of superseding that note itself.
- **New: Envelope A, Envelope B and the LFO types.** Cascadia provenance (manual v1.2: the behaviour and ranges, not code); A's SPEED ranges, HOLD POSITION and fixed RC shape; B's MODE, FORM and slider mappings; the ENV n LEVEL, TIME, RISE, FALL, SHAPE destinations; FUNC; the TYPE-change rules.
- **New: KIND lays out the panel.** Per-kind panels as `const` data; route knobs as views; shortcuts; MODE follows KIND. ParamIds per the table; ids 3–5 retired (ADR 0009).
- The model choice, topologies and oversampling policy are #128's other ADR.

## The VA spec

`docs/superpowers/specs/2026-09-27-va-engine-design.md` says VA's VCA source defaults to ENV 1 and offers ENV 1 and GATE (its § Storage bullet "The VCA ends a VA Part's notes", ADR 0033's bullets, and its tests). Under this spec there is no VCA source: VA's default Sound has the route ENV 2 → VCA at 100 %, and a VA Sound with no VCA route gates (§ 4). The audible result and the lifetime are the same as that spec intends, and no engine branch is needed beyond the no-route rule. That spec needs an edit: "ENV 1" there becomes "ENV 2 → VCA", "VCA source ENV 1 or GATE" becomes "a VCA route, or the gate with none", and ADR 0033 drops its own supersession of ADR 0022's VCA note in favour of this spec's routing ADR. Its "only the amp envelope retriggers" should read "only the modulators retrigger": all six restart per their SYNC on a note-on, and the oscillators keep their phases as it says.

## Out of scope

- Each filter model's DSP (topology, references, cost targets, KIND crossfade): #123–#127, specified under #128 before it is built.
- Velocity-scaled filter depth (route amounts are not modulatable). VEL → ENV 1 LEVEL scales ENV 1 itself, which gives it for any route from ENV 1.
- Modulating envelope or LFO parameters beyond ENV n LEVEL, TIME, RISE, FALL and SHAPE, or route amounts.
- The Cascadia's stage outputs (EOA, EOH, EOF) as matrix sources.
- Audio-rate filter FM.
- A second filter, or filter routing in series or parallel.

## Open questions

1. **LFO-to-envelope sync:** whether an LFO slot can restart from, or be gated by, an envelope (or the reverse). Undecided; not designed here.
2. **SYNC with a clock:** Envelope B's LFO SYNC waits for the Tempo epic ([#44](https://github.com/joegiralt/chimera/issues/44)); until then it resets at each note-on.

## Risks

- **VCA as a sum:** tremolo on top of an envelope clamps at 1 and leaks after release until the voice ends (§ 4). If users want scaling tremolo, a later ADR can add a multiplicative "VCA MOD" destination; the sum stays for VCA.
- **The fade at a non-ending VCA source's key-up** cuts an LFO- or VEL-gated note 2.7 ms after key-up. That is the no-drone guarantee's price.
- **The matrix is at its source cap.** Any further source (a second velocity curve, aftertouch, an S&H bus) raises `MAX_MOD_SOURCES`.
- **The i8 amount on CUTOFF** is 10/127 ≈ 0.08 octaves a step, about a semitone. Coarse for a slow filter sweep's depth; if heard, CUTOFF's law or the amount's width changes in a later ADR.
- **GATE's 64-sample edges** on a VA with no route soften the attack by 1.3 ms and may still click on a loud low note. The click test decides; the ramp length is one const.
- **An envelope on an Algo VCA multiplies two envelopes** (the operators' and ENV 2). That is the point of the route, but a user may expect it to replace them.
- **The ramp is linear in `g`,** not in log-frequency. A 10-octave sweep inside one block bends toward the top. If a test hears it, split the block into four 16-sample ramps (four `fast_tan`s per block).
- **The CUTOFF offset lags one block** (block-start values, ramped from the previous block's). 1.3 ms, inaudible on a filter; the VCA, which would hear it, is per sample.
- **A curved type-B envelope on the VCA** costs a divide per sample (14 cycles). Three of them cost 42 of the ~158 left per voice. Type A is divide-free.
- **Per-block B rates clamp at 93.75 Hz**, far below the Cascadia's 800 Hz LFO and 1 kHz bursts; only a slot on the VCA runs the full range. Audio-rate modulation of other destinations would need per-sample routing, which this spec doesn't build.
- **A's times now follow SPEED's exponential sliders,** so ENV 1's stored seconds become positions. Nothing audible reads ENV 1 today, but any test that sets `EnvParams` in seconds changes.
- **The FLOOR row's 5 cycles look low** for a two-stage SVF with a per-sample divide. The SVF row checks whether `CHAIN_COST` really covers it; if not, the SVF's cost joins every voice's bill.
- **`played` grows** in every voice (ADR 0027) and in the per-frame publish: about 210 B, under budget but not free.

## Defaults chosen

The owner's decisions didn't settle these; each is a default until the owner says otherwise.

1. **VCA is the sum** of its routes, clamped 0..1, per sample, times AMP's VEL term; not a product (§ 4 has the reasons).
2. **Lifetime with VCA routes:** the voice ends when no VCA source holds it: type A, or B in AD or AHR, until idle; B in CYCLE or LFO, an LFO slot, VEL or NOTE only while the key is held (a CYCLE burst finishes); if the gain isn't 0 then, it ends through ADR 0027's 128-sample fade. This is the no-drone rule for a type-B slot cycling on the VCA.
3. **KIND never edits the matrix,** not even on a new Sound; the SH-101 one-envelope feel is the user's two edits. A kind's key-tracking default seeds NOTE → CUTOFF only on a new Sound (0, SVF).
4. **Default routes:** ENV 1, LFO 1 and NOTE → CUTOFF at 0 on every engine; ENV 2 → VCA at 127 on VA only.
5. **Route presence** is a bit per cell, apart from the amount, so a route at 0 and a deleted route differ; MIX+MINUS on a matrix cell deletes a route; a route knob shows "—" for a deleted one, and turning it creates the route (and the CUTOFF column if there's room).
6. **CUTOFF routes are in octaves:** 127 = 10 octaves; NOTE = (note − 60)/120, so NOTE at 127 is one octave per octave; the Prophet's HALF is 64 (0.504).
7. **ENV slots output without velocity;** velocity reaches the sound through VEL (a source), AMP's VEL and ENV n LEVEL. `EnvParams.vel_sens` stays unread and off the pages (#112).
8. **Default TYPEs:** ENV 1 A, ENV 2 A, ENV 3 B (MODE ENV, FORM AD); LFOs CLASSIC (FUNC's default FORM FREE). Any ENV slot can be A or B.
9. **Slider law:** every time or rate slider is a position 0..1 with `q = q_min · (q_max/q_min)^p`; a matrix route adds to the position, so time and rate routes act in octaves.
10. **Type A's shape** is a fixed RC curve: attack aims at 1.3 and stops at 1; decay and release aim 0.01 past their end (S, 0). The time constants make a full swing take exactly the slider's time. Per sample it is one multiply-add, divide-free.
11. **Type A's defaults:** SPEED MED, HOLD AHDSR, H at 0 (0.001 ms, so ADSR in effect), A 10 ms, D and R 300 ms, S 0.7: today's default times.
12. **ENV n LEVEL** takes base 0 when routed, like the VCA, so a route sets the peak (the Cascadia's CTRL LEVEL); unrouted, the peak is 1 (CTRL OFF). **ENV n TIME** scales every stage by `2^(−5·Σ)`: +100 % is 32 × shorter, matching CTRL TIME's "higher is shorter".
13. **Type B's ranges,** from the manual: ENV RISE and FALL 2 ms–5 s; LFO RATE 0.05–800 Hz; BURST RATE 0.05 Hz–1 kHz and LENGTH 10 ms–20 s; PHASE 0°–360°; DELTA and SLEW 0–1. Defaults: RISE 10 ms, FALL 300 ms, SHAPE centre (ENV); FUNC RATE 1 Hz, PHASE 0, TILT centre (triangle).
14. **SHAPE in ENV mode:** `w = 2^(4·(2·SHAPE − 1))`, rising `L = x/(x + (1 − x)·w)`, falling `L = 1 − f(1 − x)`; bottom log, centre linear (no divide), top exp; at the ends 94 % / 6 % of a swing at half its time.
15. **LFO FREE tilt:** the rise fraction is the SHAPE position; bottom saw (instant rise), centre triangle, top ramp (instant fall).
16. **LFV:** a clamped random walk of per-cycle targets `t_k = clamp(t_{k−1} + DELTA · r_k, −1, 1)`, linear between targets, then a one-pole slew of SLEW × one cycle.
17. **BURST:** the burst envelope is a triangle peaking at TILT × LENGTH; pulses are square at TILT's ends and sine at its centre under AD and AHR, the tilting saw under CYCLE. CYCLE repeats while the key is held and finishes the running burst at key-up.
18. **FREE, LFV and BURST phases are not reset by a note-on** (the Cascadia resets them on its gate by default); SYNC resets at each note-on until #44 gives it a clock. This keeps FREE and SYNC different before #44.
19. **Per-block B rates clamp at 93.75 Hz** (block rate ÷ 8); on the VCA, per sample, the full ranges run.
20. **TYPE, MODE and FORM changes:** into A or B ENV the new shape enters at the current level; into anything else the difference glides to 0 over 256 samples.
21. **LFO pages:** TYPE in the last cell, as on the ENV pages. CLASSIC: RATE · SHAPE · SYNC / PHASE · DEPTH · TYPE; OFFSET leaves the page and stops being applied (redundant with the destination's knob; no Sound sets it). FUNC: B's page with MODE locked to LFO; DEPTH not applied (the route amount is the depth).
22. **Rates:** an ENV slot ticks per sample only while routed to VCA; everything else per block; all six modulators always run.
23. **AMP:** short label AMP, name "Fold / VCA", VEL default 100 %, dimmed under the Algo/Modal pass-through. VA's no-route gate has 64-sample linear edges.
24. Route knobs show the matrix amount, −100..+100 %, KEY included. One bipolar amount per route on every kind, even where the original panel was unipolar.
25. The matrix always applies; "not shown, not applied" covers only the filter's own parameters.
26. **Matrix source order** ENV1, LFO1, ENV2, ENV3, LFO2, LFO3, VEL, NOTE, keeping indices 0 and 1; it fills `MAX_MOD_SOURCES` (8).
27. TB-303's DECAY edits ENV 1's DECAY and dims when ENV 1 is B.
28. Lifetime is checked once per block; an inactive engine ends the voice in every configuration.
29. SVF's MODE default is LP24 (today's), listed first; SVF's extras are DRIVE and LFO, on FLT › MODE.
30. Moog panel CUTOFF · RES · ENV · KEY · DRIVE; Prophet's KEYBD a stepped view (0, 64, 127).
31. FM, ENV and KEY's filter ids 3–5 are retired, never reused; the first version's ids 9–10 were never built and stay free.
32. Only ENV n LEVEL, TIME, RISE, FALL and SHAPE are modulatable among envelope and LFO parameters; route amounts are not.
33. `FilterKind` gains each variant with its model; there are no stand-in kinds.
34. Six new page ids from the next free (59 today, skipping 63); retired ids 13–15 are not reused.
35. `BlockRef::Env(EnvSlot)` and `BlockRef::Lfo(LfoSlot)` replace the role-named envelope refs and the single LFO ref.
