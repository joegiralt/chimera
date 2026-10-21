# 0204. TAPE STRETCH has two TYPEs: CLASSIC and GRAIN

- **Status:** Proposed (supersedes in part 0093, 0103 and 0181 for STRETCH)
- **Deciders:** owner (2026-10-21: "like a bell… stretch it out but keep the pitch the same… up to 8 seconds", in grains, the proposal in (private notes) approved; then: keep today's STRETCH beside it as a TYPE, CLASSIC and GRAIN, each billed its own), firmware

## Context
Today's STRETCH is the Akai S950's two-grain splice (0093, 0103): a time pointer at MOTION's speed, two grains, MONO period locking or POLY. To stretch a bell to a length, SPEED has to be worked out by hand for each source; at the window's end the pointer wraps and the bell strikes again; on the TX bell at C5 it sags −8, −14 and −17 cents at 2, 4 and 8 s, and on PAD CHORD at 6 s it is +71 cents sharp. The owner wants a key to play the source at its pitch and last a set number of seconds, up to 8, in grains, and to keep today's STRETCH as well: the two cost about the same (450 against 570, 1.27×).

Bank 1 is frozen (0198): the DSP goes behind a door, bank 1 gains only the closed door's data.

## Decision
### TYPE: CLASSIC and GRAIN
- **TYPE** (param 10, STRETCH's, `StretchType`) holds CLASSIC (code 0, MONO, shown CLASSIC), CLASSIC POLY (code 1, POLY, shown CLS POLY) and GRAIN (code 2, appended). A Sound from before GRAIN holds code 0 or 1, or no TYPE at all (neutral 0): it loads CLASSIC. TYPE latches at note-on.
- **CLASSIC** is today's STRETCH, its file (`stretch/classic.rs`) moved, not changed: its notes render bit for bit as before (`tape_stretch` and `tape_stretch_reverse` in `golden_test` are the hashes recorded at origin/tape-streaming, unchanged), it takes no draw of the voice's, and it bills 450. It reads SPEED (MOTION), D-TIME (SIZE) and POSITION, and TYPE's MONO or POLY.
- **GRAIN** (`stretch/grain.rs`, below) reads SPEED, LENGTH, GRAIN, SPRAY and END, and bills 570 (`COST_STRETCH_GRAIN`). A voice's bill and its held cost follow its TYPE (`TapeDeck::model_cost`, through the door).
- **Cells:** DECK TYPE · SPEED · LENGTH · D-TIME · POSITION; the STRCH page TAPE · AGE · WOW · GRAIN · SPRAY · END. Each TYPE's cells dim on the other, and the focus band says why: GRAIN TYPE ONLY, CLASSIC TYPE ONLY (`TapeDeck::stretch_only`, bank 2's: with the door closed nothing dims). A MODEL turn to STRETCH lands on GRAIN, with ONE's ENV 1 (FAST, ATTACK 0, SUSTAIN 1, ADR 0180: the source is its own envelope; today's 2.4 dB under ONE was INIT's SUSTAIN 0.7). CLASSIC's TYPE cell no longer reads POLY (UNPITCHED) on an unpitched source: TYPE must stay a choice to reach GRAIN, and CLASSIC MONO there plays POLY as it always has.
- **The bellows** (DECK moves the glyph, ADR 0178): SPEED opens them; under GRAIN LENGTH folds them over a row of grains, under CLASSIC D-TIME folds them and a tick marks POSITION. TYPE is the glyph's twelfth input.

### The grain core (`chimera-engine/src/dsp/grain/core.rs`, ADR 0202)
STRETCH plays through the delay GRAIN's core (#749, ADR 0202), after Mutable Instruments Beads (`pichenettes/eurorack`, `beads/dsp/`, MIT, Copyright 2017 Emilie Gillet; the notice is in the file): its pulse-train clock, bell window, steal fades, Hermite read and `Engine<G>`, with Q32.32 heads. The file is origin/grain-delay's, byte for byte, so the two meet when main merges into tape-streaming. TAPE's source is a `GrainSource`:

```rust
pub trait GrainSource {
    fn frames(&self) -> usize;             // a head wraps at the end
    fn frame(&self, i: usize) -> f32;      // i < frames(), the source's own scale
    fn taps(&self, i: usize) -> [f32; 4];  // i − 1 ..= i + 2, asked with 1 ≤ i, i + 2 < frames()
}
```

`stretch::Src` reads the latched window in its own frame, always forward (END before START mirrors it), its frame `i` the window's `i − 1`, so a tap never wraps at the start; `frames()` is 2³⁰, so the core never wraps a head. Past the window's end it reads silence, its last 256 frames ramping there, so a grain over the end fades on nothing. Its taps are `SourceView::row_taps`: the four 12-bit samples in two unaligned words, not four halfwords.

All of it sits behind one adapter, `stretch/cloud.rs` (`Src`, `Cloud`, `GrainAt`): nothing else in STRETCH names a core type, so re-copying the core touches that file alone. What STRETCH needs that the core keeps to itself it rebuilds outside it: each slot's window (from the samples the grain has left and the length and lateness its start gave it), the newest grain's head (its start, speed and the samples since), and the strike (a `Grain` of its own started half a bell in). The core latches each grain's speed at its start, so a bend or the wow moves the grains to come; `Engine::set_speed`, which retunes the grains in play, is being added to #749, and until it lands `wow_depth_bounds` leaves STRETCH out of its 9 Hz flutter line (6.6 cents against 4 ± 1) and lets its peak reach 35 cents.


### GRAIN (`tape_engine/stretch/grain.rs`)
- **LENGTH** (param 22, 0.25 s · 32^x, 0.25–8 s, default 2 s): a pointer `T` walks the window (START to END; END before START walks it backward) at its length ÷ LENGTH a second, times **SPEED** (MOTION, `time_speed` as 0181: −1× to +2×, 0 freezes, default 1×). The source reaches its end at LENGTH on every key.
- **Grains:** six slots, four overlapping: a start every GRAIN/4 or later, never sooner, so four overlap at most and no start steals one still sounding (the core's fades are set past any grain's length); each grain GRAIN long (param 23, 30 ms · (250/30)^x, default 80 ms), read at the key's speed, so the pitch is the key's. A grain starts `A = (inc − tstep) · GRAIN/2` frames behind `T`, so it sounds the source where the pointer says at its middle, at any key. Four smoothstep bells a quarter apart sum to 2 exactly; their squares to within 0.08 dB.
- **The strike:** the note's first grain starts at full and plays its bell's second half: the attack is heard whole, then the others take over.
- **Pitched sources lock:** with the source's PERIOD (the importer's YIN, copied at pick) and two periods in the window, a grain starts a whole number of periods from the newest grain's head, as near its place as that allows (S950 MONO's lock, now always on where there is a period). The period is ROOT's where YIN's and ROOT's are whole multiples within 3 %: TX BELL C5's YIN reads its hum, 258.6 Hz, 1.2 % off half the 523.3 Hz strike, and locked to it the strike sagged 17 cents at 8 s; locked to ROOT it holds 0 cents. Every grain then reads the same waveform in phase: no comb, no beating between grains.
- **SPRAY** (param 24, 0–100 %, default 40 %): each start up to 25 ms further back at random, and each interval up to 25 % past GRAIN/4, from the voice's seed beside the core's draw. On an unpitched source it scatters the grains so they never comb; on a pitched one the lock rounds the scatter to whole periods.
- **END** (param 25, FINISH: HOLD or STOP, default HOLD): HOLD parks the grains on the window's last GRAIN of source while the key is held, 8 s at most, then fades as STOP; STOP starts grains up to one centred on the window's end (its second half reading silence) and no more. The note ends with its last grain. Key-up is the amp envelope's.
- **The level:** the grains' sum is divided by their windows' sum, ramped across each block from its ends: grains that read alike stay at the source's level through every start, the strike and the end. Grains that read unlike add in power: the source's power about `T` (eight reads a block across what a grain there covers, at a fresh offset) against the divided cloud's, the source's over 21 ms and the cloud's rising at once, lifts the cloud back, at most by the windows' sum over the root of their squares' (all unlike). As the last grains fade both gains hold. At 1× with no SPRAY every grain reads the pointer's sample and the note nulls ONE's (−62 dB on the bell, −69 dB on the saw).

