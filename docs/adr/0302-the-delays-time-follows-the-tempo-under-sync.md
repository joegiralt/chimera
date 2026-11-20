# 0302. The delay's TIME follows the tempo under SYNC, in notes, for TAPE and GRAIN

- **Status:** Proposed
- **Deciders:** owner (2026-11-20, #959, the design's recommendations), firmware

## Context
The delay's TIME is 10–500 ms and knows nothing of the tempo (#959). The owner's brief: the principle of least surprise. Two states, FREE or SYNC; in SYNC a knob picks straight, dotted and triplet notes; a MIDI clock is followed without more settings; both TYPEs, TAPE and GRAIN. The design, with the precedent read from the manuals: (private notes).

What the code had (origin/0.4 fda23e52):
- `DelayParams` ids 0–11, TIME continuous in ms (ADR 0014: the line is 500 ms so the FX bus fits AXI). A TIME change crossfades two read heads over 20 ms (`TIME_FADE`); nothing glides.
- The delay plays behind `DelayDoor` in bank 2 (ADR 0202, 0213), and its call took no tempo. `FxBus::process` already had the tempo, for the reverb.
- The reverb's SYNC (ADR 0142, reverb TYPEs spec § Tempo sync): `rev::sync`'s `Tempo`, `Division`, the note lists, `FoldState` and `fold`, which halves a time into range with 2 % hysteresis and holds it to the limit inside that 2 %.
- The tempo is `OrbitRunner::tempo` (ADR 0071, 0072, 0080): the external clock's held beat while it leads and through a dropout, else the project's BPM, on a 0.25 BPM grid with the clock's jitter held upstream.

### Precedent, from the manuals
- **One switch, and TIME turns notes.** Strymon Brig (User Manual Rev A, pp. 3, 5, 15): under MIDI clock or tap the TIME knob is the division, TRIPLET, EIGHTH, DOTTED EIGHTH, QUARTER, by length. Eventide H9 (Algorithm Guide V12; H9000 TimeFactor): one Tempo ON/OFF, and every time parameter then turns "in common note increments". Ableton Delay (Live reference): a Sync/Time toggle. Soundtoys EchoBoy (manual p. 28): Time against Note/Dot/Trip. Boss DD-500 (Owner's Manual pp. 5, 12): TIME and NOTE.
- **One list, ordered by length, triplets and dotted values in their places.** Elektron Digitakt II (manual § 12.2): 1/32T, 1/32, 1/16T, 1/32., 1/16 … 1/4. Roland SP-404MKII (reference, Sync Delay): the same shape. Brig. Chimera's own reverb.
- **With no external clock, the box's own tempo, never ms.** Digitakt II § 6.2.1, DD-500, H9.
- **Separate memory for the free time and the note** wherever they are two parameters: DD-500 (TIME and NOTE), Ableton.
- **Too long for the range:** only Boss says (DD-500 p. 17, its synced wave rate plays at 1/2 or 1/4). Chimera's reverb halves too.
- **A tempo change** splits by character: tape-voiced units bend the pitch (Brig: "authentic pitch effects"; EchoBoy in ms; Ableton's Repitch), clean ones crossfade (H9000's Xfade, 2–200 ms; EchoBoy on note values; Ableton's Fade).
- **GRAIN** is after Mutable Instruments Beads (manual): in its delay mode a clock sets the base delay.

## Decision
1. **Two states.** `DelaySync`, FREE or SYNC, delay param 12 (ident `SYNC`), stored as its index as TYPE is. Default FREE.
2. **Under SYNC, TIME turns a note,** one a detent, through the reverb's `GATE_LENGTH` as it is: 1/32T, 1/32, 1/16T, 1/32D, 1/16, 1/8T, 1/16D, 1/8, 1/4T, 1/8D, 1/4. `DelayNote` is a place in that list, delay param 13 (ident `NOTE`), stored as the reverb stores a division (`i / (n − 1)`). Default 1/8D: 375 ms at 120 BPM, TIME's own default, so a fresh project switched to SYNC plays the same.
3. **Both TYPEs.** The note becomes the delay's one base time (`delay::sync::tap`), read by TAPE's head and by GRAIN as where its grains start behind the write head. SYNC on a note is FREE at that note's ms, bit for bit. GRAIN's DENSITY stays free; a tempo-locked DENSITY is a follow-up (private issue #1044), not built.
4. **The free TIME and the note are remembered apart.** SYNC never rewrites `time_ms`: back in FREE the ms is the one left there. This departs from the reverb's `with_sync`, which converts one slot; a delay's free time is tuned by ear.
5. **Old projects load FREE and play bit for bit.** A file without ids 12 and 13 keeps the defaults (`decode_block`); FREE reads neither the tempo nor the note. `delay_tape_hash_test`, `fx_golden_test`, `delay_grain_test` and `golden_test` stand unchanged.
6. **A note longer than the line plays halved, on the grid, and is marked.** `delay::sync::fit`: it halves when the note passes 500 ms and comes back whole only 2 % inside it (`rev::sync::HYST`), so a clock at the boundary never flips it; faster than 10 ms it doubles the same way. Unlike the reverb's `fold` it never holds the time to a limit: a delay repeats, and a time held off the grid drifts a little more each repeat. The cell shows the note that plays with the reverb's mark, `▾1/8`. The line stays 500 ms. A note the tempo makes exactly 500 ms (1/4 at 120 BPM, 1/4T at 80) plays whole from rest; the limits give 0.01 ms for `f32`'s 2/3.
7. **The tempo is the reverb's**, `Instrument::tempo`: the external clock when it leads, held through a dropout, otherwise the project tempo, playing or stopped. There is always a tempo, so SYNC never falls back to ms.
8. **Changes.**
   - A tempo move under the same note and fold **glides TAPE's head**, as tape does: at most 2 % off the tape's speed (`GLIDE_SPEED`, 34 cents, MECHANICS' own scale), a block's ramp at a time, landing exactly. A move that would take over 250 ms (`GLIDE_MAX_S`; more than 240 samples at 48 kHz) crossfades instead. This departs from the reverb, which crossfades pitch-free, on purpose: it is a tape delay, and a DAW's ritardando would otherwise be a chain of comb-smeared 20 ms crossfades, one every 0.25 BPM.
   - A note turned, a fold, SYNC switched, and every FREE TIME turn **crossfade** over 20 ms, as before.
   - **GRAIN jumps:** grains playing finish where they are, the next start on the new time, each under its own bell. TAPE's head, while a TYPE crossfade still plays it, crossfades.
9. **Where SYNC lives.** Cell E of DLY › CHAR and of DLY › GRAIN (free on both, so it sits in one place whichever TYPE shows; the main page is full). From the main page **MIX + turn on TIME** flips it, right for SYNC and left for FREE, and the TIME cell's band says so: `MIX + TURN  FREE / SYNC`. TIME keeps its label and place; under SYNC its slot is the note (`view::view` binds NOTE there), so its value, its turn, a learned CC and the editor all read the note. The note turns a detent a note at any speed (a list's curve).
10. **What the band says.** On TIME the gesture; folded, in its place, `HALVED TO FIT 500 MS` (or `DOUBLED TO FIT 10 MS`). On SYNC, `TIME TURNS NOTES OF THE TEMPO`. The design's longer line (`DOTTED EIGHTH · 375 MS AT 120 BPM`, with `EXT`) is not built: the band's foot is one line of about 30 characters, and the gesture's hint, which the owner asked for, has it. The readout is the note as the reverb writes it (`1/8D`).
11. **Typed.** `DelaySync`, `DelayNote`, `Tempo`, `FoldState`, `Aim` (what a block's TIME was read from), `Cause` (a tempo move or a step) and `Slide` (a block of glide): no bare flag crosses a function.
12. **Bank 2.** The DSP is `delay::sync` (`fit`, `tap`, `aim`, `glide`), an S4 group (`*5delay4sync*`); it reads the reverb's `GATE_LENGTH` where it is, in bank 1 (a named copy in bank 2 is folded with the reverb's by the linker, and bank 1 then reads bank 2: the firmware won't split); `tape::run` and `grain::run`, S0-S3's, call it, and `tape::span` gains a `GLIDE` instance inlined into `sync::glide`. The UI is `ui::delay_sync` (S4), reached through doors that exist: the note's text through `law_text` (`Law::DelayNote`), its curve and the hint through `feel`, the band's words through `band_note`, and MIX + turn through `feel`'s new `Ask::Snap`. Bank 1 gains the two specs, `DelayParams`' 8 B, the tempo in `DelayDoor::run`'s call, the delay's fold in the published clock word's top byte (`meter`), `SlotCtx`'s SYNC and the slot's binding, and the snap's stub: closed-door data and stubs (ADR 0198, 0251). A closed door silences the delay, as before.

## Alternatives considered
- **Always crossfade on a tempo change** (the reverb's way, H9000, EchoBoy's notes): pitch-free, but a chain of crossfades under a moving clock, and not what a tape delay does. The owner chose the glide for small moves.
- **A 1 s line** so a quarter plays whole down to 60 BPM: +48 KB of AXI the FX bus doesn't have, or 30 % of the Sound pool. Rejected by the owner; the note halves.
- **The reverb's `fold`, clamp and all:** off the grid inside its 2 % band, or +960 B of line to keep the band inside it.
- **Refusing a note that doesn't fit:** the note would vanish at slow tempos.
- **A dotted/triplet modifier** (EchoBoy, Ableton Echo): a second control the main page has no cell for; one-knob hardware uses the list.
- **SYNC on the main page:** it would push REV or TONE off it.
- **One slot for the ms and the note** (the reverb's): loses the free time on a trip into SYNC.
- **A tempo-locked DENSITY** (Beads' clocked SEED): a new behaviour, filed.

## Consequences
- MIX + turn on TIME no longer snaps the ms to its ends; it flips SYNC.
- Below 120 BPM a quarter plays as an eighth, and below 80 BPM a dotted eighth halves too. At exactly the boundary the fold depends on where the tempo came from (halved coming up from 119 until 122.4), as the reverb's does.
- A CC learned on TIME under FREE drives the ms, under SYNC the note: two params, two maps.
- The editor and the NRPN chart gain DELAY SYNC and DELAY NOTE.
- Needs the owner's ear on the unit: a DAW clock at 90, 120 and 140 BPM against the grid, and a tempo ramp for the glide.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, against 0.4 at c93eeab4 (merged in), each set built in both trees. Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| orbit | +520 | +4,640 |
| default | +464 | +4,608 |
| bank2-gate | +488 | +4,608 |
| orbit,usb-config-writer | +528 |  |
| fault-inject | +360 |  |
| bench-algo | +488 |  |
| bench-algo,master-tape | +512 |  |
| bench-routing | +432 |  |
| bench-memory | +400 |  |
| bench-stress | +464 |  |
| bench-orbit | +488 |  |
| bench-tape | +416 |  |
| filter-probes | +368 |  |

- **Bank 1 on ORBIT, +520 (903,360 B; about 14.2 KB free):** `DELAY_SPECS`' two specs (+144), `UiState::ctx` building the longer `SlotCtx` (+156), `Division::write` now a function of its own where `param_text` had it inlined (+256 against −212), the door call's tempo, the clock word's top byte, TIME's binding in `view::view`, the snap's stub, and alignment.
- **Bank 2:** S4 +4,448 (ORBIT): `sync::glide` 2,724 B (`tape::span`'s gliding instance, its `tanhf` with it), `sync::tap` 796 B, `sync::aim` 376 B, `ui::delay_sync` about 440 B with its words. More than the design's 1.5–2.5 KB: the glide is a whole copy of TAPE's loop, kept out of S0-S3. S0-S3 +192 (ORBIT; 7,104 B free, under ADR 0251's 8 KB warning): the calls in `tape::run`, `grain::run`, `walk`, `feel` and `notes`.
- **RAM:** `DelayParams` +8 B (each `AudioShared` copy and the UI's); `TapeDelay` +8 B in AXI (what it read, the fold, the glide's speed). The line is unchanged.
- **CPU, host** (release, best of five runs of 20,000 blocks, plucks through feedback and MECHANICS; `delay_sync_test::host_cost`, twice): TAPE FREE 1,475–1,476 ns a block against 0.4's 1,451–1,508; TAPE SYNC 1,477–1,481; TAPE with the tempo always moving (gliding nearly every block) 1,507–1,511, +2 %; GRAIN FREE 709–710 against 695–702; GRAIN SYNC 711–715. Within the run-to-run spread but for the glide. Not read on the unit; the bill is unchanged (GRAIN 320, ADR 0202): SYNC adds a note's `ms` and the fit a block, the glide two multiplies a sample while it runs.
- **Null:** FREE at any tempo and note is `process_wet`'s output bit for bit, TAPE and GRAIN (`free_plays_the_same_at_any_tempo_and_note`); `delay_tape_hash_test`'s and `fx_golden_test`'s hashes, `delay_grain_test` and `golden_test` are unchanged, and the host bench's checksums equal 0.4's. SYNC on a note is FREE at its ms bit for bit (`sync_is_free_at_the_notes_ms`).
- **The glide, measured** (`delay_sync_test`): 120 → 121 → 120 BPM on 1/8D bends a 440 Hz repeat ×0.9800 to ×1.0200, its second difference ×1.05 of the steady repeat's; crossfades (a jump, a note, a fold, SYNC, FREE TIME) ×1.00; a 126 → 100 BPM ramp ×1.05 on 1/8 and ×1.30 through 1/4's fold (a step of a hundredth of the sine would be ×3); GRAIN's tempo moves under ×1.0 of its steady.

As built, beyond the design: the glide's speed ramps across each block and slows in over its last blocks (a block covers a quarter of what is left), so the head starts and stops with no kink in the repeat; a head still gliding when a step comes slows to rest over one block before the crossfade starts.

## Sources
- The design and its precedent table: (private notes).
- Strymon Brig User Manual Rev A; Strymon Volante User Manual Rev E; Strymon TimeLine User Manual Rev F (strymon.net/manuals).
- Elektron Digitakt II User Manual, § 6.2.1, § 12.2.
- Boss DD-500 Owner's Manual, pp. 5, 12, 15, 17.
- Roland SP-404MKII Reference Manual v5, Sync Delay.
- Eventide H9 Algorithm Guide V12; H9000 manual 2.1.12, TimeFactor.
- Ableton Live reference manual, Delay and Echo.
- Soundtoys EchoBoy manual, p. 28.
- Mutable Instruments Beads manual (pichenettes.github.io).
- ADR 0014 (the line), 0142 and the reverb TYPEs spec § Tempo sync (`rev::sync`), 0202 and 0213 (GRAIN, the delay's door), 0080 (the clock), 0198 and 0251 (the banks), 0086 (hints and marks), 0279 (`feel`).
