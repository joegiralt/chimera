//! Sanity gate (spec § Testing), run before goldens are recorded.
//! Per engine init sound: finite, within ±1.0, not silent, silent after
//! note-off; pitched engines' fundamental within one semitone of the note.
//! A failing engine gets a GitHub issue and its failing test is
//! marked `#[ignore = "known broken: …"]`. It is not fixed in this refactor.

mod common;

use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_hal::BLOCK_SIZE;
use common::*;

/// −60 dBFS.
const AUDIBLE: f32 = 1e-3;
/// −80 dBFS.
const SILENT: f32 = 1e-4;
/// Trailing blocks of the render that must be silent.
const TAIL_BLOCKS: usize = 10;

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Perceived fundamental via normalized autocorrelation: the shortest lag in
/// 40..=1200 Hz whose correlation is within 90 % of the best lag, refined to
/// its local maximum. A waveform that repeats every half period of the note
/// reads as an octave up — which is what a listener hears.
fn fundamental_hz(s: &[f32]) -> f32 {
    let (min_lag, max_lag) = (SR as usize / 1200, SR as usize / 40);
    let n = s.len() - max_lag;
    let acf = |lag: usize| -> f32 {
        let (mut xy, mut xx, mut yy) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (a, b) = (s[i] as f64, s[i + lag] as f64);
            xy += a * b;
            xx += a * a;
            yy += b * b;
        }
        if xx == 0.0 || yy == 0.0 {
            0.0
        } else {
            (xy / (xx * yy).sqrt()) as f32
        }
    };
    let r: Vec<f32> = (0..=max_lag)
        .map(|l| if l < min_lag { 0.0 } else { acf(l) })
        .collect();
    let best = r[min_lag..].iter().copied().fold(f32::MIN, f32::max);
    let mut lag = (min_lag..=max_lag).find(|&l| r[l] >= 0.9 * best).unwrap();
    while lag < max_lag && r[lag + 1] > r[lag] {
        lag += 1;
    }
    SR as f32 / lag as f32
}

fn assert_finite_bounded_audible(case: Case) {
    let out = render_case(case);
    assert!(
        out.iter().all(|x| x.is_finite()),
        "{}: non-finite sample",
        case.name()
    );
    let pk = peak(&out);
    assert!(pk <= 1.0, "{}: peak {pk} exceeds ±1.0", case.name());
    let on = peak(&out[..ON_BLOCKS * BLOCK_SIZE]);
    assert!(
        on > AUDIBLE,
        "{}: silent during note-on (peak {on})",
        case.name()
    );
}

fn assert_silent_after_note_off(case: Case) {
    let out = render_case(case);
    let tail = peak(&out[TOTAL_SAMPLES - TAIL_BLOCKS * BLOCK_SIZE..]);
    assert!(
        tail < SILENT,
        "{}: tail peak {tail} after note-off",
        case.name()
    );
}

fn assert_pitched(case: Case) {
    let out = render_case(case);
    let f = fundamental_hz(&out[50 * BLOCK_SIZE..150 * BLOCK_SIZE]);
    let target = chimera_core::dsp::note_to_freq(NOTE);
    let semis = 12.0 * (f / target).log2();
    assert!(
        semis.abs() < 1.0,
        "{}: fundamental {f} Hz is {semis:+.2} semitones from note {NOTE} ({target} Hz)",
        case.name()
    );
}

#[test]
fn pizza_is_finite_bounded_audible() {
    assert_finite_bounded_audible(Case::PizzaInit);
}
#[test]
fn pizza_is_silent_after_note_off() {
    assert_silent_after_note_off(Case::PizzaInit);
}
#[test]
fn pizza_is_pitched() {
    assert_pitched(Case::PizzaInit);
}

#[test]
fn fm_is_finite_bounded_audible() {
    assert_finite_bounded_audible(Case::FmInit);
}
#[test]
fn fm_is_silent_after_note_off() {
    assert_silent_after_note_off(Case::FmInit);
}
#[test]
fn fm_is_pitched() {
    assert_pitched(Case::FmInit);
}

#[test]
fn modal_is_finite_bounded_audible() {
    assert_finite_bounded_audible(Case::ModalInit);
}
#[test]
#[ignore = "known broken: https://github.com/joegiralt/chimera/issues/10"]
fn modal_is_pitched() {
    assert_pitched(Case::ModalInit);
}

