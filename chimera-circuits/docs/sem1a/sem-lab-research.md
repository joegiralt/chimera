# SEM filter: research notes (epic #720)

> Copied from the first SEM lab's ledger (private notes, branch `sem-lab`). Its code is now `chimera-circuits/src/sem1a/ota_svf.rs` (models) and `stimuli.rs` (stimuli). The `dl/` files it names stay in the ledger and are cited, not copied. The WAV sets are not kept.

Date 2026-10-20. Desktop research only. Provenance: Oberheim's own schematic, Zavalishin's book, clone makers' published descriptions. No code read from anyone (GPL or otherwise).

## Sources

| # | Source | Status |
|---|---|---|
| S1 | Oberheim Electronics, *SEM-1A schematic* (VCO, VCF, VCA, envelope, PSU sheets; PSU sheet dated Nov 11 1975, rev 5-30-78), 9 pp. Original URL http://crowncity.net/ratcave/Electro/Oberheim/SEM-1A.pdf (now gone); fetched from the Wayback Machine (`web.archive.org/web/2010id_/…`, three snapshots, md5 858ccbf2…823d, all the same). Local copy `dl/SEM-1A.pdf`, page renders `dl/pg-*.png` (VCF = `pg-6.png`, mode pot and VCA = `pg-7.png`). | read (scanned drawing, read by eye) |
| S2 | V. Zavalishin, *The Art of VA Filter Design*, rev 2.1.0 (Native Instruments, 2018), 520 pp. Local copy `dl/zav.pdf` (Wayback copy of the NI URL). | read: ch. 4.1-4.4, 4.7 (SVF, TPT SVF, mixing modes, notch); ch. 6.2, 6.6, 6.9 (OTA 1-pole), 6.11 (antisaturators in the SVF) |
| S3 | A. Lanterman, ECE 4450 "EMS 2010 Homework #4", Georgia Tech, https://lanterman.ece.gatech.edu/?p=149 | read (an exercise on S1's VCF) |
| S4 | Synthesis Technology E430 page, https://synthtech.com/eurorack/E430/ (states it is "a faithful re-creation", "the original SEM circuitry was not improved") | read |
| S5 | Cherry Audio SEM manual, https://docs.cherryaudio.com/cherry-audio/sem | read (front-panel behaviour only) |
| S6 | Product pages: Studio Electronics SEM Eurorack, G-Storm Electro SEMSVF (synthanatomy.com, 2023), Arturia Filter SEM | skimmed: marketing descriptions only |
| S7 | (private notes) section 4 and `nonlinear-filter-papers.md` (our earlier notes on Zavalishin ch. 5-6) | read. The reading list has no SVF-specific or OTA-SVF paper. Werner & McClellan (DAFx-20, ladders built from SVFs) is the only SVF entry, and it is about ladders, so I skipped it. |

## 1. The circuit (S1, VCF sheet, read by eye)

Part numbers are the schematic's.

