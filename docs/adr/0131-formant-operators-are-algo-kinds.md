# 0131. Formant operators are Algo operator kinds, after the FS1R patent

- **Status:** Proposed
- **Deciders:** owner (2026-09-30, 2026-10-12), firmware (counts completed 2026-10-13, Task 15)

## Context
#243 asks for FS1R-style formant synthesis in the Algo engine: voiced bands (FRMT) and unvoiced noise bands (NOIS) that sit in an algorithm beside table operators and modulate and are modulated like them. The design is `docs/superpowers/specs/2026-09-30-formant-operators-design.md`.

The hard part is cost. ADR 0026 promises four voices for every Sound that can be built, under its heavy-route worst case. A formant operator costs 4.3 times a table operator with no link into it (260 cycles a sample against `COST_OP`'s 60) and 7.5 times with one (450), so its cost depends on whether anything modulates it. The counted terms (below) can't hold even two linked formants at four voices, and at three voices they hold one linked formant or three unlinked ones.

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
Counted on the `bench-algo` ELF by ADR 0056's method (`tools/loop_count.py`; the kernel's loops stay out of line under `span-count`, that ELF only): `COST_OP` 60, plus each path's instructions over `run_one::<0>`'s 52 at 1.46 cycles and 10 % (× 1.606), plus per-block work over 64 samples, plus 12 cycles a divide or root over 64, rounded up to 10. Recounted on the final code (Task 15, 2026-10-13):

| Term | Path | Cycles a sample |
|---|---|---|
| `COST_FRMT` (SKIRT 1–3, no link in: spans between grain starts) | `run_formant::<3>` | 220 |
| `COST_FRMT_ONE` (SKIRT 0, no link in) | `run_formant::<0>` | 210 |
| `COST_FRMT_LINKED` (SKIRT 1–3, a link in: the moving loop) | `run_moving` + `tick_at` | 390 |
| `COST_FRMT_ONE_LINKED` (SKIRT 0, a link in) | the same at S0 | 380 |
| `COST_NOIS` / `COST_NOIS_LINKED` | `run_noise::<0>` / `::<1>` | 80 / 100 |
| `COST_FRMT_MOVE` / `COST_FRMT_MOVE_LINKED` (the laws taken afresh) | `take_laws` and its calls | 40 / 60, on every FRMT |

