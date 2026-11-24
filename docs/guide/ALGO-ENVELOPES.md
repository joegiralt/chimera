# ALGO operator envelopes

Each ALGO operator has its own envelope, in the classic Yamaha four-rate shape: **AR** (attack rate) rises to full, **D1R** (first decay rate) falls to **D1L** (first decay level), **D2R** (second decay rate) falls on from there while the key is held, and **RR** (release rate) falls to silence after key up. **RS** (rate scaling) makes every rate faster on higher keys.

The cells are rates, not times: a higher number is faster. AR, D1R and D2R run 0 to 31, RR 1 to 15, D1L 0 to 15 and RS 0 to 3, as in the [NRPN chart](NRPN.md).

## From a rate to seconds

Two steps of AR, D1R or D2R make a stage twice as fast; one step of RR does. The tables give the times with no rate scaling. RS makes them shorter on higher keys (below).

- **Attack** rises in a straight line from silence to full.
- **Decays and the release** fall at a steady number of dB a second. The table gives the time to fall from full to −80 dB, where a note ends. A stage that starts lower, or stops at D1L, takes its share of that time: each D1L step below 15 is 3 dB down, so D1R falls from full to D1L 11 (−12 dB) in 12/80 of its time.

| Rate | AR: full attack | D1R or D2R: full to −80 dB |
|---|---|---|
| 1 | 8.2 s | 55 s |
| 2 | 5.8 s | 39 s |
| 4 | 2.9 s | 19 s |
| 6 | 1.45 s | 9.7 s |
| 8 | 725 ms | 4.8 s |
| 10 | 363 ms | 2.4 s |
| 12 | 181 ms | 1.2 s |
| 14 | 91 ms | 604 ms |
| 16 | 45 ms | 302 ms |
| 18 | 23 ms | 151 ms |
| 20 | 11 ms | 76 ms |
| 22 | 5.7 ms | 38 ms |
| 24 | 2.8 ms | 19 ms |
| 26 | 1.4 ms | 9.4 ms |
| 28 | 0.7 ms | 4.7 ms |
| 30 | 0.4 ms | 2.4 ms |
| 31 | 0.25 ms | 1.7 ms |

| RR | Full to −80 dB after key up |
|---|---|
| 1 | 27 s |
| 2 | 14 s |
| 3 | 6.8 s |
| 4 | 3.4 s |
| 5 | 1.7 s |
| 6 | 854 ms |
| 7 | 427 ms |
| 8 | 214 ms |
| 9 | 107 ms |
| 10 | 53 ms |
| 11 | 27 ms |
| 12 | 13 ms |
| 13 | 6.7 ms |
| 14 | 3.3 ms |
| 15 | 1.7 ms |

At 0, a rate stops its stage:
- **AR 0** never rises, so the operator stays silent.
- **D1R 0** holds at full while the key is down.
- **D2R 0** holds at D1L: it is the sustain.
- **D1L 0** makes D1R fall all the way to silence.

An operator's LEVEL sets how loud it is, not how long its stages take.

## RS: rate scaling

RS makes every rate of the operator, the release too, faster on higher keys, as the keyboard rate scaling of Yamaha's FM synths does: a high piano or bell note dies sooner than a low one. Each note adds steps to every rate, counted from the bottom of the keyboard (A0), and every 4 added steps make a stage twice as fast. **RS 0 is not off**: it still adds up to 3 steps across the keyboard, a gentle tilt, as on the TX81Z.

| RS | C2 | C4 | C6 | C8 | Per octave |
|---|---|---|---|---|---|
| 0 | 1.00× | 1.19× | 1.41× | 1.68× | about 1.1× |
| 1 | 1.19× | 1.68× | 2.4× | 3.4× | about 1.2× |
| 2 | 1.41× | 2.8× | 5.7× | 11× | about 1.4× |
| 3 | 2.4× | 9.5× | 38× | 152× | 2× |

Each figure is how much faster every stage runs on that key than in the tables above. At RS 3, a D1R of 12 (1.2 s from full to −80 dB) takes about 127 ms on C4 and 32 ms on C6. Every rate tops out at the scale's fastest, so AR 31 and RR 15 barely speed up.

## Longer than the operator envelopes

For a swell or a sweep over minutes, use a MOD envelope. A type A envelope (E1, E2 or E3) has three SPEED ranges, set on MOD › SPD:

| SPEED | HOLD | ATTACK | DECAY and RELEASE |
|---|---|---|---|
| FAST | up to 2.5 s | 0.2 ms to 1.5 s | 0.6 ms to 2.5 s |
| MED | up to 10 s | 2 ms to 10 s | 3.5 ms to 10 s |
| SLOW | up to 60 s | 9.3 ms to 60 s | 30 ms to 60 s |

To open a sound over a minute, take a modulator's LEVEL (one that feeds another operator, not a carrier) down to where the sound is dull, prime it with **MIX + PLUS** on its LEVEL cell, and route a SLOW envelope into it with a positive amount: set the envelope's ATTACK long and SUSTAIN full, and the operator's brightness rises over the whole attack. A negative amount from a full LEVEL closes the sound instead. Routes into a carrier's LEVEL change its loudness the same way.
