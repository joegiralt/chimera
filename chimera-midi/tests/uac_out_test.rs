// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! USB audio from the host (#885): the explicit-feedback servo and the
//! audio interrupt's priming, against a model of the unit and of Linux's
//! snd-usb-audio, where the SAI clock drifts from the host's SOF.

use chimera_midi::uac::NOMINAL;
use chimera_midi::uac_link::Ring;
use chimera_midi::uac_out::{
    self, BLOCK, Counts, Drain, Feedback, MAX_FRAMES, NOMINAL_FB, REFRESH, RING, TARGET,
};

#[test]
fn feedback_is_ten_fourteen_in_three_bytes() {
    assert_eq!(NOMINAL_FB, 48 << 14);
    assert_eq!(uac_out::bytes(NOMINAL_FB), [0x00, 0x00, 0x0C]);
    assert_eq!(uac_out::bytes(NOMINAL_FB + 0x1234), [0x34, 0x12, 0x0C]);
}

#[test]
fn feedback_starts_nominal_and_leans_against_the_fill() {
    let mut f = Feedback::new();
    assert_eq!(f.tick(TARGET), NOMINAL_FB);
    // Full: ask for less; empty: ask for more.
    let mut hi = Feedback::new();
    let mut lo = Feedback::new();
    for _ in 0..200 {
        assert!(hi.tick(TARGET + 64) < NOMINAL_FB);
        assert!(lo.tick(TARGET - 64) > NOMINAL_FB);
    }
}

/// Linux takes a value only in [nominal − 1/8, the packet's room]
/// (sound/usb/endpoint.c `snd_usb_handle_sync_urb`); the servo never
/// leaves half a frame of nominal whatever the fill.
#[test]
fn feedback_stays_inside_what_linux_accepts() {
    let mut f = Feedback::new();
    for fill in [0, RING, 0, RING / 2, RING] {
        for _ in 0..2_000 {
            let v = f.tick(fill);
            assert!(v.abs_diff(NOMINAL_FB) <= 1 << 14, "{v:#x}");
        }
    }
}

#[test]
fn drain_primes_then_takes_blocks_then_primes_again_when_dry() {
    let mut d = Drain::new();
    let mut c = Counts::default();
    assert!(!d.take(TARGET - 1, &mut c), "priming");
    assert!(d.take(TARGET, &mut c));
    assert!(d.take(BLOCK, &mut c));
    assert!(!d.take(BLOCK - 1, &mut c), "dry");
    assert_eq!(c.short, 1);
    assert!(!d.take(TARGET - 1, &mut c), "priming again");
    assert!(d.take(TARGET, &mut c));
    d.stop();
    assert!(!d.take(TARGET - 1, &mut c));
}

/// A deterministic generator.
struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Default, Debug)]
struct Run {
    counts: Counts,
    /// Packets the ring had no room for.
    refused: u32,
    /// Feedback values Linux would have ignored.
    ignored: u32,
    fill_min: usize,
    fill_max: usize,
    packet_min: usize,
    packet_max: usize,
    /// Frames the host lost (skipped frames), and the gaps the drain saw.
    lost: u64,
    gaps: u64,
}

