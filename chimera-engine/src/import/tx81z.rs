//! The TX81Z's 32-voice VMEM bank (importer spec § 5.2), from the owner's
//! manual's MIDI data chart: 32 × 128 B packed voices, operators stored
//! OP4, OP2, OP3, OP1. A DX21, DX27 or DX100 bank is the same with zeros
//! at 73-83, which read as W1, FINE 0, not fixed, no EG shift.
//!
//! Each field is masked to its own bits, then clamped to its range; a
//! byte either step changed counts in `Warn::adjusted`. A bad checksum is
//! a warning, never a refusal (the owner's ruling, Dexed's precedent).

/// `F0 43 0n 04 20 00`, 4096 B of voices, the checksum, `F7`.
pub const BANK_LEN: usize = 4104;
pub const VOICES: usize = 32;
pub const VOICE_LEN: usize = 128;
const HEAD: usize = 6;
/// The VMEM format byte (header byte 3).
pub const FORMAT: u8 = 0x04;

/// A field's value, clamped into `MIN..=MAX` when made: a map never
/// reads a raw byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ranged<const MIN: u8, const MAX: u8>(u8);

impl<const MIN: u8, const MAX: u8> Ranged<MIN, MAX> {
    pub const fn new(v: u8) -> Self {
        Self(if v < MIN {
            MIN
        } else if v > MAX {
            MAX
        } else {
            v
        })
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl<const MIN: u8, const MAX: u8> Default for Ranged<MIN, MAX> {
    fn default() -> Self {
        Self(MIN)
    }
}

pub type U99 = Ranged<0, 99>;

/// One operator, as the TX81Z numbers its fields (VCED and ACED).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Op {
    pub ar: Ranged<0, 31>,
    pub d1r: Ranged<0, 31>,
    pub d2r: Ranged<0, 31>,
    pub rr: Ranged<1, 15>,
    pub d1l: Ranged<0, 15>,
    /// Keyboard level scaling.
    pub ls: U99,
    /// Amplitude modulation on.
    pub ame: bool,
    /// EG bias sensitivity (a controller's).
    pub ebs: Ranged<0, 7>,
    /// Key velocity sensitivity.
    pub kvs: Ranged<0, 7>,
    pub out: U99,
    pub crs: Ranged<0, 63>,
    /// Keyboard rate scaling.
    pub rs: Ranged<0, 3>,
    /// 0-6, 3 the centre.
    pub det: Ranged<0, 6>,
    /// EG shift: 0 none, then 48, 24 and 12 dB floors.
    pub egsft: Ranged<0, 3>,
    pub fix: bool,
    pub fixrg: Ranged<0, 7>,
    /// Wave W1-W8, 0-7.
    pub osw: Ranged<0, 7>,
    pub fine: Ranged<0, 15>,
}

/// One voice: `ops[0]` is OP1. The name is read but never mapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voice {
    pub ops: [Op; 4],
    /// LFO key sync.
    pub sync: bool,
    pub fbl: Ranged<0, 7>,
    /// ALG 1-8, 0-7.
    pub alg: Ranged<0, 7>,
    pub lfs: U99,
    pub lfd: U99,
    pub pmd: U99,
    pub amd: U99,
    pub pms: Ranged<0, 7>,
    pub ams: Ranged<0, 3>,
    /// SAW UP, SQUARE, TRIANGLE, S/H.
    pub lfw: Ranged<0, 3>,
    /// 0-48, 24 no transpose.
    pub trps: Ranged<0, 48>,
    pub pbr: Ranged<0, 12>,
    /// Bit 4 chorus (DX21), 3 mono, 2 sustain foot switch, 1 portamento
    /// foot switch, 0 portamento mode.
    pub mode: Ranged<0, 31>,
    pub port: U99,
    pub fc_vol: U99,
    pub mw_pitch: U99,
    pub mw_ampl: U99,
    pub bc_pitch: U99,
    pub bc_ampl: U99,
    pub bc_pbias: Ranged<0, 100>,
    pub bc_ebias: U99,
    /// Reverb (release) rate.
    pub rev: Ranged<0, 7>,
    pub fc_pitch: U99,
    pub fc_ampl: U99,
    pub name: [u8; 10],
}

impl Voice {
    pub const fn mono(&self) -> bool {
        self.mode.get() & 0x08 != 0
    }
}