- **Types** (`stretch/units.rs`, the owner's type-driven rule): LENGTH is a `Length`, GRAIN a `GrainLen` (each a `Seconds` held in its range), SPRAY a `Spray` (its share, its 25 ms reach and its interval jitter), END a `StretchEnd` (`Hold` or `Stop`); places in the window are `WinPos` (never negative), speeds `Speed`, lengths and counts `Samples`, the lock's `Period` (at least a frame; `None` unpitched), the note's stage `Playing`, `Parked(Samples)` or `Ending`. Each is made through its constructor, which holds its range and reads NaN as its floor, so the DSP never sees one out of range. The params keep the four as the knobs and the codec do, `[f32; 4]`, and hand them out typed (`length`, `grain`, `spray`, `end_mode`).



### Cheaper GRAIN builds (the owner's ask; none is the default)
Measured one at a time (`tape_stretch_grain_test::thrift_report` on a synthetic bell and saw, `stretch_listen` on TX BELL, PAD CHORD and VOICE AH through the whole engine; `tools/loop_count.py` on the ORBIT ELF). The core's grain loop is 77 instructions a sample (125 cycles) away from the window's end after the edge ramp moved behind a compare (bit for bit the same output; 88 near the end), so four grains are 525 cycles with the block's 25, three 400.

| Build | Cycles a voice (bill) | Pitch | Length (worst) | Ripple, TX bell 2/4/8 s | Verdict |
|---|---|---|---|---|---|
| GRAIN, four grains (shipped) | 525 (570 billed) | 0 c | 2.6 % | 0.42 / 0.44 / 0.53 dB | the default |
| Three grains | 400 (460 billed with the edge path) | 0 c | 3.4 % synthetic, 0.3 % TX bell | 0.37 / 0.50 / 0.53 dB; PAD 1.15 (four: 1.07), VOICE 0.68 (0.53) | under CLASSIC's 450; on the page as GRAIN 3 for the owner's ear |
| Two to four by stretch ratio | 275–525: the bill stays 525, it can't see the ratio (the source's length isn't the Sound's) | 0 c | 2.6 % | 0.39 dB | no saving in the bill |
| Plain head within 3 % of 1× at SPRAY 0 | ~100 there, 525 elsewhere: the bill stays 525 | +1 c | 3.2 % | 0.27 dB | no saving in the bill |
| Window from a table or an add-only polynomial | about −3 a grain-sample (the bell is 6 instructions) | — | — | — | core change: asked of #749 |
| Linear read in place of Hermite | about −13 a grain-sample | — | — | linear loses 0.45 / 1.81 / 4.04 dB at 1/8, 1/4 and 3/8 of a source's rate, Hermite 0.04 / 0.54 / 2.13: a 16 kHz source's top loses 2 dB more at 6 kHz | core change: asked of #749; audible on bright sources |

