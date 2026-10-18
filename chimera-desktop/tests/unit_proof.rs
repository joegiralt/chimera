//! TAPE slice 1's unit proof (spec § 12), its files: four CC0 beds this
//! tool synthesises, then, once `just tape` has sealed them into
//! `$PROOF_DIR` (as `CHIMERA_CARD`), a project with PLAY LOOP on each in
//! Parts 1–4 and the costliest factory ALGO Sound in Part 5. Neither file
//! goes in the repo: `$PROOF_DIR` is a ledger folder.
//!
//! `PROOF_DIR=<dir> cargo test -p chimera-desktop --test unit_proof -- --ignored beds`
//! `PROOF_DIR=<dir> cargo test -p chimera-desktop --test unit_proof -- --ignored project`

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use chimera_core::MidiChannel;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::{COST_STREAM, PlayMode, TapeModel};
use chimera_core::dsp::voice::Voice;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::hw::{CPU_HZ_REV_V, Cost, SampleBudget};
use chimera_core::name::{ProjectName, SoundName};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::arena::ARENA_BYTES;
use chimera_core::project::{PartFrom, PartId, PartSource, Project, ReplaceGuard, load_kind};
use chimera_core::storage::{SourceCheck, SourceHead};
use chimera_desktop::factory_sounds::project::file_bytes;
use chimera_desktop::factory_tapes::{SoundRecipe, tape_sound};

/// The beds' rate, and length: a whole number of beats at 120 BPM, so
/// each loops seamlessly; 936 KB at 12 bits, under `put`'s 1 MiB.
const RATE: u32 = 24_000;
const SECS: f32 = 26.0;
const N: usize = (RATE as f32 * SECS) as usize;
/// The beds, in Part order.
const BEDS: [&str; 4] = ["DRONE", "WASH", "PULSE", "ARP"];
/// The project's id: out of the demos' way (P0000001–P0000009).
const PROJECT: &str = "P0000077";

fn dir() -> PathBuf {
    PathBuf::from(std::env::var("PROOF_DIR").expect("PROOF_DIR"))
}

fn hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// `f` moved to a whole number of cycles over the bed: it loops clean.
fn whole(f: f32) -> f32 {
    (f * SECS).round() / SECS
}

/// A slow minor-ninth drone, each voice breathing at its own whole rate.
fn drone() -> Vec<f32> {
    let notes = [45.0, 52.0, 55.0, 59.0, 60.0, 64.0];
    (0..N)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            notes
                .iter()
                .enumerate()
                .map(|(k, &m)| {
                    let f = whole(hz(m));
                    let breath = 0.6 + 0.4 * (TAU * (k as f32 + 1.0) * t / SECS).sin();
                    breath * ((TAU * f * t).sin() + 0.3 * (TAU * 2.0 * f * t).sin())
                })
                .sum()
        })
        .collect()
}

/// Noise through a lowpass that sweeps once a loop, its tail crossfaded
/// into its head.
fn wash() -> Vec<f32> {
    const FADE: usize = RATE as usize;
    let mut x: u32 = 0x9E37_79B9;
    let (mut lp, mut lp2) = (0.0f32, 0.0f32);
    let raw: Vec<f32> = (0..N + FADE)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let w = x as f32 / u32::MAX as f32 * 2.0 - 1.0;
            let sweep = 0.5 - 0.5 * (TAU * i as f32 / N as f32).cos();
            let a = 0.01 + 0.12 * sweep;
            lp += a * (w - lp);
            lp2 += a * (lp - lp2);
            lp2
        })
        .collect();
    (0..N)
        .map(|i| match i < FADE {
            true => {
                let g = i as f32 / FADE as f32;
                raw[i] * g.sqrt() + raw[N + i] * (1.0 - g).sqrt()
            }
            false => raw[i],
        })
        .collect()
}

/// A note of `len` samples at `at`, by `voice(phase)`, its envelope at 0
/// by its end.
fn note(out: &mut [f32], at: usize, len: usize, f: f32, decay: f32, voice: impl Fn(f32) -> f32) {
    for j in 0..len.min(out.len().saturating_sub(at)) {
        let t = j as f32 / RATE as f32;
        let attack = (j as f32 / (0.003 * RATE as f32)).min(1.0);
        let env = attack * (-t / decay).exp() * (1.0 - j as f32 / len as f32);
        out[at + j] += env * voice((f * t).fract());
    }
}

