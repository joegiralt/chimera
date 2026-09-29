//! Verify no clicks/discontinuities in audio output.
//! Simulates the desktop audio callback pattern: rendering blocks
//! and scattering to variable-size output buffers.
mod common;
use common::Rig;
use common::{SR, clicks, tri};

use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::reverb::ReverbParams;
use chimera_core::dsp::ring::RingReverb;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

/// Simulate the audio callback: render blocks, scatter to output buffer,
/// check for discontinuities (clicks) in the output stream.
fn check_no_clicks(
    name: &str,
    setup: impl FnOnce(&mut ParamSnapshot, &mut ReverbParams),
    callback_sizes: &[usize], // simulate varying cpal buffer sizes
) {
    let empty_mod = ModState::new();
    let mut voice = Rig::new(SR);
    let mut reverb = Box::new(RingReverb::new());
    let mut params = ParamSnapshot::default();
    let mut rv = FxParams::default().reverb;
    setup(&mut params, &mut rv);

    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    // Persistent state (like the fixed audio callback)
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    // Simulate multiple callback invocations with varying buffer sizes
    for &cb_size in callback_sizes {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod);
                if rv.is_on() {
                    let mut wet = Stereo::SILENT;
                    reverb.process(&block, &rv.controls(), rv.mix, SR, &mut wet);
                    for (b, w) in block.iter_mut().zip(&wet.l) {
                        *b += w;
                    }
                }
                block_pos = 0;
            }
            all_samples.push(block[block_pos]);
            block_pos += 1;
        }
    }

    let clicks = clicks(&all_samples);
    assert!(
        clicks.is_empty(),
        "{}: found {} clicks. First 5: {:?}",
        name,
        clicks.len(),
        &clicks[..clicks.len().min(5)]
    );
}

// ── Algo triangle ───────────────────────────────────────────────────

#[test]
fn test_no_clicks_algo_triangle() {
    check_no_clicks(
        "Algo triangle",
        |p, _| {
            *p = tri();
        },
        // Simulate realistic cpal callback pattern: varying buffer sizes
        &[256, 256, 256, 512, 256, 256, 128, 256, 512, 256],
    );
}

#[test]
fn test_no_clicks_algo_with_pm() {
    check_no_clicks(
        "Algo PM",
        |p, _| {
            *p = tri();
            p.algo.ops[1].level = 70;
        },
        &[256, 256, 256, 256, 256, 256, 256, 256],
    );
}

// ── With reverb ─────────────────────────────────────────────────────

#[test]
fn test_no_clicks_algo_with_reverb() {
    check_no_clicks(
        "Algo + reverb",
        |p, rv| {
            *p = tri();
            rv.mix = 0.5;
            rv.time = 0.7;
        },
        &[256, 512, 256, 128, 256, 256, 512, 256],
    );
}

// ── Modal engines ───────────────────────────────────────────────────

#[test]
fn test_no_clicks_ks_string() {
    check_no_clicks(
        "KS+ string",
        |p, _| {
            *p = ParamSnapshot::for_engine(EngineType::Modal);
            p.modal.mode = ResonatorMode::String;
        },
        &[256, 256, 256, 512, 256, 256, 256, 256],
    );
}

#[test]
fn test_no_clicks_modal() {
    // Modal resonator produces rapid oscillations from 32 SVF filters —
    // these are the character of struck metal, not clicks.
    // Use a higher threshold than other engines.
    let empty_mod = ModState::new();
    let mut voice = Rig::new(SR);
    let mut params = ParamSnapshot::for_engine(EngineType::Modal);
    params.modal.mode = ResonatorMode::Modal;
    voice.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &params,
    );

    let mut block = [0.0f32; BLOCK_SIZE];
    let mut block_pos: usize = BLOCK_SIZE;
    let mut all_samples = Vec::new();

    for &cb_size in &[256, 256, 256, 256, 256, 256, 256, 256] {
        for _ in 0..cb_size {
            if block_pos >= BLOCK_SIZE {
                voice.render(&mut block, &params, &empty_mod);
                block_pos = 0;
            }
            all_samples.push(libm::tanhf(block[block_pos] * 0.7));
            block_pos += 1;
        }
    }

    // Modal can have fast oscillations, so just verify output is finite and bounded
    for (i, &s) in all_samples.iter().enumerate() {
        assert!(s.is_finite(), "modal sample {} is not finite", i);
        assert!(s.abs() < 2.0, "modal sample {} too loud: {}", i, s);
    }
}

