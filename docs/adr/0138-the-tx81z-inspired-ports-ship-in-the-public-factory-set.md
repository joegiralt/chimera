# 0138. The TX81Z-inspired ports ship in the public factory set

- **Status:** Accepted (2026-10-14)
- **Deciders:** owner

## Context
ADR 0134 kept the 16 TX81Z ports (S9100010-25) private until the licence question (#410) was settled: the public build refused `from` and `edits` lines and the port ids, and `privacy.rs` refused any tracked Source carrying either. The owner auditioned and approved all 16 and ruled on 2026-10-14 that they ship: each is an inspired-by port on a different FM engine (phase modulation, formant operators, modulators the TX81Z lacks), mutated well away from its source voice, the same footing as the many "inspired by" libraries. The FS1R-inspired formant voices stand on the same ruling.

## Decision
- The 16 ports live in `tools/factory-sounds/ports.toml` as ordinary public Sources, ids S9100010-25 unchanged, with their approved values byte for byte. Their provenance stays as comments: `# from: tx81z <bank fingerprint> #<slot>` and `# edits: …`, plus "Inspired by the TX81Z voice <name>."
- The public build no longer refuses the ids S9100010-25. It still refuses a live `from` or `edits` line: those mark a port still in private staging.
- `privacy.rs` keeps the walls that matter: no raw bank or audio file (`.syx`, `.bnk`, `.mix`, `.wav`) and nothing under the private port directory. The `from` and port-id rules are gone.
- Private staging (`just import-tx`, `--private`) stays for future imports, which now take ids S9100070-99 (`privacy::PORT_IDS`).
- `factory-v1.zip` must hold all 16 ports; the release refuses a pack without them.

## Alternatives considered
- **Wait for #410.** The owner ruled no licence gate is needed for inspired-by ports.
- **Keep `from` and `edits` as live fields in the public set.** Needs the public walls, the QA split and the demo check reworked around them, for no change in the built Sounds; comments carry the same provenance.

## Consequences
- The factory set grows to 56 Sounds; the ports render bit-identically to their approved private renders.
- Raw Yamaha bank data still never enters the repo.
- Supersedes the port-privacy parts of 0134; the rest of 0134 stands.

## Sources
- Issue #532; owner ruling 2026-10-14.
- ADR 0134; `chimera-desktop/src/privacy.rs`; `tools/factory-sounds/ports.toml`.
