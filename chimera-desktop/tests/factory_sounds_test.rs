//! The factory Sounds' text Sources (spec § 3, ADR 0134): a golden Source
//! builds to committed bytes, `fmt` is idempotent, text → Sound → text is
//! identical, every committed Source builds, and each bad Source fails
//! with its line.

use chimera_core::block::{ParamId, ParamSpec};
use chimera_core::dsp::lfo::{LFO_SPECS, LfoParams};
use chimera_core::dsp::modulator::{Func, LfoForm};
use chimera_core::params::{ENV_SPECS, EnvParams};
use chimera_core::storage::{
    BuildStamp, Card, FileKind, SoundDecoder, load_file, peek_library, sound_crc,
};
use chimera_core::ui::draw::text_width;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::mod_panel;
use chimera_core::ui::theme::{CONTENT_W, FONT_LABEL, LABEL_TRACKING};
use chimera_core::ui::view::{EnvKind, LfoKind};
use chimera_core::{params::EngineType, preset::Sound};
use chimera_desktop::factory_sounds::{build, source};
use chimera_hal::store::{Dir, FileName, Store};
use chimera_hal::testkit::MemStore;

const STAMP: BuildStamp = BuildStamp::new("0.1.0", "1a2b3c4d", false);
const FIX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/factory_sounds/"
);
const SOURCES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/factory-sounds");

fn golden() -> String {
    std::fs::read_to_string(format!("{FIX}golden.toml")).unwrap()
}

/// A valid Source; each bad test changes one line.
const BASE: &str = r#"[[sound]]
id = "S9100002"
name = "TEST PAD"
engine = "VA"
note = "A TEST PAD: SLOW FILTER"
demo = "poly"
plays = "long texture"
home = "C3"
FILTER.CUTOFF = "1200"
mod.LFO1 = "FILTER.CUTOFF +20"
trim = "0"
"#;

/// `BASE` with line `n` (1-based) replaced by `with`.
fn base_with(n: usize, with: &str) -> String {
    BASE.lines()
        .enumerate()
        .map(|(i, l)| if i + 1 == n { with } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse, check and the public refusals, as `build::public` runs them.
fn run(text: &str) -> Result<Vec<source::Source>, String> {
    let s = source::parse(text, "bad.toml")?;
    source::check(&s)?;
    source::public(&s)?;
    for x in &s {
        source::to_sound(x)?;
    }
    Ok(s)
}

fn fails(text: &str, line: usize, what: &str) {
    let e = run(text).expect_err("a bad Source must fail");
    assert!(
        e.contains(&format!("bad.toml:{line}:")) && e.contains(what),
        "want bad.toml:{line} naming {what}, got: {e}"
    );
}

#[test]
fn base_passes() {
    run(BASE).unwrap();
}

#[test]
fn golden_builds_to_committed_bytes() {
    let s = source::parse(&golden(), "golden.toml").unwrap();
    assert_eq!(s.len(), 1);
    let bytes = source::file_bytes(&s[0], &STAMP).unwrap();
    let path = format!("{FIX}golden.A");
    if std::env::var_os("GOLDEN_RECORD").is_some() {
        std::fs::write(&path, &bytes).unwrap();
    }
    assert_eq!(
        bytes,
        std::fs::read(&path).unwrap(),
        "GOLDEN_RECORD=1 rewrites"
    );

    // The peek a FACTORY row reads: name, engine, stamp and description.
    let p = peek_library(&bytes, bytes.len() as u32).unwrap();
    assert_eq!(p.name, Some(s[0].name));
    assert_eq!(p.engine, Some(EngineType::Algo));
    assert_eq!(p.stamp, Some(STAMP));
    assert_eq!(p.note, Some(s[0].note));

    // The card decodes it to the Sound the Source describes.
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    let f = FileName::new(Dir::Sounds, b"S9100001", b"A").unwrap();
    st.write(v, f, &mut |sink| sink.put(&bytes)).unwrap();
    let mut got = Sound::neutral(EngineType::Modal);
    let h = Card::new()
        .run(&mut st, |s, r| {
            Ok::<_, chimera_core::storage::LoadError>(load_file(
                s,
                r,
                f,
                &mut SoundDecoder::new(&mut got),
            ))
        })
        .unwrap()
        .result
        .unwrap()
        .unwrap();
    assert_eq!(h.kind, FileKind::Sound);
    assert_eq!(
        sound_crc(&got),
        sound_crc(&source::to_sound(&s[0]).unwrap())
    );
}

#[test]
fn fmt_is_idempotent() {
    let g = golden();
    let once = source::fmt(&g);
    assert_eq!(source::fmt(&once), once);
    // A display comment on a value line, ignored on read.
    let cut = once
        .lines()
        .find(|l| l.starts_with("FILTER.CUTOFF"))
        .unwrap();
    assert!(cut.contains("  # "), "{cut}");
    let crc = |t: &str| sound_crc(&source::to_sound(&source::parse(t, "g").unwrap()[0]).unwrap());
    assert_eq!(crc(&once), crc(&g));
}

#[test]
fn text_sound_text_identical() {
    let g = golden();
    let s = source::parse(&g, "golden.toml").unwrap();
    let authored: Vec<String> = g
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(" = ")?;
            k.contains('.')
                .then(|| format!("{k} = {}", v.split("  #").next().unwrap()))
        })
        .collect();
    assert_eq!(source::lines(&source::patch(&s[0]).unwrap()), authored);
}

