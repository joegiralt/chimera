# HARMONIC engine (0.4): design

Status: approved by the owner, 2026-11-12 ("sounds good"); firmware work gated on DRUM landing under the bank-1 budget.

Each point is marked as the **owner's** decision (binding, 2026-11-12) or as **Claude's proposal** (open to the owner's override).

## Intent
HARMONIC is an additive engine: a bank of sine partials whose strengths come from a curated map of spectra, thinned by a Grids-style DENSITY, then shaped by a short chain of spectral modules. It is the engine Chimera lacks: ALGO is phase modulation, MODAL is resonators, VA is subtractive, TAPE is samples, DRUM is machines. None of them lets the player move through *spectra* directly, or hear one partial at a time.

Why this shape: the survey's strongest result is that a small set of well-chosen spectra spans a lot of timbre (Horner, Sandell; survey §3.4), and that interpolating stored **amplitudes** of index-aligned harmonics avoids the classic morphing faults, which come from interpolating frequencies (Momeni and Wessel; survey §3.2). The map gives the player a place to start that already sounds good; the modules give knobs whose effect is visible and predictable (product vision: the UI shows what each part does to the sound).

## Goals, non-goals, anti-goals
**Goals**
- A playable X/Y field of 25 spectra that glides smoothly, with no dead zones, neighbours that sound related, and level held steady across the field.
- DENSITY from a few strong partials up to the full spectrum, never "tinkly" (survey §3.3).
- Three module slots that each do one audible thing, with the spectrum drawn before and after.
- Alias-free at every note, glide and bend: Plaits' Nyquist taper (survey §1.3).
- Fits 5 voices beside other Parts, inside the engine slot that exists, inside the flash the owner allows.

**Non-goals (0.4)**
- Resynthesis of user samples, per-partial envelopes on every partial, more than 64 partials, an IFFT path (survey §1.4–1.5: past break-even only above ~100 partials).
- SWARM and PARTICLE (stay host prototypes).
- Masking-based partial pruning (survey §4.1: does not pay for a single harmonic source).

**Anti-goals**
- Opaque macro snapshots under the pad (Loom II's reviewed weakness, survey §3.11).
- Any Grids code or data (GPLv3). Any third-party sample data in the map.
- Interpolating frequencies in the map. Hard-gating partials in or out.
- VARIETY touching pitch.

## Owner's decisions (2026-11-12, binding)
1. **Own engine** beside ALGO, MODAL, VA, TAPE and DRUM. Ships in 0.4 only if the DRUM slices land under the bank-1 budget.
2. **Concept: "both"** — Grids for partials (a map plus a density threshold) and a Loom II-style module chain.
3. **Voices: at most 5** per HARMONIC Part.
4. **Partial count** settled by three limits: CPU measured on the unit, Nyquist per note, and the ear (an audition of 32/48/64). Recursive oscillators per partial with Plaits' Nyquist taper. Target 48, or 64 if the bench allows; always a multiple of 8.
5. **Map: option B.** A curated 5×5 field of spectra stored as 8-bit log amplitudes only (~1.6 KB at 64). X and Y glide bilinearly between the four nearest nodes. Frequencies are never interpolated. Grids is GPLv3: idea only.
6. **DENSITY** is a fixed control: a Grids-style threshold on partial strength, from sparse strong partials up to the full spectrum.
7. **AIR** (harmonic-plus-noise breath) is a fixed control.
8. **Module chain:** 3 slots, each one of TILT, ODD/EVEN, FORMANT, STRETCH, COMB, PLUCK (survey §(b)), each with its own knob(s).
9. **After the chain:** the usual voice chain (envelopes, filter, OUT, FX slot) and VARIETY like every engine, never on pitch.
10. **Type-driven development is mandatory.**
11. **Flash is the gate.** Bank 1 has 27.2 KB free after DRUM slice 1, with the rest of 0.4 to come; a flash-size investigation is under way.
12. **RAM:** D2 is full, AXI ~100 B above its margin; new static state goes in ITCM (63 KB free; `.itcm` exists on the drum branch). Per-voice state must fit the engine slot.
13. **CPU:** ~7,500 cycles a sample for all voices plus the bus; billing per ADR 0262.

## Approaches considered
- **A. Loom corners.** Each X/Y corner stores a snapshot of module settings; the pad blends settings. Rejected: the corners are opaque (survey §3.11), and one pad position moving every module makes the module knobs untrustworthy (survey §(c)). It also needs the chain to be expressive enough to reach every corner timbre, which three slots are not.
- **B. A field of spectra (chosen, owner).** The map supplies the source spectrum; modules shape it on their own knobs. Precedents: Haken's corner-weighted spectra in Kyma/Continuum, the K5000's MORF, Grids' mechanism (survey §3.3, §2.2, §3.5). Cheap: 25 × P bytes of flash, a bilinear mix of four nodes a block. Index-aligned amplitudes have no correspondence or glissando problem.
- **C. Inverse FFT.** Rejected for 0.4: break-even ~20 partials and only ~2× faster at 100 on a desktop (survey §1.4–1.5); a frame of latency smears plucks. Kept as the documented route past 100 partials.

## The voice (Claude's proposal for the order; the blocks are the owner's)
```
MAP (X, Y) ─ DENSITY ─ SLOT 1 ─ SLOT 2 ─ SLOT 3 ─ AIR ─ TAPER ─ LEVEL ─ BANK ─┐
   per block (64 samples), per partial, amplitudes in log domain          │ per sample
                                                                          ▼
                                FILTER ─ FX slot ─ AMP (VCA/LPG) ─ PAN ─ OUT / Part bus
```
- **Control rate** (once a 64-sample block, per voice): read four nodes, mix, threshold, run the slots, add AIR's weights, taper at this note's partial frequencies, normalise level. Output: a target amplitude (and, with STRETCH, a frequency) per partial.
- **Audio rate:** one recursive quadrature oscillator a partial (Vicanek's, 3 FMAs, no renormalisation; survey §1.1), amplitudes ramped linearly across the block (Plaits; survey §1.3), loop ordered partial-outer, sample-inner so the oscillator state sits in registers (survey §(a)).
- **DENSITY before the slots** (Claude's proposal): the threshold selects partials by the map's strength, so a FORMANT or COMB sweep afterwards reshapes the chosen partials without switching any in or out.
- Amplitude modules are multiplicative masks, so their order does not change the sound; only STRETCH (frequencies) and PLUCK (time) are not masks, and neither depends on order either. The slots are a menu, not a signal-flow puzzle. Duplicates are allowed (two FORMANTs give two vowel regions) (Claude's proposal).

## Type layer (owner's rule; names as the owner listed, details Claude's proposal)
| Type | What it encodes |
|---|---|
| `PARTIALS: usize` + `const` assert | the build's count; `% 8 == 0 && <= 64` checked at compile time |
| `PartialIdx(u8)` | 0..PARTIALS, made only by `PartialIdx::all()`; `.harmonic()` = idx + 1. No bare index into a partial array |
| `Spectrum<T>([T; PARTIALS])` | per-partial arrays, indexed only by `PartialIdx` |
| `LogAmp(u8)` | a stored strength: 0 silent, else (n − 255) × 0.375 dB (−95.6..0 dB) |
| `NodeSpectrum(Spectrum<LogAmp>)` | one map node; `const fn` validation: some partial non-zero, peak at 255 |
| `NodeIx` (enum N0..N4), `MapNode { x, y }` | a node's place: no out-of-range node exists |
| `MapPos { x: Unit, y: Unit }` → `(MapCell, Frac, Frac)` | the knob position and its split into the 2×2 cell and the bilinear weights |
| `Density(Unit)`, `Air(Unit)` | the fixed controls |
| `ModuleKind` (NONE, TILT, ODD/EVEN, FORMANT, STRETCH, COMB, PLUCK) | `DiskCode`; its knob labels and count from one `const` table |
| `ModuleSlot` (S1, S2, S3) | slot index; `ModuleChain([ModuleCells; 3])` |
| `Module` enum with fields (`Tilt { slope, pivot }`, `Comb { spacing, offset, depth }`…) | built from a slot's cells at block rate; a knob belongs to its kind |
| `PartialHz`/`NormHz` | per-partial frequency before the taper; the taper takes only `NormHz` |
| `HarmonicVoices` (`VoiceCap::new(5)`) | the allocator's cap, a const |
| `HarmonicCost` terms (`Cost`) | bills per ADR 0262 |

Cells stay generic in storage (KIND + three `u16` knobs a slot) because the param table, MIDI maps, lerps and the codec need uniform cells; `Module` is the typed view the DSP and the UI read. A slot's unused knob cell dims ("FORMANT HAS NO …"), as DRUM's MIX does.

## The controls
**HRM (home) cells:** X · Y · DENSITY · AIR · VARIETY · (empty).

- **X, Y** (owner): bilinear between the four nearest nodes, in the log (byte) domain: the mix is in dB, the compressed domain the survey advises (§3.10). A host sweep checks the centroid moves evenly along X.
- **DENSITY** (owner's law; numbers Claude's): with the blend's strongest partial at `peak`, a partial plays at full strength when its level ≥ `peak − R`, where R runs 0 → 96 dB over the knob (DENSITY 0: the strongest partial or two; 1: everything). A 6 dB soft knee below the threshold fades a partial in dB, and amplitudes still ramp per block: nothing pops in.
- **AIR** (owner's control; law Claude's): bandwidth-enhanced partials (Fitz and Haken; survey §4.5). Each partial's amplitude is multiplied by `1 + AIR · w(k) · n_j(t)`, `w` rising with partial frequency, `n_j` one of four low-passed noise streams rotated across partials (one shared stream would sound like tremolo). On low notes a noise band above the last partial, shaped by the spectrum's top slope, fills the air the bank cannot reach. Breath that follows the spectrum, no filter bank.
- **VARIETY** (owner: every engine; set Claude's): X, Y, DENSITY, AIR and each slot's knobs except STRETCH's, latched per note. STRETCH moves partial frequencies, not the fundamental, but is kept out to honour "never pitch" (open question 2).

## Modules (laws: Claude's proposal; all at block rate)
Slot page cells: KIND · A · B · C · (empty) · (empty). Header `SLOT 1 • FORMANT`.

| Kind | Knobs | Law |
|---|---|---|
| TILT | SLOPE · PIVOT | gain(k) = (fₖ / f_pivot)^s, s from −12 to +6 dB/oct on a curved knob law (survey §2.9); PIVOT the partial left untouched, so tilting keeps the level near |
| ODD/EVEN | BALANCE | even partials × (1 − b) for b > 0 (hollow, clarinet), odd ones above the 1st × (1 + b) for b < 0 (octave-ish); the fundamental never moves (survey §2.7) |
| FORMANT | FREQ · WIDTH · PEAKS | 1–3 Gaussian bumps in log-frequency at FREQ, FREQ × 1.6, × 2.6 (Hz, fixed per note: vowels, not tracking), WIDTH their σ; gain = floor + bumps, floor −30 dB. Plaits' three-knob design in Hz (survey §2.4) |
| STRETCH | AMOUNT · KEY | fₖ = k·f₀·√(1 + B·k²), B signed (negative compresses, clamped monotonic), the stiff-string law of MODAL STRUCTURE reused, not copied; KEY scales B down for low notes (survey §2.10). The only module that changes frequencies |
| COMB | SPACING · OFFSET · DEPTH | gain(k) = 1 − DEPTH · (½ + ½ cos 2π(k / SPACING + OFFSET)); SPACING 2–16 partials, OFFSET sweeps (the partial-domain phaser; survey §2.11) |
| PLUCK | DECAY · SLOPE | each partial decays from the note-on with T60(k) = DECAY · (fₖ / f₀)^(−SLOPE): high partials die first (survey §5.1); DECAY at max is off. Two PLUCK slots multiply into one per-partial decay |

## Pages and chain map (Claude's proposal, VA's style)
```
HRM ── FLT ── AMP ── MOD
 └ MAP    └ FMD FX JACK   └ SENDS   └ MTX E1-E3 SPD L1-L3
   SLOT 1
   SLOT 2
   SLOT 3
   PIT
```
| Page | Cells | Band |
|---|---|---|
| HRM | X · Y · DENSITY · AIR · VARIETY · — | the spectrum glyph |
| MAP | X · Y · DENSITY · — · — · — | the 5×5 field: each node a 8-bar thumbnail of its spectrum; rows labelled BELL ORGAN REED BRASS VOICE, DARK → BRIGHT along X; the cursor at (X, Y) with its four nodes outlined |
| SLOT 1–3 | KIND · A · B · C · — · — | the spectrum before the slot (outline) and after it (filled) |
| PIT | as every engine | |

D-pad as everywhere: MINUS/PLUS along HRM · FLT · AMP · MOD, EDIT/SEQ through MAP, SLOT 1–3, PIT. No hidden gestures are added; if a slot-copy shortcut is ever added it gets a focus-band hint.

**The focus-band glyph** (Claude's proposal): a drawn spectrum. One bar a partial (64 bars at 2 px + 1 px gap, 48 at 3 + 1: 192 px either way), height in dB over 48 dB. Bars passing DENSITY in the accent, the rest dim; DENSITY's threshold a dashed line; partials the taper removes at the last note played hatched; STRETCH moves the bars along x by their real ratio; AIR a stipple under the bars. The UI computes it with the same pure function the voice runs (map → DENSITY → slots) at a reference note, so no voice-to-UI buffer is needed.

## The 25 spectra (Claude's proposal)
**Layout.** Y is family, ordered so neighbouring rows share a trait; X is brightness, each row's five nodes stepping the centroid evenly (survey §3.8: even coverage, close neighbours, no pitch).

| Y row | Family | Shared trait with the next row |
|---|---|---|
| 0 | BELL / struck: few strong partials, some high ones prominent (amplitudes only; with STRETCH they become bells and bars) | sparse, discrete |
| 1 | ORGAN: drawbar registrations, flute to full organ | sparse → hollow |
| 2 | REED: clarinet (odd), oboe, bassoon, harmonica | odd/even, nasal formant |
| 3 | BRASS / BOWED: saw-like with rising brass formant, string body bumps | full, formant-shaped |
| 4 | VOICE: vowels U O A E I left to right (F2 rises, which is brightness) | — |

**How they are made: by hand with formulas, plus analysis of clean sources only.**
- Formulas: ideal series (saw 1/k, square odd 1/k, triangle 1/k²), drawbar registrations (organ practice, not data), odd/even weightings, formant envelopes from published vowel formant averages (measured facts, cited in the node's provenance line), body-resonance bumps.
- Analysis (optional, for nodes that sound thin by formula): renders of Chimera's own engines (ALGO, MODAL) and recordings of the owner's own synths, analysed to harmonic amplitudes. No third-party sample data, no Grids tables, no commercial preset data.
- A generator in chimera-utils (`just harmonic-map`) turns a text spec (formula and parameters a node) into the committed `const` table; a test checks the table matches the spec. Each node carries a one-line provenance.

## Estimates
**Flash** (Claude's estimate, 64 partials; counted against bank 1 as the owner's gate is):

| Part | KB |
|---|---|
| Partial bank: recursive oscillator, taper, block ramps | 1.5 |
| Map table 25 × 64 B (1.2 at 48) | 1.6 |
| Map read: bilinear, log → linear | 0.5 |
| DENSITY, level normalisation | 0.4 |
| AIR | 0.7 |
| Six modules (TILT 0.3, ODD/EVEN 0.2, FORMANT 0.8, STRETCH 0.6 reusing MODAL's law, COMB 0.4, PLUCK 0.5) | 2.8 |
| Engine plumbing: `EngineType`, slot variant, params block and table, codec record, cost terms, VARIETY set | 3.0 |
| Pages: HRM, MAP field, SLOT pages, glyph, labels | 4.0 |
| **Total** | **~14.5 (plan for 19)** |

Against 27.2 KB free before DRUM's later slices, it fits only if those slices and the other 0.4 slices take under ~8 KB.

**Cut first, in order** (least sound lost per byte): (1) store 48 partials if the ear chose 48 anyway, −0.4; (2) MAP's field as a dot grid with no per-node thumbnails, −1.0; (3) COMB, −0.6 (weakest support, survey §2.11); (4) AIR, −0.8 with its cell; (5) a 4×4 map, −0.6 (3×3: −1.0); (6) 2 slots, −0.5. About 4 KB in all. Past that, HARMONIC waits for bank 2 (#460) rather than shipping thin.

**RAM** (Claude's estimate):
- **Per voice, in `EngineSlot`:** per partial u, v (state), k₁, k₂ (coefficients), amplitude = 20 B → 1,280 B at 64 (960 at 48); PLUCK's decay gains 256 B; AIR's four noise streams and filters ~48 B; the played `HarmonicParams` ~32 B (see below); misc ~64 B. **~1.7 KB at 64, ~1.3 KB at 48, against `ModalEngine`'s 4,240 B: it fits, and the slot does not grow.**
- **The voice's played Sound:** today the voice copies each engine's block (`VOICE_CHAIN_BYTES` 2,464, in D2, which is full). Adding `HarmonicParams` there costs ~32 B × 8 voices of D2 that does not exist. Fix (proposal): the HARMONIC slot holds its own played params, inside the slot's spare 2.5 KB, so `VOICE_CHAIN_BYTES` stays put.
- **`ParamSnapshot`** holds every engine's block side by side, so it grows ~32 B for each copy (Parts' live Sounds and any edit copies). AXI has no room. Fix (proposal): a per-Part `HarmonicParams` side table in ITCM (~200–400 B), the snapshot holding nothing new; the builder counts the copies and measures `size_of::<ParamSnapshot>()` first. If a union of engine blocks is cheaper overall, that is a separate refactor and its own ADR.
- **Map:** `const` in flash, no RAM. **Glyph:** computed by the UI, no RAM. **Stack:** the block's scratch (amplitudes and frequencies, 2 × 256 B) under ADR 0025's check; if the check fails, one static scratch in ITCM (audio thread only).

**CPU** (Claude's estimate; the per-partial figure is the survey's 6–9, planned at 8, **to be measured on the unit**):

| Per voice, per sample | 48 | 64 |
|---|---|---|
| Bank: P × 8 | 384 | 512 |
| Control rate: ~40 cycles a partial a 64-sample block (map, DENSITY, slots, taper, k₁/k₂ on pitch moves) | 30 | 40 |
| AIR | 30 | 30 |
| Voice chain (filter, ENVs, AMP, FX slot), billed by its own terms | ~300 | ~300 |
| **Voice** | **~745** | **~880** |
| **5 voices** | **~3,725** | **~4,400** |

7,500 less the bus (~1,190 at STRESS's worst, ADR 0262) leaves ~6,300 for voices. A full 5-voice HARMONIC chord takes ~59 % of it at 48 (≈ 2,575 left, about three VA voices for other Parts) or ~70 % at 64 (≈ 1,900 left, about two).

**Billing** (ADR 0262: bills cover the worst marginal block): `HarmonicEngine::cost(p)` = base + PARTIALS × `COST_PARTIAL` + `COST_AIR` when AIR is set or routed (as VA's `noise_billed`) + a term per module kind present (STRETCH and PLUCK per partial). The worst block is the lowest note with every partial under the taper live and STRETCH re-solving k each block. The taper's savings on high notes and DENSITY's on sparse spectra are headroom, never billed down (survey §4.1). The 5-voice cap is the allocator's, independent of the bill: a Part plays min(5, what the bill admits). The bench gains HARMONIC rows at 1–5 voices for `COST_PARTIAL` and each term.

## Integration points (skimmed; for the builder)
- `chimera-engine/src/params.rs`: `EngineType::Harmonic` (next disk code after DRUM's), `ParamSnapshot` (see RAM), `for_engine`, a `BlockRef::Harmonic` param table (ranges, curves, labels, lerps) and a new card record (ADR 0045).
- `chimera-engine/src/dsp/engines.rs`: `EngineSlot::Harmonic(HarmonicEngine) => rebuild_harmonic, init_harmonic`, a `SlotKind`, `model_cost` and `cost` arms, a compile-time assert that `size_of::<HarmonicEngine>() <= size_of::<ModalEngine>()`.
- `chimera-core/src/ui/block_registry.rs`: `HRM_HOME`, `HRM_MAP`, `HRM_SLOT1..3` `BlockDef`s (free ids), `HRM_SUB_PAGES`, `HARMONIC_BLOCKS` and `HARMONIC_CHAIN` on VA's pattern (`VA_BLOCKS`, `VA_CHAIN`), registered in the chain list; a `VizType::Spectrum` and a `VizType::SpectrumField`.
- The allocator's per-engine voice cap; VARIETY's per-engine set; `just engine-maps` gains a HARMONIC map (flat maps, ADR 0266); INIT and a few factory Sounds on the card.
- Bank placement per ADR 0251, every budget line naming the HARMONIC ADR.

## Provenance
Plaits (MIT): structure and the taper, read and A/B'd by ear, no code or tables copied. Grids (GPLv3): the map-plus-threshold idea only. Vicanek's oscillator: a published algorithm. Harmor and the K5000: product behaviour from manuals and reviews. The map: formulas and clean analyses as above.

## Host prototype and audition (Claude's proposal)
1. Grow the prototype into `chimera-utils/src/harmonic/`: the generic recursive bank (Vicanek, with the magic circle as the cheaper candidate), the map, DENSITY, AIR and the six modules, each a pure function over `Spectrum`.
2. Tests on the host: no energy above 0.46 fs at any note or glide (the prototype's FFT test, kept); no block-to-block amplitude step above a bound at DENSITY sweeps; level within ±1.5 dB RMS across the field; the centroid monotonic and roughly even along X in every row.
3. **Audition A — partial count:** 32/48/64 on the listening stimuli (bass hit, chord stab, short and long sine, short texture), at the same map points, for the owner's ear.
4. **Unit bench:** one kernel row, cycles a partial-sample at 48 and 64, at 1–5 voices. With A, this sets PARTIALS.
5. **Audition B — the map:** X and Y sweeps per row, then the 25 nodes one by one, on an audition page with marks (DRUM's precedent). Nodes the owner marks weak are redrawn.
6. **Audition C — DENSITY, AIR and each module:** sweeps on short musical phrases.

## Order of work
0. Gate: DRUM slices merged and the flash investigation's bank-1 figure known.
1. Host prototype and audition A, unit bench (sets PARTIALS).
2. Map generator, 25 nodes, audition B.
3. DENSITY, AIR, modules on the host, audition C.
4. ADR: algorithm and provenance, map format, voice cap, RAM placement.
5. Typed port into chimera-engine: types, params, slot, bills, goldens.
6. Pages, glyph, MAP field; desktop QA (param sweeps, levels); annotated guide pages.
7. Factory Sounds and the engine map; flash, unit end-to-end.

## Left to the builder
Exact cost constants from the bench; magic circle against Vicanek; whether a Chebyshev fast path (the prototype's phasor plus recurrence, ~3.5 cycles a partial) is worth its flash when no STRETCH slot is set, billed by model; the k₁ tan approximation; the noise streams' filter; where the ParamSnapshot growth lands after counting copies; bank placement of the pages.

## Open questions for the owner
1. DENSITY: hold loudness constant as partials drop out (renormalise), or let a sparse spectrum be quieter?
2. VARIETY on STRETCH: allowed (it never moves the fundamental), or kept out as "pitch"?
3. Is the 5-voice cap per Part, or for all HARMONIC voices on the instrument together?
4. Map sources: formulas only, or also analyses of your own synths' recordings?
5. Duplicate module kinds across slots (two FORMANTs): allowed, or one of each?

## Builder check, 2026-11-12 (answers and Claude's calls)
- **Bank:** bank 1. ORBIT's bank 2 has 128 B free (drum-slice-1, ac04eb3e); ADR 0251's "21.3 KB after cuts" is stale. Audio-rate code runs from bank 1 (zero-wait on the H750's AXI-flash path as the other engines' bank-1 code); the cycle bench measures it there.
- **Prerequisites (Claude's call):** VARIETY and the flat engine maps (ADR 0266) land before HARMONIC; their flash counts in the same "rest of 0.4" figure. Step 0 of the order of work names both.
- **Voice cap (Claude's proposal, owner to confirm):** a per-Part cap of 5 HARMONIC voices; the CPU budget is protected by billing (ADR 0262 refuses or steals a voice whose bill doesn't fit), not by the cap. Two HARMONIC Parts can't exceed the budget; they share it. An instrument-wide cap would need cross-Part allocator state and its own ADR, so it is not proposed.
- **Flash floor:** open, for the owner, after the DRUM flash-size investigation reports (owner approved bank 1 to ~20 KB free for DRUM; the full 0.4 list does not fit above that floor at today's cost).

## Owner, 2026-11-12: "take your recommendations, but follow the main features of Loom II"
Open questions 2–6 settled as Claude recommended: the 5-voice cap is **per Part** (billing protects the CPU budget); DENSITY **keeps loudness steady** (a timbre control, not a volume control); VARIETY **leaves STRETCH alone** (it counts as pitch); map sources are **formulas now, plus analyses of the owner's own synth recordings later** (licence-clean); two slots **may hold the same module**. The flash floor stays open (it rides on the size-optimisation ADR).

### Loom II's main features and HARMONIC's answer to each (Claude's proposal)
| Loom II | HARMONIC |
|---|---|
| Module chain: up to 10 interacting modules from 34 | **4 slots** (was 3) from the six (TILT, ODD/EVEN, FORMANT, STRETCH, COMB, PLUCK); duplicates allowed. Flash and CPU cap the count, not the idea. |
| Morph Pad: up to 4 variations of the sound, one per quadrant, morphed by dragging | **Two layers moved by the same X/Y:** the spectra field (B) underneath, and **4 corner variations of the module knobs** stored per Sound on top (Loom's pad, ~16 B a Sound). X/Y glides the spectrum and the modules together. Corners are set with an EDIT-on-a-corner gesture on the MAP page (shown in the focus band, per hidden-gestures-need-hints). |
| Morph path drawn on the pad, free or synced to 1, 4… bars | **PATH:** a shape on MAP (OFF, LINE, CIRCLE, FIGURE-8, ZIGZAG) with RATE free or synced to ORBIT's tempo (1/4 bar … 8 bars) and a phase; sums with X/Y and any mod routes. |
| X-axis envelope mode (rhythmic morphs) | **ENV → X** as a one-cell shortcut on MAP (the mod matrix can already route it; the cell makes it Loom's one move). |
| Spectral noise section | **AIR** (fixed control): noise shaped by the same spectrum, so breath follows the map. |
| Subharmonic partials | **SUB:** a fixed control adding partials at ½ and ¼ of the fundamental (2 extra oscillators), taper-safe. |
| 512 partials, 8 voices | 48 or 64 partials, 5 voices a Part: the hardware's limit, set by the bench and the ear. |

Cost added by this section (estimate): corners + PATH + SUB ≈ +1.5 KB flash, +16 B a Sound, +2 partials of CPU a voice. The cut order gains, first: PATH shapes beyond LINE/CIRCLE, then SUB, then the fourth slot.

## Owner, 2026-11-12: bright from C3 up (supersedes the partial count and SUB above)
- **Partials: up to 160**, the count that reaches the top of the audio range from **C3 up** (C3 needs 152). Below C3 the same 160 play, so low notes top out lower (gracefully darker; no range cutoff). Owner's reasoning: Loom-like brightness where melodies and chords live, at a CPU this hardware can afford.
- **SUB: VA's variable-wave oscillator, reused** (owner: "steal one of the oscillators from VA"): WAVE **SINE, TRIANGLE or SQUARE**, OCTAVE −1 or −2, LEVEL. Replaces the ½/¼ sub-partials.
- **Voices: 4 per Part** (was 5): the C3 worst case is ~1,000–1,300 cycles a voice (estimate; the unit bench decides). Pitch-aware billing (a note above C3 costs less) is a later option needing its own ADR.
- The bright tail and log spacing (group 9 renders) and the hybrid inverse-FFT stay alternatives, not the plan.

## Tried and scratched, owner 2026-11-12
- **Two-family partial-sliding morph** (pick families A and B; MORPH slides each partial's pitch and level to its partner, by strength or index): prototyped in the playground. Owner: "it sounds like a weird resonance slide from one timbre to another, scratch my idea." The map keeps amplitude-only glides between neighbouring families (as the survey advised); the oscillator bank stays harmonic-only (stretched families via STRETCH).
- Still to choose by ear: group 9 (48–64 partials + bright tail, 5 voices) vs group 10 (160 partials + VA SUB, ≤3 voices).