**The counts** (`loop_count.py` on the bench-algo ELF; `tick_at`'s path is the longest acyclic path through its SKIRT arm, both lanes open):

| Term | Loop (instructions, Δ over 52) | Spans or fires a block (7) | Per block | Sum | Term |
|---|---|---|---|---|---|
| `<0, S3>` | 105, Δ 53 → 85.1 | 7 × (93 + 183) / 64 → 48.5 | 17.7 | 211.3 | **220** |
| `<0, S0>` | 100, Δ 48 → 77.1 | 7 × (93 + 179) / 64 → 47.8 | 17.7 | 202.6 | **210** |
| `<1, S3>` | `run_moving` 70 + `tick_at` 167 = 237, Δ 185 → 297.1 | 7 × 47 / 64 → 8.3 | 20.3 + 3.7 | 389.3 | **390** |
| `<1, S0>` | 70 + 161 = 231, Δ 179 → 287.5 | 8.3 | 20.3 + 3.7 | 379.7 | **380** |
| NOIS `<0>` | 55, Δ 3 → 4.8 | — | 1.9 + 4.1 + 6.3 | 77.1 | **80** |
| NOIS `<1>` | 61, Δ 9 → 14.5 | — | 2.0 + 6.5 + 6.3 + 3.7 | 93.0 | **100** |
| move, unlinked | — | — | 1,311 instructions, 23 divides | 37.2 | **40** |
| move, linked | — | — | 1,997 instructions, 32 divides | 56.1 | **60** |

- **Per block, a FRMT:** `formant_blocks` 242 (its 22× unrolled key test), `formant_block` 106 and a divide, `Kernel::formant_op`'s path (134 unlinked, 139 linked) and its three copies of 65–68 B (72 each); linked, `link_sum`'s block 97. 698 instructions → 17.7, or 800 → 20.3.
- **Links into a FRMT or NOIS** are summed by `link_sum`, 5.44 instructions a link a sample (8.7 cycles) against `COST_LINK`'s 8: the linked terms carry the difference for five links, 3.7.
- **NOIS per block:** `run_noise`'s block, `noise_op` 118 and the engine's band block (about 45, in line in `render`), and `take_noise_law` 213 instructions and 5 divides (6.3). Linked NOIS had been counted only at `<0>` (Task 12's 85); at `<1>` it is 93, so the two are split, as the FRMT's are.
- **The moving law:** `take_laws` 507 instructions and 6 divides, its scan of 15 plan edges (4× unrolled, +105) and its six operators' turns (+55), `formant_law_pm` 290 and 10, `LawBlock::new` 113 and 6, `formant_fc` 126 and 1, and one 100 B copy. With links, `drive` sums up to five edges (four more bodies of 140 instructions and 2 divides) and a FRMT source's `formant_fc`. `take_laws` now writes each law in place: the returned array was a 624 B copy a block (about 550 instructions, 14 cycles).
- **SKIRT 0 is not one lane.** The span path computes both lanes every sample, a closed lane at window 0 (no test in the loop). `COST_FRMT_ONE` is lower than `COST_FRMT` only by SKIRT 0's window squarings.
- **Conservative for shipping.** The bench-algo ELF puts `Grains::fire` in line in each span, so the span path counts it there (93 with it, about 17 more than the shipping call costs a span); the shipping build calls one out-of-line `fire`. A linked fire is counted at the shipping call, 42 + 5.
- **What moved, Task 11b → 15:** `COST_FRMT_LINKED` 380 → 390 (the longest `tick_at` path, 167, against 163 by hand; `link_sum` out of line); NOIS 85 → 80 unlinked and 100 linked; the moving law 40 → 40 and 60 linked. N is unchanged.
- **Each term is its worst block for a grain clock up to C8** (MIDI 108, 4,186 Hz): `SPANS_COVERED` = ⌈64·4186/48000⌉ + 1 = 7 spans a block unlinked, 7 fires linked. A span adds 276 instructions (the span path with its fire, 93, and the block path, 183: an upper bound), a fire 47. A typical 2.2 spans a block would give 190/180; MIDI 127 runs 18.
- **Past C8, per voice.** A formant voice whose pitch can reach past C8 pays `AlgoEngine::high_note_extra` at admission and in every recost (there, the Part's reach is taken once a block, `HighNote::of`, and each voice's from it, `HighNote::extra`: the same bill): each priced FRMT's spans (fires) past 7 at its grain clock, 276 (47) instructions each, by ADR 0056's cycles. The reach is the note with TRNSP, the PITCH offset, TUNING up, the bend's range, every PITCH route at full swing, a glide steal's start, and the operator's COARSE and FINE (FINE at its top where a route moves it), with DETUNE at its top always.
- **Recost's reach is billed** (`HighNote::PART_COST` 70, `HighNote::VOICE_COST` 7): the reach above is per-block work, so a formant Part's first voice pays the Part's (as `Knobs::COST`) and each formant voice its own, at admission (`Instrument::high_note`) and in every recost alike (`HighNote::bill`; `a_formant_part_bills_its_reach_once_and_each_voice`). Counted by ADR 0056's method on the bench-algo ELF (2026-10-13), each function out of line there, at its worst trip counts:
  - **Per Part**, once a block: `HighNote::of` 483 instructions and 9 divides (`SpanReach::of`'s six FRMTs at 50 and a divide each, the 48 B clear, the tail's three divides); then the route scans, bounded together a destination at a time: a destination costs at most 109 across both `algo_routed` scans (83 where it is the scanned param with eight zero amounts, 8 where not) and `pitch_reach`'s two finds (9–13 each), so 16 destinations 1,744; `pitch_reach`'s two swings over 8 sources at 23, 368; prologues and glue about 100. About 2,700 instructions and 9 divides: 4,445 cycles a block, **70** a sample rounded up (the earlier "about 78" took each scan at its own worst).
  - **Per voice**: `HighNote::extra` 206 instructions and 7 divides at six FRMTs (`fast_exp2` and a span count each), about 215 with recost's glue: 430 cycles a block, **7** a sample rounded up.
- **The moving term is billed always** (`COST_FRMT_MOVE`, as Modal bills its pitch, #437): the laws are only *taken* on a block whose inputs changed (`LawKey`), but the pitch wheel or a glide steal can change them any block with no route to see, so admission can't know a held block from a moving one.
- `AlgoEngine::cost` bills a FRMT `frmt_cost(skirt, linked)`, its lanes and its moving term, and a NOIS `nois_cost(linked)`. `COST_FEEDBACK` is billed for table and FRMT operators with FDBK, never for NOIS.

### Per-algorithm link billing
- A FRMT or NOIS is billed **linked** only when an operator in the patch's algorithm modulates it: `Linked::of(ALG A, ALG B)`, the operators with an incoming link. Otherwise it is billed unlinked.
- **MORPH:** the plan runs ALG A's and ALG B's links at every MORPH, so `Linked` is their union: each operator bills at the heavier of the two algorithms.
- The budget check, `AlgoEngine::cost` and admission all read the same `Linked` and the same `frmt_cost`. `OpKind::charge(linked)` is `frmt_cost(1, linked) − COST_OP` for a FRMT (SKIRT is billed at its worst, so a SKIRT edit never breaks the budget), `nois_cost(linked) − COST_OP` for a NOIS and 0 for a table.
- **FDBK isn't a link.** A FRMT's feedback goes into its carrier's phase only (`Carry::pm_phase`). The grain delay reads `pm_cycles`, the links' PM alone, and the kernel picks the moving loop only when the operator has a link (`links[0]`). So a FRMT with FDBK and no link runs the unlinked spans and bills unlinked plus `COST_FEEDBACK` (`formant_feedback_is_not_a_link`).

### The budget and the three-voice floor
- **The reference routing is `ModRouting::HEAVY`** (326, ADR 0026's convention refreshed: ENV B BURST on the VCA, a slide, FREQ and Q, 16 registry destinations, three FUNC LFOs), plus the filter's `CLEAN_MOVE_COST` 75 and `FILTER_LEVEL_COST` 35 and `Voice::CHAIN_COST` 36: `HEAVY_OVERHEAD` 472. It is the reference heavy routing, not the maximum a Part can route: more routes, FOLD, DRIVE, ALGO taps or ORBIT playing can take a voice below the floor, as ADR 0026 already says of table Sounds; admission bills what is really routed.
- **The floor (owner, 2026-10-12): a Sound holding a FRMT keeps three voices; every other Sound keeps ADR 0026's four.** Both budgets are derived at build time, never typed in:

  ```
  share = SampleBudget(rev V, 75 %) 7,500 − FxBus::COST_TAPE 1,494 − CONTROL_RESERVE 24 = 5,982
  FORMANT_BUDGET      = 5,982/4 − 472 − 832 = 191   (tables and NOIS)
  FORMANT_BUDGET_FRMT = (5,982 − 70)/3 − 472 − 832 − 7 = 659   (a Sound with a FRMT)
  ```

  `CONTROL_RESERVE` is what admission reserves beside the bus before any voice: the knobs' settle 14 (ADR 0116) and the LFO clocks 10 (ADR 0135). 832 is the costliest table Sound (A16 ∪ A17, six operators with FDBK). A Sound with a FRMT also pays recost's reach, 70 a Part and 7 a voice (above), so its budget absorbs both: three voices and the Part's term fit the share.
- **The rule:** `Σ charge(kind, linked) ≤` the budget of the Sound's floor. A FRMT charges 390 linked and 200 unlinked; a NOIS 40 linked and 20 unlinked.
  - `FORMANT_N` (linked) = ⌊659/390⌋ = **1**; `FORMANT_N_UNLINKED` = ⌊659/200⌋ = **3** (698 before the reach was billed: N unchanged). One linked FRMT and one unlinked fit (590); with two unlinked it is 790.
  - **Task 11b's outcome, confirmed by Task 15's recount:** the plan's target (a SKIRT 1–3 term ≤ 120, N = 3 at four voices) was not met; the owner's three-voice floor and per-algorithm billing (2026-10-12) took its place, so N = 3 unlinked and the spec's three-formant voicings stand. No two-formant fallback was needed.
  - The demo Sounds are on A1 (no links), so they keep the spec's voicings: CHOIR three FRMT and a NOIS (620), TALK BASS a sine and three FRMT (600), BREATH two NOIS (40, four voices).
  - A NOIS-only Sound on a heavily linked pair can now hold fewer than six NOIS at four voices (A16 ∪ A17: five linked at 40 and one unlinked is 220 > 191); on A1 six still fit.
- **Only a patch within budget can be built.** `Kinds::new` checks its kinds unlinked at compile time; `AlgoParams::new` checks them on its algorithms. The writes are `try_set_kind` and `try_set_algs`, and every path goes through them: WAVE and ALG A/B block writes, `set_enum_code`, the decoder, the UI encoders and the param sweep.
- **An edit that would overspend is refused** and changes nothing. WAVE's and ALG A/B's encoders step over refused kinds and algorithms, and the cell toasts FORMANT BUDGET FULL.
- **A load past the budget degrades:** Sounds write ALG A and B before their operators, so the file's algorithm lands first and each operator after it is checked in order. The floor rises with a FRMT, so a Sound within budget whole can be over it in operator order (A17, NOIS on 1–5 before a FRMT on 6: 200 > 191 until the FRMT lands, 400 ≤ 659 after). So `SoundCheck` keeps each refused kind or algorithm and retries them once the Sound's last record is in, until none lands: a Sound that fits whole loads whole (`a_patch_that_fits_whole_round_trips_whatever_its_order`, and the project and library loads). What still doesn't fit stays the base's (W1 for a kind), and the load says FORMANT BUDGET FULL.
- **WAVE judges the patch it would leave.** In that same patch op 6's FRMT can't step to any kind: without it the five NOIS are over the four-voice floor, so every target is refused and the cell toasts, though the edit removes cost. With one NOIS fewer every target fits and every step lands (`a_frmt_steps_to_every_kind_the_whole_patch_fits`).

### Voices
The costliest patch the budget admits (`frmt_max`, every kind mix with a FRMT on every pair, LEVEL 99, FDBK 1) is **T1 ∪ A16**: a linked FRMT on operator 2, the unlinked one on 6, NOIS on 1 (linked) and 4, tables on 3 and 5 (cost 1,472, spend 650 of 667; five other pairs with A16 tie it). Before the reach was billed it was a linked FRMT on 3 and NOIS on 1, 2 and 4 (1,511), now over budget. It is the bench's FRMT MAX row. Printed by `formant_costs_bill_by_kind` (light: `ModRouting::BASE` + `CHAIN_COST` beside the whole bus, capped at the pool's 8; heavy: `HEAVY_OVERHEAD` beside the bus with the tape; a formant Sound's counts include recost's reach):

| Sound | Engine cost | Rev V, light | Rev Y, light (with the tape) | Rev V, heavy |
|---|---|---|---|---|
| BREATH | 530 | 8 | 8 (7) | 5 |
| CHOIR | 1,230 | 4 | 3 (3) | 3 |
| TALK BASS | 1,210 | 4 | 3 (3) | 3 |
| FRMT MAX | 1,472 | 3 | 3 (3) | 3 |

The floor is rev V's, as ADR 0026's is; on rev Y with the tape the costliest formant patch gets three under light routes (two before the lower budget moved it). ADR 0031's "the costliest patch gets six voices on rev V" still holds for table patches (`the_costliest_table_patch_still_gets_six_voices_on_rev_v`); it never applied to formant Sounds.

### Fixed work outside the voice budget
The audio interrupt's per-block publishes are fixed work, not billed to voices; the 25 % reserve beside the 75 % share carries them. Counted on the shipping ELF (functions forced out of line for the count only):
- **Formant view** (`Instrument::formant_views_into` and its triple-buffer swap): 107 instructions static, its voice loop 51 a voice at the most: about 460 instructions, 11.5 cycles a sample.
- **Scope** (`ScopeWriter::write`): 64 samples copied, at most 37 instructions per four, and every 7.5 blocks a trigger search (up to 240 × 4) and a 960 B publish: about 650 instructions a block (16 cycles a sample) and 2,650 in the publishing one (66): 23 on average.
- **ORBIT's view** (`Runner::publish`, ORBIT builds): 171 instructions static, two 64-bit divides and, stopped, a chord build: about 10 cycles a sample, estimated (calls in its loop).
- **The TAPE heads** (`publish_heads`): 18 instructions in the shipping build, where TAPE is off.

### Laws recorded here
- **Held laws.** A formant's per-block law is kept while its inputs hold (`LawKey`): every played FRMT, BW, LEVEL, FINE, DETUNE and FDBK, VOWEL, MORPH, pitch, slide, rate, note, TRNSP and the COARSEs. The latched slots, KEY, SKIRT, the kinds the engine runs and its plan change only at a note-on, a kind swap or a plan change, and each drops the key where it writes. So a WAVE spin or an ALG edit under a held note retakes the law, bit for bit the law taken afresh every block (`held_laws_follow_kind_swaps_and_alg_edits`).
- **A swapped-in kind starts closed.** At the duck's end (`finish_swap`) a new FRMT's carrier ratio, PM share and peak, and a new NOIS's band coefficients, are seeded from its new law, as an idle operator's are at a note-on (`a_swapped_in_kind_starts_from_its_new_law`). The laws are taken there, so the duck block takes them twice and the next block holds them: inside the documented duck overrun.
- **The grain delay is divide-free:** `o = rest + h·g`, `h` half the slack, `g = f(pm/(3h) + β) − β` with `f(v) = v(3 − v²)/2` on v held to [−1, 1]. It stays inside the slack, C¹ onto both ends, slope ½ at rest (`(1 − v²)/2` ≤ ½); the reset grain (β = −1) opens at once and only delays. Worst modulated alias case −66.7 dB (`158 A3 99`, SKIRT 0); odd mean 2.58e-5 (bound 1e-4).
- **PM depth cap:** the links' share `k = min(clamp((24 kHz − FC − m_q·B_min)/reach, 0, 1), DELAY_SLEW·f0/dev)`, `DELAY_SLEW` 4, ramped per sample into the delay and the carrier. It holds ratio-1 PM into a formant to about 0.64 cycles (LEVEL 76 and up sound alike); 8 would allow 1.27 with 3.7 dB less alias margin.
- **NOIS links are outside the slew bound.** Noise is left out of `dev`, so a full-level NOIS link can move the delay faster than `DELAY_SLEW`. Its sidebands are noise, so no tonal alias.
- **p̂ under drive:** feedback and PM displace the carrier against its window, so both gains reference `driven_peak = min(1, p̂ + lanes·2π·φ)`, continuous in the drive. The peak stays ≤ 1 as a modulator and as a carrier; the level cost is up to −13.7 dB at BW 10 kHz. FDBK 5's modulator swing spreads 16.92 dB across BW, pinned below 17 dB as a regression bound (`algo_formant_test`).
- **The PM depth cap and p̂ together:** the cap bounds how far the links move the delay (`DELAY_SLEW` 4 periods a period); p̂ bounds the peak the drive leaves. Neither alone keeps a modulated formant both alias-free and ≤ 1.
- **Raw DC under FDBK:** −2.28e-3 before the voice's 5 Hz blocker (7.3e-6 after), in `formant_operators_are_stable`'s case: FDBK skews each grain, so the engine's raw output isn't DC-free, and the voice's blocker (ADR 0060) is what removes it.
- **`VOICE_CHAIN_BYTES` 2,112 → 2,176:** the formant operators' 13 glided settings in `AlgoLive`. `AlgoEngine` is 2,572 B and still fits inside `ModalEngine`'s 4,240, so `EngineSlot` is unchanged; `Voice` is 6,416 B (6,304 on main at 4473f0a1), pinned by `the_algo_engine_still_fits_its_slot`.

### Partial supersessions
- **ADR 0022:** its destination list gains FRMT, BW (per operator) and VOWEL (spec § 7).
- **ADR 0026:** its cost terms gain the formant and noise kinds, billed per algorithm; its four-voice floor now holds for Sounds of tables and noise bands, and **three** for a Sound holding a FRMT (the owner's decision, 2026-10-12), both under the reference heavy routing.
- **ADR 0031:** "the costliest patch gets six voices on rev V" still holds for table patches; a formant Sound gets three or more.

## Alternatives considered
- **Bill every FRMT linked** (`Kinds` blind to the algorithm): N = 1 at three voices, and no three-formant vowel.
- **Bill the moving term only under routes and glides** (ADR 0117-style): linked N = 2, but the pitch wheel and glide steals move laws with no route to see.
- **Cut 11 more cycles from the linked path:** linked spans cost about 5.5 KB of flash.
- **Four voices for every Sound:** 197 holds one unlinked FRMT.
- **One FRMT for alpha:** no vowels.

## Consequences
- Up to three formant carriers per Sound, or one formant a modulator reaches; mixes in between by the rule.
- An ALG A or B edit can be refused, which a table-only Sound never sees.
- The bench-algo build has a FORMANT screen after ALGO's: FRMT0 (one FRMT, SKIRT 0, BW 0), FRMT3 (SKIRT 3), FRMT PM (FRMT3 under a full ratio-1 sine, T1's 2 → 1), NOIS, FRMT MAX (above) and CHOIR (the demo Sound with its VOWEL route), each at 1..8 voices from C2 up in tenths (to A♯7, inside the terms' C8). The chip bench reads them at the ship flash (Task 17); a reading over its term updates the term, and N follows.

## Sources
- `docs/superpowers/specs/2026-09-30-formant-operators-design.md` §§ 1, 3, 9.
- US5610354 (expired); FSVR's prose docs (GPL-3, behaviour only, ADR 0032); Peterson & Barney (1952).
- ADR 0026, 0031, 0045, 0056, 0087, 0112, 0117, 0124; #243, #437.
