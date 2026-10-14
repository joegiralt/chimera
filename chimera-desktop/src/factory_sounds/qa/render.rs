//! Playing a Sound headlessly (spec § 4): a `Plan` through the real
//! `Instrument` (as `loudness_test` plays a scene: Part 1, P1, after the
//! limiter), offline at 48 kHz, at the unit's voice budget.

use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::limiter::OUTPUT_GAIN;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_PARTS, MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument, MixState, mix_parts};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use super::super::phrase::{Plan, hit, plan};
use super::super::source::{Script, Source, to_sound};
use super::SR;

/// The limiter's lookahead: the output is a block late.
pub(super) const LATE: usize = BLOCK_SIZE;

/// Where each of `p`'s notes starts sounding in its take: its block,
/// a block late (the limiter).
pub(super) fn note_ons(p: &Plan) -> Vec<usize> {
    p.hits
        .iter()
        .map(|h| block(h.on) * BLOCK_SIZE + LATE)
        .collect()
}

/// The block an instant falls in.
pub(super) fn block(t: f32) -> usize {
    (t * SR / BLOCK_SIZE as f32).round() as usize
}

/// A played plan: P1 after the limiter (interleaved stereo), and the most
/// gain reduction the limiter took in any block, dB (0 or more).
pub(super) struct Take {
    pub(super) stereo: Vec<f32>,
    pub(super) reduced_db: f32,
    /// The most voices sounding at once.
    pub(super) voices: usize,
}

impl Take {
    pub(super) fn mono(&self) -> Vec<f32> {
        self.stereo.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }
}

thread_local! {
    /// The scope every `play` writes (the UI's, unread here): one buffer per
    /// thread, allocated once, not one leaked per render.
    static SCOPE: std::cell::RefCell<ScopeWriter> =
        std::cell::RefCell::new(ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0));
}

/// The shipping unit's budget (rev V; `Instrument` reserves the build's
/// FX bus, with or without the master tape): every render plays the voices
/// the unit can, stealing past them.
pub const UNIT_BUDGET: SampleBudget = SampleBudget::for_cpu(CPU_HZ_REV_V);
/// The chip `UNIT_BUDGET` is, as the page and report name it.
pub const UNIT_REV: &str = "rev V (480 MHz)";

/// `plan` played by `snd` on Part 1 in `mode`, offline at 48 kHz through
/// `Instrument`: what `loudness_test`'s scenes measure.
pub(super) fn play(snd: &Sound, mode: PartMode, plan: &Plan) -> Take {
    play_at(snd, mode, plan, UNIT_BUDGET)
}

/// `play` at `budget`.
pub(super) fn play_at(snd: &Sound, mode: PartMode, plan: &Plan, budget: SampleBudget) -> Take {
    let mut perf = Performance::new();
    let e = perf.edit(PartId::ALL[0]);
    **e.sound = snd.clone();
    e.mix.mode = plan.mode.unwrap_or(mode);
    let channel = perf.part(PartId::ALL[0]).mix.channel;
    play_on(&perf, channel, plan, budget)
}

/// `plan` played on `channel` of `perf`, every Part and the FX bus as
/// set: the Parts on that channel sound.
pub(super) fn play_on(
    perf: &Performance,
    channel: MidiChannel,
    plan: &Plan,
    budget: SampleBudget,
) -> Take {
    let shared = Box::new(AudioShared::from_performance(perf));
    let mut inst = Box::new(Instrument::new(SAMPLE_RATE, budget));
    let mut fx = Box::new(FxBus::new());
    let ev = |note: u8, kind| NoteEvent {
        channel,
        note: MidiNote::new(note).expect("a MIDI note"),
        kind,
    };
    SCOPE.with_borrow_mut(|scope| {
        metered(block(plan.secs), |b, dac| {
            for h in plan.hits.iter().filter(|h| block(h.off) == b) {
                inst.handle_with(ev(h.note, NoteKind::Off), &shared, &GateView::empty());
            }
            for h in plan.hits.iter().filter(|h| block(h.on) == b) {
                let v = Velocity::new(h.vel).expect("a velocity");
                inst.handle_with(ev(h.note, NoteKind::On(v)), &shared, &GateView::empty());
            }
            inst.render_with(&mut fx, dac, &shared, scope, &GateView::empty());
            inst.sounding()
        })
    })
}