#[test]
fn trim_moves_the_output_level() {
    let s = &source::parse(&golden(), "golden.toml").unwrap()[0];
    assert!(s.trim < 0.0);
    let (p, t) = (source::patch(s).unwrap(), source::to_sound(s).unwrap());
    let want = p.params.out.volume * 10f32.powf(s.trim / 20.0);
    assert!((t.params.out.volume - want).abs() < 1e-6);
}

#[test]
fn every_committed_source_builds() {
    let s = build::sources(std::path::Path::new(SOURCES)).unwrap();
    source::check(&s).unwrap();
    source::public(&s).unwrap();
    let (files, man) = build::files(&s, &STAMP).unwrap();
    assert_eq!(files.len(), man.sounds.len());
    assert!(man.json().starts_with('{'));
}

#[test]
fn unknown_ident() {
    fails(&base_with(9, r#"FILTER.FREQ = "1200""#), 9, "FILTER.FREQ");
    fails(&base_with(9, r#"FILTR.CUTOFF = "1200""#), 9, "FILTR.CUTOFF");
    fails(&base_with(9, r#"FILTER.KIND = "MOOG""#), 9, "MOOG");
    fails(
        &base_with(10, r#"mod.LFO9 = "FILTER.CUTOFF +20""#),
        10,
        "LFO9",
    );
    fails(&base_with(6, r#"demo = "stereo""#), 6, "stereo");
    fails(&base_with(7, r#"plays = "long arp""#), 7, "arp");
    fails(&base_with(8, r#"home = "H3""#), 8, "H3");
    fails(&base_with(4, r#"engine = "TAPE""#), 4, "TAPE");
}

#[test]
fn unroutable_destination() {
    fails(
        &base_with(10, r#"mod.LFO1 = "FILTER.CUTOFF +20, FILTER.KIND +20""#),
        10,
        "FILTER.KIND",
    );
    fails(
        &base_with(10, r#"mod.LFO1 = "FILTER.CUTOFF +200""#),
        10,
        "+200",
    );
    // A tap reads an ALGO operator, into the filter's fixed columns only.
    let algo = base_with(4, r#"engine = "ALGO""#);
    let tap = |t: &str| {
        algo.lines()
            .enumerate()
            .map(|(i, l)| if i + 1 == 10 { t } else { l })
            .collect::<Vec<_>>()
            .join("\n")
    };
    run(&tap(r#"tap.OP5 = "FILTER.CUTOFF +40""#)).unwrap();
    fails(&tap(r#"tap.OP5 = "ALGO.MORPH +40""#), 10, "ALGO.MORPH");
    fails(&tap(r#"tap.OP3 = "FILTER.CUTOFF +40""#), 10, "OP3");
    fails(
        &base_with(10, r#"tap.OP5 = "FILTER.CUTOFF +40""#),
        10,
        "ALGO",
    );
}

#[test]
fn duplicate_id() {
    let two = format!("{BASE}\n{}", BASE.replace("TEST PAD\"", "OTHER PAD\""));
    fails(&two, 14, "S9100002");
}

#[test]
fn duplicate_name() {
    let two = format!("{BASE}\n{}", BASE.replace("S9100002", "S9100003"));
    fails(&two, 15, "TEST PAD");
}

#[test]
fn id_outside_range() {
    for id in ["S9000005", "S9200000", "S0000005", "S9099999", "X9100002"] {
        fails(&base_with(2, &format!("id = \"{id}\"")), 2, id);
    }
    for id in ["S9100001", "S9199999"] {
        run(&base_with(2, &format!("id = \"{id}\""))).unwrap();
    }
}

#[test]
fn public_refuses_port_id() {
    for id in ["S9100010", "S9100025"] {
        fails(&base_with(2, &format!("id = \"{id}\"")), 2, id);
    }
    for id in ["S9100009", "S9100026"] {
        run(&base_with(2, &format!("id = \"{id}\""))).unwrap();
    }
}

#[test]
fn description_too_wide() {
    let wide = "W".repeat(38);
    assert!(wide.len() <= 40);
    assert!(text_width(&FONT_LABEL, &wide, LABEL_TRACKING) > CONTENT_W);
    fails(&base_with(5, &format!("note = \"{wide}\"")), 5, "wide");
    fails(
        &base_with(5, &format!("note = \"{}\"", "I".repeat(41))),
        5,
        "40",
    );
}

#[test]
fn description_bad_char() {
    fails(&base_with(5, r#"note = "A TEST PAD!""#), 5, "note");
    fails(&base_with(5, r#"note = "a test pad""#), 5, "note");
    fails(&base_with(5, r#"note = """#), 5, "note");
}

#[test]
fn public_refuses_from() {
    let ported = format!("{BASE}from = \"tx81z 1a2b3c4d #12\"\nedits = \"filter: 2040\"\n");
    // Parses (the private build reads it), but the public set refuses it.
    source::parse(&ported, "bad.toml").unwrap();
    fails(&ported, 12, "from");
}

#[test]
fn placeholder_skipped() {
    let two = format!(
        "{BASE}\n{}status = \"placeholder\"\n",
        BASE.replace("S9100002", "S9100003")
            .replace("TEST PAD\"", "VOCAL TWO\"")
    );
    let s = run(&two).unwrap();
    assert!(s[1].placeholder);
    let (files, man) = build::files(&s, &STAMP).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(man.sounds.len(), 1);
    assert_eq!(files[0].0, "S9100002.A");
    // It still reserves its id and name.
    let clash = format!(
        "{BASE}\n{}status = \"placeholder\"\n",
        BASE.replace("S9100002", "S9100003")
    );
    fails(&clash, 15, "TEST PAD");
}

/// `fmt`'s comment for `key` in `text`.
fn comment(text: &str, key: &str) -> String {
    let f = source::fmt(text);
    let l = f.lines().find(|l| l.starts_with(key)).unwrap();
    l.split("  # ").nth(1).unwrap_or("").to_string()
}

/// The page's own readout of `v` in `block`'s param `id` on `panel`.
fn on_panel(panel: &mod_panel::ModPanel, id: ParamId, spec_of: &[ParamSpec], v: f32) -> String {
    let fmt = panel
        .slots
        .iter()
        .flatten()
        .find_map(|c| match *c {
            mod_panel::PanelSlot::Param { id: i, fmt, .. } if i == id => Some(fmt),
            _ => None,
        })
        .unwrap();
    let spec = spec_of.iter().find(|s| s.id == id).unwrap();
    let mut b = FmtBuf::new();
    fmt_val(&mut b, spec.normalize(v), fmt);
    b.as_str().to_string()
}

#[test]
fn fmt_reads_as_the_page_draws() {
    // CLASSIC LFO RATE, and a type-B ENV's RISE (its RATE under LFO).
    let t = format!(
        "{BASE}LFO1.RATE = \"0.5\"\nENV2.TYPE = \"B\"\nENV2.MODE = \"LFO\"\nENV2.RISE = \"0.3\"\nLFO2.TYPE = \"FUNC\"\nLFO2.RISE = \"0.3\"\n"
    );
    run(&t).unwrap();
    let rate = comment(&t, "LFO1.RATE");
    assert_eq!(
        rate,
        on_panel(
            mod_panel::lfo_panel(LfoKind::Classic),
            LfoParams::RATE,
            &LFO_SPECS,
            0.5
        )
    );
    let rise = comment(&t, "ENV2.RISE");
    assert_eq!(
        rise,
        on_panel(
            mod_panel::env_panel(EnvKind::B(Func::Lfo(LfoForm::Free))),
            EnvParams::RISE,
            &ENV_SPECS,
            0.3
        )
    );
    assert!(rise.ends_with("Hz"), "{rise}");
    let func = comment(&t, "LFO2.RISE");
    assert_eq!(
        func,
        on_panel(
            mod_panel::lfo_panel(LfoKind::Func(LfoForm::Free)),
            LfoParams::RISE,
            &LFO_SPECS,
            0.3
        )
    );
    assert!(func.ends_with("Hz"), "{func}");
    println!("LFO1.RATE # {rate}; ENV2.RISE # {rise}; LFO2.RISE # {func}");
}

#[test]
fn trim_out_of_range_fails() {
    // VA's neutral OUT LEVEL is 0.8: +3 dB passes 1.0, the param's top.
    fails(&base_with(11, r#"trim = "+3""#), 11, "OUT LEVEL");
    fails(&base_with(11, r#"trim = "1e30""#), 11, "OUT LEVEL");
    // Never clipped: the check is `trimmed`, which `fmt --level` reuses.
    assert!(source::trimmed(0.8, 3.0).is_err());
    assert!(source::trimmed(0.5, 6.0).is_ok());
    // OUT LEVEL's floor is 0: a cut only nears it, so no low side fails.
    assert!(source::trimmed(0.8, -200.0).is_ok_and(|t| t >= 0.0));
    run(&base_with(11, r#"trim = "+1.9""#)).unwrap();
}

// ---- QA before UAT (spec § 4) ----

use chimera_core::part::PartMode;
use chimera_desktop::factory_sounds::qa::{self, Check, Gate, Plan, Verdict};
use chimera_desktop::factory_sounds::source::Script;

const SR: f32 = 48_000.0;

fn hz(n: u8) -> f32 {
    440.0 * 2f32.powf((n as f32 - 69.0) / 12.0)
}

fn sine(f: f32, amp: f32, secs: f32) -> Vec<f32> {
    (0..(secs * SR) as usize)
        .map(|i| amp * (std::f32::consts::TAU * f * i as f32 / SR).sin())
        .collect()
}

/// Deterministic white noise, ±`amp`.
fn noise(amp: f32, secs: f32) -> Vec<f32> {
    let mut s = 0x2545_f491_u32;
    (0..(secs * SR) as usize)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            amp * (s as f32 / u32::MAX as f32 * 2.0 - 1.0)
        })
        .collect()
}

#[test]
fn scripts_follow_the_spec() {
    let notes = |p: &Plan| p.hits.iter().map(|h| h.note).collect::<Vec<_>>();
    let held = |p: &Plan| p.hits.iter().map(|h| h.off - h.on).collect::<Vec<_>>();
    let v100 = |p: &Plan| p.hits.iter().all(|h| h.vel == 100);

    let bass = qa::plan(Script::Bass, 36);
    assert_eq!(notes(&bass), [36]);
    assert_eq!(held(&bass), [0.3]);
    assert_eq!(bass.secs, 0.3 + 1.5);
    assert!(v100(&bass));

    let stab = qa::plan(Script::Stab, 36);
    assert_eq!(notes(&stab), [48, 52, 55, 59], "C3 E3 G3 B3");
    assert!(held(&stab).iter().all(|&h| h == 0.25) && v100(&stab));
    assert!(stab.hits.iter().all(|h| h.on == 0.0));

    let short = qa::plan(Script::Short, 36);
    assert_eq!(notes(&short), [60], "C4");
    assert_eq!(held(&short), [0.08]);

    let long = qa::plan(Script::Long, 48);
    assert_eq!(notes(&long), [48]);
    assert_eq!(held(&long), [2.0]);
    assert_eq!(long.secs, 4.0);

    let tex = qa::plan(Script::Texture, 48);
    assert_eq!(tex.hits.len(), 16);
    for (i, h) in tex.hits.iter().enumerate() {
        assert!(
            (h.on - i as f32 * 0.125).abs() < 1e-6,
            "sixteenths at 120 BPM"
        );
        assert!(h.off <= h.on + 0.125);
    }
    let (lo, hi) = (
        tex.hits.iter().map(|h| h.note).min().unwrap(),
        tex.hits.iter().map(|h| h.note).max().unwrap(),
    );
    assert_eq!((lo, hi), (48, 72), "two octaves from home");
    assert_eq!(tex.hits[0].vel, 40);
    assert_eq!(tex.hits[15].vel, 127);
    assert!(tex.hits.windows(2).all(|w| w[0].vel <= w[1].vel));

    let glide = qa::plan(Script::Glide, 60);
    assert_eq!(notes(&glide), [36, 43, 48], "C2 G2 C3");
    assert_eq!(glide.mode, Some(PartMode::Mono));
    assert!(
        glide.hits.windows(2).all(|w| w[1].on < w[0].off),
        "overlapping"
    );
    for s in [
        Script::Bass,
        Script::Stab,
        Script::Short,
        Script::Long,
        Script::Texture,
    ] {
        assert_eq!(qa::plan(s, 48).mode, None, "{s:?} plays the demo mode");
    }
}

#[test]
fn loudness_gate() {
    for l in [-11.0, -9.0, -13.0] {
        assert_eq!(qa::loudness(l, None), Verdict::Pass, "{l}");
    }
    for l in [-8.9, -13.1, -30.0, f32::NAN] {
        assert_eq!(qa::loudness(l, None), Verdict::Fail, "{l}");
    }
    assert!(matches!(
        qa::loudness(-20.0, Some("a soft pad under the lead")),
        Verdict::Exempt(why) if why.contains("soft pad")
    ));
}

#[test]
fn limiter_gate() {
    assert_eq!(qa::limiter(0.9, false), Verdict::Pass);
    assert_eq!(qa::limiter(1.1, false), Verdict::Fail);
    assert_eq!(qa::limiter(2.9, true), Verdict::Pass);
    assert_eq!(qa::limiter(3.1, true), Verdict::Fail);
}

#[test]
fn click_dc_finite_gate() {
    // A sine from a zero crossing: no click.
    let clean = sine(220.0, 0.3, 1.0);
    assert_eq!(qa::clicks(&clean, &[0]).1, Verdict::Pass);
    let mut click = clean.clone();
    click[24_000] += 0.2;
    assert_eq!(qa::clicks(&click, &[0]).1, Verdict::Fail, "a click");
    let dc: Vec<f32> = clean.iter().map(|v| v + 0.002).collect();
    assert_eq!(qa::clicks(&dc, &[0]).1, Verdict::Fail, "DC over -60 dBFS");
    let mut nan = clean.clone();
    nan[100] = f32::NAN;
    assert_eq!(qa::clicks(&nan, &[0]).1, Verdict::Fail, "not finite");
}

#[test]
fn aliasing_gate() {
    for n in [84u8, 96] {
        let f0 = hz(n);
        // A band-limited saw: harmonics only.
        let saw: Vec<f32> = (0..96_000)
            .map(|i| {
                let t = i as f32 / SR;
                (1..)
                    .take_while(|&k| k as f32 * f0 < 20_000.0)
                    .map(|k| (std::f32::consts::TAU * k as f32 * f0 * t).sin() / k as f32)
                    .sum::<f32>()
                    * 0.2
            })
            .collect();
        let (db, v) = qa::aliasing(&saw, f0, true);
        assert_eq!(v, Verdict::Pass, "C{}: {db:.1} dB", n / 12 - 1);
        // An alias 30 dB down, between harmonics.
        let bad: Vec<f32> = saw
            .iter()
            .zip(sine(f0 * 1.37, 0.2 * 0.0316, 2.0))
            .map(|(a, b)| a + b)
            .collect();
        let (db, v) = qa::aliasing(&bad, f0, true);
        assert_eq!(v, Verdict::Fail, "{db:.1} dB");
        assert!(matches!(
            qa::aliasing(&bad, f0, false).1,
            Verdict::Exempt(_)
        ));
    }
}

#[test]
fn tail_gate() {
    // Off at 1 s; silent by 2 s.
    let mut x = sine(220.0, 0.3, 1.0);
    x.extend(
        sine(220.0, 0.3, 1.0)
            .iter()
            .enumerate()
            .map(|(i, v)| v * (-(i as f32) / 8_000.0).exp()),
    );
    x.extend(vec![0.0; 96_000]);
    assert_eq!(qa::tail(&x, 48_000, 2.0).1, Verdict::Pass);
    // A hum that never stops.
    let hum = sine(50.0, 0.01, 5.0);
    let (db, v) = qa::tail(&hum, 48_000, 2.0);
    assert_eq!(v, Verdict::Fail, "{db}");
    // Still ringing at the limit fails; a render too short to reach it too.
    assert_eq!(qa::tail(&x, 48_000, 0.5).1, Verdict::Fail);
    assert_eq!(qa::tail(&x[..96_000], 48_000, 2.0).1, Verdict::Fail);

    // MODAL keeps ringing: 10 s; anything else its release plus 2 s.
    let modal = source::parse(&BASE.replace("\"VA\"", "\"MODAL\""), "m.toml").unwrap();
    let snd = source::to_sound(&modal[0]).unwrap();
    assert_eq!(qa::tail_within(&modal[0], &snd), 10.0);
    let va = source::parse(BASE, "v.toml").unwrap();
    let snd = source::to_sound(&va[0]).unwrap();
    let w = qa::tail_within(&va[0], &snd);
    assert!(
        (2.0..3.0).contains(&w),
        "VA's neutral release (300 ms) + 2 s: {w}"
    );
}

#[test]
fn tap_gate() {
    let a = noise(0.3, 1.0);
    // The same noise, its top lowpassed away (a one-pole at ~500 Hz).
    let mut y = 0.0;
    let dull: Vec<f32> = a
        .iter()
        .map(|&v| {
            y += 0.065 * (v - y);
            y
        })
        .collect();
    let (ba, bd) = (qa::erb_bands(&a), qa::erb_bands(&dull));
    let err = qa::spectral_error(&ba, &bd);
    assert_eq!(qa::tap(err), Verdict::Pass, "{err:.1} dB");
    let same = qa::erb_bands(&a.iter().map(|v| v * 1.05).collect::<Vec<_>>());
    let err = qa::spectral_error(&ba, &same);
    assert_eq!(
        qa::tap(err),
        Verdict::Fail,
        "{err:.1} dB: the tap is inaudible"
    );
}

#[test]
fn samey_flags_pair_within_3db_on_32_erb() {
    let a = noise(0.3, 1.0);
    let b: Vec<f32> = a.iter().map(|v| v * 1.1).collect(); // +0.8 dB
    let c = sine(440.0, 0.3, 1.0);
    let bands: Vec<[f32; 32]> = [&a, &b, &c].iter().map(|x| qa::erb_bands(x)).collect();
    assert_eq!(bands[0].len(), qa::ERB_BANDS);
    assert_eq!(qa::samey(&bands), vec![(0, 1)]);
    // 4 dB louder everywhere is not samey.
    let d: Vec<f32> = a.iter().map(|v| v * 1.585).collect();
    assert!(qa::samey(&[bands[0], qa::erb_bands(&d)]).is_empty());
}

/// A Report for `BASE` without rendering: one `long` render and gates.
fn report(pass: bool, private: bool) -> qa::Report {
    let s = &source::parse(BASE, "r.toml").unwrap()[0];
    qa::Report {
        id: s.id,
        name: s.name.as_str().into(),
        note: s.note.as_str().into(),
        demo: s.demo,
        edits: private.then(|| "filter: 2040 instead of open".into()),
        private,
        gates: vec![Gate {
            check: Check::Loudness,
            value: if pass { -11.0 } else { -20.0 },
            verdict: if pass { Verdict::Pass } else { Verdict::Fail },
        }],
        renders: vec![qa::Render {
            script: Script::Long,
            mono: sine(220.0, 0.2, 0.5),
            lufs: -11.0,
            peak_dbfs: -14.0,
        }],
        bands: [0.0; 32],
    }
}

fn scratch_ledger(name: &str) -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn page_only_when_all_pass() {
    let d = scratch_ledger("ledger-page");
    assert!(qa::write(&d, &[report(true, false)], &[], true).unwrap());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("TEST PAD") && page.contains("A TEST PAD: SLOW FILTER"));
    assert!(page.contains("POLY") && page.contains("-11.0 LUFS"));
    assert!(
        page.contains("\"renders/S9100002-long.wav\""),
        "relative paths"
    );
    assert!(d.join("renders/S9100002-long.wav").is_file());
    assert!(
        !page.contains("PRIVATE"),
        "no private section without ports"
    );
    assert!(d.join("report.txt").is_file());
    assert!(!d.join("index.html").exists(), "never index.html");

    // Ports render into their own section, with their edits.
    assert!(qa::write(&d, &[report(true, false), report(true, true)], &[], true).unwrap());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("PRIVATE") && page.contains("filter: 2040 instead of open"));

    // One failing gate: the report, and no page (an old one is removed).
    assert!(!qa::write(&d, &[report(true, false), report(false, false)], &[], true).unwrap());
    assert!(!d.join("listening.html").exists());
    let txt = std::fs::read_to_string(d.join("report.txt")).unwrap();
    assert!(txt.contains("FAIL") && txt.contains("S9100002"), "{txt}");
}

#[test]
fn ledger_never_tmp() {
    let tmp = std::env::temp_dir();
    for bad in [
        "/tmp/showcase".into(),
        "/tmp/scratch-1000/x/scratchpad/ledger".into(),
        tmp.join("ledger"),
        "~/../../tmp/x".into(),
        "relative/ledger".into(),
    ] {
        let p: std::path::PathBuf = bad;
        assert!(qa::checked_ledger(&p).is_err(), "{}", p.display());
    }
    let ok = "~/dev/chimera/.superpowers/ledgers/showcase";
    assert_eq!(
        qa::checked_ledger(ok.as_ref()).unwrap(),
        std::path::PathBuf::from(ok)
    );
    // `write` refuses one too.
    assert!(qa::write("/tmp/showcase-qa".as_ref(), &[], &[], false).is_err());
}

#[test]
fn fmt_level_writes_trim() {
    let s = source::parse(BASE, "b.toml").unwrap();
    // 1.5 dB under the reference: +1.5 dB on top of the written 0.
    let t = qa::level_trim(&s[0], -12.5).unwrap();
    assert!((t - 1.5).abs() < 0.05, "{t}");
    let out = source::set_trims(BASE, &[(&s[0], t)]);
    assert!(out.contains("trim = \"1.5\""), "{out}");
    assert_eq!(source::parse(&out, "b.toml").unwrap()[0].trim, 1.5);
    // No trim line yet: one is added inside the table.
    let bare = BASE.replace("trim = \"0\"\n", "");
    let b = source::parse(&bare, "b.toml").unwrap();
    let two = format!(
        "{bare}\n{}",
        bare.replace("S9100002", "S9100003")
            .replace("TEST PAD", "TEST TWO")
    );
    let both = source::parse(&two, "b.toml").unwrap();
    let out = source::set_trims(&two, &[(&both[0], -1.5), (&both[1], 0.5)]);
    let got = source::parse(&out, "b.toml").unwrap();
    assert_eq!((got[0].trim, got[1].trim), (-1.5, 0.5), "{out}");
    assert_eq!(b[0].trim_line, 0);
    // Past OUT LEVEL's range: fails naming the Source's line, never clips.
    let e = qa::level_trim(&s[0], -30.0).unwrap_err();
    assert!(e.contains("b.toml:11:") && e.contains("OUT LEVEL"), "{e}");
}

#[test]
fn gates_render_through_the_instrument() {
    let s = &source::parse(BASE, "g.toml").unwrap()[0];
    let long = qa::render(s, Script::Long).unwrap();
    assert_eq!(long.len(), 4 * 48_000, "48 kHz, the script's length");
    assert!(long.iter().all(|v| v.is_finite()));
    assert!(
        long.iter().fold(0f32, |m, v| m.max(v.abs())) > 0.01,
        "it sounds"
    );
    let r = qa::gates(s).unwrap();
    let scripts: Vec<Script> = r.renders.iter().map(|x| x.script).collect();
    assert_eq!(scripts, [Script::Long, Script::Texture]);
    let has = |c: Check| r.gates.iter().any(|g| g.check == c);
    assert!(has(Check::Loudness) && has(Check::Tail) && has(Check::Limiter(None)));
    assert!(has(Check::Clicks(Script::Texture)) && has(Check::Limiter(Some(Script::Texture))));
    assert!(
        has(Check::Aliasing(84)) && has(Check::Aliasing(96)),
        "VA is harmonic"
    );
    assert!(!has(Check::Tap), "no tap routes");
    for g in &r.gates {
        println!("{:?} {:.2} {:?}", g.check, g.value, g.verdict);
    }
}
