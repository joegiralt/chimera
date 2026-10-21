# 292B A/B levels (the lab's `lpg-circuit-ab`, cell VTL5C3/2)

Rendered through Chimera's engine (the takes' VA sources) and against
CLEAN LP2; the WAVs are not in the repo and the renderer is dropped. The
ping timelines and the fw/ref brightness are asserted by tests; the rest
is the record.

| file | raw RMS (dBFS) | peak (dBFS) after gain | gain (dB) |
|---|---|---|---|
| `marimba_combo_d1_ref.wav` | -16.2 | -15.9 | -11.8 |
| `marimba_combo_d1_fw.wav` | -16.2 | -15.9 | -11.8 |
| `marimba_combo_d1_clean.wav` | -22.2 | -12.7 | -5.8 |
| `marimba_combo_d2_ref.wav` | -4.2 | -3.9 | -11.8 |
| `marimba_combo_d2_fw.wav` | -4.1 | -3.9 | -11.8 |
| `marimba_combo_d2_clean.wav` | -11.1 | -1.9 | -4.9 |
| `marimba_ping_combo_d1_ref.wav` | -19.1 | -13.4 | -8.9 |
| `marimba_ping_combo_d1_fw.wav` | -19.1 | -13.4 | -8.9 |
| `bongo_combo_d1_ref.wav` | -11.5 | -16.5 | -16.5 |
| `bongo_combo_d1_fw.wav` | -11.5 | -16.6 | -16.5 |
| `bongo_combo_d1_clean.wav` | -21.8 | -11.5 | -6.2 |
| `bongo_combo_d2_ref.wav` | 0.5 | -4.5 | -16.5 |
| `bongo_combo_d2_fw.wav` | 0.5 | -4.5 | -16.5 |
| `bongo_combo_d2_clean.wav` | -12.0 | -1.1 | -4.0 |
| `bongo_ping_combo_d1_ref.wav` | -11.7 | -16.3 | -16.3 |
| `bongo_ping_combo_d1_fw.wav` | -11.7 | -16.3 | -16.3 |
| `bass_combo_d1_ref.wav` | -8.8 | -16.3 | -19.2 |
| `bass_combo_d1_fw.wav` | -8.7 | -16.3 | -19.3 |
| `bass_combo_d1_clean.wav` | -15.8 | -16.1 | -12.2 |
| `bass_combo_d2_ref.wav` | 3.3 | -4.3 | -19.2 |
| `bass_combo_d2_fw.wav` | 3.3 | -4.3 | -19.3 |
| `bass_combo_d2_clean.wav` | -6.0 | -6.1 | -9.9 |
| `bass_lowpass_d1_ref.wav` | -9.6 | -15.2 | -18.4 |
| `bass_lowpass_d1_fw.wav` | -9.6 | -15.2 | -18.4 |
| `bass_lowpass_d1_clean.wav` | -15.8 | -16.1 | -12.2 |
| `bass_gate_d1_ref.wav` | -23.4 | -10.7 | -4.6 |
| `bass_gate_d1_fw.wav` | -23.4 | -10.7 | -4.6 |
| `bass_gate_d1_clean.wav` | -15.5 | -16.1 | -12.5 |
| `pad_combo_d1_ref.wav` | -8.3 | -15.8 | -19.7 |
| `pad_combo_d1_fw.wav` | -8.3 | -15.9 | -19.7 |
| `pad_combo_d1_clean.wav` | -11.6 | -14.2 | -16.4 |
| `pad_combo_d2_ref.wav` | 3.7 | -3.8 | -19.7 |
| `pad_combo_d2_fw.wav` | 3.8 | -3.8 | -19.7 |
| `pad_combo_d2_clean.wav` | -0.8 | -3.2 | -15.1 |
| `sweep_combo_d1_ref.wav` | -12.7 | -17.6 | -15.3 |
| `sweep_combo_d1_fw.wav` | -12.7 | -17.5 | -15.3 |
| `sweep_combo_d1_clean.wav` | -13.0 | -19.6 | -15.0 |
| `sweep_combo_d2_ref.wav` | -0.7 | -5.5 | -15.3 |
| `sweep_combo_d2_fw.wav` | -0.7 | -5.5 | -15.3 |
| `sweep_combo_d2_clean.wav` | -2.3 | -10.5 | -13.6 |
| `sweep_lowpass_d1_ref.wav` | -12.6 | -16.0 | -15.4 |
| `sweep_lowpass_d1_fw.wav` | -12.6 | -15.8 | -15.4 |
| `sweep_lowpass_d1_clean.wav` | -13.0 | -19.5 | -15.0 |

