# 0300. ALGO PAGES › FIRST picks which of ALGO's views OSC opens on

- **Status:** Accepted (2026-11-20)
- **Deciders:** owner (#985, on 0.4.0 since 2026-11-15), firmware

## Context
#985 (rephazed, Elektronauts PreenFM3 thread post 251): ALGO's OSC node opens on the pages that hold one parameter across the six operators, with each operator's own page after them (ADR 0130). The owner works that way. rephazed goes to the operator pages every time (FM8 and opsix habits) and has twelve pages to pass first. The ticket's decision: a SETTINGS preference picks which view comes first; every page stays; a setting, not a Sound field; parameter-first, today's order, is the default.

What the code had (origin/0.4 a1dd3a9c): a Part's chain is a `PartChain` picked per Sound (`Algo`, or `AlgoFormant` while an operator is a formant), each a static `ChainDef2`; a page is kept by its id when a Part's chain changes (`refocus`, `Location::rechained`, `Recall::pages_of`).

## Decision
1. **SETTINGS › PERSONALIZE › ALGO PAGES**, one cell, FIRST: PARAMS (default) or OPS, for parameter-first and operator-first (PARAMETER is 243 px in the band's 218, so the cell says PARAMS; OPS after OP1–OP6, the pages' own name). `ThemeSettings::algo_first: AlgoFirst`, THEME's param 10 (ident `ALGO_FIRST`, frozen codes 0 `PARAMETER` and 1 `OPERATOR`), stored in SYSTEM with the theme; a SYSTEM from before has none and reads PARAMS. Page `SYS_ALGO_PAGES` (id 121).
2. **OPS lays OSC out by operator**: OP1 is the node's home, OP2–OP6 follow, then WAVE and the parameter pages in their own order (`ALGO_OPS_CHAIN`); with a formant operator FORMANT follows WAVE, as it does today (`ALGO_OPS_FORMANT_CHAIN`). The node still reads OSC on the map (`ChainBlock::map`). The other four nodes are the same. Both chains are built from the PARAMETER ones' tables at compile time, so a page added to one is in both; a test holds that both hold the same pages, each once.
3. **Two more `PartChain`s**, `AlgoOps` and `AlgoOpsFormant`; `PartChain::ordered(first)` swaps ALGO's chains for their twins. `UiState` keeps the order its Parts' chains are laid out in (`algo_first`) and takes the theme's up at the top of each pass (`order_pages`): the cell, a card's SYSTEM or the editor may have changed it. The page on show is kept by its id; each Part's remembered page re-resolves by id as it does across a formant change.
4. **No page is skipped or dimmed.** The ticket left "skip or dim operator pages whose LEVEL is 0 and which modulate nothing" to design. My call: neither. A skipped page moves every later page's place under the hand, and an operator at LEVEL 0 is exactly the page one opens to turn it up. Dimming its name in the row is possible later (it needs the Sound in the map's draw, bank 1) and is left for the owner to ask for.
6. **The operator-first tables are bank 2's** (the review's call, for bank 1): `notes::ALGO_OPS_CHAIN` and its formant twin, handed out through `UiDoor::notes` (`Ask::OpsChain`). `PartChain::ordered` makes the twins only while the door answers, and `PartChain::def` reads PARAMS' chain for a twin if it ever closes, so with bank 2 closed FIRST is PARAMS whatever it says and every place is on the chain it was made for (`algo_pages_door_test`).
5. **The editor is unaffected**: `ed` describes `ALGO_FORMANT_CHAIN`, every page any Sound shows, whatever FIRST says.

## Alternatives considered
- **A Sound field**: the ticket rules it out; the order is how a player works, not part of a patch.
- **Reordering at run time** (one chain, a permutation in `NavCtx`): `PageAt` is a checked place on a static chain (ADR 0066's compile-time home, `refocus` by id); a second static chain keeps all of that and costs about 400 B of tables.
- **OP1 as home with WAVE's page dropped** (the op pages hold WAVE): then the orders would not hold the same pages.

## Consequences
- Bank 1: the two chains' tables, the page, the leaf and its row, the spec and its arms, `ordered` and `order_pages`: closed-door data and the navigation's own code (measured below).
- SETTINGS › PERSONALIZE has a sixth row; its list's goldens are re-recorded.
- Guide: PERSONALIZE › ALGO PAGES and OSC in OPERATOR order need annotated screenshots; `docs/chimera-sitemap.md` describes both.

## Measurements
The batch (ADRs 0298, 0299 and this one) was measured as one branch, merged with origin/0.4 fda23e52 (ADR 0301). Release builds, `CHIMERA_STAMP_CLEAN=1`, every budgeted set. Each line is set to the size measured; its change is counted from 0.4's line, which the file's comment names (`from`), so line = from + change on every line.

| Set | Bank 1 line (B) | Change | Bank 2 used (B) | Change |
|---|---:|---:|---:|---:|
| orbit | 902,840 | +2,220 | 557,056 | +1,888 |
| default | 888,812 | +2,052 | 528,928 | +1,824 |
| bank2-gate | 893,308 | +2,044 | 528,928 | +1,856 |
| orbit,usb-config-writer | 897,688 | +2,204 | | |
| fault-inject | 844,936 | +2,076 | | |
| bench-algo | 886,448 | +2,204 | | |
| bench-algo,master-tape | 864,636 | +2,228 | | |
| bench-routing | 885,816 | +2,204 | | |
| bench-memory | 888,400 | +2,188 | | |
| bench-stress | 891,384 | +2,196 | | |
| bench-orbit | 862,232 | +2,220 | | |
| bench-tape | 852,000 | +2,052 | | |
| filter-probes | 845,856 | +2,100 | | |

ORBIT after the batch: bank 1 902,840 B; bank 2 557,056 B used, S0-S3 7,296 B free, S4-S5 221,024 B free. Against 0.4's own measured ORBIT bank 1 (900,604 B, ADR 0301) the batch adds 2,236 B.

Where bank 1's growth is, by symbol on ORBIT (`rust-nm`, taken before the review's round; the rest is alignment):
- **#985, about 0.8 KB:** `SYS_ALGO_PAGES` (96 B), THEME's eleventh spec (72 B) and its arms (about 140 B), the leaf and its row (76 B), `order_pages`, `chain_of`, `ordered` and the door call for the chains, and `chimera_board::synth` re-laid round the theme's extra byte (112 B, as ADR 0295 met). The two chains' tables (404 B) are bank 2's since the review: that round, with the loops' link mask and FDBK's hint in it, took the batch from +2,444 B to +2,236 B on ORBIT.
- **#990, about 0.3 KB:** `AlgoOpParams::write` and `get`, `fmt_val`'s arm, `quantize` by step, `snap_target`, PARAM OUT's index, `AlgoLive::from_params`.
- **#956, #986, #987, #988, #989, about 0.6 KB:** the three door calls (the band's label and ratio, the chart's loops and the links handed to them), `kinds::reads` out of line now bank 2 asks it, the pages' names and row words.

Everything else the tickets draw or say is bank 2's (`ui::notes`, `ui::feel`, S4).

## Sources
- #985 (private issue #985); Elektronauts PreenFM3 thread, post 251.
- ADR 0044 (recall), 0066 (homes), 0130 (operator pages), 0145 (the row), 0268 (PAD FOLLOW, the last THEME setting).
