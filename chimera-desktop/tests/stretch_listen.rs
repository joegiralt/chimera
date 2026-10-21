//! Probe, not a gate: STRETCH's listening takes through the whole
//! instrument (P1, the bus, the limiter, `(L + R) / 2`), raw levels, WAVs
//! to `$PROBE_OUT` and a report beside them: CLASSIC, GRAIN and GRAIN on
//! three grains, side by side.
//! `PROBE_OUT=dir cargo test -p chimera-desktop --test stretch_listen -- --ignored --nocapture`

use chimera_core::block::Block as _;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::machine::{defaults, env_defaults};
use chimera_core::dsp::tape_engine::stretch::grain::{THRIFT, Thrift};
use chimera_core::dsp::tape_engine::{StretchType, TapeEngineParams as P, TapeMachine};
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::preset::Sound;
use chimera_core::project::Project;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::{MidiNote, Velocity};
use chimera_desktop::factory_tapes::{build_all, held, originals, recipes, tape_sound};
use chimera_desktop::import::tape::Built;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::fmt::Write as _;
use std::path::PathBuf;

const SR: f32 = SAMPLE_RATE as f32;

// ── The branch's STRETCH ───────────────────────────────────────────────
/// GRAIN to last `secs` (a source of `len` s), END STOP or HOLD, SPRAY
/// `spray` (None: the default).
fn stretch_to(s: &Sound, _len: f32, secs: f32, stop: bool, spray: Option<f32>) -> Sound {
    let mut s = s.clone();
    s.params.tape.kind = StretchType::Grain;
    s.params.tape.set(P::LENGTH, (secs / 0.25).log2() / 5.0);
    s.params.tape.set(P::FINISH, if stop { 1.0 } else { 0.0 });
    if let Some(x) = spray {
        s.params.tape.set(P::SPRAY, x);
    }
    s
}

/// CLASSIC to last `secs`: MOTION for a time speed of `len / secs`
/// (`slow(2m) = s` under 1×), MONO, as today's STRETCH was set by hand.
fn classic_to(s: &Sound, len: f32, secs: f32) -> Sound {
    let mut s = s.clone();
    s.params.tape.kind = StretchType::Mono;
    let sp = (len / secs).min(1.0);
    s.params.tape.set(P::MOTION, (15.0 * sp + 1.0).log2() / 8.0);
    s
}
// ───────────────────────────────────────────────────────────────────────

fn set() -> (chimera_desktop::factory_tapes::Recipes, Vec<Option<Built>>) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/factory-tapes/recipes.toml"
    );
    let mut text = std::fs::read_to_string(path).unwrap();
    text.push_str(
        r#"
[[source]]
name = "BELL 1S"
origin = "render"
sound = "TX BELL"
notes = "72"
secs = "1.0"
tail = "0.25"
rate = "32k"
root = "C5"

[[sound]]
name = "BELL1 STRETCH"
source = "BELL 1S"
model = "STRETCH"
trim = "0"

[[sound]]
name = "PAD STRETCH"
source = "PAD CHORD"
model = "STRETCH"
trim = "0"

[[sound]]
name = "AH STRETCH"
source = "VOICE AH"
model = "STRETCH"
trim = "0"
"#,
    );
    let r = recipes(&text).unwrap();
    let b = build_all(&r, &originals()).unwrap();
    (r, b)
}

/// What the UI writes on a MODEL turn (`part_page::machine_edit`).
fn turn(s: &Sound, m: TapeMachine) -> Sound {
    let mut s = s.clone();
    s.params.tape.machine = m;
    for (id, v) in defaults(m) {
        s.params.tape.set(id, v);
    }
    for &(id, v) in env_defaults(m) {
        s.params.envelopes[0].set(id, v);
    }
    s
}

