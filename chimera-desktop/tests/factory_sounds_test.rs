//! The factory Sounds' text Sources (spec § 3, ADR 0134): a golden Source
//! builds to committed bytes, `fmt` is idempotent, text → Sound → text is
//! identical, every committed Source builds, and each bad Source fails
//! with its line.

use chimera_core::block::{ParamId, ParamSpec};
use chimera_core::dsp::lfo::{LFO_SPECS, LfoParams};
use chimera_core::dsp::modulator::{Func, LfoForm};
use chimera_core::dsp::{Gain, SendLevel};
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
use chimera_desktop::factory_sounds::{about, build, run, source};
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
    chimera_desktop::factory_sounds::phrase::sweeps(&s)?;
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
    let once = source::fmt(&g, "golden.toml").unwrap();
    assert_eq!(source::fmt(&once, "golden.toml").unwrap(), once);
    // A display comment on a value line, ignored on read.
    let cut = once
        .lines()
        .find(|l| l.starts_with("FILTER.CUTOFF"))
        .unwrap();
    assert!(cut.contains("  # "), "{cut}");
    let crc = |t: &str| sound_crc(&source::to_sound(&source::parse(t, "g").unwrap()[0]).unwrap());
    assert_eq!(crc(&once), crc(&g));
}

/// Every committed Source is `fmt`'s canonical text: each raw value
/// carries its display.
#[test]
fn every_committed_source_is_canonical() {
    for f in build::SOUND_FILES {
        let text = std::fs::read_to_string(format!("{SOURCES}/{f}")).unwrap();
        let canon = source::fmt(&text, f).unwrap();
        let first = text.lines().zip(canon.lines()).position(|(a, b)| a != b);
        assert_eq!(
            canon, text,
            "{f}: line {first:?}: run `just factory-sounds fmt`"
        );
    }
}

/// An author's comment on a param line stays, after the display; a stale
/// display is refreshed, never kept beside the new one.
#[test]
fn fmt_keeps_author_comments() {
    let line = |t: &str| {
        let f = source::fmt(t, "t.toml").unwrap();
        assert_eq!(source::fmt(&f, "t.toml").unwrap(), f, "idempotent");
        f.lines().nth(8).unwrap().to_string()
    };
    let cut = |c: &str| base_with(9, &format!("FILTER.CUTOFF = \"1200\"{c}"));
    assert_eq!(
        line(&cut("  # opens on the swell")),
        "FILTER.CUTOFF = \"1200\"  # 1.20 kHz  # opens on the swell"
    );
    assert_eq!(
        line(&cut("  # 2.40 kHz  # opens")),
        "FILTER.CUTOFF = \"1200\"  # 1.20 kHz  # opens"
    );
    assert_eq!(
        line(&cut("  # 2.40 kHz")),
        "FILTER.CUTOFF = \"1200\"  # 1.20 kHz"
    );
    assert_eq!(line(&cut("")), "FILTER.CUTOFF = \"1200\"  # 1.20 kHz");
}

