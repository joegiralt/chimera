# 0088. Import DX7, TX81Z and PreenFM3 patches on the host, as ordinary Sounds on the nearest algorithm

- **Status:** Proposed
- **Deciders:** project owner (rulings on #302, 2026-10-02 and 2026-10-03); firmware
- **Relates to:** ADR 0022 (one algorithmic engine; T1–T8 are the TX81Z's
  eight), 0023 (waves from our recipes), 0026 (cost by patch shape), 0042
  (pitch is a matrix destination), 0045 (card format), 0063 (the loudness
  reference); #302, #338, #317, #336, #263

## Context
Parity with stock (#301) means loading DX7 banks, which the stock firmware
does. The owner also wants TX81Z and PreenFM3 patches, and wants the TX81Z
banks to seed the factory library (#317). ALGO is a six-operator PM engine
shaped on the TX81Z: its rates, levels, ratios, waves and feedback law are
the TX81Z's. The DX7 is PM too, but its envelope, level and ratio laws
differ. Its 32 algorithms are not all in ALGO's set, which #338 is cutting
down. Flash is at 89.8 % (#336). The spec is
`docs/superpowers/specs/2026-10-04-patch-importer-design.md`.

## Decision
- **Host first, no new crate.**
  - The parsers live in `chimera-core/src/import/`: `no_std`, no
    allocation, no I/O, and shared with SysEx receive (#263).
  - The maps are pure functions in `chimera-desktop/src/import/`.
  - A `chimera-import` binary in `chimera-desktop` writes ordinary ADR 0045
    Sound files to the card library through `DirStore`, and renders each
    import to set VOLUME and run the QA metrics. `just import` runs it.
  - The firmware does not link the importer in v1. On-device import is in
    scope later, under its own ADR, after #336.
- **Formats:**
  - DX7: VMEM (4104 B), raw 4096 B banks and VCED singles;
  - TX81Z/DX21/DX27/DX100: VMEM, VCED and ACED. DX11/V50 extra bytes are
    ignored and logged;
  - PreenFM3: `.bnk` and the instruments of `.mix`.

  A headerless 4096 B file is resolved by plausibility, or by `--format`
  when both layouts fit.
- **Parsing:**
  - A bad checksum warns and imports, as Dexed does. ACED's checksum covers
    its 10-byte header.
  - Packed bytes are masked, then clamped, and the change is logged.
  - PreenFM3 NaN and infinite floats take their defaults.
  - A wrong size, header or format byte refuses the file.
  - Nothing panics: proptest and cargo-fuzz cover every parser.
- **TX81Z maps to ALGO's TX shapes:**
  - ALG n → Tn, and rates, levels, CRS, waves (OSW → W1–W8) and FBL are
    exact;
  - KVS, DET and FINE are converted: `FINE_TOP` is up to about 14 c off
    the TX81Z's steps;
  - EG shift is dropped.
- **There are no new algorithm shapes.** A DX7 voice is mapped per voice:
  - prune the operators that are silent at every key and velocity;
  - search every algorithm in the pool, with all 720 operator permutations;
  - minimise the modulation lost or added, weighted by output level;
  - break ties to exact, then T before A, then the lower AlgoId.

  v1's pool is today's 32. When #338 lands, the pool becomes its curated
  set, and DX7 imports are re-imported onto it.
- **There are no fixed-frequency operators.** A fixed operator becomes a
  grid ratio relative to a per-voice reference key: the key nearest the
  main fixed carrier, or note 60 when only modulators are fixed. Every
  case is logged:
  - a ratio under 0.5 clamps to 0.5;
  - a fixed-derived ratio is capped below Nyquist at note 108;
  - an all-fixed voice imports, playing its own sound on the reference key.
- **DX7 laws are converted from msfa's** (Apache-2.0, cited from
  google/music-synthesizer-for-android):
  - rates `q = ⌊41R/64⌋` less the offsets `K_d` and `K_a`;
  - the level table, less the modulator offset `K_m`;
  - FB n → FDBK n − 1;
  - a fitted KVS map.

  The offsets and the KVS map are calibrated against Dexed renders.
- **Every field is one row of one kind:** exact, converted, approximate or
  dropped. A converted value that leaves its stated bound is logged. These
  are all logged:
  - rate clamps, and RS 0's `x >> 3`;
  - beat-rate errors between near-equal ratios;
  - the VEL depth cap (21 dB against the DX7's ~78);
  - key scaling's worst-case error.

  Key scaling is one NOTE → LEVEL line, fitted for the least worst-case
  error: never boosting above the source on a modulator, centred on a
  carrier. The rule: **a voice with
  nothing logged must pass the A/B.**
- **Losses go in an import report,** printed and written beside the
  Sounds. The Sound file format does not change.
- **Imports leave ENV 1, ENV 3, LFO 2 and LFO 3 free** for DX7 and TX81Z.
  PreenFM3 imports use their own LFOs.
- **The PreenFM3 arpeggiator, step sequencers, macros and mixer state are
  dropped** for v1.
- **No legacy promise:** an imported Sound loads safely, and is clamped and
  validated like any Sound. A later importer may map the same voice
  differently.
- **Validation:**
  - **One reference engine:** msfa, as Dexed v1.0.1's Modern engine, for both the
    tables and the renders. Dexed's tag, global settings and `raw_state`
    cartridge loading are pinned, and a load proof gates the harness.
  - **Golden snapshots** of each mapped Sound and its loss list, and the
    TX81Z field round trip.
  - **The A/B:** 16 authored DX7 voices at notes 36/60/84 and velocities
    40/100/127, LFO voices key-synced and rendered from a fresh state. It
    compares attack, decay and release crossing times, peak
    level, and the per-band maximum ERB spectral difference.
  - **Thresholds** are Dexed's own noise floor (block size and offset
    changes) plus the map's stated bounds, fixed before the first
    comparison.
  - **TX81Z:** ymfm's OPZ core (BSD-3-Clause, pinned at commit
    `81aec25c`) is the reference renderer, with the same protocol, metrics
    and gate. ymfm does not implement fixed frequency and calls FINE, EG
    shift and REV "mystery fields". So FIX voices are left out, EG-shift
    and REV voices have their envelope metrics masked, and FINE voices are
    compared in full only after a frequency proof against the manual's
    ratio table.
  - **Reference sets:** authored DX7 and TX81Z voices, ours and committed,
    run in CI. The owner's TX81Z factory banks are the private set
    (2026-10-04), run locally, with their renders and features kept out of
    the repo. No Yamaha DX7 bank is used.
  - **Negative controls** must fail on both renderers: modulator level ± 4
    (`K_m` or OUT), feedback ± 1, D1R + 2, and a swapped ratio.
  - **Self-test:** voices with real modulators must pass with half the
    margin.
- **Factory method:**
  - TX81Z voices are ported into a private card, then mutated one by one by
    design, never at random.
  - Until the V1 licensing decision, the mutated derivatives stay in a
    private repository. Raw ports are committed nowhere.
  - Bank files and WAVs are git-ignored, `just check` rejects them, and the
    CLI refuses to write inside the work tree.

## Alternatives considered
- **On-device import first:** 8–15 KB of flash Chimera doesn't have, and it
  can't render, so it can't set loudness or run QA before the owner
  listens.
- **A separate `chimera-import` crate:** one more crate, for a split that
  the core/desktop line already gives. The parsers are the only part the
  device needs (#263).
- **Append the DX7's ten missing topologies as new algorithms:** the owner
  ruled it out. #338 is cutting the set, not growing it.
- **Wait for #338 before the DX7 map:** it blocks the work on curation. The
  no-legacy ruling makes re-import after #338 cheap.
- **A fixed DX7-algorithm → ALGO-algorithm table:** simpler, but it misses
  exact matches that open up when a voice leaves operators off.
- **Note 60 as the reference for every fixed operator:** it puts drum and
  effect voices off their pitch on the key where they're most likely
  played.
- **Dexed's Mark I as the reference:** closer to the hardware, but it
  differs from msfa's level path and from its feedback on 4, 6 and 32. It
  is GPL, and its tables couldn't be used, so the tables and renders would
  come from two engines.
- **Thresholds chosen by hand, or a median spectral error:** a wrong map
  could pass. Negative controls, a per-band maximum and a measured floor
  make the gate falsifiable.
- **Refuse bad checksums:** too many real files carry them. Dexed loads them
  anyway.
- **A provenance record on the Sound:** `Origin` is already a Part's record
  (ADR 0045), the owner chose the report, and a codec change isn't needed
  for v1.
- **Commit the mutated derivatives publicly now:** a licensing move before
  V1, which the owner has ruled out.

## Consequences
- The importer costs 0 B of flash. The firmware is unchanged.
- TX81Z import, and #317's private seeding, can start at once. The DX7 map
  ships against today's pool and is re-imported after #338. PreenFM3 comes
  last, imported as PM and flagged until #38.
- The A/B measures fidelity to msfa, not to a DX7. The known msfa-hardware
  gaps (the loop feedback of 4 and 6) are logged anyway.
- Fixed-frequency voices track the keyboard from their reference key.
  Ratios over 27.57 and sub-audio fixed modulators sound different. All of
  it is logged.
- The same DX7 algorithm number can land on different ALGO algorithms in
  different voices.
- #317's public factory set holds only Sounds designed from scratch until
  V1.
- Lost in v1: LFO delay (DEPTH isn't a destination), EG shift, controller
  assignments, porta and mono, key-scaling curves beyond one fitted line,
  and the PreenFM3 arp and sequencers. Each is a follow-up issue.

## Sources
- The spec above; #302 and the owner's rulings in it; #338; the 2026-10-04
  adversarial review of the first draft.
- Yamaha DX7 and TX81Z owner's manuals, MIDI data charts (VCED, VMEM,
  ACED).
- ymfm, github.com/aaronsgiles/ymfm at 81aec25c (BSD-3-Clause):
  `src/ymfm_opz.cpp`'s notes on what OPZ leaves unimplemented.
- msfa, google/music-synthesizer-for-android (Apache-2.0): the rate law,
  the level, velocity and feedback laws (detune: a constant
  `12606·(DET − 7)`). Dexed's pitch-dependent detune
  (`Source/msfa/dx7note.cc:61`, GPL) is used as measured behaviour only:
  the detune law is fitted from renders.
- Dexed `Source/PluginData.h` `Cartridge::load` (GPL-3, read only) for
  sizes and the checksum precedent. Dexed run unmodified, in its Modern
  engine, as the A/B reference; Mark I is read only, to know how it
  differs.
- `Ixox/preenfm3` `firmware/Src/filesystem/PreenFMFileType.h` and
  `Src/synth/Common.h` (GPL-3, read only).
- The Digitone manual, Sound Manager and SysEx receive: the
  host-library-then-pool precedent.
- `chimera-core/src/dsp/algo/{algorithms,tx,env,kernel,engine}.rs` and
  `storage/sound.rs` at 2e3cb4dc.