/// Stored position `i` holds OP`STORED[i]`.
const STORED: [usize; 4] = [4, 2, 3, 1];

/// What reading a voice changed: bytes masked or clamped (names, PR/PL and
/// 84-127 aside), and DX11/V50 extra data in 84-127.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Warn {
    pub adjusted: u8,
    pub extra: bool,
}

/// The checksum the payload sums to, against the one the file carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checksum {
    pub expected: u8,
    pub found: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bank {
    pub voices: [Voice; VOICES],
    pub warn: [Warn; VOICES],
    pub checksum: Option<Checksum>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Not 4104 B.
    Size(usize),
    /// Not `F0 43 0n`, or no `F7` at the end.
    Header,
    /// A format byte other than VMEM's, or a byte count other than 4096.
    Format(u8),
}

/// A VMEM bank; refused only for its size, header or format.
pub fn parse(bytes: &[u8]) -> Result<Bank, ParseError> {
    if bytes.len() != BANK_LEN {
        return Err(ParseError::Size(bytes.len()));
    }
    if bytes[0] != 0xF0 || bytes[1] != 0x43 || bytes[2] > 0x0F || bytes[BANK_LEN - 1] != 0xF7 {
        return Err(ParseError::Header);
    }
    if bytes[3] != FORMAT {
        return Err(ParseError::Format(bytes[3]));
    }
    if bytes[4..HEAD] != [0x20, 0x00] {
        return Err(ParseError::Format(FORMAT));
    }
    let payload = &bytes[HEAD..HEAD + VOICES * VOICE_LEN];
    let expected = checksum(payload);
    let found = bytes[BANK_LEN - 2];
    let mut bank = Bank {
        voices: [Voice::default(); VOICES],
        warn: [Warn::default(); VOICES],
        checksum: (expected != found).then_some(Checksum { expected, found }),
    };
    for (i, v) in payload.as_chunks::<VOICE_LEN>().0.iter().enumerate() {
        let voice = Voice::unpack(v);
        let packed = voice.pack();
        let read = (0..67).chain(73..84);
        bank.warn[i] = Warn {
            adjusted: read.filter(|&b| packed[b] != v[b]).count() as u8,
            extra: v[84..].iter().any(|&b| b != 0),
        };
        bank.voices[i] = voice;
    }
    Ok(bank)
}

/// `(-Σ bytes) & 0x7F`.
fn checksum(payload: &[u8]) -> u8 {
    let sum = payload.iter().fold(0u8, |a, &b| a.wrapping_add(b));
    sum.wrapping_neg() & 0x7F
}

/// The bank bytes of `voices`, with its checksum: what `parse` reads.
pub fn bank_bytes(voices: &[Voice; VOICES]) -> [u8; BANK_LEN] {
    let mut b = [0u8; BANK_LEN];
    b[..HEAD].copy_from_slice(&[0xF0, 0x43, 0x00, FORMAT, 0x20, 0x00]);
    for (i, v) in voices.iter().enumerate() {
        let at = HEAD + i * VOICE_LEN;
        b[at..at + VOICE_LEN].copy_from_slice(&v.pack());
    }
    b[BANK_LEN - 2] = checksum(&b[HEAD..BANK_LEN - 2]);
    b[BANK_LEN - 1] = 0xF7;
    b
}

/// `byte`'s bits `lo..lo + width`.
const fn bits(byte: u8, lo: u8, width: u8) -> u8 {
    (byte >> lo) & ((1 << width) - 1)
}

