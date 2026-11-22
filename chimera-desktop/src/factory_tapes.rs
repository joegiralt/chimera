//! The TAPE factory set (spec § 7, TAPE Task 19): `tools/factory-tapes/
//! recipes.toml`'s sources, each built as `just tape` builds one, and the
//! factory TAPE Sounds on them. Chimera's renders and the tool's own synths
//! are rebuilt here bit for bit; a CC0 original is read from
//! `$CHIMERA_TAPES_SRC` and never committed (factory library method).

use std::f32::consts::TAU;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use chimera_core::addr::Blocks;
use chimera_core::block::{Block, DiskCode, ParamId};
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::dsp::tape_engine::{
    Direction, TapeEngineParams as P, TapeMachine, TapeModel, Tron, machine,
};
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Card, FileKind, Generation, Header, SourceHead, encode_sound, write_file, write_source,
};
use chimera_core::ui::mod_grid::dest_label;
use chimera_hal::SAMPLE_RATE;
use chimera_hal::store::{Dir, FileName, Store};

use crate::factory_sounds::source::{
    ByEar, Route, by_ear, other_engine, routes, set as set_param, valid_addr,
};
use crate::import::tape::convert::{Channel, Mono, Pitch, read_wav};
use crate::import::tape::{Built, Cycle, Form, ImportError, Rate, Root, Top, build};
use crate::listen::{Table, render_sound, tables};
use crate::store::DirStore;

/// The repo's root: recipes and the pinned manifest live under it.
pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// The release this tool builds.
pub const VERSION: &str = "tapes-v1";

/// Where a source's audio comes from (spec § 7).
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A Chimera Sound, by name (a factory Sound, `INIT ALGO` or `INIT
    /// MODAL` with a `modal` model), `notes` held `secs` then `tail` of
    /// release, less its first `from` seconds; `morph` sweeps MORPH 0 →
    /// 127 over the hold.
    Render {
        sound: String,
        from: f32,
        modal: Option<ResonatorMode>,
        notes: Vec<u8>,
        secs: f32,
        tail: f32,
        morph: bool,
        /// Rendered once and kept (`frozen`, a path under
        /// `tools/factory-tapes/`): its 48 kHz samples as raw f32 LE, read
        /// back bit for bit instead of rendered, so an engine change never
        /// moves its pin or the Sounds on it (#352).
        frozen: Option<String>,
    },
    /// The tool's own synthesis, seeded: no recording in it.
    Synth { kind: Synth, secs: f32 },
    /// A CC0 recording in `$CHIMERA_TAPES_SRC`, `secs` of it from `from`.
    Cc0 {
        file: String,
        from: f32,
        secs: f32,
        credit: Credit,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Credit {
    pub title: String,
    pub author: String,
    pub url: String,
    pub licence: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Synth {
    Noise,
    Wind,
    Rain,
    /// 16 equal hits, one a slice for ONESHOT's SLICE NOTE.
    Kit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceRecipe {
    pub name: SoundName,
    pub origin: Origin,
    pub form: Form,
}

/// A factory TAPE Sound: TAPE INIT on `source`, its `machine`'s defaults, then `set`,
/// then any other block's params and routes as factory-sounds writes them
/// (`FILTER.CUTOFF`, `mod.LFO1`), its level trimmed `trim_db`.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundRecipe {
    pub name: SoundName,
    pub source: SoundName,
    pub machine: TapeMachine,
    /// WALK's DIRECTION and TRON (`mode`, `tron`).
    pub direction: Direction,
    pub tron: Tron,
    pub set: Vec<(ParamId, f32)>,
    /// `BLOCK.PARAM` keys, by frozen ident, and values as the card stores them.
    pub params: Vec<(String, String)>,
    /// `mod.SOURCE` keys: each route added to TAPE INIT's, or its amount set.
    pub mods: Vec<(ModSource, Vec<Route>)>,
    pub trim_db: f32,
    /// A level set by ear (`level = "by ear -1.7: <why>"`, dB off the
    /// median): the median gate holds it there instead.
    pub level: Option<ByEar>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipes {
    pub sources: Vec<SourceRecipe>,
    pub sounds: Vec<SoundRecipe>,
}

pub fn read_recipes() -> Result<Recipes, String> {
    let path = format!("{ROOT}/tools/factory-tapes/recipes.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    recipes(&text)
}

/// `recipes.toml`: `[[source]]` and `[[sound]]` tables (`listen::tables`).
pub fn recipes(text: &str) -> Result<Recipes, String> {
    let mut r = Recipes {
        sources: Vec::new(),
        sounds: Vec::new(),
    };
    for (kind, t) in tables(text, "recipes.toml")? {
        let name = name(&t)?;
        let at = |e: String| format!("recipes.toml [[{kind}]] {}: {e}", name.as_str());
        match kind.as_str() {
            "source" => r.sources.push(source_recipe(name, &t).map_err(at)?),
            "sound" => r.sounds.push(sound_recipe(name, &t).map_err(at)?),
            k => return Err(format!("recipes.toml: [[{k}]]")),
        }
    }
    for s in &r.sounds {
        if !r.sources.iter().any(|x| x.name == s.source) {
            return Err(format!(
                "sound {}: no source {}",
                s.name.as_str(),
                s.source.as_str()
            ));
        }
    }
    Ok(r)
}

fn get<'a>(t: &'a Table, k: &str) -> Option<&'a str> {
    t.iter().find(|e| e.0 == k).map(|e| e.1.as_str())
}

fn need<'a>(t: &'a Table, k: &str) -> Result<&'a str, String> {
    get(t, k).ok_or(format!("no {k}"))
}

