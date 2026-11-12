# NRPN chart

Every cell below has a fixed NRPN address that never changes from one firmware to the next, so a sequencer's instrument definition (a Cirklon CK instrument, a Hapax definition, a DAW's NRPN lane) made once keeps working. No binding is needed: send the NRPN on the Part's MIDI channel and the cell moves, gliding as a knob does. A binding on the same NRPN number in the Sound's MIDI map wins over the address (see [MIDI control](MIDI.md)).

An NRPN is CC 99 (the address MSB), CC 98 (the address LSB), then CC 6 (the value MSB) and, for a 14-bit value, CC 38 (the value LSB). The **NRPN** column gives the address as MSB:LSB, then as one 14-bit number (MSB × 128 + LSB).

The **Value** column says what the value carries:
- **14-bit**: the cell's whole range over 0 to 16383. A 7-bit sender's MSB alone (0 to 127) covers the range too.
- **MSB**: a step count from the cell's lowest value, in CC 6 alone (CC 38 is ignored); the range in brackets is the value shown on screen.
- **MSB code**: a choice, its code in CC 6. The codes never change; they are not the choices' order on screen.

An ENGINE's cells move only on a Part playing that engine. The master FX are shared by every Part: their NRPNs work on any Part's channel. ORBIT's ring cells move the playing scene. Addresses not listed here are reserved and do nothing.

With SETTINGS › MIDI CONFIG › ECHO › PARAM OUT at `NRPN`, Chimera sends these same NRPNs, on the Part's channel, for every cell you turn on the panel, so a sequencer can record your edits. A master FX or ORBIT ring cell goes out on the active Part's channel.

## ALGO

| Cell | NRPN | Value | Range |
|---|---|---|---|
| ALG A | 2:0 (256) | MSB code | 0 T1, 1 T2, 2 T3, 3 T4, 4 T5, 5 T6, 6 T7, 7 T8, 8 A1, 9 A2, 10 A3, 11 A4, 12 A5, 13 A6, 14 A7, 15 A8, 16 A9, 17 A10, 18 A11, 19 A12, 20 A13, 21 A14, 22 A15, 23 A16, 24 A17, 25 A18, 26 A19, 27 A20, 28 A21, 29 A22, 30 A23, 31 A24 |
| ALG B | 2:1 (257) | MSB code | 0 T1, 1 T2, 2 T3, 3 T4, 4 T5, 5 T6, 6 T7, 7 T8, 8 A1, 9 A2, 10 A3, 11 A4, 12 A5, 13 A6, 14 A7, 15 A8, 16 A9, 17 A10, 18 A11, 19 A12, 20 A13, 21 A14, 22 A15, 23 A16, 24 A17, 25 A18, 26 A19, 27 A20, 28 A21, 29 A22, 30 A23, 31 A24 |
| MORPH | 2:2 (258) | MSB | 0 to 127 |
| TRNSP | 2:3 (259) | MSB | 0 to 48 (-24 to 24) |
| VOWEL | 2:4 (260) | MSB | 0 to 100 |

### OPERATOR 1

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 3:0 (384) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 3:1 (385) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 3:2 (386) | MSB | 0 to 15 |
| DETUN | 3:3 (387) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 3:4 (388) | MSB | 0 to 99 |
| AR | 3:5 (389) | MSB | 0 to 31 |
| D1R | 3:6 (390) | MSB | 0 to 31 |
| D1L | 3:7 (391) | MSB | 0 to 15 |
| D2R | 3:8 (392) | MSB | 0 to 31 |
| RR | 3:9 (393) | MSB | 0 to 14 (1 to 15) |
| RS | 3:10 (394) | MSB | 0 to 3 |
| FDBK | 3:11 (395) | MSB | 0 to 7 |
| VEL | 3:12 (396) | MSB | 0 to 7 |
| FORMANT | 3:13 (397) | 14-bit | 0 to 168 |
| BW | 3:14 (398) | 14-bit | 0 to 216 |
| SKIRT | 3:15 (399) | MSB | 0 to 3 |
| KEY | 3:16 (400) | MSB | 0 to 100 |
| RES | 3:17 (401) | MSB | 0 to 100 |
| SLOT | 3:18 (402) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