/// A Source whose tables don't parse, or whose Sound doesn't build, is
/// left as written: `fmt` names the line and strips nothing.
#[test]
fn fmt_refuses_a_source_that_does_not_build() {
    let e = source::fmt(&base_with(9, r#"FILTER.CUTOFF = "1e9""#), "bad.toml").unwrap_err();
    assert!(e.contains("bad.toml:9:"), "{e}");
    let e = source::fmt(&base_with(9, "FILTER.CUTOFF = 1200"), "bad.toml").unwrap_err();
    assert!(e.contains("bad.toml:9:"), "{e}");
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
    chimera_desktop::factory_sounds::phrase::sweeps(&s).unwrap();
    source::public(&s).unwrap();
    let (files, man) = build::files(&s, &STAMP).unwrap();
    assert_eq!(files.len(), man.sounds.len());
    assert!(man.json().starts_with('{'));
}

/// MIDI map spec § 9, ADR 0250: every built Source carries its engine's
/// default map, and one whose LFO 1 moves something keeps CC 1 → LFO 1
/// DEPTH, the wheel it was voiced with.
#[test]
fn every_factory_source_has_the_default_map() {
    use chimera_core::midi_map::{Input, default_for, wheel_map_for};
    use chimera_core::modulation::{FixedCol, ModSource};
    let s = build::sources(std::path::Path::new(SOURCES)).unwrap();
    assert!(!s.is_empty());
    let cc1 = Input::cc(1).unwrap();
    let mut wheeled = 0;
    for x in &s {
        let snd = source::to_sound(x).unwrap();
        let (m, l) = (&snd.mod_state, ModSource::Lfo1.index());
        let lfo1 = (0..m.num_dests()).any(|d| m.amount(l, d) != 0)
            || FixedCol::ALL.iter().any(|&c| m.fixed_amount(l, c) != 0);
        let want = if lfo1 {
            wheel_map_for(x.engine)
        } else {
            default_for(x.engine)
        };
        assert!(snd.map == want, "{:?}", x.name);
        assert_eq!(
            snd.map.entries().iter().any(|b| b.input == cc1),
            lfo1,
            "{:?}",
            x.name
        );
        wheeled += usize::from(lfo1);
    }
    assert!(wheeled > 0, "some Sounds play the wheel");
}

/// Spec § 8: every committed description fits FACTORY's line as drawn.
#[test]
fn every_committed_description_fits_the_drawn_width() {
    use chimera_core::ui::browser::{NOTE_W, note_width};
    let s = build::sources(std::path::Path::new(SOURCES)).unwrap();
    let wide: Vec<_> = s
        .iter()
        .map(|s| (s.note.as_str(), note_width(s.note.as_str())))
        .filter(|&(_, w)| w > NOTE_W)
        .collect();
    assert!(wide.is_empty(), "past {NOTE_W} px: {wide:?}");
    assert!(!s.is_empty());
}

/// What the build used to take silently now fails at its line.
#[test]
fn another_engines_param_or_route_fails() {
    fails(&base_with(9, r#"ALGO.MORPH = "0.5""#), 9, "ALGO.MORPH");
    fails(
        &base_with(10, r#"mod.LFO1 = "MODAL.BRIGHT +20""#),
        10,
        "MODAL.BRIGHT",
    );
}

#[test]
fn a_destination_twice_in_one_line_fails() {
    let t = base_with(10, r#"mod.LFO1 = "FILTER.CUTOFF +20, FILTER.CUTOFF -5""#);
    fails(&t, 10, "FILTER.CUTOFF twice");
}

#[test]
fn an_amount_of_zero_fails() {
    fails(
        &base_with(10, r#"mod.LFO1 = "FILTER.CUTOFF 0""#),
        10,
        "FILTER.CUTOFF 0",
    );
}

#[test]
fn an_envelope_closing_the_vca_fails() {
    fails(
        &base_with(12, r#"mod.ENV1 = "OUT.VCA -127""#),
        12,
        "OUT.VCA -127",
    );
}

#[test]
fn a_home_note_is_spelled_as_the_unit_spells_it() {
    for bad in ["B#3", "E#3", "C+3", "C#+3", "C 3", "C"] {
        fails(&base_with(8, &format!("home = \"{bad}\"")), 8, bad);
    }
    for (good, n) in [("C3", 48), ("C#3", 49), ("B3", 59), ("C-1", 0), ("G9", 127)] {
        let s = source::parse(&base_with(8, &format!("home = \"{good}\"")), "t").unwrap();
        assert_eq!(s[0].home, n, "{good}");
    }
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
fn retired_ids_refused() {
    for id in ["S9100028", "S9100029", "S9100030", "S9100060"] {
        fails(&base_with(2, &format!("id = \"{id}\"")), 2, "retired");
    }
    run(&base_with(2, "id = \"S9100031\"")).unwrap();
}

#[test]
fn public_takes_port_ids() {
    // The ports ship public (ADR 0138): their ids are ordinary ids now.
    for id in ["S9100009", "S9100010", "S9100025", "S9100026"] {
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
    // An `edits` line alone is a port's hint too.
    fails(&format!("{BASE}edits = \"filter: 2040\"\n"), 14, "edits");
}

#[test]
fn placeholder_skipped() {
    let two = format!(
        "{BASE}\n{}status = \"placeholder\"\n",
        BASE.replace("S9100002", "S9100003")
            .replace("TEST PAD\"", "VOCAL TWO\"")
    );
    let s = run(&two).unwrap();
    assert_eq!(s[1].status, source::Status::Placeholder);
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
    let f = source::fmt(text, "t.toml").unwrap();
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

/// FORMANT, BW and VOWEL read their stored value, as the renderer
/// gives them (`ValFmt::LogHz`, `ValFmt::Vowel`).
#[test]
fn fmt_reads_formant_hz_and_the_vowel_letter() {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    let t = format!(
        "{algo}ALGO.VOWEL = \"25\"\nOP_E.WAVE = \"FRMT\"\nOP_E.FRMT = \"83\"\nOP_E.BW = \"96\"\n"
    );
    run(&t).unwrap();
    assert_eq!(comment(&t, "ALGO.VOWEL"), "O");
    assert_eq!(comment(&t, "OP_E.FRMT"), "1.10 kHz");
    assert_eq!(comment(&t, "OP_E.BW"), "320 Hz");
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
use chimera_desktop::factory_sounds::phrase::{self, Plan};
use chimera_desktop::factory_sounds::qa::measure::{self, Verdict};
use chimera_desktop::factory_sounds::qa::{
    self, Audition, Check, Gate, baseline, demo, page, render,
};
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

    let bass = phrase::plan(Script::Bass, 36);
    assert_eq!(notes(&bass), [36]);
    assert_eq!(held(&bass), [0.3]);
    assert_eq!(bass.secs, 0.3 + 1.5);
    assert!(v100(&bass));

    let stab = phrase::plan(Script::Stab, 36);
    assert_eq!(notes(&stab), [48, 52, 55, 59], "C3 E3 G3 B3");
    assert!(held(&stab).iter().all(|&h| h == 0.25) && v100(&stab));
    assert!(stab.hits.iter().all(|h| h.on == 0.0));

    let short = phrase::plan(Script::Short, 36);
    assert_eq!(notes(&short), [60], "C4");
    assert_eq!(held(&short), [0.08]);

    let long = phrase::plan(Script::Long, 48);
    assert_eq!(notes(&long), [48]);
    assert_eq!(held(&long), [2.0]);
    assert_eq!(long.secs, 4.0);

    let tex = phrase::plan(Script::Texture, 48);
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

    let glide = phrase::plan(Script::Glide, 60);
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
        assert_eq!(phrase::plan(s, 48).mode, None, "{s:?} plays the demo mode");
    }
}

#[test]
fn loudness_gate() {
    // Inside −11 ± 2 LU passes unpinned; a pin wins, so one drifted into
    // the band fails.
    for l in [-11.0, -9.0, -13.0] {
        assert_eq!(measure::loudness(l, None), Verdict::Pass, "{l}");
        assert_eq!(
            measure::loudness(l, Some((-16.2, "soft"))),
            Verdict::Fail,
            "{l}"
        );
    }
    for l in [-8.9, -13.1, -30.0, f32::NAN] {
        assert_eq!(measure::loudness(l, None), Verdict::Fail, "{l}");
    }
    // A pinned level holds within ±1 LU of its value, for the ear.
    for l in [-16.2, -15.3, -17.1] {
        assert!(
            matches!(
                measure::loudness(l, Some((-16.2, "a soft pad under the lead"))),
                Verdict::Exempt(why) if why.contains("by ear -16.2") && why.contains("soft pad")
            ),
            "{l}"
        );
    }
    // It fails once it moves.
    for l in [-15.1, -17.3, f32::NAN] {
        assert_eq!(
            measure::loudness(l, Some((-16.2, "soft"))),
            Verdict::Fail,
            "{l}"
        );
    }
}

#[test]
fn a_pinned_level_parses_and_one_inside_the_band_fails() {
    let s = &run(&format!("{BASE}level = \"by ear -16.2: a soft pad\"\n")).unwrap()[0];
    let pin = s.level.as_ref().unwrap();
    assert_eq!(
        (pin.line, pin.lufs, pin.why.as_str()),
        (14, -16.2, "a soft pad")
    );
    assert!(run(BASE).unwrap()[0].level.is_none());
    // The old unpinned form, a bad number, no reason.
    for bad in [
        "by ear: soft",
        "by ear x: soft",
        "by ear -16.2:",
        "by ear -16.2 soft",
    ] {
        fails(&format!("{BASE}level = \"{bad}\"\n"), 14, "level");
    }
    // Pinned inside the band: pointless, so drop it.
    for inside in ["-11.0", "-13.0", "-9.0", "-12.4"] {
        fails(
            &format!("{BASE}level = \"by ear {inside}: soft\"\n"),
            14,
            "drop",
        );
    }
    run(&format!("{BASE}level = \"by ear -13.1: soft\"\n")).unwrap();
}

#[test]
fn fmt_level_keeps_pinned_and_approved_levels() {
    let pinned = format!("{BASE}level = \"by ear -20.0: soft\"\n");
    let two = BASE
        .replace("S9100002", "S9100003")
        .replace("TEST PAD\"", "TEST TWO\"");
    let crc = source::approval_crc(&run(&two).unwrap()[0]).unwrap();
    let approved = format!("{two}approved = \"round 3 {crc:08X}\"\n");
    let free = BASE
        .replace("S9100002", "S9100004")
        .replace("TEST PAD\"", "TEST THREE\"")
        .replace("trim = \"0\"", "trim = \"-6\"");
    // Approved, but changed since: shown NEW, so centred.
    let stale = format!("{free}approved = \"round 3 00000000\"\n")
        .replace("S9100004", "S9100005")
        .replace("TEST THREE\"", "TEST FOUR\"");
    let text = format!("{pinned}\n{approved}\n{free}\n{stale}");
    let all = source::parse(&text, "b.toml").unwrap();
    let out = qa::levelled(&text, &all).unwrap();
    let got = source::parse(&out, "b.toml").unwrap();
    assert_eq!(got[0].trim, 0.0, "pinned: never re-centred");
    assert_eq!(got[0].level, all[0].level);
    assert_eq!(got[1].trim, 0.0, "approved: never re-centred");
    assert_ne!(got[2].trim, -6.0, "the free one is centred");
    assert_ne!(got[3].trim, -6.0, "a changed approval is centred");
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
        assert_eq!(measure::limiter(0.9, s), Verdict::Pass, "{s:?}");
        assert_eq!(measure::limiter(1.1, s), Verdict::Fail, "{s:?}");
    }
    // `stab`, the chord: 3 dB.
    assert_eq!(measure::limiter(2.9, Some(Script::Stab)), Verdict::Pass);
    assert_eq!(measure::limiter(3.1, Some(Script::Stab)), Verdict::Fail);
}

#[test]
fn click_and_finite_gate() {
    // A sine from a zero crossing: no click.
    let clean = sine(220.0, 0.3, 1.0);
    assert_eq!(measure::clicks(&clean, &[0], None).1, Verdict::Pass);
    let mut click = clean.clone();
    click[24_000] += 0.2;
    assert_eq!(
        measure::clicks(&click, &[0], None).1,
        Verdict::Fail,
        "a click"
    );
    let mut nan = clean.clone();
    nan[100] = f32::NAN;
    assert_eq!(
        measure::clicks(&nan, &[0], None).1,
        Verdict::Fail,
        "not finite"
    );
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
        let (db, v) = measure::aliasing(&saw, f0, true);
        assert_eq!(v, Verdict::Pass, "C{}: {db:.1} dB", n / 12 - 1);
        // An alias 30 dB down, between harmonics.
        let bad: Vec<f32> = saw
            .iter()
            .zip(sine(f0 * 1.37, 0.2 * 0.0316, 2.0))
            .map(|(a, b)| a + b)
            .collect();
        let (db, v) = measure::aliasing(&bad, f0, true);
        assert_eq!(v, Verdict::Fail, "{db:.1} dB");
        assert!(matches!(
            measure::aliasing(&bad, f0, false).1,
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
    assert_eq!(measure::tail(&x, 48_000, 2.0).1, Verdict::Pass);
    // A hum that never stops.
    let hum = sine(50.0, 0.01, 5.0);
    let (db, v) = measure::tail(&hum, 48_000, 2.0);
    assert_eq!(v, Verdict::Fail, "{db}");
    // Still ringing at the limit fails; a render too short to reach it too.
    assert_eq!(measure::tail(&x, 48_000, 0.5).1, Verdict::Fail);
    assert_eq!(measure::tail(&x[..96_000], 48_000, 2.0).1, Verdict::Fail);

    // MODAL keeps ringing: 10 s; anything else its release plus 2 s.
    let modal = source::parse(&BASE.replace("\"VA\"", "\"MODAL\""), "m.toml").unwrap();
    let snd = source::to_sound(&modal[0]).unwrap();
    assert_eq!(measure::tail_within(&modal[0], &snd), 10.0);
    let va = source::parse(BASE, "v.toml").unwrap();
    let snd = source::to_sound(&va[0]).unwrap();
    let w = measure::tail_within(&va[0], &snd);
    assert!(
        (2.0..3.0).contains(&w),
        "VA's neutral release (300 ms) + 2 s: {w}"
    );
    // The VCA follows ENV 2 too, its release 4.5 s (MED at 0.9): the
    // longest release routed to the VCA sets the limit.
    let env2 = format!("{BASE}ENV2.REL = \"0.9\"\nmod.ENV2 = \"OUT.VCA +127\"\n");
    let s = source::parse(&env2, "e.toml").unwrap();
    let snd = source::to_sound(&s[0]).unwrap();
    let w = measure::tail_within(&s[0], &snd);
    assert!((6.0..7.0).contains(&w), "ENV 2's 4.5 s + 2 s: {w}");
    // An ENV 2 not routed to the VCA does not count.
    let free = format!("{BASE}ENV2.REL = \"0.9\"\n");
    let s = source::parse(&free, "e.toml").unwrap();
    let snd = source::to_sound(&s[0]).unwrap();
    assert!(measure::tail_within(&s[0], &snd) < 3.0);
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
    let (ba, bd) = (measure::erb_bands(&a), measure::erb_bands(&dull));
    let err = measure::spectral_error(&ba, &bd);
    assert_eq!(measure::tap(err), Verdict::Pass, "{err:.1} dB");
    let same = measure::erb_bands(&a.iter().map(|v| v * 1.05).collect::<Vec<_>>());
    let err = measure::spectral_error(&ba, &same);
    assert_eq!(
        measure::tap(err),
        Verdict::Fail,
        "{err:.1} dB: the tap is inaudible"
    );
}

#[test]
fn samey_flags_pair_within_3db_on_32_erb() {
    let a = noise(0.3, 1.0);
    let b: Vec<f32> = a.iter().map(|v| v * 1.1).collect(); // +0.8 dB
    let c = sine(440.0, 0.3, 1.0);
    let bands: Vec<[f32; 32]> = [&a, &b, &c].iter().map(|x| measure::erb_bands(x)).collect();
    assert_eq!(bands[0].len(), measure::ERB_BANDS);
    assert_eq!(measure::samey(&bands), vec![(0, 1)]);
    // 4 dB louder everywhere is not samey.
    let d: Vec<f32> = a.iter().map(|v| v * 1.585).collect();
    assert!(measure::samey(&[bands[0], measure::erb_bands(&d)]).is_empty());
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
            voices: 1,
        }],
        long_lufs: Some(-12.3),
        bands: [0.0; 32],
        badge: source::Badge::New,
        changed: None,
        about: about::of(s).unwrap(),
        baseline: None,
        free_lfos: about::free_lfos(&source::to_sound(s).unwrap()),
        voices: 8,
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
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[], true).unwrap());
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

    // The public ledger refuses a port; ports page in their own ledger,
    // under PRIVATE, with their edits.
    let e = page::write(
        &d,
        &[report(true, false), report(true, true)],
        &[],
        &[],
        &[],
        true,
    )
    .unwrap_err();
    assert!(e.contains("S9100002") && e.contains("private"), "{e}");
    let pd = scratch_ledger("ledger-page-private");
    assert!(page::write_private(&pd, &[report(true, true)], &[], true).unwrap());
    let page = std::fs::read_to_string(pd.join("listening.html")).unwrap();
    assert!(page.contains("<h2>PRIVATE</h2>"), "{page}");
    assert!(
        page.contains("edits: filter: 2040 instead of open<") && !page.contains("not verified"),
        "{page}"
    );
    assert!(
        page::write_private(&pd, &[report(true, false)], &[], true).is_err(),
        "the private ledger holds ports only"
    );

    // One failing Sound: off the page, named in the report; the rest stay.
    let mut bad = report(false, false);
    bad.name = "BAD PAD".into();
    bad.id = source::parse(&BASE.replace("S9100002", "S9100003"), "b.toml").unwrap()[0].id;
    // An old render of it, from a run when it passed.
    std::fs::write(d.join("renders/S9100003-long.wav"), b"old").unwrap();
    assert!(!page::write(&d, &[report(true, false), bad], &[], &[], &[], true).unwrap());
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
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[], true).unwrap());
    assert!(d.join("listening.html").is_file());
    assert!(!page::write(&d, &[report(false, false)], &[], &[], &[], false).unwrap());
    assert!(!d.join("listening.html").exists(), "no stale page");
}

/// The page and its renders are this run's: a passing run without --wav
/// leaves no old page, a dropped Sound's WAVs go, and no Sounds at all
/// fails rather than passing.
#[test]
fn the_page_never_outlives_its_run() {
    let d = scratch_ledger("ledger-lifecycle");
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[], true).unwrap());
    // An orphan: the render of a Sound no Source holds any more.
    std::fs::write(d.join("renders/S9100099-long.wav"), b"old").unwrap();
    std::fs::write(d.join("renders/P0000009-1-hats.wav"), b"old").unwrap();
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[], true).unwrap());
    assert!(
        !d.join("renders/S9100099-long.wav").exists(),
        "orphan Sound WAV"
    );
    assert!(
        !d.join("renders/P0000009-1-hats.wav").exists(),
        "orphan demo WAV"
    );
    assert!(d.join("renders/S9100002-long.wav").is_file());
    // Passing, without --wav: the old page would show old renders.
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[], false).unwrap());
    assert!(!d.join("listening.html").exists(), "no stale page");
    let e = page::write(&d, &[], &[], &[], &[], true).unwrap_err();
    assert!(e.contains("no Sounds"), "{e}");
    let e = page::write_private(&d, &[], &[], true).unwrap_err();
    assert!(e.contains("no Sounds"), "{e}");
}