fn num(t: &Table, k: &str) -> Result<Option<f32>, String> {
    get(t, k)
        .map(|v| v.parse().map_err(|_| format!("{k} = {v}: a number")))
        .transpose()
}

fn name(t: &Table) -> Result<SoundName, String> {
    let n = need(t, "name")?;
    SoundName::new(n).map_err(|e| format!("name {n}: {e:?}"))
}

fn source_recipe(name: SoundName, t: &Table) -> Result<SourceRecipe, String> {
    let secs = num(t, "secs")?.ok_or("no secs")?;
    let origin = match need(t, "origin")? {
        "render" => Origin::Render {
            sound: need(t, "sound")?.into(),
            from: num(t, "from")?.unwrap_or(0.0),
            modal: get(t, "modal")
                .map(|m| {
                    [
                        ResonatorMode::String,
                        ResonatorMode::Modal,
                        ResonatorMode::Bowed,
                        ResonatorMode::Sympathetic,
                    ]
                    .into_iter()
                    .find(|r| r.disk_ident() == m)
                    .ok_or(format!("modal {m}"))
                })
                .transpose()?,
            notes: need(t, "notes")?
                .split_whitespace()
                .map(|n| n.parse().map_err(|_| format!("note {n}")))
                .collect::<Result<_, _>>()?,
            secs,
            tail: num(t, "tail")?.unwrap_or(0.0),
            morph: get(t, "morph") == Some("sweep"),
            frozen: get(t, "frozen").map(Into::into),
        },
        "synth" => Origin::Synth {
            kind: match need(t, "synth")? {
                "noise" => Synth::Noise,
                "wind" => Synth::Wind,
                "rain" => Synth::Rain,
                "kit" => Synth::Kit,
                k => return Err(format!("synth {k}")),
            },
            secs,
        },
        "cc0" => {
            let licence = need(t, "licence")?;
            if licence != "CC0 1.0" {
                return Err(format!("licence {licence}: CC0 1.0 only"));
            }
            Origin::Cc0 {
                file: need(t, "file")?.into(),
                from: num(t, "from")?.unwrap_or(0.0),
                secs,
                credit: Credit {
                    title: need(t, "title")?.into(),
                    author: need(t, "author")?.into(),
                    url: need(t, "url")?.into(),
                    licence: licence.into(),
                },
            }
        }
        o => return Err(format!("origin {o}")),
    };
    let form = match num(t, "transwave")? {
        Some(c) => Form::Transwave {
            cycle: Cycle::new(c).ok_or(format!("transwave {c}"))?,
            top: Top::T512,
            frames: 16,
        },
        None => Form::Plain {
            rate: need(t, "rate")?.parse::<Rate>()?,
            root: get(t, "root").map(str::parse::<Root>).transpose()?,
        },
    };
    Ok(SourceRecipe { name, origin, form })
}