| take / setting | ref − fw null (raw, dB) | ref − fw null (best gain) | LPG ref − CLEAN null (RMS-matched) |
|---|---|---|---|
| marimba combo_d1 | -39.0 | -40.7 | -5.2 |
| marimba combo_d2 | -39.0 | -40.7 | -5.2 |
| marimba ping_combo_d1 | -41.1 | -41.3 | — |
| bongo combo_d1 | -46.3 | -46.6 | -1.1 |
| bongo combo_d2 | -46.0 | -46.4 | -1.0 |
| bongo ping_combo_d1 | -47.7 | -48.0 | — |
| bass combo_d1 | -48.1 | -50.1 | -3.1 |
| bass combo_d2 | -48.7 | -50.7 | -2.8 |
| bass lowpass_d1 | -42.5 | -43.6 | -4.0 |
| bass gate_d1 | -38.4 | -39.2 | -7.1 |
| pad combo_d1 | -38.3 | -39.0 | -11.1 |
| pad combo_d2 | -38.2 | -39.0 | -10.6 |
| sweep combo_d1 | -42.0 | -42.2 | -10.6 |
| sweep combo_d2 | -42.1 | -42.2 | -9.6 |
| sweep lowpass_d1 | -37.8 | -38.0 | -11.9 |

Octave bands, dB re the reference's own band (fw and CLEAN after RMS matching):

| take / setting | ver | 31.5 | 63 | 125 | 250 | 500 | 1k | 2k | 4k | 8k | 16k |
|---|---|---|---|---|---|---|---|---|---|---|---|
| marimba combo_d1 | fw | +0.0 | -0.0 | -0.0 | +0.0 | +0.0 | -0.0 | -0.0 | -0.1 | -0.4 | -1.4 |
| marimba combo_d1 | CLEAN | +1.3 | +1.0 | +1.6 | -0.5 | -0.0 | +2.1 | +2.9 | +6.7 | +4.4 | +0.6 |
| marimba combo_d2 | fw | +0.3 | +0.1 | -0.0 | +0.0 | +0.0 | -0.0 | -0.0 | -0.1 | -0.4 | -1.4 |
| marimba combo_d2 | CLEAN | +0.9 | +0.9 | +1.6 | -0.5 | +0.0 | +1.5 | +2.4 | +22.1 | +14.1 | +7.5 |
| marimba ping_combo_d1 | fw | +0.0 | -0.1 | -0.1 | +0.0 | -0.0 | +0.0 | -0.0 | +0.4 | -0.4 | -1.1 |
| bongo combo_d1 | fw | -0.1 | -0.1 | -0.0 | +0.0 | -0.0 | +0.0 | -0.0 | -0.1 | -0.4 | -1.3 |
| bongo combo_d1 | CLEAN | +5.9 | +4.6 | +0.5 | -0.2 | +4.3 | +3.5 | +4.9 | +6.4 | +6.5 | +3.8 |
| bongo combo_d2 | fw | -0.1 | -0.1 | -0.0 | +0.0 | -0.0 | +0.0 | +0.0 | -0.0 | -0.3 | -1.3 |
| bongo combo_d2 | CLEAN | +12.5 | +7.7 | +0.2 | -0.2 | +4.2 | +3.5 | +0.7 | +0.8 | -1.7 | +1.4 |
| bongo ping_combo_d1 | fw | -0.1 | -0.1 | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | -0.0 | -0.4 | -1.2 |
| bass combo_d1 | fw | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | +0.0 | -0.0 | -0.1 | -0.6 | -3.0 |
| bass combo_d1 | CLEAN | +0.8 | +0.2 | -1.3 | +1.2 | +3.1 | +4.2 | +4.5 | +3.0 | -1.1 | -8.9 |
| bass combo_d2 | fw | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | +0.0 | -0.0 | -0.1 | -0.6 | -3.0 |
| bass combo_d2 | CLEAN | +1.7 | +1.0 | -2.5 | -1.5 | +0.8 | +2.2 | +3.1 | +2.4 | -1.9 | -10.0 |
| bass lowpass_d1 | fw | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | +0.1 | +0.3 | +0.7 | +1.0 | -0.7 |
| bass lowpass_d1 | CLEAN | +0.5 | -0.1 | -0.6 | +0.9 | +1.6 | +2.6 | +4.3 | +5.2 | +8.0 | +9.2 |
| bass gate_d1 | fw | +0.0 | -0.0 | +0.0 | +0.0 | -0.0 | -0.0 | -0.0 | -0.0 | -0.3 | -2.2 |
| bass gate_d1 | CLEAN | +0.4 | -0.2 | +0.4 | +0.2 | -0.1 | -0.8 | -0.9 | -0.2 | -0.7 | -4.0 |
| pad combo_d1 | fw | +0.1 | -0.0 | +0.0 | -0.0 | +0.0 | +0.0 | +0.0 | -0.1 | -0.7 | -3.5 |
| pad combo_d1 | CLEAN | -14.1 | -0.7 | -0.3 | +0.2 | +0.3 | +0.6 | +0.4 | -1.7 | -5.8 | -13.4 |
| pad combo_d2 | fw | +0.1 | -0.0 | +0.0 | -0.0 | +0.0 | +0.0 | +0.0 | -0.1 | -0.7 | -3.5 |
| pad combo_d2 | CLEAN | -13.3 | -0.6 | -0.1 | +0.2 | -0.2 | +0.3 | +0.3 | -2.1 | -6.5 | -14.3 |
| sweep combo_d1 | fw | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | +0.0 | -0.0 | -0.1 | -0.4 | -2.0 |
| sweep combo_d1 | CLEAN | -0.4 | +0.2 | -0.1 | -0.4 | -0.7 | -1.1 | -1.5 | -1.9 | -2.5 | -5.0 |
| sweep combo_d2 | fw | -0.0 | -0.0 | +0.0 | +0.0 | +0.0 | +0.0 | -0.0 | -0.1 | -0.4 | -2.0 |
| sweep combo_d2 | CLEAN | -0.8 | +0.3 | -0.4 | -0.8 | -1.0 | -1.2 | -1.4 | -1.6 | -2.0 | -3.9 |
| sweep lowpass_d1 | fw | -0.0 | -0.0 | -0.0 | +0.0 | +0.0 | +0.1 | +0.2 | +0.5 | +0.9 | -0.6 |
| sweep lowpass_d1 | CLEAN | -0.3 | +0.2 | -0.1 | -0.6 | -1.0 | -1.3 | -1.5 | -1.6 | -1.2 | +0.2 |

