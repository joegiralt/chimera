# 0294. AMP › STEREO places each note and widens the Part

- **Status:** Proposed
- **Deciders:** owner (2026-11-19, tester request chimera-releases#14, the design's recommendations A and C; the page and DRUM's cell the owner's), firmware

## Context
A tester wants stereo motion saved in the patch: an LFO moving a pad, an envelope nudging a pluck. They asked, in order, for one internal pan destination, later for per-carrier pan in ALGO (the stock firmware has it), and for a per-Part width on signals that are already stereo, knowing a width can't make mono stereo (design: (private notes), #14).

What the code had (37fc068b9, on origin/0.4 b136ef37):
- Sound OUT PAN (OUT param 1, −1..1, ident `PAN`, NRPN 18:1) was stored but read by nothing (`modulatable = false`, `Smoothing::Unheard`); only the DEMO glyph pages showed it.
- A Part's bus is mono. Stereo existed only as a per-Part side (`Sides`), written by DRUM pads off centre (`MidSide::split`, its gains `sinf`'d once a hit), mixed as left = mid − side, right = mid + side at the Part's own PAN and LEVEL; the sends and the scope take the mid. Every voice already received its Part's side buffer (`Voice::render_knobs`). Part EQ processes the sides (ADR 0284); #15's meters read them (ADR 0293).
- Part PAN's law is constant power, two soft-float `sinf` cached per Part (`instrument::pan_gains`).

The owner decided on 2026-11-19: PAN and WIDTH on a new AMP › STEREO sub-page; the same WIDTH cell also on DRUM's AMP › GATE, so DRUM Parts get it; carrier PAN (the design's D) is not in 0.4. The previous builder's engine plan was accepted: OUT PAN a per-voice destination through DRUM's mid/side path, WIDTH an eased gain on each Part's side in a bank-2 Part stage before the meters, a polynomial pan law, bit-identical at centre and at WIDTH's default.

## Decision
1. **OUT PAN places each note.** It becomes a knob (`Smoothing::KNOB`, 30 ms, one of the Part's `KNOBS`) and a destination (`modulatable`, offset law linear: a full route is the whole −1..1). After its block renders, a voice whose played PAN (the knob's value, its routes on) is off centre at the block's start or end splits its block into the Part's mid and side, ramped across the block from the last block's place to this one's (`stereo::kern::pan`), exactly as a pad's `MidSide::split` does. A fresh note's first block starts on its own place (no sweep from the voice's last note: `Voice::volume`'s ramp is unprimed there); a fading voice keeps the place it last played.
2. **The law is the Part PAN's constant power, by polynomial.** `VoicePan::gains`: mid = cos θ, side = sin θ, θ = PAN · π/4 (so left = √2·sin((1 − PAN)·π/4), right = √2·sin((1 + PAN)·π/4), the pad's and the Part's law), each a Taylor polynomial to θ⁷ and θ⁸ (|θ| ≤ π/4: within 4e-7 and 3e-8, unit-tested against `sinf` across the knob). No `sinf` a block. At the centre the gains are exactly (1, 0).
3. **Centre is the voice as it was, bit for bit.** Centred at both ends of a block, the voice makes no call and writes no side.
4. **OUT PAN applies to ALGO, MODAL, VA and TAPE, not DRUM.** My call. A DRUM hit is placed by its pad's PAN (per pad, set at the strike, routable by the pad's own routes); DRUM has no AMP › STEREO page (the owner put only WIDTH on its GATE page), so a Sound PAN there would be a hidden second pan nobody can see. The match over `EngineSlot` in `Voice::out_pan` has no wildcard. When carrier PAN comes (post-0.4), OUT PAN stays: it places the whole voice, the carriers inside it (the design's question 4; to be confirmed with that ADR).
5. **WIDTH is the Part's side's gain: 0 to 2, unity (64 on the knob) the default.** My call: above unity. A new OUT param (id 10, ident `WIDTH`, label WIDTH, `ValFmt::Uni` over 0..2 on the 1/128 grid, so 0 is mono, 64 exactly unity, 127 just under twice), read at the Part's stage as the sends are, not modulatable (ADR 0010: the voice doesn't read it), eased by its own stage (`Smoothing::Own`). The reasons for going past unity:
   - Chimera's sides come only from panning (pads, OUT PAN), so a WIDTH that stops at unity can only narrow; past unity it spreads a kit or a NOTE-spread patch wider than its pans, past the outputs, which a pan alone can't.
   - It never changes the mono sum: left + right is twice the mid whatever WIDTH is, so a mono fold-down is unchanged at every setting (tested).
   - A DAW's width (Ableton's Utility, recalled from its manual, not checked here) also goes past 100 %. Twice is where I stop: further, a hard-panned pad's far side carries more anti-phase than its near side's level, and the near side is +3.5 dB over a hard pan already (the master limiter holds it).
   - Unity at the knob's middle keeps one law for the whole travel (no knee) and a readout already in bank 1 (`Uni`); a percent readout would be a new format in frozen bank 1.