Three grains is the one build that brings GRAIN under CLASSIC's bill. Its pitch holds; its length is within 3 % on the TX bell (3.4 % on the synthetic bell at 8 s); its ripple is at most the default's on the bell and a little higher on the pad and the voice. It is a GRAIN option for the owner to hear, not the default.

### Bank 1 and bank 2
`dsp::grain` links into bank 2 (`bank2.x`, `*3dsp5grain*`), as do `stretch` and all its modules (CLASSIC, GRAIN, the adapter, the types), `read`'s `row_taps` and the TYPE's dim (`TapeDeck::stretch_only`). The idle images of the note's state are statics in `.bank2.rodata` (`stretch::IDLE`, `grain::IDLE_NOTE`, `cloud::IDLE_CLOUD`): as anonymous constants they had put 776 B into bank 1.

Each set's plain release build, `rust-objcopy -R .bank2` (bank 1) and `-j .bank2` (bank 2), against origin/tape-streaming `a6f35a7b` (with #753's SCRUB):

| Set | Bank 1 before → after (B) | Δ bank 1 | Δ bank 2 |
|---|---|---|---|
| `default` | 884,168 → 884,808 | +640 | +21,280 |
| `orbit` | 905,308 → 906,292 | +984 | +21,280 |

What the bank-1 bytes are (the default build, `llvm-size` and `rust-nm`, itemised on `8d0faa8d`, where the same change measured +728 B default and +944 B orbit):
- **The params' growth alone,** measured as GRAIN measured its own (0202): the four `f32`s added to `TapeEngineParams` on base, nothing else, moves bank 1 by −32 B (default) and +200 B (orbit), the code that builds and copies a snapshot (`init_chain`, `neutral_params`, `plain_init`) and layout; `MAX_SOUND_COST` 2,672 → 2,688 (284 params) on top moves nothing.
- **Closed-door data, +632 B of rodata:** the four param specs (+288 B), the machine table (+56 B: each machine's ENV 1 defaults now a table, not a match), STRETCH's cells, and the strings: the labels, idents and short names (LENGTH, GRAIN, SPRAY, FINISH, END, LEN, GRN, SPR, TYP, STRETCH_LENGTH, STRETCH_END), HOLD and STOP, and TYPE's CLASSIC, CLS POLY and GRAIN.
- **+128 B of text,** of which: the four params' one `get` and one `write` arm (+48 B, what the codec needs to load and save them with the door closed), the two slider laws' arms in `Law::range` (+28 B, the specs' display laws), the door stub for the TYPE's dim in `tape_note` (+32 B), and `view` −40 B (POLY (UNPITCHED) gone); the rest is the snapshot's growth above and layout (`region_data`, `recost`, `model_writes`, `machine_cell`: ±20 B each, as the params-alone build moves unrelated functions by as much).
No STRETCH code is in bank 1: no `stretch` or `grain` symbol lies below 0x08100000.

On the integrated tree (integration-1021, after #754 → after #752's merge on #749's final core and the integration's fixes, bank 1, each set as `just check` builds it): `orbit` 908,924 → 909,508 (+584); `default` 887,704 → 888,328 (+624); `bank2-gate` 889,452 → 889,924 (+472); `fault-inject` 861,640 → 862,336 (+696); `bench-algo` 906,608 → 907,224 (+616); `bench-algo,master-tape` 883,584 → 884,224 (+640); `bench-routing` 902,720 → 903,328 (+608); `bench-memory` 905,428 → 905,948 (+520); `bench-stress` 907,680 → 908,192 (+512); `bench-orbit` 886,980 → 887,572 (+592); `bench-tape` 869,152 → 869,864 (+712). Each line of `tools/bank1-budget.txt` names this ADR with its share.

### Compatibility
- Old STRETCH Sounds load as CLASSIC and render bit for bit as before (above). Every other machine is bit for bit too.
- Codes only append: TYPE's GRAIN is code 2 (the disk-codes fixture gains `E 34 10 2 GRAIN`); GRAIN's four params are ids 22–25 (21 is SCRUB's KEY, 17–20 SLICE's and PLAY's) (`B 34` lines, appended; END stored as its index, a stepped real, as THEME's HINTS).
- The factory STRETCH Sounds turn to GRAIN with ONE's ENV 1; their trims are re-measured to the set's median.

### Memory
`TapeEngineParams` grows 16 B, so the played Sound in each voice does: `VOICE_CHAIN_BYTES` 2,336 → 2,352 (8 voices, 128 B of D2), a voice 6,584 → 6,600 B, `ParamSnapshot` to 676 B. A Sound's worst sparse file grows 20 B: `MAX_SOUND_COST` 2,672 → 2,688 (284 params). The source stays in RAM as today; streaming STRETCH is later. A note's state fits `TapeDeck`'s 1 KB.

### Cost
| Voices beside the reserve | rev V | rev Y | rev V + ORBIT | rev V + master tape |
|---|---|---|---|---|
| CLASSIC, TAPE and AGE off / on (533 / 573) | 8 / 8 | 7 / 7 | 8 / 8 | 8 / 8 |
| GRAIN, TAPE and AGE off / on (653 / 693) | 8 / 7 | 6 / 6 | 8 / 7 | 7 / 7 |

## Alternatives considered
- **Replace CLASSIC with GRAIN:** the owner keeps both.
- **CLASSIC and GRAIN as two machines on MODEL:** every `TapeMachine` match in bank 1 grows an arm; a TYPE on the one machine is a value in a param and a row in a table.
- **Grains started at the pointer, not centred:** on a key an octave up an 8 s bell reached −30 dB 12 % early.
- **Random SPRAY alone, no lock:** a saw at 1× with SPRAY 40 % moved ±4 dB from block to block and a stretched bell's partials scattered ±40 cents; locked, both hold.
- **Grains trimmed to end inside the window:** short grains at the end left gaps that a block's divide by the windows' sum filled with ×4 gains; whole grains reading silence past the end fade smoothly.
- **Normalising by `1/√(grains + 1)` (Beads):** in phase (1× and every locked source) the cloud is 4.3 dB loud; by the windows' sum alone a scattered one is 4.3 dB quiet.
- **A phase vocoder:** right for an inharmonic bell, but an FFT a voice is far over the bill.

## Consequences
- GRAIN on an unpitched source (no PERIOD) keeps SPRAY's scatter only: a synthetic bell with no PERIOD reads 13–27 cents off and 1.4–2.0 dB of ripple at the default SPRAY (0.2 dB and up to 15 cents at SPRAY 0, a comb).
- GRAIN costs a voice on rev Y (7 → 6) and with TAPE and AGE on or the master tape on rev V; CLASSIC costs what it did.
- On #749's final core (integration-1021) `Cloud::set_speed` moves every grain in play and the strike to the key's speed each block, so the bend and the wow move GRAIN's grains, not only those to come; `wow_depth_bounds` (on CLASSIC, the neutral TYPE) was never exempted and is unchanged. The adapter starts no grain on a quiet block (`None`), reads each slot from `Started`, steals none (`steal: None`), and reads the window forward with a reversed grain at a negative `Speed`; the strike, half a bell from full, is its own (the core starts a bell at its foot). The reversed grains' taps sat a frame ahead of their place before; the audio goldens were re-recorded for it.

## Sources
- Owner, 2026-10-21; (private notes) (the investigation and proposal); (private notes) (the renders, the reports, the page, the bank-1 builds `x-E0` to `x-E12`).
- Mutable Instruments Beads, `beads/dsp/` (MIT), as above.
- Akai S950 Operator's Manual p. 32 (TIMESTRETCH, MONO's lock).
- `chimera-engine/src/dsp/grain/core.rs`, `chimera-engine/src/dsp/tape_engine/stretch/`, `chimera-core/tests/tape_stretch_classic_test.rs`, `chimera-core/tests/tape_stretch_grain_test.rs`.
