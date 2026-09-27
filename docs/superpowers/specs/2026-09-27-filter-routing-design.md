# Filter Page and Modulation Routing — Design

The FLT page does what its knobs say. Envelopes are known by slot (ENV 1 can drive the VCA, ENV 2 is the filter's, ENV 3 is free), and each slot has a TYPE. The filter has three fixed routes (envelope, LFO and key to cutoff), each with a SOURCE where it has a choice. The chain's FLD block becomes FLD / VCA: every Part's chain ends in one VCA, whose source is ENV 1, GATE or ENGINE. KIND, the page's first knob, picks a filter model and lays out the other five knobs as that synth's panel. The mod matrix stays for everything else, with seven sources instead of two. This is the routing and panel half of #128; the per-model DSP is the other half. Epic #120.

## Roadmap position

This comes before the filter models (#123–#127). It builds the routing, the envelope slots, the FLD / VCA block and the KIND machinery with the one model that exists, the SVF. Each model issue then adds its `FilterKind` variant, its panel row, its modes and its cost. It closes #121 and the routing part of #122, and closes #112's filter and envelope items (FM is retired; ENV, KEY and ENV 2/3 get readers).

## Intent

- Every knob on every FLT page changes the sound, or is visibly fixed.
- One envelope can feed two places (the SH-101 shared envelope) without copying anything.
- The factory Sounds sound exactly as they do today.
- The routes cost a voice at most about 20 cycles per sample on top of today's chain.

**Done when:**
- ENV, KEY, MODE and the LFO route audibly change the SVF, with a test per knob;
- an ENV sweep has no zipper noise (§ Tests, Clicks), and KEY at 1 tracks exactly one octave per octave;
- ENV 1–3 each run in every voice, each with a TYPE, and each is a matrix source;
- the matrix has the seven sources ENV1, LFO1, ENV2, ENV3, LFO2, VEL, NOTE;
- the FLD / VCA block ends every Part's chain (Algo and Modal); VCA ENV 1 and GATE shape any engine's output, and VCA ENGINE, the Algo and Modal default, renders every factory Sound bit-identically to today;
- the `FilterKind` panel machinery, the mode lists and the kind-change rules are in, with the SVF as the one built kind;
- the bench's new MODS and SVF rows are measured and their costs committed.

## What exists today

| Claim | Evidence |
|---|---|
| One 2-pole SVF, saturation in the feedback loop, 8 modes | `dsp/filter.rs`: `FilterMode` `Lp1`…`Phazor`; `SvfFilter::process` |
| The filter reads cutoff, resonance, drive and mode only | `filter.rs:55-59`; nothing reads `fm_amount`, `env_amount`, `key_track` (`params.rs:40`, #112) |
| MODE is on no page; every Sound is on LP4 | `FilterParams.mode: u8`, default 2, no `ParamSpec` (`params.rs:13,25`); `FILTER` binds CUTOFF, RES, DRIVE, FM, ENV, KEY (`block_registry.rs:94`) |
| The cutoff is computed once per block | `filter.rs:61-62`: one `fast_tan` per `process` call |
| Three `EnvParams`, only `envelopes[0]` runs | `ParamSnapshot.envelopes: [EnvParams; 3]` (`params.rs:375`); `Voice` holds one `amp_env` (`voice.rs`) |
| The amp envelope doesn't shape the output | `voice.rs:298-303`: the VCA loop ticks `amp_env` but multiplies by `volume` only; ADR 0022: "no engine puts it on the VCA" |
| A voice ends when its engine does | `voice.rs:307` |
| The fold already runs before the output level | `voice.rs`: engine → drive → filter → folder → `volume` |
| FLD is FOLD · SYM · MIX and three empty slots, on the Algo chain only | `FOLDER` (id 9, `block_registry.rs:74`); `ALGO_BLOCKS` has it, `MODAL_PLUCK_BLOCKS` doesn't, though `Voice` runs the folder for every engine |
| ENV_FILTER and ENV_AUX exist but nothing reads them | `block_registry.rs:149-195`: `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` are `ParamSlot::legacy` (bound to nothing); their chain `ENVELOPE_CHAIN` is not returned by `chain_def_for` |
| The page that edits ENV 1 is `ENVELOPE` (id 11), under MOD | `block_registry.rs:116`, `MOD_MATRIX_SUB_PAGES = [&ENVELOPE, &LFO]` |
| The matrix has two sources, by position | `PART_MOD_SOURCES = ["ENV", "LFO"]` (`block_registry.rs:434`); `voice.rs:256-263`: source 0 = `amp_env.current_level()` at block start, source 1 = the LFO |
| One LFO, run once per block | `ParamSnapshot.lfo`; `Lfo::process` advances 64 samples |
| Matrix offsets are linear in the param's range | `apply_offset`: `v + off·(max − min)` (ADR 0010) |
| No filter–envelope connection of any kind | all of the above |
| The FLT and ADSR vizzes read slots by position | `ui/renderer.rs:102` `viz::filter(a(0), a(1))`; `:105` ATK/DEC/SUS/REL from `a(0..3)` |
| Sounds are not persisted | `SoundPool` is RAM only; the factory bank is code (`factory.rs`); no serializer exists |
| No factory Sound or golden routes ENV | `factory.rs`: MORPH PAD routes source 1 (LFO) to MORPH; `tests/common/mod.rs`: every modulated golden uses source 1 |

## Decisions

These record the owner's decisions of 2026-09-27, made precise.

### 1. Envelopes are slots

- Three slots per voice: `EnvSlot::{Env1, Env2, Env3}`. What a slot is connected to is fixed by routing, never by its shape:
  - ENV 1 is the VCA's envelope when VCA is ENV 1;
  - ENV 2 is the filter route's default source;
  - ENV 3 is free: the matrix, or a route's SOURCE.
- Each slot has a TYPE. TYPE picks which stages the slot uses and shows. It never changes what the slot feeds.

| TYPE | Stages used | Gate |
|---|---|---|
| ADSR | A, D, S, R | Attack to 1, decay to S, hold S while the key is down, release to 0 on key-up |
| AHDSR | A, H, D, S, R | As ADSR, with H seconds at 1 between attack and decay |
| AD | A, D | One shot: attack to 1, decay to 0. Key-up does nothing |
| AR | A, R | Attack to 1, hold 1 while the key is down, release to 0 on key-up |
| LOOP | A, R | While the key is down: attack to 1, fall to 0 at R, repeat. Key-up: release from the current level at R |

- Segments stay linear, as today. A time is the time for a full 0↔1 swing (today's rule: rate = 1 / (time · sr)); a decay to S takes (1 − S) of D.
- The output is contour × velocity, as `Envelope::current_level` is today. LEVEL and VEL (`EnvParams.level`, `vel_sens`) stay unread and leave the page; they stay on #112.
- A stage a TYPE doesn't use keeps its value, unread and unshown, so switching TYPE back restores it.
- **TYPE change mid-note:** the level never jumps. The running stage maps to the new TYPE's next stage from the same point: Hold in a TYPE without it → Decay (or, for AR and LOOP, the held top); Decay or Sustain in AR → the hold at the current level; Sustain in AD → Decay to 0; any stage in LOOP → its current half of the cycle.

### 2. Fixed routes, and the matrix for extras

- The filter has three fixed routes into cutoff:

| Route | Source | Amount | Range of the amount |
|---|---|---|---|
| ENV | `env_src`: ENV 1, 2 or 3 | FLT ENV | −1..1 = ±10 octaves (`ENV_OCTAVES`) |
| LFO | `lfo_src`: LFO 1 or 2 | FLT LFO | −1..1 = ±5 octaves (`LFO_OCTAVES`) |
| KEY | the note | FLT KEY | 0..1: octaves per octave from C4 (note 60) |

- The routed cutoff, per block:
  `fc = clamp(base · 2^(ENV_OCTAVES·env_amt·env + LFO_OCTAVES·lfo_amt·lfo + key·(note − 60)/12), 20 Hz, min(20 kHz, 0.49·fs))`,
  where `base` is CUTOFF after the matrix's offset (linear, ADR 0010). A route the kind doesn't apply contributes 0. When every term is 0, `fc` is `base` bit for bit (no `exp2` runs).
- A page knob is always the amount of a fixed route. It is never a second copy of a matrix route. A matrix route to CUTOFF still works, and adds linearly before the fixed routes scale in octaves.
- The three route amounts are read per block, so they are modulatable (ADR 0010): VEL → FLT ENV gives velocity-scaled filter depth. A matrix route to an amount the kind doesn't apply is inert; it can only exist from before a KIND change, because priming needs the knob on a page. ADR 0010's audibility test runs per built kind over the amounts that kind applies.
- The matrix's sources become seven:

| Index | Source | Value |
|---|---|---|
| 0 | ENV1 | ENV 1 at block start (as today's source 0) |
| 1 | LFO1 | LFO 1 (as today's source 1) |
| 2 | ENV2 | ENV 2 at block start |
| 3 | ENV3 | ENV 3 at block start |
| 4 | LFO2 | LFO 2 |
| 5 | VEL | the note's velocity, 0..1 |
| 6 | NOTE | (note − 60) / 64, clamped to −1..1 |

  Indices 0 and 1 keep their meaning, so MORPH PAD's route and every golden's route are unchanged. 7 ≤ `MAX_MOD_SOURCES` (8).

### 3. Each route has a SOURCE

- `env_src` picks ENV 1, 2 or 3; default ENV 2.
- `lfo_src` picks LFO 1 or 2; default LFO 1.
- `env_src` = ENV 1 with VCA = ENV 1 is the SH-101 shared envelope: one `Envelope`, read by the VCA per sample and by the filter route per block. Nothing is copied.
- ENV SRC and LFO SRC live on FLT › MODE. The VCA's source lives on the FLD / VCA block (§ 3a).

### 3a. FLD / VCA ends every chain

Owner's decision, added 2026-09-27. It replaces the ENV 1 / GATE switch that was going on FLT › MODE.

- The FLD block becomes FLD / VCA: the fold, then one VCA. On the map its short label is **AMP**; its name is "Fold / VCA". Its page is FOLD · SYM · MIX · **VCA** · **VEL** · (empty).
- Every Part's chain has it, last before MOD: the Algo chain (where FLD is today) and the Modal chain (which gains it).
- The fold comes before the VCA, as `Voice` already runs it, so the fold's colour doesn't change with level.
- **VCA** is the VCA's source, `VcaSource`:

| VCA | Gain per sample | Default for |
|---|---|---|
| ENGINE | 1: a passthrough. The engine's own envelopes shape the level | Algo, Modal |
| ENV 1 | ENV 1's contour × the velocity term | VA (docs/superpowers/specs/2026-09-27-va-engine-design.md) |
| GATE | The SH-101's switch: 1 while the key is held, 0 after key-up, with a 64-sample linear ramp at each edge; × the velocity term | — |

- **VEL** is the VCA's velocity sensitivity, 0–100 %, default 100 %. The velocity term is `1 − VEL + VEL · v`, with `v` the note's velocity 0..1. It applies under ENV 1 and GATE; under ENGINE it is dimmed, and the engine's own velocity handling stands.
- The contour here is ENV 1's raw level. The ENV1 matrix source and the filter route keep reading contour × velocity (§ 1), so VEL shapes only the VCA.
- **Voice lifetime.** The voice goes inactive at the end of the first block in which its VCA source's end condition holds:
  - ENGINE: the engine is inactive (`Engines::is_active` false), as today;
  - ENV 1: ENV 1 is idle (after its release, or at the end of an AD's decay), or the engine is inactive;
  - GATE: the key is up and the release ramp has reached 0, or the engine is inactive.

  A silent engine can't sound through any VCA, so the engine's end always ends the voice. A fade (ADR 0027) ends it as today, whatever the source.
- The default comes from the engine (`ParamSnapshot::for_engine`), never from KIND. A Sound keeps its VCA setting when KIND changes.
- **The choices depend on the engine,** `EngineType::vca_sources()`: Algo and Modal offer ENGINE, ENV 1, GATE. VA offers ENV 1 and GATE only: a VA oscillator never goes inactive, so ENGINE would hold its notes forever.

### 4. KIND lays out the panel

- KIND is knob 1 of the FLT page. It sets knobs 2–6 to that synth's panel. Every kind has CUTOFF and RES.

| KIND | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|
| SVF | CUTOFF | RES | MODE | ENV | KEY |
| MOOG | CUTOFF | RES | ENV | KEY | DRIVE |
| SH-101 | FREQ | RES | ENV | MOD (LFO) | KYBD (KEY) |
| PROPHET-5 | CUTOFF | RES | ENV AMT | KEYBD (KEY, off/half/full) | DRIVE |
| TB-303 | CUTOFF | RESO | ENV MOD | DECAY (shortcut) | ACCENT |
| MS-20 | CUTOFF | PEAK | EG (ENV) | MG (LFO) | HP/LP (MODE) |

- A label is the original panel's word; the parameter is the same one everywhere. SH-101's FREQ is CUTOFF.
- The Moog panel follows the Minimoog's filter section: cutoff, emphasis, amount of contour, keyboard control; DRIVE stands for driving the filter from the mixer.
- **Shortcuts:** a knob can edit a parameter that lives elsewhere. TB-303's DECAY is the DECAY of the envelope the ENV route reads (ENV 2 by default). If that envelope's TYPE has no decay (AR, LOOP), DECAY shows dimmed and does nothing.
- **What a kind doesn't show is not applied.** The applied set is derived from the kind's panel (main page plus its FLT › MODE extras), so the two can't disagree. The 303 has no KEY, so key tracking is 0 for it whatever FLT KEY holds. Prophet, Moog and 303 have no LFO route. Hidden values are kept, not zeroed.
- **SVF extras:** DRIVE and LFO sit on SVF's FLT › MODE sub-page, so the SVF applies all its routes and today's DRIVE.
- **Prophet's KEYBD** is a stepped view of KEY: it shows OFF, HALF or FULL when KEY is 0, ½ or 1, and otherwise its percentage; one detent moves to the next step in that direction.
- **ACCENT** (TB-303) is a new filter parameter, defined with the 303 model in #127.

### 5. MODE follows KIND

| KIND | MODE choices (the first is the default) |
|---|---|
| SVF | LP24, LP6, LP12, BP12, BP24, HP24, NOTCH, PHASER |
| MOOG, SH-101, PROPHET-5 | LP24 |
| MS-20 | LP12, HP12 |
| TB-303 | LP18 |

- The SVF's default is LP24, today's mode, so it is listed first; the others keep #122's order.
- A single-mode kind shows its mode fixed and dimmed (for example "LP18"); turning it does nothing.
- Changing KIND keeps MODE if the new kind has it; otherwise MODE becomes the new kind's default.
- `mode ∈ kind.modes()` always holds: `FilterParams` enforces it on every write of KIND or MODE.

### 6. Per-kind default routing

| KIND | ENV SRC |
|---|---|
| SH-101 | ENV 1 |
| SVF, MOOG, PROPHET-5, TB-303, MS-20 | ENV 2 |

- KIND sets only ENV SRC. The VCA source belongs to the FLD / VCA block and defaults by engine (§ 3a). The SH-101 shared envelope is therefore ENV SRC ENV 1 (from KIND) with VCA ENV 1: automatic on VA, one turn of AMP's VCA knob on Algo or Modal. KIND never puts an envelope on an Algo or Modal VCA, which would change every such patch's level shape.
- **The rule on a KIND change:** if ENV SRC equals the old kind's default, it becomes the new kind's default; otherwise the user set it, and it stays. No "touched" flag is stored.
- Examples: SVF → SH-101 on a fresh Sound gives ENV SRC ENV 1. SVF with ENV SRC set to ENV 3 → SH-101 keeps ENV 3. SH-101 → PROPHET-5 moves ENV SRC back to ENV 2.
- LFO SRC and VCA have no per-kind default; KIND never changes them.

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
pub enum LfoSlot { Lfo1, Lfo2 }
pub enum EnvType { Adsr, Ahdsr, Ad, Ar, Loop }
pub enum FilterRoute { Env, Lfo, Key }
/// What a fixed route reads. `FilterRoute::source(&FilterParams)` is the one lookup:
/// Env → `Env(env_src)`, Lfo → `Lfo(lfo_src)`, Key → `Note`.
pub enum RouteSource { Env(EnvSlot), Lfo(LfoSlot), Note }
/// The VCA's source, on FLD / VCA. Discriminants are stored.
#[repr(u8)]
pub enum VcaSource { Engine = 0, Env1 = 1, Gate = 2 }
/// The matrix's source rows, in `Voice`'s order (§ 2).
pub enum ModSource { Env1, Lfo1, Env2, Env3, Lfo2, Vel, Note }
```

- `FilterParams` gains `kind: FilterKind`, `mode: FilterMode` (was `u8`), `lfo_amount: f32`, `env_src: EnvSlot`, `lfo_src: LfoSlot`. `kind` and `mode` are private, set through `set_kind` and `set_mode`, which keep `mode ∈ kind.modes()`. `fm_amount` is deleted.
- `EnvParams` gains `env_type: EnvType` and `hold: f32`.
- `ParamSnapshot.lfo` becomes `lfos: [LfoParams; 2]`.
- `OutParams` gains `vca: VcaSource` and `vca_vel: f32`: the VCA is the Sound's output stage, beside VOLUME.
- `BlockRef::AmpEnv`, `FilterEnv`, `AuxEnv` become `BlockRef::Env(EnvSlot)`; `BlockRef::Lfo` becomes `BlockRef::Lfo(LfoSlot)`, like `AlgoOp(Op)`. Matrix tags: E1, E2, E3, LF1, LF2 (≤ 3 characters, #15).
- `Envelope` exposes its raw `contour()` beside `current_level()` (contour × velocity).
- `Voice` holds `envs: [Envelope; 3]`, `lfos: [Lfo; 2]`, the previous block's routed `g` for the ramp, and the GATE ramp's level.

### ParamIds

No id is reused (ADR 0009). ENV and KEY keep ids 4 and 5: they are the same parameters, now read.

| Block | Param | ParamId | Kind | Range | Default | Modulatable |
|---|---|---|---|---|---|---|
| Filter | CUTOFF | 0 | continuous | 20..20,000 Hz | 1,000 | yes |
| Filter | RES | 1 | continuous | 0..1 | 0 | yes |
| Filter | DRIVE | 2 | continuous | 0..1 | 0 | yes |
| Filter | FM | 3 | **retired** | | | |
| Filter | ENV | 4 | continuous, Bi | −1..1 | 0 | yes (was no) |
| Filter | KEY (was TRACK) | 5 | continuous | 0..1 | 0 | yes (was no) |
| Filter | KIND | 6 (new) | enum, `FilterKind` | built kinds | SVF | no |
| Filter | MODE | 7 (new) | enum, `FilterMode` | the kind's list | LP24 | no |
| Filter | LFO | 8 (new) | continuous, Bi | −1..1 | 0 | yes |
| Filter | ENV SRC | 9 (new) | enum, `EnvSlot` | ENV1..ENV3 | ENV2 | no |
| Filter | LFO SRC | 10 (new) | enum, `LfoSlot` | LFO1, LFO2 | LFO1 | no |
| Env n | HOLD | 6 (new) | continuous | 0..10 s | 0 | no |
| Env n | TYPE | 7 (new) | enum, `EnvType` | 5 types | ADSR | no |
| LFO n | as today | 0–5 | | | | no |
| Out | VCA | 2 (new) | enum, `VcaSource` | ENGINE, ENV1, GATE | by engine: Algo, Modal ENGINE; VA ENV1 | no |
| Out | VEL | 3 (new) | continuous | 0..1 | 1 | no |

- ACCENT takes the next free Filter id when #127 lands.
- The envelopes and LFOs stay non-modulatable (`voice_reads` stays false for `Env` and `Lfo`): opening them is a separate decision.
- MODE's `ParamSpec` names all ten modes, for storage; the UI steps through `kind.modes()` only.

### Per-kind data, as `const`

```rust
pub enum PanelTarget {
    /// A Filter parameter.
    Filter(ParamId),
    /// A parameter of the envelope the ENV route reads (`env_src`).
    RouteEnv(ParamId),
    /// A voice parameter elsewhere (VCA).
    Out(ParamId),
}
pub enum KnobView { Spec, Steps(&'static [(f32, &'static str)]) }
pub struct PanelKnob { pub target: PanelTarget, pub label: &'static str, pub view: KnobView }
pub struct KindPanel {
    /// Knobs 2–6 of the FLT page.
    pub main: [PanelKnob; 5],
    /// Slots 4–5 of FLT › MODE.
    pub extras: [Option<PanelKnob>; 2],
}
pub struct RouteDefaults { pub env_src: EnvSlot }

impl FilterKind {
    pub const fn panel(self) -> &'static KindPanel;
    pub const fn modes(self) -> &'static [FilterMode];   // never empty; [0] is the default
    pub const fn defaults(self) -> RouteDefaults;
    pub const fn applies(self) -> Applies;               // derived from `panel`: env, lfo, key, drive
    pub const fn cost(self, mode: FilterMode) -> Cost;
}

pub struct EnvPanel { pub stages: [Option<ParamId>; 5] }   // slots 2–6 of an ENV page
impl EnvType { pub const fn panel(self) -> &'static EnvPanel; pub const fn uses(self, id: ParamId) -> bool; }
```

- `SVF_PANEL`: main CUTOFF, RES, MODE, ENV, KEY; extras DRIVE, LFO.
- The other kinds' panels are § 4's table, each added with its model issue.
- `EnvType` panels: ADSR A D S R –; AHDSR A H D S R; AD A D – – –; AR and LOOP A R – – –.
- Pure functions, no state: `routed_cutoff(base, env, lfo, note, amounts, applies) -> f32`, `kind_change(old, new, params) -> FilterParams` (§ 5 and § 6 rules), `env_type_change(stage, level, new_type) -> Stage`.

## Signal flow and rates

Per voice, per block, in `Voice::render`:

1. **Modulators.** Record every envelope's start level. The VCA envelope (ENV 1, when VCA is ENV 1) ticks per sample into a 64-sample stack buffer; every other envelope calls `advance(64)`. Both LFOs `process` once. Record the end levels.
2. **Matrix,** as today, from the start-of-block values of § 2's seven sources.
3. **Routes.** `routed_cutoff` at the block's end, from the end values; the start is the previous block's end, held in `Voice`. On a fresh note both are the end value, so the first block has no ramp.
4. **Engine → drive → filter,** the filter ramping its coefficient `g` linearly from start to end across the block. The kind's model defines what it ramps; the SVF ramps `g`.
5. **Folder, then VCA:** `sample · volume` under ENGINE (today's expression, bit for bit); `sample · volume · (env1[n] or gate[n]) · vel_term` under ENV 1 and GATE.
6. **Lifetime:** § 3a's rule, checked once at the end of the block.

A fading voice (ADR 0027) keeps its last cutoff and routes, as it keeps `played`; its envelopes keep running.

| Work | Rate | Paid when |
|---|---|---|
| VCA envelope tick and multiply | per sample | VCA is ENV 1 |
| GATE ramp and multiply | per sample | VCA is GATE |
| Other envelopes, `advance(64)`, closed-form over the linear segments | per block | always |
| LFO 1, LFO 2 | per block | always (today: only when the matrix has ≥ 2 sources) |
| `fast_tan` for the block-end `g` | per block | always, as today |
| `fast_exp2` in `routed_cutoff` | per block | any applied route nonzero |
| `g` ramp | per sample, one add | always |
| VEL, NOTE | per note | always |

- `Envelope` precomputes its rates (1 / (time · sr)) once per block, removing today's per-sample divide.
- `advance(n)` carries overflow across stage ends, so it equals `n` ticks within 1e-6 at the block boundary.
- `fast_exp2` is new in `dsp/mod.rs`: no libm, exact at integers (the integer part goes into the exponent bits), within 0.1 cent between them.

## CPU

- `Voice::cost = Engines::cost + CHAIN_COST + FilterKind::cost(kind, mode) + ModRouting::COST`.
- `ModRouting::COST` is estimated at 20, rounded high: about 8–10 per sample for the VCA envelope, plus about 3–5 amortised for two `advance`s, two LFOs and one `fast_exp2` per block (the `fast_tan` is today's). It is billed flat, whatever the VCA source, so the bill errs high. The bench's MODS row replaces the estimate.
- `FilterKind::cost(Svf, _)` is 0 until measured: today's SVF runs inside the chain that `CHAIN_COST` (10, from the FLOOR row) was measured over. The bench's new SVF row (1 OP at PHASER, its costliest mode, minus the 1 OP row) settles it; if it shows the SVF outside `CHAIN_COST`, SVF bills that reading.
- Each model issue commits its own kind's cost from its own bench row, the way ADR 0026 bills engines. #128's model half sets the targets.
- The FX diet leaves about 158 cycles per voice for the modulation modes on the costliest patch. The 20 here comes out of that.
- A KIND change that raises a Sound's cost can cut held notes (ADR 0026, #31).
- A VCA source other than ENGINE can shorten notes (ENV 1 ends a voice the engine would still ring), which frees voices sooner; the allocator doesn't count on it.
- **RAM:** `Voice` grows by two `Envelope`s, one `Lfo` and one `f32`; `ParamSnapshot` by the new fields and one `LfoParams`, in both the Sound and every voice's `played` copy (ADR 0027). The pool stays under `VOICE_RAM_BUDGET` (the existing `const` assert).

## Migration

Sounds live in RAM and the factory bank is code, so nothing on disk migrates. Every default reproduces today's sound:

| Old | New | Why nothing changes |
|---|---|---|
| `mode: u8` = 2 | `FilterMode::Lp24` (2) | Same discriminant, same code path |
| — | KIND SVF | Today's filter |
| `env_amount` (id 4), never read, 0 everywhere | The ENV route amount, id 4 | 0 → the route adds nothing, and `fc` is `base` bit for bit |
| `key_track` (id 5), never read, 0 everywhere | The KEY route amount, id 5 | Same |
| `fm_amount` (id 3), never read | Deleted; id 3 retired | Never read. Audio-rate filter FM can return as a new param |
| — | LFO route amount 0, ENV SRC ENV 2, LFO SRC LFO 1 | Amount 0 |
| — | VCA ENGINE (Algo, Modal), VEL 100 % | ENGINE is today's VCA, bit for bit; VEL is dimmed and unread under it |
| `envelopes[n]` | TYPE ADSR, HOLD 0 | Today's shape. ENV 1 advances per block under ENGINE; no Sound or golden routes ENV |
| FLD on the Algo chain | FLD / VCA (AMP) on the Algo and Modal chains | The folder already ran for both engines; only the page is new on Modal |
| `lfo` | `lfos[0]`; `lfos[1]` default | MORPH PAD's `lfo.rate` becomes `lfos[0].rate` |
| Matrix sources ENV, LFO | ENV1, LFO1 at indices 0, 1 | `ModState` indices unchanged |

- **Check:** before any change, record one audio golden per factory Sound (8). They stay bit-identical through this work.
- The `*_lfo_cutoff` goldens change: their cutoff now ramps across each block instead of stepping. They are re-recorded after the sanity gate (ADR 0011). Every other golden stays bit-identical.

## UI

- **FLT:** KIND · then the kind's five knobs. Layout BigViz, viz FilterResponse. KIND lists built kinds only.
- **FLT › MODE**, a new sub-page (id 59): MODE · ENV SRC · LFO SRC · extra · extra · (empty). Layout CellGrid.
  - A single-mode kind shows MODE as its fixed mode, dimmed.
  - LFO SRC is dimmed "--" on a kind with no LFO route.
  - Extras are the kind's (SVF: DRIVE, LFO); an empty extra is `--`.
- **AMP (FLD / VCA),** the `FOLDER` def (id 9) renamed "Fold / VCA", short AMP: FOLD · SYM · MIX · VCA · VEL · (empty). VEL is dimmed under ENGINE. The Algo map reads ALG · OSC · DRV · FLT · AMP · MOD; the Modal map MDL · FLT · AMP · MOD.
- **Dimmed:** a fixed or inapplicable slot draws its label and value in `theme::MID` with no value bar. Its encoder is ignored, and MIX+PLUS on it reports "not modulatable" (ADR 0017).
- **MOD's sub-pages** become ENV 1 (id 11, was ENVELOPE), ENV 2 (id 60), ENV 3 (id 61), LFO 1 (id 12), LFO 2 (id 62). An ENV page is TYPE · the TYPE's stages (§ Data model); its Adsr viz draws the TYPE's shape.
- **Retired:** `ENV_AMP`, `ENV_FILTER`, `ENV_AUX` (ids 13–15) and `ENVELOPE_CHAIN`. Their ids are not reused.
- **Slot binding:** a new `SlotBinding::FilterPanel(u8)` and `SlotBinding::EnvPanel(u8)` resolve against the Sound's KIND, `env_src` or TYPE, as `SelectedOp` resolves against the selected operator. `slot_addr` takes a `SlotCtx { sel_op, kind, env_src, env_types }` in place of `sel_op`.
- **Vizzes read by address, not slot:** FilterResponse reads CUTOFF, RES and MODE; Adsr reads the TYPE's stages. Today's `a(0), a(1)` and `a(0..3)` would read KIND and TYPE.
- **Animation:** a KIND or TYPE change re-seeds the page's animators to the new slots' values. A lerp between two different parameters' values would draw a meaningless sweep.
- **Matrix display:** the UI's stand-in source values (`ui/mod.rs`) cover all seven: ENV n its SUS (AD 0, AR 1, LOOP ½), LFO n its own display LFO, VEL 1, NOTE 0.

## Tests

- **Knobs are honest:** for every built kind, every applied knob on FLT and FLT › MODE changes a render of a held saw (engine-independent: Algo and Modal); every inapplicable one leaves it bit-identical.
- **Routed cutoff (pure):** 0 routes return `base` bit for bit; KEY 1 at notes 48, 60, 72, 84 gives exactly base/2, base, 2·base, 4·base; ENV 1 at env 1 is +10 octaves before the clamp; the clamp holds at both ends; a route the kind doesn't apply contributes 0.
- **`fast_exp2`:** exact at −12..12; within 0.1 cent on a 1/1000 grid.
- **Clicks:** an ENV sweep (attack 1 ms and 50 ms, ENV ±1, RES 0.5) has a largest second difference at most 1.5× that of the same render with the cutoff held at each block's mean; an ENV SRC or LFO SRC switch mid-note passes the same test.
- **Envelopes:** each TYPE's stage sequence and timing within one sample, gated and un-gated; AD ignores key-up; LOOP loops while held and releases from its current level; `advance(64)` matches 64 ticks within 1e-6 across every stage boundary; a TYPE change mid-note never moves the level.
- **Shared envelope:** with ENV SRC ENV 1 and VCA ENV 1, the filter route and the VCA read one `Envelope`, and changing ENV 1's decay moves both the cutoff sweep and the level.
- **VCA:**
  - ENGINE: every factory Sound is bit-identical to its golden, and `Sound::init` for Algo and Modal defaults to ENGINE;
  - ENV 1: an Algo and a Modal note follow ENV 1's contour within 1 %; the voice frees at the end of the block in which ENV 1 goes idle, while the engine still sounds;
  - GATE: the output is the engine's while held, reaches 0 exactly 64 samples after key-up, and the voice frees then; a gate edge passes the click test;
  - VEL: at 0 two velocities give the same level; at 100 % the level scales with velocity; under ENGINE, VEL changes nothing;
  - lifetime: under every source, an inactive engine ends the voice;
  - the fold comes before the VCA: with FOLD on, halving ENV 1's level halves the output exactly.
- **Matrix:** seven sources, in order; a route from each moves its destination; `ModState` built with two sources (old Sounds, MORPH PAD) still maps 0 → ENV1 and 1 → LFO1.
- **Kinds and modes (pure, over every built kind):** `modes()` non-empty; `mode ∈ kind.modes()` after every `set_kind` and `set_mode`; a single-mode kind can't be moved off its mode; `applies()` matches the panel.
- **Kind-change rule (pure, on `RouteDefaults` pairs, so it runs before a second kind exists):** a setting equal to the old default moves; any other stays; § 6's three examples.
- **Shortcut:** the 303's DECAY edits the decay of `env_src`'s envelope, and dims under AR and LOOP (with #127).
- **Cost:** `Voice::cost` includes `ModRouting::COST` and `FilterKind::cost`; the allocator's voice count for each factory Sound is recomputed with the 20 added, and `cost_test` pins the new counts.
- **UI:** new screen goldens: FLT (SVF), FLT › MODE (SVF), AMP under ENGINE (VEL dimmed) and ENV 1, ENV pages at each TYPE, LFO 2; the Algo and Modal maps with AMP. The dimmed readout gets its golden with the first single-mode kind (#123).
- **Goldens:** the 8 factory goldens; the two `*_lfo_cutoff` re-recorded; all others unchanged.
- **Bench:** a MODS row (1 OP, VCA ENV 1 at VEL 50 %, all three routes applied and nonzero, all seven sources routed) and an SVF row (1 OP, PHASER), each minus the 1 OP row.

## ADRs

- **New: envelope slots, fixed routes, and one VCA per chain.** Slots, not shapes; TYPE; the three fixed routes with SOURCE, in octaves; the matrix's seven sources in that order; the FLD / VCA block, its three sources, VEL, the per-engine defaults and the lifetime rule. It supersedes in part ADR 0022's note that no engine puts the amp envelope on the VCA. The VA spec's ADR 0033 then points here instead of superseding that note itself.
- **New: KIND lays out the panel.** Per-kind panels as `const` data; not shown means not applied; shortcuts; MODE follows KIND; the kind-change rule. ParamIds per the table; FM's id 3 retired (ADR 0009).
- The model choice, topologies and oversampling policy are #128's other ADR.

## The VA spec

`docs/superpowers/specs/2026-09-27-va-engine-design.md` plans the amp envelope as VA's VCA by a branch on `EngineType::Va` in `Voice::render`, with the voice active while the amp envelope is not idle (its § Storage bullet "The amp envelope is the VCA for a VA Part", and ADR 0033's bullets). Under this spec that is VA's default VCA source, ENV 1, with the same audible result and the same lifetime; no engine-type branch is needed. The VA spec needs an edit: that bullet should say VA's `for_engine` default is VCA ENV 1 (this spec's § 3a), and ADR 0033 should drop its own supersession of ADR 0022's VCA note in favour of this spec's routing ADR. VA's "only the amp envelope retriggers" and "silent after note-off" hold as written. VA's VCA choices leave out ENGINE (§ 3a), which the VA spec should also state.

## Out of scope

- Each filter model's DSP (topology, references, cost targets, KIND crossfade): #123–#127, specified under #128 before it is built.
- Envelope curves other than linear; LEVEL and VEL (#112).
- Modulating envelope or LFO parameters.
- Audio-rate filter FM.
- A second filter, or filter routing in series/parallel.

## Risks

- **GATE's 64-sample edges** soften a VA's attack by 1.3 ms and still click on a loud low note if the ramp is too short. The click test decides; the ramp length is one const.
- **ENV 1 on an Algo patch multiplies two envelopes** (the operators' and ENV 1). That is the point of the switch, but a user may expect it to replace them. The AMP page's VEL dimming under ENGINE is the only hint; a later UX pass may add one.
- **The ramp is linear in `g`,** not in log-frequency. A 10-octave sweep inside one block bends toward the top. If a test hears it, split the block into four 16-sample ramps (four `fast_tan`s per block).
- **A matrix route and a fixed route on CUTOFF sum in different units** (Hz and octaves). It is documented, and the fixed route is the one on the page. If it confuses, the matrix's CUTOFF route moves to octaves in a later ADR (not bit-exact, ADR 0010).
- **The FLOOR row's 5 cycles look low** for a two-stage SVF with a per-sample divide. The SVF row checks whether `CHAIN_COST` really covers it; if not, the SVF's cost joins every voice's bill and a heavy patch can lose a voice.
- **20 cycles of the ~158 left per voice** go to routing, before any filter model's cost. The Moog and TB-303 models may need oversampling; #128 has to set their targets inside what's left.
- **`played` grows** in every voice (ADR 0027) and in the per-frame publish. It is under budget but not free.

## Defaults chosen

The owner's decisions didn't settle these; each is a default until the owner says otherwise.

1. Envelope output stays contour × velocity for the matrix and the filter route; the envelope's own LEVEL and VEL stay unread and leave the ENV pages (#112). The VCA uses the raw contour with its own VEL.
2. Stage parameters per TYPE: ADSR A D S R; AHDSR A H D S R; AD A D (one-shot, ignores key-up); AR A R (holds 1 while held); LOOP A R (loops while held, releases at R).
3. TYPE change mid-note maps the running stage and never moves the level.
4. Route ranges: ENV ±10 octaves, LFO ±5 octaves, KEY 0..1 octave per octave, pivot C4 (60).
5. ENV and LFO are one bipolar parameter each on every kind, even where the original panel was unipolar (Prophet, SH-101): one parameter shown in several places means the same everywhere, and clamping on a KIND change would destroy a value.
6. Route amounts (ENV, LFO, KEY) are modulatable by the matrix.
7. The AMP short label; the page name "Fold / VCA"; VEL's default of 100 %; GATE's 64-sample linear edges.
8. VA's VCA choices are ENV 1 and GATE only (no ENGINE).
9. KIND never sets the VCA source; the VCA's default comes from the engine only, and the SH-101 shared envelope needs VCA ENV 1 from the user on Algo and Modal.
10. The kind-change rule: ENV SRC equal to the old kind's default takes the new kind's default; no touched flag is stored. LFO SRC and VCA are never changed by KIND.
11. Lifetime: the end condition is checked once per block, and an inactive engine ends the voice under every source.
12. SVF's MODE default is LP24 (today's), listed first; the other kinds follow #122's order.
13. SVF's panel extras are DRIVE and LFO, on FLT › MODE, so the SVF keeps applying DRIVE.
14. Moog panel: CUTOFF · RES · ENV · KEY · DRIVE, with KEY continuous.
15. Prophet's KEYBD is a stepped view (OFF, HALF, FULL) of the continuous KEY.
16. TB-303's DECAY follows ENV SRC, and dims when that envelope's TYPE has no decay.
17. FM is retired; its id 3 is never reused.
18. Matrix source order ENV1, LFO1, ENV2, ENV3, LFO2, VEL, NOTE, keeping today's indices 0 and 1; NOTE = (note − 60) / 64.
19. Envelope and LFO parameters stay non-modulatable.
20. `FilterKind` gains each variant with its model; there are no stand-in kinds.
21. Envelopes not on the VCA advance per block; the VCA envelope ticks per sample.
22. New page ids 59–62; retired page ids 13–15 are not reused.
23. `BlockRef::Env(EnvSlot)` and `BlockRef::Lfo(LfoSlot)` replace the role-named envelope refs and the single LFO ref.
