> Kept from the circuit-2040 lab ledger (private notes, 2026-10-20). Paths and commands below are the lab's, on branch `circuit-2040`; the models now live in `chimera-circuits/src/ssm2040_prophet/` (see `README.md`). The WAVs it names stay in the ledger.

# Filter STABILITY: per-voice spread and drift (#711)

The hypothesis is that the Prophet's character is partly that each voice has its own 2040 with an exponential converter that is not temperature-compensated. SD411 shows R479 as a plain 1 % 1 K, with no tempco resistor, and the tech manual § 4-17 says "the filters will drift and they will never track as well as the oscillators". So every voice's cutoff sits a little apart, wanders slowly and tracks the keyboard a little differently. Desktop only. Lab code: `chimera-utils/src/stability.rs` (`c2040-stability`, env `STAB_OUT`, `NGSPICE`), branch `circuit-2040`.

## The files

All are 32-bit float WAVs at 48 kHz, mono, **each level-matched to −20 dBFS RMS**. Before matching, the versions were within 0.5 dB of each other (`levels.md`).

| take | what |
|---|---|
| `chords_sweep_*` | 4 bars at 120 BPM (8 s plus a 1 s tail). Five sustained voices, one chord a bar, with the cutoff sweeping slowly 200 Hz → 4 kHz over the 4 bars at RES 0.5. You hear the attack and how each chord opens. |
| `chords_held_*` | The same chords at a static cutoff of 1.2 kHz, RES 0.5. This one isolates the spread. |

| suffix | filter |
|---|---|
| `2040` | today's 2040, VOICE REV2 |
| `2040_stab` | REV2 + STABILITY, Prophet-like setting |
| `2040_stab_more` | REV2 + STABILITY, pronounced setting |
| `circuit` | the ngspice Prophet-5 rev 2 circuit (as in `../ab`) |
| `circuit_stab` | the circuit + the same Prophet-like spread, voice for voice |
| `ladder` | today's LADDER |
| `ladder_stab` | LADDER + STABILITY, Prophet-like |
| `ladder_stab_more` | LADDER + STABILITY, pronounced |

Suggested order: `chords_held_2040` → `chords_held_2040_stab` → `chords_held_circuit_stab`, then the sweeps.

**The material.** The progression is our own, slow and dark: A minor 9 (A2 E3 B3 C4 G4) → F maj7♯11 (F2 C3 E3 A3 B3) → D minor 9 (D3 F3 A3 C4 E4) → E7sus4 (E2 B2 D3 A3 B3). Voice *i* always plays tone *i*, so each "chip" keeps its own offset and drift across the bars.
- **Oscillators:** the default VA patch's two saws, OSC 2 +6 cents, band-limited, identical on every version. They have no oscillator drift, so only the filter differs.
- **Amp envelope** (after the filter): 20 ms attack, held until 0.12 s before each bar ends, then released with τ 0.15 s.
- **Voices:** 5, like a Prophet.

## The amounts

Every voice gets three fixed draws and one moving one. The fixed draws use a fixed pattern across the range rather than random draws, because five seeded draws bunched up too easily to hear:

| | cutoff offset | slow drift, RMS (time scale) | key-tracking error | RES spread |
|---|---|---|---|---|
| **Prophet-like** (`_stab`) | ±25 c (voices −15, +8, +23, −25, +11 c) | 6 c (≈ 3 s; peaks ~15–20 c) | ±2 % (+1.0, −1.8, +0.4, +1.6, −0.8 %) | ±3 % of RES |
| **Pronounced** (`_stab_more`) | ±60 c (−36, +18, +54, −60, +27 c) | 15 c (≈ 2 s; peaks ~0.3–0.4 st) | ±4 % | ±8 % |

- **Drift:** white noise through two one-pole low-passes, updated once per 64-sample block and scaled to the RMS above. Each voice has its own independent stream, warmed up so it starts mid-wander.
- **Key-tracking error:** the voice's cutoff gains `error × (note − C4)` octaves. A 2 % error moves 0.24 semitones over an octave either side of C4.
- **The same draws on every filter type:** voice 3 gets the same "chip" on 2040, circuit and LADDER.

Exact per-voice values are in `levels.md`.

## What STABILITY costs

It is all control-rate. On a note-on there is one ratio for the offset and the tracking error (a multiply, `exp2` once). Each block does one RNG draw, two one-poles, an `exp2_poly` and a multiply into the block's cutoff, plus one multiply on RES. That is about 25 instructions a 64-sample block, **≈ 0.6 cycles/sample a voice** (house bill: ×1.46 × 1.1 ÷ 64). Call it **1–2 cycles a voice** with the note-on work.

**The catch:** drift changes the cutoff every block.
- **On a voice whose cutoff is already moving** (ENV/LFO → CUTOFF routed, which most VA patches are), it adds nothing more.
- **On a static-cutoff voice**, it must not push the filter onto the moving, per-sample-divide path, which costs `K2040_MOVE_COST` 60 cycles. The way round it is to hold the drifted cutoff for the whole block on the steady loop, stepping block to block. Each step is a few hundredths of a cent, far under audibility.

With that, the default VA keeps its 8 voices on rev V and 6 on rev Y. Taking the moving path instead would cost a voice on rev V.

## Notes

- **It is a per-voice param set, not DSP.** STABILITY fits as one knob on the filter page (0 = today, about ½ = Prophet-like, 1 = pronounced) scaling all four spreads together, with each voice's draw seeded by voice slot. It works the same for every TYPE (2040, LADDER, CLEAN, DIRTY).
- **It is independent of the circuit model.** It can ship on today's REV2 alone. `2040_stab` vs `circuit_stab` tells whether the pushed-circuit behaviour still adds anything on top of the spread.
- **The oscillators' own STABILITY (VA, 0.7 in INIT) was off here** to isolate the filter. In the instrument both would run.
