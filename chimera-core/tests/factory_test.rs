//! Spec § Replacing the old engines: a bank of eight Algo Sounds that plays.

use chimera_core::dsp::voice::Voice;
use chimera_core::factory::{FACTORY_LEN, factory_sound, load_factory};
use chimera_core::preset::{ChainType, SoundPool};
use chimera_core::ui::UiState;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// One second held, then three seconds of release.
fn play(i: usize) -> (Vec<f32>, Vec<f32>) {
    let s = factory_sound(i).unwrap();
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &s.params,
    );
    let (mut held, mut tail) = (Vec::new(), Vec::new());
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..750 {
        v.render(&mut b, &s.params, &s.mod_state);
        held.extend_from_slice(&b);
    }
    v.note_off();
    for _ in 0..2250 {
        v.render(&mut b, &s.params, &s.mod_state);
        tail.extend_from_slice(&b);
    }
    (held, tail)
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// RMS of a one-pole high-pass at `cutoff_hz`: crude, but a smothering
/// low-pass (issue: the factory filter defaulted to 1 kHz on every FM
/// patch) knocks it down measurably.
fn highpass_rms(samples: &[f32], sample_rate: f32, cutoff_hz: f32) -> f32 {
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
    let dt = 1.0 / sample_rate;
    let a = rc / (rc + dt);
    let (mut y, mut prev_x, mut energy) = (0.0f32, 0.0f32, 0.0f32);
    for &x in samples {
        y = a * (y + x - prev_x);
        prev_x = x;
        energy += y * y;
    }
    (energy / samples.len() as f32).sqrt()
}

#[test]
fn eight_algo_sounds_with_distinct_names() {
    let names: Vec<String> = (0..FACTORY_LEN)
        .map(|i| {
            let s = factory_sound(i).unwrap();
            assert_eq!(s.chain_type, ChainType::Algo);
            s.name_str().to_string()
        })
        .collect();
    for (i, n) in names.iter().enumerate() {
        assert!(!n.is_empty() && !names[..i].contains(n), "{n}");
    }
    assert!(factory_sound(FACTORY_LEN).is_none());
}

#[test]
fn every_factory_sound_is_audible_finite_bounded_and_ends() {
    for i in 0..FACTORY_LEN {
        let name = factory_sound(i).unwrap().name_str().to_string();
        let (held, tail) = play(i);
        assert!(held.iter().chain(&tail).all(|x| x.is_finite()), "{name}");
        assert!(
            peak(&held) > 1e-2 && peak(&held) <= 1.0,
            "{name}: peak {}",
            peak(&held)
        );
        assert!(
            peak(&tail[tail.len() - 10 * BLOCK_SIZE..]) < 1e-4,
            "{name} rings on"
        );
    }
}

#[test]
fn the_morph_showcases_morph() {
    for i in [6, 7] {
        let s = factory_sound(i).unwrap();
        assert_ne!(s.params.algo.alg_a, s.params.algo.alg_b, "{}", s.name_str());
    }
    let pad = factory_sound(6).unwrap();
    assert_eq!(pad.mod_state.num_dests(), 1);
    assert_ne!(pad.mod_state.amount(1, 0), 0, "the LFO moves MORPH");
}

/// #33: the LFO's full swing must not push MORPH past its 0..=127 range —
/// that clips the sweep flat at one end for a stretch of every cycle.
#[test]
fn morph_pad_lfo_sweep_stays_inside_morph_range() {
    use chimera_core::dsp::lfo::Lfo;
    use chimera_core::modulation::MAX_MOD_SOURCES;

    let s = factory_sound(6).unwrap();
    assert_eq!(s.name_str(), "MORPH PAD");
    let base = s.params.algo.morph as f32;
    let mut lfo = Lfo::new();
    let period = (chimera_hal::SAMPLE_RATE as f32 / s.params.lfo.rate).ceil() as usize;
    for _ in 0..period {
        let mut sources = [0.0f32; MAX_MOD_SOURCES];
        sources[1] = lfo.process(&s.params.lfo, chimera_hal::SAMPLE_RATE);
        let off = s.mod_state.sum_for(0, &sources);
        let morph = base + off * 127.0; // MORPH's range is 0..=127
        assert!((0.0..=127.0).contains(&morph), "morph {morph} clips");
    }
}

#[test]
fn tx_epiano_bark_survives_the_filter() {
    let s = factory_sound(1).unwrap();
    assert_eq!(s.name_str(), "TX EPIANO");
    let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
    v.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &s.params,
    );
    // First 5 blocks (~7 ms): op2's fast, high-ratio bark (COARSE 42, D1L 0)
    // peaks here, before its own decay rolls it off. A filter cutoff left at
    // the shared 1 kHz default flattens this measurably (0.033 vs. 0.072,
    // calibrated against both); a smothered bark is the regression this
    // guards against.
    let mut early = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    for _ in 0..5 {
        v.render(&mut b, &s.params, &s.mod_state);
        early.extend_from_slice(&b);
    }
    let hf = highpass_rms(&early, chimera_hal::SAMPLE_RATE as f32, 2000.0);
    assert!(
        hf > 0.05,
        "TX EPIANO's bark is filtered out above 2 kHz: {hf}"
    );
}

#[test]
fn the_ui_starts_with_the_bank_in_the_pool() {
    let ui = UiState::new();
    for i in 0..FACTORY_LEN {
        let want = factory_sound(i).unwrap();
        assert_eq!(ui.pool.get(i).unwrap().name_str(), want.name_str());
    }
    assert!(ui.pool.get(FACTORY_LEN).is_none());
    let mut pool = SoundPool::new();
    load_factory(&mut pool);
    assert!(pool.get(FACTORY_LEN - 1).is_some() && pool.get(FACTORY_LEN).is_none());
}
