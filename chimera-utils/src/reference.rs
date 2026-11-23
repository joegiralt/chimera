// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! External references: programs built outside the repo, run as
//! subprocesses (ADR 0098). No reference code is linked into this tool.
//!
//! The contract (README): `REF out=OUT.wav sr=RATE [in=IN.wav] [frames=N]
//! [key=value …]`. `in` is a mono 32-bit float WAV at `sr` (absent for a
//! source, which renders `frames` samples instead); the reference writes
//! `out` at `sr`, any PCM or float WAV, mono or stereo (averaged).
//!
//! The MidiVerb II emulator predates the contract; `MidiVerb` adapts it:
//! 48 kHz ↔ 23,400 Hz and its 16-bit stereo I/O.

use crate::sig::resample_poly;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MV_SR: u32 = 23_400;

pub enum Reference {
    /// A program on the contract.
    Exe(PathBuf),
    /// The MidiVerb II emulator (`midiverb N in.wav out.wav`).
    MidiVerb(PathBuf),
}

/// A scratch directory under `root`, removed when dropped.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(root: &Path) -> Result<Self, String> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = root.join(format!(
            "{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Self(p))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A WAV's samples as f64, channels averaged (integers at 1/2^(bits−1), as
/// soundfile reads them), and its rate.
pub fn read_wav(path: &Path) -> Result<(Vec<f64>, u32), String> {
    let (chans, sr) = read_channels(path)?;
    let n = chans.len() as f64;
    let x = (0..chans[0].len())
        .map(|i| chans.iter().map(|c| c[i]).sum::<f64>() / n)
        .collect();
    Ok((x, sr))
}

/// A WAV's channels, each as f64 (as `read_wav` scales them), and its rate.
pub fn read_channels(path: &Path) -> Result<(Vec<Vec<f64>>, u32), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = r.spec();
    let ch = spec.channels as usize;
    if ch == 0 || !(1..=32).contains(&spec.bits_per_sample) {
        return Err(format!("{}: not a usable WAV ({spec:?})", path.display()));
    }
    // A header promising more than the file holds (a writer that died).
    let bytes = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if u64::from(r.len()) * u64::from(spec.bits_per_sample).div_ceil(8) > bytes {
        return Err(format!("{}: truncated", path.display()));
    }
    let raw: Vec<f64> = match spec.sample_format {
        hound::SampleFormat::Float => r
            .samples::<f32>()
            .map(|s| s.map(f64::from))
            .collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f64;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f64 * scale))
                .collect::<Result<_, _>>()
        }
    }
    .map_err(|e| format!("{}: {e}", path.display()))?;
    let whole = &raw[..raw.len() / ch * ch];
    let chans = (0..ch)
        .map(|c| whole.iter().skip(c).step_by(ch).copied().collect())
        .collect();
    Ok((chans, spec.sample_rate))
}

pub fn write_f32(path: &Path, x: &[f32], sr: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    for &s in x {
        w.write_sample(s).map_err(err)?;
    }
    w.finalize().map_err(err)
}