/// The recipes' and manifests' model words, by `TapeModel::index`: WALK
/// with its `mode` (DIRECTION) and `tron` beside it.
pub const MODEL_WORDS: [&str; 4] = ["WALK", "STRETCH", "SCRUB", "TRANSWAVE"];

/// `m`'s word in `MODEL_WORDS`.
pub fn model_word(m: TapeModel) -> &'static str {
    MODEL_WORDS[usize::from(m.index())]
}

fn sound_recipe(name: SoundName, t: &Table) -> Result<SoundRecipe, String> {
    let m = need(t, "model")?;
    let model = TapeModel::ALL
        .into_iter()
        .find(|&x| model_word(x) == m)
        .ok_or(format!("model {m}"))?;
    let play = model == TapeModel::Walk;
    let mode = match get(t, "mode") {
        None => Direction::Loop,
        Some(k) if play => Direction::ALL
            .into_iter()
            .find(|x| x.disk_ident() == k)
            .ok_or(format!("mode {k}"))?,
        Some(k) => return Err(format!("mode {k}: PLAY only")),
    };
    let tron = match get(t, "tron") {
        None => Tron::Off,
        Some("ON") if play => Tron::On,
        Some("OFF") if play => Tron::Off,
        Some(k) => return Err(format!("tron {k}: ON or OFF, PLAY only")),
    };
    let specs = P::default();
    let (mut set, mut params, mut mods) = (Vec::new(), Vec::new(), Vec::new());
    let mut probe = Sound::neutral(EngineType::Tape);
    for (k, v) in t {
        if ["name", "source", "model", "mode", "tron", "trim", "level"].contains(&k.as_str()) {
            continue;
        }
        if let Some((head, rest)) = k.split_once('.') {
            if head == "mod" {
                let src = ModSource::ALL
                    .into_iter()
                    .find(|m| m.disk_ident() == rest)
                    .ok_or(format!("{k}: no such source"))?;
                mods.push((src, routes(v, 0, EngineType::Tape)?));
                continue;
            }
            let a = valid_addr(head, rest).ok_or(format!("{k}: no such param"))?;
            other_engine(a.addr(), EngineType::Tape)?;
            let mut blk = probe.params.block_mut(a.addr().block).ok_or(k.clone())?;
            set_param(&mut *blk, a, v)?;
            params.push((k.clone(), v.clone()));
            continue;
        }
        let spec = specs
            .specs()
            .iter()
            .find(|s| s.label.eq_ignore_ascii_case(k) && s.id != P::MODEL)
            .ok_or(format!("{k}: no such TAPE param"))?;
        set.push((spec.id, v.parse().map_err(|_| format!("{k} = {v}"))?));
    }
    Ok(SoundRecipe {
        name,
        source: SoundName::new(need(t, "source")?).map_err(|e| format!("source: {e:?}"))?,
        machine: TapeMachine::of(model),
        direction: mode,
        tron,
        set,
        params,
        mods,
        trim_db: num(t, "trim")?.unwrap_or(0.0),
        level: get(t, "level")
            .map(|v| by_ear(v, 0).ok_or(format!("level {v}: \"by ear <dB>: <why>\"")))
            .transpose()?,
    })
}

/// Where the CC0 originals are: `$CHIMERA_TAPES_SRC`, else
/// `~/chimera-tapes-src`.
pub fn originals() -> PathBuf {
    std::env::var_os("CHIMERA_TAPES_SRC").map_or_else(
        || PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("chimera-tapes-src"),
        PathBuf::from,
    )
}