### OPERATOR 2

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 4:0 (512) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 4:1 (513) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 4:2 (514) | MSB | 0 to 15 |
| DETUN | 4:3 (515) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 4:4 (516) | MSB | 0 to 99 |
| AR | 4:5 (517) | MSB | 0 to 31 |
| D1R | 4:6 (518) | MSB | 0 to 31 |
| D1L | 4:7 (519) | MSB | 0 to 15 |
| D2R | 4:8 (520) | MSB | 0 to 31 |
| RR | 4:9 (521) | MSB | 0 to 14 (1 to 15) |
| RS | 4:10 (522) | MSB | 0 to 3 |
| FDBK | 4:11 (523) | MSB | 0 to 7 |
| VEL | 4:12 (524) | MSB | 0 to 7 |
| FORMANT | 4:13 (525) | 14-bit | 0 to 168 |
| BW | 4:14 (526) | 14-bit | 0 to 216 |
| SKIRT | 4:15 (527) | MSB | 0 to 3 |
| KEY | 4:16 (528) | MSB | 0 to 100 |
| RES | 4:17 (529) | MSB | 0 to 100 |
| SLOT | 4:18 (530) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

### OPERATOR 3

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 5:0 (640) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 5:1 (641) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 5:2 (642) | MSB | 0 to 15 |
| DETUN | 5:3 (643) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 5:4 (644) | MSB | 0 to 99 |
| AR | 5:5 (645) | MSB | 0 to 31 |
| D1R | 5:6 (646) | MSB | 0 to 31 |
| D1L | 5:7 (647) | MSB | 0 to 15 |
| D2R | 5:8 (648) | MSB | 0 to 31 |
| RR | 5:9 (649) | MSB | 0 to 14 (1 to 15) |
| RS | 5:10 (650) | MSB | 0 to 3 |
| FDBK | 5:11 (651) | MSB | 0 to 7 |
| VEL | 5:12 (652) | MSB | 0 to 7 |
| FORMANT | 5:13 (653) | 14-bit | 0 to 168 |
| BW | 5:14 (654) | 14-bit | 0 to 216 |
| SKIRT | 5:15 (655) | MSB | 0 to 3 |
| KEY | 5:16 (656) | MSB | 0 to 100 |
| RES | 5:17 (657) | MSB | 0 to 100 |
| SLOT | 5:18 (658) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

### OPERATOR 4

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 6:0 (768) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 6:1 (769) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 6:2 (770) | MSB | 0 to 15 |
| DETUN | 6:3 (771) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 6:4 (772) | MSB | 0 to 99 |
| AR | 6:5 (773) | MSB | 0 to 31 |
| D1R | 6:6 (774) | MSB | 0 to 31 |
| D1L | 6:7 (775) | MSB | 0 to 15 |
| D2R | 6:8 (776) | MSB | 0 to 31 |
| RR | 6:9 (777) | MSB | 0 to 14 (1 to 15) |
| RS | 6:10 (778) | MSB | 0 to 3 |
| FDBK | 6:11 (779) | MSB | 0 to 7 |
| VEL | 6:12 (780) | MSB | 0 to 7 |
| FORMANT | 6:13 (781) | 14-bit | 0 to 168 |
| BW | 6:14 (782) | 14-bit | 0 to 216 |
| SKIRT | 6:15 (783) | MSB | 0 to 3 |
| KEY | 6:16 (784) | MSB | 0 to 100 |
| RES | 6:17 (785) | MSB | 0 to 100 |
| SLOT | 6:18 (786) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

### OPERATOR 5

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 7:0 (896) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 7:1 (897) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 7:2 (898) | MSB | 0 to 15 |
| DETUN | 7:3 (899) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 7:4 (900) | MSB | 0 to 99 |
| AR | 7:5 (901) | MSB | 0 to 31 |
| D1R | 7:6 (902) | MSB | 0 to 31 |
| D1L | 7:7 (903) | MSB | 0 to 15 |
| D2R | 7:8 (904) | MSB | 0 to 31 |
| RR | 7:9 (905) | MSB | 0 to 14 (1 to 15) |
| RS | 7:10 (906) | MSB | 0 to 3 |
| FDBK | 7:11 (907) | MSB | 0 to 7 |
| VEL | 7:12 (908) | MSB | 0 to 7 |
| FORMANT | 7:13 (909) | 14-bit | 0 to 168 |
| BW | 7:14 (910) | 14-bit | 0 to 216 |
| SKIRT | 7:15 (911) | MSB | 0 to 3 |
| KEY | 7:16 (912) | MSB | 0 to 100 |
| RES | 7:17 (913) | MSB | 0 to 100 |
| SLOT | 7:18 (914) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