6. **The WIDTH stage.** Each block, after the Parts' EQ and before #15's meters (so the meters read what is heard), `stereo::kern::widths` eases each Part's WIDTH toward the stored one (one-pole, 20 ms, `ease_coeff`, landing within 1e-5) and ramps its side across the block. A Part with no side this block lands on its WIDTH unheard. Its state is `PartWidths` (24 B, the Instrument's).
7. **Unity is the mix as it was, bit for bit.** With every Part's WIDTH unity last block and this, the stage returns at once and touches no side. The check is bank 2's: bank 1's stub (`Instrument::part_widths`) is the door call alone, a call a block (measured as a 290 B stub with the check in bank 1).
8. **The pages.** AMP › STEREO (`block_registry::STEREO`, id 120): PAN · WIDTH, C–F empty; AMP's sub-pages are STEREO, then SENDS (the voice's page, then the mix's, as DRUM's GATE then SENDS). DRUM's AMP › GATE takes WIDTH in F. The map reads STEREO. PAN reads C, L64..R63 as the Part's PAN does.
9. **Each cell's words** (`ui::notes::cell`, bank 2's): PAN `PLACES EACH NOTE, ROUTABLE`; WIDTH `0 MONO, 64 AS PANNED, 127 WIDE`, on both pages. Both fit the band (tested). PAN's routing gesture is the matrix's own MIX + PLUS priming, hinted there.
10. **Bank 2.** `stereo::kern` (the note's split and the Parts' WIDTHs) is an S4 group (`*6stereo4kern*`), reached through `stereo::door::StereoDoor` (magic 0xB2E0_0294, two entries) at `STEREO_DOOR_ADDR` = 0x08180430, a stride after the meters' (`s4_tables.rs`, `bank2.x`'s placement and assert, the boot's `door_ok`); `check-door-placed` pins it. A closed door plays every note centred and every side as panned, and rests the WIDTHs. Bank 1 keeps the stubs (`Voice::out_pan`, `Instrument::part_widths`), WIDTH's spec and arms, PAN's knob row, the page's `BlockDef`, GATE's cell and the map's row: closed-door data (ADR 0198).
11. **Not billed.** The split (about one multiply-add pair a sample a panned voice) is not added to `ModRouting::cost`, as ADR 0292's BASE fill isn't; the WIDTH stage runs on the mix's reserve.

