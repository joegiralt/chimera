# DRUM engine and KIT Part (0.4): design

Status: proposal for the owner, 2026-11-10. Nothing is built. Mockups: (private notes). Issues: #214 (DRUM), #215 (KIT).

## Goal
A drum Part in the Analog Rytm's shape with the Machinedrum's machines: every pad is a synthesis machine and a short one-shot sample, each with its own level, joined by a combine stage, then our filter, FX slot and VCA/LPG. Every machine is built from parts Chimera already has (ALGO operators, VA oscillators and NOISE, MODAL's exciter and bank, the filters, TAPE's head). Machines take the Machinedrum's structure and knob set per drum type, from the manual's descriptions only (ADR 0114, #214 2026-10-12): no firmware, ROM, tables or sample data, and our own names.

## Owner rulings this builds on
- 2026-10-05: DRUM is machines with a knob set each, made from existing engines; single-shot PCM from TAPE's one-shot reader (now WALK, FORWARD; ADR 0203).
- 2026-10-12: the Machinedrum's machine structures, our algorithms.
- 2026-10-26: the Rytm's voice; combine with VA's MODE set plus FM both ways; the machine is the carrier; DIRECTION per Sound (MCH ← SMP default, SMP ← MCH); multiplying modes are carrier × (1 + modulator); CLIP a mod destination latched at the hit; a kit is 12 Sounds plus a small resident sample pool; one kit per Part; shared, stolen voices with a per-Part RESERVE on every Part; Rytm MIDI (per-pad channels, C0–B0 the kit, C1 the pad at its pitch, C#1 up chromatic); choke groups and CUT or STACK per key.
- 2026-11-10: "their machines with those parameters"; the sample "really, really small, plays once, back, forth"; both layers have their own mixer and go straight into the filter and everything after it.

## The voice
```
MACHINE ─ LEVEL ─┐
                 ├─ COMBINE (MODE, AMOUNT, DIRECTION) ─ FILTER ─ FX slot ─ AMP (VCA/LPG) ─ PAN ─ Part bus / OUT
SAMPLE  ─ LEVEL ─┘
```
- **Machine:** one of the kernels below with a machine's law over its six knobs (plus up to two on MORE). Its internal decay is the hit's life; its PITCH the pitch. Machine OFF makes the sample the carrier (the Rytm's sample-only track; the Machinedrum's ROM machines).
- **Sample:** a clip of the kit pool, read by TAPE's WALK head (12-bit decode, Hermite, `inc` from the clip's rate and root), once through: FORWARD, REVERSE or PING-PONG over START and LENGTH. PITCH is an offset from the machine's pitch. CLIP = the pad's base clip + a modulation offset, rounded and clamped to the pad's reach, latched at note-on (round robin, velocity layers, random).
- **Combine:** `chimera-engine/src/dsp/va/combine.rs`'s MIX, RING, XOR, BITS, MAX and AM reused as they are (they take two signals), plus FOLD-into and FM. RING, AM and FOLD use `c·(1 + m)` so a finished clip never mutes the machine. FM and FOLD read DIRECTION; the rest dim it with `<MODE> HAS NO DIRECTION`. FM MCH ← SMP moves the machine's phase increment; SMP ← MCH moves the head's read rate (through zero clamp as VA FM, ADR 0159).
- **After combine:** the existing chain unchanged (FILTER TYPEs, FMD, FX slot ADR 0155, AMP with LPG/DECAY/STRIKE ADR 0195, SENDS). A pad's voice ends when the AMP gate or the machine's decay is under −80 dB, as today.

## Chain and pages
```
KIT ── MCH ── SMP ── MIX ── FLT ── AMP ── MOD
 └ KIT   └ <machine>  └ SMP  └ MIX  └ FMD FX JACK  └ SENDS  └ MTX E1-E3 SPD L1-L3
   KEYS    MORE
   POOL    PIT
```
| Page | Cells |
|---|---|
| KIT | PAD · MACHINE · LEVEL · PAN · OUTPUT · RESERVE (Part-wide). The band draws the 12 pads (note, name, machine, choke letter, reach dots, lit while sounding) |
| KEYS | PAD · CHANNEL · CHOKE (OFF, A–D) · RETRIG (CUT, STACK) · CHROMATIC · PRIORITY. Band: a C0–B0 keyboard with names and choke letters, the pad's C1 line, the voice line (VOICES n OF 8 · RESERVE · STEALS OLDEST) |
| POOL | the kit's clips grouped by pad, rate, length, size; the kit allowance and the arena meters. EDIT hears, MIX + EDIT adds from the card, MIX + MINUS removes |
| <machine> (MCH) | the machine's six knobs; the header names the machine; the band draws what it does (sweep, operators, partials, modes) |
| MORE | MACHINE · up to two more knobs of the structure · rest empty |
| PIT | as every engine |
| SMP | CLIP · START · LENGTH · DIRECTION (FWD, REVERSE, PING) · PITCH · LEVEL |
| MIX | MACHINE (level) · SAMPLE (level) · MODE · AMOUNT · DIRECTION · empty |

- The header reads `PART 1 • 02 SNARE` and the page: every page edits the selected pad. MIX + MINUS / PLUS steps the pad on DRUM pages (hint on the KIT band and on the PAD cell). MTX keeps MIX + MINUS for deleting a route; there the pad is picked on KIT.
- D-pad as everywhere: MINUS / PLUS along the chain, EDIT / SEQ the sub-pages.
- Optional: a CHAIN overview of the pad (mockup b) with one cell per box and EDIT opening its page.

## Machines
Families map the Machinedrum's: TONE (its analog-modelled family), FM (its FM family), BODY (its physical-model family), PLAIN (its ground family). Sample-only (its ROM family) is machine OFF. Knob sets are drafted from the manual's machine chapter as remembered; the research step reads the chapter and corrects them (behaviour and parameter descriptions only).

| Drum | MD structure | Our machine | Kernel / our parts | A–F | MORE | 0.4 |
|---|---|---|---|---|---|---|
| Kick | analog BD | TONE KICK | body: VA sine + exp sweep, NOISE click, SOFT | PITCH · DECAY · SWEEP · SWEEP TIME · CLICK · DRIVE | START · HARMONICS | yes |
| Kick | FM BD | FM KICK | fm: ALGO ops 2→1 + sweep | PITCH · DECAY · SWEEP · MOD · RATIO · MOD DECAY | SWEEP TIME · FEEDBACK | yes |
| Kick | physical BD | BODY KICK | membrane: MODAL mallet + bank | PITCH · DECAY · HARDNESS · MALLET · TENSION · DAMP | POSITION | yes |
| Snare | analog SD | TONE SNARE | body ×2 + NOISE through CLEAN BP | PITCH · DECAY · BUMP · SNAP · SNAP DECAY · TONE | INTERVAL · DRIVE | yes |
| Snare | FM SD | FM SNARE | fm + NOISE, CLEAN HP | PITCH · DECAY · NOISE · NOISE DECAY · MOD · RATIO | MOD DECAY · HIGH PASS | yes |
| Snare | physical SD | BODY SNARE | membrane + wire noise | PITCH · DECAY · HARDNESS · TENSION · WIRES · WIRE DECAY | DAMP | yes |
| Tom | analog tom | TONE TOM | body (tom ranges) | PITCH · DECAY · SWEEP · SWEEP TIME · NOISE · DAMP | — | yes |
| Tom | physical tom | BODY TOM | membrane | PITCH · DECAY · HARDNESS · TENSION · DAMP · POSITION | MALLET | yes |
| Clap | analog/FM clap | TONE CLAP | burst: NOISE bursts with jitter (ENV B BURST law) + BP tail | TONE · DECAY · CLAPS · SPREAD · TAIL · RING | — | yes |
| Rim | analog rim | TONE RIM | 2 pinged modes (MODAL bank) + click | PITCH · DECAY · TONE · CLICK · RING · NOISE | — | yes |
| Cowbell | analog CB | TONE COWBELL | 2 VA pulses + BP | PITCH · DECAY · INTERVAL · TONE · RING · DRIVE | — | yes |
| Hats | analog CH/OH | METAL HAT | metal: 6 squares once a Part, 2 BPs a voice | PITCH · DECAY · TONE · SPREAD · RING · NOISE | — | yes |
| Cymbal | analog/FM CY | TONE CYMBAL | metal + 3 BPs, slow shimmer | PITCH · DECAY · TONE · SPREAD · RING · SHIMMER | — | yes |
| Bell, gong | physical bell | BODY BELL | MODAL bank, bell ratios | PITCH · DECAY · HARDNESS · STRUCTURE · BRIGHT · DAMP | — | yes |
| Hats | FM HH | FM HAT | fm, 3 ops, high ratios | PITCH · DECAY · TREMOLO · TREMOLO RATE · MOD · FEEDBACK | RATIO · MOD DECAY | later |
| Shaker | physical maracas | BODY SHAKER | burst: random grains + BP | GRAINS · DECAY · GRAIN LENGTH · TONE · SHAKE · RING | — | later |
| Utility | ground SN/NS/IM | PLAIN SINE, NOISE, IMPULSE | VA osc, NOISE, a step | PITCH · DECAY · SWEEP · SWEEP TIME (NOISE: DECAY · TONE) | — | yes |
| Sample only | ROM | OFF | none | the SMP page | — | yes |

Six kernels (body, fm, membrane, metal, burst, plain) carry every machine; a machine is a law table (ranges, curves, defaults), which is what keeps flash small. The metal bank is the Hortator idea, built our way: one set of six squares a Part, every hat and cymbal voice filtering it.

## Voices
- **Note-on:** channel and note → pad (C0–B0 on any pad channel or the Part's; C1 up → that channel's pad, chromatic from its tuned pitch) → choke victims (one shared choke module, as ADR 0170 planned for TAPE's KIT) → allocator.
- **Allocator:** the one shared pool (8 today) with RESERVE: a Part with RESERVE n is never stolen below n voices, and others can't take its floor; unset is plain stealing. Steal order: released voices first, then the oldest of the lowest PRIORITY, declicked by the existing fade. RESERVE is a Part setting on every engine (non-DRUM Parts: PIT's empty F cell, to confirm).
- **Exclusive state (ADR 0051):** a voice becomes DRUM at the hit. The DRUM engine slot holds machine state (largest: membrane, 8 modes, ~200 B), the head (~48 B) and combine state (~16 B): well under the MODAL slot that sizes `EngineSlot`, so `Voice` does not grow (ADR 0101's 2,112 B chain unchanged).
- **Pad params at the hit:** the voice copies its pad's compact params (machine, sample, mix, filter, amp; ~200 B) and reads the Part's moving sources as today.

## Memory and flash
**Samples.** Clips are 12-bit packed mono, 1.5 B a sample (ADR 0093), resident in the 160 KiB arena beside Sounds and TAPE sources. One kit pool is one prepped source (ADR 0171's KIT prep, clips as slices), so a kit costs one of `MAX_SOURCES` (32), not 36.

| Rate | Bytes / ms | 1 KiB holds | 36 clips in 48 KiB |
|---|---|---|---|
| 24 kHz | 36 | 28 ms | 37 ms each |
| 16 kHz | 24 | 43 ms | 57 ms each |
| 12 kHz | 18 | 57 ms | 76 ms each |

The machine carries the body, so most clips are transients and textures of 20–150 ms. A worked kit: 3 × (kick 60, snare 120, clap 150, rim 30, 2 toms 80, closed 40, open 250, cymbal 300, bell 120, shaker 100, impact 150) ms ≈ 4.6 s of clips: 110 KiB at 16 kHz, too big; the same at 12 kHz with open hat, cymbal and impact single-variant ≈ 54 KiB. Proposal: a kit allowance of 48 KiB, import at 16 kHz by default (12 kHz offered), so two kits and a 50 K TAPE source fit together. 4-bit ADPCM would triple it (FORWARD only, ~2 KB flash): owner's call.

**RAM.** Voice: no growth (above). Kit Part: 12 compact pads ≈ 2.4 KB of live params where the Part's live Sound lives today, not D2 (3.6 KB free there). Full Sounds per pad (684 B each, 8.2 KB a Part) would not fit six kit Parts; see question 4. Metal bank: ~64 B a Part. Saved kit: 12 sparse Sounds (~0.3 K each) + a keymap of ~100 B in the arena.

**Flash** (estimate; bank 2 has ~2.5 KB, so per ADR 0251 this goes to bank 1, ~72 KB free):

| Piece | KB |
|---|---|
| Six kernels | 9 |
| Machine laws (17 tables) | 4 |
| Combine (reuses VA's; FOLD-into, FM-both-ways, direction) | 1.5 |
| Kit model, keymap, choke, MIDI routing, CLIP latch | 3.5 |
| RESERVE in the allocator | 1 |
| Pages and band drawings (KIT, KEYS, POOL, MCH, MORE, SMP, MIX) | 8 |
| Codec (kit, pad records) | 2 |
| **Total** | **~29** |

That leaves ~43 KB in bank 1, just over the ~40 KB line where flash gets escalated. The two "later" machines add ~1.5 KB. The bootloader slots stay the last resort.

**CPU** (estimate, cycles a sample a voice, against 7,500 at 75 %): machine 120 (TONE) to 300 (membrane); head 70; combine 10–40; the shared chain as billed today (~300 with CLEAN and the gate). A drum voice ~500–700, under an ALGO voice, so eight drum voices fit beside the bus. Billed per Sound by ADR 0249's rule once measured.

## Provenance
Machinedrum: manual descriptions only; MDX's text docs for parameter ranges as interface data (#214 2026-10-12); no firmware, no disassembly, no emulator recordings without the owner's own unit. Rytm: its manual for the architecture and the MIDI scheme. Hortator (GPL): ideas only, no code, tables or constants. Membrane mode ratios are textbook physics (Bessel zeros).

## Questions only the owner can answer
1. Seven chain nodes (KIT · MCH · SMP · MIX · FLT · AMP · MOD), or MIX folded under SMP for six?
2. Pad selection: MIX + MINUS / PLUS on DRUM pages, plus follow the last hit (Rytm) on by default?
3. Pad channels against Part channels (Parts default 1–6): pads OFF by default with only the Part's channel playing C0–B0 and C1 up? Pads on 10–16? A GM map too?
4. Pad modulation depth: compact pads (~200 B: one envelope, one LFO, four routes, the Part's LFOs shared) or full Sounds (8.2 KB a kit Part)?
5. Per-pad OUTPUT (individual outs, as the Rytm)?
6. The 0.4 ship list (16 machines marked yes); which family matters most?
7. Kit allowance 48 KiB and 16 kHz import by default? ADPCM worth ~2 KB flash?
8. May a pad hold a non-DRUM Sound (#215's first idea), or is every pad the DRUM shape (2026-10-26)?
9. Choke defaults from the Rytm's pairs (RIM/CLAP, toms, hats, CYMBAL/BELL as A–D), CUT within a pad?
10. RESERVE on non-DRUM Parts: PIT's F cell?
11. The envelope follower (2026-10-13) as a DRUM source for ducking: 0.4 or later?

## Next steps once answered
Research step (the manual's machine chapter; DAFx drum papers on the reading list), an ADR for the voice and the kit, then a plan. Host-first: each kernel on the desktop with goldens and A/B renders before the unit.