- **Summer / HP node**: A12, a 301A inverting amp. Its feedback is R139 100 K. The three audio inputs come through the panel level pots PS13-PS15 (50 K), C15-C17 0.15 µF and R155-R137 220 K each, so each input has gain −100/220 = **−0.455**. With the 0.15 µF into 220 K, the input is high-passed at 4.8 Hz. LP comes back through R138 100 K (with C18 10 pF across it, rolling off at 159 kHz), so the LP feedback is −1. A12's output is the **HP output** (J1, through R140 1 K).
- **Integrator 1**: A13, a CA3080 OTA. HP reaches its + input through R142 100 K over R144 220 Ω, so the input is divided by 455.5. The − input has the same 220 Ω (R145) plus OFFSET trim T9. The output charges C20, 300 pF 2 %, and is buffered by JFET source follower Q39 (2N4302, 15 K to −15). Q39's output is **BP** (A14, a 741 buffer, gives BP OUT J3).
- **Integrator 2**: A15, the same circuit (R148 100 K / R150 220 Ω, C21 300 pF, Q40). Its output is **LP**, through A16 (a 741) to LP OUT K3.
- **Cutoff**: one exponential converter (Q41/Q42) feeds both OTAs' I_ABC through R146 and R152, 15 K each. Inputs: INIT FREQ T11, the FREQ pot PS17, two CV inputs at 100 K, and MOD (PS18, from ENV2/LFO/EXT). Volts/octave trim T12.
- **Damping (RES)**: BP (Q39's source) goes to the top of RES pot PS16, 50 K, whose bottom is grounded. The wiper goes through R157 33 K and then C22 0.33 µF into A12's summing node. In parallel, BP also feeds a divider, R159 47 K over R158 10 K to ground (0.175 × BP, 8.25 K source). From the divider tap, **anti-parallel diodes D11/D12** (1N4148) and R156 100 K lead to the R157/C22 junction. This is the "nonlinear shaping in the feedback path" that S3 tells students to ignore.
- **MODE** (pot-board sheet, S1 p.7): PS19 "NOTCH", a 50 K pot, runs between HI-PASS OUT and LOW-PASS OUT (each through 1 K). Its wiper goes to A17, a 741 follower, which is NOTCH OUT. Switch S504 picks either the BP output or this wiper for the module's VCA (A20, a CA3080), coupled through C23 0.15 µF. So MODE is a **passive linear crossfade** from LP to HP, and BP is a separate switch that bypasses the crossfade.
- Notes sheet: resistors 5 %, diodes 1N4148, FETs 2N4302, NPN 2N5172, PNP 2N3905.

### 1.1 What the circuit does, derived (my analysis, not a source's)

- **Topology**: Zavalishin's SVF (S2 Fig. 4.1, p.95). hp = u − lp − k·bp, bp = ∫ωc·hp, lp = ∫ωc·bp, with u = 0.455·x (inverted; the sign does not matter). Both integrators are non-inverting OTA integrators. HP + LP is the notch, as S2 eq. on p.119 says (H_N = (s²+1)/(s²+2Rs+1)).
- **The OTA's saturation sits at the integrator's input**: I_out = I_ABC·tanh(v_in/2V_T), with v_in = v/455.5. So each integrator integrates V0·tanh(v/V0), where V0 = 455.5 × 2V_T = 455.5 × 51.7 mV = **23.6 V**. This is Zavalishin's OTA 1-pole structure (Fig. 6.35, pp.201-202: "a saturator at the integrator's input, … soft clipping" of the integration rate, i.e. slew limiting). On ±15 V rails the integrator inputs (the HP and BP nodes) cannot pass about ±13.5 V, so the tanh argument stays within ±0.57. **The OTA bend is at most 9 %.** The SEM's integrators are nearly linear by design.
- **Mode morph law**: the wiper fraction m runs 0 at LP to 1 at HP. A17 is a follower, and the 1 K source resistors are small against 50 K, so the output is (1−m)·LP + m·HP, which gives H = ((1−m) + m·s²)/(s² + k·s + 1). The zeros sit at s = ±j·√((1−m)/m), so **the notch moves**: f_notch = fc·√((1−m)/m). That is fc at the centre, 1.73·fc at m = ¼, 0.58·fc at m = ¾, and it runs out to 0 or ∞ at the ends. The passband is −6 dB at the centre. The pot's taper is not marked on the schematic. I assume linear.
- **Damping law**: a pot at fraction a (1 = wiper at BP, minimum RES) drives R157 33 K into the virtual ground. Taking the pot's Thevenin resistance into account, k(a) = (100 K/33 K)·a / (1 + (50 K/33 K)·a(1−a)) = 3.03a/(1 + 1.515a(1−a)). That gives k = 3.03 at RES 0 (Q 0.33), 1.10 at the middle, 0.27 at a = 0.1 and **0 with the wiper at ground**. C22 in series high-passes the damping at 1/(2π·33 K·0.33 µF) = 14.6 Hz, so below about 30 Hz cutoff the damping fades and Q rises. I leave this out of the model.
- **The diodes are an antisaturator in the damping path**: Zavalishin's Fig. 6.51 exactly (S2 p.212-213: "connect a saturator [sic: the antisaturator, sinh] in parallel with the 2R gain element … the damping feedback signal will grow exponentially at large levels of y_BP, no matter what the value of R is"). An anti-parallel 1N4148 pair conducts i = 2·I_s·sinh(v/(n·V_T)). In series with 108 K and fed from 0.175·BP, the extra damping it adds into A12's 100 K is D(bp) ≈ 0 below |bp| ≈ 2 V, then rises with slope 0.162 (with about a 3.5 V offset). **Loud resonance is damped harder.** The SEM's resonance compresses with level: the peak widens and drops, and it never clips.
- **Self-oscillation**: small-signal, the damping is exactly 0 only with the wiper on the ground end. Then the loop is lossless: it rings on at whatever level the diodes let it, but it does not *grow* from silence. In practice the pot's end resistance and the OTA's and JFET's losses leave k slightly above 0. The diodes can only add damping, never subtract it, so the SEM cannot hold a self-oscillation. That matches S4 ("No self-resonance unless patched externally"; its demo says the BP "feedback" can be played like a self-resonator only when patched) and the folklore. Allowing true self-oscillation would need k < 0, anti-damping held by the diodes (S2 p.212: "the main reason to introduce the antisaturator into the SVF is so that we could go into the selfoscillation range R < 0").
- **Levels**: digital 1.0 is taken as 5 V at one SEM audio input at full (a VCO saw at the mixer's top). This is my assumption: the VCO sheet doesn't print its saw amplitude. So 0 dB puts u = 2.27 V into the loop, and +12 dB (9.1 V) is about four VCOs' worth summed. The diodes start working near |bp| = 2 V, so at 0 dB they act only once the resonance peak has built to about Q·2.27 V ≥ 2 V, i.e. Q ≳ 1. The rails (±13.5 V) are reached only at +12 dB with high Q.

## 2. Zavalishin (S2): what we use

- TPT SVF (§4.4, pp.109-111): hp = (x − (2R+g)s1 − s2)/(1 + 2Rg + g²), then bp = g·hp + s1, s1' = bp + g·hp, lp = g·bp + s2, s2' = lp + g·bp; g = tan(πfc/fs). That is 4 multiplies and 6 adds, and it is never instantaneously unstable for R > −1 (p.110). Our k is his 2R.
- Mixed modes (§4.7, pp.117-119): notch = LP + HP, i.e. 1 − 2R·BP. Any (1−m)LP + m·HP stays inside the same pole pair.
- OTA nonlinearity (§6.9, Fig. 6.35-6.36, pp.201-203): tanh at the integrator input. It limits the integration slope; it does not bound the state.
- Antisaturator in the SVF damping (§6.11, Figs. 6.50-6.52, pp.211-213): putting it in series with R fails (no damping at R = 0). In parallel with the 2R gain, it damps large levels whatever R is. The SEM's diodes are this. The ZDF solve with an antisaturator in the SVF is "more complicated" (p.215): the unknown isn't on a path every loop shares.
- Cheap solve (§6.6, pp.191-192): linearise at 0, solve the linear ZDF, then push the solved signal through the nonlinearity. Our cheap SEM does this for both OTA tanh terms. The diode term is evaluated on the TPT state s1, the "zero-input" prediction of bp, so the linear solve needs no extra divide.

## 3. How this differs from CLEAN (from `chimera-engine/src/dsp/filter/clean.rs`)

CLEAN is the same TPT SVF with `sat_c1` on both *states* (linear to ±1, flat at ±1.5) and `k = 2(1 − RES/0.95)`, so it self-oscillates from RES 0.95 on. Its modes are fixed taps: LP1/LP2/LP4/BP2/BP4/HP4/NOTCH1 (LP + ¼HP, zero at 2·FREQ)/NOTCH2. Where SEM differs:

1. **Where the nonlinearity sits.** CLEAN clips its states at digital ±1-1.5: loud resonance flattens into harmonics. SEM bends the integrators' *rates* (OTA tanh, at most 9 %), and its resonance is limited by *damping* (the diodes): a loud peak gets wider and quieter but stays sinusoidal.
2. **Resonance ceiling.** CLEAN oscillates. SEM never does (k ≥ k_min > 0), and its loud peaks are compressed.
3. **MODE is continuous.** LP → moving notch → HP, plus a BP switch, against CLEAN's discrete taps. No CLEAN mode is a moving notch.
4. Small-signal at low RES, SEM LP and CLEAN LP2 are the same transfer function. The difference is at level, at high RES and in the morph. The A/B is built to show exactly that.

## 4. Open points

- The pot tapers (RES, MODE) are not on the schematic. I assumed linear for MODE. For RES the prototype uses CLEAN's RES law (the same knob feel on every TYPE) clamped at the SEM's k_min; the circuit's own law (Q 0.33 → ∞) is in §1.1.
- The OTA offsets (T9/T10) and the C22/C15 couplings are not modelled.
- No hardware reference exists. The "reference" in the lab is our own accurate ODE model of §1.1 (oversampled, Newton, exact tanh and Shockley diodes), not the circuit.
