# Per-Sound billing (#448): design

Status: built on `sound-billing` (ADR 0249). The rule, the guard and the re-bills are in the ADR. This page has the model's terms and the census.

## Goal
Voice admission charges each Sound only for what it turns on: operators, filter TYPE and its moving path, routed rows, the FX slot, NOISE, AMP's gate. Every term must stay an upper bound of what the unit reads for it, so admission never books past the audio deadline. Sound-identical: only `Cost` constants move, so every golden stays bit-identical.

## The model (`chimera-desktop/tests/sound_bill_test.rs`)
Bench-0.3 ROUTING and ALGO rows (rev V, 480 MHz, 2026-10-26, 4eae5d81), in cycles a sample a voice. Each term is one row less another, taken at the worst row that holds it.

Every reading is a warm 64-block average. Most are read at the mean voice. VA and ALGO's FRMT rows are read at the top voice; ALGO takes the higher of the two. None is a worst block. The guard therefore bounds the averages, not the deadline, until worst-block rows are read (ADR 0249 point 3).

| Term | Read | From |
|---|---|---|
| Bare 1 OP voice (chain, pool, CLEAN) | 483 | 1 DEST − first destination |
| First destination / each further | 6 / 4 | t13 1 CUTOFF − 1 OP; (VEL 4R − VEL 1R) / 3 |
| VCA: plain source / each more / ENV A / ENV B / BURST | 25 / 9 / 45 / 131 / 197 | VEL VCA, 2 VCA, A VCA, B GLIDE, BURST AD − bare |
| Slide (a slot's TIME..SHAPE routes) / FUNC LFO | 33 / 5 | A SLIDE − A VCA; FUNC LFO − 1 DEST |
| ALGO table operators and links over 1 OP | bill × 1.13 | WC's read over bill, against 1 OP's top 564 |
| ALGO formant / noise charges | bill × 1.07 / × 1.3 | FRMT PM's top; NOIS |
| VA MODE (top voice, routes off, less the 2040) | top − 59 − 74 | each MODE row's top voice |
| 2040 REV2 / EXP / LADDER / SEM (over CLEAN, steady) | 74 / 124 / 177 / 9 | rows − SVF; SEM from the 0.2 run |
| A moving filter (route) | +42; SEM +71; DIRTY 0 | CLEAN MOVE − SVF; SEM MOVE |
| FB (with its moving path) | CLEAN 195, DIRTY 115, 2040 192, LADDER 174, SEM 131 | FB rows − each row |
| CLEAN driven (LEVEL > ½) | 97 | LP24 HOT − SVF |
| STABILITY / taps FREQ, VCA | 5 / 178, 44 | STAB rows; TAP rows − 1 OP |
| FX FOLD / SOFT / FUZZ / FOLD TAP; ramps | 77 / 90 / 236 / 183; 22 / 25 / 43 | rows − 1 OP; RT rows |
| AMP gate / VA NOISE | 102 / 16–37 | LPG RT − A VCA; NZ rows − VA |
| MODAL engine | its bill | ADR 0218 set it at or over its rows |

Rows that carry the project's routes (ENV 1 → VCA, ENV 1 → CUTOFF, one more destination) are used only as differences between such rows.

## Needs the unit
- The new ROUTING rows: `2040 SWEEP`, `LADDER SWEEP`, `SEM SWEEP` and `VA SWEEP` against their steady rows (the moving paths' own worst), `VA WAVE` against `VA` (a WAVE segment crossed every block), `HOT RES` against SVF and LP24 HOT (RES apart from the drive), and `DIRTY HOT`.
- The existing rows read again with the routes cleared. That settles the terms the model now takes as bounds: the 2040's and CLEAN's moving bills, CLEAN driven, and the first destination.
- Then: close `KNOWN_UNDER` (raise what reads under, ALGO and the formant floor with the owner), and cut what reads over.

## Census
The public factory Sounds on rev V, QUAD, no ORBIT, in a room of 6,286, with voices capped at the 8-voice pool.

| Sound | Engine | Bill before | Bill after | Model | Margin | Voices before → after |
|---|---|---|---|---|---|---|
| TAPE DRIFT | Va | 819 | 819 | 798 | 21 | 7 → 7 |
| FADED HYMN | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| NEON BRASS | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| GROWL BASS | Va | 1036 | 1036 | 996 | 40 | 6 → 6 |
| FOLD BASS | Va | 943 | 943 | 909 | 34 | 6 → 6 |
| GLIDE BASS | Va | 815 | 815 | 782 | 33 | 7 → 7 |
| FIFTH SLIDE | Va | 815 | 815 | 782 | 33 | 7 → 7 |
| SYNC SCREAM | Va | 837 | 837 | 815 | 22 | 7 → 7 |
| RING LEAD | Va | 828 | 828 | 805 | 23 | 7 → 7 |
| SOFT FLUTE | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| SUPER SAWS | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| SOLO PORTA | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| XOR GRIT | Va | 837 | 837 | 813 | 24 | 7 → 7 |
| RING BLIPS | Va | 827 | 827 | 801 | 26 | 7 → 7 |
| WARM BRASS | Va | 816 | 816 | 786 | 30 | 7 → 7 |
| PULSE KEYS | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| SLOW TIDE | Va | 818 | 818 | 794 | 24 | 7 → 7 |
| BIT BLOOM | Va | 760 | 760 | 756 | 4 | 8 → 8 |
| ACID CLASSIC | Va | 918 | 918 | 885 | 33 | 6 → 6 |
| ACID DIRTY | Va | 707 | 707 | 701 | 6 | 8 → 8 |
| ACID WARM | Va | 873 | 873 | 832 | 41 | 7 → 7 |
| ACID TRI | Va | 918 | 918 | 885 | 33 | 6 → 6 |
| ACID MORPH | Va | 819 | 819 | 798 | 21 | 7 → 7 |
| ACID WONKY | Va | 827 | 827 | 801 | 26 | 7 → 7 |
| THE RIGHT PLACE | Va | 820 | 820 | 787 | 33 | 7 → 7 |
| VELVET FOG | Va | 733 | 753 | 750 | 3 | 8 → 8 |
| AMBER GLIDE | Va | 816 | 816 | 786 | 30 | 7 → 7 |
| SYNC VOICE | Va | 836 | 836 | 811 | 25 | 7 → 7 |
| FLUTTER KEYS | Va | 822 | 822 | 787 | 35 | 7 → 7 |
| STEPPED RAIN | Va | 816 | 816 | 786 | 30 | 7 → 7 |
| PIXEL PLUCK | Va | 758 | 758 | 728 | 30 | 8 → 8 |
| GRAIN SWARM | Va | 825 | 825 | 799 | 26 | 7 → 7 |
| VOWEL BUZZ | Algo | 956 | 956 | 1017 | -61 | 6 → 6 |
| THROAT SING | Algo | 956 | 956 | 1017 | -61 | 6 → 6 |
| GLASS MALLET | Algo | 718 | 718 | 723 | -5 | 8 → 8 |
| RUBBER BASS | Algo | 868 | 868 | 844 | 24 | 7 → 7 |
| LAST LIGHT | Algo | 990 | 990 | 988 | 2 | 6 → 6 |
| VOCAL KEYS | Algo | 1435 | 1435 | 1488 | -53 | 4 → 4 |
| SINGING BASS | Algo | 1452 | 1452 | 1490 | -38 | 4 → 4 |
| WAH BRASS | Algo | 1384 | 1384 | 1433 | -49 | 4 → 4 |
| TALKING BELL | Algo | 1194 | 1194 | 1230 | -36 | 5 → 5 |
| HUMMING PAD | Algo | 1311 | 1311 | 1320 | -9 | 4 → 4 |
| YELLING LEAD | Algo | 1226 | 1226 | 1266 | -40 | 5 → 5 |
| RUST REESE | Algo | 979 | 979 | 944 | 35 | 6 → 6 |
| IRON SUB | Algo | 803 | 803 | 779 | 24 | 7 → 7 |
| EBM PULSE | Algo | 861 | 861 | 836 | 25 | 7 → 7 |
| GRIND BASS | Algo | 876 | 876 | 863 | 13 | 7 → 7 |
| BILE LEAD | Algo | 961 | 961 | 943 | 18 | 6 → 6 |
| RING SIREN | Algo | 794 | 794 | 766 | 28 | 7 → 7 |
| NASAL MONO | Algo | 761 | 761 | 767 | -6 | 8 → 8 |
| DEAD CHOIR | Algo | 1319 | 1319 | 1330 | -11 | 4 → 4 |
| RUST PAD | Algo | 862 | 862 | 840 | 22 | 7 → 7 |
| AIR RAID | Algo | 870 | 870 | 849 | 21 | 7 → 7 |
| AGGRO STAB | Algo | 840 | 840 | 837 | 3 | 7 → 7 |
| BLACK BRASS | Algo | 862 | 862 | 840 | 22 | 7 → 7 |
| GIRDER | Algo | 802 | 802 | 775 | 27 | 7 → 7 |
| SERVO | Algo | 1044 | 1044 | 1040 | 4 | 6 → 6 |
| CRACK SNARE | Algo | 1000 | 1000 | 1004 | -4 | 6 → 6 |
| DEAD RADIO | Algo | 759 | 759 | 759 | 0 | 8 → 8 |
| SHARD LEAD | Algo | 871 | 871 | 853 | 18 | 7 → 7 |
| BROKEN WIRE | Algo | 852 | 852 | 858 | -6 | 7 → 7 |
| HEX VOICE | Algo | 1192 | 1192 | 1189 | 3 | 5 → 5 |
| NULL CHOIR | Algo | 1450 | 1450 | 1490 | -40 | 4 → 4 |
| BLACK SUN | Algo | 853 | 853 | 830 | 23 | 7 → 7 |
| SODIUM HAZE | Algo | 975 | 975 | 974 | 1 | 6 → 6 |
| SIGNAL ROT | Algo | 1013 | 1013 | 1011 | 2 | 6 → 6 |
| TORN BASS | Algo | 1063 | 1063 | 1042 | 21 | 5 → 5 |
| BENT BASS | Algo | 800 | 800 | 767 | 33 | 7 → 7 |
| MOTOR BASS | Algo | 841 | 841 | 840 | 1 | 7 → 7 |
| RIOT STAB | Algo | 824 | 824 | 828 | -4 | 7 → 7 |
| ASH CHORD | Algo | 1328 | 1328 | 1340 | -12 | 4 → 4 |
| TIN CAN | Algo | 801 | 801 | 771 | 30 | 7 → 7 |
| BIT SHRED | Algo | 863 | 863 | 849 | 14 | 7 → 7 |
| SPARK GAP | Algo | 999 | 999 | 1000 | -1 | 6 → 6 |
| SONAR VOID | Algo | 808 | 808 | 776 | 32 | 7 → 7 |
| NYLON | Modal | 670 | 670 | 673 | -3 | 8 → 8 |
| GAMELAN | Modal | 1961 | 1961 | 1967 | -6 | 3 → 3 |
| CELLO SWELL | Modal | 727 | 727 | 703 | 24 | 8 → 8 |
| SITAR DRONE | Modal | 944 | 944 | 917 | 27 | 6 → 6 |
| GLITCH BARS | Modal | 1963 | 1963 | 1975 | -12 | 3 → 3 |
| SOLID BASS | Algo | 836 | 836 | 868 | -32 | 7 → 7 |
| WOOD BASS | Algo | 923 | 923 | 898 | 25 | 6 → 6 |
| MORPH BASS | Algo | 805 | 805 | 807 | -2 | 7 → 7 |
| TINE PIANO | Algo | 720 | 720 | 728 | -8 | 8 → 8 |
| GLASS KEYS | Algo | 832 | 832 | 815 | 17 | 7 → 7 |
| FUNK CLAV | Algo | 836 | 836 | 868 | -32 | 7 → 7 |
| DRIFT ORGAN | Algo | 702 | 702 | 705 | -3 | 8 → 8 |
| TUBULAR | Algo | 770 | 770 | 765 | 5 | 8 → 8 |
| ICE BELLS | Algo | 717 | 717 | 719 | -2 | 8 → 8 |
| MUSIC BOX | Algo | 717 | 717 | 719 | -2 | 8 → 8 |
| FM BRASS | Algo | 725 | 725 | 725 | 0 | 8 → 8 |
| SOFT HORNS | Algo | 1591 | 1591 | 1630 | -39 | 3 → 3 |
| WARM PAD | Algo | 1437 | 1437 | 1496 | -59 | 4 → 4 |
| GLASS PAD | Algo | 1429 | 1429 | 1484 | -55 | 4 → 4 |
| KOTO PLUCK | Algo | 922 | 922 | 894 | 28 | 6 → 6 |
| MALLET | Algo | 869 | 869 | 845 | 24 | 7 → 7 |
