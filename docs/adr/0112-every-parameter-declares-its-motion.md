# 0112. Every parameter declares its Motion; the Part's knobs glide every one that says so

- **Status:** Accepted (owner, 2026-10-08)
- **Deciders:** owner (design approved 2026-10-08: "smoothness becomes a property every parameter declares, not a per-knob fix"), firmware
- **Supersedes in part:** [0107](0107-cutoff-ticks-in-octaves-and-the-filter-knobs-glide.md) (point 3: which knobs glide and how they are wired) and [0111](0111-algo-morph-glides-with-the-part-s-knobs.md) (point 1). Their tick laws, τ, landing rules and measurements stand.

## Context
ADR 0107 glided CUTOFF, RES and the FREQ/Q route amounts; ADR 0111 added MORPH, each by hand in `Knobs`. The sweep after it (#419) found the same staircase on the rest: ALGO's LEVEL, FDBK, FINE, DETUN (and VEL); MODAL's STRUCT, BRIGHT, DAMP, POS eased only 0.3 a block (~5 ms); FORCE and SPEED; and a per-voice `StageEase` that eased eight stage settings at 20 ms beside the Part's knobs. Each fix was a knob remembered; a knob forgotten stepped. CLAUDE.md: never snap; if the type system can solve it, it must.

## Decision
1. **`Motion` on every `ParamSpec`**, a required constructor argument (`chimera-engine/src/block.rs`), so no parameter can be defined without one:
   - `Motion::Continuous(Smoothing)`: `Knobs(GlideMs)` (the Part's knobs glide it, a one-pole of that τ), `Own` (its DSP smooths it: an effect's or the mixer's ease, a crossfade of two heads, an envelope or an oscillator travelling to it: a time or a rate bends a path, it never jumps a value), `NoteOn` (read when a note, a ring's step or a wrap starts) or `Unheard` (a setting, a route's hidden base, a value nothing reads).
   - `Motion::Stepped(Change)`: `Snap`, `Crossfade` (the DSP's own: filter TYPE/MODE/VOICE, ALG A/B, WAVE, MODEL) or `NoteOn`.
   - `ParamSpec::choice` takes a `Change`, so a choice can never glide; `continuous` takes a `Smoothing`, so a continuous value can never "step"; `stepped` (integer storage) takes a `Motion`, since LEVEL, FDBK or MORPH are stored in whole steps and still glide.
   - The domain is the spec's `Scale` (one truth: a tick is even in it, so the glide is linear in it): `Linear`, or `Octaves` for CUTOFF. No `Gain(dB)` domain: the gains that want one are stored in a dB law already (ALGO LEVEL's 0..99), and the stage levels glided linearly under `StageEase`, so they keep that.
   - A const check (`addr.rs`) refuses `Knobs` on any block a Sound doesn't hold (the knobs are a Part's): an effect's or a mix setting's would never be seen.
2. **One generic `Knobs`** (`dsp/knobs.rs`) replaces the hand-wired CUTOFF, RES and MORPH paths and `StageEase`: `KNOBS`, a static built at compile time from the registry, lists every `Smoothing::Knobs` param of a Sound (66 today). Each glides on its 0..1 place (`ParamSpec::normalize`, so CUTOFF in octaves), landing within 1e-5 of it; landed, the voice gets the stored value bit for bit. The voice writes the knobs still moving over its copy of the Sound (ALGO's into `AlgoLive`, as its bytes can't hold a fraction) **before** the modulators read it, so ENV SUS/SHAPE and LFO DEPTH/SHAPE glide too; routes add after, keeping their snap. The FREQ/Q route amounts glide as before (ADR 0107).
3. **A settled Part costs two compares.** `PartAudio`'s Sound and routes are `Tracked`: a publish stamps them with the load epoch and the Project's revision (every `&mut` path to a Project bumps it), and any write through them counts (`DerefMut`), so equal stamps mean equal values. The knobs read the Sound again only when its stamp moves, and glide only the knobs still moving. A silent Part lands (ADR 0107 point 4), a new Sound lands, a NaN lands.
4. **ALGO plays its knobs between steps**: FINE (`ratio_at`, linear in the ratio), DETUNE (`detune_at`), FDBK (`feedback_cycles`, linear between table entries), VEL, and the output scale's LEVELs (`patch_gain`), each exactly the old table at a whole step. A voice is kept by its glided LEVELs, so turning the last carrier to 0 fades it out over the glide, not at the turn.
5. **#419 by declaration**: LEVEL, FDBK, FINE, DETUN, VEL, MODAL's four macros, FORCE and SPEED, TAPE's five macros: `Knobs(30 ms)`. MODAL's (and TAPE's) 0.3-a-block ease stays, after the glide: it smooths the *modulated* value block to block (an LFO or ENV route into STRUCT), which the knobs, being before the routes, don't; at rest both land on the stored value, so a settled patch is identical, and a turn is only smoother (30 ms, then ~5 ms). Replacing it would step every route by blocks.
6. **CRSE, TRNSP and PITCH: `Stepped(Snap)`.** A ratio or a semitone is a step by nature; the oscillators' phase runs on, so nothing clicks, and a glide would be a portamento through inharmonic ratios on every turn. As on the TX81Z, a DX7, an Elektron or any keyboard transpose.
7. **Note-on-only params: `NoteOn`**, behaviour unchanged (MODAL's EXCITE, BODY, ENS, COUPLE, HALO, COLOR, BURST, MODES; TAPE's START, END, XFADE, DRAG; GLIDE TIME; ORBIT's ring values). Whether they should move a held note is a UX question for its own issue.
8. **The UI reads it**: a cell carries its spec's `Motion` and shows a value bar only for a continuous one, even with a discrete readout (FDBK, FINE, DETUN, the op envelope's rates); a stepped one shows only its value (PITCH, CRSE, TRNSP, ORBIT's CHANCE, MUTATE, GATE and BPM). A cell with no spec (a page's own) goes by its readout. The full visual and feel treatment is a follow-up (#422).
9. **A machine's own motion goes first**: a param the Sound moves by a motion of its own is handed on at once, not glided (`ParamSnapshot::moves_itself`, from the machine's table beside `reads`). Today one: SCRUB's POSITION, its jog, which the UI steps with DRAG's momentum; a glide would trail it by 30 ms. On every other machine POSITION glides. For UAT: scrub feel.

## Alternatives considered
- **Glide every knob, every block:** a dyn read of 66 params a block per sounding Part, ~25 cycles a sample a Part settled. The stamp makes settled ~1.
- **A publish sequence number** to gate the read: tests and the bench write `AudioShared` in place, past any publish; a missed change would snap. `Tracked`'s `DerefMut` catches every write by type.
- **Compare the whole Sound bytewise each block:** ~700 B a Part of copies and a compare a block; padding makes it unsound.
- **A `domain` field beside `Scale`:** two truths for one fact.
- **Keep `StageEase` per voice:** a second, hand-wired ease; per Part costs no voice bytes and every voice moves together (ADR 0107's reasoning).
- **Replace MODAL's 0.3 ease with the knobs:** routes would step by blocks (above).
- **A short pitch glide on CRSE/TRNSP:** a swoop on every detent; snap is what every FM synth and transpose does.

## Consequences
- RAM: `Knobs` 384 B a Part (was 88): Instrument +1.8 KB; a voice +56 B (`AlgoLive` +120, `StageEase` −64), 8 voices +448 B, the chain within ADR 0101's 2,112. bss +2.6 KB in all.
- Flash: +4.1 KB on the shipping build (750,856 → ~755,000 B text): the `KNOBS` table (528 B), a byte of `Motion` on every spec, the generic glide and ALGO's fractional reads.
- CPU (estimated: host-timed and counted on the thumb build, not benched on the unit): a settled sounding Part ~70 cycles a block (~1 a sample; ADR 0107's always-on glide was ~400–500); a moving knob ~100–150 a block plus ~40 per voice to write it; a publish while editing re-reads all 66 once, ~5–6k cycles, on every sounding Part (any edit moves the Project's revision, which every Part's stamp carries), once a UI frame. A silent Part, a flag. Assumes the Project's revision moves only on edits (as `Marks` already does); a per-frame `&mut` would cost a re-read a frame, never a wrong value.
- Billed: `Knobs::COST`, 12 cycles a sample a sounding Part, on its first voice (at admission and in `recost`): settled, one knob moving and the re-read spread over a 12-block UI frame, 720 / 64. Estimated, so bench rows for a gliding knob and for a publish each frame while editing come at ship. The worst single block (six sounding Parts re-reading at one edit, ~36k cycles) is over that spread and sits in the reserve.
- An op LEVEL gliding to 0 still runs its operator until the glide lands (~350 ms for a full turn; 95 % in 90 ms), 60–68 cycles a sample a voice. The allocator bills what the kernel runs (`Voice::held_ops_extra`, from `AlgoEngine::running`), so the bill drops when the op stops, a block late as every bill is. A FDBK gliding to 0 is 1 cycle a sample (`COST_FEEDBACK`), and the stages' gates already fade 20 ms past their bill: both left to the reserve.
- A note started mid-glide starts its engine on the stored values and joins the glide in its first block (ADR 0111).
- A lone voice rendered on its own (`Voice::render`) plays the stored values: a test that wants the glide renders through a one-voice Part (`Rig::render_glided`).
- A new parameter states its motion or doesn't compile, and its declaration is tested: `param_motion_test` turns every `KNOBS` entry through the panel's acceleration and bounds its step at the voice's copy (what the engines read; the stage gates read the stored values); `param_declarations_test` holds every `Unheard` param bit for bit under a held note, every `NoteOn` one until the next note (which hears it), and lists every `Own` one with what smooths it.

## Sources
- #419, #417, #399; ADR 0107, 0111, 0101, 0049, 0010.
- `chimera-engine/src/block.rs` (`Motion`), `chimera-engine/src/dsp/knobs.rs`, `chimera-engine/src/tracked.rs`, `chimera-core/tests/param_motion_test.rs`.
- (private notes).
