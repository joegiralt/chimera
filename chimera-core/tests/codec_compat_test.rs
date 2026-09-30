//! Frozen v1 card files (ADR 0045): every later firmware loads them and
//! renders them identically. The fixtures are written once and never again.

mod common;

use std::path::PathBuf;

use chimera_core::dsp::modal::{BankModes, ResonatorMode, damp_for, damp_from_v1_decay};
use chimera_core::dsp::note_to_freq;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::storage::{FileError, MIGRATIONS, TRANSLATIONS, decode_block};
use chimera_hal::BLOCK_SIZE;
use common::codec_util::{
    SYSTEM_FIXTURE, decode, decode_into, encode, fix_crc, record_offsets, system_file,
    system_fixture_settings,
};
use common::{
    SR, assert_stable, fnv1a, fundamental_hz, octave_clear, play_modal, play_modal_at,
    render_sound, rms,
};

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
    // Re-recorded: Modal 2 step A's resonators (spec § Tests).
    ("init_modal.snd", 0xb9a4be01cac830ff),
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

/// A v1 Modal block in `mode`: every old id, DECAY at `decay`.
fn v1_modal(mode: ResonatorMode, decay: f32) -> Vec<u8> {
    let mut p = vec![1, 0];
    p.extend(u32::from(mode as u8).to_le_bytes());
    let reals = [0.6, decay, 0.9, 0.4, 0.7, 0.5, 0.35, 1.0, 0.1, 0.2, 0.3];
    for (id, v) in (1u8..).zip(reals) {
        p.push(id);
        p.extend(f32::to_le_bytes(v));
    }
    p
}

fn decode_modal(payload: &[u8]) -> ParamSnapshot {
    let mut snap = ParamSnapshot::for_engine(EngineType::Modal);
    decode_block(payload, MIGRATIONS, TRANSLATIONS, Some(&mut snap)).unwrap();
    snap
}

/// Spec § 3: DECAY → DAMP, STIFF or INHARM → STRUCTURE, FDBK dropped; the
/// string models' BRIGHT flips to the new direction. BANK's BURST is its
/// EXCITE; Bowed loads an in-tune bow (#240).
#[test]
fn old_modal_patches_translate() {
    use ResonatorMode::{Bowed, Modal, String, Sympathetic};
    let init = decode(&fixture("init_modal.snd")).unwrap();
    assert!(init.bits_eq(&Sound::init(EngineType::Modal)));
    for mode in [String, Modal, Bowed, Sympathetic] {
        let snap = decode_modal(&v1_modal(mode, 0.2));
        let m = &snap.modal;
        let (damp, structure) = match mode {
            Modal => (0.2, 0.7),
            String | Bowed => (damp_from_v1_decay(0.2), 0.35),
            Sympathetic => (damp_from_v1_decay(0.2), 0.7),
        };
        let bright = if mode == Modal { 0.9 } else { 1.0 - 0.9 };
        // v1 Bowed never read them: an in-tune bow's (#240).
        let (damp, bright, pos) = if mode == Bowed {
            (damp_for(0.5), 0.5, 0.15)
        } else {
            (damp, bright, 0.4)
        };
        assert_eq!((m.damp, m.structure), (damp, structure), "{mode:?}");
        assert_eq!(
            (m.excite, m.bright, m.pos, m.body),
            (0.6, bright, pos, 0.5),
            "{mode:?}"
        );
        // The exciters' hidden values; an old strike keeps its length.
        assert_eq!((m.color, m.force, m.speed), (0.8, 0.5, 0.5), "{mode:?}");
        let burst = if mode == Modal { 0.6 } else { 0.8 };
        assert_eq!(m.burst, burst, "{mode:?}");
        assert_eq!((m.ens_depth, m.ens_rate, m.ens_mix), (0.1, 0.2, 0.3));
        assert_eq!((m.couple, m.halo, m.modes), (0.25, 0.25, BankModes::M32));
    }
}

/// A v1 Bowed patch on the two-delay bow (plan Task 14): re-recorded
/// deliberately, since the one-loop bow played an octave low (#240).
const BOWED_V1_HELD: u64 = 0x4ff7_4f3d_d407_e80c;
const BOWED_V1_RELEASED: u64 = 0x22d5_d84b_a143_3970;
/// The one-loop bow's held C3, second half of its first second, recorded at 330298c.
const BOWED_V1_RMS: f32 = 0.380_809_55;

/// A v1 Bowed patch plays at its note, at about the old bow's level.
#[test]
fn a_v1_bowed_patch_bows_in_tune() {
    let snap = decode_modal(&v1_modal(ResonatorMode::Bowed, 0.2));
    let second = SR as usize / BLOCK_SIZE;
    let held = play_modal_at(&snap.modal, 48, 127, second, 0);
    let f0 = note_to_freq(48);
    let s = &held[SR as usize / 2..];
    let cents = 1200.0 * (fundamental_hz(s, f0 as f64) / f0 as f64).log2();
    // Bowed's gate (spec § 2 BOWED): a bow moves its pitch a few cents.
    assert!(cents.abs() < 5.0, "{cents:+.2} cents");
    assert!(octave_clear(s, f0), "an octave low");
    let db = 20.0 * (rms(s) / BOWED_V1_RMS).log10();
    assert!(db.abs() < 1.0, "{db:+.2} dB from the old bow");
    let released = play_modal_at(&snap.modal, 48, 127, second, second / 2);
    assert_eq!(fnv1a(&held), BOWED_V1_HELD);
    assert_eq!(fnv1a(&released), BOWED_V1_RELEASED);
}

/// Review focus 4: FDBK 1 at DECAY 0, today's longest, once ran away. It
/// now loads as a plain long STRING.
#[test]
fn an_old_fdbk_1_patch_loads_stable() {
    let snap = decode_modal(&v1_modal(ResonatorMode::String, 0.0));
    let out = play_modal(&snap.modal, 36, 30 * SR as usize / BLOCK_SIZE, 0);
    assert_stable(&out, 1.0, 1.0, "STRING, FDBK 1");
}