/// The unit for `seconds`: the host's 1 ms frames carry packets sized as
/// Linux sizes them (`synced_next_packet_size`: a 16.16 phase plus the
/// last feedback, capped at the packet's room), reading feedback every
/// 2^`REFRESH` frames, a frame late, as a 3-byte value it shifts to 16.16
/// and takes only in [nominal − 1/8, the room]. A loaded host loses a
/// frame's packet with chance `skip`. The SAI takes a 64-frame block every
/// `block_us`, off by `ppm`; each block takes from the ring or plays
/// silence. Every frame taken is checked in order.
fn model(ppm: f64, skip: f64, seconds: u64, seed: u64) -> Run {
    let ring = Ring::<RING>::new();
    let mut rng = Lcg(seed);
    let block_us = BLOCK as f64 / 48_000.0 * 1e6 * (1.0 - ppm * 1e-6);
    let freqn: u32 = (NOMINAL as u32) << 16;
    let freqmax: u32 = (MAX_FRAMES as u32) << 16;
    let mut freqm = freqn;
    let mut phase: u32 = 0;
    let mut fb = Feedback::new();
    let mut drain = Drain::new();
    let mut armed = NOMINAL_FB;
    let mut run = Run {
        fill_min: usize::MAX,
        packet_min: usize::MAX,
        ..Run::default()
    };
    let mut sent: i64 = 0;
    let mut want: i64 = -1;
    let mut next_block = block_us;
    let ms_end = seconds * 1_000;
    for ms in 0..ms_end {
        let t = ms as f64 * 1_000.0;
        // The SAI's blocks due before this frame.
        while next_block <= t {
            next_block += block_us;
            let fill = ring.len();
            if drain.take(fill, &mut run.counts) {
                run.fill_min = run.fill_min.min(fill);
                ring.pop(BLOCK, |_, [v, _]| {
                    if want >= 0 && v as i64 != want {
                        assert!(v as i64 > want, "frame repeated");
                        run.gaps += (v as i64 - want) as u64;
                    }
                    want = v as i64 + 1;
                });
            }
        }
        // SOF: the device's feedback for the next read.
        let fill = ring.len();
        run.fill_max = run.fill_max.max(fill);
        let value = fb.tick(fill);
        // The host reads the value armed a frame ago.
        if ms % (1 << REFRESH) == 0 {
            let bytes = uac_out::bytes(armed);
            let f = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]) << 2;
            if f >= freqn - freqn / 8 && f <= freqmax {
                freqm = f;
            } else {
                run.ignored += 1;
            }
        }
        armed = value;
        // The host's packet for this frame.
        phase = (phase & 0xffff) + freqm;
        let n = ((phase >> 16) as usize).min(MAX_FRAMES);
        run.packet_min = run.packet_min.min(n);
        run.packet_max = run.packet_max.max(n);
        let base = sent;
        sent += n as i64;
        if rng.unit() < skip {
            run.lost += n as u64;
            continue;
        }
        if !ring.push(n, |i| [(base + i as i64) as i32, 0]) {
            run.refused += 1;
        }
    }
    run
}

fn clean(r: &Run) {
    assert_eq!((r.counts.short, r.refused, r.ignored), (0, 0, 0), "{r:?}");
    assert!(r.fill_max < RING, "{r:?}");
}

/// The SAI's crystal against the host's, ±300 ppm, a minute each: the
/// feedback holds the ring, no block plays silence once primed, every
/// frame arrives once, in order, and Linux takes every value.
#[test]
fn stream_holds_across_drift() {
    for (i, ppm) in [-300.0, -100.0, 0.0, 100.0, 300.0].into_iter().enumerate() {
        let r = model(ppm, 0.0, 60, 1 + i as u64);
        clean(&r);
        assert_eq!(r.gaps, 0, "{r:?}");
        // Settled at the target, give or take a block's sawtooth.
        assert!(r.fill_min + BLOCK >= TARGET - 16, "{ppm}: {r:?}");
        assert!(r.fill_max <= TARGET + BLOCK + 16, "{ppm}: {r:?}");
        // Packets of 47 to 49 frames.
        assert!(
            r.packet_min + 1 >= NOMINAL && r.packet_max <= NOMINAL + 1,
            "{ppm}: {r:?}"
        );
    }
}

/// A host losing a packet now and then (a missed service interval drops
/// the frame's data): each loss is a gap in the sound, but the ring never
/// runs dry.
#[test]
fn stream_holds_when_the_host_loses_a_packet() {
    for (i, ppm) in [-300.0, 0.0, 300.0].into_iter().enumerate() {
        let r = model(ppm, 0.001, 60, 40 + i as u64);
        clean(&r);
        assert!(r.gaps > 0 && r.gaps <= r.lost, "{r:?}");
    }
}

/// A host losing 1 % of its packets (the unit's loaded host skipped
/// 0.65 % of IN tokens) outruns any servo: the ring runs dry now and
/// then and primes again, but the feedback stays in Linux's window, the
/// ring never overflows, and nothing plays twice.
#[test]
fn stream_recovers_from_a_lossy_host() {
    let r = model(300.0, 0.01, 60, 50);
    assert_eq!((r.refused, r.ignored), (0, 0), "{r:?}");
    assert!(r.counts.short < 100 && r.gaps <= r.lost, "{r:?}");
}

/// An hour at the crystal's worst, clean; and with 0.1 % of packets lost,
/// at most a dry block or two (three losses inside the servo's half
/// second).
/// `cargo test -p chimera-midi --test uac_out_test -- --ignored`.
#[test]
#[ignore]
fn stream_holds_for_an_hour() {
    clean(&model(300.0, 0.0, 3_600, 99));
    clean(&model(-300.0, 0.0, 3_600, 98));
    let r = model(300.0, 0.001, 3_600, 97);
    assert_eq!((r.refused, r.ignored), (0, 0), "{r:?}");
    assert!(r.counts.short <= 2, "{r:?}");
}

