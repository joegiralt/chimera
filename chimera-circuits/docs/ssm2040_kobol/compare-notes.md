# Kobol Expander filter vs Chimera 2040: the A/B

2026-10-20. Desktop only, from the 2026-10-20 takes (MOTU input 5). Chimera = origin/main's 2040 (k2040.rs is unchanged since 5c216cdd), VOICE REV2 and EXP. There is no REV1 voice on main, only REV2 and EXP. Lab code is in `chimera-utils/src/kobol.rs` on local branch `kobol-ab`, worktree (private notes), not pushed. The lab's printers are gone; their measures and fits are in `chimera-circuits/src/ssm2040_kobol/takes.rs`.

## Verdict

**Close, but not the same, and the differences are all in the resonance.** With RES at 0 they are the same filter: the same 4-pole low-pass shape over the whole sweep (1.7 dB rms on the harmonics), the same bass, and both stay linear across the Kobol's whole VCO-volume range. Turn RES up and three things differ:
1. The Kobol loses about half as much bass as ours. At RES 5 it drops 3.2 dB where our matching peak costs 8 dB; at RES max it drops 7–8 dB where ours drops 13–14 dB.
2. When it self-oscillates, the Kobol is audibly dirtier: H2 −39 dBc and H3 −44 dBc. Ours has no H2 and H3 at −60 dBc.
3. At RES max the Kobol keeps ringing over the VCO (the ring sits 11 dB above H1). REV2 does much the same (+8 dB). EXP's limiter squelches the ring whenever the input is above −12 dB of full scale.

**Closest VOICE: REV2.** EXP was modelled on the Kobol, but it does not come out closer. The two tie on the sweep shapes (RES 0: 1.74/1.75 dB rms, RES 5: 2.34/2.33). At RES max REV2 is closer (4.7 against 5.3 dB rms), and only REV2 rings over the VCO the way the Kobol does. EXP is about 1.4 dB nearer on bass loss at RES max, which is small next to the 5–6 dB gap both voices have.

## What was compared, and the limits

- **VCO.** For the sweeps, the Kobol's own VCO was resynthesised from sweep1 at 12–19 s (f0 56.41 Hz, amplitudes and phases to 22 kHz) and fed to our filter at peak 1.0, LEVEL 0.5. That waveform is not a plain saw: it has notches every 12.3 harmonics (an 8 % pulse-like component, so the waveform knob was probably slightly off saw). The drive takes have no such notches, so their VCO setting differed. The drive and phrase renders use a plain band-limited saw.
- **Reference.** Responses are read per harmonic against sweep1's end plateau. That plateau is not fully open: it fits our FREQ ≈ 10 kHz, and the model is read relative to the same point. The Kobol's FREQ positions are inferred from the spectra, not read off the knob. **Assumption:** the VCA and VCO level did not change between sweep1, sweep_res5 and sweep_res10. All the bass-loss numbers rest on that. The owner has put the Kobol away, so this cannot be re-checked.
- **Drive.** The Kobol's absolute level into the 2040 is unknown. Its full VCO volume was anchored at our full scale (peak 1.0). Results that depend on this are marked below.
- **Phrases.** The Kobol's settings for these takes can't be recovered. Ours are fitted, not known: base FREQ, key tracking, envelope amount, ADS decay and sustain for the filter and the VCA, and RES, fitted to the takes' 10 ms level and centroid tracks. They match only roughly (see Phrases).

## Measured differences

### Sweeps and RES (sweeps.md, law/comp output)