### OPERATOR 6

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE | 8:0 (1024) | MSB code | 0 W1, 1 W2, 2 W3, 3 W4, 4 W5, 5 W6, 6 W7, 7 W8, 8 TRI, 9 SAW, 10 SQR, 11 P25, 12 P12, 13 TSAW, 14 RSQR, 15 SSAW, 16 FRMT, 17 NOISE |
| CRSE | 8:1 (1025) | MSB | 0 to 63 (0.50 to 25.95) |
| FINE | 8:2 (1026) | MSB | 0 to 15 |
| DETUN | 8:3 (1027) | MSB | 0 to 6 (-3 to 3) |
| LEVEL | 8:4 (1028) | MSB | 0 to 99 |
| AR | 8:5 (1029) | MSB | 0 to 31 |
| D1R | 8:6 (1030) | MSB | 0 to 31 |
| D1L | 8:7 (1031) | MSB | 0 to 15 |
| D2R | 8:8 (1032) | MSB | 0 to 31 |
| RR | 8:9 (1033) | MSB | 0 to 14 (1 to 15) |
| RS | 8:10 (1034) | MSB | 0 to 3 |
| FDBK | 8:11 (1035) | MSB | 0 to 7 |
| VEL | 8:12 (1036) | MSB | 0 to 7 |
| FORMANT | 8:13 (1037) | 14-bit | 0 to 168 |
| BW | 8:14 (1038) | 14-bit | 0 to 216 |
| SKIRT | 8:15 (1039) | MSB | 0 to 3 |
| KEY | 8:16 (1040) | MSB | 0 to 100 |
| RES | 8:17 (1041) | MSB | 0 to 100 |
| SLOT | 8:18 (1042) | MSB code | 0 AUTO, 1 F1, 2 F2, 3 F3, 4 OFF |

## MODAL

| Cell | NRPN | Value | Range |
|---|---|---|---|
| MODEL | 1:0 (128) | MSB code | 0 STRING, 1 BANK, 2 BOWED, 3 SYMP |
| STRUCT | 1:13 (141) | 14-bit | 0 to 16383, lowest to highest |
| BRIGHT | 1:3 (131) | 14-bit | 0 to 16383, lowest to highest |
| DAMP | 1:12 (140) | 14-bit | 0 to 16383, lowest to highest |
| POS | 1:4 (132) | 14-bit | 0 to 16383, lowest to highest |
| EXCITE | 1:1 (129) | 14-bit | 0 to 16383, lowest to highest |
| BODY | 1:6 (134) | 14-bit | 0 to 16383, lowest to highest |
| ENS.D | 1:9 (137) | 14-bit | 0 to 16383, lowest to highest |
| ENS.R | 1:10 (138) | 14-bit | 0 to 16383, lowest to highest |
| ENS.M | 1:11 (139) | 14-bit | 0 to 16383, lowest to highest |
| COUPLE | 1:14 (142) | 14-bit | 0 to 16383, lowest to highest |
| HALO | 1:15 (143) | 14-bit | 0 to 16383, lowest to highest |
| MODES | 1:16 (144) | MSB code | 0 16, 1 24, 2 32, 3 48 |
| COLOR | 1:19 (147) | 14-bit | 0 to 16383, lowest to highest |
| BURST | 1:20 (148) | 14-bit | 0 to 16383, lowest to highest |
| FORCE | 1:17 (145) | 14-bit | 0 to 16383, lowest to highest |
| SPEED | 1:18 (146) | 14-bit | 0 to 16383, lowest to highest |

## VA

