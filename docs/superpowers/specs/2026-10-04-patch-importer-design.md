# Patch importer: DX7, TX81Z and PreenFM3 voices become Chimera Sounds

Status: design, for owner review (2026-10-04), revised after an adversarial review the same day. Built on the 2026-10-03 draft and the owner's rulings on it (#302 comment (private issue #302)). ADR 0088 (Proposed) records the decisions. All owner questions are settled (§ 11).

- **Issue:** private issue #302. Related: private issue #338 (curate ALGO's algorithms; the DX7 map re-imports onto its result), private issue #317 (factory library), private issue #301 (parity), private issue #336 (flash diet), private issue #263 (SysEx receive), private issue #38 (modulation modes), private issue #40 (TX character), private issue #327 (NOTE FX).
- **Read against** `main` at 2e3cb4dc: `chimera-core/src/dsp/algo/{algorithms,tx,env,waves,params,kernel,engine}.rs`, `modulation.rs`, `dsp/lfo.rs`, `dsp/modulator/env_a.rs`, `name.rs`, `storage/{sound,record}.rs`, `chimera-desktop/src/store.rs`, `chimera-core/tests/common/sweep.rs`; ADRs 0022, 0023, 0024, 0026, 0035, 0042, 0045, 0049, 0063.

---

## 1. Intent and rulings

