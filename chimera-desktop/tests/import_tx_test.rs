//! The TX81Z map and `chimera-import tx` (showcase spec § 6, importer spec
//! § 6, § 8.2). Every voice is built here from field values, never a
//! Yamaha bank; every file goes under `CARGO_TARGET_TMPDIR`.

use std::path::{Path, PathBuf};
use std::process::Command;

use chimera_core::addr::{BlockRead, BlockRef};
use chimera_core::dsp::algo::tx;
use chimera_core::import::tx81z::{Op, Ranged, VOICES, Voice, bank_bytes};
use chimera_core::preset::Sound;
use chimera_core::storage::{BuildStamp, ValidAddr};
use chimera_desktop::factory_sounds::build::EditsCheck;
use chimera_desktop::factory_sounds::{build, source};
use chimera_desktop::import::tx81z::{self as map, BankEdits, LossKind};

const STAMP: BuildStamp = BuildStamp::new("0.1.0", "1a2b3c4d", false);
const FROM: &str = "tx81z 0badf00d #5";
/// A made-up voice name: no output may carry it.
const NAME: &[u8; 10] = b"QZXJ WVKYP";

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A voice with every lossy field neutral: plain ratios, no LFO, no
/// controllers.
fn plain() -> Voice {
    let mut v = Voice::default();
    for (i, op) in v.ops.iter_mut().enumerate() {
        let n = i as u8;
        *op = Op {
            ar: Ranged::new(31 - n),
            d1r: Ranged::new(9 + n),
            d2r: Ranged::new(2 + n),
            rr: Ranged::new(6 + n),
            d1l: Ranged::new(13 - n),
            kvs: Ranged::new(1 + n),
            out: Ranged::new(99 - 7 * n),
            crs: Ranged::new(4 + 6 * n),
            rs: Ranged::new(n),
            det: Ranged::new(1 + n),
            osw: Ranged::new(2 * n + 1),
            ..Op::default()
        };
    }
    v.alg = Ranged::new(4);
    v.fbl = Ranged::new(5);
    v.trps = Ranged::new(24);
    v.bc_pbias = Ranged::new(50);
    v.name = *NAME;
    v
}

fn sound(v: &Voice) -> (Sound, Vec<map::Loss>) {
    let (s, losses) = map::to_source(v, FROM);
    (source::patch(&s).unwrap(), losses)
}

/// A stored param's value as its Source line writes it.
fn val(snd: &Sound, block: &str, param: &str) -> String {
    let b = BlockRef::ALL
        .into_iter()
        .find(|b| b.disk_ident() == Some(block))
        .unwrap();
    let a = ValidAddr::of_block(b)
        .find(|a| a.spec().ident == param)
        .unwrap();
    let blk = snd.params.block(b).unwrap();
    if a.coded() {
        blk.enum_ident(a.spec().id).unwrap().to_string()
    } else {
        format!("{}", blk.get(a.spec().id))
    }
}