| Cell | NRPN | Value | Range |
|---|---|---|---|
| WAVE1 | 35:0 (4480) | 14-bit | 0 to 16383, lowest to highest |
| WAVE2 | 35:1 (4481) | 14-bit | 0 to 16383, lowest to highest |
| MODE | 35:2 (4482) | MSB code | 0 MIX, 1 SYNC, 2 RING, 3 XOR, 4 BITS, 5 FM, 6 AM, 7 MAX, 8 LOOPFM, 9 SOFT SYNC, 10 PHASE |
| AMNT | 35:3 (4483) | 14-bit | 0 to 16383, lowest to highest |
| SUB | 35:4 (4484) | 14-bit | 0 to 16383, lowest to highest |
| STAB | 35:5 (4485) | 14-bit | 0 to 16383, lowest to highest |
| CRSE1 | 35:6 (4486) | MSB | 0 to 48 (-24 to 24) |
| FINE1 | 35:7 (4487) | 14-bit | 0 to 200 (-100 to 100) |
| SHAPE1 | 35:8 (4488) | 14-bit | 0 to 16383, lowest to highest |
| CRSE2 | 35:9 (4489) | MSB | 0 to 48 (-24 to 24) |
| FINE2 | 35:10 (4490) | 14-bit | 0 to 200 (-100 to 100) |
| SHAPE2 | 35:11 (4491) | 14-bit | 0 to 16383, lowest to highest |
| KIND | 35:12 (4492) | MSB code | 0 WHITE, 1 PINK, 2 RED, 3 DIGITAL |
| NOISE | 35:13 (4493) | 14-bit | 0 to 16383, lowest to highest |

## TAPE

| Cell | NRPN | Value | Range |
|---|---|---|---|
| MODEL | 34:0 (4352) | MSB code | 0 WALK, 3 WAVE, 4 SCRUB, 5 STRCH |
| START | 34:1 (4353) | 14-bit | 0 to 16383, lowest to highest |
| END | 34:2 (4354) | 14-bit | 0 to 16383, lowest to highest |
| POSITION | 34:3 (4355) | 14-bit | 0 to 16383, lowest to highest |
| MOTION | 34:4 (4356) | 14-bit | 0 to 16383, lowest to highest |
| TAPE | 34:5 (4357) | 14-bit | 0 to 16383, lowest to highest |
| AGE | 34:6 (4358) | 14-bit | 0 to 16383, lowest to highest |
| SIZE | 34:7 (4359) | 14-bit | 0 to 16383, lowest to highest |
| XFADE | 34:8 (4360) | 14-bit | 0 to 16383, lowest to highest |
| DRAG | 34:9 (4361) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 34:10 (4362) | MSB code | 0 CLASSIC, 1 CLS POLY, 2 GRAIN |
| ROOT | 34:11 (4363) | 14-bit | 0 to 16383, lowest to highest |
| PERIOD | 34:12 (4364) | 14-bit | 0 to 16383, lowest to highest |
| SLICE | 34:13 (4365) | MSB code | 0 OFF, 1 NOTE, 2 1, 3 2, 4 3, 5 4, 6 5, 7 6, 8 7, 9 8, 10 9, 11 10, 12 11, 13 12, 14 13, 15 14, 16 15, 17 16 |
| WOW | 34:14 (4366) | MSB code | 0 CAPSTAN, 1 WARBLE |
| DIRECTION | 34:15 (4367) | MSB code | 1 LOOP, 2 PING, 3 FWD, 4 REVERSE |
| RETRIG | 34:16 (4368) | MSB code | 0 STACK, 1 CUT |
| TRON | 34:19 (4371) | MSB code | 0 OFF, 1 ON |
| DRIFT | 34:20 (4372) | 14-bit | 0 to 16383, lowest to highest |
| KEY | 34:21 (4373) | MSB code | 0 POSITION, 1 PITCH |
| LENGTH | 34:22 (4374) | 14-bit | 0 to 16383, lowest to highest |
| GRAIN | 34:23 (4375) | 14-bit | 0 to 16383, lowest to highest |
| SPRAY | 34:24 (4376) | 14-bit | 0 to 16383, lowest to highest |
| FINISH | 34:25 (4377) | MSB | 0 to 1 (HOLD to STOP) |

## Every engine

### FILTER

| Cell | NRPN | Value | Range |
|---|---|---|---|
| CUTOFF | 10:0 (1280) | 14-bit | 0 to 16383, lowest to highest |
| RESO | 10:1 (1281) | 14-bit | 0 to 16383, lowest to highest |
| INPUT LEVEL | 10:12 (1292) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 10:6 (1286) | MSB code | 0 CLEAN, 1 LADDER, 4 2040, 6 DIRTY, 8 SEM |
| MODE | 10:7 (1287) | MSB code | 0 LP1, 1 LP2, 2 LP4, 3 BP2, 4 BP4, 5 HP4, 6 NOTCH1, 7 NOTCH2, 9 HP2 |
| VOICE | 10:13 (1293) | MSB code | 0 REV2, 1 EXPNDR |
| STAB | 10:14 (1294) | 14-bit | 0 to 16383, lowest to highest |
| FEEDBACK | 10:15 (1295) | 14-bit | 0 to 16383, lowest to highest |
| SEM MODE | 10:16 (1296) | 14-bit | 0 to 16383, lowest to highest |