/// An eighth-note bass pulse at 120 BPM.
fn pulse() -> Vec<f32> {
    let step = RATE as usize / 4;
    let line = [33.0, 33.0, 36.0, 33.0, 31.0, 33.0, 40.0, 38.0];
    let mut out = vec![0.0; N];
    for (k, at) in (0..N).step_by(step).enumerate() {
        let f = hz(line[k % line.len()]);
        note(&mut out, at, step, f, 0.09, |ph| {
            if ph < 0.3 { 1.0 } else { -0.43 }
        });
    }
    // A gentle lowpass over the square's edges.
    let mut lp = 0.0;
    for v in &mut out {
        lp += 0.25 * (*v - lp);
        *v = lp;
    }
    out
}

/// Sixteenth notes at 120 BPM over Am7, Fmaj7, Cmaj7, G6, a bar each.
fn arp() -> Vec<f32> {
    let step = RATE as usize / 8;
    let chords: [[f32; 4]; 4] = [
        [57.0, 60.0, 64.0, 67.0],
        [53.0, 57.0, 60.0, 64.0],
        [48.0, 52.0, 55.0, 59.0],
        [55.0, 59.0, 62.0, 64.0],
    ];
    let order = [0, 1, 2, 3, 4, 3, 2, 1];
    let mut out = vec![0.0; N];
    for (k, at) in (0..N).step_by(step).enumerate() {
        let c = chords[(k / 16) % 4];
        let o = order[k % 8];
        let m = c[o % 4] + if o >= 4 { 12.0 } else { 0.0 };
        note(&mut out, at, step, hz(m), 0.06, |ph| {
            (TAU * ph).sin() + 0.2 * (3.0 * TAU * ph).sin()
        });
    }
    out
}

/// `x` peak-normalised to −6 dBFS, as a 24 kHz 16-bit mono WAV.
fn write(path: &Path, x: &[f32]) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav");
    for v in x {
        w.write_sample((v / peak * 0.5 * 32_767.0).round() as i16)
            .expect("sample");
    }
    w.finalize().expect("wav");
}

#[test]
#[ignore = "writes the unit proof's beds into $PROOF_DIR"]
fn beds() {
    let wav = dir().join("wav");
    std::fs::create_dir_all(&wav).expect("wav dir");
    for (name, x) in BEDS.iter().zip([drone(), wash(), pulse(), arp()]) {
        assert_eq!(x.len(), N);
        let p = wav.join(format!("{}.wav", name.to_lowercase()));
        write(&p, &x);
        println!("{}", p.display());
    }
}

/// Each bed's head, from the `.SRC`s `just tape` sealed in `$PROOF_DIR`.
fn heads() -> Vec<(SourceHead, u64)> {
    let tapes = dir().join("CHIMERA/TAPES");
    let mut all: Vec<(SourceHead, u64)> = std::fs::read_dir(&tapes)
        .expect("CHIMERA/TAPES: run `just tape` with CHIMERA_CARD=$PROOF_DIR first")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "SRC"))
        .map(|p| {
            let b = std::fs::read(&p).expect("src");
            let stem = p.file_stem().and_then(|s| s.to_str()).expect("a stem");
            let stem = u32::from_str_radix(stem, 16).expect("a CRC stem");
            let mut check = SourceCheck::new(stem, b.len() as u32).expect("a source");
            check.feed(&b).expect("checks");
            (check.finish().expect("a source head"), b.len() as u64)
        })
        .collect();
    all.retain(|(h, _)| BEDS.contains(&h.name.as_str()));
    all.sort_by_key(|(h, _)| BEDS.iter().position(|b| *b == h.name.as_str()));
    assert_eq!(all.len(), BEDS.len(), "a .SRC for each of {BEDS:?}");
    all
}