## Alternatives considered
- **Part PAN as a Part-level destination (the design's B):** one LFO would move every voice of the Part, and a Sound's route would reach a mixer setting. Skipped, as the design advised.
- **Carrier PAN (D):** the stock-parity item; post-0.4, the owner's call.
- **WIDTH 0..1 (mono to as panned):** only narrows; see decision 5.
- **WIDTH folded into `mix_sided`'s side gains (the design's C, no per-sample cost):** the meters (ADR 0293) would then read the side before WIDTH, not what is heard; the owner accepted the stage before the meters.
- **OUT PAN on DRUM, summed with the pad's angle:** see decision 4.
- **A 65-entry quarter-sine table:** 260 B of table against two short polynomials; the polynomials are exact at the centre and need no interpolation.
- **PAN without a knob (`Smoothing::Own`, the voice's block ramp alone):** a turn would move in 1.3 ms ramps a detent; every other heard AMP cell glides 30 ms.

## Consequences
- CPU: none at centre (no call); at unity one door call a block and the stage's rest check. See Measurements for the host bench.
- RAM: the Instrument's `PartWidths` 24 B; one knob a Part (`Knobs`, its publisher); 4 B in every `ParamSnapshot` (each voice's played copy, so `VOICE_CHAIN_BYTES` 2,464 → 2,472).
- Sound: a note off centre sends its mid to the effects, as a pad does: at hard pan, 3 dB less into the sends than centred (constant power). DRUM unchanged.
- MIDI and files: WIDTH is NRPN 18:10 and a Sound field; a Sound saved before it loads at unity. PAN's NRPN, ident and disk code are unchanged.
- Docs: `docs/chimera-sitemap.md` (the chain diagram and the STEREO row) and `docs/parity.md`. New annotated screenshots for the tester guide: AMP › STEREO (PAN, WIDTH, the band's words) and DRUM's AMP › GATE (WIDTH in F); the map's AMP rows.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, 37fc068b9 against this change (its three commits alone, without the review fixes that ride beside it), each set built in both trees. Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| orbit | +380 | +2,624 |
| default | +1,060 | +2,720 |
| bank2-gate | +1,020 | +2,784 |
| orbit,usb-config-writer | +396 |  |
| fault-inject | +700 |  |
| bench-algo | +940 |  |
| bench-algo,master-tape | +956 |  |
| bench-routing | +860 |  |
| bench-memory | +748 |  |
| bench-stress | +988 |  |
| bench-orbit | +460 |  |
| bench-tape | +844 |  |
| filter-probes | +660 |  |

Bank 1's growth on ORBIT (+380): `OUT_SPECS`' WIDTH (72 B), `STEREO` (96 B), `Voice::render_knobs`' OUT PAN stub (the engine match, the two places and the door call), `Instrument::part_widths` (96 B, the door call alone: with the rest check in bank 1 it was 290 B), PAN's row in `KNOBS`, WIDTH's get/write arms and the map's row. The non-ORBIT sets grow more where the other engines' code inlines differently around `render_knobs`. Bank 2: `stereo::kern::widths` 1,560 B and `stereo::kern::pan` 984 B (both unrolled for speed: audio), the notes' two words, in S4. ORBIT bank 1 has 16,732 B free after this and the review fixes (917,504 less 900,772).

RAM: bench-algo's D2 overflowed by 96 B (PAN's knob a Part, the WIDTHs' 24 B, each voice's played WIDTH): its bench text gives 128 B (`BENCH_TEXT_LEN` 5,440 → 5,312), as ADRs 0292 and 0293 did.

CPU, host, release, best of five runs of 20,000 blocks of the whole instrument holding one VA note (`stereo_test::host_timing`): centred 4,719 ns a block; a still PAN of 0.5 4,799 ns (+80 ns, +1.7 %, the split); PAN moving every block 5,225 ns (+506 ns: the split's ramp and PAN's knob gliding every block); PAN 0.5 with WIDTH 0.5 within the noise of PAN alone (a 64-sample multiply). Not read on the unit.

Null: centre and unity make no change to any sample. `golden_test` (every factory Sound and case, voice and instrument paths), the reverb, VA, MODAL, TAPE and DRUM goldens and `vca_lpg_test`'s hash are unchanged; `stereo_test::centre_and_unity_are_the_mix_as_it_was` holds a centred note's left and right equal at WIDTH 1 and 2.

## Sources
- Design: (private notes) § #14.
- Tester request: chimera-releases#14.
- Code: `chimera-engine/src/stereo/`, `dsp/voice.rs` (`Voice::out_pan`), `instrument.rs` (`Instrument::part_widths`), `params.rs` (OUT PAN, WIDTH), `dsp/drum/engine.rs` (`MidSide`, the law), `block_registry::STEREO`, `DRUM_GATE`.
- Digitakt and Analog Rytm manuals, AMP PAN as an LFO destination (from memory, per the design's caveat).