/// A packet's 3-byte subframes into the ring as DAC words (bits 31..8),
/// whole frames only; a ring without room drops the packet whole,
/// counted, and keeps what it held.
#[test]
fn receive_unpacks_whole_frames_or_drops_the_packet() {
    let r = Ring::<8>::new();
    let mut c = Counts::default();
    let p = [
        0x56, 0x34, 0x12, 0xFF, 0xFF, 0xFF, 1, 0, 0x80, 0, 0, 0x7F, 9,
    ];
    uac_out::receive(&p, &r, &mut c);
    assert_eq!((c.received, c.overruns), (1, 0));
    let mut got = Vec::new();
    r.pop(8, |_, f| got.push(f));
    assert_eq!(got, [[0x1234_5600, -256], [i32::MIN | 0x100, 0x7F00_0000]]);
    // Five frames, then a packet of five more: no room, dropped whole.
    let five = [0u8; 30];
    uac_out::receive(&five, &r, &mut c);
    uac_out::receive(&five, &r, &mut c);
    assert_eq!((c.received, c.overruns, r.len()), (3, 1, 5));
}

/// A render's block from the ring: silence (nothing taken) until primed
/// and whenever the ring is short, never a partial or stale block.
#[test]
fn a_dry_ring_plays_silence_not_garbage() {
    let r = Ring::<{ RING }>::new();
    let mut d = Drain::new();
    let mut c = Counts::default();
    let mut out = [0.5f32; 2 * BLOCK];
    let mut g = uac_out::Fade::new();
    assert!(!uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut out));
    assert!(out.iter().all(|&x| x == 0.5), "nothing added");
    r.push(TARGET, |_| [1 << 30, -(1 << 30)]);
    assert!(uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut out));
    // A fade in from silence over the first block, never a step.
    assert_eq!(out[0], 0.5);
    assert!(out[2 * BLOCK - 2] > 0.5 + 0.12 && out[2 * BLOCK - 1] < 0.5 - 0.12);
    for w in out.chunks(2).collect::<Vec<_>>().windows(2) {
        assert!(w[1][0] >= w[0][0]);
    }
    // The ring short of a block: silence, counted, and the gain falls to
    // 0 so the next start fades in again.
    while r.len() >= BLOCK {
        uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut [0.0; 2 * BLOCK]);
    }
    r.push(BLOCK - 1, |_| [1 << 30, 0]);
    let mut out = [0.0f32; 2 * BLOCK];
    assert!(!uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut out));
    assert!(out.iter().all(|&x| x == 0.0));
    assert_eq!(c.short, 1);
}

/// The level eases across a block from where it was, never snaps.
#[test]
fn level_changes_ease_across_a_block() {
    let r = Ring::<{ RING }>::new();
    let mut d = Drain::new();
    let mut c = Counts::default();
    let mut g = uac_out::Fade::new();
    r.push(RING - 1, |_| [1 << 30, 1 << 30]);
    let half = (1 << 30) as f32 / 2_147_483_648.0;
    let mut out = [0.0f32; 2 * BLOCK];
    uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut out);
    let mut out = [0.0f32; 2 * BLOCK];
    uac_out::play(&r, &mut d, &mut c, &mut g, 1.0, &mut out);
    assert!(
        out.iter().all(|&x| (x - half).abs() < 1e-6),
        "unity: {}",
        out[0]
    );
    let mut out = [0.0f32; 2 * BLOCK];
    uac_out::play(&r, &mut d, &mut c, &mut g, 0.0, &mut out);
    assert!((out[0] - half).abs() < half / 32.0 && out[2 * BLOCK - 1].abs() < half / 32.0);
}

/// USB IN's steps: OFF, rising to 0 dB.
#[test]
fn usb_in_levels_run_from_off_to_unity() {
    assert_eq!(uac_out::gain(0), 0.0);
    assert_eq!(uac_out::gain(uac_out::LEVELS - 1), 1.0);
    for s in 1..uac_out::LEVELS {
        assert!(uac_out::gain(s) > uac_out::gain(s - 1));
    }
    assert!((uac_out::gain(5) - 0.501).abs() < 0.001, "-6 dB");
    assert_eq!(uac_out::gain(200), 1.0);
}
