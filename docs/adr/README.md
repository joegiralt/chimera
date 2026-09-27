# Architecture Decision Records

Why Chimera is built the way it is. One decision per file, never edited after
acceptance except to mark it superseded; a changed decision is a new ADR.
New ADRs use [`0000-template.md`](0000-template.md) and take the next number.

| # | Decision | Status |
|---|---|---|
| [0001](0001-record-decisions-as-adrs.md) | Record architectural decisions as ADRs | Accepted |
| [0002](0002-own-engine-implementations-no-elektron-code.md) | Monomachine-inspired engines are our own implementations | Accepted |
| [0003](0003-keep-faithful-tx81z-fm.md) | Keep the faithful TX81Z 4-op FM engine | Superseded by 0022 |
| [0004](0004-modal-is-the-physical-modeling-engine.md) | Modal is the physical-modeling engine; Rings/Elements are its reference | Accepted |
| [0005](0005-engine-selection-is-patch-based.md) | Engine selection is patch-based | Accepted |
| [0006](0006-sub-project-decomposition.md) | Decompose the pivot into five sub-projects | Accepted |
| [0007](0007-per-block-param-specs.md) | Each block owns its values and a const description | Accepted |
| [0008](0008-persistent-engine-instances.md) | Engines are persistent; never constructed in the audio interrupt | Accepted |
| [0009](0009-semantic-parameter-addresses.md) | Address parameters by what they are, not where they sit | Accepted |
| [0010](0010-modulation-targets-are-honest.md) | Only parameters the voice reads per block are modulatable | Accepted |
| [0011](0011-goldens-are-a-refactor-lock.md) | Golden recordings are a refactor lock, not a quality claim | Accepted |
| [0012](0012-type-driven-development.md) | Type-driven development where it pays | Accepted |
| [0013](0013-hardware-parity-budgets.md) | The simulator enforces the chip's limits | Accepted; budget clause superseded by 0020 |
| [0014](0014-audio-memory-map.md) | Voices in D2, FX bus in AXI; buffers sized to the range they serve | Accepted |
| [0015](0015-voice-steal-and-fx-returns.md) | Steal released voices first; FX sends return wet only | Accepted |
| [0016](0016-visual-direction-refined-elektron.md) | Visual direction: refined Elektron | Accepted |
| [0017](0017-prime-status-feedback.md) | MIX+PLUS reports its outcome in place, until the next input | Accepted |
| [0018](0018-fm-alg4-follows-tx81z.md) | FM algorithm 4 follows the TX81Z, not p81z | Superseded by 0022 |
| [0019](0019-note-input-per-source-queues.md) | One parser and one single-producer queue per note source | Accepted |
| [0020](0020-audio-clocking-and-output.md) | Clock the chip by silicon revision; derive the cycle budget from it | Accepted; DTCM stack clause superseded in part by 0025 |
| [0021](0021-take-once-triple-buffer.md) | Audio↔UI shared state uses a take-once triple buffer | Accepted |
| [0022](0022-one-algorithmic-engine.md) | One algorithmic six-operator engine replaces Pizza, FM and VA | Accepted |
| [0023](0023-waves-from-our-own-recipes.md) | The waves come from our own recipes | Accepted |
| [0024](0024-morph-blends-link-weights.md) | MORPH blends link weights; one plan orders both algorithms | Accepted |
| [0025](0025-dtcm-holds-wave-tables-and-stack.md) | DTCM holds the wave tables and the stack | Accepted |
| [0026](0026-algo-voices-billed-by-patch-shape.md) | Algo voices are billed by patch shape; heavy patches get fewer voices | Accepted; shedding clause and four-voice floor superseded in part by 0027 |
| [0027](0027-shedding-fades-tails-first.md) | Shedding fades, and takes tails before held notes | Proposed |