Brightness against velocity, COMBO d1: one C4 bar note alone, spectral centroid and RMS 10–53 ms after the hit (CLEAN: FREQ the bare envelope, VCA × VEL).

| vel | ref | fw | CLEAN | ref, ping | fw, ping |
|---|---|---|---|---|---|
| 1 | 562 Hz, -11.2 dB | 559 Hz, -11.2 dB | 548 Hz, -13.2 dB | 501 Hz, -12.2 dB | 500 Hz, -12.1 dB |
| 0.8 | 530 Hz, -11.5 dB | 529 Hz, -11.5 dB | 548 Hz, -15.1 dB | 474 Hz, -12.6 dB | 473 Hz, -12.6 dB |
| 0.6 | 478 Hz, -12.4 dB | 477 Hz, -12.4 dB | 548 Hz, -17.6 dB | 466 Hz, -12.8 dB | 465 Hz, -12.8 dB |
| 0.4 | 427 Hz, -13.7 dB | 427 Hz, -13.7 dB | 548 Hz, -21.1 dB | 459 Hz, -12.9 dB | 459 Hz, -12.9 dB |
| 0.2 | 371 Hz, -17.9 dB | 371 Hz, -17.8 dB | 548 Hz, -27.2 dB | 445 Hz, -13.2 dB | 447 Hz, -13.2 dB |

Ping, 15 V for 3 ms (COMBO): the reference against the firmware shape.