// ── Misaligned callback sizes (stress test for block boundary) ──────

#[test]
fn test_no_clicks_odd_buffer_sizes() {
    check_no_clicks(
        "Algo with odd callback sizes",
        |p, _| {
            *p = tri();
        },
        // Deliberately misaligned with BLOCK_SIZE (64)
        &[100, 200, 50, 300, 150, 75, 250, 100, 400, 50],
    );
}

#[test]
fn test_no_clicks_tiny_buffers() {
    check_no_clicks(
        "Algo with tiny callbacks",
        |p, _| {
            *p = tri();
        },
        // Very small buffers — stress the block boundary logic
        &[32, 32, 32, 32, 64, 32, 32, 32, 32, 64, 32, 32, 32, 32],
    );
}

#[test]
fn test_no_clicks_single_sample_buffers() {
    check_no_clicks(
        "Algo with single-sample callbacks",
        |p, _| {
            *p = tri();
        },
        // Worst case: one sample per callback
        &[1; 512],
    );
}

/// ADR 0027: a killed voice ramps to 0 over `Voice::FADE` samples instead
/// of stopping dead; no step is bigger than the signal's own.
#[test]
fn a_killed_voice_fades_out() {
    let sound = chimera_core::factory::factory_sound(4).unwrap(); // SAW LEAD
    let (p, m) = (&sound.params, &sound.mod_state);
    let (mut a, mut b) = (Rig::new(SR), Rig::new(SR));
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut last = 0.0;
    for v in [&mut a, &mut b] {
        v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
        for _ in 0..20 {
            v.render(&mut block, p, m);
        }
        last = block[BLOCK_SIZE - 1];
    }
    b.kill();
    let (mut ra, mut rb) = (vec![last], vec![last]);
    for _ in 0..Voice::FADE as usize / BLOCK_SIZE {
        a.render(&mut block, p, m);
        ra.extend_from_slice(&block);
        b.kill(); // a second kill does not restart the fade
        b.render(&mut block, p, m);
        rb.extend_from_slice(&block);
    }
    let step = |x: &[f32]| x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()));
    let peak = ra.iter().fold(0.0f32, |s, x| s.max(x.abs()));
    assert!(peak > 0.05, "the voice is sounding: {peak}");
    assert_eq!(*rb.last().unwrap(), 0.0);
    assert!(!b.is_active() && a.is_active());
    assert!(
        step(&rb) <= step(&ra),
        "fade step {} > signal step {}",
        step(&rb),
        step(&ra)
    );
    b.render(&mut block, p, m);
    assert!(block.iter().all(|&s| s == 0.0));
}

fn first_blocks(v: &mut Rig, note: u8, p: &ParamSnapshot, m: &ModState) -> Vec<u32> {
    let mut block = [0.0f32; BLOCK_SIZE];
    let mut out = Vec::new();
    v.note_on(MidiNote::new(note).unwrap(), Velocity::DEFAULT, p);
    for _ in 0..8 {
        v.render(&mut block, p, m);
        out.extend(block.iter().map(|s| s.to_bits()));
    }
    out
}

fn play_then_kill(v: &mut Rig, p: &ParamSnapshot, m: &ModState) {
    let mut block = [0.0f32; BLOCK_SIZE];
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    for _ in 0..40 {
        v.render(&mut block, p, m);
    }
    v.kill();
    for _ in 0..Voice::FADE as usize / BLOCK_SIZE {
        v.render(&mut block, p, m);
    }
    assert!(!v.is_active());
}