fn routes(snd: &Sound, src: &str) -> String {
    let key = format!("mod.{src} = ");
    source::lines(snd)
        .into_iter()
        .find_map(|l| {
            l.strip_prefix(&key)
                .map(|r| r.trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

fn kinds(losses: &[map::Loss]) -> Vec<LossKind> {
    losses.iter().map(|l| l.kind).collect()
}

const OPS: [&str; 4] = ["OP_A", "OP_B", "OP_C", "OP_D"];

#[test]
fn alg_n_is_t_n_and_ops_5_6_are_silent() {
    for alg in 0..8u8 {
        let mut v = plain();
        v.alg = Ranged::new(alg);
        let (snd, _) = sound(&v);
        let t = format!("T{}", alg + 1);
        assert_eq!(val(&snd, "ALGO", "ALG_A"), t);
        assert_eq!(val(&snd, "ALGO", "ALG_B"), t, "ALG B = ALG A");
        assert_eq!(val(&snd, "ALGO", "MORPH"), "0");
        assert_eq!(val(&snd, "OP_E", "LEVEL"), "0");
        assert_eq!(val(&snd, "OP_F", "LEVEL"), "0");
    }
}

/// Importer spec § 8.2: every E row maps to a field equal to its input.
#[test]
fn exact_rows_round_trip() {
    let v = plain();
    let (snd, losses) = sound(&v);
    assert!(losses.is_empty(), "a plain voice loses nothing: {losses:?}");
    for (i, op) in v.ops.iter().enumerate() {
        let o = OPS[i];
        let eq = |p: &str, want: i32| assert_eq!(val(&snd, o, p), want.to_string(), "{o}.{p}");
        assert_eq!(val(&snd, o, "WAVE"), format!("W{}", op.osw.get() + 1));
        eq("CRSE", op.crs.get().into());
        eq("FINE", op.fine.get().into());
        eq("DETUN", i32::from(op.det.get()) - 3);
        eq("LEVEL", op.out.get().into());
        eq("AR", op.ar.get().into());
        eq("D1R", op.d1r.get().into());
        eq("D1L", op.d1l.get().into());
        eq("D2R", op.d2r.get().into());
        eq("RR", op.rr.get().into());
        eq("RS", op.rs.get().into());
        eq("VEL", op.kvs.get().into());
        eq("FDBK", if i == 3 { v.fbl.get().into() } else { 0 });
    }
    assert_eq!(val(&snd, "ALGO", "TRNSP"), "0");
    let mut t = v;
    t.trps = Ranged::new(36);
    assert_eq!(val(&sound(&t).0, "ALGO", "TRNSP"), "12");
    t.trps = Ranged::new(0);
    assert_eq!(val(&sound(&t).0, "ALGO", "TRNSP"), "-24");
    // CRSE and FINE index `tx.rs`'s tables: the TX81Z's ratio.
    let op = v.ops[1];
    let r = tx::ratio(op.crs.get(), op.fine.get());
    assert_eq!(r, tx::COARSE[usize::from(op.crs.get())]);
}

#[test]
fn filter_open_and_lfo_2_3_env_1_3_free() {
    let (snd, _) = sound(&plain());
    let base = Sound::neutral(chimera_core::params::EngineType::Algo);
    let cutoff = ValidAddr::of_block(BlockRef::Filter)
        .find(|a| a.spec().ident == "CUTOFF")
        .unwrap()
        .spec()
        .max;
    assert_eq!(val(&snd, "FILTER", "CUTOFF"), format!("{cutoff}"));
    assert_eq!(val(&snd, "FOLDER", "FOLD"), "0");
    for b in ["ENV1", "ENV3", "LFO2", "LFO3"] {
        for p in ["RATE", "ATK", "DEC", "SHAPE", "SYNC"] {
            let ok = BlockRef::ALL
                .into_iter()
                .find(|x| x.disk_ident() == Some(b))
                .and_then(|x| ValidAddr::of_block(x).find(|a| a.spec().ident == p));
            if ok.is_some() {
                assert_eq!(val(&snd, b, p), val(&base, b, p), "{b}.{p} left free");
            }
        }
        assert_eq!(routes(&snd, b), "", "{b} routes nothing");
    }
}

#[test]
fn kvs_fbl_osw_map_one_to_one() {
    for n in 0..8u8 {
        let mut v = plain();
        v.fbl = Ranged::new(n);
        v.ops[2].kvs = Ranged::new(n);
        v.ops[0].osw = Ranged::new(n);
        let (snd, _) = sound(&v);
        assert_eq!(val(&snd, "OP_D", "FDBK"), n.to_string(), "FBL on op 4");
        assert_eq!(val(&snd, "OP_C", "VEL"), n.to_string());
        assert_eq!(val(&snd, "OP_A", "WAVE"), format!("W{}", n + 1));
    }
}

#[test]
fn lfo_maps_to_lfo_1_and_its_routes() {
    let mut v = plain();
    let (snd, _) = sound(&v);
    assert_eq!(routes(&snd, "LFO1"), "", "no depth, no route");
    v.lfw = Ranged::new(0);
    v.sync = true;
    v.lfs = Ranged::new(50);
    v.pmd = Ranged::new(50);
    v.pms = Ranged::new(3);
    v.amd = Ranged::new(99);
    v.ams = Ranged::new(2);
    v.ops[1].ame = true;
    let (snd, losses) = sound(&v);
    assert_eq!(
        val(&snd, "LFO1", "SHAPE"),
        "SAW",
        "SAW UP is our rising SAW"
    );
    assert_eq!(val(&snd, "LFO1", "SYNC"), "RETRIG");
    let hz: f32 = val(&snd, "LFO1", "RATE").parse().unwrap();
    assert!((0.5..10.0).contains(&hz), "LFS 50 at {hz} Hz");
    let r = routes(&snd, "LFO1");
    assert!(r.contains("PITCH.FINE +"), "pitch mod: {r}");
    assert!(r.contains("OP_B.LEVEL +"), "amp mod on the AME op: {r}");
    assert!(!r.contains("OP_A.LEVEL"), "not on an op without AME: {r}");
    // Bipolar AM: the base LEVEL drops by half the depth.
    let base: f32 = val(&snd, "OP_B", "LEVEL").parse().unwrap();
    assert!(base < f32::from(v.ops[1].out.get()));
    assert_eq!(kinds(&losses), [LossKind::AmpMod]);
    for (w, shape) in [(1, "SQUARE"), (2, "TRIANGLE"), (3, "RANDOM")] {
        v.lfw = Ranged::new(w);
        assert_eq!(val(&sound(&v).0, "LFO1", "SHAPE"), shape);
    }
    v.sync = false;
    assert_eq!(val(&sound(&v).0, "LFO1", "SYNC"), "FREE");
    // Deep pitch mod moves to PITCH.
    v.pmd = Ranged::new(99);
    v.pms = Ranged::new(7);
    let r = routes(&sound(&v).0, "LFO1");
    assert!(r.contains("PITCH.PITCH +"), "over a semitone: {r}");
}

#[test]
fn fixed_frequency_is_the_nearest_ratio_at_its_key() {
    let mut v = plain();
    v.alg = Ranged::new(0); // T1: op 1 the one carrier
    // FIXRG 1, CRS 32 (8 × 16 Hz), FINE 3: 2 × 131 = 262 Hz, about C4.
    v.ops[0].fix = true;
    v.ops[0].fixrg = Ranged::new(1);
    v.ops[0].crs = Ranged::new(32);
    v.ops[0].fine = Ranged::new(3);
    let (snd, losses) = sound(&v);
    assert_eq!(val(&snd, "OP_A", "CRSE"), "4", "ratio 1.00");
    assert_eq!(val(&snd, "OP_A", "FINE"), "0");
    assert_eq!(kinds(&losses), [LossKind::FixedFreq]);
    assert_eq!(losses[0].op, Some(1));
    assert!(losses[0].detail.contains("key 60"), "{}", losses[0].detail);
    // A 3 Hz-range modulator clamps to 0.5 and says so.
    v.ops[1].fix = true;
    v.ops[1].fixrg = Ranged::new(0);
    v.ops[1].crs = Ranged::new(0);
    v.ops[1].fine = Ranged::new(0);
    let (snd, losses) = sound(&v);
    assert_eq!(val(&snd, "OP_B", "CRSE"), "0", "ratio 0.50");
    assert_eq!(
        kinds(&losses),
        [
            LossKind::FixedFreq,
            LossKind::FixedFreq,
            LossKind::FixedSubaudio
        ]
    );
}

/// Each unmappable item logs one Loss, and nothing when it is neutral.
#[test]
fn each_unmappable_item_logs_one_loss() {
    type Set = fn(&mut Voice);
    let cases: [(Set, LossKind); 10] = [
        (|v| v.ops[2].egsft = Ranged::new(2), LossKind::EgShift),
        (|v| v.rev = Ranged::new(3), LossKind::RevRate),
        (
            |v| {
                v.lfd = Ranged::new(20);
                v.pmd = Ranged::new(10);
                v.pms = Ranged::new(1);
            },
            LossKind::LfoDelay,
        ),
        (
            |v| {
                v.lfs = Ranged::new(99);
                v.pmd = Ranged::new(10);
                v.pms = Ranged::new(1);
            },
            LossKind::LfoFast,
        ),
        (|v| v.ops[3].ls = Ranged::new(40), LossKind::KeyLevel),
        (
            |v| {
                v.ops[0].ame = true;
                v.amd = Ranged::new(50);
                v.ams = Ranged::new(1);
            },
            LossKind::AmpMod,
        ),
        (|v| v.ops[1].fine = Ranged::new(5), LossKind::Fine),
        (|v| v.mw_pitch = Ranged::new(50), LossKind::Controller),
        (|v| v.pbr = Ranged::new(2), LossKind::Controller),
        (|v| v.mode = Ranged::new(0x08), LossKind::Controller),
    ];
    assert!(sound(&plain()).1.is_empty());
    for (set, kind) in cases {
        let mut v = plain();
        set(&mut v);
        let losses = sound(&v).1;
        assert_eq!(kinds(&losses), [kind], "{losses:?}");
        assert!(!kind.tag().is_empty());
    }
    // Several controllers are one line, naming each.
    let mut v = plain();
    v.mw_pitch = Ranged::new(1);
    v.bc_ampl = Ranged::new(2);
    v.port = Ranged::new(3);
    v.ops[2].ebs = Ranged::new(4);
    let losses = sound(&v).1;
    assert_eq!(kinds(&losses), [LossKind::Controller]);
    for f in ["MW PITCH", "BC AMPL", "PORT", "EBS"] {
        assert!(losses[0].detail.contains(f), "{}", losses[0].detail);
    }
    // Key level becomes a NOTE route, down the keyboard's top.
    let mut v = plain();
    v.ops[3].ls = Ranged::new(99);
    let r = routes(&sound(&v).0, "NOTE");
    assert_eq!(r, "OP_D.LEVEL -127");
}

#[test]
fn source_has_no_voice_name() {
    let mut v = plain();
    v.ops[1].fine = Ranged::new(5);
    v.rev = Ranged::new(1);
    let (text, losses) = map::to_text(&v, FROM);
    let name = std::str::from_utf8(NAME).unwrap();
    let report = map::report_line(5, &losses, Default::default());
    for out in [&text, &report] {
        assert!(!out.contains(name), "{out}");
        for w in name.split_whitespace() {
            assert!(!out.contains(w), "{w} in {out}");
        }
    }
    // The Source parses as a port: from, never edits (a raw port is
    // never built: the mutation adds them).
    let all = source::parse(&text, "5.toml").unwrap();
    let s = &all[0];
    assert_eq!(
        s.from.map(|f| (f.1.bank, f.1.voice)),
        Some((0x0bad_f00d, 5))
    );
    assert!(s.edits.is_none());
    assert!(source::ports(&all).is_err(), "unmutated: refused");
    assert_eq!(s.name.as_str(), "PORT 0BADF00D 05");
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn out_in_work_tree_refused() {
    let repo = scratch("tx-guard-refused");
    git(&repo, &["init", "-q"]);
    let want = |p: &Path| {
        format!(
            "{}: {}; ports stay out of the repo",
            p.display(),
            map::NOT_IGNORED
        )
    };
    let p = repo.join("tools/ports");
    assert_eq!(map::out_allowed(&p).unwrap_err(), want(&p));
    std::fs::create_dir_all(repo.join("tools")).unwrap();
    assert_eq!(map::out_allowed(&p).unwrap_err(), want(&p));
    assert_eq!(map::out_allowed(&repo).unwrap_err(), want(&repo));
}

#[test]
fn out_ignored_by_git_allowed() {
    let repo = scratch("tx-guard-ignored");
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join(".gitignore"), ".private/\n").unwrap();
    map::out_allowed(&repo.join(".private/ports")).unwrap();
    std::fs::create_dir_all(repo.join(".private/ports")).unwrap();
    map::out_allowed(&repo.join(".private/ports")).unwrap();
    // Outside any work tree, allowed.
    let outside = std::env::temp_dir().join("chimera-tx-guard-none");
    if Command::new("git")
        .args(["-C", outside.parent().unwrap().to_str().unwrap()])
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| !o.status.success())
    {
        map::out_allowed(&outside).unwrap();
    }
}

#[test]
fn voices_arg_reads_lists_and_ranges() {
    assert_eq!(map::voices_arg("1,4,9-12").unwrap(), [1, 4, 9, 10, 11, 12]);
    assert_eq!(map::voices_arg("32").unwrap(), [32]);
    for bad in ["0", "33", "4-2", "a", "", "1,,2", "9-40"] {
        assert!(map::voices_arg(bad).is_err(), "{bad}");
    }
}

/// A synthetic bank file in `dir` and its `from` hash.
fn bank_file(dir: &Path, voices: &[Voice; VOICES]) -> (PathBuf, String) {
    let bytes = bank_bytes(voices);
    let p = dir.join("bank.syx");
    std::fs::write(&p, bytes).unwrap();
    (p, map::bank_id(&bytes))
}

fn bank() -> [Voice; VOICES] {
    let mut voices = [plain(); VOICES];
    voices[3].rev = Ranged::new(2);
    voices[3].ops[0].fine = Ranged::new(4);
    voices
}

#[test]
fn import_tx_writes_voices_and_report() {
    let d = scratch("tx-cli");
    let (bank, id) = bank_file(&d, &bank());
    let out = d.join("ports");
    let o = Command::new(env!("CARGO_BIN_EXE_chimera-import"))
        .args(["tx", bank.to_str().unwrap(), "--voices", "1,4,9-12"])
        .args(["--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let mut files: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "1.toml",
            "10.toml",
            "11.toml",
            "12.toml",
            "4.toml",
            "9.toml",
            "import-report.txt"
        ]
    );
    let name = std::str::from_utf8(NAME).unwrap();
    let report = std::fs::read_to_string(out.join("import-report.txt")).unwrap();
    for f in &files {
        let text = std::fs::read_to_string(out.join(f)).unwrap();
        assert!(!text.contains(name) && !text.contains("QZXJ"), "{f}");
        if let Some(n) = f.strip_suffix(".toml") {
            let s = source::parse(&text, f).unwrap();
            let from = s[0].from.unwrap().1;
            assert_eq!(from.voice.to_string(), n);
            assert_eq!(format!("{:08x}", from.bank), id);
        }
    }
    assert!(
        report.contains("#04") && report.contains("rev-rate"),
        "{report}"
    );
    assert!(report.contains("fine"), "{report}");
    assert!(!stdout.contains("QZXJ"));
    // A bank whose checksum is off still imports, and the report says so.
    let mut bytes = std::fs::read(&bank).unwrap();
    bytes[4102] ^= 1;
    std::fs::write(&bank, &bytes).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_chimera-import"))
        .args(["tx", bank.to_str().unwrap(), "--voices", "2"])
        .args(["--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success());
    let report = std::fs::read_to_string(out.join("import-report.txt")).unwrap();
    assert!(report.contains("checksum"), "{report}");
    // Not a bank: refused, exit 1.
    std::fs::write(&bank, b"not a bank").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_chimera-import"))
        .args(["tx", bank.to_str().unwrap(), "--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
}

/// A port of `voice` from `bank`, edited by `set` with its `edits` line.
fn port(voice: &Voice, from: &str, edits: &str, set: &[(&str, &str)]) -> String {
    let mut text = map::to_text(voice, from).0;
    for (k, v) in set {
        let line = format!("{k} = ");
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.starts_with(&line))
            .map(String::from)
            .collect();
        lines.push(format!("{k} = \"{v}\""));
        text = lines.join("\n") + "\n";
    }
    text + &format!("edits = \"{edits}\"\n")
}

#[test]
fn edits_verified() {
    let d = scratch("tx-edits");
    let banks = d.join("banks");
    std::fs::create_dir_all(&banks).unwrap();
    let voices = bank();
    let (_, id) = bank_file(&banks, &voices);
    let from = format!("tx81z {id} #4");
    let check = BankEdits::new(banks.clone());
    let public = d.join("public-sources");
    std::fs::create_dir_all(&public).unwrap();
    let ports = d.join("ports");
    std::fs::create_dir_all(&ports).unwrap();
    let parse = |t: &str| source::parse(t, "4.toml").unwrap().remove(0);
    let v = &voices[3];

    // Each category passes on a change it owns, and fails on none.
    let cases: [(&str, (&str, &str)); 6] = [
        ("filter: 2040 low pass", ("FILTER.CUTOFF", "1200")),
        ("fold: a little", ("FOLDER.FOLD", "0.3")),
        ("LFO: a slow wobble", ("LFO2.RATE", "0.3")),
        ("envelope: longer release", ("OP_A.RR", "3")),
        ("effects: a reverb send", ("OUT.SEND3", "0.2")),
        ("formant: op 5 vowel", ("OP_E.LEVEL", "40")),
    ];
    for (edits, change) in cases {
        check
            .verify(&parse(&port(v, &from, edits, &[change])))
            .unwrap();
        let e = check
            .verify(&parse(&port(v, &from, edits, &[])))
            .unwrap_err();
        let cat = edits.split(':').next().unwrap().to_ascii_lowercase();
        assert!(e.contains("4.toml:") && e.contains(&cat), "{e}");
    }
    // A change outside the named category does not count.
    let other = port(v, &from, "filter: darker", &[("OP_E.LEVEL", "40")]);
    check.verify(&parse(&other)).unwrap_err();
    // Two categories: each must change.
    let two = port(
        v,
        &from,
        "filter: dark; lfo: slow",
        &[("FILTER.CUTOFF", "900")],
    );
    assert!(check.verify(&parse(&two)).unwrap_err().contains("lfo"));
    // A route counts in its destination's category, and its source's.
    let route = port(
        v,
        &from,
        "filter: env; envelope: on the filter",
        &[("mod.ENV2", "FILTER.CUTOFF +40")],
    );
    check.verify(&parse(&route)).unwrap();
    // No bank with that hash: named, with the variable.
    let lost = port(
        v,
        "tx81z 12345678 #4",
        "filter: x",
        &[("FILTER.CUTOFF", "900")],
    );
    let e = check.verify(&parse(&lost)).unwrap_err();
    assert!(
        e.contains("CHIMERA_TX_BANKS") && e.contains("12345678"),
        "{e}"
    );

    // Through the private build, then the real binary with the variables
    // set on the child: a port that changes nothing fails before anything
    // is written.
    std::fs::write(public.join("va.toml"), "").unwrap();
    std::fs::write(ports.join("4.toml"), port(v, &from, "filter: open", &[])).unwrap();
    let root = d.join("out");
    let e = build::private(&public, &ports, &root, &STAMP, Some(&check)).unwrap_err();
    assert!(e.contains("filter"), "{e}");
    assert!(!root.join("private").exists());
    let o = Command::new(env!("CARGO_BIN_EXE_factory-sounds"))
        .arg("--private")
        .env(build::PORTS_VAR, &ports)
        .env(build::BANKS_VAR, &banks)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains("filter"), "{err}");
    // fmt and qa verify first too, and write nothing.
    let before = std::fs::read_to_string(ports.join("4.toml")).unwrap();
    for args in [&["fmt", "--private"][..], &["qa", "--private"]] {
        let o = Command::new(env!("CARGO_BIN_EXE_factory-sounds"))
            .args(args)
            .env(build::PORTS_VAR, &ports)
            .env(build::BANKS_VAR, &banks)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(
            !o.status.success() && err.contains("edits names filter"),
            "{args:?}: {err}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(ports.join("4.toml")).unwrap(),
        before
    );
    // Changed, it builds.
    std::fs::write(
        ports.join("4.toml"),
        port(v, &from, "filter: darker", &[("FILTER.CUTOFF", "900")]),
    )
    .unwrap();
    let man = build::private(&public, &ports, &root, &STAMP, Some(&check)).unwrap();
    assert_eq!(man.sounds.len(), 1);
}

#[test]
fn out_guard_refuses_a_dotdot_bypass() {
    let repo = scratch("tx-guard-dotdot");
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join(".gitignore"), ".private/\n").unwrap();
    std::fs::create_dir_all(repo.join(".private/a")).unwrap();
    std::fs::create_dir_all(repo.join("tools")).unwrap();
    // Lexically under the ignored `.private/`, really `tools/ports`.
    let e = map::out_allowed(&repo.join(".private/a/../../tools/ports")).unwrap_err();
    assert!(e.contains(map::NOT_IGNORED), "{e}");
    // `..` past what exists can't be resolved: refused.
    let e = map::out_allowed(&repo.join(".private/new/../../tools")).unwrap_err();
    assert!(e.contains(".."), "{e}");
    map::out_allowed(&repo.join(".private/a/../b")).unwrap();
}

#[test]
fn out_guard_fails_closed_on_a_git_error() {
    let repo = scratch("tx-guard-broken");
    // A `.git` git can't read: inside a repo, so refused, not allowed.
    std::fs::write(repo.join(".git"), "gitdir: /nonexistent/chimera-tx-guard\n").unwrap();
    let e = map::out_allowed(&repo.join("ports")).unwrap_err();
    assert!(e.contains("git"), "{e}");
}

#[test]
fn an_existing_port_is_kept_without_force() {
    let d = scratch("tx-force");
    let (bank, _) = bank_file(&d, &bank());
    let out = d.join("ports");
    let run = |force: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_chimera-import"));
        c.args(["tx", bank.to_str().unwrap(), "--voices", "1,2"])
            .args(["--out", out.to_str().unwrap()]);
        if force {
            c.arg("--force");
        }
        c.output().unwrap()
    };
    let o = run(false);
    assert!(o.status.success());
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(
        stdout.contains("edits") && stdout.contains("renumber"),
        "{stdout}"
    );
    std::fs::write(out.join("2.toml"), "mutated").unwrap();
    let o = run(false);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(1));
    assert!(err.contains("2.toml") && err.contains("--force"), "{err}");
    assert_eq!(
        std::fs::read_to_string(out.join("2.toml")).unwrap(),
        "mutated"
    );
    assert!(run(true).status.success());
    assert_ne!(
        std::fs::read_to_string(out.join("2.toml")).unwrap(),
        "mutated"
    );
}

/// A quiet operator with deep key scaling and deep AM: the base clamps
/// at 0, so both routes shrink; no key and no LFO phase rises above OUT.
#[test]
fn low_out_never_rises_above_the_source() {
    let route = |r: &str, dest: &str| -> f32 {
        r.split(", ")
            .find_map(|x| x.strip_prefix(dest))
            .map_or(0.0, |a| a.trim().parse().unwrap())
    };
    for out in [0u8, 5, 20, 40, 64, 99] {
        let mut v = plain();
        v.ops[0].out = Ranged::new(out);
        v.ops[0].ls = Ranged::new(99);
        v.ops[0].ame = true;
        v.amd = Ranged::new(99);
        v.ams = Ranged::new(3);
        let (snd, losses) = sound(&v);
        let base: f32 = val(&snd, "OP_A", "LEVEL").parse().unwrap();
        let am = route(&routes(&snd, "LFO1"), "OP_A.LEVEL");
        let ls = route(&routes(&snd, "NOTE"), "OP_A.LEVEL");
        assert!(base >= 0.0 && am >= 0.0 && ls <= 0.0);
        for n in 24..=96 {
            for lfo in [-1.0f32, 0.0, 1.0] {
                let at =
                    base + am / 127.0 * lfo * 99.0 + ls / 127.0 * (n as f32 - 60.0) / 120.0 * 99.0;
                assert!(
                    at <= f32::from(out) + 1e-3,
                    "OUT {out} note {n} lfo {lfo}: {at}"
                );
            }
        }
        let k = kinds(&losses);
        assert_eq!(k, [LossKind::AmpMod, LossKind::KeyLevel], "OUT {out}");
        assert!(
            losses[0].detail.contains("under OUT"),
            "{}",
            losses[0].detail
        );
        assert!(losses[1].detail.contains("C7") && losses[1].detail.contains("under OUT"));
        if out < 64 {
            assert!(
                losses.iter().all(|l| l.detail.contains("depth cut to")),
                "OUT {out}: {losses:?}"
            );
        }
    }
    let h = map::headroom(20.0, 64.0, 127.0);
    assert_eq!(h.base, 0.0);
    assert!(h.am + 0.3 * h.ls <= 20.0 / 99.0 * 127.0);
}

/// TX81Z LS only attenuates: the NOTE route never lifts LEVEL above OUT
/// over C1-C7 (importer spec § 6.5).
#[test]
fn key_level_never_boosts() {
    for ls in [1u8, 40, 99] {
        let mut v = plain();
        v.ops[1].ls = Ranged::new(ls);
        let (snd, losses) = sound(&v);
        let base: f32 = val(&snd, "OP_B", "LEVEL").parse().unwrap();
        let r = routes(&snd, "NOTE");
        let amount: f32 = r.rsplit_once(' ').unwrap().1.parse().unwrap();
        assert!(amount < 0.0, "{r}");
        for n in 24..=96 {
            let off = amount / 127.0 * (n as f32 - 60.0) / 120.0 * 99.0;
            assert!(
                base + off <= f32::from(v.ops[1].out.get()) + 1e-3,
                "LS {ls} at note {n}: {base} + {off}"
            );
        }
        assert_eq!(kinds(&losses), [LossKind::KeyLevel]);
        assert!(
            losses[0].detail.contains("under OUT"),
            "{}",
            losses[0].detail
        );
        assert!(losses[0].detail.contains("dB"), "{}", losses[0].detail);
    }
}

#[test]
fn a_fixed_ratio_past_nyquist_is_capped_and_logged() {
    let mut v = plain();
    v.alg = Ranged::new(0);
    // A fixed modulator at FIXRG 7: (16 × 15) × 128 = 30 720 Hz, key 60.
    v.ops[1].fix = true;
    v.ops[1].fixrg = Ranged::new(7);
    v.ops[1].crs = Ranged::new(60);
    let (snd, losses) = sound(&v);
    assert_eq!(kinds(&losses), [LossKind::FixedFreq, LossKind::Nyquist]);
    let crse: u8 = val(&snd, "OP_B", "CRSE").parse().unwrap();
    let fine: u8 = val(&snd, "OP_B", "FINE").parse().unwrap();
    let top = 24_000.0 / (440.0 * 2f32.powf((108.0 - 69.0) / 12.0));
    let r = tx::ratio(crse, fine);
    assert!(r <= top, "{r} under {top}");
    // The largest grid ratio at or under the cap.
    let best = (0..64u8)
        .flat_map(|c| (0..16u8).map(move |f| tx::ratio(c, f)))
        .filter(|&x| x <= top)
        .fold(0.0f32, f32::max);
    assert_eq!(r, best);
}

#[test]
fn an_all_fixed_voice_says_so() {
    let mut v = plain();
    for op in &mut v.ops {
        op.fix = true;
        op.fixrg = Ranged::new(1);
        op.crs = Ranged::new(32);
    }
    let losses = sound(&v).1;
    let all: Vec<_> = losses
        .iter()
        .filter(|l| l.kind == LossKind::AllFixed)
        .collect();
    assert_eq!(all.len(), 1, "{losses:?}");
    assert!(all[0].detail.contains("key"), "{}", all[0].detail);
    v.ops[3].fix = false;
    assert!(!sound(&v).1.iter().any(|l| l.kind == LossKind::AllFixed));
}

#[test]
fn rate_and_level_tables_are_monotonic() {
    let hz: Vec<f32> = (0..100u8).map(map::lfo_hz).collect();
    assert!(hz.windows(2).all(|w| w[0] < w[1]), "LFO speed");
    assert!(map::PMS_CENTS.windows(2).all(|w| w[0] < w[1]));
    assert!(map::AMS_DB.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(
        map::PMS_CENTS,
        [0.0, 5.0, 10.0, 20.0, 50.0, 100.0, 400.0, 700.0]
    );
    assert_eq!(map::AMS_DB, [0.0, 23.9, 47.8, 95.6]);
    assert!(tx::LEVEL_GAIN.windows(2).all(|w| w[0] < w[1]));
    assert!(tx::COARSE.windows(2).all(|w| w[0] < w[1]));
    for c in 0..64u8 {
        let r: Vec<f32> = (0..16u8).map(|f| tx::ratio(c, f)).collect();
        assert!(r.windows(2).all(|w| w[0] <= w[1]), "CRSE {c}");
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig {
        failure_persistence: None,
        cases: 64,
        ..Default::default()
    })]

    /// Any 128 bytes read as a voice map to a Source that parses.
    #[test]
    fn any_voice_maps_to_a_source(raw in proptest::collection::vec(proptest::prelude::any::<u8>(), 128)) {
        let v = Voice::unpack(&raw.try_into().unwrap());
        let (s, _) = map::to_source(&v, FROM);
        source::patch(&s).unwrap();
    }
}
