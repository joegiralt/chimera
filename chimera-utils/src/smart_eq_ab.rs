// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `smart-eq-ab`: SMART EQ's A/B on a MODAL pad against a bass (ADR 0296):
//! the full `Instrument`, SMART EQ off, at AMOUNT 64 and at 127. Reports
//! how much the Parts overlap in 250 Hz – 2 kHz (the mud heard in a mix)
//! and what SMART EQ cut while the pad's tail rang. `overlap` is the
//! measure the teaser's vignettes are judged by too.

use chimera_core::dsp::fx_bus::{FxBus, FxParams};
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part_eq::smart::{Amount, Band, Role, SMART_VIEW, SmartEq, Switch};
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use rustfft::num_complex::Complex64;

/// The overlap's band: where the mud is heard.
pub const MUD_LO: f64 = 250.0;
pub const MUD_HI: f64 = 2_000.0;
const N: usize = 4096;

/// How much Parts overlap in 250 Hz – 2 kHz: over Hann frames of `N`
/// (hop `N/2`), each bin's smaller power of every pair where one role
/// outranks the other, summed. `buses` are the Parts' buses after their
/// EQ and SMART EQ, each already at its LEVEL.
pub fn overlap(buses: &[Vec<f32>], roles: &[Role]) -> f64 {
    let fft = crate::sig::plan(N, false);
    let len = buses.iter().map(Vec::len).min().unwrap_or(0);
    let (lo, hi) = (
        (MUD_LO * N as f64 / crate::SR) as usize,
        (MUD_HI * N as f64 / crate::SR) as usize,
    );
    let mut total = 0.0;
    let mut at = 0;
    while at + N <= len {
        let spectra: Vec<Vec<f64>> = buses
            .iter()
            .map(|b| {
                let mut x: Vec<Complex64> = (0..N)
                    .map(|i| {
                        let w =
                            0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos();
                        Complex64::new(b[at + i] as f64 * w, 0.0)
                    })
                    .collect();
                fft.process(&mut x);
                x[lo..hi].iter().map(|c| c.norm_sqr()).collect()
            })
            .collect();
        for (q, sq) in spectra.iter().enumerate() {
            for (p, sp) in spectra.iter().enumerate() {
                if roles[q].depth_over(roles[p]).get() > 0.0 {
                    total += sq.iter().zip(sp).map(|(a, b)| a.min(*b)).sum::<f64>();
                }
            }
        }
        at += N / 2;
    }
    total
}

/// A Part's energy in 250 Hz – 2 kHz.
pub fn mud(bus: &[f32]) -> f64 {
    overlap(&[bus.to_vec(), bus.to_vec()], &[Role::Front, Role::Mid])
}

/// A note at a sample on Part `part`: on (key, velocity) or off (key).
#[derive(Clone, Copy)]
pub struct Note {
    pub at: usize,
    pub part: usize,
    pub key: u8,
    pub on: Option<u8>,
}

/// What a render took: each Part's bus at its LEVEL, and each Part's LOW
/// and LOW MID cut a block.
pub struct Take {
    pub buses: Vec<Vec<f32>>,
    pub cuts: Vec<[[f32; 2]; 2]>,
}

/// `perf` playing `notes` for `len` samples under `smart`.
pub fn render(perf: &mut Performance, notes: &[Note], len: usize, smart: SmartEq) -> Take {
    perf.fx.comp.smart = smart;
    let perf = &*perf;
    let mut knobs = KnobPublisher::new();
    let mut shared = Box::new(AudioShared::from_performance(perf));
    for i in 0..chimera_core::hw::MAX_PARTS {
        let part = perf.part(PartId::ALL[i]);
        shared.publish_sound(i, &part.sound, part.loads(), &mut knobs);
    }
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let gv = GateView::empty();
    let mut ev: Vec<Note> = notes.to_vec();
    ev.sort_by_key(|n| (n.at, n.on.is_some()));
    let parts = 2;
    let mut t = Take {
        buses: (0..parts).map(|_| Vec::with_capacity(len)).collect(),
        cuts: Vec::new(),
    };
    // The view is the audio's one global: a render before this left its own.
    for p in PartId::ALL {
        SMART_VIEW.publish(p, chimera_core::part_eq::smart::CutView::NONE);
    }
    let mut next = 0;
    for b in 0..len.div_ceil(BLOCK_SIZE) {
        while next < ev.len() && ev[next].at < (b + 1) * BLOCK_SIZE {
            let n = ev[next];
            next += 1;
            let kind = match n.on {
                Some(v) => NoteKind::On(Velocity::new(v).expect("a velocity")),
                None => NoteKind::Off,
            };
            let note = NoteEvent {
                channel: perf.part(PartId::ALL[n.part]).mix.channel,
                note: MidiNote::new(n.key).expect("a key"),
                kind,
            };
            inst.handle_with(note, &shared, &gv);
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &gv);
        for (p, bus) in t.buses.iter_mut().enumerate() {
            let level = perf.part(PartId::ALL[p]).mix.level;
            bus.extend(inst.part_bus(p).iter().map(|s| s * level));
        }
        t.cuts.push([0, 1].map(|p| {
            let v = SMART_VIEW.read(PartId::ALL[p]);
            [v.cut(Band::Low).get(), v.cut(Band::LowMid).get()]
        }));
    }
    t
}

