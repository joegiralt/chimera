//! Frozen v1 card files (ADR 0045): every later firmware loads them and
//! renders them identically. The fixtures are written once and never again.

mod common;

use std::path::PathBuf;

use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::FileError;
use common::codec_util::{
    SYSTEM_FIXTURE, decode, decode_into, encode, fix_crc, record_offsets, system_file,
    system_fixture_settings,
};
use common::{fnv1a, render_sound};

/// The v1 corpus: name, and the Sound it was written from.
fn sources() -> Vec<(String, Sound)> {
    let mut v: Vec<_> = (0..FACTORY_LEN)
        .map(|i| (format!("factory_{i}.snd"), factory_sound(i).unwrap()))
        .collect();
    v.push(("init_algo.snd".into(), Sound::init(EngineType::Algo)));
    v.push(("init_modal.snd".into(), Sound::init(EngineType::Modal)));
    v
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v1")
        .join(name)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Creates `path` with `bytes`, refusing an existing file: re-recording a
/// frozen fixture means deleting it on purpose.
fn write_new(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)
}

/// Creates `name` unless it exists; `false` when it was already there.
fn write_fixture(name: &str, bytes: &[u8]) -> bool {
    match write_new(&path(name), bytes) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            println!("kept   {name}");
            false
        }
        Err(e) => panic!("{name}: {e}"),
    }
}

/// Run once with `FIXTURE_WRITE=1 cargo test -p chimera-core --test
/// codec_compat_test -- --ignored --nocapture`, then commit the files and
/// paste the printed rows into the tables. Frozen from there on: a fixture
/// that exists is kept, so a later one is added by the same run.
#[test]
#[ignore]
fn write_v1_fixtures() {
    assert!(
        std::env::var_os("FIXTURE_WRITE").is_some(),
        "FIXTURE_WRITE=1"
    );
    let sys = system_file(&system_fixture_settings());
    if write_fixture(SYSTEM_FIXTURE, &sys) {
        println!("system ({}, {:#018x}),", sys.len(), fnv1a_bytes(&sys));
    }
    for (name, s) in sources() {
        let bytes = encode(&s);
        if !write_fixture(&name, &bytes) {
            continue;
        }
        let d = decode(&bytes).unwrap();
        let h = fnv1a(&render_sound(&d.params, &d.mod_state));
        println!("render (\"{name}\", {h:#018x}),");
        println!(
            "bytes  (\"{name}\", {}, {:#018x}),",
            bytes.len(),
            fnv1a_bytes(&bytes)
        );
    }
}

#[test]
fn write_new_refuses_an_existing_file() {
    let p = std::env::temp_dir().join(format!("chimera_fixture_{}", std::process::id()));
    write_new(&p, b"x").unwrap();
    let e = write_new(&p, b"y").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&p).unwrap(), b"x");
    std::fs::remove_file(&p).unwrap();
}

