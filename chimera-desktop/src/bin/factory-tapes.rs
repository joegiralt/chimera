//! `just factory-tapes`: the TAPE factory set (spec § 7) into
//! `target/tapes-v1/`. `factory-tapes qa [--wav <dir>]` plays every source
//! through every machine (LOOP and STRETCH also with END before START,
//! ONESHOT under SLICE NOTE, LOOP with WARBLE), each macro swept as
//! `tape-listen` sweeps it, and checks each render (`listen::qa`); with
//! `--wav`, each render is written there.

use chimera_core::block::DiskCode;
use chimera_core::block::ParamId;
use chimera_core::dsp::tape_engine::TapeEngineParams as P;
use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::dsp::tape_engine::{Direction, StretchType, TapeMachine, TapeModel, Tron};
use chimera_desktop::factory_tapes::{
    ROOT, VERSION, build_all, held, originals, pitch, read_recipes, tape_sound, write_set,
};
use chimera_desktop::import::tape::free_beside_new;
use chimera_desktop::listen::{
    CLICK_RATIO, SILENT_DBFS, Script, Way, model_slug, qa, qa_takes, render, root_note, script,
    take_starts, write_wav,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        None | Some("build") => build(),
        Some("qa") => run_qa(args.get(1).filter(|a| *a == "--wav").and(args.get(2))),
        Some("demo") => args
            .get(1)
            .ok_or("demo <dir>".to_string())
            .and_then(|d| demo(d)),
        Some(a) => Err(format!(
            "factory-tapes [build | qa [--wav <dir>] | demo <dir>], not {a}"
        )),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("factory-tapes: {e}");
            ExitCode::from(2)
        }
    }
}

fn build() -> Result<bool, String> {
    let r = read_recipes()?;
    let dir = originals();
    let built = build_all(&r, &dir)?;
    if let Some(s) = r.sources.iter().zip(&built).find(|(_, b)| b.is_none()) {
        return Err(format!(
            "{}: its CC0 original isn't in {}",
            s.0.name.as_str(),
            dir.display()
        ));
    }
    let out = PathBuf::from(format!("{ROOT}/target/{VERSION}"));
    write_set(&r, &built, &dir, &out)?;
    let mut total = 0;
    for b in built.iter().flatten() {
        let h = &b.head;
        let bytes = b.packed.len() + 16;
        total += bytes;
        println!(
            "{:08X}.SRC  {:<16} {:>5} Hz {:>6} frames {:>5.2} s {:>6} B  {}",
            h.sref.crc,
            h.name.as_str(),
            h.sref.rate,
            h.sref.frames,
            h.sref.frames as f32 / f32::from(h.sref.rate),
            bytes,
            match h.shape {
                Shape::Transwave => format!("TRANSWAVE {}", h.count),
                _ if h.period_q16 == 0 => "unpitched".into(),
                _ => format!("root {} cents", h.root),
            }
        );
    }
    println!(
        "{} sources, {} KB held; {} Sounds; {} B free beside NEW\n{}",
        built.len(),
        total / 1024,
        r.sounds.len(),
        free_beside_new().bytes(),
        out.display()
    );
    Ok(true)
}

/// The engine's peak before the chain (spec § 9's model sweeps): a source
/// peaks at −0.5 dBFS, and the read's interpolation and STRETCH's splices
/// may pass 0 dBFS; the VCA and the limiter follow.
const PEAK_DBFS: f32 = 6.0;

