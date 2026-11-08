//! USB audio's pure half (ADR 0248): the ring, the packet bytes, the
//! frame parity, and the pacer against a model of the unit, where the SAI
//! clock drifts from the host's SOF and the audio interrupt holds the USB
//! interrupt off while it renders.

use chimera_midi::uac::{
    self, CNAK, Counts, EPDIS, EPENA, HIGH, MAX_FRAMES, NOMINAL, Next, Pacer, SEVNFRM, SNAK,
    SODDFRM, TARGET,
};
use chimera_midi::uac_link::Ring;

#[test]
fn ring_takes_a_block_whole_or_not_at_all() {
    let r = Ring::<8>::new();
    assert!(r.push(6, |i| [i as i32, -(i as i32)]));
    assert!(!r.push(3, |_| [9, 9]), "no room for the whole block");
    assert_eq!(r.len(), 6);
    let mut got = Vec::new();
    assert_eq!(r.pop(4, |_, f| got.push(f)), 4);
    assert_eq!(got, [[0, 0], [1, -1], [2, -2], [3, -3]]);
    assert!(r.push(6, |i| [10 + i as i32, 0]), "wraps");
    assert_eq!(r.skip(3), 3);
    let mut rest = Vec::new();
    assert_eq!(r.pop(100, |_, f| rest.push(f[0])), 5);
    assert_eq!(rest, [11, 12, 13, 14, 15]);
    assert!(r.is_empty());
}

/// One producer thread, one consumer: every frame once, in order.
#[test]
fn ring_hands_frames_across_threads_in_order() {
    static R: Ring<64> = Ring::new();
    const N: i32 = 200_000;
    let producer = std::thread::spawn(|| {
        let mut next = 0;
        while next < N {
            let n = 16.min(N - next) as usize;
            if R.push(n, |i| [next + i as i32, !(next + i as i32)]) {
                next += n as i32;
            }
        }
    });
    let mut want = 0;
    while want < N {
        R.pop(7, |_, [a, b]| {
            assert_eq!((a, b), (want, !want));
            want += 1;
        });
    }
    producer.join().unwrap();
}

/// DAC words hold the sample in bits 31..8: its three bytes, little-endian.
#[test]
fn pack_takes_the_dac_words_top_three_bytes() {
    let mut b = [0u8; 6];
    uac::pack([0x1234_5600, -256], &mut b);
    assert_eq!(b, [0x56, 0x34, 0x12, 0xFF, 0xFF, 0xFF]);
    uac::pack([0x7FFF_FF00, i32::MIN], &mut b);
    assert_eq!(b, [0xFF, 0xFF, 0x7F, 0x00, 0x00, 0x80]);
}

/// The parity is the next frame's, every other bit kept, no action bit.
#[test]
fn aim_sets_the_next_frames_parity_only() {
    let ctl = EPENA | CNAK | (1 << 16) | (1 << 15) | (1 << 18) | 576;
    let even_now = uac::aim(ctl, 10);
    assert_eq!(even_now, SODDFRM | (1 << 16) | (1 << 15) | (1 << 18) | 576);
    let odd_now = uac::aim(ctl | EPDIS | SNAK, 11);
    assert_eq!(odd_now & (SODDFRM | SEVNFRM), SEVNFRM);
    assert_eq!(odd_now & (EPENA | EPDIS | SNAK | CNAK), 0);
    assert!(uac::armed(ctl) && !uac::armed(even_now));
    assert_eq!(uac::frame(0x0007_FF00 | 0xFF), 0x7FF);
}

#[test]
fn pacer_primes_then_sends_a_frames_worth() {
    let mut p = Pacer::new();
    let mut c = Counts::default();
    assert_eq!(p.next(TARGET - 1, 5, &mut c), Next { skip: 0, send: 0 });
    assert_eq!(p.next(TARGET, 6, &mut c).send, NOMINAL);
    assert_eq!(p.next(TARGET, 7, &mut c).send, NOMINAL);
    // A frame missed: two frames' worth.
    assert_eq!(p.next(TARGET + 40, 9, &mut c).send, 2 * NOMINAL);
    assert_eq!((c.sent, c.missed, c.short), (3, 1, 0));
}

#[test]
fn pacer_drops_back_from_a_stalled_host() {
    let mut p = Pacer::new();
    let mut c = Counts::default();
    p.next(TARGET, 1, &mut c);
    let n = p.next(HIGH + 50, 2, &mut c);
    assert_eq!(n.skip, HIGH + 50 - TARGET);
    assert_eq!(n.send, NOMINAL);
    assert_eq!(c.resyncs, 1);
}

#[test]
fn pacer_runs_dry_and_primes_again() {
    let mut p = Pacer::new();
    let mut c = Counts::default();
    p.next(TARGET, 1, &mut c);
    assert_eq!(p.next(20, 2, &mut c).send, 20);
    assert_eq!(c.short, 1);
    assert_eq!(p.next(60, 3, &mut c).send, 0, "priming");
}

/// A deterministic generator for the model's render times.
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
    /// Frames the producer's push was refused once the stream primed.
    refused: u32,
    /// Frames the host received, and the last frame's index.
    got: u64,
    fill_max: usize,
    fill_min: usize,
    packet_max: usize,
    packet_min: usize,
}

