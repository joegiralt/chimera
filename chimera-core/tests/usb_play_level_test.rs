// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's USB playback reaches the main out's DAC at the host's own
//! scale (ADR 0260): a host sample of full scale plays as the synth's full
//! scale does, and USB IN 0 dB (the default) is unity. The firmware's path
//! end to end, less the DMA: a packet's 3-byte subframes, the ring, the
//! render's block (`uac_out::play` on the limited block, as `usb_in`
//! does) and `to_dac`.

use chimera_core::audio_out::to_dac;
use chimera_core::hw::BLOCK_SIZE;
use chimera_core::ui::midi_settings::{MidiSettings, USB_IN_TOP};
use chimera_midi::uac_link::Ring;
use chimera_midi::uac_out::{self, Counts, Drain, Fade, RING, TARGET};

/// `frames` of the host's S24_3LE stereo, each `[l, r]` 24-bit.
fn packet(frames: &[[i32; 2]]) -> Vec<u8> {
    let mut p = Vec::new();
    for &[l, r] in frames {
        p.extend_from_slice(&l.to_le_bytes()[..3]);
        p.extend_from_slice(&r.to_le_bytes()[..3]);
    }
    p
}

/// The DAC words a steady host signal `[l, r]` plays at USB IN `step`,
/// on a silent synth, once the fade-in is past.
fn dac_words(l: i32, r: i32, step: u8) -> Vec<[i32; 2]> {
    let ring = Ring::<RING>::new();
    let mut c = Counts::default();
    let one = packet(&[[l, r]; 48]);
    while ring.len() < TARGET + BLOCK_SIZE {
        uac_out::receive(&one, &ring, &mut c);
    }
    let (mut d, mut f) = (Drain::new(), Fade::new());
    let gain = uac_out::gain(step);
    let mut out = [0.0f32; 2 * BLOCK_SIZE];
    assert!(uac_out::play(&ring, &mut d, &mut c, &mut f, gain, &mut out));
    let mut out = [0.0f32; 2 * BLOCK_SIZE];
    assert!(uac_out::play(&ring, &mut d, &mut c, &mut f, gain, &mut out));
    out.chunks(2)
        .map(|s| [to_dac(s[0]).get(), to_dac(s[1]).get()])
        .collect()
}

#[test]
fn usb_in_defaults_to_unity() {
    assert_eq!(MidiSettings::DEFAULT.usb_in(), USB_IN_TOP);
    assert_eq!(uac_out::gain(USB_IN_TOP), 1.0);
}

/// Full scale in is full scale out: the synth's own full scale
/// (`to_dac(±1.0)`), word for word; a quarter (−12 dBFS) is a quarter.
#[test]
fn a_host_sample_plays_at_the_synths_scale() {
    let top = (1 << 23) - 1;
    for w in dac_words(top, -top, USB_IN_TOP) {
        assert_eq!(w, [to_dac(1.0).get(), to_dac(-1.0).get()]);
    }
    let quarter = 1 << 21;
    for w in dac_words(quarter, -quarter, USB_IN_TOP) {
        assert_eq!(w, [to_dac(0.25).get(), to_dac(-0.25).get()]);
    }
    // −6 dB is half again, near enough: the step's gain, not a scale slip.
    let half = dac_words(quarter, 0, 5)[0][0] as f64 / to_dac(0.25).get() as f64;
    assert!((half - 0.501).abs() < 0.001, "{half}");
}