| t (ms) | I_LED ref | I_LED fw | R ref | R fw | −3 dB ref | −3 dB fw | DC gain ref (dB) |
|---|---|---|---|---|---|---|---|
| 0.5 | 7.99 mA | 7.99 mA | 9.8 kΩ | 8.7 kΩ | 8418 Hz | 9459 Hz | -0.1 |
| 1 | 7.99 mA | 7.99 mA | 8.1 kΩ | 7.2 kΩ | 10196 Hz | 11403 Hz | -0.1 |
| 2 | 7.99 mA | 7.99 mA | 6.5 kΩ | 6.2 kΩ | 12642 Hz | 13365 Hz | -0.1 |
| 3 | 0.00 mA | 0.00 mA | 5.9 kΩ | 6.0 kΩ | 14073 Hz | 13773 Hz | -0.0 |
| 5 | 0.00 mA | 0.00 mA | 7.3 kΩ | 7.4 kΩ | 11351 Hz | 11080 Hz | -0.1 |
| 10 | 0.00 mA | 0.00 mA | 12.5 kΩ | 12.7 kΩ | 6641 Hz | 6513 Hz | -0.1 |
| 25 | 0.00 mA | 0.00 mA | 46.2 kΩ | 46.5 kΩ | 1849 Hz | 1836 Hz | -0.4 |
| 50 | 0.00 mA | 0.00 mA | 152.5 kΩ | 152.6 kΩ | 611 Hz | 611 Hz | -1.1 |
| 100 | 0.00 mA | 0.00 mA | 393.1 kΩ | 393.2 kΩ | 282 Hz | 282 Hz | -2.7 |
| 200 | 0.00 mA | 0.00 mA | 843.0 kΩ | 843.1 kΩ | 170 Hz | 170 Hz | -4.9 |
| 400 | 0.00 mA | 0.00 mA | 1.63 MΩ | 1.63 MΩ | 122 Hz | 122 Hz | -7.9 |
| 800 | 0.00 mA | 0.00 mA | 2.85 MΩ | 2.85 MΩ | 99 Hz | 99 Hz | -11.1 |
| 1600 | 0.00 mA | 0.00 mA | 4.47 MΩ | 4.47 MΩ | 86 Hz | 86 Hz | -14.1 |
| 2400 | 0.00 mA | 0.00 mA | 5.49 MΩ | 5.49 MΩ | 81 Hz | 81 Hz | -15.5 |

Ping, 7.5 V for 3 ms (COMBO): the reference against the firmware shape.

| t (ms) | I_LED ref | I_LED fw | R ref | R fw | −3 dB ref | −3 dB fw | DC gain ref (dB) |
|---|---|---|---|---|---|---|---|
| 0.5 | 0.60 mA | 0.60 mA | 18.4 kΩ | 17.7 kΩ | 4532 Hz | 4712 Hz | -0.1 |
| 1 | 0.60 mA | 0.60 mA | 18.7 kΩ | 18.2 kΩ | 4464 Hz | 4568 Hz | -0.1 |
| 2 | 0.60 mA | 0.60 mA | 19.5 kΩ | 19.3 kΩ | 4272 Hz | 4314 Hz | -0.2 |
| 3 | 0.00 mA | 0.00 mA | 20.7 kΩ | 20.7 kΩ | 4038 Hz | 4038 Hz | -0.2 |
| 5 | 0.00 mA | 0.00 mA | 23.7 kΩ | 24.0 kΩ | 3525 Hz | 3489 Hz | -0.2 |
| 10 | 0.00 mA | 0.00 mA | 34.7 kΩ | 35.3 kΩ | 2438 Hz | 2397 Hz | -0.3 |
| 25 | 0.00 mA | 0.00 mA | 87.5 kΩ | 88.5 kΩ | 1010 Hz | 1000 Hz | -0.7 |
| 50 | 0.00 mA | 0.00 mA | 205.0 kΩ | 205.9 kΩ | 473 Hz | 472 Hz | -1.5 |
| 100 | 0.00 mA | 0.00 mA | 444.0 kΩ | 444.9 kΩ | 258 Hz | 258 Hz | -2.9 |
| 200 | 0.00 mA | 0.00 mA | 889.3 kΩ | 890.0 kΩ | 165 Hz | 165 Hz | -5.1 |
| 400 | 0.00 mA | 0.00 mA | 1.67 MΩ | 1.67 MΩ | 121 Hz | 121 Hz | -8.0 |
| 800 | 0.00 mA | 0.00 mA | 2.88 MΩ | 2.88 MΩ | 99 Hz | 99 Hz | -11.2 |
| 1600 | 0.00 mA | 0.00 mA | 4.49 MΩ | 4.49 MΩ | 86 Hz | 86 Hz | -14.1 |
| 2400 | 0.00 mA | 0.00 mA | 5.50 MΩ | 5.50 MΩ | 81 Hz | 81 Hz | -15.6 |
