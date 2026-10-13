//! The TX81Z VMEM parser (importer spec § 5.2, § 8.1). Every fixture is
//! built here from field values or chart offsets: never a Yamaha bank.

use chimera_engine::import::tx81z::{
    BANK_LEN, Checksum, Op, ParseError, Ranged, VOICE_LEN, VOICES, Voice, bank_bytes, parse,
};
use proptest::prelude::*;

/// A bank of `voices` raw 128 B voices, header, checksum and `F7`.
fn raw_bank(voices: &[[u8; VOICE_LEN]; VOICES]) -> Vec<u8> {
    let mut b = vec![0xF0, 0x43, 0x00, 0x04, 0x20, 0x00];
    for v in voices {
        b.extend_from_slice(v);
    }
    let sum: u32 = b[6..].iter().map(|&x| u32::from(x)).sum();
    b.push(((0x80 - (sum & 0x7F)) & 0x7F) as u8);
    b.push(0xF7);
    b
}

/// A plain voice: RR 1 (the floor), so nothing reads as adjusted.
fn plain() -> [u8; VOICE_LEN] {
    let mut v = [0u8; VOICE_LEN];
    for op in 0..4 {
        v[op * 10 + 3] = 1;
    }
    v
}

fn sample() -> Voice {
    let mut v = Voice::default();
    for (i, op) in v.ops.iter_mut().enumerate() {
        let n = i as u8;
        *op = Op {
            ar: Ranged::new(31 - n),
            d1r: Ranged::new(10 + n),
            d2r: Ranged::new(3 + n),
            rr: Ranged::new(7 + n),
            d1l: Ranged::new(12 - n),
            ls: Ranged::new(20 * n),
            ame: n % 2 == 1,
            ebs: Ranged::new(n),
            kvs: Ranged::new(2 + n),
            out: Ranged::new(99 - 9 * n),
            crs: Ranged::new(4 + 5 * n),
            rs: Ranged::new(n),
            det: Ranged::new(3 + n % 3),
            egsft: Ranged::new(n % 4),
            fix: n == 2,
            fixrg: Ranged::new(n + 1),
            osw: Ranged::new(2 * n),
            fine: Ranged::new(3 * n),
        };
    }
    v.sync = true;
    v.fbl = Ranged::new(5);
    v.alg = Ranged::new(4);
    v.lfs = Ranged::new(35);
    v.lfd = Ranged::new(12);
    v.pmd = Ranged::new(8);
    v.amd = Ranged::new(40);
    v.pms = Ranged::new(3);
    v.ams = Ranged::new(2);
    v.lfw = Ranged::new(2);
    v.trps = Ranged::new(26);
    v.pbr = Ranged::new(4);
    v.mode = Ranged::new(0b01000);
    v.port = Ranged::new(9);
    v.fc_vol = Ranged::new(40);
    v.mw_pitch = Ranged::new(50);
    v.mw_ampl = Ranged::new(1);
    v.bc_pitch = Ranged::new(2);
    v.bc_ampl = Ranged::new(3);
    v.bc_pbias = Ranged::new(50);
    v.bc_ebias = Ranged::new(5);
    v.rev = Ranged::new(1);
    v.fc_pitch = Ranged::new(6);
    v.fc_ampl = Ranged::new(7);
    v.name = *b"TESTVOICE1";
    v
}

