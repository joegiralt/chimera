# CEM3320 reference vs the data sheet

The lab's `cem3320-validate` output (19c87c8e), checked by `cem3320_ob8::tests` (`cem3320_reference_*`, `cem3320_chip_*`, `cem3320_resonance_*`, `cem3320_quiescent_points`).

## Quiescent (data sheet: buffers at 0.46·V_CC = 6.9 V, inputs ~0.65 V)

| stage | input node V | cap node V | buffer out V |
|---|---|---|---|
| 1 | 0.650 | 8.302 | 7.645 |
| 2 | 0.650 | 7.132 | 6.476 |
| 3 | 0.650 | 8.299 | 7.642 |
| 4 | 0.650 | 7.132 | 6.479 |

## Pole frequency vs V_C (data sheet: f_p = A_IO·e^(−V_C/V_T)/(2π·R_EQ·C_P), 57.5–60–62.5 mV/decade)

| V_C mV | gain | f_p formula Hz | f_p measured Hz | error % |
|---|---|---|---|---|
| -25 | 2.3660 | 13807.2 | 13047.9 | -5.50 |
| +0 | 0.9000 | 5252.1 | 5169.5 | -1.57 |
| +30 | 0.2822 | 1646.7 | 1648.2 | +0.09 |
| +60 | 0.0885 | 516.3 | 519.6 | +0.64 |
| +90 | 0.0277 | 161.9 | 163.7 | +1.15 |
| +120 | 0.0087 | 50.7 | 53.2 | +4.78 |
| +155 | 0.0022 | 13.1 | 14.1 | +7.30 |

Measured scale: **60.37 mV/decade** (0 → 120 mV).

## Output swing at clipping (data sheet 10 / 12 / 14 V p-p)

50 Hz, 12 V peak in through R_I, f_p 5 kHz: **11.37 V p-p** (-5.10 … +6.28 V about the quiescent).

## Distortion (data sheet: passband 0.1 % typ / 0.3 % max, bandreject 0.3 / 1 %, mostly H2)

Output 3 dB below clipping (4.24 V peak) in the passband (f = f_p/10); the same input at the −3 dB frequency (0.435·f_p) for bandreject.

| f_p | where | out V peak | THD % | H2 dBc | H3 dBc |
|---|---|---|---|---|---|
| 500 | passband | 4.16 | 0.031 | -70.8 | -79.1 |
| 500 | bandreject | 3.01 | 0.111 | -59.1 | -87.6 |
| 2000 | passband | 4.15 | 0.017 | -77.7 | -78.9 |
| 2000 | bandreject | 2.99 | 0.112 | -59.0 | -88.0 |
| 5000 | passband | 4.12 | 0.018 | -77.5 | -78.5 |
| 5000 | bandreject | 2.95 | 0.114 | -58.9 | -88.3 |
| 12000 | passband | 4.07 | 0.019 | -77.1 | -77.7 |
| 12000 | bandreject | 2.85 | 0.122 | -58.2 | -86.2 |

With the ohmic `re` set to 0 (f_p 2 kHz): passband THD 0.018 %, bandreject 0.114 %.

## Resonance cell gm vs I_CR (data sheet: 0.8 / 1.0 / 1.2 mmho at 100 µA; fig. 6 ≈1.4 at 200, ≈1.6 at 300, max ≈2.2)

| I_CR µA | gm mmho (small signal) | measured large-signal knee at pin 8, mV (I_out = 90 % I_CR) |
|---|---|---|
| 25 | 0.396 | 86 |
| 50 | 0.671 | 97 |
| 100 | 1.028 | 117 |
| 200 | 1.401 | 158 |
| 300 | 1.594 | 199 |
| 1000 | 1.973 | 486 |

## Resonance (data sheet circuit: R_RI 51 K, R_RC 100 K, ≤100 µA)

Loop gain per mho 5976 Ω; the four-pole needs k = 4 (A_OSC 12 dB): the small-signal onset is at **I_CR ≈ 49.8 µA** (RES CV 4.98 V through 100 K). So 100 µA reaches oscillation ("from zero to oscillation"), k_max(100 µA) = 6.15.

Measured (f_p 1 kHz, a 0.2 V kick): envelope growth per 50 ms, steady ring.

