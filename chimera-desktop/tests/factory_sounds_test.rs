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