/// Mono 24-bit PCM (the A/B files).
pub fn write_pcm24(path: &Path, x: &[f64], sr: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    for &s in x {
        w.write_sample((s * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32)
            .map_err(err)?;
    }
    w.finalize().map_err(err)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd:?}: {}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

impl Reference {
    /// `name` is `midiverb` (the emulator `emu` gives), a path, or a program
    /// in `refs_dir`.
    /// A bare name is looked up in `refs_dir`, else the current directory,
    /// never on `PATH`.
    pub fn find(
        name: &str,
        emu: impl FnOnce() -> Result<PathBuf, String>,
        refs_dir: Option<&Path>,
    ) -> Result<Self, String> {
        if name.eq_ignore_ascii_case("midiverb") {
            return Ok(Reference::MidiVerb(emu()?));
        }
        let direct = PathBuf::from(name);
        let p = if direct.components().count() > 1 {
            direct
        } else {
            refs_dir.unwrap_or(Path::new(".")).join(name)
        };
        if p.is_file() {
            Ok(Reference::Exe(p))
        } else {
            Err(format!(
                "no reference program {} (set CHIMERA_REFS or give a path)",
                p.display()
            ))
        }
    }

    /// Renders at 48 kHz: of `input` (an effect) or `frames` samples (a
    /// source), with `params` passed through. Exactly that many samples come
    /// back; a reference that returns fewer is an error.
    pub fn render(
        &self,
        params: &[(String, String)],
        input: Option<&[f64]>,
        frames: usize,
        tmp: &Path,
    ) -> Result<Vec<f64>, String> {
        let sr = crate::SR as u32;
        let s = Scratch::new(tmp)?;
        match self {
            Reference::Exe(exe) => {
                let mut cmd = Command::new(exe);
                cmd.arg(format!("out={}", s.path("out.wav").display()))
                    .arg(format!("sr={sr}"));
                match input {
                    Some(x) => {
                        let xf: Vec<f32> = x.iter().map(|&v| v as f32).collect();
                        write_f32(&s.path("in.wav"), &xf, sr)?;
                        cmd.arg(format!("in={}", s.path("in.wav").display()));
                    }
                    None => {
                        cmd.arg(format!("frames={frames}"));
                    }
                }
                for (k, v) in params {
                    cmd.arg(format!("{k}={v}"));
                }
                run(&mut cmd)?;
                let (mut y, got) = read_wav(&s.path("out.wav"))?;
                if got != sr {
                    return Err(format!("{}: wrote {got} Hz, asked {sr}", exe.display()));
                }
                let want = input.map_or(frames, <[f64]>::len);
                if y.len() < want {
                    return Err(format!(
                        "{}: wrote {} samples, asked {want}",
                        exe.display(),
                        y.len()
                    ));
                }
                y.truncate(want);
                Ok(y)
            }
            Reference::MidiVerb(emu) => {
                let x = input.ok_or("midiverb needs an input")?;
                let prog: u32 = params
                    .iter()
                    .find(|(k, _)| k == "prog")
                    .ok_or("midiverb needs prog=N")?
                    .1
                    .parse()
                    .map_err(|_| "prog: not a number")?;
                let normalise = !params.iter().any(|(k, v)| k == "normalise" && v == "0");
                mv_render(emu, prog, x, normalise, &s)
            }
        }
    }
}

/// Program `prog`, 100 % wet, of 48 kHz mono `x`: 48 kHz mono (L+R)/2 at
/// `x`'s scale. With `normalise` the drive is set to −1 dBFS and undone;
/// without, `x` drives the unit as is (1.0 full scale).
fn mv_render(
    emu: &Path,
    prog: u32,
    x: &[f64],
    normalise: bool,
    s: &Scratch,
) -> Result<Vec<f64>, String> {
    let pk = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let g = if normalise { 0.89 / pk } else { 1.0 };
    let scaled: Vec<f64> = x.iter().map(|v| v * g).collect();
    let u = resample_poly(&scaled, 39, 80);
    let n_mv = u.len();
    let (a, b) = (s.path("in.wav"), s.path("out.wav"));
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: MV_SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let err = |e: hound::Error| format!("{}: {e}", a.display());
    let mut w = hound::WavWriter::create(&a, spec).map_err(err)?;
    for v in u {
        let v = v.clamp(-1.0, 32767.0 / 32768.0);
        let q = (v * 32768.0).round_ties_even().clamp(-32768.0, 32767.0) as i16;
        w.write_sample(q).map_err(err)?;
        w.write_sample(q).map_err(err)?;
    }
    w.finalize().map_err(err)?;
    run(Command::new(emu).arg(prog.to_string()).arg(&a).arg(&b))?;
    let (y, sr) = read_wav(&b)?;
    if sr != MV_SR {
        return Err(format!("midiverb wrote {sr} Hz"));
    }
    if y.len() != n_mv {
        return Err(format!("midiverb wrote {} samples, asked {n_mv}", y.len()));
    }
    let mut y = resample_poly(&y, 80, 39);
    y.resize(x.len(), 0.0);
    Ok(y.into_iter().map(|v| v / g).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_the_current_directory_never_path() {
        // `ls` is on PATH; a bare `ls` must still mean ./ls.
        let e = Reference::find("ls", || Err("unused".into()), None)
            .err()
            .unwrap_or_default();
        assert!(e.contains("./ls"), "{e}");
        let e = Reference::find("midiverb", || Err("set MIDIVERB_EMU".into()), None)
            .err()
            .unwrap_or_default();
        assert_eq!(e, "set MIDIVERB_EMU");
    }
}
