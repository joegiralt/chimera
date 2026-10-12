# 0131. Formant operators are Algo operator kinds, after the FS1R patent

- **Status:** Proposed
- **Deciders:** owner (2026-09-30, 2026-10-12), firmware

## Context
#243 asks for FS1R-style formant synthesis in the Algo engine: voiced bands (FRMT) and unvoiced noise bands (NOIS) that sit in an algorithm beside table operators and modulate and are modulated like them. The design is `docs/superpowers/specs/2026-09-30-formant-operators-design.md`.

The hard part is cost. ADR 0026 promises four voices for every Sound that can be built, under its heavy-route worst case. A formant operator costs two to six times a table operator, and its cost depends on whether anything modulates it. The counted terms (below) can't hold even two linked formants at four voices, and at three voices they hold one linked formant or three unlinked ones.

## Decision

### Provenance
- **The structure is from Yamaha's US5610354** (expired): a window oscillator at the pitch, a carrier reset each window cycle, two π-offset window lanes over two periods, PM into the window and the carrier, and noise low-passed, offset and ring-modulated to a centre.
- **Behaviour only is from FSVR's prose docs** (GPL-3, ADR 0032): the width is a time, so it is constant in Hz; the carrier restart holds the formant still; the skirt law doubles the window exponent per step; a noise band is a ring-modulated low-pass with an absolute width. **No FSVR code, tables or constants are used.**
- **The vowel averages are Peterson & Barney (1952)**, measurement data.
- **Everything else is ours** (spec § Provenance): the polynomial window, the SKIRT-free width floor and its `hypot`, the odd centred carrier, sub-sample starts on the unmodulated clock, PM as a bounded grain delay and its divide-free law, the exact-energy level law and its −12 dB reference, the peak-referenced modulator signal, carrier-only feedback, the −60 dB edge law, the VOWEL macro and its slots. `THIRD_PARTY.md` doesn't change.

### Kinds and frozen codes
- An operator's kind is an `OpKind`: `Table(WaveId)`, `Formant` or `Noise`, read only through `OpKind`. `AlgoParams` holds the six in a private `Kinds` newtype, beside its private ALG A and ALG B.
- Frozen (ADR 0045): WAVE 16 `FRMT` and 17 `NOIS`; operator ids 13 `FRMT`, 14 `BW`, 15 `SKIRT`, 16 `KEY`, 17 `RES`, 18 `VSLOT`; VSLOT 0–4 `AUTO`, `F1`, `F2`, `F3`, `OFF`; Algo id 4 `VOWEL`. INIT's new values are in `neutral_v1.txt` (ADR 0087).

### Cost terms: counted, never estimated
Counted on the `bench-algo` ELF by ADR 0056's method (`tools/loop_count.py`; the kernel's loops stay out of line under `span-count`, that ELF only):

| Term | Cycles a sample |
|---|---|
| `COST_FRMT` (SKIRT 1–3, no link in: spans between grain starts) | 220 |
| `COST_FRMT_LINKED` (SKIRT 1–3, a link in: the moving loop) | 380 |
| `COST_FRMT_ONE` / `COST_FRMT_ONE_LINKED` (SKIRT 0, one lane) | 210 / 380 |
| `COST_NOIS` (the loop 75, and its band law taken afresh each block, 6.3: Task 12) | 85 |
| `COST_FRMT_MOVE` (the laws taken afresh each block) | 40, on every FRMT |