/// `blocks` blocks, each rendered into the DAC by `render`, kept as P1
/// after the limiter, with the most gain reduction it took.
pub(super) fn metered(
    blocks: usize,
    mut render: impl FnMut(usize, &mut DacBlocks) -> usize,
) -> Take {
    let mut dac = Box::new(DacBlocks::new());
    let energy = |p: &[f32]| p.iter().map(|v| v * v).sum::<f32>();
    let mut t = Take {
        stereo: Vec::new(),
        reduced_db: 0.0,
        voices: 0,
    };
    let mut prev_in = 0.0f32;
    for b in 0..blocks {
        t.voices = t.voices.max(render(b, &mut dac));
        let out = &dac.out()[0];
        // The block out against the block in before it, over the rest gain.
        if prev_in > 1e-12 {
            let gr = 20.0
                * ((energy(out) / prev_in).sqrt() / OUTPUT_GAIN)
                    .max(1e-9)
                    .log10();
            t.reduced_db = t.reduced_db.max(-gr);
        }
        prev_in = energy(&dac.input()[0]);
        t.stereo.extend_from_slice(out);
    }
    t
}

/// The most voices the unit sounds for `src` on its own Part: eight notes
/// struck together and held 1 s (a note past the budget steals or waits).
pub fn unit_voices(src: &Source) -> Result<usize, String> {
    Ok(voices_of(&dried(&to_sound(src)?), src.demo))
}

pub(super) fn voices_of(snd: &Sound, mode: PartMode) -> usize {
    let held = Plan {
        hits: (0..MAX_VOICES as u8)
            .map(|i| hit(0.0, 1.0, 48 + 2 * i, 100))
            .collect(),
        secs: 1.0,
        mode: Some(mode),
    };
    play(snd, mode, &held).voices
}

/// The most voices sounding at once while `src` plays `s`.
pub fn most_voices(src: &Source, s: Script) -> Result<usize, String> {
    Ok(play(&dried(&to_sound(src)?), src.demo, &plan(s, src.home)).voices)
}

/// The listening render: the Source's Sound played under `s` at 48 kHz,
/// mono `(L + R)/2`, its sends (if any) through the FX bus at NEW's
/// settings (`Performance::new`).
pub fn render(src: &Source, s: Script) -> Result<Vec<f32>, String> {
    Ok(play(&to_sound(src)?, src.demo, &plan(s, src.home)).mono())
}

/// `render` with the Sound's sends at 0: what the gates measure, so levels
/// compare with ADR 0109's whatever the effects add.
pub fn dry(src: &Source, s: Script) -> Result<Vec<f32>, String> {
    Ok(play(&dried(&to_sound(src)?), src.demo, &plan(s, src.home)).mono())
}

/// `snd` with its sends (ADR 0123) at 0.
pub(super) fn dried(snd: &Sound) -> Sound {
    let mut d = snd.clone();
    d.params.out.sends = [0.0; FX_SENDS];
    d
}

/// The effects `snd`'s sends feed, in `FxBus` order, by their full names.
pub fn heard_through(snd: &Sound) -> Vec<&'static str> {
    ["CHORUS", "DELAY", "REVERB"]
        .into_iter()
        .zip(snd.params.out.sends)
        .filter(|&(_, s)| s > 0.0)
        .map(|(n, _)| n)
        .collect()
}

/// `x` straight onto Part 1's bus through the mixer (`mix_parts`): its
/// pair, its `sends` into the FX bus as `perf` sets it, then the master
/// section and the limiter. Block 0 is silent, so every gain lands first.
pub(super) fn play_stim(
    fx_params: &FxParams,
    sends: [f32; FX_SENDS],
    x: &[f32],
    secs: f32,
) -> Take {
    let mut perf = Performance::new();
    perf.fx = *fx_params;
    let mut snd = Sound::neutral(EngineType::Va);
    snd.params.out.sends = sends;
    **perf.edit(PartId::ALL[0]).sound = snd;
    let shared = Box::new(AudioShared::from_performance(&perf));
    let mut fx = Box::new(FxBus::new());
    let mut mix = MixState::default();
    let mut buses = Box::new([[0.0f32; BLOCK_SIZE]; MAX_PARTS]);
    let mut sends_buf = [[0.0f32; BLOCK_SIZE]; FX_SENDS];
    metered(block(secs) + 1, |b, dac| {
        let from = b.saturating_sub(1) * BLOCK_SIZE;
        let mut written = [false; MAX_PARTS];
        written[0] = b > 0;
        buses[0] = core::array::from_fn(|i| {
            if b > 0 {
                x.get(from + i).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        });
        mix_parts(
            &buses,
            &written,
            &mut sends_buf,
            &mut mix,
            &mut fx,
            &shared,
            Tempo::DEFAULT,
            SAMPLE_RATE,
            dac,
        );
        0
    })
}
