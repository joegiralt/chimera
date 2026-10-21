# Gate behaviour

The SuperCollider LPG rows are from an external render (GPL-3.0 tool, run
outside the repo as a listening reference; none of it is in the repo).
CLEAN rows are Chimera's shipped CLEAN LP2, kept for the record; that
comparison is not ported. Every other row is asserted by a test.

Cell: VTL5C3 (Najnudel's model refit to the VTL5C3 sheet) unless marked. One 220 Hz hit, the same Env.perc(1 ms, 10 ms) ping into every model (ours: × 15 V into CV; Plaits: its LEVEL input; SC: controlinput). CLEAN: Env.perc(1 ms, 300 ms) on cutoff and VCA. Levels re the loudest 5 ms.

| source | model | onset centroid | centroid 50 ms | centroid 200 ms | to −20 dB | to −40 dB | level at 1 s |
|---|---|---|---|---|---|---|---|
| saw 220 Hz | clean | 2278 Hz | 573 Hz | 297 Hz | 96 ms | 163 ms | -147.6 dB |
| saw 220 Hz | fw_combo | 2264 Hz | 609 Hz | 419 Hz | 312 ms | never (≤ 1.5 s) | -26.6 dB |
| saw 220 Hz | ref_combo | 2455 Hz | 631 Hz | 426 Hz | 312 ms | never (≤ 1.5 s) | -26.7 dB |
| saw 220 Hz | fw_lowpass | 1903 Hz | 301 Hz | 297 Hz | 104 ms | 388 ms | -49.5 dB |
| saw 220 Hz | ref_lowpass | 1804 Hz | 322 Hz | 435 Hz | 101 ms | 386 ms | -49.9 dB |
| saw 220 Hz | fw_gate | 3117 Hz | 1669 Hz | 1033 Hz | 23 ms | 175 ms | -52.8 dB |
| saw 220 Hz | ref_gate | 3391 Hz | 1843 Hz | 1114 Hz | 23 ms | 179 ms | -52.6 dB |
| saw 220 Hz | plaits | 2442 Hz | 1553 Hz | 1665 Hz | 493 ms | 1143 ms | -35.5 dB |
| saw 220 Hz | ref_lowpass (old cell VTL5C3/2) | 1955 Hz | 429 Hz | 355 Hz | 206 ms | 779 ms | -42.6 dB |
| saw 220 Hz | ref_combo (old cell VTL5C3/2) | 2575 Hz | 891 Hz | 507 Hz | 626 ms | never (≤ 1.5 s) | -22.2 dB |
| saw 220 Hz | SC LPG lowpassmode 1 | 759 Hz | 726 Hz | 596 Hz | 412 ms | 778 ms | -50.7 dB |
| saw 220 Hz | SC LPG lowpassmode 0 | 1710 Hz | 1685 Hz | 1457 Hz | 634 ms | 1230 ms | -31.1 dB |
| fold 220 Hz | clean | 673 Hz | 550 Hz | 324 Hz | 82 ms | 145 ms | -152.9 dB |
| fold 220 Hz | fw_combo | 667 Hz | 518 Hz | 443 Hz | 184 ms | never (≤ 1.5 s) | -32.1 dB |
| fold 220 Hz | ref_combo | 666 Hz | 517 Hz | 443 Hz | 184 ms | never (≤ 1.5 s) | -32.1 dB |
| fold 220 Hz | fw_lowpass | 674 Hz | 331 Hz | 324 Hz | 79 ms | 272 ms | -54.7 dB |
| fold 220 Hz | ref_lowpass | 673 Hz | 327 Hz | 310 Hz | 78 ms | 270 ms | -55.0 dB |
| fold 220 Hz | fw_gate | 676 Hz | 661 Hz | 615 Hz | 21 ms | 159 ms | -56.4 dB |
| fold 220 Hz | ref_gate | 675 Hz | 663 Hz | 615 Hz | 21 ms | 162 ms | -56.2 dB |
| fold 220 Hz | plaits | 677 Hz | 660 Hz | 585 Hz | 439 ms | 1091 ms | -37.3 dB |
| fold 220 Hz | ref_lowpass (old cell VTL5C3/2) | 676 Hz | 473 Hz | 316 Hz | 156 ms | 545 ms | -47.8 dB |
| fold 220 Hz | ref_combo (old cell VTL5C3/2) | 671 Hz | 576 Hz | 480 Hz | 375 ms | never (≤ 1.5 s) | -27.5 dB |
| fold 220 Hz | SC LPG lowpassmode 1 | 611 Hz | 586 Hz | 522 Hz | 359 ms | 733 ms | -54.4 dB |
| fold 220 Hz | SC LPG lowpassmode 0 | 672 Hz | 669 Hz | 665 Hz | 634 ms | 1159 ms | -33.5 dB |

## How far a ping opens it

| ping (CV) | peak I_LED | min R | at | LOWPASS −3 dB there | COMBO −3 dB there |
|---|---|---|---|---|---|
| Env.perc 1 ms/10 ms × 15 V (the checks' ping) | 14.2 mA | 6.3 kΩ | 1.1 ms | 14693 Hz | 13004 Hz |
| pulse 3 ms × 15 V | 18.9 mA | 4.3 kΩ | 3.0 ms | 21887 Hz | 19342 Hz |
| pulse 10 ms × 15 V | 18.9 mA | 4.1 kΩ | 10.0 ms | 22536 Hz | 19913 Hz |
| pulse 10 ms × 20 V (CV + OFFSET) | 18.9 mA | 2.3 kΩ | 10.0 ms | 40254 Hz | 35524 Hz |
| held 15 V | 18.9 mA | 4.1 kΩ | 44.1 ms | 22542 Hz | 19919 Hz |

## Over time (saw 220 Hz)

| t | I_LED | R | COMBO −3 dB | COMBO DC | ref COMBO level | ref LOWPASS level | ref GATE level | Plaits state (= gain) | Plaits cutoff | Plaits level | SC lpm 1 level |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 2 ms | 0.13 mA | 7.8 kΩ | 10535 Hz | -0.1 dB | +0.0 dB | +0.0 dB | +0.0 dB | 0.971 (-0.3 dB) | 13927 Hz | +0.0 dB | +0.0 dB |
| 5 ms | 0.00 mA | 16.9 kΩ | 4929 Hz | -0.1 dB | +0.2 dB | +0.3 dB | -4.2 dB | 0.962 (-0.3 dB) | 13483 Hz | +0.4 dB | +0.7 dB |
| 10 ms | 0.00 mA | 45.6 kΩ | 1871 Hz | -0.4 dB | -0.7 dB | -0.3 dB | -10.8 dB | 0.923 (-0.7 dB) | 11655 Hz | -0.3 dB | +0.2 dB |
| 20 ms | 0.00 mA | 136.5 kΩ | 674 Hz | -1.0 dB | -2.6 dB | -1.8 dB | -17.8 dB | 0.844 (-1.5 dB) | 8458 Hz | -1.4 dB | -0.7 dB |
| 50 ms | 0.00 mA | 429.9 kΩ | 264 Hz | -2.9 dB | -6.3 dB | -7.0 dB | -26.1 dB | 0.692 (-3.2 dB) | 4422 Hz | -2.6 dB | -1.7 dB |
| 100 ms | 0.00 mA | 882.8 kΩ | 166 Hz | -5.1 dB | -10.5 dB | -18.5 dB | -32.4 dB | 0.543 (-5.3 dB) | 2365 Hz | -5.3 dB | -3.9 dB |
| 200 ms | 0.00 mA | 1.67 MΩ | 121 Hz | -8.0 dB | -15.2 dB | -29.7 dB | -38.5 dB | 0.367 (-8.7 dB) | 1366 Hz | -9.6 dB | -8.4 dB |
| 400 ms | 0.00 mA | 2.90 MΩ | 99 Hz | -11.2 dB | -20.1 dB | -39.3 dB | -44.2 dB | 0.179 (-14.9 dB) | 1119 Hz | -16.2 dB | -18.1 dB |
| 700 ms | 0.00 mA | 4.19 MΩ | 88 Hz | -13.6 dB | -23.8 dB | -45.8 dB | -48.3 dB | 0.062 (-24.2 dB) | 1104 Hz | -25.5 dB | -34.0 dB |
| 1000 ms | 0.00 mA | 5.08 MΩ | 83 Hz | -15.0 dB | -26.0 dB | -49.2 dB | -50.6 dB | 0.021 (-33.4 dB) | 1104 Hz | -34.7 dB | -49.9 dB |
| 1500 ms | 0.00 mA | 6.08 MΩ | 78 Hz | -16.3 dB | -28.2 dB | -52.4 dB | -52.8 dB | 0.004 (-48.7 dB) | 1104 Hz | -50.1 dB | -75.7 dB |

