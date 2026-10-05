//! The TAPE listening rig (spec § 9, Desktop QA): a source held as the
//! importer holds it, played offline through a model under a macro script,
//! checked (peaks, the click rule, NaN) before the owner hears it, and laid
//! out on a page with a transport per render.

use std::fmt::Write as _;
use std::mem::MaybeUninit;

use crate::import::tape::convert::{Mono, Pitch, clean, quantise_tpdf, resample, yin};
use chimera_core::block::{Block, DiskCode, ParamId};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape, SourceView};
use chimera_core::dsp::tape_engine::{TapeEngine, TapeEngineParams as P, TapeModel};
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::storage::twelve::pack12;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// After the note-off, how long a Chimera source rings on.
const TAIL_S: f32 = 1.0;
/// The dither's seed: a source rebuilds bit for bit.
const SEED: u64 = 0x7A9E;

/// A factory Sound held on `note` for `secs`, then `TAIL_S` of its release,
/// rendered offline through `Instrument` at 48 kHz: DAC pair 1, `(L + R)/2`.
/// Slots past the factory bank are NEW's INIT Sounds, by `EngineType::ALL`
/// (as `template::new_slot`): `FACTORY_LEN + 1` is INIT MODAL.
pub fn chimera_source(slot: usize, note: u8, secs: f32) -> Mono {
    let sound = factory_sound(slot)
        .or_else(|| {
            let e = EngineType::ALL.get(slot.checked_sub(FACTORY_LEN)?)?;
            Some(Sound::init(*e))
        })
        .expect("a factory or INIT slot");
    let mut shared = Box::<AudioShared>::default();
    shared.parts[0].params = sound.params;
    shared.parts[0].mod_state = sound.mod_state;
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let ev = |kind| NoteEvent {
        channel: MidiChannel::clamped(0),
        note: MidiNote::new(note).expect("a MIDI note"),
        kind,
    };
    let blocks = |s: f32| (s * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
    inst.handle(ev(NoteKind::On(Velocity::DEFAULT)), &shared);
    let mut samples = Vec::new();
    for b in 0..blocks(secs + TAIL_S) {
        if b == blocks(secs) {
            inst.handle(ev(NoteKind::Off), &shared);
        }
        inst.render(&mut fx, &mut dac, &shared, &mut scope);
        let lr = &dac.out()[0];
        samples.extend(lr.chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    Mono {
        rate: SAMPLE_RATE,
        samples,
        smpl: None,
    }
}

/// `m` as the importer holds it at `rate`: cleaned, resampled, pitched by
/// YIN, dithered to 12 bits, then the descriptor and the packed samples.
pub fn held_from(m: &Mono, rate: u32) -> (Vec<u8>, Pitch) {
    let mut c = m.clone();
    clean(&mut c);
    let x = resample(&c.samples, c.rate, rate).expect("a source rate");
    let pitch = yin(&x, rate);
    let q = quantise_tpdf(&x, SEED);
    let desc = Descriptor {
        shape: Shape::Plain,
        count: 1,
        rate: u16::try_from(rate).expect("a rate under 65 536"),
        head_frames: 0,
    };
    let mut held = desc.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * q.len() / 2, 0);
    pack12(&q, &mut held[DESC_LEN..]);
    (held, pitch)
}

/// The source the A/B compares against: `m` cleaned, at 48 kHz.
pub fn dry(m: &Mono) -> Vec<f32> {
    let mut c = m.clone();
    clean(&mut c);
    resample(&c.samples, c.rate, SAMPLE_RATE).expect("a source rate")
}

/// The note that plays a source at its own speed: its root, or C4.
pub fn root_note(pitch: Pitch) -> u8 {
    match pitch {
        Pitch::Pitched { root_cents, .. } => ((root_cents as u32 + 50) / 100).min(127) as u8,
        Pitch::Unpitched => 60,
    }
}

/// Breakpoints `(seconds, param, value)`: each param linear between its
/// own, in time order, and held before the first and after the last. A
/// param the script never names stays neutral. It ends at its last.
pub struct Script(pub &'static [(f32, ParamId, f32)]);

impl Script {
    pub fn secs(&self) -> f32 {
        self.0.iter().fold(0.0, |a, k| a.max(k.0))
    }

    fn at(&self, id: ParamId, t: f32) -> Option<f32> {
        let mut prev: Option<(f32, f32)> = None;
        for &(tk, _, v) in self.0.iter().filter(|k| k.1 == id) {
            if t < tk {
                return Some(match prev {
                    None => v,
                    Some((t0, v0)) => v0 + (v - v0) * (t - t0) / (tk - t0),
                });
            }
            prev = Some((tk, v));
        }
        prev.map(|(_, v)| v)
    }

    /// Every scripted param at `t`.
    pub fn apply(&self, p: &mut P, t: f32) {
        for &(_, id, _) in self.0 {
            if let Some(v) = self.at(id, t) {
                p.write(id, v);
            }
        }
    }
}

/// 2 s held at neutral; POSITION, then MOTION, then SIZE each swept
/// 0 → 1 → 0 over 3 s (SIZE glides from neutral 1 to 0 in the 250 ms
/// before its sweep, and back after); then TAPE and AGE glide to 0.5 and
/// hold 3 s.
static SWEEP: [(f32, ParamId, f32); 16] = [
    (2.0, P::POSITION, 0.0),
    (3.5, P::POSITION, 1.0),
    (5.0, P::POSITION, 0.0),
    (5.0, P::MOTION, 0.0),
    (6.5, P::MOTION, 1.0),
    (8.0, P::MOTION, 0.0),
    (7.75, P::SIZE, 1.0),
    (8.0, P::SIZE, 0.0),
    (9.5, P::SIZE, 1.0),
    (11.0, P::SIZE, 0.0),
    (11.25, P::SIZE, 1.0),
    (11.25, P::TAPE, 0.0),
    (11.5, P::TAPE, 0.5),
    (14.5, P::TAPE, 0.5),
    (11.25, P::AGE, 0.0),
    (11.5, P::AGE, 0.5),
];

/// What the page says the script does.
pub const SWEEP_LEGEND: &str = "0–2 s neutral · 2–5 s POSITION 0→1→0 · 5–8 s MOTION 0→1→0 · \
    8–11 s SIZE 0→1→0 · 11.5–14.5 s TAPE and AGE at 0.5";

/// The macro script `model` is heard under.
pub fn script(_model: TapeModel) -> Script {
    Script(&SWEEP)
}

/// `held` played by `model` on `note` under `s`, from a fresh engine: the
/// engine's own output, 48 kHz. Panics on a `held` with no source in it.
pub fn render(model: TapeModel, held: &[u8], pitch: Pitch, note: u8, s: &Script) -> Vec<f32> {
    let v = SourceView::new(held).expect("a held source");
    let mut p = P {
        model,
        ..P::default()
    };
    if let Pitch::Pitched { period, root_cents } = pitch {
        (p.root, p.period) = (f32::from(root_cents), period);
    }
    s.apply(&mut p, 0.0);
    let mut slot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut slot);
    e.hold();
    e.note_on(note, &p, &v, SAMPLE_RATE);
    let blocks = (s.secs() * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut blk = [0.0; BLOCK_SIZE];
    for b in 0..blocks {
        s.apply(&mut p, (b * BLOCK_SIZE) as f32 / SAMPLE_RATE as f32);
        e.render(&mut blk, &p, &v, SAMPLE_RATE);
        out.extend_from_slice(&blk);
    }
    out
}

/// Claude's QA of a render (QA before UAT).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qa {
    pub peak_dbfs: f32,
    /// The largest sample step over the steady state's largest.
    pub max_step_ratio: f32,
    pub non_finite: usize,
}

/// The click rule (`click_free_test`'s): no step over 4× the steady
/// state's largest.
pub const CLICK_RATIO: f32 = 4.0;

/// A real recording has no steady sine to measure, so the steady state's
/// largest step is the 99.9th percentile of the steps: a click is a few
/// samples, and a source's own steep passages are many.
const STEADY: f32 = 0.999;

pub fn qa(x: &[f32]) -> Qa {
    let finite = |s: &&f32| s.is_finite();
    let peak = x.iter().filter(finite).fold(0f32, |a, s| a.max(s.abs()));
    let mut steps: Vec<f32> = x
        .windows(2)
        .filter(|w| w[0].is_finite() && w[1].is_finite())
        .map(|w| (w[1] - w[0]).abs())
        .collect();
    let max = steps.iter().copied().fold(0f32, f32::max);
    let max_step_ratio = if max == 0.0 {
        0.0
    } else {
        let k = ((steps.len() - 1) as f32 * STEADY) as usize;
        max / *steps.select_nth_unstable_by(k, f32::total_cmp).1
    };
    Qa {
        peak_dbfs: 20.0 * peak.log10(),
        max_step_ratio,
        non_finite: x.iter().filter(|s| !s.is_finite()).count(),
    }
}

impl Qa {
    /// Finite, at or under 0 dBFS, and click-free.
    pub fn passes(&self) -> bool {
        self.non_finite == 0 && self.peak_dbfs <= 0.0 && self.max_step_ratio <= CLICK_RATIO
    }
}

/// A source's row on the page.
#[derive(Clone, Debug)]
pub struct Src {
    /// The file names' middle: `<model>-<slug>-{dry,wet}.wav`.
    pub slug: String,
    pub title: String,
    pub author: String,
    pub licence: String,
    pub url: Option<String>,
}

/// `model`'s name in file names and on the command line: `loop`,
/// `pingpong`, `transwave`….
pub fn model_slug(model: TapeModel) -> String {
    model.disk_ident().to_lowercase().replace('_', "")
}

pub fn file_name(model: TapeModel, src: &Src, kind: &str) -> String {
    format!("{}-{}-{kind}.wav", model_slug(model), src.slug)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGE: &str = include_str!("page.html");

/// The listening page: a section per model, in `TapeModel::ALL`'s order,
/// a row per source, each with its transport.
pub fn page(rows: &[(TapeModel, &Src)]) -> String {
    let mut out = String::new();
    for m in TapeModel::ALL {
        let mut it = rows.iter().filter(|r| r.0 == m).peekable();
        if it.peek().is_none() {
            continue;
        }
        let name = m.disk_ident().replace('_', "-");
        let _ = write!(
            out,
            "<section><h2>{name}</h2><p class=\"legend\">{}</p>",
            esc(SWEEP_LEGEND)
        );
        for &(m, s) in it {
            let credit = match &s.url {
                Some(u) => format!("<a href=\"{}\">{}</a>", esc(u), esc(&s.author)),
                None => esc(&s.author),
            };
            let _ = write!(
                out,
                r#"<article class="track" data-dry="{dry}" data-wet="{wet}">
<div class="meta"><h3>{title}</h3><p>{credit} · {licence}</p></div>
<div class="transport">
<button type="button" data-act="play" aria-label="Play">▶</button>
<button type="button" data-act="stop" aria-label="Stop">■</button>
<button type="button" data-act="back" aria-label="Back 2 s">−2s</button>
<button type="button" data-act="fwd" aria-label="Forward 2 s">+2s</button>
<input type="range" data-act="seek" min="0" max="1000" value="0" aria-label="Seek">
<span class="time">0:00 / 0:00</span>
<button type="button" class="loop" data-act="loop" aria-pressed="false">LOOP</button>
<button type="button" class="ab" data-act="ab" aria-pressed="true" title="A/B: the dry source or the render, at the same place">WET</button>
</div>
</article>
"#,
                dry = esc(&file_name(m, s, "dry")),
                wet = esc(&file_name(m, s, "wet")),
                title = esc(&s.title),
                licence = esc(&s.licence),
            );
        }
        out.push_str("</section>\n");
    }
    PAGE.replace("<!--SECTIONS-->", &out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(slug: &str) -> Src {
        Src {
            slug: slug.into(),
            title: slug.to_uppercase(),
            author: "Chimera".into(),
            licence: "CC0 1.0".into(),
            url: Some("https://example.org/a?b&c".into()),
        }
    }

    #[test]
    fn page_lists_every_track() {
        let (a, b) = (src("tx-bass"), src("choir"));
        let rows = [(TapeModel::Loop, &a), (TapeModel::Loop, &b)];
        let html = page(&rows);
        for s in ["tx-bass", "choir"] {
            for kind in ["dry", "wet"] {
                let f = format!("loop-{s}-{kind}.wav");
                assert!(html.contains(&f), "{f} missing");
            }
        }
        for act in ["play", "stop", "seek", "back", "fwd", "loop", "ab"] {
            let n = html.matches(&format!("data-act=\"{act}\"")).count();
            assert_eq!(n, rows.len(), "{act} per row");
        }
        assert!(
            html.contains("https://example.org/a?b&amp;c"),
            "links escaped"
        );
    }

    #[test]
    fn qa_refuses_a_click() {
        let mut x: Vec<f32> = (0..48_000)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        assert!(qa(&x).passes(), "a clean sine passes: {:?}", qa(&x));
        x[24_000] += 0.3;
        assert!(!qa(&x).passes(), "{:?}", qa(&x));
    }
}