### ENV 1

| Cell | NRPN | Value | Range |
|---|---|---|---|
| ATK | 12:0 (1536) | 14-bit | 0 to 16383, lowest to highest |
| DEC | 12:1 (1537) | 14-bit | 0 to 16383, lowest to highest |
| SUS | 12:2 (1538) | 14-bit | 0 to 16383, lowest to highest |
| REL | 12:3 (1539) | 14-bit | 0 to 16383, lowest to highest |
| LEVEL | 12:4 (1540) | 14-bit | 0 to 16383, lowest to highest |
| VEL | 12:5 (1541) | 14-bit | 0 to 16383, lowest to highest |
| H | 12:6 (1542) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 12:7 (1543) | MSB code | 0 A, 1 B |
| SPEED | 12:8 (1544) | MSB code | 0 FAST, 1 MED, 2 SLOW |
| HOLD | 12:9 (1545) | MSB code | 0 OFF, 1 AHDSR, 2 GATE EXT |
| TIME | 12:10 (1546) | 14-bit | 0 to 16383, lowest to highest |
| MODE | 12:11 (1547) | MSB code | 0 ENV, 1 LFO, 2 BURST |
| RISE | 12:13 (1549) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 12:14 (1550) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 12:15 (1551) | 14-bit | 0 to 16383, lowest to highest |
| ENV FORM | 12:16 (1552) | MSB code | 0 AD, 1 AHR, 2 CYCLE |
| LFO FORM | 12:17 (1553) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| BRST FORM | 12:18 (1554) | MSB code | 0 AD, 1 AHR, 2 CYCLE |

### ENV 2

| Cell | NRPN | Value | Range |
|---|---|---|---|
| ATK | 13:0 (1664) | 14-bit | 0 to 16383, lowest to highest |
| DEC | 13:1 (1665) | 14-bit | 0 to 16383, lowest to highest |
| SUS | 13:2 (1666) | 14-bit | 0 to 16383, lowest to highest |
| REL | 13:3 (1667) | 14-bit | 0 to 16383, lowest to highest |
| LEVEL | 13:4 (1668) | 14-bit | 0 to 16383, lowest to highest |
| VEL | 13:5 (1669) | 14-bit | 0 to 16383, lowest to highest |
| H | 13:6 (1670) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 13:7 (1671) | MSB code | 0 A, 1 B |
| SPEED | 13:8 (1672) | MSB code | 0 FAST, 1 MED, 2 SLOW |
| HOLD | 13:9 (1673) | MSB code | 0 OFF, 1 AHDSR, 2 GATE EXT |
| TIME | 13:10 (1674) | 14-bit | 0 to 16383, lowest to highest |
| MODE | 13:11 (1675) | MSB code | 0 ENV, 1 LFO, 2 BURST |
| RISE | 13:13 (1677) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 13:14 (1678) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 13:15 (1679) | 14-bit | 0 to 16383, lowest to highest |
| ENV FORM | 13:16 (1680) | MSB code | 0 AD, 1 AHR, 2 CYCLE |
| LFO FORM | 13:17 (1681) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| BRST FORM | 13:18 (1682) | MSB code | 0 AD, 1 AHR, 2 CYCLE |

### ENV 3