| | Kobol | REV2 | EXP |
|---|---|---|---|
| RES 0 sweep: shape error, best RES | — | 1.74 dB rms, 0.025 | 1.75 dB rms, 0.025 |
| RES 5: resonant bump re H1 (FREQ 0.5–3 kHz, median) | **+5.2 dB**, width 0.70 | +1.0 dB at the fitted RES 0.175 | +0.9 dB at 0.175 |
| RES 5: H1 (bass) re RES 0 | **−3.2 dB** | −3.9 dB at 0.175 | −3.7 dB at 0.175 |
| our RES that gives the Kobol's +5.2 dB bump | — | ≈ 0.39 → H1 **−8.4 dB** | ≈ 0.41 → H1 **−8.1 dB** |
| RES max: H1 re RES 0 (FREQ 0.5–3 kHz, median) | **−8.0 dB** (plateau −6.8, flat 56 Hz–1.7 kHz) | −14.0 dB (RES 1) | −12.6 dB (RES 1; −13.3 static) |
| RES max sweep: shape error | — | 4.7 dB rms | 5.3 dB rms |
| RES max: ring re H1 with the VCO on | **+11.0 dB** | +8.0 dB | **0** (squelched) |

- **Bass loss.** Ours follows the uncompensated 4-pole law: H1 = 1/(1+k), which is −14 dB at k = 4. The Kobol keeps 5–6 dB more bass at the same bump. Its loss at RES max is flat from 56 Hz to 1.7 kHz, so this is not a high-pass in the loop: it behaves like a RES-dependent make-up gain.
  - Fitting an ideal linear 4-pole with a share c of the input fed into the feedback (`comp`) to RES 5's mid frames cuts the error from 2.96 to 1.53 dB rms, at k 1.55, c 0.40 (+4.2 dB make-up).
  - At RES max the same fit gives k 3.8, c 0.30 (+6.6 dB).
- **Our gain-free fit at RES 5 chooses a low RES (0.175).** To fit the shape it trades away the bump. No RES value matches both the Kobol's bump and its bass.
- **Bump width** at matched peak height is similar: Kobol 0.5–0.75 of its frequency, EXP at RES 0.4 0.57–0.83.
- **FREQ and resonance at the top.** At RES max the Kobol's sweep stops self-oscillating above roughly 7 kHz. So do ours.

### Drive (drive.md)

- **Kobol.** VCO1 at ¼ → ½ → full gives +7.25 dB, then +17.27 dB, at the output. The spectral shape change, with H1's step removed, is 0.00 dB at the peak and at most 0.37 dB on any harmonic within 50 dB of the strongest. Across its whole VCO range the Kobol's filter is linear: no compression and no squelch at RES 7¼.
- **Ours at RES 7¼.** That setting fits our RES 0.55 (REV2) or 0.525 (EXP) at FREQ ≈ 1.0 kHz, with 0.65 dB rms shape error.
  - At the Kobol's three steps, anchored at full = our full scale, ours is also linear: worst |Δ| 0.08 dB (REV2) and 0.43 dB (EXP).
  - Our compression curve: REV2 is flat to +12 dB over full scale, H1 drops 0.15 dB by +18 and it bends hard there. EXP adds +0.1/+0.2 dB of level change by +6/+9 dB, and its weak harmonics move 1.6–4 dB from +6/+9 dB.
  - **Result:** the same if the Kobol's full VCO is at or below about +6 dB over our full scale. If it is hotter, EXP would bend where the Kobol doesn't. The absolute level is unknown (no take pins it).

### Self-oscillation (selfosc.md, `squelch`, `lab`)

| | Kobol | REV2 (RES 1) | EXP (RES 1) |
|---|---|---|---|
| H2 | **−39.3 dBc** | none (−150) | none (−150) |
| H3 | **−43.6 dBc** | −59.7 dBc | −61.9 dBc |
| level spread, 340 Hz–5.3 kHz | 0.7 dB | 0.3 dB | 0.2 dB |
| VCO input at which the ring is squelched (RES 1, FREQ 1 kHz) | none seen at max VCO | +6 dB re full scale | **−12 dB** re full scale |

