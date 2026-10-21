# First SEM lab: probes (`sem.rs` `probes()`, sem-lab)

The lab's raw output, as recorded. The tests in `src/sem1a/` assert these numbers (see `README.md`).

diode fit: B0 0.10 V, W 3.32 V, KD 0.1540; worst error 0.011 V on 0–13.5 V (held: B0 0.1, W 3.32)
  D(1 V) = 0.010 V (fit 0.019)
  D(2 V) = 0.082 V (fit 0.084)
  D(3 V) = 0.206 V (fit 0.195)
  D(4 V) = 0.346 V (fit 0.345)
  D(6 V) = 0.644 V (fit 0.653)
  D(10 V) = 1.264 V (fit 1.269)

Gain at the cutoff (1 kHz sine at fc 1 kHz, LP/LP2), dB, by input level:
| RES | level | CLEAN | SEM | ref | SEM THD % | CLEAN THD % |
|---|---|---|---|---|---|---|
| 0 | -12 dB | -6.0 | -6.0 | -6.0 | 0.01 | 0.00 |
| 0 | +0 dB | -6.0 | -6.1 | -6.1 | 0.05 | 0.00 |
| 0 | +12 dB | -12.6 | -6.4 | -6.4 | 0.20 | 5.24 |
| 0.5 | -12 dB | 0.5 | 0.4 | 0.4 | 0.03 | 0.00 |
| 0.5 | +0 dB | 0.4 | 0.1 | 0.1 | 0.11 | 0.08 |
| 0.5 | +12 dB | -9.3 | -0.8 | -0.8 | 0.32 | 1.39 |
| 0.85 | -12 dB | 9.6 | 12.0 | 12.0 | 0.11 | 0.29 |
| 0.85 | +0 dB | -0.4 | 9.7 | 9.7 | 0.23 | 3.73 |
| 0.85 | +12 dB | -11.6 | 3.0 | 3.2 | 0.54 | 3.41 |
| 0.95 | -12 dB | 7.5 | 18.0 | 18.0 | 0.20 | 2.06 |
| 0.95 | +0 dB | -3.4 | 13.5 | 13.2 | 0.42 | 4.37 |
| 0.95 | +12 dB | -14.8 | 3.0 | 3.2 | 0.57 | 4.02 |
| 1 | -12 dB | 7.8 | 18.0 | 18.0 | 0.20 | 2.70 |
| 1 | +0 dB | -3.3 | 13.5 | 13.2 | 0.42 | 4.65 |
| 1 | +12 dB | -14.7 | 3.0 | 3.2 | 0.57 | 4.32 |

MODE's notch (fc 1 kHz, RES 0, 0 dB): the deepest point of a 100 Hz–10 kHz sweep, against fc·√((1−m)/m), and the 100 Hz / 10 kHz gains:
  m 0.1: notch 2934 Hz (-54.8 dB), predicted 3000 Hz; 100 Hz -1.0 dB, 10 kHz -20.7 dB
  m 0.25: notch 1745 Hz (-47.7 dB), predicted 1732 Hz; 100 Hz -2.6 dB, 10 kHz -12.3 dB
  m 0.5: notch 1008 Hz (-47.5 dB), predicted 1000 Hz; 100 Hz -6.2 dB, 10 kHz -6.2 dB
  m 0.75: notch 582 Hz (-50.7 dB), predicted 577 Hz; 100 Hz -12.4 dB, 10 kHz -2.6 dB
  m 0.9: notch 336 Hz (-56.9 dB), predicted 333 Hz; 100 Hz -20.9 dB, 10 kHz -1.0 dB

Ring-out at RES 1 (a 2 ms burst at fc, then silence): time to fall 40 dB, ms:
  fc 200 Hz: CLEAN never (holds), SEM 175, SEM with the oscillating floor never (holds)
  fc 1000 Hz: CLEAN never (holds), SEM 35, SEM with the oscillating floor never (holds)
  fc 4000 Hz: CLEAN never (holds), SEM 10, SEM with the oscillating floor never (holds)
