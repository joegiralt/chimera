# Filter Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the FLT page honest and route every modulator connection through the matrix. Every voice gets six modulators: ENV 1–3, each a Cascadia-style Envelope A or B, and LFO 1–3, each CLASSIC or FUNC. The matrix gets eight sources. CUTOFF routes in octaves. The VCA becomes a destination with a lifetime rule, FLD / VCA (AMP) goes on every Part chain, and the KIND panel machinery arrives with the SVF as its only kind.

**Architecture:** The modulators are a functional core in `dsp/modulator/`: the slider laws (`law.rs`), Envelope A (`env_a.rs`) and Envelope B (`func.rs`, `FuncGen`), all pure and host-tested. `dsp/envelope.rs` (`Envelope`) and `dsp/lfo.rs` (`Lfo`) are thin slot shells: they pick a TYPE, apply the matrix inputs and glide across a TYPE change. `Voice` runs the six slots once per block. It ticks per sample only the ENV slots routed to the VCA, sums the matrix with presence bits (`modulation.rs`), applies CUTOFF in octaves through `ParamSpec`'s offset law, and ends the voice by the VCA's routes. On the UI side, `ui/view.rs` resolves a page slot (a fixed param, a filter-panel knob, a modulator-panel cell or a route view) against the Sound, so pages, vizzes and encoders all read by address.

**Tech Stack:** Rust 2024 (`no_std` core, `f32` only in DSP), embedded-graphics with u8g2 ASCII fonts, the DWT cycle counter on the chip, and `just`.

**Spec:** `docs/superpowers/specs/2026-09-27-filter-routing-design.md` (binding). Epic https://github.com/joegiralt/chimera/issues/120; this plan closes #121, #140, the routing part of #122, #111, #112's filter and envelope items, #57 (doc) and #53 for the filter. #48 (the saturator) is deferred to the filter-model work: its fix changes high-resonance levels, which needs a deliberate re-record.

> **Out of scope.** The filter MODELS (Moog #123, MS-20 #124, SH-101 #125, Prophet-5 #126 and TB-303 #127) are not built here: each needs its own spec under #128 (topology, references, cost, the KIND crossfade) before it is built. This plan builds only the machinery a model plugs into. The VA engine does not exist on `main` (#118), so its no-route gate and its default ENV 2 → VCA route are https://github.com/joegiralt/chimera/issues/148; the no-route rule here is an exhaustive `match` on the engine, so VA cannot compile without its gate.

## Global Constraints

Every task's requirements include this section. Numbers are the spec's, verbatim, except where the owner's rulings on the plan review override them (marked **Ruling**).

**Modulator pool (spec § 1)**
- Every voice runs six modulators, always: ENV 1, ENV 2, ENV 3 (`EnvSlot`) and LFO 1, LFO 2, LFO 3 (`LfoSlot`). There is no add or remove, and the MOD page lists all six. A slot is wired to nothing: it drives what the matrix routes from it.
- Default TYPEs: ENV 1 A, ENV 2 A, ENV 3 B (MODE ENV, FORM AD); LFO 1–3 CLASSIC. Any ENV slot can be A or B; any LFO slot CLASSIC or FUNC.
- Types A and B follow the Intellijel Cascadia's Envelope A and Envelope B (manual v1.2, 2023-10-15, pp. 28–39 and 82–97): its behaviour and ranges, none of its code.
- **Outputs:** an ENV slot outputs 0..1 (A; B in ENV and BURST) or −1..1 (B in LFO), without velocity. An LFO slot outputs −1..1.
- A value a TYPE, MODE or FORM doesn't use is kept, unread and unshown, so switching back restores it.
- **Sliders are positions.** A time or rate slider stores a position `p` in 0..1, and its quantity follows `q = q_min · (q_max / q_min)^p`, one `fast_exp2` per slider per block. A matrix route adds to `p` (linear law, clamped 0..1).
- **Envelope A:** H, A, D and R are positions; S is a level, 0..1. Attack to 1; Hold (per HOLD POSITION); Decay to S; Sustain while the gate is high; Release to 0 when it goes low, from whatever stage is running. A note-on restarts Attack from the current level. A slider's time is the stage's time from its start level to its end level at full swing.
- **HOLD POSITION:** OFF: a plain ADSR, H ignored. AHDSR (default): after the attack, hold at 1 for H, then decay. GATE EXT: the gate is the key gate OR a gate H long from the note-on; H adds no stage.
- **SPEED** (default MED):

| SPEED | H | A | D and R |
|---|---|---|---|
| FAST | 0.001 ms – 2.5 s | 0.2 ms – 1.5 s | 0.6 ms – 2.5 s |
| MED | 0.001 ms – 10 s | 2 ms – 10 s | 3.5 ms – 10 s |
| SLOW | 0.001 ms – 60 s | 9.3 ms – 60 s | 30 ms – 60 s |

- **A's shape:** `L += c · (T − L)` per sample, `c = 1 − 2^(−1/(τ·fs·ln 2))` computed once per block. Attack aims at T = 1.3 and ends at 1: τ = A / ln(1.3 / 0.3) = A / 1.466. Decay aims at T = S − 0.01 and ends at S; Release aims at T = −0.01 and ends at 0: τ = time / ln(1.01 / 0.01) = time / 4.615. Per sample: one multiply-add and a compare. `advance(n)` is closed form, `L_n = T + (L − T)·2^(−n/(τ·fs·ln 2))`, with a `fast_log2` to place a stage end inside the block. **Ruling (supersedes the spec's "within 1e-6"):** the per-sample path and `advance` each match an f64 reference within 1e-4 absolute, with stage changes within ±1 sample (ADR 0036).
- **ENV n LEVEL:** with no route into it, the envelope peaks at 1; with a route, its peak is `clamp(Σ, 0, 1)`. **ENV n TIME:** every stage's time (H, A, D, R) is scaled by `2^(−5·Σ)`. Both are read per block and are inert on a type-B slot.
- **Envelope B:** MODE ENV, LFO or BURST; FORM CYCLE, AHR or AD in ENV and BURST; FREE, SYNC or LFV in LFO.

| MODE, FORM | RISE | FALL | SHAPE |
|---|---|---|---|
| ENV (AD, AHR, CYCLE) | rise time, 2 ms – 5 s | fall time, 2 ms – 5 s | curvature: log · linear · exp |
| LFO FREE | RATE, 0.05 – 800 Hz | PHASE, 0° – 360° | TILT: saw · triangle · ramp |
| LFO SYNC | RATE until #44 | PHASE | TILT |
| LFO LFV | RATE, 0.05 – 800 Hz | DELTA, 0 – 1 | SLEW, 0 – 1 |
| BURST (AD, AHR, CYCLE) | pulse RATE, 0.05 Hz – 1 kHz | LENGTH, 10 ms – 20 s | TILT of the burst and its pulses |

- **SHAPE (ENV mode):** `s = 2·SHAPE − 1`, `w = 2^(4·s)`; rising `L = f(x)`, falling `L = 1 − f(1 − x)`, `f(x) = x / (x + (1 − x)·w)`; centre is linear and skips the divide; `f⁻¹(y) = w·y / (1 − y + w·y)`.
- **LFO FREE tilt:** `r` = TILT position, `u = φ/r` for φ < r, `(1 − φ)/(1 − r)` after (r = 0: `1 − φ`; r = 1: `φ`), out = `2u − 1`. SYNC resets φ at each note-on until #44.
- **LFV:** `t_k = clamp(t_{k−1} + DELTA · r_k, −1, 1)`, `r_k` uniform in −1..1 from the slot's PRNG, linear from `t_{k−1}` to `t_k` across the cycle; SLEW is a one-pole low-pass of time constant SLEW × one cycle.
- **BURST:** output 0..1; the burst envelope rises over p·LENGTH and falls over (1 − p)·LENGTH (p = TILT); pulses under AD and AHR are `(1 − m)·square + m·sine`, `m = 1 − |2p − 1|`, `sine = ½ − ½·cos 2πφ`; under CYCLE, the tilting saw, unipolar. AD: one burst per note-on; AHR: rises, holds at its peak while the key is held, falls after key-up; CYCLE: repeats while held, the running burst finishes at key-up.
- **Rates:** a B slot routed to VCA runs per sample with the manual's full ranges; a B slot evaluated per block clamps any rate to the block rate ÷ 8. **Ruling:** the clamp is derived, `sample_rate / BLOCK_SIZE / 8` (93.75 Hz at 48 kHz, 86.1 Hz at 44.1 kHz), and it covers LFO RATE, BURST's pulse RATE and the repeat rates of ENV CYCLE (`1 / (RISE + FALL)`) and BURST CYCLE (`1 / LENGTH`); one-shot AD and AHR times are not clamped.
- **TYPE, MODE or FORM change mid-note:** into A with the key up, Release from the current level; with the key down, from a rising segment Attack, else Decay if L > S, else Sustain at L. Into B ENV: rising → rise from the current level, else fall from it. Into anything else, the difference `d` glides linearly to 0 over 256 samples.
- **LFO slots:** CLASSIC is today's LFO, its value taken before the block's advance with today's arithmetic, so LFO 1 is bit-identical; OFFSET is stored but no longer applied. FUNC is Envelope B locked to LFO mode, FORM FREE, SYNC or LFV (default FREE); DEPTH is not applied under FUNC.

**Matrix (spec § 2, § 3)**
- Sources, in this order, filling `MAX_MOD_SOURCES` (8): 0 ENV1, 1 LFO1, 2 ENV2, 3 ENV3, 4 LFO2, 5 LFO3, 6 VEL (the note's velocity, 0..1), 7 NOTE ((note − 60) / 120, clamped to −1..1). Tags E1, E2, E3, LF1, LF2, LF3, VEL, NTE (≤ 3 characters, #15).
- **Route presence:** `ModState` and `MatrixState` gain `present: [u8; MAX_MOD_DESTS]`, one bit per source, copied by `sync_from_matrix`. Turning an empty cell creates the route; turning a route to 0 keeps it; MIX+MINUS on a matrix cell deletes it. `set_amount` with a nonzero amount sets the bit. The audio thread's sums ignore the bits.
- New destinations: CUTOFF sums in octaves; VCA (`ParamAddr(Out, OutParams::VCA)`); ENV n LEVEL, TIME, RISE, FALL, SHAPE for n = 1–3.
- **CUTOFF:** `fc = clamp(base · 2^(CUTOFF_OCTAVES · Σ), 20 Hz, min(20 kHz, 0.49·fs))`, `CUTOFF_OCTAVES = 10`. When Σ is 0, `fc` is `base` bit for bit (no `exp2` runs). The filter ramps `g` linearly across the block from the previous block's `fc` to this block's; on a fresh note there is no ramp; when the two are equal, `g` is held as today.
- **Default routes** in every new Sound: ENV 1 → CUTOFF 0, LFO 1 → CUTOFF 0, NOTE → CUTOFF (the kind's key default, SVF 0), on every engine. (ENV 2 → VCA at 127 on VA only: deferred with VA.)
- **KIND never edits the matrix.**

**VCA (spec § 4)**
- VCA's value is `clamp(0 + Σ, 0, 1)`. Per sample: `g[n] = clamp(Σ_ENV routes aᵢ · eᵢ[n] + Σ_other routes aⱼ · lerp(prevⱼ, curⱼ, n/64), 0, 1) · vel`, `vel = 1 − VEL + VEL · v`. **Ruling:** an ENV slot's LEVEL peak is ramped per sample across the block on this path, like the other sources, so an LFO → LEVEL route doesn't zipper the VCA.
- No route into VCA on Algo and Modal: `sample · volume`, today's expression, bit for bit; AMP's VEL dimmed and unread. The rule is an exhaustive `match` on `EngineType` (VA's gate: #148).
- **Lifetime:** with routes into VCA, the voice ends at the end of the first block in which no routed source holds it or the engine is inactive. A type A slot, or B with FORM AD or AHR, holds it until idle; a B slot in CYCLE or LFO holds it only while the key is held (BURST CYCLE: until the burst running at key-up ends); an LFO slot, VEL or NOTE holds it while the key is held. If the gain isn't 0 then, it ends through ADR 0027's fade (`FADE`, 128 samples). An inactive engine always ends the voice.

**Pages (spec § 5–7, § UI)**
- AMP: the `FOLDER` def (id 9) renamed "Fold / VCA", short AMP: FOLD · SYM · MIX · VEL · — · —; last before MOD on the Algo and Modal chains. VEL is 0–100 %, default 100 %.
- FLT: KIND · then the kind's five knobs; SVF: CUTOFF · RES · MODE · ENV · KEY; FLT › MODE: MODE · DRIVE · LFO. Route knobs show the route's amount, −100..+100 %, or "—" when absent; turning an absent route creates it (and the CUTOFF column if there is room); with all 16 columns taken it reports "matrix full".
- SVF modes: LP24, LP6, LP12, BP12, BP24, HP24, NOTCH, PHASER (LP24 first, the default). `mode ∈ kind.modes()` always holds.
- MOD node sub-list: E1 · E2 · E3 · SPD · L1 · L2 · L3 · MTX. ENV page A: A · D · S / R · H · TYPE; B: MODE · RISE · FALL / SHAPE · FORM · TYPE. SPD: E1 SPEED · E1 HOLD · E2 SPEED / E2 HOLD · E3 SPEED · E3 HOLD. LFO CLASSIC: RATE · SHAPE · SYNC / PHASE · DEPTH · TYPE; FUNC: MODE (LFO, fixed) · RATE · PHASE or DELTA / TILT or SLEW · FORM · TYPE.
- New page ids in this order: FLT › MODE 59, E2 60, E3 61, SPD 62, L2 64, L3 65 (63 is the test page in `part_page_test.rs`). Retired: `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` (ids 13–15) and `ENVELOPE_CHAIN`; their ids are not reused.

**ParamIds (spec § ParamIds; ADR 0009: never reused)**
- Filter: CUTOFF 0 (`Octaves(10)`), RES 1, DRIVE 2, FM 3 **retired**, ENV 4 **retired**, KEY 5 **retired**, KIND 6, MODE 7.
- Env n: A 0, D 1, S 2, R 3 (A, D, R positions: defaults A 0.189, D and R 0.559; S 0.7), LEVEL 4 (hidden destination, default 1), VEL 5 (unread), H 6 (default 0), TYPE 7, SPEED 8, HOLD 9, TIME 10 (hidden destination, 0), MODE 11, FORM 12, RISE 13 (0.206), FALL 14 (0.640), SHAPE 15 (0.5). Only LEVEL, TIME, RISE, FALL and SHAPE are modulatable.
- LFO n: 0–5 as today (OFFSET 5 no longer applied), TYPE 6, FORM 7, RISE 8 (0.309), FALL 9 (0), SHAPE 10 (0.5); none modulatable.
- Out: VCA 2 (hidden, 0..1, 0, modulatable), VEL 3 (0..1, default 1).

**CPU and RAM (spec § CPU)**
- `Voice::cost = Engines::cost + CHAIN_COST + FilterKind::cost(kind, mode) + ModRouting::cost(p, mods)`.
- `ModRouting::cost` has the spec's shape: a base, plus per ENV slot routed to VCA a term for type A or type B (and more for B in ENV mode with SHAPE off centre, or with an ENV n SHAPE route that can move it), plus a term per other VCA route and one for the clamp and multiply. **Ruling:** the model must never undercount, so until the bench measures each term (Task 13) it bills the plan review's estimates, not the spec's: BASE 30, ENV_A 30, ENV_B 40, CURVE 15 more, OTHER 3, CLAMP 3. Examples: an Algo or Modal Sound with the defaults, 30; ENV 2 (type A) on the VCA, 63; the worst case (three curved B slots and five other sources on the VCA), 30 + 3 + 3·55 + 5·3 = 213. The costliest patch (842) then bills 872, and 6 × 872 + 1,360 = 6,592 ≤ 7,000 keeps six voices on rev V.
- `FilterKind::cost(Svf, _)` is 0 until the bench's SVF row (1 OP at PHASER, minus the 1 OP row) settles it. `CHAIN_COST` is not lowered until the bench shows it.
- RAM: `Instrument` from 246,728 to about 258,000 B of 286,720 (the per-slot coefficient caches, a `FuncGen` in each ENV and LFO slot and `Lfo`'s FUNC state add about 1.5 KB per voice); the VCA buffer is on the audio stack (256 B). The existing `const` asserts hold (`[Voice; MAX_VOICES]`, `Instrument`, `AXI_RESIDENT`).

**Migration and goldens**
- Before any change, one audio golden per factory Sound (8). They stay bit-identical through this work.
- The `*_lfo_cutoff` goldens change and are re-recorded after the sanity gate (ADR 0011) in the task that changes them, Task 3. **Ruling:** nothing is parked. Every other audio golden stays bit-identical.
- Every task ends green: each task lists every existing test it turns red and that test's new expectation.
- Screen goldens that show the matrix and the Algo map (AMP) are re-recorded, plus the new pages' goldens.

**Project rules**
- No heap allocation in audio; the audio thread never blocks, never allocates.
- No `unsafe` without a `// SAFETY:` comment.
- No libc; no libm and no `f64` per sample in DSP (`f64` is fine in host tests; libm per block or in the UI is allowed).
- Parameter changes are lerped in the UI; nothing snaps.
- ParamIds are never reused (ADR 0009).
- Bugs, known issues and follow-ups go to GitHub issues (joegiralt/chimera), referenced by URL, never into repo files.
- Decisions that constrain future work get an ADR in `docs/adr/` (template `0000-template.md`, listed in `docs/adr/README.md`); an accepted ADR is never edited except for its status in the README row.
- Commit messages are plain: no `fix:`/`feat:`-style prefix, no Co-Authored-By, and never any Claude attribution. Comments are terse, only where needed.
- **Never stage `docs/chimera-ui-ux-spec.md`.** Stage files by name in every commit; never `git add -A` or `git add .`.
- Build and test with `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`, written `just check` below. Run it exactly as given: its stack check reads `./target`. Every single `cargo` command below (a focused test run) is prefixed `CARGO_TARGET_DIR=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/wt-fr-target`, written `$T` below (`export T=...` once per shell). Run `cargo fmt --all` before `just check`.

## Review Focus

Inputs the spec implies but no spec test covers, most likely to bite first. Each has its test in the named task.

1. **Several routes into one destination summing far past ±1** (three sources at +127 into CUTOFF, or into TIME). `exp2` must not overflow to a non-finite `fc` or time; CUTOFF clamps to 20 Hz..0.49·fs and TIME clamps Σ to ±1. Task 3, `routed_cutoff_survives_huge_sums`; Task 5, `time_route_sums_clamp`.
2. **The desktop at 44.1 kHz** (cpal picks the device rate). Envelope A's times must follow `fs` (a MED 10 ms attack takes 441 samples, not 480), and the per-block rate clamp follows it (`block_rate_max`: 86.1 Hz there). Task 5, `stage_times_follow_the_sample_rate`; Task 6, `per_block_rates_stop_at_an_eighth_of_the_block_rate` (run at 48 and 44.1 kHz).
3. **TYPE, MODE or FORM spun one step every block** (a fast encoder). Glides must not stack past the output range, and the output must stay finite and within ±1. Task 6, `spinning_type_every_block_stays_bounded`.
4. **A route into VCA at a negative amount, or from a negative source** (LFO 1 → VCA at −127). The gain clamps at 0; the output never inverts. Task 10, `a_negative_vca_route_never_inverts`.
5. **The Sound changes engine while a VCA-routed voice sounds.** The fade keeps the old routes (not the new Sound's pass-through), stays finite, and the held note restarts on the new engine. Task 11, `an_engine_switch_fades_with_the_old_vca_routes`.

## Decisions this plan makes where the spec is silent

The executor must not re-decide these; the reviewer checks them against the spec.

| Decision | Value | Where |
|---|---|---|
| `fast_exp2`, `fast_log2` | re-exports of `algo::math::exp2` and `log2` (exact at integers, rel. error < 1e-6; abs. error < 3e-5) | Task 3 |
| CUTOFF offset law | `ParamSpec.law: OffsetLaw { Linear, Octaves(f32) }`; `ParamSpec::offset(v, off)` is the one formula, used by `apply_offset`, `routed_cutoff` and the UI's mod bars | Task 3 |
| Cutoff ramp | `SvfFilter` keeps the last block's `g` (`None` on a fresh voice); the ramp reaches the new `g` on the block's last sample | Task 3 |
| Phase-1 sources | the matrix has its eight rows from Task 4; ENV 2, ENV 3, LFO 2 and LFO 3 output 0 until Tasks 5 and 7 run them, so the first shippable slice (#121) ends after Task 7 | Tasks 4–7 |
| Route knob step | one amount unit (1/127) per detent; MIX + turn snaps to −127, 0, +127 | Task 4 |
| Route knob readout | `ValFmt::Route`: the amount as a percentage, `+47%`; an absent route draws a dash where the value goes, since the u8g2 `_tr` fonts have no "—" | Tasks 4, 8 |
| Missing fonts glyphs | "°", "·" and "→" are not in the `_tr` fonts: PHASE shows plain degrees (`90`) and the ENV page title reads `ENV 1 / A`. The mockup's route caption is not built | Tasks 16, 17 |
| Hidden destinations | **Owner's decision:** VCA, ENV n LEVEL and ENV n TIME are on no page, so MIX+PLUS on their owning cell primes them: AMP's VEL → VCA, even while VEL is dimmed (the one exception to the dimming rule, ADR 0037); an A page's A, D, R and H cells → TIME; S → LEVEL | Tasks 9, 14; ADRs 0035, 0037 |
| FORM types | **Ruling:** `EnvForm { Ad, Ahr, Cycle }` (ENV and BURST) and `LfoForm { Free, Sync, Lfv }`, default first; `FuncParams` holds `env_form`, `burst_form` and `lfo_form`, so each MODE keeps its FORM; `Func { Env(EnvForm), Lfo(LfoForm), Burst(EnvForm) }` is what B runs, so no MODE/FORM mismatch is representable | Task 6 |
| MODE and FORM as `Block` values | `get`/`write` use the index in `kind.modes()` / the MODE's FORM enum, so an encoder steps the kind's own list; the typed `set_mode`/`set_kind`/`set_func` keep the invariant | Tasks 2, 6, 15 |
| Sustain below S | Sustain holds its level; a lowered S sends a level above it back to Decay; a raised S does not lift it, in Sustain or mid-Decay (a Decay at or below S becomes Sustain at its level) | Task 5 |
| TIME's Σ | clamped to −1..1 (32 × either way) | Task 5 |
| Per-block clamp | **Ruling:** `block_rate_max(sr) = sr / BLOCK_SIZE / 8`; it caps LFO RATE, BURST pulse RATE, ENV CYCLE's `1/(RISE+FALL)` (both segments slow by the same factor) and BURST CYCLE's `1/LENGTH`; AD and AHR times are not clamped | Task 6 |
| LFV slew per block | closed form toward the block-end target (approximate; per sample it is exact) | Task 6 |
| Glide | one `Glide` type shared by `Envelope` and `Lfo`; every TYPE/MODE/FORM change sets `d = old − new` (below 1e-6 is none); into A or B ENV `d` is 0 by construction (a clamp into A's 0..1 leaves a remainder that glides); a change seen first at a note-on takes over the same way | Tasks 6, 7 |
| Kinds in types | **Ruling:** `envelope::Kind { A, B(Func) }`, `view::EnvKind { A(EnvSpeed), B(Func) }`, `view::LfoKind { Classic, Func(LfoForm) }`: no dummy fields on A | Tasks 6, 16, 17 |
| Per-sample paths | `EnvA::fill` and `FuncGen::fill` write a block of levels in tight per-stage loops (one output evaluation per sample, curve divide once). B's positions are anchored, not accumulated: `x = x₀ ± k·step`, re-anchored at each turn, wrap and block start, and φ is a `u32` turn (f32 `+= step` drifts past 1e-4 in the cycling forms); `Envelope` then adds `amount · (level · peak[n]) + glide[n]` into the VCA's gain, the peak ramped per sample and the glide in its own loop | Tasks 5, 6 |
| Coefficients per block | only the running TYPE's (`ACoefs` or `BCoefs`), and reused while their inputs are unchanged | Tasks 5, 6 |
| Matrix amount scale | `amount / 127` read from a `const` table of 255 values (bit-identical to the divide), not divided per cell | Task 4 |
| Tick vs advance tolerance | **Ruling:** each path within 1e-4 of an f64 reference, stage changes within ±1 sample (ADR 0036) | Tasks 5, 6 |
| Modulators during a fade | they keep running from the `played` settings; the VCA keeps the routes of the last block before the fade (`VcaRoutes`) | Tasks 5, 10 |
| Lifetime fade start | set after the block's fade step, so the fade starts on the next block | Task 11 |
| Other VCA sources' ramp | `prev` is the previous block's value, 0 on a fresh voice (a VEL route fades in over the first block) | Task 10 |
| No-route VCA | an exhaustive `match` on `EngineType` (Algo, Modal: pass-through) | Task 10 |
| Cutoff ramp on a note-on | a note-on to an inactive voice drops the ramp (`SvfFilter::hold`); a retrigger of a sounding voice ramps from its last cutoff | Task 3 |
| CLASSIC SYNC | a note-on retriggers a CLASSIC LFO with SYNC 1 at its PHASE (today nothing called `retrigger`); FREE, LFV and BURST phases run free | Task 7 |
| `FilterMode` Lp18, Hp12 | not added: they land with their kinds (#127, #124); `FilterMode` has today's eight | Task 2 |
| Filter panel data | `ui/filter_panel.rs` (`KindPanel`, `SVF_PANEL`, `panel(kind)`, `applies(kind, id)`), not a method on the DSP enum; `PanelTarget` has `Filter` and `Route` only (TB-303's `Env1` and Prophet's stepped view land with #127, #126) | Task 15 |
| Slot bindings | `SlotBinding::Route(ModSource)` (Task 4, removed in 15), `FilterPanel(u8)` (main 0–4, extras 5–6), `EnvPanel(EnvSlot, u8)`, `LfoPanel(LfoSlot, u8)` | Tasks 4, 15–17 |
| `SlotCtx` | read from any `Blocks` through `Block::get`, so `part_page`'s signatures stay | Task 15 |
| Dimming | `ui/view.rs::dimmed(addr, &Sound)` and `is_dimmed(&View, &Sound)`: a `View::Text` cell, KIND while one kind is built, a single-mode MODE, AMP VEL with no VCA route, SPD cells of a type-B slot | Tasks 14–17 |
| Dirty regions | `MatrixState.rev` (bumped on every amount, presence or column change) joins the Grid, Route and Cells keys; the Cells key also packs the six cells' `Look`s | Tasks 8, 14 |
| Source row name | `mod_grid::ModSource` becomes `SourceRow`, so `modulation::ModSource` is the one `ModSource` | Task 4 |
| Map label of the MOD node | `ChainBlock.map: Option<&'static str>` ("MOD"); the sub-list keeps each def's short (E1…MTX) | Task 16 |
| `MAX_PAGES` | 64 → 72 (L2 and L3 take 64 and 65) | Task 17 |
| `MatrixState::MAX_SOURCES` | 16 → `MAX_MOD_SOURCES` (8): presence is a `u8` | Task 8 |
| Matrix hint | `PRIME MIX+PLUS  DELETE MIX+MINUS` | Task 8 |
| Bench layout | a second 30-second screen, ROUTING, with ten rows (1 OP, MODS, SVF, A VCA, B VCA, B BST, B LFO, B CRV, VEL VCA, 2 VCA), each in the first screen's `voice_row` form | Task 13 |
| `ModRouting` after the bench | **Ruling:** each term is measured on its own (Task 13): OTHER = 2 VCA − VEL VCA; CLAMP = VEL VCA − 1 OP − OTHER; ENV_A = A VCA − 1 OP − CLAMP; ENV_B = max(B VCA, B BST, B LFO) − 1 OP − CLAMP, B's costliest form; CURVE = B CRV − B VCA; BASE = 1 OP − 436 (the 1 OP row before the pool, 2026-09-27). Each is rounded up (a term that reads 0 or below is billed 1), and the model is checked to bill the MODS row at least as measured: after measuring, the measured values are billed, and the MODS check keeps them from undercounting | Task 13 |

## File structure

New:

| File | Responsibility |
|---|---|
| `chimera-core/src/dsp/modulator/mod.rs` | `EnvSlot`, `LfoSlot`, `EnvType`, `EnvSpeed`, `HoldPos`, `FuncMode`, `EnvForm`, `LfoForm`, `Func`, `LfoType`, `FuncParams`, `Glide`, `pick` |
| `chimera-core/src/dsp/modulator/law.rs` | `Range`, `speed_ranges`, B's ranges, `rc_k`, `rc_coeff`, `curve`, `curve_inv`, `shape_w`, `tilt` |
| `chimera-core/src/dsp/modulator/env_a.rs` | `ACoefs`, `EnvA` (tick, per-sample `fill`, closed-form advance, TYPE-change entry) |
| `chimera-core/src/dsp/modulator/func.rs` | `Slides`, `BCoefs`, `FuncGen` (ENV, LFO, BURST; tick, `fill`, advance) |
| `chimera-core/src/ui/view.rs` | `SlotCtx`, `View`, `view`, `slot_addr`, `dimmed`, `prime_target` |
| `chimera-core/src/ui/filter_panel.rs` | `PanelTarget`, `PanelKnob`, `KindPanel`, `SVF_PANEL`, `panel`, `applies` |
| `chimera-core/src/ui/mod_panel.rs` | `PanelSlot`, `ModPanel`, `env_panel`, `lfo_panel` |
| `chimera-core/tests/filter_test.rs`, `env_a_test.rs`, `func_gen_test.rs`, `env_slot_test.rs`, `lfo_slot_test.rs`, `routing_test.rs`, `vca_test.rs`, `flt_page_test.rs`, `amp_page_test.rs`, `mod_pages_test.rs` | Tests |
| `docs/adr/0035-every-connection-is-a-matrix-route.md`, `0036-cascadia-style-modulators.md`, `0037-kind-lays-out-the-filter-panel.md` | ADRs |

Modified: `dsp/mod.rs`, `dsp/filter.rs`, `dsp/envelope.rs`, `dsp/lfo.rs`, `dsp/voice.rs`, `block.rs`, `addr.rs`, `params.rs`, `modulation.rs`, `preset.rs`, `factory.rs`, `ui/block_def.rs`, `ui/block_registry.rs`, `ui/chain.rs`, `ui/dungeon_map.rs`, `ui/components.rs`, `ui/fmt.rs`, `ui/focus.rs`, `ui/mod.rs`, `ui/mod_grid.rs`, `ui/page.rs`, `ui/part_page.rs`, `ui/region.rs`, `ui/renderer.rs`, `ui/viz.rs`, `chimera-stm32/src/bench.rs`, and the tests named in each task.

## Task order

1. Lock the factory Sounds
2. Typed MODE on FLT › MODE; FM, ENV and KEY leave the filter (#111, #57)
3. CUTOFF in octaves; the cutoff ramps across the block (#53); the `*_lfo_cutoff` goldens re-recorded after the owner's listen — **listen STOP**
4. Eight matrix sources; ENV, KEY and LFO as route knobs
5. Envelope A, and ENV 1–3 on it
6. Envelope B and the ENV slot's TYPE changes
7. LFO slots: CLASSIC and FUNC — **end of the first shippable slice (#121)**
8. Route presence, the default routes, MIX+MINUS and the "—" knob
9. ENV destinations; the UI's stand-in sources
10. VCA as a destination
11. Voice lifetime by the VCA's routes
12. `ModRouting::cost`
13. Bench the modulator pool — **hardware STOP**
14. FLD / VCA (AMP) on every chain, and dimmed cells
15. KIND and the filter panel
16. The MOD node and the ENV pages
17. The SPD page and the LFO pages
18. ADRs 0035, 0036 and 0037
19. Play the pages; the load check — **hardware STOP**

---

Shell shorthands used in every task:

```bash
export T="CARGO_TARGET_DIR=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/wt-fr-target"
export S=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/screens
cd /tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/wt-fr
```

`env $T cargo test ...` below means the focused run with the target dir. **Looking at a screen** always means: `mkdir -p $S && SCREEN_DUMP=$S env $T cargo test -p chimera-core --test screen_golden_test screen_goldens_match`, then `magick $S/<case>.ppm $S/<case>.png` (or `convert` on ImageMagick 6), then Read the PNG and check what the step lists. A screen that looks wrong is fixed before its golden is recorded.

---

### Task 1: Lock the factory Sounds

The spec's migration check: before anything changes, one audio golden per factory Sound. They must stay bit-identical to the end of this plan.

**Files:**
- Modify: `chimera-core/tests/common/mod.rs` (`Case::Factory`)
- Modify: `chimera-core/tests/golden_test.rs` (8 rows, a coverage test)

**Interfaces:**
- Consumes: `chimera_core::factory::{factory_sound, FACTORY_LEN}`.
- Produces: `common::Case::Factory(u8)` named `factory_0` … `factory_7`.

- [ ] **Step 1: Add the case**

In `chimera-core/tests/common/mod.rs`, add the variant after `AlgoToModalSwitch`:

```rust
    /// Factory Sound `i` (0–7) as the bank builds it, its own matrix included.
    Factory(u8),
```

After `TX_NAMES`:

```rust
static FACTORY_NAMES: [&str; 8] = [
    "factory_0", "factory_1", "factory_2", "factory_3", "factory_4", "factory_5", "factory_6",
    "factory_7",
];
```

Change `ALL` to 23 entries, adding after `Case::AlgoToModalSwitch,`:

```rust
        Case::Factory(0),
        Case::Factory(1),
        Case::Factory(2),
        Case::Factory(3),
        Case::Factory(4),
        Case::Factory(5),
        Case::Factory(6),
        Case::Factory(7),
```

(and `pub const ALL: [Case; 23]`). In `name`:

```rust
            Case::Factory(i) => FACTORY_NAMES[i as usize % 8],
```

In `setup`:

```rust
        Case::Factory(i) => {
            let s = chimera_core::factory::factory_sound(i as usize).expect("factory sound");
            (s.params, s.mod_state)
        }
```

- [ ] **Step 2: Write the failing coverage test**

In `chimera-core/tests/golden_test.rs`, at the end of the file:

```rust
/// Filter-routing spec § Migration: every factory Sound is locked.
#[test]
fn every_factory_sound_has_a_golden() {
    for i in 0..chimera_core::factory::FACTORY_LEN {
        let name = format!("factory_{i}");
        assert!(GOLDENS.iter().any(|g| g.0 == name), "{name}");
    }
}
```

- [ ] **Step 3: Run it to see it fail**

Run: `env $T cargo test -p chimera-core --test golden_test`
Expected: FAIL. `goldens_match` reports `factory_0: no golden recorded` (and so on), and `every_factory_sound_has_a_golden` fails on `factory_0`.

- [ ] **Step 4: Record the eight rows**

Run: `GOLDEN_RECORD=1 env $T cargo test -p chimera-core --test golden_test goldens_match -- --nocapture`
Paste the eight printed `("factory_0", …)` … `("factory_7", …)` rows at the end of `GOLDENS`, under a comment `// Filter-routing spec § Migration: recorded before the change; never re-recorded.`. Leave every other row as it is.

- [ ] **Step 5: Run the tests**

Run: `env $T cargo test -p chimera-core --test golden_test --test sanity_test`
Expected: PASS (the sanity gate gates only `algo_*` names, so the factory cases are locked, not gated).

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/tests/common/mod.rs chimera-core/tests/golden_test.rs
git commit -m "Lock the eight factory Sounds with audio goldens"
```

---

### Task 2: Typed MODE on FLT › MODE; FM, ENV and KEY leave the filter (#111, #57)

`FilterParams.mode` becomes a private `FilterMode` (#111) and MODE gets a spec (id 7) and a page. The three never-read fields (FM, ENV, KEY; ids 3–5) are deleted and their ids retired (#112). The FLT page takes its final SVF layout minus KIND, and the ENV and KEY cells stay empty until Task 4. FLT › MODE (id 59) is the new sub-page. `fast_tan`'s doc stops overclaiming (#57). #48 (the saturator) is not touched: its fix changes high-resonance levels and waits for the filter-model work (commented on #48).

**Files:**
- Modify: `chimera-core/src/dsp/filter.rs` (`FilterMode`, `SVF_MODES`, `SVF_MODE_NAMES`, `process` via `mode()`)
- Modify: `chimera-core/src/dsp/mod.rs:97-106` (`fast_tan` doc)
- Modify: `chimera-core/src/params.rs:5-88` (`FilterParams`, `FILTER_SPECS`)
- Modify: `chimera-core/src/ui/block_registry.rs` (`FILTER`, new `FILTER_MODE`, `FILTER_SUB_PAGES`)
- Modify: `chimera-core/src/ui/renderer.rs:96-103` (FilterResponse reads CUTOFF and RES by address)
- Modify: `chimera-core/src/ui/page.rs` (the two demo bindings to FM and ENV)
- Create: `chimera-core/tests/filter_test.rs`
- Modify tests: `stress_test.rs`, `live_param_test.rs`, `property_test.rs`, `desktop_sim_test.rs`, `modal_integration_test.rs`, `signal_chain_test.rs`, `chain_spectral_test.rs`, `modulation_integration_test.rs`, `part_page_test.rs`, `binding_test.rs`, `ui_test.rs`, `mod_registry_test.rs`, `prime_status_test.rs`, `preset_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `dsp::filter::FilterMode { Lp6 = 0, Lp12 = 1, Lp24 = 2, Bp12 = 3, Bp24 = 4, Hp24 = 5, Notch = 6, Phaser = 7 }` with `ALL: [FilterMode; 8]` (discriminant order) and `label(self) -> &'static str`.
  - `dsp::filter::SVF_MODES: [FilterMode; 8]` (LP24 first) and `SVF_MODE_NAMES: [&str; 8]`.
  - `FilterParams::mode(&self) -> FilterMode`, `FilterParams::set_mode(&mut self, FilterMode) -> bool`, `FilterParams::MODE = ParamId(7)`; the `Block` value of MODE is the index in `SVF_MODES`.
  - `ui::block_registry::FILTER_MODE` (id 59, "Filter Mode", short "MODE").

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/filter_test.rs`:

```rust
//! The filter: typed modes (#111) and retired ids (#112).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::filter::{FilterMode, SVF_MODES, SvfFilter};
use chimera_core::params::{FilterParams, ParamSnapshot};
use chimera_core::ui::block_def::slot_addr;
use chimera_core::ui::block_registry::{FILTER, FILTER_MODE};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

#[test]
fn mode_is_typed_and_stays_in_the_svf_list() {
    let mut f = FilterParams::default();
    assert_eq!(f.mode(), FilterMode::Lp24, "every Sound is on LP24 today");
    for m in FilterMode::ALL {
        assert!(f.set_mode(m));
        assert_eq!(f.mode(), m);
    }
    // The block value is the index in the SVF's list, so an encoder steps it.
    f.set(FilterParams::MODE, 0.0);
    assert_eq!(f.mode(), SVF_MODES[0]);
    assert_eq!(SVF_MODES[0], FilterMode::Lp24, "the default comes first");
    f.set(FilterParams::MODE, 99.0);
    assert_eq!(f.mode(), FilterMode::Phaser);
    assert_eq!(f.get(FilterParams::MODE), 7.0);
}

#[test]
fn the_discriminants_are_the_old_mode_byte() {
    for (i, m) in FilterMode::ALL.iter().enumerate() {
        assert_eq!(*m as u8, i as u8);
    }
}

#[test]
fn retired_filter_ids_have_no_spec() {
    let p = ParamSnapshot::default();
    for id in [3, 4, 5] {
        assert!(p.filter.spec(ParamId(id)).is_none(), "id {id} is retired");
    }
}

#[test]
fn mode_is_on_the_flt_pages() {
    let mode = ParamAddr::new(BlockRef::Filter, FilterParams::MODE);
    let on = |def| (0..6).any(|i| slot_addr(def, i, chimera_core::addr::Op::A) == Some(mode));
    assert!(on(&FILTER) && on(&FILTER_MODE));
}

/// Each mode filters a saw differently, and all stay finite.
#[test]
fn every_mode_renders_finite_and_distinct() {
    let mut seen = Vec::new();
    for m in FilterMode::ALL {
        let mut p = FilterParams::default();
        p.set_mode(m);
        p.resonance = 0.4;
        let mut f = SvfFilter::new();
        let mut out = Vec::new();
        for b in 0..20 {
            let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                .map(|i| ((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0)
                .collect();
            f.process(&mut buf, &p, SR);
            out.extend(buf);
        }
        assert!(out.iter().all(|x| x.is_finite()), "{m:?}");
        let bits: Vec<u32> = out.iter().map(|x| x.to_bits()).collect();
        assert!(!seen.contains(&bits), "{m:?} renders like another mode");
        seen.push(bits);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `env $T cargo test -p chimera-core --test filter_test`
Expected: FAIL to compile: no `FilterMode::ALL`, `SVF_MODES`, `mode()`, `FILTER_MODE`.

- [ ] **Step 3: Type the mode**

In `chimera-core/src/dsp/filter.rs`, replace the `FilterMode` enum and its `impl` (lines 1–29) with:

```rust
use crate::params::FilterParams;

/// The SVF's modes. The discriminants are the old `mode` byte (#111), so
/// every Sound keeps its mode (LP24 = 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterMode {
    Lp6 = 0,
    Lp12 = 1,
    Lp24 = 2,
    Bp12 = 3,
    Bp24 = 4,
    Hp24 = 5,
    Notch = 6,
    Phaser = 7,
}

impl FilterMode {
    /// In discriminant order.
    pub const ALL: [FilterMode; 8] = [
        FilterMode::Lp6,
        FilterMode::Lp12,
        FilterMode::Lp24,
        FilterMode::Bp12,
        FilterMode::Bp24,
        FilterMode::Hp24,
        FilterMode::Notch,
        FilterMode::Phaser,
    ];

    pub const fn label(self) -> &'static str {
        SVF_MODE_NAMES_BY_ID[self as usize]
    }
}

/// The SVF's modes as its MODE knob steps them, the default first (spec § 7).
pub const SVF_MODES: [FilterMode; 8] = [
    FilterMode::Lp24,
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
    FilterMode::Phaser,
];

/// `SVF_MODES`' names, for MODE's spec.
pub static SVF_MODE_NAMES: [&str; 8] = [
    "LP24", "LP6", "LP12", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];

const SVF_MODE_NAMES_BY_ID: [&str; 8] = [
    "LP6", "LP12", "LP24", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];
```

In `SvfFilter::process`, replace `let mode = FilterMode::from_u8(params.mode);` with `let mode = params.mode();`, and rename the match arms: `Lp1` → `Lp6`, `Lp2` → `Lp12`, `Lp4` → `Lp24`, `Bp2` → `Bp12`, `Bp4` → `Bp24`, `Hp4` → `Hp24`, `Nt2` → `Notch`, `Phazor` → `Phaser`. The arithmetic of every arm stays as it is.

- [ ] **Step 4: Retire FM, ENV and KEY; give MODE a spec**

In `chimera-core/src/params.rs`, replace `FilterParams`, its `Default`, its `impl`, `FILTER_SPECS` and its `Block` impl (lines 1–88) with:

```rust
use crate::addr::{BlockRef, Blocks};
use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::filter::{FilterMode, SVF_MODE_NAMES, SVF_MODES};

/// Parameters for one voice's filter.
#[derive(Clone, Copy, Debug)]
pub struct FilterParams {
    pub cutoff: f32,
    pub resonance: f32,
    pub drive: f32,
    /// Private: `set_mode` keeps it in the SVF's list (spec § 7).
    mode: FilterMode,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            cutoff: 1000.0,
            resonance: 0.0,
            drive: 0.0,
            mode: FilterMode::Lp24,
        }
    }
}

impl FilterParams {
    pub const CUTOFF: ParamId = ParamId(0);
    pub const RESONANCE: ParamId = ParamId(1);
    pub const DRIVE: ParamId = ParamId(2);
    // 3 (FM), 4 (ENV) and 5 (KEY) are retired, never reused (ADR 0009).
    pub const MODE: ParamId = ParamId(7);

    pub fn mode(&self) -> FilterMode {
        self.mode
    }

    /// Sets `m` if the SVF has it; returns whether it did.
    pub fn set_mode(&mut self, m: FilterMode) -> bool {
        let ok = SVF_MODES.contains(&m);
        if ok {
            self.mode = m;
        }
        ok
    }
}

/// Every one read by `Voice` per block; MODE is an Enum, so not modulatable.
pub static FILTER_SPECS: [ParamSpec; 4] = [
    ParamSpec::continuous(
        0,
        "CUTOFF",
        ValFmt::Uni,
        20.0,
        20000.0,
        1000.0,
        (20000.0 - 20.0) / 128.0,
        true,
    ),
    ParamSpec::continuous(1, "RESO", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(2, "DRIVE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::choice(7, "MODE", ValFmt::Names(&SVF_MODE_NAMES), 7.0, 0.0),
];

impl Block for FilterParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &FILTER_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::CUTOFF => self.cutoff,
            Self::RESONANCE => self.resonance,
            Self::DRIVE => self.drive,
            Self::MODE => SVF_MODES.iter().position(|&m| m == self.mode).unwrap_or(0) as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::CUTOFF => self.cutoff = v,
            Self::RESONANCE => self.resonance = v,
            Self::DRIVE => self.drive = v,
            Self::MODE => {
                self.set_mode(SVF_MODES[(v.max(0.0) as usize).min(SVF_MODES.len() - 1)]);
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 5: Put MODE on the pages**

In `chimera-core/src/ui/block_registry.rs`, replace `FILTER` with:

```rust
/// SVF: — · CUTOFF · RES / MODE · ENV · KEY (KIND and the route knobs come
/// later in the filter-routing plan; spec § 6).
pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        EMPTY,
        ParamSlot::param(BlockRef::Filter, FilterParams::CUTOFF),
        ParamSlot::param(BlockRef::Filter, FilterParams::RESONANCE),
        ParamSlot::param(BlockRef::Filter, FilterParams::MODE),
        EMPTY,
        EMPTY,
    ],
};

/// FLT › MODE: MODE and the SVF's extras (spec § UI).
pub static FILTER_MODE: BlockDef = BlockDef {
    id: 59,
    name: "Filter Mode",
    short: "MODE",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::MODE),
        ParamSlot::param(BlockRef::Filter, FilterParams::DRIVE),
        EMPTY,
        EMPTY,
        EMPTY,
        EMPTY,
    ],
};

static FILTER_SUB_PAGES: [&BlockDef; 1] = [&FILTER_MODE];
```

and in `KICK_BLOCKS`, `MODAL_PLUCK_BLOCKS` and `ALGO_BLOCKS`, give the `&FILTER` block `sub_pages: &FILTER_SUB_PAGES,`.

- [ ] **Step 6: The filter viz reads by address**

In `chimera-core/src/ui/renderer.rs`, replace the `VizType::FilterResponse` arm of `draw_big_viz` with:

```rust
            VizType::FilterResponse => {
                // By address, not slot (spec § UI "Vizzes read by address").
                let at = |id| {
                    let addr = crate::addr::ParamAddr::new(crate::addr::BlockRef::Filter, id);
                    (0..f.def.params.len())
                        .find(|&i| slot_addr(f.def, i, f.sel_op) == Some(addr))
                        .map_or(0.0, a)
                };
                use crate::params::FilterParams;
                let slot = &f.def.params[f.focus];
                let mut buf = FmtBuf::new();
                fmt::fmt_val(&mut buf, a(f.focus), slot.format());
                let readout =
                    (slot.binding != SlotBinding::Empty).then(|| (slot.label(), buf.as_str()));
                viz::filter(
                    display,
                    at(FilterParams::CUTOFF),
                    at(FilterParams::RESONANCE),
                    readout,
                );
            }
```

- [ ] **Step 7: The demo pages stop naming the retired ids**

In `chimera-core/src/ui/page.rs`, in `DEMO_WAVES` replace `ParamAddr::new(BlockRef::Filter, FilterParams::ENV_AMOUNT),` with `ParamAddr::new(BlockRef::Folder, FolderParams::MIX),`. In `DEMO_SHAPES` replace `ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT),` with `ParamAddr::new(BlockRef::Drive, DriveParams::MIX),`.

- [ ] **Step 8: `fast_tan`'s doc (#57)**

In `chimera-core/src/dsp/mod.rs`, replace the doc of `fast_tan` with:

```rust
/// `tan(x)` for filter coefficients: a 5th-order Taylor series. Accurate
/// below about 10 kHz at 48 kHz; above that it reads low (a 20 kHz cutoff
/// comes out near 18.3 kHz, #57). ~5 cycles vs ~400 for `libm::tanf`.
```

(The code is unchanged: changing it would change every factory Sound.)

- [ ] **Step 9: Update the tests that wrote the old fields**

- Delete every line `p.filter.mode = 2;`, `params.filter.mode = 2;`, `params_closed.filter.mode = 2;` and `ui.params_mut().filter.mode = 2;` (LP24 is the default), with its trailing comment: `sed -i '/\.filter\.mode = 2;/d' chimera-core/tests/{stress_test,live_param_test,property_test,desktop_sim_test,modal_integration_test,signal_chain_test,chain_spectral_test,modulation_integration_test}.rs`.
- `live_param_test.rs:111` (`p.filter.mode = 1; // LP2`) and `property_test.rs:451` (`p.filter.mode = 1;`, no comment): each becomes `p.filter.set_mode(chimera_core::dsp::filter::FilterMode::Lp12);` (`sed -i 's/p\.filter\.mode = 1;.*$/p.filter.set_mode(chimera_core::dsp::filter::FilterMode::Lp12);/' chimera-core/tests/{live_param_test,property_test}.rs`).
- `property_test.rs:98`: `p.filter.mode = rng.u8(7);` becomes `p.filter.set_mode(chimera_core::dsp::filter::FilterMode::ALL[rng.u8(7) as usize]);`.
- `part_page_test.rs`, `drive_filter_folder_pages`, the FILTER lines become:

```rust
    assert_eq!(read(&reg::FILTER, &p), [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    turn(&reg::FILTER, 1, -1, &mut p);
    assert_eq!(p.filter.cutoff, 20000.0 - (20000.0 - 20.0) / 128.0);
    turn(&reg::FILTER, 3, 1, &mut p);
    assert_eq!(p.filter.mode(), FilterMode::Lp6, "the next in the SVF's list");
    turn(&reg::FILTER, 3, 20, &mut p);
    assert_eq!(p.filter.mode(), FilterMode::Phaser, "clamps at the last");
```

  with `use chimera_core::dsp::filter::FilterMode;` at the top.
- `binding_test.rs`, `part_pages_display_like_before`: the FILTER row becomes `("--", Uni), ("CUTOFF", Uni), ("RESO", Uni), ("MODE", ValFmt::Names(&chimera_core::dsp::filter::SVF_MODE_NAMES)), ("--", Uni), ("--", Uni)`, and `block_def_ids_are_unique` gains `&reg::FILTER_MODE` (array length 39).
- `ui_test.rs`: replace `test_filter_env_amount_bipolar_in_registry` with:

```rust
#[test]
fn test_filter_mode_is_named_in_registry() {
    use chimera_core::ui::block_registry;
    use chimera_core::ui::page::ValFmt;
    assert!(matches!(
        block_registry::FILTER.params[3].format(),
        ValFmt::Names(_)
    ));
}
```

- `mod_registry_test.rs`, `registry_refuses_non_modulatable`: `ParamAddr::new(BlockRef::Filter, FilterParams::FM_AMOUNT), // never read` becomes `ParamAddr::new(BlockRef::Filter, ParamId(3)), // retired (FM)`.
- `screen/mod.rs`: the `bigviz_filter` case becomes (CUTOFF moved to b, RES to c):

```rust
    ("bigviz_filter", |ui| {
        plus(ui, 3);
        feed(ui, Input::turn(EncoderId::C, 80)); // resonance
        feed(ui, Input::turn(EncoderId::B, -60)); // cutoff, focused
    }),
```

  and in `mod_matrix` the first `feed(ui, Input::turn(EncoderId::A, 1)); // focus CUTOFF` becomes `feed(ui, Input::turn(EncoderId::B, 1)); // focus CUTOFF`.
- CUTOFF left slot a, so tests that primed it from the default focus now focus slot b first: in `prime_status_test.rs`, `filter_page()` turns `EncoderId::B` instead of `EncoderId::A`; in `preset_test.rs`, `priming_on_main_page_registers_focused_param` calls `ui.handle_input(&MockControls::new().encoder(EncoderId::B, 1));` before `prime(&mut ui)`.
- `prime_status_test.rs`, `priming_past_matrix_capacity_on_the_algo_chain_reports_full`: DRIVE moved from FLT to FLT › MODE, so the walk (ALG, LEVEL, DRV, FLT, FLD) now reaches only 16 addresses and never reports MATRIX FULL (`added_before` would be `None`). The walk also visits FLT › MODE, which keeps 17 addresses (MORPH, VOL, six LEVELs, DRV's three, CUTOFF, RESO, the filter's DRIVE, FLD's three). Its loop over the three nodes becomes:

```rust
    feed(&mut ui, Input::press(ButtonId::Plus)); // Drive
    prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
    feed(&mut ui, Input::press(ButtonId::Plus)); // Filter: CUTOFF, RESO (MODE refused)
    prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
    feed(&mut ui, Input::press(ButtonId::Edit)); // FLT › MODE: the filter's DRIVE
    prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
    feed(&mut ui, Input::press(ButtonId::Plus)); // Folder
    prime_every_slot(&mut ui, &ALL_SLOTS, &mut seen);
```

  and its doc reads "reaches 17 modulatable addresses (MORPH, VOL, six LEVELs, Drive's three, CUTOFF, RESO, the filter's DRIVE on FLT › MODE, Folder's three)". Its expectations stay: exactly `MAX_MOD_DESTS` ADDED, the next reports MATRIX FULL.
- Run `header_map_test` too: FLT's new sub-list (FLT, MODE) must not run into the next node's label.

- [ ] **Step 10: Run the tests**

Run: `env $T cargo test -p chimera-core` (the whole crate; these edits reach every test that named a filter field).
Expected: PASS except `screen_goldens_match` (next step). Every audio golden (factory included) is bit-identical: LP24 is discriminant 2, so the arithmetic path is unchanged.

- [ ] **Step 11: Look at the FLT page and re-record its golden**

Look at `bigviz_filter`: the first cell is a dash, then CUTOFF, RESO, MODE reading `LP24`, then two dashes. The curve follows the cutoff and resonance (a peak about a third of the way across). The map has FLT lit. Then `SCREEN_RECORD=1 env $T cargo test -p chimera-core --test screen_golden_test screen_goldens_match -- --nocapture` and paste the `bigviz_filter` row over its own. `mod_matrix` primes CUTOFF from slot b now, which nudges it one step just as slot a did, so it should still match; re-record it only if it mismatches, after looking at it.

- [ ] **Step 12: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/filter.rs chimera-core/src/dsp/mod.rs chimera-core/src/params.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/page.rs \
  chimera-core/tests/filter_test.rs chimera-core/tests/stress_test.rs chimera-core/tests/live_param_test.rs \
  chimera-core/tests/property_test.rs chimera-core/tests/desktop_sim_test.rs \
  chimera-core/tests/modal_integration_test.rs chimera-core/tests/signal_chain_test.rs \
  chimera-core/tests/chain_spectral_test.rs chimera-core/tests/modulation_integration_test.rs \
  chimera-core/tests/part_page_test.rs chimera-core/tests/binding_test.rs chimera-core/tests/ui_test.rs \
  chimera-core/tests/mod_registry_test.rs chimera-core/tests/prime_status_test.rs \
  chimera-core/tests/preset_test.rs chimera-core/tests/screen/mod.rs \
  chimera-core/tests/screen_golden_test.rs
git commit -m "Typed filter MODE on FLT › MODE; FM, ENV and KEY retire from the filter"
```

---

### Task 3: CUTOFF in octaves; the cutoff ramps across the block (#53)

CUTOFF's matrix routes apply in octaves (spec § 3) through a new offset law on `ParamSpec`. The SVF ramps its coefficient across a block whenever the cutoff changed, which fixes #53 for the filter only; a note-on to a voice that isn't sounding starts without a ramp. The two `*_lfo_cutoff` goldens move: this task passes them through the sanity gate, writes them out for a listen and re-records them (ADR 0011), so no golden is ever parked.

**Files:**
- Modify: `chimera-core/src/dsp/mod.rs` (`fast_exp2`, `fast_log2`)
- Modify: `chimera-core/src/block.rs` (`OffsetLaw`, `ParamSpec.law`, `octaves`, `offset`, `offset_normalized`, `apply_offset`)
- Modify: `chimera-core/src/params.rs` (CUTOFF's spec `.octaves(CUTOFF_OCTAVES)`)
- Modify: `chimera-core/src/dsp/filter.rs` (`CUTOFF_OCTAVES`, `routed_cutoff`, `g_at`, `SvfFilter.g`, `hold`, `last_g`, `tick`)
- Modify: `chimera-core/src/dsp/voice.rs` (`trigger` drops the ramp on a fresh note; a unit test)
- Modify: `chimera-core/src/ui/mod.rs` (`update`: mod bars through `offset_normalized`)
- Modify: `chimera-core/tests/golden_test.rs` (the two `*_lfo_cutoff` rows, re-recorded)
- Test: `chimera-core/tests/filter_test.rs`

**Interfaces:**
- Consumes: `algo::math::{exp2, log2}`.
- Produces:
  - `chimera_core::dsp::{fast_exp2, fast_log2}` (`fn(f32) -> f32`).
  - `block::OffsetLaw { Linear, Octaves(f32) }`; `ParamSpec.law`; `ParamSpec::octaves(self, f32) -> Self` (const); `ParamSpec::offset(&self, v: f32, off: f32) -> f32`; `ParamSpec::offset_normalized(&self, n: f32, off: f32) -> f32`.
  - `dsp::filter::CUTOFF_OCTAVES: f32 = 10.0`; `dsp::filter::routed_cutoff(base: f32, sum: f32) -> f32`.
  - `SvfFilter::hold(&mut self)`: the next block starts without a ramp; `pub(crate) SvfFilter::last_g(&self) -> Option<f32>`.
  - `dsp::filter::g_at(from: f32, step: f32, i: usize) -> f32`: sample `i`'s coefficient on a ramp (`from + step·(i + 1)`).

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/filter_test.rs`:

```rust
use chimera_core::dsp::fast_exp2;
use chimera_core::dsp::filter::routed_cutoff;

#[test]
fn fast_exp2_is_exact_at_integers_and_within_a_tenth_of_a_cent() {
    for i in -12..=12 {
        assert_eq!(fast_exp2(i as f32), 2f32.powi(i), "2^{i}");
    }
    let mut k = -12_000i32;
    while k <= 12_000 {
        let x = k as f32 / 1000.0;
        let cents = 1200.0 * (fast_exp2(x) as f64 / 2f64.powf(x as f64)).log2();
        assert!(cents.abs() < 0.1, "2^{x}: {cents} cents");
        k += 1;
    }
}

#[test]
fn routed_cutoff_is_in_octaves() {
    let base = 1000.0f32;
    assert_eq!(routed_cutoff(base, 0.0).to_bits(), base.to_bits(), "Σ 0 is bit for bit");
    // NOTE at 100 %: (note − 60) / 120 × 127/127 → one octave per octave.
    for (note, want) in [(48.0f32, 0.5f32), (60.0, 1.0), (72.0, 2.0), (84.0, 4.0)] {
        let fc = routed_cutoff(base, (note - 60.0) / 120.0);
        assert!((fc / (base * want) - 1.0).abs() < 1e-6, "note {note}: {fc}");
    }
    // ENV at 100 % with the envelope at 1: +10 octaves before the clamp.
    assert!((routed_cutoff(15.0, 1.0) / (15.0 * 1024.0) - 1.0).abs() < 1e-6);
    assert_eq!(routed_cutoff(1000.0, 1.0), 20_000.0);
    assert_eq!(routed_cutoff(1000.0, -1.0), 20.0);
}

/// Review Focus 1: sums far past ±1 stay finite and inside the range.
#[test]
fn routed_cutoff_survives_huge_sums() {
    for sum in [3.0f32, -3.0, 8.0, -8.0, 1e6, -1e6] {
        let fc = routed_cutoff(1000.0, sum);
        assert!(fc.is_finite() && (20.0..=20_000.0).contains(&fc), "{sum}: {fc}");
    }
}

/// Spec § Tests "Clicks": an ENV sweep on CUTOFF (attack 1 ms and 50 ms,
/// route ±100 %, RES 0.5) has a largest second difference at most 1.5 ×
/// that of the same render with each block's cutoff held at the block's
/// mean. And #53 itself: with the fast attack, the ramp removes at least
/// half the largest second difference of the same targets stepped per
/// block (a filter without the ramp fails this; the spec's own ratio would
/// not tell the two apart).
#[test]
fn a_cutoff_sweep_does_not_click() {
    #[derive(Clone, Copy, PartialEq)]
    enum How {
        Ramped,
        HeldAtMean,
        Stepped,
    }
    for attack in [0.001f32, 0.05] {
        for amount in [1.0f32, -1.0] {
            let env = |b: usize| ((b * BLOCK_SIZE) as f32 / (attack * SR as f32)).min(1.0);
            let fc = |b: usize| routed_cutoff(1000.0, amount * env(b));
            let render = |how: How| {
                let mut f = SvfFilter::new();
                let mut p = FilterParams::default();
                p.resonance = 0.5;
                let mut out = Vec::new();
                for b in 0..40usize {
                    p.cutoff = if how == How::HeldAtMean {
                        0.5 * (fc(b.saturating_sub(1)) + fc(b))
                    } else {
                        fc(b)
                    };
                    if how != How::Ramped {
                        f.hold();
                    }
                    let mut buf: Vec<f32> = (0..BLOCK_SIZE)
                        .map(|i| {
                            let n = (b * BLOCK_SIZE + i) as f32;
                            0.5 * (core::f32::consts::TAU * 220.0 * n / SR as f32).sin()
                        })
                        .collect();
                    f.process(&mut buf, &p, SR);
                    out.extend(buf);
                }
                out
            };
            let d2 = |s: &[f32]| {
                s.windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .fold(0.0f32, f32::max)
            };
            let ramped = d2(&render(How::Ramped));
            let held = d2(&render(How::HeldAtMean));
            assert!(
                ramped <= 1.5 * held,
                "attack {attack}, amount {amount}: {ramped} vs {held}"
            );
            // A 50 ms sweep moves a quarter octave a block: its steps hide
            // under the sine's own curvature, so only the fast one is checked.
            if attack < 0.01 {
                let stepped = d2(&render(How::Stepped));
                assert!(
                    ramped <= 0.5 * stepped,
                    "attack {attack}, amount {amount}: {ramped} vs stepped {stepped}"
                );
            }
        }
    }
}

/// Within a block, `g` moves by the same step every sample and lands on
/// the new value on the last one.
#[test]
fn g_ramps_evenly_to_the_new_value() {
    use chimera_core::dsp::filter::g_at;
    let (from, to) = (0.1f32, 0.9f32);
    let step = (to - from) / BLOCK_SIZE as f32;
    let g: Vec<f32> = (0..BLOCK_SIZE).map(|i| g_at(from, step, i)).collect();
    assert!((g[BLOCK_SIZE - 1] - to).abs() < 1e-6);
    let mut prev = from;
    for (i, &x) in g.iter().enumerate() {
        assert!((x - prev - step).abs() < 1e-6, "sample {i}");
        prev = x;
    }
}

/// After a cutoff change, the next steady blocks don't ramp again: the
/// ramp ended on the new `g` and kept it, so dropping any ramp (`hold`)
/// changes nothing. Fails if the ramp's end isn't stored.
#[test]
fn a_steady_cutoff_does_not_ramp() {
    // MODE is private: no struct-update syntax from a test.
    let (mut lo, mut hi) = (FilterParams::default(), FilterParams::default());
    (lo.cutoff, hi.cutoff) = (500.0, 2000.0);
    let saw = |b: usize| -> Vec<f32> {
        (0..BLOCK_SIZE)
            .map(|i| ((b * BLOCK_SIZE + i) % 97) as f32 / 48.5 - 1.0)
            .collect()
    };
    let mut a = SvfFilter::new();
    for (blk, p) in [&lo, &hi].into_iter().enumerate() {
        a.process(&mut saw(blk), p, SR); // block 1 ramps 500 → 2000 Hz
    }
    let mut b = a.clone();
    for blk in 2..10 {
        let (mut x, mut y) = (saw(blk), saw(blk));
        a.process(&mut x, &hi, SR);
        b.hold();
        b.process(&mut y, &hi, SR);
        assert_eq!(x, y, "block {blk}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test filter_test`
Expected: FAIL to compile: no `fast_exp2`, `routed_cutoff`, `SvfFilter::hold`.

- [ ] **Step 3: `fast_exp2` and `fast_log2`**

In `chimera-core/src/dsp/mod.rs`, after `fast_tan`:

```rust
/// `2^x`, exact at integers, relative error below 1e-6 (spec § Signal flow).
pub use self::algo::math::exp2 as fast_exp2;
/// `log2(x)` for `x > 0`, absolute error below 3e-5.
pub use self::algo::math::log2 as fast_log2;
```

- [ ] **Step 4: The offset law**

In `chimera-core/src/block.rs`, before `ParamSpec`:

```rust
/// How a matrix offset moves a value (spec § 3; ADR 0010 for `Linear`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetLaw {
    /// `v + off·(max − min)`.
    Linear,
    /// `v · 2^(n·off)`: `n` octaves at a full offset (CUTOFF).
    Octaves(f32),
}
```

Add the field `pub law: OffsetLaw,` last in `ParamSpec` (doc: `/// How a matrix offset applies.`), `law: OffsetLaw::Linear,` in each of `continuous`, `stepped` and `choice`, and to `impl ParamSpec`:

```rust
    /// This spec with the octave law.
    pub const fn octaves(self, n: f32) -> Self {
        Self {
            law: OffsetLaw::Octaves(n),
            ..self
        }
    }

    /// `v` moved by a matrix offset `off`, clamped to the range.
    pub fn offset(&self, v: f32, off: f32) -> f32 {
        match self.law {
            OffsetLaw::Linear => (v + off * (self.max - self.min)).clamp(self.min, self.max),
            OffsetLaw::Octaves(n) => {
                (v * crate::dsp::fast_exp2(n * off)).clamp(self.min, self.max)
            }
        }
    }

    /// `offset` on a 0..1 display value (the UI's mod bars).
    pub fn offset_normalized(&self, n: f32, off: f32) -> f32 {
        match self.law {
            OffsetLaw::Linear => (n + off).clamp(0.0, 1.0),
            OffsetLaw::Octaves(_) => {
                self.normalize(self.offset(self.min + n * (self.max - self.min), off))
            }
        }
    }
```

Replace `apply_offset`'s body with:

```rust
    if let Some(s) = blk.spec(id) {
        blk.write(id, s.offset(blk.get(id), off));
    }
```

(and its doc's first line with `/// Apply a modulation offset (spec §4) by the spec's law, written raw.`). The linear arm is the old expression, so every other destination is bit-identical.

In `params.rs`, CUTOFF's spec (the first of `FILTER_SPECS`) gets `.octaves(crate::dsp::filter::CUTOFF_OCTAVES)` after its closing parenthesis.

- [ ] **Step 5: `routed_cutoff` and the ramp**

In `chimera-core/src/dsp/filter.rs`, after the mode tables:

```rust
/// Octaves a full CUTOFF route moves (spec § 3).
pub const CUTOFF_OCTAVES: f32 = 10.0;

/// The cutoff a route sum `sum` gives from `base` (spec § 3): Σ = 0 is
/// `base` bit for bit; the filter clamps to 0.49·fs as well.
pub fn routed_cutoff(base: f32, sum: f32) -> f32 {
    if sum == 0.0 {
        return base;
    }
    crate::params::FILTER_SPECS[0].offset(base, sum)
}
```

Give `SvfFilter` a third field and replace `process`:

```rust
pub struct SvfFilter {
    ic1eq: [f32; 2],
    ic2eq: [f32; 2],
    /// The last block's `g`; `None` on a fresh voice, which starts unramped.
    g: Option<f32>,
}
```

(`new` sets `g: None`), and:

```rust
    pub fn process(&mut self, buf: &mut [f32], params: &FilterParams, sample_rate: u32) {
        let mode = params.mode();
        let drive = params.drive;
        let fc = params.cutoff.min(sample_rate as f32 * 0.49);
        let g = crate::dsp::fast_tan(core::f32::consts::PI * fc / sample_rate as f32);
        let from = self.g.replace(g).unwrap_or(g);

        // Resonance: full range. k=2 (none) to k=0.01 (screaming self-osc).
        // Let it go all the way — the nonlinear feedback keeps it stable.
        let k = 2.0 * (1.0 - params.resonance) + 0.01;

        if from == g {
            for sample in buf.iter_mut() {
                *sample = self.tick(mode, *sample * (1.0 + drive * 4.0), g, k);
            }
        } else {
            // #53: `g` ramps to this block's value, reached on the last sample.
            let step = (g - from) / buf.len() as f32;
            for (i, sample) in buf.iter_mut().enumerate() {
                let gi = g_at(from, step, i);
                *sample = self.tick(mode, *sample * (1.0 + drive * 4.0), gi, k);
            }
        }
    }

    /// Forget the last `g`: the next block starts without a ramp.
    pub fn hold(&mut self) {
        self.g = None;
    }

    /// The last block's `g` (`None`: no ramp next block).
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.g
    }

    fn tick(&mut self, mode: FilterMode, input: f32, g: f32, k: f32) -> f32 {
        match mode {
            // … the eight arms moved here from `process`, unchanged …
        }
    }
```

Move the `match mode { … }` block from the old loop into `tick` verbatim (each arm already returns the sample). Beside `routed_cutoff`:

```rust
/// Sample `i`'s coefficient on a ramp from `from` by `step` a sample: the
/// block's last sample lands on the new `g` (#53).
#[inline(always)]
pub fn g_at(from: f32, step: f32, i: usize) -> f32 {
    from + step * (i + 1) as f32
}
```

In `chimera-core/src/dsp/voice.rs`, `trigger` starts a note that follows silence without a ramp (spec § 3: "on a fresh note there is no ramp"; a voice that ended on its own is inactive but not reset, so its `g` would survive). Its first line becomes:

```rust
        if !self.active {
            self.filter.hold(); // a fresh note: no ramp from the last note's cutoff
        }
```

A retrigger of a sounding voice keeps the ramp from its last cutoff (Decisions table). Add at the end of `voice.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Spec § 3: a fresh note starts without a ramp, even on a voice
    /// whose engine went quiet on its own (inactive, never reset).
    #[test]
    fn a_note_after_silence_starts_without_a_ramp() {
        let p = ParamSnapshot::default();
        let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        let mut b = [0.0f32; BLOCK_SIZE];
        v.render(&mut b, &p, &ModState::new());
        assert!(v.filter.last_g().is_some());
        v.active = false; // its engine went quiet
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        assert!(v.filter.last_g().is_none());
        // A retrigger of a sounding voice keeps it.
        v.render(&mut b, &p, &ModState::new());
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p);
        assert!(v.filter.last_g().is_some());
    }
}
```

- [ ] **Step 6: The mod bars use the law**

In `chimera-core/src/ui/mod.rs`, `update`, replace the loop `// Apply offsets to the 6 display values` with:

```rust
            for (i, value) in values.iter_mut().enumerate() {
                let Some(addr) = slot_addr(def, i, self.sel_op) else {
                    continue;
                };
                let offset = sound.mod_state.offset_for(addr, &mod_sources);
                if offset != 0.0
                    && let Some(spec) = addr.spec()
                {
                    *value = spec.offset_normalized(*value, offset);
                }
            }
```

- [ ] **Step 7: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS except `golden_test`'s `goldens_match` and `goldens_match_through_the_instrument`, which report exactly `algo_lfo_cutoff` and `modal_lfo_cutoff` (their LFO → CUTOFF route is in octaves now, and ramped). Any other mismatch, a factory golden above all, is a bug to fix before going on.

- [ ] **Step 8: The sanity gate**

Run: `env $T cargo test -p chimera-core --test sanity_test`
Expected: PASS. `every_algo_case_is_finite_bounded_audible_and_ends` covers `algo_lfo_cutoff` (finite, within ±1, audible, silent after note-off). `modal_lfo_cutoff` is Modal's known-broken case (#10) and is not gated.

- [ ] **Step 9: Write the two cases out for a listen**

Add this test to `golden_test.rs` for the step, run it with `env $T cargo test -p chimera-core --test golden_test write_lfo_cutoff_wavs -- --ignored`, and delete it again (it is not committed):

```rust
#[test]
#[ignore = "listening aid"]
fn write_lfo_cutoff_wavs() {
    for case in [Case::AlgoLfoCutoff, Case::ModalLfoCutoff] {
        let out = render_case(case);
        let n = out.len() as u32 * 2;
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + n).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        // chunk 16, PCM mono, rate, byte rate, block align 2 and 16 bits
        for x in [16u32, 1 | 1 << 16, SR, SR * 2, 2 | 16 << 16] {
            w.extend_from_slice(&x.to_le_bytes());
        }
        w.extend_from_slice(b"data");
        w.extend_from_slice(&n.to_le_bytes());
        for s in out {
            w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        let dir = "/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad";
        std::fs::write(format!("{dir}/{}.wav", case.name()), w).unwrap();
    }
}
```

- [ ] **Step 9b: STOP. Ask the owner to listen, and wait**

Send the owner this, then wait for the answer before re-recording anything:

> Filter-routing Task 3 moves two audio goldens. Please listen to `algo_lfo_cutoff.wav` and `modal_lfo_cutoff.wav` in the scratchpad: a cutoff wobbling by about ±5 octaves (64/127 × 10) at 5 Hz. It should be smooth, with no zipper or clicks. OK to re-record them?

If the owner hears a problem, fix the ramp (Step 5) and go back to Step 7. Re-record only on the owner's yes.

- [ ] **Step 10: Re-record the two rows**

Run: `GOLDEN_RECORD=1 env $T cargo test -p chimera-core --test golden_test goldens_match -- --nocapture`. Paste only the `algo_lfo_cutoff` and `modal_lfo_cutoff` rows over theirs, with the comment `// Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).`

- [ ] **Step 11: Run every test**

Run: `env $T cargo test -p chimera-core`
Expected: PASS, including `modulated_cases_differ_from_unmodulated` and every factory golden.

- [ ] **Step 12: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/mod.rs chimera-core/src/block.rs chimera-core/src/params.rs \
  chimera-core/src/dsp/filter.rs chimera-core/src/dsp/voice.rs chimera-core/src/ui/mod.rs \
  chimera-core/tests/filter_test.rs chimera-core/tests/golden_test.rs
git commit -m "CUTOFF routes in octaves; the SVF ramps g across a block (#53)"
```

---

### Task 4: Eight matrix sources; ENV, KEY and LFO as route knobs

The matrix takes its eight source rows (spec § 2), and `Voice` fills ENV 1 (the envelope's raw contour, no velocity), LFO 1 (now always running), VEL and NOTE. ENV 2, ENV 3, LFO 2 and LFO 3 read 0 until Tasks 5 and 7 run them. The FLT page's ENV and KEY, and FLT › MODE's LFO, become views of the routes ENV 1 → CUTOFF, NOTE → CUTOFF and LFO 1 → CUTOFF, so every FLT knob changes the sound. The slice ships after Task 7, once the other four rows run. The matrix sum reads `amount / 127` from a table, not a divide, since it now sums eight rows.

**Files:**
- Modify: `chimera-core/src/modulation.rs` (`ModSource`, `note_source`, `amount_scale`, `sum_for`)
- Modify: `chimera-core/src/dsp/voice.rs:254-263` (the eight source values)
- Modify: `chimera-core/src/dsp/envelope.rs` (`level()` replaces `current_level()`)
- Modify: `chimera-core/src/block.rs` (`ValFmt::Route`)
- Modify: `chimera-core/src/ui/fmt.rs` (`Route` readout)
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding::Route`, `ParamSlot::route`)
- Modify: `chimera-core/src/ui/block_registry.rs` (`PART_MOD_SOURCES`, FILTER's ENV/KEY, FILTER_MODE's LFO)
- Modify: `chimera-core/src/ui/mod_grid.rs` (`col_of`, `route`, `set`; its row struct `ModSource` renamed `SourceRow`)
- Modify: `chimera-core/src/ui/mod.rs` (`CUTOFF`, `edit_route`, the encoder loop, `display_values`)
- Create: `chimera-core/tests/routing_test.rs`, `chimera-core/tests/flt_page_test.rs`
- Modify tests: `binding_test.rs` (`part_chains_offer_env_and_lfo_sources`), `matrix_view_test.rs`, `ui_routing_test.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: `routed_cutoff`, `CUTOFF_OCTAVES` (Task 3).
- Produces:
  - `modulation::ModSource { Env1 = 0, Lfo1 = 1, Env2 = 2, Env3 = 3, Lfo2 = 4, Lfo3 = 5, Vel = 6, Note = 7 }` with `ALL`, `index(self) -> usize`, `tag(self) -> &'static str`.
  - `modulation::note_source(note: MidiNote) -> f32`; `modulation::amount_scale(a: i8) -> f32` (`a / 127`, from a table).
  - `ui::mod_grid::SourceRow { name }` (was `mod_grid::ModSource`).
  - `block::ValFmt::Route` (a route amount as `amount_value`, shown as `+47%`).
  - `ui::block_def::SlotBinding::Route(ModSource)`; `ParamSlot::route(src: ModSource, label: &'static str) -> ParamSlot` (const).
  - `MatrixState::col_of(&self, ParamAddr) -> Option<usize>`, `MatrixState::route(&self, row: usize, addr: ParamAddr) -> Option<i8>`, `MatrixState::set(&mut self, row: usize, col: usize, amount: i8)`.
  - `ui::CUTOFF: ParamAddr` (crate-private const in `ui/mod.rs`).

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/routing_test.rs`:

```rust
//! The matrix's eight sources (filter-routing spec § 2).

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{ModSource, ModState, note_source};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::params::EngineType;
use chimera_core::ui::block_registry::PART_MOD_SOURCES;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

#[test]
fn sources_are_in_spec_order() {
    let tags = ["E1", "LF1", "E2", "E3", "LF2", "LF3", "VEL", "NTE"];
    for (i, s) in ModSource::ALL.iter().enumerate() {
        assert_eq!(s.index(), i);
        assert_eq!(s.tag(), tags[i]);
        assert!(s.tag().len() <= 3, "#15");
    }
    assert_eq!(PART_MOD_SOURCES, tags);
    for ct in EngineType::ALL {
        assert_eq!(chain_def_for(ct).mod_sources, tags, "{ct:?}");
    }
    // A two-source `ModState` still maps 0 → ENV1 and 1 → LFO1.
    assert_eq!((ModSource::Env1.index(), ModSource::Lfo1.index()), (0, 1));
}

#[test]
fn note_source_is_a_tenth_of_an_octave_law() {
    assert_eq!(note_source(MidiNote::new(60).unwrap()), 0.0);
    assert_eq!(note_source(MidiNote::new(72).unwrap()), 0.1);
    assert_eq!(note_source(MidiNote::new(0).unwrap()), -0.5);
}

/// 24 blocks of `note` with one route `source → CUTOFF` at `amount`.
fn render(source: ModSource, amount: i8, note: u8, vel: u8) -> Vec<f32> {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.cutoff = 1000.0;
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF, *b"FLTCUTOF").unwrap();
    let mut ms = ModState::from_registry(&reg, 8);
    ms.set_amount(source.index(), 0, amount);
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(note).unwrap(), Velocity::new(vel).unwrap(), &p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..24 {
        v.render(&mut b, &p, &ms);
        out.extend_from_slice(&b);
    }
    out
}

#[test]
fn env1_lfo1_vel_and_note_move_their_destination() {
    for (source, note) in [
        (ModSource::Env1, 60),
        (ModSource::Lfo1, 60),
        (ModSource::Vel, 60),
        (ModSource::Note, 72),
    ] {
        assert_ne!(
            render(source, 100, note, 100),
            render(source, 0, note, 100),
            "{source:?}"
        );
    }
    // VEL follows the velocity. The route's effect is measured against the
    // same velocity with the route at 0, so the engine's own velocity
    // response cancels; a harder note opens the filter further. (ENV 1
    // carries none: `envelope.rs`'s unit test until Task 5, then `note_on`'s
    // signature, which takes none.)
    let effect = |vel| {
        let (on, off) = (render(ModSource::Vel, 100, 60, vel), render(ModSource::Vel, 0, 60, vel));
        let diff: f32 = on.iter().zip(&off).map(|(a, b)| (a - b).abs()).sum();
        diff / off.iter().map(|x| x.abs()).sum::<f32>()
    };
    assert!(effect(120) > effect(30), "{} vs {}", effect(120), effect(30));
}
```

Create `chimera-core/tests/flt_page_test.rs`:

```rust
//! The FLT pages are honest (filter-routing spec § Tests "Knobs are
//! honest", "Route knobs").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModSource;
use chimera_core::params::FilterParams;
use chimera_core::params::EngineType;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::renderer::amount_of;
use chimera_core::ui::{PrimeStatus, UiState};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, ButtonId, EncoderId};
use screen::*;

const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const ENC: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

fn flt_node(ct: EngineType) -> usize {
    chain_def_for(ct)
        .blocks
        .iter()
        .position(|b| b.def.id == FILTER.id)
        .expect("FLT is on every Part chain")
}

/// Part 1 on `ct`'s init Sound with a held saw worth filtering, on FLT.
fn on_flt(ct: EngineType) -> UiState {
    let mut ui = UiState::new();
    load_init(&mut ui, ct);
    let p = ui.params_mut();
    p.filter.cutoff = 2000.0;
    p.algo.ops[0].wave = WaveId::SAW.get();
    for _ in 0..flt_node(ct) {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

/// 32 blocks of note 72 (so KEY moves the cutoff) from the UI's Sound.
fn render(ui: &UiState) -> Vec<f32> {
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::DEFAULT, ui.params());
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..32 {
        v.render(&mut b, ui.params(), ui.mod_state());
        out.extend_from_slice(&b);
    }
    out
}

/// Every bound FLT and FLT › MODE knob changes a held note on Algo and
/// Modal once turned (a route knob once its route is nonzero).
#[test]
fn every_flt_knob_changes_a_held_note() {
    for ct in EngineType::ALL {
        for (sub, slots) in [(0, &[1usize, 2, 3, 4, 5][..]), (1, &[0usize, 1, 2][..])] {
            for &slot in slots {
                let mut ui = on_flt(ct);
                if sub == 1 {
                    feed(&mut ui, Input::press(ButtonId::Edit));
                }
                let before = render(&ui);
                feed(&mut ui, Input::turn(ENC[slot], 40));
                assert_ne!(render(&ui), before, "{ct:?} page {sub} slot {slot}");
            }
        }
    }
}

/// Removes CUTOFF from Part 1's matrix and reloads it (B1 snaps home).
fn without_cutoff(ui: &mut UiState, fill: bool) {
    let sound = &mut ui.performance.parts[0].sound;
    sound.dest_registry.remove(CUTOFF);
    if fill {
        for b in BlockRef::ALL {
            for s in b.specs() {
                let a = ParamAddr::new(b, s.id);
                if a != CUTOFF && a.modulatable() {
                    let _ = sound.dest_registry.add(a, *b"FILL\0\0\0\0");
                }
            }
        }
    }
    // The audio-side matrix follows the registry (no routes kept).
    sound.mod_state = chimera_core::modulation::ModState::from_registry(&sound.dest_registry, 8);
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

#[test]
fn a_route_knob_creates_its_column() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, false);
    let col = |ui: &UiState| (0..ui.mod_state().num_dests()).find(|&d| ui.mod_state().dest(d) == CUTOFF);
    assert_eq!(col(&ui), None);
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // ENV
    let d = col(&ui).expect("the turn created CUTOFF's column");
    assert_eq!(ui.mod_state().amount(ModSource::Env1.index(), d), 5);
    // The knob and the matrix cell show the same amount.
    settle(&mut ui);
    assert_eq!(amount_of(ui.renderer.anim[4].current()), 5);
}

#[test]
fn a_full_matrix_keeps_the_route_knob_off() {
    let mut ui = on_flt(EngineType::Algo);
    without_cutoff(&mut ui, true);
    feed(&mut ui, Input::turn(EncoderId::F, 5)); // KEY
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Full));
    assert!((0..ui.mod_state().num_dests()).all(|d| ui.mod_state().dest(d) != CUTOFF));
}
```

In `matrix_view_test.rs`, the fixture's focus band names the source by its new tag: `components::focus_route(&mut want, "LF1", "FLT CUTOFF", "+42", amount_value(42));`, and `route_destination_names_the_block_and_fits` keeps using `PART_MOD_SOURCES` (now eight tags). In `ui_routing_test.rs`, `algo_matrix_rows_are_env_and_lfo` expects the eight tags (`PART_MOD_SOURCES`).

In `binding_test.rs`, `part_chains_offer_env_and_lfo_sources` asserts the old two names: change its first assertion to `assert_eq!(chain_def_for(ct).mod_sources, chimera_core::ui::block_registry::PART_MOD_SOURCES, "{ct:?}");` and rename it `part_chains_offer_the_eight_sources`.

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test routing_test --test flt_page_test`
Expected: FAIL to compile: no `ModSource`, `note_source`; `UiState` has no route knobs.

- [ ] **Step 3: The sources**

In `chimera-core/src/modulation.rs`, after `MAX_MOD_DESTS`:

```rust
/// The matrix's source rows, in `Voice`'s order (spec § 2). Indices are
/// stored: 0 and 1 keep their old meaning (the envelope and the LFO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ModSource {
    Env1 = 0,
    Lfo1 = 1,
    Env2 = 2,
    Env3 = 3,
    Lfo2 = 4,
    Lfo3 = 5,
    Vel = 6,
    Note = 7,
}

impl ModSource {
    pub const ALL: [ModSource; MAX_MOD_SOURCES] = [
        ModSource::Env1,
        ModSource::Lfo1,
        ModSource::Env2,
        ModSource::Env3,
        ModSource::Lfo2,
        ModSource::Lfo3,
        ModSource::Vel,
        ModSource::Note,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The matrix row's tag (≤ 3 characters, #15).
    pub const fn tag(self) -> &'static str {
        match self {
            ModSource::Env1 => "E1",
            ModSource::Lfo1 => "LF1",
            ModSource::Env2 => "E2",
            ModSource::Env3 => "E3",
            ModSource::Lfo2 => "LF2",
            ModSource::Lfo3 => "LF3",
            ModSource::Vel => "VEL",
            ModSource::Note => "NTE",
        }
    }
}

/// The NOTE source: `(note − 60) / 120`, clamped to −1..1, so a route at
/// 127 into CUTOFF tracks one octave per octave (spec § 2, § 3).
pub fn note_source(note: crate::MidiNote) -> f32 {
    ((note.get() as f32 - 60.0) / 120.0).clamp(-1.0, 1.0)
}
```

In `chimera-core/src/ui/block_registry.rs`, replace `PART_MOD_SOURCES` with:

```rust
/// Mod sources every Part voice produces, in `ModSource` order (spec § 2).
pub static PART_MOD_SOURCES: [&str; crate::modulation::MAX_MOD_SOURCES] =
    ["E1", "LF1", "E2", "E3", "LF2", "LF3", "VEL", "NTE"];
```

- [ ] **Step 4: `Voice` fills them**

In `chimera-core/src/dsp/envelope.rs`, replace `current_level` with:

```rust
    /// The raw contour, 0..1: the ENV 1 source (spec § 1, no velocity).
    pub fn level(&self) -> f32 {
        self.level
    }
```

and at the end of `envelope.rs`, the check that ENV 1 carries no velocity (a render can't show it: the engine's own level follows velocity). Task 5 replaces the file, and from there `note_on` takes no velocity, so the signature carries the rule:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env1_source_ignores_velocity() {
        let p = EnvParams::default();
        let (mut soft, mut hard) = (Envelope::new(), Envelope::new());
        soft.note_on(0.2);
        hard.note_on(1.0);
        for _ in 0..200 {
            soft.process(&p, 48_000);
            hard.process(&p, 48_000);
        }
        assert!(soft.level() > 0.0);
        assert_eq!(soft.level(), hard.level());
    }
}
```

In `chimera-core/src/dsp/voice.rs`, import `use crate::modulation::{MAX_MOD_SOURCES, ModSource, ModState, note_source};` and replace the two `// Source 0` / `// Source 1` blocks with:

```rust
            let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
            mod_values[ModSource::Env1.index()] = self.amp_env.level();
            mod_values[ModSource::Lfo1.index()] = self.lfo.process(&params.lfo, sample_rate);
            mod_values[ModSource::Vel.index()] = self.last_velocity.unit();
            mod_values[ModSource::Note.index()] = note_source(self.last_note);
```

(LFO 1 now runs every block; its value is still taken before the advance, and only a routed source reaches a golden.)

- [ ] **Step 5: The route readout**

In `chimera-core/src/block.rs`, add to `ValFmt` after `Pan`:

```rust
    /// A matrix route's amount (`amount_value`, 0.5 = 0), shown as a
    /// percentage of 127 (spec § 6).
    Route,
```

In `snap_points`, `ValFmt::Bi | ValFmt::Pan` becomes `ValFmt::Bi | ValFmt::Pan | ValFmt::Route`; in `is_bipolar`, add `| ValFmt::Route`. In `chimera-core/src/ui/fmt.rs`, `fmt_val` gains:

```rust
        ValFmt::Route => {
            let a = crate::ui::renderer::amount_of(val) as i32;
            let pct = (a * 100 + a.signum() * 63) / 127;
            let _ = if pct > 0 {
                write!(buf, "+{pct}%")
            } else {
                write!(buf, "{pct}%")
            };
        }
```

(`(a·100 ± 63) / 127` rounds half away from zero in integers: 127 → 100, 64 → 50, 1 → 1.)

- [ ] **Step 6: The route binding**

In `chimera-core/src/ui/block_def.rs`, add to `SlotBinding`:

```rust
    /// A view of the matrix route `source → CUTOFF` (spec § 6): the knob
    /// shows and edits the route's amount.
    Route(crate::modulation::ModSource),
```

to `impl ParamSlot`:

```rust
    pub const fn route(source: crate::modulation::ModSource, label: &'static str) -> Self {
        Self {
            binding: SlotBinding::Route(source),
            label_override: Some(label),
        }
    }
```

In `spec` and `slot_addr`, add `SlotBinding::Route(_)` to the arm returning `None`. In `label`, `SlotBinding::Route(s)` returns `s.tag()` (the override covers every real slot). In `format`, `SlotBinding::Route(_) => ValFmt::Route`.

In `block_registry.rs`, FILTER's slots e and f become `ParamSlot::route(ModSource::Env1, "ENV")` and `ParamSlot::route(ModSource::Note, "KEY")`. FILTER_MODE's slot c becomes `ParamSlot::route(ModSource::Lfo1, "LFO")`. Add `use crate::modulation::ModSource;`.

- [ ] **Step 7: The matrix's cell helpers, and one `ModSource`**

In `chimera-core/src/ui/mod_grid.rs`, rename the row struct `ModSource` to `SourceRow` (its three uses in that file: the struct, `sources: [Option<SourceRow>; MAX_SOURCES]` and `rebuild_sources`), so `modulation::ModSource` is the one type of that name. Then in `impl MatrixState`:

```rust
    /// The column of destination `addr`, if the matrix has one.
    pub fn col_of(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.num_dests).find(|&c| self.dests[c].is_some_and(|d| d.addr == addr))
    }

    /// The amount of route `row → addr`; `None` without a column.
    pub fn route(&self, row: usize, addr: ParamAddr) -> Option<i8> {
        self.col_of(addr).map(|c| self.amounts[row][c])
    }

    /// Set cell (`row`, `col`); out of range does nothing.
    pub fn set(&mut self, row: usize, col: usize, amount: i8) {
        if row < self.num_sources && col < self.num_dests {
            self.amounts[row][col] = amount;
        }
    }
```

and `adjust_amount`'s body after its guard becomes:

```rust
        let a = self.amounts[self.sel_row][self.sel_col];
        self.set(
            self.sel_row,
            self.sel_col,
            (a as i16 + delta as i16).clamp(-127, 127) as i8,
        );
```

- [ ] **Step 8: The knobs turn the routes**

In `chimera-core/src/ui/mod.rs`, after the imports:

```rust
/// CUTOFF: the destination the filter's route knobs view (spec § 6).
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, crate::params::FilterParams::CUTOFF);

/// MIX + turn on a route knob: the next of −127, 0, +127 that way.
fn snap_amount(a: i8, delta: i8) -> i8 {
    match (delta > 0, a) {
        (true, a) if a < 0 => 0,
        (true, _) => 127,
        (false, a) if a > 0 => 0,
        (false, _) => -127,
    }
}
```

In `impl UiState`:

```rust
    /// A route knob turned: create CUTOFF's column if needed (MATRIX FULL
    /// when there is no room), then set `source → CUTOFF` to `f(amount)`.
    fn edit_route(&mut self, source: crate::modulation::ModSource, f: impl FnOnce(i8) -> i8) {
        let at = self.active_part;
        let sound = &mut self.performance.parts[at].sound;
        if !sound.dest_registry.is_primed(CUTOFF) {
            if let Err(e) = sound.dest_registry.add(CUTOFF, *b"FLTCUTOF") {
                self.prime_status = Some(e.into());
                return;
            }
            self.matrix_state
                .rebuild_dests_from_registry(&sound.dest_registry);
            self.matrix_state.load_amounts(&sound.mod_state);
        }
        if let Some(col) = self.matrix_state.col_of(CUTOFF) {
            let row = source.index();
            let a = self.matrix_state.amounts[row][col];
            self.matrix_state.set(row, col, f(a));
            self.sync_mod_state(at);
        }
    }
```

In `handle_input`'s non-matrix encoder loop, right after the `self.focus.touch(...)` block:

```rust
                    if let block_def::SlotBinding::Route(src) = def.params[i].binding {
                        self.edit_route(src, |a| {
                            if shift {
                                snap_amount(a, delta)
                            } else {
                                (a as i16 + delta as i16).clamp(-127, 127) as i8
                            }
                        });
                        continue;
                    }
```

In `display_values`, before `if def.layout == PageLayout::Matrix`:

```rust
        for (i, slot) in def.params.iter().enumerate() {
            if let block_def::SlotBinding::Route(src) = slot.binding {
                values[i] = renderer::amount_value(
                    self.matrix_state.route(src.index(), CUTOFF).unwrap_or(0),
                );
            }
        }
```

`update` calls `display_values`, so the knob follows edits made on the matrix page too.

- [ ] **Step 8b: The sum reads a table**

With eight rows, the matrix pass could divide up to 128 times a block. In `modulation.rs`:

```rust
/// `amount / 127` for every amount (index `amount + 127`): the same f32 the
/// divide gives (const float arithmetic is IEEE), read instead of divided.
static AMOUNT_SCALE: [f32; 255] = {
    let mut t = [0.0f32; 255];
    let mut i = 0;
    while i < 255 {
        t[i] = (i as i32 - 127) as f32 / 127.0;
        i += 1;
    }
    t
};

/// `a / 127` (−128 reads as −127).
pub fn amount_scale(a: i8) -> f32 {
    AMOUNT_SCALE[(a.max(-127) as i32 + 127) as usize]
}
```

and in `sum_for`, `total += source_values[si] * (amt as f32 / 127.0);` becomes `total += source_values[si] * amount_scale(amt);` (the same value, so every golden stays bit-identical). `routing_test.rs` gains:

```rust
#[test]
fn the_amount_table_is_the_divide() {
    for a in -127i8..=127 {
        assert_eq!(chimera_core::modulation::amount_scale(a).to_bits(), (a as f32 / 127.0).to_bits(), "{a}");
    }
}
```

- [ ] **Step 9: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS except `screen_goldens_match` (next step). Every audio golden is bit-identical: ENV 1's velocity factor and the always-running LFO reach no golden's route, and the table is the divide.

- [ ] **Step 10: Look at the screens and re-record**

Look at `bigviz_filter`: the dash in slot a, then CUTOFF, RESO and MODE `LP24`, then ENV `0%` and KEY `0%` with centred bipolar bars. Look at `mod_matrix`: rows E1, LF1, E2 (three visible; the column shows its routes), and the focus band reads `LF1 → FLT CUTOFF`. Re-record only those two rows (`SCREEN_RECORD=1 …`).

- [ ] **Step 11: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/modulation.rs chimera-core/src/dsp/voice.rs chimera-core/src/dsp/envelope.rs \
  chimera-core/src/block.rs chimera-core/src/ui/fmt.rs chimera-core/src/ui/block_def.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/mod.rs \
  chimera-core/tests/routing_test.rs chimera-core/tests/flt_page_test.rs chimera-core/tests/binding_test.rs \
  chimera-core/tests/matrix_view_test.rs chimera-core/tests/ui_routing_test.rs \
  chimera-core/tests/screen_golden_test.rs
git commit -m "Eight matrix sources; FLT's ENV, KEY and LFO are views of their CUTOFF routes"
```

---

### Task 5: Envelope A, and ENV 1–3 on it

Envelope A is built as a pure core (`dsp/modulator/`), and `Envelope` becomes the ENV slot running it. `EnvParams` takes its new fields: A, D and R become positions, and H, TYPE, SPEED, HOLD and TIME arrive. All three ENV slots run per block, which fills sources 0, 2 and 3 and removes today's per-sample amp-envelope tick and its divide. The role-named `BlockRef`s become `BlockRef::Env(EnvSlot)`, and the three unreachable envelope pages and `ENVELOPE_CHAIN` retire. TYPE B is not on a page until Task 16; until Task 6 a slot runs A whatever its TYPE. Each path (per sample and per block) is held to an f64 reference of the spec's formulas (ADR 0036), and the per-sample path used on the VCA runs each stage in a tight loop.

**Files:**
- Create: `chimera-core/src/dsp/modulator/mod.rs`, `chimera-core/src/dsp/modulator/law.rs`, `chimera-core/src/dsp/modulator/env_a.rs`
- Modify: `chimera-core/src/dsp/mod.rs` (`pub mod modulator;`)
- Rewrite: `chimera-core/src/dsp/envelope.rs` (`EnvMods`, `Envelope`)
- Modify: `chimera-core/src/params.rs:90-191` (`EnvParams`, `ENV_SPECS`, the `Blocks` arms)
- Modify: `chimera-core/src/addr.rs` (`BlockRef::Env(EnvSlot)`)
- Modify: `chimera-core/src/modulation.rs` (`ModSource::of_env`)
- Modify: `chimera-core/src/preset.rs` (`PartEdit` arms)
- Modify: `chimera-core/src/dsp/voice.rs` (`envs: [Envelope; 3]`, modulators every block)
- Modify: `chimera-core/src/ui/block_registry.rs` (ENVELOPE binds `Env(Env1)`; delete `ENV_AMP`, `ENV_FILTER`, `ENV_AUX`, `ENVELOPE_BLOCKS`, `ENVELOPE_CHAIN`)
- Modify: `chimera-core/src/ui/page.rs` (delete `PageId::EnvAmp`, `EnvFilter`, `EnvAux`, `ENV_PAGE`; DEMO_MOTION names `Env(Env1)`, `Env(Env2)`)
- Modify: `chimera-core/src/ui/mod_grid.rs` (`block_tag`)
- Create: `chimera-core/tests/env_a_test.rs`; delete `chimera-core/tests/envelope_test.rs` (its three cases are covered there)
- Modify tests: `addr_test.rs`, `binding_test.rs`, `focus_test.rs`, `mod_registry_test.rs`, `page_block_test.rs`, `part_page_test.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: `fast_exp2`, `fast_log2` (Task 3); `ModSource` (Task 4).
- Produces:
  - `dsp::modulator::{EnvSlot, EnvType, EnvSpeed, HoldPos, pick}`: `EnvSlot::{ALL, index()}`; each choice enum `#[repr(u8)]` with `ALL`; `pick<T: Copy>(all: &[T], v: f32) -> T`.
  - `dsp::modulator::law::{Range { min, oct }, Range::at(self, p) -> f32, SpeedRanges { hold, attack, dec_rel }, speed_ranges(EnvSpeed) -> SpeedRanges, rc_k(tau, fs) -> f32, rc_coeff(k) -> f32}`.
  - `dsp::modulator::env_a::{Stage, ACoefs, ACoefs::new(&EnvParams, time: f32, sample_rate: u32), EnvA}`; `EnvA::{new, stage, level, is_idle, rising, note_on, tick(&mut self, &ACoefs, key: bool) -> f32, fill(&mut self, &ACoefs, key: bool, out: &mut [f32]), advance(&mut self, &ACoefs, key: bool, n: u32), stage_samples(&self, &ACoefs) -> u32, enter(&mut self, level: f32, rising: bool, key: bool, sus: f32)}`. `fill` writes the levels `tick` would, with the same arithmetic.
  - `dsp::envelope::{EnvMods { level: Option<f32>, time }, EnvMods::NONE, Envelope}`; `Envelope::{new, note_on(&mut self, &EnvParams), output(&self) -> f32, is_idle(&self) -> bool, run_block(&mut self, &EnvParams, &EnvMods, key: bool, sample_rate: u32, vca: Option<(&mut [f32; BLOCK_SIZE], f32)>) -> f32}`. `run_block` returns the block-start output; with `vca` it fills a block of levels and adds `amount · level[n] · peak[n]` into the buffer, the peak ramped from the last block's LEVEL to this one's.
  - `EnvParams` ids `ATTACK 0, DECAY 1, SUSTAIN 2, RELEASE 3, LEVEL 4, VEL_SENS 5, HOLD 6, TYPE 7, SPEED 8, HOLD_POS 9, TIME 10`; fields `attack, decay, sustain, release, level, vel_sens, hold, env_type, speed, hold_pos, time`.
  - `addr::BlockRef::Env(EnvSlot)`; `ModSource::of_env(EnvSlot) -> ModSource`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/env_a_test.rs`:

```rust
//! Envelope A (filter-routing spec § Envelope A, § Tests "Envelope A").

use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::modulator::env_a::{ACoefs, EnvA, Stage};
use chimera_core::dsp::modulator::law::speed_ranges;
use chimera_core::dsp::modulator::{EnvSpeed, HoldPos};
use chimera_core::params::EnvParams;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

fn params(speed: EnvSpeed) -> EnvParams {
    EnvParams {
        speed,
        hold: 0.0,
        hold_pos: HoldPos::Off,
        ..EnvParams::default()
    }
}

/// One sample (a stage ends on the first tick past its time: up to one
/// sample late, plus float error), or 2 ppm on long stages (f32's limit).
fn close(samples: f64, secs: f64, sr: u32, what: &str) {
    let want = secs * sr as f64;
    assert!(
        (samples - want).abs() <= 1.05f64.max(want * 2e-6),
        "{what}: {samples} samples, want {want}"
    );
}

/// The manual's ranges at every SPEED, at both slider ends.
#[test]
fn slider_ends_give_the_manuals_times() {
    // (speed, H max, A min, A max, D/R min, D/R max), seconds.
    let table = [
        (EnvSpeed::Fast, 2.5, 0.2e-3, 1.5, 0.6e-3, 2.5),
        (EnvSpeed::Med, 10.0, 2e-3, 10.0, 3.5e-3, 10.0),
        (EnvSpeed::Slow, 60.0, 9.3e-3, 60.0, 30e-3, 60.0),
    ];
    for (s, h1, a0, a1, d0, d1) in table {
        let r = speed_ranges(s);
        let at = |range: chimera_core::dsp::modulator::law::Range, p| range.at(p) as f64;
        close(at(r.hold, 0.0) * SR as f64, 1e-6, SR, "H min");
        close(at(r.hold, 1.0) * SR as f64, h1, SR, "H max");
        close(at(r.attack, 0.0) * SR as f64, a0, SR, "A min");
        close(at(r.attack, 1.0) * SR as f64, a1, SR, "A max");
        close(at(r.dec_rel, 0.0) * SR as f64, d0, SR, "D min");
        close(at(r.dec_rel, 1.0) * SR as f64, d1, SR, "D max");
    }
}

/// Samples the running stage takes from `note_on` (A: 0 → 1) and a full
/// decay (1 → S = 0), by the closed form's placement.
#[test]
fn a_full_swing_takes_the_sliders_time() {
    for speed in [EnvSpeed::Fast, EnvSpeed::Med, EnvSpeed::Slow] {
        for pos in [0.0f32, 0.3, 0.7] {
            let r = speed_ranges(speed);
            let mut p = params(speed);
            (p.attack, p.decay, p.sustain) = (pos, pos, 0.0);
            let c = ACoefs::new(&p, 0.0, SR);
            let mut e = EnvA::new();
            e.note_on();
            close(e.stage_samples(&c) as f64, r.attack.at(pos) as f64, SR, "attack");
            let n = e.stage_samples(&c);
            e.advance(&c, true, n);
            assert_eq!(e.stage(), Stage::Decay, "{speed:?} {pos}");
            close(e.stage_samples(&c) as f64, r.dec_rel.at(pos) as f64, SR, "decay");
        }
    }
}

/// Short stages, tick by tick: the per-sample path agrees with the law.
#[test]
fn ticks_take_the_sliders_time() {
    let mut p = params(EnvSpeed::Med); // A 10 ms, D 300 ms
    p.sustain = 0.0;
    let c = ACoefs::new(&p, 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    let (mut attack, mut decay) = (0, 0);
    while e.stage() != Stage::Sustain {
        match e.stage() {
            Stage::Attack => attack += 1,
            _ => decay += 1,
        }
        e.tick(&c, true);
    }
    close(attack as f64, 0.010, SR, "attack");
    close(decay as f64, 0.299_18, SR, "decay");
}

/// Review Focus 2: at 44.1 kHz a MED 10 ms attack takes 441 samples.
#[test]
fn stage_times_follow_the_sample_rate() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, 44_100);
    let mut e = EnvA::new();
    e.note_on();
    close(e.stage_samples(&c) as f64, 0.010_003, 44_100, "attack at 44.1 kHz");
}

#[test]
fn hold_positions() {
    let mut p = params(EnvSpeed::Med);
    p.hold = 0.8; // about 0.40 s
    let hold = (speed_ranges(EnvSpeed::Med).hold.at(0.8) * SR as f32) as u32;
    let stages = |p: &EnvParams, key_for: usize| {
        let c = ACoefs::new(p, 0.0, SR);
        let mut e = EnvA::new();
        e.note_on();
        (0..2 * SR as usize)
            .map(|i| {
                e.tick(&c, i < key_for);
                e.stage()
            })
            .collect::<Vec<_>>()
    };
    // OFF: no hold stage, whatever H.
    p.hold_pos = HoldPos::Off;
    assert!(!stages(&p, 100_000).contains(&Stage::Hold));
    // AHDSR: holds 1 for H after the attack. The tick that ends the attack
    // already reports Hold (at level 1, the attack's last sample), then H's
    // `hold` samples follow.
    p.hold_pos = HoldPos::Ahdsr;
    let s = stages(&p, 100_000);
    assert_eq!(s.iter().filter(|&&x| x == Stage::Hold).count() as u32, hold + 1);
    // GATE EXT: a one-sample gate still plays attack and decay, and
    // releases H after the note-on.
    p.hold_pos = HoldPos::GateExt;
    let s = stages(&p, 1);
    assert!(s.contains(&Stage::Decay) && !s.contains(&Stage::Hold));
    let release = s.iter().position(|&x| x == Stage::Release).unwrap() as u32;
    assert!((release as i64 - hold as i64).abs() <= 1, "{release} vs {hold}");
}

/// The per-sample step is one multiply-add toward the target: attack
/// levels form a geometric series about 1.3.
#[test]
fn tick_is_a_geometric_step() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    let l: Vec<f32> = (0..4).map(|_| e.tick(&c, true)).collect();
    let r1 = (l[2] - 1.3) / (l[1] - 1.3);
    let r2 = (l[3] - 1.3) / (l[2] - 1.3);
    assert!((r1 - r2).abs() < 1e-4, "{r1} {r2}");
}

/// Envelope A from the spec's formulas in f64: the reference both f32
/// paths are held to (ADR 0036). Same stages, gate and settling as `EnvA`.
struct RefA {
    stage: Stage,
    level: f64,
    hold_left: u64,
    since_on: u64,
    ca: f64,
    cd: f64,
    cr: f64,
    sus: f64,
    hold: u64,
    hold_pos: HoldPos,
}

impl RefA {
    fn new(p: &EnvParams, sr: u32) -> Self {
        let fs = sr as f64;
        // The manual's ranges in seconds; a position p is min·(max/min)^p.
        let (h, a, dr) = match p.speed {
            EnvSpeed::Fast => ((1e-6, 2.5), (2e-4, 1.5), (6e-4, 2.5)),
            EnvSpeed::Med => ((1e-6, 10.0), (2e-3, 10.0), (3.5e-3, 10.0)),
            EnvSpeed::Slow => ((1e-6, 60.0), (9.3e-3, 60.0), (3e-2, 60.0)),
        };
        let at = |(lo, hi): (f64, f64), x: f32| lo * (hi / lo).powf(x as f64);
        // c = 1 − e^(−1/(τ·fs)), τ = time / ln(the stage's overshoot ratio).
        let c = |secs: f64, ln: f64| 1.0 - (-ln / (secs * fs)).exp();
        Self {
            stage: Stage::Idle,
            level: 0.0,
            hold_left: 0,
            since_on: u64::MAX,
            ca: c(at(a, p.attack), (1.3f64 / 0.3).ln()),
            cd: c(at(dr, p.decay), 101f64.ln()),
            cr: c(at(dr, p.release), 101f64.ln()),
            sus: p.sustain as f64,
            hold: (at(h, p.hold) * fs) as u64,
            hold_pos: p.hold_pos,
        }
    }

    fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.since_on = 0;
    }

    fn tick(&mut self, key: bool) -> f64 {
        let gate = key || (self.hold_pos == HoldPos::GateExt && self.since_on < self.hold);
        self.since_on = self.since_on.saturating_add(1);
        let running = matches!(
            self.stage,
            Stage::Attack | Stage::Hold | Stage::Decay | Stage::Sustain
        );
        if running && !gate {
            self.stage = Stage::Release;
        } else if self.stage == Stage::Hold && self.hold_left == 0 {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Sustain && self.level > self.sus {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Decay && self.level <= self.sus {
            self.stage = Stage::Sustain;
        }
        let (c, t, end, rising) = match self.stage {
            Stage::Hold => {
                self.hold_left -= 1;
                return self.level;
            }
            Stage::Attack => (self.ca, 1.3, 1.0, true),
            Stage::Decay => (self.cd, self.sus - 0.01, self.sus, false),
            Stage::Release => (self.cr, -0.01, 0.0, false),
            _ => return self.level,
        };
        let l = self.level + c * (t - self.level);
        if (rising && l >= end) || (!rising && l <= end) {
            self.level = end;
            self.stage = match self.stage {
                Stage::Attack if self.hold_pos == HoldPos::Ahdsr && self.hold > 0 => {
                    self.hold_left = self.hold;
                    Stage::Hold
                }
                Stage::Attack => Stage::Decay,
                Stage::Decay => Stage::Sustain,
                _ => Stage::Idle,
            };
        } else {
            self.level = l;
        }
        self.level
    }
}

/// `level` (and `stage`, if given) at sample `n` match the reference at
/// n − 1, n or n + 1 within 1e-4: ADR 0036's tolerance, which supersedes
/// the spec's 1e-6 (f32 cannot hold it).
fn near(refs: &[(Stage, f64)], n: usize, stage: Option<Stage>, level: f32) -> bool {
    (n.saturating_sub(1)..=(n + 1).min(refs.len() - 1)).any(|m| {
        stage.is_none_or(|s| s == refs[m].0) && (refs[m].1 - level as f64).abs() <= 1e-4
    })
}

/// The per-sample paths (`tick`, `fill`) and the per-block path
/// (`advance`) each match the f64 reference across every stage boundary.
#[test]
fn each_path_matches_an_f64_reference() {
    // (…, blocks the key is held)
    for (speed, hold_pos, hold, a, d, s, r, held) in [
        (EnvSpeed::Fast, HoldPos::Off, 0.0, 0.0, 0.3, 0.5, 0.3, 150), // attack ends mid-block
        (EnvSpeed::Med, HoldPos::Ahdsr, 0.55, 0.19, 0.4, 0.2, 0.4, 150), // a hold stage
        // GATE EXT: the key is up after one block, well inside H (760
        // samples), so the extended gate decides where the release starts,
        // mid-block.
        (EnvSpeed::Med, HoldPos::GateExt, 0.6, 0.1, 0.2, 0.6, 0.3, 1),
        (EnvSpeed::Fast, HoldPos::Ahdsr, 0.0, 0.05, 0.05, 0.0, 0.05, 150), // decay to 0, release to idle
    ] {
        let p = EnvParams {
            speed,
            hold_pos,
            hold,
            attack: a,
            decay: d,
            sustain: s,
            release: r,
            ..EnvParams::default()
        };
        const BLOCKS: usize = 300;
        let key = |b: usize| b < held;
        let mut reference = RefA::new(&p, SR);
        reference.note_on();
        let refs: Vec<(Stage, f64)> = (0..BLOCKS * BLOCK_SIZE)
            .map(|n| {
                let l = reference.tick(key(n / BLOCK_SIZE));
                (reference.stage, l)
            })
            .collect();
        let c = ACoefs::new(&p, 0.0, SR);
        let (mut ticked, mut filled, mut blocked) = (EnvA::new(), EnvA::new(), EnvA::new());
        for e in [&mut ticked, &mut filled, &mut blocked] {
            e.note_on();
        }
        for b in 0..BLOCKS {
            let mut buf = [0.0f32; BLOCK_SIZE];
            filled.fill(&c, key(b), &mut buf);
            blocked.advance(&c, key(b), BLOCK_SIZE as u32);
            for (i, &f) in buf.iter().enumerate() {
                let n = b * BLOCK_SIZE + i;
                let t = ticked.tick(&c, key(b));
                assert!(near(&refs, n, Some(ticked.stage()), t), "tick {speed:?} {hold_pos:?} {n}: {t} vs {:?}", refs[n]);
                assert!(near(&refs, n, None, f), "fill {speed:?} {hold_pos:?} {n}: {f} vs {:?}", refs[n]);
            }
            let n = (b + 1) * BLOCK_SIZE - 1;
            assert!(
                near(&refs, n, Some(blocked.stage()), blocked.level()),
                "advance {speed:?} {hold_pos:?} block {b}: {} vs {:?}",
                blocked.level(),
                refs[n]
            );
            assert!(near(&refs, n, Some(filled.stage()), filled.level()), "fill's stage, block {b}");
        }
    }
}

/// A raised S doesn't lift a decaying level: it sustains where it is.
#[test]
fn raising_s_mid_decay_holds_the_level() {
    let mut p = params(EnvSpeed::Med);
    p.sustain = 0.2;
    let c = ACoefs::new(&p, 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    while e.stage() != Stage::Decay {
        e.tick(&c, true);
    }
    for _ in 0..2000 {
        e.tick(&c, true); // decaying toward 0.2
    }
    let before = e.level();
    p.sustain = 0.9;
    let c = ACoefs::new(&p, 0.0, SR);
    let after = e.tick(&c, true);
    assert_eq!(e.stage(), Stage::Sustain);
    assert_eq!(after, before);
}

/// A release during the attack and a note-on during the release leave the
/// level where it is (RETRIG from the current level).
#[test]
fn release_and_retrigger_do_not_move_the_level() {
    let c = ACoefs::new(&params(EnvSpeed::Med), 0.0, SR);
    let mut e = EnvA::new();
    e.note_on();
    for _ in 0..200 {
        e.tick(&c, true);
    }
    let before = e.level();
    let after = e.tick(&c, false);
    assert_eq!(e.stage(), Stage::Release);
    assert!((after - before).abs() < 0.01, "{before} → {after}");
    for _ in 0..2000 {
        e.tick(&c, false);
    }
    let before = e.level();
    e.note_on();
    let after = e.tick(&c, true);
    assert_eq!(e.stage(), Stage::Attack);
    assert!((after - before).abs() < 0.01, "{before} → {after}");
}

/// LEVEL: unrouted the peak is 1; routed it is `clamp(Σ, 0, 1)`.
#[test]
fn level_sets_the_peak() {
    let p = EnvParams::default();
    for (level, want) in [(None, 1.0f32), (Some(0.25), 0.25)] {
        let m = EnvMods { level, ..EnvMods::NONE };
        let mut e = Envelope::new();
        e.note_on(&p);
        let mut peak = 0.0f32;
        for _ in 0..40 {
            peak = peak.max(e.run_block(&p, &m, true, SR, None));
        }
        // Block starts sample the peak within 0.01 (the decay has begun).
        assert!((peak - want).abs() < 0.01, "{level:?}: {peak}");
    }
}

/// On the VCA path a LEVEL change ramps across the block instead of
/// stepping (no zipper from an LFO → LEVEL route).
#[test]
fn a_level_change_ramps_on_the_vca_path() {
    let p = EnvParams { sustain: 0.7, ..EnvParams::default() };
    let full = EnvMods { level: Some(1.0), ..EnvMods::NONE };
    let half = EnvMods { level: Some(0.5), ..EnvMods::NONE };
    let mut e = Envelope::new();
    e.note_on(&p);
    for _ in 0..1000 {
        e.run_block(&p, &full, true, SR, None); // well into sustain at 0.7
    }
    let mut g = [0.0f32; BLOCK_SIZE];
    e.run_block(&p, &full, true, SR, Some((&mut g, 1.0)));
    let mut g = [0.0f32; BLOCK_SIZE];
    e.run_block(&p, &half, true, SR, Some((&mut g, 1.0)));
    let step = 0.7 * 0.5 / BLOCK_SIZE as f32;
    assert!((g[BLOCK_SIZE - 1] - 0.35).abs() < 1e-4, "{}", g[BLOCK_SIZE - 1]);
    assert!((g[0] - (0.7 - step)).abs() < 1e-4, "{}", g[0]);
    assert!(g.windows(2).all(|w| (w[0] - w[1] - step).abs() < 1e-5), "an even ramp");
}

/// TIME +100 % makes every stage 32 × shorter; −100 % 32 × longer.
#[test]
fn time_scales_every_stage() {
    let p = params(EnvSpeed::Med);
    let samples = |time: f32| {
        let c = ACoefs::new(&p, time, SR);
        let mut e = EnvA::new();
        e.note_on();
        e.stage_samples(&c) as f64
    };
    let base = samples(0.0);
    assert!((samples(1.0) - base / 32.0).abs() <= 1.0);
    assert!((samples(-1.0) - base * 32.0).abs() <= 32.0);
}

/// Review Focus 1: a TIME sum far past ±1 clamps, and stays finite.
#[test]
fn time_route_sums_clamp() {
    let p = params(EnvSpeed::Med);
    let mut e = EnvA::new();
    e.note_on();
    for (sum, like) in [(8.0f32, 1.0f32), (-8.0, -1.0), (1e9, 1.0)] {
        let (c, d) = (ACoefs::new(&p, sum, SR), ACoefs::new(&p, like, SR));
        assert_eq!(e.stage_samples(&c), e.stage_samples(&d), "{sum}");
    }
    let c = ACoefs::new(&p, f32::NAN, SR);
    e.advance(&c, true, 64);
    assert!(e.level().is_finite());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test env_a_test`
Expected: FAIL to compile: no `dsp::modulator`.

- [ ] **Step 3: The modulator module's types**

Create `chimera-core/src/dsp/modulator/mod.rs`:

```rust
//! The per-voice modulators' pure parts (filter-routing spec § 1): the
//! slider laws, Envelope A and Envelope B. `Envelope` and `Lfo` wrap them.

pub mod env_a;
pub mod law;

/// The choice at `v` among `all` (a `Block` write), clamped.
pub fn pick<T: Copy>(all: &[T], v: f32) -> T {
    all[(v.max(0.0) as usize).min(all.len() - 1)]
}

/// An ENV slot of the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvSlot {
    Env1,
    Env2,
    Env3,
}

impl EnvSlot {
    pub const ALL: [EnvSlot; 3] = [EnvSlot::Env1, EnvSlot::Env2, EnvSlot::Env3];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// An ENV slot's TYPE: the Cascadia's Envelope A or Envelope B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvType {
    A = 0,
    B = 1,
}

impl EnvType {
    pub const ALL: [EnvType; 2] = [EnvType::A, EnvType::B];
}

/// Envelope A's SPEED: the manual's time ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvSpeed {
    Fast = 0,
    Med = 1,
    Slow = 2,
}

impl EnvSpeed {
    pub const ALL: [EnvSpeed; 3] = [EnvSpeed::Fast, EnvSpeed::Med, EnvSpeed::Slow];
}

/// Envelope A's HOLD POSITION.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HoldPos {
    Off = 0,
    Ahdsr = 1,
    GateExt = 2,
}

impl HoldPos {
    pub const ALL: [HoldPos; 3] = [HoldPos::Off, HoldPos::Ahdsr, HoldPos::GateExt];
}
```

In `chimera-core/src/dsp/mod.rs`, add `pub mod modulator;` to the module list.

- [ ] **Step 4: The laws**

Create `chimera-core/src/dsp/modulator/law.rs`:

```rust
//! Slider laws (spec § 1 "Sliders are positions") and the RC constants.

use crate::dsp::fast_exp2;
use crate::dsp::modulator::EnvSpeed;

/// An exponential slider: `q = min · (max/min)^p = min · 2^(p·oct)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub min: f32,
    /// `log2(max / min)`.
    pub oct: f32,
}

impl Range {
    /// The quantity at position `p` (clamped to 0..1): one `fast_exp2`.
    pub fn at(self, p: f32) -> f32 {
        self.min * fast_exp2(p.max(0.0).min(1.0) * self.oct)
    }
}

/// Envelope A's ranges at one SPEED, seconds: the manual's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedRanges {
    pub hold: Range,
    pub attack: Range,
    pub dec_rel: Range,
}

pub const fn speed_ranges(s: EnvSpeed) -> SpeedRanges {
    const fn r(min: f32, oct: f32) -> Range {
        Range { min, oct }
    }
    match s {
        // H 0.001 ms – 2.5 s, A 0.2 ms – 1.5 s, D and R 0.6 ms – 2.5 s.
        EnvSpeed::Fast => SpeedRanges {
            hold: r(1e-6, 21.253_497),
            attack: r(2e-4, 12.872_675),
            dec_rel: r(6e-4, 12.024_678),
        },
        // H 0.001 ms – 10 s, A 2 ms – 10 s, D and R 3.5 ms – 10 s.
        EnvSpeed::Med => SpeedRanges {
            hold: r(1e-6, 23.253_497),
            attack: r(2e-3, 12.287_712),
            dec_rel: r(3.5e-3, 11.480_357),
        },
        // H 0.001 ms – 60 s, A 9.3 ms – 60 s, D and R 30 ms – 60 s.
        EnvSpeed::Slow => SpeedRanges {
            hold: r(1e-6, 25.838_459),
            attack: r(9.3e-3, 12.655_444),
            dec_rel: r(3e-2, 10.965_784),
        },
    }
}

/// `log2` of an RC stage's per-sample retention, `−1 / (τ·fs·ln 2)`.
pub fn rc_k(tau: f32, fs: f32) -> f32 {
    -1.0 / (tau * fs * core::f32::consts::LN_2)
}

/// The per-sample step toward the target, `c = 1 − 2^k` (spec `rc_coeff`).
pub fn rc_coeff(k: f32) -> f32 {
    1.0 - fast_exp2(k)
}
```

- [ ] **Step 5: Envelope A**

Create `chimera-core/src/dsp/modulator/env_a.rs`:

```rust
//! Envelope A (spec § Envelope A): an AHDSR with the Cascadia's HOLD
//! POSITION and SPEED ranges and one fixed RC shape: each stage approaches
//! a target past its end, as a capacitor charging toward a rail.

use core::f32::consts::LN_2;

use crate::dsp::modulator::HoldPos;
use crate::dsp::modulator::law::{rc_coeff, rc_k, speed_ranges};
use crate::dsp::{fast_exp2, fast_log2};
use crate::params::EnvParams;

/// Attack aims at 1.3 and stops at 1: τ = A / ln(1.3 / 0.3).
const ATTACK_TARGET: f32 = 1.3;
const ATTACK_LN: f32 = 1.466_337;
/// Decay and release aim 0.01 past their end: τ = time / ln(1.01 / 0.01).
const OVERSHOOT: f32 = 0.01;
const DEC_REL_LN: f32 = 4.615_12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
}

/// One RC stage for a block: `k`, the `log2` of the per-sample retention,
/// and `c = 1 − 2^k`, the per-sample step.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rc {
    k: f32,
    c: f32,
}

impl Rc {
    fn new(secs: f32, ln: f32, fs: f32) -> Self {
        let k = rc_k(secs / ln, fs);
        Self { k, c: rc_coeff(k) }
    }
}

/// A's constants for one block: SPEED, the slider laws and TIME resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ACoefs {
    attack: Rc,
    decay: Rc,
    release: Rc,
    sus: f32,
    /// H in samples: AHDSR's hold stage, GATE EXT's gate.
    hold: u32,
    hold_pos: HoldPos,
}

impl ACoefs {
    /// `time`: TIME's route sum. Every stage × 2^(−5·Σ), Σ clamped to ±1
    /// (NaN reads as −1).
    pub fn new(p: &EnvParams, time: f32, sample_rate: u32) -> Self {
        let fs = sample_rate as f32;
        let r = speed_ranges(p.speed);
        let scale = fast_exp2(-5.0 * time.max(-1.0).min(1.0));
        Self {
            attack: Rc::new(r.attack.at(p.attack) * scale, ATTACK_LN, fs),
            decay: Rc::new(r.dec_rel.at(p.decay) * scale, DEC_REL_LN, fs),
            release: Rc::new(r.dec_rel.at(p.release) * scale, DEC_REL_LN, fs),
            sus: p.sustain.max(0.0).min(1.0),
            hold: (r.hold.at(p.hold) * scale * fs) as u32,
            hold_pos: p.hold_pos,
        }
    }
}

/// Envelope A's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvA {
    stage: Stage,
    level: f32,
    /// Samples left in Hold.
    hold_left: u32,
    /// Samples since the note-on, saturating (GATE EXT's gate).
    since_on: u32,
}

impl Default for EnvA {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvA {
    pub const fn new() -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            hold_left: 0,
            since_on: u32::MAX,
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// 0..1.
    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn rising(&self) -> bool {
        self.stage == Stage::Attack
    }

    /// Attack from the current level (the Cascadia's RETRIG).
    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.since_on = 0;
    }

    /// Take over at `level` after a TYPE change (spec § 1): the level stays.
    /// `sus` is S, 0..1.
    pub fn enter(&mut self, level: f32, rising: bool, key: bool, sus: f32) {
        self.level = level.max(0.0).min(1.0);
        self.since_on = u32::MAX;
        self.stage = match (key, rising) {
            (false, _) if self.level > 0.0 => Stage::Release,
            (false, _) => Stage::Idle,
            (true, true) => Stage::Attack,
            (true, false) if self.level > sus => Stage::Decay,
            (true, false) => Stage::Sustain,
        };
    }

    fn gate(&self, c: &ACoefs, key: bool) -> bool {
        key || (c.hold_pos == HoldPos::GateExt && self.since_on < c.hold)
    }

    /// The stage changes that take no time: a fallen gate, a finished
    /// hold, a sustain above a lowered S, a decay under a raised S.
    fn settle(&mut self, c: &ACoefs, gate: bool) {
        let running = matches!(
            self.stage,
            Stage::Attack | Stage::Hold | Stage::Decay | Stage::Sustain
        );
        if running && !gate {
            self.stage = Stage::Release;
        } else if self.stage == Stage::Hold && self.hold_left == 0 {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Sustain && self.level > c.sus {
            self.stage = Stage::Decay;
        } else if self.stage == Stage::Decay && self.level <= c.sus {
            // A raised S doesn't lift a decaying level: sustain where it is.
            self.stage = Stage::Sustain;
        }
    }

    /// The running RC stage: its constants, target, end and direction.
    fn rc(&self, c: &ACoefs) -> Option<(Rc, f32, f32, bool)> {
        match self.stage {
            Stage::Attack => Some((c.attack, ATTACK_TARGET, 1.0, true)),
            Stage::Decay => Some((c.decay, c.sus - OVERSHOOT, c.sus, false)),
            Stage::Release => Some((c.release, -OVERSHOOT, 0.0, false)),
            _ => None,
        }
    }

    /// An RC stage reached its end.
    fn finish(&mut self, c: &ACoefs, end: f32) {
        self.level = end;
        self.stage = match self.stage {
            Stage::Attack if c.hold_pos == HoldPos::Ahdsr && c.hold > 0 => {
                self.hold_left = c.hold;
                Stage::Hold
            }
            Stage::Attack => Stage::Decay,
            Stage::Decay => Stage::Sustain,
            _ => Stage::Idle,
        };
    }

    /// One sample: one multiply-add and a compare, no divide. Returns the
    /// level after it.
    pub fn tick(&mut self, c: &ACoefs, key: bool) -> f32 {
        let gate = self.gate(c, key);
        self.since_on = self.since_on.saturating_add(1);
        self.settle(c, gate);
        if self.stage == Stage::Hold {
            self.hold_left -= 1;
        } else if let Some((rc, t, end, rising)) = self.rc(c) {
            let l = self.level + rc.c * (t - self.level);
            if (rising && l >= end) || (!rising && l <= end) {
                self.finish(c, end);
            } else {
                self.level = l;
            }
        }
        self.level
    }

    /// A block of per-sample levels, as `tick` would give them (the same
    /// arithmetic), for the VCA: one tight loop per stretch of a stage, the
    /// gate and settling checked once per stretch, not per sample.
    pub fn fill(&mut self, c: &ACoefs, key: bool, out: &mut [f32]) {
        let mut i = 0;
        while i < out.len() {
            let gate = self.gate(c, key);
            self.settle(c, gate);
            let left = (out.len() - i) as u32;
            // GATE EXT's own gate falls inside this stretch: stop there.
            let span = (if gate && !key { left.min(c.hold - self.since_on) } else { left }) as usize;
            let used = match self.rc(c) {
                None => {
                    let m = if self.stage == Stage::Hold {
                        let m = span.min(self.hold_left as usize);
                        self.hold_left -= m as u32;
                        m
                    } else {
                        span
                    };
                    out[i..i + m].fill(self.level);
                    m
                }
                Some((rc, t, end, rising)) => {
                    let mut l = self.level;
                    let mut m = 0;
                    let mut ended = false;
                    while m < span {
                        l += rc.c * (t - l);
                        m += 1;
                        if (rising && l >= end) || (!rising && l <= end) {
                            ended = true;
                            break;
                        }
                        out[i + m - 1] = l;
                    }
                    if ended {
                        self.finish(c, end);
                        out[i + m - 1] = end;
                    } else {
                        self.level = l;
                    }
                    m
                }
            };
            self.since_on = self.since_on.saturating_add(used as u32);
            i += used;
        }
    }

    /// `n` samples in closed form, `L_n = T + (L − T)·2^(k·n)`, stage ends
    /// placed inside the block (within ±1 sample of the ticks, ADR 0036).
    pub fn advance(&mut self, c: &ACoefs, key: bool, mut n: u32) {
        while n > 0 {
            let gate = self.gate(c, key);
            self.settle(c, gate);
            // GATE EXT's own gate falls inside this stretch: stop there.
            let span = if gate && !key {
                n.min(c.hold - self.since_on)
            } else {
                n
            };
            let used = self.run(c, span);
            self.since_on = self.since_on.saturating_add(used);
            n -= used;
        }
    }

    /// Up to `span` samples of the running stage; returns how many it took.
    fn run(&mut self, c: &ACoefs, span: u32) -> u32 {
        match self.stage {
            Stage::Idle | Stage::Sustain => span,
            Stage::Hold => {
                let m = span.min(self.hold_left);
                self.hold_left -= m;
                m
            }
            _ => {
                let Some((rc, t, end, _)) = self.rc(c) else {
                    return span;
                };
                let m = steps_to(self.level, t, end, rc.k);
                if m <= span {
                    self.finish(c, end);
                    m
                } else {
                    self.level = t + (self.level - t) * fast_exp2(rc.k * span as f32);
                    span
                }
            }
        }
    }

    /// Samples until the running RC stage ends (`u32::MAX` in Idle, Hold
    /// and Sustain).
    pub fn stage_samples(&self, c: &ACoefs) -> u32 {
        self.rc(c)
            .map_or(u32::MAX, |(rc, t, end, _)| steps_to(self.level, t, end, rc.k))
    }
}

/// Ticks until an RC stage from `l` toward `t` reaches `end` (at least 1;
/// `u32::MAX` if it never does): `fast_log2` places it and one Newton step
/// on `2^(k·m)` sharpens it to `exp2`'s precision.
fn steps_to(l: f32, t: f32, end: f32, k: f32) -> u32 {
    let ratio = (end - t) / (l - t);
    if ratio >= 1.0 {
        return 1;
    }
    if !(ratio > 0.0) || k >= 0.0 {
        return u32::MAX;
    }
    let m = fast_log2(ratio) / k;
    let e = fast_exp2(k * m);
    let m = m - (e - ratio) / (e * k * LN_2);
    if m >= u32::MAX as f32 {
        return u32::MAX;
    }
    let i = m as u32;
    if (i as f32) < m { i + 1 } else { i.max(1) }
}
```

- [ ] **Step 6: `EnvParams` and its address**

In `chimera-core/src/params.rs`, replace `EnvParams`, its `Default`, its `impl`, `ENV_SPECS` and its `Block` impl with:

```rust
use crate::dsp::modulator::{EnvSpeed, EnvType, HoldPos, pick};

/// One ENV slot's parameters (spec § Data model).
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    /// A, D, R and H: positions 0..1 on SPEED's exponential ranges.
    pub attack: f32,
    pub decay: f32,
    pub release: f32,
    pub hold: f32,
    /// S: a level, 0..1.
    pub sustain: f32,
    /// LEVEL destination's stored value; the peak comes from its routes.
    pub level: f32,
    /// Unread and off the pages (#112).
    pub vel_sens: f32,
    pub env_type: EnvType,
    pub speed: EnvSpeed,
    pub hold_pos: HoldPos,
    /// TIME destination's stored 0.
    pub time: f32,
}

impl Default for EnvParams {
    /// Today's times at MED: A 10 ms, D and R 300 ms, S 0.7; H 0.001 ms.
    fn default() -> Self {
        Self {
            attack: 0.189,
            decay: 0.559,
            release: 0.559,
            hold: 0.0,
            sustain: 0.7,
            level: 1.0,
            vel_sens: 0.5,
            env_type: EnvType::A,
            speed: EnvSpeed::Med,
            hold_pos: HoldPos::Ahdsr,
            time: 0.0,
        }
    }
}

impl EnvParams {
    pub const ATTACK: ParamId = ParamId(0);
    pub const DECAY: ParamId = ParamId(1);
    pub const SUSTAIN: ParamId = ParamId(2);
    pub const RELEASE: ParamId = ParamId(3);
    pub const LEVEL: ParamId = ParamId(4);
    pub const VEL_SENS: ParamId = ParamId(5);
    pub const HOLD: ParamId = ParamId(6);
    pub const TYPE: ParamId = ParamId(7);
    pub const SPEED: ParamId = ParamId(8);
    pub const HOLD_POS: ParamId = ParamId(9);
    pub const TIME: ParamId = ParamId(10);
}

/// Positions and levels, per block. LEVEL and TIME are hidden destinations
/// (Task 9 of the filter-routing plan makes them modulatable).
pub static ENV_SPECS: [ParamSpec; 11] = [
    ParamSpec::continuous(0, "ATK", ValFmt::Uni, 0.0, 1.0, 0.189, 1.0 / 128.0, false),
    ParamSpec::continuous(1, "DEC", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "SUS", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "REL", ValFmt::Uni, 0.0, 1.0, 0.559, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "LEVEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "VEL", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "H", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::choice(7, "TYPE", ValFmt::Names(&["A", "B"]), 1.0, 0.0),
    ParamSpec::choice(8, "SPEED", ValFmt::Names(&["FAST", "MED", "SLOW"]), 2.0, 1.0),
    ParamSpec::choice(9, "HOLD", ValFmt::Names(&["OFF", "AHDSR", "GATE EXT"]), 2.0, 1.0),
    ParamSpec::continuous(10, "TIME", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
];

impl Block for EnvParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ENV_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ATTACK => self.attack,
            Self::DECAY => self.decay,
            Self::SUSTAIN => self.sustain,
            Self::RELEASE => self.release,
            Self::LEVEL => self.level,
            Self::VEL_SENS => self.vel_sens,
            Self::HOLD => self.hold,
            Self::TYPE => self.env_type as u8 as f32,
            Self::SPEED => self.speed as u8 as f32,
            Self::HOLD_POS => self.hold_pos as u8 as f32,
            Self::TIME => self.time,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ATTACK => self.attack = v,
            Self::DECAY => self.decay = v,
            Self::SUSTAIN => self.sustain = v,
            Self::RELEASE => self.release = v,
            Self::LEVEL => self.level = v,
            Self::VEL_SENS => self.vel_sens = v,
            Self::HOLD => self.hold = v,
            Self::TYPE => self.env_type = pick(&EnvType::ALL, v),
            Self::SPEED => self.speed = pick(&EnvSpeed::ALL, v),
            Self::HOLD_POS => self.hold_pos = pick(&HoldPos::ALL, v),
            Self::TIME => self.time = v,
            _ => {}
        }
    }
}
```

In `ParamSnapshot`'s `block` and `block_mut`, the three envelope arms become `BlockRef::Env(s) => &self.envelopes[s.index()],` (and `&mut`).

In `chimera-core/src/addr.rs`: replace the variants `AmpEnv`, `FilterEnv` and `AuxEnv` (and their doc lines) with

```rust
    /// ENV slot `n`: `envelopes[n]`.
    Env(crate::dsp::modulator::EnvSlot),
```

in `ALL` replace the three with `BlockRef::Env(EnvSlot::Env1), BlockRef::Env(EnvSlot::Env2), BlockRef::Env(EnvSlot::Env3),` (still 22; `use crate::dsp::modulator::EnvSlot;`). In `specs`: `BlockRef::Env(_) => &crate::params::ENV_SPECS,`. In `voice_reads`, `BlockRef::AmpEnv | BlockRef::FilterEnv | BlockRef::AuxEnv` becomes `BlockRef::Env(_)` (still `false` until Task 9); rewrite the doc sentence about the amp envelope as `ENV slots feed the matrix and are not read as destinations until their LEVEL, TIME, RISE, FALL and SHAPE open (filter-routing Task 9)`.

In `chimera-core/src/preset.rs`, both `PartEdit` match lists: replace `| BlockRef::AmpEnv | BlockRef::FilterEnv | BlockRef::AuxEnv` with `| BlockRef::Env(_)`.

In `chimera-core/src/ui/mod_grid.rs`, `block_tag`: replace the three arms with `BlockRef::Env(s) => ["E1", "E2", "E3"][s.index()],`.

In `chimera-core/src/modulation.rs`, `impl ModSource`:

```rust
    /// The source ENV slot `s` feeds.
    pub const fn of_env(s: crate::dsp::modulator::EnvSlot) -> Self {
        [ModSource::Env1, ModSource::Env2, ModSource::Env3][s.index()]
    }
```

- [ ] **Step 7: The ENV slot**

Replace `chimera-core/src/dsp/envelope.rs` with:

```rust
//! An ENV slot of the modulator pool (spec § 1): runs its TYPE from
//! `EnvParams` and the matrix's inputs, once per block or per sample.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::modulator::env_a::{ACoefs, EnvA};
use crate::params::EnvParams;

/// What the matrix feeds an ENV slot, from the previous block (spec
/// § Signal flow 1), so a slot never waits on the matrix it feeds. Task 6
/// adds RISE, FALL and SHAPE as `slides`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvMods {
    /// The peak with a route into LEVEL, `clamp(Σ, 0, 1)`; `None` without one (peak 1).
    pub level: Option<f32>,
    /// TIME's Σ.
    pub time: f32,
}

impl EnvMods {
    pub const NONE: Self = Self {
        level: None,
        time: 0.0,
    };
}

/// What an A slot's coefficients were built from: equal inputs reuse them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AKey {
    stages: [f32; 5], // attack, decay, sustain, release, hold
    speed: crate::dsp::modulator::EnvSpeed,
    hold_pos: crate::dsp::modulator::HoldPos,
    time: f32,
    sample_rate: u32,
}

impl AKey {
    fn of(p: &EnvParams, time: f32, sample_rate: u32) -> Self {
        Self {
            stages: [p.attack, p.decay, p.sustain, p.release, p.hold],
            speed: p.speed,
            hold_pos: p.hold_pos,
            time,
            sample_rate,
        }
    }
}

/// `gain[n] += amount · level[n] · peak[n]`, the peak ramped from `from` to
/// `to` across the block, so a route into LEVEL doesn't zipper the VCA.
fn add_ramped(gain: &mut [f32; BLOCK_SIZE], level: &[f32; BLOCK_SIZE], amount: f32, from: f32, to: f32) {
    if from == to {
        let a = amount * to;
        for (g, l) in gain.iter_mut().zip(level) {
            *g += a * l;
        }
    } else {
        let step = (to - from) / BLOCK_SIZE as f32;
        for (n, (g, l)) in gain.iter_mut().zip(level).enumerate() {
            *g += amount * l * (from + step * (n + 1) as f32);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    a: EnvA,
    /// The last A coefficients and their inputs.
    ac: Option<(AKey, ACoefs)>,
    /// This block's peak (LEVEL) and the last block's.
    peak: f32,
    prev_peak: f32,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub const fn new() -> Self {
        Self {
            a: EnvA::new(),
            ac: None,
            peak: 1.0,
            prev_peak: 1.0,
        }
    }

    /// This block's A coefficients, rebuilt only when an input changed.
    fn a_coefs(&mut self, p: &EnvParams, time: f32, sample_rate: u32) -> ACoefs {
        let key = AKey::of(p, time, sample_rate);
        match self.ac {
            Some((k, c)) if k == key => c,
            _ => {
                let c = ACoefs::new(p, time, sample_rate);
                self.ac = Some((key, c));
                c
            }
        }
    }

    pub fn note_on(&mut self, _p: &EnvParams) {
        self.a.note_on();
    }

    /// The raw contour, 0..1: no velocity (spec § 1).
    pub fn output(&self) -> f32 {
        self.a.level() * self.peak
    }

    pub fn is_idle(&self) -> bool {
        self.a.is_idle()
    }

    /// One block. Returns the output at the block's start. With `vca`, the
    /// slot fills a block of levels and adds `amount · level · peak` into
    /// the buffer (the VCA's sum), the peak ramped per sample; otherwise it
    /// advances in closed form.
    pub fn run_block(
        &mut self,
        p: &EnvParams,
        m: &EnvMods,
        key: bool,
        sample_rate: u32,
        vca: Option<(&mut [f32; BLOCK_SIZE], f32)>,
    ) -> f32 {
        let c = self.a_coefs(p, m.time, sample_rate);
        self.prev_peak = self.peak;
        self.peak = m.level.unwrap_or(1.0);
        let start = self.output();
        match vca {
            Some((gain, amount)) => {
                let mut level = [0.0f32; BLOCK_SIZE];
                self.a.fill(&c, key, &mut level);
                add_ramped(gain, &level, amount, self.prev_peak, self.peak);
            }
            None => self.a.advance(&c, key, BLOCK_SIZE as u32),
        }
        start
    }
}
```

- [ ] **Step 8: `Voice` runs the three slots**

In `chimera-core/src/dsp/voice.rs`:
- import `use crate::dsp::envelope::{EnvMods, Envelope};` and `use crate::dsp::modulator::EnvSlot;`;
- the field `amp_env: Envelope,` becomes `envs: [Envelope; 3],`, and in `init_chain` `amp_env: Envelope::new(),` becomes `envs: [Envelope::new(); 3],`;
- `trigger`: replace `self.amp_env.note_on(velocity.unit());` with

```rust
        for (e, p) in self.envs.iter_mut().zip(&params.envelopes) {
            e.note_on(p);
        }
```

- `note_off`: delete `self.amp_env.note_off();` (the key is `held`, read every block);
- in `render`, replace from `// A fading voice keeps the settings it last played.` through the end of the `mod_values` lines with:

```rust
        // The modulators run every block, fading or not, from the settings
        // the voice plays (spec § Signal flow 1).
        let src = if self.fade == 0 { params } else { &self.played };
        let key = self.held;
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        for (s, env) in EnvSlot::ALL.iter().zip(self.envs.iter_mut()) {
            mod_values[ModSource::of_env(*s).index()] =
                env.run_block(&src.envelopes[s.index()], &EnvMods::NONE, key, sample_rate, None);
        }
        mod_values[ModSource::Lfo1.index()] = self.lfo.process(&src.lfo, sample_rate);
        mod_values[ModSource::Vel.index()] = self.last_velocity.unit();
        mod_values[ModSource::Note.index()] = note_source(self.last_note);

        // A fading voice keeps the settings it last played.
        if self.fade == 0 {
```

  and keep the rest of the old `if self.fade == 0 { … }` body from `// Every routed destination gets its offset` on.
- The output stage becomes:

```rust
        // 5. Volume. No engine puts an envelope on the VCA yet.
        let volume = m.out.volume;
        for sample in output.iter_mut() {
            *sample *= volume;
        }
```

- [ ] **Step 9: Pages, retired pages and the demo bindings**

In `chimera-core/src/ui/block_registry.rs`: `ENVELOPE`'s six slots become

```rust
    params: [
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::DECAY),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::SUSTAIN),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::RELEASE),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::HOLD),
        EMPTY,
    ],
```

  Delete `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` (ids 13–15, retired), `ENVELOPE_BLOCKS` and `ENVELOPE_CHAIN`.

In `chimera-core/src/ui/page.rs`: delete `PageId::EnvAmp`, `EnvFilter` and `EnvAux`, their three `binding` arms and `ENV_PAGE`. In `DEMO_MOTION`, `BlockRef::AmpEnv` becomes `BlockRef::Env(EnvSlot::Env1)` and `BlockRef::FilterEnv` becomes `BlockRef::Env(EnvSlot::Env2)` (`use crate::dsp::modulator::EnvSlot;`).

- [ ] **Step 10: Update the tests**

- `git rm chimera-core/tests/envelope_test.rs`.
- `addr_test.rs`, `block_mut_reaches_the_named_instance`: `BlockRef::FilterEnv` becomes `BlockRef::Env(EnvSlot::Env2)`, set `EnvParams::ATTACK` to `0.5`, and assert `p.envelopes[1].attack == 0.5` and `p.envelopes[0].attack == 0.189`.
- `binding_test.rs`: drop `&reg::ENV_AMP, &reg::ENV_FILTER, &reg::ENV_AUX` from `block_def_ids_are_unique` (36 entries); the ENVELOPE row of `part_pages_display_like_before` becomes `("ATK", Uni), ("DEC", Uni), ("SUS", Uni), ("REL", Uni), ("H", Uni), ("--", Uni)`.
- `focus_test.rs`: drop `&reg::ENVELOPE_CHAIN` (array of 7).
- `mod_registry_test.rs`, `registry_refuses_non_modulatable`: the two envelope lines become `ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::ATTACK), // a slider, not a destination` and `ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::SUSTAIN), // a level, not a destination`.
- `page_block_test.rs`: in `demo_pages_step_like_before` the DemoMotion assert becomes `assert_eq!(p.envelopes[1].attack, 0.189 + 1.0 / 128.0);` and the `PageId::EnvAux` lines go; in `legacy_bindings_name_semantic_addresses` the DemoMotion address is `BlockRef::Env(EnvSlot::Env2)`; `every_legacy_binding_has_a_spec` drops the three env pages.
- `part_page_test.rs`, `envelope_and_lfo_pages`, the ENVELOPE lines:

```rust
    assert_eq!(read(&reg::ENVELOPE, &p), [0.189, 0.559, 0.7, 0.559, 0.0, 0.0]);
    turn(&reg::ENVELOPE, 0, 1, &mut p);
    assert_eq!(p.envelopes[0].attack, 0.189 + 1.0 / 128.0);
    turn(&reg::ENVELOPE, 2, -1, &mut p);
    assert_eq!(p.envelopes[0].sustain, 0.7 - 1.0 / 128.0);
```

Every file that names `EnvSlot` imports `chimera_core::dsp::modulator::EnvSlot`.

- [ ] **Step 11: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS except `screen_goldens_match` (`bigviz_env`, next step). The goldens (the factory eight too) are bit-identical: no Sound or golden routes an ENV source.

- [ ] **Step 12: Look at the ENV page and re-record**

Look at `bigviz_env`: the ADSR curve with its four stage labels, cells ATK · DEC · SUS / REL · H · dash; the values read as positions (ATK 24, DEC 71). Re-record only `bigviz_env`.

- [ ] **Step 13: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/modulator chimera-core/src/dsp/mod.rs chimera-core/src/dsp/envelope.rs \
  chimera-core/src/params.rs chimera-core/src/addr.rs chimera-core/src/modulation.rs chimera-core/src/preset.rs \
  chimera-core/src/dsp/voice.rs chimera-core/src/ui/block_registry.rs chimera-core/src/ui/page.rs \
  chimera-core/src/ui/mod_grid.rs chimera-core/tests/env_a_test.rs chimera-core/tests/envelope_test.rs \
  chimera-core/tests/addr_test.rs chimera-core/tests/binding_test.rs chimera-core/tests/focus_test.rs \
  chimera-core/tests/mod_registry_test.rs chimera-core/tests/page_block_test.rs \
  chimera-core/tests/part_page_test.rs chimera-core/tests/screen_golden_test.rs
git commit -m "Envelope A: SPEED ranges, HOLD POSITION, one RC shape; ENV 1–3 run per block"
```

---

### Task 6: Envelope B and the ENV slot's TYPE changes

Envelope B (`FuncGen`) is the Cascadia's function generator: ENV, LFO and BURST modes, each with its FORMs, and RISE, FALL and SHAPE sliders whose meaning follows MODE and FORM. `EnvParams` gains MODE, FORM, RISE, FALL and SHAPE (ids 11–15). `Envelope` runs A or B by TYPE and handles a TYPE, MODE or FORM change without a jump (spec § 1). ENV 3 defaults to B (ENV, AD). MODE and FORM live in types: each MODE has its own FORM enum and its own stored FORM, and `Func` names what B runs, so a mismatch can't be represented. Per block, every rate, including the repeat of ENV CYCLE and BURST CYCLE, stops at the block rate ÷ 8 of the running sample rate. The slot computes only the running TYPE's coefficients, reuses them while their inputs hold, and its per-sample path fills a block of levels in tight loops.

**Files:**
- Modify: `chimera-core/src/dsp/modulator/mod.rs` (`FuncMode`, `EnvForm`, `LfoForm`, `Func`, `FuncParams`, `Glide`; `pub mod func;`)
- Modify: `chimera-core/src/dsp/modulator/law.rs` (B's ranges, `block_rate_max`, `curve`, `curve_inv`, `shape_w`, `tilt`)
- Create: `chimera-core/src/dsp/modulator/func.rs` (`Slides`, `BCoefs`, `FuncGen`, and a unit test of the clamp)
- Modify: `chimera-core/src/dsp/envelope.rs` (`EnvMods.slides`; A or B; the change rules; coefficient caches)
- Modify: `chimera-core/src/params.rs` (`EnvParams.func`, ids 11–15, `ParamSnapshot::default` ENV 3 = B)
- Create: `chimera-core/tests/func_gen_test.rs`, `chimera-core/tests/env_slot_test.rs`

**Interfaces:**
- Consumes: Task 5's `EnvA`, `ACoefs`, `Range`, `rc_k`, `rc_coeff`, `EnvMods`.
- Produces:
  - `dsp::modulator::FuncMode { Env = 0, Lfo = 1, Burst = 2 }`, `EnvForm { Ad = 0, Ahr = 1, Cycle = 2 }` (ENV's and BURST's FORMs), `LfoForm { Free = 0, Sync = 1, Lfv = 2 }`, each with `ALL`, default first; `Func { Env(EnvForm), Lfo(LfoForm), Burst(EnvForm) }` with `mode(self) -> FuncMode`.
  - `dsp::modulator::FuncParams { mode, env_form, lfo_form, burst_form, rise, fall, shape }` with `FuncParams::ENV`, `FuncParams::LFO`, `func(&self) -> Func`, `set_func(&mut self, Func)`, `form_index(&self) -> f32`, `set_form_index(&mut self, f32)`.
  - `dsp::modulator::Glide` (`SAMPLES = 256`; `start(&mut self, d)`, `value(&self)`, `at(&self, n)`, `advance(&mut self, n)`, `active(&self)`), shared with Task 7's `Lfo`.
  - `law::{B_TIME, B_RATE, BURST_RATE, BURST_LEN: Range, block_rate_max(sample_rate: u32) -> f32, curve(x, w), curve_inv(y, w), shape_w(shape), tilt(p, r)}`.
  - `func::{Slides { rise, fall, shape }, BCoefs::new(&FuncParams, &Slides, sample_rate: u32, per_sample: bool), FuncGen}`; `FuncGen::{new, set(&mut self, &BCoefs), note_on(&mut self, Func), key_up(&mut self), output(&self) -> f32, tick(&mut self, &BCoefs, key) -> f32, fill(&mut self, &BCoefs, key, out: &mut [f32]), advance(&mut self, &BCoefs, key, n: u32), holds(&self, key) -> bool, is_idle(&self) -> bool, rising(&self) -> bool, enter_env(&mut self, level, rising), enter_burst(&mut self, key)}`.
  - `envelope::EnvMods` gains `slides: Slides`; `Envelope::{holds(&self, key: bool) -> bool, gliding(&self) -> bool}`.
  - `EnvParams` ids `MODE 11, FORM 12, RISE 13, FALL 14, SHAPE 15`; field `func: FuncParams`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/func_gen_test.rs`:

```rust
//! Envelope B (filter-routing spec § Envelope B, § Tests "Envelope B").

use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::law::{B_RATE, B_TIME, BURST_LEN, block_rate_max};
use chimera_core::dsp::modulator::{EnvForm, Func, FuncParams, LfoForm};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

/// `FuncParams` running `f`, with the three sliders.
fn fp(f: Func, rise: f32, fall: f32, shape: f32) -> FuncParams {
    let mut p = FuncParams { rise, fall, shape, ..FuncParams::ENV };
    p.set_func(f);
    p
}

fn coefs(p: &FuncParams, per_sample: bool) -> BCoefs {
    BCoefs::new(p, &Slides::default(), SR, per_sample)
}

/// `n` ticks from a note-on with the key held for `key_for` of them.
fn run(p: &FuncParams, n: usize, key_for: usize) -> (Vec<f32>, FuncGen) {
    let c = coefs(p, true);
    let mut g = FuncGen::new();
    g.set(&c);
    g.note_on(p.func());
    let out = (0..n)
        .map(|i| {
            let key = i < key_for;
            if !key {
                g.key_up();
            }
            g.tick(&c, key)
        })
        .collect();
    (out, g)
}

fn secs(n: usize) -> f32 {
    n as f32 / SR as f32
}

#[test]
fn ad_ignores_key_up_and_ends_idle() {
    let p = fp(Func::Env(EnvForm::Ad), 0.2, 0.3, 0.5);
    let (out, g) = run(&p, SR as usize, 10);
    let top = out
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m })
        .0;
    assert!(out[top] > 0.99, "reaches 1");
    assert!((secs(top) - B_TIME.at(0.2)).abs() < 2.0 / SR as f32);
    let idle = out.iter().rposition(|&v| v > 0.0).unwrap() + 1;
    assert!((secs(idle - top) - B_TIME.at(0.3)).abs() < 2.0 / SR as f32);
    assert!(g.is_idle());
}

#[test]
fn ahr_holds_while_held_then_falls() {
    let p = fp(Func::Env(EnvForm::Ahr), 0.1, 0.2, 0.5);
    let (out, g) = run(&p, 30_000, 20_000);
    assert!(out[5_000..20_000].iter().all(|&v| v == 1.0), "holds at 1");
    let gone = out.iter().rposition(|&v| v > 0.0).unwrap() + 1;
    assert!((secs(gone - 20_000) - B_TIME.at(0.2)).abs() < 2.0 / SR as f32);
    assert!(g.is_idle());
    // A key-up during the rise falls from where it is.
    let (out, _) = run(&p, 2_000, 50);
    assert!((out[50] - out[49]).abs() < 0.02, "no step at key-up");
}

#[test]
fn cycle_period_is_rise_plus_fall() {
    let p = fp(Func::Env(EnvForm::Cycle), 0.1, 0.15, 0.5);
    let (out, g) = run(&p, 4 * SR as usize, usize::MAX);
    let lows: Vec<usize> = (1..out.len() - 1)
        .filter(|&i| out[i] < out[i - 1] && out[i] <= out[i + 1])
        .collect();
    let period = secs(lows[2] - lows[1]);
    assert!((period - (B_TIME.at(0.1) + B_TIME.at(0.15))).abs() < 3.0 / SR as f32);
    assert!(!g.is_idle(), "CYCLE is never idle");
}

/// SHAPE bottom, centre and top: log, linear and exp; at the ends a
/// segment has covered 94 % or 6 % of its swing at half its time.
#[test]
fn shape_bends_the_segments() {
    for (shape, want) in [(0.0f32, 16.0 / 17.0), (0.5, 0.5), (1.0, 1.0 / 17.0)] {
        let p = fp(Func::Env(EnvForm::Ad), 0.3, 0.3, shape);
        let half = (B_TIME.at(0.3) * SR as f32 / 2.0) as usize;
        let (out, _) = run(&p, half, usize::MAX);
        assert!((out[half - 1] - want).abs() < 0.01, "SHAPE {shape}: {}", out[half - 1]);
    }
}

/// FREE: TILT bottom, centre and top give saw, triangle and ramp, and FALL
/// offsets the phase.
#[test]
fn lfo_free_tilts_and_offsets() {
    let at = |tilt: f32, phase_off: f32, phase: f32| {
        let p = fp(Func::Lfo(LfoForm::Free), 0.3, phase_off, tilt);
        let c = coefs(&p, true);
        let mut g = FuncGen::new();
        g.set(&c);
        let n = (phase * SR as f32 / B_RATE.at(0.3)).round() as u32;
        g.advance(&c, true, n);
        g.output()
    };
    assert!((at(0.0, 0.0, 0.25) - 0.5).abs() < 0.01, "saw: 1 − φ");
    assert!(at(0.5, 0.0, 0.25).abs() < 0.01, "triangle");
    assert!((at(1.0, 0.0, 0.25) + 0.5).abs() < 0.01, "ramp: φ");
    assert!((at(0.0, 0.25, 0.0) - at(0.0, 0.0, 0.25)).abs() < 0.01, "FALL is PHASE");
}

#[test]
fn sync_resets_the_phase_at_note_on() {
    let p = fp(Func::Lfo(LfoForm::Sync), 0.5, 0.0, 0.5);
    let c = coefs(&p, false);
    let mut g = FuncGen::new();
    g.set(&c);
    g.advance(&c, true, 1_000);
    g.note_on(Func::Lfo(LfoForm::Sync));
    assert!((g.output() + 1.0).abs() < 1e-6, "the wave restarts at PHASE 0");
}

#[test]
fn lfv_is_bounded_steps_by_delta_and_slews() {
    let steps = |slew: f32| {
        let p = fp(Func::Lfo(LfoForm::Lfv), 0.6, 0.4, slew);
        let (out, _) = run(&p, 5 * SR as usize, usize::MAX);
        assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)), "within ±1");
        let cycle = (SR as f32 / B_RATE.at(0.6)) as usize;
        for w in out.chunks(cycle).collect::<Vec<_>>().windows(2) {
            assert!((w[1][0] - w[0][0]).abs() <= 0.4 + 1e-3, "DELTA bounds a cycle's move");
        }
        out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
    };
    assert!(steps(0.8) < steps(0.0), "SLEW smooths");
}

#[test]
fn burst_forms() {
    let len = |fall: f32| (BURST_LEN.at(fall) * SR as f32) as usize;
    // AD: one burst LENGTH long, key held or not.
    let p = fp(Func::Burst(EnvForm::Ad), 0.9, 0.2, 0.5);
    let (out, g) = run(&p, 3 * len(0.2), usize::MAX);
    let last = out.iter().rposition(|&v| v > 0.0).unwrap();
    assert!((last as i64 - len(0.2) as i64).abs() < 64, "{last}");
    assert!(g.is_idle());
    // AHR: sustains while held.
    let p = fp(Func::Burst(EnvForm::Ahr), 0.9, 0.2, 0.5);
    let (out, _) = run(&p, 4 * len(0.2), 3 * len(0.2));
    assert!(out[2 * len(0.2)..3 * len(0.2)].iter().any(|&v| v > 0.5), "pulses while held");
    // CYCLE: repeats while held; the running burst ends after key-up.
    let p = fp(Func::Burst(EnvForm::Cycle), 0.9, 0.2, 0.5);
    let (out, g) = run(&p, 6 * len(0.2), 3 * len(0.2) + len(0.2) / 2);
    assert!(out[2 * len(0.2)..3 * len(0.2)].iter().any(|&v| v > 0.3));
    assert!(out[5 * len(0.2)..].iter().all(|&v| v == 0.0));
    assert!(g.is_idle());
}

/// TILT bottom, centre and top put the loudest pulse at the start, middle
/// and end of the burst.
#[test]
fn burst_tilt_moves_the_loudest_pulse() {
    for (tilt, where_) in [(0.0f32, 0.0f32), (0.5, 0.5), (1.0, 1.0)] {
        let p = fp(Func::Burst(EnvForm::Ad), 0.9, 0.3, tilt);
        let n = (BURST_LEN.at(0.3) * SR as f32) as usize;
        let (out, _) = run(&p, n, usize::MAX);
        let loudest = out
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m })
            .0;
        assert!((loudest as f32 / n as f32 - where_).abs() < 0.1, "TILT {tilt}: {loudest} of {n}");
    }
}

/// The block rate ÷ 8 (93.75 Hz at 48 kHz), and at the desktop's 44.1 kHz
/// its own 86.1 Hz (Review Focus 2).
#[test]
fn per_block_rates_stop_at_an_eighth_of_the_block_rate() {
    for sr in [48_000u32, 44_100] {
        let p = fp(Func::Lfo(LfoForm::Free), 1.0, 0.0, 1.0); // 800 Hz asked
        let c = BCoefs::new(&p, &Slides::default(), sr, false);
        let mut g = FuncGen::new();
        g.set(&c);
        let v0 = g.output();
        g.advance(&c, true, BLOCK_SIZE as u32);
        // A ramp moves 2·rate/fs per sample: the clamp over a block.
        let moved = g.output() - v0;
        assert_eq!(block_rate_max(sr), sr as f32 / BLOCK_SIZE as f32 / 8.0);
        let want = 2.0 * block_rate_max(sr) * BLOCK_SIZE as f32 / sr as f32;
        assert!((moved - want).abs() < 1e-4, "{sr}: {moved} vs {want}");
    }
}

/// Envelope B from the spec's formulas in f64, `t` samples after a
/// note-on from silence (ADR 0036's reference).
fn reference(f: Func, rise: f32, fall: f32, shape: f32, t: f64) -> f64 {
    let fs = SR as f64;
    let law = |lo: f64, hi: f64, x: f32| lo * (hi / lo).powf(x as f64);
    let frac = |x: f64| x - x.floor();
    let w = 2f64.powf(4.0 * (2.0 * shape as f64 - 1.0));
    let curve = |x: f64| x / (x + (1.0 - x) * w);
    let r = shape as f64;
    let tilt = |p: f64| if p < r { p / r } else if r < 1.0 { (1.0 - p) / (1.0 - r) } else { p };
    match f {
        Func::Env(form) => {
            let (tr, tf) = (law(2e-3, 5.0, rise) * fs, law(2e-3, 5.0, fall) * fs);
            let t = if form == EnvForm::Cycle { t % (tr + tf) } else { t };
            if t < tr {
                curve(t / tr)
            } else if t < tr + tf {
                1.0 - curve((t - tr) / tf)
            } else {
                0.0
            }
        }
        Func::Lfo(_) => 2.0 * tilt(frac(t * law(0.05, 800.0, rise) / fs + fall as f64)) - 1.0,
        // CYCLE: the burst repeats every LENGTH; its pulses are the tilting saw.
        Func::Burst(_) => {
            tilt(frac(t / (law(0.01, 20.0, fall) * fs))) * tilt(frac(t * law(0.05, 1000.0, rise) / fs))
        }
    }
}

/// The per-sample paths (`tick`, `fill`) and the per-block path
/// (`advance`) each stay within 1e-4 of the f64 reference, allowing ±1
/// sample at a turn (ADR 0036, superseding the spec's 1e-6).
#[test]
fn each_path_matches_an_f64_reference() {
    for (f, rise, fall, shape) in [
        (Func::Env(EnvForm::Cycle), 0.05, 0.08, 0.7),
        (Func::Env(EnvForm::Ad), 0.02, 0.1, 0.2),
        (Func::Lfo(LfoForm::Free), 0.4, 0.1, 0.3),
        (Func::Burst(EnvForm::Cycle), 0.5, 0.1, 0.6),
    ] {
        const BLOCKS: usize = 300;
        let refs: Vec<f64> = (0..BLOCKS * BLOCK_SIZE)
            .map(|n| reference(f, rise, fall, shape, (n + 1) as f64))
            .collect();
        let near = |n: usize, v: f32| {
            (n.saturating_sub(1)..=(n + 1).min(refs.len() - 1))
                .any(|m| (refs[m] - v as f64).abs() <= 1e-4)
        };
        let p = fp(f, rise, fall, shape);
        let c = BCoefs::new(&p, &Slides::default(), SR, true);
        let (mut ticked, mut filled, mut blocked) = (FuncGen::new(), FuncGen::new(), FuncGen::new());
        for g in [&mut ticked, &mut filled, &mut blocked] {
            g.set(&c);
            g.note_on(f);
        }
        for b in 0..BLOCKS {
            let mut buf = [0.0f32; BLOCK_SIZE];
            filled.fill(&c, true, &mut buf);
            blocked.advance(&c, true, BLOCK_SIZE as u32);
            for (i, &v) in buf.iter().enumerate() {
                let n = b * BLOCK_SIZE + i;
                let t = ticked.tick(&c, true);
                assert!(near(n, t), "tick {f:?} {n}: {t} vs {}", refs[n]);
                assert!(near(n, v), "fill {f:?} {n}: {v} vs {}", refs[n]);
            }
            let n = (b + 1) * BLOCK_SIZE - 1;
            assert!(near(n, blocked.output()), "advance {f:?} block {b}: {} vs {}", blocked.output(), refs[n]);
        }
    }
}

/// RISE, FALL and SHAPE routes move their sliders' positions.
#[test]
fn slides_move_the_sliders() {
    let p = fp(Func::Env(EnvForm::Ad), 0.3, 0.3, 0.5);
    let plain = BCoefs::new(&p, &Slides::default(), SR, true);
    for s in [
        Slides { rise: 0.2, ..Slides::default() },
        Slides { fall: -0.2, ..Slides::default() },
        Slides { shape: 0.3, ..Slides::default() },
    ] {
        assert_ne!(BCoefs::new(&p, &s, SR, true), plain, "{s:?}");
    }
}
```

Create `chimera-core/tests/env_slot_test.rs`:

```rust
//! The ENV slot: A or B, the matrix's inputs, TYPE/MODE/FORM changes
//! (filter-routing spec § 1, § Tests "TYPE, MODE and FORM changes").

use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::modulator::func::Slides;
use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, FuncMode, LfoForm};
use chimera_core::params::{EnvParams, ParamSnapshot};

const SR: u32 = 48_000;

fn a() -> EnvParams {
    EnvParams::default()
}

fn b(f: Func) -> EnvParams {
    let mut p = EnvParams { env_type: EnvType::B, ..EnvParams::default() };
    p.func.set_func(f);
    p
}

fn kinds() -> Vec<EnvParams> {
    vec![
        a(),
        b(Func::Env(EnvForm::Ad)),
        b(Func::Env(EnvForm::Ahr)),
        b(Func::Env(EnvForm::Cycle)),
        b(Func::Lfo(LfoForm::Free)),
        b(Func::Lfo(LfoForm::Lfv)),
        b(Func::Burst(EnvForm::Ad)),
        b(Func::Burst(EnvForm::Cycle)),
    ]
}

#[test]
fn env3_defaults_to_b_env_ad() {
    let p = ParamSnapshot::default();
    assert_eq!(p.envelopes[0].env_type, EnvType::A);
    assert_eq!(p.envelopes[1].env_type, EnvType::A);
    let e3 = &p.envelopes[2];
    assert_eq!((e3.env_type, e3.func.func()), (EnvType::B, Func::Env(EnvForm::Ad)));
    // Each MODE keeps its own FORM: LFO's default is FREE, not ENV's AD.
    assert_eq!(e3.func.lfo_form, LfoForm::Free);
}

/// Every change between the kinds, mid-note, key down and up: the first
/// output after it is the last before it; into A or B ENV nothing glides.
#[test]
fn type_mode_and_form_changes_never_step() {
    for from in kinds() {
        for to in kinds() {
            for key_down in [true, false] {
                let mut e = Envelope::new();
                e.note_on(&from);
                for blk in 0..12 {
                    e.run_block(&from, &EnvMods::NONE, key_down || blk < 6, SR, None);
                }
                let before = e.output();
                let start = e.run_block(&to, &EnvMods::NONE, key_down, SR, None);
                assert!((start - before).abs() < 1e-6, "{from:?} → {to:?}");
                let into_env = to.env_type == EnvType::A || to.func.mode == FuncMode::Env;
                if into_env && (0.0..=1.0).contains(&before) {
                    assert!(!e.gliding(), "{from:?} → {to:?} glides");
                }
                let mut last = [0.0f32; 4];
                for l in last.iter_mut() {
                    *l = e.run_block(&to, &EnvMods::NONE, key_down, SR, None);
                }
                assert!(!e.gliding(), "the glide lasts 256 samples");
                // A held CYCLE never parks, whatever level it was entered at.
                if key_down && to.env_type == EnvType::B && to.func.func() == Func::Env(EnvForm::Cycle) {
                    assert_ne!(last[2], last[3], "{from:?} → CYCLE is moving");
                }
            }
        }
    }
}

/// Review Focus 3: TYPE, MODE and FORM spun one step every block.
#[test]
fn spinning_type_every_block_stays_bounded() {
    let k = kinds();
    let mut e = Envelope::new();
    e.note_on(&k[0]);
    for blk in 0..1000 {
        let p = &k[blk % k.len()];
        let v = e.run_block(p, &EnvMods::NONE, blk % 97 < 60, SR, None);
        assert!(v.is_finite() && (-1.0 - 1e-6..=1.0 + 1e-6).contains(&v), "block {blk}: {v}");
    }
}

/// LEVEL and TIME are inert on B; RISE, FALL and SHAPE on A.
#[test]
fn a_and_b_destinations_are_inert_on_the_other_type() {
    let render = |p: &EnvParams, m: &EnvMods| {
        let mut e = Envelope::new();
        e.note_on(p);
        (0..50).map(|_| e.run_block(p, m, true, SR, None)).collect::<Vec<_>>()
    };
    let slides = EnvMods { slides: Slides { rise: 0.3, fall: -0.2, shape: 0.4 }, ..EnvMods::NONE };
    let ctrl = EnvMods { level: Some(0.3), time: 0.5, ..EnvMods::NONE };
    let pa = a();
    let pb = b(Func::Env(EnvForm::Ad));
    assert_eq!(render(&pa, &slides), render(&pa, &EnvMods::NONE));
    assert_eq!(render(&pb, &ctrl), render(&pb, &EnvMods::NONE));
    assert_ne!(render(&pb, &slides), render(&pb, &EnvMods::NONE));
    assert_ne!(render(&pa, &ctrl), render(&pa, &EnvMods::NONE));
}

/// A FORM change first seen at a note-on (no block ran in between) runs
/// the new FORM's note-on: an LFO switched to SYNC restarts at its PHASE.
#[test]
fn a_change_seen_at_a_note_on_runs_the_new_note_on() {
    let (free, sync) = (b(Func::Lfo(LfoForm::Free)), b(Func::Lfo(LfoForm::Sync)));
    let mut e = Envelope::new();
    e.note_on(&free);
    for _ in 0..20 {
        e.run_block(&free, &EnvMods::NONE, true, SR, None);
    }
    e.note_on(&sync);
    let mut fresh = Envelope::new();
    fresh.note_on(&sync);
    let (mut x, mut y) = (0.0, 0.0);
    for _ in 0..6 {
        x = e.run_block(&sync, &EnvMods::NONE, true, SR, None);
        y = fresh.run_block(&sync, &EnvMods::NONE, true, SR, None);
    }
    assert!(!e.gliding(), "the glide is over");
    assert!((x - y).abs() < 1e-6, "{x} vs {y}: SYNC restarted at the note-on");
}

#[test]
fn holds_follow_the_lifetime_rule() {
    // A and B AD/AHR hold until idle; B CYCLE and B LFO only while held.
    for (p, holds_after_key_up) in [
        (a(), true),
        (b(Func::Env(EnvForm::Ad)), true),
        (b(Func::Env(EnvForm::Cycle)), false),
        (b(Func::Lfo(LfoForm::Free)), false),
    ] {
        let mut e = Envelope::new();
        e.note_on(&p);
        e.run_block(&p, &EnvMods::NONE, true, SR, None);
        e.run_block(&p, &EnvMods::NONE, false, SR, None);
        assert_eq!(e.holds(false), holds_after_key_up, "{p:?}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test func_gen_test --test env_slot_test`
Expected: FAIL to compile: no `FuncMode`, `func`, `EnvParams::func`.

- [ ] **Step 3: B's types, and the glide**

Append to `chimera-core/src/dsp/modulator/mod.rs` (and add `pub mod func;` beside `pub mod env_a;`):

```rust
/// Envelope B's MODE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FuncMode {
    Env = 0,
    Lfo = 1,
    Burst = 2,
}

impl FuncMode {
    pub const ALL: [FuncMode; 3] = [FuncMode::Env, FuncMode::Lfo, FuncMode::Burst];
}

/// ENV's and BURST's FORMs (the Cascadia's TYPE SELECT, renamed so it
/// doesn't clash with the slot's TYPE), as the knob steps them, default first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvForm {
    Ad = 0,
    Ahr = 1,
    Cycle = 2,
}

impl EnvForm {
    pub const ALL: [EnvForm; 3] = [EnvForm::Ad, EnvForm::Ahr, EnvForm::Cycle];
}

/// LFO's FORMs, default first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LfoForm {
    Free = 0,
    Sync = 1,
    Lfv = 2,
}

impl LfoForm {
    pub const ALL: [LfoForm; 3] = [LfoForm::Free, LfoForm::Sync, LfoForm::Lfv];
}

/// What Envelope B runs: a MODE and one of that MODE's FORMs, so a
/// mismatch can't be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Env(EnvForm),
    Lfo(LfoForm),
    Burst(EnvForm),
}

impl Func {
    pub const fn mode(self) -> FuncMode {
        match self {
            Func::Env(_) => FuncMode::Env,
            Func::Lfo(_) => FuncMode::Lfo,
            Func::Burst(_) => FuncMode::Burst,
        }
    }
}

/// Envelope B's switches and sliders; also a FUNC LFO's, whose MODE is
/// always LFO. Each MODE keeps its own FORM, so a MODE switched away and
/// back finds its FORM again. Sliders are positions, 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuncParams {
    pub mode: FuncMode,
    pub env_form: EnvForm,
    pub lfo_form: LfoForm,
    pub burst_form: EnvForm,
    pub rise: f32,
    pub fall: f32,
    pub shape: f32,
}

impl FuncParams {
    /// ENV 3's default: ENV, AD, 10 ms rise, 300 ms fall, linear.
    pub const ENV: Self = Self {
        mode: FuncMode::Env,
        env_form: EnvForm::Ad,
        lfo_form: LfoForm::Free,
        burst_form: EnvForm::Ad,
        rise: 0.206,
        fall: 0.640,
        shape: 0.5,
    };
    /// A FUNC LFO's default: FREE at 1 Hz, PHASE 0, TILT centre (triangle).
    pub const LFO: Self = Self {
        mode: FuncMode::Lfo,
        rise: 0.309,
        fall: 0.0,
        ..Self::ENV
    };

    pub fn func(&self) -> Func {
        match self.mode {
            FuncMode::Env => Func::Env(self.env_form),
            FuncMode::Lfo => Func::Lfo(self.lfo_form),
            FuncMode::Burst => Func::Burst(self.burst_form),
        }
    }

    /// Run `f`: its MODE, with that MODE's FORM set.
    pub fn set_func(&mut self, f: Func) {
        match f {
            Func::Env(x) => (self.mode, self.env_form) = (FuncMode::Env, x),
            Func::Lfo(x) => (self.mode, self.lfo_form) = (FuncMode::Lfo, x),
            Func::Burst(x) => (self.mode, self.burst_form) = (FuncMode::Burst, x),
        }
    }

    /// FORM as a `Block` value: the index in its MODE's list.
    pub fn form_index(&self) -> f32 {
        match self.func() {
            Func::Env(x) | Func::Burst(x) => x as u8 as f32,
            Func::Lfo(x) => x as u8 as f32,
        }
    }

    pub fn set_form_index(&mut self, v: f32) {
        match self.mode {
            FuncMode::Env => self.env_form = pick(&EnvForm::ALL, v),
            FuncMode::Lfo => self.lfo_form = pick(&LfoForm::ALL, v),
            FuncMode::Burst => self.burst_form = pick(&EnvForm::ALL, v),
        }
    }
}

/// A TYPE, MODE or FORM change's leftover `d`, gliding linearly to 0 over
/// 256 samples (spec § 1). Shared by the ENV and LFO slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glide {
    d: f32,
    left: u16,
}

impl Glide {
    pub const SAMPLES: u16 = 256;
    pub const NONE: Self = Self { d: 0.0, left: 0 };

    /// Glide `d` out; below 1e-6 there is nothing to glide.
    pub fn start(&mut self, d: f32) {
        *self = if d.abs() > 1e-6 {
            Self { d, left: Self::SAMPLES }
        } else {
            Self::default()
        };
    }

    /// The leftover now.
    pub fn value(&self) -> f32 {
        self.at(0)
    }

    /// The leftover `n` samples on.
    pub fn at(&self, n: usize) -> f32 {
        let left = (self.left as usize).saturating_sub(n);
        if left == 0 { 0.0 } else { self.d * (left as f32 / Self::SAMPLES as f32) }
    }

    pub fn advance(&mut self, n: u16) {
        self.left = self.left.saturating_sub(n);
    }

    pub fn active(&self) -> bool {
        self.left > 0
    }
}
```

- [ ] **Step 4: B's laws**

Append to `chimera-core/src/dsp/modulator/law.rs`:

```rust
/// Envelope B's ranges (spec § Envelope B): ENV RISE and FALL 2 ms – 5 s.
pub const B_TIME: Range = Range { min: 2e-3, oct: 11.287_712 };
/// LFO RATE, 0.05 – 800 Hz.
pub const B_RATE: Range = Range { min: 0.05, oct: 13.965_784 };
/// BURST pulse RATE, 0.05 Hz – 1 kHz.
pub const BURST_RATE: Range = Range { min: 0.05, oct: 14.287_712 };
/// BURST LENGTH, 10 ms – 20 s.
pub const BURST_LEN: Range = Range { min: 0.01, oct: 10.965_784 };
/// A B slot evaluated per block stops its rates here: the block rate ÷ 8
/// (spec § Rates), 93.75 Hz at 48 kHz and 86.1 Hz at 44.1 kHz.
pub fn block_rate_max(sample_rate: u32) -> f32 {
    sample_rate as f32 / chimera_hal::BLOCK_SIZE as f32 / 8.0
}

/// SHAPE's curve, `f(x) = x / (x + (1 − x)·w)`; linear (and no divide) at `w` = 1.
pub fn curve(x: f32, w: f32) -> f32 {
    if w == 1.0 { x } else { x / (x + (1.0 - x) * w) }
}

/// `f⁻¹(y) = w·y / (1 − y + w·y)`.
pub fn curve_inv(y: f32, w: f32) -> f32 {
    if w == 1.0 { y } else { w * y / (1.0 - y + w * y) }
}

/// SHAPE's position to `w = 2^(4·(2·SHAPE − 1))`; exactly 1 at the centre.
pub fn shape_w(shape: f32) -> f32 {
    fast_exp2(4.0 * (2.0 * shape.max(0.0).min(1.0) - 1.0))
}

/// TILT: rise over the fraction `r` of a cycle, fall over the rest.
/// `u = p/r` rising, `(1 − p)/(1 − r)` falling; r = 0 is `1 − p`, r = 1 is `p`.
pub fn tilt(p: f32, r: f32) -> f32 {
    if p < r {
        p / r
    } else if r < 1.0 {
        (1.0 - p) / (1.0 - r)
    } else {
        p
    }
}
```

- [ ] **Step 5: Envelope B**

Create `chimera-core/src/dsp/modulator/func.rs`:

```rust
//! Envelope B (spec § Envelope B): the Cascadia's function generator in
//! ENV, LFO and BURST modes; a FUNC LFO runs it in LFO mode.

use core::f32::consts::{FRAC_PI_2, TAU};

use crate::dsp::modulator::law::{
    B_RATE, B_TIME, BURST_LEN, BURST_RATE, block_rate_max, curve, curve_inv, rc_coeff, rc_k,
    shape_w, tilt,
};
use crate::dsp::modulator::{EnvForm, Func, FuncParams, LfoForm};
use crate::dsp::{fast_exp2, fast_sin};

/// The matrix's offsets into RISE, FALL and SHAPE, added to their positions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slides {
    pub rise: f32,
    pub fall: f32,
    pub shape: f32,
}

/// B's constants for one block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BCoefs {
    func: Func,
    /// Per sample: the rise's `x` step (ENV), φ's (LFO), the pulse phase's (BURST).
    rise: f32,
    /// Per sample: the fall's `x` step (ENV); the burst's, a fraction of LENGTH (BURST).
    fall: f32,
    /// A turn's overshoot, converted between the two segments' units.
    fall_per_rise: f32,
    rise_per_fall: f32,
    /// SHAPE's curve weight (ENV).
    w: f32,
    /// SHAPE's position: TILT (LFO, BURST), SLEW (LFV).
    shape: f32,
    /// FALL's position: PHASE (FREE, SYNC), DELTA (LFV).
    fall_pos: f32,
    /// LFV's slew: its per-sample step and `log2` retention (none: 1, −∞).
    slew_c: f32,
    slew_k: f32,
    /// φ's step per sample in a `u32` turn (LFO; BURST's pulse phase): an
    /// integer accumulator wraps exactly, where `phase += step` in f32
    /// drifts past 1e-4 within a second.
    inc: u32,
}

impl BCoefs {
    /// `per_sample`: the slot feeds the VCA, so its rates keep the manual's
    /// full range. Per block, every rate stops at the block rate ÷ 8: LFO
    /// RATE, BURST's pulse RATE, and the repeats of ENV CYCLE
    /// (`1/(RISE + FALL)`, both segments slowed alike) and BURST CYCLE
    /// (`1/LENGTH`). One-shot AD and AHR times are not clamped.
    pub fn new(p: &FuncParams, s: &Slides, sample_rate: u32, per_sample: bool) -> Self {
        let fs = sample_rate as f32;
        let pos = |v: f32, off: f32| (v + off).max(0.0).min(1.0);
        let (rise_pos, fall_pos, shape) = (pos(p.rise, s.rise), pos(p.fall, s.fall), pos(p.shape, s.shape));
        let max = if per_sample { f32::INFINITY } else { block_rate_max(sample_rate) };
        let func = p.func();
        let (rise, fall) = match func {
            Func::Env(form) => {
                let (tr, tf) = (B_TIME.at(rise_pos), B_TIME.at(fall_pos));
                let slow = if form == EnvForm::Cycle { ((tr + tf) * max).recip().max(1.0) } else { 1.0 };
                (1.0 / (tr * slow * fs), 1.0 / (tf * slow * fs))
            }
            Func::Lfo(_) => (B_RATE.at(rise_pos).min(max) / fs, 0.0),
            Func::Burst(form) => {
                let len = BURST_LEN.at(fall_pos);
                let len = if form == EnvForm::Cycle { len.max(max.recip()) } else { len };
                (BURST_RATE.at(rise_pos).min(max) / fs, 1.0 / (len * fs))
            }
        };
        let (slew_c, slew_k) = if func == Func::Lfo(LfoForm::Lfv) && shape > 0.0 {
            // τ = SLEW × one cycle.
            let k = rc_k(shape / (rise * fs), fs);
            (rc_coeff(k), k)
        } else {
            (1.0, f32::NEG_INFINITY)
        };
        Self {
            func,
            rise,
            fall,
            fall_per_rise: if rise > 0.0 { fall / rise } else { 0.0 },
            rise_per_fall: if fall > 0.0 { rise / fall } else { 0.0 },
            w: shape_w(shape),
            shape,
            fall_pos,
            slew_c,
            slew_k,
            inc: match func {
                Func::Env(_) => 0,
                Func::Lfo(_) | Func::Burst(_) => (rise * TURN) as u32,
            },
        }
    }
}

/// One turn of φ as a `u32`.
const TURN: f32 = 4_294_967_296.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seg {
    Idle,
    Rise,
    Hold,
    Fall,
}

/// Envelope B's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FuncGen {
    func: Func,
    seg: Seg,
    /// ENV: the running segment's linear position, 0..1 (a fall runs 1 → 0).
    /// BURST: time into the burst, a fraction of LENGTH.
    x: f32,
    /// `x`'s anchor: `x = x0 ± k·step`, computed, not accumulated, so a
    /// constant step's f32 rounding can't build up (ADR 0036). Re-anchored
    /// at every turn, wrap and block start (`go`, `set`).
    x0: f32,
    k: u32,
    /// LFO φ; BURST's pulse phase: a `u32` turn.
    phase: u32,
    /// LFV: the cycle's start and end targets, and the slewed output.
    from: f32,
    to: f32,
    slewed: f32,
    rng: u32,
    /// BURST CYCLE: the key went up; the running burst is the last.
    last_burst: bool,
    /// The last block's SHAPE weight, TILT and PHASE, for reads between blocks.
    w: f32,
    tilt: f32,
    phase_off: f32,
}

impl Default for FuncGen {
    fn default() -> Self {
        Self::new()
    }
}

impl FuncGen {
    pub const fn new() -> Self {
        Self {
            func: Func::Env(EnvForm::Ad),
            seg: Seg::Idle,
            x: 0.0,
            x0: 0.0,
            k: 0,
            phase: 0,
            from: 0.0,
            to: 0.0,
            slewed: 0.0,
            rng: 0x2545_f491,
            last_burst: false,
            w: 1.0,
            tilt: 0.5,
            phase_off: 0.0,
        }
    }

    /// Take a block's MODE, FORM and shape; re-anchor, since the step may
    /// change.
    pub fn set(&mut self, c: &BCoefs) {
        self.func = c.func;
        (self.w, self.tilt, self.phase_off) = (c.w, c.shape, c.fall_pos);
        (self.x0, self.k) = (self.x, 0);
    }

    /// Into segment `seg` at `x`, anchored there.
    fn go(&mut self, seg: Seg, x: f32) {
        (self.seg, self.x, self.x0, self.k) = (seg, x, x, 0);
    }

    /// φ as 0..1.
    fn phase_f(&self) -> f32 {
        self.phase as f32 * (1.0 / TURN)
    }

    /// A note-on: AD and AHR rise from the current level, CYCLE restarts its
    /// rise, SYNC resets φ, BURST starts a burst; FREE, LFV and the pulse
    /// phase run on (spec, Defaults chosen 18).
    pub fn note_on(&mut self, f: Func) {
        self.func = f;
        match f {
            Func::Env(EnvForm::Cycle) => self.go(Seg::Rise, 0.0),
            Func::Env(_) => {
                let l = self.env_level();
                self.go(Seg::Rise, curve_inv(l, self.w));
            }
            Func::Lfo(LfoForm::Sync) => self.phase = 0,
            Func::Lfo(_) => {}
            Func::Burst(_) => {
                self.go(Seg::Rise, 0.0);
                self.last_burst = false;
            }
        }
    }

    /// The key is up this block: AHR falls, a held burst goes on, a
    /// cycling burst finishes.
    pub fn key_up(&mut self) {
        match (self.func, self.seg) {
            (Func::Env(EnvForm::Ahr), Seg::Rise) => {
                let l = self.env_level();
                self.fall_from(l);
            }
            (Func::Env(EnvForm::Ahr), Seg::Hold) => self.go(Seg::Fall, 1.0),
            (Func::Burst(EnvForm::Ahr), Seg::Hold) => self.seg = Seg::Rise,
            (Func::Burst(EnvForm::Cycle), Seg::Rise) => self.last_burst = true,
            _ => {}
        }
    }

    fn fall_from(&mut self, l: f32) {
        self.go(Seg::Fall, 1.0 - curve_inv(1.0 - l, self.w));
    }

    /// ENV mode's level: `f(x)` rising, `1 − f(1 − x)` falling.
    fn env_level(&self) -> f32 {
        match self.seg {
            Seg::Idle => 0.0,
            Seg::Rise => curve(self.x, self.w),
            Seg::Hold => 1.0,
            Seg::Fall => 1.0 - curve(1.0 - self.x, self.w),
        }
    }

    fn lfo_level(&self, form: LfoForm) -> f32 {
        if form == LfoForm::Lfv {
            return self.slewed;
        }
        let p = self.phase_f() + self.phase_off;
        2.0 * tilt(p - (p as u32) as f32, self.tilt) - 1.0
    }

    fn burst_level(&self, form: EnvForm) -> f32 {
        if self.seg == Seg::Idle {
            return 0.0;
        }
        let env = tilt(self.x.min(1.0), self.tilt);
        let ph = self.phase_f();
        let pulse = if form == EnvForm::Cycle {
            tilt(ph, self.tilt)
        } else {
            let m = 1.0 - (2.0 * self.tilt - 1.0).abs();
            let square = if ph < 0.5 { 1.0 } else { 0.0 };
            let sine = 0.5 - 0.5 * fast_sin(TAU * ph + FRAC_PI_2);
            (1.0 - m) * square + m * sine
        };
        env * pulse
    }

    /// The output now: 0..1 (ENV, BURST) or −1..1 (LFO).
    pub fn output(&self) -> f32 {
        match self.func {
            Func::Env(_) => self.env_level(),
            Func::Lfo(form) => self.lfo_level(form),
            Func::Burst(form) => self.burst_level(form),
        }
    }

    /// One sample; returns the output after it.
    pub fn tick(&mut self, c: &BCoefs, key: bool) -> f32 {
        self.step(c, key, 1, true);
        self.output()
    }

    /// A block of per-sample outputs, as `tick` would give them, for the
    /// VCA: the MODE matched once, one output evaluation (one curve
    /// divide) per sample.
    pub fn fill(&mut self, c: &BCoefs, key: bool, out: &mut [f32]) {
        match c.func {
            Func::Env(form) => {
                for o in out.iter_mut() {
                    self.env_step(c, form, key, 1);
                    *o = self.env_level();
                }
            }
            Func::Lfo(form) => {
                for o in out.iter_mut() {
                    self.lfo_step(c, form, 1, true);
                    *o = self.lfo_level(form);
                }
            }
            Func::Burst(form) => {
                for o in out.iter_mut() {
                    self.burst_step(c, form, key, 1);
                    *o = self.burst_level(form);
                }
            }
        }
    }

    /// `n` samples at once: linear in `x` and φ, so within ±1 sample of `n`
    /// ticks (ADR 0036).
    pub fn advance(&mut self, c: &BCoefs, key: bool, n: u32) {
        self.step(c, key, n, false);
    }

    fn step(&mut self, c: &BCoefs, key: bool, n: u32, tick: bool) {
        match c.func {
            Func::Env(form) => self.env_step(c, form, key, n),
            Func::Lfo(form) => self.lfo_step(c, form, n, tick),
            Func::Burst(form) => self.burst_step(c, form, key, n),
        }
    }

    fn env_step(&mut self, c: &BCoefs, form: EnvForm, key: bool, n: u32) {
        self.k += n;
        match self.seg {
            Seg::Rise => self.x = self.x0 + self.k as f32 * c.rise,
            Seg::Fall => self.x = self.x0 - self.k as f32 * c.fall,
            Seg::Idle | Seg::Hold => return,
        }
        // Turns carry the overshoot into the next segment, anchored there.
        for _ in 0..4 {
            match self.seg {
                Seg::Rise if self.x >= 1.0 => {
                    if form == EnvForm::Ahr && key {
                        self.go(Seg::Hold, 1.0);
                        return;
                    }
                    self.go(Seg::Fall, 1.0 - (self.x - 1.0) * c.fall_per_rise);
                }
                Seg::Fall if self.x <= 0.0 => {
                    if form != EnvForm::Cycle {
                        self.go(Seg::Idle, 0.0);
                        return;
                    }
                    self.go(Seg::Rise, -self.x * c.rise_per_fall);
                }
                _ => return,
            }
        }
    }

    fn lfo_step(&mut self, c: &BCoefs, form: LfoForm, n: u32, tick: bool) {
        // Exact in integers; per block the clamp allows at most one wrap.
        let t = self.phase as u64 + c.inc as u64 * n as u64;
        self.phase = t as u32;
        if form == LfoForm::Lfv {
            if t >> 32 != 0 {
                self.next_target(c.fall_pos);
            }
            let lin = self.from + (self.to - self.from) * self.phase_f();
            self.slewed = if tick {
                self.slewed + c.slew_c * (lin - self.slewed)
            } else {
                lin + (self.slewed - lin) * fast_exp2(c.slew_k * n as f32)
            };
        }
    }

    /// LFV's next target, `clamp(t + DELTA·r, −1, 1)` with `r` uniform in −1..1.
    fn next_target(&mut self, delta: f32) {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        let r = x as f32 / u32::MAX as f32 * 2.0 - 1.0;
        self.from = self.to;
        self.to = (self.to + delta * r).max(-1.0).min(1.0);
    }

    fn burst_step(&mut self, c: &BCoefs, form: EnvForm, key: bool, n: u32) {
        self.phase = self.phase.wrapping_add(c.inc.wrapping_mul(n));
        if matches!(self.seg, Seg::Idle | Seg::Hold) {
            return;
        }
        let before = self.x;
        self.k += n;
        self.x = self.x0 + self.k as f32 * c.fall;
        if form == EnvForm::Ahr && key && before <= c.shape && self.x >= c.shape {
            self.go(Seg::Hold, c.shape);
        } else if self.x >= 1.0 {
            if form == EnvForm::Cycle && !self.last_burst {
                self.go(Seg::Rise, self.x - 1.0);
            } else {
                self.go(Seg::Idle, 0.0);
            }
        }
    }

    /// Holds a voice through the VCA (spec § 4): AD and AHR until idle;
    /// ENV CYCLE and LFO while the key is held; BURST CYCLE until its last
    /// burst ends.
    pub fn holds(&self, key: bool) -> bool {
        match self.func {
            Func::Lfo(_) | Func::Env(EnvForm::Cycle) => key,
            _ => self.seg != Seg::Idle,
        }
    }

    /// Done: ENV CYCLE and LFO never are.
    pub fn is_idle(&self) -> bool {
        !matches!(self.func, Func::Lfo(_) | Func::Env(EnvForm::Cycle)) && self.seg == Seg::Idle
    }

    /// On a rising segment, for a TYPE change into A or B ENV.
    pub fn rising(&self) -> bool {
        match self.func {
            Func::Env(_) => self.seg == Seg::Rise,
            Func::Lfo(_) => {
                let p = self.phase_f() + self.phase_off;
                p - (p as u32) as f32 < self.tilt
            }
            Func::Burst(_) => self.seg != Seg::Idle && self.x < self.tilt,
        }
    }

    /// Into ENV mode at `level` (spec § 1): rising → rise from it, else fall
    /// from it. At 0 the fall is already over: CYCLE turns and rises (its own
    /// rule), AD and AHR are done. Call after `set`, which names the FORM.
    pub fn enter_env(&mut self, level: f32, rising: bool) {
        let l = level.max(0.0).min(1.0);
        if rising {
            self.go(Seg::Rise, curve_inv(l, self.w));
        } else if l > 0.0 {
            self.fall_from(l);
        } else if self.func == Func::Env(EnvForm::Cycle) {
            self.go(Seg::Rise, 0.0);
        } else {
            self.go(Seg::Idle, 0.0);
        }
    }

    /// Into BURST: a burst starts now if the key is held.
    pub fn enter_burst(&mut self, key: bool) {
        self.go(if key { Seg::Rise } else { Seg::Idle }, 0.0);
        self.last_burst = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per block, the cycling forms repeat no faster than the block rate ÷ 8
    /// (spec § Rates, gap 8's ruling); per sample, and in one-shot forms,
    /// nothing is clamped.
    #[test]
    fn cycles_repeat_no_faster_than_the_clamp() {
        let sr = 48_000;
        let max = block_rate_max(sr) / sr as f32; // cycles per sample
        let fastest = |f| {
            let mut p = FuncParams { rise: 0.0, fall: 0.0, ..FuncParams::ENV };
            p.set_func(f);
            p
        };
        let coefs = |f, per_sample| BCoefs::new(&fastest(f), &Slides::default(), sr, per_sample);
        // ENV CYCLE at 2 ms + 2 ms (250 Hz): one period is 1/rise + 1/fall samples.
        let env = coefs(Func::Env(EnvForm::Cycle), false);
        assert!(1.0 / (1.0 / env.rise + 1.0 / env.fall) <= max * 1.0001);
        // BURST CYCLE at 10 ms (100 Hz) with 50 Hz pulses.
        let burst = coefs(Func::Burst(EnvForm::Cycle), false);
        assert!(burst.fall <= max * 1.0001);
        assert!(coefs(Func::Env(EnvForm::Cycle), true).rise > max);
        assert!(coefs(Func::Env(EnvForm::Ad), false).rise > max);
    }

    /// CYCLE entered at level 0 (from a finished AD, or an idle A) rises at
    /// once instead of parking; AD entered at 0 stays done.
    #[test]
    fn cycle_entered_at_zero_rises() {
        for (f, moves) in [(Func::Env(EnvForm::Cycle), true), (Func::Env(EnvForm::Ad), false)] {
            let mut p = FuncParams::ENV;
            p.set_func(f);
            let c = BCoefs::new(&p, &Slides::default(), 48_000, false);
            let mut g = FuncGen::new();
            g.set(&c);
            g.enter_env(0.0, false);
            g.advance(&c, true, 64);
            assert_eq!(g.output() > 0.0, moves, "{f:?}");
        }
    }
}
```

`BCoefs` derives `PartialEq` for `slides_move_the_sliders`. `fast_exp2(−∞ · n)` clamps to 2^−126, so "no slew" leaves the slew at the linear value.

- [ ] **Step 6: `EnvParams` takes B's fields**

In `params.rs`: `EnvParams` gains `pub func: FuncParams,` (doc: `/// Envelope B's MODE, FORM, RISE, FALL and SHAPE.`), `Default` sets `func: FuncParams::ENV`, and:

```rust
    pub const MODE: ParamId = ParamId(11);
    pub const FORM: ParamId = ParamId(12);
    pub const RISE: ParamId = ParamId(13);
    pub const FALL: ParamId = ParamId(14);
    pub const SHAPE: ParamId = ParamId(15);
```

`ENV_SPECS` becomes `[ParamSpec; 16]` with:

```rust
    ParamSpec::choice(11, "MODE", ValFmt::Names(&["ENV", "LFO", "BURST"]), 2.0, 0.0),
    ParamSpec::choice(12, "FORM", ValFmt::Names(&["AD", "AHR", "CYCLE"]), 2.0, 0.0),
    ParamSpec::continuous(13, "RISE", ValFmt::Uni, 0.0, 1.0, 0.206, 1.0 / 128.0, false),
    ParamSpec::continuous(14, "FALL", ValFmt::Uni, 0.0, 1.0, 0.640, 1.0 / 128.0, false),
    ParamSpec::continuous(15, "SHAPE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
```

`get`: `Self::MODE => self.func.mode as u8 as f32, Self::FORM => self.func.form_index(), Self::RISE => self.func.rise, Self::FALL => self.func.fall, Self::SHAPE => self.func.shape,`. `write`: `Self::MODE => self.func.mode = pick(&FuncMode::ALL, v), Self::FORM => self.func.set_form_index(v), Self::RISE => self.func.rise = v, Self::FALL => self.func.fall = v, Self::SHAPE => self.func.shape = v,`.

`ParamSnapshot::default()`: `envelopes: [EnvParams::default(), EnvParams::default(), EnvParams { env_type: EnvType::B, ..EnvParams::default() }],` (its `func` is already `FuncParams::ENV`: ENV, AD). `use crate::dsp::modulator::{FuncMode, FuncParams};` joins the imports.

- [ ] **Step 7: The slot runs A or B**

In `chimera-core/src/dsp/envelope.rs`, `EnvMods` gains B's inputs:

```rust
    /// RISE, FALL and SHAPE's Σ, added to their positions (type B).
    pub slides: Slides,
```

(and `slides: Slides { rise: 0.0, fall: 0.0, shape: 0.0 }` in `NONE`). Keep `AKey` and `add_ramped` from Task 5, and replace `Envelope` with:

```rust
use crate::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use crate::dsp::modulator::{EnvType, Func, FuncParams, Glide};

/// What a slot runs (spec § 1): Envelope A, or B with its MODE and FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    A,
    B(Func),
}

impl Kind {
    fn of(p: &EnvParams) -> Self {
        match p.env_type {
            EnvType::A => Kind::A,
            EnvType::B => Kind::B(p.func.func()),
        }
    }
}

/// What a B slot's coefficients were built from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BKey {
    func: FuncParams,
    slides: Slides,
    sample_rate: u32,
    per_sample: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    a: EnvA,
    b: FuncGen,
    /// What ran last block; `None` before the first.
    kind: Option<Kind>,
    /// The running TYPE's coefficients and their inputs, reused while
    /// the inputs hold.
    ac: Option<(AKey, ACoefs)>,
    bc: Option<(BKey, BCoefs)>,
    /// This block's peak (LEVEL; 1 under B) and the last block's.
    peak: f32,
    prev_peak: f32,
    /// A change's leftover, gliding out.
    glide: Glide,
}

impl Default for Envelope {
    fn default() -> Self {
        Self::new()
    }
}

impl Envelope {
    pub const fn new() -> Self {
        Self {
            a: EnvA::new(),
            b: FuncGen::new(),
            kind: None,
            ac: None,
            bc: None,
            peak: 1.0,
            prev_peak: 1.0,
            glide: Glide::NONE,
        }
    }
```

```rust
    /// This block's A coefficients, rebuilt only when an input changed.
    fn a_coefs(&mut self, p: &EnvParams, time: f32, sample_rate: u32) -> ACoefs {
        let key = AKey::of(p, time, sample_rate);
        match self.ac {
            Some((k, c)) if k == key => c,
            _ => {
                let c = ACoefs::new(p, time, sample_rate);
                self.ac = Some((key, c));
                c
            }
        }
    }

    /// This block's B coefficients, rebuilt only when an input changed.
    fn b_coefs(&mut self, p: &EnvParams, slides: &Slides, sample_rate: u32, per_sample: bool) -> BCoefs {
        let key = BKey { func: p.func, slides: *slides, sample_rate, per_sample };
        match self.bc {
            Some((k, c)) if k == key => c,
            _ => {
                let c = BCoefs::new(&p.func, slides, sample_rate, per_sample);
                self.bc = Some((key, c));
                c
            }
        }
    }

    fn is_b(&self) -> bool {
        matches!(self.kind, Some(Kind::B(_)))
    }

    /// A note-on. A TYPE, MODE or FORM change not yet seen by a block (the
    /// slot sat idle) takes over first, so the new kind's note-on runs.
    pub fn note_on(&mut self, p: &EnvParams) {
        let k = Kind::of(p);
        if self.kind.is_some_and(|was| was != k) {
            let (old, rising) = (self.output(), self.rising());
            self.kind = Some(k);
            self.take_over(k, old, rising, true, p.sustain);
        }
        self.kind = Some(k);
        match k {
            Kind::A => self.a.note_on(),
            Kind::B(f) => self.b.note_on(f),
        }
    }

    fn raw(&self) -> f32 {
        if self.is_b() { self.b.output() } else { self.a.level() * self.peak }
    }

    /// The output now: 0..1, or −1..1 for B in LFO mode. No velocity.
    pub fn output(&self) -> f32 {
        self.raw() + self.glide.value()
    }

    pub fn is_idle(&self) -> bool {
        if self.is_b() { self.b.is_idle() } else { self.a.is_idle() }
    }

    /// Whether this slot, routed to the VCA, still holds the voice (spec § 4).
    pub fn holds(&self, key: bool) -> bool {
        if self.is_b() { self.b.holds(key) } else { !self.a.is_idle() }
    }

    /// A change's leftover is still gliding out.
    pub fn gliding(&self) -> bool {
        self.glide.active()
    }

    fn rising(&self) -> bool {
        if self.is_b() { self.b.rising() } else { self.a.rising() }
    }

    /// The new kind takes over at the old output `old` (spec § 1): A and B
    /// ENV enter at that level; the rest start where they would, and the
    /// difference glides out.
    fn take_over(&mut self, k: Kind, old: f32, rising: bool, key: bool, sus: f32) {
        let level = old.max(0.0).min(1.0);
        match k {
            Kind::A => self.a.enter(level, rising, key, sus),
            Kind::B(Func::Env(_)) => self.b.enter_env(level, rising),
            Kind::B(Func::Burst(_)) => self.b.enter_burst(key),
            Kind::B(Func::Lfo(_)) => {}
        }
        self.glide.start(old - self.raw());
    }

    /// One block. Returns the output at the block's start. With `vca`, the
    /// slot fills a block of outputs and adds `amount ·` each into the
    /// buffer (the peak ramped, the glide in its own loop); otherwise it
    /// advances in closed form. Only the running TYPE's coefficients are
    /// computed.
    pub fn run_block(
        &mut self,
        p: &EnvParams,
        m: &EnvMods,
        key: bool,
        sample_rate: u32,
        vca: Option<(&mut [f32; BLOCK_SIZE], f32)>,
    ) -> f32 {
        let kind = Kind::of(p);
        let (old, rising) = (self.output(), self.rising());
        let prev = self.kind.replace(kind);
        self.prev_peak = self.peak;
        self.peak = if kind == Kind::A { m.level.unwrap_or(1.0) } else { 1.0 };
        let (ac, bc) = match kind {
            Kind::A => (Some(self.a_coefs(p, m.time, sample_rate)), None),
            Kind::B(_) => (None, Some(self.b_coefs(p, &m.slides, sample_rate, vca.is_some()))),
        };
        if let Some(c) = &bc {
            self.b.set(c);
        }
        if prev.is_some_and(|k| k != kind) {
            self.take_over(kind, old, rising, key, p.sustain);
        }
        if bc.is_some() && !key {
            self.b.key_up();
        }
        let start = self.output();
        match vca {
            Some((gain, amount)) => {
                let mut level = [0.0f32; BLOCK_SIZE];
                if let Some(c) = &ac {
                    self.a.fill(c, key, &mut level);
                    add_ramped(gain, &level, amount, self.prev_peak, self.peak);
                } else if let Some(c) = &bc {
                    self.b.fill(c, key, &mut level);
                    add_ramped(gain, &level, amount, 1.0, 1.0);
                }
                if self.glide.active() {
                    for (n, g) in gain.iter_mut().enumerate() {
                        *g += amount * self.glide.at(n + 1);
                    }
                }
            }
            None => {
                if let Some(c) = &ac {
                    self.a.advance(c, key, BLOCK_SIZE as u32);
                } else if let Some(c) = &bc {
                    self.b.advance(c, key, BLOCK_SIZE as u32);
                }
            }
        }
        self.glide.advance(BLOCK_SIZE as u16);
        start
    }
}
```

Task 5's `a_coefs` moves into this `impl` unchanged. `ACoefs` and `BCoefs` are `Copy`, so the caches return copies.

- [ ] **Step 8: Run the tests**

Run: `env $T cargo test -p chimera-core` (with the `func.rs` unit test)
Expected: PASS; goldens bit-identical (ENV 3 is B now, but nothing routes it). Task 5's `level_sets_the_peak` still builds its `EnvMods` with `..EnvMods::NONE`, so it needs no change.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/modulator chimera-core/src/dsp/envelope.rs chimera-core/src/params.rs \
  chimera-core/tests/func_gen_test.rs chimera-core/tests/env_slot_test.rs
git commit -m "Envelope B: ENV, LFO and BURST; an ENV slot changes TYPE without a jump"
```

---

### Task 7: LFO slots: CLASSIC and FUNC

`ParamSnapshot.lfo` becomes `lfos: [LfoParams; 3]` and `BlockRef::Lfo` becomes `BlockRef::Lfo(LfoSlot)`. `Lfo` runs CLASSIC (today's arithmetic, OFFSET no longer applied) or FUNC (`FuncGen` in LFO mode, DEPTH not applied), and glides across a TYPE or FORM change. `Voice` runs all three, which fills sources 4 and 5, and a note-on retriggers a CLASSIC LFO with SYNC 1 and a FUNC LFO on SYNC. With all six slots running, every matrix row moves its destination: this task ends the first shippable slice (#121).

**Files:**
- Modify: `chimera-core/src/dsp/modulator/mod.rs` (`LfoSlot`, `LfoType`)
- Modify: `chimera-core/src/dsp/lfo.rs` (`LfoParams` fields and ids 6–10, `LFO_SPECS`, `Lfo::run_block`, `Lfo::note_on`, OFFSET dropped)
- Modify: `chimera-core/src/params.rs` (`lfos`, the `Blocks` arms)
- Modify: `chimera-core/src/addr.rs` (`BlockRef::Lfo(LfoSlot)`, `ALL` of 24)
- Modify: `chimera-core/src/modulation.rs` (`ModSource::of_lfo`)
- Modify: `chimera-core/src/preset.rs`, `chimera-core/src/factory.rs`, `chimera-core/src/dsp/voice.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/mod_grid.rs`, `chimera-core/src/ui/block_registry.rs` (LFO page on `Lfo(Lfo1)`, OFST off it)
- Create: `chimera-core/tests/lfo_slot_test.rs`
- Modify tests: `routing_test.rs`, `lfo_test.rs`, `common/mod.rs`, `modulation_integration_test.rs`, `modulatable_test.rs`, `factory_test.rs`, `part_page_test.rs`, `binding_test.rs`, `addr_test.rs`

**Interfaces:**
- Consumes: Task 6's `FuncGen`, `BCoefs`, `Slides`, `Func`, `LfoForm`, `FuncParams::LFO`, `Glide`.
- Produces:
  - `dsp::modulator::LfoSlot { Lfo1, Lfo2, Lfo3 }` (`ALL`, `index()`); `LfoType { Classic = 0, Func = 1 }` (`ALL`).
  - `LfoParams` fields `lfo_type: LfoType`, `func: FuncParams`; ids `TYPE 6, FORM 7, RISE 8, FALL 9, SHAPE 10`.
  - `Lfo::run_block(&mut self, &LfoParams, sample_rate: u32) -> f32` (the block-start value); `Lfo::note_on(&mut self, &LfoParams)`; `Lfo::new` is `const`.
  - `ParamSnapshot.lfos: [LfoParams; 3]`; `BlockRef::Lfo(LfoSlot)`; `ModSource::of_lfo(LfoSlot) -> ModSource`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/lfo_slot_test.rs`:

```rust
//! LFO slots (filter-routing spec § LFO slots).

use chimera_core::dsp::lfo::{Lfo, LfoParams};
use chimera_core::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use chimera_core::dsp::modulator::{FuncParams, LfoType};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

/// Today's LFO, verbatim (with OFFSET at its only value, 0).
struct Reference {
    phase: f32,
    random_value: f32,
    rng_state: u32,
}

impl Reference {
    fn process(&mut self, p: &LfoParams) -> f32 {
        let raw = match p.shape {
            0 => chimera_core::dsp::fast_sin(self.phase * core::f32::consts::TAU),
            1 => {
                let t = self.phase;
                if t < 0.25 { t * 4.0 } else if t < 0.75 { 2.0 - t * 4.0 } else { t * 4.0 - 4.0 }
            }
            2 => self.phase * 2.0 - 1.0,
            3 => if self.phase < 0.5 { 1.0 } else { -1.0 },
            _ => self.random_value,
        };
        let out = (raw * p.depth + 0.0).clamp(-1.0, 1.0);
        self.phase += p.rate / SR as f32 * BLOCK_SIZE as f32;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            if p.shape == 4 {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                self.random_value = (self.rng_state >> 16) as f32 / 32768.0 - 1.0;
            }
        }
        out
    }
}

/// Spec § Tests: LFO 1 CLASSIC is bit-identical to today's LFO over 1,000 blocks.
#[test]
fn classic_is_todays_lfo() {
    for shape in 0..5u8 {
        for (rate, depth) in [(1.0, 1.0), (5.0, 0.7), (19.0, 0.3)] {
            let p = LfoParams { shape, rate, depth, ..LfoParams::default() };
            let mut new = Lfo::new();
            let mut old = Reference { phase: 0.0, random_value: 0.0, rng_state: 12345 };
            for b in 0..1000 {
                assert_eq!(new.run_block(&p, SR), old.process(&p), "shape {shape} rate {rate} block {b}");
            }
        }
    }
}

#[test]
fn offset_is_no_longer_applied() {
    let with = LfoParams { offset: 0.5, ..LfoParams::default() };
    let (mut a, mut b) = (Lfo::new(), Lfo::new());
    for _ in 0..100 {
        assert_eq!(a.run_block(&with, SR), b.run_block(&LfoParams::default(), SR));
    }
}

/// FUNC is Envelope B locked to LFO mode (DEPTH not applied).
#[test]
fn func_matches_b_in_lfo_mode() {
    let p = LfoParams { lfo_type: LfoType::Func, depth: 0.2, func: FuncParams { rise: 0.4, shape: 0.2, ..FuncParams::LFO }, ..LfoParams::default() };
    let c = BCoefs::new(&p.func, &Slides::default(), SR, false);
    let mut lfo = Lfo::new();
    let mut g = FuncGen::new();
    g.set(&c);
    for b in 0..200 {
        let want = g.output();
        g.advance(&c, false, BLOCK_SIZE as u32);
        assert_eq!(lfo.run_block(&p, SR), want, "block {b}");
    }
}

/// A TYPE change glides: no step, and the glide is gone after 256 samples.
#[test]
fn a_type_change_glides_out() {
    let classic = LfoParams { shape: 3, ..LfoParams::default() };
    let func = LfoParams { lfo_type: LfoType::Func, ..LfoParams::default() };
    let mut l = Lfo::new();
    let mut last = 0.0;
    for _ in 0..30 {
        last = l.run_block(&classic, SR);
    }
    // FUNC from its first block: the same generator state as `l`'s.
    let mut fresh = Lfo::new();
    assert_eq!(l.run_block(&func, SR), last, "no step");
    fresh.run_block(&func, SR);
    for b in 1..8 {
        let (x, y) = (l.run_block(&func, SR), fresh.run_block(&func, SR));
        if b >= 4 {
            assert_eq!(x, y, "block {b}: the glide is over");
        }
    }
}
```

Append to `chimera-core/tests/routing_test.rs` (red until this task runs LFO 2 and 3):

```rust
/// Spec § Tests "Matrix": a route from each of the eight sources moves its
/// destination (NOTE at note 72, where it isn't 0).
#[test]
fn every_source_moves_cutoff() {
    for s in ModSource::ALL {
        let note = if s == ModSource::Note { 72 } else { 60 };
        assert_ne!(render(s, 127, note, 100), render(s, 0, note, 100), "{s:?}");
    }
}
```

In `lfo_test.rs`, replace `lfo_offset_shifts_output` with:

```rust
/// Spec § LFO slots: OFFSET is stored but no longer applied.
#[test]
fn lfo_offset_is_not_applied() {
    let (mut a, mut b) = (Lfo::new(), Lfo::new());
    let p = LfoParams { offset: 0.5, depth: 0.5, rate: 5.0, ..Default::default() };
    let q = LfoParams { offset: 0.0, ..p };
    for _ in 0..500 {
        assert_eq!(a.process(&p, SAMPLE_RATE), b.process(&q, SAMPLE_RATE));
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test lfo_slot_test --test lfo_test`
Expected: FAIL to compile: no `LfoType`, `run_block`, `lfo_type`.

- [ ] **Step 3: The slot types**

Append to `chimera-core/src/dsp/modulator/mod.rs`:

```rust
/// An LFO slot of the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoSlot {
    Lfo1,
    Lfo2,
    Lfo3,
}

impl LfoSlot {
    pub const ALL: [LfoSlot; 3] = [LfoSlot::Lfo1, LfoSlot::Lfo2, LfoSlot::Lfo3];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// An LFO slot's TYPE: today's LFO, or Envelope B locked to LFO mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LfoType {
    Classic = 0,
    Func = 1,
}

impl LfoType {
    pub const ALL: [LfoType; 2] = [LfoType::Classic, LfoType::Func];
}
```

- [ ] **Step 4: `LfoParams` and the slot**

In `chimera-core/src/dsp/lfo.rs`:
- `LfoParams` gains `pub lfo_type: LfoType,` and `pub func: FuncParams,` (doc: `/// FUNC's FORM and sliders; its MODE is always LFO.`); `Default` sets `lfo_type: LfoType::Classic, func: FuncParams::LFO`; the `offset` doc becomes `/// Stored, no longer applied (spec § LFO slots).`
- ids: `pub const TYPE: ParamId = ParamId(6); pub const FORM: ParamId = ParamId(7); pub const RISE: ParamId = ParamId(8); pub const FALL: ParamId = ParamId(9); pub const SHAPE_B: ParamId = ParamId(10);` (`SHAPE` is CLASSIC's wave, id 1).
- `LFO_SPECS` becomes `[ParamSpec; 11]` with:

```rust
    ParamSpec::choice(6, "TYPE", ValFmt::Names(&["CLASSIC", "FUNC"]), 1.0, 0.0),
    ParamSpec::choice(7, "FORM", ValFmt::Names(&["FREE", "SYNC", "LFV"]), 2.0, 0.0),
    ParamSpec::continuous(8, "RISE", ValFmt::Uni, 0.0, 1.0, 0.309, 1.0 / 128.0, false),
    ParamSpec::continuous(9, "FALL", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(10, "SHAPE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
```

- `get`/`write` gain `Self::TYPE => self.lfo_type as u8 as f32` / `self.lfo_type = pick(&LfoType::ALL, v)`, `Self::FORM => self.func.form_index()` / `self.func.set_form_index(v)`, and `RISE`, `FALL`, `SHAPE_B` on `self.func.rise`, `.fall`, `.shape`.
- In `process`, replace the two lines under `// Apply depth and offset` with:

```rust
        // Depth; OFFSET is no longer applied (spec § LFO slots).
        self.output = (raw * params.depth).clamp(-1.0, 1.0);
```

- `Lfo` gains fields and `new` becomes `const`:

```rust
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    /// Phase accumulator (0.0 to 1.0)
    phase: f32,
    /// Current output value
    output: f32,
    /// Random/S&H: held value until next cycle
    random_value: f32,
    /// Simple PRNG state for random
    rng_state: u32,
    /// FUNC's generator, and its coefficients with the inputs they came from.
    func: FuncGen,
    bc: Option<(FuncParams, u32, BCoefs)>,
    /// What ran last block; `None` before the first.
    kind: Option<Kind>,
    /// A TYPE or FORM change's leftover, gliding out.
    glide: Glide,
    /// Last block's output, for the glide.
    last: f32,
}

/// What an LFO slot runs: today's LFO, or B locked to LFO mode with a FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Classic,
    Func(LfoForm),
}
```

  (`new` adds `func: FuncGen::new(), bc: None, kind: None, glide: Glide::NONE, last: 0.0`), and:

```rust
    /// A note-on: a CLASSIC LFO with SYNC 1 restarts at its PHASE; FUNC
    /// resets on SYNC; FREE and LFV run on.
    pub fn note_on(&mut self, p: &LfoParams) {
        match p.lfo_type {
            LfoType::Classic if p.sync == 1 => self.retrigger(p.phase_offset),
            LfoType::Classic => {}
            LfoType::Func => self.func.note_on(Func::Lfo(p.func.lfo_form)),
        }
    }

    /// One block (spec § LFO slots): the value at the block's start, then
    /// the advance. CLASSIC is today's arithmetic, bit for bit.
    pub fn run_block(&mut self, p: &LfoParams, sample_rate: u32) -> f32 {
        let kind = match p.lfo_type {
            LfoType::Classic => Kind::Classic,
            LfoType::Func => Kind::Func(p.func.lfo_form),
        };
        let raw = match p.lfo_type {
            LfoType::Classic => self.process(p, sample_rate),
            LfoType::Func => {
                // Reused while FUNC's inputs hold (it runs per block).
                let c = match self.bc {
                    Some((f, sr, c)) if f == p.func && sr == sample_rate => c,
                    _ => {
                        let c = BCoefs::new(&p.func, &Slides::default(), sample_rate, false);
                        self.bc = Some((p.func, sample_rate, c));
                        c
                    }
                };
                self.func.set(&c);
                let v = self.func.output();
                self.func.advance(&c, false, BLOCK_SIZE as u32);
                v
            }
        };
        if self.kind.replace(kind).is_some_and(|k| k != kind) {
            self.glide.start(self.last - raw);
        }
        // No glide: `raw` itself, so CLASSIC stays bit for bit.
        let out = if self.glide.active() { raw + self.glide.value() } else { raw };
        self.glide.advance(BLOCK_SIZE as u16);
        self.last = out;
        out
    }
```

  with `use chimera_hal::BLOCK_SIZE; use crate::dsp::modulator::func::{BCoefs, FuncGen, Slides}; use crate::dsp::modulator::{Func, FuncParams, Glide, LfoForm, LfoType, pick};`. (`FuncParams::LFO`'s mode is LFO and no LFO id writes it, so `BCoefs` runs LFO mode.)

- [ ] **Step 5: Three LFOs everywhere**

- `params.rs`: `pub lfo: crate::dsp::lfo::LfoParams,` becomes `pub lfos: [crate::dsp::lfo::LfoParams; 3],`, its default `[crate::dsp::lfo::LfoParams::default(); 3]`, and both `Blocks` arms `BlockRef::Lfo(s) => &self.lfos[s.index()],` (and `&mut`).
- `addr.rs`: `Lfo,` becomes `/// LFO slot n: lfos[n].\n    Lfo(crate::dsp::modulator::LfoSlot),`; `ALL` becomes `[BlockRef; 24]` with `BlockRef::Lfo(LfoSlot::Lfo1), BlockRef::Lfo(LfoSlot::Lfo2), BlockRef::Lfo(LfoSlot::Lfo3),` in the old place; `specs`: `BlockRef::Lfo(_)`; `voice_reads`: `BlockRef::Lfo(_)` stays `false`.
- `preset.rs`: `| BlockRef::Lfo` becomes `| BlockRef::Lfo(_)` in both lists. `mod_grid.rs`, `block_tag`: `BlockRef::Lfo(s) => ["LF1", "LF2", "LF3"][s.index()],`.
- `modulation.rs`: `pub const fn of_lfo(s: crate::dsp::modulator::LfoSlot) -> Self { [ModSource::Lfo1, ModSource::Lfo2, ModSource::Lfo3][s.index()] }`.
- `block_registry.rs`, `LFO`: every `BlockRef::Lfo` becomes `BlockRef::Lfo(LfoSlot::Lfo1)`, and slot f (OFST) becomes `EMPTY`.
- `factory.rs`: `s.params.lfo.` becomes `s.params.lfos[0].` (two places).
- `voice.rs`: `pub lfo: Lfo,` becomes `lfos: [Lfo; 3],` (init `lfos: [Lfo::new(); 3],`); in `trigger`, after the envelopes, `for (l, p) in self.lfos.iter_mut().zip(&params.lfos) { l.note_on(p); }`; in `render`, replace the LFO 1 line with

```rust
        for (s, lfo) in LfoSlot::ALL.iter().zip(self.lfos.iter_mut()) {
            mod_values[ModSource::of_lfo(*s).index()] =
                lfo.run_block(&src.lfos[s.index()], sample_rate);
        }
```

- `ui/mod.rs`: `display_lfo: Lfo` becomes `display_lfos: [Lfo; 3]` (struct, `field_list!`, `init_in_place` writes `[Lfo::new(); 3]`), and in `update` the source-1 line reads `self.display_lfos[0].run_block(&sound.params.lfos[0], chimera_hal::BLOCK_SIZE as u32 * UI_FPS)` (Task 9 fills the other stand-ins).
- Tests: `sed -i 's/\bparams\.lfo\./params.lfos[0]./g; s/\bp\.lfo\./p.lfos[0]./g; s/\bs\.params\.lfo\./s.params.lfos[0]./g; s/&s\.params\.lfo,/\&s.params.lfos[0],/g' chimera-core/tests/{common/mod.rs,modulation_integration_test.rs,modulatable_test.rs,factory_test.rs,part_page_test.rs}`. In `part_page_test.rs`, delete the two OFFSET lines (`turn(&reg::LFO, 5, 3, …)` and its assert). In `binding_test.rs`, the LFO row's last entry becomes `("--", Uni)`. `addr_test.rs` needs no change (it iterates `ALL`).

- [ ] **Step 6: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS, `every_source_moves_cutoff` included. `algo_morph_sweep` and `factory_6` (MORPH PAD, LFO 1 → MORPH) are bit-identical: CLASSIC's value is still taken before the advance, with today's arithmetic.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/modulator/mod.rs chimera-core/src/dsp/lfo.rs chimera-core/src/params.rs \
  chimera-core/src/addr.rs chimera-core/src/modulation.rs chimera-core/src/preset.rs chimera-core/src/factory.rs \
  chimera-core/src/dsp/voice.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/mod_grid.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/tests/lfo_slot_test.rs chimera-core/tests/lfo_test.rs \
  chimera-core/tests/common/mod.rs chimera-core/tests/modulation_integration_test.rs \
  chimera-core/tests/modulatable_test.rs chimera-core/tests/factory_test.rs chimera-core/tests/part_page_test.rs \
  chimera-core/tests/binding_test.rs chimera-core/tests/routing_test.rs
git commit -m "Three LFO slots, CLASSIC or FUNC; OFFSET no longer applied"
```

- [ ] **Step 8: Mark the slice**

Tasks 1–7 are the first shippable slice: every FLT knob is honest and every matrix row runs.

```bash
GH_TOKEN=$(gh auth token -u joegiralt) gh issue comment 121 -R joegiralt/chimera \
  --body "First slice on filter-routing (Tasks 1–7 of docs/superpowers/plans/2026-09-28-filter-routing.md): MODE on FLT and FLT › MODE, FM retired (ids 3–5), ENV / KEY / LFO are the routes ENV 1 / NOTE / LFO 1 → CUTOFF, CUTOFF in octaves, g ramped per block, and the six modulators (ENV 1–3 as Cascadia A or B, LFO 1–3 CLASSIC or FUNC) feed the eight matrix rows. Factory Sounds bit-identical."
```

---

### Task 8: Route presence, the default routes, MIX+MINUS and the "—" knob

A route exists apart from its amount (spec § 2), so a route at 0 and a deleted route differ. This task gives the matrix a presence bit per cell and gives every new Sound its three CUTOFF routes at 0. MIX+MINUS on a matrix cell deletes a route. A route knob whose route is absent shows a dash until it is turned.

**Files:**
- Modify: `chimera-core/src/modulation.rs` (`present`, `present()`, `find`, `routes_into`, `set_route`, `push`, `set_amount`, `sync_from_matrix`; `CUTOFF`)
- Modify: `chimera-core/src/preset.rs` (`Sound::init` defaults)
- Modify: `chimera-core/src/factory.rs` (MORPH PAD builds on the defaults)
- Modify: `chimera-core/src/ui/mod_grid.rs` (`MAX_SOURCES`, `present`, `set`, `route`, `is_present`, `delete_selected`, `load_amounts`, `draw_grid`, `HINT`)
- Modify: `chimera-core/src/ui/components.rs` (`Look` on `Cell`)
- Modify: `chimera-core/src/ui/renderer.rs` (`look`: route cells and the focus band read presence), `chimera-core/src/ui/audio_page.rs` (`look: Look::Live`)
- Modify: `chimera-core/src/ui/region.rs` (`matrix_rev` and `looks` in the Grid, Route and Cells keys; `keyed`)
- Modify: `chimera-core/src/ui/mod.rs` (MIX+MINUS on the matrix; `CUTOFF` from `modulation`)
- Modify tests: `routing_test.rs`, `flt_page_test.rs`, `binding_test.rs`, `factory_test.rs`, `mixer_page_test.rs`, `preset_test.rs`, `prime_status_test.rs`, `matrix_view_test.rs`, `cell_grid_test.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: `ModSource` (Task 4).
- Produces:
  - `modulation::CUTOFF: ParamAddr`.
  - `ModState::{present(&self, d: usize) -> u8, find(&self, ParamAddr) -> Option<usize>, routes_into(&self, ParamAddr) -> u8, set_route(&mut self, source: usize, dest: usize, amount: i8), push(&mut self, ParamAddr) -> Option<usize>}`; `set_amount` sets the bit when the amount is nonzero.
  - `MatrixState::{present: [u8; MAX_DESTS], is_present(&self, row, col) -> bool, delete_selected(&mut self)}`; `set` marks presence; `route` is `None` for an absent route.
  - `mod_grid::HINT: &str`; `mod_grid::MAX_SOURCES == MAX_MOD_SOURCES`.
  - `components::Look { Live = 0, Absent = 1, Dimmed = 2 }` (`#[repr(u8)]`) and `Cell.look`; `renderer::look(f: &Frame, i: usize) -> Look`, the one place a cell's look is decided (Tasks 14–17 add their rules to it).
  - `MatrixState.rev: u16`, bumped on every amount, presence or column change; `RegionData::keyed(self, matrix_rev: u16, looks: u16) -> RegionData`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/routing_test.rs`:

```rust
use chimera_core::modulation::{CUTOFF as CUTOFF_ADDR, MAX_MOD_SOURCES};
use chimera_core::preset::Sound;
use chimera_core::ui::mod_grid::MatrixState;

const DEFAULT_BITS: u8 = 1 << 0 | 1 << 1 | 1 << 7; // ENV1, LFO1, NOTE

#[test]
fn presence_is_apart_from_the_amount() {
    let mut reg = ModDestRegistry::new();
    reg.add(CUTOFF_ADDR, *b"FLTCUTOF").unwrap();
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    assert_eq!(ms.routes_into(CUTOFF_ADDR), 0);
    ms.set_route(ModSource::Note.index(), 0, 0);
    assert_eq!(ms.routes_into(CUTOFF_ADDR), 1 << 7, "a route at 0 exists");
    ms.set_amount(ModSource::Lfo1.index(), 0, 9);
    assert_eq!(ms.routes_into(CUTOFF_ADDR), 1 << 7 | 1 << 1, "set_amount creates");
    let values = [1.0f32; MAX_MOD_SOURCES];
    ms.set_amount(ModSource::Lfo1.index(), 0, 0);
    assert_eq!(ms.sum_for(0, &values), 0.0, "a present route at 0 adds nothing");
}

#[test]
fn presence_survives_sync_from_matrix() {
    let sound = Sound::init(EngineType::Algo);
    let mut m = MatrixState::new();
    m.rebuild_sources(&PART_MOD_SOURCES);
    m.rebuild_dests_from_registry(&sound.dest_registry);
    m.load_amounts(&sound.mod_state);
    assert_eq!(m.present[0], DEFAULT_BITS);
    let mut ms = ModState::new();
    ms.sync_from_matrix(&m);
    assert_eq!(ms.present(0), DEFAULT_BITS);
}

/// Spec § Tests "Defaults": exactly the three CUTOFF routes at 0.
#[test]
fn a_new_sound_has_the_default_routes() {
    for ct in EngineType::ALL {
        let s = Sound::init(ct);
        assert_eq!(s.dest_registry.len(), 1, "{ct:?}");
        assert_eq!(s.dest_registry.get(0).unwrap().addr, CUTOFF_ADDR);
        assert_eq!(s.mod_state.num_sources(), MAX_MOD_SOURCES);
        assert_eq!(s.mod_state.num_dests(), 1);
        assert_eq!(s.mod_state.present(0), DEFAULT_BITS, "{ct:?}");
        assert!((0..MAX_MOD_SOURCES).all(|src| s.mod_state.amount(src, 0) == 0));
    }
}
```

Append to `chimera-core/tests/flt_page_test.rs`:

```rust
/// Plus five times from Part 1's home: the MOD node (the matrix, until
/// Task 16 puts E1 there).
fn to_matrix(ui: &mut UiState) {
    feed(ui, Input::press(ButtonId::B1));
    for _ in 0..5 {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// MIX+MINUS on a matrix cell deletes its route; the ENV knob shows a dash
/// until it is turned, and turning it creates the route.
#[test]
fn mix_minus_deletes_and_the_knob_recreates() {
    let mut ui = on_flt(EngineType::Algo);
    to_matrix(&mut ui); // cursor on E1 → FLT CUTOFF
    assert_eq!(ui.matrix_state.route(0, CUTOFF), Some(0));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.matrix_state.route(0, CUTOFF), None, "deleted");
    assert_eq!(ui.mod_state().present(0) & 1, 0);
    feed(&mut ui, Input::press(ButtonId::B1));
    for _ in 0..flt_node(EngineType::Algo) {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::turn(EncoderId::E, 3)); // ENV
    assert_eq!(ui.matrix_state.route(0, CUTOFF), Some(3), "created at 0, then turned");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test routing_test --test flt_page_test`
Expected: FAIL to compile: no `set_route`, `routes_into`, `present`, `CUTOFF` in `modulation`.

- [ ] **Step 3: Presence in `ModState`**

In `chimera-core/src/modulation.rs`:

```rust
use crate::params::FilterParams;

/// CUTOFF: the default routes' column, and the filter knobs' (spec § 2, § 6).
pub const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
```

Add the field `present: [u8; MAX_MOD_DESTS],` to `ModState` (doc: `/// Route presence (spec § 2): bit s of present[d] is set when source s routes to dest d, at any amount. The audio thread's sums ignore it.`), `present: [0; MAX_MOD_DESTS],` in `new`, and to `impl ModState`:

```rust
    /// The present bits of destination `d` (0 out of range).
    pub fn present(&self, d: usize) -> u8 {
        if d < self.num_dests { self.present[d] } else { 0 }
    }

    /// The column of `addr`, if any.
    pub fn find(&self, addr: ParamAddr) -> Option<usize> {
        (0..self.num_dests).find(|&d| self.dests[d] == addr)
    }

    /// The present bits of the column of `addr` (0 without a column).
    pub fn routes_into(&self, addr: ParamAddr) -> u8 {
        self.find(addr).map_or(0, |d| self.present[d])
    }

    /// Create (or set) the route `source → dest` at `amount`, 0 included.
    pub fn set_route(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
            self.present[dest] |= 1 << source;
        }
    }

    /// Append `addr` as a column if it is modulatable and there is room.
    pub fn push(&mut self, addr: ParamAddr) -> Option<usize> {
        self.push_dest(Some(addr)).then(|| self.num_dests - 1)
    }
```

`set_amount` becomes:

```rust
    /// Set one amount; a nonzero amount creates the route. Ignored when
    /// `source` or `dest` is out of range.
    pub fn set_amount(&mut self, source: usize, dest: usize, amount: i8) {
        if source < self.num_sources && dest < self.num_dests {
            self.amounts[source][dest] = amount;
            if amount != 0 {
                self.present[dest] |= 1 << source;
            }
        }
    }
```

`offset_for` uses `self.find(addr)`; in `sync_from_matrix`, after the amounts loop, `self.present[d] = matrix.present[di];`.

- [ ] **Step 4: The defaults**

In `chimera-core/src/preset.rs`, `Sound::init`:

```rust
    pub fn init(chain_type: EngineType) -> Self {
        let mut name = [0u8; NAME_LEN];
        let tag = b"(init)";
        name[..tag.len()].copy_from_slice(tag);
        // The default routes (spec § 2): ENV 1, LFO 1 and NOTE → CUTOFF at
        // 0 (NOTE at the SVF's key default, 0), on every engine.
        let mut dest_registry = ModDestRegistry::new();
        let _ = dest_registry.add(CUTOFF, *b"FLTCUTOF"); // an empty registry takes it
        let mut mod_state = ModState::from_registry(&dest_registry, MAX_MOD_SOURCES);
        for s in [ModSource::Env1, ModSource::Lfo1, ModSource::Note] {
            mod_state.set_route(s.index(), 0, 0);
        }
        Self {
            name,
            chain_type,
            params: ParamSnapshot::for_engine(chain_type.engine()),
            mod_state,
            dest_registry,
        }
    }
```

with `use crate::modulation::{CUTOFF, MAX_MOD_SOURCES, ModSource, ModState};`.

In `chimera-core/src/factory.rs`, MORPH PAD's matrix becomes:

```rust
            let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
            if s.dest_registry.add(morph, *b"ALGMORPH").is_ok()
                && let Some(d) = s.mod_state.push(morph)
            {
                // Full LFO swing (± `depth`) must land inside MORPH's 0..=127
                // range from its base, or the sweep clips flat at an end.
                let headroom = MORPH_BASE.min(127 - MORPH_BASE) as f32;
                let amount = (headroom / s.params.lfos[0].depth) as i8;
                s.mod_state.set_amount(ModSource::Lfo1.index(), d, amount); // LFO 1 → MORPH
            }
```

(and drop the now-unused `ModDestRegistry`/`ModState` imports). `factory_6` stays bit-identical: the CUTOFF column adds 0, and MORPH's sum sees the same one nonzero amount in the same order.

- [ ] **Step 5: Presence in the matrix view**

In `chimera-core/src/ui/mod_grid.rs`:
- `pub const MAX_SOURCES: usize = crate::modulation::MAX_MOD_SOURCES;` (presence is a `u8`);
- `MatrixState` gains `pub present: [u8; MAX_DESTS],` (doc: `/// Route presence, one bit per source, as ModState's.`), `present: [0; MAX_DESTS]` in `new`;
- `load_amounts` clears `self.present = [0; MAX_DESTS];` with the amounts and, per matched column, sets `self.present[di] = mod_state.present(d);`;
- `set` marks the route: after the amount, `self.present[col] |= 1 << row;`;
- `route` returns the amount only when present:

```rust
    /// The amount of route `row → addr`; `None` when the route is absent.
    pub fn route(&self, row: usize, addr: ParamAddr) -> Option<i8> {
        self.col_of(addr)
            .filter(|&c| self.is_present(row, c))
            .map(|c| self.amounts[row][c])
    }

    pub fn is_present(&self, row: usize, col: usize) -> bool {
        col < self.num_dests && self.present[col] & (1 << row) != 0
    }

    /// MIX+MINUS: delete the route under the cursor.
    pub fn delete_selected(&mut self) {
        if self.sel_col < self.num_dests && self.sel_row < self.num_sources {
            self.amounts[self.sel_row][self.sel_col] = 0;
            self.present[self.sel_col] &= !(1 << self.sel_row);
        }
    }
```

- `HINT`, and `draw_grid` draws presence:

```rust
/// The matrix's hint line (spec § UI: MIX+MINUS deletes a route).
pub const HINT: &str = "PRIME MIX+PLUS  DELETE MIX+MINUS";
```

  In `draw_grid`, the cell's `match amount { … }` becomes:

```rust
            match (state.is_present(ri, di), amount) {
                (false, _) => draw::dot(d, x, y, 1, theme::FAINT),
                (true, 0) => draw::ring(d, x, y, 2, color, 1),
                (true, a) if a > 0 => draw::dot(d, x, y, r, color),
                (true, _) => draw::ring(d, x, y, r, color, 1),
            }
```

  the hint text becomes `HINT`, and the route count filters `state.is_present(r, c)`.

- [ ] **Step 6: Absent cells**

In `chimera-core/src/ui/components.rs`:

```rust
/// How a cell reads (spec § UI). The discriminants pack into the Cells
/// region's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Look {
    Live = 0,
    /// A route knob with no route: a dash where the value goes, no bar.
    Absent = 1,
    /// Fixed or inapplicable: label and value in MID, no bar.
    Dimmed = 2,
}
```

`Cell` gains `pub look: Look,`. In `cell`, when `c.look == Look::Absent`, draw the label as usual, then instead of the value text and bar draw `draw::fill_rect(d, x, y + theme::CELL_VALUE_DY - 5, 12, 2, theme::INK2);` and return. (`Dimmed` is drawn from Task 14.) Every `Cell { … }` literal gains `look: Look::Live` (`renderer.rs`, `audio_page.rs`, `cell_grid_test.rs`).

In `chimera-core/src/ui/renderer.rs`, one function decides a cell's look, for drawing and for the dirty-region key:

```rust
/// How cell `i` of the page reads now.
pub fn look(f: &Frame, i: usize) -> components::Look {
    match f.def.params[i].binding {
        SlotBinding::Route(src)
            if f.matrix.route(src.index(), crate::modulation::CUTOFF).is_none() =>
        {
            components::Look::Absent
        }
        _ => components::Look::Live,
    }
}
```

and `draw_cells` sets `look: look(f, i)` on each `Cell`.

and `draw_focus` shows `--` as the value text for an absent route (the focus font has `-`):

```rust
        let absent = matches!(slot.binding, SlotBinding::Route(src)
            if f.matrix.route(src.index(), crate::modulation::CUTOFF).is_none());
        let mut buf = FmtBuf::new();
        if absent {
            let _ = core::fmt::Write::write_str(&mut buf, "--");
        } else {
            fmt::fmt_val(&mut buf, v, slot.format());
        }
```

- [ ] **Step 6b: The dirty-region keys see presence**

Deleting a route at 0 changes no amount, so today's Grid, Route and Cells keys wouldn't redraw (the goldens render in full and wouldn't notice). In `mod_grid.rs`, `MatrixState` gains `pub rev: u16,` (doc: `/// Bumped on every amount, presence or column change: part of the dirty-region keys.`), `rev: 0` in `new`, and `self.rev = self.rev.wrapping_add(1);` at the end of `set`, `delete_selected`, `load_amounts` and `rebuild_dests_from_registry`.

In `region.rs`, `Cells` gains `matrix_rev: u16, looks: u16`, and `Grid` and `Route` gain `matrix_rev: u16`. `cells`, `grid` and `grid_with_value` set them to 0, and the sentinels to `u16::MAX`. Add:

```rust
    /// With the matrix's revision (and the cells' looks) in the key, so a
    /// deleted route or a dimmed cell redraws.
    pub fn keyed(self, matrix_rev: u16, looks: u16) -> Self {
        match self {
            Self::Cells { page, values, focus, dest_count, .. } => {
                Self::Cells { page, values, focus, dest_count, matrix_rev, looks }
            }
            Self::Grid { sel_row, sel_col, scroll_x, scroll_y, sel_value, .. } => {
                Self::Grid { sel_row, sel_col, scroll_x, scroll_y, sel_value, matrix_rev }
            }
            Self::Route { row, col, dests, value, .. } => Self::Route { row, col, dests, value, matrix_rev },
            other => other,
        }
    }
```

In `ui/mod.rs`, `region_data` keys them: the Route literal gains `matrix_rev: self.matrix_state.rev`; the Cells arm ends `.keyed(self.matrix_state.rev, looks)` with

```rust
        let looks = (0..6).fold(0u16, |k, i| k | (renderer::look(f, i) as u16) << (2 * i));
```

and the Grid arm ends `.keyed(self.matrix_state.rev, 0)`. `region_tests.rs` builds `RegionData::cells(…)` as before; no change.

Append to `flt_page_test.rs` (after `to_matrix`):

```rust
/// Deleting a route at 0 changes no amount, but the dirty render redraws
/// the grid (the desktop and the firmware draw through `render_dirty`).
#[test]
fn deleting_a_route_at_zero_redraws_the_grid() {
    use chimera_core::ui::page::PageLayout;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionKind, layout_regions};
    let mut ui = UiState::new();
    to_matrix(&mut ui); // E1 → CUTOFF, present at 0
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    let &(_, y0, y1) = layout_regions(PageLayout::Matrix)
        .iter()
        .find(|r| r.0 == RegionKind::Grid)
        .unwrap();
    assert!(flushed.contains(&(y0, y1)), "{flushed:?}");
}
```

- [ ] **Step 7: MIX+MINUS on the matrix**

In `chimera-core/src/ui/mod.rs`, delete the local `CUTOFF` const and `use crate::modulation::CUTOFF;`. In `handle_input`'s matrix branch, after the encoder loop:

```rust
            // MIX + Minus deletes the route under the cursor (spec § 2).
            if shift && controls.button_state(ButtonId::Minus) == ButtonState::Pressed {
                self.matrix_state.delete_selected();
                self.sync_mod_state(self.active_part);
            }
```

(`ChainNav` reads MINUS only without MIX, so this never also moves the page.)

- [ ] **Step 8: Tests that assumed an empty matrix**

- `binding_test.rs`, `part_chains_offer_the_eight_sources`: the two "no pre-wired destinations" asserts become `assert_eq!(sound.dest_registry.len(), 1, "{ct:?}: the default CUTOFF column"); assert_eq!(sound.mod_state.num_dests(), 1, "{ct:?}");`.
- `factory_test.rs`: `assert_eq!(pad.mod_state.num_dests(), 2);` and `assert_ne!(pad.mod_state.amount(1, 1), 0, "LFO 1 moves MORPH");`; in `morph_pad_lfo_sweep_stays_inside_morph_range`, `sum_for(0, …)` becomes `sum_for(1, …)`.
- `mixer_page_test.rs:199`: `assert_eq!(ui.performance.parts[0].sound.dest_registry.len(), 1, "only the default CUTOFF");`.
- `preset_test.rs`: the two `assert!(primed(&ui).is_empty());` become `assert_eq!(primed(&ui), [chimera_core::modulation::CUTOFF]);`.
- `prime_status_test.rs`, `priming_past_matrix_capacity_on_the_algo_chain_reports_full`: CUTOFF is already a column, so priming it reports ALREADY ROUTED. The walk (Task 2's, through FLT › MODE) still meets 16 other addresses: 15 are ADDED, filling the registry, and the 16th reports MATRIX FULL. So `assert_eq!(added, MAX_MOD_DESTS - 1, …)` and `Some(MAX_MOD_DESTS - 1)` for `added_before`, and the matrix and registry still hold `MAX_MOD_DESTS`. The doc says "exactly `MAX_MOD_DESTS − 1` report ADDED (CUTOFF is a default column), then MATRIX FULL".
- `matrix_view_test.rs`, `an_empty_matrix_says_so`: after `UiState::new()`, empty Part 1's matrix first: `ui.performance.parts[0].sound.dest_registry.remove(chimera_core::modulation::CUTOFF); feed(&mut ui, Input::press(ButtonId::B1));`, then the five PLUS.
- `prime_status_test.rs:107` measures the old hint: measure `chimera_core::ui::mod_grid::HINT` instead, and assert it fits `theme::SCREEN_W - 2 * theme::MARGIN_X`.

- `ui_routing_test.rs` (`use chimera_core::modulation::CUTOFF;`). The default CUTOFF column is column 0 in every new Sound, so:
  - `algo_matrix_rows_are_env_and_lfo`: `ui.matrix_state.num_dests == 1`.
  - `priming_on_a_part_page_registers_its_address`: `primed(&ui) == [CUTOFF, ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE)]`, the label check reads `reg.get(1)` (`"DRVDRIVE"`), and `num_dests() == 2`.
  - `switching_part_rebuilds_the_matrix_for_that_part`: `set_first_amount` now sets E1 → CUTOFF (column 0). Part 1 reads `[(CUTOFF, 10), (drive, 0)]`. Part 2 starts at `num_dests == 1` ("only its default CUTOFF column"), and its primed address is `dest_registry.get(1)`. After its `set_first_amount(20)`, Part 2 reads `[(CUTOFF, 20), (p2, 0)]` and Part 1 still reads `[(CUTOFF, 10), (drive, 0)]`. Back on Part 1, `(num_dests, amounts[0][0]) == (2, 10)`. After `set_first_amount(1)`, Part 1 reads `[(CUTOFF, 11), (drive, 0)]` and Part 2 `[(CUTOFF, 20), (p2, 0)]`.
  - `priming_after_a_stale_cursor_does_not_inherit_a_phantom_amount` is rewritten, since the old column 2 is no longer past Part 2's end. Part 1 now has four columns (CUTOFF, DRIVE, TONE, MIX). The cursor turn becomes `encoder(EncoderId::B, 3)` with `sel_col == 3`. On Part 2, after priming DRIVE, `num_dests == 2` (CUTOFF, DRIVE) and the cursor clamps to its last column, `sel_col == 1`, with the same message. The E turn then edits Part 2's DRIVE at column 1. After priming MIX, `num_dests == 3`, and `routes(&ui, 1) == [(CUTOFF, 0), (drive, 50), (mix, 0)]`, where the E turn edited Part 2's own DRIVE, not a phantom column. The doc comment's column numbers follow.
  - `un_priming_keeps_the_other_routes_own_amounts`: DRIVE, TONE and MIX are columns 1, 2 and 3, and `num_dests == 3` becomes 4. In the matrix, each amount follows a `B` turn: `B 1` (column 1), `E 10`; `B 1`, `E 20`; `B 1`, `E 30`, with the comments renumbered. Then `routes(&ui, 0).len() == 4`. After un-priming TONE, `num_dests == 3` and `routes(&ui, 0) == [(CUTOFF, 0), (drive, 10), (mix, 30)]`.

These are all the tests the default column turns red; any other failure at Step 9 is a bug or a gap in this list, and is fixed at its cause, not by editing the test.

- [ ] **Step 9: Run the tests**

Run: `env $T cargo test -p chimera-core` (the whole crate: presence touches the UI broadly).
Expected: PASS except `screen_goldens_match` (next step). The audio goldens and all eight factory goldens are bit-identical.

- [ ] **Step 10: Look at the screens and re-record**

Look at `bigviz_filter`: CUTOFF's cell has its mod indicator (a column exists), ENV and KEY read `0%` (present at 0). Look at `mod_matrix`: row E1 has a filled dot under CUTOFF (+20), LF1's is selected (+42), E2 shows a faint dot (absent), and the hint reads `PRIME MIX+PLUS  DELETE MIX+MINUS`, fully on screen. Re-record those two.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/modulation.rs chimera-core/src/preset.rs chimera-core/src/factory.rs \
  chimera-core/src/ui/mod_grid.rs chimera-core/src/ui/components.rs chimera-core/src/ui/renderer.rs \
  chimera-core/src/ui/region.rs chimera-core/src/ui/audio_page.rs chimera-core/src/ui/mod.rs chimera-core/tests/routing_test.rs \
  chimera-core/tests/flt_page_test.rs chimera-core/tests/binding_test.rs chimera-core/tests/factory_test.rs \
  chimera-core/tests/mixer_page_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/prime_status_test.rs \
  chimera-core/tests/matrix_view_test.rs chimera-core/tests/cell_grid_test.rs \
  chimera-core/tests/screen_golden_test.rs
git commit -m "Route presence; a new Sound's three CUTOFF routes; MIX+MINUS deletes a route"
```

---

### Task 9: ENV destinations; the UI's stand-in sources

ENV n LEVEL, TIME, RISE, FALL and SHAPE become modulatable. `Voice` collects their sums in the matrix pass and feeds them to their slot on the next block (spec § Signal flow 1). The hidden LEVEL and TIME get primed from the cells that own them (see Decisions). The UI's stand-in source values cover all eight sources.

**Files:**
- Modify: `chimera-core/src/params.rs` (`ENV_SPECS`: LEVEL, TIME, RISE, FALL, SHAPE modulatable)
- Modify: `chimera-core/src/addr.rs` (`voice_reads(Env(_))` is true)
- Modify: `chimera-core/src/dsp/voice.rs` (`env_mods`, the matrix pass)
- Modify: `chimera-core/src/ui/mod.rs` (`prime_target`, `mod_label` for ENV, the stand-ins)
- Modify tests: `routing_test.rs`, `prime_status_test.rs`, `mod_registry_test.rs`

**Interfaces:**
- Consumes: `EnvMods` (Task 5), presence (Task 8).
- Produces: `ParamAddr::new(BlockRef::Env(s), EnvParams::{LEVEL, TIME, RISE, FALL, SHAPE}).modulatable() == true`; `ui::prime_target(ParamAddr) -> ParamAddr` (crate-private: A, D, R, H → TIME; S → LEVEL; everything else itself).

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/routing_test.rs`:

```rust
use chimera_core::dsp::modulator::EnvSlot;
use chimera_core::params::EnvParams;

fn env(s: EnvSlot, id: chimera_core::block::ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::Env(s), id)
}

/// 24 blocks at note 72 under `routes` (source, destination, amount).
fn render_with(p: &ParamSnapshot, routes: &[(ModSource, ParamAddr, i8)], vel: u8) -> Vec<f32> {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        let _ = reg.add(a, *b"TEST\0\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amt) in routes {
        let d = ms.find(a).unwrap();
        ms.set_route(s.index(), d, amt);
    }
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(72).unwrap(), Velocity::new(vel).unwrap(), p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..24 {
        v.render(&mut b, p, &ms);
        out.extend_from_slice(&b);
    }
    out
}

fn plain() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.cutoff = 1000.0;
    p
}

#[test]
fn env_destinations_are_modulatable() {
    for s in EnvSlot::ALL {
        for id in [EnvParams::LEVEL, EnvParams::TIME, EnvParams::RISE, EnvParams::FALL, EnvParams::SHAPE] {
            assert!(env(s, id).modulatable(), "{s:?} {id:?}");
        }
        for id in [EnvParams::ATTACK, EnvParams::SUSTAIN, EnvParams::TYPE, EnvParams::HOLD] {
            assert!(!env(s, id).modulatable(), "{s:?} {id:?}");
        }
    }
}

/// VEL → ENV 1 LEVEL at 100 %: ENV 1's peak is the velocity. A LEVEL
/// route at 0 makes the peak 0, so ENV 1 → CUTOFF then does nothing.
#[test]
fn vel_to_level_scales_env1() {
    let e1_cut = (ModSource::Env1, CUTOFF, 127);
    let level = |amt| (ModSource::Vel, env(EnvSlot::Env1, EnvParams::LEVEL), amt);
    // Against the same velocity without the LEVEL route, so the engine's
    // own velocity response cancels: at full velocity the peak is exactly 1
    // (1.0 × 127/127), a soft note lowers it.
    let no_level = |vel| render_with(&plain(), &[e1_cut], vel);
    assert_eq!(render_with(&plain(), &[e1_cut, level(127)], 127), no_level(127), "full velocity: peak 1");
    assert_ne!(render_with(&plain(), &[e1_cut, level(127)], 30), no_level(30), "a soft note: a lower peak");
    assert_eq!(
        render_with(&plain(), &[e1_cut, level(0)], 100),
        render_with(&plain(), &[(ModSource::Env1, CUTOFF, 0), level(0)], 100)
    );
}

/// TIME acts on A; RISE acts on B and not on A.
#[test]
fn time_and_rise_reach_their_slots() {
    let time = (ModSource::Vel, env(EnvSlot::Env1, EnvParams::TIME), 127);
    let e1 = (ModSource::Env1, CUTOFF, 127);
    assert_ne!(render_with(&plain(), &[e1, time], 100), render_with(&plain(), &[e1], 100));
    let e3 = (ModSource::Env3, CUTOFF, 127); // ENV 3 is B (ENV, AD)
    let rise3 = (ModSource::Vel, env(EnvSlot::Env3, EnvParams::RISE), 127);
    assert_ne!(render_with(&plain(), &[e3, rise3], 100), render_with(&plain(), &[e3], 100));
    let rise1 = (ModSource::Vel, env(EnvSlot::Env1, EnvParams::RISE), 127);
    assert_eq!(render_with(&plain(), &[e1, rise1], 100), render_with(&plain(), &[e1], 100));
}
```

Append to `chimera-core/tests/prime_status_test.rs`:

```rust
/// The hidden LEVEL and TIME are primed from the cells that own them: A,
/// D, R and H prime TIME; S primes LEVEL (Decisions table).
#[test]
fn stage_cells_prime_time_and_sustain_primes_level() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::params::EnvParams;
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // MOD node
    }
    feed(&mut ui, Input::press(ButtonId::Edit)); // the ENV 1 page
    prime(&mut ui, EncoderId::A); // ATTACK
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
    prime(&mut ui, EncoderId::C); // SUSTAIN
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
    // Whichever ENV slot the page shows (E1 here; E2 once Task 16 moves E1 home).
    let reg = &ui.performance.parts[0].sound.dest_registry;
    let primed: Vec<ParamAddr> = (0..reg.len()).map(|i| reg.get(i).unwrap().addr).collect();
    let has = |id| primed.iter().any(|a| matches!(a.block, BlockRef::Env(_)) && a.param == id);
    assert!(has(EnvParams::TIME) && has(EnvParams::LEVEL), "{primed:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test routing_test --test prime_status_test`
Expected: FAIL: `env_destinations_are_modulatable`, `vel_to_level_scales_env1`, `time_and_rise_reach_their_slots` and the priming test.

- [ ] **Step 3: Open the destinations**

In `params.rs`, set the last argument (modulatable) to `true` for ENV_SPECS' LEVEL (4), TIME (10), RISE (13), FALL (14) and SHAPE (15). In `addr.rs`, `voice_reads` moves `BlockRef::Env(_)` to the `true` arm; its doc says `ENV slots read their LEVEL, TIME, RISE, FALL and SHAPE sums per block`.

In `mod_registry_test.rs`, `registry_accepts_exactly_the_modulatable_addresses` iterates the specs, so it follows; check `registry_refuses_non_modulatable` still names only never-modulatable ENV ids (ATTACK, SUSTAIN).

- [ ] **Step 4: `Voice` feeds the slots**

In `voice.rs`, add the field `env_mods: [EnvMods; 3],` (doc: `/// The ENV destinations' sums from the last block's matrix (spec § Signal flow 1).`), `env_mods: [EnvMods::NONE; 3],` in `init_chain`, and in `render`:
- the ENV loop passes `&self.env_mods[s.index()]` instead of `&EnvMods::NONE`;
- the matrix pass (inside `if self.fade == 0`) becomes:

```rust
            let mut next = [EnvMods::NONE; 3];
            for d in 0..mod_state.num_dests() {
                let a = mod_state.dest(d);
                if let BlockRef::Env(s) = a.block {
                    // ENV destinations reach their slot next block.
                    let sum = mod_state.sum_for(d, &mod_values);
                    let n = &mut next[s.index()];
                    match a.param {
                        EnvParams::LEVEL if mod_state.present(d) != 0 => {
                            n.level = Some(sum.max(0.0).min(1.0));
                        }
                        EnvParams::TIME => n.time = sum,
                        EnvParams::RISE => n.slides.rise = sum,
                        EnvParams::FALL => n.slides.fall = sum,
                        EnvParams::SHAPE => n.slides.shape = sum,
                        _ => {}
                    }
                    continue;
                }
                let off = mod_state.sum_for(d, &mod_values);
                if off != 0.0 {
                    if live.offset(a, off) {
                        continue;
                    }
                    // Modulatable addresses are always Sound blocks (`voice_reads`).
                    if let Some(blk) = m.block_mut(a.block) {
                        apply_offset(blk, a.param, off);
                    }
                }
            }
            self.env_mods = next;
```

(imports: `use crate::addr::{BlockRef, Blocks};`, `use crate::params::{EngineType, EnvParams, ParamSnapshot};`).

- [ ] **Step 5: Priming the hidden destinations**

In `chimera-core/src/ui/mod.rs`:

```rust
/// The address MIX+PLUS primes for a slot's `addr` (Decisions table): an
/// A stage's time primes the slot's TIME, S its LEVEL; the rest themselves.
fn prime_target(addr: ParamAddr) -> ParamAddr {
    use crate::params::EnvParams as E;
    match (addr.block, addr.param) {
        (BlockRef::Env(_), E::ATTACK | E::DECAY | E::RELEASE | E::HOLD) => {
            ParamAddr::new(addr.block, E::TIME)
        }
        (BlockRef::Env(_), E::SUSTAIN) => ParamAddr::new(addr.block, E::LEVEL),
        _ => addr,
    }
}
```

`current_param_addr` returns `slot_addr(…).map(prime_target)`. In `mod_label`, add an arm before `_`:

```rust
            BlockRef::Env(s) => {
                op_prefix = [b'E', b'1' + s.index() as u8, b' '];
                (&op_prefix, addr.spec().map_or("", |s| s.label))
            }
```

- [ ] **Step 6: The stand-ins cover eight sources**

In `update`, replace the two source lines (source 0 and source 1) with:

```rust
            let p = &sound.params;
            let mut mod_sources = [0.0f32; MAX_MOD_SOURCES];
            // Spec § UI: an A slot stands in with its SUS, a B slot with ½;
            // each CLASSIC LFO its own display LFO, a FUNC LFO 0; VEL 1; NOTE 0.
            for s in EnvSlot::ALL {
                let e = &p.envelopes[s.index()];
                mod_sources[ModSource::of_env(s).index()] = match e.env_type {
                    EnvType::A => e.sustain,
                    EnvType::B => 0.5,
                };
            }
            for s in LfoSlot::ALL {
                let l = &p.lfos[s.index()];
                let v = self.display_lfos[s.index()]
                    .run_block(l, chimera_hal::BLOCK_SIZE as u32 * UI_FPS);
                mod_sources[ModSource::of_lfo(s).index()] = match l.lfo_type {
                    LfoType::Classic => v,
                    LfoType::Func => 0.0,
                };
            }
            mod_sources[ModSource::Vel.index()] = 1.0;
```

(imports from `crate::dsp::modulator` and `crate::modulation::ModSource`; drop the unused `EnvParams` import).

- [ ] **Step 7: Run the tests**

Run: `env $T cargo test -p chimera-core --test routing_test --test prime_status_test --test mod_registry_test --test golden_test --test screen_golden_test`
Expected: PASS. The goldens hold (nothing routes an ENV destination), and the screens hold too: source 0's stand-in is still SUS, and source 1 is still the first display LFO.

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/params.rs chimera-core/src/addr.rs chimera-core/src/dsp/voice.rs \
  chimera-core/src/ui/mod.rs chimera-core/tests/routing_test.rs chimera-core/tests/prime_status_test.rs \
  chimera-core/tests/mod_registry_test.rs
git commit -m "ENV n LEVEL, TIME, RISE, FALL and SHAPE as destinations; all six slots feed the matrix"
```

---

### Task 10: VCA as a destination

`OutParams` gains the hidden VCA destination (id 2) and AMP's VEL (id 3). With a route into VCA, the voice's output is `sample · volume · g[n]` (spec § 4). Every ENV slot routed to the VCA fills a block of per-sample levels into one 64-sample stack buffer (its LEVEL ramped, Task 5), and the other routed sources ramp from their last block's value. With no route, Algo and Modal pass through bit for bit, through an exhaustive `match` on the engine, so a new engine (VA, #148) must say what it does without a route. The routes a voice uses are kept across a fade.

**Files:**
- Modify: `chimera-core/src/params.rs` (`OutParams.vca`, `vca_vel`, `OUT_SPECS`)
- Modify: `chimera-core/src/modulation.rs` (`VCA`, `ModSource::env_slot`)
- Modify: `chimera-core/src/dsp/voice.rs` (`VcaRoutes`, `vca`, `prev_sources`, `last_gain`, the gain buffer, the output stage)
- Create: `chimera-core/tests/vca_test.rs`

**Interfaces:**
- Consumes: `Envelope::run_block`'s `vca` argument (Task 5), presence (Task 8).
- Produces: `OutParams::{VCA = ParamId(2), VCA_VEL = ParamId(3)}`, fields `vca`, `vca_vel`; `modulation::VCA: ParamAddr`; `ModSource::env_slot(self) -> Option<EnvSlot>`; `Voice` private `VcaRoutes { bits: u8, amount: [f32; 8] }` with `of(&ModState)` and `has(usize)`, and `last_gain: f32` (the last sample's gain, for Task 11).

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/vca_test.rs`:

```rust
//! The VCA destination (filter-routing spec § 4, § Tests "VCA").

use chimera_core::addr::ParamAddr;
use chimera_core::dsp::envelope::{EnvMods, Envelope};
use chimera_core::dsp::voice::Voice;
use chimera_core::mod_path::ModDestRegistry;
use chimera_core::modulation::{CUTOFF, MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;

pub fn mods(routes: &[(ModSource, ParamAddr, i8)]) -> ModState {
    let mut reg = ModDestRegistry::new();
    for &(_, a, _) in routes {
        let _ = reg.add(a, *b"TEST\0\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for &(s, a, amt) in routes {
        let d = ms.find(a).unwrap();
        ms.set_route(s.index(), d, amt);
    }
    ms
}

/// `blocks` blocks of note 60 at `vel`, key up at `off`; the output.
fn render(p: &ParamSnapshot, ms: &ModState, off: usize, blocks: usize, vel: u8) -> Vec<f32> {
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::new(vel).unwrap(), p);
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for i in 0..blocks {
        if i == off {
            v.note_off();
        }
        v.render(&mut b, p, ms);
        out.extend_from_slice(&b);
    }
    out
}

fn init(e: EngineType) -> ParamSnapshot {
    ParamSnapshot::for_engine(e)
}

/// No route: VEL changes nothing (the Algo/Modal pass-through).
#[test]
fn no_route_passes_through_and_vel_is_unread() {
    for e in EngineType::ALL {
        let (mut a, mut b) = (init(e), init(e));
        (a.out.vca_vel, b.out.vca_vel) = (0.0, 1.0);
        let ms = ModState::new();
        assert_eq!(render(&a, &ms, 50, 80, 100), render(&b, &ms, 50, 80, 100), "{e:?}");
    }
}

/// ENV 2 → VCA at 100 %: the note follows ENV 2's contour (× velocity)
/// within 1 %, on Algo and Modal.
#[test]
fn env2_on_the_vca_follows_its_contour() {
    for e in EngineType::ALL {
        let p = init(e);
        let plain = render(&p, &ModState::new(), 50, 80, 100);
        let routed = render(&p, &mods(&[(ModSource::Env2, VCA, 127)]), 50, 80, 100);
        let mut env = Envelope::new();
        env.note_on(&p.envelopes[1]);
        let v = 100.0 / 127.0;
        for blk in 0..80 {
            let mut g = [0.0f32; BLOCK_SIZE];
            env.run_block(&p.envelopes[1], &EnvMods::NONE, blk < 50, SR, Some((&mut g, 1.0)));
            for n in 0..BLOCK_SIZE {
                let i = blk * BLOCK_SIZE + n;
                let want = plain[i] * g[n].max(0.0).min(1.0) * v;
                assert!((routed[i] - want).abs() <= 0.01 * plain[i].abs() + 1e-7, "{e:?} sample {i}");
            }
        }
    }
}

/// Halving ENV 2's amount halves the output exactly, with FOLD on: the
/// fold comes before the VCA.
#[test]
fn half_the_amount_is_half_the_level() {
    let mut p = init(EngineType::Algo);
    p.folder.fold = 0.6;
    let full = render(&p, &mods(&[(ModSource::Env2, VCA, 126)]), 50, 80, 100);
    let half = render(&p, &mods(&[(ModSource::Env2, VCA, 63)]), 50, 80, 100);
    for (i, (f, h)) in full.iter().zip(&half).enumerate() {
        assert_eq!(*f, 2.0 * h, "sample {i}");
    }
}

/// Two routes sum and clamp at 1: ENV 1 and ENV 2 at sustain (0.7 each).
#[test]
fn two_routes_sum_and_clamp() {
    let p = init(EngineType::Algo);
    let plain = render(&p, &ModState::new(), 90, 90, 127);
    let both = render(&p, &mods(&[(ModSource::Env1, VCA, 127), (ModSource::Env2, VCA, 127)]), 90, 90, 127);
    let late = 80 * BLOCK_SIZE; // both envelopes in sustain
    for i in late..late + BLOCK_SIZE {
        assert_eq!(both[i], plain[i] * 1.0, "sample {i}: the gain clamps at 1 (VEL 1 × vel 127)");
    }
}

/// The SH-101 feel: ENV 1 on the VCA and the cutoff; ENV 1's decay moves both.
#[test]
fn one_envelope_moves_cutoff_and_level() {
    let mut p = init(EngineType::Algo);
    p.filter.cutoff = 800.0;
    let with = |decay: f32, routes: &[(ModSource, ParamAddr, i8)]| {
        let mut q = p.clone();
        q.envelopes[0].decay = decay;
        render(&q, &mods(routes), 60, 60, 100)
    };
    let vca = [(ModSource::Env1, VCA, 127)];
    let cut = [(ModSource::Env1, CUTOFF, 127)];
    let both = [(ModSource::Env1, VCA, 127), (ModSource::Env1, CUTOFF, 127)];
    for r in [&vca[..], &cut[..], &both[..]] {
        assert_ne!(with(0.3, r), with(0.6, r), "{r:?}");
    }
    assert_ne!(with(0.3, &both), with(0.3, &vca));
    assert_ne!(with(0.3, &both), with(0.3, &cut));
}

/// AMP's VEL: at 0 two velocities give the same gain; at 100 % the gain
/// scales with velocity (the engine's own velocity divided out).
#[test]
fn vel_scales_the_vca() {
    for (vca_vel, same) in [(0.0f32, true), (1.0, false)] {
        let mut p = init(EngineType::Algo);
        p.out.vca_vel = vca_vel;
        let ms = mods(&[(ModSource::Env2, VCA, 127)]);
        // Over a whole block (a single sample may sit near a zero crossing).
        let ratio = |vel| {
            let (r, q) = (render(&p, &ms, 60, 60, vel), render(&p, &ModState::new(), 60, 60, vel));
            let block = 40 * BLOCK_SIZE..41 * BLOCK_SIZE;
            let sum = |v: &[f32]| v[block.clone()].iter().map(|x| x.abs()).sum::<f32>();
            sum(&r) / sum(&q)
        };
        let (lo, hi) = (ratio(40), ratio(120));
        if same {
            assert!((lo - hi).abs() < 1e-6, "{lo} {hi}");
        } else {
            assert!((lo / hi - 40.0 / 120.0).abs() < 1e-3, "{lo} {hi}");
        }
    }
}

/// Review Focus 4: LFO 1 → VCA at −127 clamps at 0, never inverts.
#[test]
fn a_negative_vca_route_never_inverts() {
    let mut p = init(EngineType::Algo);
    p.lfos[0].rate = 8.0;
    let plain = render(&p, &ModState::new(), 90, 90, 100);
    let neg = render(&p, &mods(&[(ModSource::Lfo1, VCA, -127)]), 90, 90, 100);
    for (i, (a, b)) in plain.iter().zip(&neg).enumerate() {
        assert!(a * b >= 0.0, "sample {i}: {a} vs {b}");
    }
    assert!(neg.iter().any(|&x| x != 0.0), "the negative half of the LFO opens it");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test vca_test`
Expected: FAIL to compile: no `VCA`, `vca_vel`.

- [ ] **Step 3: The VCA parameters**

In `params.rs`, `OutParams` gains:

```rust
    /// The VCA destination's stored 0 (spec § 4): its value is the sum of its routes.
    pub vca: f32,
    /// AMP's VEL: the VCA's velocity sensitivity, 0..1.
    pub vca_vel: f32,
```

(`Default`: `vca: 0.0, vca_vel: 1.0`), ids `pub const VCA: ParamId = ParamId(2); pub const VCA_VEL: ParamId = ParamId(3);`, `get`/`write` arms, and `OUT_SPECS: [ParamSpec; 4]` gains:

```rust
    ParamSpec::continuous(2, "VCA", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, true),
    ParamSpec::continuous(3, "VEL", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
```

In `modulation.rs`:

```rust
/// The VCA: a hidden destination, the voice's output level (spec § 4).
pub const VCA: ParamAddr = ParamAddr::new(BlockRef::Out, crate::params::OutParams::VCA);
```

and in `impl ModSource`:

```rust
    /// The ENV slot this source is, if any.
    pub const fn env_slot(self) -> Option<crate::dsp::modulator::EnvSlot> {
        use crate::dsp::modulator::EnvSlot;
        match self {
            ModSource::Env1 => Some(EnvSlot::Env1),
            ModSource::Env2 => Some(EnvSlot::Env2),
            ModSource::Env3 => Some(EnvSlot::Env3),
            _ => None,
        }
    }
```

- [ ] **Step 4: The per-sample gain**

In `voice.rs`, before `impl Voice`:

```rust
/// The routes into VCA a voice plays, kept from the last block before a
/// fade (spec § 4; a fading voice keeps its routes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct VcaRoutes {
    bits: u8,
    /// `amount / 127` per source.
    amount: [f32; MAX_MOD_SOURCES],
}

impl VcaRoutes {
    fn of(m: &ModState) -> Self {
        let mut r = Self::default();
        if let Some(d) = m.find(VCA) {
            r.bits = m.present(d);
            for (s, a) in r.amount.iter_mut().enumerate() {
                *a = crate::modulation::amount_scale(m.amount(s, d));
            }
        }
        r
    }

    fn has(&self, source: usize) -> bool {
        self.bits & (1 << source) != 0
    }
}
```

`Voice` gains `vca: VcaRoutes, prev_sources: [f32; MAX_MOD_SOURCES], last_gain: f32,`, written in `init_chain`'s `write_chain!` list as `VcaRoutes::default()`, `[0.0; MAX_MOD_SOURCES]` and `0.0` (the macro fails the build until every field is listed). In `render`, right after `let src = …; let key = …;`:

```rust
        if self.fade == 0 {
            self.vca = VcaRoutes::of(mod_state);
        }
        let vca = self.vca;
        // The VCA's gain (spec § 4): 64 samples on the audio stack.
        let mut gain = [0.0f32; BLOCK_SIZE];
```

the ENV loop feeds the buffer when routed:

```rust
        for (s, env) in EnvSlot::ALL.iter().zip(self.envs.iter_mut()) {
            let i = ModSource::of_env(*s).index();
            let feed = vca.has(i).then_some((&mut gain, vca.amount[i]));
            mod_values[i] = env.run_block(
                &src.envelopes[s.index()],
                &self.env_mods[s.index()],
                key,
                sample_rate,
                feed,
            );
        }
```

and after VEL and NOTE:

```rust
        // The other VCA sources ramp from their last block's value.
        for (s, &cur) in mod_values.iter().enumerate() {
            if vca.has(s) && ModSource::ALL[s].env_slot().is_none() {
                let (a, prev) = (vca.amount[s], self.prev_sources[s]);
                for (n, g) in gain.iter_mut().enumerate() {
                    *g += a * (prev + (cur - prev) * (n as f32 / BLOCK_SIZE as f32));
                }
            }
        }
        self.prev_sources = mod_values;
```

In the matrix pass, first thing in the loop body after `let a = mod_state.dest(d);`: `if a == VCA { continue; } // per sample, above`. The output stage becomes:

```rust
        // 5. The VCA, after the fold.
        let volume = m.out.volume;
        if vca.bits == 0 {
            // No route: the engine decides (spec § 4). No wildcard, so a new
            // engine can't inherit the pass-through (VA gates: #148).
            match self.active_engine {
                EngineType::Algo | EngineType::Modal => {
                    // Its own envelopes shape the sound: today's expression, bit for bit.
                    for sample in output.iter_mut() {
                        *sample *= volume;
                    }
                }
            }
        } else {
            let vel = 1.0 - m.out.vca_vel + m.out.vca_vel * self.last_velocity.unit();
            for (sample, g) in output.iter_mut().zip(&gain) {
                *sample *= volume * (g.max(0.0).min(1.0) * vel);
            }
            self.last_gain = gain[BLOCK_SIZE - 1].max(0.0).min(1.0) * vel;
        }
```

- [ ] **Step 5: Run the tests**

Run: `env $T cargo test -p chimera-core --test vca_test --test golden_test --test routing_test --test memory_budget_test`
Expected: PASS; the goldens hold (no golden or factory Sound routes VCA); the RAM asserts hold.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/params.rs chimera-core/src/modulation.rs chimera-core/src/dsp/voice.rs \
  chimera-core/tests/vca_test.rs
git commit -m "VCA as a destination: the sum of its routes, per sample, times AMP's VEL"
```

---

### Task 11: Voice lifetime by the VCA's routes

With routes into VCA, a voice ends at the end of the first block in which no routed source holds it (spec § 4). It ends through ADR 0027's 128-sample fade if its gain isn't 0, and an inactive engine still ends it. This is the no-drone guarantee.

**Files:**
- Modify: `chimera-core/src/dsp/voice.rs` (`vca_holds`, the lifetime check)
- Modify: `chimera-core/tests/vca_test.rs`

**Interfaces:**
- Consumes: `Envelope::holds` (Task 6), `VcaRoutes`, `last_gain` (Task 10), `ModSource::env_slot`.
- Produces: `Voice::is_active` follows § 4's lifetime rule.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/vca_test.rs`:

```rust
use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, LfoForm};

/// `blocks` of note 60, key up at `off`; `(output, active after each block)`.
fn life(p: &ParamSnapshot, ms: &ModState, off: usize, blocks: usize) -> (Vec<f32>, Vec<bool>) {
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    let (mut out, mut alive) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for i in 0..blocks {
        if i == off {
            v.note_off();
        }
        v.render(&mut b, p, ms);
        out.extend_from_slice(&b);
        alive.push(v.is_active());
    }
    (out, alive)
}

/// An Algo Sound whose engine outlives any envelope here (RR 1, the slowest).
fn long() -> ParamSnapshot {
    let mut p = init(EngineType::Algo);
    p.algo.ops[0].rr = 1;
    p
}

fn with_env3(f: Func, fall: f32) -> ParamSnapshot {
    let mut p = long();
    let e = &mut p.envelopes[2];
    (e.env_type, e.func.fall) = (EnvType::B, fall);
    e.func.set_func(f);
    p
}

/// ENV 2 → VCA: the voice ends at the end of the block in which ENV 2 goes idle.
#[test]
fn env2_on_the_vca_ends_the_voice_when_it_idles() {
    let p = long();
    let (_, alive) = life(&p, &mods(&[(ModSource::Env2, VCA, 127)]), 20, 400);
    let (_, plain) = life(&p, &ModState::new(), 20, 400);
    let mut env = Envelope::new();
    env.note_on(&p.envelopes[1]);
    // Ticked per sample, as the voice runs a slot routed to its VCA.
    let idle = (0..400)
        .find(|&b| {
            let mut g = [0.0f32; BLOCK_SIZE];
            env.run_block(&p.envelopes[1], &EnvMods::NONE, b < 20, SR, Some((&mut g, 1.0)));
            env.is_idle()
        })
        .unwrap();
    assert!(alive[idle - 1] && !alive[idle], "ends at block {idle}");
    assert!(plain[idle], "the engine alone would have kept it");
}

/// No drone: a source that doesn't end holds the voice only while the key
/// is held; after key-up it ends through the 128-sample fade.
#[test]
fn nothing_drones_after_key_up() {
    let e3 = |f| (with_env3(f, 0.2), ModSource::Env3);
    for (p, s) in [
        e3(Func::Lfo(LfoForm::Free)),
        e3(Func::Env(EnvForm::Cycle)),
        (long(), ModSource::Lfo1),
        (long(), ModSource::Vel),
    ] {
        let (out, alive) = life(&p, &mods(&[(s, VCA, 127)]), 30, 60);
        let gone = alive.iter().position(|a| !a).expect("the voice ends");
        // At once (gain 0 at key-up) or after the two-block fade.
        assert!((30..=33).contains(&gone), "{s:?}: ended at block {gone}");
        assert!(out[(gone + 1) * BLOCK_SIZE..].iter().all(|&x| x == 0.0));
    }
}

/// A cycling burst ends after the burst running at key-up; a one-shot
/// burst ends after its burst, key held or not.
#[test]
fn bursts_end_when_their_burst_does() {
    let len = |fall: f32| {
        (chimera_core::dsp::modulator::law::BURST_LEN.at(fall) * SR as f32) as usize / BLOCK_SIZE
    };
    let p = with_env3(Func::Burst(EnvForm::Cycle), 0.2);
    let (_, alive) = life(&p, &mods(&[(ModSource::Env3, VCA, 127)]), 30, 200);
    let gone = alive.iter().position(|a| !a).expect("ends");
    assert!(gone > 30 && gone <= 30 + len(0.2) + 3, "cycle burst: {gone}");
    let p = with_env3(Func::Burst(EnvForm::Ad), 0.2);
    let (_, alive) = life(&p, &mods(&[(ModSource::Env3, VCA, 127)]), usize::MAX, 200);
    let gone = alive.iter().position(|a| !a).expect("ends with the key held");
    assert!(gone <= len(0.2) + 3, "AD burst: {gone}");
}

/// Under every configuration an inactive engine ends the voice: a fast
/// engine release ends it while ENV 1 (60 s release) still holds.
#[test]
fn an_inactive_engine_always_ends_the_voice() {
    let mut p = init(EngineType::Algo);
    p.algo.ops[0].rr = 15;
    p.envelopes[0].speed = chimera_core::dsp::modulator::EnvSpeed::Slow;
    p.envelopes[0].release = 1.0;
    let (_, alive) = life(&p, &mods(&[(ModSource::Env1, VCA, 127)]), 20, 400);
    assert!(alive.iter().any(|a| !a), "the engine's end ends the voice");
}

/// Review Focus 5: the Sound switches engine mid-note; the fade keeps the
/// old VCA routes, stays finite, and the held note restarts on Modal.
#[test]
fn an_engine_switch_fades_with_the_old_vca_routes() {
    let algo = long();
    let modal = init(EngineType::Modal);
    let routed = mods(&[(ModSource::Env2, VCA, 127)]);
    let mut v = Voice::new(SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &algo);
    let mut b = [0.0f32; BLOCK_SIZE];
    let mut peak = 0.0f32;
    for _ in 0..30 {
        v.render(&mut b, &algo, &routed);
        peak = b.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    for i in 0..10 {
        v.render(&mut b, &modal, &ModState::new());
        assert!(b.iter().all(|x| x.is_finite()), "block {i}");
        if i < 2 {
            assert!(b.iter().all(|x| x.abs() <= peak * 1.01), "the fade never gets louder");
        }
    }
    assert!(v.is_active(), "the held note restarted on Modal");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test vca_test`
Expected: FAIL: the voice outlives ENV 2 and every non-ending source (it ends only with the engine).

- [ ] **Step 3: The lifetime check**

In `voice.rs`:

```rust
    /// A VCA source still holds the voice (spec § 4): an ENV slot per its
    /// TYPE and FORM, anything else while the key is held.
    fn vca_holds(&self) -> bool {
        let key = self.held;
        ModSource::ALL
            .iter()
            .filter(|s| self.vca.has(s.index()))
            .any(|s| match s.env_slot() {
                Some(e) => self.envs[e.index()].holds(key),
                None => key,
            })
    }
```

At the end of `render`, after the existing `if self.fade > 0 { … }` block:

```rust
        // Lifetime by the VCA's routes (spec § 4), once per block: a voice
        // no routed source holds ends, through the fade if it still sounds.
        if self.active && self.fade == 0 && self.vca.bits != 0 && !self.vca_holds() {
            if self.last_gain != 0.0 {
                self.after_fade = AfterFade::Idle;
                self.fade = Self::FADE;
            } else {
                self.active = false;
            }
        }
```

(The engine check above it, `self.active = self.engines.is_active(…)`, still ends the voice in every configuration.)

- [ ] **Step 4: Run the tests**

Run: `env $T cargo test -p chimera-core --test vca_test --test instrument_test --test golden_test --test voice_alloc_test`
Expected: PASS. With no VCA route nothing changes, so the goldens and the instrument's shedding tests hold.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/voice.rs chimera-core/tests/vca_test.rs
git commit -m "A voice lives as long as a source routed to its VCA holds it: no drone"
```

---

### Task 12: `ModRouting::cost`

`Voice::cost` bills the modulator pool as a function of the Sound (spec § CPU), the way `Engines::cost` bills the engine. **Ruling:** the model must never undercount, so until Task 13's bench measures each term it bills the plan review's estimates: BASE 30, ENV_A 30, ENV_B 40, CURVE 15 more, OTHER 3 and CLAMP 3. The spec's 8/6/10/14/1/2 would bill the worst routing about 125 cycles low, and the allocator would overrun instead of dropping a voice.

**Files:**
- Modify: `chimera-core/src/modulation.rs` (`ModRouting`)
- Modify: `chimera-core/src/dsp/voice.rs` (`Voice::cost`)
- Modify: `chimera-core/tests/cost_test.rs`

**Interfaces:**
- Consumes: `routes_into`, `VCA`, `env_slot` (Tasks 8, 10).
- Produces: `modulation::ModRouting` with `BASE = Cost(30)`, `ENV_A = Cost(30)`, `ENV_B = Cost(40)`, `CURVE = Cost(15)`, `OTHER = Cost(3)`, `CLAMP = Cost(3)` and `cost(p: &ParamSnapshot, mods: &ModState) -> Cost`; Task 13 replaces the six numbers with bench readings.

- [ ] **Step 1: Write the failing tests**

In `chimera-core/tests/cost_test.rs`, add `use chimera_core::modulation::{ModRouting, ModSource, VCA};` and:

```rust
fn routed(routes: &[ModSource]) -> ModState {
    let mut reg = chimera_core::mod_path::ModDestRegistry::new();
    reg.add(VCA, *b"OUT VCA\0").unwrap();
    let mut ms = ModState::from_registry(&reg, 8);
    for s in routes {
        ms.set_route(s.index(), 0, 127);
    }
    ms
}

/// The spec's shape, billed high: the defaults `BASE`; ENV 2 (A) on the
/// VCA `BASE + CLAMP + ENV_A`; the worst case, three curved B slots and the
/// five other sources, `BASE + CLAMP + 3·(ENV_B + CURVE) + 5·OTHER`.
#[test]
fn mod_routing_bills_the_spec_shape() {
    use chimera_core::dsp::modulator::{EnvType, FuncMode};
    use ModRouting as M;
    let p = ParamSnapshot::for_engine(EngineType::Algo);
    let defaults = chimera_core::preset::Sound::init(chimera_core::params::EngineType::Algo).mod_state;
    assert_eq!(M::cost(&p, &defaults), M::BASE);
    assert_eq!(M::cost(&p, &routed(&[ModSource::Env2])), M::BASE + M::CLAMP + M::ENV_A);
    let mut worst = p.clone();
    for e in worst.envelopes.iter_mut() {
        (e.env_type, e.func.mode, e.func.shape) = (EnvType::B, FuncMode::Env, 0.8);
    }
    let three_b = M::ENV_B + M::CURVE + M::ENV_B + M::CURVE + M::ENV_B + M::CURVE;
    let five_other = Cost(5 * M::OTHER.0);
    assert_eq!(M::cost(&worst, &routed(&ModSource::ALL)), M::BASE + M::CLAMP + three_b + five_other);
    // A route into ENV 2's SHAPE bills the curve on a centred B ENV.
    let mut b2 = p.clone();
    (b2.envelopes[1].env_type, b2.envelopes[1].func.mode) = (EnvType::B, FuncMode::Env);
    let mut shaped = routed(&[ModSource::Env2]);
    let d = shaped
        .push(chimera_core::addr::ParamAddr::new(
            chimera_core::addr::BlockRef::Env(chimera_core::dsp::modulator::EnvSlot::Env2),
            chimera_core::params::EnvParams::SHAPE,
        ))
        .unwrap();
    shaped.set_route(ModSource::Lfo1.index(), d, 64);
    assert_eq!(M::cost(&b2, &routed(&[ModSource::Env2])), M::BASE + M::CLAMP + M::ENV_B);
    assert_eq!(M::cost(&b2, &shaped), M::BASE + M::CLAMP + M::ENV_B + M::CURVE);
    // The review's estimates, until the bench (Task 13): 30, 63 and 213.
    assert_eq!((M::BASE, M::cost(&p, &routed(&[ModSource::Env2]))), (Cost(30), Cost(63)));
    assert_eq!(M::cost(&worst, &routed(&ModSource::ALL)), Cost(213));
}
```

and change these existing tests (each turns red otherwise):
- `voice_costs_are_the_bench_measurements`: `assert_eq!(voice_cost(EngineType::Modal), Cost(400) + ModRouting::BASE);` and the loop's right side `Engines::cost(&p, &mods) + Voice::CHAIN_COST + ModRouting::BASE`;
- `voices_at`: `let voice = Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(p);`;
- `the_costliest_patch_gets_six_voices_on_rev_v`: keep `842` for the engine and chain, add `assert_eq!(Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(&p), 872);`; the doc reads "6 × 872 + 1,360 = 6,592 ≤ 7,000; rev Y (5,833 − 1,360) / 872 = 5.1". Its `voices_at(CPU_HZ_REV_Y, &p) == 5` still holds.
- `every_factory_sound_gets_six_voices_on_rev_v` needs no change (every factory patch bills at most 872).

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test cost_test`
Expected: FAIL to compile: no `ModRouting`.

- [ ] **Step 3: The bill**

In `chimera-core/src/modulation.rs`:

```rust
use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::modulator::{EnvType, FuncMode};
use crate::hw::Cost;
use crate::params::{EnvParams, ParamSnapshot};

/// The modulator pool's cycles per sample (spec § CPU). Never low: the
/// plan review's estimates until the bench measures each term.
pub struct ModRouting;

impl ModRouting {
    /// Six per-block modulators, the eight-row matrix sum and `fast_exp2`.
    pub const BASE: Cost = Cost(30);
    /// An ENV slot of type A filling the VCA's buffer.
    pub const ENV_A: Cost = Cost(30);
    /// Type B filling it.
    pub const ENV_B: Cost = Cost(40);
    /// More for B in ENV mode with SHAPE off centre, or with a route into
    /// its SHAPE: a divide per sample.
    pub const CURVE: Cost = Cost(15);
    /// Each other VCA route's ramp.
    pub const OTHER: Cost = Cost(3);
    /// The VCA's clamp and multiply, with any route.
    pub const CLAMP: Cost = Cost(3);

    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let bits = mods.routes_into(VCA);
        if bits == 0 {
            return Self::BASE;
        }
        ModSource::ALL
            .iter()
            .filter(|s| bits & (1 << s.index()) != 0)
            .map(|s| match s.env_slot() {
                None => Self::OTHER,
                Some(slot) => {
                    let e = &p.envelopes[slot.index()];
                    // A SHAPE route moves a centred SHAPE off 0.5: the divide runs.
                    let shape_routed =
                        mods.routes_into(ParamAddr::new(BlockRef::Env(slot), EnvParams::SHAPE)) != 0;
                    match e.env_type {
                        EnvType::A => Self::ENV_A,
                        EnvType::B if e.func.mode == FuncMode::Env && (e.func.shape != 0.5 || shape_routed) => {
                            Self::ENV_B + Self::CURVE
                        }
                        EnvType::B => Self::ENV_B,
                    }
                }
            })
            .fold(Self::BASE + Self::CLAMP, |a, b| a + b)
    }
}
```

In `voice.rs`, `Voice::cost` becomes `Engines::cost(p, mods) + Self::CHAIN_COST + ModRouting::cost(p, mods)` (import `ModRouting`). The allocator recosts on every block (`Instrument::recost`), so a route into VCA that raises a Sound's cost can shed held notes (ADR 0026, #31), as the spec intends.

- [ ] **Step 4: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS (the allocator's own tests use literal costs; nothing else pins `Voice::cost`).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/modulation.rs chimera-core/src/dsp/voice.rs chimera-core/tests/cost_test.rs
git commit -m "Voice::cost bills the modulator pool and the VCA's routes"
```

---

### Task 13: Bench the modulator pool (hardware STOP)

The cost model went live in Task 12 on estimates; the UI tasks after this build on it. This stop measures each term on the chip before going on: the pool's base and each per-VCA term on its own row, plus the spec's MODS and SVF rows. They run as a second bench screen, ROUTING, after the existing one.

**Files:**
- Modify: `chimera-stm32/src/bench.rs` (the ROUTING rows and their screen)
- Modify (after the STOP): `chimera-core/src/modulation.rs` (`ModRouting`'s six numbers), `chimera-core/tests/cost_test.rs`, `docs/superpowers/plans/2026-09-28-filter-routing.md` (a `## Measured` section at the end)

**Interfaces:**
- Consumes: `ModState::{find, set_route}`, `ModRouting`, `FilterMode::Phaser`.
- Produces: `bench::ROUTING: [RoutingRow; 10]`, `RoutingRow = (&'static str, fn(&mut PartAudio))`; the readings, recorded for Task 15's `FilterKind::cost`.

- [ ] **Step 1: The rows**

In `chimera-stm32/src/bench.rs` (`use chimera_core::instrument::PartAudio;` and the core types named below):

```rust
const ROUTING_ROWS: usize = 10;

/// A ROUTING row: its label and the Part it plays (params and matrix). Each
/// per-VCA row is 1 OP plus one routing, so its reading less 1 OP (and
/// less the clamp) is that term alone.
type RoutingRow = (&'static str, fn(&mut PartAudio));
const ROUTING: [RoutingRow; ROUTING_ROWS] = [
    ("1 OP", |p| p.params = algo(AlgoId::A1, 0b1, 0)),
    ("MODS", mods),
    ("SVF", |p| {
        p.params = algo(AlgoId::A1, 0b1, 0);
        p.params.filter.set_mode(FilterMode::Phaser); // the SVF's costliest mode
    }),
    ("A VCA", |p| on_vca(p, &[ModSource::Env2], None)),
    ("B VCA", |p| on_vca(p, &[ModSource::Env2], Some((Func::Env(EnvForm::Ad), 0.5)))),
    // BURST: `fast_sin` and two `tilt` divides per sample.
    ("B BST", |p| on_vca(p, &[ModSource::Env2], Some((Func::Burst(EnvForm::Ad), 0.8)))),
    // LFO: a `tilt` divide and a wrap per sample.
    ("B LFO", |p| on_vca(p, &[ModSource::Env2], Some((Func::Lfo(LfoForm::Free), 0.8)))),
    ("B CRV", |p| on_vca(p, &[ModSource::Env2], Some((Func::Env(EnvForm::Ad), 0.8)))),
    ("VEL VCA", |p| on_vca(p, &[ModSource::Vel], None)),
    ("2 VCA", |p| on_vca(p, &[ModSource::Vel, ModSource::Note], None)),
];

/// 1 OP with `sources` routed to the VCA at 127; with `b`, ENV 2 is type
/// B running that `Func` at that SHAPE (0.5 linear, 0.8 curved or tilted).
fn on_vca(p: &mut PartAudio, sources: &[ModSource], b: Option<(Func, f32)>) {
    p.params = algo(AlgoId::A1, 0b1, 0);
    if let Some((f, shape)) = b {
        let e2 = &mut p.params.envelopes[1];
        e2.env_type = EnvType::B;
        e2.func.set_func(f);
        e2.func.shape = shape;
    }
    let mut reg = ModDestRegistry::new();
    let _ = reg.add(VCA, *b"BENCH\0\0\0");
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for s in sources {
        ms.set_route(s.index(), 0, 127);
    }
    p.mod_state = ms;
}

/// Spec § Tests "Bench": 1 OP; ENV 2 type B, ENV mode, SHAPE off centre →
/// VCA; ENV 1 → CUTOFF; every source routed; all three LFOs FUNC.
fn mods(p: &mut PartAudio) {
    p.params = algo(AlgoId::A1, 0b1, 0);
    let e2 = &mut p.params.envelopes[1];
    e2.env_type = EnvType::B;
    e2.func.set_func(Func::Env(EnvForm::Ad));
    e2.func.shape = 0.8;
    for l in p.params.lfos.iter_mut() {
        l.lfo_type = LfoType::Func;
    }
    let res = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let drive = ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE);
    let fold = ParamAddr::new(BlockRef::Folder, FolderParams::FOLD);
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let level = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    let routes = [
        (ModSource::Env1, CUTOFF),
        (ModSource::Env2, VCA),
        (ModSource::Env3, res),
        (ModSource::Lfo1, morph),
        (ModSource::Lfo2, drive),
        (ModSource::Lfo3, fold),
        (ModSource::Vel, level),
        (ModSource::Note, CUTOFF),
    ];
    let mut reg = ModDestRegistry::new();
    for (_, a) in routes {
        let _ = reg.add(a, *b"BENCH\0\0\0");
    }
    let mut ms = ModState::from_registry(&reg, MAX_MOD_SOURCES);
    for (s, a) in routes {
        if let Some(d) = ms.find(a) {
            ms.set_route(s.index(), d, 64);
        }
    }
    p.mod_state = ms;
}

/// The ROUTING screen, in the first screen's `voice_row`s: ten rows from
/// y 46 at `ROW_H` 25 end at 283.
fn show_routing(display: &mut impl ChimeraDisplay, rows: &[[u32; MAX_VOICES]; ROUTING_ROWS]) {
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    draw::text(display, &theme::FONT_VALUE, "ROUTING", 4, 16, theme::INK);
    let mut line = FmtBuf::new();
    for (i, (&(label, _), c)) in ROUTING.iter().zip(rows).enumerate() {
        voice_row(display, &mut line, 46 + i as i32 * ROW_H, label, c);
    }
    display.flush();
}
```

(`use chimera_core::addr::{BlockRef, ParamAddr}; use chimera_core::dsp::algo::params::AlgoParams; use chimera_core::dsp::filter::FilterMode; use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, LfoForm, LfoType}; use chimera_core::mod_path::ModDestRegistry; use chimera_core::modulation::{CUTOFF, MAX_MOD_SOURCES, ModSource, ModState, VCA}; use chimera_core::params::{FilterParams, FolderParams, OutParams};`.) In `run`, after the first screen's hold loop:

```rust
    let mut routing = [[0u32; MAX_VOICES]; ROUTING_ROWS];
    for (row, &(_, part)) in routing.iter_mut().zip(&ROUTING) {
        for n in 1..=MAX_VOICES {
            row[n - 1] = rig.time(
                |s| {
                    part(&mut s.parts[0]);
                    black_box(&s.parts[0]);
                },
                n,
                36,
                12,
            );
        }
    }
    show_routing(display, &routing);
    for _ in 0..HOLD_SECONDS {
        crate::clocks::delay_us(clocks.cpu_hz, 1_000_000);
    }
```

Run: `cargo fmt --all && just check` (it builds and lints the bench feature and runs the stack check; `routing` is 240 B on the stack). Commit:

```bash
git add chimera-stm32/src/bench.rs
git commit -m "Bench: a ROUTING screen, each modulator cost term on its own row"
```

- [ ] **Step 2: STOP. Ask the owner to run the bench, and wait**

Send the owner this, then wait for the numbers:

> The modulator pool and the VCA routes are in (filter-routing Tasks 1–12). Please bench them:
> 1. Put the synth in DFU mode and run `just flash-bench`. The first screen shows as before for 30 s. Please read `1 OP /VOICE` there too.
> 2. A second screen, ROUTING, follows for 30 s. Please read all ten `/VOICE` numbers: 1 OP, MODS, SVF, A VCA, B VCA, B BST, B LFO, B CRV, VEL VCA, 2 VCA.
> 3. `just flash` to put the normal firmware back, and reply with the numbers.

- [ ] **Step 3: Bill what was measured**

Record the raw readings, with the date, in a new `## Measured` section at the end of this plan. From the ROUTING screen (all per voice, rev V, 480 MHz), each term is rounded up:
- OTHER = 2 VCA − VEL VCA;
- CLAMP = VEL VCA − 1 OP − OTHER;
- ENV_A = A VCA − 1 OP − CLAMP;
- ENV_B = max(B VCA, B BST, B LFO) − 1 OP − CLAMP (B's costliest form bills every form);
- CURVE = B CRV − B VCA;
- BASE = 1 OP − 436 (the 1 OP row before the pool, measured 2026-09-27, in `cost_test`'s `the_model_bills_every_bench_row_high`).

A term that reads 0 or below is billed 1: noise must never make the model cheaper than the work. Put the six numbers into `ModRouting`, and replace the doc line on each with `measured <date>, bench ROUTING row, rev V at 480 MHz`.

Check the MODS row against the model: `CHAIN_COST + AlgoEngine::cost(1 OP) + ModRouting::cost(MODS)` must be at least the MODS reading. If it isn't, raise `BASE` by the difference, rounded up, and note it in `## Measured`. Also record `SVF − 1 OP` there: Task 15 bills it as `FilterKind::cost(Svf, _)`.

In `cost_test.rs`:
- `mod_routing_bills_the_spec_shape`'s last two asserts get the new totals.
- Add `the_model_bills_the_mods_row_high`, the check above with the recorded MODS reading, in the style of `the_model_bills_every_bench_row_high`.
- Recompute the costliest patch's bill (842 + BASE) in `the_costliest_patch_gets_six_voices_on_rev_v`.
- `every_factory_sound_gets_six_voices_on_rev_v` must stay true. If a factory Sound falls below six on rev V, stop and tell the owner (ADR 0031 promises six).

```bash
cargo fmt --all && just check
git add chimera-core/src/modulation.rs chimera-core/tests/cost_test.rs docs/superpowers/plans/2026-09-28-filter-routing.md
git commit -m "ModRouting bills the bench's measured terms"
```

---

### Task 14: FLD / VCA (AMP) on every chain, and dimmed cells

The FLD block becomes FLD / VCA (spec § 5): "Fold / VCA", short AMP, with FOLD · SYM · MIX · VEL. It ends the Algo chain (where FLD was) and the Modal chain (which gains it), last before MOD. A slot that is fixed or inapplicable draws dimmed and ignores its encoder. Here that is VEL under the Algo/Modal pass-through. MIX+PLUS on VEL primes the hidden VCA (see Decisions).

**Files:**
- Create: `chimera-core/src/ui/view.rs` (`dimmed`)
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod view;`, dimmed encoders and priming, `prime_target` for VCA, `mod_label`)
- Modify: `chimera-core/src/ui/block_registry.rs` (`FOLDER`, `MODAL_PLUCK_BLOCKS`)
- Modify: `chimera-core/src/ui/components.rs` (draw `Look::Dimmed`), `chimera-core/src/ui/renderer.rs` (dimmed cells)
- Create: `chimera-core/tests/amp_page_test.rs`
- Modify tests: `block_def_tests.rs`, `binding_test.rs`, `part_page_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: `VCA`, `routes_into` (Tasks 8, 10), `Look` and `renderer::look` (Task 8).
- Produces: `ui::view::dimmed(addr: ParamAddr, sound: &Sound) -> bool` (Tasks 15–17 add rules to its match); `FOLDER` binds `ParamAddr(Out, OutParams::VCA_VEL)` in slot d.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/amp_page_test.rs`:

```rust
//! FLD / VCA (filter-routing spec § 5, § UI "AMP").

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::modulation::{ModSource, VCA};
use chimera_core::params::OutParams;
use chimera_core::preset::{EngineType, Sound};
use chimera_core::ui::block_registry::FOLDER;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::{PrimeStatus, UiState, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const VEL: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::VCA_VEL);

#[test]
fn every_part_chain_has_amp_last_before_mod() {
    for ct in EngineType::ALL {
        let blocks = chain_def_for(ct).blocks;
        assert_eq!(blocks[blocks.len() - 2].def.id, FOLDER.id, "{ct:?}");
    }
    assert_eq!((FOLDER.name, FOLDER.short), ("Fold / VCA", "AMP"));
}

#[test]
fn vel_is_dimmed_until_a_vca_route_exists() {
    let s = Sound::init(EngineType::Algo);
    assert!(view::dimmed(VEL, &s));
    let mut r = s.clone();
    let d = r.mod_state.push(VCA).unwrap();
    r.mod_state.set_route(ModSource::Env2.index(), d, 127);
    assert!(!view::dimmed(VEL, &r));
}

fn on_amp() -> UiState {
    let mut ui = UiState::new();
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    ui
}

#[test]
fn a_dimmed_vel_ignores_its_encoder() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::D, -20));
    assert_eq!(ui.params().out.vca_vel, 1.0);
}

/// MIX+PLUS on AMP's VEL primes the hidden VCA (the plan's Decisions table).
#[test]
fn mix_plus_on_vel_primes_the_vca() {
    let mut ui = on_amp();
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::Added));
    assert!(ui.performance.parts[0].sound.dest_registry.is_primed(VCA));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test amp_page_test`
Expected: FAIL to compile: no `ui::view`.

- [ ] **Step 3: The block**

In `block_registry.rs`, replace `FOLDER` with:

```rust
/// FLD / VCA (spec § 5): the fold, then the VCA; last before MOD on every
/// Part chain.
pub static FOLDER: BlockDef = BlockDef {
    id: 9,
    name: "Fold / VCA",
    short: "AMP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Folder, FolderParams::FOLD),
        ParamSlot::param(BlockRef::Folder, FolderParams::SYMMETRY),
        ParamSlot::param(BlockRef::Folder, FolderParams::MIX),
        ParamSlot::param(BlockRef::Out, OutParams::VCA_VEL),
        EMPTY,
        EMPTY,
    ],
};
```

and `MODAL_PLUCK_BLOCKS` becomes `[ChainBlock; 4]`, with `ChainBlock { def: &FOLDER, sub_pages: &[] }` between FILTER and MOD_MATRIX.

- [ ] **Step 4: Dimming**

Create `chimera-core/src/ui/view.rs`:

```rust
//! A page slot as it reads now (filter-routing spec § UI).

use crate::addr::{BlockRef, ParamAddr};
use crate::modulation::VCA;
use crate::params::OutParams;
use crate::preset::Sound;

/// A fixed or inapplicable slot draws dimmed, and its encoder is ignored
/// (spec § UI "Dimmed").
pub fn dimmed(addr: ParamAddr, sound: &Sound) -> bool {
    match (addr.block, addr.param) {
        // AMP's VEL under the Algo/Modal pass-through (spec § 5).
        (BlockRef::Out, OutParams::VCA_VEL) => sound.mod_state.routes_into(VCA) == 0,
        _ => false,
    }
}
```

and `pub mod view;` in `ui/mod.rs`.

In `components.rs`, `cell` draws `Look::Dimmed` with label and value in `theme::MID`, and neither the value bar nor the mod bar:

```rust
    let dim = c.look == Look::Dimmed;
    let label_color = if c.active && !dim { theme::ACCENT } else { theme::MID };
```

  and the value colour is `theme::MID` when `dim`; the `if !c.fmt.is_discrete()` bar block becomes `if !c.fmt.is_discrete() && !dim`, and the mod bar is skipped when `dim`.

In `renderer.rs`, Task 8's `look` gains the dimmed arm before `_ => Live`, so drawing and the dirty-region key (its `looks` field) both see it:

```rust
        _ if slot_addr(f.def, i, f.sel_op)
            .is_some_and(|a| crate::ui::view::dimmed(a, &f.parts[f.active_part].sound)) =>
        {
            components::Look::Dimmed
        }
```

- [ ] **Step 5: The encoder and MIX+PLUS on a dimmed slot**

In `ui/mod.rs`, `prime_target` gains `(BlockRef::Out, crate::params::OutParams::VCA_VEL) => crate::modulation::VCA,`. `mod_label` gains, before `_`:

```rust
            _ if addr == crate::modulation::VCA => (b"OUT ".as_slice(), "VCA"),
```

In the non-matrix encoder loop, after the route case:

```rust
                    let own = slot_addr(def, i, self.sel_op);
                    if own.is_some_and(|a| view::dimmed(a, &self.performance.parts[at].sound)) {
                        continue; // dimmed: the encoder is ignored
                    }
```

In the MIX+PLUS branch, a dimmed slot whose prime target is itself reports NOT MODULATABLE. (AMP's VEL primes the VCA, so it primes even while dimmed.)

```rust
                if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
                    let own = slot_addr(def, self.focused_slot(), self.sel_op);
                    let sound = &self.performance.parts[at].sound;
                    if let Some(a) = own
                        && prime_target(a) == a
                        && view::dimmed(a, sound)
                    {
                        self.prime_status = Some(PrimeStatus::NotModulatable);
                    } else if let Some(addr) = self.current_param_addr() {
                        // … the existing priming body, unchanged …
                    }
                }
```

- [ ] **Step 6: Update the tests and the screens**

- `block_def_tests.rs`, `algo_chain_is_alg_osc_then_the_voice_chain`: `["ALG", "OSC", "DRV", "FLT", "AMP", "MOD"]`.
- `binding_test.rs`, the FOLDER row: `("FOLD", Uni), ("SYM", Bi), ("MIX", Bi), ("VEL", Uni), ("--", Uni), ("--", Uni)`.
- `part_page_test.rs`: `assert_eq!(read(&reg::FOLDER, &p), [0.0, 0.5, 0.5, 1.0, 0.0, 0.0]);`.
- `screen/mod.rs`, new cases after `bigviz_env`:

```rust
    ("amp_vel_dimmed", |ui| {
        plus(ui, 4);
        feed(ui, Input::turn(EncoderId::A, 20)); // FOLD
        feed(ui, Input::turn(EncoderId::D, 1)); // VEL focused, dimmed
    }),
    ("amp_vel_live", |ui| {
        plus(ui, 4);
        feed(ui, Input::turn(EncoderId::D, 1));
        prime(ui); // VEL primes the VCA
        plus(ui, 1); // the matrix: E1 → FLT CUTOFF
        feed(ui, Input::turn(EncoderId::A, 2)); // E2
        feed(ui, Input::turn(EncoderId::B, 1)); // OUT VCA
        feed(ui, Input::turn(EncoderId::E, 100)); // E2 → VCA
        feed(ui, Input::press(ButtonId::Minus)); // back to AMP: VEL live
    }),
    ("modal_amp", |ui| {
        load_init(ui, EngineType::Modal);
        plus(ui, 2); // MDL · FLT · AMP
    }),
```

  with their three rows in `screen_golden_test.rs`'s `GOLDENS`, in the same place.

- [ ] **Step 7: Run the tests**

Run: `env $T cargo test -p chimera-core --test amp_page_test --test block_def_tests --test binding_test --test part_page_test --test header_map_test --test all_pages_walk_test`
Expected: PASS. Then `env $T cargo test -p chimera-core --test screen_golden_test`: every Algo-chain screen mismatches (the map now reads AMP) and the three new cases have no golden.

- [ ] **Step 8: Look at the screens and re-record**

Look at `amp_vel_dimmed`: header `FOLD / VCA`, cells FOLD · SYM · MIX / VEL · dash · dash, VEL's label and `127` in the dim grey with no bar; the map `ALG · OSC · DRV · FLT · AMP · MOD` with AMP lit. `amp_vel_live`: VEL drawn like its neighbours. `modal_amp`: the map `MDL · FLT · AMP · MOD`. Check one other Algo screen (`engine_algo`): only the map's fifth label changed. Re-record every mismatching screen golden and the three new ones.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/ui/view.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/block_registry.rs \
  chimera-core/src/ui/components.rs chimera-core/src/ui/renderer.rs chimera-core/tests/amp_page_test.rs \
  chimera-core/tests/block_def_tests.rs chimera-core/tests/binding_test.rs chimera-core/tests/part_page_test.rs \
  chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "FLD / VCA ends every Part chain; VEL dims under the pass-through"
```

---

### Task 15: KIND and the filter panel

This task adds `FilterKind`, with the SVF as its one variant, and the KIND parameter (id 6). `FilterParams` keeps `mode ∈ kind.modes()` on every write. The FLT page becomes KIND · then the kind's five knobs, and FLT › MODE becomes MODE · the kind's two extras. Both are read from `const` panel data (spec § 6). Slots now resolve through `ui::view`: a filter-panel knob resolves against the Sound's KIND, and a route view resolves to a matrix cell, so pages, vizzes and encoders read by address. KIND is dimmed while it is the only built kind. `Voice::cost` adds `FilterKind::cost`.

**Files:**
- Modify: `chimera-core/src/dsp/filter.rs` (`FilterKind`, `KIND_NAMES`, `kind_change`)
- Modify: `chimera-core/src/params.rs` (`FilterParams.kind`, `kind()`, `set_kind()`, `set_mode` by kind, KIND id 6, MODE by kind)
- Modify: `chimera-core/src/block.rs` (`normalize` on a one-choice spec)
- Create: `chimera-core/src/ui/filter_panel.rs`
- Modify: `chimera-core/src/ui/view.rs` (`SlotCtx`, `View`, `view`, `dimmed` for KIND and MODE)
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding::FilterPanel`; `Route` removed; `slot_addr` takes `&SlotCtx`)
- Modify: `chimera-core/src/ui/block_registry.rs` (FILTER, FILTER_MODE)
- Modify: `chimera-core/src/ui/part_page.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/viz.rs` (`Response`)
- Modify: `chimera-core/src/preset.rs` (NOTE's default from the kind), `chimera-core/src/dsp/voice.rs` (`Voice::cost`)
- Modify tests: `flt_page_test.rs`, `filter_test.rs`, `big_viz_test.rs`, `part_page_test.rs`, `binding_test.rs`, `ui_test.rs`, `cost_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: Tasks 4, 8 and 14 (routes, presence, `Look`, `renderer::look`, `view::dimmed`); Task 13's SVF reading.
- Produces:
  - `dsp::filter::FilterKind { Svf = 0 }` (`Default`), with `BUILT: [FilterKind; 1]`, `modes(self) -> &'static [FilterMode]`, `key_default(self) -> i8`, `cost(self, FilterMode) -> Cost`, `from_index(f32) -> FilterKind`; `KIND_NAMES`; `kind_change(p: FilterParams, new: FilterKind) -> FilterParams`.
  - `FilterParams::{KIND = ParamId(6), kind(&self) -> FilterKind, set_kind(&mut self, FilterKind)}`; `set_mode` refuses a mode the kind lacks.
  - `ui::filter_panel::{PanelTarget { Filter(ParamId), Route(ModSource) }, PanelKnob { target, label }, KindPanel { main: [PanelKnob; 5], extras: [Option<PanelKnob>; 2] }, SVF_PANEL, panel(FilterKind) -> &'static KindPanel, knob(FilterKind, k: u8) -> Option<&'static PanelKnob>, applies(FilterKind, ParamId) -> bool}`.
  - `ui::view::{SlotCtx { sel_op: Op, kind: FilterKind }, SlotCtx::read(&impl Blocks, Op) -> SlotCtx, View { Empty, SelectOp, Legacy { label, fmt }, Param { addr, label, fmt }, Route { source, label } }, View::{label, fmt, addr}, view(&BlockDef, usize, &SlotCtx) -> View}`.
  - `SlotBinding::FilterPanel(u8)` (0–4 main, 5–6 extras); `block_def::slot_addr(def: &BlockDef, slot: usize, ctx: &SlotCtx) -> Option<ParamAddr>`.
  - `viz::Response { Low, High, Band, Notch }`, `viz::response_y(t, cutoff, reso, Response) -> i32`, `viz::filter(d, cutoff, reso, Response, readout)`.
  - `renderer::Frame.ctx: SlotCtx`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/flt_page_test.rs`:

```rust
use chimera_core::block::Block;
use chimera_core::dsp::filter::{FilterKind, FilterMode, kind_change};
use chimera_core::params::FILTER_SPECS;
use chimera_core::ui::filter_panel::{self, PanelTarget};

/// Spec § Tests "Kinds and modes", over every built kind.
#[test]
fn every_kind_keeps_its_mode_in_its_list() {
    for k in FilterKind::BUILT {
        assert!(!k.modes().is_empty(), "{k:?}");
        let mut p = FilterParams::default();
        p.set_kind(k);
        assert!(k.modes().contains(&p.mode()));
        for m in FilterMode::ALL {
            let took = p.set_mode(m);
            assert_eq!(took, k.modes().contains(&m), "{k:?} {m:?}");
            assert!(k.modes().contains(&p.mode()));
        }
        // A MODE write past the list lands on its last mode.
        p.set(FilterParams::MODE, 99.0);
        assert_eq!(p.mode(), *k.modes().last().unwrap(), "{k:?}");
        // A KIND change keeps MODE if the new kind has it, else its default.
        let q = kind_change(p, k);
        assert_eq!(q.mode(), p.mode());
    }
}

/// `applies` names exactly the filter parameters on the kind's panel.
#[test]
fn applies_matches_the_panel() {
    for k in FilterKind::BUILT {
        let p = filter_panel::panel(k);
        let on = |id| {
            p.main.iter().chain(p.extras.iter().flatten()).any(|n| n.target == PanelTarget::Filter(id))
        };
        for s in FILTER_SPECS.iter() {
            let want = on(s.id) || s.id == FilterParams::KIND || s.id == FilterParams::MODE;
            assert_eq!(filter_panel::applies(k, s.id), want, "{k:?} {:?}", s.id);
        }
    }
}

/// KIND is dimmed while the SVF is the one built kind: its encoder does
/// nothing, and a KIND "change" leaves the matrix byte-identical. (KIND is
/// an enum the registry already refuses to prime, so the dimming is
/// asserted on its own.)
#[test]
fn kind_is_fixed_and_never_edits_the_matrix() {
    use chimera_core::ui::view::{self, SlotCtx};
    let s = chimera_core::preset::Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    assert!(view::is_dimmed(&view::view(&chimera_core::ui::block_registry::FILTER, 0, &ctx), &s));
    let mut ui = on_flt(EngineType::Algo);
    let before = format!("{:?}", ui.mod_state());
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    assert_eq!(ui.params().filter.kind(), FilterKind::Svf);
    assert_eq!(format!("{:?}", ui.mod_state()), before);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert_eq!(ui.prime_status(), Some(PrimeStatus::NotModulatable));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test flt_page_test`
Expected: FAIL to compile: no `FilterKind`, `filter_panel`.

- [ ] **Step 3: `FilterKind`**

In `chimera-core/src/dsp/filter.rs`:

```rust
use crate::dsp::modulator::pick;
use crate::hw::Cost;

/// Which filter model (spec § Data model). A variant lands with its model
/// (#123–#127); until then only the SVF exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum FilterKind {
    #[default]
    Svf = 0,
}

/// KIND's names, as it steps the built kinds.
pub static KIND_NAMES: [&str; 1] = ["SVF"];

impl FilterKind {
    /// The kinds built, as KIND steps them.
    pub const BUILT: [FilterKind; 1] = [FilterKind::Svf];

    /// Never empty; `[0]` is the default (spec § 7).
    pub const fn modes(self) -> &'static [FilterMode] {
        match self {
            FilterKind::Svf => &SVF_MODES,
        }
    }

    /// NOTE → CUTOFF on a new Sound (spec § 2).
    pub const fn key_default(self) -> i8 {
        match self {
            FilterKind::Svf => 0,
        }
    }

    /// Cycles per sample over `CHAIN_COST` (spec § CPU). The SVF ran inside
    /// the chain `CHAIN_COST` was measured over, at LP24; this is the bench's
    /// SVF row (PHASER, its costliest mode) less 1 OP, billed for every mode.
    pub const fn cost(self, _mode: FilterMode) -> Cost {
        match self {
            FilterKind::Svf => Cost(SVF_COST),
        }
    }

    pub fn from_index(v: f32) -> Self {
        pick(&Self::BUILT, v)
    }
}

/// `p` with KIND `new`: MODE stays if `new` has it, else becomes `new`'s
/// default; nothing else moves (spec § 7).
pub fn kind_change(p: FilterParams, new: FilterKind) -> FilterParams {
    let mut q = p;
    q.set_kind_raw(new);
    if !new.modes().contains(&p.mode()) {
        q.set_mode(new.modes()[0]);
    }
    q
}
```

`hw::Cost` is `Copy` with a public field, so `Cost(SVF_COST)` is a const expression. Above `FilterKind`: `const SVF_COST: u32 = n;` with `n` the Task 13 reading `SVF − 1 OP` from `## Measured`, rounded up (0 if it reads 0 or below), and the doc `measured <date>, bench ROUTING SVF row, rev V at 480 MHz`.

- [ ] **Step 4: `FilterParams` by kind**

In `params.rs`:
- `FilterParams` gains `kind: FilterKind,` (private, doc: `/// Private: kept consistent with mode through set_kind and set_mode.`), default `FilterKind::Svf`; `pub const KIND: ParamId = ParamId(6);`
- the methods become:

```rust
    pub fn kind(&self) -> FilterKind {
        self.kind
    }

    /// Change KIND (spec § 7): MODE stays if the new kind has it.
    pub fn set_kind(&mut self, k: FilterKind) {
        *self = crate::dsp::filter::kind_change(*self, k);
    }

    /// Only `kind_change` calls this; MODE is fixed up there.
    pub(crate) fn set_kind_raw(&mut self, k: FilterKind) {
        self.kind = k;
    }

    /// Sets `m` if the kind has it; returns whether it did.
    pub fn set_mode(&mut self, m: FilterMode) -> bool {
        let ok = self.kind.modes().contains(&m);
        if ok {
            self.mode = m;
        }
        ok
    }
```

- `FILTER_SPECS` becomes `[ParamSpec; 5]` with `ParamSpec::choice(6, "KIND", ValFmt::Names(&KIND_NAMES), 0.0, 0.0),` before MODE (a choice among the one built kind);
- `get`: `Self::KIND => FilterKind::BUILT.iter().position(|&k| k == self.kind).unwrap_or(0) as f32,` and MODE's index is `self.kind.modes().iter().position(…)`; `write`: `Self::KIND => self.set_kind(FilterKind::from_index(v)),` and MODE writes `let m = self.kind.modes(); self.set_mode(m[(v.max(0.0) as usize).min(m.len() - 1)]);`.

In `block.rs`, `normalize` guards a one-choice spec:

```rust
    pub fn normalize(&self, v: f32) -> f32 {
        if self.max == self.min {
            return 0.0;
        }
        (v - self.min) / (self.max - self.min)
    }
```

- [ ] **Step 5: The panel data**

Create `chimera-core/src/ui/filter_panel.rs`:

```rust
//! Each filter KIND's panel as `const` data (spec § 6): FLT's knobs 2–6
//! and FLT › MODE's extras. A kind's row lands with its model.

use crate::block::ParamId;
use crate::dsp::filter::FilterKind;
use crate::modulation::ModSource;
use crate::params::FilterParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTarget {
    /// A Filter parameter.
    Filter(ParamId),
    /// The amount of the matrix route `source → CUTOFF`.
    Route(ModSource),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelKnob {
    pub target: PanelTarget,
    /// The original panel's word (spec § 6).
    pub label: &'static str,
}

pub struct KindPanel {
    /// Knobs 2–6 of FLT.
    pub main: [PanelKnob; 5],
    /// Slots 2–3 of FLT › MODE.
    pub extras: [Option<PanelKnob>; 2],
}

const fn filter(id: ParamId, label: &'static str) -> PanelKnob {
    PanelKnob { target: PanelTarget::Filter(id), label }
}

const fn route(s: ModSource, label: &'static str) -> PanelKnob {
    PanelKnob { target: PanelTarget::Route(s), label }
}

/// SVF: CUTOFF · RES · MODE · ENV · KEY; extras DRIVE and LFO.
pub static SVF_PANEL: KindPanel = KindPanel {
    main: [
        filter(FilterParams::CUTOFF, "CUTOFF"),
        filter(FilterParams::RESONANCE, "RES"),
        filter(FilterParams::MODE, "MODE"),
        route(ModSource::Env1, "ENV"),
        route(ModSource::Note, "KEY"),
    ],
    extras: [
        Some(filter(FilterParams::DRIVE, "DRIVE")),
        Some(route(ModSource::Lfo1, "LFO")),
    ],
};

pub fn panel(kind: FilterKind) -> &'static KindPanel {
    match kind {
        FilterKind::Svf => &SVF_PANEL,
    }
}

/// Knob `k`: 0–4 the main row, 5–6 the extras.
pub fn knob(kind: FilterKind, k: u8) -> Option<&'static PanelKnob> {
    let p = panel(kind);
    match k {
        0..=4 => Some(&p.main[k as usize]),
        5 | 6 => p.extras[k as usize - 5].as_ref(),
        _ => None,
    }
}

/// Whether the kind shows filter parameter `id` (KIND and MODE always):
/// "not shown, not applied" covers only the filter's own parameters.
pub fn applies(kind: FilterKind, id: ParamId) -> bool {
    let p = panel(kind);
    id == FilterParams::KIND
        || id == FilterParams::MODE
        || p.main
            .iter()
            .chain(p.extras.iter().flatten())
            .any(|k| k.target == PanelTarget::Filter(id))
}
```

(`pub mod filter_panel;` in `ui/mod.rs`.)

- [ ] **Step 6: Views**

Replace `chimera-core/src/ui/view.rs` with:

```rust
//! A page slot as it reads now (filter-routing spec § UI): params, the
//! filter kind's panel and route views resolve here against the
//! Sound, so pages, vizzes and encoders all read by address.

use crate::addr::{BlockRef, Blocks, Op, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::filter::FilterKind;
use crate::modulation::{ModSource, VCA};
use crate::params::{FilterParams, OutParams};
use crate::preset::Sound;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::filter_panel::{self, PanelKnob, PanelTarget};

/// What a page's panels resolve against (spec § UI "Slot binding").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotCtx {
    pub sel_op: Op,
    pub kind: FilterKind,
}

impl SlotCtx {
    /// Read from any `Blocks`: a Sound's params or a Part view.
    pub fn read(params: &impl Blocks, sel_op: Op) -> Self {
        let get = |b: BlockRef, id| params.block(b).map_or(0.0, |blk| blk.get(id));
        Self {
            sel_op,
            kind: FilterKind::from_index(get(BlockRef::Filter, FilterParams::KIND)),
        }
    }
}

/// A slot, resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Empty,
    SelectOp,
    Legacy { label: &'static str, fmt: ValFmt },
    /// A parameter; `dimmed` decides from the Sound whether it is inert.
    Param { addr: ParamAddr, label: &'static str, fmt: ValFmt },
    /// The route `source → CUTOFF` (spec § 6).
    Route { source: ModSource, label: &'static str },
}

impl View {
    pub fn label(&self) -> &'static str {
        match *self {
            View::Empty => "--",
            View::SelectOp => "OP",
            View::Legacy { label, .. } | View::Param { label, .. } | View::Route { label, .. } => label,
        }
    }

    pub fn fmt(&self) -> ValFmt {
        match *self {
            View::Empty => ValFmt::Uni,
            View::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            View::Legacy { fmt, .. } | View::Param { fmt, .. } => fmt,
            View::Route { .. } => ValFmt::Route,
        }
    }

    pub fn addr(&self) -> Option<ParamAddr> {
        match *self {
            View::Param { addr, .. } => Some(addr),
            _ => None,
        }
    }
}

fn param(addr: ParamAddr, label: &'static str, fmt: ValFmt) -> View {
    View::Param { addr, label, fmt }
}

/// Slot `i` of `def` as it reads under `ctx`.
pub fn view(def: &BlockDef, i: usize, ctx: &SlotCtx) -> View {
    let Some(slot) = def.params.get(i) else {
        return View::Empty;
    };
    match slot.binding {
        SlotBinding::Empty => View::Empty,
        SlotBinding::SelectOp => View::SelectOp,
        SlotBinding::Legacy { label, fmt } => View::Legacy { label, fmt },
        SlotBinding::Param(addr) => param(addr, slot.label(), slot.format()),
        SlotBinding::SelectedOp(id) => {
            param(ParamAddr::new(BlockRef::AlgoOp(ctx.sel_op), id), slot.label(), slot.format())
        }
        SlotBinding::FilterPanel(k) => match filter_panel::knob(ctx.kind, k) {
            None => View::Empty,
            Some(&PanelKnob { target: PanelTarget::Filter(id), label }) => {
                let addr = ParamAddr::new(BlockRef::Filter, id);
                param(addr, label, addr.spec().map_or(ValFmt::Uni, |s| s.fmt))
            }
            Some(&PanelKnob { target: PanelTarget::Route(source), label }) => {
                View::Route { source, label }
            }
        },
    }
}

/// A fixed or inapplicable slot draws dimmed, and its encoder is ignored
/// (spec § UI "Dimmed").
pub fn dimmed(addr: ParamAddr, sound: &Sound) -> bool {
    match (addr.block, addr.param) {
        // AMP's VEL under the Algo/Modal pass-through (spec § 5).
        (BlockRef::Out, OutParams::VCA_VEL) => sound.mod_state.routes_into(VCA) == 0,
        // KIND lists built kinds only; with one it is fixed.
        (BlockRef::Filter, FilterParams::KIND) => FilterKind::BUILT.len() == 1,
        // A single-mode kind shows its mode fixed (spec § 7).
        (BlockRef::Filter, FilterParams::MODE) => sound.params.filter.kind().modes().len() == 1,
        _ => false,
    }
}

/// The view is drawn dimmed and inert.
pub fn is_dimmed(v: &View, sound: &Sound) -> bool {
    matches!(*v, View::Param { addr, .. } if dimmed(addr, sound))
}
```

In `chimera-core/src/ui/block_def.rs`: remove `SlotBinding::Route` and `ParamSlot::route`, and add:

```rust
    /// Knob `k` of the Sound's filter KIND's panel (spec § 6): 0–4 FLT's
    /// knobs 2–6, 5–6 FLT › MODE's extras.
    FilterPanel(u8),
```

with `pub const fn filter_panel(k: u8) -> Self` (like `select_op`); `spec` and `label`/`format` treat `FilterPanel(_)` like `Empty` (views resolve it); `slot_addr` becomes:

```rust
/// The address slot `slot` of `def` edits under `ctx`: operator slots name
/// the selected operator, panel knobs the Sound's kind's parameter.
pub fn slot_addr(def: &BlockDef, slot: usize, ctx: &crate::ui::view::SlotCtx) -> Option<ParamAddr> {
    crate::ui::view::view(def, slot, ctx).addr()
}
```

- [ ] **Step 7: The FLT pages bind the panel**

In `block_registry.rs`:

```rust
/// FLT: KIND, then the kind's panel (spec § 6).
pub static FILTER: BlockDef = BlockDef {
    id: 10,
    name: "Filter",
    short: "FLT",
    layout: PageLayout::BigViz,
    viz: VizType::FilterResponse,
    params: [
        ParamSlot::param(BlockRef::Filter, FilterParams::KIND),
        ParamSlot::filter_panel(0),
        ParamSlot::filter_panel(1),
        ParamSlot::filter_panel(2),
        ParamSlot::filter_panel(3),
        ParamSlot::filter_panel(4),
    ],
};
```

`FILTER_MODE`'s slots become `[ParamSlot::param(BlockRef::Filter, FilterParams::MODE), ParamSlot::filter_panel(5), ParamSlot::filter_panel(6), EMPTY, EMPTY, EMPTY]`.

- [ ] **Step 8: Pages, renderer and input read views**

`chimera-core/src/ui/part_page.rs` builds the context itself and keeps its signatures:

```rust
use crate::ui::view::{SlotCtx, View, view};

pub fn read_values(def: &BlockDef, params: &impl Blocks, sel_op: Op) -> [f32; 6] {
    let ctx = SlotCtx::read(params, sel_op);
    core::array::from_fn(|i| match view(def, i, &ctx) {
        View::SelectOp => sel_op.index() as f32 / (Op::ALL.len() - 1) as f32,
        v => v
            .addr()
            .and_then(|a| Some(params.block(a.block)?.normalized(a.param)))
            .unwrap_or(0.0),
    })
}
```

  `apply_encoder` and `snap_encoder` take `let ctx = SlotCtx::read(&*params, *sel_op);` (or `sel_op`) and call `slot_addr(def, slot, &ctx)`.

`chimera-core/src/ui/renderer.rs`:
- `Frame` gains `pub ctx: crate::ui::view::SlotCtx,`; `UiState::frame` fills it with `SlotCtx::read(&self.performance.parts[self.active_part].sound.params, self.sel_op)`.
- Every `slot_addr(…, f.sel_op)` and `slot_addr(…, Op::A)` becomes `slot_addr(…, &f.ctx)`; `cell_mod_info` goes (its call becomes `v.addr().and_then(|a| f.matrix.mod_info_for(a))`).
- `look` resolves by view (drawing and the region key still share it):

```rust
pub fn look(f: &Frame, i: usize) -> components::Look {
    match view::view(f.def, i, &f.ctx) {
        View::Route { source, .. }
            if f.matrix.route(source.index(), crate::modulation::CUTOFF).is_none() =>
        {
            components::Look::Absent
        }
        v if view::is_dimmed(&v, &f.parts[f.active_part].sound) => components::Look::Dimmed,
        _ => components::Look::Live,
    }
}
```

- `draw_cells` loops over views:

```rust
        for i in 0..f.def.params.len() {
            let v = view::view(f.def, i, &f.ctx);
            if v == View::Empty {
                components::cell(display, i, top, None);
                continue;
            }
            let value = self.anim[i].current();
            let mut buf = FmtBuf::new();
            fmt::fmt_val(&mut buf, value, v.fmt());
            let c = components::Cell {
                label: v.label(),
                text: buf.as_str(),
                value,
                fmt: v.fmt(),
                active: i == f.focus,
                mod_amount: v.addr().and_then(|a| f.matrix.mod_info_for(a)),
                look: look(f, i),
            };
            components::cell(display, i, top, Some(&c));
        }
```

- `draw_focus` takes `let v = view::view(f.def, f.focus, &f.ctx);`, returns on `View::Empty`, uses `v.label()` and `v.fmt()`, and its absent check matches `View::Route { source, .. }`.
- `FilterResponse` reads CUTOFF, RES and MODE by address:

```rust
            VizType::FilterResponse => {
                use crate::params::FilterParams;
                let at = |id| {
                    let addr = crate::addr::ParamAddr::new(crate::addr::BlockRef::Filter, id);
                    (0..f.def.params.len())
                        .find(|&i| slot_addr(f.def, i, &f.ctx) == Some(addr))
                        .map_or(0.0, a)
                };
                let mode = f.parts[f.active_part].sound.params.filter.mode();
                let v = view::view(f.def, f.focus, &f.ctx);
                let mut buf = FmtBuf::new();
                fmt::fmt_val(&mut buf, a(f.focus), v.fmt());
                let readout = (v != View::Empty).then(|| (v.label(), buf.as_str()));
                viz::filter(
                    display,
                    at(FilterParams::CUTOFF),
                    at(FilterParams::RESONANCE),
                    viz::Response::of(mode),
                    readout,
                );
            }
```

`chimera-core/src/ui/viz.rs`:

```rust
/// Which side of the cutoff passes (the viz reads MODE, spec § UI).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Low,
    High,
    Band,
    Notch,
}

impl Response {
    pub fn of(m: crate::dsp::filter::FilterMode) -> Self {
        use crate::dsp::filter::FilterMode as M;
        match m {
            M::Hp24 => Response::High,
            M::Bp12 | M::Bp24 => Response::Band,
            M::Notch => Response::Notch,
            M::Lp6 | M::Lp12 | M::Lp24 | M::Phaser => Response::Low,
        }
    }
}

/// `filter_y` for each response: high-pass mirrors it about the cutoff,
/// band-pass takes both skirts, notch dips at the cutoff.
pub fn response_y(t: f32, cutoff: f32, reso: f32, r: Response) -> i32 {
    let low = filter_y(t, cutoff, reso);
    let high = filter_y(2.0 * cutoff - t, cutoff, reso);
    match r {
        Response::Low => low,
        Response::High => high,
        Response::Band => low.max(high),
        Response::Notch => {
            let d = ((t - cutoff) * 6.0).abs();
            if d < 0.5 {
                let dip = 0.5 + 0.5 * libm::cosf(d * 2.0 * core::f32::consts::PI);
                FILTER_PASS_Y + ((PLOT_BASE - FILTER_PASS_Y) as f32 * dip) as i32
            } else {
                FILTER_PASS_Y
            }
        }
    }
}
```

  `filter` gains a `response: Response` parameter after `reso`, and its curve closure calls `response_y(…, response)`. In `big_viz_test.rs`, the three `viz::filter(…)` calls pass `viz::Response::Low` (today's curve, so those pixels are unchanged).

`chimera-core/src/ui/mod.rs`:
- `fn ctx(&self) -> SlotCtx { SlotCtx::read(&self.performance.parts[self.active_part].sound.params, self.sel_op) }`;
- `current_param_addr` is `slot_addr(def, self.focused_slot(), &self.ctx()).map(prime_target)`; `update` uses `slot_addr(def, i, &ctx)` with `let ctx = self.ctx();` taken before borrowing `sound`;
- `display_values` overlays route views: `if let View::Route { source, .. } = view::view(def, i, &ctx) { values[i] = renderer::amount_value(self.matrix_state.route(source.index(), CUTOFF).unwrap_or(0)); }`;
- the encoder loop decides by view:

```rust
            let before = self.ctx();
            for (i, &enc) in encoder_ids.iter().enumerate() {
                let delta = controls.encoder_delta(enc);
                if delta == 0 {
                    continue;
                }
                let v = view::view(def, i, &self.ctx());
                if v != View::Empty {
                    self.focus.touch(def.id, i);
                }
                if let View::Route { source, .. } = v {
                    self.edit_route(source, |a| {
                        if shift {
                            snap_amount(a, delta)
                        } else {
                            (a as i16 + delta as i16).clamp(-127, 127) as i8
                        }
                    });
                    continue;
                }
                if view::is_dimmed(&v, &self.performance.parts[at].sound) {
                    continue; // dimmed: the encoder is ignored
                }
                let params = &mut self.performance.edit(at);
                match (self.page, shift) {
                    // … the four arms, unchanged …
                }
            }
            // A KIND, TYPE or MODE change re-seeds the page's animators: a
            // lerp between two parameters' values would draw a meaningless sweep.
            if self.ctx() != before {
                let values = self.display_values();
                self.renderer.snap_to_current(values);
            }
```

- the MIX+PLUS dimmed check uses `view::view(def, self.focused_slot(), &self.ctx())` and `view::is_dimmed`, keeping the rule that a slot whose prime target is itself reports NOT MODULATABLE.

- [ ] **Step 9: The kind's key default and cost**

In `preset.rs`, `Sound::init` sets NOTE's route to `crate::dsp::filter::FilterKind::default().key_default()` instead of `0` (untested while it is 0: the kind that brings a nonzero default adds the check). In `voice.rs`, `Voice::cost` becomes:

```rust
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        Engines::cost(p, mods)
            + Self::CHAIN_COST
            + p.filter.kind().cost(p.filter.mode())
            + ModRouting::cost(p, mods)
    }
```

- [ ] **Step 10: Update the tests**

- `binding_test.rs`, `part_page_test.rs` and `filter_test.rs` call `slot_addr(def, i, Op::A)` (or another operator); each passes `&SlotCtx::read(&ParamSnapshot::default(), Op::A)` instead (`use chimera_core::ui::view::SlotCtx;`).
- `cost_test.rs`: `assert_eq!(FilterKind::Svf.cost(FilterMode::Phaser), Cost(n))` with Task 13's `n`; the six-voice tests add `n` to their per-voice sums and must stay true (if a factory Sound falls below six on rev V, stop and tell the owner).
- `part_page_test.rs`, `drive_filter_folder_pages`: `read(&reg::FILTER, &p)` is `[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]` (KIND, CUTOFF, RES, MODE, then the route views, which `part_page` reads as 0; the UI overlays them); MODE is slot 3 as before.
- `binding_test.rs`, `part_pages_display_like_before`: a panel slot's own `label()`/`format()` are the empty ones, so the FILTER row checks views instead: build `let ctx = SlotCtx::read(&ParamSnapshot::default(), Op::A);` and compare `(view(&reg::FILTER, i, &ctx).label(), view(…).fmt())` with `("KIND", Names(&KIND_NAMES)), ("CUTOFF", Uni), ("RES", Uni), ("MODE", Names(&SVF_MODE_NAMES)), ("ENV", ValFmt::Route), ("KEY", ValFmt::Route)`. The other rows keep `ParamSlot::label()`/`format()`.
- `ui_test.rs`, `test_filter_mode_is_named_in_registry`: `assert!(matches!(view(&block_registry::FILTER, 3, &ctx).fmt(), ValFmt::Names(_)));`.
- `filter_test.rs`, `mode_is_on_the_flt_pages`: `slot_addr(def, i, &SlotCtx::read(&ParamSnapshot::default(), Op::A))`.
- `screen/mod.rs`: add after `bigviz_filter`:

```rust
    ("flt_mode", |ui| {
        plus(ui, 3);
        feed(ui, Input::press(ButtonId::Edit)); // FLT › MODE
        feed(ui, Input::turn(EncoderId::A, 3)); // MODE: BP12
    }),
```

  and its row in `GOLDENS`.

- [ ] **Step 11: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS except `screen_goldens_match` (`bigviz_filter`: slot a now reads KIND `SVF` dimmed, RES is labelled RES; `flt_mode` is new).

- [ ] **Step 12: Look at the screens and re-record**

Look at `bigviz_filter`: KIND · CUTOFF · RES / MODE · ENV · KEY, KIND's `SVF` in dim grey with no bar, the curve unchanged (LP24 is `Response::Low`). `flt_mode`: MODE `BP12` · DRIVE · LFO `0%` over the live-output band. Re-record both.

- [ ] **Step 13: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/dsp/filter.rs chimera-core/src/params.rs chimera-core/src/block.rs \
  chimera-core/src/ui/filter_panel.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_def.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/src/ui/part_page.rs chimera-core/src/ui/renderer.rs \
  chimera-core/src/ui/mod.rs chimera-core/src/ui/viz.rs chimera-core/src/preset.rs chimera-core/src/dsp/voice.rs \
  chimera-core/tests/flt_page_test.rs chimera-core/tests/filter_test.rs chimera-core/tests/big_viz_test.rs \
  chimera-core/tests/part_page_test.rs chimera-core/tests/binding_test.rs chimera-core/tests/ui_test.rs \
  chimera-core/tests/cost_test.rs chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "KIND and per-kind filter panels, with the SVF as the one kind; slots resolve through views"
```

---

### Task 16: The MOD node and the ENV pages

The MOD node's home becomes E1 (id 11, formerly ENVELOPE), and the matrix moves to the end of its sub-list. Each ENV page shows its TYPE's panel: A · D · S / R · H · TYPE, or MODE · RISE · FALL / SHAPE · FORM · TYPE, with MODE's and FORM's labels (spec § 1). The time, rate and shape cells read in milliseconds, hertz and percent. The ENV viz draws the TYPE's shape by address, and B's MODE shows as tabs. The map's MOD node keeps its "MOD" label. This task builds E1–E3; Task 17 adds SPD, L2 and L3.

**Files:**
- Create: `chimera-core/src/ui/mod_panel.rs` (`PanelSlot`, `ModPanel`, `env_panel`)
- Modify: `chimera-core/src/dsp/modulator/law.rs` (`Law`)
- Modify: `chimera-core/src/block.rs` (`ValFmt::Law`), `chimera-core/src/ui/fmt.rs` (`fmt_law`)
- Modify: `chimera-core/src/ui/view.rs` (`EnvKind`, `SlotCtx.envs`, `View::Text`, `EnvPanel` views)
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding::EnvPanel`, `ChainBlock.map`)
- Modify: `chimera-core/src/ui/block_registry.rs` (E1 on ENVELOPE, ENV_2 id 60, ENV_3 id 61, the MOD node, MOD_MATRIX short MTX; `map: None` on every other `ChainBlock`)
- Modify: `chimera-core/src/ui/dungeon_map.rs` (the node label), `chimera-core/src/ui/renderer.rs` (the ENV viz, the header's TYPE, `title_type`), `chimera-core/src/ui/region.rs` (`Header.title_type`), `chimera-core/src/ui/viz.rs` (`envelope` over slices, `func`, `func_shape`)
- Create: `chimera-core/tests/mod_pages_test.rs`
- Modify tests: `ui_test.rs`, `ui_routing_test.rs`, `prime_status_test.rs`, `preset_test.rs`, `block_def_tests.rs`, `header_map_test.rs`, `big_viz_test.rs`, `binding_test.rs`, `flt_page_test.rs`, `matrix_view_test.rs`, `focus_test.rs`, `region_tests.rs`, `screen/mod.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: `EnvParams` ids (Tasks 5–6), the laws (Tasks 5–6), `view` (Task 15).
- Produces:
  - `law::Law { Hold(EnvSpeed), Attack(EnvSpeed), DecRel(EnvSpeed), BTime, BRate, BurstRate, BurstLen, Phase, Pct, Curve, Tilt }` with `range(self) -> Option<Range>`; `ValFmt::Law(Law)`.
  - `ui::mod_panel::{PanelSlot { Param { id, label, fmt }, Fixed { label, text } }, ModPanel { slots: [Option<PanelSlot>; 6] }, env_panel(EnvKind) -> &'static ModPanel}`.
  - `ui::view::EnvKind { A(EnvSpeed), B(Func) }` (A's panel follows SPEED, B's its `Func`; nothing else is stored); `SlotCtx.envs: [EnvKind; 3]`; `View::Text { label, text }`.
  - `SlotBinding::EnvPanel(EnvSlot, u8)`, `ParamSlot::env_panel(EnvSlot, u8)` (const); `ChainBlock.map: Option<&'static str>`.
  - `block_registry::{ENVELOPE (E1), ENV_2, ENV_3}`.
  - `viz::{envelope(d, widths: &[f32], heights: &[f32], labels: &[&str], lit), stage_label_spans(xs: &[i32], labels: &[&str], lit) -> [Option<(i32, i32)>; MAX_STAGES], MAX_STAGES = 5, func(d, Func, rise, fall, shape), func_shape(Func, rise, fall, shape, t) -> f32}`.

- [ ] **Step 1: Write the failing tests**

Create `chimera-core/tests/mod_pages_test.rs`:

```rust
//! The MOD node's pages (filter-routing spec § UI).

mod screen;

use chimera_core::addr::Op;
use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, LfoForm};
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::block_registry::{ALGO_CHAIN, ENV_2, ENV_3, ENVELOPE, MOD_MATRIX};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::view::{SlotCtx, view};
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn labels(p: &ParamSnapshot, def: &chimera_core::ui::block_def::BlockDef) -> [&'static str; 6] {
    let ctx = SlotCtx::read(p, Op::A);
    core::array::from_fn(|i| view(def, i, &ctx).label())
}

#[test]
fn the_mod_node_is_e1_with_its_sub_list() {
    let node = ALGO_CHAIN.blocks.last().unwrap();
    assert_eq!(node.def.id, ENVELOPE.id);
    assert_eq!(node.map, Some("MOD"));
    let subs: Vec<&str> = core::iter::once(node.def)
        .chain(node.sub_pages.iter().copied())
        .map(|d| d.short)
        .collect();
    assert_eq!(subs.first(), Some(&"E1"));
    assert_eq!(subs.last(), Some(&"MTX"));
    assert_eq!(node.sub_pages.last().unwrap().id, MOD_MATRIX.id);
    assert_eq!((ENV_2.id, ENV_3.id), (60, 61));
}

#[test]
fn an_env_page_shows_its_types_panel() {
    let mut p = ParamSnapshot::default();
    assert_eq!(labels(&p, &ENVELOPE), ["ATTACK", "DECAY", "SUSTAIN", "RELEASE", "HOLD", "TYPE"]);
    assert_eq!(labels(&p, &ENV_3), ["MODE", "RISE", "FALL", "SHAPE", "FORM", "TYPE"]);
    for (f, want) in [
        (Func::Lfo(LfoForm::Free), ["MODE", "RATE", "PHASE", "TILT", "FORM", "TYPE"]),
        (Func::Lfo(LfoForm::Lfv), ["MODE", "RATE", "DELTA", "SLEW", "FORM", "TYPE"]),
        (Func::Burst(EnvForm::Ad), ["MODE", "RATE", "LENGTH", "TILT", "FORM", "TYPE"]),
    ] {
        p.envelopes[2].func.set_func(f);
        assert_eq!(labels(&p, &ENV_3), want, "{f:?}");
    }
    p.envelopes[0].env_type = EnvType::B;
    assert_eq!(labels(&p, &ENVELOPE)[0], "MODE", "any ENV slot can be B");
}

/// Times and rates read in their units (the approved mockups).
#[test]
fn slider_cells_read_in_units() {
    let p = ParamSnapshot::default();
    let ctx = SlotCtx::read(&p, Op::A);
    let text = |def, i, v| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, v, view(def, i, &ctx).fmt());
        b.as_str().to_owned()
    };
    assert_eq!(text(&ENVELOPE, 0, 0.189), "10 ms", "MED attack at its default");
    assert_eq!(text(&ENVELOPE, 1, 1.0), "10.0 s");
    assert_eq!(text(&ENVELOPE, 2, 0.55), "55%");
    assert_eq!(text(&ENVELOPE, 4, 0.0), "0.0 ms", "the integer formatter pads");
    assert_eq!(text(&ENV_3, 3, 0.5), "LIN");
    assert_eq!(text(&ENV_3, 3, 0.8), "EXP 60");
}

/// A TYPE flip redraws the header, whose title reads `ENV 1 / B` now.
#[test]
fn a_type_flip_redraws_the_title() {
    use chimera_core::ui::page::PageLayout;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionKind, layout_regions};
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // TYPE → B
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    let &(_, y0, y1) = layout_regions(PageLayout::BigViz)
        .iter()
        .find(|r| r.0 == RegionKind::Header)
        .unwrap();
    assert!(flushed.contains(&(y0, y1)), "{flushed:?}");
}

/// A TYPE change re-seeds the animators: the cells jump to the new
/// panel's values instead of sweeping from the old ones.
#[test]
fn a_type_change_reseeds_the_page() {
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // TYPE → B
    ui.update();
    let rise = ui.params().envelopes[0].func.rise;
    assert_eq!(ui.renderer.anim[1].current(), rise, "RISE shown at once");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test mod_pages_test`
Expected: FAIL to compile: no `ENV_2`, `ChainBlock.map`, `SlotCtx.envs`.

- [ ] **Step 3: Readouts in units**

Append to `law.rs`:

```rust
/// How a slider cell reads (the approved mockups): a time or rate on its
/// range, or a percentage, phase or bend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Law {
    Hold(EnvSpeed),
    Attack(EnvSpeed),
    DecRel(EnvSpeed),
    BTime,
    BRate,
    BurstRate,
    BurstLen,
    Phase,
    Pct,
    Curve,
    Tilt,
}

impl Law {
    pub fn range(self) -> Option<Range> {
        Some(match self {
            Law::Hold(s) => speed_ranges(s).hold,
            Law::Attack(s) => speed_ranges(s).attack,
            Law::DecRel(s) => speed_ranges(s).dec_rel,
            Law::BTime => B_TIME,
            Law::BRate => B_RATE,
            Law::BurstRate => BURST_RATE,
            Law::BurstLen => BURST_LEN,
            Law::Phase | Law::Pct | Law::Curve | Law::Tilt => return None,
        })
    }
}
```

In `block.rs`, `ValFmt` gains `/// A slider shown in its unit (spec § 1's laws).\n    Law(crate::dsp::modulator::law::Law),`; `snap_points` gives `Law(_)` the `Uni` points (`ValFmt::Uni | ValFmt::Law(_) => …`). `is_bipolar` and `is_discrete` are false for it, and `max_int` takes the `_` arm.

In `ui/fmt.rs`, `fmt_val` gains `ValFmt::Law(law) => fmt_law(buf, val, law),` and:

```rust
/// `x ≥ 0` to `places` decimals in integers: `{:.1}` would link core's
/// float formatting, several KB of flash.
fn fixed(buf: &mut FmtBuf, x: f32, places: u32, unit: &str) {
    use core::fmt::Write;
    let k = 10i32.pow(places);
    let n = libm::roundf(x * k as f32) as i32;
    let _ = write!(buf, "{}.{:02$} {unit}", n / k, n % k, places as usize);
}

fn fmt_law(buf: &mut FmtBuf, v: f32, law: crate::dsp::modulator::law::Law) {
    use crate::dsp::modulator::law::Law;
    use core::fmt::Write;
    let round = |x: f32| libm::roundf(x) as i32;
    let bend = |buf: &mut FmtBuf, lo: &str, mid: &str, hi: &str| {
        let n = round((2.0 * v - 1.0) * 100.0);
        let _ = match n {
            0 => buf.write_str(mid),
            n if n < 0 => write!(buf, "{lo} {}", -n),
            n => write!(buf, "{hi} {n}"),
        };
    };
    match law {
        Law::Pct => {
            let _ = write!(buf, "{}%", round(v * 100.0));
        }
        // Degrees; the u8g2 face has no "°".
        Law::Phase => {
            let _ = write!(buf, "{}", round(v * 360.0));
        }
        Law::Curve => bend(buf, "LOG", "LIN", "EXP"),
        Law::Tilt => bend(buf, "SAW", "TRI", "RAMP"),
        Law::BRate | Law::BurstRate => {
            let hz = law.range().map_or(0.0, |r| r.at(v));
            if hz < 10.0 {
                fixed(buf, hz, 2, "Hz");
            } else if hz < 100.0 {
                fixed(buf, hz, 1, "Hz");
            } else {
                let _ = write!(buf, "{} Hz", round(hz));
            }
        }
        _ => {
            let s = law.range().map_or(0.0, |r| r.at(v));
            if s >= 1.0 {
                fixed(buf, s, 1, "s");
            } else if s >= 0.01 {
                let _ = write!(buf, "{} ms", round(s * 1000.0));
            } else {
                fixed(buf, s * 1000.0, 1, "ms");
            }
        }
    }
}
```

- [ ] **Step 4: The ENV panels**

Create `chimera-core/src/ui/mod_panel.rs`:

```rust
//! A modulator slot's page, per TYPE, MODE and FORM (spec § 1 "Page",
//! § UI), as `const` data.

use crate::block::{ParamId, ValFmt};
use crate::dsp::modulator::law::Law;
use crate::dsp::modulator::{EnvSpeed, Func, LfoForm};
use crate::params::EnvParams as E;
use crate::ui::view::EnvKind;

/// One cell of a modulator page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSlot {
    /// A parameter of the slot's own block, with its label and readout.
    Param { id: ParamId, label: &'static str, fmt: ValFmt },
    /// A fixed readout: dimmed and inert (FUNC's MODE, always LFO).
    Fixed { label: &'static str, text: &'static str },
}

pub struct ModPanel {
    pub slots: [Option<PanelSlot>; 6],
}

const fn p(id: ParamId, label: &'static str, fmt: ValFmt) -> Option<PanelSlot> {
    Some(PanelSlot::Param { id, label, fmt })
}

const TYPE: Option<PanelSlot> = p(E::TYPE, "TYPE", ValFmt::Names(&["A", "B"]));
const MODE: Option<PanelSlot> = p(E::MODE, "MODE", ValFmt::Names(&["ENV", "LFO", "BURST"]));
/// In `EnvForm::ALL`'s order.
const FORM_ENV: Option<PanelSlot> = p(E::FORM, "FORM", ValFmt::Names(&["AD", "AHR", "CYCLE"]));
/// In `LfoForm::ALL`'s order.
const FORM_LFO: Option<PanelSlot> = p(E::FORM, "FORM", ValFmt::Names(&["FREE", "SYNC", "LFV"]));

/// A · D · S / R · H · TYPE, times on SPEED's ranges.
const fn a(s: EnvSpeed) -> ModPanel {
    ModPanel {
        slots: [
            p(E::ATTACK, "ATTACK", ValFmt::Law(Law::Attack(s))),
            p(E::DECAY, "DECAY", ValFmt::Law(Law::DecRel(s))),
            p(E::SUSTAIN, "SUSTAIN", ValFmt::Law(Law::Pct)),
            p(E::RELEASE, "RELEASE", ValFmt::Law(Law::DecRel(s))),
            p(E::HOLD, "HOLD", ValFmt::Law(Law::Hold(s))),
            TYPE,
        ],
    }
}

static A_FAST: ModPanel = a(EnvSpeed::Fast);
static A_MED: ModPanel = a(EnvSpeed::Med);
static A_SLOW: ModPanel = a(EnvSpeed::Slow);

/// MODE · RISE · FALL / SHAPE · FORM · TYPE, labels by MODE and FORM.
static B_ENV: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RISE", ValFmt::Law(Law::BTime)),
        p(E::FALL, "FALL", ValFmt::Law(Law::BTime)),
        p(E::SHAPE, "SHAPE", ValFmt::Law(Law::Curve)),
        FORM_ENV,
        TYPE,
    ],
};
/// FREE and SYNC (RISE reads RATE until #44 gives SYNC a clock).
static B_LFO: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(E::FALL, "PHASE", ValFmt::Law(Law::Phase)),
        p(E::SHAPE, "TILT", ValFmt::Law(Law::Tilt)),
        FORM_LFO,
        TYPE,
    ],
};
static B_LFV: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(E::FALL, "DELTA", ValFmt::Law(Law::Pct)),
        p(E::SHAPE, "SLEW", ValFmt::Law(Law::Pct)),
        FORM_LFO,
        TYPE,
    ],
};
static B_BURST: ModPanel = ModPanel {
    slots: [
        MODE,
        p(E::RISE, "RATE", ValFmt::Law(Law::BurstRate)),
        p(E::FALL, "LENGTH", ValFmt::Law(Law::BurstLen)),
        p(E::SHAPE, "TILT", ValFmt::Law(Law::Tilt)),
        FORM_ENV,
        TYPE,
    ],
};

pub fn env_panel(k: EnvKind) -> &'static ModPanel {
    match k {
        EnvKind::A(EnvSpeed::Fast) => &A_FAST,
        EnvKind::A(EnvSpeed::Med) => &A_MED,
        EnvKind::A(EnvSpeed::Slow) => &A_SLOW,
        EnvKind::B(Func::Env(_)) => &B_ENV,
        EnvKind::B(Func::Lfo(LfoForm::Lfv)) => &B_LFV,
        EnvKind::B(Func::Lfo(_)) => &B_LFO,
        EnvKind::B(Func::Burst(_)) => &B_BURST,
    }
}
```

(`pub mod mod_panel;` in `ui/mod.rs`.)

- [ ] **Step 5: Views of ENV slots**

In `view.rs`:

```rust
use crate::dsp::modulator::{EnvForm, EnvSlot, EnvSpeed, EnvType, Func, FuncMode, LfoForm, pick};
use crate::params::EnvParams;
use crate::ui::mod_panel::{self, PanelSlot};

/// What an ENV slot's page resolves against: type A's panel follows its
/// SPEED, type B's its MODE and that MODE's FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvKind {
    A(EnvSpeed),
    B(Func),
}

/// B's `Func` from its MODE and FORM values (FORM indexes MODE's list).
fn func_at(mode: f32, form: f32) -> Func {
    match pick(&FuncMode::ALL, mode) {
        FuncMode::Env => Func::Env(pick(&EnvForm::ALL, form)),
        FuncMode::Lfo => Func::Lfo(pick(&LfoForm::ALL, form)),
        FuncMode::Burst => Func::Burst(pick(&EnvForm::ALL, form)),
    }
}
```

`SlotCtx` gains `pub envs: [EnvKind; 3],`, read in `SlotCtx::read` as:

```rust
            envs: EnvSlot::ALL.map(|s| {
                let at = |id| get(BlockRef::Env(s), id);
                match pick(&EnvType::ALL, at(EnvParams::TYPE)) {
                    EnvType::A => EnvKind::A(pick(&EnvSpeed::ALL, at(EnvParams::SPEED))),
                    EnvType::B => EnvKind::B(func_at(at(EnvParams::MODE), at(EnvParams::FORM))),
                }
            }),
```

`View` gains `/// A fixed readout, dimmed and inert.\n    Text { label: &'static str, text: &'static str },` (`label()` returns its label, `fmt()` gives `ValFmt::Uni`, `addr()` `None`), and `view` gains:

```rust
        SlotBinding::EnvPanel(s, k) => match mod_panel::env_panel(ctx.envs[s.index()]).slots[k as usize] {
            None => View::Empty,
            Some(PanelSlot::Param { id, label, fmt }) => {
                param(ParamAddr::new(BlockRef::Env(s), id), label, fmt)
            }
            Some(PanelSlot::Fixed { label, text }) => View::Text { label, text },
        },
```

In `block_def.rs`, `SlotBinding` gains `/// Cell k of ENV slot s's page, per its TYPE, MODE and FORM.\n    EnvPanel(crate::dsp::modulator::EnvSlot, u8),` and `ParamSlot::env_panel(s, k)` (const); `ParamSlot::spec`, `label` and `format` treat `EnvPanel(..)` as they treat `FilterPanel(_)` (views resolve it). `ChainBlock` gains:

```rust
    /// The map's label for this node when it isn't the home page's short
    /// (the MOD node's home is E1).
    pub map: Option<&'static str>,
```

In `renderer.rs`, `draw_cells` and `draw_focus` draw a `View::Text` as a dimmed cell whose text is its `text`. In `ui/mod.rs`, the encoder loop skips `View::Text` like a dimmed slot.

- [ ] **Step 6: The node and the pages**

In `block_registry.rs`: add `map: None,` to every `ChainBlock` literal (`sed -i 's/^\(\s*\)sub_pages: \(.*\),$/\1sub_pages: \2,\n\1map: None,/' chimera-core/src/ui/block_registry.rs`, then hand-set the MOD nodes). Then:

```rust
const fn env_page(id: u16, name: &'static str, short: &'static str, s: EnvSlot) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::BigViz,
        viz: VizType::Adsr,
        params: [
            ParamSlot::env_panel(s, 0),
            ParamSlot::env_panel(s, 1),
            ParamSlot::env_panel(s, 2),
            ParamSlot::env_panel(s, 3),
            ParamSlot::env_panel(s, 4),
            ParamSlot::env_panel(s, 5),
        ],
    }
}

/// E1: the MOD node's home (id 11, once the amp envelope's page).
pub static ENVELOPE: BlockDef = env_page(11, "Env 1", "E1", EnvSlot::Env1);
pub static ENV_2: BlockDef = env_page(60, "Env 2", "E2", EnvSlot::Env2);
pub static ENV_3: BlockDef = env_page(61, "Env 3", "E3", EnvSlot::Env3);

/// The MOD node's sub-list after its home E1 (spec § UI).
static MOD_SUB_PAGES: [&BlockDef; 4] = [&ENV_2, &ENV_3, &LFO, &MOD_MATRIX];
```

`MOD_MATRIX`'s short becomes `"MTX"`; delete `MOD_MATRIX_SUB_PAGES`. Every MOD node (`KICK_BLOCKS`, `MODAL_PLUCK_BLOCKS`, `ALGO_BLOCKS`) becomes `ChainBlock { def: &ENVELOPE, sub_pages: &MOD_SUB_PAGES, map: Some("MOD") }`.

In `dungeon_map.rs`, both node label reads (`pill_node(…, block.def.short, …)` and `ring_node(…, block.def.short, …)`) use `block.map.unwrap_or(block.def.short)`. The sub-list keeps each def's short.

- [ ] **Step 7: The ENV viz and the page title**

In `viz.rs`, `envelope` and `stage_label_spans` take slices of up to `MAX_STAGES` segments:

```rust
/// Most stages `envelope` draws: A · H · D · S · R.
pub const MAX_STAGES: usize = 5;
```

  `envelope(d, widths: &[f32], heights: &[f32], labels: &[&str], lit: Option<usize>)`: `n = widths.len().min(MAX_STAGES)`, `pts: [(i32, i32); MAX_STAGES + 1]` filled for `0..=n`, every `4`/`5` in its body becomes `n`/`n + 1`, and it passes `&xs[..=n]`, `&labels[..n]` on. `stage_label_spans(xs: &[i32], labels: &[&str], lit) -> [Option<(i32, i32)>; MAX_STAGES]` uses `n = labels.len().min(MAX_STAGES)` in place of 4. In `big_viz_test.rs`, the arrays passed coerce as they are; a comparison of the returned spans compares `spans[..4]`.

Add B's picture:

```rust
/// Envelope B's shape for the viz: 0..1 across `t` (0..1). ENV: one
/// rise-and-fall (AHR holds a quarter; CYCLE twice); LFO: three cycles, or
/// a fixed walk for LFV; BURST: eight pulses under the burst.
pub fn func_shape(f: Func, rise: f32, fall: f32, shape: f32, t: f32) -> f32 {
    use crate::dsp::modulator::law::{B_TIME, curve, shape_w, tilt};
    let frac = |x: f32| x - (x as u32) as f32;
    match f {
        Func::Env(e) => {
            let (cycles, hold) = match e {
                EnvForm::Cycle => (2.0, 0.0),
                EnvForm::Ahr => (1.0, 0.25),
                EnvForm::Ad => (1.0, 0.0),
            };
            let x = frac(t * cycles);
            let (tr, tf) = (B_TIME.at(rise), B_TIME.at(fall));
            let r = tr / (tr + tf) * (1.0 - hold);
            let w = shape_w(shape);
            if x < r {
                curve(x / r, w)
            } else if x < r + hold {
                1.0
            } else {
                1.0 - curve((x - r - hold) / (1.0 - r - hold), w)
            }
        }
        Func::Lfo(LfoForm::Lfv) => {
            const WALK: [f32; 9] = [0.0, 0.7, -0.4, 0.9, -0.8, 0.3, -0.2, 0.6, -0.5];
            let x = t * 8.0;
            let k = (x as usize).min(7);
            let u = x - k as f32;
            let at = |i: usize| 0.5 + 0.5 * (WALK[i] * fall.max(0.2)).clamp(-1.0, 1.0);
            let lin = at(k) + (at(k + 1) - at(k)) * u;
            // SLEW rounds each corner toward the segment's middle.
            let mid = 0.5 * (at(k) + at(k + 1));
            lin + (mid - lin) * shape * (1.0 - (2.0 * u - 1.0).abs())
        }
        Func::Lfo(_) => tilt(frac(t * 3.0 + fall), shape),
        Func::Burst(e) => {
            let p = frac(t * 8.0);
            let pulse = if e == EnvForm::Cycle {
                tilt(p, shape)
            } else {
                let m = 1.0 - (2.0 * shape - 1.0).abs();
                let square = if p < 0.5 { 1.0 } else { 0.0 };
                (1.0 - m) * square + m * (0.5 - 0.5 * libm::cosf(core::f32::consts::TAU * p))
            };
            tilt(t, shape) * pulse
        }
    }
}

/// The B tabs' baseline, above the curve.
pub const TAB_Y: i32 = 48;

/// B's viz: ENV · LFO · BURST tabs (MODE lit), then the shape, filled.
pub fn func<D>(d: &mut D, f: Func, rise: f32, fall: f32, shape: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, name) in ["ENV", "LFO", "BURST"].iter().enumerate() {
        let x = theme::VIZ_LEFT + i as i32 * 52;
        let on = i == f.mode() as usize;
        if on {
            draw::pill(d, x, TAB_Y - 11, 46, 14, theme::ACCENT);
        } else {
            draw::round_outline(d, x, TAB_Y - 11, 46, 14, 7, theme::FAINT);
        }
        let color = if on { theme::BG } else { theme::MID };
        draw::text_center(d, &theme::FONT_LABEL_BOLD, name, x + 23, TAB_Y, color, 0);
    }
    let top = TAB_Y + 8;
    let (w, h) = ((theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32, (PLOT_BASE - top) as f32);
    let y = |x: i32| {
        let t = (x - theme::VIZ_LEFT) as f32 / w;
        PLOT_BASE - (h * func_shape(f, rise, fall, shape, t).clamp(0.0, 1.0)) as i32
    };
    filled_curve(d, theme::VIZ_LEFT, theme::VIZ_RIGHT, PLOT_BASE, y);
}
```

(imports `crate::dsp::modulator::{EnvForm, Func, LfoForm}`; `libm::cosf` in the UI is allowed.)

In `renderer.rs`, the `VizType::Adsr` arm reads its values by address from the page's own slots:

```rust
            VizType::Adsr => {
                let Some(SlotBinding::EnvPanel(s, _)) = f.def.params.first().map(|p| p.binding)
                else {
                    return;
                };
                let blk = crate::addr::BlockRef::Env(s);
                let at = |id| {
                    let addr = crate::addr::ParamAddr::new(blk, id);
                    (0..f.def.params.len()).find(|&i| slot_addr(f.def, i, &f.ctx) == Some(addr)).map(a)
                };
                use crate::params::EnvParams as E;
                let p = &f.parts[f.active_part].sound.params.envelopes[s.index()];
                match f.ctx.envs[s.index()] {
                    EnvKind::A(_) => {
                        let hold = if p.hold_pos == HoldPos::Ahdsr { at(E::HOLD).unwrap_or(0.0) } else { 0.0 };
                        let (atk, dec, sus, rel) = (
                            at(E::ATTACK).unwrap_or(0.0).max(0.02),
                            at(E::DECAY).unwrap_or(0.0).max(0.02),
                            at(E::SUSTAIN).unwrap_or(0.0),
                            at(E::RELEASE).unwrap_or(0.0).max(0.02),
                        );
                        let total = atk + hold + dec + 0.3 + rel;
                        let focus = view::view(f.def, f.focus, &f.ctx).addr().map(|x| x.param);
                        let lit = match focus {
                            Some(E::ATTACK) => Some(0),
                            Some(E::HOLD) => Some(1),
                            Some(E::DECAY) => Some(2),
                            Some(E::SUSTAIN) => Some(3),
                            Some(E::RELEASE) => Some(4),
                            _ => None,
                        };
                        viz::envelope(
                            display,
                            &[atk / total, hold / total, dec / total, 0.3 / total, rel / total],
                            &[0.0, 1.0, 1.0, sus, sus, 0.0],
                            &["A", "H", "D", "S", "R"],
                            lit,
                        );
                    }
                    EnvKind::B(func) => {
                        viz::func(
                            display,
                            func,
                            at(E::RISE).unwrap_or(p.func.rise),
                            at(E::FALL).unwrap_or(p.func.fall),
                            at(E::SHAPE).unwrap_or(p.func.shape),
                        );
                    }
                }
            }
```

(`use crate::ui::view::EnvKind;` in `renderer.rs`. `return` skips the prime-status line only on a malformed page; place the status draw before the `match` if you prefer.) In `renderer.rs`, one function names the title's TYPE suffix, for drawing and for the header's region key:

```rust
/// The ENV page title's TYPE suffix: 0 none, 1 A, 2 B.
pub fn title_type(f: &Frame) -> u8 {
    match f.def.params.first().map(|p| p.binding) {
        Some(SlotBinding::EnvPanel(s, _)) => match f.ctx.envs[s.index()] {
            EnvKind::A(_) => 1,
            EnvKind::B(_) => 2,
        },
        _ => 0,
    }
}
```

and `draw_header` appends it after `components::header_text`:

```rust
        let (context, mut name) = components::header_text(f.nav, f.def);
        if let Some(ty) = ["", " / A", " / B"].get(title_type(f) as usize) {
            let _ = core::fmt::Write::write_str(&mut name, ty);
        }
```

The header redraws only when its region key changes, so the suffix joins the key. Otherwise a TYPE flip would keep the old letter until the load figure next moved. In `region.rs`, `Header` gains `title_type: u8` (sentinel `u8::MAX`), and `RegionData::header` takes it as a sixth argument. `region_data` passes `renderer::title_type(f)`. `region_tests.rs`'s five `RegionData::header(…)` calls add `, 0`.

- [ ] **Step 8: Update the tests for the new node**

E1 as the node's home turns red every test that reached the matrix, or a MOD sub-page, by PLUS alone. Each is listed here with its new path (the matrix is sub-page 4, LFO sub-page 3, and MINUS leaves the node from any sub-page, so the ways back are unchanged):

- `ui_test.rs`, `test_page_from_nav_part_chain`: node 5 is `reg::ENVELOPE`; `sub_page = 1` is `reg::ENV_2`, `sub_page = 2` is `reg::ENV_3`, `sub_page = 3` is `reg::LFO`.
- `prime_status_test.rs` (`mix_plus_on_a_non_modulatable_param_reports_not_modulatable`), `ui_routing_test.rs` (`priming_a_non_modulatable_param_is_refused`) and `preset_test.rs` (`priming_on_the_lfo_sub_page_registers_nothing`) press EDIT three times to reach the LFO page.
- EDIT ×4 after the PLUS presses, to reach MTX:
  - `ui_routing_test.rs`'s `set_first_amount` (after the five PLUS) and `enter_matrix` (after its three);
  - `flt_page_test.rs`'s `to_matrix` (its doc: "the MOD node, then EDIT ×4 to the matrix");
  - `matrix_view_test.rs`'s `an_empty_matrix_says_so`;
  - `focus_test.rs`'s `mixer_part_and_matrix_pages_use_the_same_mechanism`.
- `prime_status_test.rs`: `envelope_page` drops its EDIT press (E1 is the home; its doc reads "The MOD node's home, E1"), and Task 9's `stage_cells_prime_time_and_sustain_primes_level` drops its `// the ENV 1 page` EDIT press.
- `screen/mod.rs`, `amp_vel_live`: after `plus(ui, 1)`, four `feed(ui, Input::press(ButtonId::Edit));` (MTX), so E2 → VCA is made and VEL is live.
- `block_def_tests.rs`, `algo_chain_is_alg_osc_then_the_voice_chain`: the labels are `chain.blocks.iter().map(|b| b.map.unwrap_or(b.def.short))`, still `["ALG", "OSC", "DRV", "FLT", "AMP", "MOD"]`; `chain.blocks[5].sub_pages.len()` is 4.
- `header_map_test.rs`: the node labels read `b.map.unwrap_or(b.def.short)`.
- `binding_test.rs`: `block_def_ids_are_unique` gains `&reg::ENV_2, &reg::ENV_3`; the ENVELOPE row of `part_pages_display_like_before` compares views (as FILTER's does): `("ATTACK", Law(Attack(Med))), ("DECAY", Law(DecRel(Med))), ("SUSTAIN", Law(Pct)), ("RELEASE", Law(DecRel(Med))), ("HOLD", Law(Hold(Med))), ("TYPE", Names(&["A", "B"]))`.
- `screen/mod.rs`: `bigviz_env` becomes `("env_a", |ui| { plus(ui, 5); feed(ui, Input::turn(EncoderId::B, 6)); })` (E1 is the home now); `mod_matrix` presses EDIT four times after its last PLUS (the matrix is the fourth sub-page); and nine B cases on E3 follow, each starting `plus(ui, 5); feed(ui, Input::press(ButtonId::Edit)); feed(ui, Input::press(ButtonId::Edit));` (E3, type B, ENV · AD by default), then:

| Case | Then |
|---|---|
| `env_b_env_ad` | `turn(B, 10)` (RISE) |
| `env_b_env_ahr` | `turn(E, 1)` |
| `env_b_env_cycle` | `turn(E, 2)` |
| `env_b_lfo_free` | `turn(A, 1)` |
| `env_b_lfo_sync` | `turn(A, 1)`, `turn(E, 1)` |
| `env_b_lfo_lfv` | `turn(A, 1)`, `turn(E, 2)` |
| `env_b_burst_ad` | `turn(A, 2)` |
| `env_b_burst_ahr` | `turn(A, 2)`, `turn(E, 1)` |
| `env_b_burst_cycle` | `turn(A, 2)`, `turn(E, 2)` |

  (`turn(X, n)` is `feed(ui, Input::turn(EncoderId::X, n))`), with their rows in `GOLDENS`, and `bigviz_env`'s row renamed `env_a`.

- [ ] **Step 9: Run the tests**

Run: `env $T cargo test -p chimera-core`
Expected: PASS except `screen_goldens_match`.

- [ ] **Step 10: Look at the screens and re-record**

- `env_a`: header `ENV 1 / A`, the A · H · D · S · R curve with DECAY's segment lit, cells ATTACK `10 ms` · DECAY (its value in ms) · SUSTAIN `70%` / RELEASE · HOLD `0.0 ms` · TYPE `A`. The map's last node reads MOD, and its sub-list E1 E2 E3 L1 MTX has E1 lit.
- `env_b_env_ad`, `_ahr`, `_cycle`: header `ENV 3 / B`, the ENV tab lit, one rise-and-fall (AHR with a plateau; CYCLE twice), cells MODE `ENV` · RISE · FALL / SHAPE `LIN` · FORM `AD`/`AHR`/`CYCLE` · TYPE `B`.
- `env_b_lfo_free`, `_sync`: the LFO tab lit, three triangle cycles, cells RATE · PHASE · TILT `TRI`. `_lfv`: the walk; DELTA and SLEW.
- The three BURST screens: pulses under a triangle, cells RATE · LENGTH · TILT.
- `mod_matrix`: as before, with MTX lit in the sub-list.
- `amp_vel_live`: must still show VEL drawn live with its bar (its golden would otherwise lock a dimmed VEL under a name that says live). `amp_vel_dimmed` and `modal_amp`: unchanged but for the map's sub-list.

Nothing may overlap the tabs, the header or the map. Re-record the mismatches and the new cases.

- [ ] **Step 11: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/ui/mod_panel.rs chimera-core/src/dsp/modulator/law.rs chimera-core/src/block.rs \
  chimera-core/src/ui/fmt.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_def.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/src/ui/dungeon_map.rs chimera-core/src/ui/renderer.rs \
  chimera-core/src/ui/viz.rs chimera-core/src/ui/mod.rs \
  chimera-core/tests/mod_pages_test.rs chimera-core/tests/ui_test.rs chimera-core/tests/ui_routing_test.rs \
  chimera-core/tests/prime_status_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/block_def_tests.rs \
  chimera-core/tests/header_map_test.rs chimera-core/tests/big_viz_test.rs chimera-core/tests/binding_test.rs \
  chimera-core/tests/flt_page_test.rs chimera-core/tests/matrix_view_test.rs chimera-core/tests/focus_test.rs \
  chimera-core/tests/region_tests.rs chimera-core/src/ui/region.rs \
  chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "The MOD node's ENV pages: each slot's TYPE panel, its shape and units"
```

---

### Task 17: The SPD page and the LFO pages

This task adds the rest of the MOD sub-list: SPD (id 62), a shared page for type A's switches, one parameter pair per slot (spec § UI), and LFO 1–3 (ids 12, 64, 65), each showing its CLASSIC or FUNC panel with TYPE in the last cell. The sub-list becomes E1 · E2 · E3 · SPD · L1 · L2 · L3 · MTX. A type-B slot's SPD cells are dimmed. `MAX_PAGES` grows to 72 because L2 and L3 take ids 64 and 65.

**Files:**
- Modify: `chimera-core/src/ui/mod_panel.rs` (`lfo_panel`)
- Modify: `chimera-core/src/ui/view.rs` (`LfoKind`, `SlotCtx.lfos`, `LfoPanel` views, SPD dimming)
- Modify: `chimera-core/src/ui/block_def.rs` (`SlotBinding::LfoPanel`, `VizType::EnvSpeed`)
- Modify: `chimera-core/src/ui/block_registry.rs` (`ENV_SPEED` 62, `LFO` as L1, `LFO_2` 64, `LFO_3` 65, `MOD_SUB_PAGES` of 7)
- Modify: `chimera-core/src/ui/focus.rs` (`MAX_PAGES = 72`)
- Modify: `chimera-core/src/ui/viz.rs` (`env_speed`), `chimera-core/src/ui/renderer.rs` (the SPD viz)
- Modify tests: `mod_pages_test.rs`, `ui_test.rs`, `ui_routing_test.rs`, `prime_status_test.rs`, `preset_test.rs`, `block_def_tests.rs`, `binding_test.rs`, `focus_test.rs`, `part_page_test.rs`, `flt_page_test.rs`, `matrix_view_test.rs`, `screen/mod.rs`, `screen_golden_test.rs`

**Interfaces:**
- Consumes: Task 16's panels and views; `LfoParams` ids (Task 7).
- Produces: `ui::view::LfoKind { Classic, Func(LfoForm) }`, `SlotCtx.lfos: [LfoKind; 3]`; `mod_panel::lfo_panel(LfoKind) -> &'static ModPanel`; `SlotBinding::LfoPanel(LfoSlot, u8)`, `ParamSlot::lfo_panel`; `block_registry::{ENV_SPEED, LFO_2, LFO_3}`; `VizType::EnvSpeed`; `viz::env_speed(d, [(bool, EnvSpeed, HoldPos); 3])`.

- [ ] **Step 1: Write the failing tests**

Append to `chimera-core/tests/mod_pages_test.rs`:

```rust
use chimera_core::dsp::modulator::LfoType;
use chimera_core::preset::{EngineType, Sound};
use chimera_core::ui::block_registry::{ENV_SPEED, LFO, LFO_2, LFO_3};
use chimera_core::ui::view::{View, is_dimmed};

#[test]
fn the_sub_list_is_e1_to_mtx() {
    let node = ALGO_CHAIN.blocks.last().unwrap();
    let shorts: Vec<&str> = core::iter::once(node.def)
        .chain(node.sub_pages.iter().copied())
        .map(|d| d.short)
        .collect();
    assert_eq!(shorts, ["E1", "E2", "E3", "SPD", "L1", "L2", "L3", "MTX"]);
    assert_eq!((ENV_SPEED.id, LFO.id, LFO_2.id, LFO_3.id), (62, 12, 64, 65));
}

#[test]
fn lfo_pages_show_classic_or_func() {
    let mut p = ParamSnapshot::default();
    assert_eq!(labels(&p, &LFO), ["RATE", "SHAPE", "SYNC", "PHASE", "DEPTH", "TYPE"]);
    p.lfos[1].lfo_type = LfoType::Func;
    assert_eq!(labels(&p, &LFO_2), ["MODE", "RATE", "PHASE", "TILT", "FORM", "TYPE"]);
    p.lfos[1].func.lfo_form = LfoForm::Lfv;
    assert_eq!(labels(&p, &LFO_2), ["MODE", "RATE", "DELTA", "SLEW", "FORM", "TYPE"]);
    let ctx = SlotCtx::read(&p, Op::A);
    assert!(matches!(view(&LFO_2, 0, &ctx), View::Text { text: "LFO", .. }), "MODE is fixed");
}

/// SPD: a type-B slot's two cells are dimmed and inert (ENV 3 is B).
#[test]
fn spd_dims_a_type_b_slot() {
    let s = Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, Op::A);
    let dim: Vec<bool> = (0..6).map(|i| is_dimmed(&view(&ENV_SPEED, i, &ctx), &s)).collect();
    assert_eq!(dim, [false, false, false, false, true, true]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `env $T cargo test -p chimera-core --test mod_pages_test`
Expected: FAIL to compile: no `ENV_SPEED`, `LFO_2`.

- [ ] **Step 3: LFO panels**

Append to `mod_panel.rs`:

```rust
use crate::dsp::lfo::LfoParams as L;
use crate::ui::view::LfoKind;

/// CLASSIC: RATE · SHAPE · SYNC / PHASE · DEPTH · TYPE (OFST's cell is TYPE's).
static CLASSIC: ModPanel = ModPanel {
    slots: [
        p(L::RATE, "RATE", ValFmt::Uni),
        p(L::SHAPE, "SHAPE", ValFmt::Names(&["SINE", "TRI", "SAW", "SQR", "S&H"])),
        p(L::SYNC, "SYNC", ValFmt::Names(&["FREE", "RETRIG"])),
        p(L::PHASE, "PHASE", ValFmt::Uni),
        p(L::DEPTH, "DEPTH", ValFmt::Uni),
        LFO_TYPE,
    ],
};
const LFO_TYPE: Option<PanelSlot> = p(L::TYPE, "TYPE", ValFmt::Names(&["CLASSIC", "FUNC"]));
/// In `LfoForm::ALL`'s order.
const LFO_FORM: Option<PanelSlot> = p(L::FORM, "FORM", ValFmt::Names(&["FREE", "SYNC", "LFV"]));
const FUNC_MODE: Option<PanelSlot> = Some(PanelSlot::Fixed { label: "MODE", text: "LFO" });

/// FUNC: B's page with MODE locked to LFO.
static FUNC: ModPanel = ModPanel {
    slots: [
        FUNC_MODE,
        p(L::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(L::FALL, "PHASE", ValFmt::Law(Law::Phase)),
        p(L::SHAPE_B, "TILT", ValFmt::Law(Law::Tilt)),
        LFO_FORM,
        LFO_TYPE,
    ],
};
static FUNC_LFV: ModPanel = ModPanel {
    slots: [
        FUNC_MODE,
        p(L::RISE, "RATE", ValFmt::Law(Law::BRate)),
        p(L::FALL, "DELTA", ValFmt::Law(Law::Pct)),
        p(L::SHAPE_B, "SLEW", ValFmt::Law(Law::Pct)),
        LFO_FORM,
        LFO_TYPE,
    ],
};

pub fn lfo_panel(k: LfoKind) -> &'static ModPanel {
    match k {
        LfoKind::Classic => &CLASSIC,
        LfoKind::Func(LfoForm::Lfv) => &FUNC_LFV,
        LfoKind::Func(_) => &FUNC,
    }
}
```

(CLASSIC's SHAPE and SYNC get names in place of today's `Int(4)` and `Int(1)`, as the pages now read in words.)

- [ ] **Step 4: Views, dimming, bindings**

In `view.rs`:

```rust
/// What an LFO slot's page resolves against: CLASSIC, or FUNC and its
/// FORM (FUNC's MODE is always LFO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoKind {
    Classic,
    Func(LfoForm),
}
```

`SlotCtx` gains `pub lfos: [LfoKind; 3],` read as:

```rust
            lfos: LfoSlot::ALL.map(|s| {
                let at = |id| get(BlockRef::Lfo(s), id);
                match pick(&LfoType::ALL, at(LfoParams::TYPE)) {
                    LfoType::Classic => LfoKind::Classic,
                    LfoType::Func => LfoKind::Func(pick(&LfoForm::ALL, at(LfoParams::FORM))),
                }
            }),
```

(`use crate::dsp::lfo::LfoParams; use crate::dsp::modulator::{LfoSlot, LfoType};`.) `view` gains `SlotBinding::LfoPanel(s, k)`, resolved like `EnvPanel` against `mod_panel::lfo_panel(ctx.lfos[s.index()])` with `BlockRef::Lfo(s)`. `dimmed` gains:

```rust
        // SPD: a type-B slot's SPEED and HOLD (spec § UI).
        (BlockRef::Env(s), EnvParams::SPEED | EnvParams::HOLD_POS) => {
            sound.params.envelopes[s.index()].env_type == EnvType::B
        }
```

and `is_dimmed` also returns true for `View::Text`. In `block_def.rs`: `SlotBinding::LfoPanel(crate::dsp::modulator::LfoSlot, u8)` (treated by `ParamSlot::spec`, `label` and `format` as `FilterPanel(_)` is), `ParamSlot::lfo_panel(s, k)`, and `VizType::EnvSpeed` (doc: `/// SPD: each slot's SPEED and HOLD POSITION.`).

- [ ] **Step 5: The pages**

In `block_registry.rs`:

```rust
const fn lfo_page(id: u16, name: &'static str, short: &'static str, s: LfoSlot) -> BlockDef {
    BlockDef {
        id,
        name,
        short,
        layout: PageLayout::CellGrid,
        viz: VizType::None,
        params: [
            ParamSlot::lfo_panel(s, 0),
            ParamSlot::lfo_panel(s, 1),
            ParamSlot::lfo_panel(s, 2),
            ParamSlot::lfo_panel(s, 3),
            ParamSlot::lfo_panel(s, 4),
            ParamSlot::lfo_panel(s, 5),
        ],
    }
}

/// L1 (id 12, once the one LFO's page).
pub static LFO: BlockDef = lfo_page(12, "LFO 1", "L1", LfoSlot::Lfo1);
pub static LFO_2: BlockDef = lfo_page(64, "LFO 2", "L2", LfoSlot::Lfo2);
pub static LFO_3: BlockDef = lfo_page(65, "LFO 3", "L3", LfoSlot::Lfo3);

/// SPD: type A's SPEED and HOLD POSITION for the three ENV slots (spec § UI).
pub static ENV_SPEED: BlockDef = BlockDef {
    id: 62,
    name: "Env Speed",
    short: "SPD",
    layout: PageLayout::CellGrid,
    viz: VizType::EnvSpeed,
    params: [
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::SPEED).with_label("E1 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env1), EnvParams::HOLD_POS).with_label("E1 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::SPEED).with_label("E2 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env2), EnvParams::HOLD_POS).with_label("E2 HOLD"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::SPEED).with_label("E3 SPEED"),
        ParamSlot::param(BlockRef::Env(EnvSlot::Env3), EnvParams::HOLD_POS).with_label("E3 HOLD"),
    ],
};

static MOD_SUB_PAGES: [&BlockDef; 7] =
    [&ENV_2, &ENV_3, &ENV_SPEED, &LFO, &LFO_2, &LFO_3, &MOD_MATRIX];
```

In `focus.rs`, `pub const MAX_PAGES: usize = 72;` (doc: `ids are 0..=65 today (test-checked)`).

- [ ] **Step 6: The SPD viz**

In `viz.rs`:

```rust
/// SPD's picture (the approved mockup): per ENV slot, its SPEED as three
/// pills and its HOLD POSITION's name; a type-B slot's column is faint.
pub fn env_speed<D>(d: &mut D, slots: [(bool, EnvSpeed, HoldPos); 3])
where
    D: DrawTarget<Color = Rgb565>,
{
    for (i, &(is_a, speed, hold)) in slots.iter().enumerate() {
        let x = theme::VIZ_LEFT + 4 + i as i32 * 74;
        let ink = if is_a { theme::MID } else { theme::FAINT };
        draw::text_center(d, &theme::FONT_LABEL, ["E1", "E2", "E3"][i], x + 30, 50, ink, 1);
        for (k, name) in ["FAST", "MED", "SLOW"].iter().enumerate() {
            let y = 56 + k as i32 * 18;
            let lit = is_a && k == speed as usize;
            if lit {
                draw::pill(d, x + 6, y, 48, 14, theme::ACCENT);
            } else {
                draw::round_outline(d, x + 6, y, 48, 14, 7, theme::FAINT);
            }
            let c = if lit { theme::BG } else { ink };
            draw::text_center(d, &theme::FONT_LABEL_BOLD, name, x + 30, y + 10, c, 0);
        }
        let under = if is_a {
            ["OFF", "AHDSR", "GATE EXT"][hold as usize]
        } else {
            "RISE/FALL"
        };
        draw::text_center(d, &theme::FONT_LABEL, under, x + 30, 140, ink, 1);
    }
}
```

and in `renderer.rs`'s `draw_band_viz`:

```rust
            VizType::EnvSpeed => {
                let e = &f.parts[f.active_part].sound.params.envelopes;
                viz::env_speed(
                    display,
                    core::array::from_fn(|i| (e[i].env_type == EnvType::A, e[i].speed, e[i].hold_pos)),
                );
            }
```

`viz_inputs` fingerprints it: its `PageLayout::CellGrid` match gains `VizType::EnvSpeed => (region::quantize_values(&self.anim), 0)`.

- [ ] **Step 7: Update the tests for the full sub-list**

The sub-list now reads E1 · E2 · E3 · SPD · L1 · L2 · L3 · MTX, so every path Task 16 set turns red again. Each is listed with its new path:

- The LFO page is now sub-page 4: `prime_status_test.rs` (`mix_plus_on_a_non_modulatable_param_reports_not_modulatable`), `ui_routing_test.rs` (`priming_a_non_modulatable_param_is_refused`) and `preset_test.rs` (`priming_on_the_lfo_sub_page_registers_nothing`) press EDIT four times.
- MTX is now sub-page 7. These press EDIT seven times where Task 16 pressed it four:
  - `ui_routing_test.rs`: `set_first_amount` and `enter_matrix`;
  - `flt_page_test.rs`: `to_matrix`;
  - `matrix_view_test.rs`: `an_empty_matrix_says_so`;
  - `focus_test.rs`: `mixer_part_and_matrix_pages_use_the_same_mechanism`;
  - `screen/mod.rs`: `amp_vel_live` and `mod_matrix`.
- `ui_test.rs`, `test_page_from_nav_part_chain`: sub-pages 1–7 are ENV_2, ENV_3, ENV_SPEED, LFO, LFO_2, LFO_3, MOD_MATRIX.
- `block_def_tests.rs`: `chain.blocks[5].sub_pages.len()` is 7.
- `binding_test.rs`: `block_def_ids_are_unique` gains ENV_SPEED, LFO_2, LFO_3; the LFO row compares views: `("RATE", Uni), ("SHAPE", Names(…)), ("SYNC", Names(&["FREE", "RETRIG"])), ("PHASE", Uni), ("DEPTH", Uni), ("TYPE", Names(&["CLASSIC", "FUNC"]))`.
- `focus_test.rs`'s id check against `MAX_PAGES` needs no change.
- `part_page_test.rs`, `envelope_and_lfo_pages`: LFO slot 2 turned by 1 reads `p.lfos[0].sync == 1` as before; `snap` on slot 1 still reaches SINE.
- `screen/mod.rs`, new cases with rows in `GOLDENS`:

```rust
    ("spd", |ui| {
        plus(ui, 5);
        for _ in 0..3 {
            feed(ui, Input::press(ButtonId::Edit)); // SPD
        }
        feed(ui, Input::turn(EncoderId::C, -1)); // E2 SPEED → FAST
    }),
    ("lfo_classic", |ui| {
        plus(ui, 5);
        for _ in 0..4 {
            feed(ui, Input::press(ButtonId::Edit)); // L1
        }
        feed(ui, Input::turn(EncoderId::A, 5)); // RATE
    }),
    ("lfo_func", |ui| {
        plus(ui, 5);
        for _ in 0..4 {
            feed(ui, Input::press(ButtonId::Edit));
        }
        feed(ui, Input::turn(EncoderId::F, 1)); // TYPE → FUNC
    }),
```

- [ ] **Step 8: Run the tests**

Run: `env $T cargo test -p chimera-core`, then `env $T cargo test -p chimera-core --test all_pages_walk_test -- --ignored` (the exhaustive walk visits the three new pages).
Expected: PASS except `screen_goldens_match`.

- [ ] **Step 9: Look at the screens and re-record**

- `spd`: header `ENV SPEED`, three columns of FAST/MED/SLOW pills (E1 MED lit, E2 FAST lit, E3 faint with "RISE/FALL"), cells E1 SPEED · E1 HOLD · E2 SPEED / E2 HOLD · E3 SPEED · E3 HOLD with E3's two dimmed.
- `lfo_classic`: RATE · SHAPE `SINE` · SYNC `FREE` / PHASE · DEPTH · TYPE `CLASSIC`.
- `lfo_func`: MODE `LFO` dimmed · RATE `1.00 Hz` (0.9955 Hz, rounded) · PHASE `0` / TILT `TRI` · FORM `FREE` · TYPE `FUNC`; the animators re-seeded (no sweep).
- `mod_matrix`: the sub-list scrolled so MTX is lit on screen.
- `amp_vel_live`: VEL still live, with its bar.

Re-record the mismatches and the new cases.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all && just check
git add chimera-core/src/ui/mod_panel.rs chimera-core/src/ui/view.rs chimera-core/src/ui/block_def.rs \
  chimera-core/src/ui/block_registry.rs chimera-core/src/ui/focus.rs chimera-core/src/ui/viz.rs \
  chimera-core/src/ui/renderer.rs \
  chimera-core/tests/mod_pages_test.rs chimera-core/tests/ui_test.rs chimera-core/tests/ui_routing_test.rs \
  chimera-core/tests/prime_status_test.rs chimera-core/tests/preset_test.rs chimera-core/tests/block_def_tests.rs \
  chimera-core/tests/binding_test.rs chimera-core/tests/part_page_test.rs chimera-core/tests/flt_page_test.rs \
  chimera-core/tests/matrix_view_test.rs chimera-core/tests/focus_test.rs \
  chimera-core/tests/screen/mod.rs chimera-core/tests/screen_golden_test.rs
git commit -m "SPD and LFO 1–3 pages; the MOD sub-list E1 · E2 · E3 · SPD · L1 · L2 · L3 · MTX"
```

---

### Task 18: ADRs 0035, 0036 and 0037

The three ADRs the spec lists (§ ADRs), numbered from 0035: 0033 is the theme page's and 0034 the watchdog's. They are marked Proposed until the owner accepts them at Task 19's STOP.

**Files:**
- Create: `docs/adr/0035-every-connection-is-a-matrix-route.md`, `docs/adr/0036-cascadia-style-modulators.md`, `docs/adr/0037-kind-lays-out-the-filter-panel.md`
- Modify: `docs/adr/README.md` (three rows; the status of 0010 and 0022)
- Modify: `docs/superpowers/specs/2026-09-27-va-engine-design.md` (name ADR 0035 where it points at "the routing spec's ADR")

**Interfaces:** none (documents).

- [ ] **Step 1: ADR 0035**

Create `docs/adr/0035-every-connection-is-a-matrix-route.md`:

```markdown
# 0035. Every connection from a modulator to the sound is a matrix route

- **Status:** Proposed
- **Deciders:** project owner (2026-09-27, the filter-routing spec; 2026-09-28, priming the hidden destinations)

## Context
The FLT page showed ENV, KEY and FM knobs the DSP never read (#121, #112), the amp envelope ran as a mod source that nothing put on the VCA (ADR 0022), and routing lived in two places: a few fixed knobs and the matrix. The filter models (#123–#127) each want a different panel over the same connections.

## Decision
- Every voice runs six modulators, always: ENV 1–3 (Envelope A or B) and LFO 1–3 (CLASSIC or FUNC). A slot drives only what the matrix routes from it.
- The matrix has eight sources in this stored order: ENV1, LFO1, ENV2, ENV3, LFO2, LFO3, VEL, NOTE; indices 0 and 1 keep their old meaning. NOTE is (note − 60) / 120.
- A route exists apart from its amount (a presence bit per cell): a route at 0 is not a deleted route. MIX+MINUS on a matrix cell deletes a route; the audio thread's sums ignore the bits.
- CUTOFF's routes sum in octaves: `fc = clamp(base · 2^(10·Σ), 20 Hz, min(20 kHz, 0.49·fs))`; Σ = 0 leaves `base` bit for bit. This supersedes ADR 0010's linear law for CUTOFF only. The filter ramps `g` across a block.
- The VCA is a hidden destination on the Out block: the sum of its routes, clamped 0..1, applied per sample and times AMP's VEL term. It is a sum, not a product. Its gain ramps per sample, a LEVEL change on a routed ENV slot included, so no block edge steps it. The no-route rule is an exhaustive match on the engine: Algo and Modal pass through (today's expression, bit for bit). VA's arm (a gate) and its default route are #148, so adding VA does not compile until they are decided. With routes, the voice ends at the end of the first block in which no routed source holds it, through ADR 0027's 128-sample fade if its gain isn't 0; an inactive engine always ends it. This supersedes, in part, ADR 0022's note that no engine puts the amp envelope on the VCA.
- Every new Sound carries ENV 1, LFO 1 and NOTE → CUTOFF at 0 (NOTE at the kind's key default); VA's ENV 2 → VCA at 100 % comes with VA (#148).
- The filter page's ENV, LFO and KEY knobs are views of those routes: an absent route shows a dash; turning it creates it (and the CUTOFF column, or reports MATRIX FULL). KIND never edits the matrix.
- The hidden destinations, which the spec gives no page, are primed from the cells that own them (owner's decision): AMP's VEL primes VCA, even while VEL is dimmed (the one exception to ADR 0037's dimming rule). An A page's A, D, R and H prime that slot's TIME, and its S primes LEVEL.
- `Voice::cost` adds `ModRouting::cost`: a base, then for each ENV slot on the VCA a term for A, one for B (billed at B's costliest form: ENV, LFO or BURST), and more for a B ENV whose SHAPE is off centre or routed, plus a term per other VCA route and one for the clamp. The model must never undercount. Until the bench measures each term on its own row, it bills the plan review's estimates, not the spec's (BASE 30, ENV_A 30, ENV_B 40, CURVE 15, OTHER 3, CLAMP 3). After that it bills the measured values, rounded up, and a test checks that it bills the bench's MODS row at least as measured.

## Alternatives considered
- **Fixed ENV/LFO SOURCE selectors on the filter** (the spec's first version): two sources of truth for routing.
- **A VCA product with depth** (`1 − a + a·s`): an idle envelope leaves a floor of `1 − a`, so the voice would end on a step, and VCA would be the one destination that multiplies.
- **An amp envelope wired to the VCA:** the SH-101 one-envelope feel would need a special case; here it is two matrix edits.

## Consequences
- The matrix is at its source cap (8); a ninth source raises `MAX_MOD_SOURCES` to 16 and `present` to `u16`.
- Tremolo on top of an envelope clamps at 1 and can leak after release until the voice ends; a multiplicative "VCA MOD" destination is a later ADR if wanted.
- A LFO- or VEL-gated note is cut about 2.7 ms after key-up (the fade): the no-drone guarantee's price.
- The i8 amount on CUTOFF is 10/127 of an octave per step.
- A route into VCA raises a Sound's cost and can shed held notes (ADR 0026, #31).

## Sources
`docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 2–5; plan `docs/superpowers/plans/2026-09-28-filter-routing.md`; #120, #121, #140.
```

- [ ] **Step 2: ADR 0036**

Create `docs/adr/0036-cascadia-style-modulators.md`:

```markdown
# 0036. ENV slots follow the Cascadia's Envelopes A and B; LFO slots are CLASSIC or FUNC

- **Status:** Proposed
- **Deciders:** project owner (2026-09-27); the "Defaults chosen" of the spec await the owner's word

## Context
Chimera had one linear amp envelope with a per-sample divide, two envelopes nothing read, and one LFO. The owner chose the Intellijel Cascadia's two envelopes as the model for all three ENV slots.

## Decision
- Provenance: the behaviour and ranges of the Cascadia manual v1.2 (2023-10-15), pp. 28–39 and 82–97; none of its code.
- Every time or rate slider is a position 0..1 on `q = q_min · (q_max/q_min)^p` (one `fast_exp2` per block); a matrix route adds to the position, so it acts in octaves.
- **Envelope A:** AHDSR with HOLD POSITION (OFF, AHDSR, GATE EXT) and SPEED (FAST, MED, SLOW) at the manual's ranges. One fixed RC shape: attack aims at 1.3 and stops at 1; decay and release aim 0.01 past their end, so a full swing takes the slider's time. Per sample it is one multiply-add, with no divide; per block it is closed form. ENV n LEVEL (the peak, `clamp(Σ, 0, 1)` when routed) and ENV n TIME (× `2^(−5·Σ)`) are its destinations, the Cascadia's CTRL SOURCE done in the matrix.
- **Envelope B:** MODE ENV, LFO or BURST; FORM (the Cascadia's TYPE SELECT) AD, AHR or CYCLE, or FREE, SYNC or LFV, default first. RISE, FALL and SHAPE follow MODE and FORM. SHAPE's curve is `x / (x + (1 − x)·2^(4·(2·SHAPE − 1)))`, linear at the centre. LFV is a clamped random walk with a slew. BURST is pulses under a TILT-shaped burst. SYNC resets at each note-on until #44 gives a clock. A B slot run per block clamps its rates to block rate ÷ 8 (`block_rate_max(sr)`: 93.75 Hz at 48 kHz, 86.1 Hz at the desktop's 44.1 kHz). The clamp covers RATE, BURST's pulse RATE and the repeat rates of ENV CYCLE (1/(RISE + FALL)) and BURST CYCLE (1/LENGTH); one-shot AD and AHR times are not clamped. Only a slot on the VCA runs per sample at the full ranges. ENV n RISE, FALL and SHAPE are its destinations.
- **LFO slots:** CLASSIC is today's LFO, bit for bit, with OFFSET stored but no longer applied. FUNC is Envelope B locked to LFO mode, with DEPTH not applied.
- A TYPE, MODE or FORM change never jumps the level: into A or B ENV the new shape enters at the current level; otherwise the difference glides to 0 over 256 samples (one `Glide` type, shared by ENV and LFO slots).
- Outputs carry no velocity; VEL reaches the sound as a source, through AMP's VEL and through ENV n LEVEL.
- The FORM chosen is kept per MODE, in types: `EnvForm` (ENV and BURST) and `LfoForm`, stored as `env_form`, `lfo_form` and `burst_form`. What runs is `Func { Env(EnvForm), Lfo(LfoForm), Burst(EnvForm) }`, so a MODE/FORM mismatch can't be represented. A value a TYPE, MODE or FORM doesn't use is kept.
- **Accuracy:** both paths (the per-sample tick and the per-block closed form) are tested against an f64 reference, to about 1e-4 absolute, with stage changes within ±1 sample. This supersedes the spec's 1e-6, which f32 cannot meet over a long stage. B's per-sample path is anchored, not accumulated: a position is `x₀ ± k·step`, re-anchored at each turn, wrap and block start, and φ is a `u32` turn. A constant f32 step added every sample rounds the same way each time, and the error grows past 1e-4 within a few cycles.

## Alternatives considered
- **A curve control on Envelope A:** the Cascadia has none; A stays divide-free.
- **All slots per sample:** six per-sample modulators per voice would cost voices; only VCA-routed ENV slots tick per sample.
- **Resetting FREE, LFV and BURST phases on note-on** (the Cascadia's default gate behaviour): it would make FREE and SYNC the same until #44.

## Consequences
- In f32, the two paths can differ by up to 1e-4 and a sample at a stage change; the f64 reference tests hold them there.
- Per-block B rates stop far below the Cascadia's 800 Hz LFO and 1 kHz bursts; audio-rate modulation of other destinations needs per-sample routing, not built.
- `played` grows about 210 B per voice; the RAM asserts hold.

## Sources
Intellijel Cascadia manual v1.2 (2023-10-15); `docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 1; #140.
```

- [ ] **Step 3: ADR 0037**

Create `docs/adr/0037-kind-lays-out-the-filter-panel.md`:

```markdown
# 0037. KIND lays out the filter panel; MODE follows KIND

- **Status:** Proposed
- **Deciders:** project owner (2026-09-27)

## Context
The filter models (#123–#127) each bring a classic synth's panel and their own modes. MODE was a raw `u8` on no page (#111).

## Decision
- KIND (Filter ParamId 6) is FLT's first knob; it sets knobs 2–6 to that synth's panel, from `const` data (`ui/filter_panel.rs`). A label is the original panel's word; the target is the same everywhere. A kind's row lands with its model; today the SVF is the only kind: CUTOFF · RES · MODE · ENV · KEY, with DRIVE and LFO on FLT › MODE.
- Route knobs are views of matrix cells (ADR 0035); shortcuts to elsewhere (TB-303's DECAY) arrive with their kind.
- MODE (ParamId 7) is a typed `FilterMode` whose discriminants 0–7 are the old byte. `mode ∈ kind.modes()` holds on every write of KIND or MODE. A KIND change keeps MODE if the new kind has it, else takes its default. A single-mode kind shows MODE fixed and dimmed.
- The matrix always applies; "not shown, not applied" covers only the filter's own parameters.
- Filter ParamIds 3 (FM), 4 (ENV) and 5 (KEY) are retired and never reused (ADR 0009).
- A fixed or inapplicable slot draws dimmed (label and value in MID, no bar); its encoder is ignored. One exception, the owner's decision: a dimmed cell whose MIX+PLUS primes a different, hidden destination still primes it. AMP's VEL, dimmed under the pass-through, primes the VCA (ADR 0035), since there is no other way in.
- `FilterKind::cost(kind, mode)` bills each kind like an engine (ADR 0026); the SVF's is measured by the bench's SVF row (PHASER, less 1 OP).

## Alternatives considered
- **KIND rewriting routes on a change** (the SH-101 kind setting ENV 1 → VCA): it would destroy the user's routing.
- **One MODE list for every kind:** would show modes a model doesn't have.

## Consequences
Each model issue adds its `FilterKind` variant, its panel row, its modes and its cost. The model choice, topologies and oversampling policy are #128's own ADR.

## Sources
`docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 6–7; #122, #111, #128.
```

- [ ] **Step 4: The index and the VA spec's pointer**

In `docs/adr/README.md`, add the rows (in order after 0034, or after 0032 on a branch without 0033/0034):

```markdown
| [0035](0035-every-connection-is-a-matrix-route.md) | Every connection from a modulator to the sound is a matrix route | Proposed |
| [0036](0036-cascadia-style-modulators.md) | ENV slots follow the Cascadia's Envelopes A and B; LFO slots are CLASSIC or FUNC | Proposed |
| [0037](0037-kind-lays-out-the-filter-panel.md) | KIND lays out the filter panel; MODE follows KIND | Proposed |
```

and change the status cells: 0010 becomes `Accepted; linear law superseded for CUTOFF by [0035](0035-every-connection-is-a-matrix-route.md)`, and 0022 becomes `Accepted; VCA note superseded in part by [0035](0035-every-connection-is-a-matrix-route.md)`. (The ADR files themselves stay as they are.)

In `docs/superpowers/specs/2026-09-27-va-engine-design.md`, the two places that say "the routing spec's ADR" (line 7 "which supersedes 0022's VCA note" and the ADR bullet "The VCA note is superseded by the routing spec's ADR") name it: "ADR 0035".

- [ ] **Step 5: Commit**

```bash
git add docs/adr/0035-every-connection-is-a-matrix-route.md docs/adr/0036-cascadia-style-modulators.md \
  docs/adr/0037-kind-lays-out-the-filter-panel.md docs/adr/README.md \
  docs/superpowers/specs/2026-09-27-va-engine-design.md
git status --short   # docs/chimera-ui-ux-spec.md must not be staged
git commit -m "ADRs 0035–0037: routing, the Cascadia-style modulators, KIND's panel"
```

---

### Task 19: Play the pages; the load check (hardware STOP)

The costs were measured at Task 13. Here the owner plays the new pages on the synth and runs a load check. No code changes unless the owner finds something.

**Files:**
- Modify (after the STOP, if the owner accepts): `docs/adr/0035-every-connection-is-a-matrix-route.md`, `docs/adr/0036-cascadia-style-modulators.md`, `docs/adr/0037-kind-lays-out-the-filter-panel.md`, `docs/adr/README.md`

- [ ] **Step 1: STOP. Ask the owner to play the pages and check the load, and wait**

Send the owner this, then wait for the reply:

> The filter-routing branch is ready to play (Tasks 1–18).
> 1. Put the synth in DFU mode, run `just flash`, and play:
>    - FLT: KIND (fixed, SVF), CUTOFF, RES, MODE, ENV, KEY; FLT › MODE: MODE, DRIVE, LFO. Delete ENV's route in MTX (MIX+MINUS), check the knob shows a dash, and turn it back.
>    - AMP: VEL is dimmed and still primes VCA; route E2 → VCA in MTX (MIX+PLUS on VEL first) and hear the note follow ENV 2. Then the SH-101 feel: E1 → VCA and CUTOFF, E2's VCA route deleted.
>    - The MOD list E1 E2 E3 SPD L1 L2 L3 MTX. Flip E1's TYPE to B and back mid-note (no click). Try ENV 3's modes and forms, then FUNC on an LFO.
>    - Hold a chord and release: no note drones on with an LFO or E3 (B, CYCLE) routed to the VCA.
> 2. Load check: the costliest factory Sound with the MODS routing, six voices held, then open MENU › ABT › AUDIO and read LOAD, PEAK, OVER and DROPS.
> 3. Reply with the AUDIO readings and anything that looked or sounded wrong. Also say whether ADRs 0035–0037 can be marked Accepted, including the dimming exception (AMP VEL primes VCA while dimmed; A, D, R and H prime TIME; S primes LEVEL).

- [ ] **Step 2: Record the load check**

Add the AUDIO readings, with the date, to `## Measured`. If OVER or DROPS is above 0, stop and tell the owner: the cost model undercounts somewhere, and Task 13's terms need a second look before merging.

- [ ] **Step 3: Accept the ADRs**

If the owner accepted the ADRs, change their Status to `Accepted (<date>)` in the three files and in `README.md`.

```bash
git add docs/superpowers/plans/2026-09-28-filter-routing.md docs/adr/README.md \
  docs/adr/0035-every-connection-is-a-matrix-route.md docs/adr/0036-cascadia-style-modulators.md \
  docs/adr/0037-kind-lays-out-the-filter-panel.md
git commit -m "Filter routing played on the chip; ADRs 0035-0037 accepted"
```

- [ ] **Step 4: File what the owner found**

Each problem the owner reports that this plan does not fix becomes a GitHub issue (joegiralt/chimera), linked from epic #120. Nothing goes in repo files.

---

## Spec gaps found while planning

Each one is settled in this plan as noted:

1. **No way to reach the hidden destinations.** VCA, ENV n LEVEL and ENV n TIME are on no page, and a column is added only when a parameter is primed or a route knob creates one. So on Algo and Modal nobody could create E1 → VCA (the SH-101 feel) or VEL → LEVEL. **Owner's decision:** each is primed from the cell that owns it. AMP VEL primes VCA even while dimmed, A, D, R and H prime TIME, and S primes LEVEL. ADRs 0035 and 0037 record this as the one exception to the dimming rule.
2. **The VA engine is not on `main`.** VA's no-route gate, its 64-sample edges and its default ENV 2 → VCA route can't be built or tested here. They are #148 (linked to #118 and #120). Task 10's rule is an exhaustive match on the engine, so adding VA fails to compile until #148 decides its arm.
3. **"Within 1e-6" is beyond f32.** A 60 s stage accumulates f32 error well past 1e-6. Both paths (tick and `fill`) are tested against an f64 reference to 1e-4 absolute, with stage changes within ±1 sample. ADR 0036 records that this supersedes the spec's 1e-6.
4. **`MAX_PAGES` is 64,** so ids 64 and 65 (L2, L3) do not fit; it rises to 72.
5. **#48 and #57 would move the factory goldens.** A real `fast_tan` fix changes every Sound, so #57 gets its doc only. #48 is deferred to the model work (commented on #48).
6. **The approved mockups use glyphs and words the fonts and spec don't.** The u8g2 `_tr` faces lack "°", "·" and "→". The mockups' LFV cell reads VARY where the spec says DELTA; the plan follows the spec. Their "→ CUTOFF" caption over the ENV viz is not in the spec and is not built.
7. **The spec's one `form` field.** The FORM is stored per MODE (`env_form`, `lfo_form`, `burst_form`, each its own enum), so switching MODE back restores it. The ENV and BURST knob steps AD · AHR · CYCLE (AD first, the default).
8. **The per-block B rate clamp.** It covers RATE (LFO), pulse RATE (BURST) and the repeat rates of ENV CYCLE and BURST CYCLE, at `block_rate_max(sr) = sr / BLOCK_SIZE / 8`. ENV-mode AD and AHR rise and fall times are not clamped.
9. **Where the slice ships.** The shippable slice (#121) ends after Task 7, not after the UI tasks.