/// The unit for `seconds`: the SAI's 64-frame blocks at 48 kHz off by
/// `ppm` from the host's 1 ms frames, each block's render holding the USB
/// interrupt off for `load` (a range, of the 1.33 ms block) before its
/// frames reach the ring. The host polls 50 µs into each frame; a packet
/// goes out only in a frame of the parity it was aimed at; at 80 % of the
/// frame a packet still armed for it raises the incomplete interrupt,
/// which aims it again. The interrupt runs at a SOF, a completion and an
/// incomplete, once the render lets it, and arms a packet whenever none
/// is. Every frame the host gets is checked in order: nothing lost or
/// repeated.
fn model(ppm: f64, load: (f64, f64), seconds: u64, seed: u64) -> Run {
    const STEP: u64 = 10; // µs
    let ring = Ring::<{ uac::RING }>::new();
    let mut rng = Lcg(seed);
    let block_us = 64.0 / 48_000.0 * 1e6 * (1.0 - ppm * 1e-6);
    let mut next_dma = 0.0f64;
    let mut render_end: Option<u64> = None;
    let mut made: i32 = 0;
    let mut pacer = Pacer::new();
    let mut run = Run {
        fill_min: usize::MAX,
        packet_min: usize::MAX,
        ..Run::default()
    };
    // The endpoint: armed with a packet's frames and a parity.
    let mut armed: Option<(usize, u16)> = None;
    let mut packet = Vec::new();
    let mut pending = false;
    let mut incomplete = false;
    let mut want: i32 = -1;
    let end = seconds * 1_000_000;
    let mut t = 0u64;
    while t < end {
        let frame = (t / 1000) as u16 & 0x7FF;
        let in_frame = t % 1000;
        // The SAI: a block every `block_us`, its frames in the ring when
        // its render ends.
        if t as f64 >= next_dma {
            next_dma += block_us;
            let (lo, hi) = load;
            let l = (lo + (hi - lo) * rng.unit()) * block_us;
            render_end = Some(t + l as u64);
        }
        let rendering = render_end.is_some_and(|e| t < e);
        if let Some(e) = render_end
            && t >= e
        {
            render_end = None;
            let base = made;
            if ring.push(64, |i| [base + i as i32, 0]) {
                made += 64;
            } else if want >= 0 {
                run.refused += 64;
                made += 64;
            } else {
                made += 64;
            }
        }
        // The host's token, the core's end of periodic frame.
        if in_frame == 50 {
            pending = true; // SOF
            if let Some((n, parity)) = armed
                && parity == frame & 1
            {
                armed = None;
                for &v in &packet {
                    if want < 0 {
                        want = v;
                    }
                    assert_eq!(v, want, "frame lost or repeated");
                    want += 1;
                }
                run.got += n as u64;
            }
        }
        if in_frame == 800 && armed.is_some_and(|(_, p)| p == frame & 1) {
            incomplete = true;
            pending = true;
        }
        // The USB interrupt, when the render lets it.
        if pending && !rendering {
            pending = false;
            if std::mem::take(&mut incomplete)
                && let Some((_, p)) = &mut armed
            {
                *p = frame.wrapping_add(1) & 1;
                run.counts.reaimed += 1;
            }
            if armed.is_none() {
                let fill = ring.len();
                run.fill_max = run.fill_max.max(fill);
                if want >= 0 {
                    run.fill_min = run.fill_min.min(fill);
                }
                let Next { skip, send } =
                    pacer.next(fill, frame.wrapping_add(1) & 0x7FF, &mut run.counts);
                ring.skip(skip);
                if skip > 0 && want >= 0 {
                    want += skip as i32;
                }
                if send > 0 {
                    packet.clear();
                    ring.pop(send, |_, [v, _]| packet.push(v));
                    run.packet_max = run.packet_max.max(send);
                    run.packet_min = run.packet_min.min(send);
                    armed = Some((send, frame.wrapping_add(1) & 1));
                    // An armed packet completes at its token: the interrupt
                    // runs again then.
                }
            }
        }
        if in_frame == 50 && armed.is_none() {
            pending = true; // the completion
        }
        t += STEP;
    }
    run
}

fn clean(r: &Run) {
    assert_eq!(
        (r.counts.short, r.counts.resyncs, r.refused),
        (0, 0, 0),
        "{r:?}"
    );
    assert!(r.fill_max <= HIGH && r.packet_max <= MAX_FRAMES, "{r:?}");
}

/// The SAI's crystal against the host's: ±300 ppm, light and heavy
/// renders, a minute each, nothing lost.
#[test]
fn stream_holds_across_drift_and_load() {
    for (i, ppm) in [-300.0, -100.0, 0.0, 100.0, 300.0].into_iter().enumerate() {
        for (j, load) in [(0.1, 0.3), (0.5, 0.75), (0.6, 0.95)]
            .into_iter()
            .enumerate()
        {
            let r = model(ppm, load, 60, 1 + (i * 3 + j) as u64);
            clean(&r);
            // The heavy render makes the interrupt late for frames: the
            // model reaches the path the pacer makes up for.
            if j == 2 {
                assert!(r.counts.missed > 0, "{r:?}");
            }
            // The host gets the SAI's rate: within a block of the minute's.
            let made = 60.0 * 48_000.0 * (1.0 + ppm * 1e-6);
            assert!((r.got as f64 - made).abs() < 512.0, "{ppm} {load:?}: {r:?}");
        }
    }
}

/// Light load at the nominal rate: the packets stay 47 to 49.
#[test]
fn stream_sizes_stay_near_nominal_when_nothing_is_late() {
    let r = model(100.0, (0.1, 0.3), 30, 7);
    clean(&r);
    assert_eq!(r.counts.missed, 0, "{r:?}");
    assert!(
        r.packet_min >= NOMINAL - 1 && r.packet_max <= NOMINAL + 1,
        "{r:?}"
    );
}

/// An hour at the crystal's worst under a heavy render: the unit's drift
/// test, on the host. `cargo test -p chimera-midi --test uac_test -- --ignored`.
#[test]
#[ignore]
fn stream_holds_for_an_hour() {
    clean(&model(300.0, (0.6, 0.95), 3_600, 99));
    clean(&model(-300.0, (0.6, 0.95), 3_600, 98));
}