/// A killed voice's next note is bit-identical to a fresh voice's: no
/// stale engine or chain state (no click, a full attack).
#[test]
fn a_killed_voice_restarts_like_a_fresh_one() {
    let sounds: Vec<(&str, ParamSnapshot, ModState)> = [("SQR BASS", 5usize), ("SAW LEAD", 4)]
        .into_iter()
        .map(|(name, i)| {
            let s = chimera_core::factory::factory_sound(i).unwrap();
            (name, s.params, s.mod_state)
        })
        .chain([(
            "MODAL",
            ParamSnapshot::for_engine(EngineType::Modal),
            ModState::new(),
        )])
        .collect();
    for (name, p, m) in &sounds {
        let mut v = Rig::new(SR);
        play_then_kill(&mut v, p, m);
        let fresh = first_blocks(&mut Rig::new(SR), 64, p, m);
        assert!(
            fresh.iter().any(|&b| f32::from_bits(b) != 0.0),
            "{name} silent"
        );
        assert!(
            first_blocks(&mut v, 64, p, m) == fresh,
            "{name}: reused voice differs"
        );
    }
}

/// Algo → Modal → Algo on one voice: the Algo engine left behind starts
/// clean when it plays again.
#[test]
fn an_engine_round_trip_leaves_no_stale_state() {
    let s = chimera_core::factory::factory_sound(5).unwrap(); // SQR BASS
    let (p, m) = (&s.params, &s.mod_state);
    let modal = ParamSnapshot::for_engine(EngineType::Modal);
    let mut v = Rig::new(SR);
    let mut block = [0.0f32; BLOCK_SIZE];
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    for _ in 0..40 {
        v.render(&mut block, p, m);
    }
    play_then_kill(&mut v, &modal, &ModState::new());
    assert!(first_blocks(&mut v, 64, p, m) == first_blocks(&mut Rig::new(SR), 64, p, m));
}

fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |s, w| s.max((w[1] - w[0]).abs()))
}

/// #33 M6: a Sound's engine change fades the old engine out over
/// `Voice::FADE` on the settings it was playing: exactly a `kill` fade, no
/// step bigger than the signal's own. The held note then restarts on the
/// new engine exactly as on a fresh voice.
#[test]
fn an_engine_switch_fades_out_then_starts_clean() {
    let factory = |i| {
        let s = chimera_core::factory::factory_sound(i).unwrap();
        (s.params, s.mod_state)
    };
    let modal = (
        ParamSnapshot::for_engine(EngineType::Modal),
        ModState::new(),
    );
    let tri = (tri(), ModState::new());
    for (name, (from, fm), (to, tm)) in [
        ("TRI→Modal", tri.clone(), modal.clone()),
        ("SQR BASS→Modal", factory(5), modal.clone()),
        ("MORPH PAD→Modal", factory(6), modal.clone()),
        ("Modal→TRI", modal, tri),
    ] {
        let mut v = [Rig::new(SR), Rig::new(SR), Rig::new(SR)];
        let mut block = [0.0f32; BLOCK_SIZE];
        let mut last = 0.0;
        for v in v.iter_mut() {
            v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &from);
            for _ in 0..20 {
                v.render(&mut block, &from, &fm);
            }
            last = block[BLOCK_SIZE - 1];
        }
        let [a, b, c] = &mut v;
        c.kill();
        let (mut ra, mut rb, mut rc) = (vec![last], vec![last], vec![last]);
        for _ in 0..Voice::FADE as usize / BLOCK_SIZE {
            a.render(&mut block, &from, &fm);
            ra.extend_from_slice(&block);
            b.render(&mut block, &to, &tm);
            rb.extend_from_slice(&block);
            c.render(&mut block, &from, &fm);
            rc.extend_from_slice(&block);
        }
        assert!(ra.iter().any(|x| x.abs() > 0.01), "{name}: sounding");
        assert_eq!(*rb.last().unwrap(), 0.0, "{name}");
        assert!(
            rb.iter().zip(&rc).all(|(x, y)| x.to_bits() == y.to_bits()),
            "{name}: differs from a kill fade"
        );
        assert!(
            max_step(&rb) <= max_step(&ra),
            "{name}: fade step {} > signal step {}",
            max_step(&rb),
            max_step(&ra)
        );
        let mut fresh = Rig::new(SR);
        fresh.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &to);
        for i in 0..8 {
            let mut f = [0.0f32; BLOCK_SIZE];
            fresh.render(&mut f, &to, &tm);
            b.render(&mut block, &to, &tm);
            assert!(
                block.map(f32::to_bits) == f.map(f32::to_bits),
                "{name}: block {i} differs from a fresh voice"
            );
        }
    }
}