fn run_qa(wav: Option<&String>) -> Result<bool, String> {
    let r = read_recipes()?;
    let built = build_all(&r, &originals())?;
    if let Some(w) = wav {
        std::fs::create_dir_all(w).map_err(|e| e.to_string())?;
    }
    let (mut report, mut pass, mut n) = (String::new(), true, 0);
    for b in built.iter().flatten() {
        let (h, held) = (&b.head, held(b));
        let p = pitch(h);
        let slug = h.name.as_str().to_lowercase().replace(' ', "-");
        let wave = h.shape == Shape::Transwave;
        // The source played straight (ONESHOT, neutral, at its root): a
        // machine passes if it adds no click the source hasn't got.
        let own = if wave {
            0.0
        } else {
            let base = Way::One.base(TapeModel::Walk, StretchType::Mono);
            let s = Way::One.script(TapeModel::Walk);
            let y = render(base, &held, p, root_note(p), &s);
            qa_takes(&y[..y.len().min(48_000 * 5 / 2)], &take_starts(&s)).click_ratio
        };
        let _ = writeln!(report, "{slug}: played straight, click {own:.2}x");
        for m in TapeModel::ALL
            .into_iter()
            .filter(|&m| wave == (m == TapeModel::Transwave))
        {
            let ways: &[Way] = if wave { &[Way::Plain] } else { Way::of(m) };
            for &way in ways {
                let kinds: &[StretchType] = if m == TapeModel::Stretch && way == Way::Plain {
                    &[StretchType::Mono, StretchType::Poly]
                } else {
                    &[StretchType::Mono]
                };
                for &k in kinds {
                    let note = if wave { 48 } else { way.note(root_note(p)) };
                    let s = if wave { script(m) } else { way.script(m) };
                    let y = render(way.base(m, k), &held, p, note, &s);
                    let q = qa_takes(&y, &take_starts(&s));
                    let ok = q.non_finite == 0
                        && q.peak_dbfs > SILENT_DBFS
                        && q.peak_dbfs <= PEAK_DBFS
                        && q.click_ratio <= CLICK_RATIO.max(own * 1.05);
                    pass &= ok;
                    n += 1;
                    let row = format!(
                        "{}-{slug}{}{}",
                        model_slug(m),
                        if m == TapeModel::Stretch {
                            format!("-{}", k.disk_ident().to_lowercase())
                        } else {
                            String::new()
                        },
                        way.slug()
                    );
                    let _ = writeln!(
                        report,
                        "{row:<36} peak {:6.2} dBFS  click {:5.2}x at {:6.3} s  non-finite {}  {}",
                        q.peak_dbfs,
                        q.click_ratio,
                        q.click_at as f32 / 48_000.0,
                        q.non_finite,
                        if ok { "PASS" } else { "FAIL" }
                    );
                    if let Some(w) = wav {
                        write_wav(&PathBuf::from(w).join(format!("{row}.wav")), &y)?;
                    }
                }
            }
        }
    }
    let _ = writeln!(
        report,
        "{n} renders: {}",
        if pass { "all pass" } else { "FAIL" }
    );
    let out = PathBuf::from(format!("{ROOT}/target/{VERSION}-qa.txt"));
    std::fs::write(&out, &report).map_err(|e| e.to_string())?;
    print!("{report}");
    println!("{}", out.display());
    Ok(pass)
}

/// The owner's short listening set (A/B listening stimuli): each machine
/// on two factory Sounds, `(Sound, MODEL in its place, semitones of each
/// take from the root, 1.5 s apart, and a param ramped over the render)`.
type Demo = (
    &'static str,
    Option<(TapeModel, Direction, Tron)>,
    &'static [u8],
    Option<(ParamId, f32, f32)>,
);

const DEMOS: [Demo; 14] = [
    ("TX BASS HIT", None, &[0, 0], None),
    // SLICE NOTE from C4: kick, snare, closed hat, open hat.
    ("KIT", None, &[0, 2, 6, 7], None),
    ("SINE LOOP", None, &[0, 12], None),
    ("PAD LOOP", None, &[0, 5], None),
    ("TRON BOWED", None, &[0, 12], None),
    (
        "BOWED LOOP",
        Some((TapeModel::Walk, Direction::Forward, Tron::On)),
        &[0, 7],
        None,
    ),
    ("STAB STRETCH", None, &[0, 0], Some((P::MOTION, 0.25, 1.0))),
    ("VOICE STRETCH", None, &[0, 0], Some((P::MOTION, 0.25, 1.0))),
    ("WAVE MORPH", None, &[0, 7], Some((P::POSITION, 0.0, 1.0))),
    ("WAVE SAW", None, &[0, 7], Some((P::POSITION, 0.0, 1.0))),
    ("RAIN SCRUB", None, &[0, 0], Some((P::MOTION, 0.6, 0.1))),
    ("SAW SCRUB", None, &[0, 0], Some((P::MOTION, 0.3, 0.8))),
    ("PAD PINGPONG", None, &[0, 0], Some((P::SIZE, 0.5, 0.15))),
    (
        "SINE LOOP",
        Some((TapeModel::Walk, Direction::Ping, Tron::Off)),
        &[0, 0],
        Some((P::SIZE, 0.6, 0.2)),
    ),
];

