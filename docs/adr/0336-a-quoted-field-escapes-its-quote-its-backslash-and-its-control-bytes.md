# 0336. A quoted field escapes its quote, its backslash and its control bytes

- **Status:** Accepted (2026-11-23)
- **Deciders:** coordinator (editor e2e coverage), Claude

## Context
The `ed` answers quote their texts: names (`sound <n> "<name>"`, `project`, `proj`, `map`, `mapfile`), labels (`param`, `opt`, `route`, `act`, `src`, `kind`), crumbs, toasts, refusals, notes, and values' texts. Every one was written raw, through `lit::quoted` and a dozen hand-built fields. A `"` followed by a space would close its field early in the editor's grammar, and a newline would end the line.

Today no such text reaches a field:
- Sound and project names are `Name`s (A–Z, a–z, 0–9, space, `-`), whether from a card or from the editor.
- The other texts are the firmware's own literals, numbers and those names.

But nothing at the wire held that: a `Line` takes any `&str`. The coordinator asked for escaping even so.

## Decision
- Every quoted field goes through one writer, `console::ed::lit::text` (bank 2, `.bank2_ed.lit.text`).
- Inside a quoted field:
  - `\` is written `\\`;
  - `"` is written `\"`;
  - each control byte (0x00–0x1F and 0x7F) is written `\xHH`, two uppercase hex digits (LF is `\x0A`);
  - each C1 control (U+0080–U+009F, UTF-8 `C2 80`–`C2 9F`) is written as its two bytes, `\xC2\xHH`;
  - every other byte is written verbatim, UTF-8 included.
- **Reading a field:** it opens at `"`. It closes at the first `"` that no `\` escapes. Then:
  - `\\` reads as `\`;
  - `\"` reads as `"`;
  - `\xHH` reads as that byte. The bytes read make UTF-8, so `\xC2\x85` reads as U+0085.
- No other escape exists. A `\` before any other byte doesn't occur.
- Nothing the firmware writes today changes on the wire, because no text it writes carries those bytes. The editor's tokenizer and `Protocol.parse` add the unescape.

## Alternatives considered
- **Substitute `?` at the wire** (this ADR's first draft). It is smaller and needs no change in the editor, but it loses the byte. The coordinator chose escaping.
- **Refuse at each source.** Names already are refused, but every other text would need its own check, and a missed one splits a line again.

## Consequences
- A field can carry any text without breaking its line, and it reads back exactly.
- An editor that doesn't unescape shows the escapes as written. No such text exists today.
- Flash: see the PR's measures. Bank 2 only: the field's cuts are made on its bytes, since `str`'s range index is a bank-1 function.

## Sources
- ADR 0289 (protocol 4), `chimera-engine/src/name.rs` (`is_name_byte`), `chimera-core/src/console/ed/lit.rs` (the tests `a_quoted_field_keeps_its_line_whole`, `a_c1_control_is_escaped_byte_by_byte`).
- chimera-releases `editor/e2e/unit.mjs` `tokens`, `editor/src/Protocol.elm`.
