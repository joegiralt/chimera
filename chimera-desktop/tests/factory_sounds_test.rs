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
use chimera_desktop::factory_sounds::{about, build, source};
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
mod.ENV1 = "OUT.VCA +127"
phrase = "pad"
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
    fails(&two, 16, "S9100002");
}

#[test]
fn duplicate_name() {
    let two = format!("{BASE}\n{}", BASE.replace("S9100002", "S9100003"));
    fails(&two, 17, "TEST PAD");
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
    fails(&ported, 14, "from");
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
    fails(&clash, 17, "TEST PAD");
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
use chimera_desktop::factory_sounds::qa::{self, Audition, Check, Gate, PhraseNote, Plan, Verdict};
use chimera_desktop::factory_sounds::source::{Phrase, Script};

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
    // A single note (the level render, or any script but stab): 1 dB.
    for s in [
        None,
        Some(Script::Long),
        Some(Script::Bass),
        Some(Script::Texture),
    ] {
        assert_eq!(qa::limiter(0.9, s), Verdict::Pass, "{s:?}");
        assert_eq!(qa::limiter(1.1, s), Verdict::Fail, "{s:?}");
    }
    // `stab`, the chord: 3 dB.
    assert_eq!(qa::limiter(2.9, Some(Script::Stab)), Verdict::Pass);
    assert_eq!(qa::limiter(3.1, Some(Script::Stab)), Verdict::Fail);
}

#[test]
fn click_and_finite_gate() {
    // A sine from a zero crossing: no click.
    let clean = sine(220.0, 0.3, 1.0);
    assert_eq!(qa::clicks(&clean, &[0]).1, Verdict::Pass);
    let mut click = clean.clone();
    click[24_000] += 0.2;
    assert_eq!(qa::clicks(&click, &[0]).1, Verdict::Fail, "a click");
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
    // The VCA follows ENV 2 too, its release 4.5 s (MED at 0.9): the
    // longest release routed to the VCA sets the limit.
    let env2 = format!("{BASE}ENV2.REL = \"0.9\"\nmod.ENV2 = \"OUT.VCA +127\"\n");
    let s = source::parse(&env2, "e.toml").unwrap();
    let snd = source::to_sound(&s[0]).unwrap();
    let w = qa::tail_within(&s[0], &snd);
    assert!((6.0..7.0).contains(&w), "ENV 2's 4.5 s + 2 s: {w}");
    // An ENV 2 not routed to the VCA does not count.
    let free = format!("{BASE}ENV2.REL = \"0.9\"\n");
    let s = source::parse(&free, "e.toml").unwrap();
    let snd = source::to_sound(&s[0]).unwrap();
    assert!(qa::tail_within(&s[0], &snd) < 3.0);
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
            audition: Audition::Script(Script::Long),
            stereo: stereo(&sine(220.0, 0.2, 0.5), &sine(330.0, 0.1, 0.5)),
            lufs: -11.0,
            peak_dbfs: [-14.0, -20.0],
            with: Vec::new(),
            limited_db: None,
        }],
        long_lufs: Some(-12.3),
        bands: [0.0; 32],
        badge: s.badge(),
        about: about::of(s).unwrap(),
    }
}

fn scratch_ledger(name: &str) -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn a_failing_sound_is_skipped_from_the_page() {
    let d = scratch_ledger("ledger-page");
    assert!(qa::write(&d, &[report(true, false)], &[], &[], true).unwrap());
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
    assert!(
        qa::write(
            &d,
            &[report(true, false), report(true, true)],
            &[],
            &[],
            true
        )
        .unwrap()
    );
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("PRIVATE") && page.contains("filter: 2040 instead of open"));

    // One failing Sound: off the page, named in the report; the rest stay.
    let mut bad = report(false, false);
    bad.name = "BAD PAD".into();
    bad.id = source::parse(&BASE.replace("S9100002", "S9100003"), "b.toml").unwrap()[0].id;
    // An old render of it, from a run when it passed.
    std::fs::write(d.join("renders/S9100003-long.wav"), b"old").unwrap();
    assert!(!qa::write(&d, &[report(true, false), bad], &[], &[], true).unwrap());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(
        page.contains("<h3>TEST PAD") && !page.contains("<h3>BAD PAD"),
        "{page}"
    );
    let txt = std::fs::read_to_string(d.join("report.txt")).unwrap();
    assert!(txt.contains("FAIL") && txt.contains("S9100003"), "{txt}");
    assert!(txt.contains("off the page: S9100003 BAD PAD"), "{txt}");
    assert!(
        page.contains("Off the page (a gate failed, see report.txt): S9100003 BAD PAD"),
        "the page names what it left off: {page}"
    );
    assert!(page.contains("class=\"flag\">Off the page"), "flagged");
    // Its old renders are not published as current.
    assert!(
        !d.join("renders/S9100003-long.wav").exists(),
        "a dropped Sound's WAV removed"
    );
    assert!(!page.contains("S9100003-long.wav"));
    assert!(
        d.join("renders/S9100002-long.wav").is_file(),
        "the passing one's stays"
    );

    // A failing run without --wav removes the old page: it could show the
    // failing Sound as passed.
    assert!(qa::write(&d, &[report(true, false)], &[], &[], true).unwrap());
    assert!(d.join("listening.html").is_file());
    assert!(!qa::write(&d, &[report(false, false)], &[], &[], false).unwrap());
    assert!(!d.join("listening.html").exists(), "no stale page");
}