| I_CR / onset | k | growth dB/50 ms | ring V peak | stage 1 peak dev V | f_ring / f_p | THD % | H2 dBc | H3 dBc |
|---|---|---|---|---|---|---|---|---|
| 0.97 | 3.916 | -5.77 | 0.000 | 0.70 | 0.5020 | 9.70 | -21.1 | -28.3 |
| 0.99 | 3.972 | -5.43 | 0.000 | 0.70 | 1.0033 | 0.00 | -135.6 | -146.7 |
| 1.00 | 4.000 | -0.66 | 0.005 | 0.73 | 1.0050 | 0.00 | -103.6 | -148.6 |
| 1.01 | 4.028 | +2.18 | 0.210 | 1.30 | 1.0053 | 0.03 | -70.3 | -82.1 |
| 1.03 | 4.083 | +0.17 | 0.384 | 1.80 | 1.0052 | 0.06 | -65.1 | -71.7 |
| 1.10 | 4.270 | +0.00 | 0.721 | 2.74 | 1.0051 | 0.13 | -59.7 | -61.3 |
| 1.30 | 4.765 | +0.00 | 1.326 | 4.32 | 1.0044 | 0.30 | -54.9 | -52.4 |
| 1.60 | 5.411 | +0.00 | 2.036 | 5.88 | 1.0039 | 0.49 | -50.6 | -48.1 |
| 2.00 | 6.132 | +0.06 | 2.804 | 5.93 | 1.0012 | 0.32 | -57.0 | -51.4 |

Resonant peak height re the passband (small signal, 20 mV in):

| k | peak f / f_p | peak re DC dB | peak re RES 0 at that f, dB |
|---|---|---|---|
| 2.00 | 0.817 | +7.8 | +7.1 |
| 3.00 | 0.939 | +15.4 | +14.3 |
| 3.60 | 0.974 | +24.1 | +22.4 |
| 3.90 | 0.991 | +33.0 | +31.0 |
| 3.96 | 1.009 | +34.1 | +32.3 |
| 3.99 | 1.009 | +35.9 | +34.1 |

## Noise (data sheet: −76 / −86 dB re max output, low pass at 20 kHz)

Shot noise of each diode, antilog transistor and mirror (×2), thermal noise of the stage resistors, white, 20 Hz – 20 kHz: **124.4 µV rms**, **-90.7 dB** re 12 V p-p (4.24 V rms). No 1/f.

## The OB-8 4-pole as wired

- One digital unit = one CEM3340 saw at the bus, 25.3 µA peak into pin 1 (5 V / 198 K): stage 1 then sits at ±2.29 V at DC (×5 the data sheet's R_I 91 K for a 1 V input).
- Starvation: stage 1's input current reaches 0 when the error current is −63 µA: a falling step of 2.5 units (a saw reset is 2 units peak-to-peak) starves stage 1 by itself.
- Loop gain per mho 2442 Ω (A8 = 3.6 K/133.6 K): k = 4 needs I_CR = 333 µA; the house RES 1 (k 4.21) needs 413 µA (gm 1.72 mmho, near fig. 6's flat top). RES CV through RX49 33 K: 10 V gives ≈ 288 µA (k 3.85).
- Quiescent buffers: 6.955 / 7.123 / 6.955 / 7.123 V.
- OB-8 pole, g_of (data sheet formula), nominal 1000 Hz: measured 1004.9 Hz (+0.49 %).
- OB-8 pole, g_set (calibrated), nominal 50 Hz: measured 50.0 Hz (+0.01 %).
- OB-8 pole, g_set (calibrated), nominal 200 Hz: measured 200.0 Hz (+0.01 %).
- OB-8 pole, g_set (calibrated), nominal 1000 Hz: measured 999.9 Hz (-0.01 %).
- OB-8 pole, g_set (calibrated), nominal 4000 Hz: measured 4004.1 Hz (+0.10 %).
- OB-8 pole, g_set (calibrated), nominal 12000 Hz: measured 12037.4 Hz (+0.31 %).
- Pole at a nominal 1 kHz through `g_of` alone: 1005.7 Hz (|H(1 kHz)| -11.94 dB, ratio 0.9943); renders use `g_set`.