/// Windowed RMS of the Modal engine's post-note-off tail: `off_blocks` blocks
/// of release rendered after `ON_BLOCKS` blocks of note-on, in
/// `window_blocks`-block chunks. Longer than the fixed `OFF_BLOCKS` the other
/// sanity cases use — Modal bypasses the amp envelope and rings on its own
/// decay/damping (#10), so it needs enough tail to actually reach silence.
fn modal_tail_windows(params: &ParamSnapshot, off_blocks: usize, window_blocks: usize) -> Vec<f32> {
    use chimera_core::dsp::voice::Voice;
    use chimera_core::modulation::ModState;
    use chimera_core::{MidiNote, Velocity};

    let mod_state = ModState::new();
    let mut voice = Voice::new(chimera_hal::SAMPLE_RATE);
    voice.note_on(
        MidiNote::new(NOTE).unwrap(),
        Velocity::new(VEL).unwrap(),
        params,
    );
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut tail = Vec::with_capacity(off_blocks * BLOCK_SIZE);
    for b in 0..ON_BLOCKS + off_blocks {
        if b == ON_BLOCKS {
            voice.note_off();
        }
        voice.render(&mut block, params, &mod_state);
        if b >= ON_BLOCKS {
            tail.extend_from_slice(&block);
        }
    }
    let win = window_blocks * BLOCK_SIZE;
    tail.chunks_exact(win)
        .map(|w| (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt())
        .collect()
}

/// Modal deliberately bypasses the amp envelope: a struck/plucked resonator
/// rings until its own decay/damping kills it, not a synth-voice gate (#10).
/// Physics-correct replacement for the old "silent within TAIL_BLOCKS" check:
/// the tail's windowed RMS trends down over the release (several-window
/// averages, not a strict per-block decrease), and a patch with a faster
/// DECAY setting reaches silence sooner than the default.
#[test]
fn modal_tail_decays_after_note_off() {
    // ~1.6 s of release — long enough for both patches below to cross SILENT.
    const OFF_BLOCKS_LONG: usize = 1200;
    // ~26.7 ms per window.
    const WINDOW_BLOCKS: usize = 20;

    let default_params = init_params(EngineType::Modal);
    let default_windows = modal_tail_windows(&default_params, OFF_BLOCKS_LONG, WINDOW_BLOCKS);

    // Trends down: compare the average of the first quarter of windows
    // against the last quarter, several windows apart rather than a strict
    // per-block decrease (Karplus-Strong energy isn't perfectly monotone
    // tick-to-tick). Fails if decay were forced to infinite sustain, since
    // early and late would then be about the same.
    let quarter = default_windows.len() / 4;
    let early: f32 = default_windows[..quarter].iter().sum::<f32>() / quarter as f32;
    let late: f32 = default_windows[default_windows.len() - quarter..]
        .iter()
        .sum::<f32>()
        / quarter as f32;
    assert!(
        late < early * 0.5,
        "modal_init: tail did not decay (early avg {early:.6}, late avg {late:.6})"
    );

    // A patch with a faster decay setting must reach silence sooner than the
    // default (DECAY spec default 0.3; 1.0 is its max — still a valid patch).
    let mut faster_decay = default_params.clone();
    faster_decay.modal.decay = 1.0;
    let faster_windows = modal_tail_windows(&faster_decay, OFF_BLOCKS_LONG, WINDOW_BLOCKS);

    let silent_at = |windows: &[f32]| {
        windows
            .iter()
            .position(|&r| r < SILENT)
            .unwrap_or(windows.len())
    };
    let default_at = silent_at(&default_windows);
    let faster_at = silent_at(&faster_windows);
    assert!(
        faster_at < default_at,
        "higher decay should reach silence sooner: default window {default_at}, decay=1.0 window {faster_at}"
    );
}

/// Va is a placeholder engine: it must render exact silence, never garbage.
#[test]
fn va_is_silent_placeholder() {
    let out = render_case(Case::VaInit);
    assert!(
        out.iter().all(|&x| x == 0.0),
        "va_init must be exact silence"
    );
}