- **Kobol's ring.** Its THD is about 1.5 %, it is lopsided (even harmonics), and it holds a flat level over 4 octaves.
  - Ours is close to a pure sine: about 0.1 % THD and perfectly odd-symmetric.
  - The #711 lab component model (Prophet values with its four cell offsets) rings with H2 at −46 dBc. That is most of the way to the Kobol's −39, so cell offsets are what make H2. Its H3 is still −60 dBc, so the Kobol's ring is also limited harder than a tanh.
- **Key tracking.** The 2 oct/oct the owner set lies outside the filter: ours tracks whatever FREQ it is given.

### Phrases (phrases.md)

These are rough matches only. The centroid error is 0.3–0.4 octaves rms on phrase_bass, phrase_bass_vca8 and phrase_acid.

- **phrase_acid_vca8 is not matched (1.85 oct).** The Kobol rings loudly at 3–10 kHz there, about 12–14 dB over each note's fundamental. Its resonance reaches self-oscillation at high FREQ with RES 7¼, and ours, fitted, does not.
- **phrase_acid vs phrase_acid_vca8.** They differ by far more than level (centroid ≈ 0.1 kHz against 4.7 kHz), so something besides the VCA changed between them, or the filter was sitting at its oscillation edge.
- **Low-frequency thumps.** The Kobol takes also carry thumps at each note, about −35 dBFS below 60 Hz: VCA control feedthrough ("Cont Rej"). That is not filter, and ours has none.

## What would close the gap

1. **Bass loss (the big one).** Neither a RES-law change, STABILITY nor drive does it. A different RES→k mapping moves along the same 4-pole curve (bump vs 1/(1+k)), so it cannot keep the bass. STABILITY is per-voice spread, and drive doesn't apply because the Kobol is linear. What would: feed a share c ≈ 0.3–0.4 of the input into the loop sum on EXP, `u = x·(1 + c·k) − k·y₄`, i.e. a RES-dependent make-up of about +4 dB at RES 5 and +6–7 dB at max. It costs one multiply-add a sample. It moves the self-oscillation level and stage-1 drive, so the limiter and output trim would need a recheck. This contradicts kobol-research.md's "nothing compensates it". The measurement rests on the unchanged-level assumption above, so it needs an ADR (it supersedes part of 0105/0096).
2. **Squelch.** EXP's 3080 limiter (L = 0.25) squelches the ring above −12 dB of input, and the Kobol doesn't. Raise L, or take the limiter law from REV2's linear loop (REV2 already behaves like the Kobol here). Free in cycles. ADR 0105's variant E ("circuit limiter law") is the place to start.
3. **Ring purity.** Add the cells' offsets: the lab model's four offsets give H2 −46 dBc against the Kobol's −39, so a few mV more would match. Add a harder limit on the ring's amplitude for its H3 (−44 against our −60). Both are in the #711 component model's direction.
4. **Not needed:** a stage-bend or drive change (the Kobol is linear across its range) or a cutoff-law change (the RES 0 sweep matches).

## Files (16-bit mono 48 kHz)

In each set of three (Kobol, REV2, EXP) the files are at equal loudness, measured on the louder half, with the set's top peak at −1 dBFS. In the drive series each source keeps one gain across ¼ / ½ / full, so the level steps can be heard.

- `sweep1_*`, `sweep_res5_*`, `sweep_res10_*`: the held A1 sweeps; ours follow the Kobol's inferred FREQ track at the fitted RES (0.025 / 0.175 / 1.0)
- `drive_q_*`, `drive_h_*`, `drive_f_*`: the drive series
- `selfosc_*`: track2's self-oscillation notes, ours at RES 1 tuned to the Kobol's ring pitch per note
- `phrase_bass_vca8_*`, `phrase_bass_*`, `phrase_acid_vca8_*`, `phrase_acid_*`: the phrases (rough fits)
- Data: `sweeps.md` (per 0.25 s frame: FREQ fit, peak, width, H1, error), `drive.md`, `selfosc.md`, `phrases.md`

jam1 was not rendered: it is 2 min of hand tweaks that cannot be sequenced.
