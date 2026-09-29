//! Frozen v1 card files (ADR 0045): every later firmware loads them and
//! renders them identically. The fixtures are written once and never again.

mod common;

use std::path::PathBuf;

use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::FileError;
use common::codec_util::{decode, decode_into, encode, fix_crc};
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

/// Run once with `FIXTURE_WRITE=1 cargo test -p chimera-core --test
/// codec_compat_test -- --ignored`, then commit the files. Frozen from
/// there on: a later change never rewrites them.
#[test]
#[ignore]
fn write_v1_fixtures() {
    assert!(
        std::env::var_os("FIXTURE_WRITE").is_some(),
        "FIXTURE_WRITE=1"
    );
    for (name, s) in sources() {
        std::fs::write(path(&name), encode(&s)).unwrap();
        let d = decode(&encode(&s)).unwrap();
        let h = fnv1a(&render_sound(&d.params, &d.mod_state));
        println!("    (\"{name}\", {h:#018x}),");
    }
}

/// FNV-1a of each fixture's render, recorded when the fixtures were written.
const FIXTURE_RENDERS: &[(&str, u64)] = &[
    ("factory_0.snd", 0xc0e212b1b98a9bcb),
    ("factory_1.snd", 0xd11d69ebe6b0e4f7),
    ("factory_2.snd", 0x5d86bbdbd84012d3),
    ("factory_3.snd", 0xe34e4665f140410c),
    ("factory_4.snd", 0x44a3e31517fc8b68),
    ("factory_5.snd", 0xdadaa3f8d1eecb4a),
    ("factory_6.snd", 0x29b3e58dfd18575b),
    ("factory_7.snd", 0x3dc833ebd9700912),
    ("init_algo.snd", 0xf36afbe129df33fa),
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
fn v1_fixtures_equal_factory() {
    for i in 0..FACTORY_LEN {
        let d = decode(&fixture(&format!("factory_{i}.snd"))).unwrap();
        assert!(d.bits_eq(&factory_sound(i).unwrap()), "factory_{i}");
    }
}

/// The Engine record ends at 28 + 4 + 1.
const AFTER_ENGINE: usize = 33;

fn with_record(mut f: Vec<u8>, tag: u16, len: usize) -> Vec<u8> {
    let mut rec = tag.to_le_bytes().to_vec();
    rec.extend((len as u16).to_le_bytes());
    rec.extend(std::iter::repeat_n(0xA5, len));
    f.splice(AFTER_ENGINE..AFTER_ENGINE, rec);
    fix_crc(&mut f);
    f
}

#[test]
fn unknown_non_critical_skipped() {
    for (name, s) in sources() {
        let d = decode(&with_record(fixture(&name), 0x0070, 40)).unwrap();
        assert!(d.bits_eq(&s), "{name}");
    }
}

#[test]
fn unknown_critical_greys() {
    let f = with_record(fixture("factory_0.snd"), 0x8070, 40);
    let mut t = factory_sound(3).unwrap();
    assert_eq!(decode_into(&mut t, &f), Err(FileError::NeedsNewerFirmware));
    assert!(t.bits_eq(&factory_sound(3).unwrap()));
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
