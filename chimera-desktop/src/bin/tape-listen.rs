//! `just tape-listen <model>… [--src chimera|<path.wav>]…`: each model on
//! each source into `target/listen/` (48 kHz, 16-bit, mono WAVs), with
//! `report.txt` and, when every render passes QA, `index.html`.
//!
//! `chimera` is two Chimera renders: factory slot 0 (ALGO) and INIT MODAL.
//! A WAV must be credited in `tools/factory-tapes/listen.toml`; a bare file
//! name is looked for in `$CHIMERA_TAPES_SRC` (`~/chimera-tapes-src`).

use chimera_core::block::DiskCode;
use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::TapeModel;
use chimera_core::factory::FACTORY_LEN;
use chimera_desktop::import::tape::convert::{Channel, Mono, Pitch, read_wav};
use chimera_desktop::listen::{
    Src, chimera_source, credits, dry, file_name, held_from, model_slug, page, qa, render,
    root_note, script,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The rate sources are held at: the importer's default (spec § 6).
const RATE: u32 = 24_000;
/// A Chimera source's note and hold.
const NOTE: u8 = 48;
const HOLD_S: f32 = 2.0;
/// The WAVs' ends fade over 10 ms, so a player stopping there never clicks.
const FADE: usize = 480;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("tape-listen: {e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let (mut models, mut srcs) = (Vec::new(), Vec::new());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--src" {
            srcs.push(args.next().ok_or("--src needs chimera or a WAV")?);
        } else {
            let m = TapeModel::ALL
                .into_iter()
                .find(|&m| model_slug(m) == a.to_lowercase())
                .ok_or(format!("no model {a}"))?;
            models.push(m);
        }
    }
    if models.is_empty() || srcs.is_empty() {
        return Err("usage: tape-listen <model>… --src chimera|<path.wav>…".into());
    }
    let text = std::fs::read_to_string(format!("{ROOT}/tools/factory-tapes/listen.toml"))
        .map_err(|e| format!("listen.toml: {e}"))?;
    let credits = credits(&text)?;
    let mut sources = Vec::new();
    for s in &srcs {
        if s == "chimera" {
            sources.push(chimera(0, "TX BASS", "tx-bass"));
            sources.push(chimera(FACTORY_LEN + 1, "INIT MODAL", "init-modal"));
        } else {
            sources.push(wav(s, &credits)?);
        }
    }

    let out = PathBuf::from(format!("{ROOT}/target/listen"));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let index = out.join("index.html");
    // A page from an earlier run never outlives this one's QA.
    let _ = std::fs::remove_file(&index);
    let (mut report, mut rows, mut pass) = (String::new(), Vec::new(), true);
    for (src, mono) in &sources {
        let (held, pitch) = held_from(mono, RATE).map_err(|e| format!("{}: {e}", src.slug))?;
        let note = root_note(pitch);
        let pitch_txt = match pitch {
            Pitch::Pitched { period, root_cents } => {
                format!("root {root_cents} cents, period {period:.2} frames")
            }
            Pitch::Unpitched => "unpitched".into(),
        };
        let _ = writeln!(
            report,
            "{}: {} at {RATE} Hz, {} B held, {pitch_txt}, played on note {note}",
            src.slug,
            src.title,
            held.len()
        );
        let d = dry(mono);
        for &m in &models {
            let wet = render(m, &held, pitch, note, &script(m));
            for (kind, x) in [("dry", &d), ("wet", &wet)] {
                let q = qa(x);
                pass &= q.passes();
                let _ = writeln!(
                    report,
                    "{:<9} {:<12} {kind}  peak {:6.2} dBFS  click {:5.2}x at {:6.3} s  non-finite {}  {}",
                    m.disk_ident(),
                    src.slug,
                    q.peak_dbfs,
                    q.click_ratio,
                    q.click_at as f32 / chimera_hal::SAMPLE_RATE as f32,
                    q.non_finite,
                    if q.passes() { "PASS" } else { "FAIL" }
                );
                write_wav(&out.join(file_name(m, src, kind)), x)?;
            }
            rows.push((m, src));
        }
    }
    let verdict = if pass {
        "all pass"
    } else {
        "FAIL: no page written"
    };
    let _ = writeln!(report, "{verdict}");
    std::fs::write(out.join("report.txt"), &report).map_err(|e| e.to_string())?;
    print!("{report}");
    if pass {
        std::fs::write(&index, page(&rows)).map_err(|e| e.to_string())?;
        let index = std::fs::canonicalize(&index).unwrap_or(index);
        println!("{}", index.display());
    }
    Ok(pass)
}

fn chimera(slot: usize, title: &str, slug: &str) -> (Src, Mono) {
    let src = Src {
        slug: slug.into(),
        title: title.into(),
        author: "Chimera".into(),
        licence: "Chimera's own render".into(),
        url: None,
    };
    (src, chimera_source(slot, NOTE, HOLD_S))
}

fn wav(arg: &str, credits: &[(String, Src)]) -> Result<(Src, Mono), String> {
    let mut path = PathBuf::from(arg);
    if !path.exists() && path.components().count() == 1 {
        let dir = std::env::var("CHIMERA_TAPES_SRC").unwrap_or_else(|_| {
            format!(
                "{}/chimera-tapes-src",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        path = Path::new(&dir).join(arg);
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(format!("{arg}: no file name"))?;
    let src = credits
        .iter()
        .find(|c| c.0 == name)
        .map(|c| c.1.clone())
        .ok_or(format!("{name}: not credited in listen.toml"))?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mono = read_wav(&bytes, Channel::Mix).map_err(|e| format!("{name}: {e:?}"))?;
    Ok((src, mono))
}

fn write_wav(path: &Path, x: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: chimera_hal::SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    let n = x.len();
    for (i, &s) in x.iter().enumerate() {
        let fade = ((n - i) as f32 / FADE as f32).min(1.0);
        let v = clampf(s * fade, -1.0, 1.0) * 32_767.0;
        w.write_sample(v.round() as i16).map_err(err)?;
    }
    w.finalize().map_err(err)
}