| Cell | NRPN | Value | Range |
|---|---|---|---|
| ATK | 14:0 (1792) | 14-bit | 0 to 16383, lowest to highest |
| DEC | 14:1 (1793) | 14-bit | 0 to 16383, lowest to highest |
| SUS | 14:2 (1794) | 14-bit | 0 to 16383, lowest to highest |
| REL | 14:3 (1795) | 14-bit | 0 to 16383, lowest to highest |
| LEVEL | 14:4 (1796) | 14-bit | 0 to 16383, lowest to highest |
| VEL | 14:5 (1797) | 14-bit | 0 to 16383, lowest to highest |
| H | 14:6 (1798) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 14:7 (1799) | MSB code | 0 A, 1 B |
| SPEED | 14:8 (1800) | MSB code | 0 FAST, 1 MED, 2 SLOW |
| HOLD | 14:9 (1801) | MSB code | 0 OFF, 1 AHDSR, 2 GATE EXT |
| TIME | 14:10 (1802) | 14-bit | 0 to 16383, lowest to highest |
| MODE | 14:11 (1803) | MSB code | 0 ENV, 1 LFO, 2 BURST |
| RISE | 14:13 (1805) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 14:14 (1806) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 14:15 (1807) | 14-bit | 0 to 16383, lowest to highest |
| ENV FORM | 14:16 (1808) | MSB code | 0 AD, 1 AHR, 2 CYCLE |
| LFO FORM | 14:17 (1809) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| BRST FORM | 14:18 (1810) | MSB code | 0 AD, 1 AHR, 2 CYCLE |

### LFO 1

| Cell | NRPN | Value | Range |
|---|---|---|---|
| RATE | 15:0 (1920) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 15:1 (1921) | MSB code | 0 SINE, 1 TRI, 2 SAW, 3 SQR, 4 S&H |
| SYNC | 15:2 (1922) | MSB code | 0 FREE, 1 RETRIG |
| PHASE | 15:3 (1923) | 14-bit | 0 to 16383, lowest to highest |
| DEPTH | 15:4 (1924) | 14-bit | 0 to 16383, lowest to highest |
| OFST | 15:5 (1925) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 15:6 (1926) | MSB code | 0 CLASSIC, 1 FUNC |
| FORM | 15:7 (1927) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| RISE | 15:8 (1928) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 15:9 (1929) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 15:10 (1930) | 14-bit | 0 to 16383, lowest to highest |

### LFO 2

| Cell | NRPN | Value | Range |
|---|---|---|---|
| RATE | 16:0 (2048) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 16:1 (2049) | MSB code | 0 SINE, 1 TRI, 2 SAW, 3 SQR, 4 S&H |
| SYNC | 16:2 (2050) | MSB code | 0 FREE, 1 RETRIG |
| PHASE | 16:3 (2051) | 14-bit | 0 to 16383, lowest to highest |
| DEPTH | 16:4 (2052) | 14-bit | 0 to 16383, lowest to highest |
| OFST | 16:5 (2053) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 16:6 (2054) | MSB code | 0 CLASSIC, 1 FUNC |
| FORM | 16:7 (2055) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| RISE | 16:8 (2056) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 16:9 (2057) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 16:10 (2058) | 14-bit | 0 to 16383, lowest to highest |

### LFO 3

| Cell | NRPN | Value | Range |
|---|---|---|---|
| RATE | 17:0 (2176) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 17:1 (2177) | MSB code | 0 SINE, 1 TRI, 2 SAW, 3 SQR, 4 S&H |
| SYNC | 17:2 (2178) | MSB code | 0 FREE, 1 RETRIG |
| PHASE | 17:3 (2179) | 14-bit | 0 to 16383, lowest to highest |
| DEPTH | 17:4 (2180) | 14-bit | 0 to 16383, lowest to highest |
| OFST | 17:5 (2181) | 14-bit | 0 to 16383, lowest to highest |
| TYPE | 17:6 (2182) | MSB code | 0 CLASSIC, 1 FUNC |
| FORM | 17:7 (2183) | MSB code | 0 FREE, 1 SYNC, 2 LFV |
| RISE | 17:8 (2184) | 14-bit | 0 to 16383, lowest to highest |
| FALL | 17:9 (2185) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 17:10 (2186) | 14-bit | 0 to 16383, lowest to highest |

### OUTPUT

| Cell | NRPN | Value | Range |
|---|---|---|---|
| LEVEL | 18:0 (2304) | 14-bit | 0 to 16383, lowest to highest |
| PAN | 18:1 (2305) | 14-bit | 0 to 16383, lowest to highest |
| VCA | 18:2 (2306) | 14-bit | 0 to 16383, lowest to highest |
| VEL | 18:3 (2307) | 14-bit | 0 to 16383, lowest to highest |
| CHORUS | 18:4 (2308) | 14-bit | 0 to 16383, lowest to highest |
| DELAY | 18:5 (2309) | 14-bit | 0 to 16383, lowest to highest |
| REVERB | 18:6 (2310) | 14-bit | 0 to 16383, lowest to highest |
| LPG | 18:7 (2311) | 14-bit | 0 to 16383, lowest to highest |
| DECAY | 18:8 (2312) | 14-bit | 0 to 16383, lowest to highest |
| STRIKE | 18:9 (2313) | 14-bit | 0 to 16383, lowest to highest |