/// `r`'s audio at 48 kHz (a CC0 file at its own rate); `None` for a CC0
/// original that isn't in `dir`.
pub fn audio(r: &SourceRecipe, dir: &Path) -> Result<Option<Mono>, String> {
    let mono = |samples| Mono {
        rate: SAMPLE_RATE,
        samples,
        smpl: None,
    };
    Ok(Some(match &r.origin {
        Origin::Render {
            frozen: Some(file), ..
        } => {
            let path = format!("{ROOT}/tools/factory-tapes/{file}");
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            let (samples, rest) = bytes.as_chunks::<4>();
            if !rest.is_empty() {
                return Err(format!("{path}: not whole f32s"));
            }
            mono(samples.iter().map(|b| f32::from_le_bytes(*b)).collect())
        }
        Origin::Render {
            sound,
            from,
            modal,
            notes,
            secs,
            tail,
            morph,
            frozen: None,
        } => {
            let mut s = chimera_sound(sound)?;
            if let Some(m) = modal {
                s.params.modal.mode = *m;
            }
            let sweep: &dyn Fn(f32, &mut chimera_core::params::ParamSnapshot) = if *morph {
                &|t, p| p.algo.morph = (127.0 * t.min(1.0)) as u8
            } else {
                &|_, _| {}
            };
            let mut m = render_sound(&s, notes, *secs, *tail, sweep);
            m.samples
                .drain(..((from * SR) as usize).min(m.samples.len()));
            m
        }
        Origin::Synth { kind, secs } => mono(synth(*kind, *secs)),
        Origin::Cc0 {
            file, from, secs, ..
        } => {
            let path = dir.join(file);
            if !path.exists() {
                return Ok(None);
            }
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut m = read_wav(&bytes, Channel::Mix).map_err(|e| format!("{file}: {e:?}"))?;
            let at = |s: f32| ((s * m.rate as f32) as usize).min(m.samples.len());
            let (a, b) = (at(*from), at(from + secs));
            m.samples = m.samples[a..b].to_vec();
            fade_ends(&mut m.samples, m.rate);
            m.smpl = None;
            m
        }
    }))
}

/// A factory Sound by name, or INIT ALGO / INIT MODAL.
fn chimera_sound(name: &str) -> Result<Sound, String> {
    match name {
        "INIT ALGO" => Ok(Sound::init(EngineType::Algo)),
        "INIT MODAL" => Ok(Sound::init(EngineType::Modal)),
        n => (0..FACTORY_LEN)
            .filter_map(factory_sound)
            .find(|s| s.name.as_str() == n)
            .ok_or(format!("no factory Sound {n}")),
    }
}

/// 5 ms fades at a cut's ends.
fn fade_ends(x: &mut [f32], rate: u32) {
    let n = (rate as usize / 200).min(x.len() / 2);
    let len = x.len();
    for i in 0..n {
        let g = i as f32 / n as f32;
        x[i] *= g;
        x[len - 1 - i] *= g;
    }
}

/// A seeded xorshift: the synths rebuild bit for bit.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

/// Chamberlin's state-variable filter, band-pass out.
struct Svf {
    low: f32,
    band: f32,
}

impl Svf {
    fn band(&mut self, x: f32, hz: f32, q: f32) -> f32 {
        let f = 2.0 * (std::f32::consts::PI * hz / SAMPLE_RATE as f32).sin();
        self.low += f * self.band;
        let high = x - self.low - self.band / q;
        self.band += f * high;
        self.band
    }
}

const SR: f32 = SAMPLE_RATE as f32;

fn synth(kind: Synth, secs: f32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    let mut rng = Rng(0x7A9E_F00D);
    match kind {
        Synth::Noise => (0..n).map(|_| 0.5 * rng.next()).collect(),
        Synth::Wind => {
            let mut f = Svf {
                low: 0.0,
                band: 0.0,
            };
            (0..n)
                .map(|i| {
                    let t = i as f32 / SR;
                    let hz = 550.0 + 350.0 * (TAU * 0.7 * t).sin();
                    let gust = 0.6 + 0.4 * (TAU * 1.3 * t + 1.0).sin();
                    gust * f.band(rng.next(), hz, 3.0)
                })
                .collect()
        }
        Synth::Rain => {
            let mut bed = Svf {
                low: 0.0,
                band: 0.0,
            };
            let mut y: Vec<f32> = (0..n)
                .map(|_| 0.15 * bed.band(rng.next(), 2_500.0, 0.7))
                .collect();
            // About 60 drops a second: each a short decaying ping.
            let drops = (secs * 60.0) as usize;
            for _ in 0..drops {
                let at = ((rng.next() * 0.5 + 0.5) * n as f32) as usize;
                let hz = 2_000.0 + 1_500.0 * rng.next();
                let amp = 0.5 + 0.4 * rng.next();
                for (k, v) in y[at..(at + 960).min(n)].iter_mut().enumerate() {
                    let t = k as f32 / SR;
                    *v += amp * (-t / 0.004).exp() * (TAU * hz * t).sin();
                }
            }
            y
        }
        Synth::Kit => {
            let slot = n / 16;
            let mut y = Vec::with_capacity(16 * slot);
            for hit in 0..16 {
                y.extend((0..slot).map(|k| kit_hit(hit, k as f32 / SR, &mut rng)));
            }
            y
        }
    }
}

