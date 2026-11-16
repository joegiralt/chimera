# 0275. Bills from the 2026-11-16 unit readings

- **Status:** Proposed
- **Deciders:** owner (approved 2026-11-16), firmware

## Context
PR #994 billed DRUM by machine (ADR 0274), two of whose constants were estimates or margins thin at the top, and kept MODAL's SYMP and BOWED at ADR 0211 and 0218's figures. The unit read them again on 2026-11-16 (rev V, 480 MHz, build 0.3.0 eb442a48, INIT DRUM and INIT MODAL on Part 1, `stats` load_pct polled every 51 ms, 1 % = 100 cycles a sample, whole voice over idle; #991, #992):

| Row | Reading (cycles a sample) | Spread |
|---|---|---|
| PLAIN IMPULSE, DECAY 1.12 s, SWEEP 0 and 64, plateau | 500 | flat in all five |
| PLAIN IMPULSE, short first windows (65 ms hit) | 400–600 | up to 600–700 at SWEEP 64 |
| TONE COWBELL BUMP 0 | 600 | 600–700 |
| TONE COWBELL BUMP 127, sweep head | 1,400 median | 1,200–1,500 |
| TONE COWBELL BUMP 127, worst block (`peak_pct` 32 %) | about 2,000 | single running max |
| MODAL SYMP, C4 / C6 / C7 | 770 / 770 / 870 | max window 850–900 |
| MODAL BOWED, C4 | 650–700 | max window 750 |

## Decision
Each bill takes the top of its reading's spread, as ADR 0274 and 0211 do.
- **PLAIN IMPULSE 1567 → 577.** `Machine::cost` = ceil(1.1 × reading) − 83 (ADR 0274), reading 600: ceil(660) − 83 = 577. The sustained reading is 500; 600 is the short windows' top. This replaces ADR 0274's estimate (1500 from desktop ns, pad 3 read 1200 in a short window on the pre-#994 code).
- **TONE COWBELL 1457 → 1567.** Reading 1500, the BUMP 127 sweep head's top: ceil(1.1 × 1500) − 83 = 1650 − 83 = 1567. The old 1400 left that head only its 10 %.
- **MODAL SYMP `COST_SYMPATHETIC` 645 → 740.** The model bill excludes the rest of the voice (chain, PITCH, SYMP's halo retune), which is `Voice::cost` less `ModalEngine::cost` on INIT MODAL (body 0, ensemble 0): 781 − 645 = 136. C7's whole voice reads 870: 870 − 136 = 734, rounded up to 10 (ADR 0211): 740. The whole voice bills 876.
- **MODAL BOWED `COST_BOWED` 530 → 640.** Rest of the voice 643 − 530 = 113. The max window reads 750: 750 − 113 = 637, rounded up to 10: 640. The whole voice bills 753.
- **Unchanged.** `Machine::MAX_COST` is METAL JINGLE's estimate (1677), so it does not move. `Kit::DEFAULT`'s costliest machine, which sets a Part's bill, was PLAIN IMPULSE (1567) and is now TONE COWBELL (1567): the same number. The other machines, INIT DRUM's 83 and ADR 0274's formula are as before.
- **Pinned by tests.** `drum_bill_test::a_read_machine_bills_its_reading` holds every read pad's machine at ceil(1.1 × `READ`) − 83; `cost_test::modal_symp_and_bowed_bill_the_2026_11_16_readings` holds each whole voice at or above its reading and its model term at ceil10(reading − rest); `modal_bills_each_model` holds the voice counts.

This supersedes in part ADR 0274 (PLAIN IMPULSE's estimate, TONE COWBELL's constant), ADR 0211 (SYMP's 645) and ADR 0218 (BOWED's 530, SYMP kept at 645).

## Alternatives considered
- **Bill IMPULSE at the energy upper bound (about 1,100–1,660 averaged over a 65 ms hit).** That figure is an average over the hit, not a per-sample cost the allocator can charge while it sounds; the long-DECAY plateau, 500, is what a held hit costs.
- **Bill COWBELL at its single worst block (about 2,000: 2,100 as a bill).** It would cost the Part a third of its hits for a head of a few milliseconds (8 ms tau).
- **Add the usual 10 % to SYMP and BOWED.** ADR 0211's rule is the reading itself, rounded up; the readings are already the top of their spread.

## Consequences
- **Voices** (rev V, rev Y; `modal_bills_each_model`, then with the master tape): BOWED (8, 7) → (8, 6), taped (8, 7) → (7, 6); SYMP (8, 6) → (7, 5), taped (7, 6) → (6, 5); SYMP with BODY (7, 5) → (6, 5), taped (6, 5) → (6, 4); with BODY and the ensemble (6, 5) → (5, 4), taped (6, 4) → (5, 4). A DRUM kit's Part bill and voice count do not change. INIT DRUM's roll refuses no strike (two before, both PLAIN IMPULSE).
- **Known limit.** COWBELL's single worst block, about 2,000 over idle, is above even the new bill (1,567 + 83 = 1,650), briefly, during the BUMP sweep head. ADR 0273's load net is the backstop. A time-aware bill (the sweep head billed high, the plateau at 600) is the real fix, and stays open in #991.
- **Unread.** METAL JINGLE, PLAIN SINE, PLAIN NOISE and BODY SHAKER are still estimates (ADR 0274).
- Bills change admission, not sound: every golden is bit-identical. Bank 1: const values only.

## Sources
- private issue #991, private issue #992; PR #994.
- `chimera-engine/src/dsp/drum/types.rs` (`Machine::cost`), `chimera-engine/src/dsp/modal/mod.rs` (`COST_BOWED`, `COST_SYMPATHETIC`).
- ADR 0211, 0218, 0273, 0274.