### VOICE

| Cell | NRPN | Value | Range |
|---|---|---|---|
| PITCH | 19:0 (2432) | MSB | 0 to 48 (-24 to 24) |
| FINE | 19:1 (2433) | 14-bit | 0 to 200 (-100 to 100) |
| STEAL | 19:2 (2434) | MSB code | 0 CUT, 1 GLIDE |
| TIME | 19:3 (2435) | 14-bit | 0 to 16383, lowest to highest |
| SNAP | 19:4 (2436) | MSB code | 0 CHROMA, 1 MUSICAL |

### FX

| Cell | NRPN | Value | Range |
|---|---|---|---|
| AMOUNT | 36:2 (4610) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 36:3 (4611) | 14-bit | 0 to 16383, lowest to highest |
| LEVEL | 36:4 (4612) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 36:5 (4613) | 14-bit | 0 to 16383, lowest to highest |

## Part

### PART

| Cell | NRPN | Value | Range |
|---|---|---|---|
| LEVEL | 25:3 (3203) | 14-bit | 0 to 16383, lowest to highest |
| PAN | 25:4 (3204) | 14-bit | 0 to 16383, lowest to highest |

## Master FX

### CHORUS

| Cell | NRPN | Value | Range |
|---|---|---|---|
| RATE | 20:1 (2561) | 14-bit | 0 to 16383, lowest to highest |
| DEPTH | 20:2 (2562) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 20:3 (2563) | 14-bit | 0 to 16383, lowest to highest |

### DELAY

| Cell | NRPN | Value | Range |
|---|---|---|---|
| TIME | 21:0 (2688) | 14-bit | 0 to 16383, lowest to highest |
| FDBK | 21:1 (2689) | 14-bit | 0 to 16383, lowest to highest |
| MECHANICS | 21:2 (2690) | 14-bit | 0 to 16383, lowest to highest |
| SAT | 21:3 (2691) | 14-bit | 0 to 16383, lowest to highest |
| TONE | 21:4 (2692) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 21:5 (2693) | 14-bit | 0 to 16383, lowest to highest |
| REV | 21:6 (2694) | 14-bit | 0 to 16383, lowest to highest |
| SIZE | 21:8 (2696) | 14-bit | 0 to 16383, lowest to highest |
| DENSITY | 21:9 (2697) | 14-bit | 0 to 16383, lowest to highest |
| PITCH | 21:10 (2698) | MSB | 0 to 48 (-24 to 24) |
| TEXTURE | 21:11 (2699) | 14-bit | 0 to 16383, lowest to highest |

### REVERB

| Cell | NRPN | Value | Range |
|---|---|---|---|
| TIME | 22:1 (2817) | 14-bit | 0 to 16383, lowest to highest |
| DAMP | 22:2 (2818) | 14-bit | 0 to 16383, lowest to highest |
| SIZE | 22:3 (2819) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 22:4 (2820) | 14-bit | 0 to 16383, lowest to highest |
| TONE | 22:7 (2823) | 14-bit | 0 to 16383, lowest to highest |
| DIFFUSE | 22:8 (2824) | 14-bit | 0 to 16383, lowest to highest |
| PRE-DLY | 22:9 (2825) | 14-bit | 0 to 16383, lowest to highest |
| SHAPE | 22:10 (2826) | 14-bit | 0 to 16383, lowest to highest |
| SMEAR | 22:11 (2827) | 14-bit | 0 to 16383, lowest to highest |
| RATE | 22:12 (2828) | 14-bit | 0 to 16383, lowest to highest |
| DENSITY | 22:14 (2830) | 14-bit | 0 to 16383, lowest to highest |

### REEL

| Cell | NRPN | Value | Range |
|---|---|---|---|
| DRIVE | 23:0 (2944) | 14-bit | 0 to 16383, lowest to highest |
| TONE | 23:1 (2945) | 14-bit | 0 to 16383, lowest to highest |
| WOW | 23:2 (2946) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 23:3 (2947) | 14-bit | 0 to 16383, lowest to highest |