/// SMART EQ at `amount`.
pub fn on(amount: u8) -> SmartEq {
    SmartEq::new(Switch::On, Amount::from_code(amount).expect("an AMOUNT"))
}

fn db(on: f64, off: f64) -> f64 {
    10.0 * (on / off).log10()
}

/// The MODAL pad (MID, MODAL's INIT, a long ring) over a bass (FRONT,
/// ALGO's INIT an octave down): two bars of four chords, each struck and
/// let ring, the bass on every beat.
pub fn run(_args: &[String]) -> Result<(), String> {
    let mut perf = Performance::new();
    perf.fx = FxParams::OFF;
    for (p, (engine, role)) in [
        (EngineType::Algo, Role::Front),
        (EngineType::Modal, Role::Mid),
    ]
    .into_iter()
    .enumerate()
    {
        let e = perf.edit(PartId::ALL[p]);
        **e.sound = Sound::init(engine);
        e.mix.role = role;
    }
    let beat = SAMPLE_RATE as usize / 2;
    let mut notes = Vec::new();
    let chords: [[u8; 3]; 4] = [[57, 60, 64], [53, 57, 60], [55, 59, 62], [52, 55, 59]];
    let roots = [45u8, 41, 43, 40];
    for (bar, (chord, root)) in chords.iter().zip(roots).enumerate() {
        let at = bar * 4 * beat;
        for &k in chord {
            notes.push(Note {
                at,
                part: 1,
                key: k,
                on: Some(100),
            });
            notes.push(Note {
                at: at + beat / 4,
                part: 1,
                key: k,
                on: None,
            });
        }
        for i in 0..4 {
            let t = at + i * beat;
            notes.push(Note {
                at: t,
                part: 0,
                key: root - 12,
                on: Some(110),
            });
            notes.push(Note {
                at: t + beat / 2,
                part: 0,
                key: root - 12,
                on: None,
            });
        }
    }
    let len = 16 * beat + 2 * SAMPLE_RATE as usize;
    let roles = [Role::Front, Role::Mid];
    let off = render(&mut perf, &notes, len, SmartEq::OFF);
    let o = overlap(&off.buses, &roles);
    println!("MODAL pad (MID) over a bass (FRONT), 250 Hz - 2 kHz:");
    for a in [64u8, 95, 127] {
        let t = render(&mut perf, &notes, len, on(a));
        let ring: Vec<[f32; 2]> = t.cuts.iter().map(|c| c[1]).collect();
        let peak = ring
            .iter()
            .fold([0.0f32; 2], |m, c| [m[0].max(c[0]), m[1].max(c[1])]);
        // Each chord's last beat: the tail ringing, struck three beats ago.
        let tail: Vec<f32> = (0..4)
            .map(|bar| ring[((bar * 4 + 3) * beat + beat / 2) / BLOCK_SIZE][1])
            .collect();
        println!(
            "  AMOUNT {a:3}: overlap {:+.2} dB; pad {:+.2} dB, bass {:+.2} dB; pad's cut most LOW {:.2} LOW MID {:.2} dB, LOW MID late in each ring {:?}",
            db(overlap(&t.buses, &roles), o),
            db(mud(&t.buses[1]), mud(&off.buses[1])),
            db(mud(&t.buses[0]), mud(&off.buses[0])),
            peak[0],
            peak[1],
            tail.iter()
                .map(|c| (c * 100.0).round() / 100.0)
                .collect::<Vec<_>>(),
        );
    }
    kick_over_bass()
}