Chimera loads DX7, TX81Z and PreenFM3 voices, as the stock firmware loads DX7 banks (#301). An imported voice becomes an ordinary ALGO Sound in the card library. It should sound like the original, close but not bit-exact. Everything the import loses is written down.

The owner's rulings, which this spec builds on without reopening:

| Ruling | Where it lands |
|---|---|
| **Host first.** The importer runs on the computer and writes Sounds to the card library. | § 3 |
| **No fixed-frequency operators, ever.** A fixed operator becomes the nearest ratio and is logged. | § 6.4 |
| **No new algorithm shapes.** Each DX7 voice maps to the nearest existing algorithm; the target is the set #338 curates (T1–T8 kept, A1–A24 trimmed). | § 6.1 |
| **A bad checksum warns.** It never refuses the file. | § 5 |
| **The PreenFM3 arpeggiator and step sequencers are dropped** for v1. They may return as NOTE FX (#327). | § 6.9 |
| **The factory library method:** port TX81Z voices, then mutate each one by design, never randomly. Raw bank data stays out of the repo; only our mutated results are committed. | § 9 |
| **No legacy-compatibility promise.** Imported Sounds must load safely; they need not keep sounding the same across firmware versions. | § 4 |
| From the draft's answers: operators may be renumbered; ratio errors are accepted in v1; losses go in the import report, not on the Sound; voice names only, no bank prefix; published measurements may be used with attribution; msfa (Apache-2.0) and ymfm (BSD-3-Clause) may be used host-side; imports leave ENV 1, ENV 3, LFO 2 and LFO 3 free. | throughout |
| **No licensing moves before V1** (the bootloader-hands-off ruling), so this design takes the conservative side wherever rights are unclear. | § 9 |

## 2. Precedent

| Question | Precedent | What we take |
|---|---|---|
| Where does import happen? | **Digitone:** sounds arrive from the computer through Elektron Transfer (or C6 SysEx) into the +Drive library. The device's SOUND MANAGER then imports a library Sound into a project. ([Digitone manual, Sound Manager](https://www.manualslib.com/manual/1359411/Elektron-Digitone.html?page=59); [OS 1.41 manual](https://www.elektron.se/wp-content/uploads/2024/09/Digitone_User_Manual_ENG_OS1.41_231108.pdf)) | The same two steps. The host writes the card library (`/CHIMERA/SOUNDS`), and the device's existing browser adds library Sounds to a project pool (ADR 0043). |
| Which DX7 files are accepted? | **Dexed** `Cartridge::load` (`Source/PluginData.h`, GPL-3, read only): a 4104-byte VMEM with header `F0 43 00 09 20 00`, else the first 4096 bytes taken raw. | Accept both. Also accept a 163-byte VCED single, which Dexed exports. |
| Bad checksum? | **Dexed** returns code 1, logs "sysex found, but checksum doesn't match", and loads anyway. Many `.syx` files in the wild carry bad sums. | Warn and import (the owner's ruling, with Dexed's precedent). |
| What a voice dump holds | Yamaha's published MIDI data charts: the DX7 owner's manual (VCED/VMEM), the TX81Z owner's manual (VCED, ACED, VMEM). | The byte layouts in § 5, entered from those charts. |
| How the stock PreenFM3 stores patches | `Ixox/preenfm3`, `firmware/Src/filesystem/PreenFMFileType.h` (GPL-3, read only): `ALIGNED_PATCH_SIZE 1024`, 128 patches per bank, 32 mixers per mix bank, DX7 banks under `0:/pfm3/dx7/`. | § 5.3. |

## 3. Where it runs

**v1 is host only.** The firmware does not link the importer, so it costs 0 B of flash (89.8 % used, #336). No new crate: the code splits along the line a later on-device import and SysEx receive (#263) would cut.

- **Parsers, in `chimera-core`** (`chimera-core/src/import/`), `no_std`, no allocation, no I/O. They are shared with SysEx receive (#263), which needs the same byte layouts on the device.
  - `parse(bytes: &[u8], hint: Option<Format>) -> Result<Dump, ParseError>`: recognises the format by its header and size (§ 5). `Dump` is an enum: `Dx7Bank`, `Dx7Voice`, `TxBank`, `TxVoice`, `PfmBank`, `PfmMix`. A dump yields voices one at a time.
  - The voice types (`Dx7Voice`, `TxVoice`, `PfmVoice`) hold the source fields as typed values (newtypes per range), so a map can't read a raw byte.
  - Parse warnings (`Warning::Checksum`, `Warning::Masked`, `Warning::Extra`) travel with the dump.
- **Maps, in `chimera-desktop`** (`src/import/`): pure functions, `map_dx7(&Dx7Voice) -> Imported`, `map_tx`, `map_pfm`, where `Imported { sound: Sound, losses: Vec<Loss> }`. Mapping tables are `const`. They live on the host because only the host uses them in v1. Moving them into the core is part of the on-device ADR.
  - `Loss { op: Option<OpNum>, kind: LossKind, detail }`. `LossKind` is one enum; each variant has a stable short tag (`fixed-freq`, `eg-shift`, `lfo-delay`, …) and says whether it is an **approximation** or a **drop**.
- **Imperative shell:** a `chimera-import` binary in `chimera-desktop` (`src/bin/import.rs`). It reuses `DirStore` (`store.rs`) to write the card, and the offline renderer to normalise and check. `just import` runs it (§ 7).
- **Render metrics:** the loudness, DC, click and stuck-note metrics move out of `chimera-core/tests/common/sweep.rs` into a `render_metrics` module behind `chimera-core`'s `test-support` feature (named apart from the desktop's `qa.rs`, the key-script frame loop). The tests and the importer then share one implementation.
- **On-device import is in scope later, not in v1,** under its own ADR: an IMPORT source in the Sound browser reading `.syx` from the card, after #336 frees flash. It would reuse the core's parsers and move the maps into the core. It has one limit: render-based VOLUME and the QA checks (§ 7) exist only on the host, so it would set VOLUME from a stored-level estimate.

**Why host first:** it costs no flash. It is the only path that can render every import, set its loudness, and run QA before the owner listens. #317's port-then-mutate workflow is host work anyway. None of the host work is thrown away later: the shell is the only host-specific part.

## 4. What an import produces

- **An ordinary Sound file**, `/CHIMERA/SOUNDS/S#######.A`, written by `encode_sound` through the library's own add path (ADR 0045). The engine is ALGO. There is no new record and no codec change, so the file loads on today's firmware and is decoded and validated like any other.
- **Every import sets:**
  - ALG B = ALG A, MORPH 0 (ADR 0049 dims MORPH when A = B);
  - the filter open (CUTOFF 20 kHz, DRV 0, FLD 0), as `factory.rs` does;
  - no routes except the ones § 6 adds;
  - VOLUME from the host render at C4, velocity 100. By default the **bank's median** voice lands on the −22 LUFS reference and every voice keeps its level relative to the bank, as ADR 0063 keeps the factory Sounds' own levels around the median (the owner, 2026-10-04);
  - unused operators at LEVEL 0, so `AlgoEngine::cost` bills nothing for them (ADR 0026).
- **Safe loading** is the codec's existing guarantee: every value is range-checked on decode. The importer clamps every field to its `ParamSpec` range before encoding, and a test runs every golden import through `SoundCheck` and the decoder.
- **No compatibility promise.** A later importer or firmware may map the same voice differently, for example after #338 or #40. Re-importing then gives a different Sound. Sounds already imported are never migrated to match.
- **Dedupe:** a voice whose `sound_crc` equals a library Sound's is skipped and reported as `duplicate of S#######`. Importing the same bank twice adds nothing.
- **Name:** the voice name (10 chars DX7/TX81Z, 12 PreenFM3) mapped to `SoundName` (16 chars: A–Z, a–z, 0–9, space, `-`). Any other character (`.`, `/`, `¥`, `→`, `←`) becomes `-`. Runs of `-` collapse, and the ends are trimmed. An empty result becomes `DX7 01`…`32`, `TX 01`…`32` or `PFM 001`…`128`, from the slot. Names may repeat in the library; the file number keeps them apart.

## 5. Formats

- **Checksums** are `(-(Σ summed bytes)) & 0x7F`, over the bytes after the 6-byte header. For ACED that includes its 10-byte ASCII header `LM  8976AE` (33 bytes summed). A mismatch gives `Warning::Checksum { expected, found }`, and the file still imports.
- **Refusals:** a wrong size, header or format byte is a `ParseError` and refuses the file.
- **Several messages:** any number of SysEx messages in one file are read in order, so a file of 32 singles, or an ACED+VCED pair, both work.
- **Packed bytes are masked before they are clamped.** Each field is first masked to its own bits (a VMEM byte's top bit, the unused bits beside a packed field), then clamped to its range. Both steps log `clamped-field` when they change a value, and neither refuses the file.
- **Headerless 4096-byte files** could be a DX7 or a TX81Z bank. The parser tests both layouts for plausibility: every field within its range after masking, the DX7 ALG byte ≤ 31 and OL ≤ 99, the TX81Z voices' bytes 84–127 all zero. One plausible layout is taken. If both or neither are plausible, the file is refused with `ambiguous headerless bank`, and `--format dx7|tx81z` (§ 7) settles it.

### 5.1 DX7

| Dump | Bytes | Header | Payload |
|---|---|---|---|
| VMEM 32-voice bank | 4104 | `F0 43 0n 09 20 00` | 32 × 128 B packed, checksum, `F7` |
| Raw bank | 4096 | none | 32 × 128 B packed (no checksum to check) |
| VCED single voice | 163 | `F0 43 0n 00 01 1B` | 155 B unpacked, checksum, `F7` |

**VMEM packed voice, 128 B:**
- **Operators:** 6 × 17 B, **op 6 first**. Bytes 0–3 are R1–R4 and 4–7 are L1–L4. Byte 8 is BP, 9 LD, 10 RD.
  - Byte 11: LC in bits 0–1, RC in bits 2–3.
  - Byte 12: RS in bits 0–2, DET in bits 3–6.
  - Byte 13: AMS in bits 0–1, KVS in bits 2–4.
  - Byte 14: OL.
  - Byte 15: M (fixed) in bit 0, FC in bits 1–5.
  - Byte 16: FF.
- **Common:** bytes 102–105 are PR1–4 and 106–109 PL1–4. Byte 110 is ALG (0–31).
  - Byte 111: FB in bits 0–2, OKS in bit 3.
  - Bytes 112–115: LFS, LFD, PMD, AMD.
  - Byte 116: LKS in bit 0, LFW in bits 1–3, PMS in bits 4–6.
  - Byte 117: TRNSP. Bytes 118–127: NAME (10 ASCII).

**VCED, 155 B:** the same fields unpacked.
- 21 per operator, op 6 first: R1–4, L1–4, BP, LD, RD, LC, RC, RS, AMS, KVS, OL, M, FC, FF, DET.
- Then PR1–4, PL1–4, ALG (134), FB, OKS, LFS, LFD, PMD, AMD, LKS, LFW, PMS, TRNSP (144), NAME (145–154).


### 5.2 TX81Z (also DX21, DX27, DX100)

| Dump | Bytes | Header | Payload |
|---|---|---|---|
| VMEM 32-voice bank | 4104 | `F0 43 0n 04 20 00` | 32 × 128 B packed, checksum, `F7` |
| VCED single voice | 101 | `F0 43 0n 03 00 5D` | 93 B, checksum, `F7` |
| ACED additional | 41 | `F0 43 0n 7E 00 21`, then ASCII `LM  8976AE` | 23 B, checksum, `F7` |

- **Operator order:** VCED, ACED and VMEM all store the operators as **OP4, OP2, OP3, OP1**.
- **A VCED without an ACED** (DX21/27/100) imports with the ACED defaults: W1, FINE 0, not fixed, no EG shift.
- **VMEM, 128 B per voice, as we read the owner's manual's chart.** Task 1 (§ 10) checks every row of this table against the chart, and a test pins it: a VCED+ACED pair packed by our own encoder must parse back field for field.

| Bytes | Content |
|---|---|
| 0–39 | 4 operators × 10 B: AR, D1R, D2R, RR, D1L, LS; byte 6 = AME (bit 6), EBS (bits 3–5), KVS (bits 0–2); OUT; CRS; byte 9 = RS (bits 3–4), DET (bits 0–2) |
| 40 | SYNC (bit 6), FBL (bits 3–5), ALG (bits 0–2) |
| 41–44 | LFS, LFD, PMD, AMD |
| 45 | PMS (bits 4–6), AMS (bits 2–3), LFW (bits 0–1) |
| 46–56 | TRPS, PBR, the mode bits (poly/mono, sustain, porta), PORT, FC VOL, MW PITCH, MW AMPL, BC PITCH, BC AMPL, BC P BIAS, BC E BIAS |
| 57–66 | NAME (10) |
| 67–72 | PR1–3, PL1–3 (DX21 pitch EG; the TX81Z ignores them, and so do we) |
| 73–80 | ACED, 4 operators × 2 B: EGSFT (bits 4–5), FIX (bit 3), FIXRG (bits 0–2); OSW (bits 4–6), FINE (bits 0–3) |
| 81–83 | REV, FC PITCH, FC AMPL |
| 84–127 | unused |

A DX21, DX27 or DX100 bank has zeros at 73–83. That reads as W1, FINE 0, not fixed, no shift, which is correct.

A DX11 or V50 bank uses bytes from 84 on for its own additional data (the V50's effects, the DX11's extra parameters). Those bytes are ignored, and the voice gets `Warning::Extra` (`dx11-v50-extra`, D) when any is non-zero.

### 5.3 PreenFM3

| File | Layout |
|---|---|
| `*.bnk` | 128 slots × 1024 B (`ALIGNED_PATCH_SIZE`). Each slot holds a `FlashSynthParams`: about 936 B, almost all `f32` LE in rows of four, then `presetName[13]` and 24 `u8` envelope curves (`envCurves1To4`, `envCurves5To6`). A `u32` LE version word sits near the slot's end. |
| `*.mix` | 32 mixers × (1024 B mixer state + 6 × 1024 B instruments). Each instrument is a `.bnk` slot. Only the instruments are imported; the mixer state is dropped (`pfm-mixer`). |
| `0:/pfm3/dx7/*.syx` | DX7 banks. The DX7 parser covers them. |

- **The offsets** are our own table, re-entered from the struct order in `PreenFMFileType.h` and `Common.h`. We don't copy the GPL code. A test checks that the table's total is the struct size, and a fixture from a stock-written bank (opt-in, § 8.6) pins the version word's offset and values.
- **An unknown version word is refused**, with a note naming it.
- **Floats are checked before use.** A NaN or infinite field takes its default and logs `bad-float`; every finite one is then clamped to its range (`clampf`). No float reaches a map unchecked.

## 6. Mapping

### 6.0 Notation

Chimera's ALGO targets:
- **Operator:** WAVE 0–15, CRSE 0–63, FINE 0–15, DETUN ±3 (1.5 c a step), LEVEL 0–99 (0.75 dB a step), AR/D1R/D2R 0–31, D1L 0–15 (3 dB a step), RR 1–15, RS 0–3, FDBK 0–7, VEL 0–7.
  - A modulator at LEVEL 99 swings 4 cycles of phase (`PM_CYCLES`).
  - `FEEDBACK_CYCLES` = 0, 1/32, 1/16 … 2, averaged over two samples.
- **Algorithm:** ALG A, ALG B, MORPH, TRNSP ±24.
- **Envelope law** (`env.rs`):
  - effective rate `eff = 2·r + ks`, from 0 to 63; release is `4·RR + 2 + ks`;
  - every 4 steps of `eff` doubles the speed;
  - a decay falls 96 dB in 92.8 s × 2^(−eff/4), and an attack rises linearly in 11.6 s × 2^(−eff/4);
  - the key code is `ks = (note − 21)/3 >> (3 − RS)`.
- **Matrix:** 8 sources (ENV1, LFO1, ENV2, ENV3, LFO2, LFO3, VEL, NOTE) and up to 16 destinations, with amounts ±127. NOTE is `(n − 60)/120`. Op LEVEL is modulatable; LFO DEPTH and RATE are not. PITCH and FINE follow ADR 0042's half-span law.
- **LFO:** RATE 0.01–20 Hz; SHAPE SINE, TRI, SAW, SQR, S&H; SYNC FREE or RETRIG.

**Kinds.** Every source field has one row, of one kind:
- **E**xact: a units change only.
- **C**onverted: refit to our scale, within a stated error bound that § 8.4's thresholds cover. Within the bound it is not logged. A value pushed outside it (a clamp) is logged, so every lossy case is in the report.
- **A**pproximate: the voice gets a `Loss` naming it.
- **D**ropped: the voice gets a `Loss` when the field is not at its neutral value.

Each format's fields are an enum, matched exhaustively by its map, so a field without a row doesn't compile.

**The report is complete by rule:** a voice with nothing logged must pass § 8.4's A/B. Whenever a row's error can exceed the A/B's tolerance, that row logs.

### 6.1 Algorithms

**TX81Z (E).** ALG n maps to Tn on ops 1–4, by identity: Chimera's T1–T8 are the TX81Z's eight (ADR 0022). Ops 5 and 6 stay at LEVEL 0. FBL goes on op 4, the TX81Z's feedback operator.

**DX7.** ALGO's feedback is per operator, so a DX7 algorithm only has to match in topology: operators are renumbered on import (the owner accepted renumbering). The DX7 link table was entered from the DX7 algorithm chart. `algo_map_test` re-derives every row below from `ALGORITHMS`, so a typo fails.

**Per-voice nearest (A when the cost is not 0).** The importer chooses the algorithm per voice, not per DX7 algorithm number:

1. **Prune.** Drop every operator that is silent at every key and every velocity: OL 0 with no keyboard level scaling that lifts it at any key, or L1–L4 all 0. Then drop every modulator, not a carrier, whose targets have all been dropped. Repeat until nothing changes.
2. **Search.** Over every algorithm in the **pool** (below) and all 720 operator permutations, minimise the cost:
   - each DX7 link `a → b` the candidate lacks costs `w(a)`;
   - each candidate link between two kept operators that the DX7 lacks costs `w(a)`;
   - each kept DX7 carrier placed on a non-carrier slot, and each kept modulator placed on a carrier slot, costs `w(op)`.

   `w(op)` is the operator's linear output gain at its OL: 1 at 99, about −6 dB per 8 steps. Slots left over get LEVEL 0.
3. **Ties.** An exact match wins, then T before A, then the lower AlgoId.

A voice whose cost is 0 is exact. Otherwise the report gets `alg-nearest`, with the DX7 links lost and added in the DX7's own operator numbers. Host cost: 32 × 720 candidates a voice, under a millisecond.

**The table below is the structural map**, with all six operators audible. `algo_map_test` pins it: the per-voice search with every OL at 99 must give the same row. It is computed against today's 32 algorithms, the v1 pool, and against the floor #338 guarantees (T1–T8 only).

| DX7 | Links (DX7 op numbers) | Today's 32 | DX7 op 1–6 → slot | Cost | T1–T8 only | Cost |
|---|---|---|---|---|---|---|
| 1, 2 | 6→5→4→3, 2→1 | T1 | 5 6 1 2 3 4 | 0 | T1 | 0 |
| 3, 4 | 6→5→4, 3→2→1 | A12 | 1 3 5 2 4 6 | 0 | T1 | 2 |
| 5, 6 | 6→5, 4→3, 2→1 | T5 | 1 2 3 4 5 6 | 0 | T5 | 0 |
| 7, 8, 9 | 6→5→3, 4→3, 2→1 | T3 | 5 6 1 4 2 3 | 0 | T3 | 0 |
| 10, 11 | 6→4, 5→4, 3→2→1 | T1 | 1 2 3 5 4 6 | 2 | T1 | 2 |
| 12, 13 | 6, 5, 4→3, 2→1 | T3 | 5 6 1 2 3 4 | 2 | T3 | 2 |
| 14, 15 | 6, 5→4→3, 2→1 | T2 | 5 6 1 2 3 4 | 0 | T2 | 0 |
| 16, 17 | 6→5→1, 4→3→1, 2→1 | A20 | 1 6 2 3 4 5 | 0 | T3 | 2 |
| 18 | 6→5→4→1, 3→1, 2→1 | A20 | 1 2 6 4 5 3 | 2 | T1 | 4 |
| 19 | 6→5, 6→4, 3→2→1 | A10 | 1 4 6 2 3 5 | 2 | T1 | 3 |
| 20 | 6, 5→4, 3→2, 3→1 | T2 | 1 5 6 2 3 4 | 3 | T2 | 3 |
| 21 | 6→5, 6→4, 3→2, 3→1 | T6 | 1 2 4 3 5 6 | 2 | T6 | 2 |
| 22 | 6→5, 4, 3, 2→1 | T6 | 5 6 1 2 3 4 | 0 | T6 | 0 |
| 23 | 6→5, 6→4, 3→2 | T6 | 1 5 6 2 3 4 | 1 | T6 | 1 |
| 24 | 6→5, 4, 3 | A3 | 3 4 1 2 5 6 | 1 | T6 | 2 |
| 25 | 6→5, 6→4 | A3 | 3 4 5 1 2 6 | 0 | T8 | 1 |
| 26, 27 | 6, 5→4, 3→2 | T2 | 1 5 6 2 3 4 | 2 | T2 | 2 |
| 28 | 5→4→3, 2→1 | A10 | 1 4 2 5 6 3 | 1 | T1 | 2 |
| 29 | 6→5, 4→3 | T7 | 1 2 3 4 5 6 | 0 | T7 | 0 |
| 30 | 5→4→3 | A6 | 2 3 1 5 6 4 | 0 | T7 | 2 |
| 31 | 6→5 | T8 | 1 2 3 4 5 6 | 0 | T8 | 0 |
| 32 | none | A1 | 1 2 3 4 5 6 | 0 | (A1 needed) | — |

Cost here is the unweighted count of links and carriers that differ. Notes:
- "6, 5, 4→3" means 6, 5 and 4 each modulate 3.
- **Exact today:** 19 of 32. **Exact on T1–T8 alone:** 12.
- Five A algorithms carry 7 DX7 algorithms exactly: A1 (32), A3 (25), A6 (30), A12 (3, 4) and A20 (16, 17). A3, A10 and A20 are also the nearest for 18, 19, 24 and 28. That list is an input to #338's listening session, not a demand on it.
- DX7 32, six carriers, has no T equivalent. If #338 drops A1, DX7 32 loses one carrier to T8's 6→5 link.

**Feedback.** FB goes on the slot the DX7 feedback operator lands in. DX7 4 and 6 feed back across a loop (4→6 and 5→6). ALGO has only self-feedback, so FB goes on the loop's top operator (`fb-loop`, A): the loop's noisy onset softens. The reference engine (§ 8.4, msfa) also runs 4 and 6 as self-feedback on op 6, so the A/B cannot see this loss; it is logged against the hardware.

**The pool and #338.** v1 ships against today's pool: all 32 algorithms, T1–T8 included. It does not wait for #338. When #338 lands, the pool becomes the curated set, read from the one place #338 defines it in `ALGORITHMS` (never a copy), and a test then fails if a DX7 import targets an algorithm outside it. Then the owner's DX7 imports are **re-imported** onto the curated set. The no-legacy ruling allows this; the earlier imports stay loadable, since hidden algorithms keep their disk codes.

### 6.2 Envelopes

**TX81Z (E).** AR, D1R, D1L, D2R, RR and RS map 1:1: ALGO's envelope is the TX81Z's (ADR 0022). 0 means hold in both. EGSFT (EG shift: a 48, 24 or 12 dB floor) has no equivalent. It is **D** (`eg-shift`): the operator decays fully instead of stopping at its floor. That's a candidate for #40.

**DX7 (C, A for some shapes).** The DX7 EG starts at L4, goes to L1 at rate R1, to L2 at R2 and to L3 at R3, sustains at L3, and returns to L4 at R4 after key-off. ALGO's EG is attack to full, D1R to D1L, D2R toward zero (0 = hold), and RR to zero.

**Rate law.**
- The DX7's internal rate is `q = ⌊41·R/64⌋` (0–63) (msfa, Apache-2.0), and it also doubles every 4 steps. The two laws differ only by an offset:
  - `D1R, D2R = clamp(round((q − K_d)/2), 1, 31)`;
  - `RR = clamp(round((q − K_d − 2)/4), 1, 15)`;
  - `AR = clamp(round((q − K_a)/2), 1, 31)`.
- A DX7 rate of 0 maps to 1, not 0, because ALGO's 0 means hold. The exception is a stage that doesn't move (same level): it maps to 0.
- **Slow rates clamp.** With K_d ≈ 8, every DX7 rate up to about 12 (q ≤ 7) lands on D1R/D2R 1, and the slowest releases on RR 1. A clamp that moves a stage's time by more than the grid's 19 % (41 % for release) logs `rate-clamp` with the time error.
- **K_d and K_a are calibrated, not guessed.** The calibration (§ 8.4) fits them against Dexed renders, and ADR 0088's successor records the values. The starting estimate is K_d ≈ 8: a full-span DX7 decay at q = 0 is about 4× ALGO's 92.8 s.
- K_a is separate because the DX7's attack is shaped in dB (it jumps up from about −40 dB and then accelerates), while ALGO's is a linear amplitude ramp.

**Level law.**
- DX7 attenuation is `att(L) = 0.75 dB · (127 − S(L))`. S(L) = L + 28 for L ≥ 20, and a 20-entry table below that (msfa's, Apache-2.0, credited in `THIRD_PARTY.md`).
- `D1L = clamp(15 − round(att_rel / 3 dB), 0, 15)`, where `att_rel` is measured from the envelope's peak.

**Shapes**, chosen per operator:

| Case | Mapping | Kind |
|---|---|---|
| L1 is the peak, L2 = L3, L4 = 0 (ADS) | AR ← R1, D1R ← R2, D1L ← L2, D2R 0, RR ← R4 | C |
| L3 = 0 (plucks, bells) | AR ← R1, D1R ← R2, D1L ← L2, D2R ← R3, RR ← R4 | C |
| 0 < L3 < L2 (two slopes down to a sustain) | D1L ← L3. D1R is the single rate whose time from L1 to L3 equals R2's time plus R3's time. D2R 0. The knee at L2 is lost. | A `eg-knee` |
| L2 > L1 or L3 > L2 (rises after the attack: brass, swells) | The peak is max(L1, L2, L3). AR gets the summed time to reach it, so the rise folds into the attack. | A `eg-rise` |
| L1 < 99 | The peak's attenuation moves into op LEVEL, and the EG becomes relative to the peak | C |
| L4 > 0 (starts from, or releases to, a floor) | Start at 0, release to 0 | A `eg-floor` |

**Error bounds of the C rows:**
- Rates sit on ALGO's 2-step grid, which is ±1 effective step: **±19 % in time**.
- Release sits on a 4-step grid: **±41 %**.
- D1L is ±1.5 dB, on top of the DX7's own 0.75 dB level step.
- § 8.4's pass criterion is built on these bounds.

**Rate scaling (C).** The DX7 adds `RS·x/8` to q. ALGO adds `x >> (3 − rs)`, from the same key code x (0–31).
- DX7 RS 1, 2 and 4 are exact as ALGO rs 0, 1 and 2.
- RS 3, 5, 6 and 7 round to the nearest neighbour. At the top key that is up to 4 effective steps out (2×), or 7.75 (3.8×) for RS 6. Logged as `rate-scale` (A).
- DX7 RS 0 maps to 0, which in ALGO still adds `x >> 3`: 2 steps (1.4×) at note 84, 3 (1.7×) at the top key. This is an ALGO quirk inherited from the TX. It exceeds the A/B's 25 %, so it is logged as `rate-scale` (A) too.

### 6.3 Output level and velocity

**Op LEVEL:**
- **TX81Z:** OUT → LEVEL (E).
- **DX7 carriers:** `LEVEL = clamp(S(OL) − 28 − Δpeak, 0, 99)` (C). That's 1:1 for OL ≥ 20, and `Δpeak` is the EG peak's attenuation (§ 6.2).
- **DX7 modulators:** the same, less a **modulator offset `K_m`** (C). A full-scale DX7 modulator swings about 2 cycles of phase and ALGO's swings 4, which makes `K_m = 8` (6 dB). This is calibrated from sideband levels in § 8.4, not trusted.
- An operator is either a carrier or a modulator in any one DX7 algorithm, so there is no conflict between the two rows.

**Velocity:**
- **TX81Z:** KVS → VEL 1:1 (C). ALGO's VEL is 3 dB per step at velocity 0, linear in dB; the TX's curve is close to that.
- **DX7:** KVS 0–7 → VEL 0–7 (C). The DX7's curve (msfa's velocity table) is deeper at low velocity. The calibration fits the KVS → VEL map at velocities 40, 100 and 127, the A/B's velocities, and the fitted map replaces 1:1.
- **The cap:** ALGO's VEL 7 gives at most 21 dB at velocity 0, against about 78 dB of DX7 range at KVS 7. Where the DX7 depth over velocities 40–127 exceeds what VEL 7 can give by more than 3 dB, the voice logs `vel-depth` (A) with the shortfall.

### 6.4 Frequency

| Field | TX81Z | DX7 | Kind |
|---|---|---|---|
| Coarse | CRS 0–63 → CRSE | `r = c'·(1 + FF/100)`, with c' = 0.5 when FC = 0. The nearest point on ALGO's CRSE × FINE × DETUN grid. | TX E, DX C |
| Fine | ACED FINE 0–15 → FINE. `tx::ratio` steps linearly to `FINE_TOP`, which is up to about 14 c off the TX81Z's own fine steps. | (in the DX7 ratio fit) | TX C |
| Detune | DET 0–6 → DETUN −3…+3 | DET 0–14 (7 = centre), folded into the grid fit above: target `r · 2^(d(DET, note)/1200)`. The detune law `d` is **fitted from measured Dexed renders** (§ 8.4) at notes 36, 60 and 84. Google's msfa uses a constant `12606·(DET − 7)` in its log-frequency units, about 0.9 c a step. Dexed's pitch-dependent detune (`Source/msfa/dx7note.cc:61`) is Dexed's own GPL change, cited only as measured behaviour: no code or constant is taken from it. Ours is constant in cents, so the fit is made at C4 and the error elsewhere is logged. | TX C, DX C; both log `detune-beat` (below) |
| Fixed frequency | FIX = 1: Hz from FIXRG, CRS and FINE per the manual's chart | M = 1: Hz = 10^(FC mod 4 + FF/100), from 1 Hz to 9.77 kHz | **A** `fixed-freq` |

**DX7 ratio fit**, measured over all 3 200 DX7 ratios against the 64 × 16 × 7 grid:
- **Within ALGO's range** (≤ 27.57, 1 969 ratios): median error 0.2 c, p90 1.9 c, p99 44 c, worst 95 c. The worst are 0.505–0.6, in the gap between CRSE 0.50 and 0.71.
- **Integer ratios 0.5–27:** within 0.4 c.
- **Above 27.57** (1 231 ratios, rare in real patches), handled to keep harmonicity:
  - an integer ratio (FF = 0) maps to **27**, the largest integer on the grid;
  - any other ratio clamps to 27.57;
  - both are logged as `ratio-range`, with the error in cents.
- **Errors over 10 c** are logged as `ratio-off`, with the cents.
- **Beating is logged by its rate.** For every pair of operators whose ratios lie within 2 % of each other, the importer computes the beat rate the source gives and the one the import gives at notes 36, 60 and 84. If any differs by more than 25 % (or 0.3 Hz, whichever is larger), it logs `detune-beat` with both rates. Example: a 1.01 operator lands on 1.0026, 12.7 c flat, so a pair at C4 beats 0.7 Hz instead of 2.6 Hz. The owner accepted these errors for v1, and the report names them.

**Fixed frequency, the owner's ruling: no fixed operators, ever.**
- **The reference note is chosen per voice.** It is the MIDI note whose pitch, after the voice's TRNSP, is nearest the frequency of the voice's **main fixed carrier** (the fixed carrier with the highest output level), clamped to notes 24–96. A voice whose fixed operators are all modulators uses note 60. The operator gets `r = f_fixed / f(note_ref + TRNSP)`, so the main fixed carrier plays at its original pitch on that key.
- That ratio then takes the grid fit, range rule and logging of any other ratio.
- **Sub-audio modulators:** a ratio under 0.5 (for example a 1–10 Hz fixed modulator, which wobbles rather than adds sidebands) clamps to 0.5. That turns it into an audio-rate modulator, a different sound, logged as `fixed-subaudio`.
- **Nyquist:** `engine.rs` pins an operator at 0.5 cycles a sample (`op_cycles`). A fixed source never exceeded 9.77 kHz, so a fixed-derived ratio is capped at the largest grid ratio for which `ratio × f(108)` stays below 24 kHz, logged `nyquist` when the cap bites. Ordinary ratios are not capped: the source aliases or folds at the top keys too.
- **All-fixed voices** (drums and effects, where every operator is fixed) still import. The whole voice plays its original sound on the reference key and transposes away from it. The report adds `all-fixed`, names the key, and the Sound's name keeps the source name so the voice can be found for a KIT key (the DRUM engine is the long-term home for these sounds).
- The report line gives the original Hz, the key and the ratio, e.g. `op 2 fixed 1000 Hz → ratio 2.00 at key 72 (C5)`.
- The same rule applies to the TX81Z FIX bit and to PreenFM3's fixed `frequencyType`.

### 6.5 Keyboard level scaling

ALGO has no per-operator level scaling, so the importer uses a **NOTE → op LEVEL route**, linear in dB.
- NOTE moves 0.1 per octave and LEVEL spans 99 steps, so a route at amount `a` gives `0.078·a` LEVEL steps per octave: at most **7.4 dB/oct** at a = 127.
- **The fit.** The source's level curve over C1–C7 (0 dB on an inactive side) is fitted with one line, the slope and base-LEVEL offset with the smallest worst-case dB error.
  - **Modulators never boost:** the line is constrained never to rise above the source at any key, because extra index is extra brightness, the worse error.
  - **Carriers are centred:** plain least-worst-error, which may boost a little. For a one-sided RD −LIN, never-boost costs about 10 dB at BP, against about 5 dB centred, and loudness errs both ways equally by ear.
- Every key-scaled voice logs `key-level` with the worst-case error in dB.

| Source | Mapping | Kind |
|---|---|---|
| TX81Z LS 0–99 (attenuates upward) | A negative NOTE → LEVEL route; slope from the calibration, clamped at 7.4 dB/oct | A `key-level` when LS > 0 |
| DX7, one side −LIN or +LIN (the common case: RD −LIN taming the top) | The fitted line: never-boost on a modulator (one side's slope on both sides would add up to +22 dB of index three octaves below BP), centred on a carrier. | A `key-level` |
| DX7 −EXP or +EXP, or both sides active | The same fit | A `key-level` |
| Any depth over 7.4 dB/oct (DX7 LIN 99 is about 24 dB/oct) | Clamped | A `key-level` |

Up to six LEVEL destinations, plus PITCH and FINE, fit in the 16.

### 6.6 LFO, pitch EG and controllers

A DX7 or TX81Z import uses one LFO (LFO 1) and one pitch EG (ENV 2) at most. ENV 1, ENV 3, LFO 2 and LFO 3 stay free for #317's design pass and for CHARACTER (#335). A PreenFM3 import is the exception: its own LFOs and envelopes are its content, so it uses them (§ 6.8).

| Field | Mapping | Kind |
|---|---|---|
| Wave | **DX7:** TRI, SAW↓, SAW↑, SQR, SINE, S/H → TRI, SAW (route amount negated for ↓), SAW, SQR, SINE, S&H. **TX81Z:** SAW↑, SQR, TRI, S/H → SAW, SQR, TRI, S&H. A fixture test checks SAW's direction. | E |
| Speed | LFS 0–99 → Hz, from a speed table measured in calibration (DX7 about 0.06–47 Hz; the TX81Z's is its own table). RATE stops at 20 Hz, so faster speeds clamp. | C; A `lfo-fast` over 20 Hz |
| Delay | LFD has no equivalent: LFO DEPTH is not a matrix destination | D `lfo-delay` when LFD > 0 |
| Key sync | LKS or SYNC on → RETRIG, else FREE. The DX7 has one LFO per instrument and ours is per voice; FREE is close. | C |
| Pitch mod | An LFO 1 route to FINE, or to PITCH when the depth is over 1 st. Depth = PMD/99 × PMS-table (semitones), through ADR 0042's half-span law. | C |
| Amp mod | An LFO 1 route to the LEVEL of each operator with DX7 AMS > 0 or TX81Z AME = 1. Depth = AMD/99 × AMS-table (dB). The Yamaha AM only cuts and ours is bipolar, so the base LEVEL drops by half the depth. | A `amp-mod` |
| DX7 pitch EG | Skipped when all PL are 50. Otherwise ENV 2 (Envelope A, AHDSR) → PITCH, or FINE for small depths. Attack ← (L4 → L1, R1); decay ← (→ L2 → L3, R2 + R3); sustain ← L3; release ← R4. Depth comes from a measured PEG level → cents table. It fits exactly only when L4 = 50 and L1–L3 sit on one side of 50; otherwise the best one-sided fit. | C, A `pitch-eg` for the fit |
| TX81Z PR/PL | Ignored: the TX81Z itself ignores them | — |
| Controllers: MW, BC, FC and AT assignments, EBS, PBR, porta, poly/mono, FC VOL | No such sources in the 8-source matrix | D `controller` (one line, listing the fields) |

### 6.7 Feedback, transpose, waves, misc.

| Field | Mapping | Kind |
|---|---|---|
| TX81Z FBL 0–7 | FDBK on op 4, 1:1. ALGO's 2^(n−6) cycles with two-sample averaging is the OPZ law. | E |
| DX7 FB 0–7 | The reference engine's law (msfa) is 2^(n−7) cycles, so **FDBK = n − 1**, and FB 0 → 0. The calibration confirms it by spectrum. | C |
| TRNSP / TRPS 0–48 (24 = no transpose) | ALGO TRNSP = value − 24 | E |
| TX81Z OSW 0–7 | WAVE W1–W8 (ADR 0023: our recipes, not the ROM) | E |
| DX7 waves | W1 (sine) | E |
| DX7 OKS = 0 (oscillators free-running) | ALGO resets phase at note-on | A `osc-phase` |
| TX81Z REV (reverb rate) | — | D `rev-rate` when > 0 |

### 6.8 PreenFM3

- **FM vs PM.** In the stock `Voice.cpp`, a modulator scales its target's frequency by `IM · f_mod · a`, so the FM index is β = IM · a. For a sine modulator, FM by `sin` equals PM by `−β cos`: the same sideband magnitudes.
  - So IM maps to the modulator's op LEVEL: `LEVEL = 99 + 8·log2(IM / 8π)` (8π rad is ALGO's 4-cycle swing). IM 1 gives LEVEL 62.
  - Stacked chains, feedback and DC differ. Every PreenFM3 import carries `fm-as-pm` (A). #38's FM mode is the faithful route later, and no part of this design waits for it.
- **Per-link indexes.** A PreenFM3 modulator can carry different IMs to two targets, but ALGO has one LEVEL per operator. The importer takes the larger IM (A `im-per-link`).
- **Algorithms.** The stock firmware's 32 topologies, taken from the `Voice.cpp` diagrams. They go through § 6.1's per-voice search against the pool.
- **Envelopes.** Times in seconds (0–16), linear levels and curves.
  - Times convert to rates by inverting ALGO's closed form: decay `eff = 4·log2(92.8 · log2(a/b) / (16·T))`, attack `eff = 4·log2(11.6 / T)`.
  - The stage shapes follow § 6.2's cases.
  - Curves are A (`eg-curve`).
- **Frequency.** Float ratios go to the grid as in § 6.4. A fixed `frequencyType` gets the fixed-frequency rule.
- **Shapes.** SIN → W1; SAW → SAW; SQUARE → SQR. sin², half-sine and |sin| go to the nearest TX wave (A `wave-near`). RAND, USER1–6 and the key-tracked shapes are D (`wave`), and that operator falls back to W1.
- **Matrix (12 rows).**
  - **Sources:** LFO1–3 map to LFO1–3. LFOENV1 maps to ENV 1 and LFOENV2 to ENV 3, both Envelope A; LFOENV2's loop mode is A (`env-loop`). VELOCITY maps to VEL. NOTE1 and NOTE2 map to NOTE, and their curves are A (`note-curve`). MW, PB, AT, CC, breath, MPE, random and the step sequencers are D (`matrix-source`).
  - **Destinations:** IM n maps to that modulator's LEVEL, and MIX to the carrier's LEVEL. ALL_OSC_FREQ maps to PITCH. FILTER maps to CUTOFF (A). The rest are D (`matrix-dest`).
- **Effects.** The filter effect maps to the voice filter's nearest kind (A `filter`). The other effects are D (`pfm-effect`).
- **Dropped for v1, the owner's ruling:** the arpeggiator, both step sequencers, the performance macros and the mixer state (D `arp`, `step-seq`, `macro`, `pfm-mixer`). #327 may bring the first two back as NOTE FX.

### 6.9 Fidelity estimate

These are hypotheses until § 8.4 measures them. The ADR that accepts this work records the measured rates.

| | TX81Z | DX7 | PreenFM3 |
|---|---|---|---|
| Algorithms | exact | 19 exact on today's pool; the rest nearest, re-imported after #338 | per-voice nearest |
| Envelopes | exact, except EG shift | ±19 % time, ±41 % release, some shapes approximated | curves approximated |
| Frequency | exact, except fixed → ratio | median 0.2 c; small ×1.0x offsets 10–30 c; > 27.57 remapped | float ratios to the grid |
| By ear | about 85–90 % recognisably the same | about 65–75 % the same, 15–25 % close, about 10 % off | about 40–60 % |

## 7. CLI

```
just import <FILE>... [-- OPTIONS]

chimera-import [OPTIONS] <FILE>...
  --card DIR        card root to write to (default $CHIMERA_CARD, else ./chimera-card,
                    the desktop sim's card); Sounds go to DIR/CHIMERA/SOUNDS
  --voices LIST     a subset of each bank's slots, 1-based: 1,4,9-12
  --dry-run         parse, map and report; write nothing
  --format F        dx7 | tx81z: settles a headerless 4096-byte bank (§ 5)
  --level L         bank (default) | voice: VOLUME keeps the bank's relative
                    levels, or puts every voice on the reference (Q1)
  --previews DIR    also write a WAV preview per Sound (C2, C4, C6 at velocity 100)

chimera-import ab --refs DIR [--write-features] [--controls]
                    the Dexed A/B (§ 8.4); a dev tool, not for library use
```

- **One run:** parse each file, then map each voice. Render it offline at C4, velocity 100, and set VOLUME (§ 4). Run the QA metrics (§ 8.7), then write the Sound and append to the report. A voice that fails QA is not written, and its report line says why.
- **Report:** printed to stdout and written as `DIR/IMPORT/<file stem>.txt`, next to the Sounds, so it travels with the card. One line per voice:

  ```
  ROM1A 05 "E.PIANO 1" -> "E-PIANO 1"  S0000123  ALG 5->T5 exact  VOL -3.1 dB
    approx: op3,op4 detune-beat C4 2.6 Hz -> 0.7 Hz; op2 key-level RD -LIN 41, worst -4.2 dB
    dropped: lfo-delay 22
  ```

  Then one summary line per file: voices imported, skipped as duplicates, refused, and the warnings (checksum).
- **Exit status:** 0 when every file parsed, even with warnings or skipped voices; 1 if any file was refused; 2 for a usage error.
- **Guard:** the CLI refuses a `--card`, `--previews` or `ab --refs` path inside the Chimera git work tree, so raw ports can't land in the repo by accident (§ 9).
- **No `--strict`, no `--mutate`, no `--randomize`.** The owner ruled checksums warn, and mutation is design work (§ 9).

## 8. Validation and tests

### 8.1 Parsers
- Every format parses a fixture built in test code from field values; real banks are never committed. The **DX7 INIT VOICE** is also checked as literal bytes from the DX7 manual's defaults, so the parser isn't tested only by its own encoder.
- A wrong size, header or format byte gives the typed `ParseError`. A bad checksum gives `Warning::Checksum`, and the voices still come out. The ACED checksum test covers its 10-byte header.
- Masking before clamping, the headerless 4096-byte disambiguation (both directions, and the ambiguous case), DX11/V50 extra bytes, and PreenFM3 NaN and infinity each have a test.
- **Never panic:** a proptest over arbitrary bytes and over mutated valid dumps, plus cargo-fuzz targets `parse_dx7`, `parse_tx` and `parse_pfm`, run by `just fuzz` beside the decoders.

### 8.2 Mapping tables as data
- **`algo_map_test`:** for each DX7 algorithm, the per-voice search with every OL at 99 reproduces § 6.1's table against the pool in force.
  - Every exact row is re-checked as a graph isomorphism against `ALGORITHMS`.
  - Once #338 lands, every target is in its curated set.
- **Anchor tests:**
  - R 99 → AR 31; OL 99 carrier → LEVEL 99; DX7 FB 7 → FDBK 6, FB 1 → FDBK 0; TX81Z FBL n → n;
  - integer ratios to within 0.4 c;
  - a fixed 261.63 Hz main carrier → key 60, ratio 1.00 at TRNSP 0; a 3 Hz fixed modulator → ratio 0.5 and `fixed-subaudio`;
  - every rate and level table is monotonic.
- **Logging completeness:** for each `LossKind`, a voice built to trigger it logs it, and the same voice with that field neutral logs nothing.
- **The TX81Z round trip:** for every E row, a TX81Z voice maps to a Sound whose fields equal the input.

### 8.3 The reference engine

**One engine for both tables and renders: msfa, the "Modern" engine.**
- Its tables (the rate law, the output-level and velocity tables, the feedback law) come from msfa at [google/music-synthesizer-for-android](https://github.com/google/music-synthesizer-for-android) (Apache-2.0), cited from that repo, not from Dexed's copy.
- The renders come from Dexed set to its **Modern** engine, which runs that same msfa core.
- **Not Mark I.** Dexed's Mark I engine is closer to the hardware, but it differs from msfa: its own level path, `compute_fb3`/`fb2` feedback on algorithms 4 and 6 with `fb_shift + 2`, and +2 on algorithm 32. Calibrating against msfa's tables and rendering on Mark I would fit two engines at once. Mark I is GPL-3, and nothing is taken from it.
- **What this costs:** the A/B measures fidelity to msfa, not to a DX7. Where msfa and the hardware are known to differ (the loop feedback of 4 and 6), the map logs the loss against the hardware anyway (§ 6.1).

**Pinned:**
- **The Dexed release: v1.0.1.** It is recorded in `tools/dexed_render.py` and in each feature file's header, and a feature file from a different release is refused. Moving to another release means re-rendering every reference and re-deriving the thresholds.
- **Dexed's global settings:** engine Modern; sample rate 48 kHz; master tune 0; mono off; pitch-bend range 0; output at its default; no MIDI CC mapping.
- **Loading:** Dexed keeps its cartridge and current program in its plugin state. The script reads the VST3's `raw_state` through Spotify's `pedalboard` (GPL-3, dev-only, never linked), writes the cartridge and program into it, and sets it back.
- **The load proof gates step 5 (§ 10).** Before any reference is rendered, the script must show that the load works:
  - it renders two authored voices that differ in one field (ALG, then OL);
  - the two renders differ, and each matches its own expected first-partial frequency within 1 c;
  - a third render, of the plugin's default patch, differs from both.

  So a silent fallback to Dexed's default cartridge can't pass as a reference.

### 8.4 Calibration and the Dexed A/B (DX7)

**Calibration** (host, `#[ignore]`, run by hand) fits `K_d`, `K_a`, `K_m`, the FB law, the KVS → VEL map, and the LFO speed, PMS, AMS and PEG tables against Dexed renders of authored single-feature voices: one decay at each of 8 rates, one modulator at each of 8 levels, and so on. Each fitted value lands as a `const` with its fit error in a comment, and the ADR that accepts the importer records it. msfa tables are credited in `THIRD_PARTY.md`.

**Reference set:** 16 authored DX7 voices, built in test code and covering:
- a two-operator stack at OL 70, and one at OL 99;
- an E-piano (ALG 5, detuned pair, KVS);
- a bell (ALG 1, fine ratios);
- a brass (rising L2, PEG);
- a bass with FB 6;
- an organ (ALG 32);
- a pluck (L3 = 0);
- a two-slope pad;
- key scaling −LIN and +EXP;
- an LFO vibrato and an LFO tremolo, both with key sync on (below);
- a fixed-frequency percussion voice;
- a ratio over 27.57;
- the loop-feedback algorithm 4.

**These authored voices are the whole DX7 reference set.** They are ours, so they can be rendered, measured and committed with no rights question. No Yamaha DX7 bank is used: the owner's private reference set is TX81Z (§ 8.5), and nothing else is needed to exercise every DX7 row.

**The render protocol:**
- Each voice plays notes 36, 60 and 84 at velocities **40, 100 and 127**, so KVS is exercised. Each note is held 1.0 s, then released for 1.5 s, at 48 kHz.
- The WAVs live in `$CHIMERA_REFS`, outside the repo. `chimera-import ab --write-features` reduces them to a feature file per voice, a few KB each, committed under `chimera-desktop/tests/import_ab/`. CI runs the A/B without Dexed.
- Chimera's side renders each import offline with the same protocol: FX off, filter open.
- **LFO voices are synced for the A/B.** Dexed's LFO is global and free-running, so an unsynced LFO would start at a different phase on every render. The LFO reference voices have LKS on, and each note is rendered from a freshly loaded plugin state, so Dexed's LFO starts from its reset phase at the note-on. Chimera's side uses RETRIG. An LFO voice with LKS off in a real bank is still imported as FREE, and is compared with the LFO metrics masked.
- Both sides are normalised to the same integrated loudness per voice (at velocity 100), with the same gain then applied to the other velocities, so velocity response is compared too.

**Metrics per voice, note and velocity:**
- **Attack:** the time to reach −3 dB of the peak.
- **Decay:** the times after the peak at which the broadband envelope (10 ms RMS frames) crosses −6, −20 and −40 dB.
- **Release:** the time from key-off to −40 dB.
- **Level:** the peak level, in dB, against the reference's.
- **Spectrum:** 32 ERB bands from 50 Hz to 16 kHz, in dB, floored at −60 dB under the frame's loudest band. For each band, take the median over the frames of the first 1.0 s of the dB difference. The voice's spectral error is the **maximum over the bands**, so a single missing or extra sideband shows.

**Thresholds are derived, not chosen.**
- **The noise floor:** each reference voice is rendered twice by Dexed under conditions that should not matter: host block size 64 against 512, and the note-on shifted by 37 samples. Per metric, the floor is the 99th percentile of the differences over the whole set.
- **The threshold** per metric is that floor plus the map's stated bound for that quantity: ±19 % on attack and decay times, ±41 % on release, 1.5 dB on level. The spectral threshold is the floor plus 1.5 dB.
- The script computes the thresholds once and writes them into the feature files' header. They are fixed before the first Chimera comparison, so they can't be tuned until things pass. Changing them takes a new ADR.

**Negative controls** prove the A/B can catch a wrong map, for **both renderers** (this one and § 8.5's). With `--controls`, the harness re-runs the whole reference set with one fault injected into the map at a time:
- modulator level: DX7 `K_m` ± 4; TX81Z modulator OUT ± 4;
- feedback: DX7 FB ± 1; TX81Z FBL ± 1;
- D1R + 2 on every operator;
- the ratios of two operators swapped.

Each control must make **at least one voice fail** that passed without it. Every voice the fault touches (FB controls: the voices with FB > 0) must fail at least one metric. If any control passes, the thresholds are too loose and the harness fails, whatever the clean run scored.

**The gate:**
1. **The harness proves itself on real modulation:** the two-operator stacks at OL 70 and 99, and the FB 6 bass, are exact or C on every row. Each must pass every metric with half the threshold to spare. If not, the harness or the calibration is wrong, not the map.
2. **The loss report is the oracle:** every reference voice with nothing logged must pass every metric at every note and velocity. A failing voice with nothing logged is a bug in the map or in the logging.
3. **Every negative control fails,** as above.
4. The pass rate over the whole set is printed. The ADR that accepts the importer records it.

### 8.5 The ymfm A/B (TX81Z)

Dexed is a DX7 engine: it can't play the TX81Z's waves, its four-operator algorithms as the TX81Z wires them, or its ratio table. **The TX81Z reference renderer is ymfm's OPZ core**, an emulation of the YM2414, the chip in the TX81Z.

- **Licence:** BSD-3-Clause (Aaron Giles, 2021), stated in the README and in `src/ymfm_opz.h`. Allowed host-side by the owner's earlier answer. It is used in dev tooling only and never linked into firmware, and it is listed in `THIRD_PARTY.md` with its notice if vendored.
- **Pinned:** [aaronsgiles/ymfm](https://github.com/aaronsgiles/ymfm) at commit `81aec25ccbb98f4873a255f7551ac4dadac59b4a` (HEAD on 2026-10-04; the repo has no release tags). The README warns that the interface "is still a bit in flux", so the harness builds against that commit only, and the commit is written into each feature file's header.
- **Its accuracy claim** is "extremely high (audibly indistinguishable)", not bit-exact, and for OPZ that claim is weaker. `ymfm_opz.cpp`'s own header says:
  - **fixed-frequency mode:** "not sure how that works at all, so it's not implemented";
  - **FINE, EG shift and REV:** "mystery fields", with EG shift's behaviour "unknown";
  - the channel volume is "unimplemented", and the panning is "a guess";
  - the 8 waveforms and the LFOs (with sync) are implemented.
- **The harness:** `tools/opz_render/`, a small C++ host program over ymfm. It turns a TX81Z voice into OPZ register writes (our own code, from the TX81Z and YM2414 documentation), clocks the chip at 3.579545 MHz (a native rate of about 55.9 kHz), and resamples to 48 kHz. The render protocol, metrics, LFO sync and threshold derivation are § 8.4's, with the noise floor measured on ymfm the same way.
- **What the A/B covers, given ymfm's gaps:**

  | Feature | ymfm OPZ | The A/B |
  |---|---|---|
  | Waves W1–W8, ALG, rates, levels, CRS, DET, FBL, LFO | implemented | compared on every metric |
  | FINE > 0 | a "mystery field" | Before any comparison, a **FINE proof** renders one operator at every CRS × FINE that the reference voices use and compares its measured frequency to the manual's ratio table (`tx.rs`). If every one is within 2 c, FINE voices are compared in full. Otherwise their spectral metric is masked, the time metrics still apply, and the report lists them as `ref-unverified`. |
  | EG shift ≠ 0, REV > 0 | unknown | Those voices' decay and release metrics are masked (`ref-unverified`). Their loss is logged anyway (`eg-shift`, `rev-rate`). |
  | FIX = 1 | not implemented | Those voices are left out of the audio A/B and listed. The fixed → ratio map is checked by § 8.2's anchors and by ear in UAT. |

  A masked metric never counts as a pass: the gate counts only the metrics that were compared, and the report prints how many were masked.
- **Reference sets:**
  - **Public, in CI:** authored TX81Z voices built in test code (INIT; each of W1–W8; each ALG; FBL 7; LS; KVS; an LFO voice). Their ymfm feature files are committed under `chimera-desktop/tests/import_ab/`.
  - **Private, the owner's ruling (2026-10-04):** the **TX81Z factory banks**, kept in `$CHIMERA_BANKS`. Their renders and feature files stay in `$CHIMERA_REFS`, never in the repo, because the features derive from Yamaha's voices. This A/B runs locally (`#[ignore]`), and its pass rate goes into the ADR that accepts the importer.
- **The gate is § 8.4's**, applied per renderer: the self-test (a two-op stack and an FBL 7 voice, half margin), every unflagged voice passes, and every negative control fails.
- **What it measures:** the map plus ALGO's own TX fidelity against ymfm's. ALGO's waves are our recipes (ADR 0023), so a wave-shape difference shows as spectral error. The calibration fixes it either by measuring that floor per wave and adding it to the spectral threshold, recorded in the feature header, or by filing it under #40. It never tunes the map to hide it.

### 8.6 Golden conversions
- For each reference voice and each TX81Z fixture (INIT; W2–W8 with FINE; LS and EG shift; a DX21 bank with ACED zeros; fixed), a committed text snapshot holds the mapped Sound's parameters and routes and the loss lines. A change to the map shows up as a reviewed diff.
- **Round trips:** syx → Sound → `encode_sound` → `SoundDecoder` → `bits_eq`. And importing the same bank twice writes nothing the second time.
- **Opt-in real-bank tests** (`#[ignore]`, read from `$CHIMERA_BANKS`): every voice of every bank there imports without a refusal, and the PreenFM3 version-word offset is checked against a stock-written bank.

### 8.7 QA before UAT
Every import runs the existing sweep metrics on its render: the level window, no DC, no clicks, no stuck notes (§ 7). Then the owner listens to the WAV previews bank by bank, which is UAT by ear.

## 9. The factory library method and legal posture

**The method, the owner's ruling:**
1. Port the owner's TX81Z banks with `just import --card <private dir>`. The private dir is outside the repo, and the guard enforces it.
2. **Design a mutation for each ported Sound:** a stated change of character, such as swapping op 2 to W5 and bringing LFO 2 into MORPH. This is done by hand or by a scripted edit with a one-line intent per Sound. **Never at random:** the importer has no mutate or randomise feature, and none is planned.
3. **Commit only the mutated result, and not to the public repo before V1.** Mutated derivatives of Yamaha voices go to a private repository (or a branch that is never pushed to `joegiralt/chimera`) with their intent lines, until the owner's V1 licensing decision. A raw port is never committed anywhere. #317's public factory set meanwhile holds only Sounds designed from scratch.

**What keeps raw data out:**
- `.gitignore` gains `*.syx`, `*.SYX`, `*.bnk`, `*.mix` and `*.wav`. None is tracked today.
- `just check` fails if `git ls-files` lists any of those extensions.
- The CLI's work-tree guard (§ 7).
- Fixtures are built in code from authored field values. Calibration renders and WAVs stay in `$CHIMERA_REFS`, and only the derived feature numbers are committed.

**Code provenance.** The repo was then MIT (GPL-3.0-or-later since ADR 0330). Dexed (both engines, Mark I included), Hexter and the PreenFM3 firmware are GPL: we read them only to understand, and copy no code or tables. Format facts come from Yamaha's published charts. Tables taken from msfa are cited from google/music-synthesizer-for-android (Apache-2.0), not from Dexed's GPL copy, and credited in `THIRD_PARTY.md`. ymfm (BSD-3-Clause) is the TX81Z reference renderer, host-side only (§ 8.5).

**Not decided here:** whether a mutated derivative is clear of Yamaha's rights. That is the owner's call at V1, along with the other licensing moves.

## 10. Build order

Each step lands with its tests. Nothing waits on #338.

1. **TX81Z parsers** in `chimera-core/src/import/`: `Dump`, `ParseError`, `Warning`; VMEM (bit map checked against the manual's chart), VCED, ACED, DX21/27/100, DX11/V50 extras; masking; proptest and fuzz targets; the `.gitignore` entries and the `just check` guard.
2. **The TX81Z map and the shell** in `chimera-desktop`: § 6's TX81Z rows, `Loss`/`LossKind`, the name map, the CLI with the render, VOLUME, QA, the report, the dedupe and the work-tree guard. The metrics move into `render_metrics`. TX81Z goldens, round trips and the logging-completeness tests.
3. **The ymfm harness** (§ 8.5): the pinned commit, the FINE proof first, the noise floor and thresholds, the public authored set in CI with its negative controls, then the private factory-bank run.
4. **#317's TX81Z pass can start:** the owner's banks are ported to a private card and the mutation pass begins, kept private (§ 9).
5. **DX7 parsers and the Dexed harness.** VMEM, raw 4096 (with the headerless disambiguation) and VCED, with fuzz. `tools/dexed_render.py`, and **the load proof first** (§ 8.3); nothing else in this step proceeds until it passes. Then the noise floor and thresholds, `ab --write-features`, the reference set, and the calibration.
6. **The DX7 map** against today's pool: the per-voice search, the EG fold, the ratio fit, fixed → ratio, FB, KVS, key scaling, the LFO and the PEG. Goldens; the A/B gate with its negative controls in CI.
7. **When #338 lands:** the pool becomes the curated set, `algo_map_test` gains the curated-set assertion, and the owner's DX7 imports are re-imported.
8. **PreenFM3.** `.bnk` and `.mix` parsers (with the float checks) and § 6.8's map, imported as PM and flagged.
9. **Later, each with its own issue and ADR:** on-device import (after #336), SysEx receive (#263, reusing step 1's parsers), LFO delay (needs DEPTH as a destination), EG shift (#40), controllers and porta (STEAL GLIDE, ADR 0065), and a finer DX-style ratio mode if the owner wants one.

## 11. Owner decisions

All settled: the fixed → ratio reference note is chosen per voice (§ 6.4); the algorithm is chosen per voice (§ 6.1); the A/B thresholds are measured from each renderer's own noise floor (§ 8.4, § 8.5); the private reference set is the TX81Z factory banks (§ 8.5); and **loudness keeps a bank's relative levels**: its median voice lands on the −22 LUFS reference and the others keep their levels relative to it, as ADR 0063 keeps the factory Sounds' own (the owner, 2026-10-04). `--level voice` remains for putting every voice on the reference.
