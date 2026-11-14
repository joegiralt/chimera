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
- **TRANS:** BODY machines take a choice of exciter, and a synthesized TRANS layer for the other families is under audition (see TRANS below).
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
Families map the Machinedrum's, confirmed against its manual (Appendix A, Machine Reference, OS 1.63, Elektron's public PDF; read for behaviour and parameter descriptions only): TONE ≈ TRX ("classic analogue drum machine synthesis"), FM ≈ EFM ("Enhanced Feedback Modulation"), BODY ≈ P-I ("physically informed", acoustic drums), PLAIN ≈ GND (sine, noise, impulse). Sample-only is machine OFF: the MD's E12 (its sampled percussion, with START, RETRIG and BEND per machine) and ROM (UW user samples) both land on our sample layer. Every MD machine has up to eight parameters; ours have six (A–F) plus MORE's two, so the counts match. Host prototypes: `chimera-utils/src/drums/`, `just drum-render`.

Knob sets as checked (MD names in capitals as the manual prints them; ours are full words where they fit). MORE's two follow the bar.

| Drum | MD machine and its parameters | Our machine | Kernel | A–F \| MORE | Differences from Machinedrum | 0.4 |
|---|---|---|---|---|---|---|
| Kick | TRX-BD: PTCH DEC RAMP RDEC STRT NOIS HARM CLIP | TONE KICK | body | PITCH · DECAY · SWEEP · SWEEP TIME · CLICK · DRIVE \| START · HARMONICS | One to one (NOIS → CLICK, CLIP → DRIVE, our SOFT). START starts the sine up to a quarter cycle in; HARMONICS adds the 2nd and 3rd partials. TRX-B2's HOLD, TICK, DIRT not taken (DIRT is the FX slot's BITS) | yes |
| Kick | EFM-BD: PTCH DEC RAMP RDEC MOD MFRQ MDEC MFB | FM KICK | fm | PITCH · DECAY · SWEEP · MOD · RATIO · MOD DECAY \| SWEEP TIME · FEEDBACK | One to one. MFRQ is RATIO, stepped over eight ratios (0.5–7) rather than free | yes |
| Kick | PI-BD: PTCH DEC HARD HAMR TENS DAMP | BODY KICK | membrane | PITCH · DECAY · HARDNESS · MALLET · TENSION · DAMP \| POSITION | One to one (HAMR → MALLET, higher softer as the MD's); POSITION is ours | yes |
| Snare | TRX-SD: PTCH DEC BUMP BENV SNAP TONE TUNE CLIP | TONE SNARE | body | PITCH · DECAY · BUMP · SNAP · SNAP DECAY · TONE \| INTERVAL · DRIVE | BENV (the bump's length) fixed at 8 ms; we spend the knob on SNAP DECAY, the noise's own length, which the 808-style snare needs. TUNE → INTERVAL (the second head, 1.2–1.77×), CLIP → DRIVE | yes |
| Snare | EFM-SD: PTCH DEC NOISE NDEC MOD MFRQ MDEC HPF | FM SNARE | fm | PITCH · DECAY · NOISE · NOISE DECAY · MOD · RATIO \| MOD DECAY · HIGH PASS | One to one (HPF on the noise) | yes |
| Snare | PI-SD: PTCH DEC HARD TENS RVOL RDEC RING | BODY SNARE | membrane | PITCH · DECAY · HARDNESS · TENSION · WIRES · WIRE DECAY \| RING · DAMP | One to one (RVOL/RDEC → WIRES/WIRE DECAY; RING lets the upper modes ring nearer the fundamental's length); DAMP is ours | yes |
| Tom | TRX-XT: PTCH DEC RAMP RDEC DAMP DIST DTYP | TONE TOM | body | PITCH · DECAY · SWEEP · SWEEP TIME · NOISE · DAMP \| DRIVE · DRIVE TYPE | One to one (DIST/DTYP → DRIVE/DRIVE TYPE: SOFT leaning to FOLD); NOISE is ours. TRX-XC (congas) has the same set: BODY HAND covers it | yes |
| Tom | PI-XT: PTCH DEC HARD HAMR TUNE DAMP SIZE POS | BODY TOM | membrane | PITCH · DECAY · HARDNESS · TENSION · DAMP · POSITION \| MALLET · SIZE | Eight for eight, but the MD's TUNE (the two skins' tuning) is our TENSION (the hit's pitch drop); SIZE sets the shell's mode stretch and ring length | yes |
| Clap | TRX-CP: CLPY TONE HARD RICH RATE ROOM RSIZ RTUN | TONE CLAP | burst | TONE · DECAY · CLAPS · SPREAD · TAIL · RING \| HARDNESS · ROOM TONE | CLPY → CLAPS, RATE → SPREAD, ROOM → TAIL, RSIZ → DECAY, RTUN → ROOM TONE (a low-pass on the tail alone), HARD → HARDNESS (each clap's fall). RICH folded into RING. EFM-CP not taken | yes |
| Rim | TRX-RS: PTCH DEC DIST; PI-RS: PTCH DEC HARD RING RVOL RDEC | TONE RIM | body | PITCH · DECAY · TONE · CLICK · RING · NOISE | **Cut** by ear (2026-11-10); the code stays (model work). BODY WOOD's rim and sidestick take its place | cut |
| Rim, clave, block | TRX-CL: PTCH DEC DUAL ENH TUNE CLIC | BODY WOOD | membrane (WOOD material) | PITCH · DECAY · HARDNESS · MATERIAL · RING · CLICK \| DUAL · TUNE | Added scope 4. MATERIAL glides rim → sidestick → clave (the free bar's modes) → woodblock; ENH → RING (the second mode's ring); DUAL a second strike 2–30 ms on; TUNE the second mode | yes |
| Hand drums | TRX-XC; E12-BC (BC: bongo ↔ conga) | BODY HAND | membrane + HAND exciter | PITCH · DECAY · HAND · SHELL · STROKE · TENSION \| BRIGHT · POSITION | Added scope 4. SHELL bongo → conga → timbale (the steel shell's two ringing modes); STROKE OPEN, SLAP, MUTE in three zones. HAND (owner, 2026-11-10: "missing that hand excitement") crossfades the mallet into a hand: a palm's raised-cosine contact (3–6.5 ms OPEN/MUTE, 0.6–1 ms SLAP), skin noise band-passed 1–5 kHz for 2–6 ms, SLAP's 1–2 ms high-passed crack and a 1–3 ms finger flam on some slaps, fingers (SLAP) or the palm (MUTE) damping the modes after contact, a strike bend settling in 20–40 ms, contact and noise colour drawn afresh each hit; HAND 0 is the mallet. BRIGHT is the mallet's hardness and the slap's brightness. No MD equivalent of the strokes | yes |
| Cowbell | TRX-CB: PTCH DEC ENH DAMP TONE BUMP | TONE COWBELL | metal | PITCH · DECAY · INTERVAL · TONE · RING · DRIVE \| DAMP · BUMP | ENH → RING, DAMP (the fast decay's share) and BUMP (an octave pitch blip, 6 ms) on MORE; INTERVAL and DRIVE are ours | yes |
| Hats | TRX-CH/OH: GAP DEC HPF LPF MTAL | METAL HAT | metal | PITCH · DECAY · TONE · SPREAD · RING · NOISE \| HIGH PASS · LOW PASS | GAP → SPREAD, MTAL → RING; HPF/LPF on MORE beside our TONE (the band-passes' centre). One machine for closed and open (DECAY) | yes |
| Hats | EFM-HH: PTCH DEC TREM TFRQ MOD MFRQ MDEC FB | FM HAT | fm | PITCH · DECAY · TREMOLO · TREMOLO RATE · MOD · FEEDBACK \| RATIO · MOD DECAY | One to one | yes |
| Cymbal | TRX-CY: RICH DEC TOP TTUN SIZE PEAK | TONE CYMBAL | metal | PITCH · DECAY · TONE · SPREAD · RING · SHIMMER \| TOP · TOP TUNE | SIZE → PITCH, RICH → SPREAD, PEAK → RING, TOP/TTUN → TOP/TOP TUNE (the top band-pass's level and place); SHIMMER is ours. EFM-CY, PI-RC, PI-CC not taken | yes |
| Bell, gong | PI-ML (metallica): PTCH DEC HARD TENS | BODY BELL | membrane | PITCH · DECAY · HARDNESS · STRUCTURE · BRIGHT · DAMP \| TENSION | MD's four plus our STRUCTURE (mode stretch), BRIGHT and DAMP | yes |
| Tambourine | E12-TA (sampled): PTCH DEC HP HPQ STRT RTRG RTIM BEND | METAL JINGLE | metal + burst | PITCH · DECAY · JINGLES · SHAKE · TONE · RING \| HEAD · SPREAD | Added scope 5. Synthesised where the MD's is a sample: the metal bank gated by SHAKER's particles; HEAD a frame drum's thump. RETRIG is the pad's (added scope 2), not the machine's | yes |
| Shaker | PI-MA: DEC GRNS GLEN SIZE HARD; TRX-MA: ATT SUS REV DAMP RATL RTYP TONE HARD | BODY SHAKER | burst | GRAINS · DECAY · GRAIN LENGTH · TONE · SHAKE · RING \| HARDNESS | PI-MA one to one (SIZE → TONE), plus SHAKE (the gesture's peak) and RING | yes |
| Utility | GND-SN: PTCH DEC RAMP RDEC | PLAIN SINE | plain | PITCH · DECAY · SWEEP · SWEEP TIME | One to one | yes |
| Utility | GND-NS: DEC | PLAIN NOISE | plain | DECAY · TONE | Plus TONE (low-pass below ½, high-pass above) | yes |
| Utility | GND-IM: UP UVAL DOWN DVAL | PLAIN IMPULSE | plain | PITCH · DECAY · SWEEP · SWEEP TIME \| WIDTH | Different: the MD's is one bipolar step (times and levels of its two halves); ours a pitched, swept narrow-pulse train the owner approved. WIDTH (MORE) is the nearest of the MD's set: the positive half's share | yes |
| Sample only | E12, ROM | OFF | none | the SMP page | E12's START, RETRIG, BEND live on SMP and the pad | yes |

Not taken from the MD: E12's per-machine RTRG/RTIM (our RETRIG is per pad), INP (external input) and MID/CTR machines (MIDI-sending pads are out of 0.4), EFM-RS/CB/CP/CY and the P-I cymbals.

**Fixes before porting** (measured by `just drum-render`, numbers in `manifest.json` → `fixes`; renders `fix-*_before.wav` / `_after.wav`):
- **FM sweep:** ALGO's kernel holds `inc` a block, so FM KICK's sweep was a 750 Hz staircase (largest step 84 Hz at the extreme setting). DRUM's FM path is its own small loop on ALGO's operator law (PM_CYCLES, feedback over two samples, gain ramps) with `inc` ramped across the block: the largest per-sample move is 1.3 Hz, and the energy above 650 Hz in the sweep's first 30 ms (where the staircase's sidebands land) falls from −49 to −57 dB (extreme), −58 to −74 (mid), −62 to −78 (classic). With SWEEP at 0 the new loop nulls against ALGO's kernel at −52 dB (classic) to −38 dB (ratio 7, full MOD; the kernel's table against our sine). ALGO is untouched.
- **BODY balance:** MODAL's bank (unnormalised band-passes, Q rising with the mode's pitch) let the upper modes lead. BODY now rings its own 8-mode bank: each mode impulse-normalised, amplitude `ratio^−slope` (HARDNESS, BRIGHT) times the strike position's weight, T60 `ratio^−tilt` (DAMP), struck by a half-sine mallet pulse (MALLET, HARDNESS) with a little of MODAL's burst noise; MODAL's STRUCTURE law keeps the mode tuning. BODY KICK extreme: centroid 102 → 44 Hz, strongest partial 75 → 40 Hz (f0 39), fundamental's share −8.5 → −0.4 dB. BODY BELL mid / extreme: centroid 1370 → 786 Hz / 6502 → 1109 Hz, strongest 4803 → 215 Hz at extreme. BODY SNARE classic: centroid 15.2 k → 503 Hz; BODY TOM extreme: 541 → 89 Hz. MODAL's bank is untouched.
- **4-bit ADPCM attack:** the encoder now picks the start step index that codes the clip's first 32 ms best (kept in the 4-byte header) and chooses each code with a one-sample look-ahead; the decoder is IMA's, unchanged. CLICK: peak −5.2 → +0.1 dB, first-2 ms SNR 2 → 20 dB; SNARE HIT: first-2 ms SNR 14 → 34 dB; NOISE BURST 16 → 20 dB.

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

## Owner rulings, 2026-11-10

1. **Chain map:** seven nodes, KIT · MCH · SMP · MIX · FLT · AMP · MOD.
2. **Picking the pad:** MIX + MINUS / PLUS steps the pad on every DRUM page, and the pad also follows the last hit.
3. **Pad channels:** collisions with Part channels are fine at first. No special default.
4. **Pad depth** (owner asked for a recommendation; taken unless he says otherwise): compact pads, as on the Rytm. Each pad has its machine, sample, mix, filter, amp, one mod envelope, one LFO and four routes, about 200 B. The Part's three LFOs are shared by every pad. A full Sound per pad would be 8.2 KB a kit Part, and RAM can't spare that.
5. **Per-pad output** (recommendation, taken unless he says otherwise): yes. OUTPUT is PART (default), 1, 2 or 3, as on the Rytm's individual outs. The pair buses already exist for Parts, so the cost is the routing, not new buses.
6. **Ship list:** decided by ear. Render each machine on the host for the owner to hear first.
7. **Samples:** they must be very short or low-bit. Default import is short clips at a low rate, and a low-bit mode (4-bit ADPCM or 8-bit) is in scope.
8. **Pads hold only DRUM Sounds** for now. No ALGO or MODAL Sound on a pad.
9. **Choke defaults:** the Rytm's pairs as groups A–D (RIM/CLAP, the two toms, the two hats, CYMBAL/COWBELL). CUT within a pad, STACK across pads.
10. **RESERVE** lives in PIT's empty F cell on non-DRUM Parts.
11. **Envelope follower:** belongs to #224 (cross-Part modulation: a Part's output level as another's mod source), not to DRUM.

## Ship list, owner by ear, 2026-11-10

From the audition page (host prototypes at 2cacaeae): **18 machines ship in 0.4**. TONE KICK, TONE SNARE, TONE TOM, TONE CLAP, TONE COWBELL, TONE CYMBAL, METAL HAT, FM KICK, FM SNARE, FM HAT, BODY KICK, BODY SNARE, BODY TOM, BODY BELL, BODY SHAKER, PLAIN SINE, PLAIN NOISE, PLAIN IMPULSE. **TONE RIM is cut.** Owner: "these sound great. ship."

Before porting, fix the prototype issues the measurements flagged: FM KICK's stepped sweep (pitch per block, needs per-sample pitch in the FM kernel or a smoothed sweep), BODY's too-bright upper modes, and the 4-bit click's lost attack. Then check every knob set against the Machinedrum manual's machine chapter.

## Added scope, owner 2026-11-10 ("add 1 through 6")

1. **ACCENT / velocity routing per pad.** Velocity drives two or three of the pad's knobs (e.g. DECAY, TONE, PITCH), not only level, after the Machinedrum's ACCENT and the Rytm's velocity mod.
2. **RETRIG per pad.** Rolls and flams from a single note, after the Rytm's RETRIG: RATE, LENGTH, and a velocity ramp (up or down). It works over MIDI without a sequencer.
3. **Kit macros.** Kit-wide knobs that scale every pad's DECAY, shift every pad's TUNE and set an overall DRIVE, for live performance; routable like any mod destination.
4. **WOOD and hand percussion.** A WOOD material on the BODY kernel (rim, sidestick, clave, woodblock), filling the gap TONE RIM left, and hand drums (conga, bongo, timbale) as BODY TOM plus an articulation knob (OPEN, SLAP, MUTE).
5. **Tambourine and jingles.** The shared METAL bank with SHAKER's particles.
6. **Pads modulate other pads.** One pad's envelope, level or LFO as a mod source on another pad of the same kit (a kick ducking the hats, an envelope opening a filter), after the Machinedrum's control machines and track LFOs aimed at other tracks. It shares its mechanism with #224 (cross-Part modulation, which also holds the envelope follower).

Not in 0.4: MIDI-sending pads (7) and the listening drummer (8).

**BODY WOOD ships** (owner by ear, 2026-11-10: "the new wood sounds good"). BODY HAND and METAL JINGLE await marks.

**BODY HAND and METAL JINGLE ship** (owner, by ear on the audition page, 2026-11-10/11). VOX was rebuilt consonant-first (3f46a47b) after the owner said the formant version "doesn't sound like a mouth"; its ship marks are pending.

**Audition marks, owner 2026-11-11** (audition page, collection `picks`): the world batch ships: BODY BAR, BODY PAN, BODY GONG, BODY TABLA, LPG BONGO, FILTER PING. VOX ships **ROLL only**; VOX KICK, SNARE, HAT and CLICK are cut (the code stays, model work). BODY SPRING has no mark. With the 18 of 2026-11-10 and BODY WOOD, BODY HAND, METAL JINGLE, **28 machines ship in 0.4**.

## Glyphs and icons, owner 2026-11-12

Mockup: an unpublished page, source `docs/mockups/drum-glyphs.html` (the bitmaps in `INST`, `FAM` and `L3` are the reference pixels).
- **KIT icons land "for the time being":** every machine is a composite of a family mark (TONE, FM, BODY, METAL, PLAIN, LPG, VOX) and a 14×12 instrument mark (22 marks cover the 28 machines); `#` full colour, `+` the dim second tone. Snare and tom are flat side views (short with crossed sticks and wires; tall floor tom on legs). Badge style (3×5 letter or 5×5 symbol) is still open.
- **Focus-band glyphs:** eight gauges (SWEEP, DECAY, TRANSIENT, PARTIALS, CLAPS, GRAINS, DRIVE, FILTER). Per knob or one whole-hit picture: owner "it depends", to be settled per machine family before Task 8.


## Owner, 2026-11-13: slice order and kit playing
- **Slice 2 = per-pad FILTER and AMP** (ahead of the FM and BODY machines): each pad its own filter (TYPE, CUTOFF, RESO…), amp envelope, a mod envelope, an LFO and 4 routes (the compact Rytm-style pad); the Part's three LFOs stay shared. FLT/AMP follow the selected pad as MCH does. Pad data in ITCM.
- **TONAL** on the KIT page (OFF / pad 1–12): C0–B0 trigger the kit; from C1 up the TONAL pad plays chromatically with its tuned pitch at middle C (MIDI 60), "principle of least surprise". TONAL OFF: the kit repeats in every octave. A TONAL 2 split is a later option.
- **PAD FOLLOW** (SETTINGS › PERSONALIZE): OFF / IDLE (default) / ALWAYS. **Pads stay lit while they sound.**

## TRANS: exciters for BODY, maybe a layer for the rest (owner 2026-11-14)
Owner: a transient shouldn't cost a sample, and a recorded click cross-modulating with the machine can be weird. Then: "body no… the others… maybe".

- **BODY machines (membrane, wood, bell, hand): TRANS is the EXCITER**, fed into the mode bank, not layered on top. A layer on a resonator gives two sounds glued together; exciting the resonator gives one drum whose hit has character (Elements' exciter → resonator). This replaces BODY's hard-wired half-sine mallet pulse and BODY HAND's hand exciter with a choice. BODY gets no TRANS layer.
- **Exciter TYPEs:**
  - MALLET: a soft half-sine contact (exists) — felt beater, tom, timpani.
  - STICK: a hard, very short click — snare crack, rimshot, woodblock.
  - HAND: palm contact plus skin noise (exists, BODY HAND) — conga, bongo, tabla.
  - BRUSH: a smeared noise burst — brushed snare.
  - FLAM: two strikes a few ms apart (WOOD's DUAL exists) — flams, ruffs.
  - NOISE: a filtered noise burst (MODAL's exciters) — noisy, gated, electronic hits.
  - SCRAPE: rough noise grains over tens of ms — guiro, cymbal scrape.
- **Exciter knobs:** TYPE plus what each type needs (HARDNESS, DECAY, TONE); BODY's existing HARDNESS and MALLET knobs map onto them. Each machine keeps its current exciter as its default, so today's sounds don't change.
- **TONE, FM, METAL and PLAIN machines: a TRANS layer, maybe.** The idea: the same generators layered beside the machine through the layer's COMBINE (MIX before the filter; FM moving the machine's phase at the attack; RING, AM, FOLD-into), as SAMPLE is (the layer's SOURCE: TRANS or SAMPLE). **Undecided: the owner wants to hear it first.** Build a host prototype (`chimera-utils/src/drums/`, `just drum-render`) of a few machines with and without a TRANS layer before it goes into the spec as a feature.
- **Cost:** an exciter or a layer runs only for its own few milliseconds and is billed only while it sounds, as the strike is.
- **Open:** whether the machines' own CLICK/SNAP/BUMP knobs stay beside a TRANS layer; whether exciter TYPE and HARDNESS get pad mod destinations.

## Physical additions, order after research round 2 (owner 2026-11-14)
Sources: `(private notes) physical modelling models.md` (round 1) and `Physical models round 2.md` (round 2). The owner's test for every candidate: **does the existing pad chain already get there?** Machine → FILTER → FX slot (SOFT, FUZZ, FOLD) → AMP with the LPG (STRIKE, DECAY) plus VEL already gives "a soft hit is darker", "darker as it dies" and "brighter when driven". A model earns its cycles only for what the chain can't do: brightness that rises after the hit, pitch that follows the hit's energy, self-oscillation, rattle driven by the head, timing within one hit. Every cost below is an operation-count estimate, not measured.

1. **ENERGY → PITCH.** A pad mod source ENERGY: a one-pole leaky integrator of hit energy (Bank, DAFx-09, "energy storage model"), with a new **PITCH** pad route destination (today's `RouteDest` has CUTOFF, RESO, VOLUME, LFO SPEED, LFO DEPTH). Every machine gets a physical pitch drop that follows how hard and how often it's hit; rolls and flams climb in pitch. ~1–3 cycles a sample.
2. **STICK + BOUNCE exciters, with the mallet law.** STICK with a geometric bounce train for press rolls and buzz (~5 cycles). The Hunt–Crossley mallet is **reduced to its law**: the strike pulse shortens as velocity rises (≈ v^−0.2 Hertzian, v^−0.43 felt), since the LPG and VEL already do its main audible job. If a true contact is ever wanted, use the DAFx-24 QSA scheme (CC-BY, no iterations), never an open-ended solve.
3. **BLOOM** (Poirot coupled mode bank): the only block that makes a sound brighter after the hit, and only past a threshold. A slow envelope into CUTOFF fakes the swell but can't add partials the source lacks. Knobs, cost and risk as below.
4. **A head-gated snare rattle**: a cheap rattle gated by the head's motion (no new real-time papers since round 1); before any collision model.
5. **SCRAPE** (DAFx-21 scraping model; its tanh-limited trajectory is the main realism factor). **BRUSH** is a SCRAPE preset: there is no physical brush model in the literature.
6. **Experimental:** a Puckette-style delay-network drum as a character machine, host prototype first.

**Ranked down for DRUM:** PIPE (a struck bottle or udu is a pinged resonance, which FILTER PING already is; PIPE's value is sustained blowing in MODAL, #421) and HARMONIC as a drum source (overlaps TONE, METAL and BODY at more cost; a ratio-table module is for melodic bells).

**Licences:** use the papers' equations; avoid the code of jaxdiffmodal (GPL-3.0), the Russo bowed-string repo (no licence), SDT (GPLv3) and the Schmalfuß snare repo (unchecked). Patent search still owed before release.

**Recipes first:** round 2 lists ten drum patches built only from today's machines and chain (vactrol tom, bottle/udu, a kick that brightens with force via FOLD before the filter, fake-bloom gong, brush swirl, wire snare via FUZZ, agogo, sheet-metal thunder, MODAL wood, bowed bowl). Render them on the host for the owner before building any of the above.

### BLOOM and the mallet: detail (from round 1; the mallet is reduced by item 2 above)
From the physical-modelling survey (private notes physical modelling models.md`, ranks 6 and 4). Both are cheap operations on the modal bank BODY already has, not new engines, and both serve MODAL too.

- **BLOOM: a coupled mode bank** (Poirot, Kronland-Martinet & Bilbao, DAFx 2023, CC-BY; not the reverb's BLOOM). Energy moves between modes by t = M·[p − τ]+: only above a threshold τ, so it blooms only when hit hard. Knobs: BLOOM (λ, how fast energy moves), THRESHOLD (τ), SPREAD (Δf, how far up it cascades), CHAOS (random-phase injection, shimmer). Stable when M's column sums are ≤ 0. Sounds: a gong that swells after the hit, shimmer only on hard hits, a cymbal body, sitar/tanpura buzz (the collision variant). For BODY GONG, BELL, PAN, BAR and a cymbal body. Cost about 50–60 cycles a sample with transfers once a block (estimate; K ≈ 4–8 neighbours, 48 modes). **Risk:** the paper ran transfers per sample; block-rate transfers must pass an ear test (the "never steppy" rule) on the host first, then the unit.
- **Hunt–Crossley mallet** (Avanzini & Marogna, TASLP 2010; Stulov felt): contact force F = kζ^α + λζ^α·ζ̇, so contact time follows velocity and **a soft hit is darker, not just quieter**. It becomes the MALLET and STICK exciters' law (see TRANS: exciters for BODY), with HARDNESS and MASS. The contact is a delay-free loop: solve it with a precomputed table or a fixed 1–3-step iteration, never an open-ended loop (the audio thread never blocks); clamp it and check by ear (Papetti et al., TASLP 2011). Cost negligible on average: it runs 1–5 ms a hit.
- **Provenance:** papers only (CC-BY or equations); avoid the Sound Design Toolkit's code (GPLv3). Patent search still owed before release (the survey's note).
- **Order:** host prototypes in `chimera-utils/src/drums/` with `just drum-render` renders for the owner's ear first; then the unit's cycle count.
