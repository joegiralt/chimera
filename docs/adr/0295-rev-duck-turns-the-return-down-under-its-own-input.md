# 0295. REV › DUCK turns the reverb's return down under its own input

- **Status:** Proposed
- **Deciders:** owner (2026-11-19, tester request chimera-releases#12, the design's recommendation A), firmware

## Context
A tester wants a big reverb on FM plucks without smearing the attack. They asked for a duck AMOUNT and RELEASE on REV: the reverb's own input turns down only the wet return while it is strong, and the tail comes back in the gaps; AMOUNT 0 keeps today's behaviour. They cite FabFilter Pro-R 2 (design: (private notes), #12).

What the code had (origin/0.4 b136ef37):
- `FxBus::process`: the reverb plays the summed reverb send (the Parts' REVERB sends, and the delay's REV SEND added before it) into its own wet buffer, added to the return. The wet signal is isolated exactly where a duck goes.
- `Reverb::process` is bank 2's (ADR 0142), in S0-S3's group, which has 8,000 B free (under ADR 0251's 8 KB warning).
- The master compressor (ADR 0030) has no sidechain and would duck the dry mix too, which the tester does not want.

## Decision
1. **Self-keyed, on the wet return only.** The key is the reverb's input (`send`), so the delay's REV SEND keys it too; the duck scales the return after MIX's ramp and before a TYPE switch's ramp. No key SOURCE (the design's B): the owner chose self-keyed. Built so a SOURCE could be added without a data change: the key is one block of samples handed to `duck::run`.
2. **The follower.** Each block's peak of `|send|`, in dB, held to the key's range `KEY_FLOOR` (−42 dBFS) .. `KEY_FULL` (−6 dBFS). It rises at once and falls the whole range, linearly in dB, over RELEASE: RELEASE is how long the tail takes to come all the way back once the input stops, whatever the level was. Its place in the range (`key_share`, 0..1) times AMOUNT sets the duck: `gain = 10^(MAX_DEPTH × AMOUNT × share / 20)`, `MAX_DEPTH` −30 dB. The gain ramps across each block from the last block's; AMOUNT eases over 20 ms (from rest too); at AMOUNT 0 or with the key at its floor the gain lets go to exactly 1.
3. **AMOUNT 0 is the reverb as it was, bit for bit.** With AMOUNT 0 and the duck let go (`Duck::idle`), `Reverb::process` makes no call: every reverb golden and render hash stands (proof below).
4. **Two Reverb params, project-level as the reverb's others.** `ReverbParams::duck` (id 15, ident `DUCK`, label DUCK, `Law::Pct`, default 0) and `duck_release` (id 16, ident `DUCK_RELEASE`, label RELEASE, default 96/128). Neither is per TYPE (`KnobMemory` keeps the TYPEs' panels only), and neither is modulatable (ADR 0010: the voice doesn't read them). A project saved before has neither and loads them at their defaults: AMOUNT 0. They take NRPN lines as every Reverb param does.
5. **RELEASE is on GLIDE TIME's law, 1 ms – 2 s.** My call: the design's 50 ms – 2 s needed a new `Law` and its readout in bank 1; GLIDE TIME's law (`Law::GlideTime`) gives the same top and a readout in ms and s already, and its default 96/128 is 0.30 s. Below about 30 ms the duck follows the input's blocks closely; that end is there for a gated, pumping effect.
6. **The page.** REV › DUCK (`block_registry::EFX_DUCK`, id 119), type-independent: AMOUNT · RELEASE, C–F empty; "AMOUNT" is the cell's label (`with_label`), the spec's DUCK, so a MIDI MAP row or the editor reads REVERB DUCK. My call on the design's third question: AMOUNT and RELEASE under a page called DUCK. REV's sub-pages are MORE, DUCK, MORE 2: MORE 2 (HALL's DENSITY) stays last, so `nav::skip_empty_rev` still skips it going down under the other TYPEs. The map reads it DUCK.
7. **Each cell's words** (`ui::notes::cell`, bank 2's, through `walk::band_note_in`): AMOUNT says `DUCKS THE TAIL UNDER ITS INPUT`, RELEASE `HOW FAST THE TAIL RETURNS`, in the hint's place.
8. **Bank 2.** `rev::duck::run` is out of line and sized, an S4 group (`*3rev4duck*`; S0-S3 is under 8 KB), called only from `Reverb::process`, bank 2's; `check-door-placed` pins it. The duck's state (`Duck`, 12 B) is the `Reverb`'s, in AXI. Bank 1 gains the two specs, their get/write arms, the page's `BlockDef` and its map label: closed-door data (ADR 0198). A closed door silences the reverb, as before.

## Alternatives considered
- **A key SOURCE** (REV IN / ALL / PART 1–6, the design's B): the owner chose self-keyed for 0.4.
- **The master compressor's sidechain** (C): it ducks the dry mix too.
- **A follower falling exponentially over RELEASE:** a loud input then takes several RELEASE times to let go (36 dB is about four time constants), so RELEASE wouldn't mean what it says.
- **50 ms – 2 s with a law of its own:** see decision 5.
- **The delay's duck** (the 2290 is a delay): not asked for 0.4; `duck::run` takes any stereo return, so the delay could take the same later.

## Consequences
- CPU: none at AMOUNT 0 (no call). Ducking, one abs-max over 64 samples, a `fast_log2`, two `fast_exp2` and a ramp over 128 samples a block. Host, release, best of five runs of 20,000 blocks, HALL under a steady send: 1,646 ns a block at AMOUNT 0, 1,811 ns at AMOUNT 1 (+165 ns, +10 % of the reverb, a small share of the whole block); not read on the unit; not billed (the FX bus's reserve, ADR 0116).
- RAM: 12 B in the `Reverb` (AXI).
- Sound work: needs the owner's ear, A/B on a bass hit and a chord stab per the listening stimuli, at AMOUNT 64 and 127 and RELEASE 0.1, 0.3 and 1 s. Not rendered here.
- Docs: `docs/chimera-sitemap.md`'s mixer map and text describe REV › DUCK. The page needs a new annotated screenshot for the tester guide.

## Measurements
Release builds, `CHIMERA_STAMP_CLEAN=1`, against ADR 0293's commit, each set built in both trees. Each budget line is raised by exactly its set's measured change, never the slack:

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---|---|
| orbit | +824 | +1,344 |
| default | +568 | +1,248 |
| bank2-gate | +568 | +1,184 |
| orbit,usb-config-writer | +824 |  |
| fault-inject | +768 |  |
| bench-algo | +952 |  |
| bench-algo,master-tape | +904 |  |
| bench-routing | +968 |  |
| bench-memory | +960 |  |
| bench-stress | +992 |  |
| bench-orbit | +856 |  |
| bench-tape | +848 |  |
| filter-probes | +776 |  |

Bank 1's growth on ORBIT (+824): `REVERB_SPECS`' two specs (144 B), `EFX_DUCK` (96 B), the get/write arms (32 B), the map's row (12 B), and, larger than the design's estimate, the board's `synth` (+304 B) and the audio's block (+106 B) where the reverb's specs and params are walked inline; keeping the two knobs in `ReverbParams`' padding (u8 steps) did not shrink those and cost more in the conversion, so they stay `f32`. The DSP is bank 2's: `rev::duck::run` 1,076 B in S4; `Reverb::process`'s idle check and call 224 B in S0-S3 (7,776 B free there now, under ADR 0251's 8 KB warning: new code keeps going to S4). ORBIT bank 1 has 17,128 B free after it.

Null: with AMOUNT 0 (every existing project's), `reverb_dispatch_test`'s goldens, the other reverb suites' and `golden_test` are unchanged; `reverb_duck_test` and `rev::duck`'s tests hold the duck.

## Sources
- Design: (private notes) § #12.
- Tester request: chimera-releases#12.
- Code: `chimera-engine/src/dsp/rev/duck.rs`, `rev/mod.rs` (`Reverb::process`), `dsp/reverb.rs` (`ReverbParams`, `REVERB_SPECS`), `block_registry::EFX_DUCK`.
- TC Electronic 2290 manual, dynamic delay (the ducked effect return); FabFilter Pro-R 2 manual, Ducking.