- **Each term is its worst block for a grain clock up to C8** (MIDI 108, 4,186 Hz): `SPANS_COVERED` = ⌈64·4186/48000⌉ + 1 = 7 spans a block unlinked, 7 fires linked. A span adds 276 instructions (the span path with its fire, 93, and the block path, 183: an upper bound), a fire 47. So `<0, S3>` = 60 + 85.1 + 48.5 + 19.9 → 220, `<0, S0>` → 210, `<1, S3>` = 60 + 290.7 + 8.3 + 19.9 → 380, `<1, S0>` → 380. A typical 2.2 spans a block gave 190/180/380/370; MIDI 127 runs 18.
- **Past C8, per voice.** A formant voice whose pitch can reach past C8 pays `AlgoEngine::high_note_extra` at admission and in every recost: each priced FRMT's spans (fires) past 7 at its grain clock, 276 (47) instructions each, by ADR 0056's cycles. The reach is the note with TRNSP, the PITCH offset, TUNING up, the bend's range, every PITCH route at full swing, a glide steal's start, and the operator's COARSE and FINE (FINE at its top where a route moves it), with DETUNE at its top always.
- `COST_FRMT_MOVE` is billed always, as Modal bills its pitch (#437): the pitch wheel or a glide steal moves any voice's laws with no route to see.
- `AlgoEngine::cost` bills a FRMT `frmt_cost(skirt, linked)`: its lanes plus the moving term. `COST_FEEDBACK` is billed for table and FRMT operators with FDBK, never for NOIS.

### Per-algorithm link billing
- A FRMT is billed **linked** only when an operator in the patch's algorithm modulates it: `Linked::of(ALG A, ALG B)`, the operators with an incoming link. Otherwise it is billed unlinked.
- **MORPH:** the plan runs ALG A's and ALG B's links at every MORPH, so `Linked` is their union: each operator bills at the heavier of the two algorithms.
- The budget check, `AlgoEngine::cost` and admission all read the same `Linked` and the same `frmt_cost`. `OpKind::charge(linked)` is `frmt_cost(1, linked) − COST_OP` for a FRMT (SKIRT is billed at its worst, so a SKIRT edit never breaks the budget), `COST_NOIS − COST_OP` for a NOIS and 0 for a table.
- **FDBK isn't a link.** A FRMT's feedback goes into its carrier's phase only (`Carry::pm_phase`). The grain delay reads `pm_cycles`, the links' PM alone, and the kernel picks the moving loop only when the operator has a link (`links[0]`). So a FRMT with FDBK and no link runs the unlinked spans and bills unlinked plus `COST_FEEDBACK` (`formant_feedback_is_not_a_link`).

### The budget and the three-voice floor
- **The reference routing is `ModRouting::HEAVY`** (326, ADR 0026's convention refreshed: ENV B BURST on the VCA, a slide, FREQ and Q, 16 registry destinations, three FUNC LFOs), plus the filter's `CLEAN_MOVE_COST` 75 and `FILTER_LEVEL_COST` 35 and `Voice::CHAIN_COST` 36: `HEAVY_OVERHEAD` 472. It is a reference, not the maximum a Part can route.
- **The floor (owner, 2026-10-12): a Sound holding a FRMT keeps three voices; every other Sound keeps ADR 0026's four.** Both budgets are derived at build time, never typed in:

  ```
  share = SampleBudget(rev V, 75 %) 7,500 − FxBus::COST_TAPE 1,494 = 6,006
  FORMANT_BUDGET      = 6,006/4 − 472 − 832 = 197   (tables and NOIS)
  FORMANT_BUDGET_FRMT = 6,006/3 − 472 − 832 = 698   (a Sound with a FRMT)
  ```

  832 is the costliest table Sound (A16 ∪ A17, six operators with FDBK).
- **The rule:** `Σ charge(kind, linked) ≤` the budget of the Sound's floor. A FRMT charges 360 linked and 200 unlinked; a NOIS 25.
  - `FORMANT_N` (linked) = ⌊698/360⌋ = **1**; `FORMANT_N_UNLINKED` = ⌊698/200⌋ = **3**. One linked FRMT and one unlinked fit (560); with two unlinked it is 760.
  - The demo Sounds are on A1 (no links), so they keep the spec's voicings: CHOIR three FRMT and a NOIS (625), TALK BASS a sine and three FRMT (600), BREATH two NOIS (50, four voices).
- **Only a patch within budget can be built.** `Kinds::new` checks its kinds unlinked at compile time; `AlgoParams::new` checks them on its algorithms. The writes are `try_set_kind` and `try_set_algs`, and every path goes through them: WAVE and ALG A/B block writes, `set_enum_code`, the decoder, the UI encoders and the param sweep.
- **An edit that would overspend is refused** and changes nothing. WAVE's and ALG A/B's encoders step over refused kinds and algorithms, and the cell toasts FORMANT BUDGET FULL.
- **A load past the budget degrades:** Sounds write ALG A and B before their operators, so the file's algorithm lands first and each operator after it is checked in order; a kind that doesn't fit stays the base's W1. An algorithm that arrives after its operators and would link into a full patch keeps the base's. Either way the load says FORMANT BUDGET FULL.

### Laws recorded here
- **Held laws.** A formant's per-block law is kept while its inputs hold (`LawKey`): every played FRMT, BW, LEVEL, FINE, DETUNE and FDBK, VOWEL, MORPH, pitch, slide, rate, note, TRNSP and the COARSEs. The latched slots, KEY, SKIRT, the kinds the engine runs and its plan change only at a note-on, a kind swap or a plan change, and each drops the key where it writes. So a WAVE spin or an ALG edit under a held note retakes the law, bit for bit the law taken afresh every block (`held_laws_follow_kind_swaps_and_alg_edits`).
- **The grain delay is divide-free:** `o = rest + h·g`, `h` half the slack, `g = f(pm/(3h) + β) − β` with `f(v) = v(3 − v²)/2` on v held to [−1, 1]. It stays inside the slack, C¹ onto both ends, slope ½ at rest (`(1 − v²)/2` ≤ ½); the reset grain (β = −1) opens at once and only delays. Worst modulated alias case −66.7 dB (`158 A3 99`, SKIRT 0); odd mean 2.58e-5 (bound 1e-4).
- **PM depth cap:** the links' share `k = min(clamp((24 kHz − FC − m_q·B_min)/reach, 0, 1), DELAY_SLEW·f0/dev)`, `DELAY_SLEW` 4, ramped per sample into the delay and the carrier. It holds ratio-1 PM into a formant to about 0.64 cycles (LEVEL 76 and up sound alike); 8 would allow 1.27 with 3.7 dB less alias margin.
- **NOIS links are outside the slew bound.** Noise is left out of `dev`, so a full-level NOIS link can move the delay faster than `DELAY_SLEW`. Its sidebands are noise, so no tonal alias.
- **p̂ under drive:** feedback and PM displace the carrier against its window, so both gains reference `driven_peak = min(1, p̂ + lanes·2π·φ)`, continuous in the drive. The peak stays ≤ 1 as a modulator and as a carrier; the level cost is up to −13.7 dB at BW 10 kHz. FDBK 5's modulator swing spreads 16.9 dB across BW.
- **Raw DC under FDBK:** −2.28e-3 before the voice's 5 Hz blocker (7.3e-6 after), in `formant_operators_are_stable`'s case.
- **`VOICE_CHAIN_BYTES` 2,112 → 2,176:** the formant operators' 13 glided settings in `AlgoLive`.

### Partial supersessions
ADR 0022's destination list (spec § 7); ADR 0026's cost terms, with its floor kept by the budgets (three voices for a Sound with a FRMT, by the owner's ruling); ADR 0031's "the costliest patch gets six voices", which still holds for tables.

## Alternatives considered
- **Bill every FRMT linked** (`Kinds` blind to the algorithm): N = 1 at three voices, and no three-formant vowel.
- **Bill the moving term only under routes and glides** (ADR 0117-style): linked N = 2, but the pitch wheel and glide steals move laws with no route to see.
- **Cut 11 more cycles from the linked path:** linked spans cost about 5.5 KB of flash.
- **Four voices for every Sound:** 197 holds one unlinked FRMT.
- **One FRMT for alpha:** no vowels.

## Consequences
- Up to three formant carriers per Sound, or one formant a modulator reaches; mixes in between by the rule.
- An ALG A or B edit can be refused, which a table-only Sound never sees.
- Task 15 completes this ADR with the bench rows (FRMT0, FRMT3, FRMT PM, NOIS, FRMT MAX, CHOIR), and the chip bench confirms the counted terms at ship.

## Sources
- `docs/superpowers/specs/2026-09-30-formant-operators-design.md` §§ 1, 3, 9.
- US5610354 (expired); FSVR's prose docs (GPL-3, behaviour only, ADR 0032); Peterson & Barney (1952).
- ADR 0026, 0031, 0045, 0056, 0087, 0112, 0117, 0124; #243, #437.