/// `events` (seconds, note, on) over `secs`, through P1, after the
/// limiter, `(L + R) / 2`.
fn render(snd: &Sound, b: &Built, events: &[(f32, u8, bool)], secs: f32) -> Vec<f32> {
    let (mut p, _) = Project::boxed();
    let id = p.source_store(b.head.sref, &held(b)).unwrap();
    let mut tok = p.pool().table().reader().unwrap();
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = snd.params.clone();
    *shared.parts[0].mod_state = snd.mod_state.clone();
    shared.parts[0].source = Some(id);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let ch = shared.parts[0].mix.channel;
    let mut out = Vec::new();
    let n = (secs * SR) as usize / BLOCK_SIZE;
    let mut next = 0;
    for blk in 0..n {
        let t = (blk * BLOCK_SIZE) as f32 / SR;
        let view = tok.begin_block();
        while next < events.len() && events[next].0 <= t {
            let (_, note, on) = events[next];
            let kind = if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            };
            inst.handle_with(
                NoteEvent {
                    channel: ch,
                    note: MidiNote::new(note).unwrap(),
                    kind,
                },
                &shared,
                &view,
            );
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &view);
        tok.store_sounding(inst.source_mask());
        out.extend(dac.out()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    out
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}
fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}
fn frames(x: &[f32], ms: f32) -> Vec<f32> {
    x.chunks((ms * SR / 1000.0) as usize).map(rms).collect()
}
/// ab-method's level: RMS over the louder half of 0.1 s frames.
fn level(x: &[f32]) -> f32 {
    let mut f = frames(x, 100.0);
    f.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let h = &f[..f.len().div_ceil(2)];
    db((h.iter().map(|v| v * v).sum::<f32>() / h.len() as f32).sqrt())
}
/// The last 10 ms frame above −60 dBFS, seconds.
fn end_t(x: &[f32]) -> f32 {
    let f = frames(x, 10.0);
    f.iter()
        .rposition(|&v| db(v) > -60.0)
        .map_or(0.0, |i| (i + 1) as f32 * 0.01)
}
/// The median of each 8192-sample Hann frame's loudest partial in
/// `[t0, t1)`, 60 Hz to 4 kHz, parabolic between bins.
fn f0(x: &[f32], t0: f32, t1: f32) -> f32 {
    use rustfft::{FftPlanner, num_complex::Complex};
    let w = 8192;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(w);
    let (a, b) = ((t0 * SR) as usize, ((t1 * SR) as usize).min(x.len()));
    let mut fs = Vec::new();
    let mut s = a;
    while s + w <= b {
        let fr = &x[s..s + w];
        if db(rms(fr)) > -50.0 {
            let mut buf: Vec<Complex<f32>> = fr
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    Complex::new(
                        v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / w as f32).cos()),
                        0.0,
                    )
                })
                .collect();
            fft.process(&mut buf);
            let m: Vec<f32> = buf[..w / 2]
                .iter()
                .map(|c| c.norm().max(1e-12).ln())
                .collect();
            let (lo, hi) = (
                (60.0 * w as f32 / SR) as usize,
                (4000.0 * w as f32 / SR) as usize,
            );
            let k = (lo..hi)
                .max_by(|&i, &j| m[i].partial_cmp(&m[j]).unwrap())
                .unwrap();
            let d = 0.5 * (m[k - 1] - m[k + 1]) / (m[k - 1] - 2.0 * m[k] + m[k + 1]);
            fs.push((k as f32 + d) * SR / w as f32);
        }
        s += w / 2;
    }
    if fs.is_empty() {
        return f32::NAN;
    }
    fs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fs[fs.len() / 2]
}
/// The time from the loudest 10 ms frame until one sits `d` dB under it.
fn fall_from_peak(x: &[f32], d: f32) -> f32 {
    let f = frames(x, 10.0);
    let (i, p) = f
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
    f[i..]
        .iter()
        .position(|&v| db(v) < db(p) - d)
        .map_or(f32::INFINITY, |k| (i + k) as f32 * 0.01)
}
/// Splice tremolo: the RMS of 10 ms frames' dB about a 0.2 s moving mean,
/// over `[t0, t1)`, frames above −60 dBFS.
fn ripple(x: &[f32], t0: f32, t1: f32) -> f32 {
    let f: Vec<f32> = frames(x, 10.0).iter().map(|&v| db(v)).collect();
    let (a, b) = ((t0 * 100.0) as usize, ((t1 * 100.0) as usize).min(f.len()));
    let mut acc = (0.0, 0);
    for i in a.max(10)..b.saturating_sub(10) {
        if f[i] < -60.0 {
            continue;
        }
        let m = f[i - 10..i + 10].iter().sum::<f32>() / 20.0;
        acc = (acc.0 + (f[i] - m).powi(2), acc.1 + 1);
    }
    (acc.0 / acc.1.max(1) as f32).sqrt()
}
fn env_line(x: &[f32], step_s: f32) -> String {
    frames(x, step_s * 1000.0)
        .iter()
        .map(|&v| format!("{:.0}", db(v).max(-99.0)))
        .collect::<Vec<_>>()
        .join(" ")
}
fn write(dir: &std::path::Path, name: &str, x: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(dir.join(name), spec).unwrap();
    for &v in x {
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}
fn root(b: &Built) -> u8 {
    ((b.head.root + 50) / 100) as u8
}

#[test]
#[ignore]
fn listen() {
    let dir = PathBuf::from(std::env::var("PROBE_OUT").expect("PROBE_OUT"));
    std::fs::create_dir_all(&dir).unwrap();
    let (r, built) = set();
    let find = |name: &str| {
        let s = r.sounds.iter().find(|s| s.name.as_str() == name).unwrap();
        let b = r
            .sources
            .iter()
            .zip(&built)
            .find(|(x, _)| x.name == s.source)
            .unwrap()
            .1
            .as_ref()
            .unwrap();
        (tape_sound(s, &b.head), b)
    };
    let mut rep = String::new();
    // (slug, sound, source, held s, render s, thrift)
    let mut cases: Vec<(String, Sound, &Built, f32, f32, Thrift)> = Vec::new();
    let (bell, bb) = find("BELL1 STRETCH");
    let len = bb.head.sref.frames as f32 / f32::from(bb.head.sref.rate);
    writeln!(
        rep,
        "BELL 1S: {len:.3} s, root {}, period_q16 {}",
        root(bb),
        bb.head.period_q16
    )
    .unwrap();
    let f = Thrift::Four;
    cases.push((
        "bell-natural".into(),
        turn(&bell, TapeMachine::One),
        bb,
        len,
        len + 1.0,
        f,
    ));
    for l in [2.0f32, 4.0, 8.0] {
        cases.push((
            format!("bell-{l:.0}s-classic"),
            classic_to(&bell, len, l),
            bb,
            l,
            l + 1.5,
            f,
        ));
        cases.push((
            format!("bell-{l:.0}s-grain"),
            stretch_to(&bell, len, l, true, None),
            bb,
            l,
            l + 1.5,
            f,
        ));
        cases.push((
            format!("bell-{l:.0}s-grain3"),
            stretch_to(&bell, len, l, true, None),
            bb,
            l,
            l + 1.5,
            Thrift::Three,
        ));
    }
    cases.push((
        "bell-hold-classic".into(),
        classic_to(&bell, len, 2.0),
        bb,
        6.0,
        7.0,
        f,
    ));
    cases.push((
        "bell-hold-grain".into(),
        stretch_to(&bell, len, 2.0, false, None),
        bb,
        6.0,
        7.0,
        f,
    ));
    cases.push((
        "bell-8s-grain-spray0".into(),
        stretch_to(&bell, len, 8.0, true, Some(0.0)),
        bb,
        8.0,
        9.5,
        f,
    ));
    cases.push((
        "bell-1x-grain-bypass".into(),
        stretch_to(&bell, len, len, true, Some(0.0)),
        bb,
        len,
        len + 1.0,
        Thrift::Bypass,
    ));
    cases.push((
        "bell-1x-grain".into(),
        stretch_to(&bell, len, len, true, Some(0.0)),
        bb,
        len,
        len + 1.0,
        f,
    ));
    let (pad, pb) = find("PAD STRETCH");
    let plen = pb.head.sref.frames as f32 / f32::from(pb.head.sref.rate);
    cases.push((
        "pad-natural".into(),
        turn(&pad, TapeMachine::One),
        pb,
        plen,
        plen + 1.0,
        f,
    ));
    cases.push((
        "pad-6s-classic".into(),
        classic_to(&pad, plen, 6.0),
        pb,
        6.0,
        7.5,
        f,
    ));
    cases.push((
        "pad-6s-grain".into(),
        stretch_to(&pad, plen, 6.0, true, None),
        pb,
        6.0,
        7.5,
        f,
    ));
    cases.push((
        "pad-6s-grain3".into(),
        stretch_to(&pad, plen, 6.0, true, None),
        pb,
        6.0,
        7.5,
        Thrift::Three,
    ));
    let (ah, ab) = find("AH STRETCH");
    let alen = ab.head.sref.frames as f32 / f32::from(ab.head.sref.rate);
    cases.push((
        "voice-natural".into(),
        turn(&ah, TapeMachine::One),
        ab,
        alen,
        alen + 1.0,
        f,
    ));
    cases.push((
        "voice-6s-classic".into(),
        classic_to(&ah, alen, 6.0),
        ab,
        6.0,
        7.5,
        f,
    ));
    cases.push((
        "voice-6s-grain".into(),
        stretch_to(&ah, alen, 6.0, true, None),
        ab,
        6.0,
        7.5,
        f,
    ));
    cases.push((
        "voice-6s-grain3".into(),
        stretch_to(&ah, alen, 6.0, true, None),
        ab,
        6.0,
        7.5,
        Thrift::Three,
    ));
    for (slug, snd, b, hold, secs, thrift) in cases {
        THRIFT.store(thrift as u8, core::sync::atomic::Ordering::Relaxed);
        let rt = root(b);
        let x = render(&snd, b, &[(0.0, rt, true), (hold, rt, false)], secs);
        let name = format!("{slug}.wav");
        write(&dir, &name, &x);
        let src_f0 = 440.0 * 2f32.powf((b.head.root as f32 / 100.0 - 69.0) / 12.0);
        let f = f0(&x, 0.05, hold.max(0.4));
        writeln!(
            rep,
            "{name:<26} level {:+6.2} dBFS  end {:5.2} s  fall30 {:5.2} s  ripple {:.2} dB  f0 {f:.1} Hz ({:+.0} c of the root's {src_f0:.1})",
            level(&x),
            end_t(&x),
            fall_from_peak(&x, 30.0),
            ripple(&x, 0.05, hold),
            1200.0 * (f / src_f0).log2(),
        )
        .unwrap();
        writeln!(rep, "  env 0.25s: {}", env_line(&x, 0.25)).unwrap();
    }
    std::fs::write(dir.join("report.txt"), &rep).unwrap();
    println!("{rep}");
}