impl Voice {
    /// The 128 B VMEM voice; PR/PL and 84-127 zero.
    pub fn pack(&self) -> [u8; VOICE_LEN] {
        let mut v = [0u8; VOICE_LEN];
        for (s, &n) in STORED.iter().enumerate() {
            let o = &self.ops[n - 1];
            let b = &mut v[s * 10..s * 10 + 10];
            b[0] = o.ar.get();
            b[1] = o.d1r.get();
            b[2] = o.d2r.get();
            b[3] = o.rr.get();
            b[4] = o.d1l.get();
            b[5] = o.ls.get();
            b[6] = u8::from(o.ame) << 6 | o.ebs.get() << 3 | o.kvs.get();
            b[7] = o.out.get();
            b[8] = o.crs.get();
            b[9] = o.rs.get() << 3 | o.det.get();
            v[73 + 2 * s] = o.egsft.get() << 4 | u8::from(o.fix) << 3 | o.fixrg.get();
            v[74 + 2 * s] = o.osw.get() << 4 | o.fine.get();
        }
        v[40] = u8::from(self.sync) << 6 | self.fbl.get() << 3 | self.alg.get();
        v[41] = self.lfs.get();
        v[42] = self.lfd.get();
        v[43] = self.pmd.get();
        v[44] = self.amd.get();
        v[45] = self.pms.get() << 4 | self.ams.get() << 2 | self.lfw.get();
        v[46..57].copy_from_slice(&[
            self.trps.get(),
            self.pbr.get(),
            self.mode.get(),
            self.port.get(),
            self.fc_vol.get(),
            self.mw_pitch.get(),
            self.mw_ampl.get(),
            self.bc_pitch.get(),
            self.bc_ampl.get(),
            self.bc_pbias.get(),
            self.bc_ebias.get(),
        ]);
        for (d, &c) in v[57..67].iter_mut().zip(&self.name) {
            *d = c & 0x7F;
        }
        v[81] = self.rev.get();
        v[82] = self.fc_pitch.get();
        v[83] = self.fc_ampl.get();
        v
    }

    /// Each field masked to its bits, then clamped (`Ranged::new`).
    pub fn unpack(raw: &[u8; VOICE_LEN]) -> Voice {
        let mut r = *raw;
        for b in &mut r {
            *b &= 0x7F;
        }
        let mut v = Voice::default();
        for (s, &op) in STORED.iter().enumerate() {
            let b = &r[s * 10..s * 10 + 10];
            let (a, w) = (r[73 + 2 * s], r[74 + 2 * s]);
            v.ops[op - 1] = Op {
                ar: Ranged::new(b[0]),
                d1r: Ranged::new(b[1]),
                d2r: Ranged::new(b[2]),
                rr: Ranged::new(b[3]),
                d1l: Ranged::new(b[4]),
                ls: Ranged::new(b[5]),
                ame: bits(b[6], 6, 1) == 1,
                ebs: Ranged::new(bits(b[6], 3, 3)),
                kvs: Ranged::new(bits(b[6], 0, 3)),
                out: Ranged::new(b[7]),
                crs: Ranged::new(b[8]),
                rs: Ranged::new(bits(b[9], 3, 2)),
                det: Ranged::new(bits(b[9], 0, 3)),
                egsft: Ranged::new(bits(a, 4, 2)),
                fix: bits(a, 3, 1) == 1,
                fixrg: Ranged::new(bits(a, 0, 3)),
                osw: Ranged::new(bits(w, 4, 3)),
                fine: Ranged::new(bits(w, 0, 4)),
            };
        }
        v.sync = bits(r[40], 6, 1) == 1;
        v.fbl = Ranged::new(bits(r[40], 3, 3));
        v.alg = Ranged::new(bits(r[40], 0, 3));
        v.lfs = Ranged::new(r[41]);
        v.lfd = Ranged::new(r[42]);
        v.pmd = Ranged::new(r[43]);
        v.amd = Ranged::new(r[44]);
        v.pms = Ranged::new(bits(r[45], 4, 3));
        v.ams = Ranged::new(bits(r[45], 2, 2));
        v.lfw = Ranged::new(bits(r[45], 0, 2));
        v.trps = Ranged::new(r[46]);
        v.pbr = Ranged::new(r[47]);
        v.mode = Ranged::new(r[48]);
        v.port = Ranged::new(r[49]);
        v.fc_vol = Ranged::new(r[50]);
        v.mw_pitch = Ranged::new(r[51]);
        v.mw_ampl = Ranged::new(r[52]);
        v.bc_pitch = Ranged::new(r[53]);
        v.bc_ampl = Ranged::new(r[54]);
        v.bc_pbias = Ranged::new(r[55]);
        v.bc_ebias = Ranged::new(r[56]);
        v.name.copy_from_slice(&r[57..67]);
        v.rev = Ranged::new(r[81]);
        v.fc_pitch = Ranged::new(r[82]);
        v.fc_ampl = Ranged::new(r[83]);
        v
    }
}
