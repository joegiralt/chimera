# 0216. Give every stored parameter a frozen NRPN address derived from its disk identity

- **Status:** Proposed
- **Deciders:** owner (0.3 scope: "perfect" MIDI and controllers), firmware

## Context
Sequencers and editors keep static files that name a synth's parameters by number: Hapax `[NRPN]` lines, Cirklon `.CKI`, OXI `.oxiindef`, the midi.guide CSV, Ctrlr and Edisyn panels. When a firmware renumbers, every user's files break without warning. The research report (`reports/MIDI controller and USB schemes.md`) names renumbering as one of the three reasons MIDI schemes fail in the wild, and recommends one versioned address space with NRPN as the canonical numbering.

Chimera already freezes a parameter's identity:
- ADR 0045 gives every stored block a permanent disk code (`BlockRef::disk_code`), and every param a permanent `ParamId` within its block.
- `chimera-core/tests/fixtures/disk_codes_v1.txt` pins `(code, ident)` for blocks and `(id, ident)` for params. Only appending is allowed.
- Retired params keep their codes (`storage::RETIRED`).

ADR 0128 and 0139 rule out a global CC chart: CCs belong to each Sound's map.

## Decision
- **An NRPN address is derived, not tabled.** NRPN MSB = the block's disk code. NRPN LSB = the `ParamId`.
  - Every disk code is asserted < 64 and every `ParamId` < 128.
  - MSB 64–126 is reserved for future global, system and FX spaces. MSB 127 is never used (127/127 is the null).
  - `chimera_engine::nrpn::NrpnAddr::of(ParamAddr)` and `NrpnAddr::addr` are the only conversions.
- **Each param has a value law, derived from its `ParamSpec`:**
  - `Place14` (continuous): 14-bit over the normalised span. An MSB alone is bit-replicated (`m<<7 | m`) so 127 reaches the top; the LSB refines.
  - `Code7` (a choice with frozen enum codes): MSB = the disk code, LSB ignored. A choice with a code past 7 bits (THEME BLACK stores −2 and −1 as 254 and 255) is `Index7` instead, listed in `nrpn::WIDE_CODES`.
  - `Index7` (other discrete params, ≤ 128 values): MSB = steps from min.
  - `Index14` (discrete params, > 128 values): 14-bit steps from min.
- **The chart is pinned.** `chimera-core/tests/fixtures/nrpn_v1.txt` holds one line a stored param: `N <msb> <lsb> <BLOCK_IDENT> <PARAM_IDENT> <law> <W|R>`, then one `X <msb> <lsb>` line a retired address.
  - `nrpn_chart_test` (in `just check`) fails when a line changes or disappears.
  - Appending is allowed: a new line may land anywhere (the chart is block-ordered), and the old lines keep their order.
  - A retired param's `N` line gives way to an `X` line at the same address, which is never reused.
  - A change of law counts as a renumber.
- **What can be written.** In 0.3 Phase 1, writes are accepted for ADR 0139's `Mappable` set (`W`), on the Part's channel, with jump takeover, through the same ramp and budget as a mapped CC. Every other address is reserved and inert (`R`). A later ADR may open FX on a global channel.
- **Canonical NRPNs need no binding.** A per-Sound binding on the same NRPN number wins over the canonical meaning (ADR 0217). An address the Part's engine doesn't read is ignored.
- **RPNs are the standard's.** RPN 0 sets the Part's BEND (ADR 0128): MSB semitones, clamped to 0–24. RPN 1, 2 and 5 are ignored in Phase 1. RPN 6 (MPE) belongs to MPE's ADR.
- **Generated outputs come from the same function:** `chimera-utils nrpn-chart` in Phase 1, and the Hapax, midi.guide, Ctrlr and Edisyn generators in Phase 3.

## Alternatives considered
- **A hand-kept NRPN table per engine.** It is a second source of truth that drifts from the specs. The disk identity is already frozen and fixture-checked.
- **Mirror the stock PreenFM3 NRPN map.** It only fits an FM Part with stock pages, and its numbering is a row-of-four-encoders layout, not an identity. Chimera's own editor story serves parity better (report § Editors).
- **A fixed CC chart.** Ruled out by the owner (ADR 0128).
- **Display-order indices for choices.** Reordering a list would silently change what a sequencer's stored value selects. Disk codes never move.
- **Pure 14-bit for every param.** 7-bit NRPN senders (most sequencer definitions) put the value in the MSB, so discrete params would land on step `m·128`.

## Consequences
- Every new stored param gets an address the moment it gets a `ParamId`. Adding one appends a fixture line, and nothing is renumbered by hand.
- Block codes have 63 slots left (36 used), and params 128 per block.
- An LSB-only 7-bit sender can't move a discrete param. The chart documents MSB.
- Stock PreenFM3 NRPN editors drive the wrong params on Chimera: MSB 0 and 1 are stock's rows but Chimera's block codes. This is documented, not mitigated.
- The fixture is one more file a param PR touches. Its test message says how to append.

## Sources
- `reports/MIDI controller and USB schemes.md` § Editors, PE and sequencer files; § Recommended phased plan (Phase 1).
- `research_notes/MIDI controller and USB schemes/midi_core_mpe_relative.md` (RPN list, increment/decrement), `sysex_midici_definitions.md` (stock PreenFM3 NRPN, Hapax format).
- ADR 0045 (frozen codes), 0128, 0139, 0217.
- Spec `docs/superpowers/specs/2026-10-25-midi-phase1-design.md` § 5. Plan `docs/superpowers/plans/2026-10-25-midi-phase1.md`.
- Elektron per-track fixed NRPN (Elektronauts, "Does the Digitakt have parameter feedback"); Squarp Hapax instrument definition template (squarp.community).