#[test]
fn reads_each_field_at_its_chart_offset() {
    let mut voices = [plain(); VOICES];
    let v = &mut voices[1];
    // Stored position 0 is OP4, 3 is OP1.
    v[0] = 21; // OP4 AR
    v[1] = 9; // D1R
    v[2] = 4; // D2R
    v[3] = 6; // RR
    v[4] = 11; // D1L
    v[5] = 33; // LS
    v[6] = 0x40 | (5 << 3) | 6; // AME, EBS 5, KVS 6
    v[7] = 88; // OUT
    v[8] = 17; // CRS
    v[9] = (2 << 3) | 5; // RS 2, DET 5
    v[30 + 7] = 70; // OP1 OUT
    v[10 + 8] = 9; // OP2 CRS
    v[20 + 8] = 13; // OP3 CRS
    v[40] = 0x40 | (6 << 3) | 3; // SYNC, FBL 6, ALG 4
    v[41..45].copy_from_slice(&[30, 1, 2, 3]); // LFS, LFD, PMD, AMD
    v[45] = (5 << 4) | (3 << 2) | 1; // PMS 5, AMS 3, SQUARE
    v[46..57].copy_from_slice(&[30, 7, 0b01000, 12, 1, 2, 3, 4, 5, 50, 6]);
    v[57..67].copy_from_slice(b"FIXTURE 01");
    v[73] = (2 << 4) | 0x08 | 5; // OP4 EGSFT 2, FIX, FIXRG 5
    v[74] = (6 << 4) | 9; // OP4 W7, FINE 9
    v[79] = 0x08; // OP1 FIX
    v[80] = (1 << 4) | 3; // OP1 W2, FINE 3
    v[81..84].copy_from_slice(&[4, 8, 9]); // REV, FC PITCH, FC AMPL
    let bank = parse(&raw_bank(&voices)).unwrap();
    assert_eq!(bank.checksum, None);
    let got = bank.voices[1];
    let op4 = got.ops[3];
    assert_eq!(
        (op4.ar.get(), op4.d1r.get(), op4.d2r.get(), op4.rr.get()),
        (21, 9, 4, 6)
    );
    assert_eq!((op4.d1l.get(), op4.ls.get()), (11, 33));
    assert_eq!((op4.ame, op4.ebs.get(), op4.kvs.get()), (true, 5, 6));
    assert_eq!((op4.out.get(), op4.crs.get()), (88, 17));
    assert_eq!((op4.rs.get(), op4.det.get()), (2, 5));
    assert_eq!((op4.egsft.get(), op4.fix, op4.fixrg.get()), (2, true, 5));
    assert_eq!((op4.osw.get(), op4.fine.get()), (6, 9));
    assert_eq!(got.ops[0].out.get(), 70, "stored 3 is OP1");
    assert_eq!(got.ops[1].crs.get(), 9, "stored 1 is OP2");
    assert_eq!(got.ops[2].crs.get(), 13, "stored 2 is OP3");
    assert!(got.ops[0].fix);
    assert_eq!((got.ops[0].osw.get(), got.ops[0].fine.get()), (1, 3));
    assert_eq!((got.sync, got.fbl.get(), got.alg.get()), (true, 6, 3));
    assert_eq!(
        (got.lfs.get(), got.lfd.get(), got.pmd.get(), got.amd.get()),
        (30, 1, 2, 3)
    );
    assert_eq!((got.pms.get(), got.ams.get(), got.lfw.get()), (5, 3, 1));
    assert_eq!((got.trps.get(), got.pbr.get(), got.mode.get()), (30, 7, 8));
    assert!(got.mono());
    assert_eq!(
        (
            got.port.get(),
            got.fc_vol.get(),
            got.mw_pitch.get(),
            got.mw_ampl.get()
        ),
        (12, 1, 2, 3)
    );
    assert_eq!(
        (
            got.bc_pitch.get(),
            got.bc_ampl.get(),
            got.bc_pbias.get(),
            got.bc_ebias.get()
        ),
        (4, 5, 50, 6)
    );
    assert_eq!(
        (got.rev.get(), got.fc_pitch.get(), got.fc_ampl.get()),
        (4, 8, 9)
    );
    assert_eq!(&got.name, b"FIXTURE 01");
    assert_eq!(bank.warn[1].adjusted, 0);
    assert!(!bank.warn[1].extra);
}

#[test]
fn pack_round_trips_every_field() {
    let mut voices = [Voice::default(); VOICES];
    voices[0] = sample();
    voices[31] = sample();
    voices[31].alg = Ranged::new(7);
    voices[31].ops[3].fine = Ranged::new(15);
    let bytes = bank_bytes(&voices);
    assert_eq!(bytes.len(), BANK_LEN);
    let bank = parse(&bytes).unwrap();
    assert_eq!(bank.checksum, None);
    assert_eq!(bank.voices[0], voices[0]);
    assert_eq!(bank.voices[31], voices[31]);
    // Default voices have RR 1 already: nothing adjusted.
    assert!(bank.warn.iter().all(|w| w.adjusted == 0 && !w.extra));
}

#[test]
fn dx21_zeros_read_as_aced_defaults() {
    let mut voices = [plain(); VOICES];
    voices[0][7] = 90;
    let bank = parse(&raw_bank(&voices)).unwrap();
    for op in bank.voices[0].ops {
        assert_eq!(
            (op.osw.get(), op.fine.get(), op.fix, op.egsft.get()),
            (0, 0, false, 0)
        );
    }
}