fn fnv1a_bytes(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &c| {
        (h ^ u64::from(c)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Each fixture's length and FNV-1a over its bytes: a file regenerated in a
/// newer format fails here, whatever it decodes to.
const FIXTURE_BYTES: &[(&str, usize, u64)] = &[
    ("factory_0.snd", 1159, 0xfd931b0991bfb5db),
    ("factory_1.snd", 1159, 0x38576e610ebea813),
    ("factory_2.snd", 1159, 0x57487b1534947f88),
    ("factory_3.snd", 1159, 0x45fba569228a4998),
    ("factory_4.snd", 1159, 0x22cc59f6294be4d3),
    ("factory_5.snd", 1159, 0xc32781ec0bf54c06),
    ("factory_6.snd", 1175, 0xd2f10cb6fe350953),
    ("factory_7.snd", 1159, 0xc0d43d4e79a25648),
    ("init_algo.snd", 1159, 0x44acd895ff6f6af5),
    ("init_modal.snd", 1159, 0xd122f91342eff62a),
];

/// `system.sys`'s length and FNV-1a.
const SYSTEM_FIXTURE_BYTES: (usize, u64) = (65, 0xf544fe6fdcbdda8c);

#[test]
fn v1_fixture_bytes_are_frozen() {
    assert_eq!(FIXTURE_BYTES.len(), sources().len());
    for &(name, len, h) in FIXTURE_BYTES.iter().chain([&(
        SYSTEM_FIXTURE,
        SYSTEM_FIXTURE_BYTES.0,
        SYSTEM_FIXTURE_BYTES.1,
    )]) {
        let f = fixture(name);
        assert_eq!((f.len(), fnv1a_bytes(&f)), (len, h), "{name}");
    }
}

/// FNV-1a of each fixture's render, recorded when the fixtures were written.
const FIXTURE_RENDERS: &[(&str, u64)] = &[
    ("factory_0.snd", 0x878c9ca6ca353aa1),
    ("factory_1.snd", 0x0edce6a988cf7f1b),
    ("factory_2.snd", 0xe0277f16dca55649),
    ("factory_3.snd", 0x10424d2460e991df),
    ("factory_4.snd", 0x05a4301cc61a2332),
    ("factory_5.snd", 0x73b6b9429389dc2a),
    ("factory_6.snd", 0x2cf3c112b967ff6f),
    ("factory_7.snd", 0x7ef436e312c3c9e6),
    ("init_algo.snd", 0xfd37f75c4096594b),
    ("init_modal.snd", 0x90f1197c153d0b05),
];

#[test]
fn v1_fixtures_render_identically() {
    let names: Vec<_> = sources().into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        FIXTURE_RENDERS.iter().map(|r| r.0).collect::<Vec<_>>()
    );
    for &(name, want) in FIXTURE_RENDERS {
        // Moved by Modal 2 step A: re-recorded once, in its Task 11.
        if name == "init_modal.snd" {
            continue;
        }
        let s = decode(&fixture(name)).unwrap();
        assert_eq!(
            fnv1a(&render_sound(&s.params, &s.mod_state)),
            want,
            "{name}"
        );
    }
}

/// Holds only while the factory Sounds are unchanged; the render test is the
/// lasting one.
#[test]
#[ignore = "T4 translates BRIGHT"]
fn v1_fixtures_equal_factory() {
    for i in 0..FACTORY_LEN {
        let d = decode(&fixture(&format!("factory_{i}.snd"))).unwrap();
        assert!(d.bits_eq(&factory_sound(i).unwrap()), "factory_{i}");
    }
}

/// `f` with an unknown record of `len` payload bytes inserted at `at`.
fn with_record(f: &[u8], at: usize, tag: u16, len: usize) -> Vec<u8> {
    let mut f = f.to_vec();
    let mut rec = tag.to_le_bytes().to_vec();
    rec.extend((len as u16).to_le_bytes());
    rec.extend(std::iter::repeat_n(0xA5, len));
    f.splice(at..at, rec);
    fix_crc(&mut f);
    f
}

/// Every record boundary after `Engine` (the first record), and the end.
fn after_engine(f: &[u8]) -> Vec<usize> {
    record_offsets(f)[1..].to_vec()
}

#[test]
#[ignore = "T4 translates BRIGHT"]
fn unknown_non_critical_skipped() {
    for (name, s) in sources() {
        let f = fixture(&name);
        for at in after_engine(&f) {
            let d = decode(&with_record(&f, at, 0x0070, 40)).unwrap();
            assert!(d.bits_eq(&s), "{name} at {at}");
        }
        // Before Engine no record is understood yet.
        let mut t = Sound::neutral(EngineType::Algo);
        let before = record_offsets(&f)[0];
        let f = with_record(&f, before, 0x0070, 40);
        assert_eq!(decode_into(&mut t, &f), Err(FileError::Corrupt), "{name}");
    }
}

#[test]
fn unknown_critical_greys() {
    for (name, _) in sources() {
        let f = fixture(&name);
        for at in record_offsets(&f) {
            let f = with_record(&f, at, 0x8070, 40);
            let mut t = factory_sound(3).unwrap();
            assert_eq!(
                decode_into(&mut t, &f),
                Err(FileError::NeedsNewerFirmware),
                "{name} at {at}"
            );
            assert!(t.bits_eq(&factory_sound(3).unwrap()), "{name} at {at}");
        }
    }
}
#[test]
fn truncated_bad_crc_bad_magic_leave_target() {
    for (name, _) in sources() {
        let good = fixture(&name);
        let mut cases: Vec<(&str, Vec<u8>, FileError)> = Vec::new();
        for cut in [
            0,
            10,
            31,
            32,
            60,
            good.len() / 2,
            good.len() - 4,
            good.len() - 1,
        ] {
            let want = if cut < 32 {
                FileError::Truncated
            } else {
                FileError::BadCrc
            };
            cases.push(("cut", good[..cut].to_vec(), want));
        }
        for at in [
            4,
            6,
            8,
            12,
            28,
            40,
            good.len() / 2,
            good.len() - 5,
            good.len() - 1,
        ] {
            let mut f = good.clone();
            f[at] ^= 0x01;
            cases.push(("flip", f, FileError::BadCrc));
        }
        let mut f = good.clone();
        f[0] ^= 0xFF;
        cases.push(("magic, stale crc", f, FileError::BadCrc));
        for (what, f, want) in cases {
            let mut t = factory_sound(3).unwrap();
            assert_eq!(decode_into(&mut t, &f), Err(want), "{name} {what}");
            assert!(t.bits_eq(&factory_sound(3).unwrap()), "{name} {what}");
        }
        // A bad magic under a valid CRC is the one verdict a bad file gets.
        let mut f = good.clone();
        f[0] ^= 0xFF;
        fix_crc(&mut f);
        let mut t = factory_sound(3).unwrap();
        assert_eq!(decode_into(&mut t, &f), Err(FileError::BadMagic), "{name}");
        assert!(t.bits_eq(&factory_sound(3).unwrap()), "{name}");
    }
}