/// Hit `i` of the kit at `t` seconds: kicks, snares, a clap, a rim, hats,
/// toms, a bell, a shaker, a crash, a click and a zap. Each rises in 0.5 ms
/// from −34 dB and is still above the importer's −60 dB trim at its slot's
/// end, so no slice moves.
fn kit_hit(i: usize, t: f32, rng: &mut Rng) -> f32 {
    // A 0.5 ms rise from −34 dB: no step at the onset, and still above the
    // importer's trim.
    let rise = (0.02 + t / 0.0005).min(1.0);
    let env = |d: f32| rise * (-t / d).exp().max(0.004);
    let sweep = |f0: f32, f1: f32, d: f32| {
        // The phase of a frequency gliding f0 → f1 with time constant d.
        let ph = f1 * t + (f0 - f1) * d * (1.0 - (-t / d).exp());
        (TAU * ph + 1.5).sin()
    };
    let noise = rng.next();
    match i {
        0 => env(0.09) * sweep(160.0, 48.0, 0.03),
        1 => env(0.05) * sweep(220.0, 60.0, 0.015),
        2 => env(0.05) * (0.55 * noise + 0.5 * sweep(240.0, 180.0, 0.02)),
        3 => env(0.03) * (0.8 * noise + 0.3 * sweep(330.0, 250.0, 0.01)),
        4 => {
            let burst = |a: f32| {
                if t >= a {
                    (0.02 + (t - a) / 0.0005).min(1.0) * (-(t - a) / 0.006).exp()
                } else {
                    0.0
                }
            };
            ((burst(0.0) + burst(0.011) + burst(0.022)) * 0.6 + 0.4 * env(0.03)).min(1.0) * noise
        }
        5 => env(0.008) * (0.6 * (TAU * 1_700.0 * t + 1.5).sin() + 0.4 * noise),
        6 => env(0.012) * noise,
        7 => env(0.04) * noise,
        8 => env(0.08) * sweep(130.0, 90.0, 0.04),
        9 => env(0.07) * sweep(190.0, 135.0, 0.04),
        10 => env(0.06) * sweep(270.0, 200.0, 0.04),
        11 => {
            let sq = |hz: f32| (TAU * hz * t + 1.5).sin().signum();
            env(0.05) * 0.5 * (sq(540.0) + sq(800.0))
        }
        12 => chimera_core::dsp::clampf(t / 0.01, 0.2, 1.0) * env(0.03) * noise,
        13 => env(0.2) * noise,
        14 => env(0.002) * (TAU * 3_000.0 * t + 1.5).sin(),
        _ => env(0.04) * sweep(2_400.0, 200.0, 0.012),
    }
}

/// `r` built from `m`, as `just tape` builds one.
pub fn source(r: &SourceRecipe, m: Mono) -> Result<Built, String> {
    build(m, r.form, r.name).map_err(|e| format!("{}: {e}", r.name.as_str()))
}

/// The bytes the arena holds for a built source: its descriptor, then its
/// samples.
pub fn held(b: &Built) -> Vec<u8> {
    let d = Descriptor::whole(b.head.shape, b.head.count, b.head.sref.rate).expect("a descriptor");
    let mut h = d.to_bytes().to_vec();
    h.resize(DESC_LEN, 0);
    h.extend_from_slice(&b.packed);
    h
}

/// The pitch its head's labels give.
pub fn pitch(h: &SourceHead) -> Pitch {
    match (h.shape, h.period_q16) {
        (Shape::Plain, q) if q > 0 => Pitch::Pitched {
            period: q as f32 / 65_536.0,
            root_cents: h.root,
        },
        _ => Pitch::Unpitched,
    }
}

