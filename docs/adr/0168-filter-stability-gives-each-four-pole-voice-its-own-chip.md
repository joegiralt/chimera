# 0168. Give each 2040 and LADDER voice its own chip with STABILITY

- **Status:** Proposed
- **Deciders:** owner (2026-10-20, by ear on the lab renders, #714)

## Context

A Prophet-5 has one SSM2040 per voice, and its exponential converter has no tempco resistor: SD411 shows R479 as a plain 1 % 1 K, and tech manual § 4-17 says "the filters will drift and they will never track as well as the oscillators". So each voice's cutoff sits a little apart, wanders slowly and tracks the keyboard a little differently. Our 2040 and LADDER voices were identical. The lab (#711, `chimera-utils/src/stability.rs` on `circuit-2040`) rendered chords with a per-voice spread. The owner approved the Prophet-like setting: "the stability does the thing that I needed it to do for it to sound the way that I need it to sound." The circuit model stays parked as too expensive. VA already has an oscillator STABILITY (ADR 0118), and the two should read as one idea.

## Decision

- **A param.** Filter param 14, ident `STAB`, spec label STAB, shown on the cell as STABILITY. It is `Uni` 0..1, glides as a knob (`Smoothing::KNOB`) and is modulatable, all like VA's STAB. 1 is the stored neutral and means today's identical voices. It acts on the 2040 and LADDER only (`FilterKind::four_pole`). CLEAN and DIRTY never read it.
- **The cell.** It goes in FLT › FX's F cell, the one free cell in the filter's pages. It is the kind's "chip" cell (`KindPanel::chip`, `filter_panel(9)`): STABILITY on the 2040 and LADDER (LADDER gets its own `LADDER_PANEL`, CLEAN's words plus the cell), and empty on CLEAN and DIRTY. FLT's front and FLT › MOD stay unchanged. No glyph, as VA's STAB has none yet (#593).
- **The law.** VA's: `d = (1 − STAB)²` (`va::stability::depth`), and every amount scales by `d`. At `d = 1` (STAB 0) the amounts are four times the lab's Prophet-like setting: ±100 cents of offset, a walk clamped at ±80 cents with 0.30 of that as RMS, ±8 % of an octave per octave of tracking error about C4, and ±12 % of RES. So STAB ½ (`d = ¼`) is exactly the lab's Prophet-like setting: ±25 c, 6 c RMS (peaks about 20), ±2 %, ±3 %. The lab's "pronounced" setting sits near STAB 0.23.
- **Per voice slot.** Each (Part, voice slot) is one chip, the same on every note. The walk's stream comes from VA's `Seed::of(part, slot)` and starts mid-wander from a seeded draw. The three fixed draws (offset, tracking, RES) step through a Part's slots by golden, silver and plastic strides from a per-Part start. Hashed draws bunched up too easily to hear (the lab's finding), and strides keep any two of eight slots apart. The walk runs on across notes and only restarts when the slot plays another Part.
- **The walk.** VA's cascade (two one-poles on xorshift noise, `walk_coefs`), with its corner at 0.053 Hz (the lab's 3 s time constant). It steps once a block. Its coefficients are constants at 48 kHz / 64.
- **Where it acts.** It acts on the block's ramp ends, never per sample: `g × cut` (on the prewarped coefficient, so a cent is a cent below a few kHz and a little less near Nyquist) and `k × res`, where `k` is held at RES 1's unless a route already pushed it past. When only the drift moved (a step under 0.1 cent), both ends take this block's spread. The steady block then stays on the steady loop, never the 2040's moving path (60 cycles), and the drift steps under 0.05 cent a block even at STAB 0. A larger change, such as a new note's tracking error or a turn of STABILITY, ramps from the last block's spread, as a knob glide does (ADR 0117). At `d = 0` the spread is exactly 1 and the walk holds still, so STAB 1 is bit-identical to today and costs nothing.
- **The bill.** `STAB_COST` is 3 cycles a sample on the 2040 and LADDER when stored below 1 or routed. That is `step` (about 90 instructions on the thumbv7em ORBIT build) plus four multiplies, × 1.606 / 64 = 2.4, rounded up. A STAB route bills it too.
- **Defaults.** The stored neutral is 1, so every saved Sound and every factory Sound (authored on `plain_init`) sounds as before. A new Sound (`Sound::init`, every engine) starts at STAB ½, the Prophet-like setting, as VA's INIT starts its oscillators at 0.7. INIT VA's voice counts don't change (7 on rev V, 6 on rev Y in MIX).
- **Memory.** 36 B a voice (`FilterStability`), plus the played Sound's STAB: the voice grows 40 B (6,480 → 6,520), inside ADR 0101's chain cap.

## Alternatives considered

- **Hashed draws for the fixed spread, as VA's character:** five voices bunched up audibly in the lab. Strides spread any number of slots.
- **The drift on the cutoff before `g_of`:** every block's `g` ends would differ, pushing a static 2040 onto the moving path (60 cycles, a voice on rev V).
- **A cell on FLT's front or FLT › MOD:** both are full (ADR 0167, filter-types spec § 2). Moving a jack or a route view off them would split a pair.
- **INIT stable:** it would hide the sound the owner asked for behind a knob turn. Saved and factory Sounds stay stable either way.
- **The circuit model (#711):** too expensive for now. STABILITY is independent of it.

## Consequences

Saved Sounds and the factory library play bit for bit as before. INIT Sounds are about 10 B larger in the pool, and INIT VA's golden is re-recorded. The FLT › FX screen shows the cell on the 2040 and LADDER, and `flt_fx_fold` is re-recorded. Its knob reads 0..127 like VA's. A future glyph (#593) can serve both STABILITY cells. FB on FLT › MOD (#393) still needs a cell found there.

## Sources

#714 and its comment. #711. (private notes) and `levels.md`. Lab code `chimera-utils/src/stability.rs` (branch `circuit-2040`). ADR 0118 (VA STABILITY). ADR 0117 (knob glides). ADR 0101 (voice chain). ADR 0155, 0167 (FLT layout). Sequential Circuits SD411 schematic and Prophet-5 technical manual § 4-17.