/// A DRUM kit (FRONT, its INIT, the kick on every beat at 120 BPM) over a
/// held bass (MID, ALGO's INIT at A1): the kick's transient over the bass
/// in 40–120 Hz, off and on, and the bass's dip.
fn kick_over_bass() -> Result<(), String> {
    use chimera_core::dsp::drum::types::Kit;
    use chimera_core::dsp::drum::view::KitWriter;
    let mut kits = KitWriter::take().ok_or("the kit writer, once")?;
    kits.publish(PartId::ALL[0], &Kit::init());
    let mut perf = Performance::new();
    perf.fx = FxParams::OFF;
    for (p, (engine, role)) in [
        (EngineType::Drum, Role::Front),
        (EngineType::Algo, Role::Mid),
    ]
    .into_iter()
    .enumerate()
    {
        let e = perf.edit(PartId::ALL[p]);
        **e.sound = Sound::init(engine);
        e.mix.role = role;
    }
    let beat = SAMPLE_RATE as usize / 2;
    let mut notes = Vec::new();
    for i in 0..16 {
        notes.push(Note {
            at: i * beat,
            part: 0,
            key: 36,
            on: Some(120),
        });
        notes.push(Note {
            at: i * beat + beat / 4,
            part: 0,
            key: 36,
            on: None,
        });
    }
    notes.push(Note {
        at: 0,
        part: 1,
        key: 33,
        on: Some(100),
    });
    let len = 16 * beat;
    let off = render(&mut perf, &notes, len, SmartEq::OFF);
    let base = kick_to_bass(&off.buses[0], &off.buses[1], &off.buses[0]);
    println!("A kick (DRUM, FRONT) over a held bass (MID), 40 - 120 Hz:");
    println!("  off: the kick's transient {base:+.1} dB over the bass");
    for a in [64u8, 95, 127] {
        let t = render(&mut perf, &notes, len, on(a));
        let r = kick_to_bass(&t.buses[0], &t.buses[1], &off.buses[0]);
        let most = t.cuts.iter().map(|c| c[1][0]).fold(0.0f32, f32::max);
        println!(
            "  AMOUNT {a:3}: {r:+.1} dB ({:+.1}); the bass's LOW dip at most {most:.2} dB, bass 250 Hz - 2 kHz {:+.2} dB",
            r - base,
            db(mud(&t.buses[1]), mud(&off.buses[1])),
        );
    }
    Ok(())
}

/// The kick's band (owner, 2026-11-19): where the kick's dip acts.
const KICK_LO: f64 = 40.0;
const KICK_HI: f64 = 120.0;
/// The window after each kick's onset that is its transient, samples.
const HIT: usize = 2048;
/// Onsets are found in frames of this, samples.
const FRAME: usize = 256;

/// `x` through an RBJ band-pass about √(40·120) Hz spanning 40–120 Hz.
fn kick_band(x: &[f32]) -> Vec<f64> {
    let f0 = (KICK_LO * KICK_HI).sqrt();
    let q = f0 / (KICK_HI - KICK_LO);
    let w = 2.0 * std::f64::consts::PI * f0 / crate::SR;
    let a = w.sin() / (2.0 * q);
    let (b0, b2, a0, a1, a2) = (a, -a, 1.0 + a, -2.0 * w.cos(), 1.0 - a);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    x.iter()
        .map(|&v| {
            let v = v as f64;
            let y = (b0 * v + b2 * x2 - a1 * y1 - a2 * y2) / a0;
            (x2, x1, y2, y1) = (x1, v, y1, y);
            y
        })
        .collect()
}

/// The kick's transient over the bass in 40–120 Hz, dB: at each kick
/// onset (a frame of the kick's band 9 dB over the last, 100 ms apart at
/// least), the kick's band energy over the next 43 ms against the bass's,
/// summed over the onsets. The onsets come from `onsets_of` (the same
/// kicks on and off).
pub fn kick_to_bass(kick: &[f32], bass: &[f32], onsets_of: &[f32]) -> f64 {
    let (k, b, o) = (kick_band(kick), kick_band(bass), kick_band(onsets_of));
    let frames: Vec<f64> = o
        .chunks(FRAME)
        .map(|c| c.iter().map(|v| v * v).sum())
        .collect();
    let loud = frames.iter().cloned().fold(0.0, f64::max) * 1e-3;
    let (mut ek, mut eb, mut last) = (0.0, 0.0, None::<usize>);
    for i in 1..frames.len() {
        let at = i * FRAME;
        let gap = last.is_none_or(|l| at - l > crate::SR as usize / 10);
        if frames[i] > 8.0 * frames[i - 1]
            && frames[i] > loud
            && gap
            && at + HIT <= k.len().min(b.len())
        {
            ek += k[at..at + HIT].iter().map(|v| v * v).sum::<f64>();
            eb += b[at..at + HIT].iter().map(|v| v * v).sum::<f64>();
            last = Some(at);
        }
    }
    10.0 * (ek / eb.max(1e-30)).log10()
}