/// `snd` into a pool slot and onto `part`, as a library load does.
fn place(p: &mut Project, t: chimera_core::project::TemplateCrc, part: PartId, snd: &Sound) {
    let slot = match p.needs_for(snd, None) {
        Ok(Some(s)) => s,
        Ok(None) => {
            let s = p.pool().first_free().expect("a free slot");
            p.pool_store(s, snd).expect("stored");
            s
        }
        Err(e) => panic!("no room for {}: {e:?}", snd.name.as_str()),
    };
    let to = PartSource {
        part,
        from: PartFrom::Slot(slot),
    };
    let c = ReplaceGuard::check(p, t, to).expect("no prompt");
    p.replace_part(c).expect("replaced");
}

fn cost(s: &Sound) -> Cost {
    Voice::cost(&s.params, &s.mod_state)
}

#[test]
#[ignore = "writes the unit proof's project into $PROOF_DIR"]
fn project() {
    let heads = heads();
    let (mut p, template) = Project::boxed();
    p.set_name(ProjectName::new("TAPE PROOF").expect("a name"));
    let free = chimera_core::project::arena::Cost::of(ARENA_BYTES);
    let mut bill = Vec::new();
    for (k, (h, len)) in heads.iter().enumerate() {
        let recipe = SoundRecipe {
            name: SoundName::new(h.name.as_str()).expect("a name"),
            source: h.name,
            model: TapeModel::Play,
            mode: PlayMode::Loop,
            set: Vec::new(),
            trim_db: 0.0,
        };
        let snd = tape_sound(&recipe, h);
        let kind = load_kind(h, free, &snd.params.tape);
        println!(
            "part {} {} {:08X}.SRC {len} B: {kind:?} (arena {ARENA_BYTES} B), voice {}",
            k + 1,
            h.name.as_str(),
            h.sref.crc,
            cost(&snd).0
        );
        assert!(
            matches!(kind, chimera_core::project::LoadKind::Streamed(_)),
            "{} must stream",
            h.name.as_str()
        );
        place(&mut p, template, PartId::ALL[k], &snd);
        bill.push(cost(&snd) + COST_STREAM);
    }
    let mut algo: Vec<Sound> = (0..FACTORY_LEN)
        .filter_map(factory_sound)
        .filter(|s| s.params.engine() == EngineType::Algo)
        .collect();
    algo.sort_by_key(|s| std::cmp::Reverse(cost(s).0));
    // Admission bills every voice against its cap: the costliest ALGO
    // Sound whose four voices still fit beside the beds and the FX.
    let cap = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    let beds: u32 = bill.iter().map(|c| c.0).sum::<u32>() + FxBus::COST.0;
    let room = (cap - beds) / 4;
    for s in algo.iter().take(3) {
        let fit = (cap - beds) / cost(s).0;
        println!(
            "algo {} voice {}: {fit} voices fit",
            s.name.as_str(),
            cost(s).0
        );
    }
    let heavy = algo
        .iter()
        .find(|s| cost(s).0 <= room)
        .expect("a Sound that fits");
    println!(
        "part 5 {} voice {} (room {room} a voice)",
        heavy.name.as_str(),
        cost(heavy).0
    );
    let five = PartId::ALL[4];
    place(&mut p, template, five, heavy);
    let e = p.edit_part(five);
    e.mix.mode = PartMode::Poly;
    e.mix.channel = MidiChannel::new(4).expect("CH 5");
    for (k, part) in PartId::ALL[..4].iter().enumerate() {
        p.edit_part(*part).mix.channel = MidiChannel::new(k as u8).expect("CH 1-4");
    }
    bill.extend([cost(heavy); 4]);
    let total: u32 = bill.iter().map(|c| c.0).sum::<u32>() + FxBus::COST.0;
    let cycles = CPU_HZ_REV_V / chimera_core::hw::SAMPLE_RATE;
    println!(
        "billed {total} cycles/sample (FX {}): {}% of the CPU; admission's cap {cap} ({}%)",
        FxBus::COST.0,
        total * 100 / cycles,
        cap * 100 / cycles
    );
    let out = dir().join(format!("{PROJECT}.A"));
    std::fs::write(&out, file_bytes(&p).expect("encoded")).expect("written");
    println!("{}", out.display());
}