/// Every committed Source's "what it is" facts.
fn committed_facts() -> Vec<(String, String)> {
    build::sources(std::path::Path::new(SOURCES))
        .unwrap()
        .iter()
        .filter(|s| !s.placeholder)
        .flat_map(|s| {
            about::of(s)
                .unwrap()
                .into_iter()
                .flat_map(|(label, lines)| std::iter::once(label.to_string()).chain(lines))
                .map(|l| (s.name.as_str().to_string(), l))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn no_fact_falls_back_to_a_question_mark() {
    for (name, l) in committed_facts() {
        assert!(!l.contains('?'), "{name}: {l}");
    }
}

/// No short word leaks onto the page: every word of every committed
/// Sound's facts is a full word (or a number, unit or the unit's own
/// name for a thing: 2040, LP4, REV2, T5), and none is a short word the
/// tables spell out.
#[test]
fn facts_use_full_words() {
    const KNOWN: &[&str] = &[
        "ENGINE",
        "VIRTUAL",
        "ANALOG",
        "MODAL",
        "FM",
        "ALGO",
        "FILTER",
        "MODULATED",
        "WAVE",
        "MODE",
        "MIX",
        "SYNC",
        "RING",
        "BITS",
        "XOR",
        "BLEND",
        "SUB",
        "STABILITY",
        "OSC",
        "COARSE",
        "FINE",
        "SHAPE",
        "CUTOFF",
        "RESONANCE",
        "FREQUENCY",
        "LEVEL",
        "CLEAN",
        "LADDER",
        "DIRTY",
        "LP4",
        "REV2",
        "EXP",
        "ENV",
        "LFO",
        "ATTACK",
        "DECAY",
        "SUSTAIN",
        "RELEASE",
        "HOLD",
        "RISE",
        "FALL",
        "LINEAR",
        "LOGARITHMIC",
        "EXPONENTIAL",
        "SINE",
        "TRIANGLE",
        "SAW",
        "SQUARE",
        "RANDOM",
        "RATE",
        "RETRIG",
        "FUNC",
        "VCA",
        "FOLD",
        "VELOCITY",
        "NOTE",
        "PITCH",
        "OUT",
        "OP",
        "AUDIO",
        "RATIO",
        "OFF",
        "CARRIERS",
        "MODULATORS",
        "ALGORITHM",
        "MORPH",
        "TOWARD",
        "MODEL",
        "EXCITER",
        "STRING",
        "BANK",
        "BOWED",
        "SYMPATHETIC",
        "PLUCK",
        "STRIKE",
        "BOW",
        "EXCITE",
        "COLOR",
        "BURST",
        "FORCE",
        "SPEED",
        "STRUCTURE",
        "BRIGHTNESS",
        "DAMPING",
        "POSITION",
        "MONO",
        "PART",
        "GLIDE",
        "OVERLAPPING",
        "NOTES",
        "SLIDE",
        "THE",
        "OSCILLATORS",
        "DRIFT",
        "HOLDS",
        "THEM",
        "STEADY",
        "SHARP",
        "HALF",
        "CYCLE",
        "DOUBLED",
        "PULSE",
        "TILTED",
        "ROUNDED",
        "SOFT",
        "MS",
        "S",
        "HZ",
        "KHZ",
        "T1",
        "T2",
        "T3",
        "T4",
        "T5",
        "T6",
        "T7",
        "T8",
    ];
    let mut odd = std::collections::BTreeSet::new();
    for (name, l) in committed_facts() {
        for w in l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            let w = w.trim_matches('.');
            let alg =
                w.len() > 1 && w.starts_with('A') && w[1..].bytes().all(|b| b.is_ascii_digit());
            let number = w.parse::<f32>().is_ok();
            if w.is_empty() || number || alg || w == "2040" {
                continue;
            }
            let short = about::WORDS.iter().chain(&about::VALUES).any(|x| x.0 == w);
            if short || !KNOWN.contains(&w.to_uppercase().as_str()) {
                odd.insert(format!("{w} ({name})"));
            }
        }
    }
    assert!(odd.is_empty(), "short or unknown words: {odd:?}");
}

/// The "what it is" lines, flattened.
fn about_text(text: &str) -> String {
    let s = &run(text).unwrap()[0];
    about::of(s)
        .unwrap()
        .into_iter()
        .map(|(label, lines)| format!("{label}: {}", lines.join(" | ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn what_it_is_names_a_va_sounds_wave_mode_and_filter() {
    let t = about_text(&format!(
        "{BASE}VA.MODE = \"RING\"\nVA.WAVE1 = \"0.4\"\nFILTER.KIND = \"2040\"\nFILTER.RESO = \"0.3\"\nVA.STAB = \"0.6\"\nmod.VEL = \"FOLDER.FOLD +12\"\nENV2.ATK = \"0.3\"\nmod.ENV2 = \"VA.WAVE1 +48\"\n"
    ));
    assert!(t.contains("ENGINE: VIRTUAL ANALOG"), "{t}");
    assert!(t.contains("WAVE 1 51 · WAVE 2"), "{t}");
    assert!(
        t.contains("MODE RING") && t.contains("BLEND") && t.contains("STABILITY 76"),
        "{t}"
    );
    assert!(
        t.contains("FILTER: 2040 · LP4 · REV2 · CUTOFF 1.20 kHz · RESONANCE 38"),
        "{t}"
    );
    // Each route in plain words, its modulator as the unit shows it.
    assert!(
        t.contains("LFO 1 (SINE, RATE 6 = 1 Hz) → CUTOFF +20"),
        "{t}"
    );
    assert!(t.contains("ENV 2 (ATTACK 26 ms"), "{t}");
    assert!(t.contains(") → WAVE 1 +48"), "{t}");
    assert!(
        about_text(&format!("{BASE}mod.LFO2 = \"VA.FINE2 +10\"\n")).contains("→ OSC 2 FINE +10"),
        "an oscillator's cell by its OSC page"
    );
    assert!(t.contains("VELOCITY → FOLD +12"), "{t}");
    assert!(t.contains("→ VCA +127"), "{t}");
    assert!(t.contains("STABILITY 76: the oscillators drift"), "{t}");
}

#[test]
fn what_it_is_lists_an_algo_sounds_carriers() {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    let t = about_text(&format!(
        "{algo}ALGO.ALG_A = \"T5\"\nALGO.ALG_B = \"T5\"\nOP_C.LEVEL = \"0\"\n"
    ));
    assert!(t.contains("ENGINE: FM ALGO"), "{t}");
    assert!(t.contains("ALGORITHM T5"), "{t}");
    assert!(
        t.contains("CARRIERS: OP 1 ") && t.contains("OP 3 (OFF)") && t.contains(", OP 5 "),
        "{t}"
    );
    assert!(t.contains("MODULATORS: OP 2 → OP 1 "), "{t}");
    assert!(t.contains("OP 6 → OP 5 "), "{t}");
}

#[test]
fn a_tap_route_reads_audio_rate() {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    let t = about_text(&format!("{algo}tap.OP5 = \"FILTER.CUTOFF +45\"\n"));
    assert!(
        t.contains("OP 5 → FILTER FREQUENCY +45 (audio rate)"),
        "{t}"
    );
}

#[test]
fn what_it_is_names_a_modal_sounds_model_and_exciter() {
    let modal = BASE
        .replace("engine = \"VA\"", "engine = \"MODAL\"")
        .replace("mod.ENV1 = \"OUT.VCA +127\"\n", "");
    let t = about_text(&format!(
        "{modal}MODAL.MODE = \"SYMPATHETIC\"\nMODAL.EXCITE = \"0.4\"\nmod.NOTE = \"MODAL.POS +40\"\n"
    ));
    assert!(t.contains("ENGINE: MODAL"), "{t}");
    assert!(
        t.contains("MODEL SYMPATHETIC · EXCITER PLUCK: EXCITE 51"),
        "{t}"
    );
    assert!(t.contains("NOTE → POSITION +40"), "{t}");
}

#[test]
fn glide_and_mono_read_as_modulation() {
    let t = about_text(&format!(
        "{}PITCH.STEAL = \"GLIDE\"\nPITCH.GLIDE_TIME = \"0.5765\"\n",
        BASE.replace("demo = \"poly\"", "demo = \"mono\"")
    ));
    assert!(t.contains("MONO PART, GLIDE 80 ms"), "{t}");
}

#[test]
fn badges_follow_the_source_and_a_placeholder_has_no_players() {
    let approved = &run(&format!("{BASE}approved = \"round 2\"\n")).unwrap()[0];
    assert_eq!(approved.badge(), source::Badge::Approved);
    assert_eq!(approved.approved.as_deref(), Some("round 2"));
    let new = &run(BASE).unwrap()[0];
    assert_eq!(new.badge(), source::Badge::New);
    let ph = &source::parse(
        &(BASE
            .replace("S9100002", "S9100028")
            .replace("TEST PAD", "VOWEL CHOIR")
            .replace("A TEST PAD: SLOW FILTER", "FORMANT CHOIR: COMING SOON")
            + "status = \"placeholder\"\n"),
        "p.toml",
    )
    .unwrap()[0];
    assert_eq!(ph.badge(), source::Badge::Placeholder);
    fails(
        &format!("{BASE}status = \"placeholder\"\napproved = \"round 2\"\n"),
        2,
        "a placeholder has no Sound to approve",
    );
    fails(&format!("{BASE}approved = \"\"\n"), 14, "approved");

    let mut a = report(true, false);
    a.badge = source::Badge::Approved;
    a.name = "OLD PAD".into();
    let page = qa::page(&[report(true, false), a], &[], std::slice::from_ref(ph));
    let at = |w: &str| page.find(w).unwrap_or_else(|| panic!("{w}: {page}"));
    assert!(at("OLD PAD") < at("TEST PAD"), "approved first");
    assert!(at("TEST PAD") < at("VOWEL CHOIR"), "placeholders last");
    assert!(
        page.contains(">NEW<") && page.contains(">PLACEHOLDER<"),
        "{page}"
    );
    let tail = &page[at("VOWEL CHOIR")..];
    assert!(tail.contains("waits on the formant engine"), "{page}");
    assert!(!tail.contains("<audio"), "a placeholder has no players");
    assert!(
        page.contains("ENGINE") && page.contains("VIRTUAL ANALOG"),
        "what it is"
    );
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
    assert!(qa::write("/tmp/showcase-qa".as_ref(), &[], &[], &[], false).is_err());
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
    // A trim that rounds to nothing is written 0.0, never -0.0.
    assert_eq!(
        format!("{:.1}", qa::level_trim(&s[0], -10.98).unwrap()),
        "0.0"
    );
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
    let heard: Vec<Audition> = r.renders.iter().map(|x| x.audition).collect();
    assert_eq!(
        heard,
        [
            Audition::Phrase(Phrase::Pad),
            Audition::Script(Script::Long),
            Audition::Script(Script::Texture)
        ],
        "the phrase first"
    );
    let phrase = &r.renders[0];
    assert_eq!(
        phrase.stereo.len(),
        2 * (11 * 48_000),
        "8 s of phrase + 3 s"
    );
    assert!(phrase.limited_db.is_some(), "its limiter always shown");
    let has = |c: Check| r.gates.iter().any(|g| g.check == c);
    assert!(has(Check::Loudness) && has(Check::Tail) && has(Check::Limiter(None)));
    assert!(has(Check::Clicks(Script::Texture)) && has(Check::Limiter(Some(Script::Texture))));
    assert!(
        has(Check::Aliasing(84)) && has(Check::Aliasing(96)),
        "VA is harmonic"
    );
    assert!(
        has(Check::Dc(100)) && has(Check::Dc(127)),
        "DC on the held note"
    );
    assert!(
        has(Check::Finite(Phrase::Pad)),
        "a NaN phrase fails the run"
    );
    assert!(!has(Check::Tap), "no tap routes");
}

// ---- Sends (ADR 0123) and ENV 1's attack ----

/// RMS of `x[a..b]`.
fn rms(x: &[f32], a: usize, b: usize) -> f32 {
    (x[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt()
}

#[test]
fn sends_parse_by_frozen_idents() {
    let t = format!("{BASE}OUT.SEND1 = \"0.5\"\nOUT.SEND2 = \"0.25\"\nOUT.SEND3 = \"1\"\n");
    let s = run(&t).unwrap();
    let snd = source::to_sound(&s[0]).unwrap();
    assert_eq!(snd.params.out.sends, [0.5, 0.25, 1.0]);
}

#[test]
fn listening_renders_are_wet_and_gates_dry() {
    let wet_src = &source::parse(&format!("{BASE}OUT.SEND1 = \"0.8\"\n"), "w.toml").unwrap()[0];
    let dry_src = &source::parse(BASE, "d.toml").unwrap()[0];
    // The listening render plays the Sound's sends through NEW's FX bus.
    let wet = qa::render(wet_src, Script::Long).unwrap();
    assert!(
        wet != qa::dry(wet_src, Script::Long).unwrap(),
        "CHORUS is heard"
    );
    assert!(
        qa::dry(wet_src, Script::Long).unwrap() == qa::render(dry_src, Script::Long).unwrap(),
        "dry is the Sound with its sends at 0"
    );
    // The gates measure dry: the same values as the Sound without sends.
    let (gw, gd) = (qa::gates(wet_src).unwrap(), qa::gates(dry_src).unwrap());
    assert_eq!(gw.gates, gd.gates);
    // The page marks the wet render.
    assert_eq!(gw.renders[0].with, ["CHORUS"]);
    assert!(gd.renders[0].with.is_empty());
    // On the page whatever this unlevelled Sound's loudness reads.
    let mut gw = gw;
    gw.gates.retain(|g| g.verdict != Verdict::Fail);
    let page = qa::page(&[gw], &[], &[]);
    assert!(page.contains("LONG WITH CHORUS"), "full words");
    let all = &source::parse(
        &format!("{BASE}OUT.SEND1 = \"0.1\"\nOUT.SEND2 = \"0.1\"\nOUT.SEND3 = \"0.1\"\n"),
        "a.toml",
    )
    .unwrap()[0];
    assert_eq!(
        qa::gates(all).unwrap().renders[0].with,
        ["CHORUS", "DELAY", "REVERB"]
    );
}

/// The VA author's report, "ENV1.ATK had no effect": a Source starts from
/// `Sound::neutral`, which routes nothing into the VCA (INIT routes ENV 1
/// at +127). With only VEL → VCA the VCA's gain is VEL's, flat while the
/// key is held, so ENV 1 shapes nothing; with ENV 1 → VCA its attack does.
#[test]
fn env1_attack_shapes_the_level_only_through_a_vca_route() {
    let early = |extra: &str| {
        let bare = BASE.replace("mod.ENV1 = \"OUT.VCA +127\"\n", "");
        let s = &source::parse(&format!("{bare}{extra}"), "e.toml").unwrap()[0];
        let x = qa::dry(s, Script::Long).unwrap();
        rms(&x, 2_400, 14_400) // 50-300 ms
    };
    let vel = "mod.VEL = \"OUT.VCA +40\"\n";
    assert_eq!(
        early(vel),
        early(&format!("{vel}ENV1.ATK = \"0.9\"\n")),
        "no route: no effect"
    );
    let env = "mod.ENV1 = \"OUT.VCA +127\"\n";
    let (fast, slow) = (early(env), early(&format!("{env}ENV1.ATK = \"0.9\"\n")));
    assert!(
        slow < fast * 0.25,
        "routed: a 4 s attack is quieter early ({slow} vs {fast})"
    );
}

#[test]
fn va_without_vca_envelope_fails() {
    // Drop the VCA envelope (line 12): the id line is named.
    let none: String = BASE
        .lines()
        .filter(|l| !l.starts_with("mod.ENV1"))
        .collect::<Vec<_>>()
        .join("\n");
    fails(&none, 2, "OUT.VCA");
    // Velocity alone on the VCA is still organ-gated; so is a zero amount.
    fails(
        &none.replace("trim = \"0\"", "mod.VEL = \"OUT.VCA +40\""),
        2,
        "OUT.VCA",
    );
    fails(&base_with(12, r#"mod.ENV1 = "OUT.VCA +0""#), 2, "OUT.VCA");
}

#[test]
fn va_with_vca_envelope_passes_and_others_are_exempt() {
    run(BASE).unwrap();
    run(&base_with(12, r#"mod.ENV2 = "OUT.VCA +90""#)).unwrap();
    // ALGO and MODAL own their amplitude; a placeholder is not built.
    let no_env = |extra: &str| {
        BASE.lines()
            .filter(|l| !l.starts_with("mod.ENV1"))
            .chain(std::iter::once(extra))
            .collect::<Vec<_>>()
            .join("\n")
    };
    run(&no_env("").replace("engine = \"VA\"", "engine = \"ALGO\"")).unwrap();
    run(&no_env("").replace("engine = \"VA\"", "engine = \"MODAL\"")).unwrap();
    run(&no_env("status = \"placeholder\"")).unwrap();
}

/// `fmt --level` centres and trim-checks the dry Sound, as the gates
/// measure it: sends change nothing.
#[test]
fn level_lufs_is_dry() {
    let wet = &source::parse(&format!("{BASE}OUT.SEND3 = \"1\"\n"), "w.toml").unwrap()[0];
    let dry = &source::parse(BASE, "d.toml").unwrap()[0];
    assert_eq!(qa::level_lufs(wet).unwrap(), qa::level_lufs(dry).unwrap());
}

/// Interleaved stereo from two channels.
fn stereo(l: &[f32], r: &[f32]) -> Vec<f32> {
    l.iter().zip(r).flat_map(|(&a, &b)| [a, b]).collect()
}

/// DC is judged against the signal over the held part, per channel: a
/// quieter Sound, or more tail silence, never hides it.
#[test]
fn dc_gate_is_relative_per_channel_over_the_held_part() {
    let n = 48_000;
    let s = sine(220.0, 0.3, 1.0);
    let clean = stereo(&s, &s);
    assert_eq!(qa::dc(&clean, 0, n).1, Verdict::Pass);
    // 5 % of the RMS (-26 dB) on the left only.
    let off = 0.05 * 0.3 / 2f32.sqrt();
    let left: Vec<f32> = s.iter().map(|v| v + off).collect();
    let bad = stereo(&left, &s);
    let (v, verdict) = qa::dc(&bad, 0, n);
    assert_eq!(verdict, Verdict::Fail, "{v:.1} dB");
    assert!((v + 26.0).abs() < 0.5, "{v:.1}");
    // Turned down 40 dB: the same verdict (the old absolute -60 dBFS passed it).
    let quiet: Vec<f32> = bad.iter().map(|v| v * 0.01).collect();
    assert_eq!(qa::dc(&quiet, 0, n).1, Verdict::Fail);
    // Ten seconds of silence after it: judged over the held part only.
    let mut long = bad.clone();
    long.resize(bad.len() + 2 * 480_000, 0.0);
    assert_eq!(qa::dc(&long, 0, n).1, Verdict::Fail);
    // Under the bar passes.
    let ok: Vec<f32> = s.iter().map(|v| v + 0.1 * off).collect();
    assert_eq!(qa::dc(&stereo(&ok, &s), 0, n).1, Verdict::Pass);
}

#[test]
fn listening_page_is_stereo_and_honest() {
    let d = scratch_ledger("ledger-stereo");
    let mut r = report(true, false);
    r.gates.push(Gate {
        check: Check::Tail,
        value: -240.0,
        verdict: Verdict::Pass,
    });
    // A wet render the limiter worked past the single-note bar.
    r.renders.push(qa::Render {
        audition: Audition::Script(Script::Texture),
        stereo: stereo(&sine(220.0, 0.2, 0.5), &sine(220.0, 0.2, 0.5)),
        lufs: -10.0,
        peak_dbfs: [-3.0, -3.5],
        with: vec!["REVERB"],
        limited_db: Some(2.4),
    });
    assert!(qa::write(&d, &[r], &[], &[], true).unwrap());
    let w = hound::WavReader::open(d.join("renders/S9100002-long.wav")).unwrap();
    assert_eq!(w.spec().channels, 2, "the listening WAVs keep their width");
    assert_eq!(w.len(), 2 * 24_000);
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("peak L -14.0 · R -20.0 dBFS"), "per channel");
    assert!(page.contains("level (1 s, dry) -11.0 LUFS"));
    assert!(page.contains("long (dry) -12.3 LUFS"));
    assert!(page.contains("TEXTURE WITH REVERB"));
    assert!(page.contains("limiter 2.4 dB, over the 1 dB bar"), "{page}");
    assert!(page.contains("class=\"flag\""), "flagged visibly");
    assert!(page.contains("or is marked for your ear"), "honest intro");
    assert!(!page.contains("Every gate passed,"));
    let txt = std::fs::read_to_string(d.join("report.txt")).unwrap();
    assert!(txt.contains("silent") && !txt.contains("-240"), "{txt}");
}

/// A short low note: a partial cycle is not an offset, a real one is.
#[test]
fn held_dc_reads_whole_cycles_of_each_note() {
    let hit = qa::Hit {
        on: 0.0,
        off: 0.08,
        note: 36,
        vel: 100,
    };
    // The render is a block late (the limiter's lookahead).
    let note = |dc: f32| {
        let mut x = vec![0.0; 64];
        x.extend(sine(hz(60) * 1.07, 0.3, 0.08).iter().map(|v| v + dc));
        x.resize(48_000, 0.0);
        stereo(&x, &x)
    };
    let clean = note(0.0);
    let span = &clean
        .iter()
        .step_by(2)
        .skip(64)
        .take(3_840)
        .copied()
        .collect::<Vec<f32>>();
    let mean = span.iter().sum::<f32>() / 3_840.0;
    assert!(
        20.0 * (mean.abs() / (0.3 / 2f32.sqrt())).log10() > -40.0,
        "the plain mean is fooled"
    );
    let (v, verdict) = qa::held_dc(&clean, &[hit]);
    assert_eq!(verdict, Verdict::Pass, "{v:.1} dB");
    let (v, verdict) = qa::held_dc(&note(0.05 * 0.3 / 2f32.sqrt()), &[hit]);
    assert_eq!(verdict, Verdict::Fail, "{v:.1} dB");
}

// ---- Phrase auditions ----

/// Every phrase lies within its bars (four; `sweep` 8 to 16), each note
/// sounding, and stays inside MIDI from any plausible home note (C1 to C6).
#[test]
fn phrases_fit_their_bars_and_midi() {
    assert_eq!(qa::phrase_steps(Phrase::Bass), qa::PHRASE_STEPS);
    assert!((8 * 16..=16 * 16).contains(&(qa::phrase_steps(Phrase::Sweep) as u16)));
    for p in Phrase::ALL {
        let notes = qa::phrase_notes(p);
        let steps = qa::phrase_steps(p);
        assert!(!notes.is_empty(), "{p:?}");
        for x in notes {
            assert!(
                x.len >= 1 && x.at as u16 + x.len as u16 <= steps as u16,
                "{p:?} {x:?}"
            );
            assert!((1..=127).contains(&x.vel), "{p:?} {x:?}");
            for home in 24i16..=84 {
                let m = home + x.step as i16;
                assert!((0..=127).contains(&m), "{p:?} {x:?} from {home}");
            }
        }
        let plan = qa::phrase_plan(p, 36);
        assert_eq!(plan.hits.len(), notes.len());
        assert_eq!(plan.mode, None, "the Sound's demo mode, so MONO glides");
        for (h, x) in plan.hits.iter().zip(notes) {
            assert_eq!(h.note as i16, 36 + x.step as i16);
            assert!((h.on - x.at as f32 * qa::SIXTEENTH_S).abs() < 1e-6);
            assert!((h.off - (x.at + x.len) as f32 * qa::SIXTEENTH_S).abs() < 1e-6);
            assert_eq!(h.vel, x.vel);
        }
        let end = plan.hits.iter().map(|h| h.off).fold(0f32, f32::max);
        let bars = steps as f32 * qa::SIXTEENTH_S;
        assert!(
            (bars - 1.0..=bars).contains(&end),
            "{p:?}: its bars at 120 BPM: {end}"
        );
        assert_eq!(qa::phrase_secs(p), end);
        assert!(plan.secs > end, "a tail after it");
    }
}

/// A note held past the next one's start: on a MONO Part, a glide.
fn overlaps(p: Phrase) -> usize {
    let n = qa::phrase_notes(p);
    n.iter()
        .zip(&n[1..])
        .filter(|(a, b)| b.at > a.at && b.at < a.at + a.len)
        .count()
}

#[test]
fn phrases_carry_what_they_show() {
    assert!(overlaps(Phrase::Bass) >= 1, "legato for the glide");
    let vels: Vec<u8> = qa::phrase_notes(Phrase::Bass)
        .iter()
        .map(|x| x.vel)
        .collect();
    assert!(
        vels.iter().max().unwrap() - vels.iter().min().unwrap() >= 40,
        "velocity routes are heard"
    );
    let bass = qa::phrase_notes(Phrase::Bass);
    assert!(
        bass.iter()
            .zip(&bass[1..])
            .any(|(a, b)| (a.step - b.step).abs() >= 12),
        "octave jumps"
    );
    let lead = qa::phrase_notes(Phrase::Lead);
    assert!(overlaps(Phrase::Lead) >= 2, "a couple of slides");
    assert!(lead.iter().any(|x| x.len >= 8), "a held note for vibrato");
    assert!(lead.iter().any(|x| x.len <= 2), "and short ones");
    // `wonky`: a melodic bassline with odd accents and a couple of slides.
    let wonky = qa::phrase_notes(Phrase::Wonky);
    assert!(overlaps(Phrase::Wonky) >= 2, "a couple of legato slides");
    let wv: Vec<u8> = wonky.iter().map(|x| x.vel).collect();
    assert!(
        wv.iter().max().unwrap() - wv.iter().min().unwrap() >= 40,
        "odd accents"
    );
    assert!(
        wonky.iter().any(|x| x.at % 4 != 0 && x.vel >= 110),
        "an accent off the beat"
    );
    // `arp`: syncopated, and clear rather than dense.
    let arp = qa::phrase_notes(Phrase::Arp);
    assert!(
        arp.iter().filter(|x| x.at % 2 == 1).count() >= 6,
        "onsets on the off-sixteenths"
    );
    assert!(
        arp.iter()
            .any(|x| x.at % 4 != 0 && (x.at / 4) != ((x.at + x.len - 1) / 4)),
        "a note pushed off the beat and tied across it"
    );
    assert!(
        (0..qa::PHRASE_STEPS)
            .step_by(16)
            .any(|b| arp.iter().all(|x| x.at != b)),
        "a rest on a bar's downbeat"
    );
    assert!(arp.len() <= 40, "clear, not dense: {}", arp.len());
    // `sweep`: one keys figure repeated, so only the filter moves.
    let sw = qa::phrase_notes(Phrase::Sweep);
    let fig = &qa::SWEEP_FIGURE;
    assert!(
        fig.iter().all(|x| x.at + x.len <= qa::SWEEP_FIGURE_STEPS),
        "the figure fits its bars"
    );
    assert_eq!(sw.len() % fig.len(), 0);
    assert!(
        sw.len() / fig.len() >= 4,
        "repeated: {}",
        sw.len() / fig.len()
    );
    for (k, x) in sw.iter().enumerate() {
        let f = fig[k % fig.len()];
        let shift = (k / fig.len()) as u8 * qa::SWEEP_FIGURE_STEPS;
        assert_eq!(
            *x,
            PhraseNote {
                at: f.at + shift,
                ..f
            },
            "note {k}"
        );
    }
    // Stabs: three or more notes struck together.
    assert!(
        fig.iter()
            .any(|a| fig.iter().filter(|b| b.at == a.at).count() >= 3),
        "chord stabs"
    );
    let sv: Vec<u8> = fig.iter().map(|x| x.vel).collect();
    assert!(
        sv.iter().max().unwrap() - sv.iter().min().unwrap() >= 30,
        "accents for the velocity routes"
    );
    // Every pitch is struck on the first downbeat and re-struck while it
    // still rings (within half a second), so the voice is taken back and
    // its free-running LFO stays in phase with the others: one sweep, not
    // a smear of voices at different cutoffs.
    for x in sw {
        assert!(sw.iter().any(|y| y.at == 0 && y.step == x.step), "{x:?}");
        if let Some(next) = sw.iter().find(|y| y.step == x.step && y.at > x.at) {
            let rest = next.at as i16 - (x.at + x.len) as i16;
            assert!(rest <= 4, "{x:?} rests too long");
        }
    }
    let mut chords: Vec<u8> = qa::phrase_notes(Phrase::Pad).iter().map(|x| x.at).collect();
    chords.dedup();
    assert!((2..=4).contains(&chords.len()), "{chords:?}");
    assert!(
        qa::phrase_notes(Phrase::Pad).iter().all(|x| x.len >= 8),
        "held chords"
    );
}

#[test]
fn phrase_parses_and_a_va_sound_needs_one() {
    assert_eq!(run(BASE).unwrap()[0].phrase, Some(Phrase::Pad));
    for p in Phrase::ALL {
        // A slow LFO 1 on the cutoff, as `sweep` needs.
        let with = format!("phrase = \"{}\"\nLFO1.RATE = \"0.05\"", p.word());
        let s = run(&base_with(13, &with)).unwrap();
        assert_eq!(s[0].phrase, Some(p));
    }
    fails(&base_with(13, r#"phrase = "jazz""#), 13, "phrase jazz");
    // A VA Sound without one fails on its id line.
    let none = BASE.replace("phrase = \"pad\"\n", "");
    fails(&none, 2, "phrase");
    // ALGO and MODAL may use any or none; a placeholder is not built.
    run(&none.replace("engine = \"VA\"", "engine = \"ALGO\"")).unwrap();
    run(&none.replace("engine = \"VA\"", "engine = \"MODAL\"")).unwrap();
    run(&format!("{none}status = \"placeholder\"\n")).unwrap();
    let algo = base_with(4, r#"engine = "ALGO""#);
    assert_eq!(run(&algo).unwrap()[0].phrase, Some(Phrase::Pad));
}

/// A `sweep` Sound needs a smooth (SINE or TRIANGLE), free-running LFO on
/// the cutoff, no faster than `SWEEP_MAX_HZ`, whose whole cycle (one rise
/// and fall) the phrase plays through; else the build fails on its id line.
#[test]
fn a_sweep_needs_a_slow_smooth_lfo_its_phrase_holds() {
    let sweep = |extra: &str| format!("{}\n{extra}\n", base_with(13, r#"phrase = "sweep""#));
    // BASE's LFO 1 runs at the 1 Hz default: a wobble, not a sweep.
    fails(&sweep(""), 2, "sweep");
    run(&sweep(r#"LFO1.RATE = "0.05""#)).unwrap();
    run(&sweep("LFO1.RATE = \"0.1\"\nLFO1.SHAPE = \"TRIANGLE\"")).unwrap();
    // A cycle longer than the phrase never comes back down.
    let slow = 0.9 / qa::phrase_secs(Phrase::Sweep);
    fails(&sweep(&format!("LFO1.RATE = \"{slow}\"")), 2, "cycle");
    // Stepped, or restarted by each note.
    fails(
        &sweep("LFO1.RATE = \"0.05\"\nLFO1.SHAPE = \"SQUARE\""),
        2,
        "sweep",
    );
    fails(
        &sweep("LFO1.RATE = \"0.05\"\nLFO1.SYNC = \"RETRIG\""),
        2,
        "sweep",
    );
    // No LFO on the cutoff at all.
    let none = sweep(r#"LFO1.RATE = "0.05""#).replace("mod.LFO1 = \"FILTER.CUTOFF +20\"", "");
    fails(&none, 2, "sweep");
    // The committed sweep Sounds: the render holds a full cycle.
    for s in build::sources(std::path::Path::new(SOURCES)).unwrap() {
        if s.phrase != Some(Phrase::Sweep) || s.placeholder {
            continue;
        }
        let hz = source::sweep_hz(&s).unwrap();
        let plan = qa::phrase_plan(Phrase::Sweep, s.home);
        let notes_end = plan.hits.iter().map(|h| h.off).fold(0f32, f32::max);
        assert!(1.0 / hz <= notes_end, "{}: {hz} Hz", s.name.as_str());
        assert!(hz <= qa::SWEEP_MAX_HZ);
    }
}

#[test]
fn page_plays_the_phrase_first_and_flags_its_limiter() {
    let d = scratch_ledger("ledger-phrase");
    let phrase = |p: Phrase, limited: f32| qa::Render {
        audition: Audition::Phrase(p),
        stereo: stereo(&sine(220.0, 0.2, 0.5), &sine(220.0, 0.2, 0.5)),
        lufs: -12.0,
        peak_dbfs: [-6.0, -6.0],
        with: vec!["CHORUS"],
        limited_db: Some(limited),
    };
    let mut r = report(true, false);
    r.renders.insert(0, phrase(Phrase::Pad, 2.5));
    assert!(qa::write(&d, &[r.clone()], &[], &[], true).unwrap());
    assert!(d.join("renders/S9100002-phrase-pad.wav").is_file());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    let (ph, long) = (
        page.find("PAD PHRASE WITH CHORUS").expect("its label"),
        page.find(">LONG").expect("the long render"),
    );
    assert!(ph < long, "the phrase plays first");
    assert!(page.contains("limiter 2.5 dB") && !page.contains("over the"));
    // A pad over the 3 dB chord bar; a bass over the 1 dB bar.
    r.renders[0] = phrase(Phrase::Pad, 3.4);
    qa::write(&d, &[r.clone()], &[], &[], true).unwrap();
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("limiter 3.4 dB, over the 3 dB bar"), "{page}");
    r.renders[0] = phrase(Phrase::Bass, 1.4);
    qa::write(&d, &[r], &[], &[], true).unwrap();
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("BASS PHRASE WITH CHORUS"));
    assert!(page.contains("limiter 1.4 dB, over the 1 dB bar"), "{page}");
}

#[test]
fn a_phrase_that_is_not_finite_fails() {
    let g = |bad: f32| Gate {
        check: Check::Finite(Phrase::Lead),
        value: bad,
        verdict: if bad == 0.0 {
            Verdict::Pass
        } else {
            Verdict::Fail
        },
    };
    let mut r = report(true, false);
    r.gates.push(g(0.0));
    assert!(r.passes());
    r.gates.push(g(3.0));
    assert!(!r.passes());
    assert!(qa::report_text(&[r], &[]).contains("finite lead phrase"));
}

/// `harmonic` picks the aliasing gate: an ALGO Source defaults to exempt
/// (inharmonic ratios), and one marked `harmonic = "yes"` (integer-ratio
/// FM) runs it.
#[test]
fn harmonic_flag_picks_the_aliasing_gate() {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    let alias = |text: &str| {
        let s = &run(text).unwrap()[0];
        qa::gates(s)
            .unwrap()
            .gates
            .into_iter()
            .find(|g| g.check == Check::Aliasing(96))
            .expect("an aliasing line")
            .verdict
    };
    assert!(
        matches!(alias(&algo), Verdict::Exempt(why) if why.contains("inharmonic")),
        "ALGO is exempt by default"
    );
    let ran = alias(&format!("{algo}\nharmonic = \"yes\"\n"));
    assert!(
        !matches!(ran, Verdict::Exempt(_)),
        "marked harmonic: {ran:?}"
    );
    let va_no = alias(&format!("{BASE}harmonic = \"no\"\n"));
    assert!(matches!(va_no, Verdict::Exempt(_)), "VA marked inharmonic");
    fails(
        &format!("{BASE}harmonic = \"maybe\"\n"),
        14,
        "harmonic maybe",
    );
}
