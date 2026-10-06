//! `chimera-import tape` (TAPE spec § 6): a WAV to a sealed `.SRC` on the
//! sim's card, and the report that says what it made.

use chimera_core::dsp::tape_engine::heads::Bounds;
use chimera_core::dsp::tape_engine::read::{Descriptor, Shape, SourceView};
use chimera_core::name::SoundName;
use chimera_core::project::Project;
use chimera_core::project::arena::Cost;
use chimera_core::storage::{Card, SourceCheck, SourceHead, Wrote, read_source_into};
use chimera_desktop::import::tape::convert::Channel;
use chimera_desktop::import::tape::{
    Cycle, Form, ImportError, Rate, Report, Root, SEND_LINE, TapeOpts, Top, Tune, free_beside_new,
    import,
};
use chimera_desktop::store::DirStore;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory: a card root, or a WAV's folder.
fn fresh(what: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("chimera-import-{what}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A full-scale saw, `period` samples a cycle: loud to its last sample,
/// so the trim keeps it whole.
fn saw(period: f64, n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (2.0 * ((i as f64 + 0.5) / period).fract() - 1.0) as f32 * 0.9)
        .collect()
}

/// `x` as a mono 16-bit WAV at `rate`, then a `smpl` chunk (unity C4
/// +25 cents, loop `smpl`) if given, in a fresh folder.
fn wav_file(x: &[f32], rate: u32, smpl: Option<(u32, u32)>) -> PathBuf {
    let path = fresh("wav").join("Test take_1.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut c = std::io::Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut c, spec).unwrap();
    x.iter()
        .for_each(|s| w.write_sample((s * 32_767.0) as i16).unwrap());
    w.finalize().unwrap();
    let mut b = c.into_inner();
    if let Some(lp) = smpl {
        b.extend_from_slice(b"smpl");
        b.extend_from_slice(&60u32.to_le_bytes());
        // Unity C4, a quarter semitone up; one loop.
        for v in [
            0,
            0,
            20_833,
            60,
            1 << 30,
            0,
            0,
            1,
            0,
            0,
            0,
            lp.0,
            lp.1,
            0,
            0,
        ] {
            b.extend_from_slice(&u32::to_le_bytes(v));
        }
        let riff = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&riff.to_le_bytes());
    }
    std::fs::write(&path, b).unwrap();
    path
}

fn plain(wav: &Path, rate: Rate, root: Option<Root>) -> TapeOpts {
    TapeOpts {
        wav: wav.to_path_buf(),
        form: Form::Plain { rate, root },
        name: None,
        channel: Channel::Mix,
    }
}

/// The file's head, as `put`'s streaming check reads it.
fn head_of(root: &Path, rep: &Report) -> SourceHead {
    let path = root
        .join("CHIMERA/TAPES")
        .join(rep.file().rsplit('/').next().unwrap());
    let bytes = std::fs::read(path).unwrap();
    let mut c = SourceCheck::new(rep.sref.crc, bytes.len() as u32).unwrap();
    c.feed(&bytes).unwrap();
    c.finish().unwrap()
}

/// One import onto the card at `root`.
fn run(o: &TapeOpts, root: &Path) -> Result<Report, ImportError> {
    let mut s = DirStore::new(root.to_path_buf());
    Card::new()
        .run(&mut s, |s, r| import(o, s, r))
        .expect("the sim card mounts")
        .result
}

/// The files in the card's `/CHIMERA/TAPES/`: none if it isn't there.
fn tapes(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root.join("CHIMERA/TAPES"))
        .map(|d| d.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

/// The source's `held`, read back from the card as the pool reads it.
fn held(root: &Path, rep: &Report) -> Vec<u8> {
    let mut s = DirStore::new(root.to_path_buf());
    let mut out = vec![0; 2 << 20];
    let n = Card::new()
        .run(&mut s, |s, r| read_source_into(s, r, rep.sref, &mut out))
        .unwrap()
        .result
        .expect("the source reads back");
    out.truncate(n);
    out
}

/// Two imports of one WAV, each onto its own card: the same bytes, the
/// same file, the same name.
#[test]
fn crc_is_deterministic() {
    let w = wav_file(&saw(109.0, 24_000), 48_000, None);
    let (a, b) = (fresh("card"), fresh("card"));
    let (ra, rb) = (
        run(&plain(&w, Rate::K24, None), &a).unwrap(),
        run(&plain(&w, Rate::K24, None), &b).unwrap(),
    );
    assert_eq!((ra.wrote, rb.wrote), (Wrote::New, Wrote::New));
    assert_eq!(ra.name, rb.name);
    assert_eq!(ra.name.as_str(), "TEST TAKE 1");
    assert_eq!(ra.file(), rb.file());
    let (fa, fb) = (tapes(&a), tapes(&b));
    assert_eq!(fa.len(), 1);
    assert_eq!(
        fa[0].file_name().unwrap().to_str().unwrap(),
        ra.file().rsplit('/').next().unwrap()
    );
    assert_eq!(
        std::fs::read(&fa[0]).unwrap(),
        std::fs::read(&fb[0]).unwrap()
    );
}

/// The root is a label: a re-import that only changes it is the same file,
/// already sealed on the card.
#[test]
fn reimport_with_new_root_is_present() {
    let w = wav_file(&saw(109.0, 24_000), 48_000, None);
    let card = fresh("card");
    let c3 = run(&plain(&w, Rate::K24, Some("C3".parse().unwrap())), &card).unwrap();
    let d3 = run(&plain(&w, Rate::K24, Some("D3".parse().unwrap())), &card).unwrap();
    assert_eq!((c3.wrote, d3.wrote), (Wrote::New, Wrote::Present));
    assert_eq!(c3.file(), d3.file());
    assert_eq!(tapes(&card).len(), 1);
    let root = |r: &Report| match r.tune {
        Tune::Pitched { root, .. } | Tune::Unpitched { root } => root.cents(),
        Tune::Note => panic!("a plain source has a root"),
    };
    assert_eq!((root(&c3), root(&d3)), (4_800, 5_000));
}

/// A `smpl` loop is printed as the POSITION and SIZE that play it: LOOP's
/// own bounds, from those two, land on the loop, here and after a
/// resample to 24 kHz.
#[test]
fn smpl_loop_suggests_position_and_size() {
    // 0.1 s of silence the trim takes, so the loop moves with it: frames
    // 12 000 to 36 000 of the 48 000 kept.
    let x = [vec![0.0; 4_800], saw(109.0, 48_000)].concat();
    let w = wav_file(&x, 48_000, Some((16_800, 40_800)));
    for (rate, frames) in [(Rate::K48, 48_000), (Rate::K24, 24_000)] {
        let rep = run(&plain(&w, rate, None), &fresh("card")).unwrap();
        assert_eq!(rep.sref.frames, frames);
        // The `smpl` root comes before YIN's A4.
        assert!(
            matches!(rep.tune, Tune::Pitched { root, .. } if root.cents() == 6_025),
            "{:?}",
            rep.tune
        );
        let s = rep.suggest.expect("a loop to suggest");
        // ls = POSITION · (W − Ll), Ll = 2 ms · (W / 2 ms)^SIZE.
        let b = Bounds::of(0, frames, (s.size, s.position, 0.0), rate.hz());
        let tol = frames as f32 * 0.002;
        assert!(
            (b.ls as f32 - frames as f32 / 4.0).abs() < tol,
            "{rate:?}: starts at {} of {frames}",
            b.ls
        );
        assert!(
            (b.frames() as f32 - frames as f32 / 2.0).abs() < tol,
            "{rate:?}: {} long",
            b.frames()
        );
        assert!((s.position - 0.5).abs() < 0.01, "{s:?}");
        // ln(24 000 / 96) / ln(48 000 / 96), at 48 kHz and at 24.
        assert!((s.size - 0.8885).abs() < 0.01, "{s:?}");
    }
    let none = wav_file(&saw(109.0, 48_000), 48_000, None);
    let rep = run(&plain(&none, Rate::K24, None), &fresh("card")).unwrap();
    assert_eq!(rep.suggest, None);
}

/// What NEW's Sounds leave, as the pool's own NEW counts it: 7 s at
/// 24 kHz (252 016 B held) is written, with a warning; 4 s fits. Spec § 4's
/// 153 600 B is the budget for 32 NEW Sounds; NEW holds fewer today.
#[test]
fn wont_fit_warns() {
    let (new, _) = Project::boxed();
    assert_eq!(free_beside_new(), new.pool().free());
    assert!(free_beside_new() >= Cost::of(153_600));
    let seven = run(
        &plain(
            &wav_file(&saw(109.0, 168_000), 24_000, None),
            Rate::K24,
            None,
        ),
        &fresh("card"),
    )
    .unwrap();
    assert!(seven.wont_fit());
    assert_eq!(seven.cost(), Cost::of(16 + 16 + 252_000));
    assert!(seven.to_string().contains("WON'T FIT BESIDE NEW"));
    let four = run(
        &plain(
            &wav_file(&saw(109.0, 96_000), 24_000, None),
            Rate::K24,
            None,
        ),
        &fresh("card"),
    )
    .unwrap();
    assert!(!four.wont_fit());
    assert!(!four.to_string().contains("WON'T FIT"));
}

/// 40 s at 24 kHz is past `put`'s 1 MiB: refused, and nothing written.
#[test]
fn over_1_mib_refused() {
    let card = fresh("card");
    let w = wav_file(&saw(109.0, 960_000), 24_000, None);
    let r = run(&plain(&w, Rate::K24, None), &card);
    assert!(
        matches!(r, Err(ImportError::TooBig { bytes }) if bytes > 1 << 20),
        "{r:?}"
    );
    assert!(tapes(&card).is_empty());
    // 28 s is under it (29 s, with its records' heads, is just over).
    let w = wav_file(&saw(109.0, 672_000), 24_000, None);
    assert!(run(&plain(&w, Rate::K24, None), &card).is_ok());
}

/// `--transwave 512` on 18 cycles (16 whole ones clear the resampler's
/// edges): a `Transwave` of 16 frames at the nominal 48 kHz, 1 216 samples
/// a frame, or 2 240 with `--top 1024`.
#[test]
fn transwave_import_layout() {
    let w = wav_file(&saw(512.0, 18 * 512), 48_000, None);
    for (top, len) in [(Top::T512, 1_216), (Top::T1024, 2_240)] {
        let card = fresh("card");
        let o = TapeOpts {
            wav: w.clone(),
            form: Form::Transwave {
                cycle: Cycle::new(512.0).unwrap(),
                top,
            },
            name: Some(SoundName::new("WAVES").unwrap()),
            channel: Channel::Mix,
        };
        let rep = run(&o, &card).unwrap();
        assert_eq!(rep.sref.frames, 16 * len, "{top:?}");
        assert_eq!(rep.sref.rate, 48_000);
        assert_eq!(rep.tune, Tune::Note);
        let h = held(&card, &rep);
        let d = Descriptor::from_bytes(&h).unwrap();
        assert_eq!((d.shape, d.count, d.rate), (Shape::Transwave, 16, 48_000));
        assert_eq!(SourceView::new(&h).unwrap().frames(), 16 * len);
    }
}

/// The report's last line says where the source is and how it reaches the
/// unit; Task 18 changes this line and this test.
#[test]
fn report_says_how_to_send() {
    assert_eq!(SEND_LINE, "ON THE SIM CARD · put FOR SOURCES IS TASK 18");
    let w = wav_file(&saw(109.0, 24_000), 48_000, None);
    let rep = run(&plain(&w, Rate::K24, None), &fresh("card")).unwrap();
    let text = rep.to_string();
    assert_eq!(text.lines().last(), Some(SEND_LINE));
    for field in [
        "NAME", "RATE", "FRAMES", "SECONDS", "ROOT", "PERIOD", "COST", "WROTE", "FILE",
    ] {
        assert!(
            text.lines().any(|l| l.starts_with(field)),
            "{field}:\n{text}"
        );
    }
}

/// An odd take is held even: its last sample repeated, the head's frames
/// the view's. A 5 000-sample WAV keeps 4 999: the high-pass, primed on the
/// first sample, outputs 0 there, and the trim takes it.
#[test]
fn odd_input_imports_even() {
    let card = fresh("card");
    let w = wav_file(&saw(109.0, 5_000), 24_000, None);
    let rep = run(&plain(&w, Rate::K24, None), &card).unwrap();
    assert_eq!(rep.sref.frames, 5_000);
    assert_eq!(SourceView::new(&held(&card, &rep)).unwrap().frames(), 5_000);
}

/// What the card holds reads into the pool as a source TAPE plays.
#[test]
fn imported_file_reads_into_a_view() {
    let card = fresh("card");
    let w = wav_file(&saw(109.0, 24_000), 48_000, None);
    let rep = run(&plain(&w, Rate::K16, None), &card).unwrap();
    assert_eq!((rep.sref.frames, rep.sref.rate), (8_000, 16_000));
    let h = held(&card, &rep);
    let v = SourceView::new(&h).expect("a view");
    assert_eq!(v.frames(), rep.sref.frames);
    assert_eq!(v.desc().rate, 16_000);
    // Pitched at the saw's 440.37 Hz: A4 +1, its period in 16 kHz frames.
    let Tune::Pitched { root, period } = rep.tune else {
        panic!("{:?}", rep.tune)
    };
    assert_eq!(root.to_string(), "A4 +1");
    assert!(
        (period - 16_000.0 / (48_000.0 / 109.0)).abs() < 0.05,
        "{period}"
    );
    // The head's labels, as `put`'s check reads them from the file.
    let h = head_of(&card, &rep);
    assert_eq!((h.sref, h.shape, h.count), (rep.sref, Shape::Plain, 1));
    assert_eq!(h.root, 6_901);
    assert_eq!(h.period_q16, (period * 65_536.0).round() as u32);
    assert_eq!(h.name.as_str(), "TEST TAKE 1");
    // One field a line.
    let text = rep.to_string();
    for line in [
        "RATE     16 kHz",
        "FRAMES   8000",
        "SECONDS  0.50",
        "ROOT     A4 +1",
        "WROTE    NEW",
    ] {
        assert!(text.lines().any(|l| l == line), "{line:?} in\n{text}");
    }
    assert!((rep.seconds() - 0.5).abs() < 1e-6);
}

/// Silence, noise and a too-short take: refused, or unpitched.
#[test]
fn what_it_refuses() {
    let card = fresh("card");
    let silent = wav_file(&[0.0; 4_800], 48_000, None);
    assert!(matches!(
        run(&plain(&silent, Rate::K24, None), &card),
        Err(ImportError::Silent)
    ));
    let short = wav_file(&saw(10.0, 100), 48_000, None);
    assert!(matches!(
        run(&plain(&short, Rate::K24, None), &card),
        Err(ImportError::TooShort { frames: 50 })
    ));
    // A cycle longer than the take.
    let o = TapeOpts {
        form: Form::Transwave {
            cycle: Cycle::new(4_000.0).unwrap(),
            top: Top::T512,
        },
        ..plain(&wav_file(&saw(109.0, 1_000), 48_000, None), Rate::K24, None)
    };
    assert!(matches!(run(&o, &card), Err(ImportError::NoCycles)));
    let missing = card.join("nope.wav");
    assert!(matches!(
        run(&plain(&missing, Rate::K24, None), &card),
        Err(ImportError::Io(_))
    ));
    assert!(tapes(&card).is_empty());
}

/// The arguments `just tape` passes, and the ones it refuses.
#[test]
fn args_parse() {
    let args = |s: &str| -> Vec<String> { s.split(' ').map(String::from).collect() };
    let o = TapeOpts::from_args(&args("a.wav")).unwrap();
    assert_eq!(
        o.form,
        Form::Plain {
            rate: Rate::K24,
            root: None
        }
    );
    assert_eq!((o.channel, o.name), (Channel::Mix, None));
    let o =
        TapeOpts::from_args(&args("a.wav --rate 8k --root F#2 --name CHOIR --channel R")).unwrap();
    assert_eq!(
        o.form,
        Form::Plain {
            rate: Rate::K8,
            root: Some(Root::new(4_200).unwrap())
        }
    );
    assert_eq!(o.channel, Channel::R);
    assert_eq!(o.name.unwrap().as_str(), "CHOIR");
    let o = TapeOpts::from_args(&args("a.wav --transwave 109.5 --top 1024")).unwrap();
    assert_eq!(
        o.form,
        Form::Transwave {
            cycle: Cycle::new(109.5).unwrap(),
            top: Top::T1024
        }
    );
    for bad in [
        "",
        "a.wav --rate 44k",
        "a.wav --root H2",
        "a.wav --top 1024",
        "a.wav --transwave 1",
        "a.wav --transwave 512 --rate 24k",
        "a.wav --transwave 512 --root C3",
        "a.wav --name lower_case!",
        "a.wav --channel both",
        "a.wav b.wav",
        "a.wav --rate",
    ] {
        assert!(TapeOpts::from_args(&args(bad)).is_err(), "{bad:?}");
    }
}

/// Note names to MIDI cents (6 000 is C4), and back.
#[test]
fn roots_name_notes() {
    for (s, c) in [
        ("C4", 6_000),
        ("C3", 4_800),
        ("Bb2", 4_600),
        ("C-1", 0),
        ("G9", 12_700),
    ] {
        let r: Root = s.parse().unwrap();
        assert_eq!(r.cents(), c, "{s}");
    }
    assert!("G#9".parse::<Root>().is_err());
    assert_eq!(Root::new(4_612).unwrap().to_string(), "A#2 +12");
    assert_eq!(Root::new(4_588).unwrap().to_string(), "A#2 -12");
    assert_eq!(Root::new(12_701), None);
}