#[test]
fn wrong_size_header_or_format_refused() {
    let good = bank_bytes(&[Voice::default(); VOICES]);
    assert_eq!(parse(&good[..4103]), Err(ParseError::Size(4103)));
    assert_eq!(parse(&[]), Err(ParseError::Size(0)));
    let mut b = good;
    b[0] = 0xF1;
    assert_eq!(parse(&b), Err(ParseError::Header));
    let mut b = good;
    b[1] = 0x41;
    assert_eq!(parse(&b), Err(ParseError::Header));
    let mut b = good;
    b[2] = 0x10;
    assert_eq!(parse(&b), Err(ParseError::Header));
    let mut b = good;
    b[BANK_LEN - 1] = 0x00;
    assert_eq!(parse(&b), Err(ParseError::Header));
    let mut b = good;
    b[3] = 0x09; // a DX7 bank
    assert_eq!(parse(&b), Err(ParseError::Format(0x09)));
    let mut b = good;
    b[4] = 0x10;
    assert_eq!(parse(&b), Err(ParseError::Format(0x04)));
    // Any MIDI channel is a TX81Z bank.
    let mut b = good;
    b[2] = 0x0F;
    assert!(parse(&b).is_ok());
}

#[test]
fn bad_checksum_warns() {
    let mut voices = [Voice::default(); VOICES];
    voices[4] = sample();
    let mut b = bank_bytes(&voices);
    let right = b[BANK_LEN - 2];
    b[BANK_LEN - 2] = (right + 1) & 0x7F;
    let bank = parse(&b).unwrap();
    assert_eq!(
        bank.checksum,
        Some(Checksum {
            expected: right,
            found: (right + 1) & 0x7F
        })
    );
    assert_eq!(bank.voices[4], voices[4], "the voices still come out");
}

#[test]
fn masked_then_clamped_counts_each_byte() {
    let mut voices = [plain(); VOICES];
    let v = &mut voices[2];
    v[7] = 120; // OUT past 99
    v[0] = 0x80 | 5; // AR with the top bit set
    v[9] = 0x60 | 6; // RS 0, DET 6, unused bits 5-6 set
    v[13] = 0; // OP2 RR 0, under its floor of 1
    v[46] = 60; // TRPS past 48
    v[45] = 0x80 | 0x02; // the top bit beside LFW
    v[57..67].copy_from_slice(b"ANY NAME 1");
    v[67] = 99; // PR1: ignored, not counted
    let bank = parse(&raw_bank(&voices)).unwrap();
    let got = bank.voices[2];
    assert_eq!(got.ops[3].out.get(), 99);
    assert_eq!(got.ops[3].ar.get(), 5);
    assert_eq!(got.ops[3].det.get(), 6);
    assert_eq!(got.ops[1].rr.get(), 1);
    assert_eq!(got.trps.get(), 48);
    assert_eq!(got.lfw.get(), 2);
    assert_eq!(bank.warn[2].adjusted, 6);
    assert!(!bank.warn[2].extra);
}

#[test]
fn extra_bytes_warn() {
    let mut voices = [plain(); VOICES];
    voices[7][100] = 3;
    let bank = parse(&raw_bank(&voices)).unwrap();
    assert!(bank.warn[7].extra);
    assert!(!bank.warn[6].extra);
}

proptest! {
    #[test]
    fn parse_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..5000)) {
        let _ = parse(&bytes);
    }

    #[test]
    fn parse_never_panics_on_mutated_banks(
        edits in proptest::collection::vec((0..BANK_LEN, any::<u8>()), 1..64)
    ) {
        let mut voices = [Voice::default(); VOICES];
        voices[0] = sample();
        let mut b = bank_bytes(&voices);
        for (i, x) in edits {
            if (6..BANK_LEN - 1).contains(&i) {
                b[i] = x;
            }
        }
        let bank = parse(&b).unwrap();
        // Whatever the bytes, every field is in its range.
        for v in bank.voices {
            prop_assert!(v.alg.get() <= 7 && v.trps.get() <= 48);
            for op in v.ops {
                prop_assert!(op.out.get() <= 99 && (1..=15).contains(&op.rr.get()));
            }
        }
    }
}
