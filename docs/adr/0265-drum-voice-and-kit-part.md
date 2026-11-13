# 0265. A DRUM Part keeps its kit on the Part: 12 compact, typed pads

- **Status:** Accepted (accepted by the owner, 2026-11-12/13)
- **Deciders:** owner (2026-10-05, 10-12, 10-26, 11-10 rulings and ship list, 11-11 audition marks, 11-12 flash and "strict type-driven development"), firmware

## Context
0.4's DRUM engine (#214, #215; spec `docs/superpowers/specs/2026-11-10-drum-engine-design.md`) makes a Part play a 12-pad kit in the Analog Rytm's shape with the Machinedrum's machine structures (ADR 0114: the manual's descriptions only, our algorithms and names). The owner fixed the shape on 2026-11-10: compact pads as on the Rytm, not a full Sound per pad; pads hold only DRUM machines; the Rytm's choke pairs as groups A–D, CUT within a pad, STACK across pads; C0–B0 the kit, C1 up the pad at its pitch. 28 machines ship in 0.4 by ear; slice 1 builds 12 of them on four kernels.

Where the kit lives decides RAM and the card format. `ParamSnapshot` is copied per Sound: a Part's live Sound, its glide target, the library's copies. A kit inside it would grow every snapshot by ~200 B (12 pads × ~17 B in slice 1, more once each pad has its sample, mix, filter and amp), times every Part and every copy, for the five Parts that are not DRUM.

Flash: bank 2 has ~400 B free on main (ADR 0263), far under ADR 0251's 8 KB floor. The ORBIT build's bank 1 is 863,440 B of 917,504: ~54 KB free. The spec estimates the full engine at ~29 KB. On 2026-11-12 the owner approved spending bank 1 down to ~20 KB free for the full drum engine.

The owner's rule (2026-11-12): every unit, range, index, code and mode is a type. The host prototypes' `Knobs = [f32; 8]`, `&str` machine names and `Box<dyn Machine>` don't port.

## Decision
- **The kit lives on the Part, not in `ParamSnapshot`.** A DRUM Part holds a `Kit` of 12 `Pad`s beside its live Sound. The Sound's FILTER, AMP, LFOs and MOD are shared by every pad in slice 1. A hit copies its `Pad` into the voice as a `DrumHit`; the voice never reads the kit again.
- **Compact pads.** `Pad { machine, knobs, level, pan, choke, retrig }`, all typed: `Machine`, `KnobSet`, `U7`, `Pan` (−64..=63), `ChokeGroup` (OFF, A–D), TAPE's `Retrig` (STACK, CUT). Later slices add the sample, mix, per-pad filter, amp and modulation to `Pad`, keeping it near the spec's ~200 B.
- **The type layer** (`chimera-engine/src/dsp/drum/types.rs`). Every type is `Copy`, its fields private, its constructors clamp or return `Option`, never panic:
  - `PadId` 0..=11 (`ALL`, `index`, `number` 1..=12, wrapping `next` / `prev`);
  - `Knob` A–H (`MAIN` A–F, `MORE` G–H);
  - `KnobValue(U7)` with `unit()` = k / 127;
  - `KnobSet([KnobValue; 8])`, reached by `Knob` only;
  - `Kit` (`pad`, `pad_mut` by `PadId`);
  - `Semitones` 0..=115;
  - `KitNote`;
  - `DrumHit { pad, params, pitch }`.