/// Each demo, 1.5 s a take, to `<dir>/<model>-<sound>.wav` at its Sound's
/// trim (−8 dB of headroom), with `manifest.json`.
fn demo(dir: &str) -> Result<bool, String> {
    let r = read_recipes()?;
    let built = build_all(&r, &originals())?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut man = String::from("[\n");
    for (k, &(name, model, notes, ramp)) in DEMOS.iter().enumerate() {
        let s = r
            .sounds
            .iter()
            .find(|s| s.name.as_str() == name)
            .ok_or(name)?;
        let b = r
            .sources
            .iter()
            .zip(&built)
            .find(|(x, _)| x.name == s.source)
            .and_then(|(_, b)| b.as_ref())
            .ok_or(name)?;
        let snd = tape_sound(s, &b.head);
        let mut base = snd.params.tape;
        if let Some((m, d, t)) = model {
            (base.machine, base.direction, base.tron) = (TapeMachine::of(m), d, t);
        }
        let secs = 1.5 * notes.len() as f32;
        let points: Vec<(f32, ParamId, f32)> = match ramp {
            Some((id, a, z)) => vec![(0.0, id, a), (secs, id, z)],
            None => vec![(secs, P::TAPE, base.tape)],
        };
        let takes: Vec<f32> = (1..notes.len()).map(|i| 1.5 * i as f32).collect();
        let sc = Script(
            Box::leak(points.into_boxed_slice()),
            Box::leak(takes.into_boxed_slice()),
            notes,
        );
        let p = pitch(&b.head);
        let note = if base.slice == chimera_core::dsp::tape_engine::SliceCode::NOTE {
            60
        } else {
            root_note(p).max(36)
        };
        let note = if b.head.shape == Shape::Transwave {
            48
        } else {
            note
        };
        let gain = 0.4 * 10f32.powf(s.trim_db / 20.0);
        let y: Vec<f32> = render(base, &held(b), p, note, &sc)
            .iter()
            .map(|v| v * gain)
            .collect();
        let q = qa(&y);
        let file = format!(
            "{:02}-{}-{}.wav",
            k + 1,
            model_slug(base.model()),
            name.to_lowercase().replace(' ', "-")
        );
        write_wav(&PathBuf::from(dir).join(&file), &y)?;
        let _ = write!(
            man,
            "{}{{\"file\": \"{}\", \"machine\": \"{}\", \"sound\": \"{name}\", \"source\": \"{}\", \"note\": {note}, \"takes\": {:?}, \"ramp\": \"{}\", \"peak_dbfs\": {:.2}, \"click\": {:.2}}}",
            if k == 0 { "" } else { ",\n" },
            file.replace(".wav", ".oga"),
            chimera_desktop::factory_tapes::model_word(base.model()),
            s.source.as_str(),
            notes,
            ramp.map_or(String::from("none"), |(id, a, z)| format!(
                "{} {a} to {z}",
                [
                    "MODEL", "START", "END", "POSITION", "MOTION", "TAPE", "AGE", "SIZE"
                ]
                .get(usize::from(id.0))
                .unwrap_or(&"?")
            )),
            q.peak_dbfs,
            q.click_ratio
        );
        println!(
            "{file}: peak {:.2} dBFS, click {:.2}x",
            q.peak_dbfs, q.click_ratio
        );
    }
    man.push_str("\n]\n");
    std::fs::write(PathBuf::from(dir).join("manifest.json"), man).map_err(|e| e.to_string())?;
    Ok(true)
}