### COMP

| Cell | NRPN | Value | Range |
|---|---|---|---|
| THRESH | 24:0 (3072) | 14-bit | 0 to 16383, lowest to highest |
| ATK | 24:2 (3074) | 14-bit | 0 to 16383, lowest to highest |
| REL | 24:3 (3075) | 14-bit | 0 to 16383, lowest to highest |
| MAKEUP | 24:4 (3076) | 14-bit | 0 to 16383, lowest to highest |
| MIX | 24:5 (3077) | 14-bit | 0 to 16383, lowest to highest |
| VOL | 24:6 (3078) | 14-bit | 0 to 16383, lowest to highest |
| PAN | 24:7 (3079) | 14-bit | 0 to 16383, lowest to highest |

## ORBIT

### RING 1

| Cell | NRPN | Value | Range |
|---|---|---|---|
| HITS | 28:1 (3585) | MSB | 0 to 32 |
| ROTATE | 28:2 (3586) | MSB | 0 to 31 |
| CHANCE | 28:3 (3587) | MSB | 0 to 100 |
| MUTATE | 28:4 (3588) | MSB | 0 to 100 |
| GATE | 28:6 (3590) | MSB | 0 to 100 |
| VEL | 28:7 (3591) | MSB | 0 to 126 (1 to 127) |
| ACCENT | 28:8 (3592) | MSB | 0 to 32 |
| OCTAVE | 28:9 (3593) | MSB | 0 to 8 |

### RING 2

| Cell | NRPN | Value | Range |
|---|---|---|---|
| HITS | 29:1 (3713) | MSB | 0 to 32 |
| ROTATE | 29:2 (3714) | MSB | 0 to 31 |
| CHANCE | 29:3 (3715) | MSB | 0 to 100 |
| MUTATE | 29:4 (3716) | MSB | 0 to 100 |
| GATE | 29:6 (3718) | MSB | 0 to 100 |
| VEL | 29:7 (3719) | MSB | 0 to 126 (1 to 127) |
| ACCENT | 29:8 (3720) | MSB | 0 to 32 |
| OCTAVE | 29:9 (3721) | MSB | 0 to 8 |

### RING 3

| Cell | NRPN | Value | Range |
|---|---|---|---|
| HITS | 30:1 (3841) | MSB | 0 to 32 |
| ROTATE | 30:2 (3842) | MSB | 0 to 31 |
| CHANCE | 30:3 (3843) | MSB | 0 to 100 |
| MUTATE | 30:4 (3844) | MSB | 0 to 100 |
| GATE | 30:6 (3846) | MSB | 0 to 100 |
| VEL | 30:7 (3847) | MSB | 0 to 126 (1 to 127) |
| ACCENT | 30:8 (3848) | MSB | 0 to 32 |
| OCTAVE | 30:9 (3849) | MSB | 0 to 8 |

### RING 4

| Cell | NRPN | Value | Range |
|---|---|---|---|
| HITS | 31:1 (3969) | MSB | 0 to 32 |
| ROTATE | 31:2 (3970) | MSB | 0 to 31 |
| CHANCE | 31:3 (3971) | MSB | 0 to 100 |
| MUTATE | 31:4 (3972) | MSB | 0 to 100 |
| GATE | 31:6 (3974) | MSB | 0 to 100 |
| VEL | 31:7 (3975) | MSB | 0 to 126 (1 to 127) |
| ACCENT | 31:8 (3976) | MSB | 0 to 32 |
| OCTAVE | 31:9 (3977) | MSB | 0 to 8 |

### RING 5

| Cell | NRPN | Value | Range |
|---|---|---|---|
| HITS | 32:1 (4097) | MSB | 0 to 32 |
| ROTATE | 32:2 (4098) | MSB | 0 to 31 |
| CHANCE | 32:3 (4099) | MSB | 0 to 100 |
| MUTATE | 32:4 (4100) | MSB | 0 to 100 |
| GATE | 32:6 (4102) | MSB | 0 to 100 |
| VEL | 32:7 (4103) | MSB | 0 to 126 (1 to 127) |
| ACCENT | 32:8 (4104) | MSB | 0 to 32 |
| OCTAVE | 32:9 (4105) | MSB | 0 to 8 |