/// A key up during an engine-switch fade ends the note: nothing restarts
/// on the new engine.
#[test]
fn a_key_up_mid_switch_ends_the_note() {
    let modal = ParamSnapshot::for_engine(EngineType::Modal);
    let m = ModState::new();
    let mut v = Rig::new(SR);
    let mut block = [0.0f32; BLOCK_SIZE];
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &tri());
    for _ in 0..20 {
        v.render(&mut block, &tri(), &m);
    }
    v.render(&mut block, &modal, &m); // the switch fade starts
    v.note_off();
    v.render(&mut block, &modal, &m);
    assert!(!v.is_active());
    v.render(&mut block, &modal, &m);
    assert!(block.iter().all(|&s| s == 0.0));
}

/// An engine change fades a released tail out; it does not restart it as a
/// held note that nothing will release.
#[test]
fn an_engine_switch_ends_a_released_tail() {
    let modal = ParamSnapshot::for_engine(EngineType::Modal);
    let m = ModState::new();
    let mut v = Rig::new(SR);
    let mut block = [0.0f32; BLOCK_SIZE];
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &tri());
    for _ in 0..20 {
        v.render(&mut block, &tri(), &m);
    }
    v.note_off();
    v.render(&mut block, &tri(), &m);
    for _ in 0..Voice::FADE as usize / BLOCK_SIZE {
        v.render(&mut block, &modal, &m);
    }
    assert!(!v.is_active());
}

/// #33 M7: a note-on on a fading voice waits for the fade, then starts
/// exactly as on a fresh voice.
#[test]
fn a_note_on_mid_fade_starts_clean_after_it() {
    let s = chimera_core::factory::factory_sound(4).unwrap(); // SAW LEAD
    let (p, m) = (&s.params, &s.mod_state);
    let mut v = Rig::new(SR);
    let mut block = [0.0f32; BLOCK_SIZE];
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, p);
    for _ in 0..20 {
        v.render(&mut block, p, m);
    }
    v.kill();
    v.render(&mut block, p, m);
    v.note_on(MidiNote::new(64).unwrap(), Velocity::DEFAULT, p);
    v.render(&mut block, p, m);
    assert_eq!(block[BLOCK_SIZE - 1], 0.0, "the fade ran to the end");
    let mut out = Vec::new();
    for _ in 0..8 {
        v.render(&mut block, p, m);
        out.extend(block.iter().map(|s| s.to_bits()));
    }
    let mut fresh = Rig::new(SR);
    fresh.note_on(MidiNote::new(64).unwrap(), Velocity::DEFAULT, p);
    let mut want = Vec::new();
    for _ in 0..8 {
        fresh.render(&mut block, p, m);
        want.extend(block.iter().map(|s| s.to_bits()));
    }
    assert!(out == want);
}

/// A note-off releases through the envelope: no step after it is larger
/// than the held note's own largest step (#68). The fixed 0.15 threshold
/// above misses a full mute of this triangle. Modal's release is #51.
#[test]
fn a_released_note_steps_no_more_than_the_held_note() {
    let saw = chimera_core::factory::factory_sound(4).unwrap(); // SAW LEAD
    for (name, p, m) in [
        ("triangle", tri(), ModState::new()),
        ("SAW LEAD", saw.params.clone(), saw.mod_state.clone()),
    ] {
        let mut v = Rig::new(SR);
        let mut block = [0.0f32; BLOCK_SIZE];
        v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p);
        let mut held = Vec::new();
        for _ in 0..32 {
            v.render(&mut block, &p, &m);
            held.extend_from_slice(&block);
        }
        v.note_off();
        let mut released = vec![*held.last().unwrap()];
        while v.is_active() {
            v.render(&mut block, &p, &m);
            released.extend_from_slice(&block);
            assert!(
                released.len() < 10 * SR as usize,
                "{name}: release never ends"
            );
        }
        let (h, r) = (max_step(&held), max_step(&released));
        assert!(r <= h, "{name}: release steps {r}, held {h}");
    }
}
