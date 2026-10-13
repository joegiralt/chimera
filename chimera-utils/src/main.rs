//! chimera-utils: A/B Chimera's DSP against external references (ADR 0098).
//! Host only; renders in-process through chimera-core.

// The workspace bans float `clamp` for the firmware's flash (it links float
// formatting); this tool never ships.
#![allow(clippy::disallowed_methods)]

mod devices;
mod generic;
mod grain;
mod metrics;
mod models;
mod models_burst;
mod mv;
mod plot;
mod reference;
mod sig;
mod stim;
mod voice;

use chimera_core::dsp::reverb::ReverbType;
use std::path::PathBuf;

/// The one rate everything runs at: Chimera's.
pub const SR: f64 = chimera_hal::SAMPLE_RATE as f64;

const USAGE: &str = "\
usage: chimera-utils [flags] COMMAND …
  ab REF DEVICE [k=v …]    REF vs a Chimera DEVICE: null, response, THD, centroid; A/B WAVs
                           (an effect also on the listening set)
  devices                  the devices and their knobs
  mv-refs [PROG …]         MidiVerb II references (default 1–49)
  mv-fit TYPE PROG …       fit TYPE's knobs to programs
  mv-verify TYPE           re-measure the fits: residuals, PASS/FAIL
  mv-start [TYPE …]        each TYPE's START against its source program
  mv-ab TYPE               RMS-matched A/B WAVs on the listening set:
                           snare bass stab sine_short sine_long texture hats
  mv-structure PROG …      a program's structure from the decompiled listing
  voice-stats WAV …        loudness, vibrato, drift, breath share, formants and
                           vowel motion of each WAV (with --json PATH, as JSON)
  voice-plot OUT.svg WAV … waveforms, spectra and envelope-peak tracks side by
                           side (--at S, --f0 HZ, --periods N, --top HZ)
  model-check hall FITS    the Python model's HALL fits through the Rust model
  model-fit hall TAG [dens=own|fixed:X|KNOB[-][:lo:hi]] [warm=FITS] PROG …
                           fit the proposed HALL model (ADR 0095) to programs
  model-taps hall FITS [dens=…] [taps=FILE]
                           search HALL's tap fractions, the fits' knobs held
  model-fit VARIANT PROG … fit a `models` sketch (BURST v2) to programs
flags:
  --jobs N        worker threads (default 8)
  --out DIR       outputs (env MV_OUT, default target/mv)
  --emu PATH      MidiVerb emulator (env MIDIVERB_EMU)
  --listing PATH  decompiled-midiverb2.h (env MV_LISTING, default beside the emulator)
  --refs-dir DIR  reference programs for `ab` (env CHIMERA_REFS)
  --refs PATH     reference metrics.json (default OUT/refs/metrics.json)
  --fits PATH     mv-verify: fits from elsewhere (results go to OUT/fits/TYPE_verify.json)
  --seeds N       mv-fit: noise seeds per search step (default 8; verify uses 16)
  --start PATH    mv-fit: search from these fits' knobs (finer steps, SMEAR held)
  --steps A,B,…   mv-fit --start: the pattern steps (default 0.03,0.012)
  --redo          mv-fit: refit presets already fitted, and re-sweep
  --robust        mv-verify: also nudge every fitted knob ±0.001 (a report)
  --size-worst    mv-fit: score each setting by the worst of it and SIZE ±0.001 (3× slower)
  --no-sources    mv-refs: skip the listening-set renders
  --json PATH     voice-stats: also write the rows as JSON
  --at S, --f0 HZ, --periods N, --top HZ
                  voice-plot: the moment (default 1), the note (130.81), the
                  periods shown (3) and the spectrum's top (6000)";

