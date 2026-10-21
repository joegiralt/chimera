# 292B and VTL5C3/2 validation (the lab's `lpg-circuit-check`)

Cell VTL5C3/2 throughout (the lab's default then). The tests in
`src/buchla292b/tests.rs` and `src/vtl5c3/tests.rs` assert these numbers.

Units: 292B (reference), P&D Table 1 (P&D check).

## 1. Audio core vs Parker & D'Angelo eq. 7 (Table 1, a = 1), reference at 8× and at 48 kHz

| mode | Rf | f | eq. 7 (dB) | ref 8× (dB) | 48 kHz TPT (dB) |
|---|---|---|---|---|---|
| Combo | 1.0 kΩ | 100 | -0.00 | -0.00 | -0.00 |
| Combo | 1.0 kΩ | 1000 | -0.00 | -0.00 | -0.00 |
| Combo | 1.0 kΩ | 5000 | -0.02 | -0.02 | -0.02 |
| Combo | 1.0 kΩ | 15000 | -0.17 | -0.17 | -0.39 |
| Combo | 10.0 kΩ | 100 | -0.04 | -0.04 | -0.04 |
| Combo | 10.0 kΩ | 1000 | -0.11 | -0.11 | -0.11 |
| Combo | 10.0 kΩ | 5000 | -1.62 | -1.62 | -1.72 |
| Combo | 10.0 kΩ | 15000 | -7.02 | -7.05 | -10.20 |
| Combo | 100.0 kΩ | 100 | -0.41 | -0.41 | -0.41 |
| Combo | 100.0 kΩ | 1000 | -4.57 | -4.57 | -4.57 |
| Combo | 100.0 kΩ | 5000 | -17.00 | -17.00 | -17.34 |
| Combo | 100.0 kΩ | 15000 | -28.93 | -29.00 | -34.76 |
| Combo | 1.00 MΩ | 100 | -5.74 | -5.74 | -5.74 |
| Combo | 1.00 MΩ | 1000 | -24.07 | -24.07 | -24.09 |
| Combo | 1.00 MΩ | 5000 | -47.13 | -47.14 | -47.74 |
| Combo | 1.00 MΩ | 15000 | -65.86 | -65.95 | -73.16 |
| Gate | 1.0 kΩ | 100 | -2.92 | -2.92 | -2.92 |
| Gate | 1.0 kΩ | 1000 | -2.92 | -2.92 | -2.92 |
| Gate | 1.0 kΩ | 5000 | -2.93 | -2.93 | -2.93 |
| Gate | 1.0 kΩ | 15000 | -3.01 | -3.01 | -3.12 |
| Gate | 10.0 kΩ | 100 | -13.98 | -13.98 | -13.98 |
| Gate | 10.0 kΩ | 1000 | -13.98 | -13.98 | -13.98 |
| Gate | 10.0 kΩ | 5000 | -14.06 | -14.06 | -14.07 |
| Gate | 10.0 kΩ | 15000 | -14.68 | -14.69 | -15.47 |
| Gate | 100.0 kΩ | 100 | -32.26 | -32.26 | -32.26 |
| Gate | 100.0 kΩ | 1000 | -32.28 | -32.28 | -32.28 |
| Gate | 100.0 kΩ | 5000 | -32.88 | -32.88 | -32.92 |
| Gate | 100.0 kΩ | 15000 | -36.39 | -36.42 | -39.57 |
| Gate | 1.00 MΩ | 100 | -52.08 | -52.08 | -52.08 |
| Gate | 1.00 MΩ | 1000 | -53.77 | -53.77 | -53.77 |
| Gate | 1.00 MΩ | 5000 | -63.31 | -63.31 | -63.61 |
| Gate | 1.00 MΩ | 15000 | -73.30 | -73.35 | -77.87 |
| Lowpass | 1.0 kΩ | 100 | -0.00 | -0.00 | -0.00 |
| Lowpass | 1.0 kΩ | 1000 | -0.00 | -0.00 | -0.00 |
| Lowpass | 1.0 kΩ | 5000 | 0.02 | 0.02 | 0.02 |
| Lowpass | 1.0 kΩ | 15000 | 0.18 | 0.18 | 0.41 |
| Lowpass | 10.0 kΩ | 100 | -0.03 | -0.03 | -0.03 |
| Lowpass | 10.0 kΩ | 1000 | 0.05 | 0.05 | 0.05 |
| Lowpass | 10.0 kΩ | 5000 | 1.19 | 1.19 | 1.19 |
| Lowpass | 10.0 kΩ | 15000 | -11.97 | -12.07 | -19.73 |
| Lowpass | 100.0 kΩ | 100 | -0.26 | -0.26 | -0.26 |
| Lowpass | 100.0 kΩ | 1000 | -4.68 | -4.68 | -4.70 |
| Lowpass | 100.0 kΩ | 5000 | -33.64 | -33.65 | -34.28 |
| Lowpass | 100.0 kΩ | 15000 | -52.80 | -52.89 | -60.13 |
| Lowpass | 1.00 MΩ | 100 | -6.38 | -6.38 | -6.38 |
| Lowpass | 1.00 MΩ | 1000 | -45.75 | -45.75 | -45.78 |
| Lowpass | 1.00 MΩ | 5000 | -73.72 | -73.73 | -74.36 |
| Lowpass | 1.00 MΩ | 15000 | -92.81 | -92.90 | -100.13 |

Worst 8× error ≤ 5 kHz: 0.010 dB.

Eq. 12 DC gain and eq. 11 a_max (LOWPASS), Table 1:

| Rf | Rα/(Rα+2Rf) (dB) | ref DC (dB) | a_max |
|---|---|---|---|
| 1.0 kΩ | -0.00 | -0.00 | 1.473 |
| 100.0 kΩ | -0.34 | -0.34 | 1.493 |
| 1.00 MΩ | -2.92 | -2.92 | 1.682 |
| 10.00 MΩ | -13.98 | -13.98 | 3.566 |

COMBO's slope (Table 1), dB/oct between 2× and 8× its −3 dB:

| Rf | −3 dB | slope |
|---|---|---|
| 3.0 kΩ | 25041 Hz | -5.9 |
| 30.0 kΩ | 2531 Hz | -5.9 |
| 300.0 kΩ | 280 Hz | -5.9 |

292B values against Table 1 (−3 dB, DC gain):

| mode | Rf | Table 1 −3 dB | 292B −3 dB | Table 1 DC | 292B DC |
|---|---|---|---|---|---|
| Combo | 2.0 kΩ | 37546 Hz | 41048 Hz | -0.0 dB | -0.0 dB |
| Combo | 20.0 kΩ | 3781 Hz | 4171 Hz | -0.1 dB | -0.2 dB |
| Combo | 200.0 kΩ | 405 Hz | 483 Hz | -0.7 dB | -1.5 dB |
| Combo | 2.00 MΩ | 67 Hz | 113 Hz | -5.1 dB | -9.0 dB |
| Gate | 2.0 kΩ | 67011 Hz | 51837 Hz | -5.1 dB | -2.1 dB |
| Gate | 20.0 kΩ | 28591 Hz | 14463 Hz | -19.1 dB | -11.3 dB |
| Gate | 200.0 kΩ | 6819 Hz | 5583 Hz | -38.2 dB | -28.9 dB |
| Gate | 2.00 MΩ | 722 Hz | 718 Hz | -58.1 dB | -48.6 dB |
| Lowpass | 2.0 kΩ | 45626 Hz | 46526 Hz | -0.0 dB | -0.0 dB |
| Lowpass | 20.0 kΩ | 4570 Hz | 4667 Hz | -0.1 dB | -0.2 dB |
| Lowpass | 200.0 kΩ | 464 Hz | 478 Hz | -0.7 dB | -1.5 dB |
| Lowpass | 2.00 MΩ | 48 Hz | 44 Hz | -5.1 dB | -9.0 dB |

## 2. VTL5C3/2 static law

| If | sheet | law (A/I^1.4 + B) | Najnudel model steady | P&D eq. 39 (VTL5C3) |
|---|---|---|---|---|
| 0.5 mA | — | 142.4 kΩ | 142.4 kΩ | 146.0 kΩ |
| 1 mA | 55 kΩ | 55.0 kΩ | 55.0 kΩ | 56.0 kΩ |
| 5 mA | — | 7.3 kΩ | 7.3 kΩ | 6.9 kΩ |
| 10 mA | — | 3.8 kΩ | 3.8 kΩ | 3.3 kΩ |
| 40 mA | 2 kΩ | 2.0 kΩ | 2.0 kΩ | 1.5 kΩ |

Slope R(0.5 mA)/R(5 mA): law 19.5, model 19.5 (sheet 19). f(If) low-light log-log slope used below 0.1 mA: 4.56.

Najnudel eq. 12 with Table 1's printed P0, P1, α0, α1 (f must be ≥ 0, eq. 10):

| LED power | f(PD) printed |
|---|---|
| 1 mW | -5.176e-8 W |
| 10 mW | -7.768e-8 W |
| 30 mW | -5.118e-8 W |
| 56 mW | 7.832e-11 W |
| 70 mW | 2.916e-8 W |
| 300 mW | 4.835e-7 W |
| 1000 mW | 1.600e-6 W |
| 12000 mW | 1.154e-5 W |

LED law (eq. 13) round trip: I(V(10 mA)) = 10.0000 mA.

LED law (eq. 13): 1.482 V at 1 mA, 1.557 V at 10 mA, 1.601 V at 20 mA, 1.684 V at 40 mA (VTL5C3 sheet: 1.65 V typ, 2.0 V max at 20 mA... at its rated test).

## 3. VTL5C3/2 dynamics (Najnudel eq. 4, Table 1)

- 40 mA: turn-on to 63 % of final conductance 1.87 ms (sheet VTL5C3/2: 3.0 ms; VTL5C3 2.5 ms). Turn-on: 0.5 ms 5.8 kΩ; 1.0 ms 4.1 kΩ; 2.0 ms 3.1 kΩ; 4.0 ms 2.5 kΩ; 6.0 ms 2.2 kΩ; 10.0 ms 2.1 kΩ; 
  turn-off to 100 kΩ 44.4 ms (sheet max 50 ms; VTL5C3 35 ms). Turn-off: 5 ms 3.7 kΩ; 10 ms 6.7 kΩ; 20 ms 19.1 kΩ; 50 ms 125.4 kΩ; 100 ms 366.1 kΩ; 200 ms 818.4 kΩ; 500 ms 1.95 MΩ; 1000 ms 3.33 MΩ; 2000 ms 5.02 MΩ; 3000 ms 6.03 MΩ; 
- 10 mA: turn-on to 63 % of final conductance 1.90 ms (sheet VTL5C3/2: 3.0 ms; VTL5C3 2.5 ms). Turn-on: 0.5 ms 11.4 kΩ; 1.0 ms 8.1 kΩ; 2.0 ms 5.9 kΩ; 4.0 ms 4.7 kΩ; 6.0 ms 4.3 kΩ; 10.0 ms 4.0 kΩ; 
  turn-off to 100 kΩ 39.2 ms (sheet max 50 ms; VTL5C3 35 ms). Turn-off: 5 ms 6.9 kΩ; 10 ms 11.9 kΩ; 20 ms 30.4 kΩ; 50 ms 150.2 kΩ; 100 ms 390.9 kΩ; 200 ms 840.9 kΩ; 500 ms 1.97 MΩ; 1000 ms 3.34 MΩ; 2000 ms 5.03 MΩ; 3000 ms 6.04 MΩ; 
- From 10 kΩ steady (3.77 mA): 1 MΩ at 229 ms (Najnudel Fig. 7a, 10 kΩ → 1 MΩ: ≈ 200 ms read off the plot).

## 4. 292B LED driver, static (OFFSET 0)

| CV | top node | I_LED | slope | Rf (static) | COMBO −3 dB | COMBO DC |
|---|---|---|---|---|---|---|
| 0 V | 0.22 V | 0.000 mA | 0 µA/V | 10.00 MΩ | 66 Hz | -20.1 dB |
| 1 V | -0.02 V | 0.016 mA | 16 µA/V | 7.13 MΩ | 74 Hz | -17.5 dB |
| 2 V | -0.26 V | 0.087 mA | 71 µA/V | 1.60 MΩ | 123 Hz | -7.8 dB |
| 3 V | -0.51 V | 0.161 mA | 73 µA/V | 690.5 kΩ | 192 Hz | -4.2 dB |
| 4 V | -0.75 V | 0.238 mA | 77 µA/V | 398.9 kΩ | 279 Hz | -2.7 dB |
| 5 V | -0.99 V | 0.322 mA | 84 µA/V | 261.9 kΩ | 387 Hz | -1.9 dB |
| 6 V | -1.23 V | 0.417 mA | 95 µA/V | 182.8 kΩ | 522 Hz | -1.3 dB |
| 7 V | -1.47 V | 0.531 mA | 114 µA/V | 131.0 kΩ | 699 Hz | -1.0 dB |
| 8 V | -1.72 V | 0.676 mA | 145 µA/V | 94.0 kΩ | 946 Hz | -0.7 dB |
| 9 V | -1.96 V | 0.872 mA | 197 µA/V | 66.2 kΩ | 1311 Hz | -0.5 dB |
| 10 V | -2.20 V | 1.155 mA | 283 µA/V | 45.2 kΩ | 1886 Hz | -0.4 dB |
| 11 V | -2.44 V | 1.584 mA | 429 µA/V | 29.7 kΩ | 2834 Hz | -0.2 dB |
| 12 V | -2.68 V | 2.255 mA | 671 µA/V | 18.8 kΩ | 4441 Hz | -0.1 dB |
| 13 V | -2.92 V | 3.334 mA | 1078 µA/V | 11.6 kΩ | 7155 Hz | -0.1 dB |
| 14 V | -3.17 V | 5.092 mA | 1758 µA/V | 7.2 kΩ | 11529 Hz | -0.1 dB |
| 15 V | -3.41 V | 7.988 mA | 2897 µA/V | 4.6 kΩ | 17885 Hz | -0.0 dB |
| 16 V | -3.65 V | 12.789 mA | 4801 µA/V | 3.2 kΩ | 25694 Hz | -0.0 dB |

Threshold: the LEDs start at CV ≈ 0.77 V. Linear-region slope by hand (R4/R2/R5 + 1/R2): 67.6 µA/V (MEMS 292B bench: ~75 µA/V with a modified input resistor). Zener (1N5228B fit): 23 Ω at 20 mA, 1880 Ω at 0.25 mA (sheet 23, 1900). Rail current with the top at −3.9 V: 15.4 mA.

## 5. Q1 source follower (292B)

Small-signal gain 0.9566. THD of the whole 292B GATE path at full light, 100 Hz:

- input 2.5 V peak: f0 94 Hz, out 4.23 V peak, THD -57.8 dB (H2 -57.8, H3 -81.7)
- input 10.0 V peak: f0 94 Hz, out 14.57 V peak, THD -21.1 dB (H2 -46.8, H3 -21.3)

## 6. Reference convergence

Ping on a C4 bar, COMBO: the 8× reference against the same equations at 48 kHz (no oversampling): null -44.0 dB raw, -44.0 dB best gain. (Oversampling matters only near Nyquist; this bounds what the 48 kHz firmware loses to warping.)

