# 0300. ALGO PAGES › FIRST picks which of ALGO's views OSC opens on

- **Status:** Proposed
- **Deciders:** owner (#985, on 0.4.0 since 2026-11-15), firmware

## Context
#985 (rephazed, Elektronauts PreenFM3 thread post 251): ALGO's OSC node opens on the pages that hold one parameter across the six operators, with each operator's own page after them (ADR 0130). The owner works that way. rephazed goes to the operator pages every time (FM8 and opsix habits) and has twelve pages to pass first. The ticket's decision: a SETTINGS preference picks which view comes first; every page stays; a setting, not a Sound field; parameter-first, today's order, is the default.

What the code had (origin/0.4 a1dd3a9c): a Part's chain is a `PartChain` picked per Sound (`Algo`, or `AlgoFormant` while an operator is a formant), each a static `ChainDef2`; a page is kept by its id when a Part's chain changes (`refocus`, `Location::rechained`, `Recall::pages_of`).

## Decision
1. **SETTINGS › PERSONALIZE › ALGO PAGES**, one cell, FIRST: PARAMS (default) or OPS, for parameter-first and operator-first (PARAMETER is 243 px in the band's 218, so the cell says PARAMS; OPS after OP1–OP6, the pages' own name). `ThemeSettings::algo_first: AlgoFirst`, THEME's param 10 (ident `ALGO_FIRST`, frozen codes 0 `PARAMETER` and 1 `OPERATOR`), stored in SYSTEM with the theme; a SYSTEM from before has none and reads PARAMS. Page `SYS_ALGO_PAGES` (id 121).
2. **OPS lays OSC out by operator**: OP1 is the node's home, OP2–OP6 follow, then WAVE and the parameter pages in their own order (`ALGO_OPS_CHAIN`); with a formant operator FORMANT follows WAVE, as it does today (`ALGO_OPS_FORMANT_CHAIN`). The node still reads OSC on the map (`ChainBlock::map`). The other four nodes are the same. Both chains are built from the PARAMETER ones' tables at compile time, so a page added to one is in both; a test holds that both hold the same pages, each once.
3. **Two more `PartChain`s**, `AlgoOps` and `AlgoOpsFormant`; `PartChain::ordered(first)` swaps ALGO's chains for their twins. `UiState` keeps the order its Parts' chains are laid out in (`algo_first`) and takes the theme's up at the top of each pass (`order_pages`): the cell, a card's SYSTEM or the editor may have changed it. The page on show is kept by its id; each Part's remembered page re-resolves by id as it does across a formant change.
4. **No page is skipped or dimmed.** The ticket left "skip or dim operator pages whose LEVEL is 0 and which modulate nothing" to design. My call: neither. A skipped page moves every later page's place under the hand, and an operator at LEVEL 0 is exactly the page one opens to turn it up. Dimming its name in the row is possible later (it needs the Sound in the map's draw, bank 1) and is left for the owner to ask for.
5. **The editor is unaffected**: `ed` describes `ALGO_FORMANT_CHAIN`, every page any Sound shows, whatever FIRST says.

## Alternatives considered
- **A Sound field**: the ticket rules it out; the order is how a player works, not part of a patch.
- **Reordering at run time** (one chain, a permutation in `NavCtx`): `PageAt` is a checked place on a static chain (ADR 0066's compile-time home, `refocus` by id); a second static chain keeps all of that and costs about 400 B of tables.
- **OP1 as home with WAVE's page dropped** (the op pages hold WAVE): then the orders would not hold the same pages.
- **The chains' tables in bank 2**: navigation runs with bank 2 closed, and reads them.

## Consequences
- Bank 1: the two chains' tables, the page, the leaf and its row, the spec and its arms, `ordered` and `order_pages`: closed-door data and the navigation's own code (measured below).
- SETTINGS › PERSONALIZE has a sixth row; its list's goldens are re-recorded.
- Guide: PERSONALIZE › ALGO PAGES and OSC in OPERATOR order need annotated screenshots; `docs/chimera-sitemap.md` describes both.

## Measurements
The batch (ADRs 0298, 0299 and this one) was built and measured once, as one branch. Release builds, `CHIMERA_STAMP_CLEAN=1`, origin/0.4 a1dd3a9c against the branch, each set built in both trees; each budget line is raised by exactly its set's measured change, never the slack.

| Set | Bank 1 (B) | Bank 2 used (B) |
|---|---:|---:|
| orbit | +2,444 | +1,056 |
| default | +2,308 | +1,056 |
| bank2-gate | +2,292 | +1,024 |
| orbit,usb-config-writer | +2,436 |  |
| fault-inject | +2,348 |  |
| bench-algo | +2,444 |  |
| bench-algo,master-tape | +2,460 |  |
| bench-routing | +2,436 |  |
| bench-memory | +2,420 |  |
| bench-stress | +2,452 |  |
| bench-orbit | +2,468 |  |
| bench-tape | +2,300 |  |
| filter-probes | +2,348 |  |

ORBIT after the batch: bank 1 902,744 B; bank 2 554,304 B used, S0-S3 7,744 B free (it was 7,776: the old groups' own churn), S4-S5 223,328 B free.

Bank 1's growth on ORBIT, by symbol (`rust-nm`, base against branch; the rest is alignment):
- **#985, about 1.2 KB:** the two chains' tables (`ALGO_OPS_*`, 404 B), `SYS_ALGO_PAGES` (96 B), THEME's eleventh spec (72 B) and its arms (about 140 B), the leaf and its row (76 B), `order_pages`, `chain_of` and `ordered` (212 B), and `chimera_board::synth` re-laid round the theme's extra byte (112 B, as ADR 0295 met).
- **#990, about 0.3 KB:** `AlgoOpParams::write` and `get` (FDBK's tenths), `fmt_val`'s arm, `quantize` by step (one rounding for stepped and choice, so `quantized`, `moved` and `op_env` stay as they were), `snap_target`, PARAM OUT's index, `AlgoLive::from_params`.
- **#956, #986, #987, #988, #989, about 0.6 KB:** the three door calls (`band_label` 100 B; the band's ratio and the chart's loops inside `draw_region_with_def`, 204 B), the chart's key (`region_data`, 76 B), `kinds::reads` out of line now bank 2 asks it (56 B), the pages' names and row words.

Everything else the tickets draw or say is bank 2's (`ui::notes`, S4): +1,024 B.

**The owner's line:** ORBIT's bank 1 had about 17 KB left; the batch takes 2.4 KB of it, half for #985. If that is too much for a page order, #985's chains could be handed out by bank 2 (`PartChain::def` through the door, PARAMS' chain while it is closed), about 0.4 KB back, at the cost of navigation asking a door for its tables.

## Sources
- #985 (private issue #985); Elektronauts PreenFM3 thread, post 251.
- ADR 0044 (recall), 0066 (homes), 0130 (operator pages), 0145 (the row), 0268 (PAD FOLLOW, the last THEME setting).