/// `s` as a TAPE Sound on `head` (picked as SOURCE picks it).
pub fn tape_sound(s: &SoundRecipe, head: &SourceHead) -> Sound {
    let mut snd = Sound::factory_init(EngineType::Tape);
    // A factory Sound's filter stays stable, as `plain_init`'s (ADR 0168).
    snd.params.filter.stability = 1.0;
    snd.name = s.name;
    snd.pick_source(head, None);
    let t = &mut snd.params.tape;
    t.machine = s.machine;
    t.direction = s.direction;
    // The machine's own, FORWARD's FADE holding, then TRON.
    for (id, v) in machine::defaults(t.machine(), s.direction) {
        t.set(id, v);
    }
    t.tron = s.tron;
    for &(id, v) in &s.set {
        t.set(id, v);
    }
    // ONE's hit: ENV 1 at its floor, holding full (`env_defaults`); TRON
    // keeps INIT's, as the TRON machine did.
    let env = if s.tron == Tron::On {
        &[][..]
    } else {
        machine::env_defaults(t.machine(), s.direction)
    };
    for &(id, v) in env {
        snd.params.envelopes[0].set(id, v);
    }
    for (k, v) in &s.params {
        let a = k
            .split_once('.')
            .and_then(|(b, p)| valid_addr(b, p))
            .expect("checked when read");
        let mut blk = snd.params.block_mut(a.addr().block).expect("a Sound block");
        set_param(&mut *blk, a, v).expect("checked when read");
    }
    for (src, routes) in &s.mods {
        for r in routes {
            if FixedCol::of(r.dest).is_none() && snd.dest_registry.find(r.dest).is_none() {
                snd.dest_registry
                    .add(r.dest, dest_label(r.dest))
                    .expect("a registry column");
            }
            assert!(snd.mod_state.set_route_addr(src.index(), r.dest, r.amount));
        }
    }
    // On the knob's grid, as INIT's 45/128 is.
    let v = snd.params.out.volume * 10f32.powf(s.trim_db / 20.0);
    snd.params.out.volume = ((v * 128.0).round() / 128.0).min(1.0);
    snd.map = snd.factory_map();
    snd
}

/// Every source built, in recipe order; `None` for a CC0 original that
/// isn't in `dir`.
pub fn build_all(r: &Recipes, dir: &Path) -> Result<Vec<Option<Built>>, String> {
    r.sources
        .iter()
        .map(|s| audio(s, dir)?.map(|m| source(s, m)).transpose())
        .collect()
}

/// A library Sound's stem: `S9` and six digits from 1, out of the way of
/// the card library's own ids (#317).
pub fn sound_stem(i: usize) -> String {
    format!("S9{:06}", i + 1)
}