pub struct Config {
    pub out: PathBuf,
    emu: Option<PathBuf>,
    listing: Option<PathBuf>,
    pub refs_dir: Option<PathBuf>,
    pub refs: Option<PathBuf>,
    /// Noise seeds per measurement in `mv-fit`'s pattern search.
    pub refine_seeds: usize,
    /// `mv-fit --start`'s pattern steps, coarse to fine.
    pub steps: Option<Vec<f64>>,
    /// `mv-fit` scores a setting by the worst of it and SIZE ±0.001.
    pub size_worst: bool,
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl Config {
    pub fn emu(&self) -> Result<PathBuf, String> {
        self.emu
            .clone()
            .ok_or_else(|| "set MIDIVERB_EMU (or --emu) to the MidiVerb emulator".into())
    }

    pub fn listing(&self) -> Result<PathBuf, String> {
        match &self.listing {
            Some(p) => Ok(p.clone()),
            None => Ok(self.emu()?.with_file_name("decompiled-midiverb2.h")),
        }
    }

    pub fn tmp(&self) -> PathBuf {
        self.out.join("tmp")
    }
}

fn nums(args: &[String]) -> Result<Vec<u32>, String> {
    args.iter()
        .map(|a| {
            a.parse()
                .map_err(|_| format!("`{a}`: not a program number"))
        })
        .collect()
}

fn run() -> Result<bool, String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut flag = |name: &str| -> Result<Option<String>, String> {
        match args.iter().position(|a| a == name) {
            Some(i) if i + 1 < args.len() => {
                let v = args.remove(i + 1);
                args.remove(i);
                Ok(Some(v))
            }
            Some(_) => Err(format!("{name} needs a value")),
            None => Ok(None),
        }
    };
    let jobs: usize =
        flag("--jobs")?.map_or(Ok(8), |v| v.parse().map_err(|_| "--jobs: not a number"))?;
    let path = |v: Option<String>, env: &str| v.map(PathBuf::from).or_else(|| env_path(env));
    let mut cfg = Config {
        out: path(flag("--out")?, "MV_OUT").unwrap_or_else(|| PathBuf::from("target/mv")),
        emu: path(flag("--emu")?, "MIDIVERB_EMU"),
        listing: path(flag("--listing")?, "MV_LISTING"),
        refs_dir: path(flag("--refs-dir")?, "CHIMERA_REFS"),
        refs: flag("--refs")?.map(PathBuf::from),
        refine_seeds: flag("--seeds")?
            .map_or(Ok(8), |v| v.parse().map_err(|_| "--seeds: not a number"))?,
        steps: flag("--steps")?
            .map(|v| {
                v.split(',')
                    .map(|x| x.parse().map_err(|_| "--steps: numbers, comma-separated"))
                    .collect::<Result<Vec<f64>, _>>()
            })
            .transpose()?,
        size_worst: false,
    };
    let fits = flag("--fits")?.map(PathBuf::from);
    let start = flag("--start")?.map(PathBuf::from);
    let json = flag("--json")?.map(PathBuf::from);
    let num = |v: Option<String>, d: f64, f: &str| {
        v.map_or(Ok(d), |v| {
            v.parse().map_err(|_| format!("{f}: not a number"))
        })
    };
    let at = num(flag("--at")?, 1.0, "--at")?;
    let f0 = num(flag("--f0")?, 130.81, "--f0")?;
    let periods = num(flag("--periods")?, 3.0, "--periods")?;
    let top = num(flag("--top")?, 6_000.0, "--top")?;
    let mut switch = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .map(|i| args.remove(i))
            .is_some()
    };
    let (redo, no_sources, robust) = (switch("--redo"), switch("--no-sources"), switch("--robust"));
    cfg.size_worst = switch("--size-worst");
    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .stack_size(16 << 20)
        .build_global()
        .map_err(|e| e.to_string())?;
    let Some((cmd, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let ty = |i: usize| {
        rest.get(i)
            .ok_or_else(|| USAGE.to_string())
            .and_then(|s| devices::reverb_type(s))
    };
    let ok = match cmd.as_str() {
        "ab" if rest.len() >= 2 => generic::ab(&cfg, &rest[0], &rest[1], &rest[2..]).map(|_| true),
        "devices" => {
            for (name, d) in devices::Device::all() {
                let ks: Vec<String> = d
                    .knobs()
                    .iter()
                    .map(|k| match k.choices {
                        [] => format!("{}={}", k.name, k.default),
                        c => format!("{}={}", k.name, c.join("|")),
                    })
                    .collect();
                println!("{name:14} {}", ks.join(" "));
            }
            Ok(true)
        }
        "mv-refs" => {
            let p = nums(rest)?;
            let p = if p.is_empty() { (1..=49).collect() } else { p };
            mv::refs(&cfg, &p, !no_sources).map(|_| true)
        }
        "mv-fit" if rest.len() >= 2 => {
            mv::fit(&cfg, ty(0)?, &nums(&rest[1..])?, redo, start.as_deref()).map(|_| true)
        }
        "mv-verify" => mv::verify(&cfg, ty(0)?, fits.as_deref()).and_then(|ok| {
            // A report, not a gate (ADR 0095).
            if robust {
                mv::robust_verify(&cfg, ty(0)?, fits.as_deref())?;
            }
            Ok(ok)
        }),
        "mv-start" => {
            let types = if rest.is_empty() {
                ReverbType::ALL.to_vec()
            } else {
                rest.iter()
                    .map(|s| devices::reverb_type(s))
                    .collect::<Result<_, _>>()?
            };
            mv::start(&cfg, &types)
        }
        "mv-ab" => mv::ab(&cfg, ty(0)?).map(|_| true),
        "mv-structure" => mv::structure(&cfg, &nums(rest)?).map(|_| true),
        "voice-stats" if !rest.is_empty() => voice::stats(rest, json.as_deref()).map(|_| true),
        "voice-plot" if rest.len() >= 2 => {
            plot::plot(&rest[0], &rest[1..], at, f0, periods, top).map(|_| true)
        }
        "model-check" if rest.len() == 2 && rest[0] == "hall" => {
            models::check(&cfg, std::path::Path::new(&rest[1])).map(|_| true)
        }
        "model-taps" if rest.len() >= 2 && rest[0] == "hall" => {
            models::taps_search(&cfg, std::path::Path::new(&rest[1]), &rest[2..]).map(|_| true)
        }
        "model-fit" if rest.len() >= 3 && rest[0] == "hall" => {
            let (opts, progs): (Vec<String>, Vec<String>) =
                rest[2..].iter().cloned().partition(|a| a.contains('='));
            models::fit(&cfg, &rest[1], &opts, &nums(&progs)?).map(|_| true)
        }
        "model-compare" if rest.len() >= 3 => {
            models_burst::compare(&cfg, &rest[0], &rest[1], &nums(&rest[2..])?).map(|_| true)
        }
        "model-fit" if rest.len() >= 2 => {
            models_burst::fit(&cfg, &rest[0], &nums(&rest[1..])?).map(|_| true)
        }
        _ => Err(USAGE.into()),
    };
    let _ = std::fs::remove_dir(cfg.tmp());
    ok
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