/// Every committed Source's "what it is" facts.
fn committed_facts() -> Vec<(String, String)> {
    build::sources(std::path::Path::new(SOURCES))
        .unwrap()
        .iter()
        .filter(|s| s.status != source::Status::Placeholder)
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
        "SEM",
        "LP4",
        "LP2",
        "BP2",
        "REV2",
        "EXPNDR",
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
        "FX",
        "AMOUNT",
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
        // The formant operators (ADR 0131): kinds, cells and slots.
        "FORMANT",
        "BANDWIDTH",
        "WIDTH",
        "NOISE",
        "VOWEL",
        "SLOT",
        "AUTO",
        "F1",
        "F2",
        "F3",
    ];
    // VOWEL's five vowels, by letter: a word only after VOWEL.
    const VOWELS: &[&str] = &["U", "O", "A", "E", "I"];
    let mut odd = std::collections::BTreeSet::new();
    for (name, l) in committed_facts() {
        let mut prev = "";
        for w in l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.')) {
            let w = w.trim_matches('.');
            if w.is_empty() {
                continue;
            }
            let after = std::mem::replace(&mut prev, w);
            let alg =
                w.len() > 1 && w.starts_with('A') && w[1..].bytes().all(|b| b.is_ascii_digit());
            let number = w.parse::<f32>().is_ok();
            let vowel = after == "VOWEL" && VOWELS.contains(&w);
            if number || alg || vowel || w == "2040" {
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
        "{BASE}VA.MODE = \"RING\"\nVA.WAVE1 = \"0.4\"\nFILTER.KIND = \"2040\"\nFILTER.RESO = \"0.3\"\nVA.STAB = \"0.6\"\nmod.VEL = \"VOICEFX.P1 +12\"\nENV2.ATK = \"0.3\"\nmod.ENV2 = \"VA.WAVE1 +48\"\n"
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
    assert!(t.contains("LFO 1 (SINE, RATE 1.00 Hz) → CUTOFF +20"), "{t}");
    assert!(t.contains("ENV 2 (ATTACK 26 ms"), "{t}");
    assert!(t.contains(") → WAVE 1 +48"), "{t}");
    assert!(
        about_text(&format!("{BASE}mod.LFO2 = \"VA.FINE2 +10\"\n")).contains("→ OSC 2 FINE +10"),
        "an oscillator's cell by its OSC page"
    );
    assert!(t.contains("VELOCITY → FX AMOUNT +12"), "{t}");
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

/// The renders play the unit's voice budget (rev V, the build's bus, as
/// `Instrument` admits): a Sound past its voice count steals or waits, so
/// eight held notes of a heavily routed three-FRMT Sound sound fewer than
/// eight, and its 4-note stab fewer than four, where the render budget of
/// old (`u32::MAX`) played them all.
#[test]
fn renders_steal_at_the_units_voice_budget() {
    use chimera_core::hw::MAX_VOICES;
    use chimera_desktop::factory_sounds::source::Script;
    let heavy = &heavy_frmt();
    let n = render::unit_voices(heavy).unwrap();
    assert!(
        n < MAX_VOICES,
        "eight held notes of a heavy three-FRMT Sound: {n}"
    );
    let stab = render::most_voices(heavy, Script::Stab).unwrap();
    assert!(
        stab < 4 && stab <= n,
        "its 4-note stab sounds {stab} of {n}"
    );
    // A plain VA pad (2040 REV2, 78 a voice since #642) fits seven on
    // rev V, one under the cap.
    assert_eq!(
        render::unit_voices(&run(BASE).unwrap()[0]).unwrap(),
        MAX_VOICES - 1
    );
}

/// A FRMT operator reads by its formant and width, and the head names
/// the VOWEL every slotted operator follows (ADR 0131).
#[test]
fn what_it_is_names_formant_operators_and_the_vowel() {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    let t = about_text(&format!(
        "{algo}ALGO.ALG_A = \"T5\"\nALGO.ALG_B = \"T5\"\nALGO.VOWEL = \"25\"\nOP_E.WAVE = \"FRMT\"\nOP_E.LEVEL = \"90\"\nOP_E.FRMT = \"69\"\nOP_E.BW = \"72\"\nOP_E.VSLOT = \"F2\"\nOP_C.WAVE = \"NOIS\"\nOP_C.FRMT = \"123\"\nOP_C.RES = \"40\"\nmod.LFO2 = \"ALGO.VOWEL +30, OP_E.BW +10\"\n"
    ));
    assert!(t.contains("ALGORITHM T5 · VOWEL O"), "{t}");
    assert!(
        t.contains("OP 5 (FORMANT 734 Hz, BANDWIDTH 160 Hz, SLOT F2, RATIO 1.00, LEVEL "),
        "{t}"
    );
    assert!(
        t.contains("→ VOWEL +30") && t.contains("OP 5 BANDWIDTH +10"),
        "{t}"
    );
    // A noise band reads by its band and its vowel slot (its centre
    // follows VOWEL), and RES when set; it has no pitch, so no RATIO.
    assert!(
        t.contains("OP 3 (NOISE 3.49 kHz, WIDTH 160 Hz, SLOT AUTO, RESONANCE 40, LEVEL "),
        "{t}"
    );
    let table = about_text(&format!("{algo}ALGO.ALG_A = \"T5\"\nALGO.ALG_B = \"T5\"\n"));
    assert!(!table.contains("VOWEL"), "no formant, no vowel: {table}");
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
    let crc = source::approval_crc(&run(BASE).unwrap()[0]).unwrap();
    let approved = &run(&format!("{BASE}approved = \"round 2 {crc:08X}\"\n")).unwrap()[0];
    assert_eq!(approved.badge(crc), source::Badge::Approved);
    let new = &run(BASE).unwrap()[0];
    assert_eq!(new.badge(crc), source::Badge::New);
    let ph = &source::parse(
        &(BASE
            .replace("S9100002", "S9100090")
            .replace("TEST PAD", "VOWEL CHOIR")
            .replace("A TEST PAD: SLOW FILTER", "FORMANT CHOIR: COMING SOON")
            + "status = \"placeholder\"\n"),
        "p.toml",
    )
    .unwrap()[0];
    assert_eq!(ph.badge(0), source::Badge::Placeholder);
    fails(
        &format!("{BASE}status = \"placeholder\"\napproved = \"round 2 {crc:08X}\"\n"),
        15,
        "a placeholder has no Sound to approve",
    );

    let mut a = report(true, false);
    a.badge = source::Badge::Approved;
    a.name = "OLD PAD".into();
    let page = page::page(
        &[report(true, false), a],
        &[],
        std::slice::from_ref(ph),
        &[],
    );
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
fn approval_binds_to_the_sounds_crc() {
    use source::{Badge, Round, Status};
    let crc = source::approval_crc(&run(BASE).unwrap()[0]).unwrap();
    let line = format!("approved = \"round 2 {crc:08X}\"\n");
    let ok = &run(&format!("{BASE}{line}")).unwrap()[0];
    assert_eq!(
        ok.status,
        Status::Approved {
            round: Round(2),
            crc
        }
    );
    assert_eq!(
        source::approval_crc(ok).unwrap(),
        crc,
        "approved adds nothing to the Sound"
    );
    assert_eq!((ok.badge(crc), ok.changed(crc)), (Badge::Approved, None));

    // One param edited since: still builds, demoted to NEW and flagged.
    let edited = format!("{BASE}{line}").replace("\"1200\"", "\"1300\"");
    let ed = &run(&edited).unwrap()[0];
    let now = source::approval_crc(ed).unwrap();
    assert_ne!(now, crc);
    assert_eq!(
        (ed.badge(now), ed.changed(now)),
        (Badge::New, Some(Round(2)))
    );
    let mut r = qa::gates(ed).unwrap();
    assert_eq!((r.badge, r.changed), (Badge::New, Some(Round(2))));
    // BASE is not levelled: only a passing Sound is paged.
    r.gates.retain(|g| g.verdict != Verdict::Fail);
    let page = page::page(std::slice::from_ref(&r), &[], &[], &[]);
    assert!(
        page.contains("class=\"flag\">CHANGED SINCE ROUND 2: re-listen"),
        "{page}"
    );
    assert!(
        page.contains(">NEW<") && !page.contains(">APPROVED<"),
        "{page}"
    );
    let txt = page::report_text(&[r], &[]);
    assert!(
        txt.contains("S9100002 TEST PAD") && txt.contains("CHANGED SINCE ROUND 2: re-listen"),
        "{txt}"
    );

    // What the owner heard is bound too: the demo mode, home note,
    // scripts and phrase each demote it alone.
    for (from, to) in [
        ("demo = \"poly\"", "demo = \"mono\""),
        ("home = \"C3\"", "home = \"C2\""),
        ("plays = \"long texture\"", "plays = \"long\""),
        ("phrase = \"pad\"", "phrase = \"lead\""),
    ] {
        let s = &run(&format!("{BASE}{line}").replace(from, to)).unwrap()[0];
        assert_eq!(source::crc(s).unwrap(), source::crc(ok).unwrap(), "{to}");
        let now = source::approval_crc(s).unwrap();
        assert_eq!(
            (s.badge(now), s.changed(now)),
            (Badge::New, Some(Round(2))),
            "{to}"
        );
    }

    // The manifest records the Sound's own sound_crc.
    let (_, man) = build::files(std::slice::from_ref(ok), &STAMP).unwrap();
    let sound = source::crc(ok).unwrap();
    assert_eq!(man.sounds[0].sound, sound);
    assert!(man.json().contains(&format!("\"sound\": \"{sound:08X}\"")));

    // A bad format fails at its line.
    for bad in [
        "".to_string(),
        "round 2".into(),
        "round 2 1a2b3c4d".into(),
        "round 2 1A2B3C4".into(),
        "round 2 1A2B3C4D5".into(),
        "round 2 1A2B3C4G".into(),
        "2 1A2B3C4D".into(),
        "round two 1A2B3C4D".into(),
        "round +2 1A2B3C4D".into(),
        "round 0 1A2B3C4D".into(),
    ] {
        fails(&format!("{BASE}approved = \"{bad}\"\n"), 14, "approved");
    }
}

#[test]
fn manifest_crc_is_each_files_trailer() {
    let two = format!(
        "{BASE}\n{}",
        BASE.replace("S9100002", "S9100003")
            .replace("TEST PAD\"", "TEST TWO\"")
            .replace("\"1200\"", "\"900\"")
    );
    let s = run(&two).unwrap();
    let (files, man) = build::files(&s, &STAMP).unwrap();
    for (f, e) in files.iter().zip(&man.sounds) {
        let t = &f.1[f.1.len() - 4..];
        assert_eq!(e.crc, u32::from_le_bytes(t.try_into().unwrap()), "{}", f.0);
    }
    assert_ne!(man.sounds[0].crc, man.sounds[1].crc);
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
        assert!(page::checked_ledger(&p).is_err(), "{}", p.display());
    }
    let ok = "~/dev/chimera/.superpowers/ledgers/showcase";
    assert_eq!(
        page::checked_ledger(ok.as_ref()).unwrap(),
        std::path::PathBuf::from(ok)
    );
    // `write` refuses one too.
    assert!(page::write("/tmp/showcase-qa".as_ref(), &[], &[], &[], &[], false).is_err());
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
    let long = render::render(s, Script::Long).unwrap();
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
    assert_eq!(snd.params.out.sends, [0.5, 0.25, 1.0].map(SendLevel::new));
}

#[test]
fn listening_renders_are_wet_and_gates_dry() {
    let wet_src = &source::parse(&format!("{BASE}OUT.SEND1 = \"0.8\"\n"), "w.toml").unwrap()[0];
    let dry_src = &source::parse(BASE, "d.toml").unwrap()[0];
    // The listening render plays the Sound's sends through NEW's FX bus.
    let wet = render::render(wet_src, Script::Long).unwrap();
    assert!(
        wet != render::dry(wet_src, Script::Long).unwrap(),
        "CHORUS is heard"
    );
    assert!(
        render::dry(wet_src, Script::Long).unwrap()
            == render::render(dry_src, Script::Long).unwrap(),
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
    let page = page::page(&[gw], &[], &[], &[]);
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
        let x = render::dry(s, Script::Long).unwrap();
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
    // Velocity alone on the VCA is still organ-gated; a zero amount fails
    // at its own line.
    fails(
        &none.replace("trim = \"0\"", "mod.VEL = \"OUT.VCA +40\""),
        2,
        "OUT.VCA",
    );
    fails(
        &base_with(12, r#"mod.ENV1 = "OUT.VCA +0""#),
        12,
        "OUT.VCA +0",
    );
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
    assert_eq!(measure::dc(&clean, 0, n).1, Verdict::Pass);
    // 5 % of the RMS (-26 dB) on the left only.
    let off = 0.05 * 0.3 / 2f32.sqrt();
    let left: Vec<f32> = s.iter().map(|v| v + off).collect();
    let bad = stereo(&left, &s);
    let (v, verdict) = measure::dc(&bad, 0, n);
    assert_eq!(verdict, Verdict::Fail, "{v:.1} dB");
    assert!((v + 26.0).abs() < 0.5, "{v:.1}");
    // Turned down 40 dB: the same verdict (the old absolute -60 dBFS passed it).
    let quiet: Vec<f32> = bad.iter().map(|v| v * 0.01).collect();
    assert_eq!(measure::dc(&quiet, 0, n).1, Verdict::Fail);
    // Ten seconds of silence after it: judged over the held part only.
    let mut long = bad.clone();
    long.resize(bad.len() + 2 * 480_000, 0.0);
    assert_eq!(measure::dc(&long, 0, n).1, Verdict::Fail);
    // Under the bar passes.
    let ok: Vec<f32> = s.iter().map(|v| v + 0.1 * off).collect();
    assert_eq!(measure::dc(&stereo(&ok, &s), 0, n).1, Verdict::Pass);
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
        voices: 1,
    });
    assert!(page::write(&d, &[r], &[], &[], &[], true).unwrap());
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
    let hit = phrase::Hit {
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
    let (v, verdict) = measure::held_dc(&clean, &[hit]);
    assert_eq!(verdict, Verdict::Pass, "{v:.1} dB");
    let (v, verdict) = measure::held_dc(&note(0.05 * 0.3 / 2f32.sqrt()), &[hit]);
    assert_eq!(verdict, Verdict::Fail, "{v:.1} dB");
}

// ---- Phrase auditions ----

/// Every phrase lies within its bars (four; `sweep` 8 to 16), each note
/// sounding, and stays inside MIDI from any plausible home note (C1 to C6).
#[test]
fn phrases_fit_their_bars_and_midi() {
    assert_eq!(phrase::phrase_steps(Phrase::Bass), phrase::PHRASE_STEPS);
    assert!((8 * 16..=16 * 16).contains(&(phrase::phrase_steps(Phrase::Sweep) as u16)));
    for p in Phrase::ALL {
        let notes = phrase::phrase_notes(p);
        let steps = phrase::phrase_steps(p);
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
        let plan = phrase::phrase_plan(p, 36);
        assert_eq!(plan.hits.len(), notes.len());
        assert_eq!(plan.mode, None, "the Sound's demo mode, so MONO glides");
        for (h, x) in plan.hits.iter().zip(notes) {
            assert_eq!(h.note as i16, 36 + x.step as i16);
            assert!((h.on - x.at as f32 * phrase::SIXTEENTH_S).abs() < 1e-6);
            assert!((h.off - (x.at + x.len) as f32 * phrase::SIXTEENTH_S).abs() < 1e-6);
            assert_eq!(h.vel, x.vel);
        }
        let end = plan.hits.iter().map(|h| h.off).fold(0f32, f32::max);
        let bars = steps as f32 * phrase::SIXTEENTH_S;
        assert!(
            (bars - 1.0..=bars).contains(&end),
            "{p:?}: its bars at 120 BPM: {end}"
        );
        assert_eq!(phrase::phrase_secs(p), end);
        assert!(plan.secs > end, "a tail after it");
    }
}

/// A note held past the next one's start: on a MONO Part, a glide.
fn overlaps(p: Phrase) -> usize {
    let n = phrase::phrase_notes(p);
    n.iter()
        .zip(&n[1..])
        .filter(|(a, b)| b.at > a.at && b.at < a.at + a.len)
        .count()
}

#[test]
fn phrases_carry_what_they_show() {
    assert!(overlaps(Phrase::Bass) >= 1, "legato for the glide");
    let vels: Vec<u8> = phrase::phrase_notes(Phrase::Bass)
        .iter()
        .map(|x| x.vel)
        .collect();
    assert!(
        vels.iter().max().unwrap() - vels.iter().min().unwrap() >= 40,
        "velocity routes are heard"
    );
    let bass = phrase::phrase_notes(Phrase::Bass);
    assert!(
        bass.iter()
            .zip(&bass[1..])
            .any(|(a, b)| (a.step - b.step).abs() >= 12),
        "octave jumps"
    );
    let lead = phrase::phrase_notes(Phrase::Lead);
    assert!(overlaps(Phrase::Lead) >= 2, "a couple of slides");
    assert!(lead.iter().any(|x| x.len >= 8), "a held note for vibrato");
    assert!(lead.iter().any(|x| x.len <= 2), "and short ones");
    // `wonky`: a melodic bassline with odd accents and a couple of slides.
    let wonky = phrase::phrase_notes(Phrase::Wonky);
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
    // `acid`: mostly sixteenths, accented, with several slides and a rest.
    let acid = phrase::phrase_notes(Phrase::Acid);
    assert!(overlaps(Phrase::Acid) >= 6, "slides");
    assert!(
        acid.iter().filter(|x| x.len == 1).count() >= acid.len() / 2,
        "mostly single sixteenths"
    );
    let av: Vec<u8> = acid.iter().map(|x| x.vel).collect();
    assert!(
        av.iter().max().unwrap() - av.iter().min().unwrap() >= 40,
        "accents"
    );
    assert!(
        acid.iter().any(|x| x.at % 4 != 0 && x.vel >= 110),
        "an accent off the beat"
    );
    assert!(
        acid.iter()
            .zip(&acid[1..])
            .any(|(a, b)| (a.step - b.step).abs() >= 12),
        "octave jumps"
    );
    // `arp`: syncopated, and clear rather than dense.
    let arp = phrase::phrase_notes(Phrase::Arp);
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
        (0..phrase::PHRASE_STEPS)
            .step_by(16)
            .any(|b| arp.iter().all(|x| x.at != b)),
        "a rest on a bar's downbeat"
    );
    assert!(arp.len() <= 40, "clear, not dense: {}", arp.len());
    // `sweep`: two or three long chords, each struck whole (every note at
    // once, held together), so one envelope start swells each chord.
    let sw = phrase::phrase_notes(Phrase::Sweep);
    let mut starts: Vec<u8> = sw.iter().map(|x| x.at).collect();
    starts.dedup();
    assert!((2..=3).contains(&starts.len()), "{starts:?}");
    for at in starts {
        let chord: Vec<_> = sw.iter().filter(|x| x.at == at).collect();
        assert!(chord.len() >= 3, "a chord at {at}");
        assert!(chord.iter().all(|x| x.len == chord[0].len), "held together");
        assert!(chord[0].len >= 32, "held long: {}", chord[0].len);
    }
    let mut chords: Vec<u8> = phrase::phrase_notes(Phrase::Pad)
        .iter()
        .map(|x| x.at)
        .collect();
    chords.dedup();
    assert!((2..=4).contains(&chords.len()), "{chords:?}");
    assert!(
        phrase::phrase_notes(Phrase::Pad).iter().all(|x| x.len >= 8),
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

/// A `sweep` Sound needs a slow swell on the cutoff: a SLOW ENV over each
/// chord, or a slow smooth free LFO whose cycle the phrase holds, at ±16
/// or deeper; else the build fails on its id line.
#[test]
fn a_sweep_needs_a_slow_swell_its_phrase_holds() {
    let sweep = |extra: &str| format!("{}\n{extra}\n", base_with(13, r#"phrase = "sweep""#));
    // BASE's LFO 1 runs at the 1 Hz default: a wobble, not a swell.
    fails(&sweep(""), 2, "sweep");
    // ENV 2 at SLOW: rises 3.7 s, falls 3.3 s, within the 7.9 s chord.
    let env = |atk: &str, sus: &str, amt: &str, speed: &str| {
        sweep(&format!(
            "ENV2.SPEED = \"{speed}\"\nENV2.ATK = \"{atk}\"\nENV2.DEC = \"0.62\"\n\
             ENV2.SUS = \"{sus}\"\nmod.ENV2 = \"FILTER.CUTOFF {amt}\""
        ))
    };
    let ok = phrase::sweep(&run(&env("0.68", "0", "+40", "SLOW")).unwrap()[0]).unwrap();
    assert!(
        matches!(ok, phrase::Swell::Env { rise_s, .. } if rise_s > 3.0),
        "{ok:?}"
    );
    // A closing swell (negative depth) counts too.
    run(&env("0.68", "0", "-40", "SLOW")).unwrap();
    fails(&env("0.68", "0", "+10", "SLOW"), 2, "depth");
    fails(&env("0.68", "0", "+40", "MED"), 2, "SLOW");
    fails(&env("0.3", "0", "+40", "SLOW"), 2, "rises in");
    fails(&env("0.68", "1", "+40", "SLOW"), 2, "sustains");
    fails(&env("0.8", "0", "+40", "SLOW"), 2, "rise and fall");
    // The LFO path.
    let lfo = |hz: &str, extra: &str| {
        sweep(&format!(
            "LFO1.RATE = \"{hz}\"\nENV1.REL = \"0.5\"\n{extra}"
        ))
    };
    assert_eq!(
        phrase::sweep(&run(&lfo("0.05", "")).unwrap()[0]).unwrap(),
        phrase::Swell::Lfo { hz: 0.05 }
    );
    run(&lfo("0.1", "LFO1.SHAPE = \"TRIANGLE\"")).unwrap();
    let slow = 0.9 / phrase::phrase_secs(Phrase::Sweep);
    fails(&lfo(&slow.to_string(), ""), 2, "cycle");
    fails(&lfo("0.05", "LFO1.SHAPE = \"SQUARE\""), 2, "FREE SINE");
    fails(&lfo("0.05", "LFO1.SYNC = \"RETRIG\""), 2, "FREE SINE");
    fails(
        &lfo("0.05", "").replace("FILTER.CUTOFF +20", "FILTER.CUTOFF +10"),
        2,
        "depth",
    );
    fails(
        &lfo("0.05", "ENV1.SPEED = \"FAST\"").replace("ENV1.REL = \"0.5\"", "ENV1.REL = \"0\""),
        2,
        "release",
    );
    // No swell on the cutoff at all.
    let none = lfo("0.05", "").replace("mod.LFO1 = \"FILTER.CUTOFF +20\"", "");
    fails(&none, 2, "sweep");
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
        voices: 1,
    };
    let mut r = report(true, false);
    r.renders.insert(0, phrase(Phrase::Pad, 2.5));
    assert!(page::write(&d, &[r.clone()], &[], &[], &[], true).unwrap());
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
    page::write(&d, &[r.clone()], &[], &[], &[], true).unwrap();
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("limiter 3.4 dB, over the 3 dB bar"), "{page}");
    r.renders[0] = phrase(Phrase::Bass, 1.4);
    page::write(&d, &[r], &[], &[], &[], true).unwrap();
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
    assert!(page::report_text(&[r], &[]).contains("finite lead phrase"));
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

/// The click gates a Sound's `Clicks` rows.
fn click_gates(r: &qa::Report) -> Vec<&Gate> {
    r.gates
        .iter()
        .filter(|g| matches!(g.check, Check::Clicks(_)))
        .collect()
}

#[test]
fn a_va_ring_sounds_clicks_are_left_to_the_ear() {
    use chimera_desktop::listen::{CLICK_RATIO, click_exempt};
    let mut click = sine(220.0, 0.3, 1.0);
    click[24_000] += 0.2;
    let (ratio, v) = measure::clicks(&click, &[0], Some("VA RING: why"));
    assert!(ratio > CLICK_RATIO, "{ratio}");
    assert_eq!(v, Verdict::Exempt("VA RING: why".into()));
    // Exempt from the bar, not from being finite.
    let mut nan = click.clone();
    nan[100] = f32::NAN;
    assert_eq!(measure::clicks(&nan, &[0], Some("why")).1, Verdict::Fail);

    let ring = &run(&format!("{BASE}VA.MODE = \"RING\"\n")).unwrap()[0];
    let why = click_exempt(&source::to_sound(ring).unwrap()).expect("VA RING is exempt");
    let r = qa::gates(ring).unwrap();
    let gates = click_gates(&r);
    assert_eq!(gates.len(), 2, "long and texture");
    for g in gates {
        assert_eq!(g.verdict, Verdict::Exempt(why.into()), "{:?}", g.check);
    }
    let txt = page::report_text(std::slice::from_ref(&r), &[]);
    assert!(txt.contains(&format!("EXEMPT ({why})")), "{txt}");
    // On the page (its other gates aside: this test Sound is not levelled).
    let mut shown = r.clone();
    shown.gates.retain(|g| matches!(g.check, Check::Clicks(_)));
    let page = page::page(&[shown], &[], &[], &[]);
    assert!(
        page.contains(&format!("class=\"flag\">clicks long: {why}")),
        "{page}"
    );

    // A VA in another mode keeps the bar.
    let saw = qa::gates(&run(BASE).unwrap()[0]).unwrap();
    assert!(click_gates(&saw).iter().all(|g| g.verdict == Verdict::Pass));
}

#[test]
fn residual_against_the_baseline() {
    let old = sine(220.0, 0.2, 0.5);
    assert_eq!(baseline::residual_db(&old, &old), f32::NEG_INFINITY);
    // new − old is a tenth of old: −20 dB.
    let new: Vec<f32> = old.iter().map(|v| v * 1.1).collect();
    assert!((baseline::residual_db(&new, &old) + 20.0).abs() < 0.01);
    // As written: a longer render's extra counts against silence.
    let mut longer = old.clone();
    longer.extend_from_slice(&old);
    assert!(baseline::residual_db(&longer, &old).abs() < 0.01);
    const { assert!(baseline::AUDIO_CHANGED_DB == -40.0) };
}

/// The heavily routed three-FRMT Source `renders_steal_…` plays.
fn heavy_frmt() -> source::Source {
    let algo = BASE.replace("engine = \"VA\"", "engine = \"ALGO\"");
    run(&format!(
        "{algo}ALGO.ALG_A = \"A1\"\nALGO.ALG_B = \"A1\"\nOP_A.WAVE = \"FRMT\"\nOP_B.WAVE = \"FRMT\"\nOP_C.WAVE = \"FRMT\"\nOP_D.LEVEL = \"99\"\nOP_E.LEVEL = \"99\"\nOP_F.LEVEL = \"99\"\nFILTER.KIND = \"2040\"\nmod.LFO2 = \"OP_A.FRMT +10, OP_B.FRMT +10, OP_C.BW +10, ALGO.VOWEL +20\"\nmod.ENV2 = \"FILTER.CUTOFF +20, OP_A.LEVEL +10\"\n"
    ))
    .unwrap()
    .remove(0)
}

/// The voice budget is a changed render's cause only when proven: the same
/// render at an unlimited budget sounds more voices than the unit's did.
#[test]
fn the_voice_budget_cause_is_proven() {
    use chimera_desktop::listen::write_wav_stereo;
    let heavy = heavy_frmt();
    let light = run(BASE).unwrap().remove(0);
    let stab = Audition::Script(Script::Stab);
    assert!(qa::over_budget(&heavy, stab).unwrap(), "its stab steals");
    assert!(!qa::over_budget(&light, stab).unwrap());
    assert!(!qa::over_budget(&heavy, Audition::Script(Script::Long)).unwrap());

    let d = scratch_ledger("voice-budget");
    std::fs::create_dir_all(&d).unwrap();
    let mut r = report_approved();
    r.voices = 3;
    r.renders[0].audition = stab;
    let old: Vec<f32> = r.renders[0].stereo.iter().map(|v| v * 0.9).collect();
    write_wav_stereo(&d.join("S9100002-stab.wav"), &old).unwrap();
    let cause = |r: &mut qa::Report, s: &source::Source| {
        baseline::compare(r, &d).unwrap();
        qa::voice_cause(r, s).unwrap();
        page::report_text(std::slice::from_ref(r), &[])
    };
    let txt = cause(&mut r, &heavy);
    assert!(
        txt.contains("voice budget: the unit plays 3 voices"),
        "{txt}"
    );
    let txt = cause(&mut r, &light);
    assert!(
        txt.contains("AUDIO CHANGED") && !txt.contains("voice budget"),
        "{txt}"
    );
}

/// Each Sound's voice count on the unit, in report.txt and on the page.
#[test]
fn the_voice_count_is_printed() {
    let r = report(true, false);
    let txt = page::report_text(std::slice::from_ref(&r), &[]);
    assert!(txt.contains("voices on the unit: 8"), "{txt}");
    let page = page::page(std::slice::from_ref(&r), &[], &[], &[]);
    assert!(
        page.contains("8 VOICES") && page.contains("rev V (480 MHz)"),
        "{page}"
    );
}

#[test]
fn an_engine_change_flags_an_approved_sound() {
    use chimera_desktop::listen::write_wav_stereo;
    let d = scratch_ledger("baseline");
    std::fs::create_dir_all(&d).unwrap();
    let wav = d.join("S9100002-long.wav");
    let mut r = report(true, false);
    r.badge = source::Badge::Approved;
    let flagged = |r: &qa::Report| {
        page::page(std::slice::from_ref(r), &[], &[], &[])
            .contains("class=\"flag\">AUDIO CHANGED BY ENGINE UPDATE: re-listen")
    };

    // The same render: no flag.
    write_wav_stereo(&wav, &r.renders[0].stereo).unwrap();
    baseline::compare(&mut r, &d).unwrap();
    let b = r.baseline.clone().expect("compared");
    assert_eq!(b.changed(), None);
    assert!(b.missing.is_empty());
    assert!(!flagged(&r));

    // The baseline at 0.9 of it: −19.1 dB, flagged, still APPROVED.
    let old: Vec<f32> = r.renders[0].stereo.iter().map(|v| v * 0.9).collect();
    write_wav_stereo(&wav, &old).unwrap();
    baseline::compare(&mut r, &d).unwrap();
    let (a, db) = r.baseline.as_ref().unwrap().changed().expect("flagged");
    assert_eq!(a, Audition::Script(Script::Long));
    assert!((db + 19.08).abs() < 0.2, "{db}");
    assert!(flagged(&r));
    let page = page::page(std::slice::from_ref(&r), &[], &[], &[]);
    assert!(page.contains(">APPROVED<"), "the badge stays");
    let txt = page::report_text(std::slice::from_ref(&r), &[]);
    assert!(
        txt.contains("AUDIO CHANGED BY ENGINE UPDATE: re-listen (worst -19.1 dB: long)"),
        "{txt}"
    );

    // A NEW Sound is not compared; without a baseline nothing is.
    let mut new = report(true, false);
    baseline::compare(&mut new, &d).unwrap();
    assert!(new.baseline.is_none() && !flagged(&new));

    // No baseline file: no flag, and a note.
    std::fs::remove_file(&wav).unwrap();
    baseline::compare(&mut r, &d).unwrap();
    let b = r.baseline.clone().unwrap();
    assert_eq!(b.changed(), None);
    assert_eq!(b.missing, vec![Audition::Script(Script::Long)]);
    assert!(!flagged(&r));
    let txt = page::report_text(std::slice::from_ref(&r), &[]);
    assert!(
        txt.contains("baseline: no S9100002-long.wav, not compared"),
        "{txt}"
    );
}

#[test]
fn free_lfos_name_their_routes() {
    let snd = |t: &str| source::to_sound(&run(t).unwrap()[0]).unwrap();
    assert_eq!(about::free_lfos(&snd(BASE)), vec!["LFO 1 on CUTOFF"]);
    let retrig = format!("{BASE}LFO1.SYNC = \"RETRIG\"\n");
    assert!(
        about::free_lfos(&snd(&retrig)).is_empty(),
        "RETRIG runs its own"
    );
}

#[test]
fn bisected_causes_name_the_change_and_coherent_ones_lead() {
    use chimera_desktop::listen::write_wav_stereo;
    let d = scratch_ledger("causes");
    std::fs::create_dir_all(&d).unwrap();
    // Two approved Sounds, each 0.9 of its baseline (-19.1 dB).
    let mut drift = report(true, false);
    drift.badge = source::Badge::Approved;
    let mut other = drift.clone();
    other.name = "OTHER PAD".into();
    other.id = source::parse(&BASE.replace("S9100002", "S9100003"), "b.toml").unwrap()[0].id;
    for r in [&drift, &other] {
        let old: Vec<f32> = r.renders[0].stereo.iter().map(|v| v * 0.9).collect();
        write_wav_stereo(&d.join(format!("{}-long.wav", r.id)), &old).unwrap();
    }
    std::fs::write(
        d.join("causes.txt"),
        "# id, then each change it bisects to and its residual\nS9100002 #489 -14.6\nS9100003 #485 -24.1 #489 -3.0\n",
    )
    .unwrap();
    baseline::compare(&mut drift, &d).unwrap();
    baseline::compare(&mut other, &d).unwrap();
    let lines = drift.baseline.as_ref().unwrap().lines(&drift.free_lfos);
    assert_eq!(
        lines,
        vec![
            "AUDIO CHANGED BY ENGINE UPDATE: re-listen".to_string(),
            "LFO MOTION CHANGED -14.6 dB: #489 FREE LFO 1 on CUTOFF now runs on the Part clock"
                .into(),
        ]
    );
    let lines = other.baseline.as_ref().unwrap().lines(&other.free_lfos);
    assert!(
        lines.contains(
            &"TIMBRE CHANGED -24.1 dB: #485 the sub flips at the shaped saw's edge".into()
        ),
        "{lines:?}"
    );
    assert!(lines.contains(
        &"LFO MOTION CHANGED -3.0 dB: #489 FREE LFO 1 on CUTOFF now runs on the Part clock".into()
    ));

    // Both coherent: Source order. A near-0 dB cause alone goes after.
    let mut near = drift.clone();
    near.name = "NEAR PAD".into();
    near.baseline.as_mut().unwrap().causes[0].1 = -2.0;
    let page = page::page(&[near.clone(), other.clone(), drift.clone()], &[], &[], &[]);
    let at = |w: &str| page.find(w).unwrap_or_else(|| panic!("{w}: {page}"));
    assert!(at("<h3>OTHER PAD") < at("<h3>TEST PAD"), "Source order");
    assert!(at("<h3>TEST PAD") < at("<h3>NEAR PAD"), "coherent first");
    assert!(page.contains("class=\"flag\">LFO MOTION CHANGED -14.6 dB"));
    assert!(
        page.contains("coherent drift, not timbre"),
        "the page says what it measures"
    );
    let txt = page::report_text(&[near, other, drift], &[]);
    assert!(txt.contains("TIMBRE CHANGED -24.1 dB: #485"), "{txt}");
    assert!(txt.contains("coherent drift, not timbre"), "{txt}");

    // A change the tool does not know fails at its line.
    std::fs::write(d.join("causes.txt"), "S9100002 #999 -3.0\n").unwrap();
    let e = baseline::compare(&mut report_approved(), &d).unwrap_err();
    assert!(e.contains("causes.txt:1"), "{e}");
}

fn report_approved() -> qa::Report {
    let mut r = report(true, false);
    r.badge = source::Badge::Approved;
    r
}

#[test]
fn a_trim_backs_off_until_the_limiter_passes() {
    // Over the bar above 3.85 dB: 4.1 steps down to 3.8.
    assert_eq!(qa::backed_off(4.1, |t| Ok(t > 3.85)).unwrap(), 3.8);
    assert_eq!(qa::backed_off(4.1, |_| Ok(false)).unwrap(), 4.1);
    assert!(
        qa::backed_off(4.1, |_| Ok(true)).is_err(),
        "never past 6 dB"
    );
}

// ── demo projects (spec § 5) ──

use chimera_core::MidiChannel;
use chimera_core::dsp::chorus::ChorusMode;
use chimera_core::dsp::reverb::ReverbType;
use chimera_core::project::{Origin, PartId, Project, ProjectCheck, ProjectDecoder};
use chimera_core::storage::{Check as _, DecodeInPlace, Framer};
use chimera_desktop::factory_sounds::project;

/// Two public Sounds a demo names: TEST PAD (into the reverb) and TEST TWO
/// (into the chorus and the delay).
fn demo_sounds() -> Vec<source::Source> {
    let two = BASE
        .replace("S9100002", "S9100003")
        .replace("TEST PAD\"", "TEST TWO\"")
        .replace("\"1200\"", "\"900\"");
    // Trimmed under the chord bar, as a levelled Sound is.
    run(
        &format!("{BASE}OUT.SEND3 = \"0.5\"\n\n{two}OUT.SEND1 = \"0.6\"\nOUT.SEND2 = \"0.4\"\n")
            .replace("trim = \"0\"", "trim = \"-18\""),
    )
    .unwrap()
}

/// A valid demo; each bad test changes one line.
const DEMO: &str = r#"[[project]]
id = "P0000001"
name = "REVERB ROOMS"
try = "Turn TYPE through QUAD, ROOM and HALL"
fx.REVERB.TYPE = "ROOM"
part.1 = "TEST PAD, POLY, CH 1"
part.2 = "TEST TWO, POLY, CH 2"
"#;

fn demo_with(n: usize, with: &str) -> String {
    DEMO.lines()
        .enumerate()
        .map(|(i, l)| if i + 1 == n { with } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse, check against `sounds` and build, as `build::public` runs them.
fn run_demo(text: &str, sounds: &[source::Source]) -> Result<Vec<u8>, String> {
    let demos = project::parse(text)?;
    project::check(&demos, sounds)?;
    let (files, _) = build::demo_files(&demos, sounds)?;
    Ok(files.into_iter().next().expect("one demo").1)
}

fn demo_fails(text: &str, sounds: &[source::Source], line: usize, what: &str) {
    let e = run_demo(text, sounds).expect_err("a bad demo must fail");
    assert!(
        e.contains(&format!("projects.toml:{line}:")) && e.contains(what),
        "want projects.toml:{line} naming {what}, got: {e}"
    );
}

/// The project file read back as a load reads it: checked, then applied.
fn decoded(bytes: &[u8]) -> Box<Project> {
    let (mut p, _) = Project::boxed();
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    let mut c = ProjectCheck::new();
    f.push(bytes, &mut |e| c.event(e)).unwrap();
    f.finish().unwrap();
    c.end().unwrap();
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    let mut d = ProjectDecoder::new_for_test(&mut p);
    f.push(bytes, &mut |e| d.apply(e)).unwrap();
    f.finish().unwrap();
    DecodeInPlace::finish(&mut d).unwrap();
    p
}

#[test]
fn demo_round_trips() {
    let sounds = demo_sounds();
    let bytes = run_demo(DEMO, &sounds).unwrap();
    let (files, entries) = build::demo_files(&project::parse(DEMO).unwrap(), &sounds).unwrap();
    assert_eq!(files[0].0, "P0000001.A");
    assert_eq!(entries[0].name, "REVERB ROOMS");
    let p = decoded(&bytes);
    assert_eq!(p.meta().name().as_str(), "REVERB ROOMS");
    assert_eq!(p.perf().fx.reverb.kind, ReverbType::Room);
    for (i, s) in sounds.iter().enumerate() {
        let part = p.part(PartId::ALL[i]);
        let want = source::to_sound(s).unwrap();
        assert_eq!(part.sound.name.as_str(), s.name.as_str(), "Part {}", i + 1);
        // The Part carries the Sound whole, its own sends with it.
        assert_eq!(sound_crc(&part.sound), sound_crc(&want), "Part {}", i + 1);
        assert_eq!(part.sound.params.out.sends, want.params.out.sends);
        assert_eq!(part.mix.channel, MidiChannel::new(i as u8).unwrap());
        assert_eq!(part.mix.mode(), PartMode::Poly);
        // From a pool slot, as a library load leaves it.
        assert!(
            matches!(part.origin(), Origin::Slot { .. }),
            "Part {}",
            i + 1
        );
    }
}

#[test]
fn demo_part_unknown_sound_fails() {
    let sounds = demo_sounds();
    demo_fails(
        &demo_with(7, "part.2 = \"NO SUCH, POLY, CH 2\""),
        &sounds,
        7,
        "NO SUCH",
    );
    // A placeholder has no Sound to hold.
    let ph = run(&format!(
        "{}status = \"placeholder\"\n",
        BASE.replace("S9100002", "S9100004")
            .replace("TEST PAD\"", "LATER PAD\"")
    ))
    .unwrap();
    let all: Vec<_> = sounds.iter().cloned().chain(ph).collect();
    demo_fails(
        &demo_with(6, "part.1 = \"LATER PAD, POLY, CH 1\""),
        &all,
        6,
        "LATER PAD",
    );
    for (bad, what) in [
        ("part.2 = \"TEST TWO, POLY\"", "CH"),
        ("part.2 = \"TEST TWO, STEREO, CH 2\"", "POLY or MONO"),
        ("part.2 = \"TEST TWO, POLY, CH 17\"", "CH 1-16"),
        ("part.7 = \"TEST TWO, POLY, CH 2\"", "part.1-part.6"),
        ("part.1 = \"TEST TWO, POLY, CH 2\"", "part.1 twice"),
    ] {
        demo_fails(&demo_with(7, bad), &sounds, 7, what);
    }
    demo_fails(
        &demo_with(5, "fx.REVERB.KIND = \"ROOM\""),
        &sounds,
        5,
        "fx.REVERB.KIND",
    );
    demo_fails(
        &demo_with(5, "fx.REVERB.TYPE = \"CAVE\""),
        &sounds,
        5,
        "CAVE",
    );
    demo_fails(
        &demo_with(5, "fx.FILTER.CUTOFF = \"900\""),
        &sounds,
        5,
        "fx.FILTER",
    );
    demo_fails(&demo_with(4, "# no try"), &sounds, 1, "try");
}

#[test]
fn demo_part_private_sound_fails() {
    let mut sounds = demo_sounds();
    // A Source with a `from` line is a private port.
    let from = source::parse(
        &format!(
            "{}from = \"tx81z 0123abcd #1\"\n",
            BASE.replace("S9100002", "S9100090")
                .replace("TEST PAD\"", "FROM ONE\"")
        ),
        "ports.toml",
    )
    .unwrap();
    sounds.extend(from);
    demo_fails(
        &demo_with(7, "part.2 = \"FROM ONE, POLY, CH 2\""),
        &sounds,
        7,
        "private",
    );
}

#[test]
fn demo_id_outside_1_to_9_fails() {
    let sounds = demo_sounds();
    for bad in ["P0000000", "P0000010", "P0000099", "S9100001", "P1"] {
        demo_fails(
            &demo_with(2, &format!("id = \"{bad}\"")),
            &sounds,
            2,
            "P0000001-P0000009",
        );
    }
    // Two demos on one id: the second fails at its id.
    demo_fails(
        &format!("{DEMO}\n{}", DEMO.replace("REVERB ROOMS", "OTHER ROOMS")),
        &sounds,
        10,
        "P0000001",
    );
    run_demo(&demo_with(2, "id = \"P0000009\""), &sounds).unwrap();
}

#[test]
fn mono_part_mode_set() {
    let sounds = demo_sounds();
    // CH 7: no Part NEW leaves in place plays there.
    let p =
        decoded(&run_demo(&demo_with(7, "part.2 = \"TEST TWO, MONO, CH 7\""), &sounds).unwrap());
    let two = p.part(PartId::ALL[1]);
    assert_eq!(two.mix.mode(), PartMode::Mono);
    assert_eq!(two.mix.channel, MidiChannel::new(6).unwrap());
    assert_eq!(p.part(PartId::ALL[0]).mix.mode(), PartMode::Poly);
}

#[test]
fn manifest_lists_each_demo_with_its_trailer_crc() {
    let sounds = demo_sounds();
    let two = format!(
        "{DEMO}\n{}",
        DEMO.replace("P0000001", "P0000002")
            .replace("REVERB ROOMS", "REVERB BLOOM")
            .replace("\"ROOM\"", "\"BLOOM\"")
    );
    let demos = project::parse(&two).unwrap();
    let (files, entries) = build::demo_files(&demos, &sounds).unwrap();
    let (_, mut man) = build::files(&sounds, &STAMP).unwrap();
    man.projects = entries;
    let json = man.json();
    for (f, e) in files.iter().zip(&man.projects) {
        let crc = u32::from_le_bytes(f.1[f.1.len() - 4..].try_into().unwrap());
        assert_eq!(e.crc, crc, "{}", f.0);
        let line = format!(
            "{{\"id\": \"{}\", \"name\": \"{}\", \"crc\": \"{crc:08X}\"}}",
            &f.0[..8],
            e.name
        );
        assert!(json.contains(&line), "{line} in {json}");
    }
    assert!(json.contains("\"projects\": ["), "{json}");
    assert_ne!(man.projects[0].crc, man.projects[1].crc);
}

/// The spec's four demos (§ 5), each `hear` line a "what to try".
fn spec_demos() -> String {
    r#"[[project]]
id = "P0000001"
name = "REVERB ROOMS"
try = "Turn TYPE through QUAD, ROOM and HALL"
fx.REVERB.TYPE = "ROOM"
hear.REVERB.TYPE = "QUAD, HALL"
part.1 = "TEST PAD, POLY, CH 1"

[[project]]
id = "P0000002"
name = "REVERB BLOOM"
try = "Turn TYPE to REVERSE"
fx.REVERB.TYPE = "BLOOM"
hear.REVERB.TYPE = "REVERSE"
part.1 = "TEST PAD, POLY, CH 1"

[[project]]
id = "P0000003"
name = "REVERB GATE"
try = "Gated percussive tails"
fx.REVERB.TYPE = "GATE"
part.1 = "TEST PAD, POLY, CH 1"

[[project]]
id = "P0000004"
name = "CHORUS DELAY"
try = "CHORUS I, II and I+II; delay feedback"
fx.CHORUS.MODE = "JUNO_BOTH"
hear.CHORUS.MODE = "JUNO_I, JUNO_II"
part.1 = "TEST TWO, POLY, CH 1"
part.2 = "TEST TWO, MONO, CH 2"
"#
    .into()
}

#[test]
fn demo_render_plan_hears_every_effect() {
    use project::Effect;
    let sounds = demo_sounds();
    let demos = project::parse(&spec_demos()).unwrap();
    project::check(&demos, &sounds).unwrap();
    assert_eq!(
        project::coverage(&demos, &sounds).unwrap(),
        project::Coverage::default()
    );

    // Each setting (its own FX, then one per `hear` value) plays every
    // Part's stab and short, then the hats and the snare.
    let one = &demos[0];
    let settings = project::settings(one, &sounds).unwrap();
    assert_eq!(settings.len(), 3);
    assert_eq!(settings[0].heard, vec![Effect::Reverb(ReverbType::Room)]);
    assert_eq!(settings[1].heard, vec![Effect::Reverb(ReverbType::Quad)]);
    assert_eq!(settings[2].heard, vec![Effect::Reverb(ReverbType::Hall)]);
    let takes = demo::demo_takes(&demos[3]);
    assert_eq!(takes.len(), 2 * 2 + 2);
    assert!(takes.contains(&demo::DemoTake::Part(PartId::ALL[1], Script::Stab)));
    assert!(takes.contains(&demo::DemoTake::Part(PartId::ALL[1], Script::Short)));
    assert!(takes.contains(&demo::DemoTake::Hats));
    assert!(takes.contains(&demo::DemoTake::Snare));
    let four = project::settings(&demos[3], &sounds).unwrap();
    assert_eq!(
        four[0].heard,
        vec![Effect::Chorus(ChorusMode::JunoBoth), Effect::Delay]
    );

    // Without the `hear` lines, what goes unheard is named in full words.
    let bare: String = spec_demos()
        .lines()
        .filter(|l| !l.starts_with("hear."))
        .map(|l| format!("{l}\n"))
        .collect();
    let gaps = project::coverage(&project::parse(&bare).unwrap(), &sounds)
        .unwrap()
        .unheard;
    let words: Vec<String> = gaps.iter().map(|e| e.label()).collect();
    assert_eq!(
        words,
        [
            "REVERB QUAD",
            "REVERB HALL",
            "REVERB REVERSE",
            "CHORUS I",
            "CHORUS II"
        ]
    );
    // An effect nobody sends to is not heard, whatever its TYPE.
    let dry = demo_with(6, "part.1 = \"TEST TWO, POLY, CH 1\"");
    let d = project::parse(&dry.replace("part.2 = \"TEST TWO, POLY, CH 2\"", "")).unwrap();
    let s = project::settings(&d[0], &sounds).unwrap();
    assert!(
        !s[0].heard.contains(&Effect::Reverb(ReverbType::Room)),
        "{:?}",
        s[0].heard
    );
    // A `hear` value that is not one fails at its line.
    demo_fails(
        &format!("{DEMO}hear.REVERB.TYPE = \"QUAD, CAVE\"\n"),
        &sounds,
        8,
        "CAVE",
    );
}

#[test]
fn demo_renders_gate_and_sit_on_the_page() {
    let sounds = demo_sounds();
    let demos = project::parse(DEMO).unwrap();
    // A 50 ms Hann-windowed burst: smooth at both ends, as a stimulus is.
    let burst = |hz: f32| -> Vec<f32> {
        let n = 2_400;
        let mut x: Vec<f32> = (0..n)
            .map(|i| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
                0.25 * w * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin()
            })
            .collect();
        x.resize(24_000, 0.0);
        x
    };
    let stims = demo::Stims {
        hats: burst(5_000.0),
        snare: burst(200.0),
    };
    let r = demo::demo_gates(&demos[0], &sounds, &stims).unwrap();
    assert!(r.passes(), "{:#?}", r.gates);
    // Each Part's stab and short and the two stimuli, through its own FX.
    assert_eq!(r.renders.len(), 2 * 2 + 2);
    assert!(
        r.gates.iter().any(|g| g.check
            == demo::DemoCheck::Limiter(demo::DemoTake::Part(PartId::ALL[0], Script::Stab)))
    );
    let d = scratch_ledger("ledger-demo");
    assert!(
        page::write(
            &d,
            &[report(true, false)],
            &[],
            &[],
            std::slice::from_ref(&r),
            true
        )
        .unwrap()
    );
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("<h2>DEMO PROJECTS</h2>"), "{page}");
    assert!(page.contains("Turn TYPE through QUAD, ROOM and HALL"));
    assert!(
        page.contains("PART 1 · TEST PAD (NEW) · POLY · CH 1"),
        "{page}"
    );
    assert!(
        page.contains("REVERB ROOM"),
        "the setting heard, in full words"
    );
    let wavs: Vec<_> = std::fs::read_dir(d.join("renders"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("P0000001-"))
        .collect();
    assert_eq!(wavs.len(), 6, "{wavs:?}");
    let txt = std::fs::read_to_string(d.join("report.txt")).unwrap();
    assert!(txt.contains("P0000001 REVERB ROOMS  PASS"), "{txt}");

    // A failing project render keeps it off the page and fails the run.
    let mut bad = r.clone();
    bad.gates[0].verdict = Verdict::Fail;
    assert!(!page::write(&d, &[report(true, false)], &[], &[], &[bad], true).unwrap());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(!page.contains("Turn TYPE through"), "{page}");
}

#[test]
fn a_part_on_a_channel_new_keeps_fails() {
    let sounds = demo_sounds();
    // Part 3 stays NEW's on CH 3: Part 2 there would stack with it.
    demo_fails(
        &demo_with(7, "part.2 = \"TEST TWO, MONO, CH 3\""),
        &sounds,
        7,
        "CH 3 is Part 3's",
    );
    // Naming Part 3 frees its channel: named Parts may layer.
    let three = format!(
        "{}\npart.3 = \"TEST PAD, POLY, CH 2\"\n",
        demo_with(7, "part.2 = \"TEST TWO, MONO, CH 3\"")
    );
    run_demo(&three, &sounds).unwrap();
}

#[test]
fn hear_values_are_ones_the_try_line_names() {
    let sounds = demo_sounds();
    demo_fails(
        &format!("{DEMO}hear.REVERB.TYPE = \"QUAD, BLOOM\"\n"),
        &sounds,
        8,
        "does not name BLOOM",
    );
    // A CHORUS MODE by its page name: JUNO_BOTH reads I+II.
    let chorus = DEMO.replace("Turn TYPE through QUAD, ROOM and HALL", "CHORUS I and II");
    run_demo(
        &format!("{chorus}hear.CHORUS.MODE = \"JUNO_I, JUNO_II\"\n"),
        &sounds,
    )
    .unwrap();
    demo_fails(
        &format!("{chorus}hear.CHORUS.MODE = \"JUNO_BOTH\"\n"),
        &sounds,
        8,
        "does not name I+II",
    );
}

/// `MIN_SEND` is the gain reaching an effect, past the send taper
/// (ADR 0278): a level of 0.31 feeds 0.096, under it; 0.32 feeds 0.102.
#[test]
fn a_send_under_a_tenth_is_not_heard() {
    use project::Effect;
    let fx = chimera_core::dsp::fx_bus::FxParams::DEFAULT;
    let reverb = |l: SendLevel| [SendLevel::OFF, SendLevel::OFF, l];
    assert_eq!(project::MIN_SEND, Gain::new(0.1));
    assert!(project::heard(&fx, reverb(SendLevel::new(0.31))).is_empty());
    assert_eq!(
        project::heard(&fx, reverb(SendLevel::new(0.32))),
        vec![Effect::Reverb(ReverbType::Quad)]
    );
    let mut off = fx;
    off.reverb.mix = 0.0;
    assert!(
        project::heard(&off, [0.5, 0.0, 0.5].map(SendLevel::new))
            .iter()
            .all(|e| !matches!(e, Effect::Reverb(_)))
    );
}

/// The delay's REV SEND counts by its gain too: 0.3 into the reverb is
/// not hearing it.
#[test]
fn a_rev_send_under_a_tenth_is_not_heard() {
    use project::Effect;
    let mut fx = chimera_core::dsp::fx_bus::FxParams::DEFAULT;
    let delay = [SendLevel::OFF, SendLevel::FULL, SendLevel::OFF];
    fx.delay.rev_send = SendLevel::new(0.3);
    assert_eq!(project::heard(&fx, delay), vec![Effect::Delay]);
    fx.delay.rev_send = SendLevel::new(0.4);
    assert!(project::heard(&fx, delay).contains(&Effect::Reverb(fx.reverb.kind)));
}

/// The factory's sends and REV SENDs sit on the knob's 1/128 grid
/// (ADR 0278: re-voiced by `SendLevel::from_gain`, never between steps).
#[test]
fn factory_sends_sit_on_the_grid() {
    let dir = std::path::Path::new(SOURCES);
    for s in build::sources(dir).unwrap() {
        if s.is_placeholder() || s.from.is_some() {
            continue;
        }
        for l in source::to_sound(&s).unwrap().params.out.sends {
            assert!(l.grid_step().is_some(), "{}: {l:?}", s.name.as_str());
        }
    }
    for d in build::demos(dir).unwrap() {
        for fx in std::iter::once(d.fx).chain(d.hear) {
            let l = fx.delay.rev_send;
            assert!(l.grid_step().is_some(), "{:?}: {l:?}", d.name);
        }
    }
}

#[test]
fn a_sound_already_in_the_pool_takes_no_new_slot() {
    let sounds = demo_sounds();
    let one = demo_with(7, "part.2 = \"TEST PAD, POLY, CH 2\"");
    let p = project::build(&project::parse(&one).unwrap()[0], &sounds).unwrap();
    let (new, _) = Project::boxed();
    // Both Parts share TEST PAD's one slot, found by its sound_crc.
    assert_eq!(p.pool().sound_count(), new.pool().sound_count() + 1);
    let slot = |i: usize| match p.part(PartId::ALL[i]).origin() {
        Origin::Slot { slot, .. } => slot,
        o => panic!("{o:?}"),
    };
    assert_eq!(slot(0), slot(1));
}

/// No reverb TYPE leaves a demo's clicks to the ear: REVERSE's and
/// GATE's edges ramp since #499, so their takes are click-gated like ROOM's.
#[test]
fn reverse_and_gate_settings_are_click_gated() {
    let sounds = demo_sounds();
    let stims = demo::Stims {
        hats: vec![0.0; 4_800],
        snare: vec![0.0; 4_800],
    };
    for kind in ["REVERSE", "GATE", "ROOM"] {
        let text = demo_with(5, &format!("fx.REVERB.TYPE = \"{kind}\""));
        let d = &project::parse(&text).unwrap()[0];
        let heard = &project::settings(d, &sounds).unwrap()[0].heard;
        assert!(
            heard.iter().any(|e| e.label() == format!("REVERB {kind}")),
            "{kind}: {heard:?}"
        );
        let clicks: Vec<_> = demo::demo_gates(d, &sounds, &stims)
            .unwrap()
            .gates
            .into_iter()
            .filter(|g| matches!(g.check, demo::DemoCheck::Clicks(demo::DemoTake::Part(..))))
            .map(|g| g.verdict)
            .collect();
        assert!(!clicks.is_empty(), "{kind}");
        for v in clicks {
            assert!(!matches!(v, Verdict::Exempt(_)), "{kind}: {v:?}");
        }
    }
}

/// The spec's demos with P1 and P2 held.
fn held_demos() -> String {
    spec_demos()
        .replace(
            "name = \"REVERB ROOMS\"\n",
            "name = \"REVERB ROOMS\"\nheld = \"#499: HALL and BLOOM tails jump at block edges\"\n",
        )
        .replace(
            "name = \"REVERB BLOOM\"\n",
            "name = \"REVERB BLOOM\"\nheld = \"#499: HALL and BLOOM tails jump at block edges\"\n",
        )
}

/// A held demo is checked like any other but neither built, listed nor
/// rendered; what only it plays is pending, not unheard.
#[test]
fn held_demos_are_checked_not_built_and_their_effects_pend() {
    use project::Effect;
    let sounds = demo_sounds();
    let demos = project::parse(&held_demos()).unwrap();
    assert!(
        demos[0]
            .held
            .as_deref()
            .is_some_and(|h| h.starts_with("#499"))
    );
    assert!(demos[2].held.is_none());
    let (files, entries) = build::demo_files(&demos, &sounds).unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["P0000003.A", "P0000004.A"]);
    let ids: Vec<String> = entries.iter().map(|e| e.id.stem()).collect();
    assert_eq!(ids, ["P0000003", "P0000004"]);

    // Still checked: a held demo's unknown Sound fails at its line.
    let bad = held_demos().replacen("part.1 = \"TEST PAD", "part.1 = \"NO SUCH", 1);
    let e = build::demo_files(&project::parse(&bad).unwrap(), &sounds).unwrap_err();
    assert!(e.contains("no Sound named NO SUCH"), "{e}");
    // An empty reason fails at its line.
    let e = project::parse(&held_demos().replacen(
        "held = \"#499: HALL and BLOOM tails jump at block edges\"",
        "held = \" \"",
        1,
    ))
    .unwrap_err();
    assert!(
        e.contains("projects.toml:4:") && e.contains("reason"),
        "{e}"
    );

    let cov = project::coverage(&demos, &sounds).unwrap();
    assert!(cov.unheard.is_empty(), "{cov:?}");
    assert_eq!(
        cov.pending_lines(),
        [
            "pending: REVERB QUAD (held: P0000001)",
            "pending: REVERB ROOM (held: P0000001)",
            "pending: REVERB HALL (held: P0000001)",
            "pending: REVERB BLOOM (held: P0000002)",
            "pending: REVERB REVERSE (held: P0000002)",
        ]
    );

    // Held or not, an effect no demo plays still fails.
    let no_bloom: String = held_demos()
        .split("[[project]]")
        .filter(|t| !t.contains("REVERB BLOOM"))
        .collect::<Vec<_>>()
        .join("[[project]]");
    let cov = project::coverage(&project::parse(&no_bloom).unwrap(), &sounds).unwrap();
    assert_eq!(
        cov.unheard,
        [
            Effect::Reverb(ReverbType::Bloom),
            Effect::Reverb(ReverbType::Reverse)
        ]
    );
    assert_eq!(cov.pending.len(), 3);

    // On the page and in report.txt: HELD with its reason, its pending
    // lines, and no render; the run passes.
    let cov = project::coverage(&demos, &sounds).unwrap();
    let held = demo::held_report(&demos[0], &sounds, &cov).unwrap();
    assert!(held.renders.is_empty() && held.gates.is_empty());
    let d = scratch_ledger("ledger-held");
    assert!(page::write(&d, &[report(true, false)], &[], &[], &[held], true).unwrap());
    let page = std::fs::read_to_string(d.join("listening.html")).unwrap();
    assert!(page.contains("<h2>DEMO PROJECTS</h2>"), "{page}");
    assert!(
        page.contains("REVERB ROOMS <span class=\"badge held\">HELD</span>"),
        "{page}"
    );
    assert!(page.contains("Held: #499: HALL and BLOOM tails jump at block edges"));
    assert!(page.contains("pending: REVERB HALL (held: P0000001)"));
    let txt = std::fs::read_to_string(d.join("report.txt")).unwrap();
    assert!(
        txt.contains(
            "P0000001 REVERB ROOMS  HELD (#499: HALL and BLOOM tails jump at block edges)"
        ),
        "{txt}"
    );
    assert!(
        txt.contains("pending: REVERB QUAD (held: P0000001)"),
        "{txt}"
    );
    assert!(txt.contains("1 demo projects: all pass (1 held)"), "{txt}");
    let wavs = std::fs::read_dir(d.join("renders"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("P0000001-"))
        .count();
    assert_eq!(wavs, 0);
}

// ── the private build (spec § 3 `--private`, § 6) ──

/// A valid port Source; each bad test changes one line.
const PORT: &str = r#"[[sound]]
id = "S9100070"
name = "PORT ONE"
engine = "VA"
note = "A PORTED PAD"
demo = "poly"
plays = "long"
home = "C3"
FILTER.CUTOFF = "1200"
mod.ENV1 = "OUT.VCA +127"
phrase = "pad"
from = "tx81z 1a2b3c4d #12"
edits = "filter: 2040 instead of open"
"#;

fn port_with(n: usize, with: &str) -> String {
    PORT.lines()
        .enumerate()
        .map(|(i, l)| if i + 1 == n { with } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse, check and the ports' walls, as `build::sounds` runs them.
fn port_run(text: &str) -> Result<Vec<source::Source>, String> {
    let s = source::parse(text, "port.toml")?;
    source::check(&s)?;
    source::ports(&s)?;
    Ok(s)
}

fn port_fails(text: &str, line: usize, what: &str) {
    let e = port_run(text).expect_err("a bad port must fail");
    assert!(
        e.contains(&format!("port.toml:{line}:")) && e.contains(what),
        "want port.toml:{line} naming {what}, got: {e}"
    );
}

/// An empty dir under the test target (never the real ports dir).
fn scratch_dir(name: &str) -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A ports dir holding `PORT`, and a public dir holding `BASE`.
fn port_dirs(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let d = scratch_dir(name);
    let (public, ports) = (d.join("public-sources"), d.join("ports"));
    std::fs::create_dir_all(&public).unwrap();
    std::fs::create_dir_all(&ports).unwrap();
    std::fs::write(public.join("va.toml"), BASE).unwrap();
    std::fs::write(ports.join("one.toml"), PORT).unwrap();
    (public, ports)
}

#[test]
fn private_requires_from_and_edits() {
    let s = port_run(PORT).unwrap();
    assert_eq!(
        s[0].from.as_ref().map(|(l, f)| (*l, f.bank, f.voice)),
        Some((12, 0x1a2b3c4d, 12))
    );
    port_fails(&port_with(12, "# no from"), 2, "from");
    port_fails(&port_with(13, "# no edits"), 2, "edits");
    for bad in [
        "from = \"tx81z 1a2b3c4 #12\"",
        "from = \"tx81z 1a2b3c4g #12\"",
        "from = \"tx81z 1a2b3c4d\"",
        "from = \"tx81z 1a2b3c4d #0\"",
        "from = \"tx81z 1a2b3c4d #33\"",
        "from = \"dx7 1a2b3c4d #12\"",
        "from = \"tx81z 1a2b3c4d #12 BRASS\"",
    ] {
        port_fails(&port_with(12, bad), 12, "from");
    }
    // A port is private: never approved, never a placeholder.
    port_fails(
        &format!("{PORT}approved = \"round 2 1A2B3C4D\"\n"),
        2,
        "approved",
    );
    port_fails(
        &format!("{PORT}status = \"placeholder\"\n"),
        2,
        "placeholder",
    );
    // Through the build: a port dir with a Source missing `edits` fails.
    let (public, ports) = port_dirs("private-requires");
    std::fs::write(ports.join("one.toml"), port_with(13, "# no edits")).unwrap();
    let root = scratch_dir("private-requires-out");
    let e = build::private(&public, &ports, &root, &STAMP, None).unwrap_err();
    assert!(e.contains("one.toml:2:") && e.contains("edits"), "{e}");
    assert!(!root.join("private").exists());
    // A port named as a public Sound is refused with the public one.
    std::fs::write(
        ports.join("one.toml"),
        PORT.replace("PORT ONE\"", "TEST PAD\""),
    )
    .unwrap();
    let e = build::private(&public, &ports, &root, &STAMP, None).unwrap_err();
    assert!(e.contains("TEST PAD") && e.contains("va.toml"), "{e}");
}

#[test]
fn private_edits_name_a_category() {
    for ok in [
        "edits = \"filter: 2040 instead of open\"",
        "edits = \"fx: a touch\"",
        "edits = \"LFO: slower\"",
        "edits = \"lfo: slower\"",
        "edits = \"envelope: longer release\"",
        "edits = \"effects: hall send\"",
        "edits = \"filter: 2040; lfo: slower; envelope: softer attack\"",
    ] {
        let s = port_run(&port_with(13, ok)).unwrap_or_else(|e| panic!("{ok}: {e}"));
        assert!(!s[0].edits.as_ref().unwrap().items.is_empty());
    }
    let s = port_run(&port_with(13, "edits = \"filter: 2040; lfo: slower\"")).unwrap();
    let e = s[0].edits.as_ref().unwrap();
    assert_eq!(
        e.items.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![source::Edit::Filter, source::Edit::Lfo]
    );
    assert_eq!(e.text, "filter: 2040; lfo: slower");
    for bad in [
        "edits = \"\"",
        "edits = \"warmer\"",
        "edits = \"texture: warmer\"",
        "edits = \"filter:\"",
        "edits = \"filter: 2040; \"",
        "edits = \"filter: 2040; tone: darker\"",
    ] {
        port_fails(&port_with(13, bad), 13, "edits");
    }
}

#[test]
fn private_edits_formant_is_a_category() {
    let s = port_run(&port_with(13, "edits = \"formant: ops 5-6 sing an A\"")).unwrap();
    assert_eq!(
        s[0].edits.as_ref().unwrap().items[0].0,
        source::Edit::Formant
    );
}

#[test]
fn private_id_must_be_in_port_range() {
    for id in ["S9100069", "S9100100", "S9100002"] {
        port_fails(&port_with(2, &format!("id = \"{id}\"")), 2, id);
    }
    for id in ["S9100070", "S9100099"] {
        port_run(&port_with(2, &format!("id = \"{id}\""))).unwrap();
    }
    // The public walls still stand: no `from`, no port id.
    assert!(run(PORT).unwrap_err().contains("from"));
}

/// Every file under `d`, relative, with its bytes.
fn tree(d: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((
                    p.strip_prefix(d).unwrap().into(),
                    std::fs::read(&p).unwrap(),
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn private_never_writes_public_dir() {
    let (public, ports) = port_dirs("private-out");
    let root = scratch_dir("private-out-root");
    let pubdir = root.join("public");
    std::fs::create_dir_all(pubdir.join("CHIMERA/SOUNDS")).unwrap();
    std::fs::write(pubdir.join("manifest.json"), "public").unwrap();
    std::fs::write(pubdir.join("CHIMERA/SOUNDS/S9100002.A"), "public").unwrap();
    let before = (tree(&pubdir), tree(&public), tree(&ports));
    let man = build::private(&public, &ports, &root, &STAMP, None).unwrap();
    assert_eq!(
        man.sounds.iter().map(|e| e.id.get()).collect::<Vec<_>>(),
        vec![9_100_070]
    );
    assert!(man.projects.is_empty());
    assert_eq!(
        before,
        (tree(&pubdir), tree(&public), tree(&ports)),
        "public/ and the Sources untouched"
    );
    let built = tree(&root.join("private"));
    let names: Vec<_> = built
        .iter()
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["CHIMERA/SOUNDS/S9100070.A", "manifest.json"]);
    let mut root_entries: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    root_entries.sort();
    assert_eq!(root_entries, ["private", "public"]);
    // The built file is the port as a card file.
    let (files, _) = build::files(&port_run(PORT).unwrap(), &STAMP).unwrap();
    assert_eq!(built[0].1, files[0].1);
    // A rebuild empties private/ first.
    std::fs::write(root.join("private/CHIMERA/SOUNDS/S9100071.A"), "stale").unwrap();
    build::private(&public, &ports, &root, &STAMP, None).unwrap();
    assert!(!root.join("private/CHIMERA/SOUNDS/S9100071.A").exists());
    // A failed build leaves no old set to install.
    std::fs::write(ports.join("one.toml"), port_with(13, "# no edits")).unwrap();
    build::private(&public, &ports, &root, &STAMP, None).unwrap_err();
    assert!(!root.join("private").exists());
    assert!(root.join("public/manifest.json").is_file());
}

#[test]
fn private_missing_ports_dir_names_the_var() {
    // Unset, every private default is under the one private ledger.
    let private = std::path::Path::new(build::PRIVATE_LEDGER);
    assert_eq!(build::ports_dir(None), private.join("ports"));
    assert_eq!(build::banks_dir(None), private.join("banks"));
    assert_eq!(build::private_qa_ledger(), private.join("qa"));
    assert_eq!(
        build::ports_dir(Some("/x/ports".into())),
        std::path::Path::new("/x/ports")
    );
    // A ports dir that is not there names the variable.
    let missing = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-ports");
    let _ = std::fs::remove_dir_all(&missing);
    let e = build::ports(&missing).unwrap_err();
    assert!(
        e.contains("CHIMERA_TX_PORTS") && e.contains("no-such-ports"),
        "{e}"
    );
    let empty = scratch_dir("empty-ports");
    let e = build::ports(&empty).unwrap_err();
    assert!(
        e.contains("CHIMERA_TX_PORTS") && e.contains("empty-ports"),
        "{e}"
    );
    // The CLI, before it writes anything.
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_factory-sounds"))
        .arg("--private")
        .env("CHIMERA_TX_PORTS", &missing)
        .output()
        .unwrap();
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("CHIMERA_TX_PORTS") && err.contains("no-such-ports"),
        "{err}"
    );
}

/// `PORT_IDS`' WAVs in `renders/` under `ledger`.
fn port_wavs(ledger: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(ledger.join("renders"))
        .map(|d| {
            d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|n| {
                    n.get(1..8)
                        .and_then(|id| id.parse::<u32>().ok())
                        .is_some_and(|id| chimera_desktop::privacy::PORT_IDS.contains(&id))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn private_qa_stays_in_its_ledger() {
    let (public, ports) = port_dirs("private-qa");
    // Levelled, so both pass and render.
    for (p, f) in [(&public, "va.toml"), (&ports, "one.toml")] {
        let text = std::fs::read_to_string(p.join(f)).unwrap();
        let all = source::parse(&text, f).unwrap();
        std::fs::write(p.join(f), qa::levelled(&text, &all).unwrap()).unwrap();
    }
    let pub_ledger = scratch_ledger("private-qa-public");
    let priv_ledger = scratch_ledger("private-qa-private");
    // A port's render left in the public ledger by an older run.
    std::fs::create_dir_all(pub_ledger.join("renders")).unwrap();
    std::fs::write(pub_ledger.join("renders/S9100070-long.wav"), b"old").unwrap();
    // A port whose edits its bank refutes fails before anything renders.
    let e = run::qa(&run::Qa {
        dir: &public,
        ledger: pub_ledger.clone(),
        private: Some(run::Private {
            ports: &ports,
            ledger: priv_ledger.clone(),
            edits: &Edits(Err("one.toml:13: edits names filter".into())),
        }),
        wav: true,
        baseline: None,
    })
    .unwrap_err();
    assert!(e.contains("edits names filter"), "{e}");
    assert!(!priv_ledger.join("report.txt").exists());
    let r = run::qa(&run::Qa {
        dir: &public,
        ledger: pub_ledger.clone(),
        private: Some(run::Private {
            ports: &ports,
            ledger: priv_ledger.clone(),
            edits: &Edits(Ok(())),
        }),
        wav: true,
        baseline: None,
    });
    // The public ledger: never a port.
    assert_eq!(port_wavs(&pub_ledger), Vec::<String>::new());
    for f in ["listening.html", "report.txt"] {
        let t = std::fs::read_to_string(pub_ledger.join(f)).unwrap();
        assert!(
            !t.contains("PRIVATE") && !t.contains("PORT ONE") && !t.contains("S9100070"),
            "{f}: {t}"
        );
        assert!(t.contains("TEST PAD"), "{f}: {t}");
    }
    // The private ledger: the ports, under PRIVATE, and nothing public.
    let page = std::fs::read_to_string(priv_ledger.join("listening.html")).unwrap();
    let txt = std::fs::read_to_string(priv_ledger.join("report.txt")).unwrap();
    assert!(txt.contains("S9100070 PORT ONE  PRIVATE  PASS"), "{txt}");
    assert!(!txt.contains("TEST PAD") && !page.contains("TEST PAD"));
    let at = page.find("<h2>PRIVATE</h2>").expect("a PRIVATE section");
    assert!(page[at..].contains("PORT ONE"));
    assert!(page.contains("edits: filter: 2040 instead of open"));
    assert!(!page.contains("not verified"));
    assert!(!port_wavs(&priv_ledger).is_empty());
    r.unwrap();
}

/// An `EditsCheck` answering `self.0` for every port.
struct Edits(Result<(), String>);

impl build::EditsCheck for Edits {
    fn verify(&self, _: &source::Source) -> Result<(), String> {
        self.0.clone()
    }
}

/// `qa --private` never writes into the published ledger: a private
/// ledger that is it, inside it, or a link into it is refused before
/// anything is read or rendered.
#[test]
fn the_private_ledger_is_never_the_published_one() {
    let (public, ports) = port_dirs("private-guard");
    let pub_ledger = scratch_ledger("private-guard-public");
    std::fs::create_dir_all(&pub_ledger).unwrap();
    let link = scratch_ledger("private-guard-link");
    std::os::unix::fs::symlink(&pub_ledger, &link).unwrap();
    for bad in [
        pub_ledger.clone(),
        pub_ledger.join("qa"),
        link.clone(),
        link.join("qa"),
    ] {
        let e = run::qa(&run::Qa {
            dir: &public,
            ledger: pub_ledger.clone(),
            private: Some(run::Private {
                ports: &ports,
                ledger: bad.clone(),
                edits: &Edits(Ok(())),
            }),
            wav: true,
            baseline: None,
        })
        .unwrap_err();
        assert!(e.contains("published"), "{}: {e}", bad.display());
        assert!(!pub_ledger.join("report.txt").exists(), "nothing written");
    }
    let _ = std::fs::remove_file(&link);
}
