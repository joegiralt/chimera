# SEM-1A fw against the reference: probes (`ab.rs` `probes()`, sem-circuit-lab)

The lab's raw output, as recorded. The tests in `src/sem1a/` assert these numbers (see `README.md`).

Firmware coefficients: V0 25.04 V, states -12.60 … +13.78 V, r(b) coefficients [0.0011630848, -0.00020177249, 1.37442075e-5], A_in 0.4545, VCAL 3.6 V.

r(b) against the follower model (b V: model / fit): -12.5: 0.8688 / 0.9271; -12: 0.9111 / 0.9332; -10: 0.9612 / 0.9544; -8: 0.9779 / 0.9707; -4: 0.9928 / 0.9912; -1: 0.9986 / 0.9986; +1: 1.0013 / 1.0010; +4: 1.0044 / 1.0023; +8: 1.0073 / 1.0034; +12: 1.0093 / 1.0087; +13.5: 0.9196 / 1.0127;

### Gain at the cutoff, harmonics (1 kHz sine at fc 1 kHz, LP; CLEAN at the SEM's Q)

| RES | level | ref gain / H2 / H3 / THD % | fw | CLEAN |
|---|---|---|---|---|
| 0 | -12 dB | -9.6 / -88 / -112 / 0.00 | -9.6 / -90 / -104 / 0.00 | -6.0 / -298 / -152 / 0.00 |
| 0 | +0 dB | -9.6 / -77 / -87 / 0.02 | -9.6 / -78 / -76 / 0.02 | -6.0 / -298 / -149 / 0.00 |
| 0 | +12 dB | -9.7 / -65 / -60 / 0.12 | -9.7 / -66 / -63 / 0.09 | -12.6 / -299 / -26 / 5.24 |
| 0.5 | -12 dB | -0.9 / -78 / -93 / 0.01 | -0.9 / -79 / -77 / 0.02 | -0.9 / -298 / -147 / 0.00 |
| 0.5 | +0 dB | -1.1 / -66 / -63 / 0.08 | -1.1 / -67 / -64 / 0.08 | -0.9 / -165 / -155 / 0.00 |
| 0.5 | +12 dB | -1.7 / -54 / -53 / 0.30 | -1.7 / -55 / -54 / 0.27 | -9.6 / -299 / -41 / 1.39 |
| 0.9 | -12 dB | +10.5 / -64 / -64 / 0.09 | +10.3 / -65 / -64 / 0.08 | +8.7 / -149 / -153 / 0.00 |
| 0.9 | +0 dB | +8.7 / -53 / -54 / 0.29 | +8.7 / -54 / -55 / 0.27 | +0.4 / -298 / -30 / 3.48 |
| 0.9 | +12 dB | +5.5 / -37 / -50 / 1.46 | +5.6 / -37 / -47 / 1.55 | -10.8 / -299 / -32 / 3.20 |
| 1 | -12 dB | +20.1 / -53 / -54 / 0.29 | +20.1 / -54 / -55 / 0.26 | +7.8 / -298 / -36 / 1.92 |
| 1 | +0 dB | +15.4 / -45 / -51 / 0.63 | +15.3 / -45 / -54 / 0.63 | -3.1 / -298 / -28 / 4.32 |
| 1 | +12 dB | +5.7 / -36 / -47 / 1.78 | +5.8 / -36 / -44 / 1.80 | -14.4 / -298 / -29 / 3.96 |

### Harmonics against drive (200 Hz sine, fc 1 kHz, RES 0, LP): H2 / H3 dB re H1

| level | ref | fw | CLEAN |
|---|---|---|---|
| -12 dB | -80 / -123 | -81 / -114 | -300 / -141 |
| -6 dB | -74 / -111 | -75 / -108 | -300 / -141 |
| +0 dB | -67 / -99 | -69 / -85 | -300 / -143 |
| +6 dB | -61 / -85 | -63 / -80 | -300 / -17 |
| +12 dB | -55 / -69 | -56 / -81 | -300 / -12 |
| +18 dB | -46 / -55 | -46 / -60 | -300 / -11 |

### The same at BP (200 Hz sine, fc 200 Hz, RES 0.9): H2 / H3 dB re H1

| level | ref | fw |
|---|---|---|
| -12 dB | -63 / -54 | -65 / -54 |
| -6 dB | -58 / -46 | -59 / -49 |
| +0 dB | -53 / -45 | -54 / -45 |
| +6 dB | -47 / -48 | -47 / -47 |
| +12 dB | -34 / -42 | -32 / -40 |
| +18 dB | -31 / -40 | -31 / -39 |

### The RES ceiling: a 2 ms burst at fc, then silence; time to fall 40 dB (ms), and the k it implies

| fc | ref | fw | CLEAN at RES 1 | SEM k (small-signal) |
|---|---|---|---|---|
| 100 | 382.0 | 401.8 | holds | 0.0145 |
| 200 | 286.0 | 278.2 | holds | 0.0151 |
| 1000 | 54.8 | 54.8 | holds | 0.0193 |
| 4000 | 8.0 | 8.2 | holds | 0.0349 |
| 10000 | 3.5 | 2.8 | holds | 0.0661 |

### Resonance under level: the peak's gain (sine at fc 1 kHz, LP, RES 1) against input level

| level | ref dB | fw dB | CLEAN at the SEM's Q dB |
|---|---|---|---|
| -24 dB | +25.8 | +25.5 | +18.9 |
| -18 dB | +22.8 | +22.9 | +13.3 |
| -12 dB | +20.1 | +20.1 | +7.8 |
| -6 dB | +17.8 | +17.8 | +2.3 |
| +0 dB | +15.4 | +15.3 | -3.1 |
| +6 dB | +11.4 | +11.5 | -8.6 |
| +12 dB | +5.7 | +5.8 | -14.4 |

### MODE's notch in the firmware model (fc 1 kHz, RES 0, −12 dB): deepest point, against the reference's

| m | fw notch Hz | depth dB | ref notch Hz (validate § 6) |
|---|---|---|---|
| 0.1 | 2980 | -62.9 | 2970 |
| 0.25 | 1722 | -61.5 | 1715 |
| 0.5 | 995 | -55.0 | 990 |
| 0.75 | 575 | -56.2 | 572 |
| 0.9 | 332 | -61.7 | 330 |

### Numerical: ×8 against ×16 (saw 110 Hz, fc 1.2 kHz, RES 0.9, +12 dB), the residual

null ×8 vs ×16: -87.6 dB; LSD 0.001 dB.
Newton at ×8 over that second: 3.32 iterations a step, 0 steps unconverged.

### Follower sensitivity (2N4302 spread): H2 dB re H1 at BP, 200 Hz at fc 200 Hz, RES 0.9, 0 dB

- IDSS 1.5 mA, V_P -1 V: H2 -56 dB, H3 -45 dB
- IDSS 3.0 mA, V_P -2 V: H2 -53 dB, H3 -45 dB
- IDSS 5.0 mA, V_P -3.5 V: H2 -50 dB, H3 -45 dB