/// The release's manifest (spec § 7): a line a source, then a line a Sound.
pub fn manifest(r: &Recipes, built: &[Option<Built>]) -> String {
    let mut j = format!("{{\"version\": \"{VERSION}\",\n\"sources\": [\n");
    let mut first = true;
    for (s, b) in r.sources.iter().zip(built) {
        let Some(b) = b else { continue };
        let h = &b.head;
        let origin = match &s.origin {
            Origin::Render { sound, notes, .. } => format!(
                "\"render\", \"of\": \"{sound} {}\"",
                notes
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Origin::Synth { kind, .. } => format!("\"synth\", \"of\": \"{kind:?}\""),
            Origin::Cc0 { file, credit, .. } => format!(
                "\"cc0\", \"of\": \"{file}\", \"author\": \"{}\", \"url\": \"{}\", \"licence\": \"{}\"",
                credit.author, credit.url, credit.licence
            ),
        };
        let _ = write!(
            j,
            "{}{{\"name\": \"{}\", \"crc\": \"{:08X}\", \"frames\": {}, \"rate\": {}, \"shape\": \"{:?}\", \"count\": {}, \"origin\": {origin}}}",
            if first { "" } else { ",\n" },
            h.name.as_str(),
            h.sref.crc,
            h.sref.frames,
            h.sref.rate,
            h.shape,
            h.count,
        );
        first = false;
    }
    j.push_str("\n],\n\"sounds\": [\n");
    let mut first = true;
    for (i, s) in r.sounds.iter().enumerate() {
        let Some(b) = r
            .sources
            .iter()
            .zip(built)
            .find(|(x, _)| x.name == s.source)
            .and_then(|(_, b)| b.as_ref())
        else {
            continue;
        };
        let _ = write!(
            j,
            "{}{{\"name\": \"{}\", \"file\": \"{}.A\", \"source\": \"{:08X}\", \"model\": \"{}\", \"mode\": \"{}\", \"tron\": \"{}\"}}",
            if first { "" } else { ",\n" },
            s.name.as_str(),
            sound_stem(i),
            b.head.sref.crc,
            model_word(s.machine.model()),
            s.direction.disk_ident(),
            s.tron.disk_ident()
        );
        first = false;
    }
    j.push_str("\n]}\n");
    j
}

/// A manifest's sources: each line's name and CRC.
pub fn manifest_crcs(json: &str) -> Vec<(String, u32)> {
    let field = |l: &str, k: &str| {
        let at = l.find(&format!("\"{k}\": \""))? + k.len() + 5;
        Some(l[at..].split('"').next()?.to_string())
    };
    json.lines()
        .filter(|l| l.contains("\"frames\""))
        .filter_map(|l| {
            Some((
                field(l, "name")?,
                u32::from_str_radix(&field(l, "crc")?, 16).ok()?,
            ))
        })
        .collect()
}

/// The set written to `out`: each `.SRC` sealed through `write_source` on a
/// card directory, copied out flat, each Sound file, `manifest.json`, and
/// `cc0-originals/` (each original with its licence) for the zip.
pub fn write_set(
    r: &Recipes,
    built: &[Option<Built>],
    dir: &Path,
    out: &Path,
) -> Result<(), String> {
    let card = out.join("card");
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(&card).map_err(|e| e.to_string())?;
    let mut store = DirStore::new(card.clone());
    let sounds: Vec<(usize, Sound)> = r
        .sounds
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let b = r
                .sources
                .iter()
                .zip(built)
                .find(|(x, _)| x.name == s.source)?
                .1
                .as_ref()?;
            Some((i, tape_sound(s, &b.head)))
        })
        .collect();
    Card::new()
        .run(&mut store, |st, rd| {
            for b in built.iter().flatten() {
                write_source(st, rd, &b.head, &b.packed).map_err(ImportError::Save)?;
            }
            st.make_dir(rd.volume(), Dir::Sounds)?;
            for (i, s) in &sounds {
                let stem = sound_stem(*i);
                let file = FileName::new(Dir::Sounds, stem.as_bytes(), b"A").expect("an 8.3 stem");
                let h = Header {
                    kind: FileKind::Sound,
                    generation: Generation::FIRST,
                    name: Some(s.name),
                };
                st.write(rd.volume(), file, &mut |sink| {
                    write_file(sink, &h, &mut |w| encode_sound(s, w))
                })?;
            }
            Ok::<_, ImportError>(())
        })
        .and_then(|o| o.result)
        .map_err(|e| e.to_string())?;
    for sub in ["CHIMERA/TAPES", "CHIMERA/SOUNDS"] {
        for e in std::fs::read_dir(card.join(sub)).map_err(|e| e.to_string())? {
            let p = e.map_err(|e| e.to_string())?.path();
            std::fs::copy(&p, out.join(p.file_name().expect("a file")))
                .map_err(|e| e.to_string())?;
        }
    }
    std::fs::remove_dir_all(&card).map_err(|e| e.to_string())?;
    std::fs::write(out.join("manifest.json"), manifest(r, built)).map_err(|e| e.to_string())?;
    let cc0 = out.join("cc0-originals");
    for s in &r.sources {
        if let Origin::Cc0 { file, credit, .. } = &s.origin {
            std::fs::create_dir_all(&cc0).map_err(|e| e.to_string())?;
            std::fs::copy(dir.join(file), cc0.join(file)).map_err(|e| format!("{file}: {e}"))?;
            std::fs::write(
                cc0.join(format!("{file}.LICENCE.txt")),
                format!(
                    "{}\nby {}\n{}\nLicence: {} (Creative Commons Zero, public domain dedication)\nhttps://creativecommons.org/publicdomain/zero/1.0/\n",
                    credit.title, credit.author, credit.url, credit.licence
                ),
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