- **`Machine` is a closed enum with a law table.** Each machine maps to a `Kernel` (`Tone`, `Metal`, `Burst`, `Plain` in slice 1). A const law per machine turns its `KnobSet` into that kernel's typed params (ranges, curves, defaults). Machines don't carry code, so a machine costs a table and a match arm. There are no trait objects and no names in strings.
- **Disk codes are fixed by hand** (ADR 0045), frozen in `disk_codes_v1.txt` as `C DRUM_MACHINE` and `C CHOKE`. The codes are TONE KICK 1, TONE SNARE 2, TONE TOM 3, TONE CLAP 4, TONE COWBELL 5, TONE CYMBAL 6, METAL HAT 7, METAL JINGLE 8, BODY SHAKER 9, PLAIN SINE 10, PLAIN NOISE 11, PLAIN IMPULSE 12. Later machines append, and a code is never reused. Code 0 is left free for machine OFF (the sample-only pad).
- **Where the kits live (Task 6, 2026-11-12).** Every Part has a kit, DRUM or not, so switching engines keeps it. The Parts live in the UI's state in D2, which is full (~8 B, ADR 0260), so the six kits (13 B a pad, 156 B a kit, 936 B) with each Part's selected pad and its 12 pad lights (1,014 B in all, `memory_budget_test`) are `project::Kits`, a static lent to the `Project` at build with the pool's arena and reached by `PartId` (`Project::kit`, `kit_mut`). The static is in ITCM (NOLOAD `.itcm` at 0x400, built in place at `take_ui`), the first use of ADR 0266's region: in AXI its 936 B left the ORBIT build 836 B under ADR 0087's 32 KiB margin (31,932 B free), with no clean sound-identical AXI saving that large. ITCM is CPU-only, which the kits need: the card writes a copy (`put_kit`), never the kits through DMA. The Project's pointer to it (+4 B) is paid for by moving the arena's per-row pending flags out of its padded rows (−96 B): D2 is 92 B lighter on every build. A Sound load never touches the kit; NEW and a load reset it.
- **The audio reads the kits through a view (Task 5, 2026-11-12).** The UI owns `Kits`; the audio never touches them. `dsp::drum::view::KIT_VIEW`, a static of atomics only in ITCM beside them (1,944 B), holds each Part's kit as `Pad::to_bytes`' 13 B a pad in a two-slot seqlock, its selected pad, the live hits for the UI (the last hit's pad, and the `PadSet` of every pad struck since the UI last took them, so a UI frame's hits all light, not just its last: Task 9), and the Part's hit seed (the audio's own). The UI's one `KitWriter` publishes a Part's kit only when it differs from the published slot, writing the other slot, then flipping. The audio reads a pad at the hit and never waits: on the chip the UI can't run inside the audio's interrupt, so a read is never overtaken, and on the desktop a lapped read reads again, at most twice more. Any bytes decode to a valid pad (`Pad::from_bytes`), so NOLOAD ITCM needs no build. Not `AudioShared` (AXI, three copies, 2.8 KB) nor a `TripleBuffer<Kits>` (2.8 KB, and a reader handle in AXI and both shells' plumbing).
- **Hits (Task 5).** On a DRUM Part a note's key is a `KitNote`, never snapped: C0–B0 strike their pads, C1 up the selected pad. The `Instrument` plays a kit's notes out of line (`drum_on`, bank 1), apart from the port-generic note-on, whose ORBIT copy is bank 2's (ADR 0251). A hit first ends, through the fade as a shed does, its own pad's voices if RETRIG is CUT and every other pad's in its choke group (not OFF), then takes a voice without re-striking its key's (STACK keeps the old one). A voice still sounding, whichever Part's, is waited out by the `Instrument` (`waiting`), so a voice never queues a hit to replay into another engine's slot. A held note never restarts across a switch to or from DRUM. Each hit takes its Part's next `HitSeed` (the view's per-Part seed).
- **Note-off and AMP (Task 5).** DRUM doesn't sustain: note-off lets the key go, which only the Sound's AMP hears (a route into VCA, the VCA gate). With neither, nothing ends a hit and it rings its machine's whole decay; with ENV → VCA, the envelope's release ends it.
- **LEVEL and PAN (Task 5).** LEVEL scales the machine before the filter, k / 127. PAN is constant power, unity at centre: off centre, a voice puts its mid on the Part's mono bus and its side on a side bus on the audio stack, and the pair takes mid − side left, mid + side right under the Part's own pan and level. The sends and the scope take the mid. A Part with no panned pad mixes bit for bit as before.
- **The `Kit` record (0x0024, not critical).** One a Part, after its `Origin`: 13 B a pad, pads 1–12 in order: machine code, knobs A–H, level, pan (`i8`), choke code, RETRIG code. Not critical, because a firmware before DRUM refuses any DRUM Sound by its critical `Engine` record, so the kit it skips belongs to a Part that isn't playing DRUM. A file without it (0.3) loads `Kit::DEFAULT` on every Part and still reads Saved. An unknown machine code loads as the pad's default machine with its default knobs. An unknown choke or RETRIG code loads as the pad's default, and a byte past its range is held to it. Any of these makes the load lossy (not Saved). A wrong length is `Bounds`, and a second `Kit` in one Part is `Corrupt`.
- **DRUM INIT.** `Kit::DEFAULT` is the Rytm's layout on slice 1's machines, each at its law's defaults with RETRIG CUT: 1 TONE KICK, 2 TONE SNARE, 3 PLAIN IMPULSE (the rim's stand-in), 4 TONE CLAP, 5–7 TONE TOM at PITCH 30, 50 and 70, 8 METAL HAT at DECAY 20 (closed) and 9 at DECAY 80 (open), 10 TONE CYMBAL, 11 TONE COWBELL, 12 BODY SHAKER. The chokes are A for pads 3+4, B for 6+7, C for 8+9 and D for 10+11. DRUM joins `EngineType::PLAYABLE`, as TAPE did in ADR 0187: DRUM INIT is a browser INIT row in place of COMING SOON, one of NEW's INIT Sounds, and program 4 of bank 1.
- **Notes, as the Rytm (`KitNote: From<MidiNote>`, total).** Elektron's C0 is MIDI note 0: notes 0–11 are `Pad(PadId)`. Notes 12–127 are `Chromatic(note − 12)`: C1 plays the selected pad at its tuned pitch, C#1 up a semitone, and so on.
- **Slice 1's machines and kernels.**

  | Machine | Kernel |
  |---|---|
  | TONE KICK, TONE SNARE, TONE TOM | tone |
  | TONE CLAP, BODY SHAKER | burst |
  | TONE COWBELL, TONE CYMBAL, METAL HAT, METAL JINGLE | metal (METAL JINGLE gates the bank with the burst's particles) |
  | PLAIN SINE, PLAIN NOISE, PLAIN IMPULSE | plain |

  The knob sets are the spec's table. FM, BODY membrane, the world batch, VOX ROLL, the sample layer, combine, the pool, per-pad filter / amp / mod, pad channels, OUTPUT and RESERVE come in later slices.
- **Kernels go in bank 1** under ADR 0251, since bank 2 has ~400 B free. Each raised `tools/bank1-budget.txt` line reads `; ADR 0251 +N DRUM slice 1 (from X)`. Kernel entry points are `#[inline(never)]`, kept outside `chimera-stm32/build.rs`'s bank-2 globs.
- **Flash after DRUM is projected at ~20–25 KB free in bank 1.** That is under the ~40 KB line, so this ADR escalates it to the owner, who approved it on 2026-11-12. The bootloader slots (0x08180000 and up) stay the last reserve.

## Alternatives considered
- **The kit inside `ParamSnapshot`:** ~200 B more in every snapshot copy of every Part, DRUM or not.
- **A full Sound per pad:** 684 B × 12 = 8.2 KB a kit Part. RAM can't hold six of them (owner ruling 4, 2026-11-10).
- **Machines as trait objects (the prototypes' `Box<dyn Machine>`):** they need heap, vtables in rodata and dynamic dispatch on the audio path, and they can't be matched exhaustively.
- **`[f32; 8]` knobs and `u8` pad indices:** unchecked ranges, and a knob could be reached by a bare index. The type layer replaces those runtime checks.
- **Bank 2 behind doors:** ~400 B free.

## Consequences
- Every later DRUM task (laws, kernels, engine slot, kit codec, hit routing, pages) builds on these types and can't build an out-of-range pad, knob, pan or note offset.
- A non-DRUM Part pays nothing in its Sound for the kit.
- The kit has its own record in the project file (`Kit`, above), with machine and choke codes as frozen here.
- Bank 1 drops to ~20–25 KB free after the full engine. Any later feature beyond 0.4 has to weigh the bootloader slots or cuts.

## Sources
- Spec `docs/superpowers/specs/2026-11-10-drum-engine-design.md` (rulings 2026-11-10, ship list, audition marks 2026-11-11); plan `docs/superpowers/plans/2026-11-12-drum-slice-1.md`.
- ADR 0045 (disk codes), 0051 (exclusive voice state), 0101 (voice chain size), 0114 (curated DSP, Machinedrum provenance), 0170 (TAPE's RETRIG and KIT), 0251 (bank 1 when bank 2 is full), 0263 (bank 2 ~400 B free).
- Elektron Machinedrum manual (OS 1.63), Appendix A, machine reference: structure and knob sets only. Elektron Analog Rytm manual: architecture and MIDI note scheme.
